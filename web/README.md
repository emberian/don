# web — browser spectator for one simulation or a cluster of thousands

Full write-up, architecture and measurements: **`docs/tracks/web-spectator.md`**
(the previous round, which built the data path on placeholder mechanics, is
`docs/tracks/web-frontend.md`).

```sh
node web/tools/pack-gamedata.mjs      # schema/live/* -> public/data/gamedata.bin (gitignored)
node web/tools/gen-wire.mjs           # schema/command-wire.json -> the JS + Rust command codec
node web/tools/gen-readiness.mjs      # replay scoreboard -> compact browser evidence
web/build.sh                          # generated-source checks + cargo -> public/wasm/don_web.wasm
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
node web/tools/pack-gamedata.mjs
node web/tools/pack-playdata.mjs
node web/tools/gen-wire.mjs --check
node web/tools/gen-readiness.mjs --check
web/build.sh
node web/tools/check-play-wasm.mjs
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
the authoritative Sim. The team-layout control is the bounded exception: manual start recreates
the requested seed and sends the complete roster, explicit team bytes, team style, and local slot
through one frame-zero `Sim::start_manual_player_setup` transaction. That owner applies the
recovered deterministic/non-ranked `Game::init_teams` body, then the full sequential eight-row
diplomacy/treaty/shared-vision loop from `Leader::init`, before activating any leader. It retains
the exact `PlayerSetup` image, option/semaphore/prerequisite facts, rows, and ordered receipts in
the Sim. The browser has no technology prerequisite host yet, so it supplies an explicit zero
`has_preq(0x2B0)` mask. Random-team byte 5,
ranked setup, malformed/inactive team bytes, repeated start, and nonzero frames all refuse without
mutation. JavaScript queries the resulting roster and teams back; it retains no parallel copy.
Shared-session URLs and command-journal v3 baselines carry the bounded team preset and reconstruct
it through the same transaction. V1 and v2 journals remain importable as inactive/own-slot-team
baselines. `setup`, `active`, and `ended` remain live core projections.

This does not make later diplomacy or victory setup complete. ATTACK ingress refuses self, allied,
and other non-hostile targets before installing an order; the mutable `Leader::set_diplo`
transaction stays red. Raw `game_set_team` and `game_set_victory_mode` exports stay forbidden.
DoNSave v11 reconstructs the exact PlayerSetup transaction at frame zero, then restores the
authoritative mutable Leaders/Match lifecycle and derives and verifies the step-8 views. Browser
save/load therefore admits supported active matches as well as inactive setup state; every still
unsupported subsystem continues to fail closed before serialization.

The authoritative-roster ABI tranche passed six focused native tests in both independent remote
profiles on 2026-08-09: hbox
`web-authoritative-roster-20260809T214825Z-33897-19722-18c26aa332f9` and persvati release
`web-authoritative-roster-release-20260809T214825Z-33893-25343-18c26aa332f9`. Both exited 0.
The JavaScript modules and smoke source also pass `node --check`. Root convergence then rebuilt
and optimized the Wasm, verified the original 73 required exports with both unsupported setup setters absent,
and passed the full Chrome/WebGPU smoke. The renderer read back 42,496 non-black pixels, real
mouse input installed `MOVE_TO`, URL reload reconstructed the exact authoritative roster, and
native/Wasm digests agreed at 600 frames for both inactive setup
(`e526f20feb32cb49`) and roster `0,1,2,3` (`b68aa66f8a4703d0`).

The subsequent deterministic PlayerSetup tranche expanded the contract to 76 required exports
while keeping both raw setup setters forbidden. The active-team diplomacy tranche then rebuilt the
artifact at 771,141 bytes. Nine native ABI tests, five Sim owner tests, five bounded Leader-init
tests, and the complete Chrome/WebGPU smoke passed. FFA and 2v2 setup reconstruct through
URL/journal state owned by Sim; the smoke reads P0/P2 as allies and proves a teammate ATTACK packet
increments the non-hostile gap without installing an order. The inactive 600-frame digest is now
`c315fabe7d19cb90`; the now diplomacy- and sparse-identity-visible active-roster digest is
`61e14d1ab48d964f`. These replace the prior pair because sparse Object-band owner activity and
marks are now deliberately inside the canonical World digest.

The canonical Group→Move candidate was rebuilt twice from exact archived commit `41828a6` on
2026-08-12. Both optimized modules are 1,190,735 bytes at SHA-256
`def395ffc247d3a4a561c1d110b22b9f7ef8988a2637f9db79fde1d9c5339d40`; all 81 required exports
are present and both raw setup setters remain absent. Two independent native Wasm instances
produced equal package receipts and frame/digest/RNG after-images, moved both initially selected
land Units, saved exact DoNSave v16 roots carrying all 14 sections through `FARMS`, loaded at frame
32 without divergence, accepted equal fresh receipts, and moved again to equal frame-64 state.
Chrome evidence remains unavailable on this host and is not claimed by this candidate record.

`web/build.sh` refuses stale command-wire or replay-readiness generated sources, then statically
checks the fresh Wasm export table both before and after optional optimization. The same three
source/artefact preflights run before `play-smoke.mjs` opens Chrome. The export contract requires
the Sim-owned activation and active-roster query while forbidding `game_set_team` and
`game_set_victory_mode`; a source advance paired with an old checked-in Wasm therefore fails with a
specific stale-ABI error rather than a late panel exception.

The script performs the Cargo build from a fresh, locked `/tmp/don-web-canonical-source-v1`
source root. This stabilizes path-dependency package identities as well as embedded source paths,
so the same source archive produces byte-identical Wasm regardless of its extraction directory.
An existing lock or source root is treated as a concurrent/stale build and refused, never reused.

Cross-target digest checks take an explicit roster, so they never inherit browser session state.
From the repository root, compare both supported baselines with:

```sh
cargo run --manifest-path web/wasm/Cargo.toml --release --bin playcheck -- \
  digest web/public/data/gamedata.bin web/public/data/playdata.bin c0ffee 600 -
