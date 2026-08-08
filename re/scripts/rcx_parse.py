#!/usr/bin/env python3
"""
rcx_parse.py -- parser for Rise of Nations: Extended Edition `.rcx` recorded games.

Everything encoded here was derived from riseofnations.exe (sha256 30478a44...625079,
2024-06-20 MSVC rebuild, image base 0x00400000).  Provenance for each structure is
given in the comment above it as a virtual address in that image.  Nothing here comes
from community documentation; no such documentation exists for this format.

See /Users/ember/dev/don/docs/derivation/replay-io.md for the full derivation.

usage:
    uv run --quiet python re/scripts/rcx_parse.py ron-data/replays/today.rcx
    uv run --quiet python re/scripts/rcx_parse.py FILE --commands      # dump command stream
    uv run --quiet python re/scripts/rcx_parse.py FILE --checksums     # dump checksum packets
    uv run --quiet python re/scripts/rcx_parse.py FILE --json
"""

from __future__ import annotations

import argparse
import json
import struct
import sys
import zlib
from dataclasses import dataclass, field
from typing import Any


# ---------------------------------------------------------------------------
# container
# ---------------------------------------------------------------------------
# The whole file is one gzip stream from offset 0.  The engine writes the
# recording UNCOMPRESSED to a temp file while playing, then on game end
# CommandManager::finish_recording (FUN_00952b40, 0x00952b40) memory-maps that
# raw file, copies it through a gz-mode File (File::write, FUN_00a2cc10 ->
# zlib gzwrite FUN_00509350) into "recordgame.tmp", removes the original and
# _wrename()s the tmp over it.  Hence: gzip of the raw stream, one member.

def gunzip(path: str) -> bytes:
    raw = open(path, "rb").read()
    return zlib.decompress(raw, 16 + zlib.MAX_WBITS)


# ---------------------------------------------------------------------------
# byte reader
# ---------------------------------------------------------------------------

class R:
    def __init__(self, buf: bytes, pos: int = 0):
        self.b = buf
        self.p = pos

    def u8(self) -> int:
        v = self.b[self.p]
        self.p += 1
        return v

    def i8(self) -> int:
        v = struct.unpack_from("<b", self.b, self.p)[0]
        self.p += 1
        return v

    def u16(self) -> int:
        v = struct.unpack_from("<H", self.b, self.p)[0]
        self.p += 2
        return v

    def u32(self) -> int:
        v = struct.unpack_from("<I", self.b, self.p)[0]
        self.p += 4
        return v

    def i32(self) -> int:
        v = struct.unpack_from("<i", self.b, self.p)[0]
        self.p += 4
        return v

    def raw(self, n: int) -> bytes:
        v = self.b[self.p:self.p + n]
        self.p += n
        return v

    # String::walk_data, FUN_00a1b2d0 (0x00a1b2d0):
    #   walk(&len, &len+4)      -> u32 character count
    #   walk(chars, chars+2*len)-> len UTF-16LE code units, NOT nul terminated
    def wstr(self) -> str:
        n = self.u32()
        if n > 1 << 20:
            raise ValueError(f"absurd string length {n} at {self.p - 4:#x}")
        return self.raw(2 * n).decode("utf-16-le", errors="replace")


# ---------------------------------------------------------------------------
# section tag bytes
# ---------------------------------------------------------------------------
# SaveGame::walk_tag (FUN_0043d840, vftable 0x00b35ac4 slot 1) writes exactly ONE
# byte per section: the byte at offset +0x10 of the tag String, which
# FUN_00a1b6b0 fills in when it hashes the tag name (the 32-bit hash goes to
# String+0x0c, the byte marker to String+0x10).  LoadGame::walk_tag
# (FUN_0043da60) reads that byte back and raises
# "Error loading section, probably in <name>" on mismatch.
# Observed values, confirmed by exact structural alignment on today.rcx:
TAG_GAME = 0x16        # Game::walk_data       FUN_00589600
TAG_GAMEINFO = 0x42    # GameInfo::walk_data   FUN_005d6570
TAG_PLAYER = 0x50      # per-player slot       FUN_005d6570 inner loop


