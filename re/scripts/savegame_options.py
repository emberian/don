#!/usr/bin/env python3
"""Parse the complete retail ``Options::walk_data`` save image."""

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


TAG_OPTIONS = 0
TAG_STRING_TABLE_INDEX = 5077
OPTION_SIZE = 20
OPTION_WALKED_SIZE = 18
OPTIONS_DIRECT_OFFSET = 0x20
OPTIONS_DIRECT_END = 0x7C
OPTIONS_OPT_OFFSET = 0x7C
OPTIONS_OPT_END = 0x8E
MAX_ARRAY_LENGTH = 1 << 20
DEFAULT_SCHEMA_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"


class OptionsParseError(ValueError):
    """The stream or PDB layout contradicts ``Options::walk_data``."""


@dataclasses.dataclass(frozen=True)
class OptionImage:
    index: int | None
    offset: int
    end: int
    option: int
    object: int
    count: int
    disable: int
    grid_x: int
    grid_y: int
    raw: bytes
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class OptionArray:
    offset: int
    end: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    rows: tuple[OptionImage, ...]
    raw: bytes
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class OptionsSection:
    offset: int
    end: int
    array: OptionArray
    tag: int
    direct_words: tuple[int, ...]
    selected: OptionImage
    raw: bytes
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class _Layout:
    sha256: str


ARRAY_FIELDS = (
    ("length", 4, 4, "int"),
    ("size", 8, 4, "int"),
    ("increment", 12, 2, "short"),
    ("list", 16, 4, "Option*"),
    ("flags", 20, 1, "unsigned char"),
    ("cur_index", 24, 4, "int"),
)

OPTION_FIELDS = (
    ("option", 0, 4, "enum OptionIndex"),
    ("object", 4, 4, "int"),
    ("count", 8, 4, "int"),
    ("disable", 12, 4, "int"),
    ("grid_x", 16, 1, "char"),
    ("grid_y", 17, 1, "char"),
)

OPTIONS_FIELDS = (
    ("num", 32, 4, "int"),
    ("selecting_spot", 36, 4, "int"),
    ("editor_unit_move_drag", 40, 4, "int"),
    ("editor_unit_rotate_drag", 44, 4, "int"),
    ("cycle_research", 48, 64, "int[16]"),
    ("cycle_index", 112, 4, "int"),
    ("mode", 116, 4, "enum OptionMode"),
    ("mode_data", 120, 4, "int"),
    ("opt", 124, 20, "Option"),
    ("rebuild", 144, 4, "int"),
    ("cycle_stamp", 148, 4, "int"),
    ("cycle_level", 152, 4, "int"),
    ("cycle_opt", 156, 20, "Option"),
    ("drop_type", 176, 4, "int"),
    ("drop_x", 180, 4, "Coord"),
    ("drop_y", 184, 4, "Coord"),
    ("drop_z", 188, 4, "Coord"),
    ("place_x", 192, 4, "Coord"),
    ("place_y", 196, 4, "Coord"),
    ("draw_x", 200, 4, "int"),
    ("draw_y", 204, 4, "int"),
    ("shortcut_string", 208, 20, "String"),
    ("current_opt_obj", 228, 4, "int"),
)


def _fields(record: dict[str, object], key: str) -> tuple[tuple[object, ...], ...]:
    return tuple(
        (field["name"], field["offset"], field["size"], field["type"])
        for field in record[key]
    )


@functools.lru_cache(maxsize=4)
def _load_layout(path_text: str) -> _Layout:
    path = pathlib.Path(path_text)
    try:
        classes = json.loads(path.read_bytes())["classes"]
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise OptionsParseError(f"cannot load Options PDB layout from {path}: {error}") from error

    expected = {
        "Options": (232, OPTIONS_FIELDS, "fields"),
        "Array<Option>": (28, ARRAY_FIELDS, "flattened"),
        "Option": (20, OPTION_FIELDS, "fields"),
    }
    receipt: dict[str, object] = {}
    for name, (size, fields, key) in expected.items():
        record = classes.get(name)
        actual = _fields(record, key) if record else ()
        if not record or record.get("size") != size or actual != fields:
            raise OptionsParseError(
                f"PDB {name} layout disagrees: "
                f"size={record.get('size') if record else None}, fields={actual!r}"
            )
        receipt[name] = {"size": size, key: fields}

    bases = {
        name: tuple((base["name"], base["offset"], base["size"]) for base in classes[name]["bases"])
        for name in ("Options", "Array<Option>")
    }
    expected_bases = {
        "Options": (("Array<Option>", 0, 28), ("GameAccessConst", 32, 1)),
        "Array<Option>": (("ArrayBaseSimpleCopy<Option>", 0, 24),),
    }
    if bases != expected_bases:
        raise OptionsParseError(f"PDB Options bases disagree: {bases!r}")

    virtual_bases = tuple(
        (
            base["name"],
            base["size"],
            base["vbptr_offset"],
            base["vbtable_index"],
            base["derived_offset"],
        )
        for base in classes["Options"]["virtual_bases"]
    )
    if virtual_bases != (("MiscAccess", 1, 28, 1, 231),):
        raise OptionsParseError(f"PDB Options virtual base disagrees: {virtual_bases!r}")

    receipt["bases"] = bases
    receipt["virtual_bases"] = virtual_bases
    receipt["selectors"] = {
        "array_length": [4, 8],
        "array_rows": [0, OPTION_WALKED_SIZE],
        "tag": TAG_STRING_TABLE_INDEX,
        "direct": [OPTIONS_DIRECT_OFFSET, OPTIONS_DIRECT_END],
        "selected": [OPTIONS_OPT_OFFSET, OPTIONS_OPT_END],
        "next_owner": "CommandManager::walk_data",
    }
    encoded = json.dumps(receipt, sort_keys=True, separators=(",", ":")).encode()
    return _Layout(hashlib.sha256(encoded).hexdigest())


