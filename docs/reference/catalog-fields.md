# Catalog documents: the list of recommended models

_Generated from the types in `crates/quickai-core/src/doc/catalog.rs` by `quickai config docs catalog`. Do not edit by hand: `QUICKAI_UPDATE_GOLDEN=1 cargo test -p quickai-core --test reference` rewrites it, and a test fails while it is out of date._

A catalog document (`kind = "catalog"`) says which models the application recommends, what is known of each and, for each engine, how to get it and how to run it (docs/catalog.md). Catalog documents are data: they can name files and say where they are, and they cannot start a program, add a word to a command line or reach a host of their own (the hub files are fetched from is the one the engine's own look-up of Hugging Face names). The application ships its list in several documents, one for each kind of model; a document of the person's with the same `id` as a shipped one patches it model by model, and one with a new `id` adds a page of their own. Tables merge by key and `[[model]]` entries by `id` (docs/architecture.md, section 5.2). A key this version does not know is a warning; `quickai config check --strict` makes warnings fail.

## the document

The top level of a catalog document, besides the envelope (`schema`, `kind`, `id`). Every document of kind `catalog` is a page of the one list: the application ships several and a person adds their own.

| Key | Value | Meaning |
|---|---|---|
| `title` | text | What the page is called, for messages; default the document's `id`. |
| `verified` | text | The day its facts were last checked against the sites, `YYYY-MM-DD`; the window says the list is as old as its oldest page. |
| `model` | list of tables, below | The models of the page. The list merges by `id`, so a document with the same `id` as a shipped one patches it model by model. |

## `[[model]]`

One model of the list. A model id is unique across every catalog document: a second document that writes it is refused and the message names the first.

| Key | Value | Meaning |
|---|---|---|
| `id` | text | Lower case letters, digits and `.`, `_`, `-`, beginning with a letter or a digit (`qwen3.5-4b`). Required. |
| `name` | text | What the window calls it; default the id. |
| `by` | text | Who made it. |
| `type` | text | An id of the vocabulary's `[[model_type]]` (`chat`, `code`, `vision`, ...); default the vocabulary's default type. One no vocabulary defines is an error, with the nearest one suggested. |
| `note` | text | What it is good for, in a sentence or two (a warning beyond 600 characters). |
| `license` | text | Its licence, as its makers name it. |
| `params` | text | How big it is as its makers say (`4B`, `30B-A3B`). |
| `context` | whole number | The tokens of context it was made for. |
| `input` | list of text | What it reads, as ids of the vocabulary's `[[modality]]` (`text`, `image`, `audio`); default `text`. |
| `features` | list of text | What else is worth knowing, as short lower case words of 24 characters at most, eight at most (`thinking`, `tools`, `multilingual`). They are searched. |
| `start` | bool | Whether "Start here" shows it. Default false. |
| `bundled` | bool | Whether the packages ship it, so that it counts as there when its file is in the folder of the models that come with the application. Default false. |
| `hidden` | bool | A person's document sets it to take a shipped model out of the list: it is not checked and takes no name. Default false. |
| `run` | table of tables, below | How each engine gets and runs it, `[model.run.<engine id>]`. An engine with no table does not offer the model; a model that no engine offers is a warning. An engine id that does not exist is an error. |

## `[model.run.<engine>]`

What one engine does with the model. Which keys are meant depends on what the engine accepts (`[provides]` `accepts`): an engine whose models are files (`formats`) takes `repo`, `rev`, `files`, `companions`, `mode` and `place`; an engine that pulls names (`refs`) takes `ref` and `size`. The other keys are errors, not ignored.

| Key | Value | Meaning |
|---|---|---|
| `repo` | text | Files: the repository they are in, `owner/name`. Required for files. |
| `rev` | text | Files: the commit they are fetched at; default `main`. The application's own documents give a commit, so that what was checked is what arrives. |
| `mode` | text | Files: one of the engine's `[[models.mode]]`, how it is started for this model (`embedding`). One the engine does not have is an error. |
| `place` | text | Files: the folder of the models folder the weights are kept in, for an engine that finds its files by the folder they are in (ComfyUI) and a model whose weights are not where it keeps weights (`checkpoints`). A plain name. |
| `files` | list of tables, below | Files: the weights on offer, one for each quantization. Required for files. They are shown smallest first. |
| `companions` | table of tables, below | Files: the files that go with the weights, by the kind of component they are (`mmproj = { file = ... }`). The engine must be able to attach each kind (`[components.<kind>]`) and have a `[[models.bundle]]` that pairs weights with exactly these kinds; the weights are then offered in a second row, `Get with projector`, as one download. |
| `ref` | text | A name: what the engine pulls (`qwen3.5:4b`). Required for a name. |
| `size` | whole number | A name: what the pull downloads, in bytes, as the registry says. |

## `files` and `companions`

A file of the repository.

| Key | Value | Meaning |
|---|---|---|
| `file` | text | Its path in the repository (`model-Q4_K_M.gguf`, `gguf/model.gguf`): relative, with no `..`, and of a format the engine takes. Required. |
| `size` | whole number | Its size in bytes; a download is refused if it comes to another size. |
| `sha256` | text | Its SHA-256, 64 hexadecimal digits: what a download is checked against while it arrives, and a file that hashes to something else is not kept. |
| `label` | text | What its button says; default the quantization its name says (`Q4_K_M`), else the name. |
| `best` | bool | Whether it is the one the list recommends of the weights (shown in bold); only one may be. Default: the first. |

## What is checked

Each of these is an error that names the file and the place, and a document with an error in it defines no models (the document below it, if any, stays in force):

- a model id is a name and is written once, in one document;
- `type` is a type the vocabulary defines, and `input` names only modalities it defines;
- `features` are lower case words of 24 characters at most, eight at most, and `context` is not 0;
- every `[model.run.<engine>]` names an engine that exists, and has the keys that engine's models need and none of the others;
- `repo` is `owner/name`, `rev` is a commit or a branch, every `file` is a path inside the repository of a format the engine takes, listed once, `size` is not 0 and `sha256` is 64 hexadecimal digits;
- at most one file is `best`;
- every companion kind is one the engine has a `[components.<kind>]` for, and the engine has a `[[models.bundle]]` that pairs weights with exactly the kinds a model has;
- `mode` is one of the engine's `[[models.mode]]`;
- `ref` is a name the engine can pull.

A warning is a model no engine offers and a note of more than 600 characters.
