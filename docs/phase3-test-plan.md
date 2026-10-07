# Phase 3 test plan

_Status: **written before the code**; the code lands work package by work package (3.5 is in, so K and S19a are `ready`) · Last updated: 2026-10-06 · Part of
[implementation-plan.md](implementation-plan.md) §8 · Counts at the end of §4_

Phase 3 makes Ollama an engine, downloaded on demand and never bundled. This document is the acceptance
criteria of that work in the form of cases: each row is one test, named after its ID, and most of them exist
already and fail for the reason the row gives. They were written first, as the plan's ground rule 4 asks of
kernel code ("write the table or golden test, watch it fail, then write the code"), and at the user's request
before the phase begins. The work packages (§8 of the implementation plan) turn them green; a work package is
done when its rows are, with the gate of §6.

## 1. How to read this

Every row has an **ID**, the **work package** (WP) that makes it pass, a **state** and the **case**. The ID is
the test's name: `p3_z05_...` is row Z05, and `crates/quickai/tests/phase3_plan.rs` checks, on every
`cargo test`, that this table and the test code agree in both directions (every row that has a test has one,
every test is a row, a `red` row is ignored and a `ready` row is not).

| State | Meaning |
|---|---|
| `ready` | The test exists and passes now: behaviour that already holds and must go on holding, or a check of the test kit itself |
| `red` | The test exists, is `#[ignore = "phase 3 WPx.y: ..."]`, and **fails when run**, for a reason that is the work it waits for. The work package removes the `ignore` |
| `manual` | The test exists and is ignored for good: it needs a real binary or the internet and is run by hand |
| `spec` | Defined here, not yet coded: it needs an interface not designed yet, or a platform that is not here. Its test is the first thing written in its work package, and the row turns `red` and then `ready` |

```
cargo test --workspace                                          # the ready tests; the red ones are ignored
cargo test -p quickai -- --ignored p3_ --skip p3_c02            # the red ones of the application: all fail until their WP lands
cargo test -p quickai-core -- --ignored p3_                     # the red ones of the core
QUICKAI_REAL_OLLAMA=<bin> QUICKAI_REAL_GGUF=<small.gguf> cargo test -p quickai p3_c02 -- --ignored   # a real Ollama against the recordings (manual)
( cd ../test/resources && python3 -m unittest -v test_fake_ollama )   # the Python fake against the same recordings
build/test.sh --include pending suites/ollama_lifecycle         # the GUI cases (when the work is there; see section 5)
```

