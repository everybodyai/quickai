#!/usr/bin/env python3
"""Row P19 of docs/phase3-test-plan.md, by hand: the page of a real ComfyUI, in a real browser, with the bridge.

    real_page.py BUILD_DIR [WORK_DIR]

BUILD_DIR is a build of ComfyUI that QuickAI installed (`~/.quickai/comfyui/v0.39.0`: `comfyui`, `app/`, `venv/`).
It is started on the processor (`--cpu --offline`) on a free port, with the folder of the bridge beside its models
folder and `--extra-model-paths-config` as the document gives them (what the application itself writes is row P18),
in a work folder that this makes in WORK_DIR (the system's temporary folder if none is given; it is about 700 MB, and a
machine whose /tmp is memory wants another place) and removes, and only it. The browser is Chromium driven by Playwright (`pip install playwright`, `playwright
install chromium`). It needs the internet: it fetches `ae.safetensors` of `Comfy-Org/z_image_turbo` (335 MB) from
huggingface.co, about 6 MB/s here, and starts a second file only to see the browser take it.

What it holds to, in order (each is printed as it passes):

 1. ComfyUI started with the bridge contacts nothing by itself (the proxy's log is empty).
 2. The Z-Image-Turbo template of the build lists 3 missing models; **Download** on `ae.safetensors` fetches it into
    `vae/` through the bridge, and the browser saves nothing itself.
 3. ComfyUI is killed (SIGKILL) in the middle of it, started again, and the same press goes on from where the part
    ended and ends with the file the hub describes (its size and sha256), after which the page lists 2 missing.
 4. A file the bridge will not fetch (not the hub's address, a .ckpt) is left to the browser, with a sentence; a
    file that is there is said to be there.
 5. The hosts contacted are ones the document declares (`egress`), and the hub is one of them: the run that made
    `fixtures/comfyui/recorded/egress.txt` was this.
 6. With QUICKAI_PAGE_DOWNLOADS=browser the page has no host and no panel, the routes are not there, and the
    button hands the file to the browser.
"""

import hashlib
import os
import re
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

from playwright.sync_api import sync_playwright

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
ASSETS = os.path.join(HERE, "..", "..", "assets", "comfyui")
PROXY = os.path.join(HERE, "..", "fixtures", "ollama", "connect_proxy.py")
DOCUMENT = os.path.join(HERE, "..", "..", "manifests", "engines", "comfyui.toml")
URL = "https://huggingface.co/Comfy-Org/z_image_turbo/resolve/main/split_files/vae/ae.safetensors"
SIZE = 335304388
SHA = "afc8e28272cd15db3919bacdb6918ce9c1ed22e96cb12c4d5ed0fba823529e38"
READY = "window.comfyAPI && window.comfyAPI.app && window.comfyAPI.app.app && window.comfyAPI.app.app.rootGraph"


