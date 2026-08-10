#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Validate the hash-bound release proof pack and refuse unproved readiness claims."""

from __future__ import annotations

import argparse
from datetime import date
import fnmatch
import hashlib
import importlib.util
import json
import os
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
    "source-archive-reproducibility": {"source", "distribution"},
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
    "controller-stop-incident-closure",
    "retail-controller-byte-restoration",
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
PAYLOAD_CATEGORIES = {
    "content-license",
    "dependency-notice",
    "first-party",
    "independent-content",
    "installer",
    "mixed-binary",
    "third-party-runtime",
}
DEPENDENCY_BEARING_CATEGORIES = {"mixed-binary", "third-party-runtime"}
INSTALLER_ACTIONS = {"install", "configure", "repair", "remove"}


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
    for directory, children, files in os.walk(root):
        children[:] = [
            child for child in children if child not in IGNORED_DISCOVERY_PARTS
        ]
        for name in files:
            if not fnmatch.fnmatchcase(name, basename):
                continue
            path = Path(directory) / name
            relative = path.relative_to(root)
            if path.is_symlink() or not path.is_file():
                raise ProofError(
                    f"discovered {basename} is not a regular file: {relative.as_posix()}"
                )
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


def _exact_object(value: object, fields: set[str], context: str) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != fields:
        raise ProofError(f"{context} fields are invalid")
    return value


def _nonempty(value: object, context: str) -> str:
    if not isinstance(value, str) or not value.strip() or value != value.strip():
        raise ProofError(f"{context} must be a non-empty string without surrounding whitespace")
    return value


def _digest(value: object, context: str) -> str:
    if (
        not isinstance(value, str)
        or len(value) != 64
        or value != value.lower()
        or any(char not in "0123456789abcdef" for char in value)
    ):
        raise ProofError(f"{context} must be a lowercase SHA-256 digest")
    return value


def _positive_integer(value: object, context: str) -> int:
    if type(value) is not int or value <= 0:
        raise ProofError(f"{context} must be a positive integer")
    return value


def _json_object(path: Path, context: str) -> dict[str, Any]:
    try:
        payload = json.loads(path.read_bytes())
    except (OSError, json.JSONDecodeError) as exc:
        raise ProofError(f"{context} is unreadable") from exc
    if not isinstance(payload, dict):
        raise ProofError(f"{context} root must be an object")
    return payload


def _payload_member(payload_root: str, value: object, context: str) -> str:
    relative = _relative(value, context)
    return (PurePosixPath(payload_root) / PurePosixPath(relative)).as_posix()


def _scan_payload(root: Path, payload_root: str) -> set[str]:
    directory = root
    for part in PurePosixPath(payload_root).parts:
        directory /= part
        if directory.is_symlink():
            raise ProofError(f"product payload root contains a symlink: {payload_root}")
    if not directory.is_dir():
        raise ProofError(f"product payload root is absent or not a directory: {payload_root}")
    found: set[str] = set()
    for path in directory.rglob("*"):
        relative = path.relative_to(directory).as_posix()
        if path.is_symlink():
            raise ProofError(f"product payload contains a symlink: {relative}")
        if path.is_dir():
            continue
        if not path.is_file():
            raise ProofError(f"product payload contains a special filesystem object: {relative}")
        canonical = _relative(relative, "product payload tree path")
        folded = canonical.casefold()
        if any(existing.casefold() == folded for existing in found):
            raise ProofError(f"product payload contains a case-insensitive path collision: {canonical}")
        found.add(canonical)
    return found


