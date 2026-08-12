#!/usr/bin/env python3
"""Dynamic grammar, mutation, truncation, PE/PDB, and SVX GraphicEvents gates."""

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
from savegame_game_daemon import parse_game_daemon_block  # noqa: E402
from savegame_game_random import parse_game_random_block  # noqa: E402
from savegame_graphic_events import (  # noqa: E402
    EVENT_KINDS_PER_GROUP,
    FRESH_RETAIL_SLOT_COUNT,
    TAG_STRING_TABLE_INDEX,
    GraphicEventsParseError,
    parse_graphic_events_section,
)
from savegame_world import parse_world_section  # noqa: E402


PREFIX = b"RANDOM-END"
FOLLOWING_OWNER = b"SCENE-SENTINEL" * 8
LAYOUT_SHA = "ec970c7071509946a605359595292b3c614c5faaca94ca0ff95f365da1ebbeb7"


def _event(event_type: int, seed: int) -> bytes:
    size = 19 if event_type == 8 else 28
    return struct.pack("<i", event_type) + bytes((seed + i) & 0xFF for i in range(size))


def _event_array(kind: int, active: bool) -> bytes:
    if not active:
        return struct.pack("<i", 0)
    presence = (1, 0, 1)
    history = struct.pack("<iihB", 3, 5, -2, 3)
    repeated = struct.pack("<ih", 5, -2)
    return history + bytes(presence) + repeated + _event(8, 0x20 + kind) + _event(5, 0x60 + kind)


def _group(civ: int, age: int, next_present: int, active_kind: int | None) -> bytes:
    arrays = b"".join(_event_array(kind, kind == active_kind) for kind in range(EVENT_KINDS_PER_GROUP))
    return arrays + struct.pack("<bbB", civ, age, next_present)


def _array(length: int, element_size: int, seed: int) -> bytes:
    if not length:
        return struct.pack("<i", 0)
    data = bytes((seed + i) & 0xFF for i in range(length * element_size))
    return struct.pack("<iihB", length, length + 2, -3, 2) + data


def _section() -> tuple[bytes, int, bytes]:
    # Three runtime slots: root 0 owns a two-node chain, root 1 is empty, root
    # 2 owns one group. Each group activates a different GraphicEvent array.
    payload = bytes((0, 1, 0, 1))
    payload += _group(-2, 3, 1, 0)
    payload += _group(4, -5, 0, 7)
    payload += _group(6, 7, 0, 37)
    payload += _array(2, 2, 0xA0)
    payload += _array(1, 4, 0xB0)
    payload += _array(0, 4, 0)
    payload += _array(2, 24, 0xC0)
    payload += struct.pack("<3i", -101, 202, -303)
    return PREFIX + payload + FOLLOWING_OWNER, len(PREFIX), FOLLOWING_OWNER


def _find(node: object, prefix: str):
    if node.name.startswith(prefix): return node
    for child in node.kids:
        found = _find(child, prefix)
        if found is not None: return found
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


def _empty_scene_end(data: bytes, offset: int) -> int:
    # Exact independent Scene grammar in this fresh fixture: tag, two empty
    # BitMask<32> images (bits,size), five empty arrays, direct +0x304..+0x308.
    view = memoryview(data)
    if view[offset] != 0: raise AssertionError("fresh Scene tag")
    pos = offset + 1
    for _ in range(2):
        bits, size = struct.unpack_from("<ii", view, pos)
        if (bits, size) != (0, 0): raise AssertionError("fresh Scene mask")
        pos += 8
    for _ in range(5):
        if struct.unpack_from("<i", view, pos)[0] != 0: raise AssertionError("fresh Scene array")
        pos += 4
    pos += 4
    return pos


