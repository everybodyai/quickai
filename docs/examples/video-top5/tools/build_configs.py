#!/usr/bin/env python3
"""build_configs: writes the QuickAI configurations (quickai-run) of the video options from the hub's own listing of their files.

    build_configs.py --meta hubmeta_out.json --out DIR [--date 2026-10-13]

--meta is what hubmeta.py printed (repository, commit, path, size, SHA-256 of each file). A SHA-256 the hub hides (the files of a gated
repository, for someone who is not signed in) is left out of the file and said so in its header: quickai-run then checks the size only.
Only the standard library of Python 3.11 or later is used.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

GB = 1000 ** 3
GIB = 1024 ** 3


def gb(n: int) -> str:
    return f"{n / GB:.1f} GB"


class Meta:
    def __init__(self, items):
        self.by = {(e["repo"], e["path"]): e for e in items}

    def get(self, repo: str, path: str) -> dict:
        e = self.by[(repo, path)]
        if "error" in e:
            raise SystemExit(f"{repo}:{path}: {e['error']}")
        return e


def model(meta: Meta, role: str, name: str, repo: str, path: str, into: str) -> tuple[str, int]:
    e = meta.get(repo, path)
    file = path.rsplit("/", 1)[-1]
    lines = ["[[model]]", f'role = "{role}"', f'name = "{name}"', f'file = "{file}"', f'from = "huggingface:{repo}"']
    if "/" in path:
        lines.append(f'path = "{path}"')
    lines += [f'revision = "{e["revision"]}"', f'into = "{into}"', f"size = {e['size']}"]
    sha = e.get("sha256") or ""
    if len(sha) == 64 and set(sha) <= set("0123456789abcdef"):
        lines.append(f'sha256 = "{sha}"')
    else:
        lines.append("# sha256: the hub does not show it to someone who is not signed in (a gated repository); the size is checked")
    return "\n".join(lines) + "\n", e["size"]


HEAD = '''schema = 1
kind = "profile"
id = "{id}"
requires = {{ app = ">=0.1" }}
name = "{name}"
engine = "comfyui"

[meta]
note = "{note}"

'''
TAIL = '''[values.comfyui]
port = 8188

'''


def write(out: Path, fname: str, header: str, id_: str, name: str, note: str, models, needs: str, extra: str = "") -> int:
    body = HEAD.format(id=id_, name=name, note=note)
    total = 0
    for text, size in models:
        body += text + "\n"
        total += size
    body += TAIL + needs + extra
    (out / fname).write_text(header.rstrip() + "\n" + body.rstrip() + "\n")
    return total


NEED_DIR = '''[[needs]]                               # what this file cannot carry and whoever runs it has to say
setting = "comfyui.models_dir"
why = "Somewhere with {free} GB free: {files} GB of model files, and room for the clips it makes."
'''


# What the computer has to have, which `quickai-run` looks at before it fetches anything (WP4.20; docs/headless.md, section 12): the venv module of
# the Python ComfyUI's environment is made with, the driver of the PyTorch for CUDA 13, and for the models that run through Triton a compiler, the headers
# of Python and a test that a kernel builds and runs. Wan 2.2's fp8 files are run by PyTorch itself: the first two only.
SYSTEM_ALL = '''
[[needs]]                               # what the computer has to have: nobody gives these, `quickai-run` looks (the document of ComfyUI says how)
system = "python_venv"
why = "ComfyUI is installed into a Python environment of its own, and Debian and Ubuntu make none without the venv package of the Python (on Ubuntu python3.12-venv)."

[[needs]]
system = "nvidia_driver"
why = "ComfyUI is installed with PyTorch for CUDA 13, which needs a driver of the 580 series or newer."

[[needs]]
system = "c_compiler"
why = "The int8 and 4-bit files run through Triton, which builds a small helper in C the first time a clip is made; without a compiler that first clip stops with a message that says only that gcc returned non-zero exit status 1."

[[needs]]
system = "python_headers"
why = "Triton builds that helper against the headers of Python (Python.h), which a server often lacks (on Ubuntu the package python3.12-dev)."

[[needs]]
system = "triton"
why = "After the install and before the files are fetched, one kernel is built and run: the first clip is not the one that finds out."
'''

SYSTEM_DRIVER = '''
[[needs]]                               # what the computer has to have: nobody gives it, `quickai-run` looks (the document of ComfyUI says how)
system = "python_venv"
why = "ComfyUI is installed into a Python environment of its own, and Debian and Ubuntu make none without the venv package of the Python (on Ubuntu python3.12-venv)."

[[needs]]
system = "nvidia_driver"
why = "ComfyUI is installed with PyTorch for CUDA 13, which needs a driver of the 580 series or newer."
'''


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--meta", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--date", default="2026-10-13")
    a = ap.parse_args()
    meta = Meta(json.load(open(a.meta)))
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    date = a.date
    H3 = "Comfy-Org/MiniMax-H3"

    # ---- FastH3 V2 ---------------------------------------------------------------------------------------------------------------------
    models = [
        model(meta, "diffusion", "FastH3 V2, 8 steps, pruned, int8", "FastVideo/FastVideo-FastH3-Comfy", "diffusion_models/fastvideo_fasth3_8step_v2_pruned_int8_convrot.safetensors", "diffusion_models"),
        model(meta, "text_encoder", "Qwen3-VL 32B for MiniMax H3, 4 bits", H3, "text_encoders/qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors", "text_encoders"),
        model(meta, "vae", "MiniMax H3 video VAE, int8", H3, "vae/minimax_h3_video_vae_int8_convrot.safetensors", "vae"),
        model(meta, "vae", "MiniMax H3 audio VAE", H3, "vae/minimax_h3_audio_vae_fp32.safetensors", "vae"),
    ]
    total = sum(s for _, s in models)
    header = f"""# A configuration for a server: ComfyUI with the files of FastH3 V2, FastVideo's 8-step distillation of MiniMax H3 (a clip with its sound in
