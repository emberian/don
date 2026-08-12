#!/usr/bin/env python3
"""Exact tests for savegame_direct_scalars.py (no retail bytes embedded)."""

from __future__ import annotations

import gzip
import hashlib
import pathlib
import struct
import sys
import unittest


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import savegame_parse  # noqa: E402
from savegame_constants_block import parse_constants_block  # noqa: E402
from savegame_direct_scalars import (  # noqa: E402
    DIRECT_SCALARS_SIZE,
    DirectScalarsParseError,
    parse_direct_scalars,
)
from savegame_leaders import parse_leaders_section  # noqa: E402
from savegame_mountains import parse_mountains_section  # noqa: E402
from savegame_tileset import parse_tileset_section  # noqa: E402
from savegame_types import parse_types_section  # noqa: E402


def _section() -> tuple[bytes, int, bytes]:
    prefix = b"CONSTANTS-END"
    payload = struct.pack("<3i", -17, 31, 1)
    following_owner = b"ARMIES-SENTINEL"
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


class DirectScalarsTests(unittest.TestCase):
    def test_exact_order_values_and_next_owner_boundary(self) -> None:
        data, offset, following_owner = _section()
        parsed = parse_direct_scalars(data, offset)
        self.assertEqual(parsed.size, DIRECT_SCALARS_SIZE)
        self.assertEqual(
            [field.name for field in parsed.fields],
            [
                "Constants.mongol_three_mil_cavalry",
                "GameAccess.ai_speed",
                "GameAccess.ai_off",
            ],
        )
        self.assertEqual([field.value for field in parsed.fields], [-17, 31, 1])
        self.assertEqual(
            [field.caller_va for field in parsed.fields],
            [0x005A2A7C, 0x005A2A97, 0x005A2AB2],
        )
        self.assertEqual(
            parsed.sha256, hashlib.sha256(data[offset : parsed.end]).hexdigest()
        )
        self.assertEqual(data[parsed.end :], following_owner)

    def test_every_owned_byte_mutation_changes_exactly_one_field(self) -> None:
        data, offset, _ = _section()
        baseline = parse_direct_scalars(data, offset)
        for relative in range(DIRECT_SCALARS_SIZE):
            mutated = bytearray(data)
            mutated[offset + relative] ^= 1
            parsed = parse_direct_scalars(mutated, offset)
            changed = [
                index
                for index, (before, after) in enumerate(
                    zip(baseline.fields, parsed.fields, strict=True)
                )
                if before != after
            ]
            self.assertEqual(changed, [relative // 4])
            self.assertNotEqual(parsed.sha256, baseline.sha256)

    def test_every_truncation_is_rejected(self) -> None:
        data, offset, _ = _section()
        for retained in range(DIRECT_SCALARS_SIZE):
            with self.subTest(retained=retained):
                with self.assertRaisesRegex(DirectScalarsParseError, "exceeds"):
                    parse_direct_scalars(data[: offset + retained], offset)

    def test_following_owner_mutation_is_excluded(self) -> None:
        data, offset, _ = _section()
        baseline = parse_direct_scalars(data, offset)
        mutated = bytearray(data)
        mutated[baseline.end] ^= 0xFF
        self.assertEqual(parse_direct_scalars(mutated, offset), baseline)

    def test_invalid_offset_is_rejected(self) -> None:
        data, _, _ = _section()
        with self.assertRaisesRegex(DirectScalarsParseError, "outside"):
            parse_direct_scalars(data, -1)

    def test_fresh_save_splices_from_constants_without_replay_join(self) -> None:
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
        mountains = parse_mountains_section(save_plain, tileset.end)
        constants = parse_constants_block(save_plain, mountains.end)
        self.assertEqual(constants.end, 0x26FF2)
        parsed = parse_direct_scalars(save_plain, constants.end)
        self.assertEqual(parsed.end, 0x26FFE)
        self.assertEqual([field.value for field in parsed.fields], [0, 0, 0])
        self.assertEqual(
            parsed.sha256,
            "15ec7bf0b50732b49f8228e07d24365338f9e3ab994b00af08e5a3bffe55fd8b",
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
