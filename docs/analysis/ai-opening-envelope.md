# AI opening envelope — decisions, not a skill score

`analysis/ai/opening_envelope.py` is a bounded evaluation artifact for the independent,
observation-only `don-ai` player. It answers one useful question:

> Which economically important surfaces do human players exercise in the shipped replay
> corpus that our current AI never exercises in the same 12-minute Arena trace?

It does **not** answer whether DoN's AI is stronger than a human or than retail's AI. It
emits no Elo, no scalar match score, and no retail-AI imitation target.

## What the corpus actually contains

The checked-in 61-recording command table contains 301 present player slots: 135 human and
166 non-human. Across the non-human slots it contains **zero production command rows and
zero production decisions**. That is expected lockstep behaviour: every client runs an AI
from shared state, so its decisions are not transmitted and cannot appear in `.rcx`.

Within the first 12 game-minutes, 115 human player-games issue 42,543 production decisions.
The analyzer retains each type's human first-issue min/p10/p50/p90/max and weights economic
families by the actual command counts:

| family | human decisions | fraction of production | component types |
|---|---:|---:|---|
| basic labour | 11,089 | 26.07% | Citizen, Farm, Fishermen, Woodcutter's Camp, Mine |
| **knowledge economy** | **4,665** | **10.97%** | Scholar 4,134; University 531 |
| gather upgrades | 973 | 2.29% | Granary, Lumber Mill, Smelter |
| wealth economy | 837 | 1.97% | Market, Caravan, Merchant |
| city expansion | 580 | 1.36% | Small City |

These are weights, not a canonical build order. The replay table does not yet stratify
starting age/resources, game speed, nation, map, or victory options. For example, first
Scholar issue spans frames 56..7690 and first University 17..7302. The ranges are a human
envelope across heterogeneous games, not a target time for a standard Ancient start.

## What “shipped production AI is reachable” means

`crates/don-sim/src/systems/leader_production_ai.rs` now executes the exact entry/dispatch
of `Leader::plan_strategy` and the complete 628-byte `Leader::production_ai` step machine.
That is important: tick step 11 can prove when retail would reach `found_cities`,
`research_techs`, `create_units`, `create_buildings`, and the other production boundaries.

It does not yet execute any of the compiled decision-stage bodies. The analyzer records
that distinction as `shipped_production_ai_audit`: nine reached boundaries, zero owned
decision bodies. `ShippedOpening` in the candidate trace is only the shipped
`economic.bhs` cases 6..18 purchase order. It is not a replay-mined retail AI and it is not
the full compiled retail player.

## Candidate trace

`crates/don-ai/src/bin/ai_opening_trace.rs` runs the shipped opening book and our independent
`Ai` beside an inert leader, on identical deterministic Arena seeds and in both seats. It
records only accepted `Queue` and `Build` issue times. Unaffordable retries, movement,
combat commands, and completion events do not enter the trace.

The JSON report keeps candidate first-issue traces beside the human bounds; it never
substitutes one for the other. It also prints the recovered base-University runtime audit
and all declared confounds:

- Arena travel, ordinary gather capacity/income, construction, combat, and map generation
  retain declared MODEL paths. Base University capacity, contained Scholar placement,
  production-cost ramp, literacy, and Scholar gross now call recovered exact owners inside
  that larger non-retail Arena.
- The replay setup is not stratified.
- The shipped opening trace is not the compiled retail AI.
- Accepted issue time is comparable to a replay production command; completion time is not.

Consequently, bot-vs-bot outcomes are deliberately absent. A win would jointly measure the
two policies and these incomplete Arena physics.

The post-correction bounded three-seed/two-seat run produced six traces per policy. `Ai`
had 141 accepted production decisions: 89 basic-labour, three city-expansion, **six
Universities and ten Scholars**, and zero wealth or gather-upgrade decisions. University
appeared in 6/6 runs; Scholar appeared in 5/6. Candidate first issue was University
frames 3,976..9,061 (p50 5,100) and Scholar 5,086..9,106 (p50 5,190). These are Arena issue
measurements, not timing targets; notably, one accepted University did not complete in the
horizon through the still-modelled construction path. `ShippedOpening` remained 160:
126 basic-labour, six city-expansion, six Markets, and zero knowledge or gather upgrades.

## Correction outcome and next boundary

The previous diagnostic priority, **knowledge economy**, is no longer a zero-coverage
family. The independent policy now places a University from public placement facts, queues
the tribe-roster Scholar through exact ramped affordability, and the Arena retains completed
Scholars off-map inside the University. One completed city University with seven base
Scholars credits exactly 45 knowledge per 450 frames: ten flat literacy plus seven times
five Scholar income. The deterministic test also proves contained Scholars do not act or
reveal fog and Citizens cannot enter the University's income term.

With both University and Scholar present, the same zero-coverage ranking advances to
**gather upgrades**: Granary, Lumber Mill, and Smelter account for 973 / 42,543 human
production decisions (2.29%). That is the next missing family, not an assertion that it is
the best strategic purchase. Knowledge timing calibration still needs setup stratification
or retail differential evidence before any candidate/human p50 comparison can become a
fidelity claim.

The knowledge lifecycle's authority boundary is explicit in
`docs/mechanics/arena-knowledge-economy.md`: University construction remains Arena
`ResearchModel`; University levels 2..6 require an unhosted BonusType/property resolver;
dynamic retail train-time modifiers and contained-object destruction/ejection teardown are
not claimed.

## Reproduction

```sh
python3 -m unittest analysis/ai/test_opening_envelope.py -v

cargo run -q -p don-ai --bin ai_opening_trace -- \
  --minutes 12 --seeds 3 > /tmp/don-ai-opening-trace-v1.tsv

python3 analysis/ai/opening_envelope.py \
  --trace /tmp/don-ai-opening-trace-v1.tsv --minutes 12 --pretty
```

Omitting `--trace` makes the analyzer run the same bounded Cargo trace itself. The trace
schema is versioned (`don.ai.accepted-production-trace.v1`), malformed or unlabelled input
fails closed, and its minutes/seeds/FPS metadata prevents a 12-minute trace from being
silently scored against a different human horizon. The analyzer also requires both seats
for every configured seed and both fixed policies, so a vanished/empty run cannot improve
coverage by disappearing. The tests pin the zero-AI-command corpus fact, the original
knowledge-economy diagnosis for a trace missing that family, and the advance to gather
upgrades once University and Scholar rows are present.
