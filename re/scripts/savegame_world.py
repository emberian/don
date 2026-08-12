#!/usr/bin/env python3
"""Parse the complete retail ``World::walk_data(-1)`` save image.

The helper begins at the World tag, follows all selector phases active for -1,
including six coordinate arrays, direct WorldData ranges, map planes, optional
CollBlocks, and four Terrain arrays, then stops before the caller's direct
``GameDaemon`` range.
"""

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


TAG_WORLD = 0x00
TAG_STRING_TABLE_INDEX = 2924
MAX_COUNT = 1 << 24
COLL_BLOCK_BYTES = 96
DEFAULT_SCHEMA_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"


class WorldParseError(ValueError):
    """The stream or PDB layout contradicts World::walk_data(-1)."""


@dataclasses.dataclass(frozen=True)
class PodArray:
    name: str
    offset: int
    end: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    element_size: int
    data_offset: int | None
    data: bytes
    sha256: str


@dataclasses.dataclass(frozen=True)
class CollBlockImage:
    index: int
    offset: int
    end: int
    present: int
    bits: int | None
    payload_size: int | None
    payload: bytes
    sha256: str


@dataclasses.dataclass(frozen=True)
class WorldSection:
    offset: int
    end: int
    tag: int
    xs: int
    ys: int
    coordinate_arrays: tuple[PodArray, ...]
    direct_offset: int
    direct_words: tuple[int, ...]
    wdata_offset: int
    wdata_rows: tuple[bytes, ...]
    tdata_offset: int
    tdata: bytes
    seen_offset: int
    seen_planes: tuple[bytes, ...]
    wcoord_seen_offset: int
    wcoord_seen: bytes
    danger_offset: int
    danger_planes: tuple[bytes, ...]
    collision_offset: int
    collision_blocks: tuple[CollBlockImage, ...]
    terrain_arrays: tuple[PodArray, ...]
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset

    def direct(self, byte_offset: int) -> int:
        if byte_offset < 8 or byte_offset >= 128 or byte_offset % 4:
            raise KeyError(byte_offset)
        return self.direct_words[(byte_offset - 8) // 4]


@dataclasses.dataclass(frozen=True)
class _Layout:
    sha256: str


_WORLD_FIELDS = (
    ("xs", 0, 4, "int"), ("ys", 4, 4, "int"), ("size", 8, 4, "int"),
    ("fog_xs", 12, 4, "int"), ("fog_ys", 16, 4, "int"), ("fog_size", 20, 4, "int"),
    ("tile_xs", 24, 4, "int"), ("tile_ys", 28, 4, "int"), ("tile_size", 32, 4, "int"),
    ("reg_xs", 36, 4, "int"), ("reg_ys", 40, 4, "int"), ("reg_size", 44, 4, "int"),
    ("map", 48, 4, "int"), ("sea_map", 52, 4, "int"),
    ("player_territory_limit", 56, 4, "int"), ("player_territory_limit_civic", 60, 4, "int"),
    ("player_territory_limit_city", 64, 4, "int"), ("colonized_territory_limit", 68, 4, "int"),
    ("colonized_territory_limit_civic", 72, 4, "int"), ("colonized_territory_limit_city", 76, 4, "int"),
    ("player_reg", 80, 4, "int"), ("resource_reg", 84, 4, "int"),
    ("forest_size", 88, 4, "int"), ("mountain_size", 92, 4, "int"),
    ("rock_size", 96, 4, "int"), ("total_metal", 100, 4, "int"),
    ("total_oil", 104, 4, "int"), ("goodies", 108, 4, "int"),
    ("land_resources", 112, 4, "int"), ("sea_resources", 116, 4, "int"),
    ("land_size", 120, 4, "int"), ("seed", 124, 4, "int"),
    ("start_x", 128, 28, "SimpleArray<WCoord>"), ("start_y", 156, 28, "SimpleArray<WCoord>"),
    ("start_city_x", 184, 28, "SimpleArray<WCoord>"), ("start_city_y", 212, 28, "SimpleArray<WCoord>"),
    ("start_city_locs", 240, 12, "DynamicBitMask"),
    ("oil_x", 252, 28, "SimpleArray<WCoord>"), ("oil_y", 280, 28, "SimpleArray<WCoord>"),
    ("wdata", 308, 4, "WData*"), ("tdata", 312, 4, "TData*"),
    ("danger", 316, 32, "int*[8]"), ("seen", 348, 4, "unsigned char*"),
    ("seen2", 352, 4, "unsigned char*"), ("seen3", 356, 4, "unsigned char*"),
    ("wcoord_seen", 360, 4, "unsigned char*"),
)
_ARRAY_WCOORD = (
    ("length", 4, 4, "int"), ("size", 8, 4, "int"), ("increment", 12, 2, "short"),
    ("list", 16, 4, "WCoord*"), ("flags", 20, 1, "unsigned char"),
    ("cur_index", 24, 4, "int"),
)
_ARRAY_WCOORD_DATA = tuple(
    (name, offset, size, "WCoordData*" if name == "list" else type_name)
    for name, offset, size, type_name in _ARRAY_WCOORD
)
_ARRAY_INT = tuple(
    (name, offset, size, "int*" if name == "list" else type_name)
    for name, offset, size, type_name in _ARRAY_WCOORD
)
_WDATA = (
    ("flags", 0, 2, "unsigned short"), ("land", 2, 1, "char"),
    ("land_sub", 3, 1, "unsigned char"), ("region", 4, 2, "short"),
    ("region2", 6, 2, "short"), ("down", 8, 2, "short"),
    ("down_who", 10, 2, "short"), ("val", 12, 1, "unsigned char"),
    ("goods", 13, 1, "unsigned char"), ("light", 14, 1, "unsigned char"),
    ("who", 15, 1, "char"), ("who2", 16, 1, "char"),
    ("blocked", 17, 1, "unsigned char"), ("bad", 18, 1, "unsigned char"),
    ("solid", 19, 1, "char"), ("was_seen", 20, 1, "unsigned char"),
    ("block", 24, 4, "CollBlock*"),
)
_COLL = (
    ("bits", 0, 4, "int"), ("size", 4, 4, "int"),
    ("flags", 8, 4, "int"), ("ptr", 12, 96, "unsigned char[96]"),
)


def _fields(record: dict[str, object]) -> tuple[tuple[object, ...], ...]:
    return tuple(
        (field["name"], field["offset"], field["size"], field["type"])
        for field in record["flattened"]  # type: ignore[index]
    )


@functools.lru_cache(maxsize=None)
def _load_layout_cached(path_text: str) -> _Layout:
    path = pathlib.Path(path_text)
    try:
        classes = json.loads(path.read_text())["classes"]
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise WorldParseError(f"cannot load World PDB layout from {path}: {error}") from error
    expected = {
        "World": (372, _WORLD_FIELDS), "WorldData": (364, _WORLD_FIELDS),
        "WorldOut": (368, _WORLD_FIELDS), "SimpleArray<WCoord>": (28, _ARRAY_WCOORD),
        "Array<WCoordData>": (28, _ARRAY_WCOORD_DATA), "SimpleArray<int>": (28, _ARRAY_INT),
        "WData": (28, _WDATA), "TData": (2, (("mask", 0, 2, "unsigned short"),)),
        "CollBlock": (108, _COLL),
    }
    receipt: dict[str, object] = {}
    for name, (size, fields) in expected.items():
        record = classes.get(name)
        actual = _fields(record) if record else ()
        if not record or record.get("size") != size or actual != fields:
            raise WorldParseError(
                f"PDB {name} layout disagrees: size={record.get('size') if record else None}, fields={actual!r}"
            )
        receipt[name] = {"size": size, "flattened": actual}
    terrain = classes.get("Terrain")
    terrain_fields = {field["name"]: (field["offset"], field["size"], field["type"]) for field in terrain["flattened"]} if terrain else {}
    terrain_expected = {
        "halfland_locs": (19328, 28, "WCoordList"),
        "halfland_types": (19356, 28, "SimpleArray<int>"),
        "halfland_subtypes": (19384, 28, "SimpleArray<int>"),
        "nuke_hits": (19412, 28, "SimpleArray<int>"),
    }
    if not terrain or terrain.get("size") != 27336 or any(terrain_fields.get(name) != value for name, value in terrain_expected.items()):
        raise WorldParseError("PDB Terrain tail layout disagrees")
    receipt["Terrain-tail"] = terrain_expected
    return _Layout(hashlib.sha256(json.dumps(receipt, sort_keys=True, separators=(",", ":")).encode()).hexdigest())


class _Reader:
    def __init__(self, data: bytes | bytearray | memoryview, offset: int):
        self.data = memoryview(data)
        if offset < 0 or offset > len(self.data):
            raise WorldParseError(f"offset {offset:#x} is outside {len(self.data):#x}-byte stream")
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise WorldParseError(f"{what} range [{self.pos:#x},{end:#x}) exceeds {len(self.data):#x}-byte stream")
        start = self.pos
        self.pos = end
        return self.data[start:end]

    def u8(self, what: str) -> int: return self.take(1, what)[0]
    def i16(self, what: str) -> int: return struct.unpack("<h", self.take(2, what))[0]
    def i32(self, what: str) -> int: return struct.unpack("<i", self.take(4, what))[0]


def _count(reader: _Reader, what: str, maximum: int = MAX_COUNT) -> int:
    value = reader.i32(what)
    if value < 0 or value > maximum:
        raise WorldParseError(f"invalid {what} {value}")
    return value


def _array(reader: _Reader, name: str, element_size: int) -> PodArray:
    offset = reader.pos
    length = _count(reader, f"{name} length")
    capacity = increment = flags = data_offset = None
    data = b""
    if length:
        capacity = _count(reader, f"{name} capacity")
        if capacity < length:
            raise WorldParseError(f"{name} capacity {capacity} is below length {length}")
        increment = reader.i16(f"{name} increment")
        flags = reader.u8(f"{name} flags")
        if flags & 0x40:
            raise WorldParseError(f"{name} flags retain writer-cleared 0x40 bit")
        data_offset = reader.pos
        data = bytes(reader.take(length * element_size, f"{name} data"))
    return PodArray(name, offset, reader.pos, length, capacity, increment, flags, element_size, data_offset, data, hashlib.sha256(reader.data[offset:reader.pos]).hexdigest())


def parse_world_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tag: bool = True,
    schema_path: pathlib.Path | None = None,
) -> WorldSection:
    """Parse every phase selected by retail ``World::walk_data(..., -1)``."""
    layout = _load_layout_cached(str((schema_path or DEFAULT_SCHEMA_PATH).resolve()))
    reader = _Reader(data, offset)
    tag = reader.u8("World tag")
    if require_tag and tag != TAG_WORLD:
        raise WorldParseError(f"World tag {tag:#04x} != {TAG_WORLD:#04x} at {offset:#x}")
    xs, ys = reader.i32("World.xs"), reader.i32("World.ys")
    coordinate_arrays = tuple(
        _array(reader, name, 4)
        for name in ("World.start_x", "World.start_y", "World.start_city_x", "World.start_city_y", "World.oil_x", "World.oil_y")
    )
    direct_offset = reader.pos
    direct_words = tuple(reader.i32(f"World direct +{8 + 4 * i}") for i in range(30))
    size, fog_size, tile_size, reg_size = direct_words[0], direct_words[3], direct_words[6], direct_words[9]
    for name, value in (("size", size), ("fog_size", fog_size), ("tile_size", tile_size), ("reg_size", reg_size)):
        if value < 0 or value > MAX_COUNT:
            raise WorldParseError(f"invalid World.{name} {value}")
    wdata_offset = reader.pos
    wdata_rows = tuple(bytes(reader.take(21, f"World.wdata[{i}] +0..+21")) for i in range(size))
    tdata_offset = reader.pos
    tdata = bytes(reader.take(2 * tile_size, "World.tdata"))
    seen_offset = reader.pos
    seen_planes = tuple(bytes(reader.take(fog_size, f"World.seen{suffix}")) for suffix in ("", "2", "3"))
    wcoord_seen_offset = reader.pos
    wcoord_seen = bytes(reader.take(size, "World.wcoord_seen"))
    danger_offset = reader.pos
    danger_planes = tuple(bytes(reader.take(4 * reg_size, f"World.danger[{i}]")) for i in range(8))
    collision_offset = reader.pos
    collision_blocks = []
    for index in range(size):
        block_offset = reader.pos
        present = reader.i32(f"World.wdata[{index}].block presence")
        if present not in (0, 1):
            raise WorldParseError(f"World.wdata[{index}].block presence is not boolean")
        bits = payload_size = None
        payload = b""
        if present:
            bits = reader.i32(f"CollBlock[{index}].bits")
            payload_size = reader.i32(f"CollBlock[{index}].size")
            if bits < 0 or bits > COLL_BLOCK_BYTES * 8 or payload_size < 0 or payload_size > COLL_BLOCK_BYTES:
                raise WorldParseError(f"CollBlock[{index}] dimensions {bits}/{payload_size} exceed PDB storage")
            payload = bytes(reader.take(payload_size, f"CollBlock[{index}].ptr"))
        collision_blocks.append(CollBlockImage(index, block_offset, reader.pos, present, bits, payload_size, payload, hashlib.sha256(reader.data[block_offset:reader.pos]).hexdigest()))
    terrain_arrays = (
        _array(reader, "Terrain.halfland_locs", 8),
        _array(reader, "Terrain.halfland_types", 4),
        _array(reader, "Terrain.halfland_subtypes", 4),
        _array(reader, "Terrain.nuke_hits", 4),
    )
    return WorldSection(offset, reader.pos, tag, xs, ys, coordinate_arrays, direct_offset, direct_words, wdata_offset, wdata_rows, tdata_offset, tdata, seen_offset, seen_planes, wcoord_seen_offset, wcoord_seen, danger_offset, danger_planes, collision_offset, tuple(collision_blocks), terrain_arrays, hashlib.sha256(reader.data[offset:reader.pos]).hexdigest(), layout.sha256)


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def _jsonable(value: object) -> object:
    if dataclasses.is_dataclass(value):
        result = {}
        for field in dataclasses.fields(value):
            item = getattr(value, field.name)
            result[field.name] = (None if item is None else f"0x{item:x}") if field.name.endswith("offset") or field.name == "end" else _jsonable(item)
        if hasattr(value, "size"): result["size"] = getattr(value, "size")
        return result
    if isinstance(value, tuple): return [_jsonable(item) for item in value]
    if isinstance(value, bytes): return value.hex()
    return value


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument("--offset", required=True, type=lambda text: int(text, 0))
    parser.add_argument("--schema", type=pathlib.Path, default=DEFAULT_SCHEMA_PATH)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    section = parse_world_section(_load(args.file), args.offset, schema_path=args.schema)
    if args.json:
        print(json.dumps(_jsonable(section), indent=2))
    else:
        print(f"{args.file}: World {section.offset:#x}..{section.end:#x} ({section.size} bytes) sha256={section.sha256}\n  PDB-layout sha256={section.layout_sha256}\n  size={section.direct(8)}, fog_size={section.direct(20)}, tile_size={section.direct(32)}, reg_size={section.direct(44)}\n  next owner begins at {section.end:#x} (caller GameDaemon direct range)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
