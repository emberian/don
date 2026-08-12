#!/usr/bin/env python3
"""Exact mutation, truncation, PE/PDB, and SVX gates for World."""

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
from savegame_hotkey_groups import parse_hotkey_groups_section  # noqa: E402
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
from savegame_world import TAG_STRING_TABLE_INDEX, WorldParseError, parse_world_section  # noqa: E402


PREFIX = b"PRE-WORLD"
FOLLOWING_OWNER = b"GAME-DAEMON-SENTINEL" * 32
LAYOUT_SHA = "353dad0d87212ade4f9a9fd768eb7bd025a0dbefdbf8e65be05fc2b2c805134b"


def _array(length: int, element_size: int, seed: int) -> bytes:
    if length == 0:
        return struct.pack("<i", 0)
    data = bytes((seed + i) & 0xFF for i in range(length * element_size))
    return struct.pack("<iihB", length, length + 2, -3, 2) + data


def _section(tag: int = 0) -> bytes:
    arrays = b"".join(_array(length, 4, 0x10 + i * 0x10) for i, length in enumerate((2, 0, 1, 2, 1, 0)))
    words = list(range(-30, 0))
    words[0] = 2       # size
    words[3] = 3       # fog_size
    words[6] = 2       # tile_size
    words[9] = 2       # reg_size
    wdata = bytes(range(42))
    tdata = bytes(range(0x40, 0x44))
    seen = bytes(range(0x50, 0x59))
    wcoord_seen = b"\x60\x61"
    danger = bytes(range(0x70, 0xB0))
    collisions = struct.pack("<i", 0) + struct.pack("<iii", 1, 17, 3) + b"XYZ"
    terrain = b"".join((
        _array(2, 8, 0xC0), _array(1, 4, 0xD0),
        _array(0, 4, 0), _array(2, 4, 0xE0),
    ))
    return bytes((tag,)) + struct.pack("<2i", -111, 222) + arrays + struct.pack("<30i", *words) + wdata + tdata + seen + wcoord_seen + danger + collisions + terrain


def _synthetic() -> tuple[bytes, int, bytes]:
    offset = len(PREFIX)
    return PREFIX + _section() + FOLLOWING_OWNER, offset, FOLLOWING_OWNER


def _find(node: object, prefix: str):
    if node.name.startswith(prefix): return node
    for child in node.kids:
        found = _find(child, prefix)
        if found is not None: return found
    return None


def _field(node: object, name: str):
    return next(field["value"] for field in node.fields if field["name"] == name)


def _pe_offset(image: bytes, va: int) -> int:
    pe = struct.unpack_from("<I", image, 0x3C)[0]
    section_count = struct.unpack_from("<H", image, pe + 6)[0]
    optional_size = struct.unpack_from("<H", image, pe + 20)[0]
    optional = pe + 24
    image_base = struct.unpack_from("<I", image, optional + 28)[0]
    rva, table = va - image_base, optional + optional_size
    for index in range(section_count):
        section = table + index * 40
        virtual_size, virtual_address, raw_size, raw = struct.unpack_from("<IIII", image, section + 8)
        if virtual_address <= rva < virtual_address + max(virtual_size, raw_size):
            return raw + rva - virtual_address
    raise AssertionError(f"VA {va:#x} outside image")


