# Remote runs: QuickAI runs a configuration on another computer itself

_Status: designed 2026-10-28, its questions answered the same day (section 7), not started (WP4.33, C186). It revamps the remote computers of WP4.27 and WP4.32, which
attach to an engine the person already runs there; that way stays, as one module of several._

## 1. What was asked

The user, after hearing that llama.cpp cannot run on another computer because QuickAI only attaches to an engine that is
already running there:

> i think its better to run quickai headless itself it in the other server by clonong yourself or something and the
> configuration i give

and then, as the design:

> For the remote runs, the idea would be to bunde the linux/mac/windows version by default with all the quick ai
> releases. when the time comes - i.e. to run a specific configiruation in a remote machine, identify the machine and
> copy the specific headless executable, copy over the configuration. Install any missing runners(e.g. lamacpp, ollama
> etc - for that platform - in case it doesnt exist, an alternate can be suggested).
>
> This will be a revamp.
>
> Essentially the idea is to run a given configuration - quick ai resolves the environment - installs any missing
> package etc - later runt he config. It will provide alternatives that could be accepted or rejected by the user.
>
> Let us park the routing mode - it could also be helpful later on - make things modular as you implment - such that we
> can switch off or remove a specific module if needed.

and, of what comes after:

> AFter the run on the remote, the idea is to tunnel the ports for usage locally -that is all

## 2. In short

A run is **a configuration and a computer**. For a computer that is not this one, QuickAI:

1. **identifies it** over SSH: its system and processor, its home folder, its free space, and whether the `quickai-run`
   of this release is already there;
2. **copies over** the `quickai-run` for that system, which every release carries (section 4), and the configuration;
3. **resolves the environment** there: what the configuration needs (the engine's program, packages, the models) and
   what that computer lacks;
4. **offers alternatives** where something cannot be had for that computer (another build, another engine, a smaller
   file), each **accepted or rejected** by the person before anything is installed;
5. **installs** what is missing and **runs** the configuration there, through `quickai-run`;
6. **tunnels its ports** to this computer, and that is all: what the configuration serves there (the engine's API, a
   web page, a companion such as Open WebUI) is used here as if it ran here, by any app, the browser, or the window's
   own chat as one more client. The window does not manage the engine there (no listing, pulling or loading through
   it): the configuration says what runs.

Steps 3 to 5 are the same for this computer: "run this configuration" resolves, offers, installs and runs wherever it
runs. The other computer only adds the transport (1, 2 and 6). Nothing is opened to the network on either side, as
today (C171).

What is already there: `quickai-run` (`init`, `check`, `prepare`, `up`, with `--json` events and `--ready-file`;
[headless.md](headless.md)), the configurations it reads ([configurations.md](configurations.md)), what a configuration
needs from a computer, looked at before anything is fetched (WP4.20), the choice of a llama.cpp build for the graphics
card (`provision::pick_build`), and the tunnel and the step-by-step test (`remote.rs`, WP4.27, WP4.32).

## 3. The steps

**Identify.** One SSH command, with no question asked (BatchMode, as the tunnel): `uname -sm` and the home folder on
Linux and macOS; where that fails, the same asked of Windows' OpenSSH server, whose shell is `cmd` (or PowerShell): its
version and `%PROCESSOR_ARCHITECTURE%`. Then the free space of the folder `quickai-run` keeps its data in, and
`quickai-run --version` of the copy there, if any. The graphics
cards are found by `quickai-run` itself, once it is there (`check --json` says them in its `computer` event).

**Copy.** The `quickai-run` of this package for that system and processor, into the data folder there (C123: it writes
nothing outside it), by the person's own `scp` (Windows' OpenSSH server has it too), and checked by its SHA-256 once
copied. A copy of another version is replaced: the window and the program it drives are always of one release. The
configuration is **the one selected in the window**, as Export writes it (what belongs to this computer left out), and
with it the environment file (`init` writes one: what belongs to that computer, such as a folder or a login), filled
from the person's answers.

**Resolve.** `quickai-run check <configuration> --json` there: what would be installed and fetched, from where, and how
big it is. The window shows it before anything changes on that computer.

**Alternatives** (section 6). Where the configuration cannot be run as it is, the resolver proposes another way. The
person accepts or rejects each; a rejected one leaves the run stopped with what it needs said in a sentence.

**Install.** `quickai-run prepare`: the engine's program (a llama.cpp build for its card, Ollama, ComfyUI and its
Python), the packages, the models. System packages only when the person said so (`--install-system-packages`, C138),
and since nothing over SSH can answer a password, through `sudo -n` when that needs none; otherwise the window shows the
command for the person to run there.

