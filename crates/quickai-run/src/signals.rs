//! A person's "stop": Ctrl-C, the SIGTERM a service manager sends, the terminal hanging up (not for a run in the
//! background, which has none), and `quickai-run down`, which writes the stop file of the run's folder
//! (`run_file`). Each only sets a flag, which is all that is safe in a handler; the run watches the flag and stops
//! what it started, with the grace each program's document gives it. A second signal does not wait: it ends this
//! program at once.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Set when a stop is asked for.
pub static STOP: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn on_signal(_: libc::c_int) {
    if STOP.swap(true, Ordering::SeqCst) {
        // the second time: the person is not going to wait
        unsafe { libc::_exit(130) };
    }
}

/// Makes SIGINT and SIGTERM set `STOP`, and SIGHUP too unless the run is in the background (`background`), where
/// the end of the connection that started it is not a stop.
#[cfg(unix)]
pub fn install(background: bool) {
    let mut signals = vec![libc::SIGINT, libc::SIGTERM];
    if background {
        // SAFETY: SIG_IGN is a disposition the system takes as it is
        unsafe { libc::signal(libc::SIGHUP, libc::SIG_IGN) };
    } else {
        signals.push(libc::SIGHUP);
    }
    for signal in signals {
        // SAFETY: the handler touches nothing but an atomic and `_exit`, both safe in a handler, and the
        // `sigaction` is filled in whole before it is given to the system.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as usize;
            libc::sigemptyset(&mut action.sa_mask);
            action.sa_flags = 0;
            libc::sigaction(signal, &action, std::ptr::null_mut());
        }
    }
}

#[cfg(windows)]
unsafe extern "system" fn on_console(_kind: u32) -> windows_sys::core::BOOL {
    if STOP.swap(true, Ordering::SeqCst) {
        std::process::exit(130);
    }
    // handled: the run stops by itself
    1
}

/// Makes Ctrl-C, Ctrl-Break and the console closing set `STOP` (a run in the background has no console, and is
/// stopped by `down`).
#[cfg(windows)]
pub fn install(_background: bool) {
    // SAFETY: the handler touches nothing but an atomic, and the system calls it on a thread of its own
    unsafe {
        windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(on_console), 1);
    }
}

/// Sets `STOP` when `down` asks for it: the stop file of the run's folder `dir` is looked for each second, on a
/// thread of its own, for as long as this program lives.
pub fn watch_stop_file(dir: PathBuf) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        if quickai_runtime::run_file::take_stop(&dir) {
            STOP.store(true, Ordering::SeqCst);
        }
    });
}
