#!/usr/bin/env python3
"""Parse the exact retail HotKeyGroup array generic-save image.

The helper begins at the caller's StringTable[3963] tag, follows the complete
``Array<HotKeyGroup>`` history and every count-dependent ``Group`` row, and
stops before ``World::walk_data``.
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


TAG_HOTKEY_GROUPS = 0x00
TAG_HOTKEY_GROUP = 0x00
OUTER_TAG_STRING_TABLE_INDEX = 3963
ROW_TAG_STRING_TABLE_INDEX = 3962
MAX_GROUP_MEMBERS = 128
MAX_ARRAY_LENGTH = 1 << 20
DEFAULT_SCHEMA_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"


class HotKeyGroupsParseError(ValueError):
    """The stream or PDB layout contradicts the HotKeyGroup array walk."""


@dataclasses.dataclass(frozen=True)
class HotKeyGroupRow:
    index: int
    offset: int
    end: int
    core_offset: int
    core_words: tuple[int, ...]
    facing: int
    buildings: int
    who: int
    march: int
    list_offset: int | None
    members: tuple[int, ...]
    off_x: tuple[int, ...]
    off_y: tuple[int, ...]
    curr_x: tuple[int, ...]
    curr_y: tuple[int, ...]
    angles: tuple[int, ...]
    tag_offset: int
    tag: int
    loc_x_bits: int
    loc_y_bits: int
    valid: int
    sha256: str

    @property
    def num(self) -> int:
        return self.core_words[2]

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class HotKeyGroupsSection:
    offset: int
    end: int
    tag: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    rows: tuple[HotKeyGroupRow, ...]
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class _Layout:
    sha256: str


_ARRAY_FIELDS = (
    ("length", 4, 4, "int"),
    ("size", 8, 4, "int"),
    ("increment", 12, 2, "short"),
    ("list", 16, 4, "HotKeyGroup*"),
    ("flags", 20, 1, "unsigned char"),
    ("cur_index", 24, 4, "int"),
)
_GROUP_FIELDS = (
    ("id", 4, 4, "int"),
    ("army", 8, 4, "int"),
    ("num", 12, 4, "int"),
    ("form", 16, 4, "int"),
    ("stamp", 20, 4, "int"),
    ("ox", 24, 4, "Coord"),
    ("oy", 28, 4, "Coord"),
    ("o_dist", 32, 4, "int"),
    ("o_angle", 36, 4, "int"),
    ("disband", 40, 4, "int"),
    ("order_num", 44, 4, "int"),
    ("priority", 48, 4, "int"),
    ("role", 52, 4, "int"),
    ("think_frame", 56, 4, "int"),
    ("new_speed", 60, 4, "int"),
    ("speed", 64, 4, "int"),
    ("form_num", 68, 4, "int"),
    ("facing", 72, 1, "unsigned char"),
    ("buildings", 73, 1, "unsigned char"),
    ("who", 74, 1, "unsigned char"),
    ("march", 75, 1, "unsigned char"),
    ("off_x", 76, 512, "int[128]"),
    ("off_y", 588, 512, "int[128]"),
    ("curr_x", 1100, 512, "Coord[128]"),
    ("curr_y", 1612, 512, "Coord[128]"),
    ("angles", 2124, 128, "char[128]"),
    ("list", 2252, 256, "short[128]"),
)
_HOTKEY_DATA = _GROUP_FIELDS + (
    ("loc_x", 2512, 4, "float"),
    ("loc_y", 2516, 4, "float"),
    ("valid", 2520, 4, "int"),
)
_HOTKEY_OUT = _HOTKEY_DATA + (
    ("zoom_level", 2524, 4, "int"),
    ("name", 2528, 20, "String"),
    ("statwin_icon", 2548, 4, "int"),
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
        raise HotKeyGroupsParseError(
            f"cannot load HotKeyGroup PDB layout from {path}: {error}"
        ) from error
    expected = {
        "Array<HotKeyGroup>": (28, _ARRAY_FIELDS),
        "GroupData": (2508, _GROUP_FIELDS),
        "GroupOut": (2512, _GROUP_FIELDS),
        "Group": (2516, _GROUP_FIELDS),
        "HotKeyGroupData": (2528, _HOTKEY_DATA),
        "HotKeyGroupOut": (2556, _HOTKEY_OUT),
        "HotKeyGroup": (2556, _HOTKEY_OUT),
    }
    receipt: dict[str, object] = {}
    for name, (size, fields) in expected.items():
        try:
            record = classes[name]
        except KeyError as error:
            raise HotKeyGroupsParseError(f"PDB layout lacks {name}") from error
        actual = _fields(record)
        if record.get("size") != size or actual != fields:
            raise HotKeyGroupsParseError(
                f"PDB {name} layout disagrees: size={record.get('size')}, fields={actual!r}"
            )
        receipt[name] = {"size": size, "flattened": actual}
    return _Layout(
        hashlib.sha256(
            json.dumps(receipt, sort_keys=True, separators=(",", ":")).encode()
        ).hexdigest()
    )


def _load_layout(schema_path: pathlib.Path | None) -> _Layout:
    return _load_layout_cached(str((schema_path or DEFAULT_SCHEMA_PATH).resolve()))


class _Reader:
    def __init__(self, data: bytes | bytearray | memoryview, offset: int):
        self.data = memoryview(data)
        if offset < 0 or offset > len(self.data):
            raise HotKeyGroupsParseError(
                f"offset {offset:#x} is outside {len(self.data):#x}-byte stream"
            )
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise HotKeyGroupsParseError(
                f"{what} range [{self.pos:#x},{end:#x}) exceeds {len(self.data):#x}-byte stream"
            )
        start = self.pos
        self.pos = end
        return self.data[start:end]

    def u8(self, what: str) -> int:
        return self.take(1, what)[0]

    def i8(self, what: str) -> int:
        return struct.unpack("<b", self.take(1, what))[0]

    def i16(self, what: str) -> int:
        return struct.unpack("<h", self.take(2, what))[0]

    def i32(self, what: str) -> int:
        return struct.unpack("<i", self.take(4, what))[0]

    def u32(self, what: str) -> int:
        return struct.unpack("<I", self.take(4, what))[0]


def _count(reader: _Reader, what: str, maximum: int) -> int:
    value = reader.i32(what)
    if value < 0 or value > maximum:
        raise HotKeyGroupsParseError(f"invalid {what} {value}")
    return value


def _tag(reader: _Reader, expected: int, what: str, required: bool) -> int:
    offset = reader.pos
    value = reader.u8(what)
    if required and value != expected:
        raise HotKeyGroupsParseError(
            f"{what} {value:#04x} != {expected:#04x} at {offset:#x}"
        )
    return value


def _row(reader: _Reader, index: int, require_tags: bool) -> HotKeyGroupRow:
    offset = reader.pos
    core_offset = reader.pos
    core_words = tuple(reader.i32(f"HotKeyGroup[{index}] core[{i}]") for i in range(17))
    facing = reader.u8(f"HotKeyGroup[{index}].facing")
    buildings = reader.u8(f"HotKeyGroup[{index}].buildings")
    who = reader.u8(f"HotKeyGroup[{index}].who")
    march = reader.u8(f"HotKeyGroup[{index}].march")
    num = core_words[2]
    if num < 0 or num > MAX_GROUP_MEMBERS:
        raise HotKeyGroupsParseError(
            f"HotKeyGroup[{index}].num {num} is outside the PDB 128-member arrays"
        )
    list_offset = reader.pos if num else None
    members = tuple(reader.i16(f"HotKeyGroup[{index}].list[{i}]") for i in range(num))
    off_x = tuple(reader.i32(f"HotKeyGroup[{index}].off_x[{i}]") for i in range(num))
    off_y = tuple(reader.i32(f"HotKeyGroup[{index}].off_y[{i}]") for i in range(num))
    curr_x = tuple(reader.i32(f"HotKeyGroup[{index}].curr_x[{i}]") for i in range(num))
    curr_y = tuple(reader.i32(f"HotKeyGroup[{index}].curr_y[{i}]") for i in range(num))
    angles = tuple(reader.i8(f"HotKeyGroup[{index}].angles[{i}]") for i in range(num))
    tag_offset = reader.pos
    tag = _tag(reader, TAG_HOTKEY_GROUP, f"HotKeyGroup[{index}] tag", require_tags)
    loc_x_bits = reader.u32(f"HotKeyGroup[{index}].loc_x")
    loc_y_bits = reader.u32(f"HotKeyGroup[{index}].loc_y")
    valid = reader.i32(f"HotKeyGroup[{index}].valid")
    return HotKeyGroupRow(
        index, offset, reader.pos, core_offset, core_words,
        facing, buildings, who, march, list_offset, members,
        off_x, off_y, curr_x, curr_y, angles, tag_offset, tag,
        loc_x_bits, loc_y_bits, valid,
        hashlib.sha256(reader.data[offset:reader.pos]).hexdigest(),
    )


def parse_hotkey_groups_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tag: bool = True,
    require_row_tags: bool = True,
    schema_path: pathlib.Path | None = None,
) -> HotKeyGroupsSection:
    """Parse the tagged HotKeyGroup array and stop before World."""
    layout = _load_layout(schema_path)
    reader = _Reader(data, offset)
    tag = _tag(reader, TAG_HOTKEY_GROUPS, "HotKeyGroup array tag", require_tag)
    length = _count(reader, "HotKeyGroup array length", MAX_ARRAY_LENGTH)
    capacity: int | None = None
    increment: int | None = None
    flags: int | None = None
    rows: tuple[HotKeyGroupRow, ...] = ()
    if length:
        capacity = _count(reader, "HotKeyGroup array capacity", MAX_ARRAY_LENGTH)
        if capacity < length:
            raise HotKeyGroupsParseError(
                f"HotKeyGroup array capacity {capacity} is below length {length}"
            )
        increment = reader.i16("HotKeyGroup array increment")
        flags = reader.u8("HotKeyGroup array flags")
        if flags & 0x40:
            raise HotKeyGroupsParseError(
                f"HotKeyGroup array flags retain writer-cleared 0x40 bit: {flags:#04x}"
            )
        rows = tuple(_row(reader, index, require_row_tags) for index in range(length))
    return HotKeyGroupsSection(
        offset, reader.pos, tag, length, capacity, increment, flags, rows,
        hashlib.sha256(reader.data[offset:reader.pos]).hexdigest(), layout.sha256,
    )


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def _jsonable(value: object) -> object:
    if dataclasses.is_dataclass(value):
        result: dict[str, object] = {}
        for field in dataclasses.fields(value):
            item = getattr(value, field.name)
            if field.name.endswith("offset") or field.name == "end":
                result[field.name] = None if item is None else f"0x{item:x}"
            elif field.name.endswith("_bits"):
                result[field.name] = f"0x{item:08x}"
            else:
                result[field.name] = _jsonable(item)
        if hasattr(value, "size"):
            result["size"] = getattr(value, "size")
        return result
    if isinstance(value, tuple):
        return [_jsonable(item) for item in value]
    return value


def _summary(section: HotKeyGroupsSection, path: pathlib.Path) -> str:
    return "\n".join(
        (
            f"{path}: HotKeyGroups {section.offset:#x}..{section.end:#x} "
            f"({section.size} bytes) sha256={section.sha256}",
            f"  PDB-layout sha256={section.layout_sha256}",
            f"  length={section.length}, capacity={section.capacity}, "
            f"increment={section.increment}, flags={section.flags}",
            f"  next owner begins at {section.end:#x} (World::walk_data)",
        )
    )


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument("--offset", required=True, type=lambda text: int(text, 0))
    parser.add_argument("--schema", type=pathlib.Path, default=DEFAULT_SCHEMA_PATH)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    section = parse_hotkey_groups_section(
        _load(args.file), args.offset, schema_path=args.schema
    )
    print(json.dumps(_jsonable(section), indent=2) if args.json else _summary(section, args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
