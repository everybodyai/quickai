#!/usr/bin/env bash
# Cross-compile the GUI and package a Windows x86_64 release bundle.
#
# The GUI itself is cross-compiled via `cross` (https://github.com/cross-rs/cross),
# which runs the build inside a Docker image with a matching mingw-w64
# toolchain — nothing extra is installed on the host. Requires a working
# `docker` daemon.
#
# llama-server.exe is NOT cross-compiled (that would mean cross-building a
# C++/CMake/Vulkan project via mingw, which `cross` doesn't help with at
# all) — instead this fetches the official prebuilt Vulkan build from
# llama.cpp's own GitHub releases (see fetch_windows_llama in common.sh),
# for the same release tag the local Linux build came from.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
# shellcheck source=./common.sh
source ./common.sh

TARGET="x86_64-pc-windows-gnu"

if ! command -v cross >/dev/null 2>&1; then
    echo "==> 'cross' not found; installing (cargo install cross --locked)"
    cargo install cross --locked
fi

if ! docker info >/dev/null 2>&1; then
    echo "error: docker is not available/running — 'cross' needs it to build for Windows." >&2
    exit 1
fi

echo "==> Building $APP_NAME $VERSION for $TARGET (via cross + Docker)"
cross build --release --target "$TARGET" -p "$APP_NAME"

echo "==> Fetching official llama.cpp Windows Vulkan build ($LLAMA_RELEASE_TAG)"
LLAMA_DIR="$(fetch_windows_llama)"

package_bundle \
    "$ROOT_DIR/target/$TARGET/release/$APP_NAME.exe" \
    "$LLAMA_DIR/llama-server.exe" \
    "$LLAMA_DIR" \
    "$APP_NAME-$VERSION-windows-x86_64" \
    zip
