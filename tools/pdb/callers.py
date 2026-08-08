#!/usr/bin/env python3
"""List callers of a function VA by scanning .text for E8/E9 rel32 targets.
Usage: callers.py <target-va-hex> [...]"""
import bisect, json, os, struct, sys
import pefile

EXE = "/Users/ember/dev/don/ron-bin/riseofnations.exe"
st = json.load(open(os.environ.get("DON_SYMTAB", "/Users/ember/dev/don/re/symtab.json")))
procs = st["procs"]; pva = [p["va"] for p in procs]
pe = pefile.PE(EXE, fast_load=True)
IB = pe.OPTIONAL_HEADER.ImageBase
txt = [s for s in pe.sections if s.Name.rstrip(b"\x00") == b".text"][0]
lo = IB + txt.VirtualAddress; data = txt.get_data()


def owner(v):
    i = bisect.bisect_right(pva, v) - 1
    if i >= 0:
        p = procs[i]
        if p["size"] and v < p["va"] + p["size"]:
            return p["name"], v - p["va"]
    return None, 0


for a in sys.argv[1:]:
    t = int(a, 16)
    from collections import Counter
    c = Counter()
    n = 0
    for i in range(len(data) - 5):
        if data[i] in (0xE8, 0xE9):
            rel = struct.unpack_from("<i", data, i + 1)[0]
            if lo + i + 5 + rel == t:
                nm, off = owner(lo + i)
                c[nm or "<unknown>"] += 1
                n += 1
    print(f"== callers of 0x{t:08x} ({owner(t)[0]}): {n}")
    for nm, k in c.most_common(40):
        print(f"   {k:5} {nm}")
