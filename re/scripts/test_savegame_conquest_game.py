#!/usr/bin/env python3
"""Complete dynamic, mutation, truncation, PE/PDB, and SVX ConquestGame gates."""

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
from savegame_conquest_game import CONQUEST_PIECE_LISTS, TAG_STRING_TABLE_INDEX, ConquestGameParseError, parse_conquest_game_section  # noqa: E402
from savegame_farm_structs import parse_farm_structs_section  # noqa: E402
from savegame_game_daemon import parse_game_daemon_block  # noqa: E402
from savegame_game_random import parse_game_random_block  # noqa: E402
from savegame_graphic_events import FRESH_RETAIL_SLOT_COUNT, parse_graphic_events_section  # noqa: E402
from savegame_scene import parse_scene_section  # noqa: E402
from savegame_unbuilt_cities import parse_unbuilt_cities_section  # noqa: E402
from savegame_unbuilt_wonders import parse_unbuilt_wonders_section  # noqa: E402
from savegame_world import parse_world_section  # noqa: E402

try:
    from savegame_unbuilt_forts import parse_unbuilt_forts_section  # type: ignore  # noqa: E402
except ModuleNotFoundError:  # pragma: no cover - explicit integration-stage skip
    parse_unbuilt_forts_section = None


PREFIX = b"UNBUILT-FORTS-END"
FOLLOWING_OWNER = b"DETAIL-THRESHOLD-SENTINEL" * 8
LAYOUT_SHA = "f757a494435953871928f846c4e6c340579095e36cf40e08bf1f29ecabb8364a"


def _wstr(text: str) -> bytes:
    payload = text.encode("utf-16-le"); return struct.pack("<i", len(text)) + payload


def _array(rows: list[bytes], seed: int = 0) -> bytes:
    if not rows: return struct.pack("<i", 0)
    return struct.pack("<iihB", len(rows), len(rows) + 2, -seed - 1, seed & 3) + b"".join(rows)


def _ptr(rows: list[bytes | None], seed: int = 0) -> bytes:
    if not rows: return struct.pack("<i", 0)
    present = bytes(row is not None for row in rows); capacity, increment = len(rows) + 3, -seed - 2
    return struct.pack("<iihB", len(rows), capacity, increment, seed & 3) + present + struct.pack("<ih", capacity, increment) + b"".join(row for row in rows if row is not None)


def _simple(values: list[int], seed: int = 0) -> bytes:
    return _array([struct.pack("<I", value & 0xFFFFFFFF) for value in values], seed)


def _mask(seed: int) -> bytes:
    return struct.pack("<ii", 17, 3) + bytes((seed, seed + 1, seed + 2))


def _link(seed: int) -> bytes:
    return bytes((seed + i) & 0xFF for i in range(24)) + b"".join(_simple([seed * 10 + i], seed + i) for i in range(5))


def _node(seed: int) -> bytes:
    return bytes((0x20 + seed,)) + bytes((seed + i) & 0xFF for i in range(80)) + _wstr("node") + _wstr("battle") + _wstr("desc") + _array([_link(seed)], seed)


def _colony(seed: int) -> bytes:
    return bytes((0x30 + seed + i) & 0xFF for i in range(16)) + _simple([1, 2], seed) + _wstr("colony") + _wstr("file")


def _string_entry(seed: int) -> bytes:
    return _wstr("entry") + _wstr("pre") + bytes((seed + i) & 0xFF for i in range(12))


def _style(seed: int) -> bytes:
    return bytes((0x40 + seed,)) + bytes((seed + i) & 0xFF for i in range(140)) + b"".join(_wstr(f"s{i}") for i in range(10)) + struct.pack("<i", 77) + _ptr([_string_entry(seed), None], seed)


def _leader(seed: int) -> bytes:
    return bytes((0x50 + seed,)) + bytes((seed + i) & 0xFF for i in range(843)) + _array([bytes(range(12))], seed) + b"".join(_simple([seed + i], seed + i) for i in range(5)) + _wstr("leader") + _mask(0x60) + _mask(0x64) + _mask(0x68) + b"".join(_simple([100 + i], seed + i + 5) for i in range(3))


def _pieces() -> bytes:
    return _ptr([bytes(range(44)), None], 1) + b"".join(struct.pack("<i", 0) for _ in range(CONQUEST_PIECE_LISTS - 1))


def _named_ints(seed: int) -> bytes:
    return _array([struct.pack("<i", 100 + seed), struct.pack("<i", 200 + seed)], seed) + _wstr("one") + _wstr("two")


def _named_strings(seed: int) -> bytes:
    return _array([_wstr("value")], seed) + _wstr("name")


