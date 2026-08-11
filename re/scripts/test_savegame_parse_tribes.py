#!/usr/bin/env python3
"""Focused, payload-free tests for the exact Tribes save prefix."""

from __future__ import annotations

import hashlib
import importlib.util
import io
import struct
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "savegame_parse", Path(__file__).with_name("savegame_parse.py")
)
PARSER = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(PARSER)


def minimal_tribes() -> bytearray:
    out = bytearray()
    out += bytes([PARSER.TAG_TRIBES])
    out += struct.pack("<iihB", 1, 1, -1, 0)
    out += bytes([PARSER.TAG_TRIBE_ROW])
    out += struct.pack("<7i", 0, 0, 0, 0, 0, 0, 0)
    out += struct.pack("<352I", *range(352))
    out += struct.pack("<4I", 0, 0, 0, 0)
    return out


def find(node, prefix):
    if node.name.startswith(prefix):
        return node
    for child in node.kids:
        found = find(child, prefix)
        if found is not None:
            return found
    return None


def field(node, name):
    return next(f["value"] for f in node.fields if f["name"] == name)


class TribesPrefixTests(unittest.TestCase):
    def parse_minimal(self, image):
        reader = PARSER.R(bytes(image))
        root = PARSER.Tree("root", 0)
        tribes = PARSER.parse_tribes(reader, root)
        self.assertEqual(reader.p, len(image))
        return tribes

    def test_exact_minimal_shape(self):
        tribes = self.parse_minimal(minimal_tribes())
        self.assertEqual(field(tribes, "length"), 1)
        self.assertEqual(field(tribes, "capacity"), 1)
        self.assertEqual(field(tribes, "increment"), -1)
        self.assertEqual(field(tribes.kids[0], "graft"), list(range(352)))

    def test_tags_capacity_string_and_truncation_fail_closed(self):
        cases = []
        image = minimal_tribes()
        image[0] ^= 1
        cases.append(image)

        image = minimal_tribes()
        image[12] ^= 1
        cases.append(image)

        image = minimal_tribes()
        struct.pack_into("<i", image, 5, 0)
        cases.append(image)

        image = minimal_tribes()
        first_string_len = 1 + 11 + 1 + 28 + 1408
        struct.pack_into("<I", image, first_string_len, (1 << 20) + 1)
        cases.append(image)

        cases.append(minimal_tribes()[:-1])
        for mutated in cases:
            with self.subTest(length=len(mutated)):
                with self.assertRaises((ValueError, IndexError, struct.error)):
                    self.parse_minimal(mutated)

    def test_fresh_save_reaches_leaders_without_cross_match_join(self):
        save = ROOT / "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX"
        replay = ROOT / "ron-data/replays/Playback - 2026.08.11 11'44'38 (Tue).rcx"
        if not save.exists() or not replay.exists():
            self.skipTest("user-owned fresh SVX/RCX fixtures are not installed")

        self.assertEqual(
            hashlib.sha256(save.read_bytes()).hexdigest(),
            "161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7",
        )
        tree = PARSER.parse(PARSER.load(str(save)))
        self.assertEqual(tree.end, 0x9A5A)
        tribes = find(tree, "Tribes / ObjectArray<Tribe>")
        self.assertIsNotNone(tribes)
        self.assertEqual(len(tribes.kids), 25)
        self.assertEqual([field(row, "name") for row in tribes.kids][-1], "Random")

        replay_tree = PARSER.parse(PARSER.load(str(replay)))
        save_seed = field(find(tree, "GameInfo::walk_data"), "seed")
        replay_seed = field(find(replay_tree, "GameInfo::walk_data"), "seed")
        self.assertEqual(save_seed, "0x014810ac")
        self.assertEqual(replay_seed, "0x007f93e0")
        self.assertNotEqual(save_seed, replay_seed)

    def test_verify_reports_parse_failure_as_failure(self):
        with tempfile.NamedTemporaryFile() as bad:
            bad.write(b"not a save or recording")
            bad.flush()
            with redirect_stdout(io.StringIO()):
                self.assertEqual(PARSER.verify([bad.name]), 1)


if __name__ == "__main__":
    unittest.main()
