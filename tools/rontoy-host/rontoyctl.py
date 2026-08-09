#!/usr/bin/env python3
"""rontoyctl — bring the whole read-only RoNtoy pipeline up with one command.

    python3 tools/rontoy-host/rontoyctl.py doctor
    python3 tools/rontoy-host/rontoyctl.py up --open
    python3 tools/rontoy-host/rontoyctl.py replay recording.ndjson

``up`` owns the pieces that used to be three terminals and a copied token: it starts
the loopback host in-process, publishes its ephemeral ingest token to an owner-only
file, launches ``donfeed`` inside the Parallels guest, and pipes the guest's NDJSON
through the same ``normalize_donfeed_observation`` + ``POST /v1/snapshot`` path the
documented bridge uses.  The HTTP hop is kept deliberately: it is the admission
boundary under test, so the supervised run and a hand-run bridge exercise identical
validation, rate limiting, and rejection accounting.

Nothing here writes to the game.  The guest side runs ``donfeed`` and, on shutdown,
closes its pipe before removing only an orphaned probe PID; neither operation touches
``riseofnations.exe``.
"""

from __future__ import annotations

import argparse
import http.client
import json
import pathlib
import signal
import subprocess
import sys
import threading
import time
import webbrowser
from typing import Any, Optional

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from rontoy_host import (  # noqa: E402  (path shim must precede the import)
    DEFAULT_MAX_BODY_BYTES,
    DEFAULT_MAX_CONNECTIONS,
    DEFAULT_MIN_INTERVAL_MS,
    DEFAULT_PORT,
    DEFAULT_TOKEN_FILE,
    SCHEMA_VERSION,
    AdmissionError,
    RoNtoyServer,
    SnapshotStore,
    normalize_donfeed_observation,
    parse_json_object,
    resolve_ingest_token,
    write_token_file,
)

DEFAULT_VM = "Windows 11"
DEFAULT_DONFEED = r"C:\Users\Public\donfeed.exe"
DEFAULT_GAME_PROCESS = "riseofnations.exe"
# donscan::live::SOURCE_SHA256 — the one retail build whose offsets we hold.
SUPPORTED_MODULE_SHA256 = "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079"


class SourceRestartRequired(RuntimeError):
    """The retained host slot and producer no longer name the same process."""


# ---------------------------------------------------------------------------
# guest access (read-only with respect to the game)


def prlctl_exec(vm: str, command: str, timeout: float = 30.0) -> subprocess.CompletedProcess:
    """Run one ``cmd.exe`` line in the guest and capture its output."""

    return subprocess.run(
        ["prlctl", "exec", vm, "cmd.exe", "/d", "/s", "/c", command],
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )


def vm_is_running(vm: str) -> tuple[bool, str]:
    try:
        listing = subprocess.run(["prlctl", "list", "-a"], capture_output=True, text=True, timeout=30, check=False)
    except (OSError, subprocess.SubprocessError) as error:
        return False, f"prlctl unavailable: {error}"
    for line in listing.stdout.splitlines()[1:]:
        if line.rstrip().endswith(vm):
            return "running" in line, line.strip()
    return False, f"no VM named {vm!r} in `prlctl list -a`"


def guest_file_exists(vm: str, path: str) -> bool:
    result = prlctl_exec(vm, f'if exist "{path}" (echo YES) else (echo NO)')
    return "YES" in result.stdout


def guest_process_pids(vm: str, image: str) -> list[int]:
    result = prlctl_exec(vm, f'tasklist /FI "IMAGENAME eq {image}" /NH /FO CSV')
    pids: list[int] = []
    for line in result.stdout.splitlines():
        fields = [field.strip('" ') for field in line.split('","')]
        if len(fields) >= 2 and fields[0].lower() == image.lower():
            try:
                pids.append(int(fields[1]))
            except ValueError:
                continue
    return pids


# ---------------------------------------------------------------------------
# doctor


def doctor(args: argparse.Namespace) -> int:
    findings: list[tuple[bool, str]] = []

    running, detail = vm_is_running(args.vm)
    findings.append((running, f"VM {args.vm!r}: {detail}"))

    if running:
        present = guest_file_exists(args.vm, args.donfeed)
        findings.append((present, f"guest probe {args.donfeed}: {'present' if present else 'MISSING'}"))
        game_pids = guest_process_pids(args.vm, args.game_process)
        findings.append(
            (
                bool(game_pids),
                f"guest {args.game_process}: {'pid ' + ', '.join(map(str, game_pids)) if game_pids else 'not running'}",
            )
        )
        stale = guest_process_pids(args.vm, "donfeed.exe")
        findings.append((not stale, f"stale donfeed.exe in guest: {stale or 'none'}"))
    else:
        findings.append((False, "guest checks skipped: VM is not running"))

    free = port_is_free(args.port)
    findings.append((free, f"host loopback port {args.port}: {'free' if free else 'IN USE'}"))

    dashboard = pathlib.Path(__file__).resolve().parents[2] / "web" / "public" / "rontoy.html"
    findings.append((dashboard.is_file(), f"dashboard asset {dashboard}: {'present' if dashboard.is_file() else 'MISSING'}"))

    for ok, line in findings:
        print(f"[{'ok ' if ok else 'FAIL'}] {line}")
    return 0 if all(ok for ok, _ in findings) else 1


