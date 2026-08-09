// The client: camera, selection, orders, HUD, build and train menus, hotkeys, minimap.
//
// Every order a player gives leaves this file as an **engine command packet** — the bytes
// `schema/command-wire.json` describes, encoded by the generated codec, decoded in Rust at
// the derived offsets. A human clicking and a policy acting therefore emit the same bytes,
// which is what makes that a checkable statement rather than an aspiration.
//
// What the client refuses to do: swallow an order. When the simulation cannot execute
// something, the wasm side counts it in `game_gaps_ptr` and the coverage panel shows it
// live, with the reason.

import { GameModule, RES_NAMES, GAP_NAMES, OP, TAG } from './wasmgame.js';
import { makeRenderer } from './gfx.js';
import { REPLAY_EVIDENCE } from './readiness.gen.js';
import { decode } from '../wire.gen.js';

const $ = (id) => document.getElementById(id);
const clamp = (v, a, b) => (v < a ? a : v > b ? b : v);

/** Ticks per second at Normal speed: `TurnControl::timings` 0x00AFC4A4 is 67 ms. */
const TICK_MS = 67;
const DEFAULT_SEED = 0x00c0ffee;
const SESSION_MAP = 'integration-land';
const POPULATION_LIMITS = Object.freeze([25, 50, 75, 100, 125, 150, 175, 200]);
const INCOME_MODES = Object.freeze([
  Object.freeze({ value: 0, slug: 'retail-cap', label: 'retail commerce cap' }),
  Object.freeze({ value: 1, slug: 'uncapped-experiment', label: 'DoN uncapped experiment' }),
]);
const QUEUE_CAPACITY = 8; // game_object_info exposes queue_n plus q0..q7.
const OWNER_COLOURS = Object.freeze(['#5c9eff', '#ff5c4d', '#6bd97a', '#ffbf47']);

const state = {
  mod: null, gfx: null, data: null, play: null,
  who: 0,
  sessionSeed: DEFAULT_SEED,
  sessionInitialDigest: '',
  sessionIncomeMode: 0,
  sessionPopSetting: 3,
  cam: { x: 0, y: 0, tilePx: 22 },
  drag: null, panning: null,
  groups: new Map(),
  lastGroupKey: { key: null, t: 0 },
  buildType: null,
  speed: 1, paused: false,
  hoverTile: [0, 0],
  fps: 0, stepMs: 0, uploadMs: 0, drawMs: 0,
  log: [],
  terrainVersion: -1,
  idleCursor: 0,
  selection: [],
  edge: null,
  commandMode: null,
  toastTimer: 0,
  rendererErrorCount: 0,
  paletteNotice: '',
  cameraSource: 'home',
};

// ---------------------------------------------------------------------------------------
// boot
// ---------------------------------------------------------------------------------------

async function boot() {
  const canvas = $('gl');
  const params = new URLSearchParams(location.search);

  const mod = await GameModule.load('./wasm/don_web.wasm');
  state.mod = mod;

  const [gamedata, playdata, playjson] = await Promise.all([
    fetchBytes('./data/gamedata.bin'),
    fetchBytes('./data/playdata.bin'),
    fetchJson('./data/playdata.json'),
  ]);
  if (!gamedata || !playdata || !playjson) {
    throw new Error('playable client requires gamedata.bin, playdata.bin, and playdata.json');
  }
  state.play = playjson;

  const seed = parseSessionSeed(params.get('seed') ?? DEFAULT_SEED);
  if (!mod.create(gamedata, playdata, seed)) throw new Error('game_create failed');
  state.sessionSeed = seed;
  state.who = parseSessionPlayer(params.get('player'), mod.playerCount);
  state.sessionIncomeMode = parseSessionIncome(params.get('income'));
  state.sessionPopSetting = parseSessionPopulation(params.get('population'));
  mod.setIncomeMode(state.sessionIncomeMode);
  mod.setPopSetting(state.sessionPopSetting);
  state.sessionInitialDigest = mod.digest();
  if (!mod.hasGameData || !mod.hasPlayData) {
    throw new Error('packed retail-derived tables failed validation');
  }
  renderReadinessStatic();
  say(`wasm up — ${mod.tiles}x${mod.tiles} tiles, ${mod.playerCount} players`, 'ok');
  badge('data', mod.hasGameData ? 'unit tables: live' : 'unit tables: SYNTHETIC',
    mod.hasGameData ? 'on' : 'bad');
  badge('pdata', mod.hasPlayData ? 'costs + 312 edges: live' : 'play tables: MISSING',
    mod.hasPlayData ? 'on' : 'bad');

  state.gfx = await makeRenderer(canvas, params.get('backend'));
  badge('be', state.gfx.kind, state.gfx.kind === 'webgpu' ? 'on' : 'off');
  resize();
  state.gfx.provision(mod.tiles, mod.x.game_capacity(mod.g));

  const [sx, sy] = mod.startOf(state.who);
  centreOn(sx, sy);

  // A failed WebGPU attempt replaces the canvas before Canvas 2D claims it. Bind input to
  // the renderer's live canvas, not the now-detached element captured at boot.
  wireInput($('gl'));
  wirePanels();
  initializeSessionPanel();
  initializeObjectivesPanel();
  buildPalette();
  renderMenus();
  requestAnimationFrame(frame);
}

async function fetchBytes(url) {
  try {
    const r = await fetch(url);
    if (!r.ok) return null;
    return new Uint8Array(await r.arrayBuffer());
  } catch { return null; }
}
async function fetchJson(url) {
  try {
    const r = await fetch(url);
    if (!r.ok) return null;
    return await r.json();
  } catch { return null; }
}

function parseSessionSeed(value) {
  const text = String(value).trim();
  const radix = /^0x/i.test(text) ? 16 : 10;
  const digits = radix === 16 ? text.slice(2) : text;
  const valid = radix === 16 ? /^[0-9a-f]{1,8}$/i.test(digits) : /^\d{1,10}$/.test(digits);
  if (!valid) {
    throw new Error(`invalid session seed ${JSON.stringify(text)}; use uint32 decimal or 0x hexadecimal`);
  }
  const seed = Number.parseInt(digits, radix);
  if (!Number.isSafeInteger(seed) || seed < 0 || seed > 0xffffffff) {
    throw new Error(`session seed outside uint32: ${text}`);
  }
  return seed >>> 0;
}

function parseSessionPlayer(value, count) {
  if (value === null || value === undefined || value === '') return 0;
  const player = Number(value);
  return Number.isInteger(player) && player >= 0 && player < count ? player : 0;
}

function formatSeed(seed) { return `0x${(seed >>> 0).toString(16).padStart(8, '0')}`; }

function parseSessionIncome(value) {
  if (value === null || value === '') return 0;
  return INCOME_MODES.find((mode) => mode.slug === value)?.value ?? 0;
}

function parseSessionPopulation(value) {
  if (value === null || value === '') return 3;
  const index = POPULATION_LIMITS.indexOf(Number(value));
  return index < 0 ? 3 : index;
}

// ---------------------------------------------------------------------------------------
// camera
// ---------------------------------------------------------------------------------------

function vp() {
  const c = $('gl');
  return [c.width, c.height];
}
/** Device pixels per subtile. */
function pxPerSub() { return state.cam.tilePx / state.mod.subtile; }

function camPack() {
  const m = state.mod;
  return { x: state.cam.x, y: state.cam.y, px: pxPerSub(), span: m.span, tiles: m.tiles, sub: m.subtile };
}

function centreOn(wx, wy) {
  const [w, h] = vp();
  const p = pxPerSub();
  state.cam.x = wx - w / 2 / p;
  state.cam.y = wy - h / 2 / p;
  clampCam();
}

function clampCam() {
  const m = state.mod;
  const [w, h] = vp();
  const p = pxPerSub();
  const marginW = w / p, marginH = h / p;
  state.cam.x = clamp(state.cam.x, -marginW * 0.25, m.span - marginW * 0.75);
  state.cam.y = clamp(state.cam.y, -marginH * 0.25, m.span - marginH * 0.75);
}

function screenToWorld(cx, cy) {
  const c = $('gl');
  const r = c.getBoundingClientRect();
  const dpr = c.width / r.width;
  const p = pxPerSub();
  return [
    Math.round(state.cam.x + (cx - r.left) * dpr / p),
    Math.round(state.cam.y + (cy - r.top) * dpr / p),
  ];
}

function worldToScreen(wx, wy) {
  const c = $('gl');
  const r = c.getBoundingClientRect();
  const dpr = c.width / r.width;
  const p = pxPerSub();
  return [(wx - state.cam.x) * p / dpr, (wy - state.cam.y) * p / dpr];
}

function zoomAt(cx, cy, factor) {
  const before = screenToWorld(cx, cy);
  state.cam.tilePx = clamp(state.cam.tilePx * factor, 3, 90);
  const after = screenToWorld(cx, cy);
  state.cam.x += before[0] - after[0];
  state.cam.y += before[1] - after[1];
  state.cameraSource = 'map zoom';
  clampCam();
}

// ---------------------------------------------------------------------------------------
// input
// ---------------------------------------------------------------------------------------

const keys = new Set();

