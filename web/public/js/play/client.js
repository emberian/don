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

import { GameModule, RES_NAMES, GAP_NAMES, OP } from './wasmgame.js';
import { makeRenderer } from './gfx.js';
import { decode } from '../wire.gen.js';

const $ = (id) => document.getElementById(id);
const clamp = (v, a, b) => (v < a ? a : v > b ? b : v);

/** Ticks per second at Normal speed: `TurnControl::timings` 0x00AFC4A4 is 67 ms. */
const TICK_MS = 67;

const state = {
  mod: null, gfx: null, data: null, play: null,
  who: 0,
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

  if (!mod.create(gamedata, playdata, 0xc0ffee)) throw new Error('game_create failed');
  if (!mod.hasGameData || !mod.hasPlayData) {
    throw new Error('packed retail-derived tables failed validation');
  }
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
      state.drag = { x0: w[0], y0: w[1], x1: w[0], y1: w[1], sx: e.clientX, sy: e.clientY, add: e.shiftKey };
      return;
    }
    if (e.button === 2) rightClick(w, e);
  });

  canvas.addEventListener('pointermove', (e) => {
    const w = screenToWorld(e.clientX, e.clientY);
    state.hoverTile = [Math.floor(w[0] / state.mod.subtile), Math.floor(w[1] / state.mod.subtile)];
    if (state.panning) {
      const c = $('gl');
      const r = c.getBoundingClientRect();
      const dpr = c.width / r.width;
      const p = pxPerSub();
      state.cam.x = state.panning.cx - (e.clientX - state.panning.x) * dpr / p;
      state.cam.y = state.panning.cy - (e.clientY - state.panning.y) * dpr / p;
      clampCam();
      return;
    }
    if (state.drag) { state.drag.x1 = w[0]; state.drag.y1 = w[1]; }
  });

  canvas.addEventListener('pointerup', (e) => {
    if (state.panning) { state.panning = null; return; }
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
    else centreOn(wx, wy);
  };
  mini.addEventListener('contextmenu', (e) => e.preventDefault());
  mini.addEventListener('pointerdown', miniJump);
  mini.addEventListener('pointermove', (e) => { if (e.buttons & 1) miniJump(e); });
}

