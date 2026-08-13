#!/usr/bin/env python3
"""Dynamic, mutation, truncation, PE/PDB, and SVX SelectGroups gates."""

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
from savegame_camera import parse_camera_section  # noqa: E402
from savegame_conquest_game import parse_conquest_game_section  # noqa: E402
from savegame_detail_threshold import parse_detail_threshold_block  # noqa: E402
from savegame_farm_structs import parse_farm_structs_section  # noqa: E402
from savegame_game_daemon import parse_game_daemon_block  # noqa: E402
from savegame_game_random import parse_game_random_block  # noqa: E402
from savegame_graphic_events import FRESH_RETAIL_SLOT_COUNT, parse_graphic_events_section  # noqa: E402
from savegame_scene import parse_scene_section  # noqa: E402
from savegame_select_groups import (  # noqa: E402
    GROUP_MEMBER_CAPACITY,
    SELECT_GROUP_TAIL_SIZE,
    SELECT_LIST_COUNT,
    TAG_SELECT_GROUP_INDEX,
    TAG_SELECT_GROUPS_INDEX,
    SelectGroupsParseError,
    parse_select_groups_section,
)
from savegame_unbuilt_cities import parse_unbuilt_cities_section  # noqa: E402
from savegame_unbuilt_forts import parse_unbuilt_forts_section  # noqa: E402
from savegame_unbuilt_wonders import parse_unbuilt_wonders_section  # noqa: E402
from savegame_world import parse_world_section  # noqa: E402


PREFIX = b"CAMERA-END"
FOLLOWING_OPTIONS = b"OPTIONS-ARRAY-OWNER" * 64
LAYOUT_SHA = "e5bbd4c96699cfabc50f372a7b8f1561b22639e0482a4e1e2ddd67187d124281"


def _row(array_index: int, row_index: int, num: int, tag: int = 0) -> bytes:
    fixed = [1000 + array_index * 100 + row_index, -50 - row_index, num]
    fixed.extend(200 + array_index * 20 + row_index * 3 + field for field in range(14))
    group = struct.pack("<17i4B", *fixed, 10 + row_index, 1 + array_index, 7 - row_index, 3 + row_index)
    group += struct.pack(f"<{num}h", *(300 + row_index * 10 + slot for slot in range(num)))
    group += struct.pack(f"<{num}i", *(-1000 - slot for slot in range(num)))
    group += struct.pack(f"<{num}i", *(2000 + slot for slot in range(num)))
    group += struct.pack(f"<{num}i", *(-3000 - slot for slot in range(num)))
    group += struct.pack(f"<{num}i", *(4000 + slot for slot in range(num)))
    group += struct.pack(f"<{num}b", *(-10 + slot for slot in range(num)))
    tail = struct.pack("<B6i", tag, -2 - row_index, 20 + row_index, 30 + row_index, -40 - row_index, 50 + row_index, -60 - row_index)
    return group + tail


def _array(array_index: int, nums: tuple[int, ...]) -> bytes:
    if not nums:
        return struct.pack("<i", 0)
    return struct.pack("<iihB", len(nums), len(nums) + 3, -7 - array_index, array_index + 1) + b"".join(
        _row(array_index, row_index, num) for row_index, num in enumerate(nums)
    )


def _section(outer_tag: int = 0) -> tuple[bytes, int, bytes]:
    payload = bytes((outer_tag,)) + _array(0, (0, 3)) + _array(1, (1,))
    return PREFIX + payload + FOLLOWING_OPTIONS, len(PREFIX), FOLLOWING_OPTIONS


def _find(node: object, prefix: str):
    if node.name.startswith(prefix): return node
    for child in node.kids:
        found = _find(child, prefix)
        if found is not None: return found
    return None


def _field(node: object, name: str):
    return next(field["value"] for field in node.fields if field["name"] == name)


def _pe_offset(image: bytes, va: int) -> int:
    pe = struct.unpack_from("<I", image, 0x3C)[0]; count = struct.unpack_from("<H", image, pe + 6)[0]; optional_size = struct.unpack_from("<H", image, pe + 20)[0]; optional = pe + 24; base = struct.unpack_from("<I", image, optional + 28)[0]; rva, table = va - base, optional + optional_size
    for index in range(count):
        section = table + index * 40; virtual_size, virtual_address, raw_size, raw = struct.unpack_from("<IIII", image, section + 8)
        if virtual_address <= rva < virtual_address + max(virtual_size, raw_size): return raw + rva - virtual_address
    raise AssertionError(f"VA {va:#x} outside image")


