# Privacy policy: QuickAI

_Last updated: 2026-11-08_

QuickAI ("the app") is a desktop program for running language models on your own computer: you pick a model,
start it, and chat with it in your browser or from another program on the same machine. It can run them with
llama.cpp (which comes with the app) or, if you choose it in Settings, with Ollama, or with ComfyUI for pictures
and video (neither of which comes with the app: it downloads them only when you ask). Ollama can be given a chat page,
Open WebUI, in the same way. This policy covers the
app however you install it: the Microsoft Store, GitHub Releases (tar.gz, AppImage or zip), Snap, Homebrew,
Scoop, Chocolatey or the AUR.

## In short

QuickAI sends nothing to its developer. It has no telemetry, no install id and nothing else that tells your copy from
anyone else's. When something breaks (QuickAI crashes, an engine stops by itself, or the window stops answering), it
shows you a report of what happened, and you choose whether to post it: one click opens QuickAI's site or a GitHub
issue with the report filled in, and nothing is sent until you press the button on that page (see
[Problem reports](#problem-reports)).

The app has no accounts, no advertising and no automatic update check. Your prompts and the model's replies are
processed on your own computer and are never uploaded. The app talks to the internet only when you ask it to, to search for or download a model, a llama.cpp build, an Ollama build, a ComfyUI build
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
- **What you say, if you use the microphone.** Voice is off until you turn on **Use voice** in Settings › Voice; until
  then QuickAI never opens the microphone. Once it is on, the microphone records only while you talk to QuickAI: after
  you press it (or hold Ctrl+Space), or while hands-free is on, which the window shows. What it records is written down
  by quickai-whisper (whisper.cpp), a program that runs beside QuickAI on your computer and connects to nothing, and is
  kept nowhere: the recording is dropped once its words are in the chat's box (or sent, when you chose that). Answers
  are read aloud on this computer, with a voice you installed. Nothing of either is sent anywhere. The speech model it needs is
  downloaded once, from Hugging Face (below), unless your package came with it.
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
  port, context size and GPU layers, light or dark mode, the live-traffic switch, and the custom models folder,
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

## Problem reports

QuickAI has no telemetry: it sends nothing about you or your computer by itself. Versions 0.2.1 to 0.2.5 could send
crash and error reports to EverybodyAI if you chose to; from 0.2.6 that is gone. At its first start, 0.2.6 deletes what
that telemetry kept on your computer (`~/.quickai/telemetry`, its install id among it) and the level you chose. The
server no longer takes those reports, and everything it held from them has been deleted.

**When a report is shown.** When QuickAI crashed (at the next start), when an engine stopped by itself while it
started or ran, and when the window did not answer for five seconds or more. **Help → Report a problem** shows one too,
whenever you like.

**What a report holds.** What happened; QuickAI's version, whether it is a build EverybodyAI released or one built from
source, its commit and the kind of computer it was built for, and how it was installed (AppImage, Snap, zip...); your
system, its version and your processor's architecture; your memory rounded to 8, 16, 32, 64 or 128 GB and more; the
makers of your graphics adapters (NVIDIA, AMD, Intel, Apple), never the model of the card. Then, for a crash: the
error's message, where in QuickAI's code it happened and the names of the functions that led there, the name of the
thread, how long QuickAI had been open (in ranges), the engine and its state, what was going on (a download, an
install...), which windows were open, and how the computer's memory and disk were at that moment (in ranges). For an
engine that stopped: the engine, the label of its build, the model as named below, whether it was loading or serving,
how it ended (such as `exit 1`), how many seconds after Start, the number of the engine document's diagnostic its log
matched, and the last 30 lines of the engine's log. For a window that stopped answering: for how long (in ranges), what
was going on, the engine and its state, which windows were open, the same figures about the computer, and what the
window's thread was doing as the system says it. A model is named only when it is on QuickAI's recommended list
(`catalog:<id>`) or is a public Hugging Face repository you found through the app (`hf:<owner>/<repo>`); any other model
is `other` with its size in ranges. A report has no id, no time, and nothing that tells your computer from another.

**What is taken out first.** Before you see it, the app replaces in a report your home folder, your user name, your
computer's name, the addresses of your remote computers, other IP addresses, email addresses, the query of any web
address, model file names in paths, and anything that looks like a password or a token. It cannot be perfect, which is
why you read the report before you post it.

**How it is posted, if you want.** The window has three buttons. **Report on QuickAI's site** opens
`quickai.everybodyai.dev/report` in your browser with the report filled in: it travels in the part of the address after
`#`, which your browser never sends to a server, so nothing leaves your computer until you press **Send** on that page.
What that page keeps, and for how long, is in [the site's privacy policy](https://everybodyai.dev/privacy).
**Report on GitHub** opens a new issue on GitHub's bug report form with the report filled in, for you to read and post
under GitHub's privacy statement; nothing is posted until you press its button. **Copy** puts the report on your
clipboard. Each of the first two puts it on your clipboard as well, and a report too long for an address goes only by
the clipboard, for you to paste.

**On your computer.** A crash's report is written to `~/.quickai/reports` when it happens (QuickAI cannot show a window
while it crashes), shown at the next start, and deleted when you close its window, or after 30 days. The times the
window stopped answering are also written, as plain lines, to `~/.quickai/ui-stalls.log`. Neither leaves your computer
by itself.

## When the app uses the internet

The app never connects to the internet by itself: not at startup, not
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
- **The speech model.** The first time you press the microphone, if your package did not come with it, the card over
  the microphone offers to download it; **Download** fetches one file of `ggerganov/whisper.cpp` from Hugging Face
  (`huggingface.co`, about 82 MB, checked against the checksum the app pins). A larger one is fetched the same way when
  you choose it in Settings › Voice.
- **A voice to read the answers aloud.** No voice comes with QuickAI. When you choose Kokoro in Settings › Voice › Read
  with and press **Download**, the voice is fetched from sherpa-onnx's releases on GitHub (`github.com`, 350 MB or 132
  MB, checked against the checksum the app pins), and with the first voice QuickAI's voice program, quickai-voice, from
  its releases on GitHub (`github.com/everybodyai/quickai-voice`). Both are kept in `~/.quickai/voice`. Reading aloud
  then happens on this computer and contacts no one.
- **A speech server, if you choose one.** With **A speech server** as what reads, each sentence of an answer is sent to
  the address you give, which turns it into sound: a server on this computer keeps it here, one elsewhere sees the
  text it reads. Its key, if you give one, is kept in QuickAI's settings on this computer.
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
- **MCP servers you add.** Only servers you add yourself under **MCP servers** in the sidebar, and only those that are
  on. QuickAI starts them on your computer when you chat (or open that page), to list their tools, and llama-server
  starts them too at each **Start** for its own chat page. A server that is a command is a program that runs on your
  computer: what it reads, writes and contacts is that program's, and the developer of QuickAI does not know it. A
  server at an address is contacted by QuickAI itself to list its tools and whenever a tool of it runs, with the headers
  you gave it (a key). A tool runs only when you allow it, or always for a tool you set to **Always**. What the model
  puts in a call goes to that server, which sees your IP address and handles what it gets under its own terms; what the
  tool gives back goes to the model, so with a model on another computer of yours it goes there. The list, with its
  keys, and what you said of each tool stay on this computer (in QuickAI's settings and
  `~/.quickai/llamacpp/mcp-servers.json`, both readable by you alone) and are never part of a configuration you share.

Each of these is an ordinary web request (HTTPS, or plain HTTP if you type an `http://` address), or for a remote
computer an SSH connection, from your computer straight to that server. The developer of QuickAI is not involved in them
and receives nothing. A problem report reaches EverybodyAI or GitHub only if you post it yourself, from your browser.

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
