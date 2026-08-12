# Arena gather upgrades — exact percentage inside a model payout

This is the next decision-weighted correction after the base University/Scholar loop.
Within the first twelve replay minutes, human players issue 973 Granary, Lumber Mill or
Smelter decisions plus 370 decisions across their nine gather-enhancer researches. The
complete measured lifecycle is therefore 1,343 decisions, not the earlier building-only
count.

The bounded correction is deliberately smaller than “ordinary gathering is exact.” It
adds first-copy enhancer policy, validates the shipped rows, and calls the recovered city
percentage arithmetic. Arena's generated-map Farm/Camp/Mine sources and construction
activation remain their existing declared models.

## Shipped source rows

`schema/live/live-tables-building.tsv` and `live-tables-tech.tsv`, checked at `World::new`,
plus `rules.xml`, checked by `load_world`, give the complete generic source set:

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

The same validation covers the actual gather research rows:

| chain | TypeIndex | effective costs | job times | producer |
|---|---|---|---|---|
| Carpentry → Logging Industry → Papermill | 609 → 610 → 611 | 150/250/450 food + the same metal | 300/350/450 | Lumber Mill 424 |
| Agriculture → Crop Rotation → Food Industry | 612 → 613 → 614 | 150/250/450 timber + the same metal | 300/350/450 | Granary 423 |
| Metal Alloys → Cold Casting → Steel | 620 → 621 → 622 | 250/350/450 food + the same timber | 350/400/450 | Smelter 425 |

Their age/science prerequisites also come from the loaded rows. The `rules.xml`
`TECHBONUSES` order is validated at BonusTypes 699..711: Agriculture/Crop Rotation/Food
Industry map to Granary levels 2..4, Carpentry/Logging Industry/Papermill to Lumber levels
2..4, and Metal Alloys/Cold Casting/Steel to Smelter levels 2..4. A source mutation fails
before the match loads.

The policy buys only the first copy of each building. That matters: retail's later-copy
building cost uses the civilian/Worker ramp, while Arena's generic building command still
charges the loaded base row. At owned-plus-queued count zero the base row is the exact
cost, so no unsupported ramp enters the candidate trace.

## Exact arithmetic

`City::calc_gather` `0x00737C60` writes the city bytes from the shipped tables recovered in
`don_sim::systems::tech_cities::calc_gather_enhancers`. Granary and Lumber levels 1..4 are
20/50/100/200; Smelter levels 1..4 are 50/100/150/200.
`LeaderData::get_granary` `0x006DB340`, `CityData::lumber_level` `0x00736820`, and
`LeaderData::get_smelter` `0x006DB3F0` define the descending held-property precedence.
`CityData::enhancer_amount` `0x00738360` then computes:

```text
enhanced = (100 + city_bonus_byte) * amount / 100
```

The multiplication precedes signed integer division. Thus a mutated base gross of 11
becomes 13 under a base Granary, not 13.2 carried into the accumulator; Agriculture raises
the same source to 16 immediately after its TechType completes. Only food, timber and metal
select an enhancer byte. `CITY_GATHER` is added outside each building's
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
per University, and reserves further Scholar ramp costs until Chemistry. Thus the first
gather-upgrade path starts from normal stock and Market tax without injected wealth; the
University and one Scholar preserve the prior policy surface.

The adapter is intentionally Roman-bounded. The shipped descriptors for Greek research
speed/cost and Egyptian/French early Granary/Lumber enable/grant properties are validated,
but Arena has no nation-power host for them. Non-Roman games therefore do not receive a
false exact higher-level claim from this adapter.

## Policy and evaluation meaning

The observation-only `Ai` researches City State, Written Word, Barter, Classical Age and
Mathematics to expose the first two buildings; Chemistry follows the later University-fed
knowledge loop, and Carpentry/Agriculture follow only through their shipped completed
producers. Placement and affordability are read from `Obs` and the validated live rows.

An accepted trace row means `World::submit` charged and installed a building site. It does
not claim completion-time parity. A focused completed-producer test pins Carpentry's exact
150 food + 150 metal charge, 300-frame countdown, held-tech insertion and dynamic payout.
A changed generated-map income proves that the exact percentage is wired into the Arena
evaluation feedback loop, but the composed payout remains red because ordinary Camp/Mine
terrain geometry, generated Farm terrain facts, seating, construction activation and
national exceptions are not all authoritative.

The natural 30-minute Roman cohort reaches Chemistry and completes exactly one Granary,
Lumber Mill and Smelter in all four fixed seed/seat runs without injected resources. Each
5x5 site is paid from the live stock, placed through the public predicate, approached by its
reserved founder, and completed by ordinary `BuildData` frames. Producer-gated research is
therefore admitted only after the corresponding completed building; no incomplete producer
or direct completion path is used. Full retail multi-founder swarm formation and dynamic
crowd assignment remain outside this Arena subdomain.

`arena_gather_upgrades.rs` pins source mutation, exact integer ordering, Market completion
and city census, enhancer completion and city locality, sibling-city admission, policy
affordability mutation, exact research queue completion, dynamic held-tech effects, and
fixed seeds in both seats. `analysis/ai/test_opening_envelope.py` ranks missing type weight
even after a family appears once: Market/University/Scholar plus one Lumber row still leave
a 1,025-decision gather lifecycle gap. This is a correction priority, not retail-AI,
timing-parity, Elo, or whole-economy evidence.
