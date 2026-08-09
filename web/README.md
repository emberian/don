# web — browser spectator for one simulation or a cluster of thousands

Full write-up, architecture and measurements: **`docs/tracks/web-spectator.md`**
(the previous round, which built the data path on placeholder mechanics, is
`docs/tracks/web-frontend.md`).

```sh
node web/tools/pack-gamedata.mjs      # schema/live/* -> public/data/gamedata.bin (gitignored)
node web/tools/gen-wire.mjs           # schema/command-wire.json -> the JS + Rust command codec
web/build.sh                          # cargo -> wasm32 -> public/wasm/don_web.wasm  (~62 KB)
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
It exposes every rejected command in the HUD and labels itself an integration build because
map generation, nation/builder eligibility, movement/collision, acquisition, construction,
fog/LOS, and parts of the economy are not yet retail-complete. The recovered primitives are
real, but their present composition is not called Fidelity mode.

## Playing

Left click selects — that emits a real `GroupCommand` (`0x00`): `num`, `who`, then `num`
two-byte object indices. Right click emits `MoveToCommand` (`0x07`, 22 bytes); shift +
right click emits `AttackCommand` (`0x04`, 17 bytes); the halt button emits `HaltCommand`
(`0x0c`, 1 byte). The bytes are laid out by `wire.gen.js` at the offsets
`schema/command-wire.json` gives, posted to whichever worker owns that world, decoded in
Rust by `wire_gen.rs`, and drained at a tick boundary in arrival order.

A selection is capped at **255** objects, because `GroupCommand.num` is an `unsigned char`.
That is the packet's limit, not the page's, and the page says so when it truncates.

## Fidelity

Nothing on this page is a fidelity claim about Rise of Nations as a *game*. The damage
arithmetic is Tier B (differentially tested against retail by the combat lane, in
`don-sim`); the data is a live read; everything joining them is a placeholder. See the
track report for the line-by-line split.
