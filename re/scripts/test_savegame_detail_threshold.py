#!/usr/bin/env python3
"""Mutation, truncation, PE/PDB, and fresh-SVX detail-threshold gates."""

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
from savegame_detail_threshold import (  # noqa: E402
    DETAIL_THRESHOLD_SIZE,
    DETAIL_THRESHOLD_VA,
    NEXT_GLOBAL_VA,
    DetailThresholdParseError,
    parse_detail_threshold_block,
)
from savegame_farm_structs import parse_farm_structs_section  # noqa: E402
from savegame_game_daemon import parse_game_daemon_block  # noqa: E402
from savegame_game_random import parse_game_random_block  # noqa: E402
from savegame_graphic_events import FRESH_RETAIL_SLOT_COUNT, parse_graphic_events_section  # noqa: E402
from savegame_scene import parse_scene_section  # noqa: E402
from savegame_unbuilt_cities import parse_unbuilt_cities_section  # noqa: E402
from savegame_unbuilt_forts import parse_unbuilt_forts_section  # noqa: E402
from savegame_unbuilt_wonders import parse_unbuilt_wonders_section  # noqa: E402
from savegame_world import parse_world_section  # noqa: E402

try:
    from savegame_conquest_game import parse_conquest_game_section  # noqa: E402
except ImportError:
    parse_conquest_game_section = None


PREFIX = b"CONQUEST-END"
FOLLOWING_CAMERA = b"\x00CAMERA-OWNER" * 8
LAYOUT_SHA = "ffe89a9c7186460ad3e018116a788196a12c2c5a2b8833fdd7dce771dbc857b2"


def _fixture(bits: int = 0x7FC01234) -> tuple[bytes, int, bytes]:
    return PREFIX + struct.pack("<I", bits) + FOLLOWING_CAMERA, len(PREFIX), FOLLOWING_CAMERA


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
    count = struct.unpack_from("<H", image, pe + 6)[0]
    optional_size = struct.unpack_from("<H", image, pe + 20)[0]
    optional = pe + 24
    base = struct.unpack_from("<I", image, optional + 28)[0]
    rva, table = va - base, optional + optional_size
    for index in range(count):
        section = table + index * 40
        virtual_size, virtual_address, raw_size, raw = struct.unpack_from("<IIII", image, section + 8)
        if virtual_address <= rva < virtual_address + max(virtual_size, raw_size):
            return raw + rva - virtual_address
    raise AssertionError(f"VA {va:#x} outside image")


