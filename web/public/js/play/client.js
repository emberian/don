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

import { GameModule, RES_NAMES, GAP_NAMES, COMMANDS, OP, TAG } from './wasmgame.js';
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
const SETTINGS_PROTOCOL = 'don.browser-settings.v1';
const SETTINGS_STORAGE_KEY = 'don.browser-settings.v1';
const OWNER_PALETTES = Object.freeze({
  standard: Object.freeze([
    '#5c9eff', '#ff5c4d', '#6bd97a', '#ffbf47', '#cc80f2', '#59d9e0', '#f28ccb', '#bfbfbf',
  ]),
  'okabe-ito': Object.freeze([
    '#0072b2', '#d55e00', '#009e73', '#e69f00', '#cc79a7', '#56b4e9', '#f0e442', '#f4f4f4',
  ]),
  'high-separation': Object.freeze([
    '#4477aa', '#ee6677', '#228833', '#ccbb44', '#aa3377', '#66ccee', '#ee8866', '#eeeeee',
  ]),
});
const BINDING_ACTIONS = Object.freeze([
  Object.freeze({ id: 'pause', label: 'pause / resume', fallback: 'KeyP' }),
  Object.freeze({ id: 'home', label: 'camera home', fallback: 'Home' }),
  Object.freeze({ id: 'focusSelection', label: 'focus selection', fallback: 'KeyF' }),
  Object.freeze({ id: 'halt', label: 'halt selection', fallback: 'KeyH' }),
  Object.freeze({ id: 'build', label: 'build catalog', fallback: 'KeyB' }),
  Object.freeze({ id: 'train', label: 'train catalog', fallback: 'Shift+KeyT' }),
  Object.freeze({ id: 'research', label: 'research catalog', fallback: 'Shift+KeyR' }),
  Object.freeze({ id: 'idle', label: 'next idle worker', fallback: 'Period' }),
  Object.freeze({ id: 'selectAll', label: 'select all units', fallback: 'Primary+KeyA' }),
]);
const JOURNAL_PROTOCOL = 'don.command-journal.v1';
const MAX_JOURNAL_FRAMES = 1000000; // about 18.6 hours at the recovered 67 ms tick.
const MAX_JOURNAL_EVENTS = 50000;
const MAX_JOURNAL_JSON_BYTES = 16 * 1024 * 1024;
const MAX_JOURNAL_COMMAND_BYTES = 8 * 1024 * 1024;

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
  activeGroup: '1',
  groupStatus: 'group 1 empty',
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
  paletteAge: -1,
  cameraSource: 'home',
  settings: null,
  bindingCapture: null,
  settingsStatus: 'loading browser settings',
  coreSaveStatus: 'core save/load ready at inactive setup boundary',
  replay: {
    events: [], baseline: null, headFrame: 0,
    applying: false, playback: false, restoring: false,
    status: 'recording exact browser command packets',
  },
  commandFeedback: {
    nextId: 1, entries: [],
    lastTransport: null, lastGaps: null,
  },
};

// ---------------------------------------------------------------------------------------
// boot
// ---------------------------------------------------------------------------------------

