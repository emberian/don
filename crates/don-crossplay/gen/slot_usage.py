#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Which `ICrossPlayService` slots does retail actually dispatch?

The 58 slot names invite guessing about which ones matter. This measures it
instead, by following the service pointer through real machine code:

* in `riseofnations.exe`, from every `call dword ptr [0x00ac503c]` — the IAT
  entry for `?Service@Crossplay@@YAPAUICrossPlayService@1@XZ`;
* in `CrossplayNetLib.dll`, from every `mov rX, dword ptr [rY + 0xcc]` — the
  cached `CrossplayNetLibSys::m_crossplay` at +204.

From each seed it tracks the value forward through register moves, notes the
vptr load, and records the byte offset of every `call dword ptr [vptr + imm]`.
Volatile registers are dropped across any intervening `call`, and the scan stops
at an unconditional branch.

**Read the result as a one-sided detector.** A slot that appears IS dispatched
by retail — that is a measurement. A slot that does not appear is *not shown to
be unused*: the tracker gives up on a spilled pointer, a longer window, a
tail-merged path, or dispatch through a copy this seeding does not see.

    uv run --with capstone python3 crates/don-crossplay/gen/slot_usage.py
    uv run --with capstone python3 crates/don-crossplay/gen/slot_usage.py --json out.json
