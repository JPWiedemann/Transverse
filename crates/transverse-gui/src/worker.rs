//! Runs an analysis or synthesis on a background thread with progress and cancel.
//!
//! The frontend starts a [`Worker`], polls it from its event loop and calls [`Worker::cancel`] on
//! user request. `wake` is invoked whenever there is something to poll (e.g. to repaint).

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use transverse_core::{Ctl, TvError};
use transverse_lines::model::{Analysis, LineModel, ParamSet};

pub enum Job {
    Analyze {
        params: ParamSet,
    },
    /// Find dimension `target_id` that gives `z0` Ohm; with `phase_deg` the line length is set for
    /// that electrical length as well.
    Synthesize {
        params: ParamSet,
        target_id: String,
        z0: f64,
        phase_deg: Option<f64>,
    },
}

pub struct JobOutput {
    pub analysis: Analysis,
    /// Parameters found by a synthesis (None for a plain analysis).
    pub new_params: Option<ParamSet>,
}

pub enum Msg {
    Progress(String),
    Done(Box<JobOutput>),
    Cancelled,
    Error(String),
}

pub struct Worker {
    rx: Receiver<Msg>,
    ctl: Ctl,
    started: Instant,
    handle: Option<JoinHandle<()>>,
    finished: bool,
}

impl Worker {
    pub fn start(model: Arc<dyn LineModel>, job: Job, wake: impl Fn() + Send + Sync + 'static) -> Self {
        let (tx, rx) = channel();
        let wake = Arc::new(wake);
        let ctl = {
            let (tx, wake) = (tx.clone(), wake.clone());
            Ctl::new().with_progress(move |m| {
                let _ = tx.send(Msg::Progress(m.to_string()));
                wake();
            })
        };
        let run_ctl = ctl.clone();
        let handle = std::thread::spawn(move || {
            let result = match job {
                Job::Analyze { params } => {
                    model.analyze(&params, &run_ctl).map(|analysis| JobOutput { analysis, new_params: None })
                }
                Job::Synthesize { params, target_id, z0, phase_deg } => model
                    .synthesize(&params, &target_id, z0, phase_deg, &run_ctl)
                    .map(|(p, analysis)| JobOutput { analysis, new_params: Some(p) }),
            };
            let msg = match result {
                Ok(out) => Msg::Done(Box::new(out)),
                Err(TvError::Cancelled) => Msg::Cancelled,
                Err(e) => Msg::Error(e.to_string()),
            };
            let _ = tx.send(msg);
            wake();
        });
        Self { rx, ctl, started: Instant::now(), handle: Some(handle), finished: false }
    }

    /// Ask the job to stop; it ends after the current linear solve.
    pub fn cancel(&self) {
        self.ctl.cancel();
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// True once a final message (done, cancelled, error) has been returned by `poll`/`recv`.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    fn note(&mut self, m: &Msg) {
        if !matches!(m, Msg::Progress(_)) {
            self.finished = true;
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
        }
    }

