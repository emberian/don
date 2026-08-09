# README-LLM — orientation for agents

Read this file completely before changing the repository. Then read
[`docs/CHARTER.md`](docs/CHARTER.md) and the documentation for the subsystem you touch.
[`README.md`](README.md) is the human-facing project overview;
[`GOAL.md`](GOAL.md) is the execution board, but its measured snapshot can lag the current
tree and generated records.

## Mission

Descent of Nations (DoN) is becoming three things at once:

1. **An independent, complete, enjoyable successor to _Rise of Nations_.** It must eventually
   be good enough to give an expert human a serious, non-cheating game—not merely expose a
   mechanics library.
2. **A defensible fidelity implementation.** Retail behavior is reconstructed from the
   supported executable, shipped data, live process, and replay state checksums.
3. **A high-throughput RL and evaluation platform.** Batched simulation, full parameter-level
   action masks, deterministic parallelism, self-play, league evaluation, and an eventual
   RoNEval suite are first-class product requirements.

Improved mode may exceed retail. Fidelity mode stays as its reference. Do not let work on one
of these collapse attention onto it at the expense of the other two.

## Repository workflow: one permanent branch

**Work directly on `dev`, the permanent GitHub default branch. Never create, switch, rename,
or delete branches, and never create a Git worktree.** Do not create `codex/*`, recovery,
feature, or agent branches. If the checkout is not already on `dev`, stop and tell Ember;
do not “repair” it by switching.

The working tree is shared by every active agent:

- preserve unrelated modifications;
- never use `git stash`, destructive reset, checkout-based restoration, or broad cleanup;
- stage named files or exact hunks—never `git add -A` while lanes are active;
- agents report their file sets and validation; the root/orchestrator commits coherent
  tranches and pushes `dev`;
- temporary compile failures can be another lane’s in-flight API migration. Identify the
  owner before editing around it.

History is a work log, not a reliquary: make useful thematic commits promptly. Do not hold a
large dirty tree for an elaborate “perfect” strategy.

## The methodology law

**Ground truth is the supported binary, shipped game data, or measured live-process behavior.
Community documentation is a source of hypotheses only.**

Before introducing a constant, branch, formula, state transition, or layout, identify its
source:

- an executable address and instruction evidence;
- a PDB name/layout plus behavioral evidence for semantics;
- a path and line in extracted shipped data; or
- a recorded retail observation/oracle sample.

If the real source is “the wiki says so,” “RoN usually works this way,” or “this model is
reasonable,” stop and derive it. Plausible substitutions are more dangerous than visible
missing behavior because they compile, train policies, and silently become the new game.

### Fidelity tiers

- **Tier A — proven equivalent:** whole-domain SMT/bitvector equivalence to lifted semantics;
  state the model boundary.
- **Tier B — differentially tested:** Rust and actual shipped machine code agree over a named
  generated domain. Always report sample count, distribution, and exclusions. This is testing,
  not formal verification.
- **Tier C — behaviorally faithful:** matches observed behavior with a stated evidence and
  divergence boundary.

Never call an ordinary test, decompilation, PDB lookup, or finite differential run “verified”
or “proven.” Capture expectations from retail; do not hand-calculate fixtures and then test
the transcription against itself. Mutation-test differential harnesses so a one-bit error is
known to fail.

### Fidelity, improved behavior, and drift

The authoritative policy is `crates/don-sim/src/deviations.rs`; prose lives in
[`docs/tracks/dual-mode.md`](docs/tracks/dual-mode.md) and
[`docs/tracks/deviations.md`](docs/tracks/deviations.md).

- A **Fix** is an explicit, reversible improved-mode behavior with a real execution seam.
- **Drift** is a reachable approximation or incomplete model. It is not a feature or a mode;
  it blocks readiness for every product surface it can reach.
- **ResearchOnly** code may support bounded experiments but is not evidence about the game,
  replay fidelity, or a releasable RL surface.

