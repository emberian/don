#!/usr/bin/env python3
"""Focused tests for fail-closed Tribe capture ingestion."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
import zlib
from pathlib import Path


SCRIPT = Path(__file__).with_name("rules-channel-assets.py")
SPEC = importlib.util.spec_from_file_location("rules_channel_assets", SCRIPT)
assert SPEC and SPEC.loader
ASSETS = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = ASSETS
SPEC.loader.exec_module(ASSETS)


def generated_bytes(length: int, seed: int) -> bytes:
    """Generate deterministic specimen bytes without claiming retail values."""
    state = seed
    output = bytearray()
    for _ in range(length):
        state = (state * 1_664_525 + 1_013_904_223) & 0xFFFF_FFFF
        output.append(state >> 24)
    return bytes(output)


def peek_text(payload: bytes, *, enhanced: bool = False) -> str:
    base = 0x0100_0000
    address = 0x1200_1000
    if enhanced:
        header = (
            "# donject-peek-v1 pid=4242 module=riseofnations.exe "
            f"base={base:08X} rva={ASSETS.TRIBE_POINTER_RVA:08X} deref=1 off=0 "
            f"root={base + ASSETS.TRIBE_POINTER_RVA:08X} addr={address:08X} "
            f"len={len(payload):X} image_sha256={ASSETS.SUPPORTED_EXE_SHA256} stable=true"
        )
    else:
        header = f"# base={base:08X} addr={address:08X} len={len(payload):X}"
    rows = []
    for offset in range(0, len(payload), 16):
        row = payload[offset : offset + 16]
        rows.append(f"{address + offset:08X}:" + "".join(f" {byte:02X}" for byte in row))
    return "\n".join((header, *rows)) + "\n"


class TribeCaptureTests(unittest.TestCase):
    def parse(self, text: str):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "tribes.txt"
            path.write_text(text, encoding="ascii")
            return ASSETS.parse_tribe_capture(path)

    def test_current_peek_format_is_address_checked_and_normalized(self):
        raw = generated_bytes(ASSETS.TRIBE_CAPTURE_BYTES, 0xC001_D00D)
        capture = self.parse(peek_text(raw))

        self.assertEqual(len(capture.records), ASSETS.TRIBE_COUNT)
        first = capture.records[0]
        self.assertEqual(first[:0x54], bytes(0x54))
        self.assertEqual(first[0x54:0x6C], raw[0x54:0x6C])
        self.assertEqual(first[0x6C:0x70], bytes(4))
        self.assertEqual(first[0x70:0x5F0], raw[0x70:0x5F0])

    def test_enhanced_header_validates_measured_pointer_boundary(self):
        raw = generated_bytes(ASSETS.TRIBE_CAPTURE_BYTES, 0x51A7_E001)
        capture = self.parse(peek_text(raw, enhanced=True))
        self.assertEqual(capture.header["rva"], f"{ASSETS.TRIBE_POINTER_RVA:08X}")

        wrong = peek_text(raw, enhanced=True).replace(
            f"rva={ASSETS.TRIBE_POINTER_RVA:08X}", "rva=00A7FA30", 1
        )
        with self.assertRaisesRegex(ASSETS.AssetError, "Tribe pointer RVA"):
            self.parse(wrong)

    def test_missing_row_and_wrong_row_address_fail_closed(self):
        raw = generated_bytes(ASSETS.TRIBE_CAPTURE_BYTES, 0xA11C_E123)
        lines = peek_text(raw).splitlines()
        with self.assertRaisesRegex(ASSETS.AssetError, "decoded .* header declares"):
            self.parse("\n".join(lines[:-1]) + "\n")

        address, payload = lines[2].split(":", 1)
        lines[2] = f"{int(address, 16) + 4:08X}:{payload}"
        with self.assertRaisesRegex(ASSETS.AssetError, "row starts"):
            self.parse("\n".join(lines) + "\n")

    def test_raw_blob_and_wrong_length_are_rejected(self):
        raw = generated_bytes(ASSETS.TRIBE_CAPTURE_BYTES, 0xB10B_0001)
        with self.assertRaisesRegex(ASSETS.AssetError, "missing donject peek header"):
            self.parse(raw.hex())
        wrong = peek_text(raw).replace(f"len={len(raw):X}", "len=8E7F", 1)
        with self.assertRaisesRegex(ASSETS.AssetError, r"expected 24 \* 0x5f0"):
            self.parse(wrong)

    def test_generated_inputs_walk_compositionally_without_retail_fixture_values(self):
        after_types = 0x3141_5926
        constants = generated_bytes(ASSETS.RULES_BLOCK_BYTES, 0x1111_2222)
        balance = generated_bytes(ASSETS.BALANCE_BYTES, 0x3333_4444)
        raw_tribes = generated_bytes(ASSETS.TRIBE_CAPTURE_BYTES, 0x5555_6666)
        tribes = ASSETS.normalize_tribe_records(raw_tribes)
        actual = ASSETS.walk_static_rules(after_types, constants, balance, tribes)

        expected = zlib.adler32(constants, after_types) & 0xFFFF_FFFF
        expected = zlib.adler32(
            constants[
                ASSETS.RULES_DUPLICATE_OFFSET : ASSETS.RULES_DUPLICATE_OFFSET + 4
            ],
            expected,
        ) & 0xFFFF_FFFF
        expected = zlib.adler32(balance, expected) & 0xFFFF_FFFF
        for index in range(ASSETS.TRIBE_COUNT):
            record = raw_tribes[
                index * ASSETS.TRIBE_SIZE : (index + 1) * ASSETS.TRIBE_SIZE
            ]
            for begin, end in ASSETS.TRIBE_RANGES:
                expected = zlib.adler32(record[begin:end], expected) & 0xFFFF_FFFF

        self.assertEqual(actual["after_tribes"], expected)
        self.assertEqual(actual["bytes_walked"], ASSETS.RULES_WALKED_BYTES)

    def test_unwalked_mutation_is_stripped_but_walked_mutation_bites(self):
        raw = bytearray(generated_bytes(ASSETS.TRIBE_CAPTURE_BYTES, 0x7777_8888))
        baseline = ASSETS.normalize_tribe_records(bytes(raw))
        raw[0x20] ^= 0xFF
        self.assertEqual(ASSETS.normalize_tribe_records(bytes(raw)), baseline)
        raw[0x54] ^= 0xFF
        self.assertNotEqual(ASSETS.normalize_tribe_records(bytes(raw)), baseline)


if __name__ == "__main__":
    unittest.main()
