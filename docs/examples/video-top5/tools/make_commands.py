#!/usr/bin/env python3
"""make_commands: prints the commands of the bake-off (bakeoff/PLAN.md) for a ComfyUI, one line a run. Nothing is run.

    make_commands.py --server http://127.0.0.1:8189 [--only SUBSTRING] > bakeoff/commands.sh

Each line is a run_workflow.py call: a shot of bakeoff/shots.json made by one workflow of workflows/ with the prompt of bakeoff/prompts/<way>/<shot>.txt.
The pictures are the ones you put in bakeoff/pics/ (see PLAN.md for their names) and the voice clips the ones in bakeoff/voices/. --only keeps the
lines that hold the substring (a shot such as ` s3 `, a lane such as `fasth3`, a workflow such as `ref2v-voice`). The runs go to bakeoff/runs/.
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(HERE))
from render_prompts import order, speakers  # noqa: E402  (the pictures and the voices go in the order the prompts number them)

# (shot, lane, workflow, prompt way, seconds, pictures, voices, first picture)    pictures: the sheets of the cast then the plate of the place
# A picture named key_<shot> is a still of the opening of the shot (the cast placed in the place); without one the sheet of the first character is used.
MATRIX = [
    # s1 an establishing shot, no cast
    ("s1", "h3-base",  "01-h3-base/h3-t2v",              "h3-t2va",     8, [], [], None),
    ("s1", "h3-turbo", "02-h3-turbo/h3-turbo-t2v",       "h3-t2va",     8, [], [], None),
    ("s1", "fasth3",   "03-fasth3-v2/fasth3-t2v",        "h3-t2va",     8, [], [], None),
    ("s1", "ltx",      "04-ltx-2.5/ltx25-t2v",           "ltx-t2v",     8, [], [], None),
    ("s1", "wan",      "05-wan22-14b/wan22-i2v-lightning", "wan",       5, [], [], "place"),
    # s2 slapstick, one character, no line
    ("s2", "h3-base",  "01-h3-base/h3-ref2v",            "h3-sixfield", 8, ["cast", "place"], [], None),
    ("s2", "h3-base-director", "01-h3-base/h3-ref2v",    "h3-director", 8, ["cast", "place"], [], None),
    ("s2", "h3-turbo", "02-h3-turbo/h3-turbo-ref2v",     "h3-sixfield", 8, ["cast", "place"], [], None),
    ("s2", "fasth3",   "03-fasth3-v2/fasth3-i2v",        "h3-fl2va",    8, [], [], "key"),
    ("s2", "ltx",      "04-ltx-2.5/ltx25-i2v",           "ltx",         8, [], [], "key"),
    ("s2", "wan",      "05-wan22-14b/wan22-i2v-lightning", "wan",       5, [], [], "key"),
    ("s2", "wan-full", "05-wan22-14b/wan22-i2v-full",    "wan",         5, [], [], "key"),
    # s3 one short line
    ("s3", "h3-base",  "01-h3-base/h3-ref2v-voice",      "h3-sixfield", 6, ["cast", "place"], ["speakers"], None),
    ("s3", "h3-base-director", "01-h3-base/h3-ref2v-voice", "h3-director", 6, ["cast", "place"], ["speakers"], None),
    ("s3", "h3-base-ownvoice", "01-h3-base/h3-i2v",      "h3-fl2va",    6, [], [], "key"),
    ("s3", "h3-base-track", "01-h3-base/h3-ref2v-track", "h3-sixfield-track", 6, ["cast", "place"], ["line"], None),
    ("s3", "h3-base-track-director", "01-h3-base/h3-ref2v-track", "h3-director-track", 6, ["cast", "place"], ["line"], None),
    ("s3", "h3-turbo", "02-h3-turbo/h3-turbo-ref2v-voice", "h3-sixfield", 6, ["cast", "place"], ["speakers"], None),
    ("s3", "fasth3",   "03-fasth3-v2/fasth3-i2v",        "h3-fl2va",    6, [], [], "key"),
    ("s3", "ltx",      "04-ltx-2.5/ltx25-i2v",           "ltx",         6, [], [], "key"),
    ("s3", "wan",      "05-wan22-14b/wan22-i2v-lightning", "wan",       5, [], [], "key"),
    # s4 one long line
    ("s4", "h3-base",  "01-h3-base/h3-ref2v-voice",      "h3-sixfield", 9, ["cast", "place"], ["speakers"], None),
    ("s4", "h3-turbo", "02-h3-turbo/h3-turbo-ref2v-voice", "h3-sixfield", 9, ["cast", "place"], ["speakers"], None),
    ("s4", "h3-base-track", "01-h3-base/h3-ref2v-track", "h3-sixfield-track", 9, ["cast", "place"], ["line"], None),
    ("s4", "ltx",      "04-ltx-2.5/ltx25-i2v",           "ltx",         9, [], [], "key"),
    # s5 two speakers
    ("s5", "h3-base",  "01-h3-base/h3-ref2v-voice",      "h3-sixfield", 6, ["cast", "place"], ["speakers"], None),
    ("s5", "h3-base-director", "01-h3-base/h3-ref2v-voice", "h3-director", 6, ["cast", "place"], ["speakers"], None),
    ("s5", "fasth3",   "03-fasth3-v2/fasth3-i2v",        "h3-fl2va",    6, [], [], "key"),
    ("s5", "ltx",      "04-ltx-2.5/ltx25-i2v",           "ltx",         6, [], [], "key"),
    # s6 the vertical version of s3
    ("s6", "h3-base",  "01-h3-base/h3-ref2v-voice",      "h3-sixfield", 6, ["cast", "place"], ["speakers"], None),
    ("s6", "fasth3",   "03-fasth3-v2/fasth3-i2v",        "h3-fl2va",    6, [], [], "key"),
    ("s6", "ltx",      "04-ltx-2.5/ltx25-i2v",           "ltx",         6, [], [], "key"),
]
SLUG = {"Shikari Shambu": "shambu", "Jeeva Jeevantak": "jeeva", "Chandanpur village square": "square", "The dry waterhole": "waterhole",
        "Sundar Van jungle trail": "trail", "Rani's cave": "cave"}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--server", default="http://127.0.0.1:8189")
    ap.add_argument("--only")
    ap.add_argument("--by-shot", action="store_true", help="keep the order of the table (by shot) instead of grouping by workflow")
    a = ap.parse_args()
    shots = {s["id"]: s for s in json.load(open(ROOT / "bakeoff" / "shots.json"))["shots"]}
    print("#!/bin/sh\n# Generated by tools/make_commands.py; one run a line; run from the package folder. Nothing here has been run.\n# The first run of a workflow loads its models (minutes); judge the time of a run by the second of the same workflow.\nset -e")
    rows = MATRIX if a.by_shot else sorted(MATRIX, key=lambda r: (r[2].split("/")[0], "ref2v" in r[2], r[2], r[0]))     # grouped by model: the reference build of H3 is a file of its own
    last = None
    for shot, lane, wf, way, seconds, pics, voices, first in rows:
        s = shots[shot]
        aspect = s.get("aspect", "16:9")
        cmd = [f"python3 tools/run_workflow.py --server {a.server} workflows/{wf}.json",
               f"--prompt-file bakeoff/prompts/{way}/{shot}.txt", f"--aspect {aspect}", f"--seconds {seconds}", f"--seed 1",
               f"--label {shot}-{lane}", "--out bakeoff/runs"]
        names = [SLUG[c] for c in order(s)]
        place = SLUG[s["place"]]
        if "cast" in pics:
            cmd += [f"--ref-image bakeoff/pics/{n}.png" for n in names] + [f"--ref-image bakeoff/pics/{place}.png"]
        if voices == ["line"]:
            cmd.append(f"--ref-audio bakeoff/voices/line_{shot}.wav")        # the whole spoken part of the shot as one recorded track
        elif voices:
            cmd += [f"--ref-audio bakeoff/voices/{SLUG[n]}.wav" for n in speakers(s)]
        if first == "key":
            cmd.append(f"--first-image bakeoff/pics/key_{shot}.png")
        elif first == "place":
            cmd.append(f"--first-image bakeoff/pics/{place}.png")
        line = " ".join(cmd)
        if a.only is None or a.only in f" {shot} {lane} {wf} ":
            if wf != last and not a.by_shot:
                print(f"# ---- {wf}")
                last = wf
            print(line)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
