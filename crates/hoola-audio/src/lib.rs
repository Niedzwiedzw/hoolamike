#![feature(iter_collect_into)]
#![allow(clippy::unit_arg)]

use {
    anyhow::{bail, Context, Result},
    clap::{Args, Parser, Subcommand},
    itertools::{repeat_n, Itertools},
    mp3lame_encoder::{Bitrate, DualPcm, MonoPcm},
    num::ToPrimitive,
    rubato::{FastFixedIn, FftFixedOut, PolynomialDegree, Resampler},
    std::{
        convert::identity,
        io::{BufWriter, Write},
        num::{NonZeroU32, NonZeroU8, NonZeroUsize},
        ops::Not,
        path::{Path, PathBuf},
    },
    symphonia::core::{
        audio::{SampleBuffer, SignalSpec},
        codecs::{Decoder, DecoderOptions, CODEC_TYPE_NULL},
        formats::{FormatReader, Packet},
        io::MediaSourceStream,
        probe::{Hint, ProbeResult},
    },
    tap::prelude::*,
    tracing::{debug, info_span, instrument, trace, warn},
    vorbis_rs::VorbisEncoderBuilder,
};

pub mod resampler;

pub mod chunk_while;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Args, Clone, Debug)]
pub struct FromTo {
    /// path to source file
    from: PathBuf,
    /// path to target (output) file
    to: PathBuf,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    ConvertStereoMP3ToMono {
        #[command(flatten)]
        context: FromTo,
    },
    ConvertOGGToWAV(FromTo),
    ResampleOGG {
        #[command(flatten)]
        context: FromTo,
        /// target sample frequency
        #[arg(long)]
        target_frequency: u32,
    },
}

impl Commands {
    pub fn run(self) -> Result<()> {
        let command = self;
        debug!("debug logging on");
        let _span = info_span!("running", ?command).entered();
        match command {
            Commands::ConvertStereoMP3ToMono { context: FromTo { from, to } } => {
                convert_to_mp3(&from, &to, None, Some(44100), Some(Mp3TargetChannelMode::Mono))
            }
            Commands::ConvertOGGToWAV(FromTo { from, to }) => convert_to_wav(&from, &to, None),
            Commands::ResampleOGG {
                context: FromTo { from, to },
                target_frequency,
            } => resample_ogg(&from, &to, target_frequency),
        }
    }
}

#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub struct FormatReaderIterator {
    #[derivative(Debug = "ignore")]
    decoder: Box<dyn Decoder>,
    #[derivative(Debug = "ignore")]
    probe_result: ProbeResult,
    selected_track: u32,
}

impl FormatReaderIterator {
    #[instrument]
    fn from_file(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path).with_context(|| format!("opening file at [{path:?}]"))?;
        let from = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        path.extension()
            .map(|extension| hint.with_extension(&extension.to_string_lossy()));
        let probe_result = symphonia::default::get_probe()
            .format(&hint, from, &Default::default(), &Default::default())
            .context("probing format")?;
        Self::new(probe_result).context("instantiating the decoder iterator")
    }
    #[instrument(skip(probe_result), ret, level = "DEBUG")]
    fn new(probe_result: ProbeResult) -> Result<Self> {
        let track = probe_result
            .format
            .tracks()
            .iter()
            .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
            .context("no track could be decoded")?;
        debug!(
            "selected track [{}]",
            format!("{track:#?}")
                .chars()
                .take(1024)
                .chain(repeat_n('.', 3))
                .collect::<String>()
        );
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .with_context(|| format!("building a decoder for track [{track:#?}]"))?;
        Ok(Self {
            selected_track: track.id,
            probe_result,
            decoder,
        })
    }
}

#[instrument(skip_all, ret, level = "TRACE")]
fn skip_metadata(format: &mut Box<dyn FormatReader>) {
    // Consume any new metadata that has been read since the last packet.
    while !format.metadata().is_latest() {
        // Pop the old head of the metadata queue.
        format.metadata().pop();
    }
}

