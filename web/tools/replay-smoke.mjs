// Drive the replay viewer in a real browser and assert it actually decoded and played.
//
// A page that "looks right" when you click it is not a measurement. This launches Chrome,
// loads `replay.html?file=...`, scrubs the transport across the recording, and reads back
// `window.donReplay.stats()` — the same counters the page shows — plus every console error
// and page exception. Exit code 0 only if the file decoded with zero framing residue, the
// coverage counters add up to the command count, and nothing threw.
//
//     node web/serve.mjs &                       # the page needs an origin
//     node web/tools/replay-smoke.mjs            # first three indexed replays
//     node web/tools/replay-smoke.mjs --file today.rcx --keep
//     node web/tools/replay-smoke.mjs --all --json smoke.json

import { spawn } from 'node:child_process';
import { mkdtemp, rm, writeFile, readFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const flag = (n, d = null) => { const i = args.indexOf(n); return i < 0 ? d : (args[i + 1] ?? true); };
const has = (n) => args.includes(n);

const PORT = Number(flag('--port', 8787));
const CDP = Number(flag('--cdp', 9334));
const KEEP = has('--keep');
const OUT = flag('--json', null);

const CHROME = [
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/Applications/Google Chrome Dev.app/Contents/MacOS/Google Chrome Dev',
  '/Applications/Chromium.app/Contents/MacOS/Chromium',
];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function launch(profile) {
  const bin = CHROME.find((p) => existsSync(p));
  if (!bin) throw new Error('no Chrome found in ' + CHROME.join(', '));
  const proc = spawn(bin, [
    `--remote-debugging-port=${CDP}`, `--user-data-dir=${profile}`,
    '--no-first-run', '--no-default-browser-check', '--disable-sync',
    '--disable-background-timer-throttling', '--disable-renderer-backgrounding',
    '--disable-backgrounding-occluded-windows',
    '--headless=new', '--window-size=1600,1000', 'about:blank',
  ], { stdio: 'ignore' });
  for (let i = 0; i < 120; i++) {
    try { const r = await fetch(`http://127.0.0.1:${CDP}/json/version`); if (r.ok) return { proc, bin }; } catch {}
    await sleep(250);
  }
  await stopChrome(proc);
  throw new Error('chrome did not come up');
}

class Cdp {
  constructor(ws) { this.ws = ws; this.id = 0; this.waits = new Map(); this.events = []; }
  static async open(url) {
    const ws = new WebSocket(url);
    await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
    const c = new Cdp(ws);
    ws.onmessage = (m) => {
      const d = JSON.parse(m.data);
      if (d.id && c.waits.has(d.id)) { c.waits.get(d.id)(d); c.waits.delete(d.id); }
      else if (d.method) c.events.push(d);
    };
    return c;
  }
  send(method, params = {}) {
    const id = ++this.id;
    this.ws.send(JSON.stringify({ id, method, params }));
    return new Promise((res) => this.waits.set(id, res));
  }
  async eval(expr) {
    const r = await this.send('Runtime.evaluate', {
      expression: expr, returnByValue: true, awaitPromise: true,
    });
    if (r.result?.exceptionDetails) throw new Error(JSON.stringify(r.result.exceptionDetails).slice(0, 400));
    return r.result?.result?.value;
  }
  close() { this.ws.close(); }
}

async function stopChrome(proc) {
  if (proc.exitCode !== null) return;
  const exited = new Promise((resolve) => proc.once('exit', resolve));
  proc.kill('SIGTERM');
  const clean = await Promise.race([exited.then(() => true), sleep(4000).then(() => false)]);
  if (!clean && proc.exitCode === null) {
    proc.kill('SIGKILL');
    await exited;
  }
}

// ---------------------------------------------------------------------------

try { await fetch(`http://127.0.0.1:${PORT}/replay.html`); }
catch { console.error(`no server on ${PORT} — run: node web/serve.mjs ${PORT}`); process.exit(3); }

let index;
try {
  index = JSON.parse(await readFile(join(HERE, '..', 'public', 'data', 'replays.json'), 'utf8'));
} catch { console.error('no public/data/replays.json — run: node web/tools/pack-replays.mjs'); process.exit(3); }

let files;
if (flag('--file')) files = [flag('--file')];
else {
  const ok = index.replays.filter((r) => !r.error && r.commandStream !== false);
  // The corpus's named solo control is explicit. `xorKey === 0` is not a game-mode
  // classifier: it is an output of the payload decoder and can also occur by chance.
  const solo = ok.find((r) => r.file === 'today.rcx')
    ?? ok.find((r) => r.obfuscationSource === 'plain' && r.commands > 0 && !r.checksumPackets);
  const mp = ok.filter((r) => r.checksumPackets > 0);
  files = has('--all') ? ok.map((r) => r.file)
    : [solo?.file, mp[0]?.file, mp[mp.length - 1]?.file].filter(Boolean);
}

const profile = await mkdtemp(join(tmpdir(), 'don-replay-smoke-'));
const results = [];
let bad = 0;
let proc = null;
let c = null;
try {
  ({ proc } = await launch(profile));
  const targets = await (await fetch(`http://127.0.0.1:${CDP}/json/list`)).json();
  const page = targets.find((t) => t.type === 'page');
  c = await Cdp.open(page.webSocketDebuggerUrl);
  await c.send('Runtime.enable');
  await c.send('Log.enable');
  await c.send('Page.enable');

  for (const f of files) {
    c.events.length = 0;
    const decodeT0 = performance.now();
    await c.send('Page.navigate', { url: `http://127.0.0.1:${PORT}/replay.html?file=${encodeURIComponent(f)}` });
    let stats = null;
    for (let i = 0; i < 240; i++) {
      await sleep(250);
      stats = await c.eval('window.donReplay ? window.donReplay.stats() : null');
      if (stats?.name === f) break;
      stats = null;
    }
    const decodeMs = Math.round(performance.now() - decodeT0);
    if (!stats) { results.push({ file: f, error: 'never decoded' }); bad++; continue; }

    // The repeated 35% probe is the backward-seek assertion: rebuilding after 90% must
    // produce byte-for-byte identical counters to the first forward visit at 35%.
    const last = await c.eval('window.donReplay.replay.frameLast');
    const probes = [0, 0.25, 0.35, 0.9, 0.35, 1].map((k) => Math.floor(k * last));
    const cov = [];
    for (const p of probes) {
      await c.eval(`window.donReplay.seek(${p})`);
      cov.push(await c.eval('JSON.stringify(window.donReplay.model.counts)'));
    }
    const backwardDeterministic = cov[2] === cov[4];
    await c.eval('window.donReplay.seek(0); window.donReplay.play()');
    await sleep(1200);
    await c.eval('window.donReplay.pause()');
    const after = await c.eval('window.donReplay.stats()');

    const errs = c.events
      .filter((e) => e.method === 'Log.entryAdded' && e.params.entry.level === 'error')
      .map((e) => e.params.entry.text)
      .concat(c.events.filter((e) => e.method === 'Runtime.exceptionThrown')
        .map((e) => e.params.exceptionDetails?.exception?.description ?? 'exception'));

    const final = JSON.parse(cov[cov.length - 1]);
    const sums = final.interpreted + final.plotted + final.annotated;
    const ok = stats.residue === 0 && stats.decoded === stats.packages
      && stats.checksumTotalOk === stats.checksumPackets
      && stats.checksumShapeOk === stats.checksumPackets
      && sums === final.total && final.total === stats.commands
      && backwardDeterministic && decodeMs <= 30000 && errs.length === 0;
    if (!ok) bad++;
    results.push({
      file: f, ok, packages: stats.packages, decoded: stats.decoded,
      residue: stats.residue, turns: stats.turns, commands: stats.commands,
      checksumPackets: stats.checksumPackets, checksumTotalOk: stats.checksumTotalOk,
      checksumShapeOk: stats.checksumShapeOk, obfuscationSource: stats.obfuscationSource,
      decodeMs, coverageAtEnd: final, classesSumToTotal: sums === final.total,
      backwardDeterministic, coverageAcrossScrub: cov.map((s) => JSON.parse(s).total),
      playedFrames: after ? after.coverage.total : null,
      errors: errs,
    });
    const pct = (100 * (final.oInterpreted + final.oPlotted) / Math.max(1, final.orders)).toFixed(1);
    console.log(`${ok ? 'ok  ' : 'FAIL'} ${f.slice(0, 46).padEnd(46)} ` +
      `pkgs ${String(stats.packages).padStart(7)} cmds ${String(stats.commands).padStart(7)} ` +
      `acted-on ${pct.padStart(5)}%  wasm-handler-not-run ${final.wasmHandler}` +
      (errs.length ? `  ERR ${errs[0].slice(0, 80)}` : ''));
  }
} finally {
  c?.close();
  if (!KEEP && proc) await stopChrome(proc);
  if (!KEEP) await rm(profile, { recursive: true, force: true });
}

if (OUT) await writeFile(OUT, JSON.stringify({ generated_by: 'web/tools/replay-smoke.mjs', results }, null, 1));
console.log(`\n${results.length - bad}/${results.length} pages healthy`);
process.exitCode = bad ? 1 : 0;
