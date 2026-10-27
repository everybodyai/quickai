# Look-up results, categories and bundles

This pulls three pieces of Phase 4 forward (WP4.2's Hugging Face look-up as a declared primitive, WP4.3's several files fetched as one
thing, and WP4.4's component attached to a model when it starts, as far as `bind.arg`) and leaves the rest of Phase 4 alone: no catalog of recipes, no
Adapters panel, no LoRA, no modality discovery. Where this document and the generated references
([engine-fields.md](reference/engine-fields.md), [vocab-fields.md](reference/vocab-fields.md)) differ on a key, the references win.

## 1. What a person sees

Both engines (llama.cpp and Ollama) look up what is typed in the box of **Add a model** (words, for Ollama a name
without a tag; UI v2, C177), and show the same panel under it; ComfyUI's page has a **Look up online** button:

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

- **Types** (Chat, Code, Vision, Embedding, Audio, Other) are sections, in the order the vocabulary gives. A model is in the first
  type whose rules match what the source says about it: the tags and pipeline of a Hugging Face repository, the capabilities
  of an Ollama card, the words of its name, and whether a projector was found beside it.
- **Size** is a filter. Every button carries the size of what it fetches (exact for a file; for an Ollama tag the parameter
  count times about 0.6 bytes, written `~`), and a size class keeps the buttons that fall in it, and the models that have any.
- A model that needs a **second file** (a vision model's projector) shows it beside the model: one row for the model alone,
  one for the model with its projector, as one download. A model that is self-contained (all of Ollama's) has one row.
- A button fetches what it says: a file or two (llama.cpp) or a name (`ollama pull`). The one in bold is the quantization the
  look-up prefers (its hover says so). Nothing is asked of a site before a click, and nothing is fetched before a second one.

## 2. The shape of it

```
 source ──► Candidate ──present(rules)──► Entry { type, offers[] } ──arrange(filter)──► sections ──► the panel
 (page, JSON)  what the source says        what the window shows       pure, tested            one function, two windows
```

| Piece | Where | Pure |
|---|---|---|
| `Candidate`, `Origin`, `RemoteFile` | `quickai-core::lookup::candidate` | yes |
| `present`, `Entry`, `Offer`, `Fetch` | `lookup::present` | yes |
| types and size classes (`TypeDef`, `SizeClass`, `classify`, `size_class`) | `lookup::types` | yes |
| bundle rules (`Bundle`, `Part`), file globs | `lookup::bundle` | yes |
| arranging entries into sections for a filter | `lookup::view` | yes |
| Ollama's page: address, reading | `lookup::ollama_library` (existing) | yes |
| Hugging Face's JSON: addresses, reading | `lookup::huggingface` | yes |
| the requests | `prims/lookup.rs` (Ollama, existing), `prims/hub.rs` (Hugging Face) | no |
| the panel and its state | `app/lookup_view.rs` | no |
| several files as one job, the pairing file | `downloader.rs`, `models.rs` | no |
| a component in a plan | `plan` (`Selection.components`, `[components.<kind>]`) | yes |

The window never decides what a result is: it draws entries. Both look-ups end in `Candidate`s, so a third look-up (a
folder of a company, a curated list) costs a primitive and no window code.

## 3. Documents

**Vocabulary** (`vocab/core.toml`, data tier, patched by `id`): the nouns the panel uses.

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

A `match` holds when *any* of its lists has a hit: a capability the source names, the pipeline tag, a tag, a **word** of the name
(a word of four letters or more also matches the start of one: `embed` matches `embeddinggemma`; shorter ones match whole), or a
companion kind found beside the model. Matching is case-insensitive.

**Engine** (`engines/llamacpp.toml`): which look-up the window has, how its files are read, which of them go together, and how
the engine attaches a companion.

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

A bundle is offered for a repository when every part finds a file (`match` is a glob over the file's name or path; `prefer`
orders the choices, else the smallest wins). The file is not weights: a file that matches any bundle part is a companion. A bundle
names a `kind` the engine must have a `[components.<kind>]` for, so a bundle that cannot be attached does not load.

To **configure a bundle** a person writes a document of their own in the configuration directory (`~/.config/quickai/engines/llamacpp.toml`
on Linux, with `id = "llamacpp"`; `config check` says what is wrong with it, and the shipped document stays in force while it is wrong):
`[[models.bundle]]` entries merge by `id`, so one can change `prefer` or `label` of the shipped one, or add another. A draft
model for speculative decoding, which a repository such as `unsloth/Qwen3.8-27B-GGUF` keeps as `MTP/mtp-Qwen3.8-27B-Q4_0.gguf`:

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

The vocabulary of a person's (`vocab/core.toml`, `id = "core"`) does the same for the types and the sizes: a `[[model_type]]` of an
`id` the shipped one has changes it (a list such as `words` is replaced as a whole), a new `id` adds a type that is tried after the
shipped ones, and a `[[size_class]]` of the same `id` moves a band. The generated pages say every key.

## 4. Fetching, keeping, attaching

- **Weights keep the name they have always had** (`gemma-3-4b-it-Q4_K_M.gguf`), so what a plain "Get" did does not move.
- **A companion is stored as `<owner>--<repo>--<its name>`**: `mmproj-F16.gguf` exists in nearly every repository and a projector
  of another model is not a stand-in for this one (A12: llama.cpp refuses the pair). A file already there with the size the listing
  gives is not fetched again, so a second quantization of the same repository shares the projector.
- **One job**, files in order, the bar counting the bytes of all of them, a line in the log for each part. The weights come first:
  if a later part fails, the weights stay (they run alone) and the log says what was not fetched.
- **The pairing is a file** beside the weights, `<weights>.bundle.json`: `{ "version": 1, "bundle": "vision", "source":
  "huggingface:ggml-org/gemma-3-4b-it-GGUF", "parts": [ { "kind": "mmproj", "file": "...", "name": "mmproj-model-f16.gguf" } ] }`,
  written after the last file is in place. The picker lists weights only: a file that a pairing names, or that a bundle part
  matches, is not a model, and the paired weights say in their note which projector they use.
- **Start** puts each existing companion in the selection (`Selection.components`); the planner appends the engine's
  `bind.arg` for it after the fields of lower `order` and before `extra_args` (so a person can still say the last word), and the
  log's first line shows it. A companion that has gone missing is said in the log and the model starts without it.
- **Delete** removes the pairing file and the companions no other pairing names, and the confirmation says so.

## 5. The Hugging Face look-up

Two kinds of request, as before: the search (`/api/models?filter=gguf&sort=downloads&direction=-1&limit=50`, with `&search=` for
words) and, for each of the first 30 repositories (20 for words, 20 per other word of the phrase), `/api/models/<id>?blobs=true`
for the files and their sizes. What is read: `id`, `downloads`, `likes`, `pipeline_tag`, `tags`, `gated`, and each file's `rfilename`
and `size`. Findings (probed 2026-10-07, [engine-facts.md](engine-facts.md) L14 and following): the list endpoint gives file names
and no sizes; `gguf.architecture` of a repository that holds a projector can be that of the projector (`clip`), so it is not used;
a popular repository may have no `pipeline_tag` (the second most downloaded had none), so the file names say what the tags do not;
the first of the day's top GGUF repositories was an object detector, which is why there is an Other.

- A **gated** repository (a login is needed) is left out and counted in the status line.
- `User-Agent: quickai/<version>`, no cookies, 15 s a call, replies read as bytes up to 4 MiB; a failure is a sentence that stays
  (not reached, rate limit with the allowance of the API, other status, too big) and is never "no models found".
- The host is the document's `site`, which is how a test points it at a fake. Files come from `<site>/<repo>/resolve/main/<path>`.

## 6. How it is tested

Rows V01 to V16 and B01 to B20 of [phase3-test-plan.md](phase3-test-plan.md), written with the code and each held by a mutation check (the
logic was broken one piece at a time and a test failed every time): the pure half in `crates/quickai-core/tests/p3_entries.rs` and `p3_bundles.rs`
on the recorded answers of the real Hugging Face (`crates/quickai-runtime/tests/fixtures/huggingface/`, made by `record.sh`) and on the documents the
application ships; the primitive in `prims/hub.rs` against `testkit::fake_hub`; the download, the record, the picker, Start and Delete in
`downloader.rs`, `bundles.rs`, `models.rs` and `app/engine.rs`; the window in the GUI suite `lookup_results` (against `test/resources/fake_hub.py`)
and in `ollama_lookup` for Ollama's side. V15 (and U18 for Ollama) run by hand against the real sites:
`cargo test -p quickai p3_v15 -- --ignored`, and `build/test.sh --online` for the window against them; B21 runs the whole chain with a real
vision bundle (`ggml-org/SmolVLM-256M-Instruct-GGUF`, 365 MB) and a real `llama-server`, whose `/props` must then say it takes pictures.

To try it by hand: `scripts/run.sh --fresh`, **Add a model** and a word such as `qwen`; with llama.cpp chosen a vision repository has the two rows.

## 6a. Files that go by themselves, and an engine that is searched by pipeline (WP4.13)

An engine whose files are kept in a folder for each kind (ComfyUI: `diffusion_models`, `text_encoders`, `vae`; [comfyui.md](comfyui.md),
"Getting models") fetches a text encoder or a VAE as often by itself as with a model, because a workflow names its files one by one.
Three keys of the look-up say so, and the rest is what is described above:

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

- **`pipelines` and `words`** make the plain list a few searches instead of one (`&pipeline_tag=text-to-video`, `&search=encoder`; F24),
  each looking into its own first `popular` repositories, each repository once. Words typed are one search, as before.
- **`alone`** gives each file of the repository that a part of that kind finds a button in a row of its own (*Get VAE only:*): the
  quantizations the preferences like and then the smallest, smallest first, as for weights. Such a file keeps the name it has (a
  workflow someone shared names it so; a file of a *bundle* keeps `<owner>--<repo>--<name>` as before), is kept in the folder of its kind
  (`[components.<kind>] place`), and is one file: no record is written. A repository that holds only such files has no model row and is
  offered by them; a file that is not a `.gguf` or `.safetensors`, or is a shard or a draft (`skip`), is not offered.
- **The checksum.** `?blobs=true` has the SHA-256 of every file in large-file storage (F25); the look-up carries it to the piece, so a
  download of any look-up result is hashed as it arrives and a file that is not what the hub states is deleted (the same check as the
  recommended list's). Before WP4.13 a look-up's files were not checked.
- **The sections.** Two types join the vocabulary, `image` (the pictures pipelines, formerly `other`) and `text_encoder` (a repository in
  which a part of a bundle finds a text encoder); an engine that does not run them lists them under `other`, as it did.
- **Where a button's files go** is said in its hover, in a sentence of its own (*It is kept in the vae folder.*).

## 7. What is not done

- Sharded files (`-00001-of-00004`) are left out, as before: a bundle could fetch all the shards, and the picker would then have to
  hide all but the first. Draft models (`MTP/`) are likewise left out until someone writes the bundle for them.
- Hugging Face results in Ollama's window (as `hf.co/user/repo:quant`) and
  a projector chosen by hand (Phase 4's Adapters panel) are not here. (The curated list of recommended bundles was built afterwards as
  [catalog.md](catalog.md): the same entries, a bundled download first and the weights alone second, with the SHA-256 of every file.) `requires_flag` of a component, the fields of a component
  (offload, image tokens) and LoRA are Phase 4.
- The size of an Ollama tag is estimated, not looked up (the registry has the exact size per tag, one request each), and the library's
  models are self-contained, so Ollama's window has no bundle row; a bundle of two names pulled together would be a second kind of
  `Fetch`.
- What a model is comes from what the source says about it: a repository with no pipeline tag, no useful tag and a name that says nothing
  is a chat model, which is the commonest guess and is sometimes wrong; the words and tags of the vocabulary are where that is mended.
