#!/usr/bin/env python3
"""Decode the caller's direct ``GameDaemon[+0x00..+0x28)`` save block.

``WalkDataGame::walk_data`` submits this range directly to the DataWalk at
``0x005a2dc0``.  The matching PDB names it as ``repaths[8]``,
``empty_colls``, and ``borders``.  The helper stops before the caller's
separate four-byte ``GameAccess::game_random`` walk; ``GameDaemon::busy`` at
``+0x28`` is not serialized by this call.
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


GAME_DAEMON_BLOCK_SIZE = 0x28
GAME_DAEMON_CLASS_SIZE = 0x2C
CALLER_VA = 0x005A2DC0
NEXT_CALLER_VA = 0x005A2DDB
DEFAULT_SCHEMA_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"
_INT_TYPE = re.compile(r"^int(?:\[(\d+)\])?$")


class GameDaemonParseError(ValueError):
    """The stream or PDB layout contradicts the direct GameDaemon block."""


@dataclasses.dataclass(frozen=True)
class GameDaemonField:
    name: str
    relative_offset: int
    stream_offset: int
    type_name: str
    values: tuple[int, ...]

    @property
    def size(self) -> int:
        return len(self.values) * 4


@dataclasses.dataclass(frozen=True)
class GameDaemonBlock:
    offset: int
    end: int
    fields: tuple[GameDaemonField, ...]
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset

    @property
    def repaths(self) -> tuple[int, ...]:
        return self.fields[0].values

    @property
    def empty_colls(self) -> int:
        return self.fields[1].values[0]

    @property
    def borders(self) -> int:
        return self.fields[2].values[0]

    @property
    def words(self) -> tuple[int, ...]:
        return tuple(value for field in self.fields for value in field.values)


@dataclasses.dataclass(frozen=True)
class _FieldLayout:
    name: str
    offset: int
    type_name: str
    count: int


@dataclasses.dataclass(frozen=True)
class _Layout:
    fields: tuple[_FieldLayout, ...]
    sha256: str


@functools.lru_cache(maxsize=4)
def _load_layout(path_text: str) -> _Layout:
    path = pathlib.Path(path_text)
    try:
        raw = path.read_bytes()
        record = json.loads(raw)["classes"]["GameDaemon"]
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise GameDaemonParseError(f"cannot load GameDaemon PDB layout from {path}: {error}") from error
    if record.get("size") != GAME_DAEMON_CLASS_SIZE:
        raise GameDaemonParseError(
            f"PDB sizeof(GameDaemon)={record.get('size')!r}, expected {GAME_DAEMON_CLASS_SIZE}"
        )
    raw_fields = record.get("fields")
    if not isinstance(raw_fields, list):
        raise GameDaemonParseError("PDB GameDaemon fields are missing")

    layouts: list[_FieldLayout] = []
    cursor = 0
    for field in raw_fields:
        try:
            name, offset, size, type_name = (
                field["name"], field["offset"], field["size"], field["type"]
            )
        except KeyError as error:
            raise GameDaemonParseError(f"malformed PDB GameDaemon field: {field!r}") from error
        if offset >= GAME_DAEMON_BLOCK_SIZE:
            break
        if offset != cursor:
            relation = "overlap" if offset < cursor else "gap"
            raise GameDaemonParseError(
                f"PDB GameDaemon block has {relation} before {name!r}: {cursor:#x} != {offset:#x}"
            )
        match = _INT_TYPE.fullmatch(type_name)
        count = int(match.group(1) or "1") if match else 0
        if not match or count * 4 != size:
            raise GameDaemonParseError(
                f"PDB GameDaemon field {name!r} has unsupported {type_name!r}/{size}-byte layout"
            )
        layouts.append(_FieldLayout(name, offset, type_name, count))
        cursor += size
    expected = (("repaths", 0, "int[8]", 8), ("empty_colls", 32, "int", 1), ("borders", 36, "int", 1))
    actual = tuple((field.name, field.offset, field.type_name, field.count) for field in layouts)
    if cursor != GAME_DAEMON_BLOCK_SIZE or actual != expected:
        raise GameDaemonParseError(
            f"PDB GameDaemon serialized prefix disagrees: end={cursor:#x}, fields={actual!r}"
        )
    excluded = [field for field in raw_fields if field.get("offset") == GAME_DAEMON_BLOCK_SIZE]
    if len(excluded) != 1 or any(
        excluded[0].get(key) != value
        for key, value in (("name", "busy"), ("size", 4), ("type", "int"))
    ):
        raise GameDaemonParseError("PDB first excluded GameDaemon field is not int busy at +0x28")
    receipt = {
        "class_size": GAME_DAEMON_CLASS_SIZE,
        "fields": actual,
        "excluded": ("busy", GAME_DAEMON_BLOCK_SIZE, 4, "int"),
    }
    return _Layout(
        tuple(layouts),
        hashlib.sha256(json.dumps(receipt, sort_keys=True, separators=(",", ":")).encode()).hexdigest(),
    )


def parse_game_daemon_block(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    schema_path: pathlib.Path = DEFAULT_SCHEMA_PATH,
) -> GameDaemonBlock:
    """Decode exactly 40 bytes and return the game_random owner boundary."""

    view = memoryview(data)
    if offset < 0 or offset > len(view):
        raise GameDaemonParseError(f"offset {offset:#x} is outside the stream")
    end = offset + GAME_DAEMON_BLOCK_SIZE
    if end > len(view):
        raise GameDaemonParseError(
            f"GameDaemon block [{offset:#x},{end:#x}) exceeds {len(view):#x}-byte stream"
        )
    layout = _load_layout(str(pathlib.Path(schema_path).resolve()))
    fields = tuple(
        GameDaemonField(
            field.name,
            field.offset,
            offset + field.offset,
            field.type_name,
            struct.unpack_from(f"<{field.count}i", view, offset + field.offset),
        )
        for field in layout.fields
    )
    return GameDaemonBlock(
        offset,
        end,
        fields,
        hashlib.sha256(view[offset:end]).hexdigest(),
        layout.sha256,
    )


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def _jsonable(value: object) -> object:
    if dataclasses.is_dataclass(value):
        result: dict[str, object] = {}
        for field in dataclasses.fields(value):
            item = getattr(value, field.name)
            result[field.name] = f"0x{item:x}" if field.name in ("offset", "end", "relative_offset", "stream_offset") else _jsonable(item)
        if isinstance(value, (GameDaemonField, GameDaemonBlock)):
            result["size"] = value.size
        return result
    if isinstance(value, tuple):
        return [_jsonable(item) for item in value]
    return value


def _summary(block: GameDaemonBlock, path: pathlib.Path) -> str:
    return "\n".join((
        f"{path}: GameDaemon[+0x00..+0x28) {block.offset:#x}..{block.end:#x} "
        f"({block.size} bytes) sha256={block.sha256}",
        f"  repaths={block.repaths} empty_colls={block.empty_colls} borders={block.borders}",
        f"  PDB layout receipt sha256={block.layout_sha256}",
        f"  next owner begins at {block.end:#x}: GameAccess::game_random[+0x0..+0x4)",
    ))


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument("--offset", required=True, type=lambda text: int(text, 0))
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    block = parse_game_daemon_block(_load(args.file), args.offset)
    print(json.dumps(_jsonable(block), indent=2) if args.json else _summary(block, args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
