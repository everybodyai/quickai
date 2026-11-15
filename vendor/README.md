# Changed copies of libraries

## winit

`vendor/winit` is [winit 0.30.13](https://crates.io/crates/winit/0.30.13), the window library under eframe, with three
changes. `[patch.crates-io]` in the root `Cargo.toml` makes every build use it. The commit that adds the copy holds it as
published, and the commit after it is the first change; the second and the third are each a commit of their own.

### What was done (2026-11-05, for release 0.2.4)

KDE's window blur protocol is taken out of winit:

- `Cargo.toml`: `wayland-protocols-plasma` is no longer a dependency, nor part of the `wayland` feature.
- `src/platform_impl/linux/wayland/types/kwin_blur.rs` is deleted, and `types/mod.rs` no longer names it.
- `src/platform_impl/linux/wayland/state.rs`: the blur manager is gone.
- `src/platform_impl/linux/wayland/window/state.rs`: the blur fields are gone, and `set_blur` only logs a message if a
  blurred window is asked for.

The changed files say so in their first line, as winit's license, the Apache License 2.0 (`winit/LICENSE`), asks. The
documentation comments in `src/window.rs` still describe winit as published.

### Why

`wayland-protocols-plasma` is under the MIT License, but it holds KDE's `blur.xml`, by Martin Gräßlin and Marco Martin,
under the LGPL 2.1 or later, and the code generated from that file was compiled into the window program for Linux.
QuickAI never asks for a blurred window, and winit has no setting to leave the protocol out.

### What it changes

Nothing a person sees. `set_blur` and `with_blur` do nothing on Wayland, and QuickAI calls neither. Wayland itself, X11,
Windows and macOS are as they were. Releases up to 0.2.3 still hold the generated code; see
`THIRD_PARTY_LICENSES.md`.

### The second change (2026-11-12): a redraw waits for its frame callback at most a patience

On Wayland, winit holds a window's `RedrawRequested` back from the moment the application says it is about to present
until the compositor's frame callback asks for the next frame. A compositor sends none to a window it does not draw
(covered by another, minimized, on another workspace, a locked screen), and egui draws QuickAI's other windows (Add a
model, Settings, a problem report: immediate viewports) in the main window's frames. While the main window was hidden
they all stopped, a download's progress among them, and the watcher of the window (`app/stall.rs`) wrote that the
window had stopped answering and opened a problem report when it was shown again.

- `src/platform_impl/linux/wayland/frame_patience.rs` (new): the patience, none by default, and when a redraw held back
  goes through.
- `src/platform/wayland.rs`: `set_frame_callback_patience`, which QuickAI calls on Wayland (`main.rs`, 100 ms).
- `src/platform_impl/linux/wayland/window/state.rs`: when the frame callback in flight was asked for.
- `src/platform_impl/linux/wayland/event_loop/mod.rs`: a redraw that waited its patience goes through, and the next
  waits the patience again (the callback stays in flight: one at a time); the loop wakes when a redraw has waited it.
- `src/platform_impl/linux/wayland/mod.rs`: the new module.

A window the compositor shows is paced by its frame callbacks as before; one it does not show is drawn at most ten times
a second while QuickAI asks for frames, and not at all otherwise. It works with the present mode QuickAI uses on
Wayland (`main.rs`: no frame waits for the compositor; one that waited would wait for as long as the window is hidden).
Without `set_frame_callback_patience` winit behaves as published. winit's own report of the problem is
[rust-windowing/winit#4668](https://github.com/rust-windowing/winit/issues/4668); eframe 0.36.2 stopped the busy loop
that came with it ([emilk/egui#8326](https://github.com/emilk/egui/issues/8326)).

### The third change (2026-11-15): files dropped on a window on Wayland

winit takes files dropped on a window on X11, Windows and macOS (`WindowEvent::HoveredFile`, `DroppedFile`), but its
Wayland backend has no drag-and-drop at all, so a file dropped on QuickAI's chat (C226) did nothing on GNOME or KDE.

- `src/platform_impl/linux/wayland/dnd.rs` (new): a drag that offers `text/uri-list`, as a file manager's does, is
  accepted as a copy. While it is over a window, the window gets `HoveredFile` with an empty path (the paths are read
  only once the files are dropped), and `HoveredFileCancelled` when it leaves. Dropped, the list is read through the
  event loop, never by waiting on it, 4 MB of it at the most, and the window gets `DroppedFile` for each local file in
  it. Any other drag is refused, as before. winit offers nothing to other programs.
- `src/platform_impl/linux/wayland/state.rs`: the data device manager, and the window under the drag.
- `src/platform_impl/linux/wayland/seat/mod.rs`: each seat gets a data device.
- `src/platform_impl/linux/wayland/mod.rs`: the new module.

Copying and pasting text is egui's (its own clipboard, smithay-clipboard, has a data device of its own), and is as it
was. When a newer winit takes dropped files on Wayland by itself, this change can go.

### When eframe moves to a newer winit

1. Replace `winit/` with the new published crate, leaving out `.cargo-ok`, `Cargo.toml.orig` and `Cargo.lock`, and commit
   that by itself.
2. Make the same changes again, and commit them. winit's development version uses the same library for other effects too
   (`bgr_effects.rs`), so read what it needs from it first. If the new winit lets a redraw through without the frame
   callback by itself, or reports a window the compositor does not draw, the second change can go.
3. `python3 scripts/licenses/generate.py` stops if `wayland-protocols-plasma` is back in `Cargo.lock`.
4. A release build must hold no KDE protocol: `strings -a quickai | grep org_kde` prints nothing.
5. `cargo test --manifest-path vendor/winit/Cargo.toml --lib frame_patience` (with a target directory outside the copy,
   and its `Cargo.lock` removed afterwards) tests the patience; `--lib dnd` tests how a list of dropped files is read.

The copy can go once neither change is needed: winit gains a setting to leave the protocol out (or KDE relicenses
`blur.xml`), a window the compositor does not draw no longer holds back the others, and winit takes dropped files on
Wayland.
