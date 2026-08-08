#!/usr/bin/env python3
"""
study.py -- runs the build-order study and prints every table in
docs/tracks/build-order-analytics.md.

    python3 analysis/study.py            # all sections
    python3 analysis/study.py S3         # one section
"""

from __future__ import annotations

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from econ import (  # noqa: E402
    B,
    BUILD_COST_FACTOR,
    COMMERCE_CAP,
    CITY_GATHER,
    FOOD,
    GATHER_RATE,
    KNOWLEDGE,
    MARKET_TAXES,
    METAL,
    OIL_RATE,
    PEASANT_RATE,
    POP_CAP,
    RAMP_MAX,
    STARTING_GOODS,
    T,
    TECH_COST_FACTOR,
    TIMBER,
    U,
    UNIT_COST_FACTOR,
    UNIVERSITY_LITERACY,
    Assumptions,
    State,
    ramped_cost,
)
from plan import Plan, beam_search, step, _finish_all, _queued  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)


def mmss(frames: float) -> str:
    t = frames / 15.0
    return f"{int(t)//60}:{int(t)%60:02d}"


def hr(title: str) -> None:
    print()
    print("=" * 78)
    print(title)
    print("=" * 78)


# ---------------------------------------------------------------------------
def S1_rates() -> None:
    hr("S1  Derived rates -- resources per GATHER_RATE period (450 frames = 30 s)")
    print(f"  GATHER_RATE            {GATHER_RATE} frames = {GATHER_RATE/15:.0f} s   [rules.xml, 0x006CE7B9]")
    print(f"  accumulator period     {GATHER_RATE*16} = GATHER_RATE<<4        [0x006CE7C1 shl ecx,4]")
    print(f"  one worked gather slot {PEASANT_RATE/256:.0f} resources / 30 s = {PEASANT_RATE/256*2:.0f} / min   [PEASANT_RATE 2560, 0x0063A1D9]")
    print(f"  one oil slot           {OIL_RATE/256:.0f} resources / 30 s               [OIL_RATE 8960, 0x0063A1C6]")
    print(f"  one city (free)        {CITY_GATHER[0]} food + {CITY_GATHER[1]} timber / 30 s     [CITY_GATHER, 0x006D5979]")
    print(f"  one Market             {MARKET_TAXES} wealth / 30 s                [MARKET_TAXES, FUN_00737B50]")
    print(f"  one University         {UNIVERSITY_LITERACY} knowledge / 30 s             [UNIVERSITY_LITERACY, FUN_00737C00]")
    print(f"  commerce clamp         {COMMERCE_CAP} per resource per 30 s")
    print(f"                         indexed by econ+0xF0 = Commerce tech level [0x006CE940]")
    print(f"  population cap         {POP_CAP} indexed by econ+0xE8 = Military level")
    print(f"  city limit             Civic level + 1                 [FUN_006D6130]")
    print(f"  starting goods         {STARTING_GOODS}   [rules.xml STARTING_GOODS]")

    print()
    print("  Costs (live type tables; COST field x cost factor 10):")
    print(f"    {'item':<22} {'food':>5} {'timb':>5} {'wlth':>5} {'know':>5} {'metl':>5}  job_time  ramp/unit")
    rows = [
        ("Citizen", U["Citizen"], UNIT_COST_FACTOR),
        ("Scholar", U["Scholar"], UNIT_COST_FACTOR),
        ("Farm", B["Farm"], BUILD_COST_FACTOR),
        ("Woodcutter's Camp", B["Woodcutter's Camp"], BUILD_COST_FACTOR),
        ("Mine", B["Mine"], BUILD_COST_FACTOR),
        ("Small City", B["Small City"], BUILD_COST_FACTOR),
        ("Library", B["Library"], BUILD_COST_FACTOR),
        ("Market", B["Market"], BUILD_COST_FACTOR),
        ("University", B["University"], BUILD_COST_FACTOR),
        ("Granary", B["Granary"], BUILD_COST_FACTOR),
    ]
    for name, rec, f in rows:
        c = [x * f for x in rec["cost"]]
        ramp = ", ".join(
            f"+{rec['support_cost'][k]}{'ftwkmo'[rec['support'][k]]}"
            for k in (0, 1)
            if rec["support"][k] >= 0 and rec["support_cost"][k]
        )
        print(f"    {name:<22} {c[0]:5d} {c[1]:5d} {c[2]:5d} {c[3]:5d} {c[4]:5d}  {rec['job_time']:8d}  {ramp}")
    print()
    print(f"    {'tech':<22} {'food':>5} {'timb':>5} {'wlth':>5} {'know':>5}  job_time")
    for name in ["Classical Age", "Written Word", "City State", "Barter",
                 "The Art of War", "Mathematics", "Coinage", "Empire", "Medieval Age"]:
        c = [x * TECH_COST_FACTOR for x in T[name]["cost"]]
        print(f"    {name:<22} {c[0]:5d} {c[1]:5d} {c[2]:5d} {c[3]:5d}  {T[name]['job_time']:8d}")


