//! Background jobs: a download, a local import, a runtime install (and, later, a model pull). One
//! worker thread reports to the UI over a channel that is drained once per frame, the same
//! thread-plus-channel shape as `ServerManager`. The UI holds a `Job<T>`; the worker gets a `send`
//! function.

use crossbeam_channel::Receiver;

use crate::hardware::format_bytes;

/// What a worker tells the UI while it runs. `T` is what a successful job produces: the file name
/// of a download, the binary of an install.
pub enum JobEvent<T> {
    Progress {
        downloaded: u64,
        total: Option<u64>,
    },
    /// How far along it is, from 0 to 1, when bytes are not what the work is counted in (a model
    /// pull adds up layers the engine announces as it goes).
    Fraction(f32),
    /// A line about what the job is doing now (`Unpacking ollama-linux-amd64.tar.zst`, or an
    /// engine's own words while it pulls a model), for the log.
    Status(String),
    /// What it is doing right now, in more words than the progress line has (the step, the file,
    /// how fast): each one replaces the one before, and none is kept in the log.
    Detail(String),
    Done(T),
    Failed(String),
}

/// How far along `downloaded` of `total` is, from 0 to 1: more than the total announced is all of
/// it, and a total that is not there, or is 0, says nothing about how far along it is.
pub fn fraction_of(downloaded: u64, total: Option<u64>) -> Option<f32> {
    total
        .filter(|t| *t > 0)
        .map(|total| (downloaded as f64 / total as f64).min(1.0) as f32)
}

/// A running job as the UI holds it: the receiving end of the channel plus what the progress line
/// needs.
pub struct Job<T> {
    rx: Receiver<JobEvent<T>>,
    pub label: String,
    /// "Downloading", "Importing" or "Installing": the verb of the progress line, so a local file
    /// copy does not claim to be a network download.
    pub verb: &'static str,
    pub downloaded: u64,
    pub total: Option<u64>,
    /// Set instead of the two above by a job that reports a fraction.
    fraction_only: Option<f32>,
    /// What the job has said since the interface last asked (`take_statuses`).
    statuses: Vec<String>,
    /// The last thing it said about what it is doing right now (`JobEvent::Detail`).
    detail: Option<String>,
}

impl<T: Send + 'static> Job<T> {
    /// Runs `work` on a new thread and returns at once. Every event `work` passes to its `send`
    /// reaches the UI, and `wake` is called after each so the UI repaints promptly.
    pub fn spawn(
        label: String,
        verb: &'static str,
        wake: impl Fn() + Send + Sync + 'static,
        work: impl FnOnce(&dyn Fn(JobEvent<T>)) + Send + 'static,
    ) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        std::thread::spawn(move || {
            let send = |event: JobEvent<T>| {
                let _ = tx.send(event);
                wake();
            };
            work(&send);
        });
        Self {
            rx,
            label,
            verb,
            downloaded: 0,
            total: None,
            fraction_only: None,
            statuses: Vec::new(),
            detail: None,
        }
    }
}

impl<T> Job<T> {
    /// Drains the channel once: updates the progress counters and returns the outcome when the job
    /// has finished, `None` while it is still running. Call once per frame.
    pub fn poll(&mut self) -> Option<Result<T, String>> {
        let mut outcome = None;
        while let Ok(event) = self.rx.try_recv() {
            match event {
                JobEvent::Progress { downloaded, total } => {
                    self.downloaded = downloaded;
                    self.total = total;
                }
                JobEvent::Fraction(f) => self.fraction_only = Some(f.clamp(0.0, 1.0)),
                JobEvent::Status(line) => self.statuses.push(line),
                JobEvent::Detail(text) => self.detail = Some(text),
                JobEvent::Done(value) => outcome = Some(Ok(value)),
                JobEvent::Failed(message) => outcome = Some(Err(message)),
            }
        }
        outcome
    }

    /// The lines the job has said since the last call, oldest first: for the log pane.
    pub fn take_statuses(&mut self) -> Vec<String> {
        std::mem::take(&mut self.statuses)
    }

    /// What the job is doing right now, if it has said: shown under the progress line.
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    /// The progress line: "Downloading model.gguf: 1.2 GB / 4.0 GB", or without the total when the
    /// server did not announce one.
    pub fn progress_line(&self) -> String {
        if let Some(fraction) = self.fraction_only {
            return format!(
                "{} {}: {}%",
                self.verb,
                self.label,
                (fraction * 100.0).floor() as u32
            );
        }
        match self.total.filter(|t| *t > 0) {
            Some(total) => format!(
                "{} {}: {} / {}",
                self.verb,
                self.label,
                format_bytes(self.downloaded),
                format_bytes(total)
            ),
            None => format!(
                "{} {}: {}",
                self.verb,
                self.label,
                format_bytes(self.downloaded)
            ),
        }
    }

