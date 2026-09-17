# Snap packaging

The actual manifest lives at [`snap/snapcraft.yaml`](../../snap/snapcraft.yaml)
(Snapcraft only looks for it there, not under `packaging/`). This file is
the maintainer-facing build/test/publish notes for it.

**Status: drafted, not yet build-tested.** Unlike the sibling `jsonquery`
project's snap (pure Rust, already live on the Snap Store), this one builds
llama.cpp with Vulkan from source inside the snap sandbox as well as the
GUI, then bundles both Bonsai models — none of that has been run end to
end yet. Expect to iterate on `override-build` once it's actually tried.

## Why not `plugin: rust`

Same reason as `jsonquery_gui`: the `rust` plugin fetches its toolchain via
the `rustup` *snap*, but the `gnome` extension's own PATH composition
hides `/snap/bin` from the plugin's environment-validation step, so it
fails with `'rustup' not found` regardless of build backend. Installing
Rust ourselves via `rustup.sh` sidesteps it.

## Open questions before this can ship

- **Vulkan in the build environment**: `libvulkan-dev`/`glslc`/etc. are
  listed as `build-packages` on the assumption core24's build base can
  `apt install` them like a normal Ubuntu 24.04 system (the same packages
  the main README's "First-time setup" needs) — not yet confirmed.
- **Vulkan at runtime inside strict confinement**: the `plugs` list only
  has `opengl`, mirroring what the `gnome` extension typically pairs with
  GPU access; whether that's sufficient for `llama-server`'s Vulkan
  compute path (as opposed to OpenGL rendering) under strict confinement,
  or whether it additionally needs something like `hardware-observe` or a
  different interface, needs a real GPU-equipped test run to confirm.
- **Bundle size**: two Bonsai models plus `llama-server` push the snap well
  past what's typical — worth checking Snap Store size limits before
  trying to publish.

## Build

```sh
build/snap.sh   # not yet added — see build/all.sh for the pattern used by
                # the other targets; this needs the same treatment once
                # the manifest above is confirmed working
# -> dist/quickai-<version>-amd64.snap
```

Needs `snapcraft` plus a multipass or LXD build backend.

## Test locally

```sh
sudo snap install --dangerous dist/quickai-<version>-amd64.snap
snap run quickai
sudo snap remove quickai   # when done
```

## Publish

Not registered on the Snap Store yet. Once the build is confirmed working:

```sh
snapcraft login                        # your Ubuntu One / Snap Store account
snapcraft register quickai             # first release only
snapcraft upload --release=stable dist/quickai-<version>-amd64.snap
```

Bump `version` in `snap/snapcraft.yaml` alongside `Cargo.toml` for each
release.