async function boot() {
  const canvas = $('gl');
  const params = new URLSearchParams(location.search);
  const loadedSettings = loadBrowserSettings();
  const requestedBackend = params.get('backend');
  if (['webgpu', 'canvas2d'].includes(requestedBackend)) {
    loadedSettings.performance.renderer = requestedBackend;
  }
  applyBrowserSettings(loadedSettings, { persist: false, announce: false });

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

  const rendererPreference = state.settings.performance.renderer === 'auto'
    ? null : state.settings.performance.renderer;
  state.gfx = await makeRenderer(canvas, rendererPreference);
  state.gfx.setOwnerPalette(ownerColours());
  badge('be', state.gfx.kind, state.gfx.kind === 'webgpu' ? 'on' : 'off');
  resize();
  state.gfx.provision(mod.tiles, mod.x.game_capacity(mod.g));

  const [sx, sy] = mod.startOf(state.who);
  centreOn(sx, sy);

  // A failed WebGPU attempt replaces the canvas before Canvas 2D claims it. Bind input to
  // the renderer's live canvas, not the now-detached element captured at boot.
  wireInput($('gl'));
  wirePanels();
  initializeSettingsPanel();
  initializeSessionPanel();
  initializeObjectivesPanel();
  initializeControlGroupsPanel();
  initializeCommandFeedbackPanel();
  initializeReplayPanel();
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

function defaultBrowserSettings() {
  return {
    protocol: SETTINGS_PROTOCOL,
    visual: {
      uiScale: 1,
      ownerPalette: 'standard',
      highContrast: false,
      reducedMotion: matchMedia('(prefers-reduced-motion: reduce)').matches,
    },
    performance: { renderer: 'auto', fpsCap: 0 },
    input: {
      bindings: Object.fromEntries(BINDING_ACTIONS.map((action) => [action.id, action.fallback])),
    },
  };
}

function canonicalBinding(value) {
  const parts = String(value).split('+').filter(Boolean);
  const code = parts.pop();
  if (!code || !/^[A-Za-z][A-Za-z0-9]{1,30}$/.test(code)) {
    throw new Error(`invalid binding code ${JSON.stringify(value)}`);
  }
  const modifiers = new Set();
  for (const part of parts) {
    const normalized = part === 'Ctrl' || part === 'Meta' ? 'Primary' : part;
    if (!['Primary', 'Alt', 'Shift'].includes(normalized) || modifiers.has(normalized)) {
      throw new Error(`invalid binding modifier in ${JSON.stringify(value)}`);
    }
    modifiers.add(normalized);
  }
  return [...['Primary', 'Alt', 'Shift'].filter((modifier) => modifiers.has(modifier)), code].join('+');
}

function bindingConflictsWithFixedInput(binding) {
  const parts = binding.split('+');
  const code = parts.at(-1);
  const unmodified = parts.length === 1;
  if (/^Digit[1-9]$/.test(code)) return 'control groups reserve Digit1..Digit9 with modifiers';
  if (['Escape', 'Space', 'ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown',
    'Equal', 'Minus', 'NumpadAdd', 'NumpadSubtract'].includes(code)) return `${code} is a fixed map/runtime control`;
  if (unmodified && ['KeyW', 'KeyA', 'KeyS', 'KeyD'].includes(code)) {
    return `${code} is reserved for camera movement`;
  }
  if (unmodified && /^Key[QWERTYUI]$/.test(code)) return `${code} is reserved for palette slots`;
  return '';
}

function normalizeBrowserSettings(input) {
  if (!input || typeof input !== 'object' || Array.isArray(input)) throw new Error('settings root must be an object');
  if (input.protocol !== SETTINGS_PROTOCOL) throw new Error(`unsupported settings protocol ${JSON.stringify(input.protocol)}`);
  const visual = input.visual;
  const performanceSettings = input.performance;
  const bindingsInput = input.input?.bindings;
  if (!visual || !performanceSettings || !bindingsInput || typeof bindingsInput !== 'object') {
    throw new Error('settings visual, performance, and input.bindings sections are required');
  }
  const uiScale = Number(visual.uiScale);
  if (![0.9, 1, 1.15, 1.3].includes(uiScale)) throw new Error('UI scale is unsupported');
  if (!OWNER_PALETTES[visual.ownerPalette]) throw new Error('owner palette is unsupported');
  if (typeof visual.highContrast !== 'boolean' || typeof visual.reducedMotion !== 'boolean') {
    throw new Error('contrast and reduced-motion settings must be boolean');
  }
  const renderer = String(performanceSettings.renderer);
  if (!['auto', 'webgpu', 'canvas2d'].includes(renderer)) throw new Error('renderer preference is unsupported');
  const fpsCap = Number(performanceSettings.fpsCap);
  if (![0, 20, 30, 60].includes(fpsCap)) throw new Error('visual FPS cap is unsupported');
  const bindings = {};
  const used = new Map();
  for (const action of BINDING_ACTIONS) {
    const binding = canonicalBinding(bindingsInput[action.id]);
    const fixedConflict = bindingConflictsWithFixedInput(binding);
    if (fixedConflict) throw new Error(`${action.label}: ${fixedConflict}`);
    if (used.has(binding)) throw new Error(`${action.label} conflicts with ${used.get(binding)} at ${binding}`);
    used.set(binding, action.label);
    bindings[action.id] = binding;
  }
  return {
    protocol: SETTINGS_PROTOCOL,
    visual: {
      uiScale, ownerPalette: visual.ownerPalette,
      highContrast: visual.highContrast, reducedMotion: visual.reducedMotion,
    },
    performance: { renderer, fpsCap },
    input: { bindings },
  };
}

function loadBrowserSettings() {
  try {
    const stored = localStorage.getItem(SETTINGS_STORAGE_KEY);
    if (!stored) return normalizeBrowserSettings(defaultBrowserSettings());
    if (new TextEncoder().encode(stored).length > 65536) throw new Error('stored settings exceed 64 KiB');
    return normalizeBrowserSettings(JSON.parse(stored));
  } catch (error) {
    state.settingsStatus = `stored settings refused; defaults loaded: ${error.message}`;
    return normalizeBrowserSettings(defaultBrowserSettings());
  }
}

function ownerColours() {
  return OWNER_PALETTES[state.settings?.visual.ownerPalette ?? 'standard'];
}

function settingsClone() {
  return JSON.parse(JSON.stringify(state.settings));
}

function persistBrowserSettings() {
  try {
    localStorage.setItem(SETTINGS_STORAGE_KEY, JSON.stringify(state.settings));
    return true;
  } catch (error) {
    state.settingsStatus = `settings applied for this tab but persistence failed: ${error.message}`;
    return false;
  }
}

function applyBrowserSettings(input, { persist = true, announce = true, status = '' } = {}) {
  const settings = normalizeBrowserSettings(input);
  state.settings = settings;
  const root = document.documentElement;
  root.dataset.contrast = settings.visual.highContrast ? 'high' : 'standard';
  root.dataset.reducedMotion = String(settings.visual.reducedMotion);
  root.dataset.ownerPalette = settings.visual.ownerPalette;
  root.style.setProperty('--ui-scale', String(settings.visual.uiScale));
  if (settings.visual.reducedMotion && typeof pings !== 'undefined') pings.length = 0;
  if (state.gfx?.setOwnerPalette) state.gfx.setOwnerPalette(ownerColours());
  if (persist) persistBrowserSettings();
  if (state.mod) syncSessionUrl();
  syncSettingsControls();
  if (state.mod && $('owner-legend')?.childElementCount) renderObjectivesPanel();
  if (state.gfx) { resize(); clampCam(); }
  state.settingsStatus = status ||
    `saved in this browser · ${settings.visual.ownerPalette} palette · ` +
    `${settings.performance.fpsCap || 'display'} FPS · renderer ${settings.performance.renderer}`;
  renderSettingsStatus();
  if (announce) say('browser settings applied', 'ok');
  return settingsSnapshot();
}

function mutateBrowserSettings(mutator, status = '') {
  const next = settingsClone();
  mutator(next);
  return applyBrowserSettings(next, { status });
}

function formatBinding(binding) {
  return binding
    .replace('Primary+', navigator.platform.includes('Mac') ? '⌘+' : 'Ctrl+')
    .replace(/Key([A-Z])/g, '$1')
    .replace(/Digit([0-9])/g, '$1');
}

function chordFromEvent(event) {
  const modifiers = [];
  if (event.ctrlKey || event.metaKey) modifiers.push('Primary');
  if (event.altKey) modifiers.push('Alt');
  if (event.shiftKey) modifiers.push('Shift');
  return canonicalBinding([...modifiers, event.code].join('+'));
}

function bindingActionForEvent(event) {
  let chord;
  try { chord = chordFromEvent(event); } catch { return null; }
  return BINDING_ACTIONS.find((action) => state.settings.input.bindings[action.id] === chord) ?? null;
}

function runBoundAction(action) {
  switch (action.id) {
    case 'pause': setPaused(!state.paused); break;
    case 'home': focusPlayerStart(state.who, 'bound home action'); break;
    case 'focusSelection': jumpToSelection(); break;
    case 'halt': logPacket('HALT', state.mod.halt(state.who)); break;
    case 'build': openCatalog('build'); break;
    case 'train': openCatalog('train'); break;
    case 'research': openCatalog('research'); break;
    case 'idle': cycleIdleWorker(); break;
    case 'selectAll': selectAllUnits(); break;
    default: return false;
  }
  return true;
}

function settingsSnapshot() {
  return Object.freeze({
    ...settingsClone(),
    activeRenderer: state.gfx?.kind ?? 'booting',
    storageKey: SETTINGS_STORAGE_KEY,
    status: state.settingsStatus,
  });
}

function exportBrowserSettings() {
  return `${JSON.stringify(state.settings, null, 2)}\n`;
}

function importBrowserSettings(input) {
  let parsed;
  try {
    if (typeof input === 'string' && new TextEncoder().encode(input).length > 65536) {
      throw new Error('settings JSON exceeds 64 KiB');
    }
    parsed = typeof input === 'string' ? JSON.parse(input) : input;
  } catch (error) {
    throw new Error(`settings import refused: ${error.message}`);
  }
  return applyBrowserSettings(parsed, { status: 'imported and saved; renderer changes require reload' });
}

function resetBrowserSettings() {
  return applyBrowserSettings(defaultBrowserSettings(), { status: 'browser settings reset to defaults' });
}

function syncSettingsControls() {
  if (!$('settings') || !state.settings) return;
  $('settings-scale').value = String(state.settings.visual.uiScale);
  $('settings-palette').value = state.settings.visual.ownerPalette;
  $('settings-contrast').checked = state.settings.visual.highContrast;
  $('settings-motion').checked = state.settings.visual.reducedMotion;
  $('settings-renderer').value = state.settings.performance.renderer;
  $('settings-fps').value = String(state.settings.performance.fpsCap);
  const binding = (id) => formatBinding(state.settings.input.bindings[id]);
  $('halt').textContent = `halt (${binding('halt')})`;
  $('cmd-halt').lastElementChild.textContent = binding('halt');
  $('cmd-pause').lastElementChild.textContent = binding('pause');
  $('cmd-build').lastElementChild.textContent = binding('build');
  $('tab-build').querySelector('.ck').textContent = binding('build');
  $('tab-train').querySelector('.ck').textContent = binding('train');
  $('tab-research').querySelector('.ck').textContent = binding('research');
  setPaused(state.paused, false);
  renderSettingsBindings();
}

function renderSettingsBindings() {
  const host = $('settings-bindings');
  if (!host || !state.settings) return;
  if (!host.childElementCount) {
    for (const action of BINDING_ACTIONS) {
      const row = document.createElement('div');
      row.className = 'binding-row';
      const label = document.createElement('span');
      label.textContent = action.label;
      const button = document.createElement('button');
      button.type = 'button';
      button.dataset.binding = action.id;
      button.addEventListener('click', () => {
        state.bindingCapture = action.id;
        state.settingsStatus = `press a non-reserved key chord for ${action.label}; Escape cancels`;
        renderSettingsBindings();
        renderSettingsStatus();
      });
      row.append(label, button);
      host.appendChild(row);
    }
  }
  for (const button of host.querySelectorAll('button[data-binding]')) {
    const listening = button.dataset.binding === state.bindingCapture;
    button.classList.toggle('listening', listening);
    button.textContent = listening ? 'press keys…' : formatBinding(state.settings.input.bindings[button.dataset.binding]);
  }
}

function renderSettingsStatus() {
  if (!$('settings-status') || !state.settings) return;
  const active = state.gfx?.kind ?? 'booting';
  const requested = state.settings.performance.renderer;
  let rendererNote = '';
  if (requested !== 'auto' && active !== 'booting' && requested !== active) {
    const attempted = new URL(location.href).searchParams.get('backend') === requested;
    rendererNote = attempted
      ? ` · requested ${requested}; active ${active} fallback`
      : ` · reload required for ${requested}`;
  }
  $('settings-status').textContent = `${state.settingsStatus} · active ${active}${rendererNote}`;
}

function captureBinding(event) {
  if (!state.bindingCapture) return;
  event.preventDefault();
  event.stopImmediatePropagation();
  if (event.code === 'Escape') {
    state.bindingCapture = null;
    state.settingsStatus = 'binding change cancelled';
    renderSettingsBindings();
    renderSettingsStatus();
    return;
  }
  const action = BINDING_ACTIONS.find((candidate) => candidate.id === state.bindingCapture);
  try {
    const binding = chordFromEvent(event);
    const next = settingsClone();
    next.input.bindings[action.id] = binding;
    applyBrowserSettings(next, { status: `${action.label} bound to ${formatBinding(binding)}` });
    state.bindingCapture = null;
  } catch (error) {
    state.settingsStatus = `binding refused: ${error.message}`;
  }
  renderSettingsBindings();
  renderSettingsStatus();
}

function downloadBrowserSettings() {
  const blob = new Blob([exportBrowserSettings()], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = 'don-browser-settings.json';
  anchor.click();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}

function initializeSettingsPanel() {
  $('settings-scale').addEventListener('change', (event) =>
    mutateBrowserSettings((settings) => { settings.visual.uiScale = Number(event.target.value); }));
  $('settings-palette').addEventListener('change', (event) =>
    mutateBrowserSettings((settings) => { settings.visual.ownerPalette = event.target.value; }));
  $('settings-contrast').addEventListener('change', (event) =>
    mutateBrowserSettings((settings) => { settings.visual.highContrast = event.target.checked; }));
  $('settings-motion').addEventListener('change', (event) =>
    mutateBrowserSettings((settings) => { settings.visual.reducedMotion = event.target.checked; }));
  $('settings-renderer').addEventListener('change', (event) =>
    mutateBrowserSettings((settings) => { settings.performance.renderer = event.target.value; },
      'renderer preference saved; apply and reload to recreate the canvas backend'));
  $('settings-fps').addEventListener('change', (event) =>
    mutateBrowserSettings((settings) => { settings.performance.fpsCap = Number(event.target.value); }));
  $('settings-renderer-apply').addEventListener('click', () => location.assign(sessionUrl().href));
  $('settings-export').addEventListener('click', downloadBrowserSettings);
  $('settings-import').addEventListener('click', () => $('settings-file').click());
  $('settings-reset').addEventListener('click', resetBrowserSettings);
  $('settings-file').addEventListener('change', async (event) => {
    const file = event.target.files?.[0];
    event.target.value = '';
    if (!file) return;
    try { importBrowserSettings(await file.text()); say('browser settings imported', 'ok'); }
    catch (error) { state.settingsStatus = error.message; renderSettingsStatus(); say(error.message, 'warn'); }
  });
  window.addEventListener('keydown', captureBinding, true);
  syncSettingsControls();
  renderSettingsStatus();
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

function normalizeGroupSlot(slot) {
  const key = String(slot);
  if (!/^[1-9]$/.test(key)) throw new Error(`control group must be 1..9, got ${JSON.stringify(slot)}`);
  return key;
}

function selectionRecords(ids = state.selection) {
  const seen = new Set();
  const records = [];
  for (const value of ids) {
    const id = Number(value);
    if (!Number.isInteger(id) || seen.has(id)) continue;
    const info = state.mod.info(id);
    if (!info || info.owner !== state.who) continue;
    seen.add(id);
    records.push(Object.freeze({ id, owner: info.owner, typeId: info.typeId }));
    if (records.length === 255) break;
  }
  return records;
}

function liveControlGroup(slot, prune = true) {
  const key = normalizeGroupSlot(slot);
  const stored = state.groups.get(key) ?? [];
  const live = stored.filter((record) => {
    const info = state.mod.info(record.id);
    return info && info.owner === record.owner && info.typeId === record.typeId && info.owner === state.who;
  });
  if (prune && live.length !== stored.length) state.groups.set(key, live);
  return live;
}

function setGroupStatus(message) {
  state.groupStatus = message;
  const status = $('group-status');
  if (status) status.textContent = message;
}

function replaceControlGroup(slot, ids = state.selection) {
  const key = normalizeGroupSlot(slot);
  state.activeGroup = key;
  const records = selectionRecords(ids);
  state.groups.set(key, records);
  setGroupStatus(records.length
    ? `group ${key} replaced with ${records.length} current object ID(s)`
    : `group ${key} cleared; the current selection has no commandable IDs`);
  renderControlGroups();
  return controlGroupsSnapshot();
}

function addToControlGroup(slot, ids = state.selection) {
  const key = normalizeGroupSlot(slot);
  state.activeGroup = key;
  const records = liveControlGroup(key);
  const known = new Set(records.map((record) => record.id));
  for (const record of selectionRecords(ids)) {
    if (!known.has(record.id) && records.length < 255) {
      known.add(record.id);
      records.push(record);
    }
  }
  state.groups.set(key, records);
  setGroupStatus(`group ${key} now has ${records.length} current object ID(s)`);
  renderControlGroups();
  return controlGroupsSnapshot();
}

function removeFromControlGroup(slot, ids = state.selection) {
  const key = normalizeGroupSlot(slot);
  state.activeGroup = key;
  const removing = new Set(selectionRecords(ids).map((record) => record.id));
  const records = liveControlGroup(key).filter((record) => !removing.has(record.id));
  state.groups.set(key, records);
  setGroupStatus(`group ${key} now has ${records.length} current object ID(s)`);
  renderControlGroups();
  return controlGroupsSnapshot();
}

function clearControlGroup(slot) {
  return replaceControlGroup(slot, []);
}

function recallControlGroup(slot, { jumpOnRepeat = false, source = 'control group' } = {}) {
  const key = normalizeGroupSlot(slot);
  state.activeGroup = key;
  const before = (state.groups.get(key) ?? []).length;
  const records = liveControlGroup(key);
  const pruned = before - records.length;
  const now = performance.now();
  const repeated = state.lastGroupKey.key === key && now - state.lastGroupKey.t < 500;
  state.lastGroupKey = { key, t: now };
  if (!records.length) {
    setGroupStatus(`group ${key} is empty${pruned ? `; pruned ${pruned} stale ID(s)` : ''}`);
    renderControlGroups();
    return [];
  }
  const ids = records.map((record) => record.id);
  selectIds(ids);
  if (jumpOnRepeat && repeated) {
    jumpToSelection();
    state.cameraSource = `${source} ${key} repeat`;
  }
  setGroupStatus(
    `recalled group ${key}: ${ids.length} exact object ID(s)` +
    (pruned ? `; pruned ${pruned} stale ID(s)` : '') +
    (jumpOnRepeat && repeated ? '; camera centred' : ''));
  renderControlGroups();
  return ids;
}

function controlGroupsSnapshot() {
  const groups = {};
  for (let slot = 1; slot <= 9; slot++) {
    groups[String(slot)] = liveControlGroup(slot).map((record) => record.id);
  }
  return Object.freeze({
    active: state.activeGroup,
    selection: state.selection.slice(),
    groups: Object.freeze(groups),
    status: state.groupStatus,
    nativePersistence: 'unavailable',
    objectGeneration: 'unavailable',
  });
}

function renderControlGroups() {
  const host = $('group-slots');
  if (!host || !state.mod) return;
  if (!host.childElementCount) {
    for (let slot = 1; slot <= 9; slot++) {
      const button = document.createElement('button');
      button.type = 'button';
      button.className = 'group-slot';
      button.dataset.group = String(slot);
      button.addEventListener('click', () => recallControlGroup(slot, {
        jumpOnRepeat: true, source: 'touch control group',
      }));
      host.appendChild(button);
    }
  }
  for (const button of host.children) {
    const key = button.dataset.group;
    const count = liveControlGroup(key).length;
    button.classList.toggle('active', key === state.activeGroup);
    button.setAttribute('aria-pressed', String(key === state.activeGroup));
    button.setAttribute('aria-label', `Recall control group ${key}, ${count} objects`);
    button.innerHTML = `${key}<span class="group-count">${count || '—'}</span>`;
  }
  const hasSelection = selectionRecords().length > 0;
  $('group-add').disabled = !hasSelection;
  $('group-remove').disabled = !hasSelection;
  $('group-clear').disabled = liveControlGroup(state.activeGroup).length === 0;
  setGroupStatus(state.groupStatus);
}

function initializeControlGroupsPanel() {
  $('group-set').addEventListener('click', () => replaceControlGroup(state.activeGroup));
  $('group-add').addEventListener('click', () => addToControlGroup(state.activeGroup));
  $('group-remove').addEventListener('click', () => removeFromControlGroup(state.activeGroup));
  $('group-clear').addEventListener('click', () => clearControlGroup(state.activeGroup));
  renderControlGroups();
}

function onKeyDown(e) {
  const tag = e.target && e.target.tagName;
  if (['INPUT', 'SELECT', 'TEXTAREA', 'BUTTON'].includes(tag) || e.target?.isContentEditable) return;
  const bound = bindingActionForEvent(e);
  if (bound && runBoundAction(bound)) {
    e.preventDefault();
    return;
  }
  keys.add(e.code);

  // Control groups live over exact currently-exported object IDs. Membership edits do not
  // fabricate engine commands; recall goes through the same GroupCommand as mouse selection.
  if (/^Digit[1-9]$/.test(e.code)) {
    const k = e.code.slice(5);
    if (e.ctrlKey || e.metaKey) {
      if (e.shiftKey) addToControlGroup(k);
      else replaceControlGroup(k);
    } else if (e.altKey) {
      removeFromControlGroup(k);
    } else {
      recallControlGroup(k, { jumpOnRepeat: true, source: 'keyboard control group' });
    }
    e.preventDefault();
    return;
  }

  switch (e.code) {
    case 'Escape': cancelTargeting(); break;
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
  renderControlGroups();
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
  $('session-activate').addEventListener('click', activateSessionRoster);
  $('session-seed').addEventListener('keydown', (event) => {
    if (event.key === 'Enter') restartSessionFromPanel();
  });
  players.addEventListener('change', () => switchPlayer(Number(players.value)));
  $('session-share').addEventListener('click', shareSessionLink);
  syncSessionUrl();
  renderSessionStatus();
  renderSessionSummary();
}

function activateSessionRoster() {
  const roster = Array.from({ length: state.mod.playerCount }, (_, player) => player);
  if (!state.mod.activatePlayers(roster)) return false;
  $('session-activate').disabled = true;
  $('core-save').disabled = true;
  state.coreSaveStatus =
    'live roster active — core save unavailable because active step-8/victory state is not serialized; load remains available';
  $('core-save-status').textContent = state.coreSaveStatus;
  syncSessionUrl();
  renderSessionSummary();
  renderObjectivesPanel();
  say(`activated authoritative Sim roster ${roster.join(', ')}`, 'ok');
  return true;
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
  resetClientForWorld(seed, false, `new session: P${state.who}`);
  startReplayJournal();
  input.value = formatSeed(seed);
  syncSessionUrl();
  say(`new session — requested seed ${formatSeed(seed)}, player ${state.who}; ` +
    'seed-dependent map generation remains blocked', 'ok');
  return true;
}

function resetClientForWorld(seed, paused, cameraSource) {
  state.sessionSeed = seed;
  state.selection = [];
  state.groups.clear();
  state.activeGroup = '1';
  state.groupStatus = 'group 1 empty';
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
  state.coreSaveStatus = 'core save/load ready at inactive setup boundary';
  if ($('session-activate')) $('session-activate').disabled = false;
  if ($('core-save')) $('core-save').disabled = !state.mod.supports('save');
  resetCommandFeedback();
  miniVersion = -1;
  miniTerrain = null;
  previousGaps = null;
  pings.length = 0;
  acc = 0;
  last = performance.now();
  visualLast = 0;
  state.gfx.provision(state.mod.tiles, state.mod.x.game_capacity(state.mod.g));
  const [sx, sy] = state.mod.startOf(state.who);
  centreOn(sx, sy);
  state.cameraSource = cameraSource;
  setPaused(paused, false);
  renderMenus();
  renderSelection();
  refreshPaletteAvailability();
  renderPaletteFeedback();
  renderSessionStatus();
  renderSessionSummary();
}

function switchPlayer(player) {
  const next = parseSessionPlayer(player, state.mod.playerCount);
  if (next === state.who) return;
  state.mod.group(state.who, []);
  state.who = next;
  state.selection = [];
  state.groups.clear();
  state.activeGroup = '1';
  state.groupStatus = `groups reset for player ${state.who} perspective`;
  state.mod.group(state.who, []);
  state.buildType = null;
  state.commandMode = null;
  const [sx, sy] = state.mod.startOf(state.who);
  centreOn(sx, sy);
  state.cameraSource = `perspective: P${state.who}`;
  syncSessionUrl();
  renderMenus();
  renderSelection();
  renderControlGroups();
  refreshPaletteAvailability();
  renderPaletteFeedback();
  renderSessionStatus();
  renderSessionSummary();
  say(`player perspective changed to ${state.who}`, 'hi');
}

function sessionUrl() {
  const current = new URL(location.href);
  const url = new URL(current.href);
  url.search = '';
  url.hash = '';
  url.searchParams.set('seed', formatSeed(state.sessionSeed));
  url.searchParams.set('player', String(state.who));
  url.searchParams.set('map', SESSION_MAP);
  url.searchParams.set('size', `${state.mod.tiles}x${state.mod.tiles}`);
  url.searchParams.set('nation', 'unavailable');
  url.searchParams.set('team', `unconfigured-${state.mod.leader(state.who).team}`);
  url.searchParams.set('slots', state.mod.activePlayers().join(','));
  url.searchParams.set('ai_slots', 'unavailable');
  url.searchParams.set('ai_difficulty', 'unavailable');
  url.searchParams.set('income', 'unavailable');
  url.searchParams.set('population', 'unavailable');
  url.searchParams.set('victory', state.mod.match().slug);
  if (state.settings.performance.renderer !== 'auto') {
    url.searchParams.set('backend', state.settings.performance.renderer);
  }
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
    `initial digest ${state.sessionInitialDigest} · seed is owned by don_sim::Sim`;
}

function sessionDescriptor() {
  const leader = state.mod.leader(state.who);
  const match = state.mod.match();
  return Object.freeze({
    seed: formatSeed(state.sessionSeed),
    player: state.who,
    map: SESSION_MAP,
    size: `${state.mod.tiles}x${state.mod.tiles}`,
    nation: 'unavailable',
    team: leader.team,
    teamConfigured: leader.teamConfigured,
    teamMutable: false,
    slots: state.mod.activePlayers().length,
    activePlayers: state.mod.activePlayers(),
    aiSlots: 'unavailable',
    aiDifficulty: 'unavailable',
    income: 'unavailable',
    population: 'unavailable',
    victory: match.slug,
    victoryMutable: false,
  });
}

function renderSessionSummary() {
  if (!$('session-summary') || !state.mod) return;
  const setup = sessionDescriptor();
  const leader = state.mod.leader(setup.player);
  const match = state.mod.match();
  const teamOption = $('session-team').options[0];
  teamOption.value = String(leader.team);
  teamOption.textContent = `unconfigured slot ${leader.team} — core read-only`;
  $('session-team').value = String(leader.team);
  const victoryOption = $('session-victory').options[0];
  victoryOption.value = match.slug;
  victoryOption.textContent = `${match.label} — core read-only`;
  $('session-victory').value = match.slug;
  $('summary-world').textContent =
    `integration land · ${state.mod.tiles} × ${state.mod.tiles} tiles · fixed`;
  $('summary-player').textContent =
    `P${setup.player} · nation unavailable · unconfigured team slot ${setup.team} (read-only)`;
  $('summary-slots').textContent =
    `${setup.slots} active manual core leaders (${setup.activePlayers.join(', ')}) · AI unavailable`;
  $('summary-rules').textContent =
    `income unavailable · population unavailable · ${match.label} victory (read-only)`;
}

function initializeObjectivesPanel() {
  const colours = ownerColours();
  const legend = $('owner-legend');
  const players = $('world-players');
  legend.replaceChildren();
  players.replaceChildren();
  for (let p = 0; p < state.mod.playerCount; p++) {
    const key = document.createElement('div');
    key.className = 'owner-key';
    const swatch = document.createElement('span');
    swatch.className = 'owner-swatch';
    swatch.style.background = colours[p % colours.length];
    const label = document.createElement('span');
    label.id = `owner-key-${p}`;
    key.append(swatch, label);
    legend.appendChild(key);

    const row = document.createElement('div');
    row.className = 'world-player';
    row.dataset.player = String(p);
    const owner = document.createElement('div');
    owner.className = 'owner-id';
    owner.style.color = colours[p % colours.length];
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
  const match = m.match();
  const owners = Array.from({ length: m.playerCount }, (_, player) => {
    const ledger = m.player(player);
    return {
      player,
      objects: 0,
      units: 0,
      buildings: 0,
      foundations: 0,
      ledger,
      economyStock: ledger.stock.reduce((sum, value) => sum + value, 0),
      leader: m.leader(player),
      relation: m.relation(state.who, player).name,
    };
  });
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
    diplomacy: 'effective-core-readonly',
    victory: match.slug,
    gameOver: match.gameOver,
    score: owners[state.who].leader.score,
    teamScore: owners[state.who].leader.teamScore,
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
  say(`camera focused on P${p} exported start; diplomacy is read-only and visibility stays omniscient`, 'hi');
  return cameraSnapshot();
}

function renderObjectivesPanel() {
  if (!$('objectives') || !state.mod) return;
  const snapshot = exportedWorldSnapshot();
  const camera = cameraSnapshot();
  const colours = ownerColours();
  const match = state.mod.match();
  const local = snapshot.owners[state.who];
  const outcome = local.leader.won ? 'won' : local.leader.defeated ? 'defeated' :
    local.leader.active ? 'active' : 'leader slot inactive';
  $('objective-time').textContent =
    `frame ${snapshot.frame.toLocaleString()} · elapsed ${snapshot.elapsedSeconds.toFixed(1)} s`;
  $('objective-state').textContent =
    `${match.label} · ${snapshot.gameOver ? 'game over' : outcome} · core read-only`;
  $('objective-score').textContent =
    `P${state.who} victory ${snapshot.score} · team ${snapshot.teamScore} · ` +
    `economy stock ${local.economyStock}`;
  $('objective-countdown').textContent = 'unavailable — mode-specific countdown is not exported';
  for (const owner of snapshot.owners) {
    const relation = owner.player === state.who ? 'you' : owner.relation;
    const inactive = owner.leader.active ? '' : ' · inactive leader slot';
    const key = $(`owner-key-${owner.player}`);
    if (key) key.textContent = `P${owner.player} ${relation}`;
    const swatch = key?.previousElementSibling;
    if (swatch) swatch.style.background = colours[owner.player % colours.length];
    const row = document.querySelector(`.world-player[data-player="${owner.player}"]`);
    if (row) row.classList.toggle('you', owner.player === state.who);
    const ownerId = row?.querySelector('.owner-id');
    if (ownerId) ownerId.style.color = colours[owner.player % colours.length];
    const stateEl = $(`owner-state-${owner.player}`);
    if (stateEl) {
      const objectWord = owner.objects === 1 ? 'object' : 'objects';
      const unitWord = owner.units === 1 ? 'unit' : 'units';
      const buildingWord = owner.buildings === 1 ? 'building' : 'buildings';
      stateEl.textContent =
        `${owner.objects} ${objectWord} · ${owner.units} ${unitWord} · ` +
        `${owner.buildings} ${buildingWord}` +
        (owner.foundations ? ` (${owner.foundations} foundations)` : '') +
        ` · pop ${owner.ledger.pop}/${owner.ledger.popCap} · age ${owner.ledger.age} · ` +
        `team unconfigured/${owner.leader.team} · ${relation}${inactive}`;
    }
  }
  $('camera-status').textContent =
    `camera tile ${camera.tileX},${camera.tileY} · ${camera.source} · ` +
    'minimap click/drag navigates; right-click issues a move for the current selection';
  $('mini').setAttribute('aria-label',
    `Omniscient integration minimap, camera at tile ${camera.tileX}, ${camera.tileY}. ` +
    'All exported owners are visible; diplomacy is read-only and fog is unavailable.');
}

// ---------------------------------------------------------------------------------------
// exact packet lifecycle feedback
// ---------------------------------------------------------------------------------------

function resetCommandFeedback() {
  const transport = state.mod.transport();
  state.commandFeedback.nextId = 1;
  state.commandFeedback.entries = [];
  state.commandFeedback.issuedTotal = 0;
  state.commandFeedback.appliedTotal = 0;
  state.commandFeedback.refusedTotal = 0;
  state.commandFeedback.drainedTotal = 0;
  state.commandFeedback.lastTransport = { ...transport };
  state.commandFeedback.lastGaps = state.mod.gaps();
  renderCommandFeedback();
}

function recordCommandIssued({ frame, who, bytes }, source = 'player') {
  reconcileCommandFeedback();
  const decoded = decode(bytes);
  const entry = {
    id: state.commandFeedback.nextId++,
    kind: 'packet',
    issuedFrame: frame,
    drainedFrame: null,
    who,
    op: decoded.op,
    struct: decoded.struct ?? `unknown opcode 0x${decoded.op.toString(16).padStart(2, '0')}`,
    bytes: bytes.length,
    hex: bytesToHex(bytes),
    selection: state.selection.slice(),
    source,
    status: 'pending',
    reason: 'issued to the exported command buffer; waiting for a tick',
  };
  state.commandFeedback.entries.push(entry);
  state.commandFeedback.issuedTotal++;
  trimCommandFeedback();
  renderCommandFeedback();
  return entry;
}

function refusalDelta(before, after) {
  const reasons = [];
  let total = 0;
  for (let i = 0; i < Math.max(before.length, after.length); i++) {
    const delta = (after[i] ?? 0) - (before[i] ?? 0);
    if (delta > 0) {
      total += delta;
      reasons.push(`${GAP_NAMES[i] ?? `gap ${i}`} +${delta}`);
    }
  }
  return { total, reasons };
}

function settleCommandEntry(entry, status, frame, reason) {
  if (!entry || entry.status !== 'pending') return;
  entry.status = status;
  entry.drainedFrame = frame;
  entry.reason = reason;
  state.commandFeedback.drainedTotal++;
  if (status === 'applied') state.commandFeedback.appliedTotal++;
  if (status === 'refused' || status === 'partial') state.commandFeedback.refusedTotal++;
}

function reconcileCommandFeedback() {
  if (!state.mod || !state.commandFeedback.lastTransport || !state.commandFeedback.lastGaps) return;
  const current = state.mod.transport();
  const currentGaps = state.mod.gaps();
  const prior = state.commandFeedback.lastTransport;
  if (current.drained < prior.drained || current.submitted < prior.submitted) {
    // A world replacement owns a new set of transport counters.
    state.commandFeedback.lastTransport = { ...current };
    state.commandFeedback.lastGaps = currentGaps;
    return;
  }
  const drained = current.drained - prior.drained;
  const ordersApplied = current.ordersApplied - prior.ordersApplied;
  const refusal = refusalDelta(state.commandFeedback.lastGaps, currentGaps);
  if (drained > 0) {
    const pending = state.commandFeedback.entries.filter((entry) =>
      entry.kind === 'packet' && entry.status === 'pending').slice(0, drained);
    const groupPackets = pending.filter((entry) => entry.op === OP.GROUP);
    const orderPackets = pending.filter((entry) => entry.op !== OP.GROUP);
    for (const entry of groupPackets) {
      settleCommandEntry(entry, 'applied', state.mod.frame,
        'GroupCommand drained; membership was accepted through the exported selection packet');
    }
    if (refusal.total === 0) {
      for (const entry of orderPackets) {
        settleCommandEntry(entry, 'applied', state.mod.frame,
          'packet drained and no exported refusal counter increased');
      }
    } else if (orderPackets.length === 1) {
      const status = ordersApplied > 0 ? 'partial' : 'refused';
      settleCommandEntry(orderPackets[0], status, state.mod.frame,
        `${refusal.reasons.join(' · ')}${ordersApplied > 0 ? ` · ${ordersApplied} object order(s) also applied` : ''}`);
    } else {
      for (const entry of orderPackets) {
        settleCommandEntry(entry, 'drained', state.mod.frame,
          'drained in a multi-packet tick; per-packet refusal attribution is not exported');
      }
      state.commandFeedback.entries.push({
        id: state.commandFeedback.nextId++, kind: 'batch', issuedFrame: state.mod.frame,
        drainedFrame: state.mod.frame, who: null, op: null, struct: 'tick refusal batch',
        bytes: 0, hex: '', selection: [], source: 'exported counters', status: 'refused',
        reason: `${refusal.reasons.join(' · ')} across ${orderPackets.length} non-group packets; attribution unavailable`,
      });
      state.commandFeedback.refusedTotal++;
    }
  } else if (refusal.total > 0) {
    state.commandFeedback.entries.push({
      id: state.commandFeedback.nextId++, kind: 'batch', issuedFrame: state.mod.frame,
      drainedFrame: state.mod.frame, who: null, op: null, struct: 'unattributed refusal counters',
      bytes: 0, hex: '', selection: [], source: 'exported counters', status: 'refused',
      reason: `${refusal.reasons.join(' · ')}; no newly drained observed browser packet`,
    });
    state.commandFeedback.refusedTotal++;
  }
  state.commandFeedback.lastTransport = { ...current };
  state.commandFeedback.lastGaps = currentGaps;
  trimCommandFeedback();
}

function trimCommandFeedback() {
  const entries = state.commandFeedback.entries;
  while (entries.length > 60) {
    const resolved = entries.findIndex((entry) => entry.status !== 'pending');
    if (resolved < 0) break;
    entries.splice(resolved, 1);
  }
}

function commandFeedbackSnapshot() {
  reconcileCommandFeedback();
  const feedback = state.commandFeedback;
  return Object.freeze({
    issued: feedback.issuedTotal,
    pending: feedback.entries.filter((entry) => entry.status === 'pending').length,
    applied: feedback.appliedTotal,
    refused: feedback.refusedTotal,
    drained: feedback.drainedTotal,
    entries: Object.freeze(feedback.entries.map((entry) => Object.freeze({
      ...entry, selection: entry.selection.slice(),
    }))),
    refusalAttribution: 'exact for a single non-group packet; batch-labelled otherwise',
  });
}

function renderCommandFeedback() {
  const host = $('command-history-list');
  if (!host || !state.mod || !state.commandFeedback.lastTransport) return;
  reconcileCommandFeedback();
  const snapshot = commandFeedbackSnapshot();
  $('command-history-summary').textContent =
    `${snapshot.issued} issued · ${snapshot.pending} pending · ${snapshot.applied} applied · ` +
    `${snapshot.refused} refused/partial`;
  host.replaceChildren();
  for (const entry of snapshot.entries.slice(-20).reverse()) {
    const record = document.createElement('div');
    record.className = 'command-record';
    record.dataset.status = entry.status;
    const head = document.createElement('div');
    head.className = 'command-head';
    const name = document.createElement('b');
    name.textContent = entry.kind === 'packet'
      ? `#${entry.id} P${entry.who} ${entry.struct}` : `#${entry.id} ${entry.struct}`;
    const status = document.createElement('span');
    status.className = 'command-state';
    status.textContent = entry.status;
    head.append(name, status);
    const detail = document.createElement('div');
    detail.className = 'command-detail';
    const frames = entry.drainedFrame === null
      ? `issued f${entry.issuedFrame}` : `issued f${entry.issuedFrame} → drained f${entry.drainedFrame}`;
    detail.textContent = entry.kind === 'packet'
      ? `${frames} · op 0x${entry.op.toString(16).padStart(2, '0')} · ${entry.bytes} B · ` +
        `${entry.source} · ${entry.hex.slice(0, 48)}${entry.hex.length > 48 ? '…' : ''} · ${entry.reason}`
      : `${frames} · ${entry.reason}`;
    record.append(head, detail);
    host.appendChild(record);
  }
  if (!snapshot.entries.length) {
    const empty = document.createElement('div');
    empty.className = 'hint';
    empty.textContent = 'No command packets issued in this session.';
    host.appendChild(empty);
  }
}

function initializeCommandFeedbackPanel() {
  resetCommandFeedback();
}

// ---------------------------------------------------------------------------------------
// deterministic browser command journal
// ---------------------------------------------------------------------------------------

function bytesToHex(bytes) {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

function hexToBytes(hex) {
  if (typeof hex !== 'string' || !/^(?:[0-9a-f]{2})+$/i.test(hex)) {
    throw new Error('journal command hex must contain one or more complete bytes');
  }
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < bytes.length; i++) bytes[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return bytes;
}

function replayBaseline() {
  return Object.freeze({
    seed: formatSeed(state.sessionSeed),
    player: state.who,
    income: INCOME_MODES[state.sessionIncomeMode].slug,
    population: POPULATION_LIMITS[state.sessionPopSetting],
    initialDigest: state.sessionInitialDigest,
  });
}

function startReplayJournal() {
  state.replay.events = [];
  state.replay.baseline = replayBaseline();
  state.replay.headFrame = state.mod.frame;
  state.replay.applying = false;
  state.replay.playback = false;
  state.replay.restoring = false;
  state.replay.status = 'recording exact browser command packets from this new-session baseline';
  state.mod.observeCommands(({ frame: at, who, bytes }) => {
    recordCommandIssued({ frame: at, who, bytes }, state.replay.applying ? 'journal replay' : 'player');
    if (state.replay.applying) return;
    recordReplayEvent({
      frame: at,
      kind: 'command',
      who,
      hex: bytesToHex(bytes),
      selection: state.selection.slice(),
    });
  });
  renderReplayPanel();
}

function recordReplayEvent(event) {
  const frame = state.mod.frame;
  if (state.replay.playback || frame < state.replay.headFrame) {
    // A restore leaves the world at a tick boundary before that frame's command packets.
    // Immediate rule setters at the boundary have already been reconstructed and remain
    // part of a new branch; queued command packets are the future being replaced.
    state.replay.events = state.replay.events.filter(
      (entry) => entry.frame < frame || (entry.frame === frame && entry.kind !== 'command'));
    state.replay.headFrame = frame;
    state.replay.playback = false;
    state.replay.status = `branched at frame ${frame}; later journal packets were discarded`;
  }
  state.replay.events.push(Object.freeze({ ...event, frame }));
  state.replay.headFrame = Math.max(state.replay.headFrame, frame);
  renderReplayPanel();
}

function recordReplayRule(kind, value) {
  if (state.replay.applying || !state.replay.baseline) return;
  recordReplayEvent({ frame: state.mod.frame, kind, value });
}

function updateReplayHead() {
  if (!state.replay.playback && !state.replay.restoring) {
    state.replay.headFrame = Math.max(state.replay.headFrame, state.mod.frame);
  }
}

function replayDocument() {
  updateReplayHead();
  return {
    protocol: JOURNAL_PROTOCOL,
    boundary: 'tick-boundary deterministic restart plus exact frame-stamped command packets; not a native save',
    setup: { ...state.replay.baseline },
    frame: state.mod.frame,
    headFrame: state.replay.headFrame,
    events: state.replay.events.map((event) => ({
      ...event,
      selection: event.selection ? event.selection.slice() : undefined,
    })),
  };
}

function exportReplayJournal() {
  const documentValue = replayDocument();
  if (documentValue.headFrame > MAX_JOURNAL_FRAMES || documentValue.events.length > MAX_JOURNAL_EVENTS) {
    throw new Error(`journal exceeds the ${MAX_JOURNAL_FRAMES}-frame or ${MAX_JOURNAL_EVENTS}-event bound`);
  }
  const text = `${JSON.stringify(documentValue, null, 2)}\n`;
  if (new TextEncoder().encode(text).length > MAX_JOURNAL_JSON_BYTES) {
    throw new Error(`journal JSON exceeds ${MAX_JOURNAL_JSON_BYTES} bytes`);
  }
  return text;
}

function normalizeReplayJournal(input) {
  if (typeof input === 'string' && new TextEncoder().encode(input).length > MAX_JOURNAL_JSON_BYTES) {
    throw new Error(`journal JSON exceeds ${MAX_JOURNAL_JSON_BYTES} bytes`);
  }
  let documentValue;
  try {
    documentValue = typeof input === 'string' ? JSON.parse(input) : input;
  } catch (error) {
    throw new Error(`journal is not valid JSON: ${error.message}`);
  }
  if (!documentValue || typeof documentValue !== 'object' || Array.isArray(documentValue)) {
    throw new Error('journal root must be an object');
  }
  if (documentValue.protocol !== JOURNAL_PROTOCOL) {
    throw new Error(`unsupported journal protocol ${JSON.stringify(documentValue.protocol)}`);
  }
  const setup = documentValue.setup;
  if (!setup || typeof setup !== 'object' || Array.isArray(setup)) throw new Error('journal setup is missing');
  const seed = parseSessionSeed(setup.seed);
  const player = Number(setup.player);
  if (!Number.isInteger(player) || player < 0 || player >= state.mod.playerCount) {
    throw new Error('journal player is outside the exported player range');
  }
  const incomeMode = INCOME_MODES.find((mode) => mode.slug === setup.income);
  if (!incomeMode) throw new Error('journal income mode is unsupported');
  const popIndex = POPULATION_LIMITS.indexOf(Number(setup.population));
  if (popIndex < 0) throw new Error('journal population limit is unsupported');
  if (typeof setup.initialDigest !== 'string' || !/^[0-9a-f]{16}$/i.test(setup.initialDigest)) {
    throw new Error('journal initial digest must be 16 hexadecimal digits');
  }
  const headFrame = Number(documentValue.headFrame);
  const targetFrame = Number(documentValue.frame);
  if (!Number.isInteger(headFrame) || headFrame < 0 || headFrame > MAX_JOURNAL_FRAMES ||
      !Number.isInteger(targetFrame) || targetFrame < 0 || targetFrame > headFrame) {
    throw new Error(`journal frame bounds must satisfy 0 <= frame <= head <= ${MAX_JOURNAL_FRAMES}`);
  }
  if (!Array.isArray(documentValue.events) || documentValue.events.length > MAX_JOURNAL_EVENTS) {
    throw new Error(`journal events must be an array of at most ${MAX_JOURNAL_EVENTS} entries`);
  }
  let priorFrame = -1;
  let commandBytes = 0;
  const events = documentValue.events.map((entry, index) => {
    if (!entry || typeof entry !== 'object' || Array.isArray(entry)) {
      throw new Error(`journal event ${index} must be an object`);
    }
    const frame = Number(entry.frame);
    if (!Number.isInteger(frame) || frame < priorFrame || frame < 0 || frame > headFrame) {
      throw new Error(`journal event ${index} has an invalid or out-of-order frame`);
    }
    priorFrame = frame;
    if (entry.kind === 'income') {
      const mode = INCOME_MODES.find((candidate) => candidate.slug === entry.value);
      if (!mode) throw new Error(`journal event ${index} has an unsupported income mode`);
      return Object.freeze({ frame, kind: 'income', value: mode.slug });
    }
    if (entry.kind === 'population') {
      const population = Number(entry.value);
      if (!POPULATION_LIMITS.includes(population)) {
        throw new Error(`journal event ${index} has an unsupported population limit`);
      }
      return Object.freeze({ frame, kind: 'population', value: population });
    }
    if (entry.kind !== 'command') throw new Error(`journal event ${index} has an unknown kind`);
    const who = Number(entry.who);
    if (!Number.isInteger(who) || who < 0 || who >= state.mod.playerCount) {
      throw new Error(`journal event ${index} has an invalid player`);
    }
    const bytes = hexToBytes(entry.hex);
    commandBytes += bytes.length;
    if (commandBytes > MAX_JOURNAL_COMMAND_BYTES) {
      throw new Error(`journal command payloads exceed ${MAX_JOURNAL_COMMAND_BYTES} bytes`);
    }
    if (bytes.length > state.mod.views().cmd.length) {
      throw new Error(`journal event ${index} exceeds the exported command buffer`);
    }
    let decoded;
    try { decoded = decode(bytes); } catch (error) {
      throw new Error(`journal event ${index} is not a supported wire packet: ${error.message}`);
    }
    const command = COMMANDS[decoded.op];
    const expectedBytes = decoded.op === OP.GROUP ? 3 + (decoded.num * 2) : command?.size;
    if (!command || bytes.length !== expectedBytes) {
      throw new Error(`journal event ${index} has an unknown opcode or non-canonical packet length`);
    }
    if (!Array.isArray(entry.selection) || entry.selection.length > 255 ||
        entry.selection.some((id) => !Number.isInteger(id) || id < 0 || id > 0x7fffffff)) {
      throw new Error(`journal event ${index} has an invalid selection snapshot`);
    }
    return Object.freeze({
      frame, kind: 'command', who, hex: bytesToHex(bytes), selection: entry.selection.slice(),
    });
  });
  const normalized = Object.freeze({
    protocol: JOURNAL_PROTOCOL,
    setup: Object.freeze({
      seed: formatSeed(seed), player, income: incomeMode.slug,
      population: POPULATION_LIMITS[popIndex], initialDigest: setup.initialDigest.toLowerCase(),
    }),
    frame: targetFrame,
    headFrame,
    events: Object.freeze(events),
  });
  const observedDigest = scratchReplayBaselineDigest(normalized.setup);
  if (observedDigest !== normalized.setup.initialDigest) {
    throw new Error(
      `journal baseline digest mismatch: expected ${normalized.setup.initialDigest}, got ${observedDigest}`);
  }
  return normalized;
}

function scratchReplayBaselineDigest(setup) {
  const x = state.mod.x;
  const game = x.game_create(parseSessionSeed(setup.seed), 0);
  if (!game) throw new Error('could not allocate a scratch world to validate the journal baseline');
  try {
    for (const player of state.mod.activePlayers()) {
      if (x.game_activate_player(game, player) !== 1) {
        throw new Error(`scratch world refused active roster slot ${player}`);
      }
    }
    x.game_set_income_mode(game, INCOME_MODES.find((mode) => mode.slug === setup.income).value);
    x.game_set_pop_setting(game, POPULATION_LIMITS.indexOf(setup.population));
    x.game_step(game, 0);
    const lo = x.game_digest_lo(game) >>> 0;
    const hi = x.game_digest_hi(game) >>> 0;
    return hi.toString(16).padStart(8, '0') + lo.toString(16).padStart(8, '0');
  } finally {
    x.game_destroy(game);
    // The scratch allocation can grow linear memory and detach every cached view.
    state.mod._buf = null;
  }
}

function applyReplayEventsAt(frame) {
  for (const event of state.replay.events) {
    if (event.frame < frame) continue;
    if (event.frame > frame) break;
    if (event.kind === 'command') {
      state.selection = event.selection.slice();
      state.mod.submit(event.who, hexToBytes(event.hex));
    } else if (event.kind === 'income') {
      state.sessionIncomeMode = INCOME_MODES.find((mode) => mode.slug === event.value).value;
      state.mod.setIncomeMode(state.sessionIncomeMode);
    } else if (event.kind === 'population') {
      state.sessionPopSetting = POPULATION_LIMITS.indexOf(event.value);
      state.mod.setPopSetting(state.sessionPopSetting);
    }
  }
}

async function restoreReplayFrame(targetFrame) {
  const target = Number(targetFrame);
  if (state.replay.restoring) throw new Error('a journal restore is already running');
  if (!Number.isInteger(target) || target < 0 || target > state.replay.headFrame) {
    throw new Error(`journal target must be between 0 and ${state.replay.headFrame}`);
  }
  const setup = state.replay.baseline;
  state.replay.restoring = true;
  state.replay.applying = true;
  state.replay.playback = false;
  state.replay.status = `restoring frame ${target} from the deterministic baseline…`;
  setPaused(true, false);
  renderReplayPanel();
  try {
    state.who = setup.player;
    state.sessionIncomeMode = INCOME_MODES.find((mode) => mode.slug === setup.income).value;
    state.sessionPopSetting = POPULATION_LIMITS.indexOf(setup.population);
    if (!state.mod.restart(parseSessionSeed(setup.seed))) throw new Error('Wasm world restart failed');
    resetClientForWorld(parseSessionSeed(setup.seed), true, `journal frame ${target}`);
    if (state.sessionInitialDigest !== setup.initialDigest) {
      throw new Error(`baseline digest mismatch: expected ${setup.initialDigest}, got ${state.sessionInitialDigest}`);
    }
    for (let frame = 0; frame < target; frame++) {
      applyReplayEventsAt(frame);
      state.mod.step(1);
      reconcileCommandFeedback();
      if (frame && frame % 2048 === 0) await new Promise(requestAnimationFrame);
    }
    // Rule setters are immediate and idempotent. Reconstruct them at the selected boundary;
    // command packets at this same frame stay queued in the journal until play/step.
    for (const event of state.replay.events) {
      if (event.frame > target) break;
      if (event.frame !== target || event.kind === 'command') continue;
      if (event.kind === 'income') {
        state.sessionIncomeMode = INCOME_MODES.find((mode) => mode.slug === event.value).value;
        state.mod.setIncomeMode(state.sessionIncomeMode);
      } else if (event.kind === 'population') {
        state.sessionPopSetting = POPULATION_LIMITS.indexOf(event.value);
        state.mod.setPopSetting(state.sessionPopSetting);
      }
    }
    const lastSelection = [...state.replay.events].reverse().find(
      (event) => event.kind === 'command' && event.frame <= target);
    state.selection = lastSelection ? lastSelection.selection.slice() : [];
    state.replay.playback = true;
    state.replay.status = target === state.replay.headFrame
      ? `journal head restored at frame ${target}; resume drains packets recorded at this boundary`
      : `journal frame ${target} restored; play or step replays exact packets toward head ${state.replay.headFrame}`;
    $('session-player').value = String(state.who);
    $('income').value = String(state.sessionIncomeMode);
    $('popset').value = String(state.sessionPopSetting);
    $('session-seed').value = setup.seed;
    syncSessionUrl();
    renderMenus();
    renderSelection();
    renderSessionSummary();
    return replaySnapshot();
  } finally {
    state.replay.applying = false;
    state.replay.restoring = false;
    renderReplayPanel();
  }
}

async function importReplayJournal(input) {
  const journal = normalizeReplayJournal(input);
  const previous = {
    events: state.replay.events,
    baseline: state.replay.baseline,
    headFrame: state.replay.headFrame,
    status: state.replay.status,
  };
  state.replay.events = journal.events.slice();
  state.replay.baseline = journal.setup;
  state.replay.headFrame = journal.headFrame;
  try {
    const snapshot = await restoreReplayFrame(journal.frame);
    state.replay.status = `imported ${journal.events.length} events; ${state.replay.status}`;
    renderReplayPanel();
    return snapshot;
  } catch (error) {
    state.replay.events = previous.events;
    state.replay.baseline = previous.baseline;
    state.replay.headFrame = previous.headFrame;
    state.replay.status = `import refused: ${error.message}`;
    renderReplayPanel();
    throw error;
  }
}

function advanceSimulationFrame() {
  if (state.replay.playback) {
    state.replay.applying = true;
    try { applyReplayEventsAt(state.mod.frame); } finally { state.replay.applying = false; }
  }
  state.mod.step(1);
  reconcileCommandFeedback();
  if (state.replay.playback && state.mod.frame > state.replay.headFrame) {
    state.replay.playback = false;
    state.replay.headFrame = state.mod.frame;
    state.replay.status = `journal head passed; recording resumed at frame ${state.mod.frame}`;
  } else if (!state.replay.playback) {
    state.replay.headFrame = Math.max(state.replay.headFrame, state.mod.frame);
  }
}

function replaySnapshot() {
  updateReplayHead();
  return Object.freeze({
    protocol: JOURNAL_PROTOCOL,
    frame: state.mod.frame,
    headFrame: state.replay.headFrame,
    events: state.replay.events.length,
    playback: state.replay.playback,
    paused: state.paused,
    digest: state.mod.digest(),
    initialDigest: state.replay.baseline?.initialDigest ?? '',
    status: state.replay.status,
  });
}

function renderReplayPanel() {
  if (!$('replay') || !state.mod || !state.replay.baseline) return;
  updateReplayHead();
  const frame = state.mod.frame;
  const timeline = $('replay-timeline');
  timeline.max = String(state.replay.headFrame);
  timeline.value = String(Math.min(frame, state.replay.headFrame));
  timeline.disabled = state.replay.restoring;
  $('replay-frame').textContent = `frame ${frame}`;
  $('replay-head').textContent = `head ${state.replay.headFrame} · ${state.replay.events.length} events`;
  $('replay-status').textContent = `${state.replay.status}. This is a command journal, not a native save-state.`;
  $('replay-play').textContent = state.paused
    ? (state.replay.playback ? 'play journal' : 'resume') : 'pause';
  $('replay-step').disabled = state.replay.restoring;
  $('replay-live').disabled = state.replay.restoring || frame === state.replay.headFrame;
  $('replay-import').disabled = state.replay.restoring;
  $('replay-export').disabled = state.replay.restoring;
  $('replay-speed').value = String(state.speed);
}

function downloadReplayJournal() {
  let journal;
  try { journal = exportReplayJournal(); }
  catch (error) {
    state.replay.status = `export refused: ${error.message}`;
    say(`command journal export refused — ${error.message}`, 'warn');
    renderReplayPanel();
    return;
  }
  const blob = new Blob([journal], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = `don-command-journal-${state.mod.frame}.json`;
  anchor.click();
  setTimeout(() => URL.revokeObjectURL(url), 0);
  state.replay.status = `exported ${state.replay.events.length} events at frame ${state.mod.frame}`;
  renderReplayPanel();
}

function coreSaveSnapshot() {
  return {
    supported: state.mod.supports('save') && state.mod.supports('load'),
    frame: state.mod.frame,
    digest: state.mod.digest(),
    rngState: state.mod.rngState,
    status: state.coreSaveStatus,
  };
}

function exportCoreSave() {
  try {
    const bytes = state.mod.saveCore();
    state.coreSaveStatus = `saved ${bytes.length.toLocaleString()} authoritative bytes · ` +
      `frame ${state.mod.frame} · digest ${state.mod.digest()} · RNG 0x${state.mod.rngState.toString(16).padStart(8, '0')}`;
    $('core-save-status').textContent = state.coreSaveStatus;
    return bytes;
  } catch (error) {
    state.coreSaveStatus = error.message;
    $('core-save-status').textContent = state.coreSaveStatus;
    throw error;
  }
}

function downloadCoreSave() {
  try {
    const bytes = exportCoreSave();
    const blob = new Blob([bytes], { type: 'application/octet-stream' });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = `don-core-frame-${state.mod.frame}.donsave`;
    anchor.click();
    setTimeout(() => URL.revokeObjectURL(url), 0);
    say(`core save downloaded — ${bytes.length.toLocaleString()} bytes`, 'ok');
  } catch (error) {
    say(`core save refused — ${error.message}`, 'warn');
  }
}

function importCoreSave(input) {
  try {
    const result = state.mod.loadCore(input);
    const seed = state.mod.coreSeed;
    resetClientForWorld(seed, true, 'loaded core save');
    startReplayJournal();
    $('session-seed').value = formatSeed(seed);
    syncSessionUrl();
    state.coreSaveStatus = `loaded ${result.bytes.toLocaleString()} authoritative bytes · ` +
      `frame ${result.frame} · digest ${result.digest} · RNG 0x${result.rngState.toString(16).padStart(8, '0')} · paused`;
    $('core-save-status').textContent = state.coreSaveStatus;
    say(`core save loaded — frame ${result.frame}; browser selection and command journal reset`, 'ok');
    return { ...result, selection: state.selection.length, groups: state.groups.size, paused: state.paused };
  } catch (error) {
    state.coreSaveStatus = error.message;
    $('core-save-status').textContent = state.coreSaveStatus;
    say(`core load refused — ${error.message}`, 'warn');
    throw error;
  }
}

function initializeReplayPanel() {
  startReplayJournal();
  $('replay-play').addEventListener('click', () => setPaused(!state.paused));
  $('replay-step').addEventListener('click', () => {
    setPaused(true, false);
    const wasPlayback = state.replay.playback;
    advanceSimulationFrame();
    if (!wasPlayback || state.replay.playback) state.replay.status = `stepped to frame ${state.mod.frame}`;
    renderHud();
  });
  $('replay-live').addEventListener('click', async () => {
    try { await restoreReplayFrame(state.replay.headFrame); }
    catch (error) { say(`journal seek failed — ${error.message}`, 'warn'); }
  });
  $('replay-speed').addEventListener('change', (event) => setSpeed(Number(event.target.value)));
  $('replay-timeline').addEventListener('change', async (event) => {
    try { await restoreReplayFrame(Number(event.target.value)); }
    catch (error) { say(`journal seek failed — ${error.message}`, 'warn'); }
  });
  $('replay-export').addEventListener('click', downloadReplayJournal);
  $('replay-import').addEventListener('click', () => $('replay-file').click());
  $('replay-file').addEventListener('change', async (event) => {
    const file = event.target.files?.[0];
    event.target.value = '';
    if (!file) return;
    try {
      await importReplayJournal(await file.text());
      say(`command journal imported — frame ${state.mod.frame}`, 'ok');
    } catch (error) {
      say(`command journal refused — ${error.message}`, 'warn');
    }
  });
  $('core-save').disabled = !state.mod.supports('save');
  $('core-load').disabled = !state.mod.supports('load');
  $('core-save').addEventListener('click', downloadCoreSave);
  $('core-load').addEventListener('click', () => $('core-load-file').click());
  $('core-load-file').addEventListener('change', async (event) => {
    const file = event.target.files?.[0];
    event.target.value = '';
    if (!file) return;
    try { importCoreSave(new Uint8Array(await file.arrayBuffer())); }
    catch { /* status and toast are set by importCoreSave */ }
  });
  $('core-save-status').textContent = state.coreSaveStatus;
  renderReplayPanel();
}

function wirePanels() {
  for (const id of ['tab-build', 'tab-train', 'tab-research']) {
    $(id).addEventListener('click', () => {
      const previousMode = paletteMode();
      document.querySelectorAll('.tab').forEach((t) => t.classList.remove('sel'));
      $(id).classList.add('sel');
      if ($(id).dataset.mode !== previousMode) $('palette-filter').value = '';
      if (id !== 'tab-build') state.buildType = null;
      renderMenus();
    });
  }
  $('income').addEventListener('change', (e) => {
    state.sessionIncomeMode = Number(e.target.value) === 1 ? 1 : 0;
    state.mod.setIncomeMode(state.sessionIncomeMode);
    recordReplayRule('income', INCOME_MODES[state.sessionIncomeMode].slug);
    syncSessionUrl();
    renderSessionSummary();
    say(`income mode: ${e.target.selectedOptions[0].textContent}`, 'hi');
  });
  $('popset').addEventListener('change', (e) => {
    state.sessionPopSetting = clamp(Number(e.target.value) | 0, 0, POPULATION_LIMITS.length - 1);
    state.mod.setPopSetting(state.sessionPopSetting);
    recordReplayRule('population', POPULATION_LIMITS[state.sessionPopSetting]);
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
    $('side').scrollIntoView({
      behavior: state.settings.visual.reducedMotion ? 'auto' : 'smooth', block: 'start',
    });
  }
}

function zoomCentre(factor) {
  const r = $('gl').getBoundingClientRect();
  zoomAt(r.left + r.width / 2, r.top + r.height / 2, factor);
}

function setPaused(paused, announce = true) {
  state.paused = paused;
  const el = $('pause');
  if (el) {
    const binding = state.settings ? formatBinding(state.settings.input.bindings.pause) : 'P';
    el.textContent = `${paused ? 'resume' : 'pause'} (${binding})`;
  }
  const dock = $('cmd-pause');
  if (dock) {
    dock.firstElementChild.textContent = paused ? 'resume' : 'pause';
    dock.classList.toggle('active', paused);
    dock.setAttribute('aria-pressed', String(paused));
  }
  if (announce) {
    say(paused ? 'simulation paused — commands remain queued for the next tick' : 'simulation resumed');
  }
  renderReplayPanel();
}

function setSpeed(speed) {
  state.speed = clamp(Number(speed) || 1, 0.25, 8);
  const el = $('speed');
  if (el && Number(el.value) !== state.speed) el.value = String(state.speed);
  const replaySpeed = $('replay-speed');
  if (replaySpeed && [...replaySpeed.options].some((option) => Number(option.value) === state.speed)) {
    replaySpeed.value = String(state.speed);
  }
  renderReplayPanel();
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
  state.paletteAge = ctx.me.age;

  if (mode === 'build') {
    const workers = ctx.mobiles.filter((info) => info.typeId === 50 || info.typeId === 51);
    $('palette-context').textContent = workers.length
      ? `${workers.length} selected Citizen worker(s) · age ${ctx.me.age} · ` +
        'Library construction uses the authoritative Sim BuildAt transaction; other buildings remain inspect-only'
      : 'select a Citizen worker · only Library construction is executable in the authoritative core';
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
        'exact WHERE edges queue through the authoritative Sim production runtime'
      : 'select a completed producer to train through the authoritative Sim queue';
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
    const libraries = ctx.producers.filter((producer) => producer.typeId === 435);
    $('palette-context').textContent = libraries.length
      ? `player age ${ctx.me.age} · selected completed Library queues the next age through the authoritative tech runtime`
      : `player age ${ctx.me.age} · select a completed Library to research the next age`;
    const age = state.play?.ages?.[ctx.me.age];
    if (age && ctx.me.age < 7) {
      paletteItems.push({
        kind: 'research', id: age.id, name: age.name, cost: age.cost,
        age: ctx.me.age, jobTime: age.jobTime,
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
    if (!state.mod.supports('build')) reasons.push('authoritative building capability unavailable');
    if (it.id !== 435) reasons.push('only Library construction is live');
    if (!ctx.mobiles.some((info) => info.typeId === 50 || info.typeId === 51)) {
      reasons.push('select a Citizen worker');
    }
    if (it.age > ctx.me.age) reasons.push(`requires age ${it.age}`);
  } else if (it.kind === 'train') {
    const producers = ctx.producers.filter((p) => p.typeId === it.producerType);
    if (!producers.length) reasons.push(`select ${typeName(it.producerType)}`);
    else if (producers.every((p) => p.queueN >= QUEUE_CAPACITY)) reasons.push('producer queue full');
    if (it.age > ctx.me.age) reasons.push(`requires age ${it.age}`);
    if (ctx.me.popCap > 0 && ctx.me.pop + Math.max(0, it.pop) > ctx.me.popCap) {
      reasons.push('population capped');
    }
    if (!state.mod.supports('train')) reasons.push('authoritative training capability unavailable');
  } else if (it.kind === 'research') {
    if (!state.mod.supports('research')) reasons.push('authoritative research capability unavailable');
    if (!ctx.producers.some((producer) => producer.typeId === 435)) {
      reasons.push('select a completed Library');
    }
    if (it.age !== ctx.me.age) reasons.push('age changed; refresh the research catalog');
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
  if (it.kind === 'research') {
    const queues = ctx.producers.filter((p) => p.typeId === 435).map((p) => p.queueN);
    detail = `Library · queue ${queues.length ? Math.min(...queues) : 0}/${QUEUE_CAPACITY}`;
  }
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
    const jobTime = Math.max(1, state.play?.ages?.[ctx.me.age]?.jobTime ?? 1);
    message = `age research active · ${Math.min(100, Math.floor(ctx.me.research / jobTime * 100))}%`;
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
    ?? state.play?.ages?.find((age) => age.id === id)?.name
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
    'don_sim::tick::Sim — authoritative local core projected through the playable WASM ABI; don_ai::arena::World connected: no';
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
  localSlug.textContent = 'web-core-adapter-incomplete';
  local.append(localSlug, document.createTextNode(
    ' — the browser now owns don_sim::Sim, including bounded Library construction, sequential age research, and resumable post-step core saves; other buildings/tech remain fail-closed'));
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
  const researchJobTime = Math.max(1, state.play?.ages?.[me.age]?.jobTime ?? 1);
  $('meta').textContent =
    `P${state.who}   ${formatSeed(state.sessionSeed)}   pop ${me.pop}/${me.popCap}   ${ageName}` +
    (me.research ? `   researching ${(me.research / researchJobTime * 100) | 0}%` : '');

  const stat = $('stat');
  stat.textContent =
    `${state.fps.toFixed(0)} fps   ${m.live} objects   frame ${m.frame} ` +
    `(${(m.frame * TICK_MS / 1000).toFixed(0)} s)   ` +
    `step ${state.stepMs.toFixed(2)} ms   upload ${state.uploadMs.toFixed(2)} ms   ` +
    `x${state.speed}   visual ${state.settings.performance.fpsCap || 'display'} fps cap` +
    `${state.paused ? '  PAUSED' : ''}`;

  renderSelection();
  if (paletteMode() === 'research' && state.paletteAge !== me.age) renderMenus();
  else refreshPaletteAvailability();
  renderPaletteFeedback();
  renderSessionStatus();
  renderObjectivesPanel();
  renderReplayPanel();
  renderControlGroups();
  renderCoverage();
  renderTransport();
  renderCommandFeedback();
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
  for (const mode of ['move', 'attack', 'gather']) {
    const el = $(`cmd-${mode}`);
    if (!el) continue;
    const active = state.commandMode === mode;
    el.classList.toggle('active', active);
    el.setAttribute('aria-pressed', String(active));
    const supported = mode !== 'gather' && state.mod.supports(mode);
    el.disabled = selected === 0 || !supported;
    if (!supported) el.title = `${mode} unavailable in the authoritative don_sim adapter`;
  }
  for (const id of ['cmd-halt']) if ($(id)) $(id).disabled = selected === 0;
  const build = $('cmd-build');
  if (build) {
    const active = state.buildType !== null;
    build.classList.toggle('active', active);
    build.setAttribute('aria-pressed', String(active));
    const workers = selectedPaletteContext().mobiles
      .filter((info) => info.typeId === 50 || info.typeId === 51);
    const supported = state.mod.supports('build');
    build.disabled = !supported || workers.length === 0;
    build.title = supported
      ? (workers.length
        ? 'Open building catalog; only authoritative Library construction is enabled'
        : 'Select a Citizen worker to construct a Library')
      : 'Build unavailable in the authoritative don_sim adapter';
  }
  const train = $('cmd-train');
  if (train) {
    const producers = selectedPaletteContext().producers;
    const supported = state.mod.supports('train');
    train.disabled = !supported || producers.length === 0;
    train.title = supported
      ? (producers.length
        ? 'Open authoritative training actions for the selected producer'
        : 'Select a completed producer to train')
      : 'Train unavailable in the authoritative don_sim adapter';
  }
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
function ping(w, colour) {
  if (state.settings.visual.reducedMotion) return;
  pings.push({ x: w[0], y: w[1], t: performance.now(), colour });
}

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
  const colours = ownerColours();
  for (let i = 0; i < live; i++) {
    const tag = v.tag[i];
    if ((tag & 0x80000000) === 0) continue;
    g.fillStyle = colours[(tag & 0xf) % colours.length];
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
    g.strokeStyle = colours[p % colours.length];
    g.lineWidth = p === state.who ? 2 : 1;
    g.strokeRect(sx - 4, sy - 4, 8, 8);
    g.fillStyle = colours[p % colours.length];
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

let acc = 0, last = performance.now(), visualLast = 0, fpsAcc = 0, fpsN = 0, hudAt = 0;

function frame(now) {
  const dt = Math.min(now - last, 200);
  last = now;

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
    while (acc >= TICK_MS && steps < 16) { advanceSimulationFrame(); acc -= TICK_MS; steps++; }
    if (steps) state.stepMs = (performance.now() - t0) / steps;
  }

  // This cap throttles only visual uploads/draws. Input, camera movement, and the recovered
  // 67 ms simulation clock above continue on every animation callback.
  const fpsCap = state.settings.performance.fpsCap;
  const visualInterval = fpsCap ? 1000 / fpsCap : 0;
  if (visualLast && visualInterval && now - visualLast < visualInterval - 1) {
    requestAnimationFrame(frame);
    return;
  }
  const visualDt = visualLast ? now - visualLast : dt;
  visualLast = now;
  fpsAcc += visualDt; fpsN++;
  if (fpsAcc > 400) { state.fps = 1000 * fpsN / fpsAcc; fpsAcc = 0; fpsN = 0; }

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
    activate: () => activateSessionRoster(),
  },
  objectives: {
    snapshot: () => exportedWorldSnapshot(),
    camera: () => cameraSnapshot(),
    focusPlayer: (player) => focusPlayerStart(player, 'automation/player panel'),
  },
  controlGroups: {
    snapshot: () => controlGroupsSnapshot(),
    replace: (slot, ids = state.selection) => replaceControlGroup(slot, ids),
    add: (slot, ids = state.selection) => addToControlGroup(slot, ids),
    remove: (slot, ids = state.selection) => removeFromControlGroup(slot, ids),
    recall: (slot) => recallControlGroup(slot),
    clear: (slot) => clearControlGroup(slot),
  },
  commands: {
    snapshot: () => commandFeedbackSnapshot(),
  },
  settings: {
    snapshot: () => settingsSnapshot(),
    export: () => exportBrowserSettings(),
    import: (input) => importBrowserSettings(input),
    apply: (input) => applyBrowserSettings(input),
    reset: () => resetBrowserSettings(),
    reloadStored: () => applyBrowserSettings(loadBrowserSettings(), {
      persist: false, status: 'reloaded persisted browser settings',
    }),
  },
  save: {
    snapshot: () => coreSaveSnapshot(),
    export: () => exportCoreSave(),
    import: (bytes) => importCoreSave(bytes),
  },
  replay: {
    snapshot: () => replaySnapshot(),
    export: () => exportReplayJournal(),
    import: (journal) => importReplayJournal(journal),
    seek: (frame) => restoreReplayFrame(frame),
    step() {
      setPaused(true, false);
      advanceSimulationFrame();
      renderReplayPanel();
      renderCommandFeedback();
      return replaySnapshot();
    },
    play() { setPaused(false, false); return replaySnapshot(); },
    pause() { setPaused(true, false); return replaySnapshot(); },
  },
  activate,
  info: (id) => state.mod.info(id),
  player: (p = 0) => state.mod.player(p),
  gaps: () => state.mod.gaps(),
  readiness: () => ({
    runtime: 'don_sim::tick::Sim via web::wasm::game_abi',
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
    objectives: exportedWorldSnapshot(), camera: cameraSnapshot(), replay: replaySnapshot(),
    save: coreSaveSnapshot(),
    controlGroups: controlGroupsSnapshot(), commands: commandFeedbackSnapshot(),
    settings: settingsSnapshot(),
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
    for (const player of state.mod.activePlayers()) {
      if (x.game_activate_player(g, player) !== 1) {
        x.game_destroy(g);
        throw new Error(`fresh digest world refused active roster slot ${player}`);
      }
    }
    x.game_step(g, frames);
    const lo = x.game_digest_lo(g) >>> 0, hi = x.game_digest_hi(g) >>> 0;
    const live = x.game_live(g);
    x.game_destroy(g);
    // Creating a second game can grow linear memory, which detaches every view.
    state.mod._buf = null;
    return { seed, frames, live, digest: hi.toString(16).padStart(8, '0') + lo.toString(16).padStart(8, '0') };
  },
  key(code, modifiers = {}) {
    onKeyDown({
      code,
      key: code.replace('Key', ''),
      shiftKey: !!modifiers.shiftKey,
      ctrlKey: !!modifiers.ctrlKey,
      metaKey: !!modifiers.metaKey,
      altKey: !!modifiers.altKey,
      preventDefault() {},
      target: {},
    });
    // This automation surface represents one complete keystroke. Keeping the key in the
    // physical-key set after returning makes camera keys pan forever and diverges from what
    // a human press/release does.
    keys.delete(code);
  },

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
