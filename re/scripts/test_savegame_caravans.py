#!/usr/bin/env python3
"""Exact structural, mutation, artifact, and authority tests for Caravans saves."""

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
from savegame_caravans import (  # noqa: E402
    CARAVAN_TAG_STRING_TABLE_INDEX,
    TAG_STRING_TABLE_INDEX,
    CaravansParseError,
    parse_caravans_section,
)
from savegame_cities import parse_cities_section  # noqa: E402
from savegame_constants_block import parse_constants_block  # noqa: E402
from savegame_direct_scalars import parse_direct_scalars  # noqa: E402
from savegame_docks import parse_docks_section  # noqa: E402
from savegame_forms import parse_forms_section  # noqa: E402
from savegame_forts import parse_forts_section  # noqa: E402
from savegame_goods import parse_goods_section  # noqa: E402
from savegame_herds import parse_herds_section  # noqa: E402
from savegame_heroes import parse_heroes_section  # noqa: E402
from savegame_items import parse_items_section  # noqa: E402
from savegame_leaders import parse_leaders_section  # noqa: E402
from savegame_mountains import parse_mountains_section  # noqa: E402
from savegame_oil_wells import parse_oil_wells_section  # noqa: E402
from savegame_specials import parse_specials_section  # noqa: E402
from savegame_supplies import parse_supplies_section  # noqa: E402
from savegame_tileset import parse_tileset_section  # noqa: E402
from savegame_types import parse_types_section  # noqa: E402
from savegame_wonders import parse_wonders_section  # noqa: E402


PREFIX = b"PRE-CARAVANS"
FOLLOWING_OWNER = b"LANDS-SENTINEL" * 32


def _path(to_x: int, to_y: int, tolerance: int, flags: int) -> bytes:
    return struct.pack("<iiii", to_x, to_y, tolerance, flags)


PATH0 = _path(0x63637, -0x637B7, 0x63A77, 0x21)
PATH1 = _path(-7, 9, 11, -13)


def _road(capacity: int, increment: int, *rows: bytes) -> bytes:
    return struct.pack("<iib", capacity, len(rows), increment) + b"".join(rows)


def _row(
    tag: int,
    fields: tuple[int, int, int, int, int, int, int, int, int, int],
    road: bytes,
) -> bytes:
    city2, whom, city3, whose, cara, o, flags, who, making, reset = fields
    return bytes((tag,)) + struct.pack(
        "<hhhhhhBbii",
        city2,
        whom,
        city3,
        whose,
        cara,
        o,
        flags,
        who,
        making,
        reset,
    ) + road


ROW0 = _row(
    0x41,
    (3, 9, 5, 7, 11, 13, 0xFD, -2, 17, -19),
    _road(0, -1),
)
ROW2 = _row(
    0x42,
    (-1, -2, -3, -4, -5, -6, 1, 7, -23, 29),
    _road(5, -3, PATH0, PATH1),
)


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