def say(text):
    print(f"  ok  {text}", flush=True)


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class Comfy:
    """A ComfyUI of the build, started the way the document starts it, with the bridge written beside its models."""

    def __init__(self, build, work, port, proxy_port=None):
        self.build, self.work, self.port, self.proxy_port = build, work, port, proxy_port
        self.data = os.path.join(work, "data")
        self.log = os.path.join(work, "comfy.log")
        self.process = None

    def put_bridge(self):
        os.makedirs(os.path.join(self.data, "extensions"), exist_ok=True)
        target = os.path.join(self.data, "extensions", "quickai_bridge")
        shutil.rmtree(target, ignore_errors=True)
        shutil.copytree(os.path.join(ASSETS, "quickai_bridge"), target, ignore=shutil.ignore_patterns("__pycache__"))
        shutil.copy(os.path.join(ASSETS, "extra_model_paths.yaml"), os.path.join(self.data, "extra_model_paths.yaml"))
        for name in ("models", "output", "input", "user", "temp"):
            os.makedirs(os.path.join(self.data, name), exist_ok=True)

    def start(self, page_downloads="install"):
        self.put_bridge()
        env = dict(os.environ, PYTHONUNBUFFERED="1", QUICKAI_PAGE_DOWNLOADS=page_downloads, HOME=os.path.join(self.work, "home"))
        os.makedirs(env["HOME"], exist_ok=True)
        if self.proxy_port:
            env["HTTPS_PROXY"] = f"http://127.0.0.1:{self.proxy_port}"
        d = self.data
        args = [
            os.path.join(self.build, "comfyui"), "--listen", "127.0.0.1", "--port", str(self.port), "--disable-auto-launch",
            "--cpu", "--offline", "--models-directory", f"{d}/models", "--output-directory", f"{d}/output",
            "--input-directory", f"{d}/input", "--user-directory", f"{d}/user", "--temp-directory", f"{d}/temp",
            "--database-url", f"sqlite:///{d}/user/comfyui.db", "--extra-model-paths-config", f"{d}/extra_model_paths.yaml",
        ]
        with open(self.log, "ab") as log:
            self.process = subprocess.Popen(args, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        end = time.time() + 180
        while time.time() < end:
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{self.port}/system_stats", timeout=2):
                    break
            except OSError:
                if self.process.poll() is not None:
                    raise AssertionError(f"ComfyUI ended at start: {open(self.log, errors='replace').read()[-2000:]}")
                time.sleep(1)
        else:
            raise AssertionError("ComfyUI never answered")
        self.api("POST", "/api/settings/Comfy.RightSidePanel.IsOpen", b"true")

    def api(self, method, path, body=None):
        request = urllib.request.Request(f"http://127.0.0.1:{self.port}{path}", data=body, method=method,
                                         headers={"Content-Type": "application/json"} if body else {})
        try:
            with urllib.request.urlopen(request, timeout=10) as reply:
                return reply.status, reply.read()
        except urllib.error.HTTPError as error:
            return error.code, error.read()

    def stop(self, hard=False):
        if self.process and self.process.poll() is None:
            os.killpg(self.process.pid, signal.SIGKILL if hard else signal.SIGTERM)
            self.process.wait(timeout=30)

    def said(self):
        return open(self.log, errors="replace").read()


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as file:
        for block in iter(lambda: file.read(1 << 22), b""):
            h.update(block)
    return h.hexdigest()


class Seen:
    """What the browser did on its own: the files it saved (an address of the page's own site), and the addresses of
    the hub it was sent to (another site opens in a tab of its own, which the browser saves from and closes)."""

    def __init__(self):
        self.downloads = []
        self.hub_pages = []


def open_template(browser, port):
    context = browser.new_context(viewport={"width": 1500, "height": 950}, accept_downloads=True)
    page = context.new_page()
    seen = Seen()
    page.on("download", lambda d: seen.downloads.append(d))
    context.on(
        "request",
        lambda r: seen.hub_pages.append(r.url) if r.is_navigation_request() and "huggingface.co" in r.url else None,
    )
    page.goto(f"http://127.0.0.1:{port}")
    page.wait_for_function(READY, timeout=90000)
    time.sleep(2)
    page.keyboard.press("Escape")
    template = page.evaluate("fetch('/templates/image_z_image_turbo.json').then(r => r.json())")
    page.evaluate("(g) => window.comfyAPI.app.app.loadGraphData(g)", template)
    time.sleep(3)
    page.keyboard.press("Escape")
    page.get_by_role("tab", name="Issues").click()
    time.sleep(1.5)
    return page, seen


def missing(page):
    return page.evaluate("() => [...document.querySelectorAll('button')].filter(b => b.innerText.trim() === 'Download').length")


def latest(page, name):
    views = page.evaluate("fetch('/api/quickai/downloads').then(r => r.json())")
    mine = [v for v in views if v["name"] == name]
    return mine[-1] if mine else None


def wait_for(page, name, states, seconds=300, more=None):
    end = time.time() + seconds
    while time.time() < end:
        view = latest(page, name)
        if view and view["state"] in states and (more is None or more(view)):
            return view
        time.sleep(0.5)
    raise AssertionError(f"{name} never reached {states}: {latest(page, name)}")


def main(build, work):
    ae = os.path.join(work, "data", "models", "vae", "ae.safetensors")
    log = os.path.join(work, "egress.log")
    proxy_port = free_port()
    proxy = subprocess.Popen([sys.executable, "-I", PROXY, str(proxy_port), log], start_new_session=True)
    time.sleep(1)
    comfy = Comfy(build, work, free_port(), proxy_port=proxy_port)
    try:
        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True)
            comfy.start()
            time.sleep(8)
            assert not os.path.exists(log) or open(log).read() == "", "ComfyUI contacted something by itself"
            say("ComfyUI with the bridge, started and idle, contacted nothing")

            page, seen = open_template(browser, comfy.port)
            assert missing(page) == 3, missing(page)
            assert page.evaluate("typeof window.__comfyDesktop2?.downloadModel") == "function"
            page.get_by_role("button", name="Download ae.safetensors").click()
            wait_for(page, "ae.safetensors", ("running",), more=lambda v: v["received"] > 100_000_000, seconds=120)
            assert seen.downloads == [] and seen.hub_pages == [], "the browser took the file by itself"
            say("Download of ae.safetensors is carried out by ComfyUI, and the panel is there")

            comfy.stop(hard=True)
            kept = os.path.getsize(ae + ".part")
            assert 0 < kept < SIZE and not os.path.exists(ae)
            say(f"ComfyUI killed at {kept / 1e6:.1f} MB of {SIZE / 1e6:.1f}: the part is kept, the file is not there")
            comfy.start()
            page, seen = open_template(browser, comfy.port)
            assert missing(page) == 3
            page.get_by_role("button", name="Download ae.safetensors").click()
            wait_for(page, "ae.safetensors", ("done",), seconds=400)
            time.sleep(6)
            assert os.path.getsize(ae) == SIZE and sha256(ae) == SHA, "the file is not the one the hub describes"
            assert "going on from" in comfy.said(), "it did not go on from the part"
            assert missing(page) == 2, f"the page did not read its list again: {missing(page)}"
            assert not os.path.exists(ae + ".part") and seen.downloads == [] and seen.hub_pages == []
            say("it went on from the part, ended with the hub's size and sha256, and the page's list went from 3 to 2")

            # a file the bridge will not fetch is left to the browser; one that is there is said to be
            def ask(url, name, directory):
                page.evaluate("([u, n, d]) => window.__comfyDesktop2.downloadModel(u, n, d)", [url, name, directory])
                time.sleep(2.5)
                return page.locator("#quickai-downloads").inner_text()

            local = f"http://127.0.0.1:{comfy.port}/templates/image_z_image_turbo.json"
            assert "browser will save it" in ask(local, "a.safetensors", "vae")
            assert [d.suggested_filename for d in seen.downloads] == ["a.safetensors"], seen.downloads
            ask(local, "old.ckpt", "checkpoints")
            assert [d.suggested_filename for d in seen.downloads] == ["a.safetensors", "old.ckpt"], seen.downloads
            assert "already in the vae folder" in ask(URL, "ae.safetensors", "vae")
            say("an address that is not the hub's and a .ckpt went to the browser, and a file that is there was said to be")

            comfy.stop()
            hosts = sorted({line.strip() for line in open(log) if line.strip()}) if os.path.exists(log) else []
            declared = re.search(r'^egress = \[(.*?)\]', open(DOCUMENT).read(), re.M).group(1)
            declared = re.findall(r'"([^"]+)"', declared)
            for host in hosts:
                name = host.rsplit(":", 1)[0]
                assert any(name == d or (d.startswith("*.") and name.endswith(d[1:])) for d in declared), (host, declared)
            assert "huggingface.co:443" in hosts, hosts
            say(f"the hosts contacted are {', '.join(hosts)}, which the document declares ({', '.join(declared)})")

            # the setting that leaves the button to the browser
            comfy.start(page_downloads="browser")
            page, seen = open_template(browser, comfy.port)
            assert page.evaluate("typeof window.__comfyDesktop2") == "undefined", "the page still has a host"
            assert page.locator("#quickai-downloads").count() == 0
            assert comfy.api("GET", "/api/quickai/downloads")[0] == 404, "the routes are still there"
            page.get_by_role("button", name="Download qwen_3_4b.safetensors").click()
            end = time.time() + 30
            while not seen.hub_pages and time.time() < end:
                time.sleep(0.3)
            assert seen.hub_pages and seen.hub_pages[0].endswith("/split_files/text_encoders/qwen_3_4b.safetensors"), seen.hub_pages
            assert page.locator("#quickai-downloads").count() == 0
            say("with the setting `browser` there is no host and no route, and the button hands the file to the browser")
            browser.close()
    finally:
        comfy.stop(hard=True)
        proxy.kill()
        proxy.wait()
    print("the page installs, goes on after a kill, and leaves the rest to the browser", flush=True)


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    parent = os.path.abspath(sys.argv[2]) if len(sys.argv) > 2 else tempfile.gettempdir()
    os.makedirs(parent, exist_ok=True)
    work = tempfile.mkdtemp(prefix="quickai-p19-", dir=parent)       # a folder of its own: only it is removed
    try:
        main(os.path.abspath(sys.argv[1]), work)
    finally:
        shutil.rmtree(work, ignore_errors=True)