def port_is_free(port: int) -> bool:
    import socket

    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            probe.bind(("127.0.0.1", port))
        except OSError:
            return False
    return True


# ---------------------------------------------------------------------------
# supervised pipeline


class Pipeline:
    """Host + collector lifecycle for one supervised run."""

    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.store = SnapshotStore(min_interval_ms=args.min_interval_ms)
        self.server = RoNtoyServer(
            ("127.0.0.1", args.port),
            self.store,
            max_body_bytes=DEFAULT_MAX_BODY_BYTES,
            max_connections=DEFAULT_MAX_CONNECTIONS,
            ingest_token=resolve_ingest_token(args.token),
            access_log=bool(args.verbose),
        )
        self.token = self.server.ingest_token
        self.token_file: Optional[pathlib.Path] = None if args.no_token_file else args.token_file
        self.stop = threading.Event()
        self.posted = 0
        self.post_rejected = 0
        self.lines = 0
        self.last_error = ""
        self.source_state = "waiting_for_probe"
        self.feed: Optional[subprocess.Popen] = None
        self._connection: Optional[http.client.HTTPConnection] = None
        # Opt-in only: recording is off unless --record names a destination.
        self.recorder = args.record.open("ab") if getattr(args, "record", None) else None

    # -- host ---------------------------------------------------------------

    def start_host(self) -> None:
        if self.token_file is not None:
            write_token_file(self.token_file, self.token)
        threading.Thread(target=self.server.serve_forever, kwargs={"poll_interval": 0.25}, daemon=True).start()
        print(f"rontoyctl: host on http://127.0.0.1:{self.server.server_port}/ (schema v{SCHEMA_VERSION})")
        print(f"rontoyctl: ingest token {self.token}")
        if self.token_file is not None:
            print(f"rontoyctl: token file {self.token_file} (mode 0600, removed on exit)")

    def stop_host(self) -> None:
        self.server.shutdown()
        self.server.server_close()
        if self.token_file is not None:
            try:
                self.token_file.unlink()
            except FileNotFoundError:
                pass

    # -- admission ----------------------------------------------------------

    def post(self, snapshot: dict[str, Any]) -> None:
        body = json.dumps(snapshot, separators=(",", ":"), sort_keys=True).encode("utf-8")
        if self._connection is None:
            self._connection = http.client.HTTPConnection("127.0.0.1", self.server.server_port, timeout=5)
        try:
            self._connection.request(
                "POST",
                "/v1/snapshot",
                body=body,
                headers={"Content-Type": "application/json", "X-RoNtoy-Token": self.token},
            )
            response = self._connection.getresponse()
            payload = response.read(DEFAULT_MAX_BODY_BYTES)
        except (OSError, http.client.HTTPException):
            # The host closes each request connection; reopen and let the caller
            # see the failure only if the retry also fails.
            self._connection.close()
            self._connection = None
            raise
        # The host answers every request with `Connection: close`.
        self._connection.close()
        self._connection = None
        if response.status != 202:
            message = payload.decode("utf-8", errors="replace")
            try:
                error_code = json.loads(message)["error"]["code"]
            except (json.JSONDecodeError, KeyError, TypeError):
                error_code = ""
            if response.status == 409 and error_code in {"source_conflict", "identity_changed"}:
                raise SourceRestartRequired(
                    f"{error_code}: game process identity changed; restart rontoyctl to bind a fresh host slot"
                )
            raise RuntimeError(f"host rejected snapshot: HTTP {response.status}: {message}")

    def ingest_line(self, raw: bytes) -> None:
        self.lines += 1
        if self.recorder is not None:
            self.recorder.write(raw + b"\n")
            self.recorder.flush()
        try:
            snapshot = normalize_donfeed_observation(parse_json_object(raw))
            self.post(snapshot)
            self.posted += 1
            if self.source_state != "replaying":
                self.source_state = "streaming"
        except SourceRestartRequired as error:
            self.post_rejected += 1
            self.last_error = str(error)
            self.source_state = "restart_required"
            self.stop.set()
        except (AdmissionError, OSError, RuntimeError, http.client.HTTPException, ValueError) as error:
            self.post_rejected += 1
            self.last_error = str(error)
            if self.args.verbose:
                print(f"rontoyctl: dropped observation: {error}", file=sys.stderr)

    def pump(self, stream: Any) -> None:
        for raw in iter(stream.readline, b""):
            if self.stop.is_set():
                break
            raw = raw.strip()
            if raw:
                self.ingest_line(raw)
        if self.source_state not in {"restart_required", "operator_stopped", "duration_complete"}:
            self.source_state = "probe_exited"
        self.stop.set()

    # -- guest probe --------------------------------------------------------

    def start_feed(self) -> None:
        stale = guest_process_pids(self.args.vm, "donfeed.exe")
        if stale:
            raise RuntimeError(f"refusing to start beside an existing donfeed.exe: {stale}")
        command = f'"{self.args.donfeed}" --hz {self.args.hz}'
        if self.args.count:
            command += f" --count {self.args.count}"
        argv = ["prlctl", "exec", self.args.vm, "cmd.exe", "/d", "/s", "/c", command]
        print(f"rontoyctl: guest probe: {command}")
        self.feed = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.source_state = "probe_running"
        threading.Thread(target=self.pump, args=(self.feed.stdout,), daemon=True).start()
        threading.Thread(target=self._drain_feed_stderr, daemon=True).start()

    def _drain_feed_stderr(self) -> None:
        assert self.feed is not None and self.feed.stderr is not None
        for line in iter(self.feed.stderr.readline, b""):
            text = line.decode("utf-8", errors="replace").rstrip()
            if text:
                print(f"rontoyctl: donfeed: {text}", file=sys.stderr)

    def stop_feed(self) -> None:
        if self.feed is None:
            return
        if self.feed is not None and self.feed.poll() is None:
            self.feed.terminate()
            try:
                self.feed.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.feed.kill()
        if not self.args.no_guest_kill:
            # Closing the pipe normally ends donfeed on its next write. We refused
            # to start beside a pre-existing probe, so any remaining PID is the
            # orphan from this supervised run. Name the PID, never the image, so a
            # later unrelated probe cannot be swept up accidentally.
            for pid in guest_process_pids(self.args.vm, "donfeed.exe"):
                prlctl_exec(self.args.vm, f"taskkill /PID {pid} /F >NUL 2>&1 & exit /b 0", timeout=20)

    # -- reporting ----------------------------------------------------------

    def summary(self) -> dict[str, Any]:
        status = self.store.status()
        latest = self.store.latest()
        advice = list(latest.analysis["advice"]) if latest is not None else []
        return {
            "lines_read": self.lines,
            "snapshots_admitted": self.posted,
            "snapshots_dropped": self.post_rejected,
            "host_accepted": status["accepted"],
            "host_rejected": status["rejected"],
            "rejections_by_code": status["rejections_by_code"],
            "latest": status["latest"],
            "advice_allowed": latest.analysis["advice_allowed"] if latest is not None else None,
            "suppressed_reasons": latest.analysis["suppressed_reasons"] if latest is not None else [],
            "advice_codes": [item["code"] for item in advice],
            "last_error": self.last_error,
            "source_state": self.source_state,
        }

    def print_status_line(self) -> None:
        status = self.store.status()
        latest = status["latest"]
        if latest is None:
            tail = "waiting for first snapshot"
        else:
            advice = self.store.latest().analysis
            tail = (
                f"seq {latest['sequence']} frame {latest['frame']} age {latest['age_ms']}ms "
                f"advice {len(advice['advice'])}"
                + ("" if advice["advice_allowed"] else f" (suppressed: {','.join(advice['suppressed_reasons'])})")
            )
        print(
            f"\rrontoyctl: admitted {status['accepted']} dropped {self.post_rejected + status['rejected']} | {tail}   ",
            end="",
            flush=True,
        )


