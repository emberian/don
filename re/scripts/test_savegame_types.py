#!/usr/bin/env python3
"""Byte-exact tests for savegame_types.py (no retail bytes embedded)."""

from __future__ import annotations

import gzip
import hashlib
import pathlib
import sys
import unittest


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from savegame_types import (  # noqa: E402
    TECH_TYPE_COUNT,
    TECH_TYPE_INDEX_END,
    TECH_TYPE_INDEX_START,
    TypesParseError,
    parse_types_section,
)
from savegame_leaders import parse_leaders_section  # noqa: E402
import savegame_parse  # noqa: E402


def _section() -> tuple[bytes, int, bytes]:
    prefix = b"LEADERS-END"
    payload = bytes(((index * 37) ^ 0xA5) & 0xFF for index in range(TECH_TYPE_COUNT))
    following_owner = b"TILESET-SENTINEL"
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


class TypesSectionTest(unittest.TestCase):
    def test_exact_fixed_walk_and_next_owner_boundary(self) -> None:
        data, offset, following_owner = _section()
        parsed = parse_types_section(data, offset)
        self.assertEqual(parsed.size, 85)
        self.assertEqual(len(parsed.fields), TECH_TYPE_COUNT)
        self.assertEqual(parsed.fields[0].tech_type_index, TECH_TYPE_INDEX_START)
        self.assertEqual(parsed.fields[-1].tech_type_index, TECH_TYPE_INDEX_END - 1)
        self.assertEqual(parsed.fields[0].offset, offset)
        self.assertEqual(parsed.fields[-1].offset, parsed.end - 1)
        self.assertEqual(
            [field.value for field in parsed.fields],
            list(data[offset : offset + TECH_TYPE_COUNT]),
        )
        self.assertEqual(
            parsed.sha256,
            hashlib.sha256(data[offset : offset + TECH_TYPE_COUNT]).hexdigest(),
        )
        self.assertEqual(data[parsed.end :], following_owner)

    def test_each_consumed_byte_is_mutation_sensitive(self) -> None:
        data, offset, _ = _section()
        baseline = parse_types_section(data, offset)
        for relative in range(TECH_TYPE_COUNT):
            mutated = bytearray(data)
            mutated[offset + relative] ^= 0xFF
            parsed = parse_types_section(mutated, offset)
            changed = [
                index
                for index, (before, after) in enumerate(
                    zip(baseline.fields, parsed.fields, strict=True)
                )
                if before != after
            ]
            self.assertEqual(changed, [relative])
            self.assertNotEqual(parsed.sha256, baseline.sha256)

    def test_following_owner_mutation_is_excluded(self) -> None:
        data, offset, _ = _section()
        baseline = parse_types_section(data, offset)
        mutated = bytearray(data)
        mutated[baseline.end] ^= 0xFF
        parsed = parse_types_section(mutated, offset)
        self.assertEqual(parsed, baseline)

    def test_truncation_is_rejected(self) -> None:
        data, offset, _ = _section()
        for retained in range(TECH_TYPE_COUNT):
            with self.subTest(retained=retained):
                with self.assertRaisesRegex(TypesParseError, "exceeds"):
                    parse_types_section(data[: offset + retained], offset)

    def test_invalid_offset_is_rejected(self) -> None:
        data, _, _ = _section()
        with self.assertRaisesRegex(TypesParseError, "outside"):
            parse_types_section(data, -1)

    def test_fresh_save_splices_from_leaders_without_replay_join(self) -> None:
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
        self.assertEqual(leaders.end, 0x26247)
        parsed = parse_types_section(save_plain, leaders.end)
        self.assertEqual(parsed.end, 0x2629C)
        self.assertEqual(
            parsed.sha256,
            "faabcd32d202c89e15b2530cb87dae68370357360133e712eef2ded2f1d6532e",
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
