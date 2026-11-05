# Changed copies of libraries

## winit

`vendor/winit` is [winit 0.30.13](https://crates.io/crates/winit/0.30.13), the window library under eframe, with one
change. `[patch.crates-io]` in the root `Cargo.toml` makes every build use it. The commit that adds the copy holds it as
published, and the commit after it is the whole change.

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

### When eframe moves to a newer winit

1. Replace `winit/` with the new published crate, leaving out `.cargo-ok`, `Cargo.toml.orig` and `Cargo.lock`, and commit
   that by itself.
2. Make the same change again, and commit it. winit's development version uses the same library for other effects too
   (`bgr_effects.rs`), so read what it needs from it first.
3. `python3 scripts/licenses/generate.py` stops if `wayland-protocols-plasma` is back in `Cargo.lock`.
4. A release build must hold no KDE protocol: `strings -a quickai | grep org_kde` prints nothing.

If winit gains a setting to leave the protocol out, or KDE relicenses `blur.xml`, this copy can go.