impl FormatReaderIterator {
    #[instrument(level = "TRACE")]
    fn next_packet(&mut self) -> Result<Option<Packet>> {
        loop {
            skip_metadata(&mut self.probe_result.format);
            match self
                .probe_result
                .format
                .next_packet()
                .tap_err(|message| tracing::debug!(?message, "interpreting error"))
            {
                Ok(packet) => {
                    trace!(
                        packet_dur=%packet.dur,
                        packet_ts=%packet.ts,
                        packet_track_id=%packet.track_id(),
                        "next packet",
                    );
                    // if packet.data.is_empty() {
                    //     tracing::trace!("skipping empty chunk (data len == 0)");
                    //     continue;
                    // }
                    // if packet.dur() == 0 {
                    //     tracing::trace!("skipping empty chunk");
                    //     continue;
                    // }
                    if packet.track_id() == self.selected_track {
                        return Ok(Some(packet));
                    } else {
                        continue;
                    }
                }
                Err(e) => match &e {
                    symphonia::core::errors::Error::IoError(error) => match error.kind() {
                        std::io::ErrorKind::Interrupted => {
                            tracing::warn!("[Interrupted], continuing");
                            continue;
                        }
                        std::io::ErrorKind::UnexpectedEof if e.to_string() == "end of stream" => {
                            tracing::debug!("stream finished");
                            return Ok(None);
                        }

                        message => bail!("{message:#?}"),
                    },
                    symphonia::core::errors::Error::DecodeError(_) => {
                        tracing::warn!("{e:#?}");
                        continue;
                    }
                    symphonia::core::errors::Error::SeekError(_) => bail!("{e:#?}"),
                    symphonia::core::errors::Error::Unsupported(_) => bail!("{e:#?}"),
                    symphonia::core::errors::Error::LimitError(_) => bail!("{e:#?}"),
                    symphonia::core::errors::Error::ResetRequired => bail!("{e:#?}"),
                },
            }
        }
    }
}

#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub struct DecodedChunk {
    spec: SignalSpec,
    /// it contains interleaved data
    #[derivative(Debug = "ignore")]
    sample_buffer: SampleBuffer<f32>,
}

fn split_channels_raw<const COUNT: usize>(samples: &[f32]) -> [impl Iterator<Item = f32> + '_; COUNT] {
    std::array::from_fn(move |channel| samples.iter().skip(channel).step_by(COUNT).copied())
}

impl DecodedChunk {
    /// length of a single channel
    pub fn single_channel_length(&self) -> usize {
        let channel_count = self.spec.channels.count();
        if channel_count == 0 {
            panic!("what? channel count == 0?");
        }
        self.sample_buffer.samples().len() / channel_count
    }

    pub fn split_channels(&self) -> impl Iterator<Item = impl Iterator<Item = f32> + '_> + '_ {
        let channels = self.spec.channels.count();
        (0..channels).map(move |channel| {
            self.sample_buffer
                .samples()
                .iter()
                .skip(channel)
                .step_by(channels)
                .copied()
        })
    }

    #[instrument(level = "DEBUG")]
    pub fn upmix_to_stereo(&self) -> Result<Vec<f32>> {
        let current_mode = Mp3TargetChannelMode::from_count(self.spec.channels.count()).context("checking current rate")?;
        match current_mode {
            Mp3TargetChannelMode::Mono => {
                debug!("converting from mono to stereo makes no sense, but sure here you go");
                self.sample_buffer
                    .samples()
                    .iter()
                    .flat_map(|s| [*s, *s])
                    .collect_vec()
                    .pipe(Ok)
            }
            Mp3TargetChannelMode::Stereo => self.sample_buffer.samples().to_vec().pipe(Ok),
        }
    }
    #[instrument(level = "DEBUG")]
    pub fn downmix_to_mono(&self) -> Result<Vec<f32>> {
        let channel_count = self.spec.channels.count();
        let mut buf: Vec<f32> = Vec::with_capacity(channel_count);
        match channel_count {
            0 => anyhow::bail!("track has 0 channels"),
            1 => self
                .sample_buffer
                .samples()
                .iter()
                .copied()
                .collect_vec()
                .pipe(Ok),
            more => self
                .sample_buffer
                .samples()
                .iter()
                .chunks(more)
                .into_iter()
                .map(|chunk| {
                    buf.clear().pipe(|_| {
                        chunk.collect_into(&mut buf).pipe(|chunk| {
                            chunk
                                .len()
                                .eq(&channel_count)
                                .then_some(chunk)
                                .context("interleaved data does not contain all channels")
                                .map(|chunk| {
                                    chunk
                                        .drain(..)
                                        .map(|sample| sample / (channel_count as f32))
                                        .sum::<f32>()
                                })
                        })
                    })
                })
                .collect::<Result<Vec<f32>>>(),
        }
        .tap_ok(|downmixed| debug!(downmixed_samples = downmixed.len()))
    }
}

