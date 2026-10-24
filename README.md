<p align="center">
  <img src="assets/logo.svg" width="96" height="96" alt="QuickAI">
</p>

<h1 align="center">QuickAI</h1>

<p align="center">
  <strong>Run AI models on your own computer. Private by default.</strong>
</p>

<p align="center">
  <a href="https://github.com/nihal-kader/quickai/actions/workflows/ci.yml"><img src="https://github.com/nihal-kader/quickai/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-PolyForm%20Noncommercial-246E37" alt="License: PolyForm Noncommercial 1.0.0"></a>
  <img src="https://img.shields.io/badge/platforms-Linux%20%7C%20Windows-0E1611" alt="Platforms: Linux and Windows">
</p>

<p align="center">
  <a href="https://everybodyai.dev/quickai">Website</a> ·
  <a href="#install">Install</a> ·
  <a href="#quick-start">Quick start</a> ·
  <a href="docs/README.md">Docs</a> ·
  <a href="COMMERCIAL.md">Commercial use</a>
</p>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/readme/quickai-dark.png">
    <img src="assets/readme/quickai-light.png" width="760" alt="The QuickAI window running the Bonsai 8B model: a green Running pill, the model card, and the Open Chat UI button">
  </picture>
</p>

QuickAI is a small desktop app that downloads, runs and manages AI models on the hardware you already have:
chat, code, vision, speech, documents, images and video. There's no account, no cloud and no telemetry, and your
prompts never leave your computer.

## Why QuickAI

- **Private by default.** The models run on your computer, and the servers QuickAI starts listen on `127.0.0.1`
  only. No telemetry, no accounts, no update checks. It uses the internet only when you click something that
  needs it. [PRIVACY.md](PRIVACY.md) lists every host it can contact, and why.
- **Works out of the box.** It ships with llama.cpp and two 1-bit Bonsai models (1.1 GB and 0.25 GB), so your
  first chat needs no download at all.
- **Runs on ordinary hardware.** GPU acceleration through Vulkan on AMD, NVIDIA and Intel. QuickAI suggests a
  model that fits your memory and estimates how fast it will run before you start it.
- **80+ recommended models.** A curated list for chat, reasoning, code, vision, document reading (OCR), speech,
  translation and search. Every file is checked against its SHA-256 as it arrives.
- **Three engines.** llama.cpp is built in. [Ollama](https://ollama.com) and [ComfyUI](https://www.comfy.org)
  (images and video) are installed only when you ask, each in a folder of its own.
- **Works with your other apps.** An OpenAI-compatible API on your own machine, for editors, scripts and chat
  clients. Turn on the **Live** tab to watch what each app asks and what the model answers, as it happens.
- **Configurations you can share.** Save a whole setup (engine, model, settings) as one small file and give it
  to someone else. Local paths and secrets are never exported.
- **Headless, too.** `quickai-run` starts a configuration on a server with no display.

## Install

> **QuickAI 0.1.0 is being prepared.** Linux (tar.gz and AppImage) and Windows (zip) builds will be on the
> [Releases](https://github.com/nihal-kader/quickai/releases) page, with packages for the AUR, Homebrew, Snap,
> Scoop, Chocolatey and the Microsoft Store to follow. To hear when it's out, sign up at
> [everybodyai.dev](https://everybodyai.dev/quickai). Until then, [build it from source](#build-from-source).

Each package bundles QuickAI, llama.cpp's `llama-server` and the two Bonsai models, and runs as it is: nothing
to install first. The Linux packages run on glibc 2.34 and newer (Ubuntu 22.04 and later); the Windows package
on Windows 10 and 11.

## Quick start

1. **Start QuickAI.** Bonsai 8B is already selected, or QuickAI's recommendation for your computer is.
2. **Press Start.** The pill at the top turns green and says *Running*.
3. **Press Open Chat UI** to chat in your browser, or **API** to connect another app.

To get more models, press **Recommended** for the curated list, **Look up online** to search Hugging Face, or
paste a link or a file path into the **Get** box.

## Use it from other apps

While a model runs, QuickAI serves an OpenAI-compatible API on your machine (the **API** button shows the
details):

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
| **Ollama** | The Ollama model library | ⚙ Settings → Engine; downloaded and checksummed on request |
| **ComfyUI** | Images and video from workflows (Wan 2.2, LTX-Video and more) | ⚙ Settings → Engine; installed from source into its own environment |
| **Open WebUI** | A full chat page for Ollama | ⚙ Settings → Chat UI; offline by default |

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
- No telemetry, analytics, crash reporting, accounts or automatic update checks.
- The internet is used only when you search for or download a model or an engine, and the app says so.
- What QuickAI keeps (settings, configurations, downloaded models) stays in your own folders.

The full policy is [PRIVACY.md](PRIVACY.md).

## Build from source

```bash
git clone https://github.com/nihal-kader/quickai
cd quickai
cargo run --release -p quickai
```

To run a model you also need a llama.cpp build. [docs/development.md](docs/development.md) covers the setup,
the layout of the code, and the checks CI runs.

## License

QuickAI is **free for personal and other noncommercial use** under the
[PolyForm Noncommercial License 1.0.0](LICENSE). That includes learning, hobby projects, research, charities,
schools and public institutions. **Using QuickAI in or for a business needs a commercial license.** See
[COMMERCIAL.md](COMMERCIAL.md).

The components QuickAI bundles or downloads keep their own licenses ([THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md)),
and each model you download comes with its own license, too.

## Contributing

Bug reports, ideas and pull requests are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) first.
Contributions are accepted under a [Contributor License Agreement](CLA.md), and everyone is asked to follow the
[Code of Conduct](CODE_OF_CONDUCT.md). Report security problems privately, as described in [SECURITY.md](SECURITY.md).

---

<p align="center">
  QuickAI is made by <a href="https://everybodyai.dev"><strong>EverybodyAI</strong></a>: tools that make AI usable by everybody, privately.
</p>
