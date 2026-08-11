// Local-only browser handoff for the real don-crossplay ServiceMatch lifecycle.
//
// This module deliberately stops at the confirmed MatchStart boundary. The two browser
// clients receive an exact seed/epoch/roster only after the configured native peer has
// proved Create/Find/Join/ready/StartGame -> MatchStart agreement. Browser turn relay is
// not attached yet, so the playable clients remain paused at the identical frame-zero
// boundary instead of drifting and pretending to be synchronized.

import { spawn as spawnChild } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { constants as fsConstants } from 'node:fs';
import { access } from 'node:fs/promises';

export const LOCAL_MATCH_PROTOCOL = 'don.local-match-handoff.v1';
export const LOCAL_MATCH_PLAYERS = 2;
export const MAX_LOCAL_MATCH_LOBBIES = 16;
export const MAX_LOCAL_MATCH_BODY_BYTES = 16 * 1024;
export const MAX_LOCAL_MATCH_OUTPUT_BYTES = 256 * 1024;
export const DEFAULT_LOCAL_MATCH_TIMEOUT_MS = 25_000;

function boundedName(value) {
  if (typeof value !== 'string') throw new Error('player name must be text');
  const name = value.trim();
  if (!name || name.length > 40) throw new Error('player name must contain 1..40 characters');
  return name;
}

function boundedSeed(value) {
  if (!Number.isInteger(value) || value < 0 || value > 0xffff_ffff) {
    throw new Error('game seed must be a u32');
  }
  return value >>> 0;
}

function exactCode(value) {
  if (typeof value !== 'string' || !/^[a-f0-9]{8}$/.test(value)) {
    throw new Error('lobby code must be eight lowercase hexadecimal characters');
  }
  return value;
}

function processLine(line) {
  try { return JSON.parse(line); }
  catch { return null; }
}

function lifecycleEvidence(output, side) {
  const lines = output.split(/\r?\n/);
  const events = lines.map(processLine).filter(Boolean);
  const directoryAt = events.findIndex((entry) => entry.event === 'directory_started');
  const matchAt = events.findIndex((entry) => entry.event === 'match_confirmed');
  const turnAt = events.findIndex((entry) => entry.event === 'turn');
  const done = events.find((entry) => entry.event === 'done');
  const confirmed = events[matchAt];
  if (directoryAt < 0 || matchAt <= directoryAt || turnAt <= matchAt || !done || !confirmed) {
    throw new Error(`${side} peer did not complete directory -> MatchStart -> turn in order`);
  }
  return { confirmed, done, eventCount: events.length };
}

function appendBounded(current, chunk, side) {
  const next = current + chunk;
  if (Buffer.byteLength(next) > MAX_LOCAL_MATCH_OUTPUT_BYTES) {
    throw new Error(`${side} peer exceeded the ${MAX_LOCAL_MATCH_OUTPUT_BYTES}-byte output bound`);
  }
  return next;
}

function serviceLine(text) {
  for (const line of text.split(/\r?\n/)) {
    const fields = line.trim().split(/\s+/);
    if (fields.length === 3 && fields[0] === 'SERVICE') {
      if (!/^127\.0\.0\.1:\d+$/.test(fields[1])) {
        throw new Error(`peer published a non-loopback directory endpoint ${fields[1]}`);
      }
      if (!fields[2] || fields[2].length > 128) throw new Error('peer published an invalid lobby id');
      return { directory: fields[1], lobbyId: fields[2] };
    }
  }
  return null;
}

function waitForExit(child, side) {
  return new Promise((resolve, reject) => {
    child.once('error', (error) => reject(new Error(`${side} peer launch failed: ${error.message}`)));
    child.once('exit', (code, signal) => resolve({ code, signal }));
  });
}