# ---------------------------------------------------------------------------
def S2_cap_scale() -> None:
    hr("S2  Falsifying the literal reading of the commerce clamp")
    print("""
  0x006CE509 compares the gross income -- which is in 1/16-resource-per-period units,
  because the accumulator's period is GATHER_RATE<<4 -- against econ[0x30+r*4], which
  0x006CE940 wrote straight from RULES.COMMERCE_CAP with NO <<4.  Two readings:

    H1  the comparison is missing a <<4; the intended cap is COMMERCE_CAP resources
        per 30 s (70 at Commerce 0)
    H2  the code means what it says; the effective cap is COMMERCE_CAP/16 resources
        per 30 s (4.375 at Commerce 0)

  H2 is testable against a shipped recorded game.  Under H2 no player can EVER bank more
  than 4.375 food per 30 s at Commerce level 0, whatever they build.
""")
    import replay_order

    path = os.path.join(ROOT, "ron-data", "replays", "today.rcx")
    rows = replay_order.extract(path)
    # Everything the recorded player queued, priced with our own cost model.
    counts = {}
    spend = [0] * 6
    last = 0
    for frame, kind, tid, name, cnt in rows:
        last = frame
        n = counts.get(name, 0)
        rec = None
        if name in U:
            rec = ramped_cost(U[name], n, UNIT_COST_FACTOR, RAMP_MAX["worker"])
        elif name in B:
            rec = ramped_cost(B[name], n, BUILD_COST_FACTOR, 0)
        elif name in T:
            rec = [c * TECH_COST_FACTOR for c in T[name]["cost"]]
        if rec:
            for r in range(6):
                spend[r] += rec[r]
            counts[name] = n + 1
    periods = last / GATHER_RATE
    print(f"  recorded game: {os.path.basename(path)}, {len(rows)} queue commands, "
          f"last at frame {last} = {mmss(last)} = {periods:.1f} gather periods")
    print(f"  priced spend  food={spend[0]}  timber={spend[1]}  wealth={spend[2]}  knowledge={spend[3]}")
    print(f"  shipped STARTING_GOODS food={STARTING_GOODS[0]} timber={STARTING_GOODS[1]}")
    need = spend[0] - STARTING_GOODS[0]
    for hyp, scale in (("H1", 16), ("H2", 1)):
        capfood = COMMERCE_CAP[0] * scale / 16.0
        maxearn = capfood * periods
        verdict = "CONSISTENT" if maxearn >= need else f"REFUTED ({need/maxearn:.1f}x over)"
        print(f"  {hyp}: cap = {capfood:7.3f} food/30s -> at most {maxearn:8.1f} food earned; "
              f"player needed >= {need:6d}   ==> {verdict}")
    ncit = sum(1 for _, _, _, n, _ in rows if n == "Citizen")
    floor = ncit * 20 - STARTING_GOODS[0]
    print()
    print("  A floor that uses NO cost model at all: the recorded player queued "
          f"{ncit} Citizens,")
    print("  and the Citizen COST field is 2 x UNIT_COST_FACTOR 10 = 20 food before any "
          "ramp, which")
    print(f"  only adds.  So food earned >= {floor}.  Under H2 the whole-game ceiling is "
          f"{COMMERCE_CAP[0]/16*periods:.0f}.")
    print(f"  Implied average food income under H1: {need/periods:.1f} per 30 s against a "
          f"cap of {COMMERCE_CAP[0]}.")
    print()
    print("  H2 is refuted by a wide margin, by an argument that does not depend on our")
    print("  cost model.  H2 would also make the engine's own global ceiling (16000 =")
    print("  1000 resources / 30 s at 0x006CE706) unreachable dead code, since")
    print("  COMMERCE_CAP maxes at 500.  The study uses H1.")
    print()
    print("  H1 is not comfortable either: the implied average is a large fraction of the")
    print("  H1 cap sustained from frame 0, which cannot literally happen.  The two ways")
    print("  out are (a) our Citizen ramp is steeper than retail's, or (b) the recorded")
    print("  game did not start from the shipped STARTING_GOODS -- a game option scales it")
    print("  (economic.bhs tests get_starting_resources(who) > 4).  Either way H1 survives")
    print("  and H2 does not.  A second oddity stays on record: DUTCH_INTEREST_CAP *is*")
    print("  shifted <<4 at 0x006CE6FC while the commerce cap it is added to is not")
    print("  (sim-economy.md 4.2 flagged the same asymmetry).")