def _validate_product_payload(
    root: Path, artifact_path: str, artifact: dict[str, Any]
) -> dict[str, Any]:
    artifact = _exact_object(
        artifact,
        {"schema", "product", "version", "payload_root", "files"},
        artifact_path,
    )
    if artifact["schema"] != "don.release-product-payload.v1":
        raise ProofError(f"product payload schema drift: {artifact_path}")
    _nonempty(artifact["product"], f"{artifact_path}.product")
    _nonempty(artifact["version"], f"{artifact_path}.version")
    payload_root = _relative(artifact["payload_root"], f"{artifact_path}.payload_root")
    records = artifact["files"]
    if not isinstance(records, list) or not records:
        raise ProofError(f"{artifact_path}.files must be a non-empty list")
    by_path: dict[str, dict[str, Any]] = {}
    by_category: dict[str, set[str]] = {category: set() for category in PAYLOAD_CATEGORIES}
    folded_paths: set[str] = set()
    for index, raw in enumerate(records):
        record = _exact_object(
            raw,
            {"path", "size", "sha256", "role", "category"},
            f"{artifact_path}.files[{index}]",
        )
        relative = _relative(record["path"], f"{artifact_path}.files[{index}].path")
        folded = relative.casefold()
        if relative in by_path or folded in folded_paths:
            raise ProofError(f"duplicate or case-colliding product payload path: {relative}")
        folded_paths.add(folded)
        category = record["category"]
        if category not in PAYLOAD_CATEGORIES:
            raise ProofError(f"unsupported product payload category for {relative}: {category!r}")
        _nonempty(record["role"], f"product payload role for {relative}")
        expected_size = _positive_integer(record["size"], f"product payload size for {relative}")
        expected_digest = _digest(record["sha256"], f"product payload digest for {relative}")
        repository_path = _payload_member(payload_root, relative, "product payload member")
        file_path = _regular_file(root, repository_path)
        actual_size = file_path.stat().st_size
        actual_digest = _sha256(file_path)
        if actual_size != expected_size or actual_digest != expected_digest:
            raise ProofError(f"product payload bytes drift: {relative}")
        by_path[relative] = record
        by_category[str(category)].add(relative)
    discovered = _scan_payload(root, payload_root)
    declared = set(by_path)
    if discovered != declared:
        raise ProofError(
            "product payload coverage drift: "
            f"unmanifested={sorted(discovered - declared)}, absent={sorted(declared - discovered)}"
        )
    if not by_category["first-party"] and not by_category["mixed-binary"]:
        raise ProofError("product payload has no first-party product file")
    return {
        "artifact_path": artifact_path,
        "artifact_sha256": _sha256(_regular_file(root, artifact_path)),
        "payload_root": payload_root,
        "by_path": by_path,
        "by_category": by_category,
    }


def _load_content_license_auditor() -> Any:
    tool = HERE.parent / "content-license-audit.py"
    spec = importlib.util.spec_from_file_location("don_release_content_license_audit", tool)
    if spec is None or spec.loader is None:
        raise ProofError("content-license auditor cannot be loaded")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _validate_archive_reproducibility(root: Path) -> dict[str, Any]:
    tool = HERE / "archive_reproducibility.py"
    spec = importlib.util.spec_from_file_location("don_archive_reproducibility", tool)
    if spec is None or spec.loader is None:
        raise ProofError("archive reproducibility verifier cannot be loaded")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    artifact = root / "release/source-archive-reproducibility.json"
    try:
        return module.verify(root, artifact)
    except module.ReproducibilityError as exc:
        raise ProofError(f"archive reproducibility evidence failed: {exc}") from exc


