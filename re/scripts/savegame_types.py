#!/usr/bin/env python3
"""Parse the retail ``Types::walk_data`` save-stream section.

This helper is intentionally bounded: it begins at the first byte owned by
``Types::walk_data`` and consumes only that function's 85 one-byte
``TechType::leader_off`` fields.  It does not search for the section and it
does not consume the following ``TileSet::walk_data`` owner.

The grammar is recovered from the shipped PE at ``0x00669780``.  The matching
PDB names the global as ``PtrArray<TechType>&`` and identifies the byte reached
at ``TechType+0x1e2`` as ``leader_off``.  No specimen bytes are embedded or
written.
"""

from __future__ import annotations

import argparse
import dataclasses
import gzip
import hashlib
import json
import pathlib
from typing import Sequence


TECH_TYPE_INDEX_START = 0x880 // 4
TECH_TYPE_INDEX_END = 0x9D4 // 4
TECH_TYPE_COUNT = TECH_TYPE_INDEX_END - TECH_TYPE_INDEX_START
LEADER_OFF_FIELD_OFFSET = 0x1E2


class TypesParseError(ValueError):
    """The input cannot contain the complete recovered Types traversal."""


@dataclasses.dataclass(frozen=True)
class TechLeaderOff:
    tech_type_index: int
    offset: int
    value: int


@dataclasses.dataclass(frozen=True)
class TypesSection:
    offset: int
    end: int
    fields: tuple[TechLeaderOff, ...]
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


def parse_types_section(
    data: bytes | bytearray | memoryview,
    offset: int,
) -> TypesSection:
    """Parse one exact retail Types section beginning at ``offset``.

    Every possible byte value is valid for ``TechType::leader_off``.  The
    structural invariant is therefore the PE-derived fixed count, not a value
    whitelist.  The returned ``end`` is the first byte owned by the following
    ``TileSet::walk_data`` call.
    """

    view = memoryview(data)
    if offset < 0 or offset > len(view):
        raise TypesParseError(f"offset {offset:#x} is outside the stream")
    end = offset + TECH_TYPE_COUNT
    if end > len(view):
        raise TypesParseError(
            f"Types range [{offset:#x},{end:#x}) exceeds "
            f"{len(view):#x}-byte stream"
        )

    fields = tuple(
        TechLeaderOff(
            tech_type_index=TECH_TYPE_INDEX_START + relative,
            offset=offset + relative,
            value=view[offset + relative],
        )
        for relative in range(TECH_TYPE_COUNT)
    )
    return TypesSection(
        offset=offset,
        end=end,
        fields=fields,
        sha256=hashlib.sha256(view[offset:end]).hexdigest(),
    )


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def _jsonable(section: TypesSection) -> dict[str, object]:
    return {
        "offset": f"0x{section.offset:x}",
        "end": f"0x{section.end:x}",
        "size": section.size,
        "sha256": section.sha256,
        "leader_off": [dataclasses.asdict(field) for field in section.fields],
    }


def _summary(section: TypesSection, path: pathlib.Path) -> str:
    nonzero = [
        (field.tech_type_index, field.value)
        for field in section.fields
        if field.value != 0
    ]
    return "\n".join(
        (
            f"{path}: Types {section.offset:#x}..{section.end:#x} "
            f"({section.size} bytes) sha256={section.sha256}",
            f"  TechType indices {TECH_TYPE_INDEX_START}..{TECH_TYPE_INDEX_END - 1}; "
            f"leader_off nonzero={nonzero}",
            f"  next owner begins at {section.end:#x} "
            "(TileSet::walk_data 0x0087b290)",
        )
    )


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument(
        "--offset",
        required=True,
        type=lambda text: int(text, 0),
        help="decompressed stream offset of the first Types byte",
    )
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    section = parse_types_section(_load(args.file), args.offset)
    if args.json:
        print(json.dumps(_jsonable(section), indent=2))
    else:
        print(_summary(section, args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