cargo run --manifest-path web/wasm/Cargo.toml --release --bin playcheck -- \
  digest web/public/data/gamedata.bin web/public/data/playdata.bin c0ffee 600 0,1,2,3
```

Its readiness panel has three independent inputs: the runtime identifies the Sim-backed
browser adapter (not `don_ai::arena::World`), the playable blocker list is read from
`don_sim::deviations` compiled into the Wasm module, and replay evidence is generated from
the authoritative validation JSON. Packet counters show submitted, tick-drained, applied,
pending, and fail-closed commands, so UI activity is not mistaken for engine activity.

The Save and Load controls exchange the bounded deterministic `DoNSave` byte image owned by
`don_sim::systems::save_load`. Decode is atomic: malformed input leaves the current session
unchanged. Supported post-step worlds and active matches roundtrip and resume: load derives and
verifies the exact step-8 leader views from saved canonical inputs instead of serializing a second
copy, while DoNSave v11 owns the mutable Leaders/Match lifecycle. Unsupported subsystems and
adapter queue shapes still fail closed rather than being silently dropped. Manual PlayerSetup
stores its small request/options/semaphore input image, reruns the sequential transaction at frame
zero on load, and validates all derived leader/world/fog projections before installing current
match state. After load, the exact DoNSave bytes remain an in-memory command-journal seek anchor;
the page refuses to export that journal as a frame-zero restart document because doing so would
silently omit its native-save baseline.

The new-game panel also has a loopback-only, two-seat lobby handoff. `web/serve.mjs` exposes a
bounded same-origin JSON API and invokes the configured native `service-match-peer`; it never
returns seed, epoch, or roster until the independent host and joining processes agree on
Crossplay StartGame and the `don-net` MatchStart. Each browser then reconstructs the exact
two-player Sim setup from that handoff. The opt-in native `--relay` mode keeps both `ServiceMatch`
owners alive. Each browser independently generates an exact 27-byte owner-local `GroupCommand`
plus `MoveToCommand` package. The native peer validates that exact command boundary with
`don-net::decode_commands`, and the server exposes it only when both peers return the identical
ordered `TurnPackage` set. Both tabs independently decode and re-encode the bytes, apply P0 then
P1 through the receipt-bearing Wasm package ABI, step exactly once, and acknowledge
frame/digest/RNG. The next stamp opens only when those acknowledgements are equal. The tabs remain
pause-locked throughout; every other command and free-running multiplayer remain refused rather
than being presented as synchronized.

Build the native seam and run the Web server with its explicit path:

```sh
cargo build --manifest-path crates/don-crossplay/Cargo.toml \
  --features local-match --bin service-match-peer
DON_SERVICE_MATCH_PEER="$PWD/crates/don-crossplay/target/debug/service-match-peer" \
  node web/serve.mjs &
node web/tools/play-smoke.mjs --local-match
```

The server binds only `127.0.0.1`, admits at most 16 in-memory lobbies, caps JSON bodies and child
output, bounds relay lifetime and barrier timeouts, uses unguessable per-seat tokens, and fails
closed when the configured executable is absent, native package sets disagree, or browser state
acknowledgements differ. `node --test web/tools/local-match.test.mjs` mutation-tests those admission
rules with a parser fixture; the `--local-match` Chrome smoke uses the real compiled Rust peer and
two actual browser tabs.

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
