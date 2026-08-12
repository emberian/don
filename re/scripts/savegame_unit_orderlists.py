#!/usr/bin/env python3
"""Localize a structurally reached Unit ``OrderList`` in a retail SVX stream.

This parser is deliberately bounded.  Its caller supplies the first byte owned
by ``Objects::walk_data``; it then follows the shipped
``Objects -> MultiPtrArray<Object> -> Unit`` grammar to the first live Unit in
owner band zero.  It consumes that Unit through its path stack and complete
``OrderList``, and stops at the first byte owned by ``PtrArray<Guy>``.

No byte search is performed.  Unsupported object layouts, launching arrays,
and concrete order classes fail closed instead of being guessed or skipped.
The currently supported concrete payload is ``UnitOrder::flags`` followed by
the 76-byte ``MoveOrder`` field range shared by MOVE_TO, ATTACK_TO,
EXPLORE_TO, and FLEE_TO.
"""

from __future__ import annotations

import argparse
import dataclasses
import gzip
import hashlib
import json
import pathlib
import struct
from typing import Sequence


# WalkDataGame calls Groups immediately before Objects.  The caller owns the
# Groups boundary; these constants are the tags emitted by the exact retail
# walk_test calls reached after that boundary.
OBJECTS_TAG = 0x7C
SUBOBJECT_TAG = 0x5E
OBJECT_TAG = 0x2E
UNIT_TAG = 0x12

OBJECTS_FIXED_BYTES = 142
UNIT_BAND_END = 2000
UNIT_OBJECT_TYPE = 0
PATH_RECORD_BYTES = 16
UNIT_FIXED_BYTES = 111
MOVE_ORDER_BYTES = 76
MOVE_ORDER_TYPES = {
    1: "MOVE_TO",
    2: "ATTACK_TO",
    3: "EXPLORE_TO",
    4: "FLEE_TO",
}


class UnitOrderListParseError(ValueError):
    """The supplied range is not the bounded retail Unit walk."""


@dataclasses.dataclass(frozen=True)
class MoveOrderPayload:
    offset: int
    end: int
    x: int
    y: int
    angle: int
    dest: int
    tolerance: int
    pause: int
    retry: int
    attempts: int
    timer: int
    facing: int
    dest_x: int
    dest_y: int
    last_x: int
    last_y: int
    coll_x: int
    coll_y: int
    orig_x: int
    orig_y: int
    off_x: int
    off_y: int
    sha256: str


@dataclasses.dataclass(frozen=True)
class OrderNode:
    offset: int
    end: int
    order_type: int
    order_name: str
    metric_offset: int
    metric: int
    order_flags_offset: int
    order_flags: int
    payload: MoveOrderPayload


@dataclasses.dataclass(frozen=True)
class UnitOrderListSection:
    objects_offset: int
    owner_array_offset: int
    owner_array_body_offset: int
    unit_slot: int
    unit_body_offset: int
    who: int
    o: int
    ptype_index: int
    path_offset: int
    path_end: int
    path_capacity: int
    path_length: int
    path_increment: int
    orderlist_offset: int
    orderlist_end: int
    nodes: tuple[OrderNode, ...]
    next_owner_offset: int
    sha256: str

    @property
    def size(self) -> int:
        return self.orderlist_end - self.orderlist_offset


