#!/usr/bin/env python3
"""Exact mutation, artifact, and boundary tests for the Groups save tail."""

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
from savegame_groups_tail import (  # noqa: E402
    TAG_STRING_TABLE_INDEX,
    GroupsTailParseError,
    parse_groups_tail_section,
)
from savegame_herds import parse_herds_section  # noqa: E402
from savegame_heroes import parse_heroes_section  # noqa: E402
from savegame_items import parse_items_section  # noqa: E402
from savegame_lands import parse_lands_section  # noqa: E402
from savegame_leader_options import parse_leader_options_section  # noqa: E402
from savegame_leaders import parse_leaders_section  # noqa: E402
from savegame_mountains import parse_mountains_section  # noqa: E402
from savegame_oil_wells import parse_oil_wells_section  # noqa: E402
from savegame_option_info import parse_option_info_section  # noqa: E402
from savegame_pathfinder_groups import parse_pathfinder_groups_section  # noqa: E402
from savegame_specials import parse_specials_section  # noqa: E402
from savegame_supplies import parse_supplies_section  # noqa: E402
from savegame_tileset import parse_tileset_section  # noqa: E402
from savegame_types import parse_types_section  # noqa: E402
from savegame_wonders import parse_wonders_section  # noqa: E402


PREFIX = b"PRE-GROUPS-TAIL"
FOLLOWING_OWNER = b"OBJECTS-SENTINEL" * 32
LAST_GROUP = (-1, 2, -3, 4, -5, 6, -7, 8)
PROC_GROUP = -0x1234567


def _section(tag: int = 0) -> bytes:
    return bytes((tag,)) + struct.pack("<8ii", *LAST_GROUP, PROC_GROUP)


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


class GroupsTailParserTests(unittest.TestCase):
    def test_exact_fields_and_objects_boundary(self) -> None:
        data, offset, following_owner = _synthetic()
        parsed = parse_groups_tail_section(data, offset)
        self.assertEqual(TAG_STRING_TABLE_INDEX, 2920)
        self.assertEqual(parsed.tag, 0)
        self.assertEqual(parsed.last_group, LAST_GROUP)
        self.assertEqual(parsed.proc_group, PROC_GROUP)
        self.assertEqual(parsed.last_group_offset, offset + 1)
        self.assertEqual(parsed.proc_group_offset, offset + 33)
        self.assertEqual(parsed.size, 37)
        self.assertEqual(
            parsed.layout_sha256,
            "e3837df2200337b43c2d74847af1493eb9448c3ac5e3c2d8ce1acf51ceba2196",
        )
        self.assertEqual(data[parsed.end :], following_owner)
        self.assertEqual(
            parsed.sha256, hashlib.sha256(data[offset : parsed.end]).hexdigest()
        )

    def test_every_owned_byte_mutation_changes_receipt_or_is_rejected(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_groups_tail_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[offset + relative] ^= 1
                try:
                    changed = parse_groups_tail_section(damaged, offset)
                except GroupsTailParseError:
                    continue
                self.assertNotEqual(changed, baseline)
                self.assertNotEqual(changed.sha256, baseline.sha256)

    def test_every_truncation_is_rejected(self) -> None:
        data, offset, _ = _synthetic()
        end = parse_groups_tail_section(data, offset).end
        for retained in range(end - offset):
            with self.subTest(retained=retained):
                with self.assertRaises(GroupsTailParseError):
                    parse_groups_tail_section(data[: offset + retained], offset)

    def test_following_objects_mutation_is_excluded(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_groups_tail_section(data, offset)
        damaged = bytearray(data)
        damaged[baseline.end] ^= 0xFF
        self.assertEqual(parse_groups_tail_section(damaged, offset), baseline)

    def test_outer_tag_fails_closed_but_signed_scalars_round_trip(self) -> None:
        with self.assertRaises(GroupsTailParseError):
            parse_groups_tail_section(_section(tag=1), 0)
        permissive = parse_groups_tail_section(_section(tag=0xE7), 0, require_tag=False)
        self.assertEqual(permissive.tag, 0xE7)
        self.assertEqual(permissive.last_group, LAST_GROUP)
        self.assertEqual(permissive.proc_group, PROC_GROUP)

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
        self.assertEqual(classes["Groups"]["size"], 76)
        self.assertEqual(classes["GroupsData"]["size"], 68)
        self.assertEqual(classes["GroupsOut"]["size"], 72)
        spans = (
            (0x00713E30, 72, "0b2f56fd40a7403c610058a615a61afaa5db4b6b1963d9535e4fbd03c66405ee"),
            (0x005A2C38, 75, "fb6301aad3a1e6c9524c9aff495ec6a9b7eb8d4912596b77625b0f2fa2a31252"),
            (0x006541E0, 469, "f5c338b7e518b05f9c192545773cdf092d9d11764bdb26c5e00dec01710ff8e8"),
        )
        for va, size, digest in spans:
            with self.subTest(va=f"{va:#x}"):
                raw = _pe_offset(image, va)
                self.assertEqual(hashlib.sha256(image[raw : raw + size]).hexdigest(), digest)

    def test_fresh_svx_chains_to_exact_objects_boundary_without_replay_join(self) -> None:
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
        option_info = parse_option_info_section(save_plain, leader_options.end)
        pathfinder_groups = parse_pathfinder_groups_section(save_plain, option_info.end)
        self.assertEqual(pathfinder_groups.end, 0x27E6E)
        parsed = parse_groups_tail_section(save_plain, pathfinder_groups.end)
        self.assertEqual((parsed.offset, parsed.end, parsed.size), (0x27E6E, 0x27E93, 37))
        self.assertEqual((parsed.tag, parsed.last_group, parsed.proc_group), (0, (0,) * 8, 0))
        self.assertEqual(parsed.sha256, "ab24a95f44ceca5d2aed4b6d056adddd8539f44c6cd6ca506534e830c82ea8a8")
        changed = bytearray(save_plain)
        changed[parsed.end] ^= 1
        self.assertEqual(parse_groups_tail_section(changed, pathfinder_groups.end), parsed)

        save_tree = savegame_parse.parse(save_plain)
        replay_tree = savegame_parse.parse(gzip.decompress(replay_raw))
        save_seed = _field(_find(save_tree, "GameInfo::walk_data"), "seed")
        replay_seed = _field(_find(replay_tree, "GameInfo::walk_data"), "seed")
        self.assertEqual((save_seed, replay_seed), ("0x014810ac", "0x007f93e0"))
        self.assertNotEqual(save_seed, replay_seed)


if __name__ == "__main__":
    unittest.main()
