#!/usr/bin/env python3
"""Exact structural, mutation, artifact, and authority tests for Heroes saves."""

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
from savegame_heroes import HeroesParseError, parse_heroes_section  # noqa: E402
from savegame_items import parse_items_section  # noqa: E402
from savegame_leaders import parse_leaders_section  # noqa: E402
from savegame_mountains import parse_mountains_section  # noqa: E402
from savegame_tileset import parse_tileset_section  # noqa: E402
from savegame_types import parse_types_section  # noqa: E402


PREFIX = b"PRE-HEROES"
FOLLOWING_OWNER = b"HERDS-TAG-AND-ARRAY" * 32


def _hero_empty(hero: int = 10, o: int = 20, flags: int = -3, who: int = 2) -> bytes:
    return struct.pack("<ihhbb", 0, hero, o, flags, who)


def _hero_spells(
    *,
    capacity: int = 5,
    increment: int = -2,
    flags: int = 0x25,
    hero: int = -30,
    o: int = 40,
    hero_flags: int = -7,
    who: int = 6,
) -> bytes:
    return (
        struct.pack("<iihB", 2, capacity, increment, flags)
        + struct.pack("<iiiiii", 635, 100, 200, 643, -50, 900)
        + struct.pack("<hhbb", hero, o, hero_flags, who)
    )


def _owner0(
    *,
    capacity: int = 7,
    increment: int = 5,
    flags: int = 0x21,
    nested_capacity: int = 5,
    nested_increment: int = -2,
    nested_flags: int = 0x25,
) -> bytes:
    return (
        struct.pack("<iihB", 3, capacity, increment, flags)
        + bytes((1, 0, 1))
        + struct.pack("<ih", capacity, increment)
        + _hero_empty()
        + _hero_spells(
            capacity=nested_capacity,
            increment=nested_increment,
            flags=nested_flags,
        )
    )


def _section(**kwargs: int) -> bytes:
    return b"\0" + _owner0(**kwargs) + struct.pack("<7i", *([0] * 7))


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


