#!/usr/bin/env python3
"""Parse the complete caller-owned ``Farms``/``Array<FarmStruct>`` save image.

Retail first writes the ten logical bytes of ``Farms::start_color``, then the
two wheat-height words, and finally dispatches ``Array<FarmStruct>::walk_data``.
Each live array row contributes 190 logical bytes from a 192-byte in-memory
``FarmStruct``; the two trailing structure-padding bytes are not serialized.
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


TAG_FARMS = 4
TAG_STRING_TABLE_INDEX = 2672
COLOR_LOGICAL_SIZE = 10
FARM_STRUCT_LOGICAL_SIZE = 190
FARM_STRUCT_MEMORY_SIZE = 192
MAX_COUNT = 1 << 20
DEFAULT_SCHEMA_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"


class FarmStructsParseError(ValueError):
    """The stream or PDB layout contradicts the caller's Farms tranche."""


@dataclasses.dataclass(frozen=True)
class ColorImage:
    offset: int
    end: int
    raw: bytes
    red: int
    green: int
    blue: int
    alpha: int
    rgb: int
    w_555: int
    w_565: int
    flags: int
    index: int
    sha256: str


@dataclasses.dataclass(frozen=True)
class FarmStructImage:
    index: int
    offset: int
    end: int
    who: int
    object_id: int
    percent_bits: tuple[int, ...]
    terrain_height_bits: tuple[int, ...]
    status: bytes
    valid: int
    farm_type: int
    raw: bytes
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class FarmStructArray:
    offset: int
    end: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    rows: tuple[FarmStructImage, ...]
    sha256: str


@dataclasses.dataclass(frozen=True)
class FarmStructsSection:
    offset: int
    end: int
    tag: int
    start_color: ColorImage
    wheat_max_height_bits: int
    wheat_min_height_bits: int
    farm_data: FarmStructArray
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class _Layout:
    sha256: str


def _fields(record: dict[str, object]) -> tuple[tuple[object, ...], ...]:
    return tuple(
        (field["name"], field["offset"], field["size"], field["type"])
        for field in record["flattened"]
    )


@functools.lru_cache(maxsize=4)
def _load_layout(path_text: str) -> _Layout:
    path = pathlib.Path(path_text)
    try:
        classes = json.loads(path.read_bytes())["classes"]
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise FarmStructsParseError(f"cannot load Farms PDB layout from {path}: {error}") from error

    expected_sizes = {
        "Farms": 480,
        "Color": 12,
        "FarmStruct": FARM_STRUCT_MEMORY_SIZE,
        "Array<FarmStruct>": 28,
    }
    receipt: dict[str, object] = {}
    for name, size in expected_sizes.items():
        record = classes.get(name)
        if not record or record.get("size") != size:
            raise FarmStructsParseError(
                f"PDB {name} size disagrees: {record.get('size') if record else None}"
            )
        receipt[name] = {"size": size, "flattened": _fields(record)}

    farms = {name: (offset, size, kind) for name, offset, size, kind in _fields(classes["Farms"])}
    expected_farms = {
        "start_color": (64, 12, "Color"),
        "wheat_max_height": (76, 4, "float"),
        "wheat_min_height": (80, 4, "float"),
        "farm_data": (84, 28, "Array<FarmStruct>"),
    }
    if any(farms.get(name) != value for name, value in expected_farms.items()):
        raise FarmStructsParseError("PDB Farms serialized fields disagree")

    color = _fields(classes["Color"])
    expected_color = (
        ("red", 0, 1, "unsigned char"),
        ("rgb", 0, 4, "unsigned long"),
        ("green", 1, 1, "unsigned char"),
        ("blue", 2, 1, "unsigned char"),
        ("alpha", 3, 1, "unsigned char"),
        ("w_555", 4, 2, "unsigned short"),
        ("w_565", 6, 2, "unsigned short"),
        ("flags", 8, 1, "unsigned char"),
        ("index", 9, 1, "unsigned char"),
    )
    if color != expected_color:
        raise FarmStructsParseError(f"PDB Color layout disagrees: {color!r}")

    row = _fields(classes["FarmStruct"])
    expected_row = (
        ("who", 0, 4, "int"),
        ("o", 4, 4, "int"),
        ("percent", 8, 64, "float[4][4]"),
        ("terrain_height", 72, 100, "float[5][5]"),
        ("status", 172, 16, "unsigned char[4][4]"),
        ("valid", 188, 1, "unsigned char"),
        ("farm_type", 189, 1, "unsigned char"),
    )
    if row != expected_row:
        raise FarmStructsParseError(f"PDB FarmStruct layout disagrees: {row!r}")
    if row[-1][1] + row[-1][2] != FARM_STRUCT_LOGICAL_SIZE:
        raise FarmStructsParseError("PDB FarmStruct logical prefix does not end at +0xbe")

    array = _fields(classes["Array<FarmStruct>"])
    expected_array = (
        ("length", 4, 4, "int"),
        ("size", 8, 4, "int"),
        ("increment", 12, 2, "short"),
        ("list", 16, 4, "FarmStruct*"),
        ("flags", 20, 1, "unsigned char"),
        ("cur_index", 24, 4, "int"),
    )
    if array != expected_array:
        raise FarmStructsParseError(f"PDB Array<FarmStruct> layout disagrees: {array!r}")

    receipt["selectors"] = {
        "caller_direct": [[64, 74], [76, 84]],
        "array": [84, 112],
        "row_direct": [0, FARM_STRUCT_LOGICAL_SIZE],
        "row_stride": FARM_STRUCT_MEMORY_SIZE,
    }
    encoded = json.dumps(receipt, sort_keys=True, separators=(",", ":")).encode()
    return _Layout(hashlib.sha256(encoded).hexdigest())


