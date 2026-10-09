"""Fetching one model file for ComfyUI's page, whole or not at all. Standard library only.

ComfyUI's page offers "Download" on a model that a workflow names and the machine lacks. In a browser that
button only asks the browser to save the file, in the browser's own folder. What this does instead, when the
page asks it to: fetch the file from the hub the page names, into the folder of its kind in ComfyUI's models
folder, under the name the workflow gives it, so that the workflow finds it.

What it will do, and nothing else:

* fetch from `huggingface.co` over https (and from the download servers the hub sends it on to, over https);
* a `.safetensors` or `.gguf` file (the formats that hold numbers and cannot hold a program), under a plain
  file name, into a folder that is one of the models folder's;
* write `<name>.part` and put it in place by a rename only when it is whole and, when the hub says what its
  checksum is, when it hashes to that; a `.part` that a stop left is gone on from, not fetched again;
* one file at a time, and never over a file that is there.

A request that is not that is refused with a sentence for the person, and `Refused.browser` says whether the
page may leave it to the browser, which is what it did before there was this.
"""

import hashlib
import http.client
import os
import queue
import re
import shutil
import ssl
import threading
import time
import traceback
import urllib.error
import urllib.parse
import urllib.request
import uuid
from collections import OrderedDict, deque

CHUNK = 1 << 20                 # what is read, hashed and written at a time
REDIRECTS = 8                   # the hub sends a file on to its download server: once, or twice
TIMEOUT = 30                    # seconds without a byte after which a fetch is given up (its .part is kept)
KEEP_FREE = 512 << 20           # what a fetch leaves free on the disk
SPEED_WINDOW = 8.0              # seconds the speed is the mean of
KEPT = 40                       # downloads remembered, for the page to show what became of them
SUFFIXES = (".safetensors", ".sft", ".gguf")
USER_AGENT = "QuickAI-ComfyUI-bridge/1"

_SEGMENT = re.compile(r"[^\x00-\x1f\\/:*?\"<>|]{1,200}")
_SHA256 = re.compile(r"[0-9a-f]{64}")


class Refused(Exception):
    """A request that is not carried out. The text is for the person; `browser` says that it is a file the
    page may as well leave to the browser (an address that is not the hub's, a file that wants a login)."""

    def __init__(self, text, browser=False):
        super().__init__(text)
        self.browser = browser


class Failed(Exception):
    """A fetch that stopped. What was fetched is kept, so that asking again goes on from it."""


class Cancelled(Exception):
    """The person stopped it."""


class Again(Exception):
    """What is on the disk is not a part of the file the hub has now: begin again, from nothing."""


class Policy:
    """Which addresses may be fetched. The default is the hub, over https; a test says another."""

    def __init__(self, hosts=("huggingface.co",), schemes=("https",), any_port=False):
        self.hosts = tuple(hosts)
        self.schemes = tuple(schemes)
        self.any_port = any_port

    def check_start(self, url):
        """The address the page gives."""
        try:
            parts = urllib.parse.urlsplit(url)
            port = parts.port
        except (ValueError, TypeError, AttributeError):         # not text, or not an address
            raise Refused("that is not an address", browser=True)
        host = (parts.hostname or "").lower()
        if parts.scheme not in self.schemes or host not in self.hosts or parts.username or parts.password:
            names = ", ".join(self.hosts)
            raise Refused(f"only files of {names} are fetched here", browser=True)
        if port not in (None, 443) and not self.any_port:
            raise Refused("that address has a port the hub does not use", browser=True)
        return parts

    def check_hop(self, url):
        """An address the hub sends the fetch on to: its download server, whatever its name, over https."""
        scheme = urllib.parse.urlsplit(url).scheme
        if scheme not in self.schemes:
            raise Refused("the hub sent the fetch on to an address that is not secure")


def plain_segments(name):
    """`vae/ae.safetensors` -> ['vae', 'ae.safetensors']: the parts of a file name that stays in its folder."""
    if not isinstance(name, str) or not name or len(name) > 400:
        raise Refused("the file has no name", browser=True)
    parts = name.split("/")
    if len(parts) > 4:
        raise Refused(f"{name} is not a plain file name", browser=True)
    for part in parts:
        if not _SEGMENT.fullmatch(part) or part.startswith(".") or part.endswith((" ", ".")):
            raise Refused(f"{name} is not a plain file name", browser=True)
    if not name.lower().endswith(SUFFIXES):
        raise Refused(f"{name} is not a .safetensors or .gguf file, and only those are fetched here", browser=True)
    return parts


