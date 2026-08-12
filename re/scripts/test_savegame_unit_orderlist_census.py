#!/usr/bin/env python3
"""Structural, mutation, artifact, and canonical-owner tests for the census."""

from __future__ import annotations

import gzip
import hashlib
import pathlib
import struct
import sys
import unittest
from collections import Counter


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from savegame_unit_orderlist_census import (  # noqa: E402
    BUILD_OBJECT_TYPE,
    BUILD_TAG,
    OBJECTS_FIXED_BYTES,
    OBJECTS_TAG,
    OBJECT_TAG,
    SUBOBJECT_TAG,
    UNIT_OBJECT_TYPE,
    UNIT_TAG,
    UnitOrderListCensusError,
    encode_retail_payload,
    parse_unit_orderlist_census,
)


MOVE_VALUES = (
    101,
    -202,
    303,
    1,
    48,
    6,
    7,
    8,
    9,
    10,
    11,
    12,
    -1,
    -2,
    15,
    16,
    17,
    18,
    -19,
    20,
)


def _active_unit() -> bytes:
    out = bytearray()
    out.extend((SUBOBJECT_TAG, 0x41, 1, 2))
    out.extend(struct.pack("<hiii", 17, 300, 400, 500))
    out.extend(struct.pack("<i", 69))
    out.extend((OBJECT_TAG, 1))
    out.extend(bytes(range(34)))
    out.append(0)  # Object::launching absent
    out.extend((UNIT_TAG, 1))
    out.extend(bytes((index * 7) & 0xFF for index in range(111)))
    out.extend(struct.pack("<iib", 2, 1, -1))
    out.extend(struct.pack("<4i", 1000, 2000, 48, 1))

    out.extend(struct.pack("<i", 4))
    out.extend(struct.pack("<iBB18i2h", 1, 0xFE, 0xA5, *MOVE_VALUES))
    out.extend(struct.pack("<iBBiiH", 6, 7, 4, 2007, 1, 15))
    out.extend(
        struct.pack(
            "<iBBiiH4i4B",
            7,
            8,
            0,
            2001,
            0,
            1,
            270,
            116,
            418,
            295,
            0,
            1,
            4,
            1,
        )
    )
    out.extend(
        struct.pack(
            "<iBBiiH4i",
            14,
            9,
            0,
            -1,
            -1,
            0xFFFF,
            -1,
            -1,
            1,
            658,
        )
    )

    # PtrArray<Guy>: one live GuyData image.
    out.extend(struct.pack("<iihB", 1, 1, 1, 0))
    out.append(1)
    out.extend(struct.pack("<ih", 1, 1))
    out.extend(bytes((index * 11) & 0xFF for index in range(155)))
    return bytes(out)


def _inactive_build() -> bytes:
    return bytes(
        (
            0xFF,
            0,
            SUBOBJECT_TAG,
            0,
            0,
            OBJECT_TAG,
            0,
            0,
            0x7E,
            0,
            BUILD_TAG,
            0,
        )
    )


def _owner_array(bodies: tuple[tuple[int, bytes], ...]) -> bytes:
    length = len(bodies)
    out = bytearray(struct.pack("<i", length))
    if not length:
        return bytes(out)
    out.extend(struct.pack("<ihB", length, -1, 0))
    out.extend((1,) * length)
    for type_code, _ in bodies:
        out.extend(struct.pack("<i", type_code))
    out.extend(struct.pack("<ih", length, -1))
    for _, body in bodies:
        out.extend(body)
    return bytes(out)


def _synthetic() -> tuple[bytes, int]:
    out = bytearray((OBJECTS_TAG,))
    out.extend(bytes((index * 13) & 0xFF for index in range(OBJECTS_FIXED_BYTES)))
    out.extend(
        _owner_array(
            (
                (UNIT_OBJECT_TYPE, _active_unit()),
                (BUILD_OBJECT_TYPE, _inactive_build()),
            )
        )
    )
    for _ in range(8):
        out.extend(_owner_array(()))
    return bytes(out), 0


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


