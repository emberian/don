#!/usr/bin/env python3
"""Host-side controller for the fail-closed retail-control DLL."""

from __future__ import annotations

import argparse
import http.server
import json
import os
from pathlib import Path
import random
import socketserver
import subprocess
import sys
import threading
import time


HERE = Path(__file__).resolve().parent
VM = "Windows 11"
GUEST_ROOT = r"C:\Users\Public\don-retail-control"
EXPECTED_SHA256 = "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079"


def run(cmd: list[str], *, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                          check=check)


def guest_cmd(command: str, *, check: bool = True) -> str:
    p = run(["prlctl", "exec", VM, "cmd.exe", "/d", "/s", "/c", command], check=check)
    return p.stdout.replace("\r\n", "\n").strip()


def guest_ps(command: str, *, check: bool = True) -> str:
    p = run(["prlctl", "exec", VM, "powershell.exe", "-NoProfile", "-Command", command],
            check=check)
    return p.stdout.replace("\r\n", "\n").strip()


def pid() -> int:
    out = guest_cmd("for /f \"tokens=2\" %p in ('tasklist /nh /fi \"imagename eq riseofnations.exe\"') do @echo %p")
    values = [line.strip() for line in out.splitlines() if line.strip().isdigit()]
    if len(values) != 1:
        raise SystemExit(f"expected one riseofnations.exe, got {values!r}:\n{out}")
    return int(values[0])


class ReusableTCPServer(socketserver.TCPServer):
    allow_reuse_address = True


def serve_once(port: int):
    handler = lambda *a, **kw: http.server.SimpleHTTPRequestHandler(  # noqa: E731
        *a, directory=str(HERE), **kw
    )
    server = ReusableTCPServer(("0.0.0.0", port), handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server


def build() -> None:
    p = run(["sh", str(HERE / "build.sh")])
    print(p.stdout, end="")


def preflight(target_pid: int) -> None:
    out = guest_ps(f"(Get-FileHash -Algorithm SHA256 (Get-Process -Id {target_pid}).Path).Hash")
    compact = "".join(out.lower().split())
    if EXPECTED_SHA256 not in compact:
        raise SystemExit(f"REFUSING unsupported target digest; expected {EXPECTED_SHA256}:\n{out}")


def deploy(target_pid: int, port: int) -> None:
    build()
    preflight(target_pid)
    server = serve_once(port)
    try:
        guest_cmd(f'if not exist "{GUEST_ROOT}" mkdir "{GUEST_ROOT}"')
        guest_cmd(
            f'curl.exe -f -sS -o "{GUEST_ROOT}\\retail_control.dll" '
            f'http://10.211.55.2:{port}/retail_control.dll'
        )
        guest_cmd(f'del /q "{GUEST_ROOT}\\STOP" "{GUEST_ROOT}\\ready.txt" 2>nul & exit /b 0')
        injector = r"C:\Users\ember\donhook\donject.exe"
        out = guest_cmd(f'"{injector}" inject {target_pid} "{GUEST_ROOT}\\retail_control.dll"')
        print(out)
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            ready = guest_cmd(f'if exist "{GUEST_ROOT}\\ready.txt" type "{GUEST_ROOT}\\ready.txt"',
                              check=False)
            if "state=armed" in ready:
                print(ready)
                return
            if "state=refused" in ready:
                raise SystemExit(ready)
            time.sleep(0.1)
        raise SystemExit("DLL loaded but did not publish state=armed")
    finally:
        server.shutdown()
        server.server_close()


def next_seq() -> int:
    return ((int(time.time() * 1000) & 0x7FFFFFFF) ^ random.getrandbits(20)) or 1


def validate_words(words: list[str]) -> None:
    if not words:
        raise SystemExit("a retail command is required")
    allowed = {"observe", "pause", "speed", "speed-up", "speed-down", "checksum",
               "move", "halt", "attack"}
    if words[0] not in allowed:
        raise SystemExit(f"unsupported verb {words[0]!r}")
    for word in words:
        if not word or any(c not in "abcdefghijklmnopqrstuvwxyz-0123456789xABCDEF" for c in word):
            raise SystemExit(f"unsafe token {word!r}")


def send(words: list[str], timeout: float) -> list[dict]:
    validate_words(words)
    seq = next_seq()
    line = " ".join([str(seq), *words])
    # A rename makes the one-slot request atomic from the worker's point of view.
    guest_cmd(
        f'(echo {line})>"{GUEST_ROOT}\\request.tmp" && '
        f'move /y "{GUEST_ROOT}\\request.tmp" "{GUEST_ROOT}\\request.txt" >nul'
    )
    deadline = time.monotonic() + timeout
    seen: dict[str, dict] = {}
    while time.monotonic() < deadline:
        out = guest_cmd(f'if exist "{GUEST_ROOT}\\events.ndjson" type "{GUEST_ROOT}\\events.ndjson"',
                        check=False)
        for raw in out.splitlines():
            raw = raw.strip()
            if not raw.startswith("{"):
                continue
            try:
                event = json.loads(raw)
            except json.JSONDecodeError:
                continue
            if event.get("seq") == seq:
                seen[event["phase"]] = event
        if "rejected" in seen or "observed" in seen or "applied" in seen:
            break
        if "queued" in seen and words[0] not in {"pause", "speed", "move", "halt", "attack"}:
            break
        time.sleep(0.05)
    if not seen:
        raise SystemExit(
            "no main-thread response; the process is attached but TurnControl::do_frame is not running"
        )
    for event in seen.values():
        print(json.dumps(event, sort_keys=True))
    return list(seen.values())


def status() -> None:
    target_pid = pid()
    print(f"pid={target_pid}")
    print(guest_cmd(f'tasklist /v /fi "pid eq {target_pid}"'))
    print(guest_cmd(f'if exist "{GUEST_ROOT}\\ready.txt" type "{GUEST_ROOT}\\ready.txt"',
                    check=False))


def stop() -> None:
    guest_cmd(f'(echo stop)>"{GUEST_ROOT}\\STOP"')
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        ready = guest_cmd(f'type "{GUEST_ROOT}\\ready.txt"', check=False)
        if "state=parked" in ready:
            print(ready)
            return
        time.sleep(0.05)
    raise SystemExit("STOP written but hook did not report parked")


def rearm() -> None:
    guest_cmd(f'del /q "{GUEST_ROOT}\\STOP"')
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        ready = guest_cmd(f'type "{GUEST_ROOT}\\ready.txt"', check=False)
        if "state=armed" in ready:
            print(ready)
            return
        time.sleep(0.05)
    raise SystemExit("STOP removed but hook did not report armed")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    sub = ap.add_subparsers(dest="action", required=True)
    sub.add_parser("status")
    sub.add_parser("build")
    d = sub.add_parser("deploy")
    d.add_argument("--pid", type=int)
    d.add_argument("--port", type=int, default=18082)
    s = sub.add_parser("send")
    s.add_argument("--timeout", type=float, default=5.0)
    s.add_argument("command", nargs=argparse.REMAINDER)
    sub.add_parser("stop")
    sub.add_parser("rearm")
    a = ap.parse_args()
    if a.action == "status": status()
    elif a.action == "build": build()
    elif a.action == "deploy": deploy(a.pid or pid(), a.port)
    elif a.action == "send": send(a.command, a.timeout)
    elif a.action == "stop": stop()
    elif a.action == "rearm": rearm()


if __name__ == "__main__":
    main()
