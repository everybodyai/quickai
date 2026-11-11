# Configurations

A configuration captures a complete QuickAI runtime in one small file: the engine, the model, the build and the settings. Configurations are chosen from a dropdown, saved under a name and shared with others. This page describes how they are created, checked and shared safely.

## 1. In short

A **configuration** is a small text file that specifies what QuickAI runs and how: the engine, the model, the build of the program, the engine's
settings and command-line arguments, and any program that accompanies the engine (Open WebUI beside Ollama). The window always has one **active**
configuration. A dropdown, in a section of its own under the title row, selects it and states in words when the window's current state no longer matches
what was saved. A separate window, built like the Settings, manages configurations: new, save as, rename, duplicate, delete, import and export. A file can
be given to someone else, and when it is opened on their computer QuickAI states, before anything is applied, what that computer lacks or does
differently.

Four rules shape everything below.

1. **A configuration selects; it does not install.** Choosing one sets the state of the window. It starts, fetches and installs nothing.
   Anything it needs that the computer lacks (a model, a build, Open WebUI) is listed, and fetched when the user asks (with a button on the note).
2. **It never carries anything that belongs to a computer or to a person**: folders, program paths, the programs the user has allowed to run, accounts,
   secrets or the theme. (The llama.cpp document already states this: "a settings file copied to another machine must not carry a path".)
3. **A file from outside is not trusted until it has been reviewed.** Text that becomes command-line arguments or environment entries, and the few settings
   that widen what a program may do, are shown in full and applied only with the user's consent.
4. **Nothing changes for users who do not use configurations.** `settings.json` remains the working state; the only new key is the id of the active
   configuration, and it is absent while that is the default, so such a user's file is byte for byte unchanged.

The architecture reserved this in Q10 ("a selection already is a profile; save/load UI later, in Settings first"), and the loader already recognized the
`profile` kind, so this completes a design that was already planned rather than introducing a new one.

## 2. What is data and what is code

The user asked for a data-driven design. The boundary between data and code is as follows:

| Item | Location | What the code does |
|---|---|---|
| A configuration | a `profile` document (TOML): `manifests/profiles/default.toml` inside the application, `<exe>/config/profiles/` from a package, `profiles/` in the user's configuration directory | loads it through the same loader as every other document: the envelope, the layers, diagnostics with file and line, `QUICKAI_SAFE` |
| Which settings a configuration holds, omits or asks about | the **`share`** key of each `[[field]]` in the engine documents (§4), derived from the field's `type` when the document is silent | reads it. No engine, field or program is named in the code (a test scans the code to confirm this: A54, A65) |
| What a companion adds | the companion's document, in the same way | the same code |
| The kinds of name a model can have | the engine's `provides.accepts.refs` (`ollama`) and the recommended list | resolves a reference with them |
| How a build is recognized, and how its backend is judged | the engine's `[acquire]` (label rule, backends) | resolves a label with it |
| The wording of each note (§7) | `[[finding]]` in the vocabulary | selects one and fills it in |
| Names that are never taken from a file (`LD_PRELOAD`...) | `[profile] deny_env` in the vocabulary, in addition to a short floor kept in the code (`LD_*`, `DYLD_*`, `PATH`, `PYTHON*`) | refuses them |
| The screens | nothing specific to any engine | draws the store, the notes and the form model |

This is proven by a test: an engine that exists only as a document (a fifth engine, `echo`, with a companion of its own) is captured, applied, written
out, read back and checked with no change to the code (A10).

## 3. The file

```toml
schema = 1
kind = "profile"
id = "vision-chat"                        # the file is vision-chat.toml; made from the name, never the name itself
requires = { app = ">=0.1" }              # the lowest version that reads this file
name = "Vision chat"                      # what the dropdown shows
engine = "ollama"                         # the keys that are not tables come first: after a table header they would belong to it

[meta]                                    # for the person who gets the file; nothing in it changes what runs
note = "Qwen3-VL 8B on Ollama, 16K context, with Open WebUI beside it."
made_with = "QuickAI 0.1.0"
made_on = { os = "linux", arch = "x86_64", gpu = ["amd"], memory_gb = 32 }    # coarse on purpose: nothing that names a person or a computer

[runtime]                                 # the program as who it is, never as where it is
build = "v0.40.0-ollama-linux-amd64"      # the label of a build in the manager's list: the release and the file
version = "0.40.0"                        # what the program said it was; for the notes

[[model]]
role = "main"
ollama = "qwen3-vl:8b"                    # one source: a name the engine takes (the key is one of its `accepts.refs`), a `catalog` id (+ `file`), or a `file`. Never a path
size = 6140000000                         # to recognise it by, and to say what getting it costs
# from = "huggingface:owner/repo"         # for a file that is no entry of the list: where it can be had; the window says it and fetches on a click, `quickai-headless` fetches from it (headless.md, section 6)
# path = "VAE/x.safetensors"              # ... the file in that repository (left out: at the top, called `file`); `revision`, `into` and `url` are there too

[values.ollama]                           # field values by field id; only those that differ from the document's default
ctx_size = 16384
extra_env = "OLLAMA_FLASH_ATTENTION=1"    # free text: looked at before it is taken (§8)

[values.openwebui]                        # a companion's values sit in a table of their own
enabled = true
port = 3000

[app]
traffic_proxy = false
```

Every key is documented on the generated page [reference/profile-fields.md](reference/profile-fields.md) (`quickai config docs profile`). In summary:

| Key | Meaning |
|---|---|
| `schema`, `kind`, `id`, `requires` | the envelope shared by every document. `requires.app` is the lowest version that can read the file; a file for a newer version is refused with a message asking the user to update |
| `name` | any text up to 60 characters (control and direction characters are removed). The file's id is derived from it (lower case, letters, digits and `-`; a number is appended if the id is taken); the name is never used as a path |
| `[meta]` | `note` (typed by the user who exports), `made_with`, `made_on`. No user name, host name or folder is ever written |
| `engine` | an engine the user can choose; a companion is not one |
| `[runtime]` | `build`, `version`. If absent: the program this computer uses by default (for llama.cpp, the one the package ships). `[runtime.<program>]` pins the build of a program that accompanies the engine (`[runtime.openwebui]`); only `quickai-headless` installs from it |
| `[[model]]` | `role`, one source (§5), `size`, `sha256`, `name` for display, and, for a file, where it can be obtained: `from`, `path`, `revision`, `into`, or `url`. Several are allowed; the first is the one the engine loads |
| `[[needs]]` | what the person running the file must provide in addition: a login (`token`), a computer setting (`setting`), a variable of a program's environment (`env`, and `for`), each with `why` (headless.md, section 7); and what the computer must have (`system`: the id of a prerequisite in the engine's document, such as `c_compiler`, for which the document says how to detect it and what provides it: headless.md, section 12). Only `quickai-headless` asks for them; the window preserves them when it saves |
| `[values.<engine>]` | field values by field id, for the engine and for its companions |
| `[app]` | `traffic_proxy` |
| unknown keys | preserved when the file is rewritten (at the top level and in `[meta]`) and reported as a warning, as in every document |

