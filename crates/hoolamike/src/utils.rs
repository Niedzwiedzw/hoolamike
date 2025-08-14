use {
    anyhow::Context,
    futures::FutureExt,
    itertools::Itertools,
    serde::{Deserialize, Serialize},
    std::{convert::identity, future::Future, path::PathBuf},
    tap::prelude::*,
    tempfile::{NamedTempFile, TempPath},
    tracing::{debug_span, info_span},
};

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

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Hash, derive_more::Display, Clone, Ord)]
pub struct MaybeWindowsPath(pub String);

impl std::fmt::Debug for MaybeWindowsPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl MaybeWindowsPath {
    pub fn into_path(self) -> PathBuf {
        let s = self.0;
        let s = match s.contains("\\\\") {
            true => s.split("\\\\").join("/"),
            false => s,
        };
        let s = match s.contains("\\") {
            true => s.split("\\").join("/"),
            false => s,
        };
        PathBuf::from(s)
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

#[extension_traits::extension(pub(crate) trait PathReadWrite)]
impl<T: AsRef<std::path::Path>> T {
    fn open_file_read(&self) -> anyhow::Result<(PathBuf, std::fs::File)> {
        debug_span!("open_file_read", path=%self.as_ref().display()).in_scope(|| {
            std::fs::OpenOptions::new()
                .read(true)
                .open(self)
                .with_context(|| format!("opening file for reading at [{}]", self.as_ref().display()))
                .map(|file| (self.as_ref().to_owned(), file))
        })
    }
    fn open_file_write(&self) -> anyhow::Result<(PathBuf, std::fs::File)> {
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
                    .map(|file| (self.as_ref().to_owned(), file))
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
