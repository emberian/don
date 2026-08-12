#!/usr/bin/env python3
"""Exact structural, mutation, artifact, and authority tests for Supplies saves."""

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
from savegame_forts import parse_forts_section  # noqa: E402
from savegame_goods import parse_goods_section  # noqa: E402
from savegame_herds import parse_herds_section  # noqa: E402
from savegame_heroes import parse_heroes_section  # noqa: E402
from savegame_items import parse_items_section  # noqa: E402
from savegame_leaders import parse_leaders_section  # noqa: E402
from savegame_mountains import parse_mountains_section  # noqa: E402
from savegame_specials import parse_specials_section  # noqa: E402
from savegame_tileset import parse_tileset_section  # noqa: E402
from savegame_types import parse_types_section  # noqa: E402
from savegame_wonders import parse_wonders_section  # noqa: E402
from savegame_docks import parse_docks_section  # noqa: E402
from savegame_supplies import SuppliesParseError, parse_supplies_section  # noqa: E402


PREFIX = b"PRE-SUPPLIES"
FOLLOWING_OWNER = b"CARAVANS-SENTINEL" * 32


def _row(supply: int, o: int, flags: int, who: int) -> bytes:
    return struct.pack("<hhBB", supply, o, flags & 0xFF, who & 0xFF)


ROW0 = _row(3, 9, 0xFD, 2)
ROW2 = _row(-1, -2, 1, -7)


def _owner0(capacity: int = 7, increment: int = 5, flags: int = 0x21) -> bytes:
    return (
        struct.pack("<iihB", 3, capacity, increment, flags)
        + bytes((1, 0, 1))
        + struct.pack("<ih", capacity, increment)
        + ROW0
        + ROW2
    )


def _section(capacity: int = 7, increment: int = 5, flags: int = 0x21) -> bytes:
    return b"\0" + _owner0(capacity, increment, flags) + struct.pack("<7i", *([0] * 7))


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
    count = struct.unpack_from("<H", image, pe + 6)[0]
    optional_size = struct.unpack_from("<H", image, pe + 20)[0]
    optional = pe + 24
    image_base = struct.unpack_from("<I", image, optional + 28)[0]
    rva = va - image_base
    table = optional + optional_size
    for index in range(count):
        section = table + index * 40
        virtual_size, virtual_address, raw_size, raw = struct.unpack_from(
            "<IIII", image, section + 8
        )
        if virtual_address <= rva < virtual_address + max(virtual_size, raw_size):
            return raw + rva - virtual_address
    raise AssertionError(f"VA {va:#x} is outside the image")


