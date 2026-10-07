#!/usr/bin/env bash
# Records what a real Ollama answers, as fixtures for the contract tests of the fakes
# (docs/phase3-test-plan.md, group C). A fake of a program is only as good as its likeness to the
# program, so the answers the application depends on are recorded from the real one, and both
# fakes (the Rust one in the application's tests, `fake_ollama.py` in the GUI suite's repository)
# are held to the *shape* of every recording: the same keys with the same kinds of value.
#
#   record.sh --bin PATH/TO/ollama --gguf SMALL.gguf [--pull all-minilm] [--out DIR]
#
#   --bin    the ollama executable (an unpacked release: bin/ollama beside lib/ollama)
#   --gguf   a small GGUF text model to import (the project's Bonsai-1.7B-Q1_0.gguf is 248 MB)
#   --pull   also pull this model from the real registry (all-minilm is 46 MB) through a
#            forwarding proxy that logs the hosts contacted (recorded/egress.txt). The one step
#            that uses the internet; without it the pull is recorded failing, through a proxy
#            that refuses, which touches nothing
#   --out    where the recordings go (default: recorded/ beside this script)
#
# Everything runs in a scratch HOME and models directory under $TMPDIR; nothing touches ~/.ollama.
# Needs bash, curl, jq and python3. The server runs with OLLAMA_NO_CLOUD=1, as QuickAI starts it.
set -u

here=$(cd "$(dirname "$0")" && pwd)
bin="" gguf="" pull="" out="$here/recorded"
while [ $# -gt 0 ]; do
    case "$1" in
    --bin) bin=$2; shift 2 ;;
    --gguf) gguf=$2; shift 2 ;;
    --pull) pull=$2; shift 2 ;;
    --out) out=$2; shift 2 ;;
    *) echo "record.sh: unknown argument $1" >&2; exit 2 ;;
    esac
done
[ -x "$bin" ] || { echo "record.sh: --bin must be an executable ollama" >&2; exit 2; }
[ -f "$gguf" ] || { echo "record.sh: --gguf must be a GGUF file" >&2; exit 2; }
for tool in curl jq python3; do
    command -v "$tool" >/dev/null || { echo "record.sh: $tool is needed" >&2; exit 2; }
done