# ---------------------------------------------------------------------------
# header  (Game::walk_data 0x00589600 -> GameInfo::walk_data 0x005d6570)
# ---------------------------------------------------------------------------

@dataclass
class PlayerSlot:
    index: int
    tag: int
    flags_word: int          # GameInfo player[i] bytes +0x00..+0x02
    present: bool
    blob: bytes = b""        # 57 bytes, GameInfo player[i] -0x30 .. +0x09
    name: str = ""


@dataclass
class Header:
    tag_game: int
    tag_gameinfo: int
    version: str
    gi_00: int               # GameInfo +0x00 .. +0x04  (u32)
    gi_04: bytes             # GameInfo +0x04 .. +0x14  (16 bytes)
    gi_14: int               # GameInfo +0x14 .. +0x18  (u32)
    gi_18: bytes             # GameInfo +0x18 .. +0x35  (29 bytes, walked one at a time)
    gi_35: int               # GameInfo +0x35           (1 byte)
    players: list[PlayerSlot] = field(default_factory=list)
    end: int = 0


def parse_header(buf: bytes) -> Header:
    r = R(buf)
    tag_game = r.u8()          # Game::walk_data       walk_tag
    tag_gameinfo = r.u8()      # GameInfo::walk_data   walk_tag
    if tag_game != TAG_GAME or tag_gameinfo != TAG_GAMEINFO:
        raise ValueError(f"unexpected section tags {tag_game:#04x} {tag_gameinfo:#04x}")

    # 0x005d6650: str = "(Version: " + build_version() + ")" ; String::walk_data
    version = r.wstr()

    # 0x005d665b: walk(gi+0x00, gi+0x04)
    gi_00 = r.u32()
    # 0x005d6666: walk(gi+0x04, gi+0x14)
    gi_04 = r.raw(0x10)
    # 0x005d6692: walk(gi+0x14, gi+0x18)  (save/load only; CheckSum takes the other branch)
    gi_14 = r.u32()
    # 0x005d669d: 0x1d single-byte walks, gi+0x18 .. gi+0x35
    gi_18 = r.raw(0x1D)
    # 0x005d66b7: walk(gi+0x35, gi+0x36)
    gi_35 = r.u8()

    # 0x005d66cd: for 8 player slots, stride 0x8c starting at gi+0x68:
    #   walk_tag(...)                  -> 1 byte
    #   walk(p+0, p+2)                 -> 2 bytes
    #   if (p[0] & 1):
    #       walk(p-0x30, p+9)          -> 0x39 = 57 bytes
    #       String::walk_data(p+...)   -> player name
    players = []
    for i in range(8):
        tag = r.u8()
        w = r.u16()
        present = bool(w & 1)
        blob, name = b"", ""
        if present:
            blob = r.raw(0x39)
            name = r.wstr()
        players.append(PlayerSlot(i, tag, w, present, blob, name))

    h = Header(tag_game, tag_gameinfo, version, gi_00, gi_04, gi_14, gi_18, gi_35, players)
    h.end = r.p
    return h


# ---------------------------------------------------------------------------
# command opcode table
# ---------------------------------------------------------------------------
# CommandPackage::process (FUN_0094a700, 0x0094a700) is a switch on data[0].
# Each handler returns the number of bytes the packet occupies, so the table
# below IS the wire format.  Names come from the UTF-16 log-format literals each
# handler pushes (e.g. L"process_halt game.frame: %d" at 0x00af8810).
#
# size:  int              -> fixed size in bytes, including the opcode byte
#        ("expr", off, w) -> variable; count is read at `off` with width `w`,
#                            total = count * mul + base   (see VARIABLE below)

