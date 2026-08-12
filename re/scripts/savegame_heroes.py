#!/usr/bin/env python3
"""Parse the exact retail ``Heroes::walk_data`` generic-save image.

The helper starts at the caller tag owned by ``Heroes::walk_data``, preserves
all eight ``PtrArray<Hero>`` histories and presence planes, decodes each
``Hero`` and its nested ``Array<ActiveSpell>``, and stops before the following
caller-owned Herds tag.  Stream order comes from the matched PE; field names,
offsets, and widths are checked against the matched PDB export.
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


TAG_HEROES = 0x00
TAG_STRING_TABLE_INDEX = 3959
HERO_OWNER_COUNT = 8
MAX_ARRAY_LENGTH = 1 << 20

DEFAULT_SCHEMA_PATH = (
    pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"
)


class HeroesParseError(ValueError):
    """The stream or PDB layout contradicts the recovered Heroes traversal."""


@dataclasses.dataclass(frozen=True)
class ActiveSpellImage:
    index: int
    offset: int
    end: int
    type_index: int
    start: int
    frame: int
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class ActiveSpellArrayImage:
    offset: int
    end: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    spells: tuple[ActiveSpellImage, ...]
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class HeroSlot:
    index: int
    present: bool
    presence_offset: int
    offset: int | None
    end: int | None
    active_spells: ActiveSpellArrayImage | None
    hero: int | None
    o: int | None
    hero_flags: int | None
    who: int | None
    sha256: str | None

    @property
    def size(self) -> int:
        return 0 if self.offset is None or self.end is None else self.end - self.offset


@dataclasses.dataclass(frozen=True)
class HeroOwner:
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
    slots: tuple[HeroSlot, ...]
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class HeroesSection:
    offset: int
    end: int
    tag: int
    owners: tuple[HeroOwner, ...]
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class _Layout:
    sha256: str


_EXPECTED_HEROES = (("lists", 0, 224, "PtrArray<Hero>[8]"),)
_EXPECTED_PTR_ARRAY = (
    ("length", 4, 4, "int"),
    ("size", 8, 4, "int"),
    ("increment", 12, 2, "short"),
    ("list", 16, 4, "Hero**"),
    ("flags", 20, 1, "unsigned char"),
    ("cur_index", 24, 4, "int"),
)
_EXPECTED_HERO = (
    ("active_spells", 4, 28, "Array<ActiveSpell>"),
    ("hero", 36, 2, "short"),
    ("o", 38, 2, "short"),
    ("hero_flags", 40, 1, "char"),
    ("who", 41, 1, "char"),
)
_EXPECTED_CASTER = (("active_spells", 4, 28, "Array<ActiveSpell>"),)
_EXPECTED_ACTIVE_ARRAY = (
    ("length", 4, 4, "int"),
    ("size", 8, 4, "int"),
    ("increment", 12, 2, "short"),
    ("list", 16, 4, "ActiveSpell*"),
    ("flags", 20, 1, "unsigned char"),
    ("cur_index", 24, 4, "int"),
)
_EXPECTED_ACTIVE_SPELL = (
    ("t", 0, 4, "enum TypeIndex"),
    ("start", 4, 4, "int"),
    ("frame", 8, 4, "int"),
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
            "Heroes": classes["Heroes"],
            "PtrArray<Hero>": classes["PtrArray<Hero>"],
            "Hero": classes["Hero"],
            "Caster": classes["Caster"],
            "Array<ActiveSpell>": classes["Array<ActiveSpell>"],
            "ActiveSpell": classes["ActiveSpell"],
        }
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise HeroesParseError(
            f"cannot load Heroes PDB layout from {path}: {error}"
        ) from error

    expected = {
        "Heroes": (232, _EXPECTED_HEROES),
        "PtrArray<Hero>": (28, _EXPECTED_PTR_ARRAY),
        "Hero": (48, _EXPECTED_HERO),
        "Caster": (40, _EXPECTED_CASTER),
        "Array<ActiveSpell>": (28, _EXPECTED_ACTIVE_ARRAY),
        "ActiveSpell": (12, _EXPECTED_ACTIVE_SPELL),
    }
    receipt: dict[str, object] = {}
    for name, (size, fields) in expected.items():
        record = records[name]
        actual_fields = _field_tuples(record)
        if record.get("size") != size or actual_fields != fields:
            raise HeroesParseError(
                f"PDB {name} layout disagrees: size={record.get('size')}, "
                f"fields={actual_fields!r}"
            )
        receipt[name] = {"size": record["size"], "flattened": actual_fields}

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
            raise HeroesParseError(
                f"offset {offset:#x} is outside {len(self.data):#x}-byte stream"
            )
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise HeroesParseError(
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


def _parse_active_spells(reader: _Reader, owner_index: int, slot_index: int) -> ActiveSpellArrayImage:
    start = reader.pos
    name = f"Heroes[{owner_index}][{slot_index}].active_spells"
    length = reader.i32(f"{name}.length")
    if length < 0 or length > MAX_ARRAY_LENGTH:
        raise HeroesParseError(f"{name} has invalid length {length}")
    if length == 0:
        return ActiveSpellArrayImage(
            offset=start,
            end=reader.pos,
            length=0,
            capacity=None,
            increment=None,
            flags=None,
            spells=(),
            sha256=hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
        )

    capacity = reader.i32(f"{name}.capacity")
    increment = reader.i16(f"{name}.increment")
    flags = reader.u8(f"{name}.flags")
    if capacity < length or capacity > MAX_ARRAY_LENGTH:
        raise HeroesParseError(
            f"{name} has invalid history length={length}, capacity={capacity}"
        )
    if flags & 0x40:
        raise HeroesParseError(
            f"{name} flags {flags:#04x} retain writer-cleared bit 0x40"
        )

    spells = []
    for index in range(length):
        row_start = reader.pos
        type_index = reader.i32(f"{name}[{index}].t")
        spell_start = reader.i32(f"{name}[{index}].start")
        frame = reader.i32(f"{name}[{index}].frame")
        spells.append(
            ActiveSpellImage(
                index=index,
                offset=row_start,
                end=reader.pos,
                type_index=type_index,
                start=spell_start,
                frame=frame,
                sha256=hashlib.sha256(
                    reader.data[row_start : reader.pos]
                ).hexdigest(),
            )
        )
    return ActiveSpellArrayImage(
        offset=start,
        end=reader.pos,
        length=length,
        capacity=capacity,
        increment=increment,
        flags=flags,
        spells=tuple(spells),
        sha256=hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
    )


def _empty_slot(index: int, presence_offset: int) -> HeroSlot:
    return HeroSlot(
        index=index,
        present=False,
        presence_offset=presence_offset,
        offset=None,
        end=None,
        active_spells=None,
        hero=None,
        o=None,
        hero_flags=None,
        who=None,
        sha256=None,
    )


def _parse_slot(
    reader: _Reader, owner_index: int, index: int, presence_offset: int
) -> HeroSlot:
    start = reader.pos
    active_spells = _parse_active_spells(reader, owner_index, index)
    hero = reader.i16(f"Heroes[{owner_index}][{index}].hero")
    o = reader.i16(f"Heroes[{owner_index}][{index}].o")
    hero_flags = reader.i8(f"Heroes[{owner_index}][{index}].hero_flags")
    who = reader.i8(f"Heroes[{owner_index}][{index}].who")
    return HeroSlot(
        index=index,
        present=True,
        presence_offset=presence_offset,
        offset=start,
        end=reader.pos,
        active_spells=active_spells,
        hero=hero,
        o=o,
        hero_flags=hero_flags,
        who=who,
        sha256=hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
    )


def _parse_owner(reader: _Reader, index: int) -> HeroOwner:
    start = reader.pos
    name = f"Heroes.owners[{index}]"
    length = reader.i32(f"{name}.length")
    if length < 0 or length > MAX_ARRAY_LENGTH:
        raise HeroesParseError(f"{name} has invalid length {length}")
    if length == 0:
        return HeroOwner(
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
            sha256=hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
        )

    capacity = reader.i32(f"{name}.capacity")
    increment = reader.i16(f"{name}.increment")
    flags = reader.u8(f"{name}.flags")
    if capacity < length or capacity > MAX_ARRAY_LENGTH:
        raise HeroesParseError(
            f"{name} has invalid history length={length}, capacity={capacity}"
        )
    if flags & 0x40:
        raise HeroesParseError(
            f"{name} flags {flags:#04x} retain writer-cleared bit 0x40"
        )

    presence_offset = reader.pos
    presence = tuple(reader.u8(f"{name}.presence[{slot}]") for slot in range(length))
    if any(value not in (0, 1) for value in presence):
        raise HeroesParseError(f"{name} pointer-presence plane is not boolean")

    repeated_capacity_offset = reader.pos
    repeated_capacity = reader.i32(f"{name}.repeated_capacity")
    repeated_increment_offset = reader.pos
    repeated_increment = reader.i16(f"{name}.repeated_increment")
    if (repeated_capacity, repeated_increment) != (capacity, increment):
        raise HeroesParseError(
            f"{name} duplicated capacity/increment image disagrees: "
            f"({capacity}, {increment}) != "
            f"({repeated_capacity}, {repeated_increment})"
        )

    slots = tuple(
        _parse_slot(reader, index, slot, presence_offset + slot)
        if present
        else _empty_slot(slot, presence_offset + slot)
        for slot, present in enumerate(presence)
    )
    return HeroOwner(
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
        slots=slots,
        sha256=hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
    )


def parse_heroes_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tag: bool = True,
    schema_path: pathlib.Path | None = None,
) -> HeroesSection:
    """Parse one complete retail ``Heroes::walk_data`` at ``offset``.

    The returned ``end`` is the first byte owned by the caller's Herds tag.
    Both levels of container history are preserved without normalization.
    """

    layout = _load_layout(schema_path)
    reader = _Reader(data, offset)
    tag = reader.u8("Heroes tag")
    if require_tag and tag != TAG_HEROES:
        raise HeroesParseError(
            f"Heroes tag {tag:#04x} != {TAG_HEROES:#04x} at {offset:#x}"
        )
    owners = tuple(_parse_owner(reader, index) for index in range(HERO_OWNER_COUNT))
    return HeroesSection(
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


def _summary(section: HeroesSection, path: pathlib.Path) -> str:
    present = sum(sum(owner.presence) for owner in section.owners)
    spells = sum(
        slot.active_spells.length
        for owner in section.owners
        for slot in owner.slots
        if slot.active_spells is not None
    )
    return "\n".join(
        (
            f"{path}: Heroes {section.offset:#x}..{section.end:#x} "
            f"({section.size} bytes) sha256={section.sha256}",
            f"  PDB-layout sha256={section.layout_sha256}",
            f"  owners=8 present_heroes={present} active_spells={spells}",
            f"  next owner begins at {section.end:#x} "
            "(caller Herds tag; StringTable[3958])",
        )
    )


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument(
        "--offset",
        required=True,
        type=lambda text: int(text, 0),
        help="decompressed stream offset of the Heroes tag",
    )
    parser.add_argument("--schema", type=pathlib.Path, default=DEFAULT_SCHEMA_PATH)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    section = parse_heroes_section(
        _load(args.file), args.offset, schema_path=args.schema
    )
    if args.json:
        print(json.dumps(_jsonable(section), indent=2))
    else:
        print(_summary(section, args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
