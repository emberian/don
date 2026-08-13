#!/usr/bin/env python3
"""Parse complete retail ``SelectGroups::walk_data`` save images."""

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


TAG_SELECT_GROUPS = 0
TAG_SELECT_GROUP = 0
TAG_SELECT_GROUPS_INDEX = 6009
TAG_SELECT_GROUP_INDEX = 6007
SELECT_LIST_COUNT = 2
GROUP_MEMBER_CAPACITY = 128
SELECT_GROUP_TAIL_SIZE = 24
MAX_ARRAY_LENGTH = 1 << 20
DEFAULT_SCHEMA_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"


class SelectGroupsParseError(ValueError):
    """The stream or PDB layout contradicts ``SelectGroups::walk_data``."""


@dataclasses.dataclass(frozen=True)
class GroupImage:
    offset: int
    end: int
    fixed_values: tuple[int, ...]
    facing: int
    buildings: int
    who: int
    march: int
    member_ids: tuple[int, ...]
    off_x: tuple[int, ...]
    off_y: tuple[int, ...]
    curr_x: tuple[int, ...]
    curr_y: tuple[int, ...]
    angles: tuple[int, ...]
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset

    @property
    def num(self) -> int:
        return self.fixed_values[2]


@dataclasses.dataclass(frozen=True)
class SelectGroupImage:
    index: int
    offset: int
    end: int
    group: GroupImage
    tag: int
    whose: int
    flashing: int
    named: int
    item_ox: int
    good_ox: int
    flash_frame: int
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class SelectGroupArray:
    array_index: int
    offset: int
    end: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    rows: tuple[SelectGroupImage, ...]
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class SelectGroupsSection:
    offset: int
    end: int
    tag: int
    arrays: tuple[SelectGroupArray, ...]
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class _Layout:
    sha256: str


GROUP_FIELDS = (
    ("id", 4, 4, "int"), ("army", 8, 4, "int"), ("num", 12, 4, "int"),
    ("form", 16, 4, "int"), ("stamp", 20, 4, "int"), ("ox", 24, 4, "Coord"),
    ("oy", 28, 4, "Coord"), ("o_dist", 32, 4, "int"), ("o_angle", 36, 4, "int"),
    ("disband", 40, 4, "int"), ("order_num", 44, 4, "int"),
    ("priority", 48, 4, "int"), ("role", 52, 4, "int"),
    ("think_frame", 56, 4, "int"), ("new_speed", 60, 4, "int"),
    ("speed", 64, 4, "int"), ("form_num", 68, 4, "int"),
    ("facing", 72, 1, "unsigned char"), ("buildings", 73, 1, "unsigned char"),
    ("who", 74, 1, "unsigned char"), ("march", 75, 1, "unsigned char"),
    ("off_x", 76, 512, "int[128]"), ("off_y", 588, 512, "int[128]"),
    ("curr_x", 1100, 512, "Coord[128]"), ("curr_y", 1612, 512, "Coord[128]"),
    ("angles", 2124, 128, "char[128]"), ("list", 2252, 256, "short[128]"),
)

SELECT_GROUP_FIELDS = (
    ("whose", 2512, 4, "int"), ("flashing", 2516, 4, "int"),
    ("named", 2520, 4, "int"), ("item_ox", 2524, 4, "int"),
    ("good_ox", 2528, 4, "int"), ("flash_frame", 2532, 4, "int"),
    ("hilited", 2536, 4, "int"),
)

ARRAY_FIELDS = (
    ("length", 4, 4, "int"), ("size", 8, 4, "int"),
    ("increment", 12, 2, "short"), ("list", 16, 4, "SelectGroup*"),
    ("flags", 20, 1, "unsigned char"), ("cur_index", 24, 4, "int"),
)


def _fields(record: dict[str, object]) -> tuple[tuple[object, ...], ...]:
    return tuple((field["name"], field["offset"], field["size"], field["type"]) for field in record["flattened"])


