#!/usr/bin/env python3
"""Parse the complete retail ``ConquestGame::walk_data`` save image."""

from __future__ import annotations

import argparse
import dataclasses
import functools
import gzip
import hashlib
import json
import pathlib
import struct
from collections.abc import Callable
from typing import Sequence


TAG_CONQUEST_GAME = 0
TAG_STRING_TABLE_INDEX = 995
TAG_LEADERS_STRING_TABLE_INDEX = 1140
TAG_NODES_STRING_TABLE_INDEX = 1299
TAG_TRIBES_STRING_TABLE_INDEX = 6937
CONQUEST_PIECE_LISTS = 24
MAX_COUNT = 1 << 20
DEFAULT_SCHEMA_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"


class ConquestGameParseError(ValueError):
    """The stream or PDB layout contradicts ConquestGame::walk_data."""


@dataclasses.dataclass(frozen=True)
class Image:
    name: str
    offset: int
    end: int
    raw: bytes
    values: tuple[object, ...] = ()
    children: tuple["Image", ...] = ()
    sha256: str = ""

    @property
    def size(self) -> int: return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class ContainerImage:
    name: str
    offset: int
    end: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    presence: tuple[int, ...]
    repeated_capacity: int | None
    repeated_increment: int | None
    rows: tuple[Image, ...]
    sha256: str


@dataclasses.dataclass(frozen=True)
class ConquestGameSection:
    offset: int
    end: int
    tag: int
    fixed_prefix: bytes
    segments: tuple[Image | ContainerImage, ...]
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int: return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class _Layout:
    sha256: str


def _fields(record: dict[str, object]) -> tuple[tuple[object, ...], ...]:
    return tuple((f["name"], f["offset"], f["size"], f["type"]) for f in record["flattened"])


@functools.lru_cache(maxsize=4)
def _load_layout(path_text: str) -> _Layout:
    path = pathlib.Path(path_text)
    try: classes = json.loads(path.read_bytes())["classes"]
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error: raise ConquestGameParseError(f"cannot load ConquestGame PDB layout from {path}: {error}") from error
    sizes = {
        "ConquestGame": 5308, "Color": 12, "ConquestLeader": 1156,
        "ConquestNode": 172, "ConquestColony": 84, "ConquestPieces": 676,
        "ConquestPiece": 48, "ReinforcementArmy": 36, "DynamicBitMask": 12,
        "ConquestNewsItem": 24, "ConquestStyle": 376, "ConquestLink": 164,
        "StringListEntry": 52, "Tribe": 1520,
        "Array<Color>": 28, "ObjectArray<ConquestLeader>": 24,
        "ObjectArray<ConquestNode>": 24, "ObjectArray<ConquestColony>": 24,
        "ObjectArray<String>": 24, "SimpleArray<float>": 28,
        "PtrArray<ConquestPiece>": 28, "Array<ReinforcementArmy>": 28,
        "Array<ConquestNewsItem>": 28, "LinkList<int,short>": 24,
        "SimpleArray<int>": 28, "ObjectArray<ObjectArray<ConquestStyle> >": 24,
        "ObjectArray<ConquestStyle>": 24, "NamedSimpleArray<int>": 28,
        "NamedObjectArray<String>": 28, "ObjectArray<Tribe>": 24,
        "ObjectArray<ConquestBonusCard>": 24, "ObjectArray<ConquestLink>": 24,
        "PtrArray<StringListEntry>": 28,
    }
    receipt: dict[str, object] = {}
    for name, size in sizes.items():
        record = classes.get(name)
        if not record or record.get("size") != size: raise ConquestGameParseError(f"PDB {name} size disagrees: {record.get('size') if record else None}")
        receipt[name] = {"size": size, "flattened": _fields(record)}
    game = {name: (off, size, typ) for name, off, size, typ in _fields(classes["ConquestGame"])}
    expected = {
        "player_tribe": (4, 4, "int"), "starting_round": (388, 4, "int"),
        "conquest_leaders": (392, 76, "ConquestLeaders"), "conquest_nodes": (468, 28, "ConquestNodes"),
        "conquest_colonies": (496, 24, "ConquestColonies"), "conquest_continents": (520, 24, "ObjectArray<String>"),
        "barbarian_files": (544, 24, "ObjectArray<String>"), "map_file": (568, 20, "String"),
        "map_size_scale": (768, 28, "SimpleArray<float>"), "conquest_pieces": (796, 676, "ConquestPieces"),
        "reinforcements": (1472, 28, "Array<ReinforcementArmy>"), "help_pointers_shown": (1500, 12, "DynamicBitMask"),
        "conquest_news": (1512, 52, "ConquestNews"), "valid_tribes": (1564, 24, "LinkList<int,short>"),
        "game_styles": (1668, 132, "ConquestStyles"), "stored_ints": (1800, 28, "NamedSimpleArray<int>"),
        "ctw_tribes": (1884, 28, "Tribes"), "end_game_text": (1912, 20, "String"),
        "allied_punks": (2072, 28, "SimpleArray<int>"), "conquest_countries": (2100, 144, "ConquestCountries"),
    }
    if any(game.get(name) != value for name, value in expected.items()): raise ConquestGameParseError("PDB ConquestGame selected offsets disagree")
    if _fields(classes["ConquestPiece"])[0][1:] != (4, 4, "int") or _fields(classes["ConquestPiece"])[-1][1:] != (36, 12, "Vector<float>"): raise ConquestGameParseError("PDB ConquestPiece logical ranges disagree")
    if _fields(classes["ReinforcementArmy"])[0][1] != 4 or _fields(classes["ReinforcementArmy"])[-1][1] + 4 != 36: raise ConquestGameParseError("PDB ReinforcementArmy prefix disagrees")
    receipt["selectors"] = {"fixed": [4, 392], "last": [2072, 2100], "piece_lists": 24, "piece_ranges": [[4, 36], [36, 48]]}
    return _Layout(hashlib.sha256(json.dumps(receipt, sort_keys=True, separators=(",", ":")).encode()).hexdigest())


