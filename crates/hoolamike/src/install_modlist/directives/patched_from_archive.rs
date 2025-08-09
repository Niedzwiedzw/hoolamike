use {
    super::*,
    crate::{
        compression::forward_only_seek::ForwardOnlySeek,
        install_modlist::download_cache::to_u64_from_base_64,
        modlist_json::directive::PatchedFromArchiveDirective,
        progress_bars_v2::IndicatifWrapIoExt,
        read_wrappers::ReadExt,
        utils::spawn_rayon,
    },
    preheat_archive_hash_paths::PreheatedArchiveHashPaths,
    std::io::{Read, Seek, Write},
    tracing::Instrument,
    wabbajack_file_handle::WabbajackFileHandle,
};

#[derive(Clone, derivative::Derivative)]
#[derivative(Debug)]
pub struct PatchedFromArchiveHandler {
    #[derivative(Debug = "ignore")]
    pub wabbajack_file: WabbajackFileHandle,
    pub output_directory: PathBuf,
    pub download_summary: DownloadSummary,
}

impl PatchedFromArchiveHandler {
    #[tracing::instrument(skip(self, preheated), level = "INFO")]
    pub fn handle(
        self,
        PatchedFromArchiveDirective {
            hash,
            size,
            to,
            archive_hash_path,
            from_hash: _,
            patch_id,
        }: PatchedFromArchiveDirective,
        preheated: Arc<PreheatedArchiveHashPaths>,
    ) -> Result<u64> {
        let source_file = self
            .download_summary
            .resolve_archive_path(&archive_hash_path)
            .and_then(|path| preheated.get_archive(path))
            .with_context(|| format!("reading archive for [{archive_hash_path:?}]"))?;

        let output_path = self.output_directory.join(to.into_path());

        let wabbajack_file = self.wabbajack_file.clone();
        #[tracing::instrument(skip(source, delta, target), level = "INFO")]
        fn perform_copy<S, D, T>(source: S, delta: D, target: T, expected_size: u64, expected_hash: String) -> Result<()>
        where
            S: Read + Seek,
            D: Read,
            T: Write,
        {
            // this applies delta on the fly
            let from = crate::octadiff_reader::ApplyDetla::new_from_readers(source, ForwardOnlySeek::new(delta))
                .context("invalid delta")?
                .context("delta is empty")?;
            let mut writer = &mut std::io::BufWriter::new(target);
            std::io::copy(
                &mut tracing::Span::current()
                    .wrap_read(expected_size, from)
                    .and_validate_size(expected_size)
                    .and_validate_hash(to_u64_from_base_64(expected_hash)?),
                &mut writer,
            )
            .context("copying file from archive")
            .and_then(|_| writer.flush().context("flushing"))
            .map(|_| ())
        }
        let (_guard, delta_file) = wabbajack_file
            .get_source_data(patch_id)
            .and_then(|source_data| {
                source_data
                    .open_file_read()
                    .map(|(_, file)| (source_data, file))
            })
            .with_context(|| format!("patch {patch_id:?} does not exist"))?;

        source_file
            .open_file_read()
            .and_then(|(final_source_path, mut final_source)| {
                create_file_all(&output_path).and_then(|mut output_file| {
                    perform_copy(&mut final_source, delta_file, &mut output_file, size, hash)
                        .with_context(|| format!("when extracting from [{final_source_path:?}] to [{output_path:?}]"))
                        .with_context(|| format!("when handling [{archive_hash_path:?}] copy"))
                })
            })
            .map(|_| size)
    }
}
