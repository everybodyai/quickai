#!/usr/bin/env python3
"""hubmeta: size and SHA-256 of files of Hugging Face repositories, at one commit, from the hub's own listing (no file is downloaded).

    hubmeta.py REPO:PATH [REPO:PATH ...] [--rev REPO=COMMIT ...] [--token TOKEN]

For each repository the commit of `main` is asked once (or the --rev given is used), then the folder of each path is listed at that commit
(GET /api/models/<repo>/tree/<commit>/<folder>), which gives, for a file kept with LFS, its size and the SHA-256 (lfs.oid). Prints JSON.
For a GATED repository the hub shows the SHA-256 as asterisks to someone who is not signed in; with --token (or the variable HF_TOKEN) of an
account that accepted the repository's terms it shows it. The token is sent to huggingface.co only.
"""
import json
import os
import sys
import urllib.parse
import urllib.request

TOKEN = os.environ.get("HF_TOKEN", "")


def get(url):
    headers = {"User-Agent": "hubmeta/1"}
    if TOKEN and url.startswith("https://huggingface.co/"):
        headers["Authorization"] = "Bearer " + TOKEN
    req = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.loads(r.read())


def main(argv):
    global TOKEN
    revs = {}
    items = []
    it = iter(argv)
    for a in it:
        if a == "--rev":
            k, v = next(it).split("=", 1)
            revs[k] = v
        elif a == "--token":
            TOKEN = next(it)
        else:
            items.append(a)
    heads = {}
    out = []
    for item in items:
        repo, path = item.split(":", 1)
        if repo not in heads:
            heads[repo] = revs.get(repo) or get(f"https://huggingface.co/api/models/{repo}")["sha"]
        rev = heads[repo]
        folder, _, name = path.rpartition("/")
        url = f"https://huggingface.co/api/models/{repo}/tree/{rev}"
        if folder:
            url += "/" + urllib.parse.quote(folder)
        entry = None
        for e in get(url):
            if e.get("path") == path:
                entry = e
                break
        if entry is None:
            out.append({"repo": repo, "path": path, "error": "not found at " + rev})
            continue
        lfs = entry.get("lfs") or {}
        out.append({"repo": repo, "path": path, "revision": rev, "size": lfs.get("size", entry.get("size")), "sha256": lfs.get("oid"), "type": entry.get("type")})
    print(json.dumps(out, indent=1))


if __name__ == "__main__":
    main(sys.argv[1:])
