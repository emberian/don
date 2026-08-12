#!/usr/bin/env python3
"""Exact mutation, artifact, and boundary tests for LeaderOptions saves."""

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
from savegame_caravans import parse_caravans_section  # noqa: E402
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
from savegame_lands import parse_lands_section  # noqa: E402
from savegame_leader_options import (  # noqa: E402
    ROW_TAG_STRING_TABLE_INDEX,
    TAG_STRING_TABLE_INDEX,
    LeaderOptionsParseError,
    parse_leader_options_section,
)
from savegame_leaders import parse_leaders_section  # noqa: E402
from savegame_mountains import parse_mountains_section  # noqa: E402
from savegame_oil_wells import parse_oil_wells_section  # noqa: E402
from savegame_specials import parse_specials_section  # noqa: E402
from savegame_supplies import parse_supplies_section  # noqa: E402
from savegame_tileset import parse_tileset_section  # noqa: E402
from savegame_types import parse_types_section  # noqa: E402
from savegame_wonders import parse_wonders_section  # noqa: E402


PREFIX = b"PRE-LEADER-OPTIONS"
FOLLOWING_OWNER = b"OPTION-INFO-SENTINEL" * 32
BIT_COUNTS = (0, 1, 7, 8, 9, 16, 17, 24, 31, 32)


def _row(index: int, bits: int) -> bytes:
    mask_size = (bits + 7) // 8
    mask = bytes(((index * 29 + byte * 17) & 0xFF) for byte in range(mask_size))
    return bytes((0x40 + index,)) + struct.pack(
        "<iiiiii",
        index - 5,
        index * 3 - 11,
        100 - index * 7,
        -200 + index * 13,
        bits,
        mask_size,
    ) + mask


ROWS = tuple(_row(index, bits) for index, bits in enumerate(BIT_COUNTS))


def _section() -> bytes:
    return b"\0" + b"".join(ROWS)


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


