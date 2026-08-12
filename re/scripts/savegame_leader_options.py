#!/usr/bin/env python3
"""Parse the exact retail ``LeaderOptions::walk_data`` generic-save image.

The helper consumes the LeaderOptions tag and all ten positional
``LeaderOption`` rows, preserving each row tag and the exact variable
``BitMask<32>`` projection, then stops before ``OptionInfo::walk_data``.
PE control flow defines stream order; the matched PDB export defines field
names, offsets, widths, and excluded runtime state.
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


TAG_LEADER_OPTIONS = 0x00
TAG_STRING_TABLE_INDEX = 4614
ROW_TAG_STRING_TABLE_INDEX = 4613
LEADER_OPTION_COUNT = 10
BIT_CAPACITY = 32
DEFAULT_SCHEMA_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"


class LeaderOptionsParseError(ValueError):
    """The stream or PDB layout contradicts the LeaderOptions traversal."""


@dataclasses.dataclass(frozen=True)
class LeaderOptionRow:
    index: int
    offset: int
    end: int
    row_tag: int
    who: int
    peasants: int
    peasants_wait: int
    buildings: int
    bit_count: int
    mask_size: int
    mask_offset: int
    mask: tuple[int, ...]
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class LeaderOptionsSection:
    offset: int
    end: int
    tag: int
    rows: tuple[LeaderOptionRow, ...]
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class _Layout:
    sha256: str


_EXPECTED_OPTIONS = (("list", 0, 320, "LeaderOption[10]"),)
_EXPECTED_OPTION = (
    ("who", 0, 4, "int"),
    ("peasants", 4, 4, "int"),
    ("peasants_wait", 8, 4, "int"),
    ("buildings", 12, 4, "int"),
    ("flags", 16, 16, "BitMask<32>"),
)
_EXPECTED_BIT_MASK = (
    ("bits", 0, 4, "int"),
    ("size", 4, 4, "int"),
    ("flags", 8, 4, "int"),
    ("ptr", 12, 4, "unsigned char[4]"),
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
            "LeaderOptions": classes["LeaderOptions"],
            "LeaderOption": classes["LeaderOption"],
            "LeaderOptionData": classes["LeaderOptionData"],
            "BitMask<32>": classes["BitMask<32>"],
        }
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise LeaderOptionsParseError(
            f"cannot load LeaderOptions PDB layout from {path}: {error}"
        ) from error
    expected = {
        "LeaderOptions": (320, _EXPECTED_OPTIONS),
        "LeaderOption": (32, _EXPECTED_OPTION),
        "LeaderOptionData": (32, _EXPECTED_OPTION),
        "BitMask<32>": (16, _EXPECTED_BIT_MASK),
    }
    receipt: dict[str, object] = {}
    for name, (size, fields) in expected.items():
        record = records[name]
        actual = _field_tuples(record)
        if record.get("size") != size or actual != fields:
            raise LeaderOptionsParseError(
                f"PDB {name} layout disagrees: size={record.get('size')}, fields={actual!r}"
            )
        receipt[name] = {"size": record["size"], "flattened": actual}
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
            raise LeaderOptionsParseError(
                f"offset {offset:#x} is outside {len(self.data):#x}-byte stream"
            )
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise LeaderOptionsParseError(
                f"{what} range [{self.pos:#x},{end:#x}) exceeds {len(self.data):#x}-byte stream"
            )
        start = self.pos
        self.pos = end
        return self.data[start:end]

    def u8(self, what: str) -> int:
        return self.take(1, what)[0]

    def i32(self, what: str) -> int:
        return struct.unpack("<i", self.take(4, what))[0]


def _parse_row(reader: _Reader, index: int) -> LeaderOptionRow:
    start = reader.pos
    name = f"LeaderOptions[{index}]"
    # The fresh specimen supplies the saved byte (zero), but row tags remain
    # receipts rather than parser delimiters so historical streams round-trip.
    row_tag = reader.u8(f"{name}.tag")
    who = reader.i32(f"{name}.who")
    peasants = reader.i32(f"{name}.peasants")
    peasants_wait = reader.i32(f"{name}.peasants_wait")
    buildings = reader.i32(f"{name}.buildings")
    bit_count = reader.i32(f"{name}.flags.bits")
    mask_size = reader.i32(f"{name}.flags.size")
    if bit_count < 0 or bit_count > BIT_CAPACITY:
        raise LeaderOptionsParseError(
            f"{name} BitMask<32> has invalid bit count {bit_count}"
        )
    exact_size = (bit_count + 7) // 8
    if mask_size != exact_size:
        raise LeaderOptionsParseError(
            f"{name} BitMask<32> size {mask_size} != ceil({bit_count}/8)={exact_size}"
        )
    mask_offset = reader.pos
    mask = tuple(reader.take(mask_size, f"{name}.flags.ptr"))
    return LeaderOptionRow(
        index,
        start,
        reader.pos,
        row_tag,
        who,
        peasants,
        peasants_wait,
        buildings,
        bit_count,
        mask_size,
        mask_offset,
        mask,
        hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
    )


def parse_leader_options_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tag: bool = True,
    schema_path: pathlib.Path | None = None,
) -> LeaderOptionsSection:
    """Parse one complete retail ``LeaderOptions::walk_data`` at ``offset``.

    The returned ``end`` is the first byte owned by ``OptionInfo::walk_data``.
    """
    layout = _load_layout(schema_path)
    reader = _Reader(data, offset)
    tag = reader.u8("LeaderOptions tag")
    if require_tag and tag != TAG_LEADER_OPTIONS:
        raise LeaderOptionsParseError(
            f"LeaderOptions tag {tag:#04x} != {TAG_LEADER_OPTIONS:#04x} at {offset:#x}"
        )
    rows = tuple(_parse_row(reader, index) for index in range(LEADER_OPTION_COUNT))
    return LeaderOptionsSection(
        offset,
        reader.pos,
        tag,
        rows,
        hashlib.sha256(reader.data[offset : reader.pos]).hexdigest(),
        layout.sha256,
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
                result[field.name] = f"0x{field_value:x}"
            else:
                result[field.name] = _jsonable(field_value)
        if hasattr(value, "size"):
            result["size"] = getattr(value, "size")
        return result
    if isinstance(value, tuple):
        return [_jsonable(item) for item in value]
    return value


def _summary(section: LeaderOptionsSection, path: pathlib.Path) -> str:
    mask_bytes = sum(row.mask_size for row in section.rows)
    return "\n".join(
        (
            f"{path}: LeaderOptions {section.offset:#x}..{section.end:#x} "
            f"({section.size} bytes) sha256={section.sha256}",
            f"  PDB-layout sha256={section.layout_sha256}",
            f"  rows=10 mask-bytes={mask_bytes}",
            f"  next owner begins at {section.end:#x} (OptionInfo::walk_data)",
        )
    )


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument("--offset", required=True, type=lambda text: int(text, 0))
    parser.add_argument("--schema", type=pathlib.Path, default=DEFAULT_SCHEMA_PATH)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    section = parse_leader_options_section(
        _load(args.file), args.offset, schema_path=args.schema
    )
    print(json.dumps(_jsonable(section), indent=2) if args.json else _summary(section, args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
