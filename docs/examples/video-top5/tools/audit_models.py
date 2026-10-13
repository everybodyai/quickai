#!/usr/bin/env python3
"""audit_models: does every model loader of an API-format workflow name a file that its template declares for that folder?

    audit_models.py TEMPLATE.json WORKFLOW.json [WORKFLOW.json ...]

The template (UI format) carries, on its loader nodes, properties.models = [{name, url, directory}]. A workflow made from it must name, in each
loader, a file declared for the folder that loader reads (UNETLoader: diffusion_models, CLIPLoader: text_encoders, VAELoader: vae,
LoraLoaderModelOnly: loras, LatentUpscaleModelLoader: latent_upscale_models). Prints one line per loader and exits 1 on any that is not.
"""
import json
import sys

FOLDERS = {"UNETLoader": ("unet_name", "diffusion_models"), "CLIPLoader": ("clip_name", "text_encoders"), "VAELoader": ("vae_name", "vae"),
           "LoraLoaderModelOnly": ("lora_name", "loras"), "LoraLoader": ("lora_name", "loras"),
           "LatentUpscaleModelLoader": ("model_name", "latent_upscale_models")}


def declared(template: dict) -> set:
    nodes = list(template["nodes"])
    for s in template.get("definitions", {}).get("subgraphs", []):
        nodes += s["nodes"]
    out = set()
    for n in nodes:
        for m in (n.get("properties") or {}).get("models") or []:
            out.add((m["directory"], m["name"]))
    return out


def audit(template_path: str, workflow_paths: list[str]) -> int:
    decl = declared(json.load(open(template_path)))
    bad = 0
    for wp in workflow_paths:
        g = json.load(open(wp))
        print(wp)
        for gid, node in g.items():
            if gid.startswith("_"):
                continue
            ct = node["class_type"]
            if ct in FOLDERS:
                field, folder = FOLDERS[ct]
                v = node["inputs"].get(field)
                ok = (folder, v) in decl
                print(f"   {'ok ' if ok else 'BAD'} {gid:>10} {ct:<26} {folder:<22} {v}")
                bad += 0 if ok else 1
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(audit(sys.argv[1], sys.argv[2:]))
