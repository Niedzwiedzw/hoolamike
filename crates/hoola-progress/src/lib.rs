use {
    crate::progress_span::{ProgressDelta, ProgressState},
    hooks::{read::ReadHookExt, write::WriteHookExt, IoHook},
    std::{
        borrow::Cow,
        collections::{btree_map::Entry, BTreeMap, BTreeSet},
        io::{Read, Write},
        iter::once,
        sync::{atomic::AtomicUsize, Arc},
    },
    tap::prelude::*,
};

static NEXT_SPAN_ID: AtomicUsize = AtomicUsize::new(0);

pub mod hooks;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Copy)]
pub struct SpanId(usize);

impl SpanId {
    pub fn next() -> Self {
        Self(NEXT_SPAN_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SpanPath(Arc<[SpanId]>);

impl SpanPath {
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn starts_with(&self, other: &Self) -> bool {
        self.0.get(0..other.0.len()) == Some(other.0.as_ref())
    }
    pub fn parent(&self) -> Option<Self> {
        match self.0.len() {
            0 => None,
            len => Some(Self(Arc::from(&self.0[..len - 1]))),
        }
    }
    pub fn child(&self) -> Self {
        Self(
            self.0
                .iter()
                .copied()
                .chain(once(SpanId::next()))
                .collect::<Arc<[_]>>(),
        )
    }
}

#[derive(Debug)]
pub struct ProgressMap {
    pub finished_pending: BTreeSet<SpanPath>,
    pub progress: BTreeMap<SpanPath, ProgressSpan>,
}

pub mod progress_span {

    #[derive(Debug)]
    pub struct ProgressState {
        pub total: i64,
        pub current: i64,
    }

    #[derive(Debug)]
    pub struct ProgressDelta {
        pub total: i64,
        pub current: i64,
    }

    impl ProgressDelta {
        pub fn apply(self, state: &mut ProgressState) {
            let Self { total, current } = self;
            state.total += total;
            state.current += current;
        }
    }
}

#[derive(Debug)]
pub enum ProgressKind {
    Bytes,
    Iter,
    Parent,
}

#[derive(Debug)]
pub enum Update {
    Start(ProgressSpan),
    Update(progress_span::ProgressDelta),
    Finish,
}

#[derive(Debug)]
pub struct ProgressMessage {
    pub span: SpanPath,
    pub update: Update,
}

#[derive(Clone)]
struct CommunicatorInner(Arc<Sender>);

impl CommunicatorInner {
    pub fn new() -> (Receiver, Self) {
        let (tx, rx) = self::channel();
        (rx, Self(Arc::new(tx)))
    }
}

type Receiver = futures_channel::mpsc::UnboundedReceiver<ProgressMessage>;
type Sender = futures_channel::mpsc::UnboundedSender<ProgressMessage>;

fn channel() -> (Sender, Receiver) {
    futures_channel::mpsc::unbounded()
}

pub struct ProgressCommunicator {
    span: SpanPath,
    communicator: CommunicatorInner,
}

impl Drop for ProgressCommunicator {
    fn drop(&mut self) {
        self.send(Update::Finish)
    }
}

pub trait Progress: Sized {
    fn send(&self, update: Update);
    fn child(&self, name: impl Into<Cow<'static, str>>) -> Self;
    /// you probably don't want to use it unless you're writing an extension
    fn span_raw(&self, span: ProgressSpan) -> Self;
    fn wrap_write<W>(&self, name: impl Into<Cow<'static, str>>, expected: i64, writer: W) -> IoHook<W, impl Fn(usize)>
    where
        W: Write + Sized,
    {
        let communicator = self.span_raw(ProgressSpan {
            name: name.into(),
            state: ProgressState { total: expected, current: 0 },
            kind: ProgressKind::Bytes,
        });
        writer.hook_write(move |current| {
            // TODO: FIXME
            communicator.send(Update::Update(ProgressDelta {
                total: 0,
                current: current as _,
            }));
        })
    }
    fn wrap_read<W>(&self, name: impl Into<Cow<'static, str>>, expected: i64, reader: W) -> IoHook<W, impl Fn(usize)>
    where
        W: Read + Sized,
    {
        let communicator = self.span_raw(ProgressSpan {
            name: name.into(),
            state: ProgressState { total: expected, current: 0 },
            kind: ProgressKind::Bytes,
        });
        reader.hook_read(move |current| {
            // TODO: FIXME
            communicator.send(Update::Update(ProgressDelta {
                total: 0,
                current: current as _,
            }));
        })
    }

    #[cfg(feature = "tokio")]
    fn wrap_async_write<W>(&self, name: impl Into<Cow<'static, str>>, expected: i64, reader: W) -> IoHook<W, impl Fn(usize)>
    where
        W: tokio::io::AsyncWrite + Sized,
    {
        let communicator = self.span_raw(ProgressSpan {
            name: name.into(),
            state: ProgressState { total: expected, current: 0 },
            kind: ProgressKind::Bytes,
        });
        IoHook {
            inner: reader,
            callback: move |current| {
                // TODO: FIXME
                communicator.send(Update::Update(ProgressDelta {
                    total: 0,
                    current: current as _,
                }));
            },
        }
    }
}

impl Progress for () {
    fn send(&self, _update: Update) {}
    fn span_raw(&self, _span: ProgressSpan) -> Self {}
    fn child(&self, _name: impl Into<Cow<'static, str>>) -> Self {}
}

impl Progress for ProgressCommunicator {
    fn span_raw(&self, span: ProgressSpan) -> Self {
        ProgressCommunicator::span_raw(self, span)
    }
    fn send(&self, update: Update) {
        ProgressCommunicator::send(self, update)
    }

    fn child(&self, name: impl Into<Cow<'static, str>>) -> Self {
        ProgressCommunicator::child(self, name)
    }
}

impl ProgressCommunicator {
    fn new() -> (Receiver, Self) {
        let (rx, communicator) = CommunicatorInner::new();
        (
            rx,
            Self {
                span: SpanPath(Arc::from([])),
                communicator,
            },
        )
    }
    fn send(&self, message: Update) {
        if let Err(m) = self.communicator.0.unbounded_send(ProgressMessage {
            span: self.span.clone(),
            update: message,
        }) {
            tracing::trace!("could not send a message:\n{m:?}");
        }
    }

    /// you should probably use [Self::child] unless you're writing a custom extension
    pub fn span_raw(&self, span: ProgressSpan) -> Self {
        let this = Self {
            span: self.span.child(),
            communicator: self.communicator.clone(),
        };
        this.send(Update::Start(span));
        this
    }

    pub fn child(&self, name: impl Into<Cow<'static, str>>) -> Self {
        self.span_raw(ProgressSpan {
            kind: ProgressKind::Parent,
            name: name.into(),
            state: ProgressState { total: 0, current: 0 },
        })
    }
}

#[derive(Debug)]
pub struct ProgressSpan {
    pub name: Cow<'static, str>,
    pub state: ProgressState,
    pub kind: ProgressKind,
}

impl ProgressMap {
    pub fn new() -> (Self, Receiver, ProgressCommunicator) {
        let (rx, communicator) = ProgressCommunicator::new();
        (
            Self {
                progress: Default::default(),
                finished_pending: Default::default(),
            },
            rx,
            communicator,
        )
    }
}

const DELTA_NEW: ProgressDelta = ProgressDelta { total: 1, current: 0 };
const DELTA_FINISHED: ProgressDelta = ProgressDelta { total: 0, current: 1 };

// immutable access

mod immutable_access {
    use super::*;
    type Item<'a> = (&'a SpanPath, &'a ProgressSpan);

    impl ProgressMap {
        pub fn get<'a>(&'a self, span: &'_ SpanPath) -> Option<Item<'a>> {
            self.progress.get_key_value(span)
        }
        pub fn parent<'a>(&'a self, span: &'a SpanPath) -> Option<Item<'a>> {
            span.parent().and_then(|p| self.get(&p))
        }
        pub fn with_descendants<'a>(&'a self, span: &'a SpanPath) -> impl Iterator<Item = Item<'a>> + 'a {
            self.progress
                .range(span..)
                .skip(1)
                .take_while(|(i, _)| i.starts_with(span))
        }

        pub fn descendants<'a>(&'a self, span: &'a SpanPath) -> impl Iterator<Item = Item<'a>> + 'a {
            self.with_descendants(span).skip(1)
        }

        pub fn children<'a>(&'a self, span: &'a SpanPath) -> impl Iterator<Item = Item<'a>> + 'a {
            let child_len = span.len() + 1;
            self.descendants(span)
                .take_while(move |(e, _)| e.len() == child_len)
        }
        pub fn has_children(&self, span: &SpanPath) -> bool {
            self.children(span).next().is_some()
        }
    }
}

mod mutable_access {
    use super::*;
    type Item<'a> = (&'a SpanPath, &'a mut ProgressSpan);
    type GetItem<'b> = (SpanPath, &'b mut ProgressSpan);

    impl ProgressMap {
        pub fn get_mut<'a>(&'a mut self, span: &'_ SpanPath) -> Option<GetItem<'a>> {
            self.progress
                .get_mut(span)
                .map(|value| (span.clone(), value))
        }
        pub fn parent_mut<'a>(&'a mut self, span: &'_ SpanPath) -> Option<GetItem<'a>> {
            span.parent().and_then(|p| self.get_mut(&p))
        }
        pub fn with_descendants_mut<'a>(&'a mut self, span: &'a SpanPath) -> impl Iterator<Item = Item<'a>> + 'a {
            self.progress
                .range_mut(span..)
                .skip(1)
                .take_while(|(i, _)| i.starts_with(span))
        }

        pub fn descendants_mut<'a>(&'a mut self, span: &'a SpanPath) -> impl Iterator<Item = Item<'a>> + 'a {
            self.with_descendants_mut(span).skip(1)
        }

        pub fn children_mut<'a>(&'a mut self, span: &'a SpanPath) -> impl Iterator<Item = Item<'a>> + 'a {
            let child_len = span.len() + 1;
            self.descendants_mut(span)
                .take_while(move |(e, _)| e.len() == child_len)
        }
    }
}

// mutable access

impl ProgressMap {
    pub fn handle(&mut self, ProgressMessage { span, update }: ProgressMessage) {
        let parent = span.parent();

        match update {
            Update::Start(progress_state) => match self.progress.entry(span) {
                Entry::Vacant(vacant_entry) => {
                    vacant_entry.insert(progress_state);
                    if let Some(parent) = parent {
                        self.handle(ProgressMessage {
                            span: parent,
                            update: Update::Update(DELTA_NEW),
                        })
                    }
                }
                Entry::Occupied(occupied_entry) => occupied_entry.into_mut().pipe(|m| {
                    progress_state.pipe(|ProgressSpan { name, state, kind }| {
                        m.name = name;
                        m.kind = kind;
                        state
                            .pipe(|ProgressState { total, current }| ProgressDelta { total, current })
                            .apply(&mut m.state)
                    });
                }),
            },
            Update::Update(delta) => match self.progress.entry(span.clone()) {
                Entry::Vacant(vacant_entry) => {
                    vacant_entry.insert(ProgressSpan {
                        name: Cow::Borrowed("<unknown>"),
                        state: delta.pipe(|ProgressDelta { total, current }| ProgressState { total, current }),
                        kind: ProgressKind::Iter,
                    });
                    if let Some(parent) = parent {
                        self.handle(ProgressMessage {
                            span: parent,
                            update: Update::Update(DELTA_NEW),
                        })
                    }
                }
                Entry::Occupied(mut occupied_entry) => {
                    delta.apply(&mut occupied_entry.get_mut().state);
                    if occupied_entry
                        .get()
                        .pipe(|e| &e.state)
                        .pipe(|e| e.total == e.current)
                        && self.finished_pending.contains(&span)
                    {
                        self.progress.remove(&span);
                        self.finished_pending.remove(&span);
                        if let Some(parent) = parent {
                            self.handle(ProgressMessage {
                                span: parent,
                                update: Update::Update(DELTA_FINISHED),
                            })
                        }
                    }
                }
            },
            Update::Finish => match self.has_children(&span) {
                true => {
                    self.finished_pending.insert(span);
                }
                false => {
                    self.progress.remove(&span);
                    if let Some(parent) = parent {
                        self.handle(ProgressMessage {
                            span: parent,
                            update: Update::Update(DELTA_FINISHED),
                        })
                    }
                }
            },
        }
    }
}
