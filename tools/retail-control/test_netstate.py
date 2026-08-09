#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import unittest


HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("retail_netstate", HERE / "netstate.py")
assert SPEC and SPEC.loader
netstate = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = netstate
SPEC.loader.exec_module(netstate)


def module_output(pid: int = 77) -> str:
    modules = [
        ("riseofnations.exe", r"C:\Game\riseofnations.exe", 0x00400000, 0x900000),
        ("CrossplayProxy.dll", r"C:\Game\CrossplayProxy.dll", 0x10000000, 0xCF000),
    ]
    rows = [
        f"protocol=donject.v2 command=modules status=ok pid={pid} count={len(modules)}"
    ]
    for index, (name, path, base, size) in enumerate(modules):
        rows.append(
            "protocol=donject.v2 command=modules status=module "
            f'pid={pid} index={index} module_name="{name}" module_path="{path}" '
            f"module_base=0x{base:08X} module_size=0x{size:08X}"
        )
    return "\n".join(rows)


def peek_output(data: bytes, *, module: str = "CrossplayProxy.dll", base: int = 0x10000000,
                address: int = 0x20000038, root: int = 0x100C2ED8,
                root_value: int = 0x20000000, stable: int = 1) -> str:
    lines = [
        f"# base={base:08X} addr={address:08X} len={len(data):X} module={module} "
        f"rva=C2ED8 deref={0 if stable == -1 else 1} nderef={0 if stable == -1 else 1} "
        f"off=38 root={root:08X} pointer_addr={root:08X} "
        f"root_value={root_value:08X} stable={stable}"
    ]
    for offset in range(0, len(data), 16):
        lines.append(
            f"{address + offset:08X}: " + " ".join(f"{byte:02X}" for byte in data[offset:offset + 16])
        )
    return "\n".join(lines)


class FakeProbe:
    def __init__(self, *, torn_title: bool = False, heap_title: bool = False):
        self.torn_title = torn_title
        self.heap_title = heap_title
        self.title_reads = 0
        self.mods = []
        self.hash_by_path = {}
        base = 0x00400000
        for index, (name, (size, digest)) in enumerate(netstate.EXPECTED_MODULES.items()):
            actual_size = size if size is not None else 0x900000
            module = netstate.Module(name, f"C:\\Game\\{name}", base + index * 0x10000000,
                                     actual_size)
            self.mods.append(module)
            self.hash_by_path[module.path] = digest

    def modules(self, pid: int):
        return list(self.mods)

    def sha256(self, path: str):
        return self.hash_by_path[path]

    def peek(self, pid: int, module: str, rva: int, derefs: int, offset: int, length: int):
        if derefs == 0:
            title = b"HEAP9"
            assert self.heap_title and length == len(title)
            anchor = next(item for item in self.mods if item.name.lower() == module.lower())
            address = anchor.base + rva
            assert address == 0x60000000
            return netstate.Peek(module, anchor.base, address, address, 0, -1, title)
        root = self.mods[1].base + rva
        root_value = 0x71000000 if module.lower() == "crossplaynetlib.dll" else 0x72000000
        if module.lower() == "crossplaynetlib.dll" and rva == netstate.NETSYS_RVA:
            data = bytearray(44)
            data[:4] = (2).to_bytes(4, "little")
            data[4:8] = (0x73000000).to_bytes(4, "little")
            data[8:12] = (0x73001000).to_bytes(4, "little")
            data[36:40] = (0x73000000).to_bytes(4, "little")
            data[40:44] = (0x73001000).to_bytes(4, "little")
        elif module.lower() == "crossplaynetlib.dll" and offset == 88:
            data = (0x1234).to_bytes(4, "little")
        elif module.lower() == "crossplaynetlib.dll" and offset == 96:
            data = (0x74000000).to_bytes(4, "little")
        elif module.lower() == "crossplaynetlib.dll" and offset == 468:
            data = b"\x01"
        elif module.lower() == "crossplayproxy.dll":
            self.title_reads += 1
            title = b"HEAP9" if self.heap_title else b"AB12C"
            if self.heap_title:
                data = (0x60000000).to_bytes(4, "little") + b"\0" * 12
                data += len(title).to_bytes(4, "little") + (31).to_bytes(4, "little")
            else:
                data = title + b"\0" * (16 - len(title)) + len(title).to_bytes(4, "little")
                data += (15).to_bytes(4, "little")
            if self.torn_title and self.title_reads == 2:
                data = b"XB12C" + data[5:]
        else:
            raise AssertionError((module, rva, derefs, offset, length))
        assert len(data) == length
        return netstate.Peek(module, self.mods[1].base, root_value + offset, root, root_value, 1,
                             bytes(data))


class NetStateTests(unittest.TestCase):
    def test_v2_module_inventory_is_counted_and_contiguous(self):
        modules = netstate.parse_modules(module_output(), 77)
        self.assertEqual([module.name for module in modules], ["riseofnations.exe", "CrossplayProxy.dll"])
        broken = module_output().replace("index=1", "index=0")
        with self.assertRaisesRegex(netstate.NetStateError, "row identity|indices"):
            netstate.parse_modules(broken, 77)

    def test_peek_parser_requires_exact_contiguous_bytes_and_stability_metadata(self):
        data = bytes(range(24))
        parsed = netstate.parse_peek(peek_output(data), "CrossplayProxy.dll", 24)
        self.assertEqual(parsed.data, data)
        broken = peek_output(data).replace("20000048:", "20000049:")
        with self.assertRaisesRegex(netstate.NetStateError, "incomplete or non-contiguous"):
            netstate.parse_peek(broken, "CrossplayProxy.dll", 24)
        wrong_address = peek_output(data).replace("addr=20000038", "addr=20000039")
        with self.assertRaisesRegex(netstate.NetStateError, "address/stability"):
            netstate.parse_peek(wrong_address, "CrossplayProxy.dll", 24)

    def test_collection_emits_only_bounded_scalar_and_title_fields(self):
        result = netstate.collect(77, FakeProbe())
        self.assertTrue(result["ready"])
        self.assertEqual(result["playfab_title_id"], "AB12C")
        self.assertEqual(result["netsys"]["player_count"], 2)
        self.assertEqual(result["netsys"]["player_pointer_present_mask"], 3)
        serialized = str(result).lower()
        for forbidden in ("developer_secret_key", "ticket_value", "player_name_value"):
            self.assertNotIn(forbidden, serialized)

    def test_title_object_double_read_refuses_a_torn_snapshot(self):
        with self.assertRaisesRegex(netstate.NetStateError, "changed during the bracket"):
            netstate.collect(77, FakeProbe(torn_title=True))

    def test_heap_title_reads_only_the_declared_length_twice(self):
        result = netstate.collect(77, FakeProbe(heap_title=True))
        self.assertEqual(result["playfab_title_id"], "HEAP9")

    def test_title_decoder_rejects_non_msvc_or_unbounded_shape(self):
        raw = b"A" + b"\0" * 15 + (1).to_bytes(4, "little") + (14).to_bytes(4, "little")
        with self.assertRaisesRegex(netstate.NetStateError, "small-string capacity"):
            netstate._decode_title_object(FakeProbe(), 77, FakeProbe().mods, raw)


if __name__ == "__main__":
    unittest.main()
