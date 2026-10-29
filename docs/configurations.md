# Configurations

A configuration is a whole QuickAI runtime in one small file: the engine, the model, the build and the settings, chosen from a dropdown, saved under a name and shared with others. This page says how they are made, checked and shared safely.

## 1. In short

A **configuration** is one small text file that says what QuickAI runs and how: the engine, the model, which build of the program, the engine's
settings and command-line arguments, and what goes with it (Open WebUI beside Ollama). The window always has one **active** configuration. A
dropdown in a section of its own under the title row chooses it, and says in words when what the window holds is no longer what was saved; a window of its own,
built like the Settings, keeps them: new, save as, rename, duplicate, delete, import, export. A file can be given to someone else, and when it is opened there QuickAI says, before anything is taken, what that computer lacks or does
differently.

Four rules shape everything below.

1. **A configuration selects; it does not install.** Choosing one sets what the window holds. It starts nothing, fetches nothing, installs nothing.
   What it needs and the computer lacks (a model, a build, Open WebUI) is listed, and fetched when the person asks (a button on the note).
2. **It never carries what belongs to a computer or to a person**: folders, program paths, the programs the person allowed to run, accounts, secrets, the theme.
   (The llama.cpp document says it already: "a settings file copied to another machine must not carry a path".)
3. **A file from outside is not trusted until it is read.** Text that becomes command-line words or environment, and the few settings that widen
   what a program may do, are shown in full and taken only when the person says so.
4. **Nothing changes for a person who does not use it.** `settings.json` stays the working state it is; the only new key is the id of the active
   configuration, and it is absent while that is the default, so such a person's file is byte for byte what it is today.

The architecture reserved this in Q10 ("a selection already is a profile; save/load UI later, in Settings first") and the loader already knew the
`profile` kind, so this is the last piece of a design that was waiting, not a new one.

## 2. What is data and what is code

The user asked for a data-driven design. This is the line between the two:

| What | Where it is | What the code does |
|---|---|---|
| A configuration | a `profile` document (TOML): `manifests/profiles/default.toml` inside the application, `<exe>/config/profiles/` from a package, `profiles/` in the person's configuration directory | loads it through the loader every document goes through: the envelope, the layers, diagnostics with file and line, `QUICKAI_SAFE` |
| Which settings a configuration holds, leaves out, or asks about | the **`share`** of each `[[field]]` of the engine documents (§4); from the field's `type` when the document says nothing | reads it. No engine, field or program is named in the code (a test reads the code to be sure: A54, A65) |
| What a companion adds | the companion's document, in the same way | the same code |
| The kinds of name a model has | the engine's `provides.accepts.refs` (`ollama`) and the recommended list | resolves a reference with them |
| How a build is recognised, and its backend judged | the engine's `[acquire]` (label rule, backends) | resolves a label with it |
| The sentence of each note (§7) | `[[finding]]` in the vocabulary | picks one and fills it in |
| Names never taken from a file (`LD_PRELOAD`...) | `[profile] deny_env` in the vocabulary, on top of a short floor the code keeps (`LD_*`, `DYLD_*`, `PATH`, `PYTHON*`) | refuses them |
| The screens | nothing about any engine | draws the store, the notes and the form model |

The proof is a test: an engine that exists only as a document (a fifth, `echo`, with a companion of its own) is captured, applied, written out, read in and
checked with no change in code (A10).

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
# from = "huggingface:owner/repo"         # for a file that is no entry of the list: where it can be had; the window says it and fetches on a click, `quickai-run` fetches from it (headless.md, section 6)
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

Every key is on the generated page [reference/profile-fields.md](reference/profile-fields.md) (`quickai config docs profile`). In short:

| Key | Meaning |
|---|---|
| `schema`, `kind`, `id`, `requires` | the envelope of every document. `requires.app` is the lowest version that reads the file; a file for a newer one is refused with the sentence that says to update |
| `name` | any text up to 60 characters (control and direction characters are taken out). The file's id is made from it (lower case, letters, digits, `-`; a number is added if it is taken); the name is never used as a path |
| `[meta]` | `note` (typed by the person who exports), `made_with`, `made_on`. No user name, host name or folder is ever written |
| `engine` | an engine a person can choose; a companion is not one |
| `[runtime]` | `build`, `version`. Absent: the program this computer uses by default (for llama.cpp, the one the package ships). `[runtime.<program>]` pins the build of a program that goes with the engine (`[runtime.openwebui]`): only `quickai-run` installs from it |
| `[[model]]` | `role`, one source (§5), `size`, `sha256`, `name` for people, and for a file where it can be had from: `from`, `path`, `revision`, `into`, or `url`. Several are allowed; the first is the one the engine loads |
| `[[needs]]` | what whoever runs the file has to give besides it: a login (`token`), a setting of a computer (`setting`), a variable of a program's environment (`env`, and `for`), each with `why` (headless.md, section 7); and what the computer has to have (`system`: the id of a prerequisite of the engine's document, such as `c_compiler`, which the document says how to look for and what brings: headless.md, section 12). Only `quickai-run` asks for them; the window keeps them when it saves |
| `[values.<engine>]` | field values by field id, for the engine and for its companions |
| `[app]` | `traffic_proxy` |
| unknown keys | kept when the file is written again (at the top and in `[meta]`) and told as a warning, as in every document |

**Where.** One file per configuration: `profiles/<id>.toml` in the person's configuration directory (`~/.config/quickai` on Linux). The layers are the
usual four. The application has `default` inside it; a package may ship more in `<exe>/config/profiles/`; a file of the person's with the same id replaces it
(Save writes `replace = true` when a layer below has the id, so the two are never merged), and deleting that file brings the built-in one back. `QUICKAI_SAFE=1`
ignores the person's layer, which is also the way out of a configuration that makes QuickAI misbehave (in safe mode Save and import say so and write nothing).
The dropdown says where each one comes from: *built in*, *from this package*, *yours*; the window marks only what is not the person's own (C188).