FIXED = {
    0x01: (1,      "begin",                   0x00949fd0),
    0x02: (5,      "stance",                  0x00949ed0),
    0x03: (0x0d,   "form",                    0x00949d90),
    0x04: (0x11,   "attack",                  0x00949c30),
    0x05: (0x0d,   "siege_attack",            0x00949ae0),
    0x06: (0x11,   "swarm_around",            0x00949970),
    0x07: (0x16,   "move_to",                 0x009497c0),
    0x08: (0x1a,   "move_near",               0x009495c0),
    0x09: (0x0a,   "attack_ground",           0x009494a0),
    0x0a: (0x0a,   "patrol",                  0x00949380),
    0x0b: (0x19,   "launch_patrol",           0x00949230),
    0x0c: (1,      "halt",                    0x00949140),
    0x0d: (1,      "transport",               0x00949050),
    0x0e: (5,      "set_transport",           0x00948f60),
    0x0f: (9,      "set_transport_o",         0x00948e00),
    0x10: (0x0d,   "repair",                  0x00948cb0),
    0x11: (0x15,   "trade",                   0x00948b20),
    0x12: (9,      "city_gather",             0x00948a10),
    0x13: (9,      "gather",                  0x009488b0),
    0x14: (0x0d,   "garrison",                0x00948760),
    0x15: (5,      "disband",                 0x00948660),
    0x16: (0x11,   "gather_point",            0x00948510),
    0x17: (0x15,   "spell",                   0x00948340),
    0x18: (9,      "queue_up",                0x00948230),
    0x19: (0x19,   "queue_up_build",          0x00948110),
    0x1a: (0x11,   "eject_all",               0x00947fe0),
    0x1b: (1,      "alarm",                   0x00947ef0),
    0x1c: (0x19,   "flight",                  0x00947db0),
    0x1d: (1,      "stop_spell",              0x00947cc0),
    0x1e: (0x0d,   "follow",                  0x009479c0),
    0x1f: (0x0d,   "guard",                   0x009478a0),
    0x20: (9,      "unitmask",                0x00947790),
    0x21: (9,      "buildmask",               0x00947680),
    0x22: (0x19,   "hotkey",                  0x009474d0),
    0x23: (1,      "recall",                  0x00947bd0),
    0x24: (1,      "scramble",                0x00947ae0),
    0x25: (0x0d,   "treaty",                  0x009473c0),
    0x26: (0x0d,   "declare",                 0x009472b0),
    0x27: (9,      "clear_tributes",          0x009471b0),
    0x28: (9,      "clear_all",               0x009470b0),
    0x29: (9,      "accept",                  0x00946fb0),
    0x2a: (9,      "reject",                  0x00946e90),
    0x2b: (0x11,   "tribute",                 0x00946d70),
    0x2c: (0x11,   "demand_tribute",          0x00946c50),
    0x2d: (0x11,   "demand_tribute_onoff",    0x00946b30),
    0x2e: (0x0d,   "buy",                     0x00946a20),
    0x2f: (0x0d,   "sell",                    0x009468a0),
    0x30: (0x0f,   "unqueue",                 0x009466f0),
    0x31: (0x0b,   "come_out",                0x009465d0),
    0x32: (9,      "ping",                    0x009453f0),
    0x34: (5,      "speed_set",               0x00946380),
    0x35: (1,      "speed_up",                0x009461a0),
    0x36: (1,      "speed_down",              0x00946290),
    0x37: (1,      "mp_log",                  0x00946080),
    0x38: (5,      "check_random",            0x00946020),
    0x39: (0x41,   "check_sums",              0x009459d0),
    0x3a: (6,      "next_check_sum",          0x00945e20),
    0x3b: (5,      "cheat_view_all",          0x009449b0),
    0x3c: (5,      "cheat_give_techs",        0x00945070),
    0x3d: (5,      "cheat_zero_techs",        0x00944fa0),
    0x3e: (1,      "cheat_ai_speed_increase", 0x00944ec0),
    0x3f: (1,      "cheat_ai_speed_normal",   0x00944df0),
    0x40: (1,      "cheat_ai_toggle",         0x00944d20),
    0x41: (5,      "cheat_increase_buckets",  0x00944c50),
    0x42: (5,      "cheat_zero_buckets",      0x00944b80),
    0x43: (0x11,   "cheat_init_unit",         0x00944a80),
    0x45: (9,      "chat_stats",              0x009458e0),
    0x46: (5,      "resign",                  0x009438c0),
    0x47: (7,      "quit",                    0x009439a0),
    0x48: (0x0a,   "camera",                  0x00943b00),
    0x49: (0x21,   "leader_options",          0x009441d0),
    0x4a: (0x0b,   "turn_data",               0x00943d20),
    0x4b: (0x35,   "rename_city",             0x00944090),
    0x4c: (2,      "pause",                   0x00944160),
    0x4d: (2,      "cannon_time",             0x009464f0),
    0x4e: (0x209,  "console_cmd",             0x00943f30),
    0x4f: (9,      "player_speed",            0x00943730),
    0x50: (3,      "ungraceful_player_drop",  0x00943ea0),
    0x51: (2,      "marwan",                  0x00943660),
}