def run_up(args: argparse.Namespace) -> int:
    pipeline = Pipeline(args)
    pipeline.start_host()

    def request_stop(_signum: int, _frame: Any) -> None:
        pipeline.source_state = "operator_stopped"
        pipeline.stop.set()

    signal.signal(signal.SIGINT, request_stop)
    signal.signal(signal.SIGTERM, request_stop)

    try:
        pipeline.start_feed()
        if args.open:
            webbrowser.open(f"http://127.0.0.1:{pipeline.server.server_port}/")

        deadline = None if args.seconds is None else time.monotonic() + args.seconds
        while not pipeline.stop.wait(1.0):
            if not args.quiet:
                pipeline.print_status_line()
            if deadline is not None and time.monotonic() >= deadline:
                pipeline.source_state = "duration_complete"
                break
            if pipeline.feed is not None and pipeline.feed.poll() is not None and pipeline.lines:
                break
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        pipeline.last_error = str(error)
        pipeline.source_state = "probe_start_failed"
    finally:
        pipeline.stop.set()
        if not args.quiet:
            print()
        pipeline.stop_feed()
        summary = pipeline.summary()
        pipeline.stop_host()
        if pipeline.recorder is not None:
            pipeline.recorder.close()

    print(json.dumps(summary, indent=2, sort_keys=True))
    if args.summary_json:
        args.summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if summary["source_state"] == "restart_required":
        return 2
    return 0 if summary["snapshots_admitted"] > 0 else 1


