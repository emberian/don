#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Capture and verify source-archive-to-product-Wasm reproducibility.

The capture builds the exact committed ``git archive`` in a temporary directory, obtains
rustc's build-produced depfile, and requires the resulting Wasm bytes to equal the checked-in
candidate.  Retail/live inputs remain excluded.  A separate negative record keeps the known
test-only owned-input dependency visible without weakening the positive release-binary claim.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
from typing import Any


SCHEMA = "don.release-source-archive-reproducibility.v2"
DEFAULT_ROOT = Path(__file__).resolve().parents[2]
REQUIRED_LIVE_INPUT = "schema/live/live-tables-unit.tsv"
FORMER_CONSUMER = "crates/don-sim/src/systems/leaders.rs"
FORMER_LITERAL = "../../../../schema/live/live-tables-unit.tsv"
CANDIDATE = "web/public/wasm/don_web.wasm"
COMPONENT_PROVENANCE = "release/web-wasm-component-provenance.json"
MANIFEST = "web/wasm/Cargo.toml"
LOCK = "web/wasm/Cargo.lock"
BUILD_COMMAND = ["web/build.sh"]
TEST_COMMAND = ["cargo", "test", "--locked", "-p", "don-sim", "--lib"]
PIPELINE_INPUTS = [
    "schema/command-wire.json",
    "schema/replay-validation.json",
    "web/build.sh",
    "web/public/js/play/client.js",
    "web/public/js/play/readiness.gen.js",
    "web/public/js/play/wasm-contract.mjs",
    "web/public/js/play/wasmgame.js",
    "web/public/js/wire.gen.js",
    "web/tools/check-play-wasm.mjs",
    "web/tools/gen-readiness.mjs",
    "web/tools/gen-wire.mjs",
]
EXPECTED_CLAIMS = {
    "source_archive_product_wasm_buildable": True,
    "candidate_reproduced_from_archive": True,
    "product_binary_source_linkage": True,
    "live_table_compile_time_dependency_absent": True,
    "don_sim_lib_archive_tests_buildable": True,
    "whole_archive_workspace_tests_proven": False,
    "proprietary_input_redistributed": False,
}


class ReproducibilityError(RuntimeError):
    """The recorded archive/build relationship no longer matches repository truth."""


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


def _record_bytes(path: str, data: bytes) -> dict[str, object]:
    return {"path": path, "size": len(data), "sha256": _sha256(data)}


def _record(root: Path, path: str) -> dict[str, object]:
    return _record_bytes(path, _git_bytes(root, path))


def _working_record(root: Path, path: str) -> dict[str, object]:
    try:
        data = (root / path).read_bytes()
    except OSError as exc:
        raise ReproducibilityError(f"working candidate evidence is unavailable: {path}") from exc
    return _record_bytes(path, data)


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


def _former_live_include_absent(root: Path) -> bool:
    source = _git_bytes(root, FORMER_CONSUMER).decode("utf-8")
    return FORMER_LITERAL not in source


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


def _source_inputs_digest(records: list[dict[str, object]]) -> str:
    encoded = json.dumps(records, separators=(",", ":"), sort_keys=True).encode("utf-8")
    return _sha256(encoded)


def _depfile_inputs(tree: Path, depfile: Path) -> list[dict[str, object]]:
    tree = tree.resolve(strict=True)
    try:
        text = depfile.read_text(encoding="utf-8").replace("\\\n", " ")
    except (OSError, UnicodeError) as exc:
        raise ReproducibilityError("build-produced Wasm depfile is unreadable") from exc
    if ": " not in text:
        raise ReproducibilityError("build-produced Wasm depfile has no dependency list")
    dependencies = text.split(": ", 1)[1].split()
    by_path: dict[str, dict[str, object]] = {}
    for raw in dependencies:
        path = Path(raw)
        try:
            relative = path.resolve(strict=True).relative_to(tree).as_posix()
        except (OSError, RuntimeError, ValueError) as exc:
            raise ReproducibilityError(f"Wasm depfile names an input outside the archive: {raw}") from exc
        relative = _canonical(relative, "Wasm build input")
        data = path.read_bytes()
        by_path[relative] = _record_bytes(relative, data)
    if not by_path:
        raise ReproducibilityError("Wasm depfile resolved to an empty source set")
    for required in (FORMER_CONSUMER, "web/wasm/src/lib.rs"):
        if required not in by_path:
            raise ReproducibilityError(f"Wasm depfile omits required source input: {required}")
    if REQUIRED_LIVE_INPUT in by_path:
        raise ReproducibilityError("Wasm depfile still includes the retail/live unit table")
    return [by_path[path] for path in sorted(by_path)]


