#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Validate the hash-bound release proof pack and refuse unproved readiness claims."""

from __future__ import annotations

import argparse
from datetime import date
import hashlib
import json
from pathlib import Path, PurePosixPath
import sys
import tomllib
from typing import Any


HERE = Path(__file__).resolve().parent
DEFAULT_ROOT = HERE.parents[1]
DEFAULT_MANIFEST = DEFAULT_ROOT / "docs/release-proof/evidence-manifest.json"
SCOPES = {"source", "distribution"}
REQUIRED_GATE_SCOPES = {
    "source-license-coverage": {"source", "distribution"},
    "remote-workspace-license-consistency": {"source", "distribution"},
    "whole-source-license-provenance": {"source", "distribution"},
    "source-proprietary-payload-exclusion": {"source"},
    "derived-research-release-review": {"source", "distribution"},
    "binary-third-party-notices": {"distribution"},
    "owned-data-bootstrap-boundary": {"distribution"},
    "standalone-product-installer": {"distribution"},
    "independent-presentation-content": {"distribution"},
    "retail-controller-byte-restoration": {"distribution"},
    "scoped-crash-dump-workflow": {"distribution"},
    "controller-stop-incident-closure": {"distribution"},
    "release-documentation-consistency": {"source", "distribution"},
    "assembled-product-packaging": {"distribution"},
}
REQUIRED_COMPLETION_ARTIFACTS = {
    "whole-source-license-provenance": (
        "release/source-license-inventory.json",
        "don.source-license-inventory.v1",
    ),
    "derived-research-release-review": (
        "release/derived-research-review.json",
        "don.derived-research-review.v1",
    ),
    "binary-third-party-notices": (
        "release/product-dependency-notices.json",
        "don.product-dependency-notices.v1",
    ),
    "standalone-product-installer": (
        "release/installer-evidence.json",
        "don.release-installer-evidence.v1",
    ),
    "independent-presentation-content": (
        "release/content-clearance.json",
        "don.release-content-clearance.v1",
    ),
    "retail-controller-byte-restoration": (
        "schema/live/retail-control-active-stop-cycles-v1.json",
        "don.retail-control.lifecycle-proof.v1",
    ),
    "controller-stop-incident-closure": (
        "schema/live/retail-control-stop-incident-closure-v1.json",
        "don.retail-control.incident-closure.v1",
    ),
    "assembled-product-packaging": (
        "release/product-payload.json",
        "don.release-product-payload.v1",
    ),
}
KNOWN_LICENSE_TEXT_DIGESTS = {
    "3972dc9744f6499f0f9b2dbf76696f2ae7ad8af9b23dde66d6af86c9dfb36986": {
        "GPL-3.0-only",
        "GPL-3.0-or-later",
    },
}
BOUND_PROVED_GATES = {
    "source-proprietary-payload-exclusion",
    "owned-data-bootstrap-boundary",
    "scoped-crash-dump-workflow",
}
IGNORED_DISCOVERY_PARTS = {
    ".git",
    ".revive",
    "ron-bin",
    "ron-data",
    "target",
    "__pycache__",
}


class ProofError(RuntimeError):
    """The proof pack is malformed, stale, or contradicts repository state."""


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _relative(value: object, field: str) -> str:
    if not isinstance(value, str) or not value or "\\" in value or "\0" in value:
        raise ProofError(f"{field} is not a canonical relative POSIX path")
    path = PurePosixPath(value)
    if path.is_absolute() or any(part in {"", ".", ".."} for part in path.parts):
        raise ProofError(f"{field} escapes the repository root")
    if path.as_posix() != value:
        raise ProofError(f"{field} is not a canonical relative POSIX path")
    return path.as_posix()


def _regular_file(root: Path, relative: str) -> Path:
    path = root
    for part in PurePosixPath(relative).parts:
        path /= part
        if path.is_symlink():
            raise ProofError(f"evidence path contains a symlink: {relative}")
    if not path.is_file():
        raise ProofError(f"evidence is absent, non-regular, or a symlink: {relative}")
    try:
        path.resolve(strict=True).relative_to(root)
    except ValueError as exc:
        raise ProofError(f"evidence escapes the repository root: {relative}") from exc
    return path