# 8 steps and with block-sparse attention, about three times quicker than the base model on the one card it was timed on). It carries no model.
# Each file says where it can be had from: a repository of the hub, the commit of it that was checked, the file in it, how big it is and what
# its SHA-256 is. `quickai-run up` installs ComfyUI, fetches the four files into the folders ComfyUI looks for each in, and starts it on
# 127.0.0.1:8188.
#
#     quickai-run init  fasth3-v2.toml                                   # the file to fill in: where the models are kept
#     quickai-run check fasth3-v2.toml --environment fasth3-v2.environment.toml --accept-review
#     quickai-run up    fasth3-v2.toml --environment fasth3-v2.environment.toml --accept-review
#
# What the four files are: the distilled transformer ({gb(models[0][1])}, FastVideo/FastVideo-FastH3-Comfy), and the text encoder (Qwen3-VL 32B in
# 4 bits, which does not need a Blackwell card) and the two VAEs of MiniMax H3 itself (Comfy-Org/MiniMax-H3), which the distillation shares
# with the base model. {gb(total)} in all ({total / GIB:.1f} GiB, which is how the program counts). If MiniMax H3's own configuration
# (minimax-h3.toml) is used on the same models folder, the three shared files are there already and are not fetched twice.
#
# What it does and does not do (ComfyUI's page on it and the model card): words to a clip, or a first and/or a last picture to a clip, with its
# sound. It has no reference pictures and no reference sound (the reference build was not distilled). 8 steps, fixed: more or fewer
# makes it worse. Sigma shift 10 for the picture and 3 for the sound, not the base model's 12. "It trades some motion and audio fidelity for
# speed": use it for drafts and for shots without a line to speak; use the base model for the shots that matter. ComfyUI 0.36.0 or later (this
# program installs 0.39.0). Note that FastVideo's own page says only the text-to-video mode was distilled (the first/last-frame and reference
# builds were not), while ComfyUI ships a first/last-frame template for it all the same: try the picture modes before relying on them.
# ComfyUI compiles small Triton helpers on first use (the 4-bit text encoder does, and a clip stops with a compile error without them): the
# computer needs gcc and the Python headers (on Ubuntu: build-essential and python3.12-dev).
#
# The licence is MiniMax's (the distillation inherits it) and it is narrow: the world EXCEPT the European Union, the United Kingdom, the
# Republic of Korea and the United States of America, for using the model AND for showing what it makes (V.4); written authorisation above
# 20 million US dollars of yearly revenue (IV.1); "MiniMax H3" shown by a commercial product (IV.2); what it makes not used to improve another
# model (V.3). MiniMax says organisations in the excluded regions can apply for a licence: https://platform.minimax.io/h3-license
# Read it: https://huggingface.co/MiniMaxAI/MiniMax-H3/blob/main/LICENSE
#
# What the computer must have: an NVIDIA card with 48 GB and 64 GB of memory, which nothing checks; and what `quickai-run` looks at before it
# fetches anything (the `system` needs at the end of this file; the document of ComfyUI says how each is checked): a driver of the 580 series
# or newer (ComfyUI is installed with PyTorch for CUDA 13), and a C compiler and the headers of Python, for the Triton helpers named above.
# After the install, and before the files are fetched, it builds and runs a Triton kernel once. Before ComfyUI is installed it also looks for
# the venv module of the Python its environment is made with (on Ubuntu the package python3.12-venv). Where something is missing it says
# what, with the command that brings it (on Ubuntu: apt-get install -y python3.12-venv build-essential python3.12-dev), and
# `--install-system-packages` lets it run that command. Sizes and checksums were read from the hub's listing on {date}; `quickai-run check` read the file as written. It has not
# been fetched or run.
"""
    write(out, "fasth3-v2.toml", header, "fasth3-v2", "FastH3 V2 (MiniMax H3, 8 steps) on ComfyUI",
          "FastH3 V2, the 8-step distillation of MiniMax H3 with block-sparse attention: words or first and last picture to a clip with sound, in a third of the time. About 41 GB of files. Licence: not for the EU, UK, South Korea or USA.",
          models, NEED_DIR.format(free=int(total / GB) + 25, files=int(total / GB) + 1) + SYSTEM_ALL)

    # ---- LTX-2.5 distilled -------------------------------------------------------------------------------------------------------------
    L = "Lightricks/LTX-2.5"
    models = [
        model(meta, "diffusion", "LTX-2.5 22B distilled, int8", L, "diffusion_models/ltx-2.5-22b-distilled-transformer-comfy-int8-convrot.safetensors", "diffusion_models"),
        model(meta, "text_encoder", "Gemma 4 12B with LTX-2.5 projections, int8", L, "text_encoders/gemma4-12b-with-proj-ltx-2.5-comfy-int8-convrot.safetensors", "text_encoders"),
        model(meta, "vae", "LTX-2.5 video VAE", L, "vae/ltx-2.5-video-vae-bf16.safetensors", "vae"),
        model(meta, "vae", "LTX-2.5 audio VAE", L, "vae/ltx-2.5-audio-vae-bf16.safetensors", "vae"),
    ]
    upscaler = meta.get(L, "latent_upscale_models/ltx-2.5-latent-spatial-upscaler-x2-bf16-1.0.safetensors")
    total = sum(s for _, s in models)
    enh = meta.get("Comfy-Org/gemma-4", "text_encoders/gemma4_e2b_it_int8_convrot.safetensors")
    header = f"""# A configuration for a server: ComfyUI with the files of LTX-2.5 (Lightricks, August 2026), the open video model that makes a clip with its
