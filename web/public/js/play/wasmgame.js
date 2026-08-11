// The wasm game module, its typed-array views, and the command path.
//
// Nothing here parses JSON in the frame loop. `views()` hands back `Int32Array` /
// `Uint32Array` / `Uint16Array` windows onto the simulation's own allocations, and the
// renderer binds those straight to GPU buffers. The only revalidation is on the buffer
// identity: any wasm allocation can grow linear memory and *detach* every existing view,
// and `game_products` / the command decode path both allocate, so the check is not
// theoretical.

import { COMMANDS, OP, encode, encodeGroup } from '../wire.gen.js';
import { assertPlayWasmContract } from './wasm-contract.mjs';

export { OP, COMMANDS };

export const RES_NAMES = ['food', 'timber', 'wealth', 'knowledge', 'metal', 'oil'];
export const GAP_NAMES = [
  'unknown opcode', 'nothing selected', 'cannot afford', 'population capped',
  'placement blocked', 'not a producer', 'queue full', 'no worker',
  'tile not gatherable', 'wrong age', 'object capacity full', 'movement route failed',
  'target is not hostile',
];
export const DIPLOMACY = Object.freeze(['war', 'peace', 'ally']);
export const VICTORY_MODES = Object.freeze([
  Object.freeze({ slug: 'standard', label: 'Standard' }),
  Object.freeze({ slug: 'sudden-death', label: 'Sudden Death' }),
  Object.freeze({ slug: 'conquest', label: 'Conquest' }),
  Object.freeze({ slug: 'score', label: 'Score' }),
  Object.freeze({ slug: 'time-limit', label: 'Time Limit' }),
  Object.freeze({ slug: 'musical-chairs', label: 'Musical Chairs' }),
  Object.freeze({ slug: 'wonder', label: 'Wonder' }),
  Object.freeze({ slug: 'territory', label: 'Territory' }),
  Object.freeze({ slug: 'economic', label: 'Economic' }),
  Object.freeze({ slug: 'tech-race', label: 'Tech Race' }),
  Object.freeze({ slug: 'scenario', label: 'Scenario' }),
]);
const LEADER_FLAG = Object.freeze({
  valid: 1, active: 2, won: 0x20, defeated: 0x40, survived: 0x80,
});

/** Tag bit layout — must match the authoritative-core projection in `game_abi`. */
export const TAG = {
  occupied: 1 << 31, selected: 1 << 30, building: 1 << 29, underConstruction: 1 << 28,
};
export const CORE_CAP = Object.freeze({
  save: 1 << 0, load: 1 << 1, move: 1 << 2, attack: 1 << 3, halt: 1 << 4,
  train: 1 << 5, research: 1 << 6, build: 1 << 7,
});

export class GameModule {
  constructor(instance) {
    this.x = instance.exports;
    assertPlayWasmContract(this.x);
    this.mem = this.x.memory;
    this.g = 0;
    this._buf = null;
    this._v = {};
    this.tiles = this.x.game_map_tiles();
    this.span = this.x.game_map_span();
    this.subtile = this.x.game_subtile();
    this.playerCount = this.x.game_players_count();
    this.playerFields = this.x.game_player_fields();
    this.gapCount = this.x.game_gap_count();
    this.capabilityBits = this.x.game_capabilities() >>> 0;
    this._submitted = 0;
    this._commandObserver = null;
    this._readiness = this._loadReadiness();
  }

  static async load(url) {
    const src = await fetch(url);
    if (!src.ok) throw new Error(`${url}: ${src.status}`);
    const { instance } = await WebAssembly.instantiate(await src.arrayBuffer(), {});
    return new GameModule(instance);
  }

