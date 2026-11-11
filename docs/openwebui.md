# Open WebUI beside Ollama

`ollama serve` has no page to open: it is a daemon with an API. **Open WebUI** is a chat interface for Ollama's models
(conversations, documents and tools), and QuickAI offers it in Ollama's settings. If you want a chat interface for
Ollama, you turn it on and install it once; from then on it starts together with Ollama, and **Open Chat UI** opens
it. Nothing is downloaded until you request it, and it remains off until you turn it on.

Open WebUI is a **companion** of Ollama: it is described by an engine document in every respect but one
([`openwebui.toml`](examples/engines/openwebui.toml), `[engine] companion_of = "ollama"`), so it cannot be chosen as an
engine. The facts it relies on are W1 to W16 of [engine-facts.md](engine-facts.md), measured with the real package; the
design decisions are C89 to C97 of [implementation-plan.md](implementation-plan.md); and the tests are group T of
[phase3-test-plan.md](phase3-test-plan.md).

Open WebUI is distributed under the [Open WebUI License](https://docs.openwebui.com/license) (BSD-3-Clause with one
additional condition: its name and logo must not be altered or removed). It is **not included in any package**: it is
downloaded at your request, and QuickAI leaves its name and logo unchanged.

## What the user sees

* **Settings → Chat UI: Open WebUI** (shown only when Ollama is the engine). The section describes Open WebUI and its
  license, and contains: *Start Open WebUI with Ollama* (off), *Port* (3000), *Open WebUI's internet access* (offline),
  the standard build manager (*Look up Open WebUI builds*, *Installed builds*, *Unfinished installs*, *Cancel*) and,
  under *More*, the Python interpreter to install with and any extra environment variables. The section opens
  automatically when Open WebUI is enabled but not yet installed, since installing it is the next step. Its list of
  builds starts with *Default* (the newest installed build, marked `not installed yet` until one exists): no part of
  Open WebUI is bundled, and the window never claims otherwise. *Open WebUI's internet access* controls whether it may
  use the internet (`OFFLINE_MODE`, below); it can reach Ollama either way.
* **The browser chat button** is the globe icon beside the model while Ollama runs (its label appears when you hover
  over it). For Ollama it reads **Set up chat UI...** while Open WebUI is off or not installed (it opens the settings
  with the Open WebUI section expanded), and **Open Chat UI** once it is enabled and installed: the button is disabled
  until the page is running, and then opens `http://127.0.0.1:3000` in the browser.
* **The status line** reports what the pair is doing:
  `Ollama ready, no model loaded (port 11434) · Open WebUI on port 3000`, `· Open WebUI starting...`, or
  `· Open WebUI stopped unexpectedly` (with a panel showing the advice its document gives for what the log reported).
* **The log** shows Open WebUI's output prefixed with its name: `[Open WebUI] ...`.
* **When it starts.** It starts together with Ollama, as soon as Ollama responds, provided it is enabled and installed.
  If it is enabled or installed while Ollama is already running, it starts at that moment (once per Ollama run: if it
  stops on its own, it waits for the next Start). If it is turned off while running, it keeps running until Ollama's
  run ends. Its checkbox, port, internet access and extra environment are locked while Open WebUI itself is running
  (they are applied when it is started), but not while only Ollama is running.
* **On the first visit**, the page asks you to create an account. The first account becomes the administrator. It is
  stored in Open WebUI's folder and nowhere else.

## Installing it

*Look up Open WebUI builds* lists the releases of the `open-webui` package on the Python Package Index (the tested
release first, followed by the four newest releases that provide a wheel for every platform); a single click installs
one. The installation uses `install.python_app@1` with a wheel in place of a source archive:

