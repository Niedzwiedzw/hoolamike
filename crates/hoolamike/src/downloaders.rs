use {
    crate::modlist_json::{ArchiveDescriptor, HumanUrl},
    std::path::PathBuf,
};

pub mod gamefile_source_downloader;
pub mod google_drive;
pub mod mediafire;
pub mod mega;
pub mod nexus;
pub mod wabbajack_cdn;

pub mod helpers;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, transpare::Transpare)]
pub struct WithArchiveDescriptor<T> {
    pub inner: T,
    pub descriptor: ArchiveDescriptor,
}

pub type MergeDownloadTask = WithArchiveDescriptor<(Vec<HumanUrl>, PathBuf)>;
pub type DownloadTask = WithArchiveDescriptor<(HumanUrl, PathBuf)>;
pub type CopyFileTask = WithArchiveDescriptor<(PathBuf, PathBuf)>;

#[derive(Debug, Clone, derive_more::From)]
pub enum SyncTask {
    MergeDownload(MergeDownloadTask),
    Download(DownloadTask),
    Copy(CopyFileTask),
}
