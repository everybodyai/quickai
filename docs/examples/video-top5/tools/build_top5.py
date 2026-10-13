#!/usr/bin/env python3
"""build_top5: writes the API-format ComfyUI workflows of five ways to make clips for a video series (MiniMax H3, H3 with its Turbo LoRA,
FastH3 V2, LTX-2.5 distilled, Wan 2.2 14B), from ComfyUI's own templates, and the table of where a caller puts its values (slots.json).

    build_top5.py --templates DIR --object-info FILE --out DIR

DIR (templates) is the folder of the package comfyui-workflow-templates that the ComfyUI to run them pins (0.11.76 for ComfyUI 0.39.0:
site-packages/comfyui_workflow_templates_json/templates). FILE is what GET /object_info of that ComfyUI answers. Each workflow is the template
converted to the API format (ui2api.py), then: the switches of the template (Turbo on or off) are decided, the values it computes from a
duration or a megapixel count are written as numbers, what nothing reads is dropped, and the placeholders a caller overwrites are named in
slots.json. Nothing else is changed except what the NOTES of a function say. Only the standard library of Python 3.11 or later is used.
"""
from __future__ import annotations

import argparse
import copy
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import wf_tools as T  # noqa: E402
from ui2api import convert  # noqa: E402

PROMPT = "A calm ocean at sunset."          # a placeholder: a caller writes its own
IMG1, IMG2, IMG3 = "placeholder.png", "placeholder2.png", "placeholder3.png"     # names a caller replaces with the name ComfyUI has its picture under
VOICE1, VOICE2 = "voice1.wav", "voice2.wav"
NEG_LTX = "worst quality, blurry, distorted, jpeg artifacts, watermark, subtitles"   # inert at CFG 1; the template's "cartoon, childish" is not wanted here


class Build:
    def __init__(self, templates: Path, object_info: dict):
        self.templates = templates
        self.oi = object_info
        self.cache: dict = {}

    def src(self, name: str) -> dict:
        if name not in self.cache:
            self.cache[name] = convert(json.load(open(self.templates / f"{name}.json")), self.oi)
        return copy.deepcopy(self.cache[name])


def finish(g: dict, roots, keep=()) -> dict:
    T.simplify(g, roots, keep)
    return T.sort_graph(T.strip_prefix(g))


def named(g: dict, nid: str, text: str) -> None:
    T.title(g, nid, text)


# ======================================================================================================================================
# MiniMax H3: the build for words and first/last frame (FL2VA) and the build for references (REF2VA)
# ======================================================================================================================================
def h3_fl2va(b: Build, mode: str, turbo: bool, w=1344, h=768, frames=124):
    """NOTES: the template's Lightning switch is decided (off: 20 steps, the base weights; on: the 8-step LoRA at strength 1); the size and the
    number of frames are numbers; t2v has no picture, i2v the first, flf2v the first and the last."""
    g = b.src("video_minimax_h3_i2v")
    prefix = [k for k in g if k.endswith(":104")][0].split(":")[0] + ":"
    model = prefix + "104"
    T.set_in(g, prefix + "126", "value", bool(turbo))
    T.set_in(g, model, "width", w)
    T.set_in(g, model, "height", h)
    T.set_in(g, model, "length", frames)
    T.set_in(g, model, "prompt", PROMPT)
    T.set_in(g, prefix + "15", "noise_seed", 1)
    first = T.find(g, "LoadImage")[0]
    T.set_in(g, first, "image", IMG1)
    out = T.find(g, "SaveVideo")[0]
    name = f"h3-{'turbo-' if turbo else ''}{mode}"
    T.set_in(g, out, "filename_prefix", f"bampu/{name}")
    if mode == "t2v":
        T.drop_input(g, model, "first_frame")
        del g[first]
    g = finish(g, [out])
    nid = T.find(g, "MiniMaxH3ImageToVideo")[0]
    last = None
    if mode == "flf2v":
        last = "115"
        T.add(g, last, "LoadImage", "last picture", image=IMG2)
        g[nid]["inputs"]["last_frame"] = [last, 0]
    g = T.sort_graph(g)
    slots = dict(prompt=[nid, "prompt"], seed=[T.find(g, "RandomNoise")[0], "noise_seed"], width=[nid, "width"], height=[nid, "height"],
                 frames=[nid, "length"], fps=[T.find(g, "CreateVideo")[0], "fps"], output=out)
    if mode != "t2v":
        slots["first_image"] = [first, "image"]
    if last:
        slots["last_image"] = [last, "image"]
    return name, g, slots


