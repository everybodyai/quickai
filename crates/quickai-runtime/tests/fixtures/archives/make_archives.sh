#!/usr/bin/env bash
# Builds the real-zstd fixtures of the installer tests (docs/phase3-test-plan.md, group Z).
#
# The generated archives of the unit tests (testkit) are written by this repository's own code,
# so they cannot show that the installer reads what a *real* zstd produces. These can: the real
# `zstd` CLI compressed them, with the settings of the real release archive (one frame, an XXH64
# checksum, a long-distance window) and with two other framings the format allows. The tree inside
# has the layout of ollama-linux-amd64.tar.zst (v0.35.1): the same names, modes and relative
# symlinks, in the same order, and, like the real one, no entries for `bin/` or `lib/ollama/`
# themselves; only the sizes are small and the contents are made up.
#
#   make_archives.sh        rebuilds the three .tar.zst files here (needs zstd, tar, python3)
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d "${TMPDIR:-/var/tmp}/quickai-archives.XXXXXX")
trap 'rm -rf "$work"' EXIT
tree="$work/tree"
mkdir -p "$tree/bin" "$tree/lib/ollama/cuda_v12" "$tree/lib/ollama/cuda_v13" "$tree/lib/ollama/vulkan"

blob() { python3 -c 'import random,sys; sys.stdout.buffer.write(random.Random(sys.argv[1]).randbytes(int(sys.argv[2])))' "$1" "$2"; }

cat >"$tree/bin/ollama" <<'SH'
#!/bin/sh
# A stand-in for the real binary: answers --version the way the real one does with a server up.
case "$1" in
    --version) echo "ollama version is 0.35.1" ;;
    serve) exec sleep 300 ;;
    *) echo "usage: ollama serve" >&2; exit 2 ;;
esac
SH
chmod 755 "$tree/bin/ollama"
printf 'Copyright (c) the Go authors.\n' >"$tree/lib/ollama/GO_LICENSE"
blob base 6000 >"$tree/lib/ollama/libggml-base.so.0.25.3"
blob cpu 6000 >"$tree/lib/ollama/libggml-cpu-x64.so"
printf '#!/bin/sh\nexit 0\n' >"$tree/lib/ollama/llama-server"
blob cu12 24000 >"$tree/lib/ollama/cuda_v12/libggml-cuda.so"
blob rt12 4000 >"$tree/lib/ollama/cuda_v12/libcudart.so.12.8.90"
blob cu13 24000 >"$tree/lib/ollama/cuda_v13/libggml-cuda.so"
blob vk 16000 >"$tree/lib/ollama/vulkan/libggml-vulkan.so"
chmod 755 "$tree/lib/ollama/libggml-base.so.0.25.3" "$tree/lib/ollama/llama-server" \
    "$tree/lib/ollama/cuda_v12/libggml-cuda.so" "$tree/lib/ollama/cuda_v12/libcudart.so.12.8.90" \
    "$tree/lib/ollama/cuda_v13/libggml-cuda.so" "$tree/lib/ollama/vulkan/libggml-vulkan.so"
chmod 644 "$tree/lib/ollama/libggml-cpu-x64.so" "$tree/lib/ollama/GO_LICENSE"
ln -s libggml-base.so.0.25.3 "$tree/lib/ollama/libggml-base.so.0"
ln -s libggml-base.so.0 "$tree/lib/ollama/libggml-base.so"
ln -s libcudart.so.12.8.90 "$tree/lib/ollama/cuda_v12/libcudart.so.12"

# The order of the real archive: the binary, the licences, the CUDA directories (a directory entry,
# then its files), the base libraries and their links, then Vulkan.
cat >"$work/list" <<'LIST'
bin/ollama
lib/ollama/GO_LICENSE
lib/ollama/cuda_v12/
lib/ollama/cuda_v12/libggml-cuda.so
lib/ollama/cuda_v12/libcudart.so.12.8.90
lib/ollama/cuda_v12/libcudart.so.12
lib/ollama/cuda_v13/
lib/ollama/cuda_v13/libggml-cuda.so
lib/ollama/libggml-base.so
lib/ollama/libggml-base.so.0
lib/ollama/libggml-base.so.0.25.3
lib/ollama/libggml-cpu-x64.so
lib/ollama/llama-server
lib/ollama/vulkan/
lib/ollama/vulkan/libggml-vulkan.so
LIST
tar -C "$tree" -cf "$work/mini.tar" --no-recursion --owner=0 --group=0 --numeric-owner \
    --mtime=@1790000000 --format=gnu -T "$work/list"

# 1. what the real release archive is: one frame, XXH64 checksum, a window of 2^23, level 19
zstd -q -19 --long=23 --check -T1 -f "$work/mini.tar" -o "$here/mini-ollama-linux.tar.zst"
# 2. no checksum, the default level (different block types and window)
zstd -q -3 --no-check -T1 -f "$work/mini.tar" -o "$here/mini-ollama-linux-nocheck.tar.zst"
# 3. two frames, cut at a tar block boundary in the middle of a file: a decoder that stops at the
#    end of the first frame loses the rest of the tree
size=$(stat -c %s "$work/mini.tar")
cut=$(( (size / 2 / 512) * 512 + 100 ))   # not a block boundary on purpose: a frame boundary need not be one
head -c "$cut" "$work/mini.tar" >"$work/part1"
tail -c +"$((cut + 1))" "$work/mini.tar" >"$work/part2"
zstd -q -19 --check -T1 -f "$work/part1" -o "$work/part1.zst"
zstd -q -19 --check -T1 -f "$work/part2" -o "$work/part2.zst"
cat "$work/part1.zst" "$work/part2.zst" >"$here/mini-ollama-linux-multiframe.tar.zst"

tar -tvf "$work/mini.tar"
ls -l "$here"/*.tar.zst
for f in "$here"/*.tar.zst; do zstd -q -t "$f" && echo "ok: $(basename "$f")"; done
(cd "$here" && sha256sum ./*.tar.zst >SHA256SUMS)
