#!/usr/bin/env python3
"""Compare our replacement CrossplayProxy.dll's export surface against the
shipped one.

`riseofnations.exe` imports two of the four names by ordinal-bearing name from
its import directory (`?Logger@Logging@Crossplay@@…` and
`?Service@Crossplay@@…`). A single missing or misspelled export is an
unresolved import and the process will not start, so this check is the gate
before the DLL is ever put next to the game.

    uv run --with pefile --with capstone python crates/don-crossplay/dll/check-exports.py

Exit 0 when all of the following hold. Extra exports on our side are reported
but allowed — Rust's internal alias targets are visible, exactly as in
`netsys-shim` — but the complete shipped name/ordinal surface must occupy
ordinals 1..4 exactly.

  * every symbol the shipped DLL exports is exported by ours at the same
    ordinal;
  * PE32 / i386 / IMAGE_FILE_DLL identity and the shipped image base;
  * ordinals 1 and 2 are code, and both are `__cdecl` — a bare `ret`, no stack
    cleanup, because `?…@@YAPA…XZ` takes no arguments;
  * ordinals 3 and 4 are **data**: their addresses land in a writable section
    in both images and hold the shipped value `1`. These are the hybrid-GPU
    hints a vendor driver finds by walking export tables; publishing them as
    code would be the wrong kind of symbol at the right address;
  * our image resolves its heap through the same shared UCRT the three shipped
    images do (`api-ms-win-crt-heap-l1-1-0.dll`). That is what makes a
    cross-module `std::function::_Delete_this` — the ownership primitive in
    `don_crossplay::func` — a same-heap operation rather than a corruption.
"""
import pathlib
import sys

try:
    import pefile
except ImportError:
    sys.exit("needs pefile: uv run --with pefile python <this>")

try:
    import capstone
except ImportError:
    sys.exit("needs capstone: uv run --with capstone python <this>")

ROOT = pathlib.Path(__file__).resolve().parents[3]
SHIPPED = ROOT / "ron-bin/dll/CrossplayProxy.dll"
OURS = (
    ROOT
    / "crates/don-crossplay/dll/target/i686-pc-windows-msvc/release/CrossplayProxy.dll"
)

IMAGE_FILE_MACHINE_I386 = 0x014C
IMAGE_FILE_DLL = 0x2000
PE32_MAGIC = 0x010B
SHIPPED_IMAGE_BASE = 0x10000000
IMAGE_SCN_MEM_WRITE = 0x80000000

LOGGER = "?Logger@Logging@Crossplay@@YAPAVICrossplayLogger@12@XZ"
SERVICE = "?Service@Crossplay@@YAPAUICrossPlayService@1@XZ"
CODE_EXPORTS = {LOGGER: 1, SERVICE: 2}
# name -> (ordinal, the DWORD the shipped image holds)
DATA_EXPORTS = {
    "AmdPowerXpressRequestHighPerformance": (3, 1),
    "NvOptimusEnablement": (4, 1),
}
SHARED_CRT_HEAP = "api-ms-win-crt-heap-l1-1-0.dll"


def load(path):
    pe = pefile.PE(str(path))
    names = {}
    directory = getattr(pe, "DIRECTORY_ENTRY_EXPORT", None)
    if directory:
        for symbol in directory.symbols:
            if symbol.name:
                names[symbol.name.decode()] = (symbol.ordinal, symbol.address)
    identity = {
        "machine": pe.FILE_HEADER.Machine,
        "magic": pe.OPTIONAL_HEADER.Magic,
        "image_base": pe.OPTIONAL_HEADER.ImageBase,
        "is_dll": bool(pe.FILE_HEADER.Characteristics & IMAGE_FILE_DLL),
    }
    return pe, names, identity


def section_of(pe, rva):
    for section in pe.sections:
        start = section.VirtualAddress
        end = start + max(section.Misc_VirtualSize, section.SizeOfRawData)
        if start <= rva < end:
            return section
    return None


def imported_dlls(pe):
    pe.parse_data_directories(
        directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_IMPORT"]]
    )
    entries = getattr(pe, "DIRECTORY_ENTRY_IMPORT", None) or []
    return {entry.dll.decode().lower() for entry in entries}