**The Default** is `manifests/profiles/default.toml`: the engine llama.cpp, no model (the application chooses, as it does on a first run), no values (every
field at its default). It can be saved over like any other, and **Reset to factory** puts the built-in one back.

## 4. What is in a configuration

Not a copy of `settings.json`: the part of it that describes a run and travels.

| State today | Kept in | In? | Why |
|---|---|---|---|
| The engine | `values."app.engine"` | yes | `engine` |
| Numbers, choices and flags of the engine's fields (port, context size, GPU layers, GPU, memory, mode...) | `settings.json` through `legacy_key` or `values` | yes, those that differ from the default | `[values.<engine>]` |
| Free text that becomes command-line words or environment lines (*Extra command-line arguments*, *Extra environment*) | the same | yes, **looked at** | the user asked for command-line arguments; a file from outside must not slip them in |
| Settings that widen what a program may do (internet access *online*, Ollama's own store, the page's Download button) | the same | yes, **looked at** | `share = "review"` |
| The model | `last_model` | yes, as who it is | §5 |
| What goes with a model (a projector, a mode such as embedding) | the record beside the weights | no: it follows the model | the catalog entry and the record say |
| The build of the program | `bin_override` | yes, as a label | §5 |
| Folders (models, outputs), a program or a Python path, a custom `bin_override` | `models_dir`, `python`, ... | **never** | they belong to the computer; with them in, a file would break on the first other machine |
| Open WebUI's box, port, internet access | `values` | yes | §9 |
| Open WebUI's install, data, accounts, stored settings, extra environment | `~/.quickai/openwebui` | **never** | an environment is not a setting (§9) |
| The live-traffic proxy | `enable_traffic_proxy` | yes | it changes the run (a hop in front of the engine) |
| The programs the person allowed to run | `values."app.confirmed_engines"` | **never** | consent is the person's, not a file's |
| Light or dark theme | `dark_mode` | no | the person's, not the run's |
| Downloaded models, installed builds | the disk | no | a configuration selects (rule 1) |

**`share`** is a key of `[[field]]`: `yes`, `review` or `never`, with an optional `share_note` (the sentence shown when it is asked about). When the
document says nothing the type decides, so that no document of before needed a change and a new field is private until a document says otherwise:

| Type | `share` by default |
|---|---|
| `int`, `float`, `bool`, `choice`, `port` | `yes` |
| `text`, `url`, `kv_lines` | `review` (the value reaches a command line, an environment or a host) |
| `dir`, `file`, `runtime_path` | `never` |
| `hidden`, `model_ref` | not a value: the model section, or nothing |

A document that says `share = "yes"` of a folder, or `share` of a hidden value, is told (a warning): the first would fail on the first other computer, the second does nothing.
What the four shipped documents say is held by a golden test (A08):

| Engine | `yes` | `review` | `never` |
|---|---|---|---|
| llama.cpp | port, context size, GPU layers | extra command-line arguments, extra environment (C189) | models folder, program |
| Ollama | mode, port, context size, GPU | extra environment, *use Ollama's own store* | model store, program |
| ComfyUI | mode, port, GPU, memory | extra arguments, extra environment, internet access, the page's Download button | present the GPU as (names this computer's card), models folder, output folder, Python, launcher |
| Open WebUI | box, port | internet access | Python, extra environment (§9), launcher |

**Only what differs from the default is written.** A default can move between versions, and a computed one (llama.cpp's context size follows the model and
the memory) must not be frozen at the exporter's number. For a setting that has always been in `settings.json` the default is what a first run stores; for
another it is the document's literal default.

## 5. Models and builds

A configuration says *who* a model is, never where it is on a disk.

| Form | Means | Is it here? | If not |
|---|---|---|---|
| `ollama = "qwen3-vl:8b"` (the key is a kind of name the engine takes) | a name the engine's daemon lists and pulls | in the list it gave last time (it cannot be told while Ollama is stopped, and the note says so: it is never called missing then) | the note has **Get**, enabled while that engine runs, which pulls it |
| `catalog = "lfm2.5-vl-450m"`, `file = "...Q4_K_M.gguf"` | an entry of the recommended list and one of its files; its projector follows from the entry | the file is in a models folder at the size the entry pins | the note has **Get**, which is the Recommended window's own download (with the projector when the list pairs it), after a click |
| `file = "name.gguf"`, `size`, `from` | a file of the models folder, by name only (a name with a separator, `..`, a drive or a direction character is refused) | a file of that name and size | `from` known: the note says where it can be had; **Look up online** with the repository's name in the box. Not known: the same button with the file's name, and the note says its source is not known |

The third form says where the file can be had from, because **a configuration never carries a model** and a computer that has not got the file must be able to get it: `from` (a kind of place and a name in it:
`huggingface:owner/repo`), `path` (the file in that repository, when it is not at the top), `revision` (a branch, a tag or a commit; a commit is the same file for ever), `into` (the folder of the models folder, for an engine that keeps its
files by the folder: it is one the engine's document names) or, instead of `from`, `url` (`https://`, no login in it, no fragment; this computer's own over `http`). None of them takes a path of a computer. The window **says** it (the note has
the repository and the file in it) and fetches on a click, as it always did; `quickai-run` fetches from it, after a file from outside has been read ([headless.md](headless.md), section 6). The recommended list keeps its own way, `catalog`.

