// Drive the playable client in a real browser and assert it is actually a game.
//
// A screenshot is not evidence — the spectator lane proved that four different capture
// paths all report "black" whether the renderer drew or not. So this script does three
// things a screenshot cannot:
//
//   1. reads back pixels the *renderer itself* owns (`window.don.snapshot()`), and counts
//      non-black pixels and distinct colours;
//   2. plays a scripted opening through the same code path a human's clicks take — select,
//      gather, build, train, advance an age — and checks the ledger moved;
//   3. measures frames per second over a fixed window at a stated unit count, and reports
//      the browser, backend and viewport it measured on.
//
//     node web/serve.mjs &
//     node web/tools/play-smoke.mjs [--backend canvas2d] [--json out.json] [--keep]
//
// Exit 0 only if the game played, the renderer drew, and nothing threw.

import { spawn } from 'node:child_process';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const args = process.argv.slice(2);
const flag = (n, d = null) => { const i = args.indexOf(n); return i < 0 ? d : (args[i + 1] ?? true); };
const has = (n) => args.includes(n);

const PORT = Number(flag('--port', 8787));
const CDP = Number(flag('--cdp', 9336));
const KEEP = has('--keep');
const OUT = flag('--json', null);
const BACKEND = flag('--backend', null);

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
    // WebGPU in headless needs to be asked for explicitly; without these the client falls
    // back to canvas-2D and says so, which is a valid run but a different measurement.
    '--enable-unsafe-webgpu', '--enable-features=Vulkan,UseSkiaRenderer',
    has('--headed') ? '--window-size=1600,1000' : '--headless=new',
    '--window-size=1600,1000', 'about:blank',
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
    if (r.result?.exceptionDetails) {
      throw new Error(JSON.stringify(r.result.exceptionDetails).slice(0, 500));
    }
    return r.result?.result?.value;
  }
  close() { this.ws.close(); }
}

async function stopChrome(proc) {
  if (proc.exitCode !== null) return;
  const exited = new Promise((resolve) => proc.once('exit', resolve));
  proc.kill('SIGTERM');
  const clean = await Promise.race([exited.then(() => true), sleep(4000).then(() => false)]);
  if (!clean && proc.exitCode === null) { proc.kill('SIGKILL'); await exited; }
}

// ---------------------------------------------------------------------------------------

try { await fetch(`http://127.0.0.1:${PORT}/play.html`); }
catch { console.error(`no server on ${PORT} — run: node web/serve.mjs ${PORT}`); process.exit(3); }

const profile = await mkdtemp(join(tmpdir(), 'don-play-smoke-'));
let proc = null, c = null;
const out = { generated_by: 'web/tools/play-smoke.mjs', when: new Date().toISOString() };
let bad = 0;

