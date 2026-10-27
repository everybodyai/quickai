# Open WebUI beside Ollama

`ollama serve` has no page to open: it is a daemon with an API. **Open WebUI** is a chat page for Ollama's models
(conversations, documents, tools), and QuickAI offers it in the settings of Ollama: a person who wants a chat page
for Ollama turns it on, installs it once, and from then on it starts with Ollama and **Open Chat UI** opens it.
Nothing is downloaded until the person asks, and it is off until they do.

It is a **companion** of Ollama: a document of an engine in every way but one ([`openwebui.toml`](examples/engines/openwebui.toml),
`[engine] companion_of = "ollama"`), so it is not an engine to choose. The facts it rests on are W1 to W16 of
[engine-facts.md](engine-facts.md), measured with the real package; the decisions are C89 to C97 of
[implementation-plan.md](implementation-plan.md); the tests are group T of [phase3-test-plan.md](phase3-test-plan.md).

Open WebUI is under the [Open WebUI License](https://docs.openwebui.com/license) (BSD-3-Clause and one more
condition: its name and logo are not to be altered or removed). It is **not part of any package**: it is
downloaded when the person asks, and QuickAI leaves its name and logo as they are.

## What a person sees

* **Settings → Chat UI: Open WebUI** (only when Ollama is the engine). The section says what it is and its
  licence, and has: *Start Open WebUI with Ollama* (off), *Port* (3000), *Open WebUI's internet access* (offline), the
  usual manager of builds (*Look up Open WebUI builds*, *Installed builds*, *Unfinished installs*, *Cancel*) and,
  under *More*, the Python to install with and extra environment. The section is open when Open WebUI is wanted and
  not installed yet, which is the thing to do. Its list of builds starts with *Default* (the newest build installed,
  and `not installed yet` beside it until there is one): nothing of Open WebUI is bundled, and the window never says
  so. *Open WebUI's internet access* is whether it may use the internet (`OFFLINE_MODE`, below); it reaches Ollama
  either way.
* **The button of the chat in a browser,** in **Connect an app** (beside Stop while Ollama runs) and in the Server
  menu. For Ollama it is **Set up chat UI...** while Open WebUI is off or not installed (it opens the settings with its
  section open), and **Open Chat UI** once it is on and installed: disabled until the page runs, then it opens
  `http://127.0.0.1:3000` in the browser.
* **The status line** says what the pair is doing: `Ollama ready, no model loaded (port 11434) · Open WebUI on port
  3000`, or `· Open WebUI starting...`, or `· Open WebUI stopped unexpectedly` (with a panel that has the advice its
  document has for what the log said).
* **The log** has Open WebUI's words with its name in front: `[Open WebUI] ...`.
* **When it starts.** With Ollama, as soon as Ollama answers, if it is turned on and installed. Turned on, or installed, while Ollama is
  already running, it starts then (once in a run of Ollama's: one that stopped by itself waits for the next Start). Turned off while it
  runs, it runs until Ollama's run ends. Its box, port, internet access and extra environment are locked while Open WebUI itself runs (they are
  spawn-time settings), and not while only Ollama does.
* **The first visit** to the page asks for an account. The first one is the administrator's. It is kept in the
  folder of Open WebUI and nowhere else.

## Installing it

*Look up Open WebUI builds* lists the releases of the package `open-webui` on the Python Package Index (the tested
one first, then the newest four that have a wheel for every platform); one click installs it. The install is
`install.python_app@1` with a wheel in place of a source archive:

| Step | What |
|---|---|
| python | the first of 3.12 and 3.11 the machine has (Open WebUI says `<3.13`), or the one named in the **Python** box; none is installed, and a machine with only a newer one is told so before anything is downloaded |
| wheel | the release's wheel, downloaded, checked against its sha256 (the document's pin for the tested release; the index's own for another, and the log says which), kept until the build is whole |
| venv | `<python> -m venv venv/`, inside the build: nothing is installed into a Python the person has, and nothing is shared with ComfyUI's |
| pip | PyTorch for the processor from `download.pytorch.org/whl/cpu` (on Linux only; elsewhere the index's own is that one), then the wheel and what it needs; always `--only-binary=:all:`, `--no-cache-dir`, the temporary files inside the install's folder |
| launcher | `open-webui`, a script that puts the person's Python variables away and has the environment's own Python call the function the package says the program calls (`open_webui:app`: asked of the environment; the script pip made is not run, because it holds the path of the folder it was made in, which is not where the build is once it is whole: the first run of the real thing found that) |
| check | the launcher with `--help` |
| rename | `<version>.partial` → `<version>` |

Measured with the real package (W2): **247 seconds and 2.95 GB** on the machine this was built on, 1.6 GB of it
downloaded. The document asks for 8 GiB free. An install that stops is gone on with, as ComfyUI's is (the journal
records where the wheel came from, so **Continue** works).

