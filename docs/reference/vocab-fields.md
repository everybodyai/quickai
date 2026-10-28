# Vocabulary documents: modalities, APIs, types of model and sizes

_Generated from the types in `crates/quickai-core/src/doc/vocab.rs` by `quickai config docs vocab`. Do not edit by hand: `QUICKAI_UPDATE_GOLDEN=1 cargo test -p quickai-core --test reference` rewrites it, and a test fails while it is out of date._

A vocabulary document (`kind = "vocab"`) names what engines and the window have in common: the kinds of input and output a model can have, the APIs an engine can serve, with where the text is in the requests and replies of each, so that the Live tab can read the traffic of an engine the code has never heard of, the types of model and the classes of size a look-up sorts what it found by, and the sentences of the notes a person is told about a configuration. A vocabulary can name things and describe them; it cannot start a program or reach a host. A document in the user's configuration directory with the same `id` patches the embedded one: tables merge by key and `[[modality]]`, `[[api]]`, `[[model_type]]`, `[[size_class]]` and `[[finding]]` entries by `id` (docs/architecture.md, section 5.2). A key this version does not know is a warning; `quickai config check --strict` makes warnings fail.

The primitives a document relies on go in `requires` (`requires = { primitives = ["decode@1"] }` for one with `decode` tables); a build that lacks one skips the document and says which.

## `[[modality]]`

A kind of input or output a model can have, named once here so that everything else can refer to it by `id`.

| Key | Value | Meaning |
|---|---|---|
| `id` | text | Names it (`image`). Required. |
| `label` | text | What the window calls it (`Images`). |

## `[[model_type]]`

A type of model, which is a section of the results of a look-up (Chat, Code, Vision, ...). A model is of the first type, in the order written, whose `match` holds, else of the `default` one; the sections stand in the order of `order`. The list merges by `id`, so a document of the person's can change a type or add one (a new one is tried after the embedded ones).

| Key | Value | Meaning |
|---|---|---|
| `id` | text | Lower case letters, digits and `.`, `_`, `-` (`vision`). Required. An engine's `types` list names them, and `other` is where what an engine does not run is shown. |
| `label` | text | The heading of its section (`Vision`); default the id. |
| `help` | text | What it means, shown when the pointer rests on the heading. |
| `order` | whole number | Where its section stands: lowest first; default after the ones that say, in the order written. |
| `default` | bool | What a model is when no other type matches. At most one type is the default, across all vocabularies. Default false. |
| `match` | table, below | What makes a model of this type; any one hit is enough. A type that is not the default and has no `match` is a warning: nothing is ever of it. |

## `[model_type.match]`

What a type's rule looks at, each list in any case.

| Key | Value | Meaning |
|---|---|---|
| `capabilities` | list of text | What the source says the model can do: an Ollama card's `vision`, `embedding`, `audio`. |
| `pipelines` | list of text | The pipeline tag of a Hugging Face repository (`image-text-to-text`, `feature-extraction`). |
| `tags` | list of text | Another tag of the repository (`conversational`). |
| `words` | list of text | A word of the model's name (`qwen3-coder` has `qwen3` and `coder`). A word of four letters or more also matches the start of one (`embed` is in `embeddinggemma`); a shorter one only itself (`vl` is not in `vllm`). |
| `companions` | list of text | A kind of companion file found beside the model by an engine's bundles (`mmproj`): a repository with a projector is a vision model whatever its tags say. |

## `[[size_class]]`

A band of sizes, which is a button of the size filter of a look-up. A model's button belongs to the first class, from the smallest up, that its size is below. Write them from the smallest to the largest; the one with no `below` is last. The list merges by `id`.

| Key | Value | Meaning |
|---|---|---|
| `id` | text | Lower case letters, digits and `.`, `_`, `-` (`small`). Required. |
| `label` | text | What the filter says (`Small (< 4 GB)`); default the id. |
| `help` | text | A sentence for the person who wonders what it is. |
| `below` | size | Where it ends: sizes below it are in the class (`4GiB`, `500MB`; `GB` counts thousands, `GiB` 1024s). Left out, the class takes every size beyond the last bound. |

## `[[finding]]`

The sentence of one note that a person is told about a configuration (docs/configurations.md, section 7): `too_new`, `unknown_engine`, `build_missing` and the rest. The code decides when a note is made and how much it matters (it stops an import, asks a question, warns or only says); a document can reword it and cannot change that. A vocabulary that gives some of the notes gives all of them. The list merges by `id`, so a document of the person's can reword one.

| Key | Value | Meaning |
|---|---|---|
| `id` | text | The note it is for. One this version does not make is a warning. |
| `text` | text | The sentence. `{name}` is replaced by the word the note has for it (`{needs}`, `{this}`, `{model}`...); one the note does not have is an error, and the error lists the ones it has. Required. |

## `[profile]`

What a configuration file may and may not do.

| Key | Value | Meaning |
|---|---|---|
| `deny_env` | list of text | Names of environment variables no configuration may set, in any case; a trailing `*` stands for any ending (`DYLD_*`). They add to a short list the application always refuses (`LD_*`, `DYLD_*`, `PATH`, `PYTHON*`), which no document can take away: a file from outside must not make a program load other code. |

