//! Cancel flag and progress callback, passed through all long-running calls.
//!
//! Nothing here assumes how the host runs the work (thread, Web Worker): the flag is an
//! atomic and the progress sink is a plain callback.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::error::{TvError, TvResult};

type ProgressFn = dyn Fn(&str) + Send + Sync;

#[derive(Clone)]
pub struct Ctl {
    cancel: Arc<AtomicBool>,
    progress: Option<Arc<ProgressFn>>,
    n_solves: Arc<AtomicUsize>,
}

impl Default for Ctl {
    fn default() -> Self {
        Self::new()
    }
}

impl Ctl {
    pub fn new() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            progress: None,
            n_solves: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn with_progress(mut self, f: impl Fn(&str) + Send + Sync + 'static) -> Self {
        self.progress = Some(Arc::new(f));
        self
    }

    /// Request cancellation (callable from any clone, e.g. the GUI thread).
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn check(&self) -> TvResult<()> {
        if self.cancel.load(Ordering::Relaxed) { Err(TvError::Cancelled) } else { Ok(()) }
    }

    pub fn msg(&self, text: &str) {
        if let Some(f) = &self.progress {
            f(text);
        }
    }

    pub(crate) fn count_solve(&self) {
        self.n_solves.fetch_add(1, Ordering::Relaxed);
    }

    /// Number of linear solves so far (diagnostics).
    pub fn n_solves(&self) -> usize {
        self.n_solves.load(Ordering::Relaxed)
    }
}