def _load_retail_control_evidence_verifier() -> Any:
    tool = HERE / "retail_control_evidence.py"
    spec = importlib.util.spec_from_file_location("don_retail_control_evidence", tool)
    if spec is None or spec.loader is None:
        raise ProofError("retail-control evidence verifier cannot be loaded")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _validate_content_clearance(
    root: Path,
    artifact_path: str,
    artifact: dict[str, Any],
    product: dict[str, Any],
) -> dict[str, Any]:
    artifact = _exact_object(
        artifact,
        {
            "schema",
            "payload",
            "payload_sha256",
            "content_manifest",
            "audit_report",
            "human_review",
            "attribution_notice",
        },
        artifact_path,
    )
    if artifact["schema"] != "don.release-content-clearance.v1":
        raise ProofError(f"content clearance schema drift: {artifact_path}")
    if artifact["payload"] != product["artifact_path"]:
        raise ProofError("content clearance names the wrong product payload")
    if _digest(artifact["payload_sha256"], "content clearance payload_sha256") != product[
        "artifact_sha256"
    ]:
        raise ProofError("content clearance payload hash drift")
    manifest_path = _relative(artifact["content_manifest"], "content clearance manifest")
    payload_prefix = product["payload_root"] + "/"
    if not manifest_path.startswith(payload_prefix):
        raise ProofError("content clearance manifest is outside the product payload")
    manifest_relative = manifest_path[len(payload_prefix) :]
    if manifest_relative not in product["by_category"]["content-license"]:
        raise ProofError("content clearance manifest is not classified as content-license")
    manifest_file = _regular_file(root, manifest_path)
    auditor = _load_content_license_auditor()
    try:
        actual_report = auditor.audit_release(
            root / product["payload_root"], manifest_file
        )
    except auditor.AuditError as exc:
        raise ProofError(f"content clearance audit failed: {exc}") from exc
    if artifact["audit_report"] != actual_report:
        raise ProofError("stored content clearance audit report does not match live audit")
    if actual_report.get("ready") is not True or actual_report.get("legal_title_certified") is not False:
        raise ProofError("content clearance audit overstates or fails its mechanical claim")

    review = _exact_object(
        artifact["human_review"],
        {"reviewer", "reviewed_on", "decision", "manifest_sha256", "scope"},
        "content clearance human_review",
    )
    _nonempty(review["reviewer"], "content clearance reviewer")
    _nonempty(review["scope"], "content clearance review scope")
    try:
        reviewed_on = review["reviewed_on"]
        if not isinstance(reviewed_on, str) or date.fromisoformat(reviewed_on).isoformat() != reviewed_on:
            raise ValueError
    except ValueError as exc:
        raise ProofError("content clearance reviewed_on is not a canonical date") from exc
    if review["decision"] != "approved-for-distribution":
        raise ProofError("content clearance is not approved for distribution")
    if review["manifest_sha256"] != actual_report["manifest_sha256"]:
        raise ProofError("content clearance human review is bound to the wrong manifest")

    attribution_relative = _relative(
        artifact["attribution_notice"], "content clearance attribution_notice"
    )
    if attribution_relative not in product["by_category"]["content-license"]:
        raise ProofError("attribution notice is not classified as content-license")
    attribution_path = _regular_file(
        root, _payload_member(product["payload_root"], attribution_relative, "attribution notice")
    )
    try:
        attribution_text = attribution_path.read_text(encoding="utf-8")
        manifest = json.loads(manifest_file.read_bytes())
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise ProofError("content attribution or manifest is unreadable") from exc
    asset_paths = {str(record["path"]) for record in manifest["assets"]}
    license_paths = {str(record["path"]) for record in manifest["licenses"]}
    for index, record in enumerate(manifest["assets"]):
        attribution = str(record["attribution"])
        if attribution not in attribution_text:
            raise ProofError(
                f"attribution notice omits manifest attribution for assets[{index}]"
            )
    if asset_paths != product["by_category"]["independent-content"]:
        raise ProofError("independent-content payload classification does not match manifest assets")
    expected_license_paths = license_paths | {manifest_relative, attribution_relative}
    if expected_license_paths != product["by_category"]["content-license"]:
        raise ProofError("content-license payload classification does not match clearance evidence")
    return {
        "manifest": manifest_path,
        "asset_paths": asset_paths,
        "license_paths": expected_license_paths,
    }


def _lock_identity(record: dict[str, Any]) -> tuple[object, object, object, object]:
    return (
        record.get("name"),
        record.get("version"),
        record.get("source"),
        record.get("checksum"),
    )


def _dependency_reference(value: object, context: str) -> tuple[object, object, object, object]:
    record = _exact_object(value, {"name", "version", "source", "checksum"}, context)
    _nonempty(record["name"], f"{context}.name")
    _nonempty(record["version"], f"{context}.version")
    for field in ("source", "checksum"):
        if record[field] is not None:
            _nonempty(record[field], f"{context}.{field}")
    return _lock_identity(record)


