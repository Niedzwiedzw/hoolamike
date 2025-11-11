use {
    dashmap::DashMap,
    futures::{FutureExt, future::Shared},
    std::{
        future::{Future, ready},
        sync::Arc,
    },
    tap::prelude::*,
    tokio::task::JoinHandle,
    tracing::{Instrument, instrument, trace_span},
};

pub struct CachedFutureQueue<K, V> {
    tasks: DashMap<K, Shared<ClonableJoinHandle<Arc<V>>>>,
}

#[derive(Debug, Clone)]
pub struct ArcJoinError(Arc<tokio::task::JoinError>);

impl std::fmt::Display for ArcJoinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for ArcJoinError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }

    #[allow(deprecated, deprecated_in_future)]
    fn description(&self) -> &str {
        self.0.description()
    }

    #[allow(deprecated)]
    fn cause(&self) -> Option<&dyn std::error::Error> {
        self.0.cause()
    }
}

struct ClonableJoinHandle<T>(JoinHandle<T>);

impl From<tokio::task::JoinError> for ArcJoinError {
    fn from(value: tokio::task::JoinError) -> Self {
        ArcJoinError(Arc::new(value))
    }
}

impl<T: Clone> std::future::Future for ClonableJoinHandle<T> {
    type Output = std::result::Result<T, ArcJoinError>;

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        self.0.poll_unpin(cx).map_err(ArcJoinError::from)
    }
}

impl<K, V> CachedFutureQueue<K, V>
where
    K: std::hash::Hash + Eq + std::fmt::Debug + Clone + Send + Sync + 'static,
    V: Send + Sync + 'static,
{
    pub fn new() -> Arc<Self> {
        Arc::new(Self { tasks: Default::default() })
    }
    pub fn preheat(&self, key: K, value: V) {
        self.tasks.insert(
            key,
            tokio::task::spawn(ready(value.pipe(Arc::new)))
                .pipe(ClonableJoinHandle)
                .shared(),
        );
    }
    #[instrument(skip(self, with), level = "TRACE")]
    pub async fn get<F, Fut>(self: Arc<Self>, key: K, with: F) -> std::result::Result<Arc<V>, ArcJoinError>
    where
        Fut: Future<Output = V> + Send + 'static,
        F: FnOnce(K) -> Fut + Send + 'static,
    {
        let future = self
            .tasks
            .entry(key.clone())
            .or_insert_with(move || {
                tokio::task::spawn(
                    (with)(key.clone())
                        .instrument(trace_span!("doing_work"))
                        .map(Arc::new),
                )
                .pipe(ClonableJoinHandle)
                .shared()
            })
            .clone();

        future.await
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        futures::{FutureExt, StreamExt, TryStreamExt},
        tokio::time::{Duration, sleep},
        tracing::{Instrument, info, info_span},
    };

    #[test_log::test(tokio::test)]
    async fn test_simple() -> anyhow::Result<()> {
        let queue = CachedFutureQueue::new();
        let slow_times_two = |num| async move {
            info!("sleeping for 100ms");
            sleep(Duration::from_millis(100)).await;
            info!("slept, returning result");
            num * 2
        };
        info!("spawning task_1");
        let task_1 = queue
            .clone()
            .get(1, slow_times_two)
            .instrument(info_span!("task_1"))
            .inspect(|res| info!(?res, "task_1 finished"));
        sleep(Duration::from_millis(80)).await;
        info!("spawning task_2");
        let task_2 = queue
            .get(1, slow_times_two)
            .instrument(info_span!("task_2"))
            .inspect(|res| info!(?res, "task_2 finished"));

        info!("joining");
        let finished = task_1
            .into_stream()
            .chain(task_2.into_stream())
            .try_collect::<Vec<_>>()
            .await?;
        let [a, b] = finished.as_slice() else { panic!("bad task count") };
        info!("results: (a={a}, b={b})");

        assert!(Arc::ptr_eq(a, b));
        Ok(())
    }
}
