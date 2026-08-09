#!/usr/bin/env python3
"""Audit the mechanical licensing record for independent presentation content.

This is deliberately narrower than legal review.  It proves that every regular file under
the declared asset roots is named by one exact manifest entry, that its bytes match that
entry, and that the entry carries the provenance fields required by DoN's release policy.
It cannot prove that an author owned the rights they claimed or that a source URL is truthful.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import stat
import sys
import unicodedata
from urllib.parse import urlsplit


SCHEMA = "don.independent-content.v1"
REPORT_SCHEMA = "don.content-license-audit.v1"
MIN_LICENSE_TEXT_BYTES = 256

# This is a release-policy allowlist, not a claim that every possible use of these licenses is
# automatically compatible.  New identifiers require an explicit policy change and review.
ALLOWED_LICENSES = frozenset(
    {
        "Apache-2.0",
        "CC-BY-4.0",
        "CC0-1.0",
        "GPL-3.0-or-later",
        "MIT",
        "OFL-1.1",
    }
)

KIND_EXTENSIONS = {
    "art": frozenset({".basis", ".jpeg", ".jpg", ".ktx2", ".png", ".svg", ".webp"}),
    "audio": frozenset({".flac", ".ogg", ".opus", ".wav"}),
    "font": frozenset({".otf", ".ttf", ".woff", ".woff2"}),
    "model": frozenset({".glb", ".gltf"}),
    "video": frozenset({".mp4", ".webm"}),
}

FORBIDDEN_MAGIC = (
    (b"MZ", "Windows PE executable"),
    (b"\x7fELF", "ELF executable"),
    (b"\xfe\xed\xfa\xce", "Mach-O executable"),
    (b"\xce\xfa\xed\xfe", "Mach-O executable"),
    (b"\xfe\xed\xfa\xcf", "Mach-O executable"),
    (b"\xcf\xfa\xed\xfe", "Mach-O executable"),
    (b"\xca\xfe\xba\xbe", "Mach-O universal binary"),
    (b"\xbe\xba\xfe\xca", "Mach-O universal binary"),
    (b"\x00asm", "WebAssembly executable"),
    (b"#!", "executable script"),
    (b"PK\x03\x04", "ZIP archive"),
    (b"Microsoft C/C++ MSF 7.00", "Microsoft PDB/MSF"),
    (b"DONPACK2", "retail-derived DONPACK2 pack"),
    (b"DONPLAY1", "retail-derived DONPLAY1 pack"),
)

WINDOWS_RESERVED_STEMS = frozenset(
    {"CON", "PRN", "AUX", "NUL", "CLOCK$"}
    | {f"COM{index}" for index in range(1, 10)}
    | {f"LPT{index}" for index in range(1, 10)}
)


class AuditError(RuntimeError):
    """A fail-closed manifest or release-tree finding."""


def _object_without_duplicate_keys(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise AuditError(f"manifest contains duplicate JSON key {key!r}")
        result[key] = value
    return result


def _read_manifest(path: Path) -> tuple[dict[str, object], str]:
    try:
        raw = path.read_bytes()
    except OSError as exc:
        raise AuditError(f"cannot read manifest: {exc}") from exc
    try:
        payload = json.loads(raw.decode("utf-8"), object_pairs_hook=_object_without_duplicate_keys)
    except UnicodeDecodeError as exc:
        raise AuditError("manifest is not UTF-8") from exc
    except json.JSONDecodeError as exc:
        raise AuditError(f"manifest is not valid JSON: {exc}") from exc
    if not isinstance(payload, dict):
        raise AuditError("manifest root must be an object")
    return payload, hashlib.sha256(raw).hexdigest()


def _exact_object(value: object, fields: set[str], context: str) -> dict[str, object]:
    if not isinstance(value, dict):
        raise AuditError(f"{context} must be an object")
    actual = set(value)
    if actual != fields:
        missing = sorted(fields - actual)
        unknown = sorted(actual - fields)
        detail = []
        if missing:
            detail.append(f"missing {', '.join(missing)}")
        if unknown:
            detail.append(f"unknown {', '.join(unknown)}")
        raise AuditError(f"{context} fields are invalid ({'; '.join(detail)})")
    return value


def _nonempty_string(value: object, context: str) -> str:
    if not isinstance(value, str) or not value.strip() or value != value.strip():
        raise AuditError(f"{context} must be a non-empty, surrounding-whitespace-free string")
    for char in value:
        codepoint = ord(char)
        if unicodedata.category(char) in {"Cc", "Cs"}:
            raise AuditError(f"{context} contains an invalid control or surrogate character")
        if 0xFDD0 <= codepoint <= 0xFDEF or codepoint & 0xFFFF in {0xFFFE, 0xFFFF}:
            raise AuditError(f"{context} contains an invalid Unicode noncharacter")
    return value


def _canonical_relative(value: object, context: str) -> PurePosixPath:
    text = _nonempty_string(value, context)
    if "\\" in text or "\0" in text:
        raise AuditError(f"{context} must be a canonical POSIX path")
    relative = PurePosixPath(text)
    if relative.is_absolute() or any(part in {"", ".", ".."} for part in relative.parts):
        raise AuditError(f"{context} must stay beneath the release root")
    if relative.as_posix() != text:
        raise AuditError(f"{context} must be a canonical POSIX path")
    for part in relative.parts:
        if unicodedata.normalize("NFC", part) != part:
            raise AuditError(f"{context} contains a non-NFC path component")
        if any(char in '<>:"|?*' for char in part) or part.endswith((" ", ".")):
            raise AuditError(f"{context} is not portable to a Windows release target")
        if part.split(".", 1)[0].upper() in WINDOWS_RESERVED_STEMS:
            raise AuditError(f"{context} uses a reserved Windows filename")
    return relative


def _positive_size(value: object, context: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
        raise AuditError(f"{context} must be a positive integer")
    return value


def _license_size(value: object, context: str) -> int:
    size = _positive_size(value, context)
    if size < MIN_LICENSE_TEXT_BYTES:
        raise AuditError(
            f"{context} must be at least {MIN_LICENSE_TEXT_BYTES} bytes; "
            "short labels are not included license texts"
        )
    return size


def _sha256(value: object, context: str) -> str:
    if (
        not isinstance(value, str)
        or len(value) != 64
        or value != value.lower()
        or any(char not in "0123456789abcdef" for char in value)
    ):
        raise AuditError(f"{context} must be a lowercase SHA-256 digest")
    return value


def _https_url(value: object, context: str) -> str:
    text = _nonempty_string(value, context)
    try:
        parsed = urlsplit(text)
        parsed.port
    except ValueError as exc:
        raise AuditError(f"{context} is not a valid HTTPS URL: {exc}") from exc
    if (
        parsed.scheme != "https"
        or not parsed.hostname
        or parsed.username
        or parsed.password
        or "\\" in text
        or any(char.isspace() for char in text)
    ):
        raise AuditError(f"{context} must be an HTTPS URL without embedded credentials")
    return text


def _list(value: object, context: str, *, nonempty: bool = True) -> list[object]:
    if not isinstance(value, list) or (nonempty and not value):
        qualifier = "non-empty " if nonempty else ""
        raise AuditError(f"{context} must be a {qualifier}array")
    return value


def _under(path: PurePosixPath, root: PurePosixPath) -> bool:
    path_parts = tuple(part.casefold() for part in path.parts)
    root_parts = tuple(part.casefold() for part in root.parts)
    return path_parts[: len(root_parts)] == root_parts


def _reject_casefold_duplicates(paths: list[PurePosixPath], context: str) -> None:
    seen: dict[str, PurePosixPath] = {}
    for path in paths:
        key = path.as_posix().casefold()
        if key in seen:
            raise AuditError(
                f"{context} contains a case-insensitive collision: {seen[key]} and {path}"
            )
        seen[key] = path


def _resolved_release_root(root: Path) -> Path:
    if root.is_symlink():
        raise AuditError("release root itself must not be a symbolic link")
    try:
        resolved = root.resolve(strict=True)
    except (OSError, RuntimeError, UnicodeError) as exc:
        raise AuditError(f"release root is unavailable: {exc}") from exc
    if not resolved.is_dir():
        raise AuditError("release root is not a directory")
    return resolved


def _descendant(root: Path, relative: PurePosixPath, context: str) -> Path:
    current = root
    for part in relative.parts:
        current = current / part
        try:
            mode = current.lstat().st_mode
        except (OSError, UnicodeError) as exc:
            raise AuditError(f"{context} is unavailable: {relative}: {exc}") from exc
        if stat.S_ISLNK(mode):
            raise AuditError(f"{context} contains a symbolic link: {relative}")
    try:
        current.resolve(strict=True).relative_to(root)
    except (OSError, RuntimeError, UnicodeError, ValueError) as exc:
        raise AuditError(f"{context} escapes the release root: {relative}") from exc
    return current


def _regular_file(root: Path, relative: PurePosixPath, context: str) -> Path:
    path = _descendant(root, relative, context)
    try:
        mode = path.stat(follow_symlinks=False).st_mode
    except OSError as exc:
        raise AuditError(f"cannot stat {context}: {relative}: {exc}") from exc
    if not stat.S_ISREG(mode):
        raise AuditError(f"{context} is not a regular file: {relative}")
    if mode & (stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH):
        raise AuditError(f"{context} has executable mode bits: {relative}")
    return path


def _hash_file(path: Path) -> tuple[int, str, bytes]:
    digest = hashlib.sha256()
    size = 0
    prefix = bytearray()
    try:
        with path.open("rb") as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                if len(prefix) < 32:
                    prefix.extend(chunk[: 32 - len(prefix)])
                size += len(chunk)
                digest.update(chunk)
    except OSError as exc:
        raise AuditError(f"cannot read {path}: {exc}") from exc
    return size, digest.hexdigest(), bytes(prefix)


def _scan_asset_root(release_root: Path, relative: PurePosixPath) -> list[PurePosixPath]:
    root = _descendant(release_root, relative, "asset root")
    try:
        mode = root.stat(follow_symlinks=False).st_mode
    except OSError as exc:
        raise AuditError(f"cannot stat asset root {relative}: {exc}") from exc
    if not stat.S_ISDIR(mode):
        raise AuditError(f"asset root is not a directory: {relative}")

    files: list[PurePosixPath] = []

    def visit(directory: Path, rel_dir: PurePosixPath) -> None:
        try:
            entries = sorted(os.scandir(directory), key=lambda entry: entry.name.casefold())
        except OSError as exc:
            raise AuditError(f"cannot enumerate asset directory {rel_dir}: {exc}") from exc
        sibling_names: dict[str, str] = {}
        for entry in entries:
            rel = _canonical_relative(
                (rel_dir / entry.name).as_posix(), "asset tree path"
            )
            folded = entry.name.casefold()
            if folded in sibling_names:
                raise AuditError(
                    f"asset tree contains a case-insensitive sibling collision: "
                    f"{rel_dir / sibling_names[folded]} and {rel}"
                )
            sibling_names[folded] = entry.name
            try:
                if entry.is_symlink():
                    raise AuditError(f"asset tree contains a symbolic link: {rel}")
                if entry.is_dir(follow_symlinks=False):
                    visit(Path(entry.path), rel)
                elif entry.is_file(follow_symlinks=False):
                    files.append(rel)
                else:
                    raise AuditError(f"asset tree contains a special filesystem object: {rel}")
            except OSError as exc:
                raise AuditError(f"cannot inspect asset path {rel}: {exc}") from exc

    visit(root, relative)
    return files


def _validate_manifest_shape(payload: dict[str, object]) -> tuple[
    list[PurePosixPath], list[dict[str, object]], list[dict[str, object]]
]:
    payload = _exact_object(
        payload, {"schema", "package", "asset_roots", "licenses", "assets"}, "manifest"
    )
    if payload["schema"] != SCHEMA:
        raise AuditError(f"unsupported manifest schema {payload['schema']!r}")
    package = _exact_object(payload["package"], {"name", "version"}, "package")
    _nonempty_string(package["name"], "package.name")
    _nonempty_string(package["version"], "package.version")

    asset_roots = [
        _canonical_relative(item, f"asset_roots[{index}]")
        for index, item in enumerate(_list(payload["asset_roots"], "asset_roots"))
    ]
    _reject_casefold_duplicates(asset_roots, "asset_roots")
    for index, left in enumerate(asset_roots):
        for right in asset_roots[index + 1 :]:
            if _under(left, right) or _under(right, left):
                raise AuditError(f"asset roots overlap: {left} and {right}")

    licenses = []
    for index, raw in enumerate(_list(payload["licenses"], "licenses")):
        entry = _exact_object(raw, {"id", "path", "size", "sha256"}, f"licenses[{index}]")
        license_id = _nonempty_string(entry["id"], f"licenses[{index}].id")
        if license_id not in ALLOWED_LICENSES:
            raise AuditError(f"license {license_id!r} is outside the reviewed release allowlist")
        _canonical_relative(entry["path"], f"licenses[{index}].path")
        _license_size(entry["size"], f"licenses[{index}].size")
        _sha256(entry["sha256"], f"licenses[{index}].sha256")
        licenses.append(entry)

    assets = []
    asset_fields = {
        "path",
        "kind",
        "role",
        "size",
        "sha256",
        "author",
        "source_url",
        "license",
        "attribution",
        "modifications",
    }
    for index, raw in enumerate(_list(payload["assets"], "assets")):
        entry = _exact_object(raw, asset_fields, f"assets[{index}]")
        path = _canonical_relative(entry["path"], f"assets[{index}].path")
        kind = _nonempty_string(entry["kind"], f"assets[{index}].kind")
        if kind not in KIND_EXTENSIONS:
            raise AuditError(f"assets[{index}].kind is unsupported: {kind!r}")
        if path.suffix.lower() not in KIND_EXTENSIONS[kind]:
            raise AuditError(
                f"asset {path} extension is not admitted for kind {kind!r}"
            )
        _nonempty_string(entry["role"], f"assets[{index}].role")
        _positive_size(entry["size"], f"assets[{index}].size")
        _sha256(entry["sha256"], f"assets[{index}].sha256")
        _nonempty_string(entry["author"], f"assets[{index}].author")
        _https_url(entry["source_url"], f"assets[{index}].source_url")
        _nonempty_string(entry["license"], f"assets[{index}].license")
        _nonempty_string(entry["attribution"], f"assets[{index}].attribution")
        _nonempty_string(entry["modifications"], f"assets[{index}].modifications")
        assets.append(entry)
    return asset_roots, licenses, assets


def audit_release(release_root: Path, manifest_path: Path) -> dict[str, object]:
    """Audit one exact release tree and return a deterministic machine-readable report."""

    lexical_root = Path(os.path.abspath(release_root))
    lexical_manifest = (
        Path(os.path.abspath(manifest_path))
        if manifest_path.is_absolute()
        else lexical_root / manifest_path
    )
    try:
        manifest_relative = _canonical_relative(
            lexical_manifest.relative_to(lexical_root).as_posix(), "manifest path"
        )
    except (AuditError, OSError, ValueError) as exc:
        raise AuditError("manifest must be a regular file inside the release root") from exc
    root = _resolved_release_root(release_root)
    manifest_file = _regular_file(root, manifest_relative, "manifest")
    payload, manifest_sha256 = _read_manifest(manifest_file)
    asset_roots, licenses, assets = _validate_manifest_shape(payload)

    license_ids: dict[str, dict[str, object]] = {}
    license_paths: list[PurePosixPath] = []
    for index, entry in enumerate(licenses):
        license_id = str(entry["id"])
        if license_id in license_ids:
            raise AuditError(f"license id is declared more than once: {license_id}")
        relative = _canonical_relative(entry["path"], f"licenses[{index}].path")
        license_paths.append(relative)
        if any(_under(relative, asset_root) for asset_root in asset_roots):
            raise AuditError(f"license text must be outside asset roots: {relative}")
        path = _regular_file(root, relative, "license text")
        actual_size, actual_sha256, prefix = _hash_file(path)
        if not prefix.strip():
            raise AuditError(f"license text is blank: {relative}")
        if actual_size != entry["size"] or actual_sha256 != entry["sha256"]:
            raise AuditError(f"license text bytes do not match manifest: {relative}")
        license_ids[license_id] = entry
    _reject_casefold_duplicates(license_paths, "license paths")

    declared_paths: list[PurePosixPath] = []
    referenced_licenses: set[str] = set()
    total_bytes = 0
    by_kind: dict[str, int] = {kind: 0 for kind in sorted(KIND_EXTENSIONS)}
    for index, entry in enumerate(assets):
        relative = _canonical_relative(entry["path"], f"assets[{index}].path")
        declared_paths.append(relative)
        owners = [asset_root for asset_root in asset_roots if _under(relative, asset_root)]
        if len(owners) != 1:
            raise AuditError(f"asset must be beneath exactly one declared asset root: {relative}")
        license_id = str(entry["license"])
        if license_id not in license_ids:
            raise AuditError(f"asset {relative} references undeclared license {license_id!r}")
        referenced_licenses.add(license_id)
        path = _regular_file(root, relative, "asset")
        actual_size, actual_sha256, prefix = _hash_file(path)
        for magic, label in FORBIDDEN_MAGIC:
            if prefix.startswith(magic):
                raise AuditError(f"asset {relative} contains forbidden {label} content")
        if actual_size != entry["size"] or actual_sha256 != entry["sha256"]:
            raise AuditError(f"asset bytes do not match manifest: {relative}")
        total_bytes += actual_size
        by_kind[str(entry["kind"])] += 1
    _reject_casefold_duplicates(declared_paths, "assets")

    unreferenced = sorted(set(license_ids) - referenced_licenses)
    if unreferenced:
        raise AuditError(f"unreferenced license declarations: {', '.join(unreferenced)}")

    scanned_paths: list[PurePosixPath] = []
    for asset_root in asset_roots:
        root_files = _scan_asset_root(root, asset_root)
        if not root_files:
            raise AuditError(f"asset root contains no regular files: {asset_root}")
        scanned_paths.extend(root_files)
    _reject_casefold_duplicates(scanned_paths, "asset tree")
    declared = {path.as_posix() for path in declared_paths}
    scanned = {path.as_posix() for path in scanned_paths}
    missing = sorted(declared - scanned)
    unmanifested = sorted(scanned - declared)
    if missing:
        raise AuditError(f"manifest names missing asset files: {', '.join(missing)}")
    if unmanifested:
        raise AuditError(f"asset roots contain unmanifested files: {', '.join(unmanifested)}")

    return {
        "schema": REPORT_SCHEMA,
        "ready": True,
        "scope": "mechanical manifest coverage and DoN release-license policy",
        "legal_title_certified": False,
        "manifest": manifest_relative.as_posix(),
        "manifest_sha256": manifest_sha256,
        "asset_roots": [path.as_posix() for path in asset_roots],
        "assets": len(assets),
        "asset_bytes": total_bytes,
        "assets_by_kind": {kind: count for kind, count in by_kind.items() if count},
        "licenses": len(licenses),
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release-root", required=True, type=Path)
    parser.add_argument("--manifest", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        report = audit_release(args.release_root, args.manifest)
    except AuditError as exc:
        print(f"content-license-audit: REFUSE: {exc}", file=sys.stderr)
        return 3
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
