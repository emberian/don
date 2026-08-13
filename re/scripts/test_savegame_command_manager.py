#!/usr/bin/env python3
"""Grammar, ownership, PE/PDB, and installed-prefix CommandManager gates."""

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
from savegame_command_manager import (  # noqa: E402
    COMMAND_PACKAGE_HEADER_SIZE,
    COMMAND_PACKAGE_SIZE,
    COMMAND_PAYLOAD_CAPACITY,
    INSTALLED_FULL_FIFO_COUNT,
    INSTALLED_PARTIAL_FIFO_INDEX,
    INSTALLED_PARTIAL_PACKAGE_COUNT,
    PACKAGES_PER_FIFO,
    PACKAGE_FIFO_HEADER_SIZE,
    PLAYER_FIFO_COUNT,
    TAG_COMMAND_MANAGER_INDEX,
    TAG_COMMAND_PACKAGE_INDEX,
    TAG_PACKAGE_FIFO_INDEX,
    CommandManagerParseError,
    parse_command_manager_section,
    parse_installed_command_manager_prefix,
)
from savegame_conquest_game import parse_conquest_game_section  # noqa: E402
from savegame_detail_threshold import parse_detail_threshold_block  # noqa: E402
from savegame_farm_structs import parse_farm_structs_section  # noqa: E402
from savegame_game_daemon import parse_game_daemon_block  # noqa: E402
from savegame_game_random import parse_game_random_block  # noqa: E402
from savegame_graphic_events import FRESH_RETAIL_SLOT_COUNT, parse_graphic_events_section  # noqa: E402
from savegame_options import parse_options_section  # noqa: E402
from savegame_scene import parse_scene_section  # noqa: E402
from savegame_select_groups import parse_select_groups_section  # noqa: E402
from savegame_unbuilt_cities import parse_unbuilt_cities_section  # noqa: E402
from savegame_unbuilt_forts import parse_unbuilt_forts_section  # noqa: E402
from savegame_unbuilt_wonders import parse_unbuilt_wonders_section  # noqa: E402
from savegame_world import parse_world_section  # noqa: E402


PREFIX = b"OPTIONS-END"
FOLLOWING_RIVERS = b"PTR-ARRAY-RIVER-OWNER" * 64
LAYOUT_SHA = "6d1f90af707c8dc294103ff12c8a364c92d682122633bf5846818d4c4cabd74c"


def _package(fifo_index: int, package_index: int, payload: bytes, *, tagged: bool) -> bytes:
    header = struct.pack(
        "<Iiiih",
        0x80000000 + fifo_index * 100 + package_index,
        fifo_index - 4,
        package_index % 3,
        -1 - package_index,
        len(payload),
    )
    return (b"\0" if tagged else b"") + header + payload


def _fifo(fifo_index: int) -> bytes:
    header = bytes((0,)) + struct.pack("<4i", fifo_index, -fifo_index, 10 + fifo_index, 20 - fifo_index)
    rows = []
    for package_index in range(PACKAGES_PER_FIFO):
        payload = b""
        if (fifo_index, package_index) == (0, 0):
            payload = b"FIRST"
        elif (fifo_index, package_index) == (3, 9):
            payload = bytes(range(32))
        elif (fifo_index, package_index) == (7, 19):
            payload = b"LAST-PACKAGE"
        rows.append(_package(fifo_index, package_index, payload, tagged=True))
    return header + b"".join(rows)


def _fixture() -> tuple[bytes, int, bytes]:
    local = _package(9, 0, b"LOCAL-PAYLOAD", tagged=False)
    payload = b"\0\0" + local + b"".join(_fifo(index) for index in range(PLAYER_FIFO_COUNT))
    return PREFIX + payload + FOLLOWING_RIVERS, len(PREFIX), FOLLOWING_RIVERS


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


