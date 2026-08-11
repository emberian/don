#!/usr/bin/env python3
"""
scoreboard.py -- the evaluation harness: what the shipped recordings say, and how a
candidate build order or bot compares against them.

The problem this exists to solve
--------------------------------
`analysis/opening.py` and `analysis/plan.py` search for an "optimal" opening over a
model whose own header lists what it omits (walking time, pathing, gather-point
distance, caravans, rare resources).  Nothing anywhere checks the result against
anything.  A search that returns a faster number every time you widen the beam is not
evidence that the model is right; it is evidence that the search works.

So the reference here is not our model.  It is 61 shipped `.rcx` recordings --
43,035 production commands from 115 player-games, read by `analysis/corpus.py`,
whose decode is cross-checked command-for-command against the independent Rust
harness in `crates/don-replay` (5,055,253 commands, 61 opcodes, zero delta).

Design rule, stated so it can be held against this file
------------------------------------------------------
**Being faster than the corpus is reported as SUSPICION, not as a win.**  A plan that
places its first Farm before any of the 115 recorded player-games did has most likely
found a modelling gap -- and `plan.py`'s own header names the gap: `walk_frames = 0`.
`score` therefore reports three numbers that can each go DOWN and one that cannot go
up at all:

    supported   fraction of the plan's actions the corpus ever performs.  A plan full
                of actions no recorded game contains scores low here and cannot be
                rescued by being fast.
    inside      fraction of the plan's timings that land inside the corpus's observed
                range for that action.
    ahead       fraction that land EARLIER than every recorded game.  This is the
                suspicion counter.  High `ahead` with high `supported` means the model
                is optimistic, not that the plan is good.

usage:
    python3 analysis/scoreboard.py types      # which shipped types real play uses
    python3 analysis/scoreboard.py openings   # the empirical opening envelope
    python3 analysis/scoreboard.py closure    # command-weighted closure ledger
    python3 analysis/scoreboard.py score      # score opening.py's plan + the shipped
                                              # economic.bhs order against the corpus
    python3 analysis/scoreboard.py all
"""

from __future__ import annotations

import json
import os
import sys
from collections import Counter, defaultdict

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)

# `Game::frame` advances 15 times per simulated second: GATHER_RATE = 450 frames is
# the 30-second income period econ.py derives from Player::TickResource 0x006CE450.
FPS = 15

DERIVED = os.path.join(HERE, "derived.json")
CORPUS = os.path.join(HERE, "corpus-commands.json")
CLOSURE = os.path.join(ROOT, "schema", "simulation-closure.json")
VALIDATION = os.path.join(ROOT, "schema", "replay-validation.json")


def closure_name(rows: list[dict], op: int) -> str:
    for r in rows:
        if r["op"] == op:
            return r["name"]
    return f"op_{op:#04x}"


def mmss(frame: float) -> str:
    s = int(frame / FPS)
    return f"{s // 60}:{s % 60:02d}"


def load() -> tuple[dict, dict]:
    if not os.path.exists(CORPUS):
        sys.exit(f"missing {CORPUS} -- run: python3 analysis/corpus.py extract")
    return json.load(open(DERIVED)), json.load(open(CORPUS))


# ---------------------------------------------------------------------------
# player-games
# ---------------------------------------------------------------------------

def player_games(corpus: dict, derived: dict):
    """(file, play) -> {'human': bool, 'rows': [(stamp, kind, tid, name)]}.

    `play` is `CommandPackage +0x04`, the slot that ISSUED the package, so a
    recording contributes one player-game per participating slot -- including the
    AI slots, whose commands go down the same lockstep stream.
    """
    names = {int(k): v for k, v in derived["slot_names"].items()}
    kinds = {int(k): v for k, v in derived["slot_kind"].items()}
    human: dict[tuple[str, int], bool] = {}
    for f in corpus["files"]:
        for p in f.get("players", []):
            human[(f["file"], p["index"])] = p["human"]
    games: dict[tuple[str, int], dict] = {}
    for file, stamp, play, kind, tid, count in corpus["rows"]:
        g = games.setdefault((file, play), {"human": human.get((file, play)), "rows": []})
        g["rows"].append((stamp, kind, tid, names.get(tid, f"?{tid}"),
                          kinds.get(tid, "?"), count))
    for g in games.values():
        g["rows"].sort(key=lambda r: r[0])
    return games


