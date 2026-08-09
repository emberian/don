# Descent of Nations

**An independent, high-performance reimplementation of _Rise of Nations_—built to become
both a better game and the reference environment for training and evaluating serious RTS
agents.**

Descent of Nations (DoN) has three inseparable goals:

1. **A complete game.** An enjoyable, moddable, standalone edition that can eventually give
   an expert RoN player a real, non-cheating challenge.
2. **A fidelity reference.** A deterministic reconstruction of retail simulation behavior,
   measured against the shipped executable, live process, and replay checksums.
3. **An improved edition.** Explicitly governed fixes and enhancements—better AI, performance,
   ergonomics, and balance experiments—without quietly contaminating the fidelity baseline.

This is already a substantial engine and toolchain, not a design document. It is also not
yet a finished standalone release. The playable browser client is currently an integration
build, and the product gate deliberately refuses a release claim while known whole-game
systems remain incomplete.

## What exists today

| Surface | Current state |
|---|---|
| **Simulation** | Deterministic Rust state, retail-ordered tick structure, command/order plumbing, and broad ports of economy, production, combat, movement, A*, collision, groups, cities, borders/fog, air, naval, walls, items, scoring, RNG, and checksums. Runtime coverage is substantial but not yet the complete retail game. |
| **Retail differential oracle** | 22 registered i686 cases, 18,622,556 trials in the current record, with zero mismatches, skips, crashes, or errors. The harness runs shipped machine code and mutation-tests its comparisons. |
| **Replay validation** | 61 `.rcx` recordings, 585,152 turns, and 488,557 structurally valid checksum packets. Exact Game/GameInfo/player/team/map/seed setup now drives a non-empty `world` checksum on all 222,938 retail comparisons; the missing retail terrain/start generator is exposed as first-turn divergence, not empty agreement. |
| **RL environment** | Batched Rust environment with Gymnasium, VectorEnv, and PettingZoo adapters; derived 82-opcode action taxonomy, parameter-level masks, zero-copy observations, native masked sampling, and deterministic parallel stepping. Some accepted verbs still expose absent dynamics instead of pretending to work. |
| **AI** | Shipped BHS economic/runtime foundations plus a deterministic, observation-driven arena AI. The intended player is strong and non-cheating; remaining arena world-model substitutions are registered product blockers, not hidden advantages. |
| **Playable frontend** | WebGPU/WebGL2/Canvas2D browser client, real command packets, native/wasm digest checks, cluster visualization, replay tools, and an integration game surface. It is fast and interactive, but not yet presented as Fidelity mode. |
| **Live retail tools** | RoNtoy live economy coach and overlay; read-only state capture; and a reversible main-thread command ingress that has paused, moved, and trajectory-recorded a real retail unit without restarting the match. |
| **BHS and mods** | Recovered source compiler, bytecode decoder, stack VM, builtin registry, fail-closed host boundary, aggregate execution, content overlays, mod discovery/path compatibility, and fidelity/strict behavior gates. Unsupported behavior errors rather than becoming a convenient stub. |

The current release gate is executable:

```sh
tools/product-readiness.sh
```

It is expected to exit nonzero today. That is a feature: the output names the remaining
reachable product drift instead of letting a prototype call itself the game.

## Fidelity mode and improved mode

DoN does not equate “retail did it” with “the new game should keep doing it.” The deviations
registry distinguishes:

- **Fidelity mode:** reproduce measured retail behavior, including bugs when they affect the
  simulation.
- **Improved mode:** enable reviewed fixes through real execution seams, with every change
  named and reversible.
- **Drift:** an incomplete or simplified reachable model. Drift is never a mode and blocks
  the applicable product surface.

The policy and current register live in
[`docs/tracks/dual-mode.md`](docs/tracks/dual-mode.md) and
[`docs/tracks/deviations.md`](docs/tracks/deviations.md).

## Why fidelity is measurable

RoN is a deterministic lockstep RTS: peers exchange orders, simulate locally, and compare
state. Retail multiplayer recordings preserve per-turn checksum packets across fifteen
simulation channels. DoN uses that architecture as an integration oracle.

At smaller boundaries, the 32-bit regression harness maps the supported retail executable
and calls individual shipped functions with controlled inputs. Every Tier-B result records
its sample count and domain. Decompiled C supplies hypotheses; the executable settles
behavior. The shipped private PDB supplies names and layouts, not semantics.

This work has already overturned plausible folklore: damage is a 31-stage integer chain
with armor in the middle; general movement is integer; border simulation is a cell-radius
model rather than a spline; the normal tick is 67 ms; and the shipped AI receives large
difficulty-dependent economic bonuses.

## Try the repository

The Rust workspace:

```sh
cargo test --workspace --all-targets
```

The evidence and readiness gates:

```sh
tools/oracle-regress.sh --status   # inspect the last retail differential record
tools/replay-validate.sh           # regenerate the replay scoreboard
tools/product-readiness.sh         # fail closed on reachable product drift
```

The playable browser integration build requires data extracted from a legally owned copy of
the game:

```sh
node web/tools/pack-gamedata.mjs
node web/tools/pack-playdata.mjs
web/build.sh
node web/serve.mjs 8787
# open http://127.0.0.1:8787/play.html
```

The RL surface:

```sh
python3 crates/don-env/gen/gen_spec.py
cargo test -p don-env
bash python/build.sh
PYTHONPATH=python python3 python/smoke_test.py --envs 256
```

See [`web/README.md`](web/README.md),
[`docs/tracks/rl-env.md`](docs/tracks/rl-env.md), and
[`README-LLM.md`](README-LLM.md) for operational detail.

## Repository map

- `crates/don-sim` — deterministic simulation, systems, state walkers, and fidelity policy.
- `crates/don-replay` — replay decoding, command application, and checksum validation.
- `crates/don-env` — batched RL API, masks, observations, rewards, and Python bindings.
- `crates/don-ai` — shipped-AI compatibility and the playable non-cheating arena player.
- `crates/don-bhs`, `crates/don-bhs-cc` — Big Huge Script VM and compiler.
- `crates/don-content` — shipped/local/Workshop content and overlay semantics.
- `crates/don-net` — lockstep transport and headless multiplayer work.
- `crates/don-gpu` — measured GPU/batch simulation experiments.
- `web` — playable client, spectator/cluster renderer, and replay UI.
- `tools/rontoy-*` — live retail coach, host, dashboard, and overlay.
- `tools/retail-control` — reversible, generation-safe retail observation and command ingress.
- `crates/oracle`, `re`, `schema` — executable oracle, reverse-engineering evidence, and generated records.

The live execution board is [`GOAL.md`](GOAL.md); the durable methodology is
[`docs/CHARTER.md`](docs/CHARTER.md).

## Game data and project independence

This repository does not distribute Microsoft/Big Huge Games binaries, art, audio, or rule
data. `ron-bin/`, `ron-data/`, live captures, and generated proprietary packs are ignored.
Development and local play require a legally obtained copy of _Rise of Nations: Extended
Edition_; a distributable DoN release must use an extractor and/or independently licensed
assets.

Descent of Nations is an independent project and is not affiliated with or endorsed by
Microsoft or Big Huge Games. “Rise of Nations” is used only to identify compatibility and
the behavior being studied.

## License

DoN source code is free software under the
[GNU General Public License, version 3 or later](LICENSE), except where a file or
subdirectory carries a different compatible notice. See `LICENSE`. Retail game content is
not covered by this license and is not part of the repository.
