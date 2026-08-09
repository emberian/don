// Automated measurement driver. Launches Chrome, drives the page over the DevTools
// protocol, and prints one JSON document of results.
//
// Dependency-free on purpose: Node's global `WebSocket` and `fetch` are enough to speak
// CDP, so there is no `npm install` between a fresh checkout and a reproducible number.
//
// # Why the browser is launched *headed*
//
// A headless Chrome on macOS falls back to a software rasteriser for a lot of GPU work, and
// a software number reported as a GPU number is worse than no number. This launches a real
// window on the real Metal device, in a throwaway profile so it cannot disturb anything.
// `--disable-*-throttling` keeps an unfocused window from being clamped, which would
// otherwise silently halve every `requestAnimationFrame` measurement.
//
//     node web/bench.mjs                       # default sweep
//     node web/bench.mjs --quick               # short sweep
//     node web/bench.mjs --out results.json
//     node web/bench.mjs --port 8788 --keep    # leave the browser open

import { spawn } from 'node:child_process';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir, loadavg, cpus } from 'node:os';
import { join } from 'node:path';

const args = process.argv.slice(2);
const flag = (n, d = null) => { const i = args.indexOf(n); return i < 0 ? d : (args[i + 1] ?? true); };
const has = (n) => args.includes(n);

const PORT = Number(flag('--port', 8787));
const CDP = Number(flag('--cdp', 9333));
const OUT = flag('--out', null);
const QUICK = has('--quick');
const KEEP = has('--keep');
const MS = Number(flag('--ms', QUICK ? 1500 : 3000));
// This machine is shared with other agents. A single timing on a box at load average 60
// is not a measurement, it is a sample from an unknown distribution -- so every row runs
// REPEATS times and the report keeps all of them plus the median.
const REPEATS = Number(flag('--repeats', QUICK ? 2 : 3));

const median = (xs) => { const s = [...xs].sort((a, b) => a - b); const h = s.length >> 1;
  return s.length % 2 ? s[h] : (s[h - 1] + s[h]) / 2; };

const CHROME = [
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/Applications/Google Chrome Dev.app/Contents/MacOS/Google Chrome Dev',
  '/Applications/Chromium.app/Contents/MacOS/Chromium',
];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function launch(profile) {
  const { existsSync } = await import('node:fs');
  const bin = CHROME.find((p) => existsSync(p));
  if (!bin) throw new Error('no Chrome found in ' + CHROME.join(', '));
  const proc = spawn(bin, [
    `--remote-debugging-port=${CDP}`,
    `--user-data-dir=${profile}`,
    '--no-first-run', '--no-default-browser-check', '--disable-sync',
    '--disable-background-timer-throttling',
    '--disable-renderer-backgrounding',
    '--disable-backgrounding-occluded-windows',
    // Timestamp queries are behind the developer-features gate in stable Chrome. Without
    // them the page still runs; it just reports gpuMs = 0.
    '--enable-webgpu-developer-features',
    '--enable-dawn-features=allow_unsafe_apis',
    '--window-size=1600,1000',
    'about:blank',
  ], { stdio: 'ignore', detached: false });
  for (let i = 0; i < 120; i++) {
    try { const r = await fetch(`http://127.0.0.1:${CDP}/json/version`); if (r.ok) return { proc, bin, version: await r.json() }; } catch {}
    await sleep(150);
  }
  throw new Error('Chrome did not open a debugging port');
}

