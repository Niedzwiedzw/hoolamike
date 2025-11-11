use {
    super::{RetBuffers, ringbuf_types::BufferSplit},
    itertools::Itertools,
    nonempty::NonEmpty,
    ringbuf::traits::{Consumer, Observer, Producer},
    std::num::NonZeroUsize,
    tap::prelude::*,
};

pub struct MultichannelChunkBuffer<T> {
    pub channel_buffers: NonEmpty<BufferSplit<T>>,
}

impl<T> MultichannelChunkBuffer<T>
where
    T: Copy + Default,
{
    pub fn new(chunk_size: NonZeroUsize, channels: NonZeroUsize) -> Self {
        Self {
            channel_buffers: (0..channels.get())
                .map(|_| BufferSplit::new(chunk_size.get()))
                .collect_vec()
                .pipe(NonEmpty::from_vec)
                .expect("channels to be non zero"),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn len(&self) -> usize {
        self.channel_buffers.first().reader.occupied_len()
    }

    /// reads chunk into chunk reader
    pub fn read_from<Inner>(&mut self, chunk: &[Inner]) -> usize
    where
        Inner: AsRef<[T]>,
    {
        chunk
            .iter()
            .map(|c| c.as_ref())
            .zip(self.channel_buffers.iter_mut())
            .map(|(input, channel)| {
                debug_assert!(input.len() <= channel.writer.vacant_len());
                channel.writer.push_slice(input)
            })
            .last()
            .expect("chunk cannot be empty")
    }

    pub fn try_write_into(&mut self, buffer: &mut RetBuffers<T>, size: usize) -> Option<usize> {
        match self.len() >= size {
            true => self
                .channel_buffers
                .iter_mut()
                .zip(buffer.iter_mut())
                .map(|(from, to)| {
                    to.resize(size, Default::default());
                    from.reader
                        .pop_iter()
                        .take(size)
                        .zip(to.iter_mut())
                        .for_each(|(from, to)| *to = from);
                    size
                })
                .last(),
            false => None,
        }
    }
}
