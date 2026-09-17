#!/usr/bin/env bash
# Build and package the native Linux x86_64 release bundle (GUI + our local
# Vulkan-accelerated llama-server build + the bundled model).
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
# shellcheck source=./common.sh
source ./common.sh

TARGET="x86_64-unknown-linux-gnu"
LLAMA_BIN="$ROOT_DIR/llama.cpp/build/bin/llama-server"

if [ ! -f "$LLAMA_BIN" ]; then
    echo "error: $LLAMA_BIN not found — build llama.cpp first (see README.md)." >&2
    exit 1
fi

echo "==> Building $APP_NAME $VERSION for $TARGET"
cargo build --release --target "$TARGET" -p "$APP_NAME"

package_bundle \
    "$ROOT_DIR/target/$TARGET/release/$APP_NAME" \
    "$LLAMA_BIN" \
    "" \
    "$APP_NAME-$VERSION-linux-x86_64" \
    tar.gz