def _optional_regular_file(root: Path, relative: str) -> Path | None:
    path = root
    parts = PurePosixPath(relative).parts
    for index, part in enumerate(parts):
        path /= part
        if path.is_symlink():
            raise ProofError(f"optional evidence path contains a symlink: {relative}")
        if not path.exists():
            return None
        if index + 1 < len(parts) and not path.is_dir():
            raise ProofError(f"optional evidence parent is not a directory: {relative}")
    if not path.is_file():
        raise ProofError(f"optional evidence is not a regular file: {relative}")
    return path


def _discover(root: Path, basename: str) -> set[str]:
    found: set[str] = set()
    for path in root.rglob(basename):
        relative = path.relative_to(root)
        if any(part in IGNORED_DISCOVERY_PARTS for part in relative.parts):
            continue
        if path.is_symlink() or not path.is_file():
            raise ProofError(f"discovered {basename} is not a regular file: {relative.as_posix()}")
        found.add(relative.as_posix())
    return found


def _discover_cargo_manifests(root: Path) -> set[str]:
    return _discover(root, "*Cargo.toml")


def _object_list(payload: dict[str, Any], field: str) -> list[dict[str, Any]]:
    value = payload.get(field)
    if not isinstance(value, list) or not value or not all(isinstance(item, dict) for item in value):
        raise ProofError(f"{field} must be a non-empty object list")
    return value


def _unique_paths(records: list[dict[str, Any]], field: str) -> dict[str, dict[str, Any]]:
    by_path: dict[str, dict[str, Any]] = {}
    for index, record in enumerate(records):
        path = _relative(record.get("path"), f"{field}[{index}].path")
        if path in by_path:
            raise ProofError(f"duplicate {field} path: {path}")
        by_path[path] = record
    return by_path


def _validate_hashes(root: Path, payload: dict[str, Any]) -> dict[str, dict[str, Any]]:
    records = _object_list(payload, "hash_bound_files")
    by_path = _unique_paths(records, "hash_bound_files")
    for path, record in by_path.items():
        expected = record.get("sha256")
        if not isinstance(expected, str) or len(expected) != 64 or any(
            char not in "0123456789abcdef" for char in expected
        ):
            raise ProofError(f"invalid SHA-256 for {path}")
        role = record.get("role")
        if not isinstance(role, str) or not role:
            raise ProofError(f"missing evidence role for {path}")
        actual = _sha256(_regular_file(root, path))
        if actual != expected:
            raise ProofError(f"hash drift for {path}: expected {expected}, found {actual}")
    return by_path


def _declared_license(manifest: dict[str, Any], kind: str) -> str:
    if kind in {"workspace", "workspace-template"}:
        try:
            value = manifest["workspace"]["package"]["license"]
        except (KeyError, TypeError) as exc:
            raise ProofError("workspace manifest has no workspace.package.license") from exc
    else:
        try:
            value = manifest["package"]["license"]
        except (KeyError, TypeError) as exc:
            raise ProofError("package manifest has no package.license") from exc
    if value == {"workspace": True}:
        return "workspace"
    if not isinstance(value, str) or not value:
        raise ProofError("Cargo license declaration is not a string or workspace inheritance")
    return value