impl Iterator for FormatReaderIterator {
    type Item = Result<self::DecodedChunk>;
    #[instrument(level = "trace", ret)]
    fn next(&mut self) -> Option<Self::Item> {
        self.next_packet()
            .context("reading next packet")
            .transpose()
            .map(|packet| {
                packet.and_then(|packet| {
                    trace!("decoding packet");
                    self.decoder
                        .decode(&packet)
                        .context("decoding packet for track")
                        .map(|decoded| {
                            let spec = *decoded.spec();
                            trace!(?spec, "packet decode success");

                            SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec()).pipe(|mut sample_buf| {
                                trace!("copying decoded data into a buffer");
                                sample_buf
                                    .copy_interleaved_ref(decoded)
                                    .pipe(|_| DecodedChunk {
                                        spec,
                                        sample_buffer: sample_buf,
                                    })
                            })
                        })
                })
            })
    }
}

#[extension_traits::extension(trait Mp3LameBuildErrorAnyhowExt)]
impl<T> std::result::Result<T, mp3lame_encoder::BuildError> {
    fn for_anyhow(self) -> Result<T> {
        self.map_err(|e| anyhow::anyhow!("{e:#?}"))
    }
}

// #[extension_traits::extension(trait Mp3LameEncodeErrorAnyhowExt)]
// impl<T> std::result::Result<T, mp3lame_encoder::EncodeError> {
//     fn for_anyhow(self) -> Result<T> {
//         self.map_err(|e| anyhow::anyhow!("{e:#?}"))
//     }
// }

#[derive(Debug, Clone, Copy)]
pub enum Mp3TargetChannelMode {
    Mono,
    Stereo,
}

impl Mp3TargetChannelMode {
    pub fn as_count(self) -> usize {
        match self {
            Mp3TargetChannelMode::Mono => 1,
            Mp3TargetChannelMode::Stereo => 2,
        }
    }
    pub fn from_count(count: usize) -> Result<Self> {
        match count {
            1 => Ok(Self::Mono),
            2 => Ok(Self::Stereo),
            other => Err(anyhow::anyhow!("bad channel count: [{other}], only mono/stereo is supported")),
        }
    }
}

