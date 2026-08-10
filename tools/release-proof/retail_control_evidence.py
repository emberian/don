#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Verify compact controller lifecycle and STOP-incident closure evidence.

This verifier is intentionally offline. It binds compact JSON to the exact public prose and
incident record already present in the repository; it does not access a retail process, infer a
crash cause, or claim that an absent dump proves that no failure occurred outside the measured run.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import sys
from typing import Any


HERE = Path(__file__).resolve().parent
DEFAULT_ROOT = HERE.parents[1]
LIFECYCLE_PATH = "schema/live/retail-control-active-stop-cycles-v1.json"
CLOSURE_PATH = "schema/live/retail-control-stop-incident-closure-v1.json"
SOURCE_PATH = "docs/tooling/live-control.md"
INCIDENT_PATH = "schema/live/retail-control-stop-incident-v1.json"

SOURCE_SHA256 = "5487b2f72c9c50175c3cae6ecbe3d5ec7c77dd7906aaf1e0621c1127e2df488f"
INCIDENT_SHA256 = "d20ae37a9a2321df58fbf45a17a196e935dd76d07974ac3efcf86111254ca577"
RETAIL_EXE_SHA256 = "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079"
CONTROLLER_DLL_SHA256 = "e2829ae2ae93e24e95b87d2d9e469fc79e915b67e30f53d9638b84fbe6c92527"


class EvidenceError(RuntimeError):
    """The compact evidence is malformed, stale, or overclaims its bound source."""


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _repo_file(root: Path, value: object, context: str) -> tuple[str, Path]:
    if not isinstance(value, str) or not value or "\\" in value or "\0" in value:
        raise EvidenceError(f"{context} is not a canonical relative POSIX path")
    relative = PurePosixPath(value)
    if relative.is_absolute() or relative.as_posix() != value or any(
        part in {"", ".", ".."} for part in relative.parts
    ):
        raise EvidenceError(f"{context} is not a canonical relative POSIX path")
    path = root
    for part in relative.parts:
        path /= part
        if path.is_symlink():
            raise EvidenceError(f"{context} contains a symlink: {value}")
    if not path.is_file():
        raise EvidenceError(f"{context} is absent or not a regular file: {value}")
    try:
        path.resolve(strict=True).relative_to(root.resolve(strict=True))
    except ValueError as exc:
        raise EvidenceError(f"{context} escapes the repository root: {value}") from exc
    return value, path


def _load(path: Path, context: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_bytes())
    except (OSError, json.JSONDecodeError) as exc:
        raise EvidenceError(f"{context} is unreadable") from exc
    if not isinstance(value, dict):
        raise EvidenceError(f"{context} root must be an object")
    return value


def _exact(value: object, fields: set[str], context: str) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != fields:
        raise EvidenceError(f"{context} fields are invalid")
    return value


def _require_equal(actual: object, expected: object, context: str) -> None:
    if type(actual) is not type(expected) or actual != expected:
        raise EvidenceError(f"{context} drift: expected {expected!r}, found {actual!r}")


def _require_digest(actual: object, expected: str, context: str) -> None:
    if not isinstance(actual, str) or len(actual) != 64 or any(
        character not in "0123456789abcdef" for character in actual
    ):
        raise EvidenceError(f"{context} is not a lowercase SHA-256 digest")
    if actual != expected:
        raise EvidenceError(f"{context} drift: expected {expected}, found {actual}")


def _verify_source(root: Path, record: object) -> None:
    source = _exact(record, {"path", "sha256"}, "lifecycle evidence_source")
    _require_equal(source["path"], SOURCE_PATH, "lifecycle evidence_source.path")
    _, source_file = _repo_file(root, source["path"], "lifecycle evidence source")
    actual_digest = _sha256(source_file)
    _require_digest(source["sha256"], actual_digest, "lifecycle evidence_source.sha256")
    _require_digest(actual_digest, SOURCE_SHA256, "supported lifecycle prose SHA-256")

    normalized = " ".join(source_file.read_text(encoding="utf-8").split())
    required_statements = (
        "Generation `relaunch-v22` supplied that missing active-match exercise on 2026-08-09 in a freshly paused solo skirmish, PID `13876`, supported executable SHA-256 and runtime base `0x00D60000`.",
        "The immutable controller DLL had SHA-256 `e2829ae2ae93e24e95b87d2d9e469fc79e915b67e30f53d9638b84fbe6c92527`, loaded once at `0x6AEA0000` with image size `0x165000`.",
        "Five consecutive rearm cycles and a final park each acknowledged on the main-thread boundary with `dropped_events=0`; after every STOP, the host's independent external read reproduced `E8 45 67 3C 00`.",
        "No new file appeared in the scoped WER dump directory.",
    )
    for statement in required_statements:
        if statement not in normalized:
            raise EvidenceError("bound lifecycle prose no longer contains the measured statement")
    if RETAIL_EXE_SHA256 not in normalized:
        raise EvidenceError("bound lifecycle prose no longer names the supported executable hash")


