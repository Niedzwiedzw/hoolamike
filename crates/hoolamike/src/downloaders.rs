use {
    crate::modlist_json::{ArchiveDescriptor, HumanUrl},
    case_insensitive_path::ExistingPathBuf,
    std::path::PathBuf,
    typed_path::Utf8PlatformPathBuf,
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

pub type MergeDownloadTask = WithArchiveDescriptor<(Vec<HumanUrl>, Utf8PlatformPathBuf)>;
pub type DownloadTask = WithArchiveDescriptor<(HumanUrl, Utf8PlatformPathBuf)>;
pub type CopyFileTask = WithArchiveDescriptor<(ExistingPathBuf, Utf8PlatformPathBuf)>;

#[derive(Debug, Clone, derive_more::From)]
pub enum SyncTask {
    MergeDownload(MergeDownloadTask),
    Download(DownloadTask),
    Copy(CopyFileTask),
}