**Location.** Each configuration is one file: `profiles/<id>.toml` in the user's configuration directory (`~/.config/quickai` on Linux). The usual four
layers apply. The application has `default` built in; a package may ship more in `<exe>/config/profiles/`; a user's file with the same id replaces it
(Save writes `replace = true` when a lower layer has the id, so the two are never merged), and deleting that file restores the built-in one. `QUICKAI_SAFE=1`
ignores the user's layer, which is also the way out of a configuration that causes QuickAI to misbehave (in safe mode, Save and import say so and write
nothing). The dropdown shows where each configuration comes from: *built in*, *from this package*, *yours*; the window marks only those that are not the
user's own (C188).

**The Default** is `manifests/profiles/default.toml`: the llama.cpp engine, no model (the application chooses one, as it does on a first run), and no
values (every field at its default). It can be overwritten like any other configuration, and **Reset to factory** restores the built-in one.

## 4. What is in a configuration

A configuration is not a copy of `settings.json`: it is the part of it that describes a run and is meant to travel.

| Current state | Stored in | Included? | Why |
|---|---|---|---|
| The engine | `values."app.engine"` | yes | `engine` |
| Numbers, choices and flags of the engine's fields (port, context size, GPU layers, GPU, memory, mode...) | `settings.json`, through `legacy_key` or `values` | yes, where they differ from the default | `[values.<engine>]` |
| Free text that becomes command-line arguments or environment lines (*Extra command-line arguments*, *Extra environment*) | the same | yes, **reviewed** | the user asked for command-line arguments; a file from outside must not be able to slip them in |
| Settings that widen what a program may do (internet access *online*, Ollama's own store, the page's Download button) | the same | yes, **reviewed** | `share = "review"` |
| The model | `last_model` | yes, by identity | §5 |
| What accompanies a model (a projector, a mode such as embedding) | the record beside the weights | no: it follows the model | stated by the catalog entry and the record |
| The program's build | `bin_override` | yes, as a label | §5 |
| Folders (models, outputs), a program or Python path, a custom `bin_override` | `models_dir`, `python`, ... | **never** | they belong to the computer; including them would break the file on the first other machine |
| Open WebUI's checkbox, port and internet access | `values` | yes | §9 |
| Open WebUI's installation, data, accounts, stored settings and extra environment | `~/.quickai/openwebui` | **never** | an environment is not a setting (§9) |
| The live-traffic proxy | `enable_traffic_proxy` | yes | it changes the run (an extra hop in front of the engine) |
| The programs the user has allowed to run | `values."app.confirmed_engines"` | **never** | consent belongs to the user, not to a file |
| Light or dark theme | `dark_mode` | no | it is a personal preference, not part of the run |
| Downloaded models, installed builds | the disk | no | a configuration selects (rule 1) |

**`share`** is a key of `[[field]]`: `yes`, `review` or `never`, with an optional `share_note` (the sentence shown when the field is asked about). When
the document is silent, the type decides, so that no existing document needed changing and a new field remains private until a document says otherwise:

| Type | Default `share` |
|---|---|
| `int`, `float`, `bool`, `choice`, `port` | `yes` |
| `text`, `url`, `kv_lines` | `review` (the value reaches a command line, an environment or a host) |
| `dir`, `file`, `runtime_path` | `never` |
| `hidden`, `model_ref` | not a value: handled by the model section, or not at all |

A document that sets `share = "yes"` on a folder, or `share` on a hidden value, receives a warning: the first would fail on the first other computer, and
the second has no effect. What the four shipped documents declare is held by a golden test (A08):

| Engine | `yes` | `review` | `never` |
|---|---|---|---|
| llama.cpp | port, context size, GPU layers | extra command-line arguments, extra environment (C189) | models folder, program |
| Ollama | mode, port, context size, GPU | extra environment, *use Ollama's own store* | model store, program |
| ComfyUI | mode, port, GPU, memory | extra arguments, extra environment, internet access, the page's Download button | present the GPU as (names this computer's card), models folder, output folder, Python, launcher |
| Open WebUI | checkbox, port | internet access | Python, extra environment (§9), launcher |

**Only values that differ from the default are written.** A default can change between versions, and a computed default (llama.cpp's context size depends
on the model and the memory) must not be frozen at the exporter's value. For a setting that has always been in `settings.json`, the default is what a
first run stores; for any other, it is the document's literal default.

## 5. Models and builds

A configuration identifies a model; it never says where the model is on a disk.

| Form | Meaning | Present here? | If not |
|---|---|---|---|
| `ollama = "qwen3-vl:8b"` (the key is a kind of name the engine accepts) | a name the engine's daemon lists and pulls | if it appears in the list the daemon returned last time (this cannot be determined while Ollama is stopped, and the note says so: the model is never reported missing in that case) | the note offers **Get**, enabled while that engine is running, which pulls it |
| `catalog = "lfm2.5-vl-450m"`, `file = "...Q4_K_M.gguf"` | an entry of the recommended list and one of its files; its projector follows from the entry | if the file is in a models folder at the size the entry pins | the note offers **Get**, which runs the Recommended window's own download (including the projector when the list pairs one), after a click |
| `file = "name.gguf"`, `size`, `from` | a file in the models folder, by name only (a name containing a separator, `..`, a drive or a direction character is refused) | if a file of that name and size exists | if `from` is known: the note says where the file can be obtained, and **Look up online** opens with the repository's name in the box. If not: the same button with the file's name, and the note says its source is unknown |