# ---------------------------------------------------------------------------
# S1 -- which shipped types real play actually exercises
# ---------------------------------------------------------------------------

def s_types(derived: dict, corpus: dict) -> None:
    print("=" * 78)
    print("S1  Which of the shipped types does real play ever produce?")
    print("=" * 78)
    names = {int(k): v for k, v in derived["slot_names"].items()}
    kinds = {int(k): v for k, v in derived["slot_kind"].items()}

    used = Counter()
    for _file, _stamp, _play, _kind, tid, count in corpus["rows"]:
        used[tid] += count

    by_kind_total = Counter(kinds.values())
    by_kind_used = Counter(kinds.get(t, "?") for t in used)
    print(f"  shipped type slots        {len(names)}")
    for k in ("unit", "building", "tech"):
        print(f"    {k:<10} exercised {by_kind_used[k]:4d} / {by_kind_total[k]:4d} "
              f"({by_kind_used[k] / by_kind_total[k]:6.1%})")
    print(f"  total exercised           {len(used)} / {len(names)} "
          f"({len(used) / len(names):.1%})")
    print(f"  production commands       {len(corpus['rows'])}, "
          f"{sum(used.values())} units/buildings/techs ordered")

    # The graft-variant repair, quantified: what the OLD 449-entry slot_names lost.
    print("\n  Commands whose type id resolves only because slot_names was completed:")
    canon = set()
    for d in ("units", "buildings", "techs"):
        canon |= {rec["slot"] for rec in derived[d].values()}
    graft = {t: c for t, c in used.items() if t not in canon}
    print(f"    {sum(graft.values())} of {sum(used.values())} ordered "
          f"({sum(graft.values()) / sum(used.values()):.1%}) over {len(graft)} type ids")
    for t, c in sorted(graft.items(), key=lambda kv: -kv[1])[:8]:
        print(f"      slot {t:4d} {names[t]:<26} {c:6d}")

    print("\n  Top 20 exercised types:")
    for t, c in used.most_common(20):
        print(f"    {t:4d} {kinds.get(t, '?'):<9} {names.get(t, '?'):<28} {c:7d}")

    never = [t for t in sorted(names) if t not in used]
    print(f"\n  NEVER produced in any of the {len(corpus['files'])} recordings: "
          f"{len(never)} type slots")
    for k in ("unit", "building", "tech"):
        n = [t for t in never if kinds.get(t) == k]
        print(f"    {k:<10} {len(n):4d}   e.g. " +
              ", ".join(names[t] for t in n[:6]))
    print("\n  READ THIS AS: a bound on what the corpus can evaluate, not as a claim")
    print("  that the unused types are unimportant. 61 recordings of one community's")
    print("  multiplayer games are not a sample of all Rise of Nations play.")


# ---------------------------------------------------------------------------
# S2 -- the empirical opening envelope
# ---------------------------------------------------------------------------

OPENING_HORIZON = 15 * 60 * 12   # 12 minutes


def first_occurrence(games) -> dict[str, list[int]]:
    """type name -> sorted list of first-issue frames, one per player-game that did it."""
    out: dict[str, list[int]] = defaultdict(list)
    for g in games.values():
        seen = set()
        for stamp, _kind, _tid, name, _k, _c in g["rows"]:
            if stamp > OPENING_HORIZON or name in seen:
                continue
            seen.add(name)
            out[name].append(stamp)
    for v in out.values():
        v.sort()
    return out


def pct(sorted_vals: list[int], q: float) -> int:
    if not sorted_vals:
        return -1
    i = min(len(sorted_vals) - 1, max(0, int(round(q * (len(sorted_vals) - 1)))))
    return sorted_vals[i]


