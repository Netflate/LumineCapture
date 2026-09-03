// Runs OCR off the UI thread to keep the app responsive.
//
// Model loading and text recognition are slow (100+ ms), so they run on a
// single background worker thread. Only one job runs at a time.
//
// Highlights:
// - Non-blocking: Requests sent before the engine finishes loading are queued.
// - Error handling: If loading fails, future requests are rejected instantly
//   without retrying on the UI thread.

use std::sync::mpsc::{Receiver, TryRecvError, channel};

use super::models::ModelFiles;
use super::{OcrBackend, OcrError, OcrImage, OcrText, default_backend};

type JobDone = (Box<dyn OcrBackend>, Result<OcrText, String>);
type WarmDone = Result<Box<dyn OcrBackend>, String>;

pub enum StartOutcome {
    /// Running, or parked until the engine is ready.
    Started,
    /// A recognition is already in flight.
    Busy,
    /// The engine could not be built (see the contained message).
    Unavailable(String),
}

pub struct OcrRuntime {
    backend: Option<Box<dyn OcrBackend>>,
    warm_rx: Option<Receiver<WarmDone>>,
    job_rx: Option<Receiver<JobDone>>,
    /// Parked until the engine is ready.
    queued: Option<OcrImage>,
    /// Set if the engine fails to build; later requests fail fast.
    failed: Option<String>,
    /// we can't kill the thread, so we wait for it to finish, and only then
    /// return the engine with discarding the text
    discarded: bool,
    // Engine in the running task was built with the previous model; do not reuse it further
    stale: bool,
}

impl Default for OcrRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl OcrRuntime {
    /// starts empty; `load` builds the engine on a background thread.
    pub fn new() -> Self {
        Self {
            backend: None,
            warm_rx: None,
            job_rx: None,
            queued: None,
            failed: None,
            discarded: false,
            stale: false,
        }
    }

    pub fn load(&mut self, files: ModelFiles) {
        self.load_with(move || default_backend(&files));
    }

    fn load_with<F>(&mut self, build: F)
    where
        F: FnOnce() -> Result<Box<dyn OcrBackend>, OcrError> + Send + 'static,
    {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(build().map_err(|e| e.to_string()));
        });
        self.warm_rx = Some(rx);
        self.backend = None;
        self.failed = None;
        self.stale = self.job_rx.is_some();
    }

    /// A scan somebody is still waiting for: running, or parked until the
    /// engine is ready. A cancelled one does not count.
    pub fn is_busy(&self) -> bool {
        (self.job_rx.is_some() && !self.discarded) || self.queued.is_some()
    }

    pub fn needs_poll(&self) -> bool {
        self.job_rx.is_some() || self.queued.is_some()
    }

    pub fn cancel(&mut self) {
        self.queued = None;
        if self.job_rx.is_some() {
            self.discarded = true;
        }
    }

    /// Start recognizing `image`. Parks the request if the engine isn't ready.
    pub fn start(&mut self, image: OcrImage) -> StartOutcome {
        if let Some(err) = &self.failed {
            return StartOutcome::Unavailable(err.clone());
        }
        if self.is_busy() {
            return StartOutcome::Busy;
        }
        // Hold off sending the new engine to the thread until the old task returns 
        // `stale` tracks whether the old task is still running.
        if self.job_rx.is_none()
            && let Some(backend) = self.backend.take()
        {
            self.spawn(backend, image);
        } else {
            self.queued = Some(image);
        }
        StartOutcome::Started
    }

    /// Once per event-loop iteration. Returns the text when a job finishes,
    /// or the reason it could not run.
    pub fn poll(&mut self) -> Option<Result<OcrText, String>> {
        self.collect_warm();

        // Engine arrived while a request was parked.
        if self.job_rx.is_none()
            && let Some(image) = self.queued.take()
        {
            match self.backend.take() {
                Some(backend) => self.spawn(backend, image),
                None => self.queued = Some(image),
            }
        }

        // Engine failed while a request was parked.
        if self.queued.is_some()
            && let Some(err) = self.failed.clone()
        {
            self.queued = None;
            return Some(Err(err));
        }

        let rx = self.job_rx.as_ref()?;
        let (done, out) = match rx.try_recv() {
            Ok((backend, result)) => {
                if !self.stale {
                    self.backend = Some(backend);
                }
                (true, Some(result))
            }
            Err(TryRecvError::Empty) => (false, None),
            Err(TryRecvError::Disconnected) => {
                (true, Some(Err("recognition thread vanished".into())))
            }
        };
        if !done {
            return None;
        }
        self.job_rx = None;
        self.stale = false;

        if self.discarded {
            self.discarded = false;
            return None;
        }
        out
    }

    fn spawn(&mut self, backend: Box<dyn OcrBackend>, image: OcrImage) {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let result = backend.recognize(image).map_err(|e| e.to_string());
            let _ = tx.send((backend, result));
        });
        self.job_rx = Some(rx);
    }

    /// Collect the backend if the build thread is done. Non-blocking.
    fn collect_warm(&mut self) {
        let Some(rx) = &self.warm_rx else { return };
        match rx.try_recv() {
            Ok(Ok(backend)) => {
                self.backend = Some(backend);
                self.warm_rx = None;
            }
            Ok(Err(e)) => {
                eprintln!("ocr: failed to initialise engine: {e}");
                self.failed = Some(e);
                self.warm_rx = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.failed = Some("engine build thread vanished".into());
                self.warm_rx = None;
            }
        }
    }
}