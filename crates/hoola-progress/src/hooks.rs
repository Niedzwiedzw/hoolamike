#[cfg(feature = "tokio")]
pub mod async_write {
    use {
        super::IoHook,
        std::{
            pin::Pin,
            task::{Context, Poll},
        },
        tokio::io,
    };

    impl<W: io::AsyncWrite + Unpin, F: Fn(usize)> tokio::io::AsyncWrite for IoHook<W, F> {
        fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
            Pin::new(&mut self.inner).poll_write(cx, buf).map(|poll| {
                poll.inspect(|inc| {
                    (self.callback)(*inc);
                })
            })
        }

        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.inner).poll_flush(cx)
        }

        fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.inner).poll_shutdown(cx)
        }
    }
}

pub mod read {
    use {
        super::IoHook,
        std::io::{self, Read, Seek},
    };

    #[extension_traits::extension(pub trait ReadHookExt)]
    impl<T: Read> T
    where
        Self: Sized,
    {
        fn hook_read<F: Fn(usize)>(self, hook_read: F) -> IoHook<T, F> {
            IoHook {
                inner: self,
                callback: hook_read,
            }
        }
    }

    impl<R, F> Read for IoHook<R, F>
    where
        R: Read,
        F: Fn(usize),
    {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let bytes_read = self.inner.read(buf)?;
            (self.callback)(bytes_read);
            Ok(bytes_read)
        }
    }

    impl<R: Seek, F> Seek for IoHook<R, F> {
        fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
            self.inner.seek(pos)
        }
    }
}

pub mod write {
    use {
        super::IoHook,
        std::io::{self, Write},
    };

    #[extension_traits::extension(pub trait WriteHookExt)]
    impl<T: Write> T
    where
        Self: Sized,
    {
        fn hook_write<F: Fn(usize)>(self, hook_read: F) -> IoHook<T, F> {
            IoHook {
                inner: self,
                callback: hook_read,
            }
        }
    }

    impl<W, F> Write for IoHook<W, F>
    where
        W: Write,
        F: Fn(usize),
    {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let bytes_written = self.inner.write(buf)?;
            (self.callback)(bytes_written);
            Ok(bytes_written)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.inner.flush()
        }
    }
}

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