def _validate_product_notices(
    root: Path,
    artifact_path: str,
    artifact: dict[str, Any],
    product: dict[str, Any],
) -> dict[str, Any]:
    artifact = _exact_object(
        artifact,
        {"schema", "payload", "payload_sha256", "locks", "artifacts", "packages"},
        artifact_path,
    )
    if artifact["schema"] != "don.product-dependency-notices.v1":
        raise ProofError(f"product dependency notice schema drift: {artifact_path}")
    if artifact["payload"] != product["artifact_path"]:
        raise ProofError("product dependency notices name the wrong payload")
    if _digest(artifact["payload_sha256"], "product notices payload_sha256") != product[
        "artifact_sha256"
    ]:
        raise ProofError("product dependency notices payload hash drift")

    locks = artifact["locks"]
    if not isinstance(locks, list) or not locks:
        raise ProofError("product dependency notices locks must be non-empty")
    lock_packages: dict[str, set[tuple[object, object, object, object]]] = {}
    for index, raw in enumerate(locks):
        record = _exact_object(raw, {"path", "sha256"}, f"product notices locks[{index}]")
        path = _relative(record["path"], f"product notices locks[{index}].path")
        if path in lock_packages:
            raise ProofError(f"duplicate product notice lock: {path}")
        lock_file = _regular_file(root, path)
        if _digest(record["sha256"], f"product notices lock hash for {path}") != _sha256(lock_file):
            raise ProofError(f"product notice lock hash drift: {path}")
        try:
            parsed = tomllib.loads(lock_file.read_text(encoding="utf-8"))
        except (OSError, UnicodeError, tomllib.TOMLDecodeError) as exc:
            raise ProofError(f"product notice lock is unreadable: {path}") from exc
        packages = parsed.get("package")
        if not isinstance(packages, list):
            raise ProofError(f"product notice lock has no package records: {path}")
        lock_packages[path] = {_lock_identity(package) for package in packages}

    artifact_records = artifact["artifacts"]
    if not isinstance(artifact_records, list) or not artifact_records:
        raise ProofError("product dependency notices artifacts must be non-empty")
    selected: set[tuple[str, tuple[object, object, object, object]]] = set()
    covered_payload_paths: set[str] = set()
    referenced_locks: set[str] = set()
    for index, raw in enumerate(artifact_records):
        record = _exact_object(
            raw, {"payload_path", "lockfile", "packages"}, f"product notices artifacts[{index}]"
        )
        payload_path = _relative(
            record["payload_path"], f"product notices artifacts[{index}].payload_path"
        )
        if payload_path in covered_payload_paths:
            raise ProofError(f"duplicate dependency-bearing payload artifact: {payload_path}")
        if not any(
            payload_path in product["by_category"][category]
            for category in DEPENDENCY_BEARING_CATEGORIES
        ):
            raise ProofError(f"dependency notice covers a non-dependency payload path: {payload_path}")
        covered_payload_paths.add(payload_path)
        lockfile = _relative(record["lockfile"], "product notice artifact lockfile")
        if lockfile not in lock_packages:
            raise ProofError(f"product notice artifact names an undeclared lock: {lockfile}")
        referenced_locks.add(lockfile)
        references = record["packages"]
        if not isinstance(references, list) or not references:
            raise ProofError(f"product notice artifact has no packages: {payload_path}")
        local: set[tuple[object, object, object, object]] = set()
        for package_index, reference in enumerate(references):
            identity = _dependency_reference(
                reference,
                f"product notices artifacts[{index}].packages[{package_index}]",
            )
            if identity in local:
                raise ProofError(f"duplicate package in dependency-bearing artifact: {payload_path}")
            if identity not in lock_packages[lockfile]:
                raise ProofError(f"dependency package is absent from exact lock {lockfile}: {identity}")
            local.add(identity)
            selected.add((lockfile, identity))

    expected_payload_paths = set().union(
        *(product["by_category"][category] for category in DEPENDENCY_BEARING_CATEGORIES)
    )
    if covered_payload_paths != expected_payload_paths:
        raise ProofError("dependency-bearing product payload classification is not fully covered")
    if set(lock_packages) != referenced_locks:
        raise ProofError("product dependency notices contain an unreferenced lock")

    package_records = artifact["packages"]
    if not isinstance(package_records, list) or not package_records:
        raise ProofError("product dependency notice package records must be non-empty")
    recorded: set[tuple[str, tuple[object, object, object, object]]] = set()
    notice_paths: set[str] = set()
    for index, raw in enumerate(package_records):
        record = _exact_object(
            raw,
            {
                "lockfile",
                "name",
                "version",
                "source",
                "checksum",
                "license_expression",
                "license_evidence",
                "required_notice_paths",
                "source_provision",
            },
            f"product notices packages[{index}]",
        )
        lockfile = _relative(record["lockfile"], f"product notices packages[{index}].lockfile")
        identity = _dependency_reference(
            {field: record[field] for field in ("name", "version", "source", "checksum")},
            f"product notices packages[{index}] identity",
        )
        key = (lockfile, identity)
        if key in recorded:
            raise ProofError(f"duplicate product dependency notice package: {key}")
        recorded.add(key)
        _nonempty(record["license_expression"], f"product notices packages[{index}].license_expression")
        _nonempty(record["source_provision"], f"product notices packages[{index}].source_provision")
        for field in ("license_evidence", "required_notice_paths"):
            values = record[field]
            if not isinstance(values, list) or not values:
                raise ProofError(f"product notices packages[{index}].{field} must be non-empty")
            for path_index, value in enumerate(values):
                notice_path = _relative(
                    value, f"product notices packages[{index}].{field}[{path_index}]"
                )
                if notice_path not in product["by_category"]["dependency-notice"]:
                    raise ProofError(f"dependency notice path is not classified as a notice: {notice_path}")
                notice_paths.add(notice_path)
    if recorded != selected:
        raise ProofError("product dependency notice records do not match the conveyed lock subset")
    if notice_paths != product["by_category"]["dependency-notice"]:
        raise ProofError("dependency-notice payload classification does not match package evidence")
    return {
        "covered_payload_paths": covered_payload_paths,
        "notice_paths": notice_paths,
        "packages": len(recorded),
    }