class _Reader:
    def __init__(self, data: bytes | bytearray | memoryview, offset: int):
        self.data = memoryview(data)
        if offset < 0 or offset > len(self.data): raise ConquestGameParseError(f"offset {offset:#x} outside stream")
        self.pos = offset

    def take(self, size: int, what: str) -> memoryview:
        end = self.pos + size
        if size < 0 or end > len(self.data): raise ConquestGameParseError(f"{what} range [{self.pos:#x},{end:#x}) exceeds {len(self.data):#x}")
        start = self.pos; self.pos = end; return self.data[start:end]

    def u8(self, what: str) -> int: return self.take(1, what)[0]
    def i16(self, what: str) -> int: return struct.unpack("<h", self.take(2, what))[0]
    def i32(self, what: str) -> int: return struct.unpack("<i", self.take(4, what))[0]


def _count(r: _Reader, what: str) -> int:
    value = r.i32(what)
    if value < 0 or value > MAX_COUNT: raise ConquestGameParseError(f"invalid {what} {value}")
    return value


def _image(r: _Reader, name: str, size: int, values: tuple[object, ...] = (), children: tuple[Image, ...] = ()) -> Image:
    start = r.pos; raw = bytes(r.take(size, name)); return Image(name, start, r.pos, raw, values, children, hashlib.sha256(raw).hexdigest())


def _wstr(r: _Reader, name: str) -> Image:
    start = r.pos; length = _count(r, f"{name} length"); payload = bytes(r.take(length * 2, f"{name} UTF-16")); raw = bytes(r.data[start:r.pos])
    return Image(name, start, r.pos, raw, (length, payload), (), hashlib.sha256(raw).hexdigest())


RowParser = Callable[[_Reader, str], Image]


def _array(r: _Reader, name: str, row_parser: RowParser, *, pointer: bool = False) -> ContainerImage:
    start = r.pos; length = _count(r, f"{name} length"); capacity = increment = flags = repeated_capacity = repeated_increment = None; presence: tuple[int, ...] = (); rows: list[Image] = []
    if length:
        capacity = _count(r, f"{name} capacity"); increment = r.i16(f"{name} increment"); flags = r.u8(f"{name} flags")
        if capacity < length or flags & 0x40: raise ConquestGameParseError(f"invalid {name} history")
        if pointer:
            presence = tuple(r.u8(f"{name} presence[{i}]") for i in range(length))
            if any(value not in (0, 1) for value in presence): raise ConquestGameParseError(f"{name} presence is not boolean")
            repeated_capacity = r.i32(f"{name} repeated capacity"); repeated_increment = r.i16(f"{name} repeated increment")
            if (repeated_capacity, repeated_increment) != (capacity, increment): raise ConquestGameParseError(f"{name} repeated history disagrees")
        else: presence = (1,) * length
        for index, present in enumerate(presence):
            if present: rows.append(row_parser(r, f"{name}[{index}]"))
    raw = bytes(r.data[start:r.pos])
    return ContainerImage(name, start, r.pos, length, capacity, increment, flags, presence, repeated_capacity, repeated_increment, tuple(rows), hashlib.sha256(raw).hexdigest())


def _raw_row(size: int) -> RowParser:
    return lambda r, name: _image(r, name, size)


def _simple(r: _Reader, name: str, element_size: int = 4) -> ContainerImage: return _array(r, name, _raw_row(element_size))
def _strings(r: _Reader, name: str) -> ContainerImage: return _array(r, name, _wstr)