def _tribe(seed: int) -> bytes:
    return bytes((0x70 + seed,)) + bytes((seed + i) & 0xFF for i in range(1432))


def _section(tag: int = 0) -> tuple[bytes, int, bytes]:
    payload = bytes((tag,)) + bytes((i * 7) & 0xFF for i in range(388))
    payload += _array([bytes(range(10)), bytes(range(10, 20))], 1)
    payload += b"\x00" + _array([_leader(1)], 2)
    payload += b"\x00" + _array([_node(2)], 3) + _array([_colony(3)], 4)
    payload += _array([_wstr("continent")], 5) + _array([_wstr("barbarian")], 6)
    payload += b"".join(_wstr(f"script{i}") for i in range(10))
    payload += _simple([0x7FC01234, 0xBF800000], 7) + _pieces()
    payload += _array([bytes(range(32))], 8) + _mask(0x80)
    payload += _array([_wstr("headline")], 9) + _array([bytes(range(24))], 10)
    payload += struct.pack("<i", 2) + bytes.fromhex("010002000000030004000000")
    payload += _simple([11], 11) + _simple([12], 12) + _array([_wstr("card")], 13)
    payload += _array([_array([_style(4)], 14)], 15)
    payload += _named_ints(16) + _named_strings(17) + _named_ints(18)
    payload += b"\x00" + _array([_tribe(5)], 19)
    payload += b"".join(_wstr(f"ending{i}") for i in range(8)) + _simple([91, 92], 20)
    return PREFIX + payload + FOLLOWING_OWNER, len(PREFIX), FOLLOWING_OWNER


def _find(node: object, prefix: str):
    if node.name.startswith(prefix): return node
    for child in node.kids:
        found = _find(child, prefix)
        if found is not None: return found
    return None


def _field(node: object, name: str): return next(field["value"] for field in node.fields if field["name"] == name)


def _pe_offset(image: bytes, va: int) -> int:
    pe = struct.unpack_from("<I", image, 0x3C)[0]; count = struct.unpack_from("<H", image, pe + 6)[0]; optional_size = struct.unpack_from("<H", image, pe + 20)[0]; optional = pe + 24; base = struct.unpack_from("<I", image, optional + 28)[0]; rva, table = va - base, optional + optional_size
    for index in range(count):
        section = table + index * 40; virtual_size, virtual_address, raw_size, raw = struct.unpack_from("<IIII", image, section + 8)
        if virtual_address <= rva < virtual_address + max(virtual_size, raw_size): return raw + rva - virtual_address
    raise AssertionError(f"VA {va:#x} outside image")


