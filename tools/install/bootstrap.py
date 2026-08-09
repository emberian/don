#!/usr/bin/env python3
"""Verify and locally install supported user-owned retail data for DoN development.

The tool never downloads data, copies executables, or writes outside ``ron-data`` in an
already-recognized DoN checkout.  Every source byte is validated before the first write.
Existing files are retained only when they match exactly; mismatches are refused, never
overwritten.  Missing files are staged and installed with create-only hard links.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import tempfile


HERE = Path(__file__).resolve().parent
DEFAULT_MANIFEST = HERE / "owned-inputs.json"


class BootstrapError(RuntimeError):
    pass


@dataclass(frozen=True)
class Entry:
    source: PurePosixPath
    destination: PurePosixPath
    size: int
    sha256: str


@dataclass(frozen=True)
class Manifest:
    identity: Entry
    files: tuple[Entry, ...]
    digest: str


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _safe_relative(value: object, field: str) -> PurePosixPath:
    if not isinstance(value, str) or not value or "\\" in value or "\0" in value:
        raise BootstrapError(f"manifest {field} is not a canonical relative POSIX path")
    path = PurePosixPath(value)
    if path.is_absolute() or any(part in {"", ".", ".."} for part in path.parts):
        raise BootstrapError(f"manifest {field} escapes its root")
    return path


def _entry(raw: object, *, identity: bool = False) -> Entry:
    if not isinstance(raw, dict) or set(raw) != {"source", "size", "sha256"} | (
        set() if identity else {"destination"}
    ):
        raise BootstrapError("manifest entry fields are invalid")
    source = _safe_relative(raw.get("source"), "source")
    destination = source if identity else _safe_relative(raw.get("destination"), "destination")
    size = raw.get("size")
    digest = raw.get("sha256")
    if not isinstance(size, int) or size <= 0:
        raise BootstrapError("manifest entry size is invalid")
    if not isinstance(digest, str) or not re_full_sha256(digest):
        raise BootstrapError("manifest entry SHA-256 is invalid")
    return Entry(source, destination, size, digest.lower())


def re_full_sha256(value: str) -> bool:
    return len(value) == 64 and all(char in "0123456789abcdefABCDEF" for char in value)


def load_manifest(path: Path = DEFAULT_MANIFEST) -> Manifest:
    try:
        raw_bytes = path.read_bytes()
        payload = json.loads(raw_bytes)
    except (OSError, json.JSONDecodeError) as exc:
        raise BootstrapError("owned-input manifest is unreadable") from exc
    if not isinstance(payload, dict) or set(payload) != {"schema", "retail_identity", "files"}:
        raise BootstrapError("owned-input manifest shape is invalid")
    if payload["schema"] != "don.owned-inputs.v1" or not isinstance(payload["files"], list):
        raise BootstrapError("owned-input manifest schema is unsupported")
    identity = _entry(payload["retail_identity"], identity=True)
    files = tuple(_entry(item) for item in payload["files"])
    if not files:
        raise BootstrapError("owned-input manifest has no data files")
    destinations = [entry.destination.as_posix().casefold() for entry in files]
    sources = [entry.source.as_posix().casefold() for entry in files]
    if len(set(destinations)) != len(destinations) or len(set(sources)) != len(sources):
        raise BootstrapError("owned-input manifest has case-insensitive path collisions")
    if any(entry.destination.parts[0] not in {"ai-scripts"} and len(entry.destination.parts) != 1
           for entry in files):
        raise BootstrapError("owned-input destination is outside the supported data layout")
    return Manifest(identity, files, hashlib.sha256(raw_bytes).hexdigest())


def resolve_casefold(root: Path, relative: PurePosixPath) -> Path:
    if root.is_symlink():
        raise BootstrapError("retail root is a symbolic link")
    current = root.resolve(strict=True)
    if not current.is_dir():
        raise BootstrapError("retail root is not a real directory")
    for part in relative.parts:
        try:
            children = [child for child in current.iterdir() if child.name.casefold() == part.casefold()]
        except OSError as exc:
            raise BootstrapError(f"could not enumerate owned-install component {part!r}") from exc
        if len(children) != 1:
            raise BootstrapError(f"owned-install component is missing or ambiguous: {relative}")
        candidate = children[0]
        if candidate.is_symlink():
            raise BootstrapError(f"owned-install path contains a symbolic link: {relative}")
        current = candidate
    resolved = current.resolve(strict=True)
    try:
        resolved.relative_to(root.resolve(strict=True))
    except ValueError as exc:
        raise BootstrapError("owned-install path escaped its root") from exc
    if not resolved.is_file():
        raise BootstrapError(f"owned-install input is not a regular file: {relative}")
    return resolved


def verify_file(path: Path, entry: Entry, label: str) -> None:
    try:
        size = path.stat().st_size
    except OSError as exc:
        raise BootstrapError(f"could not stat {label}") from exc
    if size != entry.size:
        raise BootstrapError(f"{label} has unsupported size")
    if sha256_file(path) != entry.sha256:
        raise BootstrapError(f"{label} has unsupported SHA-256")


def verify_sources(retail_root: Path, manifest: Manifest) -> dict[Entry, Path]:
    identity_path = resolve_casefold(retail_root, manifest.identity.source)
    verify_file(identity_path, manifest.identity, "retail executable identity")
    resolved: dict[Entry, Path] = {}
    for entry in manifest.files:
        source = resolve_casefold(retail_root, entry.source)
        verify_file(source, entry, f"owned input {entry.source}")
        resolved[entry] = source
    return resolved


def validate_workspace(workspace: Path) -> Path:
    if workspace.is_symlink():
        raise BootstrapError("destination checkout is a symbolic link")
    root = workspace.resolve(strict=True)
    if not (root / "Cargo.toml").is_file() or not (root / "GOAL.md").is_file():
        raise BootstrapError("destination is not a recognized DoN source checkout")
    try:
        ignore = (root / ".gitignore").read_text(encoding="utf-8")
    except OSError as exc:
        raise BootstrapError("destination checkout has no readable .gitignore") from exc
    if "ron-data/" not in {line.strip() for line in ignore.splitlines()}:
        raise BootstrapError("destination checkout does not ignore ron-data/")
    output = root / "ron-data"
    if output.is_symlink():
        raise BootstrapError("destination ron-data is a symbolic link")
    return output


def _destination(output: Path, relative: PurePosixPath) -> Path:
    path = output.joinpath(*relative.parts)
    current = output
    for part in relative.parts:
        current /= part
        if current.is_symlink():
            raise BootstrapError(f"destination path contains a symbolic link: {relative}")
    try:
        path.resolve(strict=False).relative_to(output.resolve(strict=False))
    except ValueError as exc:
        raise BootstrapError("destination path escaped ron-data") from exc
    return path


def inspect_destinations(output: Path, manifest: Manifest) -> tuple[list[Entry], int]:
    missing = []
    kept = 0
    for entry in manifest.files:
        destination = _destination(output, entry.destination)
        if destination.is_symlink():
            raise BootstrapError(f"destination is a symbolic link: {entry.destination}")
        if destination.exists():
            if not destination.is_file():
                raise BootstrapError(f"destination is not a regular file: {entry.destination}")
            verify_file(destination, entry, f"existing destination {entry.destination}")
            kept += 1
        else:
            missing.append(entry)
    return missing, kept


def install(retail_root: Path, workspace: Path, manifest: Manifest, *, dry_run: bool) -> dict:
    sources = verify_sources(retail_root, manifest)
    output = validate_workspace(workspace)
    if output.exists() and not output.is_dir():
        raise BootstrapError("destination ron-data exists but is not a directory")
    missing, kept = inspect_destinations(output, manifest)
    result = {
        "schema": "don.owned-input-bootstrap.v1",
        "ready": True,
        "mutation": "none" if dry_run else "create-only exact local copies under ignored ron-data",
        "manifest_sha256": manifest.digest,
        "verified_sources": len(manifest.files),
        "already_present": kept,
        "to_install": len(missing),
        "installed": 0,
        "retail_content_redistributed": False,
    }
    if dry_run or not missing:
        return result

    output.mkdir(mode=0o700, parents=False, exist_ok=True)
    if output.is_symlink():
        raise BootstrapError("destination ron-data became a symbolic link")
    staging = Path(tempfile.mkdtemp(prefix=".don-owned-inputs-", dir=output))
    try:
        for entry in missing:
            staged = _destination(staging, entry.destination)
            staged.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
            shutil.copyfile(sources[entry], staged, follow_symlinks=False)
            verify_file(staged, entry, f"staged destination {entry.destination}")

        for entry in missing:
            staged = _destination(staging, entry.destination)
            destination = _destination(output, entry.destination)
            destination.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
            if destination.is_symlink() or destination.exists():
                raise BootstrapError(f"destination changed during install: {entry.destination}")
            try:
                os.link(staged, destination, follow_symlinks=False)
            except FileExistsError as exc:
                raise BootstrapError(f"destination appeared during install: {entry.destination}") from exc
            verify_file(destination, entry, f"installed destination {entry.destination}")
            result["installed"] += 1
    finally:
        shutil.rmtree(staging, ignore_errors=True)
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["check", "install"])
    parser.add_argument("--retail-root", required=True, type=Path)
    parser.add_argument("--workspace", type=Path, default=HERE.parents[1])
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST,
                        help=argparse.SUPPRESS)
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    try:
        manifest = load_manifest(args.manifest)
        if args.command == "check":
            verify_sources(args.retail_root, manifest)
            result = {
                "schema": "don.owned-input-bootstrap.v1",
                "ready": True,
                "mutation": "none",
                "manifest_sha256": manifest.digest,
                "verified_sources": len(manifest.files),
                "retail_content_redistributed": False,
            }
        else:
            result = install(args.retail_root, args.workspace, manifest, dry_run=args.dry_run)
    except BootstrapError as exc:
        print(f"REFUSING owned-input bootstrap: {exc}")
        return 2
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