# sound in one pass: the 22B transformer in its distilled build (8 steps at half the size, then 3 at the full size), in int8 for ComfyUI.
# It carries no model. Each file says where it can be had from: a repository of the hub, the commit of it that was checked, the file in it,
# and how big it is. `quickai-run up` installs ComfyUI, fetches the four files below into the folders ComfyUI looks for each in, and starts it.
#
#     quickai-run init  ltx-2.5-distilled.toml                           # the file to fill in: where the models are kept, a login for the hub
#     quickai-run check ltx-2.5-distilled.toml --environment ltx.environment.toml --accept-review
#     quickai-run up    ltx-2.5-distilled.toml --environment ltx.environment.toml --accept-review
#
# The repository is GATED: before the first fetch, sign in to Hugging Face, open https://huggingface.co/Lightricks/LTX-2.5 and accept its terms
# with the account whose token you give (`--hf-token`, the variable HF_TOKEN, or `huggingface_token` under [fetch] of the environment file).
# The hub does not show a gated file's SHA-256 to someone who is not signed in, so the files below carry their size and not their checksum;
# `hubmeta.py --token ...` (tools/) prints them once you have a token, and they can be added here.
#
# The files, {gb(total)} ({total / GIB:.1f} GiB, which is how the program counts): the distilled transformer {gb(models[0][1])}, the text encoder (Gemma 4 12B
# with LTX's projections, int8) {gb(models[1][1])}, the video VAE {gb(models[2][1])} and the audio VAE {gb(models[3][1])}.
#
# A FIFTH FILE THIS PROGRAM CANNOT FETCH YET: the spatial latent upscaler ({gb(upscaler["size"])}, ltx-2.5-latent-spatial-upscaler-x2-bf16-1.0.safetensors), which
# the text-to-video and image-to-video workflows need for their second stage (first/last frame does not use it). ComfyUI keeps it in the
# folder `latent_upscale_models`, and the ComfyUI document of this program does not list that folder among the ones a configuration may fill
# (`places` under [models.acquire] of engines/comfyui.toml: one word to add). Until it does, put the file there by hand, for example
#     mkdir -p MODELS_DIR/latent_upscale_models && curl -L -H "Authorization: Bearer $HF_TOKEN" \\
#       https://huggingface.co/Lightricks/LTX-2.5/resolve/{upscaler["revision"]}/latent_upscale_models/ltx-2.5-latent-spatial-upscaler-x2-bf16-1.0.safetensors \\
#       -o MODELS_DIR/latent_upscale_models/ltx-2.5-latent-spatial-upscaler-x2-bf16-1.0.safetensors      # {upscaler["size"]} bytes
# and, once the folder is listed, add it here as a [[model]] with role = "upscaler", into = "latent_upscale_models" and
# path = "latent_upscale_models/<file>" from huggingface:Lightricks/LTX-2.5 at revision = "{upscaler["revision"]}", size = {upscaler["size"]}.
#
# The official templates also load a small second Gemma (gemma4_e2b_it_int8_convrot.safetensors, {gb(enh["size"])}, Comfy-Org/gemma-4, Apache-2.0) that
# rewrites a short prompt into a long one; the workflows of this package switch that off and do not need it. To have it anyway, add (a
# configuration holds at most 8 models):
#   [[model]] role = "text_encoder" file = "gemma4_e2b_it_int8_convrot.safetensors" from = "huggingface:Comfy-Org/gemma-4"
#   path = "text_encoders/gemma4_e2b_it_int8_convrot.safetensors" revision = "{enh["revision"]}" into = "text_encoders"
#   size = {enh["size"]} sha256 = "{enh["sha256"]}"
#
# What it does: words, a first picture, or a first and a last picture to a clip of a few seconds (Lightricks says up to 20; the workflows of
# this package are for 2 to 10) at 24 frames a second or more, with speech, sound effects and ambience made in the same pass; its other
# pipelines (audio to video, dubbing, retake, control by IC-LoRA, training of a LoRA) and its multi-shot mode are in Lightricks' own
# repository and are not in these workflows. Lightricks' own advice for 2D animation is to keep clips short (2 to 4 seconds) so the style
# does not drift towards photographs.
#
# The licence is the LTX-2.x Community License: free for commercial and production use below 10 million US dollars of yearly revenue of
# the whole entity; above it a paid agreement; no territory is excluded. Read it before you use the files.
#
# What the computer must have: an NVIDIA card of 24 GB at least (32 GB is what a hardware guide advises for the int8 files with the text
# encoder; 48 GB is easy) and 32 GB of memory, which nothing checks; and what `quickai-run` looks at before it fetches anything (the `system`
# needs at the end of this file; the document of ComfyUI says how each is checked): a driver of the 580 series or newer (PyTorch for CUDA 13),
# and a C compiler and the headers of Python, because the int8 files run through Triton, which builds a small helper in C the first time a
# kernel is launched. After the install, and before the files are fetched, it builds and runs a Triton kernel once. Before ComfyUI is
# installed it also looks for the venv module of the Python its environment is made with (on Ubuntu the package python3.12-venv). Where
# something is missing it says what, with the command that brings it (on Ubuntu: apt-get install -y python3.12-venv build-essential
# python3.12-dev), and `--install-system-packages` lets it run that command. Sizes were read from the hub's listing on {date}; `quickai-run check` read the
# file as written. It has not been fetched or run.
"""
    needs = '''[[needs]]
token = "huggingface"
why = "Lightricks/LTX-2.5 is a gated repository: accept its terms on huggingface.co with the account the token belongs to."

''' + NEED_DIR.format(free=int(total / GB) + 25, files=int(total / GB) + 1) + SYSTEM_ALL
    write(out, "ltx-2.5-distilled.toml", header, "ltx-2-5-distilled", "LTX-2.5 distilled on ComfyUI",
          "LTX-2.5 22B distilled in int8, with Gemma 4 12B (the x2 spatial upscaler, 1 GB, is put in by hand for now): words, a first picture or a first and a last picture to a clip of a few seconds with sound. About 40 GB of files. A gated repository: a Hugging Face token is needed.",
          models, needs)

    # ---- Wan 2.2 14B image to video ----------------------------------------------------------------------------------------------------
    W22 = "Comfy-Org/Wan_2.2_ComfyUI_Repackaged"
    W21 = "Comfy-Org/Wan_2.1_ComfyUI_repackaged"
    models = [
        model(meta, "diffusion", "Wan 2.2 14B image to video, high noise, fp8", W22, "split_files/diffusion_models/wan2.2_i2v_high_noise_14B_fp8_scaled.safetensors", "diffusion_models"),
        model(meta, "diffusion", "Wan 2.2 14B image to video, low noise, fp8", W22, "split_files/diffusion_models/wan2.2_i2v_low_noise_14B_fp8_scaled.safetensors", "diffusion_models"),
        model(meta, "text_encoder", "UMT5-XXL encoder, fp8", W21, "split_files/text_encoders/umt5_xxl_fp8_e4m3fn_scaled.safetensors", "text_encoders"),
        model(meta, "vae", "Wan 2.1 VAE (the 14B models use it)", W22, "split_files/vae/wan_2.1_vae.safetensors", "vae"),
        model(meta, "lora", "LightX2V 4-step LoRA, image to video, high noise", W22, "split_files/loras/wan2.2_i2v_lightx2v_4steps_lora_v1_high_noise.safetensors", "loras"),
        model(meta, "lora", "LightX2V 4-step LoRA, image to video, low noise", W22, "split_files/loras/wan2.2_i2v_lightx2v_4steps_lora_v1_low_noise.safetensors", "loras"),
    ]
    total = sum(s for _, s in models)
    header = f"""# A configuration for a server: ComfyUI with the files of Wan 2.2 14B for image to video, as ComfyUI's own templates of the model load them:
