#!/usr/bin/env python3
"""collect_runs: one line for each record run_workflow.py kept (bakeoff/runs/*.json): what ran, how long, how much memory, whether it worked.

    collect_runs.py [bakeoff/runs]            a table on the screen
    collect_runs.py [bakeoff/runs] --csv      the same as CSV (to open in a spreadsheet and add the scores of PLAN.md)
"""
import csv
import json
import sys
from pathlib import Path

args = [a for a in sys.argv[1:] if not a.startswith("--")]
folder = Path(args[0] if args else "bakeoff/runs")
rows = []
for f in sorted(folder.glob("*.json")):
    r = json.load(open(f))
    rows.append([r.get("label"), r.get("workflow"), "x".join(map(str, r.get("size", []))), r.get("frames"), r.get("status"),
                 r.get("comfyui_seconds"), r.get("wall_seconds"), r.get("vram_peak_gib"), r.get("vram_total_gib"), (r.get("error") or {}).get("message", "")[:60],
                 ";".join(Path(x).name for x in r.get("files", []))])
head = ["label", "workflow", "size", "frames", "status", "comfyui_s", "wall_s", "vram_peak_gib", "vram_total_gib", "error", "files"]
if "--csv" in sys.argv:
    w = csv.writer(sys.stdout)
    w.writerow(head + ["style", "identity", "motion", "speech", "voice_match", "lip_sync", "artifacts", "notes"])
    w.writerows(rows)
else:
    print(" | ".join(head[:9]))
    for r in rows:
        print(" | ".join("" if v is None else str(v) for v in r[:9]), ("  ERROR: " + r[9]) if r[9] else "")
    print(len(rows), "runs")
