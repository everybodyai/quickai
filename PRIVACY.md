# Privacy policy: QuickAI

_Last updated: 2026-11-11_

QuickAI ("the app") is a desktop application for running language models on your own computer. You select a model,
start it, and chat with it in your browser or from another program on the same computer. The app can run models with
llama.cpp, which is included in the bundled edition and downloaded on request otherwise, or, if you select them in
Settings, with Ollama or with ComfyUI (for images and video). Ollama and ComfyUI are not included with the app; they are
downloaded only when you ask. Open WebUI, a chat interface for Ollama, is obtained in the same way.

This policy applies to the app however it is installed: from the Microsoft Store, from GitHub Releases (tar.gz,
AppImage or zip), or through Snap, Homebrew, Scoop, Chocolatey or the AUR.

## In short

QuickAI sends nothing to its developer. It has no telemetry, no installation identifier, and nothing else that
distinguishes your copy from anyone else's. When a problem occurs (QuickAI crashes, an engine stops unexpectedly, or
the window stops responding), the app shows you a report describing what happened, and you decide whether to submit
it. A single click opens QuickAI's website or a GitHub issue with the report filled in; nothing is sent until you press
the submit button on that page (see [Problem reports](#problem-reports)).

The app has no accounts, no advertising and no automatic update check. Your prompts and the model's responses are
processed on your own computer, and the app does not upload them. The app connects to the internet only at your
request: to search for or download a model, a llama.cpp build, an Ollama build, a ComfyUI build or an Open WebUI build,
or to search Ollama's library (see [When the app uses the internet](#when-the-app-uses-the-internet)). Some of the
stores and package managers through which you can install the app collect data of their own, independently of the
app; see [Stores and package managers](#stores-and-package-managers) below.

## Limitations

This policy describes how QuickAI is designed to handle your data. We have taken care to ensure that it behaves as
described, and we test it accordingly, but we cannot guarantee that it will do so in every case. Software may contain
defects, and an error or malfunction in QuickAI, or in a program it runs on your behalf (an engine, a model, an MCP
server or a chat interface), could cause data to be transmitted or retained unexpectedly, or a host not listed in this
policy to be contacted. If you observe such behavior, please report it as described in [SECURITY.md](SECURITY.md); we
treat it as a defect and will correct it. If your data must not leave your computer under any circumstances, we
recommend that you also apply safeguards of your own, such as a firewall rule that blocks QuickAI and the programs it
starts.

QuickAI is provided "as is", without warranty of any kind, express or implied, and its authors shall not be liable for
any claim, damages or other liability arising from its use, as set out in its [MIT License](LICENSE). Nothing in this
policy creates any warranty or obligation beyond those terms.

## What the app does with your data

- **Your prompts and the model's responses.** The model runs on your computer in `llama-server` (from llama.cpp),
  which QuickAI starts and stops for you. QuickAI configures it to listen on `127.0.0.1` only, so that other computers
  on your network cannot reach it. Prompts and responses pass only between that local server and your browser (the
  chat page opened by **Open Chat UI**) or the program you connected; the app does not send them to the developer or
  to any third party. Any conversation history the chat page keeps is stored by your browser, on your computer; the
  app does not store it. The local server has no password (the **API** panel shows the key as "none"), so while it is
  running, any program or other user on the same computer can send it requests.
- **Your voice, if you use the microphone.** Voice features are off until you turn on **Use voice** in Settings ›
  Voice; until then, QuickAI does not open the microphone. Once voice is on, the microphone records only while you are
  speaking to QuickAI: after you press the microphone button (or while you hold Ctrl+Space), or while hands-free mode
  is on, which the window indicates. The recording is transcribed by quickai-whisper (based on whisper.cpp) or, if you
  select the Omnilingual model, by quickai-voice; both run alongside QuickAI on your computer and make no network
  connections. The recording is not stored: it is
  discarded once its text has been placed in the chat input (or sent, if you chose that option). Responses are read
  aloud on your computer, using a voice you have installed. Neither the recordings nor the read-aloud text are sent
  anywhere. The speech recognition model is downloaded once from Hugging Face (see below), unless your package
  included it.
- **Ollama, if you select it.** Ollama is a separate, MIT-licensed program that QuickAI downloads at your request
  (Settings, **Look up Ollama builds**); it is never included with the app.
  - QuickAI starts it as `ollama serve`, listening on `127.0.0.1` only, with `OLLAMA_NO_CLOUD=1` set. This variable
    cannot be overridden from the app's extra-environment box. In our measurements, an idle `ollama serve` without this
    variable contacted `ollama.com` shortly after starting and again every few hours; with it, a complete session of
    starting, chatting and stopping made no connection outside your computer and no DNS lookup.
  - Ollama keeps files of its own: when it first starts, it writes a key pair (`.ollama/id_ed25519` and its public
    key), and the graphics driver writes shader caches. QuickAI starts Ollama with its own home folder and temporary
    folder under `~/.quickai/ollama` (`home` and `cache`), so that these files are kept there and nothing is written to
    your home folder, your cache folders or the system's temporary folder. (In our measurements, when started with an
    empty home folder, it wrote nothing outside `~/.quickai/ollama`.) The exception is when you select Ollama's own
    model store in Settings: that store belongs to the Ollama you run yourself, so QuickAI starts Ollama with your home
    folder, as it would be started manually.
  - The builds QuickAI installs are kept under `~/.quickai/ollama`, as are the models it pulls
    (`~/.quickai/ollama/models`), unless you choose Ollama's own store. A list of the models Ollama had at its last run
    is also kept there, so that the app can display them while Ollama is not running.
  - If an Ollama instance of your own is already running on the port, QuickAI uses it, does not change how it was
    started, and leaves it running when you press **Stop**.
  - As with llama-server, prompts and responses pass only between your client and the local Ollama, and the live
    traffic view works in the same way.
- **Open WebUI, if you enable it alongside Ollama.** Open WebUI is a separate program, a chat interface for Ollama's
  models, licensed under the Open WebUI License (BSD-3-Clause with an additional condition that its name and logo be
  retained). QuickAI downloads it at your request (Settings → **Chat UI: Open WebUI**, **Look up Open WebUI builds**);
  it is never included with the app.
  - It is installed as a Python package, in a dedicated Python environment under `~/.quickai/openwebui/<release>`
    (about 3 GB). Packages are installed only from pre-built wheels.
  - QuickAI starts it listening on `127.0.0.1` only and, unless you set its internet access to online, in **offline**
    mode: it does not check for updates, does not download models, does not send usage statistics (these are disabled
    in every case), and does not install packages requested by tools.
  - Everything it stores is kept in `~/.quickai/openwebui`: its data (`data`: the accounts, conversations, files and
    vectors you create in it, and its secret key), its caches and temporary files (`cache`), and its own home folder
    (`home`). It therefore writes nothing to your home folder, your cache folders, or the folder from which QuickAI was
    started.
  - The first account created on its page is the administrator account, and it requires a password. A web page you
    open on the internet cannot use it, because Open WebUI responds only to requests addressed to its own address.
  - What you write in it is sent only to the Ollama instance that QuickAI started or found, on your computer.
  - With its internet access set to online, it may contact other sites, as it would when started manually: Hugging
    Face, for a document-reading or speech model if you choose one of its own, and whatever a tool or web search that
    you request reaches.
- **ComfyUI, if you select it.** ComfyUI is a separate, GPL-3.0-licensed program that generates images and video from
  workflows you build in its own web page. QuickAI downloads it at your request (Settings, **Look up ComfyUI
  builds**); it is never included with the app.
  - It is installed from source, with a dedicated Python environment, under `~/.quickai/comfyui/<release>` (about
    17 GB, most of which is PyTorch). Packages are installed only from pre-built wheels, so no package runs a build
    script on your computer.
  - QuickAI starts it listening on `127.0.0.1` only and, unless you change the **The page's network** setting, with
    `--offline`, so that the page it serves contacts no other site.
  - Its folders are under `~/.quickai/comfyui` (`models`, `output`, `input`, `user`, `temp`). QuickAI also starts it
    with its own home folder and temporary folder there (`home`, `cache`), so that the caches kept by PyTorch and the
    graphics driver are stored there rather than in your home folder or the system's temporary folder. The Python
    variables of your shell (`PYTHONPATH`, `PYTHONHOME`) are removed from its environment.
  - An interrupted installation keeps its progress in a directory alongside the builds, together with a small record
    of its steps (`.quickai-install.json`) and a lock file. **Unfinished installs** in Settings lists it, and you may
    resume or delete it.
  - If a ComfyUI instance of your own is already running on the port, QuickAI uses it and does not change how it was
    started.
  - Each time QuickAI starts ComfyUI, it also writes a small extension folder next to the models folder
    (`~/.quickai/comfyui/extensions/quickai_bridge`) and a file that tells ComfyUI to load it. This extension makes the
    **Download** button on ComfyUI's page save a model into the models folder (see
    [When the app uses the internet](#when-the-app-uses-the-internet)); it has no other effect. A second small folder
    (`~/.quickai/comfyui/extensions/quickai_manager`) adds a **Manager** button at the top of ComfyUI's page when
    ComfyUI-Manager is installed, and does nothing otherwise.
- **ComfyUI-Manager, if you leave its option selected.** When you install a ComfyUI build, Settings offers **Install
  ComfyUI-Manager (highly recommended)**, selected by default. ComfyUI-Manager is ComfyUI's own add-on (the version
  named by the ComfyUI release, from the Python Package Index: 13 packages, about 30 MB, none of them PyTorch). It
  installs custom nodes from within ComfyUI's page, and its **Model Manager** saves models from its list into the
  models folder.
  - A build made with it starts ComfyUI with the Manager's classic interface; a build made without it starts without.
  - Its settings, lists and history are stored in `~/.quickai/comfyui/user/__manager`.
  - **A custom node installed through it is a program written by someone else, which runs on your computer with your
    permissions.** QuickAI does not check it, just as it does not check a node you place in ComfyUI's folder yourself.
  - No part of ComfyUI-Manager is installed in a build made without this option.
- **The optional live traffic view.** This feature is off by default. If you turn on "Show live conversation traffic"
  in Settings, the app runs a small proxy on your computer (also listening on `127.0.0.1` only) between your clients
  and `llama-server`, so that the **Live traffic** tab can display the chat requests and responses passing through it.
  The captured traffic is held in memory while the app runs and shown only in that tab; it is never written to disk,
  and it is discarded when you press **Clear all** or quit the app.
- **The log pane.** The pane at the bottom of the window shows the command line the app used to start `llama-server`
  and the output of `llama-server`, up to the last 2000 lines, in memory only. It is not saved.
- **When the window stops responding.** While an installation, download, deletion or lookup is in progress, a
  background thread of the app checks once per second whether the window is still being drawn. If it has not been
  drawn for five seconds, the app appends an entry to `~/.quickai/ui-stalls.log` recording when this happened, for how
  long, what the window's thread was waiting on (its state, a kernel function, a system call), figures on the
  computer's memory, swap and disk pressure at that moment, and when the window became responsive again. The entry
  contains no prompts, file names or addresses; the file stays on your computer, and you may delete it.
- **Your hardware.** To suggest a model and estimate its speed, the app reads how much memory your computer has. At
  startup it also asks your operating system which graphics adapters are installed (by running `lspci` on Linux, a
  short PowerShell command on Windows, or `system_profiler` on macOS, locally), so that it can warn you when a
  llama.cpp build you select targets a GPU vendor your computer does not appear to have. The results remain within the
  app; they are not stored or sent anywhere.
- **Settings.** The app saves a small settings file in your operating system's per-user configuration folder (on Linux
  `~/.config/quickai/settings.json`, on Windows under `%APPDATA%\quickai`). It contains the file name of the last model
  used, the port, context size and GPU layers, light or dark mode, the live-traffic setting, and, if you entered them,
  the custom models folder, the `llama-server` path and extra command-line arguments. It holds no account or personal
  data beyond the paths you entered, and it is never sent anywhere. The app keeps no history of your prompts, files,
  searches or downloads.
- **Configurations.** A configuration is a small text file that describes what to run and how: the engine, the model
  (by name, never by its location on your disk), the build of the program (by its label), and the settings and
  command-line arguments of the engine and its companion programs.
  - Configurations are stored in `profiles/` next to the settings file (on Linux `~/.config/quickai/profiles/`).
    QuickAI writes one when you save, import or export a configuration, and once, the first time it starts with this
    feature, to preserve your existing settings as your own Default.
  - **Export** writes the file only to the location you choose, and omits the folders and programs of your computer
    and any line that appears to be a secret (a key, a token or a password).
  - **Import** reads only the file you choose, shows you in full any text in it that would be passed to a command line
    or an environment and any settings that would widen what a program may do, and applies only the items you select.
    It uses no network and starts no program.
  - Nothing about a configuration is sent to anyone; it leaves your computer only if you share the file yourself.
  - So that an export can state where a model you downloaded can be obtained again, the app keeps
    `~/.quickai/model-sources.json`: the name and size of each file and the repository it was downloaded from
    (`huggingface:owner/repo`), for models you download from a search or from the recommended list. It holds no
    address and no account, and it is never sent anywhere.
- **The headless program.** `quickai-headless` is the same application without a window, intended for servers. It contacts
  the same hosts for the same purposes, but as a single command rather than one button at a time.
  - `quickai-headless up` (or `prepare`) reads a configuration you name, installs the programs and downloads the models it
    requires from the hosts listed below (GitHub for builds, Hugging Face for model files, and the Python Package Index
    and PyTorch's package index for the components of ComfyUI and Open WebUI). It shows what it is going to do, and the
    download size, before doing it; `quickai-headless check` shows the same information and does nothing.
  - It contacts nothing else and listens on the loopback interface only. It writes only below `~/.quickai`, plus your
    configurations when you run `quickai-headless configuration import`, and, when you run `quickai-headless init`, the single
    file you requested, readable only by you.
  - A file from an outside source that requests free text or wider access, or that would download from a repository
    or address not on the recommended list, is not used unless you pass `--accept-review`.
  - A configuration states where a model can be obtained and never contains the model itself; you may change where a
    model's files come from (a file you already have, a folder, another address, a mirror of the hub, or no network at
    all).
  - **Credentials you provide** (`--hf-token`, `HF_TOKEN` or an environment file for Hugging Face; `--github-token`,
    `GITHUB_TOKEN` for GitHub's list of builds) are sent only to that site: not to a host to which the hub redirects a
    file, not to an address you name on another host, and not to any program it starts. They are not printed, and do
    not appear in the JSON output or the ready file; the environment file written by `quickai-headless init` is readable
    only by you. Other variables you pass to a program (`--env`, `--set`) are passed to that program and are not
    printed when their names suggest they are secrets.
- **Models and builds you download or import.** Models are saved under `~/.quickai/models` (or the folder you choose in
  Settings), and llama.cpp builds under `~/.quickai/llama-bin`; on Windows, `.quickai` is in your user profile folder.
  Importing a local `.gguf` file creates a hard link or a copy there; the original is neither changed nor uploaded. A
  download in progress is kept as a `.part` file next to its destination until it completes. The models included with
  the app are only read, from the app's own folder.

To remove everything the app has saved, delete `~/.quickai` and the settings folder named above. Uninstalling the app
does not always do this for you.

## Problem reports

QuickAI has no telemetry: it does not send any information about you or your computer on its own. Versions 0.2.1 to
0.2.5 could send crash and error reports to EverybodyAI if you opted in; this capability was removed in 0.2.6. On its
first start, version 0.2.6 deletes the data that this telemetry stored on your computer (`~/.quickai/telemetry`,
including the installation identifier) and the reporting level you had chosen. The server no longer accepts those
reports, and all data it held from them has been deleted.

**When a report is shown.** At the next start after QuickAI crashed; when an engine stopped unexpectedly while starting
or running; and when the window did not respond for five seconds or more. You can also open one at any time with
**Help → Report a problem**.

**What a report contains.**

- **In every report:** a description of what happened; QuickAI's version, whether it is a build released by
  EverybodyAI or one built from source, its commit, the type of computer it was built for, and how it was installed
  (AppImage, Snap, zip, and so on); your operating system, its version and your processor's architecture; your memory,
  rounded to 8, 16, 32, 64 or 128 GB and above; and the makers of your graphics adapters (NVIDIA, AMD, Intel, Apple),
  never the model of the card.
- **For a crash:** the error message, where in QuickAI's code it occurred and the names of the functions that led
  there, the name of the thread, how long QuickAI had been open (in ranges), the engine and its state, the activity in
  progress (a download, an installation, and so on), which windows were open, and the state of the computer's memory
  and disk at that moment (in ranges).
- **For an engine that stopped:** the engine, the label of its build, the model (identified as described below),
  whether it was loading or serving, how it ended (for example `exit 1`), how many seconds after Start, the number of
  the engine document's diagnostic that its log matched, and the last 30 lines of the engine's log.
- **For a window that stopped responding:** for how long (in ranges), the activity in progress, the engine and its
  state, which windows were open, the same figures about the computer, and what the window's thread was doing as
  reported by the operating system.

A model is named only when it is on QuickAI's recommended list (`catalog:<id>`) or is a public Hugging Face repository
you found through the app (`hf:<owner>/<repo>`); any other model is reported as `other`, with its size in ranges. A
report has no identifier, no timestamp, and nothing that distinguishes your computer from another.

**What is removed first.** Before the report is shown to you, the app removes your home folder, your user name, your
computer's name, the addresses of your remote computers, other IP addresses, email addresses, the query part of any web
address, model file names in paths, and anything that resembles a password or token. This process cannot be perfect,
which is why you review the report before submitting it.

**How to submit it, if you choose to.** The report window has three buttons:

- **Report on QuickAI's site** opens `quickai.everybodyai.dev/report` in your browser with the report filled in. The
  report is carried in the part of the address after `#`, which your browser never sends to a server, so nothing leaves
  your computer until you press **Send** on that page. What that page retains, and for how long, is described in
  [the site's privacy policy](https://everybodyai.dev/privacy).
- **Report on GitHub** opens a new issue on GitHub's bug report form with the report filled in, for you to review and
  submit under GitHub's privacy statement; nothing is posted until you press its button.
- **Copy** places the report on your clipboard.

The first two buttons also place the report on your clipboard, and a report too long to fit in an address is
transferred only through the clipboard, for you to paste.

**On your computer.** A crash report is written to `~/.quickai/reports` when the crash occurs (QuickAI cannot show a
window while it is crashing), shown at the next start, and deleted when you close its window or after 30 days. Periods
when the window stopped responding are also recorded, as plain text lines, in `~/.quickai/ui-stalls.log`. Neither file
leaves your computer on its own.

## When the app uses the internet

The app is designed not to connect to the internet on its own: not at startup, not while it runs, and not while you
chat. It connects only when you press a button, or press Enter in an input box, for one of the following purposes:

- **Searching for a model.** Words typed into the **Add a model** box followed by Enter (any input that is neither a
  link nor the path of an existing file is treated as a search), Enter in the empty box (which lists the most
  downloaded models), or **Look up online** on ComfyUI's page. The app asks Hugging Face (`huggingface.co`) for popular
  GGUF models and, for each repository it displays, for its tags and the names, sizes and checksums of its files (one
  request each; a search makes about thirty requests). The words you typed are part of the request. With ComfyUI
  selected, the default list is built from several searches instead of one (the most downloaded GGUF models for
  text-to-video, image-to-video, text-to-image and image-to-image, and text encoders; about thirty-five requests in
  all). No cookie is kept, and nothing is sent other than the request itself and a `User-Agent` of `quickai`. Hugging
  Face sees your IP address and handles it under its own [privacy policy](https://huggingface.co/privacy).
- **The speech recognition model.** The first time you press the microphone, if your package did not include the
  model, the panel above the microphone offers to download it. **Download** fetches one file of
  `ggerganov/whisper.cpp` from Hugging Face (`huggingface.co`, about 82 MB), which is verified against the checksum
  pinned in the app. A larger model is downloaded in the same way when you select it in Settings › Voice. The
  Omnilingual model, if you select it, is one archive of sherpa-onnx's releases on GitHub (`github.com`, about
  290 MB), and Omnilingual large two files of
  `csukuangfj2/sherpa-onnx-omnilingual-asr-1600-languages-1B-ctc-v2-int8-2026-02-05` from Hugging Face (about 1 GB),
  each verified in the same way; QuickAI's voice program, quickai-voice, which runs them, is downloaded with either from
  GitHub if it is not already on your computer.
- **A voice for reading responses aloud.** No voice is included with QuickAI. When you select a voice in Settings ›
  Voice › Read with (Kokoro, or the voices for Arabic, Persian and Urdu) and press **Download**, it is downloaded from
  sherpa-onnx's releases on GitHub (`github.com`, 67 MB to 350 MB), verified against the checksum pinned in the app. With the first voice, QuickAI's
  voice program, quickai-voice, is downloaded from its releases on GitHub (`github.com/everybodyai/quickai-voice`).
  Both are stored in `~/.quickai/voice`. Reading aloud then takes place on your computer and contacts no one.
- **A speech server, if you choose one.** If you select **A speech server** as the reader, each sentence of a response
  is sent to the address you provide, which converts it to audio. A server on your own computer keeps the text local;
  a server elsewhere receives the text it reads. Its key, if you provide one, is stored in QuickAI's settings on your
  computer.
- **The recommended models.** With its box empty, **Add a model** suggests models from a list built into the
  application, and **Recommended** (in the Models menu) opens the complete list. Opening, searching and closing these
  lists contacts no one. A button in the list downloads what it describes (see below) from the host the list names:
  Hugging Face for files, and, for Ollama, the model is pulled by Ollama itself from its registry
  (`registry.ollama.ai`). Every file is verified against the SHA-256 checksum held in the list, and a file that does
  not match is deleted.
- **Downloading a model.** The download button of a search result or suggestion, or a link pasted into the **Add a
  model** box. The file is requested from that address: Hugging Face for search results, or the address you entered.
  That server sees your IP address and the request. A result that includes a second file (a vision model's projector)
  downloads both from the same source, one after the other; QuickAI writes a small record of the pair
  (`<model>.bundle.json`) next to the model in `~/.quickai/models`, and nothing else leaves your computer. A file from a
  search result is verified against the SHA-256 checksum that Hugging Face states for it, and a file that does not
  match is deleted. With ComfyUI selected, the box accepts the address of a single `.gguf` or `.safetensors` file (a
  link to its page on Hugging Face is converted into a link to the file) and saves it in the subfolder of the models
  folder that you choose or that its name suggests. Hugging Face redirects the file to a download server
  (`*.cdn.hf.co`), which also sees your IP address and the name of the file.
- **Open WebUI.** **Look up Open WebUI builds** in Settings asks the Python Package Index (`pypi.org`) for the releases
  of the `open-webui` package; the request identifies itself as `quickai`. Installing a release downloads its wheel
  from the index (`files.pythonhosted.org`) and verifies it against the SHA-256 checksum the index lists (and, for the
  release QuickAI was tested with, the checksum QuickAI holds). pip then downloads the required packages from the index,
  and PyTorch for the processor from `download.pytorch.org` (about 1.6 GB in total). The index and PyTorch's site see
  your IP address and the names of the packages. Unless you selected a Python installation in Settings, the first
  installation also downloads QuickAI's own Python (see **QuickAI's own Python** below).
- **llama.cpp builds.** **Look up llama.cpp builds** in Settings asks GitHub's API (`api.github.com`) for llama.cpp's
  recent releases; the request identifies itself as `quickai`. Installing one of them, or entering a direct download
  link in the `llama-server` box, downloads that archive from GitHub or from the address you provided. GitHub handles
  your IP address under its
  [privacy statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement). A
  build you install is a program that runs on your computer with your own permissions, so install builds only from
  sources you trust.
- **Ollama builds.** With Ollama selected, **Look up Ollama builds** in Settings asks GitHub's API (`api.github.com`)
  for the releases of `ollama/ollama`. Installing one downloads that archive from GitHub (`github.com`, which serves the
  file from `release-assets.githubusercontent.com`), and the app verifies it against the checksum GitHub lists (and the
  checksum recorded in the app for the release it was tested with) before unpacking anything.
- **Searching Ollama models.** With Ollama selected, words typed into the **Add a model** box followed by Enter query
  the library on Ollama's website, `ollama.com`, for models that run on your computer: the search page for those words,
  or, for Enter in the empty box, the models it lists first. The suggestions shown under an empty box come from the
  application's own list and contact no one. The words are part of the request (a `:tag` after a name is omitted), and
  the request identifies itself as `quickai` with its version. The site sets an anonymous cookie on its pages to
  distinguish visitors; the app does not keep it, so the next request carries none. ollama.com sees your IP address
  and handles it under its own [privacy policy](https://ollama.com/privacy). The app reads the returned page, lists the
  models it names, and pulls nothing until you choose one (see the next item). The search is the app's own request,
  not Ollama's, so it does not require Ollama to be installed or running. Selecting Ollama does not in itself trigger
  a search.
- **Pulling an Ollama model.** A name with a tag typed into the **Add a model** box with Ollama selected (`qwen3:4b`,
  or a Hugging Face reference such as `hf.co/user/repo`) is pulled only when you press Enter. QuickAI asks the local
  Ollama to pull it, and **Ollama** then contacts its registry, `registry.ollama.ai`, or, for an `hf.co/` reference,
  Hugging Face: `hf.co` and the servers that deliver the files, `*.cdn.hf.co` (both measured with a real Ollama, which
  contacted nothing else). Those servers see your IP address and the model name. A size selected in the results of the
  search above is pulled in the same way, as `name:size`. Selecting Ollama does not in itself pull, list or start
  anything.
- **QuickAI's own Python.** Open WebUI and ComfyUI run in a Python environment that QuickAI creates with its own Python
  installation, so that your computer's Python is neither required nor modified. Unless you selected a Python
  installation in the engine's Python box in Settings, the first installation of each downloads that Python once
  (CPython 3.12, a build from the python-build-standalone project, 25 to 67 MB) from GitHub (`github.com`, which serves
  the file from `release-assets.githubusercontent.com`), verifies it against the SHA-256 checksum recorded in the app,
  and stores it in `~/.quickai/openwebui/python` or `~/.quickai/comfyui/python`. GitHub sees your IP address and the
  name of the file.
- **ComfyUI builds.** With ComfyUI selected, **Look up ComfyUI builds** in Settings asks GitHub's API
  (`api.github.com`) for the releases of `Comfy-Org/ComfyUI`. Installing one downloads the source of that release, and
  of the GGUF node pack (`city96/ComfyUI-GGUF`), from GitHub (`github.com`, which serves the archive from
  `codeload.github.com`), and the app verifies each against the checksum recorded in the app (a release other than the
  one it was tested with has no recorded checksum, and the log states that it was not verified). Unless you selected a
  Python installation in Settings, the first installation also downloads QuickAI's own Python (see above). **pip**, run
  by the app within the environment it created, then downloads PyTorch from the PyTorch package index
  (`download.pytorch.org`, which serves files from `download-r2.pytorch.org`) and ComfyUI's dependencies from the
  Python Package Index (`pypi.org`, with files from `files.pythonhosted.org`); with the ComfyUI-Manager option
  selected, its thirteen packages come from the same two sources. Those servers see your IP address and the names of
  the packages, which are the same for everyone. Selecting ComfyUI does not in itself download anything.
- **ComfyUI-Manager.** In a build made with it (see the option above), **every time ComfyUI starts**, without any
  action on your part, the Manager refreshes its lists: five files from `raw.githubusercontent.com` (its lists of
  custom nodes and models) and the Comfy Registry's pages of node packs from `api.comfy.org` (as measured on
  2026-10-24, nothing else was contacted at startup). When you ask its Model Manager for a model, it downloads the model
  from the site that hosts it: a model on its list may be hosted on `github.com`, on Hugging Face (`huggingface.co`,
  `*.cdn.hf.co`) or on another site. When you ask it for a custom node, it downloads the node from where it is hosted,
  and the node's own packages from the Python Package Index. Those sites see your IP address and what you requested.
  **The page's network setting does not affect this**: that setting governs what the page may contact, whereas the
  Manager's requests are made by ComfyUI itself. A build made without the option does none of this.
- **The Download button on ComfyUI's page.** A workflow can name the models it requires, and ComfyUI's page then lists
  those you do not have, each with a **Download** button. In a browser alone, that button only asks the browser to save
  the file in its own downloads folder. With ComfyUI started by QuickAI, it asks **ComfyUI** to download the file, and
  only when you press it: ComfyUI fetches that single file from Hugging Face (`huggingface.co`, which redirects it to a
  download server, `*.cdn.hf.co`; as measured, nothing else was contacted) into the appropriate subfolder of the models
  folder, under the name the workflow specifies, and verifies it against the checksum Hugging Face states. Those
  servers see your IP address and the name of the file. Only `.safetensors` and `.gguf` files, only from
  `huggingface.co`, and only requests from your own computer are carried out; for anything else, the page behaves as it
  would without QuickAI and leaves the download to your browser. Opening a workflow does not in itself download
  anything, and **The page's network** setting does not affect this, as it governs only what the page itself may
  contact.
- **A remote computer.** When you add a computer of your own under **Remote computers** and press **Test**, or press
  **Start** with it selected under **Runs on**, QuickAI runs your own `ssh` program to connect to that computer, using
  your keys and `~/.ssh/config`, and keeps the connection open for as long as the model runs there. The models, the
  chat, and everything a connected application sends then travel to that computer and back inside the SSH connection;
  no port is opened to the network on either side. QuickAI stores the address, the ports and the name of any key file
  you provide in its settings, and never a password (it cannot prompt for one). That computer, and whoever operates it,
  can see what you send to the models running there.
- **MCP servers you add.** Only servers you add yourself under **MCP servers** in the sidebar, and only those that are
  turned on, are used. QuickAI starts them on your computer when you chat (or open that page) in order to list their
  tools, and llama-server also starts them at each **Start** for its own chat page.
  - A server configured as a command is a program that runs on your computer: what it reads, writes and contacts is
    determined by that program, and is not known to QuickAI's developer.
  - A server configured as an address is contacted by QuickAI itself to list its tools and whenever one of its tools
    runs, with the headers you provided (such as a key).
  - A tool runs only when you allow it, or automatically if you set it to **Always**. The content of the model's tool
    call is sent to that server, which sees your IP address and handles what it receives under its own terms. The
    tool's result is returned to the model; if the model runs on another computer of yours, the result is sent there.
  - The server list, including its keys, and your permission settings for each tool are stored on your computer (in
    QuickAI's settings and in `~/.quickai/llamacpp/mcp-servers.json`, both readable only by you), and are never
    included in a configuration you share.

Each of these connections is an ordinary web request (HTTPS, or plain HTTP if you enter an `http://` address), or, for
a remote computer, an SSH connection, made directly from your computer to that server. QuickAI's developer is not
involved in these connections and receives nothing from them. A problem report reaches EverybodyAI or GitHub only if
you submit it yourself, from your browser.

The chat stores nothing unless you turn on **Keep chats**. When it is on, each conversation is saved as a file in
QuickAI's own data folder on your computer (`~/.quickai/chats`), and you can delete it from the list. An image you
attach is sent to the model and is not stored.

## Bundled software

Ollama is not bundled: it is downloaded at your request, and its archive includes its own license files (see
`THIRD_PARTY_LICENSES.md`). The app is distributed with `llama-server` (from
[llama.cpp](https://github.com/ggml-org/llama.cpp), MIT) and one model, Bonsai 2 4B (from Prism ML, built from Alibaba
Cloud's Qwen3.5-4B, Apache-2.0), both unmodified. They run entirely on your computer; QuickAI starts `llama-server`
without any option that would cause it to download anything. If you add your own command-line arguments in Settings,
their effect is determined by `llama-server`. The license texts are in `LICENSE` and `THIRD_PARTY_LICENSES.md`.

## Stores and package managers

The following services collect data independently of the app: none of it passes through QuickAI's own code, and most
of it does not reach the developer at all.

### Microsoft Store

If you install the app from the Microsoft Store, Microsoft may collect data about the installation and use of apps
under [Microsoft's privacy statement](https://privacy.microsoft.com/privacystatement). The developer can see the
aggregate reports the Store provides (for example, install counts, and crash reports from users who chose to share
diagnostic data with Microsoft). These reports do not identify you. Windows may keep the app's settings file in the
app's private data folder, which is removed when you uninstall the app.

### Snap Store

If you install the app from the Snap Store, `snapd` periodically checks for and installs updates on its own, and
Canonical's servers receive those requests under [Canonical's privacy policy](https://canonical.com/legal/data-privacy).
The developer can see the aggregate installation metrics the Snap Store provides (for example, counts by country,
architecture and app version). These metrics do not identify you. A snap keeps the app's files, including its settings
and downloads, in its own private folder under `~/snap/quickai/` rather than directly in your home folder.

### Chocolatey

Chocolatey's Community Repository logs the IP address, package name and timestamp of each download in order to
produce the public install-count statistics shown on the package's page; see
[Chocolatey's privacy policy](https://chocolatey.org/privacy). The `choco` client itself sends no telemetry of its own.

### Homebrew

Homebrew (the package manager, not this project) collects anonymous installation analytics by default for formulae
from any tap, including this one: the package and tap name, CPU architecture, operating system, and whether the
installation was explicit, with no user identifier or IP address recorded, as described in
[Homebrew's analytics documentation](https://docs.brew.sh/Analytics). This collection is on by default; you can turn
it off with `brew analytics off`. The data feeds Homebrew's own public aggregate statistics, not a per-project
dashboard visible to the developer.

### Others

GitHub Releases (the direct tarball, AppImage and zip downloads, which are also the source from which the Scoop,
Chocolatey, Homebrew and AUR packages obtain the app), Scoop and the AUR add no telemetry of their own beyond what is
described above. A GitHub release page shows public per-file download counts, visible to anyone, not only to the
developer. An AppImage embeds update information so that tools such as AppImageUpdate can check GitHub for new
releases; QuickAI itself never does so.

## Children

The app is not directed at children and does not knowingly collect information from anyone.

## Changes

If this policy changes, the revised version will be published in this file with a new date.

## Contact

QuickAI is made by EverybodyAI (Nihal Abdul Kader). For questions about this policy, write to
<hello@everybodyai.dev>, or open an issue at <https://github.com/everybodyai/quickai/issues>.
