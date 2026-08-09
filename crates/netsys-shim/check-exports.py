#!/usr/bin/env python3
"""Compare our replacement CrossplayNetLib.dll's export table against the
shipped one.

The game resolves nine `CrossplayNetLibSys` methods and two free functions
through its import table, and `get_netsys_object_ptr` through
`GetProcAddress`. A single missing or misspelled export is an unresolved
import and the process will not start, so this check is the gate before the
DLL is ever put next to the game.

    uv run --with pefile --with capstone python crates/netsys-shim/check-exports.py

Exit 0 if every symbol the shipped DLL exports is also exported by ours with
the same ordinal and PE32/i386 DLL identity. It also disassembles the
`set_p2p_callbacks` export and requires the measured `ret 0x78` stack cleanup
for its three by-value 40-byte `std::function` arguments. Extra exports on our
side are reported but allowed; Rust's internal alias targets are currently
visible, but the complete shipped name/ordinal surface must occupy ordinals
1..11 exactly.
"""
import sys
import pathlib

try:
    import pefile
except ImportError:
    sys.exit("needs pefile: uv run --with pefile python <this>")

try:
    import capstone
except ImportError:
    sys.exit("needs capstone: uv run --with capstone python <this>")

ROOT = pathlib.Path(__file__).resolve().parents[2]
SHIPPED = ROOT / "ron-bin/dll/CrossplayNetLib.dll"
OURS = ROOT / "crates/netsys-shim/target/i686-pc-windows-msvc/release/CrossplayNetLib.dll"

IMAGE_FILE_MACHINE_I386 = 0x014C
IMAGE_FILE_DLL = 0x2000
PE32_MAGIC = 0x010B
SHIPPED_IMAGE_BASE = 0x10000000
CALLBACK_EXPORT = (
    "?set_p2p_callbacks@CrossplayNetLibSys@@QAEXV?$function@$$A6AXPAVICrossplayPlayer@"
    "P2P@Crossplay@@@Z@std@@0V?$function@$$A6AXV?$basic_string@_WU?$char_traits@_W@"
    "std@@V?$allocator@_W@2@@std@@0@Z@3@@Z"
)


def exports(path):
    pe = pefile.PE(str(path))
    names = {}
    d = getattr(pe, "DIRECTORY_ENTRY_EXPORT", None)
    if d:
        for s in d.symbols:
            if s.name:
                names[s.name.decode()] = (s.ordinal, s.address)
    identity = {
        "machine": pe.FILE_HEADER.Machine,
        "magic": pe.OPTIONAL_HEADER.Magic,
        "image_base": pe.OPTIONAL_HEADER.ImageBase,
        "is_dll": bool(pe.FILE_HEADER.Characteristics & IMAGE_FILE_DLL),
    }
    pe.close()
    return names, identity


def callback_stack_cleanup(path, rva):
    pe = pefile.PE(str(path))
    code = pe.get_data(rva, 512)
    pe.close()
    decoder = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    for insn in decoder.disasm(code, SHIPPED_IMAGE_BASE + rva):
        if insn.mnemonic == "ret":
            return int(insn.op_str, 0) if insn.op_str else 0
    return None


def main():
    if not SHIPPED.exists():
        sys.exit(f"missing reference DLL: {SHIPPED}")
    if not OURS.exists():
        sys.exit(
            f"missing built DLL: {OURS}\n"
            "build it first:  cd crates/netsys-shim && XWIN_ARCH=x86 cargo xwin build --release"
        )

    want, want_identity = exports(SHIPPED)
    got, got_identity = exports(OURS)

    ok = True
    if got_identity["machine"] != IMAGE_FILE_MACHINE_I386:
        print(
            f"FAIL machine: ours is {got_identity['machine']:#06x}, "
            f"must be i386 {IMAGE_FILE_MACHINE_I386:#06x}"
        )
        ok = False
    if got_identity["magic"] != PE32_MAGIC:
        print(f"FAIL optional-header magic: {got_identity['magic']:#06x}, must be PE32")
        ok = False
    if not got_identity["is_dll"]:
        print("FAIL image is not marked DLL")
        ok = False
    if got_identity["image_base"] != SHIPPED_IMAGE_BASE:
        print(
            f"FAIL image base: {got_identity['image_base']:#010x}, "
            f"must match shipped {SHIPPED_IMAGE_BASE:#010x}"
        )
        ok = False
    if want_identity["machine"] != IMAGE_FILE_MACHINE_I386:
        print(f"note: reference machine {want_identity['machine']:#06x}")

    missing = sorted(want.keys() - got.keys())
    extra = sorted(got.keys() - want.keys())

    print(f"shipped exports: {len(want)}")
    print(f"ours:            {len(got)}")
    for n in sorted(want.keys() & got.keys()):
        want_ordinal, _ = want[n]
        got_ordinal, _ = got[n]
        if got_ordinal != want_ordinal:
            print(f"  ORDINAL {n}: ours={got_ordinal}, shipped={want_ordinal}")
            ok = False
        else:
            print(f"  ok {got_ordinal:2d}   {n}")
    for n in missing:
        print(f"  MISSING {n}")
        ok = False
    for n in extra:
        print(f"  extra   {n}")

    if CALLBACK_EXPORT in want and CALLBACK_EXPORT in got:
        shipped_cleanup = callback_stack_cleanup(SHIPPED, want[CALLBACK_EXPORT][1])
        cleanup = callback_stack_cleanup(OURS, got[CALLBACK_EXPORT][1])
        if shipped_cleanup != 0x78:
            print(
                f"FAIL reference set_p2p_callbacks stack cleanup changed: "
                f"{shipped_cleanup!r}, expected measured ret 0x78"
            )
            ok = False
        elif cleanup != shipped_cleanup:
            print(
                f"FAIL set_p2p_callbacks stack cleanup: {cleanup!r}, "
                f"must match shipped ret {shipped_cleanup:#x} for three "
                "40-byte by-value objects"
            )
            ok = False
        else:
            print("  ok      set_p2p_callbacks ret 0x78")

    print("PASS" if ok else "FAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