def s_openings(derived: dict, corpus: dict) -> None:
    print("=" * 78)
    print("S2  The empirical opening envelope (first 12:00, per player-game)")
    print("=" * 78)
    games = player_games(corpus, derived)
    active = {k: v for k, v in games.items() if v["rows"]}
    humans = {k: v for k, v in active.items() if v["human"]}
    ais = {k: v for k, v in active.items() if v["human"] is False}
    print(f"  player-games with production: {len(active)} "
          f"({len(humans)} human, {len(ais)} AI, "
          f"{len(active) - len(humans) - len(ais)} unattributed)")
    if not ais:
        print()
        print("  *** THE CORPUS CONTAINS NO AI PLAY AT ALL. ***")
        print("  Not one of the 5,055,253 commands in the 61 recordings was issued by")
        print("  a slot without the HUMAN bit, across 301 present player slots, and")
        print("  many of those recordings do contain AI players. That is what lockstep")
        print("  requires: an AI is simulated identically on every client from shared")
        print("  state, so its decisions are never transmitted and are never recorded.")
        print("  CONSEQUENCE: this corpus can evaluate an economy or an opening against")
        print("  HUMAN play. It cannot be used to recover, imitate or score retail's AI.")
        print("  Any plan to mine retail AI behaviour out of .rcx files is impossible in")
        print("  principle, not merely hard. `docs/tracks/ron-ai-impl.md`'s 306 compiled")
        print("  functions are the only place that behaviour exists.")

    for label, sub in (("ALL", active), ("HUMAN", humans), ("AI", ais)):
        fo = first_occurrence(sub)
        if not fo:
            continue
        print(f"\n  {label}: first-issue frame by type "
              f"(n = player-games that ever issued it, of {len(sub)})")
        print(f"    {'type':<26} {'n':>4}  {'p10':>7} {'p50':>7} {'p90':>7}   "
              f"{'earliest':>8}")
        rows = sorted(fo.items(), key=lambda kv: -len(kv[1]))[:18]
        for name, vals in rows:
            print(f"    {name:<26} {len(vals):4d}  {mmss(pct(vals, .10)):>7} "
                  f"{mmss(pct(vals, .50)):>7} {mmss(pct(vals, .90)):>7}   "
                  f"{mmss(vals[0]):>8}")


# ---------------------------------------------------------------------------
# S3 -- command-weighted closure
# ---------------------------------------------------------------------------

#   An opcode the engine emits on a SCHEDULE (once or twice per player per turn)
#   has a corpus count that scales with the number of turns recorded, not with
#   anything a player decided.  The corpus separates the two cleanly: five
#   opcodes sit at 0.835-2.215 commands per turn and the sixth-heaviest is at
#   0.134 -- a 6x gap.  The threshold below sits in that gap; it is measured,
#   not chosen for a nice answer, and `s_closure` prints the gap so the choice
#   can be checked.
SCHEDULED_PER_TURN = 0.5


