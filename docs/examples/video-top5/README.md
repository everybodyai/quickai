# video-top5: configurations and workflows for the five ways to make clips with Bampu

Prepared on 2026-10-13, for later use. The findings behind it (the models compared, their licences, speeds and voices) are in a separate
document, `video-models-for-bampu.md`, that is not part of this repository.

This copy sits in QuickAI's examples and differs from the original in three things. The folder of the configurations is called `profiles/`, because
a `.toml` below `docs/examples` must be a configuration in a folder named after its kind (`crates/quickai-core/tests/examples.rs`). The sample
environment file is a block under [Fetch the models on the server](#fetch-the-models-on-the-server-quickai), because it is not a configuration.
And `profiles/minimax-h3.toml` is a copy of [../profiles/minimax-h3.toml](../profiles/minimax-h3.toml): keep the two the same.

**Nothing here has been run on a GPU.** You asked for the tests to wait. What was checked is listed under
[What was checked, and what was not](#what-was-checked-and-what-was-not); the main thing not checked is how good any of it looks.

## What is here

```
profiles/     QuickAI configurations (quickai-run): where each model file comes from, with size, SHA-256 and commit
workflows/    ComfyUI workflows in the API format, one folder for each option, and slots.json (where a caller puts its values)
bakeoff/      the test plan (PLAN.md), six real shots of the series, 54 prompts written from them, the commands to run, and the places for
              the pictures, voice clips and results
tools/        what made these files and what runs them (below)
```

## The five options

| # | Option | Configuration | Workflows (`workflows/<folder>/`) | Files to fetch | Sound | Starts from | Steps | Licence |
|---|---|---|---|---|---|---|---|---|
| 1 | MiniMax H3, the base weights | `profiles/minimax-h3.toml` (a copy of `../profiles/minimax-h3.toml`) | `01-h3-base`: t2v, i2v, flf2v, ref2v, ref2v-voice, ref2v-track | 7 files, 65.0 GB | speech, effects, ambience | words; a first (and last) picture; up to 3 reference pictures (the files take 9); a voice clip as the timbre (2 places; the files take 3) or a recorded track used as it is | 20 (25 if references drift) | MiniMax H3 Community: not for the EU, UK, South Korea, USA |
| 2 | H3 with its Turbo LoRA | the same file | `02-h3-turbo`: the same five | the same | the same; speech is weakest at 8 steps | the same | 8; 4 for references | the same |
| 3 | FastH3 V2, FastVideo's 8-step distillation of H3 | `profiles/fasth3-v2.toml` | `03-fasth3-v2`: t2v, i2v, flf2v | 4 files, 41.2 GB (19.1 GB of it are files of option 1) | yes | words; a first (and last) picture; no references | 8, fixed | the same as H3 |
| 4 | LTX-2.5, distilled | `profiles/ltx-2.5-distilled.toml` | `04-ltx-2.5`: t2v, i2v, flf2v | 4 files, 38.7 GB, and the 1.0 GB upscaler **by hand** (see Known gaps) | yes | words; a first (and last) picture | 8, then 3 | LTX-2.x Community: free under 10 million US dollars of yearly revenue; a gated repository |
| 5 | Wan 2.2 14B, image to video | `profiles/wan22-14b-i2v-lightning.toml` | `05-wan22-14b`: i2v and flf2v, each as `lightning` (4 steps) and `full` (20) | 6 files, 38.0 GB | **none** | a first (and last) picture | 4 or 20 | Apache-2.0 |

Sizes are decimal gigabytes; the programs count 1024-based ("GiB"). The disk of the rented server had 32 GiB free after H3 was installed, so only
option 3 fits beside H3 without more disk.

## Using it

### Fetch the models on the server (QuickAI)

```sh
quickai-run init  profiles/fasth3-v2.toml                       # writes the file to fill in; its lines are shown below
quickai-run check profiles/fasth3-v2.toml --environment fasth3.environment.toml --accept-review      # the plan, nothing changed
quickai-run up    profiles/fasth3-v2.toml --environment fasth3.environment.toml --accept-review      # do it, and stay
```

The environment file (`fasth3.environment.toml` above) holds what belongs to this computer and not to a configuration. `quickai-run init` writes one
for a configuration, with the lines it needs, readable by its owner only (`chmod 600`); the lines this package's configurations may need are:

```toml
[values.comfyui]
models_dir = "/path/with/room/models"        # the biggest option here is about 45 GB of files; keep room for the clips too
# output_dir = "/path/with/room/output"
# extra_args = "--use-sage-attention"        # only once SageAttention is installed in ComfyUI's environment (it is not by default)

[fetch]
huggingface_token = ""                      # the LTX-2.5 files need one (a gated repository); HF_TOKEN in the environment does the same
```

Before the first job, the server needs **gcc and the Python headers** (on Ubuntu 24.04: `build-essential` and `python3.12-dev`), and to install ComfyUI at all
the venv module of its Python (`python3.12-venv`). ComfyUI compiles small Triton helpers on first use (H3's 4-bit text encoder does; the int8 and
sparse-attention kernels of the others very likely do); without the headers the job stops at once with a message that says only that gcc "returned
non-zero exit status 1" (the cause, `Python.h: No such file`, is in the server's log). This is what stopped the first Bampu job on the rented server:
the headers were missing there on 2026-10-13, and they stay missing until someone installs them.

**`quickai-run` checks all of that now** (WP4.20, [docs/headless.md](../../headless.md) section 12): each configuration here lists what the computer has to have
(`[[needs]] system = ...`: the venv module, the driver of the 580 series, and, for H3, FastH3 and LTX-2.5, a compiler, the headers of Python and a test that a
Triton kernel builds and runs). `check` says what is missing, with the exact command, before the plan and before anything is fetched; the same command
with `--install-system-packages` installs the packages (as the administrator, or through `sudo` without a password) and goes on; after ComfyUI is installed
and before the models are fetched it builds and runs a Triton kernel once. `--skip-prerequisites` leaves all of it out.

Each configuration starts ComfyUI on 127.0.0.1:8188 of the server; from your computer: `ssh -N -L 8189:127.0.0.1:8188 you@server`.
The LTX-2.5 files are in a gated repository: accept the terms on its page with the account whose token you give.

### Run one workflow, without Bampu

```sh
python3 tools/run_workflow.py --server http://127.0.0.1:8189 workflows/04-ltx-2.5/ltx25-i2v.json \
    --prompt-file bakeoff/prompts/ltx/s3.txt --first-image bakeoff/pics/key_s3.png --aspect 16:9 --seconds 6 --label try1 --out bakeoff/runs
```

It uploads the pictures and sounds, puts the values into the slots of `workflows/slots.json`, sends the graph, waits, fetches the clip, and writes
`bakeoff/runs/try1.json`: the seconds ComfyUI took, the seconds from sending to the file, the most graphics memory it held, the version of ComfyUI,
the card. `--dry-run` prints the graph that would be sent and stops. `python3 tools/run_workflow.py --help` says the rest.

### Put one into Bampu (not done)

`workflows/slots.json` says, for each workflow, the node and input that take the prompt, the seed, the width, the height, the frames, the pictures and
the voice clips, the sizes of each aspect, the rule for the number of frames and the frame rate. That is what a Bampu service needs
(`[capability.slots]`, `[capability.request]`); the H3 service files of the kit in `bampu_base/others/comfyui/h3` have the shape. The one thing that kit
cannot do and these workflows can is hand the model a **voice clip** (`h3-ref2v-voice`, `ref_audios`): Bampu would have to upload a sound the way it
uploads a picture (`POST /upload/image` takes any file) and name it in the slot.

### Test them (when you say so)

`bakeoff/PLAN.md` is the plan: six shots of your series, which workflow makes each, what to measure, how to score it. `python3 tools/make_commands.py`
prints the commands.

## The slots

Generated from `workflows/slots.json` (`node.input`; "frames rule": the frame count n the model allows):

| workflow | prompt | negative | first / last picture | reference pictures | voice clips | steps | frames rule | fps |
|---|---|---|---|---|---|---|---|---|
| `01-h3-base/h3-t2v` | `104.prompt` | - | - / - | - | - | 20 | n % 17 == 5 | 24 |
| `01-h3-base/h3-i2v` | `104.prompt` | - | `114.image` / - | - | - | 20 | n % 17 == 5 | 24 |
| `01-h3-base/h3-flf2v` | `104.prompt` | - | `114.image` / `115.image` | - | - | 20 | n % 17 == 5 | 24 |
| `01-h3-base/h3-ref2v` | `136.prompt` | - | - / - | 3 | - | 20 | n % 17 == 5 | 24 |
| `01-h3-base/h3-ref2v-voice` | `136.prompt` | - | - / - | 3 | 2 (timbre) | 20 | n % 17 == 5 | 24 |
| `01-h3-base/h3-ref2v-track` | `136.prompt` | - | - / - | 3 | 1 (track as it is) | 20 | n % 17 == 5 | 24 |
| `02-h3-turbo/h3-turbo-t2v` | `104.prompt` | - | - / - | - | - | 8 | n % 17 == 5 | 24 |
| `02-h3-turbo/h3-turbo-i2v` | `104.prompt` | - | `114.image` / - | - | - | 8 | n % 17 == 5 | 24 |
| `02-h3-turbo/h3-turbo-flf2v` | `104.prompt` | - | `114.image` / `115.image` | - | - | 8 | n % 17 == 5 | 24 |
| `02-h3-turbo/h3-turbo-ref2v` | `136.prompt` | - | - / - | 3 | - | 4 | n % 17 == 5 | 24 |
| `02-h3-turbo/h3-turbo-ref2v-voice` | `136.prompt` | - | - / - | 3 | 2 (timbre) | 4 | n % 17 == 5 | 24 |
| `02-h3-turbo/h3-turbo-ref2v-track` | `136.prompt` | - | - / - | 3 | 1 (track as it is) | 4 | n % 17 == 5 | 24 |
| `03-fasth3-v2/fasth3-t2v` | `104.prompt` | - | - / - | - | - | 8 | n % 17 == 5 | 24 |
| `03-fasth3-v2/fasth3-i2v` | `104.prompt` | - | `136.image` / - | - | - | 8 | n % 17 == 5 | 24 |
| `03-fasth3-v2/fasth3-flf2v` | `104.prompt` | - | `136.image` / `115.image` | - | - | 8 | n % 17 == 5 | 24 |
| `04-ltx-2.5/ltx25-t2v` | `364.text` | `373.text` | - / - | - | - | 8 + 3 | n % 8 == 1 | 24 |
| `04-ltx-2.5/ltx25-i2v` | `364.text` | `373.text` | `395.image` / - | - | - | 8 + 3 | n % 8 == 1 | 24 |
| `04-ltx-2.5/ltx25-flf2v` | `222.text` | `217.text` | `31.image` / `39.image` | - | - | 8 + 3 | n % 8 == 1 | 24 |
| `05-wan22-14b/wan22-i2v-lightning` | `93.text` | `89.text` | `97.image` / - | - | - | 4 | n % 4 == 1 | 16 |
| `05-wan22-14b/wan22-flf2v-lightning` | `90.text` | `78.text` | `80.image` / `89.image` | - | - | 4 | n % 4 == 1 | 16 |
| `05-wan22-14b/wan22-i2v-full` | `93.text` | `89.text` | `97.image` / - | - | - | 20 | n % 4 == 1 | 16 |
| `05-wan22-14b/wan22-flf2v-full` | `90.text` | `78.text` | `80.image` / `89.image` | - | - | 20 | n % 4 == 1 | 16 |

The sizes (`sizes` in slots.json): H3 and FastH3 1344x768, 768x1344, 768x768 (a short side of 768 is what H3 is made for); LTX-2.5 1280x704,
704x1280, 768x768 (its first stage runs at half the size, so width and height are multiples of 64); Wan 2.2 1280x720, 720x1280, 720x720 (832x480 and
480x832 are its lighter size). A caller may ask for another size with `--size`. LTX-2.5's width, height, frame rate and frames are `PrimitiveInt`
nodes (`value`) that the template's small expressions read; `slots.json` names them (`width`, `height`, `fps`, `frames`).

## What the workflows are

Each is ComfyUI's own template of the model (the package `comfyui-workflow-templates` 0.11.76, the one ComfyUI 0.39.0 pins), converted from the
editor's format to the API format (`tools/ui2api.py`) and then made fit for a caller (`tools/build_top5.py`; the notes of each function say what):
the Turbo or Lightning switch of the template is decided; the size, the number of frames and the prompt are plain inputs; what nothing reads is
dropped; the placeholders (`placeholder.png`, `voice1.wav`, the prompt) are what a caller overwrites. `python3 tools/check_package.py . --templates
DIR --object-info FILE` prints, for each workflow, the node classes it dropped and added against its template. In short:

* H3 and FastH3: the resolution selector, the seconds-to-frames expression and the Lightning switches are gone (numbers and a decided switch in their
  place); the first/last-frame and reference workflows have as many `LoadImage` (and `LoadAudio`) nodes as places.
* LTX-2.5: the prompt enhancer (a second 5 GB text model) is switched off and left out; the template's negative text, which bans "cartoon,
  childish", is replaced by a neutral one (it does nothing at CFG 1).
* Wan 2.2: the template's Lightning switch is decided for `i2v`; for `flf2v` the template carries the 20-step pipeline and, muted, the Lightning
  one, so the Lightning workflow is made from the 20-step one the way the muted group does it. Wan makes 16 frames a second.

## What was checked, and what was not

Checked, without a GPU (`tools/check_package.py`, 2026-10-13):

* all 195 slots of the 22 workflows name a node and an input that exist; every model a workflow loads is a file its configuration fetches, into the
  folder that loader reads (and, against the template, a file the template itself declares for that folder);
* ComfyUI 0.39.0 (the version the server runs, here on a CPU with empty stand-in model files, in a private folder, never the one anybody works with)
  accepts all 22 workflows: node classes, inputs, types, ranges, links. A converter bug found on the way (a subgraph input typed `IMAGE,MASK` shifted
  every value after it) was why the loaders were first wired to the wrong files; the audit against the template's own file list is what shows it;
* `quickai-run check` (the current build, without a network) reads the four configurations and prints the plan; a fresh run of the two builders
  writes byte-identical files; `run_workflow.py` uploaded a picture, sent a graph and reported the loader error of the empty files cleanly.

**Not checked:** a clip, its quality, its time, its memory; how the voice references, the first/last frame of FastH3, LTX-2.5's two stages at the sizes
chosen, or the 16-frames-a-second Wan clip behave; the SHA-256 of the five LTX-2.5 files (the hub hides it for a gated repository); that the Triton
helpers compile once the Python headers are installed on the rented server (the test that says so was run for real only on the development computer's AMD
card, with and without the headers of its Python); anything on the rented server (nothing was changed there).

## Known gaps

* **LTX-2.5's spatial upscaler cannot be fetched by `quickai-run` yet.** QuickAI's ComfyUI document lists the folders a configuration may fill
  (`places` under `[models.acquire]` in `quickai/crates/quickai-runtime/manifests/engines/comfyui.toml`) and `latent_upscale_models` is not among them;
  adding the word (and the same for `model_patches` and `audio_encoders`, which other templates use) is a change in QuickAI. Until then the file is put
  in by hand (the header of `profiles/ltx-2.5-distilled.toml` has the command). The text-to-video and image-to-video LTX workflows need it; first/last
  frame does not.
* The LTX-2.5 configuration has sizes but no checksums (gated repository). `python3 tools/hubmeta.py --token ... Lightricks/LTX-2.5:<path>` prints them
  once the terms are accepted.
* A sound for H3 (a voice clip or a track) has to be uploaded by whatever calls the workflow; Bampu does not do it yet. The two ways differ: in
  `ref2v-voice` the clip gives the *timbre* and the model speaks the words of the prompt (users report it replays the clip or starts with garble about
  half the time); in `ref2v-track` the clip is used *as it is* and the model animates the mouth to it (users report this works), which is the way a
  text-to-speech line would be fed in.
* The prompts in `bakeoff/prompts` are drafts written from MiniMax's guides, LTX's template and ComfyUI's templates; none has been tried.

## Rebuilding

```sh
python3 tools/build_top5.py --templates <comfyui_workflow_templates_json/templates> --object-info <GET /object_info of ComfyUI 0.39.0, saved> --out workflows
python3 tools/hubmeta.py <repo>:<path> ... > hubmeta.json && python3 tools/build_configs.py --meta hubmeta.json --out profiles
python3 tools/render_prompts.py bakeoff/shots.json bakeoff/prompts
python3 tools/check_package.py . --templates <the same templates> --object-info <the same file> [--server URL --base DIR]
```

`tools/validate_api.py` and `check_package.py --server` post the workflows to a ComfyUI; use only a throw-away one with no real models (they make empty
stand-in files in its model folders). Only the standard library of Python 3.11 or later is used everywhere.
