#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Capture and verify a fail-closed source-archive build blocker.

This proof is intentionally negative.  It records that the checked-in browser Wasm cannot
currently be reproduced from the public ``git archive`` projection because a Rust
``include_str!`` compile-time input is marked ``export-ignore``.  It never copies that input
into the archive or treats a hash of the input as redistribution authority.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import tarfile
import tempfile
from typing import Any


SCHEMA = "don.release-source-archive-reproducibility.v1"
DEFAULT_ROOT = Path(__file__).resolve().parents[2]
CONSUMER = "crates/don-sim/src/systems/leaders.rs"
REQUIRED_INPUT = "schema/live/live-tables-unit.tsv"
CANDIDATE = "web/public/wasm/don_web.wasm"
COMPONENT_PROVENANCE = "release/web-wasm-component-provenance.json"
MANIFEST = "web/wasm/Cargo.toml"
LOCK = "web/wasm/Cargo.lock"
INCLUDE_LITERAL = "../../../../schema/live/live-tables-unit.tsv"
BUILD_COMMAND = [
    "cargo",
    "build",
    "--locked",
    "--release",
    "--target",
    "wasm32-unknown-unknown",
    "--manifest-path",
    MANIFEST,
]
EXPECTED_CLAIMS = {
    "source_archive_buildable": False,
    "candidate_reproduced_from_archive": False,
    "product_binary_source_linkage": False,
    "proprietary_input_redistributed": False,
}


class ReproducibilityError(RuntimeError):
    """The recorded archive blocker no longer matches repository truth."""


def _sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _git_env() -> dict[str, str]:
    env = os.environ.copy()
    env["GIT_CONFIG_GLOBAL"] = "/dev/null"
    env["GIT_CONFIG_SYSTEM"] = "/dev/null"
    return env


def _run(
    command: list[str], root: Path, *, check: bool = True, text: bool = False
) -> subprocess.CompletedProcess[Any]:
    return subprocess.run(
        command,
        cwd=root,
        env=_git_env(),
        check=check,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=text,
    )


def _git_bytes(root: Path, path: str) -> bytes:
    try:
        return _run(["git", "show", f"HEAD:{path}"], root).stdout
    except subprocess.CalledProcessError as exc:
        raise ReproducibilityError(f"tracked source is unavailable at HEAD: {path}") from exc


def _record(root: Path, path: str) -> dict[str, object]:
    data = _git_bytes(root, path)
    return {"path": path, "size": len(data), "sha256": _sha256(data)}


def _canonical(path: object, context: str) -> str:
    if not isinstance(path, str) or not path or "\\" in path or "\0" in path:
        raise ReproducibilityError(f"{context} is not a canonical relative path")
    candidate = PurePosixPath(path)
    if candidate.is_absolute() or any(part in {"", ".", ".."} for part in candidate.parts):
        raise ReproducibilityError(f"{context} escapes the repository")
    if candidate.as_posix() != path:
        raise ReproducibilityError(f"{context} is not canonical")
    return path


def _archive_files(root: Path, paths: list[str]) -> set[str]:
    command = ["git", "archive", "--format=tar", "HEAD", "--", *paths]
    try:
        raw = _run(command, root).stdout
    except subprocess.CalledProcessError as exc:
        raise ReproducibilityError("could not project selected paths through git archive") from exc
    files: set[str] = set()
    try:
        with tarfile.open(fileobj=io.BytesIO(raw), mode="r:") as archive:
            for member in archive.getmembers():
                if member.isfile():
                    files.add(member.name)
    except tarfile.TarError as exc:
        raise ReproducibilityError("git archive emitted an unreadable tar stream") from exc
    return files


def _export_ignore(root: Path, path: str) -> bool:
    result = _run(
        ["git", "check-attr", "export-ignore", "--", path], root, text=True
    ).stdout.strip()
    return result.endswith(": export-ignore: set")


def _include_site(root: Path) -> dict[str, object]:
    data = _git_bytes(root, CONSUMER)
    try:
        source = data.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise ReproducibilityError(f"consumer is not UTF-8: {CONSUMER}") from exc
    pattern = re.compile(r'include_str!\(\s*"' + re.escape(INCLUDE_LITERAL) + r'"\s*\)')
    match = pattern.search(source)
    if match is None:
        raise ReproducibilityError("the recorded include_str blocker is absent")
    line = source.count("\n", 0, match.start()) + 1
    resolved = (PurePosixPath(CONSUMER).parent / INCLUDE_LITERAL)
    collapsed: list[str] = []
    for part in resolved.parts:
        if part == "..":
            if not collapsed:
                raise ReproducibilityError("include_str path escapes the repository")
            collapsed.pop()
        elif part != ".":
            collapsed.append(part)
    if "/".join(collapsed) != REQUIRED_INPUT:
        raise ReproducibilityError("include_str literal resolves to an unexpected input")
    return {"line": line, "macro": "include_str", "literal": INCLUDE_LITERAL}