def inside(base, path):
    """Is `path` below `base`, once the links in it are followed."""
    base, path = os.path.realpath(base), os.path.realpath(path)
    try:
        return path != base and os.path.commonpath([base, path]) == base
    except ValueError:          # another drive
        return False


def folder_for(directory, where, root):
    """The folder of a kind of model (`vae`) that is inside the models folder. `where(kind)` says the folders
    ComfyUI looks in for that kind, or nothing when it has no such kind."""
    paths = where(directory) if isinstance(directory, str) else None
    if not paths:
        raise Refused(f"ComfyUI has no models folder called {directory}", browser=True)
    below = [path for path in paths if inside(root, path)]
    if not below:
        raise Refused(f"{directory} is not a folder of the models folder", browser=True)
    # `diffusion_models` is looked for in `unet` first, which is an old name: the folder of the kind's own name
    # is where QuickAI and everyone else keeps it
    for path in below:
        if os.path.basename(os.path.normpath(path)) == directory:
            return path
    return below[0]


def human(size):
    for unit in ("bytes", "KB", "MB"):
        if size < 1000:
            return f"{size} bytes" if unit == "bytes" else f"{size:.1f} {unit}"
        size /= 1000.0
    return f"{size:.1f} GB"


def describe(error):
    """What went wrong with the network, in words."""
    if isinstance(error, urllib.error.HTTPError):
        return f"the hub answered {error.code} {error.reason}"
    if isinstance(error, urllib.error.URLError):
        return f"the hub could not be reached ({error.reason})"
    if isinstance(error, (TimeoutError, ssl.SSLError, http.client.HTTPException, ConnectionError)):
        return f"the connection broke ({error})"
    return f"{error}"


_context = None


def tls():
    """The certificates to trust: the system's, and the bundle of `certifi` where a Python has none."""
    global _context
    if _context is None:
        context = ssl.create_default_context()
        if not context.get_ca_certs():
            try:
                import certifi
                context = ssl.create_default_context(cafile=certifi.where())
            except Exception:
                pass
        _context = context
    return _context


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    """Redirects are followed by `open_url`, which looks at every address it is sent to."""

    def redirect_request(self, *args, **kwargs):
        return None


def open_url(url, headers, policy, timeout=TIMEOUT):
    """The answer to a GET, and the headers of the answers that sent it on. An error status raises
    `HTTPError`, which has the same list as `hops`."""
    opener = urllib.request.build_opener(_NoRedirect, urllib.request.HTTPSHandler(context=tls()))
    hops = []
    for _ in range(REDIRECTS + 1):
        try:
            response = opener.open(urllib.request.Request(url, headers=headers), timeout=timeout)
            return response, hops
        except urllib.error.HTTPError as error:
            location = error.headers.get("Location") if error.code in (301, 302, 303, 307, 308) else None
            if not location:
                error.hops = hops
                raise
            hops.append(error.headers)
            error.close()
            url = urllib.parse.urljoin(url, location)
            policy.check_hop(url)
    raise Failed("the hub sent the fetch on too many times")


def checksum_of(headers):
    """The sha256 the hub says the file has (`X-Linked-Etag`, on the answer that sends it to the download
    server), or nothing."""
    value = (headers.get("X-Linked-Etag") or "").strip()
    if value.startswith("W/"):
        value = value[2:]
    value = value.strip('"').lower()
    return value if _SHA256.fullmatch(value) else None


def content_range(text):
    """`bytes 100-199/200` -> (100, 200); `bytes */200` -> (None, 200)."""
    found = re.fullmatch(r"\s*bytes\s+(?:(\d+)-\d+|\*)/(\d+)\s*", text or "")
    if not found:
        return None, 0
    return (int(found.group(1)) if found.group(1) is not None else None), int(found.group(2))


