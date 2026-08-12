#!/usr/bin/env python3
"""Exact structural, mutation, and artifact tests for savegame_armies.py."""

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
from savegame_armies import (  # noqa: E402
    ARMY_TAG_STRING_TABLE_INDEX,
    OWNER_COUNT,
    TAG_ARMIES,
    ArmiesParseError,
    parse_armies_section,
)
from savegame_constants_block import parse_constants_block  # noqa: E402
from savegame_direct_scalars import parse_direct_scalars  # noqa: E402
from savegame_leaders import parse_leaders_section  # noqa: E402
from savegame_mountains import parse_mountains_section  # noqa: E402
from savegame_tileset import parse_tileset_section  # noqa: E402
from savegame_types import parse_types_section  # noqa: E402


PREFIX = b"PRE-ARMIES"
FOLLOWING_OWNER = b"CITIES-SENTINEL" * 32
ARMY_SCALAR_VALUES = tuple(101 + index * 11 for index in range(20))
ARMY_LIST_VALUES = tuple(-1000 - index for index in range(16))


def _live_tail() -> bytes:
    tail = struct.pack(
        "<h20i16i2h",
        -7,
        *ARMY_SCALAR_VALUES,
        *ARMY_LIST_VALUES,
        -3,
        2,
    )
    assert len(tail) == 150
    return tail


def _synthetic() -> tuple[bytes, int, bytes]:
    out = bytearray(PREFIX)
    offset = len(out)
    out.append(TAG_ARMIES)
    out.extend(struct.pack("<i", 0))  # owner 0: empty branch

    # owner 1: three pointer slots, one absent, one invalid, one live.
    out.extend(struct.pack("<iihB", 3, 4, -1, 0x21))
    out.extend((1, 0, 1))
    out.extend(struct.pack("<ih", 4, -1))
    out.extend(struct.pack("<Bh", 0x5A, 0))
    out.extend(struct.pack("<Bh", 0x5B, 1))
    out.extend(_live_tail())

    for _ in range(2, OWNER_COUNT):
        out.extend(struct.pack("<i", 0))
    return bytes(out) + FOLLOWING_OWNER, offset, FOLLOWING_OWNER


def _canonical_save() -> bytes:
    out = bytearray((TAG_ARMIES,))
    for owner in range(OWNER_COUNT):
        out.extend(struct.pack("<iihB", 16, 16, -1, 0))
        out.extend((1,) * 16)
        out.extend(struct.pack("<ih", 16, -1))
        for slot in range(16):
            out.extend(struct.pack("<Bh", owner * 16 + slot, 0))
    return bytes(out)


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


