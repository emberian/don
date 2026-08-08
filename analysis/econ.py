#!/usr/bin/env python3
"""
econ.py -- a frame-stepped model of the Rise of Nations opening economy, built only
from quantities derived from riseofnations.exe and its shipped data.

EVERY number used here has a provenance string in PROV below.  Everything that is an
ASSUMPTION rather than a derivation is declared in Assumptions, is defaulted to the
most defensible value, and is swept in study.py.

Units, and where they come from
-------------------------------
The engine carries income in "sixteenths of a resource per GATHER_RATE frames".
`Player::TickResource` (0x006CE450) runs once per frame per player and does

    period = RULES.GATHER_RATE << 4          ; 0x006CE7B9  shl ecx,4      -> 7200
    whole  = income / period                 ; 0x006CE7C5  idiv
    acc   += income % period                 ; 0x006CE7DE  econ+0x18+r*4 ^0x3421
    while (acc >= period) { acc -= period; whole++ }
    stockpile += whole                       ; 0x006CE855  ^0x8221

so over GATHER_RATE = 450 frames a player banks income*450/7200 = income/16 resources.
This module therefore works in **resources per GATHER_RATE period (30 s)** and converts
to the engine's units only where the engine's own clamps are applied.

Composition of `income`, from FUN_006CEEE0 (the gross-income builder) --
each term [measured] at the instruction level:

    income[r]  = BASIC_GATHER[r] << 4                       0x006CF06B (all zero shipped)
               + sum over cities of  FUN_00737C60           (city block, below)
               + territory taxes, rare resources, wonders   (not modelled -- see below)

and per city, from FUN_006D5530:

    for each gather building b in the city, for each resource r:
        slots16 = 16 * (workers on b)                       (1/16-slot fixed point)
        v       = trunc(slots16 * RATE[r] / 256)            0x0063A1EF
        v       = v * (100 + cityBonus[r]) / 100            FUN_00738360 via 0x0063A249
        income[r] += max(v, 0)
    income[r] += CITY_GATHER[r] << 4    (if nonzero)        0x006D592F..0x006D5989
    income[2] += FUN_00737B50() << 4    (wealth: taxes)
    income[3] += FUN_00737C00() << 4    (knowledge: literacy)

with RATE[r] = PEASANT_RATE (2560 = 10.0 in 8.8) for r != 5 and OIL_RATE (8960 = 35.0)
for oil, and cityBonus from FUN_00738360:  food <- GRANARY_BONUS[lvl],
timber <- LUMBERMILL_BONUS[lvl], metal <- SMELTER_BONUS[lvl].

Net effect, in resources per 30 s:  **one worked gather slot = PEASANT_RATE/256 = 10**,
**one city = CITY_GATHER = 10 food + 10 timber**, **one Market = MARKET_TAXES = 10
wealth**, **one University = UNIVERSITY_LITERACY = 10 knowledge**.

The caps
--------
`Player::UpdateCommerceCaps` (0x006CE900) writes, per resource,
`econ[0x30+r*4] = RULES.COMMERCE_CAP[econ[0xF0] ^ 0x63187]`, and TickResource clamps the
income to it at 0x006CE509.  `econ+0xF0` is NOT the age (see report S3): the four dwords
econ+0xE8/0xEC/0xF0/0xF4 are the four library-column levels, and econ+0xE8 indexes
POP_CAP while econ+0xEC + 1 is the city limit.  So COMMERCE_CAP is indexed by the
**Commerce** tech level.

The scale of that clamp is the one genuinely open question in this model; see
`COMMERCE_CAP_SCALE` and the falsification test in study.py.
"""

from __future__ import annotations

import json
import math
import os
from dataclasses import dataclass, field, replace

HERE = os.path.dirname(os.path.abspath(__file__))
D = json.load(open(os.path.join(HERE, "derived.json")))

FOOD, TIMBER, WEALTH, KNOWLEDGE, METAL, OIL = range(6)
RESNAMES = D["res"]

R = {k: v["values"] for k, v in D["rules"].items()}