The third form states where the file can be obtained, because **a configuration never carries a model**, and a computer that lacks the file must be
able to get it: `from` (a kind of source and a name within it: `huggingface:owner/repo`), `path` (the file within that repository, when it is not at the
top level), `revision` (a branch, a tag or a commit; a commit always identifies the same file), `into` (the subfolder of the models folder, for an engine
that organizes its files by folder; it must be one that the engine's document names) or, instead of `from`, `url` (`https://`, with no login and no
fragment; `http` only for this computer's own addresses). None of them accepts a path on a computer. The window **displays** this information (the note
shows the repository and the file within it) and fetches on a click, as it always has; `quickai-headless` fetches from it once a file from outside has been
reviewed ([headless.md](headless.md), section 6). The recommended list keeps its own mechanism, `catalog`.

An export finds a `catalog` entry automatically: the file's name and size are looked up in the loaded catalogs, which pin them. **A fetched file records
where it came from**: when a download completes, the application writes the name, the size and the source (`huggingface:owner/repo`) to
`model-sources.json` in its own data directory. (It does not write beside the weights: no models folder receives a file it did not already have, and the
tests of the folders' contents are unchanged.) Along with the source, it records the file's location within the repository when that is not the top
level, and the revision when it is not `main`, so that a file in a subfolder (the VAE of Wan 2.2 is in `VAE/`) is exported with the `path` needed to
fetch it. The export writes this into the file as `from` (and `path`, `revision`), which the window displays but never fetches from on its own; a file
fetched before this was introduced, or placed there by hand, is exported by its name and size, and the note says its source is unknown.

**Builds.** `runtime.build` is the label of a build in the manager's list (the engine's label rule derives it from the release and the file). Choosing
the configuration sets the engine's program to the installed build with that label (the only way a configuration ever sets `bin_override`, and never to
a path supplied by the configuration); a configuration that names no build leaves the program unchanged. A label that is not installed produces a note
listing the labels that are installed (or stating that none is), and **Look up builds** opens the Settings with the label in the manager's filter, where
the user starts the manager's own search. The backend of an installed build is checked against this computer by the manager's own function
(`gpu::mismatch_warning`). `runtime.version` is kept for the notes; comparing it with what the installed program reports requires querying the engine's
`--version` (WP4.4), so for now builds are compared by label.

## 6. Choosing, changing, saving

**The state.** `settings.json` remains what a run uses, and it is written as before: it is the "implicit profile" of Q10. A configuration is the saved
form of its portable part. The window computes one function, `capture(working state)`, and compares the result with the saved configuration: when they
differ, the section shows **• Modified** in amber after the dropdown ("changed since it was saved"), with the list of changes on hover. The comparison
uses the live state, not the file, and is made on the values in force rather than on how a file spells them (C116).
A configuration that names no model accepts whichever model is selected (the application chooses one when none was chosen, and that choice is not a
change). Likewise, none of the following counts as a difference: a model the configuration names that the window cannot hold (not yet downloaded, or not
listed because the engine is stopped); a build it names that is not installed here (the program is left as it was when the configuration is chosen); and
a value the file states that equals the default (in a file written by hand or by another version). A configuration that has just been chosen is a fact,
and it is never reported as modified because of something this computer lacks (A75).

**In the panel (C206, C213).** Since UI v3, the configuration appears as one line at the top of the panel on the right, without the *Modified* word: its
box has an **amber edge** while the list of changes is not empty, and its hover shows that list. **Picking a different model** (from the model list, or
one just downloaded) than the one named by the configuration in use leaves that configuration as it was saved: the window then holds a configuration
**with no name**, which the box shows as *Unnamed* in grey italics, with the amber edge. Its list shows it at the top, marked *not saved*, with no
*Save to* option, so that the original configuration cannot be overwritten by mistake; *Save as...* suggests the model's name; choosing a configuration
(including the original one) applies it, model included. The state is kept for the next start (`app.configuration_unnamed`, with `app.configuration`
still naming the original), the Configurations window marks none as in use, and `quickai configuration list` stars none. A configuration that names no
model (the built-in Default) runs whatever model is picked and keeps its name.

| Action | Result |
|---|---|
| **Choose** (dropdown, or *Use* in the window) | refused while something is running or downloading (the same rule as the Engine row, `engine_locked`), with the same message; with unsaved changes it asks *Save / Don't save / Cancel*; the notes (§7) are computed for this computer: if one of them blocks the configuration, the window says why and stays as it was; otherwise the configuration is applied, `settings.json` is written, the engine's state is reloaded, and the remaining notes are shown in the log and in the window |
| **Apply** | sets the engine, every field of the engine and its companions that the configuration lists, **and resets every other such field to its default** (otherwise two configurations would bleed into each other, and "the same runtime" would depend on what was chosen before); the model is selected if it is present (or cannot be determined); the build is selected if it is installed; the traffic proxy is set. It does not touch folders, program paths, other engines' values, the theme or consents. The new state is completed before anything is stored |
| **Save** | writes the active configuration from the working state, completely or not at all (a temporary file, then a rename) |
| **Save as...** | asks for a name (an empty name, and one already in use in any letter case, are rejected in the box), writes a new configuration and makes it active. The previous one remains as it was saved |
| **Revert** | asks for confirmation (*Go back to what was saved?*, in the window where it was pressed), then applies the saved configuration again; it is a button in the section while there are changes, and on the card of the configuration in use in the Configurations window |
| **Rename**, **Duplicate**, **Delete** | Rename changes `name` only; Duplicate asks for a name; a built-in or package configuration cannot be renamed or deleted (save it under another name instead); deleting the configuration in use asks for confirmation, after which the Default becomes the one in use and **the window keeps its current state** (applying the Default would change what the user runs merely because they deleted a file) |
| **Reset to factory** | available on the user's own Default, after confirmation: removes their file, and the built-in one becomes the Default again |

Choosing starts nothing. As when choosing Ollama, the user presses Start.

**Nothing is applied at startup.** The working state is authoritative; at startup QuickAI only reads which configuration is active and loads it as the
baseline for the badge. If that configuration is gone (a deleted file, a start in safe mode), QuickAI falls back to the Default with a line in the log,
and the working state is not touched.

**The first start with configurations** *adopts* the working state: if it differs from the built-in Default, it is written once as the user's Default (a
file in their own configuration directory, noted in the log), so that users who tuned their settings do not see *Default* marked as modified on their
first start. The `profiles/` folder records that this was done: it is created at that point, and if it exists, nothing is written. A user whose settings
match what a first run holds gets the folder and no file. What is adopted is **what the user had set, as stored in the file** (C112): a model the window
picked for someone who never chose one is not theirs and is not included in their Default (a Default that names no model accepts whichever is selected,
C108); a model they had chosen is recorded in the form the window compares (the file and its size, not the recommended-list entry that has a file of that
name). This was found by the first run of the GUI test suite, which the maintainers keep outside this repository: a Default containing the model the
window had picked showed the unsaved mark immediately (a dot, at the time) when the file was not the one in the list.

