#!/usr/bin/env python3
"""Parse the exact retail ``OptionInfo::walk_data`` generic-save image.

The helper consumes the OptionInfo tag and all 331 positional OptionData row
projections. Each row preserves its tag plus the exact variable UTF-16
``name`` and ``desc`` strings walked by retail, while excluding the unwalked
English name and texture metadata. It stops before the caller's next direct
108-byte block.
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


TAG_OPTION_INFO = 0x00
TAG_STRING_TABLE_INDEX = 5073
ROW_TAG_STRING_TABLE_INDEX = 5072
OPTION_COUNT = 331
MAX_STRING_CODE_UNITS = 0xFFFF
DEFAULT_SCHEMA_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"


class OptionInfoParseError(ValueError):
    """The stream or PDB layout contradicts the OptionInfo traversal."""


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
class OptionDataRow:
    index: int
    offset: int
    end: int
    row_tag: int
    name: WideStringImage
    desc: WideStringImage
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class OptionInfoSection:
    offset: int
    end: int
    tag: int
    rows: tuple[OptionDataRow, ...]
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class _Layout:
    sha256: str


_EXPECTED_OPTION_INFO = (("list", 0, 22508, "OptionData[331]"),)
_EXPECTED_OPTION_DATA = (
    ("english_name", 0, 20, "String"),
    ("name", 20, 20, "String"),
    ("desc", 40, 20, "String"),
    ("tex_x", 60, 2, "short"),
    ("tex_y", 62, 2, "short"),
    ("tex_col", 64, 1, "char"),
    ("tex_row", 65, 1, "char"),
    ("tex_clip", 66, 1, "char"),
    ("tex_id", 67, 1, "char"),
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
            "OptionInfo": classes["OptionInfo"],
            "OptionData": classes["OptionData"],
            "String": classes["String"],
        }
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise OptionInfoParseError(
            f"cannot load OptionInfo PDB layout from {path}: {error}"
        ) from error
    expected = {
        "OptionInfo": (22508, _EXPECTED_OPTION_INFO),
        "OptionData": (68, _EXPECTED_OPTION_DATA),
        "String": (20, _EXPECTED_STRING),
    }
    receipt: dict[str, object] = {}
    for name, (size, fields) in expected.items():
        record = records[name]
        actual = _field_tuples(record)
        if record.get("size") != size or actual != fields:
            raise OptionInfoParseError(
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
            raise OptionInfoParseError(
                f"offset {offset:#x} is outside {len(self.data):#x}-byte stream"
            )
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise OptionInfoParseError(
                f"{what} range [{self.pos:#x},{end:#x}) exceeds {len(self.data):#x}-byte stream"
            )
        start = self.pos
        self.pos = end
        return self.data[start:end]

    def u8(self, what: str) -> int:
        return self.take(1, what)[0]

    def u32(self, what: str) -> int:
        return struct.unpack("<I", self.take(4, what))[0]


def _parse_string(reader: _Reader, owner: str) -> WideStringImage:
    start = reader.pos
    length = reader.u32(f"{owner}.length")
    if length > MAX_STRING_CODE_UNITS:
        raise OptionInfoParseError(
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


def _parse_row(reader: _Reader, index: int) -> OptionDataRow:
    start = reader.pos
    owner = f"OptionInfo[{index}]"
    row_tag = reader.u8(f"{owner}.OptionData tag")
    name = _parse_string(reader, f"{owner}.name")
    desc = _parse_string(reader, f"{owner}.desc")
    return OptionDataRow(
        index,
        start,
        reader.pos,
        row_tag,
        name,
        desc,
        hashlib.sha256(reader.data[start : reader.pos]).hexdigest(),
    )


def parse_option_info_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tag: bool = True,
    schema_path: pathlib.Path | None = None,
) -> OptionInfoSection:
    """Parse one complete retail ``OptionInfo::walk_data`` at ``offset``.

    The returned ``end`` is the first byte of the caller's direct 108-byte
    block at PE globals ``0x00e85e98..0x00e85f04``.
    """
    layout = _load_layout(schema_path)
    reader = _Reader(data, offset)
    tag = reader.u8("OptionInfo tag")
    if require_tag and tag != TAG_OPTION_INFO:
        raise OptionInfoParseError(
            f"OptionInfo tag {tag:#04x} != {TAG_OPTION_INFO:#04x} at {offset:#x}"
        )
    rows = tuple(_parse_row(reader, index) for index in range(OPTION_COUNT))
    return OptionInfoSection(
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


def _summary(section: OptionInfoSection, path: pathlib.Path) -> str:
    code_units = sum(row.name.length + row.desc.length for row in section.rows)
    return "\n".join(
        (
            f"{path}: OptionInfo {section.offset:#x}..{section.end:#x} "
            f"({section.size} bytes) sha256={section.sha256}",
            f"  PDB-layout sha256={section.layout_sha256}",
            f"  rows=331 UTF-16-code-units={code_units}",
            f"  next owner begins at {section.end:#x} (caller direct 108-byte block)",
        )
    )


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument("--offset", required=True, type=lambda text: int(text, 0))
    parser.add_argument("--schema", type=pathlib.Path, default=DEFAULT_SCHEMA_PATH)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    section = parse_option_info_section(_load(args.file), args.offset, schema_path=args.schema)
    print(json.dumps(_jsonable(section), indent=2) if args.json else _summary(section, args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
