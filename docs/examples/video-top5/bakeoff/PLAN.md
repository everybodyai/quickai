# The bake-off: which way of making a clip is best for Bampu's cartoons

Written 2026-10-13. Not started: you said to wait for your word. Everything below is a plan; nothing has been run on a GPU.

## What it is meant to settle

No published test covers 2D cartoons for any of these models, and the one independent ranking (Artificial Analysis, human votes) is not about
cartoons. So the choice between H3 (base, Turbo, FastH3), LTX-2.5 and Wan 2.2 is made on **your** shots. It answers, in this order:

1. Does each model keep the drawn look (bold outlines, flat colours) and the character as drawn, over a 6 to 9 second shot?
2. What does a clip cost in time and in memory on the L40S (the numbers in the findings are scaled from other cards)?
3. How good is the speech, and does one character keep one voice from shot to shot? (53% of the shots of the sample series have none; 46% have
   exactly one line of about 8 words; almost none have two speakers.)
4. Which prompt style does H3 like better: MiniMax's six fields or the plain "director" style of ComfyUI's own template?
5. How much does Turbo / FastH3 give up against 20 steps, on the shots that matter and on the ones that do not?

## Before it can start

| Needed | Why | How |
|---|---|---|
| `build-essential`, `python3.12-dev` and `python3.12-venv` on the server | Triton cannot compile its helper without `Python.h`, and the first Bampu job died of it; ComfyUI's environment cannot be made without the venv module | `quickai-run up profiles/<file>.toml --install-system-packages ...` checks them and installs them (or prints the command); by hand: `apt-get install -y python3.12-venv build-essential python3.12-dev` (the server was not changed) |
| More disk | 32 GiB free; FastH3 +20.6 GiB fits, LTX-2.5 +36 GiB and Wan +35 GiB do not | grow the block storage (the provider of the rented server lets you resize it at any time) or fetch the options one at a time and delete the last |
| The models of each option | the runs | `quickai-run up profiles/<file>.toml ...` (README.md); LTX-2.5 needs a Hugging Face token and its upscaler by hand |
| Pictures in `bakeoff/pics/` | the references and the first frames | below |
| Voice clips in `bakeoff/voices/` | H3's voice reference | below |
| The tunnel | `ssh -N -L 8189:127.0.0.1:8188 you@server` | `--server` of the commands defaults to http://127.0.0.1:8189 |

### Pictures (`bakeoff/pics/`, PNG, 16:9 shots want 16:9-ish sheets; the model crops or scales)

* A sheet for each character and a plate for each place, in the style of the series: `shambu.png`, `jeeva.png`, `square.png`, `waterhole.png`,
  `trail.png`, `cave.png`. Bampu draws them with Gemini from the cards of the series (about $0.07 each by its documents); the series' own database holds
  only the demo AI's placeholders.
* A **key frame** for each shot that starts from a picture: `key_s2.png` ... `key_s6.png`: the opening still of the shot (the cast placed in the place,
  in the pose the shot starts from). Making the key frame is itself one of the things the test shows: try (a) Gemini composing it from the sheet and
  the plate, (b) the sheet of the first character as it is. A shot starting from a sheet that is not a scene looks like a sheet.

### Voice clips (`bakeoff/voices/`, WAV, 2 to 15 seconds, one speaker, quiet room)

