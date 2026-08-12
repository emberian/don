#!/usr/bin/env python3
"""Exact structural, mutation, artifact, and authority tests for Herds saves."""

from __future__ import annotations

import gzip
import hashlib
import json
import pathlib
import struct
import sys
import unittest


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import savegame_parse  # noqa: E402
from savegame_armies import parse_armies_section  # noqa: E402
from savegame_cities import parse_cities_section  # noqa: E402
from savegame_constants_block import parse_constants_block  # noqa: E402
from savegame_direct_scalars import parse_direct_scalars  # noqa: E402
from savegame_forms import parse_forms_section  # noqa: E402
from savegame_goods import parse_goods_section  # noqa: E402
from savegame_herds import HerdsParseError, parse_herds_section  # noqa: E402
from savegame_heroes import parse_heroes_section  # noqa: E402
from savegame_items import parse_items_section  # noqa: E402
from savegame_leaders import parse_leaders_section  # noqa: E402
from savegame_mountains import parse_mountains_section  # noqa: E402
from savegame_tileset import parse_tileset_section  # noqa: E402
from savegame_types import parse_types_section  # noqa: E402


PREFIX = b"PRE-HERDS"
FOLLOWING_OWNER = b"SPECIALS-SENTINEL" * 32


def _row(
    cx: int,
    cy: int,
    wx: int,
    wy: int,
    type_index: int,
    good_object: int,
    herd: int,
    herd_flags: int,
) -> bytes:
    return struct.pack(
        "<iiiiiihb", cx, cy, wx, wy, type_index, good_object, herd, herd_flags
    )


ROW0 = _row(0x63637, 0x637B7, 0x63A77, 0x63AF7, 516, 19, 3, -3)
ROW2 = _row(-1, -2, -3, -4, 522, -1, -20, 1)


def _section(capacity: int = 7, increment: int = 5, flags: int = 0x21) -> bytes:
    return (
        b"\0"
        + struct.pack("<iihB", 3, capacity, increment, flags)
        + bytes((1, 0, 1))
        + struct.pack("<ih", capacity, increment)
        + ROW0
        + ROW2
    )


def _synthetic() -> tuple[bytes, int, bytes]:
    offset = len(PREFIX)
    return PREFIX + _section() + FOLLOWING_OWNER, offset, FOLLOWING_OWNER


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
    if image[pe : pe + 4] != b"PE\0\0":
        raise AssertionError("not a PE image")
    section_count = struct.unpack_from("<H", image, pe + 6)[0]
    optional_size = struct.unpack_from("<H", image, pe + 20)[0]
    optional = pe + 24
    image_base = struct.unpack_from("<I", image, optional + 28)[0]
    rva = va - image_base
    table = optional + optional_size
    for index in range(section_count):
        section = table + index * 40
        virtual_size, virtual_address, raw_size, raw = struct.unpack_from(
            "<IIII", image, section + 8
        )
        if virtual_address <= rva < virtual_address + max(virtual_size, raw_size):
            return raw + rva - virtual_address
    raise AssertionError(f"VA {va:#x} is outside the image")


