use {::wrapped_7zip::Wrapped7Zip, itertools::Itertools, std::num::NonZeroUsize};

thread_local! {
    pub static WRAPPED_7ZIP: Arc<Wrapped7Zip> = Arc::new(Wrapped7Zip::find_bin(*crate::consts::TEMP_FILE_DIR).expect("no 7z found, fix your dependencies"));
}

use super::*;
impl ProcessArchive for ::wrapped_7zip::ArchiveHandle {
    fn list_paths(&mut self) -> Result<Vec<PathBuf>> {
        self.list_files()
            .map(|files| files.into_iter().map(|entry| entry.path).collect())
    }
    fn get_many_handles(&mut self, paths: &[&Path]) -> Result<Vec<(PathBuf, super::ArchiveFileHandle)>> {
        ::wrapped_7zip::ArchiveHandle::get_many_handles(self, paths, Some(NonZeroUsize::new(1).expect("expected non-zero"))).map(|output| {
            output
                .into_iter()
                .map(|e| (e.0.path.clone(), super::ArchiveFileHandle::Wrapped7Zip(e)))
                .collect_vec()
        })
    }
    fn get_handle(&mut self, path: &Path) -> Result<super::ArchiveFileHandle> {
        self.get_file(path)
            .map(super::ArchiveFileHandle::Wrapped7Zip)
    }
}
