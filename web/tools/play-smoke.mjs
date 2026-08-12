// Drive the playable client in a real browser and assert it is actually a game.
//
// A screenshot is not evidence — the spectator lane proved that four different capture
// paths all report "black" whether the renderer drew or not. So this script does three
// things a screenshot cannot:
//
//   1. reads back pixels the *renderer itself* owns (`window.don.snapshot()`), and counts
//      non-black pixels and distinct colours;
//   2. drives selection, MoveTo, City training, Library construction, and age research through
//      real input, proves unsupported actions fail closed, and resumes post-step core saves;
//   3. measures frames per second over a fixed window at a stated unit count, and reports
//      the browser, backend and viewport it measured on.
//
//     node web/serve.mjs &
//     node web/tools/play-smoke.mjs [--backend canvas2d] [--json out.json] [--keep]
//
// Exit 0 only if the playable boundary held, the renderer drew, and nothing threw.

import { spawn, spawnSync } from 'node:child_process';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = join(HERE, '..', '..');

for (const [script, scriptArgs] of [
  ['gen-wire.mjs', ['--check']],
  ['gen-readiness.mjs', ['--check']],
  ['check-play-wasm.mjs', []],
]) {
  const gate = spawnSync(process.execPath, [join(HERE, script), ...scriptArgs], {
    cwd: REPO,
    stdio: 'inherit',
  });
  if (gate.status !== 0) {
    console.error(`play smoke preflight failed: ${script} ${scriptArgs.join(' ')}`.trim());
    process.exit(4);
  }
}

const args = process.argv.slice(2);
const flag = (n, d = null) => { const i = args.indexOf(n); return i < 0 ? d : (args[i + 1] ?? true); };
const has = (n) => args.includes(n);

const PORT = Number(flag('--port', 8787));
const CDP = Number(flag('--cdp', 9336));
const KEEP = has('--keep');
const OUT = flag('--json', null);
const BACKEND = flag('--backend', null);
const LOCAL_MATCH = has('--local-match');

