#!/usr/bin/env python3
"""Exact structural, mutation, and artifact tests for savegame_cities.py."""

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
from savegame_cities import (  # noqa: E402
    OWNER_COUNT,
    TAG_CITIES,
    CitiesParseError,
    parse_cities_section,
)
from savegame_constants_block import parse_constants_block  # noqa: E402
from savegame_direct_scalars import parse_direct_scalars  # noqa: E402
from savegame_leaders import parse_leaders_section  # noqa: E402
from savegame_mountains import parse_mountains_section  # noqa: E402
from savegame_tileset import parse_tileset_section  # noqa: E402
from savegame_types import parse_types_section  # noqa: E402


PREFIX = b"PRE-CITIES"
FOLLOWING_OWNER = b"FORMS-SENTINEL" * 64
CITY_FLAGS = 0x9131
CITY_SHORTS = (3, 2004, 11)
CITY_COORDS = (0x11223344, -0x01020304)
CITY_STAMPS = (7, -8, 9, -10, 11, -12)
CITY_TRADED = tuple(100 + index for index in range(8))
CITY_MORE_SHORTS = (-1, 2, -3, 4, -5)
CITY_U8 = tuple(10 + index for index in range(8))
CITY_I8 = (-2, 3, -4)
CITY_TRAILING_U8 = tuple(20 + index for index in range(8))
CITY_SPACE = (31, 32, 33)
CITY_TER = tuple(41 + index for index in range(6))
NAME_UNITS = (ord("N"), 0x03A9)
ID_UNITS = tuple(map(ord, "City"))
CARAVAN_LINKS = ((0x10203040, 6), (-17, 2))


def _pod() -> bytes:
    out = bytearray()
    out.extend(struct.pack("<3h", *CITY_SHORTS))
    out.extend(struct.pack("<2i", *CITY_COORDS))
    out.extend(struct.pack("<6i", *CITY_STAMPS))
    out.extend(struct.pack("<8i", *CITY_TRADED))
    out.extend(struct.pack("<5h", *CITY_MORE_SHORTS))
    out.extend(struct.pack("<8B", *CITY_U8))
    out.extend(struct.pack("<3b", *CITY_I8))
    out.extend(struct.pack("<8B", *CITY_TRAILING_U8))
    out.extend(struct.pack("<3B", *CITY_SPACE))
    out.extend(struct.pack("<6B", *CITY_TER))
    assert len(out) == 108
    return bytes(out)


def _wide(units: tuple[int, ...]) -> bytes:
    return struct.pack(f"<I{len(units)}H", len(units), *units)


def _vans(capacity: int = 10, flags: int = 0x05) -> bytes:
    out = bytearray(struct.pack("<iihB", len(CARAVAN_LINKS), capacity, -1, flags))
    for cara, who in CARAVAN_LINKS:
        out.extend(struct.pack("<ii", cara, who))
    assert len(out) == 27
    return bytes(out)


def _active_city() -> bytes:
    body = (
        struct.pack("<H", CITY_FLAGS)
        + _pod()
        + _wide(NAME_UNITS)
        + _wide(ID_UNITS)
        + _vans()
    )
    assert len(body) == 157
    return body


def _synthetic() -> tuple[bytes, int, bytes]:
    out = bytearray(PREFIX)
    offset = len(out)
    out.append(TAG_CITIES)
    out.extend(struct.pack("<i", 0))

    # Owner 1 has one inactive City, one absent pointer, and one active City.
    out.extend(struct.pack("<iihB", 3, 7, 5, 0x25))
    out.extend((1, 0, 1))
    out.extend(struct.pack("<ih", 7, 5))
    out.extend(struct.pack("<H", 0x1200))
    out.extend(_active_city())

    for _ in range(2, OWNER_COUNT):
        out.extend(struct.pack("<i", 0))
    return bytes(out) + FOLLOWING_OWNER, offset, FOLLOWING_OWNER


def _canonical_save() -> bytes:
    out = bytearray((TAG_CITIES,))
    active = _active_city()
    for owner in range(OWNER_COUNT):
        out.extend(struct.pack("<iihB", 20, 20, -1, 0))
        out.extend((1,) * 20)
        out.extend(struct.pack("<ih", 20, -1))
        for slot in range(20):
            out.extend(active if owner == 0 and slot == 0 else b"\0\0")
    return bytes(out)


def _checksum_projection(data: bytes, parsed: object) -> bytes:
    active = [slot for owner in parsed.owners for slot in owner.slots if slot.active]
    out = bytearray()
    for slot in active:
        out.extend(data[slot.offset : slot.pod_end])
        out.extend(data[slot.caravans.offset : slot.caravans.end])
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


