# QuickAI documentation

## Guides

- [headless.md](headless.md): `quickai-run`, QuickAI on a server with no display
- [configurations.md](configurations.md): save a whole setup as a file, share it, and import one safely
- [catalog.md](catalog.md): the list of recommended models, and how to add or hide one with a document of your own
- [lookup-and-bundles.md](lookup-and-bundles.md): how **Look up online** sorts results, and bundles such as a vision model with its projector
- [comfyui.md](comfyui.md): images and video with ComfyUI
- [openwebui.md](openwebui.md): Open WebUI, a chat page for Ollama
- [development.md](development.md): building, running and testing QuickAI from source

## Reference

Generated from the code, so always current:

- [engine-fields.md](reference/engine-fields.md): every key of an engine document
- [vocab-fields.md](reference/vocab-fields.md): every key of a vocabulary document
- [catalog-fields.md](reference/catalog-fields.md): every key of a catalog document
- [profile-fields.md](reference/profile-fields.md): every key of a configuration

## Examples

- [examples/profiles/](examples/profiles/): ready-made configurations (vision chat, voice chat, Open WebUI, video models)
- [examples/workflows/](examples/workflows/): ComfyUI workflows for Wan 2.2 and LTX-Video
- [examples/engines/](examples/engines/): engine documents to start your own from

## Design records

How QuickAI was designed, built and verified, kept as written. They include notes on the machines things were
measured on.

- [architecture.md](architecture.md): the declarative runtime (start here)
- [implementation-plan.md](implementation-plan.md): the phases and work packages, with their logs
- [phase3-test-plan.md](phase3-test-plan.md): the acceptance tests, written before the code
- [engine-facts.md](engine-facts.md): what was verified about each engine, and how
- [reports-and-community.md](reports-and-community.md): telemetry and crash reports the way VS Code does them, the admin, and the community configurations (the plan; nothing built yet)
- [ollama-backend-architecture.md](ollama-backend-architecture.md): the first Ollama design, superseded by architecture.md