class WorldParserTests(unittest.TestCase):
    def test_complete_selector_minus_one_grammar_and_next_boundary(self) -> None:
        data, offset, following = _synthetic()
        parsed = parse_world_section(data, offset)
        self.assertEqual(TAG_STRING_TABLE_INDEX, 2924)
        self.assertEqual((parsed.tag, parsed.xs, parsed.ys), (0, -111, 222))
        self.assertEqual([array.length for array in parsed.coordinate_arrays], [2, 0, 1, 2, 1, 0])
        self.assertEqual((parsed.direct(8), parsed.direct(20), parsed.direct(32), parsed.direct(44)), (2, 3, 2, 2))
        self.assertEqual([len(row) for row in parsed.wdata_rows], [21, 21])
        self.assertEqual(len(parsed.tdata), 4)
        self.assertEqual([len(plane) for plane in parsed.seen_planes], [3, 3, 3])
        self.assertEqual(len(parsed.wcoord_seen), 2)
        self.assertEqual([len(plane) for plane in parsed.danger_planes], [8] * 8)
        self.assertEqual([block.present for block in parsed.collision_blocks], [0, 1])
        self.assertEqual((parsed.collision_blocks[1].bits, parsed.collision_blocks[1].payload_size, parsed.collision_blocks[1].payload), (17, 3, b"XYZ"))
        self.assertEqual([array.length for array in parsed.terrain_arrays], [2, 1, 0, 2])
        self.assertEqual(parsed.layout_sha256, LAYOUT_SHA)
        self.assertEqual(data[parsed.end:], following)
        self.assertEqual(parsed.sha256, hashlib.sha256(data[offset:parsed.end]).hexdigest())

    def test_every_owned_byte_mutation_changes_receipt_or_is_rejected(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_world_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data); damaged[offset + relative] ^= 1
                try: changed = parse_world_section(damaged, offset)
                except WorldParseError: continue
                self.assertNotEqual(changed.sha256, baseline.sha256)
                self.assertNotEqual(changed, baseline)

    def test_every_truncation_is_rejected(self) -> None:
        data, offset, _ = _synthetic()
        end = parse_world_section(data, offset).end
        for retained in range(end - offset):
            with self.subTest(retained=retained):
                with self.assertRaises(WorldParseError):
                    parse_world_section(data[:offset + retained], offset)

    def test_next_game_daemon_mutation_is_excluded(self) -> None:
        data, offset, _ = _synthetic(); baseline = parse_world_section(data, offset)
        damaged = bytearray(data); damaged[baseline.end] ^= 0xFF
        self.assertEqual(parse_world_section(damaged, offset), baseline)

    def test_tags_histories_dimensions_and_collision_bounds_fail_closed(self) -> None:
        with self.assertRaises(WorldParseError): parse_world_section(_section(tag=1), 0)
        self.assertEqual(parse_world_section(_section(tag=0xE7), 0, require_tag=False).tag, 0xE7)
        bad = bytearray(_section()); bad[19] |= 0x40
        with self.assertRaisesRegex(WorldParseError, "writer-cleared"): parse_world_section(bad, 0)
        bad = bytearray(_section())
        parsed = parse_world_section(bad, 0)
        second = parsed.collision_blocks[1].offset
        struct.pack_into("<i", bad, second + 8, 97)
        with self.assertRaisesRegex(WorldParseError, "exceed PDB storage"): parse_world_section(bad, 0)

    def test_pdb_mutation_and_executable_bodies_are_frozen(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]
        schema, pdb, exe = root / "schema/pdb-types.json", root / "ron-bin/sbl/rise.pdb", root / "ron-bin/riseofnations.exe"
        if not schema.exists(): self.skipTest("matched schema unavailable")
        classes = json.loads(schema.read_text())["classes"]
        names = ("World", "WorldData", "WorldOut", "SimpleArray<WCoord>", "Array<WCoordData>", "SimpleArray<int>", "WData", "TData", "CollBlock", "Terrain")
        subset = {name: classes[name] for name in names}; subset["World"] = dict(subset["World"]); subset["World"]["size"] = 371
        with tempfile.TemporaryDirectory() as directory:
            bad = pathlib.Path(directory) / "pdb-types.json"; bad.write_text(json.dumps({"classes": subset}))
            with self.assertRaisesRegex(WorldParseError, "World layout disagrees"): parse_world_section(_section(), 0, schema_path=bad)
        if not pdb.exists() or not exe.exists(): self.skipTest("matched PE/PDB unavailable")
        self.assertEqual(hashlib.sha256(schema.read_bytes()).hexdigest(), "399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14")
        self.assertEqual(hashlib.sha256(pdb.read_bytes()).hexdigest(), "334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5")
        image = exe.read_bytes(); self.assertEqual(hashlib.sha256(image).hexdigest(), "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079")
        spans = (
            (0x006B5CF0, 903, "adf50b4197020da3932b562442b1cf7d8e80443da42f6181ebd23276bd080e01"),
            (0x0047C660, 464, "c5e9db4bf6477f2d408bdeb06b75e9ed940fcd0ecfba190e3ec8cfd25fddb094"),
            (0x00478990, 477, "0387b704aef41c645a2d09dbf1bca10a8cabe5130bfb45f1dde152958c6190f4"),
            (0x00473120, 464, "4019613cc0bb0110f2c0fa8be7e9837394cf064e63ea8b7fd6199e8c4958024a"),
            (0x005A2DAE, 35, "9e8393a502838b817f1a40024935b2633e045bccb81f5b4226b7a4fe264dc217"),
        )
        for va, size, digest in spans:
            raw = _pe_offset(image, va)
            self.assertEqual(hashlib.sha256(image[raw:raw + size]).hexdigest(), digest)

    def test_fresh_chain_reaches_exact_game_daemon_boundary_and_keeps_rcx_independent(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]
        save = root / "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX"
        replay = root / "ron-data/replays/Playback - 2026.08.11 11'44'38 (Tue).rcx"
        if not save.exists() or not replay.exists(): self.skipTest("user SVX/RCX unavailable")
        save_raw, replay_raw = save.read_bytes(), replay.read_bytes()
        self.assertEqual(hashlib.sha256(save_raw).hexdigest(), "161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7")
        self.assertEqual(hashlib.sha256(replay_raw).hexdigest(), "558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54")
        plain = gzip.decompress(save_raw)
        leaders = parse_leaders_section(plain, 0x9A5A); types = parse_types_section(plain, leaders.end); tileset = parse_tileset_section(plain, types.end); mountains = parse_mountains_section(plain, tileset.end); constants = parse_constants_block(plain, mountains.end); scalars = parse_direct_scalars(plain, constants.end); armies = parse_armies_section(plain, scalars.end); cities = parse_cities_section(plain, armies.end); forms = parse_forms_section(plain, cities.end); goods = parse_goods_section(plain, forms.end); items = parse_items_section(plain, goods.end); heroes = parse_heroes_section(plain, items.end); herds = parse_herds_section(plain, heroes.end); specials = parse_specials_section(plain, herds.end); wonders = parse_wonders_section(plain, specials.end); forts = parse_forts_section(plain, wonders.end); docks = parse_docks_section(plain, forts.end); oil = parse_oil_wells_section(plain, docks.end); supplies = parse_supplies_section(plain, oil.end); caravans = parse_caravans_section(plain, supplies.end); lands = parse_lands_section(plain, caravans.end); leader_options = parse_leader_options_section(plain, lands.end); option_info = parse_option_info_section(plain, leader_options.end); pathfinder = parse_pathfinder_groups_section(plain, option_info.end); groups = parse_groups_tail_section(plain, pathfinder.end); objects = parse_objects_section(plain, groups.end); hotkeys = parse_hotkey_groups_section(plain, objects.end)
        self.assertEqual(hotkeys.end, 0x27F53)
        parsed = parse_world_section(plain, hotkeys.end)
        self.assertEqual((parsed.offset, parsed.end, parsed.size), (0x27F53, 0x27FFC, 169))
        self.assertEqual(parsed.sha256, "605d47a6802a6ba6675ce2970606011e1d53eebdd846effd6f47bd0903d7ed13")
        changed = bytearray(plain); changed[parsed.end] ^= 1
        self.assertEqual(parse_world_section(changed, hotkeys.end), parsed)
        save_tree, replay_tree = savegame_parse.parse(plain), savegame_parse.parse(gzip.decompress(replay_raw))
        self.assertEqual((_field(_find(save_tree, "GameInfo::walk_data"), "seed"), _field(_find(replay_tree, "GameInfo::walk_data"), "seed")), ("0x014810ac", "0x007f93e0"))


if __name__ == "__main__": unittest.main()