Do not deliberately simplify fidelity mode. For DoN proper, meet retail’s sophistication and
then exceed it deliberately in improved mode.

## Current measured snapshot

Snapshot date: 2026-08-08. Prefer the generated records and commands below if they disagree
with prose.

| Gate/surface | Current authoritative state |
|---|---|
| Retail differential suite | `schema/oracle-regression.json`: 16/16 cases, 17,223,055 trials, zero mismatches/skips/crashes/errors after the full-state `Guy::turn_towards` case. |
| Replay corpus | `schema/replay-validation.json`: 61 files, 585,152 turns, 488,557 structurally valid checksum packets; dynamic world reconstruction remains unmodelled, so current checksum agreement is not whole-game fidelity. |
| Improved-mode seams | The seven default fixes are wired through real economy/production/border/BHS paths and covered by the fidelity gate. |
| Product readiness | `tools/product-readiness.sh` intentionally refuses with two registered blockers: `env-patrol-execution` and `arena-model-simplifications`. Do not turn them into waivers. |
| RL surface | Builds, imports, resets, steps, masks, and exposes zero-copy observations through native/Gymnasium/PettingZoo APIs. The last published partial-dynamics report reaches 98,505 env-steps/s at 1,024 worlds; remeasure before quoting it as current. |
| Playable browser | Native/wasm integration smoke, real command codecs, WebGPU/WebGL2/Canvas2D rendering, and deterministic digest paths exist. The UI labels itself an integration build because whole-game model gaps remain. |
| Live retail control | Main-thread pause/move commands and frame-by-frame movement trajectories are live-proven. Hook generations upgrade in place without restarting the game and restore the exact overwritten bytes on STOP. |
| RoNtoy | Read-only live capture, fail-closed host, browser coach, recording/replay, and native overlay are working. |
| BHS/content | Compiler, decoder, VM, aggregate execution, builtin registry, and content/mod resolution exist; unsupported semantics fail closed. |

Passing Rust tests does not upgrade the fidelity tier. A port can be compiled, unit-tested,
called, checksummed, and retail-matched at five different levels; report which one is true.

## The product data flow

The intended architecture is one shared game, not separate demo implementations:

```text
retail command schema / human click / policy action
                    ↓
          command decoder and queue semantics
                    ↓
      retail-order dispatcher + deterministic systems
                    ↓
          one authoritative world/tick implementation
          ↙              ↓                 ↘
 replay checksum     batched RL          playable client
 validation          observations        and strong AI
```

The replay harness, RL environment, arena AI, and browser must converge on that command and
world path. A straight-line environment movement model, a frontend-only economy, or an arena
watchdog can be useful research scaffolding, but it cannot become the product path.

Retail lockstep supplies the whole-system reference: recorded command packages plus fifteen
simulation-state checksum channels. The checksum walker shares the engine’s `DataWalk`
interface with save/load state. Dynamic replay agreement requires deterministic reconstruction
of the initial map, players, objects, RNG streams, and container histories; `.rcx` does not
contain a complete initial save image.

## Repository map

### Authoritative product crates

- `crates/don-sim` — state, retail tick, mechanics, orders, walkers, RNG, deviations policy.
- `crates/don-replay` — `.rcx` parser, command streams, sim bridge, checksum scoreboard.
- `crates/don-env` — batched RL world, action masks, observations/reward, Python API.
- `crates/don-ai` — BHS/economic compatibility and the observation-driven playable arena AI.
- `crates/don-rules` — shipped rules parsing and generated field offsets.
- `crates/don-content` — shipped/local/Workshop content resolution and overlays.
- `crates/don-bhs`, `crates/don-bhs-cc` — BHS VM and source compiler.
- `crates/don-net` — lockstep session/transport and headless retail-network work.
- `crates/don-gpu` — measured batch/GPU kernels with CPU parity gates.

### User-facing surfaces and instruments

