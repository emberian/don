#!/usr/bin/env python3
"""Mutation and retail-artifact tests for savegame_unit_orderlists.py."""

from __future__ import annotations

import gzip
import hashlib
import json
import pathlib
import struct
import sys
import unittest


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from savegame_unit_orderlists import (  # noqa: E402
    OBJECTS_FIXED_BYTES,
    OBJECTS_TAG,
    OBJECT_TAG,
    SUBOBJECT_TAG,
    UNIT_FIXED_BYTES,
    UNIT_TAG,
    UnitOrderListParseError,
    parse_first_unit_orderlist,
)


MOVE_WORDS = (
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
)


def _synthetic() -> tuple[bytes, int, bytes]:
    out = bytearray()
    out.append(OBJECTS_TAG)
    out.extend(bytes((index * 13) & 0xFF for index in range(OBJECTS_FIXED_BYTES)))

    # First owner MultiPtrArray<Object>: one live Unit in slot zero.
    out.extend(struct.pack("<iihB", 4, 4, -1, 0))
    out.extend((1, 0, 0, 0))
    out.extend(struct.pack("<i", 0))
    out.extend(struct.pack("<ih", 4, -1))

    out.extend((SUBOBJECT_TAG, 0x41, 1, 2))
    out.extend(struct.pack("<hiii", 17, 300, 400, 500))
    out.extend(struct.pack("<i", 69))
    out.extend((OBJECT_TAG, 1))
    out.extend(bytes(range(34)))
    out.append(0)  # Object::launching absent
    out.extend((UNIT_TAG, 1))
    out.extend(bytes((index * 7) & 0xFF for index in range(UNIT_FIXED_BYTES)))

    out.extend(struct.pack("<iib", 2, 1, -1))
    out.extend(struct.pack("<iiii", 1000, 2000, 48, 1))

    out.extend(struct.pack("<i", 1))
    out.extend(struct.pack("<iB", 3, 0xFE))
    out.append(0xA5)  # UnitOrder::flags; not a concrete payload tag.
    out.extend(struct.pack("<18i2h", *MOVE_WORDS, -19, 20))
    following_owner = b"GUYS-SENTINEL"
    return bytes(out) + following_owner, 0, following_owner


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


