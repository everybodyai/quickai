# Headless: `quickai-run`

`quickai-run` runs a QuickAI configuration on a computer with no display, such as a rented GPU server: it installs what the configuration needs, fetches its models, starts it and stays until it is stopped. This page says what it does, how, and what has been run for real.

## 1. In short

`quickai-run` is QuickAI with no window, for a computer that has none. It takes a **configuration** (the file the window exports and
imports, [configurations.md](configurations.md)), finds out what that needs on this computer, installs what is missing, fetches the
models, starts everything, says where it is, and stays until it is told to stop.

```sh
quickai-run init  wan.toml        # the file to fill in: what the configuration needs from you, and what is the computer's
quickai-run check wan.toml        # what would be done, from where, to where, how big it is; changes nothing
quickai-run up    wan.toml        # do it, start it, stay (Ctrl-C or SIGTERM stops it)
quickai-run prepare wan.toml      # install and fetch, start nothing
```

Four things had to be true for that, and the first is most of the work:

1. **What QuickAI does to a computer is not in the window.** Until now the install, the download, the start and the stop were
   written in the window's own code. They are now in a crate of their own, `quickai-runtime`, that the window and this program both use
   (section 2).
2. **The sequence the buttons make one by one is one call**: `provision::up` (section 3).
3. **It works on a server**: the cards are found without `lspci`, a stop is a signal, nothing is asked of a person, and it listens
   on the loopback only (section 5).
4. **The configuration is the whole of what is to run, and the computer's own part is the person's** (WP4.19, sections 6 to 9): a
   configuration says where each file of a model can be had from and never carries one; it says which program goes with the engine and which build of it;
   it says what it needs from whoever runs it; and whatever belongs to the computer (a folder, a login, a variable of a program's environment, a mirror,
   a copy of a file that is already there) is said by that person, in a file that `init` writes, or on the command line.

## 2. The split (WP4.18.1)

```
quickai-core      pure: documents, expressions, the planner. No processes, network, files or window.   (as before)
quickai-runtime   every effect, no toolkit: find, install, fetch, start, watch, stop; settings and configurations on disk;
                  the documents and assets the application ships.                                       (new)
quickai           the window. It draws; what it does to the computer it asks the runtime for.            (what is left of it)
quickai-run       the headless program: arguments, text or JSON lines, signals.                          (new)
```

The window had 47,000 lines in one crate. The ones that do things to the computer have no `egui` in them (they talk through channels
and closures), so the cut was between `app/` and everything else, and the effect modules moved with their tests, their fakes, the
documents they embed and the assets they write (`git mv`, so the history is kept). Nothing in them was rewritten for the move.

**How it was proved to have changed nothing** (before any new code was written on top of it):

- every one of the 1,042 tests of before exists after under the same name (`cargo test -- --list`, sorted, compared), and the totals
  are the same: 1,022 passed, 0 failed, 20 ignored;
- the 28 headless commands (`config check`, `config docs`, `plan` for each engine with and without options, `configuration list|show|
  export|check`, the usage errors) print the same bytes and exit with the same codes before and after (the one difference is a path
  of the developer's tree, `crates/quickai/../..` becoming `crates/quickai-runtime/../..`, which names the same directory);
- fmt, clippy with warnings denied, and the whole GUI suite on the new executable (section 10);
- two guard tests keep it so (`tests/no_window.rs`, O01 and O02): the runtime depends on no toolkit and names none in its code; the
  window crate holds `main.rs` and `app/` and nothing else.

Beyond moving files, three pieces of logic that the window and a command line had each and would have had to copy were made one:

| One function | Was | Used by |
|---|---|---|
| `resolve::resolve` | the body of the `plan` command: a configuration, a model, values and a program become the plan of a run | `quickai plan` and the headless run, so the plan that is printed is the plan that is run |
| `models::catalog_fetch` (the core) | the choice inside the window's *Get* button: which download fetches a model of the list (the pairing, a projector with the weights, comes first) | the window's *Get* and the headless run |
| `acquire::pick_build` (the core) | nothing: the window shows a list and a person picks | the headless run, when nobody named a build |

## 3. What it does, in order

`quickai-run up <configuration>` where `<configuration>` is the id of one you keep or the path of a file:

1. **Reads and reviews** it for this computer, as the window does when a file is imported (`profile::check`): a file that cannot be
   used here stops it (exit 1 or 3); what a file from outside *asks* about (free text that reaches a command line, settings that widen
   what a program may do, a place it says a model can be had from) is shown and is not used unless `--accept-review` says so.
2. **Puts over it what the person says about this computer** (an environment file, `--set`, `--env`, the logins; section 7) and, if the
   configuration still needs something nobody gave, stops there with code 5, before anything is fetched.
3. **Plans**: for the engine, and for the program that goes with it when the configuration has it start (Open WebUI beside Ollama),
   what is missing (a program, the files of a model), and what is done once they run (start, pull and load a model an engine takes by
   name). The plan is printed first, with sizes, how each download is checked, and for each file the address it comes from and the place it goes to. `check`
   (or `--dry-run`) stops here.
   **Before the plan, the computer is looked at** for what the configuration says it has to have (`[[needs]] system = "..."`: a C compiler, the
   headers of Python, a driver of the 580 series; section 12): what it found is printed, and what is missing stops the run, before anything is
   fetched, with a code of its own (6) and the exact command that brings it, or is installed when the person said so
   (`--install-system-packages`).
4. **Installs and fetches**, in that order, with progress that is printed by the tenth and by what changed, not by the poll. A second run
   finds what the first left and does nothing but start. A file the person already has is checked and linked, not fetched. Between the
   install and the first fetch, the tests the engine's document has for the installed environment are run (a kernel of Triton built and
   run once: section 12), so that a computer that fails one is told so after the install and not after tens of gigabytes.
5. **Starts** the engine; for an engine that takes models by name, asks it what it has, pulls what it lacks and loads the first; starts
   the program that goes with it, told where the engine is.
6. **Says where**: `Ready.` and the addresses (and writes them to `--ready-file` as JSON, which it removes when it stops).
7. **Waits.** SIGTERM or Ctrl-C stops the program that goes with the engine first and then the engine, each with the grace its document
   gives. A program that stops by itself ends the run (exit 4) with the last lines it wrote.

Nothing in the code names an engine: the documents say how each is installed, started and asked for its models, as they do for the window.

### What the plan is made of

