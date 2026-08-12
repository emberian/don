#!/usr/bin/env python3
"""Census every structurally reachable Unit OrderList in a retail SVX.

The caller supplies the exact ``Objects::walk_data`` boundary.  This parser
walks the nine shipped ``Objects::lists`` owner arrays in program order.  It
consumes every Unit and intervening Build body needed to reach the next owner,
including PathData stacks, OrderLists, PtrArray<Guy> planes, GuyData images,
BuildQueue, MiningList, and GatherPointList branches.

The fresh-save terminal owner contains only concrete type 3 (Good) bodies.
Their type plane proves that no Unit remains, so this deliberately bounded
Unit census stops at the first Good body rather than guessing its grammar.
No byte or tag search is performed anywhere.
"""

from __future__ import annotations

import argparse
import dataclasses
import gzip
import hashlib
import json
import pathlib
import struct
from collections import Counter
from typing import Sequence


OBJECTS_TAG = 0x7C
SUBOBJECT_TAG = 0x5E
OBJECT_TAG = 0x2E
UNIT_TAG = 0x12
WALL_TAG = 0x7E
BUILD_TAG = 0xC5
BUILD_QUEUE_TAG = 0x32

OBJECTS_FIXED_BYTES = 142
OBJECT_OWNER_ARRAYS = 9
UNIT_OBJECT_TYPE = 0
BUILD_OBJECT_TYPE = 1
GOOD_OBJECT_TYPE = 3
UNIT_FIXED_BYTES = 111
WALL_FIXED_BYTES = 30
BUILD_FIXED_BYTES = 22
PATH_RECORD_BYTES = 16
GUY_DATA_BYTES = 155
QUEUE_ITEM_WALK_BYTES = 18
TCOORD_DATA_BYTES = 8
GATHER_POINT_WALK_BYTES = 9

ORDER_NAMES = {
    1: "MOVE_TO",
    2: "ATTACK_TO",
    3: "EXPLORE_TO",
    4: "FLEE_TO",
    6: "BUILD_AT",
    7: "GATHER",
    14: "CAST_SPELL",
}

MOVE_FIELD_NAMES = (
    "x",
    "y",
    "angle",
    "dest",
    "tolerance",
    "pause",
    "retry",
    "attempts",
    "timer",
    "facing",
    "dest_x",
    "dest_y",
    "last_x",
    "last_y",
    "coll_x",
    "coll_y",
    "orig_x",
    "orig_y",
    "off_x",
    "off_y",
)
TARGET_FIELD_NAMES = ("target_o", "target_who", "target_uid")
GATHER_FIELD_NAMES = TARGET_FIELD_NAMES + (
    "tx",
    "ty",
    "build_type",
    "wait",
    "goto_build",
    "non_flat_gather",
    "dist_mod",
    "been_there",
)
CAST_FIELD_NAMES = TARGET_FIELD_NAMES + ("x", "y", "paid", "spell")


class UnitOrderListCensusError(ValueError):
    """The supplied range is not the bounded retail Objects/Unit walk."""


@dataclasses.dataclass(frozen=True)
class FieldValue:
    name: str
    value: int


@dataclasses.dataclass(frozen=True)
class GuyImage:
    slot: int
    offset: int
    end: int
    sha256: str


@dataclasses.dataclass(frozen=True)
class GuyArrayImage:
    offset: int
    end: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    present_slots: tuple[int, ...]
    guys: tuple[GuyImage, ...]
    sha256: str


@dataclasses.dataclass(frozen=True)
class PathImage:
    offset: int
    end: int
    capacity: int
    length: int
    increment: int
    sha256: str


@dataclasses.dataclass(frozen=True)
class RetailOrderNode:
    owner: int
    unit_slot: int
    unit_who: int
    unit_o: int
    unit_ptype: int
    node_index: int
    offset: int
    end: int
    order_type: int
    order_name: str
    metric_offset: int
    metric: int
    payload_offset: int
    payload_end: int
    order_flags_offset: int
    order_flags: int
    payload_family: str
    payload_fields: tuple[FieldValue, ...]
    payload_hex: str
    payload_sha256: str
    node_sha256: str
    don_save_v13_payload_tag: int
    canonical_authority: str
    canonical_lossless_for_retail_fields: bool
    canonical_note: str

    @property
    def size(self) -> int:
        return self.end - self.offset

    def field(self, name: str) -> int:
        for field in self.payload_fields:
            if field.name == name:
                return field.value
        raise KeyError(name)


