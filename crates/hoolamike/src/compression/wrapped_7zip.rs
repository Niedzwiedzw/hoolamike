use {::wrapped_7zip::Wrapped7Zip, itertools::Itertools, std::num::NonZeroUsize};

thread_local! {
    pub static WRAPPED_7ZIP: Arc<Wrapped7Zip> = Arc::new(Wrapped7Zip::find_bin(*crate::consts::TEMP_FILE_DIR).expect("no 7z found, fix your dependencies"));
}

use super::*;
impl ProcessArchive for ::wrapped_7zip::ArchiveHandle {
    fn list_paths(&mut self) -> Result<Vec<PathBuf>> {
        self.list_files()
            .and_then(|files| {
                files
                    .into_iter()
                    .map(|entry| entry.path.pipe_deref(CaseInsensitivePathBuf::from_path))
                    .collect::<Result<Vec<_>>>()
            })
            .context("listing paths of 7zip archive")
    }
    fn get_many_handles(&mut self, paths: &[&Path]) -> Result<Vec<(PathBuf, super::ArchiveFileHandle)>> {
        paths
            .iter()
            .map(|p| p.as_original_std_path())
            .collect_vec()
            .pipe(|paths| {
                paths
                    .iter()
                    .map(|p| p.as_path())
                    .collect_vec()
                    .pipe_deref(|paths| ::wrapped_7zip::ArchiveHandle::get_many_handles(self, paths, Some(NonZeroUsize::new(1).expect("expected non-zero"))))
            })
            .and_then(|output| {
                output
                    .into_iter()
                    .map(|e| {
                        e.0.path
                            .pipe_deref(CaseInsensitivePathBuf::from_path)
                            .map(|name| (name, super::ArchiveFileHandle::Wrapped7Zip(e)))
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .with_context(|| {
                format!(
                    "when getting multiple handles out of an archive of kind [{kind:?}]",
                    kind = ArchiveHandleKind::Wrapped7Zip
                )
            })
    }
    fn get_handle(&mut self, path: &Path) -> Result<super::ArchiveFileHandle> {
        self.get_file(&path.as_original_std_path())
            .map(super::ArchiveFileHandle::Wrapped7Zip)
    }
}
