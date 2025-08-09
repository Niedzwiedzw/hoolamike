pub mod async_write;
pub mod read;
pub mod write;

pub struct IoHook<R, F> {
    pub inner: R,
    pub callback: F,
}

impl<T, F> Unpin for IoHook<T, F> {}

impl<R, F> IoHook<R, F> {
    #[allow(dead_code)]
    pub fn new(inner: R, callback: F) -> Self {
        IoHook { inner, callback }
    }
}