def _validate_cargo_manifests(
    root: Path,
    payload: dict[str, Any],
    hash_records: dict[str, dict[str, Any]],
    license_catalog: dict[str, set[str]],
) -> tuple[list[str], list[str]]:
    records = _object_list(payload, "repository_cargo_manifests")
    by_path = _unique_paths(records, "repository_cargo_manifests")
    discovered = _discover_cargo_manifests(root)
    if set(by_path) != discovered:
        missing = sorted(discovered - set(by_path))
        stale = sorted(set(by_path) - discovered)
        raise ProofError(f"Cargo manifest coverage drift: missing={missing}, stale={stale}")

    workspace_license: str | None = None
    blocked: list[str] = []
    template_mismatches: list[str] = []
    for path, record in by_path.items():
        kind = record.get("kind")
        if kind not in {"workspace", "workspace-template", "package"}:
            raise ProofError(f"invalid Cargo manifest kind for {path}")
        try:
            parsed = tomllib.loads(_regular_file(root, path).read_text(encoding="utf-8"))
        except (OSError, UnicodeError, tomllib.TOMLDecodeError) as exc:
            raise ProofError(f"cannot parse Cargo manifest: {path}") from exc
        actual = _declared_license(parsed, kind)
        if record.get("declared") != actual:
            raise ProofError(
                f"license declaration drift for {path}: expected {record.get('declared')!r}, "
                f"found {actual!r}"
            )
        if kind == "workspace":
            if workspace_license is not None:
                raise ProofError("more than one workspace license root is declared")
            workspace_license = actual

    if workspace_license is None:
        raise ProofError("no workspace license root is declared")
    for path, record in by_path.items():
        effective = workspace_license if record["declared"] == "workspace" else record["declared"]
        if record.get("effective") != effective:
            raise ProofError(f"effective license drift for {path}")
        state = record.get("state")
        text = record.get("license_text")
        if state == "covered":
            text_path = _relative(text, f"license_text for {path}")
            _regular_file(root, text_path)
            if text_path not in hash_records:
                raise ProofError(f"license text is not hash-bound for {path}: {text_path}")
            if effective not in license_catalog.get(text_path, set()):
                raise ProofError(
                    f"license text catalog does not map {effective!r} to {text_path} for {path}"
                )
        elif state == "blocked_missing_license_text":
            if text is not None:
                raise ProofError(f"blocked license record unexpectedly names text: {path}")
            blocked.append(path)
        else:
            raise ProofError(f"invalid license coverage state for {path}: {state!r}")
        if record["kind"] == "workspace-template" and effective != workspace_license:
            template_mismatches.append(path)
    return blocked, template_mismatches


def _validate_license_catalog(
    root: Path, payload: dict[str, Any], hash_records: dict[str, dict[str, Any]]
) -> dict[str, set[str]]:
    records = _object_list(payload, "license_text_catalog")
    by_path = _unique_paths(records, "license_text_catalog")
    catalog: dict[str, set[str]] = {}
    for path, record in by_path.items():
        _regular_file(root, path)
        if path not in hash_records:
            raise ProofError(f"license catalog text is not hash-bound: {path}")
        expressions = record.get("expressions")
        if (
            not isinstance(expressions, list)
            or not expressions
            or any(not isinstance(value, str) or not value for value in expressions)
            or len(set(expressions)) != len(expressions)
        ):
            raise ProofError(f"invalid license expressions for catalog text: {path}")
        evidence = record.get("evidence")
        if not isinstance(evidence, str) or not evidence:
            raise ProofError(f"missing license catalog evidence description: {path}")
        digest = hash_records[path]["sha256"]
        authoritative = KNOWN_LICENSE_TEXT_DIGESTS.get(digest)
        if authoritative is None or set(expressions) != authoritative:
            raise ProofError(
                f"license catalog mapping is not recognized for {path} at SHA-256 {digest}"
            )
        catalog[path] = set(expressions)
    return catalog


