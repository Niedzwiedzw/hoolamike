use {
    crate::path::ExistingPathBuf,
    anyhow::Context,
    base64::{Engine, prelude::BASE64_STANDARD},
    case_insensitive_path::{ExistingPath, PathExistsUtf8Ext},
    futures::FutureExt,
    itertools::Itertools,
    serde::{Deserialize, Serialize},
    std::{borrow::Cow, convert::identity, future::Future},
    tap::prelude::*,
    tempfile::{NamedTempFile, TempPath},
    tracing::{debug_span, info_span},
};

#[extension_traits::extension(pub trait StreamLenExt)]
impl<T: std::io::Seek> T {
    fn stream_len(&mut self) -> std::io::Result<u64> {
        let old_pos = self.stream_position()?;
        let len = self.seek(std::io::SeekFrom::End(0))?;

        // Avoid seeking a third time when we were already at the end of the
        // stream. The branch is usually way cheaper than a seek operation.
        if old_pos != len {
            self.seek(std::io::SeekFrom::Start(old_pos))?;
        }

        Ok(len)
    }
}

pub fn obfuscate_value(value: &str) -> String {
    match value {
        value if value.len() < 3 => value.chars().map(|_| '*').collect(),
        other => {
            let chars = || other.chars();
            chars()
                .take(1)
                .chain(chars().skip(1).take(other.len() - 1).map(|_| '*'))
                .chain(chars().last())
                .collect()
        }
    }
}

#[derive(derive_more::Constructor, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Serialize, Deserialize)]
pub struct Obfuscated<T>(pub T);

impl<T> std::fmt::Debug for Obfuscated<T>
where
    T: std::fmt::Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        obfuscate_value(&format!("{}", self.0)).fmt(f)
    }
}

#[extension_traits::extension(pub trait ReadableCatchUnwindExt)]
impl<T> std::result::Result<T, Box<dyn std::any::Any + Send>> {
    fn for_anyhow(self) -> anyhow::Result<T> {
        self.map_err(ReadableCatchUnwindErrorExt::to_readable_error)
    }
}

#[extension_traits::extension(pub trait ReadableCatchUnwindErrorExt)]
impl Box<dyn std::any::Any + Send> {
    fn to_readable_error(self) -> anyhow::Error {
        if let Some(message) = self.downcast_ref::<&str>() {
            format!("Caught panic with message: {message}")
        } else if let Some(message) = self.downcast_ref::<String>() {
            format!("Caught panic with message: {message}")
        } else {
            "Caught panic with an unknown type.".to_string()
        }
        .pipe(|e| anyhow::anyhow!("{e}"))
    }
}

#[extension_traits::extension(pub trait ResultZipExt)]
impl<T, E> std::result::Result<T, E> {
    fn zip<O>(self, other: std::result::Result<O, E>) -> std::result::Result<(T, O), E> {
        self.and_then(|one| other.map(|other| (one, other)))
    }
}

