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

  // Ask for unsupported setup values on purpose. The client must canonicalize those to
  // unavailable while honoring the two rule parameters the current WASM ABI really accepts.
  const requested = new URLSearchParams({
    map: 'requested-ocean', nation: 'requested-nation', team: '2',
    ai_slots: '3', ai_difficulty: 'hard', victory: 'conquest',
    income: 'uncapped-experiment', population: '150',
  });
  if (BACKEND) requested.set('backend', BACKEND);
  const url = `http://127.0.0.1:${PORT}/play.html?${requested}`;
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
    const initialCatalogDisabled = [...document.querySelectorAll('#palette button')]
      .every(button => button.disabled);
    const m = window.don.state.mod, views = m.views();
    let mobile = -1;
    for (let row = 0; row < m.live; row++) {
      const tag = views.tag[row];
      if ((tag & 0x80000000) && (tag & 0xf) === 0 && !((tag >>> 29) & 1)) {
        mobile = m.idAtRow(row); break;
      }
    }
    window.don.select([mobile]);
    filter.value = 'barracks';
    filter.dispatchEvent(new Event('input', { bubbles: true }));
    const filteredCatalog = document.querySelectorAll('#palette button').length;
    const buildButtonEnabled = !document.querySelector('#palette button')?.disabled;
    filter.value = '';
    filter.dispatchEvent(new Event('input', { bubbles: true }));
    const futureBuildDisabled = [...document.querySelectorAll('#palette button')]
      .some(button => button.disabled && button.querySelector('.why')?.textContent.includes('requires age'));
    window.don.key('KeyT', { shiftKey: true });
    const keyboardTrain = document.getElementById('tab-train').classList.contains('sel');
    window.don.key('KeyR', { shiftKey: true });
    const keyboardResearch = document.getElementById('tab-research').classList.contains('sel');
    const researchHasEnabledAge = !!document.querySelector('#palette [data-kind="research"]:not(:disabled)');
    const unavailableResearchDisabled = !!document.querySelector('#palette [data-kind="unavailable"]:disabled');
    document.getElementById('tab-build').click();
    window.don.select([]);
    pause.click();
    const pauseLabel = pause.textContent;
    pause.click();
    const replayPlay = document.getElementById('replay-play');
    replayPlay.click();
    const replayPauseWorked = window.don.state.paused && replayPlay.textContent.includes('resume');
    replayPlay.click();
    const replaySpeed = document.getElementById('replay-speed');
    replaySpeed.value = '2';
    replaySpeed.dispatchEvent(new Event('change', { bubbles: true }));
    const replaySpeedWorked = window.don.state.speed === 2 && document.getElementById('speed').value === '2';
    replaySpeed.value = '1';
    replaySpeed.dispatchEvent(new Event('change', { bubbles: true }));
    return JSON.stringify({
      initialCatalog, initialCatalogDisabled, filteredCatalog, buildButtonEnabled,
      futureBuildDisabled, keyboardTrain, keyboardResearch, researchHasEnabledAge,
      unavailableResearchDisabled, orderTabs: document.querySelectorAll('.tabs .tab').length,
      pauseLabel,
      integrationLabel: document.querySelector('.status-note')?.textContent ?? '',
      runtimeLabel: document.getElementById('readiness-runtime')?.textContent ?? '',
      gateLabel: document.getElementById('readiness-gate')?.textContent ?? '',
      replayLabel: document.getElementById('readiness-replay')?.textContent ?? '',
      blockerItems: document.querySelectorAll('#readiness-blockers li').length,
      incomeOptions: [...document.querySelectorAll('#income option')].map(o => o.textContent),
      commandButtons: document.querySelectorAll('#command-dock button').length,
      sessionSeed: document.getElementById('session-seed')?.value ?? '',
      sessionPlayers: document.querySelectorAll('#session-player option').length,
      sessionStatus: document.getElementById('session-status')?.textContent ?? '',
      sessionStatusLive: document.getElementById('session-status')?.getAttribute('aria-live') ?? '',
      sessionShare: !!document.getElementById('session-share'),
      sessionSetup: window.don.session.setup(),
      sessionUrl: window.don.session.url(),
      sessionUnavailableDisabled: [
        'session-map', 'session-size', 'session-nation', 'session-team',
        'session-ai-slots', 'session-ai-difficulty', 'session-victory',
      ].every(id => document.getElementById(id)?.disabled),
      sessionSummary: document.getElementById('session-summary')?.textContent ?? '',
      objectiveTime: document.getElementById('objective-time')?.textContent ?? '',
      objectiveState: document.getElementById('objective-state')?.textContent ?? '',
      objectiveScore: document.getElementById('objective-score')?.textContent ?? '',
      objectiveCountdown: document.getElementById('objective-countdown')?.textContent ?? '',
      objectiveFactsDisabled: ['objective-state', 'objective-score', 'objective-countdown']
        .every(id => document.getElementById(id)?.getAttribute('aria-disabled') === 'true'),
      visibilityHonesty: document.getElementById('visibility-honesty')?.textContent ?? '',
      ownerLegend: document.querySelectorAll('#owner-legend .owner-key').length,
      ownerRows: document.querySelectorAll('#world-players .world-player').length,
      ownerStates: [...document.querySelectorAll('#world-players .owner-state')]
        .map(node => node.textContent),
      minimapLabel: document.getElementById('mini')?.getAttribute('aria-label') ?? '',
      replayProtocol: window.don.replay.snapshot().protocol,
      replayStatus: document.getElementById('replay-status')?.textContent ?? '',
      replayStatusLive: document.getElementById('replay-status')?.getAttribute('aria-live') ?? '',
      replayControls: document.querySelectorAll('#replay .replay-controls button').length,
      replayTimeline: document.getElementById('replay-timeline')?.getAttribute('aria-label') ?? '',
      replayJournalActionsEnabled: ['replay-export', 'replay-import']
        .every(id => !document.getElementById(id)?.disabled),
      replayUnavailableDisabled: [...document.querySelectorAll('#replay-capabilities button')]
        .every(button => button.disabled),
      replayCapabilities: document.getElementById('replay-capabilities')?.textContent ?? '',
      replayPauseWorked, replaySpeedWorked,
      controlGroupSlots: document.querySelectorAll('#group-slots .group-slot').length,
      controlGroupActions: document.querySelectorAll('#group-actions button').length,
      controlGroupLive: document.getElementById('group-status')?.getAttribute('aria-live') ?? '',
      controlGroupBoundary: document.getElementById('control-groups')?.textContent ?? '',
      commandHistoryRecords: document.querySelectorAll('#command-history-list .command-record').length,
      commandHistorySummary: document.getElementById('command-history-summary')?.textContent ?? '',
      commandFeedbackBoundary: document.getElementById('command-history')?.textContent ?? '',
      targetButtonsDisabledWithoutSelection: ['cmd-move', 'cmd-attack', 'cmd-gather']
        .every(id => document.getElementById(id)?.disabled),
      toastLiveRegion: document.getElementById('toast')?.getAttribute('aria-live') ?? '',
    });
  })()`).then(JSON.parse);
  for (const [name, ok] of [
    ['the initial order catalog is populated', out.ui.initialCatalog > 0],
    ['build commands require a selection', out.ui.initialCatalogDisabled],
    ['catalog filtering works', out.ui.filteredCatalog === 1],
    ['a selected non-building object unlocks a known-age affordable building', out.ui.buildButtonEnabled],
    ['known future-age prerequisites disable building actions', out.ui.futureBuildDisabled],
    ['keyboard shortcuts open train and research modes', out.ui.keyboardTrain && out.ui.keyboardResearch],
    ['research offers the implemented age action and disables unavailable technologies',
      out.ui.researchHasEnabledAge && out.ui.unavailableResearchDisabled && out.ui.orderTabs === 3],
    ['pause visibly becomes resume', out.ui.pauseLabel.includes('resume')],
    ['the page identifies itself as an integration build', out.ui.integrationLabel.includes('Not Fidelity mode')],
    ['runtime identity says the web GameWorld is not Arena',
      out.ui.runtimeLabel.includes('web::wasm::game::GameWorld') && out.ui.runtimeLabel.includes('connected: no')],
    ['the compiled playable gate is visibly blocked', out.ui.gateLabel.includes('BLOCKED')],
    ['the replay card reports non-empty walks and zero matches',
      out.ui.replayLabel.includes('world walks non-empty') && out.ui.replayLabel.includes('0 matches')],
    ['the local boundary and compiled blockers are inspectable', out.ui.blockerItems > 1],
    ['no whole-world fidelity option is advertised', out.ui.incomeOptions.every((x) => !/^fidelity\b/i.test(x))],
    ['the touch command dock is complete', out.ui.commandButtons >= 9],
    ['session setup exposes the deterministic seed', out.ui.sessionSeed === '0x00c0ffee'],
    ['session setup exposes every player perspective', out.ui.sessionPlayers >= 2],
    ['session identity and seed boundary are visible', out.ui.sessionStatus.includes('0x00c0ffee') &&
      out.ui.sessionStatus.includes('player 0') && out.ui.sessionStatus.includes('seed not yet consumed')],
    ['session changes are announced', out.ui.sessionStatusLive === 'polite'],
    ['session links are shareable', out.ui.sessionShare],
    ['unsupported setup choices stay disabled', out.ui.sessionUnavailableDisabled],
    ['unsupported URL requests are canonicalized rather than fabricated',
      out.ui.sessionSetup.map === 'integration-land' && out.ui.sessionSetup.size === '128x128' &&
      out.ui.sessionSetup.nation === 'unavailable' && out.ui.sessionSetup.team === 'unavailable' &&
      out.ui.sessionSetup.aiSlots === 'unavailable' && out.ui.sessionSetup.aiDifficulty === 'unavailable' &&
      out.ui.sessionSetup.victory === 'unavailable'],
    ['the two accepted rules are restored from the URL',
      out.ui.sessionSetup.income === 'uncapped-experiment' && out.ui.sessionSetup.population === 150 &&
      out.boot.player.popCap === 150],
    ['the pregame summary exposes world, slots, rules, and unavailable systems',
      out.ui.sessionSummary.includes('128 × 128') && out.ui.sessionSummary.includes('manual') &&
      out.ui.sessionSummary.includes('population 150') && out.ui.sessionSummary.includes('victory unavailable')],
    ['the objective panel reports real elapsed time but disables absent endgame facts',
      out.ui.objectiveTime.includes('frame') && out.ui.objectiveFactsDisabled &&
      out.ui.objectiveState.includes('unavailable') && out.ui.objectiveScore.includes('not a victory score') &&
      out.ui.objectiveCountdown.includes('elapsed time only')],
    ['the objective panel labels omniscient visibility and unknown relations honestly',
      out.ui.visibilityHonesty.includes('Omniscient integration view') &&
      out.ui.visibilityHonesty.includes('not exported') &&
      out.ui.ownerStates.slice(1).every(text => text.includes('relation unavailable'))],
    ['the real minimap exposes every owner without inventing teams',
      out.ui.ownerLegend === 4 && out.ui.ownerRows === 4 &&
      out.ui.minimapLabel.includes('All exported owners are visible')],
    ['the command journal exposes playback, timeline, import, and export controls',
      out.ui.replayProtocol === 'don.command-journal.v1' && out.ui.replayControls >= 5 &&
      out.ui.replayTimeline.includes('Command journal frame') && out.ui.replayJournalActionsEnabled &&
      out.ui.replayPauseWorked && out.ui.replaySpeedWorked],
    ['journal feedback is announced and does not claim to be a native save',
      out.ui.replayStatusLive === 'polite' && out.ui.replayStatus.includes('not a native save-state')],
    ['unsupported native save/load and retail replay actions stay disabled with reasons',
      out.ui.replayUnavailableDisabled && out.ui.replayCapabilities.includes('no state serializer') &&
      out.ui.replayCapabilities.includes('no state deserializer') &&
      out.ui.replayCapabilities.includes('no retail replay playback bridge')],
    ['nine touch control-group slots expose replace, add, remove, clear, and announced feedback',
      out.ui.controlGroupSlots === 9 && out.ui.controlGroupActions === 4 &&
      out.ui.controlGroupLive === 'polite'],
    ['control groups disclose browser-local IDs and unavailable native persistence',
      out.ui.controlGroupBoundary.includes('exported object IDs') &&
      out.ui.controlGroupBoundary.includes('not exported') &&
      out.ui.controlGroupBoundary.includes('stale IDs are pruned')],
    ['issued packet feedback is visible before the next tick',
      out.ui.commandHistoryRecords > 0 && out.ui.commandHistorySummary.includes('pending') &&
      out.ui.commandFeedbackBoundary.includes('ABI cannot') &&
      out.ui.commandFeedbackBoundary.includes('batch is labelled')],
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
    const tab = document.getElementById('tab-research').getBoundingClientRect();
    const palette = document.querySelector('#palette button').getBoundingClientRect();
    const objectives = document.getElementById('objectives').getBoundingClientRect();
    const focus = document.querySelector('#world-players button').getBoundingClientRect();
    const replay = document.getElementById('replay').getBoundingClientRect();
    const replayControls = [...document.querySelectorAll('#replay .replay-controls button')]
      .map(button => button.getBoundingClientRect().height);
    const groups = document.getElementById('control-groups').getBoundingClientRect();
    const groupControls = [...document.querySelectorAll('#group-slots button, #group-actions button')]
      .map(button => button.getBoundingClientRect().height);
    const commands = document.getElementById('command-history').getBoundingClientRect();
    return {
      viewport: [innerWidth, innerHeight], stage: [stage.width, stage.height],
      sideBelowStage: side.top >= stage.bottom - 1,
      dockInsideViewport: dock.left >= 0 && dock.right <= innerWidth,
      dockScrollable: document.getElementById('command-dock').scrollWidth >= dock.width,
      touchTarget: [first.width, first.height], paletteTouchTargets: [tab.height, palette.height],
      objectivePanel: [objectives.left, objectives.right], focusTouchHeight: focus.height,
      replayPanel: [replay.left, replay.right], replayControlHeights: replayControls, coverageVisible:
        getComputedStyle(document.getElementById('coverage')).display !== 'none',
      groupPanel: [groups.left, groups.right], groupControlHeights: groupControls,
      commandPanel: [commands.left, commands.right],
    };
  })()`);
  for (const [name, ok] of [
    ['narrow stage remains playable', out.narrow.stage[0] >= 380 && out.narrow.stage[1] >= 360],
    ['narrow panel follows the map', out.narrow.sideBelowStage],
    ['narrow command dock stays in the viewport', out.narrow.dockInsideViewport],
    ['narrow command dock owns its overflow', out.narrow.dockScrollable],
    ['narrow command targets are at least 40 px', out.narrow.touchTarget[0] >= 40 && out.narrow.touchTarget[1] >= 40],
    ['narrow palette tabs and actions are touch-sized', out.narrow.paletteTouchTargets.every(x => x >= 40)],
    ['narrow objectives stay in the page and camera buttons are touch-sized',
      out.narrow.objectivePanel[0] >= 0 && out.narrow.objectivePanel[1] <= out.narrow.viewport[0] &&
      out.narrow.focusTouchHeight >= 40],
    ['narrow replay controls stay in the page and remain touch-sized',
      out.narrow.replayPanel[0] >= 0 && out.narrow.replayPanel[1] <= out.narrow.viewport[0] &&
      out.narrow.replayControlHeights.every(height => height >= 40)],
    ['narrow group and command feedback panels stay in-page with touch-sized controls',
      out.narrow.groupPanel[0] >= 0 && out.narrow.groupPanel[1] <= out.narrow.viewport[0] &&
      out.narrow.commandPanel[0] >= 0 && out.narrow.commandPanel[1] <= out.narrow.viewport[0] &&
      out.narrow.groupControlHeights.every(height => height >= 40)],
    ['narrow layout keeps fidelity counters visible', out.narrow.coverageVisible],
  ]) {
    if (!ok) { console.error(`FAIL: ${name}`); bad++; }
  }
  await c.send('Emulation.clearDeviceMetricsOverride');
  await sleep(250);

  // A playable page needs a reproducible session boundary, not a hard-coded seed that can
  // only be reset by throwing the tab away. Restart the real Wasm world, reject a malformed
  // seed without losing it, exercise player perspective, and leave player zero selected for
  // the gameplay script below.
  out.session = await c.eval(`(() => {
    const d = window.don, m = d.state.mod;
    m.step(9);
    const dirtyFrame = m.frame;
    const oldDigest = d.state.sessionInitialDigest;
    const income = document.getElementById('income');
    income.value = '0';
    income.dispatchEvent(new Event('change', { bubbles: true }));
    const population = document.getElementById('popset');
    population.value = '2';
    population.dispatchEvent(new Event('change', { bubbles: true }));
    const configured = d.session.setup();
    const restarted = d.session.restart('0x1234abcd');
    const after = d.stats();
    m.step(1);
    const afterRuleTick = d.stats();
    const url = new URL(d.session.url());
    const input = document.getElementById('session-seed');
    input.value = 'not-a-seed';
    document.getElementById('session-new').click();
    const invalidPreserved = d.state.sessionSeed === 0x1234abcd && input.validationMessage.length > 0;
    input.setCustomValidity('');
    input.value = '0x1234abcd';
    const switched = d.session.player(1);
    const returned = d.session.player(0);
    return JSON.stringify({
      dirtyFrame, restarted, oldDigest, newDigest: d.state.sessionInitialDigest,
      frameAfterRestart: after.frame, seedAfterRestart: after.sessionSeed,
      packsAfterRestart: [after.hasGameData, after.hasPlayData],
      stockAfterRestart: after.player.stock,
      popCapAfterRuleTick: afterRuleTick.player.popCap,
      configured,
      urlSeed: url.searchParams.get('seed'), urlPlayer: url.searchParams.get('player'),
      urlMap: url.searchParams.get('map'), urlSize: url.searchParams.get('size'),
      urlNation: url.searchParams.get('nation'), urlTeam: url.searchParams.get('team'),
      urlSlots: url.searchParams.get('slots'), urlAiSlots: url.searchParams.get('ai_slots'),
      urlAiDifficulty: url.searchParams.get('ai_difficulty'),
      urlIncome: url.searchParams.get('income'), urlPopulation: url.searchParams.get('population'),
      urlVictory: url.searchParams.get('victory'),
      invalidPreserved, switched, returned,
      status: document.getElementById('session-status').textContent,
    });
  })()`).then(JSON.parse);
  for (const [name, ok] of [
    ['the pre-restart world advanced', out.session.dirtyFrame >= 9],
    ['new game replaced the Wasm world', out.session.restarted && out.session.frameAfterRestart === 0],
    ['the selected seed reached session state', out.session.seedAfterRestart === 0x1234abcd],
    ['restart retains the packed data tables', out.session.packsAfterRestart.every(Boolean)],
    ['restart exposes the initial player ledger without advancing',
      JSON.stringify(out.session.stockAfterRestart) === JSON.stringify([200, 200, 100, 100, 100, 100])],
    ['supported setup rules are applied to the replacement world',
      out.session.configured.income === 'retail-cap' && out.session.configured.population === 75 &&
      out.session.popCapAfterRuleTick === 75],
    ['the current seed-invariant initializer is exposed honestly',
      out.session.newDigest === out.session.oldDigest && out.session.status.includes('seed not yet consumed')],
    ['the share URL carries the canonical seed and player',
      out.session.urlSeed === '0x1234abcd' && out.session.urlPlayer === '0'],
    ['the share URL records the fixed world and missing player systems',
      out.session.urlMap === 'integration-land' && out.session.urlSize === '128x128' &&
      out.session.urlNation === 'unavailable' && out.session.urlTeam === 'unavailable' &&
      out.session.urlSlots === '4-manual' && out.session.urlAiSlots === 'unavailable' &&
      out.session.urlAiDifficulty === 'unavailable'],
    ['the share URL records only the live rule values and the absent victory host',
      out.session.urlIncome === 'retail-cap' && out.session.urlPopulation === '75' &&
      out.session.urlVictory === 'unavailable'],
    ['a malformed seed preserves the live session', out.session.invalidPreserved],
    ['player perspective switches and returns', out.session.switched === 1 && out.session.returned === 0],
    ['session status returns to player zero', out.session.status.includes('player 0')],
  ]) {
    if (!ok) { console.error(`FAIL: ${name}`); bad++; }
  }

  // The current ABI cannot serialize the native world, but it can restart deterministically
  // and accept exact command packets. Prove that the bounded command journal reconstructs
  // an intermediate tick bit-for-bit, survives export/import, rejects malformed input
  // without touching the live world, seeks, and resumes beyond its recorded head.
  out.journal = await c.eval(`(async () => {
    const d = window.don;
    d.session.restart('0x1234abcd');
    d.replay.pause();
    const m = d.state.mod, views = m.views();
    let mobile = -1;
    for (let row = 0; row < m.live; row++) {
      const tag = views.tag[row];
      if ((tag & 0x80000000) && (tag & 0xf) === 0 && !((tag >>> 29) & 1)) {
        mobile = m.idAtRow(row); break;
      }
    }
    const before = m.info(mobile);
    d.select([mobile]);
    m.moveTo(0, before.x + m.subtile * 5, before.y);
    for (let i = 0; i < 2; i++) d.replay.step();
    const population = document.getElementById('popset');
    population.value = '3';
    population.dispatchEvent(new Event('change', { bubbles: true }));
    for (let i = 0; i < 2; i++) d.replay.step();
    document.getElementById('replay-step').click();
    const recorded = d.replay.snapshot();
    const digestAtExport = m.digest();
    const journal = d.replay.export();
    const parsed = JSON.parse(journal);
    for (let i = 0; i < 7; i++) d.replay.step();
    const advanced = d.replay.snapshot();
    const imported = await d.replay.import(journal);
    const digestAfterImport = m.digest();
    const stableBeforeMalformed = { frame: m.frame, digest: m.digest() };
    let malformedRefused = false;
    try { await d.replay.import('{"protocol":"not-don"}'); }
    catch { malformedRefused = true; }
    const stableAfterMalformed = { frame: m.frame, digest: m.digest() };
    const wrongDigest = JSON.parse(journal);
    wrongDigest.setup.initialDigest = '0000000000000000';
    let wrongDigestRefused = false;
    try { await d.replay.import(JSON.stringify(wrongDigest)); }
    catch { wrongDigestRefused = true; }
    const stableAfterWrongDigest = { frame: m.frame, digest: m.digest() };
    const zero = await d.replay.seek(0);
    const sought = await d.replay.seek(parsed.headFrame);
    document.getElementById('replay-step').click();
    const resumed = d.replay.snapshot();
    const status = document.getElementById('replay-status').textContent;
    const time = document.getElementById('replay-time').textContent;
    population.value = '2';
    population.dispatchEvent(new Event('change', { bubbles: true }));
    d.session.restart('0x1234abcd');
    d.replay.play();
    return JSON.stringify({
      mobile, recorded, advanced, imported, digestAtExport, digestAfterImport,
      malformedRefused, wrongDigestRefused, stableBeforeMalformed, stableAfterMalformed,
      stableAfterWrongDigest,
      zero, sought, resumed, status, time,
      protocol: parsed.protocol, boundary: parsed.boundary,
      setup: parsed.setup, eventKinds: parsed.events.map(event => event.kind),
      commandHex: parsed.events.find(event => event.kind === 'command')?.hex ?? '',
      headFrame: parsed.headFrame, targetFrame: parsed.frame,
    });
  })()`).then(JSON.parse);
  for (const [name, ok] of [
    ['the journal exports its bounded protocol and deterministic session baseline',
      out.journal.protocol === 'don.command-journal.v1' &&
      out.journal.boundary.includes('not a native save') &&
      out.journal.setup.seed === '0x1234abcd' && out.journal.setup.initialDigest.length === 16],
    ['the journal records exact wire packets with frame and selection context',
      out.journal.mobile >= 0 && out.journal.recorded.events >= 2 &&
      out.journal.eventKinds.filter(kind => kind === 'command').length >= 2 &&
      out.journal.eventKinds.includes('population') &&
      /^[0-9a-f]+$/.test(out.journal.commandHex)],
    ['export and import restore the recorded frame and digest bit-for-bit',
      out.journal.targetFrame === out.journal.recorded.frame &&
      out.journal.imported.frame === out.journal.recorded.frame &&
      out.journal.digestAfterImport === out.journal.digestAtExport],
    ['the live world can advance beyond an exported snapshot before restoration',
      out.journal.advanced.frame > out.journal.recorded.frame &&
      out.journal.advanced.digest !== out.journal.digestAtExport],
    ['a malformed journal is fail-closed without mutating the restored world',
      out.journal.malformedRefused && out.journal.wrongDigestRefused &&
      JSON.stringify(out.journal.stableAfterMalformed) === JSON.stringify(out.journal.stableBeforeMalformed) &&
      JSON.stringify(out.journal.stableAfterWrongDigest) === JSON.stringify(out.journal.stableBeforeMalformed)],
    ['timeline seek reconstructs frame zero and the exact exported head',
      out.journal.zero.frame === 0 && out.journal.sought.frame === out.journal.headFrame &&
      out.journal.sought.digest === out.journal.digestAtExport],
    ['step from a restored head resumes simulation and returns to recording',
      out.journal.resumed.frame === out.journal.headFrame + 1 && !out.journal.resumed.playback &&
      out.journal.status.includes('recording resumed') && out.journal.time.includes('head')],
  ]) {
    if (!ok) { console.error(`FAIL: ${name}`); bad++; }
  }

  // Control-group membership is browser state over exact exported object IDs; recall is
  // still an engine GroupCommand. Exercise replace/add/remove with keyboard, the identical
  // touch toolbar, invalid/foreign-ID filtering, and packet lifecycle feedback from issued
  // through pending, applied, and an exactly-attributable refusal.
  out.controlGroups = await c.eval(`(() => {
    const d = window.don;
    d.session.restart('0x1234abcd');
    d.replay.pause();
    const m = d.state.mod, views = m.views();
    const mine = [], foreign = [];
    for (let row = 0; row < m.live; row++) {
      const tag = views.tag[row];
      if (!(tag & 0x80000000) || ((tag >>> 29) & 1)) continue;
      const id = m.idAtRow(row), owner = tag & 0xf;
      if (owner === 0 && mine.length < 3) mine.push(id);
      if (owner === 1 && foreign.length < 1) foreign.push(id);
    }
    d.select(mine.slice(0, 2));
    d.key('Digit1', { ctrlKey: true });
    d.select([mine[2]]);
    d.key('Digit1', { ctrlKey: true, shiftKey: true });
    d.select([mine[1]]);
    d.key('Digit1', { altKey: true });
    d.key('Digit1');
    const keyboard = d.controlGroups.snapshot();

    document.querySelector('.group-slot[data-group="2"]').click();
    d.select([mine[0]]);
    document.getElementById('group-set').click();
    d.select([mine[2]]);
    document.getElementById('group-add').click();
    d.select([mine[0]]);
    document.getElementById('group-remove').click();
    document.querySelector('.group-slot[data-group="2"]').click();
    const touch = d.controlGroups.snapshot();

    d.controlGroups.replace(3, [mine[0], foreign[0], 0x7fffffff]);
    const filtered = d.controlGroups.snapshot();
    document.querySelector('.group-slot[data-group="2"]').click();
    document.getElementById('group-clear').click();
    const cleared = d.controlGroups.snapshot();
    const pendingGroups = d.commands.snapshot();
    d.replay.step();
    const appliedGroups = d.commands.snapshot();

    d.controlGroups.recall(1);
    const unit = m.info(mine[0]);
    m.moveTo(0, unit.x + m.subtile * 4, unit.y);
    const pendingMove = d.commands.snapshot();
    d.replay.step();
    const appliedMove = d.commands.snapshot();

    d.select([]);
    m.moveTo(0, unit.x, unit.y);
    const pendingRefusal = d.commands.snapshot();
    d.replay.step();
    const refused = d.commands.snapshot();
    const dom = {
      summary: document.getElementById('command-history-summary').textContent,
      refusedRows: document.querySelectorAll('#command-history-list [data-status="refused"]').length,
      appliedRows: document.querySelectorAll('#command-history-list [data-status="applied"]').length,
      groupStatus: document.getElementById('group-status').textContent,
      activeSlot: document.querySelector('#group-slots .active')?.dataset.group ?? '',
    };
    d.session.restart('0x1234abcd');
    d.replay.play();
    return JSON.stringify({
      mine, foreign, keyboard, touch, filtered, cleared,
      pendingGroups, appliedGroups, pendingMove, appliedMove, pendingRefusal, refused, dom,
    });
  })()`).then(JSON.parse);
  const packetEntries = (snapshot) => snapshot.entries.filter(entry => entry.kind === 'packet');
  for (const [name, ok] of [
    ['keyboard replace/add/remove/recall preserves exact current IDs',
      out.controlGroups.mine.length === 3 &&
      JSON.stringify(out.controlGroups.keyboard.groups['1']) ===
        JSON.stringify([out.controlGroups.mine[0], out.controlGroups.mine[2]]) &&
      JSON.stringify(out.controlGroups.keyboard.selection) ===
        JSON.stringify([out.controlGroups.mine[0], out.controlGroups.mine[2]])],
    ['touch slot and toolbar perform the same add/remove/recall semantics',
      JSON.stringify(out.controlGroups.touch.groups['2']) === JSON.stringify([out.controlGroups.mine[2]]) &&
      JSON.stringify(out.controlGroups.touch.selection) === JSON.stringify([out.controlGroups.mine[2]])],
    ['foreign, invalid, and cleared group IDs never survive membership validation',
      JSON.stringify(out.controlGroups.filtered.groups['3']) === JSON.stringify([out.controlGroups.mine[0]]) &&
      out.controlGroups.cleared.groups['2'].length === 0 &&
      out.controlGroups.filtered.nativePersistence === 'unavailable' &&
      out.controlGroups.filtered.objectGeneration === 'unavailable'],
    ['issued selection packets remain visibly pending until a tick drains them',
      out.controlGroups.pendingGroups.pending > 0 &&
      packetEntries(out.controlGroups.pendingGroups).every(entry =>
        entry.status === 'pending' && entry.hex.length >= 2 && entry.issuedFrame === 0)],
    ['drained GroupCommands are visibly applied from exported counters',
      out.controlGroups.appliedGroups.pending === 0 &&
      packetEntries(out.controlGroups.appliedGroups).every(entry =>
        entry.struct === 'GroupCommand' && entry.status === 'applied')],
    ['a real MoveTo packet transitions from pending to applied after one tick',
      packetEntries(out.controlGroups.pendingMove).some(entry =>
        entry.struct === 'MoveToCommand' && entry.status === 'pending') &&
      packetEntries(out.controlGroups.appliedMove).some(entry =>
        entry.struct === 'MoveToCommand' && entry.status === 'applied')],
    ['a no-selection MoveTo refusal is attributed and rendered without guessing',
      packetEntries(out.controlGroups.pendingRefusal).some(entry =>
        entry.struct === 'MoveToCommand' && entry.status === 'pending') &&
      packetEntries(out.controlGroups.refused).some(entry =>
        entry.struct === 'MoveToCommand' && entry.status === 'refused' &&
        entry.reason.includes('nothing selected')) &&
      out.controlGroups.dom.refusedRows >= 1 && out.controlGroups.dom.appliedRows >= 1 &&
      out.controlGroups.dom.summary.includes('refused')],
    ['group actions and lifecycle feedback remain visible and announced',
      out.controlGroups.cleared.status.includes('cleared') && out.controlGroups.cleared.active === '2' &&
      out.controlGroups.dom.groupStatus.includes('recalled group 1') && out.controlGroups.dom.activeSlot === '1'],
  ]) {
    if (!ok) { console.error(`FAIL: ${name}`); bad++; }
  }

  // The minimap is an exported-world navigator, not fog or diplomacy evidence. Exercise
  // panel, pointer, and keyboard navigation while checking the snapshot totals and explicit
  // unavailable victory surface.
  out.objectives = await c.eval(`(() => {
    const d = window.don, m = d.state.mod;
    const snapshot = d.objectives.snapshot();
    const initial = d.objectives.camera();
    const focused = d.objectives.focusPlayer(2);
    const mini = document.getElementById('mini');
    const rect = mini.getBoundingClientRect();
    mini.dispatchEvent(new PointerEvent('pointerdown', {
      bubbles: true, button: 0, buttons: 1,
      clientX: rect.left + rect.width * .78,
      clientY: rect.top + rect.height * .22,
    }));
    const pointer = d.objectives.camera();
    document.querySelector('.world-player[data-player="1"] button').click();
    const panel = d.objectives.camera();
    mini.focus();
    mini.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, code: 'Digit4', key: '4' }));
    const keyboard = d.objectives.camera();
    const returned = d.objectives.focusPlayer(0);
    return JSON.stringify({
      snapshot, initial, focused, pointer, panel, keyboard, returned,
      live: m.live,
      cameraStatus: document.getElementById('camera-status').textContent,
      cameraLive: document.getElementById('camera-status').getAttribute('aria-live'),
    });
  })()`).then(JSON.parse);
  for (const [name, ok] of [
    ['the world snapshot accounts for every live exported object',
      out.objectives.snapshot.owners.reduce((n, owner) => n + owner.objects, 0) === out.objectives.live],
    ['the world snapshot refuses victory, score, countdown, diplomacy, and fog claims',
      out.objectives.snapshot.visibility === 'omniscient-export' &&
      out.objectives.snapshot.victory === 'unavailable' && out.objectives.snapshot.score === 'unavailable' &&
      out.objectives.snapshot.countdown === 'unavailable' && out.objectives.snapshot.diplomacy === 'unavailable'],
    ['player focus buttons navigate to exported starts',
      out.objectives.focused.source.includes('P2') && out.objectives.panel.source.includes('P1')],
    ['minimap pointer navigation moves the camera',
      out.objectives.pointer.source === 'minimap' &&
      (out.objectives.pointer.tileX !== out.objectives.focused.tileX ||
       out.objectives.pointer.tileY !== out.objectives.focused.tileY)],
    ['minimap keyboard navigation reaches owner starts and can return home',
      out.objectives.keyboard.source.includes('P3') && out.objectives.returned.source.includes('P0')],
    ['camera navigation feedback is announced',
      out.objectives.cameraLive === 'polite' && out.objectives.cameraStatus.includes('camera tile')],
  ]) {
    if (!ok) { console.error(`FAIL: ${name}`); bad++; }
  }

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
    document.getElementById('tab-build').click();
    const paletteFilter = document.getElementById('palette-filter');
    paletteFilter.value = 'barracks';
    paletteFilter.dispatchEvent(new Event('input', { bubbles: true }));
    const barracksAction = document.querySelector('#palette [data-kind="build"][data-type-id="427"]');
    note('buildPaletteAction', !!barracksAction && !barracksAction.disabled);
    barracksAction?.click();
    note('buildPaletteArmed', s.buildType === 427);
    paletteFilter.value = '';
    paletteFilter.dispatchEvent(new Event('input', { bubbles: true }));
    let placed = null;
    search:
    for (let r = 3; r < 22; r++) {
      for (let dy = -r; dy <= r; dy++) for (let dx = -r; dx <= r; dx++) {
        const tx = stx + dx, ty = sty + dy;
        if (m.placementGrade(0, 427, tx, ty) === 4) {
          const b = s.play.buildings['427'];
          window.don.order.build([
            (tx + (b.xSize >> 1)) * m.subtile,
            (ty + (b.ySize >> 1)) * m.subtile,
          ]);
          placed = [tx, ty]; break search;
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
      document.getElementById('tab-train').click();
      const trainAction = document.querySelector('#palette [data-kind="train"]:not(:disabled)');
      const futureTrainDisabled = [...document.querySelectorAll('#palette [data-kind="train"]:disabled')]
        .some(button => button.querySelector('.why')?.textContent.includes('requires age'));
      note('trainPaletteAction', !!trainAction);
      note('futureTrainDisabled', futureTrainDisabled);
      const trainedType = Number(trainAction?.dataset.typeId ?? -1);
      const countType = () => {
        let n = 0;
        for (let i = 0; i < m.live; i++) {
          const id = m.idAtRow(i), inf = id >= 0 ? m.info(id) : null;
          if (inf && inf.typeId === trainedType) n++;
        }
        return n;
      };
      const trainedBefore = countType();
      trainAction?.click();
      trainAction?.click();
      m.step(1);
      await new Promise(resolve => setTimeout(resolve, 160));
      const queued = m.info(barracks);
      note('queueAfterPalette', queued ? queued.queueN : -1);
      note('queueFeedback', document.getElementById('palette-feedback').textContent);
      for (let i = 0; i < 1500; i++) m.step(1);
      trained = countType() - trainedBefore;
    }
    note('trained', trained);

    // Advance an age through the only implemented research path. The second card stays
    // disabled because ordinary technology/prerequisite execution is not exported.
    const ageBefore = m.player(0).age;
    document.getElementById('tab-research').click();
    const ageAction = document.querySelector('#palette [data-kind="research"]:not(:disabled)');
    const otherTechDisabled = !!document.querySelector('#palette [data-kind="unavailable"]:disabled');
    note('researchPaletteAction', !!ageAction);
    note('otherTechDisabled', otherTechDisabled);
    note('researchContext', document.getElementById('palette-context').textContent);
    ageAction?.click();
    m.step(1);
    note('researchStarted', m.player(0).research > 0);
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
    ['selection-driven build palette armed the recovered Barracks action',
      S.buildPaletteAction === true && S.buildPaletteArmed === true],
    ['a building finished', S.barracksBuilt === true],
    ['the producer menu is non-empty', S.barracksProducts > 0],
    ['training used an enabled WHERE-edge action and disabled future-age actions',
      S.trainPaletteAction === true && S.futureTrainDisabled === true],
    ['palette queue feedback exposes both queued items',
      S.queueAfterPalette === 2 && S.queueFeedback.includes('live queue') && S.queueFeedback.includes('(2/8)')],
    ['a unit was trained', S.trained > 0],
    ['research exposes age advance and disables unsupported technologies',
      S.researchPaletteAction === true && S.otherTechDisabled === true && S.researchStarted === true &&
      S.researchContext.includes('prerequisite hosts are unavailable')],
    ['the age advanced', S.age[1] > S.age[0]],
  ]) {
    if (!ok) { console.error(`FAIL: ${name}`); bad++; }
  }
  out.scriptTransport = await c.eval('window.don.readiness().transport');
  for (const [name, ok] of [
    ['commands crossed the exact packet boundary', out.scriptTransport.submitted > 0],
    ['every submitted packet drained at a tick',
      out.scriptTransport.drained === out.scriptTransport.submitted && out.scriptTransport.pending === 0],
    ['packet handlers applied object orders', out.scriptTransport.ordersApplied > 0],
  ]) {
    if (!ok) { console.error(`FAIL: ${name}`); bad++; }
  }
  await sleep(150);
  out.objectivesAfterPlay = await c.eval(`(() => {
    const snapshot = window.don.objectives.snapshot();
    return JSON.stringify({
      snapshot,
      live: window.don.state.mod.live,
      playerZeroText: document.getElementById('owner-state-0').textContent,
      scoreText: document.getElementById('objective-score').textContent,
    });
  })()`).then(JSON.parse);
  for (const [name, ok] of [
    ['the owner snapshot updates after real building and training',
      out.objectivesAfterPlay.snapshot.owners[0].objects >= out.objectives.snapshot.owners[0].objects + 3 &&
      out.objectivesAfterPlay.playerZeroText.includes(
        `${out.objectivesAfterPlay.snapshot.owners[0].objects} objects`)],
    ['post-play owner totals still equal the live world',
      out.objectivesAfterPlay.snapshot.owners.reduce((n, owner) => n + owner.objects, 0) ===
        out.objectivesAfterPlay.live],
    ['object growth is never relabelled as victory score',
      out.objectivesAfterPlay.scoreText.includes('not a victory score')],
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
