#!/usr/bin/env python3
"""
savegame_parse.py -- parser for the Rise of Nations `SaveGame` byte stream.

The engine has exactly ONE serialiser.  `DataWalk` is a two-method pure-virtual
visitor (vftable 0x00b2bcd8, both slots -> _purecall); `SaveGame`, `LoadGame` and
`CheckSum` are its three concrete implementations, so *the save-game format is the
lockstep-checksum traversal is the replay header*.  See
docs/derivation/checksum.md and docs/derivation/savegame.md.

Everything encoded below was derived from riseofnations.exe
(sha256 30478a44...625079, PE32 i386, image base 0x00400000) and cross-checked
against ron-bin/sbl/rise.pdb, whose GUID 51D4F219-61C6-4F84-9D5B-C3361B0D291F /
age 1 is byte-identical to the PE's own CodeView record.  Each structure carries
its provenance as a virtual address.  Nothing here comes from community
documentation; none exists for this format.

Handles three containers, all of which carry the *same* SaveGame stream:

  *.rcx   recorded game   gzip; stream starts at Game::walk_data
  *.svx   save game       gzip; stream starts at a magic String + u32 version
  *.svx   CTW map save    RAW (not gzipped), magic String + u32 version

usage:
    uv run --quiet python re/scripts/savegame_parse.py FILE [FILE...]
    uv run --quiet python re/scripts/savegame_parse.py FILE --json
    uv run --quiet python re/scripts/savegame_parse.py FILE --hex     # trailing bytes
    uv run --quiet python re/scripts/savegame_parse.py --schema GameInfo
"""

from __future__ import annotations

import argparse
import json
import os
import struct
import sys
import zlib

SCHEMA = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                      "..", "..", "schema", "state-schema.json")


# ---------------------------------------------------------------------------
# container
# ---------------------------------------------------------------------------
# SaveGame::save_game (FUN_005a8220, 0x005a8220) opens the target with
# File::open(path, 3) -> "wb" through the gz writer (File::write FUN_00a2cc10 ->
# zlib gzwrite FUN_00509350).  Recordings are written raw and gzipped wholesale
# at game end by FUN_00952b40.  Either way the artefact on disk is ONE gzip
# member starting at offset 0 -- except CTW *map* backups, which are raw.

def load(path: str) -> bytes:
    raw = open(path, "rb").read()
    if raw[:2] == b"\x1f\x8b":
        return zlib.decompress(raw, 16 + zlib.MAX_WBITS)
    return raw


# ---------------------------------------------------------------------------
# the serialisation primitives
# ---------------------------------------------------------------------------
# SaveGame::walk_function (FUN_0043d730, vftable 0x00b35ac4 slot 0) is
#     fwrite(begin, 1, end-begin, file)
# -- raw bytes, no framing, no alignment, no type tag.  Therefore EVERY fixed
# field below is exactly the byte range its walk_data pushed, in program order.
#
# SaveGame::walk_test (FUN_0043d840, slot 1) writes exactly ONE byte: the byte at
# String+0x10 (String::module_id), filled in by FUN_00a1b6b0 when it hashes the
# section-tag name.  LoadGame::walk_test (FUN_0043da60) reads one byte and raises
# "Error loading section, probably in <name>" on mismatch.  That single byte is
# the only self-delimiting feature the format has.

class R:
    def __init__(self, b: bytes, p: int = 0):
        self.b, self.p = b, p

    def rem(self):
        return len(self.b) - self.p

    def u8(self):
        v = self.b[self.p]; self.p += 1; return v

    def u16(self):
        v = struct.unpack_from("<H", self.b, self.p)[0]; self.p += 2; return v

    def u32(self):
        v = struct.unpack_from("<I", self.b, self.p)[0]; self.p += 4; return v

    def i32(self):
        v = struct.unpack_from("<i", self.b, self.p)[0]; self.p += 4; return v

    def raw(self, n):
        if n < 0 or self.p + n > len(self.b):
            raise ValueError("range [%#x,%#x) past end of %d-byte stream"
                             % (self.p, self.p + n, len(self.b)))
        v = self.b[self.p:self.p + n]; self.p += n; return v

    # String::walk_data, FUN_00a1b2d0 (0x00a1b2d0):
    #     walk(&curr_len_as_u32, +4)                -> u32 character count
    #     walk(buf + offset*2, buf + (offset+len)*2)-> len UTF-16LE code units
    # NOT nul-terminated; the count is characters, not bytes.  A zero count
    # writes only the u32 (the second walk is skipped when len == 0).
    def wstr(self):
        n = self.u32()
        if n > (1 << 20):
            raise ValueError("absurd string length %d at %#x" % (n, self.p - 4))
        return self.raw(2 * n).decode("utf-16-le", errors="replace")


