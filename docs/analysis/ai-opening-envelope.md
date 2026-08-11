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
substitutes one for the other. It also prints all declared confounds:

- Arena travel, placement, terrain-derived gather capacity, gross income, construction,
  combat, and map generation are physics under test.
- The replay setup is not stratified.
- The shipped opening trace is not the compiled retail AI.
- Accepted issue time is comparable to a replay production command; completion time is not.

Consequently, bot-vs-bot outcomes are deliberately absent. A win would jointly measure the
two policies and these incomplete Arena physics.

The bounded three-seed/two-seat run used to validate the artifact produced six traces per
policy. `Ai` had 131 accepted production decisions: 72 basic-labour, five city-expansion,
and **zero knowledge, wealth, or gather-upgrade decisions**. `ShippedOpening` had 160:
126 basic-labour, six city-expansion, six Markets, and zero knowledge or gather upgrades.
Those counts describe these policy implementations inside Arena; they are not retail-AI
measurements.

## Highest-value correction

The decision-weighted diagnostic priority is the **knowledge economy**: add University
placement, Scholar production, and the associated knowledge-income accounting to the
independent AI/Arena evaluation path. The current `Ai` action surface has no University or
Scholar production, while those types account for 4,665 human production decisions in the
bounded envelope—more than the wealth and gather-upgrade families combined.

This is a model-coverage correction, not a claim that humans always build Scholars, that
retail AI follows the human distribution, or that adding Scholars increases skill. It is
the next correction because the current evaluator cannot represent a heavily exercised
human economic feedback loop at all. After it exists, timing calibration must be stratified
by replay setup or differentially tested against retail before any p50 comparison can be a
fidelity claim.

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
coverage by disappearing. The tests pin the zero-AI-command corpus fact and the
decision-weighted knowledge-economy diagnosis.
