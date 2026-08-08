// rontoy — a read-only live economy coach.
//
// The UI consumes a deliberately small, versioned snapshot envelope. It defaults to a
// deterministic demo so layout and coaching behavior can be exercised before live memory
// reads are available. Pass `?ws=ws://host:port/path` to consume one JSON snapshot per
// WebSocket message. All strings from telemetry reach the page through `textContent`.

const SCHEMA = 'rontoy.snapshot';
const VERSION = 1;
const STALE_AFTER_MS = 4_000;
const DEAD_AFTER_MS = 15_000;
const COLORS = ['#55d6d0', '#64b5f6', '#b89cff', '#ffbd69', '#77dfa8', '#8896a3', '#ff776d'];

const byId = (id) => document.getElementById(id);
const el = Object.fromEntries([
  'playerName', 'gamePhase', 'sourceBadge', 'sourceText', 'freshBadge', 'freshText', 'reconnect',
  'advicePriority', 'adviceConfidence', 'adviceConfidenceBar', 'adviceHeadline', 'adviceDetail',
  'adviceActions', 'incomeWindow', 'resources', 'workerSummary', 'allocation', 'workerList',
  'capStatus', 'popUsed', 'popCap', 'popTrend', 'popBar', 'popFree', 'popForecast', 'queueSummary',
  'queues', 'gameClock', 'timeline', 'sequence', 'overallConfidence', 'snapshotAge',
  'frameCoherence', 'rateCacheAge', 'transportState', 'footerSource', 'footerErrors',
].map((id) => [id, byId(id)]));

const state = {
  snapshot: null,
  mode: 'starting',
  transport: 'starting',
  source: 'none',
  renders: 0,
  accepted: 0,
  rejected: 0,
  reconnects: 0,
  lastError: '',
  wsUrl: null,
  socket: null,
  reconnectTimer: 0,
  reconnectAttempt: 0,
  mockTimer: 0,
};

const finite = (value, fallback = 0) => Number.isFinite(Number(value)) ? Number(value) : fallback;
const bounded = (value, lo = 0, hi = 1) => Math.max(lo, Math.min(hi, finite(value)));
const str = (value, fallback = '—') => value === undefined || value === null ? fallback : String(value);
const replaceChildren = (node, children) => node.replaceChildren(...children);
const node = (tag, className, text) => {
  const n = document.createElement(tag);
  if (className) n.className = className;
  if (text !== undefined) n.textContent = str(text);
  return n;
};

function formatNumber(value) {
  const n = finite(value);
  return Math.abs(n) >= 1000 ? new Intl.NumberFormat('en', { maximumFractionDigits: 0 }).format(n)
    : (Math.round(n * 10) / 10).toFixed(Math.abs(n) < 100 && n % 1 ? 1 : 0);
}

function formatRate(value) {
  const n = finite(value);
  return `${n >= 0 ? '+' : '−'}${Math.abs(n).toFixed(1)}/m`;
}

function formatClock(seconds) {
  const s = Math.max(0, Math.floor(finite(seconds)));
  return `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`;
}

function formatAge(ms) {
  if (!Number.isFinite(ms)) return '—';
  if (ms < 1_000) return `${Math.max(0, Math.round(ms))} ms`;
  return `${(ms / 1_000).toFixed(ms < 10_000 ? 1 : 0)} s`;
}

function setPill(target, tone) {
  target.className = `pill ${tone}`;
}

function validateSnapshot(s) {
  if (!s || typeof s !== 'object' || Array.isArray(s)) return 'snapshot is not an object';
  if (s.schema !== SCHEMA) return `unsupported schema ${str(s.schema, '(missing)')}`;
  if (s.version !== VERSION) return `snapshot version ${str(s.version, '(missing)')} is not supported`;
  if (!Number.isFinite(Number(s.sequence))) return 'sequence is not numeric';
  if (!Number.isFinite(Date.parse(s.capturedAt))) return 'capturedAt is not an ISO timestamp';
  for (const key of ['resources', 'workers', 'production', 'events']) {
    if (!Array.isArray(s[key])) return `${key} is not an array`;
  }
  if (!s.population || typeof s.population !== 'object') return 'population is missing';
  if (!s.advice || typeof s.advice !== 'object') return 'advice is missing';
  return null;
}

