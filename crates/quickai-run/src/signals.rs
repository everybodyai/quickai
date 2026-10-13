//! A person's "stop": Ctrl-C, or the SIGTERM a service manager sends. The handler only sets a flag, which is
//! all that is safe in one; the run watches the flag and stops what it started, with the grace each program's
//! document gives it. A second signal does not wait: it ends this program at once.

#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};

/// Set when a stop is asked for.
#[cfg(unix)]
pub static STOP: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn on_signal(_: libc::c_int) {
    if STOP.swap(true, Ordering::SeqCst) {
        // the second time: the person is not going to wait
        unsafe { libc::_exit(130) };
    }
}

/// Makes SIGINT, SIGTERM and SIGHUP set `STOP`.
#[cfg(unix)]
pub fn install() {
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
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
