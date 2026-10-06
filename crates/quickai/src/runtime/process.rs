//! The `spawn@1` primitive's supporting threads: stream a child's output into the log, notice when it
//! exits, and a few small helpers shared by the supervisor. The thread-plus-channel idiom is the
//! same one `downloader.rs` and the proxy use: workers send `Event`s, the UI drains them once per
//! frame.

use std::io::{BufRead, BufReader};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::Sender;

use crate::events::{Event, ServerState};

const WAITER_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Sends `event` to the UI and wakes it so it repaints promptly.
pub fn send(tx: &Sender<Event>, event: Event, wake: &Arc<impl Fn() + Send + Sync + 'static>) {
    let _ = tx.send(event);
    wake();
}

/// Asks the OS for a free loopback port by briefly binding to port 0, then
/// releasing it so the engine can bind it moments later. There's a small
/// theoretical race if something else grabs the exact same port in between,
/// same as any "find a free port" trick — if it happens, the engine just
/// fails to bind and that surfaces in the log pane like any other failure.
pub fn ephemeral_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    listener.local_addr().map(|a| a.port())
}

/// Forwards each line of a child's output to the log.
pub fn spawn_reader<R: std::io::Read + Send + 'static>(
    pipe: Option<R>,
    evt_tx: Sender<Event>,
    wake: Arc<impl Fn() + Send + Sync + 'static>,
) {
    let Some(pipe) = pipe else { return };
    std::thread::spawn(move || {
        let reader = BufReader::new(pipe);
        for line in reader.lines() {
            match line {
                Ok(line) => send(&evt_tx, Event::Log(line), &wake),
                Err(_) => break,
            }
        }
    });
}

/// Watches the child and reports when it ends: `Idle` after a stop that was asked for, `Crashed`
/// otherwise.
pub fn spawn_waiter(
    child: Arc<Mutex<Option<Child>>>,
    intentional_stop: Arc<AtomicBool>,
    evt_tx: Sender<Event>,
    wake: Arc<impl Fn() + Send + Sync + 'static>,
) {
    std::thread::spawn(move || loop {
        std::thread::sleep(WAITER_POLL_INTERVAL);
        let mut guard = child.lock().unwrap();
        let Some(c) = guard.as_mut() else { return };
        match c.try_wait() {
            Ok(Some(status)) => {
                *guard = None;
                drop(guard);
                let was_intentional = intentional_stop.load(Ordering::SeqCst);
                if was_intentional {
                    send(
                        &evt_tx,
                        Event::Log(format!("Server stopped (exit status: {status}).")),
                        &wake,
                    );
                    send(&evt_tx, Event::StateChanged(ServerState::Idle), &wake);
                } else {
                    send(
                        &evt_tx,
                        Event::Log(format!(
                            "Server process exited unexpectedly (exit status: {status})."
                        )),
                        &wake,
                    );
                    send(&evt_tx, Event::StateChanged(ServerState::Crashed), &wake);
                }
                return;
            }
            Ok(None) => continue, // still running
            Err(_) => return,
        }
    });
}
