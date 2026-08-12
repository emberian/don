#!/usr/bin/env python3
"""Dynamic, mutation, truncation, PE/PDB, and SVX UnbuiltWonders gates."""

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
from savegame_farm_structs import parse_farm_structs_section  # noqa: E402
from savegame_game_daemon import parse_game_daemon_block  # noqa: E402
from savegame_game_random import parse_game_random_block  # noqa: E402
from savegame_graphic_events import FRESH_RETAIL_SLOT_COUNT, parse_graphic_events_section  # noqa: E402
from savegame_scene import parse_scene_section  # noqa: E402
from savegame_unbuilt_wonders import (  # noqa: E402
    PLAYER_LIST_COUNT,
    ROW_LOGICAL_SIZE,
    ROW_MEMORY_SIZE,
    TAG_STRING_TABLE_INDEX,
    UnbuiltWondersParseError,
    parse_unbuilt_wonders_section,
)
from savegame_world import parse_world_section  # noqa: E402


PREFIX = b"FARMS-END"
FOLLOWING_OWNER = b"UNBUILT-CITIES-SENTINEL" * 8
LAYOUT_SHA = "9d51184a7cd1e152ec9d78e19070f71efa723815809a8ed8bf73ee22ca424f73"


def _array(player: int) -> bytes:
    length = (2, 0, 1, 0, 3, 1, 0, 2)[player]
    if not length:
        return struct.pack("<i", 0)
    history = struct.pack("<iihB", length, length + 3, -player - 1, player & 3)
    rows = b"".join(struct.pack("<hb", 1000 + player * 10 + index, -20 + player + index) for index in range(length))
    return history + rows


def _section(tag: int = 0) -> tuple[bytes, int, bytes]:
    payload = bytes((tag,)) + b"".join(_array(player) for player in range(PLAYER_LIST_COUNT))
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