class LeaderOptionsParserTests(unittest.TestCase):
    def test_all_ten_rows_and_exact_option_info_boundary(self) -> None:
        data, offset, following_owner = _synthetic()
        parsed = parse_leader_options_section(data, offset)
        self.assertEqual(
            (TAG_STRING_TABLE_INDEX, ROW_TAG_STRING_TABLE_INDEX), (4614, 4613)
        )
        self.assertEqual(
            parsed.layout_sha256,
            "48086cd9601a5ae00269549dbb931436fc6930a92f8d29d906302aebff47c54a",
        )
        self.assertEqual(len(parsed.rows), 10)
        self.assertEqual(data[parsed.end :], following_owner)
        self.assertEqual(
            parsed.sha256, hashlib.sha256(data[offset : parsed.end]).hexdigest()
        )
        for index, row in enumerate(parsed.rows):
            with self.subTest(index=index):
                self.assertEqual(row.row_tag, 0x40 + index)
                self.assertEqual(
                    (row.who, row.peasants, row.peasants_wait, row.buildings),
                    (index - 5, index * 3 - 11, 100 - index * 7, -200 + index * 13),
                )
                self.assertEqual(row.bit_count, BIT_COUNTS[index])
                self.assertEqual(row.mask_size, (BIT_COUNTS[index] + 7) // 8)
                self.assertEqual(row.size, 25 + row.mask_size)
                self.assertEqual(data[row.offset : row.end], ROWS[index])
                self.assertEqual(
                    data[row.mask_offset : row.end], bytes(row.mask)
                )

    def test_every_owned_byte_mutation_is_killed_or_changes_receipt(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_leader_options_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[offset + relative] ^= 1
                try:
                    changed = parse_leader_options_section(damaged, offset)
                except LeaderOptionsParseError:
                    continue
                self.assertNotEqual(changed, baseline)
                self.assertNotEqual(changed.sha256, baseline.sha256)

    def test_every_truncation_is_rejected(self) -> None:
        data, offset, _ = _synthetic()
        end = parse_leader_options_section(data, offset).end
        for retained in range(end - offset):
            with self.subTest(retained=retained):
                with self.assertRaises(LeaderOptionsParseError):
                    parse_leader_options_section(data[: offset + retained], offset)

    def test_following_option_info_mutation_is_excluded(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_leader_options_section(data, offset)
        damaged = bytearray(data)
        damaged[baseline.end] ^= 0xFF
        self.assertEqual(parse_leader_options_section(damaged, offset), baseline)

    def test_tag_and_bitmask_invariants_fail_closed(self) -> None:
        data, offset, _ = _synthetic()
        parsed = parse_leader_options_section(data, offset)
        row = parsed.rows[4]
        bit_count_offset = row.offset + 17
        mask_size_offset = row.offset + 21
        cases: dict[str, tuple[int, bytes]] = {
            "wrong outer tag": (offset, b"\x01"),
            "negative bit count": (bit_count_offset, struct.pack("<i", -1)),
            "bit count above capacity": (bit_count_offset, struct.pack("<i", 33)),
            "negative mask size": (mask_size_offset, struct.pack("<i", -1)),
            "undersized mask": (mask_size_offset, struct.pack("<i", 1)),
            "oversized mask": (mask_size_offset, struct.pack("<i", 3)),
        }
        for name, (where, replacement) in cases.items():
            with self.subTest(name=name):
                damaged = bytearray(data)
                damaged[where : where + len(replacement)] = replacement
                with self.assertRaises(LeaderOptionsParseError):
                    parse_leader_options_section(damaged, offset)

    def test_row_tags_and_dynamic_mask_payloads_are_preserved(self) -> None:
        data = bytearray(_section())
        baseline = parse_leader_options_section(data, 0)
        for index, row in enumerate(baseline.rows):
            data[row.offset] = 0xE0 + index
            for byte in range(row.mask_size):
                data[row.mask_offset + byte] ^= 0xA5
        changed = parse_leader_options_section(data, 0)
        self.assertEqual([row.row_tag for row in changed.rows], list(range(0xE0, 0xEA)))
        self.assertNotEqual(changed.sha256, baseline.sha256)
        for before, after in zip(baseline.rows, changed.rows):
            self.assertEqual(after.bit_count, before.bit_count)
            self.assertEqual(after.mask_size, before.mask_size)
            if before.mask_size:
                self.assertNotEqual(after.mask, before.mask)

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
        self.assertEqual(classes["LeaderOptions"]["size"], 320)
        self.assertEqual(classes["LeaderOption"]["size"], 32)
        self.assertEqual(classes["LeaderOptionData"]["size"], 32)
        self.assertEqual(classes["BitMask<32>"]["size"], 16)
        spans = (
            (0x006F19A0, 179, "80833da1c6a3adbe29304b65f7f4ad82157fe1f7dfc2200798c445e5e5823c4d"),
            (0x006F1DB0, 31, "fe8b08a34a2bd6b00b74f82442916ca85eb1b7dd298c7409475bf8803b9fa057"),
            (0x005A2BF7, 22, "6d15abf7b303a2e65131698f13964e938c908dcce780c4e84c37489b08a110ba"),
            (0x0072C1E0, 89, "93fcab6ac7619e0e9786d0e819d6de794b37690729a27fed75f992981da25247"),
        )
        for va, size, digest in spans:
            with self.subTest(va=f"{va:#x}"):
                raw = _pe_offset(image, va)
                self.assertEqual(hashlib.sha256(image[raw : raw + size]).hexdigest(), digest)

    def test_fresh_svx_chains_to_exact_option_info_boundary_without_replay_join(self) -> None:
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
        caravans = parse_caravans_section(save_plain, supplies.end)
        lands = parse_lands_section(save_plain, caravans.end)
        self.assertEqual(lands.end, 0x2715F)
        parsed = parse_leader_options_section(save_plain, lands.end)
        self.assertEqual((parsed.offset, parsed.end, parsed.size), (0x2715F, 0x2725A, 251))
        self.assertEqual(parsed.tag, 0)
        self.assertEqual([row.row_tag for row in parsed.rows], [0] * 10)
        self.assertEqual([(row.bit_count, row.mask_size) for row in parsed.rows], [(0, 0)] * 10)
        self.assertEqual(parsed.sha256, "e258fc78e23908bdff0123123cb31e7a81008118ab1188ddcb740360727add4f")
        changed = bytearray(save_plain)
        changed[parsed.end] ^= 1
        self.assertEqual(parse_leader_options_section(changed, lands.end), parsed)

        save_tree = savegame_parse.parse(save_plain)
        replay_tree = savegame_parse.parse(gzip.decompress(replay_raw))
        save_seed = _field(_find(save_tree, "GameInfo::walk_data"), "seed")
        replay_seed = _field(_find(replay_tree, "GameInfo::walk_data"), "seed")
        self.assertEqual((save_seed, replay_seed), ("0x014810ac", "0x007f93e0"))
        self.assertNotEqual(save_seed, replay_seed)


if __name__ == "__main__":
    unittest.main()
