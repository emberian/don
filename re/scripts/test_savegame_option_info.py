#!/usr/bin/env python3
"""Exact mutation, artifact, and boundary tests for OptionInfo saves."""

from __future__ import annotations

import gzip
import hashlib
import json
import pathlib
import struct
import sys
import unittest


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import savegame_parse  # noqa: E402
from savegame_armies import parse_armies_section  # noqa: E402
from savegame_caravans import parse_caravans_section  # noqa: E402
from savegame_cities import parse_cities_section  # noqa: E402
from savegame_constants_block import parse_constants_block  # noqa: E402
from savegame_direct_scalars import parse_direct_scalars  # noqa: E402
from savegame_docks import parse_docks_section  # noqa: E402
from savegame_forms import parse_forms_section  # noqa: E402
from savegame_forts import parse_forts_section  # noqa: E402
from savegame_goods import parse_goods_section  # noqa: E402
from savegame_herds import parse_herds_section  # noqa: E402
from savegame_heroes import parse_heroes_section  # noqa: E402
from savegame_items import parse_items_section  # noqa: E402
from savegame_lands import parse_lands_section  # noqa: E402
from savegame_leader_options import parse_leader_options_section  # noqa: E402
from savegame_leaders import parse_leaders_section  # noqa: E402
from savegame_mountains import parse_mountains_section  # noqa: E402
from savegame_oil_wells import parse_oil_wells_section  # noqa: E402
from savegame_option_info import (  # noqa: E402
    OPTION_COUNT,
    ROW_TAG_STRING_TABLE_INDEX,
    TAG_STRING_TABLE_INDEX,
    OptionInfoParseError,
    parse_option_info_section,
)
from savegame_specials import parse_specials_section  # noqa: E402
from savegame_supplies import parse_supplies_section  # noqa: E402
from savegame_tileset import parse_tileset_section  # noqa: E402
from savegame_types import parse_types_section  # noqa: E402
from savegame_wonders import parse_wonders_section  # noqa: E402


PREFIX = b"PRE-OPTION-INFO"
FOLLOWING_OWNER = b"DIRECT-108-SENTINEL" * 32


def _wide(*code_units: int) -> bytes:
    return struct.pack("<I", len(code_units)) + struct.pack(
        f"<{len(code_units)}H", *code_units
    )


def _strings(index: int) -> tuple[tuple[int, ...], tuple[int, ...]]:
    name = () if index % 4 == 0 else (0x40 + index, index & 0xFFFF)
    desc = () if index % 5 == 0 else (0x390 + index, 0, 0xD800 + index % 0x400)
    return name, desc


def _row(index: int) -> bytes:
    name, desc = _strings(index)
    return bytes(((0x20 + index) & 0xFF,)) + _wide(*name) + _wide(*desc)


ROWS = tuple(_row(index) for index in range(OPTION_COUNT))


def _section() -> bytes:
    return b"\0" + b"".join(ROWS)


def _synthetic() -> tuple[bytes, int, bytes]:
    offset = len(PREFIX)
    return PREFIX + _section() + FOLLOWING_OWNER, offset, FOLLOWING_OWNER


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


