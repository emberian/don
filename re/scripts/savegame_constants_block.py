#!/usr/bin/env python3
"""Decode the direct ``Constants[+0x000..+0xd40)`` save-stream block.

``WalkDataGame::walk_data`` submits this range directly to
``DataWalk::walk_function`` at ``0x005a2a68``; there is no nested Constants
walker or per-field framing.  The matching PDB proves that the range is exactly
721 contiguous named ``int``/``int[N]`` fields (848 little-endian i32 words),
with no gaps, padding, or overlaps.

This exclusive helper loads that compiler-emitted layout from the repository's
``schema/pdb-types.json`` and stops before the caller's separate duplicate walk
of ``Constants[+0x804..+0x808)``.  No specimen bytes are embedded or written.
"""

from __future__ import annotations

import argparse
import dataclasses
import functools
import gzip
import hashlib
import json
import pathlib
import re
import struct
from typing import Sequence


CONSTANTS_BLOCK_SIZE = 0xD40
CONSTANTS_CLASS_SIZE = 0xD68
CONSTANTS_FIELD_COUNT = 721
CONSTANTS_WORD_COUNT = CONSTANTS_BLOCK_SIZE // 4
NEXT_DUPLICATE_FIELD_OFFSET = 0x804
NEXT_DUPLICATE_FIELD_NAME = "mongol_three_mil_cavalry"
DEFAULT_LAYOUT_PATH = (
    pathlib.Path(__file__).resolve().parents[2] / "schema" / "pdb-types.json"
)

_INT_TYPE = re.compile(r"^int(?:\[(\d+)\])?$")


class ConstantsParseError(ValueError):
    """The stream or PDB layout contradicts the recovered direct block."""


@dataclasses.dataclass(frozen=True)
class ConstantsField:
    name: str
    relative_offset: int
    stream_offset: int
    type_name: str
    values: tuple[int, ...]

    @property
    def size(self) -> int:
        return len(self.values) * 4


@dataclasses.dataclass(frozen=True)
class ConstantsBlock:
    offset: int
    end: int
    fields: tuple[ConstantsField, ...]
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset

    @property
    def words(self) -> tuple[int, ...]:
        return tuple(value for field in self.fields for value in field.values)


@dataclasses.dataclass(frozen=True)
class _FieldLayout:
    name: str
    offset: int
    type_name: str
    word_count: int


@dataclasses.dataclass(frozen=True)
class _ConstantsLayout:
    fields: tuple[_FieldLayout, ...]
    schema_sha256: str


def _word_count(type_name: str, size: int, name: str) -> int:
    match = _INT_TYPE.fullmatch(type_name)
    if match is None:
        raise ConstantsParseError(
            f"PDB Constants field {name!r} has unsupported type {type_name!r}"
        )
    count = int(match.group(1) or "1")
    if size != count * 4:
        raise ConstantsParseError(
            f"PDB Constants field {name!r} type {type_name} has size {size}, "
            f"expected {count * 4}"
        )
    return count


@functools.lru_cache(maxsize=4)
def _load_layout(path: pathlib.Path) -> _ConstantsLayout:
    try:
        raw = path.read_bytes()
        document = json.loads(raw)
        constants = document["classes"]["Constants"]
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as exc:
        raise ConstantsParseError(
            f"cannot load Constants PDB layout from {path}: {exc}"
        ) from exc

    if constants.get("size") != CONSTANTS_CLASS_SIZE:
        raise ConstantsParseError(
            f"PDB sizeof(Constants)={constants.get('size')!r}, "
            f"expected {CONSTANTS_CLASS_SIZE}"
        )
    raw_fields = constants.get("fields")
    if not isinstance(raw_fields, list):
        raise ConstantsParseError("PDB Constants fields are missing")

    prefix_fields = [field for field in raw_fields if field.get("offset", -1) < CONSTANTS_BLOCK_SIZE]
    if len(prefix_fields) != CONSTANTS_FIELD_COUNT:
        raise ConstantsParseError(
            f"PDB Constants prefix has {len(prefix_fields)} fields, "
            f"expected {CONSTANTS_FIELD_COUNT}"
        )

    layouts: list[_FieldLayout] = []
    cursor = 0
    for field in prefix_fields:
        try:
            name = field["name"]
            offset = field["offset"]
            size = field["size"]
            type_name = field["type"]
        except KeyError as exc:
            raise ConstantsParseError(f"malformed PDB Constants field: {field}") from exc
        if offset != cursor:
            relation = "overlap" if offset < cursor else "gap"
            raise ConstantsParseError(
                f"PDB Constants prefix has {relation} before {name!r}: "
                f"cursor={cursor:#x}, field={offset:#x}"
            )
        count = _word_count(type_name, size, name)
        layouts.append(_FieldLayout(name, offset, type_name, count))
        cursor += size

    if cursor != CONSTANTS_BLOCK_SIZE:
        raise ConstantsParseError(
            f"PDB Constants prefix ends at {cursor:#x}, expected {CONSTANTS_BLOCK_SIZE:#x}"
        )
    excluded = [field for field in raw_fields if field.get("offset") == CONSTANTS_BLOCK_SIZE]
    if len(excluded) != 1 or excluded[0].get("name") != "curr_element":
        raise ConstantsParseError(
            "PDB Constants first excluded field is not curr_element at +0xd40"
        )

    duplicate = [
        layout for layout in layouts if layout.offset == NEXT_DUPLICATE_FIELD_OFFSET
    ]
    if len(duplicate) != 1 or duplicate[0].name != NEXT_DUPLICATE_FIELD_NAME:
        raise ConstantsParseError(
            "PDB Constants +0x804 field does not match the caller's duplicate walk"
        )
    return _ConstantsLayout(tuple(layouts), hashlib.sha256(raw).hexdigest())


