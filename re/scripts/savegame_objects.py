#!/usr/bin/env python3
"""Parse the exact retail ``Objects::walk_data`` generic-save image.

The helper begins at the Objects walk-test byte, preserves all directly walked
marker planes, follows the nine ``MultiPtrArray<Object>`` histories, the
``PtrArray<Ammo>`` tree (including optional Spline data), and the contiguous
``ObjectArray<DeathObj>``.  It stops at the caller's following HotKeyGroup tag.

Concrete Object bodies are virtual children, not an Objects-owned POD image.
The fresh reference SVX has no such children.  For non-empty object arrays a
caller must supply an exact decoder for each serialized factory type; absent
decoders fail closed rather than searching for the next array boundary.
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
from collections.abc import Callable, Mapping, Sequence


TAG_OBJECTS = 0x00
TAG_AMMO = 0x00
TAG_SPLINE = 0x00
TAG_DEATH_OBJECT = 0x00
OBJECTS_TAG_STRING_TABLE_INDEX = 5066
AMMO_TAG_STRING_TABLE_INDEX = 126
SPLINE_TAG_STRING_TABLE_INDEX = 6209
DEATH_TAG_STRING_TABLE_INDEX = 2610
NEXT_TAG_STRING_TABLE_INDEX = 3963
OWNER_COUNT = 9
MAX_ARRAY_LENGTH = 1 << 20
DEFAULT_SCHEMA_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"


class ObjectsParseError(ValueError):
    """The stream or PDB layout contradicts the recovered Objects walk."""


@dataclasses.dataclass(frozen=True)
class ObjectBody:
    owner: int
    slot: int
    type_code: int
    offset: int
    end: int
    sha256: str


@dataclasses.dataclass(frozen=True)
class ObjectOwner:
    owner: int
    offset: int
    end: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    presence_offset: int | None
    presence: tuple[int, ...]
    type_codes_offset: int | None
    type_codes: tuple[int, ...]
    repeated_capacity_offset: int | None
    repeated_capacity: int | None
    repeated_increment_offset: int | None
    repeated_increment: int | None
    bodies: tuple[ObjectBody, ...]
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class PodArray:
    name: str
    offset: int
    end: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    data_offset: int | None
    element_size: int
    sha256: str


@dataclasses.dataclass(frozen=True)
class SplineImage:
    offset: int
    end: int
    tag: int
    core_offset: int
    core: bytes
    arrays: tuple[PodArray, ...]
    sha256: str


@dataclasses.dataclass(frozen=True)
class AmmoBody:
    slot: int
    offset: int
    end: int
    tag: int
    flags: int
    active_data_offset: int | None
    active_data: bytes
    path_presence_offset: int
    path_present: int
    path: SplineImage | None
    sha256: str


@dataclasses.dataclass(frozen=True)
class AmmoArray:
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
    bodies: tuple[AmmoBody, ...]
    sha256: str


@dataclasses.dataclass(frozen=True)
class DeathBody:
    slot: int
    offset: int
    end: int
    tag: int
    valid: int
    active_data_offset: int | None
    active_data: bytes
    sha256: str


@dataclasses.dataclass(frozen=True)
class DeathArray:
    offset: int
    end: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    bodies: tuple[DeathBody, ...]
    sha256: str


@dataclasses.dataclass(frozen=True)
class ObjectsSection:
    offset: int
    end: int
    tag: int
    valid: int
    ammo_index: int
    good_mark: int
    rare_mark: int
    unit_mark: tuple[int, ...]
    build_mark: tuple[int, ...]
    wall_mark: tuple[int, ...]
    obj_ctr: tuple[int, ...]
    owners: tuple[ObjectOwner, ...]
    ammo: AmmoArray
    deaths: DeathArray
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class _Layout:
    sha256: str


_ARRAY28 = (
    ("length", 4, 4, "int"),
    ("size", 8, 4, "int"),
    ("increment", 12, 2, "short"),
    ("list", 16, 4, None),
    ("flags", 20, 1, "unsigned char"),
    ("cur_index", 24, 4, "int"),
)
_ARRAY24 = _ARRAY28[:-1]

_OBJECTS_DATA = (
    ("lists", 4, 280, "ObjectsArray[10]"),
    ("ammo_objs", 284, 28, "PtrArray<Ammo>"),
    ("const_ammo_objs", 312, 4, "ConstPtrArray<Ammo const >&"),
    ("death_objs", 316, 24, "ObjectArray<DeathObj>"),
    ("good_mark", 340, 4, "int"),
    ("rare_mark", 344, 4, "int"),
    ("unit_mark", 348, 40, "int[10]"),
    ("build_mark", 388, 40, "int[10]"),
    ("wall_mark", 428, 40, "int[10]"),
    ("obj_ctr", 468, 20, "unsigned short[10]"),
    ("obj_mark", 488, 12, "int*[3]"),
    ("valid", 500, 4, "int"),
    ("ammo_index", 504, 4, "int"),
    ("find_dist", 508, 4, "int"),
    ("find_who", 512, 4, "int"),
    ("find_list", 516, 28, "PtrArray<Object>"),
)
_OBJECTS_OUT = _OBJECTS_DATA + (
    ("building_orders", 548, 36, "Tree<RenderObject,int>"),
    ("farm_orders", 584, 36, "Tree<RenderObject,int>"),
    ("guys_to_kill_soon", 620, 28, "PtrArray<Guy>"),
    ("guy_orders", 648, 28, "Tree<Guy const *,float>"),
    ("unit_who_orders", 676, 28, "SimpleArray<int>"),
    ("unit_o_orders", 704, 28, "SimpleArray<int>"),
    ("item_orders", 732, 28, "Tree<Item const *,int>"),
    ("good_orders", 760, 28, "Tree<Good const *,int>"),
    ("death_orders", 788, 28, "Tree<DeathObj const *,int>"),
    ("last_render_type", 816, 4, "enum RenderOpacityType"),
)
_AMMO_DATA = (
    ("flags", 4, 1, "unsigned char"),
    ("rolling", 5, 1, "char"),
    ("accuracy", 6, 2, "short"),
    ("gpiece", 8, 4, "int"),
    ("sx", 12, 4, "int"),
    ("sy", 16, 4, "int"),
    ("sz", 20, 4, "int"),
    ("ex", 24, 4, "int"),
    ("ey", 28, 4, "int"),
    ("ez", 32, 4, "int"),
    ("cur_time", 36, 4, "unsigned long"),
    ("total_time", 40, 4, "unsigned long"),
    ("angle", 44, 4, "int"),
    ("splash_area", 48, 4, "int"),
    ("index", 52, 4, "int"),
    ("graph_index", 56, 4, "int"),
    ("who", 60, 4, "int"),
    ("o", 64, 4, "int"),
    ("num_guys", 68, 4, "int"),
    ("whom", 72, 4, "int"),
    ("ox", 76, 4, "int"),
    ("traj", 80, 4, "enum TrajectoryType"),
    ("v1z", 84, 4, "float"),
    ("dx", 88, 4, "float"),
    ("bank_dx", 92, 4, "float"),
    ("bank_dy", 96, 4, "float"),
    ("start_roll_angle", 100, 4, "int"),
    ("ammo_path", 104, 4, "Spline*"),
)
_DEATH_DATA = (
    ("valid", 0, 4, "int"),
    ("first_frame", 4, 4, "int"),
    ("cur_anim", 8, 4, "int"),
    ("x", 12, 4, "Coord"),
    ("y", 16, 4, "Coord"),
    ("z", 20, 4, "Coord"),
    ("who", 24, 4, "int"),
    ("o", 28, 4, "int"),
    ("gpiece", 32, 4, "int"),
    ("ammo_gpiece", 36, 4, "int"),
    ("ammo_angle", 40, 4, "float"),
    ("new_angle", 44, 4, "int"),
    ("turret_angles", 48, 16, "int[4]"),
    ("cur_frame", 64, 4, "int"),
    ("skel_gpiece", 68, 4, "int"),
    ("node_flags", 72, 2, "short"),
    ("unit_crew", 74, 1, "char"),
)
_SPLINE_DATA = (
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


def _fields(record: dict[str, object]) -> tuple[tuple[object, ...], ...]:
    return tuple(
        (field["name"], field["offset"], field["size"], field["type"])
        for field in record["flattened"]  # type: ignore[index]
    )


def _array_expected(list_type: str, *, current: bool = True) -> tuple[tuple[object, ...], ...]:
    source = _ARRAY28 if current else _ARRAY24
    return tuple(
        (name, offset, size, list_type if name == "list" else type_name)
        for name, offset, size, type_name in source
    )


@functools.lru_cache(maxsize=None)
def _load_layout_cached(path_text: str) -> _Layout:
    path = pathlib.Path(path_text)
    try:
        classes = json.loads(path.read_text())["classes"]
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise ObjectsParseError(f"cannot load Objects PDB layout from {path}: {error}") from error
    expected = {
        "Objects": (824, _OBJECTS_OUT),
        "ObjectsData": (544, _OBJECTS_DATA),
        "ObjectsOut": (820, _OBJECTS_OUT),
        "ObjectsArray": (28, _array_expected("Object**")),
        "MultiPtrArray<Object>": (28, _array_expected("Object**")),
        "PtrArray<Ammo>": (28, _array_expected("Ammo**")),
        "ObjectArray<DeathObj>": (24, _array_expected("DeathObj*", current=False)),
        "AmmoData": (108, _AMMO_DATA),
        "DeathObjData": (76, _DEATH_DATA),
        "SplineData": (268, _SPLINE_DATA),
        "Vert3Array": (28, _array_expected("Vector<float>*")),
        "SimpleArray<float>": (28, _array_expected("float*")),
    }
    receipt: dict[str, object] = {}
    for name, (size, fields) in expected.items():
        try:
            record = classes[name]
        except KeyError as error:
            raise ObjectsParseError(f"PDB layout lacks {name}") from error
        actual = _fields(record)
        if record.get("size") != size or actual != fields:
            raise ObjectsParseError(
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
            raise ObjectsParseError(
                f"offset {offset:#x} is outside {len(self.data):#x}-byte stream"
            )
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise ObjectsParseError(
                f"{what} range [{self.pos:#x},{end:#x}) exceeds {len(self.data):#x}-byte stream"
            )
        start = self.pos
        self.pos = end
        return self.data[start:end]

    def u8(self, what: str) -> int:
        return self.take(1, what)[0]

    def i16(self, what: str) -> int:
        return struct.unpack("<h", self.take(2, what))[0]

    def u16(self, what: str) -> int:
        return struct.unpack("<H", self.take(2, what))[0]

    def i32(self, what: str) -> int:
        return struct.unpack("<i", self.take(4, what))[0]


def _count(reader: _Reader, what: str) -> int:
    value = reader.i32(what)
    if value < 0 or value > MAX_ARRAY_LENGTH:
        raise ObjectsParseError(f"invalid {what} {value}")
    return value


def _flags(reader: _Reader, what: str) -> int:
    value = reader.u8(what)
    if value & 0x40:
        raise ObjectsParseError(f"{what} retains writer-cleared 0x40 bit: {value:#04x}")
    return value


def _tag(reader: _Reader, expected: int, what: str, require: bool) -> int:
    offset = reader.pos
    value = reader.u8(what)
    if require and value != expected:
        raise ObjectsParseError(
            f"{what} {value:#04x} != {expected:#04x} at {offset:#x}"
        )
    return value


ObjectBodyDecoder = Callable[[memoryview, int, int, int, int], int]


def _object_owner(
    reader: _Reader,
    owner: int,
    decoders: Mapping[int, ObjectBodyDecoder],
) -> ObjectOwner:
    offset = reader.pos
    length = _count(reader, f"Objects.lists[{owner}] length")
    if length == 0:
        return ObjectOwner(
            owner, offset, reader.pos, 0, None, None, None, None, (), None, (),
            None, None, None, None, (),
            hashlib.sha256(reader.data[offset:reader.pos]).hexdigest(),
        )
    capacity = _count(reader, f"Objects.lists[{owner}] capacity")
    if capacity < length:
        raise ObjectsParseError(
            f"Objects.lists[{owner}] capacity {capacity} is below length {length}"
        )
    increment = reader.i16(f"Objects.lists[{owner}] increment")
    flags = _flags(reader, f"Objects.lists[{owner}] flags")
    presence_offset = reader.pos
    presence = tuple(reader.u8(f"Objects.lists[{owner}] presence[{slot}]") for slot in range(length))
    if any(value not in (0, 1) for value in presence):
        raise ObjectsParseError(f"Objects.lists[{owner}] presence plane is not boolean")
    slots = tuple(index for index, value in enumerate(presence) if value)
    type_codes_offset = reader.pos
    type_codes = tuple(
        reader.i32(f"Objects.lists[{owner}] type[{slot}]") for slot in slots
    )
    repeated_capacity_offset = reader.pos
    repeated_capacity = reader.i32(f"Objects.lists[{owner}] repeated capacity")
    repeated_increment_offset = reader.pos
    repeated_increment = reader.i16(f"Objects.lists[{owner}] repeated increment")
    if (repeated_capacity, repeated_increment) != (capacity, increment):
        raise ObjectsParseError(f"Objects.lists[{owner}] repeated history disagrees")
    bodies = []
    for slot, type_code in zip(slots, type_codes, strict=True):
        body_offset = reader.pos
        decoder = decoders.get(type_code)
        if decoder is None:
            raise ObjectsParseError(
                f"Objects.lists[{owner}] slot {slot} type {type_code} requires an exact virtual-body decoder"
            )
        try:
            body_end = decoder(reader.data, body_offset, owner, slot, type_code)
        except ObjectsParseError:
            raise
        except Exception as error:
            raise ObjectsParseError(
                f"Objects.lists[{owner}] slot {slot} decoder failed: {error}"
            ) from error
        if not isinstance(body_end, int) or not (body_offset < body_end <= len(reader.data)):
            raise ObjectsParseError(
                f"Objects.lists[{owner}] slot {slot} decoder returned invalid end {body_end!r}"
            )
        reader.pos = body_end
        bodies.append(
            ObjectBody(
                owner, slot, type_code, body_offset, body_end,
                hashlib.sha256(reader.data[body_offset:body_end]).hexdigest(),
            )
        )
    return ObjectOwner(
        owner, offset, reader.pos, length, capacity, increment, flags,
        presence_offset, presence, type_codes_offset, type_codes,
        repeated_capacity_offset, repeated_capacity,
        repeated_increment_offset, repeated_increment, tuple(bodies),
        hashlib.sha256(reader.data[offset:reader.pos]).hexdigest(),
    )


def _pod_array(reader: _Reader, name: str, element_size: int) -> PodArray:
    offset = reader.pos
    length = _count(reader, f"{name} length")
    if length == 0:
        return PodArray(
            name, offset, reader.pos, length, None, None, None, None,
            element_size, hashlib.sha256(reader.data[offset:reader.pos]).hexdigest(),
        )
    capacity = _count(reader, f"{name} capacity")
    if capacity < length:
        raise ObjectsParseError(f"{name} capacity {capacity} is below length {length}")
    increment = reader.i16(f"{name} increment")
    flags = _flags(reader, f"{name} flags")
    data_offset = reader.pos
    reader.take(length * element_size, f"{name} data")
    return PodArray(
        name, offset, reader.pos, length, capacity, increment, flags,
        data_offset, element_size,
        hashlib.sha256(reader.data[offset:reader.pos]).hexdigest(),
    )


def _spline(reader: _Reader, require_tags: bool) -> SplineImage:
    offset = reader.pos
    tag = _tag(reader, TAG_SPLINE, "Spline tag", require_tags)
    core_offset = reader.pos
    core = bytes(reader.take(36, "Spline +64..+100 core"))
    arrays = (
        _pod_array(reader, "Spline.control_verts", 12),
        _pod_array(reader, "Spline.knots", 4),
        _pod_array(reader, "Spline.spline_knots", 4),
        _pod_array(reader, "Spline.weights", 4),
        _pod_array(reader, "Spline.spline_verts", 12),
        _pod_array(reader, "Spline.spline_normals", 12),
    )
    return SplineImage(
        offset, reader.pos, tag, core_offset, core, arrays,
        hashlib.sha256(reader.data[offset:reader.pos]).hexdigest(),
    )


def _ammo_body(reader: _Reader, slot: int, require_tags: bool) -> AmmoBody:
    offset = reader.pos
    tag = _tag(reader, TAG_AMMO, f"Ammo[{slot}] tag", require_tags)
    flags = reader.u8(f"Ammo[{slot}].flags")
    active_data_offset = reader.pos if flags & 3 else None
    active_data = bytes(reader.take(99, f"Ammo[{slot}] +5..+104")) if flags & 3 else b""
    path_presence_offset = reader.pos
    path_present = reader.u8(f"Ammo[{slot}].ammo_path presence")
    if path_present not in (0, 1):
        raise ObjectsParseError(f"Ammo[{slot}].ammo_path presence is not boolean")
    path = _spline(reader, require_tags) if path_present else None
    return AmmoBody(
        slot, offset, reader.pos, tag, flags, active_data_offset, active_data,
        path_presence_offset, path_present, path,
        hashlib.sha256(reader.data[offset:reader.pos]).hexdigest(),
    )


def _ammo_array(reader: _Reader, require_tags: bool) -> AmmoArray:
    offset = reader.pos
    length = _count(reader, "Objects.ammo_objs length")
    if length == 0:
        return AmmoArray(
            offset, reader.pos, 0, None, None, None, None, (), None, None,
            None, None, (), hashlib.sha256(reader.data[offset:reader.pos]).hexdigest(),
        )
    capacity = _count(reader, "Objects.ammo_objs capacity")
    if capacity < length:
        raise ObjectsParseError(
            f"Objects.ammo_objs capacity {capacity} is below length {length}"
        )
    increment = reader.i16("Objects.ammo_objs increment")
    flags = _flags(reader, "Objects.ammo_objs flags")
    presence_offset = reader.pos
    presence = tuple(reader.u8(f"Objects.ammo_objs presence[{i}]") for i in range(length))
    if any(value not in (0, 1) for value in presence):
        raise ObjectsParseError("Objects.ammo_objs presence plane is not boolean")
    repeated_capacity_offset = reader.pos
    repeated_capacity = reader.i32("Objects.ammo_objs repeated capacity")
    repeated_increment_offset = reader.pos
    repeated_increment = reader.i16("Objects.ammo_objs repeated increment")
    if (repeated_capacity, repeated_increment) != (capacity, increment):
        raise ObjectsParseError("Objects.ammo_objs repeated history disagrees")
    bodies = tuple(
        _ammo_body(reader, slot, require_tags)
        for slot, present in enumerate(presence)
        if present
    )
    return AmmoArray(
        offset, reader.pos, length, capacity, increment, flags,
        presence_offset, presence, repeated_capacity_offset, repeated_capacity,
        repeated_increment_offset, repeated_increment, bodies,
        hashlib.sha256(reader.data[offset:reader.pos]).hexdigest(),
    )


def _death_array(reader: _Reader, require_tags: bool) -> DeathArray:
    offset = reader.pos
    length = _count(reader, "Objects.death_objs length")
    if length == 0:
        return DeathArray(
            offset, reader.pos, 0, None, None, None, (),
            hashlib.sha256(reader.data[offset:reader.pos]).hexdigest(),
        )
    capacity = _count(reader, "Objects.death_objs capacity")
    if capacity < length:
        raise ObjectsParseError(
            f"Objects.death_objs capacity {capacity} is below length {length}"
        )
    increment = reader.i16("Objects.death_objs increment")
    flags = _flags(reader, "Objects.death_objs flags")
    bodies = []
    for slot in range(length):
        body_offset = reader.pos
        tag = _tag(reader, TAG_DEATH_OBJECT, f"DeathObj[{slot}] tag", require_tags)
        valid = reader.i32(f"DeathObj[{slot}].valid")
        active_data_offset = reader.pos if valid else None
        active_data = bytes(reader.take(71, f"DeathObj[{slot}] +4..+75")) if valid else b""
        bodies.append(
            DeathBody(
                slot, body_offset, reader.pos, tag, valid,
                active_data_offset, active_data,
                hashlib.sha256(reader.data[body_offset:reader.pos]).hexdigest(),
            )
        )
    return DeathArray(
        offset, reader.pos, length, capacity, increment, flags, tuple(bodies),
        hashlib.sha256(reader.data[offset:reader.pos]).hexdigest(),
    )


def parse_objects_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    object_body_decoders: Mapping[int, ObjectBodyDecoder] | None = None,
    require_tag: bool = True,
    require_child_tags: bool = True,
    schema_path: pathlib.Path | None = None,
) -> ObjectsSection:
    """Parse ``Objects::walk_data`` and return the next caller-owned byte.

    ``object_body_decoders`` is required only for present polymorphic Object
    slots.  Each decoder receives ``(data, offset, owner, slot, type_code)``
    and must return the exact exclusive end of that virtual child walk.
    """
    layout = _load_layout(schema_path)
    reader = _Reader(data, offset)
    tag = _tag(reader, TAG_OBJECTS, "Objects tag", require_tag)
    valid = reader.i32("Objects.valid")
    ammo_index = reader.i32("Objects.ammo_index")
    good_mark = reader.i32("Objects.good_mark")
    rare_mark = reader.i32("Objects.rare_mark")
    unit_mark = tuple(reader.i32(f"Objects.unit_mark[{i}]") for i in range(9))
    build_mark = tuple(reader.i32(f"Objects.build_mark[{i}]") for i in range(9))
    wall_mark = tuple(reader.i32(f"Objects.wall_mark[{i}]") for i in range(9))
    obj_ctr = tuple(reader.u16(f"Objects.obj_ctr[{i}]") for i in range(9))
    decoders = object_body_decoders or {}
    owners = tuple(_object_owner(reader, owner, decoders) for owner in range(OWNER_COUNT))
    ammo = _ammo_array(reader, require_child_tags)
    deaths = _death_array(reader, require_child_tags)
    return ObjectsSection(
        offset, reader.pos, tag, valid, ammo_index, good_mark, rare_mark,
        unit_mark, build_mark, wall_mark, obj_ctr, owners, ammo, deaths,
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
            else:
                result[field.name] = _jsonable(item)
        if hasattr(value, "size"):
            result["size"] = getattr(value, "size")
        return result
    if isinstance(value, tuple):
        return [_jsonable(item) for item in value]
    if isinstance(value, bytes):
        return value.hex()
    return value


def _summary(section: ObjectsSection, path: pathlib.Path) -> str:
    live_objects = sum(len(owner.bodies) for owner in section.owners)
    return "\n".join(
        (
            f"{path}: Objects {section.offset:#x}..{section.end:#x} "
            f"({section.size} bytes) sha256={section.sha256}",
            f"  PDB-layout sha256={section.layout_sha256}",
            f"  nine object owners, {live_objects} present virtual bodies; "
            f"ammo={len(section.ammo.bodies)}, deaths={len(section.deaths.bodies)}",
            f"  next owner begins at {section.end:#x} "
            f"(StringTable[{NEXT_TAG_STRING_TABLE_INDEX}] / Array<HotKeyGroup>)",
        )
    )


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument("--offset", required=True, type=lambda text: int(text, 0))
    parser.add_argument("--schema", type=pathlib.Path, default=DEFAULT_SCHEMA_PATH)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    section = parse_objects_section(_load(args.file), args.offset, schema_path=args.schema)
    print(json.dumps(_jsonable(section), indent=2) if args.json else _summary(section, args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
