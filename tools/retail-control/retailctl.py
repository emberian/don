#!/usr/bin/env python3
"""Host-side controller for the fail-closed retail-control DLL."""

from __future__ import annotations

import argparse
import http.server
import json
import os
from pathlib import Path
import random
import re
import socketserver
import subprocess
import sys
import threading
import time


HERE = Path(__file__).resolve().parent
VM = "Windows 11"
DEFAULT_GENERATION = "v2"
LEGACY_GENERATION = "v1"
GUEST_ROOT_BASE = r"C:\Users\Public\don-retail-control"
EXPECTED_SHA256 = "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079"


def validate_generation(generation: str) -> str:
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,47}", generation):
        raise SystemExit(f"unsafe controller generation {generation!r}")
    return generation


def generation_root(generation: str) -> str:
    generation = validate_generation(generation)
    if generation == LEGACY_GENERATION:
        return GUEST_ROOT_BASE
    return f"{GUEST_ROOT_BASE}-{generation}"


def generation_dll(generation: str) -> str:
    generation = validate_generation(generation)
    if generation == LEGACY_GENERATION:
        return "retail_control.dll"
    return f"retail_control-{generation}.dll"


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


def loaded_module(target_pid: int, dll_name: str) -> str:
    # The host PowerShell is 64-bit and does not reliably enumerate emulated x86
    # modules. Use the already-deployed x86 Toolhelp probe in the same ABI instead.
    injector = r"C:\Users\ember\donhook\donject.exe"
    out = guest_cmd(f'"{injector}" base {target_pid} "{dll_name}"', check=False)
    for line in out.splitlines():
        value = line.strip()
        if re.fullmatch(r"[0-9A-Fa-f]{8}", value) and int(value, 16):
            return f"{dll_name}@0x{value.lower()}"
    return ""


def deploy(target_pid: int, port: int, generation: str) -> None:
    root = generation_root(generation)
    dll_name = generation_dll(generation)
    preflight(target_pid)
    mapped = loaded_module(target_pid, dll_name)
    if mapped:
        raise SystemExit(
            f"REFUSING to replace mapped generation {generation!r}: {mapped}\n"
            "choose a new --generation; mapped controller DLLs remain parked by design"
        )
    build()
    server = serve_once(port)
    try:
        guest_cmd(f'if not exist "{root}" mkdir "{root}"')
        guest_cmd(
            f'curl.exe -f -sS -o "{root}\\{dll_name}.download" '
            f'http://10.211.55.2:{port}/retail_control.dll'
        )
        guest_cmd(f'move /y "{root}\\{dll_name}.download" "{root}\\{dll_name}" >nul')
        guest_cmd(
            f'del /q "{root}\\STOP" "{root}\\ready.txt" "{root}\\request.txt" '
            f'"{root}\\request.tmp" "{root}\\events.ndjson" 2>nul & exit /b 0'
        )
        injector = r"C:\Users\ember\donhook\donject.exe"
        out = guest_cmd(f'"{injector}" inject {target_pid} "{root}\\{dll_name}"')
        print(out)
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            ready = guest_cmd(f'if exist "{root}\\ready.txt" type "{root}\\ready.txt"',
                              check=False)
            if "state=armed" in ready:
                print(ready)
                return
            if "state=refused" in ready:
                raise SystemExit(ready)
            time.sleep(0.1)
        raise SystemExit("DLL loaded but did not publish state=armed")
    finally:
        guest_cmd(f'del /q "{root}\\{dll_name}.download" 2>nul & exit /b 0', check=False)
        server.shutdown()
        server.server_close()


def next_seq() -> int:
    return ((int(time.time() * 1000) & 0x7FFFFFFF) ^ random.getrandbits(20)) or 1


def validate_words(words: list[str]) -> None:
    if not words:
        raise SystemExit("a retail command is required")
    allowed = {"observe", "pause", "speed", "speed-up", "speed-down", "checksum",
               "move", "halt", "attack", "trace-move", "observe-guys"}
    if words[0] not in allowed:
        raise SystemExit(f"unsupported verb {words[0]!r}")
    for word in words:
        if not word or any(c not in "abcdefghijklmnopqrstuvwxyz-0123456789xABCDEF" for c in word):
            raise SystemExit(f"unsafe token {word!r}")