## 7. Sharing, and the notes

**Export.** Started from a row of the window: a name, a note for the recipient, a file (a path box with *Browse*), and a preview of the text that will be
written. Below it, what was left out: *Settings left out because they belong to this computer (a folder, a program): 3*, *Lines left out because they look
like secrets (a key, a token, a password): 1*, *X has no known source*. Secrets are not written: an environment line or an argument whose name or flag
contains `KEY`, `TOKEN`, `SECRET` or `PASSWORD` (`is_secret_name`) is left out and reported. The file is written only where the user chose; an existing
file is replaced only if the user ticks the option to do so.

**Import.** A path box with *Browse* (a native file dialog cannot be driven by the GUI tests, so the box is also how the tests operate it), followed by
the **report**, shown before anything is stored: the name and what the file says about where it was made, *This computer:*, the settings that would change,
the notes, the items to tick, the name (with three options when it is already in use: *Keep both*, *Replace*, *Cancel*), and **Import**, **Import and use**
and **Cancel**.

Nothing is written until **Import** is pressed. Anything not ticked is not stored; what is stored becomes the user's own configuration from then on.
Checking, previewing and reading never use the network, start a program or write a file (A33, A48).

**The notes.** There are four levels: **Stops** (the file cannot be used here; nothing is written), **Asks** (consent is needed), **Warns** (it differs;
it can still be used) and **Says** (information only). The ids are stable names; **the wording comes from the vocabulary's** `[[finding]]` (a vocabulary
that defines some must define all of them, and the words in a sentence are the placeholders its note fills in), **while the levels are fixed in the
code**: a document, which may come from anyone, can reword a note but cannot turn a question into a statement. A golden test covers all of them (A31).