@dataclasses.dataclass(frozen=True)
class UnitImage:
    owner: int
    slot: int
    offset: int
    end: int
    subobject_flags: int
    active: bool
    who: int | None
    o: int | None
    ptype_index: int | None
    path: PathImage | None
    orderlist_offset: int | None
    orderlist_end: int | None
    orderlist_sha256: str | None
    order_indices: tuple[int, ...]
    guys: GuyArrayImage | None
    sha256: str


@dataclasses.dataclass(frozen=True)
class BuildImage:
    owner: int
    slot: int
    offset: int
    end: int
    active: bool
    sha256: str


@dataclasses.dataclass(frozen=True)
class OwnerArrayImage:
    owner: int
    offset: int
    body_offset: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    present_slots: tuple[int, ...]
    type_codes: tuple[int, ...]
    body_end: int | None


@dataclasses.dataclass(frozen=True)
class UnitOrderListCensus:
    objects_offset: int
    owners: tuple[OwnerArrayImage, ...]
    units: tuple[UnitImage, ...]
    builds: tuple[BuildImage, ...]
    orders: tuple[RetailOrderNode, ...]
    terminal_body_offset: int
    terminal_unparsed_types: tuple[int, ...]
    order_manifest_sha256: str
    unit_manifest_sha256: str
    guy_manifest_sha256: str

    @property
    def active_units(self) -> int:
        return sum(unit.active for unit in self.units)

    @property
    def inactive_units(self) -> int:
        return len(self.units) - self.active_units

    @property
    def guys(self) -> tuple[GuyImage, ...]:
        return tuple(
            guy
            for unit in self.units
            if unit.guys is not None
            for guy in unit.guys.guys
        )

    @property
    def order_type_counts(self) -> dict[str, int]:
        counts = Counter(order.order_name for order in self.orders)
        return dict(sorted(counts.items()))


class _Reader:
    def __init__(self, data: bytes | bytearray | memoryview, offset: int):
        self.data = memoryview(data)
        if offset < 0 or offset > len(self.data):
            raise UnitOrderListCensusError(
                f"offset {offset:#x} is outside {len(self.data):#x}-byte stream"
            )
        self.p = offset

    def raw(self, size: int) -> memoryview:
        if size < 0 or self.p + size > len(self.data):
            raise UnitOrderListCensusError(
                f"range [{self.p:#x},{self.p + size:#x}) exceeds "
                f"{len(self.data):#x}-byte stream"
            )
        value = self.data[self.p : self.p + size]
        self.p += size
        return value

    def u8(self) -> int:
        return self.raw(1)[0]

    def i8(self) -> int:
        return struct.unpack("<b", self.raw(1))[0]

    def u16(self) -> int:
        return struct.unpack("<H", self.raw(2))[0]

    def i16(self) -> int:
        return struct.unpack("<h", self.raw(2))[0]

    def i32(self) -> int:
        return struct.unpack("<i", self.raw(4))[0]

    def expect_u8(self, expected: int, owner: str) -> int:
        offset = self.p
        value = self.u8()
        if value != expected:
            raise UnitOrderListCensusError(
                f"{owner} tag {value:#04x} != {expected:#04x} at {offset:#x}"
            )
        return value


def _sha(reader: _Reader, begin: int, end: int) -> str:
    return hashlib.sha256(bytes(reader.data[begin:end])).hexdigest()


def _bounded_count(value: int, owner: str, limit: int = 1_000_000) -> int:
    if value < 0 or value > limit:
        raise UnitOrderListCensusError(f"invalid {owner} count {value}")
    return value


