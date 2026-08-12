#!/usr/bin/env python3
"""Mutation, truncation, PDB/PE, and SVX tests for HotKeyGroups."""

from __future__ import annotations

import gzip
import hashlib
import json
import pathlib
import struct
import sys
import tempfile
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
from savegame_groups_tail import parse_groups_tail_section  # noqa: E402
from savegame_herds import parse_herds_section  # noqa: E402
from savegame_heroes import parse_heroes_section  # noqa: E402
from savegame_hotkey_groups import (  # noqa: E402
    OUTER_TAG_STRING_TABLE_INDEX,
    ROW_TAG_STRING_TABLE_INDEX,
    HotKeyGroupsParseError,
    parse_hotkey_groups_section,
)
from savegame_items import parse_items_section  # noqa: E402
from savegame_lands import parse_lands_section  # noqa: E402
from savegame_leader_options import parse_leader_options_section  # noqa: E402
from savegame_leaders import parse_leaders_section  # noqa: E402
from savegame_mountains import parse_mountains_section  # noqa: E402
from savegame_objects import parse_objects_section  # noqa: E402
from savegame_oil_wells import parse_oil_wells_section  # noqa: E402
from savegame_option_info import parse_option_info_section  # noqa: E402
from savegame_pathfinder_groups import parse_pathfinder_groups_section  # noqa: E402
from savegame_specials import parse_specials_section  # noqa: E402
from savegame_supplies import parse_supplies_section  # noqa: E402
from savegame_tileset import parse_tileset_section  # noqa: E402
from savegame_types import parse_types_section  # noqa: E402
from savegame_wonders import parse_wonders_section  # noqa: E402


PREFIX = b"PRE-HOTKEY-GROUPS"
FOLLOWING_OWNER = b"WORLD-SENTINEL" * 64
EXPECTED_LAYOUT_SHA256 = "7f6450dae5436ae33a9e86b599ce6fdd3c688017c6c7d966034662b4211b90ca"


def _row(
    index: int,
    num: int,
    *,
    tag: int = 0,
    loc_x_bits: int = 0x3F800000,
    loc_y_bits: int = 0xC0200000,
    valid: int = -1,
) -> bytes:
    words = tuple(index * 100 + value for value in range(17))
    words = words[:2] + (num,) + words[3:]
    core = struct.pack("<17i4B", *words, 0x81, 0x02, 0x09, 0xFF)
    members = tuple((-1 if value % 2 == 0 else 1) * (value + 1) for value in range(num))
    off_x = tuple(-100 - value for value in range(num))
    off_y = tuple(200 + value for value in range(num))
    curr_x = tuple(-300 - value for value in range(num))
    curr_y = tuple(400 + value for value in range(num))
    angles = tuple(-1 - value for value in range(num))
    dynamic = b"".join(
        (
            struct.pack(f"<{num}h", *members) if num else b"",
            struct.pack(f"<{num}i", *off_x) if num else b"",
            struct.pack(f"<{num}i", *off_y) if num else b"",
            struct.pack(f"<{num}i", *curr_x) if num else b"",
            struct.pack(f"<{num}i", *curr_y) if num else b"",
            struct.pack(f"<{num}b", *angles) if num else b"",
        )
    )
    return core + dynamic + bytes((tag,)) + struct.pack("<IIi", loc_x_bits, loc_y_bits, valid)


def _section(*, outer_tag: int = 0, row_tag: int = 0) -> bytes:
    header = bytes((outer_tag,)) + struct.pack("<iihB", 2, 4, -3, 0x02)
    return header + _row(0, 0, tag=row_tag) + _row(
        1,
        3,
        tag=row_tag,
        loc_x_bits=0x7FC01234,
        loc_y_bits=0x80000000,
        valid=-0x1234567,
    )


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


