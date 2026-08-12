#!/usr/bin/env python3
"""Exact mutation, artifact, and boundary tests for PathFinder plus Groups."""

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
from savegame_option_info import parse_option_info_section  # noqa: E402
from savegame_pathfinder_groups import (  # noqa: E402
    NEXT_TAG_STRING_TABLE_INDEX,
    PathfinderGroupsParseError,
    parse_pathfinder_groups_section,
)
from savegame_specials import parse_specials_section  # noqa: E402
from savegame_supplies import parse_supplies_section  # noqa: E402
from savegame_tileset import parse_tileset_section  # noqa: E402
from savegame_types import parse_types_section  # noqa: E402
from savegame_wonders import parse_wonders_section  # noqa: E402


PREFIX = b"PRE-PATHFINDER-GROUPS"
FOLLOWING_OWNER = b"GROUPS-TAIL-TAG-SENTINEL" * 32
PATH_VALUES = tuple(-500 + index * 37 for index in range(27))
PATH_BYTES = struct.pack("<27i", *PATH_VALUES)


def _fixed(index: int, num: int) -> bytes:
    ints = [index * 100 + value - 50 for value in range(17)]
    ints[2] = num
    return struct.pack("<17iBBBB", *ints, 0xE1 - index, 7 + index, 8 + index, 9 + index)


ROW0 = _fixed(0, 0)
ROW1 = (
    _fixed(1, 3)
    + struct.pack("<3h", -1, 2, 300)
    + struct.pack("<3i", 11, -12, 13)
    + struct.pack("<3i", -21, 22, -23)
    + struct.pack("<3i", 31, -32, 33)
    + struct.pack("<3i", -41, 42, -43)
    + struct.pack("<3b", -7, 8, -9)
)


def _groups(capacity: int = 5, increment: int = -3, flags: int = 0x21) -> bytes:
    return struct.pack("<iihB", 2, capacity, increment, flags) + ROW0 + ROW1


def _section(capacity: int = 5, increment: int = -3, flags: int = 0x21) -> bytes:
    return PATH_BYTES + _groups(capacity, increment, flags)


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


