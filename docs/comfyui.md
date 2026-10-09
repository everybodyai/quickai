# ComfyUI

ComfyUI makes pictures and video from workflows that a person builds in its own web page. QuickAI manages it as a third
engine, the way it manages Ollama: it installs it, starts and stops it, attaches to one that is running, keeps its
models, says what its API is and what went wrong. The engine is a document ([`comfyui.toml`](examples/engines/comfyui.toml)),
and what the document cannot say is a primitive (`install.python_app@1`, below). The facts it rests on are F1 to F27 of
[engine-facts.md](engine-facts.md) (written without a leading zero, as the other engines' are); the decisions are C78 to C86
of [implementation-plan.md](implementation-plan.md); the tests are groups F, P and Q of [phase3-test-plan.md](phase3-test-plan.md)
(F01 to F91, P01 to P19, Q01 to Q15).

ComfyUI is GPL-3.0 and a program of its own. It is **not part of any package**: it is downloaded when the person asks.

## What is different from the other engines

| | llama.cpp, Ollama | ComfyUI |
|---|---|---|
| What starts | a program that is given a model | a server that is given none (`needs_model = false`): a workflow that a client sends names the files it uses |
| A build | an archive of a release, unpacked | the **source** of a release, with a Python environment of its own made beside it |
| Models | one file, or a file and its projector | files of several kinds, each kept in the folder ComfyUI looks for that kind in (`diffusion_models/`, `text_encoders/`, `vae/`, `loras/`) |
| The window | a picker and a model box | the list of what is installed, **Recommended**, **Look up online**, a box for a link or a file and the folder it goes in, Refresh, Open folder, and **Open ComfyUI** once it runs |
| API | `/v1`, Ollama's | `POST /prompt`, `GET /history/<id>`, `GET /view`, `/ws`, `GET /system_stats` (`comfyui.workflow`) |
| Its page | none, or the server's own | a web page of its own, which QuickAI gives a **Download** that installs into the models folder ([below](#the-pages-download-button)) |

## Installing it

**Settings → Advanced → Look up ComfyUI builds** lists the releases of `Comfy-Org/ComfyUI`: the tested one first
(its source archive has a pinned sha256; any other is installed with the log saying that it was not checked). One click
installs it. The install is one primitive, `install.python_app@1`, and everything it does is in the staging directory
`~/.quickai/comfyui/<tag>.partial/`:

| Step | What | Test |
|---|---|---|
| python | the first of 3.13, 3.12, 3.14, 3.11, 3.10 the machine has (the `PATH`, pyenv, uv), or the one named in the **Python** box; nothing is installed to find it | F20 |
| source | the release's source archive, checked against its pin, unpacked as `app/` | F21, F22 |
| nodes | the pack of GGUF nodes (`ComfyUI-GGUF`, one commit, pinned by the digest of its archive) unpacked into `app/custom_nodes/` | F21, F22 |
| venv | `<python> -m venv venv/` | F21 |
| pip | one PyTorch for this machine's graphics card (ROCm 7.2 for AMD on Linux, CUDA 13 for NVIDIA, the processor's otherwise), then what ComfyUI and the pack need; always `--only-binary=:all:`, so no package runs a build script on the machine, and the temporary files are inside the staging directory (a machine's `/tmp` can be memory: this one's is) | F05, F21 |
| launcher | `comfyui`, a script that runs `app/main.py` with `venv/`'s Python | F21 |
| check | the launcher with `--quick-test-for-ci --cpu`, which loads every node and exits | F21, F26 |
| rename | `<tag>.partial` → `<tag>`, so a build is there whole or not at all | F21 |

It takes **about twenty minutes and 17 GB**: on the machine this was developed on, 19 min 51 s from the click to the
rename, 17,364,523,756 bytes in 65,609 files, nearly all of it PyTorch (the wheel alone is 6.2 GB, fetched at about
7 MB/s). The document asks for 30 GiB free before it starts, which holds the environment and the wheel while it is unpacked.