GATHER_RATE = R["gather_rate"][0]            # 450 frames
FPS = 15                                     # 0x005924CF: idiv 15 on the frame counter
PERIOD_S = GATHER_RATE / FPS                 # 30.0 s
PEASANT_RATE = R["peasant_rate"][0]          # 2560 = 10.0 in 8.8
OIL_RATE = R["oil_rate"][0]                  # 8960 = 35.0 in 8.8
CITY_GATHER = R["city_gather"]               # [10,10,0,0,0,0]
BASIC_GATHER = R["basic_gather"]             # all zero
STARTING_GOODS = R["starting_goods"]         # [200,200,100,100,100,100]
COMMERCE_CAP = R["commerce_cap"]             # [70,100,150,200,260,320,400,500]
POP_CAP = R["pop_cap"]                       # [25,50,75,100,125,150,175,200]
FARMS_PER_CITY = R["farms_per_city_base"][0]  # 5
GRANARY_BONUS = R["granary_bonus"]           # [20,50,100,200,250] %
LUMBERMILL_BONUS = R["lumbermill_bonus"]
SMELTER_BONUS = R["smelter_bonus"]
MARKET_TAXES = R["market_taxes"][0]          # 10 wealth / city with a Market
UNIVERSITY_LITERACY = R["university_literacy"][0]   # 10 knowledge / city with a University
UNIT_COST_FACTOR = R["unit_cost_factor"][0]  # 10
BUILD_COST_FACTOR = R["build_cost_factor"][0]  # 10
TECH_COST_FACTOR = R["tech_cost_factor"][0]  # 10
RAMP_MAX = {
    "scholar": R["unit_scholar_ramp_max"][0],           # 2000 %
    "worker": R["unit_worker_ramp_max"][0],             # 500 %
    "other_civilian": R["unit_other_civilian_ramp_max"][0],  # 200 %
    "military": R["unit_military_ramp_max"][0],         # 125 %
}

U = D["units"]
B = D["buildings"]
T = D["techs"]


# ---------------------------------------------------------------------------
# assumptions -- everything the binary has NOT told us
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class Assumptions:
    """Each field is a quantity we could not derive.  study.py sweeps them."""

    # The clamp `cmp esi, cap` at 0x006CE509 compares a 1/16-scaled income against the
    # UNSCALED rules value.  Either the engine means "cap resources per period" (the
    # comparison is missing a <<4, i.e. we should treat the cap as cap*16 in income
    # units) or it means "cap/16 resources per period" literally.  study.py refutes the
    # literal reading against a shipped replay; base model = 16.
    commerce_cap_scale: int = 16

    # Gather slots per building.  The engine derives these from terrain
    # (FUN_00639E40's slot walk), so they are map-dependent and not in any data file.
    # Farm: 1 is forced by FARMS_PER_CITY_BASE=5 and by the shipped AI treating one
    # farm as one worker.  Woodcutter's Camp: the shipped AI (aibestbuildlibrary.bhs
    # place_woodcutter) *destroys and re-places* a camp whose max_workers_at_building
    # is below min_size=5, falling back to 4 then 3 -- so 5 is the designers' target.
    slots_farm: int = 1
    slots_wood: int = 5
    slots_mine: int = 4

    # Construction with k citizens.  JOB_TIME is documented in buildingrules.xml as
    # "How long for one citizen to build, in 1/15sec"; the k-citizen speedup is not
    # derived.  Linear is the natural reading.
    build_speedup_linear: bool = True
    max_builders: int = 4

    # Unit train time.  JOB_TIME frames, no ramp.  FUN_006508C0's ramp
    # (clamp(base + count*JOB_EXTRA_TIME*UNIT_RATE_PROGRESSION, 0, 3*base)) is a RATE,
    # not confirmed to be train time (economy.md 4.2, sim-economy.md 3.3), and with the
    # live JOB_EXTRA_TIME=10 it saturates the 3x clamp after one unit.
    train_ramp: bool = False

    # Cost ramp ceiling for BUILDINGS.  FUN_00664090 picks the ceiling by unit class;
    # which class a building falls in is not derived, and a zero ceiling means NO
    # ceiling (0x00665452 `test esi,esi; je`).  Default: no ceiling for buildings.
    building_ramp_ceiling_pct: int = 0

    # A citizen walking to a new job / a new building site loses time.  Not derived.
    walk_frames: int = 0

    # Starting citizens on a "small town" start.  Not in rules.xml.  economic.bhs pins it
    # twice: the nomad path (case 4) builds "up to 5 peasants" to reconstruct a standard
    # town, and case 8 ("Build 4 Citizens") sets needed_citizens = 9.  9 - 4 = 5.
    start_citizens: int = 5

    # Starting Woodcutter's Camp.  citytemplates.xml "small" lists only 3 Farms + 1
    # Library, but the nomad path in economic.bhs rebuilds a standard town as
    # {Woodcutter's Camp (case 2), 3 Farms (case 3), Library (case 5)}, and case 13 is
    # labelled "Build Woodcutter #2" while case 17 says "got 5 already" about farms after
    # cases 9 and 12 added one each to a base of 3.  Both books balance only if the
    # standard town starts with 1 camp and 3 farms.
    start_woodcamps: int = 1

    # Scholars.  SCHOLAR_RATE (offset 644, [5,7,10,15,20]) has NO read site in a linear
    # capstone scan of .text, so scholar knowledge is modelled as an ordinary gather
    # slot at PEASANT_RATE.  Sensitivity: 5 instead of 10.
    scholar_rate: int = PEASANT_RATE // 256