def _array_header(
    reader: _Reader,
    owner: str,
) -> tuple[int, int | None, int | None, int | None]:
    length = _bounded_count(reader.i32(), f"{owner} length")
    if length == 0:
        return 0, None, None, None
    capacity = _bounded_count(reader.i32(), f"{owner} capacity")
    increment = reader.i16()
    flags = reader.u8()
    if capacity < length:
        raise UnitOrderListCensusError(
            f"{owner} capacity {capacity} is below length {length}"
        )
    if flags & 0x40:
        raise UnitOrderListCensusError(
            f"{owner} save flags retain forbidden 0x40 bit: {flags:#04x}"
        )
    return length, capacity, increment, flags


def _simple_array_i32(reader: _Reader, owner: str) -> None:
    length, _, _, _ = _array_header(reader, owner)
    reader.raw(length * 4)


def _subobject(
    reader: _Reader,
    owner: str,
) -> tuple[int, int, int | None, int | None, int | None]:
    reader.expect_u8(SUBOBJECT_TAG, f"{owner} SubObject")
    flags = reader.u8()
    must_walk = reader.u8()
    if must_walk not in (0, 1):
        raise UnitOrderListCensusError(
            f"{owner} SubObject::must_walk is not boolean: {must_walk}"
        )
    if not must_walk:
        return flags, must_walk, None, None, None
    who = reader.u8()
    o = reader.i16()
    reader.raw(12)  # z_internal, x_internal, y_internal
    ptype_index = reader.i32()
    return flags, must_walk, who, o, ptype_index


def _object_base(
    reader: _Reader,
    owner: str,
) -> tuple[int, int, int | None, int | None, int | None]:
    flags, sub_gate, who, o, ptype_index = _subobject(reader, owner)
    reader.expect_u8(OBJECT_TAG, f"{owner} Object")
    object_gate = reader.u8()
    if object_gate not in (0, 1):
        raise UnitOrderListCensusError(
            f"{owner} Object::must_walk is not boolean: {object_gate}"
        )
    if object_gate:
        reader.raw(34)
    launching_present = reader.u8()
    if launching_present not in (0, 1):
        raise UnitOrderListCensusError(
            f"{owner} Object::launching presence is not boolean"
        )
    if launching_present:
        _simple_array_i32(reader, f"{owner} Object::launching")
    if sub_gate != object_gate:
        raise UnitOrderListCensusError(
            f"{owner} inherited gates disagree: {sub_gate}/{object_gate}"
        )
    return flags, object_gate, who, o, ptype_index


def _path(reader: _Reader, owner: str) -> PathImage:
    offset = reader.p
    capacity = _bounded_count(reader.i32(), f"{owner} PathData capacity")
    length = _bounded_count(reader.i32(), f"{owner} PathData length")
    increment = reader.i8()
    if length > capacity:
        raise UnitOrderListCensusError(
            f"{owner} PathData length {length} exceeds capacity {capacity}"
        )
    reader.raw(length * PATH_RECORD_BYTES)
    return PathImage(
        offset=offset,
        end=reader.p,
        capacity=capacity,
        length=length,
        increment=increment,
        sha256=_sha(reader, offset, reader.p),
    )


def _field_values(names: tuple[str, ...], values: tuple[int, ...]) -> tuple[FieldValue, ...]:
    return tuple(FieldValue(name, value) for name, value in zip(names, values, strict=True))


def _target_is_canonical(fields: tuple[FieldValue, ...], required: bool) -> tuple[bool, str]:
    values = {field.name: field.value for field in fields}
    o = values["target_o"]
    who = values["target_who"]
    uid = values["target_uid"]
    if (o, who, uid) == (-1, -1, 0xFFFF):
        return (not required), "exact no-target sentinel"
    if not (-0x8000 <= o <= 0x7FFF and -0x80 <= who <= 0x7F):
        return False, "retail target exceeds flattened Order target widths"
    if o >= 2000 and 0 <= who < 10:
        return True, "Build/Wall-band identity is save-owned without an additive Handle"
    return False, "Unit-band identity requires registry Handle resolution before v13 encoding"


