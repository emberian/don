#!/usr/bin/env python3
"""Exact structural, mutation, artifact, and authority tests for Goods saves."""

from __future__ import annotations

import gzip
import hashlib
import json
import pathlib
import struct
import sys
import unittest
import zlib


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import savegame_parse  # noqa: E402
from savegame_armies import parse_armies_section  # noqa: E402
from savegame_cities import parse_cities_section  # noqa: E402
from savegame_constants_block import parse_constants_block  # noqa: E402
from savegame_direct_scalars import parse_direct_scalars  # noqa: E402
from savegame_forms import parse_forms_section  # noqa: E402
from savegame_goods import GoodsParseError, parse_goods_section  # noqa: E402
from savegame_leaders import parse_leaders_section  # noqa: E402
from savegame_mountains import parse_mountains_section  # noqa: E402
from savegame_tileset import parse_tileset_section  # noqa: E402
from savegame_types import parse_types_section  # noqa: E402


PREFIX = b"PRE-GOODS"
FOLLOWING_OWNER = b"ITEMS-SENTINEL" * 32


def _dormant(tag: int = 0x5A, subtag: int = 0x6A) -> bytes:
    # Good tag, ever_seen, SubObject tag, flags, must_walk=false.
    return bytes((tag, 7, subtag, 0, 0))


def _active(tag: int = 0x5B, subtag: int = 0x6B) -> bytes:
    # Good tag, ever_seen, SubObject tag, flags, must_walk=true, followed by
    # the exact [+9,+24) SubObject range and the resolved TypeIndex.
    return bytes((tag, 8, subtag, 1, 1, 0xFF)) + struct.pack(
        "<hiiii", 2, 0x63637, 0x637B7, 0x63A77, 5
    )


def _synthetic() -> tuple[bytes, int, bytes]:
    out = bytearray(PREFIX)
    offset = len(out)
    out.extend(struct.pack("<iihB", 3, 7, 5, 0x21))
    out.extend((1, 0, 1))
    out.extend(struct.pack("<ih", 7, 5))
    out.extend(_dormant())
    out.extend(_active())
    return bytes(out) + FOLLOWING_OWNER, offset, FOLLOWING_OWNER


def _history_fixture(
    capacity: int = 4, increment: int = -1, flags: int = 0
) -> bytes:
    out = bytearray(struct.pack("<iihB", 2, capacity, increment, flags))
    out.extend((1, 1))
    out.extend(struct.pack("<ih", capacity, increment))
    out.extend(_dormant())
    out.extend(_active())
    return bytes(out)


def _retail_checksum_rows(data: bytes, parsed) -> bytes:
    """Project CheckSums::check_goods, independently of generic-save tags."""
    out = bytearray()
    for slot in parsed.slots:
        if not slot.present or slot.flags is None or not slot.flags & 1:
            continue
        # CheckSum::walk_test is a no-op. Good::walk_data still walks
        # ever_seen; SubObject walks flags, must_walk, its 15-byte range, and
        # TypeIndex.  must_walk is a real DataWalk byte, not a control-only bit.
        raw = data[slot.offset : slot.end]
        out.extend(raw[1:2])  # skip Good tag
        out.extend(raw[3:])   # skip SubObject tag, retain flags + must + tail
    return bytes(out)


def _landed_21_byte_projection(data: bytes, parsed) -> bytes:
    """Model the currently landed GoodNode::walked_bytes projection exactly."""
    out = bytearray()
    for slot in parsed.slots:
        if not slot.present or slot.flags is None or not slot.flags & 1:
            continue
        raw = data[slot.offset : slot.end]
        out.extend(raw[1:2])
        out.extend(raw[3:4])
        out.extend(raw[5:])  # currently omits must_walk at raw[4]
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