function wireInput(canvas) {
  canvas.addEventListener('contextmenu', (e) => e.preventDefault());

  canvas.addEventListener('pointerdown', (e) => {
    canvas.setPointerCapture(e.pointerId);
    const w = screenToWorld(e.clientX, e.clientY);
    if (e.button === 1 || (e.button === 0 && keys.has('Space'))) {
      state.panning = { x: e.clientX, y: e.clientY, cx: state.cam.x, cy: state.cam.y };
      return;
    }
    if (e.button === 0) {
      if (state.buildType !== null) { placeBuilding(w); return; }
      if (state.commandMode !== null) { targetCommand(w, e); return; }
      // Touch has no middle button or screen-edge hover. A tap still selects; a drag pans
      // the map, making every core interaction reachable without pretending touch has a
      // right click. Mouse drag retains retail-style box selection.
      if (e.pointerType === 'touch') {
        state.panning = {
          x: e.clientX, y: e.clientY, cx: state.cam.x, cy: state.cam.y,
          touch: true, moved: false, tapWorld: w, pointerId: e.pointerId,
        };
        return;
      }
      state.drag = { x0: w[0], y0: w[1], x1: w[0], y1: w[1], sx: e.clientX, sy: e.clientY, add: e.shiftKey };
      return;
    }
    if (e.button === 2) rightClick(w, e);
  });

  canvas.addEventListener('pointermove', (e) => {
    const w = screenToWorld(e.clientX, e.clientY);
    state.hoverTile = [Math.floor(w[0] / state.mod.subtile), Math.floor(w[1] / state.mod.subtile)];
    if (state.panning) {
      if (state.panning.pointerId !== undefined && state.panning.pointerId !== e.pointerId) return;
      const c = $('gl');
      const r = c.getBoundingClientRect();
      const dpr = c.width / r.width;
      const p = pxPerSub();
      if (Math.abs(e.clientX - state.panning.x) + Math.abs(e.clientY - state.panning.y) > 6) {
        state.panning.moved = true;
      }
      state.cam.x = state.panning.cx - (e.clientX - state.panning.x) * dpr / p;
      state.cam.y = state.panning.cy - (e.clientY - state.panning.y) * dpr / p;
      state.cameraSource = 'map drag';
      clampCam();
      return;
    }
    if (state.drag) { state.drag.x1 = w[0]; state.drag.y1 = w[1]; }
  });

  canvas.addEventListener('pointerup', (e) => {
    if (state.panning) {
      if (state.panning.pointerId !== undefined && state.panning.pointerId !== e.pointerId) return;
      const pan = state.panning;
      state.panning = null;
      if (pan.touch && !pan.moved) clickSelect(pan.tapWorld, false, e.detail >= 2);
      return;
    }
    if (!state.drag) return;
    const d = state.drag;
    state.drag = null;
    const px = Math.abs(e.clientX - d.sx) + Math.abs(e.clientY - d.sy);
    if (px < 5) clickSelect([d.x0, d.y0], d.add, e.detail >= 2);
    else boxSelect(d, d.add);
  });
  canvas.addEventListener('pointercancel', () => {
    state.panning = null;
    state.drag = null;
  });

  canvas.addEventListener('wheel', (e) => {
    e.preventDefault();
    zoomAt(e.clientX, e.clientY, e.deltaY < 0 ? 1.12 : 1 / 1.12);
  }, { passive: false });

  // edge scroll
  canvas.addEventListener('pointerleave', () => { state.edge = null; });
  canvas.addEventListener('pointermove', (e) => {
    const r = canvas.getBoundingClientRect();
    const m = 18;
    state.edge = {
      x: (e.clientX - r.left < m ? -1 : e.clientX > r.right - m ? 1 : 0),
      y: (e.clientY - r.top < m ? -1 : e.clientY > r.bottom - m ? 1 : 0),
    };
  });

  window.addEventListener('keydown', onKeyDown);
  window.addEventListener('keyup', (e) => keys.delete(e.code));
  window.addEventListener('blur', () => {
    keys.clear();
    state.edge = null;
    state.panning = null;
    state.drag = null;
  });
  window.addEventListener('resize', () => { resize(); clampCam(); });

  const mini = $('mini');
  const miniJump = (e) => {
    const r = mini.getBoundingClientRect();
    const wx = (e.clientX - r.left) / r.width * state.mod.span;
    const wy = (e.clientY - r.top) / r.height * state.mod.span;
    if (e.buttons === 2 || e.button === 2) issueMove([wx | 0, wy | 0]);
    else {
      centreOn(wx, wy);
      state.cameraSource = 'minimap';
      renderObjectivesPanel();
    }
  };
  mini.addEventListener('contextmenu', (e) => e.preventDefault());
  mini.addEventListener('pointerdown', miniJump);
  mini.addEventListener('pointermove', (e) => { if (e.buttons & 1) miniJump(e); });
  mini.addEventListener('keydown', (e) => {
    if (/^Digit[1-4]$/.test(e.code)) {
      focusPlayerStart(Number(e.code.slice(5)) - 1, 'minimap keyboard');
      e.preventDefault();
      e.stopPropagation();
    } else if (e.code === 'Home' || e.code === 'Enter') {
      focusPlayerStart(state.who, 'minimap keyboard');
      e.preventDefault();
      e.stopPropagation();
    }
  });
}

function onKeyDown(e) {
  const tag = e.target && e.target.tagName;
  if (['INPUT', 'SELECT', 'TEXTAREA', 'BUTTON'].includes(tag) || e.target?.isContentEditable) return;
  keys.add(e.code);
  const m = state.mod;

  // Palette modes have dedicated shortcuts without stealing the unmodified QWERTYUI
  // action row. The buttons provide the identical touch path.
  if (e.code === 'KeyB' && !e.shiftKey) {
    openCatalog('build');
    e.preventDefault();
    return;
  }
  if (e.code === 'KeyT' && e.shiftKey) {
    openCatalog('train');
    e.preventDefault();
    return;
  }
  if (e.code === 'KeyR' && e.shiftKey) {
    openCatalog('research');
    e.preventDefault();
    return;
  }

  // control groups: Ctrl+N assigns, N recalls, NN jumps
  if (/^Digit[1-9]$/.test(e.code)) {
    const k = e.code.slice(5);
    if (e.ctrlKey || e.metaKey) {
      state.groups.set(k, state.selection.slice());
      say(`control group ${k} = ${state.selection.length} objects`);
    } else {
      const g = state.groups.get(k);
      if (g && g.length) {
        selectIds(g);
        const now = performance.now();
        if (state.lastGroupKey.key === k && now - state.lastGroupKey.t < 400) jumpToSelection();
        state.lastGroupKey = { key: k, t: now };
      }
    }
    e.preventDefault();
    return;
  }

  switch (e.code) {
    case 'Escape': cancelTargeting(); break;
    case 'KeyH': logPacket('HALT', m.halt(state.who)); break;
    case 'KeyF': jumpToSelection(); break;
    case 'Home': focusPlayerStart(state.who, 'Home key'); break;
    case 'KeyP': case 'Pause': setPaused(!state.paused); break;
    case 'Period': cycleIdleWorker(); break;
    case 'KeyA': if (e.ctrlKey || e.metaKey) { selectAllUnits(); e.preventDefault(); } break;
    case 'Equal': case 'NumpadAdd': setSpeed(state.speed * 2); break;
    case 'Minus': case 'NumpadSubtract': setSpeed(state.speed / 2); break;
    default: break;
  }
  // palette hotkeys — the row a player's left hand already rests on
  const slot = 'QWERTYUI'.indexOf(e.key.toUpperCase());
  if (slot >= 0 && !e.ctrlKey && !e.metaKey) paletteActivate(slot);
  if (['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Space'].includes(e.code)) {
    e.preventDefault();
  }
}

// ---------------------------------------------------------------------------------------
// selection — every one of these ends in a real GroupCommand
// ---------------------------------------------------------------------------------------

function selectIds(ids) {
  const capped = ids.slice(0, 255);
  state.selection = capped;
  state.paletteNotice = '';
  const bytes = state.mod.group(state.who, capped);
  if (ids.length > 255) {
    say(`selection truncated to 255 — GroupCommand.num is an unsigned char`, 'warn');
  }
  logPacket('GROUP', bytes);
  renderMenus();
}

function clickSelect(w, add, doubleClick) {
  const m = state.mod;
  const id = m.pickAt(w[0], w[1]);
  if (id < 0) {
    $('inspect').textContent = '';
    if (!add) selectIds([]);
    return;
  }
  const info = m.info(id);
  if (!info || info.owner !== state.who) {
    // Clicking an enemy inspects it but never selects it — you cannot command it.
    showInspect(info);
    return;
  }
  if (doubleClick) {
    const all = Array.from(m.pickType(state.who, info.typeId));
    say(`select all of type ${typeName(info.typeId)} — ${all.length}`);
    selectIds(all);
    return;
  }
  selectIds(add ? Array.from(new Set([...state.selection, id])) : [id]);
}

function boxSelect(d, add) {
  const m = state.mod;
  const ids = Array.from(m.pickBox(state.who, d.x0, d.y0, d.x1, d.y1));
  selectIds(add ? Array.from(new Set([...state.selection, ...ids])) : ids);
}

function selectAllUnits() {
  const m = state.mod;
  const v = m.views();
  const n = m.live;
  const ids = [];
  for (let i = 0; i < n; i++) {
    const tag = v.tag[i];
    if ((tag & 0x80000000) === 0) continue;
    if ((tag & 0xf) !== state.who) continue;
    if ((tag >>> 29) & 1) continue;              // buildings excluded
    const id = m.idAtRow(i);
    if (id >= 0) ids.push(id);
  }
  selectIds(ids);
}

function jumpToSelection() {
  const m = state.mod;
  let sx = 0, sy = 0, n = 0;
  for (const id of state.selection) {
    const i = m.info(id);
    if (i) { sx += i.x; sy += i.y; n++; }
  }
  if (n) {
    centreOn(sx / n, sy / n);
    state.cameraSource = 'selection';
  }
}

function cycleIdleWorker() {
  const m = state.mod;
  const v = m.views();
  const live = m.live;
  const idle = [];
  for (let i = 0; i < live; i++) {
    const tag = v.tag[i];
    if ((tag & 0x80000000) === 0 || (tag & 0xf) !== state.who || ((tag >>> 29) & 1)) continue;
    if (((tag >>> 4) & 0xf) !== 0) continue;   // carrying = working
    const id = m.idAtRow(i);
    const info = id >= 0 ? m.info(id) : null;
    if (info && info.order === 0) idle.push(id);
  }
  if (!idle.length) { say('no idle workers'); return; }
  state.idleCursor = (state.idleCursor + 1) % idle.length;
  selectIds([idle[state.idleCursor]]);
  jumpToSelection();
}

// ---------------------------------------------------------------------------------------
// orders
// ---------------------------------------------------------------------------------------