# variable-length packets:  (name, handler, count_offset, count_width, mul, base)
VARIABLE = {
    #  FUN_0094a0c0: return (uint)data[1] * 2 + 3
    0x00: ("group",     0x0094a0c0, 1, 1, 2, 3),
    #  FUN_00945140: return (uint)*(u16*)(data+4) * 8 + 6
    0x33: ("ping_line", 0x00945140, 4, 2, 8, 6),
    #  FUN_009454f0: return *(i32*)(data+0x0d) * 2 + 0x13
    0x44: ("chat_set",  0x009454f0, 0x0d, 4, 2, 0x13),
}

MAXOP = 0x51


def cmd_name(op: int) -> str:
    if op in FIXED:
        return FIXED[op][1]
    if op in VARIABLE:
        return VARIABLE[op][0]
    return f"op_{op:#04x}"


def cmd_size(data: bytes, off: int) -> int:
    """Size in bytes of the command packet starting at data[off]. 0 if undecodable."""
    op = data[off]
    if op in FIXED:
        return FIXED[op][0]
    if op in VARIABLE:
        _n, _ea, coff, cw, mul, base = VARIABLE[op]
        if off + coff + cw > len(data):
            return 0
        if cw == 1:
            c = data[off + coff]
        elif cw == 2:
            c = struct.unpack_from("<H", data, off + coff)[0]
        else:
            c = struct.unpack_from("<i", data, off + coff)[0]
        if c < 0 or c > 4096:
            return 0
        return c * mul + base
    return 0


def split_commands(payload: bytes) -> tuple[list[tuple[int, int, int]], bool]:
    """Split a package payload into (offset, opcode, size). Returns (list, clean)."""
    out: list[tuple[int, int, int]] = []
    p = 0
    while p < len(payload):
        op = payload[p]
        n = cmd_size(payload, p)
        if n <= 0 or p + n > len(payload):
            return out, False
        out.append((p, op, n))
        p += n
    return out, True


# ---------------------------------------------------------------------------
# command-package records
# ---------------------------------------------------------------------------
# Writer: FUN_00952fb0 (0x00952fb0), called from CommandManager::process_turn
# (FUN_0093ef10) whenever the record flag game[0x820] & 8 is set.  It writes, in
# this exact order:
#     fwrite(&game.frame,   1, 4)     # game struct is [0x00c061ec], frame at +0x550
#     fwrite(pkg + 0x04,    1, 4)     # 'play'   (GameLog name at 0x00af7718)
#     fwrite(pkg + 0x08,    1, 4)     # 'valid'  (0x00af7738)
#     fwrite(pkg + 0x00,    1, 4)     # 'stamp'  (0x00af76b8)
#     fwrite(pkg + 0x10,    1, 2)     # u16 size
#     fwrite(pkg + 0x12,    1, size)  # payload
# Reader: FUN_00952d90 (0x00952d90) reads the same six fields in the same order.
REC_HDR = 18


