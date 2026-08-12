#!/usr/bin/env python3
"""Decode the direct four-byte ``GameAccess::game_random`` save owner.

The top-level retail caller loads the ``Random&`` at ``0x00c06184`` and submits
the complete PDB-sized object ``[+0,+4)`` directly to DataWalk.  This helper
stops at the following ``GraphicEvents::walk_data`` call.
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


GAME_RANDOM_BLOCK_SIZE = 4
GAME_RANDOM_REFERENCE_VA = 0x00C06184
GAME_RANDOM_OBJECT_VA = 0x00E37A8C
CALLER_VA = 0x005A2DDB
NEXT_OWNER_CALL_VA = 0x005A2DEC
NEXT_OWNER_WALK_VA = 0x008E4D70
DEFAULT_SCHEMA_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"


class GameRandomParseError(ValueError):
    """The stream or PDB layout contradicts the direct Random owner."""


@dataclasses.dataclass(frozen=True)
class GameRandomBlock:
    offset: int
    end: int
    random_seed: int
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@functools.lru_cache(maxsize=4)
def _load_layout(path_text: str) -> str:
    path = pathlib.Path(path_text)
    try:
        record = json.loads(path.read_bytes())["classes"]["Random"]
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise GameRandomParseError(f"cannot load Random PDB layout from {path}: {error}") from error
    fields = record.get("fields")
    if record.get("size") != 4 or not isinstance(fields, list) or len(fields) != 1:
        raise GameRandomParseError(
            f"PDB Random layout disagrees: size={record.get('size')!r}, fields={fields!r}"
        )
    field = fields[0]
    expected = {"name": "random_seed", "offset": 0, "size": 4, "type": "unsigned long"}
    if any(field.get(key) != value for key, value in expected.items()):
        raise GameRandomParseError(f"PDB Random.random_seed layout disagrees: {field!r}")
    receipt = {"class": "Random", "size": 4, "field": expected}
    return hashlib.sha256(
        json.dumps(receipt, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()


def parse_game_random_block(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    schema_path: pathlib.Path = DEFAULT_SCHEMA_PATH,
) -> GameRandomBlock:
    """Decode one unsigned 32-bit seed and return GraphicEvents' boundary."""

    view = memoryview(data)
    if offset < 0 or offset > len(view):
        raise GameRandomParseError(f"offset {offset:#x} is outside the stream")
    end = offset + GAME_RANDOM_BLOCK_SIZE
    if end > len(view):
        raise GameRandomParseError(
            f"game_random block [{offset:#x},{end:#x}) exceeds {len(view):#x}-byte stream"
        )
    layout_sha256 = _load_layout(str(pathlib.Path(schema_path).resolve()))
    return GameRandomBlock(
        offset,
        end,
        struct.unpack_from("<I", view, offset)[0],
        hashlib.sha256(view[offset:end]).hexdigest(),
        layout_sha256,
    )


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def _jsonable(block: GameRandomBlock) -> dict[str, object]:
    return {
        "offset": f"0x{block.offset:x}",
        "end": f"0x{block.end:x}",
        "size": block.size,
        "random_seed": block.random_seed,
        "random_seed_hex": f"0x{block.random_seed:08x}",
        "sha256": block.sha256,
        "layout_sha256": block.layout_sha256,
    }


def _summary(block: GameRandomBlock, path: pathlib.Path) -> str:
    return "\n".join((
        f"{path}: game_random[+0x0..+0x4) {block.offset:#x}..{block.end:#x} "
        f"({block.size} bytes) sha256={block.sha256}",
        f"  Random.random_seed=0x{block.random_seed:08x} ({block.random_seed})",
        f"  PDB layout receipt sha256={block.layout_sha256}",
        f"  next owner begins at {block.end:#x}: GraphicEvents::walk_data 0x008e4d70",
    ))


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument("--offset", required=True, type=lambda text: int(text, 0))
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    block = parse_game_random_block(_load(args.file), args.offset)
    print(json.dumps(_jsonable(block), indent=2) if args.json else _summary(block, args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