PROV = {
    "gather_rate": "rules.xml GATHER_RATE=450 frames; period = 450<<4 = 7200 at 0x006CE7B9",
    "fps": "0x005924BF/0x005924CF: frame counter at game+0x550, idiv 15",
    "peasant_rate": "rules.xml PEASANT_RATE '10 resources' scale 256 -> 2560; used at 0x0063A1D9",
    "oil_rate": "rules.xml OIL_RATE '35 oil' scale 256 -> 8960; used at 0x0063A1C6 for r==5",
    "city_gather": "rules.xml CITY_GATHER [10,10,0,0,0,0]; added <<4 per city at 0x006D5979",
    "commerce_cap": "rules.xml COMMERCE_CAP; written at 0x006CE940 indexed by econ+0xF0",
    "pop_cap": "rules.xml POP_CAP; read at 0x006DC6xx indexed by econ+0xE8 (Military level)",
    "city_limit": "FUN_006D6130 returns econ[0xEC]+1 (+pyramids_city_limit, +bantu_city_limit)",
    "market_taxes": "rules.xml MARKET_TAXES=10; FUN_00737B50 adds it when the city holds type 436",
    "university_literacy": "rules.xml UNIVERSITY_LITERACY=10; FUN_00737C00, city holds type 420",
    "granary_bonus": "rules.xml GRANARY_BONUS; city+0x56 via FUN_00736890, read by FUN_00738360 case 0",
    "costs": "schema/live/{unit,building,tech}-attributes.txt -- read from a running process",
    "cost_factor": "rules.xml UNIT/BUILD/TECH_COST_FACTOR = 10; applied at 0x006645B4 (imul rules+0x354)",
    "cost_ramp": "FUN_00664090 0x00665440: cost += min(SUPPORTVALUE*count + extra, base*RAMP_MAX/100)",
}


# ---------------------------------------------------------------------------
# costs
# ---------------------------------------------------------------------------
def base_cost(rec: dict, factor: int) -> list[int]:
    return [c * factor for c in rec["cost"]]


def ramped_cost(rec: dict, count: int, factor: int, ceiling_pct: int) -> list[int]:
    """FUN_00664090, reduced to the shipped-data case.

    0x00665440:
        for i in 0,1:
            if supportResource[i] == r:
                v = supportValue[i] * count + progressive_extra
                if ceiling != 0 and ceiling < v: v = ceiling      # 0x00665452
                cost[r] += v
    with ceiling = base[r] * RAMP_MAX/100 (0x00665405) and count = the player's owned +
    queued count of the type (0x00665966).  progressive_extra is nonzero only on the
    scholar path.  A ceiling of 0 means NO ceiling -- the opposite of the natural read.
    """
    cost = base_cost(rec, factor)
    for k in (0, 1):
        r = rec["support"][k]
        if r < 0:
            continue
        v = rec["support_cost"][k] * count
        if ceiling_pct:
            ceil_v = cost[r] * ceiling_pct // 100
            if ceil_v and ceil_v < v:
                v = ceil_v
        cost[r] += v
    return cost


