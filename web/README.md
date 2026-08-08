# web — browser spectator for one simulation or a cluster of thousands

Full write-up, architecture rationale and measurements: **`docs/tracks/web-frontend.md`**.

```sh
web/build.sh                 # cargo -> wasm32 -> public/wasm/don_web.wasm  (~24 KB)
node web/serve.mjs           # http://127.0.0.1:8787/  with COOP/COEP set
node web/bench.mjs --out results.json     # launches Chrome, drives it over CDP, prints JSON
```

`serve.mjs` is not optional dressing. `SharedArrayBuffer` — and therefore the multi-worker
simulation path — exists only on a **cross-origin isolated** page, which needs
`Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp`
on the document. Serve the directory with anything that does not set those and the page
still runs, but the `sab` path is disabled and says so in the panel.

## What is where

| path | what |
|---|---|
| `wasm/` | `don-web`: raw C-ABI wasm shim over `don-sim`. No `wasm-bindgen`. Standalone cargo workspace, so the root `cargo test` never builds it. |
| `wasm/src/bin/digest.rs` | native twin of the browser's digest and sim-throughput readouts, for cross-target comparison |
| `public/js/proto.js` | SharedArrayBuffer layout: per-shard control block + triple-buffered column banks |
| `public/js/wasm.js` | module loader and typed-array views over wasm linear memory |
| `public/js/sim.worker.js` | one cluster shard per worker |
| `public/js/render.worker.js` | OffscreenCanvas owner; frame loop; the three data paths; the benchmark |
| `public/js/webgpu.js` | primary backend — SoA columns bound as per-instance vertex buffers |
| `public/js/webgl2.js` | fallback, same architecture via `vertexAttribIPointer` + `vertexAttribDivisor` |
| `public/js/main.js` | DOM, pointer, and the `window.don` automation surface |

No bundler, no `npm install`, no transpile step. The only build artefact is the `.wasm`.

Query parameters: `?backend=webgl2` forces the fallback backend (the choice is made once, at
device creation, so it cannot be switched live).

The click-to-order path emits the engine's **real** command opcodes from
`schema/command-wire.json` — `0x07 MoveToCommand`, `0x0c HaltCommand` — honouring only the
fields the placeholder mechanics can mean anything by (`to_x`, `to_y`). See
`don_web::order_kind` for exactly what is and is not modelled.

## Fidelity

Everything simulated here is `don-sim`'s **placeholder** mechanics plus one more
placeholder of this crate's own (the order-follow pass). Nothing on this page is a fidelity
claim about Rise of Nations. The numbers measure the *data path* — how fast state can get
from a Rust SoA column onto a screen — which is what this lane is for.