pub fn convert_to_mp3(
    from: &Path,
    to: &Path,
    target_bitrate: Option<u32>,
    target_frequency: Option<u32>,
    target_channel_mode: Option<Mp3TargetChannelMode>,
) -> Result<()> {
    FormatReaderIterator::from_file(from).and_then(|reader| -> Result<_> {
        use mp3lame_encoder::{Builder, FlushNoGap};
        let mut reader = reader
            .filter(|c| {
                c.as_ref()
                    .map(|e| !e.sample_buffer.is_empty())
                    .unwrap_or(true)
            })
            .peekable();
        let (source_sample_rate, source_channel_mode, buffer_size) = reader
            .peek()
            .context("stream is empty")
            .and_then(|e| {
                e.as_ref()
                    .map_err(|e| anyhow::anyhow!("{e:#?}"))
                    .map(|c| (c.spec.rate, c.spec.channels.count(), c.sample_buffer.len()))
            })
            .and_then(|(rate, channels, chunk_size)| Mp3TargetChannelMode::from_count(channels).map(|channels| (rate, channels, chunk_size)))?;

        let target_frequency = target_frequency.unwrap_or(source_sample_rate);

        let target_channel_mode = target_channel_mode.unwrap_or(source_channel_mode);
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(to)
            .with_context(|| format!("opening output file: [{to:?}]"))?
            .pipe(BufWriter::new);
        let buffer_size = buffer_size
            .pipe(NonZeroUsize::new)
            .context("buffer size cannot be 0")?;
        let mut resampler = target_frequency
            .eq(&source_sample_rate)
            .not()
            .then_some(())
            .and_then(|_| {
                BufferedResampler::resampler_from_to(
                    source_sample_rate,
                    target_frequency,
                    buffer_size,
                    source_channel_mode
                        .as_count()
                        .pipe(NonZeroUsize::new)
                        .expect("enum to handle empty channels"),
                )
                .transpose()
            })
            .transpose()
            .context("deducing resampler")?;
        let mut buffer = Vec::new();
        Builder::new()
            .context("creating mp3 lame encoder builder")
            .and_then(|mut encoder| {
                encoder
                    .set_num_channels(target_channel_mode.as_count() as u8)
                    .for_anyhow()
                    .context("set_num_channels")?;
                encoder
                    .set_sample_rate(target_frequency)
                    .for_anyhow()
                    .context("set_sample_rate")?;

                let target_bitrate = target_bitrate
                    .map(|f| match f {
                        8 => Ok(Bitrate::Kbps8),
                        16 => Ok(Bitrate::Kbps16),
                        24 => Ok(Bitrate::Kbps24),
                        32 => Ok(Bitrate::Kbps32),
                        40 => Ok(Bitrate::Kbps40),
                        48 => Ok(Bitrate::Kbps48),
                        64 => Ok(Bitrate::Kbps64),
                        80 => Ok(Bitrate::Kbps80),
                        96 => Ok(Bitrate::Kbps96),
                        112 => Ok(Bitrate::Kbps112),
                        128 => Ok(Bitrate::Kbps128),
                        160 => Ok(Bitrate::Kbps160),
                        192 => Ok(Bitrate::Kbps192),
                        224 => Ok(Bitrate::Kbps224),
                        256 => Ok(Bitrate::Kbps256),
                        320 => Ok(Bitrate::Kbps320),
                        bad_bitrate => Err(anyhow::anyhow!("invalid bitrate: [{bad_bitrate}]")),
                    })
                    .transpose()
                    .context("Reading frequency")?
                    .unwrap_or(Bitrate::Kbps192);

                encoder
                    .set_brate(target_bitrate)
                    .for_anyhow()
                    .context("setting bitrate")?;
                encoder
                    .set_quality(mp3lame_encoder::Quality::Good)
                    .for_anyhow()
                    .context("set quality")?;
                encoder
                    .build()
                    .for_anyhow()
                    .context("building lame encoder")
            })
            .tap_ok(|encoder| {
                tracing::debug!(
                    encoder_sample_rate = encoder.sample_rate(),
                    encoder_num_channels = encoder.num_channels(),
                    "created mp3 lame encoder"
                );
            })
            .and_then(|mut encoder| {
                reader
                    .try_for_each(|chunk| {
                        chunk.and_then(|chunk| -> Result<_> {
                            match (source_channel_mode, target_channel_mode) {
                                (Mp3TargetChannelMode::Mono, Mp3TargetChannelMode::Mono) => match resampler.as_mut() {
                                    Some(resampler) => {
                                        let resampled = resampler
                                            .process(&[chunk.sample_buffer.samples()])
                                            .context("resampling")?;
                                        buffer.reserve(mp3lame_encoder::max_required_buffer_size(chunk.sample_buffer.len()));
                                        encoder
                                            .encode_to_vec(MonoPcm(resampled.first().context("channel mismatch")?), &mut buffer)
                                            .map_err(|e| anyhow::anyhow!("{e:#?}"))
                                            .context("encoding mp3 chunk")
                                            .inspect(|size| debug!("encoded chunk of size [{size}]"))
                                            .and_then(|size| {
                                                output
                                                    .write_all(&buffer)
                                                    .context("writing chunk of encoded mp3 to file")
                                                    .tap_ok(|_| buffer.clear())
                                                    .tap_ok(|_| debug!("wrote [{size}]"))
                                            })
                                    }
                                    None => {
                                        buffer.reserve(mp3lame_encoder::max_required_buffer_size(chunk.sample_buffer.len()));
                                        encoder
                                            .encode_to_vec(MonoPcm(chunk.sample_buffer.samples()), &mut buffer)
                                            .map_err(|e| anyhow::anyhow!("{e:#?}"))
                                            .context("encoding mp3 chunk")
                                            .inspect(|size| debug!("encoded chunk of size [{size}]"))
                                            .and_then(|size| {
                                                output
                                                    .write_all(&buffer)
                                                    .context("writing chunk of encoded mp3 to file")
                                                    .tap_ok(|_| buffer.clear())
                                                    .tap_ok(|_| debug!("wrote [{size}]"))
                                            })
                                    }
                                },
                                (Mp3TargetChannelMode::Stereo, Mp3TargetChannelMode::Stereo) => {
                                    let [left, right] = chunk
                                        .split_channels()
                                        .map(|ch| ch.collect_vec())
                                        .collect_vec()
                                        .try_conv::<[_; 2]>()
                                        .map_err(|bad_size| anyhow::anyhow!("bad size: {bad_size:?}"))
                                        .context("channel size mismatch")?;
                                    match resampler.as_mut() {
                                        Some(resampler) => {
                                            let resampled = resampler.process(&[&left, &right]).context("resampling")?;
                                            buffer.reserve(mp3lame_encoder::max_required_buffer_size(chunk.sample_buffer.len()));
                                            let [left, right] = resampled
                                                .iter()
                                                .collect_vec()
                                                .try_conv::<[_; 2]>()
                                                .map_err(|s| anyhow::anyhow!("{s:?}"))
                                                .context("Bad size")?;
                                            encoder
                                                .encode_to_vec(DualPcm { left, right }, &mut buffer)
                                                .map_err(|e| anyhow::anyhow!("{e:#?}"))
                                                .context("encoding mp3 chunk")
                                                .inspect(|size| debug!("encoded chunk of size [{size}]"))
                                                .and_then(|size| {
                                                    output
                                                        .write_all(&buffer)
                                                        .context("writing chunk of encoded mp3 to file")
                                                        .tap_ok(|_| buffer.clear())
                                                        .tap_ok(|_| debug!("wrote [{size}]"))
                                                })
                                        }
                                        None => {
                                            buffer.reserve(mp3lame_encoder::max_required_buffer_size(chunk.sample_buffer.len()));
                                            encoder
                                                .encode_to_vec(DualPcm { left: &left, right: &right }, &mut buffer)
                                                .map_err(|e| anyhow::anyhow!("{e:#?}"))
                                                .context("encoding mp3 chunk")
                                                .inspect(|size| debug!("encoded chunk of size [{size}]"))
                                                .and_then(|size| {
                                                    output
                                                        .write_all(&buffer)
                                                        .context("writing chunk of encoded mp3 to file")
                                                        .tap_ok(|_| buffer.clear())
                                                        .tap_ok(|_| debug!("wrote [{size}]"))
                                                })
                                        }
                                    }
                                }
                                (Mp3TargetChannelMode::Mono, Mp3TargetChannelMode::Stereo) => {
                                    let stereo = chunk.upmix_to_stereo().context("upmixing to stereo")?;
                                    let [left, right] = split_channels_raw::<2>(stereo.as_slice()).map(|i| i.collect_vec());
                                    match resampler.as_mut() {
                                        Some(resampler) => {
                                            let resampled = resampler.process(&[&left, &right]).context("resampling")?;
                                            buffer.reserve(mp3lame_encoder::max_required_buffer_size(chunk.sample_buffer.len()));
                                            let [left, right] = resampled
                                                .iter()
                                                .collect_vec()
                                                .try_conv::<[_; 2]>()
                                                .map_err(|s| anyhow::anyhow!("{s:?}"))
                                                .context("Bad size")?;
                                            encoder
                                                .encode_to_vec(DualPcm { left, right }, &mut buffer)
                                                .map_err(|e| anyhow::anyhow!("{e:#?}"))
                                                .context("encoding mp3 chunk")
                                                .inspect(|size| debug!("encoded chunk of size [{size}]"))
                                                .and_then(|size| {
                                                    output
                                                        .write_all(&buffer)
                                                        .context("writing chunk of encoded mp3 to file")
                                                        .tap_ok(|_| buffer.clear())
                                                        .tap_ok(|_| debug!("wrote [{size}]"))
                                                })
                                        }
                                        None => {
                                            buffer.reserve(mp3lame_encoder::max_required_buffer_size(chunk.sample_buffer.len()));
                                            encoder
                                                .encode_to_vec(DualPcm { left: &left, right: &right }, &mut buffer)
                                                .map_err(|e| anyhow::anyhow!("{e:#?}"))
                                                .context("encoding mp3 chunk")
                                                .inspect(|size| debug!("encoded chunk of size [{size}]"))
                                                .and_then(|size| {
                                                    output
                                                        .write_all(&buffer)
                                                        .context("writing chunk of encoded mp3 to file")
                                                        .tap_ok(|_| buffer.clear())
                                                        .tap_ok(|_| debug!("wrote [{size}]"))
                                                })
                                        }
                                    }
                                }
                                (Mp3TargetChannelMode::Stereo, Mp3TargetChannelMode::Mono) => {
                                    let chunk = chunk.downmix_to_mono().context("downmixing to mono")?;
                                    match resampler.as_mut() {
                                        Some(resampler) => {
                                            let resampled = resampler
                                                .process(&[chunk.as_slice()])
                                                .context("resampling")?;
                                            buffer.reserve(mp3lame_encoder::max_required_buffer_size(chunk.len()));
                                            encoder
                                                .encode_to_vec(MonoPcm(resampled.first().context("channel mismatch")?), &mut buffer)
                                                .map_err(|e| anyhow::anyhow!("{e:#?}"))
                                                .context("encoding mp3 chunk")
                                                .inspect(|size| debug!("encoded chunk of size [{size}]"))
                                                .and_then(|size| {
                                                    output
                                                        .write_all(&buffer)
                                                        .context("writing chunk of encoded mp3 to file")
                                                        .tap_ok(|_| buffer.clear())
                                                        .tap_ok(|_| debug!("wrote [{size}]"))
                                                })
                                        }
                                        None => {
                                            buffer.reserve(mp3lame_encoder::max_required_buffer_size(chunk.len()));
                                            encoder
                                                .encode_to_vec(MonoPcm(chunk.as_slice()), &mut buffer)
                                                .map_err(|e| anyhow::anyhow!("{e:#?}"))
                                                .context("encoding mp3 chunk")
                                                .inspect(|size| debug!("encoded chunk of size [{size}]"))
                                                .and_then(|size| {
                                                    output
                                                        .write_all(&buffer)
                                                        .context("writing chunk of encoded mp3 to file")
                                                        .tap_ok(|_| buffer.clear())
                                                        .tap_ok(|_| debug!("wrote [{size}]"))
                                                })
                                        }
                                    }
                                } // Some(target) => match target {
                                  //     Mp3TargetChannelMode::Mono => chunk.downmix_to_mono(),
                                  //     Mp3TargetChannelMode::Stereo => chunk.upmix_to_stereo(),
                                  // },
                                  // None => chunk
                                  //     .sample_buffer
                                  //     .samples()
                                  //     .iter()
                                  //     .copied()
                                  //     .collect_vec()
                                  //     .pipe(Ok),
                            }
                        })
                    })
                    .and_then(|_| {
                        encoder
                            .flush_to_vec::<FlushNoGap>(&mut buffer)
                            .map_err(|e| anyhow::anyhow!("{e:#?}"))
                            .context("finalizing the encoder")
                            .and_then(|size| {
                                output
                                    .write_all(&buffer)
                                    .context("writing final chunk to file")
                                    .tap_ok(|_| debug!("wrote [{size}]"))
                            })
                    })
                    .tap_ok(|_| debug!("[DONE]"))
            })
    })
}

