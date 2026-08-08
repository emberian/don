#!/usr/bin/env python3
"""Minimal CodeView TPI reader for MSF 7.00 PDBs: struct/class/union/enum
layouts with member names, offsets and types.

  python3 pdb_types.py <pdb> --struct 'MoveToCommand'
  python3 pdb_types.py <pdb> --list 'Command$'
"""
import argparse
import struct
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from pdb_read import MSF  # noqa: E402

LF_MODIFIER = 0x1001
LF_POINTER = 0x1002
LF_PROCEDURE = 0x1008
LF_MFUNCTION = 0x1009
LF_ARGLIST = 0x1201
LF_FIELDLIST = 0x1203
LF_BITFIELD = 0x1205
LF_BCLASS = 0x1400
LF_VFUNCTAB = 0x1409
LF_INDEX = 0x1404
LF_VBCLASS = 0x1401
LF_IVBCLASS = 0x1402
LF_ENUMERATE = 0x1502
LF_ARRAY = 0x1503
LF_CLASS = 0x1504
LF_STRUCTURE = 0x1505
LF_UNION = 0x1506
LF_ENUM = 0x1507
LF_MEMBER = 0x150D
LF_STMEMBER = 0x150E
LF_METHOD = 0x150F
LF_NESTTYPE = 0x1510
LF_VFUNCOFF = 0x1513
LF_ONEMETHOD = 0x1511

BASIC = {
    0x0000: "<notype>", 0x0003: "void", 0x0008: "HRESULT",
    0x0010: "signed char", 0x0011: "short", 0x0012: "long", 0x0013: "__int64",
    0x0020: "unsigned char", 0x0021: "unsigned short", 0x0022: "unsigned long",
    0x0023: "unsigned __int64",
    0x0030: "bool", 0x0040: "float", 0x0041: "double",
    0x0068: "__int8", 0x0069: "unsigned __int8",
    0x0070: "char", 0x0071: "wchar_t",
    0x0072: "__int16", 0x0073: "unsigned __int16",
    0x0074: "int", 0x0075: "unsigned int",
    0x0076: "__int64", 0x0077: "unsigned __int64",
}
BASIC_SIZE = {
    0x0003: 0, 0x0010: 1, 0x0011: 2, 0x0012: 4, 0x0013: 8,
    0x0020: 1, 0x0021: 2, 0x0022: 4, 0x0023: 8, 0x0030: 1,
    0x0040: 4, 0x0041: 8, 0x0068: 1, 0x0069: 1, 0x0070: 1, 0x0071: 2,
    0x0072: 2, 0x0073: 2, 0x0074: 4, 0x0075: 4, 0x0076: 8, 0x0077: 8,
}


def numeric(b, o):
    (v,) = struct.unpack_from("<H", b, o)
    if v < 0x8000:
        return v, o + 2
    if v == 0x8000:
        return struct.unpack_from("<b", b, o + 2)[0], o + 3
    if v == 0x8001:
        return struct.unpack_from("<h", b, o + 2)[0], o + 4
    if v == 0x8002:
        return struct.unpack_from("<H", b, o + 2)[0], o + 4
    if v == 0x8003:
        return struct.unpack_from("<i", b, o + 2)[0], o + 6
    if v == 0x8004:
        return struct.unpack_from("<I", b, o + 2)[0], o + 6
    if v == 0x8009:
        return struct.unpack_from("<q", b, o + 2)[0], o + 10
    if v == 0x800A:
        return struct.unpack_from("<Q", b, o + 2)[0], o + 10
    raise ValueError("numeric leaf %#x" % v)


def sz(b, o):
    e = b.index(b"\x00", o)
    return b[o:e].decode("utf-8", "replace"), e + 1