class Tree:
    """Ordered object tree with byte extents, so every node is falsifiable."""

    def __init__(self, name, off, note=None):
        self.name, self.off, self.note = name, off, note
        self.end = off
        self.kids = []
        self.fields = []

    def f(self, name, value, off, size, typ=None):
        self.fields.append(dict(name=name, value=value, off=off,
                                size=size, type=typ))
        return value

    def kid(self, t):
        self.kids.append(t)
        return t

    def to_dict(self):
        d = dict(node=self.name, at="0x%x" % self.off, size=self.end - self.off)
        if self.note:
            d["note"] = self.note
        if self.fields:
            d["fields"] = [dict(f, off="0x%x" % f["off"]) for f in self.fields]
        if self.kids:
            d["children"] = [k.to_dict() for k in self.kids]
        return d

    def show(self, ind=0, maxfields=64):
        pad = "  " * ind
        print("%s%-28s @0x%-7x %d bytes%s"
              % (pad, self.name, self.off, self.end - self.off,
                 "   " + self.note if self.note else ""))
        for f in self.fields[:maxfields]:
            v = f["value"]
            if isinstance(v, bytes):
                v = v.hex(" ")
            print("%s   +0x%-6x %-34s %s" % (pad, f["off"], f["name"], v))
        if len(self.fields) > maxfields:
            print("%s   ... %d more fields" % (pad, len(self.fields) - maxfields))
        for k in self.kids:
            k.show(ind + 1, maxfields)


# ---------------------------------------------------------------------------
# GameInfo::walk_data -- FUN_005d6570 (0x005d6570)
# ---------------------------------------------------------------------------
# Disassembled at 0x005d6596-0x005d7065.  `this` is Game::info (game+0x0c).
# Field names/offsets/types are the PDB's; the ORDER and the INCLUSION are the
# instruction stream's.
#
#   0x5d65ab  walk_test(strpool[0xdde0])                       1 byte
#   0x5d65f0  if (!checksum):
#   0x5d6656      String::walk_data("(Version: " + build + ")")
#   0x5d6664      walk(gi+0x00, +0x04)      version
#   0x5d6672  walk(gi+0x04, +0x14)          seed, checksum_deep,
#                                           checksum_window_size,
#                                           checksum_failure_threshold
#   0x5d669b  walk(gi+0x14, +0x18)          flags        (checksum path instead
#                                           walks a masked temporary: flags & ~1)
#   0x5d66ae  29 x walk(p, p+1)             gi+0x18..0x35, one byte at a time
#   0x5d66c6  walk(gi+0x35, +0x36)          mods
#   0x5d66cc  if (checksum) return
#   0x5d66d5  8 x { walk_test(strpool[0x17a48])              1 byte
#                   walk(p+0x30, p+0x32)     Player::flags
#                   if (flags & 1) {
#                       walk(p+0x00, p+0x39) 57 bytes: synced_* .. diff
#                       String::walk_data(p+0x40)  Player::name } }
#             p = gi+0x38 + i*0x8c   (sizeof(Player) == 140, PDB)
#   0x5d6734  if (sGameSaveVersion [0x00c06240] >= 0x10)  ... v16 tail
#             else                                        ... v15 tail

GI_SETTINGS = [
    "team_style", "map_style", "map_size", "players", "max_observers",
    "game_speed", "game_rules", "difficulty", "starting_town",
    "starting_resources", "starting_resources2", "tech_cost", "reveal_map",
    "pop_limit", "rush_rules", "cannon_times", "starting_technology",
    "starting_technology2", "ending_technology", "elimination", "victory",
    "wonderwin", "score_goal", "popwin", "time_limit", "chairs", "econwin",
    "scenario_type", "script_type", "mods",
]