The window says where it is while it works. Under the bar: `Step 4 of 7 · PyTorch for AMD GPUs (ROCm 7.2) · downloading
torch-2.14.1+rocm7.2-cp314-cp314-manylinux_2_28_x86_64.whl: 117.5 MB of 5.8 GB, 7.0 MB/s, about 13 min 49 s left`; while
pip unpacks, which takes minutes in which it prints nothing, how long it has been; and after 45 seconds of silence
that pip is still running. The log holds pip's own words. **Cancel** stops pip, or the check, and everything
they started within seconds and throws the work away.

## If an install is interrupted

The window it was started from can be closed, be killed, or stop answering, the machine can be switched off, a step
can fail. Twenty minutes of work must not be lost to any of those, and a person must not have to do anything to a
directory by hand to go on (on 2026-10-08 one did: see *If the window stops answering*).

- **A journal.** The staging directory keeps `.quickai-install.json`: the steps of the install, which are done, and a
  fingerprint of what they were made for (the release, the source's address, the Python, every package and pack). A
  step is written down when it ends, whole or not at all.
- **What survives.** An install that **fails after its environment was made** (pip says no, the network goes, the check
  does not pass) and one that is **killed** keep their directory. One that is **cancelled by the person**, or fails
  before the environment (a checksum that does not match, no Python, no room), leaves nothing.
- **Not a build.** A directory with a journal is not a build, whatever it is called: `Installed builds` does not list it
  and the engine's own search for its program skips it. A person who renames `v0.39.0.partial` to `v0.39.0` because it
  looks whole has made a directory that is still not one, and is told so: **Unfinished installs**.
- **Continue.** When the application starts with ComfyUI chosen, the log says, once, that an install stopped (`ComfyUI: an
  install of v0.39.0 stopped before it was whole (3 of 7 steps done, ...)`). Settings → Advanced lists them
  (`v0.39.0 (3 of 7 steps done, 4.8 MB, stopped 12 min ago)`), each with **Continue** and a trash can. Continue, or the same
  install asked for again, does the steps that are left and none that are done (the log says which were: `Already done by the install that stopped: ...`; `Going on with the install that
  stopped (3 of 7 steps are done)`), and **always** the check, since it is the check that says the whole works. A step
  whose files are gone is not done, and nor is any after it. If the plan is not the same (another Python, other
  packages, a newer application) the work is not believed and the install begins again, saying so. The directory is
  taken up under any name; the room it asks for is what is still to be written.
- **A record that cannot be written** (a full disk) is said, stops the install and costs nothing: what was done stays where it
  was, and the next install goes on from it.
- **What cannot be saved.** A wheel of 6 GB that was half fetched when the install stopped: pip cannot resume a
  download, so that step begins again. A step that was finished is never fetched again.
- **A lock.** While an install works in a directory it holds a lock on `.quickai-install.lock` (an advisory lock on
  the file, which the system lets go when the process ends, however it ends). Another window of QuickAI neither clears
  that directory, nor continues it, nor lists it as stopped; it is told that another window is installing it.
- **Where the record ends.** After the rename the journal is removed from the build: a build that is whole has none, and
  one that has is one that stopped between the rename and its removal, which Continue finishes at once.

## If the window stops answering

A window that is drawn by one thread stops answering when that thread waits for something that does not come. What the
application does so that it need not:

- the **disk is never read in a frame**: the list of builds and of installs that stopped (a build of ComfyUI is 65,000
  files, and adding up their sizes took 140 ms in every frame the section was drawn in, before this) is read by a thread of its
  own, one scan at a time, and again only when an install or a delete ends or the section was away;
- a **delete runs on a thread of its own** (seventy thousand files), and locks the engine meanwhile;
- what an install or a download says many times a second asks for a repaint **soon**, not at once;
- the install and every wait are on threads that the window only reads the events of.

When it happens anyway, a thread that looks at the window once a second **while something is running** (an install, a
download, a delete, a look-up) writes `~/.quickai/ui-stalls.log` if no frame has been drawn for five seconds: when, for
how long, what the window's thread was waiting in (its state, the kernel function it sleeps in, the system call),
how the memory and the swap were, how much of the last ten seconds anything waited for memory or for the disk. When the window
answers again the file says when, and the log pane says that it did not answer. Nothing leaves the machine. A window
with nothing running draws nothing and is not looked at, and nor is one that is covered or minimized: the toolkit
does not paint a window nobody can see, which is no stall.

A record has this form (the numbers are an example, not a measurement):

```
2026-10-08 01:06:12 UTC · the window has not answered for 5 s while an install was running
  its thread: S, sleeping; waiting in the kernel at do_epoll_wait; system call: ppoll (271)
  memory: 21.3 GB available, 0.1 GB of 8.0 GB swap free; this program holds 221.4 MB and has 0 B in swap
  the machine in the last 10 s: something waited for memory 34.20% of the time, for the disk 61.00%
2026-10-08 01:06:40 UTC · the window answers again after 33 s
```

**What happened on 2026-10-08.** A person installed ComfyUI from the window; it stopped answering, and they killed it after
looking at the files and renaming a directory; ComfyUI was there when they started the window again. What the files say
about the install: the staging directory was created at 01:04:07, the launcher written at 01:23:48.07, the check ran until
01:23:58, the temporary directory was removed at 01:23:58.601, and the directory's change time is that same instant.
Renaming a directory changes its change time (checked), so **the application itself made the rename into place, twenty
minutes after the install began, and nothing in the files shows a rename by hand** (one made before would have broken
the steps that came after it, which wrote under the old name, and one made later would have a later change time). The
settings file has no record of the install ending, which the window writes the moment it hears of it, so the window never
took the news: its thread was blocked, not slow, and by the person's account from early on. What blocked it was **not
established**: the journal of the system holds no out-of-memory kill; a frame of the settings window drawn in a test
costs under a millisecond; the one thing in a frame that scaled with the size of a build (adding up the files of every
installed build, 140 ms for this one) cannot take twenty minutes, and did not run for the staging directory (a name that
ends in `.partial` was skipped). The machine had 20 GB in a memory-backed `/tmp`, a full swap and 17 GB being written, which
can starve a window; the display server or the graphics driver not answering are others (a window that is covered is not
painted at all, so that is not it). The recorder
is there to say which, the next time. Nothing in the install depended on the window answering, and nothing is lost to a
kill.

## Models

The files of a model are kept in the folders of the models folder (`~/.quickai/comfyui/models/` unless the person chose
another) that ComfyUI looks for each kind in; `[components.<kind>] place` says which, and the record beside the weights
(`<weights>.bundle.json`) says where the others are, so that deleting the model deletes them all and keeps what another
model uses. A file other than the weights is kept as `<owner>--<repo>--<name>`, as a projector is (C63): a workflow
names the file **as it is on disk**.

**Recommended → LTX-Video 2B distilled** (Lightricks, 2B, free below $10M a year in revenue) offers the weights alone or
with their text encoder and VAE, each checked against its pinned hash while it arrives:

| Kind | File on disk | Folder | Bytes |
|---|---|---|---|
| weights | `ltxv-2b-0.9.8-distilled-q6_k.gguf` (Q6_K, the default) or `...-q8_0.gguf` | `diffusion_models/` | 1,718,809,088 or 2,173,891,072 |
| text encoder | `biali--ltxv-gguf--t5xxl_fp16-q4_0.gguf` | `text_encoders/` | 2,896,123,072 |
| VAE | `biali--ltxv-gguf--ltxv_0.9.5_vae_fp16.safetensors` | `vae/` | 2,493,868,876 |

The VAE is **not** `ltxv_vae_fp32-f16.gguf`, which a guide for this model names: that one is not the VAE of this model, and
the 0.9.8 model decodes with the 0.9.5 VAE (fact F12). [`examples/workflows/ltx-video-2b-t2v.json`](examples/workflows/ltx-video-2b-t2v.json)
is a workflow for it in the API's format (send it to `POST /prompt`), naming the files of the default offer; with Q8_0
chosen, pick it in the first box. A test holds its names to what the button stores (F64).

A 4-second clip at 512×288 took 64 to 92 s on a 12 GB RX 6700 XT, with the **tiled** VAE decode that the example uses,
in tiles small enough for the Q8_0 weights too (fact F13: at 256 they ran out of memory in the decode). On an AMD card that PyTorch has no kernels for, **Present the GPU as** (`HSA_OVERRIDE_GFX_VERSION`) is worked
out from what the driver says (`gfx1031` as `10.3.0`) and can be set by hand (fact F9, test F09).

A different model, **Wan 2.2 5B Turbo** (`hum-ma/Wan2.2-TI2V-5B-Turbo-GGUF`, Apache-2.0: a 5B video model, distilled to 4 steps), is fetched with **Look up online** in three
buttons, because its text encoder and its VAE are other people's repositories: the model (`Wan2_2-TI2V-5B-Turbo-Q8_0.gguf`, 5.4 GB, into `diffusion_models/`),
the text encoder (`umt5-xxl-encoder-Q8_0.gguf`, 6.0 GB, `city96/umt5-xxl-encoder-gguf`, into `text_encoders/`) and the VAE (`Wan2.2_VAE.safetensors`, 1.4 GB, in the folder `VAE` of
`QuantStack/Wan2.2-TI2V-5B-GGUF`, into `vae/`). [`examples/workflows/wan22-ti2v-5b-turbo-t2v.json`](examples/workflows/wan22-ti2v-5b-turbo-t2v.json) is a workflow for it in the
API's format (send it to `POST /prompt`, as the LTX one): the model card's own settings (4 steps, a guidance of 1, Euler, the simple scheduler), 640×352 and 33 frames; with the
**tiled** VAE decode in tiles of 192 and 16 frames it made a clip on the 12 GB RX 6700 XT in 183 s (90 s on a fresh ComfyUI with the files cached), and at 256 and 32 the decode ran out of memory
twice and the run took 453 s (fact F27). The model's preferred size is 1280×704: that was not run, and a card of 12 GB may need a smaller one. A test holds the workflow's names to what the buttons store
(Q01d). It is not in **Recommended** because the list holds a model's files from one repository, and these are three.

## Getting models

There are three ways to get a model from the window, as there are in llama.cpp's, and a fourth from the page ([below](#the-pages-download-button)).
They differ in what they know: **Recommended** knows a model and every file it needs; **Look up online** finds files on
the hub and puts each where it belongs; the box takes a file the person already has an address for. Whichever it was, a
file is kept in the folder of the models folder that ComfyUI looks for that kind in, and a workflow names it by its file
name. QuickAI changes nothing in ComfyUI for them: it puts a file in a folder, and **Refresh** in the window (or the `r` key in the page) makes ComfyUI read the
folders again.

```
Models: 3 installed   [Recommended] [Look up online] [Refresh] [Open folder]
Get: [ link to a file, or search by name...       ] [...]  Into: [auto v]  [Search]
Results (3)   Size: Any  Small  Medium  Large
v Video (1)
    QuantStack/Wan2.2-TI2V-5B-GGUF         text-to-video · 224.2K downloads · Quantized from Wan-AI/Wan2.2-TI2V-5B · apache-2.0
    Get model:           [Q4_K_M · 3.2 GB] [Q5_K_M · 3.5 GB] [Q6_K · 3.9 GB] [Q8_0 · 5.0 GB]
    Get VAE only:        [Wan2.2_VAE · 1.3 GB]
v Text encoders (1)
    city96/umt5-xxl-encoder-gguf           270.9K downloads · Quantized from google/umt5-xxl · apache-2.0
    Get text encoder only:   [Q4_K_M · 3.4 GB] [Q5_K_M · 3.9 GB] [Q6_K · 4.3 GB] [Q8_0 · 5.6 GB]
```

- **Look up online.** With nothing typed it makes the plain list: the most downloaded GGUF repositories of each pipeline the
  document names (`text-to-video`, `image-to-video`, `text-to-image`, `image-to-image`) and of the word `encoder`, because a
  text encoder carries no pipeline tag (F24): five searches and a request for each of the thirty repositories they find, which is
  what llama.cpp's makes (L14 of [engine-facts.md](engine-facts.md): 500 requests in five minutes are allowed). Words typed in the box are one search, as in
  llama.cpp's window: the phrase, then each of its words alone. The most downloaded GGUF repositories of any kind are language
  models, which ComfyUI does not run, and are not what it lists; one that a search finds anyway is under **Other**, folded.
- **What a result offers.** The quantizations of a repository's weights as buttons, the four the preferences like (closest to
  the original first: `Q8_0`, `Q6_K`, `Q5_K_M`, `Q4_K_M`), smallest first, the preferred in bold, each with its size. Beside
  them, **a file of a kind that goes by itself gets a row of its own**: *Get VAE only* for a VAE, *Get text encoder only* for a
  text encoder, each file the repository holds of that kind (the document says how a kind is found: the parts of its
  `[[models.bundle]]`, and which kinds are offered by themselves: `alone`). A repository that holds only text encoders
  (`city96/umt5-xxl-encoder-gguf`) has no model row and is a section of its own, **Text encoders**. A repository that holds
  the model, a text encoder and a VAE (the one the recommended list has) also has the row that fetches the three as one
  download. Which repository holds what is the repository's: three people's repositories are three results.
