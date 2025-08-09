use {
    super::*,
    crate::{modlist_json::directive::InlineFileDirective, progress_bars_v2::IndicatifWrapIoExt, utils::spawn_rayon},
    std::io::Write,
    wabbajack_file_handle::WabbajackFileHandle,
};

#[derive(Clone, Debug)]
pub struct InlineFileHandler {
    pub wabbajack_file: WabbajackFileHandle,
    pub output_directory: PathBuf,
}

impl InlineFileHandler {
    #[tracing::instrument(skip(self))]
    pub fn handle(
        self,
        InlineFileDirective {
            hash,
            size,
            source_data_id,
            to,
        }: InlineFileDirective,
    ) -> Result<u64> {
        let output_path = self.output_directory.join(to.into_path());
        let wabbajack_file = self.wabbajack_file.clone();
        let output_file = create_file_all(&output_path)?;

        let archive = wabbajack_file;
        archive
            .get_source_data(source_data_id)
            .and_then(|source_data| {
                source_data
                    .open_file_read()
                    .map(|(_, file)| (source_data, file))
            })
            .and_then(|(_guard, mut file)| {
                let mut writer = std::io::BufWriter::new(output_file);
                std::io::copy(
                    &mut tracing::Span::current().wrap_read(size, &mut file),
                    // WARN: stuff that's inside modlist.wabbajack/modlist(.json) is incorrect
                    // .and_validate_size(size)
                    // .and_validate_hash(hash.pipe(to_u64_from_base_64).expect("come on")),
                    &mut writer,
                )
                .context("copying file from archive")
                .and_then(|_| writer.flush().context("flushing"))
            })
            .map(|_| ())
            .map(|_| size)
    }
}
