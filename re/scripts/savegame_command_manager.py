#!/usr/bin/env python3
"""Parse retail ``CommandManager::walk_data`` images and the exact installed prefix."""

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


TAG_COMMAND_MANAGER = 0
TAG_COMMAND_PACKAGE = 0
TAG_PACKAGE_FIFO = 0
TAG_COMMAND_MANAGER_INDEX = 574
TAG_COMMAND_PACKAGE_INDEX = 623
TAG_PACKAGE_FIFO_INDEX = 5206
PLAYER_FIFO_COUNT = 8
PACKAGES_PER_FIFO = 20
COMMAND_PACKAGE_SIZE = 536
COMMAND_PAYLOAD_CAPACITY = 512
COMMAND_PACKAGE_HEADER_SIZE = 18
PACKAGE_FIFO_HEADER_SIZE = 16
INSTALLED_FULL_FIFO_COUNT = 5
INSTALLED_PARTIAL_FIFO_INDEX = 5
INSTALLED_PARTIAL_PACKAGE_COUNT = 12
DEFAULT_SCHEMA_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"


class CommandManagerParseError(ValueError):
    """The stream or PDB layout contradicts ``CommandManager::walk_data``."""


@dataclasses.dataclass(frozen=True)
class CommandPackageImage:
    index: int | None
    offset: int
    end: int
    tag: int | None
    stamp: int
    play: int
    valid: int
    group: int
    payload_size: int
    payload: bytes
    raw: bytes
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class PackageFifoImage:
    index: int
    offset: int
    end: int
    tag: int
    front: int
    front_local: int
    length: int
    length_local: int
    packages: tuple[CommandPackageImage, ...]
    raw: bytes
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class CommandManagerImage:
    offset: int
    end: int
    manager_tag: int
    package_tag: int
    local_package: CommandPackageImage
    fifos: tuple[PackageFifoImage, ...]
    complete: bool
    frontier: str
    raw: bytes
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class _Layout:
    sha256: str


COMMAND_MANAGER_FIELDS = (
    ("package_stamps", 4, 32, "unsigned long[8]"),
    ("local_package_stamp", 36, 4, "unsigned long"),
    ("local_package", 40, 536, "CommandPackage"),
    ("package_fifos", 576, 85888, "PackageFifo[8]"),
    ("wait_time_accum", 86464, 4, "unsigned long"),
    ("wait_time_per_player", 86468, 32, "unsigned long[8]"),
    ("end_net_session", 86500, 4, "int"),
    ("message_set", 86504, 4, "int"),
    ("mp_playback", 86508, 120, "RecordGame"),
    ("use_mp_playback", 86628, 1, "unsigned char"),
)

PACKAGE_FIFO_FIELDS = (
    ("front", 0, 4, "int"),
    ("front_local", 4, 4, "int"),
    ("length", 8, 4, "int"),
    ("length_local", 12, 4, "int"),
    ("packages", 16, 10720, "CommandPackage[20]"),
)

COMMAND_PACKAGE_FIELDS = (
    ("stamp", 0, 4, "unsigned long"),
    ("play", 4, 4, "int"),
    ("valid", 8, 4, "int"),
    ("group", 12, 4, "int"),
    ("size", 16, 2, "short"),
    ("data", 18, 512, "unsigned char[512]"),
    ("padding", 532, 4, "Random"),
)


def _fields(record: dict[str, object]) -> tuple[tuple[object, ...], ...]:
    return tuple(
        (field["name"], field["offset"], field["size"], field["type"])
        for field in record["fields"]
    )