function renderResources(resources) {
  const children = resources.slice(0, 12).map((resource) => {
    const wrap = node('div', 'resource');
    wrap.append(node('div', 'res-name', resource.label ?? resource.id ?? 'resource'));
    wrap.append(node('div', 'res-value', formatNumber(resource.amount)));
    const rate = node('div', `res-rate${finite(resource.rate) < 0 ? ' negative' : ''}`, formatRate(resource.rate));
    wrap.append(rate);
    const confidence = node('div', 'res-confidence');
    const fill = node('i');
    fill.style.width = `${Math.round(100 * bounded(resource.confidence, 0, 1))}%`;
    confidence.append(fill);
    wrap.append(confidence);
    return wrap;
  });
  replaceChildren(el.resources, children);
}

function renderWorkers(workers) {
  const rows = workers.slice(0, 16);
  const total = rows.reduce((sum, worker) => sum + Math.max(0, finite(worker.count)), 0);
  const idle = rows.find((worker) => worker.id === 'idle');
  el.workerSummary.textContent = `${formatNumber(total)} workers · ${formatNumber(idle?.count ?? 0)} idle`;

  const segments = rows.filter((worker) => finite(worker.count) > 0).map((worker, index) => {
    const segment = node('i');
    segment.style.width = `${100 * finite(worker.count) / Math.max(1, total)}%`;
    segment.style.background = COLORS[index % COLORS.length];
    segment.title = `${str(worker.label, worker.id)}: ${formatNumber(worker.count)}`;
    return segment;
  });
  replaceChildren(el.allocation, segments);

  const list = rows.map((worker, index) => {
    const row = node('div', 'worker-row');
    const label = node('div', 'worker-label', worker.label ?? worker.id ?? 'workers');
    label.style.setProperty('--swatch', COLORS[index % COLORS.length]);
    row.append(label, node('div', 'worker-count', formatNumber(worker.count)),
      node('div', 'worker-target', `→ ${formatNumber(worker.target)}`));
    const delta = finite(worker.target) - finite(worker.count);
    const moveClass = Math.abs(delta) < .5 ? 'worker-move hold' : `worker-move${delta < 0 ? ' down' : ''}`;
    row.append(node('div', moveClass, Math.abs(delta) < .5 ? 'hold' : `${delta > 0 ? '+' : '−'}${formatNumber(Math.abs(delta))}`));
    return row;
  });
  replaceChildren(el.workerList, list);
}

function renderPopulation(population) {
  const used = Math.max(0, finite(population.used));
  const cap = Math.max(0, finite(population.cap));
  const free = Math.max(0, cap - used);
  const pct = bounded(used / Math.max(1, cap), 0, 1);
  el.popUsed.textContent = formatNumber(used);
  el.popCap.textContent = `/ ${formatNumber(cap)}`;
  el.popTrend.textContent = `${finite(population.trend) >= 0 ? '+' : '−'}${Math.abs(finite(population.trend)).toFixed(1)}/m`;
  el.popFree.textContent = `${formatNumber(free)} free`;
  el.capStatus.textContent = free <= 4 ? 'cap pressure' : 'headroom';
  el.popForecast.textContent = Number.isFinite(Number(population.secondsToCap))
    ? `cap in ${formatClock(population.secondsToCap)}` : 'cap stable';
  el.popBar.style.width = `${Math.round(pct * 100)}%`;
  el.popBar.className = `pop-fill${pct >= .9 ? ' warn' : ''}`;
}

