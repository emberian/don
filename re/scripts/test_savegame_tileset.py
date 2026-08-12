#!/usr/bin/env python3
"""Byte-exact tests for savegame_tileset.py (no retail bytes embedded)."""

from __future__ import annotations

import gzip
import hashlib
import pathlib
import struct
import sys
import unittest


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import savegame_parse  # noqa: E402
from savegame_leaders import parse_leaders_section  # noqa: E402
from savegame_tileset import (  # noqa: E402
    MAX_STRING_CODE_UNITS,
    TAG_TILESET,
    TileSetParseError,
    parse_tileset_section,
)
from savegame_types import parse_types_section  # noqa: E402


def _section(name: str = "Temperate") -> tuple[bytes, int, bytes]:
    prefix = b"TYPES-END"
    encoded = name.encode("utf-16-le")
    payload = bytes([TAG_TILESET]) + struct.pack("<I", len(encoded) // 2) + encoded
    following_owner = b"MOUNTAINS-SENTINEL"
    return prefix + payload + following_owner, len(prefix), following_owner


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


class TileSetSectionTests(unittest.TestCase):
    def test_exact_string_walk_and_next_owner_boundary(self) -> None:
        data, offset, following_owner = _section()
        parsed = parse_tileset_section(data, offset)
        encoded = "Temperate".encode("utf-16-le")
        self.assertEqual(parsed.tag, TAG_TILESET)
        self.assertEqual(parsed.name_code_units, 9)
        self.assertEqual(parsed.current_tileset_name, "Temperate")
        self.assertEqual(parsed.size, 1 + 4 + len(encoded))
        self.assertEqual(
            parsed.name_payload_sha256, hashlib.sha256(encoded).hexdigest()
        )
        self.assertEqual(
            parsed.sha256, hashlib.sha256(data[offset : parsed.end]).hexdigest()
        )
        self.assertEqual(data[parsed.end :], following_owner)

    def test_every_owned_byte_mutation_is_killed(self) -> None:
        data, offset, _ = _section()
        baseline = parse_tileset_section(data, offset)
        for relative in range(baseline.size):
            mutated = bytearray(data)
            mutated[offset + relative] ^= 1
            try:
                parsed = parse_tileset_section(mutated, offset)
            except TileSetParseError:
                continue
            self.assertNotEqual(parsed, baseline, f"mutation at byte {relative}")

    def test_every_truncation_is_rejected(self) -> None:
        data, offset, _ = _section()
        size = parse_tileset_section(data, offset).size
        for retained in range(size):
            with self.subTest(retained=retained):
                with self.assertRaisesRegex(TileSetParseError, "exceeds"):
                    parse_tileset_section(data[: offset + retained], offset)

    def test_following_owner_mutation_is_excluded(self) -> None:
        data, offset, _ = _section()
        baseline = parse_tileset_section(data, offset)
        mutated = bytearray(data)
        mutated[baseline.end] ^= 0xFF
        self.assertEqual(parse_tileset_section(mutated, offset), baseline)

    def test_tag_length_utf16_and_offset_fail_closed(self) -> None:
        data, offset, _ = _section()

        bad_tag = bytearray(data)
        bad_tag[offset] ^= 1
        with self.assertRaisesRegex(TileSetParseError, "TileSet tag"):
            parse_tileset_section(bad_tag, offset)

        too_long = bytearray(data)
        struct.pack_into("<I", too_long, offset + 1, MAX_STRING_CODE_UNITS + 1)
        with self.assertRaisesRegex(TileSetParseError, "impossible String length"):
            parse_tileset_section(too_long, offset)

        invalid_utf16 = bytes([TAG_TILESET]) + struct.pack("<I", 1) + b"\x00\xd8"
        with self.assertRaisesRegex(TileSetParseError, "not valid UTF-16LE"):
            parse_tileset_section(invalid_utf16, 0)

        with self.assertRaisesRegex(TileSetParseError, "outside"):
            parse_tileset_section(data, -1)

    def test_empty_current_tileset_is_exact_five_byte_image(self) -> None:
        image = bytes([TAG_TILESET]) + struct.pack("<I", 0) + b"MOUNTAINS"
        parsed = parse_tileset_section(image, 0)
        self.assertEqual(parsed.end, 5)
        self.assertEqual(parsed.current_tileset_name, "")
        self.assertEqual(
            parsed.sha256,
            "8855508aade16ec573d21e6a485dfd0a7624085c1a14b5ecdd6485de0c6839a4",
        )

    def test_fresh_save_splices_from_types_without_replay_join(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]
        save = root / "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX"
        replay = root / "ron-data/replays/Playback - 2026.08.11 11'44'38 (Tue).rcx"
        if not save.exists() or not replay.exists():
            self.skipTest("user-owned fresh SVX/RCX fixtures are not installed")

        save_raw = save.read_bytes()
        replay_raw = replay.read_bytes()
        self.assertEqual(
            hashlib.sha256(save_raw).hexdigest(),
            "161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7",
        )
        self.assertEqual(
            hashlib.sha256(replay_raw).hexdigest(),
            "558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54",
        )
        save_plain = gzip.decompress(save_raw)
        self.assertEqual(
            hashlib.sha256(save_plain).hexdigest(),
            "fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8",
        )

        leaders = parse_leaders_section(save_plain, 0x9A5A)
        types = parse_types_section(save_plain, leaders.end)
        self.assertEqual(types.end, 0x2629C)
        parsed = parse_tileset_section(save_plain, types.end)
        self.assertEqual(parsed.end, 0x262A1)
        self.assertEqual(parsed.tag, 0)
        self.assertEqual(parsed.name_code_units, 0)
        self.assertEqual(parsed.current_tileset_name, "")
        self.assertEqual(
            parsed.sha256,
            "8855508aade16ec573d21e6a485dfd0a7624085c1a14b5ecdd6485de0c6839a4",
        )

        save_tree = savegame_parse.parse(save_plain)
        replay_tree = savegame_parse.parse(gzip.decompress(replay_raw))
        save_seed = _field(_find(save_tree, "GameInfo::walk_data"), "seed")
        replay_seed = _field(_find(replay_tree, "GameInfo::walk_data"), "seed")
        self.assertEqual(save_seed, "0x014810ac")
        self.assertEqual(replay_seed, "0x007f93e0")
        self.assertNotEqual(save_seed, replay_seed)


if __name__ == "__main__":
    unittest.main()
