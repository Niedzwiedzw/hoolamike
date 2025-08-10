use {
    super::concurrency,
    crate::downloaders::helpers::FutureAnyhowExt,
    anyhow::Result,
    futures::TryFutureExt,
    once_cell::sync::Lazy,
    std::{future::ready, sync::Arc},
    tokio::sync::{OwnedSemaphorePermit, Semaphore},
    tracing::{info_span, instrument, Instrument},
};

pub fn max_open_files() -> usize {
    concurrency() * 40
}

#[allow(dead_code)]
pub(crate) static OPEN_FILE_PERMITS: Lazy<Arc<Semaphore>> = Lazy::new(|| Arc::new(Semaphore::new(max_open_files())));

#[allow(dead_code)]
#[derive(Debug)]
pub struct WithPermit<T> {
    pub permit: OwnedSemaphorePermit,
    pub inner: T,
}

impl<T> WithPermit<T>
where
    T: Send + 'static,
{
    #[instrument(skip_all, level = "DEBUG")]
    pub async fn new<Fut, F>(semaphore: &Arc<Semaphore>, new: F) -> Result<Self>
    where
        Fut: std::future::Future<Output = Result<T>>,
        F: FnOnce() -> Fut,
    {
        semaphore
            .clone()
            .acquire_owned()
            .instrument(info_span!("waiting_for_file_permit"))
            .map_context("semaphore closed")
            .and_then(move |permit| new().map_ok(|inner| Self { permit, inner }))
            .await
    }

    #[instrument(skip_all, level = "DEBUG")]
    pub async fn new_blocking<F>(semaphore: &Arc<Semaphore>, new: F) -> Result<Self>
    where
        F: FnOnce() -> Result<T> + Clone + Send + 'static,
    {
        let span = tracing::Span::current();
        Self::new(semaphore, move || {
            tokio::task::spawn_blocking(move || span.in_scope(new))
                .instrument(tracing::Span::current())
                .map_context("thread crashed")
                .and_then(ready)
        })
        .await
    }
}