class UnitOrderListCensusTest(unittest.TestCase):
    def test_synthetic_census_decodes_every_payload_and_guy(self) -> None:
        data, offset = _synthetic()
        census = parse_unit_orderlist_census(data, offset)
        self.assertEqual((len(census.units), census.active_units), (1, 1))
        self.assertEqual((len(census.builds), sum(row.active for row in census.builds)), (1, 0))
        self.assertEqual(len(census.guys), 1)
        self.assertEqual(
            [order.order_name for order in census.orders],
            ["MOVE_TO", "BUILD_AT", "GATHER", "CAST_SPELL"],
        )
        self.assertEqual([order.metric for order in census.orders], [0xFE, 7, 8, 9])
        self.assertEqual([order.order_flags for order in census.orders], [0xA5, 4, 0, 0])
        self.assertEqual(
            [order.don_save_v13_payload_tag for order in census.orders],
            [1, 0, 2, 3],
        )
        self.assertTrue(
            all(order.canonical_lossless_for_retail_fields for order in census.orders)
        )
        for order in census.orders:
            with self.subTest(order=order.order_name):
                rebuilt = encode_retail_payload(order)
                self.assertEqual(rebuilt.hex(), order.payload_hex)
                self.assertEqual(hashlib.sha256(rebuilt).hexdigest(), order.payload_sha256)
        self.assertEqual(census.orders[0].field("off_y"), 20)
        self.assertEqual(census.orders[1].field("target_o"), 2007)
        self.assertEqual(census.orders[2].field("been_there"), 1)
        self.assertEqual(census.orders[3].field("spell"), 658)

    def test_supplied_boundary_is_used_without_search(self) -> None:
        section, _ = _synthetic()
        decoy = bytes((OBJECTS_TAG, SUBOBJECT_TAG, OBJECT_TAG, UNIT_TAG, BUILD_TAG))
        data = decoy + section
        census = parse_unit_orderlist_census(data, len(decoy))
        self.assertEqual(census.objects_offset, len(decoy))
        with self.assertRaises(UnitOrderListCensusError):
            parse_unit_orderlist_census(data, 0)

    def test_structural_mutations_are_killed(self) -> None:
        original, _ = _synthetic()
        census = parse_unit_orderlist_census(original, 0)
        owner = census.owners[0]
        unit = census.units[0]
        build = census.builds[0]
        first_order = census.orders[0]
        assert unit.path is not None and unit.guys is not None
        mutations = {
            "Objects tag": (0, OBJECTS_TAG ^ 0xFF),
            "object presence": (owner.offset + 11, 2),
            "object history": (owner.body_offset - 6, 3),
            "SubObject tag": (unit.offset, SUBOBJECT_TAG ^ 0xFF),
            "SubObject gate": (unit.offset + 2, 0),
            "Object tag": (unit.offset + 22, OBJECT_TAG ^ 0xFF),
            "Unit tag": (unit.offset + 59, UNIT_TAG ^ 0xFF),
            "path length": (unit.path.offset + 4, 3),
            "order type": (first_order.offset, 27),
            "Guy presence": (unit.guys.offset + 11, 2),
            "Build tag": (build.offset + 10, BUILD_TAG ^ 0xFF),
        }
        for name, (at, value) in mutations.items():
            with self.subTest(name=name):
                damaged = bytearray(original)
                damaged[at] = value
                with self.assertRaises(UnitOrderListCensusError):
                    parse_unit_orderlist_census(damaged, 0)
        with self.assertRaises(UnitOrderListCensusError):
            parse_unit_orderlist_census(original[:-1], 0)

    def test_fresh_svx_exact_census_and_manifests(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]
        save = root / "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX"
        if not save.exists():
            self.skipTest("user-owned fresh SVX is not installed")
        raw = save.read_bytes()
        self.assertEqual(
            hashlib.sha256(raw).hexdigest(),
            "161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7",
        )
        plain = gzip.decompress(raw)
        self.assertEqual(
            hashlib.sha256(plain).hexdigest(),
            "fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8",
        )
        census = parse_unit_orderlist_census(plain, 0x4F21F)
        self.assertEqual((len(census.units), census.active_units, census.inactive_units), (800, 47, 753))
        self.assertEqual((len(census.builds), sum(row.active for row in census.builds)), (800, 37))
        self.assertEqual(len(census.guys), 55)
        self.assertEqual(len(census.orders), 43)
        self.assertEqual(
            Counter(order.order_type for order in census.orders),
            Counter({7: 29, 3: 8, 6: 4, 14: 1, 1: 1}),
        )
        self.assertEqual(
            Counter((order.metric, order.order_flags) for order in census.orders),
            Counter({(0, 0): 30, (0, 1): 9, (0, 4): 4}),
        )
        self.assertTrue(
            all(order.canonical_lossless_for_retail_fields for order in census.orders)
        )
        self.assertEqual(census.terminal_body_offset, 0x6039A)
        self.assertEqual(set(census.terminal_unparsed_types), {3})
        self.assertEqual(
            census.order_manifest_sha256,
            "3bc050ab7e0d49c9529ed879f0828217e664cbb3f778b1eb0c5ac5f4e1ad6dc9",
        )
        self.assertEqual(
            census.unit_manifest_sha256,
            "b6f265645c006ff3432ca71f7ac852df020ec2804054666f24dc4f8df899d580",
        )
        self.assertEqual(
            census.guy_manifest_sha256,
            "1bf44c19f2efa2554809023176e50621ad5b1d3643cd18f8d5cbe52832c6a17f",
        )
        self.assertEqual(
            [(owner.offset, owner.body_offset, owner.body_end) for owner in census.owners],
            [
                (0x4F2AE, 0x504B7, 0x53DFC),
                (0x53DFC, 0x55005, 0x57786),
                (0x57786, 0x5898F, 0x5B38E),
                (0x5B38E, 0x5C597, 0x5F4A1),
                (0x5F4A1, 0x5F4A5, 0x5F4A5),
                (0x5F4A5, 0x5F4A9, 0x5F4A9),
                (0x5F4A9, 0x5F4AD, 0x5F4AD),
                (0x5F4AD, 0x5F4B1, 0x5F4B1),
                (0x5F4B1, 0x6039A, None),
            ],
        )
        for order in census.orders:
            self.assertEqual(encode_retail_payload(order).hex(), order.payload_hex)

    def test_executable_spans_freeze_dynamic_and_payload_routes(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]
        exe = root / "ron-bin/riseofnations.exe"
        if not exe.exists():
            self.skipTest("matched retail executable is not installed")
        image = exe.read_bytes()
        self.assertEqual(
            hashlib.sha256(image).hexdigest(),
            "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079",
        )
        expected = (
            (0x0046DF30, 810, "d0e12370667b5da4429ce4ffaefe4a93953fe681f102bb9108635429beb0c71c"),
            (0x005E0210, 29, "d0bf2b9b17557694d4e27478380cddf616e139eb33647fe3e372ccce7cffb8a2"),
            (0x0062F270, 250, "840554c625f81478f65ad110d4ae5058ab31d156c628f3bcf4bad674f7f1c021"),
            (0x00642510, 131, "fa7b325126bf8047d9cc20adde6d90573a435b3564fbe6882a7e9a503503e994"),
            (0x006305F0, 117, "7b9f998623e87101c64c7eecc9498f05488cccf10c35ab68b6e8f189e719a221"),
            (0x00471C30, 476, "b10b5fc6aa5136b32794e5d6974da0db8fabd127f5eed95df2d877be47297e7e"),
            (0x004708A0, 448, "a587c3883e08a12b6c2fd74d27551e0d025657601d7682611596fb37b70d385b"),
            (0x00470F30, 26, "89488a7fefc4f80be6043554f8a2786823edfed19ff3f58dfa0c2b408cfa3dd6"),
            (0x0047F220, 56, "7ef6d784096fb82a05c82718592939e8784f761e008694f36c6ecce8315d23f8"),
            (0x00486E60, 70, "7b15539744870b94a18fd5da451821461c0268e9b382022250a2409928e5afb4"),
            (0x004860A0, 70, "50b0f95ec4926d8ac96a2bd80c96ed9dda2e5b79d8036fd2a8677726b89bfd50"),
        )
        for va, size, digest in expected:
            with self.subTest(va=f"{va:#x}"):
                offset = _pe_offset(image, va)
                self.assertEqual(
                    hashlib.sha256(image[offset : offset + size]).hexdigest(),
                    digest,
                )

    def test_canonical_v13_authority_constants_match_projection(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]
        economy = (root / "crates/don-sim/src/systems/economy_order_payload_authority.rs").read_text()
        save_load = (root / "crates/don-sim/src/systems/save_load.rs").read_text()
        for exact in (
            "pub const TARGET_ORDER_WALKED_BYTES: usize = 11;",
            "pub const GATHER_ORDER_WALKED_BYTES: usize = 31;",
            "pub const CAST_ORDER_WALKED_BYTES: usize = 27;",
            "pub const ORDER_LIST_NODE_PREFIX_BYTES: usize = 5;",
            "Gather = 2,",
            "CastSpell = 3,",
        ):
            self.assertIn(exact, economy)
        self.assertIn("w.u8(order.node_metric);", save_load)
        self.assertIn("w.u8(o.kind as u8);", save_load)
        self.assertIn("w.u8(o.flags);", save_load)


if __name__ == "__main__":
    unittest.main()