function renderProduction(production) {
  const active = production.filter((queue) => queue.item && !queue.blocked).length;
  const blocked = production.filter((queue) => queue.blocked).length;
  el.queueSummary.textContent = `${active} active${blocked ? ` · ${blocked} blocked` : ''}`;
  const rows = production.slice(0, 16).map((queue) => {
    const row = node('div', 'queue');
    const building = node('div', 'queue-building');
    building.append(node('strong', '', queue.building ?? 'building'), node('span', '', queue.location ?? 'unknown location'));
    const item = node('div', 'queue-item');
    const name = node('div', 'queue-name');
    name.append(node('span', '', queue.item ?? (queue.blocked ? 'blocked' : 'idle')),
      node('span', 'queued', `${Math.max(0, Math.floor(finite(queue.queued)))} queued`));
    const progress = node('div', 'progress');
    const fill = node('i', queue.blocked ? 'blocked' : '');
    fill.style.width = `${Math.round(100 * bounded(queue.progress, 0, 1))}%`;
    progress.append(fill);
    item.append(name, progress);
    const eta = queue.blocked ? str(queue.blockedReason, 'blocked')
      : Number.isFinite(Number(queue.remaining)) ? formatClock(queue.remaining) : 'idle';
    row.append(building, item, node('div', 'queue-eta', eta));
    return row;
  });
  replaceChildren(el.queues, rows.length ? rows : [node('div', 'queue', 'No production buildings observed')]);
}

function renderEvents(events) {
  const now = Date.now();
  const unique = new Map();
  for (const event of events) {
    if (event.expiresAt && Date.parse(event.expiresAt) <= now) continue;
    unique.set(str(event.id, `${event.gameTime}:${event.kind}:${event.text}`), event);
  }
  const rows = [...unique.values()].slice(-24).reverse().map((event) => {
    const row = node('div', 'event');
    const kind = str(event.kind, 'info').toLowerCase();
    const color = kind === 'warning' ? '#ffbd69' : kind === 'milestone' ? '#77dfa8'
      : kind === 'error' ? '#ff776d' : '#64b5f6';
    row.style.setProperty('--event', color);
    const copy = node('div', 'event-copy');
    copy.append(node('span', 'event-kind', kind), document.createTextNode(str(event.text, 'event')));
    row.append(node('div', 'event-time', formatClock(event.gameTime)), copy);
    return row;
  });
  replaceChildren(el.timeline, rows.length ? rows : [node('div', 'event', 'No economic events yet')]);
}

function renderAdvice(advice) {
  if (advice.validUntil && Date.parse(advice.validUntil) <= Date.now()) {
    suppressAdvice('expired', 'Advice paused — recommendation expired',
      'A newer coherent snapshot is required before rontoy can renew this recommendation.');
    return;
  }
  const severity = ['good', 'warning', 'urgent'].includes(advice.severity) ? advice.severity : 'info';
  el.advicePriority.textContent = severity === 'urgent' ? 'act now' : severity === 'warning' ? 'priority' : severity;
  el.advicePriority.className = `priority${severity === 'good' ? ' good' : severity === 'urgent' ? ' bad' : ''}`;
  el.adviceHeadline.textContent = str(advice.headline, 'No advice available');
  el.adviceDetail.textContent = str(advice.detail, 'The current snapshot does not include an explanation.');
  const confidence = bounded(advice.confidence, 0, 1);
  el.adviceConfidence.textContent = `${Math.round(confidence * 100)}% confidence`;
  el.adviceConfidenceBar.style.width = `${Math.round(confidence * 100)}%`;
  replaceChildren(el.adviceActions, (Array.isArray(advice.actions) ? advice.actions : []).slice(0, 5)
    .map((action) => node('span', 'action', action)));
}