@functools.lru_cache(maxsize=4)
def _load_layout(path_text: str) -> _Layout:
    path = pathlib.Path(path_text)
    try:
        classes = json.loads(path.read_bytes())["classes"]
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise SelectGroupsParseError(f"cannot load SelectGroups PDB layout from {path}: {error}") from error
    expected = {
        "SelectGroups": (60, (("select_list", 4, 28, "Array<SelectGroup>"), ("select_list_2", 32, 28, "Array<SelectGroup>"))),
        "Array<SelectGroup>": (28, ARRAY_FIELDS),
        "GroupData": (2508, GROUP_FIELDS),
        "GroupOut": (2512, GROUP_FIELDS),
        "Group": (2516, GROUP_FIELDS),
        "SelectGroup": (2544, GROUP_FIELDS + SELECT_GROUP_FIELDS),
    }
    receipt: dict[str, object] = {}
    for name, (size, fields) in expected.items():
        record = classes.get(name)
        actual = _fields(record) if record else ()
        if not record or record.get("size") != size or actual != fields:
            raise SelectGroupsParseError(f"PDB {name} layout disagrees: size={record.get('size') if record else None}, fields={actual!r}")
        receipt[name] = {"size": size, "flattened": fields}
    bases = {
        name: tuple((base["name"], base["offset"], base["size"]) for base in classes[name]["bases"])
        for name in ("SelectGroups", "Array<SelectGroup>", "SelectGroup")
    }
    expected_bases = {
        "SelectGroups": (("GameAccessConst", 4, 1),),
        "Array<SelectGroup>": (("ArrayBaseSimpleCopy<SelectGroup>", 0, 24),),
        "SelectGroup": (("Group", 0, 2516),),
    }
    if bases != expected_bases:
        raise SelectGroupsParseError(f"PDB SelectGroups bases disagree: {bases!r}")
    receipt["bases"] = bases
    receipt["selectors"] = {
        "outer_tag": TAG_SELECT_GROUPS_INDEX,
        "row_tag": TAG_SELECT_GROUP_INDEX,
        "array_count": SELECT_LIST_COUNT,
        "group_direct": [4, 76],
        "group_member_capacity": GROUP_MEMBER_CAPACITY,
        "select_group_direct": [2512, 2536],
        "select_group_stride": 2544,
        "next_owner": "Options.Array<Option>",
    }
    return _Layout(hashlib.sha256(json.dumps(receipt, sort_keys=True, separators=(",", ":")).encode()).hexdigest())


class _Reader:
    def __init__(self, data: bytes | bytearray | memoryview, offset: int):
        self.data = memoryview(data)
        if offset < 0 or offset > len(self.data):
            raise SelectGroupsParseError(f"offset {offset:#x} is outside the stream")
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise SelectGroupsParseError(f"{what} range [{self.pos:#x},{end:#x}) exceeds {len(self.data):#x}-byte stream")
        start = self.pos
        self.pos = end
        return self.data[start:end]

    def u8(self, what: str) -> int: return self.take(1, what)[0]
    def i8(self, what: str) -> int: return struct.unpack("<b", self.take(1, what))[0]
    def i16(self, what: str) -> int: return struct.unpack("<h", self.take(2, what))[0]
    def i32(self, what: str) -> int: return struct.unpack("<i", self.take(4, what))[0]


def _parse_group(reader: _Reader, array_index: int, row_index: int) -> GroupImage:
    prefix = f"select_list[{array_index}][{row_index}].Group"
    start = reader.pos
    fixed_values = tuple(reader.i32(f"{prefix}.{GROUP_FIELDS[index][0]}") for index in range(17))
    facing = reader.u8(f"{prefix}.facing")
    buildings = reader.u8(f"{prefix}.buildings")
    who = reader.u8(f"{prefix}.who")
    march = reader.u8(f"{prefix}.march")
    num = fixed_values[2]
    if num < 0 or num > GROUP_MEMBER_CAPACITY:
        raise SelectGroupsParseError(f"{prefix}.num {num} exceeds fixed member capacity {GROUP_MEMBER_CAPACITY}")
    member_ids = tuple(reader.i16(f"{prefix}.list[{slot}]") for slot in range(num))
    off_x = tuple(reader.i32(f"{prefix}.off_x[{slot}]") for slot in range(num))
    off_y = tuple(reader.i32(f"{prefix}.off_y[{slot}]") for slot in range(num))
    curr_x = tuple(reader.i32(f"{prefix}.curr_x[{slot}]") for slot in range(num))
    curr_y = tuple(reader.i32(f"{prefix}.curr_y[{slot}]") for slot in range(num))
    angles = tuple(reader.i8(f"{prefix}.angles[{slot}]") for slot in range(num))
    return GroupImage(start, reader.pos, fixed_values, facing, buildings, who, march, member_ids, off_x, off_y, curr_x, curr_y, angles, hashlib.sha256(reader.data[start:reader.pos]).hexdigest())