  /** Stage the two data packs, then create the authoritative core and its projections. */
  create(gamedataBytes, playdataBytes, seed = 0xc0ffee) {
    if (gamedataBytes) {
      const p = this.x.game_gamedata_alloc(gamedataBytes.length);
      new Uint8Array(this.mem.buffer, p, gamedataBytes.length).set(gamedataBytes);
    }
    if (playdataBytes) {
      const p = this.x.game_playdata_alloc(playdataBytes.length);
      new Uint8Array(this.mem.buffer, p, playdataBytes.length).set(playdataBytes);
    }
    this.g = this.x.game_create(seed >>> 0, 0);
    this.seed = seed >>> 0;
    this._submitted = 0;
    this._buf = null;
    // `Game::players` is an exported snapshot populated by `game_step`, rather than the
    // live ledger itself. A zero-frame step refreshes that snapshot without advancing the
    // world, so callers can inspect a newly-created session immediately.
    if (this.g) this.x.game_step(this.g, 0);
    return this.g !== 0;
  }

  /**
   * Replace the current world with a fresh world over the already staged data packs.
   * Create first and destroy second so an allocation failure cannot discard the live
   * session. The temporary second world may grow wasm memory, so all views are retired.
   */
  restart(seed) {
    const normalized = seed >>> 0;
    const next = this.x.game_create(normalized, 0);
    if (!next) return false;
    const previous = this.g;
    this.g = next;
    this.seed = normalized;
    this._submitted = 0;
    this._buf = null;
    this._v = {};
    this.x.game_step(this.g, 0);
    if (previous) this.x.game_destroy(previous);
    return true;
  }

  activatePlayers(players) {
    const teams = Array.from({ length: this.playerCount }, (_, player) => player);
    return this.startManualTeams(players, teams, 0, players?.[0] ?? 0, false);
  }

  /**
   * Configure and start one frame-zero manual match in the authoritative Sim.
   * JavaScript validates and packs the request but retains no roster or team copy; all
   * success checks query the installed PlayerSetup owner back through the read-only ABI.
   */
  startManualTeams(players, teams, teamStyle = 0, localPlayer = 0, ranked = false) {
    if (!this.g || !Array.isArray(players) || !Array.isArray(teams) ||
        teams.length !== this.playerCount || !Number.isInteger(teamStyle) ||
        teamStyle < 0 || teamStyle > 0xff || !Number.isInteger(localPlayer) ||
        localPlayer < 0 || localPlayer >= this.playerCount || typeof ranked !== 'boolean') return false;
    const roster = [];
    const seen = new Set();
    for (const player of players) {
      if (!Number.isInteger(player) || player < 0 || player >= this.playerCount || seen.has(player)) {
        return false;
      }
      seen.add(player);
      roster.push(player);
    }
    if (!roster.length || !seen.has(localPlayer)) return false;
    let mask = 0;
    let packed = 0;
    for (let player = 0; player < this.playerCount; player++) {
      const team = seen.has(player) ? teams[player] : 8;
      if (!Number.isInteger(team) || team < 0 || team > 0xff) return false;
      mask |= seen.has(player) ? (1 << player) : 0;
      packed |= team << (player * 8);
    }
    if (this.x.game_start_manual_teams(
      this.g, mask >>> 0, packed >>> 0, teamStyle, localPlayer, ranked ? 1 : 0) !== 1) return false;
    const active = new Set(this.activePlayers());
    const configuredMask = this.x.game_team_configured_mask(this.g) >>> 0;
    return roster.every((player) => active.has(player) &&
      (configuredMask & (1 << player)) !== 0 &&
      this.x.game_team(this.g, player) === (teamStyle === 3 ? (player === localPlayer ? 0 : 1) : teams[player])) &&
      this.x.game_team_style(this.g) === teamStyle;
  }

  /** Live setup roster queried from `Sim::vic_leaders`; JavaScript owns no roster copy. */
  activePlayers() {
    const mask = this.x.game_active_player_mask(this.g) >>> 0;
    const players = [];
    for (let player = 0; player < this.playerCount; player++) {
      if ((mask & (1 << player)) !== 0) players.push(player);
    }
    return players;
  }

  _staticText(ptr, len) {
    if (!ptr || !len) return '';
    return new TextDecoder().decode(new Uint8Array(this.mem.buffer, ptr, len));
  }

