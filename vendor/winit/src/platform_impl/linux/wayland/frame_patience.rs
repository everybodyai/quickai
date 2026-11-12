// Changed for QuickAI, 2026-11-12: this file is added. See vendor/README.md in QuickAI's source.
//! How long a window's redraw waits for the compositor's frame callback before it goes through anyway.
//!
//! winit holds a window's `RedrawRequested` back from the moment the application says it is about to present
//! (`Window::pre_present_notify`) until the compositor's frame callback says it wants the next frame. A compositor
//! sends no frame callback to a window it does not draw (covered by another, minimized, on another workspace), so that
//! window is not redrawn until it is shown again. An application that draws its other windows in that window's redraw
//! (egui's immediate viewports) then stops drawing them too, and one that watches its own frames takes that for a hang.
//!
//! With a patience set, a redraw that waited that long goes through, and the next one waits the patience again: a
//! window the compositor does not draw is drawn at most once a patience while the application asks for it, and one that
//! it draws is paced by its frame callbacks as before. One frame callback at a time stays in flight. Without a patience
//! (the default) a redraw waits for the callback for as long as it takes, as winit is published.
//!
//! It is for an application whose presents do not wait for the compositor (a mailbox or immediate present mode): a
//! present that waits for the frame callback of a window that is not drawn waits for as long as the window is hidden.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::window::state::FrameCallbackState;

/// The patience, in microseconds; 0 is none (a redraw waits for its frame callback for ever).
static PATIENCE_MICROS: AtomicU64 = AtomicU64::new(0);

/// Sets the patience of every window, from now on.
pub fn set(patience: Option<Duration>) {
    let micros = patience.map_or(0, |p| p.as_micros().clamp(1, u128::from(u64::MAX)) as u64);
    PATIENCE_MICROS.store(micros, Ordering::Relaxed);
}

/// The patience set, if any.
pub fn get() -> Option<Duration> {
    match PATIENCE_MICROS.load(Ordering::Relaxed) {
        0 => None,
        micros => Some(Duration::from_micros(micros)),
    }
}

/// When a redraw held back for the frame callback asked for at `asked_at` goes through anyway: never while no callback
/// is in flight (nothing is held back then), and never without a patience.
pub fn held_back_until(
    state: FrameCallbackState,
    asked_at: Option<Instant>,
    patience: Option<Duration>,
) -> Option<Instant> {
    match (state, asked_at, patience) {
        (FrameCallbackState::Requested, Some(at), Some(patience)) => Some(at + patience),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_redraw_waits_for_its_frame_callback_for_ever_unless_a_patience_is_set() {
        let at = Instant::now();
        let patience = Duration::from_millis(100);
        // as published: held back until the callback comes
        assert_eq!(held_back_until(FrameCallbackState::Requested, Some(at), None), None);
        // with a patience: until the patience is over
        assert_eq!(
            held_back_until(FrameCallbackState::Requested, Some(at), Some(patience)),
            Some(at + patience)
        );
        // nothing in flight, or the callback came: nothing is held back
        assert_eq!(held_back_until(FrameCallbackState::None, Some(at), Some(patience)), None);
        assert_eq!(held_back_until(FrameCallbackState::Received, Some(at), Some(patience)), None);
        assert_eq!(held_back_until(FrameCallbackState::Requested, None, Some(patience)), None);
    }

    #[test]
    fn the_patience_set_is_the_one_read_and_none_is_none() {
        set(Some(Duration::from_millis(100)));
        assert_eq!(get(), Some(Duration::from_millis(100)));
        set(Some(Duration::ZERO));
        // a patience of nothing is still a patience: the shortest there is
        assert_eq!(get(), Some(Duration::from_micros(1)));
        set(None);
        assert_eq!(get(), None);
    }
}
