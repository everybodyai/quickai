"""The bridge's downloads, against a server that behaves like the hub: it sends a file on to a download
server with the checksum in the answer that does, and it can stop short, ignore a range, stall, or refuse.

Run by `crates/quickai-runtime/tests/comfy_bridge.rs` (`python -I -m unittest`), which names each test by the row of
docs/phase3-test-plan.md it is (`test_p01_...` is row P01).
"""

import hashlib
import http.server
import importlib.util
import os
import shutil
import sys
import tempfile
import threading
import time
import unittest

sys.dont_write_bytecode = True      # the file under test is part of the application: nothing is written beside it

HERE = os.path.dirname(os.path.abspath(__file__))
SOURCE = os.environ.get("QUICKAI_FETCH_UNDER_TEST") or os.path.join(
    HERE, "..", "..", "assets", "comfyui", "quickai_bridge", "fetch.py"
)
spec = importlib.util.spec_from_file_location("quickai_bridge_fetch", SOURCE)
fetch = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fetch)

CONTENT = os.urandom(3_000_000)
SHA = hashlib.sha256(CONTENT).hexdigest()


class Hub(http.server.ThreadingHTTPServer):
    """What the hub does, set by each test: `body` is what it has, `claims` what it says its checksum is."""

    daemon_threads = True

    def __init__(self):
        super().__init__(("127.0.0.1", 0), Handler)
        self.body = CONTENT
        self.claims = SHA
        self.ignore_range = False
        self.stop_after = None          # a body that ends after this many bytes
        self.pause = 0.0                # seconds between pieces
        self.status = None              # an answer of this status instead of the file
        self.asked = []                 # (path, Range header) of every request
        self.thread = threading.Thread(target=self.serve_forever, daemon=True)
        self.thread.start()

    def url(self, name="model.safetensors"):
        return f"http://127.0.0.1:{self.server_address[1]}/owner/repo/resolve/main/{name}"

    def close(self):
        self.shutdown()
        self.server_close()


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def do_GET(self):
        hub = self.server
        hub.asked.append((self.path, self.headers.get("Range")))
        if hub.status:
            self.send_response(hub.status)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        if "/resolve/" in self.path:
            self.send_response(302)
            self.send_header("Location", "/cdn/" + self.path.rsplit("/", 1)[-1])
            if hub.claims:
                self.send_header("X-Linked-Etag", f'"{hub.claims}"')
            self.send_header("X-Linked-Size", str(len(hub.body)))
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        body, start, status = hub.body, 0, 200
        wanted = self.headers.get("Range")
        if wanted and not hub.ignore_range:
            start = int(wanted.split("=")[1].split("-")[0])
            if start >= len(body):
                self.send_response(416)
                self.send_header("Content-Range", f"bytes */{len(body)}")
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            status = 206
        self.send_response(status)
        if status == 206:
            self.send_header("Content-Range", f"bytes {start}-{len(body) - 1}/{len(body)}")
        piece = body[start:]
        self.send_header("Content-Length", str(len(piece)))
        self.end_headers()
        if hub.stop_after is not None:
            piece = piece[: hub.stop_after]
        try:
            for i in range(0, len(piece), 200_000):
                self.wfile.write(piece[i : i + 200_000])
                self.wfile.flush()
                if hub.pause:
                    time.sleep(hub.pause)
        except OSError:
            return
        if hub.stop_after is not None:
            self.close_connection = True


def read(path):
    with open(path, "rb") as file:
        return file.read()


def wait(fetcher, job, states, seconds=20):
    end = time.time() + seconds
    while time.time() < end:
        if job.state in states:
            return
        time.sleep(0.02)
    raise AssertionError(f"{job.name} is {job.state}, not one of {states}: {job.error}")