- `web` — playable integration client, one-world spectator, cluster renderer, replay UI.
- `tools/rontoy-host`, `tools/rontoy-overlay`, `web/public/rontoy.html` — live coach.
- `tools/retail-control` — reversible retail command ingress and trajectory recorder.
- `crates/donscan` — Windows live-process reader; excluded from the arm64 workspace.
- `crates/netsys-shim` — i686 retail-loaded network shim; excluded from the workspace.
- `crates/oracle` — i686 executable oracle; excluded from the arm64 workspace.

### Evidence

- `schema/` — generated command/layout tables, oracle/replay records, PDB extracts.
- `docs/provenance-ledger.md` — mechanic-by-mechanic provenance and fidelity tier.
- `docs/mechanics/` and `docs/derivation/` — recovered behavior and audit reports.
- `re/decomp-all/` — bulk decompiler corpus; use for control-flow orientation.
- `ron-bin/`, `ron-data/`, `schema/live/` — local proprietary inputs/captures, gitignored.

## Normal gates

Run the narrow gate while iterating and the broad gate before handing off a shared-structure
change.

```sh
cargo check --workspace
cargo test --workspace --all-targets
cargo test -p don-sim --lib
cargo test -p don-env
```

Evidence and release gates:

```sh
tools/oracle-regress.sh --status   # inspect current record without running hbox
tools/oracle-regress.sh            # sync/build/run every registered i686 case
tools/replay-validate.sh           # regenerate corpus scoreboard
tools/product-readiness.sh         # expected to refuse while product drift remains
```

Web and RL commands:

```sh
node web/tools/pack-gamedata.mjs
node web/tools/pack-playdata.mjs
web/build.sh
node web/serve.mjs 8787
node web/tools/play-smoke.mjs --json web/play-results.json

python3 crates/don-env/gen/gen_spec.py
bash python/build.sh
PYTHONPATH=python python3 python/smoke_test.py --envs 256
```

Generated benchmark result files, live captures, compiled DLLs, and extracted game data are
not source. Respect existing ignore rules; commit a stable schema artifact only when its
provenance and reproducibility are documented.

## Machine topology

| Machine | Role |
|---|---|
| This Mac (arm64, 12 cores, 96 GB) | Rust workspace, orchestration, Ghidra/Capstone, web/browser work. It cannot execute 32-bit x86. |
| `hbox` (x86_64 Linux) | Maps and executes the 32-bit retail code for differential cases. It is a co-tenant: use `nice -n 15 taskset -c 0-3`; never install packages or launch unbounded builds. |
| Parallels VM `Windows 11` (ARM64 Windows, x86 emulation) | Supported retail game, live memory, RoNtoy, reversible command ingress. |

### Reverse-engineering tools