- **The program of an engine.** If the person named a program (`--set <engine>.bin_override=...`, an install of their own) that one, and nothing is
  installed. If the configuration names a build (`[runtime] build`) or `--build` does, that one. Otherwise the one
  the engine's document finds by itself (a package's, one on the path, the newest installed). If there is none, one is installed,
  chosen by `pick_build`, which is the document's words and the same for every engine:
  1. a build whose pin and the feed's digest disagree is never taken;
  2. a build of a backend that needs a vendor's card (`needs_gpu`) is never taken unless that card is here, so a computer whose cards
     could not be told is not given CUDA;
  3. the tested release if the document has one, else the newest; among its builds the backend the document lists first;
  4. a build whose name says no backend comes after all of them, except for a computer whose cards could not be told, which gets it
     before them: it is what a machine with no card runs;
  5. of builds that rank the same, the smaller download. (A document does not list every backend a project publishes: the first
     real run met an OpenVINO build of llama.cpp that none names, which "names no backend" like the plain build and lists first;
     it is 105 MB beside the plain build's 16. [engine-facts.md](engine-facts.md) L24.)
  If nothing offered can run here, it says so and installs nothing. A named build that is older than the newest few is looked for in as
  many releases as the host gives. Overlays (Ollama's ROCm libraries) are not installed unless asked for in the window.
- **The files of a model** are section 6. A file that is here at the size it should be is not fetched again; before the first byte
  the room is checked against the disk the files go to.
- **A model an engine pulls by name** (`ollama = "qwen3:8b"`) is pulled with the engine's own progress after it is running, unless it
  has it, and is the one loaded. For an engine that runs on its own (ComfyUI) the models are the files of the configuration's
  `[[model]]` entries, put in the folders ComfyUI looks in.
- **The program that goes with the engine** is section 8.

### Words, codes, output

```
quickai-run up|prepare|check <configuration> [--accept-review] [--build [PROGRAM=]LABEL] [--port N] [--models-dir DIR] [--python PATH]
                             [--gpu nvidia|amd|intel|apple|none] [--allow-program HASH] [--dry-run] [--json] [--quiet] [--ready-file PATH]
                             [--environment FILE]... [--set ENGINE.FIELD=VALUE]... [--env [PROGRAM:]NAME=VALUE]...
                             [--model-source FILE=WHERE]... [--models-from DIR]... [--offline]
                             [--hf-token TOKEN] [--hf-endpoint URL] [--github-token TOKEN]
                             [--install-system-packages] [--skip-prerequisites]
quickai-run init <configuration> [-o FILE | -o -] [--force]
quickai-run plan | config | configuration ...         QuickAI's other headless commands, the same code as `quickai`'s
```

| Code | Means |
|---|---|
| 0 | done, or stopped when asked |
| 1 | it could not be done: unreadable file, nothing it needs can be had, an install or a fetch failed, a program would not start, a file that is missing offline |
| 2 | a mistake in the command (the usage follows), or in what was said about the computer (a setting the engine has not, a name that is no file of the configuration) |
| 3 | the file asks and was not accepted, or a note of the review stops it |
| 4 | a program stopped by itself after it was started |
| 5 | the configuration needs something from whoever runs it, and nobody gave it |
| 6 | the computer lacks something the configuration needs (a program, a header file of Python, a driver), or the installed environment failed the test its document has for it |
| 130 | stopped before it ran (a signal during an install or a fetch) |

Sentences go to the standard output and the failure to the error stream; `--json` prints one object a line for each event (`computer`,
`environment`, `review`, `prerequisites`, `plan`, `step`, `progress`, `step_done`, `self_test`, `state`, `log`, `ready`, `stopping`, `finished`), the last always `finished`.
`prerequisites` has `items`, one for each thing the configuration needs of the computer (`id`, `label`, `state` which is `met`, `missing`, `unchecked` or `later`, `detail`, and when there is one `why`, `fix`, the command that brings it, and `hint`); `self_test` has `id`, `label`, `state` (`running`, then `passed` or `failed`) and `output`, the lines the test wrote. A step of the plan that puts files in the models folder has them one by one (`files`: the name, `fetch` or `link`, where from, where to, the size and the checksum
when known), so that another program can read the plan and do the fetching itself (section 6).

## 4. What is decided here

| # | Decision | Why |
|---|---|---|
| C117 | **The application is four crates**: `quickai-core` (pure), `quickai-runtime` (every effect, no toolkit), `quickai` (the window), `quickai-run` (the headless program). The window keeps its paths (`crate::config`, ...) by naming the runtime's modules at its root. | The user: "make things modular in the current QuickAI". A second program that copied the install would drift from the first; a crate boundary the compiler checks does not. |
| C118 | **Running the headless program is the click.** A configuration still only selects (C103); `quickai-run` is the act: it installs and fetches what the file needs, after printing what that is and how big. The window's rule (nothing is fetched until a button is pressed) is unchanged. | The user asked for a program that "has to get all the info from the config, download/configure itself and start". Ground rule 8 (no surprise network) is met by the plan that comes first, `check`, and `--accept-review` for what a file from outside asks. |
| C119 | **When nobody names a build, the document's words choose it** (`pick_build`, section 3). | A program with no list for a person to read still must not install a CUDA build on a machine whose cards it could not tell, nor guess. The rule is generic and tested against the shipped llama.cpp and Ollama documents. |
| C120 | **A card is found without `lspci`**: when `lspci` cannot be run, the kernel's list of devices (`/sys/bus/pci/devices`, display class) and the NVIDIA driver's files are read instead; an `lspci` that answered is believed, even when it lists nothing (the GUI suite puts a stand-in first on the path to decide what the application is told, and found out that a second opinion would make it depend on the computer it runs on). `--gpu` overrides all of it. | A rented server's minimal image has no pciutils, and a machine whose card is not found is given PyTorch for the processor (ComfyUI's install chooses by `has(gpu.vendors, ...)`). |
| C121 | **It listens on the loopback only**, and reaching it from elsewhere is an SSH tunnel. There is no `--listen`. | The architecture's non-goal (no non-loopback serving) and ComfyUI has no login. |
| C122 | **A document of the person's that starts a program QuickAI did not install asks once, by hash** (`--allow-program`), as the window's question does. A file from outside that asks is not used without `--accept-review`: free text that reaches a command line, settings that widen access, and a place it says a model can be had from that is not the recommended list's. | The guard (C33) and the review (C104) are the same for both programs; the window never fetches from such a hint by itself either (its look-up is a button). |
| C123 | **`quickai-run` writes nothing outside the data directory** (`~/.quickai`, or below `HOME`): not `settings.json`, not the configuration directory (`configuration import` is the one command that does, and it is the same code as `quickai`'s; `init` writes the one file the person asked for, where they asked). | A server has no window to own those files, and a person who runs both on one computer must not have them changed by a command. |
| C124 | **Linux and macOS first.** On Windows `quickai-run` said it was not there yet; since WP4.33.1 (section 13) it runs there too, built and type-checked here and not yet run on Windows. | The run is signals and process groups; the Windows branch could not be built or run here, and a program that was never run is not offered. |
| C125 | **A configuration says where a model can be had from, and never carries one.** A file model has `from` (a kind of place and a name in it), `path` (the file in that repository), `revision` (a branch, tag or commit), `into` (the folder of the models folder) or `url` (an address, `https://`, no login in it), and `size` and `sha256`; no key takes a path of a computer, a file of more than 256 KiB is not read. `quickai-run` fetches from it (C109 stays for the window: it says the place and fetches on a click). | The user: "it should just carry the information about where to fetch it from and the runtime will autofetch". The Wan 2.2 set the user runs comes from three repositories, one of the files in a folder of its repository: that is what `path` is for. The recommended list's `catalog` remains the way to say a model the list vouches for. |
| C126 | **What the person says about where a file is wins over the configuration, and is checked.** `--model-source FILE=WHERE` (a path of the file or of a folder that has it, an address, `huggingface:owner/repo`), `[models]` of an environment file, `--models-from DIR` (looked in before anything is fetched: by the name, the folder of its kind, the path in its repository), `--models-dir`. A file that is used where it is is **linked, not copied**, after its size and, when known, its SHA-256 are checked; a file of another size, or other bytes, is refused with a sentence and nothing is put in its place. A name that is no file of the configuration is a mistake that lists the names it has. The place a file from outside names is asked about only when the program is about to fetch from it: not when the person said where the file is. | The user: "the user should have the ability to specify the location". A copy that is already on a server (a mounted disk, a file put there by `scp`) must be usable without 13 GB being fetched again, and without trusting it unchecked. The first version suppressed the question when a folder had a file of the right name, so a file of the wrong size stood for the person's yes (found by writing the rule down, Y33). |
| C127 | **"Alternate means" are: bring the files by any means, or point at a mirror; the program checks them.** `--offline` fetches nothing and says what is missing, one file at a time, with the place it goes and the address it would have come from; `check --json` has the same for a script. There is no hook that runs a command to fetch. | The user: "or provide alternate means to get it". A command that fetches would be a command the program runs on the word of a file or of a flag; the means a person has (`aria2c`, `rsync`, `scp`, a mounted disk, a mirror) all end with a file in a folder, which is checked. |
| C128 | **A login goes to the host it was given for and to no other.** `--hf-token` / `HF_TOKEN` / `[fetch] huggingface_token` is sent as a bearer login to the hub (or to the mirror that replaces it), and goes on only through a redirect inside that host; the signed address a hub sends a file on to (the real hub sends every large file of the Wan 2.2 set to `us.aws.cdn.hf.co`: L26) gets none; an address the person names on another host gets none. `--github-token` / `GITHUB_TOKEN` signs in the requests for the list of builds (60 an hour without it) and nothing else. The four variables that can hold a login (`HF_TOKEN`, `HUGGING_FACE_HUB_TOKEN`, `GITHUB_TOKEN`, `GH_TOKEN`) are taken out of the environment of everything the program starts. A 401 or 403 says what to do. `--hf-endpoint` / `HF_ENDPOINT` replaces the hub's address in every fetch. | A server shares an address with other people's servers and its 60 requests an hour run out; a gated repository needs a login. A token that followed a file to the next host would be given to a host nobody chose. |
| C129 | **What belongs to the computer is said by the person who runs the configuration**, not by the configuration: an **environment file** (`--environment FILE`, TOML: `[fetch]`, `[values.<engine>]`, `[env.<program>]`, `[models]`; an empty value is not set; `init` writes it), `--set <engine>.<field>=VALUE` (a setting of the engine or of a program that goes with it, also the ones a configuration cannot carry: a folder, a Python, a launcher), `--env [program:]NAME=VALUE` (a line of that program's environment, after the configuration's own lines), the usual variables for the logins. A flag wins over a variable, a variable over a file, a later file over an earlier, and all of them over the configuration. `share` is not asked of the person's own words (they are theirs); the names an engine's document keeps for itself (`OLLAMA_HOST`, `DATA_DIR`, `HOME`...) are still not set from here. The file is named explicitly: nothing is read because it is next to the configuration. | The user: "the environment specific variables if any are to be specified by the user - either through a configuration file (which you ask the user to fill) or via commandline etc." The engine documents already say which settings belong to a computer (`share = "never"`) and which program takes lines of environment (the field that binds them), so the file to fill in is made from them and the program names no engine. |
| C130 | **A configuration can say what it needs from whoever runs it** (`[[needs]]`: a login, a setting, a variable of a program's environment, each with why), and `quickai-run` stops with a code of its own, 5, before it fetches or installs anything, names each thing with why and how to give it, and says that `init` writes the file to fill in. `init` makes the file from the configuration and the documents: what is needed as a line to complete, the settings that belong to a computer, the lines of environment, and the files the configuration fetches, each with where from and how big; it is written readable by its owner only and is not written over unless told to. A value left empty is not given. | "which you ask the user to fill": the program asks, and says how. Stopping before the first byte means a person who forgot a login does not wait for a 17 GB install to find out. |
| C131 | **A secret is in nothing the program says.** A login is a type that prints as hidden, whatever prints it (`Debug`, `Display`); the sentences, the events, the JSON, the ready file and the error stream say where a value was given and show it unless its name says it is a secret (KEY, TOKEN, SECRET, PASSWORD) or it is a login, when they say `(hidden)` and `null`; a block of lines of environment is counted, not shown; the file `init` writes is mode 600 and an environment file that holds a secret and that others can read is warned about. A path is shown whole. | The first version of the `Debug` of a line of environment printed its value (found by the test that holds it, Y23), and the first version of the sentence about a folder cut it at 60 characters (found by the Robot suite). |
| C132 | **A program that goes with the engine is part of the configuration, and its build can be pinned.** `[values.openwebui] enabled = true` (and its port and internet access) is how a configuration says it; `[runtime.openwebui] build = "0.11.4"` is which build of it is installed, `--build openwebui=LABEL` wins over it. It is installed, configured (told where the engine is, its data below its own folder, the variables of its environment from the person) and started after the engine, and stopped before. The window's Save keeps what it cannot hold: the pins, the needs, and the models beyond the one it shows with where each is from (it writes the model it shows, and the others stay while that model is the first). | The user: "the configuration should specify that as well and must be able to download and configure that too". A server must be able to be given the same Open WebUI twice, which a version that moves with the document's tested release does not do. |
| C133 | **What a configuration may not do.** It names no program and cannot add code to an engine: the custom nodes of ComfyUI that its document installs are the document's, and a person who wants others writes a document of their own (which asks, by hash, before it starts anything). `into` is one of the folders the engine's document says it keeps files in, so a file cannot be put where the engine does not look. A file name has the letters, digits, `.`, `-` and `_` the models folder keeps. | The architecture's rule that a data-tier document names no program; a shared file must not be able to make a server run someone else's Python. |
| C134 | **What the computer has to have is declared in two places, and each says what only it knows.** The engine's document names the prerequisites (`[[prerequisite]]`: an id, a label, a condition on the computer, a kind of check, the packages that bring it, a hint); a configuration says which it needs (`[[needs]] system = "<id>"`, with its own words about why). Nothing is looked at that no configuration asks for. | The need comes from the files of the model (the int8 and 4-bit builds of H3 make Triton build a helper in C); how to look, and what brings it, belongs to the engine and to the PyTorch its document installs. The first clip of a real run of H3 on a rented server died of it (section 12), and the program that had installed the server knew nothing of it. A prerequisite is not asked of every configuration of the engine: a Wan 2.2 set in fp8 does not need a compiler. |
| C135 | **A package is named by a document and by no configuration.** A package is installed as the administrator, and its installation scripts run as the administrator too, so a package name taken from a file that was given away would be a way to run someone's code as root. A configuration can say only the id of a prerequisite that the engine's document has (an id it does not have is refused when the file is read, with the ones it has); a document's package names are held, when it loads, to plain names with no option, path or shell word (the only words are `${python.version}` and `${python.digits}`); a driver has no package at all. | The same rule as C133 (a configuration names no program), applied to the one place where this program would run a name from a file with more than a person's rights. |
| C136 | **What can be looked at is a closed list, and a document composes it** (the primitive `prerequisite@1`): `program` (one of the names on the `PATH`, or the one a variable names: Triton reads `CC`, then gcc, then clang), `python_module` (a module the Python the environment is made with can import: `ensurepip`, which Debian puts in `python3.X-venv`), `python_header` (a file in the folder of the headers of the Python the environment is made with: the one beside an installed launcher, else the one the install will use), `nvidia_driver` (what `nvidia-smi` says, else the driver's own file; compared number by number), and `python_script` (one of the application's own scripts, run by the environment's Python, in the environment the engine is started with; it passes on status 0). | Documents name audited actions and carry no command (the architecture's rule for primitives). The compiler and the headers are read the way Triton reads them (F29), so that the program is not stricter or looser than the thing it is protecting. |
| C137 | **It is looked at before anything is fetched, said before the plan, and what is missing stops the run with a code of its own (6) and the exact command.** `sudo apt-get update && sudo apt-get install -y build-essential python3.12-dev`, with the `sudo` when the person is not the administrator, and for the package manager the computer has (apt-get, dnf, zypper, pacman, apk). `check` says the same and installs nothing. A missing thing that no package brings (a driver) is said with the document's hint. | 6 and not 5: 5 is a thing a person gives, in a file or on the command line; this is a thing the computer lacks, and a service unit can tell the two apart (`RestartPreventExitStatus`). Before the plan, because a person who runs `check` should see what the computer lacks and what would be done, in that order. |
| C138 | **Packages are installed only when the person says so** (`--install-system-packages`): C123 (it writes nothing outside its data folder) holds unless they say this once. Then: as the administrator, or through `sudo -n` when that needs no password (nothing here can answer a question); apt's list of packages is refreshed first (a rented server's is as old as its image); apt's install is told to wait for a lock another process holds, two minutes at most (`-o DPkg::Lock::Timeout=120` after `install -y`, F32; the words printed for a person stay the plain command); each command leads a process group of its own, so that a Ctrl-C at the terminal does not stop a package manager half way through a package; every line it prints is passed on; `--offline` and `check` never install; when it fails, the failure says which command and its last lines. The computer is looked at again afterwards, and what is still missing stops the run. | A rented server is the administrator's own, and installing two packages on it is what a person would type; but a program that changes a system when it was asked to run a configuration is not what C118 ("running it is the click") covers, which is about its own folder. The flag is the click. A rented server runs upgrades of its own in its first hours and they hold apt's lock; without the wait, an install that meets one ends at once (F32) and a person has to find out why and run it again. |
| C139 | **The installed environment is tried before it is used, after the install and before the files of the models are fetched**: the tests the document names (`python_script`) are run with the environment the engine is started with (the same plan: its home, its temporary folder, the person's Python variables put away), from a folder made for the test and removed after, and the first failure stops the run with how it ended, the last lines it wrote, the configuration's words and the document's hint. The one ComfyUI has builds and runs a Triton kernel from an empty cache of Triton's, so that the helper is built here, in a few seconds, and not found from an earlier run. `check` lists a test and runs none. | The look can say a compiler is there and still be wrong (a compiler without its libraries, a driver that PyTorch cannot use, no `libcuda` to link); the test is the thing itself. Before the models, because they are 65 GB and 13 minutes: the clip that finds out is not the first. |
| C140 | **`--skip-prerequisites` leaves all of it out**, the look and the tests, and says so. | A person who knows better (their own Python build, a compiler Triton finds by another name, a test that is wrong for their card) must not be held by a check they cannot get past. |

## 5. On a rented server

```sh
# on your computer, once: the configuration you made in the window, or one you wrote (docs/examples/profiles/wan22-ti2v-turbo.toml)
quickai configuration export vision-chat -o vision-chat.toml
scp vision-chat.toml you@server:
scp dist/quickai-run-0.1.0-linux-x86_64.tar.gz you@server:   # from build/headless.sh; unpack it there

# on the server
./quickai-run check vision-chat.toml          # read it: the plan, the sizes, from where, to where, what asks
./quickai-run up    vision-chat.toml          # do it
```

- **A service.** `quickai-run up` is a foreground program that exits 0 on SIGTERM, so a unit file is four lines
  (`ExecStart=/home/you/quickai-run up /home/you/vision-chat.toml --environment /home/you/vision-chat.environment.toml`, `Restart=on-failure`, `User=you`,
  `KillSignal=SIGTERM`); the logins can be in the unit's `Environment=` or `EnvironmentFile=` (`HF_TOKEN=...`) instead of the environment file.
  Add `RestartPreventExitStatus=2 3 5 6`: exit 2 is a mistake in the command or the file, 3 is a file that asks and nobody answered, 5 is a
  configuration that needs something nobody gave, 6 is a computer that lacks something the configuration needs, and starting again gives the
  same answer (`Restart=on-failure` is for 1 and 4: a download that failed, a program that stopped by itself). A second SIGTERM (or Ctrl-C) does not wait for the first to finish: it ends this program at
  once, with 130, without stopping what it started: on Linux the kernel tells each program to leave (SIGTERM) when this one dies, which
  the real `llama-server` did when it was seen (section 11); on macOS a program would be left running.
  `--ready-file` is the file a script waits for.
- **Where it keeps things.** Below `~/.quickai` (programs under `llama-bin/`, `ollama/`, `comfyui/`, `openwebui/`, models under `models/`). On a
  server with a small system disk and a large one mounted elsewhere, run it with `HOME=/mnt/big`, give `--models-dir`, or set the folder in the
  environment file (`[values.comfyui] models_dir`); a ComfyUI install is 17 GB with PyTorch for AMD and about 7 GB with PyTorch for CUDA 13, and asks for 30 GiB free either way.
- **Reaching it.** It listens on `127.0.0.1`. From your computer: `ssh -N -L 8189:127.0.0.1:8188 you@server`, then the address is
  `http://127.0.0.1:8189`. For bampu, whose ComfyUI provider is a file with a `base_url`, that address (or the tunnel on the default
  port) is all it needs.
- **The cards.** They are found from the kernel; if that is wrong, `--gpu nvidia`. The line `This computer:` says what was found.
- **Python: none.** The installs of ComfyUI and Open WebUI make a Python environment with QuickAI's own Python (CPython 3.12, downloaded once into the
  engine's folder, `python/`, and checked against the SHA-256 the engine's document pins; the plan says so). Nothing of the computer's Python is needed or
  changed, and llama.cpp and Ollama need none at all. `--python PATH` names a Python of the computer's to use instead (3.10 or newer; Open WebUI: 3.11 or 3.12);
  then it must have the venv module (`python3.12-venv` on Debian and Ubuntu). A Python that is only a shim of a version manager (`pyenv`) for a version it has not
  selected does not answer: name its real file (`~/.pyenv/versions/3.12.3/bin/python3.12`).
- **A compiler (and a driver).** A configuration of ComfyUI that runs the int8 and 4-bit video models says that the computer must have a C compiler and the
  headers of the Python the environment is made with, because Triton builds a helper in C the first time a kernel of the model is launched (section 12). QuickAI's
  own Python has its headers; a Python named with `--python` may not (the package `python3.12-dev`). A minimal Ubuntu server has no compiler (it is in
  `build-essential`); `quickai-run` looks before it fetches anything, says what is missing with the exact command, and installs the packages itself with
  `--install-system-packages` (as the administrator, or through `sudo` when that needs no password).
- **System libraries.** llama.cpp's own build needs a few that a minimal image lacks: `libgomp1` and OpenSSL 3 (`libssl3`), and
  glibc 2.34 or newer. When one is missing the failure says which, in the program's own words (`error while loading shared
  libraries: libssl.so.3`): install it and run the same command again, which finds what the first run installed.
- **NVIDIA.** The document lists CUDA first and the two CUDA builds of a release rank the same, so the lighter, CUDA 13, is taken.
  A CUDA build is only the program: it needs the CUDA runtime libraries on the computer (the toolkit, or the release's `cudart-`
  file), and CUDA 13 wants a driver of the 580 series. On a server where that is not so, name the build with `--build` (a CUDA 12 one,
  or the Vulkan one if the driver has Vulkan) or install the libraries. ComfyUI's install is likewise PyTorch for CUDA 13 on an NVIDIA
  computer (the document's choice, not this program's), and wants the same driver, which a configuration can have looked at (`system = "nvidia_driver"`,
  section 12). The CUDA build of llama.cpp was not run (the card here is AMD's).
- **GitHub's allowance.** The list of builds is asked of GitHub's API, which lets 60 requests an hour without signing in; `--build`
  with a label that is installed asks nothing, and `--github-token` (or `GITHUB_TOKEN`) raises the allowance.

## 6. A configuration that carries no model: where the files come from (WP4.19.1, WP4.19.2)

```toml
[[model]]
role = "vae"                                  # what it is for: the first model is the one the engine loads
file = "Wan2.2_VAE.safetensors"               # the name it is kept under (letters, digits . - _)
from = "huggingface:QuantStack/Wan2.2-TI2V-5B-GGUF"      # a kind of place and a name in it ...
path = "VAE/Wan2.2_VAE.safetensors"           # ... the file in the repository (left out: at the top, called `file`)
revision = "57437632ddd08bdcbd1508c866aa22e126ed51d2"    # a branch, a tag or a commit (left out: main); a commit is the same file for ever
into = "vae"                                  # the folder of the models folder (left out: the one the document gives a file like this)
size = 1409400960                             # held to what arrives, with or without a checksum
sha256 = "e40321bd..."                        # held to what arrives (and to a file the person has)
# url = "https://files.example/Wan2.2_VAE.safetensors"   # an address instead of `from` (https; this computer's own over http)
```

A file of a model has one of three sources: a name the engine pulls (`ollama = "qwen3:8b"`), an entry of the recommended list (`catalog` and its `file`), or a file
(`file`, and where it can be had from). The keys of a place go with a file, not with the other two. [docs/examples/profiles/wan22-ti2v-turbo.toml](examples/profiles/wan22-ti2v-turbo.toml) is the
user's own Wan 2.2 TI2V 5B Turbo set (three files from three repositories, one in a folder), with the commits, sizes and checksums read from the hub on 2026-10-12 and held against the files on the
user's computer: `quickai-run check` on it says, against the live documents, what it would fetch from where into which folder (the weights in `diffusion_models`, the
encoder in `text_encoders`, the VAE in `vae`). The folder is the one the engine's document gives a file of that name, or `into`, which can only be one the document says the engine keeps files in
(`models::folders`: ComfyUI's are those of its document; llama.cpp keeps all its files in one).

[docs/examples/profiles/minimax-h3.toml](examples/profiles/minimax-h3.toml) is the same for MiniMax H3, the open-weight video model the user chose (2026-10-13) for an NVIDIA L40S of 48 GB with 64 GB of memory (and [minimax-h3-fl2va.toml](examples/profiles/minimax-h3-fl2va.toml) is its lean half: the build for words and first and last frame, five of the files, 42 GB): the seven files of its two builds (words, first and last frame; reference pictures), all from `Comfy-Org/MiniMax-H3` at one commit, the weights in int8, the text encoder in 4 bits, the video and audio VAEs and one Lightning LoRA for each build, 65 GB in all, with the commit, size and SHA-256 of each read from the hub's answer for the file at that commit. They are the files ComfyUI's own templates of the model load. It needs the models folder (`[[needs]]`, because 65 GB is not for a system disk), pins nothing, and says in its header and its note what the licence covers: the world except the EU, UK, South Korea and USA, for using the model *and for showing what it makes there*. A configuration holds at most 8 models, so each file says in a comment what it can take besides. Y70 reads both.

**What is fetched from where.** For each file that is not in the models folder at the size it should be, in this order: (1) a place the person said for it (`--model-source`, `[models]`); (2) a folder
they pointed at that has it (`--models-from`); (3) the place the configuration names, on the mirror if there is one (`--hf-endpoint`). The address of a file is
`{hub}/{owner}/{repo}/resolve/{revision}/{path}`, the path percent-encoded; the hub is the site the engine's document gives for its look-up of Hugging Face (a user-layer document can point
it at a mirror, and so can `--hf-endpoint`). A file that arrives is checked as it is written (the hash is worked out on its way to the disk) and is not kept if it is not what the configuration
says; nothing partial is left.

| To say | Use |
|---|---|
| "this file is already on this computer" | `--model-source FILE=/path/to/file`, or a folder: `FILE=/path/to/folder` |
| "all my files are in this folder (a mounted disk, a copy of the repository's layout)" | `--models-from DIR`: looked in by the name, in the folder of its kind, by the path in its repository |
| "get it from here instead" | `--model-source FILE=https://...` or `FILE=huggingface:me/my-copy` |
| "my hub is a mirror" | `--hf-endpoint https://mirror.example` or `HF_ENDPOINT` |
| "the repository wants a login" | `--hf-token`, `HF_TOKEN`, or `[fetch] huggingface_token` of an environment file |
| "do not touch the network" | `--offline`: the plan is said, then what is missing, with where to put it and where it would come from |
| "I will fetch them myself" | `quickai-run check wan.toml --json` has each file's address and place; fetch them (`aria2c`, `rsync`...), then `--offline --models-from DIR` |
| "keep the models on the big disk" | `--models-dir DIR`, or `[values.<engine>] models_dir` of an environment file |

A file from outside that names a place is read before it is fetched from there: `quickai-run` stops with code 3, says the whole of it (the repository, the file in it, the revision; or the address) and how to say yes
(`--accept-review`), and nothing is fetched or written. It is not asked when the person said where the file is, and a configuration they keep (imported, run by its id) is not from outside.

## 7. What belongs to the computer: the environment (WP4.19.3)

A configuration is given away, so it carries nothing of a computer or a person (configurations.md, rule 2). Whoever runs it says:

```toml
# vision-chat.environment.toml -- `quickai-run init vision-chat.toml` writes it, for the configuration it was made for
schema = 1

[fetch]
huggingface_token = ""                   # NEEDED by this configuration: the repository asks you to accept its licence
# huggingface_endpoint = ""              # a mirror of huggingface.co (also HF_ENDPOINT)
# github_token = ""                      # the list of builds: 60 requests an hour without one (also GITHUB_TOKEN)

[values.comfyui]                         # the settings of the engine that belong to a computer (from its document: share = "never" or "review")
# models_dir = ""                        # Where the models are kept, in the folders ComfyUI looks in ...
# python = ""
# gfx_override = ""

[env.openwebui]                          # variables of a program's environment: one NAME = "value" each
WEBUI_SECRET_KEY = ""                    # NEEDED by this configuration: signs the logins of Open WebUI
# NAME = "value"

[models]                                 # where to get a file from on this computer instead of where the configuration says
# "wan2.2_vae.safetensors" = ""          # from huggingface:QuantStack/Wan2.2-TI2V-5B-GGUF (VAE/wan2.2_vae.safetensors) (1.3 GB)
```

It is read with `--environment FILE` (more than one, a later one wins); `--set ENGINE.FIELD=VALUE` is one setting (the text is read as the type of the setting: a number, true or false, one of the options, a path),
`--env [PROGRAM:]NAME=VALUE` one variable of a program's environment (PROGRAM is the engine unless a program that goes with it is named), `--model-source FILE=WHERE` one place for a file. A word that makes no
sense is code 2, with the file and the key or the flag it was said in (`values.comfyui.port: more than 65535...`, `--set llamacpp.port=eighty`), all at once; a key this version does not know is told as a warning and the rest is used;
an empty value is not set (so a file to fill in can be given back blank, and what is needed in it is still needed). What was said is told, with where it was said, a secret as `(hidden)`:

```
What you said about this computer:
  fetch.huggingface_token = (hidden)  (the variable HF_TOKEN)
  values.comfyui.models_dir = /mnt/big/models  (environment file /home/you/wan.environment.toml)
  env.openwebui.WEBUI_SECRET_KEY = (hidden)  (--env)
```

**The strength of each**, from the weakest: the engine's document, the configuration, an environment file (a later one over an earlier), `--set` and `--env` (in the order given), `--port` and `--models-dir`; for the logins a file, then
the variable, then the flag. A line of environment (`--env`, `[env.<program>]`) follows the lines the configuration's own `extra_env` holds; a setting (`--set`, `[values.<engine>]`) replaces. The names an engine's
document protects (`OLLAMA_HOST`, `DATA_DIR`, `OLLAMA_BASE_URL`, `HOME`...) are not changed by a line, as they are not by the person in the window. The floor of names that no configuration may set (`LD_*`, `PATH`...)
is for what comes from a configuration; a person who writes `LD_PRELOAD` in their own file or flag is believed, as in the window's own field.

**What the configuration needs.** `[[needs]]` in a configuration says what whoever runs it has to give:

```toml
[[needs]]
token = "huggingface"            # or "github"
why = "The repository asks you to accept its licence."

[[needs]]
setting = "comfyui.models_dir"   # <engine>.<field>, of the engine or of a program that goes with it
why = "Somewhere with 40 GB free."

[[needs]]
env = "WEBUI_SECRET_KEY"         # a variable of a program's environment ...
for = "openwebui"                # ... the engine's unless this names a program that goes with it
secret = true                    # (a login, and a name with KEY, TOKEN, SECRET or PASSWORD in it, always is)
why = "Signs the logins; keep it the same between runs."
```

Until each is given, `check`, `prepare` and `up` stop with code 5 and a sentence per thing (`a Hugging Face token`, `comfyui.models_dir`, `WEBUI_SECRET_KEY (for openwebui)`), why it is asked, and
how to give it (`--hf-token, the variable HF_TOKEN, or `huggingface_token` under [fetch] of an environment file`); nothing is fetched or installed meanwhile; the sentence ends with `quickai-run init <configuration>`.
A setting the configuration itself gave counts as given. The window never reads `[[needs]]`; it keeps them when it saves the configuration.

## 8. Programs that go with the engine (WP4.19.4)

Open WebUI beside Ollama is the only one the application ships. A configuration says it is part of the run in `[values.openwebui]` (`enabled = true`, its `port`, its internet access `network`) and which build of it
in `[runtime.openwebui] build`:

```toml
engine = "ollama"
[runtime]
build = "v0.35.1-ollama-linux-amd64"   # Ollama's: named as the list of builds names it, the release and the file (left out: the one the document was tested with)
[runtime.openwebui]
build = "0.11.4"             # Open WebUI's: the release of the package index (left out: the one the document was tested with)
[values.openwebui]
enabled = true
port = 3000
```

`quickai-run` installs both (Ollama from its release, Open WebUI from the package index into a Python environment of its own, below `~/.quickai/openwebui`), pulls the model, and starts Open WebUI after Ollama, told where Ollama
is (`OLLAMA_BASE_URL`), with its data and its home in its own folder, its internet access as the configuration says (offline unless it says `online`, which a file from outside has to be allowed to
say), and the variables of its environment that the person gave (`--env openwebui:WEBUI_NAME=Studio`, `[env.openwebui]`): the ones that Open WebUI keeps in its database after its first start, and so that a configuration may not
promise (configurations.md, section 9), are the person's to give here, at the run. It says both addresses, and stops Open WebUI before Ollama. `--build openwebui=LABEL` names the build on the command line and wins over
the pin; a label the index does not have is told with the ones it has before anything is written. A pin for a program that does not go with the engine is not one (it is told as a key this version does not know), and a pin for one
that is not switched on changes nothing. `docs/examples/profiles/chat-with-open-webui.toml` is the whole of it, with the key that signs the logins as a `[[needs]]`.

What is **not** in a configuration: custom nodes for ComfyUI or any other program a configuration would have a server install and run (C133). The nodes ComfyUI needs for GGUF files are in its document and are installed with it; others are a document of
the person's own, which asks by hash before it starts anything.

## 9. Tests (WP4.19)

Group Y of [phase3-test-plan.md](phase3-test-plan.md): the file (Y01 to Y12: the keys of a place and what they may not be, `[[needs]]`, the pins, what an export says of where in a repository a file is and the Save of the window keeps, and that a build pinned in an example is named the way the list of builds names it: Y68), the environment file and the file to fill in
(Y20 to Y27: the same code that reads, layers, shows without secrets, applies and writes the template, and a source scan that names no engine), the plan against the fakes (Y30 to Y51: the files of a set in their folders, what arrives held to what is stated, the review, the person's
own files and folders, offline, the logins and where they go, the mirror, the environment reaching the program, what is needed, the secrets, the pins), and the binary (Y60 to Y67). The program run for real, end to end, with the fakes of the GUI suites and a fake hub that remembers who asked, is the Robot suites `headless_*` of the
GUI repository ([docs/21_headless.md](../../test/docs/21_headless.md) there, TC-RUN-001 to -111).

**Mutations.** 31 pieces of the code were broken on purpose, one at a time, and a test was seen to fail for 29 of them. Three were not noticed at first and each is now a test: a blank login of Hugging Face's counted as given (Y20), a login sent to every host (Y37c, TC-RUN-055),
and a file written over keeping the mode it had (Y63, TC-RUN-008). The two left change nothing anyone can see: a login written into an address is refused by three other checks as well as the one that was broken, and the mode a file is first created with is replaced by the 0600 it is given
straight after. 15 of the pieces were also broken in the program the Robot suites run, and each was seen to fail a case there. The list of the pieces is in the run log of the [implementation plan](implementation-plan.md); the script that made them is not part of the repository.

## 10. What it does not do, and what has not been run

- **Windows.** It builds for Windows and the run, the stop and the start in the background are written for it (section 13), but it
  has not been run on Windows: that is a run on a Windows computer, as A8 is.
- **A real NVIDIA card.** The install chooses PyTorch for CUDA 13 on a machine where an NVIDIA card is found, from the engine's
  document, and llama.cpp's CUDA build (section 5). That path was written from the document and tested against the fakes; the
  development machine has an AMD card. ComfyUI's install was run on a real one on 2026-10-13 (the L40S of a rented server, section 11);
  llama.cpp's CUDA build has not been run on one.
- **A real ComfyUI install through this program.** The install is the window's own primitive (`install.python_app@1`, run for real on
  this machine on 2026-10-08), and the tests here run it through the program against a fake GitHub and a fake Python; the real one takes about 20 minutes and
  17 GB with PyTorch for AMD, which a test run does not. Through this program it was run for real on 2026-10-13, with PyTorch for CUDA 13
  (about 5 minutes and 7 GB, section 11).
- **A gated repository.** The fake hub answers 401 and 403 as the real one does; no real gated repository was used (no login was at hand).
- **Several models in the window.** The window shows one; it keeps the others (with where each is from) while that one stays the first, and drops them when another model is chosen. A configuration with several files is for `quickai-run`.
- **Overlays**, the live-traffic proxy (which is for the window's Live tab), and a second program per engine other than the one the
  document names.
- **Authentication.** There is none to configure: it is on the loopback.

## 11. What has been run for real

On the development computer, in a private home, on 2026-10-12:

- **The plan** (`check`) of three configurations against the live GitHub and PyPI feeds: llama.cpp with a 219 MB model; Ollama with Open
  WebUI beside it (`install Ollama v0.35.1 ... install Open WebUI 0.11.4 ... pull ... load ... start Open WebUI`); ComfyUI with the LTX
  video model (`install ComfyUI v0.39.0 ... PyTorch for AMD GPUs (ROCm 7.2) ... it needs 30GiB free`, `fetch ... and 2 more files (6.6 GB)`).
  The home was left as it was found.
- **llama.cpp b11564 and `LFM2.5-350M-Q4_K_M.gguf`, through `up`**: the release's plain build installed (16.9 MB), the model fetched
  (218.7 MB, progress by the tenth), the real `llama-server` started, a question over `/v1/chat/completions` answered, a SIGTERM
  ended it in under a second with its server, exit 0, the ready file removed and the port free. A second run did nothing but start.
  The first of these runs found a defect (it installed an OpenVINO build; section 3, rule 5), which is why there is a rule 5.
- **The packaged binary in a bare `debian:12` container** (no `lspci`, no Python, no OpenSSL or OpenMP): it told the host's AMD card
  from the kernel's list, chose the Vulkan build, and, where the build needed a library the image lacks, stopped with the loader's
  own sentence naming it. With `libgomp1` and `libssl3` installed, and `quickai-run` as the container's PID 1: the plain build of the
  newest release installed, the model loaded, a question answered, and `docker stop` ended it in 0.2 s with exit 0.
- **The whole GUI suite** (608 cases, four runs side by side) on the split application: 606 passed, 2 skipped (the long-standing ones), 0
  failed. Its first full run found two things: the test repository's helpers named the old crate directory, and a stand-in `lspci` that
  the suite puts first on the path was looked past by the first version of the card detection (section 3 and C120).

WP4.19, the same day:

- **The real hub's answers for the three files of the user's set** (headers only, nothing downloaded): the address `{hub}/{repo}/resolve/{commit}/{path}` with a folder in the path answers `302` to a signed address on another host,
  with the size and the SHA-256 of the file (L26). The sizes and the three checksums are in the worked example, and the checksums **matched the three files on the user's computer** (`sha256sum` of 13 GB: 74 s).
- **`quickai-run check docs/examples/profiles/wan22-ti2v-turbo.toml --gpu amd --accept-review`** in a private home, against the live documents: install ComfyUI v0.39.0 (PyTorch for AMD GPUs), and for each of the three files the
  address on the real hub and the folder it goes in, which are the folders the user's own set is in.
- **`quickai-run prepare` of the same file with `--models-from ~/.quickai/comfyui/models --set comfyui.bin_override=<a stub>`** in a private home: the three files were checked against their SHA-256 (13 GB, with progress by the tenth; slow in a debug build, which hashes
  without the processor's help), linked, not copied, into the folders of the private home, and the program said `Everything it needs is in place; nothing was started`. ComfyUI (v0.39.0, the user's own copy) lists a linked file and opens it (L27).
- **`quickai-run up` of a configuration that carries no model, against the real hub**: a 1.1 MB public story model (`ggml-org/models-moved`, `tinyllamas/stories260K.gguf`, pinned to a commit, with its `size` and `sha256` in the configuration) and an environment file that sets `models_dir` and
  `extra_args = "--threads 2"`. The plan named the file's exact address on the hub, where it goes, and that those two settings came from the environment file; the file was fetched and its SHA-256 equals the one stated; the real `llama-server` started with `--threads 2` and
  that folder, and answered a question over `/completion`; one SIGTERM by pid ended it (`Stopping...`, `Stopped.`, exit 0, the ready file removed, the port free, no process left); running it again fetched nothing. The program was the development tree's own build of llama.cpp (the
  document's search finds it before it installs one), so this run did not install a release; that is the run of llama.cpp b11564 above. Two signals sent by mistake at once (a `pkill` that matched this program and the `timeout` that held it) ended it at once with 130, as section 5 says, and the
  server went with it.
- **Ollama with Open WebUI beside it, through `up`, in a private home on private ports** (21434 and 23000, so as not to meet the person's own Ollama), with the shape of `chat-with-open-webui.toml` and a 271 MB model (`smollm2:135m`). `check` without the key that signs the logins
  stopped with code 5 and said what, why and how to give it; `init` wrote the file (mode 600), the key was filled in by hand, and `check` then printed the chain: Ollama v0.35.1 (1.3 GB, checked against the document's SHA-256), Open WebUI 0.11.4 (the index's file checked against the pin,
  and what it needs, into an environment of its own), start Ollama, pull, load, start Open WebUI. `up` did all of it: Ollama 2.2 GB, Open WebUI 3.0 GB, the model 259 MB, 5.2 GB in all, no package cache outside the folder. Open WebUI's process had `OLLAMA_BASE_URL=http://127.0.0.1:21434`,
  `OFFLINE_MODE=true`, a home and a data folder of its own, and the key from the file (read from the process's environment; the key is in nothing the program printed or wrote); Ollama listed the model and answered a question; Open WebUI answered `GET /` with 200 and said it is 0.11.4;
  one SIGTERM stopped Open WebUI and then Ollama in 0.4 s, the ready file was removed, both ports were free and nothing was left. The person's own `~/.quickai`, `~/.ollama` and `~/.cache/pip` were not touched. Two things came out of it: the pins in the two examples that pin Ollama's build (`v0.35.1`, and `v0.40.0` in the older `vision-chat.toml`) are not names in
  the list of builds (they are named by release and file, `v0.35.1-ollama-linux-amd64`; the examples and section 8 now say so, Y68 holds every pin of the examples to that form, and the program's sentence for a name that is not there already lists the names), and this computer has Python 3.12 only behind a `pyenv` shim, so the first run stopped
  with the sentence that no Python 3.12 or 3.11 was found and what to install, and `--python` named the interpreter for the second.

- **The package `build/headless.sh` makes (4.5 MB, built in Docker for the old glibc) in a bare `debian:12` container**, with this work in it: `init` printed the file to fill in for the Wan 2.2 set; `check` against the live feeds named the three hub addresses and the folders below `/root/.quickai`;
  `up --offline` with nothing installed ended with code 1 and said to install first; and `prepare --models-from` the person's own models folder (mounted read-only) with `--set comfyui.bin_override=/bin/true` checked the three files (13 GB) against their SHA-256, linked them
  and did not copy them, and said `Everything it needs is in place; nothing was started`. The offline sentence had two spaces where a build's name was left out; that is mended.

WP4.19, 2026-10-13, the example of MiniMax H3:

- **`quickai-run check docs/examples/profiles/minimax-h3.toml --gpu nvidia --accept-review --set comfyui.models_dir=/mnt/big/models`** (the packaged binary, a private home, the live documents): `install ComfyUI v0.39.0 ... PyTorch for NVIDIA GPUs (CUDA 13) ... it needs 30GiB free`, the seven fetches with the address, the folder and the size of each, `start ComfyUI on 127.0.0.1:8188`; without the folder it stopped with code 5 and said why. The lean `minimax-h3-fl2va.toml`: the same with five.
- **The hub's answer for each of the seven addresses at the pinned commit** (`HEAD`, nothing downloaded): `302`, `x-linked-etag` equal to the SHA-256 in the file, `x-linked-size` equal to its size, `x-repo-commit` equal to the commit. The same for the four extra files the header's comment offers; with all four added to the seven the file is refused (more than 8 models), and with three or one in place of another it is read.
- **`quickai-run prepare` of a configuration with one of the files (the audio VAE, 577 MB) and a stand-in for ComfyUI**: fetched through the hub's redirect in 1 minute 30 seconds, the size equal, and `sha256sum` of the file equal to the one in the configuration.
- **`quickai-run prepare` and `up --offline` of `minimax-h3.toml` on the rented server it was made for** (a virtual machine with Ubuntu 24.04, glibc 2.39, 8 processors, 62.8 GiB of memory and an NVIDIA L40S (46068 MiB, as `nvidia-smi` counts) with the driver 595.58.03, which is CUDA 13.2; the package's binary as it is, `HOME` a folder of its own, `--gpu nvidia --accept-review`, the models and output folders set below that folder): **the first run of this program on an NVIDIA computer.** After `apt install python3.12-venv` (Ubuntu's Python cannot make an environment without it, as the README says), ComfyUI v0.39.0 was installed with PyTorch 2.14.1 for CUDA 13 (about 7 GB) and the seven files were fetched (65 GB), 13 minutes 5 seconds and 67.3 GiB on the disk in all; the size and SHA-256 of every file equal to the configuration's (computed again afterwards from the files: 66 seconds), and PyTorch in the environment ran a matrix product on the card. `up --offline --ready-file` had ComfyUI answering after 6 to 7 seconds; ComfyUI itself listed the seven files in `diffusion_models`, `text_encoders`, `vae` and `loras` and had both nodes of the model (`MiniMaxH3ImageToVideo` and `MiniMaxH3ReferenceToVideo`, from `/object_info`); a SIGTERM ended it in 0.24 seconds with the card's memory at 0 MiB and the port free, and it was started again.
- **Not run:** the model itself (no clip has been made, so what it takes in time, in the card's memory and in the computer's is not measured here), and llama.cpp's CUDA build on a real card. A driver of the 580 series was not checked by anything in this program then; since WP4.20 a configuration can have it looked at (section 12).

WP4.20, 2026-10-13, what the computer has to have, on the development computer (an AMD card; a private home each time, `--gpu nvidia` to name the card the documents ask about):

- **`quickai-run check` of the five configurations of video models** (`minimax-h3.toml`, `minimax-h3-fl2va.toml`, `fasth3-v2.toml`, `wan22-14b-i2v-lightning.toml`, and `ltx-2.5-distilled.toml` with a stand-in login): exit 6 on each, because this computer has no NVIDIA driver, with the look
  printed before the plan (the driver `MISSING`, the compiler `ok: gcc at /usr/bin/gcc`, the headers `ok` in `/usr/include/python3.14/Python.h`, the test `later`; the Wan 2.2 set asks for the driver alone), the plan after it, and the private home left empty.
- **The script of the test, run for real** with the user's own ComfyUI environment (Python 3.13.7 of `pyenv`, PyTorch for ROCm, Triton 3.8.0, a private home and temporary folder): it built Triton's helper with `/usr/bin/gcc` against the headers of that Python, launched a kernel on the AMD card (`hip_utils.c` is
  the helper of Triton's AMD backend as `cuda_utils` is of its NVIDIA one), got the right answer and removed its cache; **4.2 seconds**. Then **the failure that started this work, reproduced for real**: the same interpreter copied without its `include/` folder (the Python of a server that lacks `python3.12-dev`) makes
  gcc say `hip_utils.c:5:10: fatal error: Python.h: No such file or directory` and Triton end with `CalledProcessError: Command '[...]' returned non-zero exit status 1`, which is the one line the first job's message had; the script ends with status 1 and both are the last lines it writes.
- **`quickai-run prepare` through the real binary on that Python**, with a stand-in `nvidia-smi` that says 595.58.03 and a stand-in build folder (a launcher that does nothing, and beside it the Python): with the headers asked for, it stopped with 6, `Python.h is not in /var/tmp/.../py-noheaders/include/python3.13, where Python 3.13 keeps its headers`, offering no package
  (that Python is not the system's); with only the test asked for, it ran the test in 2.7 seconds, ended with 6 and printed `| ... fatal error: Python.h: No such file or directory` under `the environment of ComfyUI failed a test`, with the configuration's words and the document's hint, and the engine's temporary folder empty after; with the Python that has its headers it ran the test in 4.0 seconds,
  said `ok      the environment passed`, and `Everything it needs is in place; nothing was started.` with code 0.
- **The real apt, in a fresh `ubuntu:26.04` container** (root, with `python3` and `gcc` installed and nothing else, which is the state the rented server was found in: a Python without its venv module and its headers, and a compiler; the packages are `python3.14-*` there because that release's Python is 3.14, on the server's 24.04 they are `python3.12-*`): `check` of a configuration that needs the venv module, a compiler and the headers said `MISSING` for the two Python pieces with the real Python's own words (`ModuleNotFoundError: No module named 'ensurepip'`, `Python.h is not in /usr/include/python3.14`), `ok` for gcc, the command `apt-get update && apt-get install -y python3.14-venv python3.14-dev`, and exit 6. `prepare --install-system-packages` then ran exactly that (the package manager's lines passed on, 10 new packages, 16.5 MB), looked again and found all three `ok` (`Python 3.14 imports ensurepip`, `/usr/include/python3.14/Python.h`), and went on to install ComfyUI: **the step that makes the Python environment, which is the one that fails without the venv package, passed**, and PyTorch for the processor and the requirements were being downloaded when the run was cut off at 200 s (step 5 of 7). Afterwards `import ensurepip` works and gcc builds a shared object against `Python.h`.
- **Type-checked for `x86_64-pc-windows-gnu` and `aarch64-apple-darwin`**, which this computer cannot link for: the core crate, and the real `host.rs` in a scratch crate that stands in for the few functions of `runtime::process` it calls (the Unix-only parts are behind `cfg(unix)`; on Windows nothing is found to install with and `which` finds no `.exe`, which no document of this version asks for).
- **Not run for real on the development computer:** installing packages on the development computer itself (only in that container, and against stand-ins of the package manager and of `sudo` in the tests: Y77, Y87 to Y90, Y98); an NVIDIA card, and the test on an NVIDIA driver and `libcuda`. Those were run later the same day on the rented server, below.

WP4.20, 2026-10-13, later the same day, on the rented server (the virtual machine of the WP4.19 run above, with its NVIDIA L40S and the driver 595.58.03; the package of `quickai-run` as built for this work, a private `HOME`,
`--gpu nvidia --accept-review`, the models and output folders set below it). The earlier install (ComfyUI with its environment, 6.8 GiB, and the models, 60.5 GiB) was removed first, so that the program met the computer as the first job
had found it, with the compiler and Python's venv module and without Python's headers:

- **`quickai-run check` of `minimax-h3.toml`**: exit 6, with the look printed before the plan: the venv module `ok` (`Python 3.12 imports ensurepip`), the driver `ok` (`driver 595.58.03`), the compiler `ok` (`gcc at /usr/bin/gcc`) and
  **the headers of Python alone `MISSING`** (`Python.h is not in /usr/include/python3.12, where Python 3.12 keeps its headers`), with the document's hint and the command `apt-get update && apt-get install -y python3.12-dev`; the test
  `later`. That is the failure that started this work, said before any file was fetched.
- **`quickai-run prepare --install-system-packages`**, run as the administrator: it ran that command itself (four new packages, `python3.12-dev`, `libpython3.12-dev`, `libexpat1-dev` and `zlib1g-dev`; none upgraded or removed), looked
  again (all four `ok`, `/usr/include/python3.12/Python.h`), installed ComfyUI v0.39.0 with PyTorch 2.14.1 for CUDA 13, **ran the test of the environment on the card after the install and before the first of the seven files was fetched**
  (`ok the environment passed`), fetched the files and ended with `Everything it needs is in place; nothing was started.` and code 0: 11 minutes 7 seconds and 67.3 GiB in all (13 minutes 5 seconds the first time).
- **Afterwards**: the size and SHA-256 of every file equal to the configuration's, PyTorch in the environment ran a matrix product on the card, `up --offline` had ComfyUI answering after 9 seconds (the test of the environment runs
  again at every start, section 12) with the seven files and both nodes of the model listed, and the script of the test by hand took 2.0 seconds and left nothing behind (F30 of [engine-facts.md](engine-facts.md)).
- **Not run:** on the NVIDIA computer, a test that fails, a driver that is too old, and `prepare` without `--install-system-packages` (`check` showed the stop and its command); only the development computer, the container and the
  stand-ins have seen those. A package manager held by another process was not met on the server (its lock was free; the container below met it, with the program that was deployed there). No clip has been made, and
  llama.cpp's CUDA build has not been run on a real card.

WP4.20, 2026-10-13, after the run on the server: apt and a lock another process holds. Both runs are in a fresh `ubuntu:26.04` container (apt 3.2, root, `python3` installed and nothing else), with another process of Python holding
`/var/lib/dpkg/lock-frontend` for 30 seconds (an `fcntl` lock, like apt's), and the program's own binary:

- **The program as it was deployed to the server** (no wait), `prepare --install-system-packages` of a configuration that needs the headers of Python: it ran `apt-get update`, then `apt-get install -y python3.14-dev`, which ended after a
  second with `E: Could not get lock /var/lib/dpkg/lock-frontend. It is held by process N (python3)`; the program said `installing the packages failed: ... ended with exit status: 100`, ended with code 6 and had installed nothing. That is what
  an install does on a server that is running upgrades of its own.
- **The program with the wait** (the same lock; a configuration that needs the headers and the venv module): it said `running: apt-get install -y python3.14-dev python3.14-venv` (the plain command), apt's `Waiting for cache lock: Could not
  get lock /var/lib/dpkg/lock-frontend. It is held by process N (python3)...` followed once a second for 28 seconds and was passed on, and when the other process let the lock go the 14 new packages (20 MB) were installed, the look was
  made again and found both `ok`, and ComfyUI's install went on, through PyTorch to its requirements (step 5 of 7), until the run was cut off at 170 seconds. What apt does with the option, with a lock held for longer than the wait, and with the lock of the
  lists is F32 of [engine-facts.md](engine-facts.md), measured with apt 2.8 in a container of Ubuntu 24.04, the release of the server.
- **Not run:** the server itself with the wait (the program deployed there is the one before it), a lock held by a real `unattended-upgrade`, and dnf, zypper, pacman and apk at a held lock.

What these did not cover is section 10.

## 12. What the computer has to have (WP4.20)

On 2026-10-13 the first job that was sent to the ComfyUI of the rented server (a clip with a reference picture, MiniMax H3, int8 weights) died after 3.4
seconds, and the message that reached the program that sent it said only that `gcc` had "returned non-zero exit status 1". The cause was in the server's own
log and nowhere else: Triton, which the PyTorch of a graphics card brings, builds a small helper in C the first time a kernel is launched, and the
server had a compiler and no `Python.h` (the package `python3.12-dev`). The install that had made that server was this program's, and it knew nothing of
it. The user's analysis of who owns that (their words; the part about the other program is left out):

> QuickAI - should ensure that the environment is setup correctly: [...] The server's environment belongs to QuickAI, because it provisions the server and
> the H3 profile's int8 weights are what create the need for Triton. The H3 profile should declare its system prerequisites: a C compiler, the Python
> headers (here python3.12-dev) and an NVIDIA driver of the 580 series or newer. quickai-run up should check them, then install them or stop with the exact
> command. It should also run a small self-test before saying "ready", for example compiling Triton's helper. Alternatively, it could build ComfyUI's
> environment on a self-contained Python that ships its own headers.

That is this section. (The alternative is not done: a self-contained Python would bring its own headers, and so take one of the three things away, but it would
still need the compiler and the driver, and it is a Python this program would have to download, pin, check and keep, which is a piece of its own.)

### The document says how, the configuration says which

The engine's document names what can be looked at, how, and what brings it (decisions C134 to C136). The ComfyUI document names five:

```toml
[[prerequisite]]
id = "python_venv"                                                      # ensurepip, which Debian leaves out of the Python (python3.12-venv)
when = "os == 'linux'"
check = { use = "python_module", module = "ensurepip" }
fix = { apt = ["python${python.version}-venv"] }

[[prerequisite]]
id = "c_compiler"
label = "a C compiler"
when = "os == 'linux'"
check = { use = "program", any_of = ["gcc", "clang"], env = "CC" }     # what Triton looks for: $CC, else gcc, else clang
fix = { apt = ["build-essential"], dnf = ["gcc"], zypper = ["gcc"], pacman = ["gcc"] }
hint = "Install gcc or clang, or name the compiler to use with the variable CC."

[[prerequisite]]
id = "python_headers"                                                   # Python.h, for the Python the environment is made with
check = { use = "python_header", file = "Python.h" }
fix = { apt = ["python${python.version}-dev"], dnf = ["python${python.version}-devel"], zypper = ["python${python.digits}-devel"] }

[[prerequisite]]
id = "nvidia_driver"                                                    # the PyTorch it installs for an NVIDIA card is built for CUDA 13
when = "os == 'linux' and has(gpu.vendors, 'nvidia')"
check = { use = "nvidia_driver", at_least = "580" }                     # no package: a driver is not installed here

[[prerequisite]]
id = "triton"                                                           # after the install: build a kernel and run it on the card
when = "os == 'linux' and has(gpu.vendors, 'nvidia')"
check = { use = "python_script", asset = "comfyui/selftest/triton_kernel.py", timeout = "180s" }
```

A configuration says which of them it needs and why, in its own words, in `[[needs]]` with the kinds it already had (a login, a setting, a variable):

```toml
[[needs]]
system = "c_compiler"
why = "The int8 and 4-bit files run through Triton, which builds a small helper in C the first time a clip is made."
```

The five examples of video models say it: the venv module and the driver in every one, and the compiler, the headers and the test in the four whose int8 and 4-bit files run through
Triton (MiniMax H3 in both builds, FastH3 and LTX-2.5), not in the fp8 set of Wan 2.2, which no workflow is known to run through a kernel of Triton's. A configuration that asks for nothing is looked at for nothing; the window
does not show any of it yet.

### What happens, in order

1. **The look**, after the plan is made and before it is printed (so before anything is fetched): `program` finds a name on the `PATH` (or the program the variable
   names), `python_module` asks the Python the install will use whether it imports a module (`ensurepip`: Debian's Python does not without `python3.12-venv`; an environment that is made was made, so it is not asked), `python_header` asks the Python of the environment for its headers folder (the environment beside an installed launcher; for a launcher not yet
   installed, the Python the install will choose, by the same rule) and looks for the file there, `nvidia_driver` reads the version from `nvidia-smi` or from
   `/proc/driver/nvidia/version` and compares it number by number. What was found is printed (`ok`, `MISSING`, `unknown` when it could not be looked at, `later`
   for the test), with the configuration's words and the document's hint under what is missing, and the command that brings what a package can:

   ```
   What this computer has to have:
     ok      the venv module of the Python that ComfyUI's environment is made with (ensurepip): Python 3.12 imports ensurepip
     MISSING an NVIDIA driver of the 580 series or newer: the driver is 535.54.03, older than 580
     MISSING a C compiler: none of gcc, clang is on the PATH
             The int8 and 4-bit files run through Triton, which builds a small helper in C ...
             bring it with: sudo apt-get update && sudo apt-get install -y build-essential python3.12-dev
     MISSING the headers of the Python that ComfyUI's environment is made with (Python.h): Python.h is not in /usr/include/python3.12, where Python 3.12 keeps its headers
             bring it with the same command
     later   a Triton kernel that builds with this computer's compiler and runs on the card: a test of the installed environment ...
   ```

2. **What is missing stops the run** with code 6 and the sentence that lists it, the command, and what no package brings. `check` does this too, after the plan, and
   changes nothing. With `--install-system-packages` (C138) the packages are installed instead (as the administrator, or through `sudo -n`; the list of
   packages first, each package once; every line passed on; apt's install told to wait up to two minutes for a lock another process holds, and apt's own `Waiting for cache lock`
   lines, one a second, are among the lines passed on), the computer is looked at again and said again, and the run goes on if nothing is left; a package manager that
   fails, `--offline`, no `sudo` without a password, a manager the document has no package for, and a thing no package brings each stop the run with a sentence that says why. The
   packages are the Python's own only when that Python is the system's (`/usr`): for a Python of `pyenv` or `uv` the headers come with it, and a file
   missing from its folder is not put right by a package of the system.
3. **The test of the installed environment** (C139) after the install and before the first file of a model is fetched (or before the start, when there is nothing to fetch). ComfyUI's
   test writes a tiny Triton kernel, builds it from an empty cache and runs it on the card (`assets/comfyui/selftest/triton_kernel.py`), and ends with a status that says which
   step failed: PyTorch cannot be imported (2), cannot use a card (3, with PyTorch's own reason, such as a driver too old), Triton cannot be imported (4), the kernel could not be
   built or run (1, with the compiler's own words before the last line, which is what the message of the first job lacked), a wrong answer (5). It runs in a few seconds and on every `up`
   and `prepare` of a configuration that asks for it; `check` lists it as `later`. A failure stops the run, before the models are fetched and before ComfyUI is started.
4. `--skip-prerequisites` (C140) leaves out the look and the test.

### What it does not do

- **The window shows none of it.** A missing compiler is not a note in the window's review of a configuration, and the window starts ComfyUI as it always did. The pure
  half is in the core (`quickai_core::prerequisite`), so it can.
- **Only the document of ComfyUI names prerequisites.** llama.cpp's CUDA build (which also wants a driver of the 580 series for CUDA 13, section 5) and the other engines say nothing yet; the
  mechanism is the same for them, with a document of their own.
- **A driver is never installed**, and a Python is never brought: when the computer has none of the versions the install wants, the install says so and no package is named for "a Python". The venv module of
  the one it has is a prerequisite (`python_venv`) that can be looked at and installed, for a configuration that asks for it; one that does not meets its absence in the install, with the installer's own sentence (TC-RUN-124).
- **Only apt's install waits for a lock.** The refresh of apt's lists takes another lock, the lists', which `-o DPkg::Lock::Timeout` does not cover (F32): with that lock held (the
  system's own refresh running) the program's refresh ends at once and the install goes on with the lists there are, as after any refresh that fails. dnf, zypper, pacman and apk are
  run as they are printed; what they do at a held lock was not looked at. A stop given while apt waits is answered when the wait is over (two minutes at most), because a package
  manager is not stopped half way through.
- **A configuration that asks for nothing is looked at for nothing**: a ComfyUI on an NVIDIA card whose configuration does not say `system = ...` meets Triton's
  compiler problem at the first kernel, as it did.
- **Run on an NVIDIA computer once, and only the way that works**: the tests (Y71 to Y99d) use stand-ins for the compiler, the package manager, `nvidia-smi` and the Python; the script of the test
  was run for real on the development computer (an AMD card), and the whole of it on the NVIDIA L40S of the rented server, where one package was missing, the program installed it and the test passed
  (section 11). The stops other than that one (a driver too old, a compiler that is missing, a test that fails) have been seen only on the development computer, in a container and against the stand-ins.

Tests: group Y of [phase3-test-plan.md](phase3-test-plan.md), Y71 to Y99d (the looking, the installing and the running of a script: `runtime/host.rs`; the documents and the
configuration: `core/tests/p3_prerequisites.rs`; the whole through `provision::up` against a described computer: `runtime/p3/prerequisites.rs`; the words and the program: `run`), and the Robot suite
`headless_prerequisites` of the GUI repository (TC-RUN-112 to -129, 18 cases: the look, the command, the install through `sudo`, a driver no package brings, the test that fails as a real one does without
`Python.h`, and the failures of installing what Python needs: the package manager that cannot find the headers' package, one that installs the headers of another Python, a Python without its venv module,
pip with no matching distribution, a Python that cannot be run, an environment that is made).

**Mutations.** 31 pieces of the Rust code were broken on purpose, one at a time (a condition ignored, a flag read the wrong way, the test run after the fetch, the refresh of apt's list made fatal, `sudo` left out of the printed command, the environment not passed to the script...), and a test was seen to fail for each; the one that was not noticed at first was a second guard for the same thing (the person is not asked for what the computer is looked at for), and is now one guard that is tested. 6 of them were also broken in the program the Robot suite runs, and each was seen to fail a case there (TC-RUN-112, -113, -116, -122). Seven more were made for the wait for a lock (Y78c: the option never added, given to the refresh too, printed for a person, run from the printed words, a wrong number of seconds, another manager run with other words than it is printed with, the option before the subcommand), each seen to fail a test, and the first again in the program the Robot suite runs (TC-RUN-113, -123, -126). The list is in the run log of the [implementation plan](implementation-plan.md); the script that made them is not part of the repository.

## 13. A run that lives until it is stopped (WP4.33.2)

For a run another computer asks for over SSH ([remote-runs.md](remote-runs.md), section 3.1), and as useful on a server by hand:

```sh
quickai-run up wan.toml --detach   # start it in the background; it goes on when the terminal or the connection closes
quickai-run status [--json]        # what runs (the configuration, since when, where it listens), or how the last run ended
quickai-run down [--no-wait]       # stop it, in the background or in another terminal, and wait until it has
```

- **One run at a time** on a computer, for one person: `up` and `prepare` hold the run's folder (`~/.quickai/run`, the data
  folder's `run`) with a lock the system lets go of when the run ends, however it ends, and a second one says what runs and exits
  with 7. `check` changes nothing and holds nothing.
- **What runs is said in `run.json`** beside the lock (the process, the configuration, the version, since when, `starting`,
  `preparing`, `ready` or `stopping`, and once ready where each thing listens); **how the last run ended** in `last.json`;
  **what a run in the background said**, one JSON event a line, in `events.jsonl` (and its error stream in `stderr.log`).
  `status --json` is `quickai_runtime::run_file::Status`: the window reads it over SSH.
- **`down` writes a `stop` file** the run looks for each second: a stop is the same on Linux, macOS and Windows, where there is no
  SIGTERM to send to a program of another console. Ctrl-C and SIGTERM still stop a run in a terminal; a hang-up stops it only when
  it is not in the background.
- **`free PORT... [--json]`** stops the programs that listen on those ports (for a window that was told to override
  them, [remote-runs.md](remote-runs.md) section 3.2): a program of the person is asked to end, then ended; a system
  service is stopped by its manager (`systemctl` on Linux, `launchctl` on macOS) through `sudo -n` when that needs no
  password, and otherwise the command that stops it is said; on Windows `taskkill`. It exits with 8 when a port stays
  taken. `check` says the ports a run would listen on and whether another program has one now (the `ports` event,
  with what answers there when it answers as the engine does).
- **`--detach`** starts the program again with `--detached --json` and returns once the run has made its plan and gone on (or has
  ended, with its code and why, or after a minute). On Linux and macOS the run is in a session of its own; on Windows it has no
  console, a group of its own, and is started out of the job Windows' OpenSSH server closes with a connection (asked again without
  that when the job does not allow it; the Task Scheduler is the way left if neither holds, to be found out on a Windows server).

