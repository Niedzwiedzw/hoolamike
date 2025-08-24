use {
    crate::progress_state::Progress,
    parking_lot::Mutex,
    std::{
        borrow::Cow,
        sync::{mpsc::Receiver, Arc},
    },
};

pub mod progress_state {
    pub struct Progress {
        pub total: usize,
        pub current: usize,
    }
}

pub enum ProgressEvent {
    Added(Progress),
    AdvancedBy(usize),
    Finished(Progress),
}

pub struct ProgressSpan {
    pub name: Cow<'static, str>,
    pub in_progress: usize,
    pub progress: progress_state::Progress,
    pub buffer: Receiver<ProgressEvent>,
    pub parent: Option<Arc<Mutex<Self>>>,
}
