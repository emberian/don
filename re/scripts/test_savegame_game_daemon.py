#!/usr/bin/env python3
"""Exact mutation, truncation, PE/PDB, and SVX gates for GameDaemon."""

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
from savegame_game_daemon import (  # noqa: E402
    CALLER_VA,
    GAME_DAEMON_BLOCK_SIZE,
    NEXT_CALLER_VA,
    GameDaemonParseError,
    parse_game_daemon_block,
)
from savegame_world import parse_world_section  # noqa: E402


PREFIX = b"WORLD-END"
FOLLOWING_OWNER = b"GAME-RANDOM-SENTINEL"
LAYOUT_SHA = "506b314ae1911e57fcd96ee6e14aeec6f4b8ff7a756799e94053780d6ba4b4c1"


def _section() -> tuple[bytes, int, bytes]:
    values = (-9, -7, -5, -3, 2, 4, 6, 8, 101, -202)
    return PREFIX + struct.pack("<10i", *values) + FOLLOWING_OWNER, len(PREFIX), FOLLOWING_OWNER


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
    image_base = struct.unpack_from("<I", image, optional + 28)[0]
    rva, table = va - image_base, optional + optional_size
    for index in range(count):
        section = table + index * 40
        virtual_size, virtual_address, raw_size, raw = struct.unpack_from("<IIII", image, section + 8)
        if virtual_address <= rva < virtual_address + max(virtual_size, raw_size):
            return raw + rva - virtual_address
    raise AssertionError(f"VA {va:#x} outside image")


class GameDaemonParserTests(unittest.TestCase):
    def test_exact_pdb_fields_and_game_random_boundary(self) -> None:
        data, offset, following = _section()
        parsed = parse_game_daemon_block(data, offset)
        self.assertEqual((CALLER_VA, NEXT_CALLER_VA), (0x005A2DC0, 0x005A2DDB))
        self.assertEqual((parsed.offset, parsed.size, parsed.end), (offset, 40, offset + 40))
        self.assertEqual([field.name for field in parsed.fields], ["repaths", "empty_colls", "borders"])
        self.assertEqual([field.type_name for field in parsed.fields], ["int[8]", "int", "int"])
        self.assertEqual(parsed.repaths, (-9, -7, -5, -3, 2, 4, 6, 8))
        self.assertEqual((parsed.empty_colls, parsed.borders), (101, -202))
        self.assertEqual(parsed.words, (-9, -7, -5, -3, 2, 4, 6, 8, 101, -202))
        self.assertEqual(parsed.layout_sha256, LAYOUT_SHA)
        self.assertEqual(parsed.sha256, hashlib.sha256(data[offset:parsed.end]).hexdigest())
        self.assertEqual(data[parsed.end:], following)

    def test_every_owned_byte_mutation_changes_exactly_one_pdb_field(self) -> None:
        data, offset, _ = _section()
        baseline = parse_game_daemon_block(data, offset)
        for relative in range(GAME_DAEMON_BLOCK_SIZE):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[offset + relative] ^= 1
                parsed = parse_game_daemon_block(damaged, offset)
                changed = [
                    index
                    for index, (before, after) in enumerate(zip(baseline.fields, parsed.fields, strict=True))
                    if before != after
                ]
                self.assertEqual(changed, [0 if relative < 32 else 1 + (relative - 32) // 4])
                self.assertNotEqual(parsed.sha256, baseline.sha256)

    def test_every_truncation_is_rejected(self) -> None:
        data, offset, _ = _section()
        for retained in range(GAME_DAEMON_BLOCK_SIZE):
            with self.subTest(retained=retained):
                with self.assertRaisesRegex(GameDaemonParseError, "exceeds"):
                    parse_game_daemon_block(data[:offset + retained], offset)

    def test_next_owner_mutation_is_excluded(self) -> None:
        data, offset, _ = _section()
        baseline = parse_game_daemon_block(data, offset)
        damaged = bytearray(data)
        damaged[baseline.end] ^= 0xFF
        self.assertEqual(parse_game_daemon_block(damaged, offset), baseline)

    def test_invalid_offsets_are_rejected(self) -> None:
        data, _, _ = _section()
        for offset in (-1, len(data) + 1):
            with self.subTest(offset=offset):
                with self.assertRaisesRegex(GameDaemonParseError, "outside"):
                    parse_game_daemon_block(data, offset)

    def test_pdb_mutation_and_exact_caller_spans_are_frozen(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]
        schema = root / "schema/pdb-types.json"
        pdb = root / "ron-bin/sbl/rise.pdb"
        exe = root / "ron-bin/riseofnations.exe"
        if not schema.exists():
            self.skipTest("matched PDB schema unavailable")
        classes = json.loads(schema.read_text())["classes"]
        record = dict(classes["GameDaemon"])
        fields = [dict(field) for field in record["fields"]]
        fields[2]["offset"] = 35
        record["fields"] = fields
        with tempfile.TemporaryDirectory() as directory:
            bad = pathlib.Path(directory) / "pdb-types.json"
            bad.write_text(json.dumps({"classes": {"GameDaemon": record}}))
            with self.assertRaisesRegex(GameDaemonParseError, "overlap"):
                parse_game_daemon_block(bytes(40), 0, schema_path=bad)
        if not pdb.exists() or not exe.exists():
            self.skipTest("matched PE/PDB unavailable")
        self.assertEqual(hashlib.sha256(schema.read_bytes()).hexdigest(), "399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14")
        self.assertEqual(hashlib.sha256(pdb.read_bytes()).hexdigest(), "334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5")
        image = exe.read_bytes()
        self.assertEqual(hashlib.sha256(image).hexdigest(), "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079")
        spans = (
            (CALLER_VA, 17, "915eb356f30b313a78116fd4865dba1b53eb1fd2f0eb82434abd5c3081f0159e"),
            (NEXT_CALLER_VA, 17, "5717106e2cf73f745b667649c2e2dacb542ca64c7c3c823b51dc186b2942753d"),
        )
        for va, size, digest in spans:
            raw = _pe_offset(image, va)
            self.assertEqual(hashlib.sha256(image[raw:raw + size]).hexdigest(), digest)

    def test_fresh_world_splice_and_rcx_independence(self) -> None:
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
        self.assertEqual(world.end, 0x27FFC)
        parsed = parse_game_daemon_block(plain, world.end)
        self.assertEqual((parsed.offset, parsed.end, parsed.size), (0x27FFC, 0x28024, 40))
        self.assertEqual(parsed.words, (0,) * 10)
        self.assertEqual(parsed.sha256, "2c34ce1df23b838c5abf2a7f6437cca3d3067ed509ff25f11df6b11b582b51eb")
        self.assertEqual(hashlib.sha256(plain[parsed.end:parsed.end + 4]).hexdigest(), "df3f619804a92fdb4057192dc43dd748ea778adc52bc498ce80524c014b81119")
        damaged = bytearray(plain)
        damaged[parsed.end] ^= 1
        self.assertEqual(parse_game_daemon_block(damaged, world.end), parsed)
        save_tree = savegame_parse.parse(plain)
        replay_tree = savegame_parse.parse(gzip.decompress(replay_raw))
        self.assertEqual(
            (_field(_find(save_tree, "GameInfo::walk_data"), "seed"), _field(_find(replay_tree, "GameInfo::walk_data"), "seed")),
            ("0x014810ac", "0x007f93e0"),
        )


if __name__ == "__main__":
    unittest.main()