pub fn convert_to_wav(from: &Path, to: &Path, target_frequency: Option<u32>) -> Result<()> {
    let track = FormatReaderIterator::from_file(from)
        .and_then(LoadedTrack::from_reader)
        .context("loading track")
        .and_then(|track| match target_frequency {
            Some(target) => track.resample_if_needed(target).context("resampling"),
            None => Ok(track),
        })
        .context("maybe resampling")?;
    let mut writer = hound::WavWriter::create(
        to,
        hound::WavSpec {
            channels: track.channels.len() as _,
            sample_rate: track.sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        }
        .tap(|spec| tracing::trace!(?spec, "creating wav writer with spec")),
    )
    .context("creating WAV writer")?;
    let wrote = track
        .interleaved_samples_iter()
        .try_for_each(|sample| writer.write_sample(sample))
        .context("writing to writer failed");
    wrote.and_then(|_| writer.finalize().context("finalizing the writer"))
}

struct BufferedResampler {
    resampler: FftFixedOut<f32>,
    out_buffers: Vec<Vec<f32>>,
}

impl BufferedResampler {
    fn process<Inner>(&mut self, chunk: &[Inner]) -> Result<&Vec<Vec<f32>>>
    where
        Inner: AsRef<[f32]>,
    {
        // self.buffers.iter_mut().for_each(|b| {
        //     b.clear();
        // });
        //
        self.out_buffers = self.resampler.process(chunk, None)?;
        // let frames = self.resampler.output_frames_next();

        // self.buffers.iter_mut().for_each(|b| {
        //     b.resize(frames, 0.);
        // });
        // let (_, to) = self
        //     .resampler
        //     .process_into_buffer(chunk, &mut self.buffers, None)
        //     .context("resampling chunk")?;
        // self.buffers.iter_mut().for_each(|b| {
        //     b.truncate(to);
        // });
        Ok(&self.out_buffers)
    }
    fn resampler_from_to(from: u32, to: u32, chunk_size: NonZeroUsize, channels: NonZeroUsize) -> Result<Option<Self>> {
        match from == to {
            true => Ok(None),
            false => {
                let resampler = FftFixedOut::new(from as _, to as _, chunk_size.get(), 2, channels.get()).context("Creating sinc interpolation resampler")?;
                Ok(Some(Self {
                    out_buffers: (0..channels.get())
                        .map(|_| vec![0f32; Resampler::output_frames_max(&resampler) + 10])
                        .collect_vec(),
                    resampler,
                }))
            }
        }
    }
}