work=$(mktemp -d "${TMPDIR:-/var/tmp}/quickai-ollama-record.XXXXXX")
mkdir -p "$work/home" "$work/models" "$out"
rm -f "$out"/*.body "$out"/index.json "$out"/egress.txt
: >"$work/index.jsonl"

free_port() { python3 -c 'import socket;s=socket.socket();s.bind(("127.0.0.1",0));print(s.getsockname()[1])'; }

serve_pid=""
proxy_pid=""
serve_start() { # serve_start [VAR=value ...]
    port=$(free_port)
    env -i PATH="$PATH" HOME="$work/home" OLLAMA_HOST="127.0.0.1:$port" OLLAMA_MODELS="$work/models" \
        OLLAMA_NO_CLOUD=1 "$@" setsid "$bin" serve >>"$work/serve.log" 2>&1 &
    serve_pid=$!
    local i
    for i in $(seq 1 80); do
        curl -fsS "http://127.0.0.1:$port/api/version" >/dev/null 2>&1 && return 0
        kill -0 "$serve_pid" 2>/dev/null || { echo "record.sh: serve exited early" >&2; tail -20 "$work/serve.log" >&2; return 1; }
        sleep 0.25
    done
    echo "record.sh: serve did not answer" >&2
    return 1
}
serve_stop() {
    [ -n "$serve_pid" ] || return 0
    kill -TERM "$serve_pid" 2>/dev/null
    local i
    for i in $(seq 1 40); do
        [ -z "$(ps -o pid= -s "$serve_pid" 2>/dev/null)" ] && break
        sleep 0.25
    done
    pkill -KILL -s "$serve_pid" 2>/dev/null
    serve_pid=""
}
cleanup() {
    serve_stop
    [ -n "$proxy_pid" ] && kill "$proxy_pid" 2>/dev/null
    rm -rf "$work"
}
trap cleanup EXIT
trap 'cleanup; exit 143' TERM INT HUP

# rec NAME METHOD PATH [JSON]: one request, its status, content type and body kept as recorded.
rec() {
    local name=$1 method=$2 path=$3 body=${4-} hdr="$work/$1.hdr"
    if [ -n "$body" ]; then
        curl -sS -N --max-time "${API_TIMEOUT:-300}" -D "$hdr" -o "$out/$name.body" -X "$method" \
            -H 'Content-Type: application/json' -d "$body" "http://127.0.0.1:$port$path"
    else
        curl -sS -N --max-time "${API_TIMEOUT:-300}" -D "$hdr" -o "$out/$name.body" -X "$method" \
            "http://127.0.0.1:$port$path"
    fi
    local status ctype
    status=$(head -1 "$hdr" | awk '{print $2}')
    ctype=$(grep -i '^content-type:' "$hdr" | head -1 | cut -d' ' -f2- | tr -d '\r')
    jq -cn --arg name "$name" --arg method "$method" --arg path "$path" --arg body "$body" \
        --argjson status "${status:-0}" --arg ctype "$ctype" \
        '{name:$name, method:$method, path:$path, request:(if $body == "" then null else ($body|fromjson? // $body) end),
          status:$status, content_type:$ctype, file:($name + ".body")}' >>"$work/index.jsonl"
    echo "recorded $name: $method $path -> $status ($(wc -c <"$out/$name.body") bytes)"
}

M=bonsai-q1

echo "== the server, as QuickAI starts it"
serve_start || exit 1
version=$(curl -fsS "http://127.0.0.1:$port/api/version" | jq -r .version)
echo "ollama $version on port $port"

rec root GET /
curl -sS -I "http://127.0.0.1:$port/" >"$out/root_head.body" 2>/dev/null
rec route_unknown GET /no/such/route
rec version GET /api/version
rec tags_empty GET /api/tags
rec ps_empty GET /api/ps

echo "== import $(basename "$gguf")"
sum=$(sha256sum "$gguf" | cut -d' ' -f1)
curl -sS -o /dev/null -w 'blob upload: http %{http_code}\n' -X POST --data-binary @"$gguf" \
    "http://127.0.0.1:$port/api/blobs/sha256:$sum"
rec create POST /api/create "{\"model\":\"$M\",\"files\":{\"$(basename "$gguf")\":\"sha256:$sum\"}}"

rec tags_one GET /api/tags
rec show POST /api/show "{\"model\":\"$M\"}"
rec copy POST /api/copy "{\"source\":\"$M\",\"destination\":\"bonsai-copy\"}"
rec delete_ok DELETE /api/delete '{"model":"bonsai-copy"}'

echo "== errors"
rec generate_unknown POST /api/generate '{"model":"no-such-model","prompt":"hi"}'
rec chat_unknown POST /api/chat '{"model":"no-such-model","messages":[{"role":"user","content":"hi"}]}'
rec show_unknown POST /api/show '{"model":"no-such-model"}'
rec delete_unknown DELETE /api/delete '{"model":"no-such-model"}'
rec generate_bad_json POST /api/generate '{"model":'
rec pull_empty POST /api/pull '{"model":""}'

echo "== load, use, unload"
rec generate_load POST /api/generate "{\"model\":\"$M\",\"keep_alive\":-1}"
rec ps_loaded GET /api/ps
rec generate_stream POST /api/generate "{\"model\":\"$M\",\"prompt\":\"Say hello.\",\"stream\":true,\"think\":false,\"options\":{\"num_predict\":12,\"temperature\":0}}"
rec generate_nostream POST /api/generate "{\"model\":\"$M\",\"prompt\":\"Say hello.\",\"stream\":false,\"think\":false,\"options\":{\"num_predict\":12,\"temperature\":0}}"
rec chat_stream POST /api/chat "{\"model\":\"$M\",\"messages\":[{\"role\":\"user\",\"content\":\"Say hello.\"}],\"stream\":true,\"think\":false,\"options\":{\"num_predict\":12,\"temperature\":0}}"
rec chat_stream_thinking POST /api/chat "{\"model\":\"$M\",\"messages\":[{\"role\":\"user\",\"content\":\"Say hello.\"}],\"stream\":true,\"options\":{\"num_predict\":24,\"temperature\":0}}"
rec chat_nostream POST /api/chat "{\"model\":\"$M\",\"messages\":[{\"role\":\"user\",\"content\":\"Say hello.\"}],\"stream\":false,\"think\":false,\"options\":{\"num_predict\":12,\"temperature\":0}}"
rec v1_chat_stream POST /v1/chat/completions "{\"model\":\"$M\",\"messages\":[{\"role\":\"user\",\"content\":\"Say hello.\"}],\"stream\":true,\"max_tokens\":12,\"temperature\":0}"
rec v1_chat POST /v1/chat/completions "{\"model\":\"$M\",\"messages\":[{\"role\":\"user\",\"content\":\"Say hello.\"}],\"stream\":false,\"max_tokens\":12,\"temperature\":0}"
rec v1_models GET /v1/models
rec generate_unload POST /api/generate "{\"model\":\"$M\",\"keep_alive\":0}"
rec ps_unloaded GET /api/ps

serve_stop

echo "== a pull that cannot reach the registry (the proxy refuses: nothing leaves this machine)"
serve_start HTTPS_PROXY=http://127.0.0.1:1 HTTP_PROXY=http://127.0.0.1:1 || exit 1
rec pull_unreachable POST /api/pull '{"model":"library/no-such-model"}'
serve_stop

if [ -n "$pull" ]; then
    echo "== a real pull of $pull, through a logging proxy (uses the internet)"
    pport=$(free_port)
    python3 "$here/connect_proxy.py" "$pport" "$out/egress.txt" &
    proxy_pid=$!
    sleep 0.5
    serve_start HTTPS_PROXY="http://127.0.0.1:$pport" HTTP_PROXY="http://127.0.0.1:$pport" || exit 1
    rec pull_real POST /api/pull "{\"model\":\"$pull\"}"
    rec tags_pulled GET /api/tags
    rec pull_again POST /api/pull "{\"model\":\"$pull\"}"
    rec delete_pulled DELETE /api/delete "{\"model\":\"$pull\"}"
    serve_stop
    kill "$proxy_pid" 2>/dev/null
    proxy_pid=""
    sort -u "$out/egress.txt" -o "$out/egress.txt"
    echo "hosts contacted:"
    cat "$out/egress.txt"
fi

# The scratch directory shows in a model's Modelfile (its FROM line); a recording should not carry it.
grep -rl "$work" "$out" 2>/dev/null | xargs -r sed -i "s#$work#/SCRATCH#g"

jq -s --arg v "$version" --arg d "$(date -u +%Y-%m-%d)" --arg g "$(basename "$gguf")" \
    '{ollama_version:$v, recorded:$d, model_file:$g, responses:.}' "$work/index.jsonl" >"$out/index.json"
echo "done: $(jq '.responses|length' "$out/index.json") responses in $out"