@dataclass
class Record:
    off: int
    frame: int
    play: int
    valid: int
    stamp: int
    size: int
    payload: bytes


def try_records(buf: bytes, start: int, limit: int | None = None,
                strict: bool = True) -> list[Record]:
    """Walk the record chain from `start`.

    strict=True also requires each payload to split into whole command packets;
    that holds for every single-player recording measured, and is what makes the
    stream start unambiguous.  strict=False validates only the 18-byte framing,
    which is what multiplayer recordings need (their payloads are obfuscated —
    see docs/derivation/replay-io.md).
    """
    recs: list[Record] = []
    p = start
    n = len(buf)
    last_frame = -1
    while p + REC_HDR <= n:
        frame, play, valid, stamp = struct.unpack_from("<4I", buf, p)
        size = struct.unpack_from("<H", buf, p + 16)[0]
        if p + REC_HDR + size > n:
            break
        if frame > 50_000_000 or play > 7 or valid > 1 or stamp > 50_000_000:
            break
        if frame < last_frame:
            break
        payload = buf[p + REC_HDR:p + REC_HDR + size]
        if strict:
            _cmds, clean = split_commands(payload)
            if not clean:
                break
        recs.append(Record(p, frame, play, valid, stamp, size, payload))
        last_frame = frame
        p += REC_HDR + size
        if limit and len(recs) >= limit:
            break
    return recs


def find_stream(buf: bytes, header_end: int) -> tuple[int, list[Record], bool]:
    """Locate the command-package stream. Returns (start, records, payloads_plain).

    The stream always runs to the last byte of the payload (the writer appends and
    nothing follows), so the correct start is the offset from which the chain
    consumes the file exactly.
    """
    for strict in (True, False):
        for start in range(header_end, len(buf)):
            recs = try_records(buf, start, limit=200, strict=strict)
            if len(recs) < 6:
                continue
            if strict is False and sum(1 for r in recs if r.size) < 4:
                continue
            full = try_records(buf, start, strict=strict)
            consumed = (full[-1].off + REC_HDR + full[-1].size) if full else start
            if len(buf) - consumed <= 4:
                return start, full, strict
    return 0, [], True


# ---------------------------------------------------------------------------
# checksum packets  (opcode 0x39, 65 bytes)
# ---------------------------------------------------------------------------
# FUN_009459d0 logs sixteen u32 at data+1, +5, +9, ... +0x3d, in the same order
# as check_all (FUN_00936560) computes them.  See docs/derivation/checksum.md.
CHECKSUM_CHANNELS = [
    "units", "builds", "walls", "ammo", "deaths", "groups", "guys", "leaders",
    "cities", "items", "goods", "world", "rules", "scenario_data",
    "script_run_time", "total",
]


def parse_checksum(payload: bytes, off: int) -> dict[str, int]:
    vals = struct.unpack_from("<16I", payload, off + 1)
    return dict(zip(CHECKSUM_CHANNELS, vals))


# ---------------------------------------------------------------------------
# multiplayer payload obfuscation  -- NOT yet fully derived
# ---------------------------------------------------------------------------
# In every multiplayer recording measured, the 18-byte record framing is
# plaintext and identical to single-player, but the payload bytes are not.  The
# dominant component of the transform is a repeating 2-byte value that differs
# per file (0xbb97 for the 2024 capture, 0x3f69 for the 2020 capture); XOR-ing
# with it recovers recognisable opcode sequences, so the transform is XOR-like,
# but the recovered stream still slips by a byte here and there, so the real
# keystream is not this constant.  Treat the result as a probe, not a parse.
def guess_xor_key(recs: list["Record"], n: int = 2) -> bytes:
    import collections
    cnt = [collections.Counter() for _ in range(n)]
    for r in recs[:400]:
        for i, b in enumerate(r.payload):
            cnt[i % n][b] += 1
    if not all(c for c in cnt):
        return b""
    return bytes(c.most_common(1)[0][0] for c in cnt)


