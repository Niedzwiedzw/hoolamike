use {
    futures::channel::mpsc::UnboundedReceiver,
    futures_util::{Stream, StreamExt},
    std::{
        pin::Pin,
        task::{Context, Poll},
    },
};

#[derive(Debug)]
pub struct UnboundedReceiverStream<T> {
    inner: UnboundedReceiver<T>,
}

impl<T> UnboundedReceiverStream<T> {
    fn new(recv: UnboundedReceiver<T>) -> Self {
        Self { inner: recv }
    }

    pub fn into_inner(self) -> UnboundedReceiver<T> {
        self.inner
    }

    pub fn close(&mut self) {
        self.inner.close()
    }
}

impl<T> Stream for UnboundedReceiverStream<T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.poll_next_unpin(cx)
    }
}

#[extension_traits::extension(pub trait UnboundedReceiverStreamExt)]
impl<T> UnboundedReceiver<T> {
    fn into_stream(self) -> UnboundedReceiverStream<T> {
        UnboundedReceiverStream::new(self)
    }
}
