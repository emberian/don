#!/usr/bin/env python3
"""Parse the exact retail ``PtrArray<River>`` save image.

The installed SVX is damaged inside the preceding ``CommandManager`` owner,
so this helper intentionally accepts an explicit synthetic/known-good offset.
It preserves both pointer-array history images, every River and spline
presence byte, the complete ``SplineData`` body, and the six nested array
histories.  PE control flow defines stream order; PDB records define member
offsets and widths.
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


MAX_ARRAY_LENGTH = 1 << 20
SPLINE_DATA_TAG_STRING_TABLE_INDEX = 6209
NEXT_OWNER = "Terrain::walk_coord_data"
DEFAULT_SCHEMA_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"


class RiversParseError(ValueError):
    """The stream or PDB layout contradicts the recovered River traversal."""


@dataclasses.dataclass(frozen=True)
class ArrayImage:
    name: str
    element_size: int
    offset: int
    end: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    payload: bytes
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class SplineImage:
    offset: int
    end: int
    tag: int
    type: int
    flags: int
    max_control_depth_ratio_bits: int
    total_spline_length_bits: int
    last_knot: int
    curr_dist_bits: int
    next_search_dist_bits: int
    search_scan: int
    degree: int
    depth: int
    arrays: tuple[ArrayImage, ...]
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class RiverSlot:
    index: int
    present: bool
    presence_offset: int
    offset: int | None
    end: int | None
    spline_presence_offset: int | None
    spline_present: int | None
    spline: SplineImage | None
    sha256: str | None

    @property
    def size(self) -> int:
        return 0 if self.offset is None or self.end is None else self.end - self.offset


@dataclasses.dataclass(frozen=True)
class RiversSection:
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
    slots: tuple[RiverSlot, ...]
    sha256: str
    layout_sha256: str
    next_owner: str

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
    ("list", 16, 4, None),
    ("flags", 20, 1, "unsigned char"),
    ("cur_index", 24, 4, "int"),
)

_RIVER_FIELDS = (
    ("creation_spline", 8, 4, "Spline*"),
    ("creation_distance", 12, 4, "float"),
    ("max_width", 16, 4, "float"),
    ("all_sections", 20, 28, "PtrArray<RiverSection>"),
    ("width_fractal", 48, 120, "Fractal"),
    ("cur_frac_x", 168, 4, "int"),
    ("cur_frac_y", 172, 4, "int"),
    ("last_width", 176, 4, "float"),
    ("tcoords_to_smooth", 180, 28, "Array<int>"),
    ("first", 208, 1, "unsigned char"),
    ("calculated", 209, 1, "unsigned char"),
    ("delta_start", 212, 4, "int"),
    ("debug_creation_spline_rs", 216, 360, "RenderState"),
    ("wcoords_touched", 576, 12, "DynamicBitMask"),
    ("sections_to_render", 588, 28, "Array<unsigned short>"),
    ("sections_per_wcoord", 616, 28, "PtrArray<RiverWCoordSections>"),
    ("spot_update_wcoords", 644, 28, "WCoordList"),
    ("tcoords_added", 672, 12, "DynamicBitMask"),
)

_SPLINE_FIELDS = (
    ("registered_params", 4, 28, "NamedArray<void *>"),
    ("registered_param_descs", 32, 28, "Array<RegisteredVarDesc>"),
    ("type", 64, 4, "int"),
    ("flags", 68, 4, "int"),
    ("max_control_depth_ratio", 72, 4, "float"),
    ("total_spline_length", 76, 4, "float"),
    ("last_knot", 80, 4, "int"),
    ("curr_dist", 84, 4, "float"),
    ("next_search_dist", 88, 4, "float"),
    ("search_scan", 92, 4, "int"),
    ("degree", 96, 2, "unsigned short"),
    ("depth", 98, 2, "unsigned short"),
    ("control_verts", 100, 28, "Vert3Array"),
    ("knots", 128, 28, "SimpleArray<float>"),
    ("weights", 156, 28, "SimpleArray<float>"),
    ("spline_knots", 184, 28, "SimpleArray<float>"),
    ("spline_verts", 212, 28, "Vert3Array"),
    ("spline_normals", 240, 28, "Vert3Array"),
)


def _field_tuples(record: dict[str, object]) -> tuple[tuple[object, ...], ...]:
    return tuple(
        (field["name"], field["offset"], field["size"], field["type"])
        for field in record["flattened"]  # type: ignore[index]
    )


def _bases(record: dict[str, object]) -> tuple[tuple[object, ...], ...]:
    return tuple((base["name"], base["offset"], base["size"]) for base in record["bases"])  # type: ignore[index]


@functools.lru_cache(maxsize=4)
def _load_layout(path_text: str) -> _Layout:
    path = pathlib.Path(path_text)
    try:
        classes = json.loads(path.read_bytes())["classes"]
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise RiversParseError(f"cannot load River PDB layout from {path}: {error}") from error

    expected = {
        "PtrArray<River>": (28, _ARRAY_FIELDS[:3] + (("list", 16, 4, "River**"),) + _ARRAY_FIELDS[4:]),
        "River": (696, _RIVER_FIELDS),
        "SplineData": (268, _SPLINE_FIELDS),
        "Vert3Array": (28, _ARRAY_FIELDS[:3] + (("list", 16, 4, "Vector<float>*"),) + _ARRAY_FIELDS[4:]),
        "Array<Vector<float> >": (28, _ARRAY_FIELDS[:3] + (("list", 16, 4, "Vector<float>*"),) + _ARRAY_FIELDS[4:]),
        "SimpleArray<float>": (28, _ARRAY_FIELDS[:3] + (("list", 16, 4, "float*"),) + _ARRAY_FIELDS[4:]),
    }
    receipt: dict[str, object] = {}
    for name, (size, fields) in expected.items():
        record = classes.get(name)
        actual = _field_tuples(record) if record else ()
        if not record or record.get("size") != size or actual != fields:
            raise RiversParseError(
                f"PDB {name} layout disagrees: size={record.get('size') if record else None}, fields={actual!r}"
            )
        receipt[name] = {"size": size, "flattened": fields}

    actual_bases = {name: _bases(classes[name]) for name in expected}
    expected_bases = {
        "PtrArray<River>": (("Array<River *>", 0, 28),),
        "River": (("RiverOut", 8, 680), ("GameAccess", 689, 1)),
        "SplineData": (("BaseParamRegister", 0, 60), ("Recycler<Spline>", 60, 1), ("GameAccessConst", 61, 1)),
        "Vert3Array": (("Array<Vector<float> >", 0, 28),),
        "Array<Vector<float> >": (("ArrayBaseSimpleCopy<Vector<float> >", 0, 24),),
        "SimpleArray<float>": (("ArrayBaseSimpleCopy<float>", 0, 24),),
    }
    if actual_bases != expected_bases:
        raise RiversParseError(f"PDB River bases disagree: {actual_bases!r}")
    receipt["bases"] = actual_bases
    receipt["stream_order"] = (
        "control_verts",
        "knots",
        "spline_knots",
        "weights",
        "spline_verts",
        "spline_normals",
    )
    receipt["spline_tag_index"] = SPLINE_DATA_TAG_STRING_TABLE_INDEX
    receipt["next_owner"] = NEXT_OWNER
    encoded = json.dumps(receipt, sort_keys=True, separators=(",", ":")).encode()
    return _Layout(hashlib.sha256(encoded).hexdigest())


class _Reader:
    def __init__(self, data: bytes | bytearray | memoryview, offset: int):
        self.data = memoryview(data)
        if offset < 0 or offset > len(self.data):
            raise RiversParseError(f"offset {offset:#x} is outside {len(self.data):#x}-byte stream")
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise RiversParseError(
                f"{what} range [{self.pos:#x},{end:#x}) exceeds {len(self.data):#x}-byte stream"
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


def _count(reader: _Reader, what: str) -> int:
    value = reader.i32(what)
    if value < 0 or value > MAX_ARRAY_LENGTH:
        raise RiversParseError(f"{what} is invalid: {value}")
    return value


def _parse_array(reader: _Reader, name: str, element_size: int) -> ArrayImage:
    start = reader.pos
    length = _count(reader, f"{name}.length")
    capacity = increment = flags = None
    payload = b""
    if length:
        capacity = reader.i32(f"{name}.capacity")
        increment = reader.i16(f"{name}.increment")
        flags = reader.u8(f"{name}.flags")
        if capacity < length or capacity > MAX_ARRAY_LENGTH:
            raise RiversParseError(
                f"{name} has invalid history length={length}, capacity={capacity}"
            )
        if flags & 0x40:
            raise RiversParseError(f"{name} flags {flags:#04x} retain writer-cleared bit 0x40")
        payload = bytes(reader.take(length * element_size, f"{name}.elements"))
    raw = bytes(reader.data[start:reader.pos])
    return ArrayImage(
        name, element_size, start, reader.pos, length, capacity, increment,
        flags, payload, hashlib.sha256(raw).hexdigest()
    )


def _parse_spline(reader: _Reader, river_index: int) -> SplineImage:
    start = reader.pos
    label = f"Rivers[{river_index}].creation_spline"
    tag = reader.u8(f"{label}.SplineData tag")
    values = (
        reader.i32(f"{label}.type"),
        reader.i32(f"{label}.flags"),
        reader.u32(f"{label}.max_control_depth_ratio bits"),
        reader.u32(f"{label}.total_spline_length bits"),
        reader.i32(f"{label}.last_knot"),
        reader.u32(f"{label}.curr_dist bits"),
        reader.u32(f"{label}.next_search_dist bits"),
        reader.i32(f"{label}.search_scan"),
        reader.u16(f"{label}.degree"),
        reader.u16(f"{label}.depth"),
    )
    arrays = tuple(
        _parse_array(reader, f"{label}.{name}", element_size)
        for name, element_size in (
            ("control_verts", 12),
            ("knots", 4),
            ("spline_knots", 4),
            ("weights", 4),
            ("spline_verts", 12),
            ("spline_normals", 12),
        )
    )
    raw = bytes(reader.data[start:reader.pos])
    return SplineImage(
        start, reader.pos, tag, *values, arrays, hashlib.sha256(raw).hexdigest()
    )


def _empty_slot(index: int, presence_offset: int) -> RiverSlot:
    return RiverSlot(index, False, presence_offset, None, None, None, None, None, None)


def _parse_slot(reader: _Reader, index: int, presence_offset: int) -> RiverSlot:
    start = reader.pos
    spline_presence_offset = reader.pos
    # River::walk_data uses an int temporary for its Spline* marker, unlike
    # PtrArray's byte-wide pointer-presence plane.
    spline_present = reader.i32(f"Rivers[{index}].creation_spline presence")
    if spline_present not in (0, 1):
        raise RiversParseError(
            f"Rivers[{index}] creation_spline presence is not boolean: {spline_present}"
        )
    spline = _parse_spline(reader, index) if spline_present else None
    raw = bytes(reader.data[start:reader.pos])
    return RiverSlot(
        index, True, presence_offset, start, reader.pos, spline_presence_offset,
        spline_present, spline, hashlib.sha256(raw).hexdigest()
    )


def parse_rivers_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    schema_path: pathlib.Path = DEFAULT_SCHEMA_PATH,
) -> RiversSection:
    """Parse one complete retail ``PtrArray<River>`` at ``offset``.

    ``end`` is the first byte owned by ``Terrain::walk_coord_data``.  It is a
    logical boundary only until an undamaged SVX provides an installed offset.
    """

    layout = _load_layout(str(pathlib.Path(schema_path).resolve()))
    reader = _Reader(data, offset)
    length = _count(reader, "Rivers.length")
    if not length:
        raw = bytes(reader.data[offset:reader.pos])
        return RiversSection(
            offset, reader.pos, 0, None, None, None, None, (), None, None,
            None, None, (), hashlib.sha256(raw).hexdigest(), layout.sha256,
            NEXT_OWNER,
        )

    capacity = reader.i32("Rivers.capacity")
    increment = reader.i16("Rivers.increment")
    flags = reader.u8("Rivers.flags")
    if capacity < length or capacity > MAX_ARRAY_LENGTH:
        raise RiversParseError(
            f"Rivers has invalid history length={length}, capacity={capacity}"
        )
    if flags & 0x40:
        raise RiversParseError(f"Rivers flags {flags:#04x} retain writer-cleared bit 0x40")

    presence_offset = reader.pos
    presence = tuple(reader.u8(f"Rivers.presence[{index}]") for index in range(length))
    if any(value not in (0, 1) for value in presence):
        raise RiversParseError("Rivers pointer-presence plane is not boolean")

    repeated_capacity_offset = reader.pos
    repeated_capacity = reader.i32("Rivers.repeated_capacity")
    repeated_increment_offset = reader.pos
    repeated_increment = reader.i16("Rivers.repeated_increment")
    if (repeated_capacity, repeated_increment) != (capacity, increment):
        raise RiversParseError(
            "Rivers duplicated capacity/increment image disagrees: "
            f"({capacity}, {increment}) != ({repeated_capacity}, {repeated_increment})"
        )

    slots = tuple(
        _parse_slot(reader, index, presence_offset + index)
        if present else _empty_slot(index, presence_offset + index)
        for index, present in enumerate(presence)
    )
    raw = bytes(reader.data[offset:reader.pos])
    return RiversSection(
        offset, reader.pos, length, capacity, increment, flags,
        presence_offset, presence, repeated_capacity_offset, repeated_capacity,
        repeated_increment_offset, repeated_increment, slots,
        hashlib.sha256(raw).hexdigest(), layout.sha256, NEXT_OWNER,
    )


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument("--offset", required=True, type=lambda text: int(text, 0))
    parser.add_argument("--schema", type=pathlib.Path, default=DEFAULT_SCHEMA_PATH)
    args = parser.parse_args(argv)
    section = parse_rivers_section(_load(args.file), args.offset, schema_path=args.schema)
    print(
        f"{args.file}: Rivers {section.offset:#x}..{section.end:#x} "
        f"({section.size} bytes) sha256={section.sha256}\n"
        f"  length={section.length}, present={sum(section.presence)}, "
        f"next={section.next_owner} (logical boundary; installed offset unknown)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