def parse_constants_block(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    layout_path: pathlib.Path = DEFAULT_LAYOUT_PATH,
) -> ConstantsBlock:
    """Decode the exact direct Constants block beginning at ``offset``.

    The returned ``end`` is the first byte of the caller's separate direct
    ``Constants[+0x804..+0x808)`` walk.
    """

    view = memoryview(data)
    if offset < 0 or offset > len(view):
        raise ConstantsParseError(f"offset {offset:#x} is outside the stream")
    end = offset + CONSTANTS_BLOCK_SIZE
    if end > len(view):
        raise ConstantsParseError(
            f"Constants block [{offset:#x},{end:#x}) exceeds "
            f"{len(view):#x}-byte stream"
        )

    layout = _load_layout(pathlib.Path(layout_path).resolve())
    fields = tuple(
        ConstantsField(
            name=field.name,
            relative_offset=field.offset,
            stream_offset=offset + field.offset,
            type_name=field.type_name,
            values=struct.unpack_from(
                f"<{field.word_count}i", view, offset + field.offset
            ),
        )
        for field in layout.fields
    )
    return ConstantsBlock(
        offset=offset,
        end=end,
        fields=fields,
        sha256=hashlib.sha256(view[offset:end]).hexdigest(),
        layout_sha256=layout.schema_sha256,
    )


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def _jsonable(value: object) -> object:
    if dataclasses.is_dataclass(value):
        result: dict[str, object] = {}
        for field in dataclasses.fields(value):
            field_value = getattr(value, field.name)
            if field.name in ("offset", "end", "relative_offset", "stream_offset"):
                result[field.name] = f"0x{field_value:x}"
            else:
                result[field.name] = _jsonable(field_value)
        if isinstance(value, (ConstantsField, ConstantsBlock)):
            result["size"] = value.size
        return result
    if isinstance(value, tuple):
        return [_jsonable(item) for item in value]
    return value


def _summary(block: ConstantsBlock, path: pathlib.Path) -> str:
    nonzero_fields = sum(any(field.values) for field in block.fields)
    nonzero_words = sum(value != 0 for value in block.words)
    return "\n".join(
        (
            f"{path}: Constants[+0x000..+0xd40) "
            f"{block.offset:#x}..{block.end:#x} ({block.size} bytes) "
            f"sha256={block.sha256}",
            f"  decoded {len(block.fields)} PDB fields / {len(block.words)} i32 words; "
            f"nonzero fields={nonzero_fields}, nonzero words={nonzero_words}",
            f"  layout schema sha256={block.layout_sha256}",
            f"  next owner begins at {block.end:#x}: "
            "Constants.mongol_three_mil_cavalry (+0x804) duplicate direct walk",
        )
    )


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument(
        "--offset",
        required=True,
        type=lambda text: int(text, 0),
        help="decompressed stream offset of Constants +0x000",
    )
    parser.add_argument("--layout", type=pathlib.Path, default=DEFAULT_LAYOUT_PATH)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    block = parse_constants_block(_load(args.file), args.offset, layout_path=args.layout)
    if args.json:
        print(json.dumps(_jsonable(block), indent=2))
    else:
        print(_summary(block, args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
