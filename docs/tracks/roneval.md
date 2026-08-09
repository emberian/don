# RoNEval policy evaluation

RoNEval's Cycle-6 harness compares four policy surfaces over a balanced batch of arena
matches:

- `ShippedOpening`: the shipped `economic.bhs` purchase order;
- `CapFirst`: the optimiser-derived opening;
- `Marshal`: the strong, non-income-cheating heuristic player;
- `MarshalHeads`: the same Marshal routed through `don-env`'s ten action heads and back.

This is useful evaluation infrastructure, not a fidelity promotion. The report labels itself
`arena-model-evaluation-only`, sets `roneval_release_ready=false`, and lists the arena's
remaining model blockers. Its 64-bit checksums are stable identities for evaluator results;
they are deliberately named `result_checksum` and are **not** retail
`CheckSum::walk_data` values.

## Reproduce

Run on an otherwise idle host for meaningful throughput:

```sh
cargo run --release -p don-ai --bin roneval -- \
  --minutes 8 --seeds 4 --threads 8
```

For each seed the schedule contains every unordered pairing in both seat orders: twelve
matches per seed, six appearances per policy. The seed sequence and report order are fixed.
The harness runs that whole schedule once serially and once at the requested worker count;
it fails closed on the first result mismatch. Throughput describes the requested-worker run
only and includes arena construction, policy decisions, command conversion, simulation, and
result aggregation. It excludes Python, model inference, and accelerator transfer.

The report does not collapse timeouts into an invented score. It retains elimination wins
and losses, timeout-tiebreak wins and losses, draws, command acceptance/refusal/invalid
counts, and raw final resource, damage, and army-value totals. The timeout ordering is the
arena's existing documented ordering: cities standing, then damage dealt, then resources.

## Non-cheating observation contract

Every evaluated bot receives only `arena::obs::Obs` and can change the game only through
`World::submit`. `Obs::known` contains current sightings and last-known enemy state under
fog. A remembered entity outside current line of sight is not queried against the
authoritative object table: Marshal walks to the last sighting and can attack only after the
tile becomes visible again. The Cycle-6 audit removed one such liveness query and added the
narrow `Obs::visible(tile)` predicate.

`MarshalHeads` is a direct seam test rather than a new strategy. On every CapFirst probe,
native Marshal and MarshalHeads must have the same outcome, scores, command accounting, and
evaluation-result checksum. This protects the claim that a learned policy can replace a
heuristic through the public action layout without receiving a richer world interface.

## Determinism and readiness gates

The reusable `don_env::eval::ordered_parallel_map` assigns cases dynamically but restores
global input order. Seeds belong to cases, never workers. Its unit gates compare one, two,
and eight workers and freeze the cross-platform byte image of `EvalDigest`.

The arena evaluator additionally gates:

1. balanced seat counts and stable schedule order;
2. native-Marshal versus MarshalHeads result equivalence;
3. full-batch serial versus requested-thread report equality and checksum equality;
4. fog-safe observations and disabled difficulty income bonus in the emitted readiness
   block.

Current blockers are printed into every report: arena fidelity is still Tier C and not
differentially measured against retail; full water/naval/air/diplomacy/attrition/supply
dynamics are absent; the evaluator checksum is not a retail lockstep channel; and the
harness does not time policy inference or device transfer. Those facts keep the current
tool honest while leaving its schedule and reporting format ready for the authoritative
simulator as each gate closes.
