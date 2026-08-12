#!/usr/bin/env python3
"""Parse the caller Herds tag and exact ``PtrArray<Herd>`` save image.

The helper begins at ``walk_test(StringTable[3958])``, preserves both copies
of the pointer-array allocation history and every logical-slot presence byte,
decodes each exact 27-byte ``HerdData`` body, and stops before
``Specials::walk_data``.  PE control flow defines stream order; the matched
PDB export defines field names, offsets, and widths.
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


TAG_HERDS = 0x00
TAG_STRING_TABLE_INDEX = 3958
MAX_ARRAY_LENGTH = 1 << 20

DEFAULT_SCHEMA_PATH = (
    pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"
)


class HerdsParseError(ValueError):
    """The stream or PDB layout contradicts the recovered Herds traversal."""


@dataclasses.dataclass(frozen=True)
class HerdSlot:
    index: int
    present: bool
    presence_offset: int
    offset: int | None
    end: int | None
    cx: int | None
    cy: int | None
    wx: int | None
    wy: int | None
    type_index: int | None
    good_object: int | None
    herd: int | None
    herd_flags: int | None
    sha256: str | None

    @property
    def size(self) -> int:
        return 0 if self.offset is None or self.end is None else self.end - self.offset


@dataclasses.dataclass(frozen=True)
class HerdsSection:
    offset: int
    end: int
    tag: int
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
    slots: tuple[HerdSlot, ...]
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
    ("list", 16, 4, "Herd**"),
    ("flags", 20, 1, "unsigned char"),
    ("cur_index", 24, 4, "int"),
)
_EXPECTED_HERD = (
    ("cx", 0, 4, "WCoord"),
    ("cy", 4, 4, "WCoord"),
    ("wx", 8, 4, "WCoord"),
    ("wy", 12, 4, "WCoord"),
    ("t", 16, 4, "enum TypeIndex"),
    ("good_o", 20, 4, "int"),
    ("herd", 24, 2, "short"),
    ("herd_flags", 26, 1, "char"),
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
        ptr_array = classes["PtrArray<Herd>"]
        herd = classes["Herd"]
        herd_data = classes["HerdData"]
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise HerdsParseError(
            f"cannot load Herds PDB layout from {path}: {error}"
        ) from error

    ptr_fields = _field_tuples(ptr_array)
    herd_fields = _field_tuples(herd)
    herd_data_fields = _field_tuples(herd_data)
    if ptr_array.get("size") != 28 or ptr_fields != _EXPECTED_PTR_ARRAY:
        raise HerdsParseError(
            f"PDB PtrArray<Herd> layout disagrees: size={ptr_array.get('size')}, "
            f"fields={ptr_fields!r}"
        )
    if herd.get("size") != 36 or herd_fields != _EXPECTED_HERD:
        raise HerdsParseError(
            f"PDB Herd layout disagrees: size={herd.get('size')}, "
            f"fields={herd_fields!r}"
        )
    if herd_data.get("size") != 28 or herd_data_fields != _EXPECTED_HERD:
        raise HerdsParseError(
            f"PDB HerdData layout disagrees: size={herd_data.get('size')}, "
            f"fields={herd_data_fields!r}"
        )

    receipt = {
        "PtrArray<Herd>": {
            "size": ptr_array["size"],
            "flattened": ptr_fields,
        },
        "Herd": {"size": herd["size"], "flattened": herd_fields},
        "HerdData": {
            "size": herd_data["size"],
            "flattened": herd_data_fields,
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
            raise HerdsParseError(
                f"offset {offset:#x} is outside {len(self.data):#x}-byte stream"
            )
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise HerdsParseError(
                f"{what} range [{self.pos:#x},{end:#x}) exceeds "
                f"{len(self.data):#x}-byte stream"
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


def _empty_slot(index: int, presence_offset: int) -> HerdSlot:
    return HerdSlot(
        index=index,
        present=False,
        presence_offset=presence_offset,
        offset=None,
        end=None,
        cx=None,
        cy=None,
        wx=None,
        wy=None,
        type_index=None,
        good_object=None,
        herd=None,
        herd_flags=None,
        sha256=None,
    )


def _parse_slot(reader: _Reader, index: int, presence_offset: int) -> HerdSlot:
    start = reader.pos
    cx = reader.i32(f"Herds[{index}].cx")
    cy = reader.i32(f"Herds[{index}].cy")
    wx = reader.i32(f"Herds[{index}].wx")
    wy = reader.i32(f"Herds[{index}].wy")
    type_index = reader.i32(f"Herds[{index}].type_index")
    good_object = reader.i32(f"Herds[{index}].good_object")
    herd = reader.i16(f"Herds[{index}].herd")
    herd_flags = reader.i8(f"Herds[{index}].herd_flags")
    return HerdSlot(
        index=index,
        present=True,
        presence_offset=presence_offset,
        offset=start,
        end=reader.pos,
        cx=cx,
        cy=cy,
        wx=wx,
        wy=wy,
        type_index=type_index,
        good_object=good_object,
        herd=herd,
        herd_flags=herd_flags,
        sha256=hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
    )


def parse_herds_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tag: bool = True,
    schema_path: pathlib.Path | None = None,
) -> HerdsSection:
    """Parse the caller Herds tag plus complete ``PtrArray<Herd>``.

    The returned ``end`` is the first byte owned by ``Specials::walk_data``.
    Pointer-array allocation history is retained without normalization.
    """

    layout = _load_layout(schema_path)
    reader = _Reader(data, offset)
    tag = reader.u8("Herds tag")
    if require_tag and tag != TAG_HERDS:
        raise HerdsParseError(
            f"Herds tag {tag:#04x} != {TAG_HERDS:#04x} at {offset:#x}"
        )
    length = reader.i32("Herds.length")
    if length < 0 or length > MAX_ARRAY_LENGTH:
        raise HerdsParseError(f"Herds has invalid length {length} at {offset:#x}")
    if length == 0:
        return HerdsSection(
            offset=offset,
            end=reader.pos,
            tag=tag,
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

    capacity = reader.i32("Herds.capacity")
    increment = reader.i16("Herds.increment")
    flags = reader.u8("Herds.flags")
    if capacity < length or capacity > MAX_ARRAY_LENGTH:
        raise HerdsParseError(
            f"Herds has invalid history length={length}, capacity={capacity}"
        )
    if flags & 0x40:
        raise HerdsParseError(
            f"Herds flags {flags:#04x} retain writer-cleared bit 0x40"
        )

    presence_offset = reader.pos
    presence = tuple(
        reader.u8(f"Herds.presence[{index}]") for index in range(length)
    )
    if any(value not in (0, 1) for value in presence):
        raise HerdsParseError("Herds pointer-presence plane is not boolean")

    repeated_capacity_offset = reader.pos
    repeated_capacity = reader.i32("Herds.repeated_capacity")
    repeated_increment_offset = reader.pos
    repeated_increment = reader.i16("Herds.repeated_increment")
    if (repeated_capacity, repeated_increment) != (capacity, increment):
        raise HerdsParseError(
            "Herds duplicated capacity/increment image disagrees: "
            f"({capacity}, {increment}) != "
            f"({repeated_capacity}, {repeated_increment})"
        )

    slots = tuple(
        _parse_slot(reader, index, presence_offset + index)
        if present
        else _empty_slot(index, presence_offset + index)
        for index, present in enumerate(presence)
    )
    return HerdsSection(
        offset=offset,
        end=reader.pos,
        tag=tag,
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
        if hasattr(value, "size"):
            result["size"] = getattr(value, "size")
        return result
    if isinstance(value, tuple):
        return [_jsonable(item) for item in value]
    return value


def _summary(section: HerdsSection, path: pathlib.Path) -> str:
    lines = [
        f"{path}: Herds {section.offset:#x}..{section.end:#x} "
        f"({section.size} bytes) sha256={section.sha256}",
        f"  PDB-layout sha256={section.layout_sha256}",
    ]
    if section.length == 0:
        lines.append("  PtrArray<Herd>: empty")
    else:
        lines.append(
            f"  PtrArray<Herd>: length={section.length} "
            f"capacity={section.capacity} increment={section.increment} "
            f"flags={section.flags:#04x} present={sum(section.presence)}"
        )
    lines.append(
        f"  next owner begins at {section.end:#x} "
        "(Specials::walk_data 0x007403b0)"
    )
    return "\n".join(lines)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument(
        "--offset",
        required=True,
        type=lambda text: int(text, 0),
        help="decompressed stream offset of the caller Herds tag",
    )
    parser.add_argument("--schema", type=pathlib.Path, default=DEFAULT_SCHEMA_PATH)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    section = parse_herds_section(
        _load(args.file), args.offset, schema_path=args.schema
    )
    if args.json:
        print(json.dumps(_jsonable(section), indent=2))
    else:
        print(_summary(section, args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