def send(words: list[str], timeout: float, root: str) -> list[dict]:
    validate_words(words)
    seq = next_seq()
    line = " ".join([str(seq), *words])
    # A rename makes the one-slot request atomic from the worker's point of view.
    guest_cmd(
        f'(echo {line})>"{root}\\request.tmp" && '
        f'move /y "{root}\\request.tmp" "{root}\\request.txt" >nul'
    )
    deadline = time.monotonic() + timeout
    seen: dict[str, dict] = {}
    while time.monotonic() < deadline:
        out = guest_cmd(f'if exist "{root}\\events.ndjson" type "{root}\\events.ndjson"',
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


def normalized_trace_event(event: dict, start_frame: int, base: int) -> dict:
    runtime_vtable = int(event["order_vtable"], 16)
    preferred_vtable = runtime_vtable - base + 0x00400000 if runtime_vtable else 0
    order_kind = "MoveOrder" if preferred_vtable == 0x00B4A12C else (
        "none" if not runtime_vtable else "unresolved"
    )
    out = {
        "frame": event["frame"],
        "frame_delta": event["frame"] - start_frame,
        "seconds": event["seconds"],
        "pause": event["paused"],
        "speed": event["speed"],
        "position": {"x": event["unit_x"], "y": event["unit_y"]},
        "heading_u32": event["unit_angle"],
        "queued_action_heading_u32": event["unit_dest_angle"],
        "queued_action_position": {
            "x": event["unit_orders_x"], "y": event["unit_orders_y"]
        },
        "order": {
            "kind": order_kind,
            "length": event["order_length"],
            "preferred_vtable": f"0x{preferred_vtable:08x}",
            "flags": event["order_flags"],
            "metric": event["order_metric"],
        },
    }
    if event["move_valid"]:
        out["move_order"] = {
            "x": event["move_x"], "y": event["move_y"],
            "arrival_angle": event["move_angle"], "dest_latch": event["move_dest"],
            "tolerance": event["move_tolerance"], "pause": event["move_pause"],
            "retry": event["move_retry"], "attempts": event["move_attempts"],
            "timer": event["move_timer"], "facing": event["move_facing"],
            "active_destination": {
                "x": event["move_dest_x"], "y": event["move_dest_y"]
            },
            "last": {"x": event["move_last_x"], "y": event["move_last_y"]},
            "collision": {"x": event["move_coll_x"], "y": event["move_coll_y"]},
            "origin": {"x": event["move_orig_x"], "y": event["move_orig_y"]},
            "offset": {"x": event["move_off_x"], "y": event["move_off_y"]},
        }
    return out


def trajectory(owner: int, unit_id: int, x: int, y: int, max_frames: int,
               timeout: float, root: str, generation: str, output: Path) -> None:
    initial_events = send(["observe"], 5.0, root)
    initial = next((e for e in initial_events if e.get("phase") == "observed"), None)
    if not initial or initial.get("paused") != 1:
        raise SystemExit("REFUSING trace: retail must begin paused so the run is bounded")
    seq = next_seq()
    words = ["trace-move", str(owner), str(unit_id), str(x), str(y), str(max_frames)]
    validate_words(words)
    line = " ".join([str(seq), *words])
    guest_cmd(
        f'(echo {line})>"{root}\\request.tmp" && '
        f'move /y "{root}\\request.tmp" "{root}\\request.txt" >nul'
    )
    events: list[dict] = []
    seen: set[tuple] = set()
    terminal: dict | None = None
    failure: BaseException | None = None
    try:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            raw_events = guest_cmd(
                f'if exist "{root}\\events.ndjson" type "{root}\\events.ndjson"',
                check=False,
            )
            for raw in raw_events.splitlines():
                try:
                    event = json.loads(raw)
                except json.JSONDecodeError:
                    continue
                if event.get("seq") != seq:
                    continue
                key = (event.get("phase"), event.get("frame"), event.get("tick"),
                       event.get("command_hex"))
                if key in seen:
                    continue
                seen.add(key)
                events.append(event)
                print(json.dumps(event, sort_keys=True))
                if event.get("phase") == "rejected":
                    raise RuntimeError(f"retail rejected bounded trace (note={event.get('note')})")
                if event.get("phase") in {"trace-complete", "trace-bounded"}:
                    terminal = event
            if terminal:
                break
            time.sleep(0.05)
        if not terminal:
            raise TimeoutError("trajectory did not publish a bounded terminal record")
        queued = next((e for e in events if e.get("phase") == "queued"), None)
        if not queued:
            raise RuntimeError("trace completed without its initial queued-state record")

        base = int(initial["game"], 16)  # replaced below by the executable base from ready.txt
        ready = guest_cmd(f'type "{root}\\ready.txt"')
        for ready_line in ready.splitlines():
            if ready_line.startswith("base="):
                base = int(ready_line.split("=", 1)[1], 16)
        samples = [
            normalized_trace_event(e, queued["frame"], base)
            for e in events if e.get("phase") == "trace-sample"
        ]
        artifact = {
            "schema": "don.retail-move-trajectory.v1",
            "retail_executable_sha256": EXPECTED_SHA256,
            "controller_generation": generation,
            "coordinate_units_per_tile": 192,
            "simulation_frames_per_game_second": 15,
            "angle_encoding": "u32 binary angle; 2^32 is one turn; 0 points north/-y",
            "object_coordinate_encoding": "decoded from stored_u32 XOR 0x00063637",
            "subject": {"owner": owner, "object_id": unit_id},
            "request": {"x": x, "y": y, "max_frames": max_frames},
            "initial": normalized_trace_event(queued, queued["frame"], base),
            "retail_command_hex": queued["command_hex"],
            "termination": terminal["phase"],
            "terminal": normalized_trace_event(terminal, queued["frame"], base),
            "checksum": {
                "available": False,
                "reason": "retail network flag was 0; solo intentionally emits no checksum packet",
            },
            "samples": samples,
        }
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(artifact, indent=2) + "\n")
        print(f"wrote {len(samples)} samples to {output}")
    except BaseException as exc:
        failure = exc
    finally:
        # The recorder owns both fail-safes: restore the initial pause state, then
        # restore retail's original five call-site bytes and park this generation.
        try:
            send(["pause", "1"], 5.0, root)
        except BaseException as exc:
            if failure is None:
                failure = exc
        try:
            stop(root)
        except BaseException as exc:
            if failure is None:
                failure = exc
    if failure is not None:
        raise failure


