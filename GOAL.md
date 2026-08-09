# Descent of Nations — execution board

This is the live project board. The human overview is [`README.md`](README.md), the agent
operating manual is [`README-LLM.md`](README-LLM.md), and the durable evidence rules are in
[`docs/CHARTER.md`](docs/CHARTER.md).

## North star

Build Descent of Nations into all three of these, using one authoritative game core:

1. **An independent, complete, enjoyable successor to _Rise of Nations_** that a strong
   human can play seriously and keep improving.
2. **A defensible fidelity reference** whose mechanics can be compared to retail machine
   code, live behavior, and replay checksums.
3. **A high-performance RL and evaluation platform** capable of training strong non-cheating
   agents and eventually hosting RoNEval.

Retail fidelity defines the reference behavior. Improved mode names and governs deliberate
departures. Reachable approximations are drift, not features, and block readiness.

## Current measured snapshot

Snapshot date: 2026-08-08. Generated records override prose if the tree advances.

| Surface | State | Evidence |
|---|---|---|
| Retail differential oracle | green | `schema/oracle-regression.json`: 18/18 cases, 17,573,073 trials, zero mismatches/skips/crashes/errors; includes full-state turning and map/start anchors |
| Replay corpus | structurally green, dynamically incomplete | 61 recordings, 585,152 turns, 488,557 valid checksum packets; initial-world reconstruction is still absent, so current matches are not non-trivial whole-game agreement |
| Simulation | broad and actively integrating | retail tick/order foundations plus economy, production, combat, movement/A*, collision/boats, spatial chains, groups/guys, cities, borders/fog, air, naval, walls, items, animation events, RNG, walkers |
| Product readiness | intentionally red | default improved seams are wired; current reachable drift is tracked by `tools/product-readiness.sh`, not waived |
| RL environment | working over incomplete dynamics | native/Gymnasium/VectorEnv/PettingZoo APIs, derived action taxonomy, parameter masks, zero-copy observations, deterministic batch stepping; performance remeasurement is active |
| AI / playable arena | working integration, not release-ready | deterministic observation-driven Marshal, real command path and A* movement; remaining arena world models are declared product blockers |
| Browser | working integration build | WebGPU/WebGL2/Canvas2D, responsive touch/desktop command dock, cluster/replay views, wasm/native digest paths; labels do not claim whole-game fidelity |
| Live retail | gameplay-validated | RoNtoy coach/overlay plus reversible versioned command hook, exact movement trajectory, and fog-safe player-observation work |
| BHS / content | substantial, fail-closed | compiler, decoder, VM, aggregate execution, builtin registry, overlays and mod paths; current fidelity work is resolving retail type/member quirks rather than guessing |
| License/workflow | established | GPL-3.0-or-later; permanent `dev` branch; no branches or worktrees |

## The gates

```sh
cargo check --workspace
cargo test --workspace --all-targets
tools/oracle-regress.sh
tools/replay-validate.sh
tools/product-readiness.sh
```

`product-readiness.sh` is expected to refuse while a reachable drift entry remains. A green
workspace test does not promote a fidelity tier; a green oracle case proves only its stated
finite domain; replay agreement is substantive only when the corresponding channel walks
non-empty reconstructed state.

## Active broad frontier

These fronts advance in parallel. Do not collapse the project into only the first item that
has an obvious test.

### 1. Independent game core

- Complete deterministic map generation, start placement, nation/team/diplomacy setup, and
  game-mode/victory state from a pinned seed.
- Finish command → queue → order → system execution for every gameplay opcode, including
  group and air patrol, construction, gathering, combat, spells, transport, naval, air, and
  diplomacy.
- Replace the arena’s remaining MODEL substitutions with systems at least as sophisticated
  as retail: builder/construction state, terrain/resource occupancy, target acquisition,
  tactical/flank inputs, and the missing water/air/diplomacy/attrition/supply game.
- Complete animation/event/projectile/plane release, save/load, scenario, BHS, and content
  integration.
- Provide independently licensed presentation assets or a legal extractor-backed path for
  a distributable edition.

### 2. Replay fidelity

- Parse Game/GameInfo setup, rules, players, teams, seed, map parameters, and initial command
  packages from `.rcx`.
- Deterministically reconstruct the initial world; `.rcx` is not a complete `.svx` state
  dump.
- Populate the generated PDB-shaped bridge in retail object/container order, including
  capacities and growth metadata where walked.
- Apply decoded commands through the same path used by the game, RL environment, and browser.
- Turn at least one channel from structurally/trivially green into non-empty agreement, then
  record and reduce its first divergence.
- Close every RNG stream consumer as its owning system becomes reachable.

The scoreboard is:

```sh
tools/replay-validate.sh
```