def _validate_locks(
    root: Path, payload: dict[str, Any], hash_records: dict[str, dict[str, Any]]
) -> list[str]:
    records = _object_list(payload, "locked_dependency_sets")
    by_path = _unique_paths(records, "locked_dependency_sets")
    discovered = _discover(root, "Cargo.lock")
    if set(by_path) != discovered:
        missing = sorted(discovered - set(by_path))
        stale = sorted(set(by_path) - discovered)
        raise ProofError(f"Cargo lock coverage drift: missing={missing}, stale={stale}")
    missing_notices: list[str] = []
    for path, record in by_path.items():
        if path not in hash_records:
            raise ProofError(f"Cargo lock is not hash-bound: {path}")
        try:
            parsed = tomllib.loads(_regular_file(root, path).read_text(encoding="utf-8"))
        except (OSError, UnicodeError, tomllib.TOMLDecodeError) as exc:
            raise ProofError(f"cannot parse Cargo lock: {path}") from exc
        packages = parsed.get("package")
        count = len(packages) if isinstance(packages, list) else 0
        expected_count = record.get("package_records")
        if type(expected_count) is not int or expected_count <= 0:
            raise ProofError(f"invalid Cargo lock package count for {path}")
        if expected_count != count:
            raise ProofError(
                f"Cargo lock package-count drift for {path}: "
                f"expected {record.get('package_records')}, found {count}"
            )
        notice = record.get("notice_inventory")
        if notice is not None:
            notice_path = _relative(notice, f"notice_inventory for {path}")
            if notice_path not in hash_records:
                raise ProofError(f"dependency notice inventory is not hash-bound: {notice_path}")
            _validate_notice_inventory(
                root,
                notice_path,
                path,
                hash_records[path]["sha256"],
                packages if isinstance(packages, list) else [],
                hash_records,
            )
        else:
            missing_notices.append(path)
    return missing_notices


def _package_identity(package: dict[str, Any]) -> tuple[object, object, object, object]:
    return (
        package.get("name"),
        package.get("version"),
        package.get("source"),
        package.get("checksum"),
    )


def _validate_notice_inventory(
    root: Path,
    inventory_path: str,
    lock_path: str,
    lock_sha256: str,
    lock_packages: list[dict[str, Any]],
    hash_records: dict[str, dict[str, Any]],
) -> None:
    try:
        inventory = json.loads(_regular_file(root, inventory_path).read_bytes())
    except (OSError, json.JSONDecodeError) as exc:
        raise ProofError(f"dependency notice inventory is unreadable: {inventory_path}") from exc
    if not isinstance(inventory, dict) or set(inventory) != {
        "schema",
        "lockfile",
        "lockfile_sha256",
        "packages",
    }:
        raise ProofError(f"dependency notice inventory fields are invalid: {inventory_path}")
    if inventory["schema"] != "don.dependency-notices.v1":
        raise ProofError(f"dependency notice inventory schema is unsupported: {inventory_path}")
    if inventory["lockfile"] != lock_path or inventory["lockfile_sha256"] != lock_sha256:
        raise ProofError(f"dependency notice inventory is bound to the wrong lock: {inventory_path}")
    packages = inventory["packages"]
    if not isinstance(packages, list) or not all(isinstance(item, dict) for item in packages):
        raise ProofError(f"dependency notice package records are invalid: {inventory_path}")
    expected = [_package_identity(package) for package in lock_packages]
    actual: list[tuple[object, object, object, object]] = []
    for index, package in enumerate(packages):
        if set(package) != {
            "name",
            "version",
            "source",
            "checksum",
            "license_expression",
            "license_evidence",
            "required_notice_paths",
            "source_provision",
        }:
            raise ProofError(
                f"dependency notice package fields are invalid at {inventory_path}:{index}"
            )
        actual.append(_package_identity(package))
        expression = package["license_expression"]
        provision = package["source_provision"]
        if not isinstance(expression, str) or not expression:
            raise ProofError(f"missing dependency license expression at {inventory_path}:{index}")
        if not isinstance(provision, str) or not provision:
            raise ProofError(f"missing dependency source treatment at {inventory_path}:{index}")
        for field in ("license_evidence", "required_notice_paths"):
            paths = package[field]
            if not isinstance(paths, list) or not paths:
                raise ProofError(f"missing {field} at {inventory_path}:{index}")
            for path_index, value in enumerate(paths):
                evidence_path = _relative(
                    value, f"{inventory_path}.packages[{index}].{field}[{path_index}]"
                )
                _regular_file(root, evidence_path)
                if evidence_path not in hash_records:
                    raise ProofError(
                        f"dependency {field} is not hash-bound: {evidence_path}"
                    )
    if actual != expected:
        raise ProofError(f"dependency notice inventory package order/identity drift: {inventory_path}")


