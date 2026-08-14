#!/usr/bin/env python3
"""Synthetic grammar, ownership, PE/PDB, SVX, and RCX gates for Rivers."""

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

from savegame_command_manager import (  # noqa: E402
    CommandManagerParseError,
    parse_command_manager_section,
    parse_installed_command_manager_prefix,
)
from savegame_rivers import (  # noqa: E402
    MAX_ARRAY_LENGTH,
    NEXT_OWNER,
    SPLINE_DATA_TAG_STRING_TABLE_INDEX,
    RiversParseError,
    parse_rivers_section,
)


PREFIX = b"COMMAND-MANAGER-END"
FOLLOWING_TERRAIN = b"TERRAIN-COORD-DATA" * 32
LAYOUT_SHA = "727ee2df32d73d9dd279cf7e3ec8a365d4491f352fa2a0cf7fe87b8f3f0cc354"


def _array(
    element_size: int,
    values: bytes,
    *,
    capacity: int,
    increment: int,
    flags: int,
) -> bytes:
    if len(values) % element_size:
        raise AssertionError("misaligned fixture payload")
    length = len(values) // element_size
    if not length:
        return struct.pack("<i", 0)
    return struct.pack("<iihB", length, capacity, increment, flags) + values


def _spline() -> bytes:
    out = bytearray((0x7A,))
    out.extend(
        struct.pack(
            "<iiIIiIIiHH",
            -3,
            0x10203040,
            0x3FC00000,
            0x42C80000,
            6,
            0x3E800000,
            0x40400000,
            -9,
            4,
            7,
        )
    )
    out.extend(_array(12, bytes(range(24)), capacity=5, increment=3, flags=0x12))
    out.extend(struct.pack("<i", 0))
    out.extend(_array(4, struct.pack("<I", 0x3F000000), capacity=1, increment=-1, flags=0))
    out.extend(_array(4, struct.pack("<2I", 0x3F800000, 0x40000000), capacity=4, increment=7, flags=0x25))
    out.extend(_array(12, bytes(range(0x30, 0x3C)), capacity=2, increment=1, flags=1))
    out.extend(struct.pack("<i", 0))
    return bytes(out)


def _fixture() -> tuple[bytes, int, bytes]:
    out = bytearray(PREFIX)
    offset = len(out)
    out.extend(struct.pack("<iihB", 3, 7, -5, 0x21))
    out.extend((1, 0, 1))
    out.extend(struct.pack("<ih", 7, -5))
    out.extend(struct.pack("<i", 0))  # River markers are int, not byte.
    out.extend(struct.pack("<i", 1))
    out.extend(_spline())
    return bytes(out) + FOLLOWING_TERRAIN, offset, FOLLOWING_TERRAIN


def _pe_offset(image: bytes, va: int) -> int:
    pe = struct.unpack_from("<I", image, 0x3C)[0]
    count = struct.unpack_from("<H", image, pe + 6)[0]
    optional_size = struct.unpack_from("<H", image, pe + 20)[0]
    optional = pe + 24
    base = struct.unpack_from("<I", image, optional + 28)[0]
    rva, table = va - base, optional + optional_size
    for index in range(count):
        section = table + index * 40
        virtual_size, virtual_address, raw_size, raw = struct.unpack_from(
            "<IIII", image, section + 8
        )
        if virtual_address <= rva < virtual_address + max(virtual_size, raw_size):
            return raw + rva - virtual_address
    raise AssertionError(f"VA {va:#x} outside image")