class Download:
    """One file being fetched, and what became of it. The states: queued, running, checking (what an earlier
    stop left is read again to be hashed), done, exists (it was there), failed, cancelled."""

    def __init__(self, url, name, directory, dest):
        self.id = uuid.uuid4().hex[:12]
        self.url = url
        self.name = name
        self.directory = directory
        self.dest = dest
        self.state = "queued"
        self.received = 0
        self.total = 0
        self.error = ""
        self.browser = False
        self.cancel = threading.Event()
        self.samples = deque()
        self.began = time.time()
        self.ended = 0.0

    def sample(self):
        now = time.monotonic()
        if self.samples and now - self.samples[-1][0] < 0.5:
            return
        self.samples.append((now, self.received))
        while len(self.samples) > 2 and now - self.samples[0][0] > SPEED_WINDOW:
            self.samples.popleft()

    def speed(self):
        if self.state != "running" or len(self.samples) < 2:
            return 0
        (t0, r0), (t1, r1) = self.samples[0], self.samples[-1]
        return int((r1 - r0) / (t1 - t0)) if t1 > t0 else 0

    def view(self):
        return {
            "id": self.id,
            "name": self.name,
            "directory": self.directory,
            "state": self.state,
            "received": self.received,
            "total": self.total,
            "speed": self.speed(),
            "error": self.error,
            "browser": self.browser,
        }