class UnbuiltWondersParserTests(unittest.TestCase):
    def test_all_eight_dynamic_lists_and_city_boundary(self) -> None:
        data, offset, following = _section(); parsed = parse_unbuilt_wonders_section(data, offset)
        self.assertEqual((TAG_STRING_TABLE_INDEX, PLAYER_LIST_COUNT), (7082, 8))
        self.assertEqual((ROW_LOGICAL_SIZE, ROW_MEMORY_SIZE), (3, 4))
        self.assertEqual([array.length for array in parsed.lists], [2, 0, 1, 0, 3, 1, 0, 2])
        self.assertEqual([(row.object_id, row.who) for row in parsed.lists[0].rows], [(1000, -20), (1001, -19)])
        self.assertEqual([(row.object_id, row.who) for row in parsed.lists[7].rows], [(1070, -13), (1071, -12)])
        self.assertEqual([row.end - row.offset for array in parsed.lists for row in array.rows], [3] * 9)
        self.assertEqual(parsed.layout_sha256, LAYOUT_SHA)
        self.assertEqual(parsed.sha256, hashlib.sha256(data[offset:parsed.end]).hexdigest())
        self.assertEqual(data[parsed.end:], following)

    def test_every_owned_byte_mutation_changes_receipt_or_is_rejected(self) -> None:
        data, offset, _ = _section(); baseline = parse_unbuilt_wonders_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data); damaged[offset + relative] ^= 1
                try: parsed = parse_unbuilt_wonders_section(damaged, offset)
                except UnbuiltWondersParseError: continue
                self.assertNotEqual(parsed, baseline); self.assertNotEqual(parsed.sha256, baseline.sha256)

    def test_every_truncation_and_histories_fail_closed(self) -> None:
        data, offset, _ = _section(); baseline = parse_unbuilt_wonders_section(data, offset)
        for retained in range(baseline.size):
            with self.subTest(retained=retained):
                with self.assertRaises(UnbuiltWondersParseError): parse_unbuilt_wonders_section(data[:offset + retained], offset)
        first = baseline.lists[0].offset
        damaged = bytearray(data); struct.pack_into("<i", damaged, first + 4, 1)
        with self.assertRaisesRegex(UnbuiltWondersParseError, "history"): parse_unbuilt_wonders_section(damaged, offset)
        damaged = bytearray(data); damaged[first + 10] |= 0x40
        with self.assertRaisesRegex(UnbuiltWondersParseError, "history"): parse_unbuilt_wonders_section(damaged, offset)
        damaged = bytearray(data); struct.pack_into("<i", damaged, first, -1)
        with self.assertRaisesRegex(UnbuiltWondersParseError, "length"): parse_unbuilt_wonders_section(damaged, offset)

    def test_tag_row_padding_and_next_owner_are_excluded(self) -> None:
        data, offset, _ = _section(); baseline = parse_unbuilt_wonders_section(data, offset)
        damaged = bytearray(data); damaged[baseline.end] ^= 0xFF
        self.assertEqual(parse_unbuilt_wonders_section(damaged, offset), baseline)
        damaged = bytearray(data); damaged[offset] = 1
        with self.assertRaisesRegex(UnbuiltWondersParseError, "tag"): parse_unbuilt_wonders_section(damaged, offset)
        self.assertEqual(parse_unbuilt_wonders_section(damaged, offset, require_tag=False).tag, 1)
        self.assertEqual(baseline.lists[0].rows[0].end, baseline.lists[0].rows[1].offset)
        self.assertEqual(baseline.lists[0].rows[0].end - baseline.lists[0].rows[0].offset, ROW_MEMORY_SIZE - 1)

    def test_pdb_mutation_and_exact_executable_bodies_are_frozen(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]; schema, pdb, exe = root / "schema/pdb-types.json", root / "ron-bin/sbl/rise.pdb", root / "ron-bin/riseofnations.exe"
        if not schema.exists(): self.skipTest("matched PDB schema unavailable")
        classes = json.loads(schema.read_text())["classes"]; names = ("UnbuiltWonders", "Array<UnbuiltWonder>", "UnbuiltWonder"); subset = {name: classes[name] for name in names}; subset["UnbuiltWonder"] = dict(subset["UnbuiltWonder"]); subset["UnbuiltWonder"]["size"] = 3
        with tempfile.TemporaryDirectory() as directory:
            bad = pathlib.Path(directory) / "pdb-types.json"; bad.write_text(json.dumps({"classes": subset}))
            with self.assertRaisesRegex(UnbuiltWondersParseError, "UnbuiltWonder size disagrees"): parse_unbuilt_wonders_section(bytes(33), 0, schema_path=bad)
        if not pdb.exists() or not exe.exists(): self.skipTest("matched PE/PDB unavailable")
        self.assertEqual(hashlib.sha256(schema.read_bytes()).hexdigest(), "399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14"); self.assertEqual(hashlib.sha256(pdb.read_bytes()).hexdigest(), "334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5")
        image = exe.read_bytes(); self.assertEqual(hashlib.sha256(image).hexdigest(), "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079")
        spans = ((0x0073C290, 550, "2f39ca65083926b449a36b5b40f06d086ec707e9b9e9436b06e18e0129d50234"), (0x00460DC0, 533, "fcb7592505bc0864a54da846796768b6cbf2cae2ffb76413cee811b6248dcdf0"), (0x00951300, 269, "130612cbfdab81986a51b53395ecf0a1496904de784461472b5fbb9ede75c18c"), (0x005A2E6B, 22, "b1aaad6a61d08d9481307df99772f9a0d8ff683b9d0c9b4741cc54f748f9bf94"))
        for va, size, digest in spans:
            raw = _pe_offset(image, va); self.assertEqual(hashlib.sha256(image[raw:raw + size]).hexdigest(), digest)

    def test_fresh_predecessor_chain_reaches_exact_city_boundary_and_rcx_is_independent(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]; save = root / "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX"; replay = root / "ron-data/replays/Playback - 2026.08.11 11'44'38 (Tue).rcx"
        if not save.exists() or not replay.exists(): self.skipTest("user SVX/RCX unavailable")
        save_raw, replay_raw = save.read_bytes(), replay.read_bytes(); self.assertEqual(hashlib.sha256(save_raw).hexdigest(), "161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7"); self.assertEqual(hashlib.sha256(replay_raw).hexdigest(), "558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54")
        plain = gzip.decompress(save_raw); world = parse_world_section(plain, 0x27F53); daemon = parse_game_daemon_block(plain, world.end); random = parse_game_random_block(plain, daemon.end); graphics = parse_graphic_events_section(plain, random.end, FRESH_RETAIL_SLOT_COUNT); scene = parse_scene_section(plain, graphics.end); farms = parse_farm_structs_section(plain, scene.end); parsed = parse_unbuilt_wonders_section(plain, farms.end)
        self.assertEqual((parsed.offset, parsed.end, parsed.size), (0x2BC8D, 0x2BCAE, 33)); self.assertEqual([array.length for array in parsed.lists], [0] * 8); self.assertEqual(parsed.sha256, "7f9c9e31ac8256ca2f258583df262dbc7d6f68f2a03043d5c99a4ae5a7396ce9")
        self.assertEqual(hashlib.sha256(plain[parsed.end:parsed.end + 32]).hexdigest(), "66687aadf862bd776c8fc18b8e9f8e20089714856ee233b3902a591d0d5f2925")
        damaged = bytearray(plain); damaged[parsed.end] ^= 1; self.assertEqual(parse_unbuilt_wonders_section(damaged, farms.end), parsed)
        save_tree, replay_tree = savegame_parse.parse(plain), savegame_parse.parse(gzip.decompress(replay_raw)); self.assertEqual((_field(_find(save_tree, "GameInfo::walk_data"), "seed"), _field(_find(replay_tree, "GameInfo::walk_data"), "seed")), ("0x014810ac", "0x007f93e0"))


if __name__ == "__main__": unittest.main()