/** Run the existing two-process Rust product seam and return only verified MatchStart facts. */
export async function runServiceMatch(peerPath, seed, options = {}) {
  const spawn = options.spawn ?? spawnChild;
  const timeoutMs = options.timeoutMs ?? DEFAULT_LOCAL_MATCH_TIMEOUT_MS;
  seed = boundedSeed(seed);

  let host;
  let client;
  let hostOut = '';
  let hostErr = '';
  let clientOut = '';
  let clientErr = '';
  let outputError = null;
  let serviceResolve;
  let serviceReject;
  const serviceReady = new Promise((resolve, reject) => {
    serviceResolve = resolve;
    serviceReject = reject;
  });
  let publishedService = false;

  try {
    host = spawn(peerPath, ['host', '--seed', String(seed)], {
      stdio: ['ignore', 'pipe', 'pipe'],
    });
  } catch (error) {
    throw new Error(`host peer launch failed: ${error.message}`);
  }
  host.stdout.setEncoding('utf8');
  host.stderr.setEncoding('utf8');
  host.once('error', (error) => {
    serviceReject(new Error(`host peer launch failed: ${error.message}`));
  });
  host.stdout.on('data', (chunk) => {
    try {
      hostOut = appendBounded(hostOut, chunk, 'host');
      const service = serviceLine(hostOut);
      if (service && !publishedService) {
        publishedService = true;
        serviceResolve(service);
      }
    } catch (error) {
      outputError = error;
      host.kill('SIGTERM');
      serviceReject(error);
    }
  });
  host.stderr.on('data', (chunk) => {
    try { hostErr = appendBounded(hostErr, chunk, 'host stderr'); }
    catch (error) { outputError = error; host.kill('SIGTERM'); }
  });

  const hostExit = waitForExit(host, 'host');
  hostExit.then(
    ({ code, signal }) => {
      if (!publishedService) {
        serviceReject(new Error(
          `host peer exited before SERVICE (code ${code}, signal ${signal ?? 'none'}): ${hostErr.trim()}`));
      }
    },
    (error) => serviceReject(error),
  );

  const timer = setTimeout(() => {
    outputError = new Error(`local match lifecycle exceeded ${timeoutMs} ms`);
    host?.kill('SIGTERM');
    client?.kill('SIGTERM');
    serviceReject(outputError);
  }, timeoutMs);

  try {
    const service = await serviceReady;
    client = spawn(peerPath, ['join', service.directory, service.lobbyId], {
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    client.stdout.setEncoding('utf8');
    client.stderr.setEncoding('utf8');
    client.stdout.on('data', (chunk) => {
      try { clientOut = appendBounded(clientOut, chunk, 'client'); }
      catch (error) { outputError = error; client.kill('SIGTERM'); }
    });
    client.stderr.on('data', (chunk) => {
      try { clientErr = appendBounded(clientErr, chunk, 'client stderr'); }
      catch (error) { outputError = error; client.kill('SIGTERM'); }
    });
    const [hostStatus, clientStatus] = await Promise.all([
      hostExit, waitForExit(client, 'client'),
    ]);
    if (outputError) throw outputError;
    if (hostStatus.code !== 0 || clientStatus.code !== 0) {
      throw new Error(
        `local match peer failure: host=${hostStatus.code}/${hostStatus.signal ?? 'none'} ` +
        `client=${clientStatus.code}/${clientStatus.signal ?? 'none'} ` +
        `host stderr=${hostErr.trim()} client stderr=${clientErr.trim()}`);
    }

    const hostEvidence = lifecycleEvidence(hostOut, 'host');
    const clientEvidence = lifecycleEvidence(clientOut, 'client');
    const expected = hostEvidence.confirmed;
    const observed = clientEvidence.confirmed;
    for (const field of ['lobby', 'reference', 'epoch', 'seed']) {
      if (expected[field] !== observed[field]) {
        throw new Error(`peers disagree on confirmed MatchStart ${field}`);
      }
    }
    if (expected.lobby !== service.lobbyId || expected.reference !== service.lobbyId) {
      throw new Error('confirmed MatchStart is not bound to the completed StartGame reference');
    }
    if (expected.seed !== seed || !Number.isInteger(expected.epoch) || expected.epoch <= 0) {
      throw new Error('confirmed MatchStart has an invalid seed or epoch');
    }
    if (hostEvidence.done.hash !== clientEvidence.done.hash || !hostEvidence.done.hash) {
      throw new Error('native peers disagreed on their post-MatchStart turn evidence');
    }

    return Object.freeze({
      protocol: LOCAL_MATCH_PROTOCOL,
      lobbyId: expected.lobby,
      sessionReference: expected.reference,
      epoch: expected.epoch >>> 0,
      seed,
      activePlayers: Object.freeze([0, 1]),
      teams: Object.freeze([0, 1, 8, 8]),
      teamStyle: 0,
      nativeTurnHash: hostEvidence.done.hash,
      nativeEvents: Object.freeze({
        host: hostEvidence.eventCount,
        client: clientEvidence.eventCount,
      }),
      turnRelay: 'unavailable',
    });
  } finally {
    clearTimeout(timer);
    if (host && host.exitCode === null) host.kill('SIGTERM');
    if (client && client.exitCode === null) client.kill('SIGTERM');
  }
}

function publicLobby(lobby, token) {
  const member = lobby.members.find((entry) => entry.token === token);
  if (!member) throw new Error('lobby token is invalid');
  return {
    protocol: LOCAL_MATCH_PROTOCOL,
    code: lobby.code,
    phase: lobby.phase,
    // The requested seed stays server-private until the native lifecycle confirms it.
    seed: lobby.handoff?.seed ?? null,
    seat: member.seat,
    members: lobby.members.map((entry) => ({
      seat: entry.seat, name: entry.name, ready: entry.ready,
    })),
    error: lobby.error,
    handoff: lobby.handoff ? { ...lobby.handoff, player: member.seat } : null,
    turnRelay: 'unavailable',
  };
}

/** Bounded in-memory browser lobby owner; it is reachable only through the loopback server. */
export class LocalMatchGateway {
  constructor(peerPath, options = {}) {
    this.peerPath = peerPath;
    this.timeoutMs = options.timeoutMs ?? DEFAULT_LOCAL_MATCH_TIMEOUT_MS;
    this.spawn = options.spawn ?? spawnChild;
    this.lobbies = new Map();
  }

  async available() {
    if (!this.peerPath) return false;
    try { await access(this.peerPath, fsConstants.X_OK); return true; }
    catch { return false; }
  }

  create(seed, name) {
    if (this.lobbies.size >= MAX_LOCAL_MATCH_LOBBIES) {
      throw new Error(`local lobby limit ${MAX_LOCAL_MATCH_LOBBIES} reached`);
    }
    seed = boundedSeed(seed);
    name = boundedName(name);
    let code;
    do { code = randomBytes(4).toString('hex'); } while (this.lobbies.has(code));
    const token = randomBytes(16).toString('hex');
    const lobby = {
      code, seed, phase: 'lobby', error: null, handoff: null, launching: false,
      members: [{ seat: 0, name, token, ready: false }],
    };
    this.lobbies.set(code, lobby);
    return { token, lobby: publicLobby(lobby, token) };
  }

  join(code, name) {
    code = exactCode(code);
    name = boundedName(name);
    const lobby = this.lobbies.get(code);
    if (!lobby) throw new Error('local lobby does not exist');
    if (lobby.phase !== 'lobby' || lobby.members.length >= LOCAL_MATCH_PLAYERS) {
      throw new Error('local lobby is not joinable');
    }
    const token = randomBytes(16).toString('hex');
    lobby.members.push({ seat: 1, name, token, ready: false });
    return { token, lobby: publicLobby(lobby, token) };
  }

  snapshot(code, token) {
    code = exactCode(code);
    const lobby = this.lobbies.get(code);
    if (!lobby) throw new Error('local lobby does not exist');
    return publicLobby(lobby, token);
  }

  ready(code, token, ready = true) {
    code = exactCode(code);
    if (typeof ready !== 'boolean') throw new Error('ready must be boolean');
    const lobby = this.lobbies.get(code);
    if (!lobby) throw new Error('local lobby does not exist');
    if (lobby.phase !== 'lobby') throw new Error('local lobby has already left setup');
    const member = lobby.members.find((entry) => entry.token === token);
    if (!member) throw new Error('lobby token is invalid');
    member.ready = ready;
    if (lobby.members.length === LOCAL_MATCH_PLAYERS &&
        lobby.members.every((entry) => entry.ready) && !lobby.launching) {
      lobby.launching = true;
      lobby.phase = 'starting';
      void this.#launch(lobby);
    }
    return publicLobby(lobby, token);
  }

  leave(code, token) {
    code = exactCode(code);
    const lobby = this.lobbies.get(code);
    if (!lobby) return false;
    if (!lobby.members.some((entry) => entry.token === token)) {
      throw new Error('lobby token is invalid');
    }
    if (lobby.phase === 'starting') throw new Error('cannot leave while MatchStart is in flight');
    this.lobbies.delete(code);
    return true;
  }

  async #launch(lobby) {
    try {
      if (!(await this.available())) {
        throw new Error('configured service-match-peer binary is absent or not executable');
      }
      lobby.handoff = await runServiceMatch(this.peerPath, lobby.seed, {
        spawn: this.spawn, timeoutMs: this.timeoutMs,
      });
      lobby.phase = 'started';
    } catch (error) {
      lobby.error = error.message;
      lobby.phase = 'failed';
    }
  }
}