# ---------------------------------------------------------------------------
# state
# ---------------------------------------------------------------------------
@dataclass
class State:
    a: Assumptions
    frame: int = 0
    stock: list[int] = field(default_factory=lambda: list(STARTING_GOODS))
    acc: list[int] = field(default_factory=lambda: [0] * 6)
    cities: int = 1
    farms: int = 3          # citytemplates.xml "small" template: 3 Farms + 1 Library
    libraries: int = 1
    woodcamps: int = -1    # -1 => take Assumptions.start_woodcamps
    mines: int = 0
    markets: int = 0
    universities: int = 0
    granaries: int = 0
    lumbermills: int = 0
    smelters: int = 0
    citizens: int = -1     # -1 => take Assumptions.start_citizens
    # citizens assigned to gathering, by resource index
    on: list[int] = field(default_factory=lambda: [0] * 6)
    builders: int = 0
    lvl_mil: int = 0
    lvl_civ: int = 0
    lvl_com: int = 0
    lvl_sci: int = 0
    techs: frozenset = field(default_factory=frozenset)
    log: tuple = ()

    def __post_init__(self):
        if self.citizens < 0:
            self.citizens = self.a.start_citizens
        if self.woodcamps < 0:
            self.woodcamps = self.a.start_woodcamps
        # seat everyone the starting buildings can employ
        sl = self.slots()
        for r in (FOOD, TIMBER, METAL):
            while sum(self.on) < self.citizens and self.on[r] < sl[r]:
                self.on[r] += 1

    def clone(self) -> "State":
        return replace(
            self,
            stock=list(self.stock),
            acc=list(self.acc),
            on=list(self.on),
        )

    # -- capacities -------------------------------------------------------
    @property
    def pop(self) -> int:
        return self.citizens

    @property
    def pop_cap(self) -> int:
        return POP_CAP[min(self.lvl_mil, 7)]

    @property
    def city_limit(self) -> int:
        return self.lvl_civ + 1

    def slots(self) -> list[int]:
        s = [0] * 6
        s[FOOD] = self.farms * self.a.slots_farm
        s[TIMBER] = self.woodcamps * self.a.slots_wood
        s[METAL] = self.mines * self.a.slots_mine
        return s

    def idle(self) -> int:
        return self.citizens - sum(self.on) - self.builders

    # -- income -----------------------------------------------------------
    def income_per_period(self) -> list[int]:
        """Resources per GATHER_RATE frames, after the commerce clamp."""
        gran = GRANARY_BONUS[0] if self.granaries else 0
        lumb = LUMBERMILL_BONUS[0] if self.lumbermills else 0
        smel = SMELTER_BONUS[0] if self.smelters else 0
        bonus = [gran, lumb, 0, 0, smel, 0]
        gross16 = [0] * 6   # engine units: 1/16 resource per period
        for r in range(6):
            rate = OIL_RATE if r == OIL else PEASANT_RATE
            v = (16 * self.on[r] * rate) // 256
            v = v * (100 + bonus[r]) // 100
            gross16[r] += max(v, 0)
        for r in range(6):
            if CITY_GATHER[r]:
                gross16[r] += CITY_GATHER[r] * 16 * self.cities
            gross16[r] += BASIC_GATHER[r] * 16
        gross16[WEALTH] += MARKET_TAXES * 16 * self.markets
        gross16[KNOWLEDGE] += UNIVERSITY_LITERACY * 16 * self.universities
        cap16 = COMMERCE_CAP[min(self.lvl_com, 7)] * self.a.commerce_cap_scale
        out = []
        for r in range(6):
            v = min(gross16[r], cap16)
            v = min(v, 16000)          # 0x006CE706 global ceiling = 1000 res / period
            out.append(v)
        return out

    def gross_uncapped_per_period(self) -> list[float]:
        gran = GRANARY_BONUS[0] if self.granaries else 0
        lumb = LUMBERMILL_BONUS[0] if self.lumbermills else 0
        smel = SMELTER_BONUS[0] if self.smelters else 0
        bonus = [gran, lumb, 0, 0, smel, 0]
        out = [0.0] * 6
        for r in range(6):
            rate = OIL_RATE if r == OIL else PEASANT_RATE
            out[r] = self.on[r] * (rate / 256.0) * (100 + bonus[r]) / 100.0
            out[r] += CITY_GATHER[r] * self.cities
        out[WEALTH] += MARKET_TAXES * self.markets
        out[KNOWLEDGE] += UNIVERSITY_LITERACY * self.universities
        return out

    # -- time stepping ----------------------------------------------------
    def advance(self, frames: int) -> None:
        """Run the engine's accumulator for `frames` frames at the current income."""
        if frames <= 0:
            return
        inc = self.income_per_period()
        period = GATHER_RATE * 16
        for r in range(6):
            i = inc[r]
            if i <= 0:
                continue
            total = i * frames + self.acc[r]
            self.stock[r] += total // period
            self.acc[r] = total % period
        self.frame += frames

    def frames_until_affordable(self, cost: list[int], limit: int = 60 * 60 * 15) -> int | None:
        """Exact number of frames until `cost` is affordable at the present income."""
        need = [max(0, cost[r] - self.stock[r]) for r in range(6)]
        if not any(need):
            return 0
        inc = self.income_per_period()
        period = GATHER_RATE * 16
        worst = 0
        for r in range(6):
            if not need[r]:
                continue
            if inc[r] <= 0:
                return None
            # frames f such that (inc*f + acc) // period >= need
            f = math.ceil((need[r] * period - self.acc[r]) / inc[r])
            worst = max(worst, max(0, f))
        return worst if worst <= limit else None

    def pay(self, cost: list[int]) -> None:
        for r in range(6):
            self.stock[r] -= cost[r]