# ---------------------------------------------------------------------------
def _menu_boom(p: Plan):
    """Only actions that can appear in a plan reaching GOAL, so the beam stays small."""
    s = p.s
    acts = []
    if s.citizens + _queued(p, "citizen") < min(s.pop_cap, GOAL["citizens"] + 1):
        acts.append(("citizen", "food"))
    if s.farms + _queued(p, "farm") < GOAL["farms"] + 1:
        acts.append(("farm",))
    if s.woodcamps + _queued(p, "wood") < GOAL["woodcamps"] + 1:
        acts.append(("wood",))
    for t in sorted(GOAL["techs"]):
        if t not in s.techs and not any(a == t for _, k, a in p.pending if k == "tech"):
            acts.append(("tech", t))
    if s.cities + _queued(p, "city") < min(s.city_limit, GOAL["cities"]):
        acts.append(("city",))
    if s.markets + _queued(p, "market") < GOAL["markets"]:
        acts.append(("market",))
    return acts


def _unmet(p: Plan) -> int:
    q = p.clone()
    _finish_all(q)
    s = q.s
    n = 0
    n += max(0, GOAL["cities"] - s.cities)
    n += max(0, GOAL["citizens"] - s.citizens)
    n += max(0, GOAL["farms"] - s.farms)
    n += max(0, GOAL["woodcamps"] - s.woodcamps)
    n += max(0, GOAL["markets"] - s.markets)
    n += len(GOAL["techs"] - s.techs)
    return n


# The target state is exactly what economic.bhs cases 6..18 build, so the shipped order
# and the searched order are compared on identical terms.
GOAL = dict(cities=2, citizens=14, farms=7, woodcamps=2, markets=1,
            techs={"Written Word", "City State", "Barter", "Classical Age"})
# economic.bhs cases 6..18, with the case numbers that produce each requirement:
#   6 Written Word | 7 City State | 8 citizens->9 | 9 farm | 10 city #2 | 11 citizens->11
#   12 farm | 13 woodcutter #2 | 14 Barter | 15 Market #1 | 16 citizens->14
#   17 farms->7 | 18 Classical Age


def _goal_reached(p: Plan) -> bool:
    q = p.clone()
    _finish_all(q)
    s = q.s
    return (
        s.cities >= GOAL["cities"]
        and s.citizens >= GOAL["citizens"]
        and s.farms >= GOAL["farms"]
        and s.woodcamps >= GOAL["woodcamps"]
        and s.markets >= GOAL["markets"]
        and GOAL["techs"] <= s.techs
    )


def _goal_frame(p: Plan) -> int:
    return max([p.s.frame] + [f for f, _, _ in p.pending])


def min_boom(assumptions: Assumptions | None = None, quiet: bool = False, width: int = 700):
    a = assumptions or Assumptions()
    start = Plan(State(a=a))
    finished, _ = beam_search(
        start,
        _menu_boom,
        # frame + a crude admissible-ish penalty per unmet requirement, so the beam is
        # not dominated by short prefixes that have simply not spent anything yet.
        score=lambda p: _goal_frame(p) + 220 * _unmet(p),
        done=_goal_reached,
        depth=26,
        width=width,
    )
    if not finished:
        return None, None
    best = min(finished, key=_goal_frame)
    return _goal_frame(best), best


