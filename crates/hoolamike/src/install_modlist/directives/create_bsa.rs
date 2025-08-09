use {
    super::*,
    crate::{
        modlist_json::directive::create_bsa_directive::CreateBSADirective,
        progress_bars_v2::{count_progress_style, IndicatifWrapIoExt},
        utils::PathReadWrite,
    },
    remapped_inline_file::wabbajack_consts::BSA_CREATION_DIR,
};

#[derive(Clone, Debug)]
pub struct CreateBSAHandler {
    pub output_directory: PathBuf,
}

pub mod fallout_4;
pub mod tes_4;

#[allow(unused_variables)]
fn try_optimize_memory_mapping(memmap: &memmap2::Mmap) {
    #[cfg(unix)]
    if let Err(err) = memmap.advise(memmap2::Advice::Sequential) {
        tracing::trace!(?err, "sequential memory mapping is not supported");
    } else {
        tracing::trace!("memory mapping optimized for sequential access")
    }
}

impl CreateBSAHandler {
    #[tracing::instrument(skip(create_bsa_directive), level = "INFO")]
    pub fn handle(self, create_bsa_directive: CreateBSADirective) -> Result<u64> {
        let Self { output_directory } = self;
        let size = create_bsa_directive.size();
        let span = tracing::Span::current();
        span.in_scope(|| {
            let bsa_creation_dir = output_directory.join(BSA_CREATION_DIR.with(|p| p.to_owned()));
            match create_bsa_directive {
                CreateBSADirective::Ba2(ba2) => self::fallout_4::create_archive(bsa_creation_dir, ba2, |archive, options, output_path| {
                    output_directory
                        .join(output_path.into_path())
                        .open_file_write()
                        .context("opening file for writing")
                        .and_then(|(output_path, output)| {
                            archive
                                .write(&mut tracing::Span::current().wrap_write(size, output), &options)
                                .with_context(|| format!("writing ba2 (fallout 4 / starfield) file to {output_path:?}"))
                        })
                }),
                CreateBSADirective::Bsa(bsa) => self::tes_4::create_archive(bsa_creation_dir, bsa, |archive, options, output_path| {
                    output_directory
                        .join(output_path.into_path())
                        .open_file_write()
                        .context("opening file for writing")
                        .and_then(|(output_path, output)| {
                            archive
                                .write(&mut tracing::Span::current().wrap_write(size, output), &options)
                                .with_context(|| format!("writing bsa file (skyrim and before) to {output_path:?}"))
                        })
                }),
            }
        })
        .map(|_| size)
    }
}