# ---------------------------------------------------------------------------
# main
# ---------------------------------------------------------------------------

def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("path")
    ap.add_argument("--commands", action="store_true", help="dump every command packet")
    ap.add_argument("--checksums", action="store_true", help="dump checksum packets")
    ap.add_argument("--seeds", action="store_true", help="dump check_random packets")
    ap.add_argument("--records", type=int, default=0, help="dump first N records")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--raw", help="write the decompressed payload here")
    ap.add_argument("--xor", help="hex XOR key to apply to record payloads")
    a = ap.parse_args()

    buf = gunzip(a.path)
    if a.raw:
        open(a.raw, "wb").write(buf)

    h = parse_header(buf)
    start, recs, plain = find_stream(buf, h.end)

    xor_key = b""
    if recs and not plain:
        xor_key = guess_xor_key(recs)
        if a.xor:
            xor_key = bytes.fromhex(a.xor.replace(" ", ""))
        if xor_key:
            for rec in recs:
                rec.payload = bytes(b ^ xor_key[i % len(xor_key)]
                                    for i, b in enumerate(rec.payload))

    tally: dict[int, int] = {}
    total_cmds = 0
    dirty = 0
    for rec in recs:
        cmds, clean = split_commands(rec.payload)
        if not clean:
            dirty += 1
        for _o, op, _n in cmds:
            tally[op] = tally.get(op, 0) + 1
            total_cmds += 1

    if a.json:
        out: dict[str, Any] = {
            "file": a.path,
            "decompressed": len(buf),
            "header": {
                "tag_game": h.tag_game, "tag_gameinfo": h.tag_gameinfo,
                "version": h.version, "gi_00": h.gi_00,
                "gi_04": h.gi_04.hex(), "gi_14": h.gi_14,
                "gi_18": h.gi_18.hex(), "gi_35": h.gi_35,
                "end": h.end,
                "players": [
                    {"index": p.index, "present": p.present, "flags": p.flags_word,
                     "name": p.name, "blob": p.blob.hex()} for p in h.players
                ],
            },
            "stream_start": start,
            "payloads_plain": plain,
            "xor_probe_key": xor_key.hex(),
            "records": len(recs),
            "commands": total_cmds,
            "opcodes": {cmd_name(op): n for op, n in sorted(tally.items())},
        }
        print(json.dumps(out, indent=1))
        return 0

    print(f"file                 {a.path}")
    print(f"decompressed         {len(buf)} bytes")
    print()
    print("== header (Game::walk_data 0x00589600 / GameInfo::walk_data 0x005d6570) ==")
    print(f"  tag(game)          {h.tag_game:#04x}")
    print(f"  tag(gameinfo)      {h.tag_gameinfo:#04x}")
    print(f"  version            {h.version!r}")
    print(f"  gi+0x00 u32        {h.gi_00:#010x}  ({h.gi_00})")
    print(f"  gi+0x04 [16]       {h.gi_04.hex(' ')}")
    print(f"          as u32     {list(struct.unpack('<4I', h.gi_04))}")
    print(f"  gi+0x14 u32        {h.gi_14:#010x}  ({h.gi_14})")
    print(f"  gi+0x18 [29]       {h.gi_18.hex(' ')}")
    print(f"  gi+0x35 u8         {h.gi_35:#04x}")
    for p in h.players:
        if p.present:
            print(f"  player[{p.index}] tag={p.tag:#04x} w={p.flags_word:#06x} "
                  f"name={p.name!r}")
            print(f"             blob {p.blob.hex(' ')}")
        else:
            print(f"  player[{p.index}] tag={p.tag:#04x} w={p.flags_word:#06x} (empty)")
    print(f"  header ends at     {h.end:#x}")
    print()

    if not recs:
        print("== command stream: NOT FOUND ==")
        return 1

    gap = start - h.end
    consumed = recs[-1].off + REC_HDR + recs[-1].size
    print("== command-package stream (writer FUN_00952fb0 / reader FUN_00952d90) ==")
    if not plain:
        print(f"  payloads           OBFUSCATED (multiplayer); probe key "
              f"{xor_key.hex(' ') if xor_key else '<none>'} applied")
    print(f"  starts at          {start:#x}   (unparsed gap after header: {gap} bytes)")
    print(f"  records            {len(recs)}")
    print(f"  ends at            {consumed:#x} of {len(buf):#x} "
          f"({len(buf) - consumed} trailing bytes)")
    print(f"  frame range        {recs[0].frame} .. {recs[-1].frame}")
    print(f"  stamp range        {recs[0].stamp} .. {recs[-1].stamp}")
    print(f"  players seen       {sorted({r.play for r in recs})}")
    print(f"  valid values       {sorted({r.valid for r in recs})}")
    print(f"  commands decoded   {total_cmds}   (records with garbage: {dirty})")
    print()
    print("  opcode histogram:")
    for op, n in sorted(tally.items(), key=lambda kv: -kv[1]):
        print(f"    {op:#04x} {cmd_name(op):<24} {n}")

    if a.records:
        print()
        print("  first records:")
        for rec in recs[:a.records]:
            cmds, _ = split_commands(rec.payload)
            names = " ".join(cmd_name(op) for _o, op, _n in cmds)
            print(f"    @{rec.off:#08x} frame={rec.frame:<6} play={rec.play} "
                  f"valid={rec.valid} stamp={rec.stamp:<6} size={rec.size:<4} {names}")

    if a.commands:
        print()
        print("  command stream:")
        for rec in recs:
            cmds, _ = split_commands(rec.payload)
            for o, op, n in cmds:
                body = rec.payload[o + 1:o + n]
                print(f"    frame={rec.frame:<6} stamp={rec.stamp:<6} play={rec.play} "
                      f"{op:#04x} {cmd_name(op):<24} {body.hex(' ')}")

    if a.checksums:
        print()
        print("  checksum packets (opcode 0x39):")
        for rec in recs:
            cmds, _ = split_commands(rec.payload)
            for o, op, _n in cmds:
                if op == 0x39:
                    cs = parse_checksum(rec.payload, o)
                    print(f"    frame={rec.frame:<6} stamp={rec.stamp:<6} play={rec.play} " +
                          " ".join(f"{k}={v:#010x}" for k, v in cs.items()))

    if a.seeds:
        print()
        print("  check_random packets (opcode 0x38):")
        prev = None
        for rec in recs:
            cmds, _ = split_commands(rec.payload)
            for o, op, _n in cmds:
                if op == 0x38:
                    seed = struct.unpack_from("<I", rec.payload, o + 1)[0]
                    note = ""
                    if prev is not None:
                        # LCG at FUN_00a39cf0: s = s*0x19660d + 0x3c6ef35f
                        s = prev
                        for k in range(1, 4097):
                            s = (s * 0x19660D + 0x3C6EF35F) & 0xFFFFFFFF
                            if s == seed:
                                note = f"  (= prev advanced {k} steps, LCG 0x19660d/0x3c6ef35f)"
                                break
                    print(f"    frame={rec.frame:<6} stamp={rec.stamp:<6} "
                          f"play={rec.play} seed={seed:#010x}{note}")
                    prev = seed

    return 0


if __name__ == "__main__":
    sys.exit(main())