def S3_min_age(assumptions: Assumptions | None = None, quiet: bool = False):
    """The headline optimisation."""
    a = assumptions or Assumptions()
    if not quiet:
        hr("S3  Fastest complete Ancient boom")
        print(f"""
  Objective: minimise the frame at which ALL of these hold --
      Classical Age, Written Word, City State, Barter researched
      >= {GOAL['cities']} cities, >= {GOAL['citizens']} citizens, >= {GOAL['farms']} farms,
      >= {GOAL['woodcamps']} woodcutter's camps, >= {GOAL['markets']} market
  which is exactly the state economic.bhs cases 6..18 build, so the shipped order and the
  searched order are being asked the same question.

  start: 1 Small City, {a.start_citizens} citizens, 3 Farms, 1 Library
  (citytemplates.xml 'small' template), STARTING_GOODS = {STARTING_GOODS}
""")
    t, best = min_boom(a, quiet=True)
    if best is None:
        if not quiet:
            print("  no plan found within the horizon")
        return None, None
    if not quiet:
        print(f"  BEST FOUND: complete at frame {t} = {mmss(t)}")
        print()
        print(f"  {'frame':>6} {'t':>6}  action")
        for f, act in best.seq:
            print(f"  {f:6d} {mmss(f):>6}  {' '.join(str(x) for x in act)}")
        p = best.clone()
        _finish_all(p)
        print()
        print(f"  end state: citizens={p.s.citizens} on={p.s.on} farms={p.s.farms} "
              f"camps={p.s.woodcamps} cities={p.s.cities} markets={p.s.markets}")
        print(f"  income (res/30s, uncapped): "
              f"{[round(x,1) for x in p.s.gross_uncapped_per_period()]}")
        print(f"  income (res/30s, clamped):  "
              f"{[round(x/16,2) for x in p.s.income_per_period()]}")
        print()
        print("  For contrast, the DEGENERATE objective 'reach the Classical Age and nothing")
        print("  else' is solved trivially: STARTING_GOODS is 200 food and the age costs 250,")
        print("  so a player who builds nothing banks the difference in about 35 s and the")
        print("  age lands at ~1:04.  Minimum-age-time on its own is not an interesting")
        print("  objective in Rise of Nations; the age is cheap and the economy is not.")
    return t, best


def S3b_sensitivity() -> None:
    hr("S3b  Sensitivity of the boom time to each assumption")
    base = Assumptions()
    W = 250   # narrower beam than S3 so the sweep is affordable; deltas use the same W
    t0, _ = min_boom(base, quiet=True, width=W)
    print(f"  (beam width {W} throughout this table, so the BASE row need not equal S3)")
    print("  Search-noise floor first: the SAME assumptions at several beam widths --")
    for w in (100, 250, 500, 700):
        tw, _ = min_boom(base, quiet=True, width=w)
        print(f"      width {w:4d} -> {mmss(tw)}")
    print("  Only deltas larger than that spread are evidence about the assumption.")
    print(f"  {'assumption':<44} {'value':>10}  {'boom done':>10}  delta")
    print(f"  {'BASE':<44} {'':>10}  {mmss(t0):>10}   --")
    variants = [
        ("slots per Woodcutter's Camp", "slots_wood", [3, 4, 6]),
        ("starting citizens", "start_citizens", [2, 3, 6, 8]),
        ("max builders on one site", "max_builders", [1, 2, 8]),
        ("building ramp ceiling (%)", "building_ramp_ceiling_pct", [200, 500]),
        ("walk overhead per build (frames)", "walk_frames", [30, 75, 150]),
        ("commerce clamp scale", "commerce_cap_scale", [8, 32]),
    ]
    for label, fieldname, vals in variants:
        for v in vals:
            a = Assumptions(**{**base.__dict__, fieldname: v})
            t, _ = min_boom(a, quiet=True, width=W)
            if t is None:
                print(f"  {label:<44} {v:>10}  {'unreachable':>10}")
            else:
                d = (t - t0) / 15.0
                print(f"  {label:<44} {v:>10}  {mmss(t):>10}  {d:+6.1f} s")