class GoodsParserTests(unittest.TestCase):
    def test_complete_sparse_history_and_exact_items_boundary(self) -> None:
        data, offset, following_owner = _synthetic()
        parsed = parse_goods_section(data, offset)
        self.assertEqual(
            (parsed.length, parsed.capacity, parsed.increment, parsed.flags),
            (3, 7, 5, 0x21),
        )
        self.assertEqual(parsed.presence, (1, 0, 1))
        self.assertEqual(
            (parsed.repeated_capacity, parsed.repeated_increment), (7, 5)
        )
        self.assertEqual(
            parsed.layout_sha256,
            "597622152a52d474951aa920abf388281366561adc263933ec79e3a89a12d3a2",
        )
        self.assertEqual(data[parsed.end :], following_owner)
        self.assertEqual(parsed.sha256, hashlib.sha256(data[offset : parsed.end]).hexdigest())

        dormant, hole, active = parsed.slots
        self.assertEqual(
            (
                dormant.present,
                dormant.good_tag,
                dormant.ever_seen,
                dormant.subobject_tag,
                dormant.flags,
                dormant.must_walk,
                dormant.size,
            ),
            (True, 0x5A, 7, 0x6A, 0, 0, 5),
        )
        self.assertFalse(hole.present)
        self.assertEqual(hole.presence_offset, parsed.presence_offset + 1)
        self.assertEqual(
            (
                active.present,
                active.good_tag,
                active.ever_seen,
                active.subobject_tag,
                active.flags,
                active.must_walk,
                active.who,
                active.o,
                active.z_internal,
                active.x_internal,
                active.y_internal,
                active.type_index,
                active.size,
            ),
            (True, 0x5B, 8, 0x6B, 1, 1, 0xFF, 2, 0x63637, 0x637B7, 0x63A77, 5, 24),
        )
        self.assertEqual(active.must_walk_offset, active.offset + 4)

    def test_every_owned_byte_mutation_is_killed_or_changes_receipt(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_goods_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[offset + relative] ^= 1
                try:
                    changed = parse_goods_section(damaged, offset)
                except GoodsParseError:
                    continue
                self.assertNotEqual(changed, baseline)
                self.assertNotEqual(changed.sha256, baseline.sha256)

    def test_every_truncation_is_rejected(self) -> None:
        data, offset, _ = _synthetic()
        end = parse_goods_section(data, offset).end
        for retained in range(end - offset):
            with self.subTest(retained=retained):
                with self.assertRaises(GoodsParseError):
                    parse_goods_section(data[: offset + retained], offset)

    def test_following_items_mutation_is_excluded(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_goods_section(data, offset)
        damaged = bytearray(data)
        damaged[baseline.end] ^= 0xFF
        self.assertEqual(parse_goods_section(damaged, offset), baseline)

    def test_structural_invariants_fail_closed(self) -> None:
        data, offset, _ = _synthetic()
        parsed = parse_goods_section(data, offset)
        cases: dict[str, tuple[int, bytes]] = {
            "negative length": (offset, struct.pack("<i", -1)),
            "oversized length": (offset, struct.pack("<i", (1 << 20) + 1)),
            "capacity below length": (offset + 4, struct.pack("<i", 2)),
            "writer-cleared flags": (offset + 10, b"\x61"),
            "nonboolean presence": (parsed.presence_offset + 1, b"\x02"),
            "repeated capacity mismatch": (
                parsed.repeated_capacity_offset,
                struct.pack("<i", 8),
            ),
            "repeated increment mismatch": (
                parsed.repeated_increment_offset,
                struct.pack("<h", 6),
            ),
            "nonboolean must_walk": (parsed.slots[0].must_walk_offset, b"\x02"),
        }
        for name, (where, replacement) in cases.items():
            with self.subTest(name=name):
                damaged = bytearray(data)
                damaged[where : where + len(replacement)] = replacement
                with self.assertRaises(GoodsParseError):
                    parse_goods_section(damaged, offset)

    def test_valid_container_history_and_tags_are_preserved(self) -> None:
        baseline = parse_goods_section(_history_fixture(), 0)
        changed_data = _history_fixture(capacity=99, increment=-17, flags=0x25)
        changed = parse_goods_section(changed_data, 0)
        self.assertEqual((changed.capacity, changed.increment, changed.flags), (99, -17, 0x25))
        self.assertEqual((changed.repeated_capacity, changed.repeated_increment), (99, -17))
        self.assertEqual(changed.end, baseline.end)

        tags = bytearray(changed_data)
        tags[changed.slots[0].offset] = 0xE7
        tags[changed.slots[1].offset + 2] = 0xD6
        retagged = parse_goods_section(tags, 0)
        self.assertEqual(retagged.slots[0].good_tag, 0xE7)
        self.assertEqual(retagged.slots[1].subobject_tag, 0xD6)

    def test_retail_checksum_projection_proves_landed_missing_must_walk_byte(self) -> None:
        data = _history_fixture()
        parsed = parse_goods_section(data, 0)
        retail = _retail_checksum_rows(data, parsed)
        landed = _landed_21_byte_projection(data, parsed)
        self.assertEqual(len(retail), 22)
        self.assertEqual(len(landed), 21)
        self.assertEqual(retail[2], 1)  # SubObject::must_walk's DataWalk byte.
        self.assertEqual(retail[:2] + retail[3:], landed)

        # A one-bit mutation of the PE-proven must_walk byte changes the retail
        # checksum.  The landed 21-byte projection cannot observe that byte at all.
        mutated = bytearray(retail)
        mutated[2] ^= 1
        self.assertNotEqual(zlib.adler32(retail), zlib.adler32(mutated))
        self.assertEqual(zlib.adler32(landed), zlib.adler32(retail[:2] + retail[3:]))

        root = pathlib.Path(__file__).resolve().parents[2]
        economy = (root / "crates/don-sim/src/systems/economy.rs").read_text()
        oil = (root / "crates/don-sim/src/systems/world_oil_goods.rs").read_text()
        self.assertIn("pub fn walked_bytes(self) -> [u8; 21]", economy)
        self.assertIn("pub const GOOD_WALKED_BYTES: usize = 21;", oil)

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
        self.assertEqual(classes["PtrArray<Good>"]["size"], 28)
        self.assertEqual(classes["Good"]["size"], 48)
        self.assertEqual(classes["GoodData"]["size"], 48)
        self.assertEqual(classes["SubObject"]["size"], 40)
        self.assertEqual(classes["SubObjectData"]["size"], 28)

        spans = (
            (0x0045CCE0, 830, "e68361521d98ae326acba23195fb5f0650fbfc010a8697884b54df21376cdd5b"),
            (0x0066E5D0, 59, "ab6be8aad00a37925d5ddd62fe606cbd22fd82b9f2e236b93961d9c61be8949e"),
            (0x006621D0, 216, "21da7fd9c4c463d308a7a8ec90cd867befb03d21920dd953e3c24cc8a74f51a1"),
            (0x006623A0, 54, "121e715dc032544828bfae099900a496bd4934afbec24c6021f70ad399c1c2fd"),
            (0x00937710, 123, "c52132ff0289da6fd1143c0a3aad5a0b0bd21142a0258e9906c44b72c954c091"),
            (0x00936FF0, 43, "c73ec95b4dd8cd5259be8210894b38eed2cd88033d16e2d69916969189fd94d4"),
            (0x0045DEC0, 105, "583d9320c3780b9fdc47551d011daaa0fe8b8d59b04a4cb11ce69954a0d6b818"),
            (0x005A2B04, 36, "af6606034455df65d4478ff2d9723ea11cb008b3d06519592ecf838d9d14f013"),
            (0x0045D020, 797, "d4d11d0acccd5b7ea82cf2e3ddcf6b97eb393da039167ff32a0f047de29b9c0e"),
        )
        for va, size, digest in spans:
            with self.subTest(va=f"{va:#x}"):
                raw = _pe_offset(image, va)
                self.assertEqual(hashlib.sha256(image[raw : raw + size]).hexdigest(), digest)

    def test_fresh_svx_chains_to_exact_items_boundary_without_replay_join(self) -> None:
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
        cities = parse_cities_section(save_plain, armies.end)
        forms = parse_forms_section(save_plain, cities.end)
        self.assertEqual(forms.end, 0x27045)
        parsed = parse_goods_section(save_plain, forms.end)
        self.assertEqual((parsed.offset, parsed.end, parsed.size), (0x27045, 0x27049, 4))
        self.assertEqual(parsed.length, 0)
        self.assertEqual(parsed.sha256, "df3f619804a92fdb4057192dc43dd748ea778adc52bc498ce80524c014b81119")

        save_tree = savegame_parse.parse(save_plain)
        replay_tree = savegame_parse.parse(gzip.decompress(replay_raw))
        save_seed = _field(_find(save_tree, "GameInfo::walk_data"), "seed")
        replay_seed = _field(_find(replay_tree, "GameInfo::walk_data"), "seed")
        self.assertEqual(save_seed, "0x014810ac")
        self.assertEqual(replay_seed, "0x007f93e0")
        self.assertNotEqual(save_seed, replay_seed)


if __name__ == "__main__":
    unittest.main()