class CommandManagerParserTests(unittest.TestCase):
    def test_complete_dynamic_grammar_and_river_boundary(self) -> None:
        data, offset, following = _fixture()
        parsed = parse_command_manager_section(data, offset)
        self.assertEqual(
            (TAG_COMMAND_MANAGER_INDEX, TAG_COMMAND_PACKAGE_INDEX, TAG_PACKAGE_FIFO_INDEX),
            (574, 623, 5206),
        )
        self.assertEqual(
            (
                PLAYER_FIFO_COUNT,
                PACKAGES_PER_FIFO,
                COMMAND_PACKAGE_SIZE,
                COMMAND_PACKAGE_HEADER_SIZE,
                PACKAGE_FIFO_HEADER_SIZE,
                COMMAND_PAYLOAD_CAPACITY,
            ),
            (8, 20, 536, 18, 16, 512),
        )
        self.assertTrue(parsed.complete)
        self.assertEqual((len(parsed.fifos), {len(fifo.packages) for fifo in parsed.fifos}), (8, {20}))
        self.assertEqual(parsed.local_package.payload, b"LOCAL-PAYLOAD")
        self.assertEqual(parsed.fifos[0].packages[0].payload, b"FIRST")
        self.assertEqual(parsed.fifos[3].packages[9].payload, bytes(range(32)))
        self.assertEqual(parsed.fifos[7].packages[19].payload, b"LAST-PACKAGE")
        self.assertEqual(
            (parsed.fifos[3].front, parsed.fifos[3].front_local, parsed.fifos[3].length, parsed.fifos[3].length_local),
            (3, -3, 13, 17),
        )
        self.assertEqual(parsed.layout_sha256, LAYOUT_SHA)
        self.assertEqual(parsed.sha256, hashlib.sha256(data[offset:parsed.end]).hexdigest())
        self.assertEqual(data[parsed.end:], following)

    def test_every_owned_byte_mutation_changes_receipt_or_is_rejected(self) -> None:
        data, offset, _ = _fixture()
        baseline = parse_command_manager_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[offset + relative] ^= 1
                try:
                    parsed = parse_command_manager_section(damaged, offset)
                except CommandManagerParseError:
                    continue
                self.assertNotEqual(parsed, baseline)
                self.assertNotEqual(parsed.sha256, baseline.sha256)

    def test_every_truncation_payload_bounds_and_tags_fail_closed(self) -> None:
        data, offset, _ = _fixture()
        baseline = parse_command_manager_section(data, offset)
        for retained in range(baseline.size):
            with self.subTest(retained=retained):
                with self.assertRaises(CommandManagerParseError):
                    parse_command_manager_section(data[:offset + retained], offset)

        damaged = bytearray(data)
        struct.pack_into("<h", damaged, baseline.local_package.offset + 16, -1)
        with self.assertRaisesRegex(CommandManagerParseError, "local_package.size -1"):
            parse_command_manager_section(damaged, offset)
        package = baseline.fifos[1].packages[4]
        damaged = bytearray(data)
        struct.pack_into("<h", damaged, package.offset + 1 + 16, 513)
        with self.assertRaisesRegex(CommandManagerParseError, "outside"):
            parse_command_manager_section(damaged, offset)

        for at, label in (
            (offset, "CommandManager tag"),
            (offset + 1, "CommandPackage tag"),
            (baseline.fifos[2].offset, "package_fifos\\[2\\] tag"),
            (baseline.fifos[2].packages[7].offset, "packages\\[7\\] tag"),
        ):
            with self.subTest(label=label):
                damaged = bytearray(data)
                damaged[at] = 1
                with self.assertRaisesRegex(CommandManagerParseError, label):
                    parse_command_manager_section(damaged, offset)
                self.assertEqual(
                    parse_command_manager_section(damaged, offset, require_tags=False).end,
                    baseline.end,
                )
        with self.assertRaises(CommandManagerParseError):
            parse_command_manager_section(data, -1)

    def test_every_following_river_byte_is_excluded(self) -> None:
        data, offset, _ = _fixture()
        baseline = parse_command_manager_section(data, offset)
        for relative in range(len(data) - baseline.end):
            with self.subTest(relative=relative):
                damaged = bytearray(data)
                damaged[baseline.end + relative] ^= 0xFF
                self.assertEqual(parse_command_manager_section(damaged, offset), baseline)

    def test_pdb_mutation_and_exact_executable_bodies_are_frozen(self) -> None:
        source_root = pathlib.Path(__file__).resolve().parents[2]
        retail_root = pathlib.Path(os.environ.get("DON_RETAIL_ROOT", source_root))
        schema = source_root / "schema/pdb-types.json"
        pdb = retail_root / "ron-bin/sbl/rise.pdb"
        exe = retail_root / "ron-bin/riseofnations.exe"
        classes = json.loads(schema.read_text())["classes"]
        subset = {name: classes[name] for name in ("CommandManager", "PackageFifo", "CommandPackage")}
        subset["PackageFifo"] = dict(subset["PackageFifo"])
        subset["PackageFifo"]["size"] = 10732
        with tempfile.TemporaryDirectory() as directory:
            bad = pathlib.Path(directory) / "pdb-types.json"
            bad.write_text(json.dumps({"classes": subset}))
            with self.assertRaisesRegex(CommandManagerParseError, "PackageFifo layout disagrees"):
                parse_command_manager_section(bytes(3196), 0, schema_path=bad)
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
            (0x00942D30, 222, "bfe3ff7f3e2bc9468aeb7187bf71bc2ee15281b73dae6e189a672d2478d2cb96"),
            (0x00952500, 129, "606e9e4708c029224c4d74d82cfca60d56a4c33267fd3ccdb2a8aa80290db706"),
            (0x0043D730, 259, "98f6ddc9eeab25a905f8c1dfaf39df1bcac09a310ea5e68f87da0afb8685d5f9"),
            (0x0043D950, 259, "5b215f6e2e6f891fcea759c0741eb23e015e56e842312bbd65eb9d96b0829612"),
            (0x00509350, 157, "0ae50e79f5b3dfdc5e589ed51d86388fc3579286f03b0da3a73ef93ecde42e4a"),
            (0x005A2FD5, 44, "7cd889c60efcd81abd32445f3893c1c6aab4fcbbef35850c9089900494525565"),
            (0x004A2F80, 843, "fe26891065951acd4cc4344cd2fda3a6110c16b5406461bb592b1f77d6530f1c"),
        )
        for va, size, digest in spans:
            raw = _pe_offset(image, va)
            self.assertEqual(hashlib.sha256(image[raw:raw + size]).hexdigest(), digest)

    def test_installed_maximal_prefix_and_first_contradiction_are_exact(self) -> None:
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
        options = parse_options_section(plain, select_groups.end)
        parsed = parse_installed_command_manager_prefix(plain, options.end)
        self.assertEqual((options.end, parsed.offset), (0x2C2B0, 0x2C2B0))
        self.assertEqual(
            (
                parsed.end,
                parsed.size,
                len(parsed.fifos),
                len(parsed.fifos[-1].packages),
                INSTALLED_FULL_FIFO_COUNT,
                INSTALLED_PARTIAL_FIFO_INDEX,
                INSTALLED_PARTIAL_PACKAGE_COUNT,
            ),
            (0x2CB7A, 2250, 6, 12, 5, 5, 12),
        )
        self.assertFalse(parsed.complete)
        self.assertEqual(parsed.sha256, "35a5a2477ca191574974e3bc44f3453f69d71136231a9c12c92532b6d06d2269")
        self.assertEqual(plain[parsed.end], 0xFF)
        with self.assertRaisesRegex(
            CommandManagerParseError,
            r"package_fifos\[5\]\.packages\[12\] tag 0xff != 0x00 at 0x2cb7a",
        ):
            parse_command_manager_section(plain, options.end)
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
