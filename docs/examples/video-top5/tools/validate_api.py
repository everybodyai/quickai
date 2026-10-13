#!/usr/bin/env python3
"""validate_api: ask a ComfyUI whether it accepts API-format workflows (POST /prompt), without running them.

    validate_api.py [--server http://127.0.0.1:18188] [--base DIR] WORKFLOW.json ...

--base is the base directory of that ComfyUI (the one it was started with --base-directory): for every model file and picture a workflow names
that is not there yet an EMPTY file of that name is made, so that ComfyUI's check of the names (a combo must hold a name it lists) passes and
what is left is the check of the graph itself: the node classes, the inputs each needs, their types and ranges, the links. The workflows are
then taken out of the queue; the empty files make every one fail at its first loader anyway, which is also said.

Use it only against a ComfyUI that has no real models and is not the one anyone works with. Only the standard library is used.
"""
from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

FOLDERS = {   # class -> (input, folder under models/)
    "UNETLoader": ("unet_name", "diffusion_models"),
    "CLIPLoader": ("clip_name", "text_encoders"),
    "DualCLIPLoader": ("clip_name1", "text_encoders"),
    "VAELoader": ("vae_name", "vae"),
    "LoraLoaderModelOnly": ("lora_name", "loras"),
    "LoraLoader": ("lora_name", "loras"),
    "LatentUpscaleModelLoader": ("model_name", "latent_upscale_models"),
    "CheckpointLoaderSimple": ("ckpt_name", "checkpoints"),
    "AudioEncoderLoader": ("audio_encoder_name", "audio_encoders"),
    "CLIPVisionLoader": ("clip_name", "clip_vision"),
    "ModelPatchLoader": ("name", "model_patches"),
}


def http(method: str, url: str, body=None, timeout=60):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method, headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, json.loads(r.read() or b"null")
    except urllib.error.HTTPError as e:
        raw = e.read()
        try:
            return e.code, json.loads(raw)
        except Exception:
            return e.code, {"raw": raw.decode("utf-8", "replace")[:500]}


def make_standins(graph: dict, base: Path) -> list[str]:
    made = []
    for node in graph.values():
        ct = node.get("class_type")
        if ct in FOLDERS:
            field, folder = FOLDERS[ct]
            name = node["inputs"].get(field)
            if isinstance(name, str):
                p = base / "models" / folder / name
                if not p.exists():
                    p.parent.mkdir(parents=True, exist_ok=True)
                    p.write_bytes(b"")
                    made.append(str(p.relative_to(base)))
        elif ct == "LoadImage":
            name = node["inputs"].get("image")
            if isinstance(name, str):
                p = base / "input" / name
                src = base / "input" / "placeholder.png"
                if not p.exists() and src.exists():
                    p.write_bytes(src.read_bytes())
                    made.append(str(p.relative_to(base)))
        elif ct == "LoadAudio":
            name = node["inputs"].get("audio")
            if isinstance(name, str):
                p = base / "input" / name
                if not p.exists():
                    p.write_bytes(b"")
                    made.append(str(p.relative_to(base)))
    return made


def check(server: str, graph: dict, base: Path | None, quiet=False) -> tuple[bool, list[str]]:
    """True when ComfyUI accepts the graph; the lines say what it did not like (or what it did after accepting)."""
    lines: list[str] = []
    if base is not None:
        for m in make_standins(graph, base):
            lines.append("stand-in made: " + m)
        time.sleep(0.3)
    status, answer = http("POST", server + "/prompt", {"prompt": graph, "client_id": "validate-api"})
    if status != 200:
        err = answer.get("error", {})
        lines.append(f"REFUSED ({status}): {err.get('type')}: {err.get('message')} {err.get('details') or ''}".strip())
        for nid, ne in (answer.get("node_errors") or {}).items():
            cls = ne.get("class_type")
            for e in ne.get("errors", []):
                lines.append(f"  node {nid} {cls}: {e.get('message')} - {str(e.get('details'))[:160]}")
        return False, lines
    pid = answer["prompt_id"]
    # it was accepted: take it out of the queue at once, and say what happened to it if it already ran
    time.sleep(1.0)
    http("POST", server + "/queue", {"delete": [pid]})
    http("POST", server + "/interrupt", {})
    time.sleep(0.5)
    s, hist = http("GET", f"{server}/history/{pid}")
    if s == 200 and hist.get(pid):
        st = hist[pid].get("status", {})
        for m in st.get("messages", []):
            if m[0] == "execution_error":
                e = m[1]
                lines.append(f"accepted; it then stopped at node {e.get('node_id')} {e.get('node_type')}: {str(e.get('exception_message'))[:100]}")
                break
    return True, lines


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--server", default="http://127.0.0.1:18188")
    ap.add_argument("--base", default=None, help="base directory of that ComfyUI (to make empty stand-in files)")
    ap.add_argument("files", nargs="+")
    a = ap.parse_args()
    base = Path(a.base) if a.base else None
    bad = 0
    for f in a.files:
        graph = json.load(open(f))
        graph = {k: v for k, v in graph.items() if not k.startswith("_")}
        ok, lines = check(a.server, graph, base)
        print(("OK       " if ok else "REFUSED  ") + f)
        for l in lines:
            print("   " + l)
        bad += 0 if ok else 1
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