class HotKeyGroupsParserTests(unittest.TestCase):
    def test_exact_count_dependent_rows_and_world_boundary(self) -> None:
        data, offset, following = _synthetic()
        parsed = parse_hotkey_groups_section(data, offset)
        self.assertEqual((OUTER_TAG_STRING_TABLE_INDEX, ROW_TAG_STRING_TABLE_INDEX), (3963, 3962))
        self.assertEqual((parsed.tag, parsed.length, parsed.capacity, parsed.increment, parsed.flags), (0, 2, 4, -3, 2))
        self.assertEqual(len(parsed.rows), 2)
        first, second = parsed.rows
        self.assertEqual(first.num, 0)
        self.assertIsNone(first.list_offset)
        self.assertEqual((first.members, first.off_x, first.off_y, first.curr_x, first.curr_y, first.angles), ((),) * 6)
        self.assertEqual(second.num, 3)
        self.assertEqual(second.members, (-1, 2, -3))
        self.assertEqual(second.off_x, (-100, -101, -102))
        self.assertEqual(second.off_y, (200, 201, 202))
        self.assertEqual(second.curr_x, (-300, -301, -302))
        self.assertEqual(second.curr_y, (400, 401, 402))
        self.assertEqual(second.angles, (-1, -2, -3))
        self.assertEqual((second.facing, second.buildings, second.who, second.march), (0x81, 2, 9, 0xFF))
        self.assertEqual((second.loc_x_bits, second.loc_y_bits, second.valid), (0x7FC01234, 0x80000000, -0x1234567))
        self.assertEqual(parsed.layout_sha256, EXPECTED_LAYOUT_SHA256)
        self.assertEqual(data[parsed.end :], following)
        self.assertEqual(parsed.sha256, hashlib.sha256(data[offset : parsed.end]).hexdigest())

    def test_every_owned_byte_mutation_changes_receipt_or_is_rejected(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_hotkey_groups_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[offset + relative] ^= 1
                try:
                    changed = parse_hotkey_groups_section(damaged, offset)
                except HotKeyGroupsParseError:
                    continue
                self.assertNotEqual(changed.sha256, baseline.sha256)
                self.assertNotEqual(changed, baseline)

    def test_every_truncation_is_rejected(self) -> None:
        data, offset, _ = _synthetic()
        end = parse_hotkey_groups_section(data, offset).end
        for retained in range(end - offset):
            with self.subTest(retained=retained):
                with self.assertRaises(HotKeyGroupsParseError):
                    parse_hotkey_groups_section(data[: offset + retained], offset)

    def test_world_mutation_is_excluded(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_hotkey_groups_section(data, offset)
        damaged = bytearray(data)
        damaged[baseline.end] ^= 0xFF
        self.assertEqual(parse_hotkey_groups_section(damaged, offset), baseline)

    def test_tag_history_and_member_bounds_fail_closed(self) -> None:
        with self.assertRaises(HotKeyGroupsParseError):
            parse_hotkey_groups_section(_section(outer_tag=1), 0)
        with self.assertRaises(HotKeyGroupsParseError):
            parse_hotkey_groups_section(_section(row_tag=1), 0)
        permissive = parse_hotkey_groups_section(
            _section(outer_tag=0xE7, row_tag=0xA5),
            0,
            require_tag=False,
            require_row_tags=False,
        )
        self.assertEqual(permissive.tag, 0xE7)
        self.assertEqual([row.tag for row in permissive.rows], [0xA5, 0xA5])

        words = tuple(range(17))
        words = words[:2] + (129,) + words[3:]
        too_many = (
            bytes((0,))
            + struct.pack("<iihB", 1, 1, 1, 0)
            + struct.pack("<17i4B", *words, 0, 0, 0, 0)
        )
        with self.assertRaisesRegex(HotKeyGroupsParseError, "128-member"):
            parse_hotkey_groups_section(too_many, 0)
        bad_capacity = bytearray(_section())
        struct.pack_into("<i", bad_capacity, 5, 1)
        with self.assertRaisesRegex(HotKeyGroupsParseError, "below length"):
            parse_hotkey_groups_section(bad_capacity, 0)
        bad_flags = bytearray(_section())
        bad_flags[11] |= 0x40
        with self.assertRaisesRegex(HotKeyGroupsParseError, "writer-cleared"):
            parse_hotkey_groups_section(bad_flags, 0)

    def test_pdb_layout_mutation_and_executable_bodies_are_frozen(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]
        schema = root / "schema/pdb-types.json"
        pdb = root / "ron-bin/sbl/rise.pdb"
        exe = root / "ron-bin/riseofnations.exe"
        if not schema.exists():
            self.skipTest(f"matched schema is not installed: {schema}")
        classes = json.loads(schema.read_text())["classes"]
        names = (
            "Array<HotKeyGroup>", "GroupData", "GroupOut", "Group",
            "HotKeyGroupData", "HotKeyGroupOut", "HotKeyGroup",
        )
        subset = {name: classes[name] for name in names}
        subset["GroupData"] = dict(subset["GroupData"])
        subset["GroupData"]["size"] = 2507
        with tempfile.TemporaryDirectory() as directory:
            bad = pathlib.Path(directory) / "pdb-types.json"
            bad.write_text(json.dumps({"classes": subset}))
            with self.assertRaisesRegex(HotKeyGroupsParseError, "GroupData layout disagrees"):
                parse_hotkey_groups_section(_section(), 0, schema_path=bad)

        for path in (pdb, exe):
            if not path.exists():
                self.skipTest(f"matched artifact is not installed: {path}")
        self.assertEqual(hashlib.sha256(schema.read_bytes()).hexdigest(), "399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14")
        self.assertEqual(hashlib.sha256(pdb.read_bytes()).hexdigest(), "334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5")
        image = exe.read_bytes()
        self.assertEqual(hashlib.sha256(image).hexdigest(), "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079")
        spans = (
            (0x00480290, 526, "4e1dfe9d96758c1d299ba00db75095612fcd87fe3bb6f462a83e96eb16b027a7"),
            (0x00708400, 181, "4b69d8857b8c760dc33641a2b1fc344e76bbac3d03d6bb1bf804bbc74fcd794f"),
            (0x005A2D88, 28, "68e71570bb3be91d13eb340791a7b7e25cd4603e2929a1e9d8e7eda606fd39cc"),
            (0x006B5CF0, 903, "adf50b4197020da3932b562442b1cf7d8e80443da42f6181ebd23276bd080e01"),
        )
        for va, size, digest in spans:
            with self.subTest(va=f"{va:#x}"):
                raw = _pe_offset(image, va)
                self.assertEqual(hashlib.sha256(image[raw : raw + size]).hexdigest(), digest)

    def test_fresh_svx_chains_to_exact_world_boundary_without_replay_join(self) -> None:
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
        groups = parse_groups_tail_section(save_plain, pathfinder_groups.end)
        objects = parse_objects_section(save_plain, groups.end)
        self.assertEqual(objects.end, 0x27F4E)
        parsed = parse_hotkey_groups_section(save_plain, objects.end)
        self.assertEqual((parsed.offset, parsed.end, parsed.size), (0x27F4E, 0x27F53, 5))
        self.assertEqual((parsed.tag, parsed.length, parsed.rows), (0, 0, ()))
        self.assertEqual(parsed.sha256, "8855508aade16ec573d21e6a485dfd0a7624085c1a14b5ecdd6485de0c6839a4")
        changed = bytearray(save_plain)
        changed[parsed.end] ^= 1
        self.assertEqual(parse_hotkey_groups_section(changed, objects.end), parsed)

        save_tree = savegame_parse.parse(save_plain)
        replay_tree = savegame_parse.parse(gzip.decompress(replay_raw))
        save_seed = _field(_find(save_tree, "GameInfo::walk_data"), "seed")
        replay_seed = _field(_find(replay_tree, "GameInfo::walk_data"), "seed")
        self.assertEqual((save_seed, replay_seed), ("0x014810ac", "0x007f93e0"))
        self.assertNotEqual(save_seed, replay_seed)


if __name__ == "__main__":
    unittest.main()
