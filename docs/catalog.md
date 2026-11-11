# The recommended models

This is the first part of the architecture's `catalog` document kind ([architecture.md](architecture.md), sections 5.1 and 6.4) that the
application uses: the models it recommends, shipped as documents, together with a window for finding and downloading them. The recipe side of
the catalog (components chosen by hand, adapters, LoRA) remains part of Phase 4. Where this document and the generated reference page
([reference/catalog-fields.md](reference/catalog-fields.md)) disagree on a key, the reference page takes precedence.

## 1. What the user sees

**Models → Recommended** (and *See all recommended models* below the suggestions of **Add a model**, which are the list's starter models of
the kinds the chat can work with, in the size that fits this computer) is available for llama.cpp, Ollama and ComfyUI. It opens a window that
starts from **what the user wants to do** (work package 4.34). Several tasks can be chosen at once, and a combination that no model covers may
show nothing:

```
Recommended models                                                                        x
What do you want to do?
 [Chat] [Write code] [Solve problems] [Read pictures] [Read scans] [Translate] [Transcribe] [Search my files]
Best for this computer (30.6 GB of memory)
 +- RECOMMENDED ------------+  +- SMALLER, FASTER --------+  +- SMALLEST ----------------+
 | Qwen3.5 9B               |  | Qwen3.5 4B               |  | LFM2.5 1.2B Instruct      |
 | (eye) (bulb) (tools) (globe) |  | ...                  |  | (bolt)                    |
 | The all-rounder, one size up. | The all-rounder to start with. | Tiny and fast: try it first. |
 | [Download · 6.1 GB] Other sizes | [3.2 GB] Other sizes  |  | v Installed               |
 +--------------------------+  +--------------------------+  +---------------------------+
 Also good:  Gemma 4 E4B  Reads text, pictures and audio.                 (eye)(ear)(globe) [5.7 GB] Other sizes
 All 53 models that do this
[ Or search all 77 models                                              ] [Browse all 77]
Nothing is downloaded until you click. The list was checked on 2026-10-30.
```

- **The tasks come from the vocabulary** (`[[task]]` in `vocab/core.toml`: Chat, Write code, Solve problems, Read pictures, Read scans,
  Translate, Transcribe, Search my files, Make pictures, Make videos). Each is a button with an icon; a task that no model of the engine
  performs is not shown (ComfyUI's window offers only *Make videos*). The first task is selected when the window opens. **Several tasks can be
  selected**: the models shown must perform all of them, and when no model does, the page reads *No model in the list does all of these.
  Choose fewer.*
- **A model performs a task** when the task's rule applies to it (its type is one of the task's types, it accepts the input the task
  requires, or it has a feature the task names: Solve problems covers the reasoning models and those that think), or when the list says so
  (`tasks`: Qwen3-VL reads scans well, Granite Speech translates speech). A task's first type is its primary one: picks are drawn from models
  of that type whenever there are any, so that Chat shows assistants rather than code models that can also chat
  (`quickai_core::lookup::picks`).
- **The picks are tailored to this computer**: the largest starter model (`start`) that fits with room to spare (half the memory, as
  everywhere else: `fit`), a smaller one of about half its size, and the smallest; the remaining starter models that perform the task follow
  under *Also good*. When nothing fits with room to spare, the smallest model is shown with a note that it exceeds what the memory holds
  comfortably.
- **One button per model**: the file the list recommends (`best`), together with its projector when it has one, as a single download.
  *Other sizes* lists the remaining files in plain terms: *Standard* (4-bit), *Better quality* (5-bit), *High quality* (6- and 8-bit),
  *Full quality* (16-bit), and *text only* for the weights of a model that accepts more input with its projector; the file's own name appears
  on hover.
- **Less to read**: a one-line summary below each name (`tagline`) and small icons for what a model can do (read images, hear audio, think,
  call tools, handle many languages, run fast; each is described on hover). The maker, license, size and context length are shown in the list.
- **The full list is one click away**: *All N models that do this* opens the list filtered to those models (a line says so, with *Show all*);
  *Browse all* opens it unfiltered; typing in the search box opens it with the search terms in the same box and keyboard focus retained. The
  list has a back control at the top.

**Browse all** is the list as it was before the task page was introduced:

```
Recommended models                                                                        x
< What do you want to do?
73 models that run with llama.cpp. The list was checked on 2026-10-30. Nothing is downloaded until you click a size.
This computer has 31.0 GB of memory: sizes that would not fit are greyed.
Search: [ a name, a maker, or what you need: image, code, thinking...        ] Size: Any  Small (< 4 GB)  Medium (4-15 GB)  Large (15 GB+)
 * Start here (14)  | Qwen3.5 4B   Alibaba (Qwen) · 4.2B · Apache-2.0 · 262.1K context    (v Installed)
 All (78)           | thinking · tools · multilingual · sees images
                    | The all-rounder to start with. Qwen 3.5 understands 201 languages and dialects, can think ...
 Chat (27)          | Get with projector:  [Q4_K_M · 3.3 GB]  [Q8_0 · 5.1 GB]
 Reasoning (9)      | Get model only:      [Q4_K_M · 2.6 GB]  [Q8_0 · 4.2 GB]
 Code (7)           | ...
 Vision (9)  ...
```

- **No website is contacted** to open, search or close the list: the list is built into the application. Clicking a size is the first
  request, and it works exactly as the search results' buttons already did.
- **Start here** (the default view) is a short selection of one or two models for each need; **All** shows everything; each **category**
  shows its count (Chat, Reasoning, Code, Translation, Vision, Documents, Embedding, Audio, in vocabulary order). The counts follow the search
  terms and the size filter.
- **Search** matches words that must each appear somewhere in a model's description: its name, its maker, its purpose, its license, its
  size, its inputs (`image`, `audio`) and its features (`thinking`, `tools`, `multilingual`). A word ending in `s` also matches without it
  (`images` finds `image`). The search box has keyboard focus when the window opens, so typing begins immediately.
- **Size** uses the same bands as search: Small below 4 GiB, Medium below 15 GiB, Large above (the vocabulary's bands). It applies per
  button, as there.
- A model's card shows its maker, its size, its license and the context length it was trained for; chips indicate what it accepts
  (`sees images`, `hears audio`) and other noteworthy features. The note describes what it is good for, based on the model's own card or on
  plain fact.
- **A model that needs a second file offers both options, and the choice is the user's.** The first row, *Get with projector*, downloads the
  model and its projector as **one download**; the second, *Get model only*, downloads the weights alone. Hovering over each row explains what
  the extra file is for and what the model cannot do without it. Nothing is chosen on the user's behalf: a model that reads images is listed
  with its projector so that both files take one click, and nothing prevents a user from downloading the text-only weights.
- **Installed** appears beside a model whose button's files are all present in the models folders (or, for Ollama, whose name the daemon
  lists), and that button is greyed out. A size larger than this computer's memory is also greyed out, with an explanation on hover (the hint
  considers only total system memory, not graphics memory: `fit` in `hardware.rs`).
- **The list is a single column and makes clear that it continues.** Sections and cards are stacked vertically; each note is shown in full,
  on as many lines as it needs (a note truncated with an ellipsis, with the remainder in a tooltip, left users searching for the rest); the
  scroll bar is always visible, beside the text rather than over it; and switching to another page, other search terms or another size, or
  reopening the window, starts at the top of the list (the toolkit preserves the scroll position, and a page that opened at its end appeared
  to contain only a few models). The search box spans the width of its row, so its placeholder text is fully visible.
- Escape closes the window. A download shows its progress bar over the window and in the log, like any download. **A failed download is
  reported in the window**, in red at the top (the log line with the same message sits beneath the window, which covers most of the
  application), until the next click on a size.

A downloaded model is **listed under the name the list gives it** in the model picker (`Qwen3.5 4B (Q4_K_M)` rather than a file name),
together with its note, and is started **with the options it requires**: an embedding model with `--embeddings` (without it, llama-server
answers `/v1/embeddings` with 501; L18 of [engine-facts.md](engine-facts.md)), a reranker with `--reranking`, and a vision model with
`--mmproj` and its file.

## 2. The list

There is one document per category, in `crates/quickai-runtime/manifests/catalog/` (compiled into the application). The documents are
**generated** by a tool the maintainers run (section 5), so that every commit, size, SHA-256 and pull size is read from the actual site rather
than typed by hand.