def _require_hash_bound(
    root: Path, path: str, hash_records: dict[str, dict[str, Any]], context: str
) -> Path:
    if path not in hash_records:
        raise ProofError(f"{context} is not hash-bound: {path}")
    file_path = _regular_file(root, path)
    if _sha256(file_path) != hash_records[path].get("sha256"):
        raise ProofError(f"{context} hash drift: {path}")
    return file_path


def _validate_installer_evidence(
    root: Path,
    artifact_path: str,
    artifact: dict[str, Any],
    product: dict[str, Any],
    hash_records: dict[str, dict[str, Any]],
) -> dict[str, Any]:
    artifact = _exact_object(
        artifact,
        {"schema", "payload", "payload_sha256", "installers", "operations"},
        artifact_path,
    )
    if artifact["schema"] != "don.release-installer-evidence.v1":
        raise ProofError(f"installer evidence schema drift: {artifact_path}")
    if artifact["payload"] != product["artifact_path"]:
        raise ProofError("installer evidence names the wrong product payload")
    if _digest(artifact["payload_sha256"], "installer payload_sha256") != product[
        "artifact_sha256"
    ]:
        raise ProofError("installer evidence payload hash drift")
    installers = artifact["installers"]
    if not isinstance(installers, list) or not installers:
        raise ProofError("installer evidence installers must be non-empty")
    installer_paths = {
        _relative(value, f"installer evidence installers[{index}]")
        for index, value in enumerate(installers)
    }
    if len(installer_paths) != len(installers):
        raise ProofError("installer evidence contains duplicate installer paths")
    if installer_paths != product["by_category"]["installer"]:
        raise ProofError("installer payload classification does not match installer evidence")

    operations = artifact["operations"]
    if not isinstance(operations, list) or not operations:
        raise ProofError("installer evidence operations must be non-empty")
    by_platform: dict[str, set[str]] = {}
    reports: set[str] = set()
    expected_results: dict[str, dict[str, Any]] = {
        "install": {"managed_payload_matches": True, "launch_probe_passed": True},
        "configure": {"configuration_roundtrip_matches": True},
        "repair": {
            "tamper_injected": True,
            "tamper_detected": True,
            "managed_payload_matches": True,
        },
        "remove": {"managed_paths_remaining": [], "user_data_policy_matches": True},
    }
    for index, raw in enumerate(operations):
        record = _exact_object(
            raw, {"platform", "action", "report"}, f"installer operations[{index}]"
        )
        platform = _nonempty(record["platform"], f"installer operations[{index}].platform")
        action = record["action"]
        if action not in INSTALLER_ACTIONS:
            raise ProofError(f"unsupported installer action: {action!r}")
        actions = by_platform.setdefault(platform, set())
        if action in actions:
            raise ProofError(f"duplicate installer operation for {platform}: {action}")
        actions.add(str(action))
        report_path = _relative(record["report"], f"installer operations[{index}].report")
        if report_path in reports:
            raise ProofError(f"installer operation report is reused: {report_path}")
        reports.add(report_path)
        report_file = _require_hash_bound(
            root, report_path, hash_records, "installer operation report"
        )
        report = _exact_object(
            _json_object(report_file, f"installer operation report {report_path}"),
            {"schema", "payload_sha256", "platform", "action", "exit_code", "result"},
            f"installer operation report {report_path}",
        )
        if report["schema"] != "don.installer-operation-result.v1":
            raise ProofError(f"installer operation report schema drift: {report_path}")
        if report["payload_sha256"] != product["artifact_sha256"]:
            raise ProofError(f"installer operation report is bound to the wrong payload: {report_path}")
        if report["platform"] != platform or report["action"] != action:
            raise ProofError(f"installer operation report identity drift: {report_path}")
        if report["exit_code"] != 0 or report["result"] != expected_results[str(action)]:
            raise ProofError(f"installer operation did not prove successful {action}: {report_path}")
    for platform, actions in by_platform.items():
        if actions != INSTALLER_ACTIONS:
            raise ProofError(
                f"installer operation coverage is incomplete for {platform}: "
                f"missing={sorted(INSTALLER_ACTIONS - actions)}"
            )
    return {"installer_paths": installer_paths, "platforms": sorted(by_platform)}