def _probe_archive_build(
    root: Path,
    expected_candidate: bytes,
) -> tuple[dict[str, object], list[dict[str, object]], dict[str, object]]:
    with tempfile.TemporaryDirectory(prefix="don-source-archive-probe-") as directory:
        scratch = Path(directory)
        tree = scratch / "tree"
        tree.mkdir()
        try:
            raw_archive = _run(["git", "archive", "--format=tar", "HEAD"], root).stdout
            with tarfile.open(fileobj=io.BytesIO(raw_archive), mode="r:") as archive:
                archive.extractall(tree, filter="data")
        except (OSError, tarfile.TarError, subprocess.CalledProcessError) as exc:
            raise ReproducibilityError("could not materialize the source archive") from exc
        if (tree / REQUIRED_LIVE_INPUT).exists():
            raise ReproducibilityError("retail/live unit table leaked into the source archive")
        canonical_root = Path("/tmp/don-web-canonical-source-v1")
        canonical_lock = Path("/tmp/don-web-canonical-source-v1.lock")
        try:
            canonical_lock.mkdir(mode=0o700)
        except FileExistsError as exc:
            raise ReproducibilityError(
                f"canonical Web build lock is active or stale: {canonical_lock}"
            ) from exc
        try:
            if canonical_root.exists():
                raise ReproducibilityError(
                    f"canonical Web source root is unexpectedly present: {canonical_root}"
                )
            canonical_root.mkdir(mode=0o700)
            copy = subprocess.run(
                [
                    "rsync",
                    "-a",
                    "--exclude=.git/",
                    "--exclude=target/",
                    "--exclude=web/public/wasm/don_web.wasm",
                    f"{tree}/",
                    f"{canonical_root}/",
                ],
                check=False,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            if copy.returncode != 0:
                raise ReproducibilityError(f"could not populate canonical source root: {copy.stderr}")
            command = [str(canonical_root / BUILD_COMMAND[0])]
            env = _git_env()
            env["DON_WEB_CANONICAL_INNER"] = "1"
            result = subprocess.run(
                command,
                cwd=canonical_root,
                env=env,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            if result.returncode != 0:
                tail = "\n".join(result.stderr.splitlines()[-8:])
                raise ReproducibilityError(f"source-archive Wasm build failed:\n{tail}")
            built = canonical_root / CANDIDATE
            raw = canonical_root / "web/wasm/target/wasm32-unknown-unknown/release/don_web.wasm"
            depfile = canonical_root / "web/wasm/target/wasm32-unknown-unknown/release/don_web.d"
            try:
                built_bytes = built.read_bytes()
                raw_bytes = raw.read_bytes()
            except OSError as exc:
                raise ReproducibilityError("archive build or candidate Wasm is absent") from exc
            if built_bytes != expected_candidate:
                raise ReproducibilityError(
                    "archive-built Wasm does not byte-match the checked-in candidate: "
                    f"built={len(built_bytes)}:{_sha256(built_bytes)}, "
                    f"candidate={len(expected_candidate)}:{_sha256(expected_candidate)}"
                )
            inputs = _depfile_inputs(canonical_root, depfile)
            output = _record_bytes(CANDIDATE, built_bytes)
            raw_output = _record_bytes(
                "web/wasm/target/wasm32-unknown-unknown/release/don_web.wasm", raw_bytes
            )
            test_env = _git_env()
            test_env.pop("CARGO_ENCODED_RUSTFLAGS", None)
            test_env.pop("RUSTFLAGS", None)
            test_env["CARGO_TARGET_DIR"] = str(canonical_root / "target-archive-test")
            test_result = subprocess.run(
                TEST_COMMAND,
                cwd=canonical_root,
                env=test_env,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            combined = test_result.stdout + "\n" + test_result.stderr
            matches = re.findall(
                r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;",
                combined,
            )
            if test_result.returncode != 0 or len(matches) != 1:
                tail = "\n".join(combined.splitlines()[-12:])
                raise ReproducibilityError(f"source-archive don-sim lib tests failed:\n{tail}")
            passed, failed, ignored = (int(value) for value in matches[0])
            if failed != 0 or passed == 0:
                raise ReproducibilityError(
                    "source-archive don-sim lib tests have no positive result"
                )
            test_probe = {
                "command": TEST_COMMAND,
                "outcome": "passed",
                "exit_code": test_result.returncode,
                "passed": passed,
                "failed": failed,
                "ignored": ignored,
            }
        finally:
            if canonical_root.exists():
                shutil.rmtree(canonical_root)
            canonical_lock.rmdir()
    return {"output": output, "raw_output": raw_output}, inputs, test_probe


def capture(root: Path, output: Path) -> dict[str, object]:
    root = root.resolve(strict=True)
    selected = _archive_files(root, [FORMER_CONSUMER, REQUIRED_LIVE_INPUT])
    if FORMER_CONSUMER not in selected:
        raise ReproducibilityError("required Rust consumer is absent from the archive")
    if REQUIRED_LIVE_INPUT in selected:
        raise ReproducibilityError("owned/live input leaked into the source archive")
    if not _export_ignore(root, REQUIRED_LIVE_INPUT):
        raise ReproducibilityError("owned/live input lost export-ignore protection")
    if not _former_live_include_absent(root):
        raise ReproducibilityError("leaders still compile-time-includes the live unit table")
    try:
        candidate_bytes = (root / CANDIDATE).read_bytes()
    except OSError as exc:
        raise ReproducibilityError("working candidate Wasm is unavailable") from exc
    built, source_inputs, test_probe = _probe_archive_build(root, candidate_bytes)
    artifact: dict[str, object] = {
        "schema": SCHEMA,
        "scope": {
            "candidate": _working_record(root, CANDIDATE),
            "component_provenance": _working_record(root, COMPONENT_PROVENANCE),
            "manifest": _record(root, MANIFEST),
            "lock": _record(root, LOCK),
            "build_command": BUILD_COMMAND,
            "pipeline_inputs": [_record(root, path) for path in PIPELINE_INPUTS],
        },
        "toolchain": {
            "cargo": _version("cargo"),
            "rustc": _version("rustc"),
            "node": _version("node"),
            "wasm_opt": _version("wasm-opt"),
            "target": "wasm32-unknown-unknown",
        },
        "archive_projection": {
            "live_input": {
                **_record(root, REQUIRED_LIVE_INPUT),
                "classification": "tracked-retail-live-derived-export-ignored",
                "export_ignore": True,
                "present": False,
            },
            "former_consumer": {
                **_record(root, FORMER_CONSUMER),
                "compile_time_literal_absent": True,
            },
        },
        "build": {
            "outcome": "reproduced-byte-identical-candidate",
            "exit_code": 0,
            "raw_output": built["raw_output"],
            "output": built["output"],
            "source_inputs": source_inputs,
            "source_inputs_sha256": _source_inputs_digest(source_inputs),
        },
        "test_projection": test_probe,
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
        {"schema", "scope", "toolchain", "archive_projection", "build", "test_projection", "claims"},
        "artifact",
    )
    if artifact["schema"] != SCHEMA:
        raise ReproducibilityError("archive reproducibility schema drift")
    scope = _exact(
        artifact["scope"],
        {"candidate", "component_provenance", "manifest", "lock", "build_command", "pipeline_inputs"},
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
    pipeline_inputs = scope["pipeline_inputs"]
    if not isinstance(pipeline_inputs, list) or len(pipeline_inputs) != len(PIPELINE_INPUTS):
        raise ReproducibilityError("archive build pipeline input coverage drift")
    for index, path in enumerate(PIPELINE_INPUTS):
        _verify_record(root, pipeline_inputs[index], path, f"scope.pipeline_inputs[{index}]")
    toolchain = _exact(
        artifact["toolchain"], {"cargo", "rustc", "node", "wasm_opt", "target"}, "toolchain"
    )
    if not all(isinstance(toolchain[field], str) and toolchain[field] for field in toolchain):
        raise ReproducibilityError("toolchain values are invalid")
    if toolchain["target"] != "wasm32-unknown-unknown":
        raise ReproducibilityError("archive build target drift")

    projection = _exact(
        artifact["archive_projection"], {"live_input", "former_consumer"}, "archive_projection"
    )
    live = _exact(
        projection["live_input"],
        {"path", "size", "sha256", "classification", "export_ignore", "present"},
        "archive_projection.live_input",
    )
    expected_live = {
        **_record(root, REQUIRED_LIVE_INPUT),
        "classification": "tracked-retail-live-derived-export-ignored",
        "export_ignore": True,
        "present": False,
    }
    if live != expected_live:
        raise ReproducibilityError("live-input identity or archive classification drift")
    former = _exact(
        projection["former_consumer"],
        {"path", "size", "sha256", "compile_time_literal_absent"},
        "archive_projection.former_consumer",
    )
    expected_former = {**_record(root, FORMER_CONSUMER), "compile_time_literal_absent": True}
    if former != expected_former or not _former_live_include_absent(root):
        raise ReproducibilityError("former live-table consumer boundary drift")
    selected = _archive_files(root, [FORMER_CONSUMER, REQUIRED_LIVE_INPUT])
    if REQUIRED_LIVE_INPUT in selected:
        raise ReproducibilityError("owned/live input leaked into the source archive")
    if not _export_ignore(root, REQUIRED_LIVE_INPUT):
        raise ReproducibilityError("live unit table lost export-ignore protection")

    build = _exact(
        artifact["build"],
        {"outcome", "exit_code", "raw_output", "output", "source_inputs", "source_inputs_sha256"},
        "build",
    )
    if build["outcome"] != "reproduced-byte-identical-candidate" or build["exit_code"] != 0:
        raise ReproducibilityError("archive build result does not prove reproduction")
    if build["output"] != scope["candidate"]:
        raise ReproducibilityError("archive build output does not match the checked-in candidate")
    raw_output = _exact(build["raw_output"], {"path", "size", "sha256"}, "build.raw_output")
    if raw_output["path"] != "web/wasm/target/wasm32-unknown-unknown/release/don_web.wasm":
        raise ReproducibilityError("raw rustc output path drift")
    if raw_output["sha256"] == build["output"]["sha256"]:
        raise ReproducibilityError("canonical optimizer stage is not distinguished from raw rustc output")
    inputs = build["source_inputs"]
    if not isinstance(inputs, list) or not inputs:
        raise ReproducibilityError("archive build source closure is empty")
    paths: set[str] = set()
    for index, record in enumerate(inputs):
        record = _exact(record, {"path", "size", "sha256"}, f"build.source_inputs[{index}]")
        path = _canonical(record["path"], f"build.source_inputs[{index}].path")
        if path in paths:
            raise ReproducibilityError(f"duplicate archive build input: {path}")
        paths.add(path)
        if record != _record(root, path):
            raise ReproducibilityError(f"archive build source-input drift: {path}")
    if REQUIRED_LIVE_INPUT in paths or FORMER_CONSUMER not in paths or "web/wasm/src/lib.rs" not in paths:
        raise ReproducibilityError("archive build source closure violates required boundaries")
    if build["source_inputs_sha256"] != _source_inputs_digest(inputs):
        raise ReproducibilityError("archive build source-closure digest drift")

    test = _exact(
        artifact["test_projection"],
        {"command", "outcome", "exit_code", "passed", "failed", "ignored"},
        "test_projection",
    )
    if (
        test["command"] != TEST_COMMAND
        or test["outcome"] != "passed"
        or test["exit_code"] != 0
        or not isinstance(test["passed"], int)
        or test["passed"] <= 0
        or test["failed"] != 0
        or not isinstance(test["ignored"], int)
        or test["ignored"] < 0
    ):
        raise ReproducibilityError("archive don-sim lib test result overstates evidence")
    if artifact["claims"] != EXPECTED_CLAIMS:
        raise ReproducibilityError("archive reproducibility claims contradict current evidence")
    return {
        "schema": "don.release-source-archive-reproducibility-check.v2",
        "ok": True,
        "outcome": build["outcome"],
        "candidate_reproduced": True,
        "source_inputs": len(inputs),
        "live_input_present": False,
        "don_sim_lib_tests": {
            "passed": test["passed"],
            "ignored": test["ignored"],
        },
        "whole_archive_workspace_tests_proven": False,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    subparsers = parser.add_subparsers(dest="command", required=True)
    probe_parser = subparsers.add_parser("probe", help="run the isolated archive build probe")
    probe_parser.add_argument("--output", type=Path, required=True)
    verify_parser = subparsers.add_parser("verify", help="verify the recorded build linkage offline")
    verify_parser.add_argument("--artifact", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == "probe":
            result = capture(args.root, args.output)
            report = {
                "schema": "don.release-source-archive-reproducibility-probe.v2",
                "ok": True,
                "outcome": result["build"]["outcome"],
                "artifact": str(args.output),
                "source_inputs": len(result["build"]["source_inputs"]),
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