class PathfinderGroupsParserTests(unittest.TestCase):
    def test_pathfinder_history_and_dynamic_groups_exact_next_tag_boundary(self) -> None:
        data, offset, following_owner = _synthetic()
        parsed = parse_pathfinder_groups_section(data, offset)
        self.assertEqual(NEXT_TAG_STRING_TABLE_INDEX, 2920)
        self.assertEqual(
            parsed.layout_sha256,
            "e6ba89e00795a435dbe4686fdc02d9202d6887c5814b61fde7b9e2db63cefe98",
        )
        self.assertEqual(parsed.pathfinder.size, 108)
        self.assertEqual([field.value for field in parsed.pathfinder.fields], list(PATH_VALUES))
        self.assertEqual(
            [field.name for field in parsed.pathfinder.fields],
            [
                "sx", "sy", "dbg_collisions", "anti_unit", "offx", "offy",
                "army", "iroquois", "worker", "no_danger", "limit", "saving",
                "avoid_land", "avoid_sea", "valid_hit", "scouting", "can_transport",
                "dbg_view_failures", "road_base_val", "road_avoid_sea",
                "road_cross_coast", "road_enemy", "road_noone", "road_bad_path",
                "road_river", "road_z_max", "road_diag_penalty",
            ],
        )
        groups = parsed.groups
        self.assertEqual((groups.length, groups.capacity, groups.increment, groups.flags), (2, 5, -3, 0x21))
        self.assertEqual(len(groups.rows), 2)
        first, second = groups.rows
        self.assertEqual((first.num, first.size, second.num, second.size), (0, 72, 3, 129))
        self.assertEqual(second.member_ids, (-1, 2, 300))
        self.assertEqual(second.off_x, (11, -12, 13))
        self.assertEqual(second.off_y, (-21, 22, -23))
        self.assertEqual(second.curr_x, (31, -32, 33))
        self.assertEqual(second.curr_y, (-41, 42, -43))
        self.assertEqual(second.angles, (-7, 8, -9))
        self.assertEqual(data[first.offset : first.end], ROW0)
        self.assertEqual(data[second.offset : second.end], ROW1)
        self.assertEqual(data[parsed.end :], following_owner)
        self.assertEqual(parsed.sha256, hashlib.sha256(data[offset : parsed.end]).hexdigest())

    def test_every_owned_byte_mutation_is_killed_or_changes_receipt(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_pathfinder_groups_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[offset + relative] ^= 1
                try:
                    changed = parse_pathfinder_groups_section(damaged, offset)
                except PathfinderGroupsParseError:
                    continue
                self.assertNotEqual(changed, baseline)
                self.assertNotEqual(changed.sha256, baseline.sha256)

    def test_every_truncation_is_rejected(self) -> None:
        data, offset, _ = _synthetic()
        end = parse_pathfinder_groups_section(data, offset).end
        for retained in range(end - offset):
            with self.subTest(retained=retained):
                with self.assertRaises(PathfinderGroupsParseError):
                    parse_pathfinder_groups_section(data[: offset + retained], offset)

    def test_following_groups_tail_tag_mutation_is_excluded(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_pathfinder_groups_section(data, offset)
        damaged = bytearray(data)
        damaged[baseline.end] ^= 0xFF
        self.assertEqual(parse_pathfinder_groups_section(damaged, offset), baseline)

    def test_container_and_group_member_invariants_fail_closed(self) -> None:
        data, offset, _ = _synthetic()
        parsed = parse_pathfinder_groups_section(data, offset)
        groups = parsed.groups
        second = groups.rows[1]
        cases: dict[str, tuple[int, bytes]] = {
            "negative length": (groups.offset, struct.pack("<i", -1)),
            "oversized length": (groups.offset, struct.pack("<i", (1 << 20) + 1)),
            "capacity below length": (groups.offset + 4, struct.pack("<i", 1)),
            "oversized capacity": (groups.offset + 4, struct.pack("<i", (1 << 20) + 1)),
            "writer-cleared flags": (groups.offset + 10, b"\x61"),
            "negative num": (second.offset + 8, struct.pack("<i", -1)),
            "num above fixed arrays": (second.offset + 8, struct.pack("<i", 129)),
        }
        for name, (where, replacement) in cases.items():
            with self.subTest(name=name):
                damaged = bytearray(data)
                damaged[where : where + len(replacement)] = replacement
                with self.assertRaises(PathfinderGroupsParseError):
                    parse_pathfinder_groups_section(damaged, offset)

    def test_valid_container_history_and_signed_dynamic_arrays_are_preserved(self) -> None:
        baseline = parse_pathfinder_groups_section(_section(), 0)
        changed_data = _section(99, 17, 0x2D)
        changed = parse_pathfinder_groups_section(changed_data, 0)
        self.assertEqual((changed.groups.capacity, changed.groups.increment, changed.groups.flags), (99, 17, 0x2D))
        self.assertEqual(changed.end, baseline.end)
        second = changed.groups.rows[1]
        self.assertEqual(changed_data[second.offset : second.end], ROW1)
        self.assertEqual(second.fields[17].value, 0xE0)
        self.assertEqual(second.angles, (-7, 8, -9))

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
        self.assertEqual(classes["PathFinder"]["size"], 204)
        self.assertEqual(classes["PathFinderData"]["size"], 136)
        self.assertEqual(classes["Array<Group>"]["size"], 28)
        self.assertEqual(classes["Group"]["size"], 2516)
        self.assertEqual(classes["GroupData"]["size"], 2508)
        spans = (
            (0x00689CB0, 24, "13f209249ee4b3a9e15cf3dfd2b9ea6194e5d5be21efb2bfa295182ae739da16"),
            (0x0047EA30, 494, "8ebb1587588ee7a415b6e831b7c79bf279821054a244885d0d9e42f64e8b3d92"),
            (0x00708400, 181, "4b69d8857b8c760dc33641a2b1fc344e76bbac3d03d6bb1bf804bbc74fcd794f"),
            (0x005A2C17, 82, "4f63e51d8c33dd1a1f7850f07344a7469a2e8dea6fb73f3df413d38d6a98e6c5"),
        )
        for va, size, digest in spans:
            with self.subTest(va=f"{va:#x}"):
                raw = _pe_offset(image, va)
                self.assertEqual(hashlib.sha256(image[raw : raw + size]).hexdigest(), digest)

    def test_fresh_svx_chains_to_exact_groups_tail_tag_without_replay_join(self) -> None:
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
        self.assertEqual(option_info.end, 0x27DFE)
        parsed = parse_pathfinder_groups_section(save_plain, option_info.end)
        self.assertEqual((parsed.offset, parsed.end, parsed.size), (0x27DFE, 0x27E6E, 112))
        self.assertEqual([field.value for field in parsed.pathfinder.fields], [0] * 27)
        self.assertEqual(parsed.pathfinder.sha256, "77133f431d5e12dd850002c0d3d4e0fecbe3a7a699d604dc8c5eae9976e1d260")
        self.assertEqual((parsed.groups.length, parsed.groups.size), (0, 4))
        self.assertEqual(parsed.sha256, "b5fdab78d8947eacc864bfeecb4d2100780e5afe1cd8efafb124887913ac49fa")
        changed = bytearray(save_plain)
        changed[parsed.end] ^= 1
        self.assertEqual(parse_pathfinder_groups_section(changed, option_info.end), parsed)

        save_tree = savegame_parse.parse(save_plain)
        replay_tree = savegame_parse.parse(gzip.decompress(replay_raw))
        save_seed = _field(_find(save_tree, "GameInfo::walk_data"), "seed")
        replay_seed = _field(_find(replay_tree, "GameInfo::walk_data"), "seed")
        self.assertEqual((save_seed, replay_seed), ("0x014810ac", "0x007f93e0"))
        self.assertNotEqual(save_seed, replay_seed)


if __name__ == "__main__":
    unittest.main()
