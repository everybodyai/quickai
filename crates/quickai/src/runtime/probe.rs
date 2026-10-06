//! The `probe.http@1` primitive: wait until an engine answers a request with the expected status.
//! A plan's `ready` step runs it; reaching the expected status is what turns `Starting` into
//! `Running`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use quickai_core::plan::ReadyPlan;

use super::process::send;
use crate::events::{Event, ServerState};

/// Polls `http://{host}:{port}{path}` every `interval` until it answers with `expect_status`, then
/// reports `Running`. Gives up after `timeout` with a log line and no state change (the process may
/// still be loading a big model; the user can wait or stop). A stop that was asked for while still
/// starting ends the poll quietly: reporting `Running` would be a stale, misleading transition.
pub fn spawn_http_ready(
    host: String,
    port: u16,
    ready: ReadyPlan,
    intentional_stop: Arc<AtomicBool>,
    evt_tx: Sender<Event>,
    wake: Arc<impl Fn() + Send + Sync + 'static>,
) {
    std::thread::spawn(move || {
        let url = format!("http://{host}:{port}{}", ready.path);
        let deadline = Instant::now() + Duration::from_millis(ready.timeout_ms);
        let interval = Duration::from_millis(ready.interval_ms);
        loop {
            if intentional_stop.load(Ordering::SeqCst) {
                return;
            }
            if Instant::now() > deadline {
                send(
                    &evt_tx,
                    Event::Log("Timed out waiting for server to become healthy.".into()),
                    &wake,
                );
                return;
            }
            if let Ok(resp) = ureq::get(&url).call() {
                if resp.status() == ready.expect_status {
                    send(&evt_tx, Event::StateChanged(ServerState::Running), &wake);
                    return;
                }
            }
            std::thread::sleep(interval);
        }
    });
}
