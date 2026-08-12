#!/usr/bin/env python3
"""Parse the complete retail ``Scene::walk_data`` save image."""

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


TAG_SCENE = 0
TAG_STRING_TABLE_INDEX = 5508
MAX_COUNT = 1 << 20
DEFAULT_SCHEMA_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"


class SceneParseError(ValueError):
    """The stream or PDB layout contradicts Scene::walk_data."""


@dataclasses.dataclass(frozen=True)
class BitMaskImage:
    name: str
    offset: int
    end: int
    bits: int
    payload_size: int
    payload: bytes
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
class SceneSection:
    offset: int
    end: int
    tag: int
    masks: tuple[BitMaskImage, ...]
    arrays: tuple[PodArray, ...]
    last_time: int
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
        raise SceneParseError(f"cannot load Scene PDB layout from {path}: {error}") from error
    expected_sizes = {
        "Scene": 824, "BitMask<32>": 16, "SimpleArray<Coord>": 28,
        "SimpleArray<unsigned char>": 28, "SimpleArray<unsigned long>": 28,
    }
    receipt: dict[str, object] = {}
    for name, size in expected_sizes.items():
        record = classes.get(name)
        if not record or record.get("size") != size:
            raise SceneParseError(f"PDB {name} size disagrees")
        receipt[name] = {"size": size, "flattened": _fields(record)}
    scene = {name: (offset, size, kind) for name, offset, size, kind in _fields(classes["Scene"])}
    expected = {
        "flags": (484, 16, "BitMask<32>"), "draw_flags": (500, 16, "BitMask<32>"),
        "ping_x": (560, 28, "SimpleArray<Coord>"), "ping_y": (588, 28, "SimpleArray<Coord>"),
        "ping_who": (616, 28, "SimpleArray<unsigned char>"),
        "ping_timer": (644, 28, "SimpleArray<unsigned char>"),
        "ping_finish_frame": (672, 28, "SimpleArray<unsigned long>"),
        "last_time": (772, 4, "int"),
    }
    if any(scene.get(name) != value for name, value in expected.items()):
        raise SceneParseError("PDB Scene serialized fields disagree")
    mask = _fields(classes["BitMask<32>"])
    if mask != (("bits", 0, 4, "int"), ("size", 4, 4, "int"), ("flags", 8, 4, "int"), ("ptr", 12, 4, "unsigned char[4]")):
        raise SceneParseError(f"PDB BitMask<32> layout disagrees: {mask!r}")
    array_prefix = (("length", 4, 4, "int"), ("size", 8, 4, "int"), ("increment", 12, 2, "short"))
    for name in ("SimpleArray<Coord>", "SimpleArray<unsigned char>", "SimpleArray<unsigned long>"):
        if _fields(classes[name])[:3] != array_prefix:
            raise SceneParseError(f"PDB {name} history disagrees")
    receipt["selectors"] = {"masks": [[484, 492], [500, 508]], "arrays": [560, 588, 616, 644, 672], "direct": [772, 776]}
    return _Layout(hashlib.sha256(json.dumps(receipt, sort_keys=True, separators=(",", ":")).encode()).hexdigest())


class _Reader:
    def __init__(self, data: bytes | bytearray | memoryview, offset: int):
        self.data = memoryview(data)
        if offset < 0 or offset > len(self.data): raise SceneParseError(f"offset {offset:#x} is outside the stream")
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data): raise SceneParseError(f"{what} range [{self.pos:#x},{end:#x}) exceeds {len(self.data):#x}-byte stream")
        start = self.pos; self.pos = end; return self.data[start:end]

    def u8(self, what: str) -> int: return self.take(1, what)[0]
    def i16(self, what: str) -> int: return struct.unpack("<h", self.take(2, what))[0]
    def i32(self, what: str) -> int: return struct.unpack("<i", self.take(4, what))[0]


def _count(reader: _Reader, what: str) -> int:
    value = reader.i32(what)
    if value < 0 or value > MAX_COUNT: raise SceneParseError(f"invalid {what} {value}")
    return value


def _mask(reader: _Reader, name: str) -> BitMaskImage:
    offset = reader.pos
    bits = reader.i32(f"{name}.bits")
    size = reader.i32(f"{name}.size")
    if bits < 0 or bits > 32 or size < 0 or size > 4 or bits > size * 8:
        raise SceneParseError(f"invalid {name} bits/size {bits}/{size}")
    payload = bytes(reader.take(size, f"{name}.payload"))
    return BitMaskImage(name, offset, reader.pos, bits, size, payload, hashlib.sha256(reader.data[offset:reader.pos]).hexdigest())


def _array(reader: _Reader, name: str, element_size: int) -> PodArray:
    offset = reader.pos
    length = _count(reader, f"{name} length")
    capacity = increment = flags = None; data = b""
    if length:
        capacity = _count(reader, f"{name} capacity"); increment = reader.i16(f"{name} increment"); flags = reader.u8(f"{name} flags")
        if capacity < length or flags & 0x40: raise SceneParseError(f"invalid {name} history")
        data = bytes(reader.take(length * element_size, f"{name} data"))
    return PodArray(name, offset, reader.pos, length, capacity, increment, flags, element_size, data, hashlib.sha256(reader.data[offset:reader.pos]).hexdigest())


def parse_scene_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tag: bool = True,
    schema_path: pathlib.Path = DEFAULT_SCHEMA_PATH,
) -> SceneSection:
    """Parse the complete Scene owner and return the following caller tag."""

    layout = _load_layout(str(pathlib.Path(schema_path).resolve()))
    reader = _Reader(data, offset)
    tag = reader.u8("Scene tag")
    if require_tag and tag != TAG_SCENE: raise SceneParseError(f"Scene tag {tag:#04x} != {TAG_SCENE:#04x}")
    masks = (_mask(reader, "flags"), _mask(reader, "draw_flags"))
    arrays = (
        _array(reader, "ping_x", 4), _array(reader, "ping_y", 4),
        _array(reader, "ping_who", 1), _array(reader, "ping_timer", 1),
        _array(reader, "ping_finish_frame", 4),
    )
    last_time = reader.i32("Scene.last_time")
    return SceneSection(offset, reader.pos, tag, masks, arrays, last_time, hashlib.sha256(reader.data[offset:reader.pos]).hexdigest(), layout.sha256)


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes(); return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__); parser.add_argument("file", type=pathlib.Path); parser.add_argument("--offset", required=True, type=lambda text: int(text, 0)); args = parser.parse_args(argv)
    section = parse_scene_section(_load(args.file), args.offset)
    print(f"{args.file}: Scene {section.offset:#x}..{section.end:#x} ({section.size} bytes) sha256={section.sha256}\n  masks={[(m.bits,m.payload_size) for m in section.masks]} arrays={[(a.name,a.length) for a in section.arrays]} last_time={section.last_time}\n  next owner begins at {section.end:#x}: caller FarmStruct tag")
    return 0


if __name__ == "__main__": raise SystemExit(main())
