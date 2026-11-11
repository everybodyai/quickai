# Headless: `quickai-headless`

`quickai-headless` runs a QuickAI configuration on a computer without a display, such as a rented GPU server. It installs what the configuration requires, downloads its models, starts it, and keeps running until it is stopped. This page describes what it does, how it does it, and what has been tested in practice.

Before version 0.2.6, this program was named `quickai-run`; its commands, options and files are otherwise unchanged.

## 1. Overview

`quickai-headless` is QuickAI without a window, for computers that have no display. It takes a **configuration** (the file the window exports and
imports; see [configurations.md](configurations.md)), determines what that configuration requires on this computer, installs anything that is
missing, downloads the models, starts everything, reports where it is listening, and keeps running until it is told to stop.

```sh
quickai-headless init  wan.toml        # the file to fill in: what the configuration needs from you, and what is the computer's
quickai-headless check wan.toml        # what would be done, from where, to where, how big it is; changes nothing
quickai-headless up    wan.toml        # do it, start it, stay (Ctrl-C or SIGTERM stops it)
quickai-headless prepare wan.toml      # install and fetch, start nothing
```

Four conditions had to be met, and the first accounted for most of the work:

1. **What QuickAI does to a computer no longer lives in the window.** Previously, installing, downloading, starting and stopping were
   implemented in the window's own code. They now live in a separate crate, `quickai-runtime`, which both the window and this program use
   (section 2).
2. **The sequence that the buttons perform step by step is a single call**: `provision::up` (section 3).
3. **It works on a server**: graphics cards are detected without `lspci`, stopping is done with a signal, no user interaction is
   required, and it listens on the loopback interface only (section 5).
