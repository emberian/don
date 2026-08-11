#!/usr/bin/env python3
"""
corpus.py -- read the whole `.rcx` recorded-game corpus into a normalised command
table, in Python, fast enough to iterate on.

Why this exists
---------------
`analysis/replay_order.py` shells out to `re/scripts/rcx_parse.py --commands` once per
file and re-parses its human-readable text.  That is fine for one single-player
recording and useless for the corpus: `rcx_parse.py` cannot read a *multiplayer*
payload at all (its own header says the obfuscation "is not fully derived", and its
`find_stream` is an O(n) rescan per candidate offset).  Every multiplayer recording in
`ron-data/replays/multi/` -- which is the whole corpus bar one file -- was therefore
invisible to `analysis/`.

It is no longer undecoded.  `crates/don-net/src/obfuscate.rs` derived the transform
from `CommandPackage::process_all` 0x0094c500 and `crates/don-replay` reads all 61
recordings with it.  This module is a faithful Python port of that path so the
analysis pillar can use the corpus without a Cargo build.

Provenance of every constant here (all Tier C, read off riseofnations.exe):

  find_stream            docs/derivation/replay-stream.md §2 / crates/don-net/src/stream.rs
  18-byte record framing CommandManager record writer FUN_00952fb0
  payload XOR            CommandPackage::process_all 0x0094c500, key = G >> 8
  inter-command padding  Random::get(0,2) 0x00a39d70 on a STACK-LOCAL Random seeded
                         with G, so the pad sequence restarts every package
  LCG constants          0x0019660D / 0x3C6EF35F, read at 0x00a39ea5
  command wire sizes     crates/don-net/src/opcodes.rs, GENERATED from rise.pdb
                         (`sizeof(<X>Command)`); variable-length rules from
                         `Command::wire_len` (process_group 0x0094a6f3,
                         process_spline 0x00945396, process_chat 0x009458b4)

Nothing in this file is a guess.  The `verify` subcommand exists precisely so the port
can be refuted: it recomputes the corpus-wide opcode histogram and diffs it against
`schema/replay-validation.json`, which was produced by the independent Rust harness.

usage:
    python3 analysis/corpus.py verify            # cross-check against the Rust harness
    python3 analysis/corpus.py extract           # write analysis/corpus-commands.json
    python3 analysis/corpus.py summary
"""

from __future__ import annotations

import glob
import json
import os
import struct
import sys
import zlib
from dataclasses import dataclass, field

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)

REC_HDR = 18

# --------------------------------------------------------------------------
# command wire sizes -- transcribed from crates/don-net/src/opcodes.rs, which is
# itself GENERATED from `sizeof(<X>Command)` in rise.pdb.  `None` = variable.
# --------------------------------------------------------------------------
COMMAND_SIZES: list[int | None] = [
    None, 1, 5, 13, 17, 13, 17, 22, 26, 10, 10, 25, 1, 1, 5, 9,
    13, 21, 9, 9, 13, 5, 17, 21, 9, 25, 17, 1, 25, 1, 13, 13,
    9, 9, 25, 1, 1, 13, 13, 9, 9, 9, 9, 17, 17, 17, 13, 13,
    15, 11, 9, None, 5, 1, 1, 1, 5, 65, 6, 5, 5, 5, 1, 1,
    1, 5, 5, 17, None, 9, 5, 7, 10, 33, 11, 53, 2, 2, 521, 9,
    3, 2,
]

