#!/usr/bin/env bash
# Records the answers of Hugging Face's API that the look-up of ComfyUI's window is held to
# (docs/comfyui.md, "Getting models"; test plan group Q): the repositories of a video model, its text
# encoder and its VAE as three different people published them, the repository of the model the recommended
# list has, and the searches that find them (the plain list of a pipeline, and words).
#
#   record.sh [--out DIR]
#
# Each answer is trimmed to the keys the look-up reads: the id, the counts, the pipeline and tags, whether it
# is gated, and the files with their sizes and the SHA-256 the hub states for each file it keeps in LFS
# (`lfs.sha256`, which a download is checked against). One request each, an honest User-Agent, nothing else
# is fetched. Needs bash, curl and python3. When a repository goes away, name another.
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

trim() { # trim KIND RAW OUT
    python3 -I - "$1" "$2" "$3" <<'PY'
import json, sys
kind, raw, out = sys.argv[1:4]
data = json.load(open(raw, encoding="utf-8"))
keep = ["id", "modelId", "downloads", "likes", "pipeline_tag", "library_name", "tags", "gated", "private", "disabled"]
if kind == "repo":
    keep.append("siblings")
    data = {k: data[k] for k in keep if k in data}
    files = []
    for s in data.get("siblings", []):
        f = {k: s[k] for k in ("rfilename", "size") if k in s}
        sha = (s.get("lfs") or {}).get("sha256")
        if sha:
            f["lfs"] = {"sha256": sha}
        files.append(f)
    data["siblings"] = files
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

# the plain list of a pipeline, and words that find a model by its name
record_list search_text_to_video.json "&pipeline_tag=text-to-video"
record_list search_umt5.json "&search=umt5"
# a video model, its text encoder and its VAE, each in a repository of its own; the model of the
# recommended list, which holds all three
record_repo hum-ma/Wan2.2-TI2V-5B-Turbo-GGUF
record_repo city96/umt5-xxl-encoder-gguf
record_repo QuantStack/Wan2.2-TI2V-5B-GGUF
record_repo biali/ltxv-gguf
mv "$out/index.tsv.new" "$out/index.tsv"