class SelectGroupsParserTests(unittest.TestCase):
    def test_two_dynamic_arrays_group_rows_and_options_boundary(self) -> None:
        data, offset, following = _section(); parsed = parse_select_groups_section(data, offset)
        self.assertEqual((TAG_SELECT_GROUPS_INDEX, TAG_SELECT_GROUP_INDEX, SELECT_LIST_COUNT), (6009, 6007, 2))
        self.assertEqual((GROUP_MEMBER_CAPACITY, SELECT_GROUP_TAIL_SIZE), (128, 24))
        self.assertEqual([array.length for array in parsed.arrays], [2, 1])
        self.assertEqual([row.group.num for row in parsed.arrays[0].rows], [0, 3])
        row = parsed.arrays[0].rows[1]
        self.assertEqual(row.group.member_ids, (310, 311, 312)); self.assertEqual(row.group.off_x, (-1000, -1001, -1002)); self.assertEqual(row.group.angles, (-10, -9, -8))
        self.assertEqual((row.whose, row.flashing, row.named, row.item_ox, row.good_ox, row.flash_frame), (-3, 21, 31, -41, 51, -61))
        self.assertEqual(parsed.layout_sha256, LAYOUT_SHA); self.assertEqual(parsed.sha256, hashlib.sha256(data[offset:parsed.end]).hexdigest()); self.assertEqual(data[parsed.end:], following)

    def test_every_owned_byte_mutation_changes_receipt_or_is_rejected(self) -> None:
        data, offset, _ = _section(); baseline = parse_select_groups_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data); damaged[offset + relative] ^= 1
                try: parsed = parse_select_groups_section(damaged, offset)
                except SelectGroupsParseError: continue
                self.assertNotEqual(parsed, baseline); self.assertNotEqual(parsed.sha256, baseline.sha256)

    def test_every_truncation_and_structural_contradictions_fail_closed(self) -> None:
        data, offset, _ = _section(); baseline = parse_select_groups_section(data, offset)
        for retained in range(baseline.size):
            with self.subTest(retained=retained):
                with self.assertRaises(SelectGroupsParseError): parse_select_groups_section(data[:offset + retained], offset)
        first = baseline.arrays[0]
        damaged = bytearray(data); struct.pack_into("<i", damaged, first.offset + 4, 1)
        with self.assertRaisesRegex(SelectGroupsParseError, "invalid history"): parse_select_groups_section(damaged, offset)
        damaged = bytearray(data); damaged[first.offset + 10] |= 0x40
        with self.assertRaisesRegex(SelectGroupsParseError, "invalid history"): parse_select_groups_section(damaged, offset)
        damaged = bytearray(data); struct.pack_into("<i", damaged, first.rows[0].group.offset + 8, 129)
        with self.assertRaisesRegex(SelectGroupsParseError, "member capacity"): parse_select_groups_section(damaged, offset)
        damaged = bytearray(data); struct.pack_into("<i", damaged, first.offset, -1)
        with self.assertRaisesRegex(SelectGroupsParseError, "invalid length"): parse_select_groups_section(damaged, offset)

    def test_both_tag_levels_and_next_options_owner_are_exact(self) -> None:
        data, offset, _ = _section(); baseline = parse_select_groups_section(data, offset)
        damaged = bytearray(data); damaged[offset] = 1
        with self.assertRaisesRegex(SelectGroupsParseError, "SelectGroups tag"): parse_select_groups_section(damaged, offset)
        self.assertEqual(parse_select_groups_section(damaged, offset, require_tags=False).tag, 1)
        row_tag = baseline.arrays[0].rows[1].group.end
        damaged = bytearray(data); damaged[row_tag] = 1
        with self.assertRaisesRegex(SelectGroupsParseError, "SelectGroup tag"): parse_select_groups_section(damaged, offset)
        self.assertEqual(parse_select_groups_section(damaged, offset, require_tags=False).arrays[0].rows[1].tag, 1)
        for relative in range(len(data) - baseline.end):
            with self.subTest(relative=relative):
                damaged = bytearray(data); damaged[baseline.end + relative] ^= 0xFF
                self.assertEqual(parse_select_groups_section(damaged, offset), baseline)

    def test_pdb_mutation_and_exact_executable_bodies_are_frozen(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]; schema, pdb, exe = root / "schema/pdb-types.json", root / "ron-bin/sbl/rise.pdb", root / "ron-bin/riseofnations.exe"
        if not schema.exists(): self.skipTest("matched PDB schema unavailable")
        classes = json.loads(schema.read_text())["classes"]; names = ("SelectGroups", "Array<SelectGroup>", "GroupData", "GroupOut", "Group", "SelectGroup"); subset = {name: classes[name] for name in names}; subset["SelectGroup"] = dict(subset["SelectGroup"]); subset["SelectGroup"]["size"] = 2540
        with tempfile.TemporaryDirectory() as directory:
            bad = pathlib.Path(directory) / "pdb-types.json"; bad.write_text(json.dumps({"classes": subset}))
            with self.assertRaisesRegex(SelectGroupsParseError, "SelectGroup layout disagrees"): parse_select_groups_section(bytes(9), 0, schema_path=bad)
        if not pdb.exists() or not exe.exists(): self.skipTest("matched PE/PDB unavailable")
        self.assertEqual(hashlib.sha256(schema.read_bytes()).hexdigest(), "399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14"); self.assertEqual(hashlib.sha256(pdb.read_bytes()).hexdigest(), "334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5")
        image = exe.read_bytes(); self.assertEqual(hashlib.sha256(image).hexdigest(), "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079")
        spans = ((0x00717230, 57, "3187e207ece6a4fdc580107bfdd4255e1b6e228295e89b817a40e23b4069d526"), (0x00480900, 545, "752388b8cbff6cbe86bbb73d62050fa43707d626b9262177e0e4f11dfdb95346"), (0x007171B0, 63, "986eebaf5f23b364dee9c059b1e181984141036bce495c465ef21ba2fbe17a84"), (0x00708400, 181, "4b69d8857b8c760dc33641a2b1fc344e76bbac3d03d6bb1bf804bbc74fcd794f"), (0x0072C240, 80, "5173806c509cd96f5f5b0a9f43a45e188c6d9974f23d6e8a5412bfb5a92d42ed"), (0x00480CC0, 487, "8af883a3db8cb42c004123fec9e4baac0171f00cb12bb1358695320b4d0b6f29"), (0x005A2F57, 45, "ebc2f58ea2e39d1cde84f72f69e1f937aa6b3318f8ce4d469733c3da799d40dd"))
        for va, size, digest in spans:
            raw = _pe_offset(image, va); self.assertEqual(hashlib.sha256(image[raw:raw + size]).hexdigest(), digest)

    def test_fresh_chain_reaches_exact_options_array_and_rcx_is_independent(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]; save = root / "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX"; replay = root / "ron-data/replays/Playback - 2026.08.11 11'44'38 (Tue).rcx"
        if not save.exists() or not replay.exists(): self.skipTest("user SVX/RCX unavailable")
        save_raw, replay_raw = save.read_bytes(), replay.read_bytes(); self.assertEqual(hashlib.sha256(save_raw).hexdigest(), "161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7"); self.assertEqual(hashlib.sha256(replay_raw).hexdigest(), "558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54")
        plain = gzip.decompress(save_raw); world = parse_world_section(plain, 0x27F53); daemon = parse_game_daemon_block(plain, world.end); random = parse_game_random_block(plain, daemon.end); graphics = parse_graphic_events_section(plain, random.end, FRESH_RETAIL_SLOT_COUNT); scene = parse_scene_section(plain, graphics.end); farms = parse_farm_structs_section(plain, scene.end); wonders = parse_unbuilt_wonders_section(plain, farms.end); cities = parse_unbuilt_cities_section(plain, wonders.end); forts = parse_unbuilt_forts_section(plain, cities.end); conquest = parse_conquest_game_section(plain, forts.end); detail = parse_detail_threshold_block(plain, conquest.end); camera = parse_camera_section(plain, detail.end); parsed = parse_select_groups_section(plain, camera.end)
        self.assertEqual((parsed.offset, parsed.end, parsed.size), (0x2C234, 0x2C23D, 9)); self.assertEqual([array.length for array in parsed.arrays], [0, 0]); self.assertEqual(parsed.sha256, "3e7077fd2f66d689e0cee6a7cf5b37bf2dca7c979af356d0a31cbc5c85605c7d"); self.assertEqual(plain[parsed.end:parsed.end + 4], bytes(4))
        damaged = bytearray(plain); damaged[parsed.end] ^= 1; self.assertEqual(parse_select_groups_section(damaged, camera.end), parsed)
        save_tree, replay_tree = savegame_parse.parse(plain), savegame_parse.parse(gzip.decompress(replay_raw)); self.assertEqual((_field(_find(save_tree, "GameInfo::walk_data"), "seed"), _field(_find(replay_tree, "GameInfo::walk_data"), "seed")), ("0x014810ac", "0x007f93e0"))


if __name__ == "__main__": unittest.main()
