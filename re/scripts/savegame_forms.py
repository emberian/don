#!/usr/bin/env python3
"""Parse the caller Forms tag and exact ``ObjectArray<Form>`` save image.

This exclusive helper starts at the Forms walk-test emitted by
``WalkDataGame::walk_data``, follows the concrete value-array history and every
full Form row, and stops before ``PtrArray<Good>::walk_data``.  PE control flow
defines stream order; the matching PDB export defines field names and widths.
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


TAG_FORMS = 0x00
TAG_STRING_TABLE_INDEX = 2690
FORM_TAG_STRING_TABLE_INDEX = 2694
MAX_ARRAY_LENGTH = 1 << 20
MAX_STRING_CODE_UNITS = 0xFFFF
FORM_DATA_START = 0x28
FORM_DATA_END = 0xE90
FORM_DATA_SIZE = FORM_DATA_END - FORM_DATA_START

DEFAULT_SCHEMA_PATH = (
    pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"
)


class FormsParseError(ValueError):
    """The stream or PDB layout contradicts the recovered Forms traversal."""


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
class FormField:
    name: str
    pdb_offset: int
    type_name: str
    offset: int
    end: int
    values: tuple[int, ...]


@dataclasses.dataclass(frozen=True)
class FormRow:
    index: int
    offset: int
    end: int
    tag: int
    name: WideStringImage
    desc: WideStringImage
    data_offset: int
    fields: tuple[FormField, ...]
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class FormsSection:
    offset: int
    end: int
    tag: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    rows: tuple[FormRow, ...]
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
    fields: tuple[_FieldLayout, ...]
    sha256: str


_EXPECTED_OBJECT_ARRAY = (
    ("length", 4, 4, "int"),
    ("size", 8, 4, "int"),
    ("increment", 12, 2, "short"),
    ("list", 16, 4, "Form*"),
    ("flags", 20, 1, "unsigned char"),
)

_EXPECTED_FORM = (
    ("name", 0, 20, "String"),
    ("desc", 20, 20, "String"),
    ("form", 40, 4, "int"),
    ("density", 44, 4, "int"),
    ("o", 48, 4, "int"),
    ("idx", 52, 4, "int"),
    ("who", 56, 4, "int"),
    ("num_category", 60, 72, "int[18]"),
    ("x_spacing", 132, 72, "int[18]"),
    ("y_spacing", 204, 72, "int[18]"),
    ("cat_id", 276, 512, "int[128]"),
    ("category", 788, 512, "enum FormCatIndex[128]"),
    ("to_x", 1300, 512, "Coord[128]"),
    ("to_y", 1812, 512, "Coord[128]"),
    ("off_x", 2324, 512, "Coord[128]"),
    ("off_y", 2836, 512, "Coord[128]"),
    ("wedge", 3348, 4, "int"),
    ("total", 3352, 4, "int"),
    ("guarding", 3356, 4, "int"),
    ("per", 3360, 72, "int[18]"),
    ("space", 3432, 288, "int[4][18]"),
    ("reverse", 3720, 4, "int"),
    ("across", 3724, 4, "int"),
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
        array = classes["ObjectArray<Form>"]
        form = classes["Form"]
        string = classes["String"]
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise FormsParseError(
            f"cannot load Forms PDB layout from {path}: {error}"
        ) from error

    array_fields = _field_tuples(array)
    form_fields = _field_tuples(form)
    if array.get("size") != 24 or array_fields != _EXPECTED_OBJECT_ARRAY:
        raise FormsParseError(
            f"PDB ObjectArray<Form> layout disagrees: size={array.get('size')}, "
            f"fields={array_fields!r}"
        )
    if form.get("size") != 3736 or form_fields != _EXPECTED_FORM:
        raise FormsParseError(
            f"PDB Form layout disagrees: size={form.get('size')}, "
            f"fields={form_fields!r}"
        )
    string_curr_len = tuple(
        (field["offset"], field["size"], field["type"])
        for field in string["flattened"]
        if field["name"] == "curr_len"
    )
    if string.get("size") != 20 or string_curr_len != ((8, 2, "unsigned short"),):
        raise FormsParseError(
            f"PDB String layout disagrees: size={string.get('size')}, "
            f"curr_len={string_curr_len!r}"
        )

    cursor = FORM_DATA_START
    for name, offset, size, _type_name in form_fields[2:]:
        if offset != cursor:
            raise FormsParseError(
                f"PDB Form image has a gap before {name}: {cursor:#x}..{offset:#x}"
            )
        cursor += size
    if cursor != FORM_DATA_END:
        raise FormsParseError(
            f"PDB Form image ends at {cursor:#x}, expected {FORM_DATA_END:#x}"
        )

    receipt = {
        "ObjectArray<Form>": {
            "size": array["size"],
            "flattened": array_fields,
        },
        "Form": {"size": form["size"], "flattened": form_fields},
        "String": {"size": string["size"], "curr_len": string_curr_len},
    }
    digest = hashlib.sha256(
        json.dumps(receipt, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()
    return _Layout(
        fields=tuple(
            _FieldLayout(str(name), int(offset), int(size), str(type_name))
            for name, offset, size, type_name in form_fields[2:]
        ),
        sha256=digest,
    )


def _load_layout(schema_path: pathlib.Path | None) -> _Layout:
    return _load_layout_cached(str((schema_path or DEFAULT_SCHEMA_PATH).resolve()))


class _Reader:
    def __init__(self, data: bytes | bytearray | memoryview, offset: int):
        self.data = memoryview(data)
        if offset < 0 or offset > len(self.data):
            raise FormsParseError(
                f"offset {offset:#x} is outside {len(self.data):#x}-byte stream"
            )
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise FormsParseError(
                f"{what} range [{self.pos:#x},{end:#x}) exceeds "
                f"{len(self.data):#x}-byte stream"
            )
        start = self.pos
        self.pos = end
        return self.data[start:end]

    def u8(self, what: str) -> int:
        return self.take(1, what)[0]

    def i16(self, what: str) -> int:
        return struct.unpack("<h", self.take(2, what))[0]

    def u32(self, what: str) -> int:
        return struct.unpack("<I", self.take(4, what))[0]

    def i32(self, what: str) -> int:
        return struct.unpack("<i", self.take(4, what))[0]


def _parse_string(reader: _Reader, name: str) -> WideStringImage:
    start = reader.pos
    length = reader.u32(f"Form.{name}.length")
    if length > MAX_STRING_CODE_UNITS:
        raise FormsParseError(
            f"Form.{name} length {length} exceeds writer's unsigned-short curr_len"
        )
    payload = reader.take(length * 2, f"Form.{name}.UTF-16LE")
    units = struct.unpack(f"<{length}H", payload) if length else ()
    return WideStringImage(
        name=name,
        offset=start,
        end=reader.pos,
        length=length,
        code_units=units,
        sha256=hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
    )


def _decode_field(reader: _Reader, field: _FieldLayout) -> FormField:
    start = reader.pos
    raw = reader.take(field.size, f"Form.{field.name}")
    if field.size % 4:
        raise FormsParseError(f"PDB Form field {field.name} is not word-aligned")
    count = field.size // 4
    values = struct.unpack(f"<{count}i", raw)
    return FormField(
        name=field.name,
        pdb_offset=field.offset,
        type_name=field.type_name,
        offset=start,
        end=reader.pos,
        values=values,
    )


def _parse_row(reader: _Reader, index: int, layout: _Layout) -> FormRow:
    start = reader.pos
    # SaveGame/LoadGame walk_test(StringTable[2694]) emits this byte.  The fresh
    # specimen has no rows, so preserve the value rather than inventing it.
    tag = reader.u8(f"Forms[{index}] tag")
    name = _parse_string(reader, "name")
    desc = _parse_string(reader, "desc")
    data_offset = reader.pos
    fields = tuple(_decode_field(reader, field) for field in layout.fields)
    return FormRow(
        index=index,
        offset=start,
        end=reader.pos,
        tag=tag,
        name=name,
        desc=desc,
        data_offset=data_offset,
        fields=fields,
        sha256=hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
    )


def parse_forms_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tag: bool = True,
    schema_path: pathlib.Path | None = None,
) -> FormsSection:
    """Parse the exact caller tag plus ObjectArray<Form> at ``offset``.

    The returned ``end`` is the first byte owned by the following
    ``PtrArray<Good>::walk_data`` call.
    """

    layout = _load_layout(schema_path)
    reader = _Reader(data, offset)
    tag = reader.u8("Forms tag")
    if require_tag and tag != TAG_FORMS:
        raise FormsParseError(
            f"Forms tag {tag:#04x} != {TAG_FORMS:#04x} at {offset:#x}"
        )
    length = reader.i32("Forms.length")
    if length < 0 or length > MAX_ARRAY_LENGTH:
        raise FormsParseError(f"Forms has invalid length {length} at {offset + 1:#x}")
    if length == 0:
        return FormsSection(
            offset=offset,
            end=reader.pos,
            tag=tag,
            length=0,
            capacity=None,
            increment=None,
            flags=None,
            rows=(),
            sha256=hashlib.sha256(reader.data[offset : reader.pos]).hexdigest(),
            layout_sha256=layout.sha256,
        )

    capacity = reader.i32("Forms.capacity")
    increment = reader.i16("Forms.increment")
    flags = reader.u8("Forms.flags")
    if capacity < length or capacity > MAX_ARRAY_LENGTH:
        raise FormsParseError(
            f"Forms has invalid history length={length}, capacity={capacity}"
        )
    if flags & 0x40:
        raise FormsParseError(
            f"Forms flags {flags:#04x} retain writer-cleared bit 0x40"
        )
    rows = tuple(_parse_row(reader, index, layout) for index in range(length))
    return FormsSection(
        offset=offset,
        end=reader.pos,
        tag=tag,
        length=length,
        capacity=capacity,
        increment=increment,
        flags=flags,
        rows=rows,
        sha256=hashlib.sha256(reader.data[offset : reader.pos]).hexdigest(),
        layout_sha256=layout.sha256,
    )


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def _jsonable(value: object) -> object:
    if dataclasses.is_dataclass(value):
        result: dict[str, object] = {}
        for field in dataclasses.fields(value):
            field_value = getattr(value, field.name)
            if field.name in ("offset", "end", "data_offset", "pdb_offset"):
                result[field.name] = f"0x{field_value:x}"
            else:
                result[field.name] = _jsonable(field_value)
        if isinstance(value, (WideStringImage, FormRow, FormsSection)):
            result["size"] = value.size
        return result
    if isinstance(value, tuple):
        return [_jsonable(item) for item in value]
    return value


def _summary(section: FormsSection, path: pathlib.Path) -> str:
    lines = [
        f"{path}: Forms {section.offset:#x}..{section.end:#x} "
        f"({section.size} bytes) sha256={section.sha256}",
        f"  tag={section.tag:#04x} (StringTable[{TAG_STRING_TABLE_INDEX}]); "
        f"PDB-layout sha256={section.layout_sha256}",
    ]
    if section.length == 0:
        lines.append("  ObjectArray<Form>: empty")
    else:
        lines.append(
            f"  ObjectArray<Form>: length={section.length} "
            f"capacity={section.capacity} increment={section.increment} "
            f"flags={section.flags:#04x}"
        )
        lines.extend(
            f"  row {row.index}: tag={row.tag:#04x} "
            f"name_units={row.name.length} desc_units={row.desc.length} "
            f"({row.size} bytes)"
            for row in section.rows
        )
    lines.append(
        f"  next owner begins at {section.end:#x} "
        "(PtrArray<Good>::walk_data 0x0045cce0)"
    )
    return "\n".join(lines)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument(
        "--offset",
        required=True,
        type=lambda text: int(text, 0),
        help="decompressed stream offset of the caller Forms tag",
    )
    parser.add_argument("--schema", type=pathlib.Path, default=DEFAULT_SCHEMA_PATH)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    section = parse_forms_section(
        _load(args.file), args.offset, schema_path=args.schema
    )
    if args.json:
        print(json.dumps(_jsonable(section), indent=2))
    else:
        print(_summary(section, args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
