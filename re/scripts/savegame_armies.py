#!/usr/bin/env python3
"""Parse the complete retail ``Armies::walk_data`` save-stream section.

This exclusive helper begins at the Armies walk-test byte, follows all eight
``PtrArray<Army>`` owners, and stops at the first byte owned by
``Cities::walk_data``.  The pointer-array history and conditional Army body
come from the shipped PE; compiler field names, offsets, and widths are
validated against the matching PDB export in ``schema/pdb-types.json``.

SaveGame/LoadGame walk tests emit bytes.  CheckSum walk tests do not, so this
save grammar intentionally includes both the Armies tag and each present
Army's tag rather than copying the otherwise useful checksum image.
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


TAG_ARMIES = 0x00
TAG_STRING_TABLE_INDEX = 133
ARMY_TAG_STRING_TABLE_INDEX = 134
OWNER_COUNT = 8
MAX_ARRAY_LENGTH = 1 << 20
ARMY_WALK_TAIL_HI = 0x98

DEFAULT_SCHEMA_PATH = (
    pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"
)


class ArmiesParseError(ValueError):
    """The input or PDB layout contradicts the recovered Armies traversal."""


@dataclasses.dataclass(frozen=True)
class ArmyField:
    name: str
    pdb_offset: int
    type_name: str
    offset: int
    end: int
    values: tuple[int, ...]


@dataclasses.dataclass(frozen=True)
class ArmySlot:
    index: int
    present: bool
    presence_offset: int
    offset: int | None
    end: int | None
    tag: int | None
    valid_offset: int | None
    valid: int | None
    fields: tuple[ArmyField, ...]
    sha256: str | None


@dataclasses.dataclass(frozen=True)
class ArmyOwner:
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
    slots: tuple[ArmySlot, ...]

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class ArmiesSection:
    offset: int
    end: int
    tag: int
    owners: tuple[ArmyOwner, ...]
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
    tail_fields: tuple[_FieldLayout, ...]
    sha256: str


_EXPECTED_PTR_ARRAY = (
    ("length", 4, 4, "int"),
    ("size", 8, 4, "int"),
    ("increment", 12, 2, "short"),
    ("list", 16, 4, "Army**"),
    ("flags", 20, 1, "unsigned char"),
    ("cur_index", 24, 4, "int"),
)

_EXPECTED_ARMY_FIELDS = (
    ("valid", 0, 2, "short"),
    ("army", 2, 2, "short"),
    ("status", 4, 4, "int"),
    ("reg", 8, 4, "int"),
    ("role", 12, 4, "int"),
    ("num_units", 16, 4, "int"),
    ("num_captains", 20, 4, "int"),
    ("num_standard", 24, 4, "int"),
    ("num_decoys", 28, 4, "int"),
    ("city", 32, 4, "int"),
    ("navy", 36, 4, "int"),
    ("human_frame", 40, 4, "int"),
    ("hurry", 44, 4, "int"),
    ("target_o", 48, 4, "int"),
    ("target_who", 52, 4, "int"),
    ("x", 56, 4, "Coord"),
    ("y", 60, 4, "Coord"),
    ("angle", 64, 4, "int"),
    ("rally_dist", 68, 4, "int"),
    ("muster_x", 72, 4, "WCoord"),
    ("muster_y", 76, 4, "WCoord"),
    ("muster_angle", 80, 4, "int"),
    ("list", 84, 64, "int[16]"),
    ("who", 148, 2, "short"),
    ("num_groups", 150, 2, "short"),
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
        armies = classes["Armies"]
        ptr_array = classes["PtrArray<Army>"]
        army = classes["Army"]
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise ArmiesParseError(f"cannot load Armies PDB layout from {path}: {error}") from error

    armies_fields = _field_tuples(armies)
    if armies.get("size") != 232 or armies_fields != (
        ("lists", 0, 224, "PtrArray<Army>[8]"),
    ):
        raise ArmiesParseError(
            f"PDB Armies layout disagrees: size={armies.get('size')}, "
            f"fields={armies_fields!r}"
        )

    ptr_fields = _field_tuples(ptr_array)
    if ptr_array.get("size") != 28 or ptr_fields != _EXPECTED_PTR_ARRAY:
        raise ArmiesParseError(
            f"PDB PtrArray<Army> layout disagrees: size={ptr_array.get('size')}, "
            f"fields={ptr_fields!r}"
        )

    army_fields = _field_tuples(army)
    if army.get("size") != 160 or army_fields != _EXPECTED_ARMY_FIELDS:
        raise ArmiesParseError(
            f"PDB Army layout disagrees: size={army.get('size')}, "
            f"fields={army_fields!r}"
        )
    cursor = 0
    for name, offset, size, _type_name in army_fields:
        if offset != cursor:
            raise ArmiesParseError(
                f"PDB ArmyData image has a gap before {name}: {cursor:#x}..{offset:#x}"
            )
        cursor += size
    if cursor != ARMY_WALK_TAIL_HI:
        raise ArmiesParseError(
            f"PDB ArmyData image ends at {cursor:#x}, expected {ARMY_WALK_TAIL_HI:#x}"
        )

    receipt = {
        "Armies": {"size": armies["size"], "flattened": armies_fields},
        "PtrArray<Army>": {
            "size": ptr_array["size"],
            "flattened": ptr_fields,
        },
        "Army": {"size": army["size"], "flattened": army_fields},
    }
    digest = hashlib.sha256(
        json.dumps(receipt, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()
    return _Layout(
        tail_fields=tuple(
            _FieldLayout(str(name), int(offset), int(size), str(type_name))
            for name, offset, size, type_name in army_fields[1:]
        ),
        sha256=digest,
    )


def _load_layout(schema_path: pathlib.Path | None) -> _Layout:
    path = (schema_path or DEFAULT_SCHEMA_PATH).resolve()
    return _load_layout_cached(str(path))


class _Reader:
    def __init__(self, data: bytes | bytearray | memoryview, offset: int):
        self.data = memoryview(data)
        if offset < 0 or offset > len(self.data):
            raise ArmiesParseError(
                f"offset {offset:#x} is outside {len(self.data):#x}-byte stream"
            )
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise ArmiesParseError(
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


def _decode_field(reader: _Reader, field: _FieldLayout) -> ArmyField:
    start = reader.pos
    raw = reader.take(field.size, f"Army.{field.name}")
    if field.type_name in ("short",):
        values = (struct.unpack("<h", raw)[0],)
    elif field.type_name in ("int", "Coord", "WCoord"):
        values = (struct.unpack("<i", raw)[0],)
    elif field.type_name == "int[16]":
        values = struct.unpack("<16i", raw)
    else:  # Protected by the exact PDB receipt above.
        raise ArmiesParseError(f"unsupported PDB Army field type {field.type_name}")
    return ArmyField(
        name=field.name,
        pdb_offset=field.offset,
        type_name=field.type_name,
        offset=start,
        end=reader.pos,
        values=values,
    )


def _empty_slot(index: int, presence_offset: int) -> ArmySlot:
    return ArmySlot(
        index=index,
        present=False,
        presence_offset=presence_offset,
        offset=None,
        end=None,
        tag=None,
        valid_offset=None,
        valid=None,
        fields=(),
        sha256=None,
    )


def _parse_owner(reader: _Reader, index: int, layout: _Layout) -> ArmyOwner:
    start = reader.pos
    length = reader.i32(f"Armies.lists[{index}].length")
    if length < 0 or length > MAX_ARRAY_LENGTH:
        raise ArmiesParseError(
            f"Armies.lists[{index}] has invalid length {length} at {start:#x}"
        )
    if length == 0:
        return ArmyOwner(
            index=index,
            offset=start,
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
        )

    capacity = reader.i32(f"Armies.lists[{index}].capacity")
    increment = reader.i16(f"Armies.lists[{index}].increment")
    flags = reader.u8(f"Armies.lists[{index}].flags")
    if capacity < length or capacity > MAX_ARRAY_LENGTH:
        raise ArmiesParseError(
            f"Armies.lists[{index}] has invalid history "
            f"length={length}, capacity={capacity}"
        )
    if flags & 0x40:
        raise ArmiesParseError(
            f"Armies.lists[{index}] flags {flags:#04x} retain writer-cleared bit 0x40"
        )

    presence_offset = reader.pos
    presence = tuple(
        reader.u8(f"Armies.lists[{index}].presence[{slot}]")
        for slot in range(length)
    )
    if any(value not in (0, 1) for value in presence):
        raise ArmiesParseError(
            f"Armies.lists[{index}] pointer-presence plane is not boolean"
        )

    repeated_capacity_offset = reader.pos
    repeated_capacity = reader.i32(f"Armies.lists[{index}].repeated_capacity")
    repeated_increment_offset = reader.pos
    repeated_increment = reader.i16(f"Armies.lists[{index}].repeated_increment")
    if (repeated_capacity, repeated_increment) != (capacity, increment):
        raise ArmiesParseError(
            f"Armies.lists[{index}] duplicated capacity/increment image disagrees: "
            f"({capacity}, {increment}) != "
            f"({repeated_capacity}, {repeated_increment})"
        )

    slots: list[ArmySlot] = []
    for slot, present in enumerate(presence):
        marker_offset = presence_offset + slot
        if not present:
            slots.append(_empty_slot(slot, marker_offset))
            continue
        body_offset = reader.pos
        # This value is emitted by SaveGame/LoadGame walk_test(StringTable[134]).
        # No nonempty retail specimen is available, so preserve it without
        # inventing a numeric constant.  Its structural position is exact.
        tag = reader.u8(f"Armies.lists[{index}][{slot}].Army tag")
        valid_offset = reader.pos
        valid = reader.i16(f"Armies.lists[{index}][{slot}].valid")
        fields = (
            tuple(_decode_field(reader, field) for field in layout.tail_fields)
            if valid != 0
            else ()
        )
        slots.append(
            ArmySlot(
                index=slot,
                present=True,
                presence_offset=marker_offset,
                offset=body_offset,
                end=reader.pos,
                tag=tag,
                valid_offset=valid_offset,
                valid=valid,
                fields=fields,
                sha256=hashlib.sha256(
                    reader.data[body_offset : reader.pos]
                ).hexdigest(),
            )
        )

    return ArmyOwner(
        index=index,
        offset=start,
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
        slots=tuple(slots),
    )


def parse_armies_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tag: bool = True,
    schema_path: pathlib.Path | None = None,
) -> ArmiesSection:
    """Parse exactly one retail Armies section beginning at ``offset``.

    The returned ``end`` is the first byte owned by ``Cities::walk_data``.
    Per-Army tag values are preserved rather than guessed; their locations and
    StringTable identity are proven even though the fresh specimen has no
    present Army pointers.
    """

    layout = _load_layout(schema_path)
    reader = _Reader(data, offset)
    tag = reader.u8("Armies tag")
    if require_tag and tag != TAG_ARMIES:
        raise ArmiesParseError(
            f"Armies tag {tag:#04x} != {TAG_ARMIES:#04x} at {offset:#x}"
        )
    owners = tuple(_parse_owner(reader, index, layout) for index in range(OWNER_COUNT))
    return ArmiesSection(
        offset=offset,
        end=reader.pos,
        tag=tag,
        owners=owners,
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
            if field.name in (
                "offset",
                "end",
                "presence_offset",
                "repeated_capacity_offset",
                "repeated_increment_offset",
                "valid_offset",
                "pdb_offset",
            ) and field_value is not None:
                result[field.name] = f"0x{field_value:x}"
            else:
                result[field.name] = _jsonable(field_value)
        if isinstance(value, (ArmyOwner, ArmiesSection)):
            result["size"] = value.size
        return result
    if isinstance(value, tuple):
        return [_jsonable(item) for item in value]
    return value


def _summary(section: ArmiesSection, path: pathlib.Path) -> str:
    lines = [
        f"{path}: Armies {section.offset:#x}..{section.end:#x} "
        f"({section.size} bytes) sha256={section.sha256}",
        f"  tag={section.tag:#04x} (StringTable[{TAG_STRING_TABLE_INDEX}]); "
        f"PDB-layout sha256={section.layout_sha256}",
    ]
    for owner in section.owners:
        if owner.length == 0:
            lines.append(f"  owner {owner.index}: empty ({owner.size} bytes)")
        else:
            live = sum(slot.present and slot.valid != 0 for slot in owner.slots)
            lines.append(
                f"  owner {owner.index}: length={owner.length} "
                f"capacity={owner.capacity} increment={owner.increment} "
                f"flags={owner.flags:#04x} present={sum(owner.presence)} "
                f"live={live} ({owner.size} bytes)"
            )
    lines.append(
        f"  next owner begins at {section.end:#x} "
        "(Cities::walk_data 0x00735410)"
    )
    return "\n".join(lines)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument(
        "--offset",
        required=True,
        type=lambda text: int(text, 0),
        help="decompressed stream offset of the Armies tag",
    )
    parser.add_argument("--schema", type=pathlib.Path, default=DEFAULT_SCHEMA_PATH)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    section = parse_armies_section(
        _load(args.file), args.offset, schema_path=args.schema
    )
    if args.json:
        print(json.dumps(_jsonable(section), indent=2))
    else:
        print(_summary(section, args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
