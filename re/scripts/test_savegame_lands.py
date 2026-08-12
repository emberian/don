#!/usr/bin/env python3
"""Exact structural, mutation, artifact, and authority tests for Lands saves."""

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
from savegame_lands import (  # noqa: E402
    LAND_TAG_STRING_TABLE_INDEX,
    TAG_STRING_TABLE_INDEX,
    LandsParseError,
    parse_lands_section,
)
from savegame_leaders import parse_leaders_section  # noqa: E402
from savegame_mountains import parse_mountains_section  # noqa: E402
from savegame_oil_wells import parse_oil_wells_section  # noqa: E402
from savegame_specials import parse_specials_section  # noqa: E402
from savegame_supplies import parse_supplies_section  # noqa: E402
from savegame_tileset import parse_tileset_section  # noqa: E402
from savegame_types import parse_types_section  # noqa: E402
from savegame_wonders import parse_wonders_section  # noqa: E402


PREFIX = b"PRE-LANDS"
FOLLOWING_OWNER = b"LEADER-OPTIONS-SENTINEL" * 32


def _wide(*code_units: int) -> bytes:
    return struct.pack("<I", len(code_units)) + struct.pack(
        f"<{len(code_units)}H", *code_units
    )


POD0_VALUES = tuple(range(-33, 33))
POD1_VALUES = tuple(1000 + value * 7 for value in range(66))
POD0 = struct.pack("<66i", *POD0_VALUES)
POD1 = struct.pack("<66i", *POD1_VALUES)
NAME0 = _wide(0x004C, 0x0061, 0x006E, 0x0064)
KEY0 = _wide(0x004B, 0x0000, 0xD800)
NAME1 = _wide()
KEY1 = _wide(0x03A9)
ROW0 = b"\x41" + POD0 + NAME0 + KEY0
ROW1 = b"\x42" + POD1 + NAME1 + KEY1


