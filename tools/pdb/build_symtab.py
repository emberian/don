#!/usr/bin/env python3
"""Build a VA -> symbol index from the shipped rise.pdb dumps.

Inputs (produced by llvm-pdbutil):
  symbols.txt  <- llvm-pdbutil dump --symbols rise.pdb
  publics.txt  <- llvm-pdbutil dump --publics rise.pdb

Section 1 (.text) has RVA 0x1000, image base 0x400000, so
VA = 0x401000 + offset for `addr = 0001:<offset>`.
Section n uses the PE section table RVAs below.
"""
import json
import re
import sys

# from `llvm-pdbutil dump --section-headers rise.pdb`
SECT_RVA = {1: 0x1000, 2: 0x6C5000, 3: 0x806000, 4: 0xAE2000, 5: 0xAE9000}
IMAGE_BASE = 0x400000


def va(sect: int, off: int):
    if sect not in SECT_RVA:
        return None
    return IMAGE_BASE + SECT_RVA[sect] + off


PROC_RE = re.compile(r"^\s*\d+ \| (S_GPROC32|S_LPROC32|S_THUNK32|S_GPROC32_ID|S_LPROC32_ID) \[size = \d+\] `(.*)`$")
ADDR_RE = re.compile(r"addr = ([0-9A-Fa-f]{4}):(\d+)")
CODESIZE_RE = re.compile(r"code size = (\d+)")
TYPE_RE = re.compile(r"^\s*type = `0x[0-9A-Fa-f]+ \((.*)\)`")
PUB_RE = re.compile(r"^\s*\d+ \| S_PUB32 \[size = \d+\] `(.*)`$")
PUBADDR_RE = re.compile(r"flags = (.*), addr = ([0-9A-Fa-f]{4}):(\d+)")


def parse_symbols(path):
    procs = []
    lines = open(path, errors="replace").read().splitlines()
    i = 0
    n = len(lines)
    while i < n:
        m = PROC_RE.match(lines[i])
        if m:
            kind, name = m.group(1), m.group(2)
            addr = None
            size = None
            tstr = None
            for j in range(i + 1, min(i + 4, n)):
                a = ADDR_RE.search(lines[j])
                if a and addr is None:
                    addr = (int(a.group(1), 16), int(a.group(2)))
                c = CODESIZE_RE.search(lines[j])
                if c and size is None:
                    size = int(c.group(1))
                t = TYPE_RE.match(lines[j])
                if t and tstr is None:
                    tstr = t.group(1)
            if addr:
                v = va(*addr)
                if v is not None:
                    procs.append({"va": v, "size": size or 0, "name": name,
                                  "kind": kind, "type": tstr})
        i += 1
    return procs


def parse_publics(path):
    pubs = []
    lines = open(path, errors="replace").read().splitlines()
    i = 0
    n = len(lines)
    while i < n:
        m = PUB_RE.match(lines[i])
        if m and i + 1 < n:
            a = PUBADDR_RE.search(lines[i + 1])
            if a:
                v = va(int(a.group(2), 16), int(a.group(3)))
                if v is not None:
                    pubs.append({"va": v, "name": m.group(1),
                                 "flags": a.group(1),
                                 "sect": int(a.group(2), 16)})
        i += 1
    return pubs


if __name__ == "__main__":
    sp = sys.argv[1]
    procs = parse_symbols(f"{sp}/symbols.txt")
    pubs = parse_publics(f"{sp}/publics.txt")
    procs.sort(key=lambda p: p["va"])
    pubs.sort(key=lambda p: p["va"])
    json.dump({"procs": procs, "publics": pubs}, open(f"{sp}/symtab.json", "w"))
    print(f"procs={len(procs)} publics={len(pubs)}")