def _mask(r: _Reader, name: str) -> Image:
    start = r.pos; bits = r.i32(f"{name}.bits"); size = r.i32(f"{name}.size")
    if bits < 0 or size < 0 or size > MAX_COUNT or bits > size * 8: raise ConquestGameParseError(f"invalid {name} bits/size {bits}/{size}")
    payload = bytes(r.take(size, f"{name}.payload")); raw = bytes(r.data[start:r.pos]); return Image(name, start, r.pos, raw, (bits, size, payload), (), hashlib.sha256(raw).hexdigest())


def _bonus_card(r: _Reader, name: str) -> Image: return _image(r, name, 12)


def _conquest_link(r: _Reader, name: str) -> Image:
    start = r.pos; _image(r, name + ".fixed", 24); children = tuple(_simple(r, f"{name}.list[{i}]") for i in range(5)); raw = bytes(r.data[start:r.pos]); return Image(name, start, r.pos, raw, (), children, hashlib.sha256(raw).hexdigest())


def _conquest_node(r: _Reader, name: str) -> Image:
    start = r.pos; tag = r.u8(name + ".tag"); fixed = _image(r, name + ".fixed", 80); strings = tuple(_wstr(r, f"{name}.string[{i}]") for i in range(3)); links = _array(r, name + ".links", _conquest_link); raw = bytes(r.data[start:r.pos]); return Image(name, start, r.pos, raw, (tag,), (*strings, Image(links.name, links.offset, links.end, bytes(r.data[links.offset:links.end]), (), links.rows, links.sha256)), hashlib.sha256(raw).hexdigest())


def _conquest_colony(r: _Reader, name: str) -> Image:
    start = r.pos; fixed = _image(r, name + ".fixed", 16); armies = _simple(r, name + ".armies"); strings = (_wstr(r, name + ".name"), _wstr(r, name + ".file")); raw = bytes(r.data[start:r.pos]); return Image(name, start, r.pos, raw, (), (fixed, Image(armies.name, armies.offset, armies.end, bytes(r.data[armies.offset:armies.end]), (), armies.rows, armies.sha256), *strings), hashlib.sha256(raw).hexdigest())


def _string_list_entry(r: _Reader, name: str) -> Image:
    start = r.pos; strings = (_wstr(r, name + ".string"), _wstr(r, name + ".prefix")); fixed = _image(r, name + ".fixed", 12); raw = bytes(r.data[start:r.pos]); return Image(name, start, r.pos, raw, (), (*strings, fixed), hashlib.sha256(raw).hexdigest())


def _conquest_style(r: _Reader, name: str) -> Image:
    start = r.pos; tag = r.u8(name + ".tag"); fixed = _image(r, name + ".fixed", 140); strings = tuple(_wstr(r, f"{name}.string[{i}]") for i in range(10)); change = _image(r, name + ".change_count", 4); entries = _array(r, name + ".info_text", _string_list_entry, pointer=True); raw = bytes(r.data[start:r.pos]); return Image(name, start, r.pos, raw, (tag,), (fixed, *strings, change, Image(entries.name, entries.offset, entries.end, bytes(r.data[entries.offset:entries.end]), (), entries.rows, entries.sha256)), hashlib.sha256(raw).hexdigest())


def _style_array(r: _Reader, name: str) -> Image:
    start = r.pos; inner = _array(r, name + ".inner", _conquest_style); raw = bytes(r.data[start:r.pos])
    return Image(name, start, r.pos, raw, (), inner.rows, hashlib.sha256(raw).hexdigest())


def _leader(r: _Reader, name: str) -> Image:
    start = r.pos; tag = r.u8(name + ".tag"); fixed = _image(r, name + ".fixed", 843); cards = _array(r, name + ".cards", _bonus_card); arrays1 = tuple(_simple(r, f"{name}.array[{i}]") for i in range(5)); label = _wstr(r, name + ".name"); masks = tuple(_mask(r, f"{name}.mask[{i}]") for i in range(3)); arrays2 = tuple(_simple(r, f"{name}.tail_array[{i}]") for i in range(3)); raw = bytes(r.data[start:r.pos]); return Image(name, start, r.pos, raw, (tag,), (fixed, Image(cards.name, cards.offset, cards.end, bytes(r.data[cards.offset:cards.end]), (), cards.rows, cards.sha256), *[Image(a.name, a.offset, a.end, bytes(r.data[a.offset:a.end]), (), a.rows, a.sha256) for a in arrays1], label, *masks, *[Image(a.name, a.offset, a.end, bytes(r.data[a.offset:a.end]), (), a.rows, a.sha256) for a in arrays2]), hashlib.sha256(raw).hexdigest())


def _piece(r: _Reader, name: str) -> Image: return _image(r, name, 44)


