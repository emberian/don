#!/usr/bin/env python3
"""
opening.py -- the Rise of Nations opening as an optimisation problem, rebuilt on
MEASURED engine quantities.

This supersedes the model in econ.py / plan.py in one respect that changes every
number downstream: the commerce clamp's scale is no longer an assumption.

    Leader::calc_resource_caps  0x006CE900   (PDB name; leaders.cpp:14339)
    ...
    006cee70  mov  eax, [ecx + 0x30]      ; resource_cap[r], encrypted
    006cee73  xor  eax, 0x1281            ; decrypt
    006cee78  shl  eax, 4                 ; <<<< x16, in place, at the END of the loop
    006cee7b  mov  [0xcb195c], eax        ; LeaderDataEncrypt::scratch (setter idiom)
    006cee80  xor  eax, 0x1281            ; re-encrypt
    006cee85  mov  [ecx + 0x30], eax      ; store back
    006cee8b  cmp  ebx, 7 ; jl            ; r = 0..6, matching resource_cap[7]

so `resource_cap[r] = commerce_cap[commerce_level] * 16`, in the same 1/16-resource
units as the income it is compared against in Leader::do_gather 0x006CE512.  The
effective Ancient-age ceiling is **70 resources per 30 s**, not 4.375.  Prior work
called this "the model's weakest link" and assumed it; it is now measured.

Everything else here is likewise traced to a named function in the shipped PDB.
See docs/tracks/analytics-v2.md for the full ledger.

Usage:
    python3 analysis/opening.py facts
    python3 analysis/opening.py caps
    python3 analysis/opening.py plan
    python3 analysis/opening.py shipped
    python3 analysis/opening.py ages
    python3 analysis/opening.py marginal
    python3 analysis/opening.py alloc
    python3 analysis/opening.py military
    python3 analysis/opening.py all
"""

from __future__ import annotations

import base64
import json
import os
import struct
import sys
from dataclasses import dataclass, field, replace

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
D = json.load(open(os.path.join(HERE, "derived.json")))

FOOD, TIMBER, WEALTH, KNOWLEDGE, METAL, OIL = range(6)
RES = ["food", "timber", "wealth", "knowledge", "metal", "oil"]

# ---------------------------------------------------------------------------
# constants, read out of the LIVE Constants block where we have one
# ---------------------------------------------------------------------------
_LIVE = os.path.join(ROOT, "schema", "live", "rules-block-pid14644.txt")
_TYPES = os.path.join(ROOT, "schema", "types.json")


