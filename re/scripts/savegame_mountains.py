#!/usr/bin/env python3
"""Parse the retail ``Mountains::walk_data`` save-stream section.

This exclusive helper begins at the first Mountains byte and consumes its tag
plus four exact array histories.  It stops before the caller's direct
``Constants`` walk and does not edit the shared save parser.

The grammar comes from ``Mountains::walk_data`` at ``0x0089d320`` and the three
container walkers it calls.  Field names and sizes are resolved through the
matching PDB's ``MountainsData`` virtual base.  No specimen bytes are embedded
or written.
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


TAG_MOUNTAINS = 0x00
TAG_STRING_TABLE_INDEX = 4934
MAX_ARRAY_LENGTH = 1 << 20

ARRAY_LAYOUT = (
    ("mountain_loc_wcoords_x", 4),
    ("mountain_loc_wcoords_y", 4),
    ("mountain_locs", 12),
    ("mountain_types", 4),
)


class MountainsParseError(ValueError):
    """The input contradicts the recovered Mountains traversal."""


@dataclasses.dataclass(frozen=True)
class ArrayImage:
    name: str
    offset: int
    end: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    element_size: int
    payload_words: tuple[int, ...]
    payload_sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class MountainsSection:
    offset: int
    end: int
    tag: int
    arrays: tuple[ArrayImage, ...]
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


class _Reader:
    def __init__(self, data: bytes | bytearray | memoryview, offset: int):
        self.data = memoryview(data)
        if offset < 0 or offset > len(self.data):
            raise MountainsParseError(f"offset {offset:#x} is outside the stream")
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise MountainsParseError(
                f"{what} range [{self.pos:#x},{end:#x}) exceeds "
                f"{len(self.data):#x}-byte stream"
            )
        start = self.pos
        self.pos = end
        return self.data[start:end]

    def u8(self, what: str) -> int:
        return self.take(1, what)[0]

    def i16(self, what: str) -> int:
        return struct.unpack("<h", self.take(2, what))[0]

    def i32(self, what: str) -> int:
        return struct.unpack("<i", self.take(4, what))[0]


def _sha(data: memoryview) -> str:
    return hashlib.sha256(data).hexdigest()


def _parse_array(reader: _Reader, name: str, element_size: int) -> ArrayImage:
    start = reader.pos
    length = reader.i32(f"{name}.length")
    if length < 0 or length > MAX_ARRAY_LENGTH:
        raise MountainsParseError(
            f"{name} has invalid length {length} at {start:#x}"
        )
    if length == 0:
        empty = memoryview(b"")
        return ArrayImage(
            name=name,
            offset=start,
            end=reader.pos,
            length=0,
            capacity=None,
            increment=None,
            flags=None,
            element_size=element_size,
            payload_words=(),
            payload_sha256=_sha(empty),
        )

    capacity = reader.i32(f"{name}.capacity")
    increment = reader.i16(f"{name}.increment")
    flags = reader.u8(f"{name}.flags")
    if capacity < length or capacity > MAX_ARRAY_LENGTH:
        raise MountainsParseError(
            f"{name} has invalid history length={length}, capacity={capacity} "
            f"at {start:#x}"
        )
    if flags & 0x40:
        raise MountainsParseError(
            f"{name} flags {flags:#04x} retain writer-cleared bit 0x40 "
            f"at {reader.pos - 1:#x}"
        )

    payload = reader.take(length * element_size, f"{name}.elements")
    word_count = len(payload) // 4
    payload_words = struct.unpack(f"<{word_count}I", payload)
    return ArrayImage(
        name=name,
        offset=start,
        end=reader.pos,
        length=length,
        capacity=capacity,
        increment=increment,
        flags=flags,
        element_size=element_size,
        payload_words=payload_words,
        payload_sha256=_sha(payload),
    )


def parse_mountains_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tag: bool = True,
) -> MountainsSection:
    """Parse one exact retail Mountains section beginning at ``offset``.

    The returned ``end`` is the first byte of the following direct Constants
    range walked by ``WalkDataGame::walk_data``.
    """

    reader = _Reader(data, offset)
    tag = reader.u8("Mountains tag")
    if require_tag and tag != TAG_MOUNTAINS:
        raise MountainsParseError(
            f"Mountains tag {tag:#04x} != {TAG_MOUNTAINS:#04x} at {offset:#x}"
        )
    arrays = tuple(
        _parse_array(reader, name, element_size)
        for name, element_size in ARRAY_LAYOUT
    )
    return MountainsSection(
        offset=offset,
        end=reader.pos,
        tag=tag,
        arrays=arrays,
        sha256=_sha(reader.data[offset : reader.pos]),
    )


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def _jsonable(value: object) -> object:
    if dataclasses.is_dataclass(value):
        result: dict[str, object] = {}
        for field in dataclasses.fields(value):
            field_value = getattr(value, field.name)
            if field.name in ("offset", "end"):
                result[field.name] = f"0x{field_value:x}"
            else:
                result[field.name] = _jsonable(field_value)
        if isinstance(value, (ArrayImage, MountainsSection)):
            result["size"] = value.size
        return result
    if isinstance(value, tuple):
        return [_jsonable(item) for item in value]
    return value


def _summary(section: MountainsSection, path: pathlib.Path) -> str:
    lines = [
        f"{path}: Mountains {section.offset:#x}..{section.end:#x} "
        f"({section.size} bytes) sha256={section.sha256}",
        f"  tag={section.tag:#04x} (StringTable[{TAG_STRING_TABLE_INDEX}])",
    ]
    for array in section.arrays:
        history = "empty" if array.length == 0 else (
            f"length={array.length} capacity={array.capacity} "
            f"increment={array.increment} flags={array.flags:#04x}"
        )
        lines.append(
            f"  {array.name}: {history}, element_size={array.element_size}, "
            f"payload_sha256={array.payload_sha256}"
        )
    lines.append(
        f"  next owner begins at {section.end:#x} "
        "(WalkDataGame direct Constants[+0x000..+0xd40) at 0x005a2a68)"
    )
    return "\n".join(lines)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument(
        "--offset",
        required=True,
        type=lambda text: int(text, 0),
        help="decompressed stream offset of the Mountains tag",
    )
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    section = parse_mountains_section(_load(args.file), args.offset)
    if args.json:
        print(json.dumps(_jsonable(section), indent=2))
    else:
        print(_summary(section, args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
