# QuickAI

Start/stop local GGUF language models with a simple GUI. Comes with **Bonsai
8B** and **Bonsai 1.7B** (both 1-bit quantized, Apache-2.0) already bundled
in `models/` — just run the app and click **Start**. The app picks a
recommended one for you based on this machine's RAM (a green highlight in
the picker marks both as the safe, always-available choices); switch to the
1.7B by hand if the 8B feels slow.

## Running it

- **Linux (tar.gz)**: extract, then run `./quickai` from inside the
  extracted folder (it looks for `llama-server` and `models/` next to
  itself).
- **AppImage**: `chmod +x` the `.AppImage` file, then run it directly.
- **Windows (zip)**: extract, then double-click `quickai.exe`.

## GPU acceleration

The bundled `llama-server` uses the **Vulkan** backend, which works on AMD,
NVIDIA, and Intel GPUs alike (no CUDA/ROCm toolkit install required) as long
as your normal graphics driver is installed — Vulkan support ships with it
on every current desktop GPU driver. Set "GPU layers" to `0` in the app to
force CPU-only if you ever need to compare or don't have a Vulkan-capable
GPU.

## Interface

Top-right of the window: a theme toggle (☀/🌙), a speed icon
(🐢 slower / 🐂 medium / 🐎 fast) estimating how the currently selected
model will run on this machine — a rough guide, not a live benchmark — a
**⚙** settings button, and an **ℹ** about button (usage, disclaimer,
license). Most controls have hover tooltips explaining what they do.

## Adding more models

- Drop any other `.gguf` file into the `models/` folder next to the app
  and click **Refresh** in the GUI.
- Or paste a direct download link — or a local file path to a `.gguf` you
  already have (Linux or Windows style) — into the "Model URL / Path"
  field and click **Download**/**Import**. Either way it lands in
  `~/.quickai/models` (not the `models/` folder next to the app, which may
  be read-only, e.g. inside an AppImage) and is selected automatically
  once done.
- Or click **Look up online** to search Hugging Face for currently
  popular GGUF models, plus High/Medium/Small size sections, and download
  one with a click — it lands in `~/.quickai/models` too.

Either way, the GUI lists models from both folders together, so downloaded
models show up right alongside the bundled ones.

## Settings

Last-used model/port/context-size/GPU-layers are remembered between runs
(stored in your OS's per-user config directory, not inside this folder).

Click **⚙** next to the speed icon for more:

- **Downloaded models folder** — where models you fetch end up (see
  "Adding more models" above); change and **Apply**, or **Reset to
  default**.
- **Advanced: llama.cpp** — override the llama-server binary, or add extra
  command-line arguments (e.g. `--threads 8`) appended to every launch.
  Hover the field for example flags. An unsupported flag will make the
  server fail to start; check the log pane at the bottom of the window.

## License

Click **ℹ** for a usage/disclaimer summary and license info. In short:
QuickAI itself is licensed under the PolyForm Noncommercial License 1.0.0; the bundled `llama-server` (from llama.cpp)
is MIT; the bundled Bonsai model weights (from PrismML) are Apache-2.0,
unmodified. Full texts are in `LICENSE` and `THIRD_PARTY_LICENSES.md` next
to this app.
