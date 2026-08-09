#!/usr/bin/env python3
"""Compare our replacement CrossplayNetLib.dll's export table against the
shipped one.

The game resolves nine `CrossplayNetLibSys` methods and two free functions
through its import table, and `get_netsys_object_ptr` through
`GetProcAddress`. A single missing or misspelled export is an unresolved
import and the process will not start, so this check is the gate before the
DLL is ever put next to the game.

    uv run --with pefile python crates/netsys-shim/check-exports.py

Exit 0 if every symbol the shipped DLL exports is also exported by ours with
the same machine type. Extra exports on our side are reported but allowed.
"""
import sys
import pathlib

try:
    import pefile
except ImportError:
    sys.exit("needs pefile: uv run --with pefile python <this>")

ROOT = pathlib.Path(__file__).resolve().parents[2]
SHIPPED = ROOT / "ron-bin/dll/CrossplayNetLib.dll"
OURS = ROOT / "crates/netsys-shim/target/i686-pc-windows-msvc/release/CrossplayNetLib.dll"

IMAGE_FILE_MACHINE_I386 = 0x014C


def exports(path):
    pe = pefile.PE(str(path))
    names = set()
    d = getattr(pe, "DIRECTORY_ENTRY_EXPORT", None)
    if d:
        for s in d.symbols:
            if s.name:
                names.add(s.name.decode())
    machine = pe.FILE_HEADER.Machine
    pe.close()
    return names, machine


def main():
    if not SHIPPED.exists():
        sys.exit(f"missing reference DLL: {SHIPPED}")
    if not OURS.exists():
        sys.exit(
            f"missing built DLL: {OURS}\n"
            "build it first:  cd crates/netsys-shim && XWIN_ARCH=x86 cargo xwin build --release"
        )

    want, want_machine = exports(SHIPPED)
    got, got_machine = exports(OURS)

    ok = True
    if got_machine != IMAGE_FILE_MACHINE_I386:
        print(f"FAIL machine: ours is {got_machine:#06x}, must be i386 {IMAGE_FILE_MACHINE_I386:#06x}")
        ok = False
    if want_machine != IMAGE_FILE_MACHINE_I386:
        print(f"note: reference machine {want_machine:#06x}")

    missing = sorted(want - got)
    extra = sorted(got - want)

    print(f"shipped exports: {len(want)}")
    print(f"ours:            {len(got)}")
    for n in sorted(want & got):
        print(f"  ok      {n}")
    for n in missing:
        print(f"  MISSING {n}")
        ok = False
    for n in extra:
        print(f"  extra   {n}")

    print("PASS" if ok else "FAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