  _loadReadiness() {
    const blockers = [];
    const n = this.x.game_playable_blocker_count();
    for (let i = 0; i < n; i++) {
      blockers.push({
        slug: this._staticText(
          this.x.game_playable_blocker_slug_ptr(i), this.x.game_playable_blocker_slug_len(i)),
        title: this._staticText(
          this.x.game_playable_blocker_title_ptr(i), this.x.game_playable_blocker_title_len(i)),
      });
    }
    return Object.freeze({
      mode: 'improved',
      surface: 'playable',
      ready: blockers.length === 0,
      blockers: Object.freeze(blockers.map(Object.freeze)),
      source: 'don_sim::deviations compiled into don_web.wasm',
    });
  }

  /**
   * Views over the simulation's own columns. Rebuilt only when linear memory has been
   * replaced, which is the one thing that can silently invalidate them.
   */
  views() {
    if (this._buf === this.mem.buffer) return this._v;
    const b = this.mem.buffer;
    const cap = this.x.game_capacity(this.g);
    this._v = {
      x: new Int32Array(b, this.x.game_x_ptr(this.g), cap),
      y: new Int32Array(b, this.x.game_y_ptr(this.g), cap),
      tag: new Uint32Array(b, this.x.game_tag_ptr(this.g), cap),
      tiles: new Uint16Array(b, this.x.game_tile_ptr(this.g), this.tiles * this.tiles),
      pick: new Int16Array(b, this.x.game_pick_ptr(this.g), 4096),
      info: new Int32Array(b, this.x.game_info_ptr(this.g), 32),
      players: new Int32Array(
        b, this.x.game_players_ptr(this.g), this.playerCount * this.playerFields),
      gaps: new Uint32Array(b, this.x.game_gaps_ptr(this.g), this.gapCount),
      products: new Int32Array(b, this.x.game_products_ptr(this.g), 512),
      cmd: new Uint8Array(b, this.x.game_cmd_ptr(this.g), this.x.game_cmd_capacity()),
    };
    this._buf = b;
    return this._v;
  }

  step(frames = 1) { this.x.game_step(this.g, frames); }
  get live() { return this.x.game_live(this.g); }
  get frame() { return this.x.game_frame(this.g); }
  get terrainVersion() { return this.x.game_terrain_version(this.g); }
  get hasGameData() { return this.x.game_has_gamedata(this.g) === 1; }
  get hasPlayData() { return this.x.game_has_playdata(this.g) === 1; }
  get rngState() { return this.x.game_rng_state(this.g) >>> 0; }
  get coreSeed() { return this.x.game_seed(this.g) >>> 0; }
  supports(name) {
    const bit = CORE_CAP[name];
    return Number.isInteger(bit) && (this.capabilityBits & bit) !== 0;
  }
  digest() {
    const lo = this.x.game_digest_lo(this.g) >>> 0;
    const hi = this.x.game_digest_hi(this.g) >>> 0;
    return hi.toString(16).padStart(8, '0') + lo.toString(16).padStart(8, '0');
  }

  _lastError() {
    return this._staticText(this.x.game_error_ptr(this.g), this.x.game_error_len(this.g)) ||
      'core refused the operation';
  }

  /** Copy the deterministic `don_sim::systems::save_load` image out of wasm memory. */
  saveCore() {
    if (!this.supports('save')) throw new Error('core save export is unavailable');
    if (this.x.game_save(this.g) !== 1) throw new Error(this._lastError());
    const len = this.x.game_save_len(this.g) >>> 0;
    const ptr = this.x.game_save_ptr(this.g) >>> 0;
    // `game_save` may grow memory; read the current buffer only after it returns.
    return new Uint8Array(new Uint8Array(this.mem.buffer, ptr, len));
  }