## `[[api]]`

An API a client can call on a running engine. An engine lists the ones it serves in `[provides]` `apis`, and an engine that names an `id` no vocabulary defines does not load.

| Key | Value | Meaning |
|---|---|---|
| `id` | text | Lower case letters, digits and `.`, `_`, `-` (`openai.chat`). Required. Every API of every vocabulary has a different one. |
| `paths` | list of text | Where the engine serves it, exactly as a request names it (`/v1/chat/completions`): a `/` first and no query. An API an engine serves under two names lists both. A path belongs to one API across all vocabularies. Required when there is a `decode`. |
| `capabilities` | list of text | What a client can do with it: `chat`, `completion`, `embedding`. |
| `decode` | table, below | How the Live tab reads its traffic. A request to an API without one is passed on and not shown. |
| `examples` | list of tables, below | Curl examples for the API panel (the window does not show them yet). |

## `[api.decode]`

What the proxy makes of the requests and replies of an API. Each value that says where something is, is a JSON pointer (RFC 6901): a `/` and then the keys and list positions down to it, as in `/choices/0/delta/content`. The proxy passes every byte on as it is whatever this says; something it does not describe is an entry left blank in the Live tab, never a failure.

| Key | Value | Meaning |
|---|---|---|
| `framing` | list of `sse`, `ndjson`, `json` | How a reply comes. The type the engine says it is sending chooses among the ones listed: `text/event-stream` is read event by event (`sse`: lines of `data: {json}`, ending with `data: [DONE]`), a type that says `ndjson` or `jsonl` line by line (`ndjson`: one JSON object a line), anything else when the whole body is there (`json`). A reply that none of the listed framings fits is not read. Required. |
| `prompt` | pointer | Where the prompt is in a request that is not a chat. A request that has none, or an empty one, is not a conversation: it is a load or an unload, and it is not shown. This or `chat`, not both. |
| `delta` | list of pointers | Where the new text is in one event of a stream; the first that gives a text is used. |
| `final` | list of pointers | Where the text is in a reply that comes whole. |
| `error` | list of pointers | Where the engine says what went wrong, in a reply or in an event: the first that is there and not null is shown as the error (a text as it is, an object by its `message`). |
| `chat` | table, below | The shape of a chat request: what the person said is the text of the last message that is not the system's. An empty list of messages is not a conversation either. This or `prompt`, not both. |

## `[api.decode.chat]`

Where the messages of a chat request are and what a message looks like. Pointers inside a message start at that message.

| Key | Value | Meaning |
|---|---|---|
| `messages` | pointer | The list of messages in the request. Required. |
| `role` | pointer | Within a message: who says it. Required. |
| `content` | pointer | Within a message: what is said, a text or a list of parts. Required. |
| `skip_roles` | list of text | Messages with these roles are not what the person asked (`system`). |
| `parts` | table, below | For a content that is a list of typed parts. Without it such a content shows no text. |
| `images` | pointer | Within a message: a list of base64 images, shown as attachments with their type when the first bytes say it is a PNG, JPEG, GIF or WebP. |

## `[api.decode.chat.parts]`

A part of a content that is a list. The text parts are joined for the prompt; the others are shown as attachments by their type (`image_url`, `input_audio`, `file`, or a label of their own for a type the proxy has no word for).

| Key | Value | Meaning |
|---|---|---|
| `type` | pointer | Within a part: its type, `text` for one that is text. Required. |
| `text` | pointer | Within a part: its text. Required. |

## `[[api.examples]]`

One example for the API panel: a curl command, and the same request in Python and JavaScript when they are given (the panel has a tab for each).

| Key | Value | Meaning |
|---|---|---|
| `label` | text | What the example shows (`Chat with an image`). Required. |
| `when` | condition | Show it only while this holds (`has(model.modalities.input, 'image')`). |
| `curl` | text | The command, as text. Required. |
| `python` | text | The same request in Python, as text. |
| `javascript` | text | The same request in JavaScript, as text. |

## What is checked

Each of these is an error that names the file and the place, and a document with an error in it defines nothing:

- an API `id` is lower case letters, digits and `.`, `_`, `-`, and is used once; two documents may not define the same API or claim the same path;
- a path starts with `/` and has no query, and is listed once;
- a pointer starts with `/`;
- `framing` is not empty and names only `sse`, `ndjson` and `json`;
- a `decode` comes with `paths`, and says where the text of a request is, by `prompt` or by `chat`, not both;
- a model type id is a name and is used once (also across documents), at most one type is the `default`, and a list of its `match` has no empty entry;
- a size class id is a name and is used once, every `below` is a size, the bounds go up from one class to the next, and the class with no bound is the last;
- a `[[finding]]` has the `id` of a note this version makes (else a warning), a `text` that is not empty and uses only the words its note fills in, and is given once; the notes of the document all have a sentence when it has any;
- a name of `deny_env` is the name of an environment variable, with `*` allowed at the end, and not `*` alone.

An engine that names an API in `[provides]` `apis` that no vocabulary defines does not load (when there is a vocabulary at all), with the nearest name suggested.
