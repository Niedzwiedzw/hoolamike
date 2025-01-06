use {
    crate::{
        compression::{ArchiveHandle, ProcessArchive, SeekWithTempFileExt},
        utils::spawn_rayon,
    },
    anyhow::Context,
    futures::TryFutureExt,
    nonempty::NonEmpty,
    std::{
        convert::identity,
        ffi::{OsStr, OsString},
        future::ready,
        path::{Path, PathBuf},
        sync::Arc,
    },
    tap::prelude::*,
    tokio::{
        sync::{oneshot::error::RecvError, AcquireError, Semaphore},
        task::JoinHandle,
    },
    tracing::{debug_span, instrument, trace_span, Instrument},
};

#[derive(thiserror::Error, Debug, Clone)]
pub enum Error {
    #[error("channel has been closed")]
    ChannelClosed(#[source] RecvError),
    #[error("extraction from archive failed")]
    ExtractingFromArchive(#[source] Arc<anyhow::Error>),
    #[error("thread crashed")]
    ThreadCrashed(#[source] Arc<tokio::task::JoinError>),
    #[error("Cached future task failed")]
    CachedFutureFailed(
        #[source]
        #[from]
        tokio_cached_future::ArcJoinError,
    ),
    #[error("could not acquire permit")]
    AcquiringPermit(#[source] Arc<AcquireError>),
    #[error("could not communicate with worker thread")]
    Recv(#[source] RecvError),
}

impl Error {
    pub fn extracting_from_archive(error: anyhow::Error) -> Self {
        error.pipe(Arc::new).pipe(Self::ExtractingFromArchive)
    }
    pub fn thread_crashed(error: tokio::task::JoinError) -> Self {
        error.pipe(Arc::new).pipe(Self::ThreadCrashed)
    }
}

pub(crate) type Result<T> = std::result::Result<T, Error>;

pub type Extracted = tempfile::TempPath;

#[derive(Debug)]
pub enum SourceKind {
    JustPath(PathBuf),
    CachedPath(Extracted),
}

#[derive(Clone)]
pub struct QueuedArchiveService {
    pub tasks: Arc<tokio_cached_future::CachedFutureQueue<NonEmpty<PathBuf>, Result<Arc<SourceKind>>>>,
    pub permits: Arc<Semaphore>,
}

impl QueuedArchiveService {
    pub fn new(concurrency: usize) -> Arc<Self> {
        Arc::new(Self {
            tasks: tokio_cached_future::CachedFutureQueue::new(),
            permits: Arc::new(Semaphore::new(concurrency)),
        })
    }

    #[tracing::instrument(skip_all)]
    pub fn get_archive_spawn(self: Arc<Self>, archive: NonEmpty<PathBuf>) -> JoinHandle<Result<Arc<SourceKind>>> {
        tokio::task::spawn(
            self.get_archive(archive)
                .instrument(debug_span!("getting_archive_from_queue")),
        )
    }

    #[async_recursion::async_recursion]
    #[tracing::instrument(skip_all)]
    async fn init_archive(self: Arc<Self>, archive_path: NonEmpty<PathBuf>) -> Result<SourceKind> {
        fn popped<T>(mut l: NonEmpty<T>) -> Option<(NonEmpty<T>, T)> {
            l.pop().map(|i| (l, i))
        }
        match popped(archive_path.clone()) {
            Some((parent, archive_path)) => {
                self.clone()
                    .get_archive(parent.clone())
                    .pipe(Box::pin)
                    .instrument(debug_span!("entry was not found, so scheduling creation of parent"))
                    .and_then(|parent_source| {
                        prepare_archive(
                            self.permits.clone(),
                            parent_source,
                            parent.last().extension().map(ToOwned::to_owned),
                            archive_path,
                        )
                        .instrument(tracing::Span::current())
                        .pipe(Box::pin)
                    })
                    .map_ok(SourceKind::CachedPath)
                    .await
            }
            None => Ok(SourceKind::JustPath(archive_path.head)),
        }
    }

    #[instrument(skip(self))]
    pub async fn get_archive(self: Arc<Self>, archive_path: NonEmpty<PathBuf>) -> Result<Arc<SourceKind>> {
        let queue = self.clone();
        tokio::task::spawn(
            async move {
                cloned![queue];
                self.tasks
                    .clone()
                    .get(archive_path, {
                        cloned![queue];
                        move |archive_path| {
                            cloned![queue];
                            queue.init_archive(archive_path).map_ok(Arc::new)
                        }
                    })
                    .pipe(Box::pin)
                    .map_err(self::Error::from)
                    .and_then(|r| r.pipe_as_ref(|r| r.clone()).pipe(ready))
                    .await
            }
            .instrument(tracing::Span::current()),
        )
        .pipe(Box::pin)
        .await
        .map_err(self::Error::thread_crashed)
        .and_then(identity)
    }
}

#[instrument]
pub fn prepare_archive_blocking(source: &SourceKind, archive_path: &Path, extension: Option<&OsStr>) -> anyhow::Result<(u64, Extracted)> {
    ArchiveHandle::with_guessed(source.as_ref(), extension, |mut archive| {
        archive.get_handle(archive_path).and_then(|mut handle| {
            handle
                .size()
                .and_then(|size| handle.seek_with_temp_file_blocking_raw(size))
        })
    })
    .with_context(|| format!("preparing [{source:?}] -> {archive_path:?}"))
}

#[instrument(skip(_computation_permits))]
async fn prepare_archive(
    _computation_permits: Arc<Semaphore>,
    source: Arc<SourceKind>,
    extension: Option<OsString>,
    archive_path: PathBuf,
) -> Result<Extracted> {
    tokio::task::spawn({
        async move {
            let prepare_archive_on_thread = trace_span!("prepare_archive_on_thread");
            spawn_rayon(move || prepare_archive_on_thread.in_scope(|| prepare_archive_blocking(&source, &archive_path, extension.as_deref())))
                .map_err(self::Error::extracting_from_archive)
                .instrument(debug_span!("waiting for thread to finish"))
                .await
        }
        .instrument(trace_span!("preparation_task"))
    })
    .map_err(self::Error::thread_crashed)
    .and_then(ready)
    .instrument(tracing::Span::current())
    .await
    .map(|(_, extracted)| extracted)
}

impl AsRef<std::path::Path> for SourceKind {
    fn as_ref(&self) -> &std::path::Path {
        match self {
            SourceKind::JustPath(path_buf) => path_buf,
            SourceKind::CachedPath(cached) => cached,
        }
    }
}