def _completion_artifact(
    root: Path,
    gate: dict[str, Any],
    artifact_path: str,
    artifact_schema: str,
    hash_records: dict[str, dict[str, Any]],
) -> dict[str, Any] | None:
    artifact_file = _optional_regular_file(root, artifact_path)
    if artifact_file is None:
        return None
    if artifact_path not in hash_records:
        raise ProofError(f"completion artifact is not hash-bound: {artifact_path}")
    if _sha256(artifact_file) != hash_records[artifact_path].get("sha256"):
        raise ProofError(f"completion artifact hash drift: {artifact_path}")
    artifact = _json_object(artifact_file, f"completion artifact {artifact_path}")
    if artifact.get("schema") != artifact_schema:
        raise ProofError(
            f"completion artifact schema drift for {artifact_path}: expected {artifact_schema}"
        )
    if artifact_path not in gate["evidence"]:
        raise ProofError(f"completion artifact is absent from gate evidence: {artifact_path}")
    return artifact


def _validate_distribution_artifacts(
    root: Path,
    by_id: dict[str, dict[str, Any]],
    hash_records: dict[str, dict[str, Any]],
) -> dict[str, bool]:
    paths = {
        gate_id: REQUIRED_COMPLETION_ARTIFACTS[gate_id]
        for gate_id in (
            "assembled-product-packaging",
            "binary-third-party-notices",
            "independent-presentation-content",
            "standalone-product-installer",
        )
    }
    loaded = {
        gate_id: _completion_artifact(
            root, by_id[gate_id], artifact_path, schema, hash_records
        )
        for gate_id, (artifact_path, schema) in paths.items()
    }
    product_artifact = loaded["assembled-product-packaging"]
    dependent_present = any(
        loaded[gate_id] is not None
        for gate_id in (
            "binary-third-party-notices",
            "independent-presentation-content",
            "standalone-product-installer",
        )
    )
    if product_artifact is None:
        if dependent_present:
            raise ProofError("distribution completion evidence exists without product-payload.json")
        return {gate_id: False for gate_id in paths}

    product_path = paths["assembled-product-packaging"][0]
    product = _validate_product_payload(root, product_path, product_artifact)
    completed = {
        "binary-third-party-notices": False,
        "independent-presentation-content": False,
        "standalone-product-installer": False,
    }
    if loaded["binary-third-party-notices"] is not None:
        _validate_product_notices(
            root,
            paths["binary-third-party-notices"][0],
            loaded["binary-third-party-notices"],
            product,
        )
        completed["binary-third-party-notices"] = True
    if loaded["independent-presentation-content"] is not None:
        _validate_content_clearance(
            root,
            paths["independent-presentation-content"][0],
            loaded["independent-presentation-content"],
            product,
        )
        completed["independent-presentation-content"] = True
    if loaded["standalone-product-installer"] is not None:
        _validate_installer_evidence(
            root,
            paths["standalone-product-installer"][0],
            loaded["standalone-product-installer"],
            product,
            hash_records,
        )
        completed["standalone-product-installer"] = True
    completed["assembled-product-packaging"] = all(completed.values())
    if completed["assembled-product-packaging"]:
        required_evidence = {artifact_path for artifact_path, _ in paths.values()}
        missing = required_evidence - set(by_id["assembled-product-packaging"]["evidence"])
        if missing:
            raise ProofError(
                f"assembled-product-packaging omits distribution proof artifacts: {sorted(missing)}"
            )
    return completed


