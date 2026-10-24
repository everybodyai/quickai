#!/usr/bin/env python3
"""Row P21 of docs/phase3-test-plan.md, by hand: the page of a real ComfyUI, in a real browser, with ComfyUI-Manager.

    real_manager.py BUILD_DIR [WORK_DIR]

BUILD_DIR is a build of ComfyUI that QuickAI installed (`~/.quickai/comfyui/v0.39.0`: `comfyui`, `app/`, `venv/`). It is
**not changed**: the packages the Manager needs and its environment lacks (what `pip` would install for ComfyUI's own
`manager_requirements.txt`, wheels only, 13 packages, 78 MB) are put in a folder of the work, and ComfyUI is started with
that folder on its path. That is what a build made with the Manager is, as far as ComfyUI can tell.

ComfyUI is started on the processor (`--cpu --offline`, the page's own default of QuickAI) on a free port, twice: with the
Manager and the classic interface (`--enable-manager-legacy-ui`, what the launcher of such a build passes), and without,
each with the folder of the Manager's button beside its models folder and `--extra-model-paths-config` as the document gives
them (what the application itself writes is row P18). Everything it writes is in WORK_DIR (the system's temporary folder if
none is given; about 150 MB, and a machine whose /tmp is memory wants another place), and is removed, and only it. The
browser is Chromium driven by Playwright (`pip install playwright`, `playwright install chromium`). It needs the internet:
the Manager fetches its lists from GitHub and the Comfy Registry, and a 4.7 MB model from GitHub.

What it holds to, in order (each is printed as it passes):

 1. With the Manager, the page's default menu has a button of ours at the top (the Manager's own is in the old menu, which the
    page hides), and it opens the classic Manager, which has the **Custom Nodes Manager** and the **Model Manager**.
 2. The Model Manager lists its models, and **Install** on the TAEF1 decoder (`taef1_decoder.pth`, 4.7 MB) puts the whole
    file in the folder of its kind (`vae_approx`) below the models folder ComfyUI was given.
 3. The hosts contacted are the ones the document declares (`egress`): at the start, before anyone opened anything, only
    `raw.githubusercontent.com` and `api.comfy.org`; with the model, `github.com` as well. (The names looked up are written down by a
    `sitecustomize.py` of the work folder: every name a program of the process asked the resolver for.)
 4. The Manager's settings, lists and history are below the user folder ComfyUI was given, and nothing of it is beside the build.
 5. Without the Manager the page has no button of ours and nothing of ours in its console, and the routes of the Manager are not there.
"""

import json
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
DOCUMENT = os.path.join(HERE, "..", "..", "manifests", "engines", "comfyui.toml")
MODEL = "taef1_decoder.pth"
MODEL_FOLDER = "vae_approx"
MODEL_SIZE = 4943336
BUTTON = "ComfyUI-Manager: custom nodes and models"

RECORDER = '''import os, socket
_log = os.environ.get("QAI_HOSTS_LOG")
if _log:
    _real = socket.getaddrinfo
    def getaddrinfo(host, *args, **kwargs):
        if isinstance(host, str) and host not in ("127.0.0.1", "localhost", "::1", "0.0.0.0"):
            with open(_log, "a") as out:
                out.write(host + "\\n")
        return _real(host, *args, **kwargs)
    socket.getaddrinfo = getaddrinfo
'''


def say(text):
    print(f"  ok  {text}", flush=True)


def fail(text):
    print(f"  FAILED  {text}", flush=True)
    sys.exit(1)


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def declared_hosts():
    """The hosts the document says ComfyUI contacts (`egress` of the document)."""
    text = open(DOCUMENT, encoding="utf-8").read()
    line = next(l for l in text.splitlines() if l.startswith("egress = ["))
    return re.findall(r'"([^"]+)"', line.split("#")[0])


def covered(declared, host):
    return any(d == host or (d.startswith("*.") and host.endswith(d[1:])) for d in declared)


