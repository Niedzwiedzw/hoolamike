use {
    anyhow::Context,
    futures::FutureExt,
    itertools::Itertools,
    serde::{Deserialize, Serialize},
    std::{convert::identity, future::Future, path::PathBuf, sync::Arc},
    tap::prelude::*,
    tracing::info_span,
};

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
            format!("Caught panic with message: {}", message)
        } else if let Some(message) = self.downcast_ref::<String>() {
            format!("Caught panic with message: {}", message)
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
        std::fs::OpenOptions::new()
            .read(true)
            .open(self)
            .with_context(|| format!("opening file for reading at [{}]", self.as_ref().display()))
            .map(|file| (self.as_ref().to_owned(), file))
    }
    fn open_file_write(&self) -> anyhow::Result<(PathBuf, std::fs::File)> {
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
    }
}

#[derive(derive_more::Display, Debug, Clone)]
pub struct ArcError(Arc<anyhow::Error>);

pub type ArcResult<T> = std::result::Result<T, ArcError>;

#[extension_traits::extension(pub trait AnyhowArcResultExt)]
impl<T> anyhow::Result<T> {
    fn arced(self) -> ArcResult<T> {
        self.map_err(Arc::new).map_err(ArcError)
    }
}

#[extension_traits::extension(pub trait ArcResultExt)]
impl<T> ArcResult<T> {
    fn into_inner_err(self) -> anyhow::Result<T> {
        self.map_err(|e| Arc::try_unwrap(e.0.clone()).unwrap_or_else(|_| anyhow::anyhow!("{e:#?}")))
    }
}

impl std::error::Error for ArcError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }

    fn cause(&self) -> Option<&dyn std::error::Error> {
        self.source()
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
    use std::iter::repeat;
    assert_eq!(
        chunk_while(repeat(1u8).take(6).collect(), |chunk| chunk.len() == 2),
        vec![vec![1u8, 1], vec![1u8, 1], vec![1u8, 1]]
    );
}

pub fn deserialize_json_with_error_location<T: serde::de::DeserializeOwned>(text: &str) -> anyhow::Result<T> {
    serde_json::from_str(text)
        .pipe(|res| {
            if let Some((line, column)) = res.as_ref().err().map(|err| (err.line(), err.column())) {
                res.with_context(|| format!("error occurred at [{}:{}]", line, column))
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
