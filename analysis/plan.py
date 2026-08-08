#!/usr/bin/env python3
"""
plan.py -- the optimisation layer: a three-server discrete-event simulator over the
econ.py economy, plus a beam search over build orders.

Why this method
---------------
The opening is NOT a linear program.  Three things break LP/continuous relaxations:

  * the accumulator is integral and the purchases are integral (you cannot buy 0.7 of a
    farm, and a citizen only starts paying back once a *whole* slot exists);
  * the commerce clamp is a kink whose position moves with a discrete tech level, so the
    income function is piecewise-linear and non-convex in the decision variables;
  * the cost ramps are step functions of counts.

A MILP over 15 fps time steps would have ~10^4 binaries per building type and would still
need the ramp linearised.  The state that actually matters is small (counts of five
building types, four tech levels, a citizen split, six stockpiles), so a **simulator plus
beam search over purchase sequences** dominates: it is exact w.r.t. the modelled dynamics,
it never relaxes an integrality, and it makes the assumptions inspectable.

Three servers, because real play overlaps them:
  * CITY     -- trains citizens (one at a time per city)
  * CREW     -- the citizens doing construction (one building at a time)
  * LIBRARY  -- research (one tech at a time)

What the model omits, and why it matters
----------------------------------------
  * walking time, pathing, and gather-point distance   (map-dependent, not in any file)
  * caravans and rare resources                        (FUN_006CEEE0's 0x1a2/0x1a3 and
                                                        rare-resource loops, not derived)
  * territory taxes                                    (needs map area + territory tiles)
  * scouting, ruins, and everything military
  * the 8..300-frame lag on the gross-income recompute  (FUN_006CEEE0's gate)
  * per-city granary/lumbermill bonuses are applied globally, not per city
Each of these makes the model OPTIMISTIC about income or PESSIMISTIC about time; the
report states the direction for each.
"""

from __future__ import annotations

import heapq
from dataclasses import dataclass, field, replace

from econ import (
    B,
    BUILD_COST_FACTOR,
    FARMS_PER_CITY,
    FOOD,
    GATHER_RATE,
    METAL,
    RAMP_MAX,
    State,
    T,
    TECH_COST_FACTOR,
    TIMBER,
    U,
    UNIT_COST_FACTOR,
    Assumptions,
    ramped_cost,
)

CITY, CREW, LIBRARY = 0, 1, 2


@dataclass
class Plan:
    s: State
    free: list[int] = field(default_factory=lambda: [0, 0, 0])   # server free-at frames
    pending: list = field(default_factory=list)                  # (frame, kind, arg)
    seq: tuple = ()
    failed: bool = False

    def clone(self) -> "Plan":
        return Plan(self.s.clone(), list(self.free), list(self.pending), self.seq, self.failed)


# --------------------------------------------------------------------------
# effects
# --------------------------------------------------------------------------
def _apply(p: Plan, kind: str, arg) -> None:
    s = p.s
    if kind == "citizen":
        s.citizens += 1
        if arg in ("food", "timber", "metal"):
            r = {"food": FOOD, "timber": TIMBER, "metal": METAL}[arg]
            if sum(s.on) + s.builders < s.citizens and s.on[r] < s.slots()[r]:
                s.on[r] += 1
        elif arg == "builder":
            s.builders += 1
    elif kind == "farm":
        s.farms += 1
    elif kind == "wood":
        s.woodcamps += 1
    elif kind == "mine":
        s.mines += 1
    elif kind == "market":
        s.markets += 1
    elif kind == "university":
        s.universities += 1
    elif kind == "granary":
        s.granaries += 1
    elif kind == "city":
        s.cities += 1
    elif kind == "release":
        s.builders -= arg
    elif kind == "tech":
        s.techs = s.techs | {arg}
        col = TECH_COLUMN[arg]
        if col == "mil":
            s.lvl_mil += 1
        elif col == "civ":
            s.lvl_civ += 1
        elif col == "com":
            s.lvl_com += 1
        elif col == "sci":
            s.lvl_sci += 1
    # after any structural change, re-seat idle citizens onto newly opened slots
    _reseat(s)


def _reseat(s: State) -> None:
    """Reconcile the citizen assignment with the slots and the build crew.

    Citizens pulled onto a construction site stop gathering, which is a real cost that the
    model would otherwise miss; when the site finishes they go back to a slot.
    """
    sl = s.slots()
    for r in range(6):
        s.on[r] = min(s.on[r], sl[r])
    # the build crew has priority over gathering
    while sum(s.on) + s.builders > s.citizens:
        for r in (METAL, TIMBER, FOOD):
            if s.on[r] > 0:
                s.on[r] -= 1
                break
        else:
            break
    for r in (FOOD, TIMBER, METAL):
        while s.idle() > 0 and s.on[r] < sl[r]:
            s.on[r] += 1


TECH_COLUMN = {
    "Classical Age": "age",
    "Medieval Age": "age",
    "Written Word": "sci",
    "Mathematics": "sci",
    "City State": "civ",
    "Empire": "civ",
    "Barter": "com",
    "Coinage": "com",
    "The Art of War": "mil",
}


def _drain(p: Plan, until: int) -> None:
    """Advance the economy to `until`, applying every pending completion on the way."""
    p.pending.sort()
    while p.pending and p.pending[0][0] <= until:
        f, kind, arg = p.pending.pop(0)
        if f > p.s.frame:
            p.s.advance(f - p.s.frame)
        _apply(p, kind, arg)
    if until > p.s.frame:
        p.s.advance(until - p.s.frame)


def _finish_all(p: Plan) -> None:
    if p.pending:
        _drain(p, max(f for f, _, _ in p.pending))


