import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

import {
  LOCAL_MATCH_PROTOCOL, LocalMatchGateway, runServiceMatch,
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

  const stalledSpawn = (_command, _args, options) => spawn(
    process.execPath, ['-e', 'setInterval(() => {}, 1000)'], options);
  await assert.rejects(
    runServiceMatch('/unused/service-match-peer', 7, {
      spawn: stalledSpawn, timeoutMs: 50,
    }),
    /exceeded 50 ms/,
  );
});