@functools.lru_cache(maxsize=4)
def _load_layout(path_text: str) -> _Layout:
    path = pathlib.Path(path_text)
    try:
        classes = json.loads(path.read_bytes())["classes"]
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise CommandManagerParseError(
            f"cannot load CommandManager PDB layout from {path}: {error}"
        ) from error

    expected = {
        "CommandManager": (86632, COMMAND_MANAGER_FIELDS),
        "PackageFifo": (10736, PACKAGE_FIFO_FIELDS),
        "CommandPackage": (COMMAND_PACKAGE_SIZE, COMMAND_PACKAGE_FIELDS),
    }
    receipt: dict[str, object] = {}
    for name, (size, fields) in expected.items():
        record = classes.get(name)
        actual = _fields(record) if record else ()
        if not record or record.get("size") != size or actual != fields:
            raise CommandManagerParseError(
                f"PDB {name} layout disagrees: "
                f"size={record.get('size') if record else None}, fields={actual!r}"
            )
        receipt[name] = {"size": size, "fields": fields}

    bases = {
        name: tuple((base["name"], base["offset"], base["size"]) for base in classes[name]["bases"])
        for name in expected
    }
    expected_bases = {
        "CommandManager": (("GameAccess", 4, 1),),
        "PackageFifo": (("GameAccess", 0, 1),),
        "CommandPackage": (("GameAccess", 0, 1),),
    }
    if bases != expected_bases:
        raise CommandManagerParseError(f"PDB CommandManager bases disagree: {bases!r}")

    virtual_bases = tuple(
        (
            base["name"],
            base["size"],
            base["vbptr_offset"],
            base["vbtable_index"],
            base["derived_offset"],
        )
        for base in classes["CommandManager"]["virtual_bases"]
    )
    if virtual_bases != (("MiscAccess", 1, 0, 1, 86631),):
        raise CommandManagerParseError(
            f"PDB CommandManager virtual base disagrees: {virtual_bases!r}"
        )

    receipt["bases"] = bases
    receipt["virtual_bases"] = virtual_bases
    receipt["selectors"] = {
        "manager_tag": TAG_COMMAND_MANAGER_INDEX,
        "package_tag": TAG_COMMAND_PACKAGE_INDEX,
        "fifo_tag": TAG_PACKAGE_FIFO_INDEX,
        "players": PLAYER_FIFO_COUNT,
        "packages_per_fifo": PACKAGES_PER_FIFO,
        "package_header": [0, COMMAND_PACKAGE_HEADER_SIZE],
        "payload_capacity": COMMAND_PAYLOAD_CAPACITY,
        "fifo_header": [0, PACKAGE_FIFO_HEADER_SIZE],
        "next_owner": "PtrArray<River>::walk_data",
    }
    encoded = json.dumps(receipt, sort_keys=True, separators=(",", ":")).encode()
    return _Layout(hashlib.sha256(encoded).hexdigest())


