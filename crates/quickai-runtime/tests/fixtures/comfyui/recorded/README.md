# What a real ComfyUI contacted when the page's Download button fetched a model

`egress.txt` is the list of hosts (`host:port`) that ComfyUI v0.39.0, with QuickAI's bridge
(`crates/quickai-runtime/assets/comfyui/quickai_bridge`), connected to, measured on 2026-10-08 by pointing its
`HTTPS_PROXY` at `../../ollama/connect_proxy.py`, which forwards everything and writes down the first line of
each CONNECT:

1. ComfyUI was started on the processor (`--cpu --offline`, the build QuickAI had installed) with the bridge
   as a folder of custom nodes: it started, loaded every node and idled for ten seconds: **nothing was
   contacted** (the log of the proxy was empty).
2. `POST /api/quickai/downloads` with the address of `ae.safetensors` of `Comfy-Org/z_image_turbo` (335 MB),
   the way the page's button asks. After 14 MB it was stopped (`DELETE`): the hosts were contacted by then.

The hub answers the first request with a redirect to the server that has the file; the name of that server
depends on where the person is (`us.aws.cdn.hf.co` here), so the engine document declares `*.cdn.hf.co`.
`p3_x08` holds the document to this file in both directions: nothing seen is undeclared, and nothing is
declared that was not seen. The run by hand `p3_p18` records it again.