function renderSnapshot(snapshot) {
  const age = Date.now() - Date.parse(snapshot.capturedAt);
  el.playerName.textContent = str(snapshot.player?.name, 'unknown player');
  el.gamePhase.textContent = `${str(snapshot.phase, 'unknown phase')} · ${str(snapshot.player?.nation, 'unknown nation')} · ${str(snapshot.map, 'unknown map')}`;
  el.gameClock.textContent = formatClock(snapshot.gameTime);
  el.sequence.textContent = `snapshot ${Math.floor(finite(snapshot.sequence))}`;
  el.overallConfidence.textContent = `${Math.round(100 * bounded(snapshot.confidence, 0, 1))}%`;
  el.frameCoherence.textContent = `${Number.isFinite(Number(snapshot.gameFrame)) ? Math.floor(finite(snapshot.gameFrame)) : '—'} · ${str(snapshot.coherence, 'not reported')}`;
  el.rateCacheAge.textContent = Number.isFinite(Number(snapshot.rateCacheAgeMs)) ? formatAge(finite(snapshot.rateCacheAgeMs)) : 'not reported';
  el.incomeWindow.textContent = `rate / ${Math.max(1, Math.floor(finite(snapshot.rateWindowSeconds, 60)))} s`;
  renderAdvice(snapshot.advice);
  renderResources(snapshot.resources);
  renderWorkers(snapshot.workers);
  renderPopulation(snapshot.population);
  renderProduction(snapshot.production);
  renderEvents(snapshot.events);
  updateFreshness(age);
  state.renders++;
}

function suppressAdvice(label, headline, detail) {
  document.querySelector('.advice').classList.add('detached');
  el.advicePriority.textContent = label;
  el.advicePriority.className = 'priority bad';
  el.adviceHeadline.textContent = headline;
  el.adviceDetail.textContent = detail;
  replaceChildren(el.adviceActions, []);
}

function updateFreshness(forceAge) {
  const age = forceAge ?? (state.snapshot ? Date.now() - Date.parse(state.snapshot.capturedAt) : Infinity);
  el.snapshotAge.textContent = formatAge(age);
  if (age < STALE_AFTER_MS) {
    el.freshText.textContent = `fresh · ${formatAge(age)}`;
    setPill(el.freshBadge, 'live');
  } else if (age < DEAD_AFTER_MS) {
    el.freshText.textContent = `stale · ${formatAge(age)}`;
    setPill(el.freshBadge, 'wait');
    suppressAdvice('stale', 'Advice paused — snapshot is stale',
      'The displayed economy is aging. Wait for a fresh coherent snapshot before changing the build plan.');
  } else {
    el.freshText.textContent = state.snapshot ? `signal lost · ${formatAge(age)}` : 'no snapshot';
    setPill(el.freshBadge, 'bad');
    suppressAdvice('detached', 'Advice paused — telemetry is detached',
      'Reconnect the local snapshot stream before acting on an economic recommendation.');
  }
}

function updateTransport(tone = 'wait') {
  el.sourceText.textContent = `${state.source} · ${state.transport}`;
  setPill(el.sourceBadge, tone);
  el.transportState.textContent = state.transport;
  el.footerSource.textContent = `source: ${state.source}`;
  el.footerErrors.textContent = `${state.rejected} rejected snapshot${state.rejected === 1 ? '' : 's'}`;
}

function reject(message) {
  state.rejected++;
  state.lastError = str(message);
  state.transport = `rejected: ${state.lastError}`;
  updateTransport('bad');
  return false;
}

function ingest(snapshot) {
  const error = validateSnapshot(snapshot);
  if (error) return reject(error);
  if (state.snapshot && snapshot.sequence < state.snapshot.sequence && snapshot.source === state.snapshot.source) {
    return reject(`out-of-order sequence ${snapshot.sequence}`);
  }
  state.snapshot = snapshot;
  state.accepted++;
  document.querySelector('.advice').classList.remove('detached');
  if (state.mode === 'websocket') {
    state.source = str(snapshot.source, 'live stream');
    state.transport = 'streaming';
    updateTransport('live');
  } else if (state.mode === 'mock') {
    state.source = 'demo telemetry';
    state.transport = 'streaming';
    updateTransport('live');
  }
  renderSnapshot(snapshot);
  return true;
}

// ---------------------------------------------------------------------------
// Deterministic demo snapshots. This is conspicuously labeled "demo telemetry" in the UI;
// no generated value is presented as a live read from Rise of Nations.

let mockSequence = 0;
let mockGameTime = 434;
let mockEvents = [
  { id: 1, gameTime: 405, kind: 'milestone', text: 'Classical Age research completed' },
  { id: 2, gameTime: 418, kind: 'info', text: 'Second city gathering timber efficiently' },
  { id: 3, gameTime: 429, kind: 'warning', text: 'Population will reach cap before both queues finish' },
];

