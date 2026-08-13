#!/usr/bin/env python3
"""Dynamic, ownership, PE/PDB, and installed-SVX Options gates."""

from __future__ import annotations

import gzip
import hashlib
import json
import os
import pathlib
import struct
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import savegame_parse  # noqa: E402
from savegame_camera import parse_camera_section  # noqa: E402
from savegame_conquest_game import parse_conquest_game_section  # noqa: E402
from savegame_detail_threshold import parse_detail_threshold_block  # noqa: E402
from savegame_farm_structs import parse_farm_structs_section  # noqa: E402
from savegame_game_daemon import parse_game_daemon_block  # noqa: E402
from savegame_game_random import parse_game_random_block  # noqa: E402
from savegame_graphic_events import FRESH_RETAIL_SLOT_COUNT, parse_graphic_events_section  # noqa: E402
from savegame_options import (  # noqa: E402
    OPTION_SIZE,
    OPTION_WALKED_SIZE,
    OPTIONS_DIRECT_END,
    OPTIONS_DIRECT_OFFSET,
    OPTIONS_OPT_END,
    OPTIONS_OPT_OFFSET,
    TAG_STRING_TABLE_INDEX,
    OptionsParseError,
    parse_options_section,
)
from savegame_scene import parse_scene_section  # noqa: E402
from savegame_select_groups import parse_select_groups_section  # noqa: E402
from savegame_unbuilt_cities import parse_unbuilt_cities_section  # noqa: E402
from savegame_unbuilt_forts import parse_unbuilt_forts_section  # noqa: E402
from savegame_unbuilt_wonders import parse_unbuilt_wonders_section  # noqa: E402
from savegame_world import parse_world_section  # noqa: E402


PREFIX = b"SELECT-GROUPS-END"
FOLLOWING_COMMAND_MANAGER = b"COMMAND-MANAGER-OWNER" * 64
LAYOUT_SHA = "5af595e0d291bf8abde2989141c0f57fd70ee323eaacf3999c7c45100f426ff6"


def _option(index: int) -> bytes:
    return struct.pack(
        "<4i2b",
        -100 - index,
        200 + index,
        -300 - index,
        400 + index,
        -10 + index,
        20 - index,
    )


def _fixture(tag: int = 0) -> tuple[bytes, int, bytes]:
    array = struct.pack("<iihB", 3, 7, -9, 5) + b"".join(_option(index) for index in range(3))
    direct = struct.pack("<23i", *(1000 + index * 17 if index % 2 else -1000 - index * 19 for index in range(23)))
    payload = array + bytes((tag,)) + direct + _option(90)
    return PREFIX + payload + FOLLOWING_COMMAND_MANAGER, len(PREFIX), FOLLOWING_COMMAND_MANAGER


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