A red test was checked to fail for the right reason: its failure message says what is missing ("the test engine
document was not accepted by this build: needs `attach@1`", "unknown variant `graceful`", "don't know how to
extract ... (expected a .zip or .tar.gz)", "not implemented: phase 3, WP3.3: http_call"). Where a behaviour
could be run against today's build, it was: K01 to K06 were run against the kill-only stop that exists today
and each failed on its own assertion.

## 2. What the tests stand on

| What | Where | For |
|---|---|---|
| **Recordings of a real Ollama v0.35.1** | `crates/quickai/tests/fixtures/ollama/recorded/` (33 answers), made by `record.sh` | What the real thing answers to every request QuickAI makes and to the mistakes a client can make; a real 46 MB pull, transcript and the hosts it contacted (`egress.txt`) |
| **The real release list** of `ollama/ollama` | `tests/fixtures/ollama/github_releases.json` | Four releases of seventeen files, with digests: the feed tests and the digests of the pins |
| **Archives made by the real `zstd`** | `tests/fixtures/archives/*.tar.zst`, made by `make_archives.sh` | The layout of the real archive; one frame with a checksum, none, and two frames; what the installer must read |
| **The test kit** | `crates/quickai/src/testkit/` (compiled for tests only; 24 tests of its own) | `mini_http` (a server that streams, cuts off and hangs up), `fake_ollama`, `fake_github`, `files`, `archives` (zip, tar.gz and tar.zst written by hand so an entry may be hostile), `procs`, `recorded` (shape comparison), `http` |
| **Interface stubs** | `quickai-core`: `stream`, `kvlines`, `loaderenv`, `registry::input`; `quickai`: `prims::http_call`, `prims::exec_probe`, `Session::load` and `unload` | The shape of what Phase 3 builds, with bodies that panic "not implemented", so the tests compile and fail for the right reason. The work packages fill them in and may change a signature; the rows are the contract, not the signatures |
| **The Python fake** | `quickai_base/test/resources/fake_ollama.py`, `test_fake_ollama.py` (19 tests), `OllamaLibrary.py` | The GUI suites' `ollama`: the same shapes, a runner child, SIGTERM that can be ignored, an environment record |
| **GUI cases** | `quickai_base/test/suites/` (47 cases tagged `pending`) | Section 5 |

## 3. The plan shape the E and S tests read

Planning stays a pure function, and its result stays plain data (decision C18). The tests read the plan as
JSON, so what follows is the contract between the planner and the supervisor, independent of the Rust types
that hold it:

```
RunPlan { engine, model, public_port, gateway, units: [Unit] }
Unit    { id, engine, endpoint { host, port }, apis,
          start:    [ Attach | Spawn ]                       in the order to try; the `mode` field decides which are present
          ready:    { path, expect_status, timeout_ms, interval_ms } | null
          warm:     [ Call ]                                  empty when no model is selected (management mode)
          unload:   [ Call ]
          stop:     { policy: "kill" | "graceful", grace_ms, exit_grace_ms } }
Attach  { how: "attach", endpoint, identity { path, pointer }, liveness { path, every_ms, failures } }
Spawn   { how: "spawn", program, args, env: [ EnvOp ], group }
Call    { method, path, body, timeout_ms, best_effort }
```

`mode = auto` gives `[attach, spawn]`, `attach` gives `[attach]`, `spawn` gives `[spawn]`. The session decides
at the moment of the start: an attach is made only when the port answers `GET /api/version` with JSON that has a
string `version` (anything else on the port is "something else is using the port", never attached to and never
a reason to start a second daemon on it); a spawn is refused when an Ollama already answers there.

## 4. Cases by group

### Z. The installer: `install.archive@1` grown up (WP3.1)

Formats, damaged and hostile archives, and a build that is either there completely or not at all. Every test goes through `spawn_install` with an archive served by a real local server. The generated archives are written by hand (`testkit::archives`) so a test can put anything in one; the three `.tar.zst` made by the real `zstd` (`tests/fixtures/archives/`) are what shows that the installer reads what a real compressor writes. The tree is the layout of the real v0.35.1 Linux archive: the binary first, no entry for `bin/` or `lib/ollama/` themselves, three directories with entries (the CUDA two and Vulkan), 17 relative links. **Z16 found a real defect on the first run**: an install killed 8 ms in leaves a directory that `list_installed` offers as a build with files missing. **WP3.1 landed these:** every Z row is `ready` (Z39, by hand, installs the real 1.44 GB archive: 9.8 s without CUDA and 10.6 s with everything, in a release build). Writing the code showed that `tar` stops at its end marker, so a zstd checksum and any trailing bytes are never read unless the stream is read on to its end (Z06 and Z07 caught it), and that `/tmp` on this machine has a per-user quota that other projects had used up (Z15 and Z16 write 64 MB: run them with `TMPDIR` on the disk).

| ID | WP | State | Case | Where |
|---|---|---|---|---|
| Z01 | 3.1 | ready | A .tar.zst with the real layout is installed and runs. | `p3/install.rs` |
| Z02 | 3.1 | ready | Archives made by the real zstd install the same whatever their framing. | `p3/install.rs` |
| Z03 | 3.1 | ready | A .tgz of the mac layout is installed. | `p3/install.rs` |
| Z04 | 3.1 | ready | A .zip of the windows layout is installed with its binary at the root. | `p3/install.rs` |
| Z05 | 3.1 | ready | A truncated .tar.zst says it is incomplete and leaves nothing. | `p3/install.rs` |
| Z06 | 3.1 | ready | A damaged .tar.zst is refused by its checksum and leaves nothing. | `p3/install.rs` |
| Z07 | 3.1 | ready | Garbage after the last frame is refused and a skippable frame before it is not. | `p3/install.rs` |
| Z10 | 3.1 | ready | An entry that climbs out or is absolute refuses the whole install. | `p3/install.rs` |
| Z10a | 3.1 | ready | Nothing is ever written outside the install directory. | `p3/install.rs` |
| Z11 | 3.1 | ready | A link that points out and a file written through it are refused. | `p3/install.rs` |
| Z12 | 3.1 | ready | Only a relative link that stays inside the tree is kept. | `p3/install.rs` |
| Z13 | 3.1 | ready | A hard link is refused. | `p3/install.rs` |
| Z15 | 3.1 | ready | While it extracts the final directory never holds half a tree. | `p3/install.rs` |
| Z16 | 3.1 | ready | An install killed at any moment leaves nothing that looks installed. | `p3/install.rs` |
| Z17 | 3.1 | ready | The leftovers of an install are never listed as builds. | `p3/install.rs` |
| Z18 | 3.1 | ready | A stale staging directory is replaced not merged by the next install. | `p3/install.rs` |
| Z19 | 3.1 | ready | No download or staging files remain after a success or a failed download or extract. | `p3/install.rs` |
| Z19b | 3.1 | ready | An archive with no executable leaves nothing behind. | `p3/install.rs` |
| Z22 | 3.1 | ready | An archive with no entries for its parent directories still extracts. | `p3/install.rs` |
| Z30 | 3.1 | ready | A download whose sha256 equals the digest of the build is installed; one that does not is refused **before extraction starts**, the message names both digests (shortened), the `.part` file is removed and nothing is left in the root. | `p3/install.rs` |
| Z31 | 3.1 | ready | Pin and host digest: equal is installed; a pin with no host digest is verified by the pin; a pin that **differs** from the host digest is refused ("the file changed after it was tested"), whichever is right. | `p3/install.rs` |
| Z32 | 3.1 | ready | No digest anywhere (an address the person pasted): installed, the result is flagged unverified and the status line says so. | `p3/install.rs` |
| Z33 | 3.1 | ready | `skip` globs: with `gpu.known and not has(gpu.vendors, 'nvidia')` the `lib/ollama/cuda_*` entries (the directory entry, its files, a link inside it) are not unpacked while `bin/ollama` and `lib/ollama/vulkan` are; with an NVIDIA GPU, or none detected, nothing is skipped; the installed size reflects it (the real archive: 2.2 GB, 102 MB). | `p3/install.rs` |
| Z34 | 3.1 | ready | Smoke test: the unpacked binary runs `--version` and exits 0 → installed; a non-zero exit → refused with the program's own output, nothing installed; a program that hangs is killed at the timeout and refused. | `p3/install.rs` |
| Z35 | 3.1 | ready | Free-space preflight: less than archive + unpacked + margin is refused **before any request** (the file server sees none) and the message says needed against free. | `p3/install.rs` |
| Z36 | 3.1 | ready | Overlays: the ROCm archive is unpacked over the same staging directory and the directory renamed once; a failing overlay leaves nothing; an overlay is not downloaded unless chosen. | `p3/install.rs` |
| Z37 | 3.1 | ready | Progress is monotonic, ends at the total the server announced, and unpacking is reported as its own step. | `p3/install.rs` |
| Z38 | 3.1 | ready | llama.cpp installs go through the same pipeline: every existing installer test passes unchanged, and an install killed mid-way leaves nothing listed (Z16 with the llama.cpp spec). | `p3/install.rs` |
| Z39 | 3.1 | manual | The real v0.35.1 archive (1.44 GB) installs through the pipeline: its digest is the pin, the CUDA libraries are left out (102 MB), the real binary passes the smoke test, and all 2.2 GB unpack in a reasonable time. | `p3/install.rs` |

### R. The release feed (WP3.2)

Against the recorded release list of the real `ollama/ollama` repository (`tests/fixtures/ollama/github_releases.json`: four releases of seventeen files each; **the release candidate v0.40.0-rc5 is listed above the stable v0.35.1**, so taking the first release would install a candidate). R01 to R03 and R07 passed before the work: the exact-name rules of Phase 2 already held up against real data. **WP3.2 landed the rest** (all but R08, which waits for the embedded Ollama document): digests and sizes on every build (R04), the rate limit said as one with when it resets (R05), `feed.api` (R06, R06b), the tested release first with its pins and a conflict flagged (R10), overlays (R11).

| ID | WP | State | Case | Where |
|---|---|---|---|---|
| R01 | 3.2 | ready | The real feed offers one build per stable release for each supported platform. | `p3/feed.rs` |
| R02 | 3.2 | ready | Of the seventeen files of a release only the five base builds are ever offered. | `p3/feed.rs` |
| R03 | 3.2 | ready | The release candidate listed first is skipped unless the document counts prereleases. | `p3/feed.rs` |
| R04 | 3.2 | ready | Every offered build carries the sha256 and size the API gave for its file; a file with no `digest` has none. | `p3/feed.rs` |
| R05 | 3.2 | ready | A rate limit is reported as one with the time it resets. | `p3/feed.rs` |
| R06 | 3.2 | ready | A document can name another API root which is how a test points a feed at a fake. | `p3/feed.rs` |
| R06b | 3.2 | ready | An API root that is not HTTP is refused when the document loads. | `p3/feed.rs` |
| R07 | 3.2 | ready | A lookup asks once and building the spec asks nothing. | `p3/feed.rs` |
| R08 | 3.7 | ready | The embedded ollama document finds real builds. | `p3/feed.rs` |
| R09 | 3.2 | ready | A rate limited answer is recognised by the helper the real network tests use. | `p3/feed.rs` |
| R10 | 3.2 | ready | The tested tag (v0.35.1) is flagged and listed first when the feed has it, with the document's pins; a pin that disagrees with the feed's digest flags the build as conflicting. | `p3/feed.rs` |
| R11 | 3.2 | ready | Overlays (`rocm`) are offered only on the platforms that have one, are never offered as a base build and are off until chosen. | `p3/feed.rs` |
| R12 | 3.8 | ready | Through the window: a look-up that fails keeps the sentence it gave (the rate limit's, with when it resets, or `Lookup failed: ...`) instead of being overwritten by "No compatible builds found.", which is what a look-up that found nothing says. | `app/engine.rs` |

### H. Calling the engine and reading a stream (WP3.3)

`http_call@1`, the NDJSON and SSE framing, and what a model pull adds up to. **WP3.3 landed all of it** (the stubs are real now); H17b was added for the cancel of a pull, and H05f's expected count was corrected (the recording has 14 events, 28 lines). The interfaces were stubs (`quickai-core::stream`, `quickai::prims::http_call`) when the tests were written against them. The facts the pull tests are built on come from the recording of a real pull (O18 in `engine-facts.md`): **the real stream has many lines with `total` and no `completed`, small layers never get one, the big layer ends a little short of its total, and a failed pull is an `{"error": ...}` line inside a 200 response.**

| ID | WP | State | Case | Where |
|---|---|---|---|---|
| H01 | 3.3 | ready | The JSON pointers of the documents resolve as RFC 6901 says. | `core/tests/p3_pure.rs` |
| H02 | 3.3 | ready | A call sends what the document says and says who is asking. | `p3/http_call.rs` |
| H03 | 3.3 | ready | An error status carries the engines own sentence when it gave one. | `p3/http_call.rs` |
| H04 | 3.3 | ready | An empty reply is null a delete may carry a body and text is not json. | `p3/http_call.rs` |
| H05a | 3.3 | ready | Lines come out whole and in order however the bytes arrive. | `core/tests/p3_pure.rs` |
| H05b | 3.3 | ready | Crlf blank lines and a last line without a newline. | `core/tests/p3_pure.rs` |
| H05c | 3.3 | ready | A line that is not JSON is an error with its number and the others still come. | `core/tests/p3_pure.rs` |
| H05d | 3.3 | ready | A multibyte character cut by a chunk boundary is not damaged. | `core/tests/p3_pure.rs` |
| H05e | 3.3 | ready | A very long line is read in reasonable time. | `core/tests/p3_pure.rs` |
| H05f | 3.3 | ready | The recorded OpenAI stream is the same events however it is chunked. | `core/tests/p3_pure.rs` |
| H05g | 3.3 | ready | SSE names comments multiline data crlf and an unfinished last event. | `core/tests/p3_pure.rs` |
| H06a | 3.3 | ready | The real pull never goes backwards stays below one until success and ends finished. | `core/tests/p3_pure.rs` |
| H06b | 3.3 | ready | The engines own words go to the log once per step and once per layer. | `core/tests/p3_pure.rs` |
| H06c | 3.3 | ready | An error line fails the pull with the engines sentence. | `core/tests/p3_pure.rs` |
| H06d | 3.3 | ready | Layers pulled side by side add up and a missing completed counts as nothing. | `core/tests/p3_pure.rs` |
| H06e | 3.3 | ready | A completed above its total is clamped and a total that grows does not move the bar back. | `core/tests/p3_pure.rs` |
| H06f | 3.3 | ready | A model that is already there goes straight to the end. | `core/tests/p3_pure.rs` |
| H07 | 3.3 | ready | Each event of a stream is handed over as it arrives not all at the end. | `p3/http_call.rs` |
| H08 | 3.3 | ready | An error line in a stream that started with 200 fails the call after the events before it. | `p3/http_call.rs` |
| H09 | 3.3 | ready | A stream that is cut off is an error not a quiet end. | `p3/http_call.rs` |
| H10 | 3.3 | ready | Cancelling ends the call at once and the engine stops writing. | `p3/http_call.rs` |
| H11 | 3.3 | ready | A call the engine never answers times out. | `p3/http_call.rs` |
| H12 | 3.3 | ready | Nothing can be aimed anywhere but at the engine on this machine. | `p3/http_call.rs` |
| H13 | 3.3 | ready | A redirect is not followed wherever it points. | `p3/http_call.rs` |
| H15 | 3.3 | ready | `catalog.engine_list`: `/api/tags` becomes rows (name, size, digest, capabilities) in the order given; a row without a name is skipped with a warning; an empty list is an empty list; the list is cached (written atomically) and read back, marked as stale, when the daemon does not answer; a corrupt cache is ignored. | `p3/http_call.rs` |
| H16 | 3.3 | ready | Server sent events arrive as JSON and the done marker ends the stream. | `p3/http_call.rs` |
| H17 | 3.3 | ready | The pull as a job: progress follows the tracker, the engine's words reach the log, `Done` refreshes the list, `Failed` carries the sentence, cancelling closes the connection (the fake stops writing). | `p3/http_call.rs` |
| H17b | 3.3 | ready | Cancelling a pull closes the connection and the job says so. | `p3/http_call.rs` |

### S. The session: attach, start, load, stop (WP3.4)

Everything through a document, as the application does: a test engine document with the sections of `docs/examples/engines/ollama.toml` and test-sized timings, the planner, and `Session::start`. The "daemon" is a shell script that records its environment and arguments and keeps a runner child; its HTTP side is the fake, bound only after the script has started (an Ollama that answers before it exists would be attached to). A test fails first at "the document was not accepted by this build" and then, as the work packages land, on its own assertion; the harness itself is proved by K00, which passes today.

| ID | WP | State | Case | Where |
|---|---|---|---|---|
| S01 | 3.4 | ready | An ollama that already answers is attached to and nothing is started. | `p3/session.rs` |
| S02 | 3.4 | ready | With nothing on the port the daemon is started as the document says. | `p3/session.rs` |
| S03 | 3.4 | ready | Attach mode with nothing running fails and starts nothing. | `p3/session.rs` |
| S04 | 3.4 | ready | Spawn mode refuses a port where an ollama already answers. | `p3/session.rs` |
| S05 | 3.4 | ready | A server on the port that is not ollama is never attached to. | `p3/session.rs` |
| S06 | 3.4 | ready | A daemon that never answers is given up on and stopped with its runner. | `p3/session.rs` |
| S07 | 3.4 | ready | The session is not running until the model is loaded. | `p3/session.rs` |
| S08 | 3.4 | ready | A model that is not there crashes the start and a started daemon is stopped. | `p3/session.rs` |
| S09 | 3.4 | ready | A model that does not fit is reported with the engines sentence. | `p3/session.rs` |
| S10 | 3.4 | ready | A load that takes too long is a crash that says so. | `p3/session.rs` |
| S11 | 3.4 | ready | With no model the daemon runs with nothing loaded. | `p3/session.rs` |
| S12 | 3.4 | ready | A model is loaded into a running session and a failed load leaves it running. | `p3/session.rs` |
| S13 | 3.4 | ready | Stop unloads the model first and then stops the daemon it started. | `p3/session.rs` |
| S14 | 3.4 | ready | Stop on an attached daemon unloads the model and leaves the daemon running. | `p3/session.rs` |
| S15 | 3.4 | ready | An unload that fails or hangs does not hold up the stop. | `p3/session.rs` |
| S16 | 3.4 | ready | A daemon that dies by itself is a crash and clears what the run was for. | `p3/session.rs` |
| S17 | 3.4 | ready | An attached daemon that disappears is noticed and probing stops with the run. | `p3/session.rs` |
| S18 | 3.4 | ready | One missed probe is not a crash. | `p3/session.rs` |
| S19 | 3.12 | ready | The log's second line also carries the user's own environment lines, hides their secrets and shows a locked variable as the document set it, whatever the lines said. | `p3/session.rs` |
| S19a | 3.5 | ready | The log starts with the command then the variables the plan sets and hides secrets (the hygiene document). | `p3/session.rs` |
| S20 | 3.4 | ready | Live traffic is not started in front of a daemon that was already there. | `p3/session.rs` |
| S21 | 3.4 | ready | Live traffic in front of a started daemon binds the public port and the daemon the internal one. | `p3/session.rs` |
| S22 | 3.4 | ready | A second start while running is ignored and stop then start works again. | `p3/session.rs` |
| S23 | 3.4 | ready | The environment the application itself was started with never overrides a locked variable (`OLLAMA_HOST=0.0.0.0:80` inherited → the daemon gets the planned one) and, with the engine's own store switched on, an inherited `OLLAMA_MODELS` is left alone (run in a child process, so the test does not touch its own environment). | `p3/session.rs` |

### K. Process hygiene (WP3.5)

A daemon owns GPU memory and a runner process: Stop, closing the window and killing the application must leave neither. The tests start real processes and look for them by pid. They use a smaller document of their own, so that WP3.5 can land before WP3.4. K00 proves the harness on today's build; K01 to K06 were also run against the kill-only stop of today to see that each fails for its own reason (the runner survives; no SIGTERM was sent; killed before the grace; not a group leader; the daemon outlives the application). K05 runs the application's side in a child process and kills it. **WP3.5 landed these:** K01 to K10 pass (K08 is a unit test of a pure function, K09 runs only on Windows, in CI, K10 is the Unix stop path made portable), and so does S19a, the same log lines as S19 through the hygiene document; S19 itself waits for the user's environment lines (WP3.12). Writing the code showed one flaw in the stand-in daemon: it set its `SIGTERM` trap after it started its runner, so a group-wide SIGTERM killed the runner, the loop that waits for it ended and a "stubborn" daemon left at once; the trap now comes first (an ignored signal is inherited, a handler is not).

| ID | WP | State | Case | Where |
|---|---|---|---|---|
| K00 | 3.5 | ready | The harness runs a daemon to running and stops it. | `p3/session.rs` |
| K01 | 3.5 | ready | Stop ends the whole process group not only the child. | `p3/session.rs` |
| K02 | 3.5 | ready | A daemon that ignores SIGTERM is killed when the grace has passed. | `p3/session.rs` |
| K03 | 3.5 | ready | A daemon that obeys SIGTERM is not made to wait out the grace. | `p3/session.rs` |
| K04 | 3.5 | ready | Closing the app stops even a stubborn daemon within the exit grace. | `p3/session.rs` |
| K05 | 3.5 | ready | A daemon follows the app when the app is killed. | `p3/session.rs` |
| K06 | 3.5 | ready | The daemon leads a process group of its own. | `p3/session.rs` |
| K07a | 3.5 | ready | An AppImage's directories are dropped from the loader path. | `core/tests/p3_pure.rs` |
| K07b | 3.5 | ready | Only a directory inside the root counts not one that merely starts like it. | `core/tests/p3_pure.rs` |
| K07c | 3.5 | ready | A snap's directories go too and a variable with nothing left is removed. | `core/tests/p3_pure.rs` |
| K07d | 3.5 | ready | With no appimage and no snap nothing changes which is how llama.cpp stays as it was. | `core/tests/p3_pure.rs` |
| K08 | 3.5 | ready | Every spawn carries `CREATE_NO_WINDOW` on Windows: the flag set is a pure function, tested on every platform; that no console window opens is A8, observed on Windows. | `runtime/process.rs` |
| K09 | 3.5 | ready | Windows: the daemon is in a Job Object with `KILL_ON_JOB_CLOSE`: a child that spawns a child, and neither survives Stop or the end of the application (a `cfg(windows)` test, run by CI). | `runtime/process.rs` |
| K10 | 3.5 | ready | macOS: no parent-death signal exists, so the daemon is stopped by Stop and by the exit path only; the stop path is the Linux one (CI compiles and runs it). | `p3/session.rs` |
| K11 | 3.8 | ready | A program that leads no process group of its own (llama-server) follows the application when it is killed, as the daemon of K05 does: on Linux the kernel signals every program the application started, with a group or without (finding F6, TC-SRV-022). | `p3/session.rs` |

### D. Reading Ollama's traffic (WP3.6)

The proxy in front of the fake Ollama. D01 and D07 hold today: the OpenAI shape is read as it always was and the relay is byte for byte. How a path is read is the vocabulary's to say (D08): the proxy picks the decoder that claims the path, so Ollama's native API is read like the OpenAI one (D02 to D06), a load or an unload is not a conversation (D03b), and an error status reaches the client as it is (D09, which TC-LIVE-017 asked for). Streams carry their text in `ResponseDelta` events and end in a `ResponseDone` with no text, as an OpenAI stream does; a reply that comes whole carries it in the `ResponseDone` (D03c).

| ID | WP | State | Case | Where |
|---|---|---|---|---|
| D01 | 3.6 | ready | An OpenAI chat through the proxy is read as it always was. | `p3/proxy.rs` |
| D02 | 3.6 | ready | Ollama's native chat is read like an OpenAI chat. | `p3/proxy.rs` |
| D03 | 3.6 | ready | Ollama's generate is read from its prompt and its response field. | `p3/proxy.rs` |
| D03b | 3.6 | ready | A load or an unload is not shown as a conversation. | `p3/proxy.rs` |
| D03c | 3.6 | ready | A reply that comes whole carries its text in the done event. | `p3/proxy.rs` |
| D04 | 3.6 | ready | Images sent to ollama are listed as attachments. | `p3/proxy.rs` |
| D05 | 3.6 | ready | An error reply and an error in the stream are shown as errors. | `p3/proxy.rs` |
| D06 | 3.6 | ready | Lines cut in the middle by the network are put back together. | `p3/proxy.rs` |
| D07 | 3.6 | ready | The stream reaches the client exactly as the engine sent it. | `p3/proxy.rs` |
| D08a | 3.6 | ready | An API that only a document defines is read by its pointers with no code change (request, deltas, final text, error, skip roles, images). | `core/tests/p3_decode.rs` |
| D08b | 3.6 | ready | An engine naming an API the vocabulary lacks is a load diagnostic with a suggestion, and is left out; with no vocabulary at all nothing is checked. | `core/tests/p3_decode.rs` |
| D08c | 3.6 | ready | A vocabulary that says it wrongly is reported where it is wrong (framing, pointers, paths, id, prompt and chat together, none of them, two documents claiming one API or path, a primitive this build lacks). | `core/tests/p3_decode.rs` |
| D08d | 3.6 | ready | The plan carries the decoders of the engine's APIs when the proxy is on, and none without it. | `core/tests/p3_decode.rs` |
| D08e | 3.6 | ready | The proxy reads an API that only a document defines, on a server of a made-up engine, and shows nowhere a path no document claims. | `p3/proxy.rs` |
| D08f | 3.6 | ready | A vocabulary of the person's patches the embedded one by API id: a list such as `paths` is replaced, a table such as `decode` is merged key by key, and an API the patch adds is read like any other. | `core/tests/p3_decode.rs` |
| D09 | 3.6 | ready | An error status reaches the client as it is (not a 502) and is shown as an error with the engine's sentence, for Ollama's shape and for OpenAI's. | `p3/proxy.rs` |
| D10 | 3.8 | ready | A reply that streams reaches the client as it arrives, not when the engine has finished (finding F1 of the GUI suite, TC-LIVE-008): the first line is read long before the last, and the reply is whole. | `p3/proxy.rs` |

### E. The Ollama engine as shipped: document, planner, form, settings (WP3.7, 3.12)

Against the embedded registry. The plan is read as JSON, so the tests say what a plan contains (section 3) without depending on the Rust types. **The context size has no default for Ollama** (decision C35): the engine's own default is tiered by VRAM (O12), which QuickAI cannot see, so a number made up from RAM would override something better informed; the variable is set only when the person fills the field.

| ID | WP | State | Case | Where |
|---|---|---|---|---|
| E01 | 3.7 | ready | The embedded set has an ollama engine that loads clean. | `p3/engine.rs` |
| E01b | 3.7 | ready | The engines are listed with llama.cpp first for the selector. | `p3/engine.rs` |
| E02 | 3.7 | ready | The pins of the embedded document are those digests. | `p3/engine.rs` |
| E02a | 3.7 | ready | The digests in the facts are the ones of the recorded release. | `p3/engine.rs` |
| E03a | 3.7 | ready | The default plan attaches if it can and otherwise starts ollama serve. | `p3/engine.rs` |
| E03b | 3.7 | ready | A context size the user chose is passed as the environment and never in the warm up. | `p3/engine.rs` |
| E03c | 3.7 | ready | Turning the GPU off sets the three variables that do it. | `p3/engine.rs` |
| E03d | 3.7 | ready | The model store is quickais own unless Ollama's is asked for. | `p3/engine.rs` |
| E03e | 3.7 | ready | The mode decides which strategies the plan has. | `p3/engine.rs` |
| E03f | 3.7 | ready | Another port is the port of the attach the spawn and the plan. | `p3/engine.rs` |
| E03g | 3.7 | ready | With the proxy on the daemon listens on the internal port and the gateway on the public one. | `p3/engine.rs` |
| E03h | 3.7 | ready | Without a model the plan has no warm up and no unload. | `p3/engine.rs` |
| E03i | 3.7 | ready | The users environment lines follow the documents and cannot replace the locked ones. | `p3/engine.rs` |
| E03j | 3.12 | ready | A field's variables (bind.env) are set when its condition holds, none while the field has no value, in the order of the fields' bindings and after the document's own, each with its field as the origin. | `core/tests/p3_env.rs` |
| E03k | 3.12 | ready | The person's lines come last, a line for an engine variable that is not locked replaces it in place, and the locked, protected and prepended names cannot be set in any spelling. | `core/tests/p3_env.rs` |
| E03l | 3.12 | ready | On Windows two spellings of a name are one variable (the later line wins) and the protected names stay protected; on Linux they are two variables. | `core/tests/p3_env.rs` |
| E04 | 3.7 | ready | An attach only plan configures nothing for a daemon that is not ours. | `p3/engine.rs` |
| E05 | 3.7 | ready | `plan --explain` of the default Ollama selection, as a golden: every environment variable and argument with its origin, the warm call and the unload. | `p3/engine.rs` |
| E06 | 3.7 | ready | The form of the Ollama engine as a golden (`tests/golden/form_ollama.json`) in four states: no model, a model, running (spawned), attached. | `p3/engine.rs` |
| E07 | 3.7 | ready | The form has the ollama fields in the documents sections and order. | `p3/engine.rs` |
| E08 | 3.7 | ready | While the session runs the settings that need a restart cannot be edited. | `p3/engine.rs` |
| E09 | 3.7 | ready | A bad environment line is an error under the field and the value is kept. | `p3/engine.rs` |
| E09b | 3.12 | ready | A bad line is the field's error with its line number, a protected one says who sets it, and the text stays as typed (on the test document; E09 is the same on the real one). | `core/tests/p3_env.rs` |
| E10 | 3.7 | ready | Ollama's values live under values and the nine keys of the file are untouched. | `p3/engine.rs` |
| E11a | 3.12 | ready | Lines are read blank lines and comments are skipped and the value keeps its equals signs. | `core/tests/p3_pure.rs` |
| E11b | 3.12 | ready | A bad line is a problem with its number and the good ones still count. | `core/tests/p3_pure.rs` |
| E11c | 3.12 | ready | The protected names are refused by name and on windows in any case. | `core/tests/p3_pure.rs` |
| E11d | 3.12 | ready | A later line for the same name replaces the earlier in place. | `core/tests/p3_pure.rs` |
| E12 | 3.7 | ready | An engine the settings name that has no document falls back to llama.cpp, with a diagnostic; its values are kept. | `p3/engine.rs` |
| E13 | 3.7 | ready | The API panel data for Ollama: `http://127.0.0.1:PORT/v1`, the extra URL, the key "none" and the note that a request naming another model loads that one; llama.cpp's is unchanged. | `p3/engine.rs` |
| E14 | 3.7 | ready | The document's `[[diagnostic]]` rules match the log: address in use → hint and focus on the port, out of memory → hint, no GPU → hint; a rule with a regular expression that does not compile is a load diagnostic. | `p3/engine.rs` |
| E14b | 3.7 | ready | A diagnostic rule applies when every condition it names holds (the log, the state the run ended in, both), in the document's order and each piece of advice once, with the hint filled in from the settings. | `core/tests/p3_advice.rs` |
| E14c | 3.7 | ready | A diagnostic rule written wrongly is a load diagnostic at its line: a pattern that does not compile, a state a run does not end in, an action that is not an id, an unknown name or an unclosed hole in the hint, no condition at all. | `core/tests/p3_advice.rs` |
| E15 | 3.7 | ready | A document needing a primitive this build lacks (`attach@1` removed from the known set) is skipped with the primitive named. | `p3/engine.rs` |
| E16 | 3.7 | ready | `config check --strict --embedded` is clean with the Ollama document in the set, and CI runs it. | `cli.rs` |
| E17 | 3.7 | ready | The ollama example fits the typed engine schema with nothing ignored. | `core/tests/p3_schema.rs` |
| E18 | 3.7 | ready | The llama.cpp example is typed but for the components that wait for phase 4. | `core/tests/p3_schema.rs` |
| E19 | 3.7 | ready | The generated reference page (`docs/reference/engine-fields.md`) describes every new attribute and key, in both directions as in Phase 2 (D1). | `core/tests/reference.rs` |
| E20 | 3.7 | ready | `the_llamacpp_example_fits_the_typed_engine_schema` in `tests/examples.rs`, which asserts that `runtime`, `models`, `traffic`, `api_info` and `diagnostic` are *ignored*, is replaced by E17 and E18. | `core/tests/examples.rs` |
| E21 | 3.7 | ready | Settings: `values."app.engine"` is read and written through an API of `Settings`; it is absent for llama.cpp (so a llama.cpp user's file keeps exactly its nine keys after choosing Ollama and back), and the Ollama values stay when the choice goes back. | `p3/engine.rs` |
| E22 | 3.7 | ready | The Ollama document the application ships is the one `docs/examples` shows. | `p3/engine.rs` |
| E23 | 3.7 | ready | Changing the engine keeps what each holds, saves the choice at once, loads the other's program and shows the models it listed last time (the remembered one chosen by what it means); it cannot be changed while something is being fetched. | `app/engine.rs` |
| E24 | 3.7 | ready | Choosing Ollama starts nothing and asks nothing: the frames that follow, a refresh with it stopped and the picker opened send no request to a daemon that is running on the configured port. | `app/engine.rs` |
| E25 | 3.12 | ready | The bindings of environment are checked where they are written: protect names, env_lines on the wrong type, an unknown name, value and prepend together, locked on a field's variable, a bad name. | `core/tests/p3_env.rs` |
| E25b | 3.7 | ready | An engine that is downloaded on request and is not installed is missing (Start cannot work and the window says so); one that ships with the app is tried. | `app/engine.rs` |
| E26 | 3.12 | ready | plan --explain says where every variable came from (the document, a field, the person's lines). | `core/tests/p3_env.rs` |
| E26b | 3.7 | ready | Every panel of the main window and the Settings window draws for both engines in every state a window can be in (stopped, starting, running, stopping, crashed; with and without models, a notice, a removal waiting for a yes). | `app/engine.rs` |
| E27 | 3.7 | ready | `quickai plan --engine ollama --model NAME` explains an Ollama run (the environment, the warm-up by name) and runs empty without a model; an engine nobody has a document for is refused. | `cli.rs` |
| E28 | 3.8 | ready | The embedded Ollama document can look up and install a build on every platform it has a file for: a look-up spec, an install spec (the executable named, `${data}/ollama` as the root, the smoke test that runs `--version`) — without one the Download button of the window does nothing. | `p3/engine.rs` |

### L. Finding the binary and asking its version (WP3.11)

`exec.probe@1` against what the real `ollama --version` prints in both situations (A17), and the three locate strategies. **WP3.11 landed all of it:** the strategies are given the machine to look at (a directory standing for its disk, a system, an environment), so a test plays Windows or a Mac on Linux; the version probe is not yet asked by the application (WP3.7).

| ID | WP | State | Case | Where |
|---|---|---|---|---|
| L01 | 3.11 | ready | `installed`: with several builds under the root the newest by modification time wins; a `.partial` directory and one without the binary are skipped; the binary's path is per OS (`bin/ollama`, `ollama.exe`); a missing root is no match. | `p3/probe.rs` |
| L02 | 3.11 | ready | `path_search`: finds `ollama` in a directory of PATH; on Unix it must be executable, on Windows the extensions of PATHEXT count; a directory called `ollama` is no match; empty and relative PATH entries are ignored; the first match wins. | `p3/probe.rs` |
| L03 | 3.11 | ready | `known_paths`: per-OS lists; `%LOCALAPPDATA%` and `~` are expanded from the environment that is passed in; a path that does not exist is no match. | `p3/probe.rs` |
| L04 | 3.11 | ready | The order of the strategies is the document's, the first that yields a path wins, and `plan --explain` and the Settings window say which strategy found it. | `p3/probe.rs` |
| L05 | 3.11 | ready | The version is read from both forms of what the real ollama prints. | `p3/probe.rs` |
| L05b | 3.11 | ready | Stderr counts a failing program that still says it counts and one that says nothing does not. | `p3/probe.rs` |
| L05c | 3.11 | ready | A program that hangs is killed at the timeout and gives no version. | `p3/probe.rs` |
| L06 | 3.11 | ready | The version is cached by path and modification time: the second ask does not start the program (a script that appends to a file counts the starts). | `p3/probe.rs` |

### M. Models as named references (WP3.13)

What the model box does with what was typed (the document's `input_rules`, first match wins), and the model list, pull and remove around a running daemon.

| ID | WP | State | Case | Where |
|---|---|---|---|---|
| M01 | 3.13 | ready | What is typed into the model box is pulled or refused with a sentence. | `core/tests/p3_pure.rs` |
| M01b | 3.13 | ready | A link to a file a local file and nonsense are refused in the documents words. | `core/tests/p3_pure.rs` |
| M02 | 3.13 | ready | The model list is fetched only when a daemon is running (the engine's `needs_server`); stopped, the cached list is shown, marked as cached; nothing is fetched because Ollama was chosen. | `p3/models.rs` |
| M03 | 3.13 | ready | Removing a model asks first, sends `DELETE /api/delete` with its name and refreshes the list; the model that is loaded cannot be removed until the session is stopped. | `p3/models.rs` |
| M04 | 3.13 | ready | `qwen3` and `qwen3:latest` are the same model for the picker, for the remembered model and for the warm-up; the warm-up uses exactly the name as the engine lists it. | `core/tests/p3_models.rs` |
| M04b | 3.13 | ready | The warm-up names the model exactly as the engine lists it: a remembered `bonsai-q1` is found among the listed names and the plan carries `bonsai-q1:latest`; with no model there is nothing to warm. | `core/tests/p3_models.rs` |
| M05 | 3.13 | ready | The size the listing gives is the size the context heuristics and the speed estimate use for an Ollama model, as the file size is for a GGUF. | `p3/models.rs` |
| M06 | 3.13 | ready | The `[models]` section loads and its calls come out with the templates filled in: the list (its cache with `${data}`), the pull (`${ref}` in the body, how the reply is read, the progress pointers and their defaults), the remove; an engine with no `[models]` has none of it. | `core/tests/p3_models.rs` |
| M06b | 3.13 | ready | A `[models]` section that says it wrongly is reported where it is wrong: pointers, a way to read a reply, a method, a path, a rule that does nothing, a `pull` with no `ref`, a `reject` with no message, an unknown name in a template; a rule after one that takes everything is a warning; a warm-up that streams is an error. | `core/tests/p3_models.rs` |
| M06c | 3.13 | ready | The document gives the calls the built-in specs make: the same rows from the list, and a pull the fake completes with exactly the name asked for. | `p3/models.rs` |
| M06d | 3.13 | ready | The `[models]` section of `docs/examples/engines/ollama.toml` loads with no diagnostic of any kind and its rules answer as M01 says. | `core/tests/p3_models.rs` |
| M07 | 3.13 | ready | The model box of Ollama refuses what is not a name with the document's sentence and sends nothing, and asks for a running daemon to pull a name. | `app/engine.rs` |
| M08 | 3.13 | ready | With Ollama running: the models are listed when it comes up (once), a name typed into the box is pulled with progress and appears, a pull that fails shows the daemon's words, a removal goes by the listed name and refreshes the list, and attaching to a daemon of someone else's is known to the form. | `app/engine.rs` |
| M09 | 3.8 | ready | Through the window: with the daemon running and nothing loaded, Start loads the model the picker has chosen (one warm-up call with the name the engine lists, kept for ever, no context size), does nothing while the load is under way or once a model is loaded, and a load that fails leaves the daemon running with nothing loaded, says why in the log and lets Start try again. | `app/engine.rs` |

### W. The widgets Phase 2 deferred (WP3.14)

Key-value lines, a progress bar, a button, a path picker: each has its first user in the Ollama form.

| ID | WP | State | Case | Where |
|---|---|---|---|---|
| W01 | 3.14 | ready | Key-value lines: a multi-line text box; the whole text is handed back as typed (nothing reformatted while typing); the form's messages show under it, one per bad line. | `app/widgets.rs` |
| W01b | 3.14 | ready | The form's messages show under the box of key-value lines, a row to a message (the form puts each problem on a line of its own). | `app/widgets.rs` |
| W02 | 3.14 | ready | Progress bar: a fraction and an indeterminate state render without panicking for a total of 0, a download above its total and a fraction of exactly 1. | `app/widgets.rs` |
| W02b | 3.14 | ready | Only a bar with no known end keeps the window repainting: it is drawn moving, a bar that says how far along it is does not ask for more frames. | `app/widgets.rs` |
| W02c | 3.14 | ready | A job's fraction: a total of 0 or none says nothing, more than the total is all of it. | `jobs.rs` |
| W03 | 3.14 | ready | Button: a click hands back its action id once; disabled while its `enabled_when` is false. | `app/widgets.rs` |
| W03b | 3.14 | ready | A `hidden` field that names a widget is shown, a button carries its action and its label, and is enabled only while `enabled_when` holds. | `core/tests/p3_widgets.rs` |
| W03c | 3.14 | ready | A button whose field is disabled cannot be pressed, and one with no action hands back nothing. | `app/widgets.rs` |
| W04 | 3.14 | ready | Path picker: a text box and a browse button; a path with spaces and a non-ASCII path are kept as typed. | `app/widgets.rs` |
| W05 | 3.14 | ready | A field naming a widget the registry does not have is a load diagnostic with the file and line. | `core/tests/p3_widgets.rs` |
| W05b | 3.14 | ready | A button says what a click does (`ui.action`, as `verb` or `verb:what`) and nothing else has an action: each mistake is an error at its own line. | `core/tests/p3_widgets.rs` |
| W05c | 3.14 | ready | The application's own registry is told the widgets it draws: `config check` reports one it does not have, with the line and the nearest name. | `cli.rs` |
| W06 | 3.14 | ready | A number with a hint (an optional one, with no default) is an empty box until it is typed into: only digits count, emptying the box takes the value away; a number with no hint is the drag box it has always been. | `app/widgets.rs` |

### G. The foreign-program guard (WP3.10, Q4)

A user-layer engine document may name any program; the application asks once, per document content, before it runs one it did not find.

| ID | WP | State | Case | Where |
|---|---|---|---|---|
| G01 | 3.10 | ready | An embedded document never asks; a program the engine's own `locate` strategies found, or one inside an installed runtime root, never asks. | `core/tests/p3_guard.rs` |
| G02 | 3.10 | ready | A user-layer document whose `launch.spawn.program` is none of those is refused with the program and the document's hash named, and nothing is started. | `core/tests/p3_guard.rs` |
| G03 | 3.10 | ready | After the person confirms, the hash is remembered (`values."app.confirmed_engines"`), survives a restart, and the plan goes ahead. | `core/tests/p3_guard.rs` |
| G04 | 3.10 | ready | Any change to the merged document changes the hash and asks again; a change of formatting or comments alone does not. | `core/tests/p3_guard.rs` |
| G05 | 3.10 | ready | A user patch that changes only field defaults of the embedded Ollama does not ask. | `core/tests/p3_guard.rs` |
| G06 | 3.10 | ready | Declined: the document stays refused, with no side effect. | `core/tests/p3_guard.rs` |
| G07 | 3.10 | ready | Through the window: the application's own document and a patch of a default start with no question; a patch that names another program asks and starts nothing until the answer; no leaves nothing remembered and asks again; yes is remembered by the hash in the settings file, the program runs, and after a restart the same document is not asked about. | `app/engine.rs` |

### X. What the documents say (WP3.9)

The privacy text, the licences and the declared hosts against what the application does and what was measured.

| ID | WP | State | Case | Where |
|---|---|---|---|---|
| X01 | 3.9 | ready | The privacy text says what ollama contacts what it writes and when. | `p3/docs.rs` |
| X02 | 3.9 | ready | The licences say ollama is mit and is not part of the package. | `p3/docs.rs` |
| X04 | 3.9 | ready | Every host an engine document declares is named in the privacy text. | `p3/docs.rs` |
| X05 | 3.7 | ready | The hosts a real pull contacted (a registry model and an `hf.co/` reference, as recorded under a logging proxy) are declared by the ollama document, a name beginning `*.` covering the hosts under it, and the document declares nothing that was not seen. | `p3/docs.rs` |
| X06 | 3.8 | ready | A declared host with a star covers the hosts under it and only those. | `p3/docs.rs` |

### C. The fakes against the real thing

A fake of a program is only as good as its likeness to the program. `crates/quickai/tests/fixtures/ollama/record.sh` records what a real Ollama v0.35.1 answers; C01 replays every recorded request against the Rust fake and compares the answer by shape; `fake_ollama.py` in the GUI suite's repository does the same (`test_fake_ollama.py`, FO01); C02 re-records with a real binary to renew the recordings; C03 and C04 pin the facts about a real pull and the real error sentences that the code is written around. C01 found one difference on its first run: asking again for a model that is there answers with every layer complete at once, not with the layers announced again.

| ID | WP | State | Case | Where |
|---|---|---|---|---|
| C01 | - | ready | The rust fake answers every recorded request with the shape the real one did. | `p3/contract.rs` |
| C02 | - | manual | A real ollama still answers in the recorded shapes. | `p3/contract.rs` |
| C03 | - | ready | The recordings show what a real pull looks like. | `p3/contract.rs` |
| C04 | - | ready | The recordings name the errors the application will show. | `p3/contract.rs` |

### N. llama.cpp must not change

Phase 3 touches the supervisor, the installer, the feed and the proxy that llama.cpp runs through.

| ID | WP | State | Case | Where |
|---|---|---|---|---|
| N01 | - | spec | The llama.cpp goldens are unchanged: `launch_matrix.json`, `settings_default.json`, `settings_populated.json`, `form_llamacpp.json`, `ui_texts.json`; the real-spawn proof still matches the matrix. | — |
| N02 | - | spec | The 25 pixel snapshots are identical to the untouched baseline except the Settings states, which differ only by the added Engine row (reviewed by a diff of the regions). | — |
| N03 | - | spec | The default GUI run still has the baseline result (202 tests: 194 passed, 8 skipped), and the 47 pending cases pass once their tag is removed. | — |
| N04 | - | spec | Every `cfg` branch compiles for Windows and macOS: `quickai-core` is checked here for `x86_64-pc-windows-gnu`; the application crate by CI. | — |
| N05 | - | spec | `cargo test --workspace` uses the network only in the tests that say so, and those pass when GitHub's anonymous allowance is used up (R08, the llama.cpp feed test). | — |

**Counts** (`ready` / `red` / `manual` / `spec`): 224 / 0 / 2 / 5 (total 231). The test kit has 26 tests of its own and the Python fake
19, none of them in the table (they are tests of the tests); the ready tests of the table are the ones that
describe the application.

## 5. The GUI cases

In the test repository (`quickai_base/test`, [docs/15_ollama.md](../../test/docs/15_ollama.md)); written
now, tagged `pending`, left out of a default run until the work lands, then un-tagged one suite at a time and
calibrated against the real window.

| Suite | Cases | What |
|---|---|---|
| `engine_selection` | TC-ENG-001 to -009 (9) | llama.cpp is the first-run engine and a llama.cpp user's file keeps its nine keys; choosing Ollama is saved as `values."app.engine"` and un-chosen again; the row is locked while running; Ollama not installed; its own settings; no chat page; an unknown engine id falls back and is kept; llama.cpp never touches Ollama |
| `ollama_lifecycle` | TC-OLL-001 to -018 (18) | Start with the document's environment; the model loaded before Running; the log's first lines; Stop unloads and ends the daemon **and its runner**; attach to one already running (and leave it); spawn mode refuses a busy port; something that is not Ollama on the port; a daemon that dies; a model that cannot be loaded; closing the window and killing the app; SIGTERM ignored; management mode; live traffic; context, GPU and environment lines; the model store; the API panel |
| `ollama_models` | TC-OMD-001 to -007 (7) | listing, pulling with progress, a failing pull (inside a 200), what is not a model name, an `hf.co` reference, delete with confirmation, nothing before a click |
| `ollama_runtime` | TC-ORT-001 to -009 (9) | nothing looked up before the click; the newest stable release first; install whole; a wrong checksum and a cut-off archive install nothing; a pasted address; a rate limit; delete; CUDA left out |
| `network_privacy` | TC-NET-019 to -022 (4) | idle with Ollama chosen; running and using it; loopback and `OLLAMA_NO_CLOUD=1`; the look-up contacts `api.github.com` and nothing else |

## 6. The gate of these tests

- [ ] every `red` row is `ready`: no test is ignored with "phase 3" (`grep -rn 'ignore = "phase 3' crates` prints nothing)
- [ ] every `spec` row has become a test and passed
- [ ] the `manual` row (C02) was run against a real binary for the release the document pins, and the recordings are renewed if the shapes moved
- [ ] the rows of N hold: the llama.cpp goldens, the pixel comparison with the Settings states reviewed, the default GUI run at its baseline
- [ ] the 47 pending GUI cases lost their tag and pass, calibrated, one suite at a time
- [ ] a manual run with a real Ollama on this machine: install, pull a small model, start, chat, stop, and `ps` shows no orphan (the gate of `implementation-plan.md` §8)
- [ ] the real-network tests still pass when GitHub's anonymous allowance is used up, and nothing else in `cargo test` needs the network

## 7. What preparing these tests found

Facts that changed the plan or the design, found by looking at real data instead of the documentation; each is in
`engine-facts.md` (O17 to O23) and each has a row above.

1. **A feed does not list the newest stable release first.** GitHub's list of `ollama/ollama` has the release
   candidate v0.40.0-rc5 *above* v0.35.1; the first release is not the one to install (R03, and `prereleases = false`).
2. **A real pull's progress is not a clean sum.** `completed` is missing from many lines and never appears for a small
   layer, the big layer ends short of its total, a layer is announced three times before a byte has moved, and a
   model that is already there is reported complete at once. Progress is a tracker with rules, not a division (H06).
3. **A failed pull is an `error` line inside a 200 response**, so a status check does not see it (H08, C03).
4. **The installer is not atomic today.** An install killed 8 ms in leaves a directory that `list_installed` offers as
   a build with files missing (Z16); the leftovers `.part` and `.partial` are listed as builds (Z17); an archive with
   no executable leaves the extracted files behind (Z19b).
5. **The `tar` reader skips an entry that climbs out of the install directory without saying so**, and the install goes on
   (Z10): safe, but a hostile archive is installed as if it were whole. Links pointing outside are accepted (Z12) and
   so are hard links (Z13).
6. **The live-traffic proxy does not recognise Ollama's own API** (it classifies by `.../chat/completions`), and a reply cut
   off in the middle leaves the conversation open in the Live tab (D02 to D06).
7. **The planner does not know a model's reference** (`model.ref` is null; `ModelInfo` has no such field), and the
   document types do not hold the example: `install` demands a `binary` the Ollama example does not give (E17), and
   the example wrote `gpu.has('nvidia')` where the expression language has `has(gpu.vendors, 'nvidia')` (Z33).
8. **`tests/examples.rs` asserts the opposite** of what Phase 3 delivers: it checks that `runtime`, `models`, `traffic`,
   `api_info` and `diagnostic` are ignored sections (E20).
9. **The GUI suite's teardown would leak the daemon**: it kills the application's process group, and the daemon is in
   its own (K01); the Ollama suites tear down through the fake's pid files.
10. **The fake was wrong once, and the recordings said so** (C01): it announced the layers again for a model that was
    already there, where the real server reports each complete at once.