def stack_cleanup(pe, rva):
    """The `ret imm16` of the function at `rva`, or None if the linear sweep
    finds no `ret`.

    The window has to clear the shipped `Logger`'s SEH prologue and its
    `_Init_thread_header` slow path; its first `ret` is at `+0x170`, and
    `Service`'s at `+0x9e`."""
    code = pe.get_data(rva, 1024)
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
            "build it first:  cd crates/don-crossplay/dll && "
            "XWIN_CACHE_DIR=... XWIN_ARCH=x86 cargo xwin build --release"
        )

    shipped, want, want_identity = load(SHIPPED)
    ours, got, got_identity = load(OURS)
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
    for name in sorted(want.keys() & got.keys()):
        want_ordinal, _ = want[name]
        got_ordinal, _ = got[name]
        if got_ordinal != want_ordinal:
            print(f"  ORDINAL {name}: ours={got_ordinal}, shipped={want_ordinal}")
            ok = False
        else:
            print(f"  ok {got_ordinal:2d}   {name}")
    for name in missing:
        print(f"  MISSING {name}")
        ok = False
    for name in extra:
        print(f"  extra   {name}")

    shipped_surface = {name: ordinal for name, (ordinal, _) in want.items()}
    expected_surface = dict(CODE_EXPORTS)
    expected_surface.update(
        {name: ordinal for name, (ordinal, _) in DATA_EXPORTS.items()}
    )
    if shipped_surface != expected_surface:
        print(f"FAIL the shipped surface changed: {shipped_surface}")
        ok = False

    for name, ordinal in CODE_EXPORTS.items():
        if name not in got or name not in want:
            continue
        for label, image, table in (("shipped", shipped, want), ("ours", ours, got)):
            rva = table[name][1]
            section = section_of(image, rva)
            section_name = section.Name.rstrip(b"\0").decode() if section else "<none>"
            executable = bool(section and section.Characteristics & 0x20000000)
            if not executable:
                print(f"FAIL {label} ordinal {ordinal} is not in an executable section")
                ok = False
            cleanup = stack_cleanup(image, rva)
            if cleanup != 0:
                print(
                    f"FAIL {label} ordinal {ordinal} cleanup: {cleanup!r}, "
                    "must be a bare `ret` — the shipped declaration is __cdecl "
                    "with no arguments"
                )
                ok = False
            else:
                print(f"  ok      ordinal {ordinal} {label}: code in {section_name}, __cdecl ret 0")

    for name, (ordinal, expected_value) in DATA_EXPORTS.items():
        if name not in got or name not in want:
            continue
        for label, image, table in (("shipped", shipped, want), ("ours", ours, got)):
            rva = table[name][1]
            section = section_of(image, rva)
            section_name = section.Name.rstrip(b"\0").decode() if section else "<none>"
            if not section or not (section.Characteristics & IMAGE_SCN_MEM_WRITE):
                print(
                    f"FAIL {label} ordinal {ordinal} ({name}) is at {rva:#x} in "
                    f"{section_name}, which is not writable data"
                )
                ok = False
                continue
            value = int.from_bytes(image.get_data(rva, 4), "little")
            if value != expected_value:
                print(
                    f"FAIL {label} ordinal {ordinal} ({name}) holds {value}, "
                    f"expected {expected_value}"
                )
                ok = False
            else:
                print(
                    f"  ok      ordinal {ordinal} {label}: data in {section_name} = {value}"
                )

    ours_imports = imported_dlls(ours)
    shipped_imports = imported_dlls(shipped)
    if SHARED_CRT_HEAP not in shipped_imports:
        print(f"FAIL reference no longer imports {SHARED_CRT_HEAP}")
        ok = False
    elif SHARED_CRT_HEAP not in ours_imports:
        print(
            f"FAIL ours does not import {SHARED_CRT_HEAP}; a cross-module "
            "`_Delete_this` would not be a same-heap free"
        )
        ok = False
    else:
        print(f"  ok      both images allocate on the shared UCRT ({SHARED_CRT_HEAP})")

    shipped.close()
    ours.close()
    print("PASS" if ok else "FAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
