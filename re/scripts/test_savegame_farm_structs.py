#!/usr/bin/env python3
"""Dynamic grammar, mutation, truncation, PE/PDB, and SVX Farms gates."""

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
from savegame_farm_structs import (  # noqa: E402
    FARM_STRUCT_LOGICAL_SIZE,
    FARM_STRUCT_MEMORY_SIZE,
    TAG_STRING_TABLE_INDEX,
    FarmStructsParseError,
    parse_farm_structs_section,
)
from savegame_game_daemon import parse_game_daemon_block  # noqa: E402
from savegame_game_random import parse_game_random_block  # noqa: E402
from savegame_graphic_events import FRESH_RETAIL_SLOT_COUNT, parse_graphic_events_section  # noqa: E402
from savegame_world import parse_world_section  # noqa: E402

try:  # The immediate predecessor may still be awaiting its isolated landing.
    from savegame_scene import parse_scene_section  # type: ignore  # noqa: E402
except ModuleNotFoundError:  # pragma: no cover - explicit integration-stage skip
    parse_scene_section = None


PREFIX = b"SCENE-END"
FOLLOWING_OWNER = b"UNBUILT-WONDERS-SENTINEL" * 8
LAYOUT_SHA = "6880c7d5db8782867943ace21c178e71fa2741062491a31618e900f7fd3645db"


def _row(index: int) -> bytes:
    who, object_id = (-100 - index, 9000 + index)
    percent = tuple((0x7FC00000 + index * 0x100 + i) & 0xFFFFFFFF for i in range(16))
    terrain = tuple((0x3F000000 + index * 0x100 + i) & 0xFFFFFFFF for i in range(25))
    status = bytes((0x40 + index * 0x10 + i) & 0xFF for i in range(16))
    return (
        struct.pack("<ii", who, object_id)
        + struct.pack("<16I", *percent)
        + struct.pack("<25I", *terrain)
        + status
        + bytes((index & 1, 7 + index))
    )


def _section(tag: int = 4) -> tuple[bytes, int, bytes]:
    color = bytes.fromhex("102030405060708090a0")
    wheat = struct.pack("<II", 0x7FC01234, 0xBF800000)
    array = struct.pack("<iihB", 2, 5, -3, 2) + _row(0) + _row(1)
    payload = bytes((tag,)) + color + wheat + array
    return PREFIX + payload + FOLLOWING_OWNER, len(PREFIX), FOLLOWING_OWNER


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
    count = struct.unpack_from("<H", image, pe + 6)[0]
    optional_size = struct.unpack_from("<H", image, pe + 20)[0]
    optional = pe + 24
    base = struct.unpack_from("<I", image, optional + 28)[0]
    rva, table = va - base, optional + optional_size
    for index in range(count):
        section = table + index * 40
        virtual_size, virtual_address, raw_size, raw = struct.unpack_from("<IIII", image, section + 8)
        if virtual_address <= rva < virtual_address + max(virtual_size, raw_size):
            return raw + rva - virtual_address
    raise AssertionError(f"VA {va:#x} outside image")