    /// All messages that arrived since the last call (non-blocking).
    pub fn poll(&mut self) -> Vec<Msg> {
        let mut out = Vec::new();
        if self.finished {
            return out;
        }
        loop {
            match self.rx.try_recv() {
                Ok(m) => {
                    self.note(&m);
                    let last = !matches!(m, Msg::Progress(_));
                    out.push(m);
                    if last {
                        break;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    let m = Msg::Error("worker thread terminated unexpectedly".into());
                    self.note(&m);
                    out.push(m);
                    break;
                }
            }
        }
        out
    }

    /// Wait for the next message (blocking). None once the job has finished.
    pub fn recv(&mut self) -> Option<Msg> {
        if self.finished {
            return None;
        }
        let m = self.rx.recv().unwrap_or_else(|_| Msg::Error("worker thread terminated unexpectedly".into()));
        self.note(&m);
        Some(m)
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // a window closed during a job must not leave the solver running
        self.ctl.cancel();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use transverse_lines::cpwg_inner::CpwgInner;
    use transverse_lines::model::ParamValue;

    fn run_to_end(w: &mut Worker) -> (Vec<String>, Msg) {
        let mut progress = Vec::new();
        loop {
            match w.recv().expect("finished early") {
                Msg::Progress(p) => progress.push(p),
                other => return (progress, other),
            }
        }
    }

    #[test]
    fn analyze_reports_progress_and_result() {
        let m: Arc<dyn LineModel> = Arc::new(CpwgInner);
        let wakes = Arc::new(AtomicUsize::new(0));
        let wk = wakes.clone();
        let mut w = Worker::start(m.clone(), Job::Analyze { params: m.defaults() }, move || {
            wk.fetch_add(1, Ordering::Relaxed);
        });
        let (progress, end) = run_to_end(&mut w);
        let Msg::Done(out) = end else { panic!("not done") };
        assert!((out.analysis.modes[0].z0 / 35.001 - 1.0).abs() < 5e-4 && out.new_params.is_none());
        assert!(progress.iter().any(|p| p.contains("mesh level 1")), "{progress:?}");
        assert!(wakes.load(Ordering::Relaxed) > progress.len());
        assert!(w.is_finished() && w.recv().is_none() && w.poll().is_empty());
    }

    #[test]
    fn cancel_before_and_during_a_run() {
        let m: Arc<dyn LineModel> = Arc::new(CpwgInner);
        let mut w = Worker::start(m.clone(), Job::Analyze { params: m.defaults() }, || {});
        w.cancel();
        let (_, end) = run_to_end(&mut w);
        assert!(matches!(end, Msg::Cancelled));

        // a long synthesis is stopped at the next linear solve
        let mut set = m.defaults();
        set.set("tol", ParamValue::Num(0.001));
        let mut w = Worker::start(
            m.clone(),
            Job::Synthesize { params: set, target_id: "w".into(), z0: 50.0, phase_deg: None },
            || {},
        );
        loop {
            match w.recv().unwrap() {
                Msg::Progress(_) => w.cancel(),
                Msg::Cancelled => break,
                Msg::Done(_) => panic!("finished despite cancel"),
                Msg::Error(e) => panic!("{e}"),
            }
        }
    }

    #[test]
    fn synthesis_returns_new_parameters() {
        let m: Arc<dyn LineModel> = Arc::new(CpwgInner);
        let job =
            Job::Synthesize { params: m.defaults(), target_id: "w".into(), z0: 35.0, phase_deg: Some(90.0) };
        let mut w = Worker::start(m, job, || {});
        let (_, end) = run_to_end(&mut w);
        let Msg::Done(out) = end else { panic!("not done") };
        let p = out.new_params.unwrap();
        assert!((p.num("w").unwrap() / 0.3605 - 1.0).abs() < 5e-4);
        assert!(out.analysis.report.contains("Synthesis: Z0 = 35 Ohm"));
        // the length follows from the 90 degrees (a quarter wavelength), not from the old 100 mm
        let mode = &out.analysis.modes[0];
        assert!((mode.phase_deg - 90.0).abs() < 1e-9);
        assert!((p.num("length").unwrap() - mode.wavelength_mm / 4.0).abs() < 1e-9);
    }

    #[test]
    fn invalid_input_is_an_error_message() {
        let m: Arc<dyn LineModel> = Arc::new(CpwgInner);
        let mut set = m.defaults();
        set.set("w", ParamValue::Num(-1.0));
        let mut w = Worker::start(m, Job::Analyze { params: set }, || {});
        let (_, end) = run_to_end(&mut w);
        let Msg::Error(e) = end else { panic!("no error") };
        assert!(e.contains("w must be > 0"), "{e}");
    }

    #[test]
    fn poll_is_non_blocking_and_collects_in_order() {
        let m: Arc<dyn LineModel> = Arc::new(CpwgInner);
        let mut w = Worker::start(m.clone(), Job::Analyze { params: m.defaults() }, || {});
        let mut seen_done = false;
        for _ in 0..2000 {
            for msg in w.poll() {
                if matches!(msg, Msg::Done(_)) {
                    seen_done = true;
                }
            }
            if seen_done {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(seen_done && w.is_finished());
    }
}
