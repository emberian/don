#!/usr/bin/env python3
"""Decode the caller's exact direct ``detail_threshold`` save word."""

from __future__ import annotations

import argparse
import dataclasses
import functools
import gzip
import hashlib
import json
import pathlib
import struct
from typing import Sequence


DETAIL_THRESHOLD_SIZE = 4
DETAIL_THRESHOLD_VA = 0x00C0623C
NEXT_GLOBAL_VA = 0x00C06240
DEFAULT_SYMBOLS_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/symbols.json"


class DetailThresholdParseError(ValueError):
    """The stream or PDB symbol receipt contradicts the direct owner."""


@dataclasses.dataclass(frozen=True)
class DetailThresholdBlock:
    offset: int
    end: int
    bits: int
    raw: bytes
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int: return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class _Layout:
    sha256: str


@functools.lru_cache(maxsize=4)
def _load_layout(path_text: str) -> _Layout:
    path = pathlib.Path(path_text)
    try: globals_ = json.loads(path.read_bytes())["globals"]
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error: raise DetailThresholdParseError(f"cannot load PDB globals from {path}: {error}") from error
    selected = [row for row in globals_ if row.get("kind") == "global" and row.get("va") in ("0x00c0623c", "0x00c06240")]
    receipt = sorted((row.get("va"), row.get("name"), row.get("size"), row.get("type"), row.get("type_index"), row.get("mangled")) for row in selected)
    expected = [("0x00c0623c", "detail_threshold", 4, "float", 64, "?detail_threshold@@3MA"), ("0x00c06240", "sGameSaveVersion", 4, "int", 116, "?sGameSaveVersion@@3HA")]
    if receipt != expected: raise DetailThresholdParseError(f"PDB global boundary disagrees: {receipt!r}")
    return _Layout(hashlib.sha256(json.dumps(receipt, separators=(",", ":")).encode()).hexdigest())


def parse_detail_threshold_block(data: bytes | bytearray | memoryview, offset: int, *, symbols_path: pathlib.Path = DEFAULT_SYMBOLS_PATH) -> DetailThresholdBlock:
    """Decode exactly four float bits and stop at the following Camera tag."""
    layout = _load_layout(str(pathlib.Path(symbols_path).resolve())); view = memoryview(data); end = offset + DETAIL_THRESHOLD_SIZE
    if offset < 0 or end > len(view): raise DetailThresholdParseError(f"detail_threshold range [{offset:#x},{end:#x}) exceeds {len(view):#x}")
    raw = bytes(view[offset:end]); bits = struct.unpack("<I", raw)[0]
    return DetailThresholdBlock(offset, end, bits, raw, hashlib.sha256(raw).hexdigest(), layout.sha256)


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes(); return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__); parser.add_argument("file", type=pathlib.Path); parser.add_argument("--offset", required=True, type=lambda text: int(text, 0)); args = parser.parse_args(argv); block = parse_detail_threshold_block(_load(args.file), args.offset)
    print(f"{args.file}: detail_threshold {block.offset:#x}..{block.end:#x} bits={block.bits:#010x} sha256={block.sha256}\n  next owner begins at {block.end:#x}: caller Camera tag")
    return 0


if __name__ == "__main__": raise SystemExit(main())