function onKeyDown(e) {
  if (e.target.tagName === 'INPUT' || e.target.tagName === 'SELECT') return;
  keys.add(e.code);
  const m = state.mod;

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
    case 'Escape': state.buildType = null; renderMenus(); break;
    case 'KeyH': logPacket('HALT', m.halt(state.who)); break;
    case 'KeyB': $('tab-build').click(); break;
    case 'KeyF': jumpToSelection(); break;
    case 'Home': { const [sx, sy] = m.startOf(state.who); centreOn(sx, sy); break; }
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
  if (n) centreOn(sx / n, sy / n);
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

function issueMove(w) {
  if (!state.selection.length) return;
  logPacket('MOVE_TO', state.mod.moveTo(state.who, w[0], w[1]));
  ping(w, '#5ab7ff');
}

function placeBuilding(w) {
  const m = state.mod;
  const b = state.play?.buildings?.[String(state.buildType)];
  const [tx, ty] = anchorFor(w, b);
  const grade = m.placementGrade(state.who, state.buildType, tx, ty);
  if (grade === 0) { say('placement blocked — space_at_corner graded CORE_BLOCKED', 'warn'); return; }
  logPacket('BUILD', m.build(state.who, tx, ty, state.buildType));
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

function wirePanels() {
  for (const id of ['tab-build', 'tab-train']) {
    $(id).addEventListener('click', () => {
      document.querySelectorAll('.tab').forEach((t) => t.classList.remove('sel'));
      $(id).classList.add('sel');
      renderMenus();
    });
  }
  $('income').addEventListener('change', (e) => {
    state.mod.setIncomeMode(Number(e.target.value));
    say(`income mode: ${e.target.selectedOptions[0].textContent}`, 'hi');
  });
  $('popset').addEventListener('change', (e) => state.mod.setPopSetting(Number(e.target.value)));
  $('speed').addEventListener('input', (e) => setSpeed(Number(e.target.value)));
  $('pause').addEventListener('click', () => setPaused(!state.paused));
  $('halt').addEventListener('click', () => logPacket('HALT', state.mod.halt(state.who)));
  $('palette-filter').addEventListener('input', renderMenus);
}

function setPaused(paused) {
  state.paused = paused;
  const el = $('pause');
  if (el) el.textContent = paused ? 'resume (P)' : 'pause (P)';
}

function setSpeed(speed) {
  state.speed = clamp(Number(speed) || 1, 0.25, 8);
  const el = $('speed');
  if (el && Number(el.value) !== state.speed) el.value = String(state.speed);
}

/** The complete costed building catalog; eligibility gates are explicitly still open work. */
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

function renderMenus() {
  const m = state.mod;
  const buildTab = $('tab-build').classList.contains('sel');
  const host = $('palette');
  host.innerHTML = '';
  paletteItems = [];
  const filter = $('palette-filter').value.trim().toLocaleLowerCase();

  const sel = state.selection.map((id) => m.info(id)).filter(Boolean);
  const producers = sel.filter((s) => s.isBuilding && s.buildProgress < 0);

  if (buildTab) {
    if (!state.buildable) { host.textContent = 'no play data — build menu unavailable'; return; }
    if (!sel.some((s) => !s.isBuilding)) {
      host.innerHTML = '<div class="hint">select a citizen to place a building</div>';
    }
    for (const b of state.buildable) paletteItems.push({ kind: 'build', ...b });
  } else {
    if (!producers.length) {
      host.innerHTML = '<div class="hint">select a building to see what it makes — ' +
        'the menu is the 312 <code>WHERE</code> edges, so it can only offer what the ' +
        'tables say</div>';
    }
    const seen = new Set();
    for (const b of producers) {
      for (const t of m.products(b.typeId)) {
        if (seen.has(t)) continue;
        seen.add(t);
        const u = state.play?.units?.[String(t)];
        if (u) paletteItems.push({ kind: 'train', id: t, ...u });
      }
    }
    // Age advance, from the seven real age techs.
    const me = m.player(state.who);
    const age = state.play?.ages?.[me.age];
    if (age && me.age < 7) paletteItems.push({ kind: 'age', id: age.id, name: age.name, cost: age.cost, jobTime: 600 });
  }

  if (filter) {
    paletteItems = paletteItems.filter((it) =>
      it.name.toLocaleLowerCase().includes(filter) || String(it.id).includes(filter));
  }
  if (!paletteItems.length && !host.childElementCount) {
    host.innerHTML = '<div class="hint">no catalog entries match this filter</div>';
  }
  const me = m.player(state.who);
  paletteItems.forEach((it, i) => {
    const el = document.createElement('button');
    el.className = 'pal';
    if (state.buildType === it.id && it.kind === 'build') el.classList.add('armed');
    const afford = it.cost.every((c, k) => me.stock[k] >= c);
    if (!afford) el.classList.add('poor');
    const key = i < 8 ? 'QWERTYUI'[i] : '';
    el.innerHTML =
      `<span class="k">${key}</span><span class="n">${it.name}</span>` +
      `<span class="c">${costText(it.cost)}</span>`;
    el.title = `${it.name} — ${it.kind}, cost ${costText(it.cost, true)}, ` +
      `${it.jobTime} frames (${(it.jobTime * TICK_MS / 1000).toFixed(1)} s)` +
      (it.xSize ? `, footprint ${it.xSize}x${it.ySize} tiles` : '');
    el.addEventListener('click', () => activate(it));
    host.appendChild(el);
  });
}

function paletteActivate(slot) {
  if (slot < paletteItems.length) activate(paletteItems[slot]);
}

function activate(it) {
  const m = state.mod;
  if (it.kind === 'build') {
    state.buildType = state.buildType === it.id ? null : it.id;
    say(state.buildType ? `place ${it.name} — click the map, Esc cancels` : 'build cancelled');
    renderMenus();
  } else if (it.kind === 'train') {
    logPacket('QUEUE_UP', m.queueUp(state.who, it.id, 1));
  } else if (it.kind === 'age') {
    logPacket('QUEUE_UP(age)', m.queueUp(state.who, it.id, 1));
  }
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

function renderHud() {
  const m = state.mod;
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
    `pop ${me.pop}/${me.popCap}   ${ageName}` +
    (me.research ? `   researching ${(me.research / 600 * 100) | 0}%` : '');

  const stat = $('stat');
  stat.textContent =
    `${state.fps.toFixed(0)} fps   ${m.live} objects   frame ${m.frame} ` +
    `(${(m.frame * TICK_MS / 1000).toFixed(0)} s)   ` +
    `step ${state.stepMs.toFixed(2)} ms   upload ${state.uploadMs.toFixed(2)} ms   ` +
    `x${state.speed}${state.paused ? '  PAUSED' : ''}`;

  renderSelection();
  renderCoverage();
}

function renderSelection() {
  const m = state.mod;
  const host = $('sel');
  if (!state.selection.length) { host.innerHTML = '<div class="hint">nothing selected</div>'; return; }
  const infos = state.selection.map((id) => m.info(id)).filter(Boolean);
  if (infos.length !== state.selection.length) state.selection = infos.map((i) => i.id);
  if (!infos.length) { host.innerHTML = '<div class="hint">selection is gone</div>'; return; }

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
    if (cq) cq.addEventListener('click', () => logPacket('UNQUEUE', m.unqueue(state.who, -1)));
  } else {
    const byType = new Map();
    for (const i of infos) byType.set(i.typeId, (byType.get(i.typeId) ?? 0) + 1);
    host.innerHTML = `<div class="big">${infos.length} selected</div>` +
      [...byType].map(([t, n]) => `<div class="row"><span>${typeName(t)}</span><b>${n}</b></div>`).join('');
  }
}

let coverageEl = null;
function renderCoverage() {
  const m = state.mod;
  const gaps = m.gaps();
  const rows = gaps.map((v, i) => (v ? `${GAP_NAMES[i]}: ${v}` : null)).filter(Boolean);
  const el = $('coverage');
  const txt = rows.length
    ? 'issued but not executed\n' + rows.join('\n')
    : 'issued but not executed\n(none yet)';
  if (txt !== coverageEl) { el.textContent = txt; coverageEl = txt; }
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
  const OWNER = ['#5c9eff', '#ff5c4d', '#6bd97a', '#ffbf47'];
  for (let i = 0; i < live; i++) {
    const tag = v.tag[i];
    if ((tag & 0x80000000) === 0) continue;
    g.fillStyle = OWNER[(tag & 0xf) % OWNER.length];
    const b = (tag >>> 29) & 1;
    g.fillRect(v.x[i] * s - (b ? 1.5 : 0.5), v.y[i] * s - (b ? 1.5 : 0.5), b ? 3 : 1.5, b ? 3 : 1.5);
  }
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
  if (dx || dy) { state.cam.x += dx * panSpeed; state.cam.y += dy * panSpeed; clampCam(); }

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
  activate,
  info: (id) => state.mod.info(id),
  player: (p = 0) => state.mod.player(p),
  gaps: () => state.mod.gaps(),
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
    selection: state.selection.length, digest: state.mod.digest(),
    gaps: state.mod.gaps(), player: state.mod.player(state.who),
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
  key: (code) => onKeyDown({ code, key: code.replace('Key', ''), preventDefault() {}, target: {} }),

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
  document.body.insertAdjacentHTML('afterbegin',
    `<pre style="color:#ff6b5b;padding:16px;font:12px monospace">boot failed: ${e.message}\n${e.stack}</pre>`);
});