def s_closure(corpus: dict) -> None:
    print("=" * 78)
    print("S3  Closure ledger, weighted by what the corpus actually issues")
    print("=" * 78)
    if not os.path.exists(CLOSURE):
        print("  schema/simulation-closure.json absent -- skipped")
        return
    closure = json.load(open(CLOSURE))
    totals = json.load(open(VALIDATION))["totals"]
    counts = {int(k, 16): v["count"] for k, v in totals["opcode_counts"].items()}
    turns = totals["turns"]
    rows = closure["domains"]["opcodes"]

    rate = {op: c / turns for op, c in counts.items()}
    scheduled = {op for op, r in rate.items() if r >= SCHEDULED_PER_TURN}
    decision = {op for op in counts if op not in scheduled}

    n = len(rows)
    done = sum(1 for r in rows if r["complete"])
    tot_w = sum(counts.get(r["op"], 0) for r in rows)
    done_w = sum(counts.get(r["op"], 0) for r in rows if r["complete"])
    dec_w = sum(counts.get(r["op"], 0) for r in rows if r["op"] in decision)
    dec_done = sum(counts.get(r["op"], 0) for r in rows
                   if r["op"] in decision and r["complete"])

    print(f"  corpus: {turns} turns, {sum(counts.values())} commands\n")
    print("  Three weightings of the same ledger. Only the third is a scoreboard.")
    print(f"    unweighted                {done}/{n} opcodes ({done / n:.1%})")
    print(f"    all-command-weighted      {done_w}/{tot_w} ({done_w / tot_w:.1%})"
          "   <- INFLATED, see below")
    print(f"    PLAYER-DECISION-weighted  {dec_done}/{dec_w} "
          f"({dec_done / dec_w:.1%})")
    print(f"\n  Why the middle number is not a scoreboard: {len(scheduled)} opcodes are")
    print("  emitted on a schedule, so their counts measure how long the recordings")
    print("  are, not what anyone did. They are "
          f"{sum(counts[o] for o in scheduled) / sum(counts.values()):.1%} of all commands:")
    print(f"    {'op':>5} {'struct':<26} {'count':>10} {'per turn':>9}  complete")
    by_rate = sorted(counts, key=lambda o: -rate[o])
    status = {r["op"]: r["complete"] for r in rows}
    for op in by_rate[:7]:
        mark = "<-- threshold" if op in scheduled and rate[by_rate[
            by_rate.index(op) + 1]] < SCHEDULED_PER_TURN else ""
        name = closure_name(rows, op)
        print(f"    {op:#05x} {name:<26} {counts[op]:10d} {rate[op]:9.3f}  "
              f"{str(status.get(op)):<5} {mark}")
    print(f"  the gap: last scheduled {min(rate[o] for o in scheduled):.3f}/turn, "
          f"first decision {max(rate[o] for o in decision):.3f}/turn")
    print("\n  The unweighted number treats MarwanCommand and GroupCommand as equal.")
    print("  These are the heaviest opcodes the corpus issues that are still red:")
    print(f"    {'op':>5} {'struct':<26} {'corpus commands':>16}  bridge_status")
    red = [r for r in rows if not r["complete"] and counts.get(r["op"], 0) > 0]
    red.sort(key=lambda r: -counts.get(r["op"], 0))
    for r in red[:12]:
        print(f"    {r['op']:#05x} {r['name']:<26} {counts.get(r['op'], 0):16d}  "
              f"{r.get('bridge_status')}")
    red_dec = sum(counts.get(r["op"], 0) for r in red if r["op"] in decision)
    print(f"    ({red_dec} of the {dec_w} player-decision commands, "
          f"{red_dec / dec_w:.1%}, land on a red opcode)")
    dead = [r for r in rows if counts.get(r["op"], 0) == 0]
    dead_done = sum(1 for r in dead if r["complete"])
    print(f"\n  {len(dead)} of {n} opcodes NEVER appear in the corpus; "
          f"{dead_done} of those are already marked complete.")
    print("  Those complete-but-unexercised rows carry no replay evidence at all --")
    print("  their completeness rests entirely on the disassembly and unit tests.")


# ---------------------------------------------------------------------------
# S4 -- score a candidate plan against the corpus
# ---------------------------------------------------------------------------

