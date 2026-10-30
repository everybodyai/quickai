# Privacy policy: QuickAI

_Last updated: 2026-10-30_

QuickAI ("the app") is a desktop program for running language models on your own computer: you pick a model,
start it, and chat with it in your browser or from another program on the same machine. It can run them with
llama.cpp (which comes with the app) or, if you choose it in Settings, with Ollama, or with ComfyUI for pictures
and video (neither of which comes with the app: it downloads them only when you ask). Ollama can be given a chat page,
Open WebUI, in the same way. This policy covers the
app however you install it: the Microsoft Store, GitHub Releases (tar.gz, AppImage or zip), Snap, Homebrew,
Scoop, Chocolatey or the AUR.

## In short

QuickAI sends nothing to its developer until you choose to. The first time it starts it asks you to pick a
**telemetry level**: **All** (crash reports, error reports, and counts of what the app did, such as which engine ran
and whether a model started), **Errors and crashes**, **Crashes only**, or **Off**. Until you answer, and whenever it is
Off, nothing is sent. Reports go to EverybodyAI's server, `api.everybodyai.dev`, with a random install id and a rough
description of your computer; they never include your prompts, the model's replies, your files, folders or searches.
`quickai --telemetry` lists everything the app can send, and Settings → Privacy shows what it did send, lets you change
the level at any time, and download or delete what EverybodyAI holds (see [Telemetry](#telemetry)).

The app has no accounts, no advertising and no automatic update check. Your prompts and the model's replies are
processed on your own computer and are never uploaded. Apart from the telemetry you choose, the app talks to the
internet only when you ask it to, to search for or download a model, a llama.cpp build, an Ollama build, a ComfyUI build
or an Open WebUI build, or to look Ollama's library up (see [When the app uses the internet](#when-the-app-uses-the-internet)).
Some of the stores and package managers you can install it through do their own data collection, independent of
anything the app does; see [Stores and package managers](#stores-and-package-managers) below.

## What the app does with your data

- **Your prompts and the model's replies.** The model runs on your computer in `llama-server` (from llama.cpp),
  which QuickAI starts and stops for you. QuickAI starts it so that it listens on `127.0.0.1` only, so other
  computers on your network cannot reach it. Prompts and replies travel only between your browser (the chat page
  that **Open Chat UI** opens) or the program you connected, and that local server; they are never sent to the
  developer or to any third party. Whatever conversation history the chat page keeps is kept by your browser, on
  your computer; the app does not store it. The local server has no password (the **API** panel shows the key as
  "none"), so any program or other user on the same computer can send it requests while it is running.
- **Ollama, if you choose it.** Ollama is a separate program, MIT-licensed, that QuickAI downloads on request
  (Settings, **Look up Ollama builds**) and never ships. QuickAI starts it as `ollama serve` so that it listens
  on `127.0.0.1` only and with `OLLAMA_NO_CLOUD=1`, which you cannot override from the app's extra-environment box:
  measured, without that variable an idle `ollama serve` contacts `ollama.com` a moment after it starts and again
  every few hours, and with it a whole session of starting, chatting and stopping made no connection off your
  computer and no DNS lookup. Ollama keeps files of its own: the first time it starts it writes a key pair
  (`.ollama/id_ed25519` and its public half), and the graphics driver writes shader caches. QuickAI starts it with a
  home folder and a temporary folder of its own, below `~/.quickai/ollama` (`home` and `cache`), so that all of that is
  kept there and nothing is written to your home folder, your cache folders or the system's temporary folder (measured:
  run with an empty home, it wrote nothing outside `~/.quickai/ollama`); the exception is when you ask for Ollama's own
  store in Settings, which is the home of the Ollama you run yourself, and then it is started with your home as it
  would be by hand. The builds QuickAI installs are kept under `~/.quickai/ollama`,
  and so are the models it pulls (`~/.quickai/ollama/models`) unless you tell it to use Ollama's own store; a list
  of the models Ollama had last time is kept there too, so something can be shown while Ollama is not running.
  If an Ollama of your own is already running on the port, QuickAI uses it, changes nothing about how it was
  started, and leaves it running when you press **Stop**. Prompts and replies travel only between your client and
  the local Ollama, as with llama-server, and the live traffic view works the same way.
- **Open WebUI, if you ask for it beside Ollama.** Open WebUI is a separate program, a chat page for Ollama's models, under
  the Open WebUI License (BSD-3-Clause with a condition that its name and logo are left as they are), that QuickAI
  downloads on request (Settings → **Chat UI: Open WebUI**, **Look up Open WebUI builds**) and never ships. It is
  installed as a Python package with a Python environment of its own under `~/.quickai/openwebui/<release>` (about 3 GB);
  packages are installed only as built wheels. QuickAI starts it so that it listens on `127.0.0.1` only, and, unless you set
  its internet access to online, **offline**: it checks for no update, fetches no model, sends no usage statistics (they are switched off
  in every case) and installs nothing a tool asks for. Everything it keeps is in `~/.quickai/openwebui`: its data
  (`data`: the accounts, conversations, files and vectors you make in it, and its secret key), its caches and temporary files
  (`cache`) and a home folder of its own (`home`), so that it writes nothing to your home folder, your cache folders or the
  folder QuickAI was started from. The first account made on its page is its administrator's, and it asks for a password; a
  page on the internet that you open cannot use it, because it answers only to its own address. What you write in it goes
  only to the Ollama QuickAI started or found, on this computer. With its internet access set to online it may contact other
  sites as it does when started by hand: Hugging Face for a model it reads documents or speech with, if you choose
  one of its own, and whatever a tool or a web search that you ask for reaches.
- **ComfyUI, if you choose it.** ComfyUI is a separate program, GPL-3.0-licensed, that makes pictures and video from
  workflows you build in its own web page, and QuickAI downloads it on request (Settings, **Look up ComfyUI
  builds**) and never ships it. It is installed as source, with a Python environment of its own, under
  `~/.quickai/comfyui/<release>` (about 17 GB, most of it PyTorch); packages are installed only as built wheels, so no
  package runs a build script on your computer. QuickAI starts it so that it listens on `127.0.0.1` only and, unless
  you change **The page's network** setting, with `--offline`, so that the page it serves contacts no other site;
  its folders are under `~/.quickai/comfyui` (`models`, `output`, `input`, `user`, `temp`), and it is started with a home
  folder and a temporary folder of its own there too (`home`, `cache`), so that the caches PyTorch and the graphics driver
  keep are kept there and not in your home folder or the system's temporary folder, and with the Python variables of your
  shell (`PYTHONPATH`, `PYTHONHOME`) put away. An install that is interrupted keeps what it did in a directory beside the builds, with a small
  record of its steps (`.quickai-install.json`) and a lock file; **Unfinished installs** in Settings lists it, and
  it is yours to continue or delete. If a ComfyUI of your own is already running on the port, QuickAI uses it and
  changes nothing about how it was started. Every time QuickAI starts ComfyUI it also writes a small folder of
  its own beside the models folder (`~/.quickai/comfyui/extensions/quickai_bridge`) and a file that tells ComfyUI to
  read it: with it, the **Download** button of ComfyUI's page puts a model in the models folder (see
  [When the app uses the internet](#when-the-app-uses-the-internet)); it has no other effect. A second small folder
  (`~/.quickai/comfyui/extensions/quickai_manager`) puts a **Manager** button at the top of ComfyUI's page when
  ComfyUI-Manager is there, and does nothing when it is not.
- **ComfyUI-Manager, if you leave its box ticked.** When you install a ComfyUI build, Settings offers **Install
  ComfyUI-Manager (highly recommended)**, ticked until you untick it. It is ComfyUI's own add-on (the version its release
  names, from the Python Package Index, 13 packages and about 30 MB, none of them PyTorch), which installs custom nodes from
  inside ComfyUI's page, and whose **Model Manager** puts a model of its list in the models folder. A build made with it
  starts ComfyUI with the Manager's classic interface; a build made without it is started without. Its settings, lists and
  history are in `~/.quickai/comfyui/user/__manager`. **A custom node it installs is another person's program, which runs on
  your computer with your rights**; QuickAI does not check it, as it does not check a node you put in ComfyUI's folder
  yourself. Nothing of ComfyUI-Manager is installed on a build made without the box.
- **The optional live traffic view.** Off by default. If you turn on "Show live conversation traffic" in
  Settings, the app puts a small proxy on your computer (also on `127.0.0.1` only) between your clients and
  `llama-server`, so the **Live traffic** tab can show the chat requests and replies passing through. What it captures
  is held in memory while the app runs and shown in that tab; it is never written to disk, and it is gone when you
  press **Clear all** or quit the app.
- **The log pane.** The pane at the bottom of the window shows the command line the app used to start
  `llama-server` and what `llama-server` prints, the last 2000 lines at most, in memory only. It is not saved.
- **When the window stops answering.** While an install, a download, a delete or a look-up runs, a thread of the app
  looks once a second at whether the window is still drawing. If it is not for five seconds, the app appends to
  `~/.quickai/ui-stalls.log` when, for how long, what the window's thread was waiting in (its state, a kernel
  function, a system call), and figures about the computer's memory, swap and disk pressure at that moment, and
  later when the window answered again. It records no prompt, file name or address, it stays on your computer, and
  you can delete it.
- **Your hardware.** To suggest a model and estimate its speed, the app reads how much memory your computer has.
  At startup it also asks your operating system which graphics adapters are installed (by running `lspci` on
  Linux, a short PowerShell command on Windows or `system_profiler` on macOS, on your computer) so that it can warn
  when a llama.cpp build you pick targets a GPU vendor you do not seem to have. The results stay inside the app;
  they are not stored or sent anywhere.
- **Settings.** The app saves a small settings file in your operating system's per-user configuration folder (on
  Linux `~/.config/quickai/settings.json`, on Windows under `%APPDATA%\quickai`): the last-used model file name,
  port, context size and GPU layers, light or dark mode, the live-traffic switch, the telemetry level you chose, and the custom models folder,
  `llama-server` path and extra command-line arguments if you entered any. It holds no account or personal data
  beyond the paths you typed, and it is never sent anywhere. The app keeps no history of your prompts, files,
  searches or downloads.
