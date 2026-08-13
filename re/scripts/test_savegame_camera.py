#!/usr/bin/env python3
"""Direct-image, mutation, truncation, PE/PDB, and SVX Camera gates."""

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
from savegame_camera import (  # noqa: E402
    BASE_CAMERA_TAIL_OFFSET,
    BASE_CAMERA_TAIL_SIZE,
    CAMERA_SECTION_SIZE,
    CAMERA_TAIL_OFFSET,
    CAMERA_TAIL_SIZE,
    NEXT_TAG_STRING_TABLE_INDEX,
    TAG_STRING_TABLE_INDEX,
    CameraParseError,
    parse_camera_section,
)

try:
    from savegame_conquest_game import parse_conquest_game_section  # noqa: E402
    from savegame_detail_threshold import parse_detail_threshold_block  # noqa: E402
    from savegame_farm_structs import parse_farm_structs_section  # noqa: E402
    from savegame_game_daemon import parse_game_daemon_block  # noqa: E402
    from savegame_game_random import parse_game_random_block  # noqa: E402
    from savegame_graphic_events import FRESH_RETAIL_SLOT_COUNT, parse_graphic_events_section  # noqa: E402
    from savegame_scene import parse_scene_section  # noqa: E402
    from savegame_unbuilt_cities import parse_unbuilt_cities_section  # noqa: E402
    from savegame_unbuilt_forts import parse_unbuilt_forts_section  # noqa: E402
    from savegame_unbuilt_wonders import parse_unbuilt_wonders_section  # noqa: E402
    from savegame_world import parse_world_section  # noqa: E402
except ImportError:
    parse_detail_threshold_block = None


PREFIX = b"DETAIL-THRESHOLD-END"
FOLLOWING_SELECT_GROUPS = b"SELECT-GROUPS-OWNER" * 64
LAYOUT_SHA = "a709d3a452dc8e08a3e539189e2ea36de5b594cf88dbe34941044f0ae40fa11d"


