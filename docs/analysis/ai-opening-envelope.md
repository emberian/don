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
  retain declared MODEL paths. Base University/Scholar mechanics, gather-enhancer source
  rows and city arithmetic, and the base completed-Market tax source call recovered exact
  owners inside that larger non-retail Arena. Applying enhancer percentages to ordinary
  generated-map payout remains MODEL.
- The replay setup is not stratified.
- The shipped opening trace is not the compiled retail AI.
- Accepted issue time is comparable to a replay production command; completion time is not.

Consequently, bot-vs-bot outcomes are deliberately absent. A win would jointly measure the
two policies and these incomplete Arena physics.

The gather-upgrade bounded three-seed/two-seat run produced six traces per policy. `Ai`
had 153 accepted queue/build decisions, including 96 basic-labour, **six Markets, six
Universities, eight Scholars, and four Lumber Mills**. Every run accepted Barter, Market
and University. Mathematics appeared in 5/6 runs at frames 8,610..10,425 (p50 8,625), and
Lumber Mill in 4/6 at 8,895..8,911 (p50 8,910). Those purchases used ordinary starting
stock plus completed-Market tax, never injected wealth or Scholar income. Granary and
Smelter remained absent. `ShippedOpening` remained 160 accepted decisions with six Markets
and zero knowledge or gather upgrades.

The prior knowledge surface remains exercised: Scholar appeared in 4/6 runs, eight accepted
decisions total, at frames 5,445..5,475. The policy permits one early Scholar per University
but reserves later ramped purchases until Mathematics so Scholar spending cannot consume
the tax bank indefinitely. Candidate rows are Arena issue measurements, not human timing
targets or completion-parity claims.

## Correction outcome and next boundary

The former zero-coverage **gather upgrades** family is now represented. Live rows validate
the first-copy six-slot costs, prerequisites, job times, footprints and flags; recovered
city tables yield 120% food, 120% timber and 150% metal; multiply-before-divide makes an
11-point food source become 13. The deterministic economy test keeps the authoritative
Farm snapshot separate and applies these percentages only to the explicitly MODEL
generated-map ordinary term.

The enabler is equally bounded. `CityData::get_taxes` over a completed same-city census
contributes `10 << 4` Market wealth gross, exactly ten wealth per 450 frames through its
own 7,200-point accumulator. Incomplete or unlinked Markets pay zero. Porcelain Tower,
Caravan, Merchant and trade remain absent, and composition with Arena's MODEL-period
ordinary economy does not become a whole-economy authority claim.

Because the measured `Ai` trace now contains Market, Lumber Mill and University, every
ranked economic family appears at least once. The analyzer therefore returns
`no_zero-coverage_family` and directs the next investigation to per-type/timing gaps rather
than inferring another world correction. This does not erase the missing Scholar, Granary,
Smelter, Caravan or Merchant rows; Scholar alone still carries 4,134 human decisions.
Knowledge timing and the remaining gather types need policy/construction evidence, while
Camp/Mine terrain payout and the overlapping-city placement graph remain explicit MODEL
boundaries. Full derivations are in `docs/mechanics/arena-knowledge-economy.md` and
`docs/mechanics/arena-gather-upgrades.md`.

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
