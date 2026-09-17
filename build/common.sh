#!/usr/bin/env bash
# Shared config/helpers, sourced by the other scripts in this directory.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="$ROOT_DIR/dist"
APP_NAME="quickai"
VERSION="$(grep -m1 '^version' "$ROOT_DIR/Cargo.toml" | sed -E 's/.*"(.*)".*/\1/')"

# The bundled models: Bonsai 8B (the recommended default) and Bonsai 1.7B
# (a lighter fallback for slower machines, picked automatically by the GUI
# based on detected RAM — see hardware.rs) both ship in the package.
# Anything else can still be dropped into the models/ folder after install;
# the app already scans it at startup. models/ is gitignored (large
# binaries, not source), so a fresh checkout fetches these on demand — see
# fetch_model() below.
BONSAI_8B_FILE="Bonsai-8B-Q1_0.gguf"
BONSAI_8B_URL="https://huggingface.co/prism-ml/Bonsai-8B-gguf/resolve/main/$BONSAI_8B_FILE"
BONSAI_1_7B_FILE="Bonsai-1.7B-Q1_0.gguf"
BONSAI_1_7B_URL="https://huggingface.co/prism-ml/Bonsai-1.7B-gguf/resolve/main/$BONSAI_1_7B_FILE"

BUNDLED_MODEL_FILES=("$BONSAI_8B_FILE" "$BONSAI_1_7B_FILE")
BUNDLED_MODEL_URLS=("$BONSAI_8B_URL" "$BONSAI_1_7B_URL")

# Kept as the single "the bundled model" name for anything that only cares
# about one (e.g. the dev-tree integration test in server_manager.rs).
MODEL_FILE="$BONSAI_8B_FILE"
MODEL_PATH="$ROOT_DIR/models/$MODEL_FILE"

# llama.cpp build/release identifier the bundled binaries come from. The
# Linux one is our own local Vulkan build (build/appimage.sh, build/linux.sh
# use it directly); Windows has no local build, so windows.sh/windows.bat
# fetch the official prebuilt Vulkan zip for this exact same release tag —
# bump this if llama.cpp/ is ever updated and rebuilt.
LLAMA_RELEASE_TAG="b11007"

# Non-llama-specific runtime libs (libc, libstdc++, Vulkan loader, etc.) are
# NOT bundled on Linux — they're assumed present on any modern desktop with
# a working GPU driver, same baseline llama.cpp itself assumes.
LLAMA_LINUX_LIB_GLOBS=(
    "libggml-base.so*"
    "libggml-cpu.so*"
    "libggml-vulkan.so*"
    "libggml.so*"
    "libllama-common.so*"
    "libllama-server-impl.so"
    "libllama.so*"
    "libmtmd.so*"
)

mkdir -p "$DIST_DIR"

# Downloads each bundled model into models/ if it's not already there
# (they're gitignored, so this is what makes a fresh checkout buildable
# without a manual download step first). Downloads to a .part file and
# renames on success, so a failed/interrupted download never leaves a
# truncated file that looks complete on the next run.
fetch_model() {
    mkdir -p "$ROOT_DIR/models"
    for i in "${!BUNDLED_MODEL_FILES[@]}"; do
        local file="${BUNDLED_MODEL_FILES[$i]}" url="${BUNDLED_MODEL_URLS[$i]}"
        local path="$ROOT_DIR/models/$file"
        if [ ! -f "$path" ]; then
            echo "==> Downloading $file (one-time)..." >&2
            curl -Lo "$path.part" "$url"
            mv "$path.part" "$path"
        fi
    done
}
fetch_model

# Stage a full bundle (GUI binary + llama-server + its libs + the model +
# README) into $stage/$pkg_name, then archive it as $kind ("tar.gz"/"zip").
#
# Args: gui_bin llama_server_bin lib_dir_or_empty pkg_name kind
package_bundle() {
    local gui_bin="$1" llama_bin="$2" lib_dir="$3" pkg_name="$4" kind="$5"
    local stage out
    stage="$(mktemp -d)"
    out="$stage/$pkg_name"
    mkdir -p "$out"

    cp "$gui_bin" "$out/"
    cp "$llama_bin" "$out/"
    if [ -n "$lib_dir" ]; then
        # Windows: bundle every DLL the official release shipped alongside
        # llama-server.exe, since we don't know the minimal set by hand.
        find "$lib_dir" -maxdepth 1 -iname '*.dll' -exec cp {} "$out/" \;
    else
        # Linux: only the specific llama libs (see LLAMA_LINUX_LIB_GLOBS),
        # not the whole build/bin directory.
        for pattern in "${LLAMA_LINUX_LIB_GLOBS[@]}"; do
            cp -P "$ROOT_DIR"/llama.cpp/build/bin/$pattern "$out/" 2>/dev/null || true
        done
    fi

    mkdir -p "$out/models"
    for file in "${BUNDLED_MODEL_FILES[@]}"; do
        cp "$ROOT_DIR/models/$file" "$out/models/"
    done
    cp "$ROOT_DIR/build/PACKAGE_README.md" "$out/README.md"
    cp "$ROOT_DIR/LICENSE" "$out/LICENSE"
    cp "$ROOT_DIR/THIRD_PARTY_LICENSES.md" "$out/THIRD_PARTY_LICENSES.md"

    case "$kind" in
    tar.gz)
        tar -C "$stage" -czf "$DIST_DIR/$pkg_name.tar.gz" "$pkg_name"
        echo "==> Wrote $DIST_DIR/$pkg_name.tar.gz"
        ;;
    zip)
        (cd "$stage" && zip -rq "$DIST_DIR/$pkg_name.zip" "$pkg_name")
        echo "==> Wrote $DIST_DIR/$pkg_name.zip"
        ;;
    *)
        echo "package_bundle: unknown archive kind '$kind'" >&2
        exit 1
        ;;
    esac

    rm -rf "$stage"
}

# Downloads+caches the official llama.cpp Windows Vulkan build (avoids
# cross-compiling a C++/CMake/Vulkan project for Windows from Linux, which
# `cross` can't help with — it only handles the Rust half). Extracts to
# build/.cache/llama-win-vulkan-$LLAMA_RELEASE_TAG/, returns that path on
# stdout.
fetch_windows_llama() {
    local cache_dir="$ROOT_DIR/build/.cache/llama-win-vulkan-$LLAMA_RELEASE_TAG"
    if [ ! -f "$cache_dir/llama-server.exe" ]; then
        local zip_name="llama-$LLAMA_RELEASE_TAG-bin-win-vulkan-x64.zip"
        local url="https://github.com/ggml-org/llama.cpp/releases/download/$LLAMA_RELEASE_TAG/$zip_name"
        local zip_path="$ROOT_DIR/build/.cache/$zip_name"
        mkdir -p "$ROOT_DIR/build/.cache" "$cache_dir"
        echo "    downloading $zip_name..." >&2
        curl -Lo "$zip_path" "$url"
        unzip -oq "$zip_path" -d "$cache_dir"
    fi
    echo "$cache_dir"
}