| Id | Level | When | What the user is told |
|---|---|---|---|
| `not_a_configuration` | Stops | not TOML, not a `profile`, no id, no engine, over 256 KiB, or a model or name that is a path | This file is not a QuickAI configuration (why). |
| `too_new` | Stops | `schema` or `requires.app` above this version | This configuration was made by a newer QuickAI: it needs QuickAI >=0.3, and this is 0.1.0. Update QuickAI to use it. |
| `unknown_engine` | Stops | an engine this build has no document for, or a companion named as the engine | It is for `whispercpp`, which this QuickAI does not have. |
| `free_text` | Asks | `review` text (extra arguments, extra environment) | the field's own `share_note` (*Lines of environment, set for Ollama as they are written.*), otherwise the vocabulary's; the text is shown in full |
| `wider_access` | Asks | a `review` field that is not text (internet access *online*, Ollama's own store...) | the field's own sentence |
| `name_taken` | Asks | the id or the name already exists | Keep both / Replace / Cancel |
| `refused_line` | Warns | an environment name on the deny-list, or one the engine protects | `LD_PRELOAD` cannot be set from a configuration: the line is left out (whatever is ticked) |
| `names_a_path` | Warns | free text names an absolute folder | names `/home/alice/x`, a folder on the computer it was made on |
| `unknown_setting` | Warns | a value for a field the engine does not have, or for an engine that does not accompany this one | `ollama.flavour` is not a setting this QuickAI has: ignored |
| `private_setting` | Warns | a value for a field whose `share` is `never` | belongs to a computer and is not carried: ignored |
| `bad_value` | Warns | out of range, of the wrong type, not an option, or too long | Context size: 99999999 is not accepted (more than 131072 or less than 256); the default is used |
| `unknown_keys` | Warns | keys this version does not recognize | kept in the file, ignored |
| `other_system` | Warns | a different OS or architecture | Made on Windows x86_64; this computer is Linux x86_64. Arguments and builds made there may not exist here |
| `other_gpu` | Warns | the build's backend requires a GPU vendor this computer lacks | the manager's own sentence |
| `memory` | Warns | the model does not fit (`heuristics::fit`, the Recommended window's rule, now in the core) | needs about 14.4 GB of memory, and this computer has 8.0 GB |
| `model_to_get` | Warns | a model that is not present and can be obtained | Not here: qwen3-vl:8b (5.7 GB). It can be got from Ollama. (**Get**, **Look up online**) |
| `model_unknown` | Warns | not present, with no known source | X (2.6 GB) is not here, and the file does not say where it came from. (**Look up online**) |
| `build_missing`, `build_none` | Warns | the label is not installed | Made with v0.40.0, which is not installed here. The builds that are: ... (or: and no build of this program is). (**Look up builds**) |
| `port_taken`, `ports_clash` | Warns | a port the file sets is currently in use; two ports in the file are the same | |
| `companion_missing` | Warns | Open WebUI is enabled but not installed | Open WebUI is switched on and is not installed here. (**Set up**) |
| `companion_address` | Warns | §9 | Open WebUI keeps the address of Ollama it was first started with (...); it would be given ... now |
| `model_unseen` | Says | Ollama is stopped | Ollama is not running, so it cannot be told whether X is here. Start Ollama to see. |
| `will_change` | Says | always | the list of settings that change (six, and how many more) |
| `left_out_private`, `left_out_secrets`, `left_out_source` | Says | an export | what is left out of the file |

Two further notes require querying the engine's `--version` and `--help` (WP4.4): `argument_unknown` (an argument the installed build does not support)
and a version comparison.

## 8. Safety

A configuration is as likely to come from a stranger as from the user. The threat is the file itself, and the defenses are built into the design rather
than relying on a prompt.

* **It is data.** The `profile` kind belongs to the data tier: it can select things the registry already knows and cannot name a program or a host to
  run. A model is a name, a catalog id or a file name, and may state where the file can be obtained (`from` with its `path` and `revision`, or an `https`
  address without a login), which the window displays and `quickai-headless` fetches from only after the user has reviewed it (a file from outside; C122,
  C125); a build is the label of an installed build. The model itself is never in the file, and no key accepts a path on a computer.
* **Free text is reviewed.** Everything that becomes a command-line argument or an environment entry is shown in full and applied only if ticked; if
  unticked, it is not stored. Lines the engine protects (`OLLAMA_HOST`, the isolation of C92) cannot be set by a file, just as they cannot be set by the
  user. Names on the vocabulary's deny-list, and a floor kept in the code that no document can lower (`LD_*`, `DYLD_*`, `PATH`, `PYTHON*`), are refused
  even when ticked, and again when a file placed in the folder by hand is chosen.
* **Limits.** 256 KiB per file; strings of at most 4 KiB; at most 512 settings and 8 models; values are checked against their field's type, range and
  options; model names and labels may not contain a separator, `..`, a drive, NUL or a direction character; names are displayed with control and
  direction characters removed; a number that wraps around to a valid schema number is not accepted as one. A table of hostile files (A32: nesting
  100 000 levels deep, a million keys, a name of 100 000 characters...) is refused or sanitized, and never causes a crash.
* **No network, no process.** Reading, checking and previewing do neither, and the core cannot (`tests/purity.rs` enforces this, A33). The network is
  used only by actions the user starts: *Get*, *Look up online*, and the manager's search.
* **Private data is not written.** The export is built from an allow-list (fields with `share = yes`, and `review` fields the user kept), so a field
  added in the future remains private until its document says otherwise.
* **Recovery.** `QUICKAI_SAFE=1`, the Default's *Reset to factory*, and the fact that choosing a configuration starts and installs nothing.
* **Provenance is not claimed.** There is no signature and no author verification; the report states what the file says about itself and nothing more.
  Signing is deferred to the content packs of Phase 6.

PRIVACY.md has a paragraph on configurations: they are files in the user's configuration directory; export writes where the user chooses; import reads
what the user chooses; QuickAI is designed not to send them anywhere; and `model-sources.json`.

## 9. Open WebUI, and the companions

The user: "for something like Open WebUI the environment may not be mutable, so leave this out if not possible". **This is correct, and it was verified
in the release's own source code (open-webui 0.11.4, the version the document pins) and then by running it (W16 of [engine-facts.md](engine-facts.md),
A67):** `models/config.py` keeps a `config` table in Open WebUI's database, and `seed_defaults` states "Existing DB values take precedence over defaults":
the environment supplies the values only the **first** time. The settings QuickAI passes through the environment that are retained in this way are
`OLLAMA_BASE_URL` (`ollama.base_urls`), `ENABLE_OLLAMA_API`, `ENABLE_OPENAI_API`, `RAG_EMBEDDING_ENGINE` and `RAG_EMBEDDING_MODEL`, as are many of the
settings a user might type in *Extra environment* (which ones is for Open WebUI's own documentation to say; its code does not distinguish them by name).
What is read at **every** start is `OFFLINE_MODE` (the *internet access* field), `CORS_ALLOW_ORIGIN`, `DATA_DIR` and similar variables, and the port is a
command-line argument.

For a companion, therefore:

* **Included in a configuration: only what QuickAI sets at every start**: Open WebUI's checkbox (`enabled`), its `port` and its internet access (`review`
  when online).
* **Never included**: its build and installation (an environment of its own), its data (accounts, chats, documents, and what its administrator configured
  in its pages), its Python and launcher paths, and **its extra environment**: a line there works for some settings but not for those Open WebUI keeps in
  its database after its first start, and a configuration must not promise behavior that depends on that. The document states this with
  `share = "never"` and a `share_note`; the code knows nothing about Open WebUI (A65).
* **The request requires the program.** A configuration that enables Open WebUI on a computer that lacks it produces the note `companion_missing`
  (**Set up** opens its section). Nothing is installed by choosing.

**An issue in previously built behavior (WP4.16), now tracked.** Open WebUI uses the Ollama address it was given **at its first start** and retains it:
if Ollama's port is changed afterwards (by the user, and now easily by a configuration), it continues to call the old address until the address is
changed in its *Admin settings, Connections*. QuickAI now records the address Open WebUI was first started with (in the field its document names,
`first_address_field = "first_ollama"`: a hidden value of QuickAI's own, never included in a configuration; Open WebUI's database is not opened), notes in
the log at each start when the address it would now pass is different, and the notes of a configuration that would move Ollama say the same
(`companion_address`, A66). `ENABLE_PERSISTENT_CONFIG=false` was rejected: every setting the user made in Open WebUI's pages would be lost at each start.
A possible later improvement, if the user wants it: Open WebUI always connects to a fixed loopback port that QuickAI forwards to the current Ollama (the
gateway of the live view already does something similar), so that the stored address never goes stale.

ComfyUI does not behave like this (it retains nothing the environment cannot set), and llama.cpp and Ollama read their settings at every start.

## 10. What the user sees

**The section.** The title row contains the heading, the live-traffic tabs (when enabled) and the four icons, as it did before configurations existed.
Below it, on the Main tab, is a section as wide as the page, framed like the page's other sections (C114):

```
 QuickAI                                                                          speed  gear  info  moon
+---------------------------------------------------------------------------------------------------------+
| Configuration:  [ Vision chat                      v ]                                                  |
+---------------------------------------------------------------------------------------------------------+
```

While the window's state differs from the configuration in use, the section has an amber outline with a faint amber fill (a deeper amber in the light
theme), and shows the word and the three available actions (C115):

```
+- amber -----------------------------------------------------------------------------------------------+
| Configuration:  [ Vision chat                      v ]  • Modified   [ Save ] [ Save as... ] [ Revert ] |
+---------------------------------------------------------------------------------------------------------+
```

```
 v Default                          llama.cpp        built in
   Vision chat (modified)           Ollama           yours
   Coding, small                    llama.cpp        yours
   Studio                           whispercpp       yours      (grey: the reason on hover: this QuickAI has not that engine)
 ---------------------------------------------------------------
   Save          Save as...          Manage configurations...
```

The user's words, on seeing the first version, which had the dropdown in the title row and a small amber dot after the name: "move the dropdown to a different section, doesn't look good at the location - right next to the header.
Additionally, when user makes an edit, it has to show the configuration as edited - or find other way to specify that it has changed. i.e. If I load a configuration, it is a fact - but the moment I change, It should
convey that it is modified". An unchanged section is a fact, so it says nothing; a change is prominent: a word, a color, and the available actions. The
word is a separate badge rather than a mark after the name, because a long name is truncated by the dropdown and would take the mark with it. *Save*
overwrites the configuration in use, *Save as...* asks for a name, and *Revert* asks for confirmation (*Go back to what was saved?*) and then reverts; the
dropdown still offers *Save*, *Save as...* and *Manage configurations...* below its list, and the row of the configuration in use shows *(modified)* after
its name. The bullet is amber and uses a character the fonts include (the filled circle is drawn as an empty box, so the mark is U+2022; a test queries
the fonts, A50). The dropdown is greyed out, with the Engine row's message on hover, while something is running or downloading, and *Revert* along with
it. Hovering over the badge or the dropdown lists what changed since the last save, in plain language (*Port: 8080 to 9000*, the first eight and a count
of the rest). The question when a choice would discard changes, the *Revert* confirmation and the name box are modal dialogs of the main window.

