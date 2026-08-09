#!/usr/bin/env python3
"""Normalize donfeed NDJSON from stdin and POST it to the loopback RoNtoy host."""

from __future__ import annotations

import argparse
import http.client
import json
import pathlib
import sys
from typing import Optional

from rontoy_host import (
    DEFAULT_MAX_BODY_BYTES,
    DEFAULT_PORT,
    TOKEN_ENVIRONMENT_VARIABLE,
    AdmissionError,
    normalize_donfeed_observation,
    parse_json_object,
    read_token_file,
    resolve_ingest_token,
)


def post_snapshot(port: int, token: str, snapshot: dict) -> None:
    body = json.dumps(snapshot, separators=(",", ":"), sort_keys=True).encode("utf-8")
    if len(body) > DEFAULT_MAX_BODY_BYTES:
        raise RuntimeError("normalized snapshot exceeds host body limit")
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
    try:
        connection.request(
            "POST",
            "/v1/snapshot",
            body=body,
            headers={"Content-Type": "application/json", "X-RoNtoy-Token": token},
        )
        response = connection.getresponse()
        payload = response.read(DEFAULT_MAX_BODY_BYTES)
        if response.status != 202:
            message = payload.decode("utf-8", errors="replace")
            raise RuntimeError(f"host rejected snapshot: HTTP {response.status}: {message}")
    finally:
        connection.close()


def _discard_line_tail(stream: object) -> None:
    while True:
        chunk = stream.readline(DEFAULT_MAX_BODY_BYTES + 1)
        if not chunk or chunk.endswith(b"\n"):
            return


def run(port: int, token: str, max_errors: int) -> int:
    accepted = 0
    dropped = 0
    consecutive_errors = 0
    stream = sys.stdin.buffer
    while True:
        raw = stream.readline(DEFAULT_MAX_BODY_BYTES + 1)
        if not raw:
            break
        if len(raw) > DEFAULT_MAX_BODY_BYTES:
            if not raw.endswith(b"\n"):
                _discard_line_tail(stream)
            error: Exception = AdmissionError("body_too_large", "donfeed line exceeds 64 KiB", 413)
        else:
            raw = raw.strip()
            if not raw:
                continue
            try:
                observation = parse_json_object(raw)
                snapshot = normalize_donfeed_observation(observation)
                post_snapshot(port, token, snapshot)
                accepted += 1
                consecutive_errors = 0
                continue
            except (AdmissionError, OSError, RuntimeError, http.client.HTTPException) as caught:
                error = caught
        dropped += 1
        consecutive_errors += 1
        print(f"rontoy bridge: dropped observation: {error}", file=sys.stderr)
        if consecutive_errors >= max_errors:
            print(
                f"rontoy bridge: stopping after {consecutive_errors} consecutive errors "
                f"({accepted} accepted, {dropped} dropped)",
                file=sys.stderr,
            )
            return 1
    print(f"rontoy bridge: input ended ({accepted} accepted, {dropped} dropped)", file=sys.stderr)
    return 0 if accepted or not dropped else 1


def build_argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Bridge donfeed NDJSON into the loopback RoNtoy host")
    parser.add_argument("--port", type=int, default=DEFAULT_PORT, help=f"host loopback port (default {DEFAULT_PORT})")
    parser.add_argument("--token", help=f"host ingest token (or set {TOKEN_ENVIRONMENT_VARIABLE})")
    parser.add_argument(
        "--token-file",
        type=pathlib.Path,
        help="read the ingest token from this file (as written by the host's --token-file)",
    )
    parser.add_argument("--max-errors", type=int, default=5, help="stop after this many consecutive rejected lines")
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = build_argument_parser().parse_args(argv)
    if not 1 <= args.port <= 65535:
        raise SystemExit("--port must be in 1..65535")
    if args.max_errors <= 0:
        raise SystemExit("--max-errors must be positive")
    if args.token and args.token_file:
        raise SystemExit("give --token or --token-file, not both")
    token = read_token_file(args.token_file) if args.token_file else resolve_ingest_token(args.token)
    if not token:
        raise SystemExit(f"provide --token, --token-file, or {TOKEN_ENVIRONMENT_VARIABLE}")
    return run(args.port, token, args.max_errors)


if __name__ == "__main__":
    raise SystemExit(main())