class RiversParserTests(unittest.TestCase):
    def test_complete_sparse_spline_grammar_and_exact_terrain_boundary(self) -> None:
        data, offset, following = _fixture()
        parsed = parse_rivers_section(data, offset)
        self.assertEqual(
            (parsed.length, parsed.capacity, parsed.increment, parsed.flags),
            (3, 7, -5, 0x21),
        )
        self.assertEqual(parsed.presence, (1, 0, 1))
        self.assertEqual(
            (parsed.repeated_capacity, parsed.repeated_increment), (7, -5)
        )
        self.assertEqual(parsed.next_owner, NEXT_OWNER)
        self.assertEqual(NEXT_OWNER, "Terrain::walk_coord_data")
        self.assertEqual(SPLINE_DATA_TAG_STRING_TABLE_INDEX, 6209)
        self.assertEqual(parsed.layout_sha256, LAYOUT_SHA)
        self.assertEqual(data[parsed.end:], following)
        self.assertEqual(parsed.sha256, hashlib.sha256(data[offset:parsed.end]).hexdigest())

        null_spline, hole, full_spline = parsed.slots
        self.assertEqual(
            (null_spline.present, null_spline.spline_present, null_spline.size),
            (True, 0, 4),
        )
        self.assertFalse(hole.present)
        self.assertEqual(hole.presence_offset, parsed.presence_offset + 1)
        self.assertEqual((full_spline.present, full_spline.spline_present), (True, 1))
        spline = full_spline.spline
        self.assertIsNotNone(spline)
        self.assertEqual(
            (
                spline.tag,
                spline.type,
                spline.flags,
                spline.max_control_depth_ratio_bits,
                spline.total_spline_length_bits,
                spline.last_knot,
                spline.curr_dist_bits,
                spline.next_search_dist_bits,
                spline.search_scan,
                spline.degree,
                spline.depth,
            ),
            (
                0x7A,
                -3,
                0x10203040,
                0x3FC00000,
                0x42C80000,
                6,
                0x3E800000,
                0x40400000,
                -9,
                4,
                7,
            ),
        )
        self.assertEqual(
            tuple(array.name.rsplit(".", 1)[-1] for array in spline.arrays),
            ("control_verts", "knots", "spline_knots", "weights", "spline_verts", "spline_normals"),
        )
        self.assertEqual(tuple(array.length for array in spline.arrays), (2, 0, 1, 2, 1, 0))
        self.assertEqual(tuple(array.element_size for array in spline.arrays), (12, 4, 4, 4, 12, 12))

    def test_every_owned_byte_and_truncation_is_accounted_for(self) -> None:
        data, offset, _ = _fixture()
        baseline = parse_rivers_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(kind="mutation", relative=relative):
                damaged = bytearray(data)
                damaged[offset + relative] ^= 1
                try:
                    changed = parse_rivers_section(damaged, offset)
                except RiversParseError:
                    continue
                self.assertNotEqual(changed, baseline)
                self.assertNotEqual(changed.sha256, baseline.sha256)
            with self.subTest(kind="truncation", relative=relative):
                with self.assertRaises(RiversParseError):
                    parse_rivers_section(data[:offset + relative], offset)

        for relative in range(len(data) - baseline.end):
            with self.subTest(kind="following", relative=relative):
                damaged = bytearray(data)
                damaged[baseline.end + relative] ^= 0xFF
                self.assertEqual(parse_rivers_section(damaged, offset), baseline)

    def test_histories_presence_and_nested_array_invariants_fail_closed(self) -> None:
        data, offset, _ = _fixture()
        parsed = parse_rivers_section(data, offset)
        spline = parsed.slots[2].spline
        self.assertIsNotNone(spline)
        control, knots, spline_knots, weights, spline_verts, normals = spline.arrays
        cases: dict[str, tuple[int, bytes]] = {
            "negative outer length": (offset, struct.pack("<i", -1)),
            "oversized outer length": (offset, struct.pack("<i", MAX_ARRAY_LENGTH + 1)),
            "outer capacity below length": (offset + 4, struct.pack("<i", 2)),
            "outer writer-cleared flag": (offset + 10, b"\x61"),
            "outer presence nonboolean": (parsed.presence_offset + 1, b"\x02"),
            "outer repeated capacity mismatch": (parsed.repeated_capacity_offset, struct.pack("<i", 8)),
            "outer repeated increment mismatch": (parsed.repeated_increment_offset, struct.pack("<h", -4)),
            "spline presence nonboolean": (parsed.slots[2].spline_presence_offset, struct.pack("<i", 2)),
            "negative nested length": (control.offset, struct.pack("<i", -1)),
            "nested capacity below length": (control.offset + 4, struct.pack("<i", 1)),
            "nested writer-cleared flag": (control.offset + 10, b"\x52"),
            "oversized nested length": (weights.offset, struct.pack("<i", MAX_ARRAY_LENGTH + 1)),
        }
        self.assertEqual((knots.size, normals.size), (4, 4))
        self.assertEqual((spline_knots.capacity, spline_verts.capacity), (1, 2))
        for name, (where, replacement) in cases.items():
            with self.subTest(name=name):
                damaged = bytearray(data)
                damaged[where:where + len(replacement)] = replacement
                with self.assertRaises(RiversParseError):
                    parse_rivers_section(damaged, offset)

        empty = parse_rivers_section(struct.pack("<i", 0) + FOLLOWING_TERRAIN, 0)
        self.assertEqual((empty.length, empty.end, empty.size, empty.presence), (0, 4, 4, ()))
        self.assertIsNone(empty.capacity)

    def test_pdb_mutation_and_exact_executable_bodies_are_frozen(self) -> None:
        source_root = pathlib.Path(__file__).resolve().parents[2]
        retail_root = pathlib.Path(os.environ.get("DON_RETAIL_ROOT", source_root))
        schema = source_root / "schema/pdb-types.json"
        pdb = retail_root / "ron-bin/sbl/rise.pdb"
        exe = retail_root / "ron-bin/riseofnations.exe"
        document = json.loads(schema.read_text())
        document["classes"]["SplineData"]["size"] = 264
        with tempfile.TemporaryDirectory() as directory:
            bad = pathlib.Path(directory) / "pdb-types.json"
            bad.write_text(json.dumps(document))
            with self.assertRaisesRegex(RiversParseError, "SplineData layout disagrees"):
                parse_rivers_section(struct.pack("<i", 0), 0, schema_path=bad)
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
            (0x004A2F80, 843, "fe26891065951acd4cc4344cd2fda3a6110c16b5406461bb592b1f77d6530f1c"),
            (0x00883950, 181, "b4183fb4eaaa413b887b6acd5ba0c3f0ea27b7f557fe99c9bdfef3e117b5707f"),
            (0x009132B0, 127, "f5eb1132763a792b60a0d5f8e32043d814bcd308afd44faf1f7b24ba39695d35"),
            (0x004A46D0, 488, "69a9c55e0a532f52a0d40a4df7a2f4e93241fefe83426f9a08fba833c06cf790"),
            (0x00490B10, 464, "a75d884c0c4fbb8062305ef785364a612b6c4bf6538f9b49e0d7693b6d944394"),
            (0x005A2FF5, 68, "00970fee6129066661741cb61442a53c36fccff528eddb94e33a3ecae111326b"),
            (0x00850EF0, 128, "eb243a029738c82e65614da956f4788efc024dcee8d2855e70db36d590bedb22"),
            (0x00952A50, 233, "7925ad68827762228d3a2faedf8a892aaceaacd21446029a9680f3ae9a2565e0"),
        )
        for va, size, digest in spans:
            with self.subTest(va=f"{va:#x}"):
                raw = _pe_offset(image, va)
                self.assertEqual(hashlib.sha256(image[raw:raw + size]).hexdigest(), digest)

    def test_installed_svx_keeps_river_boundary_unknown(self) -> None:
        source_root = pathlib.Path(__file__).resolve().parents[2]
        retail_root = pathlib.Path(os.environ.get("DON_RETAIL_ROOT", source_root))
        save = retail_root / "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX"
        if not save.exists():
            self.skipTest("installed SVX unavailable")
        self.assertEqual(
            [path.resolve() for path in (retail_root / "ron-data").rglob("*.SVX")],
            [save.resolve()],
        )
        raw = save.read_bytes()
        self.assertEqual(
            hashlib.sha256(raw).hexdigest(),
            "161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7",
        )
        plain = gzip.decompress(raw)
        prefix = parse_installed_command_manager_prefix(plain, 0x2C2B0)
        self.assertEqual((prefix.end, prefix.size), (0x2CB7A, 2250))
        self.assertEqual(
            prefix.sha256,
            "35a5a2477ca191574974e3bc44f3453f69d71136231a9c12c92532b6d06d2269",
        )
        self.assertEqual(plain[prefix.end], 0xFF)
        with self.assertRaisesRegex(CommandManagerParseError, r"packages\[12\] tag 0xff"):
            parse_command_manager_section(plain, 0x2C2B0)

        # There is no defensible installed River offset: treating the first
        # contradiction as Rivers.length would be an arbitrary cross-owner join.
        arbitrary = struct.unpack_from("<i", plain, prefix.end)[0]
        self.assertNotEqual(arbitrary, 0)
        with self.assertRaises(RiversParseError):
            parse_rivers_section(plain, prefix.end)

    def test_replay_corpus_contains_no_embedded_save_container(self) -> None:
        source_root = pathlib.Path(__file__).resolve().parents[2]
        retail_root = pathlib.Path(os.environ.get("DON_RETAIL_ROOT", source_root))
        replay_root = retail_root / "ron-data/replays"
        files = sorted(replay_root.rglob("*.rcx")) if replay_root.exists() else []
        if len(files) != 64:
            self.skipTest("64-file replay corpus unavailable")

        names = ("RoNSave", "RoNMultiSave", "RoNCTWSave", "RonCTWMapSave")
        patterns = {name: name.encode("utf-16le") for name in names}
        hits = {name: [] for name in names}
        for path in files:
            raw = path.read_bytes()
            plain = gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw
            for name, pattern in patterns.items():
                container_prefix = struct.pack("<I", len(name)) + pattern
                self.assertFalse(
                    plain.startswith(container_prefix),
                    f"{path} unexpectedly begins with save container {name}",
                )
                cursor = 0
                while True:
                    at = plain.find(pattern, cursor)
                    if at < 0:
                        break
                    hits[name].append((path, at))
                    cursor = at + 1

        self.assertEqual({name: len(found) for name, found in hits.items()}, {
            "RoNSave": 0,
            "RoNMultiSave": 18,
            "RoNCTWSave": 0,
            "RonCTWMapSave": 0,
        })
        for path, at in hits["RoNMultiSave"]:
            raw = path.read_bytes()
            plain = gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw
            self.assertGreaterEqual(at, 4)
            self.assertEqual(struct.unpack_from("<I", plain, at - 4)[0], 12)


if __name__ == "__main__":
    unittest.main()