# --------------------------------------------------------------------------
# actions
# --------------------------------------------------------------------------
def _queued(p: Plan, kind: str) -> int:
    """Items of `kind` already paid for but not yet finished.

    The engine's ramp count is owned + queued (0x00665966 sums two u16 arrays), and the
    capacity checks are against owned + queued too, so both use this.
    """
    return sum(1 for _, k, _ in p.pending if k == kind)


def action_spec(p: Plan, act: tuple):
    """Return (server, cost, duration, kind, arg) or None if illegal right now."""
    s = p.s
    a = s.a
    kind = act[0]
    if kind == "citizen":
        n = s.citizens + _queued(p, "citizen")
        if n >= s.pop_cap:
            return None
        rec = U["Citizen"]
        cost = ramped_cost(rec, n, UNIT_COST_FACTOR, RAMP_MAX["worker"])
        return (CITY, cost, rec["job_time"], "citizen", act[1])
    if kind in ("farm", "wood", "mine", "market", "university", "granary", "city"):
        name = {
            "farm": "Farm",
            "wood": "Woodcutter's Camp",
            "mine": "Mine",
            "market": "Market",
            "university": "University",
            "granary": "Granary",
            "city": "Small City",
        }[kind]
        rec = B[name]
        have = {
            "farm": s.farms,
            "wood": s.woodcamps,
            "mine": s.mines,
            "market": s.markets,
            "university": s.universities,
            "granary": s.granaries,
            "city": s.cities,
        }[kind] + _queued(p, kind)
        ncity = s.cities + _queued(p, "city")
        if kind == "farm" and have >= FARMS_PER_CITY * ncity:
            return None
        if kind == "city" and have >= s.city_limit:
            return None
        # buildingrules.xml BUILD_FLAGS 'j' = "Max 1 of this building allowed per city".
        # Market, University, Granary, Library and Temple all carry it.
        if kind in ("market", "university", "granary") and have >= ncity:
            return None
        if kind == "mine" and "Classical Age" not in s.techs:
            return None
        if kind == "university" and "Classical Age" not in s.techs:
            return None
        if kind == "market" and "Barter" not in s.techs:
            return None
        if kind == "granary" and not {"Mathematics", "Classical Age"} <= s.techs:
            return None
        cost = ramped_cost(rec, have, BUILD_COST_FACTOR, a.building_ramp_ceiling_pct)
        nb = max(1, min(a.max_builders, s.citizens - 1))
        dur = rec["job_time"] // nb if a.build_speedup_linear else rec["job_time"]
        return (CREW, cost, dur + a.walk_frames, kind, nb)
    if kind == "tech":
        name = act[1]
        if name in s.techs:
            return None
        rec = T[name]
        preq = {
            "Mathematics": "Written Word",
            "Empire": "City State",
            "Coinage": "Barter",
        }.get(name)
        if preq and preq not in s.techs:
            return None
        cost = [c * TECH_COST_FACTOR for c in rec["cost"]]
        return (LIBRARY, cost, rec["job_time"], "tech", name)
    raise ValueError(kind)


def step(p: Plan, act: tuple, horizon: int = 15 * 60 * 45) -> Plan | None:
    """Enqueue `act`; returns a NEW plan or None if it can never be afforded."""
    q = p.clone()
    spec = action_spec(q, act)
    if spec is None:
        # the action may become legal after pending completions -- drain and retry
        if q.pending:
            _drain(q, max(f for f, _, _ in q.pending))
            spec = action_spec(q, act)
        if spec is None:
            return None
    server, cost, dur, kind, arg = spec
    start = max(q.s.frame, q.free[server])
    _drain(q, start)
    wait = q.s.frames_until_affordable(cost)
    if wait is None:
        return None
    if wait:
        _drain(q, q.s.frame + wait)
        # re-price: ramps depend on counts that may have changed while we waited
        spec2 = action_spec(q, act)
        if spec2 is None:
            return None
        server, cost, dur, kind, arg = spec2
        if q.s.frames_until_affordable(cost):
            w2 = q.s.frames_until_affordable(cost)
            if w2 is None:
                return None
            _drain(q, q.s.frame + w2)
    if q.s.frame > horizon:
        return None
    q.s.pay(cost)
    q.free[server] = q.s.frame + dur
    if server == CREW:
        # the crew stops gathering for the duration
        q.s.builders += arg
        _reseat(q.s)
        q.pending.append((q.s.frame + dur, "release", arg))
        q.pending.append((q.s.frame + dur, kind, None))
    else:
        q.pending.append((q.s.frame + dur, kind, arg))
    q.seq = q.seq + ((q.s.frame, act),)
    return q


# --------------------------------------------------------------------------
# beam search
# --------------------------------------------------------------------------
def beam_search(start: Plan, menu, score, done, depth: int, width: int):
    """Generic beam search over action sequences.

    `score(plan)`  -> lower is better, used to rank the beam
    `done(plan)`   -> True if the goal is reached (plan is retired to `finished`)
    """
    beam = [start]
    finished = []
    for _ in range(depth):
        nxt = []
        for p in beam:
            for act in menu(p):
                q = step(p, act)
                if q is None:
                    continue
                if done(q):
                    finished.append(q)
                else:
                    nxt.append(q)
        if not nxt:
            break
        nxt.sort(key=score)
        # de-duplicate on a coarse signature so the beam does not fill with clones
        seen = set()
        beam = []
        for p in nxt:
            s = p.s
            sig = (
                s.frame // 60,
                s.citizens,
                s.farms,
                s.woodcamps,
                s.cities,
                s.markets,
                s.universities,
                tuple(sorted(s.techs)),
                tuple(s.on),
            )
            if sig in seen:
                continue
            seen.add(sig)
            beam.append(p)
            if len(beam) >= width:
                break
    return finished, beam