COMMAND_STRUCTS = [
    "GroupCommand", "BeginCommand", "StanceCommand", "FormCommand", "AttackCommand",
    "SiegeAttackCommand", "SwarmAroundCommand", "MoveToCommand", "MoveNearCommand",
    "AttackGroundCommand", "PatrolCommand", "LaunchPatrolCommand", "HaltCommand",
    "TransportCommand", "SetTransportCommand", "BoardShipCommand", "RepairCommand",
    "TradeCommand", "CityGatherCommand", "GatherCommand", "GarrisonCommand",
    "DisbandCommand", "GatherPointCommand", "SpellCommand", "QueueUpCommand",
    "BuildCommand", "EjectAllCommand", "AlarmCommand", "FlightCommand",
    "StopSpellCommand", "FollowCommand", "GuardCommand", "UnitmaskCommand",
    "BuildmaskCommand", "HotKeyCommand", "RecallCommand", "ScrambleCommand",
    "TreatyCommand", "DeclareCommand", "ClearTributesCommand", "ClearAllCommand",
    "AcceptCommand", "RejectCommand", "TributeCommand", "DemandTributeCommand",
    "ProposeAttackCommand", "BuyCommand", "SellCommand", "UnqueueCommand",
    "ComeOutCommand", "PingCommand", "SplineCommand", "SpeedSetCommand",
    "SpeedUpCommand", "SpeedDownCommand", "MPLogCommand", "CheckRandomCommand",
    "CheckSumsCommand", "NextCheckSumCommand", "CheatViewAllCommand",
    "CheatGiveTechsCommand", "CheatZeroTechsCommand", "CheatAISpeedIncreaseCommand",
    "CheatAISpeedNormalCommand", "CheatAIToggleCommand", "CheatIncreaseBucketsCommand",
    "CheatZeroBucketsCommand", "CheatInitUnitCommand", "ChatCommand", "ChatSetCommand",
    "ResignCommand", "QuitCommand", "CameraCommand", "LeaderOptionsCommand",
    "TurnDataCommand", "RenameCityCommand", "PauseCommand", "CannonTimeCommand",
    "ConsoleCmdCommand", "PlayerSpeedCommand", "UngracefulPlayerDrop", "MarwanCommand",
]

QUEUE_UP = 0x18
BUILD = 0x19
UNQUEUE = 0x30

# `Sim` / `Lockstep` / `Presentation` -- the classification don-replay's report uses.
LOCKSTEP_OPS = {0x39, 0x3A}
PRESENTATION_OPS = {0x44, 0x48}


class DecodeError(Exception):
    pass


def wire_len(buf: memoryview | bytes, i: int, n: int) -> int:
    """Length of the command packet at buf[i:].  Raises on truncation/unknown op."""
    if i >= n:
        raise DecodeError("truncated at opcode")
    op = buf[i]
    if op == 0x00:  # process_group  0x0094a6f3   lea eax,[eax*2+3]
        if i + 2 > n:
            raise DecodeError("truncated group count")
        return 3 + 2 * buf[i + 1]
    if op == 0x33:  # process_spline 0x00945396   lea esi,[eax*8+6]
        if i + 6 > n:
            raise DecodeError("truncated spline count")
        return 6 + 8 * struct.unpack_from("<H", buf, i + 4)[0]
    if op == 0x44:  # process_chat   0x009458b4   lea esi,[eax*2+0x13]
        if i + 17 > n:
            raise DecodeError("truncated chat length")
        ln = struct.unpack_from("<i", buf, i + 13)[0]
        if not (0 <= ln <= 512):
            raise DecodeError(f"chat length {ln} outside the 512-byte send buffer")
        return 19 + 2 * ln
    if op >= len(COMMAND_SIZES) or COMMAND_SIZES[op] is None:
        raise DecodeError(f"unknown opcode {op:#04x}")
    return COMMAND_SIZES[op]  # type: ignore[return-value]


# --------------------------------------------------------------------------
# Random::get(0, 2)  --  0x00a39d70, instructions at 0x00a39ea5
# --------------------------------------------------------------------------
LCG_A = 0x0019660D
LCG_C = 0x3C6EF35F
MASK32 = 0xFFFFFFFF


