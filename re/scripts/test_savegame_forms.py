#!/usr/bin/env python3
"""Exact structural, mutation, and artifact tests for savegame_forms.py."""

from __future__ import annotations

import gzip
import hashlib
import json
import pathlib
import re
import struct
import sys
import unittest


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import savegame_parse  # noqa: E402
from savegame_armies import parse_armies_section  # noqa: E402
from savegame_cities import parse_cities_section  # noqa: E402
from savegame_constants_block import parse_constants_block  # noqa: E402
from savegame_direct_scalars import parse_direct_scalars  # noqa: E402
from savegame_forms import (  # noqa: E402
    FORM_DATA_SIZE,
    FORM_TAG_STRING_TABLE_INDEX,
    TAG_FORMS,
    FormsParseError,
    parse_forms_section,
)
from savegame_leaders import parse_leaders_section  # noqa: E402
from savegame_mountains import parse_mountains_section  # noqa: E402
from savegame_tileset import parse_tileset_section  # noqa: E402
from savegame_types import parse_types_section  # noqa: E402


PREFIX = b"PRE-FORMS"
FOLLOWING_OWNER = b"GOODS-SENTINEL" * 32
NAME_UNITS = tuple(map(ord, "Line"))
DESC_UNITS = (ord("A"), 0x03A9, ord("B"))
FORM_WORDS = tuple(index * 17 - 7000 for index in range(FORM_DATA_SIZE // 4))


def _wide(units: tuple[int, ...]) -> bytes:
    return struct.pack(f"<I{len(units)}H", len(units), *units)


def _row(tag: int = 0x5A, words: tuple[int, ...] = FORM_WORDS) -> bytes:
    raw = struct.pack(f"<{len(words)}i", *words)
    assert len(raw) == FORM_DATA_SIZE
    return bytes((tag,)) + _wide(NAME_UNITS) + _wide(DESC_UNITS) + raw


def _synthetic() -> tuple[bytes, int, bytes]:
    out = bytearray(PREFIX)
    offset = len(out)
    out.append(TAG_FORMS)
    out.extend(struct.pack("<iihB", 1, 7, 5, 0x25))
    out.extend(_row())
    return bytes(out) + FOLLOWING_OWNER, offset, FOLLOWING_OWNER


def _canonical_shape_fixture() -> bytes:
    out = bytearray((TAG_FORMS,))
    out.extend(struct.pack("<iihB", 10, 10, -1, 0))
    for index in range(10):
        words = [0] * (FORM_DATA_SIZE // 4)
        words[0] = index  # FormData::form at +0x28.
        out.extend(_row(index, tuple(words)))
    return bytes(out)


def _find(node: object, prefix: str):
    if node.name.startswith(prefix):
        return node
    for child in node.kids:
        found = _find(child, prefix)
        if found is not None:
            return found
    return None


def _field(node: object, name: str):
    return next(field["value"] for field in node.fields if field["name"] == name)


def _pe_offset(image: bytes, va: int) -> int:
    pe = struct.unpack_from("<I", image, 0x3C)[0]
    if image[pe : pe + 4] != b"PE\0\0":
        raise AssertionError("not a PE image")
    section_count = struct.unpack_from("<H", image, pe + 6)[0]
    optional_size = struct.unpack_from("<H", image, pe + 20)[0]
    optional = pe + 24
    image_base = struct.unpack_from("<I", image, optional + 28)[0]
    rva = va - image_base
    table = optional + optional_size
    for index in range(section_count):
        section = table + index * 40
        virtual_size, virtual_address, raw_size, raw = struct.unpack_from(
            "<IIII", image, section + 8
        )
        if virtual_address <= rva < virtual_address + max(virtual_size, raw_size):
            return raw + rva - virtual_address
    raise AssertionError(f"VA {va:#x} is outside the image")


class FormsParserTests(unittest.TestCase):
    def test_complete_form_row_and_exact_goods_boundary(self) -> None:
        data, offset, following_owner = _synthetic()
        parsed = parse_forms_section(data, offset)
        self.assertEqual(parsed.tag, TAG_FORMS)
        self.assertEqual((parsed.length, parsed.capacity, parsed.increment, parsed.flags), (1, 7, 5, 0x25))
        self.assertEqual(parsed.layout_sha256, "c84580b8b69cc7606cb6a792a7c0ebc452e68601dd925d009d326004e98bc818")
        self.assertEqual(data[parsed.end :], following_owner)
        self.assertEqual(len(parsed.rows), 1)
        row = parsed.rows[0]
        self.assertEqual(row.tag, 0x5A)
        self.assertEqual(FORM_TAG_STRING_TABLE_INDEX, 2694)
        self.assertEqual(row.name.code_units, NAME_UNITS)
        self.assertEqual(row.desc.code_units, DESC_UNITS)
        self.assertEqual(row.name.offset, row.offset + 1)
        self.assertEqual(row.desc.offset, row.name.end)
        self.assertEqual(row.data_offset, row.desc.end)

        fields = {field.name: field for field in row.fields}
        self.assertEqual(tuple(fields), (
            "form", "density", "o", "idx", "who", "num_category",
            "x_spacing", "y_spacing", "cat_id", "category", "to_x", "to_y",
            "off_x", "off_y", "wedge", "total", "guarding", "per", "space",
            "reverse", "across",
        ))
        cursor = 0
        for field in row.fields:
            count = (field.end - field.offset) // 4
            self.assertEqual(field.values, FORM_WORDS[cursor : cursor + count])
            self.assertEqual(field.offset, row.data_offset + field.pdb_offset - 0x28)
            cursor += count
        self.assertEqual(cursor, len(FORM_WORDS))
        self.assertEqual(row.end - row.data_offset, FORM_DATA_SIZE)
        self.assertEqual(parsed.sha256, hashlib.sha256(data[offset : parsed.end]).hexdigest())

    def test_every_owned_byte_mutation_is_killed_or_changes_receipt(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_forms_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[offset + relative] ^= 1
                try:
                    changed = parse_forms_section(damaged, offset)
                except FormsParseError:
                    continue
                self.assertNotEqual(changed, baseline)
                self.assertNotEqual(changed.sha256, baseline.sha256)

    def test_every_truncation_is_rejected(self) -> None:
        data, offset, _ = _synthetic()
        end = parse_forms_section(data, offset).end
        for retained in range(end - offset):
            with self.subTest(retained=retained):
                with self.assertRaises(FormsParseError):
                    parse_forms_section(data[: offset + retained], offset)

    def test_following_goods_mutation_is_excluded(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_forms_section(data, offset)
        damaged = bytearray(data)
        damaged[baseline.end] ^= 0xFF
        self.assertEqual(parse_forms_section(damaged, offset), baseline)

    def test_structural_invariants_fail_closed(self) -> None:
        data, offset, _ = _synthetic()
        row = parse_forms_section(data, offset).rows[0]
        cases: dict[str, tuple[int, bytes]] = {
            "main tag": (offset, b"\x7f"),
            "negative length": (offset + 1, struct.pack("<i", -1)),
            "oversized length": (offset + 1, struct.pack("<i", (1 << 20) + 1)),
            "capacity below length": (offset + 5, struct.pack("<i", 0)),
            "writer-cleared flags": (offset + 11, b"\x65"),
            "oversized name": (row.name.offset, struct.pack("<I", 0x10000)),
            "oversized desc": (row.desc.offset, struct.pack("<I", 0x10000)),
        }
        for name, (where, replacement) in cases.items():
            with self.subTest(name=name):
                damaged = bytearray(data)
                damaged[where : where + len(replacement)] = replacement
                with self.assertRaises(FormsParseError):
                    parse_forms_section(damaged, offset)

    def test_valid_container_history_and_row_tag_are_preserved(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_forms_section(data, offset)
        changed = bytearray(data)
        struct.pack_into("<i", changed, offset + 5, 99)
        struct.pack_into("<h", changed, offset + 9, -17)
        changed[baseline.rows[0].offset] = 0xE7
        parsed = parse_forms_section(changed, offset)
        self.assertEqual((parsed.capacity, parsed.increment), (99, -17))
        self.assertEqual(parsed.rows[0].tag, 0xE7)
        self.assertEqual(parsed.end, baseline.end)

    def test_forms_init_shape_cross_checks_canonical_group_formation_indices(self) -> None:
        data = _canonical_shape_fixture()
        parsed = parse_forms_section(data, 0)
        self.assertEqual((parsed.length, parsed.capacity, parsed.increment, parsed.flags), (10, 10, -1, 0))
        self.assertEqual([row.fields[0].values[0] for row in parsed.rows], list(range(10)))

        root = pathlib.Path(__file__).resolve().parents[2]
        source = (root / "crates/don-sim/src/systems/groups_guys.rs").read_text()
        block = re.search(r"pub enum Formation \{(?P<body>.*?)\n\}", source, re.S)
        self.assertIsNotNone(block)
        variants = re.findall(r"^\s*([A-Za-z]+) = (\d+),", block.group("body"), re.M)
        self.assertEqual(variants, [
            ("Line", "0"), ("Refused", "1"), ("Envelop", "2"),
            ("EchelonRight", "3"), ("EchelonLeft", "4"), ("Sparse", "5"),
            ("Square", "6"), ("Wedge", "7"), ("Column", "8"), ("Mob", "9"),
        ])

    def test_pdb_layout_and_executable_bodies_are_frozen(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]
        schema = root / "schema/pdb-types.json"
        pdb = root / "ron-bin/sbl/rise.pdb"
        exe = root / "ron-bin/riseofnations.exe"
        for path in (schema, pdb, exe):
            if not path.exists():
                self.skipTest(f"matched artifact is not installed: {path}")
        self.assertEqual(hashlib.sha256(schema.read_bytes()).hexdigest(), "399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14")
        self.assertEqual(hashlib.sha256(pdb.read_bytes()).hexdigest(), "334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5")
        image = exe.read_bytes()
        self.assertEqual(hashlib.sha256(image).hexdigest(), "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079")

        classes = json.loads(schema.read_text())["classes"]
        self.assertEqual(classes["ObjectArray<Form>"]["size"], 24)
        self.assertEqual(classes["Form"]["size"], 3736)
        self.assertEqual(classes["FormData"]["size"], 3728)
        self.assertEqual(classes["String"]["size"], 20)

        spans = (
            (0x00481190, 532, "033e1f6fd5961bf88c6ebc7d7af88da7b1678e8d8d8cf30e2e11bce09ecb9b19"),
            (0x0072DF10, 71, "013cff750959b382526ddbdb358e92d8f39c76ae199880f9dbf637dac2f462e5"),
            (0x0072E970, 40, "b1f0256b7a174f1df2165a97f000812897cb238255cbf281492a00c6143085f7"),
            (0x0072E9A0, 652, "8645f8ae33df39aa200a43548f08e112f464646f72321f82eb23750f3c71b891"),
            (0x00A1B2D0, 207, "301e6a00fb85903d10e1068d02ff73a58aa85dc72ed640f32c3a2791003805b2"),
            (0x005A2ADE, 21, "d259f614f270769b0c91c7c6a97257314ee3ac8e7460f22ed72bb446d85e27af"),
            (0x0045CCE0, 830, "e68361521d98ae326acba23195fb5f0650fbfc010a8697884b54df21376cdd5b"),
        )
        for va, size, digest in spans:
            with self.subTest(va=f"{va:#x}"):
                raw = _pe_offset(image, va)
                self.assertEqual(hashlib.sha256(image[raw : raw + size]).hexdigest(), digest)

    def test_fresh_svx_chains_to_exact_goods_boundary_without_replay_join(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]
        save = root / "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX"
        replay = root / "ron-data/replays/Playback - 2026.08.11 11'44'38 (Tue).rcx"
        if not save.exists() or not replay.exists():
            self.skipTest("user-owned fresh SVX/RCX fixtures are not installed")

        save_raw = save.read_bytes()
        replay_raw = replay.read_bytes()
        self.assertEqual(hashlib.sha256(save_raw).hexdigest(), "161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7")
        self.assertEqual(hashlib.sha256(replay_raw).hexdigest(), "558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54")
        save_plain = gzip.decompress(save_raw)
        self.assertEqual(hashlib.sha256(save_plain).hexdigest(), "fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8")

        leaders = parse_leaders_section(save_plain, 0x9A5A)
        types = parse_types_section(save_plain, leaders.end)
        tileset = parse_tileset_section(save_plain, types.end)
        mountains = parse_mountains_section(save_plain, tileset.end)
        constants = parse_constants_block(save_plain, mountains.end)
        scalars = parse_direct_scalars(save_plain, constants.end)
        armies = parse_armies_section(save_plain, scalars.end)
        cities = parse_cities_section(save_plain, armies.end)
        self.assertEqual(cities.end, 0x27040)
        parsed = parse_forms_section(save_plain, cities.end)
        self.assertEqual((parsed.offset, parsed.end, parsed.size), (0x27040, 0x27045, 5))
        self.assertEqual(parsed.length, 0)
        self.assertEqual(parsed.sha256, "8855508aade16ec573d21e6a485dfd0a7624085c1a14b5ecdd6485de0c6839a4")

        save_tree = savegame_parse.parse(save_plain)
        replay_tree = savegame_parse.parse(gzip.decompress(replay_raw))
        save_seed = _field(_find(save_tree, "GameInfo::walk_data"), "seed")
        replay_seed = _field(_find(replay_tree, "GameInfo::walk_data"), "seed")
        self.assertEqual(save_seed, "0x014810ac")
        self.assertEqual(replay_seed, "0x007f93e0")
        self.assertNotEqual(save_seed, replay_seed)


if __name__ == "__main__":
    unittest.main()