def h3_ref2va(b: Build, turbo: bool, voice=False, w=1344, h=768, frames=124, pictures=3, voices=2):
    """NOTES: the template has two picture places and the sound places left empty; this has `pictures` places for pictures (up to 9) and,
    with voice=True ("timbre"), `voices` places for a voice clip in `ref_audios` (the model takes the timbre and speaks the prompt's words),
    or, with voice="track", one place in `ref_video_audios` for a recorded track that is used as it is (the model lip-syncs to it: the way a
    text-to-speech line would be fed in). 3 sound clips of 2 to 15 seconds, 15 seconds in all, are what the model takes."""
    mode = "timbre" if voice is True else voice
    g = b.src("video_minimax_h3_r2v")
    T.set_in(g, "146", "value", bool(turbo))
    ref = "136"
    T.set_in(g, ref, "width", w)
    T.set_in(g, ref, "height", h)
    T.set_in(g, ref, "length", frames)
    T.set_in(g, ref, "prompt", PROMPT)
    T.set_in(g, "129", "noise_seed", 1)
    out = "92"
    suffix = {"timbre": "-voice", "track": "-track", False: ""}[mode]
    name = f"h3-{'turbo-' if turbo else ''}ref2v{suffix}"
    T.set_in(g, out, "filename_prefix", f"bampu/{name}")
    for k in [k for k in g[ref]["inputs"] if k.startswith(("ref_images.", "ref_audios.", "ref_video_audios."))]:
        del g[ref]["inputs"][k]
    for nid in ("137", "139"):
        del g[nid]
    g = finish(g, [out])
    pic_ids = ["137", "139", "150", "152", "153", "154", "155", "156", "157"][:pictures]
    for k, nid in enumerate(pic_ids):
        T.add(g, nid, "LoadImage", f"reference picture {k + 1}", image=[IMG1, IMG2, IMG3, IMG1, IMG2, IMG3, IMG1, IMG2, IMG3][k])
        g[ref]["inputs"][f"ref_images.ref_image_{k}"] = [nid, 0]
    aud_ids = []
    if mode == "timbre":
        aud_ids = ["151", "158", "159"][:voices]
        for k, nid in enumerate(aud_ids):
            T.add(g, nid, "LoadAudio", f"voice clip {k + 1} (timbre)", audio=[VOICE1, VOICE2, VOICE1][k])
            g[ref]["inputs"][f"ref_audios.ref_audio_{k}"] = [nid, 0]
    elif mode == "track":
        aud_ids = ["151"]
        T.add(g, "151", "LoadAudio", "recorded track (used as it is)", audio="track.wav")
        g[ref]["inputs"]["ref_video_audios.ref_video_audio_0"] = ["151", 0]
    g = T.sort_graph(g)
    slots = dict(prompt=[ref, "prompt"], seed=[T.find(g, "RandomNoise")[0], "noise_seed"], width=[ref, "width"], height=[ref, "height"],
                 frames=[ref, "length"], fps=[T.find(g, "CreateVideo")[0], "fps"], output=out,
                 ref_images=[[n, "image"] for n in pic_ids], ref_image_size=[ref, "ref_image_size"])
    if aud_ids:
        slots["ref_audios"] = [[n, "audio"] for n in aud_ids]
        slots["audio_mode"] = mode
    return name, g, slots


