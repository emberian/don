// Loader for the raw wasm shim. No wasm-bindgen, no glue module, zero imports.
//
// The module is instantiated with an empty import object because `don_web.wasm` imports
// nothing at all (verify with `WebAssembly.Module.imports`). Everything that crosses the
// boundary is a `u32` or a byte offset into linear memory.
//
// # The detach hazard
//
// A `TypedArray` over `WebAssembly.Memory.buffer` is **detached** if the wasm heap grows,
// and every later read from it throws or returns zeros. The shim allocates all per-frame
// storage inside `sim_create`, so a running shard never grows — but "never" here is a
// property of the Rust code, not of the platform, and it is exactly the kind of assumption
// that rots. So every view goes through `#view`, which compares the current buffer object
// against the one the cache was built from and rebuilds on mismatch. The check is one
// identity comparison per accessor call, not per element.

export class WasmSim {
  /** @type {WebAssembly.Instance} */ #inst;
  /** @type {WebAssembly.Memory} */ #mem;
  /** Buffer identity the cached views were built against. */ #buf = null;
  #cache = new Map();
  /** Shard handle from `sim_create`. */ #s = 0;

  #worlds = 0;
  #stride = 0;
  #statsPerWorld = 0;
  #mapSpan = 0;
  #tickHz = 0;

  static async load(url) {
    // `instantiateStreaming` compiles while the bytes are still arriving. It requires the
    // server to send `application/wasm`; serve.mjs does.
    const src = await WebAssembly.instantiateStreaming(fetch(url), {});
    return new WasmSim(src.instance);
  }

  constructor(inst) {
    this.#inst = inst;
    this.#mem = inst.exports.memory;
    this.#mapSpan = inst.exports.sim_map_span();
    this.#tickHz = inst.exports.sim_tick_hz();
    this.#statsPerWorld = inst.exports.sim_stats_per_world();
  }

  get exports() { return this.#inst.exports; }
  get mapSpan() { return this.#mapSpan; }
  get tickHz() { return this.#tickHz; }
  get worlds() { return this.#worlds; }
  get stride() { return this.#stride; }
  get statsPerWorld() { return this.#statsPerWorld; }
  /** Cells in the cluster mirror: `worlds * stride`. */
  get cells() { return this.#worlds * this.#stride; }

  create(worlds, unitsPerWorld, capacity, seed) {
    const e = this.#inst.exports;
    if (this.#s) e.sim_destroy(this.#s);
    this.#s = e.sim_create(worlds, unitsPerWorld, capacity,
      seed >>> 0, Math.floor(seed / 4294967296) >>> 0);
    if (!this.#s) throw new Error('sim_create returned null');
    this.#worlds = e.sim_worlds(this.#s);
    this.#stride = e.sim_stride(this.#s);
    this.#cache.clear();
    this.#buf = null;
    return this.#s;
  }

  destroy() {
    if (this.#s) this.#inst.exports.sim_destroy(this.#s);
    this.#s = 0;
    this.#cache.clear();
  }

  step(frames) { this.#inst.exports.sim_step(this.#s, frames); }
  gather() { this.#inst.exports.sim_gather(this.#s); }
  gatherStats() { this.#inst.exports.sim_gather_stats(this.#s); }
  get liveTotal() { return this.#inst.exports.sim_live_total(this.#s); }
  get frame() { return this.#inst.exports.sim_frame(this.#s) >>> 0; }
  get ordersApplied() { return this.#inst.exports.sim_orders_applied(this.#s) >>> 0; }

  /** 64-bit batch digest as a lower-case hex string, for the native/wasm cross-check. */
  digestHex() {
    const e = this.#inst.exports;
    const hi = e.sim_digest_hi(this.#s) >>> 0, lo = e.sim_digest_lo(this.#s) >>> 0;
    return hi.toString(16).padStart(8, '0') + lo.toString(16).padStart(8, '0');
  }

  order(kind, world, owner, sx, sy, tx, ty, radius) {
    return this.#inst.exports.sim_order(this.#s, kind, world, owner, sx, sy, tx, ty, radius);
  }

  #view(key, Ctor, ptr, len) {
    const buf = this.#mem.buffer;
    if (buf !== this.#buf) { this.#cache.clear(); this.#buf = buf; }
    let v = this.#cache.get(key);
    if (v === undefined) { v = new Ctor(buf, ptr, len); this.#cache.set(key, v); }
    return v;
  }

  /** The cluster mirror's x column. One CPU copy behind the authoritative world columns. */
  mirrorX() {
    return this.#view('mx', Int32Array, this.#inst.exports.sim_mirror_x_ptr(this.#s), this.cells);
  }
  mirrorY() {
    return this.#view('my', Int32Array, this.#inst.exports.sim_mirror_y_ptr(this.#s), this.cells);
  }
  /** Per-instance tag: bit 31 occupied, bits 0..7 owner. */
  tags() {
    return this.#view('tag', Uint32Array, this.#inst.exports.sim_tag_ptr(this.#s), this.cells);
  }
  stats() {
    return this.#view('st', Uint32Array, this.#inst.exports.sim_stats_ptr(this.#s),
      this.#worlds * this.#statsPerWorld);
  }

  /**
   * **Zero-copy surface.** A view directly over world `w`'s own `pos_x` column — this array
   * *is* the simulation's memory, not a snapshot of it. Stepping the sim changes its
   * contents with no call on this side.
   */
  worldX(w) {
    const e = this.#inst.exports;
    return this.#view('wx' + w, Int32Array, e.sim_world_x_ptr(this.#s, w), this.#stride);
  }
  worldY(w) {
    const e = this.#inst.exports;
    return this.#view('wy' + w, Int32Array, e.sim_world_y_ptr(this.#s, w), this.#stride);
  }
  worldLive(w) { return this.#inst.exports.sim_world_live(this.#s, w) >>> 0; }
}

/**
 * Order kinds; mirrors `don_web::order_kind`.
 *
 * `MOVE_TO` and `HALT` are the engine's **real** opcodes from `schema/command-wire.json`
 * (`MoveToCommand` = 0x07, `HaltCommand` = 0x0c). `DEMO_SPAWN` is not an engine command and
 * sits outside the 0x00-0x51 range on purpose. See the Rust module for exactly which fields
 * of the real structs are honoured.
 */
export const ORDER = { MOVE_TO: 0x07, HALT: 0x0c, DEMO_SPAWN: 0x1000 };