- **Configurations.** A configuration is a small text file that says what to run and how: the engine, the model (by its
  name, never by where it is on your disk), the build of the program (by its label), and the settings and
  command-line arguments of the engine and of what goes with it. They are kept in `profiles/` beside the settings file
  (on Linux `~/.config/quickai/profiles/`), and QuickAI writes one when you save, import or export one, and once, the first time it
  starts with this feature, to keep the settings you already had as your own Default. **Export**
  writes the file where you choose and nowhere else, and leaves out the folders and programs of your computer and any
  line that looks like a secret (a key, a token, a password). **Import** reads only the file you choose, shows you in full
  the text in it that would reach a command line or an environment and the settings that widen what a program may
  do, and takes only what you tick; it uses no network and starts no program. Nothing about a configuration is sent
  to anyone: it leaves your computer only if you give the file to someone. So that an export can say where a
  model you fetched can be had again, the app keeps `~/.quickai/model-sources.json`: the name and size of a file and the
  repository it was fetched from (`huggingface:owner/repo`), for the models you fetch from a look-up or from the
  recommended list; it holds no address and no account, and is never sent anywhere.
- **The headless program.** `quickai-run` is the same application without the window, for a server. It contacts the same hosts for the
  same things, but as one command and not one button at a time: `quickai-run up` (or `prepare`) reads a configuration you name and installs
  the programs and fetches the models it needs, from the hosts above (GitHub for the builds, Hugging Face for model files, the Python
  package index and PyTorch's for what ComfyUI and Open WebUI are made of). It prints what it is going to do, and how big it is, before it
  does it, and `quickai-run check` prints that and does nothing. It contacts nothing else, listens on the loopback only, writes only
  below `~/.quickai` (and, when you run `quickai-run configuration import`, your configurations, and when you run `quickai-run init`, the one
  file you asked for, readable by you only), and a file from outside that asks to
  set free text or wider access, or to fetch from a repository or an address that is not on the recommended list, is not used unless you pass
  `--accept-review`. A configuration says where a model can be had from and never holds the model; where the files of a model come from is
  yours to change (a file you have, a folder, another address, a mirror of the hub, no network at all). **A login you give** (`--hf-token`, `HF_TOKEN`
  or an environment file for Hugging Face; `--github-token`, `GITHUB_TOKEN` for GitHub's list of builds) is sent to that site and to no other: not to the
  host a hub sends a file on to, not to an address you name on another host, and not to anything the program starts. It is not printed, not in the
  JSON, not in the ready file; the environment file `quickai-run init` writes is readable by you only. Other variables you give a program
  (`--env`, `--set`) are passed to that program and are not printed when their names look like secrets.
- **Models and builds you download or import.** Models are saved under `~/.quickai/models` (or the folder you pick
  in Settings) and llama.cpp builds under `~/.quickai/llama-bin`; on Windows `.quickai` is in your user profile
  folder. Importing a local `.gguf` file creates a hard link or a copy there; the original is not changed or
  uploaded. A download in progress is kept as a `.part` file next to its destination until it finishes. The models
  that came with the app are only read, from the app's own folder.

To remove everything the app has saved, delete `~/.quickai` and the settings folder named above. Uninstalling the
app does not always do that for you.

## Telemetry

**The levels.** You choose one when QuickAI first starts, and can change it in Settings → Privacy.

- **Crashes only**: a report when QuickAI stops because of an error. It holds the error's message, where in QuickAI's
  code it happened and the names of the functions that led there, the name of the thread, and what the app was doing
  (which engine, whether it was running, which windows were open, how long the app had been open, in ranges). Never a
  log.
- **Errors and crashes** adds reports of what failed: an engine that stopped by itself (the engine, the label of its
  build, the model as named below, whether it was loading or serving, how it ended, such as `exit 1`, and how long after
  Start), a model download that failed (its site, such as Hugging Face, and a word for the reason, such as `network`
  or `checksum`, never the file or the address), the window not answering for five seconds or more (for how long, in
  ranges), and a session that ended without closing (counted at the next start).
- **All** adds counts of what the app did: that it started and was closed (and how long it was open, in ranges),
  which of its windows were opened, which features were used (the live traffic view, keeping chats, a remote computer,
  Open WebUI, the chat, importing or exporting a configuration), how a model was added (from the recommended list, a
  look-up or a link), and for each Start the engine, its build, the model, whether it ran here or on another computer
  and how long it took to be ready.

Every report also says: QuickAI's version, whether it is a build EverybodyAI released or one built from source, the
commit and the kind of computer it was built for, and how it was installed (AppImage, Snap, DMG...); your system, its
version and your processor's architecture; your memory rounded to 8, 16, 32, 64 or 128 GB and more; the makers of your
graphics adapters (NVIDIA, AMD, Intel, Apple), never the model of the card; your system's language, without the region;
and the level in force. A model is named only when it is on QuickAI's recommended list (`catalog:<id>`) or is a public
Hugging Face repository you found through the app (`hf:<owner>/<repo>`); any other model is `other` with its size in
ranges, so a file you made or renamed is never named. Before a crash report is written, the app replaces your home
folder, your user name, your computer's name, the addresses of your remote computers, other IP addresses, email
addresses, the query of any web address and anything that looks like a password or a token. Each event carries the hour
it happened in, not the moment. `quickai --telemetry` prints the whole list, as JSON; the same list is
[docs/reference/telemetry-events.md](docs/reference/telemetry-events.md).

**What it never contains**: your prompts, the model's replies or your chats; pictures; file or folder names; words you
typed into a search or a box; web addresses you typed; the text of command-line arguments or environment lines; your
configurations; your IP address (it reaches Cloudflare, which carries the request; EverybodyAI does not store it).

**At Off.** Nothing is sent. If QuickAI stopped because of an error, the next start offers to send that one report,
shows it to you first, and sends it only if you press **Send this report**. At **Crashes only** and above, a crash
report is sent at the next start without asking.

**On your computer.** Telemetry keeps its files in `~/.quickai/telemetry`: the install id (`install-id`, made the first
time anything is sent, readable by you only), the events waiting to be sent (`queue.jsonl`, at most 1 MB), when the last
batch went (`state.json`), what was sent, exactly as it was sent (`telemetry.log`, its last 1 MB), crash reports not yet
sent (`crashes/`; those you chose not to send are in `crashes/kept`, for a GitHub issue; both are deleted after 90 days),
and a small marker of each session that is open (`running-<number>.json`), so that the next start can tell a session
that ended without closing. A crash report is written there whatever the level, and leaves your computer only as said
above.

**When it is sent.** Events are sent in batches: when 100 wait, every six hours while the app is open, and when you
close it, but not in the first two minutes after it starts. If the server cannot be reached the app waits an hour, then
six, then a day, and drops what waited more than three days. Each batch is one HTTPS request to `api.everybodyai.dev`
(`POST /v1/events`, and `/v1/crash` for a crash report) that carries the install id and the user agent `quickai/<version>`.

**The install id** is random. It is not made from your hardware and is not linked to your name or any account, but it
does link your reports together, so the data is pseudonymous, not anonymous. Settings → Privacy shows it, and **New
install id** replaces it. Don't post it in public: it is how you ask EverybodyAI for your data.

**Where it goes and how long it is kept.** To EverybodyAI's server, `api.everybodyai.dev`, which runs on Cloudflare
(Workers and a D1 database kept in the European Union) under Cloudflare's data processing terms. The server records the
country your request came from (two letters), not your address, and logs nothing of a request. Reports are kept in full
for 90 days, and counts per engine and model for 13 months; after that only totals with no install id remain. Crash
reports are kept for 180 days.

**Your choices.** Settings → Privacy → **Show what is sent** shows what was sent, what waits, and everything that can be
sent. **Download what EverybodyAI holds** saves all of it to a file in `~/.quickai/telemetry`; **Delete what EverybodyAI
holds** deletes it from the server at once, empties the queue and starts a new install id (`GET` and `DELETE
/v1/me`). You can change the level at any time; a lower level drops what waits that it no longer allows. Without the
app, write to <hello@everybodyai.dev> with your install id. Consent is the basis of all of this, and withdrawing it
changes nothing that was lawful before.

**Off for good.** A build made without the `telemetry` feature has no code that sends, and Settings says so (packagers
can make one with `--no-default-features --features remote-setup`). A file `telemetry.toml` in the package's `config`
folder with `level = "off"` (or `crash`, `error`) holds the level down for everyone using that installation, for an
organization. `QUICKAI_TELEMETRY=off` (or `crash`, `error`) or `DO_NOT_TRACK=1` in the environment does the same.
`quickai-run`, the headless program, sends no telemetry.

**What it is not for.** Telemetry is never sold or shared, never used for advertising, and never used to check a
licence.

**Help → Report a problem** sends nothing to EverybodyAI. It opens a new issue on GitHub with what you wrote and your
system's details, which you read and can change before you post it, under GitHub's privacy statement.

## When the app uses the internet

Apart from the telemetry you choose (above), the app never connects to the internet by itself: not at startup, not
while it runs, not while you chat. It does so only when you press a button, or Enter in a box, for one of these:

- **Searching for a model.** Words typed into the box of **Add a model** (anything that is neither a link nor the
  path of an existing file is treated as a search) and Enter, Enter in the empty box (which lists the most downloaded),
  or **Look up online** on ComfyUI's page. The app asks
  Hugging Face (`huggingface.co`) for popular GGUF models and, for each repository it shows, for its tags and the
  names, sizes and checksums of its files (one request each: a look-up makes about thirty). The words you typed are
  part of the request. With ComfyUI chosen the plain list is made of a few searches instead of one (the most
  downloaded GGUF models of text-to-video, image-to-video, text-to-image and image-to-image, and of text encoders,
  about thirty-five requests in all). No cookie is kept and nothing is sent but the request and a `User-Agent` that
  says `quickai`. Hugging Face sees your IP address and handles it under its own
  [privacy policy](https://huggingface.co/privacy).
- **The recommended models.** **Add a model** with its box empty suggests models from a list that is part of the
  application, and **Recommended** (in the Models menu) opens the whole list: opening them, searching the list and
  closing them contact no one. A button of it fetches what it says (below), from the host the list names: Hugging Face
  for files, and for Ollama the name is pulled by Ollama itself from its registry (`registry.ollama.ai`). Every file is
  checked against the SHA-256 the list holds, and one that does not match is deleted.
- **Downloading a model.** The download button of a result or a suggestion, or a link pasted into the box of **Add a
  model**. The file is requested from that
  address: Hugging Face for results, or whatever address you entered. That server sees your IP address and the
  request. A result that comes with a second file (a vision model's projector) fetches both from the same place,
  one after the other; QuickAI writes a small record of the pair (`<model>.bundle.json`) beside the model in
  `~/.quickai/models` and nothing else leaves your machine. A file of a result is checked against the SHA-256 that
  Hugging Face states for it, and one that does not match is deleted. With ComfyUI chosen the box takes the address of
  one `.gguf` or `.safetensors` file (a link to its page on Hugging Face is turned into the link to the file) and
  keeps it in the folder of the models folder you choose or its name suggests; Hugging Face sends the file on to a
  download server (`*.cdn.hf.co`), which sees your IP address and the name of the file too.
- **Open WebUI.** Settings, **Look up Open WebUI builds** asks the Python Package Index (`pypi.org`) for the releases of
  the package `open-webui`; the request identifies itself as `quickai`. Installing a release downloads its wheel from the
  index (`files.pythonhosted.org`), checks it against the SHA-256 the index lists (and, for the release QuickAI was tested
  with, the one QuickAI holds), and then pip fetches the packages it needs from the index and PyTorch for the processor from
  `download.pytorch.org` (about 1.6 GB in all). The index and PyTorch's site see your IP address and the names of the
  packages. Unless you chose a Python in Settings, the first install also downloads QuickAI's own Python (see **QuickAI's
  own Python** below).
- **llama.cpp builds.** Settings, **Look up llama.cpp builds** asks GitHub's API (`api.github.com`) for llama.cpp's
  recent releases; the request identifies itself as `quickai`. Installing one of them, or entering a direct
  download link in the `llama-server` box, downloads that archive from GitHub or from the address you gave. GitHub
  handles your IP address under its
  [privacy statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement).
  A build you install is a program that runs on your computer with your own rights, so install builds only from
  sources you trust.

- **Ollama builds.** With Ollama chosen, Settings, **Look up Ollama builds** asks GitHub's API
  (`api.github.com`) for the releases of `ollama/ollama`; installing one downloads that archive from GitHub
  (`github.com`, which sends the file from `release-assets.githubusercontent.com`), and the app checks it against
  the checksum GitHub lists (and the one recorded in the app for the release it was tested with) before it unpacks
  anything.
- **Looking up Ollama models.** With Ollama chosen, words typed into the box of **Add a model** and Enter ask the
  library at Ollama's website, `ollama.com`, for models that run on your computer: the page of its search for those
  words, or, for Enter in the empty box, the models it shows first. The suggestions under an empty box are the
  application's own list, and ask nothing of anyone. The words are part of the request (a `:tag` after a name is
  left out), and the request identifies itself as `quickai` with its version. The site sets an anonymous cookie on
  its pages to tell visitors apart; the app does not keep it, so the next request carries none. ollama.com sees your
  IP address and handles it under its own [privacy policy](https://ollama.com/privacy). The app reads the page it
  gets back and lists the models that page names, and pulls nothing until you choose one (see the next item). The
  look-up is the app's own request, not Ollama's, so it needs no Ollama installed or running. Nothing is looked up
  just because Ollama was chosen.
- **Pulling an Ollama model.** A name with a tag typed into the box of **Add a model** with Ollama chosen
  (`qwen3:4b`, or a Hugging Face reference such as `hf.co/user/repo`) is pulled only when you press Enter. QuickAI asks the local Ollama to do
  it, and **Ollama** then contacts its registry, `registry.ollama.ai`, or Hugging Face for an `hf.co/`
  reference: `hf.co`, and the servers the files come from, `*.cdn.hf.co` (both measured with a real Ollama, which
  contacted nothing else); those servers see your IP address and the name. A size chosen in the results of the
  look-up above is pulled the same way, as `name:size`. Nothing is pulled, listed or started just because Ollama was
  chosen.
- **QuickAI's own Python.** Open WebUI and ComfyUI run in a Python environment that QuickAI makes with a Python of its
  own, so that nothing of your computer's Python is needed or changed. Unless you chose a Python in the engine's Python
  box in Settings, the first install of each downloads that Python once (CPython 3.12, a build of the
  python-build-standalone project, 25 to 67 MB) from GitHub (`github.com`, which sends the file from
  `release-assets.githubusercontent.com`), checks it against the SHA-256 recorded in the app, and keeps it in
  `~/.quickai/openwebui/python` or `~/.quickai/comfyui/python`. GitHub sees your IP address and the name of the file.
- **ComfyUI builds.** With ComfyUI chosen, Settings, **Look up ComfyUI builds** asks GitHub's API
  (`api.github.com`) for the releases of `Comfy-Org/ComfyUI`; installing one downloads the source of that release,
  and of the pack of GGUF nodes (`city96/ComfyUI-GGUF`), from GitHub (`github.com`, which sends the archive from
  `codeload.github.com`), and the app checks each against the checksum recorded in the app (a release other than the
  one it was tested with has none, and the log says it was not checked). Unless you chose a Python in Settings, the first
  install also downloads QuickAI's own Python (above). Then **pip**, run by the app inside the
  environment it made, downloads PyTorch from the PyTorch package index (`download.pytorch.org`, whose files come from
  `download-r2.pytorch.org`) and what ComfyUI needs from the Python Package Index (`pypi.org`, and
  `files.pythonhosted.org` for the files); with the box for ComfyUI-Manager ticked, its thirteen packages come from the
  same two. Those servers see your IP address and the names of the packages, which
  are the same for everyone. Nothing is downloaded just because ComfyUI was chosen.
- **ComfyUI-Manager.** In a build made with it (the box above), **every time ComfyUI starts**, with no one asking, the Manager
  refreshes its lists: five files from `raw.githubusercontent.com` (its lists of custom nodes and models) and the Comfy
  Registry's pages of node packs from `api.comfy.org` (measured on 2026-10-24: nothing else was contacted at the start). When you
  ask its Model Manager for a model it fetches it from the site the model is on: a model of its own list may be on
  `github.com`, on Hugging Face (`huggingface.co`, `*.cdn.hf.co`) or on another site; when you ask it for a custom node it
  fetches it from where the node is, and the node's own packages from the Python Package Index. Those sites see your IP
  address and what you asked for. **The page's network setting does not change this**: it is about what the page may
  contact, and the Manager's requests are ComfyUI's own. A build made without the box does none of it.
- **The Download button of ComfyUI's page.** A workflow can name the models it needs, and ComfyUI's page then lists
  the ones you do not have, each with a **Download** button. In a browser that button only asks the browser to save
  the file in its own folder. With ComfyUI started by QuickAI it asks **ComfyUI** to do it, and only when you press
  it: ComfyUI fetches that one file from Hugging Face (`huggingface.co`, which sends it on to a download server,
  `*.cdn.hf.co`; measured, and nothing else was contacted) into the folder of its kind in the models folder, under the
  name the workflow gives it, and checks it against the checksum Hugging Face states. Those servers see your IP
  address and the name of the file. Only `.safetensors` and `.gguf` files, only from `huggingface.co`, and only a request
  from this computer are carried out; for anything else the page does what it does without QuickAI, which is to leave
  the file to your browser. Nothing is fetched just because a workflow was opened, and **The page's network** setting
  does not change this: it is about what the page itself may contact.

- **A remote computer.** When you add a computer of your own under **Remote computers** and press **Test**, or **Start**
  with it chosen under **Runs on**, QuickAI runs your own `ssh` program to that computer, with your keys and
  `~/.ssh/config`, and keeps the connection open while the run lasts. The models, the chat and everything an app you
  connect sends then go to that computer and back, inside the SSH connection; nothing is opened to the network on either
  side. QuickAI keeps the address, the ports and the name of a key file you give in its settings, and never a password
  (it cannot ask for one). That computer, and whoever runs it, sees what you send to the models there.

- **Telemetry, at the level you chose.** As described in [Telemetry](#telemetry), to `api.everybodyai.dev` only, and
  nothing at Off.

Each of these is an ordinary web request (HTTPS, or plain HTTP if you type an `http://` address), or for a remote
computer an SSH connection, from your computer straight to that server. Apart from telemetry, the developer of QuickAI is
not involved in them and receives nothing.

The chat keeps nothing unless you tick **Keep chats**; then each conversation is a file in QuickAI's own data folder on
this computer (`~/.quickai/chats`), and you can delete it from the list. A picture you attach is sent to the model and
not kept.

## Bundled software

Ollama is not bundled: it is downloaded on request, and its archive carries its own licence files (see
`THIRD_PARTY_LICENSES.md`). The app ships with `llama-server` (from [llama.cpp](https://github.com/ggml-org/llama.cpp), MIT) and one model,
Bonsai 2 4B (from Prism ML, built from Alibaba Cloud's Qwen3.5-4B, Apache-2.0), unmodified. They run entirely on your computer; QuickAI
starts `llama-server` without any option that makes it download anything. If you add your own extra command-line
arguments in Settings, what they do is up to `llama-server`. The licenses are in `LICENSE` and
`THIRD_PARTY_LICENSES.md`.

## Stores and package managers

These collect data independently of the app: none of it passes through QuickAI's own code, and most of it doesn't
reach the developer at all.

### Microsoft Store

If you install the app from the Microsoft Store, Microsoft may collect data about the installation and use of apps
under [Microsoft's privacy statement](https://privacy.microsoft.com/privacystatement). The developer can see the
aggregate reports the Store provides (for example install counts, and crash reports from people who chose to share
diagnostic data with Microsoft). These do not identify you. Windows may keep the app's settings file in the app's
private data folder, which is removed when you uninstall the app.

### Snap Store

If you install the app from the Snap Store, `snapd` periodically checks for and installs updates on its own, and
Canonical's servers see those requests under [Canonical's privacy policy](https://canonical.com/legal/data-privacy).
The developer can see aggregate install metrics the Snap Store provides (for example counts by country,
architecture and app version). These do not identify you. A snap keeps the app's files, including its settings and
downloads, in its own private folder under `~/snap/quickai/` instead of directly in your home folder.

### Chocolatey

Chocolatey's Community Repository logs the IP address, package name and timestamp of each download to produce
public install-count statistics shown on the package's page; see
[Chocolatey's privacy policy](https://chocolatey.org/privacy). The `choco` client itself sends no telemetry of its
own.

### Homebrew

Homebrew (the package manager, not this project) collects anonymous install analytics by default for formulae from
any tap, including this one: package and tap name, CPU architecture, OS, and whether the install was explicit, with
no user identifier or IP address recorded, per [Homebrew's analytics documentation](https://docs.brew.sh/Analytics).
It's on by default; turn it off with `brew analytics off`. This feeds Homebrew's own public aggregate statistics,
not a per-project dashboard the developer can see.

### Others

GitHub Releases (the direct tarball, AppImage and zip downloads, and where the Scoop, Chocolatey, Homebrew and AUR
packages fetch the app from), Scoop, and the AUR add no telemetry of their own beyond what's already covered above;
a GitHub release page shows public per-file download counts, visible to anyone, not just the developer. An AppImage
embeds update information so that tools such as AppImageUpdate can look for new releases on GitHub; QuickAI itself
never does.

## Children

The app is not directed at children and does not knowingly collect information from anyone.

## Changes

If this policy changes, the new version is published in this file with a new date.

## Contact

QuickAI is made by EverybodyAI (Nihal Abdul Kader). Questions about this policy: write to
<hello@everybodyai.dev>, or open an issue at <https://github.com/everybodyai/quickai/issues>.