function rightClick(w, e) {
  const m = state.mod;
  if (state.buildType !== null) { state.buildType = null; renderMenus(); return; }
  if (!state.selection.length) { say('nothing selected', 'warn'); return; }

  const id = m.pickAt(w[0], w[1]);
  const info = id >= 0 ? m.info(id) : null;
  if (info && info.owner !== state.who) {
    logPacket('ATTACK', m.attack(state.who, id));
    ping(w, '#ff6b5b');
    return;
  }
  if (info && info.isBuilding && info.owner === state.who && info.buildProgress >= 0) {
    // Right-clicking your own foundation means "help build it" — the same order the
    // BuildCommand issued, re-pointed.
    logPacket('BUILD-ASSIST', m.build(state.who,
      Math.floor(info.x / m.subtile) - (info.sizeX >> 1),
      Math.floor(info.y / m.subtile) - (info.sizeY >> 1), info.typeId));
    ping(w, '#ffb454');
    return;
  }
  const tx = Math.floor(w[0] / m.subtile), ty = Math.floor(w[1] / m.subtile);
  if (m.tileResource(tx, ty) >= 0) {
    logPacket('GATHER', m.gather(state.who, ty * m.tiles + tx));
    ping(w, '#6ee7a8');
    return;
  }
  issueMove(w);
}

/** Execute a dock-selected command against one map target. These are the same packet
 * builders used by rightClick; the dock only replaces the missing right mouse button. */
function targetCommand(w, e = {}) {
  const m = state.mod;
  if (!state.selection.length) {
    say('select one or more units before choosing a target', 'warn');
    setCommandMode(null);
    return;
  }
  if (state.commandMode === 'move') {
    issueMove(w);
  } else if (state.commandMode === 'attack') {
    const id = m.pickAt(w[0], w[1]);
    const info = id >= 0 ? m.info(id) : null;
    if (!info || info.owner === state.who) {
      say('attack needs an enemy object target', 'warn');
      return;
    }
    logPacket('ATTACK', m.attack(state.who, id));
    ping(w, '#ff6b5b');
  } else if (state.commandMode === 'gather') {
    const tx = Math.floor(w[0] / m.subtile), ty = Math.floor(w[1] / m.subtile);
    if (m.tileResource(tx, ty) < 0) {
      say('gather needs a highlighted resource tile', 'warn');
      return;
    }
    logPacket('GATHER', m.gather(state.who, ty * m.tiles + tx));
    ping(w, '#6ee7a8');
  }
  if (!e.shiftKey) setCommandMode(null);
}

function setCommandMode(mode) {
  state.commandMode = state.commandMode === mode ? null : mode;
  if (state.commandMode !== null) state.buildType = null;
  const prompts = {
    move: 'move armed — choose a destination on the map',
    attack: 'attack armed — choose an enemy object',
    gather: 'gather armed — choose a highlighted resource tile',
  };
  if (state.commandMode) say(prompts[state.commandMode]);
  renderActionDock();
  drawOverlay();
}

function cancelTargeting() {
  const hadMode = state.commandMode !== null || state.buildType !== null;
  state.commandMode = null;
  state.buildType = null;
  if (hadMode) say('command cancelled');
  renderMenus();
  renderActionDock();
}

function issueMove(w) {
  if (!state.selection.length) return;
  logPacket('MOVE_TO', state.mod.moveTo(state.who, w[0], w[1]));
  ping(w, '#5ab7ff');
}

function placeBuilding(w) {
  const m = state.mod;
  const b = state.play?.buildings?.[String(state.buildType)];
  const gate = b ? paletteGate({ kind: 'build', id: state.buildType, ...b })
    : { enabled: false, reasons: ['building record unavailable'] };
  if (!gate.enabled) {
    const message = `building unavailable — ${gate.reasons.join(' · ')}`;
    state.paletteNotice = message;
    say(message, 'warn');
    renderPaletteFeedback();
    return;
  }
  const [tx, ty] = anchorFor(w, b);
  const grade = m.placementGrade(state.who, state.buildType, tx, ty);
  if (grade === 0) { say('placement blocked — space_at_corner graded CORE_BLOCKED', 'warn'); return; }
  logPacket('BUILD', m.build(state.who, tx, ty, state.buildType));
  state.paletteNotice = `${b.name} build packet submitted; applies at the next tick`;
  renderPaletteFeedback();
  ping(w, '#6ee7a8');
  if (!keys.has('ShiftLeft') && !keys.has('ShiftRight')) { state.buildType = null; renderMenus(); }
}

function anchorFor(w, b) {
  const m = state.mod;
  const sx = b ? b.xSize : 2, sy = b ? b.ySize : 2;
  return [
    clamp(Math.floor(w[0] / m.subtile) - (sx >> 1), 0, m.tiles - sx),
    clamp(Math.floor(w[1] / m.subtile) - (sy >> 1), 0, m.tiles - sy),
  ];
}

// ---------------------------------------------------------------------------------------
// panels
// ---------------------------------------------------------------------------------------

let paletteItems = [];

function initializeSessionPanel() {
  const players = $('session-player');
  const colours = ['blue', 'red', 'green', 'amber'];
  players.replaceChildren();
  for (let i = 0; i < state.mod.playerCount; i++) {
    const option = document.createElement('option');
    option.value = String(i);
    option.textContent = `player ${i}${colours[i] ? ` (${colours[i]})` : ''}`;
    players.appendChild(option);
  }
  players.value = String(state.who);
  $('session-seed').value = formatSeed(state.sessionSeed);
  $('income').value = String(state.sessionIncomeMode);
  $('popset').value = String(state.sessionPopSetting);
  const size = `${state.mod.tiles}x${state.mod.tiles}`;
  const sizeOption = $('session-size').options[0];
  sizeOption.value = size;
  sizeOption.textContent = `${state.mod.tiles} × ${state.mod.tiles} tiles — fixed`;

  $('session-new').addEventListener('click', restartSessionFromPanel);
  $('session-seed').addEventListener('keydown', (event) => {
    if (event.key === 'Enter') restartSessionFromPanel();
  });
  players.addEventListener('change', () => switchPlayer(Number(players.value)));
  $('session-share').addEventListener('click', shareSessionLink);
  syncSessionUrl();
  renderSessionStatus();
  renderSessionSummary();
}

function restartSessionFromPanel() {
  const input = $('session-seed');
  let seed;
  try {
    seed = parseSessionSeed(input.value);
    input.setCustomValidity('');
  } catch (error) {
    input.setCustomValidity(error.message);
    input.reportValidity();
    $('session-status').textContent = error.message;
    say(error.message, 'warn');
    return false;
  }

  if (!state.mod.restart(seed)) {
    const message = 'new game allocation failed; the previous session is still live';
    $('session-status').textContent = message;
    say(message, 'warn');
    return false;
  }
  state.sessionSeed = seed;
  state.selection = [];
  state.groups.clear();
  state.lastGroupKey = { key: null, t: 0 };
  state.buildType = null;
  state.commandMode = null;
  state.drag = null;
  state.panning = null;
  state.edge = null;
  state.idleCursor = 0;
  state.terrainVersion = -1;
  state.mod.setIncomeMode(state.sessionIncomeMode);
  state.mod.setPopSetting(state.sessionPopSetting);
  state.sessionInitialDigest = state.mod.digest();
  miniVersion = -1;
  miniTerrain = null;
  previousGaps = null;
  pings.length = 0;
  acc = 0;
  last = performance.now();
  state.gfx.provision(state.mod.tiles, state.mod.x.game_capacity(state.mod.g));
  const [sx, sy] = state.mod.startOf(state.who);
  centreOn(sx, sy);
  state.cameraSource = `new session: P${state.who}`;
  setPaused(false, false);
  input.value = formatSeed(seed);
  syncSessionUrl();
  renderMenus();
  renderSelection();
  refreshPaletteAvailability();
  renderPaletteFeedback();
  renderSessionStatus();
  renderSessionSummary();
  say(`new session — requested seed ${formatSeed(seed)}, player ${state.who}; ` +
    'seed-dependent map generation remains blocked', 'ok');
  return true;
}

function switchPlayer(player) {
  const next = parseSessionPlayer(player, state.mod.playerCount);
  if (next === state.who) return;
  state.mod.group(state.who, []);
  state.who = next;
  state.selection = [];
  state.mod.group(state.who, []);
  state.buildType = null;
  state.commandMode = null;
  const [sx, sy] = state.mod.startOf(state.who);
  centreOn(sx, sy);
  state.cameraSource = `perspective: P${state.who}`;
  syncSessionUrl();
  renderMenus();
  renderSelection();
  refreshPaletteAvailability();
  renderPaletteFeedback();
  renderSessionStatus();
  renderSessionSummary();
  say(`player perspective changed to ${state.who}`, 'hi');
}

function sessionUrl() {
  const current = new URL(location.href);
  const backend = current.searchParams.get('backend');
  const url = new URL(current.href);
  url.search = '';
  url.hash = '';
  url.searchParams.set('seed', formatSeed(state.sessionSeed));
  url.searchParams.set('player', String(state.who));
  url.searchParams.set('map', SESSION_MAP);
  url.searchParams.set('size', `${state.mod.tiles}x${state.mod.tiles}`);
  url.searchParams.set('nation', 'unavailable');
  url.searchParams.set('team', 'unavailable');
  url.searchParams.set('slots', `${state.mod.playerCount}-manual`);
  url.searchParams.set('ai_slots', 'unavailable');
  url.searchParams.set('ai_difficulty', 'unavailable');
  url.searchParams.set('income', INCOME_MODES[state.sessionIncomeMode].slug);
  url.searchParams.set('population', String(POPULATION_LIMITS[state.sessionPopSetting]));
  url.searchParams.set('victory', 'unavailable');
  if (backend) url.searchParams.set('backend', backend);
  return url;
}

function syncSessionUrl() {
  history.replaceState(null, '', sessionUrl());
}

