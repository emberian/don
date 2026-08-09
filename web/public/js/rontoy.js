// rontoy — a read-only live economy coach.
//
// The UI consumes the loopback host's versioned SSE envelope. It defaults to a deterministic
// demo so layout and coaching behavior can be exercised before live memory reads are
// available. Pass `?sse=/v1/stream` when the host serves this page. Protocol bytes terminate
// server-side; the browser sees only admitted aggregate economy JSON. Every telemetry string
// reaches the page through `textContent`.

const SCHEMA = 'rontoy.snapshot';
const VERSION = 1;
const STALE_AFTER_MS = 4_000;
const DEAD_AFTER_MS = 15_000;
const MAX_EVENT_BYTES = 128 * 1024;
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
  sseUrl: null,
  eventSource: null,
  reconnectTimer: 0,
  reconnectAttempt: 0,
  mockTimer: 0,
  statusTimer: 0,
  hostAgeMs: null,
  hostStale: false,
};

const finite = (value, fallback = 0) => Number.isFinite(Number(value)) ? Number(value) : fallback;
const bounded = (value, lo = 0, hi = 1) => Math.max(lo, Math.min(hi, finite(value)));
const str = (value, fallback = '—') => value === undefined || value === null ? fallback : String(value);
const record = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);
const number = (value) => typeof value === 'number' && Number.isFinite(value);
const integer = (value) => Number.isSafeInteger(value);
const text = (value, max = 512) => typeof value === 'string' && value.length <= max;
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

