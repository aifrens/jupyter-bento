#!/usr/bin/env python3
"""Start Notebook on loopback and verify health plus recent-file callback."""

from __future__ import annotations

import argparse
import json
import os
import selectors
import signal
import socket
import subprocess
import threading
import time
import urllib.parse
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def runtime_python(env_root: Path) -> Path:
    """返回当前平台快照中的解释器路径。"""
    windows_python = env_root / "python.exe"
    if windows_python.exists():
        return windows_python
    python = env_root / "bin" / "python"
    if python.exists():
        return python
    return env_root / "bin" / "python3"


class RecentCallbackHandler(BaseHTTPRequestHandler):
    """捕获 Handler 发出的单次最近文件回调。"""

    expected_token = ""
    event = threading.Event()
    received: list[dict[str, object]] = []

    def do_POST(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler API
        length = int(self.headers.get("Content-Length", "0"))
        raw_body = self.rfile.read(length)
        self.__class__.received.append(
            {
                "path": self.path,
                "authorization": self.headers.get("Authorization"),
                "content_type": self.headers.get("Content-Type"),
                "payload": json.loads(raw_body.decode("utf-8")),
            }
        )
        self.__class__.event.set()
        self.send_response(204)
        self.end_headers()

    def log_message(self, _format: str, *_args: object) -> None:
        pass


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--env", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    args = parser.parse_args()

    args.root.mkdir(parents=True, exist_ok=True)
    notebook_relative_path = "中文 子目录/季度 报告.ipynb"
    notebook_path = args.root / notebook_relative_path
    notebook_path.parent.mkdir(parents=True, exist_ok=True)
    notebook_path.write_text(
        json.dumps(
            {
                "cells": [],
                "metadata": {},
                "nbformat": 4,
                "nbformat_minor": 4,
            }
        ),
        encoding="utf-8",
    )

    port = free_port()
    token = "local-validation-token"
    callback_token = "recent-callback-token"
    RecentCallbackHandler.expected_token = callback_token
    RecentCallbackHandler.event.clear()
    RecentCallbackHandler.received = []
    callback_server = ThreadingHTTPServer(
        ("127.0.0.1", 0), RecentCallbackHandler
    )
    callback_thread = threading.Thread(
        target=callback_server.serve_forever,
        name="recent-callback-test-server",
        daemon=True,
    )
    callback_thread.start()
    callback_host, callback_port = callback_server.server_address
    callback_url = f"http://{callback_host}:{callback_port}/recent"

    resource_parent = Path(__file__).resolve().parents[2] / "app" / "src-tauri" / "resources"
    env = os.environ.copy()
    env.update(
        {
            "PYTHONNOUSERSITE": "1",
            "JUPYTER_CONFIG_DIR": str(args.root / "jupyter-config"),
            "JUPYTER_DATA_DIR": str(args.root / "jupyter-data"),
            "JUPYTER_RUNTIME_DIR": str(args.root / "jupyter-runtime"),
            "IPYTHONDIR": str(args.root / "ipython"),
            "MPLCONFIGDIR": str(args.root / "mpl"),
            "PYTHONPATH": str(resource_parent),
            "JUPITER_RECENT_CALLBACK_URL": callback_url,
            "JUPITER_RECENT_CALLBACK_TOKEN": callback_token,
        }
    )
    for key in (
        "JUPYTER_CONFIG_DIR",
        "JUPYTER_DATA_DIR",
        "JUPYTER_RUNTIME_DIR",
        "IPYTHONDIR",
        "MPLCONFIGDIR",
    ):
        Path(env[key]).mkdir(parents=True, exist_ok=True)

    config_path = Path(env["JUPYTER_CONFIG_DIR"]) / "jupyter_notebook_config.py"
    config_path.write_text(
        'c.NotebookApp.extra_services = ["jupiter_recent_handler.handlers"]\n',
        encoding="utf-8",
    )

    command = [
        str(runtime_python(args.env)),
        "-m",
        "notebook",
        "--no-browser",
        "--ip=127.0.0.1",
        f"--port={port}",
        f"--NotebookApp.token={token}",
        "--NotebookApp.allow_remote_access=False",
        f"--notebook-dir={args.root}",
    ]
    process = subprocess.Popen(
        command,
        cwd=args.root,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        start_new_session=True,
    )
    selector = selectors.DefaultSelector()
    if process.stdout is not None:
        selector.register(process.stdout, selectors.EVENT_READ)
    output: list[str] = []
    deadline = time.monotonic() + 45
    try:
        while time.monotonic() < deadline:
            events = selector.select(timeout=0.25)
            for key, _ in events:
                line = key.fileobj.readline()
                if line:
                    output.append(line.rstrip())
            if process.poll() is not None:
                break
            if any("Jupyter Notebook" in line and "running at" in line for line in output):
                break

        url = f"http://127.0.0.1:{port}/tree?token={token}"
        response = urllib.request.urlopen(url, timeout=10)
        body = response.read(200)
        if response.status != 200:
            raise RuntimeError(f"Notebook returned HTTP {response.status}")
        print(f"Notebook OK: {url}")
        print(f"HTTP status: {response.status}; body prefix: {body[:80]!r}")

        notebook_url = (
            f"http://127.0.0.1:{port}/notebooks/"
            f"{urllib.parse.quote(notebook_relative_path, safe='/')}?token={token}"
        )
        notebook_response = urllib.request.urlopen(notebook_url, timeout=10)
        notebook_body = notebook_response.read(200)
        if notebook_response.status != 200:
            raise RuntimeError(
                f"Notebook page returned HTTP {notebook_response.status}"
            )
        if not RecentCallbackHandler.event.wait(timeout=5):
            raise RuntimeError("recent-file callback was not received")
        if len(RecentCallbackHandler.received) != 1:
            raise RuntimeError(
                "recent-file callback count mismatch: "
                f"{len(RecentCallbackHandler.received)}"
            )
        callback = RecentCallbackHandler.received[0]
        expected_callback = {
            "path": "/recent",
            "authorization": f"Bearer {callback_token}",
            "content_type": "application/json; charset=utf-8",
            "payload": {"path": notebook_relative_path},
        }
        if callback != expected_callback:
            raise RuntimeError(
                "recent-file callback mismatch: "
                f"expected {expected_callback!r}, got {callback!r}"
            )
        print(f"Notebook handler OK: {notebook_url}")
        print(
            "Recent callback OK: "
            f"{callback['authorization']} {callback['payload']!r}; "
            f"body prefix: {notebook_body[:80]!r}"
        )
        return_code = 0
    except Exception as exc:
        print("Notebook validation failed:", exc)
        print("--- server output ---")
        print("\n".join(output[-80:]))
        return_code = 1
    finally:
        selector.close()
        if process.poll() is None:
            try:
                request = urllib.request.Request(
                    f"http://127.0.0.1:{port}/api/shutdown?token={token}",
                    data=b"{}",
                    method="POST",
                )
                urllib.request.urlopen(request, timeout=5)
            except Exception:
                pass
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
        callback_server.shutdown()
        callback_server.server_close()
        callback_thread.join(timeout=5)
    return return_code


if __name__ == "__main__":
    raise SystemExit(main())
