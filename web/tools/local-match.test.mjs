import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

import {
  LOCAL_MATCH_PROTOCOL, LOCAL_MATCH_TURN_RELAY, LocalMatchGateway, runServiceMatch,
  startServiceMatchRelay, validateEmptyTurnRequest,
} from '../local-match.mjs';

const FIXTURE = fileURLToPath(new URL('./fixtures/fake-service-match-peer.mjs', import.meta.url));
const fixtureSpawn = (_command, args, options) => spawn(process.execPath, [FIXTURE, ...args], options);

test('native lifecycle facts are admitted only after both peers agree', async () => {
  const handoff = await runServiceMatch('/unused/service-match-peer', 0x89abcdef, {
    spawn: fixtureSpawn,
    timeoutMs: 2_000,
  });
  assert.equal(handoff.protocol, LOCAL_MATCH_PROTOCOL);
  assert.equal(handoff.seed, 0x89abcdef);
  assert.equal(handoff.epoch, 305419896);
  assert.deepEqual(handoff.activePlayers, [0, 1]);
  assert.deepEqual(handoff.teams, [0, 1, 8, 8]);
  assert.equal(handoff.turnRelay, 'unavailable');
});

test('empty turn request schema refuses every browser command field', () => {
  assert.deepEqual(validateEmptyTurnRequest({ token: 'seat', stamp: 0 }), {
    token: 'seat', stamp: 0,
  });
  for (const body of [
    { token: 'seat', stamp: 0, commands: [] },
    { token: 'seat', stamp: 0, payload: '' },
    { token: 'seat', stamp: 0, bytes: '00' },
  ]) {
    assert.throws(() => validateEmptyTurnRequest(body), /refuses browser command input/);
  }
});

test('long-lived native peers admit only identical ordered empty turn packages', async () => {
  const started = await startServiceMatchRelay('/unused/service-match-peer', 0x89abcdef, {
    spawn: fixtureSpawn,
    timeoutMs: 2_000,
  });
  try {
    assert.equal(started.handoff.turnRelay, LOCAL_MATCH_TURN_RELAY);
    const turn = await started.relay.completeTurn(0);
    assert.equal(turn.stamp, 0);
    assert.deepEqual(turn.packages.map(({ play, payload }) => ({ play, payload })), [
      { play: 0, payload: '444f4e420100' },
      { play: 1, payload: '444f4e420100' },
    ]);
    await assert.rejects(() => started.relay.completeTurn(0), /expected stamp 1/);
  } finally {
    started.relay.close();
  }
});

