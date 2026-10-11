#!/usr/bin/env python3 -I
"""Records the answers of the real Python Package Index that the look-up of an application installed from a wheel reads
(`feed.pypi@1`, `prims/feed.rs`), trimmed to what the look-up reads and what the fakes of the suites must be held to.

    python3 -I record.py [PACKAGE [VERSION]]          (default: open-webui 0.11.4; writes next to this script)

`GET https://pypi.org/pypi/<package>/json` is every release with its files (about 290 KB for Open WebUI, most of it the
package's long description and 166 releases); `GET https://pypi.org/pypi/<package>/<version>/json` is one release. Kept: `info`
reduced to the four fields a look-up could want (the long description and the classifiers are dropped), every file of the
releases kept with every key the index gave it, the twelve newest releases (by upload time) and the newest pre-release of each of three versions (the
index names them `X.Y.Z.devN`), `last_serial`, the files of the page's own release (`urls`). Dropped: `ownership`,
`vulnerabilities`, and the rest of the releases. Nothing else is changed: the digests, sizes, addresses and times are the index's.
"""

import json
import os
import sys
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
INFO_KEPT = ("name", "version", "requires_python", "summary")
NEWEST = 12
PRE_RELEASES = 3


def get(url):
    with urllib.request.urlopen(urllib.request.Request(url, headers={"Accept": "application/json"}), timeout=60) as r:
        return json.load(r)


def trim_info(info):
    return {k: info[k] for k in INFO_KEPT if k in info}


def uploaded(files):
    return max(f["upload_time_iso_8601"] for f in files)


def main():
    package = sys.argv[1] if len(sys.argv) > 1 else "open-webui"
    version = sys.argv[2] if len(sys.argv) > 2 else "0.11.4"
    listing = get(f"https://pypi.org/pypi/{package}/json")
    releases = listing["releases"]
    newest = sorted(releases, key=lambda v: uploaded(releases[v]), reverse=True)
    families = {}
    for v in newest:
        if ".dev" in v:
            families.setdefault(v.split(".dev")[0], v)
    pre = list(families.values())[:PRE_RELEASES]
    keep = list(dict.fromkeys(newest[:NEWEST] + pre))
    out = {
        "info": trim_info(listing["info"]),
        "last_serial": listing["last_serial"],
        "releases": {v: releases[v] for v in sorted(keep)},
        "urls": listing["urls"],
    }
    with open(os.path.join(HERE, f"{package}.json"), "w") as f:
        json.dump(out, f, indent=1, sort_keys=True)
        f.write("\n")
    one = get(f"https://pypi.org/pypi/{package}/{version}/json")
    out = {"info": trim_info(one["info"]), "last_serial": one["last_serial"], "urls": one["urls"]}
    with open(os.path.join(HERE, f"{package}-{version}.json"), "w") as f:
        json.dump(out, f, indent=1, sort_keys=True)
        f.write("\n")
    print(f"{package}: {len(keep)} of {len(releases)} releases kept; the release {version} has {len(one['urls'])} files")


if __name__ == "__main__":
    main()
