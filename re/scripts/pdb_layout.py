"""Flatten a PDB class layout into a leaf-field list: (offset, size, name, type).

Reads ron-bin/sbl/rise.pdb, whose GUID/age match the PE CodeView record of
ron-bin/riseofnations.exe exactly, so the type stream is authoritative for this build.

    uv run --quiet python re/scripts/pdb_layout.py Unit GameInfo Player
"""
import os
import sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from pdb_read import MSF
from pdb_types import TPI, LF_CLASS, LF_STRUCTURE, LF_UNION

_PDB = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                    "..", "..", "ron-bin", "sbl", "rise.pdb")
_msf = None
_tpi = None


def tpi():
    global _msf, _tpi
    if _tpi is None:
        _msf = MSF(_PDB)
        _tpi = TPI(_msf)
    return _tpi


def flatten(name, base=0, depth=0, prefix="", seen=None):
    """Return list of dicts {off,size,name,type,kind} for leaf fields of `name`."""
    t = tpi()
    if depth > 6:
        return []
    L = t.layout(name)
    if not L or L["kind"] not in ("struct", "union"):
        return []
    seen = seen or set()
    if name in seen:
        return []
    seen = seen | {name}
    out = []
    for f in L["fields"]:
        if f["k"] == "base":
            out += flatten(f["name"], base + f["off"], depth + 1, prefix, seen)
        elif f["k"] == "vfptr":
            out.append(dict(off=base + f["off"], size=4, name=prefix + "__vfptr",
                            type="void**", leaf=True))
        elif f["k"] == "member":
            off = base + f["off"]
            ty = f["type"]
            sub = None
            # expand a nested aggregate member
            if ty and not ty.endswith("*") and "[" not in ty and ty in t.by_name:
                sub = flatten(ty, off, depth + 1, prefix + f["name"] + ".", seen)
            if sub:
                out += sub
            else:
                out.append(dict(off=off, size=f["size"], name=prefix + f["name"],
                                type=ty, leaf=True))
        elif f["k"] == "vbase":
            out.append(dict(off=None, size=None, name="[vbase]" + f["name"],
                            type=f["name"], leaf=False))
    out.sort(key=lambda d: (d["off"] is None, d["off"] or 0))
    return out


def size_of(name):
    L = tpi().layout(name)
    return L["size"] if L else None


def cover(name, lo, hi):
    """Fields of `name` overlapping [lo,hi)."""
    return [f for f in flatten(name)
            if f["off"] is not None and f["size"]
            and f["off"] < hi and f["off"] + f["size"] > lo]


if __name__ == "__main__":
    for n in sys.argv[1:]:
        print("==", n, "sizeof", size_of(n))
        for f in flatten(n):
            print("   +0x%-5s %-4s %-34s %s" % (
                ("%x" % f["off"]) if f["off"] is not None else "?",
                f["size"], f["name"], f["type"]))