def verify_lifecycle(root: Path, artifact_path: Path) -> dict[str, Any]:
    root = root.resolve(strict=True)
    artifact = _exact(
        _load(artifact_path, f"lifecycle artifact {artifact_path}"),
        {
            "schema",
            "recorded_on",
            "evidence_source",
            "supported_retail",
            "controller",
            "session",
            "result",
        },
        "lifecycle artifact",
    )
    _require_equal(
        artifact["schema"], "don.retail-control.lifecycle-proof.v1", "lifecycle schema"
    )
    _require_equal(artifact["recorded_on"], "2026-08-09", "lifecycle recorded_on")
    _verify_source(root, artifact["evidence_source"])

    retail = _exact(
        artifact["supported_retail"], {"executable_sha256", "runtime_base"}, "supported_retail"
    )
    _require_digest(retail["executable_sha256"], RETAIL_EXE_SHA256, "supported executable SHA-256")
    _require_equal(retail["runtime_base"], "0x00D60000", "runtime base")

    controller = _exact(
        artifact["controller"],
        {"generation", "dll_sha256", "load_base", "image_size"},
        "controller",
    )
    _require_equal(controller["generation"], "relaunch-v22", "controller generation")
    _require_digest(controller["dll_sha256"], CONTROLLER_DLL_SHA256, "controller DLL SHA-256")
    _require_equal(controller["load_base"], "0x6AEA0000", "controller load base")
    _require_equal(controller["image_size"], "0x165000", "controller image size")

    session = _exact(artifact["session"], {"pid", "scope"}, "session")
    _require_equal(session["pid"], 13876, "session PID")
    _require_equal(session["scope"], "freshly paused solo skirmish", "session scope")

    result = _exact(
        artifact["result"],
        {
            "consecutive_stop_rearm_cycles",
            "final_park_acknowledged",
            "acknowledgement_boundary",
            "dropped_events_after_each_acknowledgement",
            "external_stop_read",
            "scoped_wer_dump_directory_new_files",
        },
        "lifecycle result",
    )
    _require_equal(result["consecutive_stop_rearm_cycles"], 5, "STOP/rearm cycle count")
    _require_equal(result["final_park_acknowledged"], True, "final park acknowledgement")
    _require_equal(result["acknowledgement_boundary"], "retail-main-thread", "acknowledgement boundary")
    _require_equal(result["dropped_events_after_each_acknowledgement"], 0, "dropped events")
    external = _exact(result["external_stop_read"], {"coverage", "bytes_hex"}, "external STOP read")
    _require_equal(external["coverage"], "after-every-stop", "external read coverage")
    _require_equal(external["bytes_hex"], "E8 45 67 3C 00", "restored call bytes")
    _require_equal(result["scoped_wer_dump_directory_new_files"], 0, "new scoped WER files")

    return {
        "artifact": LIFECYCLE_PATH,
        "artifact_sha256": _sha256(artifact_path),
        "controller_dll_sha256": CONTROLLER_DLL_SHA256,
        "consecutive_stop_rearm_cycles": 5,
        "final_park_acknowledged": True,
        "restored_bytes_after_every_stop": "E8 45 67 3C 00",
        "dropped_events": 0,
        "new_scoped_wer_files": 0,
    }


def _verify_incident(root: Path, record: object) -> None:
    incident_record = _exact(
        record,
        {"path", "sha256", "controller_generation", "pid", "dump_retained", "causality_status"},
        "original_incident",
    )
    _require_equal(incident_record["path"], INCIDENT_PATH, "original incident path")
    _, incident_file = _repo_file(root, incident_record["path"], "original incident")
    actual_digest = _sha256(incident_file)
    _require_digest(incident_record["sha256"], actual_digest, "original incident SHA-256")
    _require_digest(actual_digest, INCIDENT_SHA256, "supported incident SHA-256")
    _require_equal(incident_record["controller_generation"], "tactical-v19", "incident generation")
    _require_equal(incident_record["pid"], 12324, "incident PID")
    _require_equal(incident_record["dump_retained"], False, "incident dump_retained")
    _require_equal(
        incident_record["causality_status"],
        "unresolved-no-retained-stack",
        "incident causality status",
    )

    incident = _load(incident_file, "original incident source")
    _require_equal(incident.get("schema"), "don.retail-control-incident.v1", "incident schema")
    body = incident.get("incident")
    if not isinstance(body, dict):
        raise EvidenceError("original incident body is invalid")
    _require_equal(body.get("pid"), 12324, "source incident PID")
    _require_equal(body.get("controller_generation"), "tactical-v19", "source incident generation")
    wer = body.get("windows_error_reporting")
    if not isinstance(wer, dict):
        raise EvidenceError("original incident WER record is invalid")
    _require_equal(wer.get("dump_retained"), False, "source incident dump_retained")
    causality = body.get("causality")
    if not isinstance(causality, str) or "does not claim" not in causality or "uniquely proven causal" not in causality:
        raise EvidenceError("original incident no longer preserves the unresolved-causality boundary")