// fn resamples_from_to(from: u32, to: u32, chunk_size: usize, channels: usize) -> Result<Option<SincFixedIn<f32>>> {
//     match from == to {
//         true => Ok(None),
//         false => Ok(Some(
//             SincFixedIn::new(
//                 to as f32 / from as f32,
//                 2.0,
//                 SincInterpolationParameters {
//                     sinc_len: 256,
//                     f_cutoff: 0.95,
//                     oversampling_factor: 256,
//                     interpolation: SincInterpolationType::Quadratic,
//                     window: rubato::WindowFunction::BlackmanHarris2,
//                 },
//                 chunk_size,
//                 channels,
//             )
//             .context("Creating sinc interpolation resampler")?,
//         )),
//     }
// }

type ChanVec<T> = heapless::Vec<T, 2>;

pub struct LoadedTrack {
    pub channels: ChanVec<Vec<f32>>,
    pub sample_rate: u32,
}

impl std::fmt::Debug for LoadedTrack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedTrack")
            .field("channels", &self.channels.len())
            .field("sample_rate", &self.sample_rate)
            .finish_non_exhaustive()
    }
}

#[extension_traits::extension(trait HeaplessVecTryCollectExt)]
impl<const N: usize, T> heapless::Vec<T, N>
where
    Self: Sized,
{
    fn try_from_iter<I: Iterator<Item = T>>(mut iter: I) -> Result<Self> {
        iter.try_fold(Self::new(), |mut acc, next| {
            acc.push(next)
                .map_err(|_| anyhow::anyhow!("max capacity [{N}] reached, could not push another element"))
                .map(|_| acc)
        })
    }
}