class UnitOrderListParserTest(unittest.TestCase):
    def test_structure_reaches_move_payload_and_preserves_metric(self) -> None:
        data, offset, following_owner = _synthetic()
        parsed = parse_first_unit_orderlist(data, offset)
        self.assertEqual((parsed.who, parsed.o, parsed.ptype_index), (2, 17, 69))
        self.assertEqual((parsed.path_capacity, parsed.path_length), (2, 1))
        self.assertEqual(parsed.path_increment, -1)
        self.assertEqual(len(parsed.nodes), 1)
        node = parsed.nodes[0]
        self.assertEqual((node.order_type, node.order_name), (3, "EXPLORE_TO"))
        self.assertEqual(node.metric_offset, node.offset + 4)
        self.assertEqual(node.metric, 0xFE)
        self.assertEqual(node.order_flags, 0xA5)
        self.assertEqual(
            dataclass_payload_words(node.payload),
            MOVE_WORDS + (-19, 20),
        )
        self.assertEqual(
            data[parsed.next_owner_offset :],
            following_owner,
            "PtrArray<Guy> owner was consumed",
        )
        self.assertEqual(
            parsed.sha256,
            hashlib.sha256(
                data[parsed.orderlist_offset : parsed.orderlist_end]
            ).hexdigest(),
        )

    def test_supplied_boundary_is_used_without_tag_search(self) -> None:
        section, _, following_owner = _synthetic()
        decoy = bytes((OBJECTS_TAG, SUBOBJECT_TAG, OBJECT_TAG, UNIT_TAG, 0xA5))
        data = decoy + section
        parsed = parse_first_unit_orderlist(data, len(decoy))
        self.assertEqual(parsed.objects_offset, len(decoy))
        self.assertEqual(data[parsed.next_owner_offset :], following_owner)
        with self.assertRaises(UnitOrderListParseError):
            parse_first_unit_orderlist(data, 0)

    def test_structural_mutations_are_killed(self) -> None:
        original, _, _ = _synthetic()
        parsed = parse_first_unit_orderlist(original, 0)
        array = parsed.owner_array_offset
        unit = parsed.unit_body_offset
        node = parsed.nodes[0]
        mutations = {
            "objects tag": (0, OBJECTS_TAG ^ 0xFF),
            "presence is not boolean": (array + 11, 2),
            "duplicated capacity": (array + 19, 5),
            "SubObject tag": (unit, SUBOBJECT_TAG ^ 0xFF),
            "SubObject gate": (unit + 2, 0),
            "Object tag": (unit + 22, OBJECT_TAG ^ 0xFF),
            "launching presence": (unit + 58, 1),
            "Unit tag": (unit + 59, UNIT_TAG ^ 0xFF),
            "Unit gate": (unit + 60, 0),
            "path length": (parsed.path_offset + 4, 3),
            "order type": (node.offset, 27),
        }
        for name, (offset, value) in mutations.items():
            with self.subTest(name=name):
                damaged = bytearray(original)
                damaged[offset] = value
                with self.assertRaises(UnitOrderListParseError):
                    parse_first_unit_orderlist(damaged, 0)

        with self.assertRaises(UnitOrderListParseError):
            parse_first_unit_orderlist(original[: parsed.orderlist_end - 1], 0)

    def test_executable_spans_freeze_the_structure_recipe(self) -> None:
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
            (0x006541E0, 469, "f5c338b7e518b05f9c192545773cdf092d9d11764bdb26c5e00dec01710ff8e8"),
            (0x0045D550, 907, "675810f0a1ef4dbb3d7e44c50784dcf4df60699b19a19c05c07fd86eaf790672"),
            (0x006621D0, 216, "21da7fd9c4c463d308a7a8ec90cd867befb03d21920dd953e3c24cc8a74f51a1"),
            (0x00647830, 256, "49e892d0bf7bbe455339c55ba20bf6ed2be162e0958bdaffda644ac3526bb152"),
            (0x0060CF40, 248, "a4b6e96d3c349dcd2b65f5256c1373ac8e16ddd544cf443d7f255f76ce0a12c7"),
            (0x0046D8B0, 239, "457efeb27bdd357c9e0b4d1f45168a220617944683a0d62b0782c1aaa306c243"),
            (0x00730270, 391, "34db4525635f25073be363792643145f6ece881b52bc3ade20ee6a669c176d2d"),
            (0x00482E70, 56, "3ba6e2fbcb0af227d2e63b4fac56739160e87cf8d1bcb7115d0fbd566e22a45b"),
        )
        for va, size, digest in expected:
            with self.subTest(va=f"{va:#x}"):
                offset = _pe_offset(image, va)
                self.assertEqual(
                    hashlib.sha256(image[offset : offset + size]).hexdigest(),
                    digest,
                )

    def test_pdb_fields_pin_metric_flags_and_move_payload(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]
        types = json.loads((root / "schema/pdb-types.json").read_text())["classes"]
        recycled = types["RecycledOrderNode"]
        unit_order = types["UnitOrder"]
        move_order = types["MoveOrder"]
        self.assertEqual(
            [(field["name"], field["offset"], field["size"]) for field in recycled["fields"]],
            [("next", 0, 4), ("prev", 4, 4), ("data", 8, 4), ("metric", 12, 1)],
        )
        self.assertEqual(
            [(field["name"], field["offset"], field["size"]) for field in unit_order["fields"]],
            [("flags", 4, 1)],
        )
        expected_move_fields = (
            "x",
            "y",
            "angle",
            "dest",
            "tolerance",
            "pause",
            "retry",
            "attempts",
            "timer",
            "facing",
            "dest_x",
            "dest_y",
            "last_x",
            "last_y",
            "coll_x",
            "coll_y",
            "orig_x",
            "orig_y",
            "off_x",
            "off_y",
        )
        self.assertEqual(
            tuple(field["name"] for field in move_order["fields"]),
            expected_move_fields,
        )
        self.assertEqual(
            [(field["offset"], field["size"]) for field in move_order["fields"]],
            [(offset, 4) for offset in range(4, 76, 4)] + [(76, 2), (78, 2)],
        )
        self.assertEqual(
            move_order["virtual_bases"][0],
            {
                "direct": True,
                "name": "UnitOrder",
                "size": 8,
                "vbptr_offset": 0,
                "vbtable_index": 1,
                "derived_offset": 84,
                "derived_offset_note": move_order["virtual_bases"][0]["derived_offset_note"],
            },
        )

    def test_fresh_svx_localizes_a_positive_explore_order(self) -> None:
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
        parsed = parse_first_unit_orderlist(plain, 0x4F21F)
        self.assertEqual(parsed.owner_array_offset, 0x4F2AE)
        self.assertEqual(parsed.owner_array_body_offset, 0x504B7)
        self.assertEqual(parsed.unit_body_offset, 0x504B7)
        self.assertEqual((parsed.unit_slot, parsed.who, parsed.o), (0, 0, 0))
        self.assertEqual(parsed.ptype_index, 69)
        self.assertEqual((parsed.path_offset, parsed.path_end), (0x50563, 0x5058C))
        self.assertEqual(
            (parsed.path_capacity, parsed.path_length, parsed.path_increment),
            (30, 2, 10),
        )
        self.assertEqual((parsed.orderlist_offset, parsed.orderlist_end), (0x5058C, 0x505E2))
        self.assertEqual(parsed.size, 86)
        self.assertEqual(
            parsed.sha256,
            "2cea2ffb1e52ff2077982ee3004fcd02c74fda6bf512c3bf5e5b78c8c31fbfc7",
        )
        self.assertEqual(len(parsed.nodes), 1)
        node = parsed.nodes[0]
        self.assertEqual((node.offset, node.end), (0x50590, 0x505E2))
        self.assertEqual((node.order_type, node.order_name, node.metric), (3, "EXPLORE_TO", 0))
        self.assertEqual(node.metric_offset, 0x50594)
        self.assertEqual((node.order_flags_offset, node.order_flags), (0x50595, 1))
        self.assertEqual((node.payload.offset, node.payload.end), (0x50596, 0x505E2))
        self.assertEqual(
            node.payload.sha256,
            "ed0f47e809ea729dbc48679abaaad1df5b96d922898e43a7d3292c1ee4860c5f",
        )
        self.assertEqual(
            dataclass_payload_words(node.payload),
            (
                48888,
                30456,
                -1550974976,
                1,
                0,
                0,
                0,
                0,
                0,
                1,
                50424,
                29688,
                -1,
                -1,
                0,
                0,
                48864,
                30432,
                504,
                504,
            ),
        )
        self.assertEqual(struct.unpack_from("<i", plain, parsed.next_owner_offset)[0], 2)


def dataclass_payload_words(payload: object) -> tuple[int, ...]:
    return (
        payload.x,
        payload.y,
        payload.angle,
        payload.dest,
        payload.tolerance,
        payload.pause,
        payload.retry,
        payload.attempts,
        payload.timer,
        payload.facing,
        payload.dest_x,
        payload.dest_y,
        payload.last_x,
        payload.last_y,
        payload.coll_x,
        payload.coll_y,
        payload.orig_x,
        payload.orig_y,
        payload.off_x,
        payload.off_y,
    )


if __name__ == "__main__":
    unittest.main()
