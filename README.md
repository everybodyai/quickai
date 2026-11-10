<p align="center">
  <img src="assets/logo.svg" width="96" height="96" alt="QuickAI">
</p>

<h1 align="center">QuickAI</h1>

<p align="center">
  <strong>Run AI models on your own computer. Private by default.</strong>
</p>

<p align="center">
  <a href="https://github.com/everybodyai/quickai/actions/workflows/ci.yml"><img src="https://github.com/everybodyai/quickai/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-246E37" alt="License: MIT"></a>
  <img src="https://img.shields.io/badge/platforms-Linux%20%7C%20macOS%20%7C%20Windows-0E1611" alt="Platforms: Linux, macOS and Windows">
</p>

<p align="center">
  <a href="https://everybodyai.dev/quickai">Website</a> ·
  <a href="#install">Install</a> ·
  <a href="#quick-start">Quick start</a> ·
  <a href="docs/README.md">Docs</a>
</p>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/readme/quickai-dark.png">
    <img src="assets/readme/quickai-light.png" width="760" alt="The QuickAI window running Bonsai 2 4B, the model it comes with: the models in the sidebar at the left, the model and its Start button in the bar over the page, its reply to a question in the chat, and how it runs in the panel at the right.">
  </picture>
</p>

QuickAI is a small desktop app that downloads, runs and manages AI models on the hardware you already have:
chat, code, vision, speech, documents, images and video. Type to it or talk to it. There's no account, no cloud and
no telemetry, and your prompts never leave your computer.

## Why QuickAI

- **Private by default.** The models run on your computer, and the servers QuickAI starts listen on `127.0.0.1`
  only. No accounts, no update checks, no telemetry: nothing is sent to us. It uses the internet only when you click
  something that needs it. [PRIVACY.md](PRIVACY.md) lists every host it can contact,
  and why.
- **Works out of the box.** The packages whose names end in `-with-llamacpp-bonsai2-4b` come with llama.cpp,
  Bonsai 2 4B (ternary weights, 1.2 GB) and a speech model, so your first chat needs no download at all. The app
  alone is about 30 MB and fetches them when you ask.
- **Talk to it (experimental).** Turn on voice in Settings → Voice, and the microphone in the chat writes down what
  you say, **Read aloud** reads an answer in your computer's own voice, and hands-free mode lets you talk with nothing
  to press. Your voice never leaves your computer either.
- **Runs on ordinary hardware.** GPU acceleration through Vulkan on AMD, NVIDIA and Intel, and Metal on Apple
  silicon. QuickAI suggests a model that fits your memory and estimates how fast it will run before you start it,
  on the graphics card, the processor, or both.
- **70+ recommended models.** A curated list for chat, reasoning, code, vision, document reading (OCR), speech,
  translation and search. Every file is checked against its SHA-256 as it arrives.