class ConquestGameParserTests(unittest.TestCase):
    def test_complete_dynamic_grammar_and_detail_threshold_boundary(self) -> None:
        data, offset, following = _section(); parsed = parse_conquest_game_section(data, offset); by_name = {segment.name: segment for segment in parsed.segments}
        self.assertEqual(TAG_STRING_TABLE_INDEX, 995); self.assertEqual(len(parsed.fixed_prefix), 388); self.assertEqual(by_name["colors"].length, 2); self.assertEqual(by_name["leaders"].length, 1); self.assertEqual(by_name["nodes"].length, 1); self.assertEqual(by_name["colonies"].length, 1)
        self.assertEqual(by_name["pieces"].children[0].size, 63); self.assertEqual(len(by_name["pieces"].children), 24); self.assertEqual(by_name["reinforcements"].rows[0].size, 32); self.assertEqual(by_name["news_items"].rows[0].size, 24); self.assertEqual(by_name["valid_tribes"].values, (2,)); self.assertEqual(by_name["tribes"].rows[0].size, 1433)
        self.assertEqual(parsed.layout_sha256, LAYOUT_SHA); self.assertEqual(parsed.sha256, hashlib.sha256(data[offset:parsed.end]).hexdigest()); self.assertEqual(data[parsed.end:], following)

    def test_every_owned_byte_mutation_changes_receipt_or_is_rejected(self) -> None:
        data, offset, _ = _section(); baseline = parse_conquest_game_section(data, offset)
        for relative in range(baseline.size):
            with self.subTest(relative=relative):
                damaged = bytearray(data); damaged[offset + relative] ^= 1
                try: parsed = parse_conquest_game_section(damaged, offset)
                except (ConquestGameParseError, UnicodeError): continue
                self.assertNotEqual(parsed, baseline); self.assertNotEqual(parsed.sha256, baseline.sha256)

    def test_every_truncation_and_dynamic_histories_fail_closed(self) -> None:
        data, offset, _ = _section(); baseline = parse_conquest_game_section(data, offset)
        for retained in range(baseline.size):
            with self.subTest(retained=retained):
                with self.assertRaises(ConquestGameParseError): parse_conquest_game_section(data[:offset + retained], offset)
        colors = baseline.segments[0]; damaged = bytearray(data); struct.pack_into("<i", damaged, colors.offset + 4, 1)
        with self.assertRaisesRegex(ConquestGameParseError, "history"): parse_conquest_game_section(damaged, offset)
        pieces = next(segment for segment in baseline.segments if segment.name == "pieces"); first = pieces.children[0]
        damaged = bytearray(data); damaged[first.offset + 11] = 2
        with self.assertRaisesRegex(ConquestGameParseError, "presence"): parse_conquest_game_section(damaged, offset)
        damaged = bytearray(data); damaged[first.offset + 13] ^= 1
        with self.assertRaisesRegex(ConquestGameParseError, "repeated history"): parse_conquest_game_section(damaged, offset)

    def test_top_tag_and_next_owner_are_strictly_separate(self) -> None:
        data, offset, _ = _section(); baseline = parse_conquest_game_section(data, offset); damaged = bytearray(data); damaged[baseline.end] ^= 0xFF
        self.assertEqual(parse_conquest_game_section(damaged, offset), baseline); damaged = bytearray(data); damaged[offset] = 1
        with self.assertRaisesRegex(ConquestGameParseError, "tag"): parse_conquest_game_section(damaged, offset)
        self.assertEqual(parse_conquest_game_section(damaged, offset, require_tag=False).tag, 1)

    def test_pdb_mutation_and_full_executable_family_are_frozen(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[2]; schema, pdb, exe = root / "schema/pdb-types.json", root / "ron-bin/sbl/rise.pdb", root / "ron-bin/riseofnations.exe"
        if not schema.exists(): self.skipTest("matched PDB schema unavailable")
        classes = json.loads(schema.read_text())["classes"]; names = ("ConquestGame", "Color", "ConquestLeader", "ConquestNode", "ConquestColony", "ConquestPieces", "ConquestPiece", "ReinforcementArmy", "DynamicBitMask", "ConquestNewsItem", "ConquestStyle", "ConquestLink", "StringListEntry", "Tribe", "Array<Color>", "ObjectArray<ConquestLeader>", "ObjectArray<ConquestNode>", "ObjectArray<ConquestColony>", "ObjectArray<String>", "SimpleArray<float>", "PtrArray<ConquestPiece>", "Array<ReinforcementArmy>", "Array<ConquestNewsItem>", "LinkList<int,short>", "SimpleArray<int>", "ObjectArray<ObjectArray<ConquestStyle> >", "ObjectArray<ConquestStyle>", "NamedSimpleArray<int>", "NamedObjectArray<String>", "ObjectArray<Tribe>", "ObjectArray<ConquestBonusCard>", "ObjectArray<ConquestLink>", "PtrArray<StringListEntry>"); subset = {name: classes[name] for name in names}; subset["ConquestGame"] = dict(subset["ConquestGame"]); subset["ConquestGame"]["size"] = 5307
        with tempfile.TemporaryDirectory() as directory:
            bad = pathlib.Path(directory) / "pdb-types.json"; bad.write_text(json.dumps({"classes": subset}))
            with self.assertRaisesRegex(ConquestGameParseError, "ConquestGame size disagrees"): parse_conquest_game_section(bytes(648), 0, schema_path=bad)
        if not pdb.exists() or not exe.exists(): self.skipTest("matched PE/PDB unavailable")
        self.assertEqual(hashlib.sha256(schema.read_bytes()).hexdigest(), "399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14"); self.assertEqual(hashlib.sha256(pdb.read_bytes()).hexdigest(), "334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5"); image = exe.read_bytes(); self.assertEqual(hashlib.sha256(image).hexdigest(), "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079")
        spans = ((0x00798410, 957, "c8a3801f2ef2c4affb5014b2e400bff4f7b5ede2c34daba15a1903dfe6569c54"), (0x007ACCB0, 957, "bd5cb0bad6c353f5dc8e515431ea9c52c297528ab9bcbb562f072e98dbbb1a93"), (0x0079B6D0, 349, "003522c2e99e7ebe5d911b18a3443eb98b31fdbe968bf3ebf30ce8834d265eea"), (0x007A5520, 103, "15da7fd88b730cf867b2b905fe637e1e62ed8934b9c236b266459549e16be42a"), (0x00789400, 59, "3b6681b9127692d4111663e6a49bb1f2be70a6326b37b6c652a02b68d67eb7e5"), (0x007A5860, 81, "f771f7df141caacb24ee9594edaad5c1b1dd381298eecf3dcd2987df191a1c9a"), (0x007A9170, 205, "9ae6a1be1f199ee8f1a5219467983d73e393949b24365438884bde9825047478"), (0x006F1270, 68, "1ba92dc09cd5df8226b3ffc6009bb823dc51ed0358641de19bc48bfe65a07a51"), (0x004912E0, 469, "17305dcb44b7ae46854bcbcda3f227017bb19314a8d68479bca3dc95d225332a"), (0x00493050, 428, "cbfc9171241c2585f6f196c9838fa6c4db233ed7959cd0ccca01fc9efc1dccf7"), (0x00493A40, 541, "0f330fac3f5ec6fa4e21985a60e098386a075971163bf6fe2cfc05cf2c7fb289"), (0x004914C0, 474, "7b129600fb27a02b86cff87ac5db6251821dcad788af2e81ad17ff6314e369eb"), (0x004918A0, 491, "606e75b2c4d0fc0b00b32ee18ae3e1ae7f655577fa3600806fa3e1dc8a375461"), (0x004934C0, 488, "6bd00480339e8a7838b2f7552b4641fd51f40cbdcc9f4835b04b08fe7e885b53"), (0x00491A90, 323, "12284dec11c8782f1a436db57172985cf3df0297047a32a6d7cb033a921ecba9"), (0x004916F0, 426, "1f5d69bddd804580198b8845e407f27bacc3469d5282a03e481b3f273d66ac95"), (0x00490960, 425, "cbce04e118d3baa2bbebf7c2491c38f341967e4e150d91700f8b403e4a7552d1"), (0x00491CA0, 430, "6c83fb9381991dbc19f6f731a77666201c5134aa23322ee2e53e6c6ce8b36618"), (0x004940D0, 832, "441880d1b9b3ff58ebf708432e9ccf036bd5925dee4d671e47b4c17912c4479b"), (0x0047E230, 540, "11973c16779072e4737b7efa1aed294076878e175bf03d0c5f4e096509a4c271"))
        for va, size, digest in spans:
            raw = _pe_offset(image, va); self.assertEqual(hashlib.sha256(image[raw:raw + size]).hexdigest(), digest)

    def test_fresh_chain_reaches_exact_detail_threshold_and_rcx_is_independent(self) -> None:
        if parse_unbuilt_forts_section is None: self.skipTest("immediate predecessor UnbuiltForts helper not yet landed")
        root = pathlib.Path(__file__).resolve().parents[2]; save = root / "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX"; replay = root / "ron-data/replays/Playback - 2026.08.11 11'44'38 (Tue).rcx"
        if not save.exists() or not replay.exists(): self.skipTest("user SVX/RCX unavailable")
        save_raw, replay_raw = save.read_bytes(), replay.read_bytes(); self.assertEqual(hashlib.sha256(save_raw).hexdigest(), "161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7"); self.assertEqual(hashlib.sha256(replay_raw).hexdigest(), "558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54")
        plain = gzip.decompress(save_raw); world = parse_world_section(plain, 0x27F53); daemon = parse_game_daemon_block(plain, world.end); random = parse_game_random_block(plain, daemon.end); graphics = parse_graphic_events_section(plain, random.end, FRESH_RETAIL_SLOT_COUNT); scene = parse_scene_section(plain, graphics.end); farms = parse_farm_structs_section(plain, scene.end); wonders = parse_unbuilt_wonders_section(plain, farms.end); cities = parse_unbuilt_cities_section(plain, wonders.end); forts = parse_unbuilt_forts_section(plain, cities.end); parsed = parse_conquest_game_section(plain, forts.end)
        self.assertEqual((parsed.offset, parsed.end, parsed.size), (0x2BCEF, 0x2BF77, 648)); self.assertEqual(parsed.sha256, "f4bd841308415de6ed2727462cd66a7333ac8155b4e8e95de0220355189c785c"); self.assertEqual(set(parsed.fixed_prefix), {0}); self.assertTrue(all(getattr(segment, "length", 0) == 0 for segment in parsed.segments if hasattr(segment, "length"))); self.assertEqual(hashlib.sha256(plain[parsed.end:parsed.end + 4]).hexdigest(), "df3f619804a92fdb4057192dc43dd748ea778adc52bc498ce80524c014b81119")
        damaged = bytearray(plain); damaged[parsed.end] ^= 1; self.assertEqual(parse_conquest_game_section(damaged, forts.end), parsed)
        save_tree, replay_tree = savegame_parse.parse(plain), savegame_parse.parse(gzip.decompress(replay_raw)); self.assertEqual((_field(_find(save_tree, "GameInfo::walk_data"), "seed"), _field(_find(replay_tree, "GameInfo::walk_data"), "seed")), ("0x014810ac", "0x007f93e0"))


if __name__ == "__main__": unittest.main()