def _pieces(r: _Reader, name: str) -> Image:
    start = r.pos; lists = tuple(_array(r, f"{name}.lists[{i}]", _piece, pointer=True) for i in range(CONQUEST_PIECE_LISTS)); raw = bytes(r.data[start:r.pos]); children = tuple(Image(a.name, a.offset, a.end, bytes(r.data[a.offset:a.end]), (), a.rows, a.sha256) for a in lists); return Image(name, start, r.pos, raw, (), children, hashlib.sha256(raw).hexdigest())


def _named_ints(r: _Reader, name: str) -> ContainerImage:
    start = r.pos; values = _array(r, name, _raw_row(4)); names = tuple(_wstr(r, f"{name}.name[{i}]") for i in range(values.length)); raw = bytes(r.data[start:r.pos]); rows = values.rows + names
    return dataclasses.replace(values, end=r.pos, rows=rows, sha256=hashlib.sha256(raw).hexdigest())


def _named_strings(r: _Reader, name: str) -> ContainerImage:
    start = r.pos; values = _strings(r, name); names = tuple(_wstr(r, f"{name}.name[{i}]") for i in range(values.length)); raw = bytes(r.data[start:r.pos]); return dataclasses.replace(values, end=r.pos, rows=values.rows + names, sha256=hashlib.sha256(raw).hexdigest())


def _link_list(r: _Reader, name: str) -> Image:
    start = r.pos; length = _count(r, name + ".length"); rows = tuple(_image(r, f"{name}[{i}]", 6) for i in range(length)); raw = bytes(r.data[start:r.pos]); return Image(name, start, r.pos, raw, (length,), rows, hashlib.sha256(raw).hexdigest())


def _tribe(r: _Reader, name: str) -> Image:
    start = r.pos; tag = r.u8(name + ".tag"); fixed = _image(r, name + ".fixed", 1432); raw = bytes(r.data[start:r.pos]); return Image(name, start, r.pos, raw, (tag,), (fixed,), hashlib.sha256(raw).hexdigest())


def parse_conquest_game_section(data: bytes | bytearray | memoryview, offset: int, *, require_tag: bool = True, schema_path: pathlib.Path = DEFAULT_SCHEMA_PATH) -> ConquestGameSection:
    """Parse every ConquestGame phase and return the detail_threshold owner."""
    layout = _load_layout(str(pathlib.Path(schema_path).resolve())); r = _Reader(data, offset); tag = r.u8("ConquestGame tag")
    if require_tag and tag != TAG_CONQUEST_GAME: raise ConquestGameParseError(f"ConquestGame tag {tag:#04x} != {TAG_CONQUEST_GAME:#04x}")
    fixed = bytes(r.take(388, "ConquestGame +4..+392")); segments: list[Image | ContainerImage] = []
    segments.append(_array(r, "colors", _raw_row(10)))
    segments.append(_image(r, "leaders tag", 1)); segments.append(_array(r, "leaders", _leader))
    segments.append(_image(r, "nodes tag", 1)); segments.append(_array(r, "nodes", _conquest_node)); segments.append(_array(r, "colonies", _conquest_colony))
    segments.append(_strings(r, "continents")); segments.append(_strings(r, "barbarian_files"))
    segments.extend(_wstr(r, f"script[{i}]") for i in range(10)); segments.append(_simple(r, "map_size_scale")); segments.append(_pieces(r, "pieces"))
    segments.append(_array(r, "reinforcements", _raw_row(32))); segments.append(_mask(r, "help_pointers_shown")); segments.append(_strings(r, "news_strings")); segments.append(_array(r, "news_items", _raw_row(24))); segments.append(_link_list(r, "valid_tribes"))
    segments.append(_simple(r, "overrun_armies")); segments.append(_simple(r, "continents_captured")); segments.append(_strings(r, "bonus_card_deck")); segments.append(_array(r, "game_styles", _style_array))
    segments.append(_named_ints(r, "stored_ints")); segments.append(_named_strings(r, "stored_strs")); segments.append(_named_ints(r, "diplo_deals")); segments.append(_image(r, "tribes tag", 1)); segments.append(_array(r, "tribes", _tribe))
    segments.extend(_wstr(r, f"ending_string[{i}]") for i in range(8)); segments.append(_simple(r, "allied_punks"))
    return ConquestGameSection(offset, r.pos, tag, fixed, tuple(segments), hashlib.sha256(r.data[offset:r.pos]).hexdigest(), layout.sha256)


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes(); return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__); parser.add_argument("file", type=pathlib.Path); parser.add_argument("--offset", required=True, type=lambda text: int(text, 0)); args = parser.parse_args(argv); section = parse_conquest_game_section(_load(args.file), args.offset)
    print(f"{args.file}: ConquestGame {section.offset:#x}..{section.end:#x} ({section.size} bytes) sha256={section.sha256}\n  next owner begins at {section.end:#x}: detail_threshold")
    return 0


if __name__ == "__main__": raise SystemExit(main())
