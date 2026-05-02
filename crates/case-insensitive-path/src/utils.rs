#[extension_traits::extension(pub trait ResultZipExt)]
impl<T, E> std::result::Result<T, E> {
    fn zip<O>(self, other: std::result::Result<O, E>) -> std::result::Result<(T, O), E> {
        self.and_then(|one| other.map(|other| (one, other)))
    }
}

#[allow(dead_code)]
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
