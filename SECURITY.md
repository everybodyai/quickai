# Security policy

## Reporting a vulnerability

Please report security issues **privately** rather than in a public issue.

- Preferred: open the repository's **Security** tab on GitHub and select
  [**Report a vulnerability**](https://github.com/everybodyai/quickai/security/advisories/new).
- Alternatively, email **hello@everybodyai.dev** with "SECURITY" in the subject line.

Please describe the issue, the steps to reproduce it, and the version or commit you used. We aim to respond as soon
as possible, normally within a few days. The fix and the security advisory will credit you, unless you prefer not to
be named.

## Supported versions

Security fixes are made to the latest release.

## Security principles and scope

QuickAI is designed to uphold the following principles. Any behavior that breaks one of them is considered a
vulnerability:

- **Local servers are reachable from this computer only.** llama-server, Ollama, ComfyUI and Open WebUI are started
  on `127.0.0.1`. Exposing any of them to the network without the user's explicit choice is a defect.
- **Checksums are enforced.** Engine builds and recommended models are verified against a pinned or published checksum
  before they are used. Any way to make QuickAI keep or run a file that fails verification is a defect. (A file
  downloaded from an address you enter yourself has no checksum to verify; trusting it is your decision.)
- **Programs QuickAI did not install are not started without consent.** An engine document that would start such a
  program asks the user first.
- **An imported configuration cannot act on its own.** Before anything is applied, the user is shown in full any text
  that would reach a command line or an environment variable, and lines that make a program load other code
  (`LD_PRELOAD`, `PATH`) are rejected. A configuration that runs code or changes a setting without consent is a defect.
- **No unexpected network traffic.** QuickAI contacts only the hosts listed in [PRIVACY.md](PRIVACY.md), and only at
  the user's request. It sends nothing automatically: a problem report reaches anyone only when the user submits it
  from their browser. Any other connection is a defect.
- **Problem reports contain no content and identify no one.** A report never includes a prompt, a response, a file or
  folder name, a search or address the user typed, a secret, or anything that distinguishes one computer from another
  (an identifier, a name or an address). A report that does is a defect.
- **Files stay where they belong.** Engines keep their files under `~/.quickai`. Writing elsewhere is a defect.

Known limitation, by design: the local servers have no password while they run, so any program or user **on the same
computer** can send them requests. [PRIVACY.md](PRIVACY.md) states this as well.

These principles describe QuickAI's intended behavior and the defects we undertake to fix; they do not constitute a
guarantee. QuickAI is provided "as is", without warranty of any kind, as set out in its [MIT License](LICENSE).
