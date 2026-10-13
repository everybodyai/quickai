"""The bridge's glue to ComfyUI (`__init__.py`), without ComfyUI: the three modules it takes from ComfyUI
(`aiohttp`, `folder_paths`, `server`) are stood in for, and the routes it registers are called with requests
that are made here. The downloads themselves are tested in test_fetch.py.

Run by `crates/quickai-runtime/tests/comfy_bridge.rs` like it: `test_p12_...` is row P12 of docs/phase3-test-plan.md.
"""

import asyncio
import importlib.util
import os
import shutil
import sys
import tempfile
import time
import types
import unittest

sys.dont_write_bytecode = True

import test_fetch  # noqa: E402  (the directory of this file is on the path: unittest puts it there)

HERE = os.path.dirname(os.path.abspath(__file__))
SOURCE = os.environ.get("QUICKAI_BRIDGE_UNDER_TEST") or os.path.join(
    HERE, "..", "..", "assets", "comfyui", "quickai_bridge", "__init__.py"
)


class Routes:
    """What ComfyUI's `PromptServer.instance.routes` is to a node: decorators that take note of the handlers."""

    def __init__(self):
        self.table = {}

    def _add(self, method):
        def with_path(path):
            def decorate(handler):
                self.table[(method, path)] = handler
                return handler

            return decorate

        return with_path

    def __getattr__(self, name):
        if name in ("get", "post", "delete"):
            return self._add(name.upper())
        raise AttributeError(name)


class Request:
    def __init__(self, remote, body=None, ident=None):
        self.remote = remote
        self._body = body
        self.match_info = {"id": ident}

    async def json(self):
        if self._body is None:
            raise ValueError("not JSON")
        return self._body


def load(models, setting):
    """The bridge's `__init__.py` run against stand-ins for ComfyUI, with QUICKAI_PAGE_DOWNLOADS as given."""
    web = types.SimpleNamespace(json_response=lambda body, status=200: (status, body))
    aiohttp = types.ModuleType("aiohttp")
    aiohttp.web = web
    folder_paths = types.ModuleType("folder_paths")
    folder_paths.models_dir = models

    def get_folder_paths(kind):
        return {"vae": [os.path.join(models, "vae")]}[kind]

    folder_paths.get_folder_paths = get_folder_paths
    server = types.ModuleType("server")
    routes = Routes()
    server.PromptServer = types.SimpleNamespace(instance=types.SimpleNamespace(routes=routes))
    names = ("aiohttp", "folder_paths", "server")
    before = {name: sys.modules.get(name) for name in names}
    sys.modules.update(aiohttp=aiohttp, folder_paths=folder_paths, server=server)
    old = os.environ.get("QUICKAI_PAGE_DOWNLOADS")
    if setting is None:
        os.environ.pop("QUICKAI_PAGE_DOWNLOADS", None)
    else:
        os.environ["QUICKAI_PAGE_DOWNLOADS"] = setting
    try:
        spec = importlib.util.spec_from_file_location("quickai_bridge_under_test", SOURCE)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
    finally:
        for name, was in before.items():
            if was is None:
                sys.modules.pop(name, None)
            else:
                sys.modules[name] = was
        if old is None:
            os.environ.pop("QUICKAI_PAGE_DOWNLOADS", None)
        else:
            os.environ["QUICKAI_PAGE_DOWNLOADS"] = old
    return module, routes


def call(routes, method, path, request):
    return asyncio.run(routes.table[(method, path)](request))


class Glue(unittest.TestCase):
    def setUp(self):
        os.environ["NO_PROXY"] = "127.0.0.1"
        self.models = tempfile.mkdtemp(prefix="quickai-glue-")

    def tearDown(self):
        shutil.rmtree(self.models, ignore_errors=True)

    def test_p12_the_routes_answer_this_machine_only_and_the_setting_leaves_the_button_to_the_browser(self):
        # on: the three routes and the script of the page are there
        module, routes = load(self.models, None)
        self.assertEqual(
            sorted(routes.table),
            [("DELETE", "/quickai/downloads/{id}"), ("GET", "/quickai/downloads"), ("POST", "/quickai/downloads")],
        )
        self.assertEqual(module.WEB_DIRECTORY, "./web")
        self.assertEqual(module.NODE_CLASS_MAPPINGS, {}, "it has no node")
        self.assertTrue(os.path.isfile(os.path.join(os.path.dirname(SOURCE), "web", "quickai_bridge.js")))
        post, listing, delete = ("POST", "/quickai/downloads"), ("GET", "/quickai/downloads"), ("DELETE", "/quickai/downloads/{id}")

        # only a program on this machine may ask, in any of the ways a machine has its own address
        good = {"url": "https://huggingface.co/a/b/resolve/main/x.safetensors", "name": "x.safetensors", "directory": "vae"}
        for other in ("192.168.1.5", "10.0.0.2", "8.8.8.8", "fe80::1", "", None, "not an address", "::ffff:192.168.1.5"):
            self.assertEqual(call(routes, *post, Request(other, good))[0], 403, other)
            self.assertEqual(call(routes, *listing, Request(other))[0], 403, other)
            self.assertEqual(call(routes, *delete, Request(other, ident="x"))[0], 403, other)
        for mine in ("127.0.0.1", "127.0.0.2", "::1", "::ffff:127.0.0.1"):
            self.assertEqual(call(routes, *listing, Request(mine)), (200, []), mine)

        # what is not a request is 400, what the bridge will not fetch is 422 and says whether the browser may
        self.assertEqual(call(routes, *post, Request("127.0.0.1", None))[0], 400)
        self.assertEqual(call(routes, *post, Request("127.0.0.1", {"url": "x"}))[0], 400)
        status, body = call(routes, *post, Request("127.0.0.1", dict(good, name="x.ckpt")))
        self.assertEqual((status, body["browser"]), (422, True), body)
        status, body = call(routes, *post, Request("127.0.0.1", dict(good, directory="nothing")))
        self.assertEqual(status, 422, body)
        self.assertEqual(call(routes, *delete, Request("127.0.0.1", ident="nothing"))[0], 404)

        # an accepted request is carried out by the fetcher the glue made, with the folders ComfyUI says
        hub = test_fetch.Hub()
        try:
            module.FETCHER.policy = module.fetch.Policy(hosts=("127.0.0.1",), schemes=("http",), any_port=True)
            status, body = call(routes, *post, Request("::1", dict(good, url=hub.url("x.safetensors"))))
            self.assertIn(status, (200, 202), body)
            ident = body["id"]
            end = time.time() + 20
            while time.time() < end:
                (_, views) = call(routes, *listing, Request("127.0.0.1"))
                if [v for v in views if v["id"] == ident and v["state"] == "done"]:
                    break
                time.sleep(0.05)
            else:
                self.fail(f"never done: {views}")
            with open(os.path.join(self.models, "vae", "x.safetensors"), "rb") as file:
                self.assertEqual(file.read(), test_fetch.CONTENT)
            self.assertEqual(call(routes, *delete, Request("127.0.0.1", ident=ident)), (200, {"stopped": True}))
        finally:
            hub.close()

        # off: nothing is registered, and the page gets no script
        module, routes = load(self.models, "browser")
        self.assertEqual(routes.table, {})
        self.assertFalse(hasattr(module, "WEB_DIRECTORY"), "no script, so the page's button is the browser's")
        self.assertFalse(hasattr(module, "FETCHER"))
        # any other word is the default
        module, routes = load(self.models, "install")
        self.assertEqual(len(routes.table), 3)


if __name__ == "__main__":
    unittest.main()