# ---------------------------------------------------------------------------
def S4_marginal_citizen() -> None:
    hr("S4  Marginal value of the Nth citizen")
    print("""
  Citizen cost (FUN_00664090 reduced): base 2 food x UNIT_COST_FACTOR 10 = 20 food, plus
  a linear ramp of SUPPORT_COST0 = 1 food per citizen already owned+queued, clamped at
  UNIT_WORKER_RAMP_MAX = 500% of base = +100.  A worked slot yields PEASANT_RATE/256 = 10
  resources per 30 s.  A new food slot usually costs a Farm: 4 timber x 10 = 40, ramping
  +4 timber per farm already owned.
""")
    print(f"  {'N':>4} {'citizen cost':>13} {'payback':>9} | "
          f"{'farm #':>7} {'farm cost':>10} {'payback':>9} | {'combined':>9}")
    for n in [1, 2, 3, 5, 8, 10, 12, 15, 20, 25, 30, 40, 50, 75, 100, 110, 120]:
        c = ramped_cost(U["Citizen"], n - 1, UNIT_COST_FACTOR, RAMP_MAX["worker"])[FOOD]
        pay = c / (PEASANT_RATE / 256) * 0.5     # minutes: 10 res per 0.5 min
        fn = min(n, 5)
        fc = ramped_cost(B["Farm"], n - 1, BUILD_COST_FACTOR, 0)[TIMBER]
        fpay = fc / (PEASANT_RATE / 256) * 0.5
        print(f"  {n:4d} {c:10d} fd {pay:8.2f}m | {n:7d} {fc:7d} tb {fpay:8.2f}m | "
              f"{pay+fpay:8.2f}m")
    print("""
  Read: a citizen alone repays its food in 1.0-6.0 minutes; a citizen PLUS the farm that
  employs it repays in 3.0-12+ minutes.  The ramp is gentle -- the binding constraint in
  the opening is not the citizen price, it is (a) the population cap, (b) FARMS_PER_CITY
  = 5, and (c) the commerce clamp below.
""")

    print("  Where the commerce clamp bites (income = 10*cities + 10*workers per 30 s):")
    print(f"  {'Commerce lvl':>12} {'cap/30s':>8} {'1 city':>9} {'2 cities':>9} {'3 cities':>9}")
    for lvl in range(4):
        cap = COMMERCE_CAP[lvl]
        row = []
        for cities in (1, 2, 3):
            w = (cap - CITY_GATHER[FOOD] * cities) / (PEASANT_RATE / 256)
            row.append(f"{w:9.1f}")
        techname = ["-", "Barter", "Coinage", "Trade"][lvl]
        print(f"  {lvl:>3} {techname:<9} {cap:8d} {''.join(row)}   (max USEFUL food workers)")


# ---------------------------------------------------------------------------
def _alloc_sweep(prelude, horizon):
    """Sweep (target citizens, food workers) under a fixed prelude of actions."""
    a = Assumptions()
    results = []
    for target in range(4, 25):
        for fw in range(0, min(target, 15) + 1):
            tw = target - fw
            p = Plan(State(a=a))
            acts = list(prelude)
            need_camps = (tw + a.slots_wood - 1) // a.slots_wood
            need_farms = max(0, fw - 3)
            acts += [("wood",)] * need_camps + [("farm",)] * need_farms
            acts += [("citizen", "food")] * (target - a.start_citizens)
            ok = True
            for act in acts:
                q = step(p, act, horizon=horizon)
                if q is None:
                    ok = False
                    break
                p = q
            if not ok:
                continue
            _finish_all(p)
            if p.s.frame < horizon:
                p.s.advance(horizon - p.s.frame)
            tot = p.s.stock[FOOD] + p.s.stock[TIMBER]
            results.append((tot, target, p))
    results.sort(reverse=True, key=lambda x: x[0])
    return results


def S5_allocation() -> None:
    hr("S5  Best citizen allocation over the first 10 minutes")
    print("""
  Fixed plan: build the woodcutter's camps and farms a given split needs, train citizens
  up to a target N, let them fill the slots.  Objective: (food + timber) banked at
  t = 10:00, because in the Ancient age every purchase is priced in exactly those two.

  Note that the food/timber split is not a free variable in this game: a citizen gathers
  whatever the slot it stands in gathers, and slots come from buildings.  The split is
  chosen by choosing buildings, and FARMS_PER_CITY_BASE = 5 caps food slots per city.
""")
    horizon = 15 * 600
    scen = [
        ("1 city, no techs", []),
        ("1 city, Barter (Commerce 1)", [("tech", "Barter")]),
        ("2 cities (City State)", [("tech", "City State"), ("city",)]),
        ("2 cities + Barter", [("tech", "City State"), ("city",), ("tech", "Barter")]),
    ]
    for label, prelude in scen:
        res = _alloc_sweep(prelude, horizon)
        if not res:
            print(f"  {label}: no feasible plan")
            continue
        print(f"  {label}")
        print(f"    {'citizens':>9} {'food wk':>8} {'timb wk':>8} {'farms':>6} {'camps':>6} "
              f"{'food@10m':>9} {'timb@10m':>9} {'sum':>8}")
        for tot, target, p in res[:4]:
            print(f"    {target:9d} {p.s.on[FOOD]:8d} {p.s.on[TIMBER]:8d} {p.s.farms:6d} "
                  f"{p.s.woodcamps:6d} {p.s.stock[FOOD]:9d} {p.s.stock[TIMBER]:9d} {tot:8d}")
        best = res[0][2]
        capw = COMMERCE_CAP[min(best.s.lvl_com, 7)]
        print(f"    commerce cap {capw}/30s; income food="
              f"{best.s.gross_uncapped_per_period()[FOOD]:.0f} timber="
              f"{best.s.gross_uncapped_per_period()[TIMBER]:.0f} (uncapped)")
        print(f"    worst in sweep: {res[-1][1]} citizens -> {res[-1][0]}")
        print()


