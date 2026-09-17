#!/usr/bin/env bash
# Build a self-integrating AppImage for Linux x86_64, bundling llama-server
# (Vulkan build) and both Bonsai models (8B, 1.7B) alongside the GUI.
#
# Structure/approach borrowed from a sibling project's build.sh
# (jsonquery/build/appimage.sh, itself borrowed from ubuntu_manager/sysmanager):
# the AppRun script registers a .desktop file + icon into ~/.local/share on
# first run (keyed off the AppImage runtime's $APPIMAGE variable, which is
# stable across launches unlike the FUSE mount point), so the app shows up
# in the app menu/taskbar and can be pinned there without requiring
# appimaged or AppImageLauncher.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
# shellcheck source=./common.sh
source ./common.sh

TARGET="x86_64-unknown-linux-gnu"
ARCH="x86_64"
APPDIR="$ROOT_DIR/build/AppDir"
OUTPUT="$DIST_DIR/$APP_NAME-$VERSION-$ARCH.AppImage"
LLAMA_BIN="$ROOT_DIR/llama.cpp/build/bin/llama-server"

if [ ! -f "$LLAMA_BIN" ]; then
    echo "error: $LLAMA_BIN not found — build llama.cpp first (see README.md)." >&2
    exit 1
fi

echo "==> Building $APP_NAME $VERSION for $TARGET"
cargo build --release --target "$TARGET" -p "$APP_NAME"

echo "==> Assembling AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/share/applications"
for size in 16 32 48 64 128 256 512; do
    mkdir -p "$APPDIR/usr/share/icons/hicolor/${size}x${size}/apps"
    cp "$ROOT_DIR/assets/icons/icon-$size.png" \
        "$APPDIR/usr/share/icons/hicolor/${size}x${size}/apps/$APP_NAME.png"
done
mkdir -p "$APPDIR/usr/share/pixmaps"
cp "$ROOT_DIR/assets/icons/icon-256.png" "$APPDIR/usr/share/pixmaps/$APP_NAME.png"

cp "$ROOT_DIR/target/$TARGET/release/$APP_NAME" "$APPDIR/usr/bin/$APP_NAME"
cp "$LLAMA_BIN" "$APPDIR/usr/bin/llama-server"
for pattern in "${LLAMA_LINUX_LIB_GLOBS[@]}"; do
    cp -P "$ROOT_DIR"/llama.cpp/build/bin/$pattern "$APPDIR/usr/bin/" 2>/dev/null || true
done
mkdir -p "$APPDIR/usr/bin/models"
for file in "${BUNDLED_MODEL_FILES[@]}"; do
    cp "$ROOT_DIR/models/$file" "$APPDIR/usr/bin/models/"
done
cp "$ROOT_DIR/LICENSE" "$APPDIR/usr/bin/LICENSE"
cp "$ROOT_DIR/THIRD_PARTY_LICENSES.md" "$APPDIR/usr/bin/THIRD_PARTY_LICENSES.md"

# AppDir root: appimagetool looks for .DirIcon (a plain file, not a symlink)
# to embed into the AppImage so file managers can show it without appimaged.
cp "$ROOT_DIR/assets/icons/icon-256.png" "$APPDIR/$APP_NAME.png"
cp "$ROOT_DIR/assets/icons/icon-256.png" "$APPDIR/.DirIcon"

cat >"$APPDIR/AppRun" <<APPRUN_EOF
#!/bin/bash
HERE="\$(dirname "\$(readlink -f "\$0")")"

# Self-integrate into the desktop app menu/taskbar on first run, and
# re-integrate if the AppImage has since moved. \$APPIMAGE is set by the
# AppImage type2 runtime to this file's own real path (unlike \$HERE, a
# throwaway FUSE mount point that differs on every launch), so it's stable
# enough to point a launcher's Exec= at.
if [ -n "\$APPIMAGE" ]; then
    DESKTOP_DST="\$HOME/.local/share/applications/$APP_NAME.desktop"
    ICON_DST="\$HOME/.local/share/icons/hicolor/256x256/apps/$APP_NAME.png"
    EXEC_LINE="Exec=\"\$APPIMAGE\""
    if [ ! -f "\$DESKTOP_DST" ] || ! grep -qF "\$EXEC_LINE" "\$DESKTOP_DST" 2>/dev/null; then
        mkdir -p "\$(dirname "\$DESKTOP_DST")" "\$(dirname "\$ICON_DST")"
        cp "\$HERE/$APP_NAME.png" "\$ICON_DST" 2>/dev/null
        sed "s|^Exec=.*|\$EXEC_LINE|" "\$HERE/$APP_NAME.desktop" > "\$DESKTOP_DST"
        command -v update-desktop-database &>/dev/null && update-desktop-database "\$HOME/.local/share/applications" &>/dev/null
        HICOLOR="\$HOME/.local/share/icons/hicolor"
        if [ -f "\$HICOLOR/icon-theme.cache" ]; then
            gtk-update-icon-cache -f -t --ignore-theme-index "\$HICOLOR" &>/dev/null || touch "\$HICOLOR"
        fi
    fi
fi

exec "\$HERE/usr/bin/$APP_NAME" "\$@"
APPRUN_EOF
chmod +x "$APPDIR/AppRun"

# .desktop entry (must live at AppDir root AND usr/share/applications/).
# StartupWMClass matches APP_ID set via ViewportBuilder::with_app_id() in
# main.rs, so the WM associates the running window with this launcher —
# without that match, "pin to taskbar" after launch doesn't stick.
cat >"$APPDIR/$APP_NAME.desktop" <<DESKTOP_EOF
[Desktop Entry]
Type=Application
Name=QuickAI
GenericName=LLM Server Manager
Comment=Start/stop local GGUF models (Bonsai, Qwen3) on the GPU via llama.cpp
Exec=$APP_NAME
Icon=$APP_NAME
Categories=Development;Utility;
Terminal=false
StartupWMClass=$APP_NAME
X-AppImage-Version=$VERSION
DESKTOP_EOF
cp "$APPDIR/$APP_NAME.desktop" "$APPDIR/usr/share/applications/$APP_NAME.desktop"

echo "==> Locating appimagetool"
if command -v appimagetool &>/dev/null; then
    APPIMAGETOOL="appimagetool"
else
    TOOL_PATH="$ROOT_DIR/build/appimagetool-$ARCH.AppImage"
    if [ ! -f "$TOOL_PATH" ]; then
        echo "    downloading appimagetool..."
        curl -Lo "$TOOL_PATH" \
            "https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage"
        chmod +x "$TOOL_PATH"
    fi
    APPIMAGETOOL="$TOOL_PATH"
fi

echo "==> Building AppImage"
rm -f "$OUTPUT"
ARCH=$ARCH "$APPIMAGETOOL" "$APPDIR" "$OUTPUT"

echo "==> Wrote $OUTPUT"