An export finds a `catalog` entry by itself: the file's name and size are looked for in the loaded catalogs, which pin them. **A fetched file remembers where it
came from**: when a download of files ends, the application writes the name, the size and the place (`huggingface:owner/repo`) into
`model-sources.json` in its own data directory. (Not beside the weights: no models folder gets a file it did not have, and the tests of the folders'
contents are as they were.) With the place it keeps where in the repository the file is when that is not the top, and the revision when it is not `main`, so that a file of a folder (the VAE of Wan 2.2 is in `VAE/`) is
exported with the `path` that fetches it. The export puts it in the file as `from` (and `path`, `revision`), which the window says and never fetches from by itself; a file fetched before, or put there by hand, is
exported by its name and size and the note says its source is not known.

**Builds.** `runtime.build` is the label of a build in the manager's list (the engine's label rule makes it from the release and the file). Choosing the
configuration sets the engine's program to the installed build of that label (the only way `bin_override` is ever set by a configuration, and never to a
path the configuration gave); a configuration that names no build leaves the program as it is. A label that is not installed is a note with the labels that are
(or that none is), and **Look up builds** opens the Settings with the label in the manager's filter: the person presses the manager's own look-up. The
backend of an installed build is judged against this computer by the manager's own function (`gpu::mismatch_warning`).
`runtime.version` is kept for the notes; comparing it with what the installed program reports waits for the engine's `--version` to be asked
(WP4.4), so for now builds are compared by label.

## 6. Choosing, changing, saving

**The state.** `settings.json` is still what a run uses and still written as it is: it is the "implicit profile" of Q10. A configuration is the saved
form of its portable part. The window computes one function, `capture(working state)`, and compares it with the saved configuration: when they differ the
section says **• Modified** in amber after the dropdown, "changed since it was saved", with what changed on hover. The comparison reads the live state, not the file, and it is made
on the values in force and not on how a file spells them (C116).
A configuration that names no model does not mind which one is selected (the application chooses one when nothing was chosen, and its choice is not a change), and
a model it names that the window cannot hold (not here yet, or not listed because the engine is stopped) is not counted either, nor a build it names that is not installed here (the program
is left as it was when it is chosen), nor a value the file says that is the default (a file written by hand, or by another version): a configuration that was just chosen is a fact, and is never
said to be modified for what this computer lacks (A75).

| Action | What happens |
|---|---|
| **Choose** (dropdown, or *Use* in the window) | refused while something runs or is fetched (the rule of the Engine row, `engine_locked`), with the same sentence; with unsaved changes it asks *Save / Don't save / Cancel*; the notes (§7) are made for this computer: if one stops it the window says why and stays as it was, otherwise the configuration is applied, `settings.json` is written, the engine's state is reloaded, and the notes that remain are in the log and in the window |
| **Apply** | sets the engine, every field of the engine and its companions that the configuration lists, **and returns every other such field to its default** (otherwise two configurations would bleed into each other and "the same runtime" would depend on what was chosen before); the model is picked if it is here (or cannot be told); the build is chosen if it is installed; the traffic proxy is set. It does not touch folders, program paths, other engines' values, the theme, or the consents. The new state is made whole before anything is stored |
| **Save** | writes the active configuration from the working state, whole or not at all (a temporary file, then a rename) |
| **Save as...** | asks a name (an empty one, and one in use in any case, are refused in the box), writes a new configuration, makes it active. The one left behind stays as it was saved |
| **Revert** | asks (*Go back to what was saved?*, in the window where it was pressed), then applies the saved configuration again; it is a button of the section while there are changes, and of the card of the one in use in the window |
| **Rename**, **Duplicate**, **Delete** | rename changes `name` only; duplicate asks a name; a built-in or a package's configuration cannot be renamed or deleted (save it under another name); deleting the one in use asks, and then the Default is the one in use and **the window keeps what it holds** (applying the Default would change what a person runs because they deleted a file) |
| **Reset to factory** | on the Default of the person's, after a question: removes their file, and the built-in one is the Default again |

Choosing starts nothing. As with choosing Ollama today, the person presses Start.

**Start-up applies nothing.** The working state is the truth; start-up only reads which configuration is active and loads it as the baseline for the badge. One
that is gone (a deleted file, a safe-mode start) falls back to the Default with a line in the log, and the working state is not touched.

**The first start with configurations** *adopts* the working state: if it differs from the built-in Default, it is written once as the person's Default
(a file in their own configuration directory, said in the log), so that nobody who tuned their settings sees *Default* as modified on the first morning. The folder
`profiles/` is what remembers it was done: it is made then, and where it is, nothing is written. A person whose settings are what a first run holds gets
the folder and no file. What is adopted is **what the person had set, the settings as the file had them** (C112): the model the window picked for someone who never chose one
is not theirs and is not in their Default (a Default that names no model does not mind which is selected, C108); a model they had chosen is named as the window
compares it (the file and its size, not the entry of the recommended list that has a file of that name). The first run of the GUI suite found it: a Default with the model the
window had picked showed the unsaved mark at once (a dot, then) when the file was not the one of the list.

## 7. Sharing, and the notes

**Export.** From a row of the window: a name, a note for the person who gets it, a file (a path box with *Browse*), and the text that will be written,
shown. Under it, what was left out: *Settings left out because they belong to this computer (a folder, a program): 3*, *Lines left out because they look like secrets
(a key, a token, a password): 1*, *X has no known source*. Secrets are never written: an environment line or an argument whose name or flag contains `KEY`, `TOKEN`,
`SECRET` or `PASSWORD` (`is_secret_name`) is left out and said. The file is written where the person chose and nowhere else; a file that is there is replaced only
when the person ticks that.

**Import.** A path box with *Browse* (a native file dialog cannot be driven by the GUI suite, so the box is also how the cases work), then the **report**,
before anything is stored: the name and what the file says of where it was made, *This computer:*, the settings that would change, the notes, the items to
tick, the name (with the three answers when it is in use: *Keep both*, *Replace*, *Cancel*), and **Import**, **Import and use**, **Cancel**.