    /// How far along it is, for a progress bar; `None` when the total is not known.
    pub fn fraction(&self) -> Option<f32> {
        if self.fraction_only.is_some() {
            return self.fraction_only;
        }
        fraction_of(self.downloaded, self.total)
    }

    /// Blocks until the job finishes or `timeout` passes. For tests, which have no frame loop.
    #[cfg(test)]
    pub fn wait(&mut self, timeout: std::time::Duration) -> Option<Result<T, String>> {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            if let Some(outcome) = self.poll() {
                return Some(outcome);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn progress_then_done_is_reported_in_order_and_wakes_the_ui() {
        let wakes = Arc::new(AtomicUsize::new(0));
        let counted = wakes.clone();
        let mut job = Job::spawn(
            "model.gguf".into(),
            "Downloading",
            move || {
                counted.fetch_add(1, Ordering::SeqCst);
            },
            |send| {
                send(JobEvent::Progress {
                    downloaded: 10,
                    total: Some(100),
                });
                send(JobEvent::Done("model.gguf".to_string()));
            },
        );
        let outcome = job.wait(Duration::from_secs(5)).expect("the job finishes");
        assert_eq!(outcome.unwrap(), "model.gguf");
        assert_eq!((job.downloaded, job.total), (10, Some(100)));
        assert_eq!(wakes.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_failure_carries_its_message() {
        let mut job: Job<()> = Job::spawn(
            "x".into(),
            "Installing",
            || {},
            |send| {
                send(JobEvent::Failed("disk full".to_string()));
            },
        );
        let outcome = job.wait(Duration::from_secs(5)).expect("the job finishes");
        assert_eq!(outcome.unwrap_err(), "disk full");
    }

    #[test]
    fn poll_is_none_while_the_job_is_running() {
        let (release_tx, release_rx) = crossbeam_channel::bounded::<()>(0);
        let mut job: Job<u8> = Job::spawn(
            "slow".into(),
            "Installing",
            || {},
            move |send| {
                let _ = release_rx.recv();
                send(JobEvent::Done(7));
            },
        );
        assert!(job.poll().is_none());
        release_tx.send(()).unwrap();
        assert_eq!(job.wait(Duration::from_secs(5)).unwrap().unwrap(), 7);
    }

    #[test]
    fn p3_w02c_a_total_of_zero_says_nothing_and_more_than_the_total_is_all_of_it() {
        assert_eq!(fraction_of(5, Some(0)), None, "a total of 0");
        assert_eq!(fraction_of(5, None), None, "no total announced");
        assert_eq!(fraction_of(0, Some(100)), Some(0.0));
        assert_eq!(fraction_of(25, Some(100)), Some(0.25));
        assert_eq!(fraction_of(100, Some(100)), Some(1.0), "exactly the total");
        assert_eq!(
            fraction_of(150, Some(100)),
            Some(1.0),
            "more than the total"
        );
        assert_eq!(fraction_of(u64::MAX, Some(1)), Some(1.0));
        assert_eq!(fraction_of(u64::MAX, Some(u64::MAX)), Some(1.0));
        // and a job reports the same
        let mut job: Job<()> = Job::spawn("m.gguf".into(), "Downloading", || {}, |_| {});
        job.downloaded = 3_000;
        job.total = Some(2_000);
        assert_eq!(job.fraction(), Some(1.0));
    }

    #[test]
    fn p3_f77_a_job_says_what_it_is_doing_in_a_line_that_replaces_the_last_and_is_not_logged() {
        let mut job: Job<u8> = Job::spawn(
            "v1".into(),
            "Installing",
            || {},
            |send| {
                send(JobEvent::Detail("step 1 of 3".into()));
                send(JobEvent::Status("Using Python 3.13".into()));
                send(JobEvent::Detail("step 2 of 3".into()));
                send(JobEvent::Done(1));
            },
        );
        assert_eq!(job.detail(), None, "nothing said yet");
        assert!(job.wait(Duration::from_secs(5)).is_some());
        assert_eq!(
            job.detail(),
            Some("step 2 of 3"),
            "the last one is the one kept"
        );
        assert_eq!(
            job.take_statuses(),
            ["Using Python 3.13"],
            "a detail is not a line of the log"
        );
    }

    #[test]
    fn the_progress_line_matches_what_the_panels_showed() {
        let mut job: Job<()> = Job::spawn("m.gguf".into(), "Downloading", || {}, |_| {});
        job.downloaded = 1024 * 1024;
        assert_eq!(job.progress_line(), "Downloading m.gguf: 1.0 MB");
        assert_eq!(job.fraction(), None);
        job.total = Some(4 * 1024 * 1024);
        assert_eq!(job.progress_line(), "Downloading m.gguf: 1.0 MB / 4.0 MB");
        assert_eq!(job.fraction(), Some(0.25));
        job.total = Some(0);
        assert_eq!(job.progress_line(), "Downloading m.gguf: 1.0 MB");
        assert_eq!(job.fraction(), None);
    }
}
