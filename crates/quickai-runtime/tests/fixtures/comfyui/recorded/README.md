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

## ComfyUI-Manager

`egress_manager.txt` is what ComfyUI v0.39.0 with ComfyUI-Manager 4.2.2 (the classic interface, `--enable-manager-legacy-ui`, with
`--offline`) contacted, measured on 2026-10-24 on a throw-away ComfyUI (the build QuickAI had installed, read only, the
processor only, every folder it writes in a scratch directory). This one is **not** the proxy of the recording above: it is the host
names that the process looked up (`socket.getaddrinfo` wrapped by a `sitecustomize.py` that wrote each name down), which is every
name a program of the process asked the resolver for, whatever library it used. All three are https, hence `:443`.

1. Started, and nothing opened for 45 seconds: `raw.githubusercontent.com` (5 lookups: the five lists the Manager keeps, `alter-list`,
   `model-list`, `extension-node-map`, `github-stats` and `custom-node-list`) and `api.comfy.org` (56: the Registry's pages of node
   packs). So the Manager contacts them **whenever ComfyUI starts**, with no one asking.
2. The Model Manager asked for `taef1_decoder.pth` (4.7 MB, its list's smallest model): `github.com` (and the lists again from
   `raw.githubusercontent.com`). A model of its list that is on Hugging Face is `huggingface.co` and `*.cdn.hf.co`, which the document
   declares for the page's Download button already.

`p3_x08` holds the document to both files. What a node or a model the person asks the Manager for is on may be any site; only what it
contacted by itself and for the one model measured is declared.
