use {
    super::{
        LazyArchiveChunk, RepackingContext, SeekWithTempFileExt,
        manifest_file::asset::{Asset, CopyAsset, LocationIndex, NewAsset, PatchAsset},
    },
    crate::{
        compression::preheated_archive::PreheatedArchive,
        utils::{ExistingPathRead, ReadableCatchUnwindExt, with_scoped_temp_path},
    },
    anyhow::{Context, Result},
    hoola_audio::Mp3TargetChannelMode,
    std::{collections::BTreeMap, io::BufReader, sync::Arc},
    tracing::instrument,
};

#[derive(Clone)]
pub struct AssetContext {
    pub preheated_mpi_file: Arc<PreheatedArchive>,
    pub repacking_context: RepackingContext,
    pub preheated: Arc<BTreeMap<LocationIndex, PreheatedArchive>>,
}

impl AssetContext {
    #[instrument(skip(self))]
    pub fn handle_asset(self, asset: Asset) -> Result<Option<LazyArchiveChunk>> {
        let Self { preheated_mpi_file, .. } = self.clone();
        match asset {
            Asset::New(NewAsset {
                tags: _,
                status: _,
                source,
                target,
            }) => {
                let target = target.lookup_from_both_source_and_target(&source);
                preheated_mpi_file
                    .paths
                    .get(&source.path.0)
                    .with_context(|| format!("no [{source:?}] in mpi file"))
                    .and_then(|path| path.open_file_read())
                    .and_then(|(_, handle)| target.insert_into(self.repacking_context.clone(), &mut BufReader::new(handle)))
                    .with_context(|| format!("handling with target: {target:?}"))
            }
            Asset::Copy(CopyAsset {
                tags: _,
                status: _,
                source,
                target,
            }) => {
                let target = target.lookup_from_both_source_and_target(&source);
                source
                    .into_reader(self.clone())
                    .context("building source")
                    .and_then(|mut source| {
                        target
                            .insert_into(self.repacking_context.clone(), &mut source)
                            .context("performing move")
                    })
            }
            Asset::Patch(PatchAsset {
                tags: _,
                status: _,
                source,
                target,
            }) => {
                let target = target.lookup_from_both_source_and_target(&source);
                preheated_mpi_file
                    .paths
                    .get(&target.path.0)
                    .with_context(|| format!("no [{source:?}] in mpi file"))
                    .context("reading patch file")
                    .and_then(|patch_file| {
                        source
                            .into_reader(self.clone())
                            .and_then(|reader| reader.seek_with_temp_file_blocking_raw(0))
                            .map(|(_, file)| file)
                            .context("reading source file")
                            .and_then(|source_file| {
                                with_scoped_temp_path(|output_buffer| {
                                    std::panic::catch_unwind(|| xdelta::decode_file(Some(&source_file), patch_file, output_buffer))
                                        .for_anyhow()
                                        .context("decoding xdelta patch")
                                        .map(|_| output_buffer)
                                        .and_then(|patched_file| {
                                            patched_file
                                                .open_file_read()
                                                .and_then(|(_, mut file)| target.insert_into(self.repacking_context.clone(), &mut file))
                                        })
                                })
                            })
                    })
            }
            Asset::XwmaFuz(_xwma_fuz_asset) => Err(anyhow::anyhow!(" not implemented")),
            Asset::OggEnc2(ogg_enc_asset) => {
                let target = ogg_enc_asset
                    .target
                    .clone()
                    .lookup_from_both_source_and_target(&ogg_enc_asset.source);
                ogg_enc_asset
                    .params
                    .parse()
                    .context("bad params")
                    .and_then(|mut params| {
                        let target_frequency = params
                            .remove("f")
                            .context("no 'f' param (frequency)")
                            .context("frequency reading ogg encoder params")
                            .and_then(|f| {
                                f.parse::<u32>()
                                    .with_context(|| format!("'{f}' is not a valid frequency"))
                            })?;
                        if let Some(quality) = params.remove("q") {
                            tracing::debug!(%quality, "found quality param, byt it cannot currently be parametrized");
                        }

                        if !params.is_empty() {
                            anyhow::bail!("leftover params: {params:#?}");
                        }
                        ogg_enc_asset
                            .source
                            .into_reader(self.clone())
                            .and_then(|source| {
                                source
                                    .seek_with_temp_file_blocking_raw(0)
                                    .and_then(|(_, source)| {
                                        with_scoped_temp_path(|buffer| {
                                            hoola_audio::resample_ogg(&source, buffer, target_frequency).and_then(|_| {
                                                buffer
                                                    .open_file_read()
                                                    .and_then(|(_, mut buffer)| target.insert_into(self.repacking_context.clone(), &mut buffer))
                                            })
                                        })
                                    })
                            })
                    })
            }
            Asset::AudioEnc(audio_enc) => {
                let target = audio_enc
                    .target
                    .clone()
                    .lookup_from_both_source_and_target(&audio_enc.source);
                let target_path = target.path.0.clone();
                audio_enc
                    .params
                    .parse()
                    .context("bad params")
                    .and_then(|mut params| {
                        let target_frequency = params
                            .remove("f")
                            .map(|f| {
                                f.parse::<u32>()
                                    .with_context(|| format!("'{f}' is not a valid frequency"))
                            })
                            .transpose()?;

                        let target_format = params.remove("fmt").map(ToOwned::to_owned);
                        let target_channel_mode = params
                            .remove("c")
                            .map(|c| {
                                c.parse::<usize>()
                                    .with_context(|| format!("'{c}' is not a valid channel number"))
                                    .and_then(Mp3TargetChannelMode::from_count)
                            })
                            .transpose()
                            .context("reading channel mode")?;

                        let target_bitrate = params
                            .remove("b")
                            .map(|b| {
                                b.parse::<u32>()
                                    .with_context(|| format!("'{b}' is not a valid bitrate"))
                            })
                            .transpose()?;

                        if let Some(quality) = params.remove("q") {
                            tracing::warn!(%quality, "found quality param, byt it cannot currently be parametrized");
                        }

                        if !params.is_empty() {
                            anyhow::bail!("leftover params: {params:#?}");
                        }

                        let target_extension = target_path
                            .extension()
                            .context("target file has no extension")?
                            .to_lowercase();
                        if let Some(target_format) = target_format
                            && target_format != target_extension
                        {
                            anyhow::bail!("specified format [{target_format}], but extension is [{target_extension}]")
                        }

                        audio_enc
                            .source
                            .into_reader(self.clone())
                            .and_then(|source| {
                                source
                                    .seek_with_temp_file_blocking_raw(0)
                                    .and_then(|(_, source)| {
                                        with_scoped_temp_path(|buffer| {
                                            (match target_extension.as_str() {
                                                "wav" => hoola_audio::convert_to_wav(&source, buffer, target_frequency)
                                                    .context("converting to wav")
                                                    .map(|_| buffer),
                                                "mp3" => hoola_audio::convert_to_mp3(&source, buffer, target_bitrate, target_frequency, target_channel_mode)
                                                    .context("converting to mp3")
                                                    .map(|_| buffer),
                                                other => Err(anyhow::anyhow!("extension [.{other}] is not supported by hoolamike, file an issue")),
                                            })
                                            .and_then(|buffer| {
                                                buffer
                                                    .open_file_read()
                                                    .and_then(|(_, mut file)| target.insert_into(self.repacking_context.clone(), &mut file))
                                            })
                                        })
                                    })
                            })
                    })
            }
        }
    }
}