def _decode_payload(
    order_type: int,
    raw: bytes,
) -> tuple[
    str,
    int,
    tuple[FieldValue, ...],
    int,
    str,
    bool,
    str,
]:
    flags = raw[0]
    if order_type in (1, 2, 3, 4):
        if len(raw) != 77:
            raise UnitOrderListCensusError("MoveOrder payload is not 77 bytes")
        words = struct.unpack_from("<18i2h", raw, 1)
        return (
            "MoveOrder",
            flags,
            _field_values(MOVE_FIELD_NAMES, words),
            1,
            "Order.move_state / DoNSave v13 Move",
            True,
            "all retail MoveOrder fields map losslessly; v13 additionally owns target/handle and group defaults",
        )
    if order_type == 6:
        if len(raw) != 11:
            raise UnitOrderListCensusError("BuildOrder payload is not 11 bytes")
        fields = _field_values(TARGET_FIELD_NAMES, struct.unpack_from("<iiH", raw, 1))
        lossless, note = _target_is_canonical(fields, required=True)
        return (
            "TargetOrder",
            flags,
            fields,
            0,
            "Order generic target / DoNSave v13 None",
            lossless,
            note,
        )
    if order_type == 7:
        if len(raw) != 31:
            raise UnitOrderListCensusError("GatherOrder payload is not 31 bytes")
        values = struct.unpack_from("<iiH4i4B", raw, 1)
        fields = _field_values(GATHER_FIELD_NAMES, values)
        lossless, note = _target_is_canonical(fields, required=True)
        return (
            "GatherOrder",
            flags,
            fields,
            2,
            "EconomyOrderPayload::Gather / DoNSave v13 Gather",
            lossless,
            note,
        )
    if order_type == 14:
        if len(raw) != 27:
            raise UnitOrderListCensusError("CastOrder payload is not 27 bytes")
        values = struct.unpack_from("<iiH4i", raw, 1)
        fields = _field_values(CAST_FIELD_NAMES, values)
        lossless, note = _target_is_canonical(fields, required=False)
        return (
            "CastOrder",
            flags,
            fields,
            3,
            "EconomyOrderPayload::CastSpell / DoNSave v13 CastSpell",
            lossless,
            note,
        )
    raise UnitOrderListCensusError(f"unsupported concrete OrderIndex {order_type}")


def encode_retail_payload(node: RetailOrderNode) -> bytes:
    """Rebuild the exact concrete virtual-walk image from decoded fields."""

    values = tuple(field.value for field in node.payload_fields)
    if node.payload_family == "MoveOrder":
        suffix = struct.pack("<18i2h", *values)
    elif node.payload_family == "TargetOrder":
        suffix = struct.pack("<iiH", *values)
    elif node.payload_family == "GatherOrder":
        suffix = struct.pack("<iiH4i4B", *values)
    elif node.payload_family == "CastOrder":
        suffix = struct.pack("<iiH4i", *values)
    else:
        raise UnitOrderListCensusError(
            f"cannot encode payload family {node.payload_family}"
        )
    return bytes((node.order_flags,)) + suffix


def _orderlist(
    reader: _Reader,
    owner: int,
    unit_slot: int,
    who: int,
    o: int,
    ptype_index: int,
    global_orders: list[RetailOrderNode],
) -> tuple[int, int, str, tuple[int, ...]]:
    offset = reader.p
    count = _bounded_count(reader.i32(), "OrderList", 65_536)
    indices: list[int] = []
    for node_index in range(count):
        node_offset = reader.p
        order_type = reader.i32()
        metric_offset = reader.p
        metric = reader.u8()
        payload_offset = reader.p
        payload_size = {
            1: 77,
            2: 77,
            3: 77,
            4: 77,
            6: 11,
            7: 31,
            14: 27,
        }.get(order_type)
        if payload_size is None:
            raise UnitOrderListCensusError(
                f"unsupported concrete OrderIndex {order_type} at {node_offset:#x}"
            )
        raw = bytes(reader.raw(payload_size))
        (
            family,
            order_flags,
            fields,
            don_save_tag,
            authority,
            lossless,
            note,
        ) = _decode_payload(order_type, raw)
        node = RetailOrderNode(
            owner=owner,
            unit_slot=unit_slot,
            unit_who=who,
            unit_o=o,
            unit_ptype=ptype_index,
            node_index=node_index,
            offset=node_offset,
            end=reader.p,
            order_type=order_type,
            order_name=ORDER_NAMES[order_type],
            metric_offset=metric_offset,
            metric=metric,
            payload_offset=payload_offset,
            payload_end=reader.p,
            order_flags_offset=payload_offset,
            order_flags=order_flags,
            payload_family=family,
            payload_fields=fields,
            payload_hex=raw.hex(),
            payload_sha256=hashlib.sha256(raw).hexdigest(),
            node_sha256=_sha(reader, node_offset, reader.p),
            don_save_v13_payload_tag=don_save_tag,
            canonical_authority=authority,
            canonical_lossless_for_retail_fields=lossless,
            canonical_note=note,
        )
        if encode_retail_payload(node) != raw:
            raise AssertionError("decoded retail order did not reconstruct")
        indices.append(len(global_orders))
        global_orders.append(node)
    return offset, reader.p, _sha(reader, offset, reader.p), tuple(indices)


