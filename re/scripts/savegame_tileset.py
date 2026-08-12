#!/usr/bin/env python3
"""Parse the retail ``TileSet::walk_data`` save-stream section.

This helper begins at the exact end of ``Types::walk_data`` and consumes only
the TileSet section tag and its current-tileset ``String``.  It deliberately
stops before ``Mountains::walk_data``.

The grammar is recovered from the shipped PE at ``0x0087b290`` and
``String::walk_data`` at ``0x00a1b2d0``.  The matching PDB identifies
``TileSet::cur_tileset``, ``TileSetData::name``, and the String layout.  No
specimen bytes are embedded or written.
"""

from __future__ import annotations

import argparse
import dataclasses
import gzip
import hashlib
import json
import pathlib
import struct
from typing import Sequence


TAG_TILESET = 0x00
TAG_STRING_TABLE_INDEX = 6712
MAX_STRING_CODE_UNITS = 0xFFFF


class TileSetParseError(ValueError):
    """The input contradicts the recovered TileSet traversal."""


@dataclasses.dataclass(frozen=True)
class TileSetSection:
    offset: int
    end: int
    tag: int
    name_code_units: int
    current_tileset_name: str
    name_payload_sha256: str
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


class _Reader:
    def __init__(self, data: bytes | bytearray | memoryview, offset: int):
        self.data = memoryview(data)
        if offset < 0 or offset > len(self.data):
            raise TileSetParseError(f"offset {offset:#x} is outside the stream")
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise TileSetParseError(
                f"{what} range [{self.pos:#x},{end:#x}) exceeds "
                f"{len(self.data):#x}-byte stream"
            )
        start = self.pos
        self.pos = end
        return self.data[start:end]

    def u8(self, what: str) -> int:
        return self.take(1, what)[0]

    def u32(self, what: str) -> int:
        return struct.unpack("<I", self.take(4, what))[0]


def _sha(data: memoryview) -> str:
    return hashlib.sha256(data).hexdigest()


def parse_tileset_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tag: bool = True,
) -> TileSetSection:
    """Parse one exact retail TileSet section beginning at ``offset``.

    The returned ``end`` is the first byte owned by the following
    ``Mountains::walk_data`` call.
    """

    reader = _Reader(data, offset)
    tag = reader.u8("TileSet tag")
    if require_tag and tag != TAG_TILESET:
        raise TileSetParseError(
            f"TileSet tag {tag:#04x} != {TAG_TILESET:#04x} at {offset:#x}"
        )

    name_code_units = reader.u32("current tileset name length")
    if name_code_units > MAX_STRING_CODE_UNITS:
        raise TileSetParseError(
            f"current tileset name has impossible String length "
            f"{name_code_units} at {reader.pos - 4:#x}"
        )
    payload = reader.take(name_code_units * 2, "current tileset name payload")
    try:
        current_tileset_name = payload.tobytes().decode("utf-16-le")
    except UnicodeDecodeError as exc:
        raise TileSetParseError(
            f"current tileset name is not valid UTF-16LE: {exc}"
        ) from exc

    return TileSetSection(
        offset=offset,
        end=reader.pos,
        tag=tag,
        name_code_units=name_code_units,
        current_tileset_name=current_tileset_name,
        name_payload_sha256=_sha(payload),
        sha256=_sha(reader.data[offset : reader.pos]),
    )


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def _jsonable(section: TileSetSection) -> dict[str, object]:
    result = dataclasses.asdict(section)
    result["offset"] = f"0x{section.offset:x}"
    result["end"] = f"0x{section.end:x}"
    result["size"] = section.size
    result["tag"] = f"0x{section.tag:02x}"
    return result


def _summary(section: TileSetSection, path: pathlib.Path) -> str:
    return "\n".join(
        (
            f"{path}: TileSet {section.offset:#x}..{section.end:#x} "
            f"({section.size} bytes) sha256={section.sha256}",
            f"  tag={section.tag:#04x} (StringTable[{TAG_STRING_TABLE_INDEX}]) "
            f"current_tileset_name={section.current_tileset_name!r} "
            f"code_units={section.name_code_units}",
            f"  next owner begins at {section.end:#x} "
            "(Mountains::walk_data 0x0089d320)",
        )
    )


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument(
        "--offset",
        required=True,
        type=lambda text: int(text, 0),
        help="decompressed stream offset of the TileSet tag",
    )
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    section = parse_tileset_section(_load(args.file), args.offset)
    if args.json:
        print(json.dumps(_jsonable(section), indent=2))
    else:
        print(_summary(section, args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
