#!/usr/bin/env python3
"""Exact dynamic, mutation, truncation, PE/PDB, and SVX Scene gates."""

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
from savegame_game_daemon import parse_game_daemon_block  # noqa: E402
from savegame_game_random import parse_game_random_block  # noqa: E402
from savegame_graphic_events import FRESH_RETAIL_SLOT_COUNT, parse_graphic_events_section  # noqa: E402
from savegame_scene import TAG_STRING_TABLE_INDEX, SceneParseError, parse_scene_section  # noqa: E402
from savegame_world import parse_world_section  # noqa: E402


PREFIX = b"GRAPHIC-EVENTS-END"
FOLLOWING_OWNER = b"FARM-STRUCT-SENTINEL" * 8
LAYOUT_SHA = "e0f695fdb0a0d7c2c29702d9bfa42848ae031152e018edeb89a8d6af3a50a8c2"


def _array(length: int, element_size: int, seed: int) -> bytes:
    if not length: return struct.pack("<i", 0)
    return struct.pack("<iihB", length, length + 2, -3, 2) + bytes((seed + i) & 0xFF for i in range(length * element_size))


def _section(tag: int = 0) -> tuple[bytes, int, bytes]:
    payload = bytes((tag,))
    payload += struct.pack("<ii", 17, 3) + b"ABC"
    payload += struct.pack("<ii", 32, 4) + b"WXYZ"
    payload += _array(2, 4, 0x10) + _array(1, 4, 0x20)
    payload += _array(3, 1, 0x30) + _array(0, 1, 0)
    payload += _array(2, 4, 0x40) + struct.pack("<i", -123456)
    return PREFIX + payload + FOLLOWING_OWNER, len(PREFIX), FOLLOWING_OWNER


def _find(node: object, prefix: str):
    if node.name.startswith(prefix): return node
    for child in node.kids:
        found = _find(child, prefix)
        if found is not None: return found
    return None


def _field(node: object, name: str): return next(field["value"] for field in node.fields if field["name"] == name)


def _pe_offset(image: bytes, va: int) -> int:
    pe = struct.unpack_from("<I", image, 0x3C)[0]; count = struct.unpack_from("<H", image, pe + 6)[0]; optional_size = struct.unpack_from("<H", image, pe + 20)[0]; optional = pe + 24; base = struct.unpack_from("<I", image, optional + 28)[0]; rva, table = va - base, optional + optional_size
    for index in range(count):
        section = table + index * 40; virtual_size, virtual_address, raw_size, raw = struct.unpack_from("<IIII", image, section + 8)
        if virtual_address <= rva < virtual_address + max(virtual_size, raw_size): return raw + rva - virtual_address
    raise AssertionError(f"VA {va:#x} outside image")


