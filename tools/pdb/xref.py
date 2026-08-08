#!/usr/bin/env python3
"""Find immediate references to a VA (or a UTF-16LE string's VA) inside .text,
and name each containing function from the shipped PDB.

Usage:
  xref.py --addr 0x00C06184
  xref.py --str flank_bonus
"""
import bisect, json, os, struct, sys
import pefile

EXE = "/Users/ember/dev/don/ron-bin/riseofnations.exe"
SYMTAB = os.environ.get("DON_SYMTAB", "/Users/ember/dev/don/re/symtab.json")

pe = pefile.PE(EXE, fast_load=True)
IB = pe.OPTIONAL_HEADER.ImageBase
secs = [(s.Name.rstrip(b"\x00").decode(), IB + s.VirtualAddress,
         IB + s.VirtualAddress + s.Misc_VirtualSize, s.get_data()) for s in pe.sections]
text = [s for s in secs if s[0] == ".text"][0]

st = json.load(open(SYMTAB))
procs = st["procs"]
pva = [p["va"] for p in procs]


def owner(v):
    i = bisect.bisect_right(pva, v) - 1
    if i >= 0:
        p = procs[i]
        if p["size"] and v < p["va"] + p["size"]:
            return p["name"], v - p["va"]
    return None, 0


def find_str_vas(s):
    pat = s.encode("utf-16-le")
    out = []
    for name, lo, hi, data in secs:
        if name == ".text":
            continue
        off = 0
        while True:
            i = data.find(pat, off)
            if i < 0:
                break
            # require null-terminated / word aligned start
            out.append((name, lo + i))
            off = i + 2
    return out


def scan(target):
    pat = struct.pack("<I", target)
    hits = []
    _, lo, hi, data = text
    off = 0
    while True:
        i = data.find(pat, off)
        if i < 0:
            break
        hits.append(lo + i)
        off = i + 1
    return hits


if __name__ == "__main__":
    mode, arg = sys.argv[1], sys.argv[2]
    targets = []
    if mode == "--str":
        for sec, va in find_str_vas(arg):
            print(f"string '{arg}' (utf-16le) at 0x{va:08x} in {sec}")
            targets.append(va)
    else:
        targets = [int(arg, 16)]
    from collections import Counter
    c = Counter()
    for t in targets:
        for h in scan(t):
            n, o = owner(h)
            c[n or "<unknown>"] += 1
    for name, n in c.most_common():
        print(f"  {n:5}  {name}")
    print(f"  total refs: {sum(c.values())}")
