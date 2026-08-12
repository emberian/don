#!/usr/bin/env python3
"""Parse complete retail ``UnbuiltCities::walk_data`` save images."""

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


PLAYER_LIST_COUNT = 8
ROW_LOGICAL_SIZE = 3
ROW_MEMORY_SIZE = 4
MAX_COUNT = 1 << 20
DEFAULT_SCHEMA_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"


class UnbuiltCitiesParseError(ValueError):
    """The stream or PDB layout contradicts UnbuiltCities::walk_data."""


@dataclasses.dataclass(frozen=True)
class UnbuiltCityImage:
    index: int
    offset: int
    end: int
    object_id: int
    who: int
    raw: bytes
    sha256: str


@dataclasses.dataclass(frozen=True)
class UnbuiltCityArray:
    player: int
    offset: int
    end: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    rows: tuple[UnbuiltCityImage, ...]
    sha256: str


@dataclasses.dataclass(frozen=True)
class UnbuiltCitiesSection:
    offset: int
    end: int
    lists: tuple[UnbuiltCityArray, ...]
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int: return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class _Layout:
    sha256: str


def _fields(record: dict[str, object]) -> tuple[tuple[object, ...], ...]:
    return tuple((field["name"], field["offset"], field["size"], field["type"]) for field in record["flattened"])


@functools.lru_cache(maxsize=4)
def _load_layout(path_text: str) -> _Layout:
    path = pathlib.Path(path_text)
    try: classes = json.loads(path.read_bytes())["classes"]
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error: raise UnbuiltCitiesParseError(f"cannot load UnbuiltCities PDB layout from {path}: {error}") from error
    expected_sizes = {"UnbuiltCities": 224, "Array<UnbuiltCity>": 28, "UnbuiltCity": ROW_MEMORY_SIZE}
    receipt: dict[str, object] = {}
    for name, size in expected_sizes.items():
        record = classes.get(name)
        if not record or record.get("size") != size: raise UnbuiltCitiesParseError(f"PDB {name} size disagrees: {record.get('size') if record else None}")
        receipt[name] = {"size": size, "flattened": _fields(record)}
    owner = _fields(classes["UnbuiltCities"])
    if owner != (("lists", 0, 224, "Array<UnbuiltCity>[8]"),): raise UnbuiltCitiesParseError(f"PDB UnbuiltCities fields disagree: {owner!r}")
    array = _fields(classes["Array<UnbuiltCity>"])
    expected_array = (("length", 4, 4, "int"), ("size", 8, 4, "int"), ("increment", 12, 2, "short"), ("list", 16, 4, "UnbuiltCity*"), ("flags", 20, 1, "unsigned char"), ("cur_index", 24, 4, "int"))
    if array != expected_array: raise UnbuiltCitiesParseError(f"PDB Array<UnbuiltCity> fields disagree: {array!r}")
    row = _fields(classes["UnbuiltCity"])
    if row != (("o", 0, 2, "short"), ("who", 2, 1, "char")): raise UnbuiltCitiesParseError(f"PDB UnbuiltCity fields disagree: {row!r}")
    receipt["selectors"] = {"tag": None, "array_count": PLAYER_LIST_COUNT, "row_direct": [0, ROW_LOGICAL_SIZE], "row_stride": ROW_MEMORY_SIZE}
    return _Layout(hashlib.sha256(json.dumps(receipt, sort_keys=True, separators=(",", ":")).encode()).hexdigest())


class _Reader:
    def __init__(self, data: bytes | bytearray | memoryview, offset: int):
        self.data = memoryview(data)
        if offset < 0 or offset > len(self.data): raise UnbuiltCitiesParseError(f"offset {offset:#x} is outside the stream")
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data): raise UnbuiltCitiesParseError(f"{what} range [{self.pos:#x},{end:#x}) exceeds {len(self.data):#x}-byte stream")
        start = self.pos; self.pos = end; return self.data[start:end]

    def u8(self, what: str) -> int: return self.take(1, what)[0]
    def i16(self, what: str) -> int: return struct.unpack("<h", self.take(2, what))[0]
    def i32(self, what: str) -> int: return struct.unpack("<i", self.take(4, what))[0]


def _count(reader: _Reader, what: str) -> int:
    value = reader.i32(what)
    if value < 0 or value > MAX_COUNT: raise UnbuiltCitiesParseError(f"invalid {what} {value}")
    return value


def _array(reader: _Reader, player: int) -> UnbuiltCityArray:
    offset = reader.pos; length = _count(reader, f"lists[{player}] length"); capacity = increment = flags = None; rows: tuple[UnbuiltCityImage, ...] = ()
    if length:
        capacity = _count(reader, f"lists[{player}] capacity"); increment = reader.i16(f"lists[{player}] increment"); flags = reader.u8(f"lists[{player}] flags")
        if capacity < length or flags & 0x40: raise UnbuiltCitiesParseError(f"invalid lists[{player}] history")
        images = []
        for index in range(length):
            row_offset = reader.pos; raw = bytes(reader.take(ROW_LOGICAL_SIZE, f"lists[{player}][{index}] logical bytes")); object_id, who = struct.unpack("<hb", raw)
            images.append(UnbuiltCityImage(index, row_offset, reader.pos, object_id, who, raw, hashlib.sha256(raw).hexdigest()))
        rows = tuple(images)
    return UnbuiltCityArray(player, offset, reader.pos, length, capacity, increment, flags, rows, hashlib.sha256(reader.data[offset:reader.pos]).hexdigest())


def parse_unbuilt_cities_section(data: bytes | bytearray | memoryview, offset: int, *, schema_path: pathlib.Path = DEFAULT_SCHEMA_PATH) -> UnbuiltCitiesSection:
    """Parse all eight untagged player lists, stopping at UnbuiltForts."""
    layout = _load_layout(str(pathlib.Path(schema_path).resolve())); reader = _Reader(data, offset); lists = tuple(_array(reader, player) for player in range(PLAYER_LIST_COUNT))
    return UnbuiltCitiesSection(offset, reader.pos, lists, hashlib.sha256(reader.data[offset:reader.pos]).hexdigest(), layout.sha256)


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes(); return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__); parser.add_argument("file", type=pathlib.Path); parser.add_argument("--offset", required=True, type=lambda text: int(text, 0)); args = parser.parse_args(argv); section = parse_unbuilt_cities_section(_load(args.file), args.offset)
    print(f"{args.file}: UnbuiltCities {section.offset:#x}..{section.end:#x} ({section.size} bytes) sha256={section.sha256}\n  lengths={[array.length for array in section.lists]}\n  next owner begins at {section.end:#x}: UnbuiltForts::walk_data tag")
    return 0


if __name__ == "__main__": raise SystemExit(main())
