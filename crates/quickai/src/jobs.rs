//! Background jobs: a download, a local import, a runtime install (and, later, a model pull). One
//! worker thread reports to the UI over a channel that is drained once per frame, the same
//! thread-plus-channel shape as `ServerManager`. The UI holds a `Job<T>`; the worker gets a `send`
//! function.

use crossbeam_channel::Receiver;

use crate::hardware::format_bytes;

/// What a worker tells the UI while it runs. `T` is what a successful job produces: the file name
/// of a download, the binary of an install.
pub enum JobEvent<T> {
    Progress { downloaded: u64, total: Option<u64> },
    Done(T),
    Failed(String),
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
                JobEvent::Done(value) => outcome = Some(Ok(value)),
                JobEvent::Failed(message) => outcome = Some(Err(message)),
            }
        }
        outcome
    }

    /// The progress line: "Downloading model.gguf: 1.2 GB / 4.0 GB", or without the total when the
    /// server did not announce one.
    pub fn progress_line(&self) -> String {
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
        self.total
            .filter(|t| *t > 0)
            .map(|total| self.downloaded as f32 / total as f32)
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