async function shareSessionLink() {
  const url = sessionUrl().href;
  let copied = false;
  try {
    await navigator.clipboard.writeText(url);
    copied = true;
  } catch {
    const area = document.createElement('textarea');
    area.value = url;
    area.style.cssText = 'position:fixed;left:-10000px;top:0';
    document.body.appendChild(area);
    area.select();
    copied = document.execCommand('copy');
    area.remove();
  }
  if (copied) say('session link copied — the canonical supported and unavailable setup is encoded', 'ok');
  else {
    $('session-status').textContent = `copy unavailable — ${url}`;
    say('clipboard unavailable; session link is shown in the session panel', 'warn');
  }
}

function renderSessionStatus() {
  if (!$('session-status') || !state.mod) return;
  $('session-status').textContent =
    `${formatSeed(state.sessionSeed)} · player ${state.who} · frame ${state.mod.frame} · ` +
    `initial digest ${state.sessionInitialDigest} · seed not yet consumed by world setup`;
}

function sessionDescriptor() {
  const income = INCOME_MODES[state.sessionIncomeMode];
  return Object.freeze({
    seed: formatSeed(state.sessionSeed),
    player: state.who,
    map: SESSION_MAP,
    size: `${state.mod.tiles}x${state.mod.tiles}`,
    nation: 'unavailable',
    team: 'unavailable',
    slots: state.mod.playerCount,
    aiSlots: 'unavailable',
    aiDifficulty: 'unavailable',
    income: income.slug,
    population: POPULATION_LIMITS[state.sessionPopSetting],
    victory: 'unavailable',
  });
}

function renderSessionSummary() {
  if (!$('session-summary') || !state.mod) return;
  const setup = sessionDescriptor();
  $('summary-world').textContent =
    `integration land · ${state.mod.tiles} × ${state.mod.tiles} tiles · fixed`;
  $('summary-player').textContent =
    `P${setup.player} · nation unavailable · team unavailable`;
  $('summary-slots').textContent =
    `${setup.slots} manual command perspectives · AI unavailable`;
  $('summary-rules').textContent =
    `population ${setup.population} · ${INCOME_MODES[state.sessionIncomeMode].label} · victory unavailable`;
}

function initializeObjectivesPanel() {
  const legend = $('owner-legend');
  const players = $('world-players');
  legend.replaceChildren();
  players.replaceChildren();
  for (let p = 0; p < state.mod.playerCount; p++) {
    const key = document.createElement('div');
    key.className = 'owner-key';
    const swatch = document.createElement('span');
    swatch.className = 'owner-swatch';
    swatch.style.background = OWNER_COLOURS[p % OWNER_COLOURS.length];
    const label = document.createElement('span');
    label.id = `owner-key-${p}`;
    key.append(swatch, label);
    legend.appendChild(key);

    const row = document.createElement('div');
    row.className = 'world-player';
    row.dataset.player = String(p);
    const owner = document.createElement('div');
    owner.className = 'owner-id';
    owner.style.color = OWNER_COLOURS[p % OWNER_COLOURS.length];
    owner.textContent = `P${p}`;
    const exported = document.createElement('div');
    exported.className = 'owner-state';
    exported.id = `owner-state-${p}`;
    const focus = document.createElement('button');
    focus.type = 'button';
    focus.dataset.focusPlayer = String(p);
    focus.textContent = 'focus';
    focus.setAttribute('aria-label', `Focus camera on player ${p} start`);
    focus.addEventListener('click', () => focusPlayerStart(p, 'player panel'));
    row.append(owner, exported, focus);
    players.appendChild(row);
  }
  renderObjectivesPanel();
}

function exportedWorldSnapshot() {
  const m = state.mod;
  const owners = Array.from({ length: m.playerCount }, (_, player) => ({
    player,
    objects: 0,
    units: 0,
    buildings: 0,
    foundations: 0,
    ledger: m.player(player),
    relation: player === state.who ? 'local perspective' : 'unavailable',
  }));
  const views = m.views();
  for (let row = 0; row < m.live; row++) {
    const tag = views.tag[row];
    if ((tag & TAG.occupied) === 0) continue;
    const owner = tag & 0xf;
    if (!owners[owner]) continue;
    owners[owner].objects++;
    if (tag & TAG.building) {
      owners[owner].buildings++;
      if (tag & TAG.underConstruction) owners[owner].foundations++;
    } else {
      owners[owner].units++;
    }
  }
  return Object.freeze({
    frame: m.frame,
    elapsedSeconds: m.frame * TICK_MS / 1000,
    visibility: 'omniscient-export',
    diplomacy: 'unavailable',
    victory: 'unavailable',
    score: 'unavailable',
    countdown: 'unavailable',
    owners: Object.freeze(owners.map(Object.freeze)),
  });
}

function cameraSnapshot() {
  const [width, height] = vp();
  const scale = pxPerSub();
  const worldX = state.cam.x + width / scale / 2;
  const worldY = state.cam.y + height / scale / 2;
  return Object.freeze({
    worldX: Math.round(worldX),
    worldY: Math.round(worldY),
    tileX: clamp(Math.floor(worldX / state.mod.subtile), 0, state.mod.tiles - 1),
    tileY: clamp(Math.floor(worldY / state.mod.subtile), 0, state.mod.tiles - 1),
    source: state.cameraSource,
  });
}

function focusPlayerStart(player, source = 'player panel') {
  const p = parseSessionPlayer(player, state.mod.playerCount);
  const [x, y] = state.mod.startOf(p);
  centreOn(x, y);
  state.cameraSource = `${source}: P${p}`;
  renderObjectivesPanel();
  say(`camera focused on P${p} exported start; relation and visibility remain unavailable`, 'hi');
  return cameraSnapshot();
}

function renderObjectivesPanel() {
  if (!$('objectives') || !state.mod) return;
  const snapshot = exportedWorldSnapshot();
  const camera = cameraSnapshot();
  $('objective-time').textContent =
    `frame ${snapshot.frame.toLocaleString()} · elapsed ${snapshot.elapsedSeconds.toFixed(1)} s`;
  $('objective-state').textContent = 'unavailable — no victory/endgame host';
  $('objective-score').textContent = 'unavailable — object counts are not a victory score';
  $('objective-countdown').textContent = 'unavailable — elapsed time only';
  for (const owner of snapshot.owners) {
    const relation = owner.player === state.who ? 'you' : 'relation unavailable';
    const key = $(`owner-key-${owner.player}`);
    if (key) key.textContent = `P${owner.player} ${relation}`;
    const row = document.querySelector(`.world-player[data-player="${owner.player}"]`);
    if (row) row.classList.toggle('you', owner.player === state.who);
    const stateEl = $(`owner-state-${owner.player}`);
    if (stateEl) {
      const objectWord = owner.objects === 1 ? 'object' : 'objects';
      const unitWord = owner.units === 1 ? 'unit' : 'units';
      const buildingWord = owner.buildings === 1 ? 'building' : 'buildings';
      stateEl.textContent =
        `${owner.objects} ${objectWord} · ${owner.units} ${unitWord} · ` +
        `${owner.buildings} ${buildingWord}` +
        (owner.foundations ? ` (${owner.foundations} foundations)` : '') +
        ` · pop ${owner.ledger.pop}/${owner.ledger.popCap} · age ${owner.ledger.age} · ${relation}`;
    }
  }
  $('camera-status').textContent =
    `camera tile ${camera.tileX},${camera.tileY} · ${camera.source} · ` +
    'minimap click/drag navigates; right-click issues a move for the current selection';
  $('mini').setAttribute('aria-label',
    `Omniscient integration minimap, camera at tile ${camera.tileX}, ${camera.tileY}. ` +
    'All exported owners are visible; diplomacy and fog are unavailable.');
}

function wirePanels() {
  for (const id of ['tab-build', 'tab-train', 'tab-research']) {
    $(id).addEventListener('click', () => {
      document.querySelectorAll('.tab').forEach((t) => t.classList.remove('sel'));
      $(id).classList.add('sel');
      if (id !== 'tab-build') state.buildType = null;
      renderMenus();
    });
  }
  $('income').addEventListener('change', (e) => {
    state.sessionIncomeMode = Number(e.target.value) === 1 ? 1 : 0;
    state.mod.setIncomeMode(state.sessionIncomeMode);
    syncSessionUrl();
    renderSessionSummary();
    say(`income mode: ${e.target.selectedOptions[0].textContent}`, 'hi');
  });
  $('popset').addEventListener('change', (e) => {
    state.sessionPopSetting = clamp(Number(e.target.value) | 0, 0, POPULATION_LIMITS.length - 1);
    state.mod.setPopSetting(state.sessionPopSetting);
    syncSessionUrl();
    renderSessionSummary();
    say(`population limit: ${POPULATION_LIMITS[state.sessionPopSetting]}`, 'hi');
  });
  $('speed').addEventListener('input', (e) => setSpeed(Number(e.target.value)));
  $('pause').addEventListener('click', () => setPaused(!state.paused));
  $('halt').addEventListener('click', () => logPacket('HALT', state.mod.halt(state.who)));
  $('palette-filter').addEventListener('input', renderMenus);
  $('cmd-move').addEventListener('click', () => setCommandMode('move'));
  $('cmd-attack').addEventListener('click', () => setCommandMode('attack'));
  $('cmd-gather').addEventListener('click', () => setCommandMode('gather'));
  $('cmd-build').addEventListener('click', () => {
    if (state.buildType !== null) cancelTargeting();
    else openCatalog('build');
  });
  $('cmd-train').addEventListener('click', () => openCatalog('train'));
  $('cmd-halt').addEventListener('click', () => logPacket('HALT', state.mod.halt(state.who)));
  $('cmd-pause').addEventListener('click', () => setPaused(!state.paused));
  $('cmd-zoom-out').addEventListener('click', () => zoomCentre(1 / 1.25));
  $('cmd-zoom-in').addEventListener('click', () => zoomCentre(1.25));
  renderActionDock();
}

function openCatalog(which) {
  const tab = which === 'research' ? $('tab-research')
    : which === 'train' ? $('tab-train') : $('tab-build');
  tab.click();
  if (matchMedia('(max-width:700px)').matches) {
    $('side').scrollIntoView({ behavior: 'smooth', block: 'start' });
  }
}

