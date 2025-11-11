use case_insensitive_path::{CaseInsensitivePathBuf, ExistingPathBuf, IntoUtf8CaseInsensitivePath};

pub type Extracted = tempfile::TempPath;

#[derive(Debug)]
pub enum SourceKind {
    JustPath(CaseInsensitivePathBuf),
    CachedPath(Extracted),
}

impl SourceKind {
    pub fn exists(&self) -> anyhow::Result<ExistingPathBuf> {
        match self {
            SourceKind::JustPath(path_buf) => path_buf.try_exists(),
            SourceKind::CachedPath(cached) => cached.case_insensitive_utf8().and_then(|c| c.try_exists()),
        }
    }
}