try {
  ({ proc, bin: out.chrome } = await launch(profile));
  const targets = await (await fetch(`http://127.0.0.1:${CDP}/json/list`)).json();
  const page = targets.find((t) => t.type === 'page');
  c = await Cdp.open(page.webSocketDebuggerUrl);
  await c.send('Runtime.enable');
  await c.send('Log.enable');
  await c.send('Page.enable');
  out.userAgent = (await (await fetch(`http://127.0.0.1:${CDP}/json/version`)).json())['User-Agent'];

  const url = `http://127.0.0.1:${PORT}/play.html` + (BACKEND ? `?backend=${BACKEND}` : '');
  await c.send('Page.navigate', { url });
  let up = false;
  for (let i = 0; i < 100; i++) {
    await sleep(200);
    up = await c.eval('!!(window.don && window.don.ready && window.don.ready())');
    if (up) break;
    const be = await c.eval('window.don && window.don.bootError');
    if (be) throw new Error('client boot failed:\n' + be);
  }
  if (!up) throw new Error('the client never became ready');

  await sleep(600);
  out.boot = await c.eval('JSON.stringify(window.don.stats())').then(JSON.parse);
  out.viewport = await c.eval('[document.getElementById("gl").width, document.getElementById("gl").height]');
  console.log(`backend ${out.boot.backend}  gamedata ${out.boot.hasGameData}  ` +
    `playdata ${out.boot.hasPlayData}  objects ${out.boot.live}`);
  if (out.boot.backendErrors.length) {
    console.error('renderer errors: ' + out.boot.backendErrors.join(' | '));
    bad++;
  }

  // The recovered playable page must say what it is, show a usable initial catalog, and
  // keep its controls live at boot. These checks caught the cut-off lane's blank palette
  // and the old whole-world "fidelity" label.
  out.ui = await c.eval(`(() => {
    const filter = document.getElementById('palette-filter');
    const pause = document.getElementById('pause');
    const initialCatalog = document.querySelectorAll('#palette button').length;
    filter.value = 'barracks';
    filter.dispatchEvent(new Event('input', { bubbles: true }));
    const filteredCatalog = document.querySelectorAll('#palette button').length;
    filter.value = '';
    filter.dispatchEvent(new Event('input', { bubbles: true }));
    pause.click();
    const pauseLabel = pause.textContent;
    pause.click();
    return JSON.stringify({
      initialCatalog, filteredCatalog, pauseLabel,
      integrationLabel: document.querySelector('.status-note')?.textContent ?? '',
      incomeOptions: [...document.querySelectorAll('#income option')].map(o => o.textContent),
      commandButtons: document.querySelectorAll('#command-dock button').length,
      targetButtonsDisabledWithoutSelection: ['cmd-move', 'cmd-attack', 'cmd-gather']
        .every(id => document.getElementById(id)?.disabled),
      toastLiveRegion: document.getElementById('toast')?.getAttribute('aria-live') ?? '',
    });
  })()`).then(JSON.parse);
  for (const [name, ok] of [
    ['the initial order catalog is populated', out.ui.initialCatalog > 0],
    ['catalog filtering works', out.ui.filteredCatalog === 1],
    ['pause visibly becomes resume', out.ui.pauseLabel.includes('resume')],
    ['the page identifies itself as an integration build', out.ui.integrationLabel.includes('Not Fidelity mode')],
    ['no whole-world fidelity option is advertised', out.ui.incomeOptions.every((x) => !/^fidelity\b/i.test(x))],
    ['the touch command dock is complete', out.ui.commandButtons >= 9],
    ['target commands require a selection', out.ui.targetButtonsDisabledWithoutSelection],
    ['command feedback is announced', out.ui.toastLiveRegion === 'polite'],
  ]) {
    if (!ok) { console.error(`FAIL: ${name}`); bad++; }
  }

  // The narrow layout is a first-class play surface: the stage remains usable, the side
  // panel moves below it, and the command dock stays inside the viewport (scrolling its
  // own buttons when necessary). This is a layout assertion, not a screenshot judgement.
  await c.send('Emulation.setDeviceMetricsOverride', {
    width: 390, height: 844, deviceScaleFactor: 1, mobile: true,
  });
  await sleep(250);
  out.narrow = await c.eval(`(() => {
    const stage = document.getElementById('stage').getBoundingClientRect();
    const side = document.getElementById('side').getBoundingClientRect();
    const dock = document.getElementById('command-dock').getBoundingClientRect();
    const first = document.querySelector('#command-dock button').getBoundingClientRect();
    return {
      viewport: [innerWidth, innerHeight], stage: [stage.width, stage.height],
      sideBelowStage: side.top >= stage.bottom - 1,
      dockInsideViewport: dock.left >= 0 && dock.right <= innerWidth,
      dockScrollable: document.getElementById('command-dock').scrollWidth >= dock.width,
      touchTarget: [first.width, first.height], coverageVisible:
        getComputedStyle(document.getElementById('coverage')).display !== 'none',
    };
  })()`);
  for (const [name, ok] of [
    ['narrow stage remains playable', out.narrow.stage[0] >= 380 && out.narrow.stage[1] >= 360],
    ['narrow panel follows the map', out.narrow.sideBelowStage],
    ['narrow command dock stays in the viewport', out.narrow.dockInsideViewport],
    ['narrow command dock owns its overflow', out.narrow.dockScrollable],
    ['narrow command targets are at least 40 px', out.narrow.touchTarget[0] >= 40 && out.narrow.touchTarget[1] >= 40],
    ['narrow layout keeps fidelity counters visible', out.narrow.coverageVisible],
  ]) {
    if (!ok) { console.error(`FAIL: ${name}`); bad++; }
  }
  await c.send('Emulation.clearDeviceMetricsOverride');
  await sleep(250);

  // ---- 1. does it actually draw? -------------------------------------------------------
  out.readback = await c.eval('window.don.snapshot().then(r => JSON.stringify(r))').then(JSON.parse);
  const px = out.readback.w * out.readback.h;
  out.drewFraction = out.readback.nonBlack / px;
  console.log(`readback ${out.readback.w}x${out.readback.h}: ` +
    `${out.readback.nonBlack} non-black px (${(100 * out.drewFraction).toFixed(1)}%), ` +
    `${out.readback.distinctColours} distinct colours`);
  if (out.drewFraction < 0.5 || out.readback.distinctColours < 8) {
    console.error('FAIL: the renderer did not draw a scene');
    bad++;
  }

  // ---- 2. play a scripted opening ------------------------------------------------------
  // Every step below goes through the same functions the mouse and keyboard call.
  const script = `(async () => {
    const s = window.don.state, m = s.mod;
    const R = { steps: [] };
    const note = (k, v) => R.steps.push([k, v]);

    // select every citizen the way Ctrl+A does
    window.don.key('KeyA');                       // no ctrl -> pans; harmless
    const v = m.views(), n = m.live, ids = [];
    for (let i = 0; i < n; i++) {
      const tag = v.tag[i];
      if ((tag & 0x80000000) === 0 || (tag & 0xf) !== 0 || ((tag >>> 29) & 1)) continue;
      const id = m.idAtRow(i);
      if (id >= 0) ids.push(id);
    }
    window.don.select(ids);
    note('selected', ids.length);

    // find a gatherable tile and send them, one GatherCommand per worker
    let assigned = 0;
    const start = m.startOf(0);
    const stx = Math.floor(start[0] / m.subtile), sty = Math.floor(start[1] / m.subtile);
    outer:
    for (let k = 0; k < ids.length; k++) {
      for (let r = 2; r < 26; r++) {
        for (let dy = -r; dy <= r; dy++) for (let dx = -r; dx <= r; dx++) {
          if (Math.abs(dx) !== r && Math.abs(dy) !== r) continue;
          const tx = stx + dx, ty = sty + dy;
          if (m.tileResource(tx, ty) < 0) continue;
          if (R.used && R.used.has(ty * m.tiles + tx)) continue;
          (R.used = R.used || new Set()).add(ty * m.tiles + tx);
          window.don.select([ids[k]]);
          m.gather(0, ty * m.tiles + tx);
          assigned++;
          continue outer;
        }
      }
    }
    delete R.used;
    note('gatherOrders', assigned);

    const before = m.player(0).stock.slice();
    for (let i = 0; i < 1400; i++) m.step(1);
    const after = m.player(0).stock.slice();
    note('stockBefore', before); note('stockAfter', after);
    note('workers', m.player(0).workers);

    // place a Barracks (427) on the first FULLY_CLEAR anchor near the base
    window.don.select(ids.slice(0, 3));
    let placed = null;
    search:
    for (let r = 3; r < 22; r++) {
      for (let dy = -r; dy <= r; dy++) for (let dx = -r; dx <= r; dx++) {
        const tx = stx + dx, ty = sty + dy;
        if (m.placementGrade(0, 427, tx, ty) === 4) {
          m.build(0, tx, ty, 427); placed = [tx, ty]; break search;
        }
      }
    }
    note('barracksAt', placed);
    for (let i = 0; i < 2500; i++) m.step(1);
    let barracks = -1;
    for (let i = 0; i < m.live; i++) {
      const id = m.idAtRow(i);
      const inf = id >= 0 ? m.info(id) : null;
      if (inf && inf.typeId === 427 && inf.buildProgress < 0) { barracks = id; break; }
    }
    note('barracksBuilt', barracks >= 0);

    // train from it — the menu is the WHERE join, so ask the join what it can make
    let trained = 0;
    if (barracks >= 0) {
      window.don.select([barracks]);
      const prods = m.products(427);
      note('barracksProducts', prods.length);
      m.queueUp(0, prods[0], 2);
      for (let i = 0; i < 1500; i++) m.step(1);
      for (let i = 0; i < m.live; i++) {
        const id = m.idAtRow(i);
        const inf = id >= 0 ? m.info(id) : null;
        if (inf && inf.typeId === prods[0]) trained++;
      }
    }
    note('trained', trained);

    // advance an age with the real age tech
    const ageBefore = m.player(0).age;
    m.queueUp(0, 544, 1);
    for (let i = 0; i < 900; i++) m.step(1);
    note('age', [ageBefore, m.player(0).age]);
    note('gaps', m.gaps());
    note('digest', m.digest());
    return JSON.stringify(R);
  })()`;
  out.script = JSON.parse(await c.eval(script));
  const S = Object.fromEntries(out.script.steps);
  console.log(`scripted opening: ${S.selected} citizens, ${S.gatherOrders} gather orders, ` +
    `workers ${JSON.stringify(S.workers)}`);
  console.log(`  stock ${JSON.stringify(S.stockBefore)} -> ${JSON.stringify(S.stockAfter)}`);
  console.log(`  barracks at ${JSON.stringify(S.barracksAt)} built=${S.barracksBuilt} ` +
    `products=${S.barracksProducts} trained=${S.trained}  age ${JSON.stringify(S.age)}`);
  const stockMoved = S.stockAfter.some((v, i) => v > S.stockBefore[i]);
  for (const [name, ok] of [
    ['a citizen was selected', S.selected > 0],
    ['gather orders were accepted', S.gatherOrders > 0],
    ['the ledger moved', stockMoved],
    ['a building finished', S.barracksBuilt === true],
    ['the producer menu is non-empty', S.barracksProducts > 0],
    ['a unit was trained', S.trained > 0],
    ['the age advanced', S.age[1] > S.age[0]],
  ]) {
    if (!ok) { console.error(`FAIL: ${name}`); bad++; }
  }

  // ---- 3. frames per second, against object count ---------------------------------------
  //
  // Two numbers per row, because one is not enough. `rAF` is what a player actually sees
  // and it saturates at the display's refresh rate, so on its own it cannot tell a client
  // with 10x headroom from one about to drop frames. `uncapped` runs the same step + upload
  // + draw with no rAF in the way and reports the headroom.
  out.fps = [];
  const counts = [0, 512, 1024, 2048, 4000];
  for (const want of counts) {
    if (want > 0) {
      const s = await c.eval(`JSON.stringify(window.don.stress(${want}))`).then(JSON.parse);
      if (s.made === 0) continue;
    }
    await c.eval('window.don.state.paused = false');
    await sleep(1800);
    const raf = await c.eval('JSON.stringify(window.don.stats())').then(JSON.parse);
    await c.eval('window.don.state.paused = true');
    const unc = await c.eval('window.don.bench(1500).then(JSON.stringify)').then(JSON.parse);
    out.fps.push({ target: want, raf, uncapped: unc });
    console.log(`  ${String(unc.live).padStart(5)} objects   ` +
      `rAF ${raf.fps.toFixed(1).padStart(6)} fps   uncapped ${unc.fps.toFixed(1).padStart(7)} fps   ` +
      `step ${unc.stepMs.toFixed(3)}  upload ${unc.uploadMs.toFixed(3)}  ` +
      `draw ${unc.drawMs.toFixed(3)} ms`);
  }
  await c.eval('window.don.state.paused = false');
  const worst = out.fps[out.fps.length - 1];
  if (worst && worst.raf.fps < 55) {
    console.error(`FAIL: only ${worst.raf.fps.toFixed(1)} rAF fps at ${worst.uncapped.live} objects`);
    bad++;
  }
  console.log(`viewport ${out.viewport.join('x')}  backend ${out.boot.backend}`);

  // ---- 4. an unexecutable order must be visible, not swallowed --------------------------
  out.gapProbe = await c.eval(`(() => {
    const m = window.don.state.mod;
    const before = m.gaps().slice();
    window.don.select([]);                 // nothing selected
    m.moveTo(0, 1000, 1000);               // ... then order a move
    m.gather(0, 0);                        // ... and gather an ungatherable tile
    for (let i = 0; i < 3; i++) m.step(1);
    return JSON.stringify({ before, after: m.gaps() });
  })()`).then(JSON.parse);
  const grew = out.gapProbe.after.some((v, i) => v > out.gapProbe.before[i]);
  console.log(`unexecutable orders counted: ${grew ? 'yes' : 'NO'} ` +
    `${JSON.stringify(out.gapProbe.after)}`);
  if (!grew) { console.error('FAIL: an unexecutable order was swallowed silently'); bad++; }

  // ---- 4b. real mouse and keyboard, dispatched by the browser --------------------------
  //
  // Everything above drives `window.don`. This drives the *DOM*: synthetic pointer and key
  // events at real screen coordinates, so the camera, the hit test and the event wiring are
  // all in the loop. A client that only works when a script calls its functions is not a
  // client.
  const target = await c.eval(`(() => {
    const m = window.don.state.mod, v = m.views();
    for (let i = 0; i < m.live; i++) {
      const tag = v.tag[i];
      if ((tag & 0x80000000) === 0 || (tag & 0xf) !== 0 || ((tag >>> 29) & 1)) continue;
      window.don.centreOn(v.x[i], v.y[i]);
      const s = window.don.worldToScreen(v.x[i], v.y[i]);
      const r = document.getElementById('gl').getBoundingClientRect();
      return JSON.stringify({ id: m.pickAt(v.x[i], v.y[i]), sx: r.left + s[0], sy: r.top + s[1] });
    }
    return null;
  })()`).then((s) => (s ? JSON.parse(s) : null));
  if (!target) { console.error('FAIL: no unit to click'); bad++; }
  else {
    await c.eval('window.don.select([])');
    for (const type of ['mousePressed', 'mouseReleased']) {
      await c.send('Input.dispatchMouseEvent', {
        type, x: Math.round(target.sx), y: Math.round(target.sy),
        button: 'left', buttons: type === 'mousePressed' ? 1 : 0, clickCount: 1,
      });
    }
    await sleep(120);
    const selN = await c.eval('window.don.state.selection.length');
    // right click 6 tiles away -> a MoveToCommand the unit must accept
    for (const type of ['mousePressed', 'mouseReleased']) {
      await c.send('Input.dispatchMouseEvent', {
        type, x: Math.round(target.sx) + 90, y: Math.round(target.sy) + 60,
        button: 'right', buttons: type === 'mousePressed' ? 2 : 0, clickCount: 1,
      });
    }
    await c.eval('for (let i=0;i<3;i++) window.don.state.mod.step(1)');
    const ordered = await c.eval(`(() => {
      const i = window.don.info(${target.id});
      return i ? i.order : -1;
    })()`);
    // The visible dock must arm the same map input and disarm after a target. This catches
    // a decorative mobile toolbar that looks useful but never reaches the packet path.
    const dockArmed = await c.eval(`(() => {
      document.getElementById('cmd-move').click();
      const b = document.getElementById('cmd-move');
      return window.don.state.commandMode === 'move' && b.getAttribute('aria-pressed') === 'true';
    })()`);
    for (const type of ['mousePressed', 'mouseReleased']) {
      await c.send('Input.dispatchMouseEvent', {
        type, x: Math.round(target.sx) + 120, y: Math.round(target.sy) + 80,
        button: 'left', buttons: type === 'mousePressed' ? 1 : 0, clickCount: 1,
      });
    }
    await c.eval('for (let i=0;i<3;i++) window.don.state.mod.step(1)');
    const dockResult = await c.eval(`(() => {
      const i = window.don.info(${target.id});
      return { modeCleared: window.don.state.commandMode === null, order: i ? i.order : -1 };
    })()`);
    out.mouse = {
      clickedId: target.id, selectedAfterClick: selN, orderAfterRightClick: ordered,
      dockArmed, dockModeCleared: dockResult.modeCleared, orderAfterDockMove: dockResult.order,
    };
    console.log(`real mouse: click selected ${selN}, right click set order ${ordered} ` +
      `(1 = MOVE_TO)`);
    if (selN !== 1) { console.error('FAIL: a real click did not select the unit'); bad++; }
    if (ordered !== 1 && ordered !== 3) {
      console.error('FAIL: a real right click produced no order'); bad++;
    }
    if (!dockArmed || !dockResult.modeCleared || (dockResult.order !== 1 && dockResult.order !== 3)) {
      console.error('FAIL: command dock did not arm, issue, and clear a move target'); bad++;
    }
  }

  // ---- 5. cross-target determinism -----------------------------------------------------
  out.freshDigest = await c.eval(
    'JSON.stringify(window.don.freshDigest(0xc0ffee, 600))').then(JSON.parse);
  console.log(`wasm fresh game: seed 0xc0ffee 600 frames -> ` +
    `${out.freshDigest.live} live, digest ${out.freshDigest.digest}`);
  console.log('  compare with: cd web/wasm && cargo run --release --bin playcheck -- ' +
    'digest ../public/data/gamedata.bin ../public/data/playdata.bin c0ffee 600');
  const expect = flag('--expect-digest', null);
  if (expect) {
    out.expectDigest = expect;
    if (expect !== out.freshDigest.digest) {
      console.error(`FAIL: wasm digest ${out.freshDigest.digest} != native ${expect}`);
      bad++;
    } else {
      console.log(`  wasm32 and native agree bit for bit: ${expect}`);
    }
  }

  out.consoleErrors = c.events
    .filter((e) => e.method === 'Log.entryAdded' && e.params.entry.level === 'error')
    .map((e) => e.params.entry.text)
    .concat(c.events.filter((e) => e.method === 'Runtime.exceptionThrown')
      .map((e) => e.params.exceptionDetails?.exception?.description ?? 'exception'));
  if (out.consoleErrors.length) {
    console.error('page errors: ' + out.consoleErrors.slice(0, 3).join(' | '));
    bad++;
  }
} catch (e) {
  console.error('smoke failed: ' + e.message);
  out.error = e.message;
  bad++;
} finally {
  c?.close();
  if (!KEEP && proc) await stopChrome(proc);
  if (!KEEP) await rm(profile, { recursive: true, force: true });
}

if (OUT) await writeFile(OUT, JSON.stringify(out, null, 1));
console.log(bad ? `\n${bad} check(s) failed` : '\nall checks passed');
process.exitCode = bad ? 1 : 0;