## What it is started with

`open-webui serve --host 127.0.0.1 --port 3000`, in `~/.quickai/openwebui/data`, and:

| | |
|---|---|
| `OLLAMA_BASE_URL` | the Ollama it goes with, at the port its settings give (through QuickAI's gateway when the live view is on, and the address of a daemon that was already running) |
| `OFFLINE_MODE=true` | by default (*Open WebUI's internet access: offline*): it looks for no update, fetches no model and installs no package a tool asks for. Online takes only this away |
| `RAG_EMBEDDING_ENGINE=ollama` | the vectors of the documents a person gives it are made by Ollama, with `nomic-embed-text` unless they choose another in its settings; by default Open WebUI loads a model of its own from huggingface.co at start (W3) |
| no telemetry | `ANONYMIZED_TELEMETRY`, `DO_NOT_TRACK`, `SCARF_NO_ANALYTICS`, and no check for a newer version (QuickAI installs the versions) |
| `CORS_ALLOW_ORIGIN` | its own two origins, `http://127.0.0.1:<port>` and `http://localhost:<port>`, and never `*` |
| authentication | **on**. With it off, any page the person opens in their browser could call a server on 127.0.0.1 that runs the functions an administrator adds |

## Isolation (the person's rule)

*"Always isolate llamacpp/comfyUI/ollama - completely."* Open WebUI has a root of its own and a Python environment
of its own, and keeps everything in it:

* `~/.quickai/openwebui/<version>/` is a build: the launcher and `venv/`. A build is replaced or deleted and
  nothing of the person's goes with it; deleting it leaves ComfyUI's and Ollama's builds where they were (T28, T39).
* `~/.quickai/openwebui/data/` is its data (accounts, chats, uploads, vectors, its secret key), `cache/` its caches
  and temporary files, `home/` its home folder. It is started **in** `data/` (`[launch.spawn] cwd`: it writes its
  secret key to its working directory, and until now a program started where QuickAI was started from), with
  `HOME`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `TMPDIR`, `TEMP`, `TMP`, `XDG_CACHE_HOME`, `HF_HOME`,
  `DATA_DIR` and `STATIC_DIR` all inside that root, and the variables of the person's own Python
  (`PYTHONPATH`, `PYTHONHOME`) empty.
* It was measured: started with a home of its own and nothing else, it wrote nothing outside `data/` and `cache/` (W7).
  Started with the environment it has by default, it wrote 889 MB to `~/.cache/huggingface` before it answered (W3).
* The person's own lines of environment cannot replace any of this (T10).

## Limits

* It needs Python 3.11 or 3.12, which QuickAI finds and never installs. A machine with only 3.13 or newer says so.
* The packages Open WebUI needs other than the hundred it pins by version are the newest the index offers at the
  time of the install; the wheel's own sha256 is the only pin QuickAI holds. A file of locked versions with their
  hashes would close that (not done).
* One companion per engine is drawn. A user document of the person's own that changes the program it starts is not
  asked about at Start, as an engine's would be: the companion is not started, and the log says why.
* When Open WebUI stops by itself while Ollama runs, it is started again at the next Start of Ollama, not before.
* **It keeps the address of Ollama it was first given** (W16, measured with the real one: started a second time beside an Ollama on
  another port, it listed none of its models and never asked it). `OLLAMA_BASE_URL` and the other settings the environment seeds are written into its
  own database at its first start. QuickAI writes down the address of that first start (in the field its document names, `first_address_field = "first_ollama"`: a value in its own settings, never in a
  configuration), says in the log at each start when the address it would give now is another, and says so in the notes of a configuration that would
  put Ollama elsewhere; the person changes it in Open WebUI's *Admin settings, Connections*, or keeps Ollama on the port it had. A fixed loopback port that QuickAI forwards
  to the current Ollama would close it (not done). What a configuration carries of Open WebUI is only what is read at every start (docs/configurations.md, section 9).
* The isolation of the engines was measured on Linux (O29, F28, W7). On Windows and macOS the same variables are set. Open WebUI also gets
  `APPDATA` and `LOCALAPPDATA` of its own (inert on Linux); Ollama and ComfyUI do not, because what their programs keep in `%LOCALAPPDATA%` or
  `~/Library` was not measured: that is the open point of the isolation rule on those two systems.