function validateSnapshot(s, now = Date.now()) {
  if (!s || typeof s !== 'object' || Array.isArray(s)) return 'snapshot is not an object';
  if (s.schema !== SCHEMA) return `unsupported schema ${str(s.schema, '(missing)')}`;
  if (s.version !== VERSION) return `snapshot version ${str(s.version, '(missing)')} is not supported`;
  if (!integer(s.sequence) || s.sequence < 0) return 'sequence is not a non-negative safe integer';
  const captured = Date.parse(s.capturedAt);
  if (!Number.isFinite(captured)) return 'capturedAt is not an ISO timestamp';
  if (captured > now + 1_000) return 'capturedAt is in the future';
  if (!record(s.capture) || s.capture.complete !== true || typeof s.capture.adviceAllowed !== 'boolean') {
    return 'capture is not complete or lacks an advice gate';
  }
  if (!Array.isArray(s.capture.suppressedReasons) || s.capture.suppressedReasons.length > 8
      || s.capture.suppressedReasons.some((reason) => !text(reason, 128))) return 'capture suppression reasons are invalid';
  if (!integer(s.capture.frameStart) || !integer(s.capture.frameEnd)
      || s.capture.frameStart !== s.capture.frameEnd || s.capture.frameEnd !== s.gameFrame
      || s.coherence !== 'single-frame') return 'capture is not single-frame coherent';
  if (!text(s.source, 256)) return 'source is missing or too long';
  if (s.confidence !== null && s.confidence !== undefined && (!number(s.confidence) || s.confidence < 0 || s.confidence > 1)) {
    return 'confidence is outside 0..1';
  }
  if (s.confidenceLabel !== undefined && !text(s.confidenceLabel, 64)) return 'confidence label is invalid';
  if (s.rateCacheAgeFrames !== null && s.rateCacheAgeFrames !== undefined
      && (!integer(s.rateCacheAgeFrames) || s.rateCacheAgeFrames < 0)) return 'rate-cache frame age is invalid';
  if (!Array.isArray(s.resources) || s.resources.length < 1 || s.resources.length > 12) return 'resources has invalid length';
  if (!Array.isArray(s.workers) || s.workers.length > 16) return 'workers has invalid length';
  if (!Array.isArray(s.production) || s.production.length > 16) return 'production has invalid length';
  if (!Array.isArray(s.events) || s.events.length > 64) return 'events has invalid length';
  if (!record(s.population) || !integer(s.population.used) || !integer(s.population.cap)
      || s.population.used < 0 || s.population.cap < 0) return 'population is invalid';
  if (s.population.trend !== null && s.population.trend !== undefined && !number(s.population.trend)) return 'population trend is invalid';
  if (s.population.secondsToCap !== null && s.population.secondsToCap !== undefined
      && (!number(s.population.secondsToCap) || s.population.secondsToCap < 0)) return 'population forecast is invalid';
  if (!record(s.advice) || !text(s.advice.headline, 512) || !text(s.advice.detail, 2_048)
      || !text(s.advice.severity, 32) || !Array.isArray(s.advice.actions) || s.advice.actions.length > 5
      || s.advice.actions.some((action) => !text(action, 512))) return 'advice is invalid';
  if (s.advice.confidence !== null && s.advice.confidence !== undefined
      && (!number(s.advice.confidence) || s.advice.confidence < 0 || s.advice.confidence > 1)) return 'advice confidence is invalid';
  for (const [index, resource] of s.resources.entries()) {
    if (!record(resource) || !text(resource.id, 64) || !text(resource.label, 128)
        || !number(resource.amount) || resource.amount < 0 || !number(resource.rate)) return `resources[${index}] is invalid`;
    if (resource.confidence !== null && resource.confidence !== undefined
        && (!number(resource.confidence) || resource.confidence < 0 || resource.confidence > 1)) return `resources[${index}].confidence is invalid`;
  }
  for (const [index, worker] of s.workers.entries()) {
    if (!record(worker) || !text(worker.id, 64) || !text(worker.label, 128)
        || !integer(worker.count) || worker.count < 0) return `workers[${index}] is invalid`;
    if (worker.target !== null && worker.target !== undefined
        && (!integer(worker.target) || worker.target < 0)) return `workers[${index}].target is invalid`;
  }
  for (const [index, queue] of s.production.entries()) {
    if (!record(queue) || !text(queue.building, 128) || !text(queue.location, 128)
        || !text(queue.item, 256) || !integer(queue.queued) || queue.queued < 0) return `production[${index}] is invalid`;
    if (queue.progress !== null && queue.progress !== undefined
        && (!number(queue.progress) || queue.progress < 0 || queue.progress > 1)) return `production[${index}].progress is invalid`;
    if (queue.remaining !== null && queue.remaining !== undefined
        && (!number(queue.remaining) || queue.remaining < 0)) return `production[${index}].remaining is invalid`;
    if (queue.blocked !== undefined && typeof queue.blocked !== 'boolean') return `production[${index}].blocked is invalid`;
    if (queue.blockedReason !== undefined && !text(queue.blockedReason, 256)) return `production[${index}].blockedReason is invalid`;
  }
  for (const [index, event] of s.events.entries()) {
    if (!record(event) || !text(str(event.id), 128) || !number(event.gameTime) || event.gameTime < 0
        || !text(event.kind, 64) || !text(event.text, 1_024)) return `events[${index}] is invalid`;
  }
  return null;
}

function renderResources(resources) {
  const children = resources.slice(0, 12).map((resource) => {
    const wrap = node('div', 'resource');
    wrap.append(node('div', 'res-name', resource.label ?? resource.id ?? 'resource'));
    wrap.append(node('div', 'res-value', formatNumber(resource.amount)));
    const rate = node('div', `res-rate${finite(resource.rate) < 0 ? ' negative' : ''}`, formatRate(resource.rate));
    wrap.append(rate);
    if (number(resource.confidence)) {
      const confidence = node('div', 'res-confidence');
      const fill = node('i');
      fill.style.width = `${Math.round(100 * resource.confidence)}%`;
      confidence.append(fill);
      wrap.append(confidence);
    }
    return wrap;
  });
  replaceChildren(el.resources, children);
}

function renderWorkers(workers) {
  const rows = workers.slice(0, 16);
  if (rows.length === 0) {
    el.workerSummary.textContent = 'allocation not reported';
    replaceChildren(el.allocation, []);
    const note = node('div', 'worker-row', 'No direct worker counts reported by this capture.');
    note.style.gridTemplateColumns = '1fr';
    note.style.color = 'var(--dim)';
    replaceChildren(el.workerList, [note]);
    return;
  }
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
    const hasTarget = integer(worker.target);
    row.append(label, node('div', 'worker-count', formatNumber(worker.count)),
      node('div', 'worker-target', hasTarget ? `→ ${formatNumber(worker.target)}` : 'observed'));
    const delta = hasTarget ? worker.target - worker.count : null;
    const moveClass = delta === null || Math.abs(delta) < .5 ? 'worker-move hold' : `worker-move${delta < 0 ? ' down' : ''}`;
    row.append(node('div', moveClass, delta === null ? '—' : Math.abs(delta) < .5 ? 'hold' : `${delta > 0 ? '+' : '−'}${formatNumber(Math.abs(delta))}`));
    return row;
  });
  replaceChildren(el.workerList, list);
}