class DetailThresholdParserTests(unittest.TestCase):
    def test_exact_float_bits_and_camera_boundary(self) -> None:
        data, offset, following = _fixture()
        parsed = parse_detail_threshold_block(data, offset)
        self.assertEqual((DETAIL_THRESHOLD_VA, NEXT_GLOBAL_VA, DETAIL_THRESHOLD_SIZE), (0x00C0623C, 0x00C06240, 4))
        self.assertEqual((parsed.offset, parsed.end, parsed.size, parsed.bits), (offset, offset + 4, 4, 0x7FC01234))
        self.assertEqual(parsed.raw, struct.pack("<I", 0x7FC01234))
        self.assertEqual(parsed.sha256, hashlib.sha256(parsed.raw).hexdigest())
        self.assertEqual(parsed.layout_sha256, LAYOUT_SHA)
        self.assertEqual(data[parsed.end:], following)

    def test_every_owned_byte_mutation_changes_receipt(self) -> None:
        data, offset, _ = _fixture()
        baseline = parse_detail_threshold_block(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[offset + relative] ^= 1
                parsed = parse_detail_threshold_block(damaged, offset)
                self.assertNotEqual(parsed, baseline)
                self.assertNotEqual(parsed.sha256, baseline.sha256)
                self.assertNotEqual(parsed.bits, baseline.bits)

    def test_every_truncation_and_invalid_offsets_fail_closed(self) -> None:
        data, offset, _ = _fixture()
        for retained in range(DETAIL_THRESHOLD_SIZE):
            with self.subTest(retained=retained):
                with self.assertRaises(DetailThresholdParseError):
                    parse_detail_threshold_block(data[:offset + retained], offset)
        with self.assertRaises(DetailThresholdParseError):
            parse_detail_threshold_block(data, -1)

    def test_next_camera_owner_is_excluded(self) -> None:
        data, offset, _ = _fixture()
        baseline = parse_detail_threshold_block(data, offset)
        for relative in range(len(data) - baseline.end):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[baseline.end + relative] ^= 0xFF
                self.assertEqual(parse_detail_threshold_block(damaged, offset), baseline)

    def test_pdb_symbol_mutation_and_exact_executable_caller_are_frozen(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]
        symbols = root / "schema/symbols.json"
        schema = root / "schema/pdb-types.json"
        pdb = root / "ron-bin/sbl/rise.pdb"
        exe = root / "ron-bin/riseofnations.exe"
        if not symbols.exists() or not schema.exists():
            self.skipTest("matched PDB symbol exports unavailable")
        document = json.loads(symbols.read_text())
        selected = [row for row in document["globals"] if row.get("kind") == "global" and row.get("name") == "detail_threshold"]
        self.assertEqual(len(selected), 1)
        selected[0]["size"] = 8
        with tempfile.TemporaryDirectory() as directory:
            bad = pathlib.Path(directory) / "symbols.json"
            bad.write_text(json.dumps(document))
            with self.assertRaisesRegex(DetailThresholdParseError, "global boundary disagrees"):
                parse_detail_threshold_block(bytes(4), 0, symbols_path=bad)
        self.assertEqual(hashlib.sha256(symbols.read_bytes()).hexdigest(), "8e0fdfc4a538c1dc51f615c38d2fb713a901efee70f80d65106b9fb5c52d623f")
        classes = json.loads(schema.read_text())["classes"]
        self.assertEqual(classes["Camera"]["size"], 880)
        camera = {field["name"]: (field["offset"], field["size"], field["type"]) for field in classes["Camera"]["flattened"]}
        self.assertEqual(camera["distance"], (876, 4, "float"))
        if not pdb.exists() or not exe.exists():
            self.skipTest("matched PE/PDB unavailable")
        self.assertEqual(hashlib.sha256(schema.read_bytes()).hexdigest(), "399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14")
        self.assertEqual(hashlib.sha256(pdb.read_bytes()).hexdigest(), "334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5")
        image = exe.read_bytes()
        self.assertEqual(hashlib.sha256(image).hexdigest(), "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079")
        raw = _pe_offset(image, 0x005A2EFA)
        self.assertEqual(hashlib.sha256(image[raw:raw + 43]).hexdigest(), "7403157b8e699cc491e164896f010852b9db3ef91b603899d67bead6a67d0368")

    def test_fresh_chain_reaches_exact_camera_tag_and_rcx_is_independent(self) -> None:
        if parse_conquest_game_section is None:
            self.skipTest("ConquestGame predecessor helper not yet integrated")
        root = pathlib.Path(__file__).resolve().parents[2]
        save = root / "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX"
        replay = root / "ron-data/replays/Playback - 2026.08.11 11'44'38 (Tue).rcx"
        if not save.exists() or not replay.exists():
            self.skipTest("user SVX/RCX unavailable")
        save_raw, replay_raw = save.read_bytes(), replay.read_bytes()
        self.assertEqual(hashlib.sha256(save_raw).hexdigest(), "161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7")
        self.assertEqual(hashlib.sha256(replay_raw).hexdigest(), "558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54")
        plain = gzip.decompress(save_raw)
        self.assertEqual(hashlib.sha256(plain).hexdigest(), "fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8")
        world = parse_world_section(plain, 0x27F53)
        daemon = parse_game_daemon_block(plain, world.end)
        random = parse_game_random_block(plain, daemon.end)
        graphics = parse_graphic_events_section(plain, random.end, FRESH_RETAIL_SLOT_COUNT)
        scene = parse_scene_section(plain, graphics.end)
        farms = parse_farm_structs_section(plain, scene.end)
        wonders = parse_unbuilt_wonders_section(plain, farms.end)
        cities = parse_unbuilt_cities_section(plain, wonders.end)
        forts = parse_unbuilt_forts_section(plain, cities.end)
        conquest = parse_conquest_game_section(plain, forts.end)
        parsed = parse_detail_threshold_block(plain, conquest.end)
        self.assertEqual((parsed.offset, parsed.end, parsed.size, parsed.bits), (0x2BF77, 0x2BF7B, 4, 0))
        self.assertEqual(parsed.sha256, "df3f619804a92fdb4057192dc43dd748ea778adc52bc498ce80524c014b81119")
        self.assertEqual(plain[parsed.end], 0)
        damaged = bytearray(plain)
        damaged[parsed.end] ^= 1
        self.assertEqual(parse_detail_threshold_block(damaged, conquest.end), parsed)
        save_tree = savegame_parse.parse(plain)
        replay_tree = savegame_parse.parse(gzip.decompress(replay_raw))
        self.assertEqual((_field(_find(save_tree, "GameInfo::walk_data"), "seed"), _field(_find(replay_tree, "GameInfo::walk_data"), "seed")), ("0x014810ac", "0x007f93e0"))


if __name__ == "__main__":
    unittest.main()