class TPI:
    def __init__(self, msf, stream=2):
        raw = msf.stream(stream)
        (ver, hdr, self.ti_begin, self.ti_end, nbytes) = struct.unpack_from("<5I", raw, 0)
        body = raw[hdr:hdr + nbytes]
        self.recs = {}
        o = 0
        ti = self.ti_begin
        while o + 4 <= len(body):
            (ln, kind) = struct.unpack_from("<HH", body, o)
            self.recs[ti] = (kind, body[o + 4:o + 2 + ln])
            o += 2 + ln
            ti += 1
        self.by_name = {}
        for t, (kind, d) in self.recs.items():
            if kind in (LF_CLASS, LF_STRUCTURE, LF_UNION, LF_ENUM):
                n = self.tag_name(kind, d)
                if n and not self.is_fwdref(kind, d):
                    self.by_name.setdefault(n, t)

    def is_fwdref(self, kind, d):
        if kind in (LF_CLASS, LF_STRUCTURE):
            prop = struct.unpack_from("<H", d, 2)[0]
        elif kind == LF_UNION:
            prop = struct.unpack_from("<H", d, 2)[0]
        elif kind == LF_ENUM:
            prop = struct.unpack_from("<H", d, 2)[0]
        else:
            return False
        return bool(prop & 0x80)

    def tag_name(self, kind, d):
        try:
            if kind in (LF_CLASS, LF_STRUCTURE):
                _n, _p, _f, _dl, _vs = struct.unpack_from("<HHIII", d, 0)
                _v, o = numeric(d, 16)
                return sz(d, o)[0]
            if kind == LF_UNION:
                _n, _p, _f = struct.unpack_from("<HHI", d, 0)
                _v, o = numeric(d, 8)
                return sz(d, o)[0]
            if kind == LF_ENUM:
                _n, _p, _ut, _f = struct.unpack_from("<HHII", d, 0)
                return sz(d, 12)[0]
        except Exception:
            return None
        return None

    # ---- type naming ----
    def tname(self, ti, depth=0):
        if ti < 0x1000:
            base = BASIC.get(ti & 0xFF, "t%#x" % ti)
            mode = (ti >> 8) & 0xF
            return base + ("*" if mode in (1, 2, 3, 4, 5, 6) else "")
        if ti not in self.recs or depth > 8:
            return "T%#x" % ti
        kind, d = self.recs[ti]
        if kind in (LF_CLASS, LF_STRUCTURE, LF_UNION, LF_ENUM):
            return self.tag_name(kind, d) or "T%#x" % ti
        if kind == LF_POINTER:
            (ut, attr) = struct.unpack_from("<II", d, 0)
            return self.tname(ut, depth + 1) + "*"
        if kind == LF_MODIFIER:
            (ut, m) = struct.unpack_from("<IH", d, 0)
            return ("const " if m & 1 else "") + self.tname(ut, depth + 1)
        if kind == LF_ARRAY:
            (et, it) = struct.unpack_from("<II", d, 0)
            n, _o = numeric(d, 8)
            es = self.tsize(et) or 1
            return "%s[%d]" % (self.tname(et, depth + 1), n // es if es else n)
        if kind == LF_BITFIELD:
            (ut, bits, pos) = struct.unpack_from("<IBB", d, 0)
            return "%s:%d@%d" % (self.tname(ut, depth + 1), bits, pos)
        if kind == LF_PROCEDURE or kind == LF_MFUNCTION:
            return "fn"
        return "T%#x" % ti

    def tsize(self, ti, depth=0):
        if ti < 0x1000:
            mode = (ti >> 8) & 0xF
            if mode:
                return 4
            return BASIC_SIZE.get(ti & 0xFF)
        if ti not in self.recs or depth > 8:
            return None
        kind, d = self.recs[ti]
        if kind == LF_POINTER:
            return 4
        if kind == LF_MODIFIER:
            return self.tsize(struct.unpack_from("<I", d, 0)[0], depth + 1)
        if kind in (LF_CLASS, LF_STRUCTURE):
            _n, _p, _f, _dl, _vs = struct.unpack_from("<HHIII", d, 0)
            return numeric(d, 16)[0]
        if kind == LF_UNION:
            return numeric(d, 8)[0]
        if kind == LF_ARRAY:
            return numeric(d, 8)[0]
        if kind == LF_ENUM:
            return self.tsize(struct.unpack_from("<I", d, 4)[0], depth + 1)
        if kind == LF_BITFIELD:
            return self.tsize(struct.unpack_from("<I", d, 0)[0], depth + 1)
        return None

    def fields(self, fl_ti):
        """Flatten an LF_FIELDLIST into a list of dicts."""
        out = []
        if fl_ti not in self.recs:
            return out
        kind, d = self.recs[fl_ti]
        if kind != LF_FIELDLIST:
            return out
        o = 0
        while o + 2 <= len(d):
            (lk,) = struct.unpack_from("<H", d, o)
            o += 2
            if lk == LF_MEMBER:
                (attr, ti) = struct.unpack_from("<HI", d, o)
                off, p = numeric(d, o + 6)
                name, p = sz(d, p)
                out.append(dict(k="member", name=name, off=off, ti=ti,
                                type=self.tname(ti), size=self.tsize(ti)))
                o = p
            elif lk == LF_STMEMBER:
                (attr, ti) = struct.unpack_from("<HI", d, o)
                name, p = sz(d, o + 6)
                out.append(dict(k="static", name=name, ti=ti, type=self.tname(ti)))
                o = p
            elif lk == LF_BCLASS:
                (attr, ti) = struct.unpack_from("<HI", d, o)
                off, p = numeric(d, o + 6)
                out.append(dict(k="base", name=self.tname(ti), off=off, ti=ti,
                                type=self.tname(ti), size=self.tsize(ti)))
                o = p
            elif lk in (LF_VBCLASS, LF_IVBCLASS):
                (attr, ti, vbp) = struct.unpack_from("<HII", d, o)
                v1, p = numeric(d, o + 10)
                v2, p = numeric(d, p)
                out.append(dict(k="vbase", name=self.tname(ti), ti=ti))
                o = p
            elif lk == LF_VFUNCTAB:
                (pad, ti) = struct.unpack_from("<HI", d, o)
                out.append(dict(k="vfptr", name="__vfptr", off=0, ti=ti,
                                type="void**", size=4))
                o += 6
            elif lk == LF_ENUMERATE:
                (attr,) = struct.unpack_from("<H", d, o)
                val, p = numeric(d, o + 2)
                name, p = sz(d, p)
                out.append(dict(k="enum", name=name, value=val))
                o = p
            elif lk == LF_NESTTYPE:
                (pad, ti) = struct.unpack_from("<HI", d, o)
                name, p = sz(d, o + 6)
                o = p
            elif lk == LF_ONEMETHOD:
                (attr, ti) = struct.unpack_from("<HI", d, o)
                p = o + 6
                if ((attr >> 2) & 7) in (4, 6):   # intro virtual
                    p += 4
                name, p = sz(d, p)
                o = p
            elif lk == LF_METHOD:
                (cnt, ml) = struct.unpack_from("<HI", d, o)
                name, p = sz(d, o + 6)
                o = p
            elif lk == LF_INDEX:
                (pad, ti) = struct.unpack_from("<HI", d, o)
                out.extend(self.fields(ti))
                o += 6
            elif lk == LF_VFUNCOFF:
                o += 10
            else:
                break
            while o < len(d) and d[o] >= 0xF0:   # LF_PAD
                o += 1
        return out

    def layout(self, name):
        ti = self.by_name.get(name)
        if ti is None:
            return None
        kind, d = self.recs[ti]
        if kind in (LF_CLASS, LF_STRUCTURE):
            (nfields, prop, fl, dl, vs) = struct.unpack_from("<HHIII", d, 0)
            size, o = numeric(d, 16)
            return dict(name=name, ti=ti, size=size, kind="struct",
                        fields=self.fields(fl))
        if kind == LF_UNION:
            (nfields, prop, fl) = struct.unpack_from("<HHI", d, 0)
            size, o = numeric(d, 8)
            return dict(name=name, ti=ti, size=size, kind="union",
                        fields=self.fields(fl))
        if kind == LF_ENUM:
            (nfields, prop, ut, fl) = struct.unpack_from("<HHII", d, 0)
            return dict(name=name, ti=ti, size=self.tsize(ut), kind="enum",
                        fields=self.fields(fl))
        return None


def show(t, name):
    L = t.layout(name)
    if not L:
        print("!! not found:", name)
        return
    print("%s %s  // sizeof = %d (0x%x)" % (L["kind"], L["name"], L["size"], L["size"]))
    for f in L["fields"]:
        if f["k"] == "enum":
            print("    %-34s = %s" % (f["name"], f["value"]))
        elif f["k"] in ("member", "base", "vfptr"):
            print("    +0x%02x  %-28s %-22s %s"
                  % (f["off"], f["type"], f["name"],
                     "" if f["k"] == "member" else "[%s]" % f["k"]))
        else:
            print("    %-6s %s" % (f["k"], f["name"]))
    print()


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("pdb")
    ap.add_argument("--struct", nargs="*", default=[])
    ap.add_argument("--list", default=None)
    ap.add_argument("--all-matching", default=None)
    a = ap.parse_args()
    m = MSF(a.pdb)
    t = TPI(m)
    print("# %d type records, %d named tags" % (len(t.recs), len(t.by_name)),
          file=sys.stderr)
    if a.list:
        import re
        rx = re.compile(a.list)
        for n in sorted(t.by_name):
            if rx.search(n):
                print(n)
    for s in a.struct:
        show(t, s)
    if a.all_matching:
        import re
        rx = re.compile(a.all_matching)
        for n in sorted(t.by_name):
            if rx.search(n):
                show(t, n)