Nothing is written until **Import**. What is not ticked is not stored; what is stored is the person's own configuration from then on.
Checking, previewing and reading never use the network, start a program or write a file (A33, A48).

**The notes.** Four levels: **Stops** (the file cannot be used here; nothing is written), **Asks** (a yes is needed), **Warns** (it differs; it can be used),
**Says** (information). The ids are stable names; **the sentences are the vocabulary's** `[[finding]]` (a vocabulary that gives some gives all of them, and the words of a
sentence are the holes its note fills in), **and the levels are the code's**: a document, which may come from anyone, can reword a note and cannot turn a question into a statement. A golden holds the lot (A31).

| Id | Level | When | What the person is told |
|---|---|---|---|
| `not_a_configuration` | Stops | not TOML, not a `profile`, no id, no engine, over 256 KiB, a model or a name that is a path | This file is not a QuickAI configuration (why). |
| `too_new` | Stops | `schema` or `requires.app` above this version | This configuration was made by a newer QuickAI: it needs QuickAI >=0.3, and this is 0.1.0. Update QuickAI to use it. |
| `unknown_engine` | Stops | an engine this build has no document for, or a companion named as the engine | It is for `whispercpp`, which this QuickAI does not have. |
| `free_text` | Asks | `review` text (extra arguments, extra environment) | the field's own `share_note` (*Lines of environment, set for Ollama as they are written.*), else the vocabulary's; the text is shown in full |
| `wider_access` | Asks | a `review` field that is not text (internet online, Ollama's own store...) | the field's own sentence |
| `name_taken` | Asks | the id or the name exists | Keep both / Replace / Cancel |
| `refused_line` | Warns | an environment name from the deny-list, or one the engine protects | `LD_PRELOAD` cannot be set from a configuration: the line is left out (whatever is ticked) |
| `names_a_path` | Warns | free text names an absolute folder | names `/home/alice/x`, a folder on the computer it was made on |
| `unknown_setting` | Warns | a value for a field the engine does not have, or of an engine that does not go with this one | `ollama.flavour` is not a setting this QuickAI has: ignored |
| `private_setting` | Warns | a value for a field whose `share` is `never` | belongs to a computer and is not carried: ignored |
| `bad_value` | Warns | out of range, wrong type, not an option, too long | Context size: 99999999 is not accepted (more than 131072 or less than 256); the default is used |
| `unknown_keys` | Warns | keys this version does not know | kept in the file, ignored |
| `other_system` | Warns | another OS or architecture | Made on Windows x86_64; this computer is Linux x86_64. Arguments and builds made there may not exist here |
| `other_gpu` | Warns | the build's backend needs a GPU vendor this computer lacks | the manager's own sentence |
| `memory` | Warns | the model does not fit (`heuristics::fit`, the Recommended window's rule, now in the core) | needs about 14.4 GB of memory, and this computer has 8.0 GB |
| `model_to_get` | Warns | a model that is not here and can be had | Not here: qwen3-vl:8b (5.7 GB). It can be got from Ollama. (**Get**, **Look up online**) |
| `model_unknown` | Warns | not here, no known source | X (2.6 GB) is not here, and the file does not say where it came from. (**Look up online**) |
| `build_missing`, `build_none` | Warns | the label is not installed | Made with v0.40.0, which is not installed here. The builds that are: ... (or: and no build of this program is). (**Look up builds**) |
| `port_taken`, `ports_clash` | Warns | a port the file sets is in use now; two ports in the file are the same | |
| `companion_missing` | Warns | Open WebUI wanted and not installed | Open WebUI is switched on and is not installed here. (**Set up**) |
| `companion_address` | Warns | §9 | Open WebUI keeps the address of Ollama it was first started with (...); it would be given ... now |
| `model_unseen` | Says | Ollama is stopped | Ollama is not running, so it cannot be told whether X is here. Start Ollama to see. |
| `will_change` | Says | always | the list of settings that change (six, and how many more) |
| `left_out_private`, `left_out_secrets`, `left_out_source` | Says | an export | what stays out of the file |

Two more notes wait for the engine's `--version` and `--help` to be asked (WP4.4): `argument_unknown` (an argument the installed build does not have) and a comparison of versions.

## 8. Safety

A configuration comes from a stranger as often as from the person. The threat is the file, and the answers are in the design, not in a prompt.

* **It is data.** The `profile` kind is data-tier: it can pick things the registry already knows and cannot name a program or a host to run. A model is a name, a catalog id or a file name, and may say where the file can be had from (`from` and
  its `path` and `revision`, or an `https` address without a login), which the window says and `quickai-run` fetches from only after the person has read it (a file from outside; C122, C125); a build is a label of an installed build. The model is never in the file, and
  no key takes a path of a computer.
* **Free text is looked at.** Everything that becomes a command-line word or an environment entry is shown in full and is taken only if ticked. Unticked,
  it is not stored. Lines the engine protects (`OLLAMA_HOST`, the isolation of C92) cannot be set by a file, as they cannot by the person; names on the vocabulary's
  deny-list, and a floor the code keeps that no document can lower (`LD_*`, `DYLD_*`, `PATH`, `PYTHON*`), are refused even when ticked, and again when a file that was put in the folder by hand is chosen.
* **Limits.** 256 KiB; strings of at most 4 KiB; at most 512 settings and 8 models; values are held to their field's type, range and options; model names and labels hold no separator, `..`, drive, NUL or direction character; names are shown
  with control and direction characters removed; a number that wraps round to a schema is not one. A table of hostile files (A32: nesting 100 000 deep, a million keys, a name of 100 000 characters...) is refused or cleaned, never a crash.
* **No network, no process.** Reading, checking and previewing do neither; the core cannot (`tests/purity.rs` holds it, A33). The network is used only by what the person starts: *Get*, *Look up online*, the manager's look-up.
* **Nothing private is written.** The export is built from an allow-list (fields with `share = yes`, and `review` ones the person kept), so a field added tomorrow is private until its document says otherwise.
* **The way back.** `QUICKAI_SAFE=1`, the Default's *Reset to factory*, and the fact that nothing is started or installed by choosing.
* **Provenance is not claimed.** There is no signature and no author check; the report says what the file says about itself and nothing more. Signing waits for the content packs of Phase 6.

PRIVACY.md has a paragraph: configurations are files in the person's configuration directory; export writes where the person chose; import reads what the person chose; nothing is sent; `model-sources.json`.

## 9. Open WebUI, and the companions

The user: "for something like Open WebUI the environment may not be mutable, so leave this out if not possible". **It is right, and it was checked in the release's own source
(open-webui 0.11.4, the one the document pins) and then by running it (W16 of [engine-facts.md](engine-facts.md), A67):** `models/config.py` keeps a table `config` in Open WebUI's database, and
`seed_defaults` says "Existing DB values take precedence over defaults": the environment gives the values only the **first** time. The settings QuickAI gives it in the
environment that are kept this way are `OLLAMA_BASE_URL` (`ollama.base_urls`), `ENABLE_OLLAMA_API`, `ENABLE_OPENAI_API`, `RAG_EMBEDDING_ENGINE` and `RAG_EMBEDDING_MODEL`, and so are many of the settings a person would type in *Extra environment* (which ones is for Open WebUI's
own documentation to say; its code does not tell them apart by name). What is read at **every** start is `OFFLINE_MODE` (the *internet access* field), `CORS_ALLOW_ORIGIN`, `DATA_DIR`
and the like, and the port is a command-line argument.

So, for a companion:

* **In a configuration: only what QuickAI sets at every start**: Open WebUI's box (`enabled`), its `port` and its internet access (`review` when online).
* **Never**: its build and install (an environment of its own), its data (accounts, chats, documents, what its administrator set in its pages), its Python and launcher paths, and **its
  extra environment**: a line there works for some settings and not for the ones Open WebUI keeps in its database after its first start, and a configuration must not promise what depends on that.
  The document says so with `share = "never"` and a `share_note`; the code knows nothing of Open WebUI (A65).
* **The wish needs the program.** A configuration that turns Open WebUI on, on a computer that lacks it, is the note `companion_missing` (**Set up** opens its section). Nothing is installed by choosing.

**A finding in what was already built (WP4.16), now remembered.** Open WebUI uses the address of Ollama it was given **at its first start**
and keeps it: if Ollama's port is changed afterwards (by the person, and now easily by a configuration), it goes on calling the old one until the address is
changed in its *Admin settings, Connections*. QuickAI now writes down the address Open WebUI was first started with (in the field its document names, `first_address_field = "first_ollama"`: a hidden value of QuickAI's own,
never in a configuration; Open WebUI's database is not opened), says in the log at each start when the address it would give now is another, and the notes of a configuration that would put Ollama elsewhere say the same
(`companion_address`, A66). `ENABLE_PERSISTENT_CONFIG=false` was rejected: every setting the person made in Open WebUI's pages would be forgotten at each start. What could be done later, if the person wants it: Open WebUI always talks to a fixed loopback
port that QuickAI forwards to the current Ollama (the gateway of the live view does that kind of thing already), so that the stored address never goes stale.

ComfyUI is not like this (it keeps nothing the environment cannot set), and llama.cpp and Ollama read their settings at every start.

## 10. What a person sees

**The section.** The title row has the heading, the tabs of the live traffic (when it is on) and the four icons, as it had before there were configurations. Under it, on the Main tab, is a section as wide as the page,
framed as the other sections of the page are (C114):

```
 QuickAI                                                                          speed  gear  info  moon
+---------------------------------------------------------------------------------------------------------+
| Configuration:  [ Vision chat                      v ]                                                  |
+---------------------------------------------------------------------------------------------------------+
```

and while what the window holds is not what the configuration in use says, in an amber outline with a faint amber fill (a deeper amber on the light theme), the word and the three things to do about it (C115):

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

The user's words, looking at the first version, which had the dropdown in the title row and a small amber dot after the name in it: "move the dropdown to a different section, doesn't look good at the location - right next to the header.
Additionally, when user makes an edit, it has to show the configuration as edited - or find other way to specify that it has changed. i.e. If I load a configuration, it is a fact - but the moment I change, It should
convey that it is modified". A quiet section is a fact, so it says nothing; a change is loud: a word and a colour, and what to do. The word is a badge of its own and not a mark after the name, because a long name is cut
by the dropdown and would take the mark with it. *Save* writes over the one in use, *Save as...* asks a name, *Revert* asks (*Go back to what was saved?*) and goes back; the dropdown still has *Save*, *Save as...* and *Manage configurations...*
under its list, and the row of the one in use says *(modified)* after its name. The bullet is amber and a character the fonts have (the filled circle is drawn as an empty box, so the mark is U+2022; a test asks the fonts, A50). The dropdown is greyed, with the
Engine row's sentence on hover, while something runs or is fetched, and *Revert* with it. A hover on the badge or on the dropdown says what changed since the save, as a person says it (*Port: 8080 to 9000*, the first eight and how many more).
The question when a choice would lose changes, the question of *Revert* and the box that asks a name are modals of the main window.

**Nothing moves when a change is made.** The section keeps the room for the badge and the buttons all the time, the dropdown is where it was and as wide, and the section is as high, so that the field a person is typing in,
which is under it, does not slide when its first keystroke makes the badge appear (A73: the label, the dropdown, the title row and what is under the section are at the same places in both states, at three widths).
The window can be as narrow as 520: the dropdown gives up its width and then the word *Configuration:* when there is not enough room for all of it with the badge and the buttons (`section_shape`; A72 checks the parts are in order,
apart and inside the outline from 520 to 1400, with the tabs on and off). A name that is longer than the dropdown is cut at its width with an ellipsis (a name is 60 characters at the most; egui's dropdown grows to the length of its text unless it is told to
cut it, and the first version pushed the badge and the buttons out of the row at 520: found by looking at a name of 57 characters), and the hover has the whole name. The Live tab, a window on the traffic and not on the run, has no section.

**The window** *QuickAI Configurations*: a viewport of its own, like the Settings (C87: 780x700 to open at since C188, the window manager's title bar, **Close** at the foot with the last thing that was done, asked for again it comes to the front,
closing it any way writes what was changed). Since C188 each configuration is a card (the user's choice of four mockups, `mockups/configurations/` beside the repositories):

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

- **The badge** is the first letter of the engine's name on a tint of its own (by the engine's place in the selector, so no two of this QuickAI share one: `theme::tint`); an engine this QuickAI does not have is an outline.
- **The tags** after the name: *In use*, *• Modified* while what the window holds is not what was saved, *built in* or *from this package* (the person's own say nothing: most are), and how many notes warn about it here. When the name has
  no room for them (a narrow window, Revert and Save beside them) they give way one by one: the count of the notes, then where it is from, then *In use* (the green frame says it too); *Modified* never does.
- **The line** under the name: the engine, the model by its name in the recommended list when it is one of it (`Qwen2.5-Coder 7B Instruct`, not the file), else the name the file gives or the engine's own, *any model* when it names
  none, and *with* each program that goes with the engine and is switched on. One that the notes stop has the first of them instead, and is grey with no Use.
- **At the right**: *Use* (grey with the Engine row's sentence while something runs; its hover says what using it does and lists what would change, the first eight); on the one in use, while it has changes, *Revert* and *Save*
  (the only green button of the window: the suites find the main button by its colour); then the **...** menu: *Rename...*, *Duplicate...*, *Export...* and *Delete...* (*Reset to factory...* for the Default); what is
  built in or from the package cannot be renamed or deleted, and says why on hover.
- **A click on the card** (not on its buttons) opens it under its first rows: the note of whoever made it, the computer it was made on when that is another kind than this one (another system, processor or GPU), what it
  sets as chips (the label without the hint in brackets, a program that goes with the engine named before it; the switch that turns that program on is the line's to say, not a chip), the program it was made with, and the
  notes with a warning sign and their button at the right. One with nothing set says *Every setting at its default.* Nothing is open when the window opens.
  Since C189 the chips begin with what goes with the model on its command line, which a configuration does not carry (it follows the model, §4): each
  file fetched with it by the part's label and the file's name (*Projector mmproj-Qwen3VL-2B-Instruct-F16.gguf*), from the record beside the weights when
  the model is here and from its entry of the recommended list when it is not, and *Started as* with the label of the mode it is started in (*embedding
  server*). The whole command line is on the page *Starting it* of the Settings ([implementation-plan.md](implementation-plan.md), C189, C190).
- **Left out** since C188: the arrows before the names, *yours* on every row, *N settings differ from what you have set now* (the hover of Use lists them), the line of the computer it was made on for one made on this kind, and
  **Save here** (it replaced another configuration with what the window holds: *+ New from current* and then *Delete* do it, and it read too much like Save). The files that could not be used fold under
  *N file(s) couldn't be used*, with the reason when opened.

The list, the details, the notes and the import and export dialogs are built from the store, the checks and the form model: no word of an engine is in the code. *Use* is grey with the reason on hover when the notes stop it
or something is running; the buttons on a note are grey, with the reason, when the configuration is for another engine than the one in use. The cases look at every view (`look-at-every-view`): the dropdown open in each state, the list scrolled to its end with many configurations and long names, each dialog, a narrow window (440 px: A57); the cards were also looked at in both themes at 780 and 500 wide.

## 11. From the command line

For scripts, for support, and as the way the whole thing is tested without a window:

```
quickai configuration list
quickai configuration show <id | file>
quickai configuration check <file> [--strict]          # the notes for this computer; exit 1 if one stops it (and, with --strict, if one asks or warns)
quickai configuration export <id> [-o file] [--note TEXT]
quickai configuration import <file> [--as NAME] [--replace | --keep-both] [--accept-review]    # what is asked about is not taken without --accept-review
quickai plan --configuration <id | file> [--explain | --json]   # the run plan it makes here; what is said on the line wins
quickai config docs profile                             # the reference page
```

`use` is left out on purpose: the window writes `settings.json` when it changes and when it closes, and a command that wrote it under a running window would be undone by it.

## 12. Decisions

| Id | Decision | Why, and the alternative |
|---|---|---|
| C98 | **The word.** *Configuration* in windows, the command line and the docs for people; `profile` stays the name of the document kind. | It is the user's word. The code and the logs already say "configuration" for the layers of documents (`config check`, `QUICKAI_CONFIG_DIR`), so a kind of that name would mean two things; the kind was reserved as `profile` (Q10) and nothing used it. Alternative: rename the kind now |
| C99 | **A configuration is a `profile` document of the data tier**, one file each, in the existing layers. | The loader, the diagnostics, safe mode and the generated reference page come with it |
| C100 | **Presets, not auto-save.** `settings.json` remains the working state; a configuration is saved on purpose (Save, Save as); the section says *Modified* while there are changes (first a dot in the title row, C115); choosing with unsaved changes asks. | The user wrote "save by a different name", and it is the only model in which a person can try something and keep the original. Alternative: the active configuration follows every change; it can be a switch later, as it is the same write |
| C101 | **The Default is a document, can be saved over, and has *Reset to factory*; the working state is adopted into it once.** | No dot on the first morning; nothing hidden |
| C102 | **What is in is declared by the documents** (`share`, by type when silent). Folders, program paths and consents are never in. Only what differs from the default is written. | Data-driven; a new field is private until said otherwise |
| C103 | **A configuration selects and does not install.** Models and builds are references; nothing is fetched until the person clicks. | The user's "same runtime is available there" is met by saying what is missing and making getting it one click, not by doing it unasked (the rule of C93) |
| C104 | **A file from outside is read before it is taken**: free text and wider access are ticked one by one, the deny-list is absolute, limits, no network. | See §8 |
| C105 | **A companion contributes only what QuickAI sets at every start**, never its environment; the address Open WebUI keeps is remembered and told. | See §9 |
| C106 | **Choosing is refused while something runs or is fetched**, as the engine row is, and starts nothing. | The same rule the person already knows. Alternative: *Stop and switch* as a button, later |
| C107 | **The sentence of a note is the vocabulary's and its level is the code's**, and the code keeps a floor of names a file may never set that no document can lower. | A document, which may come from anyone, must not turn a question into a statement, or take away the names that make a program load other code |
| C108 | **A configuration that names no model does not mind which one is selected**, and a model it names that the window cannot hold is not a difference (`matches_run`). | The Default names none, and the application chooses a model on a first run: without it every fresh install showed the Default as modified, which the window found in its first run |
| C109 | **Where a file was fetched from is kept in the data directory** (`model-sources.json`), and a configuration carries it as a hint (`from`), said and never fetched from by itself. | The plan said "in the record beside the weights"; that record is written for bundles and modes only, and a record for every download changes the folders that tests and people look at. A data-tier document must not name an address (changed for the headless program by C125: a file may say where its model can be had from; the window still fetches only on a click) |
| C110 | **Deleting the configuration in use makes the Default the one in use and leaves the window as it is.** | Applying the Default would change what a person runs because they deleted a file. The plan said "chooses the Default" |
| C111 | **The field that remembers a companion's first address is named by the companion's document** (`[engine] first_address_field`, a `hidden` field of the same document), not by the code. | The first version of the code said `first_ollama`; the source scan of the configuration code (A54) found an engine's name in it, and the same word was in the companion's own code. A document that names a field that does not exist or is not hidden is an error with its line; one that is not a companion's is a warning |
| C112 | **The first start adopts what the person had set: the settings as the file had them, and a model only when they had chosen one.** | The window had picked a model for a person who never chose one, and the Default pinned it; with a file that was not the one of the recommended list the dot showed at once (found by the GUI suite, TC-CNF-002; A69) |
| C113 | **The two sentences that count what was left out do not depend on the count**: `Settings left out because they belong to this computer (a folder, a program): 3.` and `Lines left out because they look like secrets (a key, a token, a password): 1.` | The first wording said `1 lines`, which a person reads in the CLI and in the export dialog; the vocabulary has no plural, and a count that comes last reads right for one and for many |
| C114 | **The configuration is a section of the Main page under the title row, not a part of the title row.** | The user, looking at the first version: "move the dropdown to a different section, doesn't look good at the location - right next to the header". The title row was also crowded (the dropdown gave up its label and its width at 520, and the GUI suite had found a tab covered by the settings icon, A70). The section is as wide as the page, framed like the sections under it, and the title row is as it was. Alternatives: a status bar at the foot (the choice would be far from what it chooses), in the Settings window (a top-level choice would be a second click away); not asked, said in the report |
| C115 | **A change is said in words and in a colour, in the place of the configuration, with what to do about it**: a badge *• Modified*, an amber outline, *Save*, *Save as...* and *Revert*; the room is kept, so nothing moves; a quiet section says nothing. | The user: "when user makes an edit, it has to show the configuration as edited ... If I load a configuration, it is a fact - but the moment I change, it should convey that it is modified". A dot after the name was too small, and was cut away with the end of a long name. *Revert* is new in the section (the window had it) and asks, as the window does. Alternatives: the word *(modified)* inside the dropdown only (cut with a long name); a line under the section that appears (the page would slide under the field being typed in) |
| C116 | **"Modified" is "the list of what changed is not empty"**: one answer for the badge, the hover and the question; the settings are compared as the values in force, and what the computer cannot hold (a model that is not here, a build that is not installed) is not a difference. | The first version of the section was Modified at once after choosing a configuration of a person who had not installed its build, with a hover that listed nothing: the answer was made from the raw values (`matches_run`: a value the file says and that is the default made a difference) and the list from the values in force (`changes`). "If I load a configuration, it is a fact": found by looking at the screenshot of the first version of this change (A75) |

## 13. Work packages and tests

WP4.17, size XL. All of it is built and tested; the order put the testable core first.

| WP | Work | Verified by |
|---|---|---|
| 4.17.1 | **The document.** The registry reads `profile` (`doc/profile.rs`, `registry/profile.rs`, `profile/mod.rs`): the schema of §3 held to the engines' documents; `share` and `share_note` on `[[field]]` with the defaults by type, the shipped documents marked; the built-in `default.toml`; `[[finding]]` and `[profile]` in the vocabulary; `quickai config docs profile` and `docs/reference/profile-fields.md`; the sketch in `docs/examples/profiles/` replaced | A01-A09 |
| 4.17.2 | **Capture, apply, difference** (pure, in the core: `profile/state.rs`): working state to configuration to working state; the difference the badge is made of; **the plan the imported configuration makes is the plan of the computer that made it**, apart from the paths that belong to each | A11-A19, A71 |
| 4.17.3 | **The notes** (pure: `profile/check.rs`, `notes.rs`, `screen.rs`): `review(configuration, registry, inventory)` with the inventory the application gives; review and refusal; the left-out list of an export; the table of hostile files; **a fifth engine that is only a document takes part** | A10, A20-A34 |
| 4.17.4 | **The store and the state** (`configurations.rs`, `settings.rs`, `configuration_cli.rs`): the profiles folder (atomic writes, ids), `app.configuration`, the baseline, the adoption, the start-up fallback, Save, Save as, Rename, Duplicate, Delete, Reset; `quickai configuration ...` and `plan --configuration` | A18, A41-A49, A69 |
| 4.17.5 | **The dropdown**, the unsaved-changes question, the name box (`app/configurations_ui.rs`) | A50-A52, A68, A70 |
| 4.17.9 | **The section and the badge** (after the user's screenshot of 4.17.5, C114-C116): the dropdown moves out of the title row to a section under it; a change is said in words, in amber, with Save, Save as... and Revert, and nothing moves; "modified" is one answer (`app/configurations_ui.rs`, `app/header.rs`, `app/configurations_window.rs`); the GUI suite's keywords and ten cases | A72-A75; GUI TC-CNF-049 to -058 |
| 4.17.6 | **The Configurations window** (C87's way: `app/configurations_window.rs`) with the list, the details, the import and export dialogs; the GUI suite `configurations` | A53-A57 |
| 4.17.7 | **Models and builds on import**: the buttons of the notes (the Recommended window's job, the engine's pull, Look up online, the manager's filter), the memory of where a fetch came from (`sources.rs`) | A60-A64 |
| 4.17.8 | **Companions and the gate**: Open WebUI's shares, W16, `companion_address` and the first address; PRIVACY.md, the docs of both repositories, the matrices | A65-A68 |

**The cases.** Group A of the test plan has 75 rows (A01-A75); the tests are `core/tests/p3_configurations.rs` (the document, `share`, capture and apply, the notes, the fifth engine), `core/tests/reference.rs` and `purity.rs`,
and in the application `configurations.rs`, `cli.rs`, `settings.rs`, `sources.rs`, `app/configurations_ui.rs`, `app/configurations_window.rs`, `app/engine.rs` and `app/companion.rs`. Every group of them was checked by changing the code
on purpose to see that a test fails (the mutations are in the run log of the implementation plan).

## 14. Left out, and the risks

**Left out, on purpose:** syncing between computers; signatures and author checks; a `quickai://` link and import by address; updating a shared configuration by itself; *Use and start* in one click; switching by model;
several engines at once (the `voice-chat` sketch of stacks is Phase 6 and would add `[[unit]]`, a later schema); the models a ComfyUI workflow needs (a configuration of ComfyUI says how to run it, not which models its pages use);
the theme; a Windows or macOS run (the paths are the platform's, nothing was run there, as with the other work); `argument_unknown` and a comparison of versions (WP4.4).

**Risks, and what holds them:**

* A false badge (a value the form re-derives when a model is picked, a model the window cannot hold, a build that is not installed, a value that the file spells out and is the default). The capture takes stored values only, a configuration without a model does not mind, the comparison is made on the values in force, and A17, A50 and A75 hold it.
* Ollama's models cannot be listed while it is stopped. The note says so (`model_unseen`) and never claims a model is missing.
* Native file dialogs cannot be driven by the GUI suite. Both dialogs have a path box; the dialogs behind *Browse* are by hand.
* The adoption writes a file nobody asked for. It is in the person's own configuration directory, said in the log and in this document, and only the first start does it.
* More questions in the window (unsaved changes, replace, reset, revert, delete). Each is one of the questions with *Cancel* that the window already has (the foreign program, the delete).
* A configuration that names an engine's field that is renamed in a later version: noted and ignored (`unknown_setting`), never an error, so old files keep opening.
* The page has a section more under the title row, 44 pixels of it: the GUI suites that read the page by position were recalibrated, and the keywords that waited for a row that the whole-window read lost after the move (`Configuration:`, and with the tabs on the picker's `Refresh`) read the row of the section from a region of its own (TC-CNF-001, `Open Main Tab`); reading the window in bands every 50 pixels instead of 70 found the rows and lost the first line of every question, so it stays at 70.
* A badge that appears while a person types: the section keeps the room for it, and A73 compares the places of everything before and after.

## 11. Running one without a window

`quickai-run up <configuration>` (WP4.18, [headless.md](headless.md)) takes the same file and does what the buttons of the notes do, one after the other:
it installs the build the file names or the one its engine's document chooses, fetches the models, starts the engine and the program that goes with it, and
stays until it is told to stop. It is the person typing its name that makes this different from rule 1 above (a configuration *selects*, the window
installs only when a button is pressed): the program prints what it is going to do, and how big it is, before it does it (`quickai-run check` stops
there), and a file from outside that asks about free text or wider access is not used unless `--accept-review` says so. It never writes the working
state (`settings.json`) or the configurations of the person. For an engine that runs on its own (ComfyUI) the models are the file's `[[model]]` entries,
put in the folders the engine looks in; the window writes one model when it exports (with the place, the path in the repository and the revision it fetched it at), so a set of files (the three of a video model)
is written by hand, or kept by the window while its first model stays the first. What belongs to the computer that runs it (a folder, a login, a mirror, the variables of a program's environment, a copy of a file that is already there)
is said by the person who runs it, in a file `quickai-run init` writes, and a configuration can say what it needs from them (`[[needs]]`): [headless.md](headless.md), sections 6 to 8.
