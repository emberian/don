#!/usr/bin/env python3
"""Parse the retail PathFinder direct block followed by ``Array<Group>``.

The helper consumes the exact 108-byte ``PathFinder::walk_data`` projection,
then the complete allocation history and every dynamic row of the caller's
``Array<Group>``. It stops before the following caller tag for the remaining
Groups state. PE control flow defines order; the matched PDB defines all
field names, widths, array bounds, and excluded runtime state.
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


PATHFINDER_FIELD_COUNT = 27
PATHFINDER_BYTES = 108
GROUP_MEMBER_CAPACITY = 128
MAX_ARRAY_LENGTH = 1 << 20
NEXT_TAG_STRING_TABLE_INDEX = 2920
DEFAULT_SCHEMA_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"


class PathfinderGroupsParseError(ValueError):
    """The stream or PDB layout contradicts the recovered combined tranche."""


@dataclasses.dataclass(frozen=True)
class ScalarField:
    name: str
    pdb_offset: int
    type_name: str
    offset: int
    end: int
    value: int

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class PathFinderImage:
    offset: int
    end: int
    fields: tuple[ScalarField, ...]
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class GroupRow:
    index: int
    offset: int
    end: int
    fixed_offset: int
    fixed_end: int
    fields: tuple[ScalarField, ...]
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
        return self.fields[2].value


@dataclasses.dataclass(frozen=True)
class GroupsArrayImage:
    offset: int
    end: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    rows: tuple[GroupRow, ...]
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class PathfinderGroupsSection:
    offset: int
    end: int
    pathfinder: PathFinderImage
    groups: GroupsArrayImage
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class _FieldLayout:
    name: str
    offset: int
    size: int
    type_name: str


@dataclasses.dataclass(frozen=True)
class _Layout:
    path_fields: tuple[_FieldLayout, ...]
    group_fields: tuple[_FieldLayout, ...]
    sha256: str


_EXPECTED_PATH_FIELDS = (
    ("sx", 88, 4, "TCoord"),
    ("sy", 92, 4, "TCoord"),
    ("dbg_collisions", 96, 4, "int"),
    ("anti_unit", 100, 4, "int"),
    ("offx", 104, 4, "int"),
    ("offy", 108, 4, "int"),
    ("army", 112, 4, "int"),
    ("iroquois", 116, 4, "int"),
    ("worker", 120, 4, "int"),
    ("no_danger", 124, 4, "int"),
    ("limit", 128, 4, "int"),
    ("saving", 132, 4, "int"),
    ("avoid_land", 136, 4, "int"),
    ("avoid_sea", 140, 4, "int"),
    ("valid_hit", 144, 4, "int"),
    ("scouting", 148, 4, "int"),
    ("can_transport", 152, 4, "int"),
    ("dbg_view_failures", 156, 4, "int"),
    ("road_base_val", 160, 4, "int"),
    ("road_avoid_sea", 164, 4, "int"),
    ("road_cross_coast", 168, 4, "int"),
    ("road_enemy", 172, 4, "int"),
    ("road_noone", 176, 4, "int"),
    ("road_bad_path", 180, 4, "int"),
    ("road_river", 184, 4, "int"),
    ("road_z_max", 188, 4, "int"),
    ("road_diag_penalty", 192, 4, "int"),
)
_EXPECTED_ARRAY = (
    ("length", 4, 4, "int"),
    ("size", 8, 4, "int"),
    ("increment", 12, 2, "short"),
    ("list", 16, 4, "Group*"),
    ("flags", 20, 1, "unsigned char"),
    ("cur_index", 24, 4, "int"),
)
_EXPECTED_GROUP = (
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


def _field_tuples(record: dict[str, object]) -> tuple[tuple[object, ...], ...]:
    return tuple(
        (field["name"], field["offset"], field["size"], field["type"])
        for field in record["flattened"]  # type: ignore[index]
    )


@functools.lru_cache(maxsize=None)
def _load_layout_cached(path_text: str) -> _Layout:
    path = pathlib.Path(path_text)
    try:
        classes = json.loads(path.read_text())["classes"]
        pathfinder = classes["PathFinder"]
        pathfinder_data = classes["PathFinderData"]
        array = classes["Array<Group>"]
        group = classes["Group"]
        group_data = classes["GroupData"]
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise PathfinderGroupsParseError(
            f"cannot load PathFinder/Groups PDB layout from {path}: {error}"
        ) from error
    path_fields = _field_tuples(pathfinder)
    path_data_fields = _field_tuples(pathfinder_data)
    array_fields = _field_tuples(array)
    group_fields = _field_tuples(group)
    group_data_fields = _field_tuples(group_data)
    actual_path_slice = tuple(field for field in path_fields if 88 <= field[1] < 196)
    expected_data_slice = tuple(
        (name, offset - 64, size, type_name)
        for name, offset, size, type_name in _EXPECTED_PATH_FIELDS
    )
    actual_data_slice = tuple(field for field in path_data_fields if 24 <= field[1] < 132)
    checks = (
        ("PathFinder", pathfinder.get("size"), 204, actual_path_slice, _EXPECTED_PATH_FIELDS),
        ("PathFinderData", pathfinder_data.get("size"), 136, actual_data_slice, expected_data_slice),
        ("Array<Group>", array.get("size"), 28, array_fields, _EXPECTED_ARRAY),
        ("Group", group.get("size"), 2516, group_fields, _EXPECTED_GROUP),
        ("GroupData", group_data.get("size"), 2508, group_data_fields, _EXPECTED_GROUP),
    )
    receipt: dict[str, object] = {}
    for name, size, expected_size, fields, expected_fields in checks:
        if size != expected_size or fields != expected_fields:
            raise PathfinderGroupsParseError(
                f"PDB {name} layout disagrees: size={size}, fields={fields!r}"
            )
        receipt[name] = {"size": size, "walked_fields": fields}
    return _Layout(
        tuple(_FieldLayout(*field) for field in _EXPECTED_PATH_FIELDS),
        tuple(_FieldLayout(*field) for field in _EXPECTED_GROUP[:21]),
        hashlib.sha256(
            json.dumps(receipt, sort_keys=True, separators=(",", ":")).encode()
        ).hexdigest(),
    )


def _load_layout(schema_path: pathlib.Path | None) -> _Layout:
    return _load_layout_cached(str((schema_path or DEFAULT_SCHEMA_PATH).resolve()))


class _Reader:
    def __init__(self, data: bytes | bytearray | memoryview, offset: int):
        self.data = memoryview(data)
        if offset < 0 or offset > len(self.data):
            raise PathfinderGroupsParseError(
                f"offset {offset:#x} is outside {len(self.data):#x}-byte stream"
            )
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise PathfinderGroupsParseError(
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


def _parse_pathfinder(reader: _Reader, layout: _Layout) -> PathFinderImage:
    start = reader.pos
    fields = []
    for field in layout.path_fields:
        field_start = reader.pos
        fields.append(
            ScalarField(
                field.name,
                field.offset,
                field.type_name,
                field_start,
                field_start + 4,
                reader.i32(f"PathFinder.{field.name}"),
            )
        )
    return PathFinderImage(
        start,
        reader.pos,
        tuple(fields),
        hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
    )


def _parse_group(reader: _Reader, layout: _Layout, index: int) -> GroupRow:
    start = fixed_offset = reader.pos
    fields = []
    for field in layout.group_fields:
        field_start = reader.pos
        if field.type_name in ("int", "Coord"):
            value = reader.i32(f"Groups[{index}].{field.name}")
        elif field.type_name == "unsigned char":
            value = reader.u8(f"Groups[{index}].{field.name}")
        else:  # Protected by exact layout receipt.
            raise PathfinderGroupsParseError(f"unsupported Group field {field.type_name}")
        fields.append(
            ScalarField(
                field.name,
                field.offset,
                field.type_name,
                field_start,
                reader.pos,
                value,
            )
        )
    fixed_end = reader.pos
    num = fields[2].value
    if num < 0 or num > GROUP_MEMBER_CAPACITY:
        raise PathfinderGroupsParseError(
            f"Groups[{index}].num {num} exceeds fixed member capacity 128"
        )
    member_ids = tuple(reader.i16(f"Groups[{index}].list[{slot}]") for slot in range(num))
    off_x = tuple(reader.i32(f"Groups[{index}].off_x[{slot}]") for slot in range(num))
    off_y = tuple(reader.i32(f"Groups[{index}].off_y[{slot}]") for slot in range(num))
    curr_x = tuple(reader.i32(f"Groups[{index}].curr_x[{slot}]") for slot in range(num))
    curr_y = tuple(reader.i32(f"Groups[{index}].curr_y[{slot}]") for slot in range(num))
    angles = tuple(reader.i8(f"Groups[{index}].angles[{slot}]") for slot in range(num))
    return GroupRow(
        index,
        start,
        reader.pos,
        fixed_offset,
        fixed_end,
        tuple(fields),
        member_ids,
        off_x,
        off_y,
        curr_x,
        curr_y,
        angles,
        hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
    )


def _parse_groups(reader: _Reader, layout: _Layout) -> GroupsArrayImage:
    start = reader.pos
    length = reader.i32("Groups.list.length")
    if length < 0 or length > MAX_ARRAY_LENGTH:
        raise PathfinderGroupsParseError(f"Groups list has invalid length {length}")
    if length == 0:
        return GroupsArrayImage(
            start,
            reader.pos,
            0,
            None,
            None,
            None,
            (),
            hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
        )
    capacity = reader.i32("Groups.list.capacity")
    increment = reader.i16("Groups.list.increment")
    flags = reader.u8("Groups.list.flags")
    if capacity < length or capacity > MAX_ARRAY_LENGTH:
        raise PathfinderGroupsParseError(
            f"Groups list has invalid history length={length}, capacity={capacity}"
        )
    if flags & 0x40:
        raise PathfinderGroupsParseError(
            f"Groups list flags {flags:#04x} retain writer-cleared bit 0x40"
        )
    rows = tuple(_parse_group(reader, layout, index) for index in range(length))
    return GroupsArrayImage(
        start,
        reader.pos,
        length,
        capacity,
        increment,
        flags,
        rows,
        hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
    )


def parse_pathfinder_groups_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    schema_path: pathlib.Path | None = None,
) -> PathfinderGroupsSection:
    """Parse the direct PathFinder block and following ``Array<Group>``.

    The returned ``end`` is the following caller tag at StringTable[2920].
    """
    layout = _load_layout(schema_path)
    reader = _Reader(data, offset)
    pathfinder = _parse_pathfinder(reader, layout)
    groups = _parse_groups(reader, layout)
    return PathfinderGroupsSection(
        offset,
        reader.pos,
        pathfinder,
        groups,
        hashlib.sha256(reader.data[offset : reader.pos]).hexdigest(),
        layout.sha256,
    )


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def _jsonable(value: object) -> object:
    if dataclasses.is_dataclass(value):
        result: dict[str, object] = {}
        for field in dataclasses.fields(value):
            field_value = getattr(value, field.name)
            if field.name.endswith("offset") or field.name in ("end", "fixed_end"):
                result[field.name] = f"0x{field_value:x}"
            else:
                result[field.name] = _jsonable(field_value)
        if hasattr(value, "size"):
            result["size"] = getattr(value, "size")
        return result
    if isinstance(value, tuple):
        return [_jsonable(item) for item in value]
    return value


def _summary(section: PathfinderGroupsSection, path: pathlib.Path) -> str:
    members = sum(row.num for row in section.groups.rows)
    return "\n".join(
        (
            f"{path}: PathFinder+Groups {section.offset:#x}..{section.end:#x} "
            f"({section.size} bytes) sha256={section.sha256}",
            f"  PDB-layout sha256={section.layout_sha256}",
            f"  pathfinder={section.pathfinder.size} bytes groups={section.groups.length} members={members}",
            f"  next owner begins at {section.end:#x} (caller tag StringTable[2920])",
        )
    )


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument("--offset", required=True, type=lambda text: int(text, 0))
    parser.add_argument("--schema", type=pathlib.Path, default=DEFAULT_SCHEMA_PATH)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    section = parse_pathfinder_groups_section(
        _load(args.file), args.offset, schema_path=args.schema
    )
    print(json.dumps(_jsonable(section), indent=2) if args.json else _summary(section, args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