`shambu.wav`, `jeeva.wav` (the timbre runs) and `line_s3.wav`, `line_s4.wav` (the track runs: the line of the shot, spoken by a text-to-speech voice of
the character, the whole spoken part of the shot in one file; the model animates the mouth to it and uses it as it is). The timbre clips: the voice
described by the card (`a nervous, squeaky voice ...`, `a loud, booming, boastful voice`), saying a sentence
**other than the line of the shot** (MiniMax: when only the timbre is referenced, the reference's own words must not be carried over), about the length
of the line to be spoken (a forum reports that a reference trimmed to the length of the line works better). Where they come from is part of the test:
a text-to-speech voice made once and kept (Qwen3-TTS VoiceDesign invents a voice from the description; Chatterbox clones one from a short recording),
or a person. A second set is worth making to see how much the reference matters: a different voice for the same character.

## The runs

`python3 tools/make_commands.py --server http://127.0.0.1:8189 > bakeoff/commands.sh` prints 32 runs, grouped by model so ComfyUI does not swap 20 GB
files between runs (`--only s3`, `--only fasth3` ... keeps some). The first run of a workflow loads its models and takes minutes more: **time the
second run of the same workflow** (the commands print a comment before each group). One job at a time; the card is the limit.

| Shot | What it tests | Lanes |
|---|---|---|
| s1 establishing, 8 s, no cast, a caption | scenery in the style; ambience and a flute from words alone | H3 base, Turbo, FastH3, LTX, Wan (from the plate) |
| s2 Jeeva creeping, 8 s, no line | slapstick motion of one character from his sheet; sound effects | H3 ref (six fields, director), Turbo, FastH3, LTX, Wan lightning and full |
| s3 Shambu dazed, 6 s, one line of 5 words | identity, one voice, the short line | H3 ref+voice (six fields, director), H3 ref+track (six fields, director), H3 from a key frame with its own voice, Turbo, FastH3, LTX, Wan |
| s4 Jeeva arrives, 9 s, one line of 17 words | the long line, 9 seconds | H3 ref+voice, H3 ref+track, Turbo, LTX |
| s5 two-shot, 6 s, two speakers | two voices in one clip | H3 ref+voice (six fields, director), FastH3, LTX |
| s6 = s3 vertical (9:16) | the shape of a short | H3 ref+voice, FastH3, LTX |

Wan makes at most about 5 seconds at 16 frames a second and has no sound: its runs are 5 seconds, silent, and are judged for the picture only.
LTX-2.5 and Wan runs need the first frame; the H3 reference runs need none.

Runs that stop with an error are results too (`bakeoff/runs/<label>.json` keeps the message). A run that says `REFUSED` never started: read the
message; a node is missing or a file is not in its folder.

## What to measure

`tools/run_workflow.py` records, for each run: the seconds ComfyUI took, the seconds from sending to the saved file, the most graphics memory it held
(polled once a second, so read it as "at least"), the card and the version. `python3 tools/collect_runs.py bakeoff/runs --csv > bakeoff/results.csv`
puts them in a table with empty score columns. Watch the clips with the sound on, **and the last second** (ComfyUI's own page lists morphing near the end of
a clip, and garbled text, as a known problem of the int8 builds).

| Score (1 to 5, you) | 1 | 3 | 5 |
|---|---|---|---|
| style | photographic or 3D, outlines gone | the look drifts or wobbles | bold outlines and flat colours all through |
| identity | another character | recognisable, details change | the character of the sheet, throughout |
| motion | frozen, or melting | usable | readable slapstick, the right timing |
| speech | not words | most words | every word clear |
| voice match | another voice | similar | the reference's voice |
| lip sync | no relation | roughly | in time |
| artefacts | broken | some | none (say which: end morph, text, grid, flicker, a replay of the reference) |

Also: **usable as it is?** (yes / yes with a retake / no). If you want a number for the speech, run Whisper on the audio and compare with the line
(word error rate); for the voice, compare the clips of one character with a speaker-verification model. Both are cheap on a CPU.

## How to read the result (suggestions, not rules)

* If LTX-2.5 scores within half a point of H3 base on style and identity in at least four of the six shots and is three or more times quicker, make LTX
  the production lane and keep H3 for the shots it alone does (the voice reference, several characters) if its licence allows.
* If H3's voice reference scores 3.5 or more on voice match in s3 to s6 without a replay of the reference, use it. If it replays the reference or
  garbles the start, try the line as the audio itself (Audio "exactly as it is", MiniMax's dialogue copying) from a text-to-speech voice, then
  LTX-2.5's audio-to-video with the same track, then voice conversion of the model's own speech.
* If Turbo (8 steps) speech is clearly worse than 20 steps on s3 and s4, use it only for shots without a line; same question for FastH3.
* If the six-field prompt beats the director style for H3, make it Bampu's dialect for H3 (the shot card maps onto it field for field); if not, keep the
  plain style the kit has.
* Wan 2.2 stays the fallback with no territory and no revenue limit; judge it on s2 and s3 only.

## Cost of the test

About 32 runs at an estimated 1 to 7 minutes: roughly 2 to 3 hours of the server, plus the first loads of five model sets. At about 1.1 euro an hour
(unconfirmed) that is a few euros, and the server is on anyway while it is rented by the hour. Stop the server afterwards (a stopped GPU server is not
billed; its disk and address are). Clear `input/run-*` and `input/bampu-*` in ComfyUI's input folder when you are done.