class CitiesParserTests(unittest.TestCase):
    def test_complete_dynamic_city_and_exact_forms_boundary(self) -> None:
        data, offset, following_owner = _synthetic()
        parsed = parse_cities_section(data, offset)
        self.assertEqual(parsed.tag, TAG_CITIES)
        self.assertEqual(len(parsed.owners), OWNER_COUNT)
        self.assertEqual(parsed.layout_sha256, "24fe2a7a1980c4d2a37b9f8f03de1df2afe80cfa1555dc6351b5dd0029ef3a5a")
        self.assertEqual([owner.length for owner in parsed.owners], [0, 3, 0, 0, 0, 0, 0, 0])
        self.assertEqual(data[parsed.end :], following_owner)

        owner = parsed.owners[1]
        self.assertEqual((owner.capacity, owner.increment, owner.flags), (7, 5, 0x25))
        self.assertEqual(owner.presence, (1, 0, 1))
        self.assertEqual((owner.repeated_capacity, owner.repeated_increment), (7, 5))
        inactive, absent, active = owner.slots
        self.assertEqual((inactive.city_flags, inactive.end - inactive.offset), (0x1200, 2))
        self.assertFalse(inactive.active)
        self.assertIsNone(absent.offset)
        self.assertEqual((active.city_flags, active.end - active.offset), (CITY_FLAGS, 157))
        self.assertTrue(active.active)
        self.assertEqual(active.pod_end - active.offset, 110)

        fields = {field.name: field for field in active.fields}
        self.assertEqual(fields["city"].values, (CITY_SHORTS[0],))
        self.assertEqual(fields["o"].values, (CITY_SHORTS[1],))
        self.assertEqual(fields["reg"].values, (CITY_SHORTS[2],))
        self.assertEqual((fields["x"].values[0], fields["y"].values[0]), CITY_COORDS)
        self.assertEqual(tuple(fields[name].values[0] for name in (
            "attack_stamp", "raid_stamp", "reduce_stamp", "capture_stamp",
            "assimilation_timer", "capture_strength",
        )), CITY_STAMPS)
        self.assertEqual(fields["traded_with"].values, CITY_TRADED)
        self.assertEqual(fields["space"].values, CITY_SPACE)
        self.assertEqual(fields["ter"].values, CITY_TER)
        for field in active.fields:
            self.assertEqual(field.offset, active.offset + field.pdb_offset - 4)

        self.assertEqual(active.name.code_units, NAME_UNITS)
        self.assertEqual(active.city_id.code_units, ID_UNITS)
        self.assertEqual(active.name.offset, active.pod_end)
        self.assertEqual(active.city_id.offset, active.name.end)
        self.assertEqual(active.caravans.offset, active.city_id.end)
        self.assertEqual(
            (active.caravans.length, active.caravans.capacity, active.caravans.increment, active.caravans.flags),
            (2, 10, -1, 0x05),
        )
        self.assertEqual(
            [(link.cara, link.who) for link in active.caravans.links],
            list(CARAVAN_LINKS),
        )
        self.assertEqual(parsed.sha256, hashlib.sha256(data[offset : parsed.end]).hexdigest())

    def test_every_owned_byte_mutation_is_killed_or_changes_receipt(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_cities_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[offset + relative] ^= 1
                try:
                    changed = parse_cities_section(damaged, offset)
                except CitiesParseError:
                    continue
                self.assertNotEqual(changed, baseline)
                self.assertNotEqual(changed.sha256, baseline.sha256)

    def test_every_truncation_is_rejected(self) -> None:
        data, offset, _ = _synthetic()
        end = parse_cities_section(data, offset).end
        for retained in range(end - offset):
            with self.subTest(retained=retained):
                with self.assertRaises(CitiesParseError):
                    parse_cities_section(data[: offset + retained], offset)

    def test_following_forms_mutation_is_excluded(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_cities_section(data, offset)
        damaged = bytearray(data)
        damaged[baseline.end] ^= 0xFF
        self.assertEqual(parse_cities_section(damaged, offset), baseline)

    def test_outer_and_child_structural_invariants_fail_closed(self) -> None:
        data, offset, _ = _synthetic()
        parsed = parse_cities_section(data, offset)
        owner0 = parsed.owners[0]
        owner1 = parsed.owners[1]
        city = owner1.slots[2]
        vans = city.caravans
        cases: dict[str, tuple[int, bytes]] = {
            "main tag": (offset, b"\x7f"),
            "negative owner length": (owner0.offset, struct.pack("<i", -1)),
            "oversized owner length": (owner0.offset, struct.pack("<i", (1 << 20) + 1)),
            "owner capacity below length": (owner1.offset + 4, struct.pack("<i", 2)),
            "owner writer-cleared flag": (owner1.offset + 10, b"\x65"),
            "nonboolean presence": (owner1.presence_offset + 1, b"\x02"),
            "repeated capacity": (owner1.repeated_capacity_offset, struct.pack("<i", 8)),
            "repeated increment": (owner1.repeated_increment_offset, struct.pack("<h", -1)),
            "oversized name": (city.name.offset, struct.pack("<I", 0x10000)),
            "negative vans length": (vans.offset, struct.pack("<i", -1)),
            "vans capacity below length": (vans.offset + 4, struct.pack("<i", 1)),
            "vans writer-cleared flag": (vans.offset + 10, b"\x45"),
        }
        for name, (where, replacement) in cases.items():
            with self.subTest(name=name):
                damaged = bytearray(data)
                damaged[where : where + len(replacement)] = replacement
                with self.assertRaises(CitiesParseError):
                    parse_cities_section(damaged, offset)

    def test_valid_retail_history_is_preserved_never_normalized(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_cities_section(data, offset)
        owner = baseline.owners[1]
        vans = owner.slots[2].caravans
        changed = bytearray(data)
        struct.pack_into("<i", changed, owner.offset + 4, 23)
        struct.pack_into("<i", changed, owner.repeated_capacity_offset, 23)
        struct.pack_into("<i", changed, vans.offset + 4, 13)
        parsed = parse_cities_section(changed, offset)
        self.assertEqual((parsed.owners[1].capacity, parsed.owners[1].repeated_capacity), (23, 23))
        self.assertEqual(parsed.owners[1].slots[2].caravans.capacity, 13)
        self.assertEqual(parsed.end, baseline.end)

    def test_canonical_citypool_save_projects_to_independent_checksum_authority(self) -> None:
        save_image = _canonical_save()
        parsed = parse_cities_section(save_image, 0)
        self.assertEqual(parsed.size, 772)
        self.assertTrue(all(owner.length == 20 for owner in parsed.owners))
        self.assertTrue(all(owner.capacity == 20 and owner.increment == -1 for owner in parsed.owners))
        self.assertTrue(all(owner.presence == (1,) * 20 for owner in parsed.owners))
        self.assertEqual(sum(slot.active for owner in parsed.owners for slot in owner.slots), 1)

        expected_checksum = struct.pack("<H", CITY_FLAGS) + _pod() + _vans()
        projection = _checksum_projection(save_image, parsed)
        self.assertEqual(projection, expected_checksum)
        self.assertEqual(len(projection), 137)

        root = pathlib.Path(__file__).resolve().parents[2]
        sim = (root / "crates/don-sim/src/systems/tech_cities.rs").read_text()
        replay = (root / "crates/don-replay/src/cities_runtime.rs").read_text()
        self.assertIn("pub const CITIES_PER_PLAYER: usize = 20;", sim)
        self.assertIn("pub const EMPTY_CARAVAN_CITY_WALK_BYTES: u64 = 114;", replay)

    def test_city_strings_are_save_only_in_checksum_projection(self) -> None:
        save_image = _canonical_save()
        baseline = parse_cities_section(save_image, 0)
        active = baseline.owners[0].slots[0]
        renamed = bytearray(save_image)
        renamed[active.name.offset + 4] ^= 0x20
        changed = parse_cities_section(renamed, 0)
        self.assertNotEqual(changed.sha256, baseline.sha256)
        self.assertEqual(
            _checksum_projection(bytes(renamed), changed),
            _checksum_projection(save_image, baseline),
        )

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
        self.assertEqual(classes["Cities"]["size"], 232)
        self.assertEqual(classes["PtrArray<City>"]["size"], 28)
        self.assertEqual(classes["City"]["size"], 192)
        self.assertEqual(classes["Array<CaravanLink>"]["size"], 28)
        self.assertEqual(classes["CaravanLink"]["size"], 8)
        self.assertEqual(classes["String"]["size"], 20)

        spans = (
            (0x00735410, 1054, "18037395e88ff059ce184efe46a62147fd9e16f3bb1a345e5f2a670dae636aad"),
            (0x00489220, 91, "10f5f0af14697aee3254635b56b9952c0a412b6383806c7e4a9a85b112a7ffde"),
            (0x00489040, 476, "5a5b3f42f2fdf694b68fb2a24585ed3aa54e7049871220e96c5d6a7d39159d12"),
            (0x00A1B2D0, 207, "301e6a00fb85903d10e1068d02ff73a58aa85dc72ed640f32c3a2791003805b2"),
            (0x005A2ACF, 5, "4356c51f26b1b90dc92462df90200d705cab1e1005db9fdb3f420b375e5eaadf"),
            (0x005A2ADE, 21, "d259f614f270769b0c91c7c6a97257314ee3ac8e7460f22ed72bb446d85e27af"),
            (0x00481190, 532, "033e1f6fd5961bf88c6ebc7d7af88da7b1678e8d8d8cf30e2e11bce09ecb9b19"),
        )
        for va, size, digest in spans:
            with self.subTest(va=f"{va:#x}"):
                raw = _pe_offset(image, va)
                self.assertEqual(hashlib.sha256(image[raw : raw + size]).hexdigest(), digest)

    def test_fresh_svx_chains_to_exact_forms_boundary_without_replay_join(self) -> None:
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
        armies = parse_armies_section(save_plain, scalars.end)
        self.assertEqual(armies.end, 0x2701F)
        parsed = parse_cities_section(save_plain, armies.end)
        self.assertEqual((parsed.offset, parsed.end, parsed.size), (0x2701F, 0x27040, 33))
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