def _fixture(tag: int = 0) -> tuple[bytes, int, bytes]:
    camera_words = tuple((0x41000000 + index * 0x010203) & 0xFFFFFFFF for index in range(CAMERA_TAIL_SIZE // 4))
    base_words = tuple((0x7FC00001 + index * 0x01010101) & 0xFFFFFFFF for index in range(BASE_CAMERA_TAIL_SIZE // 4))
    payload = bytes((tag,)) + struct.pack(f"<{len(camera_words)}I", *camera_words) + struct.pack(f"<{len(base_words)}I", *base_words)
    return PREFIX + payload + FOLLOWING_SELECT_GROUPS, len(PREFIX), FOLLOWING_SELECT_GROUPS


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


class CameraParserTests(unittest.TestCase):
    def test_exact_two_direct_ranges_and_select_groups_boundary(self) -> None:
        data, offset, following = _fixture()
        parsed = parse_camera_section(data, offset)
        self.assertEqual((TAG_STRING_TABLE_INDEX, NEXT_TAG_STRING_TABLE_INDEX), (413, 6009))
        self.assertEqual((CAMERA_TAIL_OFFSET, CAMERA_TAIL_SIZE), (0x28C, 228))
        self.assertEqual((BASE_CAMERA_TAIL_OFFSET, BASE_CAMERA_TAIL_SIZE), (0xB4, 468))
        self.assertEqual((parsed.offset, parsed.end, parsed.size), (offset, offset + CAMERA_SECTION_SIZE, 697))
        self.assertEqual((len(parsed.camera_words), len(parsed.base_camera_words)), (57, 117))
        self.assertEqual(parsed.camera_words[:2], (0x41000000, 0x41010203))
        self.assertEqual(parsed.base_camera_words[:2], (0x7FC00001, 0x80C10102))
        self.assertEqual(parsed.layout_sha256, LAYOUT_SHA)
        self.assertEqual(parsed.sha256, hashlib.sha256(data[offset:parsed.end]).hexdigest())
        self.assertEqual(data[parsed.end:], following)

    def test_every_owned_byte_mutation_changes_receipt(self) -> None:
        data, offset, _ = _fixture()
        baseline = parse_camera_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[offset + relative] ^= 1
                try:
                    parsed = parse_camera_section(damaged, offset)
                except CameraParseError:
                    continue
                self.assertNotEqual(parsed, baseline)
                self.assertNotEqual(parsed.sha256, baseline.sha256)

    def test_every_truncation_and_invalid_offset_fail_closed(self) -> None:
        data, offset, _ = _fixture()
        baseline = parse_camera_section(data, offset)
        for retained in range(baseline.size):
            with self.subTest(retained=retained):
                with self.assertRaises(CameraParseError):
                    parse_camera_section(data[:offset + retained], offset)
        with self.assertRaises(CameraParseError):
            parse_camera_section(data, -1)

    def test_tag_and_next_owner_gates(self) -> None:
        data, offset, _ = _fixture()
        baseline = parse_camera_section(data, offset)
        damaged = bytearray(data)
        damaged[offset] = 1
        with self.assertRaisesRegex(CameraParseError, "Camera tag"):
            parse_camera_section(damaged, offset)
        self.assertEqual(parse_camera_section(damaged, offset, require_tag=False).tag, 1)
        for relative in range(len(data) - baseline.end):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[baseline.end + relative] ^= 0xFF
                self.assertEqual(parse_camera_section(damaged, offset), baseline)

    def test_pdb_mutation_and_exact_executable_bodies_are_frozen(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]
        schema, pdb, exe = root / "schema/pdb-types.json", root / "ron-bin/sbl/rise.pdb", root / "ron-bin/riseofnations.exe"
        if not schema.exists():
            self.skipTest("matched PDB schema unavailable")
        classes = json.loads(schema.read_text())["classes"]
        names = ("Camera", "BaseCamera", "HierObj", "GameAccessConst")
        subset = {name: classes[name] for name in names}
        subset["Camera"] = dict(subset["Camera"])
        subset["Camera"]["size"] = 876
        with tempfile.TemporaryDirectory() as directory:
            bad = pathlib.Path(directory) / "pdb-types.json"
            bad.write_text(json.dumps({"classes": subset}))
            with self.assertRaisesRegex(CameraParseError, "Camera size disagrees"):
                parse_camera_section(bytes(CAMERA_SECTION_SIZE), 0, schema_path=bad)
        if not pdb.exists() or not exe.exists():
            self.skipTest("matched PE/PDB unavailable")
        self.assertEqual(hashlib.sha256(schema.read_bytes()).hexdigest(), "399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14")
        self.assertEqual(hashlib.sha256(pdb.read_bytes()).hexdigest(), "334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5")
        image = exe.read_bytes()
        self.assertEqual(hashlib.sha256(image).hexdigest(), "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079")
        spans = (
            (0x00844090, 84, "12c6751e3794eaf2300c5714da80b62e931694ac7d95c9b591281aeb3437ea69"),
            (0x005A2F0A, 67, "8acbf09832cc9ae0c02660c8a76255f569a3915c05da47309799be2cfcb2747f"),
            (0x00717230, 57, "3187e207ece6a4fdc580107bfdd4255e1b6e228295e89b817a40e23b4069d526"),
            (0x00480900, 545, "752388b8cbff6cbe86bbb73d62050fa43707d626b9262177e0e4f11dfdb95346"),
        )
        for va, size, digest in spans:
            raw = _pe_offset(image, va)
            self.assertEqual(hashlib.sha256(image[raw:raw + size]).hexdigest(), digest)

    def test_fresh_camera_image_and_full_predecessor_chain(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]
        save = root / "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX"
        replay = root / "ron-data/replays/Playback - 2026.08.11 11'44'38 (Tue).rcx"
        if not save.exists() or not replay.exists():
            self.skipTest("user SVX/RCX unavailable")
        save_raw, replay_raw = save.read_bytes(), replay.read_bytes()
        self.assertEqual(hashlib.sha256(save_raw).hexdigest(), "161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7")
        self.assertEqual(hashlib.sha256(replay_raw).hexdigest(), "558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54")
        plain = gzip.decompress(save_raw)
        parsed = parse_camera_section(plain, 0x2BF7B)
        self.assertEqual((parsed.offset, parsed.end, parsed.size), (0x2BF7B, 0x2C234, 697))
        self.assertEqual(parsed.sha256, "4d5e25a4acedc3660f8ad38b1629fa719ea6fca82cf58bbed13c00b8c7c82996")
        self.assertEqual(parsed.raw, bytes(697))
        damaged = bytearray(plain)
        damaged[parsed.end] ^= 1
        self.assertEqual(parse_camera_section(damaged, parsed.offset), parsed)
        if parse_detail_threshold_block is not None:
            world = parse_world_section(plain, 0x27F53)
            daemon = parse_game_daemon_block(plain, world.end)
            random = parse_game_random_block(plain, daemon.end)
            graphics = parse_graphic_events_section(plain, random.end, FRESH_RETAIL_SLOT_COUNT)
            scene = parse_scene_section(plain, graphics.end)
            farms = parse_farm_structs_section(plain, scene.end)
            wonders = parse_unbuilt_wonders_section(plain, farms.end)
            cities = parse_unbuilt_cities_section(plain, wonders.end)
            forts = parse_unbuilt_forts_section(plain, cities.end)
            conquest = parse_conquest_game_section(plain, forts.end)
            detail = parse_detail_threshold_block(plain, conquest.end)
            self.assertEqual(detail.end, parsed.offset)
            self.assertEqual(parse_camera_section(plain, detail.end), parsed)
        save_tree = savegame_parse.parse(plain)
        replay_tree = savegame_parse.parse(gzip.decompress(replay_raw))
        self.assertEqual(
            (_field(_find(save_tree, "GameInfo::walk_data"), "seed"), _field(_find(replay_tree, "GameInfo::walk_data"), "seed")),
            ("0x014810ac", "0x007f93e0"),
        )


if __name__ == "__main__":
    unittest.main()
