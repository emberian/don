// Loader for the raw wasm shim. No wasm-bindgen, no glue module, zero imports.
//
// The module is instantiated with an empty import object because `don_web.wasm` imports
// nothing at all (verify with `WebAssembly.Module.imports`). Everything that crosses the
// boundary is a `u32`, a byte offset into linear memory, or a span of engine command bytes.
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
//
// # Game data
//
// `loadGameData(bytes)` stages the packed unit/balance/rules blob *before* `create`, which
// is also the only allocation that can grow the heap. `isReal` afterwards says whether the
// shard is running on the derived tables or on the synthetic stand-in; the UI must show it.

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
  #tickMs = 0;
  #dataReal = false;

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
    this.#tickMs = inst.exports.sim_tick_ms();
    this.#statsPerWorld = inst.exports.sim_stats_per_world();
  }

  get exports() { return this.#inst.exports; }
  get mapSpan() { return this.#mapSpan; }
  /** Milliseconds per tick at Normal speed: 67, from `TurnControl::timings`. */
  get tickMs() { return this.#tickMs; }
  get worlds() { return this.#worlds; }
  get stride() { return this.#stride; }
  get statsPerWorld() { return this.#statsPerWorld; }
  /** Cells in the cluster mirror: `worlds * stride`. */
  get cells() { return this.#worlds * this.#stride; }
  /** Whether the staged pack parsed. False means the synthetic table is in play. */
  get dataReal() { return this.#dataReal; }

  /**
   * Stage the packed game data. Copies once into wasm memory, before any view exists, and
   * returns whether the shim accepted it.
   */
  loadGameData(bytes) {
    const e = this.#inst.exports;
    const p = e.sim_data_alloc(bytes.length);
    new Uint8Array(this.#mem.buffer, p, bytes.length).set(bytes);
    this.#cache.clear();
    this.#buf = null;
    this.#dataReal = e.sim_data_check() === 1;
    return this.#dataReal;
  }

  create(worlds, owners, perOwner, capacity, seed) {
    const e = this.#inst.exports;
    if (this.#s) e.sim_destroy(this.#s);
    this.#s = e.sim_create(worlds, owners, perOwner, capacity,
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
  get kills() { return this.#inst.exports.sim_kills(this.#s) >>> 0; }
  get damage() { return this.#inst.exports.sim_damage_lo(this.#s) >>> 0; }
  get ordersApplied() { return this.#inst.exports.sim_orders_applied(this.#s) >>> 0; }
  get commandsSeen() { return this.#inst.exports.sim_commands_seen(this.#s) >>> 0; }
  get isReal() { return this.#inst.exports.sim_is_real(this.#s) === 1; }
  /** Distinct values in the loaded balance table — 367 for the real one. */
  get balanceDistinct() { return this.#inst.exports.sim_balance_distinct(this.#s) >>> 0; }

  /** 64-bit batch digest as a lower-case hex string, for the native/wasm cross-check. */
  digestHex() {
    const e = this.#inst.exports;
    const hi = e.sim_digest_hi(this.#s) >>> 0, lo = e.sim_digest_lo(this.#s) >>> 0;
    return hi.toString(16).padStart(8, '0') + lo.toString(16).padStart(8, '0');
  }

  /**
   * Submit one engine command, as wire bytes, for `world` on behalf of player `who`.
   *
   * The bytes go into the shim's scratch buffer and are decoded on the other side at the
   * offsets `schema/command-wire.json` gives — so what crosses this boundary is the
   * engine's packet, not an envelope this app invented.
   */
  submit(world, who, bytes) {
    const e = this.#inst.exports;
    const cap = e.sim_cmd_capacity();
    if (bytes.length > cap) throw new Error(`command ${bytes.length} B exceeds scratch ${cap} B`);
    new Uint8Array(this.#mem.buffer, e.sim_cmd_ptr(this.#s), bytes.length).set(bytes);
    return e.sim_submit(this.#s, world, who, bytes.length);
  }

  /** Object ids of `who`'s units within `radius` subtiles of a point. A query, not a command. */
  pickBox(world, who, x, y, radius) {
    const e = this.#inst.exports;
    const n = e.sim_pick_box(this.#s, world, who, x, y, radius);
    if (!n) return new Int16Array(0);
    // Copied out: the caller keeps it across calls that could invalidate the view.
    return new Int16Array(this.#mem.buffer, e.sim_pick_ptr(this.#s), n).slice();
  }

  /** Nearest unit to a point, any owner. `null` when the world is empty. */
  pickNearest(world, x, y) {
    const e = this.#inst.exports;
    if (!e.sim_pick_nearest(this.#s, world, x, y)) return null;
    const i = new Int32Array(this.#mem.buffer, e.sim_info_ptr(this.#s), 8);
    return { id: i[0], owner: i[1], typeId: i[2], hits: i[3], maxHits: i[4],
             roster: i[5], attackX10: i[6], armor: i[7] };
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
  mirrorTag() {
    return this.#view('mt', Uint32Array, this.#inst.exports.sim_mirror_tag_ptr(this.#s), this.cells);
  }
  stats() {
    return this.#view('st', Uint32Array, this.#inst.exports.sim_stats_ptr(this.#s),
      this.#worlds * this.#statsPerWorld);
  }

  /**
   * **Zero-copy surface.** Views directly over world `w`'s own columns — these arrays *are*
   * the simulation's memory, not snapshots of it. Stepping the sim changes their contents
   * with no call on this side. The tag column is refreshed inside the tick for exactly this
   * reason: packing render bits in JavaScript per entity per frame is the CPU loop the
   * whole architecture exists to delete.
   */
  worldX(w) {
    const e = this.#inst.exports;
    return this.#view('wx' + w, Int32Array, e.sim_world_x_ptr(this.#s, w), this.#stride);
  }
  worldY(w) {
    const e = this.#inst.exports;
    return this.#view('wy' + w, Int32Array, e.sim_world_y_ptr(this.#s, w), this.#stride);
  }
  worldTag(w) {
    const e = this.#inst.exports;
    return this.#view('wt' + w, Uint32Array, e.sim_world_tag_ptr(this.#s, w), this.#stride);
  }
  worldLive(w) { return this.#inst.exports.sim_world_live(this.#s, w) >>> 0; }
}

/**
 * Per-instance tag bits, mirroring `RealWorld::tag_of`. Decoding them here rather than
 * spreading shifts through the UI keeps one definition of the layout on this side.
 */
export const TAG = {
  occupied: (t) => (t >>> 31) !== 0,
  selected: (t) => ((t >>> 30) & 1) !== 0,
  size: (t) => (t >>> 24) & 0x3f,
  hp: (t) => (t >>> 16) & 0xff,
  hue: (t) => (t >>> 8) & 0xff,
  owner: (t) => t & 0xf,
};