PLAYER_57 = [
    ("synced_used_zoom_control", 0x00, 4), ("synced_frames_zoomed_in", 0x04, 4),
    ("synced_frames_zoomed_out", 0x08, 4), ("synced_clicks", 0x0c, 4),
    ("synced_hotkeys", 0x10, 4), ("synced_minimap_clicks", 0x14, 4),
    ("synced_mainmap_clicks", 0x18, 4), ("synced_cheated", 0x1c, 4),
    ("synced_control_groups_formed", 0x20, 4),
    ("synced_control_groups_activated", 0x24, 4),
    ("caravan_frame", 0x28, 4), ("pop_cap_frame", 0x2c, 4),
    ("flags", 0x30, 2), ("tribe", 0x32, 1), ("who", 0x33, 1),
    ("team", 0x34, 1), ("handicap", 0x35, 1), ("play", 0x36, 1),
    ("pauses", 0x37, 1), ("diff", 0x38, 1),
]

TAG_GAME = 0x16          # walk_test byte seen for Game::walk_data
TAG_GAMEINFO = 0x42      # ... GameInfo::walk_data
TAG_PLAYER = 0x50        # ... the per-player slot inside GameInfo


def parse_gameinfo(r: R, save_version: int, t_parent: Tree) -> Tree:
    t = Tree("GameInfo::walk_data", r.p, "FUN_005d6570")
    t_parent.kid(t)

    tag = r.u8()
    t.f("<walk_test tag>", "0x%02x" % tag, r.p - 1, 1, "u8")
    if tag != TAG_GAMEINFO:
        t.note = "UNEXPECTED TAG (expected 0x42)"

    t.f("version_string", r.wstr(), t.off + 1, r.p - t.off - 1, "String")
    o = r.p
    t.f("version", "0x%08x" % r.u32(), o, 4, "unsigned long")
    o = r.p
    t.f("seed", "0x%08x" % r.u32(), o, 4, "unsigned long")
    for nm in ("checksum_deep", "checksum_window_size",
               "checksum_failure_threshold"):
        o = r.p
        t.f(nm, r.i32(), o, 4, "int")
    o = r.p
    t.f("flags", "0x%08x" % r.u32(), o, 4, "unsigned long")
    for nm in GI_SETTINGS:                      # gi+0x18 .. gi+0x36
        o = r.p
        t.f(nm, r.u8(), o, 1, "unsigned char")

    for i in range(8):
        p = Tree("Player[%d]" % i, r.p)
        t.kid(p)
        ptag = r.u8()
        p.f("<walk_test tag>", "0x%02x" % ptag, r.p - 1, 1, "u8")
        if ptag != TAG_PLAYER:
            p.note = "UNEXPECTED TAG (expected 0x50)"
            p.end = r.p
            raise ValueError("player %d tag 0x%02x != 0x50 at %#x"
                             % (i, ptag, r.p - 1))
        o = r.p
        flags = r.u16()
        p.f("flags", "0x%04x" % flags, o, 2, "unsigned short")
        if flags & 1:
            body = r.raw(0x39)
            for nm, off, sz in PLAYER_57:
                v = int.from_bytes(body[off:off + sz], "little")
                p.f(nm, v, r.p - 0x39 + off, sz,
                    {1: "u8", 2: "u16", 4: "int"}[sz])
            o = r.p
            p.f("name", r.wstr(), o, r.p - o, "String")
        else:
            p.note = "slot unused (flags & 1 == 0)"
        p.end = r.p

    parse_mod_block(r, save_version, t)
    t.end = r.p
    return t


def parse_mod_block(r: R, save_version: int, t: Tree):
    if save_version >= 0x10:
        # 0x5d68ac..0x5d68ed -- the mod-package block, only in format >= 16
        m = Tree("mod block (sGameSaveVersion >= 16)", r.p, "0x005d68ac")
        t.kid(m)
        o = r.p
        m.f("mod_checksum", "0x%08x" % r.u32(), o, 4, "unsigned long")
        o = r.p
        m.f("mod_total_size", r.u32(), o, 4, "unsigned long")
        for nm in ("scenario_script", "scenario_path", "mod_name"):
            o = r.p
            m.f(nm, r.wstr(), o, r.p - o, "String")
        o = r.p
        m.f("mod2_checksum", "0x%08x" % r.u32(), o, 4, "unsigned long")
        o = r.p
        m.f("mod2_total_size", r.u32(), o, 4, "unsigned long")
        o = r.p
        m.f("mod2_name", r.wstr(), o, r.p - o, "String")
        m.end = r.p
    else:
        # 0x5d6fe5 -- format 15 and earlier
        m = Tree("mod block (sGameSaveVersion < 16)", r.p, "0x005d6fe5")
        t.kid(m)
        for nm in ("scenario_script", "scenario_path", "scenario_dir",
                   "mod_name"):
            o = r.p
            m.f(nm, r.wstr(), o, r.p - o, "String")
        m.end = r.p