  /** Atomically replace the authoritative core from a bounded save image. */
  loadCore(input) {
    if (!this.supports('load')) throw new Error('core save import is unavailable');
    const bytes = input instanceof Uint8Array ? input : new Uint8Array(input);
    if (!bytes.length || bytes.length > (this.x.game_save_limit() >>> 0)) {
      throw new Error(`load refused: invalid byte length ${bytes.length}`);
    }
    const ptr = this.x.game_load_alloc(this.g, bytes.length) >>> 0;
    if (!ptr) throw new Error(this._lastError());
    new Uint8Array(this.mem.buffer, ptr, bytes.length).set(bytes);
    if (this.x.game_load_commit(this.g) !== 1) throw new Error(this._lastError());
    this._submitted = 0;
    this._buf = null;
    this._v = {};
    return {
      frame: this.frame, digest: this.digest(), rngState: this.rngState,
      live: this.live, bytes: bytes.length,
    };
  }
  startOf(p) { return [this.x.game_start_x(this.g, p), this.x.game_start_y(this.g, p)]; }
  setIncomeMode(m) { this.x.game_set_income_mode(this.g, m); }
  setPopSetting(k) { this.x.game_set_pop_setting(this.g, k); }
  /** Benchmark hook — bypasses cost and population, and any run that uses it says so. */
  debugSpawn(owner, typeId, n) { return this.x.game_debug_spawn(this.g, owner, typeId, n); }

  // ---- queries ------------------------------------------------------------------------

  /** `WorldData::space_at_corner`'s grade — 0 blocked, 2 partial, 3 approach, 4 clear. */
  placementGrade(who, typeId, tx, ty) {
    return this.x.game_placement_grade(this.g, who, typeId, tx, ty);
  }
  /** `WorldData::check_building_wcoord`'s grade for the W cell containing a tile. */
  checkWCell(who, tx, ty) { return this.x.game_check_wcell(this.g, who, tx, ty); }
  /** Resource slot a tile yields (0..5) or -1; gated by `has_gather_access`. */
  tileResource(tx, ty) { return this.x.game_tile_resource(this.g, tx, ty); }
  pickAt(x, y) { return this.x.game_pick_at(this.g, x, y); }
  idAtRow(row) { return this.x.game_id_at_row(this.g, row); }
  pickBox(who, x0, y0, x1, y1) {
    const n = this.x.game_pick_box(this.g, who, x0, y0, x1, y1);
    return this.views().pick.subarray(0, n);
  }
  pickType(who, typeId) {
    const n = this.x.game_pick_type(this.g, who, typeId);
    return this.views().pick.subarray(0, n);
  }
  products(producerType) {
    const n = this.x.game_products(this.g, producerType);
    return Array.from(this.views().products.subarray(0, n));
  }

  /** Object info block, or null. Field names match `game_object_info`'s doc comment. */
  info(id) {
    if (this.x.game_object_info(this.g, id) !== 1) return null;
    const i = this.views().info;
    return {
      id: i[0], owner: i[1], typeId: i[2], isBuilding: i[3] === 1, hits: i[4], maxHits: i[5],
      attack: i[6], armor: i[7], range: i[8], moves: i[9], recharge: i[10],
      order: i[11], orderArg: i[12], buildProgress: i[13], jobTime: i[14],
      queue: Array.from(i.subarray(16, 16 + i[15])), queueN: i[15],
      pop: i[24], gatherRes: i[25], x: i[26], sizeX: i[27], sizeY: i[28], y: i[29],
    };
  }

  /** Per-player ledger. All six arrays are in engine slot order. */
  player(p) {
    const f = this.playerFields;
    const b = this.views().players.subarray(p * f, (p + 1) * f);
    return {
      stock: Array.from(b.subarray(0, 6)),
      income: Array.from(b.subarray(6, 12)),
      cap: Array.from(b.subarray(12, 18)),
      workers: Array.from(b.subarray(18, 24)),
      gross: Array.from(b.subarray(24, 30)),
      pop: b[30], popCap: b[31], age: b[32], research: b[33],
    };
  }