class SceneParserTests(unittest.TestCase):
    def test_complete_dynamic_grammar_and_farm_boundary(self) -> None:
        data, offset, following = _section(); parsed = parse_scene_section(data, offset)
        self.assertEqual(TAG_STRING_TABLE_INDEX, 5508)
        self.assertEqual([(mask.bits, mask.payload_size, mask.payload) for mask in parsed.masks], [(17, 3, b"ABC"), (32, 4, b"WXYZ")])
        self.assertEqual([array.length for array in parsed.arrays], [2, 1, 3, 0, 2])
        self.assertEqual([array.element_size for array in parsed.arrays], [4, 4, 1, 1, 4])
        self.assertEqual(parsed.last_time, -123456)
        self.assertEqual(parsed.layout_sha256, LAYOUT_SHA)
        self.assertEqual(parsed.sha256, hashlib.sha256(data[offset:parsed.end]).hexdigest())
        self.assertEqual(data[parsed.end:], following)

    def test_every_owned_byte_mutation_changes_receipt_or_is_rejected(self) -> None:
        data, offset, _ = _section(); baseline = parse_scene_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data); damaged[offset + relative] ^= 1
                try: parsed = parse_scene_section(damaged, offset)
                except SceneParseError: continue
                self.assertNotEqual(parsed, baseline); self.assertNotEqual(parsed.sha256, baseline.sha256)

    def test_every_truncation_and_strict_bounds(self) -> None:
        data, offset, _ = _section(); size = parse_scene_section(data, offset).size
        for retained in range(size):
            with self.subTest(retained=retained):
                with self.assertRaises(SceneParseError): parse_scene_section(data[:offset + retained], offset)
        damaged = bytearray(data); struct.pack_into("<i", damaged, offset + 1, 33)
        with self.assertRaisesRegex(SceneParseError, "bits/size"): parse_scene_section(damaged, offset)
        base = parse_scene_section(data, offset); damaged = bytearray(data); damaged[base.arrays[0].offset + 10] |= 0x40
        with self.assertRaisesRegex(SceneParseError, "history"): parse_scene_section(damaged, offset)

    def test_tag_and_following_owner_are_strictly_separate(self) -> None:
        data, offset, _ = _section(); baseline = parse_scene_section(data, offset)
        damaged = bytearray(data); damaged[baseline.end] ^= 0xFF
        self.assertEqual(parse_scene_section(damaged, offset), baseline)
        damaged = bytearray(data); damaged[offset] = 1
        with self.assertRaisesRegex(SceneParseError, "tag"): parse_scene_section(damaged, offset)
        self.assertEqual(parse_scene_section(damaged, offset, require_tag=False).tag, 1)

    def test_pdb_mutation_and_exact_executable_bodies_are_frozen(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]; schema, pdb, exe = root / "schema/pdb-types.json", root / "ron-bin/sbl/rise.pdb", root / "ron-bin/riseofnations.exe"
        if not schema.exists(): self.skipTest("matched PDB schema unavailable")
        classes = json.loads(schema.read_text())["classes"]; names = ("Scene", "BitMask<32>", "SimpleArray<Coord>", "SimpleArray<unsigned char>", "SimpleArray<unsigned long>"); subset = {name: classes[name] for name in names}; subset["Scene"] = dict(subset["Scene"]); subset["Scene"]["size"] = 823
        with tempfile.TemporaryDirectory() as directory:
            bad = pathlib.Path(directory) / "pdb-types.json"; bad.write_text(json.dumps({"classes": subset}))
            with self.assertRaisesRegex(SceneParseError, "Scene size disagrees"): parse_scene_section(bytes(41), 0, schema_path=bad)
        if not pdb.exists() or not exe.exists(): self.skipTest("matched PE/PDB unavailable")
        self.assertEqual(hashlib.sha256(schema.read_bytes()).hexdigest(), "399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14"); self.assertEqual(hashlib.sha256(pdb.read_bytes()).hexdigest(), "334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5")
        image = exe.read_bytes(); self.assertEqual(hashlib.sha256(image).hexdigest(), "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079")
        spans = ((0x008C0F70, 255, "27df0d70180debfaf27edbb748d786c55979c8b2257832585da17929487714e4"), (0x00483CC0, 448, "8bbc954c4648e00ac6ea44442c0c06aacaa173b418ff0fabdc29a4d0a9b59ff0"), (0x0049A090, 463, "3e5c4a5c4ae86dc1056d1b34f25333812f2b7379d06907b214a8f18bd919be56"), (0x004A7F80, 463, "4b10c0597f4be29fa3d75973fc31befb0f832b26196a720248dc59e294457b72"), (0x005A2E06, 31, "41b5dfde1c33d872dce4cb5c07faff7eda21bdfb4abe5a974fa97f6a55f836dd"))
        for va, size, digest in spans:
            raw = _pe_offset(image, va); self.assertEqual(hashlib.sha256(image[raw:raw + size]).hexdigest(), digest)

    def test_fresh_chain_reaches_exact_farm_tag_and_keeps_rcx_independent(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]; save = root / "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX"; replay = root / "ron-data/replays/Playback - 2026.08.11 11'44'38 (Tue).rcx"
        if not save.exists() or not replay.exists(): self.skipTest("user SVX/RCX unavailable")
        save_raw, replay_raw = save.read_bytes(), replay.read_bytes(); self.assertEqual(hashlib.sha256(save_raw).hexdigest(), "161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7"); self.assertEqual(hashlib.sha256(replay_raw).hexdigest(), "558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54")
        plain = gzip.decompress(save_raw); world = parse_world_section(plain, 0x27F53); daemon = parse_game_daemon_block(plain, world.end); random = parse_game_random_block(plain, daemon.end); graphics = parse_graphic_events_section(plain, random.end, FRESH_RETAIL_SLOT_COUNT); parsed = parse_scene_section(plain, graphics.end)
        self.assertEqual((parsed.offset, parsed.end, parsed.size), (0x2BC4D, 0x2BC76, 41)); self.assertEqual([(m.bits, m.payload_size) for m in parsed.masks], [(0, 0), (0, 0)]); self.assertEqual([a.length for a in parsed.arrays], [0] * 5); self.assertEqual(parsed.last_time, 0); self.assertEqual(parsed.sha256, "9e1736c43d19118e6ce4302118af337109491ecc52757dfb949bad6a7940b0c2"); self.assertEqual(plain[parsed.end], 4)
        damaged = bytearray(plain); damaged[parsed.end] ^= 1; self.assertEqual(parse_scene_section(damaged, graphics.end), parsed)
        save_tree, replay_tree = savegame_parse.parse(plain), savegame_parse.parse(gzip.decompress(replay_raw)); self.assertEqual((_field(_find(save_tree, "GameInfo::walk_data"), "seed"), _field(_find(replay_tree, "GameInfo::walk_data"), "seed")), ("0x014810ac", "0x007f93e0"))


if __name__ == "__main__": unittest.main()
