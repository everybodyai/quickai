# Search results, categories and bundles

This document brings three pieces of Phase 4 forward (WP4.2's Hugging Face search as a declared primitive, WP4.3's downloading of several
files as a single item, and WP4.4's attachment of a component to a model at start, as far as `bind.arg`) and leaves the rest of Phase 4
untouched: no recipe catalog, no Adapters panel, no LoRA and no modality discovery. Where this document and the generated references
([engine-fields.md](reference/engine-fields.md), [vocab-fields.md](reference/vocab-fields.md)) disagree on a key, the references take
precedence.

## 1. What the user sees

Both engines (llama.cpp and Ollama) search for what is typed into the **Add a model** box (search terms, or for Ollama a name without a tag;
UI v2, C177) and show the same panel below it; ComfyUI's page has a **Look up online** button:

```
Results (23)
Size: [Any] [Small (< 4 GB)] [Medium (4-15 GB)] [Large (15 GB+)]
v Chat (12)
    unsloth/Qwen3-4B-GGUF                         text-generation · 1.1M downloads · 340 likes
    Quantized from Qwen/Qwen3-4B · apache-2.0
    Get:  [Q4_K_M · 2.4 GB]  [Q5_K_M · 2.8 GB]  [Q8_0 · 4.0 GB]
v Vision (4)
    ggml-org/gemma-3-4b-it-GGUF                   image-text-to-text · 64.5K downloads
    Get model only:        [Q4_K_M · 2.4 GB]  [Q8_0 · 3.9 GB]
    Get with projector:    [Q4_K_M · 3.2 GB]  [Q8_0 · 4.7 GB]
> Embedding (3)   > Audio (1)   > Other (3)
```

- **Types** (Chat, Code, Vision, Embedding, Audio, Other) are shown as sections, in the order the vocabulary defines. A model is placed in
  the first type whose rules match what the source says about it: the tags and pipeline of a Hugging Face repository, the capabilities listed
  on an Ollama model card, the words in its name, and whether a projector was found alongside it.
- **Size** is a filter. Every button shows the size of what it downloads (exact for a file; for an Ollama tag, the parameter count multiplied
  by about 0.6 bytes, marked `~`), and a size class keeps the buttons that fall within it and the models that have any such buttons.
- A model that needs a **second file** (a vision model's projector) shows it alongside the model: one row for the model alone and one for the
  model with its projector, as a single download. A self-contained model (as all of Ollama's are) has one row.
- A button downloads exactly what it states: one or two files (llama.cpp) or a name (`ollama pull`). The button in bold is the quantization
  the search prefers (its tooltip says so). No site is contacted before the first click, and nothing is downloaded before a second one.

## 2. Structure

```
 source ──► Candidate ──present(rules)──► Entry { type, offers[] } ──arrange(filter)──► sections ──► the panel
 (page, JSON)  what the source says        what the window shows       pure, tested            one function, two windows
```

| Component | Location | Pure |
|---|---|---|
| `Candidate`, `Origin`, `RemoteFile` | `quickai-core::lookup::candidate` | yes |
| `present`, `Entry`, `Offer`, `Fetch` | `lookup::present` | yes |
| types and size classes (`TypeDef`, `SizeClass`, `classify`, `size_class`) | `lookup::types` | yes |
| bundle rules (`Bundle`, `Part`), file globs | `lookup::bundle` | yes |
| arranging entries into sections for a filter | `lookup::view` | yes |
| Ollama's page: address, parsing | `lookup::ollama_library` (existing) | yes |
| Hugging Face's JSON: addresses, parsing | `lookup::huggingface` | yes |
| the requests | `prims/lookup.rs` (Ollama, existing), `prims/hub.rs` (Hugging Face) | no |
| the panel and its state | `app/lookup_view.rs` | no |
| several files as one job, the pairing file | `downloader.rs`, `models.rs` | no |
| a component in a plan | `plan` (`Selection.components`, `[components.<kind>]`) | yes |

The window never decides what a result is; it only draws entries. Both searches produce `Candidate`s, so adding a third source (a company's
folder, a curated list) requires only a new primitive and no window code.

## 3. Documents

**Vocabulary** (`vocab/core.toml`, data tier, patched by `id`): the terms the panel uses.

```toml
[[model_type]]                     # the first whose `match` holds wins; the one with `default = true` is last resort
id = "vision"
label = "Vision"
order = 30                         # where its section is (default: after the others, in document order)
help = "Reads pictures as well as text."
match = { capabilities = ["vision"], pipelines = ["image-text-to-text"], tags = [], words = ["vl", "llava"], companions = ["mmproj"] }

[[size_class]]                     # ascending; the one with no `below` is the last
id = "small"
label = "Small (< 4 GB)"
below = "4GiB"
```

A `match` applies when *any* of its lists has a hit: a capability the source names, the pipeline tag, a tag, a **word** in the name (a word
of four letters or more also matches the beginning of a word: `embed` matches `embeddinggemma`; shorter words must match whole words), or a
companion kind found alongside the model. Matching is case-insensitive.

**Engine** (`engines/llamacpp.toml`): which search the window offers, how its files are interpreted, which files belong together, and how the
engine attaches a companion.

```toml
[[models.lookup]]
id = "hub"
use = "huggingface_gguf"           # lookup.huggingface_gguf@1
site = "https://huggingface.co"
prefer = ["Q4_K_M", "Q4_K_S", "Q5_K_M", "Q4_0", "Q8_0", "Q6_K", "Q3_K_M"]   # the default quantization, best first
skip = ["*-?????-of-?????*", "mtp/*", "mtp-*", "imatrix*"]   # a file one of these patterns matches is not weights (shards, drafts, matrices)
show_variants = 4                  # buttons per model
types = ["chat", "code", "vision", "embedding"]   # what this engine runs; the rest is "Other"

[[models.bundle]]                  # weights + parts, one download
id = "vision"
label = "with projector"           # the row: "Get with projector:"
help = "A vision model reads pictures through a second file, its projector."
parts = [ { kind = "mmproj", label = "projector", match = ["mmproj*.gguf", "*mmproj*.gguf"], prefer = ["F16", "BF16", "Q8_0", "F32"] } ]

[components.mmproj]                # how llama-server is told about it
bind.arg = ["--mmproj", "${component.path}"]
```

A bundle is offered for a repository when every part matches a file (`match` is a glob over the file's name or path; `prefer` orders the
candidates, and otherwise the smallest is chosen). Such a file is not treated as weights: any file that matches a bundle part is a companion.
A bundle names a `kind` for which the engine must have a `[components.<kind>]`, so a bundle that cannot be attached fails to load.

To **configure a bundle**, write a document of your own in the configuration directory (`~/.config/quickai/engines/llamacpp.toml` on Linux,
with `id = "llamacpp"`; `config check` reports any errors, and the shipped document remains in effect while yours contains errors).
`[[models.bundle]]` entries merge by `id`, so you can change the `prefer` or `label` of the shipped bundle, or add another. For example, a
draft model for speculative decoding, which a repository such as `unsloth/Qwen3.8-27B-GGUF` stores as `MTP/mtp-Qwen3.8-27B-Q4_0.gguf`:

```toml
schema = 1
kind = "engine"
id = "llamacpp"

[[models.bundle]]
id = "speculative"
label = "with draft model"
parts = [ { kind = "draft", label = "draft model", match = ["mtp-*.gguf"], prefer = ["Q4_0"] } ]

[components.draft]
bind.arg = ["--model-draft", "${component.path}"]
```

A user's own vocabulary (`vocab/core.toml`, `id = "core"`) works the same way for types and sizes: a `[[model_type]]` with the `id` of a
shipped type changes it (a list such as `words` is replaced as a whole), a new `id` adds a type that is tried after the shipped ones, and a
`[[size_class]]` with the same `id` moves a size band. The generated reference pages document every key.

## 4. Downloading, storing and attaching

- **Weights keep their existing file names** (`gemma-3-4b-it-Q4_K_M.gguf`), so the behavior of a plain "Get" is unchanged.
- **A companion is stored as `<owner>--<repo>--<its name>`**: `mmproj-F16.gguf` exists in nearly every repository, and a projector belonging
  to another model cannot substitute for this one (A12: llama.cpp rejects the pair). A file that is already present with the size the listing
  reports is not downloaded again, so a second quantization from the same repository shares the projector.
- **One job**, with files downloaded in order, a single progress bar counting the bytes of all of them, and one log line per part. The
  weights come first: if a later part fails, the weights are kept (they run on their own) and the log states what was not downloaded.
- **The pairing is recorded in a file** beside the weights, `<weights>.bundle.json`: `{ "version": 1, "bundle": "vision", "source":
  "huggingface:ggml-org/gemma-3-4b-it-GGUF", "parts": [ { "kind": "mmproj", "file": "...", "name": "mmproj-model-f16.gguf" } ] }`, written
  after the last file is in place. The picker lists weights only: a file named by a pairing, or matched by a bundle part, is not listed as a
  model, and the paired weights state in their note which projector they use.
- **Start** adds each existing companion to the selection (`Selection.components`); the planner appends the engine's `bind.arg` for it after
  the fields of lower `order` and before `extra_args` (so the user's own arguments still come last), and the first line of the log shows it.
  A companion that has gone missing is reported in the log, and the model starts without it.
- **Delete** removes the pairing file and any companions that no other pairing references, and the confirmation dialog says so.

## 5. The Hugging Face search

There are two kinds of request, as before: the search itself (`/api/models?filter=gguf&sort=downloads&direction=-1&limit=50`, with `&search=`
for search terms) and, for each of the first 30 repositories (20 for search terms, and 20 for each additional word of a phrase),
`/api/models/<id>?blobs=true` for the files and their sizes. The fields read are `id`, `downloads`, `likes`, `pipeline_tag`, `tags`, `gated`,
and each file's `rfilename` and `size`. Findings (probed on 2026-10-07; [engine-facts.md](engine-facts.md) L14 onward): the list endpoint
returns file names without sizes; the `gguf.architecture` of a repository that contains a projector can be the projector's (`clip`), so it
is not used; a popular repository may have no `pipeline_tag` (the second most downloaded had none), so file names supply what the tags omit;
and the top GGUF repository of the day was an object detector, which is why the Other category exists.

- A **gated** repository (one that requires a login) is excluded and counted in the status line.
- Requests use `User-Agent: quickai/<version>`, no cookies, a 15 s timeout per call, and responses read as bytes up to 4 MiB. A failure is
  reported as a persistent message (not reached, rate limited with the API's allowance, another status, or too large) and is never shown as
  "no models found".
- The host is the document's `site`, which is how a test points it at a fake. Files are downloaded from `<site>/<repo>/resolve/main/<path>`.

## 6. Testing

Rows V01 to V16 and B01 to B20 of [phase3-test-plan.md](phase3-test-plan.md) were written together with the code, and each is backed by a
mutation check (the logic was broken one piece at a time, and a test failed every time). The pure half is tested in
`crates/quickai-core/tests/p3_entries.rs` and `p3_bundles.rs`, against recorded responses from the real Hugging Face
(`crates/quickai-runtime/tests/fixtures/huggingface/`, made by `record.sh`) and the documents the application ships; the primitive in
`prims/hub.rs` against `testkit::fake_hub`; the download, the record, the picker, Start and Delete in `downloader.rs`, `bundles.rs`,
`models.rs` and `app/engine.rs`; and the window in the GUI test suite, which the maintainers keep outside this repository (against a fake
hub), for both engines. V15 (and U18 for Ollama) run by hand against the real sites: `cargo test -p quickai p3_v15 -- --ignored`, with the
GUI tests run online for the window. B21 runs the whole chain with a real vision bundle (`ggml-org/SmolVLM-256M-Instruct-GGUF`, 365 MB) and a
real `llama-server`, whose `/props` must then report that it accepts images.

To try it by hand: start the window with a fresh data folder, open **Add a model** and enter a word such as `qwen`; with llama.cpp selected,
a vision repository shows both rows.

## 6a. Standalone files, and an engine searched by pipeline (WP4.13)

An engine that keeps its files in a separate folder for each kind (ComfyUI: `diffusion_models`, `text_encoders`, `vae`; see
[comfyui.md](comfyui.md), "Getting models") downloads a text encoder or a VAE on its own as often as together with a model, because a
workflow names its files individually. Three search keys express this; everything else works as described above:

```toml
[[models.lookup]]
id = "hub"
use = "huggingface_gguf"
types = ["video", "image", "text_encoder"]
pipelines = ["text-to-video", "image-to-video", "text-to-image", "image-to-image"]   # the plain list: one search for each
words = ["encoder"]                  # and for each of these (a text encoder carries no pipeline tag)
popular = 6                          # repositories looked into for each of those
alone = ["text_encoder", "vae"]      # kinds of file (parts of a bundle) that also get a row of their own
```

- **`pipelines` and `words`** turn the default list into several searches instead of one (`&pipeline_tag=text-to-video`, `&search=encoder`;
  F24), each examining its own first `popular` repositories, with each repository examined once. Typed search terms remain a single search,
  as before.
- **`alone`** gives each file in the repository that matches a part of that kind a button in a row of its own (*Get VAE only:*): the
  quantizations preferred by the settings first, then the smallest, smallest first, as for weights. Such a file keeps its original name (a
  shared workflow refers to it by that name; a file that is part of a *bundle* keeps `<owner>--<repo>--<name>` as before), is stored in the
  folder for its kind (`[components.<kind>] place`), and is a single file, so no record is written. A repository that contains only such
  files has no model row and is offered through them; a file that is not a `.gguf` or `.safetensors`, or that is a shard or a draft (`skip`),
  is not offered.
- **The checksum.** `?blobs=true` provides the SHA-256 of every file in large-file storage (F25); the search passes it on to the download,
  so any search result is hashed as it arrives, and a file that does not match what the hub states is deleted (the same check the
  recommended list uses). Before WP4.13, files from search results were not verified.
- **The sections.** Two types were added to the vocabulary: `image` (the image pipelines, formerly `other`) and `text_encoder` (a repository
  in which a bundle part finds a text encoder); an engine that does not run them lists them under `other`, as before.
- **Where a button's files are stored** is stated in its tooltip, in a separate sentence (*It is kept in the vae folder.*).

## 7. Not yet implemented

- Sharded files (`-00001-of-00004`) remain excluded: a bundle could download all the shards, but the picker would then have to hide all but
  the first. Draft models (`MTP/`) are likewise excluded until someone writes a bundle for them.
- Hugging Face results in Ollama's window (as `hf.co/user/repo:quant`) and a projector chosen by hand (Phase 4's Adapters panel) are not
  included. (The curated list of recommended bundles was built afterwards as [catalog.md](catalog.md): the same entries, with the bundled
  download first and the weights alone second, and the SHA-256 of every file.) `requires_flag` for a component, component fields (offload,
  image tokens) and LoRA belong to Phase 4.
- The size of an Ollama tag is estimated rather than looked up (the registry has the exact size per tag, at one request each), and the
  library's models are self-contained, so Ollama's window has no bundle row; a bundle of two names pulled together would require a second
  kind of `Fetch`.
- A model's type is derived from what the source says about it: a repository with no pipeline tag, no useful tags and an uninformative name
  is classified as a chat model, which is the most common case but sometimes wrong; the vocabulary's words and tags are where such cases are
  corrected.