# ======================================================================================================================================
# FastH3 V2: the 8-step distillation of H3 (words, or first/last frame), with sparse attention
# ======================================================================================================================================
def fasth3(b: Build, mode: str, w=1344, h=768, frames=124):
    """NOTES: size and frames are numbers; t2v has no picture, i2v the first, flf2v the first and the last. The sigma shift (10, 3), the
    attention backend and the block-sparse attention (VSA, 10% of the blocks, from 20% of the schedule) are the template's."""
    g = b.src("video_fastvideo_fasth3_t2v" if mode == "t2v" else "video_fastvideo_fasth3_i2v")
    prefix = [k for k in g if k.endswith(":104")][0].split(":")[0] + ":"
    model = prefix + "104"
    T.set_in(g, model, "width", w)
    T.set_in(g, model, "height", h)
    T.set_in(g, model, "length", frames)
    T.set_in(g, model, "prompt", PROMPT)
    T.set_in(g, prefix + "15", "noise_seed", 1)
    out = T.find(g, "SaveVideo")[0]
    name = f"fasth3-{mode}"
    T.set_in(g, out, "filename_prefix", f"bampu/{name}")
    first = None
    if mode != "t2v":
        first = T.find(g, "LoadImage")[0]
        T.set_in(g, first, "image", IMG1)
    g = finish(g, [out])
    nid = T.find(g, "MiniMaxH3ImageToVideo")[0]
    last = None
    if mode == "flf2v":
        last = "115"
        T.add(g, last, "LoadImage", "last picture", image=IMG2)
        g[nid]["inputs"]["last_frame"] = [last, 0]
    g = T.sort_graph(g)
    slots = dict(prompt=[nid, "prompt"], seed=[T.find(g, "RandomNoise")[0], "noise_seed"], width=[nid, "width"], height=[nid, "height"],
                 frames=[nid, "length"], fps=[T.find(g, "CreateVideo")[0], "fps"], output=out)
    if first:
        slots["first_image"] = [first, "image"]
    if last:
        slots["last_image"] = [last, "image"]
    return name, g, slots


# ======================================================================================================================================
# Wan 2.2 14B image to video (high-noise and low-noise experts), with or without the 4-step LightX2V LoRAs
# ======================================================================================================================================
def wan_i2v(b: Build, lightning: bool, w=1280, h=720, frames=81):
    """NOTES: the template's switch is decided (on: LoRAs at strength 1, 4 steps, CFG 1, the experts split after step 2, shift 5; off:
    20 steps, CFG 3.5, split after step 10); size, frames (16 frames a second, 81 is 5 seconds) are numbers."""
    g = b.src("video_wan2_2_14B_i2v")
    prefix = [k for k in g if k.endswith(":98")][0].split(":")[0] + ":"
    T.set_in(g, prefix + "131", "value", bool(lightning))
    for lora in (prefix + "101", prefix + "102"):
        T.set_in(g, lora, "strength_model", 1.0)
    for ms in (prefix + "103", prefix + "104"):
        T.set_in(g, ms, "shift", 5.0)
    i2v = prefix + "98"
    T.set_in(g, i2v, "width", w)
    T.set_in(g, i2v, "height", h)
    T.set_in(g, i2v, "length", frames)
    T.set_in(g, prefix + "93", "text", PROMPT)
    T.set_in(g, prefix + "86", "noise_seed", 1)
    first = T.find(g, "LoadImage")[0]
    T.set_in(g, first, "image", IMG1)
    out = T.find(g, "SaveVideo")[0]
    name = f"wan22-i2v-{'lightning' if lightning else 'full'}"
    T.set_in(g, out, "filename_prefix", f"bampu/{name}")
    g = finish(g, [out])
    nid = T.find(g, "WanImageToVideo")[0]
    samplers = T.find(g, "KSamplerAdvanced")
    seeded = [s for s in samplers if g[s]["inputs"].get("add_noise") == "enable"][0]
    clips = T.find(g, "CLIPTextEncode")
    positive = [c for c in clips if g[c]["inputs"]["text"] == PROMPT][0]
    negative = [c for c in clips if c != positive][0]
    slots = dict(prompt=[positive, "text"], negative=[negative, "text"], seed=[seeded, "noise_seed"], width=[nid, "width"], height=[nid, "height"],
                 frames=[nid, "length"], fps=[T.find(g, "CreateVideo")[0], "fps"], first_image=[first, "image"], output=out)
    return name, g, slots


