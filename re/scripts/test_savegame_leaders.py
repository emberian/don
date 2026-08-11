#!/usr/bin/env python3
"""Byte-exact synthetic tests for savegame_leaders.py (no retail bytes)."""

from __future__ import annotations

import pathlib
import struct
import sys
import unittest


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from savegame_leaders import (  # noqa: E402
    ENCRYPTED_WORD_COUNT,
    LEADER_FIXED_BODY_SIZE,
    LeadersParseError,
    parse_leaders_section,
)


def _wstr(value: str) -> bytes:
    return struct.pack("<I", len(value)) + value.encode("utf-16-le")


def _buffer(bits: int, payload: bytes) -> bytes:
    return struct.pack("<ii", bits, len(payload)) + payload


def _array(rows: list[bytes], capacity: int = 4, increment: int = -1, flags: int = 0x80) -> bytes:
    if not rows:
        return struct.pack("<i", 0)
    return (
        struct.pack("<iihB", len(rows), capacity, increment, flags)
        + b"".join(rows)
    )


def _active(index: int) -> bytes:
    body = bytearray(LEADER_FIXED_BODY_SIZE)
    struct.pack_into("<ii", body, 0, index, 20 + index)
    out = bytearray(struct.pack("<Bii", 0xEE, 1, 2))
    out += body
    out += bytes(8 * 0x5C)
    out += bytes(0x60)
    for bits, payload in ((9, b"ab"), (0, b""), (8, b"c"), (1, b"d"), (2, b"e"), (3, b"f")):
        out += _buffer(bits, payload)
    out += _array([bytes(0x18), bytes(0x18)], capacity=3)
    out += _array([bytes(0x28)], capacity=1)
    out += _array([struct.pack("<i", 7)], capacity=2, flags=0)
    out += _array([])
    out += _array([struct.pack("<i", 8), struct.pack("<i", 9)], capacity=2, flags=0)
    out += _wstr("economic")
    out += _buffer(8, b"r")
    out += _buffer(0, b"")
    out += _buffer(2, b"s")
    out += struct.pack(f"<{ENCRYPTED_WORD_COUNT}i", *range(ENCRYPTED_WORD_COUNT))
    return bytes(out)


def _section() -> tuple[bytes, int]:
    out = bytearray(b"prefix")
    offset = len(out)
    out += bytes((0x20,)) + _wstr(".\\ai\\scripts\\")
    out += _active(0)
    for _ in range(7):
        out += struct.pack("<Bii", 0xEE, 0x02000000, 0)
    out += b"TYPES-SENTINEL"
    return bytes(out), offset


class LeadersSectionTest(unittest.TestCase):
    def test_complete_active_and_inactive_walk(self) -> None:
        data, offset = _section()
        parsed = parse_leaders_section(data, offset)
        self.assertEqual(parsed.prod_script_path, ".\\ai\\scripts\\")
        self.assertEqual(len(parsed.records), 8)
        first = parsed.records[0]
        self.assertTrue(first.active)
        self.assertEqual((first.who, first.tribe), (0, 20))
        self.assertEqual([item.length for item in first.arrays], [2, 1, 1, 0, 2])
        self.assertEqual([item.capacity for item in first.arrays], [3, 1, 2, None, 2])
        self.assertEqual(first.prod_script, "economic")
        self.assertEqual(len(first.encrypted_values), ENCRYPTED_WORD_COUNT)
        self.assertEqual(first.encrypted_values[0], ("bucket[0]", 0))
        self.assertEqual(first.encrypted_values[-1], ("discovered", 61))
        self.assertTrue(all(not row.active for row in parsed.records[1:]))
        self.assertEqual(data[parsed.end :], b"TYPES-SENTINEL")

    def test_tag_mismatch_is_rejected(self) -> None:
        data, offset = _section()
        bad = bytearray(data)
        bad[offset] = 0
        with self.assertRaisesRegex(LeadersParseError, "Leaders tag"):
            parse_leaders_section(bad, offset)

    def test_truncation_is_rejected(self) -> None:
        data, offset = _section()
        with self.assertRaisesRegex(LeadersParseError, "exceeds"):
            parse_leaders_section(data[: offset + 100], offset)

    def test_invalid_array_history_is_rejected(self) -> None:
        data, offset = _section()
        parsed = parse_leaders_section(data, offset)
        sites = parsed.records[0].arrays[0]
        bad = bytearray(data)
        struct.pack_into("<i", bad, sites.offset + 4, 1)  # capacity < length 2
        with self.assertRaisesRegex(LeadersParseError, "invalid history"):
            parse_leaders_section(bad, offset)


if __name__ == "__main__":
    unittest.main()