# ---------------------------------------------------------------------------
BHS_ORDER = (
    # ron-data/ai-scripts/economic.bhs, the branch a generic nation takes on a land map
    # from a size>=2 town: case 1 sets step=6, then 6,7,8,...,18.  Citizen counts in the
    # script are TOTALS (needed_citizens = 9, 11, 14), so the deltas below assume the
    # 5-citizen start that the script's own arithmetic implies.
    [("tech", "Written Word")]                      # case 6  Science I
    + [("tech", "City State")]                      # case 7  Civic I
    + [("citizen", "timber")] * 4                   # case 8  "Build 4 Citizens -> Timber" (to 9)
    + [("farm",)]                                   # case 9  farm #4
    + [("city",)]                                   # case 10 City #2
    + [("citizen", "timber")] * 2                   # case 11 to 11
    + [("farm",)]                                   # case 12 farm #5
    + [("wood",)]                                   # case 13 Woodcutter #2
    + [("tech", "Barter")]                          # case 14 Commerce I
    + [("market",)]                                 # case 15 Market #1
    + [("citizen", "timber")] * 3                   # case 16 to 14
    + [("farm",)] * 2                               # case 17 farms to 7
    + [("tech", "Classical Age")]                   # case 18
)


def S6_bhs() -> None:
    hr("S6  The shipped AI's boom order (economic.bhs) run through the same model")
    print("""
  Transcribed from ron-data/ai-scripts/economic.bhs, the branch a generic nation takes on
  a land map from a size>=2 town: step 1 sets step=6, then 6,7,8,...  Case numbers are in
  the trace.  Steps that depend on nation, map style or being attacked are dropped.
""")
    a = Assumptions()
    p = Plan(State(a=a))
    for act in BHS_ORDER:
        q = step(p, act)
        if q is None:
            print(f"  !! could not schedule {act}")
            break
        p = q
    _finish_all(p)
    print(f"  {'frame':>6} {'t':>6}  action")
    for f, act in p.seq:
        print(f"  {f:6d} {mmss(f):>6}  {' '.join(str(x) for x in act)}")
    age_f = None
    for f, act in p.seq:
        if act == ("tech", "Classical Age"):
            age_f = f + T["Classical Age"]["job_time"]
    print()
    print(f"  Classical Age completes at {mmss(age_f)}" if age_f else "  (Classical Age not reached)")
    print(f"  end: citizens={p.s.citizens} on={p.s.on} farms={p.s.farms} camps={p.s.woodcamps} "
          f"cities={p.s.cities} markets={p.s.markets}")
    print(f"  income (res/30s, uncapped): {[round(x,1) for x in p.s.gross_uncapped_per_period()]}")
    print(f"  income (res/30s, clamped):  {[round(x/16,2) for x in p.s.income_per_period()]}")

    t_opt, best = S3_min_age(a, quiet=True)
    print()
    print(f"  optimum found by beam search: {mmss(t_opt)}")
    if age_f:
        print(f"  the shipped order is {(age_f - t_opt)/15:.0f} s slower to the Classical Age")


# ---------------------------------------------------------------------------
def S7_replay() -> None:
    hr("S7  A real recorded game, for scale")
    import replay_order

    rows = replay_order.extract(os.path.join(ROOT, "ron-data", "replays", "today.rcx"))
    print(f"  {'frame':>6} {'t':>6}  what")
    for frame, kind, tid, name, cnt in rows:
        if frame > 15 * 60 * 8:
            break
        print(f"  {frame:6d} {mmss(frame):>6}  {name}")


SECTIONS = {
    "S1": S1_rates,
    "S2": S2_cap_scale,
    "S3": lambda: S3_min_age(),
    "S3b": S3b_sensitivity,
    "S4": S4_marginal_citizen,
    "S5": S5_allocation,
    "S6": S6_bhs,
    "S7": S7_replay,
}

if __name__ == "__main__":
    want = sys.argv[1:] or list(SECTIONS)
    for k in want:
        SECTIONS[k]()
