#!/usr/bin/env python3
"""Minimal MSF/PDB 7.00 reader: stream directory, PDB info stream (GUID/age),
DBI header, and public/global symbol enumeration (S_PUB32 / S_GPROC32 / S_LPROC32).

Written for /Users/ember/dev/don. No external dependencies.
Ground truth: the shipped .pdb files under ron-bin/sbl/.
"""
import struct
import sys
import uuid


class MSF:
    def __init__(self, path):
        self.buf = open(path, "rb").read()
        b = self.buf
        magic = b[:32]
        assert magic.startswith(b"Microsoft C/C++ MSF 7.00"), magic[:24]
        (self.page_size, self.free_map, self.num_pages,
         self.dir_bytes, _res, dir_map_page) = struct.unpack_from("<6I", b, 32)
        npages = lambda n: (n + self.page_size - 1) // self.page_size
        # BlockMapAddr names a block holding the directory's own block numbers
        ndir = npages(self.dir_bytes)
        dir_pages = struct.unpack_from("<%dI" % ndir, b, dir_map_page * self.page_size)
        d = b"".join(self.page(p) for p in dir_pages)[: self.dir_bytes]
        # stream directory: u32 count, u32 sizes[count], then page lists
        (count,) = struct.unpack_from("<I", d, 0)
        sizes = struct.unpack_from("<%dI" % count, d, 4)
        off = 4 + 4 * count
        self.streams = []
        for s in sizes:
            if s == 0xFFFFFFFF:
                s = 0
            n = npages(s)
            pages = struct.unpack_from("<%dI" % n, d, off)
            off += 4 * n
            self.streams.append((s, pages))

    def page(self, p):
        return self.buf[p * self.page_size:(p + 1) * self.page_size]

    def stream(self, i):
        size, pages = self.streams[i]
        return b"".join(self.page(p) for p in pages)[:size]


def pdb_info(m):
    s = m.stream(1)
    ver, sig, age = struct.unpack_from("<III", s, 0)
    g = uuid.UUID(bytes_le=s[12:28])
    return dict(version=ver, signature=sig, age=age, guid=str(g).upper())


def dbi_header(m):
    s = m.stream(3)
    if len(s) < 64:
        return None
    (magic, ver, age, gs, vers, ps, pdbdll_ver, sym, rbld,
     modinfo_size, secontr_size, secmap_size, filinfo_size,
     tsmap_size, mfcidx, dbghdr_size, ecinfo_size, flags, mach,
     pad) = struct.unpack_from("<iIIHHHHHHiiiiiIiiHHI", s, 0)
    return dict(raw=s, ver=ver, age=age, gsym=gs, psym=ps, symrec=sym,
                machine=mach, modinfo_size=modinfo_size,
                secontr_size=secontr_size, secmap_size=secmap_size,
                filinfo_size=filinfo_size, tsmap_size=tsmap_size,
                dbghdr_size=dbghdr_size, ecinfo_size=ecinfo_size)


S_PUB32 = 0x110E
S_GPROC32 = 0x1110
S_LPROC32 = 0x110F
S_GDATA32 = 0x110D
S_LDATA32 = 0x110C
S_PROCREF = 0x1125
S_LPROCREF = 0x1127
S_GPROC32_ID = 0x1147
S_LPROC32_ID = 0x1146


def sym_records(blob):
    """Yield (kind, payload) from a CodeView symbol record stream."""
    o = 0
    n = len(blob)
    while o + 4 <= n:
        (ln, kind) = struct.unpack_from("<HH", blob, o)
        if ln < 2:
            break
        rec = blob[o + 4: o + 2 + ln]
        yield kind, rec
        o += 2 + ln


def cstr(b, o=0):
    e = b.index(b"\x00", o)
    return b[o:e].decode("utf-8", "replace")


def publics(m, dbi):
    """(segment, offset, flags, name) for every S_PUB32 in the symbol record stream."""
    blob = m.stream(dbi["symrec"])
    out = []
    for kind, rec in sym_records(blob):
        if kind == S_PUB32:
            flags, off, seg = struct.unpack_from("<IIH", rec, 0)
            out.append((seg, off, flags, cstr(rec, 10)))
        elif kind in (S_GPROC32, S_LPROC32, S_GPROC32_ID, S_LPROC32_ID):
            # parent, end, next, len, dbgstart, dbgend, typind, off, seg, flags
            off, seg = struct.unpack_from("<IH", rec, 28)
            out.append((seg, off, -1, cstr(rec, 35)))
    return out


def sections(m, dbi):
    """Section headers from the optional debug header stream (index 5 of dbghdr)."""
    s = dbi["raw"]
    o = 64 + dbi["modinfo_size"] + dbi["secontr_size"] + dbi["secmap_size"] \
        + dbi["filinfo_size"] + dbi["tsmap_size"] + dbi["ecinfo_size"]
    ids = struct.unpack_from("<11h", s, o)
    sh = ids[5]
    if sh < 0:
        return []
    raw = m.stream(sh)
    out = []
    for i in range(len(raw) // 40):
        r = raw[i * 40:(i + 1) * 40]
        name = r[:8].rstrip(b"\x00").decode()
        vsize, vaddr, rsize, raddr = struct.unpack_from("<IIII", r, 8)
        out.append(dict(name=name, vaddr=vaddr, vsize=vsize))
    return out


if __name__ == "__main__":
    m = MSF(sys.argv[1])
    print("streams:", len(m.streams), "page_size:", m.page_size)
    print("info:", pdb_info(m))
    d = dbi_header(m)
    if d:
        print("dbi: machine=0x%04x symrec=%d gsym=%d psym=%d"
              % (d["machine"], d["symrec"], d["gsym"], d["psym"]))
        secs = sections(m, d)
        print("sections:", [(s["name"], hex(s["vaddr"])) for s in secs])
        p = publics(m, d)
        print("symbols:", len(p))
        for e in p[:20]:
            print("   ", e)