class _Reader:
    def __init__(self, data: bytes | bytearray | memoryview, offset: int):
        self.data = memoryview(data)
        if offset < 0 or offset > len(self.data):
            raise UnitOrderListParseError(
                f"offset {offset:#x} is outside {len(self.data):#x}-byte stream"
            )
        self.p = offset

    def raw(self, size: int) -> memoryview:
        if size < 0 or self.p + size > len(self.data):
            raise UnitOrderListParseError(
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

    def i16(self) -> int:
        return struct.unpack("<h", self.raw(2))[0]

    def i32(self) -> int:
        return struct.unpack("<i", self.raw(4))[0]

    def expect_u8(self, expected: int, owner: str) -> int:
        offset = self.p
        value = self.u8()
        if value != expected:
            raise UnitOrderListParseError(
                f"{owner} tag {value:#04x} != {expected:#04x} at {offset:#x}"
            )
        return value


def _move_payload(reader: _Reader) -> MoveOrderPayload:
    offset = reader.p
    raw = bytes(reader.raw(MOVE_ORDER_BYTES))
    words = struct.unpack_from("<18i", raw)
    off_x, off_y = struct.unpack_from("<2h", raw, 72)
    return MoveOrderPayload(
        offset=offset,
        end=reader.p,
        x=words[0],
        y=words[1],
        angle=words[2],
        dest=words[3],
        tolerance=words[4],
        pause=words[5],
        retry=words[6],
        attempts=words[7],
        timer=words[8],
        facing=words[9],
        dest_x=words[10],
        dest_y=words[11],
        last_x=words[12],
        last_y=words[13],
        coll_x=words[14],
        coll_y=words[15],
        orig_x=words[16],
        orig_y=words[17],
        off_x=off_x,
        off_y=off_y,
        sha256=hashlib.sha256(raw).hexdigest(),
    )


def parse_first_unit_orderlist(
    data: bytes | bytearray | memoryview,
    objects_offset: int,
) -> UnitOrderListSection:
    """Parse the first live owner-zero Unit and its complete OrderList.

    ``objects_offset`` must be the boundary supplied by the preceding Groups
    owner.  The function never scans for any tag or field value.
    """

    reader = _Reader(data, objects_offset)
    reader.expect_u8(OBJECTS_TAG, "Objects")
    reader.raw(OBJECTS_FIXED_BYTES)

    owner_array_offset = reader.p
    length = reader.i32()
    capacity = reader.i32()
    increment = reader.i16()
    flags = reader.u8()
    if length <= 0 or length > 1_000_000:
        raise UnitOrderListParseError(f"invalid object-array length {length}")
    if capacity < length or capacity > 1_000_000:
        raise UnitOrderListParseError(
            f"invalid object-array capacity {capacity} for length {length}"
        )
    if flags & 0x40:
        raise UnitOrderListParseError(
            f"object-array save flags retain forbidden 0x40 bit: {flags:#04x}"
        )

    presence = bytes(reader.raw(length))
    if any(value not in (0, 1) for value in presence):
        raise UnitOrderListParseError("object-array presence plane is not boolean")
    live_slots = [slot for slot, present in enumerate(presence) if present]
    if not live_slots:
        raise UnitOrderListParseError("owner-zero object array is empty")

    type_codes = [reader.i32() for _ in live_slots]
    second_capacity = reader.i32()
    second_increment = reader.i16()
    if (second_capacity, second_increment) != (capacity, increment):
        raise UnitOrderListParseError(
            "object-array duplicated capacity/increment image disagrees"
        )

    unit_slot = live_slots[0]
    if unit_slot >= UNIT_BAND_END or type_codes[0] != UNIT_OBJECT_TYPE:
        raise UnitOrderListParseError(
            f"first live body is not a Unit: slot={unit_slot}, type={type_codes[0]}"
        )
    owner_array_body_offset = reader.p
    unit_body_offset = reader.p

    reader.expect_u8(SUBOBJECT_TAG, "SubObject")
    reader.u8()  # SubObject::flags
    if reader.u8() != 1:
        raise UnitOrderListParseError("first Unit SubObject::must_walk is not true")
    who = reader.u8()
    o = reader.i16()
    reader.raw(12)  # z_internal, x_internal, y_internal
    ptype_index = reader.i32()

    reader.expect_u8(OBJECT_TAG, "Object")
    if reader.u8() != 1:
        raise UnitOrderListParseError("first Unit Object::must_walk is not true")
    reader.raw(34)
    launching_present = reader.u8()
    if launching_present != 0:
        raise UnitOrderListParseError(
            "bounded parser does not infer a present Object::launching array"
        )

    reader.expect_u8(UNIT_TAG, "Unit")
    if reader.u8() != 1:
        raise UnitOrderListParseError("first Unit Unit::must_walk is not true")
    reader.raw(UNIT_FIXED_BYTES)

    path_offset = reader.p
    path_capacity = reader.i32()
    path_length = reader.i32()
    path_increment = reader.i8()
    if (
        path_capacity < 0
        or path_capacity > 1_000_000
        or path_length < 0
        or path_length > path_capacity
    ):
        raise UnitOrderListParseError(
            f"invalid PathData stack {path_length}/{path_capacity}"
        )
    reader.raw(path_length * PATH_RECORD_BYTES)
    path_end = reader.p

    orderlist_offset = reader.p
    count = reader.i32()
    if count < 0 or count > 65_536:
        raise UnitOrderListParseError(f"invalid OrderList count {count}")
    nodes = []
    for _ in range(count):
        node_offset = reader.p
        order_type = reader.i32()
        metric_offset = reader.p
        metric = reader.u8()
        order_name = MOVE_ORDER_TYPES.get(order_type)
        if order_name is None:
            raise UnitOrderListParseError(
                f"unsupported concrete order type {order_type} at {node_offset:#x}"
            )
        # MoveOrder virtually inherits UnitOrder.  Its first walk_function call
        # reaches the virtual base through the vbtable and writes UnitOrder::flags;
        # this byte is data, not a walk_test or payload tag.
        order_flags_offset = reader.p
        order_flags = reader.u8()
        payload = _move_payload(reader)
        nodes.append(
            OrderNode(
                offset=node_offset,
                end=reader.p,
                order_type=order_type,
                order_name=order_name,
                metric_offset=metric_offset,
                metric=metric,
                order_flags_offset=order_flags_offset,
                order_flags=order_flags,
                payload=payload,
            )
        )
    orderlist_end = reader.p
    orderlist_bytes = bytes(reader.data[orderlist_offset:orderlist_end])
    return UnitOrderListSection(
        objects_offset=objects_offset,
        owner_array_offset=owner_array_offset,
        owner_array_body_offset=owner_array_body_offset,
        unit_slot=unit_slot,
        unit_body_offset=unit_body_offset,
        who=who,
        o=o,
        ptype_index=ptype_index,
        path_offset=path_offset,
        path_end=path_end,
        path_capacity=path_capacity,
        path_length=path_length,
        path_increment=path_increment,
        orderlist_offset=orderlist_offset,
        orderlist_end=orderlist_end,
        nodes=tuple(nodes),
        next_owner_offset=orderlist_end,
        sha256=hashlib.sha256(orderlist_bytes).hexdigest(),
    )


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def _jsonable(section: UnitOrderListSection) -> dict[str, object]:
    value = dataclasses.asdict(section)
    for key in (
        "objects_offset",
        "owner_array_offset",
        "owner_array_body_offset",
        "unit_body_offset",
        "path_offset",
        "path_end",
        "orderlist_offset",
        "orderlist_end",
        "next_owner_offset",
    ):
        value[key] = f"0x{value[key]:x}"
    return value


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
    section = parse_first_unit_orderlist(_load(args.file), args.objects_offset)
    if args.json:
        print(json.dumps(_jsonable(section), indent=2))
    else:
        node_names = ", ".join(node.order_name for node in section.nodes) or "empty"
        print(
            f"{args.file}: Unit ({section.who},{section.o}) ptype={section.ptype_index}; "
            f"OrderList {section.orderlist_offset:#x}..{section.orderlist_end:#x} "
            f"({section.size} bytes) sha256={section.sha256}; {node_names}; "
            f"next owner PtrArray<Guy> at {section.next_owner_offset:#x}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
