#!/usr/bin/env python3
"""Exact tests for savegame_constants_block.py (no retail bytes embedded)."""

from __future__ import annotations

import gzip
import hashlib
import pathlib
import struct
import sys
import unittest


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import savegame_parse  # noqa: E402
from savegame_constants_block import (  # noqa: E402
    CONSTANTS_BLOCK_SIZE,
    CONSTANTS_FIELD_COUNT,
    CONSTANTS_WORD_COUNT,
    ConstantsParseError,
    parse_constants_block,
)
from savegame_leaders import parse_leaders_section  # noqa: E402
from savegame_mountains import parse_mountains_section  # noqa: E402
from savegame_tileset import parse_tileset_section  # noqa: E402
from savegame_types import parse_types_section  # noqa: E402


def _section() -> tuple[bytes, int, bytes]:
    prefix = b"MOUNTAINS-END"
    words = tuple(index * 7919 - 2_000_000 for index in range(CONSTANTS_WORD_COUNT))
    payload = struct.pack(f"<{CONSTANTS_WORD_COUNT}i", *words)
    following_owner = b"DUPLICATE-CONSTANT-SENTINEL"
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


class ConstantsBlockTests(unittest.TestCase):
    def test_all_pdb_fields_cover_exact_block_and_next_owner_boundary(self) -> None:
        data, offset, following_owner = _section()
        parsed = parse_constants_block(data, offset)
        self.assertEqual(parsed.size, CONSTANTS_BLOCK_SIZE)
        self.assertEqual(len(parsed.fields), CONSTANTS_FIELD_COUNT)
        self.assertEqual(len(parsed.words), CONSTANTS_WORD_COUNT)
        self.assertEqual(parsed.fields[0].name, "unit_formation_spacing")
        self.assertEqual(parsed.fields[0].relative_offset, 0)
        self.assertEqual(parsed.fields[-1].name, "attrition")
        self.assertEqual(parsed.fields[-1].relative_offset, 0xD3C)
        self.assertEqual(parsed.fields[-1].stream_offset, parsed.end - 4)
        self.assertEqual(
            parsed.sha256, hashlib.sha256(data[offset : parsed.end]).hexdigest()
        )
        self.assertEqual(
            parsed.layout_sha256,
            "399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14",
        )
        self.assertEqual(data[parsed.end :], following_owner)

    def test_arrays_are_named_pdb_fields_not_flat_guesses(self) -> None:
        data, offset, _ = _section()
        parsed = parse_constants_block(data, offset)
        by_name = {field.name: field for field in parsed.fields}
        self.assertEqual(by_name["fort_upgrade_terr"].type_name, "int[4]")
        self.assertEqual(len(by_name["fort_upgrade_terr"].values), 4)
        self.assertEqual(by_name["temple_upgrade_terr"].type_name, "int[5]")
        self.assertEqual(by_name["starting_goods"].type_name, "int[6]")
        self.assertEqual(by_name["pop_cap"].type_name, "int[8]")
        self.assertEqual(by_name["city_level_territory_bonus"].type_name, "int[3]")
        self.assertEqual(
            by_name["mongol_three_mil_cavalry"].relative_offset, 0x804
        )

    def test_every_owned_byte_mutation_changes_exactly_one_field(self) -> None:
        data, offset, _ = _section()
        baseline = parse_constants_block(data, offset)
        for relative in range(CONSTANTS_BLOCK_SIZE):
            mutated = bytearray(data)
            mutated[offset + relative] ^= 1
            parsed = parse_constants_block(mutated, offset)
            changed = [
                index
                for index, (before, after) in enumerate(
                    zip(baseline.fields, parsed.fields, strict=True)
                )
                if before != after
            ]
            self.assertEqual(len(changed), 1, f"mutation at byte {relative:#x}")
            self.assertNotEqual(parsed.sha256, baseline.sha256)

    def test_every_truncation_is_rejected(self) -> None:
        data, offset, _ = _section()
        for retained in range(CONSTANTS_BLOCK_SIZE):
            with self.subTest(retained=retained):
                with self.assertRaisesRegex(ConstantsParseError, "exceeds"):
                    parse_constants_block(data[: offset + retained], offset)

    def test_following_owner_mutation_is_excluded(self) -> None:
        data, offset, _ = _section()
        baseline = parse_constants_block(data, offset)
        mutated = bytearray(data)
        mutated[baseline.end] ^= 0xFF
        self.assertEqual(parse_constants_block(mutated, offset), baseline)

    def test_invalid_offset_is_rejected(self) -> None:
        data, _, _ = _section()
        with self.assertRaisesRegex(ConstantsParseError, "outside"):
            parse_constants_block(data, -1)

    def test_fresh_save_splices_from_mountains_without_replay_join(self) -> None:
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
        self.assertEqual(mountains.end, 0x262B2)
        parsed = parse_constants_block(save_plain, mountains.end)
        self.assertEqual(parsed.end, 0x26FF2)
        self.assertEqual(
            parsed.sha256,
            "91dd398f2278dc983e361803fe262142b261d1a985d5e9c4d1c1a7065b4bfbf4",
        )
        self.assertEqual(sum(value != 0 for value in parsed.words), 35)
        self.assertEqual(sum(any(field.values) for field in parsed.fields), 22)
        self.assertEqual(
            save_plain[parsed.end : parsed.end + 4],
            b"\x00\x00\x00\x00",
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