It compares the fifteen simulation-state channels carried by retail checksum packets. The
current record is `schema/replay-validation.json`; caveats are in
[`docs/tracks/replay-validation.md`](docs/tracks/replay-validation.md).

### 3. RL and RoNEval

- Replace every `accepted_no_effect` gameplay verb with the authoritative dynamics or mask
  it out until those dynamics exist.
- Preserve exact per-parameter masks; an unmasked RTS action space is not acceptable.
- Benchmark reset, step, observation, mask, reward, sampling, memory, and serialization
  separately across batch sizes and thread counts. Keep only measured speedups with complete
  semantic/determinism equivalence.
- Build scenario packs, deterministic seeding, save/restore, vectorized opponent pools,
  league ratings, behavioral probes, and retail/DoN paired evaluation suitable for RoNEval.
- Keep Fidelity mode as the reference and measure which explicit improved-mode relaxations
  buy meaningful throughput.

### 4. Strong non-cheating AI

- Run the shipped BHS AI as a baseline and behavior-cloning source, including all production
  stages and tactical/diplomatic subsystems.
- Build increasingly strong scripted, search, model-based, and learned players on the same
  observation/action boundary humans and RL policies use.
- Forbid privileged fog state, hidden enemy data, free resources, or difficulty multipliers
  in the DoN player. Any handicap is an explicit evaluation configuration.
- Evaluate across maps, nations, ages, game modes, seeds, adversaries, and long-horizon
  stability—not only an Ancient-age opening.
- Expand the live retail controller into a supervised computer player so policies can act as
  the human slot through retail’s own command APIs.

### 5. Playable edition and frontend

- Turn the integration client into a complete game flow: setup/lobby, map/nation/team choice,
  control groups, command feedback, build/train/tech UI, diplomacy, objectives, minimap,
  replay/save/load, accessibility, settings, and endgame.
- Preserve the high-density spectator/cluster path for RL and debugging.
- Make touch, mouse, keyboard, narrow, and desktop layouts first-class.
- Keep every fallback and missing simulation behavior visible; never make UI polish launder
  model drift into a fidelity claim.
- Add independently licensed art/audio/typography and an enjoyable improved-edition identity.

### 6. BHS and mods

- Execute shipped scripts and all reachable builtins with retail compiler/VM semantics.
- Preserve strict/fidelity policy for retail quirks; unsupported semantics must poison code
  or fail execution rather than produce plausible values.
- Match shipped/local/Workshop discovery, activation, overlay, forbidden-file, and `info.xml`
  behavior using real evidence.
- Provide clear `scan`, `check`, `explain`, and conflict diagnostics for players and modders.

### 7. Live tools

- Keep RoNtoy useful as a low-overhead, fail-closed human coach.
- Keep retail-control main-thread-only, target-hash/call-site gated, generation-isolated, and
  byte-restoring on every exit.
- Extend fog-safe observations and bounded action batches into a supervised policy loop.
- Capture stable trajectories and state deltas that can become retail-vs-DoN differential
  fixtures without redistributing proprietary content.

## Fidelity acceptance for a mechanic

A mechanic is end-to-end only when all applicable layers exist:

- provenance and fidelity tier;
- faithful state/layout/container semantics;
- execution from the real command/tick path;
- captured or retail-differential expectations;
- RNG and checksum consequences;
- replay-channel impact or an explicit reason it is outside replay state;
- workspace and product gates.

Anything less can still be valuable, but call it “derived,” “ported,” “isolated,”
“research-only,” or “integration-complete” at the correct boundary.

## Wave and landing protocol

Wide waves are encouraged when their ownership is clear:

1. Give every lane an exclusive file set or an explicit shared-API owner.
2. Require a return state: landed, research-only, partial, or no artifact.
3. Run the relevant umbrella build after shared state/API changes.
4. Commit named files promptly in coherent tranches; never stage the shared tree wholesale.
5. Push `dev`; never create or switch branches/worktrees.
6. Refresh generated records and this blocker board when evidence materially changes.
7. Reuse finished lanes on a different product plane so breadth does not decay over time.

## Definition of project completion

The project is complete only when current evidence supports every part of the north star:

- a user can install, configure, play, save, replay, mod, and finish a polished independent
  DoN game without retail running;
- Fidelity mode survives substantive retail replay/differential gates over complete reachable
  systems;
- Improved mode is explicitly governed and materially better;
- the RL environment exposes complete dynamics at measured high throughput;
- strong non-cheating agents are reproducibly evaluated across the game;
- RoNtoy/live-control, BHS/mod compatibility, frontend, docs, licensing, and release tooling
  are tested and distributable.

Until then, keep this goal active and keep moving on every plane.
