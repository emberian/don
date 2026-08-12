#!/usr/bin/env python3
"""Parse the exact retail ``Wonders::walk_data`` generic-save image.

The helper starts at the Wonders tag, preserves all eight independent
``PtrArray<Wonder>`` histories and presence planes, decodes every exact
14-byte Wonder body, and stops before ``Forts::walk_data``. PE control flow
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


TAG_WONDERS = 0x00
TAG_STRING_TABLE_INDEX = 7131
WONDER_OWNER_COUNT = 8
MAX_ARRAY_LENGTH = 1 << 20
DEFAULT_SCHEMA_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"


class WondersParseError(ValueError):
    """The stream or PDB layout contradicts the recovered Wonders traversal."""


@dataclasses.dataclass(frozen=True)
class WonderSlot:
    index: int
    present: bool
    presence_offset: int
    offset: int | None
    end: int | None
    wonder: int | None
    o: int | None
    stamp: int | None
    timer: int | None
    wonder_flags: int | None
    who: int | None
    sha256: str | None

    @property
    def size(self) -> int:
        return 0 if self.offset is None or self.end is None else self.end - self.offset


@dataclasses.dataclass(frozen=True)
class WonderOwner:
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
    slots: tuple[WonderSlot, ...]
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class WondersSection:
    offset: int
    end: int
    tag: int
    owners: tuple[WonderOwner, ...]
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class _Layout:
    sha256: str


_EXPECTED_WONDERS = (("lists", 0, 224, "PtrArray<Wonder>[8]"),)
_EXPECTED_PTR_ARRAY = (
    ("length", 4, 4, "int"),
    ("size", 8, 4, "int"),
    ("increment", 12, 2, "short"),
    ("list", 16, 4, "Wonder**"),
    ("flags", 20, 1, "unsigned char"),
    ("cur_index", 24, 4, "int"),
)
_EXPECTED_WONDER = (
    ("wonder", 0, 2, "short"),
    ("o", 2, 2, "short"),
    ("stamp", 4, 4, "int"),
    ("timer", 8, 4, "int"),
    ("wonder_flags", 12, 1, "char"),
    ("who", 13, 1, "char"),
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
            "Wonders": classes["Wonders"],
            "PtrArray<Wonder>": classes["PtrArray<Wonder>"],
            "Wonder": classes["Wonder"],
            "WonderData": classes["WonderData"],
        }
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise WondersParseError(
            f"cannot load Wonders PDB layout from {path}: {error}"
        ) from error
    expected = {
        "Wonders": (232, _EXPECTED_WONDERS),
        "PtrArray<Wonder>": (28, _EXPECTED_PTR_ARRAY),
        "Wonder": (24, _EXPECTED_WONDER),
        "WonderData": (16, _EXPECTED_WONDER),
    }
    receipt: dict[str, object] = {}
    for name, (size, fields) in expected.items():
        record = records[name]
        actual = _field_tuples(record)
        if record.get("size") != size or actual != fields:
            raise WondersParseError(
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
            raise WondersParseError(
                f"offset {offset:#x} is outside {len(self.data):#x}-byte stream"
            )
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise WondersParseError(
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


def _empty_slot(index: int, presence_offset: int) -> WonderSlot:
    return WonderSlot(index, False, presence_offset, None, None, None, None, None, None, None, None, None)


def _parse_slot(reader: _Reader, owner_index: int, index: int, presence_offset: int) -> WonderSlot:
    start = reader.pos
    wonder = reader.i16(f"Wonders[{owner_index}][{index}].wonder")
    o = reader.i16(f"Wonders[{owner_index}][{index}].o")
    stamp = reader.i32(f"Wonders[{owner_index}][{index}].stamp")
    timer = reader.i32(f"Wonders[{owner_index}][{index}].timer")
    wonder_flags = reader.i8(f"Wonders[{owner_index}][{index}].wonder_flags")
    who = reader.i8(f"Wonders[{owner_index}][{index}].who")
    return WonderSlot(
        index=index,
        present=True,
        presence_offset=presence_offset,
        offset=start,
        end=reader.pos,
        wonder=wonder,
        o=o,
        stamp=stamp,
        timer=timer,
        wonder_flags=wonder_flags,
        who=who,
        sha256=hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
    )


def _parse_owner(reader: _Reader, index: int) -> WonderOwner:
    start = reader.pos
    name = f"Wonders.owners[{index}]"
    length = reader.i32(f"{name}.length")
    if length < 0 or length > MAX_ARRAY_LENGTH:
        raise WondersParseError(f"{name} has invalid length {length}")
    if length == 0:
        return WonderOwner(
            index, start, reader.pos, 0, None, None, None, None, (),
            None, None, None, None, (),
            hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
        )
    capacity = reader.i32(f"{name}.capacity")
    increment = reader.i16(f"{name}.increment")
    flags = reader.u8(f"{name}.flags")
    if capacity < length or capacity > MAX_ARRAY_LENGTH:
        raise WondersParseError(
            f"{name} has invalid history length={length}, capacity={capacity}"
        )
    if flags & 0x40:
        raise WondersParseError(
            f"{name} flags {flags:#04x} retain writer-cleared bit 0x40"
        )
    presence_offset = reader.pos
    presence = tuple(reader.u8(f"{name}.presence[{slot}]") for slot in range(length))
    if any(value not in (0, 1) for value in presence):
        raise WondersParseError(f"{name} pointer-presence plane is not boolean")
    repeated_capacity_offset = reader.pos
    repeated_capacity = reader.i32(f"{name}.repeated_capacity")
    repeated_increment_offset = reader.pos
    repeated_increment = reader.i16(f"{name}.repeated_increment")
    if (repeated_capacity, repeated_increment) != (capacity, increment):
        raise WondersParseError(
            f"{name} duplicated capacity/increment image disagrees: "
            f"({capacity}, {increment}) != ({repeated_capacity}, {repeated_increment})"
        )
    slots = tuple(
        _parse_slot(reader, index, slot, presence_offset + slot)
        if present
        else _empty_slot(slot, presence_offset + slot)
        for slot, present in enumerate(presence)
    )
    return WonderOwner(
        index, start, reader.pos, length, capacity, increment, flags,
        presence_offset, presence, repeated_capacity_offset, repeated_capacity,
        repeated_increment_offset, repeated_increment, slots,
        hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
    )


def parse_wonders_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tag: bool = True,
    schema_path: pathlib.Path | None = None,
) -> WondersSection:
    """Parse one complete retail ``Wonders::walk_data`` at ``offset``.

    The returned ``end`` is the first byte owned by ``Forts::walk_data``.
    """
    layout = _load_layout(schema_path)
    reader = _Reader(data, offset)
    tag = reader.u8("Wonders tag")
    if require_tag and tag != TAG_WONDERS:
        raise WondersParseError(
            f"Wonders tag {tag:#04x} != {TAG_WONDERS:#04x} at {offset:#x}"
        )
    owners = tuple(_parse_owner(reader, index) for index in range(WONDER_OWNER_COUNT))
    return WondersSection(
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


def _summary(section: WondersSection, path: pathlib.Path) -> str:
    present = sum(sum(owner.presence) for owner in section.owners)
    return "\n".join(
        (
            f"{path}: Wonders {section.offset:#x}..{section.end:#x} "
            f"({section.size} bytes) sha256={section.sha256}",
            f"  PDB-layout sha256={section.layout_sha256}",
            f"  owners=8 present_wonders={present}",
            f"  next owner begins at {section.end:#x} (Forts::walk_data 0x0073ef30)",
        )
    )


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument("--offset", required=True, type=lambda text: int(text, 0))
    parser.add_argument("--schema", type=pathlib.Path, default=DEFAULT_SCHEMA_PATH)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    section = parse_wonders_section(_load(args.file), args.offset, schema_path=args.schema)
    print(json.dumps(_jsonable(section), indent=2) if args.json else _summary(section, args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