4. **The configuration describes everything that is to run, and the computer-specific part belongs to the operator** (WP4.19, sections 6
   to 9): a configuration states where each model file can be obtained and never contains the file itself; it states which companion
   program accompanies the engine and which build of it; and it states what it requires from whoever runs it. Everything that belongs to
   the computer (a folder, a login, a variable in a program's environment, a mirror, a copy of a file that is already present) is supplied
   by the operator, either in a file that `init` generates or on the command line.

## 2. The crate split (WP4.18.1)

```
quickai-core      pure: documents, expressions, the planner. No processes, network, files or window.   (as before)
quickai-runtime   every effect, no toolkit: find, install, fetch, start, watch, stop; settings and configurations on disk;
                  the documents and assets the application ships.                                       (new)
quickai           the window. It draws; what it does to the computer it asks the runtime for.            (what is left of it)
quickai-headless       the headless program: arguments, text or JSON lines, signals.                          (new)
```

The window consisted of 47,000 lines in a single crate. The code that acts on the computer contains no `egui` (it communicates through
channels and closures), so the split was made between `app/` and everything else. The effect modules moved together with their tests,
their fakes, the documents they embed and the assets they write (using `git mv`, so that the history is preserved). None of that code was
rewritten for the move.

**How the move was verified to change nothing** (before any new code was built on top of it):

- every one of the 1,042 tests that existed before exists afterward under the same name (`cargo test -- --list`, sorted and compared), and
  the totals are unchanged: 1,022 passed, 0 failed, 20 ignored;
- the 28 headless commands (`config check`, `config docs`, `plan` for each engine with and without options, `configuration list|show|
  export|check`, and the usage errors) print the same bytes and exit with the same codes before and after (the only difference is a path
  in the developer's tree, `crates/quickai/../..` becoming `crates/quickai-runtime/../..`, which refers to the same directory);
- fmt, clippy with warnings denied, and the complete GUI test suite (which the maintainers keep outside this repository) were run against
  the new executable (section 10);
- two guard tests keep it that way (`tests/no_window.rs`, O01 and O02): the runtime depends on no GUI toolkit and names none in its code,
  and the window crate contains `main.rs` and `app/` and nothing else.

Beyond moving files, three pieces of logic that the window and the command line each had, and would otherwise have had to duplicate, were
unified:

| Function | Previously | Used by |
|---|---|---|
| `resolve::resolve` | the body of the `plan` command: turns a configuration, a model, values and a program into the plan of a run | `quickai plan` and the headless run, so the plan that is printed is the plan that is executed |
| `models::catalog_fetch` (the core) | the decision inside the window's *Get* button: which download retrieves a model from the list (the pairing of a projector with its weights comes first) | the window's *Get* button and the headless run |
| `acquire::pick_build` (the core) | nothing: the window shows a list and the user picks | the headless run, when no build is named |

## 3. What it does, step by step

`quickai-headless up <configuration>`, where `<configuration>` is the ID of a saved configuration or the path to a file:

1. **Reads and reviews** the configuration for this computer, as the window does when a file is imported (`profile::check`). A file that
   cannot be used here stops the run (exit 1 or 3). Anything a file from outside *asks* for (free text that reaches a command line,
   settings that widen what a program may do, a source it names for a model) is shown and is not used unless `--accept-review` is given.
2. **Applies what the operator specifies about this computer** (an environment file, `--set`, `--env`, the logins; section 7). If the
   configuration still requires something that nobody has supplied, the run stops there with code 5, before anything is downloaded.
3. **Plans**: for the engine, and for its companion program when the configuration starts one (Open WebUI alongside Ollama), it determines
   what is missing (a program, the files of a model) and what happens once they run (start, then pull and load a model for an engine that
   takes models by name). The plan is printed first, with sizes, how each download is verified, and, for each file, its source address and
   destination. `check` (or `--dry-run`) stops here.
   **Before the plan, the computer is examined** for what the configuration says it must have (`[[needs]] system = "..."`: a C compiler,
   the Python headers, a driver of the 580 series; section 12). The findings are printed; anything missing stops the run before anything is
   downloaded, with a dedicated exit code (6) and the exact command that installs it, or is installed if the operator asked for that
   (`--install-system-packages`).
4. **Installs and downloads**, in that order, reporting progress in tenths and on changes rather than on every poll. A second run finds what
   the first left in place and only starts. A file the operator already has is verified and linked rather than downloaded. Between the
   installation and the first download, the tests that the engine's document defines for the installed environment are run (building and
   running a Triton kernel once: section 12), so that a computer that fails one learns this right after the installation rather than after
   tens of gigabytes.
5. **Starts** the engine. For an engine that takes models by name, it asks the engine which models it has, pulls the missing ones and
   loads the first. It then starts the companion program, telling it where the engine is.
6. **Reports where it is listening**: it prints `Ready.` and the addresses (and writes them as JSON to `--ready-file`, which is removed when
   the run stops).
7. **Waits.** SIGTERM or Ctrl-C stops the companion program first and then the engine, each with the grace period its document specifies.
   A program that exits on its own ends the run (exit 4), reporting the last lines it wrote.

No engine is named in the code: the documents describe how each engine is installed, started and queried for its models, just as they do
for the window.

### How the plan is built

- **The engine's program.** If the operator named a program (`--set <engine>.bin_override=...`, a separate installation of their own), that
  program is used and nothing is installed. If the configuration names a build (`[runtime] build`) or `--build` does, that build is used.
  Otherwise, the program the engine's document finds on its own is used (a package's, one on the path, or the newest installed). If there
  is none, one is installed, chosen by `pick_build`, which follows the document's declarations and is the same for every engine:
  1. a build whose pin and the feed's digest disagree is never taken;
  2. a build for a backend that requires a vendor's card (`needs_gpu`) is never taken unless that card is present, so a computer whose
     cards could not be identified is not given CUDA;
  3. the tested release if the document names one, otherwise the newest; among its builds, the backend the document lists first;
  4. a build whose name indicates no backend ranks after all of them, except on a computer whose cards could not be identified, where it
     ranks first: it is what a machine without a graphics card runs;
  5. among builds that rank equally, the smaller download. (A document does not list every backend a project publishes: the first real
     run encountered an OpenVINO build of llama.cpp that no document names, which, like the plain build, "names no backend" and was listed
     first; it is 105 MB compared with the plain build's 16. [engine-facts.md](engine-facts.md) L24.)
  If nothing on offer can run here, it says so and installs nothing. A named build older than the most recent few is searched for in as
  many releases as the host provides. Overlays (Ollama's ROCm libraries) are not installed unless requested in the window.
- **Model files** are covered in section 6. A file that is already present at the expected size is not downloaded again; before the first
  byte, the available space is checked on the disk the files go to.
- **A model that an engine pulls by name** (`ollama = "qwen3:8b"`) is pulled with the engine's own progress once it is running, unless the
  engine already has it, and is the model that is loaded. For an engine that runs on its own (ComfyUI), the models are the files in the
  configuration's `[[model]]` entries, placed in the folders ComfyUI searches.
- **The companion program** is covered in section 8.

### Commands, exit codes and output

```
quickai-headless up|prepare|check <configuration> [--accept-review] [--build [PROGRAM=]LABEL] [--port N] [--models-dir DIR] [--python PATH]
                             [--gpu nvidia|amd|intel|apple|none] [--allow-program HASH] [--dry-run] [--json] [--quiet] [--ready-file PATH]
                             [--environment FILE]... [--set ENGINE.FIELD=VALUE]... [--env [PROGRAM:]NAME=VALUE]...
                             [--model-source FILE=WHERE]... [--models-from DIR]... [--offline]
                             [--hf-token TOKEN] [--hf-endpoint URL] [--github-token TOKEN]
                             [--install-system-packages] [--skip-prerequisites]
quickai-headless init <configuration> [-o FILE | -o -] [--force]
quickai-headless plan | config | configuration ...         QuickAI's other headless commands, the same code as `quickai`'s
```

| Code | Meaning |
|---|---|
| 0 | completed, or stopped on request |
| 1 | the task could not be completed: an unreadable file, nothing it needs can be obtained, an installation or a download failed, a program would not start, or a file is missing in offline mode |
| 2 | an error in the command (the usage follows), or in what was specified about the computer (a setting the engine does not have, a name that is not a file of the configuration) |
| 3 | the file asks for something that was not accepted, or a review note stops it |
| 4 | a program stopped on its own after it was started |
| 5 | the configuration requires something from whoever runs it, and it was not supplied |
| 6 | the computer lacks something the configuration requires (a program, a Python header file, a driver), or the installed environment failed the test its document defines |
| 130 | stopped before completion (a signal during an installation or a download) |

Messages go to standard output and failures to standard error. `--json` prints one object per line for each event (`computer`,
`environment`, `review`, `prerequisites`, `plan`, `step`, `progress`, `step_done`, `self_test`, `state`, `log`, `ready`, `stopping`, `finished`), the last always being `finished`.
`prerequisites` has `items`, one for each thing the configuration requires of the computer (`id`, `label`, `state`, which is `met`, `missing`, `unchecked` or `later`, `detail`, and, where applicable, `why`, `fix` (the command that provides it) and `hint`); `self_test` has `id`, `label`, `state` (`running`, then `passed` or `failed`) and `output`, the lines the test wrote. A plan step that places files in the models folder lists them individually (`files`: the name, `fetch` or `link`, the source, the destination, the size, and the checksum
when known), so that another program can read the plan and perform the downloads itself (section 6).

## 4. Design decisions

| # | Decision | Rationale |
|---|---|---|
| C117 | **The application consists of four crates**: `quickai-core` (pure), `quickai-runtime` (every effect, no toolkit), `quickai` (the window) and `quickai-headless` (the headless program). The window keeps its module paths (`crate::config`, ...) by naming the runtime's modules at its root. | The user: "make things modular in the current QuickAI". A second program that duplicated the installation logic would drift from the first; a crate boundary enforced by the compiler does not. |
| C118 | **Running the headless program is the equivalent of the click.** A configuration still only selects (C103); `quickai-headless` is the action: it installs and downloads what the file requires, after printing what that is and how large. The window's rule (nothing is downloaded until a button is pressed) is unchanged. | The user asked for a program that "has to get all the info from the config, download/configure itself and start". Ground rule 8 (no unexpected network traffic) is met by the plan printed first, by `check`, and by `--accept-review` for what a file from outside asks for. |
| C119 | **When no build is named, the document's declarations choose it** (`pick_build`, section 3). | A program with no list for a user to read must still neither install a CUDA build on a machine whose cards it could not identify, nor guess. The rule is generic and tested against the shipped llama.cpp and Ollama documents. |
| C120 | **Graphics cards are detected without `lspci`**: when `lspci` cannot be run, the kernel's device list (`/sys/bus/pci/devices`, display class) and the NVIDIA driver's files are read instead. An `lspci` that responded is trusted even when it lists nothing (the GUI tests place a stand-in first on the path to control what the application is told, and found that consulting a second source would make the result depend on the computer the tests run on). `--gpu` overrides all of it. | A rented server's minimal image has no pciutils, and a machine whose card is not detected is given PyTorch for the processor (ComfyUI's installation chooses by `has(gpu.vendors, ...)`). |
| C121 | **It listens on the loopback interface only**; access from elsewhere is through an SSH tunnel. There is no `--listen`. | Serving beyond the loopback is a non-goal of the architecture, and ComfyUI has no login. |
| C122 | **A user document that starts a program QuickAI did not install asks once, by hash** (`--allow-program`), as the window's prompt does. A file from outside that asks for something is not used without `--accept-review`: free text that reaches a command line, settings that widen access, and a model source it names that is not the recommended list's. | The guard (C33) and the review (C104) are the same for both programs; the window never downloads from such a hint on its own either (its search is a button). |
| C123 | **`quickai-headless` writes nothing outside the data directory** (`~/.quickai`, or below `HOME`): not `settings.json` and not the configuration directory (`configuration import` is the one command that does, and it is the same code as `quickai`'s; `init` writes only the one file the operator asked for, where they asked). | A server has no window to own those files, and someone who runs both programs on one computer must not have them changed by a command. |
| C124 | **Linux and macOS first.** On Windows, `quickai-headless` initially reported that it was not yet available; since WP4.33.1 (section 13) it runs there too. It is built and type-checked here, but has not yet been run on Windows. | A run relies on signals and process groups; the Windows branch could not be built or run here, and a program that has never been run is not offered. |
| C125 | **A configuration states where a model can be obtained, and never contains the model.** A file model has `from` (a type of source and a name within it), `path` (the file within that repository), `revision` (a branch, tag or commit), `into` (the subfolder of the models folder) or `url` (an `https://` address with no login in it), plus `size` and `sha256`. No key accepts a path on a computer, and a file larger than 256 KiB is not read. `quickai-headless` downloads from these sources (C109 remains in force for the window: it shows the source and downloads on a click). | The user: "it should just carry the information about where to fetch it from and the runtime will autofetch". The Wan 2.2 set the user runs comes from three repositories, with one of the files in a subfolder of its repository: that is what `path` is for. The recommended list's `catalog` remains the way to reference a model the list vouches for. |
| C126 | **What the operator specifies about a file's location takes precedence over the configuration, and is verified.** `--model-source FILE=WHERE` (a path to the file or to a folder containing it, an address, or `huggingface:owner/repo`), `[models]` in an environment file, `--models-from DIR` (searched before anything is downloaded: by name, in the folder for its type, and by its path within its repository), and `--models-dir`. A file used in place is **linked, not copied**, after its size and, when known, its SHA-256 are verified; a file of a different size or content is rejected with an explanation, and nothing is put in its place. A name that is not a file of the configuration is an error that lists the names it has. A source named by a file from outside is queried only when the program is about to download from it, not when the operator has stated where the file is. | The user: "the user should have the ability to specify the location". A copy already on a server (a mounted disk, or a file copied with `scp`) must be usable without downloading 13 GB again, and without being trusted unverified. The first version suppressed the question when a folder contained a file of the right name, so a file of the wrong size stood in for the operator's consent (found while documenting the rule, Y33). |
| C127 | **"Alternate means" are: bring the files by any means, or point to a mirror; the program verifies them.** `--offline` downloads nothing and reports what is missing, one file at a time, with its destination and the address it would have come from; `check --json` provides the same for scripts. There is no hook that runs a command to download. | The user: "or provide alternate means to get it". A download command would be a command the program runs on the word of a file or a flag; the means an operator has (`aria2c`, `rsync`, `scp`, a mounted disk, a mirror) all end with a file in a folder, which is verified. |
| C128 | **A login is sent only to the host it was given for.** `--hf-token` / `HF_TOKEN` / `[fetch] huggingface_token` is sent as a bearer token to the hub (or to the mirror that replaces it), and is forwarded only through a redirect within that host; the signed address to which a hub redirects a file (the real hub redirects every large file of the Wan 2.2 set to `us.aws.cdn.hf.co`: L26) receives none, and neither does an address the operator names on another host. `--github-token` / `GITHUB_TOKEN` authenticates the requests for the list of builds (60 per hour without it) and nothing else. The four variables that can hold a login (`HF_TOKEN`, `HUGGING_FACE_HUB_TOKEN`, `GITHUB_TOKEN`, `GH_TOKEN`) are removed from the environment of every program it starts. A 401 or 403 response explains what to do. `--hf-endpoint` / `HF_ENDPOINT` replaces the hub's address in every download. | A server shares an IP address with other people's servers, and its 60 requests per hour run out; a gated repository requires a login. A token that followed a file to the next host would be handed to a host nobody chose. |
| C129 | **What belongs to the computer is specified by the operator running the configuration**, not by the configuration: an **environment file** (`--environment FILE`, TOML: `[fetch]`, `[values.<engine>]`, `[env.<program>]`, `[models]`; an empty value is not applied; `init` generates it), `--set <engine>.<field>=VALUE` (a setting of the engine or of its companion program, including those a configuration cannot carry: a folder, a Python, a launcher), `--env [program:]NAME=VALUE` (a line in that program's environment, after the configuration's own lines), and the usual variables for the logins. A flag takes precedence over a variable, a variable over a file, a later file over an earlier one, and all of them over the configuration. `share` does not apply to the operator's own input (it is theirs); the names an engine's document reserves for itself (`OLLAMA_HOST`, `DATA_DIR`, `HOME`...) still cannot be set from here. The file must be named explicitly: nothing is read merely because it sits next to the configuration. | The user: "the environment specific variables if any are to be specified by the user - either through a configuration file (which you ask the user to fill) or via commandline etc." The engine documents already state which settings belong to a computer (`share = "never"`) and which program accepts environment lines (the field that binds them), so the file to fill in is generated from them and the program names no engine. |
| C130 | **A configuration can state what it requires from whoever runs it** (`[[needs]]`: a login, a setting, or a variable in a program's environment, each with a reason). `quickai-headless` stops with a dedicated code, 5, before it downloads or installs anything, names each item with its reason and how to supply it, and points out that `init` generates the file to fill in. `init` builds that file from the configuration and the documents: each required item as a line to complete, the settings that belong to a computer, the environment lines, and the files the configuration downloads, each with its source and size. It is written readable by its owner only and is not overwritten unless requested. A value left empty is not supplied. | "which you ask the user to fill": the program asks, and explains how. Stopping before the first byte means an operator who forgot a login does not wait through a 17 GB installation to find out. |
| C131 | **Secrets are kept out of everything the program outputs.** A login is a type that prints as hidden regardless of how it is printed (`Debug`, `Display`). Messages, events, JSON, the ready file and standard error state where a value was supplied and show the value unless its name marks it as a secret (KEY, TOKEN, SECRET, PASSWORD) or it is a login, in which case they show `(hidden)` and `null`. A block of environment lines is counted, not shown. The file `init` writes has mode 600, and a warning is given for an environment file that holds a secret and is readable by others. Paths are shown in full. | The first version of the `Debug` output of an environment line printed its value (caught by the test that now covers it, Y23), and the first version of the message about a folder truncated it at 60 characters (caught by the GUI tests). |
| C132 | **A companion program is part of the configuration, and its build can be pinned.** `[values.openwebui] enabled = true` (with its port and internet access) is how a configuration includes it; `[runtime.openwebui] build = "0.11.4"` specifies which build is installed, and `--build openwebui=LABEL` takes precedence over it. It is installed, configured (told where the engine is, with its data under its own folder and the environment variables supplied by the operator) and started after the engine, and stopped before it. The window's Save preserves what the window cannot represent: the pins, the needs, and the models beyond the one it shows, with each one's source (it writes the model it shows, and the others are kept as long as that model remains first). | The user: "the configuration should specify that as well and must be able to download and configure that too". A server must be able to receive the same Open WebUI twice, which a version that follows the document's tested release cannot guarantee. |
| C133 | **What a configuration may not do.** It names no program and cannot add code to an engine: the ComfyUI custom nodes that its document installs belong to the document, and anyone who wants others writes a document of their own (which asks, by hash, before it starts anything). `into` must be one of the folders the engine's document says it keeps files in, so a file cannot be placed where the engine does not look. A file name consists of the letters, digits, `.`, `-` and `_` that the models folder allows. | The architecture's rule that a data-tier document names no program; a shared file must not be able to make a server run someone else's Python. |
| C134 | **What the computer must have is declared in two places, each stating only what it knows.** The engine's document names the prerequisites (`[[prerequisite]]`: an ID, a label, a condition on the computer, a type of check, the packages that provide it, and a hint); a configuration states which ones it needs (`[[needs]] system = "<id>"`, with its own explanation of why). Nothing is checked that no configuration asks for. | The need arises from the model's files (the int8 and 4-bit builds of H3 cause Triton to build a helper in C); how to check, and what provides it, belong to the engine and to the PyTorch its document installs. The first clip of a real run of H3 on a rented server failed because of it (section 12), and the program that had set up the server knew nothing about it. A prerequisite is not imposed on every configuration of the engine: a Wan 2.2 set in fp8 does not need a compiler. |
| C135 | **Packages are named by a document, never by a configuration.** A package is installed as the administrator, and its installation scripts also run as the administrator, so a package name taken from a file that has been shared would be a way to run someone's code as root. A configuration can only name the ID of a prerequisite that the engine's document defines (an unknown ID is rejected when the file is read, with a list of the valid ones); a document's package names are restricted, when it loads, to plain names with no option, path or shell syntax (the only placeholders are `${python.version}` and `${python.digits}`); and a driver has no package at all. | The same rule as C133 (a configuration names no program), applied to the one place where this program would run a name from a file with more than the user's privileges. |
| C136 | **What can be checked is a closed list, and a document combines its items** (the primitive `prerequisite@1`): `program` (one of the names on the `PATH`, or the one a variable names: Triton reads `CC`, then gcc, then clang), `python_module` (a module that the Python used to create the environment can import: `ensurepip`, which Debian places in `python3.X-venv`), `python_header` (a file in the header folder of the Python used to create the environment: the one beside an installed launcher, otherwise the one the installation will use), `nvidia_driver` (as reported by `nvidia-smi`, otherwise by the driver's own file; compared number by number), and `python_script` (one of the application's own scripts, run by the environment's Python in the environment the engine is started with; it passes on exit status 0). | Documents name audited actions and contain no commands (the architecture's rule for primitives). The compiler and the headers are checked the way Triton checks them (F29), so that the program is neither stricter nor looser than the component it protects. |
| C137 | **The check is made before anything is downloaded and reported before the plan; anything missing stops the run with a dedicated code (6) and the exact command.** `sudo apt-get update && sudo apt-get install -y build-essential python3.12-dev`, with `sudo` when the operator is not the administrator, and for the package manager the computer has (apt-get, dnf, zypper, pacman, apk). `check` reports the same and installs nothing. A missing item that no package provides (a driver) is reported with the document's hint. | 6 rather than 5: 5 is something an operator supplies, in a file or on the command line; 6 is something the computer lacks, and a service unit can distinguish the two (`RestartPreventExitStatus`). Before the plan, because an operator running `check` should see what the computer lacks and what would be done, in that order. |
| C138 | **Packages are installed only when the operator requests it** (`--install-system-packages`): C123 (it writes nothing outside its data folder) holds unless this is requested. In that case: as the administrator, or through `sudo -n` when that requires no password (nothing here can answer a prompt); apt's package lists are refreshed first (a rented server's lists are as old as its image); apt's install is told to wait up to two minutes for a lock held by another process (`-o DPkg::Lock::Timeout=120` after `install -y`, F32; the command printed for the operator remains the plain command); each command leads its own process group, so that a Ctrl-C in the terminal does not interrupt a package manager midway through a package; every line it prints is passed on; `--offline` and `check` never install; and on failure, the message names the command and its last lines. The computer is checked again afterward, and anything still missing stops the run. | A rented server belongs to its administrator, and installing two packages on it is what an operator would type; but a program that changes the system when asked to run a configuration goes beyond what C118 ("running it is the click") covers, which concerns its own folder. The flag is the click. A rented server runs its own upgrades in its first hours, and they hold apt's lock; without the wait, an installation that encounters one ends immediately (F32), and the operator has to find out why and run it again. |
| C139 | **The installed environment is tested before it is used: after the installation and before the model files are downloaded.** The tests the document names (`python_script`) are run with the environment the engine is started with (the same plan: its home, its temporary folder, the user's Python variables removed), from a folder created for the test and removed afterward, and the first failure stops the run, reporting how it ended, the last lines it wrote, the configuration's explanation and the document's hint. ComfyUI's test builds and runs a Triton kernel from an empty Triton cache, so that the helper is built here, in a few seconds, rather than found from an earlier run. `check` lists a test but runs none. | The check can find a compiler and still be wrong (a compiler without its libraries, a driver that PyTorch cannot use, no `libcuda` to link against); the test exercises the real thing. It runs before the models because they amount to 65 GB and 13 minutes: the first clip should not be what discovers the problem. |
| C140 | **`--skip-prerequisites` skips all of it**, both the checks and the tests, and says so. | An operator who knows better (their own Python build, a compiler Triton finds under another name, a test that is wrong for their card) must not be blocked by a check they cannot get past. |

## 5. On a rented server

```sh
# on your computer, once: the configuration you made in the window, or one you wrote (docs/examples/profiles/wan22-ti2v-turbo.toml)
quickai configuration export vision-chat -o vision-chat.toml
scp vision-chat.toml you@server:
scp quickai-headless-0.1.0-linux-x86_64.tar.gz you@server:        # the package of quickai-headless; unpack it there

# on the server
./quickai-headless check vision-chat.toml          # read it: the plan, the sizes, from where, to where, what asks
./quickai-headless up    vision-chat.toml          # do it
```

- **As a service.** `quickai-headless up` is a foreground program that exits with 0 on SIGTERM, so a unit file needs only four lines
  (`ExecStart=/home/you/quickai-headless up /home/you/vision-chat.toml --environment /home/you/vision-chat.environment.toml`, `Restart=on-failure`, `User=you`,
  `KillSignal=SIGTERM`); the logins can be set in the unit's `Environment=` or `EnvironmentFile=` (`HF_TOKEN=...`) instead of the environment file.
  Add `RestartPreventExitStatus=2 3 5 6`: exit 2 is an error in the command or the file, 3 is a file that asks for something nobody approved, 5 is a
  configuration that requires something nobody supplied, and 6 is a computer that lacks something the configuration requires; restarting gives the
  same result (`Restart=on-failure` is for 1 and 4: a download that failed, or a program that stopped on its own). A second SIGTERM (or Ctrl-C) does
  not wait for the first to complete: it ends this program immediately, with 130, without stopping what it started. On Linux, the kernel tells each
  program it started to exit (SIGTERM) when this one dies, which the real `llama-server` was observed to do (section 11); on macOS, a program would be
  left running. `--ready-file` is the file a script waits for.
- **Storage locations.** Under `~/.quickai` (programs under `llama-bin/`, `ollama/`, `comfyui/`, `openwebui/`; models under `models/`). On a
  server with a small system disk and a large disk mounted elsewhere, run it with `HOME=/mnt/big`, pass `--models-dir`, or set the folder in the
  environment file (`[values.comfyui] models_dir`). A ComfyUI installation takes 17 GB with PyTorch for AMD and about 7 GB with PyTorch for CUDA 13,
  and requires 30 GiB of free space either way.
- **Access.** It listens on `127.0.0.1`. From your computer: `ssh -N -L 8189:127.0.0.1:8188 you@server`; the address is then
  `http://127.0.0.1:8189`. A program that talks to ComfyUI through a base URL needs only that address (or the tunnel on the default
  port).
- **Graphics cards.** They are detected from the kernel; if the result is wrong, use `--gpu nvidia`. The `This computer:` line reports what was
  detected.
- **Python: none required.** The ComfyUI and Open WebUI installations create a Python environment with QuickAI's own Python (CPython 3.12, downloaded
  once into the engine's folder, `python/`, and verified against the SHA-256 that the engine's document pins; the plan says so). Nothing of the
  computer's own Python is needed or changed, and llama.cpp and Ollama need no Python at all. `--python PATH` names a Python on the computer to use
  instead (3.10 or newer; for Open WebUI, 3.11 or 3.12); it must then include the venv module (`python3.12-venv` on Debian and Ubuntu). A Python that
  is only a version-manager shim (`pyenv`) for a version that has not been selected does not respond: name its actual file
  (`~/.pyenv/versions/3.12.3/bin/python3.12`).
- **A compiler (and a driver).** A ComfyUI configuration that runs the int8 and 4-bit video models declares that the computer needs a C compiler and
  the headers of the Python the environment is created with, because Triton builds a helper in C the first time a kernel of the model is launched
  (section 12). QuickAI's own Python includes its headers; a Python named with `--python` may not (the package `python3.12-dev`). A minimal Ubuntu
  server has no compiler (it is in `build-essential`); `quickai-headless` checks before downloading anything, reports what is missing with the exact
  command, and installs the packages itself with `--install-system-packages` (as the administrator, or through `sudo` when that requires no
  password).
- **System libraries.** llama.cpp's own build needs a few libraries that a minimal image lacks: `libgomp1` and OpenSSL 3 (`libssl3`), as well as
  glibc 2.34 or newer. When one is missing, the failure names it in the program's own words (`error while loading shared
  libraries: libssl.so.3`): install it and run the same command again, which picks up what the first run installed.
- **NVIDIA.** The document lists CUDA first, and the two CUDA builds of a release rank equally, so the lighter one, CUDA 13, is chosen.
  A CUDA build contains only the program: it needs the CUDA runtime libraries on the computer (the toolkit, or the release's `cudart-`
  file), and CUDA 13 requires a driver of the 580 series. On a server where that is not the case, name the build with `--build` (a CUDA 12
  build, or the Vulkan build if the driver supports Vulkan) or install the libraries. Likewise, ComfyUI's installation uses PyTorch for CUDA 13
  on an NVIDIA computer (the document's choice, not this program's) and requires the same driver, which a configuration can have checked
  (`system = "nvidia_driver"`, section 12). The CUDA build of llama.cpp has not been run (the card used here is AMD's).
- **GitHub's rate limit.** The list of builds is requested from GitHub's API, which allows 60 unauthenticated requests per hour; `--build`
  with a label that is already installed makes no request, and `--github-token` (or `GITHUB_TOKEN`) raises the limit.

## 6. Configurations that carry no model: where the files come from (WP4.19.1, WP4.19.2)

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

A model file has one of three sources: a name the engine pulls (`ollama = "qwen3:8b"`), an entry in the recommended list (`catalog` and its
`file`), or a file (`file`, with the source it can be obtained from). The source keys apply to a file, not to the other two.
[docs/examples/profiles/wan22-ti2v-turbo.toml](examples/profiles/wan22-ti2v-turbo.toml) is the user's own Wan 2.2 TI2V 5B Turbo set (three files
from three repositories, one of them in a subfolder), with the commits, sizes and checksums read from the hub on 2026-10-12 and verified against the
files on the user's computer: `quickai-headless check` on it reports, against the live documents, what it would download, from where, and into which
folder (the weights into `diffusion_models`, the encoder into `text_encoders`, the VAE into `vae`). The folder is the one the engine's document
assigns to a file of that name, or `into`, which can only be a folder the document says the engine keeps files in (`models::folders`: ComfyUI's are
those in its document; llama.cpp keeps all its files in one).

[docs/examples/profiles/minimax-h3.toml](examples/profiles/minimax-h3.toml) is the equivalent for MiniMax H3, an open-weight video model, for a
48 GB NVIDIA card (an L40S) with 64 GB of memory ([minimax-h3-fl2va.toml](examples/profiles/minimax-h3-fl2va.toml) is its lean half: the build for
text and first-and-last-frame input, five of the files, 42 GB). It lists the seven files of the model's two builds (text with first and last frame;
reference images), all from `Comfy-Org/MiniMax-H3` at one commit: the weights in int8, the text encoder in 4 bits, the video and audio VAEs, and one
Lightning LoRA for each build, 65 GB in total, with the commit, size and SHA-256 of each read from the hub's response for the file at that commit.
These are the files that ComfyUI's own templates for the model load. It requires the models folder to be specified (`[[needs]]`, because 65 GB does
not belong on a system disk), pins nothing, and states in its header and its note what the license covers: the world except the EU, UK, South
Korea and USA, for using the model *and for displaying what it generates there*. A configuration holds at most 8 models, so each file notes in a
comment which alternatives it can take. Y70 reads both.

**Download order.** For each file that is not already in the models folder at the expected size, in this order: (1) a source the operator
specified for it (`--model-source`, `[models]`); (2) a folder they pointed to that contains it (`--models-from`); (3) the source the configuration
names, through the mirror if there is one (`--hf-endpoint`). A file's address is `{hub}/{owner}/{repo}/resolve/{revision}/{path}`, with the path
percent-encoded; the hub is the site that the engine's document specifies for Hugging Face searches (a user-layer document can point it to a mirror,
as can `--hf-endpoint`). An incoming file is verified as it is written (the hash is computed on its way to disk) and is discarded if it does not
match the configuration; nothing partial is left behind.

| To specify | Use |
|---|---|
| "this file is already on this computer" | `--model-source FILE=/path/to/file`, or a folder: `FILE=/path/to/folder` |
| "all my files are in this folder (a mounted disk, a copy of the repository's layout)" | `--models-from DIR`: searched by name, in the folder for its type, and by its path within its repository |
| "get it from here instead" | `--model-source FILE=https://...` or `FILE=huggingface:me/my-copy` |
| "my hub is a mirror" | `--hf-endpoint https://mirror.example` or `HF_ENDPOINT` |
| "the repository requires a login" | `--hf-token`, `HF_TOKEN`, or `[fetch] huggingface_token` of an environment file |
| "do not use the network" | `--offline`: the plan is reported, then what is missing, with where to put it and where it would come from |
| "I will download them myself" | `quickai-headless check wan.toml --json` provides each file's address and destination; download them (`aria2c`, `rsync`...), then use `--offline --models-from DIR` |
| "keep the models on the large disk" | `--models-dir DIR`, or `[values.<engine>] models_dir` of an environment file |

A file from outside that names a source is reviewed before anything is downloaded from it: `quickai-headless` stops with code 3, shows the full source
(the repository, the file within it and the revision, or the address) and how to approve it (`--accept-review`), and nothing is downloaded or
written. This question is not asked when the operator has stated where the file is, and a configuration they keep (imported, and run by its ID)
does not count as being from outside.

## 7. Computer-specific settings: the environment (WP4.19.3)

A configuration is meant to be shared, so it carries nothing specific to a computer or a person (configurations.md, rule 2). Whoever runs it
supplies those details:

```toml
# vision-chat.environment.toml -- `quickai-headless init vision-chat.toml` writes it, for the configuration it was made for
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

It is read with `--environment FILE` (more than one may be given; a later one takes precedence). `--set ENGINE.FIELD=VALUE` sets one setting (the
text is parsed as the setting's type: a number, true or false, one of the options, or a path), `--env [PROGRAM:]NAME=VALUE` sets one variable in a
program's environment (PROGRAM is the engine unless a companion program is named), and `--model-source FILE=WHERE` sets one source for a file.
Invalid input is reported with code 2, naming the file and key or the flag where it occurred (`values.comfyui.port: more than 65535...`,
`--set llamacpp.port=eighty`), all at once; a key this version does not recognize produces a warning, and the rest is used. An empty value is not
applied (so a template can be returned blank, and anything required in it is still required). What was specified is reported, with where it was
specified, and secrets are shown as `(hidden)`:

```
What you said about this computer:
  fetch.huggingface_token = (hidden)  (the variable HF_TOKEN)
  values.comfyui.models_dir = /mnt/big/models  (environment file /home/you/wan.environment.toml)
  env.openwebui.WEBUI_SECRET_KEY = (hidden)  (--env)
```

**Precedence**, from lowest to highest: the engine's document, the configuration, environment files (later over earlier), `--set` and `--env` (in
the order given), then `--port` and `--models-dir`; for logins, a file, then the variable, then the flag. An environment line (`--env`,
`[env.<program>]`) is added after the lines in the configuration's own `extra_env`; a setting (`--set`, `[values.<engine>]`) replaces the configured
value. The names an engine's document protects (`OLLAMA_HOST`, `DATA_DIR`, `OLLAMA_BASE_URL`, `HOME`...) cannot be changed by such a line, just as
the user cannot change them in the window. The set of names that no configuration may set (`LD_*`, `PATH`...) applies to what comes from a
configuration; an operator who writes `LD_PRELOAD` in their own file or flag is trusted, as in the window's own field.

**What the configuration requires.** `[[needs]]` in a configuration states what whoever runs it must supply:

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

Until each item is supplied, `check`, `prepare` and `up` stop with code 5 and one sentence per item (`a Hugging Face token`, `comfyui.models_dir`,
`WEBUI_SECRET_KEY (for openwebui)`), stating why it is required and how to supply it (`--hf-token, the variable HF_TOKEN, or `huggingface_token` under [fetch] of an environment file`); nothing is downloaded or installed in the meantime, and the message ends with `quickai-headless init <configuration>`.
A setting the configuration itself provides counts as supplied. The window never reads `[[needs]]`, but preserves them when it saves the
configuration.

## 8. Companion programs (WP4.19.4)

Open WebUI alongside Ollama is the only companion program the application ships. A configuration includes it in the run through
`[values.openwebui]` (`enabled = true`, its `port`, and its internet access, `network`) and specifies its build in `[runtime.openwebui] build`:

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

`quickai-headless` installs both (Ollama from its release, and Open WebUI from the package index into a Python environment of its own under
`~/.quickai/openwebui`), pulls the model, and starts Open WebUI after Ollama, telling it where Ollama is (`OLLAMA_BASE_URL`), with its data and home
directory in its own folder, internet access as the configuration specifies (offline unless it says `online`, which a file from outside must be
approved to say), and the environment variables the operator supplied (`--env openwebui:WEBUI_NAME=Studio`, `[env.openwebui]`). The variables that
Open WebUI stores in its database after its first start, and which a configuration therefore cannot reliably set (configurations.md, section 9),
are for the operator to supply here, at run time. It reports both addresses, and stops Open WebUI before Ollama. `--build openwebui=LABEL` names the
build on the command line and takes precedence over the pin; a label the index does not have is reported, with the available ones, before anything
is written. A pin for a program that is not a companion of the engine is not valid (it is reported as an unrecognized key), and a pin for a
companion that is not enabled has no effect. `docs/examples/profiles/chat-with-open-webui.toml` is a complete example, with the key that signs the
logins declared as a `[[needs]]`.

What a configuration does **not** contain: ComfyUI custom nodes, or any other program that a configuration would have a server install and run
(C133). The nodes ComfyUI needs for GGUF files are in its document and are installed with it; any others require a user document of the operator's
own, which asks by hash before it starts anything.

## 9. Tests (WP4.19)

Group Y of [phase3-test-plan.md](phase3-test-plan.md) covers: the file (Y01 to Y12: the source keys and their restrictions, `[[needs]]`, the pins,
what an export records about a file's location within a repository and what the window's Save preserves, and that a build pinned in an example is
named the way the list of builds names it: Y68); the environment file and the template (Y20 to Y27: the same code reads, layers, displays without
secrets, applies and writes the template, plus a source scan confirming that no engine is named); the plan against the fakes (Y30 to Y51: the files
of a set in their folders, incoming files verified against what is declared, the review, the operator's own files and folders, offline mode, the
logins and where they are sent, the mirror, the environment reaching the program, required items, secrets, and pins); and the binary (Y60 to Y67).
The program is also run end to end against fakes (including a hub that records who made each request) by the GUI test suite, which the maintainers
keep outside this repository.

**Mutation testing.** 31 parts of the code were deliberately broken, one at a time, and a test was observed to fail for 29 of them. Three were not
caught at first, and each is now covered by a test: a blank Hugging Face login counted as supplied (Y20), a login sent to every host (Y37c), and an
overwritten file keeping its previous mode (Y63). The remaining two have no observable effect: a login embedded in an address is rejected by three
other checks besides the one that was broken, and the mode a file is first created with is replaced by the 0600 it is given immediately afterward.
15 of the parts were also broken in the program the GUI tests run, and each caused a case there to fail. The list of parts is in the run log of the
[implementation plan](implementation-plan.md); the script that produced them is not part of the repository.

## 10. Limitations and untested areas

- **Windows.** It builds for Windows, and running, stopping and starting in the background are implemented for it (section 13), but it
  has not been run on Windows: that requires a run on a Windows computer, as A8 does.
- **A real NVIDIA card.** On a machine where an NVIDIA card is detected, the installation chooses PyTorch for CUDA 13, based on the engine's
  document, and llama.cpp's CUDA build (section 5). That path was written from the document and tested against the fakes; it was developed on
  an AMD card. ComfyUI's installation was run on a real card (an L40S) on 2026-10-13 (section 11); llama.cpp's CUDA build has not been run on
  one.
- **A real ComfyUI installation through this program.** The installation is the window's own primitive (`install.python_app@1`, run for real on
  2026-10-08), and the tests here run it through the program against a fake GitHub and a fake Python; the real installation takes about 20 minutes
  and 17 GB with PyTorch for AMD, which a test run does not do. Through this program, it was run for real on 2026-10-13 with PyTorch for CUDA 13
  (about 5 minutes and 7 GB; section 11).
- **A gated repository.** The fake hub responds with 401 and 403 as the real one does; no real gated repository was used (no login was available).
- **Multiple models in the window.** The window shows one model; it keeps the others (with each one's source) as long as that model remains first,
  and drops them when another model is chosen. A configuration with several files is intended for `quickai-headless`.
- **Overlays**, the live-traffic proxy (which serves the window's Live tab), and a second program per engine other than the one the
  document names.
- **Authentication.** There is none to configure: it listens on the loopback interface.

## 11. What has been run for real

On Linux with an AMD card, in a separate home directory, on 2026-10-12:

- **The plan** (`check`) for three configurations against the live GitHub and PyPI feeds: llama.cpp with a 219 MB model; Ollama with Open
  WebUI alongside it (`install Ollama v0.35.1 ... install Open WebUI 0.11.4 ... pull ... load ... start Open WebUI`); and ComfyUI with the LTX
  video model (`install ComfyUI v0.39.0 ... PyTorch for AMD GPUs (ROCm 7.2) ... it needs 30GiB free`, `fetch ... and 2 more files (6.6 GB)`).
  The home directory was left as it was found.
- **llama.cpp b11564 and `LFM2.5-350M-Q4_K_M.gguf`, through `up`**: the release's plain build was installed (16.9 MB), the model downloaded
  (218.7 MB, with progress in tenths), the real `llama-server` started, and a question over `/v1/chat/completions` answered; a SIGTERM ended it,
  together with its server, in under a second, with exit 0, the ready file removed and the port free. A second run only started it.
  The first of these runs revealed a defect (it installed an OpenVINO build; section 3, rule 5), which is why rule 5 exists.
- **The packaged binary on a bare Debian 12** (no `lspci`, no Python, no OpenSSL or OpenMP): it identified the computer's AMD card from the
  kernel's device list, chose the Vulkan build, and, where the build needed a library the image lacks, stopped with the loader's own message
  naming it. With `libgomp1` and `libssl3` installed, and `quickai-headless` running as PID 1, the plain build of the newest release was installed,
  the model loaded, a question answered, and a stop ended it in 0.2 s with exit 0.
- **The complete GUI test suite** on the split application: everything passed except the two long-standing skips. Its first full run found that
  the first version of card detection bypassed a stand-in `lspci` that the suite places first on the path (section 3 and C120).

WP4.19, the same day:

- **The real hub's responses for the three files of a Wan 2.2 set** (headers only; nothing downloaded): the address
  `{hub}/{repo}/resolve/{commit}/{path}`, with a subfolder in the path, responds with `302` to a signed address on another host, with the size and
  SHA-256 of the file (L26). The sizes and the three checksums are in the worked example, and the checksums **matched those of the three files
  downloaded earlier** (`sha256sum` of 13 GB: 74 s).
- **`quickai-headless check docs/examples/profiles/wan22-ti2v-turbo.toml --gpu amd --accept-review`** in a separate home directory, against the live
  documents: install ComfyUI v0.39.0 (PyTorch for AMD GPUs), and for each of the three files, the address on the real hub and the destination
  folder, which matched the folders ComfyUI keeps them in.
- **`quickai-headless prepare` of the same file with `--models-from ~/.quickai/comfyui/models --set comfyui.bin_override=<a stub>`** in a separate home
  directory: the three files were verified against their SHA-256 (13 GB, with progress in tenths; slow in a debug build, which hashes without
  hardware acceleration), linked rather than copied into the folders of the separate home directory, and the program reported
  `Everything it needs is in place; nothing was started`. ComfyUI (v0.39.0) lists a linked file and opens it (L27).
- **`quickai-headless up` of a configuration that carries no model, against the real hub**: a 1.1 MB public story model (`ggml-org/models-moved`,
  `tinyllamas/stories260K.gguf`, pinned to a commit, with its `size` and `sha256` in the configuration) and an environment file that sets
  `models_dir` and `extra_args = "--threads 2"`. The plan named the file's exact address on the hub and its destination, and indicated that those
  two settings came from the environment file; the file was downloaded and its SHA-256 matched the declared one; the real `llama-server` started
  with `--threads 2` and that folder, and answered a question over `/completion`; a single SIGTERM by PID ended it (`Stopping...`, `Stopped.`,
  exit 0, the ready file removed, the port free, no process left); running it again downloaded nothing. The program used was the development
  tree's own build of llama.cpp (the document's search finds it before installing one), so this run did not install a release; that is covered by
  the llama.cpp b11564 run above. Two signals sent at once by mistake (a `pkill` that matched this program, and the `timeout` that wrapped it)
  ended it immediately with 130, as section 5 describes, and the server exited with it.
- **Ollama with Open WebUI alongside it, through `up`, in a separate home directory on separate ports** (21434 and 23000, to avoid the user's own
  Ollama), using the structure of `chat-with-open-webui.toml` and a 271 MB model (`smollm2:135m`). `check` without the key that signs the logins
  stopped with code 5 and stated what was required, why, and how to supply it; `init` wrote the file (mode 600), the key was filled in by hand,
  and `check` then printed the sequence: Ollama v0.35.1 (1.3 GB, verified against the document's SHA-256), Open WebUI 0.11.4 (the index's file
  verified against the pin, with its dependencies, into an environment of its own), start Ollama, pull, load, start Open WebUI. `up` did all of
  it: Ollama 2.2 GB, Open WebUI 3.0 GB, the model 259 MB, 5.2 GB in total, with no package cache outside the folder. Open WebUI's process had
  `OLLAMA_BASE_URL=http://127.0.0.1:21434`, `OFFLINE_MODE=true`, its own home and data folders, and the key from the file (read from the
  process's environment; the key appeared in nothing the program printed or wrote); Ollama listed the model and answered a question; Open WebUI
  answered `GET /` with 200 and reported version 0.11.4; a single SIGTERM stopped Open WebUI and then Ollama in 0.4 s, the ready file was
  removed, both ports were freed, and nothing was left running. The user's own `~/.quickai`, `~/.ollama` and `~/.cache/pip` were not touched.
  Two findings came out of this run. First, the pins in the two examples that pin Ollama's build (`v0.35.1`, and `v0.40.0` in the older
  `vision-chat.toml`) were not names in the list of builds (builds are named by release and file, `v0.35.1-ollama-linux-amd64`; the examples and
  section 8 now say so, Y68 holds every pin in the examples to that form, and the program's message for an unknown name already lists the valid
  names). Second, on a computer whose Python 3.12 is available only behind a `pyenv` shim, the first run stopped, reporting that no Python 3.12
  or 3.11 was found and what to install, and `--python` named the interpreter for the second run.

- **The `quickai-headless` package (4.5 MB) on a bare Debian 12**, including this work: `init` printed the template for the Wan 2.2 set; `check`
  against the live feeds named the three hub addresses and the folders under `/root/.quickai`; `up --offline` with nothing installed ended with
  code 1 and said to install first; and `prepare --models-from` the user's own models folder (mounted read-only) with
  `--set comfyui.bin_override=/bin/true` verified the three files (13 GB) against their SHA-256, linked rather than copied them, and reported
  `Everything it needs is in place; nothing was started`. The offline message contained two spaces where a build's name had been omitted; this
  has been fixed.

WP4.19, 2026-10-13, the MiniMax H3 example:

- **`quickai-headless check docs/examples/profiles/minimax-h3.toml --gpu nvidia --accept-review --set comfyui.models_dir=/mnt/big/models`** (the
  packaged binary, a separate home directory, the live documents): `install ComfyUI v0.39.0 ... PyTorch for NVIDIA GPUs (CUDA 13) ... it needs 30GiB free`,
  the seven downloads with the address, folder and size of each, and `start ComfyUI on 127.0.0.1:8188`; without the folder, it stopped with code 5
  and explained why. The lean `minimax-h3-fl2va.toml` gave the same result with five files.
- **The hub's response for each of the seven addresses at the pinned commit** (`HEAD`, nothing downloaded): `302`, with `x-linked-etag` equal to the
  SHA-256 in the file, `x-linked-size` equal to its size, and `x-repo-commit` equal to the commit. The same holds for the four optional files
  offered in the header's comment; with all four added to the seven, the file is rejected (more than 8 models), and with three or one substituted
  for others, it is read.
- **`quickai-headless prepare` of a configuration with one of the files (the audio VAE, 577 MB) and a stand-in for ComfyUI**: downloaded through the
  hub's redirect in 1 minute 30 seconds, with the size matching and the file's `sha256sum` equal to the one in the configuration.
- **`quickai-headless prepare` and `up --offline` of `minimax-h3.toml` on an NVIDIA computer** (Ubuntu 24.04 and an NVIDIA L40S with driver
  595.58.03, which is CUDA 13.2; the package's binary unmodified, `HOME` set to a folder of its own, `--gpu nvidia --accept-review`, and the models
  and output folders set beneath that folder): **the first run of this program on an NVIDIA computer.** After `apt install python3.12-venv`
  (Ubuntu's Python cannot create an environment without it, as the README notes), ComfyUI v0.39.0 was installed with PyTorch 2.14.1 for CUDA 13
  (about 7 GB) and the seven files were downloaded (65 GB), taking 13 minutes 5 seconds and 67.3 GiB of disk space in total. The size and SHA-256
  of every file matched the configuration (recomputed afterward from the files: 66 seconds), and PyTorch in the environment ran a matrix
  multiplication on the card. `up --offline --ready-file` had ComfyUI responding after 6 to 7 seconds; ComfyUI listed the seven files in
  `diffusion_models`, `text_encoders`, `vae` and `loras` and exposed both of the model's nodes (`MiniMaxH3ImageToVideo` and
  `MiniMaxH3ReferenceToVideo`, from `/object_info`); a SIGTERM ended it in 0.24 seconds, with the card's memory at 0 MiB and the port free, and it
  was started again.
- **Not run:** the model itself (no clip has been generated, so its time and memory requirements on the card and the computer are not measured
  here), and llama.cpp's CUDA build on a real card. A driver of the 580 series was not checked by anything in this program at the time; since
  WP4.20, a configuration can have it checked (section 12).

WP4.20, 2026-10-13, system prerequisites, on a computer with an AMD card (a separate home directory each time, with `--gpu nvidia` to name the card
the documents refer to):

- **`quickai-headless check` of the five video-model configurations** (`minimax-h3.toml`, `minimax-h3-fl2va.toml`, `fasth3-v2.toml`,
  `wan22-14b-i2v-lightning.toml`, and `ltx-2.5-distilled.toml` with a stand-in login): exit 6 for each, because that computer has no NVIDIA
  driver, with the check printed before the plan (the driver `MISSING`, the compiler `ok: gcc at /usr/bin/gcc`, the headers `ok` in
  `/usr/include/python3.14/Python.h`, the test `later`; the Wan 2.2 set requires only the driver), the plan after it, and the separate home
  directory left empty.
- **The test script, run for real** in a ComfyUI environment (Python 3.13.7 from `pyenv`, PyTorch for ROCm, Triton 3.8.0, a separate home
  directory and temporary folder): it built Triton's helper with `/usr/bin/gcc` against that Python's headers, launched a kernel on the AMD card
  (`hip_utils.c` is the helper of Triton's AMD backend, as `cuda_utils` is of its NVIDIA backend), obtained the correct result and removed its
  cache, in **4.2 seconds**. Then **the failure that prompted this work was reproduced for real**: the same interpreter copied without its
  `include/` folder (like the Python of a server lacking `python3.12-dev`) makes gcc report
  `hip_utils.c:5:10: fatal error: Python.h: No such file or directory` and Triton end with
  `CalledProcessError: Command '[...]' returned non-zero exit status 1`, which was the only line in the first job's error message; the script
  exits with status 1, and both are the last lines it writes.
- **`quickai-headless prepare` through the real binary with that Python**, with a stand-in `nvidia-smi` reporting 595.58.03 and a stand-in build folder
  (a launcher that does nothing, with the Python beside it): with the headers required, it stopped with 6 and
  `Python.h is not in .../py-noheaders/include/python3.13, where Python 3.13 keeps its headers`, offering no package (that Python is not the
  system's); with only the test required, it ran the test in 2.7 seconds, ended with 6 and printed
  `| ... fatal error: Python.h: No such file or directory` under `the environment of ComfyUI failed a test`, with the configuration's explanation
  and the document's hint, leaving the engine's temporary folder empty afterward; with the Python that includes its headers, it ran the test in
  4.0 seconds, reported `ok      the environment passed` and `Everything it needs is in place; nothing was started.`, and exited with code 0.
- **The real apt, on a fresh Ubuntu 26.04** (as root, with only `python3` and `gcc` installed, matching the state in which the NVIDIA computer was
  found: a Python without its venv module and headers, and a compiler; the packages are `python3.14-*` there because that release's Python is
  3.14, while on 24.04 they are `python3.12-*`): `check` of a configuration requiring the venv module, a compiler and the headers reported
  `MISSING` for the two Python components in the real Python's own words (`ModuleNotFoundError: No module named 'ensurepip'`,
  `Python.h is not in /usr/include/python3.14`), `ok` for gcc, the command
  `apt-get update && apt-get install -y python3.14-venv python3.14-dev`, and exit 6. `prepare --install-system-packages` then ran exactly that
  command (with the package manager's output passed through: 10 new packages, 16.5 MB), checked again and found all three `ok`
  (`Python 3.14 imports ensurepip`, `/usr/include/python3.14/Python.h`), and proceeded to install ComfyUI: **the step that creates the Python
  environment, which is the one that fails without the venv package, succeeded**, and PyTorch for the processor and the requirements were being
  downloaded when the run was stopped at 200 s (step 5 of 7). Afterward, `import ensurepip` works and gcc builds a shared object against
  `Python.h`.
- **Type-checked for `x86_64-pc-windows-gnu` and `aarch64-apple-darwin`** (not linked): the core crate, and the real `host.rs` in a scratch crate
  that stubs the few `runtime::process` functions it calls (the Unix-only parts are behind `cfg(unix)`; on Windows no package manager is found and
  `which` finds no `.exe`, which no document in this version requires).
- **Not run for real on the AMD computer:** installing packages on that computer itself (only on the fresh Ubuntu above, and against stand-ins for
  the package manager and `sudo` in the tests: Y77, Y87 to Y90, Y98); an NVIDIA card; and the test against an NVIDIA driver and `libcuda`. These
  were run later the same day on the NVIDIA computer, as described below.

WP4.20, 2026-10-13, later the same day, on the NVIDIA computer from the WP4.19 run above (the L40S with driver 595.58.03; the `quickai-headless` package
from this work, a separate `HOME`, `--gpu nvidia --accept-review`, and the models and output folders set beneath it). The earlier installation
(ComfyUI with its environment, 6.8 GiB, and the models, 60.5 GiB) was removed first, so that the program encountered the computer in the state the
first job had found it: with the compiler and Python's venv module, and without Python's headers:

- **`quickai-headless check` of `minimax-h3.toml`**: exit 6, with the check printed before the plan: the venv module `ok`
  (`Python 3.12 imports ensurepip`), the driver `ok` (`driver 595.58.03`), the compiler `ok` (`gcc at /usr/bin/gcc`), and **only the Python headers
  `MISSING`** (`Python.h is not in /usr/include/python3.12, where Python 3.12 keeps its headers`), with the document's hint and the command
  `apt-get update && apt-get install -y python3.12-dev`; the test `later`. That is the failure that prompted this work, reported before any file
  was downloaded.
- **`quickai-headless prepare --install-system-packages`**, run as the administrator: it ran that command itself (four new packages, `python3.12-dev`,
  `libpython3.12-dev`, `libexpat1-dev` and `zlib1g-dev`; none upgraded or removed), checked again (all four `ok`,
  `/usr/include/python3.12/Python.h`), installed ComfyUI v0.39.0 with PyTorch 2.14.1 for CUDA 13, **ran the environment test on the card after
  the installation and before the first of the seven files was downloaded** (`ok the environment passed`), downloaded the files and ended with
  `Everything it needs is in place; nothing was started.` and code 0: 11 minutes 7 seconds and 67.3 GiB in total (13 minutes 5 seconds the first
  time).
- **Afterward**: the size and SHA-256 of every file matched the configuration, PyTorch in the environment ran a matrix multiplication on the card,
  `up --offline` had ComfyUI responding after 9 seconds (the environment test runs again at every start; section 12) with the seven files and both
  of the model's nodes listed, and the test script run by hand took 2.0 seconds and left nothing behind (F30 of
  [engine-facts.md](engine-facts.md)).
- **Not run:** on the NVIDIA computer, a failing test, a driver that is too old, and `prepare` without `--install-system-packages` (`check`
  showed the stop and its command); only the AMD computer, the fresh Ubuntu and the stand-ins have exercised those. A package manager locked by
  another process was not encountered on the server (its lock was free; the fresh Ubuntu below encountered one, with the version of the program
  used there). No clip has been generated, and llama.cpp's CUDA build has not been run on a real card.

WP4.20, 2026-10-13, after the run on the server: apt and a lock held by another process. Both runs were on a fresh Ubuntu 26.04 (apt 3.2, root,
only `python3` installed), with another Python process holding `/var/lib/dpkg/lock-frontend` for 30 seconds (an `fcntl` lock, like apt's), using
the program's own binary:

- **The program as used on the NVIDIA computer** (without the wait), `prepare --install-system-packages` of a configuration requiring the Python
  headers: it ran `apt-get update`, then `apt-get install -y python3.14-dev`, which ended after a second with
  `E: Could not get lock /var/lib/dpkg/lock-frontend. It is held by process N (python3)`; the program reported
  `installing the packages failed: ... ended with exit status: 100`, ended with code 6, and had installed nothing. That is what an installation
  does on a server that is running its own upgrades.
- **The program with the wait** (the same lock; a configuration requiring the headers and the venv module): it reported
  `running: apt-get install -y python3.14-dev python3.14-venv` (the plain command); apt's `Waiting for cache lock: Could not
  get lock /var/lib/dpkg/lock-frontend. It is held by process N (python3)...` followed once a second for 28 seconds and was passed through, and
  when the other process released the lock, the 14 new packages (20 MB) were installed, the check was repeated and found both `ok`, and ComfyUI's
  installation continued through PyTorch to its requirements (step 5 of 7) until the run was stopped at 170 seconds. How apt behaves with the
  option, with a lock held longer than the wait, and with the lists lock is described in F32 of [engine-facts.md](engine-facts.md), measured with
  apt 2.8 on Ubuntu 24.04.
- **Not run:** the NVIDIA computer with the wait (the program used there predates it), a lock held by a real `unattended-upgrade`, and dnf,
  zypper, pacman and apk with a held lock.

What these runs did not cover is listed in section 10.

## 12. What the computer must have (WP4.20)

On 2026-10-13, the first job sent to the ComfyUI instance on a rented GPU server (a clip with a reference image, MiniMax H3, int8 weights) failed
after 3.4 seconds, and the message that reached the program that sent it said only that `gcc` had "returned non-zero exit status 1". The cause
appeared only in the server's own log: Triton, which the PyTorch build for a graphics card brings with it, builds a small helper in C the first
time a kernel is launched, and the server had a compiler but no `Python.h` (the package `python3.12-dev`). This program had set up that server,
and knew nothing about it. The user's analysis of who is responsible (in their own words; the part about the other program is omitted):

> QuickAI - should ensure that the environment is setup correctly: [...] The server's environment belongs to QuickAI, because it provisions the server and
> the H3 profile's int8 weights are what create the need for Triton. The H3 profile should declare its system prerequisites: a C compiler, the Python
> headers (here python3.12-dev) and an NVIDIA driver of the 580 series or newer. quickai-headless up should check them, then install them or stop with the exact
> command. It should also run a small self-test before saying "ready", for example compiling Triton's helper. Alternatively, it could build ComfyUI's
> environment on a self-contained Python that ships its own headers.

That is what this section implements. (The alternative is not implemented: a self-contained Python would bring its own headers and so remove one
of the three requirements, but it would still need the compiler and the driver, and it would be a Python that this program has to download, pin,
verify and maintain, which is a separate piece of work.)

### The engine document defines how; the configuration declares which

The engine's document names what can be checked, how, and what provides it (decisions C134 to C136). The ComfyUI document names five
prerequisites:

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

A configuration states which of them it needs and why, in its own words, in `[[needs]]`, alongside the types it already supported (a login, a
setting, a variable):

```toml
[[needs]]
system = "c_compiler"
why = "The int8 and 4-bit files run through Triton, which builds a small helper in C the first time a clip is made."
```

The five video-model examples declare them: the venv module and the driver in every one, and the compiler, the headers and the test in the four
whose int8 and 4-bit files run through Triton (MiniMax H3 in both builds, FastH3 and LTX-2.5), but not in the fp8 Wan 2.2 set, which no known
workflow runs through a Triton kernel. A configuration that asks for nothing is checked for nothing; the window does not show any of this yet.

### Sequence

1. **The check**, after the plan is made and before it is printed (and therefore before anything is downloaded): `program` finds a name on the
   `PATH` (or the program the variable names); `python_module` asks the Python the installation will use whether it can import a module
   (`ensurepip`: Debian's Python cannot without `python3.12-venv`; an environment that has already been created is not checked);
   `python_header` asks the environment's Python for its header folder (the environment beside an installed launcher; for a launcher not yet
   installed, the Python the installation will choose, by the same rule) and looks for the file there; and `nvidia_driver` reads the version from
   `nvidia-smi` or from `/proc/driver/nvidia/version` and compares it number by number. The findings are printed (`ok`, `MISSING`, `unknown`
   when the check could not be made, `later` for the test), with the configuration's explanation and the document's hint under each missing
   item, and the command that installs whatever a package can provide:

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

2. **Anything missing stops the run** with code 6 and a message that lists it, the command, and anything no package provides. `check` does this
   too, after the plan, and changes nothing. With `--install-system-packages` (C138), the packages are installed instead (as the administrator,
   or through `sudo -n`; the package lists first, each package once; every line passed through; apt's install told to wait up to two minutes for
   a lock held by another process, with apt's own `Waiting for cache lock` lines, one per second, among the lines passed through). The computer
   is then checked and reported again, and the run continues if nothing is missing. A package manager that fails, `--offline`, the absence of
   password-less `sudo`, a package manager for which the document lists no package, and an item that no package provides each stop the run with
   an explanation. Python's own packages are used only when that Python is the system's (`/usr`): for a Python from `pyenv` or `uv`, the headers
   come with it, and a file missing from its folder cannot be fixed by a system package.
3. **The installed environment test** (C139) runs after the installation and before the first model file is downloaded (or before the start,
   when there is nothing to download). ComfyUI's test writes a tiny Triton kernel, builds it from an empty cache and runs it on the card
   (`assets/comfyui/selftest/triton_kernel.py`), and exits with a status that identifies the step that failed: PyTorch cannot be imported (2),
   PyTorch cannot use a card (3, with PyTorch's own reason, such as a driver that is too old), Triton cannot be imported (4), the kernel could not
   be built or run (1, with the compiler's own output before the last line, which is what the first job's message lacked), or a wrong result (5).
   It takes a few seconds and runs on every `up` and `prepare` of a configuration that asks for it; `check` lists it as `later`. A failure stops
   the run before the models are downloaded and before ComfyUI is started.
4. `--skip-prerequisites` (C140) skips both the check and the test.

### Limitations

- **The window shows none of this.** A missing compiler is not a note in the window's review of a configuration, and the window starts ComfyUI as
  it always has. The pure logic is in the core (`quickai_core::prerequisite`), so the window could use it.
- **Only the ComfyUI document names prerequisites.** llama.cpp's CUDA build (which also requires a driver of the 580 series for CUDA 13; section 5)
  and the other engines declare none yet; the mechanism is the same for them, through a document of their own.
- **A driver is never installed, and a Python is never provided**: when the computer has none of the versions the installation requires, the
  installation says so and no package is named for "a Python". The venv module of the existing Python is a prerequisite (`python_venv`) that can
  be checked and installed for a configuration that asks for it; a configuration that does not ask encounters its absence during installation,
  with the installer's own message (covered by a GUI case).
- **Only apt's install waits for a lock.** Refreshing apt's lists takes a different lock, the lists lock, which `-o DPkg::Lock::Timeout` does not
  cover (F32): with that lock held (the system's own refresh running), the program's refresh ends immediately and the installation continues with
  the existing lists, as after any failed refresh. dnf, zypper, pacman and apk are run exactly as printed; their behavior with a held lock has not
  been examined. A stop requested while apt is waiting is acted on when the wait ends (two minutes at most), because a package manager is not
  interrupted midway.
- **A configuration that asks for nothing is checked for nothing**: a ComfyUI instance on an NVIDIA card whose configuration does not declare
  `system = ...` encounters Triton's compiler problem at the first kernel, as before.
- **Run on an NVIDIA computer once, and only on the successful path**: the tests (Y71 to Y99d) use stand-ins for the compiler, the package manager,
  `nvidia-smi` and Python; the test script was run for real on an AMD card, and the whole sequence on an NVIDIA L40S, where one package was
  missing, the program installed it, and the test passed (section 11). The other stops (a driver that is too old, a missing compiler, a failing
  test) have been observed only on the AMD computer, on a fresh Ubuntu and against the stand-ins.

Tests: group Y of [phase3-test-plan.md](phase3-test-plan.md), Y71 to Y99d (checking, installing and running a script: `runtime/host.rs`; the
documents and the configuration: `core/tests/p3_prerequisites.rs`; the whole sequence through `provision::up` against a simulated computer:
`runtime/p3/prerequisites.rs`; the messages and the program: `run`), and 18 cases of the GUI test suite, kept outside this repository (the check,
the command, installation through `sudo`, a driver no package provides, the test failing as a real one does without `Python.h`, and failures when
installing what Python needs: a package manager that cannot find the headers package, one that installs the headers of another Python, a Python
without its venv module, pip with no matching distribution, a Python that cannot be run, and an environment that has already been created).

**Mutation testing.** 31 parts of the Rust code were deliberately broken, one at a time (a condition ignored, a flag read the wrong way, the test
run after the download, the apt list refresh made fatal, `sudo` omitted from the printed command, the environment not passed to the script...),
and a test was observed to fail for each. The one not caught at first was a second guard for the same thing (the user is not asked for what the
computer is checked for), which is now a single, tested guard. 6 of them were also broken in the program the GUI tests run, and each caused a case
there to fail. Seven more were made for the lock wait (Y78c: the option never added, also given to the refresh, printed for the user, run from the
printed text, a wrong number of seconds, another package manager run with different arguments than printed, the option placed before the
subcommand), each observed to fail a test, and the first was also tested in the program the GUI tests run. The list is in the run log of the
[implementation plan](implementation-plan.md); the script that produced them is not part of the repository.

## 13. A run that lasts until it is stopped (WP4.33.2)

For a run that another computer requests over SSH ([remote-runs.md](remote-runs.md), section 3.1), and equally useful on a server by hand:

```sh
quickai-headless up wan.toml --detach   # start it in the background; it goes on when the terminal or the connection closes
quickai-headless status [--json]        # what runs (the configuration, since when, where it listens), or how the last run ended
quickai-headless down [--no-wait]       # stop it, in the background or in another terminal, and wait until it has
```

- **One run at a time** per computer and user: `up` and `prepare` hold the run folder (`~/.quickai/run`, the data folder's `run`) with a lock that
  the system releases when the run ends, however it ends; a second run reports what is running and exits with 7. `check` changes nothing and
  holds nothing.
- **The current run is described in `run.json`** beside the lock (the process, the configuration, the version, the start time, `starting`,
  `preparing`, `ready` or `stopping`, and, once ready, where each component listens); **how the last run ended** is in `last.json`; **what a
  background run reported**, one JSON event per line, is in `events.jsonl` (with its error stream in `stderr.log`).
  `status --json` is `quickai_runtime::run_file::Status`: the window reads it over SSH.
- **`down` writes a `stop` file** that the run checks every second: stopping works the same way on Linux, macOS and Windows, where there is no
  SIGTERM to send to a program in another console. Ctrl-C and SIGTERM still stop a run in a terminal; a hang-up stops it only when it is not
  running in the background.
- **`free PORT... [--json]`** stops the programs listening on those ports (for a window that was told to override them;
  [remote-runs.md](remote-runs.md) section 3.2): a user's program is asked to exit, then terminated; a system service is stopped through its
  manager (`systemctl` on Linux, `launchctl` on macOS) via `sudo -n` when that requires no password, and otherwise the command that stops it is
  reported; on Windows, `taskkill` is used. It exits with 8 when a port remains in use. `check` reports the ports a run would listen on and whether
  another program currently holds one (the `ports` event, including what responds there when it responds as the engine does).
- **`--detach`** restarts the program with `--detached --json` and returns once the run has made its plan and moved on (or has ended, with its
  code and the reason, or after a minute). On Linux and macOS, the run is in its own session; on Windows, it has no console and its own process
  group, and is started outside the job object that Windows' OpenSSH server closes along with the connection (retried without that when the job
  does not allow it; the Task Scheduler is the remaining option if neither works, to be determined on a Windows server).