class GraphicEventsParserTests(unittest.TestCase):
    def test_full_dynamic_grammar_linked_groups_and_scene_boundary(self) -> None:
        data, offset, following = _section()
        parsed = parse_graphic_events_section(data, offset, 3)
        self.assertEqual(TAG_STRING_TABLE_INDEX, 3575)
        self.assertEqual(parsed.presence, (1, 0, 1))
        self.assertEqual([(group.slot, group.chain_index) for group in parsed.groups], [(0, 0), (0, 1), (2, 0)])
        self.assertEqual([(group.civ, group.age, group.next_present) for group in parsed.groups], [(-2, 3, 1), (4, -5, 0), (6, 7, 0)])
        active = [next(array for array in group.event_arrays if array.length) for group in parsed.groups]
        self.assertEqual([array.kind for array in active], [0, 7, 37])
        self.assertEqual([array.presence for array in active], [(1, 0, 1)] * 3)
        self.assertEqual([[event.event_type if event else None for event in array.events] for array in active], [[8, None, 5]] * 3)
        self.assertEqual([[event.size if event else None for event in array.events] for array in active], [[23, None, 32]] * 3)
        self.assertEqual([array.length for array in parsed.arrays], [2, 1, 0, 2])
        self.assertEqual([array.element_size for array in parsed.arrays], [2, 4, 4, 24])
        self.assertEqual(parsed.missile_offset, (-101, 202, -303))
        self.assertEqual(parsed.layout_sha256, LAYOUT_SHA)
        self.assertEqual(parsed.sha256, hashlib.sha256(data[offset:parsed.end]).hexdigest())
        self.assertEqual(data[parsed.end:], following)

    def test_every_owned_byte_mutation_changes_receipt_or_is_rejected(self) -> None:
        data, offset, _ = _section()
        baseline = parse_graphic_events_section(data, offset, 3)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data); damaged[offset + relative] ^= 1
                try:
                    parsed = parse_graphic_events_section(damaged, offset, 3)
                except GraphicEventsParseError:
                    continue
                self.assertNotEqual(parsed, baseline)
                self.assertNotEqual(parsed.sha256, baseline.sha256)

    def test_every_truncation_and_wrong_runtime_count_fail_closed(self) -> None:
        data, offset, _ = _section()
        size = parse_graphic_events_section(data, offset, 3).size
        for retained in range(size):
            with self.subTest(retained=retained):
                with self.assertRaises(GraphicEventsParseError):
                    parse_graphic_events_section(data[:offset + retained], offset, 3)
        with self.assertRaises(GraphicEventsParseError): parse_graphic_events_section(data, offset, -1)
        with self.assertRaises(GraphicEventsParseError): parse_graphic_events_section(data, offset, 4)

    def test_tags_histories_presence_and_next_owner_are_strict(self) -> None:
        data, offset, _ = _section()
        baseline = parse_graphic_events_section(data, offset, 3)
        damaged = bytearray(data); damaged[baseline.end] ^= 0xFF
        self.assertEqual(parse_graphic_events_section(damaged, offset, 3), baseline)
        damaged = bytearray(data); damaged[offset] = 1
        with self.assertRaisesRegex(GraphicEventsParseError, "tag"): parse_graphic_events_section(damaged, offset, 3)
        self.assertEqual(parse_graphic_events_section(damaged, offset, 3, require_tag=False).tag, 1)
        damaged = bytearray(data); damaged[offset + 1] = 2
        with self.assertRaisesRegex(GraphicEventsParseError, "presence"): parse_graphic_events_section(damaged, offset, 3)
        active = next(array for array in baseline.groups[0].event_arrays if array.length)
        damaged = bytearray(data); damaged[active.presence_offset] = 2
        with self.assertRaisesRegex(GraphicEventsParseError, "presence"): parse_graphic_events_section(damaged, offset, 3)
        damaged = bytearray(data); damaged[active.offset + 10] |= 0x40
        with self.assertRaisesRegex(GraphicEventsParseError, "history"): parse_graphic_events_section(damaged, offset, 3)
        damaged = bytearray(data); damaged[active.presence_offset + active.length] ^= 1
        with self.assertRaisesRegex(GraphicEventsParseError, "duplicated history"): parse_graphic_events_section(damaged, offset, 3)
        damaged = bytearray(data); damaged[baseline.groups[0].next_presence_offset] = 2
        with self.assertRaisesRegex(GraphicEventsParseError, "next presence"): parse_graphic_events_section(damaged, offset, 3)

    def test_pdb_mutation_and_all_executable_bodies_are_frozen(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]
        schema, pdb, exe = root / "schema/pdb-types.json", root / "ron-bin/sbl/rise.pdb", root / "ron-bin/riseofnations.exe"
        if not schema.exists(): self.skipTest("matched PDB schema unavailable")
        classes = json.loads(schema.read_text())["classes"]
        subset = {name: classes[name] for name in ("GraphicEvents", "EventGroup", "PtrArray<GraphicEvent>", "GraphicEvent", "SimpleArray<unsigned short>", "SimpleArray<int>", "SimpleArray<float>", "Array<AmbienceStruct>", "AmbienceStruct")}
        subset["EventGroup"] = dict(subset["EventGroup"]); subset["EventGroup"]["size"] = 1071
        with tempfile.TemporaryDirectory() as directory:
            bad = pathlib.Path(directory) / "pdb-types.json"; bad.write_text(json.dumps({"classes": subset}))
            with self.assertRaisesRegex(GraphicEventsParseError, "EventGroup size disagrees"):
                parse_graphic_events_section(bytes(29), 0, 0, schema_path=bad)
        if not pdb.exists() or not exe.exists(): self.skipTest("matched PE/PDB unavailable")
        self.assertEqual(hashlib.sha256(schema.read_bytes()).hexdigest(), "399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14")
        self.assertEqual(hashlib.sha256(pdb.read_bytes()).hexdigest(), "334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5")
        image = exe.read_bytes(); self.assertEqual(hashlib.sha256(image).hexdigest(), "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079")
        spans = (
            (0x008E4D70, 802, "c319fcf5923d34b6c41d7dd0e0b4d11aa91f71c8fae342bebabf4e664f85b4bb"),
            (0x004AA490, 843, "28e28b6ff57648318ccd4a9da4d391024ed7a2ad3d31b9f1396f5b2503494aa3"),
            (0x00919780, 54, "2482f3cc01863fb7b065bda8a4efb12baac8e6b76605aadb53da09ff839a3239"),
            (0x00476610, 464, "0f315f73ac157e8832010bdbfdce3ae06c4cac5edc52aa211b9a5434c0c8a1da"),
            (0x00473120, 464, "4019613cc0bb0110f2c0fa8be7e9837394cf064e63ea8b7fd6199e8c4958024a"),
            (0x00490B10, 464, "a75d884c0c4fbb8062305ef785364a612b6c4bf6538f9b49e0d7693b6d944394"),
            (0x004AA9A0, 488, "e2d032604a61b95ca1e5074d48a75a33391d03ab4d8b3a367a8becef22825293"),
            (0x008C0F70, 255, "27df0d70180debfaf27edbb748d786c55979c8b2257832585da17929487714e4"),
        )
        for va, size, digest in spans:
            raw = _pe_offset(image, va); self.assertEqual(hashlib.sha256(image[raw:raw + size]).hexdigest(), digest)

    def test_fresh_chain_reaches_exact_scene_and_rcx_stays_independent(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]
        save = root / "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX"
        replay = root / "ron-data/replays/Playback - 2026.08.11 11'44'38 (Tue).rcx"
        if not save.exists() or not replay.exists(): self.skipTest("user SVX/RCX unavailable")
        save_raw, replay_raw = save.read_bytes(), replay.read_bytes()
        self.assertEqual(hashlib.sha256(save_raw).hexdigest(), "161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7")
        self.assertEqual(hashlib.sha256(replay_raw).hexdigest(), "558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54")
        plain = gzip.decompress(save_raw)
        world = parse_world_section(plain, 0x27F53); daemon = parse_game_daemon_block(plain, world.end); random = parse_game_random_block(plain, daemon.end)
        self.assertEqual(random.end, 0x28028)
        parsed = parse_graphic_events_section(plain, random.end, FRESH_RETAIL_SLOT_COUNT)
        self.assertEqual((parsed.offset, parsed.end, parsed.size), (0x28028, 0x2BC4D, 15397))
        self.assertEqual((sum(parsed.presence), len(parsed.groups)), (0, 0))
        self.assertEqual([array.length for array in parsed.arrays], [0, 0, 0, 0])
        self.assertEqual(parsed.missile_offset, (0, 0, 0))
        self.assertEqual(parsed.sha256, "0efb92616dbc71df8cdb506eeae773870929781508c8d341f4d1f965b2e87497")
        self.assertEqual(_empty_scene_end(plain, parsed.end), 0x2BC76)
        damaged = bytearray(plain); damaged[parsed.end] ^= 1
        self.assertEqual(parse_graphic_events_section(damaged, random.end, FRESH_RETAIL_SLOT_COUNT), parsed)
        save_tree, replay_tree = savegame_parse.parse(plain), savegame_parse.parse(gzip.decompress(replay_raw))
        self.assertEqual((_field(_find(save_tree, "GameInfo::walk_data"), "seed"), _field(_find(replay_tree, "GameInfo::walk_data"), "seed")), ("0x014810ac", "0x007f93e0"))


if __name__ == "__main__": unittest.main()