class Bridge(unittest.TestCase):
    def setUp(self):
        os.environ["NO_PROXY"] = "127.0.0.1"
        self.models = tempfile.mkdtemp(prefix="quickai-bridge-")
        self.hub = Hub()
        self.said = []
        self.fetcher = fetch.Fetcher(
            where=self.where,
            root=self.models,
            policy=fetch.Policy(hosts=("127.0.0.1",), schemes=("http",), any_port=True),
            say=self.said.append,
        )

    def tearDown(self):
        self.hub.close()
        shutil.rmtree(self.models, ignore_errors=True)

    def where(self, kind):
        paths = {
            "vae": ["vae"],
            "diffusion_models": ["unet", "diffusion_models"],      # ComfyUI looks in `unet` first
            "outside": [os.path.join(os.path.dirname(self.models), "elsewhere")],
        }.get(kind)
        return [os.path.join(self.models, p) if not os.path.isabs(p) else p for p in (paths or [])]

    def ask(self, name="ae.safetensors", directory="vae", url=None):
        return self.fetcher.add(url or self.hub.url(name), name, directory)

    def dest(self, name="ae.safetensors", directory="vae"):
        return os.path.join(self.models, directory, name)

    # P01 ---------------------------------------------------------------------------------------------
    def test_p01_a_file_is_fetched_into_the_folder_of_its_kind_whole_and_checked(self):
        job = self.ask()
        wait(self.fetcher, job, ("done",))
        self.assertEqual(read(self.dest()), CONTENT)
        self.assertFalse(os.path.exists(self.dest() + ".part"), "the part goes with the rename")
        self.assertEqual((job.received, job.total), (len(CONTENT), len(CONTENT)))
        self.assertEqual([p for p, _ in self.hub.asked], ["/owner/repo/resolve/main/ae.safetensors", "/cdn/ae.safetensors"])
        view = job.view()
        self.assertEqual(view["state"], "done")
        self.assertEqual(sorted(view), ["browser", "directory", "error", "id", "name", "received", "speed", "state", "total"])

    # P02 ---------------------------------------------------------------------------------------------
    def test_p02_what_is_not_the_hubs_or_not_a_format_of_numbers_or_not_plain_is_refused(self):
        refused = {
            "an address that is not the hub's": dict(url="http://127.0.0.2:1/x/resolve/main/a.safetensors"),
            "an address that is not text": dict(url=123),
            "a user in the address": dict(url=f"http://u:p@127.0.0.1:{self.hub.server_address[1]}/x/resolve/main/a.safetensors"),
            "a file that may hold a program": dict(name="a.ckpt"),
            "a pickle": dict(name="a.pt"),
            "no name": dict(name=""),
            "a name that climbs": dict(name="../a.safetensors"),
            "a name that begins at the root": dict(name="/etc/a.safetensors"),
            "an empty part": dict(name="x//a.safetensors"),
            "a hidden file": dict(name=".a.safetensors"),
            "a name with a backslash": dict(name="x\\a.safetensors"),
            "a name too deep": dict(name="a/b/c/d/e.safetensors"),
            "a kind of model ComfyUI has not": dict(directory="nothing"),
            "a folder outside the models folder": dict(directory="outside"),
            "a kind that is not text": dict(directory=None),
        }
        for what, change in refused.items():
            args = dict(name="a.safetensors", directory="vae")
            args.update(change)
            url = args.pop("url", None) or self.hub.url()
            with self.assertRaises(fetch.Refused, msg=what):
                self.fetcher.add(url, args["name"], args["directory"])
        # the formats of numbers are let through, in any case: safetensors, and GGUF (the quantized models
        # that the pack of GGUF nodes reads); a pickle is not, and says what is
        for name in ("a.safetensors", "A.SafeTensors", "a.sft", "model-Q8_0.gguf", "UMT5.GGUF", "x/y/z-Q4_K_M.gguf"):
            self.assertEqual(fetch.plain_segments(name), name.split("/"), name)
        for name in ("a.ckpt", "a.pt", "a.pth", "a.bin", "a.gguf.exe", "gguf", "a.safetensors.html"):
            with self.assertRaises(fetch.Refused, msg=name) as caught:
                fetch.plain_segments(name)
            self.assertTrue(caught.exception.browser, name)
            self.assertIn(".safetensors or .gguf", str(caught.exception), name)
        # the hub's own https address is the one that is let through by default
        fetch.Policy().check_start("https://huggingface.co/Comfy-Org/z_image_turbo/resolve/main/split_files/vae/ae.safetensors")
        for bad in ("http://huggingface.co/a/resolve/main/b.safetensors", "https://huggingface.co.evil.example/a",
                    "https://evil.example/huggingface.co", "ftp://huggingface.co/a", "https://huggingface.co:8443/a"):
            with self.assertRaises(fetch.Refused, msg=bad):
                fetch.Policy().check_start(bad)
        with self.assertRaises(fetch.Refused):
            fetch.Policy().check_hop("http://cdn.example/file")
        fetch.Policy().check_hop("https://us.aws.cdn.hf.co/file")
        self.assertEqual(os.listdir(self.models), [], "a refusal leaves nothing behind")

    # P03 ---------------------------------------------------------------------------------------------
    def test_p03_a_part_that_a_stop_left_is_gone_on_from_and_the_whole_is_checked(self):
        os.makedirs(os.path.dirname(self.dest()))
        with open(self.dest() + ".part", "wb") as part:
            part.write(CONTENT[:1_234_567])
        job = self.ask()
        wait(self.fetcher, job, ("done",))
        self.assertEqual(read(self.dest()), CONTENT)
        self.assertEqual(self.hub.asked[-1], ("/cdn/ae.safetensors", "bytes=1234567-"))
        # a part that is not the beginning of this file is found out by the checksum, and thrown away
        self.hub.asked.clear()
        os.remove(self.dest())
        with open(self.dest() + ".part", "wb") as part:
            part.write(b"\x00" * 1_000_000)
        job = self.ask()
        wait(self.fetcher, job, ("done",))
        self.assertEqual(read(self.dest()), CONTENT)
        ranges = [r for path, r in self.hub.asked if path.startswith("/cdn")]
        self.assertEqual(ranges, ["bytes=1000000-", None], "it asked for the rest, then for all of it")
        self.assertTrue(any("beginning again" in line for line in self.said), self.said)

    # P04 ---------------------------------------------------------------------------------------------
    def test_p04_a_hub_that_ignores_the_range_or_says_the_part_is_all_there_is_is_met(self):
        os.makedirs(os.path.dirname(self.dest()))
        with open(self.dest() + ".part", "wb") as part:
            part.write(CONTENT[:500_000])
        self.hub.ignore_range = True
        job = self.ask()
        wait(self.fetcher, job, ("done",))
        self.assertEqual(read(self.dest()), CONTENT, "the whole came, and the part was written over")
        # the part is the whole file already (the stop came after the last byte): nothing is fetched, all is checked
        os.remove(self.dest())
        with open(self.dest() + ".part", "wb") as part:
            part.write(CONTENT)
        self.hub.ignore_range = False
        self.hub.asked.clear()
        job = self.ask()
        wait(self.fetcher, job, ("done",))
        self.assertEqual(read(self.dest()), CONTENT)
        self.assertEqual([r for path, r in self.hub.asked if path.startswith("/cdn")], [f"bytes={len(CONTENT)}-"])

    # P05 ---------------------------------------------------------------------------------------------
    def test_p05_a_file_that_is_not_what_the_hub_describes_is_not_kept(self):
        self.hub.claims = hashlib.sha256(b"something else").hexdigest()
        job = self.ask()
        wait(self.fetcher, job, ("failed",))
        self.assertIn("checksum", job.error)
        self.assertFalse(os.path.exists(self.dest()))
        self.assertFalse(os.path.exists(self.dest() + ".part"), "what is wrong is not kept to be gone on from")

    # P06 ---------------------------------------------------------------------------------------------
    def test_p06_a_fetch_that_stops_short_keeps_what_came_and_asking_again_finishes_it(self):
        self.hub.stop_after = 1_000_000
        job = self.ask()
        wait(self.fetcher, job, ("failed",))
        self.assertIn("of", job.error)
        self.assertEqual(os.path.getsize(self.dest() + ".part"), 1_000_000)
        self.assertFalse(os.path.exists(self.dest()))
        self.hub.stop_after = None
        job = self.ask()
        wait(self.fetcher, job, ("done",))
        self.assertEqual(read(self.dest()), CONTENT)

    # P07 ---------------------------------------------------------------------------------------------
    def test_p07_a_fetch_the_person_stops_leaves_nothing_and_the_next_one_goes_on(self):
        self.hub.pause = 0.05
        first = self.ask("a.safetensors")
        second = self.ask("b.safetensors")
        self.assertEqual(second.state, "queued", "one at a time")
        end = time.time() + 10
        while first.received < 300_000 and time.time() < end:
            time.sleep(0.02)
        self.assertTrue(self.fetcher.cancel(first.id))
        wait(self.fetcher, first, ("cancelled",))
        self.assertFalse(os.path.exists(self.dest("a.safetensors") + ".part"))
        self.hub.pause = 0
        wait(self.fetcher, second, ("done",))
        self.assertEqual(read(self.dest("b.safetensors")), CONTENT)
        # a download that has not begun is stopped at once
        self.hub.pause = 0.2
        busy = self.ask("c.safetensors")
        waiting = self.ask("d.safetensors")
        self.assertTrue(self.fetcher.cancel(waiting.id))
        self.assertEqual(waiting.state, "cancelled")
        self.assertFalse(self.fetcher.cancel("nothing"))
        self.fetcher.cancel(busy.id)
        wait(self.fetcher, busy, ("cancelled",))

    # P08 ---------------------------------------------------------------------------------------------
    def test_p08_a_file_that_is_there_is_not_fetched_again_and_the_same_ask_is_one_download(self):
        os.makedirs(os.path.dirname(self.dest()))
        with open(self.dest(), "wb") as there:
            there.write(b"mine")
        job = self.ask()
        self.assertEqual(job.state, "exists")
        self.assertEqual(read(self.dest()), b"mine", "never written over")
        self.assertEqual(self.hub.asked, [])
        self.hub.pause = 0.05
        one = self.ask("x.safetensors")
        two = self.ask("x.safetensors")
        self.assertIs(one, two)
        self.fetcher.cancel(one.id)
        wait(self.fetcher, one, ("cancelled",))

    # P09 --------------------------------------------------------------------------------------------
    def test_p09_a_hub_that_says_no_is_told_to_the_person_and_the_page_may_leave_it_to_the_browser(self):
        self.hub.status = 401
        job = self.ask()
        wait(self.fetcher, job, ("failed",))
        self.assertIn("login", job.error)
        self.assertTrue(job.view()["browser"], "a file that wants a login may be saved by the browser, which has the login")
        self.hub.status = 404
        job = self.ask("gone.safetensors")
        wait(self.fetcher, job, ("failed",))
        self.assertFalse(job.view()["browser"], "a file that is not there is not there for the browser either")
        self.hub.status = 500
        job = self.ask("sick.safetensors")
        wait(self.fetcher, job, ("failed",))
        self.assertIn("500", job.error)
        self.assertEqual([n for n in os.listdir(os.path.join(self.models, "vae"))], [], "nothing is left by a no")

    # P10 --------------------------------------------------------------------------------------------
    def test_p10_a_file_that_does_not_fit_on_the_disk_is_not_begun(self):
        real = shutil.disk_usage
        shutil.disk_usage = lambda path: shutil._ntuple_diskusage(10**12, 10**12 - 1000, 1000)
        try:
            job = self.ask()
            wait(self.fetcher, job, ("failed",))
        finally:
            shutil.disk_usage = real
        self.assertIn("not enough room", job.error)
        self.assertFalse(os.path.exists(self.dest() + ".part"))

    # P11 --------------------------------------------------------------------------------------------
    def test_p11_the_folder_is_the_kinds_own_and_a_name_with_a_folder_in_it_keeps_it(self):
        job = self.ask("model.safetensors", "diffusion_models")
        self.assertEqual(job.dest, os.path.join(self.models, "diffusion_models", "model.safetensors"),
                         "ComfyUI looks in `unet` first; the folder of the kind's own name is where models are kept")
        wait(self.fetcher, job, ("done",))
        job = self.ask("flux/dev.safetensors", "vae")
        wait(self.fetcher, job, ("done",))
        self.assertEqual(read(self.dest("flux/dev.safetensors")), CONTENT)
        self.assertEqual(self.fetcher.views()[-1]["name"], "flux/dev.safetensors")
        if hasattr(os, "symlink"):
            os.symlink(tempfile.gettempdir(), os.path.join(self.models, "vae", "away"))
            with self.assertRaises(fetch.Refused):
                self.ask("away/x.safetensors", "vae")


if __name__ == "__main__":
    unittest.main()
