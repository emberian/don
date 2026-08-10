#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Host-side structural and cross-build checks for the PE32 injector."""

from __future__ import annotations

import shutil
import struct
import subprocess
import tempfile
import unittest
from pathlib import Path


HERE = Path(__file__).resolve().parent
SOURCE = HERE / "donject.c"
RETAIL_SHA256 = "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079"


class DonjectSourceTests(unittest.TestCase):
    def test_fail_closed_injection_contract_is_present(self) -> None:
        source = SOURCE.read_text()
        for required in (
            RETAIL_SHA256,
            "QueryFullProcessImageNameW",
            "IMAGE_FILE_MACHINE_I386",
            "IMAGE_NT_OPTIONAL_HDR32_MAGIC",
            "IsWow64Process2",
            "GetFinalPathNameByHandleW",
            "REFUSED dll-hash-mismatch",
            'GetProcAddress(kernel32, proc_name)',
            'remote_system_proc(process, pid, "LoadLibraryW", "inject", remote_proc)',
            "remote_owner.base + rva",
            "file_identity_equal(&local_image.identity, &remote_image.identity)",
            "VirtualQueryEx(process",
            "WaitForSingleObject(thread, INJECT_WAIT_MS)",
            "loaded_dll_state(pid, &dll, &loaded)",
            "VirtualFreeEx(process, remote_path, 0, MEM_RELEASE)",
            "result=already-loaded status=refused reason=DllMain-not-rerun",
            "LOADED_EXACT_SIZE_MISMATCH",
            "me.modBaseSize != dll->pe.image_size",
            "REFUSED loaded-module-size-mismatch",
            "FAILED loaded-module-size-mismatch",
            "REFUSED dll-path-encoding boundary=ASCII argument=<redacted>",
            "path_boundary=ASCII",
            "protocol=donject.v2 command=base status=mapped",
            "protocol=donject.v2 command=base status=absent",
            "protocol=donject.v2 command=base status=error",
            "protocol=donject.v2 command=modules status=ok",
            "protocol=donject.v2 command=modules status=module",
            "protocol=donject.v2 command=modules status=error",
            '"off=%X root=%08X pointer_addr=%08X root_value=%08X stable=%d',
        ):
            self.assertIn(required, source)
        self.assertNotIn('GetProcAddress(GetModuleHandleA("kernel32.dll"), "LoadLibraryA")', source)

        identity_check = source.index("file_identity_equal(&identity, &dll->identity)")
        size_check = source.index("me.modBaseSize != dll->pe.image_size", identity_check)
        exact_return = source.index("return LOADED_EXACT;", size_check)
        self.assertLess(identity_check, size_check)
        self.assertLess(size_check, exact_return)

        ascii_gate = source.index("if (!ascii_argument(dll_argument))")
        conversion = source.index("multibyte_to_wide(dll_argument", ascii_gate)
        remote_thread = source.index("CreateRemoteThread(process", conversion)
        self.assertLess(ascii_gate, conversion)
        self.assertLess(conversion, remote_thread)

    def test_cross_build_is_pe32_i386(self) -> None:
        zig = shutil.which("zig")
        if zig is None:
            self.skipTest("zig is not installed")
        with tempfile.TemporaryDirectory(prefix="donject-test-") as directory:
            output = Path(directory) / "donject.exe"
            subprocess.run(
                [
                    zig,
                    "cc",
                    "-target",
                    "x86-windows-gnu",
                    "-O2",
                    "-Wall",
                    "-Wextra",
                    "-Werror",
                    "-o",
                    str(output),
                    str(SOURCE),
                ],
                check=True,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
            )
            image = output.read_bytes()
            self.assertEqual(image[:2], b"MZ")
            pe_offset = struct.unpack_from("<I", image, 0x3C)[0]
            self.assertEqual(image[pe_offset : pe_offset + 4], b"PE\0\0")
            machine = struct.unpack_from("<H", image, pe_offset + 4)[0]
            optional_magic = struct.unpack_from("<H", image, pe_offset + 24)[0]
            self.assertEqual(machine, 0x14C)
            self.assertEqual(optional_magic, 0x10B)


if __name__ == "__main__":
    unittest.main()