function makeMockSnapshot() {
  mockSequence++;
  mockGameTime += 2;
  const wave = Math.sin(mockSequence / 3);
  const used = 86 + (Math.floor(mockSequence / 4) % 4);
  if (mockSequence % 7 === 0) mockEvents = [...mockEvents, {
    id: 10 + mockSequence, gameTime: mockGameTime, kind: mockSequence % 14 ? 'info' : 'milestone',
    text: mockSequence % 14 ? 'Caravan income sample updated' : 'Knowledge target reached for next research',
  }].slice(-24);
  return {
    schema: SCHEMA, version: VERSION, sequence: mockSequence,
    capturedAt: new Date().toISOString(), source: 'deterministic demo', confidence: .91,
    rateWindowSeconds: 60, gameTime: mockGameTime, gameFrame: Math.round(mockGameTime / .067),
    coherence: 'single-frame', rateCacheAgeMs: 180, phase: 'Classical Age', map: 'Old World',
    player: { name: 'Demo player', nation: 'Inca' },
    resources: [
      { id: 'food', label: 'Food', amount: 312 + wave * 8, rate: 93.4 + wave, confidence: .98 },
      { id: 'timber', label: 'Timber', amount: 128 - wave * 3, rate: 42.1 - wave, confidence: .97 },
      { id: 'wealth', label: 'Wealth', amount: 187, rate: 51.8 + wave * .7, confidence: .94 },
      { id: 'knowledge', label: 'Knowledge', amount: 76 + wave * 2, rate: 31.2, confidence: .92 },
      { id: 'metal', label: 'Metal', amount: 44, rate: 12.6, confidence: .89 },
      { id: 'oil', label: 'Oil', amount: 0, rate: 0, confidence: .99 },
    ],
    population: { used, cap: 100, trend: 8.8, secondsToCap: Math.max(24, (100 - used) / 8.8 * 60) },
    workers: [
      { id: 'food', label: 'Food', count: 28, target: 25 },
      { id: 'timber', label: 'Timber', count: 12, target: 18 },
      { id: 'wealth', label: 'Wealth', count: 14, target: 14 },
      { id: 'knowledge', label: 'Knowledge', count: 9, target: 9 },
      { id: 'builders', label: 'Builders', count: 4, target: 5 },
      { id: 'idle', label: 'Idle', count: 2, target: 0 },
    ],
    production: [
      { building: 'City', location: 'Capital', item: 'Citizen', progress: .68 + (mockSequence % 8) * .035, remaining: 11, queued: 2 },
      { building: 'City', location: 'South settlement', item: 'Citizen', progress: .31 + (mockSequence % 10) * .025, remaining: 19, queued: 1 },
      { building: 'Barracks', location: 'Eastern front', item: 'Heavy Infantry', progress: .47, remaining: 16, queued: 2 },
      { building: 'Library', location: 'Capital', item: 'Civic II', progress: .84, remaining: 9, queued: 0 },
    ],
    advice: {
      severity: used >= 90 ? 'urgent' : 'warning',
      headline: used >= 90 ? 'Build population capacity now' : 'Move 6 workers from food to timber',
      detail: used >= 90
        ? `Population is ${used}/100 and growing +8.8/m; both citizen queues complete inside the current cap window.`
        : 'Food income is +93.4/m versus +42.1/m timber; timber gates population capacity while 3 food workers are surplus at each city.',
      actions: used >= 90 ? ['queue a capacity building', 'keep both cities producing', 'recheck in 20 s']
        : ['−3 food at capital', '−3 food at south city', '+6 timber', 'queue capacity at 150 timber'],
      confidence: .88,
    },
    events: mockEvents,
  };
}

function clearTimers() {
  clearInterval(state.mockTimer);
  clearTimeout(state.reconnectTimer);
  state.mockTimer = 0;
  state.reconnectTimer = 0;
}