#[allow(dead_code)]
pub fn boxed_iter<'a, T: 'a>(iter: impl Iterator<Item = T> + 'a) -> Box<dyn Iterator<Item = T> + 'a> {
    Box::new(iter)
}

#[macro_export]
macro_rules! cloned {
    ($($es:ident),+) => {$(
        #[allow(unused_mut)]
        let mut $es = $es.clone();
    )*}
}

impl ExistingPathRead for tempfile::TempPath {
    fn open_file_read(&self) -> anyhow::Result<(ExistingPathBuf, std::fs::File)> {
        self.exists_utf8().and_then(|f| f.open_file_read())
    }

    fn join_create(&self, segment: &str) -> anyhow::Result<ExistingPathBuf> {
        self.exists_utf8().and_then(|d| d.join_create(segment))
    }
}

#[extension_traits::extension(pub(crate) trait ExistingPathRead)]
impl ExistingPath {
    fn open_file_read(&self) -> anyhow::Result<(ExistingPathBuf, std::fs::File)> {
        debug_span!("open_file_read", path=%self).in_scope(|| {
            std::fs::OpenOptions::new()
                .read(true)
                .open(self.as_ref())
                .with_context(|| format!("opening file for reading at [{self}]"))
                .map(|file| (self.into_owned(), file))
        })
    }
    fn join_create(&self, segment: &str) -> anyhow::Result<ExistingPathBuf> {
        self.as_path()
            .join_checked(segment)
            .context("adding segment")
            .and_then(|joined| joined.create_dir())
            .with_context(|| format!("join-creating directory [{segment}] at [{self}]"))
    }
}

#[extension_traits::extension(pub(crate) trait PathReadWrite)]
impl<T: AsRef<std::path::Path>> T {
    fn create_dir(&self) -> anyhow::Result<ExistingPathBuf> {
        let at = self.as_ref();
        std::fs::create_dir_all(&at)
            .context("creating all directories")
            .and_then(|()| ExistingPathBuf::new(at))
            .with_context(|| format!("creating directory at [{}]", at.display()))
    }

    fn open_file_write(&self) -> anyhow::Result<(ExistingPathBuf, std::fs::File)> {
        debug_span!("open_file_read", path=%self.as_ref().display()).in_scope(|| {
            Ok(()).and_then(|_| {
                if let Some(parent) = self.as_ref().parent() {
                    std::fs::create_dir_all(parent).context("creating full path for output file")?;
                }
                std::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(self)
                    .with_context(|| format!("opening file for writing at [{}]", self.as_ref().display()))
                    .and_then(|file| {
                        self.as_ref()
                            .pipe(ExistingPathBuf::new)
                            .map(|path| (path, file))
                    })
            })
        })
    }
}

// #[tracing::instrument(skip(task_fn))]
pub(crate) fn spawn_rayon<T, F>(task_fn: F) -> impl Future<Output = anyhow::Result<T>>
where
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    T: Send + Sync + 'static,
{
    let span = info_span!("performing_work_on_threadpool");
    let (tx, rx) = tokio::sync::oneshot::channel();
    rayon::spawn_fifo(move || {
        span.in_scope(|| {
            if tx.send(task_fn()).is_err() {
                tracing::error!("could not communicate from thread")
            }
        })
    });
    rx.map(|res| res.context("task crashed?").and_then(identity))
}

pub fn chunk_while<T>(input: Vec<T>, mut chunk_while: impl FnMut(&[T]) -> bool) -> Vec<Vec<T>> {
    let mut buf = vec![vec![]];
    for element in input {
        if chunk_while(buf.last().unwrap().as_slice()) {
            buf.push(vec![]);
        }
        buf.last_mut().unwrap().push(element);
    }
    buf
}

#[test]
fn test_chunk_while() {
    use std::iter::repeat_n;
    assert_eq!(
        chunk_while(repeat_n(1u8, 6).collect(), |chunk| chunk.len() == 2),
        vec![vec![1u8, 1], vec![1u8, 1], vec![1u8, 1]]
    );
}

pub fn scoped_temp_file() -> anyhow::Result<NamedTempFile> {
    tempfile::Builder::new()
        .prefix("seeked-file-")
        .tempfile_in(*crate::consts::TEMP_FILE_DIR)
        .context("creating temp file")
}

pub fn scoped_temp_path() -> anyhow::Result<TempPath> {
    self::scoped_temp_file()
        .map(|p| p.into_temp_path())
        .context("creating temp path")
}
pub fn with_scoped_temp_path<T, F: FnOnce(&TempPath) -> anyhow::Result<T>>(with: F) -> anyhow::Result<T> {
    self::scoped_temp_path()
        .and_then(|path| with(&path))
        .context("performing operation on a scoped temp file")
}

pub fn deserialize_json_with_error_location<T: serde::de::DeserializeOwned>(text: &str) -> anyhow::Result<T> {
    serde_json::from_str(text)
        .pipe(|res| {
            if let Some((line, column)) = res.as_ref().err().map(|err| (err.line(), err.column())) {
                res.with_context(|| format!("error occurred at [{line}:{column}]"))
                    .with_context(|| {
                        text.lines()
                            .enumerate()
                            .skip(line.saturating_sub(10))
                            .take(20)
                            .map(|(idx, line)| format!("{idx}.\t{line}"))
                            .join("\n")
                    })
            } else {
                res.context("oops")
            }
        })
        .context("parsing text")
        .with_context(|| format!("could not parse as {}", std::any::type_name::<T>()))
}

#[allow(dead_code)]
pub trait AsBase64 {
    fn to_base64(&self) -> String;
}

impl<T: AsRef<[u8]>> AsBase64 for T {
    fn to_base64(&self) -> String {
        BASE64_STANDARD.encode(self.as_ref())
    }
}

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

impl AsBase64 for Path {
    fn to_base64(&self) -> String {
        self.as_os_str().as_encoded_bytes().to_base64()
    }
}

pub type FileExtension<'a> = Option<&'a str>;

#[extension_traits::extension(pub trait PathFileNameOrEmpty)]
impl<P: AsRef<Path>> P {
    fn file_stem_opt(&self) -> Option<Cow<'_, str>> {
        self.as_ref().file_name().map(|name| name.to_string_lossy())
    }

    fn map_file_stem<F: FnOnce(&str) -> String>(&self, map: F) -> Option<PathBuf> {
        self.as_ref()
            .file_stem()
            .map(|p| p.to_string_lossy())
            .map(|stem| {
                map(stem.as_ref()).pipe(|file_stem| match self.as_ref().extension() {
                    Some(ext) => format!("{file_stem}.{}", ext.to_string_lossy()),
                    None => file_stem,
                })
            })
            .map(|filename| self.as_ref().with_file_name(filename))
    }
    fn extension_opt(&self) -> Option<Cow<'_, str>> {
        self.as_ref().extension().map(|e| e.to_string_lossy())
    }
    fn named_tempfile_with_context(&self) -> anyhow::Result<NamedTempFile> {
        #[allow(clippy::ptr_arg)]
        fn cow_str<'c>(cow: &'c Cow<'_, str>) -> &'c str {
            match cow {
                Cow::Borrowed(b) => b,
                Cow::Owned(o) => o.as_str(),
            }
        }
        (
            self.file_stem_opt().unwrap_or(Cow::Borrowed("unnamed")),
            self.extension_opt().map(|ext| format!(".{ext}")),
        )
            .pipe(|(stem, extension)| {
                tempfile::Builder::new()
                    .tap_mut(|b| {
                        b.prefix(stem.pipe_ref(cow_str));
                        if let Some(extension) = extension.as_ref() {
                            b.suffix(extension);
                        }
                    })
                    .tempfile_in(*crate::consts::TEMP_FILE_DIR)
                    .with_context(|| {
                        format!(
                            "creating temp file in {} (prefix: {stem}, suffix: .{extension:?})",
                            crate::consts::TEMP_FILE_DIR.display()
                        )
                    })
            })
    }
}

pub trait IteratorTryFindMap: Iterator {
    /// Applies a fallible function to each item and returns the first `Ok(Some(value))`.
    /// Returns `Ok(None)` if no item produced a `Some`, or the first `Err` encountered.
    fn try_find_map<F, T, E>(&mut self, f: F) -> Result<Option<T>, E>
    where
        F: FnMut(Self::Item) -> Result<Option<T>, E>;
}

impl<I: Iterator> IteratorTryFindMap for I {
    fn try_find_map<F, T, E>(&mut self, mut f: F) -> Result<Option<T>, E>
    where
        F: FnMut(Self::Item) -> Result<Option<T>, E>,
    {
        loop {
            match self.next() {
                Some(item) => match f(item)? {
                    Some(value) => return Ok(Some(value)),
                    None => continue,
                },
                None => return Ok(None),
            }
        }
    }
}

#[extension_traits::extension(pub trait BTreeSetRemoveEntryExt)]
impl<K: Ord> BTreeSet<K> {
    fn remove_entry(&mut self, entry: K) -> Option<K> {
        self.remove(&entry).then_some(entry)
    }
}