| Page | Models | Contents |
|---|---|---|
| Chat | 22 | Assistants from 230 MB to 13 GB: LFM2.5 (350M, 1.2B, 8B-A1B), Qwen3.5 (0.8B to 9B), Gemma 4 (E2B, E4B, 12B, 26B A4B), Granite 4.2, Ministral 3, MiniCPM5, SmolLM3, Olmo 3, Nemotron 3.5 Bolt E4B, and Bonsai 2 4B, the ternary model that ships with QuickAI and starts with thinking turned off |
| Reasoning | 12 | Models that think before they answer: LFM2.5 (1.2B Thinking, 2.6B, 24B-A2B), Nanbeige4.2 3B, K2 Horizon (3.7B, 7B), Ling 3.0 Tiny, Ministral 3 Reasoning, Ornith 1.5 9B, gpt-oss 20B, Qwen3.8 27B (the one large model, for 32 GB) |
| Code | 7 | Qwen2.5-Coder (0.5B, 1.5B, 7B: completion as you type), MiMo-V2.6 Distill 9B, Mellum2.1 Thinking, Devstral Small 2, Qwen3.6 35B-A3B (large, for 32 GB) |
| Vision | 6 | LFM2.5-VL (450M, 1.6B, 3B), MiniCPM-V 4.7 (1B, and 35B-A3B, large), Qwen3-VL 8B. Qwen3.5 and Gemma 4 on the Chat page also read images, and the *Read pictures* task finds them |
| Documents | 7 | OCR and document models: Granite Docling, MiniCPM-OCR, OvisOCR2, LightOnOCR-2, PaddleOCR-VL, GLM-OCR, Granite Vision |
| Audio | 5 | Models that listen: Qwen3-ASR (0.6B, 1.7B), Granite Speech 5.0, Voxtral Mini, Ultravox (Gemma 4 E2B and E4B on the Chat page also accept audio) |
| Translation | 6 | Hy-MT2 (1.8B, 7B), Index-Translate (2B, 9B) and TranslateGemma (4B, 12B; Ollama only) |
| Embedding | 11 | EmbeddingGemma (300M, 2), BGE small, Nomic Embed v1.5, LFM2.5 Embedding, Qwen3 Embedding (0.6B, 4B, 8B), Harrier OSS v1 0.6B, all-MiniLM, and a reranker (Qwen3 Reranker 0.6B) |

Including Video's (section 3 of [comfyui.md](comfyui.md)), there are nine documents with 77 models. 73 are offered for llama.cpp (as files
from Hugging Face) and 37 for Ollama (as a name to pull), 34 for both. By the size of their smallest button, 49 are small, 25 medium and 3
large: the list focuses on small and medium models, with one large model on the pages where large models are clearly better (Reasoning, Code,
Vision). The 17 models shown under "Start here" cover each need with one or two models.

**How the list was refreshed on 2026-10-30.** Each page's models were compared with releases since the previous list, using independent
results where available (Artificial Analysis's evaluation of small models run with llama.cpp, 2026-08-24, for Reasoning; the Open ASR
Leaderboard for Audio; MTEB for Embedding; OmniDocBench v1.6 for Documents) and the makers' own model cards otherwise. A model was removed when
a newer model of its size performed clearly better (Llama 3.2 and 3.1, Phi-4-mini, Falcon-H1, Qwen3 2507, Mistral Small 3.2, Qwen3 4B
Thinking, Phi-4-mini Reasoning, Nemotron 3 Nano, DeepSeek-R1-0528 (Qwen3 8B), Olmo 3 Think, Magistral Small, Seed-Coder, Qwen2.5-Coder 14B,
Qwen3-Coder 30B-A3B, Devstral Small 2507, SmolVLM2, MiniCPM-V 4.6, Qwen3-VL 2B and 4B, HunyuanOCR, DeepSeek-OCR, LightOnOCR 1B, BGE-M3,
mxbai-embed-large, Nomic Embed v2 MoE, Granite Speech 4.1). A model whose results are reported only by its maker was added alongside the
model it may replace rather than in its place (Index-Translate alongside Hy-MT2). Left out for now: LFM3 and NVIDIA's Dory (architectures
that llama.cpp b11889 does not support), Jina v5 embeddings (CC BY-NC 4.0), and models whose only GGUF files come from little-known uploaders.

