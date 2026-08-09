// Drive rontoy in a real browser and assert its snapshot, rendering, safety, staleness,
// and reconnect paths. Start the repository server first:
//
//     node web/serve.mjs
//     node web/tools/rontoy-smoke.mjs

import { spawn } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = args.indexOf(name);
  return i < 0 ? fallback : (args[i + 1] ?? true);
};
const PORT = Number(flag('--port', 8787));
const CDP = Number(flag('--cdp', 9337));
const KEEP = args.includes('--keep');
const SSE = args.includes('--sse');
const CHROME = [
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/Applications/Google Chrome Dev.app/Contents/MacOS/Google Chrome Dev',
  '/Applications/Chromium.app/Contents/MacOS/Chromium',
];
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const assert = (condition, message) => { if (!condition) throw new Error(message); };

async function launch(profile) {
  const bin = CHROME.find(existsSync);
  if (!bin) throw new Error(`no Chrome found in ${CHROME.join(', ')}`);
  const proc = spawn(bin, [
    `--remote-debugging-port=${CDP}`, `--user-data-dir=${profile}`,
    '--no-first-run', '--no-default-browser-check', '--disable-sync',
    '--disable-background-timer-throttling', '--disable-renderer-backgrounding',
    '--headless=new', '--window-size=1500,1000', 'about:blank',
  ], { stdio: 'ignore' });
  for (let i = 0; i < 120; i++) {
    try {
      const response = await fetch(`http://127.0.0.1:${CDP}/json/version`);
      if (response.ok) return proc;
    } catch {}
    await sleep(250);
  }
  throw new Error('Chrome did not expose its debugging endpoint');
}

class Cdp {
  constructor(ws) { this.ws = ws; this.id = 0; this.waits = new Map(); this.events = []; }
  static async open(url) {
    const ws = new WebSocket(url);
    await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; });
    const cdp = new Cdp(ws);
    ws.onmessage = (message) => {
      const data = JSON.parse(message.data);
      if (data.id && cdp.waits.has(data.id)) {
        cdp.waits.get(data.id)(data);
        cdp.waits.delete(data.id);
      } else if (data.method) cdp.events.push(data);
    };
    return cdp;
  }
  send(method, params = {}) {
    const id = ++this.id;
    this.ws.send(JSON.stringify({ id, method, params }));
    return new Promise((resolve) => this.waits.set(id, resolve));
  }
  async eval(expression) {
    const response = await this.send('Runtime.evaluate', {
      expression, returnByValue: true, awaitPromise: true,
    });
    if (response.result?.exceptionDetails) {
      throw new Error(response.result.exceptionDetails.exception?.description ?? 'browser evaluation failed');
    }
    return response.result?.result?.value;
  }
}

const pagePath = SSE ? '/' : '/rontoy.html';
try { await fetch(`http://127.0.0.1:${PORT}${pagePath}`); }
catch {
  console.error(`no server on ${PORT} — run: node web/serve.mjs ${PORT}`);
  process.exit(3);
}