def _guys(reader: _Reader, owner: str) -> GuyArrayImage:
    offset = reader.p
    length, capacity, increment, flags = _array_header(reader, f"{owner} Guys")
    if length == 0:
        return GuyArrayImage(
            offset=offset,
            end=reader.p,
            length=0,
            capacity=None,
            increment=None,
            flags=None,
            present_slots=(),
            guys=(),
            sha256=_sha(reader, offset, reader.p),
        )
    presence = bytes(reader.raw(length))
    if any(value not in (0, 1) for value in presence):
        raise UnitOrderListCensusError(f"{owner} Guys presence plane is not boolean")
    present_slots = tuple(index for index, value in enumerate(presence) if value)
    second_capacity = reader.i32()
    second_increment = reader.i16()
    if (second_capacity, second_increment) != (capacity, increment):
        raise UnitOrderListCensusError(f"{owner} Guys duplicated history disagrees")
    guys = []
    for slot in present_slots:
        guy_offset = reader.p
        reader.raw(GUY_DATA_BYTES)
        guys.append(
            GuyImage(
                slot=slot,
                offset=guy_offset,
                end=reader.p,
                sha256=_sha(reader, guy_offset, reader.p),
            )
        )
    return GuyArrayImage(
        offset=offset,
        end=reader.p,
        length=length,
        capacity=capacity,
        increment=increment,
        flags=flags,
        present_slots=present_slots,
        guys=tuple(guys),
        sha256=_sha(reader, offset, reader.p),
    )


def _unit(
    reader: _Reader,
    owner: int,
    slot: int,
    global_orders: list[RetailOrderNode],
) -> UnitImage:
    offset = reader.p
    label = f"owner {owner} Unit slot {slot}"
    flags, gate, who, o, ptype_index = _object_base(reader, label)
    reader.expect_u8(UNIT_TAG, f"{label} Unit")
    unit_gate = reader.u8()
    if unit_gate not in (0, 1):
        raise UnitOrderListCensusError(f"{label} Unit::must_walk is not boolean")
    if gate != unit_gate:
        raise UnitOrderListCensusError(f"{label} inherited Unit gate disagrees")
    if not unit_gate:
        return UnitImage(
            owner=owner,
            slot=slot,
            offset=offset,
            end=reader.p,
            subobject_flags=flags,
            active=False,
            who=None,
            o=None,
            ptype_index=None,
            path=None,
            orderlist_offset=None,
            orderlist_end=None,
            orderlist_sha256=None,
            order_indices=(),
            guys=None,
            sha256=_sha(reader, offset, reader.p),
        )
    assert who is not None and o is not None and ptype_index is not None
    reader.raw(UNIT_FIXED_BYTES)
    path = _path(reader, label)
    orderlist_offset, orderlist_end, orderlist_sha, order_indices = _orderlist(
        reader,
        owner,
        slot,
        who,
        o,
        ptype_index,
        global_orders,
    )
    guys = _guys(reader, label)
    return UnitImage(
        owner=owner,
        slot=slot,
        offset=offset,
        end=reader.p,
        subobject_flags=flags,
        active=True,
        who=who,
        o=o,
        ptype_index=ptype_index,
        path=path,
        orderlist_offset=orderlist_offset,
        orderlist_end=orderlist_end,
        orderlist_sha256=orderlist_sha,
        order_indices=order_indices,
        guys=guys,
        sha256=_sha(reader, offset, reader.p),
    )