def _version(command: str) -> str:
    try:
        output = subprocess.run(
            [command, "--version"],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError) as exc:
        raise ReproducibilityError(f"could not identify {command}") from exc
    if not output:
        raise ReproducibilityError(f"{command} returned an empty version")
    return output


def _probe_archive_build(root: Path) -> tuple[int, str]:
    with tempfile.TemporaryDirectory(prefix="don-source-archive-probe-") as directory:
        scratch = Path(directory)
        archive_path = scratch / "source.tar"
        tree = scratch / "tree"
        target = scratch / "target"
        tree.mkdir()
        target.mkdir()
        try:
            archive_path.write_bytes(
                _run(["git", "archive", "--format=tar", "HEAD"], root).stdout
            )
            with tarfile.open(archive_path, "r:") as archive:
                archive.extractall(tree, filter="data")
        except (OSError, tarfile.TarError, subprocess.CalledProcessError) as exc:
            raise ReproducibilityError("could not materialize the source archive") from exc
        command = BUILD_COMMAND.copy()
        command[-1] = str(tree / MANIFEST)
        env = _git_env()
        env["CARGO_TARGET_DIR"] = str(target)
        result = subprocess.run(
            command,
            cwd=tree,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
    diagnostic = result.stderr
    if result.returncode != 101 or REQUIRED_INPUT not in diagnostic:
        raise ReproducibilityError(
            "archive build did not fail at the expected exported include_str input"
        )
    return result.returncode, (
        f"rustc could not read {REQUIRED_INPUT}, required by "
        f"{CONSUMER}:{_include_site(root)['line']}"
    )


def capture(root: Path, output: Path) -> dict[str, object]:
    root = root.resolve(strict=True)
    site = _include_site(root)
    projection = _archive_files(root, [CONSUMER, REQUIRED_INPUT])
    if CONSUMER not in projection:
        raise ReproducibilityError("consumer is unexpectedly absent from the source archive")
    if REQUIRED_INPUT in projection:
        raise ReproducibilityError("required live input is unexpectedly present in the archive")
    if not _export_ignore(root, REQUIRED_INPUT):
        raise ReproducibilityError("required live input is not protected by export-ignore")
    exit_code, diagnostic = _probe_archive_build(root)
    artifact: dict[str, object] = {
        "schema": SCHEMA,
        "scope": {
            "candidate": _record(root, CANDIDATE),
            "component_provenance": _record(root, COMPONENT_PROVENANCE),
            "manifest": _record(root, MANIFEST),
            "lock": _record(root, LOCK),
            "build_command": BUILD_COMMAND,
        },
        "toolchain": {
            "cargo": _version("cargo"),
            "rustc": _version("rustc"),
            "target": "wasm32-unknown-unknown",
        },
        "archive_projection": {
            "consumer": {**_record(root, CONSUMER), **site},
            "required_input": {
                **_record(root, REQUIRED_INPUT),
                "classification": "tracked-retail-live-derived-export-ignored",
                "export_ignore": True,
            },
            "consumer_present": True,
            "required_input_present": False,
        },
        "probe": {
            "outcome": "blocked-missing-exported-build-input",
            "exit_code": exit_code,
            "diagnostic": diagnostic,
        },
        "claims": EXPECTED_CLAIMS,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(artifact, indent=2) + "\n", encoding="utf-8")
    return artifact


def _exact(value: object, fields: set[str], context: str) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != fields:
        raise ReproducibilityError(f"{context} fields are invalid")
    return value


def _verify_record(root: Path, value: object, expected_path: str, context: str) -> None:
    record = _exact(value, {"path", "size", "sha256"}, context)
    if _canonical(record["path"], f"{context}.path") != expected_path:
        raise ReproducibilityError(f"{context} names an unexpected path")
    if record != _record(root, expected_path):
        raise ReproducibilityError(f"{context} byte identity drift")


def verify(root: Path, artifact_path: Path) -> dict[str, object]:
    root = root.resolve(strict=True)
    try:
        artifact = json.loads(artifact_path.read_bytes())
    except (OSError, json.JSONDecodeError) as exc:
        raise ReproducibilityError("archive reproducibility artifact is unreadable") from exc
    artifact = _exact(
        artifact,
        {"schema", "scope", "toolchain", "archive_projection", "probe", "claims"},
        "artifact",
    )
    if artifact["schema"] != SCHEMA:
        raise ReproducibilityError("archive reproducibility schema drift")
    scope = _exact(
        artifact["scope"],
        {"candidate", "component_provenance", "manifest", "lock", "build_command"},
        "scope",
    )
    for field, path in (
        ("candidate", CANDIDATE),
        ("component_provenance", COMPONENT_PROVENANCE),
        ("manifest", MANIFEST),
        ("lock", LOCK),
    ):
        _verify_record(root, scope[field], path, f"scope.{field}")
    if scope["build_command"] != BUILD_COMMAND:
        raise ReproducibilityError("archive build command drift")

    toolchain = _exact(artifact["toolchain"], {"cargo", "rustc", "target"}, "toolchain")
    if not all(isinstance(toolchain[field], str) and toolchain[field] for field in toolchain):
        raise ReproducibilityError("toolchain values are invalid")
    if toolchain["target"] != "wasm32-unknown-unknown":
        raise ReproducibilityError("archive build target drift")

    projection = _exact(
        artifact["archive_projection"],
        {"consumer", "required_input", "consumer_present", "required_input_present"},
        "archive_projection",
    )
    consumer = _exact(
        projection["consumer"],
        {"path", "size", "sha256", "line", "macro", "literal"},
        "archive_projection.consumer",
    )
    expected_consumer = {**_record(root, CONSUMER), **_include_site(root)}
    if consumer != expected_consumer:
        raise ReproducibilityError("archive consumer identity or include site drift")
    required = _exact(
        projection["required_input"],
        {"path", "size", "sha256", "classification", "export_ignore"},
        "archive_projection.required_input",
    )
    expected_required = {
        **_record(root, REQUIRED_INPUT),
        "classification": "tracked-retail-live-derived-export-ignored",
        "export_ignore": True,
    }
    if required != expected_required:
        raise ReproducibilityError("required live-input identity or classification drift")
    selected = _archive_files(root, [CONSUMER, REQUIRED_INPUT])
    actual_consumer = CONSUMER in selected
    actual_input = REQUIRED_INPUT in selected
    if projection["consumer_present"] is not True or actual_consumer is not True:
        raise ReproducibilityError("consumer is not present in the archive projection")
    if projection["required_input_present"] is not False or actual_input is not False:
        raise ReproducibilityError("required live input is no longer absent from the archive")
    if not _export_ignore(root, REQUIRED_INPUT):
        raise ReproducibilityError("required live input lost export-ignore protection")

    probe = _exact(artifact["probe"], {"outcome", "exit_code", "diagnostic"}, "probe")
    expected_probe = {
        "outcome": "blocked-missing-exported-build-input",
        "exit_code": 101,
        "diagnostic": (
            f"rustc could not read {REQUIRED_INPUT}, required by "
            f"{CONSUMER}:{consumer['line']}"
        ),
    }
    if probe != expected_probe:
        raise ReproducibilityError("archive build probe record drift")
    if artifact["claims"] != EXPECTED_CLAIMS:
        raise ReproducibilityError("archive reproducibility claims overstate current evidence")
    return {
        "schema": "don.release-source-archive-reproducibility-check.v1",
        "ok": True,
        "outcome": probe["outcome"],
        "consumer": CONSUMER,
        "missing_input": REQUIRED_INPUT,
        "candidate_reproduced": False,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    subparsers = parser.add_subparsers(dest="command", required=True)
    probe_parser = subparsers.add_parser("probe", help="run the isolated archive build probe")
    probe_parser.add_argument("--output", type=Path, required=True)
    verify_parser = subparsers.add_parser("verify", help="verify the recorded blocker offline")
    verify_parser.add_argument("--artifact", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == "probe":
            result = capture(args.root, args.output)
            report = {
                "schema": "don.release-source-archive-reproducibility-probe.v1",
                "ok": True,
                "outcome": result["probe"]["outcome"],
                "artifact": str(args.output),
            }
        else:
            report = verify(args.root, args.artifact)
    except ReproducibilityError as exc:
        print(f"archive-reproducibility: ERROR: {exc}", file=sys.stderr)
        return 2
    print(json.dumps(report, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
