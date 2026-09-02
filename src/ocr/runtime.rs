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

use super::{OcrBackend, OcrImage, OcrText, default_backend};

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
}

impl Default for OcrRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl OcrRuntime {
    /// starts building on a background thread.
    pub fn new() -> Self {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(default_backend().map_err(|e| e.to_string()));
        });
        Self {
            backend: None,
            warm_rx: Some(rx),
            job_rx: None,
            queued: None,
            failed: None,
            discarded: false,
        }
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
        match self.backend.take() {
            Some(backend) => self.spawn(backend, image),
            None => self.queued = Some(image),
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
                self.backend = Some(backend);
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

#[cfg(test)]
mod state_machine {
    use super::*;
    use crate::ocr::{OcrError, OcrLine};

    struct Fake;
    impl OcrBackend for Fake {
        fn recognize(&self, _image: OcrImage) -> Result<OcrText, OcrError> {
            Ok(OcrText {
                lines: vec![OcrLine {
                    text: "ok".into(),
                    bounds: tiny_skia::Rect::from_xywh(0.0, 0.0, 10.0, 10.0).unwrap(),
                    char_x: vec![0.0, 5.0, 10.0],
                }],
            })
        }
    }

    fn warm() -> OcrRuntime {
        let (tx, rx) = channel();
        tx.send(Ok(Box::new(Fake) as Box<dyn OcrBackend>)).unwrap();
        OcrRuntime {
            backend: None,
            warm_rx: Some(rx),
            job_rx: None,
            queued: None,
            failed: None,
            discarded: false,
        }
    }

    fn image() -> OcrImage {
        OcrImage { rgb: vec![0; 3], width: 1, height: 1, origin: (0.0, 0.0) }
    }

    fn drain(rt: &mut OcrRuntime) -> Option<Result<OcrText, String>> {
        for _ in 0..200 {
            if let Some(r) = rt.poll() {
                return Some(r);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        None
    }

    #[test]
    fn idle_polling_does_not_lose_the_engine() {
        let mut rt = warm();
        for _ in 0..10 {
            assert!(rt.poll().is_none());
        }
        assert!(matches!(rt.start(image()), StartOutcome::Started));
        let out = drain(&mut rt).expect("a result must come back");
        assert_eq!(out.unwrap().lines[0].text, "ok");
    }

    #[test]
    fn request_before_the_engine_is_ready_still_runs() {
        let (tx, rx) = channel();
        let mut rt = OcrRuntime {
            backend: None,
            warm_rx: Some(rx),
            job_rx: None,
            queued: None,
            failed: None,
            discarded: false,
        };
        assert!(matches!(rt.start(image()), StartOutcome::Started));
        assert!(rt.is_busy(), "a parked request counts as busy");
        for _ in 0..5 {
            assert!(rt.poll().is_none());
            assert!(rt.is_busy(), "the parked request must survive polling");
        }
        tx.send(Ok(Box::new(Fake) as Box<dyn OcrBackend>)).unwrap();
        let out = drain(&mut rt).expect("parked request must run once the engine lands");
        assert_eq!(out.unwrap().lines[0].text, "ok");
    }

    #[test]
    fn two_jobs_in_a_row() {
        let mut rt = warm();
        for _ in 0..2 {
            assert!(matches!(rt.start(image()), StartOutcome::Started));
            assert!(drain(&mut rt).expect("result").is_ok());
            assert!(!rt.is_busy());
        }
    }

    #[test]
    fn a_cancelled_job_is_silent_but_hands_the_engine_back() {
        let mut rt = warm();
        assert!(rt.poll().is_none());
        assert!(matches!(rt.start(image()), StartOutcome::Started));

        rt.cancel();
        assert!(!rt.is_busy(), "nobody waits for a cancelled scan");

        for _ in 0..200 {
            if !rt.needs_poll() {
                break;
            }
            assert!(rt.poll().is_none(), "the cancelled result must not surface");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!rt.needs_poll(), "the worker must have been collected");

        assert!(matches!(rt.start(image()), StartOutcome::Started));
        assert!(drain(&mut rt).expect("result").is_ok());
    }

    #[test]
    fn a_scan_started_over_a_cancelled_one_still_runs() {
        let mut rt = warm();
        assert!(rt.poll().is_none());
        rt.start(image());
        rt.cancel();

        assert!(matches!(rt.start(image()), StartOutcome::Started));
        assert!(rt.is_busy(), "the replacement is live even while the old one drains");
        let out = drain(&mut rt).expect("the replacement must come back");
        assert_eq!(out.unwrap().lines[0].text, "ok");
    }

    #[test]
    fn failed_build_is_reported_then_refused() {
        let (tx, rx) = channel();
        let mut rt = OcrRuntime {
            backend: None,
            warm_rx: Some(rx),
            job_rx: None,
            queued: None,
            failed: None,
            discarded: false,
        };
        rt.start(image());
        tx.send(Err("boom".into())).unwrap();
        let out = drain(&mut rt).expect("the failure must surface");
        assert_eq!(out.unwrap_err(), "boom");
        assert!(!rt.is_busy());
        assert!(matches!(rt.start(image()), StartOutcome::Unavailable(_)));
    }
}