The exact supported executable is PE32/i386, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`, image base
`0x00400000`. Runtime addresses are preferred VA plus the ASLR delta.

The shipped private PDB is `ron-bin/sbl/rise.pdb` and matches this executable. It contains
names, signatures, code sizes, and type layouts. It does **not** establish behavior.

```sh
python3 tools/pdb/lookup.py 644130 a1d110
python3 tools/pdb/lookup.py --name 'PathFinder::'
cd ron-bin && uv run --quiet --with pefile python ../tools/pdb/callers.py 57fa60
cd ron-bin && uv run --quiet --with pefile python ../tools/pdb/xref.py --str flank_bonus
```

Prefer `re/decomp-all/<EA>.c` before opening Ghidra. Decompiled C is a control-flow map; use
Capstone for instruction-order/rounding questions and the oracle/live game for values. The
Ghidra project has a single-writer lock; copy `re/ghidra` to a lane-specific temporary path
if another process may be using it.

Run the oracle through `tools/oracle-regress.sh`, not legacy inline model commands. A case
must call the code shipped by DoN and the mapped retail function; a copied Rust-like model in
the harness proves nothing about the crate.

### Live process safety

`prlctl exec` runs as SYSTEM; user paths must be explicit. Rebase every static address. Use
bounded native readers rather than large PowerShell memory loops. Hash the target before any
injection.

Retail commands must execute on the retail main thread. `tools/retail-control` detours the
sole `TurnControl::do_frame` call only after exact byte/image gates; STOP restores those five
bytes and byte-checks the result. Loaded DLL images are immutable generations—upgrade to a
new basename/directory rather than overwriting or reinjecting one path. Never call retail
simulation APIs from an injector worker thread.

## Established facts that prevent recurring mistakes

- The supported executable is an unpacked 2024 MSVC rebuild of the original code; RTTI and
  the matching private PDB are present.
- Simulation float math is predominantly SSE binary32. Ghidra can reorder 32-bit MSVC float
  and integer expressions; instruction order matters.
- Normal simulation cadence is 67 ms. A lockstep network turn can contain multiple frames.
- The main LCG is `s = s*1664525 + 1013904223`; there are multiple RNG streams. Pathfinder
  failure paths share the main simulation stream—do not invent a private path RNG.
- Object coordinates in retail object state are XOR-obfuscated with `0x00063637`; GuyData
  coordinates are not. Order coordinates are raw.
- `Array<T>` capacity and growth metadata can be walked/checksummed. Logical element equality
  alone is insufficient for fidelity.
- `Objects::process_all` rotates owner order by frame. A fixed owner loop diverges.
- The 493×493 signed balance matrix begins at preferred VA `0x00C12BF4`.
- General pathfinding and caravan-road pathfinding are different functions. Results derived
  from `astar_caravan_road` do not automatically apply to `astar_path`.
- `Constants::log_data` is a logger, not the rules loader. `Constants::init` is the loader.
- BHS runtime state is sim-critical and participates in lockstep checksum walking; the VM and
  builtin boundary are not optional compatibility extras.

The maintained versions and provenance live in
[`docs/binary-ground-truth.md`](docs/binary-ground-truth.md) and
[`docs/provenance-ledger.md`](docs/provenance-ledger.md). Update those rather than growing a
second folklore list here.

## Broad execution priorities

Maintain parallel pressure across the full destination:

1. **Independent game:** deterministic map/start generation, full command/order/tick systems,
   complete ages/economy/combat/diplomacy/naval/air/victory, save/load, content, audio/art/UI,
   and a polished playable loop.
2. **Fidelity/replay:** reconstruct non-empty initial replay worlds, walk real state in all
   channels, and reduce first divergence with retail differential cases.
3. **RL/RoNEval:** replace every accepted-no-effect verb with real dynamics, retain exact
   parameter masks, benchmark reset/step/observe/mask/reward, and build deterministic leagues
   and scenario/evaluation packs.
4. **AI:** reproduce shipped BHS behavior as a baseline, then build stronger observation-only
   scripted/search/learned players with no privileged state or economic bonuses.
5. **Playable/web:** make the independent edition pleasant to control and understand while
   retaining the high-density cluster/replay/spectator paths.
6. **Live retail:** expand safe observation/command trajectories into a supervised computer
   player and differential laboratory; keep RoNtoy useful to a human meanwhile.
7. **BHS/mod ecosystem:** run shipped scripts and real Workshop/local mods faithfully, with
   explicit strict/improved policy for retail quirks.

Do not replace this breadth with endless micro-validation, but do not call a wide prototype
complete because its narrow tests are green. Each wave ends with owned files, exact gates,
coherent commits, and a refreshed blocker list.

## Licensing and proprietary inputs

DoN source is GPL-3.0-or-later unless a file/subdirectory says otherwise. Preserve notices
and add SPDX identifiers to new source where practical.

Microsoft/Big Huge Games binaries, data, art, audio, PDBs, live captures, and derived packs
are not covered by DoN’s license and must not be committed or redistributed. A public release
must use extraction from a legally owned game and/or independently licensed replacement
assets. Never “clean up” ignore rules in a way that stages `ron-bin/`, `ron-data/`,
`schema/live/`, or compiled injected DLLs.
