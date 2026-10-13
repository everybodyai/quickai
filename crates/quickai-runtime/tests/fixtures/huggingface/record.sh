#!/usr/bin/env bash
# Records the answers of Hugging Face's API that `lookup.huggingface_gguf@1` is held to
# (docs/lookup-and-bundles.md, section 5; docs/engine-facts.md, L14 to L17): the search of GGUF repositories,
# plain and for a word, and the files of a handful of repositories that show what a look-up meets: a vision
# model with its projector, a repository of many quantizations with two projectors, a draft model, an
# importance matrix and shards, a model for code, an embedding model, one that has no pipeline tag and one
# that is not a language model at all.
#
#   record.sh [--out DIR]
#
# Each answer is trimmed to the keys the look-up reads (a repository's own answer is mostly a chat template,
# its card and a list of widgets): the id, the counts, the pipeline and tags, whether it is gated, and the
# files with their sizes. One request each, an honest User-Agent, nothing else is fetched. Needs bash, curl and
# python3. The repositories are named below; when one goes away, name another.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
out="$here"
while [ $# -gt 0 ]; do
    case "$1" in
    --out) out=$2; shift 2 ;;
    *) echo "record.sh: unknown argument $1" >&2; exit 2 ;;
    esac
done
for tool in curl python3; do
    command -v "$tool" >/dev/null || { echo "record.sh: $tool is needed" >&2; exit 2; }
done
mkdir -p "$out/repos"

site=https://huggingface.co
agent="quickai-record (fixtures for the tests)"

trim() { # trim KIND RAW OUT: keeps the keys the look-up reads
    python3 -I - "$1" "$2" "$3" <<'PY'
import json, sys
kind, raw, out = sys.argv[1:4]
data = json.load(open(raw, encoding="utf-8"))
keep = ["id", "modelId", "downloads", "likes", "pipeline_tag", "library_name", "tags", "gated", "private", "disabled"]
if kind == "repo":
    keep.append("siblings")
    data = {k: data[k] for k in keep if k in data}
    data["siblings"] = [{k: s[k] for k in ("rfilename", "size") if k in s} for s in data.get("siblings", [])]
else:
    data = [{k: item[k] for k in keep if k in item} for item in data]
with open(out, "w", encoding="utf-8") as f:
    json.dump(data, f, indent=1, sort_keys=True)
    f.write("\n")
PY
}

: >"$out/index.tsv.new"

record_list() { # record_list FILE QUERY
    local file=$1 url="$site/api/models?filter=gguf&sort=downloads&direction=-1&limit=50$2"
    curl -fsS --max-time 60 -A "$agent" "$url" -o "$out/$file.raw"
    trim list "$out/$file.raw" "$out/$file"
    rm -f "$out/$file.raw"
    printf '%s\t%s\t%s\n' "$file" "$url" "$(date -u +%Y-%m-%d)" >>"$out/index.tsv.new"
    echo "recorded $file ($(wc -c <"$out/$file") bytes)"
}

record_repo() { # record_repo OWNER/NAME
    local file="repos/${1//\//__}.json" url="$site/api/models/$1?blobs=true"
    curl -fsS --max-time 60 -A "$agent" "$url" -o "$out/$file.raw"
    trim repo "$out/$file.raw" "$out/$file"
    rm -f "$out/$file.raw"
    printf '%s\t%s\t%s\n' "$file" "$url" "$(date -u +%Y-%m-%d)" >>"$out/index.tsv.new"
    echo "recorded $file ($(wc -c <"$out/$file") bytes)"
}

record_list popular.json ""
record_list search_qwen.json "&search=qwen"
record_repo ggml-org/gemma-3-4b-it-GGUF
record_repo unsloth/Qwen3.8-27B-GGUF
record_repo unsloth/Qwen3-Coder-30B-A3B-Instruct-GGUF
record_repo mixedbread-ai/mxbai-embed-large-v1
record_repo mudler/Laguna-XS-2.1-APEX-GGUF
record_repo mudler/locate-anything.cpp-gguf
mv "$out/index.tsv.new" "$out/index.tsv"