def _build(reader: _Reader, owner: int, slot: int) -> BuildImage:
    offset = reader.p
    label = f"owner {owner} Build slot {slot}"
    reader.raw(2)  # founder and max_age are emitted before the Wall base
    _, gate, _, _, _ = _object_base(reader, label)
    reader.expect_u8(WALL_TAG, f"{label} Wall")
    wall_gate = reader.u8()
    if wall_gate not in (0, 1):
        raise UnitOrderListCensusError(f"{label} Wall::must_walk is not boolean")
    if wall_gate:
        reader.raw(WALL_FIXED_BYTES)
    reader.expect_u8(BUILD_TAG, f"{label} Build")
    build_gate = reader.u8()
    if build_gate not in (0, 1):
        raise UnitOrderListCensusError(f"{label} Build::must_walk is not boolean")
    if not (gate == wall_gate == build_gate):
        raise UnitOrderListCensusError(f"{label} inherited Build gates disagree")
    if build_gate:
        reader.raw(BUILD_FIXED_BYTES)
        reader.expect_u8(BUILD_QUEUE_TAG, f"{label} BuildQueue")
        queue_size = _bounded_count(reader.i32(), f"{label} BuildQueue", 65_536)
        reader.raw(queue_size * QUEUE_ITEM_WALK_BYTES)
        reader.raw(2)  # MiningList::mtn, cliff
        mining_length, _, _, _ = _array_header(reader, f"{label} MiningList")
        reader.raw(mining_length * TCOORD_DATA_BYTES)
        gather_count = _bounded_count(
            reader.i32(), f"{label} GatherPointList", 65_536
        )
        for _ in range(gather_count):
            reader.i32()  # concrete data discriminator
            reader.u8()  # LLNode metric
            reader.raw(GATHER_POINT_WALK_BYTES)
        reader.i32()  # BuildData::orig_type
    return BuildImage(
        owner=owner,
        slot=slot,
        offset=offset,
        end=reader.p,
        active=bool(build_gate),
        sha256=_sha(reader, offset, reader.p),
    )


