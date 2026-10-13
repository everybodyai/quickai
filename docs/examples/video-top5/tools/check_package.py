#!/usr/bin/env python3
"""check_package: does the package hold together?

    check_package.py PACKAGE_DIR [--templates DIR --object-info FILE] [--server URL --base DIR]

PACKAGE_DIR holds profiles/*.toml, workflows/<option>/*.json and workflows/slots.json. It checks, without a GPU:
  1. every slot of slots.json names a node and an input that are in its workflow (and the output node is a SaveVideo);
  2. every model file a loader of a workflow names is a file its option's configuration fetches, into the folder that loader reads
     (the one exception is said: the LTX-2.5 spatial upscaler, which is put in by hand until the ComfyUI document lists its folder);
  3. with --templates and --object-info: what each workflow dropped and added against the official template it was made from;
  4. with --server and --base: that a ComfyUI (a throw-away one, with no real models) accepts every workflow (validate_api.py).
Exit 1 if 1, 2 or 4 find anything. Only the standard library of Python 3.11 or later is used.
"""
from __future__ import annotations

import argparse
import collections
import json
import sys
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

OPTION_CONFIG = {"01-h3-base": "minimax-h3.toml", "02-h3-turbo": "minimax-h3.toml", "03-fasth3-v2": "fasth3-v2.toml",
                 "04-ltx-2.5": "ltx-2.5-distilled.toml", "05-wan22-14b": "wan22-14b-i2v-lightning.toml"}
LOADERS = {"UNETLoader": ("unet_name", "diffusion_models"), "CLIPLoader": ("clip_name", "text_encoders"), "VAELoader": ("vae_name", "vae"),
           "LoraLoaderModelOnly": ("lora_name", "loras"), "LatentUpscaleModelLoader": ("model_name", "latent_upscale_models")}
BY_HAND = {("latent_upscale_models", "ltx-2.5-latent-spatial-upscaler-x2-bf16-1.0.safetensors")}
SOURCES = {   # workflow -> template(s) it was made from
    "h3-": "video_minimax_h3_i2v", "h3-ref2v": "video_minimax_h3_r2v", "h3-turbo-ref2v": "video_minimax_h3_r2v",
    "fasth3-t2v": "video_fastvideo_fasth3_t2v", "fasth3-": "video_fastvideo_fasth3_i2v",
    "ltx25-t2v": "video_ltx2_5_t2v", "ltx25-i2v": "video_ltx2_5_i2v", "ltx25-flf2v": "video_ltx2_5_flf2v",
    "wan22-i2v": "video_wan2_2_14B_i2v", "wan22-flf2v": "video_wan2_2_14B_flf2v",
}


def source_of(name: str) -> str:
    best = None
    for prefix, tpl in SOURCES.items():
        if name.startswith(prefix) and (best is None or len(prefix) > len(best[0])):
            best = (prefix, tpl)
    return best[1] if best else ""


def load_config(path: Path) -> dict:
    d = tomllib.loads(path.read_text())
    return {m["file"]: m.get("into") for m in d.get("model", [])}


FAMILY_KEYS = {"sizes", "frames_rule", "default_fps", "seconds_range", "steps", "audio_mode"}      # facts about the model, not places in the graph


def check_slots(root: Path) -> int:
    bad = 0
    slots = json.load(open(root / "workflows" / "slots.json"))
    for key, s in slots.items():
        g = json.load(open(root / "workflows" / (key + ".json")))
        for slot, target in s.items():
            if slot in FAMILY_KEYS:
                continue
            if slot == "output":
                ok = target in g and g[target]["class_type"] == "SaveVideo"
                if not ok:
                    print(f"  SLOT  {key}: output {target} is not a SaveVideo"); bad += 1
                continue
            targets = target if isinstance(target[0], list) else [target]
            for nid, field in targets:
                if nid not in g or field not in g[nid]["inputs"]:
                    print(f"  SLOT  {key}: {slot} -> {nid}.{field} is not in the workflow"); bad += 1
    print(f"slots: {sum(len([k for k in v if k not in FAMILY_KEYS]) for v in slots.values())} slots in {len(slots)} workflows, {bad} wrong")
    return bad


def check_models(root: Path) -> int:
    bad = 0
    configs = {n: load_config(root / "profiles" / n) for n in set(OPTION_CONFIG.values())}
    used = collections.defaultdict(set)
    for opt, cfg in OPTION_CONFIG.items():
        for wf in sorted((root / "workflows" / opt).glob("*.json")):
            g = json.load(open(wf))
            for nid, node in g.items():
                ct = node["class_type"]
                if ct in LOADERS:
                    field, folder = LOADERS[ct]
                    name = node["inputs"][field]
                    into = configs[cfg].get(name)
                    if into is None and (folder, name) in BY_HAND:
                        continue
                    if into != folder:
                        print(f"  MODEL {opt}/{wf.name}: node {nid} {ct} reads {folder}/{name}, configuration {cfg} has "
                              f"{'no such file' if into is None else 'it in ' + str(into)}"); bad += 1
                    else:
                        used[cfg].add(name)
    for cfg, files in configs.items():
        unused = sorted(set(files) - used[cfg])
        if unused:
            print(f"  note: {cfg} fetches {len(unused)} file(s) no workflow here loads: {', '.join(unused)}")
    print(f"models: every loader checked against its configuration, {bad} wrong")
    return bad


def diff_templates(root: Path, templates: Path, object_info: Path) -> None:
    from ui2api import convert
    oi = json.load(open(object_info))
    cache = {}
    print("against the official templates (node classes the workflow dropped / added):")
    for opt in sorted(OPTION_CONFIG):
        for wf in sorted((root / "workflows" / opt).glob("*.json")):
            tpl = source_of(wf.stem)
            if not tpl:
                continue
            if tpl not in cache:
                cache[tpl] = collections.Counter(n["class_type"] for n in convert(json.load(open(templates / f"{tpl}.json")), oi).values())
            have = collections.Counter(n["class_type"] for n in json.load(open(wf)).values())
            dropped = cache[tpl] - have
            added = have - cache[tpl]
            fmt = lambda c: ", ".join(f"{k}x{v}" if v > 1 else k for k, v in sorted(c.items())) or "-"   # noqa: E731
            print(f"  {opt}/{wf.name}  <- {tpl}\n      dropped: {fmt(dropped)}\n      added:   {fmt(added)}")


def check_server(root: Path, server: str, base: Path) -> int:
    import validate_api as V
    bad = 0
    for wf in sorted((root / "workflows").glob("*/*.json")):
        g = json.load(open(wf))
        ok, lines = V.check(server, g, base)
        if not ok:
            print(f"  REFUSED {wf.relative_to(root)}"); [print("     " + l) for l in lines]; bad += 1
    print(f"server: {len(list((root / 'workflows').glob('*/*.json')))} workflows posted to {server}, {bad} refused")
    return bad


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("package")
    ap.add_argument("--templates")
    ap.add_argument("--object-info")
    ap.add_argument("--server")
    ap.add_argument("--base")
    a = ap.parse_args()
    root = Path(a.package)
    bad = check_slots(root) + check_models(root)
    if a.templates and a.object_info:
        diff_templates(root, Path(a.templates), Path(a.object_info))
    if a.server and a.base:
        bad += check_server(root, a.server, Path(a.base))
    print("OK" if not bad else f"{bad} PROBLEM(S)")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