def _validate_retail_control_artifacts(
    root: Path,
    by_id: dict[str, dict[str, Any]],
    hash_records: dict[str, dict[str, Any]],
) -> dict[str, bool]:
    paths = {
        gate_id: REQUIRED_COMPLETION_ARTIFACTS[gate_id]
        for gate_id in (
            "retail-controller-byte-restoration",
            "controller-stop-incident-closure",
        )
    }
    loaded = {
        gate_id: _completion_artifact(
            root, by_id[gate_id], artifact_path, schema, hash_records
        )
        for gate_id, (artifact_path, schema) in paths.items()
    }
    lifecycle = loaded["retail-controller-byte-restoration"]
    closure = loaded["controller-stop-incident-closure"]
    if closure is not None and lifecycle is None:
        raise ProofError("controller incident closure exists without lifecycle proof")

    verifier = _load_retail_control_evidence_verifier()
    try:
        if lifecycle is not None:
            verifier.verify_lifecycle(
                root, root / paths["retail-controller-byte-restoration"][0]
            )
        if closure is not None:
            verifier.verify_closure(root, root / paths["controller-stop-incident-closure"][0])
    except verifier.EvidenceError as exc:
        raise ProofError(f"retail-control completion evidence failed: {exc}") from exc
    return {
        "retail-controller-byte-restoration": lifecycle is not None,
        "controller-stop-incident-closure": closure is not None,
    }


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
    archive_reproducibility: dict[str, Any],
    hash_records: dict[str, dict[str, Any]],
) -> tuple[
    dict[str, list[str]],
    dict[str, bool],
    dict[str, list[dict[str, str | None]]],
]:
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
        "source-archive-reproducibility": (
            "proved" if archive_reproducibility["candidate_reproduced"] else "blocked"
        ),
    }
    for gate_id, expected_status in derived.items():
        if gate_id not in by_id:
            raise ProofError(f"required derived gate is absent: {gate_id}")
        if by_id[gate_id]["status"] != expected_status:
            raise ProofError(
                f"gate {gate_id} contradicts derived evidence: "
                f"expected {expected_status}, found {by_id[gate_id]['status']}"
            )
    distribution_completion = _validate_distribution_artifacts(root, by_id, hash_records)
    for gate_id, complete in distribution_completion.items():
        expected_status = "proved" if complete else "blocked"
        if by_id[gate_id]["status"] != expected_status:
            raise ProofError(
                f"gate {gate_id} contradicts validated distribution artifacts: "
                f"expected {expected_status}, found {by_id[gate_id]['status']}"
            )

    retail_control_completion = _validate_retail_control_artifacts(
        root, by_id, hash_records
    )
    for gate_id, complete in retail_control_completion.items():
        expected_status = "proved" if complete else "blocked"
        if by_id[gate_id]["status"] != expected_status:
            raise ProofError(
                f"gate {gate_id} contradicts validated retail-control artifacts: "
                f"expected {expected_status}, found {by_id[gate_id]['status']}"
            )

    semantically_validated_gate_ids = set(distribution_completion) | set(
        retail_control_completion
    )
    for gate_id, (artifact_path, artifact_schema) in REQUIRED_COMPLETION_ARTIFACTS.items():
        if gate_id in semantically_validated_gate_ids:
            continue
        artifact = _completion_artifact(
            root, by_id[gate_id], artifact_path, artifact_schema, hash_records
        )
        if artifact is not None:
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
    blocker_details: dict[str, list[dict[str, str | None]]] = {
        scope: [
            {
                "id": gate_id,
                "reason": str(by_id[gate_id]["blocker"]),
                "expected_artifact": (
                    REQUIRED_COMPLETION_ARTIFACTS[gate_id][0]
                    if gate_id in REQUIRED_COMPLETION_ARTIFACTS
                    else None
                ),
            }
            for gate_id in blockers[scope]
        ]
        for scope in sorted(SCOPES)
    }
    return blockers, readiness, blocker_details


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
    archive_reproducibility = _validate_archive_reproducibility(root)
    incident_dump_retained = _incident_dump_retained(root)
    documentation_conflicts = _known_documentation_conflicts(root)
    blockers, readiness, blocker_details = _validate_gates(
        root,
        payload,
        missing_license_texts,
        template_license_mismatches,
        documentation_conflicts,
        archive_reproducibility,
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
        "source_archive_reproducibility": archive_reproducibility,
        "recorded_incident_dump_retained": incident_dump_retained,
        "documentation_conflicts": documentation_conflicts,
        "readiness": readiness,
        "blockers": blockers,
        "blocker_details": blocker_details,
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
