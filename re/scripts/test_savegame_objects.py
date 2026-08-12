#!/usr/bin/env python3
"""Mutation, truncation, PDB/PE, and fresh-boundary tests for Objects."""

from __future__ import annotations

import gzip
import hashlib
import json
import pathlib
import struct
import sys
import tempfile
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
from savegame_groups_tail import parse_groups_tail_section  # noqa: E402
from savegame_herds import parse_herds_section  # noqa: E402
from savegame_heroes import parse_heroes_section  # noqa: E402
from savegame_items import parse_items_section  # noqa: E402
from savegame_lands import parse_lands_section  # noqa: E402
from savegame_leader_options import parse_leader_options_section  # noqa: E402
from savegame_leaders import parse_leaders_section  # noqa: E402
from savegame_mountains import parse_mountains_section  # noqa: E402
from savegame_objects import (  # noqa: E402
    AMMO_TAG_STRING_TABLE_INDEX,
    DEATH_TAG_STRING_TABLE_INDEX,
    NEXT_TAG_STRING_TABLE_INDEX,
    OBJECTS_TAG_STRING_TABLE_INDEX,
    SPLINE_TAG_STRING_TABLE_INDEX,
    ObjectsParseError,
    parse_objects_section,
)
from savegame_oil_wells import parse_oil_wells_section  # noqa: E402
from savegame_option_info import parse_option_info_section  # noqa: E402
from savegame_pathfinder_groups import parse_pathfinder_groups_section  # noqa: E402
from savegame_specials import parse_specials_section  # noqa: E402
from savegame_supplies import parse_supplies_section  # noqa: E402
from savegame_tileset import parse_tileset_section  # noqa: E402
from savegame_types import parse_types_section  # noqa: E402
from savegame_wonders import parse_wonders_section  # noqa: E402


PREFIX = b"PRE-OBJECTS"
FOLLOWING_OWNER = b"HOTKEY-GROUP-SENTINEL" * 32
EXPECTED_LAYOUT_SHA256 = "ee1e7b37234ffadfc5426c90de61c1dacbd285fabf3312d501f8e9bf2146f886"


def _history(length: int, capacity: int, increment: int, flags: int) -> bytes:
    return struct.pack("<iihB", length, capacity, increment, flags)


def _pod(length: int, capacity: int = 0, increment: int = 0, flags: int = 0, data: bytes = b"") -> bytes:
    if length == 0:
        return struct.pack("<i", 0)
    return _history(length, capacity, increment, flags) + data


def _body_decoder(data: memoryview, offset: int, owner: int, slot: int, type_code: int) -> int:
    del owner, slot, type_code
    if offset >= len(data):
        raise ObjectsParseError("synthetic object body lacks its size")
    end = offset + 1 + data[offset]
    if end > len(data):
        raise ObjectsParseError("synthetic object body is truncated")
    return end


DECODERS = {7: _body_decoder, 8: _body_decoder}


def _section(tag: int = 0, *, child_tag: int = 0) -> bytes:
    direct = b"".join(
        (
            bytes((tag,)),
            struct.pack("<4i", -11, 22, -33, 44),
            struct.pack("<9i", *range(-9, 0)),
            struct.pack("<9i", *range(10, 19)),
            struct.pack("<9i", *range(-29, -20)),
            struct.pack("<9H", *range(0x101, 0x10A)),
        )
    )

    owner0 = b"".join(
        (
            _history(3, 5, -2, 0x02),
            b"\x01\x00\x01",
            struct.pack("<2i", 7, 8),
            struct.pack("<ih", 5, -2),
            b"\x03abc",
            b"\x02DE",
        )
    )
    owner1 = _history(2, 2, 3, 0x01) + b"\x00\x00" + struct.pack("<ih", 2, 3)
    owners = owner0 + owner1 + struct.pack("<7i", *(0,) * 7)

    spline = b"".join(
        (
            bytes((child_tag,)),
            bytes(range(36)),
            _pod(1, 2, 3, 0x01, bytes(range(0x10, 0x1C))),
            _pod(2, 2, 1, 0x02, bytes(range(0x20, 0x28))),
            _pod(0),
            _pod(1, 1, -1, 0x03, bytes(range(0x30, 0x34))),
            _pod(0),
            _pod(1, 4, 2, 0x00, bytes(range(0x40, 0x4C))),
        )
    )
    ammo0 = bytes((child_tag, 0, 0))
    ammo1 = bytes((child_tag, 3)) + bytes(range(99)) + b"\x01" + spline
    ammo = b"".join(
        (
            _history(2, 3, 4, 0x01),
            b"\x01\x01",
            struct.pack("<ih", 3, 4),
            ammo0,
            ammo1,
        )
    )

    death0 = bytes((child_tag,)) + struct.pack("<i", 0)
    death1 = bytes((child_tag,)) + struct.pack("<i", -7) + bytes(range(71))
    deaths = _history(2, 4, 5, 0x02) + death0 + death1
    return direct + owners + ammo + deaths


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


