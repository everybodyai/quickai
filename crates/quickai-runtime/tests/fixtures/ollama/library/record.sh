#!/usr/bin/env bash
# Records the pages of ollama.com's library search that the look-up is held to (docs/engine-facts.md, O23 to O26).
# ollama.com has no JSON listing of its library, so the look-up reads the HTML of /search. The pages are kept so
# that a change of the site shows up as a diff when this is run again, and the tests that read them do not need
# the internet.
#
#   record.sh [--out DIR]
#
# Each page is trimmed to what the look-up reads: the results (from the element with id="searchresults" to the end
# of <main>), with the geometry of the icons (the `d` attribute of the svg paths: a third of a page) emptied. The
# pages are what the look-up asks for, `?c=local` and no more: the other filters stay at their defaults.
# One request per page, an honest User-Agent, nothing else is fetched. Needs bash, curl and python3.
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
mkdir -p "$out"

record() { # record FILE QUERY
    local file=$1 url="https://ollama.com/search?$2"
    curl -fsS --max-time 30 -A "quickai-record (fixtures for the tests)" "$url" -o "$out/$file.raw"
    python3 -I - "$out/$file.raw" "$out/$file" <<'PY'
import re, sys
text = open(sys.argv[1], encoding="utf-8").read()
at = text.find('id="searchresults"')
end = text.find("</main>")
if at < 0 or end < 0:
    sys.exit("record.sh: the page has no results element: the site changed its layout")
start = text.rfind("<", 0, at)
body = re.sub(r' d="[^"]*"', ' d=""', text[start:end])
open(sys.argv[2], "w", encoding="utf-8").write(body + "\n")
PY
    rm -f "$out/$file.raw"
    printf '%s\t%s\t%s\n' "$file" "$url" "$(date -u +%Y-%m-%d)" >>"$out/index.tsv.new"
    echo "recorded $file ($(wc -c <"$out/$file") bytes) from $url"
}

: >"$out/index.tsv.new"
record popular_local.html "c=local"
record gpt_local.html "q=gpt&c=local"
record embed_local.html "q=embed&c=local"
record none_local.html "q=zzzzqqqq&c=local"
mv "$out/index.tsv.new" "$out/index.tsv"