**Requirements for inclusion** (enforced by tests, J08): a maker, a license and a note; a commit and a SHA-256 for every file; for every
model that reads images or listens, a projector in the list and the bundle that downloads it; a mode for embedding models; an architecture
supported by the llama.cpp release the packages ship (the tool checks that release's source table); a repository that requires no login; and
no sharded files. **A note states** what the model's card says, or a plain fact; it makes no claims of being the best.

## 3. The documents

```toml
schema = 1
kind = "catalog"
id = "chat"                       # a page; the application ships nine, a person adds their own
title = "Chat"
verified = "2026-10-07"           # when its facts were checked against the sites

[[model]]
id = "qwen3.5-4b"                 # unique across every page
name = "Qwen3.5 4B"
by = "Alibaba (Qwen)"
type = "chat"                     # an id of the vocabulary's [[model_type]]: chat reasoning code translation vision documents embedding audio
note = "The all-rounder to start with. ..."
tagline = "The all-rounder to start with."  # the line under its name on the first page (a warning beyond 80 characters)
license = "Apache-2.0"
params = "4.2B"
context = 262144
input = ["text", "image"]         # what it reads: ids of the vocabulary's [[modality]]
features = ["thinking", "tools", "multilingual"]
# tasks = ["scans"]               # ids of the vocabulary's [[task]] it does besides the ones the tasks' rules give it
start = true                      # shown by "Start here", and picked first on the first page

[model.run.llamacpp]              # how an engine gets and runs it, by the engine's id
repo = "unsloth/Qwen3.5-4B-GGUF"
rev = "e87f17640a7e..."           # the commit the files are fetched at
files = [
  { file = "Qwen3.5-4B-Q4_K_M.gguf", size = 2740000000, sha256 = "...", best = true },
  { file = "Qwen3.5-4B-Q8_0.gguf",   size = 4482000000, sha256 = "..." },
]
[model.run.llamacpp.companions]   # the files that go with the weights, by the kind of component they are
mmproj = { file = "mmproj-F16.gguf", size = 671800000, sha256 = "..." }

[model.run.ollama]                # an engine that keeps its own models pulls a name
ref = "qwen3.5:4b"
size = 3320000000                 # what the pull downloads, from the registry
```

- **The keys an engine's table accepts** depend on what the engine accepts (`[provides] accepts`): files (`repo`, `rev`, `files`,
  `companions`, `mode`) for llama.cpp, and a name (`ref`, `size`) for Ollama. Any other keys are errors.
- **Dependencies are declared as `companions`**: each is a kind of component the engine can attach (`[components.<kind>]`), and the engine must
  have a `[[models.bundle]]` that pairs weights with exactly those kinds, which is what produces the second row. The files are taken from the
  repository's pinned commit, stored the same way search results are (`<owner>--<repo>--<name>` for a companion), and used in the same way.
- **`mode`** is the id of one of the engine's `[[models.mode]]` entries: `embedding` (`--embeddings`) and `rerank` (`--reranking`) in
  llama.cpp's document. A catalog is pure data and never contains a command-line argument: the record beside the weights stores the id, and
  the engine's document defines what it stands for.
- **The hub** from which files are downloaded is the one named by the engine's own Hugging Face search
  (`[[models.lookup]] use = "huggingface_gguf"`, `site`), so a user who points that at a mirror points the list at it too, as does a test.
- **Merging**: pages merge by `id`, like every document. A user's document with the `id` of a shipped page patches it model by model
  (`[[model]]` entries by `id`), a new `id` creates a page of the user's own, and `hidden = true` removes a shipped model from the list. A
  model defined by two pages is rejected in the second; an error in a patch leaves the shipped page in effect, and the message says so.

## 4. Downloading, storing and using

- **Verified on arrival.** A file with a `sha256` is hashed as it is written to disk (avoiding a second read of a multi-gigabyte file) and
  compared at the end. A file whose hash or size does not match is not kept, not even as `.part`, and the log names the file and the expected
  value. If a projector fails, the weights are kept, since they run on their own, and no record is written. (Files from search results have no
  hash and are handled as before.)