"""

from __future__ import annotations

import argparse
import bisect
import json
import re
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
EXE = REPO / "ron-bin" / "riseofnations.exe"
NETLIB = REPO / "ron-bin" / "dll" / "CrossplayNetLib.dll"
PROXY_PDB = REPO / "ron-bin" / "sbl" / "CrossplayProxy.pdb"
NETLIB_PDB = REPO / "ron-bin" / "sbl" / "CrossplayNetLib.pdb"
RISE_SYMBOLS = REPO / "schema" / "symbols.json"
PDB_EXTRACT = REPO / "tools" / "pdb-extract" / "Cargo.toml"

SERVICE_IAT = 0x00AC503C  # riseofnations.exe import of Crossplay::Service()
M_CROSSPLAY = 0xCC  # CrossplayNetLibSys::m_crossplay, +204
WINDOW_INSTRUCTIONS = 200
VOLATILE = {"eax", "ecx", "edx"}


class Pe:
    def __init__(self, path: Path):
        d = path.read_bytes()
        e = struct.unpack_from("<I", d, 0x3C)[0]
        coff = e + 4
        _, nsec, _, _, _, optsz, _ = struct.unpack_from("<HHIIIHH", d, coff)
        opt = coff + 20
        self.data = d
        self.image_base = struct.unpack_from("<I", d, opt + 28)[0]
        self.sections = []
        for i in range(nsec):
            o = opt + optsz + i * 40
            name = d[o : o + 8].rstrip(b"\0").decode()
            vs, va, rs, rp = struct.unpack_from("<IIII", d, o + 8)
            self.sections.append((name, va, vs, rp, rs))
        self.text = next(s for s in self.sections if s[0] == ".text")

    def code(self, rva: int, n: int) -> bytes:
        _, va, _, rp, _ = self.text
        off = rp + (rva - va)
        return self.data[off : off + n]


def owners(symbols: dict):
    funcs = sorted(
        (f["rva"], f.get("size") or 0, f["name"]) for f in symbols["functions"] if f.get("size")
    )
    starts = [f[0] for f in funcs]

    def owner(rva: int) -> str:
        i = bisect.bisect_right(starts, rva) - 1
        while i >= 0:
            s, n, name = funcs[i]
            if s <= rva < s + n:
                return name
            i -= 1
        return "<unknown>"

    return funcs, owner


def track(md, instructions, seed_reg: str) -> list[int]:
    """Follow `seed_reg` (an ICrossPlayService*) and return dispatched offsets."""
    svc = {seed_reg}
    vptr: set[str] = set()
    found: list[int] = []
    for ins in instructions:
        m, o = ins.mnemonic, ins.op_str
        if m == "call":
            if o.startswith("dword ptr ["):
                inner = o[len("dword ptr [") : -1]
                if "+" in inner:
                    r, imm = (x.strip() for x in inner.split("+", 1))
                    if r in vptr and imm.startswith("0x"):
                        found.append(int(imm, 16))
                elif inner in vptr:
                    found.append(0)
            svc -= VOLATILE
            vptr -= VOLATILE
            continue
        if m in ("jmp", "ret"):
            break
        if m == "mov":
            parts = [x.strip() for x in o.split(",", 1)]
            if len(parts) == 2:
                dst, src = parts
                if src in svc and dst.isalpha():
                    svc.add(dst)
                    continue
                if src.startswith("dword ptr [") and src[len("dword ptr [") : -1].strip() in svc:
                    if dst.isalpha():
                        vptr.add(dst)
                        continue
                if dst.isalpha():
                    svc.discard(dst)
                    vptr.discard(dst)
        elif m in ("lea", "add", "sub", "xor", "pop", "movzx", "movsx"):
            dst = o.split(",")[0].strip()
            if dst.isalpha():
                svc.discard(dst)
                vptr.discard(dst)
        if not svc and not vptr:
            break
    return found


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--json", default=None)
    ap.add_argument("--work", default=None)
    args = ap.parse_args()

    try:
        import capstone
    except ImportError:
        print("needs capstone: uv run --with capstone python3 ...", file=sys.stderr)
        return 2

    for p in (EXE, NETLIB, PROXY_PDB, NETLIB_PDB, RISE_SYMBOLS):
        if not p.exists():
            print(f"missing input: {p}", file=sys.stderr)
            return 2

    work = Path(args.work) if args.work else Path(tempfile.mkdtemp(prefix="don-crossplay-slots-"))
    work.mkdir(parents=True, exist_ok=True)
    for pdb, base in ((PROXY_PDB, 0x10000000), (NETLIB_PDB, 0x10000000)):
        sym = work / f"{pdb.stem}-symbols.json"
        typ = work / f"{pdb.stem}-types.json"
        if not (sym.exists() and typ.exists()):
            subprocess.run(
                ["cargo", "run", "--release", "--quiet", "--manifest-path", str(PDB_EXTRACT),
                 "--", str(pdb), hex(base), str(sym), str(typ)],
                check=True, cwd=REPO,
            )

    proxy_types = json.loads((work / "CrossplayProxy-types.json").read_text())["classes"]
    slot_names = {
        m["vtable_offset"]: m["name"]
        for m in proxy_types["CrossplayProxy::ICrossPlayService"]["methods"]
    }

    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    usage: dict[int, dict[str, set[str]]] = {}

    def record(offset: int, image: str, fn: str) -> None:
        usage.setdefault(offset, {}).setdefault(image, set()).add(fn)

    # --- riseofnations.exe: seed at every call to the Service() import -----
    exe = Pe(EXE)
    rise_symbols = json.loads(RISE_SYMBOLS.read_text())
    _, rise_owner = owners(rise_symbols)
    _, tva, tvs, trp, trs = exe.text
    blob = exe.data[trp : trp + trs]
    pattern = b"\xff\x15" + struct.pack("<I", SERVICE_IAT)
    sites, at = [], 0
    while True:
        k = blob.find(pattern, at)
        if k < 0:
            break
        sites.append(tva + k)
        at = k + 1
    for site in sites:
        after = site + 6
        window = list(md.disasm(exe.code(after, 1500), exe.image_base + after))
        for off in track(md, window[:WINDOW_INSTRUCTIONS], "eax"):
            record(off, "riseofnations.exe", rise_owner(site))

    # --- CrossplayNetLib.dll: seed at every load of m_crossplay -----------
    lib = Pe(NETLIB)
    lib_symbols = json.loads((work / "CrossplayNetLib-symbols.json").read_text())
    lib_funcs, _ = owners(lib_symbols)
    _, lva, lvs, _, _ = lib.text
    seed_re = re.compile(r"^(\w+), dword ptr \[(\w+) \+ 0x%x\]$" % M_CROSSPLAY)
    seeds = 0
    for rva, size, fname in lib_funcs:
        if size < 6 or not lva <= rva < lva + lvs:
            continue
        window = list(md.disasm(lib.code(rva, size), lib.image_base + rva))
        for i, ins in enumerate(window):
            if ins.mnemonic != "mov":
                continue
            m = seed_re.match(ins.op_str)
            if not m:
                continue
            seeds += 1
            for off in track(md, window[i + 1 : i + 1 + WINDOW_INSTRUCTIONS], m.group(1)):
                record(off, "CrossplayNetLib.dll", fname)

    dispatched = sorted(usage)
    silent = [o for o in sorted(slot_names) if o not in usage]

    print(
        f"seeds: {len(sites)} Service() call sites in riseofnations.exe, "
        f"{seeds} m_crossplay loads in CrossplayNetLib.dll; "
        f"window {WINDOW_INSTRUCTIONS} instructions"
    )
    print(f"{len(dispatched)}/{len(slot_names)} slots have a tracked dispatch\n")
    print("=== dispatched (measured) ===")
    for o in dispatched:
        for image, fns in sorted(usage[o].items()):
            print(f"  +{o:<4} slot {o // 4:<2} {slot_names[o]:<34} {image:<20} {', '.join(sorted(fns))}")
    print("\n=== no tracked dispatch (NOT evidence of being unused) ===")
    for o in silent:
        print(f"  +{o:<4} slot {o // 4:<2} {slot_names[o]}")

    if args.json:
        Path(args.json).write_text(
            json.dumps(
                {
                    "schema": "don.crossplay-slot-usage.v1",
                    "method": {
                        "service_iat": hex(SERVICE_IAT),
                        "m_crossplay_offset": M_CROSSPLAY,
                        "window_instructions": WINDOW_INSTRUCTIONS,
                        "note": "one-sided: presence is measured dispatch, absence proves nothing",
                    },
                    "slots": {
                        str(o): {
                            "name": slot_names[o],
                            "dispatched_by": {k: sorted(v) for k, v in usage.get(o, {}).items()},
                        }
                        for o in sorted(slot_names)
                    },
                },
                indent=1,
            )
        )
        print(f"\nwrote {args.json}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