impl LoadedTrack {
    #[instrument(level = "TRACE", skip(reader), ret)]
    pub fn from_reader(reader: FormatReaderIterator) -> Result<Self> {
        trace!("loading from reader");
        let mut reader = reader
            .filter(|c| {
                c.as_ref()
                    .map(|e| !e.sample_buffer.is_empty())
                    .unwrap_or(true)
            })
            .peekable();
        let (original_rate, original_channel_count, _sample_buffer_size) = reader
            .peek()
            .map(|r| {
                r.as_ref()
                    .map_err(|e| anyhow::anyhow!("{e:#?}"))
                    .map(|r| (r.spec.rate, r.spec.channels.count(), r.sample_buffer.len()))
            })
            .context("input is empty?")
            .and_then(identity)
            .context("deducing original metadata")?;
        reader
            .try_fold(Self::empty(original_rate, original_channel_count), |mut acc, next| {
                next.map(|next| acc.load_interleaved(next.sample_buffer.samples()))
                    .map(|_| acc)
            })
            .context("loading raw track")
    }

    pub fn iter_chunks(&self, size: usize) -> impl Iterator<Item = ChanVec<&[f32]>> + '_ {
        self.channels[0]
            .chunks(size)
            .enumerate()
            .map(move |(start_idx, chunk)| (size * start_idx, chunk))
            .map(|(start, chunk)| (start, start + chunk.len()))
            .map(|(start, end)| {
                self.channels
                    .iter()
                    .enumerate()
                    .map(|(channel_index, ch)| {
                        ch.get(start..end).unwrap_or_else(|| {
                            tracing::warn!(%channel_index, %start, %end, "channel is shorter than first channel?");
                            ch.get(0..0).expect("come on")
                        })
                    })
                    .pipe(ChanVec::try_from_iter)
                    .expect("max channel count to be 2")
            })
    }

    #[instrument(level = "TRACE", ret)]
    pub fn resample_if_needed(self, target_sample_rate: u32) -> Result<Self> {
        if self.sample_rate == target_sample_rate {
            Ok(self)
        } else {
            self.resample(target_sample_rate)
        }
    }

    pub fn interleaved_samples_iter(&self) -> impl Iterator<Item = f32> + '_ {
        (0..(self.channels[0].len())).flat_map(move |sample_idx| (0..self.channels.len()).map(move |ch| self.channels[ch][sample_idx]))
    }

    #[instrument(level = "TRACE", ret)]
    pub fn resample(self, target_sample_rate: u32) -> Result<Self> {
        fn append_frames(buffers: &mut [Vec<f32>], additional: &[Vec<f32>], nbr_frames: usize) -> Result<()> {
            buffers
                .iter_mut()
                .zip(additional.iter())
                .try_for_each(|(b, a)| -> Result<()> {
                    b.extend_from_slice(
                        a.get(..nbr_frames)
                            .with_context(|| format!("bad slice: [..{nbr_frames}]"))?,
                    );
                    Ok(())
                })
        }

        let mut resampled_track = Self::empty(target_sample_rate, self.channels.len());

        let f_ratio = target_sample_rate as f32 / self.sample_rate as f32;
        // let params = SincInterpolationParameters {
        //     sinc_len: 256,
        //     f_cutoff: 0.95,
        //     interpolation: SincInterpolationType::Linear,
        //     oversampling_factor: 256,
        //     window: WindowFunction::BlackmanHarris2,
        // };
        let mut resampler =
            FastFixedIn::<f32>::new(f_ratio as f64, 2.0, PolynomialDegree::Septic, 1024, self.channels.len()).context("creating fastfft resampler")?;

        let mut input_frames_next = resampler.input_frames_next();
        let resampler_delay = resampler.output_delay();
        let mut outbuffer = vec![vec![0.0f32; resampler.output_frames_max()]; self.channels.len()];
        let mut indata_slices: Vec<&[f32]> = self.channels.iter().map(|v| v.as_slice()).collect();
        while indata_slices[0].len() >= input_frames_next {
            let (nbr_in, nbr_out) = resampler
                .process_into_buffer(&indata_slices, &mut outbuffer, None)
                .context("processing into buffer")?;
            for chan in indata_slices.iter_mut() {
                *chan = chan
                    .get(nbr_in..)
                    .with_context(|| format!("invalid slice: [{nbr_in}..]"))?;
            }
            append_frames(&mut resampled_track.channels, &outbuffer, nbr_out)?;
            input_frames_next = resampler.input_frames_next();
        }

        // Process a partial chunk with the last frames.
        if !indata_slices[0].is_empty() {
            let (_nbr_in, nbr_out) = resampler
                .process_partial_into_buffer(Some(&indata_slices), &mut outbuffer, None)
                .context("processing partial into buffer")?;
            append_frames(&mut resampled_track.channels, &outbuffer, nbr_out)?;
        }
        resampled_track.channels.iter_mut().for_each(|channel| {
            channel
                .drain(0..(resampler_delay.min(channel.len())))
                .enumerate()
                .for_each(|sample| trace!(%resampler_delay, ?sample,  "dropping sample"))
        });
        Ok(resampled_track)
    }
    pub fn empty(sample_rate: u32, channels: usize) -> Self {
        Self {
            channels: (0..channels)
                .map(|_| Default::default())
                .pipe(ChanVec::try_from_iter)
                .expect("max channel count to be 2"),
            sample_rate,
        }
    }

    pub fn load_channel(&mut self, channel: usize, data: &[f32]) {
        self.channels[channel].extend_from_slice(data);
    }

    pub fn load_interleaved(&mut self, interleaved: &[f32]) {
        let channel_count = self.channels.len();
        (0..channel_count).for_each(|channel| {
            self.channels[channel].extend(
                interleaved
                    .iter()
                    .skip(channel)
                    .step_by(channel_count)
                    .copied(),
            );
        })
    }
}