class _Reader:
    def __init__(self, data: bytes | bytearray | memoryview, offset: int):
        self.data = memoryview(data)
        if offset < 0 or offset > len(self.data):
            raise FarmStructsParseError(f"offset {offset:#x} is outside the stream")
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise FarmStructsParseError(
                f"{what} range [{self.pos:#x},{end:#x}) exceeds {len(self.data):#x}-byte stream"
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


def _count(reader: _Reader, what: str) -> int:
    value = reader.i32(what)
    if value < 0 or value > MAX_COUNT:
        raise FarmStructsParseError(f"invalid {what} {value}")
    return value


def _color(reader: _Reader) -> ColorImage:
    offset = reader.pos
    raw = bytes(reader.take(COLOR_LOGICAL_SIZE, "Farms.start_color logical bytes"))
    red, green, blue, alpha = raw[:4]
    rgb, w_555, w_565 = struct.unpack_from("<IHH", raw)
    flags, index = raw[8:10]
    return ColorImage(
        offset,
        reader.pos,
        raw,
        red,
        green,
        blue,
        alpha,
        rgb,
        w_555,
        w_565,
        flags,
        index,
        hashlib.sha256(raw).hexdigest(),
    )


def _row(reader: _Reader, index: int) -> FarmStructImage:
    offset = reader.pos
    raw = bytes(reader.take(FARM_STRUCT_LOGICAL_SIZE, f"FarmStruct[{index}] logical bytes"))
    who, object_id = struct.unpack_from("<ii", raw)
    percent_bits = struct.unpack_from("<16I", raw, 8)
    terrain_height_bits = struct.unpack_from("<25I", raw, 72)
    status = raw[172:188]
    valid, farm_type = raw[188:190]
    return FarmStructImage(
        index,
        offset,
        reader.pos,
        who,
        object_id,
        percent_bits,
        terrain_height_bits,
        status,
        valid,
        farm_type,
        raw,
        hashlib.sha256(raw).hexdigest(),
    )


def _array(reader: _Reader) -> FarmStructArray:
    offset = reader.pos
    length = _count(reader, "farm_data length")
    capacity = increment = flags = None
    rows: tuple[FarmStructImage, ...] = ()
    if length:
        capacity = _count(reader, "farm_data capacity")
        increment = reader.i16("farm_data increment")
        flags = reader.u8("farm_data flags")
        if capacity < length or flags & 0x40:
            raise FarmStructsParseError("invalid farm_data history")
        rows = tuple(_row(reader, index) for index in range(length))
    return FarmStructArray(
        offset,
        reader.pos,
        length,
        capacity,
        increment,
        flags,
        rows,
        hashlib.sha256(reader.data[offset:reader.pos]).hexdigest(),
    )


def parse_farm_structs_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tag: bool = True,
    schema_path: pathlib.Path = DEFAULT_SCHEMA_PATH,
) -> FarmStructsSection:
    """Decode the Farms direct ranges and complete FarmStruct array."""

    layout = _load_layout(str(pathlib.Path(schema_path).resolve()))
    reader = _Reader(data, offset)
    tag = reader.u8("Farms tag")
    if require_tag and tag != TAG_FARMS:
        raise FarmStructsParseError(f"Farms tag {tag:#04x} != {TAG_FARMS:#04x}")
    color = _color(reader)
    wheat_max_height_bits = struct.unpack("<I", reader.take(4, "wheat_max_height"))[0]
    wheat_min_height_bits = struct.unpack("<I", reader.take(4, "wheat_min_height"))[0]
    farm_data = _array(reader)
    return FarmStructsSection(
        offset,
        reader.pos,
        tag,
        color,
        wheat_max_height_bits,
        wheat_min_height_bits,
        farm_data,
        hashlib.sha256(reader.data[offset:reader.pos]).hexdigest(),
        layout.sha256,
    )


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument("--offset", required=True, type=lambda text: int(text, 0))
    args = parser.parse_args(argv)
    section = parse_farm_structs_section(_load(args.file), args.offset)
    print(
        f"{args.file}: Farms/FarmStruct {section.offset:#x}..{section.end:#x} "
        f"({section.size} bytes) sha256={section.sha256}\n"
        f"  color={section.start_color.raw.hex()} "
        f"wheat_bits=({section.wheat_max_height_bits:#010x},"
        f"{section.wheat_min_height_bits:#010x}) farms={section.farm_data.length}\n"
        f"  next owner begins at {section.end:#x}: UnbuiltWonders::walk_data"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
