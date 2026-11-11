# Contributing to QuickAI

Thank you for your interest in contributing. Bug reports, suggestions, documentation and code are all welcome.

## Before you start

- **Found a bug or have a suggestion?** Open an issue using one of the templates.
- **Found a security issue?** Report it privately, as described in [SECURITY.md](SECURITY.md).
- **Planning a larger change?** Open an issue first, so that we can agree on the approach before you invest the time.
- **Have a question or an idea to discuss?** Start a thread in [Discussions](https://github.com/everybodyai/quickai/discussions).

## Where to start

- **[Good first issues](https://github.com/everybodyai/quickai/labels/good%20first%20issue)** are small and
  self-contained, with a pointer to the relevant code. **[Help wanted](https://github.com/everybodyai/quickai/labels/help%20wanted)**
  issues are larger pieces of work we would welcome help with.
- **Add a model to the recommended list.** The list is written in TOML under
  `crates/quickai-runtime/manifests/catalog/`, so no Rust is required: [docs/catalog.md](docs/catalog.md) explains the
  keys, and the tests validate every entry. Include the file's size and SHA-256, and make sure the model's license
  permits people to use it.
- **Test it on your hardware and share the results.** A bug report that includes your system and graphics card, or a
  note in Discussions on which models ran and how fast, is more valuable than you might expect: we cannot test every
  card.
- **Packages.** QuickAI is not yet available in the AUR, Homebrew, Scoop or Chocolatey; if you maintain packages for
  one of them, please let us know in an issue.
- **Documentation.** If anything in Help or the documentation is unclear, corrections are very welcome.

## License of contributions

QuickAI is licensed under the [MIT License](LICENSE). Contributions are licensed under the same terms as the rest of
the code, and you retain the copyright in your work. Tick the box in the pull request template to confirm your
agreement.

## Building and testing

[docs/development.md](docs/development.md) describes the full setup. In short, with stable Rust:

```bash
cargo run --release -p quickai
```

Before opening a pull request, run the same checks as CI:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p quickai -- config check --strict --embedded
```

If you add or update a dependency, regenerate [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md) as well; a test fails
until you do:

```bash
cargo install cargo-about --locked --features cli   # once
python3 scripts/licenses/generate.py
```

The generator stops if a library's license is not one the project already accepts, or if it cannot find a library's
notice. In that case, mention it in your pull request so that the license can be reviewed.

`vendor/winit` is a modified copy of winit without KDE's blur protocol. `vendor/README.md` explains what was changed,
why, and what to do when eframe moves to a newer winit. The generator stops if the removed code reappears.

## How the code is organized

- **Four crates, each with a single responsibility.** `quickai-core` is pure logic (no processes, network or files),
  `quickai-runtime` performs the work on the computer, `quickai` is the desktop application, and `quickai-headless` is the
  headless program. Tests enforce these boundaries, so a change that crosses one will fail them.
- **Much of QuickAI is data.** Engines, the recommended models and the vocabularies are TOML documents under
  `crates/quickai-runtime/manifests/`, documented key by key in [docs/reference/](docs/reference/). Adding a model
  to the recommended list, for example, is a change to a document rather than to Rust code.
- **The design is documented.** Please read [docs/architecture.md](docs/architecture.md) before changing the kernel.

## Privacy is a feature

QuickAI is designed to keep its users' data on their own computers. Any change that makes it contact the network or
store new data must happen only at the user's request, must state what it contacts, and must update
[PRIVACY.md](PRIVACY.md) in the same pull request.

## Commit messages

Make one logical change per commit, with a short prefix: `feat:`, `fix:`, `docs:`, `refactor:`, `test:` or
`chore:`. Describe what changed and why, in plain language.

## Code of conduct

Everyone who takes part is expected to follow the [Code of Conduct](CODE_OF_CONDUCT.md).