def wan_flf2v(b: Build, lightning: bool, w=1280, h=720, frames=81):
    """NOTES: the template carries the 20-step pipeline and, muted, the Lightning one; the Lightning one is made here from the 20-step one the
    way the muted group of the template does it (a LoRA on each expert, shift 5, 4 steps, CFG 1, the experts split after step 2)."""
    g = b.src("video_wan2_2_14B_flf2v")
    flf = T.find(g, "WanFirstLastFrameToVideo")[0]
    T.set_in(g, flf, "width", w)
    T.set_in(g, flf, "height", h)
    T.set_in(g, flf, "length", frames)
    out = T.find(g, "SaveVideo")[0]
    name = f"wan22-flf2v-{'lightning' if lightning else 'full'}"
    T.set_in(g, out, "filename_prefix", f"bampu/{name}")
    unets = T.find(g, "UNETLoader")
    high = [u for u in unets if "high_noise" in g[u]["inputs"]["unet_name"]][0]
    low = [u for u in unets if "low_noise" in g[u]["inputs"]["unet_name"]][0]
    samplers = T.find(g, "KSamplerAdvanced")
    first_s = [s for s in samplers if g[s]["inputs"]["add_noise"] == "enable"][0]
    second_s = [s for s in samplers if s != first_s][0]
    pos = [c for c in T.find(g, "CLIPTextEncode") if not g[c]["inputs"]["text"].startswith("色调")][0]
    neg = [c for c in T.find(g, "CLIPTextEncode") if c != pos][0]
    T.set_in(g, pos, "text", PROMPT)
    T.set_in(g, first_s, "noise_seed", 1)
    starts = [k for k in T.find(g, "LoadImage")]
    start_img = [k for k in starts if k == [x for x in starts if g[flf]["inputs"]["start_image"][0] == x][0]][0]
    end_img = [k for k in starts if k != start_img][0]
    T.set_in(g, start_img, "image", IMG1)
    T.set_in(g, end_img, "image", IMG2)
    if lightning:
        add_high, add_low = "91", "92"
        T.add(g, add_high, "LoraLoaderModelOnly", "Lightning LoRA (high noise)", model=[high, 0],
              lora_name="wan2.2_i2v_lightx2v_4steps_lora_v1_high_noise.safetensors", strength_model=1.0)
        T.add(g, add_low, "LoraLoaderModelOnly", "Lightning LoRA (low noise)", model=[low, 0],
              lora_name="wan2.2_i2v_lightx2v_4steps_lora_v1_low_noise.safetensors", strength_model=1.0)
        for ms in T.find(g, "ModelSamplingSD3"):
            g[ms]["inputs"]["shift"] = 5.0
            src = g[ms]["inputs"]["model"][0]
            g[ms]["inputs"]["model"] = [add_high if src == high else add_low, 0]
        for s, a, e in ((first_s, 0, 2), (second_s, 2, 10000)):
            g[s]["inputs"].update(steps=4, cfg=1.0, start_at_step=a, end_at_step=e)
    else:
        for ms in T.find(g, "ModelSamplingSD3"):
            g[ms]["inputs"]["shift"] = 8.0
    g = T.sort_graph(g)
    slots = dict(prompt=[pos, "text"], negative=[neg, "text"], seed=[first_s, "noise_seed"], width=[flf, "width"], height=[flf, "height"],
                 frames=[flf, "length"], fps=[T.find(g, "CreateVideo")[0], "fps"], first_image=[start_img, "image"], last_image=[end_img, "image"],
                 output=out)
    return name, g, slots


