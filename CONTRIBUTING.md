# Contributing to QuickAI

Thank you for helping. Bug reports, ideas, documentation and code are all welcome.

## Before you start

- **Found a bug or have an idea?** Open an issue using one of the templates.
- **Found a security problem?** Report it privately, as [SECURITY.md](SECURITY.md) describes.
- **Planning a larger change?** Open an issue first, so we can agree on the approach before you put in the work.

## Contributor License Agreement

QuickAI is offered under the PolyForm Noncommercial License and under commercial licenses
([COMMERCIAL.md](COMMERCIAL.md)). To keep offering both, contributions are accepted under a
[Contributor License Agreement](CLA.md). You keep the copyright of your work. Tick the box in the pull request
template to agree.

## Building and testing

[docs/development.md](docs/development.md) has the full setup. In short, with Rust stable:

```bash
cargo run --release -p quickai
```

Before you open a pull request, run the same checks as CI:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p quickai -- config check --strict --embedded
```

## How the code is organized

- **Four crates, each with a job.** `quickai-core` is pure logic (no processes, network or files),
  `quickai-runtime` does the work on the computer, `quickai` is the window, and `quickai-run` is the headless
  program. Tests hold the boundaries, so a change that crosses one will fail them.
- **Much of QuickAI is data.** Engines, the recommended models and the vocabularies are TOML documents under
  `crates/quickai-runtime/manifests/`, described key by key in [docs/reference/](docs/reference/). Adding a
  model to the recommended list, for example, is a change to a document, not to Rust.
- **The design is written down.** Read [docs/architecture.md](docs/architecture.md) before you change the kernel.

## Privacy is a feature

QuickAI makes promises about what it does not do. Any change that makes it contact the network, or store
something new, must be something the person asks for. It must say what it contacts, and it must update
[PRIVACY.md](PRIVACY.md) in the same pull request.

## Commit messages

One logical change per commit, with a short prefix: `feat:`, `fix:`, `docs:`, `refactor:`, `test:` or `chore:`.
Say what changed and why, in plain words.

## Code of conduct

Everyone taking part is expected to follow the [Code of Conduct](CODE_OF_CONDUCT.md).
