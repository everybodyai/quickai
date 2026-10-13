"""QuickAI's bridge for ComfyUI's page: its "Download" button puts a model where ComfyUI looks for it.

QuickAI writes this folder beside ComfyUI's models folder every time it starts ComfyUI, and tells ComfyUI to read
it as a folder of custom nodes. It has no nodes. It has the three routes below, which only this machine may use,
and a script for the page (web/quickai_bridge.js) that gives the page's "Download" button what ComfyUI Desktop
gives it: a host program that fetches the file. How a file is fetched, and what is refused, is in fetch.py.

    POST   /quickai/downloads        {"url", "name", "directory"}   ask for a file; answers what it became
    GET    /quickai/downloads                                       every download, and how far it is
    DELETE /quickai/downloads/<id>                                  stop one

QuickAI's setting "The page's Download button" reaches it as QUICKAI_PAGE_DOWNLOADS: `browser` leaves the button to
the browser, as it is without QuickAI, and then this has no route and no script, and ComfyUI contacts no site.
"""

import importlib.util
import ipaddress
import logging
import os

from aiohttp import web

import folder_paths
from server import PromptServer

NODE_CLASS_MAPPINGS = {}
NODE_DISPLAY_NAME_MAPPINGS = {}

ENABLED = os.environ.get("QUICKAI_PAGE_DOWNLOADS", "install") != "browser"

_here = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location("quickai_bridge_fetch", os.path.join(_here, "fetch.py"))
fetch = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(fetch)


def _where(kind):
    try:
        return folder_paths.get_folder_paths(kind)
    except KeyError:
        return None


def _from_this_machine(request):
    """Only a program on this machine may ask: a ComfyUI that is listening for others is not a way to fill the disk."""
    try:
        address = ipaddress.ip_address(request.remote)
    except ValueError:
        return False
    if address.version == 6 and address.ipv4_mapped is not None:
        address = address.ipv4_mapped
    return address.is_loopback


def _json(body, status=200):
    return web.json_response(body, status=status)


def _routes(fetcher, routes):
    @routes.post("/quickai/downloads")
    async def ask(request):
        if not _from_this_machine(request):
            return _json({"error": "only this machine may ask for a download", "browser": False}, 403)
        try:
            body = await request.json()
            url, name, directory = body["url"], body["name"], body["directory"]
        except Exception:
            return _json({"error": "the request is not {url, name, directory}", "browser": False}, 400)
        try:
            job = fetcher.add(url, name, directory)
        except fetch.Refused as refused:
            return _json({"error": str(refused), "browser": refused.browser}, 422)
        return _json(job.view(), 202 if job.state in ("queued", "running", "checking") else 200)

    @routes.get("/quickai/downloads")
    async def list_downloads(request):
        if not _from_this_machine(request):
            return _json({"error": "only this machine may ask"}, 403)
        return _json(fetcher.views())

    @routes.delete("/quickai/downloads/{id}")
    async def stop(request):
        if not _from_this_machine(request):
            return _json({"error": "only this machine may ask"}, 403)
        if not fetcher.cancel(request.match_info["id"]):
            return _json({"error": "there is no such download"}, 404)
        return _json({"stopped": True})

    return ask, list_downloads, stop


if ENABLED:
    WEB_DIRECTORY = "./web"
    FETCHER = fetch.Fetcher(
        where=_where,
        root=folder_paths.models_dir,
        say=lambda text: logging.info("[QuickAI] %s", text),
    )
    HANDLERS = _routes(FETCHER, PromptServer.instance.routes)
else:
    logging.info("[QuickAI] the page's Download button is left to the browser (QUICKAI_PAGE_DOWNLOADS=browser)")