def verify_closure(root: Path, artifact_path: Path) -> dict[str, Any]:
    root = root.resolve(strict=True)
    artifact = _exact(
        _load(artifact_path, f"incident closure artifact {artifact_path}"),
        {"schema", "recorded_on", "original_incident", "later_active_lifecycle", "closure"},
        "incident closure artifact",
    )
    _require_equal(
        artifact["schema"], "don.retail-control.incident-closure.v1", "incident closure schema"
    )
    _require_equal(artifact["recorded_on"], "2026-08-09", "incident closure recorded_on")
    _verify_incident(root, artifact["original_incident"])

    later = _exact(
        artifact["later_active_lifecycle"],
        {"path", "sha256", "controller_generation", "controller_dll_sha256", "process_scope"},
        "later_active_lifecycle",
    )
    _require_equal(later["path"], LIFECYCLE_PATH, "later lifecycle path")
    _, lifecycle_file = _repo_file(root, later["path"], "later lifecycle artifact")
    actual_lifecycle_digest = _sha256(lifecycle_file)
    _require_digest(later["sha256"], actual_lifecycle_digest, "later lifecycle SHA-256")
    lifecycle = verify_lifecycle(root, lifecycle_file)
    _require_equal(later["controller_generation"], "relaunch-v22", "later controller generation")
    _require_digest(later["controller_dll_sha256"], CONTROLLER_DLL_SHA256, "later controller DLL SHA-256")
    _require_equal(later["process_scope"], "freshly paused solo skirmish", "later process scope")

    closure = _exact(
        artifact["closure"],
        {
            "active_main_thread_boundary_exercised",
            "consecutive_stop_rearm_cycles",
            "final_park_acknowledged",
            "dropped_events_after_each_acknowledgement",
            "exact_original_bytes_restored_after_every_stop",
            "scoped_wer_dump_directory_new_files",
            "candidate_bound_reversibility_soak",
            "original_incident_causality_remains_unresolved",
        },
        "incident closure result",
    )
    expected = {
        "active_main_thread_boundary_exercised": True,
        "consecutive_stop_rearm_cycles": 5,
        "final_park_acknowledged": True,
        "dropped_events_after_each_acknowledgement": 0,
        "exact_original_bytes_restored_after_every_stop": True,
        "scoped_wer_dump_directory_new_files": 0,
        "candidate_bound_reversibility_soak": True,
        "original_incident_causality_remains_unresolved": True,
    }
    for field, value in expected.items():
        _require_equal(closure[field], value, f"incident closure {field}")

    return {
        "artifact": CLOSURE_PATH,
        "artifact_sha256": _sha256(artifact_path),
        "original_incident_sha256": INCIDENT_SHA256,
        "lifecycle_artifact_sha256": lifecycle["artifact_sha256"],
        "candidate_bound_reversibility_soak": True,
        "original_incident_causality": "unresolved-no-retained-stack",
    }


def verify(root: Path, lifecycle_artifact: Path, closure_artifact: Path) -> dict[str, Any]:
    lifecycle = verify_lifecycle(root, lifecycle_artifact)
    closure = verify_closure(root, closure_artifact)
    return {
        "schema": "don.retail-control.release-evidence-check.v1",
        "ok": True,
        "lifecycle": lifecycle,
        "incident_closure": closure,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    parser.add_argument("--lifecycle-artifact", type=Path)
    parser.add_argument("--closure-artifact", type=Path)
    args = parser.parse_args(argv)
    root = args.root.resolve()
    lifecycle = args.lifecycle_artifact or root / LIFECYCLE_PATH
    closure = args.closure_artifact or root / CLOSURE_PATH
    try:
        report = verify(root, lifecycle, closure)
    except (EvidenceError, OSError) as exc:
        print(f"retail-control evidence: FAIL: {exc}", file=sys.stderr)
        return 2
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