def _validate_owned_inputs(root: Path) -> None:
    path = _regular_file(root, "tools/install/owned-inputs.json")
    try:
        payload = json.loads(path.read_bytes())
    except (OSError, json.JSONDecodeError) as exc:
        raise ProofError("owned-input manifest is unreadable") from exc
    if not isinstance(payload, dict) or set(payload) != {"schema", "retail_identity", "files"}:
        raise ProofError("owned-input manifest fields are invalid")
    if payload.get("schema") != "don.owned-inputs.v1":
        raise ProofError("owned-input manifest schema drift")
    files = payload.get("files")
    if not isinstance(files, list) or len(files) != 51:
        raise ProofError("owned-input manifest no longer contains exactly 51 file entries")
    identity = payload.get("retail_identity")
    if not isinstance(identity, dict) or set(identity) != {"source", "size", "sha256"}:
        raise ProofError("owned-input retail identity fields are invalid")
    if identity.get("sha256") != (
        "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079"
    ):
        raise ProofError("owned-input retail executable identity drift")
    entries = [(identity, "retail_identity", False)] + [
        (entry, f"files[{index}]", True) for index, entry in enumerate(files)
    ]
    sources: list[str] = []
    destinations: list[str] = []
    for entry, label, has_destination in entries:
        expected_fields = {"source", "size", "sha256"}
        if has_destination:
            expected_fields.add("destination")
        if not isinstance(entry, dict) or set(entry) != expected_fields:
            raise ProofError(f"owned-input {label} fields are invalid")
        source = _relative(entry["source"], f"owned-input {label}.source")
        size = entry["size"]
        digest = entry["sha256"]
        if type(size) is not int or size <= 0:
            raise ProofError(f"owned-input {label}.size is invalid")
        if (
            not isinstance(digest, str)
            or len(digest) != 64
            or any(char not in "0123456789abcdef" for char in digest)
        ):
            raise ProofError(f"owned-input {label}.sha256 is invalid")
        if has_destination:
            destination = _relative(
                entry["destination"], f"owned-input {label}.destination"
            )
            sources.append(source.casefold())
            destinations.append(destination.casefold())
    if len(set(sources)) != len(sources) or len(set(destinations)) != len(destinations):
        raise ProofError("owned-input manifest has case-insensitive path collisions")


def _incident_dump_retained(root: Path) -> bool:
    path = _regular_file(root, "schema/live/retail-control-stop-incident-v1.json")
    try:
        payload = json.loads(path.read_bytes())
        retained = payload["incident"]["windows_error_reporting"]["dump_retained"]
    except (OSError, json.JSONDecodeError, KeyError, TypeError) as exc:
        raise ProofError("retail-control incident dump evidence is unreadable") from exc
    if not isinstance(retained, bool):
        raise ProofError("retail-control incident dump_retained is not boolean")
    return retained


def _known_documentation_conflicts(root: Path) -> list[str]:
    readme = _regular_file(root, "README.md").read_text(encoding="utf-8")
    goal = _regular_file(root, "GOAL.md").read_text(encoding="utf-8")
    live_control = _regular_file(root, "docs/tooling/live-control.md").read_text(encoding="utf-8")
    conflicts: list[str] = []
    if "STOP/rearm lifecycle still needs a fresh active-match exercise" in readme:
        conflicts.append("README still says the active STOP/rearm exercise is missing")
    if (
        "real-host peering remain separate open gates" in live_control
        and "real-host lobby peering green" in goal
    ):
        conflicts.append("live-control and GOAL disagree about real-host lobby peering")
    return conflicts