def run_replay(args: argparse.Namespace) -> int:
    pipeline = Pipeline(args)
    pipeline.start_host()
    if args.open:
        webbrowser.open(f"http://127.0.0.1:{pipeline.server.server_port}/")

    def request_stop(_signum: int, _frame: Any) -> None:
        pipeline.source_state = "operator_stopped"
        pipeline.stop.set()

    signal.signal(signal.SIGINT, request_stop)
    interval = 1.0 / args.hz
    try:
        with args.recording.open("rb") as handle:
            pipeline.source_state = "replaying"
            for raw in handle:
                if pipeline.stop.is_set():
                    break
                raw = raw.strip()
                if not raw:
                    continue
                pipeline.ingest_line(raw)
                if not args.quiet:
                    pipeline.print_status_line()
                time.sleep(interval)
        if pipeline.source_state == "replaying":
            pipeline.source_state = "replay_complete"
        if args.hold and not pipeline.stop.is_set():
            print("\nrontoyctl: recording exhausted; holding the host up (Ctrl-C to stop)")
            pipeline.stop.wait()
    finally:
        if not args.quiet:
            print()
        summary = pipeline.summary()
        pipeline.stop_host()
    print(json.dumps(summary, indent=2, sort_keys=True))
    if args.summary_json:
        args.summary_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if summary["source_state"] == "restart_required":
        return 2
    return 0 if summary["snapshots_admitted"] > 0 else 1


def build_argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Supervise the read-only RoNtoy live pipeline")
    subparsers = parser.add_subparsers(dest="command", required=True)

    def add_common(sub: argparse.ArgumentParser) -> None:
        sub.add_argument("--port", type=int, default=DEFAULT_PORT, help=f"host loopback port (default {DEFAULT_PORT})")
        sub.add_argument("--vm", default=DEFAULT_VM, help=f"Parallels VM name (default {DEFAULT_VM!r})")
        sub.add_argument("--donfeed", default=DEFAULT_DONFEED, help="guest path to donfeed.exe")
        sub.add_argument("--game-process", default=DEFAULT_GAME_PROCESS, help="guest game image name")

    def add_run_options(sub: argparse.ArgumentParser) -> None:
        sub.add_argument("--token", help="ingest token to require (default: RONTOY_TOKEN, else random)")
        sub.add_argument("--token-file", type=pathlib.Path, default=DEFAULT_TOKEN_FILE, help="where to publish it")
        sub.add_argument("--no-token-file", action="store_true", help="do not write a token file")
        sub.add_argument("--min-interval-ms", type=int, default=DEFAULT_MIN_INTERVAL_MS)
        sub.add_argument("--open", action="store_true", help="open the dashboard in the default browser")
        sub.add_argument("--quiet", action="store_true", help="suppress the live status line")
        sub.add_argument("--verbose", action="store_true", help="print every dropped observation")
        sub.add_argument("--summary-json", type=pathlib.Path, help="write the run summary here")
        sub.add_argument("--record", type=pathlib.Path, help="append the raw observation NDJSON here (opt-in)")

    check = subparsers.add_parser("doctor", help="report pipeline readiness without attaching")
    add_common(check)
    check.set_defaults(handler=doctor)

    up = subparsers.add_parser("up", help="host + guest probe + collector, one command")
    add_common(up)
    add_run_options(up)
    up.add_argument("--hz", type=int, default=1, help="probe cadence, 1..15 (default 1)")
    up.add_argument("--count", type=int, help="stop the probe after n observations")
    up.add_argument("--seconds", type=float, help="stop the whole run after n seconds")
    up.add_argument("--no-guest-kill", action="store_true", help="do not taskkill donfeed.exe on exit")
    up.set_defaults(handler=run_up)

    replay = subparsers.add_parser("replay", help="feed a recorded donfeed NDJSON into the host")
    add_common(replay)
    add_run_options(replay)
    replay.add_argument("recording", type=pathlib.Path, help="donfeed NDJSON file")
    replay.add_argument("--hz", type=float, default=2.0, help="replay cadence (default 2)")
    replay.add_argument("--hold", action="store_true", help="keep the host up after the recording ends")
    replay.set_defaults(no_guest_kill=True, count=None, seconds=None)
    replay.set_defaults(handler=run_replay)

    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = build_argument_parser().parse_args(argv)
    hz = getattr(args, "hz", None)
    if hz is not None and not 0 < float(hz) <= 15:
        raise SystemExit("--hz must be in (0, 15]")
    return int(args.handler(args))


if __name__ == "__main__":
    raise SystemExit(main())
