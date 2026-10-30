# Reports and the community

Telemetry and crash reports the way VS Code does them, the admin of everybodyai.dev, and the shared configurations of
quickai.everybodyai.dev.

_Status: plan, 2026-10-30. Revised the same day: no signing anywhere, following VS Code. The decisions in
[§18](#18-decisions) were made by the user. Built since: the app's part ([§21](#21-built-in-the-app-2026-10-30)), and the
API and the admin, deployed ([§22](#22-built-on-the-server-2026-10-30)); the community is not built._

The user, 2026-10-30: "we need to track the usage of the quickai and also provide option for the user to provide crash
reports. everybodyai.dev (admin page) needs to be created and it has to have apis in place which accepts the
telemetry/crash reports by the app. We also need a subdomain quickai.everbodyai.dev to have configuratiosn shared by our
users to help each other out - which is categorized and rateable too. We also need to change the privacy policy and so on
as well to incorporate this change. When a report is provided to us, we need to ensure that it is trusted, some mechanism
of signing/verification is needed here [...] I may have missed many aspects, think about them and prepare an
implementation documentation". Then: "also need to ensure that the configuration that is sent - e.g. he same cfg sent by
many people - we have to retain only one - this would mean taking care of the order of arguments etc". Then: "ignore the
signing idea - is there an alternative - or drop the idea if its not a standard one - we will follow similar to vscode".

The host is written `quickai.everybodyai.dev` below (the request has `everbodyai`).

## 1. In short

- **VS Code's model.** One setting, *Telemetry level*: **All** (usage, errors, crashes), **Errors and crashes**,
  **Crashes only**, **Off**. A list of every event QuickAI can send (`quickai --telemetry`, and a generated reference
  page). A local log of exactly what was sent. A policy and an environment variable that turn it off for good, and
  builds without it (as VSCodium is VS Code without telemetry).
- **Off until the person chooses.** The first start shows a notice that asks for a level. Nothing is sent until it is
  answered, and closing it means Off (decision 1). At Off, after a crash, the app asks once whether to send that one
  report and shows it first (decision 2). At *Crashes only* or above, crash reports are sent by themselves, as in VS Code.
- **No signing.** Telemetry endpoints are public everywhere: VS Code's collector, Sentry's DSN ("safe to keep public"),
  and the rest. Reports are treated as untrusted input. The server checks:
  - their size and schema;
  - that the version is a released one (or says it is a source build);
  - rate limits per install and per network, with the address held in memory only;
  - a block list.

  The dashboards count distinct installs, so fake reports can skew a chart but cannot do harm (§8). Downloads from us
  rely on HTTPS and a SHA-256 in the list. Builds are proved by platform code signing (Authenticode, Apple Developer
  ID), which is a release task of its own.
- **One host for the app: `api.everybodyai.dev`.** The admin is `admin.everybodyai.dev`, behind Cloudflare Access. The
  community site is `quickai.everybodyai.dev`. All of it runs on Cloudflare (Workers, D1, R2), as the site does now. The
  code is public, in a new repository `everybodyai/services` (decision 10).
- **Count, don't collect.** Events carry ids of things and coarse buckets: the version, the engine, whether a model
  started, memory in steps, the GPU's maker. Never prompts, replies, file names, paths, search words or an IP address.
  The country is kept as two letters (decision 7).
- **Community configurations** are QuickAI's own configuration files, sorted by use, engine and hardware.
  - **Rated** with stars by people signed in with GitHub, and *worked on N computers like yours* from installs at level
    All.
  - **Moderated** at launch, under CC0. Configurations for adult image models are not allowed (decision 11).
  - **Reviewed on import.** The app reviews a downloaded file as it reviews any imported one.
  - **Kept once.** Two files that run the same thing are one configuration, whatever the order of their arguments,
    environment lines or keys. The server keeps one copy, found by the hash of a canonical form (§11.11).
- **The person can see, download and delete what we hold**, from Settings, by the install id. VS Code says it cannot do
  this, for want of a reliable id; a random install id makes it possible.

## 2. What was asked, and what it needs besides

| Asked | What it takes | § |
|---|---|---|
| Track usage | Events at level All, batched, with a schema the server enforces | 5, 6 |
| Crash reports | A panic hook and an unclean-exit marker. Engine failures are error events, because they are the crashes people actually meet | 7 |
| Admin page and APIs | `api.everybodyai.dev/v1` (Worker, D1, R2) and `admin.everybodyai.dev` behind Cloudflare Access | 9, 10 |
| `quickai.everybodyai.dev`, categorized and rateable | Server-rendered pages, GitHub sign-in, categories, stars, "worked on", moderation, browse and share from the app | 11 |
| The same configuration shared by many is kept once | A canonical form of what a configuration runs (the order of arguments, environment lines and keys, quoting, defaults) and its hash | 11.11 |
| Privacy policy | PRIVACY.md, the site's policy, community terms, SECURITY.md, About, Help, store listings | 13, App. A |
| Trusted reports | The standard way, without signing: validation, known versions, rate limits, block lists, analysis that resists junk | 8 |

Not asked, but needed for it to work or to be lawful:

1. **Consent done properly.** Nothing is sent before the person answers. Saying no is as easy as saying yes. The answer
   is tied to a version of the policy and asked again when what is collected grows. The word "anonymous" is never used:
   an install id makes the data *pseudonymous*.
2. **Access and deletion without accounts**, by the install id, from the app (§12).
3. **Switches on the server.** They stop collecting a level, or stop an old version from sending, without a release
   (§9).
4. **Off for good** for packagers (AUR, Snap, Homebrew), organizations under the commercial licence, and `quickai-run`
   on servers (§5.4).
5. **Symbols.** Release builds strip symbols today (`strip = "symbols"`), so a backtrace is only addresses. They should
   keep function names (§7.5).
6. **Engine failures.** llama-server, Ollama or ComfyUI dying during a load is QuickAI's most common "crash" (the
   ComfyUI CPU crash with GPU=auto is one). They are error events.
7. **Report a problem.** VS Code's issue reporter opens a GitHub issue with the system's details filled in, which the
   person reads before posting. QuickAI does the same, and nothing of it goes to our server (§7.6).
8. **Hardware classes.** Telemetry, community filters and "computers like yours" share one coarse vocabulary, coarse
   enough not to fingerprint anyone (§6.4).
9. **Moderation, a content policy, a licence for what is shared (CC0) and terms of use** (§11.10).
10. **Secrets and paths in shared files.** The server checks again for what the app's export leaves out; a person can
    still type a token into a free-text field (§11.6).
11. **A withdrawn configuration.** One found unsafe after people imported it is marked in the list, and the app shows a
    note on it (§11.9).
12. **Cloudflare's daily limits are per account.** A flood of reports on the free plan would also stop the waitlist,
    the contact form and the community site, so the plan is Workers Paid (decision 8, §19).
13. **The waitlist and contact messages** are already in D1 with no page to read them. The admin shows them.
14. **Giving back.** A public page of aggregate numbers, showing only buckets of at least 20 installs (decision 9).
15. **Reports are never used to check a licence.** The policy says so.

## 3. Rules

1. **Nothing is sent above the level the person chose**, and nothing at all before they choose. Crash files are written
   locally, as `ui-stalls.log` is today, and stay local unless the level or the person sends them.
2. **What can be sent is listed, and what was sent can be read.** `quickai --telemetry` lists every event and field. The
   telemetry log keeps what was sent, as it was sent.
3. **Count, don't collect.** Every event and field is in a schema, with its level, its kind of data and its purpose
   (VS Code's annotations do the same). The server refuses anything else. There is no free-form bag of properties.
4. **One host.** The app talks to `api.everybodyai.dev` and to no other EverybodyAI host. PRIVACY.md's list of hosts
   grows by one.
5. **Nothing blocks the window.** Sending runs on a background thread with short timeouts. A failed send waits and
   tries again later.
6. **Off for good is possible** (§5.4). A build without the feature has no sending code at all.
7. **Separate data.** Telemetry and the community are in separate databases. The community side holds a keyed hash of
   the install id, not the id, so the two cannot be joined by a query.

## 4. The pieces, and where they run

```
 QuickAI (window)  ─┐   HTTPS, JSON
 quickai-run       ─┼─────────────────►  api.everybodyai.dev  (Worker "api")
                    │                         │             │
                    │                         ▼             ▼
                    │                  D1 "telemetry"   R2 "telemetry" (crash bodies)
                    │                  D1 "community"
                    │                         ▲
 browser ──────────────► quickai.everybodyai.dev (Worker "community", Astro, server-rendered)
                         GitHub sign-in, ratings, sharing
 browser ──────────────► admin.everybodyai.dev (Worker "admin", behind Cloudflare Access)
                         dashboards, crashes, moderation, versions, deletion, switches, the site's forms
 daily: GitHub Releases of everybodyai/quickai ─► the list of released versions
```

| Host | What | Who reaches it |
|---|---|---|
| `everybodyai.dev` | the site, as now (static, with the forms Worker) | everyone |
| `api.everybodyai.dev` | the app's API, `/v1/...` | the app |
| `quickai.everybodyai.dev` | QuickAI's own site (live since 2026-10-30, built in `everybodyai`), and the community configurations under `/configurations` | everyone; signing in to rate or share |
| `admin.everybodyai.dev` | the admin | the owner and moderators, through Cloudflare Access |

**Repositories.** The site stays in `everybodyai`. A new public repository, **`everybodyai/services`**, holds:
- `api/`, `community/` and `admin/`, one Worker each;
- `shared/`: the event schemas, the D1 migrations and the test vectors.

It is public so that anyone can check what the server keeps (decision 10). QuickAI's site already answers on
`quickai.everybodyai.dev` (its Worker `quickai-site`, from the `everybodyai` repository). It passes `/configurations*` on
to the community Worker through a service binding, as it passes nothing else, so one hostname serves both. Its licence is QuickAI's unless you choose
otherwise. The admin's code being public is fine: Access protects it, not secrecy. The app's side is in
`quickai-public`, and the release side is in `quickai_base`.

## 5. Telemetry levels

### 5.1 The setting

*Settings → Privacy → Telemetry*, the four values of VS Code's `telemetry.telemetryLevel`:

| Level | Sends |
|---|---|
| **All** | crash reports, error events and usage events |
| **Errors and crashes** | crash reports and error events |
| **Crashes only** | crash reports |
| **Off** | nothing (after a crash the app offers once to send that report, §7.4) |

Settings key `telemetry.level`. It is absent until the person answers, which counts as Off. `telemetry.asked` records
the policy version they answered. A person who is never asked keeps a settings file without either key.

### 5.2 The first-start notice

The first start shows a notice in the main window, beside *Add a model* and not over it:

> **Help make QuickAI better?** QuickAI can send EverybodyAI reports of crashes and errors and, if you like, counts of
> what it did, with a rough description of this computer. Never your chats, files or searches.
> **[All]** **[Errors and crashes]** **[Crashes only]** **[Off]** · [What is sent] · [Privacy policy]

The four buttons have equal weight and nothing is selected beforehand. Closing the notice means Off. When a later
version collects something new, people who chose a level above Off are asked again; people who chose Off are not.

### 5.3 Seeing what is sent

- **`quickai --telemetry`** prints, as JSON, every event the build can send: its level, and each field with its type,
  kind of data and purpose (VS Code's `code --telemetry`). The same list is the generated page
  `docs/reference/telemetry-events.md` (`quickai config docs telemetry`, like the other reference pages).
- **The telemetry log.** Every batch, exactly as sent, is appended to `~/.quickai/telemetry/telemetry.log` (the last
  1 MB). *Settings → Privacy → Show what is sent* opens it in a window of its own (VS Code's *Developer: Show
  Telemetry*).
- **The install id.** It is shown with a copy button, and Settings says not to post it in public. There are also
  *Download what EverybodyAI holds*, *Delete what EverybodyAI holds* and *New install id*.

### 5.4 Off for good

In order of strength:

1. **A build without the cargo feature `telemetry`**, as VSCodium is VS Code without telemetry. Packagers build with
   `--no-default-features --features remote-setup`. The Privacy page then says "This build of QuickAI sends no
   telemetry."
2. **A policy in the package's configuration layer** (`<exe>/config/`), `telemetry_level = "off"` or a lower cap. It
   greys the setting out with "Set by whoever installed QuickAI" (VS Code's enterprise policy for the telemetry level).
3. **The environment.** `QUICKAI_TELEMETRY=off|crash|error|all` caps the level, and `DO_NOT_TRACK=1` means off.

**`quickai-run`** never asks and sends nothing unless its configuration or `--telemetry <level>` says so. It writes
crash files locally, and `quickai-run telemetry list|show|send|delete` handles them.

## 6. What is sent

### 6.1 Events

Events are recorded as they happen and sent in batches (VS Code's client batches too). Every batch carries the
**common properties** once:

| Property | Example | Kind |
|---|---|---|
| `install` | random UUID, made the first time anything is sent | pseudonymous id |
| `session` | random UUID per start, never stored | pseudonymous id |
| `app` | `{ version: "0.3.0", channel: "official", commit: "b978bde", target: "x86_64-unknown-linux-gnu", package: "appimage" }` | system metadata |
| `os`, `os_version`, `arch` | `linux`, `24.04`, `x86_64` | system metadata |
| `memory_gb`, `gpus` | `32` (bucket), `["amd"]` | system metadata |
| `language` | `en` (the language only, no region) | system metadata |
| `level` | `all` | setting |

The server adds the country from Cloudflare's `request.cf.country`. The address itself is never stored.

The install id is random, not made from the hardware: VS Code's `machineId` is a hash of a network card's address, and
QuickAI deliberately does not do that. `channel` is `official` when the build scripts set `QUICKAI_CHANNEL=official`,
and `source` otherwise.

The events (the full list is the schema in `quickai-core`; times are rounded down to the hour):

| Event | Level | Fields | Purpose |
|---|---|---|---|
| `crash.panic` | crash | `message` (cleaned, §7.3), `location`, `thread`, `backtrace` (function names), `state` (engine, its state, open windows, minutes open in a bucket) | health |
| `error.engine_exit` | error | `engine`, `build`, `phase` (installing, loading, serving), `status` ("exit 1", "signal 9"), `after_seconds`, `model` (§6.3), `changed` (ids of the engine's fields that differ from the default; values only for numbers and choices) | health |
| `error.download` | error | `what` (model, build, program), `source` (huggingface, github, ollama, pypi, pytorch, other), `reason` (checksum, network, disk_full, http_4xx, http_5xx) | health |
| `error.install` | error | `program`, `step`, `reason` | health |
| `error.stall` | error | `seconds` (bucket), `during` (install, download, delete, look-up) | health |
| `session.unclean_exit` | error | none; sent at the next start | health |
| `session.start` | all | `first_run` | feature insight |
| `session.end` | all | `minutes_open` (bucket) | feature insight |
| `window.open` | all | `window` (add_model, recommended, settings: page, help: page, configurations, community, chat) | feature insight |
| `feature.use` | all | `feature` (live_traffic, remote_run, keep_chats, open_webui, configuration_import, configuration_export, …) | feature insight |
| `model.add` | all | `how` (recommended, lookup, link, import, pull), `model` (§6.3) | feature insight |
| `run.start` | all | `engine`, `build`, `model`, `where` (local, remote), `configuration` (a community id and version, or none) | feature insight |
| `run.ready` | all | `engine`, `model`, `seconds` | performance |
| `community.outcome` | all | `config`, `version`, `worked` | feature insight |

`run.start`, `run.ready` and `error.engine_exit` together are the most useful data: which engines, builds and models
start, how often they fail and on what hardware. They are the evidence for the recommended list (WP4.40 was done by
hand). `tokens_per_second` waits until the app measures it itself.

### 6.2 Never sent

- Prompts, replies, chats, pictures.
- File and folder names or paths.
- Words typed into a search or a box, and URLs typed.
- Host names, user names, the addresses of remote computers.
- The names of models that are neither on the recommended list nor in a public repository.
- The text of command-line arguments or environment lines.
- The log.
- The contents of a configuration.
- Anything from Open WebUI's or ComfyUI's pages.
- Exact memory, exact times.

### 6.3 Which models are named

A model is named as:
- `catalog:<id>` when it is on the recommended list;
- `hf:<owner>/<repo>` when `model-sources.json` says it came from a Hugging Face look-up (a public search result);
- `ollama:<name>` when it is in Ollama's public library.

Anything else is `other`, with a size bucket in GB (`0-2`, `2-5`, `5-10`, `10-20`, `20+`). A file a person made or renamed
is never named.

### 6.4 The hardware class

One vocabulary for telemetry, the community filters and "computers like yours". It is computed in `quickai-core` from
what the app already reads (`gpu::detect`, `total_ram_bytes`):

```
<os>-<arch>-<gpu makers, sorted, or "cpu">-<memory bucket>        e.g. linux-x86_64-amd-32gb, macos-aarch64-apple-16gb
memory buckets: 8gb (up to 12), 16gb (up to 24), 32gb (up to 48), 64gb (up to 96), 128gb+
```

It has no GPU model, driver, screen or CPU model: they would add little and fingerprint a lot. VRAM can be added as a
bucket once the app reads it.

### 6.5 Batching and sending

- **The queue.** Events are queued in `~/.quickai/telemetry/queue/`, at most 1 MB, with the oldest dropped first.
- **When it is sent.** A batch goes when 100 events are queued, at the end of a session, or every 6 hours while the
  app is open. The first send of a start waits until the app has been open two minutes.
- **How.** One request per batch, from a background thread with a 15-second timeout.
- **Retries.** A failed batch waits 1 hour, then 6, then 24, and is dropped after 3 days.
- **Idempotent.** A batch has its own id, so sending it again changes nothing.
- **The level filters at the source.** An event above the level is never queued. A lowered level empties the queue of
  what it no longer allows.

## 7. Crash and error reports

### 7.1 Capturing

- **The panic hook**, installed first thing in `main`, writes `~/.quickai/telemetry/crashes/<id>.json` atomically, at
  most 64 KiB, with no network and nothing slow, then calls the default hook. The hook must not panic.
- **A session marker**, `~/.quickai/telemetry/running` (the pid, the version and the start time), is written at start
  and removed on a clean exit. If the next start finds it, its pid is gone and no panic file names that session, the
  session ended uncleanly: a native crash, the OOM killer, a forced kill or a power cut. This is sent as
  `session.unclean_exit`, never asked about, since asking after every power cut is noise.
- **Engine exits.** The runtime already knows the exit status and the phase. Any exit not caused by the app's Stop is an
  `error.engine_exit`.

### 7.2 What a crash report holds

```json
{
  "schema": 1,
  "id": "7d4c2f0e-9a71-4c1b-8d22-0b6f1e2a3c4d",
  "event": "crash.panic",
  "time": "2026-10-30T09:00:00Z",
  "common": { "...": "the common properties, §6.1" },
  "panic": {
    "message": "called `Option::unwrap()` on a `None` value",
    "location": "crates/quickai/src/app/catalog_view.rs:412:31",
    "thread": "main",
    "backtrace": ["quickai::app::catalog_view::CatalogView::show", "quickai::app::shell::…", "…"]
  },
  "state": { "engine": "ollama", "engine_state": "running", "open_windows": ["recommended"], "minutes_open": "30-60" }
}
```

A crash report sent by the level never carries a log. A log line can hold a prompt when an engine runs verbose, and
nobody reads it before it goes.

### 7.3 Cleaning

Cleaning is pure, in `quickai-core`, and held by a table test. It runs before anything is queued or shown:

| Found | Becomes |
|---|---|
| the home directory in any spelling (`/home/name`, `/Users/name`, `C:\Users\name`, escaped `\\`) | `~` |
| the user name, the host name | `<user>`, `<host>` |
| addresses of remote computers in the settings; IPv4 and IPv6 addresses other than loopback | `<remote>`, `<ip>` |
| email addresses | `<email>` |
| a URL's query string | removed (tokens live there) |
| a value whose variable name is a secret's (`plan::is_secret_name`, the rule configurations already use), and known token shapes (`hf_…`, `ghp_…`, `github_pat_…`, `sk-…`) | `<secret>` |
| a model file name that §6.3 would not name | `<model file>` |

### 7.4 At Off: asking once

At level Off, the next start after a panic opens a window of its own (C194):

> **QuickAI closed unexpectedly last time.** A report can help us fix it. It holds the error, where in QuickAI it
> happened, the version and a rough description of this computer, never your chats or files.
> [View the report] **[Send this report]** **[Don't send]** · [Change the telemetry level]

*Send this report* sends that one report and nothing else, using a new install id if none exists yet. *Don't send*
keeps the file in `crashes/`, where it can be attached to a GitHub issue. Crash files older than 90 days are deleted. At
*Crashes only* or above, there is no window: the report is sent at that start and shows in the telemetry log, as in VS
Code.

### 7.5 Reading a crash

- **Function names in release builds.** Change `[profile.release] strip = "symbols"` to `strip = "debuginfo"`, so that
  backtraces carry function names. A panic's `file:line:col` is exact already. The size cost is measured first (spike
  S-R2). Line numbers in every frame would need split debug info per target, kept privately by build; that comes later,
  if names prove too little.
- **Grouping.** A fingerprint is a SHA-256:
  - for a panic: of the event, the location without its column, and the first three frames inside QuickAI's crates;
  - for an engine exit: of the engine, the phase, the kind of status and the build's backend.

  The admin lists the groups with counts, distinct installs, versions, first and last seen, and a status (new, known,
  fixed in a version, the GitHub issue).
- **Plausibility.** The QuickAI functions of a backtrace must exist in that version: the release keeps the list of its
  function names. A report whose frames are not functions of its version is marked as such.

### 7.6 Report a problem

**Help → Report a problem…** works like VS Code's issue reporter and sends nothing to our server:

1. A window of its own asks for a title, what happened, and what was expected.
2. It adds a *System information* block: the version, the OS, the hardware class, the engine and build, and the
   location of the last crash if there was one. The person can read and edit it.
3. **Open on GitHub** opens `github.com/everybodyai/quickai/issues/new` with the title and text filled in. A text too
   long for an address is copied to the clipboard instead, and the page opens with a note to paste it.

### 7.7 Left out: memory dumps

Minidumps would catch native crashes, such as a GPU driver taking the window down, and VS Code's Crashpad sends them.
They also carry stack memory, which can hold what the person typed, so they are left out. If `session.unclean_exit`
shows many native crashes, a separate decision can add dumps that hold only registers and the list of modules.

## 8. Trust, the standard way

### 8.1 Why there is no signing

Telemetry is accepted from public endpoints everywhere:
- VS Code sends to a collector whose key is in the product's public configuration;
- Sentry says its DSN is "safe to keep public because [it] only allow[s] submission of new events"
  ([Sentry](https://docs.sentry.io/concepts/key-terms/dsn-explainer/)).

An open-source app cannot keep a signing key secret, and no desktop platform proves to a server that an app outside its
store is untouched. So the standard answer is to treat every report as untrusted input, check it, limit it, and read the
data in ways that junk cannot bend far. That is the design here. Nothing a report says can start, change or open
anything; it is only read.

### 8.2 What the server does with every request

1. **HTTPS only**, with HSTS on the domain.
2. **Size.** A batch is at most 64 KiB and 500 events; a crash report at most 256 KiB. Otherwise 413.
3. **Schema.** Each event is checked against its name's schema for that schema version. Unknown events and fields are
   refused. Otherwise 400, and the app drops the batch because it will never pass.
4. **Version.** A daily job reads the releases of `everybodyai/quickai` from GitHub into `versions`. A report from a
   released version with channel `official` is stored as **official**, one with channel `source` as **source**, and one
   that claims to be official but whose version was never released as **unknown**.
5. **Rate.** The Workers rate-limit binding, keyed by install and by the /24 or /48 network of the address (the address
   is held in memory only). The binding works per Cloudflare location and eventually catches up: it guards against
   abuse, and does not count exactly ([docs](https://developers.cloudflare.com/workers/runtime-apis/bindings/rate-limit/)).
   Otherwise 429 with `Retry-After`.
6. **Block list.** Networks and installs can be blocked in the admin, as Sentry blocks addresses. Otherwise 403, and
   the app stops sending for 30 days.
7. **Switches.** A version or a level the status no longer collects gets 410, and the app stops sending until it is
   updated.
8. **Stored with tags**: official, source or unknown, and the install's age in days.

### 8.3 Reading the data so that junk cannot bend it

- Dashboards count **distinct installs**, not events, and leave out installs less than a day old and unknown builds,
  unless asked otherwise.
- A day or a network that sends far more than usual is flagged.
- A crash's frames are checked against its version's functions (§7.5).

What remains possible: someone can write a script that sends plausible reports from many installs. It costs us skewed
charts, never a security problem. The flags make it visible, the block list and the switches stop it, and nothing pays
out because of the data.

### 8.4 What the app downloads from us

The community list and configurations come over HTTPS. The list holds the SHA-256 of every configuration version, and
the app checks a downloaded file against it. That catches a broken or stale copy. Then the app shows **the import review
it already has**, as for any file from outside.

VS Code's Marketplace signs extensions because an extension is code that runs. A configuration is data, and the person
reviews it before it is used. If configurations ever carry something that runs without that review, signing them is
the standard answer and comes then.

### 8.5 Builds

How a person knows a QuickAI binary is ours is a release matter, separate from telemetry: Windows code signing
(Authenticode) and an Apple Developer ID with notarization. Both are on the release list already; macOS is signed ad hoc
today. `SHA256SUMS` stays on every release, as now.

## 9. The API: `api.everybodyai.dev/v1`

| Method and path | What | Answer |
|---|---|---|
| `POST /v1/events` | a batch: the common properties and the events (§6) | 202 |
| `POST /v1/crash` | one crash report (§7.2) | 202 |
| `GET /v1/me` | everything held for the install named in `QuickAI-Install`, as JSON | 200 |
| `DELETE /v1/me` | delete everything held for that install, now | 200, with a receipt |
| `GET /v1/status` | the levels collected, the lowest version taken, the server's time | 200 |
| `GET /v1/community/index` | the published configurations (§11.9) | 200 |
| `GET /v1/community/configs/{id}/{version}` | one configuration's TOML | 200 |
| `POST /v1/community/drafts` | a configuration the person wants to share; answers with a claim address valid for 24 h | 201 |

**The install id** travels in the header `QuickAI-Install`, never in the path, so that no log of addresses holds it.
For `me` the id is the proof: it is 122 random bits that only the app and our database know.

**The community's requests carry no install id and no identifying header.** Browsing and fetching are not tied to
anyone, and downloads are counted per configuration only. Drafts are limited per network. The app's `User-Agent` stays
`quickai/<version>`, as for its other requests.

**Answers.**
- 400: the schema refused it, so the batch is dropped.
- 403: blocked, so the app waits 30 days.
- 410: not collected, so the app stops until it is updated.
- 413: too big.
- 429: wait for `Retry-After`.
- 5xx: retried as in §6.5.
- An error body is `{ "error": "<code>", "message": "<text for a person>" }`.

**Inside the Worker:**
- **Storage.** A batch is stored as one row, its JSON, so one request is one write. The nightly job extracts runs and
  crash rows and folds everything into `rollups`. A crash's body goes to R2 under `crash/<install>/<id>.json`, so
  deleting an install is one list and one delete.
- **Budget.** The free plan's limit is 10 ms of CPU per request
  ([limits](https://developers.cloudflare.com/workers/platform/limits/)). Checking a schema fits it; the paid plan
  (decision 8) takes the limit away anyway.
- **Logs.** Nothing logs a body, a header or an address with `console.log`. Workers Logs, which is on for the site's
  Worker, must be checked for what it records about requests (spike S-R3), and it is off for `api` unless that is
  nothing personal.
- **Cron jobs**, each night:
  - read GitHub's releases;
  - fold the day into `rollups`;
  - apply the retention of §12;
  - make the community list again;
  - email the owner about new crash groups and spikes, through the `MAILER` binding the site already has, to its
    verified address.

## 10. The admin: `admin.everybodyai.dev`

**Access.** A Cloudflare Access application covers the whole host. It is free for teams under 50 users. The policy names
the owner's email, and later the moderators'; sign-in is with GitHub or a one-time PIN. The Worker also verifies the
`Cf-Access-Jwt-Assertion` header (with `jose`, against the team's certificates and the application's audience tag) and
refuses a request without it, so a route set up wrong still exposes nothing. Roles (`owner`, `moderator`) are kept in D1
by Access email, and every change is written to an audit log.

| Page | What |
|---|---|
| Overview | installs reporting today, this week and this month; new installs; versions; levels chosen. Always with "*n* installs reporting": it is a sample of those who said yes |
| Systems | OS, architecture, memory buckets, GPU makers, languages, countries (buckets under 20 installs shown as "other") |
| Engines and models | runs, failure rate and time to ready by engine, build, model and hardware class: the evidence for the recommended list |
| Features | what is opened and used |
| Errors | downloads, installs and stalls, by reason, source and version |
| Crashes | groups with counts, versions and status; a report's page (the cleaned report, backtrace, tags, the group's other reports); a button that opens a GitHub issue with no personal data |
| Community | the moderation queue, suggested merges and edits (§11.11), flagged configurations and reviews, users (limit, ban), categories |
| Versions | the released versions read from GitHub, and the lowest version taken |
| Installs and blocks | look up an install (for a deletion asked by email); block an install or a network |
| Switches | the levels collected, retention |
| Site | the waitlist and contact messages of the site's D1 |
| Audit | who changed what |

The pages are server-rendered by the Worker with small inline charts drawn from `rollups`. That needs no front-end
framework and keeps the CSP as strict as the site's.

## 11. The community: `quickai.everybodyai.dev`

### 11.1 What a visitor sees

The community lives under `/configurations` on QuickAI's site, and the site's header gains a link to it.

- **Home** (`/configurations`): a search box, the categories, *Popular this month*, *New*, and *Fits computers like…*
  (pick a hardware class).
- **A category** (`/configurations/c/coding`): a list with filters (engine, hardware, system, *no custom arguments*) and a sort (rating,
  worked on, newest).
- **A configuration** (`/configurations/vision-chat-16gb`) shows:
  - its title, author, summary and categories;
  - badges: the engine; *Sets its own command-line arguments*, shown prominently when it does; *Reviewed by
    EverybodyAI*; *Also shared by N people*;
  - what it runs: the models with sizes, sources and licences, and the build;
  - its settings, by the engine documents' names, with free text shown in full;
  - *Worked on* by hardware class, the stars and reviews;
  - its variants, and its versions with what changed;
  - **Download** (the `.toml`), and *How to use it in QuickAI*.
- **Share** (signed in): upload a `.toml`, or finish one shared from the app.
- **Me**: my configurations, my ratings, delete my account.
- **Guidelines**, **Terms**.

The pages are server-rendered (Astro with the Cloudflare adapter), so that a search for "llama.cpp settings 8 GB" finds
them. They use the site's tokens and IBM Plex, and carry no analytics, like the site.

### 11.2 Categories

| Axis | Values | Set by |
|---|---|---|
| Use | Chat, Coding, Reasoning and maths, Pictures in (vision), Pictures out, Video, Documents and search (Open WebUI), Embeddings, Small and fast, Long context | the author (one to three); a moderator can change them |
| Engine | llama.cpp, Ollama, ComfyUI (+ Open WebUI beside) | read from the file |
| Hardware | the classes of §6.4, folded to maker and memory | the file's `made_on`, then the "worked on" results |

Categories are rows in D1: adding one is an admin action, not a release.

### 11.3 Ratings and "worked on"

- **Stars** (1 to 5) and an optional review of up to 1000 characters.
  - Rating needs a signed-in account: one rating per account and configuration, which its author can change.
  - Authors cannot rate their own, and a moderator can hide a review.
  - The list sorts by a Bayesian mean: with the site's mean `m` and `C = 5`, score = (C·m + Σ stars) / (C + n). One
    5-star rating does not top the list.
- **Worked on.** When an install at level All starts a configuration from the community, the next batch carries
  `community.outcome`: whether the engine reached ready (decision 12). The server counts one result per install and
  configuration, by hardware class, and the latest result wins: "worked on 37 computers, 9 like yours". It needs no
  account, and it is shown apart from the stars, never mixed in.
- **Abuse**:
  - accounts younger than 7 days can rate but are not counted until then;
  - rate limits per account;
  - Turnstile on the forms;
  - a spike of ratings for one configuration is flagged in the admin.

### 11.4 Accounts

- **GitHub sign-in** (decision 4), through an OAuth app owned by the `everybodyai` organization. It asks for no scope,
  which gives the public profile only. Kept: GitHub's numeric id, login, display name and avatar address.
- **Sessions.** A random 32-byte session id, stored hashed, in the cookie `__Host-session` (HttpOnly, Secure,
  SameSite=Lax, 30 days). The cookie is strictly necessary, so there is no cookie banner. The site's policy changes from
  "no cookies" to "one sign-in cookie, only if you sign in on quickai.everybodyai.dev".
- **Email sign-in is left out.** The `send_email` binding the site uses sends only to verified addresses, so sign-in
  links would need another mail provider.
- **Deleting an account** removes the user, their ratings and reviews. It asks whether to remove their configurations
  too; otherwise those stay, shown as by "a former member" (they are CC0).

### 11.5 Sharing

- **From the app.** Configurations → a configuration → **Share with the community…**:
  1. The app computes the configuration's identity (§11.11). If the list already has it, the app says "Already shared
     as *…*", opens it, and uploads nothing.
  2. Otherwise it makes the file the export already makes (the allow-list; review fields only if ticked) and shows it
     in full, with "You'll finish on quickai.everybodyai.dev, signed in with GitHub".
  3. *Upload* sends it as a draft. The answer is a claim address with a one-time token, valid for 24 hours, which the
     app opens in the browser.
  4. There, the person signs in, adds a title, a summary and categories, and submits.

  A draft carries no account and no install id, and an unclaimed draft is deleted after 24 hours.
- **From the site.** Upload a `.toml` on *Share*.
- **A new version** of one's own configuration replaces nothing: it is version *n+1*, with a line saying what changed.

### 11.6 Checking a submission

1. At most 256 KiB, UTF-8, TOML, `kind = "profile"`, and a schema the server knows.
2. **The same checks as the app's import** (configurations.md §8): limits, names, the deny-list, values within their
   field's type and range, unknown settings. To keep one set of rules, `quickai-core`'s own check runs compiled to
   WebAssembly. Its dependencies are pure Rust (`regex`, `serde`, `toml`, `toml_edit`, `semver`, `sha2`) and it already
   does no I/O. Spike S-R1 checks that it builds for `wasm32-unknown-unknown` and fits a Worker. If it does not fit, the
   check runs at moderation through the native `quickai configuration` command.
3. **No secrets, no paths**, checked again. Values that look like a token or the path of a computer are refused, and the
   author is told which line.
4. **Public sources only.** Each model is a catalog id, an Ollama library name or a Hugging Face repository. At
   moderation, the admin checks the repository once (public, not gated) and shows its licence.
5. **No adult image or video models** (decision 11). Moderation refuses them, and the guidelines say so.
6. **`needs_review`.** Any value of a `review` field marks the version: free text that reaches a command line or an
   environment, or wider access. The page shows the badge.
7. **A duplicate is found by what it runs, not by its bytes** (§11.11).

### 11.7 Moderation

- **States**: `draft` → `pending` → `published` or `rejected`. A `published` version can become `withdrawn` (for
  safety) or `removed` (by its author).
- **Everything is moderated at launch** (decision 5). Later, a clean version (no `needs_review`) from an account in good
  standing could go out at once, shown as unreviewed until a moderator looks.
- **A report button** on every configuration and review feeds the flags queue.
- **Withdrawing.** A version withdrawn for safety leaves the list. The list then carries `withdrawn: [{id, version,
  reason}]`, so an app that imported it shows a note (§11.9).

### 11.8 Publishing

Publishing gives the version its number and puts it in the list with its SHA-256 and identity. A published version
never changes; a fix is a new version.

### 11.9 In the app

- **Browse.** Configurations → **Community…** opens a window of its own (C194).
  - It fetches the list only on that click, under PRIVACY.md's rule, and filters it on the computer. *Fits this
    computer* uses the local hardware class, so nothing about the computer is sent to browse.
  - A row shows the title, author, engine, stars, "worked on N like this one", and badges.
  - **Get…** fetches the file, checks its SHA-256 against the list, then runs **the import review the app already
    has**: the notes, and the free text shown in full and taken only when ticked.
- **Where it came from.** An imported configuration keeps `[meta] community = { id = "…", version = 3 }`, a new key of
  the `profile` schema. Its export keeps it, so a configuration made from another can say "based on".
- **The same as a community one.** The Configurations window says when any configuration's identity is in the list,
  with its stars and "worked on", whether it was fetched from the community or not.
- **Worked or not.** At level All, the first start of a configuration whose identity is in the list queues
  `community.outcome`. The identity of a configuration nobody shared is never sent: it could be guessed back from a
  small set of likely candidates.
- **Withdrawn.** When the list says a version was withdrawn, the configuration's row shows a warning note with the
  reason. The app changes nothing by itself.
- **A `.toml` downloaded from the site** is just a file, imported as today with the full review.

### 11.10 Licence, terms, content policy

- **What is shared is CC0 1.0** (decision 6). Settings are hardly copyrightable, and CC0 removes the doubt so that anyone
  can use, change and share again. A review stays its writer's, licensed to us to show it.
- **Terms of use**:
  - no configuration meant to harm a computer, open it to the network, or hide what it does;
  - models must be lawful to fetch from where they are;
  - no secrets, no personal data;
  - **no adult image or video models** (decision 11);
  - we may refuse, withdraw or remove content, and say why.

### 11.11 One copy of each configuration

Two files are the same configuration when they would run the same thing. The server keeps one copy of each. It finds
duplicates by the hash of a **canonical form**: what the file runs, written one way only.

**The rule that bounds it: missing a duplicate is acceptable; merging two different configurations is not.** Where the
canonical form cannot be sure that two spellings run the same, it keeps the difference. An argument whose meaning
depends on its position is the usual case.

**Left out of the canonical form** (none of it changes what runs):
- the formatting: the order of keys and tables, inline or not, whitespace, comments, quoting, `16_384` or `16384`;
- the name, the id, `[meta]` (note, `made_with`, `made_on`, `community`) and `requires`;
- `[runtime] version`, which is only what the program said about itself (the `build` label is kept);
- a model's `size` and its `name` for people (its identity is kept);
- settings whose field says `identity = false` in its engine document. This is a new key of `[[field]]`, set on the
  ports: two configurations that differ only in their port run the same model the same way. The code names no field
  (configurations.md §2);
- a value equal to its field's literal default in the engine document. The app's export already leaves these out, but
  a hand-written file may spell them out. A computed default (llama.cpp's context size follows the model) is never
  dropped.

**Normalized:**

| Part | Written as |
|---|---|
| Numbers, booleans, choices | Their field's type (`4096` and `4096.0` for an integer are 4096). A choice is written by its id |
| Models | The first stays first, because the engine loads it. The others are sorted by role, then identity. The identity is the file's SHA-256 when known (the server fills it in from the hub at moderation). Otherwise it is the source: a catalog id; an Ollama name in lower case, with `:latest` added when it has no tag; a Hugging Face `owner/repo` and path, with the revision `main` written as none |
| Extra environment lines | Split into `NAME=value`, with blank and comment lines dropped, sorted by name. A name given twice keeps the last value, as the process receives it |
| Extra command-line arguments | Below |
| `[[needs]]` | Sorted by kind and name, with `why` left out |
| A companion's table | As the engine's |

**Arguments:**
1. They are split with the splitter the launch uses, so quoting is gone: `"a b"` and `'a b'` are the same word.
2. `--name=value` becomes `--name value`. A short alias becomes its long name when the engine document lists it, in a new
   optional `aliases` of its flags. An unknown flag keeps its spelling.
3. A flag takes the words after it, up to the next flag. A negative number is a word, not a flag. A flag that the
   document says takes no value takes none.
4. The groups are sorted by flag name. Repeats of one flag keep their order among themselves: `--lora a --lora b` may
   not mean `--lora b --lora a`.
5. If a word comes before the first flag, or anything cannot be grouped, the arguments are kept exactly in their order,
   with no reordering at all.
6. A flag given twice is not collapsed into one. A flag that repeats a field (`--ctx-size` in the free text while the
   field is set) is not folded into the field. Whether the last one wins is the engine's parser's business, and the
   canonical form does not guess.

So `-c 8192 --temp=0.7 --flash-attn` and `--flash-attn --temp 0.7 --ctx-size 8192` are one configuration (with `-c`
listed as an alias of `--ctx-size`). `model.gguf --temp 0.7` and `--temp 0.7 model.gguf` are kept apart.

**The identity hash.** The canonical form is JSON with sorted keys and no spaces (RFC 8785, the JSON Canonicalization
Scheme). It carries the number of its rules (`"canon": 1`). The **identity** is the SHA-256 of `quickai-config-v1\n`
followed by that JSON. It is computed in `quickai-core` (`profile/canonical.rs`), so the app, the Worker (S-R1) and the
admin compute the same thing, and shared test vectors hold it. The server always computes it itself and never takes the
app's word for it.

**What the server does with a duplicate:**

| The new file is the same as… | Then |
|---|---|
| A published or pending version | Nothing is stored. The person is shown the existing configuration and added to its **Also shared by** count: people reaching the same setup independently is itself a sign of quality. Their categories become suggestions, and a different title or summary goes to the moderators as a suggested edit |
| An older version of a configuration that has moved on | They are told which configuration and version it is, and nothing is stored |
| A rejected or withdrawn version | Refused with the earlier reason, so that a refused file cannot come back under a new name |
| Their own configuration's current version, with only the title or summary changed | An edit of its text, not a new version |

A draft from the app is checked when it is uploaded, so the app hears "already shared as …" at once. A unique index on
the identity among pending and published versions holds the rule even when two people submit at the same moment: the
second insert fails and becomes the *Also shared by* case. Ratings, reviews and "worked on" therefore all gather on the
one copy.

**Close, but not the same.** Configurations with the same engine, models and build but different settings are
*variants*. A second, coarser hash (`family`, of the engine and the models' identities) groups them on the page
("3 variants"). Nothing is merged.

**When the rules change** (a new alias, a new `identity = false`), `canon` goes up. A job computes the identity of every
stored version again, and new matches go to the moderators as suggested merges. Nothing is merged without them.

## 12. Data: tables, retention, deletion

Two new D1 databases. Their migrations live in `services/shared/migrations/`, written to the standard of the site's
`0001_init.sql`: a comment on every table about what it keeps and why.

```sql
-- telemetry
CREATE TABLE installs (id TEXT PRIMARY KEY, first_day TEXT NOT NULL, last_day TEXT NOT NULL,
  blocked INTEGER NOT NULL DEFAULT 0);                                                         -- days, never times
CREATE TABLE batches (id TEXT PRIMARY KEY, install_id TEXT NOT NULL, day TEXT NOT NULL, version TEXT NOT NULL,
  tag TEXT NOT NULL CHECK (tag IN ('official','source','unknown')), level TEXT NOT NULL, os TEXT, os_version TEXT,
  arch TEXT, memory_gb INTEGER, gpus TEXT, language TEXT, country TEXT, events TEXT NOT NULL); -- the validated JSON
CREATE TABLE run_days (install_id TEXT NOT NULL, day TEXT NOT NULL, engine TEXT NOT NULL, build TEXT, model TEXT NOT NULL,
  hardware TEXT NOT NULL, started INTEGER, ready INTEGER, failed INTEGER, ready_seconds INTEGER,
  PRIMARY KEY (install_id, day, engine, model));                                               -- made each night
CREATE TABLE crashes (id TEXT PRIMARY KEY, install_id TEXT NOT NULL, received TEXT NOT NULL, event TEXT NOT NULL,
  version TEXT, tag TEXT, hardware TEXT, fingerprint TEXT NOT NULL, body_key TEXT NOT NULL);
CREATE TABLE crash_groups (fingerprint TEXT PRIMARY KEY, title TEXT, first_seen TEXT, last_seen TEXT,
  reports INTEGER, installs INTEGER, status TEXT, fixed_in TEXT, issue_url TEXT, notes TEXT);
CREATE TABLE versions (version TEXT PRIMARY KEY, published TEXT, functions_key TEXT);         -- from GitHub, each night
CREATE TABLE rollups (day TEXT, metric TEXT, dimension TEXT, value INTEGER, installs INTEGER,
  PRIMARY KEY (day, metric, dimension));                                                       -- no install ids
CREATE TABLE blocks (kind TEXT CHECK (kind IN ('install','network')), value TEXT, reason TEXT, until TEXT,
  PRIMARY KEY (kind, value));
CREATE TABLE deletions (install_hash TEXT, requested TEXT, done TEXT);                         -- a receipt, no data
CREATE TABLE audit (at TEXT, who TEXT, action TEXT, target TEXT);

-- community
CREATE TABLE users (id INTEGER PRIMARY KEY, github_id INTEGER UNIQUE, login TEXT, name TEXT, avatar TEXT,
  created TEXT, role TEXT CHECK (role IN ('member','moderator','owner')), state TEXT CHECK (state IN ('ok','limited','banned')));
CREATE TABLE sessions (id_hash TEXT PRIMARY KEY, user_id INTEGER, expires TEXT);
CREATE TABLE categories (id TEXT PRIMARY KEY, axis TEXT, name TEXT, description TEXT, position INTEGER);
CREATE TABLE configs (id TEXT PRIMARY KEY, author_id INTEGER, title TEXT, summary TEXT, state TEXT,
  current_version INTEGER, created TEXT, updated TEXT, downloads INTEGER NOT NULL DEFAULT 0);
CREATE TABLE config_categories (config_id TEXT, category_id TEXT, PRIMARY KEY (config_id, category_id));
CREATE TABLE config_versions (config_id TEXT, version INTEGER, toml TEXT, sha256 TEXT, engine TEXT, models TEXT,
  made_on TEXT, needs_review INTEGER, changes TEXT, state TEXT, reviewed_by INTEGER,
  identity TEXT NOT NULL, family TEXT NOT NULL, canon INTEGER NOT NULL,                       -- §11.11
  PRIMARY KEY (config_id, version));
CREATE UNIQUE INDEX one_copy ON config_versions (identity) WHERE state IN ('pending', 'published');
CREATE TABLE config_sharers (config_id TEXT, user_id INTEGER, at TEXT, PRIMARY KEY (config_id, user_id));  -- "also shared by"
CREATE TABLE refused (identity TEXT PRIMARY KEY, reason TEXT, at TEXT);    -- a refused file cannot return under a new name
CREATE TABLE ratings (config_id TEXT, user_id INTEGER, stars INTEGER CHECK (stars BETWEEN 1 AND 5), review TEXT,
  created TEXT, updated TEXT, hidden INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (config_id, user_id));
CREATE TABLE outcomes (config_id TEXT, version INTEGER, install_hash TEXT, hardware TEXT, worked INTEGER, day TEXT,
  PRIMARY KEY (config_id, install_hash));                    -- install_hash = HMAC(secret, install id): no join with telemetry
CREATE TABLE drafts (token_hash TEXT PRIMARY KEY, toml TEXT, identity TEXT, expires TEXT);
CREATE TABLE flags (id INTEGER PRIMARY KEY, config_id TEXT, rating_user INTEGER, reporter INTEGER, reason TEXT,
  created TEXT, resolved TEXT);
```

**Retention**, applied by the nightly job and stated in the policy:

| Data | Kept | Then |
|---|---|---|
| `batches` | 90 days | only `run_days` and `rollups` remain |
| `run_days` | 13 months | folded into `rollups`, which hold no install id |
| crash reports (row and R2 body) | 180 days | the group keeps its counts |
| crash groups | until a year after they are closed | |
| local crash files | 90 days | deleted by the app |
| `installs` | 13 months after the last report | deleted with everything of it |
| drafts | 24 hours | |
| sessions | 30 days | |
| `outcomes` | 13 months | folded into the counts |
| accounts, ratings, configurations | while the account exists | §11.4 |
| `deletions`, `audit` | 2 years | |
| Cloudflare's own logs | Cloudflare's terms | |

**Deletion from the app.** *Delete what EverybodyAI holds* sends `DELETE /v1/me`. The server deletes the install's row,
batches, runs and crashes, the R2 objects under `crash/<install>/`, and the community `outcomes` under its keyed hash.
`rollups` hold no id and stay. The answer is a receipt. The app then makes a new install id, so that what is sent later
is not tied to the old data. A test lists every table with an install column and fails if one is missing from the
deletion.

**Download from the app.** `GET /v1/me` returns every row and object of the install as one JSON file, saved where the
person chooses.

**Where the data is.** ~~Choose an EU location for D1 and R2.~~ Set aside by the user on 2026-10-30: "ignore the eu
rule, the data is stored elsewhere". The policy names Cloudflare and no location.

## 13. The policy and the other documents

All of these land in, or before, the first release that can send anything:

| Document | Change |
|---|---|
| `PRIVACY.md` | the *In short* section rewritten; a new section *Telemetry*; two bullets under *When the app uses the internet*; the files under `~/.quickai/telemetry`; the date. Draft in Appendix A |
| `everybodyai.dev/privacy` | now covers the API, the admin and the community site: the sign-in cookie, GitHub as the sign-in provider, ratings and reviews being public, retention, the processors (Cloudflare; GitHub for sign-in) |
| `quickai.everybodyai.dev/terms`, `/guidelines` | new: §11.10 |
| `SECURITY.md` | *No unexpected network traffic* adds telemetry, only at the chosen level and only to `api.everybodyai.dev`. New promises: an event never carries prompts, replies, paths or secrets (one that does is a bug); a community configuration is reviewed on import like any other file. The services are in scope |
| About window | the disclaimer names `api.everybodyai.dev` and when it is contacted (TC-ABT-004 and a new case) |
| Help | a page *Telemetry and privacy*, and one *Community configurations* |
| `README.md` | the privacy line, and the telemetry levels |
| `docs/configurations.md` | §8's "Provenance is not claimed" points to §11 here; `[meta] community` in §3 |
| `docs/reference/telemetry-events.md` | new, generated (§5.3) |
| store listings | the Microsoft Store and Snap Store data declarations and privacy links |
| `COMMERCIAL.md` | organizations can switch telemetry off for good (§5.4), and telemetry is never used to check a licence |

**The law, to be confirmed by a lawyer.** This is not legal advice. Consent is the basis, which is why nothing is sent
until the notice is answered. Under the GDPR and the UK GDPR, consent has to be free, informed, specific and as easy to
withdraw as to give, and the ePrivacy rule on storing and reading information on a device points the same way. VS Code
sends by default and offers a way out. That is a choice a large company made under its own legal reading, not a model
to copy blindly.

The policy names:
- the controller (EverybodyAI);
- the processors: Cloudflare, under its data processing addendum, and GitHub for sign-in;
- international transfers;
- retention;
- the person's rights, and how to use them without an account (the install id);
- complaint to a supervisory authority.

California's law asks for a notice at collection and a statement that nothing is sold or shared. The rules of the place
EverybodyAI is established in apply too.

## 14. Changes in the app

**`quickai-core`** (pure; tested by tables and golden files) gains a new `telemetry` module:
- `schema.rs`: events and fields with their level, kind of data and purpose (`serde`, `deny_unknown_fields`, a schema
  number); the JSON of `quickai --telemetry`, and the reference page;
- `level.rs`: what each level allows, and the caps of §5.4;
- `redact.rs` (§7.3);
- `hardware.rs` (§6.4).

It also gains the configuration's canonical form and identity (`profile/canonical.rs`, §11.11). The engine documents
get two optional keys for it: `identity` on `[[field]]`, and `aliases` of flags.

**`quickai-runtime`** gains a new `telemetry` module:
- `ids.rs`: the install id (made the first time something is sent, replaced on demand) and the session id;
- `queue.rs`: the queue, its cap, and the telemetry log;
- `sender.rs`: the background thread, `ureq` with a 15 s timeout, the waits of §6.5, and the cached status;
- `crash.rs`: the panic hook, the session marker and engine exits;
- `community.rs`: the list and a configuration, fetched and checked against the list's SHA-256;
- `policy.rs`: the level in force, from the feature, the package's layer, the environment and the settings.

**`quickai`** (the window):
- the first-start notice (§5.2);
- Settings → Privacy;
- the telemetry log's window;
- the crash window at Off (§7.4);
- Help → *Report a problem…* (§7.6);
- Configurations → *Community…* and *Share with the community…*;
- the About text;
- `quickai --telemetry`.

The cargo feature `telemetry` is on by default.

**`quickai-run`**: `--telemetry <level>`, the configuration key, and `telemetry list|show|send|delete`.

**Build identity.** The build scripts set `QUICKAI_CHANNEL=official` and `QUICKAI_COMMIT`, read with `option_env!`.
A build from source says `source`.

No cryptographic dependency is added.

## 15. Changes in the release pipeline (`quickai_base`)

1. The build scripts set the channel and the commit (§14).
2. `strip = "debuginfo"` after S-R2. Each release keeps the list of QuickAI's function names (for §7.5); the admin
   reads it by version.
3. Update the store listings' data declarations (§13).
4. Separately, and already planned: Windows code signing, and an Apple Developer ID with notarization (§8.5).

## 16. Tests

- **Rust, core**:
  - the schema's golden JSON and the `--telemetry` list;
  - the generated reference page is current;
  - what each level allows: a property test that no event above the level is ever queued;
  - the cleaning table: every row of §7.3, and lines that must not change;
  - the hardware class;
  - **the canonical form** (§11.11). The same configuration spelled differently gives one identity: arguments
    regrouped and reordered, `--a=b` or `--a b`, an alias, environment lines shuffled, keys and tables reordered,
    comments, quoting, a default spelled out, another port. A configuration whose meaning changed gives another
    identity: a positional argument moved, `--lora` repeats swapped, any value changed, another model, another build.
    A property test shuffles the flag groups of generated argument strings. The vectors are shared with the Worker
    (`services/shared/test-vectors/`), so the two sides cannot drift.
- **Rust, runtime**, against a `tiny_http` fake of the API, as the project's other network code is tested:
  - **nothing is sent before the notice is answered, or at Off** (the promise of PRIVACY.md);
  - each level sends only what it allows;
  - the queue's cap and the waits; 400 drops a batch, 410 stops sending, 429 waits, 403 waits 30 days;
  - the panic hook writes the file and makes no request;
  - at Off, a crash asks once and sends only on *Send this report*;
  - an unclean exit is an event, not a question;
  - the deletion makes a new id;
  - each way of switching off for good.
- **Worker** (vitest in workerd, like the site's):
  - each check of §8.2 and each answer of §9;
  - unknown events and fields are refused;
  - the version tags;
  - deletion covers every table and the R2 prefix (the table list is read from the migrations);
  - the admin refuses a request without an Access token;
  - the community checks of §11.6 against the app's table of hostile files (A32);
  - duplicates: each row of §11.11's table, and two submissions of one identity at once (one copy, one sharer);
  - the Bayesian sort.
- **GUI** (Robot, `quickai_test`):
  - the notice and each of its buttons;
  - the Privacy page and the telemetry log's window;
  - the crash window at Off after a forced panic (a test-only `QUICKAI_TEST_PANIC`);
  - Report a problem (the address it would open);
  - Community… against a fake API: the list, a file whose SHA-256 does not match, the review shown for a community
    file, the withdrawn note, "already shared as".
- **Browser** (Playwright, like the site's):
  - the community pages, with axe in light and dark;
  - sign-in with a fake GitHub;
  - rating;
  - sharing a draft, and a duplicate.
- **Network audit.** The packaged app is run with default settings for a session, with every connection logged (a
  network namespace on Linux). Nothing goes to `everybodyai.dev`.
- **New groups of the test plan**: **RP** (telemetry and crashes) and **CM** (community). Every group is checked by
  breaking the code on purpose, as earlier groups were.

## 17. Work packages and order

One feature in the plan's numbering, WP4.42, in parts like WP4.17. Each part lands green on its own branch. The words
(4.42.7) go out in the same release as the first part that can send.

| WP | Work | Repo | Size | After | Verified by |
|---|---|---|---|---|---|
| S-R1 | `quickai-core`'s configuration check and canonical form as WebAssembly in a Worker: does it build, its size, CPU per check | public, services | S | | a measured note |
| S-R2 | `strip = "debuginfo"`: size cost on the three systems, and what a panic's backtrace shows | public, base | S | | the numbers |
| S-R3 | what Workers Logs records of a request (addresses? headers?), and how to keep it out | services | S | | a note |
| 4.42.1 | **Build identity**: channel and commit in the build, function lists per release, strip after S-R2 | base, public | S | S-R2 | a release candidate's events say `official` |
| 4.42.2 | **The core**: the schema with levels and annotations, cleaning, hardware class, `--telemetry`, the reference page | public | M | | RP core tests |
| 4.42.3 | **The API**: `/v1` events, crash, me, status; the telemetry D1 and R2; checks, rate limits, blocks; Cron jobs; the new public repository | services | M | 4.42.2 | Worker tests |
| 4.42.4 | **Sending from the app**: ids, queue, sender, levels, the notice, Settings → Privacy, the telemetry log, the ways off, `quickai-run` | public | L | 4.42.2 | RP runtime tests, GUI cases, network audit |
| 4.42.5 | **Crashes and errors**: the hook, the marker, engine exits and the other error events, the crash window at Off, Report a problem | public | M | 4.42.4 | RP tests, GUI cases |
| 4.42.6 | **The admin**: Access, overview, systems, engines and models, features, errors, crashes, versions, installs and blocks, switches, site, audit | services | L | 4.42.3 | Worker tests, Playwright |
| 4.42.7 | **The words**: PRIVACY.md, the site's policy, SECURITY.md, About, Help, README, COMMERCIAL.md, store listings; a lawyer's review | public, everybodyai, base | S | before the release of 4.42.4 | TC-ABT cases, a read-through |
| | *the first release that can send: telemetry levels and crash reports* | | | | |
| 4.42.8 | **One copy of each**: the canonical form and the identity in `quickai-core`; `identity = false` on the engine documents' ports and `aliases` of their flags (and the generated reference pages); `quickai configuration identity <file>` | public | M | 4.42.2 | CM canonical tests, shared vectors |
| 4.42.9 | **The community's back end**: accounts, configurations, versions, the checks (S-R1), duplicates, moderation, the list | services | L | 4.42.3, 4.42.8, S-R1 | Worker tests incl. hostile files and duplicates |
| 4.42.10 | **`quickai.everybodyai.dev`**: the pages of §11.1, sign-in, rating, sharing, *Also shared by*, variants | services | L | 4.42.9 | Playwright, axe |
| 4.42.11 | **The community in the app**: Community…, the SHA-256 check, the review, `[meta] community`, worked-or-not, Share, "already shared as" | public | M | 4.42.9 | CM tests, GUI cases |
| 4.42.12 | **Moderation pages** in the admin (suggested merges and edits among them); terms and guidelines | services | M | 4.42.9 | Playwright |
| 4.42.13 | **A public page of numbers** (buckets of 20 installs or more) | services | S | 4.42.6 | Playwright |

The decisions below go into the plan's decision log from C195 on, when the first part lands.

## 18. Decisions

Made by the user on 2026-10-30 ("I accept your recommendeds", with the answers for 7, 10 and 11).

| # | Decision |
|---|---|
| 1 | VS Code's four telemetry levels. **Nothing is sent until the first-start notice is answered**, and closing it means Off |
| 2 | At Off, a crash is offered once, with the report shown. At *Crashes only* or above it is sent by itself, as in VS Code |
| 3 | Hosts: `api.`, `admin.` and `quickai.everybodyai.dev` |
| 4 | Community sign-in with GitHub only, at first |
| 5 | Everything in the community is moderated at launch |
| 6 | Shared configurations are CC0 1.0 |
| 7 | The country is stored, as two letters taken from Cloudflare, never the address |
| 8 | Workers Paid before launch ($5 a month when last checked): the free plan's daily D1 writes are per account, and its CPU limit is 10 ms |
| 9 | A public page of numbers, after the admin (4.42.13) |
| 10 | The server code and schemas are public, in `everybodyai/services` |
| 11 | Configurations for adult image or video models are not allowed |
| 12 | "Worked on" counts only installs at level All |
| 13 | No signing of reports, requests or community files; the standard checks of §8 instead (the user: "drop the idea if its not a standard one - we will follow similar to vscode") |

## 19. Costs, limits and risks

**Cloudflare, at the time of writing:**
- **Workers Free**: 100 000 requests a day and 10 ms CPU per request
  ([limits](https://developers.cloudflare.com/workers/platform/limits/)).
- **D1 on the free plan**: 5 million rows read and 100 000 written a day, and 5 GB. Since 2026-09-01, queries fail
  once the day's limit is reached
  ([changelog](https://developers.cloudflare.com/changelog/post/2026-09-01-d1-free-tier-limit-enforcement/),
  [FAQ](https://developers.cloudflare.com/d1/reference/faq/)).
- **R2**: 10 GB-month, 1 million class A and 10 million class B operations a month free, with no egress fees
  ([pricing](https://developers.cloudflare.com/r2/pricing/)).
- **Access**: free for teams under 50 users ([Cloudflare Access](https://www.cloudflare.com/products/cloudflare-access/)).
- **The rate-limit binding** has been generally available since 2025-09-19
  ([changelog](https://developers.cloudflare.com/changelog/2025-09-19-ratelimit-workers-ga/)).

**A rough budget.** An install at level All sends about three batches a day: three writes, plus its install row. On the
free plan, about 25 000 such installs a day would use up D1's daily writes on their own, and that ceiling is shared with
the forms and the community. Hence decision 8.

**Risks, and what holds them:**

| Risk | Held by |
|---|---|
| An event carries something personal | the schema allow-list, the cleaning table, no logs in automatic reports, the telemetry log, the tests |
| People read "telemetry" as "spying" and leave | off until chosen, `--telemetry`, the telemetry log, the public numbers, the public server code, plain words in the notice |
| Anyone can send fake reports to a public endpoint | schemas, known versions, rate limits, block lists, counting distinct installs, the switches (§8) |
| A harmful configuration is shared | moderation, `needs_review`, the app's own review on import, withdrawal with a note in the app |
| A configuration arrives broken or stale | HTTPS and the list's SHA-256 |
| The account's daily D1 limit is reached | Workers Paid, one write per batch, the app's waits |
| The queue grows while the server is down | its cap, dropping the oldest, never blocking the window |
| The policy and the app disagree | 4.42.7 in the same release; the About test; the network audit |
| An old version keeps sending forever | 410 from the switches |
| Moderation takes more time than there is | everything moderated at first, then auto-publishing of clean files from trusted accounts |

## 20. Left out on purpose

- **Signing** of reports, requests or community files; install keys; proof of work; attestation (decision 13, §8).
- **An update check or announcements in the app.** Each would be a new reason to connect, needing its own consent.
- **Using telemetry to check a licence**, now or later.
- **Memory dumps** (§7.7), session replays, per-prompt analytics, and A/B experiments. VS Code runs experiments on its
  usage data; QuickAI does not.
- **Accounts and syncing:** an account inside the app; syncing configurations between computers; a `quickai://` link
  (still left out, as configurations.md says); threads of comments or a forum. Reviews are enough to start.
- **Tokens per second**, until the app measures it.

## 21. Built in the app (2026-10-30)

The app's part, 4.42.1 (in part), 4.42.2, 4.42.4, 4.42.5 and 4.42.7 (in the app's repository), is built: WP4.42 of the
implementation plan, decisions C195 to C197. What differs from the sections above, and what is left:

- **A crash report is a batch of one `crash.panic` event** (`Batch`, the same shape as the events'), sent to
  `POST /v1/crash`. Its `install` and `level` are filled in when it is sent; the hook writes them empty.
- **Not sent yet**: `community.outcome` (there is no community yet), an install that failed (`error.install`), and
  `model.add` for an Ollama pull or a file imported (a download from the recommended list, a look-up or a link is
  counted). `error.engine_exit` has no `changed` field. The schema lists only what is sent, and grows with it.
- **The package's cap** is a file `telemetry.toml` in the package's configuration folder (`ConfigDirs::bundle`), with one
  line `level = "off"` (or `crash`, `error`).
- **`QUICKAI_TELEMETRY_URL`** replaces `https://api.everybodyai.dev`, for a test or a server of one's own.
- **The app does not read `GET /v1/status` yet**: a version the server no longer takes answers 410 to a batch, and the
  app stops.
- **`quickai-run` sends nothing** and has no `--telemetry` yet.
- **The build's identity**: `QUICKAI_CHANNEL=official` and `QUICKAI_COMMIT` at build time, read by
  `quickai-runtime/build.rs`, which also passes the target. The release scripts of `quickai_base` set them.
- **The queue is written one line at a time**, and whole only when it has to drop its oldest events.
- **Left**: the API (4.42.3), and it must exist before a release carries this; the admin (4.42.6); the community
  (4.42.8 to 4.42.12); `strip = "debuginfo"` (S-R2); the function lists of a release for the plausibility of a
  backtrace; a lawyer's read of the policy; the site's page of QuickAI still says "No telemetry", which is true of
  0.1.0 and changes with the release that carries this.

## 22. Built on the server (2026-10-30)

The user, the same day: "prepare the apis and deploy that - I also need a page to administer the data". Built and
deployed: 4.42.3 (the API) and 4.42.6 (the admin), in a new public repository, [everybodyai/services](https://github.com/everybodyai/services),
on its branch `main`.

- **`api.everybodyai.dev`** (Worker `everybodyai-api`): `POST /v1/events`, `POST /v1/crash`, `GET` and `DELETE /v1/me`,
  `GET /v1/status`, with every check of §8.2 and every answer of §9. Workers Logs is off. The Cron trigger runs at
  02:20 UTC: versions from GitHub, the day folded into `run_days` and `rollups`, the retention of §12, and an email to
  the owner about new crash groups and spikes.
- **The schema the server checks by is the app's own list.** `quickai --telemetry` now prints a `type` for every field
  (and `values` for a choice) beside the words, and `services/shared/schemas/1.json` is that output, copied by
  `npm run sync:schema`. The server's rules repeat `Shape::accepts`, and refuse any unknown key too. A test sends an
  example of every event and field in the list.
- **Crash bodies are in D1, not R2**: R2 is not enabled on the account, and a report is at most 256 KiB, well under
  D1's 2 MB a row. Deletion is then one statement per table. No EU location: the user set that rule aside ("ignore the
  eu rule, the data is stored elsewhere"), and PRIVACY.md names no location. (The database `telemetry` was first made
  with D1's EU jurisdiction; it was made again without it the same day.)
- **Crash groups** are keyed by QuickAI's first three frames (the functions, not the lines, so a crash keeps its group
  across versions), or by the location and the message without its numbers. A group marked fixed in a version opens
  again when the crash comes from that version or a later one.
- **`admin.everybodyai.dev`** (Worker `everybodyai-admin`): the pages of §10 except Community (nothing to moderate yet).
  Server-rendered, no script, a CSP of `default-src 'none'`, charts in SVG. The tables of distinct installs read the
  batches of the period (kept 90 days, the longest period offered); the daily chart reads `rollups`. Buckets under 20
  installs are shown as "other" on Systems. Every change is in the audit log; a deletion is logged by a hash of the
  install id, not the id.
- **Signing in to the admin: a passphrase for now, Access when it is set up.** Cloudflare Zero Trust is not set up on
  the account (the API token cannot reach it), so the admin takes a passphrase (the secret `ADMIN_PASSWORD`, kept by
  the owner) and gives a signed cookie for 12 hours (`SESSION_KEY`), limited to 10 tries a minute per address. Signing
  out ends every session. Once an Access application covers `admin.everybodyai.dev`, setting `ACCESS_TEAM_DOMAIN` and
  `ACCESS_AUD` makes the Worker require and check Access's token instead, as §10 has it.
- **Tests**: 36 Worker tests in workerd (vitest), among them each refusal, the version tags, 429, deletion against
  every table with an `install_id` column (read from the database the migration made), the nightly fold and
  retention, the admin's sign-in, CSRF refusal, escaping and each change.
- **Left**: Cloudflare Access for the admin;
  Workers Paid (decision 8); the app reading `GET /v1/status`; the community (4.42.8 to 4.42.12); the public page of
  numbers (4.42.13); the function lists of a release for checking a backtrace (§7.5).

## Appendix A: draft text for PRIVACY.md

*In short* becomes:

> QuickAI sends nothing to its developer until you choose to. The first time it starts it asks you to pick a
> **telemetry level**:
> - **All**: crash reports, error reports and counts of what the app did (which engine ran, whether a model started,
>   which windows were opened);
> - **Errors and crashes**;
> - **Crashes only**;
> - **Off**.
>
> Until you answer, and whenever it is Off, nothing is sent. Reports go to EverybodyAI's server, `api.everybodyai.dev`,
> with a random install id and a rough description of your computer. They never include your prompts, the model's
> replies, your files, folders or searches. `quickai --telemetry` lists everything the app can send; Settings → Privacy
> shows what it did send, and lets you download what we hold, delete it, or change the level at any time.
>
> The app has no accounts, no advertising and no automatic update check. Your prompts and the model's replies are
> processed on your own computer and are never uploaded. Apart from telemetry, the app talks to the internet only when
> you ask it to: to search for or download a model, a build of an engine, or a community configuration (see [When the
> app uses the internet](#when-the-app-uses-the-internet)).

A new section, after *What the app does with your data*:

> ## Telemetry
>
> **The levels.**
> - **Crashes only**: a report when QuickAI stops because of an error. It holds the error, where in QuickAI it happened,
>   the version, and what the app was doing (which engine, which windows), never a log.
> - **Errors and crashes** adds reports of what failed: an engine that stopped, a download or an install that failed
>   (its kind and reason, never a file name or an address), a window that stopped answering.
> - **All** adds counts of what the app did: how long it was open, in ranges; which windows and features you used; how
>   you added models; for each engine, model and build you ran, whether it started and how long that took; and, if you
>   used a community configuration, whether it worked.
>
> Every report also says: the version and kind of package; your system, its version and your processor's architecture;
> your memory rounded to 8, 16, 32, 64 or 128 GB and more; the makers of your graphics cards; and your system's language.
> A model is named only when it is on QuickAI's recommended list, in Ollama's public library, or a public Hugging Face
> repository you found through the app; any other model is "other" with its size. Before anything is sent, the app
> replaces your home folder, user name, computer name, addresses, email addresses and anything that looks like a
> password or a token.
>
> **At Off.** Nothing is sent. If QuickAI stopped because of an error, the next start offers to send that one report,
> and shows it to you first.
>
> **What it never contains**: your prompts, the model's replies or your chats; pictures; file or folder names; words you
> typed into a search or a box; the addresses of your remote computers; the text of command-line arguments or
> environment lines; your configurations; your IP address (it reaches Cloudflare, which carries the request, and we do
> not store it).
>
> **The install id** is a random number QuickAI makes the first time it sends something
> (`~/.quickai/telemetry/install-id`). It is not made from your hardware and is not linked to your name or any account,
> but it does link your reports together, so the data is pseudonymous, not anonymous. Settings → Privacy shows it, and
> **New install id** replaces it. Don't post it in public: it is how you ask us for your data.
>
> **Where it goes, and for how long.** To EverybodyAI's server, `api.everybodyai.dev`, which runs on Cloudflare (Workers,
> D1 and R2) under Cloudflare's data processing terms. The server records the country your request came from (two
> letters), not your address. Reports are kept in full for 90 days, and counts per engine and model for 13 months; after
> that, only totals with no id remain. Crash reports are kept for 180 days.
>
> **Your choices.** `quickai --telemetry` lists every report the app can send. Settings → Privacy → **Show what is sent**
> shows what it did send, as it was sent. **Download what EverybodyAI holds** saves all of it to a file. **Delete what
> EverybodyAI holds** deletes it from the server at once and starts a new install id. You can change the level at any
> time. Without the app, write to hello@everybodyai.dev with your install id. Withdrawing consent changes nothing that
> was lawful before it.
>
> **Off for good.** Builds without telemetry exist, and packagers can make them. An organization can turn it off in the
> package's configuration; `QUICKAI_TELEMETRY=off` or `DO_NOT_TRACK=1` in the environment turns it off too. `quickai-run`
> sends nothing unless its configuration or `--telemetry` says so.
>
> **What it is not for.** Telemetry is never sold or shared, never used for advertising, and never used to check a
> licence. The server's code is public at github.com/everybodyai/services.
>
> **Report a problem**, in the Help menu, sends nothing to us. It opens an issue on GitHub with your text and your
> system's details, which you read and can change before you post it, under GitHub's privacy statement.

Two new bullets under *When the app uses the internet*:

> - **Telemetry, at the level you chose.** As described in [Telemetry](#telemetry), to `api.everybodyai.dev` only.
> - **Community configurations.** **Community…** in the Configurations window asks `api.everybodyai.dev` for the list of
>   shared configurations, and **Get** fetches the one you chose. These requests carry no install id and nothing about
>   your computer: the list is filtered on your computer. Each file is checked against the list and then shown to you,
>   as any imported configuration is. Rating and sharing happen on `quickai.everybodyai.dev`, in your browser, under the
>   site's policy.