def _section(capacity: int = 5, increment: int = -7, flags: int = 0x25) -> bytes:
    return (
        b"\0"
        + struct.pack("<iihB", 2, capacity, increment, flags)
        + ROW0
        + ROW1
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


class LandsParserTests(unittest.TestCase):
    def test_complete_history_pod_strings_and_exact_leader_options_boundary(self) -> None:
        data, offset, following_owner = _synthetic()
        parsed = parse_lands_section(data, offset)
        self.assertEqual(
            (TAG_STRING_TABLE_INDEX, LAND_TAG_STRING_TABLE_INDEX), (4606, 4604)
        )
        self.assertEqual(
            (parsed.length, parsed.capacity, parsed.increment, parsed.flags),
            (2, 5, -7, 0x25),
        )
        self.assertEqual(
            parsed.layout_sha256,
            "04016d308702adac2ef7228aa57022f7e14aea11a34b3820fd05f29b0b39aeb1",
        )
        self.assertEqual(data[parsed.end :], following_owner)
        self.assertEqual(
            parsed.sha256, hashlib.sha256(data[offset : parsed.end]).hexdigest()
        )
        first, second = parsed.rows
        self.assertEqual((first.row_tag, second.row_tag), (0x41, 0x42))
        self.assertEqual((first.pod_end - first.pod_offset, second.pod_end - second.pod_offset), (264, 264))
        self.assertEqual([field.name for field in first.fields], [
            "land", "make", "num_make", "special", "special_sum",
            "river_mask", "river_bed", "river_cost", "num_rare", "rare",
            "move_rate", "combat_bonus",
        ])
        self.assertEqual(first.fields[0].values, (-33,))
        self.assertEqual(first.fields[1].values, (-32, -31, -30, -29))
        self.assertEqual(first.fields[9].values, tuple(range(-13, 31)))
        self.assertEqual(first.fields[-1].values, (32,))
        self.assertEqual((first.name.length, first.name.code_units), (4, (0x4C, 0x61, 0x6E, 0x64)))
        self.assertEqual((first.key.length, first.key.code_units), (3, (0x4B, 0, 0xD800)))
        self.assertEqual((second.name.length, second.key.code_units), (0, (0x03A9,)))
        self.assertEqual((first.size, second.size), (287, 275))

    def test_every_owned_byte_mutation_is_killed_or_changes_receipt(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_lands_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[offset + relative] ^= 1
                try:
                    changed = parse_lands_section(damaged, offset)
                except LandsParseError:
                    continue
                self.assertNotEqual(changed, baseline)
                self.assertNotEqual(changed.sha256, baseline.sha256)

    def test_every_truncation_is_rejected(self) -> None:
        data, offset, _ = _synthetic()
        end = parse_lands_section(data, offset).end
        for retained in range(end - offset):
            with self.subTest(retained=retained):
                with self.assertRaises(LandsParseError):
                    parse_lands_section(data[: offset + retained], offset)

    def test_following_leader_options_mutation_is_excluded(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_lands_section(data, offset)
        damaged = bytearray(data)
        damaged[baseline.end] ^= 0xFF
        self.assertEqual(parse_lands_section(damaged, offset), baseline)

    def test_container_and_string_invariants_fail_closed(self) -> None:
        data, offset, _ = _synthetic()
        parsed = parse_lands_section(data, offset)
        cases: dict[str, tuple[int, bytes]] = {
            "wrong tag": (offset, b"\x01"),
            "negative length": (offset + 1, struct.pack("<i", -1)),
            "oversized length": (offset + 1, struct.pack("<i", (1 << 20) + 1)),
            "capacity below length": (offset + 5, struct.pack("<i", 1)),
            "oversized capacity": (offset + 5, struct.pack("<i", (1 << 20) + 1)),
            "writer-cleared flags": (offset + 11, b"\x65"),
            "oversized name": (parsed.rows[0].name.offset, struct.pack("<I", 0x10000)),
            "oversized key": (parsed.rows[0].key.offset, struct.pack("<I", 0x10000)),
        }
        for name, (where, replacement) in cases.items():
            with self.subTest(name=name):
                damaged = bytearray(data)
                damaged[where : where + len(replacement)] = replacement
                with self.assertRaises(LandsParseError):
                    parse_lands_section(damaged, offset)

    def test_valid_history_tags_and_exact_walked_projection_are_preserved(self) -> None:
        baseline = parse_lands_section(_section(), 0)
        changed_data = _section(99, 17, 0x2D)
        changed = parse_lands_section(changed_data, 0)
        self.assertEqual((changed.capacity, changed.increment, changed.flags), (99, 17, 0x2D))
        self.assertEqual(changed.end, baseline.end)
        first, second = changed.rows
        self.assertEqual(changed_data[first.offset : first.end], ROW0)
        self.assertEqual(changed_data[second.offset : second.end], ROW1)
        self.assertEqual(changed_data[first.pod_offset : first.pod_end], POD0)
        self.assertEqual(changed_data[first.name.offset : first.name.end], NAME0)
        self.assertEqual(changed_data[first.key.offset : first.key.end], KEY0)

        retagged = bytearray(changed_data)
        retagged[first.offset] = 0xE7
        retagged[second.offset] = 0xD6
        tags = parse_lands_section(retagged, 0)
        self.assertEqual([row.row_tag for row in tags.rows], [0xE7, 0xD6])

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
        self.assertEqual(classes["Lands"]["size"], 32)
        self.assertEqual(classes["LandsData"]["size"], 24)
        self.assertEqual(classes["ObjectArray<Land>"]["size"], 24)
        self.assertEqual(classes["Land"]["size"], 312)
        self.assertEqual(classes["LandData"]["size"], 304)
        self.assertEqual(classes["String"]["size"], 20)
        spans = (
            (0x0067E700, 40, "9ed4f708d2f4395d658fd3af70126405645c1496b65574f9f4ffa3cbbf3f3748"),
            (0x004786F0, 538, "1f35d8335db5bce4f2d7351a38abcc64b8b3d624d2112af8775fbbfb594ef178"),
            (0x0067E680, 73, "11f66b4c92cd66cbfaebdbc499be842942d52e911222cbeccb84f11acb65611c"),
            (0x00A1B2D0, 207, "301e6a00fb85903d10e1068d02ff73a58aa85dc72ed640f32c3a2791003805b2"),
            (0x005A2BD1, 44, "9a957b2cbe8c8f73760fad08af88540eab930a8e03b10c1c9aeccfb6fa633c90"),
            (0x006F19A0, 179, "80833da1c6a3adbe29304b65f7f4ad82157fe1f7dfc2200798c445e5e5823c4d"),
        )
        for va, size, digest in spans:
            with self.subTest(va=f"{va:#x}"):
                raw = _pe_offset(image, va)
                self.assertEqual(hashlib.sha256(image[raw : raw + size]).hexdigest(), digest)

    def test_fresh_svx_chains_to_exact_leader_options_boundary_without_replay_join(self) -> None:
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
        self.assertEqual(caravans.end, 0x2715A)
        parsed = parse_lands_section(save_plain, caravans.end)
        self.assertEqual((parsed.offset, parsed.end, parsed.size), (0x2715A, 0x2715F, 5))
        self.assertEqual((parsed.tag, parsed.length), (0, 0))
        self.assertEqual(parsed.sha256, "8855508aade16ec573d21e6a485dfd0a7624085c1a14b5ecdd6485de0c6839a4")
        changed = bytearray(save_plain)
        changed[parsed.end] ^= 1
        self.assertEqual(parse_lands_section(changed, caravans.end), parsed)

        save_tree = savegame_parse.parse(save_plain)
        replay_tree = savegame_parse.parse(gzip.decompress(replay_raw))
        save_seed = _field(_find(save_tree, "GameInfo::walk_data"), "seed")
        replay_seed = _field(_find(replay_tree, "GameInfo::walk_data"), "seed")
        self.assertEqual((save_seed, replay_seed), ("0x014810ac", "0x007f93e0"))
        self.assertNotEqual(save_seed, replay_seed)


if __name__ == "__main__":
    unittest.main()
