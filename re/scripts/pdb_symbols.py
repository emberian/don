#!/usr/bin/env python3
"""Resolve PDB public symbols to absolute VAs for a given image base, and
optionally look up specific VAs or grep names.

  python3 pdb_symbols.py <pdb> --base 0x400000 --va 0094a700 0094c500
  python3 pdb_symbols.py <pdb> --base 0x400000 --grep 'CommandPackage'
  python3 pdb_symbols.py <pdb> --base 0x400000 --dump out.tsv
"""
import argparse
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from pdb_read import MSF, dbi_header, publics, sections, pdb_info  # noqa: E402


def load(path, base):
    m = MSF(path)
    d = dbi_header(m)
    secs = sections(m, d)
    out = []
    for seg, off, flags, name in publics(m, d):
        if 1 <= seg <= len(secs):
            out.append((base + secs[seg - 1]["vaddr"] + off, name, secs[seg - 1]["name"]))
    out.sort()
    return m, secs, out


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("pdb")
    ap.add_argument("--base", default="0x400000")
    ap.add_argument("--va", nargs="*", default=[])
    ap.add_argument("--grep", default=None)
    ap.add_argument("--dump", default=None)
    ap.add_argument("--section", default=None)
    a = ap.parse_args()
    base = int(a.base, 16)
    m, secs, syms = load(a.pdb, base)
    print("# %s  guid=%s age=%d  %d symbols"
          % (a.pdb, pdb_info(m)["guid"], pdb_info(m)["age"], len(syms)), file=sys.stderr)
    if a.dump:
        with open(a.dump, "w") as f:
            for va, name, sec in syms:
                f.write("%08x\t%s\t%s\n" % (va, sec, name))
        print("wrote", a.dump, file=sys.stderr)
    for v in a.va:
        t = int(v, 16)
        # exact, else nearest preceding
        lo, hi = 0, len(syms)
        while lo < hi:
            mid = (lo + hi) // 2
            if syms[mid][0] <= t:
                lo = mid + 1
            else:
                hi = mid
        exact = [s for s in syms if s[0] == t]
        if exact:
            for e in exact:
                print("%08x  EXACT  %s" % (e[0], e[1]))
        elif lo:
            e = syms[lo - 1]
            print("%08x  (+%d from %08x)  %s" % (t, t - e[0], e[0], e[1]))
    if a.grep:
        import re
        rx = re.compile(a.grep)
        for va, name, sec in syms:
            if rx.search(name):
                if a.section and sec != a.section:
                    continue
                print("%08x %-8s %s" % (va, sec, name))
