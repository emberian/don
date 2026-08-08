#!/usr/bin/env python3
"""Resolve VAs in riseofnations.exe against the shipped rise.pdb.

  lookup.py <va-hex> [...]        resolve addresses (reports ALL symbols at an address:
                                  MSVC identical-COMDAT-folding means one address can carry
                                  several names, and picking one at random is how you get a
                                  3-byte `CheckSum::walk_test` reported as `...::OnPaint`)
  lookup.py --name '<regex>'      search procedure and public names

Symbol table: DON_SYMTAB or /Users/ember/dev/don/re/symtab.json (built by build_symtab.py).
"""
import bisect
import json
import os
import re
import sys

SYMTAB = os.environ.get("DON_SYMTAB", "/Users/ember/dev/don/re/symtab.json")
st = json.load(open(SYMTAB))
procs, pubs = st["procs"], st["publics"]
pva = [p["va"] for p in procs]
uva = [p["va"] for p in pubs]
MAXFN = max(p["size"] for p in procs)


def procs_at(v):
    """Every proc record whose extent covers v. Handles COMDAT-folded aliases."""
    out = []
    i = bisect.bisect_right(pva, v)
    j = bisect.bisect_left(pva, v - MAXFN)
    for k in range(j, i):
        p = procs[k]
        if p["size"] and p["va"] <= v < p["va"] + p["size"]:
            out.append(p)
    out.sort(key=lambda p: (v - p["va"], p["name"]))
    return out


def report(v):
    out = [f"== 0x{v:08x}"]
    hit = False
    seen = set()
    for p in procs_at(v):
        if (p["va"], p["name"]) in seen:
            continue
        seen.add((p["va"], p["name"]))
        out.append(f"   PROC  {p['name']}  @0x{p['va']:08x} size={p['size']} off=+{v - p['va']}")
        hit = True
    j = bisect.bisect_right(uva, v) - 1
    for k in range(max(0, j - 6), min(len(pubs), j + 7)):
        d = v - pubs[k]["va"]
        if -1 <= d <= 32:
            out.append(f"   PUB   @0x{pubs[k]['va']:08x} (v=sym+{d}) {pubs[k]['demangled']}")
            hit = True
    if not hit and j >= 0:
        out.append(f"   (nearest preceding public: {pubs[j]['demangled']} @0x{pubs[j]['va']:08x}, +{v - pubs[j]['va']})")
    elif not hit:
        out.append("   <no symbol>")
    return "\n".join(out)


if __name__ == "__main__":
    args = sys.argv[1:]
    if args and args[0] == "--name":
        rx = re.compile(args[1], re.I)
        for p in procs:
            if rx.search(p["name"]):
                print(f"0x{p['va']:08x} size={p['size']:<7} {p['name']}   [{p['type']}]")
        for p in pubs:
            if rx.search(p["demangled"]):
                print(f"0x{p['va']:08x} PUB  {p['demangled']}")
    else:
        for a in args:
            print(report(int(a, 16)))