def _manifest(rows: object) -> str:
    raw = json.dumps(rows, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(raw).hexdigest()


def parse_unit_orderlist_census(
    data: bytes | bytearray | memoryview,
    objects_offset: int,
) -> UnitOrderListCensus:
    """Walk all Unit bodies reached from the exact Objects boundary."""

    reader = _Reader(data, objects_offset)
    reader.expect_u8(OBJECTS_TAG, "Objects")
    reader.raw(OBJECTS_FIXED_BYTES)
    owners: list[OwnerArrayImage] = []
    units: list[UnitImage] = []
    builds: list[BuildImage] = []
    orders: list[RetailOrderNode] = []
    terminal_types: tuple[int, ...] = ()
    terminal_body_offset = reader.p

    for owner in range(OBJECT_OWNER_ARRAYS):
        array_offset = reader.p
        length, capacity, increment, flags = _array_header(
            reader, f"owner {owner} object array"
        )
        if length == 0:
            owners.append(
                OwnerArrayImage(
                    owner=owner,
                    offset=array_offset,
                    body_offset=reader.p,
                    length=0,
                    capacity=None,
                    increment=None,
                    flags=None,
                    present_slots=(),
                    type_codes=(),
                    body_end=reader.p,
                )
            )
            terminal_body_offset = reader.p
            continue
        presence = bytes(reader.raw(length))
        if any(value not in (0, 1) for value in presence):
            raise UnitOrderListCensusError(
                f"owner {owner} object presence plane is not boolean"
            )
        present_slots = tuple(index for index, value in enumerate(presence) if value)
        type_codes = tuple(reader.i32() for _ in present_slots)
        second_capacity = reader.i32()
        second_increment = reader.i16()
        if (second_capacity, second_increment) != (capacity, increment):
            raise UnitOrderListCensusError(
                f"owner {owner} object duplicated history disagrees"
            )
        body_offset = reader.p
        owner_image = OwnerArrayImage(
            owner=owner,
            offset=array_offset,
            body_offset=body_offset,
            length=length,
            capacity=capacity,
            increment=increment,
            flags=flags,
            present_slots=present_slots,
            type_codes=type_codes,
            body_end=None,
        )

        unsupported = tuple(
            type_code
            for type_code in type_codes
            if type_code not in (UNIT_OBJECT_TYPE, BUILD_OBJECT_TYPE)
        )
        if unsupported:
            if owner != OBJECT_OWNER_ARRAYS - 1:
                raise UnitOrderListCensusError(
                    f"unsupported object types before terminal owner {owner}: "
                    f"{sorted(set(unsupported))}"
                )
            if UNIT_OBJECT_TYPE in type_codes:
                raise UnitOrderListCensusError(
                    "terminal unsupported bodies are interleaved with untraversed Units"
                )
            terminal_types = unsupported
            terminal_body_offset = body_offset
            owners.append(owner_image)
            break

        for slot, type_code in zip(present_slots, type_codes, strict=True):
            if type_code == UNIT_OBJECT_TYPE:
                units.append(_unit(reader, owner, slot, orders))
            else:
                builds.append(_build(reader, owner, slot))
        owners.append(dataclasses.replace(owner_image, body_end=reader.p))
        terminal_body_offset = reader.p
    else:
        if len(owners) != OBJECT_OWNER_ARRAYS:
            raise AssertionError("owner traversal ended early")

    if len(owners) != OBJECT_OWNER_ARRAYS:
        raise UnitOrderListCensusError(
            f"reached only {len(owners)} of {OBJECT_OWNER_ARRAYS} owner arrays"
        )

    order_manifest = [
        (
            order.owner,
            order.unit_slot,
            order.node_index,
            order.offset,
            order.end,
            order.order_type,
            order.metric,
            order.order_flags,
            order.payload_sha256,
            order.node_sha256,
        )
        for order in orders
    ]
    unit_manifest = [
        (
            unit.owner,
            unit.slot,
            unit.offset,
            unit.end,
            unit.active,
            unit.who,
            unit.o,
            unit.ptype_index,
            unit.path.sha256 if unit.path else None,
            unit.orderlist_sha256,
            unit.guys.sha256 if unit.guys else None,
            unit.sha256,
        )
        for unit in units
    ]
    guy_manifest = [
        (unit.owner, unit.slot, guy.slot, guy.offset, guy.end, guy.sha256)
        for unit in units
        if unit.guys is not None
        for guy in unit.guys.guys
    ]
    return UnitOrderListCensus(
        objects_offset=objects_offset,
        owners=tuple(owners),
        units=tuple(units),
        builds=tuple(builds),
        orders=tuple(orders),
        terminal_body_offset=terminal_body_offset,
        terminal_unparsed_types=terminal_types,
        order_manifest_sha256=_manifest(order_manifest),
        unit_manifest_sha256=_manifest(unit_manifest),
        guy_manifest_sha256=_manifest(guy_manifest),
    )


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def _jsonable(census: UnitOrderListCensus) -> dict[str, object]:
    return dataclasses.asdict(census)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument(
        "--objects-offset",
        required=True,
        type=lambda text: int(text, 0),
        help="decompressed offset of the Objects::walk_data tag",
    )
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    census = parse_unit_orderlist_census(_load(args.file), args.objects_offset)
    if args.json:
        print(json.dumps(_jsonable(census), indent=2))
    else:
        print(
            f"{args.file}: {len(census.units)} present Units, "
            f"{census.active_units} active/{census.inactive_units} inactive, "
            f"{len(census.guys)} Guys, {len(census.orders)} order nodes "
            f"{census.order_type_counts}; terminal body "
            f"{census.terminal_body_offset:#x}, order manifest "
            f"sha256={census.order_manifest_sha256}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