| Step | What it does |
|---|---|
| python | Uses QuickAI's own interpreter (C181): CPython 3.12.15 from python-build-standalone, downloaded once into `python/` under Open WebUI's root and verified against the SHA-256 pinned in the document; or the interpreter named in the **Python** field, which must be 3.12 or 3.11 (Open WebUI requires `<3.13`). The computer's own Python is neither required nor modified |
| wheel | Downloads the release's wheel, verifies it against its sha256 (the document's pin for the tested release, or the index's own checksum for any other release, with the log stating which was used), and keeps it until the build is complete |
| venv | Runs `<python> -m venv venv/` inside the build: nothing is installed into any Python you already have, and nothing is shared with ComfyUI's environment |
| pip | Installs the CPU build of PyTorch from `download.pytorch.org/whl/cpu` (on Linux only; on other systems the index's default build is already the CPU build), then the wheel and its dependencies; always with `--only-binary=:all:` and `--no-cache-dir`, and with temporary files kept inside the installation folder |
| launcher | Writes `open-webui`, a script that clears your Python environment variables and has the environment's own Python call the entry point the package declares (`open_webui:app`, queried from the environment). The script generated by pip is not used, because it embeds the path of the folder it was created in, which is no longer the build's location once the build is complete; the first run with the real package revealed this |
| check | Runs the launcher with `--help` |
| rename | Renames `<version>.partial` → `<version>` |

Measured with the real package (W2), the installation took **247 seconds and 2.95 GB** on the development machine, of
which 1.6 GB was downloaded. The document requires 8 GiB of free space. An interrupted installation can be resumed, as
ComfyUI's can (the journal records where the wheel came from, so **Continue** works).

## Launch parameters

Open WebUI is started as `open-webui serve --host 127.0.0.1 --port 3000`, in `~/.quickai/openwebui/data`, with:

| | |
|---|---|
| `OLLAMA_BASE_URL` | The Ollama instance it accompanies, at the port given in Ollama's settings (through QuickAI's gateway when the live view is on, or the address of a daemon that was already running) |
| `OFFLINE_MODE=true` | Set by default (*Open WebUI's internet access: offline*): it does not check for updates, download models or install packages requested by tools. Choosing online removes only this setting |
| `RAG_EMBEDDING_ENGINE=ollama` | Embeddings for the documents you provide are computed by Ollama, using `nomic-embed-text` unless you choose another model in its settings; by default, Open WebUI would otherwise load its own model from huggingface.co at startup (W3) |
| no telemetry | `ANONYMIZED_TELEMETRY`, `DO_NOT_TRACK`, `SCARF_NO_ANALYTICS`, and no check for newer versions (QuickAI manages the installed versions) |
| `CORS_ALLOW_ORIGIN` | Limited to its own two origins, `http://127.0.0.1:<port>` and `http://localhost:<port>`, and never `*` |
| authentication | **Enabled**. Without it, any web page you open in your browser could call a server on 127.0.0.1 that runs the functions an administrator adds |

## Isolation (the user's requirement)

*"Always isolate llamacpp/comfyUI/ollama - completely."* Open WebUI has its own root folder and its own Python
environment, and keeps everything inside them:

* `~/.quickai/openwebui/<version>/` is a build: the launcher and `venv/`. A build can be replaced or deleted without
  affecting any of your data, and deleting it leaves ComfyUI's and Ollama's builds in place (T28, T39).
* `~/.quickai/openwebui/data/` holds its data (accounts, chats, uploads, embeddings and its secret key), `cache/` its
  caches and temporary files, and `home/` its home folder. It is started **in** `data/` (`[launch.spawn] cwd`: it writes
  its secret key to its working directory, and previously a program was started in whatever folder QuickAI was started
  from), with `HOME`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `TMPDIR`, `TEMP`, `TMP`, `XDG_CACHE_HOME`, `HF_HOME`,
  `DATA_DIR` and `STATIC_DIR` all pointing inside that root, and your own Python variables (`PYTHONPATH`,
  `PYTHONHOME`) cleared.
* This was measured: started with its own home folder and nothing else, it wrote nothing outside `data/` and `cache/`
  (W7). Started with its default environment, it wrote 889 MB to `~/.cache/huggingface` before it responded (W3).
* Environment variables you add yourself cannot override any of these settings (T10).

## Limitations

* It requires Python 3.11 or 3.12: QuickAI downloads its own Python 3.12 for it (once, about 25 to 67 MB depending on
  the computer), so a machine with only Python 3.13 or newer, or with no Python at all, needs nothing further.
* Apart from the hundred packages Open WebUI pins by version, its dependencies are installed at the newest
  versions the index offers at installation time; the wheel's own sha256 is the only pin QuickAI holds. A lock file of
  versions and hashes would close this gap (not implemented).
* Only one companion per engine is shown. If you supply your own document that changes the program a companion starts,
  you are not asked about it at Start, as you would be for an engine: the companion is not started, and the log
  explains why.
* If Open WebUI stops on its own while Ollama is running, it is restarted at Ollama's next Start, not before.
* **It retains the Ollama address it was first given** (W16, measured with the real package: started a second time
  beside an Ollama instance on another port, it listed none of that instance's models and never queried it).
  `OLLAMA_BASE_URL` and the other settings seeded from the environment are written into its own database at its first
  start. QuickAI records the address used at that first start (in the field its document names,
  `first_address_field = "first_ollama"`: a value in its own settings, never in a configuration), notes in the log at
  each start when the address it would now pass is different, and reports the same in the notes of a configuration
  that would place Ollama elsewhere. You can change the address in Open WebUI's *Admin settings, Connections*, or keep
  Ollama on its original port. A fixed loopback port that QuickAI forwards to the current Ollama instance would remove
  this limitation (not implemented). A configuration carries only those Open WebUI settings that are read at every
  start (docs/configurations.md, section 9).
* The isolation of the engines was measured on Linux (O29, F28, W7). On Windows and macOS the same variables are set.
  Open WebUI also receives its own `APPDATA` and `LOCALAPPDATA` (which have no effect on Linux); Ollama and ComfyUI do
  not, because what their programs store in `%LOCALAPPDATA%` or `~/Library` has not been measured. This is the open
  point of the isolation requirement on those two systems.
