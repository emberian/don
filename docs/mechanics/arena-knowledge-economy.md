# Arena base University knowledge economy

## Claim

The playable `don-ai` Arena now owns one bounded recovered lifecycle:

```text
completed city University (420)
  -> Queue Scholar (52, or Korean graft 53)
  -> retain the live Unit off-map inside its University, cap 7
  -> credit flat city literacy + base-level Scholar gross
```

This is an evaluation-surface correction. It is not a reconstruction of retail AI, a
whole-Arena fidelity promotion, or evidence that a particular opening is strong.

## Static facts and costs

The identities and authored fields come from `schema/live/live-tables-{unit,building}.tsv`,
the checked-in live tables:

| product | TypeIndex | WHERE / PREQ | base cost | authored job time |
|---|---:|---|---:|---:|
| University | 420 | Classical Age 544 | 60 timber + 30 wealth | 420 frames |
| Scholar | 52 | University 420 | 30 wealth | 75 frames |
| Korean Scholar | 53 | University 420 | 30 wealth | 75 frames |

The Scholar live row also supplies `support=[2,-1]`, `support_cost=[2,0]`, and
`progression=0`. `World::production_cost` calls the recovered
`production::ramp_cost`, using the exact-type owned+queued count. In Arena's admitted
no-Maize/no-militia domain, pre-purchase counts `n=0..7` therefore cost
`30,32,34,36,38,40,42,44` wealth. The queue gate counts both Scholar graft rows toward the
University cap and refuses the eighth purchase without payment.

The 75-frame value is the authored live `JOB_TIME` used by Arena's existing queue. Retail's
effective `ObjectData::train_time` also applies `UNIT_RATE_BASE`, the owned-type
`JOB_EXTRA_TIME` ramp, age penalty, and nation/wonder/game modifiers. Those are not silently
claimed by this Arena slice.

## Containment and capacity

`knowledge_economy::KnowledgeObject` retains `(who,o,uid)`; `KnowledgeEconomy` maps every
Scholar identity to one registered University identity. `MAX_KNOWLEDGE_GATHERERS=7` comes
from `don_sim::systems::gathering`, recovered from
`BuildTypeData::max_knowledge_gatherers` `0x006365E0`.

Completion follows the recovered production branch's admitted result:
post-allocation University occupancy is at or below `BuildData::gather_max`, so the route is
`UnitPlacementRoute::CheckGatherers`. The newly allocated Unit remains live for population,
counting, observation, and scoring, but its Guy collision stamps, WData anchor, and target
row are removed. Contained Scholars are skipped by Unit processing and both fog passes.
They consequently cannot move, heal, acquire targets, or reveal terrain.

Citizens are explicitly not University gatherers. A founding Citizen becomes idle on
completion, `Gather` accepts only the ordinary Farm/Camp/Mine family, and the legacy
`PEASANT_RATE * workers` term is restricted to that same family.

## Knowledge arithmetic

Two recovered owners contribute independently:

1. `tech_cities::city_literacy(CityRules::RETAIL, true, false)` contributes 10 stock units
   for a completed University assigned to a city. The Arena shifts this to gross 160.
2. `economy::scholar_rate_for_level(EconRules::shipped(), 1)` contributes gross 80 per
   base-level Scholar. `gathering::site_gross` clamps active membership to seven.

Thus one full base University produces gross `160 + 7*80 = 720`. Knowledge is passed through
the existing exact `resource_tick` and a `GATHER_RATE << 4 = 7200` accumulator, independently
of Arena's ordinary MODEL-3 gather scale:

```text
720 gross/frame * 450 frames / 7200 = 45 knowledge
```

The deterministic integration test resets only the knowledge stock/accumulator, leaves a
deliberately bogus `University.workers=7`, and still obtains exactly 45. That mutation pins
that no Citizen `PEASANT_RATE` shortcut entered the result.

## Authority boundary

- University construction placement/completion still runs through Arena's labelled
  `ResearchModel`; accepted replay-comparable issue times do not promote that model.
- University levels 2..6 are selected by BonusTypes/properties `0x2E1..0x2E5`, not by age.
  Arena does not host that resolver and stays at level 1.
- The queue cap makes the retail overflow/`come_out` arm unreachable in this slice.
- Contained-Scholar and University destruction/ejection teardown still needs the general
  contained-object close owner. Knowledge gross stops immediately when the University is no
  longer a live completed active site, but this lane does not claim the missing teardown.
- Scholar militia, Maize ramping, nation/wonder discounts, dynamic train-time modifiers,
  University building-cost ramp beyond the admitted first policy University, and later
  University research remain outside the claim.

## Verification

`crates/don-ai/tests/arena_knowledge_economy.rs` pins type data, price mutation, cap/refusal,
off-map processing and fog behavior, founder release, exact income, and fixed multi-seed /
both-seat accepted decisions. `analysis/ai/test_opening_envelope.py` separately proves that
once University and Scholar appear, the decision-weighted zero-coverage diagnostic advances
to the next family rather than continuing to recommend completed work.
