#!/usr/bin/env python3
"""render_prompts: writes the prompt of each shot of bakeoff/shots.json in the ways the models are asked in.

    render_prompts.py SHOTS.json OUT_DIR

One file per shot and way, in OUT_DIR/<way>/<shot id>.txt:
  h3-sixfield   MiniMax H3, reference mode (pictures of the cast and the place, a voice clip for each speaker): the six fields of MiniMax's
                prompt guide for reference mode: subject_definitions, summary, retention_analysis, detailed_description, overall_soundscape,
                non_diegetic_music; characters are <Subject N> built from <Picture N>, a voice clip is <Audio N> bound to a speaker (S1).
  h3-sixfield-track, h3-director-track   the same two, for the workflows that take the line as a recorded track used as it is (ref2v-track): the
                words are in the track, so they are not in the prompt.
  h3-director   MiniMax H3, reference mode, in the plain style of ComfyUI's own template of it: "Use <Picture 1> ... as reference frames";
                a cut written as a line; the sound after it.
  h3-fl2va      MiniMax H3 (and FastH3), from a first picture: the three fields of the guide for the base build, with the template's own
                first line for a first picture; h3-t2va is the same from words alone (no first line).
  ltx           LTX-2.5: one flowing paragraph in the order things happen, the sound and the speech in it; "Use the provided start image as
                the first frame." when there is a first picture; ltx-t2v is the same from words alone.
  wan           Wan 2.2: one short paragraph about what is seen and how it moves; no sound (the model makes none).
These are drafts to be tried, written from the guides and the templates; none has been run on a model. Only the standard library is used.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path


def card(c: dict, name: str) -> str:
    parts = [name]
    if c.get("age_range"):
        parts.append(c["age_range"])
    hair = ", ".join(x for x in (f"{c['hair_color']} hair" if c.get("hair_color") else "", c.get("hair_style") or "") if x)
    if hair:
        parts.append(hair)
    if c.get("eye_color"):
        parts.append(f"{c['eye_color']} eyes")
    if c.get("marks"):
        parts.append(c["marks"])
    text = ", ".join(parts)
    if c.get("outfit"):
        text += ", wearing " + c["outfit"]
    return text


def tag_names(text: str, names: list[str], tags: dict[str, str]) -> str:
    """Writes <Subject N> where the text says a character by name (the whole name, or a word of it that is capitalised and long enough)."""
    import re
    for n in sorted(names, key=len, reverse=True):
        text = re.sub(r"\b" + re.escape(n) + r"\b", tags[n], text)
    for n in names:
        for w in sorted(n.split(), key=len, reverse=True):
            if len(w) >= 5 and w[0].isupper():
                text = re.sub(r"\b" + re.escape(w) + r"\b", tags[n], text)
    return text


def lowthe(place: str) -> str:
    return "the " + place[4:] if place.startswith("The ") else place


def an(word: str) -> str:
    return "an" if word[:1].lower() in "aeiou8" else "a"


def speakers(shot: dict) -> list[str]:
    out = []
    for l in shot["lines"]:
        if l["kind"] == "dialogue" and l["speaker"] and l["speaker"] not in out:
            out.append(l["speaker"])
    return out


def order(shot: dict) -> list[str]:
    """The cast of the shot, the speakers first."""
    sp = speakers(shot)
    return sp + [c for c in shot["cast"] if c not in sp]


def dialogue_lines(shot: dict, ids: dict[str, str]) -> list[tuple[str, dict]]:
    return [(ids.get(l["speaker"], ""), l) for l in shot["lines"] if l["kind"] == "dialogue"]


def six(data: dict, shot: dict) -> str:
    cast, place = order(shot), shot["place"]
    subj = {n: f"<Subject {i + 1}>" for i, n in enumerate(cast)}
    sid = {n: f"(S{i + 1})" for i, n in enumerate(speakers(shot))}
    pnum = len(cast) + 1
    seen = tag_names(shot["seen"], cast, subj)
    lines = ["subject_definitions:"]
    for i, n in enumerate(cast):
        lines.append(f"<Subject {i + 1}> is {card(data['cast'][n], n)}, shown in <Picture {i + 1}>.")
    lines.append(f"<Subject {pnum}> is the place, {lowthe(place)}: {data['places'][place]}, shown in <Picture {pnum}>.")
    for k, n in enumerate(speakers(shot)):
        lines.append(f"<Audio {k + 1}> is the voice-timbre reference for {subj[n]} {sid[n]}: {data['cast'][n].get('voice_style', 'the voice')}.")
    lines += ["", "summary:", f"[reference generation{' + audio reference' if speakers(shot) else ''}] {an(str(shot['seconds'])).capitalize()} {shot['seconds']}-second {shot['camera'].lower()} in {subj_place(pnum)}: {seen}.",
              "", "retention_analysis:"]
    for i, n in enumerate(cast):
        lines.append(f"<Subject {i + 1}> (in [Shot 1]): fully_preserved - face, costume and colours as drawn.")
    lines.append(f"<Subject {pnum}> (behind the action in [Shot 1]): partially_preserved - the layout and look of the place.")
    for k, n in enumerate(speakers(shot)):
        lines.append(f"<Audio {k + 1}>: reference - timbre and delivery only, its words are not used.")
    style_head, style_tail = data["series"]["style"].split(":", 1)
    lines += ["", "detailed_description:",
              f"{style_head}, 2D-animated: {style_tail.strip()}.",
              f"[Shot 1] {shot['camera']}, in {subj_place(pnum)}. {seen}."]
    for who, l in dialogue_lines(shot, sid):
        n = l["speaker"]
        lines[-1] += f" {subj[n]} {who} says {l['manner']}: <d>[English] {l['text']}</d>"
    lines += ["", "overall_soundscape:", shot["heard"] + ".", "", "non_diegetic_music:", "N/A"]
    return "\n".join(lines) + "\n"


def subj_place(pnum: int) -> str:
    return f"<Subject {pnum}>"


def director(data: dict, shot: dict) -> str:
    cast, place = order(shot), shot["place"]
    refs = [f"<Picture {i + 1}> ({n})" for i, n in enumerate(cast)] + [f"<Picture {len(cast) + 1}> (the place, {lowthe(place)})"]
    text = f"{data['series']['style']}. Use " + " and ".join(refs) + " as reference frames"
    sp = speakers(shot)
    if sp:
        text += "; " + "; ".join(f"<Audio {k + 1}> is the voice of {n}" for k, n in enumerate(sp))
    text += f".\nCUT 1: {shot['camera'].lower()}. {shot['seen']}."
    for l in shot["lines"]:
        if l["kind"] == "dialogue":
            k = sp.index(l["speaker"]) + 1
            text += f" {l['speaker']} says, {l['manner']}, in the voice of <Audio {k}>: \"{l['text']}\""
    text += f"\nSound: {shot['heard']}. No music, no on-screen text.\n"
    return text


def six_track(data: dict, shot: dict) -> str:
    """Reference mode with the line as a recorded track that is used as it is (the speech is in the track, not in the prompt)."""
    cast, place = order(shot), shot["place"]
    subj = {n: f"<Subject {i + 1}>" for i, n in enumerate(cast)}
    pnum = len(cast) + 1
    seen = tag_names(shot["seen"], cast, subj)
    sp = speakers(shot)
    lines = ["subject_definitions:"]
    for i, n in enumerate(cast):
        lines.append(f"<Subject {i + 1}> is {card(data['cast'][n], n)}, shown in <Picture {i + 1}>.")
    lines.append(f"<Subject {pnum}> is the place, {lowthe(place)}: {data['places'][place]}, shown in <Picture {pnum}>.")
    if sp:
        lines.append(f"<Audio 1> is the recorded line of {' and '.join(subj[n] for n in sp)}, to be used as it is.")
    lines += ["", "summary:", f"[reference generation{' + audio reuse' if sp else ''}] {an(str(shot['seconds'])).capitalize()} {shot['seconds']}-second {shot['camera'].lower()} in {subj_place(pnum)}: {seen}.",
              "", "retention_analysis:"]
    for i, n in enumerate(cast):
        lines.append(f"<Subject {i + 1}> (in [Shot 1]): fully_preserved - face, costume and colours as drawn.")
    lines.append(f"<Subject {pnum}> (behind the action in [Shot 1]): partially_preserved - the layout and look of the place.")
    if sp:
        lines.append("<Audio 1>: fully_copy - the recorded line, as it is.")
    style_head, style_tail = data["series"]["style"].split(":", 1)
    body = f"[Shot 1] {shot['camera']}, in {subj_place(pnum)}. {seen}."
    if sp:
        body += " " + " and ".join(subj[n] for n in sp) + (" speaks" if len(sp) == 1 else " speak") + " the words of <Audio 1> with the mouth in time with them."
    lines += ["", "detailed_description:", f"{style_head}, 2D-animated: {style_tail.strip()}.", body,
              "", "overall_soundscape:", shot["heard"] + ".", "", "non_diegetic_music:", "N/A"]
    return "\n".join(lines) + "\n"


def director_track(data: dict, shot: dict) -> str:
    cast, place = order(shot), shot["place"]
    refs = [f"<Picture {i + 1}> ({n})" for i, n in enumerate(cast)] + [f"<Picture {len(cast) + 1}> (the place, {lowthe(place)})"]
    sp = speakers(shot)
    text = f"{data['series']['style']}. Use " + " and ".join(refs) + " as reference frames" + (" and <Audio 1> exactly as it is" if sp else "") + "."
    text += f"\nCUT 1: {shot['camera'].lower()}. {shot['seen']}."
    if sp:
        text += " " + " and ".join(sp) + (" speaks" if len(sp) == 1 else " speak") + " the words of <Audio 1>, the mouth in time with them."
    text += f"\nSound: {shot['heard']}. No music, no on-screen text.\n"
    return text


def fl2va(data: dict, shot: dict, first_picture=True) -> str:
    sp = speakers(shot)
    sid = {n: f"(S{i + 1})" for i, n in enumerate(sp)}
    head = "For the target video, at 0.00 seconds into the target video, <Picture 1> (from [Shot 1]) is fully referenced.\n" if first_picture else ""
    body = f"[Shot 1] {data['series']['style'].split(':')[0]}, 2D-animated: {data['series']['style'].split(':', 1)[1].strip()}. {shot['camera']}. {shot['seen']}."
    for l in shot["lines"]:
        if l["kind"] == "dialogue":
            n = l["speaker"]
            c = data["cast"][n]
            body += f" {n}, {c['age_range']} with {c['voice_style']} {sid[n]}, says {l['manner']}: <d>[English] {l['text']}</d>"
    return (head + "integrated_multimodal_description: " + body + "\n"
            + "overall_soundscape: " + shot["heard"] + ".\n" + "non_diegetic_music: N/A\n")


def ltx(data: dict, shot: dict, first_picture=True) -> str:
    text = "Use the provided start image as the first frame. " if first_picture else ""
    text += f"{data['series']['style'].split(':')[0]}, 2D animation with {data['series']['style'].split(':', 1)[1].strip()}. {shot['camera']}: {shot['seen']}."
    for l in shot["lines"]:
        if l["kind"] == "dialogue":
            c = data["cast"][l["speaker"]]
            text += f" {l['speaker']} says {l['manner']}, in {c['voice_style']}: \"{l['text']}\""
    text += f" The sound: {shot['heard'].lower()}. No music, no subtitles."
    return text + "\n"


def wan(data: dict, shot: dict) -> str:
    return (f"{data['series']['style'].split(':')[0]}, 2D animation, {data['series']['style'].split(':', 1)[1].strip()}. "
            f"{shot['camera']}. {shot['seen']}. Smooth, readable cartoon motion, the background steady.\n")


def main() -> int:
    data = json.load(open(sys.argv[1]))
    out = Path(sys.argv[2])
    ways = (("h3-sixfield", six), ("h3-director", director), ("h3-sixfield-track", six_track), ("h3-director-track", director_track), ("h3-fl2va", lambda d, s: fl2va(d, s, True)), ("h3-t2va", lambda d, s: fl2va(d, s, False)),
            ("ltx", lambda d, s: ltx(d, s, True)), ("ltx-t2v", lambda d, s: ltx(d, s, False)), ("wan", wan))
    for way, fn in ways:
        (out / way).mkdir(parents=True, exist_ok=True)
        for s in data["shots"]:
            (out / way / f"{s['id']}.txt").write_text(fn(data, s))
    print("wrote", sum(1 for _ in out.glob("*/*.txt")), "prompts to", out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
