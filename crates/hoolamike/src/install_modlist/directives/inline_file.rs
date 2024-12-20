use {
    super::*,
    crate::{
        compression::ProcessArchive,
        install_modlist::download_cache::validate_hash,
        modlist_json::directive::InlineFileDirective,
        progress_bars::{print_error, vertical_progress_bar, ProgressKind, PROGRESS_BAR},
    },
    std::{convert::identity, io::Write, path::Path},
};

#[derive(Clone, Debug)]
pub struct InlineFileHandler {
    pub wabbajack_file: WabbajackFileHandle,
    pub output_directory: PathBuf,
}

impl InlineFileHandler {
    pub async fn handle(
        self,
        InlineFileDirective {
            hash,
            size,
            source_data_id,
            to,
        }: InlineFileDirective,
    ) -> Result<()> {
        let output_path = self.output_directory.join(to.into_path());
        if let Err(message) = validate_hash(output_path.clone(), hash).await {
            print_error(source_data_id.hyphenated().to_string(), &message);
            let wabbajack_file = self.wabbajack_file.clone();
            tokio::task::spawn_blocking(move || -> Result<_> {
                let pb = vertical_progress_bar(size, ProgressKind::Extract, indicatif::ProgressFinish::AndLeave)
                    .attach_to(&PROGRESS_BAR)
                    .tap_mut(|pb| pb.set_message(output_path.display().to_string()));

                let output_file = create_file_all(&output_path)?;

                let mut archive = wabbajack_file.blocking_lock();
                archive
                    .get_handle(Path::new(&source_data_id.as_hyphenated().to_string()))
                    .and_then(|file| {
                        let mut writer = std::io::BufWriter::new(output_file);
                        std::io::copy(&mut pb.wrap_read(file), &mut writer)
                            .context("copying file from archive")
                            .and_then(|_| writer.flush().context("flushing"))
                    })
                    .map(|_| ())
            })
            .await
            .context("thread crashed")
            .and_then(identity)?;
        }
        Ok(())
    }
}
