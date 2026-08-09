# web — browser spectator for one simulation or a cluster of thousands

Full write-up, architecture and measurements: **`docs/tracks/web-spectator.md`**
(the previous round, which built the data path on placeholder mechanics, is
`docs/tracks/web-frontend.md`).

```sh
node web/tools/pack-gamedata.mjs      # schema/live/* -> public/data/gamedata.bin (gitignored)
node web/tools/gen-wire.mjs           # schema/command-wire.json -> the JS + Rust command codec
node web/tools/gen-readiness.mjs      # replay scoreboard -> compact browser evidence
web/build.sh                          # cargo -> wasm32 -> public/wasm/don_web.wasm
node web/serve.mjs                    # http://127.0.0.1:8787/  with COOP/COEP set
node web/bench.mjs --out results.json # launches Chrome, drives it over CDP, prints JSON
```

Run the three generators in that order on a fresh checkout, then `serve.mjs`. Without
`gamedata.bin` the page still runs, on a **synthetic** unit table, and says so in a red
badge — it never quietly shows made-up numbers as if they were the game's.

`serve.mjs` is not optional dressing. `SharedArrayBuffer` — and therefore the multi-worker
simulation path — exists only on a **cross-origin isolated** page, which needs
`Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp`
on the document. Serve the directory with anything that does not set those and the page
still runs, but the `sab` path is disabled and says so in the panel.

## What is on screen

Two or more armies of **real Rise of Nations unit types** — real `HITS`, `ATTACK` (×10),
`ARMOR`, `MOVES`, `MAX_RANGE`, `RECHARGE`, `OBJ_MASK` — fighting through
`don_sim::damage`, the derived 31-step `ObjectData::get_damage` chain, with the real
493×493 `Balance::final_balance_table` and the real combat constants read out of the live
`Constants` singleton. Click a unit to see its name and stats.

**What is not real**: target acquisition, movement, the attack angle, terrain, height,
buildings, economy, and every damage predicate except "both objects are alive". Those are
this crate's placeholders and `web/wasm/src/real.rs` says so at the top, in detail. The
arithmetic of a hit is the engine's; who hits whom is ours.

## What is where

| path | what |
|---|---|
| `tools/pack-gamedata.mjs` | packs `schema/live/{live-tables-unit.tsv,balance-real.bin,rules-block-*.txt}` into one blob |
| `tools/gen-wire.mjs` | generates `public/js/wire.gen.js` **and** `wasm/src/wire_gen.rs` from `schema/command-wire.json` |
| `tools/gen-readiness.mjs` | generates the compact replay card from `schema/replay-validation.json`; `web/build.sh` refuses stale evidence |
| `wasm/` | `don-web`: raw C-ABI wasm shim. No `wasm-bindgen`. Standalone cargo workspace, so the root `cargo test` never builds it. |
| `wasm/src/real.rs` | the simulation: SoA world, owner-slot rotation, derived damage. **Read its module docs before believing anything on screen.** |
| `wasm/src/gamedata.rs` | reader for the packed tables, plus the synthetic fallback |
| `wasm/src/bin/digest.rs` | native twin: `digest`, `bench`, and `damage <atk> <def>` |
| `public/js/proto.js` | SharedArrayBuffer layout: per-shard control block + triple-buffered column banks |
| `public/js/wasm.js` | module loader and typed-array views over wasm linear memory |
| `public/js/sim.worker.js` | one cluster shard per worker; owns its worlds' commands and queries |
| `public/js/render.worker.js` | OffscreenCanvas owner; frame loop; the three data paths; the benchmark |
| `public/js/webgpu.js` | primary backend — SoA columns bound as per-instance vertex buffers |
| `public/js/webgl2.js` | fallback, same architecture via `vertexAttribIPointer` + `vertexAttribDivisor` |
| `public/js/main.js` | DOM, pointer, the command path, and the `window.don` automation surface |

No bundler, no `npm install`, no transpile step. The build artefacts are the `.wasm`, the
generated codec, and the gitignored data pack.

Query parameters: `?backend=webgl2` forces the fallback (the choice is made once, at device
creation, so it cannot be switched live).

## Playable integration client

`public/play.html` is the browser control surface for the recovered game-world work. Build
its extra tables and run its native and browser gates with:

```sh
node web/tools/pack-playdata.mjs
web/build.sh
cargo run --manifest-path web/wasm/Cargo.toml --release --bin playcheck -- \
  web/public/data/gamedata.bin web/public/data/playdata.bin
node web/serve.mjs 8787
node web/tools/play-smoke.mjs --json web/play-results.json
```