class Cdp {
  #ws; #id = 0; #pending = new Map();
  static async open(wsUrl) {
    const ws = new WebSocket(wsUrl);
    await new Promise((res, rej) => { ws.onopen = res; ws.onerror = () => rej(new Error('cdp connect failed')); });
    return new Cdp(ws);
  }
  #events = new Map();
  constructor(ws) {
    this.#ws = ws;
    ws.onmessage = (e) => {
      const m = JSON.parse(e.data);
      if (m.method) { this.#events.get(m.method)?.(m.params); return; }
      const p = this.#pending.get(m.id);
      if (!p) return;
      this.#pending.delete(m.id);
      m.error ? p.rej(new Error(m.error.message)) : p.res(m.result);
    };
  }
  /** Subscribe to a CDP event. Page errors must be visible: a worker that throws during
   *  boot otherwise looks exactly like a slow boot. */
  on(method, fn) { this.#events.set(method, fn); }
  send(method, params = {}) {
    const id = ++this.#id;
    return new Promise((res, rej) => { this.#pending.set(id, { res, rej }); this.#ws.send(JSON.stringify({ id, method, params })); });
  }
  /** Evaluate an async expression in the page and return its value. */
  async eval(expr, timeout = 600000) {
    const r = await this.send('Runtime.evaluate', {
      expression: `(async () => { ${expr} })()`,
      awaitPromise: true, returnByValue: true, timeout,
    });
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || JSON.stringify(r.exceptionDetails));
    return r.result.value;
  }
  close() { this.#ws.close(); }
}

async function main() {
  const profile = await mkdtemp(join(tmpdir(), 'don-bench-'));
  const { proc, bin, version } = await launch(profile);
  const url = `http://127.0.0.1:${PORT}/`;
  const t = await (await fetch(`http://127.0.0.1:${CDP}/json/new?${encodeURIComponent(url)}`, { method: 'PUT' })).json();
  const cdp = await Cdp.open(t.webSocketDebuggerUrl);
  const pageErrors = [];
  cdp.on('Runtime.exceptionThrown', (p) => {
    const d = p.exceptionDetails?.exception?.description || p.exceptionDetails?.text;
    pageErrors.push(d); console.error('  [page exception] ' + d);
  });
  cdp.on('Runtime.consoleAPICalled', (p) => {
    if (p.type !== 'error' && p.type !== 'warning') return;
    const s = p.args.map((a) => a.value ?? a.description ?? a.type).join(' ');
    pageErrors.push(s); console.error(`  [console.${p.type}] ${s}`);
  });
  cdp.on('Log.entryAdded', (p) => {
    if (p.entry.level !== 'error') return;
    pageErrors.push(p.entry.text); console.error('  [log] ' + p.entry.text);
  });
  await cdp.send('Runtime.enable');
  await cdp.send('Log.enable').catch(() => {});
  // Workers are separate targets; without auto-attach a worker exception is invisible here.
  await cdp.send('Target.setAutoAttach', { autoAttach: true, waitForDebuggerOnStart: false, flatten: true }).catch(() => {});

  // Wait for the app to finish its own boot rather than guessing with a sleep.
  for (let i = 0; i < 200; i++) {
    const ok = await cdp.eval('return typeof window.don !== "undefined";').catch(() => false);
    if (ok) break;
    await sleep(150);
  }
  await cdp.eval('await window.don.ready; return true;');

  const env = await cdp.eval('return { ...window.don.state, ...window.don.hardware };');
  console.error(`chrome: ${version.Browser}  backend: ${env.backend}  isolated: ${env.isolated}  timestamps: ${env.timestamps}  cores: ${env.cores}  dpr: ${env.dpr}`);

  // Every suite runs REPEATS times; rows are keyed by configuration and carry every
  // sample, so the report can show the spread instead of hiding it behind one number.
  const run = async (label, expr) => {
    console.error(`-- ${label}`);
    const passes = [];
    for (let i = 0; i < REPEATS; i++) passes.push([].concat(await cdp.eval(expr)));
    const keyed = new Map();
    for (const pass of passes) {
      for (const x of pass) {
        const k = `${x.kind}|${x.path}|${x.cfg.worlds}|${x.cfg.owners}|${x.cfg.units}|${x.cfg.shards}`;
        if (!keyed.has(k)) keyed.set(k, { ...x, fpsSamples: [], stepSamples: [], uploadSamples: [], gpuSamples: [] });
        const e = keyed.get(k);
        e.fpsSamples.push(x.fps); e.stepSamples.push(x.stepMs ?? 0);
        e.uploadSamples.push(x.uploadMs ?? 0); e.gpuSamples.push(x.gpuMs ?? 0);
      }
    }
    const rows = [...keyed.values()].map((e) => ({
      ...e, fps: median(e.fpsSamples), stepMs: median(e.stepSamples),
      uploadMs: median(e.uploadSamples), gpuMs: median(e.gpuSamples),
      fpsMin: Math.min(...e.fpsSamples), fpsMax: Math.max(...e.fpsSamples),
      loadavg: loadavg()[0],
    }));
    for (const x of rows) {
      console.error(`   ${x.kind} ${x.path} worlds=${x.cfg.worlds} units=${x.cfg.units} shards=${x.cfg.shards} inst=${x.instances} -> ${x.fps.toFixed(1)} fps [${x.fpsMin.toFixed(1)}..${x.fpsMax.toFixed(1)}]  step=${x.stepMs.toFixed(2)} upl=${x.uploadMs.toFixed(2)} gpu=${x.gpuMs.toFixed(2)}  load=${x.loadavg.toFixed(0)}`);
    }
    return rows;
  };

  const out = {
    env, chrome: version, ms: MS, repeats: REPEATS, when: new Date().toISOString(),
    // The load average is part of every number below. Recorded, not assumed away.
    machine: { cpus: cpus().length, model: cpus()[0]?.model, loadavgStart: loadavg() },
    suites: {},
  };

  // `units` is now units *per side*, and every world has two sides, so instance counts are
  // twice these numbers. Stated here because a sweep axis that silently means something
  // different from last time is how two reports get compared that should not be.
  const unitsAxis = QUICK ? [256, 1024] : [64, 256, 512, 1024, 2048];
  const worldsAxis = QUICK ? [64, 1024] : [16, 64, 256, 1024, 4096];
  const OWNERS = 2;

  // 1. One world, zero-copy path: entity count against frame rate.
  out.suites.units_zerocopy = await run('units sweep, zerocopy, uncapped',
    `return await window.don.sweep('units', ${JSON.stringify(unitsAxis)}, 'uncapped', ${MS}, {worlds:1, owners:${OWNERS}, path:'zerocopy', simHz:0});`);
  out.suites.units_zerocopy_raf = await run('units sweep, zerocopy, rAF',
    `return await window.don.sweep('units', ${JSON.stringify(unitsAxis)}, 'raf', ${MS}, {worlds:1, owners:${OWNERS}, path:'zerocopy', simHz:0});`);

  // 2. World count against frame rate, sim inline in the render worker.
  out.suites.worlds_inline = await run('worlds sweep, inline, uncapped',
    `return await window.don.sweep('worlds', ${JSON.stringify(worldsAxis)}, 'uncapped', ${MS}, {units:32, owners:${OWNERS}, path:'inline', simHz:0});`);
  out.suites.worlds_inline_raf = await run('worlds sweep, inline, rAF',
    `return await window.don.sweep('worlds', ${JSON.stringify(worldsAxis)}, 'raf', ${MS}, {units:32, owners:${OWNERS}, path:'inline', simHz:0});`);

  // 3. The same cluster with the sim moved onto N workers, so the extra copy and the extra
  //    cores can be priced against each other.
  if (env.isolated) {
    out.suites.worlds_sab = await run('worlds sweep, sab (4 sim workers), uncapped',
      `return await window.don.sweep('worlds', ${JSON.stringify(worldsAxis)}, 'uncapped', ${MS}, {units:32, owners:${OWNERS}, path:'sab', shards:4, simHz:0});`);
    out.suites.shards = await run('shard-count sweep at 1024 worlds x 64 units',
      `return await window.don.sweep('shards', ${JSON.stringify(QUICK ? [1, 4] : [1, 2, 4, 8, 12])}, 'uncapped', ${MS}, {worlds:1024, units:32, owners:${OWNERS}, path:'sab', simHz:0});`);
  }

  // 4. Draw-only, to separate GPU cost from sim and upload cost, and to price the
  //    aggregate view against the units view at the same world count.
  const setView = (v) => `document.getElementById('view').value='${v}';
     document.getElementById('view').dispatchEvent(new Event('change'));
     await new Promise(r=>setTimeout(r,250));`;
  out.suites.draw_only = [];
  for (const w of (QUICK ? [1024] : [1024, 4096, 16384])) {
    for (const v of ['units', 'aggregate']) {
      const rows = await run(`draw-only ${w} worlds x 64 units, ${v} view`,
        `await window.don.configure({worlds:${w}, units:32, owners:${OWNERS}, path:'inline', simHz:0});
         ${setView(v)}
         const r = await window.don.bench('draw-only', ${MS}); r.forcedView = '${v}'; return [r];`);
      out.suites.draw_only.push(...rows);
    }
  }
  await cdp.eval(`${setView('auto')} return 1;`);

  // 5. Simulation throughput against worker count.
  //
  //    Frame rate is the wrong axis for this: the renderer displays whatever the shards
  //    have published, so fps barely moves with shard count. What moves is how many
  //    simulation frames the cluster actually advances per second, which is the number the
  //    RL side cares about, and it comes from the shards' own frame counters.
  if (env.isolated) {
    out.suites.sim_rate = [];
    for (const shards of (QUICK ? [1, 4] : [1, 2, 4, 6, 8, 12])) {
      const samples = [];
      for (let i = 0; i < REPEATS; i++) {
        samples.push(await cdp.eval(`
          await window.don.configure({worlds:1024, units:32, owners:${OWNERS}, path:'sab', shards:${shards}, simHz:0});
          await new Promise(r=>setTimeout(r,400));
          return await window.don.simRate(2000);`));
      }
      const row = {
        shards, worlds: samples[0].worlds, units: samples[0].units,
        unitStepsPerSec: median(samples.map((x) => x.unitStepsPerSec)),
        worldStepsPerSec: median(samples.map((x) => x.worldStepsPerSec)),
        simFramesPerSec: median(samples.map((x) => x.simFramesPerSec)),
        samples: samples.map((x) => x.unitStepsPerSec), loadavg: loadavg()[0],
      };
      out.suites.sim_rate.push(row);
      console.error(`   sim-rate shards=${shards}: ${(row.unitStepsPerSec / 1e6).toFixed(1)} M unit-steps/s  (${row.simFramesPerSec.toFixed(0)} sim frames/s per shard)  load=${row.loadavg.toFixed(0)}`);
    }
  }

  // 6. Cross-target determinism. The browser advances an exact frame count and reports the
  //    same `Batch::digest()` the native binary reports. This is the check that can fail.
  out.digest = await cdp.eval(`
    await window.don.configure({worlds:4, owners:2, units:64, capacity:128, path:'inline', seed:0xC0FFEE, simHz:0, autoplay:false});
    await window.don.stepFrames(600);
    return await window.don.digest();`);
  console.error(`-- digest (4 worlds x 2 sides x 64 units, seed 0xC0FFEE, after ${out.digest.frames} frames): ${out.digest.digest}`);
  console.error(`   kills=${out.digest.kills} damage=${out.digest.damage} live=${out.digest.live}`);
  console.error(`   native reference: cd web/wasm && cargo run --release --bin digest -- digest 4 2 64 128 600 0xC0FFEE`);

  // 7. The simulation on its own, with no rendering and no uploads: the honest cost of a
  //    real tick of the derived damage chain in wasm.
  out.suites.sim_only = [];
  for (const [w, u] of (QUICK ? [[1, 512]] : [[1, 128], [1, 512], [1, 2048], [64, 64], [1024, 32]])) {
    const rows = await run(`sim-only ${w} worlds x 2 x ${u} units`,
      `await window.don.configure({worlds:${w}, owners:2, units:${u}, path:'inline', simHz:0});
       await new Promise(r=>setTimeout(r,200));
       return [await window.don.bench('sim-only', ${MS})];`);
    out.suites.sim_only.push(...rows);
  }

  out.machine.loadavgEnd = loadavg();
  out.pageErrors = pageErrors;
  out.pageLog = await cdp.eval('return [...document.querySelectorAll("#log div")].map(d=>d.textContent);').catch(() => []);
  const json = JSON.stringify(out, null, 2);
  if (OUT) { await writeFile(OUT, json); console.error(`wrote ${OUT}`); } else { console.log(json); }

  if (!KEEP) { cdp.close(); proc.kill(); await sleep(300); await rm(profile, { recursive: true, force: true }).catch(() => {}); }
  else console.error(`browser left open on cdp ${CDP}; profile ${profile}`);
}

main().catch((e) => { console.error(e); process.exit(1); });