def _parse_row(reader: _Reader, array_index: int, row_index: int, require_tags: bool) -> SelectGroupImage:
    start = reader.pos
    group = _parse_group(reader, array_index, row_index)
    tag = reader.u8(f"select_list[{array_index}][{row_index}] tag")
    if require_tags and tag != TAG_SELECT_GROUP:
        raise SelectGroupsParseError(f"SelectGroup tag {tag:#04x} != {TAG_SELECT_GROUP:#04x}")
    values = tuple(reader.i32(f"select_list[{array_index}][{row_index}].{name}") for name, *_ in SELECT_GROUP_FIELDS[:6])
    return SelectGroupImage(row_index, start, reader.pos, group, tag, *values, hashlib.sha256(reader.data[start:reader.pos]).hexdigest())


def _parse_array(reader: _Reader, array_index: int, require_tags: bool) -> SelectGroupArray:
    start = reader.pos
    length = reader.i32(f"select_list[{array_index}] length")
    if length < 0 or length > MAX_ARRAY_LENGTH:
        raise SelectGroupsParseError(f"select_list[{array_index}] has invalid length {length}")
    if not length:
        return SelectGroupArray(array_index, start, reader.pos, 0, None, None, None, (), hashlib.sha256(reader.data[start:reader.pos]).hexdigest())
    capacity = reader.i32(f"select_list[{array_index}] capacity")
    increment = reader.i16(f"select_list[{array_index}] increment")
    flags = reader.u8(f"select_list[{array_index}] flags")
    if capacity < length or capacity > MAX_ARRAY_LENGTH or flags & 0x40:
        raise SelectGroupsParseError(f"select_list[{array_index}] has invalid history length={length}, capacity={capacity}, flags={flags:#04x}")
    rows = tuple(_parse_row(reader, array_index, row_index, require_tags) for row_index in range(length))
    return SelectGroupArray(array_index, start, reader.pos, length, capacity, increment, flags, rows, hashlib.sha256(reader.data[start:reader.pos]).hexdigest())


def parse_select_groups_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tags: bool = True,
    schema_path: pathlib.Path = DEFAULT_SCHEMA_PATH,
) -> SelectGroupsSection:
    """Parse the tag and two arrays, stopping before Options' Array<Option>."""
    layout = _load_layout(str(pathlib.Path(schema_path).resolve()))
    reader = _Reader(data, offset)
    tag = reader.u8("SelectGroups tag")
    if require_tags and tag != TAG_SELECT_GROUPS:
        raise SelectGroupsParseError(f"SelectGroups tag {tag:#04x} != {TAG_SELECT_GROUPS:#04x}")
    arrays = tuple(_parse_array(reader, index, require_tags) for index in range(SELECT_LIST_COUNT))
    return SelectGroupsSection(offset, reader.pos, tag, arrays, hashlib.sha256(reader.data[offset:reader.pos]).hexdigest(), layout.sha256)


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument("--offset", required=True, type=lambda text: int(text, 0))
    args = parser.parse_args(argv)
    section = parse_select_groups_section(_load(args.file), args.offset)
    print(
        f"{args.file}: SelectGroups {section.offset:#x}..{section.end:#x} "
        f"({section.size} bytes) sha256={section.sha256}\n"
        f"  lengths={[array.length for array in section.arrays]}\n"
        f"  next owner begins at {section.end:#x}: Options Array<Option>"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
