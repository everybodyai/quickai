#!/usr/bin/env python3
"""A forwarding HTTP CONNECT proxy that writes down where it was asked to go.

`record.sh --pull` points the real Ollama's HTTPS_PROXY here, so the hosts a model pull really
contacts are measured, not remembered (docs/phase3-test-plan.md, group C). It forwards everything
and looks at nothing but the first line of each CONNECT request.

    connect_proxy.py PORT LOGFILE
"""
import socket
import sys
import threading


def pump(a, b):
    try:
        while True:
            data = a.recv(65536)
            if not data:
                break
            b.sendall(data)
    except OSError:
        pass
    finally:
        for s in (a, b):
            try:
                s.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass


def handle(client, log_path, lock):
    try:
        head = b""
        while b"\r\n\r\n" not in head:
            chunk = client.recv(4096)
            if not chunk:
                return
            head += chunk
        line = head.split(b"\r\n", 1)[0].decode("latin-1")
        parts = line.split()
        if len(parts) < 2 or parts[0] != "CONNECT":
            client.sendall(b"HTTP/1.1 405 Method Not Allowed\r\n\r\n")
            return
        host, _, port = parts[1].rpartition(":")
        with lock:
            with open(log_path, "a") as f:
                f.write(f"{host}:{port}\n")
        upstream = socket.create_connection((host, int(port)), timeout=30)
        client.sendall(b"HTTP/1.1 200 Connection established\r\n\r\n")
        t = threading.Thread(target=pump, args=(upstream, client), daemon=True)
        t.start()
        pump(client, upstream)
    except OSError:
        pass
    finally:
        client.close()


def main():
    port, log_path = int(sys.argv[1]), sys.argv[2]
    server = socket.socket()
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind(("127.0.0.1", port))
    server.listen(32)
    lock = threading.Lock()
    while True:
        client, _ = server.accept()
        threading.Thread(target=handle, args=(client, log_path, lock), daemon=True).start()


if __name__ == "__main__":
    main()
