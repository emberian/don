#!/usr/bin/env python3
"""Byte-exact tests for savegame_mountains.py (no retail bytes embedded)."""

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
from savegame_mountains import (  # noqa: E402
    MAX_ARRAY_LENGTH,
    TAG_MOUNTAINS,
    MountainsParseError,
    parse_mountains_section,
)
from savegame_tileset import parse_tileset_section  # noqa: E402
from savegame_types import parse_types_section  # noqa: E402


def _array(payload: bytes, element_size: int, *, capacity: int, flags: int) -> bytes:
    assert len(payload) % element_size == 0
    length = len(payload) // element_size
    return struct.pack("<iihB", length, capacity, -3, flags) + payload


def _section() -> tuple[bytes, int, bytes]:
    prefix = b"TILESET-END"
    payload = bytearray([TAG_MOUNTAINS])
    payload += _array(struct.pack("<2i", 17, -9), 4, capacity=4, flags=0x21)
    payload += _array(struct.pack("<i", 31), 4, capacity=2, flags=0x02)
    payload += _array(
        struct.pack("<6f", 1.0, 2.0, 3.0, -4.0, 5.5, 6.25),
        12,
        capacity=3,
        flags=0x08,
    )
    payload += _array(struct.pack("<2i", 7, -11), 4, capacity=5, flags=0x10)
    following_owner = b"CONSTANTS-SENTINEL"
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


class MountainsSectionTests(unittest.TestCase):
    def test_exact_four_array_histories_and_next_owner_boundary(self) -> None:
        data, offset, following_owner = _section()
        parsed = parse_mountains_section(data, offset)
        self.assertEqual(parsed.tag, TAG_MOUNTAINS)
        self.assertEqual(
            [array.name for array in parsed.arrays],
            [
                "mountain_loc_wcoords_x",
                "mountain_loc_wcoords_y",
                "mountain_locs",
                "mountain_types",
            ],
        )
        self.assertEqual([array.length for array in parsed.arrays], [2, 1, 2, 2])
        self.assertEqual([array.element_size for array in parsed.arrays], [4, 4, 12, 4])
        self.assertEqual(parsed.arrays[0].payload_words, (17, 0xFFFFFFF7))
        self.assertEqual(parsed.arrays[2].payload_words[0], 0x3F800000)
        self.assertEqual(parsed.arrays[3].payload_words, (7, 0xFFFFFFF5))
        self.assertEqual(
            parsed.sha256, hashlib.sha256(data[offset : parsed.end]).hexdigest()
        )
        self.assertEqual(data[parsed.end :], following_owner)

    def test_every_owned_byte_mutation_is_killed(self) -> None:
        data, offset, _ = _section()
        baseline = parse_mountains_section(data, offset)
        for relative in range(baseline.size):
            mutated = bytearray(data)
            mutated[offset + relative] ^= 1
            try:
                parsed = parse_mountains_section(mutated, offset)
            except MountainsParseError:
                continue
            self.assertNotEqual(parsed, baseline, f"mutation at byte {relative}")

    def test_every_truncation_is_rejected(self) -> None:
        data, offset, _ = _section()
        size = parse_mountains_section(data, offset).size
        for retained in range(size):
            with self.subTest(retained=retained):
                with self.assertRaises(MountainsParseError):
                    parse_mountains_section(data[: offset + retained], offset)

    def test_following_owner_mutation_is_excluded(self) -> None:
        data, offset, _ = _section()
        baseline = parse_mountains_section(data, offset)
        mutated = bytearray(data)
        mutated[baseline.end] ^= 0xFF
        self.assertEqual(parse_mountains_section(mutated, offset), baseline)

    def test_tag_length_capacity_flags_and_offset_fail_closed(self) -> None:
        data, offset, _ = _section()

        bad_tag = bytearray(data)
        bad_tag[offset] ^= 1
        with self.assertRaisesRegex(MountainsParseError, "Mountains tag"):
            parse_mountains_section(bad_tag, offset)

        bad_length = bytearray(data)
        struct.pack_into("<i", bad_length, offset + 1, MAX_ARRAY_LENGTH + 1)
        with self.assertRaisesRegex(MountainsParseError, "invalid length"):
            parse_mountains_section(bad_length, offset)

        bad_capacity = bytearray(data)
        struct.pack_into("<i", bad_capacity, offset + 5, 1)
        with self.assertRaisesRegex(MountainsParseError, "invalid history"):
            parse_mountains_section(bad_capacity, offset)

        bad_flags = bytearray(data)
        bad_flags[offset + 11] |= 0x40
        with self.assertRaisesRegex(MountainsParseError, "writer-cleared bit"):
            parse_mountains_section(bad_flags, offset)

        with self.assertRaisesRegex(MountainsParseError, "outside"):
            parse_mountains_section(data, -1)

    def test_empty_arrays_are_exact_seventeen_byte_image(self) -> None:
        image = bytes([TAG_MOUNTAINS]) + struct.pack("<4i", 0, 0, 0, 0)
        parsed = parse_mountains_section(image + b"CONSTANTS", 0)
        self.assertEqual(parsed.end, 17)
        self.assertTrue(all(array.length == 0 for array in parsed.arrays))
        self.assertTrue(all(array.size == 4 for array in parsed.arrays))
        self.assertEqual(
            parsed.sha256,
            "0a88111852095cae045340ea1f0b279944b2a756a213d9b50107d7489771e159",
        )

    def test_fresh_save_splices_from_tileset_without_replay_join(self) -> None:
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
        tileset = parse_tileset_section(save_plain, types.end)
        self.assertEqual(tileset.end, 0x262A1)
        parsed = parse_mountains_section(save_plain, tileset.end)
        self.assertEqual(parsed.end, 0x262B2)
        self.assertEqual(parsed.tag, 0)
        self.assertTrue(all(array.length == 0 for array in parsed.arrays))
        self.assertEqual(
            parsed.sha256,
            "0a88111852095cae045340ea1f0b279944b2a756a213d9b50107d7489771e159",
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