class SuppliesParserTests(unittest.TestCase):
    def test_complete_eight_owner_sparse_history_and_exact_caravans_boundary(self) -> None:
        data, offset, following = _synthetic()
        parsed = parse_supplies_section(data, offset)
        self.assertEqual(parsed.tag, 0)
        self.assertEqual(len(parsed.owners), 8)
        self.assertEqual(data[parsed.end :], following)
        self.assertEqual(parsed.sha256, hashlib.sha256(data[offset : parsed.end]).hexdigest())
        self.assertEqual(
            parsed.layout_sha256,
            "2df0e191c619386789247c14320377d25dda2f317f1a4c0bf19ecd3911024275",
        )

        owner = parsed.owners[0]
        self.assertEqual(
            (owner.length, owner.capacity, owner.increment, owner.flags),
            (3, 7, 5, 0x21),
        )
        self.assertEqual(owner.presence, (1, 0, 1))
        self.assertEqual((owner.repeated_capacity, owner.repeated_increment), (7, 5))
        self.assertTrue(all(other.length == 0 for other in parsed.owners[1:]))
        first, hole, last = owner.slots
        self.assertEqual(
            (first.supply, first.o, first.supply_flags, first.who, first.size),
            (3, 9, -3, 2, 6),
        )
        self.assertFalse(hole.present)
        self.assertEqual(hole.presence_offset, owner.presence_offset + 1)
        self.assertEqual(
            (last.supply, last.o, last.supply_flags, last.who, last.size),
            (-1, -2, 1, -7, 6),
        )

    def test_every_owned_byte_mutation_is_killed_or_changes_receipt(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_supplies_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[offset + relative] ^= 1
                try:
                    changed = parse_supplies_section(damaged, offset)
                except SuppliesParseError:
                    continue
                self.assertNotEqual(changed, baseline)
                self.assertNotEqual(changed.sha256, baseline.sha256)

    def test_every_truncation_is_rejected(self) -> None:
        data, offset, _ = _synthetic()
        end = parse_supplies_section(data, offset).end
        for retained in range(end - offset):
            with self.subTest(retained=retained):
                with self.assertRaises(SuppliesParseError):
                    parse_supplies_section(data[: offset + retained], offset)

    def test_following_caravans_mutation_is_excluded(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_supplies_section(data, offset)
        damaged = bytearray(data)
        damaged[baseline.end] ^= 0xFF
        self.assertEqual(parse_supplies_section(damaged, offset), baseline)

    def test_structural_invariants_fail_closed(self) -> None:
        data, offset, _ = _synthetic()
        parsed = parse_supplies_section(data, offset)
        owner = parsed.owners[0]
        cases: dict[str, tuple[int, bytes]] = {
            "wrong tag": (offset, b"\x01"),
            "negative length": (owner.offset, struct.pack("<i", -1)),
            "oversized length": (owner.offset, struct.pack("<i", (1 << 20) + 1)),
            "capacity below length": (owner.offset + 4, struct.pack("<i", 2)),
            "writer-cleared flags": (owner.offset + 10, b"\x61"),
            "nonboolean presence": (owner.presence_offset + 1, b"\x02"),
            "repeated capacity mismatch": (owner.repeated_capacity_offset, struct.pack("<i", 8)),
            "repeated increment mismatch": (owner.repeated_increment_offset, struct.pack("<h", 6)),
        }
        for name, (where, replacement) in cases.items():
            with self.subTest(name=name):
                damaged = bytearray(data)
                damaged[where : where + len(replacement)] = replacement
                with self.assertRaises(SuppliesParseError):
                    parse_supplies_section(damaged, offset)

    def test_valid_history_and_exact_6_byte_rows_are_preserved(self) -> None:
        baseline = parse_supplies_section(_section(), 0)
        changed = parse_supplies_section(_section(99, -17, 0x25), 0)
        owner = changed.owners[0]
        self.assertEqual((owner.capacity, owner.increment, owner.flags), (99, -17, 0x25))
        self.assertEqual((owner.repeated_capacity, owner.repeated_increment), (99, -17))
        self.assertEqual(changed.end, baseline.end)
        self.assertEqual(owner.slots[0].supply_flags, -3)
        self.assertEqual(owner.slots[1].size, 0)

    def test_rows_are_exact_pdb_prefix_without_object_tail(self) -> None:
        data = _section()
        parsed = parse_supplies_section(data, 0)
        first = parsed.owners[0].slots[0]
        self.assertEqual(data[first.offset : first.end], ROW0)
        self.assertEqual(first.size, 6)
        self.assertEqual(first.supply_flags & 0xFF, 0xFD)

    def test_pdb_layout_and_executable_bodies_are_frozen(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]
        schema = root / "schema/pdb-types.json"
        pdb = root / "ron-bin/sbl/rise.pdb"
        exe = root / "ron-bin/riseofnations.exe"
        for path in (schema, pdb, exe):
            if not path.exists():
                self.skipTest(f"matched artifact is not installed: {path}")
        self.assertEqual(hashlib.sha256(schema.read_bytes()).hexdigest(), "399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14")
        self.assertEqual(hashlib.sha256(pdb.read_bytes()).hexdigest(), "334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5")
        image = exe.read_bytes()
        self.assertEqual(hashlib.sha256(image).hexdigest(), "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079")
        classes = json.loads(schema.read_text())["classes"]
        self.assertEqual(classes["Supplies"]["size"], 224)
        self.assertEqual(classes["PtrArray<Supply>"]["size"], 28)
        self.assertEqual(classes["Supply"]["size"], 16)
        self.assertEqual(classes["SupplyData"]["size"], 6)
        spans = (
            (0x0073AE90, 952, "b23ca91a0715aeb6f01e861c0ea2f6677365f232cb954028547203bb93efe404"),
            (0x0073B540, 23, "5383148c4b21e4e1c029309d3d404f3afdd605c967b1084b71b0eceaf06deea1"),
            (0x005A2BB1, 22, "e59dfe836ab5998c2840a44edcabf2ac64f9d66e1ee421515e54f05b732e9815"),
            (0x0073E3F0, 1008, "5ffd2af8c62f1cc429d3bd4be6d0c9ba0b5fb362e57da47c85b4b8434f14c244"),
        )
        for va, size, digest in spans:
            with self.subTest(va=f"{va:#x}"):
                raw = _pe_offset(image, va)
                self.assertEqual(hashlib.sha256(image[raw : raw + size]).hexdigest(), digest)

    def test_fresh_svx_chains_to_exact_caravans_boundary_without_replay_join(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]
        save = root / "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX"
        replay = root / "ron-data/replays/Playback - 2026.08.11 11'44'38 (Tue).rcx"
        if not save.exists() or not replay.exists():
            self.skipTest("user-owned fresh SVX/RCX fixtures are not installed")
        save_raw, replay_raw = save.read_bytes(), replay.read_bytes()
        self.assertEqual(hashlib.sha256(save_raw).hexdigest(), "161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7")
        self.assertEqual(hashlib.sha256(replay_raw).hexdigest(), "558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54")
        save_plain = gzip.decompress(save_raw)
        self.assertEqual(hashlib.sha256(save_plain).hexdigest(), "fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8")
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
        herds = parse_herds_section(save_plain, heroes.end)
        specials = parse_specials_section(save_plain, herds.end)
        wonders = parse_wonders_section(save_plain, specials.end)
        forts = parse_forts_section(save_plain, wonders.end)
        docks = parse_docks_section(save_plain, forts.end)
        self.assertEqual(docks.end, 0x270F7)
        # OilWells is an independently recovered/frozen 33-byte owner whose
        # forward repair may land separately from this exclusive tranche.
        self.assertEqual(
            hashlib.sha256(save_plain[docks.end : docks.end + 33]).hexdigest(),
            "7f9c9e31ac8256ca2f258583df262dbc7d6f68f2a03043d5c99a4ae5a7396ce9",
        )
        supplies_offset = docks.end + 33
        self.assertEqual(supplies_offset, 0x27118)
        parsed = parse_supplies_section(save_plain, supplies_offset)
        self.assertEqual((parsed.offset, parsed.end, parsed.size), (0x27118, 0x27139, 33))
        self.assertEqual(parsed.tag, 0)
        self.assertEqual([owner.length for owner in parsed.owners], [0] * 8)
        self.assertEqual(parsed.sha256, "7f9c9e31ac8256ca2f258583df262dbc7d6f68f2a03043d5c99a4ae5a7396ce9")
        changed = bytearray(save_plain)
        changed[parsed.end] ^= 1
        self.assertEqual(parse_supplies_section(changed, supplies_offset), parsed)
        save_tree = savegame_parse.parse(save_plain)
        replay_tree = savegame_parse.parse(gzip.decompress(replay_raw))
        save_seed = _field(_find(save_tree, "GameInfo::walk_data"), "seed")
        replay_seed = _field(_find(replay_tree, "GameInfo::walk_data"), "seed")
        self.assertEqual((save_seed, replay_seed), ("0x014810ac", "0x007f93e0"))
        self.assertNotEqual(save_seed, replay_seed)


if __name__ == "__main__":
    unittest.main()