class _Reader:
    def __init__(self, data: bytes | bytearray | memoryview, offset: int):
        self.data = memoryview(data)
        if offset < 0 or offset > len(self.data):
            raise OptionsParseError(f"offset {offset:#x} is outside the stream")
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise OptionsParseError(
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


def _parse_option(reader: _Reader, index: int | None, what: str) -> OptionImage:
    start = reader.pos
    values = tuple(reader.i32(f"{what}.{name}") for name, *_ in OPTION_FIELDS[:4])
    grid_x = reader.i8(f"{what}.grid_x")
    grid_y = reader.i8(f"{what}.grid_y")
    raw = bytes(reader.data[start:reader.pos])
    return OptionImage(
        index,
        start,
        reader.pos,
        *values,
        grid_x,
        grid_y,
        raw,
        hashlib.sha256(raw).hexdigest(),
    )


def _parse_array(reader: _Reader) -> OptionArray:
    start = reader.pos
    length = reader.i32("Array<Option>.length")
    if length < 0 or length > MAX_ARRAY_LENGTH:
        raise OptionsParseError(f"Array<Option> has invalid length {length}")
    if not length:
        raw = bytes(reader.data[start:reader.pos])
        return OptionArray(start, reader.pos, 0, None, None, None, (), raw, hashlib.sha256(raw).hexdigest())

    capacity = reader.i32("Array<Option>.capacity")
    increment = reader.i16("Array<Option>.increment")
    flags = reader.u8("Array<Option>.flags")
    if capacity < length or capacity > MAX_ARRAY_LENGTH or flags & 0x40:
        raise OptionsParseError(
            f"Array<Option> has invalid history length={length}, "
            f"capacity={capacity}, flags={flags:#04x}"
        )
    rows = tuple(_parse_option(reader, index, f"options[{index}]") for index in range(length))
    raw = bytes(reader.data[start:reader.pos])
    return OptionArray(
        start,
        reader.pos,
        length,
        capacity,
        increment,
        flags,
        rows,
        raw,
        hashlib.sha256(raw).hexdigest(),
    )


def parse_options_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tag: bool = True,
    schema_path: pathlib.Path = DEFAULT_SCHEMA_PATH,
) -> OptionsSection:
    """Parse Options, stopping at the first byte owned by CommandManager."""
    layout = _load_layout(str(pathlib.Path(schema_path).resolve()))
    reader = _Reader(data, offset)
    array = _parse_array(reader)
    tag = reader.u8("Options tag")
    if require_tag and tag != TAG_OPTIONS:
        raise OptionsParseError(f"Options tag {tag:#04x} != {TAG_OPTIONS:#04x}")
    direct = bytes(reader.take(OPTIONS_DIRECT_END - OPTIONS_DIRECT_OFFSET, "Options direct image"))
    selected = _parse_option(reader, None, "Options.opt")
    raw = bytes(reader.data[offset:reader.pos])
    return OptionsSection(
        offset,
        reader.pos,
        array,
        tag,
        struct.unpack("<23i", direct),
        selected,
        raw,
        hashlib.sha256(raw).hexdigest(),
        layout.sha256,
    )


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument("--offset", required=True, type=lambda text: int(text, 0))
    args = parser.parse_args(argv)
    section = parse_options_section(_load(args.file), args.offset)
    print(
        f"{args.file}: Options {section.offset:#x}..{section.end:#x} "
        f"({section.size} bytes) sha256={section.sha256}\n"
        f"  array length={section.array.length}, tag={section.tag:#04x} "
        f"(StringTable[{TAG_STRING_TABLE_INDEX}])\n"
        f"  next owner begins at {section.end:#x}: CommandManager::walk_data"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