class OptionsParserTests(unittest.TestCase):
    def test_dynamic_array_tag_direct_ranges_and_command_manager_boundary(self) -> None:
        data, offset, following = _fixture()
        parsed = parse_options_section(data, offset)
        self.assertEqual((TAG_STRING_TABLE_INDEX, OPTION_SIZE, OPTION_WALKED_SIZE), (5077, 20, 18))
        self.assertEqual(
            (OPTIONS_DIRECT_OFFSET, OPTIONS_DIRECT_END, OPTIONS_OPT_OFFSET, OPTIONS_OPT_END),
            (0x20, 0x7C, 0x7C, 0x8E),
        )
        self.assertEqual(
            (parsed.array.length, parsed.array.capacity, parsed.array.increment, parsed.array.flags),
            (3, 7, -9, 5),
        )
        self.assertEqual(
            (
                parsed.array.rows[1].option,
                parsed.array.rows[1].object,
                parsed.array.rows[1].count,
                parsed.array.rows[1].disable,
                parsed.array.rows[1].grid_x,
                parsed.array.rows[1].grid_y,
            ),
            (-101, 201, -301, 401, -9, 19),
        )
        self.assertEqual((parsed.direct_words[0], parsed.direct_words[1], parsed.direct_words[-1]), (-1000, 1017, -1418))
        self.assertEqual((parsed.selected.option, parsed.selected.grid_x, parsed.selected.grid_y), (-190, 80, -70))
        self.assertEqual(parsed.layout_sha256, LAYOUT_SHA)
        self.assertEqual(parsed.sha256, hashlib.sha256(data[offset:parsed.end]).hexdigest())
        self.assertEqual(data[parsed.end:], following)

    def test_every_owned_byte_mutation_changes_receipt_or_is_rejected(self) -> None:
        data, offset, _ = _fixture()
        baseline = parse_options_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[offset + relative] ^= 1
                try:
                    parsed = parse_options_section(damaged, offset)
                except OptionsParseError:
                    continue
                self.assertNotEqual(parsed, baseline)
                self.assertNotEqual(parsed.sha256, baseline.sha256)

    def test_every_truncation_and_array_contradiction_fail_closed(self) -> None:
        data, offset, _ = _fixture()
        baseline = parse_options_section(data, offset)
        for retained in range(baseline.size):
            with self.subTest(retained=retained):
                with self.assertRaises(OptionsParseError):
                    parse_options_section(data[:offset + retained], offset)
        damaged = bytearray(data)
        struct.pack_into("<i", damaged, offset + 4, 2)
        with self.assertRaisesRegex(OptionsParseError, "invalid history"):
            parse_options_section(damaged, offset)
        damaged = bytearray(data)
        damaged[offset + 10] |= 0x40
        with self.assertRaisesRegex(OptionsParseError, "invalid history"):
            parse_options_section(damaged, offset)
        damaged = bytearray(data)
        struct.pack_into("<i", damaged, offset, -1)
        with self.assertRaisesRegex(OptionsParseError, "invalid length"):
            parse_options_section(damaged, offset)
        with self.assertRaises(OptionsParseError):
            parse_options_section(data, -1)

    def test_tag_and_every_following_command_manager_byte_are_excluded(self) -> None:
        data, offset, _ = _fixture()
        baseline = parse_options_section(data, offset)
        damaged = bytearray(data)
        damaged[baseline.array.end] = 1
        with self.assertRaisesRegex(OptionsParseError, "Options tag"):
            parse_options_section(damaged, offset)
        self.assertEqual(parse_options_section(damaged, offset, require_tag=False).tag, 1)
        for relative in range(len(data) - baseline.end):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[baseline.end + relative] ^= 0xFF
                self.assertEqual(parse_options_section(damaged, offset), baseline)

    def test_pdb_mutation_and_exact_executable_bodies_are_frozen(self) -> None:
        source_root = pathlib.Path(__file__).resolve().parents[2]
        retail_root = pathlib.Path(os.environ.get("DON_RETAIL_ROOT", source_root))
        schema = source_root / "schema/pdb-types.json"
        pdb = retail_root / "ron-bin/sbl/rise.pdb"
        exe = retail_root / "ron-bin/riseofnations.exe"
        classes = json.loads(schema.read_text())["classes"]
        subset = {name: classes[name] for name in ("Options", "Array<Option>", "Option")}
        subset["Options"] = dict(subset["Options"])
        subset["Options"]["size"] = 228
        with tempfile.TemporaryDirectory() as directory:
            bad = pathlib.Path(directory) / "pdb-types.json"
            bad.write_text(json.dumps({"classes": subset}))
            with self.assertRaisesRegex(OptionsParseError, "Options layout disagrees"):
                parse_options_section(bytes(115), 0, schema_path=bad)
        if not pdb.exists() or not exe.exists():
            self.skipTest("matched PE/PDB unavailable")
        self.assertEqual(
            hashlib.sha256(schema.read_bytes()).hexdigest(),
            "399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14",
        )
        self.assertEqual(
            hashlib.sha256(pdb.read_bytes()).hexdigest(),
            "334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5",
        )
        image = exe.read_bytes()
        self.assertEqual(
            hashlib.sha256(image).hexdigest(),
            "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079",
        )
        spans = (
            (0x0072C240, 80, "5173806c509cd96f5f5b0a9f43a45e188c6d9974f23d6e8a5412bfb5a92d42ed"),
            (0x00480CC0, 487, "8af883a3db8cb42c004123fec9e4baac0171f00cb12bb1358695320b4d0b6f29"),
            (0x0072BE20, 23, "c62c3eeac8657ca872cec51e3eb538377c795e0326ecc0fd1e0e8fa37e356ccc"),
            (0x005A2F8E, 61, "25f556c70a83a7a48a666dbc574ffe4ea408861dc674e3e42d3088cf22e5580f"),
            (0x00942D30, 222, "bfe3ff7f3e2bc9468aeb7187bf71bc2ee15281b73dae6e189a672d2478d2cb96"),
        )
        for va, size, digest in spans:
            raw = _pe_offset(image, va)
            self.assertEqual(hashlib.sha256(image[raw:raw + size]).hexdigest(), digest)

    def test_fresh_chain_reaches_exact_command_manager_start_and_rcx_is_independent(self) -> None:
        source_root = pathlib.Path(__file__).resolve().parents[2]
        retail_root = pathlib.Path(os.environ.get("DON_RETAIL_ROOT", source_root))
        save = retail_root / "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX"
        replay = retail_root / "ron-data/replays/Playback - 2026.08.11 11'44'38 (Tue).rcx"
        if not save.exists() or not replay.exists():
            self.skipTest("user SVX/RCX unavailable")
        save_raw, replay_raw = save.read_bytes(), replay.read_bytes()
        self.assertEqual(
            hashlib.sha256(save_raw).hexdigest(),
            "161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7",
        )
        self.assertEqual(
            hashlib.sha256(replay_raw).hexdigest(),
            "558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54",
        )
        plain = gzip.decompress(save_raw)
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
        camera = parse_camera_section(plain, detail.end)
        select_groups = parse_select_groups_section(plain, camera.end)
        parsed = parse_options_section(plain, select_groups.end)
        self.assertEqual((select_groups.end, parsed.offset), (0x2C23D, 0x2C23D))
        self.assertEqual((parsed.end, parsed.size, parsed.array.length), (0x2C2B0, 115, 0))
        self.assertEqual(parsed.raw, bytes(115))
        self.assertEqual(parsed.sha256, "23cd67852af04fd6885d2763266f2765b5e03c6ae3a5c1c6c95f7e03e10ec10d")
        damaged = bytearray(plain)
        damaged[parsed.end] ^= 1
        self.assertEqual(parse_options_section(damaged, parsed.offset), parsed)
        save_tree = savegame_parse.parse(plain)
        replay_tree = savegame_parse.parse(gzip.decompress(replay_raw))
        self.assertEqual(
            (
                _field(_find(save_tree, "GameInfo::walk_data"), "seed"),
                _field(_find(replay_tree, "GameInfo::walk_data"), "seed"),
            ),
            ("0x014810ac", "0x007f93e0"),
        )


if __name__ == "__main__":
    unittest.main()