class ArmiesParserTests(unittest.TestCase):
    def test_complete_dynamic_layout_and_exact_next_owner(self) -> None:
        data, offset, following_owner = _synthetic()
        parsed = parse_armies_section(data, offset)
        self.assertEqual(parsed.tag, TAG_ARMIES)
        self.assertEqual(len(parsed.owners), OWNER_COUNT)
        self.assertEqual(parsed.layout_sha256, "cfeef26e4db7667fc842ac307239fdebfc12c4339f7c476a74430b9c1309c066")
        self.assertEqual([owner.length for owner in parsed.owners], [0, 3, 0, 0, 0, 0, 0, 0])
        self.assertEqual(data[parsed.end :], following_owner)

        owner = parsed.owners[1]
        self.assertEqual((owner.capacity, owner.increment, owner.flags), (4, -1, 0x21))
        self.assertEqual(owner.presence, (1, 0, 1))
        self.assertEqual((owner.repeated_capacity, owner.repeated_increment), (4, -1))
        self.assertEqual([slot.present for slot in owner.slots], [True, False, True])

        invalid, absent, live = owner.slots
        self.assertEqual((invalid.tag, invalid.valid, invalid.end - invalid.offset), (0x5A, 0, 3))
        self.assertEqual(invalid.fields, ())
        self.assertIsNone(absent.offset)
        self.assertEqual((live.tag, live.valid, live.end - live.offset), (0x5B, 1, 153))
        self.assertEqual(ARMY_TAG_STRING_TABLE_INDEX, 134)

        fields = {field.name: field for field in live.fields}
        self.assertEqual(tuple(fields), (
            "army", "status", "reg", "role", "num_units", "num_captains",
            "num_standard", "num_decoys", "city", "navy", "human_frame",
            "hurry", "target_o", "target_who", "x", "y", "angle",
            "rally_dist", "muster_x", "muster_y", "muster_angle", "list",
            "who", "num_groups",
        ))
        self.assertEqual(fields["army"].values, (-7,))
        scalar_names = tuple(fields)[1:21]
        self.assertEqual(tuple(fields[name].values[0] for name in scalar_names), ARMY_SCALAR_VALUES)
        self.assertEqual(fields["list"].values, ARMY_LIST_VALUES)
        self.assertEqual((fields["who"].values, fields["num_groups"].values), ((-3,), (2,)))
        for field in live.fields:
            self.assertEqual(field.offset, live.offset + 1 + field.pdb_offset)
            self.assertEqual(field.end - field.offset, 64 if field.name == "list" else (2 if field.type_name == "short" else 4))

        self.assertEqual(
            parsed.sha256,
            hashlib.sha256(data[offset : parsed.end]).hexdigest(),
        )

    def test_every_owned_byte_mutation_is_killed_or_changes_receipt(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_armies_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[offset + relative] ^= 1
                try:
                    changed = parse_armies_section(damaged, offset)
                except ArmiesParseError:
                    continue
                self.assertNotEqual(changed, baseline)
                self.assertNotEqual(changed.sha256, baseline.sha256)

    def test_every_truncation_is_rejected(self) -> None:
        data, offset, _ = _synthetic()
        end = parse_armies_section(data, offset).end
        for retained in range(end - offset):
            with self.subTest(retained=retained):
                with self.assertRaises(ArmiesParseError):
                    parse_armies_section(data[: offset + retained], offset)

    def test_following_cities_mutation_is_excluded(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_armies_section(data, offset)
        damaged = bytearray(data)
        damaged[baseline.end] ^= 0xFF
        self.assertEqual(parse_armies_section(damaged, offset), baseline)

    def test_structural_invariants_fail_closed(self) -> None:
        data, offset, _ = _synthetic()
        parsed = parse_armies_section(data, offset)
        owner0 = parsed.owners[0]
        owner1 = parsed.owners[1]
        cases: dict[str, tuple[int, bytes]] = {
            "main tag": (offset, b"\x7f"),
            "negative length": (owner0.offset, struct.pack("<i", -1)),
            "oversized length": (owner0.offset, struct.pack("<i", (1 << 20) + 1)),
            "capacity below length": (owner1.offset + 4, struct.pack("<i", 2)),
            "writer-cleared flag": (owner1.offset + 10, b"\x61"),
            "nonboolean presence": (owner1.presence_offset + 1, b"\x02"),
            "repeated capacity": (owner1.repeated_capacity_offset, struct.pack("<i", 5)),
            "repeated increment": (owner1.repeated_increment_offset, struct.pack("<h", 2)),
        }
        for name, (where, replacement) in cases.items():
            with self.subTest(name=name):
                damaged = bytearray(data)
                damaged[where : where + len(replacement)] = replacement
                with self.assertRaises(ArmiesParseError):
                    parse_armies_section(damaged, offset)

    def test_per_army_tag_value_is_preserved_not_inferred(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_armies_section(data, offset)
        slot = baseline.owners[1].slots[0]
        damaged = bytearray(data)
        damaged[slot.offset] = 0xE7
        changed = parse_armies_section(damaged, offset)
        self.assertEqual(changed.owners[1].slots[0].tag, 0xE7)
        self.assertEqual(changed.end, baseline.end)

    def test_canonical_save_projects_exactly_to_landed_checksum_authority(self) -> None:
        save_image = _canonical_save()
        parsed = parse_armies_section(save_image, 0)
        self.assertEqual(parsed.size, 649)
        self.assertTrue(all(owner.length == 16 for owner in parsed.owners))
        self.assertTrue(all(slot.present and slot.valid == 0 for owner in parsed.owners for slot in owner.slots))

        save_only_tags = {0}
        save_only_tags.update(
            slot.offset
            for owner in parsed.owners
            for slot in owner.slots
            if slot.offset is not None
        )
        checksum_projection = bytes(
            value for index, value in enumerate(save_image) if index not in save_only_tags
        )

        expected = bytearray()
        for _ in range(OWNER_COUNT):
            expected.extend(struct.pack("<iihB", 16, 16, -1, 0))
            expected.extend((1,) * 16)
            expected.extend(struct.pack("<ih", 16, -1))
            expected.extend(struct.pack("<h", 0) * 16)
        self.assertEqual(checksum_projection, bytes(expected))
        self.assertEqual(len(checksum_projection), 520)

        root = pathlib.Path(__file__).resolve().parents[2]
        authority = (root / "crates/don-sim/src/systems/armies.rs").read_text()
        self.assertIn("pub const INITIAL_ARMIES_WALK_BYTES", authority)
        self.assertIn("assert_eq!(INITIAL_ARMIES_WALK_BYTES, 520);", authority)

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
        self.assertEqual(classes["Armies"]["size"], 232)
        self.assertEqual(classes["PtrArray<Army>"]["size"], 28)
        self.assertEqual(classes["Army"]["size"], 160)
        self.assertEqual(classes["ArmyData"]["size"], 152)

        spans = (
            (0x006F3700, 1013, "df6b9495d0e22172cd4c15ee5bbfa84256a1301cd73b6166ffe360162b407bf3"),
            (0x006F9850, 70, "b23392fc6767bc00c9a690c11eb008a2afb74d9206f1f9bec4cf2d938cfd5755"),
            (0x0041BFE0, 4, "04e14830bb9d4a620dd18b3c17ecd4689aff9d59b77248a8863a856ea412b944"),
            (0x005A2ABF, 5, "ea79d267ba97762714f71ed4a340437af6668ef8539dc1d2b5fb09edd122b8d4"),
            (0x00735410, 32, "4a1a04231ef75fd200a3d8123f9614d82bcf871bf52b00a30e23f4ca31bae1f3"),
        )
        for va, size, digest in spans:
            with self.subTest(va=f"{va:#x}"):
                raw = _pe_offset(image, va)
                self.assertEqual(hashlib.sha256(image[raw : raw + size]).hexdigest(), digest)

    def test_fresh_svx_chains_to_exact_cities_boundary_without_replay_join(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]
        save = root / "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX"
        replay = root / "ron-data/replays/Playback - 2026.08.11 11'44'38 (Tue).rcx"
        if not save.exists() or not replay.exists():
            self.skipTest("user-owned fresh SVX/RCX fixtures are not installed")

        save_raw = save.read_bytes()
        replay_raw = replay.read_bytes()
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
        self.assertEqual(scalars.end, 0x26FFE)
        parsed = parse_armies_section(save_plain, scalars.end)
        self.assertEqual((parsed.offset, parsed.end, parsed.size), (0x26FFE, 0x2701F, 33))
        self.assertEqual([owner.length for owner in parsed.owners], [0] * OWNER_COUNT)
        self.assertEqual(parsed.sha256, "7f9c9e31ac8256ca2f258583df262dbc7d6f68f2a03043d5c99a4ae5a7396ce9")

        save_tree = savegame_parse.parse(save_plain)
        replay_tree = savegame_parse.parse(gzip.decompress(replay_raw))
        save_seed = _field(_find(save_tree, "GameInfo::walk_data"), "seed")
        replay_seed = _field(_find(replay_tree, "GameInfo::walk_data"), "seed")
        self.assertEqual(save_seed, "0x014810ac")
        self.assertEqual(replay_seed, "0x007f93e0")
        self.assertNotEqual(save_seed, replay_seed)


if __name__ == "__main__":
    unittest.main()
