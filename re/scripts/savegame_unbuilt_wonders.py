#!/usr/bin/env python3
"""Parse complete retail ``UnbuiltWonders::walk_data`` save images."""

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


TAG_UNBUILT_WONDERS = 0
TAG_STRING_TABLE_INDEX = 7082
PLAYER_LIST_COUNT = 8
ROW_LOGICAL_SIZE = 3
ROW_MEMORY_SIZE = 4
MAX_COUNT = 1 << 20
DEFAULT_SCHEMA_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"


class UnbuiltWondersParseError(ValueError):
    """The stream or PDB layout contradicts UnbuiltWonders::walk_data."""


@dataclasses.dataclass(frozen=True)
class UnbuiltWonderImage:
    index: int
    offset: int
    end: int
    object_id: int
    who: int
    raw: bytes
    sha256: str


@dataclasses.dataclass(frozen=True)
class UnbuiltWonderArray:
    player: int
    offset: int
    end: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    rows: tuple[UnbuiltWonderImage, ...]
    sha256: str


@dataclasses.dataclass(frozen=True)
class UnbuiltWondersSection:
    offset: int
    end: int
    tag: int
    lists: tuple[UnbuiltWonderArray, ...]
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class _Layout:
    sha256: str


def _fields(record: dict[str, object]) -> tuple[tuple[object, ...], ...]:
    return tuple((field["name"], field["offset"], field["size"], field["type"]) for field in record["flattened"])


@functools.lru_cache(maxsize=4)
def _load_layout(path_text: str) -> _Layout:
    path = pathlib.Path(path_text)
    try:
        classes = json.loads(path.read_bytes())["classes"]
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise UnbuiltWondersParseError(f"cannot load UnbuiltWonders PDB layout from {path}: {error}") from error
    expected_sizes = {"UnbuiltWonders": 224, "Array<UnbuiltWonder>": 28, "UnbuiltWonder": ROW_MEMORY_SIZE}
    receipt: dict[str, object] = {}
    for name, size in expected_sizes.items():
        record = classes.get(name)
        if not record or record.get("size") != size:
            raise UnbuiltWondersParseError(f"PDB {name} size disagrees: {record.get('size') if record else None}")
        receipt[name] = {"size": size, "flattened": _fields(record)}
    owner = _fields(classes["UnbuiltWonders"])
    if owner != (("lists", 0, 224, "Array<UnbuiltWonder>[8]"),):
        raise UnbuiltWondersParseError(f"PDB UnbuiltWonders fields disagree: {owner!r}")
    array = _fields(classes["Array<UnbuiltWonder>"])
    expected_array = (
        ("length", 4, 4, "int"), ("size", 8, 4, "int"), ("increment", 12, 2, "short"),
        ("list", 16, 4, "UnbuiltWonder*"), ("flags", 20, 1, "unsigned char"), ("cur_index", 24, 4, "int"),
    )
    if array != expected_array:
        raise UnbuiltWondersParseError(f"PDB Array<UnbuiltWonder> fields disagree: {array!r}")
    row = _fields(classes["UnbuiltWonder"])
    if row != (("o", 0, 2, "short"), ("who", 2, 1, "char")):
        raise UnbuiltWondersParseError(f"PDB UnbuiltWonder fields disagree: {row!r}")
    receipt["selectors"] = {"array_count": PLAYER_LIST_COUNT, "row_direct": [0, ROW_LOGICAL_SIZE], "row_stride": ROW_MEMORY_SIZE}
    encoded = json.dumps(receipt, sort_keys=True, separators=(",", ":")).encode()
    return _Layout(hashlib.sha256(encoded).hexdigest())


class _Reader:
    def __init__(self, data: bytes | bytearray | memoryview, offset: int):
        self.data = memoryview(data)
        if offset < 0 or offset > len(self.data):
            raise UnbuiltWondersParseError(f"offset {offset:#x} is outside the stream")
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise UnbuiltWondersParseError(f"{what} range [{self.pos:#x},{end:#x}) exceeds {len(self.data):#x}-byte stream")
        start = self.pos
        self.pos = end
        return self.data[start:end]

    def u8(self, what: str) -> int: return self.take(1, what)[0]
    def i16(self, what: str) -> int: return struct.unpack("<h", self.take(2, what))[0]
    def i32(self, what: str) -> int: return struct.unpack("<i", self.take(4, what))[0]


def _count(reader: _Reader, what: str) -> int:
    value = reader.i32(what)
    if value < 0 or value > MAX_COUNT:
        raise UnbuiltWondersParseError(f"invalid {what} {value}")
    return value


def _array(reader: _Reader, player: int) -> UnbuiltWonderArray:
    offset = reader.pos
    length = _count(reader, f"lists[{player}] length")
    capacity = increment = flags = None
    rows: tuple[UnbuiltWonderImage, ...] = ()
    if length:
        capacity = _count(reader, f"lists[{player}] capacity")
        increment = reader.i16(f"lists[{player}] increment")
        flags = reader.u8(f"lists[{player}] flags")
        if capacity < length or flags & 0x40:
            raise UnbuiltWondersParseError(f"invalid lists[{player}] history")
        images = []
        for index in range(length):
            row_offset = reader.pos
            raw = bytes(reader.take(ROW_LOGICAL_SIZE, f"lists[{player}][{index}] logical bytes"))
            object_id, who = struct.unpack("<hb", raw)
            images.append(UnbuiltWonderImage(index, row_offset, reader.pos, object_id, who, raw, hashlib.sha256(raw).hexdigest()))
        rows = tuple(images)
    return UnbuiltWonderArray(player, offset, reader.pos, length, capacity, increment, flags, rows, hashlib.sha256(reader.data[offset:reader.pos]).hexdigest())


def parse_unbuilt_wonders_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tag: bool = True,
    schema_path: pathlib.Path = DEFAULT_SCHEMA_PATH,
) -> UnbuiltWondersSection:
    """Parse the tag and all eight player lists, stopping before UnbuiltCities."""

    layout = _load_layout(str(pathlib.Path(schema_path).resolve()))
    reader = _Reader(data, offset)
    tag = reader.u8("UnbuiltWonders tag")
    if require_tag and tag != TAG_UNBUILT_WONDERS:
        raise UnbuiltWondersParseError(f"UnbuiltWonders tag {tag:#04x} != {TAG_UNBUILT_WONDERS:#04x}")
    lists = tuple(_array(reader, player) for player in range(PLAYER_LIST_COUNT))
    return UnbuiltWondersSection(offset, reader.pos, tag, lists, hashlib.sha256(reader.data[offset:reader.pos]).hexdigest(), layout.sha256)


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes(); return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument("--offset", required=True, type=lambda text: int(text, 0))
    args = parser.parse_args(argv)
    section = parse_unbuilt_wonders_section(_load(args.file), args.offset)
    print(f"{args.file}: UnbuiltWonders {section.offset:#x}..{section.end:#x} ({section.size} bytes) sha256={section.sha256}\n  lengths={[array.length for array in section.lists]}\n  next owner begins at {section.end:#x}: UnbuiltCities::walk_data")
    return 0


if __name__ == "__main__": raise SystemExit(main())
