#!/usr/bin/env python3
"""Parse the complete retail ``Cities::walk_data`` save-stream section.

The helper starts at the Cities walk-test byte, follows all eight
``PtrArray<City>`` histories, and decodes every present City's fixed POD,
save-only UTF-16 strings, and ``Array<CaravanLink>`` child.  It stops before
the caller's following Forms walk-test and does not touch the shared parser.

Stream order and conditional branches come from the shipped PE.  Names,
offsets, and compiler widths are checked at runtime against the matching PDB
export in ``schema/pdb-types.json``.
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


TAG_CITIES = 0x00
TAG_STRING_TABLE_INDEX = 513
OWNER_COUNT = 8
MAX_ARRAY_LENGTH = 1 << 20
MAX_STRING_CODE_UNITS = 0xFFFF
CITY_ACTIVE = 0x0001

DEFAULT_SCHEMA_PATH = (
    pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"
)


class CitiesParseError(ValueError):
    """The stream or PDB layout contradicts the recovered Cities traversal."""


@dataclasses.dataclass(frozen=True)
class CityField:
    name: str
    pdb_offset: int
    type_name: str
    offset: int
    end: int
    values: tuple[int, ...]


@dataclasses.dataclass(frozen=True)
class WideStringImage:
    name: str
    offset: int
    end: int
    length: int
    code_units: tuple[int, ...]
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class CaravanLinkImage:
    index: int
    offset: int
    end: int
    cara: int
    who: int


@dataclasses.dataclass(frozen=True)
class CaravanArrayImage:
    offset: int
    end: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    links: tuple[CaravanLinkImage, ...]
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class CitySlot:
    index: int
    present: bool
    presence_offset: int
    offset: int | None
    end: int | None
    city_flags_offset: int | None
    city_flags: int | None
    pod_end: int | None
    fields: tuple[CityField, ...]
    name: WideStringImage | None
    city_id: WideStringImage | None
    caravans: CaravanArrayImage | None
    sha256: str | None

    @property
    def active(self) -> bool:
        return self.city_flags is not None and bool(self.city_flags & CITY_ACTIVE)


@dataclasses.dataclass(frozen=True)
class CityOwner:
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
    slots: tuple[CitySlot, ...]

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class CitiesSection:
    offset: int
    end: int
    tag: int
    owners: tuple[CityOwner, ...]
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
    pod_fields: tuple[_FieldLayout, ...]
    sha256: str


_EXPECTED_PTR_ARRAY = (
    ("length", 4, 4, "int"),
    ("size", 8, 4, "int"),
    ("increment", 12, 2, "short"),
    ("list", 16, 4, "City**"),
    ("flags", 20, 1, "unsigned char"),
    ("cur_index", 24, 4, "int"),
)

_EXPECTED_CITY_FIELDS = (
    ("city_flags", 4, 2, "short"),
    ("city", 6, 2, "short"),
    ("o", 8, 2, "short"),
    ("reg", 10, 2, "short"),
    ("x", 12, 4, "Coord"),
    ("y", 16, 4, "Coord"),
    ("attack_stamp", 20, 4, "int"),
    ("raid_stamp", 24, 4, "int"),
    ("reduce_stamp", 28, 4, "int"),
    ("capture_stamp", 32, 4, "int"),
    ("assimilation_timer", 36, 4, "int"),
    ("capture_strength", 40, 4, "int"),
    ("traded_with", 44, 32, "int[8]"),
    ("scouted", 76, 2, "short"),
    ("in_port", 78, 2, "short"),
    ("peasant_dist", 80, 2, "short"),
    ("trade_val", 82, 2, "short"),
    ("conquest_node", 84, 2, "short"),
    ("granary", 86, 1, "unsigned char"),
    ("lumber_mill", 87, 1, "unsigned char"),
    ("smelter", 88, 1, "unsigned char"),
    ("refinery", 89, 1, "unsigned char"),
    ("free", 90, 1, "unsigned char"),
    ("busy", 91, 1, "unsigned char"),
    ("gatherers", 92, 1, "unsigned char"),
    ("pop", 93, 1, "unsigned char"),
    ("who", 94, 1, "char"),
    ("race", 95, 1, "char"),
    ("founder", 96, 1, "char"),
    ("plundered", 97, 1, "unsigned char"),
    ("ocean", 98, 1, "unsigned char"),
    ("land", 99, 1, "unsigned char"),
    ("filled", 100, 1, "unsigned char"),
    ("bordering", 101, 1, "unsigned char"),
    ("ocean_filled", 102, 1, "unsigned char"),
    ("dock_tile", 103, 1, "unsigned char"),
    ("was_capital_flags", 104, 1, "unsigned char"),
    ("space", 105, 3, "unsigned char[3]"),
    ("ter", 108, 6, "unsigned char[6]"),
    ("vans", 116, 28, "Array<CaravanLink>"),
    ("name", 144, 20, "String"),
    ("id", 164, 20, "String"),
)

_EXPECTED_CARAVAN_ARRAY = (
    ("length", 4, 4, "int"),
    ("size", 8, 4, "int"),
    ("increment", 12, 2, "short"),
    ("list", 16, 4, "CaravanLink*"),
    ("flags", 20, 1, "unsigned char"),
    ("cur_index", 24, 4, "int"),
)

_EXPECTED_CARAVAN_LINK = (
    ("cara", 0, 4, "int"),
    ("who", 4, 4, "int"),
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
        cities = classes["Cities"]
        ptr_array = classes["PtrArray<City>"]
        city = classes["City"]
        caravan_array = classes["Array<CaravanLink>"]
        caravan_link = classes["CaravanLink"]
        string = classes["String"]
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise CitiesParseError(
            f"cannot load Cities PDB layout from {path}: {error}"
        ) from error

    cities_fields = _field_tuples(cities)
    ptr_fields = _field_tuples(ptr_array)
    city_fields = _field_tuples(city)
    caravan_array_fields = _field_tuples(caravan_array)
    caravan_link_fields = _field_tuples(caravan_link)
    if cities.get("size") != 232 or cities_fields != (
        ("lists", 0, 224, "PtrArray<City>[8]"),
    ):
        raise CitiesParseError(
            f"PDB Cities layout disagrees: size={cities.get('size')}, "
            f"fields={cities_fields!r}"
        )
    if ptr_array.get("size") != 28 or ptr_fields != _EXPECTED_PTR_ARRAY:
        raise CitiesParseError(
            f"PDB PtrArray<City> layout disagrees: size={ptr_array.get('size')}, "
            f"fields={ptr_fields!r}"
        )
    if city.get("size") != 192 or city_fields != _EXPECTED_CITY_FIELDS:
        raise CitiesParseError(
            f"PDB City layout disagrees: size={city.get('size')}, "
            f"fields={city_fields!r}"
        )
    if (
        caravan_array.get("size") != 28
        or caravan_array_fields != _EXPECTED_CARAVAN_ARRAY
    ):
        raise CitiesParseError(
            "PDB Array<CaravanLink> layout disagrees: "
            f"size={caravan_array.get('size')}, fields={caravan_array_fields!r}"
        )
    if (
        caravan_link.get("size") != 8
        or caravan_link_fields != _EXPECTED_CARAVAN_LINK
    ):
        raise CitiesParseError(
            f"PDB CaravanLink layout disagrees: size={caravan_link.get('size')}, "
            f"fields={caravan_link_fields!r}"
        )
    string_curr_len = tuple(
        (field["offset"], field["size"], field["type"])
        for field in string["flattened"]
        if field["name"] == "curr_len"
    )
    if string.get("size") != 20 or string_curr_len != ((8, 2, "unsigned short"),):
        raise CitiesParseError(
            f"PDB String layout disagrees: size={string.get('size')}, "
            f"curr_len={string_curr_len!r}"
        )

    cursor = 4
    for name, offset, size, _type_name in city_fields[:39]:
        if offset != cursor:
            raise CitiesParseError(
                f"PDB City POD has a gap before {name}: {cursor:#x}..{offset:#x}"
            )
        cursor += size
    if cursor != 114:
        raise CitiesParseError(f"PDB City POD ends at {cursor:#x}, expected 0x72")

    receipt = {
        "Cities": {"size": cities["size"], "flattened": cities_fields},
        "PtrArray<City>": {
            "size": ptr_array["size"],
            "flattened": ptr_fields,
        },
        "City": {"size": city["size"], "flattened": city_fields},
        "Array<CaravanLink>": {
            "size": caravan_array["size"],
            "flattened": caravan_array_fields,
        },
        "CaravanLink": {
            "size": caravan_link["size"],
            "flattened": caravan_link_fields,
        },
        "String": {"size": string["size"], "curr_len": string_curr_len},
    }
    digest = hashlib.sha256(
        json.dumps(receipt, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()
    return _Layout(
        pod_fields=tuple(
            _FieldLayout(str(name), int(offset), int(size), str(type_name))
            for name, offset, size, type_name in city_fields[1:39]
        ),
        sha256=digest,
    )


def _load_layout(schema_path: pathlib.Path | None) -> _Layout:
    return _load_layout_cached(str((schema_path or DEFAULT_SCHEMA_PATH).resolve()))


class _Reader:
    def __init__(self, data: bytes | bytearray | memoryview, offset: int):
        self.data = memoryview(data)
        if offset < 0 or offset > len(self.data):
            raise CitiesParseError(
                f"offset {offset:#x} is outside {len(self.data):#x}-byte stream"
            )
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise CitiesParseError(
                f"{what} range [{self.pos:#x},{end:#x}) exceeds "
                f"{len(self.data):#x}-byte stream"
            )
        start = self.pos
        self.pos = end
        return self.data[start:end]

    def u8(self, what: str) -> int:
        return self.take(1, what)[0]

    def u16(self, what: str) -> int:
        return struct.unpack("<H", self.take(2, what))[0]

    def i16(self, what: str) -> int:
        return struct.unpack("<h", self.take(2, what))[0]

    def u32(self, what: str) -> int:
        return struct.unpack("<I", self.take(4, what))[0]

    def i32(self, what: str) -> int:
        return struct.unpack("<i", self.take(4, what))[0]


def _decode_field(reader: _Reader, field: _FieldLayout) -> CityField:
    start = reader.pos
    raw = reader.take(field.size, f"City.{field.name}")
    if field.type_name == "short":
        values = (struct.unpack("<h", raw)[0],)
    elif field.type_name in ("int", "Coord"):
        values = (struct.unpack("<i", raw)[0],)
    elif field.type_name == "int[8]":
        values = struct.unpack("<8i", raw)
    elif field.type_name == "unsigned char":
        values = (raw[0],)
    elif field.type_name == "char":
        values = (struct.unpack("<b", raw)[0],)
    elif field.type_name == "unsigned char[3]":
        values = tuple(raw)
    elif field.type_name == "unsigned char[6]":
        values = tuple(raw)
    else:  # Protected by the exact PDB receipt.
        raise CitiesParseError(f"unsupported PDB City field type {field.type_name}")
    return CityField(
        name=field.name,
        pdb_offset=field.offset,
        type_name=field.type_name,
        offset=start,
        end=reader.pos,
        values=values,
    )


def _parse_string(reader: _Reader, name: str) -> WideStringImage:
    start = reader.pos
    length = reader.u32(f"City.{name}.length")
    if length > MAX_STRING_CODE_UNITS:
        raise CitiesParseError(
            f"City.{name} length {length} exceeds writer's unsigned-short curr_len"
        )
    payload = reader.take(length * 2, f"City.{name}.UTF-16LE")
    code_units = struct.unpack(f"<{length}H", payload) if length else ()
    return WideStringImage(
        name=name,
        offset=start,
        end=reader.pos,
        length=length,
        code_units=code_units,
        sha256=hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
    )


def _parse_caravans(reader: _Reader) -> CaravanArrayImage:
    start = reader.pos
    length = reader.i32("City.vans.length")
    if length < 0 or length > MAX_ARRAY_LENGTH:
        raise CitiesParseError(f"City.vans has invalid length {length} at {start:#x}")
    if length == 0:
        return CaravanArrayImage(
            offset=start,
            end=reader.pos,
            length=0,
            capacity=None,
            increment=None,
            flags=None,
            links=(),
            sha256=hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
        )

    capacity = reader.i32("City.vans.capacity")
    increment = reader.i16("City.vans.increment")
    flags = reader.u8("City.vans.flags")
    if capacity < length or capacity > MAX_ARRAY_LENGTH:
        raise CitiesParseError(
            f"City.vans has invalid history length={length}, capacity={capacity}"
        )
    if flags & 0x40:
        raise CitiesParseError(
            f"City.vans flags {flags:#04x} retain writer-cleared bit 0x40"
        )
    links = []
    for index in range(length):
        link_start = reader.pos
        links.append(
            CaravanLinkImage(
                index=index,
                offset=link_start,
                end=link_start + 8,
                cara=reader.i32(f"City.vans[{index}].cara"),
                who=reader.i32(f"City.vans[{index}].who"),
            )
        )
    return CaravanArrayImage(
        offset=start,
        end=reader.pos,
        length=length,
        capacity=capacity,
        increment=increment,
        flags=flags,
        links=tuple(links),
        sha256=hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
    )


def _empty_slot(index: int, presence_offset: int) -> CitySlot:
    return CitySlot(
        index=index,
        present=False,
        presence_offset=presence_offset,
        offset=None,
        end=None,
        city_flags_offset=None,
        city_flags=None,
        pod_end=None,
        fields=(),
        name=None,
        city_id=None,
        caravans=None,
        sha256=None,
    )


def _parse_city(
    reader: _Reader,
    index: int,
    presence_offset: int,
    layout: _Layout,
) -> CitySlot:
    start = reader.pos
    city_flags_offset = reader.pos
    city_flags = reader.u16(f"City[{index}].city_flags")
    if city_flags & CITY_ACTIVE == 0:
        return CitySlot(
            index=index,
            present=True,
            presence_offset=presence_offset,
            offset=start,
            end=reader.pos,
            city_flags_offset=city_flags_offset,
            city_flags=city_flags,
            pod_end=reader.pos,
            fields=(),
            name=None,
            city_id=None,
            caravans=None,
            sha256=hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
        )

    fields = tuple(_decode_field(reader, field) for field in layout.pod_fields)
    pod_end = reader.pos
    name = _parse_string(reader, "name")
    city_id = _parse_string(reader, "id")
    caravans = _parse_caravans(reader)
    return CitySlot(
        index=index,
        present=True,
        presence_offset=presence_offset,
        offset=start,
        end=reader.pos,
        city_flags_offset=city_flags_offset,
        city_flags=city_flags,
        pod_end=pod_end,
        fields=fields,
        name=name,
        city_id=city_id,
        caravans=caravans,
        sha256=hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
    )


def _parse_owner(reader: _Reader, index: int, layout: _Layout) -> CityOwner:
    start = reader.pos
    length = reader.i32(f"Cities.lists[{index}].length")
    if length < 0 or length > MAX_ARRAY_LENGTH:
        raise CitiesParseError(
            f"Cities.lists[{index}] has invalid length {length} at {start:#x}"
        )
    if length == 0:
        return CityOwner(
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

    capacity = reader.i32(f"Cities.lists[{index}].capacity")
    increment = reader.i16(f"Cities.lists[{index}].increment")
    flags = reader.u8(f"Cities.lists[{index}].flags")
    if capacity < length or capacity > MAX_ARRAY_LENGTH:
        raise CitiesParseError(
            f"Cities.lists[{index}] has invalid history "
            f"length={length}, capacity={capacity}"
        )
    if flags & 0x40:
        raise CitiesParseError(
            f"Cities.lists[{index}] flags {flags:#04x} retain writer-cleared bit 0x40"
        )

    presence_offset = reader.pos
    presence = tuple(
        reader.u8(f"Cities.lists[{index}].presence[{slot}]")
        for slot in range(length)
    )
    if any(value not in (0, 1) for value in presence):
        raise CitiesParseError(
            f"Cities.lists[{index}] pointer-presence plane is not boolean"
        )

    repeated_capacity_offset = reader.pos
    repeated_capacity = reader.i32(f"Cities.lists[{index}].repeated_capacity")
    repeated_increment_offset = reader.pos
    repeated_increment = reader.i16(f"Cities.lists[{index}].repeated_increment")
    if (repeated_capacity, repeated_increment) != (capacity, increment):
        raise CitiesParseError(
            f"Cities.lists[{index}] duplicated capacity/increment image disagrees: "
            f"({capacity}, {increment}) != "
            f"({repeated_capacity}, {repeated_increment})"
        )

    slots = []
    for slot, present in enumerate(presence):
        marker_offset = presence_offset + slot
        slots.append(
            _parse_city(reader, slot, marker_offset, layout)
            if present
            else _empty_slot(slot, marker_offset)
        )
    return CityOwner(
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


def parse_cities_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tag: bool = True,
    schema_path: pathlib.Path | None = None,
) -> CitiesSection:
    """Parse exactly one retail Cities section beginning at ``offset``.

    The returned ``end`` is the caller-owned Forms walk-test immediately
    before ``ObjectArray<Form>::walk_data``.
    """

    layout = _load_layout(schema_path)
    reader = _Reader(data, offset)
    tag = reader.u8("Cities tag")
    if require_tag and tag != TAG_CITIES:
        raise CitiesParseError(
            f"Cities tag {tag:#04x} != {TAG_CITIES:#04x} at {offset:#x}"
        )
    owners = tuple(_parse_owner(reader, index, layout) for index in range(OWNER_COUNT))
    return CitiesSection(
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
                "city_flags_offset",
                "pod_end",
                "pdb_offset",
            ) and field_value is not None:
                result[field.name] = f"0x{field_value:x}"
            else:
                result[field.name] = _jsonable(field_value)
        if isinstance(
            value,
            (WideStringImage, CaravanArrayImage, CityOwner, CitiesSection),
        ):
            result["size"] = value.size
        return result
    if isinstance(value, tuple):
        return [_jsonable(item) for item in value]
    return value


def _summary(section: CitiesSection, path: pathlib.Path) -> str:
    lines = [
        f"{path}: Cities {section.offset:#x}..{section.end:#x} "
        f"({section.size} bytes) sha256={section.sha256}",
        f"  tag={section.tag:#04x} (StringTable[{TAG_STRING_TABLE_INDEX}]); "
        f"PDB-layout sha256={section.layout_sha256}",
    ]
    for owner in section.owners:
        if owner.length == 0:
            lines.append(f"  owner {owner.index}: empty ({owner.size} bytes)")
        else:
            active = sum(slot.active for slot in owner.slots)
            links = sum(
                slot.caravans.length
                for slot in owner.slots
                if slot.caravans is not None
            )
            lines.append(
                f"  owner {owner.index}: length={owner.length} "
                f"capacity={owner.capacity} increment={owner.increment} "
                f"flags={owner.flags:#04x} present={sum(owner.presence)} "
                f"active={active} caravan_links={links} ({owner.size} bytes)"
            )
    lines.append(
        f"  next owner begins at {section.end:#x} "
        "(Forms walk-test then ObjectArray<Form>::walk_data 0x00481190)"
    )
    return "\n".join(lines)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument(
        "--offset",
        required=True,
        type=lambda text: int(text, 0),
        help="decompressed stream offset of the Cities tag",
    )
    parser.add_argument("--schema", type=pathlib.Path, default=DEFAULT_SCHEMA_PATH)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    section = parse_cities_section(
        _load(args.file), args.offset, schema_path=args.schema
    )
    if args.json:
        print(json.dumps(_jsonable(section), indent=2))
    else:
        print(_summary(section, args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