def the_managers_packages(build, work, env):
    """Puts what `pip install -r manager_requirements.txt` would add to the build's environment in WORK/site, and
    changes nothing of the build: the plan comes from a dry run, the packages from the index (wheels only)."""
    python = os.path.join(build, "venv", "bin", "python")
    requirements = os.path.join(build, "app", "manager_requirements.txt")
    report = os.path.join(work, "report.json")
    run = [python, "-I", "-m", "pip", "--disable-pip-version-check"]
    subprocess.run(
        run + ["install", "--dry-run", "--quiet", "--only-binary=:all:", "--report", report, "-r", requirements],
        env=env, check=True, capture_output=True,
    )
    wanted = [
        f'{item["metadata"]["name"]}=={item["metadata"]["version"]}'
        for item in json.load(open(report))["install"]
    ]
    site = os.path.join(work, "site")
    subprocess.run(
        run + ["install", "--quiet", "--only-binary=:all:", "--no-deps", "--target", site] + wanted,
        env=env, check=True, capture_output=True,
    )
    with open(os.path.join(site, "sitecustomize.py"), "w") as out:
        out.write(RECORDER)
    return wanted, site


def start(build, work, name, port, flags, site, env):
    """ComfyUI of the build, on the processor, with everything it writes below WORK/name."""
    home = os.path.join(work, name)
    for folder in ("models", "output", "input", "user", "temp", "home", "tmp", "extensions"):
        os.makedirs(os.path.join(home, folder), exist_ok=True)
    shutil.copytree(os.path.join(ASSETS, "quickai_manager"), os.path.join(home, "extensions", "quickai_manager"),
                    ignore=shutil.ignore_patterns("__pycache__"))
    with open(os.path.join(home, "extra_model_paths.yaml"), "w") as out:
        out.write("quickai:\n  custom_nodes: extensions\n")
    hosts = os.path.join(home, "hosts.txt")
    child_env = dict(
        env, HOME=os.path.join(home, "home"), TMPDIR=os.path.join(home, "tmp"), PYTHONNOUSERSITE="1",
        PYTHONDONTWRITEBYTECODE="1", PYTHONUNBUFFERED="1", QAI_HOSTS_LOG=hosts,
        PYTHONPATH=site if site else "",
    )
    log = open(os.path.join(home, "comfyui.log"), "w")
    process = subprocess.Popen(
        [os.path.join(build, "venv", "bin", "python"), "-B", "main.py", "--cpu", "--listen", "127.0.0.1",
         "--port", str(port), "--disable-auto-launch", "--offline",
         "--models-directory", os.path.join(home, "models"), "--output-directory", os.path.join(home, "output"),
         "--input-directory", os.path.join(home, "input"), "--user-directory", os.path.join(home, "user"),
         "--temp-directory", os.path.join(home, "temp"),
         "--database-url", "sqlite:///" + os.path.join(home, "user", "comfyui.db"),
         "--extra-model-paths-config", os.path.join(home, "extra_model_paths.yaml")] + flags,
        cwd=os.path.join(build, "app"), env=child_env, stdout=log, stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    deadline = time.time() + 240
    while time.time() < deadline:
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{port}/system_stats", timeout=3).read()
            return process, home, hosts
        except (urllib.error.URLError, OSError):
            if process.poll() is not None:
                fail(f"ComfyUI ended while it started: see {log.name}")
            time.sleep(2)
    fail("ComfyUI did not answer in four minutes")


def stop(process):
    try:
        os.killpg(process.pid, signal.SIGTERM)
        process.wait(timeout=20)
    except Exception:
        os.killpg(process.pid, signal.SIGKILL)


def names(path):
    try:
        return sorted(set(open(path).read().split()))
    except FileNotFoundError:
        return []


def open_page(playwright, port, problems):
    browser = playwright.chromium.launch(headless=True)
    page = browser.new_page(viewport={"width": 1500, "height": 900})
    page.on("console", lambda m: problems.append(m.text[:200]) if m.type == "error" and "quickai" in m.text.lower() else None)
    page.on("pageerror", lambda e: problems.append("pageerror: " + str(e)[:200]))
    page.goto(f"http://127.0.0.1:{port}/", wait_until="domcontentloaded")
    page.wait_for_function("() => window.app && window.app.graph", timeout=120000)
    time.sleep(4)
    try:  # the dialog of templates that opens the first time
        page.get_by_role("button", name="Close").first.click(timeout=3000)
    except Exception:
        page.keyboard.press("Escape")
    time.sleep(2)
    return browser, page


def main():
    build = os.path.abspath(sys.argv[1])
    base = sys.argv[2] if len(sys.argv) > 2 else tempfile.gettempdir()
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="qai-p21-", dir=base)
    env = {"PATH": "/usr/bin:/bin", "PYTHONNOUSERSITE": "1"}
    declared = declared_hosts()
    processes = []
    try:
        wanted, site = the_managers_packages(build, work, env)
        say(f"the Manager's {len(wanted)} packages are in the work folder; the build is not changed")
        with sync_playwright() as playwright:
            # ---- with the Manager ------------------------------------------------------------------------------
            port = free_port()
            process, home, hosts = start(build, work, "with", port, ["--enable-manager-legacy-ui"], site, env)
            processes.append(process)
            time.sleep(45)  # nothing is opened: what is contacted now is contacted by itself
            at_start = names(hosts)
            if not at_start or any(not covered(declared, h) for h in at_start):
                fail(f"ComfyUI with the Manager contacted {at_start} at the start; the document declares {declared}")
            if sorted(at_start) != ["api.comfy.org", "raw.githubusercontent.com"]:
                fail(f"at the start it contacted {at_start}, and the document's account of it is the two of the Manager")
            say(f"started, and left alone for 45 s, it contacted {', '.join(at_start)} and nothing else")

            problems = []
            browser, page = open_page(playwright, port, problems)
            button = page.get_by_role("button", name=BUTTON)
            if button.count() == 0 or not button.first.is_visible():
                fail("the page has no Manager button at the top with its default menu")
            button.first.click()
            time.sleep(3)
            body = page.evaluate("() => document.body.innerText")
            if "Model Manager" not in body or "Custom Nodes Manager" not in body:
                fail("the button did not open the classic Manager with its Custom Nodes Manager and Model Manager")
            say("the button is at the top of the page with the page's default menu and opens the classic Manager")

            page.get_by_role("button", name="Model Manager", exact=True).click(timeout=8000)
            time.sleep(5)
            page.locator("input[type=search]").first.fill("taef1_decoder")
            time.sleep(3)
            page.get_by_role("button", name="Install", exact=True).first.click()
            target = os.path.join(home, "models", MODEL_FOLDER, MODEL)
            deadline = time.time() + 120
            while time.time() < deadline and not (os.path.isfile(target) and os.path.getsize(target) == MODEL_SIZE):
                time.sleep(2)
            if not os.path.isfile(target) or os.path.getsize(target) != MODEL_SIZE:
                fail(f"{MODEL} is not whole in {MODEL_FOLDER}: {os.path.exists(target) and os.path.getsize(target)}")
            say(f"the Model Manager put {MODEL} ({MODEL_SIZE} bytes) in {MODEL_FOLDER}/ below the models folder it was given")
            browser.close()

            time.sleep(5)
            everything = names(hosts)
            undeclared = [h for h in everything if not covered(declared, h)]
            if undeclared or "github.com" not in everything:
                fail(f"the hosts contacted were {everything}; undeclared: {undeclared}; github.com expected")
            say(f"the hosts contacted, {', '.join(everything)}, are the ones the document declares")

            if not os.path.isfile(os.path.join(home, "user", "__manager", "config.ini")):
                fail("the Manager's settings are not below the user folder")
            if os.path.exists(os.path.join(build, "app", "user", "__manager")):
                fail("the Manager wrote beside the build")
            say("the Manager's settings and lists are below the user folder it was given, and nothing is beside the build")
            if problems:
                fail(f"the page said something of ours: {problems}")
            stop(process)

            # ---- without it ------------------------------------------------------------------------------------
            port = free_port()
            process, home, hosts = start(build, work, "without", port, [], None, env)
            processes.append(process)
            problems = []
            browser, page = open_page(playwright, port, problems)
            if page.get_by_role("button", name=BUTTON).count() != 0:
                fail("the page has a Manager button when there is no Manager")
            try:
                urllib.request.urlopen(f"http://127.0.0.1:{port}/v2/manager/version", timeout=5)
                fail("the Manager's routes answer when there is no Manager")
            except urllib.error.HTTPError as error:
                if error.code != 404:
                    fail(f"the Manager's route answered {error.code}, not 404")
            if problems:
                fail(f"the page said something of ours: {problems}")
            browser.close()
            say("without the Manager the page has no button of ours and no error of ours, and the routes are not there")
        print("P21: all of it held", flush=True)
    finally:
        for process in processes:
            if process.poll() is None:
                stop(process)
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