function zoomCentre(factor) {
  const r = $('gl').getBoundingClientRect();
  zoomAt(r.left + r.width / 2, r.top + r.height / 2, factor);
}

function setPaused(paused, announce = true) {
  state.paused = paused;
  const el = $('pause');
  if (el) el.textContent = paused ? 'resume (P)' : 'pause (P)';
  const dock = $('cmd-pause');
  if (dock) {
    dock.firstElementChild.textContent = paused ? 'resume' : 'pause';
    dock.classList.toggle('active', paused);
    dock.setAttribute('aria-pressed', String(paused));
  }
  if (announce) {
    say(paused ? 'simulation paused — commands remain queued for the next tick' : 'simulation resumed');
  }
}

function setSpeed(speed) {
  state.speed = clamp(Number(speed) || 1, 0.25, 8);
  const el = $('speed');
  if (el && Number(el.value) !== state.speed) el.value = String(state.speed);
}

/** Complete costed building records. Selection and known prerequisites are applied at render. */
function buildPalette() {
  const p = state.play;
  if (!p) return;
  state.buildable = Object.entries(p.buildings)
    .filter(([id, b]) => {
      if (!Number.isFinite(Number(id))) return false;
      if (b.cost.every((c) => c === 0)) return false;
      return true;
    })
    .map(([id, b]) => ({ id: Number(id), ...b }))
    .sort((a, b) => a.age - b.age || a.id - b.id);
}

function paletteMode() {
  return document.querySelector('.tab.sel')?.dataset.mode ?? 'build';
}

function selectedPaletteContext() {
  const m = state.mod;
  const infos = state.selection.map((id) => m.info(id)).filter(Boolean);
  return {
    infos,
    mobiles: infos.filter((info) => !info.isBuilding),
    producers: infos.filter((info) => info.isBuilding && info.buildProgress < 0),
    me: m.player(state.who),
  };
}

function unavailablePaletteItem(name, reason) {
  return { kind: 'unavailable', id: -1, name, reason, cost: [0, 0, 0, 0, 0, 0], jobTime: 0 };
}

function renderMenus() {
  const m = state.mod;
  const mode = paletteMode();
  const host = $('palette');
  host.replaceChildren();
  paletteItems = [];
  const filter = $('palette-filter').value.trim().toLocaleLowerCase();
  const ctx = selectedPaletteContext();

  if (mode === 'build') {
    $('palette-context').textContent = ctx.mobiles.length
      ? `${ctx.mobiles.length} selected non-building object(s) · age ${ctx.me.age} · ` +
        'current core builder predicate is selection-only; builder-type and nation gates are unavailable'
      : 'select a non-building object · builder-type and nation eligibility are not exported';
    if (!state.buildable) {
      paletteItems.push(unavailablePaletteItem('Building catalog unavailable', 'packed play data missing'));
    } else if (!ctx.mobiles.length) {
      paletteItems.push(unavailablePaletteItem('Building placement unavailable', 'select a non-building object'));
    } else {
      for (const b of state.buildable) paletteItems.push({ kind: 'build', ...b });
    }
  } else if (mode === 'train') {
    const producerNames = [...new Set(ctx.producers.map((p) => typeName(p.typeId)))];
    $('palette-context').textContent = ctx.producers.length
      ? `${ctx.producers.length} completed producer(s): ${producerNames.join(', ')} · ` +
        'exact WHERE edges; nation-specific eligibility is unavailable'
      : 'select a completed producer · products come only from exported WHERE edges';
    const seen = new Set();
    for (const b of ctx.producers) {
      for (const t of m.products(b.typeId)) {
        if (seen.has(t)) continue;
        seen.add(t);
        const u = state.play?.units?.[String(t)];
        if (u) paletteItems.push({ kind: 'train', id: t, producerType: b.typeId, ...u });
      }
    }
    if (!ctx.producers.length) {
      paletteItems.push(unavailablePaletteItem('Training unavailable', 'select a completed producer'));
    } else if (!paletteItems.length) {
      paletteItems.push(unavailablePaletteItem('Training unavailable', 'selected producer has no exported WHERE edges'));
    }
  } else {
    $('palette-context').textContent =
      `player age ${ctx.me.age} · only the global age command is implemented; ` +
      'library, ordinary technology, and prerequisite hosts are unavailable';
    const age = state.play?.ages?.[ctx.me.age];
    if (age && ctx.me.age < 7) {
      paletteItems.push({
        kind: 'research', id: age.id, name: age.name, cost: age.cost,
        age: ctx.me.age, jobTime: 600,
      });
    } else {
      paletteItems.push(unavailablePaletteItem('Age research complete', 'no later exported age record'));
    }
    paletteItems.push(unavailablePaletteItem(
      'Other technologies unavailable', 'no exported tech catalog or executable prerequisite path'));
  }

  if (filter) {
    paletteItems = paletteItems.filter((it) =>
      it.name.toLocaleLowerCase().includes(filter) || String(it.id).includes(filter));
  }
  if (!paletteItems.length && !host.childElementCount) {
    host.innerHTML = '<div class="hint">no catalog entries match this filter</div>';
  }
  paletteItems.forEach((it, i) => {
    const el = document.createElement('button');
    el.className = 'pal';
    el.dataset.paletteIndex = String(i);
    el.dataset.kind = it.kind;
    el.dataset.typeId = String(it.id);
    if (state.buildType === it.id && it.kind === 'build') el.classList.add('armed');
    const key = i < 8 ? 'QWERTYUI'[i] : '';
    el.innerHTML =
      `<span class="k">${key}</span><span class="n">${it.name}</span>` +
      `<span class="c">${costText(it.cost)}${Number.isInteger(it.age) && it.age >= 0 ? ` · age ${it.age}` : ''}</span>` +
      '<span class="why"></span>';
    el.addEventListener('click', () => activate(it));
    host.appendChild(el);
  });
  refreshPaletteAvailability(ctx);
  renderPaletteFeedback(ctx);
  renderActionDock();
}

function canAfford(cost, stock) {
  return cost.every((amount, resource) => stock[resource] >= amount);
}

function missingCost(cost, stock) {
  const missing = [];
  for (let i = 0; i < RES_NAMES.length; i++) {
    if (cost[i] > stock[i]) missing.push(`${cost[i] - stock[i]} ${RES_NAMES[i]}`);
  }
  return missing.join(', ');
}

function paletteGate(it, ctx = selectedPaletteContext()) {
  if (it.kind === 'unavailable') return { enabled: false, reasons: [it.reason], detail: '' };
  const reasons = [];
  if (it.kind === 'build') {
    if (!ctx.mobiles.length) reasons.push('select a non-building object');
    if (it.age > ctx.me.age) reasons.push(`requires age ${it.age}`);
  } else if (it.kind === 'train') {
    const producers = ctx.producers.filter((p) => p.typeId === it.producerType);
    if (!producers.length) reasons.push(`select ${typeName(it.producerType)}`);
    else if (producers.every((p) => p.queueN >= QUEUE_CAPACITY)) reasons.push('producer queue full');
    if (it.age > ctx.me.age) reasons.push(`requires age ${it.age}`);
    if (ctx.me.pop + Math.max(0, it.pop) > ctx.me.popCap) reasons.push('population capped');
  } else if (it.kind === 'research') {
    if (ctx.me.research > 0) reasons.push('age research already active');
    if (state.paletteNotice.includes('research packet submitted') && state.mod.transport().pending > 0) {
      reasons.push('research command pending');
    }
  }
  if (!canAfford(it.cost, ctx.me.stock)) reasons.push(`needs ${missingCost(it.cost, ctx.me.stock)}`);

  let detail = '';
  if (it.kind === 'build') detail = `footprint ${it.xSize}×${it.ySize} · ${it.jobTime} frames`;
  if (it.kind === 'train') {
    const queues = ctx.producers.filter((p) => p.typeId === it.producerType).map((p) => p.queueN);
    detail = `${typeName(it.producerType)} · queue ${queues.length ? Math.min(...queues) : 0}/${QUEUE_CAPACITY}`;
  }
  if (it.kind === 'research') detail = 'global age command · 600-frame integration duration';
  return { enabled: reasons.length === 0, reasons, detail };
}

function refreshPaletteAvailability(ctx = selectedPaletteContext()) {
  const buttons = $('palette').querySelectorAll('[data-palette-index]');
  for (const button of buttons) {
    const it = paletteItems[Number(button.dataset.paletteIndex)];
    if (!it) continue;
    const gate = paletteGate(it, ctx);
    button.disabled = !gate.enabled;
    button.classList.toggle('poor', !canAfford(it.cost, ctx.me.stock));
    const why = button.querySelector('.why');
    const message = gate.enabled ? gate.detail : gate.reasons.join(' · ');
    if (why.textContent !== message) why.textContent = message;
    why.classList.toggle('ready', gate.enabled);
    button.title = `${it.name} — ${gate.enabled ? gate.detail : message}; ` +
      `cost ${costText(it.cost, true)}` +
      (it.jobTime ? `; ${it.jobTime} frames (${(it.jobTime * TICK_MS / 1000).toFixed(1)} s)` : '');
  }
}

function paletteActivate(slot) {
  if (slot < paletteItems.length) activate(paletteItems[slot]);
}

function activate(it) {
  const m = state.mod;
  const gate = paletteGate(it);
  if (!gate.enabled) {
    const message = `${it.name} unavailable — ${gate.reasons.join(' · ')}`;
    state.paletteNotice = message;
    say(message, 'warn');
    renderPaletteFeedback();
    return false;
  }
  if (it.kind === 'build') {
    state.commandMode = null;
    state.buildType = state.buildType === it.id ? null : it.id;
    say(state.buildType ? `place ${it.name} — click the map, Esc cancels` : 'build cancelled');
    renderMenus();
    if (state.buildType !== null && matchMedia('(max-width:700px)').matches) {
      $('stage').scrollIntoView({ behavior: 'smooth', block: 'start' });
    }
  } else if (it.kind === 'train') {
    state.paletteNotice = `${it.name} queue packet submitted; applies at the next tick`;
    logPacket('QUEUE_UP', m.queueUp(state.who, it.id, 1));
  } else if (it.kind === 'research') {
    state.paletteNotice = `${it.name} research packet submitted; applies at the next tick`;
    logPacket('QUEUE_UP(age)', m.queueUp(state.who, it.id, 1));
  }
  renderPaletteFeedback();
  return true;
}