  /** Read-only setup/victory facts projected directly from the authoritative Sim. */
  leader(p) {
    if (!Number.isInteger(p) || p < 0 || p >= this.playerCount) {
      throw new RangeError(`leader slot ${p} is outside the browser player cohort`);
    }
    const flags = this.x.game_leader_flags(this.g, p) >>> 0;
    const configuredMask = this.x.game_team_configured_mask(this.g) >>> 0;
    return Object.freeze({
      player: p,
      team: this.x.game_team(this.g, p),
      teamConfigured: (configuredMask & (1 << p)) !== 0,
      teamSource: (configuredMask & (1 << p)) !== 0
        ? 'Sim PlayerSetup owner after deterministic init_teams transaction'
        : 'Sim victory Leaders::team_of default-own-slot fallback',
      flags,
      active: (flags & (LEADER_FLAG.valid | LEADER_FLAG.active)) ===
        (LEADER_FLAG.valid | LEADER_FLAG.active),
      won: (flags & LEADER_FLAG.won) !== 0,
      defeated: (flags & LEADER_FLAG.defeated) !== 0,
      survived: (flags & LEADER_FLAG.survived) !== 0,
      score: this.x.game_victory_score(this.g, p),
      teamScore: this.x.game_team_score(this.g, p),
    });
  }

  relation(who, other) {
    if (![who, other].every(p => Number.isInteger(p) && p >= 0 && p < this.playerCount)) {
      throw new RangeError(`diplomacy query ${who} -> ${other} leaves the browser player cohort`);
    }
    const id = this.x.game_diplomacy(this.g, who, other);
    return Object.freeze({ id, name: DIPLOMACY[id] ?? 'invalid' });
  }

  match() {
    const modeId = this.x.game_victory_mode(this.g);
    const mode = VICTORY_MODES[modeId] ?? Object.freeze({
      slug: `unknown-${modeId}`, label: `Unknown mode ${modeId}`,
    });
    const activePlayers = Object.freeze(this.activePlayers());
    const gameOver = this.x.game_is_over(this.g) === 1;
    const phase = gameOver ? 'ended' : activePlayers.length ? 'active' : 'setup';
    return Object.freeze({
      modeId, ...mode, gameOver, phase, activePlayers,
      teamStyle: this.x.game_team_style(this.g),
      teamConfiguredMask: this.x.game_team_configured_mask(this.g) >>> 0,
    });
  }

  gaps() { return Array.from(this.views().gaps); }

  /** Repository product gate for the Sim-backed playable adapter. */
  readiness() { return this._readiness; }

  /** Exact packet lifecycle at the browser/Wasm boundary. */
  transport() {
    const drained = this.x.game_commands_seen(this.g) >>> 0;
    return {
      submitted: this._submitted,
      drained,
      pending: Math.max(0, this._submitted - drained),
      ordersApplied: this.x.game_orders_applied(this.g) >>> 0,
    };
  }

  /** Observe accepted browser-to-Wasm packets without changing the command ABI. */
  observeCommands(observer) {
    this._commandObserver = typeof observer === 'function' ? observer : null;
  }

  // ---- commands ------------------------------------------------------------------------

  /**
   * Post one engine command. `bytes` is the exact wire packet — byte 0 the opcode, the
   * rest the struct's own layout at the offsets `schema/command-wire.json` gives.
   */
  submit(who, bytes) {
    const v = this.views();
    v.cmd.set(bytes, 0);
    this.x.game_submit(this.g, who, bytes.length);
    this._submitted++;
    if (this._commandObserver) {
      this._commandObserver({
        frame: this.frame,
        who: who >>> 0,
        bytes: new Uint8Array(bytes),
      });
    }
    return bytes;
  }

  group(who, ids) { return this.submit(who, encodeGroup(who, ids)); }
  moveTo(who, x, y) { return this.submit(who, encode(OP.MOVE_TO, { to_x: x, to_y: y })); }
  attack(who, whom) { return this.submit(who, encode(OP.ATTACK, { whom })); }
  halt(who) { return this.submit(who, encode(OP.HALT, {})); }
  gather(who, tileIndex) { return this.submit(who, encode(OP.GATHER, { ox: tileIndex })); }
  build(who, tx, ty, type) {
    return this.submit(who, encode(OP.BUILD, { x: tx, y: ty, x2: tx, y2: ty, type }));
  }
  queueUp(who, type, num = 1) {
    return this.submit(who, encode(OP.QUEUE_UP, { type, num }));
  }
  unqueue(who, type) {
    return this.submit(who, encode(OP.UNQUEUE, { who, o: -1, type, uid: -1 }));
  }
}
