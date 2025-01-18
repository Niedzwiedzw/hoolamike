#![feature(iter_collect_into)]
#![allow(clippy::unit_arg)]

use {
    anyhow::{bail, Context, Result},
    chunk_while::IteratorChunkWhileExt,
    clap::{Args, Parser, Subcommand},
    itertools::Itertools,
    mp3lame_encoder::MonoPcm,
    num::ToPrimitive,
    std::{
        convert::identity,
        io::{BufWriter, Write},
        iter::repeat,
        num::{NonZeroU32, NonZeroU8},
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
    tracing::{debug, info, info_span, instrument, warn},
    vorbis_rs::VorbisEncoderBuilder,
};

pub mod chunk_while;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Args, Clone, Debug)]
struct FromTo {
    /// path to source file
    from: PathBuf,
    /// path to target (output) file
    to: PathBuf,
}

#[derive(Subcommand, Debug)]
enum Commands {
    ConvertStereoMP3ToMono(FromTo),
    ConvertOGGToWAV(FromTo),
    ResampleOGG {
        #[command(flatten)]
        context: FromTo,
        /// target sample frequency
        #[arg(long)]
        target_frequency: u32,
    },
}

#[derive(derivative::Derivative)]
#[derivative(Debug)]
struct FormatReaderIterator {
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
    #[instrument(skip(probe_result), ret)]
    fn new(probe_result: ProbeResult) -> Result<Self> {
        let track = probe_result
            .format
            .tracks()
            .iter()
            .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
            .context("no track could be decoded")?;
        info!(
            "selected track [{}]",
            format!("{track:#?}")
                .chars()
                .take(1024)
                .chain(repeat('.').take(3))
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

#[instrument(skip_all, ret, level = "DEBUG")]
fn skip_metadata(format: &mut Box<dyn FormatReader>) {
    // Consume any new metadata that has been read since the last packet.
    while !format.metadata().is_latest() {
        // Pop the old head of the metadata queue.
        format.metadata().pop();
    }
}

impl FormatReaderIterator {
    #[instrument(level = "DEBUG")]
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
                    debug!(
                        packet_dur=%packet.dur,
                        packet_ts=%packet.ts,
                        packet_track_id=%packet.track_id(),
                        "next packet",
                    );
                    if packet.dur() == 0 {
                        tracing::warn!("skipping empty chunk");
                        continue;
                    }
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
                            tracing::info!("stream finished");
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
struct DecodedChunk {
    spec: SignalSpec,
    /// it contains interleaved data
    #[derivative(Debug = "ignore")]
    sample_buffer: SampleBuffer<f32>,
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

    pub fn split_channels(&self) -> impl Iterator<Item = impl Iterator<Item = &f32> + '_> + '_ {
        let channels = self.spec.channels.count();
        (0..channels).map(move |channel| {
            self.sample_buffer
                .samples()
                .iter()
                .skip(channel)
                .step_by(channels)
        })
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
    #[instrument(level = "DEBUG", ret)]
    fn next(&mut self) -> Option<Self::Item> {
        self.next_packet()
            .context("reading next packet")
            .transpose()
            .map(|packet| {
                packet.and_then(|packet| {
                    debug!("decoding packet");
                    self.decoder
                        .decode(&packet)
                        .context("decoding packet for track")
                        .map(|decoded| {
                            let spec = *decoded.spec();
                            debug!(?spec, "packet decode success");

                            SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec()).pipe(|mut sample_buf| {
                                debug!("copying decoded data into a buffer");
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

const SAMPLE_RATE: u32 = 44_100;

fn setup_logging() {
    use tracing_subscriber::{prelude::*, EnvFilter};
    let subscriber = tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or(EnvFilter::from("info")))
        .with(tracing_subscriber::fmt::Layer::new().with_writer(std::io::stderr));
    if let Err(message) = tracing::subscriber::set_global_default(subscriber) {
        eprintln!("logging setup failed: {message:?}");
    }
}

pub fn resample_ogg(from: &Path, to: &Path, target_frequency: u32) -> Result<()> {
    let mut reader = FormatReaderIterator::from_file(&from)
        .context("opening source file")?
        .peekable();
    let (original_rate, original_channel_count) = reader
        .peek()
        .map(|r| {
            r.as_ref()
                .map_err(|e| anyhow::anyhow!("{e:#?}"))
                .map(|r| (r.spec.rate, r.spec.channels.count()))
        })
        .context("input is empty?")
        .and_then(identity)
        .context("deducing original metadata")?;
    let _source_span = info_span!("with_source_info", %original_rate, %original_channel_count).entered();
    let mut output = std::fs::File::create(&to)
        .context("opening output file for writing")?
        .pipe(BufWriter::new);
    let mut encoder = info_span!("building_vobis_encoder").in_scope(|| -> Result<_> {
        VorbisEncoderBuilder::new(
            target_frequency
                .pipe(NonZeroU32::new)
                .context("zero sampling frequency?")
                .tap_ok(|target_frequency| info!(%target_frequency))?,
            original_channel_count
                .to_u8()
                .context("too many channels (max is 255)")
                .and_then(|channels| NonZeroU8::new(channels).context("no channels"))
                .context("validating input channels")
                .tap_ok(|target_channel_count| info!(%target_channel_count))?,
            &mut output,
        )
        .context("crating vorbis encoder builder")
        .and_then(|mut e| e.build().context("finalizing vorbis encoder"))
        .context("creating vorbis encoder")
    })?;
    let mut buffers = (0..original_channel_count)
        .map(|_| Vec::new())
        .collect_vec();
    const REASONABLE_OGG_BLOCK_SIZE: usize = 2048;
    reader
        .chunk_while(|chunk| {
            chunk
                .iter()
                .map(|c| {
                    c.as_ref()
                        .map(|c| c.single_channel_length())
                        .unwrap_or_default()
                })
                .sum::<usize>()
                < REASONABLE_OGG_BLOCK_SIZE
        })
        .try_for_each(|chunk| {
            buffers.iter_mut().for_each(|b| b.clear());
            chunk
                .into_iter()
                .collect::<Result<Vec<_>>>()
                .map(|chunk| {
                    chunk.into_iter().for_each(|chunk| {
                        chunk
                            .split_channels()
                            .zip(buffers.iter_mut())
                            .for_each(|(channel, buffer)| {
                                buffer.extend(channel.copied());
                            });
                    })
                })
                .and_then(|_| {
                    tracing::debug!(
                        samples = buffers
                            .first()
                            .as_ref()
                            .map(|b| b.len())
                            .unwrap_or_default(),
                        "wrote to buffer"
                    );
                    encoder
                        .encode_audio_block(&buffers)
                        .context("encoding sample")
                })
        })
        .and_then(|_| encoder.finish().context("finalizing encoder"))
        .and_then(|w| w.flush().context("flushing the output"))
        .with_context(|| format!("resampling [from:?] -> [{to:?}]"))
}

fn main() -> Result<()> {
    setup_logging();
    let Cli { command } = Cli::parse();

    debug!("debug logging on");
    let _span = info_span!("running", ?command).entered();
    match command {
        Commands::ConvertStereoMP3ToMono(FromTo { from, to }) => FormatReaderIterator::from_file(&from).and_then(|mut reader| -> Result<_> {
            use mp3lame_encoder::{Builder, FlushNoGap};

            let mut output = std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .create(true)
                .open(&to)
                .with_context(|| format!("opening output file: [{to:?}]"))?
                .pipe(BufWriter::new);

            let mut buffer = Vec::new();
            Builder::new()
                .context("creating mp3 lame encoder builder")
                .and_then(|mut encoder| {
                    encoder
                        .set_num_channels(1)
                        .for_anyhow()
                        .context("set_num_channels")?;
                    encoder
                        .set_sample_rate(SAMPLE_RATE)
                        .for_anyhow()
                        .context("set_sample_rate")?;
                    encoder
                        .set_brate(mp3lame_encoder::Bitrate::Kbps192)
                        .for_anyhow()
                        .context("setting bitrate")?;
                    encoder
                        .set_quality(mp3lame_encoder::Quality::Best)
                        .for_anyhow()
                        .context("set quality")?;
                    encoder
                        .build()
                        .for_anyhow()
                        .context("building lame encoder")
                })
                .tap_ok(|encoder| {
                    tracing::info!(
                        encoder_sample_rate = encoder.sample_rate(),
                        encoder_num_channels = encoder.num_channels(),
                        "created mp3 lame encoder"
                    );
                })
                .and_then(|mut encoder| {
                    reader
                        .try_for_each(|chunk| {
                            chunk
                                .and_then(|chunk| chunk.downmix_to_mono())
                                .and_then(|chunk| {
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
                        .tap_ok(|_| info!("[DONE]"))
                })
        }),
        Commands::ConvertOGGToWAV(FromTo { from, to }) => FormatReaderIterator::from_file(&from).and_then(|reader| {
            let mut reader = reader.peekable();
            let (source_sample_rate, channel_count) = reader
                .peek()
                .map(|chunk| {
                    chunk
                        .as_ref()
                        .map_err(|e| anyhow::anyhow!("{e:#?}"))
                        .map(|c| (c.spec.rate, c.spec.channels.count()))
                })
                .context("input is empty")
                .and_then(identity)
                .context("deducing source spec")?;
            let mut writer = hound::WavWriter::create(
                &to,
                hound::WavSpec {
                    channels: channel_count as _,
                    sample_rate: source_sample_rate,
                    bits_per_sample: 32,
                    sample_format: hound::SampleFormat::Float,
                }
                .tap(|spec| tracing::debug!(?spec, "creating wav writer with spec")),
            )
            .context("creating WAV writer")?;
            reader
                .try_for_each(|chunk| {
                    chunk.and_then(|chunk| {
                        chunk
                            .sample_buffer
                            .samples()
                            .iter()
                            .try_for_each(|s| writer.write_sample(*s).context("wrtigin sample"))
                    })
                })
                .context("writing reencoded wav data")
                .and_then(|_| writer.finalize().context("finalizing the writer"))?;
            info!("[DONE]");
            Ok(())
        }),
        Commands::ResampleOGG {
            context: FromTo { from, to },
            target_frequency,
        } => resample_ogg(&from, &to, target_frequency),
    }
}
