use {
    super::*,
    crate::{
        modlist_json::{directive::TransformedTextureDirective, ImageState},
        progress_bars_v2::IndicatifWrapIoExt,
    },
    preheat_archive_hash_paths::PreheatedArchiveHashPaths,
    proton_wrapper::{Initialized, ProtonContext},
    std::io::{Read, Write},
    tracing::warn,
};

#[derive(Debug, Clone)]
pub struct TexconvProtonState {
    pub texconv_path: PathBuf,
    pub proton_prefix_state: Arc<Initialized<ProtonContext>>,
}

#[derive(Clone, derivative::Derivative)]
#[derivative(Debug)]
pub struct TransformedTextureHandler {
    pub output_directory: PathBuf,
    #[derivative(Debug = "ignore")]
    pub download_summary: DownloadSummary,
    pub texconv_proton_state: Option<TexconvProtonState>,
}

#[extension_traits::extension(pub trait IoResultValidateSizeExt)]
impl std::io::Result<u64> {
    fn and_validate_size(self, expected_size: u64) -> anyhow::Result<u64> {
        self.context("performing read").and_then(|size| {
            size.eq(&expected_size)
                .then_some(size)
                .with_context(|| format!("expected [{expected_size} bytes], but [{size} bytes] was read"))
        })
    }
}

// #[cfg(feature = "dds_recompression")]
mod dds_recompression;
mod dds_recompression_directx_tex;
mod dds_recompression_texconv_proton;

#[cfg(feature = "intel_tex")]
mod dds_recompression_intel_tex;

impl TransformedTextureHandler {
    #[instrument(skip(self, preheated))]
    pub fn handle(
        self,
        TransformedTextureDirective {
            hash,
            size,
            image_state:
                ImageState {
                    format,
                    height,
                    mip_levels,
                    perceptual_hash: _,
                    width,
                },
            to,
            archive_hash_path,
        }: TransformedTextureDirective,
        preheated: Arc<PreheatedArchiveHashPaths>,
    ) -> Result<u64> {
        let handle = tracing::Span::current();
        // let _image_dds_format = supported_image_format(format).context("checking for format support")?;
        let output_path = self.output_directory.join(to.into_path());
        let source_file = self
            .download_summary
            .resolve_archive_path(&archive_hash_path)
            .and_then(|path| preheated.get_archive(path))
            .with_context(|| format!("reading archive for [{archive_hash_path:?}]"))?;

        handle
            .in_scope(|| {
                let perform_copy = {
                    move |from: &mut dyn Read, to: &mut dyn Write, target_path: PathBuf| {
                        info_span!("perform_copy").in_scope(|| {
                            let mut writer = to;
                            let mut reader = tracing::Span::current().wrap_read(size, from);
                            Err(anyhow::anyhow!("trying multiple algorithms"))
                                .or_else(|reason| {
                                    self.texconv_proton_state
                                        .as_ref()
                                        .context("texconv+proton not set up, gonna try slow methods")
                                        .and_then(
                                            |TexconvProtonState {
                                                 texconv_path,
                                                 proton_prefix_state,
                                             }| {
                                                dds_recompression_texconv_proton::resize_dds(
                                                    &mut reader,
                                                    width,
                                                    height,
                                                    format,
                                                    mip_levels,
                                                    &mut writer,
                                                    texconv_path,
                                                    proton_prefix_state.as_ref(),
                                                )
                                                .with_context(|| format!("tried because: {reason:?}"))
                                            },
                                        )
                                })
                                .pipe(|r| {
                                    #[cfg(feature = "intel_tex")]
                                    {
                                        r.or_else(|e| {
                                            dds_recompression_intel_tex::resize_dds(&mut reader, width, height, format, mip_levels, &mut writer)
                                                .map(|_| size)
                                                .with_context(|| format!("tried because: {e:?}"))
                                        })
                                    }
                                    #[cfg(not(feature = "intel_tex"))]
                                    {
                                        r
                                    }
                                })
                                // .or_else(|e| {
                                //     warn!("intel texture recompression (fast) failed, falling back to microsoft directxtex (slow)\nreason:\n{e:?}");
                                //     dds_recompression_directx_tex::resize_dds(&mut reader, width, height, format, mip_levels, &mut writer)
                                //         .with_context(|| format!("tried because: {e:?}"))
                                // })
                                .and_then(|wrote| {
                                    wrote
                                        .eq(&size)
                                        .then_some(size)
                                        .with_context(|| format!("expected output size to be [{size} bytes], but got [{wrote} bytes]"))
                                })
                                .context("copying file from archive")
                                .and_then(|_| writer.flush().context("flushing write"))
                                .with_context(|| format!("writing to [{target_path:?}]"))
                                .map(|_| ())
                        })
                    }
                };

                source_file
                    .open_file_read()
                    .and_then(|(source_path, mut final_source)| {
                        create_file_all(&output_path).and_then(|mut output_file| {
                            perform_copy(&mut final_source, &mut output_file, output_path.clone())
                                // .or_else(|reason| {
                                //     let _span =
                                //         tracing::error_span!("could not resize texture, copying the original", reason = %format!("{reason:?}")).entered();
                                //     tracing::error!("could not resize the file, but it should still work");
                                //     final_source
                                //         .rewind()
                                //         .context("rewinding original file")
                                //         .map(|_| final_source)
                                //         .and_then(|final_source| {
                                //             output_path.open_file_write().and_then(|(_, mut output)| {
                                //                 std::io::copy(&mut tracing::Span::current().wrap_read(size, final_source), &mut output)
                                //                     .with_context(|| format!("writing original because resizing could not be performed due to: {reason:?}"))
                                //             })
                                //         })
                                //         .map(|_| ())
                                // })
                                .with_context(|| format!("when extracting from [{source_path:?}]({:?}) to [{}]", archive_hash_path, output_path.display()))
                        })
                    })?;
                Ok(())
            })
            .map(|_| size)
    }
}
