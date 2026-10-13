#!/usr/bin/env python3
"""run_workflow: run one workflow of this package on a ComfyUI, and keep what happened (the clip, the times, the memory it took).

    run_workflow.py --server http://127.0.0.1:8189 WORKFLOW.json --prompt "..." [options]

    --prompt TEXT | --prompt-file FILE    the words (a workflow that has a negative prompt keeps its own unless --negative is given)
    --seed N                              default 1
    --aspect 16:9|9:16|1:1                the size comes from `sizes` of slots.json for that workflow (default: its first); or --size 1344x768
    --seconds S [--fps F]                 the length; the frames are the next number the model allows (slots.json `frames_rule`)
    --first-image F  --last-image F       pictures to start from / end on (a workflow without such a slot refuses them)
    --ref-image F (repeatable)            reference pictures; --ref-audio F (repeatable) voice clips; --ref-image-size match|max
    --steps N                             only for the workflows whose scheduler is a plain `steps` input (H3, FastH3: leave it, 8 is fixed)
    --out DIR                             where the clip and the record (<label>.json) are written (default ./runs)
    --label NAME                          the name of the run (default: the workflow's name and the seed)
    --dry-run                             print the graph that would be sent and stop; nothing is uploaded or sent
    --timeout SECONDS                     give up waiting (default 3600)

It uploads the pictures and sounds (POST /upload/image: a ComfyUI keeps them in its input folder, with the name made from their content),
puts them and the values into the slots named in slots.json (next to the workflows, or --slots FILE), sends the graph (POST /prompt),
polls /history and /system_stats once a second, fetches the clip (GET /view) and writes the record: the seconds ComfyUI took (its own
start and end), the seconds from sending to the file, the smallest free graphics memory seen while it ran (so the most it held), the
version of ComfyUI and the name of the card. Only the standard library of Python 3.11 or later is used.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import mimetypes
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from pathlib import Path


def http_json(method: str, url: str, body=None, timeout=60):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method, headers={"Content-Type": "application/json"} if body is not None else {})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            raw = r.read()
            return r.status, (json.loads(raw) if raw else None)
    except urllib.error.HTTPError as e:
        raw = e.read()
        try:
            return e.code, json.loads(raw)
        except Exception:
            return e.code, {"raw": raw.decode("utf-8", "replace")[:800]}


def upload(server: str, path: Path) -> str:
    """POST /upload/image of the file under a name made from its content, so the same file is one file however often it is sent."""
    data = path.read_bytes()
    name = "run-" + hashlib.sha256(data).hexdigest()[:16] + path.suffix.lower()
    boundary = uuid.uuid4().hex
    ctype = mimetypes.guess_type(path.name)[0] or "application/octet-stream"
    parts = []
    for k, v in (("type", "input"), ("overwrite", "true")):
        parts.append(f'--{boundary}\r\nContent-Disposition: form-data; name="{k}"\r\n\r\n{v}\r\n'.encode())
    parts.append(f'--{boundary}\r\nContent-Disposition: form-data; name="image"; filename="{name}"\r\nContent-Type: {ctype}\r\n\r\n'.encode())
    parts.append(data)
    parts.append(f"\r\n--{boundary}--\r\n".encode())
    req = urllib.request.Request(server + "/upload/image", data=b"".join(parts), method="POST",
                                 headers={"Content-Type": f"multipart/form-data; boundary={boundary}"})
    with urllib.request.urlopen(req, timeout=300) as r:
        return json.loads(r.read())["name"]


def frames_for(seconds: float, fps: float, rule: dict) -> int:
    """The smallest number of frames >= seconds*fps that the model allows (n % step == extra % step)."""
    n = max(1, round(seconds * fps))
    step, extra = rule["step"], rule["extra"]
    while n % step != extra % step:
        n += 1
    return n


class Meter(threading.Thread):
    """Polls /system_stats once a second and keeps the least free graphics memory it saw."""

    def __init__(self, server: str):
        super().__init__(daemon=True)
        self.server = server
        self.stop = threading.Event()
        self.min_free = None
        self.total = None
        self.device = None

    def run(self):
        while not self.stop.is_set():
            try:
                s, d = http_json("GET", self.server + "/system_stats", timeout=10)
                dev = d["devices"][0]
                self.device = dev.get("name")
                self.total = dev.get("vram_total")
                free = dev.get("vram_free")
                if free is not None and (self.min_free is None or free < self.min_free):
                    self.min_free = free
            except Exception:
                pass
            self.stop.wait(1.0)


def set_slot(graph: dict, target, value) -> None:
    nid, field = target
    if nid not in graph:
        raise SystemExit(f"slot target {nid}.{field} is not in the workflow")
    graph[nid]["inputs"][field] = value


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("workflow")
    ap.add_argument("--server", default="http://127.0.0.1:8189")
    ap.add_argument("--slots")
    ap.add_argument("--prompt")
    ap.add_argument("--prompt-file")
    ap.add_argument("--negative")
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--aspect")
    ap.add_argument("--size")
    ap.add_argument("--seconds", type=float)
    ap.add_argument("--fps", type=float)
    ap.add_argument("--first-image")
    ap.add_argument("--last-image")
    ap.add_argument("--ref-image", action="append", default=[])
    ap.add_argument("--ref-audio", action="append", default=[])
    ap.add_argument("--ref-image-size", choices=["match", "max"])
    ap.add_argument("--steps", type=int)
    ap.add_argument("--out", default="runs")
    ap.add_argument("--label")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--timeout", type=float, default=3600)
    a = ap.parse_args()

    wf_path = Path(a.workflow).resolve()
    slots_path = Path(a.slots) if a.slots else wf_path.parent.parent / "slots.json"
    key = f"{wf_path.parent.name}/{wf_path.stem}"
    all_slots = json.load(open(slots_path))
    if key not in all_slots:
        raise SystemExit(f"{key} is not in {slots_path}")
    slots = all_slots[key]
    graph = {k: v for k, v in json.load(open(wf_path)).items() if not k.startswith("_")}
    prompt = a.prompt if a.prompt is not None else (Path(a.prompt_file).read_text().strip() if a.prompt_file else None)
    if prompt is None:
        raise SystemExit("give --prompt or --prompt-file")
    label = a.label or f"{wf_path.stem}-seed{a.seed}"

    # ---- the values ------------------------------------------------------------------------------------------------------------------------
    set_slot(graph, slots["prompt"], prompt)
    if a.negative is not None:
        if "negative" not in slots:
            raise SystemExit("this workflow has no negative prompt")
        set_slot(graph, slots["negative"], a.negative)
    set_slot(graph, slots["seed"], a.seed)
    sizes = slots.get("sizes", {})
    if a.size:
        w, h = (int(x) for x in a.size.lower().split("x"))
    else:
        aspect = a.aspect or next(iter(sizes), None)
        if aspect is None or aspect not in sizes:
            raise SystemExit(f"--aspect must be one of {list(sizes)}")
        w, h = sizes[aspect]
    set_slot(graph, slots["width"], w)
    set_slot(graph, slots["height"], h)
    fps = a.fps or slots.get("default_fps")
    if a.seconds is not None:
        frames = frames_for(a.seconds, fps, slots["frames_rule"])
        set_slot(graph, slots["frames"], frames)
    else:
        frames = graph[slots["frames"][0]]["inputs"][slots["frames"][1]]
    if a.fps is not None and "fps" in slots:
        set_slot(graph, slots["fps"], a.fps)
    if a.steps is not None:
        sched = [k for k, v in graph.items() if v["class_type"] == "BasicScheduler"]
        if not sched:
            raise SystemExit("--steps is for the H3 workflows (a BasicScheduler); the others fix their steps")
        graph[sched[0]]["inputs"]["steps"] = a.steps
    if a.ref_image_size:
        if "ref_image_size" not in slots:
            raise SystemExit("--ref-image-size is for the reference workflows")
        set_slot(graph, slots["ref_image_size"], a.ref_image_size)

    def place(kind: str, files: list[str], slot_key: str):
        if not files:
            return
        if slot_key not in slots:
            raise SystemExit(f"this workflow takes no {kind}")
        places = slots[slot_key] if isinstance(slots[slot_key][0], list) else [slots[slot_key]]
        if len(files) > len(places):
            raise SystemExit(f"this workflow has {len(places)} place(s) for {kind}, {len(files)} given")
        for k, target in enumerate(places):
            f = files[k] if k < len(files) else files[0]          # a place left over takes the first again, as bampu does
            graph_value = ("DRY:" + Path(f).name) if a.dry_run else upload(a.server, Path(f))
            set_slot(graph, target, graph_value)

    place("a first picture", [a.first_image] if a.first_image else [], "first_image")
    place("a last picture", [a.last_image] if a.last_image else [], "last_image")
    place("reference pictures", a.ref_image, "ref_images")
    place("voice clips" if slots.get("audio_mode") != "track" else "a recorded track", a.ref_audio, "ref_audios")
    if "first_image" in slots and not a.first_image and not a.dry_run:
        raise SystemExit("this workflow starts from a picture: give --first-image")
    if "last_image" in slots and not a.last_image and not a.dry_run:
        raise SystemExit("this workflow ends on a picture: give --last-image")
    if "ref_images" in slots and not a.ref_image and not a.dry_run:
        raise SystemExit("this workflow needs --ref-image")

    if a.dry_run:
        print(json.dumps({"label": label, "size": [w, h], "frames": frames, "graph": graph}, indent=1, ensure_ascii=False))
        return 0

    # ---- run -------------------------------------------------------------------------------------------------------------------------------
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    s, stats = http_json("GET", a.server + "/system_stats")
    meter = Meter(a.server)
    meter.start()
    t0 = time.time()
    s, answer = http_json("POST", a.server + "/prompt", {"prompt": graph, "client_id": "run-workflow"})
    if s != 200:
        meter.stop.set()
        print("REFUSED by ComfyUI:", json.dumps(answer, ensure_ascii=False)[:1500])
        return 1
    pid = answer["prompt_id"]
    print(f"sent {label} as {pid}: {w}x{h}, {frames} frames")
    hist = None
    while time.time() - t0 < a.timeout:
        s, h_ = http_json("GET", f"{a.server}/history/{pid}")
        if s == 200 and h_.get(pid):
            hist = h_[pid]
            if hist.get("status", {}).get("completed") or hist.get("status", {}).get("status_str") == "error":
                break
        time.sleep(1.0)
    t_done = time.time()
    meter.stop.set()
    meter.join(timeout=3)
    if hist is None:
        print(f"no answer in {a.timeout:.0f} s")
        return 1
    st = hist.get("status", {})
    times = {m[0]: m[1].get("timestamp") for m in st.get("messages", []) if isinstance(m[1], dict)}
    comfy_s = (times["execution_success"] - times["execution_start"]) / 1000 if "execution_success" in times and "execution_start" in times else None
    record = dict(label=label, workflow=key, comfyui=stats["system"].get("comfyui_version"), device=meter.device, prompt_id=pid,
                  size=[w, h], frames=frames, seed=a.seed, prompt=prompt, status=st.get("status_str"),
                  comfyui_seconds=comfy_s, wall_seconds=round(t_done - t0, 1),
                  vram_total_gib=round(meter.total / 2**30, 2) if meter.total else None,
                  vram_peak_gib=round((meter.total - meter.min_free) / 2**30, 2) if meter.total and meter.min_free is not None else None,
                  files=[])
    if st.get("status_str") == "error":
        for m in st.get("messages", []):
            if m[0] == "execution_error":
                record["error"] = dict(node=m[1].get("node_id"), type=m[1].get("node_type"), message=str(m[1].get("exception_message"))[:600])
    for node_id, o in (hist.get("outputs") or {}).items():
        for kind in ("images", "video", "videos", "gifs", "audio"):
            for f in o.get(kind, []) or []:
                q = urllib.parse.urlencode(dict(filename=f["filename"], subfolder=f.get("subfolder", ""), type=f.get("type", "output")))
                dest = out / f"{label}__{f['filename']}"
                with urllib.request.urlopen(f"{a.server}/view?{q}", timeout=600) as r:
                    dest.write_bytes(r.read())
                record["files"].append(str(dest))
    (out / f"{label}.json").write_text(json.dumps(record, indent=1, ensure_ascii=False) + "\n")
    print(json.dumps({k: v for k, v in record.items() if k not in ("prompt",)}, indent=1, ensure_ascii=False))
    return 0 if st.get("status_str") != "error" else 1


if __name__ == "__main__":
    sys.exit(main())
