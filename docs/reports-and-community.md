# Reports and the community

Usage statistics, crash reports, the admin of everybodyai.dev, and the shared configurations of quickai.everybodyai.dev.

_Status: proposal, 2026-10-30. Nothing of it is built. The choices in [§17](#17-choices-for-you) are open; the rest is the
recommended design._

The user, 2026-10-30: "we need to track the usage of the quickai and also provide option for the user to provide crash
reports. everybodyai.dev (admin page) needs to be created and it has to have apis in place which accepts the
telemetry/crash reports by the app. We also need a subdomain quickai.everbodyai.dev to have configuratiosn shared by our
users to help each other out - which is categorized and rateable too. We also need to change the privacy policy and so on
as well to incorporate this change. When a report is provided to us, we need to ensure that it is trusted, some mechanism
of signing/verification is needed here - e.g sha of the binary with a public key is provided and it is used to sign the
cfg/report etc. I may have missed many aspects, think about them and prepare an implementation documentation".

The host is written `quickai.everybodyai.dev` below (the request has `everbodyai`).

## 1. In short

- **Off until the person turns it on.** Usage statistics are opt-in. Crash reports are asked about each time, with the
  report shown first. A person who never says yes sends nothing, and QuickAI never connects anywhere by itself, as
  PRIVACY.md promises today.
- **One host for the app: `api.everybodyai.dev`.** Reports, the deletion of them, and the community list all go there.
  `admin.everybodyai.dev` is the admin, behind Cloudflare Access. `quickai.everybodyai.dev` is the community site. All of it
  runs on Cloudflare (Workers, D1, R2), like the site does now.
- **Count, don't collect.** A daily report holds counts and coarse buckets: versions, engine, whether a model started,
  memory in steps, GPU vendor. It never holds prompts, replies, file names, paths, search words or an IP address.
- **Signing, done where it can work.** Your idea of a public key in the binary is right for what *we* send the app: the
  community configurations, the list of them and the server's switches. The app checks those with our public key.
  It cannot work the other way round. A public key cannot sign. Any private key inside an open-source app is everyone's
  key. A client can claim any binary SHA. So what the app sends is signed with a **key of its own install**, made on that
  computer. That proves the reports come from the same install and that nobody else can send, read back or delete in its
  name. Whether a report came from an **official build** is checked against a release manifest we sign. Junk is held
  back by proof of work, rate limits, and dashboards that count distinct installs (§5).
- **Community configurations are QuickAI's configuration files**, the same `profile` documents the app already exports
  and imports with a review. They are sorted by use, engine and hardware. People rate them with stars after signing in
  with GitHub. "Worked on N computers like yours" comes from opted-in installs. Each published version is signed by us.
  The app still shows the import review for a signed file: a signature says where a file came from, not that it is
  safe.
- **One copy of each configuration.** Two files that run the same thing are one configuration, whatever the order of
  their arguments, environment lines or keys. The server keeps one copy, found by the hash of a canonical form. Anyone
  who shares it again is counted under "also shared by" on that copy (§10.11).
- **The person can see, download and delete everything.** Every report is shown before it is sent and kept in a sent
  log. The app has buttons that download or delete what EverybodyAI holds for this install, and one that starts a new
  install id.
- **Packagers and organizations can switch it off for good.** There will be a build feature, an environment variable
  (`DO_NOT_TRACK=1` is honoured) and a policy in the package's configuration layer.
- **The privacy policy, SECURITY.md, the site's policy, the About window and Help change in the same release.**
  Draft text is in [Appendix A](#appendix-a-draft-text-for-privacymd).

## 2. What was asked, and what it needs besides

| Asked | What it takes | § |
|---|---|---|
| Track usage | A daily, locally aggregated report, opt-in, with a fixed schema the server enforces | 6 |
| Crash reports | A panic hook and an unclean-exit marker. Engine failures become reports too: they are the crashes people actually meet. Each report is shown and confirmed | 7 |
| Admin page and APIs | `api.everybodyai.dev/v1` (Worker, D1, R2). `admin.everybodyai.dev` behind Cloudflare Access | 8, 9 |
| `quickai.everybodyai.dev`, categorized and rateable | Server-rendered pages, GitHub sign-in, categories, stars, "worked on", moderation, signed versions, browse and share from the app | 10 |
| The same configuration shared by many is kept once | A canonical form of what a configuration runs (the order of arguments, environment lines and keys, quoting, defaults) and its hash | 10.11 |
| Privacy policy | PRIVACY.md, the site's policy, community terms, SECURITY.md, About, Help, store listings | 12, App. A |
| Trusted reports, signing | Four keys with separate jobs, a signed release manifest, signed requests, signed content | 5 |

Not asked, but needed for it to work or to be lawful:

1. **Consent done properly**: opt-in; the person can say no as easily as yes; the consent is tied to a version of the
   policy and asked again when what is collected changes. The word "anonymous" is never used: an install id makes the
   data *pseudonymous*.
2. **Access and deletion without accounts.** We can only find someone's data by their install id. So the app itself
   offers *Download what EverybodyAI holds* and *Delete it*, signed with the install's key (§11).
3. **A kill switch.** The server can tell old versions to stop sending (a signed status, §8). We can stop collecting
   something without shipping a release.
4. **A way off for good** for packagers (AUR, Snap, Homebrew), organizations under the commercial licence, and
   `quickai-run` on servers (§6.6).
5. **Symbols.** Release builds strip symbols today (`strip = "symbols"`). With that, a backtrace is addresses. The
   release profile should keep function names (§7.6).
6. **Engine failures.** llama-server, Ollama or ComfyUI exiting during a load is QuickAI's most common "crash" (the
   ComfyUI CPU crash with GPU=auto is one). These get a report of their own (§7).
7. **Hardware classes.** Telemetry, community filters and "worked on computers like yours" all need one coarse
   vocabulary of computers. It has to be coarse enough not to fingerprint anyone (§6.4).
8. **Moderation and a content policy** for the community, a licence for what people share (CC0 is recommended), and
   terms of use (§10.10).
9. **Secrets and paths in shared files.** The server checks again for what the app's export leaves out (§10.6). A
   person can still type a token into a free-text field.
10. **A withdrawn configuration.** A configuration found to be unsafe after people imported it is marked in the signed
    list, and the app shows a note on that configuration (§10.9).
11. **Cloudflare's daily limits are per account.** On the free plan, a flood of reports could use up D1's writes for the
    day, and then the waitlist, the contact form and the community site would fail too (§18).
12. **The waitlist and contact messages** already sit in D1 with no page to read them. The admin should show them.
13. **Giving back.** A public page of aggregate numbers (systems, engines, the models people run) builds trust.
    It shows only buckets with at least 20 installs.
14. **Reports are never used to enforce the licence.** The policy says so. It is the first thing a wary user will ask.

## 3. Rules

1. **Nothing is sent that the person did not turn on or confirm.** By default the app makes no connection to
   EverybodyAI. Crash files are written locally, as `ui-stalls.log` is today, and stay local until the person sends one.
2. **What is sent can be seen, before and after.** *What is sent* shows the next real report. The sent log keeps the
   last 100 reports exactly as they were sent.
3. **Count, don't collect.** Every field of a report is in a schema with a reason. The server refuses unknown fields.
   The app has no free-form "properties" bag.
4. **A signature says where something came from, never that it is true or safe.** Content we sign is still reviewed by
   the app like any other file. Reports signed by an install are still checked as untrusted input.
5. **One host.** The app talks to `api.everybodyai.dev` and to no other EverybodyAI host. The PRIVACY.md list of hosts
   grows by one.
6. **Nothing blocks the window.** Sending runs on a background thread with short timeouts. A failed send waits and
   retries. Nothing ever asks the person twice about the same report.
7. **Off for good is possible** (rule of §6.6), and a build without the feature has no sending code at all.
8. **Separate data, separate keys.** The telemetry data and the community data are in separate databases. The community
   side holds a keyed hash of the install id, not the id itself, so the two cannot be joined by a query. Each key does one
   job.

## 4. The pieces, and where they run

```
 QuickAI (window)  ─┐    HTTPS, JSON, signed with the install's key
 quickai-run       ─┼──────────────────────────────►  api.everybodyai.dev  (Worker "api")
                    │   ◄── signed with the content key: status, community list, configurations
                    │                                   │            │
                    │                                   ▼            ▼
                    │                           D1 "reports"     R2 "reports" (crash bodies)
                    │                           D1 "community"   R2 "community" (nothing large yet)
                    │                                   ▲
 browser ──────────────► quickai.everybodyai.dev (Worker "community", Astro, server-rendered)
                         GitHub sign-in, ratings, submissions
 browser ──────────────► admin.everybodyai.dev (Worker "admin", behind Cloudflare Access)
                         dashboards, crashes, moderation, releases, deletion, switches
 release (quickai_base) ─► SHA256SUMS + binaries.json, signed with the release key ─► admin: known builds
```

| Host | What | Who reaches it |
|---|---|---|
| `everybodyai.dev` | the site, as now (static, with the forms Worker) | everyone |
| `api.everybodyai.dev` | the app's API, `/v1/...` | the app; the community site's pages read through bindings, not over HTTP |
| `quickai.everybodyai.dev` | community configurations | everyone; signing in to rate or share |
| `admin.everybodyai.dev` | the admin | the owner and moderators, through Cloudflare Access |

**Repositories.** Add the three Workers to `everybodyai` (the site's repository: it is already the Cloudflare project,
with D1 migrations, workerd tests and CI): `site/` (what is there now), `api/`, `community/`, `admin/`, and `shared/`
for the schemas, test vectors and the D1 migrations. The app's side is in `quickai-public`. The release side is in
`quickai_base`.
*Choice 10:* the API Worker and the report schemas could be published (in the public `quickai` repository or a public
`everybodyai/reports`) so that anyone can check what the server keeps.

## 5. Trust: what signing can prove, and the keys

### 5.1 The idea, and where it stops

The proposal: "sha of the binary with a public key is provided and it is used to sign the cfg/report".

- **A public key cannot sign.** It can only check a signature that the matching *private* key made. To sign in the
  app, the app needs a private key.
- **A private key in the app is everyone's key.** QuickAI's source is public and its binaries are downloadable.
  Anything compiled into it can be read out in minutes, and a script can then sign whatever it likes. A signature
  made with a shared embedded key proves only "someone with a copy of QuickAI sent this", which is everybody.
- **The binary's SHA is a claim, not a proof.** The app computes it and reports it. A forger reports the official one.
- **No desktop can prove an untouched official binary** to a server, for an app outside the platform stores. Remote
  attestation needs hardware and platform services that Windows, Linux and a DMG on macOS do not offer a program like
  this.

So signing is used where it does prove something:

| Direction | What a signature proves | Key |
|---|---|---|
| **We → the app** (community list, configurations, server status) | it came from EverybodyAI unchanged: not from a hijacked DNS, a cache or a copy on a forum | the **content key**. Its private half is ours alone; the **public half is compiled into the app**, which is your idea, in the direction where it holds |
| **Release → users and server** (which builds are official) | these files and these executables are the ones we released | the **release key** (offline) |
| **The app → us** (reports, deletion, shared drafts) | every report signed by this key comes from **the same install**; nobody else can send in its name, read back its data or delete it | an **install key**, made on that computer the first time it sends; the private half never leaves it |
| **Admin → admin pages** | the person is the owner or a moderator | Cloudflare Access, not a key of ours |

What a report's own checks add on top (§5.7): its build is one we released, it fits the schema, its backtrace makes
sense for that build, and its install is not new, flooding or blocked.

### 5.2 The keys

All signatures are Ed25519: `ed25519-dalek` 2 in Rust, and WebCrypto `Ed25519` in Workers (supported, with
`NODE-ED25519` as the older name: [Workers Web Crypto](https://developers.cloudflare.com/workers/runtime-apis/web-crypto/)).
Every signed message starts with a fixed prefix of its own (`quickai-request-v1`, `quickai-content-v1`,
`quickai-work-v1`, the SSH namespace `quickai-release`). That way a signature made for one purpose can never be passed
off as another.

| Key | Private half | Public half | Rotation |
|---|---|---|---|
| **Release** | Offline: the owner's machine, passphrase-protected (or a hardware key); never in CI. | `allowed_signers` in the `quickai` repository, on the site's download page, in the admin Worker | A new key is announced in a release signed by the old one |
| **Content** | A Worker secret (`wrangler secret put CONTENT_KEY`). Only the API Worker reads it | Compiled into the app as a list of `(key id, key)`, at most two at a time | Ship the new key in release N. Start signing with it once most installs run N or later (the admin shows versions). Drop the old key in N+2. A leaked key is stopped by a release that drops it; until then, the expiry of the signed list (14 days) limits a replay of an *old* list |
| **Install** | `~/.quickai/reports/install.key`, readable by the person only (0600; the profile's ACL on Windows) | Sent with every request; the server keeps it from the first one | *Start a new install id* in Settings makes a new one; the old data stays until deleted |
| **Admin** | none of ours: Cloudflare Access sessions | Cloudflare's | Cloudflare's |

*An option not taken in v1:* sign each configuration from the admin's machine with an offline content key, so that no
signing key is on Cloudflare. It is safer, but publishing would then need the owner at a terminal every time.

### 5.3 Official builds: the release manifest

At release, `quickai_base` writes `binaries.json`: the version, the commit, and the SHA-256 of every executable inside
every package (`quickai` and `quickai-run`, per target and package kind). The same binary in the tar.gz, the AppImage and
the snap has one hash. It is listed in `SHA256SUMS` like the packages. The owner signs `SHA256SUMS` once, offline:

```sh
ssh-keygen -Y sign -f ~/.keys/quickai-release -n quickai-release SHA256SUMS      # writes SHA256SUMS.sig
```

`SHA256SUMS.sig` goes on the release beside the files. **Anyone** can then check a download with tools every system
already has (OpenSSH's `ssh-keygen` is on Linux, macOS and Windows 10 and later):

```sh
ssh-keygen -Y verify -f allowed_signers -I release@everybodyai.dev -n quickai-release -s SHA256SUMS.sig < SHA256SUMS
sha256sum -c SHA256SUMS --ignore-missing
```

In the admin, *Releases → Add* takes `SHA256SUMS`, its `.sig` and `binaries.json`. It verifies the SSH signature (the
SSHSIG format is SHA-512 and Ed25519, both in WebCrypto), checks that `binaries.json` is listed with the right hash, and
stores the executables' hashes as **known builds**.

The app reports what it is: `version`, `target`, `package` (appimage, tar, snap, msix, zip, dmg, brew…), `channel`
(`official` when the build scripts set `QUICKAI_CHANNEL=official`, otherwise `source`), `commit`, and `exe_sha256`. It
hashes its own executable once per version, on the reporting thread, and caches the result by the file's size and
modification time.

### 5.4 The install key, and a signed request

The first time the app sends anything (a crash report the person confirmed, or the first daily report), it makes an
Ed25519 key pair. The **install id** is the first 16 bytes of SHA-256(public key), written as 32 hex characters. It is
shown in Settings (`3f1c9a2e…`) so a person can quote it to us.

Every request to a signed endpoint carries:

```
QuickAI-Key:       <the public key, base64url, 32 bytes>
QuickAI-Time:      2026-10-30T09:12:44Z
QuickAI-Request:   5b0e2c7a-…                 (UUID v4, new for every request)
QuickAI-Signature: <base64url, 64 bytes>
QuickAI-Work:      <base64url nonce>          (the first request of a key only)
```

The signature is over these bytes:

```
quickai-request-v1\n<METHOD>\n<path>\n<QuickAI-Time>\n<QuickAI-Request>\n<lowercase hex SHA-256 of the body>
```

- **Time.** The server refuses a request whose time is more than 10 minutes away from its own. The refusal carries the
  server's time, so the app corrects its clock offset once and tries again.
- **Replays change nothing.** Every write is keyed: a daily report by (install, day), a crash by its report id, a draft
  by its id. Sending the same request again rewrites the same row. Together with the time window, the server needs no
  table of seen requests.
- **The first request of a key (trust on first use).** There is no registration step. The server records the key the
  first time it sees it. That first request carries a proof of work: a nonce such that
  SHA-256(`quickai-work-v1\n` ‖ public key ‖ nonce) starts with 20 zero bits. That is about a million hashes, under a
  second of native code once per install. The server checks it with one hash. It makes minting thousands of fake installs
  slow, not impossible. The signed status (§8) can change the number; the app refuses anything above 24.

### 5.5 What we send, signed by us

Everything the app downloads from `api.everybodyai.dev` comes in one envelope:

```json
{
  "envelope": 1,
  "kind": "community-index",
  "key": "content-2026a",
  "payload": "<base64 of the exact bytes>",
  "signature": "<base64: Ed25519 over 'quickai-content-v1\\n' + kind + '\\n' + the payload bytes>"
}
```

The app checks things in this order:

1. The key id is one it knows. An unknown id is refused.
2. The signature is valid.
3. The payload's `kind` is the one it asked for.
4. For the community list and the status, the payload's `version` is not lower than the highest the app has seen
   (no rollback), and `expires` has not passed. The server signs the list again every day, and lists expire after
   14 days.
5. For a configuration, the SHA-256 of its TOML matches the list's entry.

A failure says "The community list could not be checked" and uses nothing. There is no certificate pinning: TLS
already does its job, and pinning would break corporate proxies. The signature is what makes the content ours.

### 5.6 What the server does with every report

1. **Size**: body at most 64 KiB (usage) or 256 KiB (crash). Otherwise 413.
2. **Signature and time** as in §5.4. Otherwise 401.
3. **Install**: known, or new with valid work. A blocked install gets 403.
4. **Rate**: the Workers rate-limit binding, keyed by install (and, on the first request, by the /24 or /48 of the
   address, used only in memory). The binding is per Cloudflare location and eventually consistent: it guards against
   abuse, it does not count exactly
   ([docs](https://developers.cloudflare.com/workers/runtime-apis/bindings/rate-limit/)). Otherwise 429 with
   `Retry-After`.
5. **Schema**: the exact schema of that report version. Unknown fields are refused. Otherwise 400, and the app drops the
   report because it will never pass.
6. **Version**: a version the status says is no longer collected gets 410, and that app stops sending until it is
   updated.
7. **Build**: `exe_sha256` is looked up in the known builds. The report is stored with `build = official` (a known
   hash), `source` (the app says it was built from source) or `unknown` (it claims official but the hash is not ours).
8. **Store** with its trust flags: `build`, the install's age in days, and whether its rate was limited recently. The
   dashboards count only `official` builds from installs older than one day unless asked otherwise. A crash's backtrace
   is checked against that build's functions when it is read (§7.6).

### 5.7 What remains possible, and why that is acceptable

Someone who sets out to can make installs (paying the proof of work), report the official hash and send plausible daily
reports. What it costs us is skewed charts, never a security problem: reports start nothing, and they are only read.
Distinct installs, install age, trimmed outliers and the `unknown` flag make such a campaign visible. The status switch
stops collection while we clean up. The data is for deciding what to build and fix. Nothing pays out because of it.

## 6. Usage statistics

### 6.1 What is sent

One report a day, made from counters the app keeps on disk during the day (`~/.quickai/reports/today.json`):

```json
{
  "schema": 1,
  "product": "quickai",
  "day": "2026-10-30",
  "app": { "version": "0.3.0", "channel": "official", "target": "x86_64-unknown-linux-gnu", "package": "appimage",
           "commit": "b978bde", "exe_sha256": "9c1e…" },
  "computer": { "os": "linux", "os_version": "24.04", "arch": "x86_64", "memory_gb": 32, "gpus": ["amd"], "language": "en" },
  "use": {
    "minutes_open": "60-180",
    "starts": 2,
    "unclean_exits": 0,
    "opened": { "add_model": 2, "recommended": 1, "settings": 1, "configurations": 0, "help": 0, "chat": 3 },
    "features": ["live_traffic", "remote_run", "keep_chats"],
    "models_added": { "recommended": 1, "lookup": 2, "link": 0, "import": 0 }
  },
  "runs": [
    { "engine": "llama.cpp", "build": "b11889-vulkan", "model": "catalog:qwen3-8b-q4_k_m", "where": "local",
      "started": 3, "ready": 3, "failed": 0, "ready_seconds_median": 9 }
  ],
  "community": [ { "config": "vision-chat-16gb", "version": 3, "worked": true } ]
}
```

| Field | Why |
|---|---|
| `app` | which versions are in use and when an old one can be dropped; is a crash already fixed |
| `computer` | which systems, memory sizes and GPUs to build and test for, and which models to recommend. Coarse on purpose (§6.4) |
| `use.opened`, `features` | what is used and what is never found |
| `models_added` | how people get models |
| `runs` | which engines, builds and models start, how often they fail, and on what. This is the most useful field. It drives the recommended list (WP4.40 was done by hand) |
| `community` | "worked on N computers like yours" (§10.3) |

Timing fields are medians in whole seconds. `tokens_per_second_median` waits until the app measures it itself.

The country is not sent by the app. The server takes it from `request.cf.country` and stores the two letters; the IP
address is never stored (*choice 7*).

### 6.2 Never sent

Prompts, replies, chats, pictures; file names and paths; words typed into a search or a box; URLs typed; host names,
user names, addresses of remote computers; the names of models that are neither on the recommended list nor a public
repository; the text of command-line arguments or environment lines; the log; the contents of a configuration; anything
from Open WebUI or ComfyUI's pages; exact memory or exact times of day.

### 6.3 Which models are named

A model appears as `catalog:<id>` when it is on the recommended list, as `hf:<owner>/<repo>` when
`model-sources.json` says it came from a Hugging Face look-up (a public search result), and as `ollama:<name>` when it
is in Ollama's public library. Everything else is `other`, with its size bucket (`<2 GB`, `2-5`, `5-10`, `10-20`,
`20+`). A file a person made or renamed is never named.

### 6.4 The hardware class

One vocabulary for telemetry, the community filters and "computers like yours". It is computed in `quickai-core` from
what the app already reads (`gpu::detect`, `total_ram_bytes`):

```
<os>-<arch>-<gpu vendors, sorted, or "cpu">-<memory bucket>        e.g. linux-x86_64-amd-32gb, macos-aarch64-apple-16gb
memory buckets: 8gb (up to 12), 16gb (up to 24), 32gb (up to 48), 64gb (up to 96), 128gb+
```

No GPU model name, no driver version, no screen, no CPU model. These would add little and fingerprint a lot. VRAM can
be added as a bucket once the app reads it.

### 6.5 When it is sent

- At most once a day: the first time the app has been open for two minutes on a new day, it sends the report of the
  last day it was used. A day the app was not opened sends nothing.
- From the outbox (`~/.quickai/reports/outbox/`, at most 50 files and 5 MB, oldest dropped first). A failed send waits
  1 hour, then 6, then 24, then is dropped after 3 days.
- In a background thread with a 15-second timeout. The window never waits for it.
- After sending, the report is moved to `sent/` (the last 100). Settings shows them.

### 6.6 Turning it on, off, and off for good

- **The question.** Asked once, in the second session (the first run is about getting a model), as a card in the main
  window: *Help make QuickAI better? Once a day QuickAI can send EverybodyAI counts of what it did, such as which
  engine ran and whether a model started, and a rough description of this computer. Never your chats, files or
  searches.* The card has three buttons of equal weight: **Send usage statistics**, **No thanks** and **See what is
  sent**. Nothing is ticked beforehand. Closing the card counts as *No thanks*.
- **Settings → Privacy** (a new page of the sidebar, C190):
  - the usage switch;
  - *Crash reports: Ask each time / Always send / Never*;
  - *What is sent* (the next report, as text);
  - *Reports sent* (the sent log);
  - *Your install id* with a copy button;
  - *Download what EverybodyAI holds*, *Delete what EverybodyAI holds* and *Start a new install id*;
  - a link to the policy.
- **Off for good**, in order of strength:
  1. A build without the cargo feature `reports`. Packagers build with `--no-default-features --features
     remote-setup`. The page then says "This build of QuickAI cannot send reports."
  2. A policy in the package's configuration layer (`<exe>/config/`), `reports = "off"`, which greys the switches out
     with "Turned off by whoever installed QuickAI". This is for organizations.
  3. `QUICKAI_REPORTS=off` or `DO_NOT_TRACK=1` in the environment.
- **`quickai-run`** never asks and sends nothing unless its configuration or a flag says so (`--reports usage,crashes`).
  It writes crash files locally, and `quickai-run report list|show|send|delete` handles them.
- **A change of what is collected** (a new field of the schema) raises the policy version, and the card asks again
  people who said yes. People who said no are not asked again.

## 7. Crash and problem reports

### 7.1 Kinds

| Kind | When | Asked |
|---|---|---|
| `panic` | a Rust panic in any thread of the app | on the next start (or at once, when it is a worker thread and the window still runs) |
| `engine_exit` | llama-server, Ollama, ComfyUI or Open WebUI exits during load or while serving, not by the app's Stop | a **Send a report…** button on the error notice the app already shows. Never a popup |
| `unclean_exit` | the last session did not close cleanly and left no panic: a native crash, the OOM killer, a forced kill, a power cut | not asked: it is counted in the usage report (`unclean_exits`), because asking after every power cut is noise |
| `stall` | the window stopped answering for 5 s (`ui-stalls.log`, as now) | attached to a report when the person ticks it |
| `problem` | **Help → Report a problem…**: what the person writes, an optional email for an answer, optional diagnostics | the person is writing it |

### 7.2 Capturing

- **Panic hook** (installed first thing in `main`): it writes `~/.quickai/reports/crashes/<id>.json` atomically, with no
  network and nothing slow, then calls the default hook. The hook must not panic. It formats with what it has and
  writes at most 64 KiB.
- **Session marker**: `~/.quickai/reports/running` holds the pid, the version and the start time. It is written at start
  and removed on a clean exit. If it is found at start, its pid is gone, and no panic file names that session, the last
  session was an unclean exit.
- **Engine exits**: the runtime already knows the exit status and the phase (installing, loading, serving). The report
  is built when the person presses the button, from that and the redacted tail of the engine's log.

### 7.3 What a report holds

```json
{
  "schema": 1,
  "id": "7d4c2f0e-9a71-4c1b-8d22-0b6f1e2a3c4d",
  "kind": "panic",
  "happened": "2026-10-30T09:12:44Z",
  "app": { "...": "as in the usage report" },
  "computer": { "...": "as in the usage report" },
  "panic": {
    "message": "called `Option::unwrap()` on a `None` value",
    "location": "crates/quickai/src/app/catalog_view.rs:412:31",
    "thread": "main",
    "backtrace": ["quickai::app::catalog_view::CatalogView::show", "quickai::app::shell::…", "…"]
  },
  "state": { "engine": "ollama", "engine_state": "running", "open_windows": ["recommended"], "minutes_open": 42 },
  "engine_exit": null,
  "log_tail": null,
  "note": null
}
```

An `engine_exit` has `{ engine, build, phase, status ("exit 1", "signal 9"), after_seconds, model (as §6.3), settings
(the engine's field ids that differ from the default, with values only for numbers and choices: free text is
"set") }`. `log_tail` is the last 50 lines, cleaned (§7.4), only when the person kept them in the dialog. `note` is what
the person typed.

### 7.4 Cleaning

Pure, in `quickai-core`, held by a table test. It runs before a report is shown, so what the person sees is what is sent:

| Found | Becomes |
|---|---|
| the home directory in any spelling (`/home/name`, `/Users/name`, `C:\Users\name`, escaped `\\`) | `~` |
| the user name, the host name | `<user>`, `<host>` |
| addresses of remote computers in the settings; IPv4 and IPv6 addresses other than loopback | `<remote>`, `<ip>` |
| email addresses | `<email>` |
| a URL's query string | removed (tokens live there) |
| a value whose variable name is a secret's (`plan::is_secret_name`, the rule configurations already use), and known token shapes (`hf_…`, `ghp_…`, `github_pat_…`, `sk-…`) | `<secret>` |
| a model file name that §6.3 would not name | `<model file>` |

Cleaning cannot be perfect: a log line can carry a prompt when an engine runs verbose. That is why the log tail is never
sent unless the person keeps it, after seeing it.

### 7.5 Asking

On the next start after a panic, a window of its own (C194) opens:

> **QuickAI closed unexpectedly last time.**
> A report can help us fix it. It holds the error, where in QuickAI it happened, the version and a rough
> description of this computer, never your chats or files.
> [View the report] ☐ Include the last lines of the log (shown below; untick lines you'd rather keep)
> **[Send]** **[Don't send]** ☐ Send crash reports without asking

*Send* moves it to the outbox. *Don't send* leaves it in `crashes/`, where a person can still send it from the sent log's
page, or attach it to a GitHub issue. Files there older than 90 days are deleted.

### 7.6 Reading a crash: symbols and grouping

- **Function names in release builds.** Change `[profile.release] strip = "symbols"` to `strip = "debuginfo"`. Backtraces
  then carry function names; the panic's own `file:line:col` is exact already. The size cost is measured first
  (spike S-R2). Line numbers in every frame would need split debug info per target, kept privately in R2 by build. That
  comes later, if the names prove too little.
- **Grouping.** A fingerprint is SHA-256 of: for a panic, the kind, the location without its column and the first three
  frames inside QuickAI's crates; for an engine exit, the engine, the phase, the kind of status and the build's backend.
  The admin lists groups, with counts, distinct installs, versions, first and last seen, and status (new, known,
  fixed in a version, with the GitHub issue).
- **Plausibility.** The backtrace's QuickAI functions must exist in that version (the admin keeps the list of function
  names per release, made from the symbols at build time). A report whose frames are not functions of its version is
  flagged.

### 7.7 Left out: memory dumps

Minidumps would catch native crashes (a GPU driver taking the window down). They also carry stack memory, and that can
hold what the person typed. They are left out of v1. If `unclean_exits` shows many native crashes, a separate decision
can add dumps that hold registers and module lists only, with no memory.

## 8. The API: `api.everybodyai.dev/v1`

| Method and path | Signed | What | Answer |
|---|---|---|---|
| `POST /v1/usage` | install | the day's report; a resend for the same day replaces it | 202 |
| `POST /v1/crash` | install | one crash or engine report; its id makes a resend a no-op | 202 |
| `POST /v1/problem` | install | Report a problem: text (at most 4000 characters), optional email, optional diagnostics | 202 |
| `GET /v1/me` | install | everything held for this install, as JSON | 200 |
| `DELETE /v1/me` | install | delete everything held for this install, now | 200, with a receipt |
| `GET /v1/status` | answer signed | what is collected; the lowest app version whose reports are taken; the work bits; the server's time | 200 |
| `GET /v1/community/index` | answer signed | the published configurations (§10.9) | 200 |
| `GET /v1/community/configs/{id}/{version}` | answer signed | one configuration | 200 |
| `POST /v1/community/drafts` | install | a configuration the person wants to share; answers with a claim address valid for 24 h | 201 |

**Answers.**
- 400: the schema refused it, and the app drops the report.
- 401: the signature or the time is wrong. The app corrects its clock once, then gives up.
- 403: the install is blocked. The app stops sending for 30 days.
- 410: this version is no longer collected. The app stops until it is updated.
- 413: too big.
- 429 with `Retry-After`.
- 5xx: retried as in §6.5.
- An error body is `{ "error": "<code>", "message": "<text for a person>" }`.

Community `GET`s carry no install key and no identifying header. Browsing and fetching are not tied to an install.
Downloads are counted per configuration only. The app's `User-Agent` stays `quickai/<version>`, as for its other requests.

**Inside the Worker.**
- The free plan allows 10 ms of CPU per request and 100 000 requests a day
  ([limits](https://developers.cloudflare.com/workers/platform/limits/)). An Ed25519 check and a JSON schema check
  fit, and S-R1 measures it. Symbolication does not happen here: it is done when the admin reads a report.
- A crash report's body goes to R2 under `crash/<install>/<report id>.json`. That prefix makes deletion one list and one
  delete. The row in D1 has the fields the lists need.
- Writes are batched into one D1 batch per request.
- No `console.log` of a body, a header or an address. Workers Logs (`observability`, on for the site's Worker) must be
  checked for what it records about requests (spike S-R3). It is off for `api` unless it records nothing personal.
- The daily jobs are Cron Triggers. Overnight they fold yesterday's rows into `rollups` (no install ids), sign the
  community list again, apply the retention of §11 and email the owner about new crash groups and spikes. The email
  goes through the `MAILER` binding the site already has, to its verified address.

## 9. The admin: `admin.everybodyai.dev`

**Access.** A Cloudflare Access application covers the whole host. Access is free for teams under 50 users. The policy
is the owner's email (and later the moderators'), and sign-in is with GitHub or a one-time PIN. The Worker also verifies
the `Cf-Access-Jwt-Assertion` header (with `jose`, against the team's certificates and the application's audience tag)
and refuses a request without it. A route set up wrong then still exposes nothing. Roles (`owner`, `moderator`) are
kept in D1 by Access email. Every change is written to an audit log.

**Pages.**

| Page | What |
|---|---|
| Overview | installs reporting today, this week and this month; new installs; versions; the share of official builds. Always with "*n* installs reporting", because this is a sample of those who said yes, not all users |
| Systems | OS, architecture, memory buckets, GPU vendors, languages, countries (buckets under 20 installs are shown as "other") |
| Engines and models | runs, failure rate and time to ready by engine, build, model and hardware class. This is the evidence for the recommended list |
| Features | what is opened and used |
| Crashes | groups with counts, versions and status; a report's page (cleaned report, backtrace, trust flags, the same group's other reports); a button that opens a GitHub issue pre-filled with no personal data |
| Problems | what people wrote with *Report a problem*, and an answer by email when they gave one |
| Community | the moderation queue, flagged configurations and reviews, users (limit, ban), categories |
| Releases | add a release (§5.3); known builds |
| Installs | look up an install id (for a deletion asked by email); block an install |
| Switches | what is collected; the lowest version whose reports are taken; the work bits; retention |
| Site | the waitlist and contact messages already in D1 |
| Audit | who changed what |

The charts are drawn from `rollups`. The pages are server-rendered by the Worker with small inline charts, so the admin
needs no front-end framework, and the CSP stays strict like the site's.

## 10. The community: `quickai.everybodyai.dev`

### 10.1 What a visitor sees

- **Home**: a search box, the categories, *Popular this month*, *New*, and *Fits computers like…* (pick a hardware class).
- **A category** (`/c/coding`): a list with filters (engine, hardware, system, *no custom arguments*) and a sort (rating,
  worked on, newest).
- **A configuration** (`/config/vision-chat-16gb`):
  - the title, author, summary and categories;
  - badges: the engine; *Sets its own command-line arguments*, shown prominently when it does; *Reviewed by
    EverybodyAI*;
  - what it runs: the models with sizes, sources and licences, and the build;
  - its settings, by the engine documents' names, with free text shown in full;
  - *Worked on* by hardware class;
  - the stars and reviews;
  - the versions and what changed;
  - **Download** (the `.toml`);
  - *How to use it in QuickAI*.
- **Share** (signed in): upload a `.toml`, or finish one shared from the app.
- **Me**: my configurations, my ratings, delete my account.
- **Guidelines**, **Terms**.

The pages are server-rendered (Astro with the Cloudflare adapter), so that a search for "llama.cpp settings 8 GB" can
find them. They use the site's tokens and IBM Plex. The pages carry no analytics, like the site.

### 10.2 Categories

Three axes; only the first is chosen by people:

| Axis | Values | Set by |
|---|---|---|
| Use | Chat, Coding, Reasoning and maths, Pictures in (vision), Pictures out, Video, Documents and search (Open WebUI), Embeddings, Small and fast, Long context | the author (one to three); a moderator can change them |
| Engine | llama.cpp, Ollama, ComfyUI (+ Open WebUI beside) | read from the file |
| Hardware | the classes of §6.4, folded to vendor and memory | the file's `made_on`, then the "worked on" reports |

Categories are rows in D1, so adding one is an admin action, not a release.

### 10.3 Ratings and "worked on"

- **Stars** (1 to 5) and an optional review (at most 1000 characters). They need a signed-in account; there is one per
  account and configuration, which its author can change. Authors cannot rate their own. A moderator can hide a review.
  The list sorts by a Bayesian mean: with the site's mean `m` and `C = 5`, score = (C·m + Σ stars) / (C + n). One
  5-star rating does not top the list.
- **Worked on.** When an install with usage statistics on starts a configuration that came from the community, the next
  daily report says whether the engine reached ready (§6.1). The server counts one result per install and configuration
  (the latest wins), by hardware class: "worked on 37 computers, 9 like yours". It is objective and needs no account.
  It is shown apart from the stars, never mixed in.
- **Abuse**:
  - accounts younger than 7 days rate but are not counted until then;
  - rate limits per account;
  - Turnstile on the forms;
  - a spike of ratings for one configuration is flagged in the admin.

### 10.4 Accounts

- **GitHub sign-in** in v1 (*choice 4*). It uses an OAuth app owned by the `everybodyai` organization and asks for no
  scope, which gives the public profile only. Kept: GitHub's numeric id, login, display name and avatar address.
- **Sessions.** A random 32-byte session id, stored hashed, in the cookie `__Host-session` (HttpOnly, Secure,
  SameSite=Lax, 30 days). It is strictly necessary, so the site needs no cookie banner. The site's policy changes from
  "no cookies" to "one sign-in cookie, only if you sign in on quickai.everybodyai.dev".
- **Email sign-in is left out** for now. The `send_email` binding the site uses sends only to verified addresses, so
  sign-in links would need another mail provider.
- **Deleting an account** removes the user and their ratings and reviews. It asks whether to also remove their
  configurations; otherwise those stay, shown as by "a former member" (they are CC0, §10.10).

### 10.5 Sharing

- **From the app.** Configurations → a configuration → **Share with the community…**.
  1. The app makes the file the export already makes (the allow-list; review fields only if ticked) and shows it in full.
  2. A line says "You'll finish on quickai.everybodyai.dev, signed in with GitHub".
  3. *Upload* sends it as a draft, signed by the install key.
  4. The answer is a claim address with a one-time token, valid for 24 hours, which the app opens in the browser.
  5. There, the person signs in, adds a title, a summary and categories, and submits.

  The draft carries no account, only the keyed hash of the install (for rate limits). An unclaimed draft is deleted
  after 24 hours. The app needs no sign-in of its own.
- **From the site.** Upload a `.toml` on *Share*.
- **A new version** of one's own configuration replaces nothing. It is version *n+1*, with a line saying what changed.

### 10.6 Checking a submission

1. At most 256 KiB, UTF-8, TOML, `kind = "profile"`, a schema the server knows.
2. **The same checks as the app's import** (configurations.md §8): limits, names, the deny-list, values within their
   field's type and range, unknown settings. The way to keep one set of rules is to run `quickai-core`'s own check,
   compiled to WebAssembly. Its dependencies are pure Rust (`regex`, `serde`, `toml`, `toml_edit`, `semver`, `sha2`), and
   the core already does no I/O. Spike S-R1 checks that it builds for `wasm32-unknown-unknown` and fits a Worker's CPU
   and size. If it does not fit, the check runs at moderation through the native `quickai configuration` command.
3. **No secrets, no paths**, checked again: values that look like a token or the path of a computer are refused, and the
   author is told which line.
4. **Public sources only.** Each model is a catalog id, an Ollama library name or a Hugging Face repository. At
   moderation, the admin checks the repository once (public, not gated) and shows its licence.
5. **`needs_review`**: any value of a `review` field (free text that reaches a command line or an environment, wider
   access) marks the version. It is always moderated, and the page shows the badge.
6. **A duplicate is found by what it runs, not by its bytes**: the same configuration with its arguments, environment
   lines or keys in another order is the same configuration, and is not stored twice (§10.11).

### 10.7 Moderation

- **States**: `draft` → `pending` → `published` or `rejected`; `published` → `withdrawn` (for safety) or `removed`
  (by its author).
- **In v1 everything is moderated** (*choice 5*). Later, a clean version (no `needs_review`) from an account in good
  standing with published work could go out at once, still shown as unreviewed until a moderator looks.
- **Reporting.** A report button on every configuration and review feeds the flags queue.
- **Withdrawing.** A version withdrawn for safety leaves the next signed list. That list also carries `withdrawn:
  [{id, version, reason}]`, so an app that imported it shows a note on that configuration (§10.9).

### 10.8 Publishing is signing

Publishing gives the version its number, writes the envelope (§5.5) with the content key around `{ id, version,
published, author, sha256, toml }`, and signs the community list again. A published version never changes; a fix is a
new version.

### 10.9 In the app

- **Browse.** Configurations → **Community…** opens a window of its own (C194). It fetches the signed list, only on that
  click (PRIVACY.md's rule), and filters it on the computer. *Fits this computer* uses the local hardware class, so
  nothing about the computer is sent to browse.
  - A row shows the title, author, engine, stars, "worked on N like this one", and badges.
  - **Get…** fetches the envelope and checks it (§5.5). Then **the import review the app already has** runs: the notes,
    and the free text shown in full and taken only when ticked. **A signature never skips the review.**
- **Where it came from.** An imported configuration keeps `[meta] community = { id = "…", version = 3 }`, a new key of
  the `profile` schema. Its export keeps it, so a configuration made from another can say "based on".
- **Worked or not.** If usage statistics are on, the first start of that configuration adds its result to the day's
  report, where *What is sent* shows it.
- **Withdrawn.** When the signed list says a version was withdrawn, the configuration's row shows a warning note with
  the reason. The app changes nothing by itself.
- **A `.toml` downloaded from the site** is just a file. It is imported as today, with no signature to check and the
  full review.

### 10.10 Licence, terms, content policy

- **What is shared is CC0 1.0** (*choice 6*): settings are hardly copyrightable, and CC0 removes the doubt so that anyone
  can use, change and share again. A review stays its writer's, licensed to us to show it.
- **Terms of use**:
  - no configuration meant to harm a computer, open it to the network or hide what it does;
  - models must be lawful to fetch from where they are;
  - no secrets, no personal data;
  - we may refuse, withdraw or remove content, and say why.
- **A content policy for pictures and video** (*choice 11*): whether configurations for adult image models are allowed,
  hidden behind a switch, or refused.

### 10.11 One copy of each configuration

The user, 2026-10-30: "also need to ensure that the configuration that is sent - e.g. he same cfg sent by many people -
we have to retain only one - this would mean taking care of the order of arguments etc".

Two files are the same configuration when they would run the same thing. The server keeps one copy of each. It finds
duplicates by the hash of a **canonical form**: what the file runs, written one way only.

**The rule that bounds it: missing a duplicate is acceptable; merging two different configurations is not.** Where the
canonical form cannot be sure that two spellings run the same, it keeps the difference. An argument whose meaning
depends on its position is the usual case.

**Left out of the canonical form** (none of it changes what runs):
- the formatting: the order of keys and tables, inline or not, whitespace, comments, quoting, `16_384` or `16384`;
- the name, the id, `[meta]` (note, `made_with`, `made_on`, `community`) and `requires`;
- `[runtime] version`, which is only what the program said about itself (the `build` label is kept);
- a model's `size`, and its `name` for people (its identity is kept);
- settings whose field says `identity = false` in its engine document. This is a new key of `[[field]]`, set on the
  ports: two configurations that differ only in their port run the same model the same way. The code names no field
  (configurations.md §2);
- a value equal to its field's literal default in the engine document. The app's export already leaves these out, and
  a hand-written file may spell them. A computed default (llama.cpp's context size follows the model) is never dropped.

**Normalized:**

| Part | Written as |
|---|---|
| Numbers, booleans, choices | Their field's type (`4096` and `4096.0` for an integer are 4096). A choice is written by its id |
| Models | The first stays first, because the engine loads it. The others are sorted by role, then identity. The identity is the file's SHA-256 when known (the server fills it in from the hub at moderation). Otherwise it is the source: a catalog id; an Ollama name in lower case with `:latest` added when it has no tag; a Hugging Face `owner/repo` and path, with the revision `main` written as none |
| Extra environment lines | Split into `NAME=value`, with blank and comment lines dropped and sorted by name. A name given twice keeps the last value, as the process receives it |
| Extra command-line arguments | Below |
| `[[needs]]` | Sorted by kind and name, with `why` left out |
| A companion's table | As the engine's |

**Arguments:**
1. They are split with the splitter the launch uses, so quoting is gone: `"a b"` and `'a b'` are the same word.
2. `--name=value` becomes `--name value`. A short alias becomes its long name when the engine document lists it, in a new
   optional `aliases` of its flags. An unknown flag keeps its spelling.
3. A flag takes the words after it, up to the next flag. A negative number is a word, not a flag. A flag that the
   document says takes no value takes none.
4. The groups are sorted by flag name. Repeats of one flag keep their order among themselves, because
   `--lora a --lora b` may not mean `--lora b --lora a`.
5. If a word comes before the first flag, or anything cannot be grouped, the arguments are kept exactly in their order,
   with no reordering at all.
6. A flag given twice is not collapsed into one. A flag that repeats a field (`--ctx-size` in the free text while the
   field is set) is not folded into the field. Whether the last one wins is up to the engine's parser, and the canonical
   form does not guess.

So `-c 8192 --temp=0.7 --flash-attn` and `--flash-attn --temp 0.7 --ctx-size 8192` are one configuration (with `-c`
listed as an alias of `--ctx-size`). `model.gguf --temp 0.7` and `--temp 0.7 model.gguf` are kept apart.

**The identity hash.** The canonical form is JSON with sorted keys and no spaces (RFC 8785, the JSON Canonicalization
Scheme). It carries the number of its rules (`"canon": 1`). The **identity** is SHA-256 of `quickai-config-v1\n`
followed by that JSON. It is computed in `quickai-core` (`profile/canonical.rs`), so the app, the Worker (S-R1) and
the admin compute the same thing. Shared test vectors hold it. The server never takes a client's word for it: it
computes the hash itself.

**What the server does with a duplicate:**

| The new file is the same as… | Then |
|---|---|
| A published or pending version | Nothing is stored. The person is shown the existing configuration and is added to its **Also shared by** count. People reaching the same setup independently is itself a sign of quality. Their categories become suggestions. A different title or summary goes to the moderators as a suggested edit |
| An older version of a configuration that has moved on | They are told which configuration and version it is. Nothing is stored |
| A rejected or withdrawn version | Refused, with the earlier reason, so that a refused file cannot come back under a new name |
| Their own configuration's current version, with only the title or summary changed | An edit of its text, not a new version |

A draft from the app is checked when it is uploaded. The app hears "already shared as …" at once, and nothing waits
for a claim. A unique index on the identity among pending and published versions holds the rule even when two people
submit at the same moment: the second insert fails and becomes the *Also shared by* case. Ratings, reviews and "worked
on" therefore all gather on the one copy.

**Close, but not the same.** Configurations with the same engine, models and build but different settings are
*variants*. A second, coarser hash (`family`, of the engine and the models' identities) groups them on the page
("3 variants"). Nothing is merged.

**In the app.** The signed list carries each version's identity. The app computes the identity of any of its own
configurations on the computer:
- **Share with the community…** says "Already shared as *Vision chat 16 GB*" and opens it, without uploading anything.
- The Configurations window says when a configuration is the same as a community one, with its stars and "worked on",
  whether it was fetched from the community or not.
- The daily report's "worked or not" names a community configuration only when the active configuration's identity is
  in the signed list. The identity of a configuration nobody shared is never sent: it could be guessed back from a small
  set of likely candidates.

**When the rules change** (a new alias, a new `identity = false`), `canon` goes up. A job then computes the identity of
every stored version again, and new matches go to the moderators as suggested merges. Nothing is merged without them.

## 11. Data: tables, retention, deletion

Two new D1 databases. Their migrations are in `shared/migrations/`, written to the standard of the existing
`0001_init.sql`: a comment on every table about what it keeps and why.

```sql
-- reports
CREATE TABLE installs (id TEXT PRIMARY KEY, public_key BLOB NOT NULL, product TEXT NOT NULL,
  first_day TEXT NOT NULL, last_day TEXT NOT NULL, blocked INTEGER NOT NULL DEFAULT 0);        -- days, never times
CREATE TABLE usage_days (install_id TEXT NOT NULL, day TEXT NOT NULL, version TEXT NOT NULL,
  build TEXT NOT NULL CHECK (build IN ('official','source','unknown')), os TEXT, os_version TEXT, arch TEXT,
  memory_gb INTEGER, gpus TEXT, language TEXT, country TEXT, report TEXT NOT NULL,             -- the validated JSON
  PRIMARY KEY (install_id, day));
CREATE TABLE run_days (install_id TEXT NOT NULL, day TEXT NOT NULL, engine TEXT NOT NULL, build_label TEXT,
  model TEXT NOT NULL, hardware TEXT NOT NULL, started INTEGER, ready INTEGER, failed INTEGER, ready_seconds INTEGER,
  PRIMARY KEY (install_id, day, engine, model));
CREATE TABLE crashes (id TEXT PRIMARY KEY, install_id TEXT NOT NULL, received TEXT NOT NULL, kind TEXT NOT NULL,
  version TEXT, build TEXT, hardware TEXT, fingerprint TEXT NOT NULL, flags TEXT, body_key TEXT NOT NULL);
CREATE TABLE crash_groups (fingerprint TEXT PRIMARY KEY, title TEXT, first_seen TEXT, last_seen TEXT,
  reports INTEGER, installs INTEGER, status TEXT, fixed_in TEXT, issue_url TEXT, notes TEXT);
CREATE TABLE problems (id TEXT PRIMARY KEY, install_id TEXT, received TEXT, text TEXT, email TEXT, diagnostics TEXT,
  answered INTEGER NOT NULL DEFAULT 0);
CREATE TABLE releases (sha256 TEXT PRIMARY KEY, version TEXT, commit_id TEXT, target TEXT, package TEXT, added TEXT);
CREATE TABLE rollups (day TEXT, metric TEXT, dimension TEXT, value INTEGER, installs INTEGER,
  PRIMARY KEY (day, metric, dimension));                                                       -- no install ids
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
  made_on TEXT, needs_review INTEGER, changes TEXT, state TEXT, reviewed_by INTEGER, envelope TEXT,
  identity TEXT NOT NULL, family TEXT NOT NULL, canon INTEGER NOT NULL,                       -- §10.11
  PRIMARY KEY (config_id, version));
CREATE UNIQUE INDEX one_copy ON config_versions (identity) WHERE state IN ('pending', 'published');
CREATE TABLE config_sharers (config_id TEXT, user_id INTEGER, at TEXT, PRIMARY KEY (config_id, user_id));  -- "also shared by"
CREATE TABLE refused (identity TEXT PRIMARY KEY, reason TEXT, at TEXT);    -- a refused file cannot return under a new name
CREATE TABLE ratings (config_id TEXT, user_id INTEGER, stars INTEGER CHECK (stars BETWEEN 1 AND 5), review TEXT,
  created TEXT, updated TEXT, hidden INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (config_id, user_id));
CREATE TABLE outcomes (config_id TEXT, version INTEGER, install_hash TEXT, hardware TEXT, worked INTEGER, day TEXT,
  PRIMARY KEY (config_id, install_hash));                    -- install_hash = HMAC(secret, install id): no join with reports
CREATE TABLE drafts (token_hash TEXT PRIMARY KEY, install_hash TEXT, toml TEXT, expires TEXT);
CREATE TABLE flags (id INTEGER PRIMARY KEY, config_id TEXT, rating_user INTEGER, reporter INTEGER, reason TEXT,
  created TEXT, resolved TEXT);
```

**Retention** (applied every night by the Cron job, and stated in the policy):

| Data | Kept | Then |
|---|---|---|
| `usage_days`, `run_days` | 13 months | folded into `rollups`, which hold no install id |
| crash reports (row and R2 body) | 180 days | the group keeps its counts |
| crash groups | until a year after they are closed | |
| local crash files | 90 days | deleted by the app |
| problem reports | two years after the last message (as the site's contact form) | |
| `installs` | 13 months after the last report | deleted with everything else of it |
| drafts | 24 hours | |
| sessions | 30 days | |
| `outcomes` | 13 months | folded into the counts |
| accounts, ratings, configurations | while the account exists | §10.4 |
| `deletions`, `audit` | 2 years | |
| Cloudflare's own logs | Cloudflare's terms | |

**Deletion from the app.** *Delete what EverybodyAI holds* sends a signed `DELETE /v1/me`. The server deletes:
- the install;
- its `usage_days`, `run_days`, `crashes` and `problems`;
- the R2 objects under `crash/<install>/`;
- the community `outcomes` and `drafts` with its keyed hash.

The `rollups` hold no install id and stay. The server writes a receipt and answers with it. The app then makes a new key,
so anything sent later is not tied to the old data. A test lists every table with an install column and fails if one is
missing from the deletion.

**Download from the app.** `GET /v1/me` returns every row and object of the install as one JSON file, saved where the
person chooses.

**Where the data is.** Choose an EU location for D1 and R2 where Cloudflare offers one (R2 has an EU jurisdiction;
check D1's current options), so that the policy can say where it is.

## 12. The policy and the other documents

All of these land before, or in, the first release that can send anything:

| Document | Change |
|---|---|
| `PRIVACY.md` | *In short* rewritten; a new section *Reports you choose to send*; two bullets under *When the app uses the internet* (the reports, the community); *What the app does with your data* gains the files under `~/.quickai/reports`; the date. Draft in Appendix A |
| `everybodyai.dev/privacy` | now covers the API, the admin and the community site: the sign-in cookie, GitHub as the sign-in provider, ratings and reviews being public, retention, and the processors (Cloudflare; GitHub for sign-in) |
| `quickai.everybodyai.dev/terms`, `/guidelines` | new: §10.10 |
| `SECURITY.md` | *No unexpected network traffic* adds the reports, only when turned on or confirmed, only to `api.everybodyai.dev`. New promises: a report never carries prompts, replies, paths or secrets (one that does is a bug); a community configuration whose signature fails is not used; a signed one is still reviewed. The web services are in scope |
| About window | the disclaimer names `api.everybodyai.dev` and when it is contacted (TC-ABT-004 and a new case) |
| Help | a page *Reports and privacy*, and one *Community configurations* |
| `README.md` | the privacy line; how to check a download (§5.3) |
| `THIRD_PARTY_LICENSES.md` | `ed25519-dalek` and its dependencies (BSD-3-Clause; to be checked when added) |
| `docs/configurations.md` | §8's "Provenance is not claimed" now points to §10 here; `[meta] community` in §3 |
| store listings | the Microsoft Store and Snap Store data declarations and privacy links |
| `COMMERCIAL.md` | organizations can switch reports off for good (§6.6), and reports are never used to check a licence |

**The law, to be confirmed by a lawyer.** This is not legal advice. Consent is the basis for reports. Under the GDPR and
the UK GDPR it has to be free, informed, specific and as easy to withdraw as to give, and the ePrivacy rule on storing
and reading information on a device points the same way. The policy needs to name:
- the controller (EverybodyAI);
- the processors: Cloudflare under its data processing addendum, and GitHub for sign-in;
- international transfers;
- retention;
- the person's rights, and how to use them without an account (the install id);
- a complaint to a supervisory authority.

California's law asks for a notice at collection and a statement that nothing is sold or shared. The rules where
EverybodyAI is established apply as well.

## 13. Changes in the app

**`quickai-core`** (pure; tested by tables and golden files), a new `report` module:
- `schema.rs`: the report types (`serde`, `deny_unknown_fields`, a schema number each);
- `redact.rs` (§7.4);
- `hardware.rs` (§6.4);
- `day.rs`: the day's counters (add, merge, bucket);
- `sign.rs`: the request's signed bytes, the content envelope's check, key ids, rollback and expiry.

Also in `quickai-core`, the configuration's canonical form and identity (`profile/canonical.rs`, §10.11). The engine
documents gain two optional keys for it: `identity` on `[[field]]` and `aliases` of flags.

`ed25519-dalek` (verification only) is pure computation, so the purity test can allow it. Making a key needs randomness,
and that is the runtime's job.

**`quickai-runtime`**, a new `reports` module:
- `keys.rs`: the install key, 0600, made on the first send, replaced on demand;
- `outbox.rs`: atomic files, the caps, `sent/`;
- `sender.rs`: the background thread, `ureq` with a 15 s timeout, the waits of §6.5, the cached status, the work;
- `crash.rs`: the panic hook, the session marker, engine exits;
- `community.rs`: the list and a configuration, fetched and checked, and the highest version seen;
- `policy.rs`: whether anything may be sent, from the feature, the package's layer, the environment and the settings.

**`quickai`** (the window):
- the card of §6.6;
- Settings → Privacy;
- the crash window (§7.5);
- *Send a report…* on an engine's error notice;
- Help → *Report a problem…* (a window of its own);
- Configurations → *Community…* (a window of its own) and *Share with the community…*;
- the About text.

The cargo feature `reports` is on by default.

**`quickai-run`**: `--reports`, the configuration key, and `report list|show|send|delete`.

**Settings keys**: `reports.usage` (absent = off), `reports.crashes` (absent = ask) and `reports.asked` (the policy
version the card was answered for). A person who is never asked keeps a settings file without them.

**Build identity**: the build scripts set `QUICKAI_CHANNEL=official` and `QUICKAI_COMMIT`, which `option_env!` reads. A
build from source says `source`.

## 14. Changes in the release pipeline (`quickai_base`)

1. Make the release key once, offline (`ssh-keygen -t ed25519 -C release@everybodyai.dev`). Publish `allowed_signers`
   in the `quickai` repository and on the site.
2. The builds write `binaries.json` (§5.3) and list it in `SHA256SUMS`.
3. At release the owner signs `SHA256SUMS`, uploads `SHA256SUMS.sig`, and adds the release in the admin.
4. The build scripts set the channel and the commit (§13).
5. Set `strip = "debuginfo"` after S-R2. Keep each release's list of QuickAI function names for §7.6.
6. Update the store listings' data declarations (§12).

## 15. Tests

- **Rust, core**:
  - the schemas' golden JSON;
  - the cleaning table (every row of §7.4, and lines that must not change);
  - the hardware class;
  - **the canonical form** (§10.11). The same configuration spelled differently must give one identity: arguments
    regrouped and reordered, `--a=b` or `--a b`, an alias, environment lines shuffled, keys and tables reordered,
    comments, quoting, a default spelled out, another port. A configuration whose meaning changed must give another
    identity: a positional argument moved, `--lora` repeats swapped, any value changed, another model, another build.
    A property test shuffles the flag groups of generated argument strings. The vectors are shared with the Worker;
  - the signed bytes and signatures against **shared test vectors** (`shared/test-vectors/*.json`: keys, messages,
    signatures, envelopes, a rolled-back list, an expired list, an unknown key id), used by both Rust and TypeScript
    so that the two sides cannot drift.
- **Rust, runtime**, against a `tiny_http` fake of the API, as the project's other network code is tested:
  - **with the defaults, no request is made** (the promise of PRIVACY.md);
  - the card's *No thanks* sends nothing;
  - the outbox caps and waits;
  - 400 drops a report, 410 stops sending, 429 waits;
  - the panic hook writes the file and makes no request;
  - an unclean exit is counted, not asked about;
  - the deletion makes a new key;
  - each way of switching it off for good.
- **Worker** (vitest in workerd, like the site's):
  - signatures, times and the work;
  - each answer of §8;
  - schemas with unknown fields refused;
  - deletion of every table and the R2 prefix (the table list is read from the migrations);
  - the admin refused without an Access token;
  - the community checks of §10.6 against the app's table of hostile files (A32);
  - duplicates: each row of §10.11's table, and two submissions of the same identity at once (one copy, one sharer);
  - the Bayesian sort.
- **GUI** (Robot, `quickai_test`):
  - the card;
  - the Privacy page;
  - the crash window after a forced panic (a test-only `QUICKAI_TEST_PANIC`);
  - *Send a report…* after a fake engine's exit;
  - Community… against a fake API: the list, a refused signature, the import review shown for a signed file, the
    withdrawn note.
- **Browser** (Playwright, like the site's): the community pages with axe in light and dark, sign-in with a fake GitHub,
  rating, sharing a draft.
- **Network audit**: the packaged app with default settings is run for a session with every network connection logged
  (a network namespace on Linux). Nothing goes to `everybodyai.dev`.
- New groups of the test plan: **RP** (reports) and **CM** (community). Every group is checked by breaking the code on
  purpose, as the earlier groups were.

## 16. Work packages and order

One feature in the plan's numbering, WP4.42, in parts like WP4.17. Each part lands green and is committed on its own
branch. The policy part (4.42.7) must be in the same release as the first part that can send.

| WP | Work | Repo | Size | After | Verified by |
|---|---|---|---|---|---|
| S-R1 | `quickai-core`'s configuration check as WebAssembly in a Worker: does it build, its size, CPU per check | public, everybodyai | S | | a measured note in engine-facts style |
| S-R2 | `strip = "debuginfo"`: size cost on the three systems, and what a panic's backtrace shows | public, base | S | | the numbers |
| S-R3 | what Workers Logs records of a request (addresses? headers?), and how to keep it out | everybodyai | S | | a note |
| 4.42.1 | **Keys and releases**: the release key, `binaries.json`, `SHA256SUMS.sig`, channel and commit in the build | base, public | S | | a release candidate verified with `ssh-keygen -Y verify` |
| 4.42.2 | **The core**: schemas, cleaning, hardware class, signed bytes, envelope check, test vectors | public | M | | RP core tests |
| 4.42.3 | **The API**: `/v1` usage, crash, problem, me, status; the `reports` D1 and R2; rate limits; Cron jobs | everybodyai | M | 4.42.2 (vectors) | Worker tests |
| 4.42.4 | **Sending from the app**: install key, outbox, sender, the switches, the card, Settings → Privacy, `quickai-run` | public | L | 4.42.2 | RP runtime tests, GUI cases, network audit |
| 4.42.5 | **Crashes**: the hook, the marker, engine exits, the crash window, Report a problem | public | M | 4.42.4 | RP tests, GUI cases |
| 4.42.6 | **The admin**: Access, overview, systems, engines and models, crashes, problems, releases, installs, switches, site, audit | everybodyai | L | 4.42.3 | Worker tests, Playwright |
| 4.42.7 | **The words**: PRIVACY.md, the site's policy, SECURITY.md, About, Help, README, COMMERCIAL.md, store listings; a lawyer's review | public, everybodyai, base | S | before the release of 4.42.4 | TC-ABT cases, a read-through |
| | *first release that can send: usage statistics and crash reports* | | | | |
| 4.42.8 | **One copy of each**: the canonical form and the identity in `quickai-core` (§10.11); `identity = false` on the engine documents' ports and `aliases` of their flags (the generated reference pages too); `quickai configuration identity <file>` | public | M | 4.42.2 | CM canonical tests, shared vectors |
| 4.42.9 | **The community's back end**: accounts, configurations, versions, the checks (S-R1), duplicates (§10.11), moderation, signing, the list | everybodyai | L | 4.42.3, 4.42.8, S-R1 | Worker tests incl. hostile files and duplicates |
| 4.42.10 | **`quickai.everybodyai.dev`**: the pages of §10.1, sign-in, rating, sharing, *Also shared by*, variants | everybodyai | L | 4.42.9 | Playwright, axe |
| 4.42.11 | **The community in the app**: Community…, the checks, the review, `[meta] community`, worked-or-not, Share, "already shared as" | public | M | 4.42.9 | CM tests, GUI cases |
| 4.42.12 | **Moderation pages** in the admin (suggested merges and edits among them); terms and guidelines | everybodyai | M | 4.42.9 | Playwright |
| 4.42.13 | *Optional*: a public page of numbers (buckets of 20 installs or more) | everybodyai | S | 4.42.6 | Playwright |

Decisions made while building go into the plan's decision log from C195 on, as usual.

## 17. Choices for you

| # | Choice | Recommended | Why |
|---|---|---|---|
| 1 | Usage statistics: opt-in or opt-out | **opt-in** | the brand is privacy; consent is the lawful basis in the EU and UK; an opt-out would contradict PRIVACY.md's first paragraph |
| 2 | Crash reports by default | **ask each time** | the person sees the report; "always" is one tick away |
| 3 | Hosts: `api.`, `admin.`, `quickai.` subdomains, or paths on `everybodyai.dev` | **subdomains** | separate Workers, CSPs and Access rules; the app names one host |
| 4 | Community sign-in | **GitHub only** at first | free, no passwords to keep, fits the audience; email later needs a mail provider |
| 5 | Moderation | **everything moderated** at launch | there are few submissions at first, and the risk is a harmful file |
| 6 | Licence of shared configurations | **CC0 1.0** | simplest reuse, no attribution chains |
| 7 | Store the country (from Cloudflare, not the IP) | **yes**, two letters | useful and coarse; disclosed |
| 8 | Cloudflare plan | **Workers Paid before launch** ($5 a month at the time of writing; check) | the free plan's daily D1 writes are per account and a flood would also stop the forms and the community (§18); 10 ms CPU is tight for checks |
| 9 | A public page of numbers | **yes, later** (4.42.13) | gives back; shows what is collected |
| 10 | Publish the API's source and schemas | **yes** | anyone can check what the server keeps |
| 11 | Adult content for image and video configurations | your call | it decides the guidelines and a filter |
| 12 | "Worked on" needs usage statistics on | **yes** | it is usage data, and it keeps consent simple |

## 18. Costs, limits and risks

**Cloudflare, at the time of writing:**
- **Workers Free**: 100 000 requests a day; 10 ms CPU per request
  ([limits](https://developers.cloudflare.com/workers/platform/limits/)).
- **D1 on the free plan**: 5 million rows read and 100 000 written a day, and 5 GB. Since 2026-09-01, queries fail when
  the day's limit is reached ([changelog](https://developers.cloudflare.com/changelog/post/2026-09-01-d1-free-tier-limit-enforcement/),
  [FAQ](https://developers.cloudflare.com/d1/reference/faq/)).
- **R2**: 10 GB-month, 1 million class A and 10 million class B operations a month free, and no egress fees
  ([pricing](https://developers.cloudflare.com/r2/pricing/)).
- **Access**: free for teams under 50 users ([Cloudflare Access](https://www.cloudflare.com/products/cloudflare-access/)).
- **The rate-limit binding** is generally available since 2025-09-19
  ([changelog](https://developers.cloudflare.com/changelog/2025-09-19-ratelimit-workers-ga/)).

**A rough budget.** An opted-in install writes about three rows a day (its day, its runs, its install row). On the free
plan, about 30 000 opted-in daily installs would use D1's daily writes on their own. That ceiling is shared with the
forms and the community. Crash reports are rare, and their bodies are small.

**Risks, and what holds them:**

| Risk | Held by |
|---|---|
| A report carries something personal | the schema allow-list, the cleaning table, the person seeing every report, the log tail only when kept, the tests |
| People read "telemetry" as "spying" and leave | opt-in, *See what is sent*, the sent log, the public numbers, the published server code, plain words in the card |
| Fake installs skew the numbers | proof of work, rate limits, distinct installs and age, the `unknown` build flag, the status switch (§5.7) |
| A harmful configuration is shared | moderation, `needs_review`, the app's own review even for signed files, withdrawal with a note in the app |
| The content key leaks | two key slots, rotation by release, a list that expires after 14 days; or the offline option of §5.2 |
| The account's daily D1 limit is reached | the paid plan (choice 8), batched writes, and the app's waits |
| A report fills the outbox when the server is down | caps and dropping the oldest, never blocking the window |
| The policy and the app disagree | 4.42.7 in the same release; the About test; the network audit |
| An old version keeps sending forever | 410 from the status's lowest version |
| Moderation takes more time than there is | everything moderated at first, then auto-publishing for clean files from trusted accounts |

## 19. Left out on purpose

- **An update check or announcements in the app.** Each would be a new reason to connect, with its own consent. They
  can use the same signed status later.
- **Using reports to check a licence**, now or later.
- Memory dumps (§7.7); session replays; per-prompt analytics; A/B tests.
- An account inside the app; syncing configurations between computers; a `quickai://` link (still left out, as
  configurations.md says); threads of comments or a forum. Reviews are enough to start.
- Tokens per second, until the app measures it.

## Appendix A: draft text for PRIVACY.md

*In short* becomes:

> QuickAI sends nothing to its developer unless you tell it to. Two things are **off until you turn them on**: **usage
> statistics**, a report once a day of what the app did (which engine ran, whether a model started, which windows were
> opened) and a rough description of your computer; and **crash reports**, which the app offers after it or an engine
> fails, showing you the report before anything is sent. Both go to EverybodyAI's server, `api.everybodyai.dev`, with a
> random install id. They never include your prompts, the model's replies, your files, folders or searches. You can
> read every report before and after it is sent, download what we hold and delete it from the app, and switch either
> off at any time. The app has no accounts and no advertising, and no automatic update check. Your prompts and the
> model's replies are processed on your own computer and are never uploaded. Apart from the reports you turn on, the
> app talks to the internet only when you ask it to: to search for or download a model, a build of an engine, or a
> community configuration (see [When the app uses the internet](#when-the-app-uses-the-internet)).

A new section, after *What the app does with your data*:

> ## Reports you choose to send
>
> **Usage statistics** (Settings → Privacy, off until you turn them on). Once a day, the first time QuickAI has been open
> for two minutes, it sends the report of the last day you used it: the version and kind of package; your system, its
> version, the processor's architecture, your memory rounded to 8, 16, 32, 64 or 128 GB and more, the makers of your
> graphics cards and your system's language; how long the app was open, in ranges; which windows and features you used,
> as counts; how you added models; for each engine, model and build you ran, how many runs started, became ready or
> failed, and how long a start took. A model is named only when it is on QuickAI's recommended list, in Ollama's public
> library or a public Hugging Face repository you found through the app; any other model is "other" with its size. If
> you started a community configuration, whether it worked.
>
> **Crash reports.** When QuickAI stops because of an error, it writes a report on your computer
> (`~/.quickai/reports/crashes`) and, the next time it starts, asks whether to send it, showing it to you first. When an
> engine fails, the error has a **Send a report** button. A report holds the error, where in QuickAI it happened, the
> version, the description of your computer above and what the app was doing; the last lines of the log only if you
> keep them after reading them. Before you see it, the app replaces your home folder, user name, computer name,
> addresses, email addresses and anything that looks like a password or a token. You can choose *Always send* or
> *Never* in Settings. **Report a problem** in the Help menu sends what you write, and your email address only if you give
> it.
>
> **What they never contain**: your prompts, the model's replies or your chats; pictures; file or folder names; words you
> typed into a search or a box; the addresses of your remote computers; the text of command-line arguments or
> environment lines; your configurations; your IP address (it reaches Cloudflare, which carries the request, and is not
> stored by us).
>
> **The install id.** The first time a report is sent, QuickAI makes a key on your computer
> (`~/.quickai/reports/install.key`, readable by you only) and signs every report with it, so that nobody else can send,
> read or delete reports in your install's name. The install id is made from that key. It is not linked to your name or
> any account, but it does link your reports to one another, so the data is pseudonymous, not anonymous. Settings →
> Privacy shows it, and **Start a new install id** replaces it.
>
> **Where it goes and how long it is kept.** To EverybodyAI's server, `api.everybodyai.dev`, run on Cloudflare (Workers,
> D1 and R2) under Cloudflare's data processing terms. Daily reports are kept for 13 months and then only as totals
> without any id; crash reports for 180 days; problem reports for two years after the last message. The server records
> the country your request came from (two letters), not your address.
>
> **Your choices.** Everything sent is listed in Settings → Privacy → **Reports sent**, as it was sent.
> **Download what EverybodyAI holds** saves all of it to a file; **Delete what EverybodyAI holds** deletes it from the
> server at once and starts a new install id; turning the switches off stops sending. Without the app, write to
> hello@everybodyai.dev with your install id. You can withdraw consent at any time; it changes nothing that was lawful
> before.
>
> **Off for good.** Builds without reports exist (packagers can make them). An organization can switch reports off in the
> package's configuration, and `QUICKAI_REPORTS=off` or `DO_NOT_TRACK=1` in the environment turns them off. `quickai-run`
> sends nothing unless its configuration or `--reports` says so.
>
> **What reports are not for.** They are never sold or shared, never used for advertising, and never used to check a
> licence.

Two new bullets under *When the app uses the internet*:

> - **Reports you turned on or confirmed.** As described in [Reports you choose to send](#reports-you-choose-to-send), to
>   `api.everybodyai.dev` only.
> - **Community configurations.** **Community…** in the Configurations window asks `api.everybodyai.dev` for the list of
>   shared configurations, and **Get** fetches the one you chose. These requests carry no install id and nothing about
>   your computer: the list is filtered on your computer. Each file is checked against EverybodyAI's signature, and then
>   shown to you as any imported configuration is. Rating and sharing happen on `quickai.everybodyai.dev`, in your
>   browser, under the site's policy.
