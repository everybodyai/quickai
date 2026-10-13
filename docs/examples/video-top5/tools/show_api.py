#!/usr/bin/env python3
"""show_api: print an API-format workflow compactly: id, class, inputs (links as ->id.slot)."""
import json
import sys


def show(path, width=150):
    g = json.load(open(path))
    g = {k: v for k, v in g.items() if not k.startswith("_")}

    def key(k):
        return [int(p) if p.isdigit() else p for p in k.split(":")]

    for gid in sorted(g, key=key):
        n = g[gid]
        parts = []
        for name, v in n["inputs"].items():
            if isinstance(v, list) and len(v) == 2 and isinstance(v[0], str) and isinstance(v[1], int):
                parts.append(f"{name}->{v[0]}.{v[1]}")
            else:
                s = json.dumps(v, ensure_ascii=False)
                parts.append(f"{name}={s[:60]}")
        line = f"{gid:>10} {n['class_type']:<28} " + " ".join(parts)
        print(line[:width * 3])


if __name__ == "__main__":
    for p in sys.argv[1:]:
        print("=" * 30, p)
        show(p)