class HerdsParserTests(unittest.TestCase):
    def test_complete_sparse_history_rows_and_exact_specials_boundary(self) -> None:
        data, offset, following_owner = _synthetic()
        parsed = parse_herds_section(data, offset)
        self.assertEqual(
            (parsed.tag, parsed.length, parsed.capacity, parsed.increment, parsed.flags),
            (0, 3, 7, 5, 0x21),
        )
        self.assertEqual(parsed.presence, (1, 0, 1))
        self.assertEqual(
            (parsed.repeated_capacity, parsed.repeated_increment), (7, 5)
        )
        self.assertEqual(data[parsed.end :], following_owner)
        self.assertEqual(
            parsed.sha256, hashlib.sha256(data[offset : parsed.end]).hexdigest()
        )
        self.assertEqual(
            parsed.layout_sha256,
            "9cd0aebae5906ab64e1b838fb2f251916e7b0b194cfb2cd72dcc9aca2923f6fc",
        )

        first, hole, last = parsed.slots
        self.assertEqual(
            (
                first.cx,
                first.cy,
                first.wx,
                first.wy,
                first.type_index,
                first.good_object,
                first.herd,
                first.herd_flags,
                first.size,
            ),
            (0x63637, 0x637B7, 0x63A77, 0x63AF7, 516, 19, 3, -3, 27),
        )
        self.assertFalse(hole.present)
        self.assertEqual(hole.presence_offset, parsed.presence_offset + 1)
        self.assertEqual(
            (
                last.cx,
                last.cy,
                last.wx,
                last.wy,
                last.type_index,
                last.good_object,
                last.herd,
                last.herd_flags,
                last.size,
            ),
            (-1, -2, -3, -4, 522, -1, -20, 1, 27),
        )

    def test_every_owned_byte_mutation_is_killed_or_changes_receipt(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_herds_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[offset + relative] ^= 1
                try:
                    changed = parse_herds_section(damaged, offset)
                except HerdsParseError:
                    continue
                self.assertNotEqual(changed, baseline)
                self.assertNotEqual(changed.sha256, baseline.sha256)

    def test_every_truncation_is_rejected(self) -> None:
        data, offset, _ = _synthetic()
        end = parse_herds_section(data, offset).end
        for retained in range(end - offset):
            with self.subTest(retained=retained):
                with self.assertRaises(HerdsParseError):
                    parse_herds_section(data[: offset + retained], offset)

    def test_following_specials_mutation_is_excluded(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_herds_section(data, offset)
        damaged = bytearray(data)
        damaged[baseline.end] ^= 0xFF
        self.assertEqual(parse_herds_section(damaged, offset), baseline)

    def test_structural_invariants_fail_closed(self) -> None:
        data, offset, _ = _synthetic()
        parsed = parse_herds_section(data, offset)
        length_offset = offset + 1
        cases: dict[str, tuple[int, bytes]] = {
            "wrong tag": (offset, b"\x01"),
            "negative length": (length_offset, struct.pack("<i", -1)),
            "oversized length": (
                length_offset,
                struct.pack("<i", (1 << 20) + 1),
            ),
            "capacity below length": (length_offset + 4, struct.pack("<i", 2)),
            "writer-cleared flags": (length_offset + 10, b"\x61"),
            "nonboolean presence": (parsed.presence_offset + 1, b"\x02"),
            "repeated capacity mismatch": (
                parsed.repeated_capacity_offset,
                struct.pack("<i", 8),
            ),
            "repeated increment mismatch": (
                parsed.repeated_increment_offset,
                struct.pack("<h", 6),
            ),
        }
        for name, (where, replacement) in cases.items():
            with self.subTest(name=name):
                damaged = bytearray(data)
                damaged[where : where + len(replacement)] = replacement
                with self.assertRaises(HerdsParseError):
                    parse_herds_section(damaged, offset)

    def test_valid_history_and_all_27_row_bytes_are_preserved(self) -> None:
        baseline = parse_herds_section(_section(), 0)
        changed = parse_herds_section(_section(99, -17, 0x25), 0)
        self.assertEqual(
            (changed.capacity, changed.increment, changed.flags), (99, -17, 0x25)
        )
        self.assertEqual(
            (changed.repeated_capacity, changed.repeated_increment), (99, -17)
        )
        self.assertEqual(changed.end, baseline.end)

        retagged = bytearray(_section())
        retagged[changed.slots[0].end - 1] = 0x80
        signed = parse_herds_section(retagged, 0)
        self.assertEqual(signed.slots[0].herd_flags, -128)
        self.assertNotEqual(signed.sha256, baseline.sha256)

    def test_27_byte_rows_cross_check_independent_sim_authority(self) -> None:
        data = _section()
        parsed = parse_herds_section(data, 0)
        first = parsed.slots[0]
        self.assertEqual(data[first.offset : first.end], ROW0)
        self.assertEqual(first.size, 27)

        root = pathlib.Path(__file__).resolve().parents[2]
        authority = (root / "crates/don-sim/src/systems/casters_animals.rs").read_text()
        self.assertIn("/// `HerdData`, PDB size 28.", authority)
        self.assertIn("#[repr(C)]", authority)
        self.assertIn("pub struct HerdData {", authority)
        for field in (
            "pub cx: i32,",
            "pub cy: i32,",
            "pub wx: i32,",
            "pub wy: i32,",
            "pub type_id: i32,",
            "pub good_object: i32,",
            "pub herd_id: i16,",
            "pub herd_flags: u8,",
        ):
            self.assertIn(field, authority)
        self.assertIn("tail padding is not a claim", authority)
        self.assertEqual(first.herd_flags & 0xFF, 0xFD)

    def test_pdb_layout_and_executable_bodies_are_frozen(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]
        schema = root / "schema/pdb-types.json"
        pdb = root / "ron-bin/sbl/rise.pdb"
        exe = root / "ron-bin/riseofnations.exe"
        for path in (schema, pdb, exe):
            if not path.exists():
                self.skipTest(f"matched artifact is not installed: {path}")
        self.assertEqual(
            hashlib.sha256(schema.read_bytes()).hexdigest(),
            "399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14",
        )
        self.assertEqual(
            hashlib.sha256(pdb.read_bytes()).hexdigest(),
            "334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5",
        )
        image = exe.read_bytes()
        self.assertEqual(
            hashlib.sha256(image).hexdigest(),
            "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079",
        )

        classes = json.loads(schema.read_text())["classes"]
        self.assertEqual(classes["PtrArray<Herd>"]["size"], 28)
        self.assertEqual(classes["Herd"]["size"], 36)
        self.assertEqual(classes["HerdData"]["size"], 28)

        spans = (
            (0x0048D610, 760, "cb3909d0ec97ac3a6d4f311371295acac5b72cc6bee0f9e94fad476c7b1dd5fc"),
            (0x00741A40, 23, "dcf013a87b5c9367b72b599e60a66ba9ded8f745254d37a95caceb96d3ab0fb1"),
            (0x00741D50, 40, "5d52ea5e46acc9ae882e2be1b424bc3d9cbe802768b795d88b763ea70e6574c8"),
            (0x005A2B3B, 44, "256de6645a0b8f0bfa848c3a3e1b0e6aa6fafe637566dcdea662aaa62b72fe4b"),
            (0x007403B0, 971, "883161a773397db9216ed5fe26b33f99452051e870ae59d4d9d4f3a6bf3a5024"),
        )
        for va, size, digest in spans:
            with self.subTest(va=f"{va:#x}"):
                raw = _pe_offset(image, va)
                self.assertEqual(
                    hashlib.sha256(image[raw : raw + size]).hexdigest(), digest
                )

    def test_fresh_svx_chains_to_exact_specials_boundary_without_replay_join(self) -> None:
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
        scalars = parse_direct_scalars(save_plain, constants.end)
        armies = parse_armies_section(save_plain, scalars.end)
        cities = parse_cities_section(save_plain, armies.end)
        forms = parse_forms_section(save_plain, cities.end)
        goods = parse_goods_section(save_plain, forms.end)
        items = parse_items_section(save_plain, goods.end)
        heroes = parse_heroes_section(save_plain, items.end)
        self.assertEqual(heroes.end, 0x2706E)
        parsed = parse_herds_section(save_plain, heroes.end)
        self.assertEqual(
            (parsed.offset, parsed.end, parsed.size), (0x2706E, 0x27073, 5)
        )
        self.assertEqual((parsed.tag, parsed.length), (0, 0))
        self.assertEqual(
            parsed.sha256,
            "8855508aade16ec573d21e6a485dfd0a7624085c1a14b5ecdd6485de0c6839a4",
        )

        changed_next = bytearray(save_plain)
        changed_next[parsed.end] ^= 1
        self.assertEqual(parse_herds_section(changed_next, heroes.end), parsed)

        save_tree = savegame_parse.parse(save_plain)
        replay_tree = savegame_parse.parse(gzip.decompress(replay_raw))
        save_seed = _field(_find(save_tree, "GameInfo::walk_data"), "seed")
        replay_seed = _field(_find(replay_tree, "GameInfo::walk_data"), "seed")
        self.assertEqual(save_seed, "0x014810ac")
        self.assertEqual(replay_seed, "0x007f93e0")
        self.assertNotEqual(save_seed, replay_seed)


if __name__ == "__main__":
    unittest.main()