class Fetcher:
    """The downloads of one ComfyUI: a queue, and a thread that takes them one at a time.

    `where(kind)` says the folders ComfyUI looks in for a kind of model, `root` is its models folder, `say` is
    where the words for its log go."""

    def __init__(self, where, root, policy=None, say=None):
        self.where = where
        self.root = root
        self.policy = policy or Policy()
        self.say = say or (lambda text: None)
        self.lock = threading.Lock()
        self.jobs = OrderedDict()
        self.todo = queue.Queue()
        self.worker = None

    # -- what the page asks -------------------------------------------------------------------------

    def add(self, url, name, directory):
        """Takes a request, or refuses it with `Refused`. The same file asked for again is the same download."""
        parts = plain_segments(name)
        self.policy.check_start(url)
        folder = folder_for(directory, self.where, self.root)
        dest = os.path.join(folder, *parts)
        if len(parts) > 1 and not inside(folder, os.path.dirname(dest)):
            raise Refused(f"{name} is not a plain file name", browser=True)
        with self.lock:
            for job in self.jobs.values():
                if job.dest == dest and job.state in ("queued", "running", "checking"):
                    return job
            job = Download(url, name, directory, dest)
            if os.path.exists(dest):
                job.state = "exists"
                job.ended = time.time()
                self._remember(job)
                return job
            self._remember(job)
            self.todo.put(job)
            if self.worker is None:
                self.worker = threading.Thread(target=self._work, name="quickai-bridge", daemon=True)
                self.worker.start()
        return job

    def views(self):
        with self.lock:
            return [job.view() for job in self.jobs.values()]

    def cancel(self, ident):
        with self.lock:
            job = self.jobs.get(ident)
            if job is None:
                return False
            job.cancel.set()
            if job.state == "queued":
                job.state = "cancelled"
                job.ended = time.time()
            return True

    def _remember(self, job):
        self.jobs[job.id] = job
        while len(self.jobs) > KEPT:
            oldest = next(iter(self.jobs))
            if self.jobs[oldest].state in ("queued", "running", "checking"):
                break
            del self.jobs[oldest]

    # -- the thread ------------------------------------------------------------------------------------

    def _work(self):
        while True:
            job = self.todo.get()
            if job.cancel.is_set() or job.state != "queued":
                continue
            self._run(job)

    def _run(self, job):
        job.state = "running"
        self.say(f"fetching {job.name} into {os.path.dirname(job.dest)}")
        try:
            try:
                self._fetch(job)
            except Again:
                self.say(f"{job.name}: what was fetched before is not the file the hub has now; beginning again")
                self._drop_part(job)
                self._fetch(job, may_resume=False)
        except Again:
            job.state, job.error = "failed", "the file at the hub kept changing while it was fetched"
        except Cancelled:
            self._drop_part(job)
            job.state = "cancelled"
        except Refused as error:
            job.state, job.error, job.browser = "failed", str(error), error.browser
        except Failed as error:
            job.state, job.error = "failed", str(error)
        except (urllib.error.URLError, OSError, http.client.HTTPException) as error:
            job.state, job.error = "failed", describe(error)
        except Exception as error:      # a mistake of this program: the thread must outlive it
            self.say(traceback.format_exc())
            job.state, job.error = "failed", f"something went wrong in the bridge: {error}"
        job.ended = time.time()
        if job.state == "done":
            self.say(f"{job.name} is in place ({human(job.received)})")
        else:
            self.say(f"{job.name}: {job.state}" + (f", {job.error}" if job.error else ""))

    def _drop_part(self, job):
        try:
            os.remove(job.dest + ".part")
        except FileNotFoundError:
            pass

    def _fetch(self, job, may_resume=True):
        part = job.dest + ".part"
        folder = os.path.dirname(job.dest)
        os.makedirs(folder, exist_ok=True)
        offset = os.path.getsize(part) if may_resume and os.path.isfile(part) else 0
        headers = {"User-Agent": USER_AGENT, "Accept-Encoding": "identity"}
        if offset:
            headers["Range"] = f"bytes={offset}-"
        try:
            response, hops = open_url(job.url, headers, self.policy)
        except urllib.error.HTTPError as error:
            hops = getattr(error, "hops", [])
            if error.code == 416 and offset:
                _, total = content_range(error.headers.get("Content-Range"))
                error.close()
                if total == offset:     # nothing is left to fetch: only to check and put in place
                    job.total = job.received = total
                    return self._finish(job, part, (hops[0] if hops else error.headers), None, True)
                raise Again()
            error.close()
            raise self._refusal(job, error)
        with response:
            first = hops[0] if hops else response.headers
            expected = checksum_of(first)
            if response.status == 206:
                start, total = content_range(response.headers.get("Content-Range"))
                if start != offset:
                    raise Again()
            elif response.status == 200:
                offset = 0
                total = int(response.headers.get("Content-Length") or first.get("X-Linked-Size") or 0)
            else:
                raise Failed(f"the hub answered {response.status}")
            job.total = total
            free = shutil.disk_usage(folder).free
            if total and free < total - offset + KEEP_FREE:
                raise Failed(
                    f"there is not enough room in {folder}: {human(total - offset)} more are needed and "
                    f"{human(free)} are free"
                )
            digest = hashlib.sha256() if expected else None
            if offset and digest:
                self._hash_part(job, part, offset, digest)
            job.received = offset
            job.state = "running"
            hosts = dict.fromkeys(
                host for host in (urllib.parse.urlsplit(u).hostname for u in (job.url, response.geturl())) if host
            )
            self.say(
                f"{job.name}: {human(total)} from {' via '.join(hosts)}"
                + (f", going on from {human(offset)}" if offset else "")
            )
            with open(part, "ab" if offset else "wb") as out:
                while True:
                    block = response.read(CHUNK)
                    if not block:
                        break
                    if job.cancel.is_set():
                        raise Cancelled()
                    out.write(block)
                    if digest:
                        digest.update(block)
                    job.received += len(block)
                    job.sample()
                out.flush()
                os.fsync(out.fileno())
        self._finish(job, part, first, digest, offset > 0)

    def _hash_part(self, job, part, size, digest):
        """What an earlier stop left is hashed again, so that the checksum at the end is of the whole."""
        job.state = "checking"
        left = size
        with open(part, "rb") as old:
            while left:
                block = old.read(min(CHUNK * 8, left))
                if not block:
                    raise Again()
                digest.update(block)
                left -= len(block)
                job.received = size - left
                if job.cancel.is_set():
                    raise Cancelled()

    def _finish(self, job, part, headers, digest, resumed):
        size = os.path.getsize(part)
        if job.total and size != job.total:
            raise Failed(f"only {human(size)} of {human(job.total)} arrived; asking again goes on from there")
        expected = checksum_of(headers)
        if expected:
            if digest is None:
                digest = hashlib.sha256()
                self._hash_part(job, part, size, digest)
            if digest.hexdigest() != expected:
                if resumed:
                    raise Again()
                self._drop_part(job)
                raise Failed("what arrived is not the file the hub describes (its checksum is different); it was not kept")
        os.replace(part, job.dest)
        job.received = size
        job.state = "done"

    def _refusal(self, job, error):
        if error.code in (401, 403):
            return Refused(f"{job.name} wants a login at the hub", browser=True)
        if error.code == 404:
            return Refused("the hub has no such file")
        return Failed(describe(error))