# ======================================================================================================================================
# LTX-2.5 distilled: two stages (half size, then twice that), 8 + 3 steps, audio carried in the same latent
# ======================================================================================================================================
# Where the numbers and the texts are in the templates of comfyui-workflow-templates 0.11.76 (the subgraph's instance id, then the ids inside
# it); every id is asserted to be the class expected, so a template that moved says so instead of being wired wrong.
LTX_IDS = {
    #        instance, width, height, fps, seconds, frames expr, text encoder (+), negative encoder, noise, images
    "t2v":   dict(inst="405:", width="372", height="360", fps="361", seconds="362", frames="378", pos="364", neg="373", noise="339", images=[]),
    "i2v":   dict(inst="398:", width="372", height="360", fps="361", seconds="362", frames="378", pos="364", neg="373", noise="339", images=["395"]),
    "flf2v": dict(inst="251:", width="215", height="216", fps="205", seconds="198", frames="226", pos="222", neg="217", noise="196", images=["31", "39"]),
}
LTX_CLASSES = dict(width="PrimitiveInt", height="PrimitiveInt", fps="PrimitiveInt", seconds="PrimitiveInt", frames="ComfyMathExpression",
                   pos="CLIPTextEncode", neg="CLIPTextEncode", noise="RandomNoise")


def ltx(b: Build, mode: str, w=1280, h=704, seconds=5, fps=24):
    """NOTES: the prompt enhancer (a second text model of 5 GB) is switched off and left out, the prompt goes straight to the text encoder;
    the template's negative text, which bans "cartoon, childish", is replaced (it does nothing at CFG 1); the size and the frame rate stay
    nodes (PrimitiveInt, named width, height and fps) that the template's small expressions read, so a caller changes the number and the
    rest follows; the number of frames is one node (`frames`, 8k+1) read by the video and by the audio latent. The first stage runs at half
    the size, so width and height are multiples of 64. In flf2v the template has one stage only, without the spatial upscaler."""
    ids = LTX_IDS[mode]
    g = b.src({"t2v": "video_ltx2_5_t2v", "i2v": "video_ltx2_5_i2v", "flf2v": "video_ltx2_5_flf2v"}[mode])
    p = ids["inst"]
    for key, cls in LTX_CLASSES.items():
        got = g[p + ids[key]]["class_type"]
        assert got == cls, f"{mode}: node {p + ids[key]} is {got}, expected {cls} for {key}: the template changed"
    assert w % 64 == 0 and h % 64 == 0, "LTX-2.5 runs its first stage at half the size: width and height are multiples of 64"
    assert (seconds * fps + 1) % 8 == 1
    frames = seconds * fps + 1
    out = T.find(g, "SaveVideo")[0]
    name = f"ltx25-{mode}"
    T.set_in(g, out, "filename_prefix", f"bampu/{name}")
    # no prompt enhancement: the template's flag is made false (it is false already) and the switch it drives folds to the plain text
    flags = [k for k in T.find(g, "PrimitiveBoolean") if any(g[c]["class_type"] == "ComfySwitchNode" for c, _, _ in T.consumers(g, k))]
    assert len(flags) == 1, flags
    T.set_in(g, flags[0], "value", False)
    # the numbers
    for key, value in (("width", w), ("height", h), ("fps", fps)):
        T.set_in(g, p + ids[key], "value", value)
        T.title(g, p + ids[key], key)
    frames_node = "1001"
    T.add(g, frames_node, "PrimitiveInt", "frames", value=frames)
    T.replace_refs(g, p + ids["frames"], 1, [frames_node, 0])
    T.replace_refs(g, p + ids["frames"], 0, float(frames))
    del g[p + ids["frames"]]
    del g[p + ids["seconds"]]
    # the negative text, the seed and the pictures
    T.set_in(g, p + ids["neg"], "text", NEG_LTX)
    T.set_in(g, p + ids["noise"], "noise_seed", 1)
    for k, img in enumerate(ids["images"]):
        T.set_in(g, img, "image", [IMG1, IMG2][k])
    prompt_nodes = [k for k in T.find(g, "PrimitiveStringMultiline")]
    assert len(prompt_nodes) == 1
    T.set_in(g, prompt_nodes[0], "value", PROMPT)
    keep = (p + ids["width"], p + ids["height"], p + ids["fps"], frames_node)
    T.simplify(g, [out], keep)
    g = T.sort_graph(T.strip_prefix(g))
    short = lambda i: i.split(":")[-1]   # noqa: E731
    slots = dict(prompt=[short(p + ids["pos"]), "text"], negative=[short(p + ids["neg"]), "text"], seed=[short(p + ids["noise"]), "noise_seed"],
                 width=[short(p + ids["width"]), "value"], height=[short(p + ids["height"]), "value"], fps=[short(p + ids["fps"]), "value"],
                 frames=[frames_node, "value"], output=out)
    if ids["images"]:
        slots["first_image"] = [ids["images"][0], "image"]
    if len(ids["images"]) > 1:
        slots["last_image"] = [ids["images"][1], "image"]
    return name, g, slots