class _Reader:
    def __init__(self, data: bytes | bytearray | memoryview, offset: int):
        self.data = memoryview(data)
        if offset < 0 or offset > len(self.data):
            raise CommandManagerParseError(f"offset {offset:#x} is outside the stream")
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise CommandManagerParseError(
                f"{what} range [{self.pos:#x},{end:#x}) exceeds {len(self.data):#x}-byte stream"
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

    def u32(self, what: str) -> int:
        return struct.unpack("<I", self.take(4, what))[0]


def _read_tag(reader: _Reader, what: str, expected: int, require_tags: bool) -> int:
    at = reader.pos
    tag = reader.u8(what)
    if require_tags and tag != expected:
        raise CommandManagerParseError(
            f"{what} {tag:#04x} != {expected:#04x} at {at:#x}"
        )
    return tag


def _parse_package(
    reader: _Reader,
    fifo_index: int | None,
    package_index: int | None,
    *,
    tagged: bool,
    require_tags: bool,
) -> CommandPackageImage:
    label = "local_package" if fifo_index is None else f"package_fifos[{fifo_index}].packages[{package_index}]"
    start = reader.pos
    tag = _read_tag(reader, f"{label} tag", TAG_COMMAND_PACKAGE, require_tags) if tagged else None
    stamp = reader.u32(f"{label}.stamp")
    play = reader.i32(f"{label}.play")
    valid = reader.i32(f"{label}.valid")
    group = reader.i32(f"{label}.group")
    payload_size = reader.i16(f"{label}.size")
    if payload_size < 0 or payload_size > COMMAND_PAYLOAD_CAPACITY:
        raise CommandManagerParseError(
            f"{label}.size {payload_size} outside [0,{COMMAND_PAYLOAD_CAPACITY}]"
        )
    payload = bytes(reader.take(payload_size, f"{label}.data"))
    raw = bytes(reader.data[start:reader.pos])
    return CommandPackageImage(
        package_index,
        start,
        reader.pos,
        tag,
        stamp,
        play,
        valid,
        group,
        payload_size,
        payload,
        raw,
        hashlib.sha256(raw).hexdigest(),
    )


def _parse_fifo(
    reader: _Reader,
    fifo_index: int,
    package_count: int,
    require_tags: bool,
) -> PackageFifoImage:
    start = reader.pos
    tag = _read_tag(reader, f"package_fifos[{fifo_index}] tag", TAG_PACKAGE_FIFO, require_tags)
    header = tuple(reader.i32(f"package_fifos[{fifo_index}].{name}") for name, *_ in PACKAGE_FIFO_FIELDS[:4])
    packages = tuple(
        _parse_package(
            reader,
            fifo_index,
            package_index,
            tagged=True,
            require_tags=require_tags,
        )
        for package_index in range(package_count)
    )
    raw = bytes(reader.data[start:reader.pos])
    return PackageFifoImage(
        fifo_index,
        start,
        reader.pos,
        tag,
        *header,
        packages,
        raw,
        hashlib.sha256(raw).hexdigest(),
    )


def _parse_prefix(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    complete: bool,
    require_tags: bool,
    schema_path: pathlib.Path,
) -> CommandManagerImage:
    layout = _load_layout(str(pathlib.Path(schema_path).resolve()))
    reader = _Reader(data, offset)
    manager_tag = _read_tag(reader, "CommandManager tag", TAG_COMMAND_MANAGER, require_tags)
    package_tag = _read_tag(reader, "CommandPackage tag", TAG_COMMAND_PACKAGE, require_tags)
    local_package = _parse_package(
        reader,
        None,
        None,
        tagged=False,
        require_tags=require_tags,
    )
    if complete:
        counts = (PACKAGES_PER_FIFO,) * PLAYER_FIFO_COUNT
        frontier = "PtrArray<River>::walk_data"
    else:
        counts = (PACKAGES_PER_FIFO,) * INSTALLED_FULL_FIFO_COUNT + (INSTALLED_PARTIAL_PACKAGE_COUNT,)
        frontier = (
            f"package_fifos[{INSTALLED_PARTIAL_FIFO_INDEX}].packages"
            f"[{INSTALLED_PARTIAL_PACKAGE_COUNT}] tag"
        )
    fifos = tuple(
        _parse_fifo(reader, fifo_index, package_count, require_tags)
        for fifo_index, package_count in enumerate(counts)
    )
    raw = bytes(reader.data[offset:reader.pos])
    return CommandManagerImage(
        offset,
        reader.pos,
        manager_tag,
        package_tag,
        local_package,
        fifos,
        complete,
        frontier,
        raw,
        hashlib.sha256(raw).hexdigest(),
        layout.sha256,
    )


def parse_command_manager_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tags: bool = True,
    schema_path: pathlib.Path = DEFAULT_SCHEMA_PATH,
) -> CommandManagerImage:
    """Parse a structurally valid complete CommandManager owner."""
    return _parse_prefix(
        data,
        offset,
        complete=True,
        require_tags=require_tags,
        schema_path=schema_path,
    )


def parse_installed_command_manager_prefix(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tags: bool = True,
    schema_path: pathlib.Path = DEFAULT_SCHEMA_PATH,
) -> CommandManagerImage:
    """Parse the maximal exact installed prefix ending before its first contradiction."""
    return _parse_prefix(
        data,
        offset,
        complete=False,
        require_tags=require_tags,
        schema_path=schema_path,
    )


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument("--offset", required=True, type=lambda text: int(text, 0))
    parser.add_argument(
        "--installed-prefix",
        action="store_true",
        help="stop before the installed FIFO 5 / package 12 contradiction",
    )
    args = parser.parse_args(argv)
    data = _load(args.file)
    if args.installed_prefix:
        section = parse_installed_command_manager_prefix(data, args.offset)
    else:
        section = parse_command_manager_section(data, args.offset)
    print(
        f"{args.file}: CommandManager {section.offset:#x}..{section.end:#x} "
        f"({section.size} bytes) sha256={section.sha256}\n"
        f"  complete={section.complete}, fifos={len(section.fifos)}, "
        f"next={section.frontier}"
    )
    if not section.complete:
        print(f"  observed frontier byte={data[section.end]:#04x}; expected tag={TAG_COMMAND_PACKAGE:#04x}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