class CaravansParserTests(unittest.TestCase):
    def test_complete_sparse_history_paths_and_exact_lands_boundary(self) -> None:
        data, offset, following_owner = _synthetic()
        parsed = parse_caravans_section(data, offset)
        self.assertEqual((TAG_STRING_TABLE_INDEX, CARAVAN_TAG_STRING_TABLE_INDEX), (417, 416))
        self.assertEqual(parsed.layout_sha256, "45d962ca2e00858bbc31ce43e9065e0ab67f70d7591df41cba071b45f8ebe405")
        self.assertEqual(data[parsed.end :], following_owner)
        self.assertEqual(parsed.sha256, hashlib.sha256(data[offset : parsed.end]).hexdigest())
        owner = parsed.owners[0]
        self.assertEqual((owner.length, owner.capacity, owner.increment, owner.flags), (3, 7, 5, 0x21))
        self.assertEqual(owner.presence, (1, 0, 1))
        self.assertEqual((owner.repeated_capacity, owner.repeated_increment), (7, 5))
        self.assertTrue(all(other.length == 0 for other in parsed.owners[1:]))

        first, hole, last = owner.slots
        self.assertEqual(
            (
                first.row_tag,
                first.city2,
                first.whom,
                first.city3,
                first.whose,
                first.cara,
                first.o,
                first.caravan_flags,
                first.who,
                first.making_road,
                first.reset_road,
                first.size,
            ),
            (0x41, 3, 9, 5, 7, 11, 13, 0xFD, -2, 17, -19, 32),
        )
        self.assertEqual((first.road.capacity, first.road.length, first.road.increment, first.road.size), (0, 0, -1, 9))
        self.assertFalse(hole.present)
        self.assertEqual(hole.presence_offset, owner.presence_offset + 1)
        self.assertEqual(
            (last.row_tag, last.city2, last.who, last.making_road, last.reset_road, last.size),
            (0x42, -1, 7, -23, 29, 64),
        )
        self.assertEqual((last.road.capacity, last.road.length, last.road.increment), (5, 2, -3))
        self.assertEqual(
            [(row.to_x, row.to_y, row.tolerance, row.flags) for row in last.road.rows],
            [(0x63637, -0x637B7, 0x63A77, 0x21), (-7, 9, 11, -13)],
        )

    def test_every_owned_byte_mutation_is_killed_or_changes_receipt(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_caravans_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[offset + relative] ^= 1
                try:
                    changed = parse_caravans_section(damaged, offset)
                except CaravansParseError:
                    continue
                self.assertNotEqual(changed, baseline)
                self.assertNotEqual(changed.sha256, baseline.sha256)

    def test_every_truncation_is_rejected(self) -> None:
        data, offset, _ = _synthetic()
        end = parse_caravans_section(data, offset).end
        for retained in range(end - offset):
            with self.subTest(retained=retained):
                with self.assertRaises(CaravansParseError):
                    parse_caravans_section(data[: offset + retained], offset)

    def test_following_lands_mutation_is_excluded(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_caravans_section(data, offset)
        damaged = bytearray(data)
        damaged[baseline.end] ^= 0xFF
        self.assertEqual(parse_caravans_section(damaged, offset), baseline)

    def test_container_and_stack_invariants_fail_closed(self) -> None:
        data, offset, _ = _synthetic()
        parsed = parse_caravans_section(data, offset)
        owner = parsed.owners[0]
        road0 = owner.slots[0].road
        road2 = owner.slots[2].road
        cases: dict[str, tuple[int, bytes]] = {
            "wrong tag": (offset, b"\x01"),
            "negative length": (owner.offset, struct.pack("<i", -1)),
            "oversized length": (owner.offset, struct.pack("<i", (1 << 20) + 1)),
            "capacity below length": (owner.offset + 4, struct.pack("<i", 2)),
            "writer-cleared flags": (owner.offset + 10, b"\x61"),
            "nonboolean presence": (owner.presence_offset + 1, b"\x02"),
            "repeated capacity mismatch": (owner.repeated_capacity_offset, struct.pack("<i", 8)),
            "repeated increment mismatch": (owner.repeated_increment_offset, struct.pack("<h", 6)),
            "negative road capacity": (road0.offset, struct.pack("<i", -1)),
            "oversized road capacity": (road0.offset, struct.pack("<i", (1 << 20) + 1)),
            "negative road length": (road0.offset + 4, struct.pack("<i", -1)),
            "road length above capacity": (road2.offset + 4, struct.pack("<i", 6)),
            "oversized road length": (road2.offset + 4, struct.pack("<i", (1 << 20) + 1)),
        }
        for name, (where, replacement) in cases.items():
            with self.subTest(name=name):
                damaged = bytearray(data)
                damaged[where : where + len(replacement)] = replacement
                with self.assertRaises(CaravansParseError):
                    parse_caravans_section(damaged, offset)

    def test_valid_histories_tags_and_exact_row_projection_are_preserved(self) -> None:
        baseline = parse_caravans_section(_section(), 0)
        changed_data = _section(99, -17, 0x25)
        changed = parse_caravans_section(changed_data, 0)
        owner = changed.owners[0]
        self.assertEqual((owner.capacity, owner.increment, owner.flags), (99, -17, 0x25))
        self.assertEqual((owner.repeated_capacity, owner.repeated_increment), (99, -17))
        self.assertEqual(changed.end, baseline.end)
        first, last = owner.slots[0], owner.slots[2]
        self.assertEqual(changed_data[first.offset : first.end], ROW0)
        self.assertEqual(changed_data[last.offset : last.end], ROW2)
        self.assertEqual(changed_data[last.road.offset + 9 : last.road.end], PATH0 + PATH1)

        retagged = bytearray(changed_data)
        retagged[first.offset] = 0xE7
        retagged[last.offset] = 0xD6
        tags = parse_caravans_section(retagged, 0)
        self.assertEqual((tags.owners[0].slots[0].row_tag, tags.owners[0].slots[2].row_tag), (0xE7, 0xD6))

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
        self.assertEqual(classes["Caravans"]["size"], 224)
        self.assertEqual(classes["PtrArray<Caravan>"]["size"], 28)
        self.assertEqual(classes["Caravan"]["size"], 80)
        self.assertEqual(classes["CaravanData"]["size"], 68)
        self.assertEqual(classes["Stack<PathData>"]["size"], 16)
        self.assertEqual(classes["PathData"]["size"], 16)
        spans = (
            (0x0073E3F0, 1008, "5ffd2af8c62f1cc429d3bd4be6d0c9ba0b5fb362e57da47c85b4b8434f14c244"),
            (0x0073D2B0, 71, "2ec9700539eda96c6369b80e56c375f278c9929483bf9da070fcb4473252cacb"),
            (0x0046D8B0, 239, "457efeb27bdd357c9e0b4d1f45168a220617944683a0d62b0782c1aaa306c243"),
            (0x005A2BC1, 40, "39d1e2223882a83c788685a34cbc6d129ba043233ef0014cdc084329786be992"),
            (0x004786F0, 538, "1f35d8335db5bce4f2d7351a38abcc64b8b3d624d2112af8775fbbfb594ef178"),
        )
        for va, size, digest in spans:
            with self.subTest(va=f"{va:#x}"):
                raw = _pe_offset(image, va)
                self.assertEqual(hashlib.sha256(image[raw : raw + size]).hexdigest(), digest)

    def test_fresh_svx_chains_to_exact_lands_boundary_without_replay_join(self) -> None:
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
        oil_wells = parse_oil_wells_section(save_plain, docks.end)
        supplies = parse_supplies_section(save_plain, oil_wells.end)
        self.assertEqual(supplies.end, 0x27139)
        parsed = parse_caravans_section(save_plain, supplies.end)
        self.assertEqual((parsed.offset, parsed.end, parsed.size), (0x27139, 0x2715A, 33))
        self.assertEqual(parsed.tag, 0)
        self.assertEqual([owner.length for owner in parsed.owners], [0] * 8)
        self.assertEqual(parsed.sha256, "7f9c9e31ac8256ca2f258583df262dbc7d6f68f2a03043d5c99a4ae5a7396ce9")
        changed = bytearray(save_plain)
        changed[parsed.end] ^= 1
        self.assertEqual(parse_caravans_section(changed, supplies.end), parsed)

        save_tree = savegame_parse.parse(save_plain)
        replay_tree = savegame_parse.parse(gzip.decompress(replay_raw))
        save_seed = _field(_find(save_tree, "GameInfo::walk_data"), "seed")
        replay_seed = _field(_find(replay_tree, "GameInfo::walk_data"), "seed")
        self.assertEqual((save_seed, replay_seed), ("0x014810ac", "0x007f93e0"))
        self.assertNotEqual(save_seed, replay_seed)


if __name__ == "__main__":
    unittest.main()