# What a caller needs to know about the family a workflow is of: the sizes it may ask for (width, height), the rule for the number of
# frames (n % step == extra % step), the frame rate the clip is made at, the seconds the model is trained for, the steps the workflow runs.
H3_SIZES = {"16:9": [1344, 768], "9:16": [768, 1344], "1:1": [768, 768]}
FAMILY = {
    "01-h3-base":   dict(sizes=H3_SIZES, frames_rule={"step": 17, "extra": 5}, default_fps=24, seconds_range=[5, 15], steps=20),
    "02-h3-turbo":  dict(sizes=H3_SIZES, frames_rule={"step": 17, "extra": 5}, default_fps=24, seconds_range=[5, 15], steps=8),
    "03-fasth3-v2": dict(sizes=H3_SIZES, frames_rule={"step": 17, "extra": 5}, default_fps=24, seconds_range=[5, 15], steps=8),
    "04-ltx-2.5":   dict(sizes={"16:9": [1280, 704], "9:16": [704, 1280], "1:1": [768, 768]}, frames_rule={"step": 8, "extra": 1}, default_fps=24,
                         seconds_range=[2, 10], steps="8 + 3"),
    "05-wan22-14b": dict(sizes={"16:9": [1280, 720], "9:16": [720, 1280], "1:1": [720, 720]}, frames_rule={"step": 4, "extra": 1}, default_fps=16,
                         seconds_range=[2, 5], steps=20),
}


# ======================================================================================================================================
def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--templates", required=True)
    ap.add_argument("--object-info", required=True)
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    b = Build(Path(a.templates), json.load(open(a.object_info)))
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    plan = []   # (option dir, builder result)
    for turbo in (False, True):
        opt = "02-h3-turbo" if turbo else "01-h3-base"
        for mode in ("t2v", "i2v", "flf2v"):
            plan.append((opt, h3_fl2va(b, mode, turbo)))
        plan.append((opt, h3_ref2va(b, turbo, voice=False)))
        plan.append((opt, h3_ref2va(b, turbo, voice="timbre")))
        plan.append((opt, h3_ref2va(b, turbo, voice="track", voices=1)))
    for mode in ("t2v", "i2v", "flf2v"):
        plan.append(("03-fasth3-v2", fasth3(b, mode)))
    for mode in ("t2v", "i2v", "flf2v"):
        plan.append(("04-ltx-2.5", ltx(b, mode)))
    for lightning in (True, False):
        plan.append(("05-wan22-14b", wan_i2v(b, lightning)))
        plan.append(("05-wan22-14b", wan_flf2v(b, lightning)))
    slots = {}
    for opt, (name, g, s) in plan:
        d = out / opt
        d.mkdir(parents=True, exist_ok=True)
        (d / f"{name}.json").write_text(json.dumps(g, indent=1, ensure_ascii=False) + "\n")
        fam = copy.deepcopy(FAMILY[opt])
        if "ref2v" in name and opt == "02-h3-turbo":
            fam["steps"] = 4
        if name.startswith("wan22") and name.endswith("lightning"):
            fam["steps"] = 4
        slots[f"{opt}/{name}"] = {**s, **fam}
    (out / "slots.json").write_text(json.dumps(slots, indent=1, ensure_ascii=False) + "\n")
    print(f"wrote {len(plan)} workflows to {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