def _validate_gates(
    root: Path,
    payload: dict[str, Any],
    missing_license_texts: list[str],
    template_license_mismatches: list[str],
    documentation_conflicts: list[str],
    hash_records: dict[str, dict[str, Any]],
) -> tuple[dict[str, list[str]], dict[str, bool]]:
    gates = _object_list(payload, "gates")
    seen: set[str] = set()
    blockers = {scope: [] for scope in sorted(SCOPES)}
    for index, gate in enumerate(gates):
        gate_id = gate.get("id")
        if not isinstance(gate_id, str) or not gate_id or gate_id in seen:
            raise ProofError(f"invalid or duplicate gate id at index {index}")
        seen.add(gate_id)
        scopes = gate.get("scopes")
        if (
            not isinstance(scopes, list)
            or not scopes
            or any(scope not in SCOPES for scope in scopes)
            or len(set(scopes)) != len(scopes)
        ):
            raise ProofError(f"invalid scopes for gate {gate_id}")
        expected_scopes = REQUIRED_GATE_SCOPES.get(gate_id)
        if expected_scopes is None or set(scopes) != expected_scopes:
            raise ProofError(
                f"gate {gate_id} scope drift: expected {sorted(expected_scopes or [])}, "
                f"found {sorted(scopes)}"
            )
        status = gate.get("status")
        if status not in {"proved", "blocked"}:
            raise ProofError(f"invalid status for gate {gate_id}")
        claim = gate.get("claim")
        if not isinstance(claim, str) or not claim:
            raise ProofError(f"missing claim for gate {gate_id}")
        evidence = gate.get("evidence")
        if not isinstance(evidence, list) or not evidence:
            raise ProofError(f"gate {gate_id} has no evidence")
        for evidence_index, value in enumerate(evidence):
            path = _relative(value, f"gates[{index}].evidence[{evidence_index}]")
            _regular_file(root, path)
        blocker = gate.get("blocker")
        if status == "blocked":
            if not isinstance(blocker, str) or not blocker:
                raise ProofError(f"blocked gate {gate_id} does not explain its blocker")
            for scope in scopes:
                blockers[scope].append(gate_id)
        elif blocker is not None:
            raise ProofError(f"proved gate {gate_id} unexpectedly carries a blocker")
        if status == "proved" and gate_id in BOUND_PROVED_GATES:
            unbound = [value for value in evidence if value not in hash_records]
            if unbound:
                raise ProofError(f"proved gate {gate_id} has unbound evidence: {unbound}")

    if seen != set(REQUIRED_GATE_SCOPES):
        missing = sorted(set(REQUIRED_GATE_SCOPES) - seen)
        extra = sorted(seen - set(REQUIRED_GATE_SCOPES))
        raise ProofError(f"release gate registry drift: missing={missing}, extra={extra}")
    by_id = {gate["id"]: gate for gate in gates}
    derived = {
        "source-license-coverage": "blocked" if missing_license_texts else "proved",
        "remote-workspace-license-consistency": (
            "blocked" if template_license_mismatches else "proved"
        ),
        "release-documentation-consistency": "blocked" if documentation_conflicts else "proved",
    }
    for gate_id, expected_status in derived.items():
        if gate_id not in by_id:
            raise ProofError(f"required derived gate is absent: {gate_id}")
        if by_id[gate_id]["status"] != expected_status:
            raise ProofError(
                f"gate {gate_id} contradicts derived evidence: "
                f"expected {expected_status}, found {by_id[gate_id]['status']}"
            )
    for gate_id, (artifact_path, artifact_schema) in REQUIRED_COMPLETION_ARTIFACTS.items():
        artifact = _optional_regular_file(root, artifact_path)
        if artifact is not None:
            if artifact_path not in hash_records:
                raise ProofError(f"completion artifact is not hash-bound: {artifact_path}")
            try:
                artifact_payload = json.loads(artifact.read_bytes())
            except (OSError, json.JSONDecodeError) as exc:
                raise ProofError(f"completion artifact is unreadable: {artifact_path}") from exc
            if (
                not isinstance(artifact_payload, dict)
                or artifact_payload.get("schema") != artifact_schema
            ):
                raise ProofError(
                    f"completion artifact schema drift for {artifact_path}: "
                    f"expected {artifact_schema}"
                )
            if artifact_path not in by_id[gate_id]["evidence"]:
                raise ProofError(f"completion artifact is absent from gate evidence: {artifact_path}")
            raise ProofError(
                f"completion artifact {artifact_path} has no schema-specific semantic validator; "
                "implement one before promoting its gate"
            )
        if by_id[gate_id]["status"] != "blocked":
            raise ProofError(
                f"gate {gate_id} contradicts completion artifact {artifact_path}: "
                f"expected blocked, found {by_id[gate_id]['status']}"
            )

    readiness = {scope: not blockers[scope] for scope in sorted(SCOPES)}
    declared = payload.get("declared_readiness")
    if (
        not isinstance(declared, dict)
        or set(declared) != SCOPES
        or any(type(value) is not bool for value in declared.values())
    ):
        raise ProofError("declared_readiness fields are invalid")
    if declared != readiness:
        raise ProofError(f"declared_readiness contradicts gates: declared={declared}, actual={readiness}")
    return blockers, readiness