const CHROME = [
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/Applications/Google Chrome Dev.app/Contents/MacOS/Google Chrome Dev',
  '/Applications/Chromium.app/Contents/MacOS/Chromium',
  '/usr/bin/google-chrome',
  '/usr/bin/chromium',
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
let proc = null, c = null, c2 = null;
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

  // Ask for unsupported setup values on purpose. The client must canonicalize those to its
  // fixed/read-only core facts; no URL-only lobby option may become browser shadow state.
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

  // This module is served to and executed by Chrome, not merely imported by the Node tests.
  // The renderer id is only a lookup key; the exact retail bytes must contain returned `o=1`.
  out.canonicalGroupMoveBridge = await c.eval(`import('./js/play/canonical-command-package.mjs')
    .then((codec) => {
      const rendererId = 0x710000;
      const identity = codec.discoverOwnerLocalSingleton(
        [rendererId], (seen) => seen === rendererId ? { who: 0, o: 1, uid: 0x6a31 } : null, 0);
      const built = codec.encodeCanonicalSingletonGroupMove(identity, 47435, 47486);
      const decoded = codec.decodeCanonicalCommandPackage(built.bytes);
      let mutationRefused = false;
      const changed = new Uint8Array(built.bytes);
      changed[25] = 49;
      try { codec.decodeCanonicalCommandPackage(changed); } catch { mutationRefused = true; }
      return {
        rendererId, identity, bytes: built.bytes.length, hex: built.hex,
        decodedWho: decoded.who, decodedO: decoded.o, mutationRefused,
      };
    }).then(JSON.stringify)`).then(JSON.parse);
  if (out.canonicalGroupMoveBridge.bytes !== 27 ||
      out.canonicalGroupMoveBridge.hex !==
        '0001000100074bb900007eb9000000000000000000000102003200' ||
      out.canonicalGroupMoveBridge.decodedWho !== 0 ||
      out.canonicalGroupMoveBridge.decodedO !== 1 ||
      out.canonicalGroupMoveBridge.rendererId === out.canonicalGroupMoveBridge.decodedO ||
      !out.canonicalGroupMoveBridge.mutationRefused) {
    console.error('FAIL: browser canonical Group+Move package/identity boundary');
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
    let mobile = -1, city = -1;
    for (let row = 0; row < m.live; row++) {
      const tag = views.tag[row];
      if ((tag & 0x80000000) && (tag & 0xf) === 0 && !((tag >>> 29) & 1)) {
        if (mobile < 0) mobile = m.idAtRow(row);
      } else if ((tag & 0x80000000) && (tag & 0xf) === 0 && ((tag >>> 29) & 1)) {
        const candidate = m.idAtRow(row), info = m.info(candidate);
        if (info?.typeId === 414 && info.buildProgress < 0) city = candidate;
      }
    }
    window.don.select([mobile]);
    filter.value = 'barracks';
    filter.dispatchEvent(new Event('input', { bubbles: true }));
    const filteredCatalog = document.querySelectorAll('#palette button').length;
    const buildButtonEnabled = !document.querySelector('#palette button')?.disabled;
    filter.value = 'library';
    filter.dispatchEvent(new Event('input', { bubbles: true }));
    const libraryBuildEnabled = !!document.querySelector('#palette [data-kind="build"]:not(:disabled)');
    const buildDockEnabled = !document.getElementById('cmd-build').disabled;
    filter.value = '';
    filter.dispatchEvent(new Event('input', { bubbles: true }));
    const futureBuildDisabled = [...document.querySelectorAll('#palette button')]
      .some(button => button.disabled && button.querySelector('.why')?.textContent.includes('requires age'));
    window.don.select([city]);
    window.don.key('KeyT', { shiftKey: true });
    const keyboardTrain = document.getElementById('tab-train').classList.contains('sel');
    const trainHasEnabledUnit = !!document.querySelector('#palette [data-kind="train"]:not(:disabled)');
    const trainDockEnabled = !document.getElementById('cmd-train').disabled;
    const trainContext = document.getElementById('palette-context').textContent;
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
      libraryBuildEnabled, buildDockEnabled,
      futureBuildDisabled, keyboardTrain, trainHasEnabledUnit, trainDockEnabled, trainContext,
      keyboardResearch, researchHasEnabledAge,
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
      sessionActivate: !!document.getElementById('session-activate'),
      sessionSetup: window.don.session.setup(),
      sessionUrl: window.don.session.url(),
      coreActivationExported: typeof m.x.game_activate_player === 'function',
      coreTeamSetupExported: typeof m.x.game_start_manual_teams === 'function',
      coreRosterQueryExported: typeof m.x.game_active_player_mask === 'function',
      coreActiveMask: m.x.game_active_player_mask(m.g) >>> 0,
      unsupportedSetupExportsAbsent:
        typeof m.x.game_set_team === 'undefined' &&
        typeof m.x.game_set_victory_mode === 'undefined',
      sessionUnsupportedDisabled: [
        'session-map', 'session-size', 'session-nation',
        'session-ai-slots', 'session-ai-difficulty', 'income', 'popset',
      ].every(id => document.getElementById(id)?.disabled),
      sessionTeamEnabled: !document.getElementById('session-team')?.disabled,
      sessionVictoryReadOnly: document.getElementById('session-victory')?.disabled,
      sessionSummary: document.getElementById('session-summary')?.textContent ?? '',
      localMatchProtocol: window.don.localMatch.snapshot().protocol,
      localMatchAvailable: window.don.localMatch.snapshot().available,
      localMatchTurnRelay: window.don.localMatch.snapshot().turnRelay,
      localMatchControls: [
        'local-match-name', 'local-match-code', 'local-match-create', 'local-match-join',
        'local-match-ready', 'local-match-turn', 'local-match-leave',
        'local-match-status', 'local-match-roster',
      ].every(id => !!document.getElementById(id)),
      localMatchBoundary: document.getElementById('session')?.textContent ?? '',
      coreMatch: m.match(),
      coreLeader: m.leader(0),
      coreRelation: m.relation(0, 1),
      objectiveTime: document.getElementById('objective-time')?.textContent ?? '',
      objectiveState: document.getElementById('objective-state')?.textContent ?? '',
      objectiveScore: document.getElementById('objective-score')?.textContent ?? '',
      objectiveCountdown: document.getElementById('objective-countdown')?.textContent ?? '',
      objectiveFactsProjected:
        ['objective-state', 'objective-score']
          .every(id => document.getElementById(id)?.getAttribute('aria-disabled') !== 'true') &&
        document.getElementById('objective-countdown')?.getAttribute('aria-disabled') === 'true',
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
      coreSaveActionsEnabled: ['core-save', 'core-load']
        .every(id => !document.getElementById(id)?.disabled),
      retailReplayDisabled: [...document.querySelectorAll('#replay-capabilities button')]
        .filter(button => !['core-save', 'core-load'].includes(button.id))
        .every(button => button.disabled),
      coreSaveStatusLive: document.getElementById('core-save-status')?.getAttribute('aria-live') ?? '',
      replayCapabilities: document.getElementById('replay-capabilities')?.textContent ?? '',
      replayPauseWorked, replaySpeedWorked,
      controlGroupSlots: document.querySelectorAll('#group-slots .group-slot').length,
      controlGroupActions: document.querySelectorAll('#group-actions button').length,
      controlGroupLive: document.getElementById('group-status')?.getAttribute('aria-live') ?? '',
      controlGroupBoundary: document.getElementById('control-groups')?.textContent ?? '',
      commandHistoryRecords: document.querySelectorAll('#command-history-list .command-record').length,
      commandHistorySummary: document.getElementById('command-history-summary')?.textContent ?? '',
      commandFeedbackBoundary: document.getElementById('command-history')?.textContent ?? '',
      settingsProtocol: window.don.settings.snapshot().protocol,
      settingsBindings: document.querySelectorAll('#settings-bindings .binding-row').length,
      settingsStatusLive: document.getElementById('settings-status')?.getAttribute('aria-live') ?? '',
      settingsRenderer: document.getElementById('settings-renderer')?.value ?? '',
      settingsFps: document.getElementById('settings-fps')?.value ?? '',
      settingsActions: ['settings-export', 'settings-import', 'settings-reset']
        .every(id => !!document.getElementById(id)),
      settingsUnavailableDisabled: [...document.querySelectorAll('#settings .settings-boundary button')]
        .every(button => button.disabled),
      settingsBoundary: document.getElementById('settings')?.textContent ?? '',
      targetButtonsDisabledWithoutSelection: ['cmd-move', 'cmd-attack', 'cmd-gather']
        .every(id => document.getElementById(id)?.disabled),
      toastLiveRegion: document.getElementById('toast')?.getAttribute('aria-live') ?? '',
    });
  })()`).then(JSON.parse);
  for (const [name, ok] of [
    ['the initial order catalog is populated', out.ui.initialCatalog > 0],
    ['build commands require a selection', out.ui.initialCatalogDisabled],
    ['catalog filtering works', out.ui.filteredCatalog === 1],
    ['only the bounded Library build is enabled for a selected Citizen',
      !out.ui.buildButtonEnabled && out.ui.libraryBuildEnabled && out.ui.buildDockEnabled],
    ['known future-age prerequisites disable building actions', out.ui.futureBuildDisabled],
    ['keyboard shortcuts open train and research modes', out.ui.keyboardTrain && out.ui.keyboardResearch],
    ['the opening City exposes an enabled authoritative train action and touch dock affordance',
      out.ui.trainHasEnabledUnit && out.ui.trainDockEnabled && out.ui.trainContext.includes('production runtime')],
    ['age research waits for a selected completed Library while other tech stays disabled',
      !out.ui.researchHasEnabledAge && out.ui.unavailableResearchDisabled && out.ui.orderTabs === 3],
    ['pause visibly becomes resume', out.ui.pauseLabel.includes('resume')],
    ['the page identifies itself as an integration build', out.ui.integrationLabel.includes('Not Fidelity mode')],
    ['runtime identity says the authoritative Sim adapter is not Arena',
      out.ui.runtimeLabel.includes('don_sim::tick::Sim') && out.ui.runtimeLabel.includes('connected: no')],
    ['the compiled playable gate is visibly blocked', out.ui.gateLabel.includes('BLOCKED')],
    ['the replay card reports non-empty walks and zero matches',
      out.ui.replayLabel.includes('world walks non-empty') && out.ui.replayLabel.includes('0 matches')],
    ['the local boundary and compiled blockers are inspectable', out.ui.blockerItems > 1],
    ['no whole-world fidelity option is advertised', out.ui.incomeOptions.every((x) => !/^fidelity\b/i.test(x))],
    ['the touch command dock is complete', out.ui.commandButtons >= 9],
    ['session setup exposes the deterministic seed', out.ui.sessionSeed === '0x00c0ffee'],
    ['session setup exposes every player perspective', out.ui.sessionPlayers >= 2],
    ['atomic Sim PlayerSetup activation is exposed but not hidden in world creation',
      out.ui.coreActivationExported && out.ui.coreTeamSetupExported &&
      out.ui.coreRosterQueryExported && out.ui.sessionActivate &&
      out.ui.coreActiveMask === 0 && out.ui.sessionSetup.phase === 'setup' &&
      JSON.stringify(out.ui.sessionSetup.activePlayers) === JSON.stringify([]) &&
      !out.ui.coreLeader.active],
    ['team and victory setters remain absent from the Wasm ABI',
      out.ui.unsupportedSetupExportsAbsent],
    ['session identity and seed boundary are visible', out.ui.sessionStatus.includes('0x00c0ffee') &&
      out.ui.sessionStatus.includes('player 0') && out.ui.sessionStatus.includes('owned by don_sim::Sim')],
    ['session changes are announced', out.ui.sessionStatusLive === 'polite'],
    ['session links are shareable', out.ui.sessionShare],
    ['unsupported setup choices stay disabled', out.ui.sessionUnsupportedDisabled],
    ['team layout is frame-zero mutable while victory remains read-only',
      out.ui.sessionTeamEnabled && out.ui.sessionVictoryReadOnly],
    ['the local lobby surface exposes the bounded canonical Group→Move barrier',
      out.ui.localMatchProtocol === 'don.local-match-handoff.v1' && out.ui.localMatchControls &&
      out.ui.localMatchTurnRelay === 'canonical-group-move-v1' &&
      out.ui.localMatchBoundary.includes('GroupCommand') &&
      out.ui.localMatchBoundary.includes('MoveToCommand') &&
      out.ui.localMatchBoundary.includes('receipt-bearing')],
    ['the configured local MatchStart service is available for this smoke',
      !LOCAL_MATCH || out.ui.localMatchAvailable],
    ['unsupported URL requests are canonicalized to authoritative facts rather than fabricated',
      out.ui.sessionSetup.map === 'integration-land' && out.ui.sessionSetup.size === '128x128' &&
      out.ui.sessionSetup.nation === 'unavailable' && out.ui.sessionSetup.team === 0 &&
      out.ui.sessionSetup.aiSlots === 'unavailable' && out.ui.sessionSetup.aiDifficulty === 'unavailable' &&
      out.ui.sessionSetup.victory === 'standard' && !out.ui.sessionSetup.teamConfigured &&
      out.ui.sessionSetup.teamMutable && out.ui.sessionSetup.teamLayout === 'ffa' &&
      !out.ui.sessionSetup.victoryMutable],
    ['team, diplomacy, and victory values come through the read-only core ABI',
      out.ui.coreMatch.slug === 'standard' && !out.ui.coreMatch.gameOver &&
      out.ui.coreLeader.team === 0 && !out.ui.coreLeader.teamConfigured &&
      out.ui.coreLeader.teamSource.includes('default-own-slot') && out.ui.coreLeader.score === 0 &&
      out.ui.coreRelation.name === 'war'],
    ['unsupported rules are canonicalized instead of accepted from the URL',
      out.ui.sessionSetup.income === 'unavailable' && out.ui.sessionSetup.population === 'unavailable' &&
      out.boot.player.popCap === 0],
    ['the pregame summary exposes world, slots, read-only facts, and unavailable systems',
      out.ui.sessionSummary.includes('setup · roster inactive') &&
      out.ui.sessionSummary.includes('128 × 128') && out.ui.sessionSummary.includes('manual') &&
      out.ui.sessionSummary.includes('population unavailable') &&
      out.ui.sessionSummary.includes('Standard victory (read-only)')],
    ['the objective panel projects core mode/score but disables the absent countdown',
      out.ui.objectiveTime.includes('frame') && out.ui.objectiveFactsProjected &&
      out.ui.objectiveState.includes('Standard') && out.ui.objectiveState.includes('setup') &&
      out.ui.objectiveState.includes('core read-only') &&
      out.ui.objectiveScore.includes('victory 0') &&
      out.ui.objectiveCountdown.includes('not exported')],
    ['the objective panel labels omniscient visibility and inactive relations honestly',
      out.ui.visibilityHonesty.includes('Omniscient integration view') &&
      out.ui.visibilityHonesty.includes('read-only core facts') &&
      out.ui.ownerStates.slice(1).every(text =>
        text.includes('war') && text.includes('inactive leader slot'))],
    ['the real minimap exposes every owner with queried teams but no fog claim',
      out.ui.ownerLegend === 4 && out.ui.ownerRows === 4 &&
      out.ui.minimapLabel.includes('All exported owners are visible') &&
      out.ui.minimapLabel.includes('fog is unavailable')],
    ['the command journal exposes playback, timeline, import, and export controls',
      out.ui.replayProtocol === 'don.command-journal.v3' && out.ui.replayControls >= 5 &&
      out.ui.replayTimeline.includes('Command journal frame') && out.ui.replayJournalActionsEnabled &&
      out.ui.replayPauseWorked && out.ui.replaySpeedWorked],
    ['journal feedback is announced and does not claim to be a native save',
      out.ui.replayStatusLive === 'polite' && out.ui.replayStatus.includes('not a native save-state')],
    ['core save/load is enabled and announced while retail replay stays disabled',
      out.ui.coreSaveActionsEnabled && out.ui.retailReplayDisabled &&
      out.ui.coreSaveStatusLive === 'polite' && out.ui.replayCapabilities.includes('deterministic authoritative') &&
      out.ui.replayCapabilities.includes('malformed files leave the running session unchanged') &&
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
    ['browser settings expose visual, performance, persistence, and remappable input controls',
      out.ui.settingsProtocol === 'don.browser-settings.v1' && out.ui.settingsBindings === 9 &&
      out.ui.settingsStatusLive === 'polite' && out.ui.settingsRenderer === (BACKEND ?? 'auto') &&
      out.ui.settingsFps === '0' && out.ui.settingsActions],
    ['audio and native profile settings stay disabled at absent ABI boundaries',
      out.ui.settingsUnavailableDisabled && out.ui.settingsBoundary.includes('no mixer/audio ABI') &&
      out.ui.settingsBoundary.includes('no native profile/settings bridge')],
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
    const settings = document.getElementById('settings').getBoundingClientRect();
    const settingsControls = [...document.querySelectorAll('#settings-actions button, .binding-row button')]
      .map(button => button.getBoundingClientRect().height);
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
      settingsPanel: [settings.left, settings.right], settingsControlHeights: settingsControls,
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
    ['narrow settings remain in-page with touch-sized binding and persistence controls',
      out.narrow.settingsPanel[0] >= 0 && out.narrow.settingsPanel[1] <= out.narrow.viewport[0] &&
      out.narrow.settingsControlHeights.every(height => height >= 40)],
    ['narrow layout keeps fidelity counters visible', out.narrow.coverageVisible],
  ]) {
    if (!ok) { console.error(`FAIL: ${name}`); bad++; }
  }
  await c.send('Emulation.clearDeviceMetricsOverride');
  await sleep(250);

  // Settings must be live, collision-checked, persisted, and reloadable. Drive the actual
  // form controls (including key-capture), prove visual FPS throttling leaves simulation
  // advancing, reject conflicting/reserved imports without mutation, reload the page from
  // localStorage, then reset so all later gameplay checks run on repository defaults.
  out.settings = await c.eval(`(() => {
    const d = window.don;
    d.replay.play();
    const setSelect = (id, value) => {
      const control = document.getElementById(id);
      control.value = value;
      control.dispatchEvent(new Event('change', { bubbles: true }));
    };
    setSelect('settings-scale', '1.15');
    setSelect('settings-palette', 'okabe-ito');
    const contrast = document.getElementById('settings-contrast');
    contrast.checked = true;
    contrast.dispatchEvent(new Event('change', { bubbles: true }));
    const motion = document.getElementById('settings-motion');
    motion.checked = true;
    motion.dispatchEvent(new Event('change', { bubbles: true }));
    setSelect('settings-fps', '20');
    document.querySelector('[data-binding="pause"]').click();
    window.dispatchEvent(new KeyboardEvent('keydown', {
      bubbles: true, code: 'KeyO', key: 'o',
    }));
    const afterCapture = d.settings.snapshot();
    d.replay.play();
    d.key('KeyP');
    const oldBindingInactive = !d.state.paused;
    d.key('KeyO');
    const newBindingActive = d.state.paused;
    d.replay.play();
    const exported = d.settings.export();
    const stable = d.settings.export();
    let conflictRefused = false, reservedRefused = false, malformedRefused = false;
    try {
      const conflict = JSON.parse(exported);
      conflict.input.bindings.halt = conflict.input.bindings.pause;
      d.settings.import(conflict);
    } catch { conflictRefused = true; }
    try {
      const reserved = JSON.parse(exported);
      reserved.input.bindings.build = 'KeyW';
      d.settings.import(reserved);
    } catch { reservedRefused = true; }
    try { d.settings.import('{"protocol":"wrong"}'); }
    catch { malformedRefused = true; }
    const invalidPreserved = d.settings.export() === stable;
    d.settings.reset();
    const imported = d.settings.import(exported);
    const stored = JSON.parse(localStorage.getItem(imported.storageKey));
    const root = document.documentElement;
    return JSON.stringify({
      afterCapture, imported, stored, exported,
      conflictRefused, reservedRefused, malformedRefused, invalidPreserved,
      oldBindingInactive, newBindingActive,
      css: {
        contrast: root.dataset.contrast,
        motion: root.dataset.reducedMotion,
        palette: root.dataset.ownerPalette,
        scale: getComputedStyle(document.getElementById('side')).zoom,
      },
      rendererPalette: d.state.gfx.ownerPalette.slice(),
      sessionBackend: new URL(d.session.url()).searchParams.get('backend'),
      frameBeforeCapWait: d.state.mod.frame,
      settingsStatus: document.getElementById('settings-status').textContent,
      disabledBoundaries: [...document.querySelectorAll('#settings .settings-boundary button')]
        .every(button => button.disabled),
    });
  })()`).then(JSON.parse);
  await sleep(1400);
  out.settings.cap = await c.eval(`(() => ({
    fps: window.don.state.fps,
    frame: window.don.state.mod.frame,
    cap: window.don.settings.snapshot().performance.fpsCap,
  }))()`);
  for (const [name, ok] of [
    ['visual settings apply live to contrast, reduced motion, owner palette, and UI scale',
      out.settings.css.contrast === 'high' && out.settings.css.motion === 'true' &&
      out.settings.css.palette === 'okabe-ito' && Number(out.settings.css.scale) === 1.15 &&
      out.settings.rendererPalette[0] === '#0072b2'],
    ['key capture remaps pause and removes the old binding',
      out.settings.afterCapture.input.bindings.pause === 'KeyO' &&
      out.settings.oldBindingInactive && out.settings.newBindingActive],
    ['conflicting, fixed-camera, and malformed imports are refused without mutation',
      out.settings.conflictRefused && out.settings.reservedRefused && out.settings.malformedRefused &&
      out.settings.invalidPreserved],
    ['settings export/reset/import round-trips the versioned browser profile',
      out.settings.imported.protocol === 'don.browser-settings.v1' &&
      out.settings.stored.visual.ownerPalette === 'okabe-ito' &&
      out.settings.stored.input.bindings.pause === 'KeyO'],
    ['renderer preference is encoded through the real backend URL seam',
      out.settings.imported.performance.renderer === (BACKEND ?? 'auto') &&
      out.settings.imported.activeRenderer === out.boot.backend &&
      out.settings.sessionBackend === (BACKEND ?? null)],
    ['the visual FPS cap throttles drawing while simulation time keeps advancing',
      out.settings.cap.cap === 20 && out.settings.cap.fps >= 15 && out.settings.cap.fps <= 25 &&
      out.settings.cap.frame >= out.settings.frameBeforeCapWait + 12],
    ['audio/native settings remain disabled and persistence is announced',
      out.settings.disabledBoundaries && out.settings.settingsStatus.includes('saved')],
  ]) {
    if (!ok) { console.error(`FAIL: ${name}`); bad++; }
  }

  // Persist a 30 FPS profile, reload the actual page, and wait for a genuinely new JS realm.
  await c.eval(`(() => {
    const next = JSON.parse(window.don.settings.export());
    next.performance.fpsCap = 30;
    window.don.settings.apply(next);
    return performance.timeOrigin;
  })()`);
  const oldTimeOrigin = await c.eval('performance.timeOrigin');
  await c.send('Page.reload');
  let settingsReloaded = false;
  for (let i = 0; i < 100; i++) {
    await sleep(100);
    try {
      settingsReloaded = await c.eval(`performance.timeOrigin !== ${oldTimeOrigin} &&
        !!(window.don && window.don.ready && window.don.ready())`);
    } catch {}
    if (settingsReloaded) break;
  }
  if (!settingsReloaded) throw new Error('settings persistence reload never became ready');
  out.settings.persisted = await c.eval(`(() => {
    const d = window.don, snapshot = d.settings.snapshot();
    d.replay.play();
    d.key('KeyO');
    const remapSurvived = d.state.paused;
    d.replay.play();
    return {
      snapshot, remapSurvived,
      root: {
        contrast: document.documentElement.dataset.contrast,
        motion: document.documentElement.dataset.reducedMotion,
        palette: document.documentElement.dataset.ownerPalette,
        scale: getComputedStyle(document.getElementById('side')).zoom,
      },
      urlBackend: new URL(location.href).searchParams.get('backend'),
    };
  })()`);
  for (const [name, ok] of [
    ['visual, input, and performance settings survive a real page reload',
      out.settings.persisted.snapshot.visual.ownerPalette === 'okabe-ito' &&
      out.settings.persisted.snapshot.visual.highContrast &&
      out.settings.persisted.snapshot.visual.reducedMotion &&
      out.settings.persisted.snapshot.performance.fpsCap === 30 &&
      out.settings.persisted.remapSurvived],
    ['the persisted profile is reflected in the reloaded DOM and renderer URL',
      out.settings.persisted.root.contrast === 'high' &&
      out.settings.persisted.root.motion === 'true' &&
      out.settings.persisted.root.palette === 'okabe-ito' &&
      Number(out.settings.persisted.root.scale) === 1.15 &&
      out.settings.persisted.urlBackend === (BACKEND ?? null)],
  ]) {
    if (!ok) { console.error(`FAIL: ${name}`); bad++; }
  }
  out.settings.reset = await c.eval(`(() => {
    const d = window.don, reset = d.settings.reset();
    d.replay.play();
    d.key('KeyP');
    const defaultPauseWorks = d.state.paused;
    d.replay.play();
    return {
      reset, defaultPauseWorks,
      stored: JSON.parse(localStorage.getItem(reset.storageKey)),
      palette: d.state.gfx.ownerPalette.slice(),
    };
  })()`);
  for (const [name, ok] of [
    ['reset restores and persists repository defaults for subsequent play',
      out.settings.reset.reset.visual.ownerPalette === 'standard' &&
      out.settings.reset.reset.visual.uiScale === 1 &&
      out.settings.reset.reset.performance.fpsCap === 0 &&
      out.settings.reset.reset.input.bindings.pause === 'KeyP' &&
      out.settings.reset.defaultPauseWorks &&
      out.settings.reset.stored.visual.ownerPalette === 'standard' &&
      out.settings.reset.palette[0] === '#5c9eff'],
  ]) {
    if (!ok) { console.error(`FAIL: ${name}`); bad++; }
  }
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
    ['restart exposes packed starting goods in the core ledger without advancing',
      JSON.stringify(out.session.stockAfterRestart) === JSON.stringify([200, 200, 100, 100, 100, 100])],
    ['unsupported setup rules are explicit and do not fabricate a core population cap',
      out.session.configured.income === 'unavailable' && out.session.configured.population === 'unavailable' &&
      out.session.popCapAfterRuleTick === 0],
    ['the selected seed is consumed by the authoritative core initializer',
      out.session.newDigest !== out.session.oldDigest && out.session.status.includes('owned by don_sim::Sim')],
    ['the share URL carries the canonical seed and player',
      out.session.urlSeed === '0x1234abcd' && out.session.urlPlayer === '0'],
    ['the share URL records the fixed world, queried team, and missing player systems',
      out.session.urlMap === 'integration-land' && out.session.urlSize === '128x128' &&
      out.session.urlNation === 'unavailable' && out.session.urlTeam === 'ffa' &&
      out.session.urlSlots === '' && out.session.urlAiSlots === 'unavailable' &&
      out.session.urlAiDifficulty === 'unavailable'],
    ['the share URL records the read-only victory and unavailable rule hosts',
      out.session.urlIncome === 'unavailable' && out.session.urlPopulation === 'unavailable' &&
      out.session.urlVictory === 'standard'],
    ['a malformed seed preserves the live session', out.session.invalidPreserved],
    ['player perspective switches and returns', out.session.switched === 1 && out.session.returned === 0],
    ['session status returns to player zero', out.session.status.includes('player 0')],
  ]) {
    if (!ok) { console.error(`FAIL: ${name}`); bad++; }
  }

  // The playable ABI owns `don_sim::Sim`: save bytes must be the core serializer's image,
  // load must atomically restore frame/digest/RNG/handles, and malformed input must leave
  // the live world and browser UI usable. Derived step-8 views must roundtrip from a live
  // post-step frame and resume without becoming a second serialized authority.
  out.coreSave = await c.eval(`(() => {
    const d = window.don;
    d.session.restart('0x5a17c0de');
    d.replay.pause();
    const m = d.state.mod;
    const ids = [];
    for (let row = 0; row < m.live; row++) {
      const tag = m.views().tag[row];
      if ((tag & 0x80000000) && (tag & 0xf) === 0) ids.push(m.idAtRow(row));
    }
    const before = {
      frame: m.frame, digest: m.digest(), rngState: m.rngState,
      live: m.live, first: m.info(ids[0]), selection: d.state.selection.length,
    };
    const bytes = d.save.export();
    const magic = Array.from(bytes.slice(0, 8));
    d.select(ids.slice(0, 2));
    d.controlGroups.replace(1, ids.slice(0, 2));
    m.moveTo(0, before.first.x + m.subtile * 8, before.first.y);
    d.replay.step();
    const advanced = {
      frame: m.frame, digest: m.digest(), rngState: m.rngState,
      live: m.live, first: m.info(ids[0]),
    };
    const postStepBytes = d.save.export();
    d.select([ids[0]]);
    m.moveTo(0, before.first.x + m.subtile * 12, before.first.y);
    d.replay.step();
    const mutated = { frame: m.frame, digest: m.digest(), rngState: m.rngState };
    const loaded = d.save.import(postStepBytes);
    const restored = {
      frame: m.frame, digest: m.digest(), rngState: m.rngState,
      live: m.live, first: m.info(ids[0]), selection: d.state.selection.length,
      groups: d.controlGroups.snapshot().groups,
    };
    const stableBeforeMalformed = { frame: m.frame, digest: m.digest(), rngState: m.rngState };
    const corrupt = new Uint8Array(postStepBytes);
    corrupt[0] ^= 0xff;
    let malformedRefused = false, malformedReason = '';
    try { d.save.import(corrupt); } catch (error) {
      malformedRefused = true; malformedReason = error.message;
    }
    const stableAfterMalformed = { frame: m.frame, digest: m.digest(), rngState: m.rngState };
    // The generation-aware handle bridge is usable after load: select the restored id,
    // issue a real MoveTo order, and observe it drain and advance.
    d.select([ids[0]]);
    m.moveTo(0, before.first.x + m.subtile * 4, before.first.y);
    d.replay.step();
    const resumed = {
      frame: m.frame, digest: m.digest(), transport: m.transport(), info: m.info(ids[0]),
    };
    d.session.restart('0x1234abcd');
    d.replay.play();
    return JSON.stringify({
      before, bytes: bytes.length, magic, advanced, postStepBytes: postStepBytes.length, mutated,
      loaded, restored, malformedRefused, malformedReason,
      stableBeforeMalformed, stableAfterMalformed, resumed,
    });
  })()`).then(JSON.parse);
  for (const [name, ok] of [
    ['core save exports the deterministic DoNSave image from a non-empty world',
      out.coreSave.before.live > 0 && out.coreSave.bytes > 0 &&
      String.fromCharCode(...out.coreSave.magic).startsWith('DoNSave')],
    ['post-step core load restores equal frame, digest, RNG, population, and object identity',
      out.coreSave.restored.frame === out.coreSave.advanced.frame &&
      out.coreSave.restored.digest === out.coreSave.advanced.digest &&
      out.coreSave.restored.rngState === out.coreSave.advanced.rngState &&
      out.coreSave.restored.live === out.coreSave.advanced.live &&
      out.coreSave.restored.first.id === out.coreSave.advanced.first.id],
    ['successful load resets browser-only selection and control groups and pauses safely',
      out.coreSave.loaded.selection === 0 && out.coreSave.loaded.groups === 0 &&
      out.coreSave.loaded.paused && out.coreSave.restored.selection === 0 &&
      Object.values(out.coreSave.restored.groups).every(ids => ids.length === 0)],
    ['live post-step state saves and load rewinds a later mutation exactly',
      out.coreSave.advanced.frame === 1 && out.coreSave.postStepBytes > 0 &&
      out.coreSave.mutated.frame === 2 && out.coreSave.mutated.digest !== out.coreSave.advanced.digest &&
      out.coreSave.restored.digest === out.coreSave.advanced.digest],
    ['malformed core bytes fail closed without changing frame, digest, or RNG',
      out.coreSave.malformedRefused && out.coreSave.malformedReason.includes('load refused') &&
      JSON.stringify(out.coreSave.stableAfterMalformed) === JSON.stringify(out.coreSave.stableBeforeMalformed)],
    ['post-load generational handles remain selectable and accept core orders',
      out.coreSave.resumed.frame === out.coreSave.advanced.frame + 1 &&
      out.coreSave.resumed.transport.ordersApplied > 0 &&
      out.coreSave.resumed.info.id === out.coreSave.before.first.id],
  ]) {
    if (!ok) { console.error(`FAIL: ${name}`); bad++; }
  }

  // Roster activation is deliberately after the inactive save/resume gate so the next
  // tranche independently proves DoNSave v11 over a live PlayerSetup/Leaders/Match owner.
  out.activation = await c.eval(`(async () => {
    const d = window.don;
    const teamLayout = document.getElementById('session-team');
    teamLayout.value = 'alternating-2v2';
    teamLayout.dispatchEvent(new Event('change', { bubbles: true }));
    const activated = d.session.activate();
    const m = d.state.mod;
    const leaders = Array.from({ length: m.playerCount }, (_, p) => m.leader(p));
    const activePlayers = m.activePlayers();
    const activeMask = m.x.game_active_player_mask(m.g) >>> 0;
    const match = m.match();
    const startedFrame = m.frame;
    const journal = JSON.parse(d.replay.export());
    const saveDisabled = document.getElementById('core-save').disabled;
    const status = document.getElementById('core-save-status').textContent;
    d.replay.step();
    const imported = await d.replay.import(JSON.stringify(journal));
    const importedMatch = m.match();
    const relations = Array.from({ length: m.playerCount }, (_, other) =>
      m.relation(0, other));
    let own = -1, teammate = -1;
    for (let row = 0; row < m.live; row++) {
      const id = m.idAtRow(row), info = id >= 0 ? m.info(id) : null;
      if (info?.owner === 0 && own < 0) own = id;
      if (info?.owner === 2 && teammate < 0) teammate = id;
    }
    m.group(0, [own]);
    m.step(1);
    const beforeFriendlyAttack = {
      gaps: m.gaps(), order: m.info(own)?.order, transport: m.transport(),
    };
    m.attack(0, teammate);
    m.step(1);
    const afterFriendlyAttack = {
      gaps: m.gaps(), order: m.info(own)?.order, transport: m.transport(),
    };
    const activeSaveBytes = d.save.export();
    const activeSaved = {
      frame: m.frame, digest: m.digest(), rngState: m.rngState,
      activePlayers: m.activePlayers(), match: m.match(),
      leaders: Array.from({ length: m.playerCount }, (_, p) => m.leader(p)),
      relations: Array.from({ length: m.playerCount }, (_, other) => m.relation(0, other)),
    };
    m.step(1);
    const activeMutated = { frame: m.frame, digest: m.digest(), rngState: m.rngState };
    const activeLoaded = d.save.import(activeSaveBytes);
    const activeRestored = {
      frame: m.frame, digest: m.digest(), rngState: m.rngState,
      activePlayers: m.activePlayers(), match: m.match(),
      leaders: Array.from({ length: m.playerCount }, (_, p) => m.leader(p)),
      relations: Array.from({ length: m.playerCount }, (_, other) => m.relation(0, other)),
      saveDisabled: document.getElementById('core-save').disabled,
    };
    const nativeJournal = d.replay.snapshot();
    let nativeJournalExportRefused = false, nativeJournalExportReason = '';
    try { d.replay.export(); } catch (error) {
      nativeJournalExportRefused = true;
      nativeJournalExportReason = error.message;
    }
    d.replay.step();
    const nativeJournalAdvanced = d.replay.snapshot();
    const nativeJournalRestored = await d.replay.seek(activeSaved.frame);
    const activeUrl = d.session.url();
    return JSON.stringify({ activated, leaders, activePlayers, activeMask, match, startedFrame,
      journal, saveDisabled, status, imported, importedMatch, relations, own, teammate,
      beforeFriendlyAttack, afterFriendlyAttack, activeSaveBytes: activeSaveBytes.length,
      activeSaved, activeMutated, activeLoaded, activeRestored, nativeJournal,
      nativeJournalExportRefused, nativeJournalExportReason,
      nativeJournalAdvanced, nativeJournalRestored, activeUrl });
  })()`).then(JSON.parse);
  for (const [name, ok] of [
    ['frame-zero match start reaches every Sim leader and is queried without a JS roster copy',
      out.activation.activated && out.activation.startedFrame === 0 &&
      out.activation.leaders.every(leader => leader.active) &&
      JSON.stringify(out.activation.leaders.map(leader => leader.team)) === JSON.stringify([0, 1, 0, 1]) &&
      out.activation.leaders.every(leader => leader.teamConfigured) &&
      out.activation.activePlayers.length === out.activation.leaders.length &&
      out.activation.activeMask === (1 << out.activation.leaders.length) - 1 &&
      out.activation.match.phase === 'active' && out.activation.match.teamStyle === 1],
    ['journal v3 owns the authoritative roster/teams and reconstructs the active baseline',
      out.activation.journal.protocol === 'don.command-journal.v3' &&
      JSON.stringify(out.activation.journal.setup.activePlayers) ===
        JSON.stringify(out.activation.activePlayers) &&
      JSON.stringify(out.activation.journal.setup.teams) === JSON.stringify([0, 1, 0, 1]) &&
      out.activation.journal.setup.teamStyle === 1 &&
      out.activation.journal.setup.initialDigest.length === 16 &&
      out.activation.imported.frame === 0 && out.activation.importedMatch.phase === 'active' &&
      out.activation.importedMatch.teamStyle === 1 &&
      JSON.stringify(out.activation.importedMatch.activePlayers) ===
        JSON.stringify(out.activation.activePlayers)],
    ['the rebuilt browser module observes active allies and refuses friendly ATTACK ingress',
      out.activation.own >= 0 && out.activation.teammate >= 0 &&
      out.activation.relations[0].id === 2 && out.activation.relations[0].name === 'ally' &&
      out.activation.relations[1].id === 0 && out.activation.relations[1].name === 'war' &&
      out.activation.relations[2].id === 2 && out.activation.relations[2].name === 'ally' &&
      out.activation.afterFriendlyAttack.gaps[12] ===
        out.activation.beforeFriendlyAttack.gaps[12] + 1 &&
      out.activation.afterFriendlyAttack.order === out.activation.beforeFriendlyAttack.order &&
      out.activation.afterFriendlyAttack.transport.ordersApplied ===
        out.activation.beforeFriendlyAttack.transport.ordersApplied],
    ['active roster exposes the v11 live-match save owner instead of a stale setup-only gate',
      !out.activation.saveDisabled && out.activation.status.includes('active PlayerSetup')],
    ['active DoNSave rewinds a later live tick with PlayerSetup, Leaders, Match, and diplomacy intact',
      out.activation.activeSaveBytes > 0 &&
      out.activation.activeSaved.frame > 0 &&
      out.activation.activeMutated.frame === out.activation.activeSaved.frame + 1 &&
      out.activation.activeMutated.digest !== out.activation.activeSaved.digest &&
      out.activation.activeLoaded.frame === out.activation.activeSaved.frame &&
      out.activation.activeRestored.frame === out.activation.activeSaved.frame &&
      out.activation.activeRestored.digest === out.activation.activeSaved.digest &&
      out.activation.activeRestored.rngState === out.activation.activeSaved.rngState &&
      JSON.stringify(out.activation.activeRestored.activePlayers) ===
        JSON.stringify(out.activation.activeSaved.activePlayers) &&
      JSON.stringify(out.activation.activeRestored.leaders) ===
        JSON.stringify(out.activation.activeSaved.leaders) &&
      JSON.stringify(out.activation.activeRestored.relations) ===
        JSON.stringify(out.activation.activeSaved.relations) &&
      out.activation.activeRestored.match.phase === 'active' &&
      !out.activation.activeRestored.saveDisabled],
    ['a loaded DoNSave becomes an exact in-memory seek anchor without fabricating an exportable restart journal',
      out.activation.nativeJournal.baseline === 'DoNSave' &&
      out.activation.nativeJournal.baseFrame === out.activation.activeSaved.frame &&
      out.activation.nativeJournalExportRefused &&
      out.activation.nativeJournalExportReason.includes('in-memory DoNSave baseline') &&
      out.activation.nativeJournalAdvanced.frame === out.activation.activeSaved.frame + 1 &&
      out.activation.nativeJournalRestored.frame === out.activation.activeSaved.frame &&
      out.activation.nativeJournalRestored.digest === out.activation.activeSaved.digest],
  ]) {
    if (!ok) { console.error(`FAIL: ${name}`); bad++; }
  }

  // Reload the canonical URL rather than merely parsing it in the same JavaScript world.
  // This proves the URL carries the roster into a new Sim and that the page queries it back
  // through the Wasm mask before constructing its journal baseline.
  await c.send('Page.navigate', { url: out.activation.activeUrl });
  let sharedUp = false;
  for (let i = 0; i < 100; i++) {
    await sleep(200);
    sharedUp = await c.eval('!!(window.don && window.don.ready && window.don.ready())');
    if (sharedUp) break;
    const bootError = await c.eval('window.don && window.don.bootError');
    if (bootError) throw new Error('shared-session boot failed:\n' + bootError);
  }
  if (!sharedUp) throw new Error('shared-session reload never became ready');
  out.sharedRoster = await c.eval(`(() => {
    const d = window.don, m = d.state.mod;
    const journal = JSON.parse(d.replay.export());
    const beforeRestart = {
      setup: d.session.setup(), activePlayers: m.activePlayers(), match: m.match(),
      urlSlots: new URL(d.session.url()).searchParams.get('slots'),
      saveDisabled: document.getElementById('core-save').disabled,
      saveStatus: document.getElementById('core-save-status').textContent,
      journalSetup: journal.setup,
    };
    d.session.restart('0x1234abcd');
    return JSON.stringify({ beforeRestart, afterRestart: m.activePlayers(), afterRestartMatch: m.match() });
  })()`).then(JSON.parse);
  for (const [name, ok] of [
    ['the shared-session URL reconstructs and queries the exact authoritative roster',
      out.sharedRoster.beforeRestart.setup.phase === 'active' &&
      JSON.stringify(out.sharedRoster.beforeRestart.activePlayers) ===
        JSON.stringify(out.activation.activePlayers) &&
      out.sharedRoster.beforeRestart.urlSlots === out.activation.activePlayers.join(',')],
    ['a shared active roster owns its journal baseline and exposes supported live save export',
      JSON.stringify(out.sharedRoster.beforeRestart.journalSetup.activePlayers) ===
        JSON.stringify(out.activation.activePlayers) &&
      !out.sharedRoster.beforeRestart.saveDisabled &&
      out.sharedRoster.beforeRestart.saveStatus.includes('active PlayerSetup')],
    ['restarting a shared match returns to an authoritative inactive setup',
      out.sharedRoster.afterRestart.length === 0 && out.sharedRoster.afterRestartMatch.phase === 'setup'],
  ]) {
    if (!ok) { console.error(`FAIL: ${name}`); bad++; }
  }

  // The bounded command journal remains a separate exact-packet tool. Prove it reconstructs
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
    const legacy = JSON.parse(journal);
    legacy.protocol = 'don.command-journal.v1';
    delete legacy.setup.activePlayers;
    const legacyImported = await d.replay.import(JSON.stringify(legacy));
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
    const noncanonicalRoster = JSON.parse(journal);
    noncanonicalRoster.setup.activePlayers = [1, 0];
    let noncanonicalRosterRefused = false;
    try { await d.replay.import(JSON.stringify(noncanonicalRoster)); }
    catch { noncanonicalRosterRefused = true; }
    const stableAfterNoncanonicalRoster = { frame: m.frame, digest: m.digest() };
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
      mobile, recorded, advanced, imported, legacyImported, digestAtExport, digestAfterImport,
      malformedRefused, wrongDigestRefused, noncanonicalRosterRefused,
      stableBeforeMalformed, stableAfterMalformed, stableAfterWrongDigest,
      stableAfterNoncanonicalRoster,
      zero, sought, resumed, status, time,
      protocol: parsed.protocol, boundary: parsed.boundary,
      setup: parsed.setup, eventKinds: parsed.events.map(event => event.kind),
      commandHex: parsed.events.find(event => event.kind === 'command')?.hex ?? '',
      headFrame: parsed.headFrame, targetFrame: parsed.frame,
    });
  })()`).then(JSON.parse);
  for (const [name, ok] of [
    ['the journal exports its bounded protocol and deterministic session baseline',
      out.journal.protocol === 'don.command-journal.v3' &&
      out.journal.boundary.includes('not a native save') &&
      out.journal.setup.seed === '0x1234abcd' && out.journal.setup.initialDigest.length === 16 &&
      Array.isArray(out.journal.setup.activePlayers) && out.journal.setup.activePlayers.length === 0],
    ['journal v1 remains an inactive-roster compatibility baseline',
      out.journal.legacyImported.frame === out.journal.recorded.frame &&
      out.journal.legacyImported.digest === out.journal.digestAtExport],
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
    ['a malformed journal, digest, or noncanonical roster is fail-closed',
      out.journal.malformedRefused && out.journal.wrongDigestRefused &&
      out.journal.noncanonicalRosterRefused &&
      JSON.stringify(out.journal.stableAfterMalformed) === JSON.stringify(out.journal.stableBeforeMalformed) &&
      JSON.stringify(out.journal.stableAfterWrongDigest) === JSON.stringify(out.journal.stableBeforeMalformed) &&
      JSON.stringify(out.journal.stableAfterNoncanonicalRoster) ===
        JSON.stringify(out.journal.stableBeforeMalformed)],
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

  // The minimap is an omniscient exported-world navigator, not fog evidence. Exercise panel,
  // pointer, and keyboard navigation while checking authoritative read-only diplomacy/victory
  // facts and the explicitly unavailable countdown.
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
    ['the world snapshot projects victory, score, and diplomacy but refuses countdown/fog claims',
      out.objectives.snapshot.visibility === 'omniscient-export' &&
      out.objectives.snapshot.phase === 'setup' && out.objectives.snapshot.activePlayers.length === 0 &&
      out.objectives.snapshot.victory === 'standard' && out.objectives.snapshot.score === 0 &&
      out.objectives.snapshot.teamScore === 0 && !out.objectives.snapshot.gameOver &&
      out.objectives.snapshot.countdown === 'unavailable' &&
      out.objectives.snapshot.diplomacy === 'effective-core-readonly' &&
      out.objectives.snapshot.owners[0].relation === 'ally' &&
      out.objectives.snapshot.owners.slice(1).every(owner =>
        owner.relation === 'war' && owner.leader.active === false)],
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

  // ---- 2. authoritative-core action boundary -------------------------------------------
  // Move, bounded Library construction, City training, and sequential age research are
  // playable through Sim. Gather, other buildings, and ordinary tech stay fail-closed.
  const script = `(async () => {
    const s = window.don.state, m = s.mod;
    const R = { steps: [] };
    const note = (k, v) => R.steps.push([k, v]);

    // select every citizen the way Ctrl+A does
    window.don.key('KeyA');                       // complete key press cannot latch camera pan
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

    // Barracks remains inspect-only. Library is the one bounded construction cohort because
    // the exported age records name it as their exact WHERE host.
    window.don.select(ids.slice(0, 3));
    document.getElementById('tab-build').click();
    const paletteFilter = document.getElementById('palette-filter');
    paletteFilter.value = 'barracks';
    paletteFilter.dispatchEvent(new Event('input', { bubbles: true }));
    const barracksAction = document.querySelector('#palette [data-kind="build"][data-type-id="427"]');
    note('barracksActionDisabled', !!barracksAction && barracksAction.disabled);
    barracksAction?.click();
    note('barracksPaletteArmed', s.buildType === 427);
    paletteFilter.value = 'library';
    paletteFilter.dispatchEvent(new Event('input', { bubbles: true }));
    const libraryAction = document.querySelector('#palette [data-kind="build"][data-type-id="435"]');
    note('libraryPaletteAction', !!libraryAction && !libraryAction.disabled);
    const libraryCost = s.play.buildings['435']?.cost ?? null;
    const stockBeforeLibrary = m.player(0).stock.slice();
    let librarySite = null;
    for (let radius = 6; radius < 16 && !librarySite; radius++) {
      for (const [tx, ty] of [[stx + radius, sty], [stx, sty + radius], [stx - radius, sty], [stx, sty - radius]]) {
        if (m.placementGrade(0, 435, tx, ty) === 4) { librarySite = [tx, ty]; break; }
      }
    }
    libraryAction?.click();
    note('libraryPaletteArmed', s.buildType === 435);
    if (librarySite && s.buildType === 435) {
      window.don.order.build([
        (librarySite[0] + 2.5) * m.subtile,
        (librarySite[1] + 2.5) * m.subtile,
      ]);
      m.step(1);
    }
    const stockAfterLibrary = m.player(0).stock.slice();
    let library = -1;
    let barracks = -1;
    for (let i = 0; i < m.live; i++) {
      const id = m.idAtRow(i);
      const inf = id >= 0 ? m.info(id) : null;
      if (inf && inf.owner === 0 && inf.typeId === 435) library = id;
      if (inf && inf.typeId === 427) barracks = id;
    }
    note('libraryFoundation', library >= 0 && (m.info(library)?.buildProgress ?? -1) >= 0);
    for (let i = 0; i < 800 && library >= 0 && (m.info(library)?.buildProgress ?? -1) >= 0; i++) {
      m.step(1);
    }
    note('libraryCompleted', library >= 0 && (m.info(library)?.buildProgress ?? 0) < 0);
    note('libraryCost', libraryCost);
    note('stockBeforeLibrary', stockBeforeLibrary);
    note('stockAfterLibrary', stockAfterLibrary);
    note('barracksBuilt', barracks >= 0);

    // The opening Small City is a concrete Sim Build row. Queue two packed WHERE products,
    // cancel/refund the last, and let the remaining entry complete in live production.
    let city = -1;
    for (let i = 0; i < m.live; i++) {
      const id = m.idAtRow(i);
      const inf = id >= 0 ? m.info(id) : null;
      if (inf && inf.owner === 0 && inf.typeId === 414 && inf.buildProgress < 0) {
        city = id; break;
      }
    }
    note('cityFound', city >= 0);
    let trained = 0, trainedType = -1, trainCost = null;
    let stockTrainBefore = null, stockAfterTwo = null, stockAfterCancel = null, stockAfterTrain = null;
    let queueAfterTwo = -1, queueAfterCancel = -1, queueAfterTrain = -1;
    if (city >= 0) {
      window.don.select([city]);
      const prods = m.products(414);
      note('cityProducts', prods.slice());
      document.getElementById('tab-train').click();
      note('paletteFilterAfterTrain', paletteFilter.value);
      const trainAction = document.querySelector('#palette [data-kind="train"]:not(:disabled)');
      const futureTrainDisabled = [...document.querySelectorAll('#palette [data-kind="train"]:disabled')]
        .some(button => button.querySelector('.why')?.textContent.includes('requires age'));
      note('trainPaletteAction', !!trainAction);
      note('futureTrainDisabled', futureTrainDisabled);
      trainedType = Number(trainAction?.dataset.typeId ?? -1);
      trainCost = s.play.units[String(trainedType)]?.cost ?? null;
      const countType = () => {
        let n = 0;
        for (let i = 0; i < m.live; i++) {
          const id = m.idAtRow(i), inf = id >= 0 ? m.info(id) : null;
          if (inf && inf.typeId === trainedType) n++;
        }
        return n;
      };
      const trainedBefore = countType();
      stockTrainBefore = m.player(0).stock.slice();
      trainAction?.click();
      trainAction?.click();
      m.step(1);
      queueAfterTwo = m.info(city)?.queueN ?? -1;
      stockAfterTwo = m.player(0).stock.slice();
      m.unqueue(0, -1);
      m.step(1);
      queueAfterCancel = m.info(city)?.queueN ?? -1;
      stockAfterCancel = m.player(0).stock.slice();
      note('queueFeedback', document.getElementById('palette-feedback').textContent);
      for (let i = 0; i < 800 && (m.info(city)?.queueN ?? 0) > 0; i++) m.step(1);
      queueAfterTrain = m.info(city)?.queueN ?? -1;
      stockAfterTrain = m.player(0).stock.slice();
      trained = countType() - trainedBefore;
    }
    note('trainedType', trainedType); note('trainCost', trainCost);
    note('stockTrainBefore', stockTrainBefore); note('stockAfterTwo', stockAfterTwo);
    note('stockAfterCancel', stockAfterCancel); note('stockAfterTrain', stockAfterTrain);
    note('queueAfterTwo', queueAfterTwo); note('queueAfterCancel', queueAfterCancel);
    note('queueAfterTrain', queueAfterTrain); note('trained', trained);

    // Advance an age through the selected completed Library. Queue/unqueue first proves
    // exact refund, then the second queue completes through the live tech transaction.
    const ageBefore = m.player(0).age;
    if (library >= 0) window.don.select([library]);
    document.getElementById('tab-research').click();
    note('paletteFilterAfterResearch', paletteFilter.value);
    const ageAction = document.querySelector('#palette [data-kind="research"]:not(:disabled)');
    const otherTechDisabled = !!document.querySelector('#palette [data-kind="unavailable"]:disabled');
    const researchCost = s.play.ages[ageBefore]?.cost ?? null;
    const stockBeforeResearch = m.player(0).stock.slice();
    note('researchPaletteAction', !!ageAction);
    note('otherTechDisabled', otherTechDisabled);
    note('researchContext', document.getElementById('palette-context').textContent);
    ageAction?.click();
    m.unqueue(0, -1);
    m.step(1);
    const stockAfterResearchCancel = m.player(0).stock.slice();
    const queueAfterResearchCancel = m.info(library)?.queueN ?? -1;
    if (library >= 0) window.don.select([library]);
    document.getElementById('tab-research').click();
    const retryAgeAction = document.querySelector('#palette [data-kind="research"]:not(:disabled)');
    retryAgeAction?.click();
    m.step(1);
    const stockAfterResearchQueue = m.player(0).stock.slice();
    note('researchStarted', m.player(0).research > 0 || (m.info(library)?.queueN ?? 0) > 0);
    for (let i = 0; i < 900 && m.player(0).age === ageBefore; i++) m.step(1);
    note('researchCost', researchCost);
    note('stockBeforeResearch', stockBeforeResearch);
    note('stockAfterResearchCancel', stockAfterResearchCancel);
    note('stockAfterResearchQueue', stockAfterResearchQueue);
    note('queueAfterResearchCancel', queueAfterResearchCancel);
    note('queueAfterResearch', m.info(library)?.queueN ?? -1);
    note('age', [ageBefore, m.player(0).age]);
    note('gaps', m.gaps());
    note('digest', m.digest());
    return JSON.stringify(R);
  })()`;
  out.script = JSON.parse(await c.eval(script));
  const S = Object.fromEntries(out.script.steps);
  console.log(`core action boundary: ${S.selected} citizens, ${S.gatherOrders} gather orders, ` +
    `workers ${JSON.stringify(S.workers)}`);
  console.log(`  stock ${JSON.stringify(S.stockBefore)} -> ${JSON.stringify(S.stockAfter)}`);
  console.log(`  city=${S.cityFound} products=${JSON.stringify(S.cityProducts)} ` +
    `queues ${S.queueAfterTwo}->${S.queueAfterCancel}->${S.queueAfterTrain} ` +
    `trained=${S.trained} age ${JSON.stringify(S.age)}`);
  const stockMoved = S.stockAfter.some((v, i) => v !== S.stockBefore[i]);
  for (const [name, ok] of [
    ['a citizen was selected', S.selected > 0],
    ['gather is unavailable without a core resource-node command host', S.gatherOrders === 0],
    ['the unsupported economy path does not mutate the core ledger', !stockMoved],
    ['only Library is enabled and its packet creates a concrete completed BuildData row',
      S.barracksActionDisabled === true && S.barracksPaletteArmed === false &&
      S.libraryPaletteAction === true && S.libraryPaletteArmed === true &&
      S.libraryFoundation === true && S.libraryCompleted === true],
    ['Library construction charges its exact packed cost and creates no shadow Barracks',
      S.libraryCost && S.stockAfterLibrary.every((v, i) => v === S.stockBeforeLibrary[i] - S.libraryCost[i]) &&
      S.barracksBuilt === false],
    ['the authoritative opening City exposes packed WHERE products',
      S.cityFound === true && Array.isArray(S.cityProducts) && S.cityProducts.includes(S.trainedType)],
    ['switching catalog families clears the prior building-only search',
      S.paletteFilterAfterTrain === '' && S.paletteFilterAfterResearch === ''],
    ['training action is enabled only through the selected producer', S.trainPaletteAction === true],
    ['two queue packets charge twice, then cancel refunds exactly one packed cost',
      S.queueAfterTwo === 2 && [0, 1].includes(S.queueAfterCancel) && S.trainCost &&
      S.stockAfterTwo.every((v, i) => v === S.stockTrainBefore[i] - 2 * S.trainCost[i]) &&
      S.stockAfterCancel.every((v, i) => v === S.stockTrainBefore[i] - S.trainCost[i])],
    ['the live production runtime completes exactly one unit and drains the queue',
      S.trained === 1 && S.queueAfterTrain === 0 &&
      S.stockAfterTrain.every((v, i) => v === S.stockAfterCancel[i])],
    ['age research is enabled only on Library and cancel refunds its exact packed cost',
      S.researchPaletteAction === true && S.otherTechDisabled === true &&
      S.queueAfterResearchCancel === 0 && S.researchCost &&
      S.stockAfterResearchCancel.every((v, i) => v === S.stockBeforeResearch[i])],
    ['the live tech runtime charges, completes, drains, and advances authoritative age',
      S.researchStarted === true && S.age[1] === S.age[0] + 1 && S.queueAfterResearch === 0 &&
      S.stockAfterResearchQueue.every((v, i) => v === S.stockBeforeResearch[i] - S.researchCost[i])],
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
    ['the owner snapshot includes the authoritative Library and trained unit',
      out.objectivesAfterPlay.snapshot.owners[0].objects === out.objectives.snapshot.owners[0].objects + 2 &&
      out.objectivesAfterPlay.playerZeroText.includes(
        `${out.objectivesAfterPlay.snapshot.owners[0].objects} objects`)],
    ['post-play owner totals still equal the live world',
      out.objectivesAfterPlay.snapshot.owners.reduce((n, owner) => n + owner.objects, 0) ===
        out.objectivesAfterPlay.live],
    ['object growth is never relabelled as victory score',
      out.objectivesAfterPlay.snapshot.score === out.objectives.snapshot.score &&
      out.objectivesAfterPlay.scoreText.includes(
        `victory ${out.objectivesAfterPlay.snapshot.score}`)],
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
      // Earlier real-pointer layout checks can leave edge scroll armed at their last CDP
      // coordinate. Freeze that input state before deriving the coordinate we will click.
      window.don.state.edge = null;
      window.don.centreOn(v.x[i], v.y[i]);
      const s = window.don.worldToScreen(v.x[i], v.y[i]);
      const r = document.getElementById('gl').getBoundingClientRect();
      const sx = r.left + s[0], sy = r.top + s[1];
      const hit = document.elementFromPoint(sx, sy);
      return JSON.stringify({
        id: m.pickAt(v.x[i], v.y[i]), wx: v.x[i], wy: v.y[i], sx, sy,
        canvasRect: [r.left, r.top, r.right, r.bottom],
        hitId: hit?.id ?? '', hitTag: hit?.tagName ?? '',
      });
    }
    return null;
  })()`).then((s) => (s ? JSON.parse(s) : null));
  if (!target) { console.error('FAIL: no unit to click'); bad++; }
  else {
    await c.eval('window.don.select([])');
    // Deliver input against a presented camera/projection boundary.
    await c.eval('new Promise(resolve => requestAnimationFrame(() => resolve()))');
    // Observe delivery at the real canvas as well as the gameplay result. Registering the
    // observer after the WebGPU benchmark also refreshes headless Chrome's compositor hit
    // region before CDP sends its mouse event.
    await c.eval(`(() => {
      window.__donSmokePointerEvents = [];
      const canvas = document.getElementById('gl');
      for (const type of ['pointerdown', 'pointerup']) {
        canvas.addEventListener(type, event => {
          const world = window.don.screenToWorld(event.clientX, event.clientY);
          window.__donSmokePointerEvents.push({
            type, button: event.button, x: event.clientX, y: event.clientY,
            world, picked: window.don.state.mod.pickAt(world[0], world[1]),
            dragging: !!window.don.state.drag, selection: window.don.state.selection.slice(),
          });
        }, { once: true, passive: true });
      }
    })()`);
    for (const type of ['mousePressed', 'mouseReleased']) {
      await c.send('Input.dispatchMouseEvent', {
        type, x: Math.round(target.sx), y: Math.round(target.sy),
        button: 'left', buttons: type === 'mousePressed' ? 1 : 0, clickCount: 1,
      });
    }
    await sleep(120);
    const selN = await c.eval('window.don.state.selection.length');
    const pointerEvents = await c.eval('window.__donSmokePointerEvents');
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
      clickedId: target.id, target, selectedAfterClick: selN, orderAfterRightClick: ordered,
      pointerEvents,
      dockArmed, dockModeCleared: dockResult.modeCleared, orderAfterDockMove: dockResult.order,
    };
    console.log(`real mouse: click selected ${selN}, right click set order ${ordered} ` +
      `(1 = MOVE_TO)`);
    if (pointerEvents.map(event => event.type).join(',') !== 'pointerdown,pointerup') {
      console.error('FAIL: the canvas did not receive the real pointer event pair'); bad++;
    }
    if (selN !== 1) { console.error('FAIL: a real click did not select the unit'); bad++; }
    if (ordered !== 1 && ordered !== 3) {
      console.error('FAIL: a real right click produced no order'); bad++;
    }
    if (!dockArmed || !dockResult.modeCleared || (dockResult.order !== 1 && dockResult.order !== 3)) {
      console.error('FAIL: command dock did not arm, issue, and clear a move target'); bad++;
    }
  }

  // ---- 4c. two browser seats consume MatchStart and native Group→Move turns ------------
  //
  // The local gateway invokes the existing Rust two-process lifecycle. It exposes no seed,
  // epoch, or roster until both native peers agree. They then submit canonical singleton
  // Group→Move packages; neither paused Sim advances until both native peers expose an identical
  // package set. Thirty-two barriers prove the receipt-bearing orders produce actual motion.
  if (LOCAL_MATCH) {
    const cleanSecondUrl = `http://127.0.0.1:${PORT}/play.html?seed=0x2468ace0&player=1`;
    const created = await c.send('Target.createTarget', { url: cleanSecondUrl });
    const secondTargetId = created.result?.targetId;
    if (!secondTargetId) throw new Error('Chrome did not create the second local-match tab');
    let secondTarget = null;
    for (let attempt = 0; attempt < 50; attempt++) {
      const listed = await (await fetch(`http://127.0.0.1:${CDP}/json/list`)).json();
      secondTarget = listed.find((target) => target.id === secondTargetId);
      if (secondTarget?.webSocketDebuggerUrl) break;
      await sleep(100);
    }
    if (!secondTarget?.webSocketDebuggerUrl) throw new Error('second local-match tab has no CDP target');
    c2 = await Cdp.open(secondTarget.webSocketDebuggerUrl);
    await c2.send('Runtime.enable');
    await c2.send('Log.enable');
    await c2.send('Page.enable');
    for (let attempt = 0; attempt < 100; attempt++) {
      const bothReady = await Promise.all([
        c.eval('!!(window.don?.ready?.() && window.don.localMatch.snapshot().available)'),
        c2.eval('!!(window.don?.ready?.() && window.don.localMatch.snapshot().available)'),
      ]);
      if (bothReady.every(Boolean)) break;
      if (attempt === 99) throw new Error('two local-match browser clients never became ready');
      await sleep(100);
    }

    await c.eval(`document.getElementById('session-seed').value = '0x2468ace0'`);
    const hostLobby = await c.eval(
      `window.don.localMatch.create('Host browser').then(JSON.stringify)`).then(JSON.parse);
    await c2.eval(
      `window.don.localMatch.join('${hostLobby.code}', 'Peer browser').then(JSON.stringify)`);
    await c.eval(`window.don.localMatch.ready().then(JSON.stringify)`);
    await c2.eval(`window.don.localMatch.ready().then(JSON.stringify)`);

    for (let attempt = 0; attempt < 600; attempt++) {
      const phases = await Promise.all([
        c.eval('window.don.localMatch.snapshot().phase'),
        c2.eval('window.don.localMatch.snapshot().phase'),
      ]);
      const applied = await Promise.all([
        c.eval('window.don.localMatch.snapshot().applied'),
        c2.eval('window.don.localMatch.snapshot().applied'),
      ]);
      if (phases.every((phase) => phase === 'started') && applied.every(Boolean)) break;
      if (phases.some((phase) => phase === 'failed')) {
        const failures = await Promise.all([
          c.eval('window.don.localMatch.snapshot().error'),
          c2.eval('window.don.localMatch.snapshot().error'),
        ]);
        throw new Error(`local MatchStart failed: ${failures.join(' | ')}`);
      }
      if (attempt === 599) throw new Error(`local MatchStart did not reach both tabs`);
      await sleep(100);
    }

    const frameZero = await Promise.all([c, c2].map((client) => client.eval(`(() => {
      const d = window.don, m = d.state.mod, before = m.transport();
      const refused = m.halt(d.state.who);
      const step = d.replay.step();
      const units = [0, 1].map((who) => {
        for (let row = 0; row < m.live; row++) {
          const rendererId = m.idAtRow(row);
          try {
            const identity = m.commandIdentity(rendererId);
            if (identity.who !== who) continue;
            const info = m.info(rendererId);
            return { rendererId, who, o: identity.o, uid: identity.uid, x: info.x, y: info.y };
          } catch { /* renderer row has no authoritative owner-local Unit identity */ }
        }
        throw new Error('missing authoritative command Unit for P' + who);
      });
      return JSON.stringify({
        frame: m.frame, digest: m.digest(), rngState: m.rngState,
        units,
        gate: { refused: refused === null, before, after: m.transport(), stepFrame: step.frame },
      });
    })()`).then(JSON.parse)));
    for (let expectedFrame = 1; expectedFrame <= 32; expectedFrame++) {
      await Promise.all([
        c.eval('window.don.localMatch.turn().then(JSON.stringify)'),
        c2.eval('window.don.localMatch.turn().then(JSON.stringify)'),
      ]);
      for (let attempt = 0; attempt < 600; attempt++) {
        const facts = await Promise.all([c, c2].map((client) => client.eval(`(() => {
          const d = window.don, local = d.localMatch.snapshot();
          return JSON.stringify({
            phase: local.phase, error: local.error, frame: d.state.mod.frame,
            confirmed: local.lastConfirmed?.stamp ?? -1, next: local.turn?.stamp ?? -1,
          });
        })()`).then(JSON.parse)));
        if (facts.every((fact) => fact.phase === 'started' && fact.frame === expectedFrame &&
            fact.confirmed === expectedFrame - 1 && fact.next === expectedFrame)) break;
        if (facts.some((fact) => fact.phase === 'failed')) {
          throw new Error(`local turn barrier failed: ${facts.map((fact) => fact.error).join(' | ')}`);
        }
        if (attempt === 599) {
          throw new Error(`local turn barrier did not confirm frame ${expectedFrame} in both tabs`);
        }
        await sleep(100);
      }
    }

    const clientEvidence = async (client) => client.eval(`(() => {
      const d = window.don, local = d.localMatch.snapshot(), mod = d.state.mod;
      const beforeResume = { frame: mod.frame, digest: mod.digest(), paused: d.state.paused };
      d.replay.play();
      const units = [0, 1].map((who) => {
        for (let row = 0; row < mod.live; row++) {
          const rendererId = mod.idAtRow(row);
          try {
            const identity = mod.commandIdentity(rendererId);
            if (identity.who !== who) continue;
            const info = mod.info(rendererId);
            return { rendererId, who, o: identity.o, uid: identity.uid, x: info.x, y: info.y };
          } catch { /* renderer row has no authoritative owner-local Unit identity */ }
        }
        throw new Error('missing authoritative command Unit for P' + who);
      });
      return JSON.stringify({
        local,
        setup: d.session.setup(),
        frame: mod.frame,
        digest: mod.digest(),
        rngState: mod.rngState,
        activePlayers: mod.activePlayers(),
        leaders: [mod.leader(0), mod.leader(1)],
        match: mod.match(),
        units,
        commands: d.commands.snapshot(),
        paused: d.state.paused,
        beforeResume,
        status: document.getElementById('local-match-status').textContent,
      });
    })()`).then(JSON.parse);
    const [hostBrowser, peerBrowser] = await Promise.all([
      clientEvidence(c), clientEvidence(c2),
    ]);
    out.localMatch = { frameZero, host: hostBrowser, peer: peerBrowser };
    for (const [name, ok] of [
      ['native StartGame and MatchStart produce one exact seed/epoch/reference in both tabs',
        hostBrowser.local.handoff.seed === 0x2468ace0 &&
        hostBrowser.local.handoff.seed === peerBrowser.local.handoff.seed &&
        hostBrowser.local.handoff.epoch === peerBrowser.local.handoff.epoch &&
        hostBrowser.local.handoff.sessionReference === peerBrowser.local.handoff.sessionReference],
      ['both browser clients reconstruct the same authoritative two-player frame-zero setup',
        frameZero[0].frame === 0 && frameZero[1].frame === 0 &&
        frameZero[0].digest === frameZero[1].digest &&
        frameZero[0].rngState === frameZero[1].rngState &&
        JSON.stringify(hostBrowser.activePlayers) === JSON.stringify([0, 1]) &&
        JSON.stringify(peerBrowser.activePlayers) === JSON.stringify([0, 1]) &&
        JSON.stringify(hostBrowser.setup.teams) === JSON.stringify(peerBrowser.setup.teams) &&
        JSON.stringify(hostBrowser.leaders) === JSON.stringify(peerBrowser.leaders) &&
        JSON.stringify(hostBrowser.match) === JSON.stringify(peerBrowser.match)],
      ['browser perspective is seat-specific without becoming a second roster authority',
        hostBrowser.setup.player === 0 && peerBrowser.setup.player === 1 &&
        hostBrowser.setup.slots === 2 && peerBrowser.setup.slots === 2],
      ['pause lock refuses direct command submission and local replay stepping before agreement',
        frameZero.every((fact) => fact.gate.refused && fact.gate.stepFrame === 0 &&
          JSON.stringify(fact.gate.before) === JSON.stringify(fact.gate.after))],
      ['strict native Group→Move barriers preserve receipts and produce equal actual motion',
        hostBrowser.frame === 32 && peerBrowser.frame === 32 &&
        hostBrowser.digest === peerBrowser.digest && hostBrowser.rngState === peerBrowser.rngState &&
        hostBrowser.local.lastConfirmed.stamp === 31 && peerBrowser.local.lastConfirmed.stamp === 31 &&
        hostBrowser.local.lastConfirmed.hash === peerBrowser.local.lastConfirmed.hash &&
        hostBrowser.local.turn.stamp === 32 && peerBrowser.local.turn.stamp === 32 &&
        hostBrowser.local.turnRelay === 'canonical-group-move-v1' &&
        peerBrowser.local.turnRelay === 'canonical-group-move-v1' &&
        JSON.stringify(hostBrowser.local.lastReceipts) ===
          JSON.stringify(peerBrowser.local.lastReceipts) &&
        hostBrowser.local.lastReceipts.length === 2 &&
        JSON.stringify(hostBrowser.units) === JSON.stringify(peerBrowser.units) &&
        hostBrowser.units.every((unit, index) =>
          unit.x !== frameZero[0].units[index].x || unit.y !== frameZero[0].units[index].y)],
      ['both clients remain paused and refuse free-running resume after the agreed turn',
        hostBrowser.paused && peerBrowser.paused &&
        hostBrowser.beforeResume.paused && peerBrowser.beforeResume.paused &&
        hostBrowser.status.includes('turn 32 waiting for both seats') &&
        peerBrowser.status.includes('turn 32 waiting for both seats')],
    ]) {
      if (!ok) { console.error(`FAIL: ${name}`); bad++; }
    }
  }

  // ---- 5. cross-target determinism -----------------------------------------------------
  out.freshDigest = await c.eval(
    'JSON.stringify(window.don.freshDigest(0xc0ffee, 600, []))').then(JSON.parse);
  out.freshActiveDigest = await c.eval(
    'JSON.stringify(window.don.freshDigest(0xc0ffee, 600, [0, 1, 2, 3]))').then(JSON.parse);
  console.log(`wasm fresh inactive setup: seed 0xc0ffee 600 frames -> ` +
    `${out.freshDigest.live} live, digest ${out.freshDigest.digest}`);
  console.log('  compare with: cargo run --manifest-path web/wasm/Cargo.toml --release --bin ' +
    'playcheck -- digest web/public/data/gamedata.bin web/public/data/playdata.bin c0ffee 600 -');
  console.log(`wasm fresh active roster 0,1,2,3: seed 0xc0ffee 600 frames -> ` +
    `${out.freshActiveDigest.live} live, digest ${out.freshActiveDigest.digest}`);
  console.log('  compare with: cargo run --manifest-path web/wasm/Cargo.toml --release --bin ' +
    'playcheck -- digest web/public/data/gamedata.bin web/public/data/playdata.bin ' +
    'c0ffee 600 0,1,2,3');
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
  const expectActive = flag('--expect-active-digest', null);
  if (expectActive) {
    out.expectActiveDigest = expectActive;
    if (expectActive !== out.freshActiveDigest.digest) {
      console.error(`FAIL: active-roster wasm digest ${out.freshActiveDigest.digest} ` +
        `!= native ${expectActive}`);
      bad++;
    } else {
      console.log(`  active-roster wasm32 and native agree bit for bit: ${expectActive}`);
    }
  }

  out.consoleErrors = c.events
    .filter((e) => e.method === 'Log.entryAdded' && e.params.entry.level === 'error')
    .map((e) => e.params.entry.text)
    .concat(c.events.filter((e) => e.method === 'Runtime.exceptionThrown')
      .map((e) => e.params.exceptionDetails?.exception?.description ?? 'exception'))
    .concat((c2?.events ?? [])
      .filter((e) => e.method === 'Log.entryAdded' && e.params.entry.level === 'error')
      .map((e) => `second tab: ${e.params.entry.text}`))
    .concat((c2?.events ?? []).filter((e) => e.method === 'Runtime.exceptionThrown')
      .map((e) => `second tab: ${e.params.exceptionDetails?.exception?.description ?? 'exception'}`));
  if (out.consoleErrors.length) {
    console.error('page errors: ' + out.consoleErrors.slice(0, 3).join(' | '));
    bad++;
  }
} catch (e) {
  console.error('smoke failed: ' + e.message);
  out.error = e.message;
  bad++;
} finally {
  c2?.close();
  c?.close();
  if (!KEEP && proc) await stopChrome(proc);
  if (!KEEP) await rm(profile, { recursive: true, force: true });
}

if (OUT) await writeFile(OUT, JSON.stringify(out, null, 1));
console.log(bad ? `\n${bad} check(s) failed` : '\nall checks passed');
process.exitCode = bad ? 1 : 0;