pub fn resample_ogg(from: &Path, to: &Path, target_frequency: u32) -> Result<()> {
    let track = FormatReaderIterator::from_file(from)
        .context("opening source file")
        .and_then(LoadedTrack::from_reader)?
        .resample_if_needed(target_frequency)?;

    const REASONABLE_OGG_BLOCK_SIZE: usize = 2048;

    let mut output = std::fs::File::create(to)
        .context("opening output file for writing")?
        .pipe(BufWriter::new);
    let mut encoder = info_span!("building_vobis_encoder").in_scope(|| -> Result<_> {
        VorbisEncoderBuilder::new(
            target_frequency
                .pipe(NonZeroU32::new)
                .context("zero sampling frequency?")
                .tap_ok(|target_frequency| debug!(%target_frequency))?,
            track
                .channels
                .len()
                .to_u8()
                .context("too many channels (max is 255)")
                .and_then(|channels| NonZeroU8::new(channels).context("no channels"))
                .context("validating input channels")
                .tap_ok(|target_channel_count| debug!(%target_channel_count))?,
            &mut output,
        )
        .context("crating vorbis encoder builder")
        .and_then(|mut e| e.build().context("finalizing vorbis encoder"))
        .context("creating vorbis encoder")
    })?;

    let reencoded = track
        .iter_chunks(REASONABLE_OGG_BLOCK_SIZE)
        .try_for_each(|chunk| {
            encoder
                .encode_audio_block(&chunk)
                .context("encoding sample")?;

            Ok(())
        });
    reencoded
        .and_then(|_| encoder.finish().context("finalizing encoder"))
        .and_then(|w| w.flush().context("flushing the output"))
        .with_context(|| format!("resampling [{from:?}] -> [{to:?}]"))
}
