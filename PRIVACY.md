# Privacy policy: QuickAI

_Last updated: 2026-10-03_

QuickAI ("the app") is a desktop program for running language models on your own computer: you pick a model,
start it, and chat with it in your browser or from another program on the same machine. This policy covers the
app however you install it: the Microsoft Store, GitHub Releases (tar.gz, AppImage or zip), Snap, Homebrew,
Scoop, Chocolatey or the AUR.

## In short

The app itself does not collect, store or send personal information to its developer or to anyone else. It has no
accounts, no analytics or telemetry, no advertising, no crash reporting and no automatic update check of its own.
Your prompts and the model's replies are processed on your own computer and are never uploaded. The app talks to
the internet only when you ask it to, to search for or download a model or a llama.cpp build (see
[When the app uses the internet](#when-the-app-uses-the-internet)). Some of the stores and package managers you can
install it through do their own data collection, independent of anything the app does; see
[Stores and package managers](#stores-and-package-managers) below.

## What the app does with your data

- **Your prompts and the model's replies.** The model runs on your computer in `llama-server` (from llama.cpp),
  which QuickAI starts and stops for you. QuickAI starts it so that it listens on `127.0.0.1` only, so other
  computers on your network cannot reach it. Prompts and replies travel only between your browser (the chat page
  that **Open Chat UI** opens) or the program you connected, and that local server; they are never sent to the
  developer or to any third party. Whatever conversation history the chat page keeps is kept by your browser, on
  your computer; the app does not store it. The local server has no password (the **API** panel shows the key as
  "none"), so any program or other user on the same computer can send it requests while it is running.
- **The optional live traffic view.** Off by default. If you turn on "Show live conversation traffic" in
  ⚙ Settings, the app puts a small proxy on your computer (also on `127.0.0.1` only) between your clients and
  `llama-server`, so the **📡 Live** tab can show the chat requests and replies passing through. What it captures
  is held in memory while the app runs and shown in that tab; it is never written to disk, and it is gone when you
  press **Clear all** or quit the app.
- **The log pane.** The pane at the bottom of the window shows the command line the app used to start
  `llama-server` and what `llama-server` prints, the last 2000 lines at most, in memory only. It is not saved.
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
- **Models and builds you download or import.** Models are saved under `~/.quickai/models` (or the folder you pick
  in ⚙ Settings) and llama.cpp builds under `~/.quickai/llama-bin`; on Windows `.quickai` is in your user profile
  folder. Importing a local `.gguf` file creates a hard link or a copy there; the original is not changed or
  uploaded. A download in progress is kept as a `.part` file next to its destination until it finishes. The models
  that came with the app are only read, from the app's own folder.

To remove everything the app has saved, delete `~/.quickai` and the settings folder named above. Uninstalling the
app does not always do that for you.

## When the app uses the internet

The app never connects to the internet by itself: not at startup, not while it runs, not while you chat. It does so
only when you press a button, or Enter in a box, for one of these:

- **Searching for a model.** **Look up online**, or typing a name into the Model box (anything that is neither a
  link nor the path of an existing file is treated as a search) and pressing Enter or **Search**. The app asks
  Hugging Face (`huggingface.co`) for popular GGUF models and for the files in a repository. The words you typed
  are part of the request. Hugging Face sees your IP address and handles it under its own
  [privacy policy](https://huggingface.co/privacy).
- **Downloading a model.** **Get** on a result, or a link pasted into the Model box. The file is requested from that
  address: Hugging Face for results, or whatever address you entered. That server sees your IP address and the
  request.
- **llama.cpp builds.** ⚙ Settings, **Look up llama.cpp builds** asks GitHub's API (`api.github.com`) for llama.cpp's
  recent releases; the request identifies itself as `quickai`. Installing one of them, or entering a direct
  download link in the `llama-server` box, downloads that archive from GitHub or from the address you gave. GitHub
  handles your IP address under its
  [privacy statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement).
  A build you install is a program that runs on your computer with your own rights, so install builds only from
  sources you trust.

Each of these is an ordinary web request (HTTPS, or plain HTTP if you type an `http://` address) from your computer
straight to that server. The developer of QuickAI is not involved in them and receives nothing.

## Bundled software

The app ships with `llama-server` (from [llama.cpp](https://github.com/ggml-org/llama.cpp), MIT) and two models,
Bonsai 8B and Bonsai 1.7B (from PrismML, Apache-2.0), unmodified. They run entirely on your computer; QuickAI
starts `llama-server` without any option that makes it download anything. If you add your own extra command-line
arguments in ⚙ Settings, what they do is up to `llama-server`. The licenses are in `LICENSE` and
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

Questions about this policy: open an issue at <https://github.com/nujufas/QuickAI/issues>.