- **What is there already.** A result whose files are all in their folders, at the size the hub says, is marked **Installed** and
  its button does nothing (a file that is there is never fetched again anyway). The folders are read when a look-up starts, when a
  download ends and on **Refresh**, so a file put there by hand or fetched by the page counts. llama.cpp's results are not marked.
- **Where a file goes.** The weights, in `diffusion_models/` (a picture model as well as a video model: the node that reads
  GGUF reads it from there, F26); a text encoder in `text_encoders/`; a VAE in `vae/`. A file that a button fetches **by
  itself keeps the name it has** (`Wan2.2_VAE.safetensors`, `umt5-xxl-encoder-Q8_0.gguf`), because a workflow that someone
  shared names it so; the files of a bundle keep the names they have always had (`<owner>--<repo>--<name>`, C63), so two
  repositories' `vae.safetensors` do not meet. Every button says where in its hover (*It is kept in the vae folder*), and the
  window says it again when the file is in (*Fetched umt5-xxl-encoder-Q8_0.gguf (in text_encoders). ComfyUI finds it after
  Refresh*).
- **Checked.** The hub states a SHA-256 for every large file (`lfs.sha256` of the repository's answer; it is the `x-linked-etag`
  of the file's own, F25), the look-up carries it, and the download hashes the bytes as they arrive: a file that is not what
  the hub says it is (a wrong size or hash) is deleted and not kept, not even as a part, and the window says so in red. A file
  that is already there at its size is not fetched again. This holds for llama.cpp's window too.
- **The box.** One box, three things it can mean: the address of **one `.gguf` or `.safetensors` file** (the page of the file on
  Hugging Face is turned into the file's address; any other `https` address must end in one of those) is downloaded; a **file of
  this computer** is linked into the folder (a symbolic link, so no second copy; a copy where links cannot be made); anything
  else is words to search for, and the *page of a repository* (`huggingface.co/city96/umt5-xxl-encoder-gguf`) is the name of
  that repository, searched. **Into** chooses the folder (`diffusion_models`, `text_encoders`, `vae`, `loras`, `checkpoints`,
  `controlnet`, `clip_vision`, `upscale_models`, `embeddings`: the weights', the kinds' and the others the document names,
  `places`); *auto* chooses from the file's name the way a result does (a VAE or a text encoder by what the document's parts
  say, else with the weights), and the window says which before the button is pressed. A link to a file that may hold a
  program (`.ckpt`, `.pt`, `.pth`, `.bin`) or to anything that is not a model is refused with a sentence, and nothing is asked
  of anyone: those are the person's to put in the folder (**Open folder**). The folder the person chose is believed over
  the guess; it does not move what a result's button does, which is already the folder it belongs in.
- **What it does not do.** Search the hub for anything but GGUF repositories (the models of `Comfy-Org` and the like are
  safetensors: they are what a workflow's own *Download* is for, below, or a link in the box); recognize a LoRA by its name
  (choose **Into** → `loras`); resume a download that stopped (the file starts again; the page's Download does go on from its
  part); tell whether a model fits the graphics card (the recommended window greys what is more than the memory; a look-up
  shows the sizes). A model fetched this way has **not been run by QuickAI** unless this page says so (Wan 2.2 5B Turbo, above, was): the
  recommended list holds what a clip was made from on a real card, and nothing here claims that of a repository the hub lists.
- **What it contacts.** `huggingface.co` for a look-up (about thirty-five requests for the plain list) and, for a file, `huggingface.co`
  and the download server it sends the file to (`*.cdn.hf.co`), only when a button is pressed ([PRIVACY.md](../PRIVACY.md)). The
  window itself makes these requests, not ComfyUI, so they are not in the engine's `egress`.
- **How it was held.** The pure half on the answers that the hub gave for a video model, its text encoder and its VAE as three
  people published them (`tests/fixtures/huggingface/comfyui/`, recorded 2026-10-08; Q01-Q07), the window against a fake hub
  that really serves the files (a look-up, each button, a wrong checksum, the box with a link, a path and words; Q08-Q11), the
  searches of the primitive (Q12) and a download of one file (Q13), and by hand against the real hub (Q14: the three
  files of a person's list are offered with the sizes and checksums the recordings say). The files were fetched for that
  person by a script that makes the same checks (size and SHA-256 against the hub's; the window was not built yet) and the
  headers were read by the build's own Python: the Turbo model is architecture `wan`, 30 blocks, 825 tensors, which the pinned pack
  of GGUF nodes lists as one it reads (F26). The real window was run on a virtual display with a throw-away home against the real
  hub: **Look up online** listed 28 models, a name typed in the box and Enter listed 43, and a button put a 219 MB text encoder in
  `text_encoders/` with the hub's SHA-256 (the implementation plan's log has the details).

## The page's Download button

A workflow names the models it needs. When the machine lacks one, ComfyUI's page lists it, in the **Issues** of the
workflow's panel under **Missing Models**, with a **Download** button. In a browser that button is a link that the browser
saves in **its own folder**: on 2026-10-08 a person who pressed it on the Z-Image-Turbo template found `ae.safetensors`
(335 MB) in `~/Downloads` and not where ComfyUI looks for it, and asked whether a setting or an environment variable
makes it install. None does (fact F17): the page has three ways to answer the button, and the one for a browser is the
link; what it has beside it is a place for a host program, which ComfyUI Desktop fills (F18). QuickAI is that host (C85).

- **What QuickAI puts there.** At every start of ComfyUI, before it starts, QuickAI writes two things beside the models folder,
  not in the build (a build can still be deleted or replaced and nothing is lost, and one made before this was has it as a new
  one does): `~/.quickai/comfyui/extensions/quickai_bridge/`, a folder of custom nodes that has no node, and
  `extra_model_paths.yaml`, which ComfyUI is told to read (`--extra-model-paths-config`) and which makes `extensions/` a
  place it loads custom nodes from (F19). What is the same is left alone, what differs is written again (the log says
  `Put … (1 of 3 files written)`), and what the person put there is not touched. This is `[[launch.spawn.write]]` of the document, which
  names files of the application's own (`crates/quickai/assets/comfyui/`, compiled in; architecture §11.7).
- **What the bridge is.** Python and a script, with nothing but the standard library and what ComfyUI loads. Three routes that
  only a program on this machine may use (`POST /quickai/downloads {url, name, directory}`, `GET /quickai/downloads`,
  `DELETE /quickai/downloads/<id>`) and a script of the page that gives it `window.__comfyDesktop2.downloadModel`. The page's
  own cross-site protection answers first (a request from another site is refused by ComfyUI itself).
- **What a press does.** ComfyUI itself fetches the file, one at a time, into the folder of the kind (`vae/`, `text_encoders/`,
  `diffusion_models/`; the folder with the kind's own name, F21), under the name the workflow gives it, as `<name>.part`,
  which is renamed when the file is whole and, when the hub says what its checksum is (F20), when it hashes to that. A file
  that is not what the hub describes is not kept. A fetch that was stopped (the connection, a kill, a power cut) is gone on from
  at the next press, from where its `.part` ends, after the part has been read again for the checksum; a hub that sends the
  whole file again is met, and so is a part that is not the beginning of the file (it begins again once, and says so). A fetch
  that is stopped from the panel leaves nothing. A file that is there is never fetched over (the panel says it is there), and
  there has to be room for it and half a gigabyte more.
- **What it will not fetch.** An address that is not `huggingface.co`'s, over https; a file that is not `.safetensors` (or `.sft`) or
  `.gguf` (the formats that hold numbers and cannot hold a program; a pickled `.ckpt`, `.pt` or `.pth` is the person's to bring in); a name that is
  not a plain file name (a folder that climbs, a hidden file); a folder that is not one of the models folder's; a request
  from another computer. **What it will not fetch is left to the browser**, as it always was, and the panel says so in a
  sentence; a file that wants a login at the hub is one the browser, which has the login, may be able to save.
- **What the person sees.** A panel at the bottom left of the page, *Models, fetched by ComfyUI*, shows each file with how much
  of it is in, how fast and how long it will take, with × to stop it. The fetch belongs to ComfyUI and not to the tab, so a page
  that is opened or reloaded in the middle of one shows it. When a file is in place the page reads its lists again, as its `r` key
  does, so the model appears in the nodes and goes from the list of what is missing. In QuickAI's window **Refresh** finds
  a weights file that arrived this way, and **Open folder** shows the folders.
- **The setting.** Settings → Advanced → **The page's Download button**: `install` (the default) or `browser`. With `browser` the
  bridge has no route and no script, the page has no host, and ComfyUI contacts no site: the button is the browser's, as in
  a ComfyUI started by hand. It is read at the start. A ComfyUI that was already running when QuickAI attached to it has no bridge
  (QuickAI changes nothing about one it did not start).
- **What it contacts.** Nothing by itself, and when a file is asked for, `huggingface.co` and the download server the hub sends the file to
  (`us.aws.cdn.hf.co` here, `*.cdn.hf.co` in the document): measured through a logging proxy, F22; the document declares them (`egress`), a test
  holds the declaration to the recording (X08), and [PRIVACY.md](../PRIVACY.md) says it. The page itself still may contact no
  other site (`--offline`): which is also why its list of missing models shows no sizes, since the page's look at the size of a
  file at the hub is refused by its own policy.
- **How it was held.** The downloads (P01 to P11) against a server that behaves like the hub, the glue (P12) with stand-ins for
  ComfyUI, the plan and the files (P13 to P18), and, by hand (P19, `crates/quickai/tests/comfy_bridge/real_page.py`), the real
  thing: the page of this build in Chromium, the Z-Image-Turbo template, **Download** on the VAE, ComfyUI killed in the middle
  and started again, and the same press going on and ending with the hub's size and sha256 (F23). Through the application
  itself too, once: started in a virtual display with a throw-away home, **Start** pressed, the files written by the supervisor, the
  same download ended in the models folder it manages.

## Not done

- A pickled file (`.ckpt`, `.pt`, `.pth`) and one from another site (Civitai) is left to the browser: the bridge fetches only what
  it can check. A token for a hub that wants a login is not used.
- The window of the application shows nothing of a fetch the page asked for (the page does): a list that shows it, from the `.part` files, is possible.
- A download the window starts (a result, the box) does not go on from where it stopped; the page's Download does.
- **A real ComfyUI in the GUI suite.** The GUI suite (`quickai_base/test`, [the test repository's docs/18_comfyui.md](../../test/docs/18_comfyui.md): 170 cases in nine suites and thirteen in four older ones) drives the window against fakes (a stand-in `main.py` that refuses what the real one refuses and hosts the bridge, a stand-in Python, a GitHub, a Hugging Face, the same over TLS); a real ComfyUI is installed by hand, as the lines above say.
- The installer was run on Linux only, with the AMD (ROCm) PyTorch; the CUDA, CPU and macOS steps follow from the same
  document and are tested by their conditions (F05), not by a machine.
- A model that needs more than ComfyUI's default nodes and the pack of GGUF nodes is not in the list, and its nodes are not
  installed (the pack is the only one). Wan 2.2 5B Turbo (a model, a text encoder and a VAE of three repositories) is fetched with **Look up
  online** and made a clip (F27), but is not in **Recommended**: the list's schema holds a model's companions from the model's own repository.