def _live_rules() -> dict[str, list[int]]:
    """Constants field name -> live values, from a running riseofnations.exe.

    The block is the first 4096 bytes of the `Constants` singleton captured out of
    pid 14644; `schema/types.json` gives the PDB field layout (722 members).
    """
    blob = base64.b64decode(open(_LIVE).read().split("BLK=")[1].strip())
    fields = json.load(open(_TYPES))["classes"]["Constants"]["fields"]
    out = {}
    for f in fields:
        n = max(1, f["size"] // 4)
        if f["offset"] + f["size"] <= len(blob) and f["type"].startswith("int"):
            out[f["name"]] = list(struct.unpack_from("<%di" % n, blob, f["offset"]))
    return out


LIVE = _live_rules()
R = {k: v["values"] for k, v in D["rules"].items()}


def rule(name: str) -> list[int]:
    """Prefer the live read; fall back to the static derivation."""
    if name in LIVE:
        return LIVE[name]
    return R[name]


GATHER_RATE = rule("gather_rate")[0]           # 450 frames
PERIOD = GATHER_RATE * 16                      # 7200; 0x006CE7C1 shl ecx,4
PEASANT_RATE = rule("peasant_rate")[0]         # 2560 = 10.0 in 8.8
OIL_RATE = rule("oil_rate")[0]                 # 8960 = 35.0 in 8.8
CITY_GATHER = rule("city_gather")              # [10,10,0,0,0,0]
BASIC_GATHER = rule("basic_gather")            # all zero, shipped
STARTING_GOODS = rule("starting_goods")        # [200,200,100,100,100,100]
COMMERCE_CAP = rule("commerce_cap")            # [70,100,150,200,260,320,400,500]
POP_CAP = rule("pop_cap")                      # [25,50,...,200]
FARMS_PER_CITY = rule("farms_per_city_base")[0]
GRANARY_BONUS = rule("granary_bonus")
LUMBERMILL_BONUS = rule("lumbermill_bonus")
SMELTER_BONUS = rule("smelter_bonus")
MARKET_TAXES = rule("market_taxes")[0]
UNIVERSITY_LITERACY = rule("university_literacy")[0]
TERRITORY_TAXES = rule("territory_taxes")      # [0,50,100,200,300] by government
UNIT_COST_FACTOR = rule("unit_cost_factor")[0]
BUILD_COST_FACTOR = rule("build_cost_factor")[0]
TECH_COST_FACTOR = rule("tech_cost_factor")[0]
RAMP = {
    "scholar": rule("unit_scholar_ramp_max")[0],
    "worker": rule("unit_worker_ramp_max")[0],
    "civilian": rule("unit_other_civilian_ramp_max")[0],
    "military": rule("unit_military_ramp_max")[0],
}

GLOBAL_CEILING = 16000       # 0x006CE706, mov eax, 0x3e70; cmovl -> 1000 res / period
KNOWLEDGE_CAP_RAW = 999      # 0x006CE92C, 0x1166 ^ 0x1281 = 999 (hardcoded for r == 3)
SLOT16 = 16 * PEASANT_RATE // 256      # 160 income units = 10 resources / period

# 67 ms per tick at Normal (TurnControl::timings 0x00AFC4A4 = {200,125,67,50,1});
# the engine's own frames->seconds conversion is idiv 15 at 0x005924CF.  450 frames
# is 30.0 s under the /15 conversion and 30.15 s at 67 ms.  We report /15.
FPS = 15

U, B, T = D["units"], D["buildings"], D["techs"]

# tech `cat` is the library column; epoch[cat] is the level counter the engine
# indexes its per-column limit with.  Confirmed four ways:
#   cat 0  Military  The Art of War   -> epoch[0] indexes POP_CAP    (Constants+0x3C4)
#   cat 1  Civic     City State       -> epoch[1] + 1 = city limit   (LeaderData::get_city_limit)
#   cat 2  Commerce  Barter           -> epoch[2] indexes COMMERCE_CAP (Constants+0x400, 0x006CE911 reads +0xF0 = epoch[2])
#   cat 3  Science   Written Word     -> epoch[3]
MILITARY, CIVIC, COMMERCE, SCIENCE = 0, 1, 2, 3


# ---------------------------------------------------------------------------
# costs -- TypeData::get_cost 0x00664090
# ---------------------------------------------------------------------------
def cost_of(rec: dict, count: int, factor: int, ramp_pct: int) -> list[int]:
    """cost[r] = COST[r]*factor + min(SUPPORTVALUE*count, COST[r]*factor*ramp/100).

    0x006645B4 imul by the cost factor; 0x0066544B the support term; 0x00665405 the
    ceiling; 0x00665452 `test esi,esi` -- a ZERO ceiling means NO ceiling.
    """
    c = [x * factor for x in rec["cost"]]
    for k in (0, 1):
        r = rec["support"][k]
        if r is None or r < 0:
            continue
        v = rec["support_cost"][k] * count
        if ramp_pct:
            lim = c[r] * ramp_pct // 100
            if lim and lim < v:
                v = lim
        c[r] += v
    return c


# ---------------------------------------------------------------------------
# assumptions -- everything the binary did NOT tell us, each with a sweep in `all`
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class Assume:
    slots_farm: int = 1          # FARMS_PER_CITY_BASE=5 and the AI's one-farm-one-worker bookkeeping
    slots_wood: int = 5          # aibestbuildlibrary.bhs place_woodcutter min_size = 5
    slots_mine: int = 4          # no source; unused in the Ancient age
    max_builders: int = 4        # buildingrules.xml documents JOB_TIME for ONE citizen
    build_linear: bool = True    # k builders -> JOB_TIME/k
    building_ramp_pct: int = 0   # which RAMP_MAX class a building is in is not derived; 0 = none
    walk_frames: int = 0         # travel to a new job/site
    start_citizens: int = 5      # economic.bhs case 4 ("up to 5 peasants") and case 8 (needed=9 after +4)
    start_farms: int = 3         # citytemplates.xml small template
    start_woodcamps: int = 1     # economic.bhs case 13 is "Woodcutter #2"
    government: int = 0          # TERRITORY_TAXES[0] = 0 -- despotism pays no territory tax
    territory: int = 0           # tiles owned; only matters when government > 0
    caravan_wealth: int = 0      # per caravan per period; NOT derived (see report S8)
    techs_per_age: int = 0       # get_techs_per_age() is a game option, not a rules constant


A0 = Assume()


# ---------------------------------------------------------------------------
# the state
# ---------------------------------------------------------------------------
@dataclass
class W:
    a: Assume = A0
    frame: int = 0
    stock: list = field(default_factory=lambda: list(STARTING_GOODS))
    leftover: list = field(default_factory=lambda: [0] * 6)
    cities: int = 1
    farms: int = -1
    woodcamps: int = -1
    mines: int = 0
    markets: int = 0
    universities: int = 0
    libraries: int = 1
    granaries: int = 0
    lumbermills: int = 0
    barracks: int = 0
    citizens: int = -1
    on: list = field(default_factory=lambda: [0] * 6)   # citizens by gathered resource
    epoch: list = field(default_factory=lambda: [0, 0, 0, 0])
    age: int = 0
    techs: frozenset = field(default_factory=frozenset)
    # servers: (busy_until_frame) for the trainer, the build crew, the library
    t_free: int = 0
    b_free: int = 0
    l_free: int = 0
    builders: int = 0
    pend: tuple = ()          # ((done_frame, name), ...) queued but not yet complete
    log: tuple = ()

    def __post_init__(self):
        if self.citizens < 0:
            self.citizens = self.a.start_citizens
        if self.farms < 0:
            self.farms = self.a.start_farms
        if self.woodcamps < 0:
            self.woodcamps = self.a.start_woodcamps
        if sum(self.on) == 0:
            self.reseat()

    def clone(self) -> "W":
        return replace(self, stock=list(self.stock), leftover=list(self.leftover),
                       on=list(self.on), epoch=list(self.epoch))

    # -- capacities -----------------------------------------------------
    # Counters below hold OWNED + QUEUED, which is what the engine charges cost
    # ramps against (TypeData::get_cost reads num_queued at 0x00665966) and what
    # the AI's num_type_with_queued() sees.  Income must use only what is BUILT,
    # so every income path subtracts the pending queue.
    def npend(self, name: str) -> int:
        return sum(1 for _, n in self.pend if n == name)

    def built(self, name: str) -> int:
        return owned(self, name) - self.npend(name)

    def slots(self) -> list[int]:
        s = [0] * 6
        s[FOOD] = self.built("Farm") * self.a.slots_farm
        s[TIMBER] = self.built("Woodcutter's Camp") * self.a.slots_wood
        s[METAL] = self.built("Mine") * self.a.slots_mine
        s[KNOWLEDGE] = self.built("University")
        return s

    @property
    def pop_cap(self) -> int:
        return POP_CAP[min(self.epoch[MILITARY], 7)]

    @property
    def city_limit(self) -> int:
        return self.epoch[CIVIC] + 1     # LeaderData::get_city_limit 0x006D6130

    def cap16(self) -> list[int]:
        """Leader::calc_resource_caps: commerce_cap[epoch[2]] * 16; knowledge = 999*16."""
        c = COMMERCE_CAP[min(self.epoch[COMMERCE], 7)] * 16
        out = [c] * 6
        out[KNOWLEDGE] = KNOWLEDGE_CAP_RAW * 16
        return out

    def reseat(self) -> None:
        """Greedy seating: fill food to the cap, then timber, then the rest."""
        sl = self.slots()
        free = self.built("Citizen") - self.builders
        self.on = [0] * 6
        for r in (FOOD, TIMBER, KNOWLEDGE, METAL):
            take = min(sl[r], free, self.useful_slots(r))
            self.on[r] = max(0, take)
            free -= self.on[r]
        # anything still idle goes wherever there is physical room (it earns nothing
        # past the cap, but the engine does not stop it standing there)
        for r in (FOOD, TIMBER, METAL):
            room = sl[r] - self.on[r]
            take = min(room, free)
            self.on[r] += take
            free -= take

    def useful_slots(self, r: int) -> int:
        """How many workers on r still produce anything before the clamp bites."""
        base = self.free_income16(r)
        cap = self.cap16()[r]
        if base >= cap:
            return 0
        return (cap - base + SLOT16 - 1) // SLOT16

    def free_income16(self, r: int) -> int:
        """Income that costs no worker: cities, markets, universities, territory."""
        v = BASIC_GATHER[r] * 16 + CITY_GATHER[r] * 16 * self.built("Small City")
        if r == WEALTH:
            v += MARKET_TAXES * 16 * self.built("Market")
            gov = self.a.government
            v += self.a.territory * TERRITORY_TAXES[gov] * 16 // 100 if gov else 0
        if r == KNOWLEDGE:
            v += UNIVERSITY_LITERACY * 16 * self.built("University")
        return v

    # -- income ---------------------------------------------------------
    def income16(self) -> list[int]:
        """Leader::calc_gather -> Leader::do_gather, in 1/16 resource per 450 frames."""
        bonus = [GRANARY_BONUS[0] if self.granaries else 0,
                 LUMBERMILL_BONUS[0] if self.lumbermills else 0,
                 0, 0,
                 SMELTER_BONUS[0] if self.smelters() else 0, 0]
        out = [0] * 6
        for r in range(6):
            rate = OIL_RATE if r == OIL else PEASANT_RATE
            v = (16 * self.on[r] * rate) // 256           # 0x0063A1EF, >>8 at 0x0063A1F6
            v = v * (100 + bonus[r]) // 100               # CityData::enhancer_amount
            out[r] = max(v, 0) + self.free_income16(r)
        cap = self.cap16()
        for r in range(6):
            if out[r] > cap[r]:
                out[r] = cap[r]                           # 0x006CE512 / 0x006CE61C
            if out[r] > GLOBAL_CEILING:
                out[r] = GLOBAL_CEILING                   # 0x006CE706
        return out

    def smelters(self) -> int:
        return 0

    def rate(self) -> list[float]:
        """Resources per 30 s, i.e. what ScenarioFuncSet::gather_rate returns (income/16)."""
        return [v / 16.0 for v in self.income16()]

    # -- time -----------------------------------------------------------
    def _tick(self, frames: int) -> None:
        """Leader::do_gather's accumulator, run `frames` times at a fixed income."""
        inc = self.income16()
        for r in range(6):
            if inc[r] <= 0:
                continue
            tot = inc[r] * frames + self.leftover[r]
            self.stock[r] += tot // PERIOD
            self.leftover[r] = tot % PERIOD
        self.frame += frames

    def _flush(self) -> None:
        """Apply every queued item whose completion frame has arrived."""
        landed = [p for p in self.pend if p[0] <= self.frame]
        if not landed and not (self.builders and self.b_free <= self.frame):
            return
        for _, nm in landed:
            if nm in T:
                if nm.endswith(" Age"):
                    self.age += 1
                else:
                    c = T[nm]["cat"]
                    self.epoch[c] = self.epoch[c] + 1
        self.pend = tuple(p for p in self.pend if p[0] > self.frame)
        if self.builders and self.b_free <= self.frame:
            self.builders = 0
        self.reseat()

    def _next_event(self, target: int):
        """Earliest frame in (frame, target) at which income changes."""
        best = None
        if self.builders and self.frame < self.b_free < target:
            best = self.b_free
        for done, _ in self.pend:
            if self.frame < done < target and (best is None or done < best):
                best = done
        return best

    def advance(self, frames: int) -> None:
        """Advance, applying completions and releasing the build crew as they land."""
        target = self.frame + max(0, frames)
        self._flush()
        while self.frame < target:
            ev = self._next_event(target)
            if ev is None:
                self._tick(target - self.frame)
                break
            self._tick(ev - self.frame)
            self._flush()
        self._flush()

    def frames_to_afford(self, cost: list[int], limit: int = 60000):
        """Frames until `cost` is affordable, honouring every income step in between."""
        probe = self.clone()
        step = 0
        for _ in range(256):
            probe._flush()
            need = [max(0, cost[r] - probe.stock[r]) for r in range(6)]
            if not any(need):
                return step
            inc = probe.income16()
            worst = 0
            for r in range(6):
                if not need[r]:
                    continue
                if inc[r] <= 0:
                    worst = None
                    break
                f = -(-(need[r] * PERIOD - probe.leftover[r]) // inc[r])
                worst = max(worst, max(0, f))
            ev = probe._next_event(probe.frame + (worst if worst is not None else limit) + 1)
            if ev is not None:
                jump = ev - probe.frame
                probe.advance(jump)
                step += jump
                if step > limit:
                    return None
                continue
            if worst is None:
                return None
            step += worst
            return step if step <= limit else None
        return None

# ---------------------------------------------------------------------------
# actions
# ---------------------------------------------------------------------------
BUILDINGS = {
    "Farm": ("farms", "b"),
    "Woodcutter's Camp": ("woodcamps", "b"),
    "Mine": ("mines", "b"),
    "Market": ("markets", "b"),
    "Library": ("libraries", "b"),
    "University": ("universities", "b"),
    "Granary": ("granaries", "b"),
    "Barracks": ("barracks", "b"),
    "Small City": ("cities", "b"),
}
TECHS = ["Barter", "City State", "Written Word", "The Art of War", "Classical Age",
         "Coinage", "Empire", "Mathematics", "Medieval Age"]


def owned(w: W, name: str) -> int:
    if name in BUILDINGS:
        return getattr(w, BUILDINGS[name][0])
    if name == "Citizen":
        return w.citizens
    return 0


def action_cost(w: W, name: str) -> list[int]:
    if name == "Citizen":
        return cost_of(U["Citizen"], w.citizens, UNIT_COST_FACTOR, RAMP["worker"])
    if name in BUILDINGS:
        return cost_of(B[name], owned(w, name), BUILD_COST_FACTOR, w.a.building_ramp_pct)
    if name in T:
        return cost_of(T[name], 0, TECH_COST_FACTOR, 0)
    raise KeyError(name)


def action_time(w: W, name: str) -> int:
    if name == "Citizen":
        return U["Citizen"]["job_time"]
    if name in BUILDINGS:
        jt = B[name]["job_time"]
        k = min(w.a.max_builders, max(1, w.citizens - w.builders))
        return (jt // k if w.a.build_linear else jt) + w.a.walk_frames
    return T[name]["job_time"]


def legal(w: W, name: str) -> bool:
    if name == "Citizen":
        return w.citizens < w.pop_cap
    if name == "Small City":
        return w.cities < w.city_limit
    if name == "Farm":
        return w.farms < FARMS_PER_CITY * w.cities
    if name in BUILDINGS:
        # a building's prerequisites are tech slot ids in the live type tables
        for p in B[name]["preq"]:
            if p and p > 0:
                pn = [n for n, t in T.items() if t["slot"] == p]
                if pn and not (pn[0] in w.techs and w.npend(pn[0]) == 0):
                    return False
        return True
    if name in T:
        if name in w.techs:
            return False
        for p in T[name]["preq"]:
            if p and p > 0:
                pn = [n for n, t in T.items() if t["slot"] == p]
                if pn and pn[0] not in w.techs:
                    return False
        return True
    return False


def apply(w: W, name: str) -> W:
    """Queue `name` at the earliest frame its server is free and it is affordable."""
    s = w.clone()
    if name == "Citizen":
        server, dur = "t_free", action_time(w, name)
    elif name in BUILDINGS:
        server, dur = "b_free", action_time(w, name)
    else:
        server, dur = "l_free", action_time(w, name)
    start = max(s.frame, getattr(s, server))
    s.advance(start - s.frame)
    cost = action_cost(s, name)
    wait = s.frames_to_afford(cost)
    if wait is None:
        return None
    s.advance(wait)
    for r in range(6):
        s.stock[r] -= cost[r]
    done = s.frame + dur
    setattr(s, server, done)
    if server == "b_free":
        # the crew stops gathering for the duration of the build
        s.builders = min(s.a.max_builders, max(0, s.built("Citizen") - 1))
    s.log = s.log + ((s.frame, done, name),)
    return queue(s, name, done)


def queue(s: W, name: str, done: int) -> W:
    """Bump the owned+queued counter now; the physical effect lands at `done`.

    The engine charges the cost ramp against owned + queued (TypeData::get_cost reads
    num_queued at 0x00665966) and the AI reads num_type_with_queued, so the counter
    moves at queue time.  Income, gather slots and tech levels only move when the
    item completes, which is what `pend` tracks.
    """
    if name == "Citizen":
        s.citizens += 1
    elif name in BUILDINGS:
        setattr(s, BUILDINGS[name][0], getattr(s, BUILDINGS[name][0]) + 1)
    else:
        s.techs = s.techs | {name}
    s.pend = s.pend + ((done, name),)
    s.reseat()
    return s


# ---------------------------------------------------------------------------
# the objective and the search
# ---------------------------------------------------------------------------
def boom_target(s: W) -> bool:
    """Exactly the state economic.bhs cases 6..18 builds."""
    return (s.age >= 1 and {"Written Word", "City State", "Barter"} <= s.techs
            and s.cities >= 2 and s.citizens >= 14 and s.farms >= 7
            and s.woodcamps >= 2 and s.markets >= 1)


CHOICES = ["Citizen", "Farm", "Woodcutter's Camp", "Small City", "Market",
           "Library", "University", "Granary", "Mine", "Barracks",
           "Barter", "City State", "Written Word", "The Art of War", "Classical Age"]


def horizon(s: W) -> int:
    """When the last thing queued completes."""
    return max([s.frame, s.t_free, s.b_free, s.l_free] + [d for d, _ in s.pend])


def settle(s: W) -> W:
    """Run the clock to the horizon so every queued item has landed."""
    t = s.clone()
    t.advance(horizon(t) - t.frame)
    return t


def key(s: W) -> tuple:
    return (s.citizens, s.farms, s.woodcamps, s.cities, s.markets, s.mines,
            s.universities, s.granaries, s.barracks, frozenset(s.techs),
            s.t_free // 40, s.b_free // 40, s.l_free // 40)


def search(start: W, target, beam: int = 900, depth: int = 40, choices=None):
    """Beam search over purchase sequences.  Returns the best (frame, state)."""
    choices = choices or CHOICES
    frontier = [start]
    best = None
    for _ in range(depth):
        nxt = {}
        for s in frontier:
            for c in choices:
                if not legal(s, c):
                    continue
                t = apply(s, c)
                if t is None:
                    continue
                ts = settle(t)
                if target(ts):
                    h = horizon(t)
                    if best is None or h < best[0]:
                        best = (h, ts)
                    continue
                k = key(t)
                if k not in nxt or horizon(nxt[k]) > horizon(t):
                    nxt[k] = t
        if not nxt:
            break
        frontier = sorted(nxt.values(), key=lambda s: (horizon(s), -sum(s.stock)))[:beam]
        if best is not None and frontier and horizon(frontier[0]) > best[0]:
            break
    return best


def dp_search(need: dict, extras: dict = None, keep: int = 6, a: Assume = A0,
              start: W = None, extra_choices=()):
    """Exact-ish search over the ORDERINGS of a known purchase multiset.

    The blind beam in `search` wanders; here the set of things that must be bought is
    fixed by the objective, so the only decision is the order and the timing.  The
    lattice of "how many of each I have bought so far" is small (tens of thousands of
    nodes), so we can hold a few Pareto-best states per node and get an answer that is
    within a couple of frames of optimal rather than within half a minute.

    `need`  : name -> how many MUST be bought
    `extras`: name -> how many MAY additionally be bought (income investments)
    """
    extras = extras or {}
    names = list(dict.fromkeys(list(need) + list(extras) + list(extra_choices)))
    cap = {n: need.get(n, 0) + extras.get(n, 0) for n in names}
    s0 = (start.clone() if start else W(a=a))
    root = tuple(0 for _ in names)
    nodes = {root: [s0]}
    best = None
    order = [root]
    seen = {root}
    qi = 0
    while qi < len(order):
        node = order[qi]; qi += 1
        pool = nodes.get(node)
        if not pool:
            continue
        if best is not None and min(horizon(x) for x in pool) >= best[0]:
            continue
        for i, nm in enumerate(names):
            if node[i] >= cap[nm]:
                continue
            child = list(node); child[i] += 1; child = tuple(child)
            got = []
            for s in pool:
                if not legal(s, nm):
                    continue
                t = apply(s, nm)
                if t is not None:
                    got.append(t)
            if not got:
                continue
            done = all(child[j] >= need.get(n, 0) for j, n in enumerate(names))
            if done:
                for t in got:
                    ts = settle(t)
                    h = horizon(t)
                    if best is None or h < best[0]:
                        best = (h, ts)
            cur = nodes.get(child, [])
            cur = cur + got
            cur.sort(key=lambda x: (horizon(x), -sum(x.stock[:2])))
            trimmed, marks = [], set()
            for x in cur:
                m = (horizon(x) // 25, x.stock[0] // 40, x.stock[1] // 40)
                if m in marks:
                    continue
                marks.add(m)
                trimmed.append(x)
                if len(trimmed) >= keep:
                    break
            nodes[child] = trimmed
            if child not in seen:
                seen.add(child)
                order.append(child)
    return best


BOOM_NEED = {"Written Word": 1, "City State": 1, "Barter": 1, "Classical Age": 1,
             "Small City": 1, "Citizen": 9, "Farm": 4, "Woodcutter's Camp": 1,
             "Market": 1}
BOOM_EXTRA = {"Citizen": 3, "Farm": 2, "Woodcutter's Camp": 1}


def mmss(f: int) -> str:
    t = f / FPS
    return "%d:%02d" % (int(t) // 60, int(t) % 60)


def run_order(order: list[str], a: Assume = A0, start: W = None, limit: int = 60000):
    """Execute a fixed order with BLOCK_ON_THIS semantics.

    economic.bhs returns BLOCK_ON_THIS from every boom case, so the script does not
    move on until the step succeeds.  When an action is not yet legal (the tech that
    unlocks it has not landed, the city limit has not risen) the script waits.
    """
    s = start.clone() if start else W(a=a)
    for name in order:
        guard = 0
        while not legal(s, name):
            ev = s._next_event(s.frame + 100000)
            step = (ev - s.frame) if ev else 60
            s.advance(max(1, step))
            guard += 1
            if s.frame > limit or guard > 400:
                return None, name
        t = apply(s, name)
        if t is None:
            return None, name
        s = t
    return settle(s), None


# ---------------------------------------------------------------------------
# sections
# ---------------------------------------------------------------------------
def S_facts():
    print("== measured quantities (all values live-read or instruction-level) ==")
    rows = [
        ("gather period", "%d frames = %.1f s" % (GATHER_RATE, GATHER_RATE / FPS),
         "Constants::gather_rate; period = <<4 at 0x006CE7C1"),
        ("income unit", "1/16 resource per period", "ScenarioFuncSet::gather_rate returns income>>4 (0x009E914E)"),
        ("one worked slot", "%d units = %.0f res/period" % (SLOT16, SLOT16 / 16),
         "peasant_rate %d /256 at 0x0063A1EF" % PEASANT_RATE),
        ("one oil slot", "%.0f res/period" % (OIL_RATE / 256), "oil_rate at 0x0063A1C6"),
        ("one city", "%d food + %d timber /period" % (CITY_GATHER[0], CITY_GATHER[1]),
         "city_gather <<4 at 0x006D5979"),
        ("one Market", "%d wealth /period" % MARKET_TAXES, "CityData::get_taxes 0x00737B50"),
        ("one University", "%d knowledge /period" % UNIVERSITY_LITERACY, "CityData::get_literacy 0x00737C00"),
        ("commerce clamp", "commerce_cap[epoch[2]] x 16", "Leader::calc_resource_caps shl eax,4 @ 0x006CEE78 [MEASURED]"),
        ("  -> in resources", "/".join(str(c) for c in COMMERCE_CAP) + " per period", "Constants::commerce_cap (live)"),
        ("knowledge cap", "%d per period" % KNOWLEDGE_CAP_RAW, "hardcoded 0x1166^0x1281 @ 0x006CE92C"),
        ("global ceiling", "%d units = %d res/period" % (GLOBAL_CEILING, GLOBAL_CEILING // 16), "0x006CE706"),
        ("population cap", "/".join(str(c) for c in POP_CAP), "Constants::pop_cap indexed by epoch[0]"),
        ("city limit", "epoch[1] + 1", "LeaderData::get_city_limit 0x006D6130"),
        ("territory taxes", "/".join(str(c) for c in TERRITORY_TAXES) + " by government",
         "Constants::territory_taxes; used at 0x006CF6A5, gov 0 pays 0"),
        ("Dutch interest", "5%% of stock over start, cap +%d res" % rule("dutch_interest_cap")[0],
         "0x006CE6C2..0x006CE703; the +50 IS <<4 at 0x006CE6FC"),
        ("income refresh", "<=8 frames when dirty; else >=300 and (frame+8*who)%256==0",
         "Leader::calc_gather gate 0x006CEEF4/0x006CEF00/0x006CF795; stamp written 0x006CF71C"),
    ]
    w = max(len(r[0]) for r in rows)
    for k, v, src in rows:
        print("  %-*s  %-38s  %s" % (w, k, v, src))
    print()
    print("  scholar_rate CORRECTION: live Constants+0x284 is int[6] = %s" % LIVE["scholar_rate"])
    print("    i.e. scale-256 [5,7,10,15,20,25].  docs/derivation/rules-constants.json says")
    print("    parser=wtoi, 5 entries [5,7,10,15,20] -- wrong count AND wrong parser.")
    print("    Read sites in .text: ZERO (full capstone operand scan for disp 0x284).")


def S_caps():
    print("== where the commerce clamp stops paying for workers ==")
    print("   income16 = 160*workers + 160*cities  (+160*markets for wealth)")
    print("   cap16    = commerce_cap[level] * 16")
    print()
    hdr = "  %-6s %-14s %6s | " % ("level", "tech", "cap")
    hdr += " ".join("%9s" % ("%d city" % c) for c in (1, 2, 3, 4))
    print(hdr)
    names = {0: "-", 1: "Barter", 2: "Coinage", 3: "Trade", 4: "Banking"}
    for lvl in range(5):
        cap = COMMERCE_CAP[lvl]
        row = "  %-6d %-14s %6d | " % (lvl, names.get(lvl, "?"), cap)
        for cities in (1, 2, 3, 4):
            free = CITY_GATHER[FOOD] * cities
            n = max(0, (cap - free) // (SLOT16 // 16))
            row += "%9d" % n
        print(row)
    print()
    print("  At Commerce 0 with one city the SEVENTH food gatherer earns nothing, and")
    print("  independently the seventh timber gatherer earns nothing.  A second city")
    print("  adds 10 free food + 10 free timber and simultaneously removes one usable")
    print("  food worker and one usable timber worker: net zero income at level 0.")


def S_plan(beam=900):
    print("== fastest complete Ancient boom (beam=%d) ==" % beam)
    r = search(W(), boom_target, beam=beam)
    if not r:
        print("  no plan found")
        return None
    f, s = r
    print("  target reached at frame %d = %s" % (f, mmss(f)))
    for st, dn, n in s.log:
        print("    %5d %5s  %-20s (done %s)" % (st, mmss(st), n, mmss(dn)))
    print("  end: %d citizens %s, %d farms, %d camps, %d cities, %d markets"
          % (s.citizens, s.on[:2], s.farms, s.woodcamps, s.cities, s.markets))
    print("  rate: " + ", ".join("%s %.1f" % (RES[i], s.rate()[i]) for i in range(4)))
    return r


SHIPPED_ORDER = [
    # economic.bhs cases 6..18, generic nation, land map, size>=2.
    "Written Word",        # case 6  Science I
    "City State",          # case 7  Civic I
    "Citizen", "Citizen", "Citizen", "Citizen",   # case 8  -> 9 citizens
    "Farm",                # case 9
    "Small City",          # case 10
    "Citizen", "Citizen",  # case 11 -> 11
    "Farm",                # case 12
    "Woodcutter's Camp",   # case 13
    "Barter",              # case 14 Commerce I
    "Market",              # case 15
    "Citizen", "Citizen", "Citizen",   # case 16 -> 14
    "Farm", "Farm",        # case 17  farms -> 7
    "Classical Age",       # case 18
]


def S_shipped():
    print("== the shipped designers' order (economic.bhs cases 6-18) ==")
    s, fail = run_order(SHIPPED_ORDER)
    if s is None:
        print("  stalled at", fail)
        return None
    h = s.frame
    print("  target reached at frame %d = %s" % (h, mmss(h)))
    for st, dn, n in s.log:
        print("    %5d %5s  %-20s (done %s)" % (st, mmss(st), n, mmss(dn)))
    return h, s


def S_versus(beam=900):
    print("== head to head, same model, same objective ==")
    sh = S_shipped()
    print()
    op = S_plan(beam=beam)
    if not sh or not op:
        return
    print()
    print("  shipped economic.bhs : %s (%d frames)" % (mmss(sh[0]), sh[0]))
    print("  optimiser            : %s (%d frames)" % (mmss(op[0]), op[0]))
    d = sh[0] - op[0]
    print("  gap                  : %s (%.0f%%)" % (mmss(abs(d)), 100.0 * d / op[0]))


def S_ages():
    print("== minimum time to each age ==")
    print("  (a) the age advance ALONE is not an interesting objective.")
    s = W()
    t = apply(s, "Classical Age")
    print("      Classical Age from a standing start, building nothing: %s"
          % mmss(t.log[-1][1]))
    print("      starting food %d, age cost %d, so the whole race is banking %d food"
          % (STARTING_GOODS[FOOD], T["Classical Age"]["cost"][FOOD] * TECH_COST_FACTOR,
             T["Classical Age"]["cost"][FOOD] * TECH_COST_FACTOR - STARTING_GOODS[FOOD]))
    print()
    print("  (b) with the economy the shipped AI actually builds behind it:")
    r = dp_search(BOOM_NEED, extras={"Citizen": 2, "Farm": 1}, keep=16)
    print("      Classical Age + the full Ancient boom: %s (searched)" % mmss(r[0]))
    print()
    print("  (c) every age after the first is a KNOWLEDGE race, and knowledge has")
    print("      exactly two sources in this game: %d per University per 30 s"
          % UNIVERSITY_LITERACY)
    print("      (CityData::get_literacy) and one gather slot per Scholar standing in")
    print("      one (%d per 30 s at PEASANT_RATE -- Constants::scholar_rate is DEAD"
          % (PEASANT_RATE // 256))
    print("      CODE, zero read sites in .text).  The cap is %d per 30 s, hardcoded."
          % KNOWLEDGE_CAP_RAW)
    print()
    ages = ["Medieval Age", "Gunpowder Age", "Enlightenment Age", "Industrial Age",
            "Modern Age", "Information Age"]
    print("      %-20s %7s %9s | %s" % ("age", "food", "knowledge",
          "  ".join("%2d src" % n for n in (1, 2, 4, 8, 16))))
    for name in ages:
        c = [x * TECH_COST_FACTOR for x in T[name]["cost"]]
        row = "      %-20s %7d %9d | " % (name, c[FOOD], c[KNOWLEDGE])
        for n in (1, 2, 4, 8, 16):
            rate = min(10 * n, KNOWLEDGE_CAP_RAW)
            periods = c[KNOWLEDGE] / rate if rate else 0
            row += "%6s " % mmss(int(periods * GATHER_RATE))
        print(row)
    print()
    print("      Read the last column as a LOWER BOUND on time spent in the age: it is")
    print("      the banking time alone, with the universities and scholars already")
    print("      standing and free, and it ignores the age's food cost entirely.")
    print("      Even so, the Information Age needs %d knowledge, which is 6h15m at one"
          % ([x * TECH_COST_FACTOR for x in T["Information Age"]["cost"]][KNOWLEDGE]))
    print("      University and 23 minutes at sixteen sources.  The knowledge cap of")
    print("      %d per 30 s puts a hard floor of %s on that last age no matter what."
          % (KNOWLEDGE_CAP_RAW,
             mmss(int(([x * TECH_COST_FACTOR for x in T["Information Age"]["cost"]][KNOWLEDGE]
                       / KNOWLEDGE_CAP_RAW) * GATHER_RATE))))


def S_marginal():
    print("== marginal value of the Nth citizen ==")
    print("  citizen cost = 20 + min(1*(owned+queued), 100) food   [worker ramp 500%]")
    print("  farm cost    = 40 + 4*(farms owned) timber            [no building ramp]")
    print("  a worked slot pays 10 resources per 30 s")
    print()
    print("  %3s %11s %9s | %5s %11s %9s | %9s" %
          ("N", "citizen", "payback", "farm", "farm cost", "payback", "combined"))
    for n in (1, 5, 10, 15, 20, 25, 50, 100, 110, 120):
        c = cost_of(U["Citizen"], n, UNIT_COST_FACTOR, RAMP["worker"])[FOOD]
        fc = cost_of(B["Farm"], n, BUILD_COST_FACTOR, 0)[TIMBER]
        print("  %3d %8d fd %6.2f min | %5d %8d tb %6.2f min | %6.2f min"
              % (n, c, c / 10 * 0.5, n, fc, fc / 10 * 0.5, (c + fc) / 10 * 0.5))
    print()
    print("  The ramp is gentle and caps at +100 food (500%% of the 20-food base).")
    print("  The price of a citizen is NOT what limits the opening.  What limits it is")
    print("  the commerce clamp, then FARMS_PER_CITY_BASE = %d, then POP_CAP[0] = %d."
          % (FARMS_PER_CITY, POP_CAP[0]))
    print()
    print("  Marginal income of the Nth food worker, 1 city, by Commerce level:")
    for lvl in range(3):
        cap = COMMERCE_CAP[lvl]
        row = []
        for n in range(1, 12):
            inc = min(10 * n + 10, cap)
            prev = min(10 * (n - 1) + 10, cap)
            row.append(inc - prev)
        print("    level %d (cap %3d): %s" % (lvl, cap, " ".join("%2d" % x for x in row)))


def S_alloc(minutes=10):
    print("== best citizen allocation over the first %d minutes ==" % minutes)
    end = minutes * 60 * FPS
    print("  objective: food+timber banked at t = %s, all purchases counted" % mmss(end))
    print()
    print("  %-26s %5s %5s %5s %5s %5s %7s %7s %8s"
          % ("scenario", "N", "food", "timb", "farm", "camp", "food@T", "timb@T", "total"))
    scen = [
        ("1 city, no techs", [], 1),
        ("1 city, Barter", ["Barter"], 1),
        ("2 cities (City State)", ["City State", "Small City"], 2),
        ("2 cities + Barter", ["Barter", "City State", "Small City"], 2),
        ("2 cities, Barter, Market", ["Barter", "City State", "Small City", "Market"], 2),
    ]
    for label, pre, _ in scen:
        best = None
        for n in range(5, 25):
            for camps in range(1, 5):
                for farms in range(3, 5 * 3 + 1):
                    order = list(pre)
                    order += ["Farm"] * max(0, farms - A0.start_farms)
                    order += ["Woodcutter's Camp"] * max(0, camps - A0.start_woodcamps)
                    order += ["Citizen"] * max(0, n - A0.start_citizens)
                    s, fail = run_order(order)
                    if s is None or s.frame > end:
                        continue
                    s2 = s.clone()
                    s2.advance(end - s2.frame)
                    tot = s2.stock[FOOD] + s2.stock[TIMBER]
                    if best is None or tot > best[0]:
                        best = (tot, n, s2)
        if best is None:
            print("  %-26s  (no feasible plan)" % label)
            continue
        tot, n, s = best
        print("  %-26s %5d %5d %5d %5d %5d %7d %7d %8d"
              % (label, n, s.on[FOOD], s.on[TIMBER], s.farms, s.woodcamps,
                 s.stock[FOOD], s.stock[TIMBER], tot))


def S_military():
    print("== opportunity cost of an early military building ==")
    aow = cost_of(T["The Art of War"], 0, TECH_COST_FACTOR, 0)
    bar = cost_of(B["Barracks"], 0, BUILD_COST_FACTOR, 0)
    print("  The Art of War costs %d food and %d frames of Library time."
          % (aow[FOOD], T["The Art of War"]["job_time"]))
    print("  A Barracks costs %d timber and %d frames of crew time."
          % (bar[TIMBER], B["Barracks"]["job_time"]))
    print()
    base = dp_search(BOOM_NEED, extras={"Citizen": 2, "Farm": 1}, keep=12)
    print("  boom target, no military          %s" % mmss(base[0]))
    n1 = dict(BOOM_NEED); n1["The Art of War"] = 1
    r1 = dp_search(n1, extras={"Citizen": 2, "Farm": 1}, keep=12)
    print("  + The Art of War                  %s   (+%d frames = +%.0f s)"
          % (mmss(r1[0]), r1[0] - base[0], (r1[0] - base[0]) / FPS))
    n2 = dict(n1); n2["Barracks"] = 1
    r2 = dp_search(n2, extras={"Citizen": 2, "Farm": 1}, keep=12)
    print("  + The Art of War + a Barracks     %s   (+%d frames = +%.0f s)"
          % (mmss(r2[0]), r2[0] - base[0], (r2[0] - base[0]) / FPS))
    print()
    print("  The Art of War raises POP_CAP from %d to %d.  The boom target is %d"
          % (POP_CAP[0], POP_CAP[1], 14))
    print("  citizens, so the population cap does not bind and the tech buys nothing")
    print("  economic at all -- its entire Ancient-age value is military.  The delay")
    print("  above is therefore the whole price of opening a military option.")


def S_sensitivity():
    print("== sensitivity, now that the commerce clamp is MEASURED ==")
    print("  The one assumption that dominated the previous study -- the clamp scale --")
    print("  is gone: `shl eax, 4` at 0x006CEE78 settles it.  What is left is small.")
    print()
    base = dp_search(BOOM_NEED, extras={"Citizen": 2, "Farm": 1}, keep=12)
    print("  BASE                                     %s" % mmss(base[0]))
    sweeps = [
        ("slots per Woodcutter's Camp = 3", Assume(slots_wood=3)),
        ("slots per Woodcutter's Camp = 4", Assume(slots_wood=4)),
        ("slots per Woodcutter's Camp = 6", Assume(slots_wood=6)),
        ("starting citizens = 3", Assume(start_citizens=3)),
        ("starting citizens = 8", Assume(start_citizens=8)),
        ("max builders on one site = 1", Assume(max_builders=1)),
        ("max builders on one site = 8", Assume(max_builders=8)),
        ("building cost-ramp ceiling 200%", Assume(building_ramp_pct=200)),
        ("walk 75 frames per build", Assume(walk_frames=75)),
        ("no starting Woodcutter's Camp", Assume(start_woodcamps=0)),
    ]
    for label, a in sweeps:
        r = dp_search(BOOM_NEED, extras={"Citizen": 2, "Farm": 1}, keep=12,
                      start=W(a=a))
        if r:
            d = r[0] - base[0]
            print("  %-40s %s  (%+d f = %+.0f s)" % (label, mmss(r[0]), d, d / FPS))
        else:
            print("  %-40s unreachable" % label)
    print()
    print("  the search's own noise floor, by Pareto width:")
    for k in (4, 8, 12, 20):
        r = dp_search(BOOM_NEED, extras={"Citizen": 2, "Farm": 1}, keep=k)
        print("    keep %-3d -> %s (%d frames)" % (k, mmss(r[0]), r[0]))


SECTIONS = {
    "facts": S_facts, "caps": S_caps, "plan": S_plan, "shipped": S_shipped,
    "versus": S_versus, "ages": S_ages, "marginal": S_marginal, "alloc": S_alloc,
    "military": S_military, "sens": S_sensitivity,
}

if __name__ == "__main__":
    args = sys.argv[1:] or ["all"]
    if args == ["all"]:
        args = ["facts", "caps", "versus", "ages", "marginal", "alloc", "military", "sens"]
    for i, a in enumerate(args):
        if i:
            print("\n" + "-" * 78 + "\n")
        SECTIONS[a]()
