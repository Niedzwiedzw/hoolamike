use std::mem;

#[extension_traits::extension(pub trait IteratorChunkWhileExt)]
impl<I: Iterator> I {
    fn chunk_while<F: FnMut(&[I::Item]) -> bool>(self, chunk_while: F) -> ChunkWhile<I, F> {
        ChunkWhile::new(self, chunk_while)
    }
}
/// An iterator adapter that chunks elements according to a predicate.
///
/// The predicate `chunk_while(&[T]) -> bool` is checked *before* pushing
/// the next element. If the predicate returns `true`, we start a new chunk
/// with the incoming element.
pub struct ChunkWhile<I: Iterator, F> {
    iter: I,
    chunk_while: F,
    current_chunk: Vec<I::Item>,
    done: bool,
}

impl<I, F> ChunkWhile<I, F>
where
    I: Iterator,
    F: FnMut(&[I::Item]) -> bool,
{
    /// Create a new `ChunkWhile` from an iterator and a predicate.
    pub fn new(iter: I, chunk_while: F) -> Self {
        Self {
            iter,
            chunk_while,
            current_chunk: Vec::new(),
            done: false,
        }
    }
}

impl<I, F> Iterator for ChunkWhile<I, F>
where
    I: Iterator,
    F: FnMut(&[I::Item]) -> bool,
{
    type Item = Vec<I::Item>;

    fn next(&mut self) -> Option<Self::Item> {
        // If we've already exhausted the underlying iterator, we're done.
        if self.done {
            return None;
        }

        // If `current_chunk` is empty, we need to pull at least one item
        // from `iter` to start a new chunk.
        if self.current_chunk.is_empty() {
            if let Some(first) = self.iter.next() {
                self.current_chunk.push(first);
            } else {
                // No items at all
                self.done = true;
                return None;
            }
        }

        // Read items until `chunk_while` tells us to cut over to a new chunk
        for item in self.iter.by_ref() {
            // Check the current chunk (before pushing the new item).
            if !(self.chunk_while)(self.current_chunk.as_slice()) {
                // The predicate says: "start a new chunk now."
                // So yield the existing chunk and begin a fresh one with `item`.
                let finished_chunk = std::mem::take(&mut self.current_chunk);
                self.current_chunk.push(item);
                return Some(finished_chunk);
            } else {
                // Still in the same chunk, so just push the item.
                self.current_chunk.push(item);
            }
        }

        // If we get here, we've exhausted the underlying iterator.
        self.done = true;

        // Yield whatever remains in `current_chunk` (if anything).
        if !self.current_chunk.is_empty() {
            Some(mem::take(&mut self.current_chunk))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test an empty iterator to ensure no chunks are produced.
    #[test]
    fn test_empty_iterator() {
        let data: Vec<i32> = vec![];
        let mut iter = data.into_iter().chunk_while(|_| true);
        assert_eq!(iter.next(), None);
    }

    /// Test with a predicate that always returns `false`,
    /// meaning no splits—everything should end up in a single chunk.
    #[test]
    fn test_no_split() {
        let data = vec![1, 2, 3];
        let mut iter = data.into_iter().chunk_while(|_chunk| true);
        assert_eq!(iter.next(), Some(vec![1, 2, 3]));
        assert_eq!(iter.next(), None);
    }

    /// Test with a predicate that always returns `true` as soon as
    /// the current chunk is non-empty. This splits between every item.
    #[test]
    fn test_split_every_time() {
        let data = vec![1, 2, 3];
        let mut iter = data.into_iter().chunk_while(|chunk| chunk.is_empty());

        // Each item should form its own chunk.
        assert_eq!(iter.next(), Some(vec![1]));
        assert_eq!(iter.next(), Some(vec![2]));
        assert_eq!(iter.next(), Some(vec![3]));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_chunk_while_size_less_than_5() {
        let data = std::iter::repeat(1u8).take(1000);
        let iter = data
            .into_iter()
            .chunk_while(|chunk| chunk.iter().copied().sum::<u8>() < 5);
        for chunk in iter {
            assert_eq!(chunk.len(), 5);
        }
    }
}
