# The recommended models

This is the first piece of the architecture's `catalog` document kind ([architecture.md](architecture.md), 5.1 and 6.4) that the application uses: the models
it recommends, as documents it ships, with a window to find one and get it. The recipe side of the catalog (components chosen by hand, adapters, LoRA) is
still Phase 4's. Where this document and the generated page ([reference/catalog-fields.md](reference/catalog-fields.md)) differ on a key, the page wins.

## 1. What a person sees

**Models → Recommended** (and *See all recommended models* under the suggestions of **Add a model**, which are the
list's own models to start with, of the kinds the chat can talk to, in the size that fits this computer), for
llama.cpp and for Ollama, opens a window:

```
Recommended models                                                                        x
78 models that run with llama.cpp. The list was checked on 2026-10-07. Nothing is downloaded until you click a size.
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

- **Nothing is asked of any site** to open it, search it or close it: the list is in the application. A click on a size is the first request, and it is
  what the look-up's buttons already were.
- **Start here** (the window opens on it) is a handful of models, one or two for each need; **All** is everything; each **category** has its count
  (Chat, Reasoning, Code, Translation, Vision, Documents, Embedding, Audio, in the order of the vocabulary). The counts follow the search and the size.
- **Search** takes words that must each be somewhere in what a model says of itself: its name, its maker, what it is for, its licence, its size, what it
  reads (`image`, `audio`) and its features (`thinking`, `tools`, `multilingual`). A word that ends in `s` is also found without it (`images` finds `image`).
  It has the keyboard when the window opens, so typing starts at once.
- **Size** is the look-up's: Small below 4 GiB, Medium below 15 GiB, Large above (the vocabulary's bands). It works on a button, as there.
- A model's card says who made it, how big it is, its licence and the context it was made for; chips say what it reads (`sees images`, `hears audio`) and
  what else is worth knowing. The note says what it is good for, and is what the model's card says or a plain fact.
- **A model that needs a second file has both choices, and the choice is the person's.** The first row, *Get with projector*, fetches the model and its
  projector as **one download**; the second, *Get model only*, fetches the weights alone. The words of each row say, when the pointer rests on them, what the
  extra file is for and what the model cannot do without it. Nothing is chosen for anybody: a model that reads pictures is listed with its projector so
  that fetching both is one click, and nothing stops a person who wants only its text.
- **Installed** appears beside a model with a button whose files are all in the models folders (or, for Ollama, whose name the daemon lists), and that
  button is greyed. A size that is more than this computer's memory is greyed too, and its hover says so (the hint knows only the total memory, not the
  graphics memory: `fit` in `hardware.rs`).
- **The list is one column, and it says that it goes on.** Sections and cards are one under the other; a note is shown whole, on as many lines as it takes (one
  cut short with an ellipsis and the rest in a hover left a person looking for the rest of it); the scroll bar is always there, beside the text and not on it;
  and another page, other words, another size, or the window opened again, starts at the top of its list (the toolkit keeps the scroll position, and a page
  that opened at its end looked as if it held a few models). The search box takes the width of its row, so its hint is whole.
- Escape closes the window. The download shows its bar over the window and in the log, as any download does. **A download that fails says so in the window**, in red at the top
  (the log that has the same line is under the window, which opens over most of the application's), until the next click on a size.

What was fetched is **named as the list names it** in the model picker (`Qwen3.5 4B (Q4_K_M)` and not a file name) with its note, and is started
**in the way it needs**: an embedding model with `--embeddings` (llama-server answers `/v1/embeddings` with 501 without it, L18 of
[engine-facts.md](engine-facts.md)), a reranker with `--reranking`, a vision model with `--mmproj` and its file.

## 2. The list

Eight documents, one for each category, in `crates/quickai-runtime/manifests/catalog/` (compiled into the application). They are **generated** by
`scripts/catalog.py` from picks (section 5), so that every commit, size, SHA-256 and pull size is read from the real site and none is typed.

| Page | Models | What is in it |
|---|---|---|
| Chat | 27 | Assistants from 230 MB to 14 GB: LFM2.5, Qwen3.5 (0.8B to 9B), Gemma 4 (E2B, E4B, 12B), Granite 4.2, Ministral 3, Llama 3.2 and 3.1, Phi-4-mini, SmolLM3, MiniCPM5, Falcon-H1, Olmo 3, Mistral Small 3.2, and Bonsai 2 4B, the ternary model that ships with QuickAI and starts with its thinking off |
| Reasoning | 10 | Models that think before they answer: LFM2.5 Thinking, Qwen3 4B Thinking, Phi-4-mini Reasoning, Nemotron 3 Nano, Ministral 3 Reasoning, DeepSeek-R1-0528 (Qwen3 8B), Olmo 3 Think, gpt-oss 20B, Magistral Small |
| Code | 7 | Qwen2.5-Coder (0.5B, 1.5B, 7B, 14B), Seed-Coder 8B, Devstral Small, Qwen3-Coder 30B-A3B (the one large model, for a big computer) |
| Vision | 9 | SmolVLM2, LFM2.5-VL (450M, 1.6B, 3B), MiniCPM-V 4.6, Qwen3-VL (2B, 4B, 8B) |
| Documents | 6 | OCR and document models: Granite Docling, GLM-OCR, LightOnOCR, HunyuanOCR, Granite Vision, DeepSeek-OCR |
| Audio | 5 | Models that listen: Qwen3-ASR (0.6B, 1.7B), Granite Speech, Voxtral Mini, Ultravox (Gemma 4 E2B and E4B on the Chat page hear too) |
| Translation | 4 | Hy-MT2 (1.8B, 7B) and TranslateGemma (4B, 12B; Ollama only) |
| Embedding | 12 | EmbeddingGemma, BGE (small, M3), Nomic Embed (v1.5, v2 MoE), mxbai-embed-large, LFM2.5 Embedding, Qwen3 Embedding (0.6B, 4B, 8B), all-MiniLM, and a reranker (Qwen3 Reranker 0.6B) |

77 are offered by llama.cpp (as files from Hugging Face) and 50 by Ollama (as a name to pull), 47 by both. By the size of their smallest button, 56 are small, 23 medium
and 1 large: the list is about small and medium models, as asked, and the large ones are for a later page. The 14 models "Start here" shows are one or two for each need.

**What a model must have to be in it** (checked by tests, J08): a maker, a licence and a note; a commit and a SHA-256 on every file; a projector in the list
for every model that reads pictures or listens, with the bundle that fetches it; its mode when it is an embedding model; an architecture that the llama.cpp
the packages ship reads (the tool checks the table of that release's source); a repository that needs no login; no shards. **What a note says**
is what the model's card says, or a plain fact; there are no claims of being the best.

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
license = "Apache-2.0"
params = "4.2B"
context = 262144
input = ["text", "image"]         # what it reads: ids of the vocabulary's [[modality]]
features = ["thinking", "tools", "multilingual"]
start = true                      # shown by "Start here"

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

- **Which keys an engine's table takes** follows what the engine accepts (`[provides] accepts`): files (`repo`, `rev`, `files`, `companions`, `mode`) for llama.cpp,
  a name (`ref`, `size`) for Ollama. The wrong ones are errors.
- **Dependencies are `companions`**: each is a kind of component the engine can attach (`[components.<kind>]`), and the engine must have a `[[models.bundle]]` that pairs
  weights with exactly those kinds, which is what makes the second row. The files come from the repository's commit, are kept as a look-up keeps them
  (`<owner>--<repo>--<name>` for a companion) and are used the way it uses them.
- **`mode`** is the id of one of the engine's `[[models.mode]]`: `embedding` (`--embeddings`) and `rerank` (`--reranking`) in llama.cpp's document. A catalog is data
  and never carries a command-line word: the record beside the weights keeps the id, and the engine's document says what it stands for.
- **The hub** files are fetched from is the one the engine's own look-up of Hugging Face names (`[[models.lookup]] use = "huggingface_gguf"`, `site`), so a person who points
  that at a mirror points the list at it too, and so does a test.
- **Merging**: pages merge by `id` like every document: a person's document with the `id` of a shipped page patches it model by model (`[[model]]` entries by `id`), a new `id` is a page of their own,
  `hidden = true` takes a shipped model out of the list. A model two pages write is refused in the second; a mistake in a patch leaves the shipped page in force and the message says so.

## 4. Fetching, keeping, using

- **Checked while it arrives.** A piece with a `sha256` is hashed on its way to the disk (no second read of a multi-gigabyte file) and compared at the end; a file that hashes to
  something else, or comes to another size, is not kept, not even as `.part`, and the log says which file and what was expected. A projector that fails leaves the
  weights, which run alone, and no record. (A look-up's pieces have no hash and are as they were.)
- **One job, one record.** The bundled download is the look-up's (`downloader::spawn_files`): files in order under one bar, the weights first, the record `<weights>.bundle.json` written last. The
  record now also carries the **mode** (by its id; read back only if it is a plain id, like every name in it), and a model with a mode and no companion has one too. Delete takes it with the model.
- **An engine that is given no model** (ComfyUI, WP4.11, [comfyui.md](comfyui.md)) keeps each file of a model in the folder of the models folder that ComfyUI looks for that kind in (`[components.<kind>] place` of its document: `diffusion_models`, `text_encoders`, `vae`); the record says where each is (`dir`), a button of the list is marked as installed only when every one of its files is in its folder at the size the list says, and Delete takes them all and keeps what another model uses. Its page is `video`; a workflow names the files as they are on disk (`<owner>--<repo>--<name>` for all but the weights).
- **Start** (`launch::attach_record`, used by the Start button and by `quickai plan`) adds, for the record beside the selected weights, each companion that is still there and the engine's words for the
  mode, after the fields' arguments and before the person's own extra ones, traced in the plan (`mode embedding`). A projector that has gone, or a mode the engine does not have, is said in the log and the model starts without it.

## 5. How the list is made and kept honest

`scripts/catalog.py` (Python 3.11, standard library only; in the packaging repository like the other scripts):

| Command | What it does |
|---|---|
| `build` | reads `scripts/catalog/picks/*.toml` (a pick says which model, which quantizations, what to say of it) and the sites, and writes the documents |
| `check` | does the same in memory and says what differs from the shipped documents: nothing is written; run it to see what has moved |
| `add OWNER/REPO` | prints a pick to start from: the commit, the architecture, the licence, the quantizations and projectors a repository has |

What it reads: **huggingface.co** (the repository's commit, its files with size and SHA-256, its licence and architecture, and the first bytes of a file when the API's answer is a projector's),
**registry.ollama.ai** (the manifest of a tag: a pull's size is the sum of its layers, drafts and projectors included, which is why an Ollama size is not a Hugging Face file's), and **GitHub** (the
architectures in `src/llama-arch.cpp` of the llama.cpp tag in `build/config.env`). It sends `User-Agent: quickai-catalog` and nothing of a person. It refuses what cannot be offered: a repository
that needs a login, an architecture the release does not read, a licence it cannot find (the pick says it), a file with no hash. Licences are written as their makers name them (`Apache-2.0`,
`Llama 3.2 Community License`, `LFM Open License v1.0`, `Gemma Terms of Use`).

**To add a model**: `scripts/catalog.py add OWNER/REPO`, write the pick (a note is a sentence from the card, or a fact), `scripts/catalog.py build`, then
`cargo test -p quickai-core --test p3_catalog` (J08 holds the list to what it must be). To refresh the list: `build`, read the diff.

Real runs (section 7) and the lists of checks of the opt-in tests (`p3_j22`, `p3_j23`) are how "runs today" is known: **a catalog entry is not a promise that a model runs on every
computer** (a 5 GB model needs memory), but each was found by the tool to be a repository that exists, at a commit, with the hashes it says, of an architecture the pinned llama.cpp reads.

## 6. Configuring it

- **Add a model or a page**: a document `~/.config/quickai/catalog/mine.toml` (Linux; `%APPDATA%\quickai\catalog` on Windows) with `kind = "catalog"`, its own `id` and `[[model]]` entries. `quickai config check` says what is wrong.
- **Change or hide a shipped one**: a document with the `id` of its page (`chat`, `code`, ...) and the model's `id`: `note = "..."` changes the note, `hidden = true` removes it.
- **A mode of your own**: an engine document (`[[models.mode]]`) adds one (`id`, `label`, `arg`); a catalog names it by its id.

## 7. What was checked

All on 2026-10-07, on this machine (Linux, CPU only, the pinned llama.cpp b11007).

- **Every model, by the tool** (section 5): the repository exists at its commit and needs no login; each file has the size and SHA-256 the hub reports (the hash of the file in LFS); the licence is on the card (or on
  the card of the model it was made from); the architecture, read from the first bytes of the weights, is in the table of the pinned release; an Ollama name has a manifest, and what a pull downloads is the sum of its layers.
  `scripts/catalog.py check` says so again, offline, from what it learned (`build/.cache/catalog`).
- **The list against the sites, again, by hand** (`p3_j22`): the 78 llama.cpp runs and the 50 Ollama runs (128 for 81 models) still have the commit, the files, the sizes and the hashes the list says. The `video` page (WP4.11, one model, one run for ComfyUI) was read from the hub the same way on 2026-10-08, and its files were fetched by the window's button, hashed again and listed by a ComfyUI started on them.
- **The list as a document** (`p3_j08` and the rest of group J, `cargo test -p quickai-core --test p3_catalog` and the application's `app/engine.rs`): it loads without a warning, every category has at least three models, every pin is a commit, every model that
  reads pictures or listens has its projector and the bundle that fetches it, 8 to 16 models are "Start here" with one for each need, no more than a twentieth is large, and the Bonsai 2 4B pin is the packaging's (`build/config.env`).
- **The window and what a click does, by the GUI suite** `catalog` (14 cases, [the test repository's docs/17_catalog.md](../../test/docs/17_catalog.md)), against a fake Hugging Face and a fake `llama-server`: opening asks no site anything, the categories and their counts, words, the two rows of a model with a projector, the bundled download as one job and the model alone as the other, a file that is not the one the list was made for refused and not kept, `--embeddings` and `--mmproj` on the command line, a person's documents adding and hiding models, Escape, Ollama's names pulled, a note shown to its end, and every section under the one before it.
- **Real models, run for real** (the files of the list fetched from the hub, each checked against its hash, started by the pinned `llama-server` the way the application starts it, with `--mmproj` and the mode words, and asked one thing of the right kind; context 4096, no GPU):

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

28 of the 29 gave what was asked (SmolVLM2 answered in a sentence, "The flag of Japan is red, white, and blue": it took a red circle on white for a flag, and the sentence has "red" in it). The one that did not, did not to the first question, and a recording can be asked about in more than one way: Voxtral Mini answers "I'm unable to transcribe audio" to *Transcribe this audio.* and writes the sentence down exactly when asked *Write down the words that are spoken, exactly.* (engine-facts L23), which its note now says.
- **By the application's own code, through the window's button** (`p3_j23`, run by hand, about 2.3 GB): the smallest model of five kinds (a chat model, an embedding model, a reranker, a picture model with its projector, a speech model with its projector) is fetched by the button of its row, the weights are listed by the picker, the Start button starts it, the command line has `--embeddings`, `--reranking` or `--mmproj` as the kind says, and the real server answers: "Paris" (LFM2.5 350M), vectors of 384 numbers (bge-small-en-v1.5), 0.998 for the passage about Paris (Qwen3 Reranker 0.6B), "Red" to a picture of a red square (LFM2.5-VL 450M), and "...ask not what your country can do for you; ask what you can do for your country" (Qwen3-ASR 0.6B). It took 11 minutes, most of it the downloads.

**What the checks had missed, and a person found.** The first version of the window laid the list out wrongly: a scroll area takes the layout of the row it stands in, and the
row of the sidebar and the list goes from left to right, so every section after the first (and, on a category page, every card after the first) was drawn beside the one before it,
off the right edge of the window, where a list that scrolls only down cannot be reached; and a note was drawn on one line and cut with an ellipsis. "All" showed the chat models and
nothing else. Nothing noticed it because the checks read what the window holds (the counts of the sidebar, the first card, a search that left one model) and the application's tests
drew the window without looking at where anything was; the screenshot of a person scrolling it is what showed it. `p3_j24` (every card of the shipped list is whole, for both engines),
`p3_j24b` (the window in three views has nothing cut off or outside it: this one fails on the old layout in all three) and `p3_j24c` (a new page starts at its top) hold it in the
application's tests, and the GUI cases TC-CAT-013 and TC-CAT-014 hold it from outside (both fail on a build with the old drawing).

**What this does not show:** that every one of the 81 models loads and answers. The real runs above are the models that cost little to fetch or whose family could fail in a way the metadata cannot show (a projector, a mode, a new architecture, a different chat template); the other 52 are known by the checks of the first two points and by their family having run (a Qwen3.5 0.8B with its projector was also run, earlier, and read a picture: L19). The Ollama names were checked against the registry, not pulled (pulling is what the look-up of Ollama already does, and TC-REAL-001 does it for real). The runs are CPU-only and Linux-only.

## 8. What is not done

- Large models (the "giants"): one is there (Qwen3-Coder 30B-A3B); the page for them is a later one, and the size filter, the fit hint and the pins already work for them.
- Sharded files (`-00001-of-00004`): left out, as in the look-up, so a model whose repository has only shards (the official Qwen2.5-Coder 7B and 14B) is taken from another repository.
- Text-to-speech and speech-to-speech: llama.cpp's server has no endpoint that speaks, so no model that only speaks is listed. Whisper models are for whisper.cpp, not for llama-server.
- Hugging Face models as `hf.co/...` pulls in Ollama's window, and models of an Ollama library that is not Ollama's own, are not offered; Ollama's entries are names of its library.
- A model of a different size than the ones listed, or a quantization the list does not name, is a click on **Look up online**.
- Windows and macOS: the code is the same and the window is egui; only Linux was run here.