test('two browser seats cannot see a handoff until both are ready', async () => {
  const gateway = new LocalMatchGateway('/usr/bin/true', {
    spawn: fixtureSpawn,
    timeoutMs: 2_000,
  });
  const host = gateway.create(0x89abcdef, 'Host browser');
  const join = gateway.join(host.lobby.code, 'Join browser');
  assert.equal(host.lobby.seat, 0);
  assert.equal(join.lobby.seat, 1);
  assert.equal(host.lobby.seed, null);
  assert.equal(join.lobby.seed, null);
  assert.equal(gateway.ready(host.lobby.code, host.token).phase, 'lobby');
  assert.equal(gateway.snapshot(host.lobby.code, host.token).handoff, null);
  assert.equal(gateway.ready(host.lobby.code, join.token).phase, 'starting');

  let hostView;
  for (let attempt = 0; attempt < 100; attempt++) {
    hostView = gateway.snapshot(host.lobby.code, host.token);
    if (hostView.phase !== 'starting') break;
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  const joinView = gateway.snapshot(host.lobby.code, join.token);
  assert.equal(hostView.phase, 'started');
  assert.equal(joinView.phase, 'started');
  assert.equal(hostView.handoff.player, 0);
  assert.equal(joinView.handoff.player, 1);
  assert.equal(hostView.handoff.epoch, joinView.handoff.epoch);
  assert.equal(hostView.handoff.seed, joinView.handoff.seed);
  assert.equal(hostView.turnRelay, LOCAL_MATCH_TURN_RELAY);

  assert.equal(gateway.submitTurn(host.lobby.code, host.token, 0).turn.phase, 'waiting');
  assert.equal(gateway.submitTurn(host.lobby.code, join.token, 0).turn.phase, 'agreeing');
  for (let attempt = 0; attempt < 100; attempt++) {
    hostView = gateway.snapshot(host.lobby.code, host.token);
    if (hostView.turn.phase !== 'agreeing') break;
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  assert.equal(hostView.turn.phase, 'agreed');
  assert.equal(hostView.turn.agreement.packages.length, 2);
  assert.equal(gateway.acknowledgeTurn(
    host.lobby.code, host.token, 0, 1, '1111222233334444', 99).turn.stamp, 0);
  const advanced = gateway.acknowledgeTurn(
    host.lobby.code, join.token, 0, 1, '1111222233334444', 99);
  assert.equal(advanced.turn.stamp, 1);
  assert.equal(advanced.lastConfirmed.stamp, 0);
  gateway.leave(host.lobby.code, host.token);
});

test('tokens, roster bounds, and unavailable peers fail closed', async () => {
  const unavailable = new LocalMatchGateway('/definitely/absent/service-match-peer');
  assert.equal(await unavailable.available(), false);

  const gateway = new LocalMatchGateway('/usr/bin/true', { spawn: fixtureSpawn });
  const host = gateway.create(7, 'Host');
  gateway.join(host.lobby.code, 'Peer');
  assert.throws(() => gateway.join(host.lobby.code, 'Third'), /not joinable/);
  assert.throws(() => gateway.snapshot(host.lobby.code, 'wrong-token'), /token is invalid/);
  assert.throws(() => gateway.create(-1, 'Bad seed'), /u32/);
  assert.throws(() => gateway.join('NOT-CODE', 'Peer'), /eight lowercase hexadecimal/);
  gateway.leave(host.lobby.code, host.token);
});

test('browser state disagreement fails closed and closes the relay', async () => {
  const gateway = new LocalMatchGateway('/usr/bin/true', {
    spawn: fixtureSpawn, timeoutMs: 2_000,
  });
  const host = gateway.create(0x89abcdef, 'Host');
  const join = gateway.join(host.lobby.code, 'Peer');
  gateway.ready(host.lobby.code, host.token);
  gateway.ready(host.lobby.code, join.token);
  for (let attempt = 0; attempt < 100; attempt++) {
    if (gateway.snapshot(host.lobby.code, host.token).phase === 'started') break;
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  gateway.submitTurn(host.lobby.code, host.token, 0);
  gateway.submitTurn(host.lobby.code, join.token, 0);
  for (let attempt = 0; attempt < 100; attempt++) {
    if (gateway.snapshot(host.lobby.code, host.token).turn?.phase === 'agreed') break;
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  gateway.acknowledgeTurn(host.lobby.code, host.token, 0, 1, '1111222233334444', 99);
  assert.throws(() => gateway.acknowledgeTurn(
    host.lobby.code, join.token, 0, 1, '9999222233334444', 99), /disagreed/);
  const failed = gateway.snapshot(host.lobby.code, host.token);
  assert.equal(failed.phase, 'failed');
  assert.match(failed.error, /disagreed/);
  gateway.leave(host.lobby.code, host.token);
});

test('peer disagreement and stalled processes expose no handoff', async () => {
  await assert.rejects(
    runServiceMatch('/definitely/absent/service-match-peer', 7, { timeoutMs: 200 }),
    /host peer launch failed/,
  );

  const mismatchSpawn = (_command, args, options) => spawn(
    process.execPath, [FIXTURE, ...args], {
      ...options,
      env: { ...process.env, DON_FAKE_MATCHSTART_MISMATCH: '1' },
    });
  await assert.rejects(
    runServiceMatch('/unused/service-match-peer', 0x89abcdef, {
      spawn: mismatchSpawn, timeoutMs: 2_000,
    }),
    /disagree on confirmed MatchStart seed/,
  );

  const turnMismatchSpawn = (_command, args, options) => spawn(
    process.execPath, [FIXTURE, ...args], {
      ...options,
      env: { ...process.env, DON_FAKE_TURN_MISMATCH: '1' },
    });
  const mismatched = await startServiceMatchRelay('/unused/service-match-peer', 0x89abcdef, {
    spawn: turnMismatchSpawn, timeoutMs: 2_000,
  });
  await assert.rejects(() => mismatched.relay.completeTurn(0), /disagreed on ordered package set/);
  mismatched.relay.close();

  const stalledSpawn = (_command, _args, options) => spawn(
    process.execPath, ['-e', 'setInterval(() => {}, 1000)'], options);
  await assert.rejects(
    runServiceMatch('/unused/service-match-peer', 7, {
      spawn: stalledSpawn, timeoutMs: 50,
    }),
    /exceeded 50 ms/,
  );
});