- **Three engines.** llama.cpp is built in. [Ollama](https://ollama.com) and [ComfyUI](https://www.comfy.org)
  (images and video) are installed only when you ask, each in a folder of its own.
- **Tools for your model.** MCP servers let a model read files, fetch web pages and more, in QuickAI's own chat,
  whichever engine runs it.
- **Works with your other apps.** An OpenAI-compatible API on your own machine, for editors, scripts and chat
  clients. **Live traffic**, at the foot of the window, shows what each app asks and what the model answers, as it
  happens.
- **Configurations you can share.** Save a whole setup (engine, model, settings) as one small file and give it
  to someone else. Local paths and secrets are never exported.
- **Headless, too.** `quickai-run` starts a configuration on a server with no display.

## Install

> QuickAI for Windows (zip), Linux (AppImage and tar.gz) and macOS (one disk image for Apple silicon and Intel
> Macs) is on the [Releases](https://github.com/everybodyai/quickai/releases) page. Packages for the AUR, Homebrew,
> Snap, Scoop, Chocolatey and the Microsoft Store are to follow; to hear when they are out, sign up at
> [everybodyai.dev](https://everybodyai.dev/quickai). You can also [build it from source](#build-from-source).

Each system has two downloads:

- **The app alone** (about 30 MB). QuickAI offers a build of llama.cpp that suits your graphics card and a model to
  download, and the speech model the first time you use the microphone.
- **With llama.cpp, Bonsai 2 4B and the speech model** (the name ends in `-with-llamacpp-bonsai2-4b`, about 1 GB).
  It runs as it is: nothing to download first.

The Linux packages run on glibc 2.34 and newer (Ubuntu 22.04 and later); the Windows packages on Windows 10 and 11;
the Mac app on macOS 13.3 (Ventura) and later, with Metal on Apple silicon.

On a Mac, drag QuickAI to Applications. Until the app is notarized by Apple, macOS stops it the first time it is
opened: choose **Done**, then press **Open Anyway** in **System Settings → Privacy & Security**. It asks only once.

## Quick start

1. **Start QuickAI.** With the bundled package, Bonsai 2 4B is already in the list of models. With the app alone,
   press **Install llama.cpp** on the first page, then add a model with the **+** beside **MODELS**: its suggestions
   suit this computer.
2. **Press Start** in the bar over the page (or Ctrl+Enter). The bar says *Running* when the model is ready.
3. **Chat** in the box at the bottom of the page. While the model runs, the panel at the right becomes **Connect an
   app**, with the address for another app, and **Open Chat UI** opens a chat in your browser. **Stop**, in the same
   bar, stops it.

To get more models, press the **+** beside **MODELS**, then **Add a model…** (Ctrl+K). With its box empty it suggests
models that suit this computer; type words to search online, paste a link to download a file, or press the folder to
add a `.gguf` you already have. **Recommended**, at the foot of the sidebar, has the whole curated list, by kind and
size. To use another model, choose it in the list and press **Start**; a right-click on it has **Details…**, **Show in
folder** and **Delete…**, and its row says its size, whether it should be fast here, and whether it is the one
recommended for this computer. Add a model, a model's details, Help and About are windows of their own: move them
anywhere, and make them bigger as you would any other window.

The panel at the right says how the model runs: **Light**, **Balanced** or **Long** for how much of a chat it keeps in
mind (**Other…** has more, up to 2M where the model can take it), on the graphics card, the processor, or both, and the
tools it may use. The button at its top puts it away, and the one at the right end of the bar brings it back.

The engine is chosen under **This computer**, at the top of the sidebar. An engine that is not installed yet has an
**Install** button on its page; pulling a model starts Ollama by itself. Help (F1: pages with pictures of the window,
a button that shows where each thing is, and a short tour), About and the light and dark theme are at the foot of the
sidebar, and Settings (Ctrl+,) is in the **QuickAI** menu at its top.

## Chat

The page under the bar is a chat with the model that runs: nothing to install, no browser. Replies come as they are
written, with how fast (tokens a second). The paper clip sends a picture to a model that sees them, or puts a text file
in the message, and the wrench beside it has the tools of your MCP servers. A reply can be copied or asked for again,
and the last message changed and sent again. The **⋯** at the right end of the bar has **Keep chats**, **Options…**
(instructions for the model, how free its answers are and how long they may be), **Hands-free** and **Export this
chat…**, which saves it as Markdown.

Nothing is kept unless you turn on **Keep chats** in that menu: then each conversation is a file on this computer.

Keyboard: Ctrl+Enter starts or stops the model, Ctrl+K adds a model, Ctrl+L goes to the chat's box, Ctrl+, opens
Settings and F1 opens Help. With voice on, Ctrl+Space talks while it is held and Ctrl+Shift+Space starts or ends
hands-free.

## Talk to it (experimental)

Voice is new, and off until you turn on **Use voice** in **Settings → Voice**: until then QuickAI shows no microphone
and never opens it. Then the microphone in the chat's box writes down what you say: press it and talk, or hold
**Ctrl+Space**. Your words go in the box, to change before you send them. **Read aloud** under an answer reads it in
your computer's own voice, lighting the sentence it is reading. **Hands-free** (in the **⋯**, or Ctrl+Shift+Space)
listens, sends what you said when you pause, reads the answer and listens again; **Esc** ends it.

It all happens on your computer: [quickai-whisper](https://github.com/everybodyai/quickai-whisper), built on
[whisper.cpp](https://github.com/ggml-org/whisper.cpp), writes your words down on the processor, and your system's own
voices read (on Linux, install `espeak-ng`). The speech model (82 MB) comes with the bundled packages and is downloaded
the first time otherwise. **Settings → Voice** has the language you speak, larger speech models, the voice and its
speed, and reading every answer aloud, which is off until you turn it on.

## Give your model tools

**MCP servers**, at the foot of the sidebar, adds servers of the Model Context Protocol, which let a model read files,
fetch web pages and more. Add one by its command or address, or paste the JSON from its instructions. QuickAI runs them
on this computer and offers their tools in its chat (the wrench in the chat's box), with llama.cpp or Ollama, and with
a model on another computer too; a card asks you before each call (**Allow once**, **Always allow**, **Deny**).
llama.cpp's own chat page has them as well.
[docs/mcp-servers.md](docs/mcp-servers.md) has a worked example, servers to try, and what to do when one does not start.

## Run models on another computer

A desktop with a big graphics card, or a rented GPU server, can run the models while you use them from here:
click **This computer** at the top of the sidebar, choose **Add a computer…**, and give its SSH address (the
hosts of your `~/.ssh/config` are offered). QuickAI tests it step by step and says what to do about anything that fails;
then choose it in the same menu. The list then shows the models that computer keeps; **Start** runs the one you pick
there, and the chat and **Connect an app** work as they do on this computer.

QuickAI connects with your own `ssh` (your keys, your agent and `~/.ssh/config` are used as they are) and opens no
port to the network on either side: the engine on that computer listens on its own loopback, and an SSH tunnel brings
its port here, as the same port unless you give another (**Port here**, under the computer's **More options**). On that
computer, run Ollama (or ComfyUI) as you would there, or `quickai-run up` with a configuration.

## Use it from other apps

While a model runs, QuickAI serves an OpenAI-compatible API on your machine (**Connect an app**, in the panel at the
right, shows the details):

```bash
curl http://127.0.0.1:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model": "local", "messages": [{"role": "user", "content": "Hello!"}]}'
```

Any client that takes a custom OpenAI base URL works: set it to `http://127.0.0.1:8080/v1`, with any API key.

## Engines

| Engine | For | How you get it |
| --- | --- | --- |
| **llama.cpp** | Chat, code, vision, embeddings: any GGUF model | Built in |
| **Ollama** | The Ollama model library | **Ollama** in the engine menu under **This computer**, then **Install Ollama**; downloaded and checksummed on request |
| **ComfyUI** | Images and video from workflows (Wan 2.2, LTX-Video and more) | **ComfyUI** in the engine menu under **This computer**, then **Install ComfyUI**; installed from source into its own environment, with ComfyUI-Manager unless you untick it |
| **Open WebUI** | A full chat page for Ollama | Settings → Open WebUI, with Ollama chosen; offline by default |

Every engine QuickAI installs lives under `~/.quickai`, starts on `127.0.0.1` only, and has its own calls home
switched off. Ollama, for example, runs with `OLLAMA_NO_CLOUD=1`.

## On a server

`quickai-run` is QuickAI without the window, for a rented GPU server. It takes a configuration exported from the
app, installs what it needs, fetches the models, and starts it:

```bash
quickai-run init  vision-chat.toml   # write the file of what this configuration needs from you
quickai-run check vision-chat.toml   # show what would be done, from where, and how big; change nothing
quickai-run up    vision-chat.toml   # do it, start it, stay until Ctrl-C
```

It listens on the loopback only; reach it through an SSH tunnel. See [docs/headless.md](docs/headless.md).

## Privacy

- Prompts and replies go only between your browser or app and the local server. Neither QuickAI nor its
  developer ever sees them.
- Voice is off until you turn it on. Then what you say into the microphone is written down on this computer, and
  the recording is not kept.
- No accounts, no automatic update checks and no telemetry. When something breaks (QuickAI crashes, an engine stops
  by itself, the window stops answering), QuickAI shows a report that names no one, and one click opens QuickAI's site
  or a GitHub issue with it filled in, for you to read and send if you want. See
  [PRIVACY.md](PRIVACY.md#problem-reports).
- The internet is used only when you search for or download a model or an engine, and the app says so.
- If you install ComfyUI with its Manager (the box is ticked by default), the Manager fetches its lists of nodes and
  models from GitHub and the Comfy Registry whenever ComfyUI starts. A custom node it installs is another person's
  program.
- What QuickAI keeps (settings, configurations, downloaded models) stays in your own folders.

The full policy is [PRIVACY.md](PRIVACY.md).

## Build from source

```bash
git clone https://github.com/everybodyai/quickai
cd quickai
cargo run --release -p quickai
```

To run a model you also need a llama.cpp build, and for the microphone the speech program from
[quickai-whisper's releases](https://github.com/everybodyai/quickai-whisper/releases).
[docs/development.md](docs/development.md) covers the setup, the layout of the code, and the checks CI runs.

## License

QuickAI is open source under the [MIT License](LICENSE): free to use for anything, at home or at work, and free
to change and share. Organizations that want help deploying it can get paid help and support from EverybodyAI:
see [everybodyai.dev/organizations](https://everybodyai.dev/organizations).

The components QuickAI bundles or downloads keep their own licenses ([THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md)),
and each model you download comes with its own license, too.

## Contributing

Bug reports, ideas and pull requests are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) first.
Contributions are accepted under the MIT License, the same as the code, and everyone is asked to follow the
[Code of Conduct](CODE_OF_CONDUCT.md). Report security problems privately, as described in [SECURITY.md](SECURITY.md).

---

<p align="center">
  QuickAI is made by <a href="https://everybodyai.dev"><strong>EverybodyAI</strong></a>: tools that make AI usable by everybody, privately.
</p>
