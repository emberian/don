#!/usr/bin/env python3
"""Parse the exact retail ``Caravans::walk_data`` generic-save image.

The helper starts at the Caravans tag, preserves all eight independent
``PtrArray<Caravan>`` histories and presence planes, decodes every exact
complete Caravan body, and stops before the caller Lands tag. PE control flow
defines stream order; the matched PDB export defines names, offsets, widths,
and the excluded padding/object tail.
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


TAG_CARAVANS = 0x00
TAG_STRING_TABLE_INDEX = 417
CARAVAN_TAG_STRING_TABLE_INDEX = 416
CARAVAN_OWNER_COUNT = 8
MAX_ARRAY_LENGTH = 1 << 20
DEFAULT_SCHEMA_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"


class CaravansParseError(ValueError):
    """The stream or PDB layout contradicts the recovered Caravans traversal."""


@dataclasses.dataclass(frozen=True)
class PathDataImage:
    index: int
    offset: int
    end: int
    to_x: int
    to_y: int
    tolerance: int
    flags: int
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class PathStackImage:
    offset: int
    end: int
    capacity: int
    length: int
    increment: int
    rows: tuple[PathDataImage, ...]
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class CaravanSlot:
    index: int
    present: bool
    presence_offset: int
    offset: int | None
    end: int | None
    row_tag: int | None
    city2: int | None
    whom: int | None
    city3: int | None
    whose: int | None
    cara: int | None
    o: int | None
    caravan_flags: int | None
    who: int | None
    making_road: int | None
    reset_road: int | None
    road: PathStackImage | None
    sha256: str | None

    @property
    def size(self) -> int:
        return 0 if self.offset is None or self.end is None else self.end - self.offset


@dataclasses.dataclass(frozen=True)
class CaravanOwner:
    index: int
    offset: int
    end: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    presence_offset: int | None
    presence: tuple[int, ...]
    repeated_capacity_offset: int | None
    repeated_capacity: int | None
    repeated_increment_offset: int | None
    repeated_increment: int | None
    slots: tuple[CaravanSlot, ...]
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class CaravansSection:
    offset: int
    end: int
    tag: int
    owners: tuple[CaravanOwner, ...]
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class _Layout:
    sha256: str


_EXPECTED_CARAVANS = (("lists", 0, 224, "PtrArray<Caravan>[8]"),)
_EXPECTED_PTR_ARRAY = (
    ("length", 4, 4, "int"),
    ("size", 8, 4, "int"),
    ("increment", 12, 2, "short"),
    ("list", 16, 4, "Caravan**"),
    ("flags", 20, 1, "unsigned char"),
    ("cur_index", 24, 4, "int"),
)
_EXPECTED_CARAVAN_DATA = (
    ("city2", 0, 2, "short"),
    ("whom", 2, 2, "short"),
    ("city3", 4, 2, "short"),
    ("whose", 6, 2, "short"),
    ("cara", 8, 2, "short"),
    ("o", 10, 2, "short"),
    ("caravan_flags", 12, 1, "unsigned char"),
    ("who", 13, 1, "char"),
    ("road", 16, 16, "Stack<PathData>"),
    ("making_road", 32, 4, "int"),
    ("reset_road", 36, 4, "int"),
    ("openlist", 40, 4, "Tree<PathNode *,int>*"),
    ("openlistrefs", 44, 4, "BRTree<TreeNode<PathNode *,int> *,unsigned long>*"),
    ("closedlist", 48, 4, "BRTree<PathNode *,unsigned long>*"),
    ("offset", 52, 4, "int"),
    ("endx", 56, 4, "Coord"),
    ("endy", 60, 4, "Coord"),
    ("traversed", 64, 4, "int"),
)
_EXPECTED_CARAVAN = _EXPECTED_CARAVAN_DATA + (
    ("last_draw_frame", 72, 4, "int"),
)
_EXPECTED_STACK = (
    ("list", 0, 4, "PathData*"),
    ("size", 4, 4, "int"),
    ("length", 8, 4, "int"),
    ("increment", 12, 1, "char"),
)
_EXPECTED_PATH_DATA = (
    ("to_x", 0, 4, "Coord"),
    ("to_y", 4, 4, "Coord"),
    ("tolerance", 8, 4, "int"),
    ("flags", 12, 4, "int"),
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
        records = {
            "Caravans": classes["Caravans"],
            "PtrArray<Caravan>": classes["PtrArray<Caravan>"],
            "Caravan": classes["Caravan"],
            "CaravanData": classes["CaravanData"],
            "Stack<PathData>": classes["Stack<PathData>"],
            "PathData": classes["PathData"],
        }
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise CaravansParseError(
            f"cannot load Caravans PDB layout from {path}: {error}"
        ) from error
    expected = {
        "Caravans": (224, _EXPECTED_CARAVANS),
        "PtrArray<Caravan>": (28, _EXPECTED_PTR_ARRAY),
        "Caravan": (80, _EXPECTED_CARAVAN),
        "CaravanData": (68, _EXPECTED_CARAVAN_DATA),
        "Stack<PathData>": (16, _EXPECTED_STACK),
        "PathData": (16, _EXPECTED_PATH_DATA),
    }
    receipt: dict[str, object] = {}
    for name, (size, fields) in expected.items():
        record = records[name]
        actual = _field_tuples(record)
        if record.get("size") != size or actual != fields:
            raise CaravansParseError(
                f"PDB {name} layout disagrees: size={record.get('size')}, fields={actual!r}"
            )
        receipt[name] = {"size": record["size"], "flattened": actual}
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
            raise CaravansParseError(
                f"offset {offset:#x} is outside {len(self.data):#x}-byte stream"
            )
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise CaravansParseError(
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


def _empty_slot(index: int, presence_offset: int) -> CaravanSlot:
    return CaravanSlot(
        index=index,
        present=False,
        presence_offset=presence_offset,
        offset=None,
        end=None,
        row_tag=None,
        city2=None,
        whom=None,
        city3=None,
        whose=None,
        cara=None,
        o=None,
        caravan_flags=None,
        who=None,
        making_road=None,
        reset_road=None,
        road=None,
        sha256=None,
    )


def _parse_road(reader: _Reader, owner_index: int, index: int) -> PathStackImage:
    start = reader.pos
    name = f"Caravans[{owner_index}][{index}].road"
    capacity = reader.i32(f"{name}.capacity")
    length = reader.i32(f"{name}.length")
    increment = reader.i8(f"{name}.increment")
    if capacity < 0 or capacity > MAX_ARRAY_LENGTH:
        raise CaravansParseError(f"{name} has invalid capacity {capacity}")
    if length < 0 or length > capacity or length > MAX_ARRAY_LENGTH:
        raise CaravansParseError(
            f"{name} has invalid history length={length}, capacity={capacity}"
        )
    rows: list[PathDataImage] = []
    for row_index in range(length):
        row_start = reader.pos
        to_x = reader.i32(f"{name}[{row_index}].to_x")
        to_y = reader.i32(f"{name}[{row_index}].to_y")
        tolerance = reader.i32(f"{name}[{row_index}].tolerance")
        flags = reader.i32(f"{name}[{row_index}].flags")
        rows.append(
            PathDataImage(
                row_index,
                row_start,
                reader.pos,
                to_x,
                to_y,
                tolerance,
                flags,
                hashlib.sha256(reader.data[row_start : reader.pos]).hexdigest(),
            )
        )
    return PathStackImage(
        start,
        reader.pos,
        capacity,
        length,
        increment,
        tuple(rows),
        hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
    )


def _parse_slot(reader: _Reader, owner_index: int, index: int, presence_offset: int) -> CaravanSlot:
    start = reader.pos
    name = f"Caravans[{owner_index}][{index}]"
    # The fresh specimen has no live rows; preserve the retail walk_test byte
    # instead of inventing a numeric tag constant from another stream.
    row_tag = reader.u8(f"{name}.Caravan tag")
    city2 = reader.i16(f"{name}.city2")
    whom = reader.i16(f"{name}.whom")
    city3 = reader.i16(f"{name}.city3")
    whose = reader.i16(f"{name}.whose")
    cara = reader.i16(f"{name}.cara")
    o = reader.i16(f"Caravans[{owner_index}][{index}].o")
    caravan_flags = reader.u8(f"Caravans[{owner_index}][{index}].caravan_flags")
    who = reader.i8(f"Caravans[{owner_index}][{index}].who")
    making_road = reader.i32(f"{name}.making_road")
    reset_road = reader.i32(f"{name}.reset_road")
    road = _parse_road(reader, owner_index, index)
    return CaravanSlot(
        index=index,
        present=True,
        presence_offset=presence_offset,
        offset=start,
        end=reader.pos,
        row_tag=row_tag,
        city2=city2,
        whom=whom,
        city3=city3,
        whose=whose,
        cara=cara,
        o=o,
        caravan_flags=caravan_flags,
        who=who,
        making_road=making_road,
        reset_road=reset_road,
        road=road,
        sha256=hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
    )


def _parse_owner(reader: _Reader, index: int) -> CaravanOwner:
    start = reader.pos
    name = f"Caravans.owners[{index}]"
    length = reader.i32(f"{name}.length")
    if length < 0 or length > MAX_ARRAY_LENGTH:
        raise CaravansParseError(f"{name} has invalid length {length}")
    if length == 0:
        return CaravanOwner(
            index, start, reader.pos, 0, None, None, None, None, (),
            None, None, None, None, (),
            hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
        )
    capacity = reader.i32(f"{name}.capacity")
    increment = reader.i16(f"{name}.increment")
    flags = reader.u8(f"{name}.flags")
    if capacity < length or capacity > MAX_ARRAY_LENGTH:
        raise CaravansParseError(
            f"{name} has invalid history length={length}, capacity={capacity}"
        )
    if flags & 0x40:
        raise CaravansParseError(
            f"{name} flags {flags:#04x} retain writer-cleared bit 0x40"
        )
    presence_offset = reader.pos
    presence = tuple(reader.u8(f"{name}.presence[{slot}]") for slot in range(length))
    if any(value not in (0, 1) for value in presence):
        raise CaravansParseError(f"{name} pointer-presence plane is not boolean")
    repeated_capacity_offset = reader.pos
    repeated_capacity = reader.i32(f"{name}.repeated_capacity")
    repeated_increment_offset = reader.pos
    repeated_increment = reader.i16(f"{name}.repeated_increment")
    if (repeated_capacity, repeated_increment) != (capacity, increment):
        raise CaravansParseError(
            f"{name} duplicated capacity/increment image disagrees: "
            f"({capacity}, {increment}) != ({repeated_capacity}, {repeated_increment})"
        )
    slots = tuple(
        _parse_slot(reader, index, slot, presence_offset + slot)
        if present
        else _empty_slot(slot, presence_offset + slot)
        for slot, present in enumerate(presence)
    )
    return CaravanOwner(
        index, start, reader.pos, length, capacity, increment, flags,
        presence_offset, presence, repeated_capacity_offset, repeated_capacity,
        repeated_increment_offset, repeated_increment, slots,
        hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
    )


def parse_caravans_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tag: bool = True,
    schema_path: pathlib.Path | None = None,
) -> CaravansSection:
    """Parse one complete retail ``Caravans::walk_data`` at ``offset``.

    The returned ``end`` is the first byte owned by the caller's Lands tag.
    """
    layout = _load_layout(schema_path)
    reader = _Reader(data, offset)
    tag = reader.u8("Caravans tag")
    if require_tag and tag != TAG_CARAVANS:
        raise CaravansParseError(
            f"Caravans tag {tag:#04x} != {TAG_CARAVANS:#04x} at {offset:#x}"
        )
    owners = tuple(_parse_owner(reader, index) for index in range(CARAVAN_OWNER_COUNT))
    return CaravansSection(
        offset,
        reader.pos,
        tag,
        owners,
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
            if field.name.endswith("offset") or field.name == "end":
                result[field.name] = None if field_value is None else f"0x{field_value:x}"
            else:
                result[field.name] = _jsonable(field_value)
        if hasattr(value, "size"):
            result["size"] = getattr(value, "size")
        return result
    if isinstance(value, tuple):
        return [_jsonable(item) for item in value]
    return value


def _summary(section: CaravansSection, path: pathlib.Path) -> str:
    present = sum(sum(owner.presence) for owner in section.owners)
    return "\n".join(
        (
            f"{path}: Caravans {section.offset:#x}..{section.end:#x} "
            f"({section.size} bytes) sha256={section.sha256}",
            f"  PDB-layout sha256={section.layout_sha256}",
            f"  owners=8 present_caravans={present}",
            f"  next owner begins at {section.end:#x} (caller Lands tag; StringTable[4606])",
        )
    )


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument("--offset", required=True, type=lambda text: int(text, 0))
    parser.add_argument("--schema", type=pathlib.Path, default=DEFAULT_SCHEMA_PATH)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    section = parse_caravans_section(_load(args.file), args.offset, schema_path=args.schema)
    print(json.dumps(_jsonable(section), indent=2) if args.json else _summary(section, args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
