#!/usr/bin/env python3
"""Recover the BHS script-function registration table from riseofnations.exe.

The engine builds its entire script API in one function, FUN_009C7570
(52,300 bytes, `skipped_large` in re/decomp-all/MANIFEST.jsonl). Each entry is

    push  <arity>
    push  <impl VA>
    push  <name, UTF-16 in .rdata>
    push  <type tag>
    mov   ecx, <registry>
    call  0x009D4DA0          ; ScriptFuncSet::AddFunc -> ScriptFunc* in eax
    push  ecx / push 0 / push 0
    push  <param name, UTF-16>
    push  <type tag>
    mov   ecx, eax
    call  0x009D4F20          ; ScriptFunc::AddParam

This walks the function linearly with capstone, matches those two call targets,
and reads the UTF-16 names out of the mapped image.

Usage (from the repo root):
    cd ron-bin && uv run --quiet --with capstone --with pefile python \
        ../crates/don-ai/tools/dump_script_funcs.py \
        > ../crates/don-ai/data/script-functions.json

Observed type tags: 0x00057BAD = int, 0x00168174 = string. Four more appear
rarely (0x00084048, 0x0012F35F, 0x0020D693, 0x0139FD8D, 0x1D0655F3) and are
left unresolved.
"""

import json
import struct
import sys

import pefile
from capstone import CS_ARCH_X86, CS_MODE_32, Cs

EXE = "riseofnations.exe"
REG_FUNC_START = 0x009C7570
REG_FUNC_SIZE = 52300
ADD_FUNC = 0x009D4DA0
ADD_PARAM = 0x009D4F20

TAGS = {0x57BAD: "int", 0x168174: "string"}


def main() -> None:
    pe = pefile.PE(EXE)
    ib = pe.OPTIONAL_HEADER.ImageBase
    secs = [(ib + s.VirtualAddress, s.Misc_VirtualSize, s.get_data()) for s in pe.sections]

    def read(va: int, n: int) -> bytes:
        for base, size, data in secs:
            if base <= va < base + size:
                return data[va - base : va - base + n]
        return b""

    def wstr(va: int, maxn: int = 96) -> str:
        out = []
        for i in range(maxn):
            c = read(va + 2 * i, 2)
            if len(c) < 2:
                break
            v = struct.unpack("<H", c)[0]
            if v == 0:
                break
            out.append(chr(v))
        return "".join(out)

    text = next(s for s in pe.sections if s.Name.rstrip(b"\x00") == b".text")
    base = ib + text.VirtualAddress
    data = text.get_data()

    md = Cs(CS_ARCH_X86, CS_MODE_32)
    lo = REG_FUNC_START - base
    hi = lo + REG_FUNC_SIZE

    recs = []
    cur = None
    pending: list[str] = []
    for ins in md.disasm(data[lo:hi], REG_FUNC_START):
        if ins.mnemonic == "push":
            pending.append(ins.op_str)
            continue
        if ins.mnemonic == "call":
            try:
                target = int(ins.op_str, 16)
            except ValueError:
                target = None
            if target == ADD_FUNC and len(pending) >= 4:
                try:
                    arity, impl, name, tag = (int(x, 16) for x in pending[-4:])
                except ValueError:
                    pending = []
                    continue
                cur = {
                    "name": wstr(name),
                    "impl": "0x%08x" % impl,
                    "arity": arity,
                    "ret_tag": "0x%x" % tag,
                    "ret_type": TAGS.get(tag, "0x%x" % tag),
                    "site": "0x%08x" % ins.address,
                    "params": [],
                }
                recs.append(cur)
            elif target == ADD_PARAM and cur is not None and len(pending) >= 2:
                try:
                    name, tag = (int(x, 16) for x in pending[-2:])
                except ValueError:
                    pending = []
                    continue
                cur["params"].append(
                    {"name": wstr(name), "tag": "0x%x" % tag, "type": TAGS.get(tag, "0x%x" % tag)}
                )
            pending = []
            continue
        # Any other instruction does not clear the push window; the registry
        # object is reloaded into ecx between the pushes and the call.

    json.dump(recs, sys.stdout, indent=1)
    sys.stdout.write("\n")
    print(f"# {len(recs)} script functions", file=sys.stderr)


if __name__ == "__main__":
    main()