function closeSocket() {
  if (!state.socket) return;
  const socket = state.socket;
  state.socket = null;
  socket.onopen = socket.onmessage = socket.onerror = socket.onclose = null;
  socket.close();
}

function useMock(delay = 0) {
  clearTimers();
  closeSocket();
  state.mode = 'mock';
  state.source = 'demo telemetry';
  state.transport = delay ? 'reconnecting' : 'streaming';
  updateTransport(delay ? 'wait' : 'live');
  const start = () => {
    state.snapshot = null;
    state.transport = 'streaming';
    updateTransport('live');
    ingest(makeMockSnapshot());
    state.mockTimer = setInterval(() => ingest(makeMockSnapshot()), 1_000);
  };
  if (delay) state.reconnectTimer = setTimeout(start, delay);
  else start();
}

function scheduleReconnect() {
  clearTimeout(state.reconnectTimer);
  state.reconnects++;
  state.reconnectAttempt++;
  const delay = Math.min(15_000, 500 * 2 ** Math.min(5, state.reconnectAttempt - 1));
  state.transport = `reconnecting in ${(delay / 1_000).toFixed(1)} s`;
  updateTransport('wait');
  suppressAdvice('reconnecting', 'Advice paused — reconnecting telemetry',
    'The previous recommendation is hidden until a fresh coherent snapshot arrives.');
  state.reconnectTimer = setTimeout(() => connect(state.wsUrl), delay);
}

function connect(url) {
  clearTimers();
  closeSocket();
  if (!url) { useMock(); return; }
  let endpoint;
  try { endpoint = new URL(str(url)); }
  catch { reject('WebSocket URL is invalid'); return; }
  const loopback = ['127.0.0.1', 'localhost', '[::1]'].includes(endpoint.hostname);
  if (endpoint.protocol !== 'ws:' || !loopback) {
    reject('live telemetry must use a loopback WebSocket');
    return;
  }
  state.mode = 'websocket';
  state.wsUrl = endpoint.href;
  state.source = 'live telemetry';
  state.transport = 'connecting';
  updateTransport('wait');
  let socket;
  try { socket = new WebSocket(state.wsUrl); }
  catch (error) { state.lastError = str(error?.message, error); scheduleReconnect(); return; }
  state.socket = socket;
  socket.onopen = () => {
    state.reconnectAttempt = 0;
    state.transport = 'connected · waiting for snapshot';
    updateTransport('live');
  };
  socket.onmessage = (message) => {
    try { ingest(JSON.parse(message.data)); }
    catch (error) { reject(`invalid JSON: ${str(error?.message, error)}`); }
  };
  socket.onerror = () => { state.lastError = 'WebSocket transport error'; };
  socket.onclose = () => {
    if (state.socket !== socket) return;
    state.socket = null;
    scheduleReconnect();
  };
}

function reconnect() {
  state.reconnects++;
  if (state.mode === 'websocket') {
    state.reconnectAttempt = 0;
    connect(state.wsUrl);
  } else {
    useMock(250);
  }
}

el.reconnect.addEventListener('click', reconnect);
setInterval(() => updateFreshness(), 250);

const params = new URLSearchParams(location.search);
const ws = params.get('ws');
if (ws) connect(ws);
else useMock();

window.rontoy = Object.freeze({
  schema: SCHEMA,
  version: VERSION,
  ingest,
  connect,
  useMock,
  reconnect,
  snapshot: () => state.snapshot ? structuredClone(state.snapshot) : null,
  stats: () => ({
    schema: SCHEMA, version: VERSION, mode: state.mode, source: state.source,
    transport: state.transport, sequence: state.snapshot?.sequence ?? null,
    staleMs: state.snapshot ? Date.now() - Date.parse(state.snapshot.capturedAt) : null,
    renders: state.renders, accepted: state.accepted, rejected: state.rejected,
    reconnects: state.reconnects, lastError: state.lastError,
    resources: state.snapshot?.resources?.length ?? 0,
    workers: state.snapshot?.workers?.length ?? 0,
    queues: state.snapshot?.production?.length ?? 0,
    events: state.snapshot?.events?.length ?? 0,
  }),
});