class ObjectsParserTests(unittest.TestCase):
    def test_exact_dynamic_grammar_and_hotkey_boundary(self) -> None:
        data, offset, following = _synthetic()
        parsed = parse_objects_section(data, offset, object_body_decoders=DECODERS)
        self.assertEqual(
            (
                OBJECTS_TAG_STRING_TABLE_INDEX,
                AMMO_TAG_STRING_TABLE_INDEX,
                SPLINE_TAG_STRING_TABLE_INDEX,
                DEATH_TAG_STRING_TABLE_INDEX,
                NEXT_TAG_STRING_TABLE_INDEX,
            ),
            (5066, 126, 6209, 2610, 3963),
        )
        self.assertEqual((parsed.valid, parsed.ammo_index, parsed.good_mark, parsed.rare_mark), (-11, 22, -33, 44))
        self.assertEqual(parsed.unit_mark, tuple(range(-9, 0)))
        self.assertEqual(parsed.build_mark, tuple(range(10, 19)))
        self.assertEqual(parsed.wall_mark, tuple(range(-29, -20)))
        self.assertEqual(parsed.obj_ctr, tuple(range(0x101, 0x10A)))
        self.assertEqual(len(parsed.owners), 9)
        self.assertEqual(parsed.owners[0].presence, (1, 0, 1))
        self.assertEqual(parsed.owners[0].type_codes, (7, 8))
        self.assertEqual((parsed.owners[0].capacity, parsed.owners[0].increment), (5, -2))
        self.assertEqual(
            (parsed.owners[0].repeated_capacity, parsed.owners[0].repeated_increment),
            (5, -2),
        )
        self.assertEqual([body.type_code for body in parsed.owners[0].bodies], [7, 8])
        self.assertEqual(parsed.owners[1].presence, (0, 0))
        self.assertTrue(all(owner.length == 0 for owner in parsed.owners[2:]))
        self.assertEqual(parsed.ammo.presence, (1, 1))
        self.assertEqual([body.flags for body in parsed.ammo.bodies], [0, 3])
        spline = parsed.ammo.bodies[1].path
        self.assertIsNotNone(spline)
        assert spline is not None
        self.assertEqual([array.name.rsplit(".", 1)[-1] for array in spline.arrays], [
            "control_verts", "knots", "spline_knots", "weights", "spline_verts", "spline_normals"
        ])
        self.assertEqual([array.length for array in spline.arrays], [1, 2, 0, 1, 0, 1])
        self.assertEqual([body.valid for body in parsed.deaths.bodies], [0, -7])
        self.assertEqual(len(parsed.deaths.bodies[1].active_data), 71)
        self.assertEqual(parsed.layout_sha256, EXPECTED_LAYOUT_SHA256)
        self.assertEqual(data[parsed.end :], following)
        self.assertEqual(parsed.sha256, hashlib.sha256(data[offset : parsed.end]).hexdigest())

    def test_every_owned_byte_mutation_changes_receipt_or_is_rejected(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_objects_section(data, offset, object_body_decoders=DECODERS)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[offset + relative] ^= 1
                try:
                    changed = parse_objects_section(
                        damaged, offset, object_body_decoders=DECODERS
                    )
                except ObjectsParseError:
                    continue
                self.assertNotEqual(changed.sha256, baseline.sha256)
                self.assertNotEqual(changed, baseline)

    def test_every_truncation_is_rejected(self) -> None:
        data, offset, _ = _synthetic()
        end = parse_objects_section(data, offset, object_body_decoders=DECODERS).end
        for retained in range(end - offset):
            with self.subTest(retained=retained):
                with self.assertRaises(ObjectsParseError):
                    parse_objects_section(
                        data[: offset + retained], offset, object_body_decoders=DECODERS
                    )

    def test_next_owner_mutation_is_excluded(self) -> None:
        data, offset, _ = _synthetic()
        baseline = parse_objects_section(data, offset, object_body_decoders=DECODERS)
        damaged = bytearray(data)
        damaged[baseline.end] ^= 0xFF
        self.assertEqual(
            parse_objects_section(damaged, offset, object_body_decoders=DECODERS),
            baseline,
        )

    def test_tags_histories_and_virtual_bodies_fail_closed(self) -> None:
        with self.assertRaises(ObjectsParseError):
            parse_objects_section(_section(tag=1), 0, object_body_decoders=DECODERS)
        with self.assertRaises(ObjectsParseError):
            parse_objects_section(_section(child_tag=1), 0, object_body_decoders=DECODERS)
        permissive = parse_objects_section(
            _section(tag=0xE7, child_tag=0xA5),
            0,
            object_body_decoders=DECODERS,
            require_tag=False,
            require_child_tags=False,
        )
        self.assertEqual(permissive.tag, 0xE7)
        self.assertEqual(permissive.ammo.bodies[0].tag, 0xA5)
        with self.assertRaisesRegex(ObjectsParseError, "virtual-body decoder"):
            parse_objects_section(_section(), 0)

        damaged = bytearray(_section())
        base = parse_objects_section(damaged, 0, object_body_decoders=DECODERS)
        repeat = base.owners[0].repeated_capacity_offset
        assert repeat is not None
        damaged[repeat] ^= 1
        with self.assertRaisesRegex(ObjectsParseError, "repeated history"):
            parse_objects_section(damaged, 0, object_body_decoders=DECODERS)

    def test_pdb_layout_mutation_and_executable_bodies_are_frozen(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]
        schema = root / "schema/pdb-types.json"
        pdb = root / "ron-bin/sbl/rise.pdb"
        exe = root / "ron-bin/riseofnations.exe"
        if not schema.exists():
            self.skipTest(f"matched schema is not installed: {schema}")

        classes = json.loads(schema.read_text())["classes"]
        names = (
            "Objects", "ObjectsData", "ObjectsOut", "ObjectsArray",
            "MultiPtrArray<Object>", "PtrArray<Ammo>", "ObjectArray<DeathObj>",
            "AmmoData", "DeathObjData", "SplineData", "Vert3Array", "SimpleArray<float>",
        )
        subset = {name: classes[name] for name in names}
        subset["Objects"] = dict(subset["Objects"])
        subset["Objects"]["size"] = 823
        with tempfile.TemporaryDirectory() as directory:
            bad = pathlib.Path(directory) / "pdb-types.json"
            bad.write_text(json.dumps({"classes": subset}))
            with self.assertRaisesRegex(ObjectsParseError, "Objects layout disagrees"):
                parse_objects_section(
                    _section(), 0, object_body_decoders=DECODERS, schema_path=bad
                )

        for path in (pdb, exe):
            if not path.exists():
                self.skipTest(f"matched artifact is not installed: {path}")
        self.assertEqual(hashlib.sha256(schema.read_bytes()).hexdigest(), "399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14")
        self.assertEqual(hashlib.sha256(pdb.read_bytes()).hexdigest(), "334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5")
        image = exe.read_bytes()
        self.assertEqual(hashlib.sha256(image).hexdigest(), "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079")
        spans = (
            (0x006541E0, 469, "f5c338b7e518b05f9c192545773cdf092d9d11764bdb26c5e00dec01710ff8e8"),
            (0x0045D550, 907, "675810f0a1ef4dbb3d7e44c50784dcf4df60699b19a19c05c07fd86eaf790672"),
            (0x00473FE0, 797, "867dc282037a33019d8720523fd4105b3a497fe6e58d27058c1839e5b4e3b427"),
            (0x00474420, 487, "a2fd2b681c6c63b432cc46460d9bbd9a8e85506ae3f6fbd221e722550f5c8034"),
            (0x0067AB50, 185, "61fb8765e59c3c832eeb6a074404b02cb86b4148045d5085a893bc045ecbf051"),
            (0x009132B0, 127, "f5eb1132763a792b60a0d5f8e32043d814bcd308afd44faf1f7b24ba39695d35"),
            (0x004A46D0, 488, "69a9c55e0a532f52a0d40a4df7a2f4e93241fefe83426f9a08fba833c06cf790"),
            (0x00490B10, 464, "a75d884c0c4fbb8062305ef785364a612b6c4bf6538f9b49e0d7693b6d944394"),
            (0x005A2C77, 12, "81f2b01ca630a3059694ca40b0c99796432d60a42593a274d3e54dd4d75290a7"),
            (0x005A2D7E, 38, "866a70dec0b71558811d5befe48d4a276db5bec7eb898129595a72fb68447164"),
            (0x00480290, 526, "4e1dfe9d96758c1d299ba00db75095612fcd87fe3bb6f462a83e96eb16b027a7"),
        )
        for va, size, digest in spans:
            with self.subTest(va=f"{va:#x}"):
                raw = _pe_offset(image, va)
                self.assertEqual(hashlib.sha256(image[raw : raw + size]).hexdigest(), digest)

    def test_fresh_svx_chains_to_exact_hotkey_boundary_without_replay_join(self) -> None:
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
        leader_options = parse_leader_options_section(save_plain, lands.end)
        option_info = parse_option_info_section(save_plain, leader_options.end)
        pathfinder_groups = parse_pathfinder_groups_section(save_plain, option_info.end)
        groups = parse_groups_tail_section(save_plain, pathfinder_groups.end)
        self.assertEqual(groups.end, 0x27E93)
        parsed = parse_objects_section(save_plain, groups.end)
        self.assertEqual((parsed.offset, parsed.end, parsed.size), (0x27E93, 0x27F4E, 187))
        self.assertEqual(parsed.tag, 0)
        self.assertEqual(
            (parsed.valid, parsed.ammo_index, parsed.good_mark, parsed.rare_mark),
            (0, 0, 0, 0),
        )
        self.assertEqual(parsed.unit_mark + parsed.build_mark + parsed.wall_mark, (0,) * 27)
        self.assertEqual(parsed.obj_ctr, (0,) * 9)
        self.assertTrue(all(owner.length == 0 for owner in parsed.owners))
        self.assertEqual((parsed.ammo.length, parsed.deaths.length), (0, 0))
        self.assertEqual(parsed.sha256, "9708fd0c3a64591c024c02167f3b7cad1dcda62d1d41b6d0a27b96989b63864f")
        changed = bytearray(save_plain)
        changed[parsed.end] ^= 1
        self.assertEqual(parse_objects_section(changed, groups.end), parsed)

        save_tree = savegame_parse.parse(save_plain)
        replay_tree = savegame_parse.parse(gzip.decompress(replay_raw))
        save_seed = _field(_find(save_tree, "GameInfo::walk_data"), "seed")
        replay_seed = _field(_find(replay_tree, "GameInfo::walk_data"), "seed")
        self.assertEqual((save_seed, replay_seed), ("0x014810ac", "0x007f93e0"))
        self.assertNotEqual(save_seed, replay_seed)


if __name__ == "__main__":
    unittest.main()