def status(root: str) -> None:
    target_pid = pid()
    print(f"pid={target_pid}")
    print(guest_cmd(f'tasklist /v /fi "pid eq {target_pid}"'))
    print(guest_cmd(f'if exist "{root}\\ready.txt" type "{root}\\ready.txt"',
                    check=False))


def stop(root: str) -> None:
    guest_cmd(f'(echo stop)>"{root}\\STOP"')
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        ready = guest_cmd(f'type "{root}\\ready.txt"', check=False)
        if "state=parked" in ready:
            print(ready)
            return
        time.sleep(0.05)
    raise SystemExit("STOP written but hook did not report parked")


def rearm(root: str) -> None:
    guest_cmd(f'del /q "{root}\\STOP"')
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        ready = guest_cmd(f'type "{root}\\ready.txt"', check=False)
        if "state=armed" in ready:
            print(ready)
            return
        time.sleep(0.05)
    raise SystemExit("STOP removed but hook did not report armed")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    sub = ap.add_subparsers(dest="action", required=True)
    def add_generation(parser: argparse.ArgumentParser, default: str = DEFAULT_GENERATION) -> None:
        parser.add_argument("--generation", default=default)

    status_parser = sub.add_parser("status")
    add_generation(status_parser)
    sub.add_parser("build")
    d = sub.add_parser("deploy")
    d.add_argument("--pid", type=int)
    d.add_argument("--port", type=int, default=18082)
    add_generation(d)
    u = sub.add_parser("upgrade")
    u.add_argument("--pid", type=int)
    u.add_argument("--port", type=int, default=18082)
    u.add_argument("--from-generation", default=LEGACY_GENERATION)
    add_generation(u)
    s = sub.add_parser("send")
    s.add_argument("--timeout", type=float, default=5.0)
    add_generation(s)
    s.add_argument("command", nargs=argparse.REMAINDER)
    t = sub.add_parser("trajectory")
    t.add_argument("owner", type=int)
    t.add_argument("unit_id", type=int)
    t.add_argument("x", type=int)
    t.add_argument("y", type=int)
    t.add_argument("--max-frames", type=int, default=120)
    t.add_argument("--timeout", type=float, default=45.0)
    t.add_argument("--output", type=Path,
                   default=HERE.parents[1] / "schema/live/retail-move-trajectory-v1.json")
    add_generation(t)
    stop_parser = sub.add_parser("stop")
    add_generation(stop_parser)
    rearm_parser = sub.add_parser("rearm")
    add_generation(rearm_parser)
    a = ap.parse_args()
    if a.action == "status": status(generation_root(a.generation))
    elif a.action == "build": build()
    elif a.action == "deploy": deploy(a.pid or pid(), a.port, a.generation)
    elif a.action == "upgrade":
        target_pid = a.pid or pid()
        stop(generation_root(a.from_generation))
        deploy(target_pid, a.port, a.generation)
    elif a.action == "send": send(a.command, a.timeout, generation_root(a.generation))
    elif a.action == "trajectory":
        trajectory(a.owner, a.unit_id, a.x, a.y, a.max_frames, a.timeout,
                   generation_root(a.generation), a.generation, a.output.resolve())
    elif a.action == "stop": stop(generation_root(a.generation))
    elif a.action == "rearm": rearm(generation_root(a.generation))


if __name__ == "__main__":
    main()
