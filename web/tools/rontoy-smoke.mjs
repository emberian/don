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

try { await fetch(`http://127.0.0.1:${PORT}/rontoy.html`); }
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
  await cdp.send('Page.navigate', { url: `http://127.0.0.1:${PORT}/rontoy.html` });

  let stats;
  for (let i = 0; i < 80; i++) {
    await sleep(100);
    stats = await cdp.eval('window.rontoy?.stats() ?? null');
    if (stats?.sequence >= 2) break;
  }
  assert(stats, 'window.rontoy did not initialize');
  assert(stats.schema === 'rontoy.snapshot' && stats.version === 1, 'snapshot protocol is not v1');
  assert(stats.mode === 'mock' && stats.source === 'demo telemetry', 'default source is not labeled demo telemetry');
  assert(stats.resources === 6, `expected 6 resources, got ${stats.resources}`);
  assert(stats.workers >= 6 && stats.queues >= 4 && stats.events >= 3, 'dashboard sections are incomplete');

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
  assert(dom.resourceCards === 6 && dom.workerRows >= 6 && dom.queueRows >= 4 && dom.eventRows >= 3,
    'snapshot did not render every dashboard section');
  assert(dom.headline.length > 12 && dom.source.includes('demo'), 'advice or source label is missing');
  assert(dom.frameCoherence.includes('single-frame') && dom.rateCacheAge.includes('ms'),
    'coherence or rate-cache age is missing');

  // Replace every externally controlled headline/event/player string with markup. The text
  // must be visible verbatim, and no element or event handler may be created from it.
  const xss = '<img src=x onerror="window.__rontoyXss=1"> coach & player';
  const injection = await cdp.eval(`(() => {
    const s = window.rontoy.snapshot();
    s.sequence += 1000;
    s.capturedAt = new Date().toISOString();
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
  assert(reconnecting === 'reconnecting', 'reconnect state was not exposed');
  await sleep(400);
  const reconnected = await cdp.eval('window.rontoy.stats()');
  assert(reconnected.transport === 'streaming' && reconnected.reconnects === reconnectsBefore + 1,
    'demo transport did not reconnect');

  const errors = cdp.events
    .filter((event) => event.method === 'Log.entryAdded' && event.params.entry.level === 'error')
    .map((event) => event.params.entry.text)
    .concat(cdp.events.filter((event) => event.method === 'Runtime.exceptionThrown')
      .map((event) => event.params.exceptionDetails?.exception?.description ?? 'exception'));
  assert(errors.length === 0, `browser errors: ${errors.join(' | ')}`);

  console.log(`ok  rontoy v${stats.version} · ${dom.resourceCards} resources · ${dom.workerRows} worker rows · ${dom.queueRows} queues`);
  console.log('ok  version rejection · stale signal · reconnect · telemetry escaping');
} catch (error) {
  exitCode = 1;
  console.error(`FAIL ${error.stack ?? error}`);
} finally {
  if (!KEEP && proc) proc.kill();
  if (!KEEP) await rm(profile, { recursive: true, force: true });
}
process.exit(exitCode);