function renderPopulation(population) {
  const used = Math.max(0, finite(population.used));
  const cap = Math.max(0, finite(population.cap));
  const headroom = cap - used;
  const pct = bounded(used / Math.max(1, cap), 0, 1);
  el.popUsed.textContent = formatNumber(used);
  el.popCap.textContent = `/ ${formatNumber(cap)}`;
  el.popTrend.textContent = number(population.trend)
    ? `${population.trend >= 0 ? '+' : '−'}${Math.abs(population.trend).toFixed(1)}/m`
    : 'trend not reported';
  el.popFree.textContent = headroom < 0 ? `${formatNumber(Math.abs(headroom))} over cap` : `${formatNumber(headroom)} free`;
  el.capStatus.textContent = headroom < 0 ? 'over cap' : headroom <= 4 ? 'cap pressure' : 'headroom';
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
    fill.style.width = number(queue.progress) ? `${Math.round(100 * queue.progress)}%` : '0%';
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
  const confidence = number(advice.confidence) ? advice.confidence : null;
  el.adviceConfidence.textContent = confidence === null ? 'confidence not reported' : `${Math.round(confidence * 100)}% confidence`;
  el.adviceConfidenceBar.style.width = confidence === null ? '0%' : `${Math.round(confidence * 100)}%`;
  replaceChildren(el.adviceActions, (Array.isArray(advice.actions) ? advice.actions : []).slice(0, 5)
    .map((action) => node('span', 'action', action)));
}

function renderAdmissionAdvice(snapshot) {
  if (snapshot.capture.adviceAllowed) renderAdvice(snapshot.advice);
  else suppressAdvice('observing', 'Economy observed — advice suppressed',
    `This capture is display-only: ${snapshot.capture.suppressedReasons.join(', ') || 'host did not allow advice'}.`);
}