**Run.** `quickai-run up <configuration> --detach` over the SSH connection: it starts on that computer as a program of
its own, which does not end with the connection (section 3.1), and the command returns. The window follows the run
with `quickai-run status --json` and the run's log there: its progress (installing, fetching with how far along,
starting), and once it is ready, where each thing it started listens.

**Tunnel.** Each of those ports is forwarded to this computer (`ssh -L`, one connection for all of them), to the same
port here unless the person gives another (as *Port here* does today), or a free one when that one is taken here. The
window says the addresses here (Connect an app, the page in the browser) and whether the run is up. That is the whole of
what it does with the run.

**Stop.** A run lives until it is stopped: from the window (Stop runs `quickai-run down` there), or by the person on that
computer (`quickai-run down`, or stopping the program). The window closing, the laptop sleeping or the connection
dropping only closes the tunnel; the run goes on, and the window finds it again (section 3.1).

### 3.1 A run that lives until it is stopped

The user: "forever - unless it is stopped from the UI - or unless the user manually stops it from the remote". What
`quickai-run` needs for it, on Linux, macOS and Windows alike:

- **`up --detach`** starts the run as a program of its own and returns once it has started. On Linux and macOS it leaves
  the connection's session (a new session, its input and output to a log file in its data folder, a hang-up ignored).
  On Windows, whose OpenSSH server closes the job of a connection's programs with it, it is started out of that job;
  where the server does not allow that, through the Task Scheduler. Both are to be tried on a real Windows server.
- **A run file** in the data folder: the program's id, the configuration, the version, when it started, and, once it is
  ready, where each thing listens (what `--ready-file` writes today). It is removed when the run ends.
- **`status --json`**: what runs there, or nothing; where it listens; the last lines of its log.
- **`down`**: stops the run. It writes a stop file the run looks for each second, so that a stop is the same on every
  system (Windows has no SIGTERM to send to a program of another console); the signals stay for Ctrl-C and a service
  manager on Linux and macOS.
- **One run at a time** on a computer, for one person: a second `up` while one runs says what runs, and starts nothing.

### 3.2 An engine that is already running there

The user: "if an engine is already running, inform the user and confirm over-ride - provide an option to stop it also",
and "here override means - stop the existing run and configuur/run ours". Before Start changes anything, the resolver
looks at what already runs on that computer: `status --json` for a run of QuickAI's, and the ports the configuration
will listen on for any other program (an Ollama started as a service, a ComfyUI someone left running). What it finds is
said (`Studio runs "Vision chat", since 14:02`; `Ollama answers on port 11434 on Studio, and QuickAI did not start it`),
and the person chooses; nothing is stopped or started without it:

- **Override**: stop what runs there, then configure and run the selected configuration;
- **Stop it**: stop what runs there, and run nothing;
- **Cancel**: leave it as it is.

A run of QuickAI's is stopped with `quickai-run down`, and another program of the same person by its process. A system
service (an Ollama installed as one) is stopped by QuickAI only through `sudo -n` when that needs no password; otherwise
the window says the command that stops it (`sudo systemctl stop ollama`) and waits for its port to be free.