const profile = await mkdtemp(join(tmpdir(), 'don-rontoy-smoke-'));
let proc;
let exitCode = 0;
try {
  proc = await launch(profile);
  const targets = await (await fetch(`http://127.0.0.1:${CDP}/json/list`)).json();
  const page = targets.find((target) => target.type === 'page');
  const cdp = await Cdp.open(page.webSocketDebuggerUrl);
  await cdp.send('Runtime.enable');
  await cdp.send('Log.enable');
  await cdp.send('Page.enable');
  await cdp.send('Page.navigate', { url: `http://127.0.0.1:${PORT}${pagePath}` });

  let stats;
  for (let i = 0; i < 80; i++) {
    await sleep(100);
    stats = await cdp.eval('window.rontoy?.stats() ?? null');
    if (stats?.sequence >= 2 && (!SSE || stats.mode === 'sse')) break;
  }
  assert(stats, 'window.rontoy did not initialize');
  assert(stats.schema === 'rontoy.snapshot' && stats.version === 1, 'snapshot protocol is not v1');
  if (SSE) {
    assert(stats.mode === 'sse' && stats.source.includes('rontoy host'), 'dashboard did not attach to host SSE');
    assert(stats.resources >= 4 && stats.workers >= 4 && stats.queues >= 1,
      `host aggregate sections are incomplete: ${JSON.stringify(stats)}`);
  } else {
    assert(stats.mode === 'mock' && stats.source === 'demo telemetry', 'default source is not labeled demo telemetry');
    assert(stats.resources === 6, `expected 6 resources, got ${stats.resources}`);
    assert(stats.workers >= 6 && stats.queues >= 4 && stats.events >= 3, 'dashboard sections are incomplete');
  }

  const dom = await cdp.eval(`(() => ({
    resourceCards: document.querySelectorAll('.resource').length,
    workerRows: document.querySelectorAll('.worker-row').length,
    queueRows: document.querySelectorAll('.queue').length,
    eventRows: document.querySelectorAll('.event').length,
    headline: document.getElementById('adviceHeadline').textContent,
    source: document.getElementById('sourceText').textContent,
    frameCoherence: document.getElementById('frameCoherence').textContent,
    rateCacheAge: document.getElementById('rateCacheAge').textContent,
  }))()`);
  assert(dom.resourceCards >= 4 && dom.workerRows >= 4 && dom.queueRows >= 1 && dom.eventRows >= 1,
    'snapshot did not render every dashboard section');
  assert(dom.headline.length > 12 && (SSE ? dom.source.includes('rontoy host') : dom.source.includes('demo')),
    'advice or source label is missing');
  assert(dom.frameCoherence.includes('single-frame') && dom.rateCacheAge.includes('sim frame'),
    'coherence or rate-cache age is missing');

  const hostAdmission = await cdp.eval(`(() => {
    const now = Date.now();
    const envelope = {
      stream_revision: 5000,
      received_at_ms: now,
      snapshot: {
        schema_version: 1,
        source: {
          session_id: 'smoke-session', sequence: 41, captured_at_ms: now,
          process_id: 41, process_started_100ns: '133994400000000000',
          module_sha256: '1'.repeat(64), module_size: 12345678,
          image_entry_rva: 1431193, image_size: 12271616,
        },
        capture: {
          frame_start: 630, frame_end: 630, complete: true, advice_allowed: true,
          duration_us: 250, read_count: 12, bytes_read: 256,
        },
        game: {
          frame: 630, player_id: 1, mode: 'single_player', age: 2, paused: false,
          human_count: 1, human_selection_basis: 'unique_active_in_play_console_flags',
        },
        economy: {
          resources: {
            food: { stock: 180, income_per_min: 62, gatherers: 7, gatherers_basis: 'direct_count' },
            timber: { stock: 75, income_per_min: 38, gatherers: 5, gatherers_basis: 'direct_count' },
            wealth: { stock: 30, income_per_min: 22, gatherers: 3, gatherers_basis: 'direct_count' },
            knowledge: { stock: 18, income_per_min: 14, gatherers: 2, gatherers_basis: 'direct_count' },
          },
          population: { used: 30, cap: 32, idle_citizens: 1, idle_basis: 'direct_count' },
          rate_sample: { basis: 'engine_direct_gather_cache', gather_stamp_raw: 625, age_frames: 5, confidence: 'direct' },
          production: { queue_depth: 2, active_sites: 1, queue_basis: 'direct_build_queue' },
        },
        goals: [],
      },
      analysis: {
        advisor_version: 'smoke', advice_allowed: true, rate_advice_allowed: true,
        suppressed_reasons: [], rate_suppressed_reasons: [],
        source_sequence: 41, game_frame: 630,
        metrics: {
          population_headroom: 2, income_rate_age_frames: 5,
          income_rate_basis: 'engine_direct_gather_cache', income_rate_confidence: 'direct',
        },
        advice: [{
          code: 'population_headroom_low', severity: 'warning', title: 'Population headroom low',
          detail: 'Only 2 population slots remain.', action: 'Start capacity before more production.', evidence: {},
        }],
      },
    };
    window.__rontoyHostEnvelope = envelope;
    const accepted = window.rontoy.ingestHostEnvelope(envelope);
    return {
      accepted,
      snapshot: window.rontoy.snapshot(),
      targets: [...document.querySelectorAll('.worker-target')].map((n) => n.textContent),
      moves: [...document.querySelectorAll('.worker-move')].map((n) => n.textContent),
      headline: document.getElementById('adviceHeadline').textContent,
      cache: document.getElementById('rateCacheAge').textContent,
    };
  })()`);
  assert(hostAdmission.accepted && hostAdmission.snapshot.sequence === 5000,
    'canonical host envelope was not admitted');
  assert(hostAdmission.snapshot.capture.adviceAllowed && hostAdmission.snapshot.capture.frameStart === 630,
    'host envelope did not produce an explicitly advice-allowed coherent capture');
  assert(hostAdmission.targets.every((value) => value === 'observed')
      && hostAdmission.moves.every((value) => value === '—'),
    'missing worker targets produced fabricated reallocation advice');
  assert(hostAdmission.headline === 'Population headroom low' && hostAdmission.cache === '5 sim frames',
    'host advice or rate-cache age was misrepresented');

  const observationOnly = await cdp.eval(`(() => {
    const observed = structuredClone(window.__rontoyHostEnvelope);
    observed.stream_revision = 5001;
    observed.snapshot.source.sequence = 42;
    observed.analysis.source_sequence = 42;
    observed.snapshot.capture.advice_allowed = false;
    observed.snapshot.game.paused = null;
    observed.analysis.advice_allowed = false;
    observed.analysis.rate_advice_allowed = false;
    observed.analysis.suppressed_reasons = ['reader_disallowed_advice', 'pause_state_unknown'];
    observed.analysis.advice = [];
    const accepted = window.rontoy.ingestHostEnvelope(observed);
    const result = {
      accepted,
      headline: document.getElementById('adviceHeadline').textContent,
      actions: document.querySelectorAll('.action').length,
      adviceAllowed: window.rontoy.snapshot()?.capture.adviceAllowed,
    };
    const restored = structuredClone(window.__rontoyHostEnvelope);
    restored.stream_revision = 5002;
    restored.snapshot.source.sequence = 43;
    restored.analysis.source_sequence = 43;
    window.__rontoyHostEnvelope = restored;
    result.recovery = window.rontoy.ingestHostEnvelope(restored);
    return result;
  })()`);
  assert(observationOnly.accepted && observationOnly.recovery && observationOnly.adviceAllowed === false
      && observationOnly.headline.includes('advice suppressed') && observationOnly.actions === 0,
    `observation-only host snapshot was misrepresented: ${JSON.stringify(observationOnly)}`);

  const rateSuppression = await cdp.eval(`(() => {
    const suppressed = structuredClone(window.__rontoyHostEnvelope);
    suppressed.stream_revision = 5003;
    suppressed.snapshot.source.sequence = 44;
    suppressed.analysis.source_sequence = 44;
    suppressed.snapshot.economy.rate_sample = {
      basis: 'sampled_stock_delta', gather_stamp_raw: null, age_frames: 15, confidence: 'estimated',
    };
    suppressed.analysis.rate_advice_allowed = false;
    suppressed.analysis.rate_suppressed_reasons = ['income_rate_not_engine_direct'];
    suppressed.analysis.metrics.income_rate_basis = 'sampled_stock_delta';
    suppressed.analysis.metrics.income_rate_age_frames = 15;
    suppressed.analysis.metrics.income_rate_confidence = 'estimated';
    suppressed.analysis.advice = [];
    const accepted = window.rontoy.ingestHostEnvelope(suppressed);
    const headline = document.getElementById('adviceHeadline').textContent;
    const actions = document.querySelectorAll('.action').length;
    const confidence = document.getElementById('overallConfidence').textContent;
    const restored = structuredClone(window.__rontoyHostEnvelope);
    restored.stream_revision = 5004;
    restored.snapshot.source.sequence = 45;
    restored.analysis.source_sequence = 45;
    window.__rontoyHostEnvelope = restored;
    const recovery = window.rontoy.ingestHostEnvelope(restored);
    return { accepted, headline, actions, confidence, recovery };
  })()`);
  assert(rateSuppression.accepted && rateSuppression.recovery && rateSuppression.actions === 0
      && rateSuppression.headline.includes('rate advice suppressed')
      && rateSuppression.confidence.includes('rate advice suppressed'),
    `rate-dependent advice was not selectively suppressed: ${JSON.stringify(rateSuppression)}`);

  const hardening = await cdp.eval(`(() => {
    const attemptEnvelope = (mutate) => {
      const e = structuredClone(window.__rontoyHostEnvelope); e.stream_revision += 1; mutate(e);
      return window.rontoy.ingestHostEnvelope(e);
    };
    const attemptSnapshot = (mutate) => {
      const s = window.rontoy.snapshot(); s.sequence += 1; s.capturedAt = new Date().toISOString(); mutate(s);
      return window.rontoy.ingest(s);
    };
    const before = window.rontoy.stats();
    const results = {
      mixedFrame: attemptEnvelope((e) => { e.snapshot.capture.frame_end += 1; }),
      readerDisallowed: attemptEnvelope((e) => { e.snapshot.capture.advice_allowed = false; }),
      analysisMismatch: attemptEnvelope((e) => { e.analysis.game_frame += 1; }),
      advisorDisallowed: attemptEnvelope((e) => { e.analysis.advice_allowed = false; e.analysis.suppressed_reasons = ['income_rate_stale']; }),
      staleRateClaim: attemptEnvelope((e) => { e.snapshot.economy.rate_sample.age_frames = 3001; }),
      mismatchedRateClaim: attemptEnvelope((e) => { e.analysis.metrics.income_rate_age_frames = 1; }),
      nullAdvice: attemptEnvelope((e) => { e.analysis.advice[0] = null; }),
      unprovedQueue: attemptEnvelope((e) => { delete e.snapshot.economy.production.queue_basis; }),
      numericProcessStart: attemptEnvelope((e) => { e.snapshot.source.process_started_100ns = 1; }),
      zeroProcessStart: attemptEnvelope((e) => { e.snapshot.source.process_started_100ns = '0'; }),
      overflowingProcessStart: attemptEnvelope((e) => { e.snapshot.source.process_started_100ns = '18446744073709551616'; }),
      invalidSessionId: attemptEnvelope((e) => { e.snapshot.source.session_id = 'bad session'; }),
      invalidSourceSequence: attemptEnvelope((e) => { e.snapshot.source.sequence = '46'; }),
      futureProducerCapture: attemptEnvelope((e) => { e.snapshot.source.captured_at_ms = Date.now() + 600000; }),
      invalidProcessId: attemptEnvelope((e) => { e.snapshot.source.process_id = 0; }),
      invalidModuleSize: attemptEnvelope((e) => { e.snapshot.source.module_size = 0; }),
      missingEntryRva: attemptEnvelope((e) => { delete e.snapshot.source.image_entry_rva; }),
      invalidImageSize: attemptEnvelope((e) => { e.snapshot.source.image_size = 0; }),
      uppercaseModuleHash: attemptEnvelope((e) => { e.snapshot.source.module_sha256 = 'A'.repeat(64); }),
      invalidPlayerId: attemptEnvelope((e) => { e.snapshot.game.player_id = 16; }),
      unsafeModeWithAdvice: attemptEnvelope((e) => { e.snapshot.game.mode = 'multiplayer'; }),
      missingHumanCount: attemptEnvelope((e) => { delete e.snapshot.game.human_count; }),
      unprovedHumanBasis: attemptEnvelope((e) => { e.snapshot.game.human_selection_basis = 'guess'; }),
      unknownPauseWithAdvice: attemptEnvelope((e) => { e.snapshot.game.paused = null; }),
      pausedWithAdvice: attemptEnvelope((e) => { e.snapshot.game.paused = true; }),
      futureReceipt: attemptEnvelope((e) => { e.received_at_ms = Date.now() + 5000; }),
      nullWorker: attemptSnapshot((s) => { s.workers[0] = null; }),
      noAdviceGate: attemptSnapshot((s) => { delete s.capture.adviceAllowed; }),
      futureCapture: attemptSnapshot((s) => { s.capturedAt = new Date(Date.now() + 5000).toISOString(); }),
      oversize: window.rontoy.ingestSseData(' '.repeat(128 * 1024 + 1)),
    };
    const after = window.rontoy.stats();
    return {
      results, beforeSequence: before.sequence, afterSequence: after.sequence,
      rejectedDelta: after.rejected - before.rejected,
      headline: document.getElementById('adviceHeadline').textContent,
      actions: document.querySelectorAll('.action').length,
    };
  })()`);
  assert(Object.values(hardening.results).every((accepted) => accepted === false),
    `unsafe payload was admitted: ${JSON.stringify(hardening.results)}`);
  assert(hardening.beforeSequence === hardening.afterSequence && hardening.rejectedDelta === 30,
    'rejected payload mutated the last admitted snapshot');
  assert(hardening.headline.includes('suppressed') && hardening.actions === 0,
    'rejection did not immediately suppress actionable advice');

  const overCap = await cdp.eval(`(() => {
    const s = window.rontoy.snapshot();
    s.sequence += 1; s.capturedAt = new Date().toISOString(); s.population.used = s.population.cap + 3;
    const accepted = window.rontoy.ingest(s);
    return {
      accepted,
      headroom: document.getElementById('popFree').textContent,
      status: document.getElementById('capStatus').textContent,
    };
  })()`);
  assert(overCap.accepted && overCap.headroom === '3 over cap' && overCap.status === 'over cap',
    `legal over-cap state was misrepresented: ${JSON.stringify(overCap)}`);

  // Replace every externally controlled headline/event/player string with markup. The text
  // must be visible verbatim, and no element or event handler may be created from it.
  const xss = '<img src=x onerror="window.__rontoyXss=1"> coach & player';
  const injection = await cdp.eval(`(() => {
    const s = window.rontoy.snapshot();
    s.sequence += 1000;
    s.capturedAt = new Date().toISOString();
    s.source = 'smoke hostile-string fixture';
    s.player.name = ${JSON.stringify(xss)};
    s.advice.headline = ${JSON.stringify(xss)};
    s.advice.detail = ${JSON.stringify(xss)};
    s.advice.actions = [${JSON.stringify(xss)}];
    s.events = [{ gameTime: 1, kind: ${JSON.stringify(xss)}, text: ${JSON.stringify(xss)} }];
    const accepted = window.rontoy.ingest(s);
    return {
      accepted,
      headline: document.getElementById('adviceHeadline').textContent,
      player: document.getElementById('playerName').textContent,
      action: document.querySelector('.action')?.textContent,
      event: document.querySelector('.event-copy')?.textContent,
      injectedElements: document.querySelectorAll('img').length,
      fired: window.__rontoyXss === 1,
    };
  })()`);
  assert(injection.accepted, 'valid hostile-string snapshot was rejected');
  assert(injection.headline === xss && injection.player === xss && injection.action === xss,
    'external strings did not survive as text');
  assert(injection.event.includes(xss) && injection.injectedElements === 0 && !injection.fired,
    'telemetry created executable markup');

  const beforeReject = (await cdp.eval('window.rontoy.stats()')).rejected;
  const rejected = await cdp.eval(`(() => {
    const s = window.rontoy.snapshot(); s.version = 2; return window.rontoy.ingest(s);
  })()`);
  const afterReject = await cdp.eval('window.rontoy.stats()');
  assert(rejected === false && afterReject.rejected === beforeReject + 1,
    'incompatible snapshot version was not rejected');

  const stale = await cdp.eval(`(() => {
    const s = window.rontoy.snapshot(); s.sequence += 1;
    s.capturedAt = new Date(Date.now() - 8000).toISOString();
    window.rontoy.ingest(s);
    return {
      status: document.getElementById('freshText').textContent,
      headline: document.getElementById('adviceHeadline').textContent,
      actions: document.querySelectorAll('.action').length,
    };
  })()`);
  assert(stale.status.includes('stale') && stale.headline.includes('paused') && stale.actions === 0,
    `stale snapshot did not suppress advice: ${JSON.stringify(stale)}`);

  const reconnectsBefore = afterReject.reconnects;
  await cdp.eval('window.rontoy.reconnect()');
  const reconnecting = await cdp.eval('window.rontoy.stats().transport');
  assert(['reconnecting', 'connecting'].includes(reconnecting), 'reconnect state was not exposed');
  await sleep(SSE ? 900 : 400);
  const reconnected = await cdp.eval('window.rontoy.stats()');
  assert(reconnected.transport === 'streaming' && reconnected.reconnects === reconnectsBefore + 1,
    `transport did not reconnect: before=${JSON.stringify(afterReject)} after=${JSON.stringify(reconnected)}`);

  const errors = cdp.events
    .filter((event) => event.method === 'Log.entryAdded' && event.params.entry.level === 'error')
    .map((event) => event.params.entry.text)
    .concat(cdp.events.filter((event) => event.method === 'Runtime.exceptionThrown')
      .map((event) => event.params.exceptionDetails?.exception?.description ?? 'exception'));
  assert(errors.length === 0, `browser errors: ${errors.join(' | ')}`);

  console.log(`ok  rontoy v${stats.version} ${SSE ? 'host SSE' : 'demo'} · ${dom.resourceCards} resources · ${dom.workerRows} worker rows · ${dom.queueRows} queues`);
  console.log('ok  host admission · coherence gates · version rejection · stale suppression · reconnect · telemetry escaping');
} catch (error) {
  exitCode = 1;
  console.error(`FAIL ${error.stack ?? error}`);
} finally {
  if (!KEEP && proc) {
    const exited = new Promise((resolve) => proc.once('exit', resolve));
    proc.kill();
    await Promise.race([exited, sleep(2_000)]);
  }
  if (!KEEP) {
    for (let attempt = 0; attempt < 4; attempt++) {
      try { await rm(profile, { recursive: true, force: true }); break; }
      catch (error) { if (attempt === 3) throw error; await sleep(150); }
    }
  }
}
process.exit(exitCode);
