# Arena gather upgrades — exact percentage inside a model payout

This is the next decision-weighted correction after the base University/Scholar loop.
Within the first twelve replay minutes, human players issue 973 Granary, Lumber Mill or
Smelter production decisions. The previous three-seed/two-seat `Ai` trace issued none.

The bounded correction is deliberately smaller than “ordinary gathering is exact.” It
adds first-copy enhancer policy, validates the shipped rows, and calls the recovered city
percentage arithmetic. Arena's generated-map Farm/Camp/Mine sources and construction
activation remain their existing declared models.

## Shipped source rows

`schema/live/live-tables-building.tsv` and `live-tables-tech.tsv`, checked at `World::new`,
give the complete generic first-copy source set:

| object | TypeIndex | prerequisites | effective cost | job time | footprint | base bonus |
|---|---:|---|---|---:|---:|---:|
| Granary | 423 | Mathematics 552 + Classical Age 544 | 60 timber + 40 wealth | 1000 | 5×5 | food +20% |
| Lumber Mill | 424 | Mathematics 552 + Classical Age 544 | 60 food + 40 metal | 1000 | 5×5 | timber +20% |
| Smelter | 425 | Chemistry 553 + Classical Age 544 | 70 timber + 50 wealth | 1000 | 5×5 | metal +50% |

All three carry raw `BUILD_FLAGS = 0x08000201`. Mathematics costs 120 wealth + 80
knowledge and takes 275 frames; Chemistry costs 200 wealth + 160 knowledge and takes 350.
Both research at the Library. Any identity, name, cost, job time, prerequisite, producer,
tribe mask, footprint or flag mismatch makes Arena creation fail rather than silently
renaming this tranche.

The policy buys only the first copy of each building. That matters: retail's later-copy
building cost uses the civilian/Worker ramp, while Arena's generic building command still
charges the loaded base row. At owned-plus-queued count zero the base row is the exact
cost, so no unsupported ramp enters the candidate trace.

## Exact arithmetic

`City::calc_gather` `0x00737C60` writes the city bytes from the shipped tables recovered in
`don_sim::systems::tech_cities::calc_gather_enhancers`. At level one they are 20, 20 and
50. `CityData::enhancer_amount` `0x00738360` then computes:

```text
enhanced = (100 + city_bonus_byte) * amount / 100
```

The multiplication precedes signed integer division. Thus a mutated base gross of 11
becomes 13 under a Granary, not 13.2 carried into the accumulator. Only food, timber and
metal select an enhancer byte. `CITY_GATHER` is added outside each building's
`BuildTypeData::calc_gather` result and is not multiplied.

Arena activates this adapter only for a completed enhancer whose stored `Ent::city`
equals the ordinary gather site's stored city. An incomplete building, a sibling city,
another owner, or a different resource does not contribute. The retained authoritative
Farm path returns before this adapter: `AuthoritativeFarmSiteFacts` already owns a frozen
external `CityData::enhancer_amount` snapshot, and silently combining it with Arena state
would double count or invent authority. A source-backed Farm sees a later Granary only
after an explicit re-evaluation/rebind, which this tranche does not add.

## Placement boundary

The shipped `j=0x200` branch inside `BuildTypeData::blocked_location` enters an
overlapping-city admission graph. Arena has one nearest-city link rather than the complete
city-building lists and overlap transaction. It therefore implements a conservative
projection: no second enhancer of the same type may link to the same nearest completed
owner-city, while a nearer completed sibling city may admit its own copy. This is MODEL,
not the exact retail placement graph. The `Ai` policy is stricter still—one copy globally—
so its measured opening decisions never depend on that ambiguity.

## Exact Market-tax enabler

The original candidate stalled before Mathematics for an economic reason, not a placement
reason. Written Word spent the starting wealth down to 50; an early University reduced it
to 20; Mathematics requires 120. Arena had no renewable wealth source, so putting an
unaffordable Mathematics first also blocked `next_tech` from reaching Barter.

The bounded enabler is one completed Market, not a general trade model. For every completed
owner city, Arena takes a completed same-`Ent::city` building census and calls recovered
`tech_cities::city_taxes(&CityRules::RETAIL, ...)`. The shipped constants are zero village,
building and Temple taxes plus ten for the Market. `calc_city_resources` stores this at
sixteenths scale, so one Market contributes 160 gross through the exact 7,200-point
accumulator and credits exactly ten wealth per 450 frames. Incomplete, foreign and unlinked
Markets pay nothing; multiple Markets in one city remain one boolean source.

This exact source accumulator is separate from Arena's ordinary MODEL-period accumulator.
Its later composition with model upkeep and commerce caps is therefore not promoted to a
whole-economy claim. Porcelain Tower scaling, Caravan/Merchant production and every trade
route remain red. After City State/Written Word the policy reads the loaded Barter and
University costs: it researches Barter before Classical only when both timber charges are
already banked; otherwise Classical/University precede Barter. It prioritises the Market,
reserves a public founder-local University footprint before paying an early Market, holds
optional construction until that reservation is consumed, allows one early Scholar decision
per University, and reserves further Scholar ramp costs until Mathematics. Thus the first
gather-upgrade path starts from normal stock and Market tax without injected wealth or
Scholar income; the University and one Scholar preserve the prior policy surface but their
knowledge output does not fund Mathematics.

## Policy and evaluation meaning

The observation-only `Ai` researches City State, Written Word, Barter, Classical Age and
Mathematics to expose the first two buildings; Chemistry follows the later University-fed
knowledge loop. Placement and affordability are read from `Obs` and the validated live
rows. Scholar state is never consulted to reach Mathematics or decide whether a Granary,
Lumber Mill or Smelter is useful; Chemistry's shipped knowledge cost merely makes the
existing feedback loop economically relevant.

An accepted trace row means `World::submit` charged and installed a building site. It does
not claim completion-time parity. A changed generated-map income proves that the exact
percentage is wired into the Arena evaluation feedback loop, but the composed payout
remains red because ordinary Camp/Mine terrain geometry, generated Farm terrain facts,
seating, construction activation, national exceptions and higher BonusType/property
levels are not all authoritative.

`arena_gather_upgrades.rs` pins source mutation, exact integer ordering, Market completion
and city census, enhancer completion and city locality, sibling-city admission, policy
affordability mutation, and fixed seeds in both seats. Because the enabling Market now also
represents the wealth family, `analysis/ai/test_opening_envelope.py` proves that a trace with
Market plus gather upgrades has no remaining wholly zero economic family. That does not
erase its missing Caravan/Merchant types; the analyzer directs the next comparison to
timing/type gaps rather than inventing another zero-coverage world correction.