The same check is made for a run on this computer, where it replaces what the window does today with a port that is
taken (*Use port N*, and C171's attach to an Ollama already answering).

## 4. Every release carries the headless program for every system

Each package of the window (the Windows zip, the macOS disk image, the AppImage and the Linux tar.gz) carries a
`remote/` folder: `quickai-run` for Linux x86_64, for macOS (universal) and for Windows x86_64, with their SHA-256. They
are small beside the model (today 4.6 MB and 8.4 MB as tarballs, against about 1 GB), and they mean a server needs no
access to GitHub to get QuickAI, and always gets the version of the window that drives it.

Windows, macOS and Linux are all built for, from the start (the user: "implement for all win/mac/linux"). What this
asks of the packaging repository (`quickai_base`):

- **A Windows build of `quickai-run`**, which does not exist yet, cross-built as the window's is. The runtime already runs
  on Windows under the window; the headless program's own parts (the stop, the console, `--detach`) are new there.
- **The macOS program is built on the Mac** (`macos-headless.sh`: `lipo`, `codesign`), and this computer cannot build it.
  The user: "just make provision for the build if you cannot - it will be built for you". So the packaging takes it from
  a known place, `dist/remote/` (or a path in `QUICKAI_RUN_MACOS`), where the Mac's run puts it, and a package made
  without it stops with a sentence that says where to put it; a flag builds one without it, for a test only.
- **Linux on ARM** (rented ARM servers, Jetson) is not built today; it is a target to add later.

## 5. Modules

The user: "make things modular as you implment - such that we can switch off or remove a specific module if needed."
Each part below is a module of its own: a file or folder in the crate it belongs to, a Cargo feature that is on by
default, and the window draws only what is compiled in. Switching one off is building without its feature; removing one
is deleting its folder and its feature line. The ones a person sees can also be switched off in the settings.

| Module | Where | What it does |
|---|---|---|
| `remote.ssh` | `quickai-runtime` | the transport: run a command, copy a file, open a tunnel (the tunnel is today's `remote.rs`) |
| `remote.probe` | `quickai-runtime` | identify a computer: system, processor, home, free space, the `quickai-run` there |
| `remote.deploy` | `quickai-runtime` | copy the headless program and the configuration, check them, replace another version |
| `resolve` | `quickai-runtime` (provision) | what a configuration needs and a computer lacks, and the alternatives; used for this computer too |
| `run.detach` | `quickai-run` | `up --detach`, the run file, `status`, `down`, the stop file, one run at a time |
| `remote.run` | `quickai-runtime`, the window | drive `quickai-run` over SSH: start detached, follow `status`, find a run again, stop |
| `remote.tunnel` | `quickai-runtime`, the window | forward the ports the run reports to this computer, and say their addresses here |
| `remote.attach` | `quickai-runtime`, the window | today's way: attach to an engine the person runs there (Ollama, ComfyUI) |
| `llamacpp.router` | parked | llama.cpp's router mode (architecture §10.4), parked at the user's word |

## 6. Alternatives

They come from what QuickAI already knows: the engine's documents (the builds there are for each system and card), the
catalog (the other files of a model, and how each engine runs it), and the computer's facts (memory, cards, free space).
Examples of what is offered:

- no llama.cpp build for that card: the Vulkan build, or the one for the processor, with what it costs in speed;
- an engine with no program for that system: another engine that runs the same model (the catalog's `run.<engine>`);
- a model that does not fit in its memory: a smaller quantization of the same model;
- not enough free space: another folder there, or fewer models.

Each is accepted or rejected in the window. What was accepted is kept in that computer's environment file, so the next
run does not ask again.

## 7. Decided, and what is left

Asked on 2026-10-28 and answered the same day:

1. **How long a run lives**: "forever - unless it is stopped from the UI - or unless the user manually stops it from the
   remote" (section 3.1).
2. **Which systems**: "windows servers - implement for all win/mac/linux" (section 4).
3. **Which configuration**: "the one which is selected in the window" (section 3).
4. **Where the macOS program is built**: "just make provision for the build if you cannot - it will be built for you"
   (section 4).

Not answered, and done as recommended unless the user says otherwise:

5. **Computers added today** (attach to a running engine): they keep working; a computer gets a *way*, "QuickAI sets it
   up" (the default for a new one) or "connect to an engine already running there".

## 8. Work packages (WP4.33)

| WP | Work | Verified by |
|---|---|---|
| 4.33.1 | `quickai-run` on Windows: it builds, stops (the stop file) and runs a configuration there | a cross build here; a run on Windows (as A8) |
| 4.33.2 | `run.detach`: `up --detach`, the run file, `status --json`, `down`, one run at a time, on all three systems | tests that start, find and stop a detached run here; a Windows run |
| 4.33.3 | Every package carries `quickai-run` for Linux, macOS and Windows (`quickai_base`), the macOS one taken from where the Mac's run puts it | the packaged-bundle check finds the three, with their SHA-256 |
| 4.33.4 | `remote.ssh` and `remote.probe`: run a command and copy a file over the person's `ssh` and `scp`; identify a computer, Windows' shell too | the stand-in for ssh (`testkit::fake_ssh`) runs them on this machine |
| 4.33.5 | `remote.deploy`: copy, check, replace another version; the selected configuration and its environment file | the stand-in, and a computer with an older copy |
| 4.33.6 | `resolve` with alternatives, for this computer first, then in `quickai-run check --json` | planner tests with documents that lack a build, a program, or room |
| 4.33.7 | `remote.run` and `remote.tunnel`: start detached, follow `status`, tunnel every port it reports, find a run again, stop; an engine already running there said, with override (stop it and run ours), stop it, or cancel (section 3.2) | the stand-in end to end, then the rented server for real |
| 4.33.8 | The window: a computer's way, the run's progress, the alternatives to accept or reject, the addresses here, a run that goes on when the window closes | the GUI suites, when the user asks for them |
| 4.33.9 | The modules as features, and their switches in the settings | a build with each feature off compiles and hides its part |
