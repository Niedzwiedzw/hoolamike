use {
    super::IoHook,
    std::io::{self, Read},
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