def score_plan(name: str, order: list[tuple[int, str]], fo: dict[str, list[int]],
               n_games: int, verbose: bool = True) -> dict:
    """`order` is [(frame, type name)] -- the first issue of each type in the plan."""
    first: dict[str, int] = {}
    for frame, ty in order:
        first.setdefault(ty, frame)

    supported = inside = ahead = behind = 0
    rows = []
    for ty, frame in sorted(first.items(), key=lambda kv: kv[1]):
        vals = fo.get(ty)
        if not vals:
            rows.append((ty, frame, None, None, None, "UNSUPPORTED"))
            continue
        supported += 1
        lo, hi = vals[0], vals[-1]
        if frame < lo:
            ahead += 1
            verdict = "AHEAD-OF-ALL"
        elif frame > hi:
            behind += 1
            verdict = "BEHIND-ALL"
        else:
            inside += 1
            below = sum(1 for v in vals if v <= frame)
            verdict = f"p{100 * below // len(vals):02d}"
        rows.append((ty, frame, lo, pct(vals, .5), hi, verdict))

    total = len(first)
    res = {
        "plan": name,
        "actions": total,
        "supported": supported,
        "supported_frac": supported / total if total else 0.0,
        "inside": inside,
        "inside_frac": inside / total if total else 0.0,
        "ahead_of_every_recorded_game": ahead,
        "ahead_frac": ahead / total if total else 0.0,
        "behind_every_recorded_game": behind,
    }
    if verbose:
        print(f"\n  -- {name} --")
        print(f"    {'type':<24} {'plan':>7}   {'corpus min':>10} {'p50':>7} "
              f"{'max':>7}   verdict")
        for ty, frame, lo, mid, hi, verdict in rows:
            if lo is None:
                print(f"    {ty:<24} {mmss(frame):>7}   {'-':>10} {'-':>7} {'-':>7}   "
                      f"{verdict}")
            else:
                print(f"    {ty:<24} {mmss(frame):>7}   {mmss(lo):>10} {mmss(mid):>7} "
                      f"{mmss(hi):>7}   {verdict}")
        print(f"    supported {supported}/{total} ({res['supported_frac']:.0%})   "
              f"inside {inside}/{total} ({res['inside_frac']:.0%})   "
              f"AHEAD-OF-ALL {ahead}/{total} ({res['ahead_frac']:.0%})   "
              f"behind {behind}")
    return res


def s_score(derived: dict, corpus: dict, beam: int = 900) -> None:
    print("=" * 78)
    print("S4  Scoring our candidate openings against the corpus")
    print("=" * 78)
    games = player_games(corpus, derived)
    active = {k: v for k, v in games.items() if v["rows"]}
    fo = first_occurrence(active)
    print(f"  reference: {len(active)} player-games, "
          f"{len(fo)} distinct types with a first-issue distribution")

    sys.path.insert(0, HERE)
    import opening  # noqa: PLC0415 -- heavy; only imported when scoring

    results = []

    # 1. the shipped designers' order, economic.bhs cases 6..18
    s, fail = opening.run_order(opening.SHIPPED_ORDER)
    if s is None:
        print(f"  economic.bhs order stalled at {fail}; not scored")
    else:
        order = [(st, n) for st, _dn, n in s.log]
        results.append(score_plan("economic.bhs (the shipped AI opening)", order,
                                  fo, len(active)))

    # 2. our beam search
    r = opening.search(opening.W(), opening.boom_target, beam=beam)
    if not r:
        print("  opening.py search found no plan; not scored")
    else:
        _f, st = r
        order = [(a, n) for a, _dn, n in st.log]
        results.append(score_plan(f"opening.py beam search (beam={beam})", order,
                                  fo, len(active)))

    print("\n  " + "-" * 74)
    print(f"  {'plan':<44} {'supported':>10} {'inside':>8} {'AHEAD':>8}")
    for res in results:
        print(f"  {res['plan']:<44} {res['supported_frac']:>9.0%} "
              f"{res['inside_frac']:>7.0%} {res['ahead_frac']:>7.0%}")
    print("\n  AHEAD is not a score to maximise. plan.py declares walk_frames = 0,")
    print("  build_linear = True and caravan_wealth = 0; every one of those makes the")
    print("  model faster than a real game. A candidate that is AHEAD of every")
    print(f"  one of the {len(active)} player-games on an action is evidence about the")
    print("  model, not about the plan.")


def main() -> int:
    derived, corpus = load()
    which = sys.argv[1] if len(sys.argv) > 1 else "all"
    if which in ("types", "S1", "all"):
        s_types(derived, corpus)
    if which in ("openings", "S2", "all"):
        print()
        s_openings(derived, corpus)
    if which in ("closure", "S3", "all"):
        print()
        s_closure(corpus)
    if which in ("score", "S4", "all"):
        print()
        s_score(derived, corpus)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
