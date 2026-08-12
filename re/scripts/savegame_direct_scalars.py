#!/usr/bin/env python3
"""Decode the three direct scalar walks between Constants and Armies.

In exact caller order these are the duplicate
``Constants::mongol_three_mil_cavalry`` value, ``GameAccess::ai_speed``, and
``GameAccess::ai_off``.  Each is one unconditional four-byte
``DataWalk::walk_function`` call.  Profiling calls between them own no stream
bytes, so this exclusive helper treats their consecutive 12-byte stream image
as one coherent tranche and stops before ``Armies::walk_data``.
"""

from __future__ import annotations

import argparse
import dataclasses
import gzip
import hashlib
import json
import pathlib
import struct
from typing import Sequence


DIRECT_SCALARS_SIZE = 12

SCALAR_LAYOUT = (
    (
        "Constants.mongol_three_mil_cavalry",
        "Constants+0x804",
        0x005A2A7C,
    ),
    ("GameAccess.ai_speed", "*(int **)0x00c061c0", 0x005A2A97),
    ("GameAccess.ai_off", "*(int **)0x00c061c4", 0x005A2AB2),
)


class DirectScalarsParseError(ValueError):
    """The stream cannot contain the complete recovered scalar tranche."""


@dataclasses.dataclass(frozen=True)
class DirectScalar:
    name: str
    offset: int
    end: int
    source: str
    caller_va: int
    value: int

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class DirectScalarsSection:
    offset: int
    end: int
    fields: tuple[DirectScalar, ...]
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


def parse_direct_scalars(
    data: bytes | bytearray | memoryview,
    offset: int,
) -> DirectScalarsSection:
    """Decode the exact 12-byte scalar tranche beginning at ``offset``.

    The returned ``end`` is the first byte owned by ``Armies::walk_data``.
    """

    view = memoryview(data)
    if offset < 0 or offset > len(view):
        raise DirectScalarsParseError(f"offset {offset:#x} is outside the stream")
    end = offset + DIRECT_SCALARS_SIZE
    if end > len(view):
        raise DirectScalarsParseError(
            f"direct scalar range [{offset:#x},{end:#x}) exceeds "
            f"{len(view):#x}-byte stream"
        )

    fields = tuple(
        DirectScalar(
            name=name,
            offset=offset + index * 4,
            end=offset + (index + 1) * 4,
            source=source,
            caller_va=caller_va,
            value=struct.unpack_from("<i", view, offset + index * 4)[0],
        )
        for index, (name, source, caller_va) in enumerate(SCALAR_LAYOUT)
    )
    return DirectScalarsSection(
        offset=offset,
        end=end,
        fields=fields,
        sha256=hashlib.sha256(view[offset:end]).hexdigest(),
    )


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def _jsonable(value: object) -> object:
    if dataclasses.is_dataclass(value):
        result: dict[str, object] = {}
        for field in dataclasses.fields(value):
            field_value = getattr(value, field.name)
            if field.name in ("offset", "end", "caller_va"):
                result[field.name] = f"0x{field_value:x}"
            else:
                result[field.name] = _jsonable(field_value)
        if isinstance(value, (DirectScalar, DirectScalarsSection)):
            result["size"] = value.size
        return result
    if isinstance(value, tuple):
        return [_jsonable(item) for item in value]
    return value


def _summary(section: DirectScalarsSection, path: pathlib.Path) -> str:
    lines = [
        f"{path}: direct scalars {section.offset:#x}..{section.end:#x} "
        f"({section.size} bytes) sha256={section.sha256}"
    ]
    lines.extend(
        f"  {field.name}={field.value} at {field.offset:#x} "
        f"(caller {field.caller_va:#010x})"
        for field in section.fields
    )
    lines.append(
        f"  next owner begins at {section.end:#x} "
        "(Armies::walk_data 0x006f3700)"
    )
    return "\n".join(lines)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument(
        "--offset",
        required=True,
        type=lambda text: int(text, 0),
        help="decompressed stream offset of the duplicate Constants scalar",
    )
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    section = parse_direct_scalars(_load(args.file), args.offset)
    if args.json:
        print(json.dumps(_jsonable(section), indent=2))
    else:
        print(_summary(section, args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