**Nothing moves when a change is made.** The section always reserves space for the badge and the buttons, the dropdown stays in place at the same width,
and the section keeps the same height, so that the field the user is typing in, below it, does not shift when the first keystroke makes the badge appear
(A73: the label, the dropdown, the title row and the content below the section are in the same positions in both states, at three widths). The window can
be as narrow as 520: the dropdown gives up its width first, and then the word *Configuration:*, when there is not enough room for everything alongside the
badge and the buttons (`section_shape`; A72 checks that the parts are in order, separated and inside the outline from 520 to 1400, with the tabs on and
off). A name longer than the dropdown is truncated at its width with an ellipsis (a name is at most 60 characters; egui's dropdown grows to fit its text
unless told to truncate it, and the first version pushed the badge and the buttons out of the row at 520, which was found by checking a name of 57
characters), and the hover shows the full name. The Live tab, which shows the traffic rather than the run, has no section.

**The window** *QuickAI Configurations* is a viewport of its own, like the Settings (C87: opening at 780x700 since C188, with the window manager's title
bar and **Close** at the bottom alongside the last action performed; when requested again it comes to the front, and closing it by any means saves what
was changed). Since C188, each configuration is shown as a card:

```
 [ + New from current ]  [ Import... ]
 +- green: the one in use ------------------------------------------------------------+
 | [L] Coding helper  (In use) (• Modified) (! 1)              [ Revert ] [ Save ]  ... |
 |     llama.cpp · Qwen2.5-Coder 7B Instruct                                           |
 +-------------------------------------------------------------------------------------+
 | [L] Default  (built in)                                                  [ Use ]  ... |
 |     llama.cpp · any model                                                           |
 +-------------------------------------------------------------------------------------+
 | [O] Vision chat  (! 3)                                                   [ Use ]  ... |
 |     Ollama · qwen3-vl:8b · with Open WebUI                                          |
 |     Qwen3-VL 8B on Ollama, 16K context, with Open WebUI beside it.                  |
 |     Made with QuickAI 0.1.0 on Linux x86_64 (AMD GPU, 32 GB)                         |
 |     (Context size 16384) (Extra environment OLLAMA_FLASH_ATTENTION=1)                |
 |     (Open WebUI Port 3000) (Program v0.40.0-ollama-linux-amd64)                      |
 |     !  Made with v0.40.0-ollama-linux-amd64, which is not installed here, ...  [ Look up builds ] |
 |     !  Open WebUI is switched on and is not installed here.                  [ Set up ] |
 +-------------------------------------------------------------------------------------+
 | [W] Studio dictation                                                             ... |  (grey)
 |     It is for whispercpp, which this QuickAI does not have.                          |
 +-------------------------------------------------------------------------------------+
 > 1 file couldn't be used
 [ Close ]                                  Imported "Shared chat".
```

- **The badge** is the first letter of the engine's name on a tint of its own (assigned by the engine's position in the selector, so that no two engines
  in this QuickAI share one: `theme::tint`); an engine this QuickAI does not have is shown as an outline.
- **The tags** after the name: *In use*, *• Modified* while the window's state differs from what was saved, *built in* or *from this package* (the user's
  own configurations show nothing, as most are the user's own), and the number of notes that warn about it on this computer. When there is no room for
  them beside the name (a narrow window, with Revert and Save alongside), they are dropped one by one: first the note count, then the origin, then
  *In use* (the green frame indicates it as well); *Modified* is never dropped.
- **The line** below the name: the engine; the model, by its name in the recommended list when it is listed there (`Qwen2.5-Coder 7B Instruct`, not the
  file name), otherwise the name the file gives or the engine's own, or *any model* when none is named; and *with* each program that accompanies the
  engine and is enabled. A configuration blocked by its notes shows the first of those notes instead, and is grey with no Use button.
- **On the right**: *Use* (grey, with the Engine row's message, while something is running; its hover explains what using it does and lists what would
  change, the first eight); on the configuration in use, while it has changes, *Revert* and *Save* (the only green button in the window, because the test
  suites find the main button by its color); then the **...** menu: *Rename...*, *Duplicate...*, *Export...* and *Delete...* (*Reset to factory...* for
  the Default). Built-in and package configurations cannot be renamed or deleted, and say why on hover.
- **Clicking the card** (not its buttons) expands it below its first rows: the note from its author, the computer it was made on when that differs in
  kind from this one (another system, processor or GPU), what it sets, shown as chips (the label without the hint in brackets, with a program that
  accompanies the engine named first; the switch that enables that program is described by the line, not by a chip), the program it was made with, and
  the notes, each with a warning sign and its button on the right. One that sets nothing says *Every setting at its default.* No card is expanded when
  the window opens. Since C189, the chips begin with what accompanies the model on its command line, which a configuration does not carry (it follows the
  model, §4): each file fetched with it, by the part's label and the file name (*Projector mmproj-Qwen3VL-2B-Instruct-F16.gguf*), taken from the record
  beside the weights when the model is present and from its recommended-list entry when it is not, and *Started as* with the label of the mode it starts
  in (*embedding server*). The full command line is shown on the *Starting it* page of the Settings ([implementation-plan.md](implementation-plan.md),
  C189, C190).