class FarmStructsParserTests(unittest.TestCase):
    def test_complete_dynamic_rows_and_unbuilt_wonders_boundary(self) -> None:
        data, offset, following = _section()
        parsed = parse_farm_structs_section(data, offset)
        self.assertEqual(TAG_STRING_TABLE_INDEX, 2672)
        self.assertEqual((FARM_STRUCT_LOGICAL_SIZE, FARM_STRUCT_MEMORY_SIZE), (190, 192))
        self.assertEqual(parsed.start_color.raw, bytes.fromhex("102030405060708090a0"))
        self.assertEqual(
            (parsed.start_color.red, parsed.start_color.green, parsed.start_color.blue, parsed.start_color.alpha),
            (0x10, 0x20, 0x30, 0x40),
        )
        self.assertEqual((parsed.start_color.rgb, parsed.start_color.w_555, parsed.start_color.w_565), (0x40302010, 0x6050, 0x8070))
        self.assertEqual((parsed.start_color.flags, parsed.start_color.index), (0x90, 0xA0))
        self.assertEqual((parsed.wheat_max_height_bits, parsed.wheat_min_height_bits), (0x7FC01234, 0xBF800000))
        self.assertEqual((parsed.farm_data.length, parsed.farm_data.capacity, parsed.farm_data.increment, parsed.farm_data.flags), (2, 5, -3, 2))
        self.assertEqual([row.size for row in parsed.farm_data.rows], [190, 190])
        self.assertEqual([(row.who, row.object_id, row.valid, row.farm_type) for row in parsed.farm_data.rows], [(-100, 9000, 0, 7), (-101, 9001, 1, 8)])
        self.assertEqual(parsed.farm_data.rows[0].percent_bits[0], 0x7FC00000)
        self.assertEqual(parsed.farm_data.rows[1].terrain_height_bits[-1], 0x3F000118)
        self.assertEqual(parsed.layout_sha256, LAYOUT_SHA)
        self.assertEqual(parsed.sha256, hashlib.sha256(data[offset:parsed.end]).hexdigest())
        self.assertEqual(data[parsed.end:], following)

    def test_every_owned_byte_mutation_changes_receipt_or_is_rejected(self) -> None:
        data, offset, _ = _section()
        baseline = parse_farm_structs_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[offset + relative] ^= 1
                try:
                    parsed = parse_farm_structs_section(damaged, offset)
                except FarmStructsParseError:
                    continue
                self.assertNotEqual(parsed, baseline)
                self.assertNotEqual(parsed.sha256, baseline.sha256)

    def test_every_truncation_and_array_history_fail_closed(self) -> None:
        data, offset, _ = _section()
        baseline = parse_farm_structs_section(data, offset)
        for retained in range(baseline.size):
            with self.subTest(retained=retained):
                with self.assertRaises(FarmStructsParseError):
                    parse_farm_structs_section(data[:offset + retained], offset)
        array_offset = baseline.farm_data.offset
        damaged = bytearray(data)
        struct.pack_into("<i", damaged, array_offset + 4, 1)
        with self.assertRaisesRegex(FarmStructsParseError, "history"):
            parse_farm_structs_section(damaged, offset)
        damaged = bytearray(data)
        damaged[array_offset + 10] |= 0x40
        with self.assertRaisesRegex(FarmStructsParseError, "history"):
            parse_farm_structs_section(damaged, offset)
        damaged = bytearray(data)
        struct.pack_into("<i", damaged, array_offset, -1)
        with self.assertRaisesRegex(FarmStructsParseError, "length"):
            parse_farm_structs_section(damaged, offset)

    def test_tag_two_byte_padding_and_next_owner_are_strictly_excluded(self) -> None:
        data, offset, _ = _section()
        baseline = parse_farm_structs_section(data, offset)
        damaged = bytearray(data)
        damaged[baseline.end] ^= 0xFF
        self.assertEqual(parse_farm_structs_section(damaged, offset), baseline)
        damaged = bytearray(data)
        damaged[offset] = 3
        with self.assertRaisesRegex(FarmStructsParseError, "tag"):
            parse_farm_structs_section(damaged, offset)
        self.assertEqual(parse_farm_structs_section(damaged, offset, require_tag=False).tag, 3)
        self.assertEqual(baseline.farm_data.rows[1].end, baseline.end)
        self.assertEqual(baseline.farm_data.rows[1].size, FARM_STRUCT_MEMORY_SIZE - 2)

    def test_pdb_mutation_and_exact_executable_bodies_are_frozen(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]
        schema = root / "schema/pdb-types.json"
        pdb = root / "ron-bin/sbl/rise.pdb"
        exe = root / "ron-bin/riseofnations.exe"
        if not schema.exists():
            self.skipTest("matched PDB schema unavailable")
        classes = json.loads(schema.read_text())["classes"]
        names = ("Farms", "Color", "FarmStruct", "Array<FarmStruct>")
        subset = {name: classes[name] for name in names}
        subset["FarmStruct"] = dict(subset["FarmStruct"])
        subset["FarmStruct"]["size"] = 190
        with tempfile.TemporaryDirectory() as directory:
            bad = pathlib.Path(directory) / "pdb-types.json"
            bad.write_text(json.dumps({"classes": subset}))
            with self.assertRaisesRegex(FarmStructsParseError, "FarmStruct size disagrees"):
                parse_farm_structs_section(bytes(23), 0, schema_path=bad)
        if not pdb.exists() or not exe.exists():
            self.skipTest("matched PE/PDB unavailable")
        self.assertEqual(hashlib.sha256(schema.read_bytes()).hexdigest(), "399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14")
        self.assertEqual(hashlib.sha256(pdb.read_bytes()).hexdigest(), "334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5")
        image = exe.read_bytes()
        self.assertEqual(hashlib.sha256(image).hexdigest(), "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079")
        spans = (
            (0x004A8DB0, 502, "e6c75197c554488767f5ffc39f25d0180ab15594abdda12504ca65f1c3a6d7b7"),
            (0x0073C290, 550, "2f39ca65083926b449a36b5b40f06d086ec707e9b9e9436b06e18e0129d50234"),
            (0x005A2E20, 81, "635f31c3efb02c178fbdf8da27fe1303d9b3658e3cc35540a293631dea56e8fc"),
        )
        for va, size, digest in spans:
            raw = _pe_offset(image, va)
            self.assertEqual(hashlib.sha256(image[raw:raw + size]).hexdigest(), digest)

    def test_fresh_chain_reaches_exact_unbuilt_wonders_and_rcx_stays_independent(self) -> None:
        if parse_scene_section is None:
            self.skipTest("immediate predecessor Scene helper not yet landed")
        root = pathlib.Path(__file__).resolve().parents[2]
        save = root / "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX"
        replay = root / "ron-data/replays/Playback - 2026.08.11 11'44'38 (Tue).rcx"
        if not save.exists() or not replay.exists():
            self.skipTest("user SVX/RCX unavailable")
        save_raw, replay_raw = save.read_bytes(), replay.read_bytes()
        self.assertEqual(hashlib.sha256(save_raw).hexdigest(), "161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7")
        self.assertEqual(hashlib.sha256(replay_raw).hexdigest(), "558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54")
        plain = gzip.decompress(save_raw)
        self.assertEqual(hashlib.sha256(plain).hexdigest(), "fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8")
        world = parse_world_section(plain, 0x27F53)
        daemon = parse_game_daemon_block(plain, world.end)
        random = parse_game_random_block(plain, daemon.end)
        graphics = parse_graphic_events_section(plain, random.end, FRESH_RETAIL_SLOT_COUNT)
        scene = parse_scene_section(plain, graphics.end)
        parsed = parse_farm_structs_section(plain, scene.end)
        self.assertEqual((parsed.offset, parsed.end, parsed.size), (0x2BC76, 0x2BC8D, 23))
        self.assertEqual(parsed.start_color.raw, bytes.fromhex("00000004007800010004"))
        self.assertEqual((parsed.wheat_max_height_bits, parsed.wheat_min_height_bits), (0, 0))
        self.assertEqual(parsed.farm_data.length, 0)
        self.assertEqual(parsed.sha256, "dfc517291c32174169a0463154005f2924a9567b493f3af5e4672629b7ecedfb")
        self.assertEqual(hashlib.sha256(plain[parsed.end:parsed.end + 4]).hexdigest(), "df3f619804a92fdb4057192dc43dd748ea778adc52bc498ce80524c014b81119")
        damaged = bytearray(plain)
        damaged[parsed.end] ^= 1
        self.assertEqual(parse_farm_structs_section(damaged, scene.end), parsed)
        save_tree = savegame_parse.parse(plain)
        replay_tree = savegame_parse.parse(gzip.decompress(replay_raw))
        self.assertEqual(
            (_field(_find(save_tree, "GameInfo::walk_data"), "seed"), _field(_find(replay_tree, "GameInfo::walk_data"), "seed")),
            ("0x014810ac", "0x007f93e0"),
        )


if __name__ == "__main__":
    unittest.main()