function renderPaletteFeedback(ctx = selectedPaletteContext()) {
  const host = $('palette-feedback');
  if (!host) return;
  const pending = state.mod.transport().pending;
  const queues = ctx.producers
    .filter((p) => p.queueN > 0)
    .map((p) => `${typeName(p.typeId)} ${p.queue.map(typeName).join(' → ')} (${p.queueN}/${QUEUE_CAPACITY})`);
  let message = '';
  let cls = '';
  if (pending > 0 && state.paletteNotice) {
    message = `${state.paletteNotice} · ${pending} packet(s) pending`;
  } else if (queues.length) {
    message = `live queue · ${queues.join(' · ')} · cancel the last item in Selection`;
    cls = 'ok';
  } else if (ctx.me.research > 0) {
    message = `age research active · ${Math.min(100, Math.floor(ctx.me.research / 6))}%`;
    cls = 'ok';
  } else if (state.paletteNotice) {
    message = `${state.paletteNotice} · command drained`;
    cls = state.paletteNotice.includes('unavailable') ? 'warn' : '';
  } else {
    message = paletteMode() === 'research'
      ? 'no age research active' : 'no selected producer queue';
  }
  if (host.textContent !== message) host.textContent = message;
  host.className = cls;
}

function costText(cost, verbose) {
  const parts = [];
  for (let i = 0; i < 6; i++) if (cost[i]) parts.push(`${cost[i]}${verbose ? ' ' + RES_NAMES[i] : RES_NAMES[i][0]}`);
  return parts.join(' ') || 'free';
}

function typeName(id) {
  return state.play?.units?.[String(id)]?.name
    ?? state.play?.buildings?.[String(id)]?.name
    ?? `type ${id}`;
}

function showInspect(info) {
  if (!info) return;
  $('inspect').textContent =
    `${typeName(info.typeId)}  (player ${info.owner})\n` +
    `hits ${info.hits}/${info.maxHits}   attack x10 ${info.attack}   armor ${info.armor}\n` +
    `range ${info.range}   moves ${info.moves}   recharge ${info.recharge}`;
}

// ---------------------------------------------------------------------------------------
// HUD
// ---------------------------------------------------------------------------------------

function renderReadinessStatic() {
  const registry = state.mod.readiness();
  const replay = REPLAY_EVIDENCE;
  $('readiness-head').textContent =
    'Not Fidelity mode: independent GPL integration build; this page is not running the Arena.';

  const runtime = $('readiness-runtime');
  runtime.textContent =
    'web::wasm::game::GameWorld — local integration composition; don_ai::arena::World connected: no';
  runtime.className = 'rv bad';

  const gate = $('readiness-gate');
  gate.textContent = registry.ready
    ? 'READY — no compiled blockers'
    : `BLOCKED — ${registry.blockers.length} compiled known drifts`;
  gate.className = `rv ${registry.ready ? 'ok' : 'bad'}`;
  gate.title = registry.source;

  const first = replay.world.firstDivergenceTurns.join(', ') || 'not observed';
  const replayEl = $('readiness-replay');
  replayEl.textContent =
    `${replay.prefixesParsed}/${replay.files} prefixes parsed · ` +
    `${replay.world.nontrivial.toLocaleString()}/${replay.world.comparisons.toLocaleString()} ` +
    `world walks non-empty · ${replay.world.matches.toLocaleString()} matches · ` +
    `first divergence turn ${first}`;
  replayEl.className = `rv ${replay.world.matches === replay.world.comparisons ? 'ok' : 'bad'}`;
  replayEl.title = `${replay.source}; ${replay.world.unsourcedBytes.min.toLocaleString()}–` +
    `${replay.world.unsourcedBytes.max.toLocaleString()} walked bytes remain unsourced`;

  const list = $('readiness-blockers');
  list.replaceChildren();
  const local = document.createElement('li');
  const localSlug = document.createElement('code');
  localSlug.textContent = 'web-gameworld-not-arena';
  local.append(localSlug, document.createTextNode(
    ' — command packets terminate in the standalone web GameWorld, not don-ai Arena'));
  list.appendChild(local);
  for (const blocker of registry.blockers) {
    const li = document.createElement('li');
    const slug = document.createElement('code');
    slug.textContent = blocker.slug;
    li.append(slug, document.createTextNode(` — ${blocker.title}`));
    list.appendChild(li);
  }
  $('readiness-blocker-summary').textContent =
    `${registry.blockers.length} compiled blockers + 1 browser runtime boundary`;
  renderTransport();
}

function renderTransport() {
  const t = state.mod.transport();
  const gaps = state.mod.gaps().reduce((a, b) => a + b, 0);
  $('readiness-transport').textContent =
    `${t.submitted.toLocaleString()} submitted · ${t.drained.toLocaleString()} drained at ticks · ` +
    `${t.ordersApplied.toLocaleString()} object orders applied · ${t.pending.toLocaleString()} pending · ` +
    `${gaps.toLocaleString()} fail-closed gaps`;
}

function renderHud() {
  const m = state.mod;
  if (state.gfx.errors.length > state.rendererErrorCount) {
    const latest = state.gfx.errors.slice(state.rendererErrorCount).join(' | ');
    state.rendererErrorCount = state.gfx.errors.length;
    badge('be', `${state.gfx.kind}: renderer error`, 'bad');
    say(`renderer error — ${latest}`, 'warn');
  }
  const me = m.player(state.who);
  const bar = $('res');
  if (!bar.childElementCount) {
    for (const r of RES_NAMES) {
      const d = document.createElement('div');
      d.className = 'r';
      d.innerHTML = `<span class="rn">${r}</span><span class="rv" id="rv-${r}">0</span>` +
        `<span class="ri" id="ri-${r}">+0</span>`;
      bar.appendChild(d);
    }
  }
  for (let i = 0; i < 6; i++) {
    $(`rv-${RES_NAMES[i]}`).textContent = me.stock[i];
    const ri = $(`ri-${RES_NAMES[i]}`);
    const capped = me.gross[i] > me.cap[i];
    ri.textContent = `+${me.income[i]}${capped ? '↑' : ''}`;
    ri.className = 'ri' + (capped ? ' capped' : '');
    ri.title = capped
      ? `gross ${me.gross[i]} clamped to COMMERCE_CAP[age ${me.age}] = ${me.cap[i]} ` +
        `(Leader::do_gather step 3). Workers past this point buy nothing with the retail cap step.`
      : `gross ${me.gross[i]}, cap ${me.cap[i]}, ${me.workers[i]} worker(s)`;
  }
  const ageName = me.age === 0 ? 'Ancient Age'
    : (state.play?.ages?.[me.age - 1]?.name ?? `age ${me.age}`);
  $('meta').textContent =
    `P${state.who}   ${formatSeed(state.sessionSeed)}   pop ${me.pop}/${me.popCap}   ${ageName}` +
    (me.research ? `   researching ${(me.research / 600 * 100) | 0}%` : '');

  const stat = $('stat');
  stat.textContent =
    `${state.fps.toFixed(0)} fps   ${m.live} objects   frame ${m.frame} ` +
    `(${(m.frame * TICK_MS / 1000).toFixed(0)} s)   ` +
    `step ${state.stepMs.toFixed(2)} ms   upload ${state.uploadMs.toFixed(2)} ms   ` +
    `x${state.speed}${state.paused ? '  PAUSED' : ''}`;

  renderSelection();
  refreshPaletteAvailability();
  renderPaletteFeedback();
  renderSessionStatus();
  renderObjectivesPanel();
  renderCoverage();
  renderTransport();
}

function renderSelection() {
  const m = state.mod;
  const host = $('sel');
  if (!state.selection.length) {
    host.innerHTML = '<div class="hint">nothing selected</div>';
    renderActionDock();
    return;
  }
  const infos = state.selection.map((id) => m.info(id)).filter(Boolean);
  if (infos.length !== state.selection.length) state.selection = infos.map((i) => i.id);
  if (!infos.length) {
    host.innerHTML = '<div class="hint">selection is gone</div>';
    renderActionDock();
    return;
  }

  if (infos.length === 1) {
    const i = infos[0];
    const orderNames = ['idle', 'move', 'attack', 'gather', 'build'];
    let extra = '';
    if (i.isBuilding && i.buildProgress >= 0) {
      extra = `\nunder construction ${i.buildProgress}/${i.jobTime}`;
    } else if (i.queueN) {
      extra = `\nqueue: ${i.queue.map(typeName).join(', ')}`;
    }
    if (i.gatherRes >= 0) extra += `\nworking ${RES_NAMES[i.gatherRes]}`;
    host.innerHTML =
      `<div class="big">${typeName(i.typeId)}</div>` +
      `<pre>hits      ${i.hits}/${i.maxHits}\n` +
      `attack    ${i.attack} (x10)\n` +
      `armor     ${i.armor}\n` +
      `range     ${i.range} tiles\n` +
      `moves     ${i.moves} subtiles/frame\n` +
      `recharge  ${i.recharge} frames\n` +
      `pop       ${i.pop < 0 ? 'unresolved' : i.pop}\n` +
      `order     ${orderNames[i.order] ?? i.order}${extra}</pre>` +
      (i.queueN ? `<button id="cancelq">cancel last queued</button>` : '');
    const cq = $('cancelq');
    if (cq) cq.addEventListener('click', () => {
      state.paletteNotice = 'cancel-last queue packet submitted; applies at the next tick';
      logPacket('UNQUEUE', m.unqueue(state.who, -1));
      renderPaletteFeedback();
    });
  } else {
    const byType = new Map();
    for (const i of infos) byType.set(i.typeId, (byType.get(i.typeId) ?? 0) + 1);
    host.innerHTML = `<div class="big">${infos.length} selected</div>` +
      [...byType].map(([t, n]) => `<div class="row"><span>${typeName(t)}</span><b>${n}</b></div>`).join('');
  }
  renderActionDock();
}