# ---------------------------------------------------------------------------
# Game::walk_data -- FUN_00589600 (0x00589600)
# ---------------------------------------------------------------------------
#   walk_test(strpool[0xd980])                       1 byte
#   GameInfo::walk_data(game+0x0c)
#   walk(game+0x550, game+0x6e4)                     404 bytes: frame .. armageddon
#   if (!checksum):
#       walk(game+0x814, game+0x81c)                 semaphore.bits, semaphore.size
#       walk(game+0x820, game+0x814 + semaphore.size + 0xc)
#                                                    == semaphore.size bytes of
#                                                       semaphore.ptr
#       walk(game+0x844, game+0x848)                 graphic_tick

GAME_404 = [
    ("frame", 0x550, 4, "int"), ("frame_to_break", 0x554, 4, "int"),
    ("playing", 0x558, 4, "int"), ("loading", 0x55c, 4, "int"),
    ("tick", 0x560, 4, "int"), ("market_tick", 0x564, 4, "int"),
    ("market", 0x568, 24, "int[6]"),
    ("world_cities", 0x6d0, 4, "int"), ("world_villages", 0x6d4, 4, "int"),
    ("total_units", 0x6d8, 4, "int"), ("everyone_mask", 0x6dc, 4, "int"),
    ("armageddon", 0x6e0, 4, "int"),
]


def parse_game(r: R, save_version, root: Tree) -> Tree:
    """save_version None => a recording, which carries no version word: try both
    GameInfo tails and keep the one whose Game block satisfies the structural
    invariant  semaphore.bits == 8*semaphore.size  and  0 <= size <= 32
    (Game::semaphore.ptr is `unsigned char[32]` in the PDB)."""
    if save_version is None:
        start, last = r.p, None
        for cand in (0x10, 0x0f):
            probe = Tree("probe", 0)
            r.p = start
            try:
                t = _parse_game(r, cand, probe)
                sem = [k for k in t.kids if k.name == "Game::semaphore"][0]
                bits = [f for f in sem.fields if f["name"] == "semaphore.bits"][0]["value"]
                size = [f for f in sem.fields if f["name"] == "semaphore.size"][0]["value"]
                if 0 <= size <= 32 and bits == 8 * size:
                    r.p = start
                    t = _parse_game(r, cand, root)
                    t.note = "FUN_00589600; sGameSaveVersion inferred = %d" % cand
                    return t
            except Exception as e:
                last = e
        r.p = start
        raise ValueError("no GameInfo tail variant validates (last: %s)" % last)
    return _parse_game(r, save_version, root)


