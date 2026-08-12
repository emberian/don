#!/usr/bin/env python3
"""Parse complete retail ``GraphicEvents::walk_data`` save images.

The event-slot count is deliberately explicit: retail obtains it from the
runtime GraphicPieces/ammo registries and does not serialize it in this owner.
The parser preserves every pointer-array history, presence plane, linked
EventGroup, conditional GraphicEvent payload, trailing typed array, and missile
offset, then stops at ``Scene::walk_data``.
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


TAG_GRAPHIC_EVENTS = 0
TAG_STRING_TABLE_INDEX = 3575
EVENT_KINDS_PER_GROUP = 38
FRESH_RETAIL_SLOT_COUNT = 15368
MAX_COUNT = 1 << 20
DEFAULT_SCHEMA_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"


class GraphicEventsParseError(ValueError):
    """The stream or PDB layout contradicts GraphicEvents::walk_data."""


@dataclasses.dataclass(frozen=True)
class GraphicEventImage:
    index: int
    offset: int
    end: int
    event_type: int
    payload: bytes
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class GraphicEventArray:
    kind: int
    offset: int
    end: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    presence_offset: int | None
    presence: tuple[int, ...]
    repeated_capacity: int | None
    repeated_increment: int | None
    events: tuple[GraphicEventImage | None, ...]
    sha256: str


@dataclasses.dataclass(frozen=True)
class EventGroupImage:
    slot: int
    chain_index: int
    presence_offset: int
    offset: int
    end: int
    event_arrays: tuple[GraphicEventArray, ...]
    civ: int
    age: int
    next_presence_offset: int
    next_present: int
    sha256: str


@dataclasses.dataclass(frozen=True)
class PodArray:
    name: str
    offset: int
    end: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    element_size: int
    data: bytes
    sha256: str


@dataclasses.dataclass(frozen=True)
class GraphicEventsSection:
    offset: int
    end: int
    tag: int
    slot_count: int
    presence_offset: int
    presence: tuple[int, ...]
    groups: tuple[EventGroupImage, ...]
    arrays: tuple[PodArray, ...]
    missile_offset: tuple[int, int, int]
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class _Layout:
    sha256: str


def _fields(record: dict[str, object]) -> tuple[tuple[object, ...], ...]:
    return tuple((field["name"], field["offset"], field["size"], field["type"]) for field in record["flattened"])


@functools.lru_cache(maxsize=4)
def _load_layout(path_text: str) -> _Layout:
    path = pathlib.Path(path_text)
    try:
        classes = json.loads(path.read_bytes())["classes"]
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise GraphicEventsParseError(f"cannot load GraphicEvents PDB layout from {path}: {error}") from error
    expected_sizes = {
        "GraphicEvents": 220,
        "EventGroup": 1072,
        "PtrArray<GraphicEvent>": 28,
        "GraphicEvent": 36,
        "SimpleArray<unsigned short>": 28,
        "SimpleArray<int>": 28,
        "SimpleArray<float>": 28,
        "Array<AmbienceStruct>": 28,
        "AmbienceStruct": 24,
    }
    receipt: dict[str, object] = {}
    for name, size in expected_sizes.items():
        record = classes.get(name)
        if not record or record.get("size") != size:
            raise GraphicEventsParseError(f"PDB {name} size disagrees: {record.get('size') if record else None}")
        receipt[name] = {"size": size, "flattened": _fields(record)}
    graphic = {name: (offset, size, kind) for name, offset, size, kind in _fields(classes["GraphicEvents"])}
    expected_graphic = {
        "events": (4, 28, "PtrArray<EventGroup>"),
        "loaded": (32, 28, "SimpleArray<unsigned char>"),
        "pre_load_gpieces": (60, 28, "SimpleArray<int>"),
        "entrench_who": (88, 28, "SimpleArray<unsigned short>"),
        "entrench_o": (116, 28, "SimpleArray<int>"),
        "entrench_angle": (144, 28, "SimpleArray<float>"),
        "ambience_structs": (172, 28, "Array<AmbienceStruct>"),
        "missile_offset_x": (200, 4, "Coord"),
        "missile_offset_y": (204, 4, "Coord"),
        "missile_offset_z": (208, 4, "Coord"),
    }
    if graphic != expected_graphic:
        raise GraphicEventsParseError(f"PDB GraphicEvents fields disagree: {graphic!r}")
    group = _fields(classes["EventGroup"])
    if group != (("events", 0, 1064, "PtrArray<GraphicEvent>[38]"), ("civ", 1064, 1, "char"), ("age", 1065, 1, "char"), ("next", 1068, 4, "EventGroup*")):
        raise GraphicEventsParseError(f"PDB EventGroup fields disagree: {group!r}")
    ptr = _fields(classes["PtrArray<GraphicEvent>"])
    if ptr != (("length", 4, 4, "int"), ("size", 8, 4, "int"), ("increment", 12, 2, "short"), ("list", 16, 4, "GraphicEvent**"), ("flags", 20, 1, "unsigned char"), ("cur_index", 24, 4, "int")):
        raise GraphicEventsParseError(f"PDB PtrArray<GraphicEvent> fields disagree: {ptr!r}")
    event = classes["GraphicEvent"]
    if not event.get("has_vfptr") or _fields(event)[0][:3] != ("event_type", 4, 4):
        raise GraphicEventsParseError("PDB GraphicEvent vptr/event_type layout disagrees")
    ambience = _fields(classes["AmbienceStruct"])
    if ambience[-1] != ("ambience_seen", 22, 1, "unsigned char"):
        raise GraphicEventsParseError("PDB AmbienceStruct serialized prefix disagrees")
    receipt["selectors"] = {
        "GraphicEvent": {"event_type": [4, 8], "type8": [8, 27], "other": [8, 36]},
        "EventGroup": {"arrays": 38, "civ_age": [1064, 1066], "next": 1068},
        "GraphicEvents": {"saved_arrays": [88, 116, 144, 172], "missile": [200, 212]},
    }
    return _Layout(hashlib.sha256(json.dumps(receipt, sort_keys=True, separators=(",", ":")).encode()).hexdigest())


class _Reader:
    def __init__(self, data: bytes | bytearray | memoryview, offset: int):
        self.data = memoryview(data)
        if offset < 0 or offset > len(self.data):
            raise GraphicEventsParseError(f"offset {offset:#x} is outside {len(self.data):#x}-byte stream")
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise GraphicEventsParseError(f"{what} range [{self.pos:#x},{end:#x}) exceeds {len(self.data):#x}-byte stream")
        start = self.pos
        self.pos = end
        return self.data[start:end]

    def u8(self, what: str) -> int: return self.take(1, what)[0]
    def i8(self, what: str) -> int: return struct.unpack("<b", self.take(1, what))[0]
    def i16(self, what: str) -> int: return struct.unpack("<h", self.take(2, what))[0]
    def i32(self, what: str) -> int: return struct.unpack("<i", self.take(4, what))[0]


def _count(reader: _Reader, what: str) -> int:
    value = reader.i32(what)
    if value < 0 or value > MAX_COUNT:
        raise GraphicEventsParseError(f"invalid {what} {value}")
    return value


def _event(reader: _Reader, index: int) -> GraphicEventImage:
    offset = reader.pos
    event_type = reader.i32(f"GraphicEvent[{index}].event_type")
    payload = bytes(reader.take(19 if event_type == 8 else 28, f"GraphicEvent[{index}] payload"))
    return GraphicEventImage(index, offset, reader.pos, event_type, payload, hashlib.sha256(reader.data[offset:reader.pos]).hexdigest())


def _event_array(reader: _Reader, kind: int) -> GraphicEventArray:
    offset = reader.pos
    length = _count(reader, f"event array {kind} length")
    capacity = increment = flags = repeated_capacity = repeated_increment = None
    presence_offset = None
    presence: tuple[int, ...] = ()
    events: tuple[GraphicEventImage | None, ...] = ()
    if length:
        capacity = _count(reader, f"event array {kind} capacity")
        increment = reader.i16(f"event array {kind} increment")
        flags = reader.u8(f"event array {kind} flags")
        if capacity < length or flags & 0x40:
            raise GraphicEventsParseError(f"invalid event array {kind} history")
        presence_offset = reader.pos
        presence = tuple(reader.u8(f"event array {kind} presence[{i}]") for i in range(length))
        if any(value not in (0, 1) for value in presence):
            raise GraphicEventsParseError(f"event array {kind} presence is not boolean")
        repeated_capacity = reader.i32(f"event array {kind} repeated capacity")
        repeated_increment = reader.i16(f"event array {kind} repeated increment")
        if (repeated_capacity, repeated_increment) != (capacity, increment):
            raise GraphicEventsParseError(f"event array {kind} duplicated history disagrees")
        events = tuple(_event(reader, i) if present else None for i, present in enumerate(presence))
    return GraphicEventArray(kind, offset, reader.pos, length, capacity, increment, flags, presence_offset, presence, repeated_capacity, repeated_increment, events, hashlib.sha256(reader.data[offset:reader.pos]).hexdigest())


def _group(reader: _Reader, slot: int, chain_index: int, presence_offset: int) -> EventGroupImage:
    offset = reader.pos
    arrays = tuple(_event_array(reader, kind) for kind in range(EVENT_KINDS_PER_GROUP))
    civ = reader.i8(f"group {slot}:{chain_index} civ")
    age = reader.i8(f"group {slot}:{chain_index} age")
    next_presence_offset = reader.pos
    next_present = reader.u8(f"group {slot}:{chain_index} next presence")
    if next_present not in (0, 1):
        raise GraphicEventsParseError("EventGroup next presence is not boolean")
    return EventGroupImage(slot, chain_index, presence_offset, offset, reader.pos, arrays, civ, age, next_presence_offset, next_present, hashlib.sha256(reader.data[offset:reader.pos]).hexdigest())


def _array(reader: _Reader, name: str, element_size: int) -> PodArray:
    offset = reader.pos
    length = _count(reader, f"{name} length")
    capacity = increment = flags = None
    data = b""
    if length:
        capacity = _count(reader, f"{name} capacity")
        increment = reader.i16(f"{name} increment")
        flags = reader.u8(f"{name} flags")
        if capacity < length or flags & 0x40:
            raise GraphicEventsParseError(f"invalid {name} history")
        data = bytes(reader.take(length * element_size, f"{name} data"))
    return PodArray(name, offset, reader.pos, length, capacity, increment, flags, element_size, data, hashlib.sha256(reader.data[offset:reader.pos]).hexdigest())


def parse_graphic_events_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    slot_count: int,
    *,
    require_tag: bool = True,
    schema_path: pathlib.Path = DEFAULT_SCHEMA_PATH,
) -> GraphicEventsSection:
    """Parse one complete section using the caller's runtime event-slot count."""

    if slot_count < 0 or slot_count > MAX_COUNT:
        raise GraphicEventsParseError(f"invalid runtime event slot count {slot_count}")
    layout = _load_layout(str(pathlib.Path(schema_path).resolve()))
    reader = _Reader(data, offset)
    tag = reader.u8("GraphicEvents tag")
    if require_tag and tag != TAG_GRAPHIC_EVENTS:
        raise GraphicEventsParseError(f"GraphicEvents tag {tag:#04x} != {TAG_GRAPHIC_EVENTS:#04x}")
    presence_offset = reader.pos
    presence = tuple(reader.u8(f"GraphicEvents.presence[{i}]") for i in range(slot_count))
    if any(value not in (0, 1) for value in presence):
        raise GraphicEventsParseError("GraphicEvents slot-presence plane is not boolean")
    groups: list[EventGroupImage] = []
    for slot, present in enumerate(presence):
        next_present = present
        chain_index = 0
        marker_offset = presence_offset + slot
        while next_present:
            group = _group(reader, slot, chain_index, marker_offset)
            groups.append(group)
            marker_offset = group.next_presence_offset
            next_present = group.next_present
            chain_index += 1
            if chain_index > MAX_COUNT:
                raise GraphicEventsParseError("EventGroup chain is too long")
    arrays = (
        _array(reader, "entrench_who", 2),
        _array(reader, "entrench_o", 4),
        _array(reader, "entrench_angle", 4),
        _array(reader, "ambience_structs", 24),
    )
    missile_offset = (reader.i32("missile_offset_x"), reader.i32("missile_offset_y"), reader.i32("missile_offset_z"))
    return GraphicEventsSection(offset, reader.pos, tag, slot_count, presence_offset, presence, tuple(groups), arrays, missile_offset, hashlib.sha256(reader.data[offset:reader.pos]).hexdigest(), layout.sha256)


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def _summary(section: GraphicEventsSection, path: pathlib.Path) -> str:
    return "\n".join((
        f"{path}: GraphicEvents {section.offset:#x}..{section.end:#x} ({section.size} bytes) sha256={section.sha256}",
        f"  slots={section.slot_count}, present roots={sum(section.presence)}, linked groups={len(section.groups)}",
        f"  arrays={[(array.name, array.length) for array in section.arrays]}, missile_offset={section.missile_offset}",
        f"  next owner begins at {section.end:#x}: Scene::walk_data 0x008c0f70",
    ))


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument("--offset", required=True, type=lambda text: int(text, 0))
    parser.add_argument("--slot-count", required=True, type=lambda text: int(text, 0))
    args = parser.parse_args(argv)
    print(_summary(parse_graphic_events_section(_load(args.file), args.offset, args.slot_count), args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
