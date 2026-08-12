#!/usr/bin/env python3
"""Parse the exact retail ``Lands::walk_data`` generic-save image.

The helper starts at the caller's Lands tag, preserves the complete
``ObjectArray<Land>`` allocation history, decodes every 264-byte Land POD
projection and both variable UTF-16 strings, and stops before
``LeaderOptions::walk_data``. PE control flow defines stream order; the
matched PDB export defines names, offsets, widths, and excluded object state.
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


TAG_LANDS = 0x00
TAG_STRING_TABLE_INDEX = 4606
LAND_TAG_STRING_TABLE_INDEX = 4604
MAX_ARRAY_LENGTH = 1 << 20
MAX_STRING_CODE_UNITS = 0xFFFF
DEFAULT_SCHEMA_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"


class LandsParseError(ValueError):
    """The stream or PDB layout contradicts the recovered Lands traversal."""


@dataclasses.dataclass(frozen=True)
class LandField:
    name: str
    pdb_offset: int
    type_name: str
    offset: int
    end: int
    values: tuple[int, ...]

    @property
    def size(self) -> int:
        return self.end - self.offset


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
class LandRow:
    index: int
    offset: int
    end: int
    row_tag: int
    pod_offset: int
    pod_end: int
    fields: tuple[LandField, ...]
    name: WideStringImage
    key: WideStringImage
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class LandsSection:
    offset: int
    end: int
    tag: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    rows: tuple[LandRow, ...]
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


_EXPECTED_LANDS = (("list", 0, 24, "ObjectArray<Land>"),)
_EXPECTED_OBJECT_ARRAY = (
    ("length", 4, 4, "int"),
    ("size", 8, 4, "int"),
    ("increment", 12, 2, "short"),
    ("list", 16, 4, "Land*"),
    ("flags", 20, 1, "unsigned char"),
)
_EXPECTED_LAND_DATA = (
    ("land", 0, 4, "int"),
    ("make", 4, 16, "int[4]"),
    ("num_make", 20, 16, "int[4]"),
    ("special", 36, 24, "int[6]"),
    ("special_sum", 60, 4, "int"),
    ("river_mask", 64, 4, "int"),
    ("river_bed", 68, 4, "int"),
    ("river_cost", 72, 4, "int"),
    ("num_rare", 76, 4, "int"),
    ("rare", 80, 176, "int[44]"),
    ("move_rate", 256, 4, "int"),
    ("combat_bonus", 260, 4, "int"),
    ("name", 264, 20, "String"),
    ("key", 284, 20, "String"),
)
_EXPECTED_STRING = (
    ("const_string", 0, 4, "const wchar_t*"),
    ("data", 0, 4, "StringGuts*"),
    ("const_len", 4, 2, "unsigned short"),
    ("offset", 6, 2, "unsigned short"),
    ("curr_len", 8, 2, "unsigned short"),
    ("flags", 10, 1, "unsigned char"),
    ("module_id", 11, 1, "unsigned char"),
    ("hash_value", 12, 4, "unsigned long"),
    ("hash_value_insensitive", 16, 4, "unsigned long"),
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
            "Lands": classes["Lands"],
            "LandsData": classes["LandsData"],
            "ObjectArray<Land>": classes["ObjectArray<Land>"],
            "Land": classes["Land"],
            "LandData": classes["LandData"],
            "String": classes["String"],
        }
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise LandsParseError(f"cannot load Lands PDB layout from {path}: {error}") from error

    expected = {
        "Lands": (32, _EXPECTED_LANDS),
        "LandsData": (24, _EXPECTED_LANDS),
        "ObjectArray<Land>": (24, _EXPECTED_OBJECT_ARRAY),
        "Land": (312, _EXPECTED_LAND_DATA),
        "LandData": (304, _EXPECTED_LAND_DATA),
        "String": (20, _EXPECTED_STRING),
    }
    receipt: dict[str, object] = {}
    for name, (size, fields) in expected.items():
        record = records[name]
        actual = _field_tuples(record)
        if record.get("size") != size or actual != fields:
            raise LandsParseError(
                f"PDB {name} layout disagrees: size={record.get('size')}, fields={actual!r}"
            )
        receipt[name] = {"size": record["size"], "flattened": actual}
    return _Layout(
        pod_fields=tuple(
            _FieldLayout(str(name), int(offset), int(size), str(type_name))
            for name, offset, size, type_name in _EXPECTED_LAND_DATA[:12]
        ),
        sha256=hashlib.sha256(
            json.dumps(receipt, sort_keys=True, separators=(",", ":")).encode()
        ).hexdigest(),
    )


def _load_layout(schema_path: pathlib.Path | None) -> _Layout:
    return _load_layout_cached(str((schema_path or DEFAULT_SCHEMA_PATH).resolve()))


class _Reader:
    def __init__(self, data: bytes | bytearray | memoryview, offset: int):
        self.data = memoryview(data)
        if offset < 0 or offset > len(self.data):
            raise LandsParseError(
                f"offset {offset:#x} is outside {len(self.data):#x}-byte stream"
            )
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise LandsParseError(
                f"{what} range [{self.pos:#x},{end:#x}) exceeds {len(self.data):#x}-byte stream"
            )
        start = self.pos
        self.pos = end
        return self.data[start:end]

    def u8(self, what: str) -> int:
        return self.take(1, what)[0]

    def u32(self, what: str) -> int:
        return struct.unpack("<I", self.take(4, what))[0]

    def i16(self, what: str) -> int:
        return struct.unpack("<h", self.take(2, what))[0]

    def i32(self, what: str) -> int:
        return struct.unpack("<i", self.take(4, what))[0]


def _parse_field(reader: _Reader, field: _FieldLayout, index: int) -> LandField:
    start = reader.pos
    raw = reader.take(field.size, f"Lands[{index}].{field.name}")
    if field.type_name in ("int", "Coord"):
        values = (struct.unpack("<i", raw)[0],)
    elif field.type_name.startswith("int["):
        count = field.size // 4
        values = struct.unpack(f"<{count}i", raw)
    else:  # Protected by the exact PDB receipt.
        raise LandsParseError(f"unsupported PDB Land field type {field.type_name}")
    return LandField(
        field.name,
        field.offset,
        field.type_name,
        start,
        reader.pos,
        tuple(values),
    )


def _parse_string(reader: _Reader, owner: str) -> WideStringImage:
    start = reader.pos
    length = reader.u32(f"{owner}.length")
    if length > MAX_STRING_CODE_UNITS:
        raise LandsParseError(
            f"{owner} length {length} exceeds String's unsigned-short curr_len"
        )
    payload = reader.take(length * 2, f"{owner}.UTF-16LE")
    code_units = struct.unpack(f"<{length}H", payload) if length else ()
    return WideStringImage(
        owner.rsplit(".", 1)[-1],
        start,
        reader.pos,
        length,
        tuple(code_units),
        hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
    )


def _parse_row(reader: _Reader, layout: _Layout, index: int) -> LandRow:
    start = reader.pos
    # Preserve the row tag: the fresh array is empty and provides no numeric
    # save value for StringTable[4604].
    row_tag = reader.u8(f"Lands[{index}].Land tag")
    pod_offset = reader.pos
    fields = tuple(_parse_field(reader, field, index) for field in layout.pod_fields)
    pod_end = reader.pos
    if pod_end - pod_offset != 264:
        raise LandsParseError(f"PDB-derived Land POD width is {pod_end - pod_offset}, not 264")
    name = _parse_string(reader, f"Lands[{index}].name")
    key = _parse_string(reader, f"Lands[{index}].key")
    return LandRow(
        index,
        start,
        reader.pos,
        row_tag,
        pod_offset,
        pod_end,
        fields,
        name,
        key,
        hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
    )


def parse_lands_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tag: bool = True,
    schema_path: pathlib.Path | None = None,
) -> LandsSection:
    """Parse one complete retail ``Lands::walk_data`` at ``offset``.

    The returned ``end`` is the first byte owned by
    ``LeaderOptions::walk_data``.
    """
    layout = _load_layout(schema_path)
    reader = _Reader(data, offset)
    tag = reader.u8("Lands tag")
    if require_tag and tag != TAG_LANDS:
        raise LandsParseError(
            f"Lands tag {tag:#04x} != {TAG_LANDS:#04x} at {offset:#x}"
        )
    length = reader.i32("Lands.list.length")
    if length < 0 or length > MAX_ARRAY_LENGTH:
        raise LandsParseError(f"Lands list has invalid length {length}")
    if length == 0:
        return LandsSection(
            offset,
            reader.pos,
            tag,
            0,
            None,
            None,
            None,
            (),
            hashlib.sha256(reader.data[offset : reader.pos]).hexdigest(),
            layout.sha256,
        )

    capacity = reader.i32("Lands.list.capacity")
    increment = reader.i16("Lands.list.increment")
    flags = reader.u8("Lands.list.flags")
    if capacity < length or capacity > MAX_ARRAY_LENGTH:
        raise LandsParseError(
            f"Lands list has invalid history length={length}, capacity={capacity}"
        )
    if flags & 0x40:
        raise LandsParseError(
            f"Lands list flags {flags:#04x} retain writer-cleared bit 0x40"
        )
    rows = tuple(_parse_row(reader, layout, index) for index in range(length))
    return LandsSection(
        offset,
        reader.pos,
        tag,
        length,
        capacity,
        increment,
        flags,
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
            if field.name.endswith("offset") or field.name in ("end", "pod_end"):
                result[field.name] = f"0x{field_value:x}"
            else:
                result[field.name] = _jsonable(field_value)
        if hasattr(value, "size"):
            result["size"] = getattr(value, "size")
        return result
    if isinstance(value, tuple):
        return [_jsonable(item) for item in value]
    return value


def _summary(section: LandsSection, path: pathlib.Path) -> str:
    strings = sum(row.name.length + row.key.length for row in section.rows)
    return "\n".join(
        (
            f"{path}: Lands {section.offset:#x}..{section.end:#x} "
            f"({section.size} bytes) sha256={section.sha256}",
            f"  PDB-layout sha256={section.layout_sha256}",
            f"  lands={section.length} UTF-16-code-units={strings}",
            f"  next owner begins at {section.end:#x} (LeaderOptions::walk_data)",
        )
    )


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument("--offset", required=True, type=lambda text: int(text, 0))
    parser.add_argument("--schema", type=pathlib.Path, default=DEFAULT_SCHEMA_PATH)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    section = parse_lands_section(_load(args.file), args.offset, schema_path=args.schema)
    print(json.dumps(_jsonable(section), indent=2) if args.json else _summary(section, args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