The playable page requires both packed data files; it will not fall back to synthetic data.
Its authoritative state is `don_sim::tick::Sim`; the browser-facing position, tag, player,
and query arrays are projections rebuilt from that core rather than a second gameplay world.
Move, attack, halt, City unit training/cancellation/completion, bounded Library construction,
sequential age research, frame stepping, digest, RNG, and core save/load use that same state.
Training and research use packed `WHERE`, cost, and job-time records with the Sim's concrete
`BuildData` queue and live production/tech runtime; construction uses Sim `BuildAt` orders and
creates a saved core foundation rather than browser state. Gather, other building placement,
ordinary technologies, live rule setters, fog/LOS, diplomacy mutation, AI, victory mutation, and
objective countdowns remain disabled until their exact core hosts are exposed. Team identity,
effective diplomacy, victory mode/status, and personal/team score are read-only projections from
the authoritative Sim; the setup controls stay disabled because those queries do not assign state.
The manual start action is the narrower exception: it recreates the requested seed at frame zero,
calls `Sim::activate` for the explicit roster, and then queries the active-player mask back from
`Sim::vic_leaders`. The browser retains no parallel roster or match-phase state. Shared-session URLs
and command-journal v2 baselines carry that bounded roster and reconstruct it through the same
transaction. A v1 journal remains importable as an inactive-roster baseline. `setup`, `active`, and
`ended` are projections of the live roster plus the core game-over latch, not UI-only phases.

This does not make team setup mutable. `setup_diplomacy::SetupDiplomacy` still lacks a Sim-owned
`PlayerSetup` image in this adapter, so no team setter is exported; the current team hook remains
explicitly unconfigured/read-only. Victory mode mutation remains absent for the same reason.

The authoritative-roster ABI tranche passed six focused native tests in both independent remote
profiles on 2026-08-09: hbox
`web-authoritative-roster-20260809T214825Z-33897-19722-18c26aa332f9` and persvati release
`web-authoritative-roster-release-20260809T214825Z-33893-25343-18c26aa332f9`. Both exited 0.
The JavaScript modules and smoke source also pass `node --check`; a rebuilt Wasm/browser smoke is
still required before treating generated browser artefacts as current.

Its readiness panel has three independent inputs: the runtime identifies the Sim-backed
browser adapter (not `don_ai::arena::World`), the playable blocker list is read from
`don_sim::deviations` compiled into the Wasm module, and replay evidence is generated from
the authoritative validation JSON. Packet counters show submitted, tick-drained, applied,
pending, and fail-closed commands, so UI activity is not mistaken for engine activity.

The Save and Load controls exchange the bounded deterministic `DoNSave` byte image owned by
`don_sim::systems::save_load`. Decode is atomic: malformed input leaves the current session
unchanged. Supported post-step worlds roundtrip and resume: load derives and verifies the exact
step-8 leader views from saved canonical inputs instead of serializing a second copy. Unsupported
subsystems and adapter queue shapes still fail closed rather than being silently dropped.

## Playing

Left click selects — that emits a real `GroupCommand` (`0x00`): `num`, `who`, then `num`
two-byte object indices. Right click emits `AttackCommand` (`0x04`, 17 bytes) for an enemy
or `MoveToCommand` (`0x07`, 22 bytes) for open ground. The visible command dock exposes the
same supported packet builders for touch users; unavailable actions are disabled instead of
mutating browser-only state. A selected Citizen can emit `BuildCommand` (`0x19`) for the exact
Library cohort, and a selected completed Library can emit `QueueUpCommand` (`0x18`) for the next
age; every other catalog entry remains disabled. The halt button emits `HaltCommand` (`0x0c`, 1 byte). The bytes
are laid out by `wire.gen.js` at the offsets `schema/command-wire.json` gives, decoded by the
WASM adapter in `game_abi.rs`, and applied to `don_sim::Sim` at a tick boundary in arrival order.

A selection is capped at **255** objects, because `GroupCommand.num` is an `unsigned char`.
That is the packet's limit, not the page's, and the page says so when it truncates.

## Fidelity

Nothing on this page is a fidelity claim about Rise of Nations as a *game*. The damage
arithmetic is Tier B (differentially tested against retail by the combat lane, in
`don-sim`); the data is a live read; everything joining them is a placeholder. See the
track report for the line-by-line split.