class OptionInfoParserTests(unittest.TestCase):
    def test_all_331_rows_strings_and_exact_direct_block_boundary(self) -> None:
        data, offset, following_owner = _synthetic()
        parsed = parse_option_info_section(data, offset)
        self.assertEqual(
            (OPTION_COUNT, TAG_STRING_TABLE_INDEX, ROW_TAG_STRING_TABLE_INDEX),
            (331, 5073, 5072),
        )
        self.assertEqual(
            parsed.layout_sha256,
            "2a78db2e1b061c5335b953d0c59bf4fa80b7ae3bac7c796ff29b00adfecad90c",
        )
        self.assertEqual(len(parsed.rows), 331)
        self.assertEqual(data[parsed.end :], following_owner)
        self.assertEqual(
            parsed.sha256, hashlib.sha256(data[offset : parsed.end]).hexdigest()
        )
        for index in (0, 1, 4, 5, 127, 255, 330):
            with self.subTest(index=index):
                row = parsed.rows[index]
                name, desc = _strings(index)
                self.assertEqual(row.row_tag, (0x20 + index) & 0xFF)
                self.assertEqual((row.name.code_units, row.desc.code_units), (name, desc))
                self.assertEqual(data[row.offset : row.end], ROWS[index])
                self.assertEqual(row.size, len(ROWS[index]))

    def test_every_owned_byte_mutation_is_killed_or_changes_receipt(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_option_info_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[offset + relative] ^= 1
                try:
                    changed = parse_option_info_section(damaged, offset)
                except OptionInfoParseError:
                    continue
                self.assertNotEqual(changed, baseline)
                self.assertNotEqual(changed.sha256, baseline.sha256)

    def test_every_truncation_is_rejected(self) -> None:
        data, offset, _ = _synthetic()
        end = parse_option_info_section(data, offset).end
        for retained in range(end - offset):
            with self.subTest(retained=retained):
                with self.assertRaises(OptionInfoParseError):
                    parse_option_info_section(data[: offset + retained], offset)

    def test_following_direct_block_mutation_is_excluded(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_option_info_section(data, offset)
        damaged = bytearray(data)
        damaged[baseline.end] ^= 0xFF
        self.assertEqual(parse_option_info_section(damaged, offset), baseline)

    def test_outer_tag_and_both_string_lengths_fail_closed(self) -> None:
        data, offset, _ = _synthetic()
        parsed = parse_option_info_section(data, offset)
        row = parsed.rows[1]
        cases: dict[str, tuple[int, bytes]] = {
            "wrong outer tag": (offset, b"\x01"),
            "oversized name": (row.name.offset, struct.pack("<I", 0x10000)),
            "oversized desc": (row.desc.offset, struct.pack("<I", 0x10000)),
        }
        for name, (where, replacement) in cases.items():
            with self.subTest(name=name):
                damaged = bytearray(data)
                damaged[where : where + len(replacement)] = replacement
                with self.assertRaises(OptionInfoParseError):
                    parse_option_info_section(damaged, offset)

    def test_row_tags_and_utf16_code_units_are_preserved_without_normalization(self) -> None:
        data = bytearray(_section())
        baseline = parse_option_info_section(data, 0)
        first = baseline.rows[1]
        last = baseline.rows[-2]
        data[first.offset] = 0xE7
        data[last.offset] = 0xD6
        struct.pack_into("<H", data, first.name.offset + 4, 0)
        struct.pack_into("<H", data, last.desc.end - 2, 0xDFFF)
        changed = parse_option_info_section(data, 0)
        self.assertEqual((changed.rows[1].row_tag, changed.rows[-2].row_tag), (0xE7, 0xD6))
        self.assertEqual(changed.rows[1].name.code_units[0], 0)
        self.assertEqual(changed.rows[-2].desc.code_units[-1], 0xDFFF)
        self.assertNotEqual(changed.sha256, baseline.sha256)

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
        self.assertEqual(classes["OptionInfo"]["size"], 22508)
        self.assertEqual(classes["OptionData"]["size"], 68)
        self.assertEqual(classes["String"]["size"], 20)
        spans = (
            (0x0072C1E0, 89, "93fcab6ac7619e0e9786d0e819d6de794b37690729a27fed75f992981da25247"),
            (0x0072BF70, 55, "75598040114fceb4013c357737280a07fe8d8b0a0c2e3e81325eabca295c7806"),
            (0x00A1B2D0, 207, "301e6a00fb85903d10e1068d02ff73a58aa85dc72ed640f32c3a2791003805b2"),
            (0x005A2C07, 44, "77143fe5103aece817538d2756a852501545f8343c309a6d8cd7edf0796fb981"),
            (0x0047EA30, 494, "8ebb1587588ee7a415b6e831b7c79bf279821054a244885d0d9e42f64e8b3d92"),
        )
        for va, size, digest in spans:
            with self.subTest(va=f"{va:#x}"):
                raw = _pe_offset(image, va)
                self.assertEqual(hashlib.sha256(image[raw : raw + size]).hexdigest(), digest)

    def test_fresh_svx_chains_to_exact_direct_block_boundary_without_replay_join(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]
        save = root / "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX"
        replay = root / "ron-data/replays/Playback - 2026.08.11 11'44'38 (Tue).rcx"
        if not save.exists() or not replay.exists():
            self.skipTest("user-owned fresh SVX/RCX fixtures are not installed")
        save_raw, replay_raw = save.read_bytes(), replay.read_bytes()
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
        forms = parse_forms_section(save_plain, cities.end)
        goods = parse_goods_section(save_plain, forms.end)
        items = parse_items_section(save_plain, goods.end)
        heroes = parse_heroes_section(save_plain, items.end)
        herds = parse_herds_section(save_plain, heroes.end)
        specials = parse_specials_section(save_plain, herds.end)
        wonders = parse_wonders_section(save_plain, specials.end)
        forts = parse_forts_section(save_plain, wonders.end)
        docks = parse_docks_section(save_plain, forts.end)
        oil_wells = parse_oil_wells_section(save_plain, docks.end)
        supplies = parse_supplies_section(save_plain, oil_wells.end)
        caravans = parse_caravans_section(save_plain, supplies.end)
        lands = parse_lands_section(save_plain, caravans.end)
        leader_options = parse_leader_options_section(save_plain, lands.end)
        self.assertEqual(leader_options.end, 0x2725A)
        parsed = parse_option_info_section(save_plain, leader_options.end)
        self.assertEqual((parsed.offset, parsed.end, parsed.size), (0x2725A, 0x27DFE, 2980))
        self.assertEqual(parsed.tag, 0)
        self.assertEqual([row.row_tag for row in parsed.rows], [0] * 331)
        self.assertEqual([(row.name.length, row.desc.length) for row in parsed.rows], [(0, 0)] * 331)
        self.assertEqual(parsed.sha256, "646ae72e39ac99003e8171cc17013d6da2017b6a477cff68c5f62aa9bdc65c0a")
        changed = bytearray(save_plain)
        changed[parsed.end] ^= 1
        self.assertEqual(parse_option_info_section(changed, leader_options.end), parsed)

        save_tree = savegame_parse.parse(save_plain)
        replay_tree = savegame_parse.parse(gzip.decompress(replay_raw))
        save_seed = _field(_find(save_tree, "GameInfo::walk_data"), "seed")
        replay_seed = _field(_find(replay_tree, "GameInfo::walk_data"), "seed")
        self.assertEqual((save_seed, replay_seed), ("0x014810ac", "0x007f93e0"))
        self.assertNotEqual(save_seed, replay_seed)


if __name__ == "__main__":
    unittest.main()