def validate(root: Path, manifest_path: Path) -> dict[str, Any]:
    if root.is_symlink() or not root.is_dir():
        raise ProofError("repository root is absent or a symlink")
    root = root.resolve(strict=True)
    try:
        payload = json.loads(manifest_path.read_bytes())
    except (OSError, json.JSONDecodeError) as exc:
        raise ProofError("release proof manifest is unreadable") from exc
    if not isinstance(payload, dict) or payload.get("schema") != "don.release-proof.v1":
        raise ProofError("release proof manifest schema is unsupported")
    if set(payload) != {
        "schema",
        "snapshot_date",
        "claims_policy",
        "hash_bound_files",
        "license_text_catalog",
        "repository_cargo_manifests",
        "locked_dependency_sets",
        "gates",
        "declared_readiness",
    }:
        raise ProofError("release proof manifest top-level fields are invalid")
    snapshot_date = payload.get("snapshot_date")
    try:
        if not isinstance(snapshot_date, str) or date.fromisoformat(snapshot_date).isoformat() != snapshot_date:
            raise ValueError
    except ValueError as exc:
        raise ProofError("snapshot_date is not a canonical ISO calendar date") from exc
    policy = payload.get("claims_policy")
    required_policy = {"rights_claims", "retail_content", "blocked_means", "proved_means"}
    if (
        not isinstance(policy, dict)
        or set(policy) != required_policy
        or any(not isinstance(value, str) or not value for value in policy.values())
    ):
        raise ProofError("claims_policy fields are invalid")
    hash_records = _validate_hashes(root, payload)
    license_catalog = _validate_license_catalog(root, payload, hash_records)
    missing_license_texts, template_license_mismatches = _validate_cargo_manifests(
        root, payload, hash_records, license_catalog
    )
    missing_notice_inventories = _validate_locks(root, payload, hash_records)
    _validate_owned_inputs(root)
    incident_dump_retained = _incident_dump_retained(root)
    documentation_conflicts = _known_documentation_conflicts(root)
    blockers, readiness = _validate_gates(
        root,
        payload,
        missing_license_texts,
        template_license_mismatches,
        documentation_conflicts,
        hash_records,
    )
    return {
        "schema": "don.release-proof-check.v1",
        "ok": True,
        "snapshot_date": snapshot_date,
        "hash_bound_files": len(hash_records),
        "cargo_manifests": len(payload["repository_cargo_manifests"]),
        "lockfiles": len(payload["locked_dependency_sets"]),
        "lock_package_records": sum(
            record["package_records"] for record in payload["locked_dependency_sets"]
        ),
        "gates": len(payload["gates"]),
        "repository_locks_without_notice_inventory": missing_notice_inventories,
        "recorded_incident_dump_retained": incident_dump_retained,
        "documentation_conflicts": documentation_conflicts,
        "readiness": readiness,
        "blockers": blockers,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--require-ready", choices=sorted(SCOPES))
    args = parser.parse_args()
    try:
        report = validate(args.root, args.manifest)
    except ProofError as exc:
        print(f"release-proof: ERROR: {exc}", file=sys.stderr)
        return 2
    print(json.dumps(report, indent=2, sort_keys=True))
    if args.require_ready and not report["readiness"][args.require_ready]:
        blocked = ", ".join(report["blockers"][args.require_ready])
        print(
            f"release-proof: REFUSE {args.require_ready} readiness: {blocked}",
            file=sys.stderr,
        )
        return 3
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
