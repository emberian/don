# RoNEval policy evaluation

RoNEval is a deterministic, batched evaluator for five policy surfaces over the headless
arena:

- `ShippedOpening`: the shipped `economic.bhs` purchase order;
- `CapFirst`: the optimiser-derived opening;
- `Marshal`: the baseline non-income-cheating combat player;
- `MarshalHeads`: the same Marshal routed through `don-env`'s ten action heads and back;
- `Ai`: the observation-only improved-edition opponent.

This is useful evaluation infrastructure, not a fidelity promotion. Every report labels
itself `arena-model-evaluation-only`, sets `roneval_release_ready=false`, and lists the
arena's remaining model blockers. Its 64-bit checksums are stable identities for evaluator
results; they are deliberately named `result_checksum` and are **not** retail
`CheckSum::walk_data` values.

## Reproduce

Run the full round robin on an otherwise idle host for meaningful throughput:

```sh
cargo run --release -p don-ai --bin roneval -- \
  --minutes 8 --seeds 4 --threads 8
```

For each seed the schedule contains every unordered pairing in both seat orders: twenty
matches per seed and eight appearances per policy, four in each seat. The seed sequence and
report order are fixed. The harness runs the whole schedule once serially and once at the
requested worker count; it fails closed on the first result mismatch. Throughput describes
the requested-worker run only and includes arena construction, policy decisions, command
conversion, simulation, and result aggregation. It excludes model inference and
accelerator transfer.

The report does not collapse timeouts into an invented weighted score. It retains
elimination wins and losses, timeout-tiebreak wins and losses, draws, command
acceptance/refusal/invalid counts, and raw final resource, damage, and army-value totals.
The timeout ordering is the arena's documented ordering: cities standing, then damage
dealt, then resources.

## Ai certification and holdout seeds

The fast direct schedule runs only `Ai` versus Marshal, both seats on every seed:

```sh
cargo run --release -p don-ai --bin roneval -- \
  --strong-only --minutes 20 --seeds 16 --seed-offset 8 --threads 8 --require-ai
```

`--seed-offset` selects a deterministic, disjoint block from the same seed generator. A
normal workflow tunes on offsets 0–7, freezes the policy, then certifies once on offsets
8–23. `--require-ai` exits nonzero unless all of these hold:

1. at least sixteen seeds have a complete two-seat pair;
2. `Ai` wins more decisive seed pairs than it loses;
3. the exact one-sided sign-test tail is at most 0.05;
4. the `Ai` policy's aggregate raw damage delta is positive.

The unit of statistical evidence is the **seed pair**, not the match. The two matches on a
map share terrain, starts, and simulation peculiarities and are not independent trials. A
pair is an `Ai` win when it wins more of the two seat assignments, a loss when it wins
fewer, and a tie otherwise; tied pairs are excluded from the exact sign test. Match wins
and every raw score delta remain visible for diagnosis.

Set `DON_RONEVAL_LOG=1` for deterministic per-frame arena events in the direct report. The
event trace participates in serial-versus-parallel equality but not the stable result
checksum, so diagnostic text cannot change result identity.

## Non-cheating observation contract

Every evaluated bot receives only `arena::obs::Obs` and changes the game only through
`World::submit`. `Obs::known` contains current sightings and last-known enemy state under
fog. A remembered entity outside current line of sight is not queried against the
authoritative object table: bots can move to its last location and attack only after the
target is currently visible. `Ai` also has a source-level regression gate forbidding a
direct `Obs`-to-`World` escape hatch.

`MarshalHeads` is a seam test rather than a new strategy. On every CapFirst probe, native
Marshal and MarshalHeads must have the same outcome, scores, command accounting, and
evaluation-result checksum. This protects the claim that a learned policy can replace a
heuristic through the public action layout without receiving a richer world interface.

## Determinism and readiness gates

The reusable `don_env::eval::ordered_parallel_map` assigns cases dynamically but restores
global input order. Seeds belong to cases, never workers. Its unit gates compare one, two,
and eight workers and freeze the cross-platform byte image of `EvalDigest`.

The arena evaluator additionally gates:

1. balanced seat counts and stable schedule order;
2. native-Marshal versus MarshalHeads result equivalence;
3. full-batch serial versus requested-thread report and checksum equality;
4. complete paired-seed accounting for the strong-AI test;
5. fog-safe observations and disabled difficulty income bonus in the readiness block.

Current blockers are printed into every report: arena fidelity remains explicitly below a
retail differential certification; full water/naval/air/diplomacy/attrition/supply dynamics
are absent; the evaluator checksum is not a retail lockstep channel; and the harness does
not time policy inference or device transfer. Those facts keep the current tool honest
while leaving its schedule and reporting format ready for a more authoritative simulator.
