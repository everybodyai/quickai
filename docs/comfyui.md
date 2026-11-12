# ComfyUI

ComfyUI generates images and video from workflows that you build in its own web page. QuickAI manages it as a third
engine, in the same way it manages Ollama: it installs it, starts and stops it, attaches to an instance that is already
running, manages its models, documents its API and reports what went wrong. The engine is described by a document
([`comfyui.toml`](examples/engines/comfyui.toml)), and whatever the document cannot express is a primitive
(`install.python_app@1`, below). The facts it relies on are F1 to F36 of [engine-facts.md](engine-facts.md) (written
without a leading zero, as for the other engines); the design decisions are C78 to C86 and C152 to C155 of
[implementation-plan.md](implementation-plan.md); and the tests are groups F, P and Q of
[phase3-test-plan.md](phase3-test-plan.md) (F01 to F99a, P01 to P21, Q01 to Q15).

ComfyUI is licensed under GPL-3.0 and is a separate program. It is **not included in any package**: it is downloaded
at your request.

## What is different from the other engines

| | llama.cpp, Ollama | ComfyUI |
|---|---|---|
| What starts | A program that is given a model | A server that is given none (`needs_model = false`): each workflow a client sends names the files it uses |
| A build | A release archive, unpacked | The **source** of a release, with its own Python environment created beside it |
| Models | One file, or a file and its projector | Files of several kinds, each stored in the folder where ComfyUI looks for that kind (`diffusion_models/`, `text_encoders/`, `vae/`, `loras/`) |
| The window | A picker and a model field | A page of its own ([below](#the-page)): before it is installed, the card that installs it; once it is, three steps of using it, three ways to get models (**ComfyUI-Manager** first, **Recommended**, and **Search or paste…** for Hugging Face with a field for a link or a file and its target folder), the files of its models folder, and **Open ComfyUI** once it is running |
| API | `/v1`, Ollama's | `POST /prompt`, `GET /history/<id>`, `GET /view`, `/ws`, `GET /system_stats` (`comfyui.workflow`) |
| Its page | None, or the server's own | A web page of its own, to which QuickAI adds a **Download** that installs into the models folder ([below](#the-pages-download-button)) and, with ComfyUI-Manager, a **Manager** button ([below](#comfyui-manager)) |
| Its files | Under `~/.quickai/<engine>` (Ollama's also in its own home and temporary folders there; llama.cpp writes nothing else) | Under `~/.quickai/comfyui`, **including its own home and temporary folders there** (`home/`, `cache/tmp/`): otherwise the ROCm compiler cache, the PyTorch and driver caches and the kernels it compiles would go to your `~/.cache` and the system's temporary folder (F28). The Python variables of your shell are also cleared (C92: every engine is completely isolated) |

## The page

ComfyUI's page in the window has two states (C222).

**Before a build is installed**, it is one card that installs it:

- **The version** it installs, the release the document was tested with (`ComfyUI v0.45.2 · tested with QuickAI`), the
  day it was published (`Released 11 November 2026`, from the document's `tested.date`: nothing is asked of GitHub to
  say it), where it comes from and how it is checked (built from its source at `github.com/Comfy-Org/ComfyUI` and checked
  against its sha256). **Other versions…** opens Settings → Program, where every release on offer is listed.
- **What it installs**: a box for each part that is yours to choose, **Install ComfyUI-Manager** (*highly recommended*,
  ticked; its summary says what it is for), then what comes with it whatever is ticked (the PyTorch for this computer's
  graphics card, what ComfyUI needs, ComfyUI-GGUF) and the Python the environment is made with (QuickAI's own, with
  **Use another Python…**).
- **The room and the button**: `Needs 30 GiB · 266 GB free`, where it is kept, and **Install ComfyUI v0.45.2**.

While it installs, the same card lists every step of the installation with a mark (done, under way, to come), the one
under way with what pip is doing, under one progress bar and **Cancel**.

**Once a build is installed**, the bar names its version beside ComfyUI (`ComfyUI v0.45.2 · Stopped`), and the page has:

- **Three steps**: Start, Open ComfyUI, Load a workflow, the one to do now in green.
- **Get models, three ways**, the first recommended: **ComfyUI-Manager** (about 500 models and every custom node, from
  inside ComfyUI's page; **Start and open the Manager** starts ComfyUI and opens its page with the Model Manager open,
  [below](#comfyui-manager)), **QuickAI's picks** (**Recommended**: the models QuickAI has verified, every file checked),
  and **Hugging Face** (**Search or paste…** opens a panel with the field for a name, a link or a file, its target
  folder, and **Look up online**, [below](#getting-models)). A build installed without the Manager says so in its tile.
- **On this computer**: every file of the models folder, by kind (`Diffusion model`, `Text encoder`, `VAE`, `LoRA`...)
  and size, the ones still being fetched first (`coming`), with **Refresh** and **Open folder**. While ComfyUI runs,
  the list is read again every few seconds, so what its page or its Manager fetches appears by itself.

In a narrow window the steps and the three ways are under each other rather than side by side.

## Installing it

**Install ComfyUI v0.45.2** on its page installs the tested release with what its card has ticked. **Settings → Program
→ Look up ComfyUI builds** lists the releases of `Comfy-Org/ComfyUI`, with the tested release first (its source archive
has a pinned sha256; any other release is installed with a log entry stating that it was not verified). The tested
release is **v0.45.2** (11 November 2026), a patch of v0.45.0 that ComfyUI tagged without publishing a release of its
own; it was installed, started and run on the date it was pinned (F37, F38). The installation is a single primitive, `install.python_app@1`, and all of its
work takes place in the staging directory `~/.quickai/comfyui/<tag>.partial/`:

| Step | What it does | Test |
|---|---|---|
| python | Uses QuickAI's own interpreter (C181): CPython 3.12.15 from python-build-standalone (with pip, venv and the headers), downloaded once into `python/` under ComfyUI's root and verified against the SHA-256 pinned in the document; or the interpreter named in the **Python** field (3.10 or newer). The computer's own Python is neither required nor modified | F20 |
| source | Verifies the release's source archive against its pin and unpacks it as `app/` | F21, F22 |
| nodes | Unpacks the GGUF node pack (`ComfyUI-GGUF`, a single commit, pinned by the digest of its archive) into `app/custom_nodes/` | F21, F22 |
| venv | Runs `<python> -m venv venv/` | F21 |
| pip | Installs one PyTorch build for this machine's graphics card (ROCm 7.2 for AMD on Linux, CUDA 13 for NVIDIA, the CPU build otherwise), then ComfyUI's dependencies, then, **when its option is selected, ComfyUI-Manager** (ComfyUI's own `manager_requirements.txt`: 13 packages, 27 MB), then the node pack's dependencies; always with `--only-binary=:all:`, so that no package runs a build script on the machine, and with temporary files inside the staging directory (a machine's `/tmp` may be held in memory) | F05, F21, F33 |
| launcher | Writes `comfyui`, a script that runs `app/main.py` with the Python from `venv/`, **adding the Manager's flag (`--enable-manager-legacy-ui`) when the build includes the Manager** | F21, F34 |
| check | Runs the launcher with `--quick-test-for-ci --cpu`, which loads every node and exits | F21, F26 |
| rename | Renames `<tag>.partial` → `<tag>`, so that a build is either complete or absent | F21 |

The installation takes **about twenty minutes and 17 GB**: on the development machine, 19 min 51 s from the click to
the rename, 17,364,523,756 bytes in 65,609 files, almost all of it PyTorch (the wheel alone is 6.2 GB, downloaded at
about 7 MB/s). The document requires 30 GiB of free space before it starts, enough for the environment and the wheel
while it is unpacked. The Manager adds a few seconds and 78 MB (F33).

The window reports progress while the installation runs. Under the toolbar: `Step 4 of 8 · PyTorch for AMD GPUs (ROCm 7.2) · downloading
torch-2.14.1+rocm7.2-cp314-cp314-manylinux_2_28_x86_64.whl: 117.5 MB of 5.8 GB, 7.0 MB/s, about 13 min 49 s left`;
while pip unpacks, which takes several minutes without any output, the elapsed time; and after 45 seconds of silence,
a note that pip is still running. The log contains pip's own output. **Cancel** stops pip, or the check, together with
everything they started, within seconds, and discards the work.

## ComfyUI-Manager

[ComfyUI-Manager](https://github.com/Comfy-Org/ComfyUI-Manager) is the add-on that installs custom nodes from within
ComfyUI's page, and whose **Model Manager** lists about 500 models (checkpoints, LoRAs, VAEs, upscalers, ControlNets)
and places the one you choose in the models folder. ComfyUI's own release refers to it (`manager_requirements.txt`, and
a section of its README explaining how to enable it), and the user who requested this option described it as highly
recommended. QuickAI offers it **when a build is installed**, as an option that is selected unless you clear it (C152).

- **The option.** On the install page itself (C222), and in Settings → Program, under **Look up ComfyUI builds**: **Install ComfyUI-Manager**,
  marked *highly recommended*, with a tooltip explaining what it is, what it contacts, and that a custom node is
  another party's program. The sentence below it describes what the installation will put on the machine, including
  ComfyUI-Manager when the option is selected. The state of the option when **Use** is clicked determines what that
  installation does (the option applies to the installation you are about to start, and is selected again each time
  QuickAI starts); it cannot be changed while an installation is running. **Continue**, on an interrupted installation,
  resumes with the choice that installation began with, not with the current option (completed work is trusted only if
  it was done for the same plan: F98, F99a). The option belongs to the window: the headless runner (`quickai-headless`)
  installs ComfyUI without it, as before.
- **What the installation does.** It adds one pip step after ComfyUI's dependencies
  (`pip install -r manager_requirements.txt` from the release, wheels only, from the package index: the version that
  release was built with), and the build's launcher passes `--enable-manager-legacy-ui` before the arguments it receives. A build is
  **started as it was built**: one built without the Manager is started without the flag, and a ComfyUI instance that
  was already running was not started by QuickAI and is not modified.
- **Why the classic Manager (C153).** ComfyUI offers two (F34). The new one has no Model Manager, and the list in its
  Nodes Manager is requested from `api.comfy.org` **by the page**, which **The page's network** setting (`--offline`,
  the default) blocks, so it would show grey placeholders indefinitely. The classic one has the Model Manager, and all
  of its work is done by ComfyUI itself, so the page's setting does not affect it. Neither choice changes the purpose
  of that setting: the page is designed to contact no other site.
- **The button (C154).** The classic Manager places its button in the page's legacy menu, which the page hides by
  default, so at every start QuickAI writes one more small folder beside the bridge
  (`~/.quickai/comfyui/extensions/quickai_manager`: no node, one script) that adds a **Manager** button at the top of
  the page, beside **Run**, when ComfyUI reports that the Manager is present. It opens the Manager's own menu:
  **Custom Nodes Manager**, **Install Missing Custom Nodes**, **Model Manager**, **Install via Git URL**, Snapshot
  Manager... Without the Manager, the script adds nothing and reports nothing.
- **Opened from the window (C222).** **Start and open the Manager** (**Open the Manager** while ComfyUI runs), the first of
  the page's ways to get models, opens ComfyUI's page at `/?quickai=manager`: the same script reads that word, opens the
  Manager's menu and in it the **Model Manager**, and takes the word out of the address, so that a reload does not open
  them again. Started from the button, ComfyUI's page opens once it answers. Whether a build has the Manager is read from
  its launcher (the Manager's flag is in it), so a build installed without it is said to be one.
- **Models (the user's recommendation, 2026-10-24).** The Model Manager is the way to obtain a model for ComfyUI that
  **Recommended** and **Look up online** do not know about: it downloads the file from the location its list gives
  (GitHub, Hugging Face and others) into the folder for its kind under the models folder ComfyUI was given
  (`vae_approx/taef1_decoder.pth`, F36), which is this window's models folder, so **Refresh** and the loaders find it.
  QuickAI's own methods remain unchanged.
- **Where its files are.** Its settings, lists, snapshots and history are stored in
  `~/.quickai/comfyui/user/__manager` (ComfyUI is given `--user-directory`), not beside the build, so the build can be
  deleted without losing them.
- **What it contacts (C155).** At every start, without being asked: `raw.githubusercontent.com` (its five lists) and
  `api.comfy.org` (the Registry's pages of node packs); and, when you request something, the site that hosts it
  (`github.com`, `huggingface.co`, `pypi.org` for a node's packages...). The page's network setting does not change
  this: that setting governs the page, and these are ComfyUI's own requests. The hosts are declared in the document
  (`egress`), checked against a recording by X08, and listed in [PRIVACY.md](../PRIVACY.md) (F35). **A custom node it
  installs is another party's program, which runs on this computer**; QuickAI does not verify it in any way.
- **Messages that can be ignored.** `[ERROR] [ComfyUI-Manager] PyTorch is not installed` appears at every start (it
  also expects `torchaudio`; installing it would downgrade PyTorch to 2.11: F33), as does a warning about `matrix-nio`;
  neither stops anything. It never installs `torch`, `torchvision`, `torchsde` or `torchaudio` for a node. Its
  **Update ComfyUI** and **Switch ComfyUI** buttons have nothing to update in a build, which is the source of a release
  rather than a clone: another release is installed from this window instead.
- **How it was verified.** The document and its plan (F93, F94), the launcher (F95), the installation (F96 to F98), the
  window's option and the choice carried by an interrupted installation (F99, F99a), the button's folder (P20), and,
  manually, the real page in Chromium with the real Manager: the button, the Model Manager placing a model in the
  folder, and the hosts contacted, with and without the Manager (P21, F33 to F36).

## System requirements

The table above describes what the installation does. What the computer must provide for it, and for models that run
through Triton, is defined by the document's `[[prerequisite]]` entries, which a configuration requests with
`[[needs]] system = "..."` and which `quickai-headless` checks before it downloads anything ([headless.md](headless.md),
section 12; facts F29 to F31):

| Id | What | How it is checked | Provided by (apt) |
|---|---|---|---|
| `python_venv` | The `ensurepip` module of the Python used to create the environment: Debian and Ubuntu omit it from their Python, and without it `python -m venv` fails | Before the installation, by querying the Python the installation will use; not checked for an environment that is created | `python3.12-venv` |
| `c_compiler` | `$CC`, otherwise `gcc`, otherwise `clang`: what Triton looks for to build the helper it needs at the first kernel | The `PATH` | `build-essential` |
| `python_headers` | `Python.h` in Python's header folder (the default `sysconfig` scheme, with Debian's `posix_local` read as `posix_prefix`) | By querying the environment's Python | `python3.12-dev` |
| `nvidia_driver` | A driver from the 580 series or newer, because the PyTorch build for NVIDIA cards targets CUDA 13 | `nvidia-smi`, otherwise `/proc/driver/nvidia/version` | None: drivers are not installed this way |
| `triton` | A Triton kernel is built with the computer's compiler and run on the card, starting from an empty cache | After the installation and before the models are downloaded, by the environment's own Python | None: it passes when the other prerequisites are met |

The window checks none of these; the installation it offers is unchanged.

## If an install is interrupted

The window that started the installation may be closed, killed or stop responding, the machine may be switched off, or
a step may fail. Twenty minutes of work must not be lost to any of these, and you must not have to repair a directory by
hand in order to continue (on 2026-10-08 a user had to: see *If the window stops answering*).

- **A journal.** The staging directory contains `.quickai-install.json`: the installation's steps, which of them are
  done, and a fingerprint of what they were performed for (the release, the source address, the Python, every package
  and node pack). A step is recorded when it ends, completely or not at all.
- **What is kept.** An installation that **fails after its environment was created** (pip refuses, the network drops,
  the check fails) and one that is **killed** keep their directory. One that **you cancel**, or that fails before the
  environment is created (a checksum mismatch, no Python, insufficient space), leaves nothing behind.
- **Not a build.** A directory with a journal is not a build, whatever its name: `Installed builds` does not list it,
  and the engine's own search for its program skips it. If you rename `v0.45.2.partial` to `v0.45.2` because it appears
  complete, the result is still not a build, and QuickAI says so under **Unfinished installs**.
- **Continue.** When QuickAI starts with ComfyUI selected, the log notes, once, that an installation was interrupted
  (`ComfyUI: an
  install of v0.45.2 stopped before it was whole (3 of 7 steps done, ...)`). Settings → Advanced lists such
  installations (`v0.45.2 (3 of 7 steps done, 4.8 MB, stopped 12 min ago)`), each with **Continue** and a delete
  button. Continue, or requesting the same installation again, performs the remaining steps and none of the completed
  ones (the log lists them: `Already done by the install that stopped: ...`; `Going on with the install that
  stopped (3 of 7 steps are done)`), and **always** runs the check, since the check confirms that the whole works. A
  step whose files are missing is not considered done, and neither is any step after it. If the plan differs (another
  Python, other packages, a newer application, or the Manager option changed), the completed work is not trusted and
  the installation starts over, saying so; the window's **Continue** requests the choice the installation began with
  (the record lists its steps, which reflect it). The directory is resumed under any name, and the space required is
  only what remains to be written.
- **A record that cannot be written** (a full disk) is reported, stops the installation and loses nothing: completed
  work stays in place, and the next installation resumes from it.
- **What cannot be saved.** A 6 GB wheel that was partly downloaded when the installation stopped: pip cannot resume a
  download, so that step starts over. A completed step is never downloaded again.
- **A lock.** While an installation works in a directory, it holds a lock on `.quickai-install.lock` (an advisory file
  lock, which the system releases when the process ends, however it ends). Another QuickAI window will neither clear
  that directory, continue it, nor list it as interrupted; it reports that another window is installing it.
- **When the record is removed.** After the rename, the journal is removed from the build: a complete build has none,
  and a build that still has one was interrupted between the rename and the removal, which Continue completes
  immediately.

## If the window stops answering

A window drawn by a single thread stops responding when that thread waits for something that never arrives. To avoid
this, the application ensures that:

- the **disk is never read during a frame**: the list of builds and interrupted installations (a ComfyUI build has
  65,000 files, and summing their sizes previously took 140 ms in every frame in which the section was drawn) is read by
  a dedicated thread, one scan at a time, and again only when an installation or deletion ends or the section is
  reopened;
- a **deletion runs on its own thread** (seventy thousand files) and locks the engine meanwhile;
- events that an installation or download emits many times per second request a repaint **soon**, not immediately;
- the installation and every wait run on threads whose events the window only reads.

If it happens nonetheless, a thread that checks the window once per second **while something is running** (an
installation, a download, a deletion or a search) writes to `~/.quickai/ui-stalls.log` if no frame has been drawn for
five seconds: when it happened, for how long, what the window's thread was waiting in (its state, the kernel function it
is sleeping in, the system call), the state of memory and swap, and how much of the last ten seconds any process spent
waiting for memory or for the disk. When the window responds again, the file records when, and the log pane notes that
the window had stopped responding. The record is designed to stay on this computer. A window with nothing running draws
nothing and is not monitored, and neither is a window that is covered or minimized: the toolkit does not paint a window
that nobody can see, which is not a stall.

A record has this form (the numbers are an example, not a measurement):

```
2026-10-08 01:06:12 UTC · the window has not answered for 5 s while an install was running
  its thread: S, sleeping; waiting in the kernel at do_epoll_wait; system call: ppoll (271)
  memory: 21.3 GB available, 0.1 GB of 8.0 GB swap free; this program holds 221.4 MB and has 0 B in swap
  the machine in the last 10 s: something waited for memory 34.20% of the time, for the disk 61.00%
2026-10-08 01:06:40 UTC · the window answers again after 33 s
```

**What happened on 2026-10-08.** A user installed ComfyUI from the window; the window stopped responding, and the user
killed it after inspecting the files and renaming a directory; ComfyUI was present when the window was started again.
The files show the following about the installation: the staging directory was created at 01:04:07, the launcher was
written at 01:23:48.07, the check ran until 01:23:58, the temporary directory was removed at 01:23:58.601, and the
directory's change time is that same instant. Renaming a directory updates its change time (verified), so **the
application itself performed the final rename, twenty minutes after the installation began, and nothing in the files
indicates a manual rename** (an earlier rename would have broken the subsequent steps, which wrote under the old name,
and a later one would have a later change time). The settings file has no record of the installation finishing, which
the window writes as soon as it is notified, so the window never received the notification: its thread was blocked, not
slow, and by the user's account from early on. The cause of the block was **not established**: the system journal
records no out-of-memory kill; a frame of the settings window drawn in a test takes under a millisecond; and the only
per-frame work that scaled with the size of a build (summing the files of every installed build, 140 ms for this one)
cannot take twenty minutes, and did not run for the staging directory (names ending in `.partial` were skipped). The
machine had 20 GB in a memory-backed `/tmp`, a full swap and 17 GB being written, which can starve a window;
an unresponsive display server or graphics driver are other possibilities (a covered window is not painted at all, so
that is not the cause). The recorder exists to identify the cause next time. Nothing in the installation depended on
the window responding, and nothing is lost when it is killed.

## Models

A model's files are stored in the folders of the models folder (`~/.quickai/comfyui/models/` unless you chose another)
in which ComfyUI looks for each kind; `[components.<kind>] place` specifies which, and the record beside the weights
(`<weights>.bundle.json`) lists where the other files are, so that deleting the model deletes all of them while keeping
any file another model uses. A file other than the weights is stored as `<owner>--<repo>--<name>`, as a projector is
(C63): a workflow refers to the file **by its name on disk**.

**Recommended → LTX-Video 2B distilled** (Lightricks, 2B, free below $10M a year in revenue) offers the weights alone
or together with their text encoder and VAE, each verified against its pinned hash as it downloads:

| Kind | File on disk | Folder | Bytes |
|---|---|---|---|
| weights | `ltxv-2b-0.9.8-distilled-q6_k.gguf` (Q6_K, the default) or `...-q8_0.gguf` | `diffusion_models/` | 1,718,809,088 or 2,173,891,072 |
| text encoder | `biali--ltxv-gguf--t5xxl_fp16-q4_0.gguf` | `text_encoders/` | 2,896,123,072 |
| VAE | `biali--ltxv-gguf--ltxv_0.9.5_vae_fp16.safetensors` | `vae/` | 2,493,868,876 |

The VAE is **not** `ltxv_vae_fp32-f16.gguf`, which one guide for this model names: that file is not this model's VAE,
and the 0.9.8 model decodes with the 0.9.5 VAE (fact F12).
[`examples/workflows/ltx-video-2b-t2v.json`](examples/workflows/ltx-video-2b-t2v.json) is a workflow for it in the
API format (send it to `POST /prompt`), naming the files of the default offer; if you chose Q8_0, select it in the first
field. A test checks its file names against what the button stores (F64).

A 4-second clip at 512×288 took 64 to 92 s on a 12 GB RX 6700 XT, using the **tiled** VAE decode that the example uses,
with tiles small enough for the Q8_0 weights as well (fact F13: at 256 they ran out of memory during decoding). On an
AMD card for which PyTorch has no kernels, **Present the GPU as** (`HSA_OVERRIDE_GFX_VERSION`) is derived from what
the driver reports (`gfx1031` as `10.3.0`) and can be set manually (fact F9, test F09).

A different model, **Wan 2.2 5B Turbo** (`hum-ma/Wan2.2-TI2V-5B-Turbo-GGUF`, Apache-2.0: a 5B video model distilled
to 4 steps), is obtained with **Look up online** using three buttons, because its text encoder and VAE are in other
parties' repositories: the model (`Wan2_2-TI2V-5B-Turbo-Q8_0.gguf`, 5.4 GB, into `diffusion_models/`), the text
encoder (`umt5-xxl-encoder-Q8_0.gguf`, 6.0 GB, `city96/umt5-xxl-encoder-gguf`, into `text_encoders/`) and the VAE
(`Wan2.2_VAE.safetensors`, 1.4 GB, in the `VAE` folder of `QuantStack/Wan2.2-TI2V-5B-GGUF`, into `vae/`).
[`examples/workflows/wan22-ti2v-5b-turbo-t2v.json`](examples/workflows/wan22-ti2v-5b-turbo-t2v.json) is a workflow for
it in the API format (send it to `POST /prompt`, as with the LTX workflow), using the model card's own settings (4
steps, a guidance of 1, Euler, the simple scheduler), 640×352 and 33 frames. With the **tiled** VAE decode in tiles of
192 and 16 frames, it produced a clip on the 12 GB RX 6700 XT in 183 s (90 s on a fresh ComfyUI with the files cached);
with tiles of 256 and 32, decoding ran out of memory twice and the run took 453 s (fact F27). The model's preferred size
is 1280×704: that size was not tested, and a 12 GB card may require a smaller one. A test checks the workflow's file
names against what the buttons store (Q01d). The model is not in **Recommended** because the list holds a model's files
from a single repository, and these come from three.

## Getting models

The page offers three ways to get models, the first recommended (C222): **ComfyUI-Manager**'s Model Manager, in
ComfyUI's own page ([above](#comfyui-manager)); **Recommended**, which knows a model and every file it needs; and
**Search or paste…**, the panel of Hugging Face, whose **Look up online** finds files on the hub and places each one
where it belongs, and whose field accepts a file you already have an address for. A fourth is the page's own Download
button ([below](#the-pages-download-button)). In every case, a file is stored in the folder of the models folder where
ComfyUI looks for that kind, and a workflow refers to it by its file name. QuickAI does not modify ComfyUI for this: it
places a file in a folder, and the `r` key in ComfyUI's page makes ComfyUI rescan the folders; the window's list of the
files (**On this computer**) is read again by **Refresh**, after a fetch of its own, and every few seconds while ComfyUI
runs.

```
Hugging Face                                                                              [x]
Get: [ link to a file, or search by name...       ] [...]  Into: [auto v]  [Search]
Or [Look up online] the most downloaded models of pictures and video, with their text encoders and VAEs.
Most downloaded · 2     Size: Any    Sort: Most downloaded
All 2   Video 1   Text encoders 1
  Wan2.2-TI2V-5B-GGUF        by QuantStack · 224.2K downloads · apache-2.0      5.0 GB   Fits   [Get v]
  umt5-xxl-encoder-gguf      by city96 · 270.9K downloads · apache-2.0          5.6 GB   Fits   [Get v]
```

Each result is a row with its **Get** and, in its ▾, the other sizes and the files that go by themselves (*The VAE
alone*, *The text encoder alone*).

- **Look up online.** With nothing entered, it builds the default list: the most downloaded GGUF repositories for each
  pipeline the document names (`text-to-video`, `image-to-video`, `text-to-image`, `image-to-image`) and for the word
  `encoder`, because a text encoder carries no pipeline tag (F24). This takes five searches and one request for each of
  the thirty repositories they return, the same as llama.cpp's search (L14 of [engine-facts.md](engine-facts.md): 500
  requests in five minutes are allowed). Words entered in the field form a single search, as in llama.cpp's window: the
  phrase, then each of its words on its own. The most downloaded GGUF repositories overall are language models, which
  ComfyUI does not run, and they are not listed; any that a search returns anyway appear under **Other**, collapsed.
- **What a result offers.** A repository is one row: its **Get** fetches the size the preferences favor (closest to
  the original first: `Q8_0`, `Q6_K`, `Q5_K_M`, `Q4_K_M`), and its ▾ lists the others, each with its size. In the ▾,
  **a file of a kind that can be used on its own is an item of its own**: *The VAE alone* for a VAE and *The text
  encoder alone* for a text encoder, for each file of that kind in the repository (the document defines how a kind is
  identified, through the parts of its `[[models.bundle]]`, and which kinds are offered on their own, through `alone`).
  A repository that holds only text encoders (`city96/umt5-xxl-encoder-gguf`) is a row of its own kind, **Text
  encoders**. A repository that holds the model, a text encoder and a VAE (the one in the recommended list) also offers
  all three together. What each repository holds is determined by the repository itself: three parties' repositories are three
  results.
- **What is already installed.** A result whose files are all present in their folders, at the size the hub reports, is
  marked **Installed** and its button does nothing (a file that is present is never downloaded again in any case). The
  folders are read when a search starts, when a download ends and on **Refresh**, so files placed there manually or
  downloaded by the page are counted. llama.cpp's results are not marked.
- **Where a file goes.** The weights go to `diffusion_models/` (for an image model as well as a video model: the node
  that reads GGUF reads it from there, F26), a text encoder to `text_encoders/`, and a VAE to `vae/`. A file that a
  button downloads **on its own keeps its original name** (`Wan2.2_VAE.safetensors`, `umt5-xxl-encoder-Q8_0.gguf`),
  because shared workflows refer to it by that name; the files of a bundle keep the names they have always had
  (`<owner>--<repo>--<name>`, C63), so that two repositories' `vae.safetensors` files do not collide. Every button
  shows the destination in its tooltip (*It is kept in the vae folder*), and the window repeats it when the file has
  arrived (*Fetched umt5-xxl-encoder-Q8_0.gguf (in text_encoders). ComfyUI finds it after
  Refresh*).
- **Verification.** The hub publishes a SHA-256 for every large file (`lfs.sha256` in the repository's response; it
  matches the `x-linked-etag` of the file itself, F25). The search carries it, and the download hashes the bytes as they
  arrive: a file that does not match what the hub reports (wrong size or hash) is deleted, not even kept as a partial
  file, and the window reports it in red. A file that is already present at its expected size is not downloaded again.
  The same applies to llama.cpp's window.
- **The field.** A single field accepts three kinds of input: the address of **one `.gguf` or `.safetensors` file** (a
  file's page on Hugging Face is converted to the file's address; any other `https` address must end in one of those
  extensions) is downloaded; a **file on this computer** is linked into the folder (a symbolic link, so no second copy
  is made; a copy where links are not supported); anything else is treated as search terms, and the *page of a
  repository* (`huggingface.co/city96/umt5-xxl-encoder-gguf`) is treated as that repository's name and searched.
  **Into** selects the folder (`diffusion_models`, `text_encoders`, `vae`, `loras`, `checkpoints`, `controlnet`,
  `clip_vision`, `upscale_models`, `embeddings`: the weights' folder, the kinds' folders and the others the document
  names in `places`); *auto* chooses from the file name in the same way a result does (a VAE or text encoder according
  to the document's parts, otherwise the weights' folder), and the window shows which before the button is pressed. A
  link to a file that may contain a program (`.ckpt`, `.pt`, `.pth`, `.bin`), or to anything that is not a model, is
  refused with an explanation, and nothing is requested from anyone: such files are yours to place in the folder
  (**Open folder**). The folder you choose takes precedence over the guess; it does not change what a result's button
  does, since that button already targets the correct folder.
- **What it does not do.** It does not search the hub for anything but GGUF repositories (models from `Comfy-Org` and
  similar publishers are safetensors: a workflow's own *Download*, below, or a link in the field is the way to get
  them); recognize a LoRA by its name (choose **Into** → `loras`); resume an interrupted download (the file starts over;
  the page's Download does resume from its partial file); or determine whether a model fits the graphics card (the
  recommended window greys out models larger than the available memory; a search shows the sizes). A model downloaded
  this way has **not been run by QuickAI** unless this page says so (Wan 2.2 5B Turbo, above, has been): the
  recommended list contains models from which a clip was produced on a real card, and nothing here makes that claim
  for a repository the hub lists.
- **What it contacts.** `huggingface.co` for a search (about thirty-five requests for the default list) and, for a
  file, `huggingface.co` and the download server it redirects to (`*.cdn.hf.co`), only when a button is pressed
  ([PRIVACY.md](../PRIVACY.md)). These requests are made by the window itself, not by ComfyUI, so they are not part of
  the engine's `egress`.
- **How it was verified.** The pure logic was tested on the hub's responses for a video model, its text encoder and its
  VAE as published by three parties (`tests/fixtures/huggingface/comfyui/`, recorded 2026-10-08; Q01-Q07); the window
  against a fake hub that actually serves the files (a search, each button, a wrong checksum, and the field with a link,
  a path and words; Q08-Q11); the primitive's searches (Q12) and a single-file download (Q13); and, manually, against the
  real hub (Q14: the three files of a user's list are offered with the sizes and checksums the recordings show). The
  files were downloaded for that user by a script performing the same checks (size and SHA-256 against the hub's; the
  window had not yet been built), and their headers were read by the build's own Python: the Turbo model has
  architecture `wan`, 30 blocks and 825 tensors, which the pinned GGUF node pack lists as supported (F26). The real
  window was run on a virtual display with a temporary home folder against the real hub: **Look up online** listed 28
  models, a name entered in the field followed by Enter listed 43, and a button placed a 219 MB text encoder in
  `text_encoders/` with the hub's SHA-256 (the implementation plan's log has the details).

## The page's Download button

A workflow names the models it needs. When the machine lacks one, ComfyUI's page lists it in the workflow panel's
**Issues**, under **Missing Models**, with a **Download** button. In a browser, that button is a link that the browser
saves in **its own downloads folder**: on 2026-10-08, a user who pressed it on the Z-Image-Turbo template found
`ae.safetensors` (335 MB) in `~/Downloads` rather than where ComfyUI looks for it, and asked whether a setting or an
environment variable would make it install instead. None does (fact F17): the page has three ways to handle the button,
and the one for a browser is the link; alongside it there is a slot for a host program, which ComfyUI Desktop fills
(F18). QuickAI acts as that host (C85).

- **What QuickAI adds.** At every start of ComfyUI, before it launches, QuickAI writes two items beside the models
  folder, not inside the build (so a build can still be deleted or replaced without losing anything, and a build made
  before this feature existed receives it just as a new one does): `~/.quickai/comfyui/extensions/quickai_bridge/`, a
  custom-node folder that contains no node, and `extra_model_paths.yaml`, which ComfyUI is told to read
  (`--extra-model-paths-config`) and which makes `extensions/` a location it loads custom nodes from (F19). Files that
  are unchanged are left alone, files that differ are rewritten (the log reports `Put … (1 of 3 files written)`), and
  anything you placed there yourself is not touched. This is the document's `[[launch.spawn.write]]`, which names files
  belonging to the application (`crates/quickai-runtime/assets/comfyui/`, compiled in; architecture §11.7).
- **What the bridge is.** Python code and a script, using only the standard library and what ComfyUI already loads.
  It provides three routes that only a program on this machine may use (`POST /quickai/downloads {url, name, directory}`,
  `GET /quickai/downloads`, `DELETE /quickai/downloads/<id>`) and a page script that provides
  `window.__comfyDesktop2.downloadModel`. The page's own cross-site protection applies first (ComfyUI itself refuses a
  request from another site).
- **What pressing the button does.** ComfyUI itself downloads the file, one at a time, into the folder for its kind
  (`vae/`, `text_encoders/`, `diffusion_models/`; the folder named after the kind, F21), under the name the workflow
  gives it, as `<name>.part`, which is renamed once the file is complete and, when the hub publishes its checksum (F20),
  once it matches that checksum. A file that does not match the hub's description is not kept. An interrupted download
  (a dropped connection, a kill, a power failure) resumes at the next press from the end of its `.part` file, after the
  partial file has been re-read for the checksum; a hub that resends the whole file is handled, as is a partial file
  that does not match the beginning of the file (the download restarts once and says so). A download stopped from the
  panel leaves nothing behind. A file that is already present is never overwritten (the panel says it is present), and
  there must be enough space for it plus half a gigabyte.
- **What it will not download.** An address other than `huggingface.co` over https; a file that is not `.safetensors`
  (or `.sft`) or `.gguf` (formats that hold numbers and cannot contain a program; a pickled `.ckpt`, `.pt` or `.pth` is
  yours to bring in yourself); a name that is not a plain file name (a path that climbs out of the folder, or a hidden
  file); a folder that is not one of the models folder's; or a request from another computer. **Anything it will not
  download is left to the browser**, as before, and the panel says so in a sentence; a file that requires a login at
  the hub may be saved by the browser, which holds the login.
- **What you see.** A panel at the bottom left of the page, *Models, fetched by ComfyUI*, shows each file with its
  progress, speed and estimated time remaining, and an × to stop it. The download belongs to ComfyUI rather than to
  the tab, so a page that is opened or reloaded midway shows it. When a file is in place, the page reloads its lists,
  as its `r` key does, so the model appears in the nodes and disappears from the list of missing models. In QuickAI's
  window, **Refresh** finds a weights file that arrived this way, and **Open folder** shows the folders.
- **The setting.** Settings → Advanced → **The page's Download button**: `install` (the default) or `browser`. With
  `browser`, the bridge has no routes and no script, the page has no host, and ComfyUI contacts no site: the button
  belongs to the browser, as in a ComfyUI started manually. The setting is read at startup. A ComfyUI instance that was
  already running when QuickAI attached to it has no bridge (QuickAI does not modify an instance it did not start).
- **What it contacts.** Nothing on its own; when a file is requested, `huggingface.co` and the download server the hub
  redirects to (`us.aws.cdn.hf.co` in the measurement, `*.cdn.hf.co` in the document): measured through a logging
  proxy, F22. The document declares them (`egress`), a test checks the declaration against the recording (X08), and
  [PRIVACY.md](../PRIVACY.md) lists them. The page itself is still not permitted to contact any other site
  (`--offline`), which is also why its list of missing models shows no sizes: the page's request for a file's size at
  the hub is refused by its own policy.
- **How it was verified.** The downloads (P01 to P11) were tested against a server that behaves like the hub, the glue
  code (P12) with stand-ins for ComfyUI, the plan and the files (P13 to P18), and, manually (P19,
  `crates/quickai-runtime/tests/comfy_bridge/real_page.py`), the real system: this build's page in Chromium, the
  Z-Image-Turbo template, **Download** on the VAE, ComfyUI killed midway and restarted, and the same press resuming and
  finishing with the hub's size and sha256 (F23). It was also verified once through the application itself: started on
  a virtual display with a temporary home folder, **Start** pressed, the files written by the supervisor, and the same
  download completed in the models folder it manages.

## Not implemented

- **ComfyUI-Manager is not added to an already installed build.** The option applies to the installation: a build made
  without it stays without it (and is started without the flag) until another release is installed with the option
  selected. Adding it to an existing build (one pip step in its environment and a new launcher) is possible but was not
  requested.
- The Manager's **new** interface is not offered: it has no Model Manager and, with the page offline, no list of nodes
  (F34). If you have set the page online and want it, you can start ComfyUI with `--enable-manager` manually.
- A pickled file (`.ckpt`, `.pt`, `.pth`), or one from another site (Civitai), is left to the browser: the bridge
  downloads only what it can verify. A token for a hub that requires a login is not used.
- The application's window does not show downloads requested by the page (the page does): a list based on the `.part`
  files would be possible.
- A download started by the window (from a result or the field) does not resume from where it stopped; the page's
  Download does.
- **A real ComfyUI in the GUI tests.** The GUI test suite, which the maintainers keep outside this repository, drives the
  window against fakes (a stand-in `main.py` that refuses what the real one refuses and hosts the bridge, a stand-in
  Python, a GitHub, a Hugging Face, and the same over TLS); a real ComfyUI is installed manually, as described above.
- The installer was run on Linux only, with the AMD (ROCm) PyTorch build; the CUDA, CPU and macOS steps follow from the
  same document and are tested through their conditions (F05), not on real machines.
- A model that needs more than ComfyUI's default nodes and the GGUF node pack is not in the list, and its nodes are not
  installed (the GGUF pack is the only one). Wan 2.2 5B Turbo (a model, a text encoder and a VAE from three
  repositories) is obtained with **Look up online** and produced a clip (F27), but is not in **Recommended**: the list's
  schema holds a model's companion files only from the model's own repository.