# the two experts (the one that draws the first half of the denoising, the one that finishes it) in fp8, UMT5-XXL as the text encoder, the Wan
# 2.1 VAE, and the 4-step LightX2V LoRA for each expert, which the templates switch on to make a clip in 4 steps instead of 20. It carries no
# model. Each file says where it can be had from: a repository of the hub, the commit of it that was checked, the file in it, how big it is
# and what its SHA-256 is. `quickai-run up` installs ComfyUI, fetches the six files into the folders ComfyUI looks for each in, and starts it.
#
#     quickai-run init  wan22-14b-i2v-lightning.toml                     # the file to fill in: where the models are kept
#     quickai-run check wan22-14b-i2v-lightning.toml --environment wan.environment.toml --accept-review
#     quickai-run up    wan22-14b-i2v-lightning.toml --environment wan.environment.toml --accept-review
#
# {gb(total)} of files ({total / GIB:.1f} GiB, which is how the program counts). The same six files serve image to video and first-and-last-frame
# to video; text to video needs the other pair of experts (wan2.2_t2v_*), which this file leaves out (a configuration holds at most 8 models).
#
# What it does and does not do: a clip from a picture (or from a first and a last picture) of up to about 5 seconds at 16 frames a second
# (81 frames; the model is trained for that), at 480p or 720p, with NO sound. ComfyUI's own note on the Lightning LoRAs: "Using the Wan2.2
# Lighting LoRA will result in the loss of video dynamics, but it will reduce the generation time." Its own table (an RTX 4090D of 24 GB,
# 640x640, 81 frames): about 513 s for 20 steps and about 71 s with the LoRAs once the models are loaded (97 to 108 s the first time).
# The workflows of this package come in both forms (wan22-*-lightning and wan22-*-full).
#
# The licence is Apache-2.0 (the model, the repackaging and the LoRAs): no territory, no revenue limit.
#
# What the computer must have: an NVIDIA card with 24 GB (fp8; 48 GB is comfortable at 720p) and 64 GB of memory is comfortable, which nothing
# checks; and what `quickai-run` looks at before it fetches anything (the `system` needs at the end of this file): the venv module of the
# Python its environment is made with (on Ubuntu the package python3.12-venv) and a driver of the 580 series or newer (ComfyUI is installed
# with PyTorch for CUDA 13). It does not ask for a compiler or the headers of Python, because nothing in
# these workflows is known to launch a kernel of Triton's (the fp8 files are run by PyTorch; this was not measured on a card). The other
# configurations of this folder ask for them (`c_compiler`, `python_headers`, `triton`), and so would this one if it used a custom node that
# does. Sizes and checksums were read from the hub's listing on {date}; `quickai-run check` read the file as written. It has not been
# fetched or run.
"""
    write(out, "wan22-14b-i2v-lightning.toml", header, "wan22-14b-i2v", "Wan 2.2 14B image to video on ComfyUI",
          "Wan 2.2 14B image to video and first-and-last-frame video in fp8, with the 4-step LightX2V LoRAs: clips of up to 5 seconds at 16 frames a second, no sound. About 38 GB of files. Apache-2.0.",
          models, NEED_DIR.format(free=int(total / GB) + 25, files=int(total / GB) + 1) + SYSTEM_DRIVER)
    print("wrote", sorted(p.name for p in out.glob("*.toml")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