- **One job, one record.** The bundled download is the same as the search's (`downloader::spawn_files`): files in order under a single
  progress bar, weights first, with the record `<weights>.bundle.json` written last. The record also stores the **mode** (by its id, and read
  back only if it is a plain id, like every name in it), and a model with a mode but no companion has a record too. Delete removes it together
  with the model.
- **An engine that is given no model** (ComfyUI, WP4.11, [comfyui.md](comfyui.md)) stores each file of a model in the subfolder of the models
  folder where ComfyUI looks for that kind of file (`[components.<kind>] place` in its document: `diffusion_models`, `text_encoders`, `vae`).
  The record states where each file is (`dir`); a button in the list is marked as installed only when every one of its files is in its
  folder at the size the list states; and Delete removes them all while keeping any file another model uses. Its page is `video`; a workflow
  refers to the files by their names on disk (`<owner>--<repo>--<name>` for all but the weights).
- **Start** (`launch::attach_record`, used by the Start button and by `quickai plan`) adds, from the record beside the selected weights, each
  companion that is still present and the engine's arguments for the mode, after the fields' arguments and before the user's own extra
  arguments, and records this in the plan (`mode embedding`). A projector that has been removed, or a mode the engine does not define, is
  reported in the log, and the model starts without it.

## 5. How the list is produced and kept accurate

The documents are written by a tool the maintainers run, not by hand. From a short selection for each model (which model, which
quantizations, and what to say about it), it reads the sites and writes the documents; it can also report what has changed since, without
writing anything.

Sources: **huggingface.co** (the repository's commit, its files with size and SHA-256, its license and architecture, and the first bytes of a
file when the API reports it as a projector), **registry.ollama.ai** (the manifest of a tag: a pull's size is the sum of its layers,
including drafts and projectors, which is why an Ollama size differs from a Hugging Face file size), and **GitHub** (the architectures in
`src/llama-arch.cpp` of the pinned llama.cpp release). The tool sends `User-Agent: quickai-catalog` and no personal data. It rejects anything
that cannot be offered: a repository that requires a login, an architecture the release does not support, a license it cannot find (unless
the selection states it), or a file without a hash. Licenses are recorded as their makers name them (`Apache-2.0`,
`Llama 3.2 Community License`, `LFM Open License v1.0`, `Gemma Terms of Use`).

