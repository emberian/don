#!/usr/bin/env python3
"""Parse the exact retail ``PtrArray<Item>`` generic-save image.

The helper starts at the Items logical length, preserves both allocation-
history images and every pointer-presence marker, decodes every present
``Item``/``SubObject`` body, and stops at the first byte owned by
``Heroes::walk_data``.  PE control flow defines stream order; the matching PDB
export defines field names, offsets, and widths.
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


ITEM_TAG_STRING_TABLE_INDEX = 4580
SUBOBJECT_TAG_STRING_TABLE_INDEX = 6262
MAX_ARRAY_LENGTH = 1 << 20

DEFAULT_SCHEMA_PATH = (
    pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"
)


class ItemsParseError(ValueError):
    """The stream or PDB layout contradicts the recovered Items traversal."""


@dataclasses.dataclass(frozen=True)
class ItemSlot:
    index: int
    present: bool
    presence_offset: int
    offset: int | None
    end: int | None
    item_tag: int | None
    ever_seen: int | None
    subobject_tag: int | None
    flags: int | None
    must_walk_offset: int | None
    must_walk: int | None
    who: int | None
    o: int | None
    z_internal: int | None
    x_internal: int | None
    y_internal: int | None
    type_index: int | None
    sha256: str | None

    @property
    def size(self) -> int:
        return 0 if self.offset is None or self.end is None else self.end - self.offset


@dataclasses.dataclass(frozen=True)
class ItemsSection:
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
    slots: tuple[ItemSlot, ...]
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class _Layout:
    sha256: str


_EXPECTED_PTR_ARRAY = (
    ("length", 4, 4, "int"),
    ("size", 8, 4, "int"),
    ("increment", 12, 2, "short"),
    ("list", 16, 4, "Item**"),
    ("flags", 20, 1, "unsigned char"),
    ("cur_index", 24, 4, "int"),
)

_EXPECTED_ITEM = (
    ("flags", 8, 1, "unsigned char"),
    ("who", 9, 1, "unsigned char"),
    ("o", 10, 2, "short"),
    ("z_internal", 12, 4, "Coord"),
    ("x_internal", 16, 4, "Coord"),
    ("y_internal", 20, 4, "Coord"),
    ("ptype", 24, 4, "ObjectType*"),
    ("on_screen", 28, 1, "unsigned char"),
    ("ever_seen", 32, 1, "unsigned char"),
)

_EXPECTED_SUBOBJECT_DATA = _EXPECTED_ITEM[:7]


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
        ptr_array = classes["PtrArray<Item>"]
        item = classes["Item"]
        item_data = classes["ItemData"]
        subobject = classes["SubObject"]
        subobject_data = classes["SubObjectData"]
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise ItemsParseError(
            f"cannot load Items PDB layout from {path}: {error}"
        ) from error

    ptr_fields = _field_tuples(ptr_array)
    item_fields = _field_tuples(item)
    item_data_fields = _field_tuples(item_data)
    subobject_fields = _field_tuples(subobject)
    subobject_data_fields = _field_tuples(subobject_data)
    if ptr_array.get("size") != 28 or ptr_fields != _EXPECTED_PTR_ARRAY:
        raise ItemsParseError(
            f"PDB PtrArray<Item> layout disagrees: size={ptr_array.get('size')}, "
            f"fields={ptr_fields!r}"
        )
    if item.get("size") != 44 or item_fields != _EXPECTED_ITEM:
        raise ItemsParseError(
            f"PDB Item layout disagrees: size={item.get('size')}, "
            f"fields={item_fields!r}"
        )
    if item_data.get("size") != 44 or item_data_fields != _EXPECTED_ITEM:
        raise ItemsParseError(
            f"PDB ItemData layout disagrees: size={item_data.get('size')}, "
            f"fields={item_data_fields!r}"
        )
    if subobject.get("size") != 40 or subobject_fields != _EXPECTED_ITEM[:8]:
        raise ItemsParseError(
            f"PDB SubObject layout disagrees: size={subobject.get('size')}, "
            f"fields={subobject_fields!r}"
        )
    if (
        subobject_data.get("size") != 28
        or subobject_data_fields != _EXPECTED_SUBOBJECT_DATA
    ):
        raise ItemsParseError(
            "PDB SubObjectData layout disagrees: "
            f"size={subobject_data.get('size')}, fields={subobject_data_fields!r}"
        )

    receipt = {
        "PtrArray<Item>": {
            "size": ptr_array["size"],
            "flattened": ptr_fields,
        },
        "Item": {"size": item["size"], "flattened": item_fields},
        "ItemData": {
            "size": item_data["size"],
            "flattened": item_data_fields,
        },
        "SubObject": {
            "size": subobject["size"],
            "flattened": subobject_fields,
        },
        "SubObjectData": {
            "size": subobject_data["size"],
            "flattened": subobject_data_fields,
        },
    }
    digest = hashlib.sha256(
        json.dumps(receipt, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()
    return _Layout(sha256=digest)


def _load_layout(schema_path: pathlib.Path | None) -> _Layout:
    return _load_layout_cached(str((schema_path or DEFAULT_SCHEMA_PATH).resolve()))


class _Reader:
    def __init__(self, data: bytes | bytearray | memoryview, offset: int):
        self.data = memoryview(data)
        if offset < 0 or offset > len(self.data):
            raise ItemsParseError(
                f"offset {offset:#x} is outside {len(self.data):#x}-byte stream"
            )
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise ItemsParseError(
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


def _empty_slot(index: int, presence_offset: int) -> ItemSlot:
    return ItemSlot(
        index=index,
        present=False,
        presence_offset=presence_offset,
        offset=None,
        end=None,
        item_tag=None,
        ever_seen=None,
        subobject_tag=None,
        flags=None,
        must_walk_offset=None,
        must_walk=None,
        who=None,
        o=None,
        z_internal=None,
        x_internal=None,
        y_internal=None,
        type_index=None,
        sha256=None,
    )


def _parse_slot(reader: _Reader, index: int, presence_offset: int) -> ItemSlot:
    start = reader.pos
    # SaveGame/LoadGame emit both walk_test bytes.  The fresh specimen has no
    # rows, so preserve their values rather than inventing numeric constants.
    item_tag = reader.u8(f"Items[{index}].Item tag")
    ever_seen = reader.u8(f"Items[{index}].ever_seen")
    subobject_tag = reader.u8(f"Items[{index}].SubObject tag")
    flags = reader.u8(f"Items[{index}].flags")
    must_walk_offset = reader.pos
    must_walk = reader.u8(f"Items[{index}].must_walk")
    if must_walk not in (0, 1):
        raise ItemsParseError(
            f"Items[{index}] SubObject::must_walk byte is not boolean: {must_walk}"
        )

    who = o = z_internal = x_internal = y_internal = type_index = None
    if must_walk:
        who = reader.u8(f"Items[{index}].who")
        o = reader.i16(f"Items[{index}].o")
        z_internal = reader.i32(f"Items[{index}].z_internal")
        x_internal = reader.i32(f"Items[{index}].x_internal")
        y_internal = reader.i32(f"Items[{index}].y_internal")
        type_index = reader.i32(f"Items[{index}].type_index")

    return ItemSlot(
        index=index,
        present=True,
        presence_offset=presence_offset,
        offset=start,
        end=reader.pos,
        item_tag=item_tag,
        ever_seen=ever_seen,
        subobject_tag=subobject_tag,
        flags=flags,
        must_walk_offset=must_walk_offset,
        must_walk=must_walk,
        who=who,
        o=o,
        z_internal=z_internal,
        x_internal=x_internal,
        y_internal=y_internal,
        type_index=type_index,
        sha256=hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
    )


def parse_items_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    schema_path: pathlib.Path | None = None,
) -> ItemsSection:
    """Parse one complete retail ``PtrArray<Item>`` at ``offset``.

    The returned ``end`` is the first byte owned by ``Heroes::walk_data``.
    Allocation history and row tags are preserved without normalization.
    """

    layout = _load_layout(schema_path)
    reader = _Reader(data, offset)
    length = reader.i32("Items.length")
    if length < 0 or length > MAX_ARRAY_LENGTH:
        raise ItemsParseError(f"Items has invalid length {length} at {offset:#x}")
    if length == 0:
        return ItemsSection(
            offset=offset,
            end=reader.pos,
            length=0,
            capacity=None,
            increment=None,
            flags=None,
            presence_offset=None,
            presence=(),
            repeated_capacity_offset=None,
            repeated_capacity=None,
            repeated_increment_offset=None,
            repeated_increment=None,
            slots=(),
            sha256=hashlib.sha256(reader.data[offset : reader.pos]).hexdigest(),
            layout_sha256=layout.sha256,
        )

    capacity = reader.i32("Items.capacity")
    increment = reader.i16("Items.increment")
    flags = reader.u8("Items.flags")
    if capacity < length or capacity > MAX_ARRAY_LENGTH:
        raise ItemsParseError(
            f"Items has invalid history length={length}, capacity={capacity}"
        )
    if flags & 0x40:
        raise ItemsParseError(
            f"Items flags {flags:#04x} retain writer-cleared bit 0x40"
        )

    presence_offset = reader.pos
    presence = tuple(
        reader.u8(f"Items.presence[{index}]") for index in range(length)
    )
    if any(value not in (0, 1) for value in presence):
        raise ItemsParseError("Items pointer-presence plane is not boolean")

    repeated_capacity_offset = reader.pos
    repeated_capacity = reader.i32("Items.repeated_capacity")
    repeated_increment_offset = reader.pos
    repeated_increment = reader.i16("Items.repeated_increment")
    if (repeated_capacity, repeated_increment) != (capacity, increment):
        raise ItemsParseError(
            "Items duplicated capacity/increment image disagrees: "
            f"({capacity}, {increment}) != "
            f"({repeated_capacity}, {repeated_increment})"
        )

    slots = tuple(
        _parse_slot(reader, index, presence_offset + index)
        if present
        else _empty_slot(index, presence_offset + index)
        for index, present in enumerate(presence)
    )
    return ItemsSection(
        offset=offset,
        end=reader.pos,
        length=length,
        capacity=capacity,
        increment=increment,
        flags=flags,
        presence_offset=presence_offset,
        presence=presence,
        repeated_capacity_offset=repeated_capacity_offset,
        repeated_capacity=repeated_capacity,
        repeated_increment_offset=repeated_increment_offset,
        repeated_increment=repeated_increment,
        slots=slots,
        sha256=hashlib.sha256(reader.data[offset : reader.pos]).hexdigest(),
        layout_sha256=layout.sha256,
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
                result[field.name] = (
                    None if field_value is None else f"0x{field_value:x}"
                )
            else:
                result[field.name] = _jsonable(field_value)
        if isinstance(value, (ItemSlot, ItemsSection)):
            result["size"] = value.size
        return result
    if isinstance(value, tuple):
        return [_jsonable(item) for item in value]
    return value


def _summary(section: ItemsSection, path: pathlib.Path) -> str:
    lines = [
        f"{path}: Items {section.offset:#x}..{section.end:#x} "
        f"({section.size} bytes) sha256={section.sha256}",
        f"  PDB-layout sha256={section.layout_sha256}",
    ]
    if section.length == 0:
        lines.append("  PtrArray<Item>: empty")
    else:
        walked = sum(slot.present and slot.must_walk == 1 for slot in section.slots)
        lines.append(
            f"  PtrArray<Item>: length={section.length} "
            f"capacity={section.capacity} increment={section.increment} "
            f"flags={section.flags:#04x} present={sum(section.presence)} "
            f"full_bodies={walked}"
        )
    lines.append(
        f"  next owner begins at {section.end:#x} "
        "(Heroes::walk_data 0x0073a510)"
    )
    return "\n".join(lines)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument(
        "--offset",
        required=True,
        type=lambda text: int(text, 0),
        help="decompressed stream offset of PtrArray<Item>.length",
    )
    parser.add_argument("--schema", type=pathlib.Path, default=DEFAULT_SCHEMA_PATH)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    section = parse_items_section(
        _load(args.file), args.offset, schema_path=args.schema
    )
    if args.json:
        print(json.dumps(_jsonable(section), indent=2))
    else:
        print(_summary(section, args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
