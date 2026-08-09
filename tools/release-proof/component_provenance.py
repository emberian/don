#!/usr/bin/env python3
"""Capture and verify a fail-closed provenance record for one Cargo-built component.

This is intentionally narrower than ``product-dependency-notices.json``.  It proves
the byte identity of one component, the exact lock graph selected from a named root,
and the license declarations/texts mechanically recovered from checksum-matching
registry archives.  It does not decide which obligations apply to a binary, certify
notice sufficiency, or represent a complete product payload.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import tarfile
import tempfile
import tomllib
import urllib.request
from pathlib import Path, PurePosixPath
from typing import Any


SCHEMA = "don.release-component-provenance.v1"
REGISTRY_PREFIX = "registry+https://github.com/rust-lang/crates.io-index"
DEPENDENCY_RE = re.compile(r"^(\S+)(?:\s+(\S+)(?:\s+\((.+)\))?)?$")
NOTICE_PREFIXES = ("copying", "license", "notice", "unlicense")


class ProvenanceError(RuntimeError):
    """The requested capture or verification is not exact."""


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def canonical_relative(value: Any, context: str) -> PurePosixPath:
    if not isinstance(value, str) or not value or "\\" in value or "\0" in value:
        raise ProvenanceError(f"{context} is not a canonical repository path")
    relative = PurePosixPath(value)
    if relative.is_absolute() or any(part in {"", ".", ".."} for part in relative.parts):
        raise ProvenanceError(f"{context} escapes the repository root")
    if relative.as_posix() != value:
        raise ProvenanceError(f"{context} is not a canonical repository path")
    return relative


def file_record(root: Path, path: Path) -> dict[str, Any]:
    try:
        relative = path.relative_to(root).as_posix()
    except ValueError as exc:
        raise ProvenanceError(f"evidence is outside the repository: {path}") from exc
    canonical = canonical_relative(relative, "evidence path")
    candidate = root
    for part in canonical.parts:
        candidate /= part
        if candidate.is_symlink():
            raise ProvenanceError(f"evidence path contains a symlink: {relative}")
    if not candidate.is_file():
        raise ProvenanceError(f"evidence is not a regular file: {relative}")
    return {"path": relative, "size": candidate.stat().st_size, "sha256": sha256(candidate)}


def load_toml(path: Path) -> dict[str, Any]:
    try:
        return tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, tomllib.TOMLDecodeError) as exc:
        raise ProvenanceError(f"cannot read TOML: {path}") from exc


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_bytes())
    except (OSError, json.JSONDecodeError) as exc:
        raise ProvenanceError(f"cannot read JSON: {path}") from exc
    if not isinstance(value, dict):
        raise ProvenanceError(f"JSON root is not an object: {path}")
    return value


def identity(package: dict[str, Any]) -> tuple[Any, Any, Any, Any]:
    return (
        package.get("name"),
        package.get("version"),
        package.get("source"),
        package.get("checksum"),
    )


def identity_record(package: dict[str, Any]) -> dict[str, Any]:
    return {
        "name": package.get("name"),
        "version": package.get("version"),
        "source": package.get("source"),
        "checksum": package.get("checksum"),
    }


def resolve_dependency(packages: list[dict[str, Any]], dependency: str) -> dict[str, Any]:
    match = DEPENDENCY_RE.fullmatch(dependency)
    if match is None:
        raise ProvenanceError(f"unsupported Cargo.lock dependency identity: {dependency!r}")
    name, version, source = match.groups()
    matches = [package for package in packages if package.get("name") == name]
    if version is not None:
        matches = [package for package in matches if package.get("version") == version]
    if source is not None:
        matches = [package for package in matches if package.get("source") == source]
    if len(matches) != 1:
        raise ProvenanceError(
            f"dependency identity is not unique in the exact lock: {dependency!r}"
        )
    return matches[0]


def selected_graph(lock: dict[str, Any], root_package: str) -> list[dict[str, Any]]:
    packages = lock.get("package")
    if not isinstance(packages, list) or not all(isinstance(item, dict) for item in packages):
        raise ProvenanceError("Cargo.lock has no package array")
    roots = [package for package in packages if package.get("name") == root_package]
    if len(roots) != 1:
        raise ProvenanceError(f"root package is not unique in Cargo.lock: {root_package}")
    pending = [roots[0]]
    selected: dict[tuple[Any, Any, Any, Any], dict[str, Any]] = {}
    while pending:
        package = pending.pop()
        key = identity(package)
        if key in selected:
            continue
        selected[key] = package
        dependencies = package.get("dependencies", [])
        if not isinstance(dependencies, list) or not all(
            isinstance(item, str) for item in dependencies
        ):
            raise ProvenanceError(f"invalid dependencies for {key}")
        pending.extend(resolve_dependency(packages, item) for item in dependencies)
    return sorted(
        selected.values(),
        key=lambda item: (
            str(item.get("name")),
            str(item.get("version")),
            str(item.get("source") or ""),
        ),
    )


def safe_members(archive: tarfile.TarFile, prefix: str) -> list[tarfile.TarInfo]:
    members: list[tarfile.TarInfo] = []
    expected = PurePosixPath(prefix)
    for member in archive.getmembers():
        path = PurePosixPath(member.name)
        if path.is_absolute() or ".." in path.parts or not path.parts or path.parts[0] != prefix:
            raise ProvenanceError(f"unsafe or unexpected registry archive member: {member.name}")
        if member.issym() or member.islnk():
            raise ProvenanceError(f"registry archive contains a link: {member.name}")
        if member.isfile() and path.parent == expected:
            members.append(member)
    return members


def archive_path(crate_dir: Path, package: dict[str, Any]) -> Path:
    return crate_dir / f"{package['name']}-{package['version']}.crate"


def registry_url(package: dict[str, Any]) -> str:
    return (
        "https://static.crates.io/crates/"
        f"{package['name']}/{package['name']}-{package['version']}.crate"
    )


def fetch_registry_archive(crate_dir: Path, package: dict[str, Any]) -> Path:
    destination = archive_path(crate_dir, package)
    destination.parent.mkdir(parents=True, exist_ok=True)
    if not destination.exists():
        request = urllib.request.Request(
            registry_url(package), headers={"User-Agent": "don-release-proof/1.0"}
        )
        try:
            with urllib.request.urlopen(request) as response, destination.open("wb") as output:
                shutil.copyfileobj(response, output)
        except OSError as exc:
            destination.unlink(missing_ok=True)
            raise ProvenanceError(f"cannot fetch exact registry archive: {destination.name}") from exc
    expected = package.get("checksum")
    if not isinstance(expected, str) or sha256(destination) != expected:
        raise ProvenanceError(f"registry archive checksum mismatch: {destination.name}")
    return destination


def extract_registry_evidence(
    root: Path,
    package: dict[str, Any],
    crate_path: Path,
    evidence_root: Path,
) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    prefix = f"{package['name']}-{package['version']}"
    destination = evidence_root / prefix
    destination.mkdir(parents=True, exist_ok=True)
    with tarfile.open(crate_path, "r:gz") as archive:
        members = safe_members(archive, prefix)
        by_name = {PurePosixPath(member.name).name: member for member in members}
        manifest_member = by_name.get("Cargo.toml.orig") or by_name.get("Cargo.toml")
        if manifest_member is None:
            raise ProvenanceError(f"registry archive has no package manifest: {prefix}")
        manifest_bytes = archive.extractfile(manifest_member).read()  # type: ignore[union-attr]
        try:
            manifest = tomllib.loads(manifest_bytes.decode("utf-8"))
            declared = manifest["package"]["license"]
        except (UnicodeError, tomllib.TOMLDecodeError, KeyError, TypeError) as exc:
            raise ProvenanceError(f"registry manifest has no declared license: {prefix}") from exc
        if not isinstance(declared, str) or not declared.strip():
            raise ProvenanceError(f"registry manifest license is empty: {prefix}")
        manifest_output = destination / "Cargo.toml.orig"
        manifest_output.write_bytes(manifest_bytes)
        notice_members = sorted(
            (
                member
                for member in members
                if PurePosixPath(member.name).name.casefold().startswith(NOTICE_PREFIXES)
            ),
            key=lambda member: member.name.casefold(),
        )
        if not notice_members:
            raise ProvenanceError(f"registry archive contains no root license/notice text: {prefix}")
        notices: list[dict[str, Any]] = []
        for member in notice_members:
            name = PurePosixPath(member.name).name
            output = destination / name
            output.write_bytes(archive.extractfile(member).read())  # type: ignore[union-attr]
            notices.append(file_record(root, output))
    return (
        {
            "declared_expression": declared,
            "declaration_evidence": file_record(root, manifest_output),
        },
        notices,
    )


def manifest_license(root: Path, manifest_path: Path) -> tuple[str, list[dict[str, Any]]]:
    manifest = load_toml(manifest_path)
    package = manifest.get("package")
    if not isinstance(package, dict):
        raise ProvenanceError(f"package manifest has no package table: {manifest_path}")
    declared = package.get("license")
    evidence = [file_record(root, manifest_path)]
    if isinstance(declared, dict) and declared.get("workspace") is True:
        workspace_manifest = root / "Cargo.toml"
        workspace = load_toml(workspace_manifest)
        try:
            declared = workspace["workspace"]["package"]["license"]
        except (KeyError, TypeError) as exc:
            raise ProvenanceError("workspace license declaration is absent") from exc
        evidence.append(file_record(root, workspace_manifest))
    if not isinstance(declared, str) or not declared.strip():
        raise ProvenanceError(f"package license declaration is absent: {manifest_path}")
    evidence.append(file_record(root, root / "LICENSE"))
    return declared, evidence


def local_manifest_map(root: Path) -> dict[tuple[str, str], Path]:
    result: dict[tuple[str, str], Path] = {}
    paths: list[Path] = []
    for directory, children, files in os.walk(root):
        children[:] = sorted(
            child
            for child in children
            if child not in {".git", "target", "node_modules", "ron-data"}
        )
        if "Cargo.toml" in files:
            paths.append(Path(directory) / "Cargo.toml")
    for path in sorted(paths):
        manifest = load_toml(path)
        package = manifest.get("package")
        if not isinstance(package, dict):
            continue
        name, version = package.get("name"), package.get("version")
        if isinstance(name, str) and isinstance(version, str):
            key = (name, version)
            if key in result:
                # A selected package is resolved below; unrelated duplicate tool manifests
                # must not make repository discovery nondeterministic.
                result[key] = Path("")
            else:
                result[key] = path
    return result


def capture(
    root: Path,
    output: Path,
    crate_dir: Path,
    evidence_root: Path,
    component: str,
    version: str,
    root_package: str,
    payload_path: Path,
    lock_path: Path,
) -> dict[str, Any]:
    lock = load_toml(lock_path)
    selected = selected_graph(lock, root_package)
    local = local_manifest_map(root)
    package_records: list[dict[str, Any]] = []
    for package in selected:
        record: dict[str, Any] = {"identity": identity_record(package)}
        source = package.get("source")
        if source is None:
            manifest = local.get((str(package["name"]), str(package["version"])))
            if manifest is None or manifest == Path(""):
                raise ProvenanceError(f"local selected package manifest is not unique: {identity(package)}")
            declared, declaration_evidence = manifest_license(root, manifest)
            record.update(
                {
                    "origin": "repository-path",
                    "archive": None,
                    "license": {
                        "declared_expression": declared,
                        "declaration_evidence": declaration_evidence,
                        "bundled_texts": [file_record(root, root / "LICENSE")],
                        "review_state": "pending-human-obligation-review",
                    },
                }
            )
        elif source == REGISTRY_PREFIX:
            crate_path = fetch_registry_archive(crate_dir, package)
            license_record, notices = extract_registry_evidence(
                root, package, crate_path, evidence_root
            )
            record.update(
                {
                    "origin": "crates.io-registry",
                    "archive": {
                        "url": registry_url(package),
                        "sha256": sha256(crate_path),
                    },
                    "license": {
                        **license_record,
                        "bundled_texts": notices,
                        "review_state": "pending-human-obligation-review",
                    },
                }
            )
        else:
            raise ProvenanceError(f"unsupported selected package source: {source}")
        package_records.append(record)

    artifact = {
        "schema": SCHEMA,
        "component": {"name": component, "version": version},
        "payload": file_record(root, payload_path),
        "lock": file_record(root, lock_path),
        "selection": {
            "root_package": root_package,
            "method": "transitive package graph reachable from root_package in the exact Cargo.lock",
            "packages": package_records,
        },
        "claims": {
            "component_bytes_hash_bound": True,
            "lock_graph_hash_bound": True,
            "registry_archive_checksums_verified": True,
            "license_declarations_mechanically_captured": True,
            "binary_dependency_reachability_proven": False,
            "notice_sufficiency_reviewed": False,
            "source_provision_reviewed": False,
            "whole_product_payload": False,
        },
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(artifact, indent=2) + "\n", encoding="utf-8")
    return artifact


def verify_file(root: Path, record: dict[str, Any], context: str) -> Path:
    if set(record) != {"path", "size", "sha256"}:
        raise ProvenanceError(f"invalid file record: {context}")
    relative = canonical_relative(record["path"], context)
    path = root
    for part in relative.parts:
        path /= part
        if path.is_symlink():
            raise ProvenanceError(f"evidence path contains a symlink: {relative}")
    if not path.is_file():
        raise ProvenanceError(f"missing regular evidence file: {record['path']}")
    if path.stat().st_size != record["size"] or sha256(path) != record["sha256"]:
        raise ProvenanceError(f"evidence bytes drift: {record['path']}")
    return path


def verify(root: Path, artifact_path: Path) -> dict[str, Any]:
    artifact = load_json(artifact_path)
    if artifact.get("schema") != SCHEMA:
        raise ProvenanceError("component provenance schema drift")
    expected_claims = {
        "component_bytes_hash_bound": True,
        "lock_graph_hash_bound": True,
        "registry_archive_checksums_verified": True,
        "license_declarations_mechanically_captured": True,
        "binary_dependency_reachability_proven": False,
        "notice_sufficiency_reviewed": False,
        "source_provision_reviewed": False,
        "whole_product_payload": False,
    }
    if artifact.get("claims") != expected_claims:
        raise ProvenanceError("component provenance claims drift or overstate evidence")
    verify_file(root, artifact["payload"], "payload")
    lock_path = verify_file(root, artifact["lock"], "lock")
    lock = load_toml(lock_path)
    selection = artifact.get("selection")
    if not isinstance(selection, dict) or set(selection) != {"root_package", "method", "packages"}:
        raise ProvenanceError("component provenance selection fields are invalid")
    selected = selected_graph(lock, str(selection["root_package"]))
    records = selection["packages"]
    if not isinstance(records, list) or len(records) != len(selected):
        raise ProvenanceError("component provenance package coverage drift")
    for index, (package, record) in enumerate(zip(selected, records, strict=True)):
        if not isinstance(record, dict) or record.get("identity") != identity_record(package):
            raise ProvenanceError(f"component provenance package identity drift at {index}")
        license_record = record.get("license")
        if not isinstance(license_record, dict) or license_record.get("review_state") != (
            "pending-human-obligation-review"
        ):
            raise ProvenanceError(f"component provenance review state drift at {index}")
        evidence = license_record.get("declaration_evidence")
        if isinstance(evidence, dict):
            declaration_paths = [verify_file(root, evidence, f"declaration {index}")]
        elif isinstance(evidence, list) and evidence:
            declaration_paths = [
                verify_file(root, item, f"declaration {index}") for item in evidence
            ]
        else:
            raise ProvenanceError(f"missing declaration evidence at {index}")
        texts = license_record.get("bundled_texts")
        if not isinstance(texts, list) or not texts:
            raise ProvenanceError(f"missing bundled license text at {index}")
        for item in texts:
            verify_file(root, item, f"license text {index}")
        declared = license_record.get("declared_expression")
        if record.get("origin") == "crates.io-registry":
            manifest = load_toml(declaration_paths[0])
            if manifest.get("package", {}).get("license") != declared:
                raise ProvenanceError(f"registry license declaration drift at {index}")
            archive = record.get("archive")
            if not isinstance(archive, dict) or archive.get("sha256") != package.get("checksum"):
                raise ProvenanceError(f"registry archive is not lock-checksum-bound at {index}")
        elif record.get("origin") == "repository-path":
            if record.get("archive") is not None:
                raise ProvenanceError(f"repository package unexpectedly names an archive at {index}")
            actual, _ = manifest_license(root, declaration_paths[0])
            if actual != declared:
                raise ProvenanceError(f"repository license declaration drift at {index}")
        else:
            raise ProvenanceError(f"unsupported package origin at {index}")
    return {
        "schema": "don.release-component-provenance-check.v1",
        "ok": True,
        "component": artifact["component"],
        "packages": len(records),
        "registry_packages": sum(
            record.get("origin") == "crates.io-registry" for record in records
        ),
        "pending_human_review": len(records),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    subparsers = parser.add_subparsers(dest="command", required=True)
    capture_parser = subparsers.add_parser("capture")
    capture_parser.add_argument("--output", type=Path, required=True)
    capture_parser.add_argument("--crate-dir", type=Path)
    capture_parser.add_argument("--evidence-root", type=Path, required=True)
    capture_parser.add_argument("--component", required=True)
    capture_parser.add_argument("--version", required=True)
    capture_parser.add_argument("--root-package", required=True)
    capture_parser.add_argument("--payload", type=Path, required=True)
    capture_parser.add_argument("--lock", type=Path, required=True)
    verify_parser = subparsers.add_parser("verify")
    verify_parser.add_argument("--artifact", type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve(strict=True)
    try:
        if args.command == "capture":
            crate_dir = args.crate_dir
            temporary: tempfile.TemporaryDirectory[str] | None = None
            if crate_dir is None:
                temporary = tempfile.TemporaryDirectory(prefix="don-crate-capture-")
                crate_dir = Path(temporary.name)
            capture(
                root,
                root / args.output,
                crate_dir,
                root / args.evidence_root,
                args.component,
                args.version,
                args.root_package,
                root / args.payload,
                root / args.lock,
            )
            if temporary is not None:
                temporary.cleanup()
            result = verify(root, root / args.output)
        else:
            result = verify(root, root / args.artifact)
    except ProvenanceError as exc:
        print(json.dumps({"schema": "don.release-component-provenance-error.v1", "ok": False, "error": str(exc)}))
        return 1
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