After changing the list, run `cargo test -p quickai-core --test p3_catalog` (J08 enforces the list's requirements).

Real runs (section 7) and the checks of the opt-in tests (`p3_j22`, `p3_j23`) establish what "runs today" means: **a catalog entry does not
promise that a model runs on every computer** (a 5 GB model needs the memory), but the tool has verified that each entry is an existing
repository, at a pinned commit, with the stated hashes, and of an architecture the pinned llama.cpp supports.

## 6. Configuration

- **Add a model or a page**: create a document `~/.config/quickai/catalog/mine.toml` (Linux; `%APPDATA%\quickai\catalog` on Windows) with
  `kind = "catalog"`, its own `id` and `[[model]]` entries. `quickai config check` reports any errors.
- **Change or hide a shipped model**: create a document with the `id` of its page (`chat`, `code`, ...) and the model's `id`:
  `note = "..."` changes the note, and `hidden = true` removes the model.
- **Define your own mode**: an engine document (`[[models.mode]]`) adds one (`id`, `label`, `arg`); a catalog refers to it by its id.

## 7. Verification

All checks were run on 2026-10-07 (Linux, CPU only, the pinned llama.cpp b11007).

- **Every model, by the tool** (section 5): the repository exists at its commit and requires no login; each file has the size and SHA-256
  that the hub reports (the hash of the file in LFS); the license appears on the model card (or on the card of the model it was derived
  from); the architecture, read from the first bytes of the weights, is in the pinned release's table; each Ollama name has a manifest, and
  the size of a pull equals the sum of its layers.
- **The list against the sites, rechecked by hand** (`p3_j22`): the 78 llama.cpp runs and 50 Ollama runs (128 for 81 models) still have the
  commits, files, sizes and hashes the list states. The `video` page (WP4.11, one model, one run for ComfyUI) was checked against the hub
  in the same way on 2026-10-08, and its files were downloaded with the window's button, hashed again, and listed by a ComfyUI instance
  started with them.
- **The list as a document** (`p3_j08` and the rest of group J, `cargo test -p quickai-core --test p3_catalog` and the application's
  `app/engine.rs`): it loads without warnings; every category has at least three models; every pin is a commit; every model that reads
  images or listens has its projector and the bundle that downloads it; 8 to 16 models are marked "Start here", with one for each need; no
  more than a twentieth of the models are large; and exactly one model is marked as the one the packages ship.
- **The window and the effect of each click, by the GUI test suite, which the maintainers keep outside this repository**, against a fake
  Hugging Face and a fake `llama-server`: opening the window contacts no site; the categories and their counts; search terms; the two rows of
  a model with a projector; the bundled download as one job and the model alone as another; a file that does not match the list is rejected
  and not kept; `--embeddings` and `--mmproj` on the command line; a user's documents adding and hiding models; Escape; Ollama names pulled;
  a note shown in full; and every section placed below the previous one.
- **Real models, actually run** (the list's files downloaded from the hub, each verified against its hash, started by the pinned
  `llama-server` exactly as the application starts it, with `--mmproj` and the mode arguments, and given one request of the appropriate kind;
  context 4096, no GPU):

| Model | Kind | Asked | Result | Answer |
|---|---|---|---|---|
| Qwen3-ASR 0.6B | audio | what is said in the JFK sample | OK | `language English<asr_text>And so, my fellow Americans, ask not what...` |
| Ultravox 0.5 (Llama 3.2 1B) | audio | what is said in the JFK sample | OK | `I can transcribe the audio for you. The audio is a quote from a spe...` |
| Granite Speech 4.1 2B | audio | what is said in the JFK sample | OK | `and so, my fellow americans, ask not what your country can do for y...` |
| Voxtral Mini 3B | audio | what is said in the JFK sample | WEAK | `I'm unable to transcribe audio. However, I can help with text-to-sp...` |
| LFM2.5 350M | chat | the capital of France | OK | `Paris` |
| Gemma 4 E2B | chat | the capital of France | OK | `The capital of France is Paris.` |
| SmolLM3 3B | chat | the capital of France | OK | `The capital of France is Paris.` |
| Ministral 3 3B Instruct | chat | the capital of France | OK | `The capital of France is **Paris**.` |
| Falcon-H1 3B Instruct | chat | the capital of France | OK | `The capital of France is Paris.` |
| Qwen2.5-Coder 0.5B Instruct | code | a Fibonacci function in Python | OK | `'''python def fibonacci(n): if n <= 0: return None elif n == 1: ret...` |
| Granite Docling 258M | documents | the text of a picture (HELLO 2026) | OK | `<loc_0><loc_0><loc_500><loc_500><_SQL_>HELLO 2026` |
| GLM-OCR | documents | the text of a picture (HELLO 2026) | OK | `HELLO 2026` |
| LightOnOCR 1B | documents | the text of a picture (HELLO 2026) | OK | `HELLO 2026` |
| HunyuanOCR | documents | the text of a picture (HELLO 2026) | OK | `HELLO 2026` |
| DeepSeek-OCR | documents | the text of a picture (HELLO 2026) | OK | `Hello 2026` |
| EmbeddingGemma 300M | embedding | vectors for two sentences | OK | `2 vectors of 768` |
| Nomic Embed Text v1.5 | embedding | vectors for two sentences | OK | `2 vectors of 768` |
| Nomic Embed Text v2 MoE | embedding | vectors for two sentences | OK | `2 vectors of 768` |
| LFM2.5 Embedding 350M | embedding | vectors for two sentences | OK | `2 vectors of 1024` |
| Qwen3 Embedding 0.6B | embedding | vectors for two sentences | OK | `2 vectors of 1024` |
| Qwen3 Reranker 0.6B | embedding | passages ranked for a question | OK | `[(0, 0.998), (2, 0.0), (1, 0.0)]` |
| LFM2.5 1.2B Thinking | reasoning | the capital of France (thinking allowed) | OK | `<think> Okay, let's see. The user wants a one short sentence answer...` |
| Nemotron 3 Nano 4B | reasoning | the capital of France (thinking allowed) | OK | `The capital of France is Paris.` |
| Hy-MT2 1.8B | translation | a sentence into French | OK | `Le temps est agréable aujourd'hui.` |
| SmolVLM2 500M | vision | the colour of a picture of a red circle | OK | `The flag of Japan is red, white, and blue.` |
| MiniCPM-V 4.6 | vision | the colour of a picture of a red circle | OK | `Red` |
| Qwen3-VL 2B Instruct | vision | the colour of a picture of a red circle | OK | `red` |
| BGE Small English v1.5 | embedding | vectors for two sentences | OK | `vectors of 384` |
| LFM2.5-VL 450M | vision | the colour of a picture of a red circle | OK | `Red` |

28 of the 29 models produced what was asked (SmolVLM2 answered with a sentence, "The flag of Japan is red, white, and blue": it took a red
circle on white for a flag, and the sentence contains "red"). The one exception failed only the first phrasing, and a recording can be
queried in more than one way: Voxtral Mini answers "I'm unable to transcribe audio" to *Transcribe this audio.* but transcribes the sentence
exactly when asked *Write down the words that are spoken, exactly.* (engine-facts L23), which its note now states.
- **By the application's own code, through the window's button** (`p3_j23`, run by hand, about 2.3 GB): the smallest model of each of five
  kinds (a chat model, an embedding model, a reranker, an image model with its projector, and a speech model with its projector) is
  downloaded with the button of its row; the weights appear in the picker; the Start button starts the model; the command line contains
  `--embeddings`, `--reranking` or `--mmproj` as the kind requires; and the real server answers: "Paris" (LFM2.5 350M), vectors of 384
  numbers (bge-small-en-v1.5), 0.998 for the passage about Paris (Qwen3 Reranker 0.6B), "Red" for a picture of a red square (LFM2.5-VL 450M),
  and "...ask not what your country can do for you; ask what you can do for your country" (Qwen3-ASR 0.6B). The run took 11 minutes, mostly
  for the downloads.

**What the checks missed, and a user found.** The first version of the window laid the list out incorrectly. A scroll area adopts the layout
of the row that contains it, and the row holding the sidebar and the list runs left to right, so every section after the first (and, on a
category page, every card after the first) was drawn beside the previous one, beyond the right edge of the window, where a list that scrolls
only vertically cannot reach it; in addition, each note was drawn on a single line and truncated with an ellipsis. "All" showed only the chat
models. None of this was detected because the checks read what the window contains (the sidebar counts, the first card, a search that left one
model), and the application's tests rendered the window without checking where anything was placed; a user's screenshot of the scrolled list
revealed it. `p3_j24` (every card of the shipped list is complete, for both engines), `p3_j24b` (the window in three views has nothing cut off
or outside it; this test fails on the old layout in all three) and `p3_j24c` (a new page starts at its top) now cover it in the
application's tests, and two cases of the GUI tests cover it from outside (both fail on a build with the old layout).

**What this does not show:** that every one of the 81 models loads and answers. The real runs above cover the models that were inexpensive to
download or whose family could fail in ways the metadata cannot reveal (a projector, a mode, a new architecture, a different chat template);
the other 52 are covered by the checks in the first two points and by a model of their family having run (a Qwen3.5 0.8B with its projector
was also run earlier and read an image: L19). The Ollama names were checked against the registry but not pulled (pulling is what the Ollama
search already does, and a GUI test case does so for real). The runs were CPU-only and Linux-only.

## 8. Not yet implemented

- Large models (the "giants"): one is included (Qwen3-Coder 30B-A3B); a dedicated page for them will come later, and the size filter, the fit
  hint and the pins already support them.
- Sharded files (`-00001-of-00004`): excluded, as in search, so a model whose repository contains only shards (the official Qwen2.5-Coder 7B
  and 14B) is taken from another repository.
- Text-to-speech and speech-to-speech: llama.cpp's server has no endpoint that produces speech, so no speech-only model is listed. Whisper
  models are for whisper.cpp, not for llama-server.
- Hugging Face models as `hf.co/...` pulls in Ollama's window, and models from an Ollama library other than Ollama's own, are not offered;
  Ollama entries are names from its library.
- A model in a size the list does not include, or a quantization it does not name, is available through **Look up online**.
- Windows and macOS: the code is the same and the window uses egui; only Linux was tested here.
