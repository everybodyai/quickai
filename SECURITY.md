# Security policy

## Reporting a vulnerability

Please report security problems **privately**. Do not open a public issue.

- Preferred: on GitHub, open the repository's **Security** tab and choose
  [**Report a vulnerability**](https://github.com/everybodyai/quickai/security/advisories/new).
- Or email **hello@everybodyai.dev** with "SECURITY" in the subject.

Say what you found, how to reproduce it, and which version or commit you used. You will get a reply as soon as
possible, normally within a few days. The fix and the advisory will credit you unless you would rather not be
named.

## Supported versions

Security fixes go into the latest release.

## What QuickAI promises, and what counts as a vulnerability

QuickAI's design rests on a few promises. Breaking any of them is a vulnerability:

- **The servers it starts are reachable from this computer only.** llama-server, Ollama, ComfyUI and Open WebUI
  are started on `127.0.0.1`. Making one reachable from the network without the person choosing it is a bug.
- **Checksums are enforced.** Builds and recommended models are checked against a pinned or published
  checksum before they are used. A way to make QuickAI keep or run a file that fails its check is a bug.
  (A file you fetch from an address you typed has no checksum to check; it is yours to trust.)
- **A program QuickAI did not install is not started without asking.** An engine document that would start
  one asks the person first.
- **An imported configuration cannot act on its own.** Before anything is taken, the person sees in full the
  text that would reach a command line or an environment; lines that make a program load other code
  (`LD_PRELOAD`, `PATH`) are refused. A configuration that runs code, or changes a setting, without consent is
  a bug.
- **No unexpected network traffic.** QuickAI contacts the hosts listed in [PRIVACY.md](PRIVACY.md), and only when
  the person asks. Any other connection is a bug.
- **Files stay where they belong.** Engines keep their files under `~/.quickai`. Writing elsewhere is a bug.

Known and by design: the local servers have no password while they run, so any program or user **on the same
computer** can send them requests. [PRIVACY.md](PRIVACY.md) says this too.
