#!/usr/bin/env bash
# Takes the pictures of the Help window (C191): QuickAI is run on a screen of its own that nobody sees (Xvfb), with a home
# of its own and a real model, and is clicked through what the pages show (the engine menu, Change, Add a model, the
# Settings, a port another program holds, the model running, Connect an app, a reply in the chat). Each frame is
# taken whole, then cut to the part a page shows, once in the dark theme and once in the light, into dark/ and light/
# beside this script. The marks drawn on them are the pages' own (app/help.rs), in the pixels of these pictures: a picture
# whose size changes is drawn to fit, and its marks are checked by a test to be inside it.
#
#   LLAMA_SERVER=~/.quickai/llama-bin/<build>/llama-server MODELS_DIR=~/.quickai/models \
#   MODEL_FILE=LFM2.5-1.2B-Instruct-Q4_K_M.gguf assets/help/take-pictures.sh
#
# It needs Xvfb, xdotool, ffmpeg, curl and `sudo`: the pictures are of a public repository, so the application is run in
# a mount namespace of its own whose /home has nobody's folder but /home/you, with the model and the server bound into it
# (it runs as the person who runs this, not as root). The model answers a question once per theme, so a small one is
# best. The places clicked are the window's at 980 x 680, its size when it opens.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
bin=${QUICKAI_BIN:-$root/target/debug/quickai}
server=${LLAMA_SERVER:?the llama-server to run the model with}
models=${MODELS_DIR:?a folder that holds the model}
model=${MODEL_FILE:?the file name of the model in it}
display=${HELP_DISPLAY:-:57}
port=${HELP_PORT:-8190}
work=$(mktemp -d)
app=""
xvfb=""
squatter=""

finish() {
    [ -n "$squatter" ] && kill "$squatter" 2>/dev/null || true
    [ -n "$app" ] && kill "$app" 2>/dev/null || true
    [ -n "$xvfb" ] && kill "$xvfb" 2>/dev/null || true
    rm -rf "$work"
}
trap finish EXIT

if curl -s -m 1 "http://127.0.0.1:$port/" >/dev/null 2>&1; then
    echo "something already answers on port $port: set HELP_PORT" >&2
    exit 1
fi
Xvfb "$display" -screen 0 1600x900x24 -nolisten tcp >/dev/null 2>&1 &
xvfb=$!
sleep 1
export DISPLAY=$display

# a whole frame of the main window (or `size` at `at`), as `name`
shoot() { ffmpeg -loglevel error -y -f x11grab -draw_mouse 0 -video_size "${3:-980x680}" -i "$display+${2:-0,0}" -frames:v 1 "$work/$theme/$1.png"; }
click() { xdotool mousemove "$1" "$2" click 1; sleep "${3:-1.2}"; }
# out of a frame: name, frame, x, y, width, height
cut() { ffmpeg -loglevel error -y -i "$work/$theme/$2.png" -vf "crop=$5:$6:$3:$4" "$here/$theme/$1.png"; }

# the application as it is seen: in /home/you, which is all /home holds for it
build=$(basename "$(dirname "$server")")
you=/home/you
cp "$bin" "$work/quickai"
# not `models` beside the copy of the application: that is where a package keeps the models it ships with
mkdir -p "$work/server" "$work/your-models"
run_as_you() {
    sudo -n unshare --mount sh -c "
        mount --make-rprivate /
        mount --bind '$(dirname "$server")' '$work/server'
        mount --bind '$models' '$work/your-models'
        mount -t tmpfs tmpfs /home
        mkdir -p $you/.quickai/models $you/.quickai/llama-bin/$build $you/.config/quickai
        mount --bind '$work/server' $you/.quickai/llama-bin/$build
        mount --bind '$work/your-models' $you/.quickai/models
        cp '$work/settings.json' $you/.config/quickai/settings.json
        chown $(id -u):$(id -g) $you $you/.quickai $you/.config $you/.config/quickai $you/.config/quickai/settings.json
        cd $you
        exec setpriv --reuid=$(id -u) --regid=$(id -g) --init-groups env -i HOME=$you DISPLAY=$display \
            PATH=/usr/bin:/bin '$work/quickai'
    " >"$work/$theme.log" 2>&1 &
    for _ in $(seq 1 50); do
        app=$(pgrep -f "^$work/quickai\$" || true)
        [ -n "$app" ] && break
        sleep 0.2
    done
}

for theme in dark light; do
    mkdir -p "$work/$theme" "$here/$theme"
    dark=$([ "$theme" = dark ] && echo true || echo false)
    cat >"$work/settings.json" <<EOF
{"last_model_filename": "$model", "port": $port, "ctx_size": 16384, "n_gpu_layers": 999, "dark_mode": $dark,
 "llama_bin_override": "$you/.quickai/llama-bin/$build/$(basename "$server")",
 "extra_args": "", "enable_traffic_proxy": false, "values": {"app.said_hello": true}}
EOF
    run_as_you
    sleep 5
    window=$(xdotool search --name "^QuickAI$" | head -1)

    shoot main
    click 815 16; shoot engine-menu; click 600 400 0.6
    click 551 100; shoot change; click 551 100 0.6
    click 905 664; shoot computer-menu; click 600 400 0.6
    click 110 59 2.5; shoot add-model; click 816 85 0.8
    click 959 16 4; shoot settings 0,0 760x600; click 35 573 1

    # a program that holds the port: Start asks what to do
    python3 -m http.server "$port" --bind 127.0.0.1 >/dev/null 2>&1 &
    squatter=$!
    sleep 1
    click 948 62 2.5; shoot port-taken
    click 410 382 0.8
    kill "$squatter"; squatter=""
    sleep 1

    click 948 62 1
    for _ in $(seq 1 60); do
        curl -s -m 1 "http://127.0.0.1:$port/health" | grep -q ok && break
        sleep 1
    done
    sleep 2
    click 910 62; shoot connect; click 600 400 0.6
    xdotool windowfocus --sync "$window"
    click 600 613 0.6
    xdotool type --delay 30 "Hi! In two short sentences, what can you help me with?"
    xdotool key Return
    sleep 15
    shoot chat
    click 948 62 3
    kill "$app"; app=""
    sleep 1

    cut main main 0 0 980 680
    cut engine-menu engine-menu 560 0 420 230
    cut change change 220 40 760 360
    cut computer-menu computer-menu 560 540 420 140
    cut add-model add-model 139 60 702 447
    cut settings settings 0 0 760 600
    cut port-taken port-taken 255 268 470 145
    cut connect connect 228 40 748 395
    cut chat chat 220 120 760 526
done
echo "pictures in $here/dark and $here/light"