function renderActionDock() {
  const selected = state.selection.length;
  const infos = state.selection.map((id) => state.mod.info(id)).filter(Boolean);
  const hasMobile = infos.some((info) => !info.isBuilding);
  const hasProducer = infos.some((info) => info.isBuilding && info.buildProgress < 0 &&
    Array.isArray(state.play?.edges?.[String(info.typeId)]));
  for (const mode of ['move', 'attack', 'gather']) {
    const el = $(`cmd-${mode}`);
    if (!el) continue;
    const active = state.commandMode === mode;
    el.classList.toggle('active', active);
    el.setAttribute('aria-pressed', String(active));
    el.disabled = selected === 0;
  }
  for (const id of ['cmd-halt']) if ($(id)) $(id).disabled = selected === 0;
  const build = $('cmd-build');
  if (build) {
    const active = state.buildType !== null;
    build.classList.toggle('active', active);
    build.setAttribute('aria-pressed', String(active));
    build.disabled = !hasMobile;
  }
  const train = $('cmd-train');
  if (train) train.disabled = !hasProducer;
  const ds = $('dock-state');
  if (ds) ds.innerHTML = selected ? `<b>${selected}</b><br>selected` : 'no<br>selection';
}

let coverageEl = null;
let previousGaps = null;
function renderCoverage() {
  const m = state.mod;
  const gaps = m.gaps();
  const rows = gaps.map((v, i) => (v ? `${GAP_NAMES[i]}: ${v}` : null)).filter(Boolean);
  const el = $('coverage');
  const txt = rows.length
    ? 'unexecuted command counters\n' + rows.join('\n')
    : 'unexecuted command counters\n(none yet)';
  if (txt !== coverageEl) { el.textContent = txt; coverageEl = txt; }
  if (previousGaps) {
    for (let i = 0; i < gaps.length; i++) {
      const delta = gaps[i] - previousGaps[i];
      if (delta > 0) {
        say(`command not executed — ${GAP_NAMES[i]}${delta > 1 ? ` (×${delta})` : ''}`, 'warn');
      }
    }
  }
  previousGaps = gaps;
}

// ---------------------------------------------------------------------------------------
// overlay + minimap
// ---------------------------------------------------------------------------------------

const pings = [];
function ping(w, colour) { pings.push({ x: w[0], y: w[1], t: performance.now(), colour }); }

function drawOverlay() {
  const c = $('ov');
  const g = c.getContext('2d');
  const r = c.getBoundingClientRect();
  if (c.width !== Math.round(r.width) || c.height !== Math.round(r.height)) {
    c.width = Math.round(r.width); c.height = Math.round(r.height);
  }
  g.clearRect(0, 0, c.width, c.height);
  const m = state.mod;

  if (state.commandMode !== null) {
    const [tx, ty] = state.hoverTile;
    const [sx, sy] = worldToScreen(tx * m.subtile, ty * m.subtile);
    const px = state.cam.tilePx / ($('gl').width / c.width);
    const colours = { move: '#5ab7ff', attack: '#ff6b5b', gather: '#6ee7a8' };
    g.strokeStyle = colours[state.commandMode];
    g.lineWidth = 2;
    g.beginPath();
    g.arc(sx + px / 2, sy + px / 2, Math.max(7, px * .38), 0, Math.PI * 2);
    g.moveTo(sx + px * .18, sy + px / 2); g.lineTo(sx + px * .82, sy + px / 2);
    g.moveTo(sx + px / 2, sy + px * .18); g.lineTo(sx + px / 2, sy + px * .82);
    g.stroke();
  }

  // footprint preview, tinted by the engine's own grade
  if (state.buildType !== null) {
    const b = state.play?.buildings?.[String(state.buildType)];
    const w = [state.hoverTile[0] * m.subtile, state.hoverTile[1] * m.subtile];
    const [tx, ty] = anchorFor(w, b);
    const grade = m.placementGrade(state.who, state.buildType, tx, ty);
    const [sx, sy] = worldToScreen(tx * m.subtile, ty * m.subtile);
    const px = state.cam.tilePx / (c.width ? ($('gl').width / c.width) : 1);
    const wpx = (b ? b.xSize : 2) * px, hpx = (b ? b.ySize : 2) * px;
    const fill = grade === 0 ? 'rgba(255,80,70,0.30)'
      : grade === 2 ? 'rgba(255,180,84,0.28)' : 'rgba(110,231,168,0.28)';
    const line = grade === 0 ? '#ff5b4d' : grade === 2 ? '#ffb454' : '#6ee7a8';
    g.fillStyle = fill; g.fillRect(sx, sy, wpx, hpx);
    g.strokeStyle = line; g.lineWidth = 2; g.strokeRect(sx, sy, wpx, hpx);
    g.fillStyle = line; g.font = '11px ui-monospace, monospace';
    const names = { 0: 'CORE_BLOCKED', 2: 'PARTIAL', 3: 'APPROACH_CLEAR', 4: 'FULLY_CLEAR' };
    g.fillText(`${b ? b.name : state.buildType} — space_at_corner: ${names[grade] ?? grade}`,
      sx, sy - 6);
  }

  // gatherable tile hint under the cursor
  if (state.buildType === null) {
    const [tx, ty] = state.hoverTile;
    const res = m.tileResource(tx, ty);
    if (res >= 0) {
      const [sx, sy] = worldToScreen(tx * m.subtile, ty * m.subtile);
      const px = state.cam.tilePx / ($('gl').width / c.width);
      g.strokeStyle = '#6ee7a8'; g.lineWidth = 2;
      g.strokeRect(sx, sy, px, px);
      g.fillStyle = '#6ee7a8'; g.font = '11px ui-monospace, monospace';
      g.fillText(RES_NAMES[res], sx, sy - 4);
    }
  }

  // drag box
  if (state.drag) {
    const a = worldToScreen(state.drag.x0, state.drag.y0);
    const b = worldToScreen(state.drag.x1, state.drag.y1);
    g.strokeStyle = '#e7e9f0'; g.lineWidth = 1;
    g.setLineDash([4, 3]);
    g.strokeRect(Math.min(a[0], b[0]), Math.min(a[1], b[1]),
      Math.abs(b[0] - a[0]), Math.abs(b[1] - a[1]));
    g.setLineDash([]);
    g.fillStyle = 'rgba(231,233,240,0.08)';
    g.fillRect(Math.min(a[0], b[0]), Math.min(a[1], b[1]),
      Math.abs(b[0] - a[0]), Math.abs(b[1] - a[1]));
  }

  // order pings
  const now = performance.now();
  for (let i = pings.length - 1; i >= 0; i--) {
    const p = pings[i];
    const age = (now - p.t) / 600;
    if (age > 1) { pings.splice(i, 1); continue; }
    const [sx, sy] = worldToScreen(p.x, p.y);
    g.strokeStyle = p.colour;
    g.globalAlpha = 1 - age;
    g.lineWidth = 2;
    g.beginPath(); g.arc(sx, sy, 4 + age * 18, 0, 6.2832); g.stroke();
  }
  g.globalAlpha = 1;
}

let miniTerrain = null, miniVersion = -1;
function drawMinimap() {
  const m = state.mod;
  const c = $('mini');
  const g = c.getContext('2d');
  const n = m.tiles;
  if (miniVersion !== m.terrainVersion) {
    miniVersion = m.terrainVersion;
    if (!miniTerrain) miniTerrain = makeCanvasSurface(n, n);
    const tg = miniTerrain.getContext('2d');
    const img = tg.createImageData(n, n);
    const t = m.views().tiles;
    for (let i = 0; i < n * n; i++) {
      const mask = t[i], blocker = mask & 3, surface = mask & 0x30;
      let r = 60, gg = 78, b = 45;
      if (surface === 0x20) { r = 20; gg = 44; b = 80; }
      else if (surface === 0x30) { r = 24; gg = 62; b = 31; }
      if (blocker === 2) { r = 96; gg = 92; b = 88; }
      else if (blocker === 3) { r = 40; gg = 38; b = 46; }
      img.data[i * 4] = r; img.data[i * 4 + 1] = gg; img.data[i * 4 + 2] = b;
      img.data[i * 4 + 3] = 255;
    }
    tg.putImageData(img, 0, 0);
  }
  g.imageSmoothingEnabled = false;
  g.drawImage(miniTerrain, 0, 0, c.width, c.height);

  const v = m.views();
  const live = m.live;
  const s = c.width / m.span;
  for (let i = 0; i < live; i++) {
    const tag = v.tag[i];
    if ((tag & 0x80000000) === 0) continue;
    g.fillStyle = OWNER_COLOURS[(tag & 0xf) % OWNER_COLOURS.length];
    const b = (tag >>> 29) & 1;
    g.fillRect(v.x[i] * s - (b ? 1.5 : 0.5), v.y[i] * s - (b ? 1.5 : 0.5), b ? 3 : 1.5, b ? 3 : 1.5);
  }
  // Start positions are real exported coordinates. The labels carry only owner identity;
  // they deliberately do not imply ally/enemy relations the ABI cannot provide.
  g.font = '10px ui-monospace, monospace';
  g.textAlign = 'center';
  for (let p = 0; p < m.playerCount; p++) {
    const [x, y] = m.startOf(p);
    const sx = x * s, sy = y * s;
    g.strokeStyle = OWNER_COLOURS[p % OWNER_COLOURS.length];
    g.lineWidth = p === state.who ? 2 : 1;
    g.strokeRect(sx - 4, sy - 4, 8, 8);
    g.fillStyle = OWNER_COLOURS[p % OWNER_COLOURS.length];
    g.fillText(`P${p}`, sx, sy - 7);
  }
  g.textAlign = 'start';
  // viewport rectangle
  const [w, h] = vp();
  const p = pxPerSub();
  g.strokeStyle = '#e7e9f0'; g.lineWidth = 1;
  g.strokeRect(state.cam.x * s, state.cam.y * s, (w / p) * s, (h / p) * s);
}

