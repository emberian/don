#!/usr/bin/env python3
"""Parse the exact retail Groups tail generic-save image.

The helper starts at the caller's Groups tag after ``Array<Group>``, decodes
the exact eight ``last_group`` integers and ``proc_group`` scalar, excludes
the intervening runtime pointer, and stops before ``Objects::walk_data``.
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


TAG_GROUPS_TAIL = 0x00
TAG_STRING_TABLE_INDEX = 2920
LAST_GROUP_COUNT = 8
DEFAULT_SCHEMA_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"


class GroupsTailParseError(ValueError):
    """The stream or PDB layout contradicts the recovered Groups tail."""


@dataclasses.dataclass(frozen=True)
class GroupsTailSection:
    offset: int
    end: int
    tag: int
    last_group_offset: int
    last_group: tuple[int, ...]
    proc_group_offset: int
    proc_group: int
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class _Layout:
    sha256: str


_EXPECTED_GROUPS = (
    ("list", 0, 28, "Array<Group>"),
    ("last_group", 28, 32, "int[8]"),
    ("const_last_group", 60, 4, "const int*"),
    ("proc_group", 64, 4, "int"),
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
            "Groups": classes["Groups"],
            "GroupsData": classes["GroupsData"],
            "GroupsOut": classes["GroupsOut"],
        }
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise GroupsTailParseError(
            f"cannot load Groups PDB layout from {path}: {error}"
        ) from error
    expected_sizes = {"Groups": 76, "GroupsData": 68, "GroupsOut": 72}
    receipt: dict[str, object] = {}
    for name, record in records.items():
        actual = _field_tuples(record)
        if record.get("size") != expected_sizes[name] or actual != _EXPECTED_GROUPS:
            raise GroupsTailParseError(
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
            raise GroupsTailParseError(
                f"offset {offset:#x} is outside {len(self.data):#x}-byte stream"
            )
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data):
            raise GroupsTailParseError(
                f"{what} range [{self.pos:#x},{end:#x}) exceeds {len(self.data):#x}-byte stream"
            )
        start = self.pos
        self.pos = end
        return self.data[start:end]

    def u8(self, what: str) -> int:
        return self.take(1, what)[0]

    def i32(self, what: str) -> int:
        return struct.unpack("<i", self.take(4, what))[0]


def parse_groups_tail_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tag: bool = True,
    schema_path: pathlib.Path | None = None,
) -> GroupsTailSection:
    """Parse the Groups fields after ``Array<Group>`` at ``offset``.

    The returned ``end`` is the first byte owned by ``Objects::walk_data``.
    """
    layout = _load_layout(schema_path)
    reader = _Reader(data, offset)
    tag = reader.u8("Groups tail tag")
    if require_tag and tag != TAG_GROUPS_TAIL:
        raise GroupsTailParseError(
            f"Groups tail tag {tag:#04x} != {TAG_GROUPS_TAIL:#04x} at {offset:#x}"
        )
    last_group_offset = reader.pos
    last_group = tuple(
        reader.i32(f"Groups.last_group[{index}]")
        for index in range(LAST_GROUP_COUNT)
    )
    proc_group_offset = reader.pos
    proc_group = reader.i32("Groups.proc_group")
    return GroupsTailSection(
        offset,
        reader.pos,
        tag,
        last_group_offset,
        last_group,
        proc_group_offset,
        proc_group,
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


def _summary(section: GroupsTailSection, path: pathlib.Path) -> str:
    return "\n".join(
        (
            f"{path}: Groups tail {section.offset:#x}..{section.end:#x} "
            f"({section.size} bytes) sha256={section.sha256}",
            f"  PDB-layout sha256={section.layout_sha256}",
            f"  last_group={section.last_group} proc_group={section.proc_group}",
            f"  next owner begins at {section.end:#x} (Objects::walk_data)",
        )
    )


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument("--offset", required=True, type=lambda text: int(text, 0))
    parser.add_argument("--schema", type=pathlib.Path, default=DEFAULT_SCHEMA_PATH)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    section = parse_groups_tail_section(_load(args.file), args.offset, schema_path=args.schema)
    print(json.dumps(_jsonable(section), indent=2) if args.json else _summary(section, args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