function renderSnapshot(snapshot) {
  const age = Date.now() - Date.parse(snapshot.capturedAt);
  el.playerName.textContent = str(snapshot.player?.name, 'unknown player');
  el.gamePhase.textContent = `${str(snapshot.phase, 'unknown phase')} · ${str(snapshot.player?.nation, 'unknown nation')} · ${str(snapshot.map, 'unknown map')}`;
  el.gameClock.textContent = formatClock(snapshot.gameTime);
  el.sequence.textContent = `snapshot ${Math.floor(finite(snapshot.sequence))}`;
  el.overallConfidence.textContent = number(snapshot.confidence) ? `${Math.round(100 * snapshot.confidence)}%`
    : str(snapshot.confidenceLabel, 'not reported');
  el.frameCoherence.textContent = `${Number.isFinite(Number(snapshot.gameFrame)) ? Math.floor(finite(snapshot.gameFrame)) : '—'} · ${str(snapshot.coherence, 'not reported')}`;
  el.rateCacheAge.textContent = integer(snapshot.rateCacheAgeFrames)
    ? `${snapshot.rateCacheAgeFrames} sim frame${snapshot.rateCacheAgeFrames === 1 ? '' : 's'}` : 'not reported';
  el.incomeWindow.textContent = `rate / ${Math.max(1, Math.floor(finite(snapshot.rateWindowSeconds, 60)))} s`;
  renderAdmissionAdvice(snapshot);
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
  const age = forceAge ?? (state.mode === 'sse' && number(state.hostAgeMs) ? state.hostAgeMs
    : state.snapshot ? Date.now() - Date.parse(state.snapshot.capturedAt) : Infinity);
  const hostSaysStale = state.mode === 'sse' && state.hostStale;
  el.snapshotAge.textContent = formatAge(age);
  if (age < STALE_AFTER_MS && !hostSaysStale) {
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
  suppressAdvice('rejected', 'Advice suppressed — snapshot rejected',
    `rontoy refused unsafe telemetry: ${state.lastError}`);
  return false;
}

function ingest(snapshot) {
  const error = validateSnapshot(snapshot);
  if (error) return reject(error);
  if (state.snapshot && snapshot.sequence < state.snapshot.sequence && snapshot.source === state.snapshot.source) {
    return reject(`out-of-order sequence ${snapshot.sequence}`);
  }
  document.querySelector('.advice').classList.remove('detached');
  try { renderSnapshot(snapshot); }
  catch (error) { return reject(`render admission failed: ${str(error?.message, error)}`); }
  state.snapshot = snapshot;
  state.accepted++;
  if (state.mode === 'sse') {
    state.source = str(snapshot.source, 'rontoy host');
    state.transport = 'streaming';
    updateTransport('live');
  } else if (state.mode === 'mock') {
    state.source = 'demo telemetry';
    state.transport = 'streaming';
    updateTransport('live');
  }
  return true;
}

function adaptHostEnvelope(envelope, now = Date.now()) {
  const fail = (error) => ({ error });
  if (!record(envelope)) return fail('host envelope is not an object');
  if (!integer(envelope.stream_revision) || envelope.stream_revision < 0) return fail('host stream revision is invalid');
  if (!integer(envelope.received_at_ms) || envelope.received_at_ms < 0) return fail('host receipt timestamp is invalid');
  if (envelope.received_at_ms > now + 1_000) return fail('host receipt timestamp is in the future');
  const raw = envelope.snapshot;
  const analysis = envelope.analysis;
  if (!record(raw) || raw.schema_version !== VERSION) return fail('host snapshot schema is unsupported');
  let processStartValid = false;
  if (text(raw.source?.process_started_100ns, 20) && /^[0-9]+$/.test(raw.source.process_started_100ns)) {
    try {
      const processStart = BigInt(raw.source.process_started_100ns);
      processStartValid = processStart > 0n && processStart <= 0xffff_ffff_ffff_ffffn;
    } catch {}
  }
  if (!record(raw.source) || !text(raw.source.session_id, 128)
      || !/^[A-Za-z0-9_.:-]{1,128}$/.test(raw.source.session_id) || !integer(raw.source.sequence)
      || raw.source.sequence < 0 || !integer(raw.source.captured_at_ms) || raw.source.captured_at_ms < 0
      || !integer(raw.source.process_id) || raw.source.process_id < 1 || raw.source.process_id > 0xffff_ffff
      || !processStartValid
      || !text(raw.source.module_sha256, 64) || !/^[0-9a-f]{64}$/.test(raw.source.module_sha256)
      || !integer(raw.source.module_size) || raw.source.module_size < 1 || raw.source.module_size > 2 ** 40
      || !integer(raw.source.image_entry_rva) || raw.source.image_entry_rva < 1 || raw.source.image_entry_rva > 0xffff_ffff
      || !integer(raw.source.image_size) || raw.source.image_size < 1 || raw.source.image_size > 0xffff_ffff
      || (raw.source.reader_version !== undefined
        && (!text(raw.source.reader_version, 64) || raw.source.reader_version.length < 1))) {
    return fail('host snapshot source is invalid');
  }
  if (raw.source.captured_at_ms > now + 300_000) return fail('producer capture timestamp is implausibly far in the future');
  if (!record(raw.capture) || raw.capture.complete !== true || typeof raw.capture.advice_allowed !== 'boolean'
      || !integer(raw.capture.frame_start)
      || !integer(raw.capture.frame_end) || raw.capture.frame_start !== raw.capture.frame_end) {
    return fail('host capture is incomplete or mixed-frame');
  }
  if (!integer(raw.capture.duration_us) || raw.capture.duration_us < 0
      || !integer(raw.capture.read_count) || raw.capture.read_count < 1
      || !integer(raw.capture.bytes_read) || raw.capture.bytes_read < 1) return fail('host capture diagnostics are invalid');
  if (!record(raw.game) || !integer(raw.game.frame) || raw.game.frame !== raw.capture.frame_end
      || !integer(raw.game.player_id) || raw.game.player_id < 0 || raw.game.player_id > 15) {
    return fail('host game identity is not coherent with capture');
  }
  if (!['single_player', 'multiplayer', 'unknown'].includes(raw.game.mode)
      || (raw.game.paused !== null && typeof raw.game.paused !== 'boolean')
      || !integer(raw.game.human_count) || raw.game.human_count < 0 || raw.game.human_count > 16
      || raw.game.human_selection_basis !== 'unique_active_in_play_console_flags') {
    return fail('host game safety evidence is invalid');
  }
  if (raw.game.age !== undefined && (!integer(raw.game.age) || raw.game.age < 0 || raw.game.age > 8)) return fail('host game age is invalid');
  if (!record(raw.economy) || !record(raw.economy.resources) || !record(raw.economy.population)) {
    return fail('host economy is incomplete');
  }
  const resourceEntries = Object.entries(raw.economy.resources);
  const knownResources = new Set(['food', 'timber', 'wealth', 'metal', 'knowledge', 'oil']);
  if (resourceEntries.length < 1 || resourceEntries.length > 6
      || resourceEntries.some(([name]) => !knownResources.has(name))) return fail('host resource set is invalid');
  for (const [name, resource] of resourceEntries) {
    if (!record(resource) || !number(resource.stock) || resource.stock < 0 || !number(resource.income_per_min)
        || ((resource.gatherers === undefined) !== (resource.gatherers_basis === undefined))
        || (resource.gatherers !== undefined && (!integer(resource.gatherers) || resource.gatherers < 0
          || resource.gatherers_basis !== 'direct_count'))) {
      return fail(`host resource ${name} is invalid`);
    }
  }
  const pop = raw.economy.population;
  if (!integer(pop.used) || pop.used < 0 || !integer(pop.cap) || pop.cap < 0
      || ((pop.idle_citizens === undefined) !== (pop.idle_basis === undefined))
      || (pop.idle_citizens !== undefined && (!integer(pop.idle_citizens) || pop.idle_citizens < 0
        || pop.idle_basis !== 'direct_count'))) return fail('host population is invalid');
  const prod = raw.economy.production;
  if (prod !== undefined && (!record(prod) || !integer(prod.queue_depth) || prod.queue_depth < 0
      || !integer(prod.active_sites) || prod.active_sites < 0
      || prod.queue_basis !== 'direct_build_queue')) return fail('host production aggregate is invalid');
  const rateSample = raw.economy.rate_sample;
  if (!record(rateSample) || !['engine_direct_gather_cache', 'sampled_stock_delta', 'unavailable'].includes(rateSample.basis)
      || !integer(rateSample.age_frames) || rateSample.age_frames < 0 || rateSample.age_frames > 54_000
      || !['direct', 'estimated', 'unavailable'].includes(rateSample.confidence)
      || (rateSample.gather_stamp_raw !== null && (!integer(rateSample.gather_stamp_raw) || rateSample.gather_stamp_raw < 0))) {
    return fail('host rate sample is invalid');
  }
  if (rateSample.basis === 'engine_direct_gather_cache') {
    const derivedAge = ((raw.game.frame >>> 0) - rateSample.gather_stamp_raw) >>> 0;
    if (rateSample.gather_stamp_raw === null || rateSample.confidence !== 'direct'
        || derivedAge !== rateSample.age_frames) return fail('host rate cache frame age is inconsistent');
  } else if (rateSample.gather_stamp_raw !== null
      || (rateSample.basis === 'sampled_stock_delta' ? rateSample.confidence !== 'estimated'
        : rateSample.confidence !== 'unavailable')) return fail('host rate sample basis and confidence disagree');
  const expectedSuppressed = [];
  if (!raw.capture.advice_allowed) expectedSuppressed.push('reader_disallowed_advice');
  if (raw.game.mode !== 'single_player') expectedSuppressed.push('not_single_player');
  if (raw.game.human_count !== 1) expectedSuppressed.push('not_unique_human');
  if (raw.game.paused === null) expectedSuppressed.push('pause_state_unknown');
  else if (raw.game.paused) expectedSuppressed.push('game_paused');
  const expectedRateSuppressed = [];
  if (rateSample.basis !== 'engine_direct_gather_cache' || rateSample.confidence !== 'direct') {
    expectedRateSuppressed.push('income_rate_not_engine_direct');
  }
  if (rateSample.age_frames > 45) expectedRateSuppressed.push('income_rate_too_old_for_eta');
  if (!record(analysis) || analysis.source_sequence !== raw.source.sequence || analysis.game_frame !== raw.game.frame
      || typeof analysis.advice_allowed !== 'boolean' || !Array.isArray(analysis.suppressed_reasons)
      || analysis.suppressed_reasons.length !== expectedSuppressed.length
      || analysis.suppressed_reasons.some((reason, index) => reason !== expectedSuppressed[index])
      || analysis.advice_allowed !== (expectedSuppressed.length === 0)
      || typeof analysis.rate_advice_allowed !== 'boolean'
      || !Array.isArray(analysis.rate_suppressed_reasons)
      || analysis.rate_suppressed_reasons.length !== expectedRateSuppressed.length
      || analysis.rate_suppressed_reasons.some((reason, index) => reason !== expectedRateSuppressed[index])
      || analysis.rate_advice_allowed !== (expectedSuppressed.length === 0 && expectedRateSuppressed.length === 0)
      || !record(analysis.metrics)
      || !Array.isArray(analysis.advice) || analysis.advice.length > 32) {
    return fail('host analysis does not match the admitted capture');
  }
  if ((!analysis.advice_allowed && analysis.advice.length !== 0)
      || (analysis.rate_advice_allowed && !analysis.advice_allowed)) return fail('host emitted advice outside its safety gate');
  if (analysis.metrics.income_rate_age_frames !== rateSample.age_frames
      || analysis.metrics.income_rate_basis !== rateSample.basis
      || analysis.metrics.income_rate_confidence !== rateSample.confidence) {
    return fail('host income rate evidence does not match analysis');
  }
  for (const [index, item] of analysis.advice.entries()) {
    if (!record(item) || !text(item.code, 128) || !text(item.severity, 32) || !text(item.title, 512)
        || !text(item.detail, 2_048) || !text(item.action, 512)) return fail(`host advice[${index}] is invalid`);
  }

  const labels = { food: 'Food', timber: 'Timber', wealth: 'Wealth', metal: 'Metal', knowledge: 'Knowledge', oil: 'Oil' };
  const resources = resourceEntries.map(([id, resource]) => ({
    id, label: labels[id], amount: resource.stock, rate: resource.income_per_min, confidence: null,
  }));
  const workers = resourceEntries.filter(([, resource]) => resource.gatherers !== undefined).map(([id, resource]) => ({
    id, label: labels[id], count: resource.gatherers, target: null,
  }));
  if (pop.idle_citizens > 0) workers.push({ id: 'idle', label: 'Idle', count: pop.idle_citizens, target: null });
  const primary = analysis.advice[0];
  const advice = !analysis.advice_allowed ? {
    severity: 'info', headline: 'Economy observed — advice suppressed',
    detail: `The host admitted aggregate telemetry but disabled coaching: ${analysis.suppressed_reasons.join(', ')}.`,
    actions: [], confidence: null,
  } : primary ? {
    severity: primary.severity === 'critical' ? 'urgent' : primary.severity,
    headline: primary.title,
    detail: primary.detail,
    actions: [primary.action],
    confidence: null,
  } : analysis.rate_advice_allowed ? {
    severity: 'good', headline: 'No immediate economic warning',
    detail: `The coherent capture reports ${pop.used}/${pop.cap} population and ${resources.length} resource rates.`,
    actions: [],
    confidence: null,
  } : {
    severity: 'info', headline: 'Economy observed — rate advice suppressed',
    detail: `Population is ${pop.used}/${pop.cap}. Rate-dependent advice is hidden: ${analysis.rate_suppressed_reasons.join(', ')}.`,
    actions: [], confidence: null,
  };
  const gameTime = raw.game.frame / 15;
  const production = prod ? [{
    building: 'Production aggregate', location: `${prod.active_sites} active site${prod.active_sites === 1 ? '' : 's'}`,
    item: 'Reported queue depth', queued: prod.queue_depth, progress: null, remaining: null, blocked: false,
  }] : [];
  const events = analysis.advice.map((item) => ({
    id: item.code, gameTime, kind: item.severity, text: item.title,
  }));
  const snapshot = {
    schema: SCHEMA, version: VERSION, sequence: envelope.stream_revision,
    // Freshness uses the Mac host's receipt clock, never the potentially skewed guest clock.
    capturedAt: new Date(envelope.received_at_ms).toISOString(),
    source: `rontoy host · ${raw.source.session_id}`, confidence: null,
    confidenceLabel: `${rateSample.confidence}${analysis.advice_allowed ? '' : ' · observation only'}${analysis.advice_allowed && !analysis.rate_advice_allowed ? ' · rate advice suppressed' : ''}`,
    rateWindowSeconds: 60, gameTime, gameFrame: raw.game.frame,
    coherence: 'single-frame', rateCacheAgeFrames: rateSample.age_frames,
    capture: {
      complete: true, adviceAllowed: analysis.advice_allowed,
      suppressedReasons: analysis.suppressed_reasons.slice(),
      frameStart: raw.capture.frame_start, frameEnd: raw.capture.frame_end,
    },
    phase: raw.game.age === undefined ? 'Age not reported' : `Age ${raw.game.age}`,
    map: 'own-player aggregate', player: { name: `Player ${raw.game.player_id}`, nation: 'own economy' },
    resources, workers, production, events,
    population: { used: pop.used, cap: pop.cap, trend: null, secondsToCap: null },
    advice,
  };
  const error = validateSnapshot(snapshot, now);
  return error ? fail(`adapted host snapshot failed admission: ${error}`) : { snapshot };
}

function ingestHostEnvelope(envelope) {
  const adapted = adaptHostEnvelope(envelope);
  return adapted.error ? reject(adapted.error) : ingest(adapted.snapshot);
}

function ingestSseData(data) {
  if (typeof data !== 'string') return reject('SSE snapshot data is not text');
  if (new Blob([data]).size > MAX_EVENT_BYTES) return reject('SSE snapshot exceeds the browser admission limit');
  let envelope;
  try { envelope = JSON.parse(data); }
  catch (error) { return reject(`invalid SSE JSON: ${str(error?.message, error)}`); }
  return ingestHostEnvelope(envelope);
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
    coherence: 'single-frame', rateCacheAgeFrames: 3,
    capture: {
      complete: true, adviceAllowed: true,
      suppressedReasons: [],
      frameStart: Math.round(mockGameTime / .067), frameEnd: Math.round(mockGameTime / .067),
    },
    phase: 'Classical Age', map: 'Old World',
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
  clearInterval(state.statusTimer);
  clearTimeout(state.reconnectTimer);
  state.mockTimer = 0;
  state.statusTimer = 0;
  state.reconnectTimer = 0;
}

function closeEventSource() {
  if (!state.eventSource) return;
  const events = state.eventSource;
  state.eventSource = null;
  events.onopen = events.onerror = null;
  events.close();
}

function useMock(delay = 0) {
  clearTimers();
  closeEventSource();
  state.mode = 'mock';
  state.hostAgeMs = null;
  state.hostStale = false;
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

async function pollHostStatus() {
  if (state.mode !== 'sse') return;
  try {
    const response = await fetch('/v1/status', { cache: 'no-store' });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const status = await response.json();
    if (!record(status) || status.ok !== true || status.schema_version !== VERSION) throw new Error('invalid status envelope');
    if (status.latest === null) {
      state.hostAgeMs = null;
      state.hostStale = true;
      updateFreshness(Infinity);
      suppressAdvice('waiting', 'Advice paused — no admitted snapshot',
        'The host is running, but it has not admitted a complete single-frame capture.');
      return;
    }
    if (!record(status.latest) || !number(status.latest.age_ms) || status.latest.age_ms < 0
        || typeof status.latest.stale !== 'boolean' || !integer(status.latest.stream_revision)) {
      throw new Error('invalid latest status');
    }
    state.hostAgeMs = status.latest.age_ms;
    state.hostStale = status.latest.stale;
    if (!state.snapshot || status.latest.stream_revision !== state.snapshot.sequence) {
      state.hostStale = true;
      updateFreshness(state.hostAgeMs);
      suppressAdvice('syncing', 'Advice paused — stream is catching up',
        'The host has a newer admitted capture than the snapshot currently displayed.');
      return;
    }
    updateFreshness(state.hostAgeMs);
  } catch (error) {
    state.hostAgeMs = null;
    state.hostStale = true;
    state.transport = 'status unavailable';
    state.lastError = str(error?.message, error);
    updateTransport('bad');
    suppressAdvice('detached', 'Advice paused — host status unavailable',
      'Freshness cannot be proven without the host monotonic status endpoint.');
  }
}

function connectSse(url = '/v1/stream') {
  clearTimers();
  closeEventSource();
  if (!url) { useMock(); return; }
  let endpoint;
  try { endpoint = new URL(str(url), location.href); }
  catch { reject('SSE URL is invalid'); return; }
  if (endpoint.origin !== location.origin || endpoint.pathname !== '/v1/stream') {
    reject('live telemetry must use the same-origin /v1/stream endpoint');
    return;
  }
  state.mode = 'sse';
  state.hostAgeMs = null;
  state.hostStale = true;
  state.sseUrl = endpoint.href;
  state.source = 'rontoy host';
  state.transport = 'connecting';
  updateTransport('wait');
  suppressAdvice('connecting', 'Advice paused — connecting to host',
    'rontoy will advise only after the host delivers a fresh complete single-frame capture.');
  let events;
  try { events = new EventSource('/v1/stream'); }
  catch (error) { reject(`SSE connection failed: ${str(error?.message, error)}`); return; }
  state.eventSource = events;
  pollHostStatus();
  state.statusTimer = setInterval(pollHostStatus, 1_000);
  events.onopen = () => {
    state.reconnectAttempt = 0;
    state.transport = 'connected · waiting for snapshot';
    updateTransport('live');
  };
  events.addEventListener('snapshot', (event) => {
    if (ingestSseData(event.data)) {
      state.hostAgeMs = 0;
      state.hostStale = false;
      document.querySelector('.advice').classList.remove('detached');
      renderAdmissionAdvice(state.snapshot);
      updateFreshness(0);
      state.transport = 'streaming';
      updateTransport('live');
    }
  });
  events.onerror = () => {
    if (state.eventSource !== events) return;
    state.reconnects++;
    state.transport = 'reconnecting';
    updateTransport('wait');
    suppressAdvice('reconnecting', 'Advice paused — host stream reconnecting',
      'The previous recommendation is hidden until a fresh coherent snapshot arrives.');
  };
}

function reconnect() {
  state.reconnects++;
  suppressAdvice('reconnecting', 'Advice paused — reconnecting telemetry',
    'The previous recommendation is hidden until a fresh coherent snapshot arrives.');
  if (state.mode === 'sse') {
    connectSse(state.sseUrl);
  } else {
    useMock(250);
  }
}

el.reconnect.addEventListener('click', reconnect);
setInterval(() => updateFreshness(), 250);

const params = new URLSearchParams(location.search);
const sse = params.get('sse');
if (sse || location.pathname === '/') connectSse(sse || '/v1/stream');
else useMock();

window.rontoy = Object.freeze({
  schema: SCHEMA,
  version: VERSION,
  ingest,
  ingestHostEnvelope,
  ingestSseData,
  connect: connectSse,
  connectSse,
  useMock,
  reconnect,
  snapshot: () => state.snapshot ? structuredClone(state.snapshot) : null,
  stats: () => ({
    schema: SCHEMA, version: VERSION, mode: state.mode, source: state.source,
    transport: state.transport, sequence: state.snapshot?.sequence ?? null,
    staleMs: state.mode === 'sse' ? state.hostAgeMs
      : state.snapshot ? Date.now() - Date.parse(state.snapshot.capturedAt) : null,
    renders: state.renders, accepted: state.accepted, rejected: state.rejected,
    reconnects: state.reconnects, lastError: state.lastError,
    resources: state.snapshot?.resources?.length ?? 0,
    workers: state.snapshot?.workers?.length ?? 0,
    queues: state.snapshot?.production?.length ?? 0,
    events: state.snapshot?.events?.length ?? 0,
  }),
});