function makeCanvasSurface(width, height) {
  if (typeof OffscreenCanvas !== 'undefined') return new OffscreenCanvas(width, height);
  const c = document.createElement('canvas');
  c.width = width;
  c.height = height;
  return c;
}

// ---------------------------------------------------------------------------------------
// frame loop
// ---------------------------------------------------------------------------------------

function resize() {
  const c = $('gl');
  const r = c.getBoundingClientRect();
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  c.width = Math.max(1, Math.round(r.width * dpr));
  c.height = Math.max(1, Math.round(r.height * dpr));
  if (state.gfx && state.gfx.kind === 'webgpu' && state.gfx.ctx) {
    state.gfx.ctx.configure({ device: state.gfx.device, format: state.gfx.format, alphaMode: 'opaque' });
  }
}

let acc = 0, last = performance.now(), fpsAcc = 0, fpsN = 0, hudAt = 0;

function frame(now) {
  const dt = Math.min(now - last, 200);
  last = now;
  fpsAcc += dt; fpsN++;
  if (fpsAcc > 400) { state.fps = 1000 * fpsN / fpsAcc; fpsAcc = 0; fpsN = 0; }

  const m = state.mod;

  // keyboard + edge pan
  const panSpeed = 900 / state.cam.tilePx * m.subtile * (dt / 1000);
  let dx = 0, dy = 0;
  if (keys.has('KeyA') && !keys.has('ControlLeft') && !keys.has('MetaLeft')) dx -= 1;
  if (keys.has('KeyD')) dx += 1;
  if (keys.has('KeyW')) dy -= 1;
  if (keys.has('KeyS')) dy += 1;
  if (keys.has('ArrowLeft')) dx -= 1;
  if (keys.has('ArrowRight')) dx += 1;
  if (keys.has('ArrowUp')) dy -= 1;
  if (keys.has('ArrowDown')) dy += 1;
  if (state.edge && !state.drag) { dx += state.edge.x; dy += state.edge.y; }
  if (dx || dy) {
    state.cam.x += dx * panSpeed;
    state.cam.y += dy * panSpeed;
    state.cameraSource = 'keyboard/edge pan';
    clampCam();
  }

  // simulation at the engine's own 67 ms tick, scaled by the speed control
  if (!state.paused) {
    acc += dt * state.speed;
    let steps = 0;
    const t0 = performance.now();
    while (acc >= TICK_MS && steps < 16) { m.step(1); acc -= TICK_MS; steps++; }
    if (steps) state.stepMs = (performance.now() - t0) / steps;
  }

  if (state.terrainVersion !== m.terrainVersion) {
    state.terrainVersion = m.terrainVersion;
    state.gfx.uploadTerrain(m.views().tiles);
  }

  const t1 = performance.now();
  state.uploadMs = state.gfx.draw(m.views(), m.live, camPack(), vp());
  state.drawMs = performance.now() - t1;

  drawOverlay();
  if (now - hudAt > 100) { hudAt = now; renderHud(); drawMinimap(); }

  requestAnimationFrame(frame);
}

// ---------------------------------------------------------------------------------------
// log
// ---------------------------------------------------------------------------------------

function say(msg, cls = '') {
  state.log.unshift(`<span class="${cls}">${msg}</span>`);
  state.log.length = Math.min(state.log.length, 60);
  $('log').innerHTML = state.log.join('\n');
  const toast = $('toast');
  if (toast) {
    toast.textContent = msg;
    toast.className = `show ${cls}`;
    clearTimeout(state.toastTimer);
    state.toastTimer = setTimeout(() => { toast.className = ''; }, cls === 'warn' ? 3600 : 2200);
  }
}

function logPacket(label, bytes) {
  if (!bytes) return;
  const d = decode(bytes);
  const hex = Array.from(bytes.subarray(0, 12))
    .map((b) => b.toString(16).padStart(2, '0')).join(' ');
  say(`${label} — 0x${d.op.toString(16).padStart(2, '0')} ${d.struct} ${bytes.length} B  ${hex}${bytes.length > 12 ? ' …' : ''}`);
}

function badge(id, text, cls) {
  const el = $(id);
  if (!el) return;
  el.textContent = text;
  el.className = 'badge ' + cls;
}

// Automation surface, so the smoke test drives exactly what a human drives.
window.don = {
  state,
  select: selectIds,
  order: { move: issueMove, build: placeBuilding },
  session: {
    restart(seed) {
      $('session-seed').value = formatSeed(parseSessionSeed(seed));
      return restartSessionFromPanel();
    },
    player(player) {
      const select = $('session-player');
      select.value = String(parseSessionPlayer(player, state.mod.playerCount));
      switchPlayer(Number(select.value));
      return state.who;
    },
    url: () => sessionUrl().href,
    setup: () => sessionDescriptor(),
  },
  objectives: {
    snapshot: () => exportedWorldSnapshot(),
    camera: () => cameraSnapshot(),
    focusPlayer: (player) => focusPlayerStart(player, 'automation/player panel'),
  },
  activate,
  info: (id) => state.mod.info(id),
  player: (p = 0) => state.mod.player(p),
  gaps: () => state.mod.gaps(),
  readiness: () => ({
    runtime: 'web::wasm::game::GameWorld',
    arenaConnected: false,
    registry: state.mod.readiness(),
    replay: REPLAY_EVIDENCE,
    transport: state.mod.transport(),
  }),
  digest: () => state.mod.digest(),
  // The only capture that tells the truth about a GPU canvas — see gfx.js `readback`.
  snapshot: () => state.gfx.readback(state.mod.views(), state.mod.live, camPack()),
  backend: () => ({ kind: state.gfx.kind, errors: state.gfx.errors }),
  ready: () => !!(state.mod && state.gfx),
  bootError: null,
  stats: () => ({
    fps: state.fps, live: state.mod.live, frame: state.mod.frame,
    stepMs: state.stepMs, uploadMs: state.uploadMs, drawMs: state.drawMs,
    backend: state.gfx.kind, backendErrors: state.gfx.errors.slice(),
    hasGameData: state.mod.hasGameData, hasPlayData: state.mod.hasPlayData,
    sessionSeed: state.sessionSeed, playerPerspective: state.who,
    sessionSetup: sessionDescriptor(),
    objectives: exportedWorldSnapshot(), camera: cameraSnapshot(),
    selection: state.selection.length, digest: state.mod.digest(),
    gaps: state.mod.gaps(), player: state.mod.player(state.who),
    transport: state.mod.transport(),
    playableBlockers: state.mod.readiness().blockers.length,
    arenaConnected: false,
    paletteSize: paletteItems.length,
  }),
  centreOn,
  screenToWorld,
  worldToScreen,

  /**
   * Create a *fresh* game on the same staged tables, run it headless for `frames`, and
   * return its digest. The point is the comparison: `playcheck digest <seed> <frames>`
   * prints the same quantity from a native aarch64 build of the identical Rust, and the
   * two must agree bit for bit. It is a check that can fail.
   */
  freshDigest(seed = 0xc0ffee, frames = 600) {
    const x = state.mod.x;
    const g = x.game_create(seed >>> 0, 0);
    x.game_step(g, frames);
    const lo = x.game_digest_lo(g) >>> 0, hi = x.game_digest_hi(g) >>> 0;
    const live = x.game_live(g);
    x.game_destroy(g);
    // Creating a second game can grow linear memory, which detaches every view.
    state.mod._buf = null;
    return { seed, frames, live, digest: hi.toString(16).padStart(8, '0') + lo.toString(16).padStart(8, '0') };
  },
  key: (code, modifiers = {}) => onKeyDown({
    code,
    key: code.replace('Key', ''),
    shiftKey: !!modifiers.shiftKey,
    ctrlKey: !!modifiers.ctrlKey,
    metaKey: !!modifiers.metaKey,
    preventDefault() {},
    target: {},
  }),

  /**
   * Fill the world for a frame-rate measurement, then report the count actually live.
   * Marked on every result it feeds, because these units were not paid for.
   */
  stress(n, typeId = 228) {
    let made = 0;
    for (let p = 0; p < state.mod.playerCount; p++) {
      made += state.mod.debugSpawn(p, typeId, Math.ceil(n / state.mod.playerCount));
    }
    state.stressed = true;
    return { requested: n, made, live: state.mod.live };
  },

  /**
   * Uncapped frame loop: step + upload + draw + overlay as fast as the machine allows for
   * `ms`, with no `requestAnimationFrame` in the way. rAF alone cannot measure headroom —
   * it reports the display's refresh rate and calls it a result.
   */
  async bench(ms = 2000, { sim = true } = {}) {
    const m = state.mod;
    const t0 = performance.now();
    let frames = 0, step = 0, upload = 0, draw = 0;
    while (performance.now() - t0 < ms) {
      if (sim) { const a = performance.now(); m.step(1); step += performance.now() - a; }
      const b = performance.now();
      upload += state.gfx.draw(m.views(), m.live, camPack(), vp());
      draw += performance.now() - b;
      frames++;
    }
    const total = performance.now() - t0;
    return {
      frames, ms: total, fps: 1000 * frames / total, live: m.live,
      stepMs: step / frames, uploadMs: upload / frames, drawMs: draw / frames,
      backend: state.gfx.kind, stressed: !!state.stressed,
      viewport: vp(), tilePx: state.cam.tilePx,
    };
  },
};

boot().catch((e) => {
  window.don.bootError = `${e.message}\n${e.stack}`;
  console.error(e);
  const pre = document.createElement('pre');
  pre.style.cssText = 'position:fixed;z-index:20;inset:12px;overflow:auto;color:#ff6b5b;' +
    'padding:16px;border:1px solid #5c2d2d;background:#140d0d;font:12px monospace';
  pre.textContent = `playable client failed to start\n\n${e.message}\n\n${e.stack}`;
  document.body.prepend(pre);
  if ($('stat')) $('stat').textContent = 'boot failed';
});