class PadRandom:
    __slots__ = ("seed",)

    def __init__(self, seed: int):
        self.seed = seed & MASK32

    def next_pad(self) -> int:
        # in_range(0, 2): lo != hi, so the draw always advances the state.
        self.seed = (self.seed * LCG_A + LCG_C) & MASK32
        return ((self.seed & 0xFFFF) * 2) >> 16


def xor_payload(b: bytearray, key: int) -> None:
    """XOR whole u16 words.  A trailing odd byte is left alone (size/2 word count)."""
    n = len(b) & ~1
    if n == 0:
        return
    a = np.frombuffer(bytes(b[:n]), dtype="<u2").copy()
    a ^= np.uint16(key)
    b[:n] = a.tobytes()


def rank_xor_keys(payloads: list[bytes], n: int = 12) -> list[int]:
    """Rank candidate keys by ciphertext u16 frequency.  0 ^ key == key dominates."""
    cnt: dict[int, int] = {}
    for p in payloads:
        m = len(p) & ~1
        if m == 0:
            continue
        for w, c in zip(*np.unique(np.frombuffer(p[:m], dtype="<u2"), return_counts=True)):
            cnt[int(w)] = cnt.get(int(w), 0) + int(c)
    return [w for w, _ in sorted(cnt.items(), key=lambda kv: (-kv[1], kv[0]))[:n]]


# --------------------------------------------------------------------------
# stream location -- backward DP, crates/don-net/src/stream.rs
# --------------------------------------------------------------------------