class HeroesParserTests(unittest.TestCase):
    def test_complete_eight_owner_sparse_history_and_exact_herds_boundary(self) -> None:
        data, offset, following_owner = _synthetic()
        parsed = parse_heroes_section(data, offset)
        self.assertEqual(parsed.tag, 0)
        self.assertEqual(len(parsed.owners), 8)
        self.assertEqual(data[parsed.end :], following_owner)
        self.assertEqual(
            parsed.sha256, hashlib.sha256(data[offset : parsed.end]).hexdigest()
        )
        self.assertEqual(
            parsed.layout_sha256,
            "ead3334b3e08937f889e670fc2b0b91bfb3967148d51e18b2cb8f71d5c33677b",
        )

        owner = parsed.owners[0]
        self.assertEqual(
            (owner.length, owner.capacity, owner.increment, owner.flags),
            (3, 7, 5, 0x21),
        )
        self.assertEqual(owner.presence, (1, 0, 1))
        self.assertEqual(
            (owner.repeated_capacity, owner.repeated_increment), (7, 5)
        )
        self.assertTrue(all(other.length == 0 for other in parsed.owners[1:]))

        empty, hole, active = owner.slots
        self.assertTrue(empty.present)
        self.assertEqual(
            (empty.hero, empty.o, empty.hero_flags, empty.who, empty.size),
            (10, 20, -3, 2, 10),
        )
        self.assertEqual(empty.active_spells.length, 0)
        self.assertFalse(hole.present)
        self.assertEqual(hole.presence_offset, owner.presence_offset + 1)
        self.assertEqual(
            (active.hero, active.o, active.hero_flags, active.who, active.size),
            (-30, 40, -7, 6, 41),
        )
        spells = active.active_spells
        self.assertEqual(
            (spells.length, spells.capacity, spells.increment, spells.flags),
            (2, 5, -2, 0x25),
        )
        self.assertEqual(
            [(row.type_index, row.start, row.frame) for row in spells.spells],
            [(635, 100, 200), (643, -50, 900)],
        )
        self.assertEqual([row.size for row in spells.spells], [12, 12])

    def test_every_owned_byte_mutation_is_killed_or_changes_receipt(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_heroes_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[offset + relative] ^= 1
                try:
                    changed = parse_heroes_section(damaged, offset)
                except HeroesParseError:
                    continue
                self.assertNotEqual(changed, baseline)
                self.assertNotEqual(changed.sha256, baseline.sha256)

    def test_every_truncation_is_rejected(self) -> None:
        data, offset, _ = _synthetic()
        end = parse_heroes_section(data, offset).end
        for retained in range(end - offset):
            with self.subTest(retained=retained):
                with self.assertRaises(HeroesParseError):
                    parse_heroes_section(data[: offset + retained], offset)

    def test_following_herds_owner_mutation_is_excluded(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_heroes_section(data, offset)
        damaged = bytearray(data)
        damaged[baseline.end] ^= 0xFF
        self.assertEqual(parse_heroes_section(damaged, offset), baseline)

    def test_outer_and_nested_structural_invariants_fail_closed(self) -> None:
        data, offset, _ = _synthetic()
        parsed = parse_heroes_section(data, offset)
        owner = parsed.owners[0]
        active = owner.slots[2].active_spells
        cases: dict[str, tuple[int, bytes]] = {
            "wrong tag": (offset, b"\x01"),
            "negative outer length": (owner.offset, struct.pack("<i", -1)),
            "oversized outer length": (
                owner.offset,
                struct.pack("<i", (1 << 20) + 1),
            ),
            "outer capacity below length": (
                owner.offset + 4,
                struct.pack("<i", 2),
            ),
            "outer writer-cleared flags": (owner.offset + 10, b"\x61"),
            "nonboolean presence": (owner.presence_offset + 1, b"\x02"),
            "repeated capacity mismatch": (
                owner.repeated_capacity_offset,
                struct.pack("<i", 8),
            ),
            "repeated increment mismatch": (
                owner.repeated_increment_offset,
                struct.pack("<h", 6),
            ),
            "negative nested length": (active.offset, struct.pack("<i", -1)),
            "oversized nested length": (
                active.offset,
                struct.pack("<i", (1 << 20) + 1),
            ),
            "nested capacity below length": (
                active.offset + 4,
                struct.pack("<i", 1),
            ),
            "nested writer-cleared flags": (active.offset + 10, b"\x65"),
        }
        for name, (where, replacement) in cases.items():
            with self.subTest(name=name):
                damaged = bytearray(data)
                damaged[where : where + len(replacement)] = replacement
                with self.assertRaises(HeroesParseError):
                    parse_heroes_section(damaged, offset)

    def test_valid_histories_and_signed_hero_scalars_are_preserved(self) -> None:
        baseline = parse_heroes_section(_section(), 0)
        changed = parse_heroes_section(
            _section(
                capacity=99,
                increment=-17,
                flags=0x25,
                nested_capacity=77,
                nested_increment=-9,
                nested_flags=0x05,
            ),
            0,
        )
        owner = changed.owners[0]
        nested = owner.slots[2].active_spells
        self.assertEqual(
            (owner.capacity, owner.increment, owner.flags), (99, -17, 0x25)
        )
        self.assertEqual(
            (owner.repeated_capacity, owner.repeated_increment), (99, -17)
        )
        self.assertEqual(
            (nested.capacity, nested.increment, nested.flags), (77, -9, 0x05)
        )
        self.assertEqual(changed.end, baseline.end)
        self.assertEqual(
            (owner.slots[0].hero_flags, owner.slots[2].hero_flags), (-3, -7)
        )

    def test_active_spell_rows_cross_check_independent_sim_authority(self) -> None:
        data = _section()
        parsed = parse_heroes_section(data, 0)
        spells = parsed.owners[0].slots[2].active_spells
        raw_rows = data[spells.spells[0].offset : spells.spells[-1].end]
        self.assertEqual(
            raw_rows,
            struct.pack("<iiiiii", 635, 100, 200, 643, -50, 900),
        )

        root = pathlib.Path(__file__).resolve().parents[2]
        authority = (root / "crates/don-sim/src/systems/casters_animals.rs").read_text()
        self.assertIn("/// `ActiveSpell`, PDB size 12.", authority)
        self.assertIn("#[repr(C)]", authority)
        self.assertIn("pub struct ActiveSpell {", authority)
        self.assertIn("pub type_id: i32,", authority)
        self.assertIn("pub start_frame: i32,", authority)
        self.assertIn("pub end_frame: i32,", authority)
        self.assertIn("checksummed capacity/growth metadata", authority)
        self.assertIn("does not pretend a Rust `Vec`", authority)

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
        self.assertEqual(classes["Heroes"]["size"], 232)
        self.assertEqual(classes["PtrArray<Hero>"]["size"], 28)
        self.assertEqual(classes["Hero"]["size"], 48)
        self.assertEqual(classes["Caster"]["size"], 40)
        self.assertEqual(classes["Array<ActiveSpell>"]["size"], 28)
        self.assertEqual(classes["ActiveSpell"]["size"], 12)

        spans = (
            (0x0073A510, 971, "e4f0499c36146b6856f4a1e4f664ccf3398c9f9b745a3ae3f8544da5fe4ba464"),
            (0x00739E20, 42, "eaab87e0e73871bfcb3d1445d1e899e4ab47498a4dbe89440466d1b6c1e81b8d"),
            (0x00739AB0, 21, "9aa53df4e41a03f9e53370b25bd40447e62d2a2f87c3532f828cccf3a456bfa9"),
            (0x0048A9E0, 488, "3a3686d74a419884802ad7111c97f34851dfc249c074ff175aab61e178a592ef"),
            (0x005A2B2B, 44, "dce7c76c5eebeda0680d948fc1e86958618b04d37508c3f6654f2013d93a2bde"),
            (0x0048D610, 760, "cb3909d0ec97ac3a6d4f311371295acac5b72cc6bee0f9e94fad476c7b1dd5fc"),
        )
        for va, size, digest in spans:
            with self.subTest(va=f"{va:#x}"):
                raw = _pe_offset(image, va)
                self.assertEqual(
                    hashlib.sha256(image[raw : raw + size]).hexdigest(), digest
                )

    def test_fresh_svx_chains_to_exact_herds_tag_without_replay_join(self) -> None:
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
        self.assertEqual(items.end, 0x2704D)
        parsed = parse_heroes_section(save_plain, items.end)
        self.assertEqual(
            (parsed.offset, parsed.end, parsed.size), (0x2704D, 0x2706E, 33)
        )
        self.assertEqual(parsed.tag, 0)
        self.assertEqual([owner.length for owner in parsed.owners], [0] * 8)
        self.assertEqual(
            parsed.sha256,
            "7f9c9e31ac8256ca2f258583df262dbc7d6f68f2a03043d5c99a4ae5a7396ce9",
        )
        self.assertEqual(
            hashlib.sha256(save_plain[parsed.end : parsed.end + 5]).hexdigest(),
            "8855508aade16ec573d21e6a485dfd0a7624085c1a14b5ecdd6485de0c6839a4",
        )

        changed_next = bytearray(save_plain)
        changed_next[parsed.end] ^= 1
        self.assertEqual(parse_heroes_section(changed_next, items.end), parsed)

        save_tree = savegame_parse.parse(save_plain)
        replay_tree = savegame_parse.parse(gzip.decompress(replay_raw))
        save_seed = _field(_find(save_tree, "GameInfo::walk_data"), "seed")
        replay_seed = _field(_find(replay_tree, "GameInfo::walk_data"), "seed")
        self.assertEqual(save_seed, "0x014810ac")
        self.assertEqual(replay_seed, "0x007f93e0")
        self.assertNotEqual(save_seed, replay_seed)


if __name__ == "__main__":
    unittest.main()