def _parse_game(r: R, save_version: int, root: Tree) -> Tree:
    t = Tree("Game::walk_data", r.p, "FUN_00589600")
    root.kid(t)
    tag = r.u8()
    t.f("<walk_test tag>", "0x%02x" % tag, r.p - 1, 1, "u8")
    if tag != TAG_GAME:
        t.note = "UNEXPECTED TAG (expected 0x16)"

    parse_gameinfo(r, save_version, t)

    blk = Tree("Game +0x550..+0x6e4", r.p, "walk(game+0x550, game+0x6e4)")
    t.kid(blk)
    base = r.p
    body = r.raw(404)
    for nm, off, sz, ty in GAME_404:
        raw = body[off - 0x550:off - 0x550 + sz]
        v = (int.from_bytes(raw, "little", signed=(sz == 4))
             if sz == 4 else list(struct.unpack("<%dI" % (sz // 4), raw)))
        blk.f(nm, v, base + off - 0x550, sz, ty)
    blk.end = r.p

    sem = Tree("Game::semaphore", r.p, "walk(game+0x814,+0x81c) then size bytes")
    t.kid(sem)
    o = r.p
    bits = sem.f("semaphore.bits", r.i32(), o, 4, "int")
    o = r.p
    size = sem.f("semaphore.size", r.i32(), o, 4, "int")
    o = r.p
    sem.f("semaphore.ptr", r.raw(size), o, size, "unsigned char[%d]" % size)
    sem.end = r.p

    o = r.p
    t.f("graphic_tick", r.i32(), o, 4, "int")
    t.end = r.p
    return t


# ---------------------------------------------------------------------------
# top-level format dispatch
# ---------------------------------------------------------------------------
# SaveGame::save_game (0x005a8220):
#     String::write(magic)          "RoNSave" | "RoNMultiSave" | "RoNCTWSave"
#     sGameSaveVersion = 0x10; fwrite(&sGameSaveVersion, 1, 4)
#     do_save() -> SaveGame::do_save 0x005a81f0 -> WalkDataGame::walk_data 0x005a2360
# ConquestSaveGame writes "RonCTWMapSave" and is NOT gzipped.
#
# The recorder (CommandManager, FUN_00952a50) writes NO magic and NO version --
# it goes straight to Game::walk_data and then String::walk_data(game.info.save_name).

MAGICS = {"RoNSave", "RoNMultiSave", "RoNCTWSave", "RonCTWMapSave"}


def parse(buf: bytes) -> Tree:
    root = Tree("stream", 0)
    r = R(buf)
    # save games open with a length-prefixed magic String
    n = struct.unpack_from("<I", buf, 0)[0]
    magic = None
    if 0 < n < 32:
        try:
            cand = buf[4:4 + 2 * n].decode("utf-16-le")
            if cand in MAGICS:
                magic = cand
        except Exception:
            pass
    if magic:
        root.note = "save game"
        o = r.p
        root.f("magic", r.wstr(), o, r.p - o, "String")
        o = r.p
        ver = root.f("sGameSaveVersion", r.u32(), o, 4, "int  [0x00c06240]")
        if magic == "RonCTWMapSave":
            root.note = "ConquestSaveGame map backup -- not the Game walk"
            root.end = r.p
            return root
        # WalkDataGame::walk_data (FUN_005a2360) opens with GameInfo::walk_data
        # on game+0x0c, then Console state, then ... then Game::walk_data.
        wdg = Tree("WalkDataGame::walk_data", r.p, "FUN_005a2360")
        root.kid(wdg)
        parse_gameinfo(r, ver, wdg)
        con = Tree("Console [0x00c06210]", r.p,
                   "walk(+0x298,+0x2b8) walk(+0x2b8,+0x2c2) walk(+0x2c4,+0x338)")
        wdg.kid(con)
        for a, b in ((0x298, 0x2b8), (0x2b8, 0x2c2), (0x2c4, 0x338)):
            o = r.p
            con.f("console+0x%03x..0x%03x" % (a, b), r.raw(b - a), o, b - a,
                  "unsigned char[%d]" % (b - a))
        con.end = r.p
        wdg.end = r.p
        wdg.note = ("continues with Console::vt[0x1c], then the world/object "
                    "pool, then Game::walk_data at 0x005a2945 -- not decoded here")
    else:
        root.note = "recorded game (.rcx): no magic, no version word"
        parse_game(r, None, root)
        o = r.p
        root.f("game.info.save_name", r.wstr(), o, r.p - o, "String")
    root.end = r.p
    return root


# ---------------------------------------------------------------------------
def show_schema(cls: str):
    """Print the recovered ordered serialisation of a class from
    schema/state-schema.json."""
    S = json.load(open(SCHEMA))["classes"]
    if cls not in S:
        cand = sorted(k for k in S if cls.lower() in k.lower())
        print("no exact match; candidates:", ", ".join(cand[:40]) or "(none)")
        return
    e = S[cls]
    print("== %s::%s   %s   sizeof=%s   sim-critical bytes=%s (%s of the class)"
          % (cls, e["method"], e["walk_data"], e["sizeof"], e["walked_bytes"],
             ("%.0f%%" % (100 * e["coverage"])) if e["coverage"] else "?"))
    if e["shared_with"]:
        print("   implementation shared with: %s" % ", ".join(e["shared_with"]))
    for o in e["ops"]:
        g = ("  [cond x%d]" % o["guard_depth"]) if o["guard_depth"] else ""
        g += "  [in loop]" if o["in_loop"] else ""
        if o["kind"] == "tag":
            print("  %s  tag byte                                    1%s"
                  % (o["at"], g))
        elif o["kind"] == "bytes":
            nb = o.get("bytes")
            rng = ("this+0x%x .. this+0x%x" % (o["begin"], o["end"])
                   if o.get("end") is not None and isinstance(o.get("begin"), int)
                   else o.get("note", ""))
            print("  %s  bytes  %-34s %s%s"
                  % (o["at"], rng, ("%d" % nb) if nb is not None else "?", g))
            for f in o.get("fields", []):
                print("            +0x%-5x %-4d %-38s %s%s"
                      % (f["off"], f["size"], f["name"], f["type"],
                         "   (PARTIAL)" if f.get("partial") else ""))
        elif o["kind"] == "sub_object":
            print("  %s  ->     %s::%s  (%s)%s"
                  % (o["at"], o.get("target_class") or "FUN_" + o["target"],
                     o.get("target_method") or "?", o.get("this"), g))
        elif o["kind"] == "virtual":
            print("  %s  VIRTUAL dispatch, vtable slot %s%s"
                  % (o["at"], o["slot"], g))


# ---------------------------------------------------------------------------
# falsifiable cross-checks
# ---------------------------------------------------------------------------
# "parses to EOF with no residue" is weak evidence.  These predicates are ones a
# wrong layout would fail: fixed tag bytes at computed positions, an internal
# arithmetic identity, and cross-file agreement over specimens spanning three
# engine builds and twelve years.

def verify(paths):
    import re as _re
    rows, byver = [], {}
    for path in paths:
        buf = load(path)
        checks = []
        try:
            t = parse(buf)
        except Exception as e:
            rows.append((os.path.basename(path), "PARSE FAILED: %s" % e, []))
            continue

        def find(node, name):
            if node.name.startswith(name):
                return node
            for k in node.kids:
                r = find(k, name)
                if r:
                    return r
            return None

        def fld(node, name):
            for f in node.fields:
                if f["name"] == name:
                    return f["value"]
            return None

        gi = find(t, "GameInfo::walk_data")
        if gi is None:
            rows.append((os.path.basename(path),
                         "%s (no GameInfo section)" % (t.note or ""), []))
            continue
        checks.append(("GameInfo walk_test byte == 0x42",
                       fld(gi, "<walk_test tag>") == "0x42"))
        pl = [k for k in gi.kids if k.name.startswith("Player[")]
        checks.append(("8 Player slots, every walk_test byte == 0x50",
                       len(pl) == 8 and all(fld(p, "<walk_test tag>") == "0x50"
                                            for p in pl)))
        vs = fld(gi, "version_string")
        checks.append((r"version_string matches '(Version: N...)'",
                       bool(_re.match(r"^\(Version: [\d.]+\)$", vs or ""))))
        g = find(t, "Game::walk_data")
        if g:
            checks.append(("Game walk_test byte == 0x16",
                           fld(g, "<walk_test tag>") == "0x16"))
            sem = find(g, "Game::semaphore")
            b, s = fld(sem, "semaphore.bits"), fld(sem, "semaphore.size")
            checks.append(("semaphore.bits == 8*semaphore.size and size <= 32",
                           s is not None and 0 <= s <= 32 and b == 8 * s))
            blk = find(g, "Game +0x550")
            checks.append(("Game::frame_to_break == -1",
                           fld(blk, "frame_to_break") == -1))
        byver.setdefault(vs, set()).add(fld(gi, "version"))
        rows.append((os.path.basename(path),
                     "%s  players=%d used  prefix=0x%x"
                     % (vs, sum(1 for p in pl if not p.note), t.end),
                     checks))

    npass = nfail = 0
    for name, info, checks in rows:
        print("%-22s %s" % (name, info))
        for label, ok in checks:
            print("      [%s] %s" % ("ok" if ok else "FAIL", label))
            npass += ok
            nfail += not ok
    print("\ncross-file: GameInfo.version is constant per version_string")
    allok = True
    for vs, vals in sorted(byver.items(), key=lambda kv: str(kv[0])):
        ok = len(vals) == 1
        allok &= ok
        print("      [%s] %-30s -> %s" % ("ok" if ok else "FAIL", vs,
                                          ", ".join(sorted(map(str, vals)))))
    npass += allok
    nfail += not allok
    print("\n%d predicates passed, %d failed, over %d specimens"
          % (npass, nfail, len(rows)))


# ---------------------------------------------------------------------------
# the tag byte, predicted rather than observed
# ---------------------------------------------------------------------------
# SaveGame::walk_test writes String::module_id (String+0x10).  FUN_00a1b6b0 is
# `String::generate_hash(const wchar_t*, unsigned long* out_ci)`: it returns the
# case-SENSITIVE hash and stores the case-INSENSITIVE one through the out-param,
# and walk_test passes &String+0x10 as that out-param, then writes its low byte.
# T is the 50-entry int table at 0x00b14500.
#
#   h = 0;  L = len
#   for u = len-1 downto 0:
#       c = towlower(s[u]);  h += T[c % 50]*L + c*T[u % 50];  L -= 1
#   tag = h & 0xff
#
# Verified: hash("game") & 0xff == 0x16, hash("gameinfo") & 0xff == 0x42,
# hash("player") & 0xff == 0x50 -- the three tag bytes real files actually carry.

HASH_T = None


def _table():
    global HASH_T
    if HASH_T is None:
        import pefile
        pe = pefile.PE(os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                    "..", "..", "ron-bin", "riseofnations.exe"))
        img = pe.get_memory_mapped_image()
        b = pe.OPTIONAL_HEADER.ImageBase
        HASH_T = list(struct.unpack_from("<50i", img, 0x00b14500 - b))
    return HASH_T


def generate_hash(s: str, insensitive=True) -> int:
    T = _table()
    h, L = 0, len(s)
    for u in range(len(s) - 1, -1, -1):
        c = ord(s[u].lower() if insensitive else s[u])
        h = (h + T[c % 50] * L + c * T[u % 50]) & 0xFFFFFFFF
        L -= 1
    return h


def show_toc():
    S = json.load(open(SCHEMA))["classes"]
    for cls, top in (("WalkDataGame", "the save game"), ("Game", "the .rcx header")):
        print("== %s::walk_data  (%s)" % (cls, top))
        for o in S[cls]["ops"]:
            k = o["kind"]
            if k == "sub_object":
                print("   %s  ->  %s::%s"
                      % (o["at"], o.get("target_class") or "FUN_" + o["target"],
                         o.get("target_method")))
            elif k == "tag":
                print("   %s  tag byte" % o["at"])
            elif k == "bytes":
                b, e = o.get("begin"), o.get("end")
                if isinstance(b, int) and isinstance(e, int):
                    print("   %s  %-6d bytes   %s +0x%x..0x%x"
                          % (o["at"], o["bytes"], o.get("base", "this"), b, e))
                else:
                    print("   %s  %-6s        %s" % (o["at"], o.get("bytes"),
                                                     o.get("note", "")))
            elif k == "virtual":
                print("   %s  VIRTUAL dispatch, slot %s" % (o["at"], o["slot"]))
        print()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("files", nargs="*")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--hex", action="store_true",
                    help="hexdump the 64 bytes following the parsed prefix")
    ap.add_argument("--schema", help="print the recovered walk_data schema of a class")
    ap.add_argument("--brief", action="store_true")
    ap.add_argument("--verify", action="store_true",
                    help="run the falsifiable cross-checks over the given files")
    ap.add_argument("--toc", action="store_true",
                    help="print the save-game section order (table of contents)")
    ap.add_argument("--tag", metavar="NAME",
                    help="predict the walk_test tag byte for a section name")
    a = ap.parse_args()

    if a.tag:
        h = generate_hash(a.tag)
        print("%-24s hash_ci = 0x%08x   tag byte = 0x%02x" % (a.tag, h, h & 0xFF))
        return
    if a.toc:
        show_toc()
        return
    if a.verify:
        verify(a.files)
        return
    if a.schema:
        show_schema(a.schema)
        return

    out = []
    for path in a.files:
        buf = load(path)
        try:
            t = parse(buf)
            err = None
        except Exception as e:
            t, err = None, e
        if a.json:
            out.append(dict(file=path, plain_bytes=len(buf),
                            error=str(err) if err else None,
                            tree=t.to_dict() if t else None))
            continue
        print("=" * 78)
        print("%s   %d plain bytes" % (path, len(buf)))
        if err:
            print("  PARSE ERROR:", err)
            continue
        t.show(maxfields=0 if a.brief else 64)
        print("  prefix consumed: 0x%x of 0x%x bytes" % (t.end, len(buf)))
        if a.hex:
            for i in range(t.end, min(t.end + 64, len(buf)), 16):
                row = buf[i:i + 16]
                print("  %06x  %-47s  %s" % (i, row.hex(" "),
                      "".join(chr(c) if 32 <= c < 127 else "." for c in row)))
    if a.json:
        json.dump(out, sys.stdout, indent=1)
        print()


if __name__ == "__main__":
    main()
