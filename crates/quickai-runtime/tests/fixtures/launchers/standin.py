#!/usr/bin/env python3
"""A stand-in for llama-server, for the tests of the headless program (docs/headless.md).

It does what the program is started to do and no more: it writes the arguments it was given beside itself
(`argv.txt`, so that a test can say what was on its command line, and `env.txt`, its environment), says that it listens, answers `/health` and
`/v1/models` on the port it was given, and stays until it is stopped. With `STANDIN_CRASH_AFTER=N` in its
environment it ends by itself after N seconds, as a program that crashes does.
"""
import json
import os
import sys
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer

args = sys.argv[1:]
here = os.path.dirname(os.path.abspath(__file__))
with open(os.path.join(here, "argv.txt"), "w") as f:
    f.write("\n".join(args) + "\n")
# and the environment it was started with, one NAME=value a line, for a test that asks what reached it
with open(os.path.join(here, "env.txt"), "w") as f:
    f.write("".join(f"{name}={value}\n" for name, value in sorted(os.environ.items())))


def value(flag, default=None):
    return args[args.index(flag) + 1] if flag in args else default


port = int(value("--port", "8080"))


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        if self.path == "/health":
            body = b'{"status":"ok"}'
        elif self.path == "/v1/models":
            body = json.dumps({"data": [{"id": os.path.basename(value("--model", "none"))}]}).encode()
        else:
            self.send_response(404)
            self.end_headers()
            return
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


crash = os.environ.get("STANDIN_CRASH_AFTER")
if crash:
    threading.Timer(float(crash), lambda: os._exit(3)).start()

print(f"standin: listening on 127.0.0.1:{port}", flush=True)
HTTPServer(("127.0.0.1", port), Handler).serve_forever()