def find_stream(buf: bytes) -> tuple[int, int] | None:
    """(start, record count) of the longest 18-byte-record chain that tiles to EOF."""
    n = len(buf)
    if n < REC_HDR:
        return None
    a = np.frombuffer(buf, dtype=np.uint8)
    hi = n - REC_HDR  # inclusive last candidate offset

    # u32at(o+4) < 8  <=>  a[o+4] < 8 and a[o+5..o+7] == 0
    ok = (a[4:hi + 5] < 8) & (a[5:hi + 6] == 0) & (a[6:hi + 7] == 0) & (a[7:hi + 8] == 0)
    lens = (a[16:hi + 17].astype(np.uint32) | (a[17:hi + 18].astype(np.uint32) << 8))
    ok &= lens <= 4000
    ok &= (np.arange(hi + 1, dtype=np.int64) + REC_HDR + lens) <= n

    cand = np.flatnonzero(ok)
    if cand.size == 0:
        return None
    stamps = np.frombuffer(buf[: (n // 4) * 4], dtype="<u4")

    def u32at(o: int) -> int:
        return int(struct.unpack_from("<I", buf, o)[0])

    count: dict[int, int] = {}
    best_c, best_o = 0, -1
    for o in cand[::-1]:
        o = int(o)
        nxt = o + REC_HDR + int(lens[o])
        if nxt == n:
            c = 1
        else:
            cn = count.get(nxt, 0)
            if cn == 0:
                continue
            sa, sb = u32at(o), u32at(nxt)
            if sb < sa or sb - sa > 200:
                continue
            c = cn + 1
        count[o] = c
        if c > best_c:
            best_c, best_o = c, o
    del stamps
    if best_o < 0:
        return None
    return best_o, best_c


# --------------------------------------------------------------------------
# replay
# --------------------------------------------------------------------------

@dataclass
class Package:
    off: int
    stamp: int
    play: int
    valid: int
    frame: int
    payload: bytes


@dataclass
class Player:
    index: int
    flags: int
    present: bool
    name: str

    # Bit 2 of the header's player flags word.  op-life derived
    # `victory_score::leader_flag::HUMAN == 4` independently from
    # `DropControl::process_drop` 0x00959500, which hands a leader to the AI by
    # clearing exactly this bit.  Here it is cross-checked against the shipped
    # default AI name: see `corpus.py human_flag_check`.
    HUMAN = 0x4

    @property
    def human(self) -> bool:
        return bool(self.flags & Player.HUMAN)


@dataclass
class Replay:
    path: str
    name: str
    stream_start: int
    packages: list[Package]
    xor_key: int
    pad_seed: int | None
    decoded: int
    plains: list[bytes] = field(default_factory=list)
    players: list[Player] = field(default_factory=list)

    @property
    def multiplayer(self) -> bool:
        return self.pad_seed is not None

    def commands(self):
        """Yield (stamp, play, opcode, body) for every decodable package.

        `body` excludes the opcode byte.  Packages that do not decode cleanly are
        skipped and counted by `self.decoded`; they are never partially emitted,
        because a mid-package desync would attribute bytes to the wrong command.
        """
        for pkg, plain in zip(self.packages, self.plains):
            rng = PadRandom(self.pad_seed) if self.pad_seed is not None else None
            i, n = 0, len(plain)
            out = []
            try:
                while i < n:
                    ln = wire_len(plain, i, n)
                    if i + ln > n:
                        raise DecodeError("truncated body")
                    out.append((plain[i], plain[i + 1:i + ln]))
                    i += ln
                    if rng is not None:
                        i += rng.next_pad()
                if i != n:
                    raise DecodeError("residual")
            except DecodeError:
                continue
            for op, body in out:
                yield pkg.stamp, pkg.play, op, body


def _read_records(buf: bytes, start: int) -> list[Package]:
    out = []
    p, n = start, len(buf)
    while p + REC_HDR <= n:
        frame, play, valid, stamp = struct.unpack_from("<4I", buf, p)
        size = struct.unpack_from("<H", buf, p + 16)[0]
        if p + REC_HDR + size > n:
            break
        out.append(Package(p, stamp, play, valid, frame,
                           buf[p + REC_HDR:p + REC_HDR + size]))
        p += REC_HDR + size
    return out


def _decodes(plain: bytes, seed: int | None) -> bool:
    rng = PadRandom(seed) if seed is not None else None
    i, n = 0, len(plain)
    try:
        while i < n:
            ln = wire_len(plain, i, n)
            if i + ln > n:
                return False
            i += ln
            if rng is not None:
                i += rng.next_pad()
    except DecodeError:
        return False
    return i == n


def _recover(pkgs: list[Package]) -> tuple[int, int | None, list[bytes], int]:
    """Mirror of don-replay's recover_obfuscation.

    Only the low 16 bits of the pad seed can change a draw, and bits 8..15 are
    pinned by the XOR key (key = G >> 8), so 256 candidates remain per key.
    """
    PROBE = 48
    raw = [p.payload for p in pkgs]
    best: tuple[int, int, int | None, list[bytes]] | None = None
    for key in rank_xor_keys(raw, 12):
        plains = []
        for p in raw:
            b = bytearray(p)
            xor_payload(b, key)
            plains.append(bytes(b))

        def score(seed: int | None, limit: int) -> int:
            return sum(1 for pl in plains[:limit] if _decodes(pl, seed))

        probe = min(PROBE, len(plains))
        shortlist: list[int | None] = [None]
        probe_best = score(None, probe)
        for lo in range(256):
            s = ((key & 0xFF) << 8) | lo
            k = score(s, probe)
            if k > probe_best:
                probe_best, shortlist = k, [s]
            elif k == probe_best and k > 0:
                shortlist.append(s)
        winner: int | None = None
        best_n = 0
        for cand in shortlist:
            k = score(cand, len(plains))
            if k > best_n:
                best_n, winner = k, cand
        if best is None or best_n > best[0]:
            best = (best_n, key, winner, plains)
        if best_n == len(plains):
            break
    assert best is not None
    return best[1], best[2], best[3], best[0]


def load_payload(path: str) -> bytes:
    """Decompress a `.rcx`, or take it raw.

    `CommandManager::finish_recording` FUN_00952b40 only gzips the temp file at
    *game end*; a recording whose game never ended normally is left as the raw
    uncompressed stream.  Three of the corpus's recordings are in that state.
    Mirrors `crates/don-replay/src/replay.rs::load_payload`, which tests the
    two-byte gzip magic for the same reason.
    """
    raw = open(path, "rb").read()
    if len(raw) < 2:
        raise DecodeError(f"{path}: too short")
    if raw[0] != 0x1F or raw[1] != 0x8B:
        return raw
    return zlib.decompress(raw, 16 + zlib.MAX_WBITS)


def open_replay(path: str) -> Replay:
    payload = load_payload(path)
    loc = find_stream(payload)
    if loc is None:
        raise DecodeError(f"{path}: no command stream")
    start, _ = loc
    pkgs = _read_records(payload, start)
    if not pkgs:
        raise DecodeError(f"{path}: empty stream")
    key, seed, plains, decoded = _recover(pkgs)
    return Replay(path=path, name=os.path.basename(path), stream_start=start,
                  packages=pkgs, xor_key=key, pad_seed=seed, decoded=decoded,
                  plains=plains, players=parse_players(payload))


def parse_players(payload: bytes) -> list[Player]:
    """Player slots out of the `.rcx` SaveGame header, via re/scripts/rcx_parse.py.

    Kept as a call into the existing derived parser rather than a second
    transcription of `GameInfo::walk_data`; only the header is read, so the
    stale multiplayer-payload note in that module does not apply.
    """
    sys.path.insert(0, os.path.join(ROOT, "re", "scripts"))
    import rcx_parse  # noqa: PLC0415 -- optional, and only for the header

    h = rcx_parse.parse_header(payload)
    return [Player(index=p.index, flags=p.flags_word, present=p.present, name=p.name)
            for p in h.players]


def corpus_files() -> list[str]:
    """The DISTINCT recordings.  `web/public/data/replays/` is a byte-identical copy
    of `ron-data/replays/multi/`, so globbing both double-counts every game."""
    files = sorted(glob.glob(os.path.join(ROOT, "ron-data", "replays", "multi", "*.rcx")))
    solo = os.path.join(ROOT, "ron-data", "replays", "today.rcx")
    if os.path.exists(solo):
        files.append(solo)
    return files


# --------------------------------------------------------------------------
# payload accessors -- offsets from the PDB command structs
# --------------------------------------------------------------------------

def queue_up(body: bytes) -> tuple[int, int]:
    """QueueUpCommand 0x18 (sizeof 9): u32 type_id, u32 count."""
    return struct.unpack_from("<II", body, 0)


def build_cmd(body: bytes) -> tuple[int, int, int, int, int, int]:
    """BuildCommand 0x19 (sizeof 25): i32 x, y, x2, y2, u32 type_id, u32 count."""
    x, y, x2, y2, tid, cnt = struct.unpack_from("<iiiiII", body, 0)
    return x, y, x2, y2, tid, cnt


# --------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------

def _histogram(files: list[str], quiet: bool = False):
    from collections import Counter
    ops = Counter()
    per_file = []
    for f in files:
        try:
            r = open_replay(f)
        except Exception as e:  # noqa: BLE001 -- a file that will not open is data
            per_file.append({"file": os.path.basename(f), "error": str(e)})
            if not quiet:
                print(f"  skip {os.path.basename(f)}: {e}", file=sys.stderr)
            continue
        c = Counter()
        for _stamp, _play, op, _body in r.commands():
            c[op] += 1
        ops.update(c)
        per_file.append({
            "file": r.name,
            "packages": len(r.packages),
            "decoded": r.decoded,
            "xor_key": r.xor_key,
            "pad_seed": r.pad_seed,
            "commands": sum(c.values()),
        })
        if not quiet:
            frac = r.decoded / len(r.packages)
            print(f"  {r.name[:52]:54s} pkgs={len(r.packages):7d} "
                  f"decoded={frac:6.2%} key={r.xor_key:#06x} "
                  f"seed={'-' if r.pad_seed is None else hex(r.pad_seed)}")
    return ops, per_file


def cmd_verify() -> int:
    """Refutation test: does this port reproduce the Rust harness's histogram?"""
    files = corpus_files()
    print(f"corpus: {len(files)} distinct recordings")
    ops, per_file = _histogram(files)
    ref_path = os.path.join(ROOT, "schema", "replay-validation.json")
    ref = json.load(open(ref_path))["totals"]["opcode_counts"]
    ref_ops = {int(k, 16): v["count"] for k, v in ref.items()}

    keys = sorted(set(ops) | set(ref_ops))
    bad = 0
    print(f"\n{'op':>5} {'struct':<28} {'this port':>12} {'don-replay':>12}  delta")
    for op in keys:
        a, b = ops.get(op, 0), ref_ops.get(op, 0)
        if a != b:
            bad += 1
        mark = "" if a == b else "   <-- MISMATCH"
        name = COMMAND_STRUCTS[op] if op < len(COMMAND_STRUCTS) else "?"
        print(f"{op:#05x} {name:<28} {a:12d} {b:12d}  {a - b:+d}{mark}")
    tot_a, tot_b = sum(ops.values()), sum(ref_ops.values())
    print(f"\ntotal {tot_a} vs {tot_b} ({tot_a - tot_b:+d}); {bad} opcode(s) differ")
    ok = sum(f.get("decoded", 0) for f in per_file)
    tot = sum(f.get("packages", 0) for f in per_file)
    print(f"packages decoded: {ok}/{tot} ({ok / tot:.4%})" if tot else "no packages")
    return 0 if bad == 0 else 1


def cmd_extract() -> int:
    """Write the production/build command table the scoreboard consumes."""
    files = corpus_files()
    rows = []
    meta = []
    for f in files:
        try:
            r = open_replay(f)
        except Exception as e:  # noqa: BLE001
            print(f"  skip {os.path.basename(f)}: {e}", file=sys.stderr)
            continue
        n = 0
        last = 0
        for stamp, play, op, body in r.commands():
            last = max(last, stamp)
            if op == QUEUE_UP and len(body) >= 8:
                tid, cnt = queue_up(body)
                rows.append([r.name, stamp, play, "queue", tid, cnt])
                n += 1
            elif op == BUILD and len(body) >= 24:
                _x, _y, _x2, _y2, tid, cnt = build_cmd(body)
                rows.append([r.name, stamp, play, "build", tid, cnt])
                n += 1
        meta.append({"file": r.name, "packages": len(r.packages),
                     "decoded": r.decoded, "multiplayer": r.multiplayer,
                     "last_stamp": last, "production_commands": n,
                     "players": [{"index": p.index, "flags": p.flags,
                                  "present": p.present, "name": p.name,
                                  "human": p.human}
                                 for p in r.players if p.present]})
        print(f"  {r.name[:52]:54s} {n:6d} production commands, last stamp {last}")
    out = os.path.join(HERE, "corpus-commands.json")
    with open(out, "w") as fh:
        json.dump({
            "_provenance": {
                "reader": "analysis/corpus.py (port of crates/don-net + crates/don-replay)",
                "files": "ron-data/replays/multi/*.rcx + ron-data/replays/today.rcx",
                "columns": ["file", "stamp", "play", "kind", "type_id", "count"],
                "kind": "queue = QueueUpCommand 0x18; build = BuildCommand 0x19",
                "stamp": "CommandPackage +0x00, the simulation frame; 15 frames = 1 s",
            },
            "files": meta,
            "rows": rows,
        }, fh)
    print(f"\nwrote {out}: {len(rows)} rows over {len(meta)} recordings")
    return 0


def shipped_ai_names() -> set[str]:
    """The shipped skirmish AI personality names.

    `ron-data/bhs-corpus/scenario/Custom/skirmish/scen_script_loc.xml`, the
    `$S(...)` localisation table the skirmish script binds leader display names
    from.  Eight strings, parsed with a real parser (a `<STRING hash=...>` regex
    is off by eight from ordinal 5950 in the other shipped table -- standing
    board finding).
    """
    import xml.etree.ElementTree as ET  # noqa: PLC0415
    p = os.path.join(ROOT, "ron-data", "bhs-corpus", "scenario",
                     "Custom", "skirmish", "scen_script_loc.xml")
    if not os.path.exists(p):
        return set()
    return {e.text for e in ET.parse(p).getroot().iter("STRING") if e.text}


def cmd_human_flag_check() -> int:
    """Three falsifiable predictions about the header player-flags word.

    op-life derived `victory_score::leader_flag::HUMAN == 4` from
    `DropControl::process_drop` 0x00959500, which hands a leader to the AI by
    clearing exactly that bit.  If bit 2 of this header word is the same flag:

      P1  every recording has EXACTLY ONE present slot carrying bit 1 (0x2) --
          the recorder's own slot, since a `.rcx` is written by one client.
      P2  every slot carrying bit 1 also carries bit 2: the recorder is human.
      P3  no slot carrying bit 2 is named from the shipped skirmish AI
          personality table.

    P3 is deliberately ONE-directional.  The converse ("every non-human slot is
    in that table") is false and known to be false: the corpus also shows AI
    slots named `Pharaoh Amenhotep III`, `Nachancan`, `Mehmed II Khan Gazi` and
    `King Shaka`, none of which appear anywhere in `ron-data/`.  The rest of the
    AI name pool was NOT located, so the converse is unsupported and is not
    asserted.

    A NAIVE test -- "an AI slot is called `Player <n>`" -- FAILS, and it fails in
    one direction only: 66 of 301 present slots read `flags == 0x1` while
    carrying a name like `Queen Chennamma`.  That name is a shipped string in
    scen_script_loc.xml, so the name test is what is wrong, not the flag.  P3 is
    the repaired form of it.
    """
    ai_names = shipped_ai_names()
    p1_ok = p1_bad = 0
    p2_ok = p2_bad = 0
    p3_ok = p3_bad = 0
    bad_rows = []
    for f in corpus_files():
        try:
            r = open_replay(f)
        except Exception:  # noqa: BLE001
            continue
        present = [p for p in r.players if p.present]
        local = [p for p in present if p.flags & 0x2]
        if len(local) == 1:
            p1_ok += 1
        else:
            p1_bad += 1
            bad_rows.append(("P1", r.name, f"{len(local)} slots carry 0x2"))
        for p in local:
            if p.human:
                p2_ok += 1
            else:
                p2_bad += 1
                bad_rows.append(("P2", r.name, f"slot {p.index} {hex(p.flags)} {p.name!r}"))
        for p in present:
            if not p.human:
                continue
            if p.name not in ai_names:
                p3_ok += 1
            else:
                p3_bad += 1
                bad_rows.append(("P3", r.name, f"slot {p.index} {hex(p.flags)} {p.name!r}"))
    print(f"P1  exactly one 0x2 slot per recording : {p1_ok} hold, {p1_bad} fail")
    print(f"P2  the 0x2 slot also carries 0x4      : {p2_ok} hold, {p2_bad} fail")
    print(f"P3  no 0x4 slot is shipped-AI-named     : {p3_ok} hold, {p3_bad} fail "
          f"({len(ai_names)} shipped names tested against)")
    for row in bad_rows[:20]:
        print("   FAIL", row)
    return 0 if not (p1_bad or p2_bad or p3_bad) else 1


def main() -> int:
    cmd = sys.argv[1] if len(sys.argv) > 1 else "verify"
    if cmd == "verify":
        return cmd_verify()
    if cmd == "human-flag-check":
        return cmd_human_flag_check()
    if cmd == "extract":
        return cmd_extract()
    if cmd == "summary":
        files = corpus_files()
        print(f"{len(files)} distinct recordings")
        _histogram(files)
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