- **Removed** in C188: the arrows before the names, *yours* on every row, *N settings differ from what you have set now* (the hover of Use lists them),
  the line about the computer it was made on for one made on the same kind of computer, and **Save here** (it replaced another configuration with the
  window's state; *+ New from current* followed by *Delete* does the same, and it read too much like Save). Files that could not be used are collapsed
  under *N file(s) couldn't be used*, with the reason shown when expanded.

The list, the details, the notes and the import and export dialogs are built from the store, the checks and the form model: no engine name appears in the
code. *Use* is grey, with the reason on hover, when the notes block it or something is running; the buttons on a note are grey, with the reason, when the
configuration is for a different engine from the one in use. The test cases inspect every view (`look-at-every-view`): the dropdown open in each state,
the list scrolled to the end with many configurations and long names, each dialog, and a narrow window (440 px: A57); the cards were also inspected in
both themes at widths of 780 and 500.

## 11. From the command line

For scripts, for support, and as the means of testing the whole feature without a window:

```
quickai configuration list
quickai configuration show <id | file>
quickai configuration check <file> [--strict]          # the notes for this computer; exit 1 if one stops it (and, with --strict, if one asks or warns)
quickai configuration export <id> [-o file] [--note TEXT]
quickai configuration import <file> [--as NAME] [--replace | --keep-both] [--accept-review]    # what is asked about is not taken without --accept-review
quickai plan --configuration <id | file> [--explain | --json]   # the run plan it makes here; what is said on the line wins
quickai config docs profile                             # the reference page
```

`use` is deliberately omitted: the window writes `settings.json` when it changes and when it closes, so a command that wrote it while the window was
running would be overwritten by the window.

## 12. Decisions

| Id | Decision | Rationale, and the alternative |
|---|---|---|
| C98 | **Terminology.** *Configuration* in windows, on the command line and in user documentation; `profile` remains the name of the document kind. | It is the user's term. The code and the logs already use "configuration" for the layers of documents (`config check`, `QUICKAI_CONFIG_DIR`), so a kind with that name would be ambiguous; the kind was reserved as `profile` (Q10) and nothing used it. Alternative: rename the kind now |
| C99 | **A configuration is a `profile` document in the data tier**, one file each, within the existing layers. | It inherits the loader, the diagnostics, safe mode and the generated reference page |
| C100 | **Presets, not auto-save.** `settings.json` remains the working state; a configuration is saved deliberately (Save, Save as); the section shows *Modified* while there are changes (originally a dot in the title row, C115); choosing with unsaved changes asks for confirmation. | The user wrote "save by a different name", and this is the only model in which a user can experiment and keep the original. Alternative: the active configuration follows every change; this could become a switch later, since it uses the same write |
| C101 | **The Default is a document, can be overwritten, and has *Reset to factory*; the working state is adopted into it once.** | No modified mark on the first start; nothing hidden |
| C102 | **What is included is declared by the documents** (`share`, derived from the type when unspecified). Folders, program paths and consents are never included. Only values that differ from the default are written. | Data-driven; a new field is private until declared otherwise |
| C103 | **A configuration selects and does not install.** Models and builds are references; nothing is fetched until the user clicks. | The user's "same runtime is available there" is met by stating what is missing and making it one click to obtain, not by acting without being asked (the rule of C93) |
| C104 | **A file from outside is reviewed before it is applied**: free text and wider access are ticked individually, the deny-list is absolute, limits apply, and there is no network access. | See §8 |
| C105 | **A companion contributes only what QuickAI sets at every start**, never its environment; the address Open WebUI retains is recorded and reported. | See §9 |
| C106 | **Choosing is refused while something is running or downloading**, as with the Engine row, and starts nothing. | The same rule users already know. Alternative: a *Stop and switch* button, later |
| C107 | **The wording of a note comes from the vocabulary and its level from the code**, and the code keeps a floor of names a file may never set, which no document can lower. | A document, which may come from anyone, must not be able to turn a question into a statement, or remove the names that make a program load other code |
| C108 | **A configuration that names no model accepts whichever model is selected**, and a model it names that the window cannot hold is not a difference (`matches_run`). | The Default names none, and the application chooses a model on a first run: without this, every fresh installation showed the Default as modified, which was found on the window's first run |
| C109 | **The source of a fetched file is kept in the data directory** (`model-sources.json`), and a configuration carries it as a hint (`from`), which is displayed and never fetched from automatically. | The plan said "in the record beside the weights"; that record is written only for bundles and modes, and a record for every download would change the folders that tests and users inspect. A data-tier document must not name an address (changed for the headless program by C125: a file may state where its model can be obtained; the window still fetches only on a click) |
| C110 | **Deleting the configuration in use makes the Default the one in use and leaves the window unchanged.** | Applying the Default would change what the user runs merely because they deleted a file. The plan said "chooses the Default" |
| C111 | **The field that records a companion's first address is named by the companion's document** (`[engine] first_address_field`, a `hidden` field of the same document), not by the code. | The first version of the code said `first_ollama`; the source scan of the configuration code (A54) found an engine's name in it, and the same word appeared in the companion's own code. A document that names a field that does not exist or is not hidden is an error reported with its line; one that is not a companion's produces a warning |
| C112 | **The first start adopts what the user had set: the settings as stored in the file, and a model only if they had chosen one.** | The window had picked a model for a user who never chose one, and the Default pinned it; when that file was not the one in the recommended list, the unsaved mark appeared immediately (found by the GUI tests; A69) |
| C113 | **The two messages that count what was left out do not depend on the count**: `Settings left out because they belong to this computer (a folder, a program): 3.` and `Lines left out because they look like secrets (a key, a token, a password): 1.` | The first wording said `1 lines`, which users read in the CLI and in the export dialog; the vocabulary has no plural forms, and placing the count last reads correctly for one and for many |
| C114 | **The configuration is a section of the Main page below the title row, not part of the title row.** | The user, on seeing the first version: "move the dropdown to a different section, doesn't look good at the location - right next to the header". The title row was also crowded (the dropdown gave up its label and its width at 520, and the GUI tests had found a tab covered by the settings icon, A70). The section is as wide as the page and framed like the sections below it, and the title row is as it was. Alternatives: a status bar at the bottom (the choice would be far from what it affects), or the Settings window (a top-level choice would require a second click); not requested, mentioned in the report |
| C115 | **A change is indicated in words and in color, in the configuration's place, together with the available actions**: a badge *• Modified*, an amber outline, *Save*, *Save as...* and *Revert*; the space is reserved, so nothing moves; an unchanged section says nothing. | The user: "when user makes an edit, it has to show the configuration as edited ... If I load a configuration, it is a fact - but the moment I change, it should convey that it is modified". A dot after the name was too small, and was cut off with the end of a long name. *Revert* is new in the section (the window already had it) and asks for confirmation, as the window does. Alternatives: the word *(modified)* inside the dropdown only (truncated with a long name); a line under the section that appears (the page would shift under the field being edited) |
| C116 | **"Modified" means "the list of changes is not empty"**: one answer for the badge, the hover and the question; settings are compared by the values in force, and what the computer cannot hold (a model that is not present, a build that is not installed) is not a difference. | The first version of the section showed Modified immediately after choosing a configuration whose build the user had not installed, with a hover that listed nothing: the answer was computed from the raw values (`matches_run`: a value stated in the file that equals the default counted as a difference) and the list from the values in force (`changes`). "If I load a configuration, it is a fact": found by reviewing the screenshot of the first version of this change (A75) |

## 13. Work packages and tests

WP4.17, size XL. All of it is built and tested; the testable core was built first.

| WP | Work | Verified by |
|---|---|---|
| 4.17.1 | **The document.** The registry reads `profile` (`doc/profile.rs`, `registry/profile.rs`, `profile/mod.rs`): the schema of §3, checked against the engines' documents; `share` and `share_note` on `[[field]]`, with defaults by type and the shipped documents annotated; the built-in `default.toml`; `[[finding]]` and `[profile]` in the vocabulary; `quickai config docs profile` and `docs/reference/profile-fields.md`; the sketch in `docs/examples/profiles/` replaced | A01-A09 |
| 4.17.2 | **Capture, apply, difference** (pure, in the core: `profile/state.rs`): working state to configuration and back; the difference the badge is based on; **the plan an imported configuration produces is the plan of the computer that made it**, apart from the paths that belong to each computer | A11-A19, A71 |
| 4.17.3 | **The notes** (pure: `profile/check.rs`, `notes.rs`, `screen.rs`): `review(configuration, registry, inventory)` with the inventory the application provides; review and refusal; the list of what an export leaves out; the table of hostile files; **a fifth engine that exists only as a document participates** | A10, A20-A34 |
| 4.17.4 | **The store and the state** (`configurations.rs`, `settings.rs`, `configuration_cli.rs`): the profiles folder (atomic writes, ids), `app.configuration`, the baseline, the adoption, the startup fallback, Save, Save as, Rename, Duplicate, Delete, Reset; `quickai configuration ...` and `plan --configuration` | A18, A41-A49, A69 |
| 4.17.5 | **The dropdown**, the unsaved-changes question and the name box (`app/configurations_ui.rs`) | A50-A52, A68, A70 |
| 4.17.9 | **The section and the badge** (after the user's screenshot of 4.17.5, C114-C116): the dropdown moves from the title row to a section below it; a change is indicated in words, in amber, with Save, Save as... and Revert, and nothing moves; "modified" has a single definition (`app/configurations_ui.rs`, `app/header.rs`, `app/configurations_window.rs`); ten GUI test cases | A72-A75, and the GUI tests |
| 4.17.6 | **The Configurations window** (following C87: `app/configurations_window.rs`) with the list, the details, and the import and export dialogs; the GUI tests | A53-A57 |
| 4.17.7 | **Models and builds on import**: the buttons on the notes (the Recommended window's download, the engine's pull, Look up online, the manager's filter) and the record of where a download came from (`sources.rs`) | A60-A64 |
| 4.17.8 | **Companions and the gate**: Open WebUI's shares, W16, `companion_address` and the first address; PRIVACY.md and the docs | A65-A68 |

**The test cases.** Group A of the test plan has 75 rows (A01-A75); the tests are `core/tests/p3_configurations.rs` (the document, `share`, capture and
apply, the notes, the fifth engine), `core/tests/reference.rs` and `purity.rs`, and, in the application, `configurations.rs`, `cli.rs`, `settings.rs`,
`sources.rs`, `app/configurations_ui.rs`, `app/configurations_window.rs`, `app/engine.rs` and `app/companion.rs`. Every group was verified by deliberately
changing the code to confirm that a test fails (the mutations are recorded in the run log of the implementation plan).

## 14. Exclusions and risks

**Deliberately left out:** synchronization between computers; signatures and author verification; a `quickai://` link and import by address; automatic
updates of a shared configuration; *Use and start* in one click; switching by model; running several engines at once (the `voice-chat` sketch of stacks
belongs to Phase 6 and would add `[[unit]]`, in a later schema); the models a ComfyUI workflow needs (a ComfyUI configuration specifies how to run it, not
which models its pages use); the theme; a run on Windows or macOS (the paths are the platform's, but nothing was run there, as with the other work);
`argument_unknown` and a version comparison (WP4.4).

**Risks, and how they are mitigated:**

* A false badge (a value the form recomputes when a model is picked, a model the window cannot hold, a build that is not installed, or a value the file
  states explicitly that equals the default). The capture takes stored values only, a configuration without a model accepts any model, the comparison is
  made on the values in force, and A17, A50 and A75 cover it.
* Ollama's models cannot be listed while it is stopped. The note says so (`model_unseen`) and never claims a model is missing.
* Native file dialogs cannot be driven by the GUI tests. Both dialogs have a path box; the dialogs behind *Browse* are tested by hand.
* The adoption writes a file nobody asked for. It is written to the user's own configuration directory, noted in the log and in this document, and only on
  the first start.
* More questions in the window (unsaved changes, replace, reset, revert, delete). Each is a confirmation with *Cancel*, like those the window already has
  (the foreign program, the delete).
* A configuration that names an engine field renamed in a later version: reported and ignored (`unknown_setting`), never an error, so old files still
  open.
* The page has an additional section below the title row, 44 pixels high: the GUI tests that read the page by position were recalibrated.
* A badge that appears while the user types: the section reserves space for it, and A73 compares the positions of everything before and after.

## 11. Running a configuration without a window

`quickai-headless up <configuration>` (WP4.18, [headless.md](headless.md)) takes the same file and performs what the note buttons do, one after another: it
installs the build the file names, or the one its engine's document chooses, fetches the models, starts the engine and the program that accompanies it,
and keeps running until it is told to stop. What distinguishes this from rule 1 above (a configuration *selects*, and the window installs only when a
button is pressed) is that the user explicitly runs the command: the program prints what it is going to do, and how large it is, before doing it
(`quickai-headless check` stops at that point), and a file from outside that asks about free text or wider access is not used unless `--accept-review` is
given. It never writes the working state (`settings.json`) or the user's configurations. For an engine that runs on its own (ComfyUI), the models are the
file's `[[model]]` entries, placed in the folders the engine reads from; the window writes one model when it exports (with the source, the path in the
repository and the revision it was fetched at), so a set of files (the three of a video model) is written by hand, or kept by the window as long as its
first model stays first. Anything that belongs to the computer running it (a folder, a login, a mirror, the variables of a program's environment, a copy
of a file that is already present) is provided by the person who runs it, in a file that `quickai-headless init` writes, and a configuration can state what it
needs from them (`[[needs]]`): [headless.md](headless.md), sections 6 to 8.
