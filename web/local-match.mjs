// Local-only browser handoff for the real don-crossplay ServiceMatch lifecycle.
//
// The two browser clients receive an exact seed/epoch/roster only after the configured
// native peers prove Create/Find/Join/ready/StartGame -> MatchStart agreement. An opt-in,
// one-HaltCommand turn barrier then keeps those peers alive: a browser frame advances only
// after both native ServiceMatch owners return the same ordered TurnPackage set, and the
// next stamp stays closed until both paused browser Sims acknowledge equal state.

import { spawn as spawnChild } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { constants as fsConstants } from 'node:fs';
import { access } from 'node:fs/promises';

export const LOCAL_MATCH_PROTOCOL = 'don.local-match-handoff.v1';
export const LOCAL_MATCH_TURN_RELAY = 'canonical-halt-v1';
export const LOCAL_MATCH_PLAYERS = 2;
export const MAX_LOCAL_MATCH_LOBBIES = 16;
export const MAX_LOCAL_MATCH_BODY_BYTES = 16 * 1024;
export const MAX_LOCAL_MATCH_OUTPUT_BYTES = 256 * 1024;
export const DEFAULT_LOCAL_MATCH_TIMEOUT_MS = 60_000;
export const HALT_COMMAND_HEX = '0c';

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

export function validateHaltTurnRequest(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value) ||
      Object.keys(value).some((key) => !['token', 'stamp', 'commandHex'].includes(key))) {
    throw new Error('Halt turn request contains unsupported browser input');
  }
  if (typeof value.token !== 'string' || !Number.isInteger(value.stamp) ||
      value.commandHex !== HALT_COMMAND_HEX) {
    throw new Error('turn relay admits exactly canonical one-byte HaltCommand 0x0c');
  }
  return { token: value.token, stamp: value.stamp, commandHex: value.commandHex };
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

function streamingPeer(peerPath, args, side, spawn) {
  let child;
  try {
    child = spawn(peerPath, args, { stdio: ['pipe', 'pipe', 'pipe'] });
  } catch (error) {
    throw new Error(`${side} peer launch failed: ${error.message}`);
  }
  const peer = {
    child, side, bytes: 0, carry: '', lines: [], waiters: [], error: null, stderr: '', closing: false,
  };
  const fail = (error) => {
    if (peer.error) return;
    peer.error = error instanceof Error ? error : new Error(String(error));
    for (const waiter of peer.waiters.splice(0)) {
      clearTimeout(waiter.timer);
      waiter.reject(peer.error);
    }
    if (child.exitCode === null) child.kill('SIGTERM');
  };
  const emit = (line) => {
    for (let index = 0; index < peer.waiters.length; index++) {
      const waiter = peer.waiters[index];
      let accepted = false;
      try { accepted = waiter.predicate(line); }
      catch (error) { fail(error); return; }
      if (accepted) {
        peer.waiters.splice(index, 1);
        clearTimeout(waiter.timer);
        waiter.resolve(line);
        return;
      }
    }
    peer.lines.push(line);
  };
  child.stdout.setEncoding('utf8');
  child.stderr.setEncoding('utf8');
  child.stdin.on('error', (error) => {
    if (!peer.closing) fail(new Error(`${side} relay input failed: ${error.message}`));
  });
  child.stdout.on('data', (chunk) => {
    try {
      peer.bytes += Buffer.byteLength(chunk);
      if (peer.bytes > MAX_LOCAL_MATCH_OUTPUT_BYTES) {
        throw new Error(`${side} peer exceeded the ${MAX_LOCAL_MATCH_OUTPUT_BYTES}-byte output bound`);
      }
      peer.carry += chunk;
      const lines = peer.carry.split(/\r?\n/);
      peer.carry = lines.pop();
      for (const line of lines) if (line) emit(line);
    } catch (error) { fail(error); }
  });
  child.stderr.on('data', (chunk) => {
    try { peer.stderr = appendBounded(peer.stderr, chunk, `${side} stderr`); }
    catch (error) { fail(error); }
  });
  child.once('error', (error) => fail(new Error(`${side} peer launch failed: ${error.message}`)));
  child.once('exit', (code, signal) => {
    if (!peer.closing) {
      fail(new Error(`${side} relay exited (code ${code}, signal ${signal ?? 'none'}): ${peer.stderr.trim()}`));
    }
  });
  peer.fail = fail;
  return peer;
}

function waitPeerLine(peer, predicate, timeoutMs, label) {
  if (peer.error) return Promise.reject(peer.error);
  const index = peer.lines.findIndex(predicate);
  if (index >= 0) return Promise.resolve(peer.lines.splice(index, 1)[0]);
  return new Promise((resolve, reject) => {
    const waiter = { predicate, resolve, reject, timer: null };
    waiter.timer = setTimeout(() => {
      const at = peer.waiters.indexOf(waiter);
      if (at >= 0) peer.waiters.splice(at, 1);
      const error = new Error(`${peer.side} peer timed out waiting for ${label}`);
      peer.fail(error);
      reject(error);
    }, timeoutMs);
    peer.waiters.push(waiter);
  });
}

function jsonEvent(line, event) {
  const value = processLine(line);
  return value?.event === event ? value : null;
}

function waitPeerEvent(peer, event, timeoutMs, predicate = () => true) {
  return waitPeerLine(peer, (line) => {
    const value = jsonEvent(line, event);
    return value !== null && predicate(value);
  }, timeoutMs, event).then(processLine);
}

function closePeer(peer) {
  if (!peer) return;
  peer.closing = true;
  if (peer.child.exitCode === null) {
    if (peer.child.stdin.writable) peer.child.stdin.end('QUIT\n');
    setTimeout(() => {
      if (peer.child.exitCode === null) peer.child.kill('SIGTERM');
    }, 250).unref?.();
  }
}

function validateRelayTurn(value, stamp, side) {
  if (!value || value.stamp !== stamp || value.packages !== LOCAL_MATCH_PLAYERS ||
      !Array.isArray(value.ordered) || value.ordered.length !== LOCAL_MATCH_PLAYERS ||
      !/^[a-f0-9]{16}$/.test(value.hash ?? '')) {
    throw new Error(`${side} peer returned a malformed turn ${stamp}`);
  }
  for (let play = 0; play < LOCAL_MATCH_PLAYERS; play++) {
    const package_ = value.ordered[play];
    if (package_?.stamp !== stamp || package_?.play !== play ||
        package_?.payload !== HALT_COMMAND_HEX) {
      throw new Error(`${side} peer returned a noncanonical or misordered Halt turn ${stamp}`);
    }
  }
  return value;
}

class NativeTurnRelay {
  constructor(host, client, timeoutMs) {
    this.host = host;
    this.client = client;
    this.timeoutMs = timeoutMs;
    this.nextStamp = 0;
    this.closed = false;
  }

  async completeTurn(stamp, payloads) {
    if (this.closed) throw new Error('native turn relay is closed');
    if (stamp !== this.nextStamp) {
      throw new Error(`native turn relay expected stamp ${this.nextStamp}, got ${stamp}`);
    }
    if (!Array.isArray(payloads) || payloads.length !== LOCAL_MATCH_PLAYERS ||
        payloads.some((payload) => payload !== HALT_COMMAND_HEX)) {
      throw new Error('native turn relay admits exactly one canonical HaltCommand per seat');
    }
    const peers = [this.host, this.client];
    const waits = peers.map((peer) =>
      waitPeerEvent(peer, 'turn', this.timeoutMs, (value) => value.stamp === stamp));
    for (let play = 0; play < peers.length; play++) {
      const peer = peers[play];
      if (!peer.child.stdin.writable) throw new Error(`${peer.side} relay input is closed`);
      peer.child.stdin.write(`TURN ${stamp} ${payloads[play]}\n`);
    }
    const [hostTurn, clientTurn] = await Promise.all(waits);
    validateRelayTurn(hostTurn, stamp, 'host');
    validateRelayTurn(clientTurn, stamp, 'client');
    if (hostTurn.hash !== clientTurn.hash ||
        JSON.stringify(hostTurn.ordered) !== JSON.stringify(clientTurn.ordered)) {
      throw new Error(`native peers disagreed on ordered package set for turn ${stamp}`);
    }
    this.nextStamp++;
    return Object.freeze({
      stamp,
      hash: hostTurn.hash,
      packages: Object.freeze(hostTurn.ordered.map((package_) => Object.freeze({ ...package_ }))),
    });
  }

  close() {
    if (this.closed) return;
    this.closed = true;
    closePeer(this.host);
    closePeer(this.client);
  }
}

/** Start two long-lived native peers and expose no turn until both package sets agree. */
export async function startServiceMatchRelay(peerPath, seed, options = {}) {
  const spawn = options.spawn ?? spawnChild;
  const timeoutMs = options.timeoutMs ?? DEFAULT_LOCAL_MATCH_TIMEOUT_MS;
  seed = boundedSeed(seed);
  let host;
  let client;
  try {
    host = streamingPeer(peerPath, ['host', '--seed', String(seed), '--relay'], 'host', spawn);
    const serviceText = await waitPeerLine(host, (line) => serviceLine(line) !== null,
      timeoutMs, 'SERVICE');
    const service = serviceLine(serviceText);
    client = streamingPeer(peerPath,
      ['join', service.directory, service.lobbyId, '--relay'], 'client', spawn);
    const [hostDirectory, clientDirectory] = await Promise.all([
      waitPeerEvent(host, 'directory_started', timeoutMs),
      waitPeerEvent(client, 'directory_started', timeoutMs),
    ]);
    const [hostConfirmed, clientConfirmed] = await Promise.all([
      waitPeerEvent(host, 'match_confirmed', timeoutMs),
      waitPeerEvent(client, 'match_confirmed', timeoutMs),
    ]);
    await Promise.all([
      waitPeerEvent(host, 'relay_ready', timeoutMs),
      waitPeerEvent(client, 'relay_ready', timeoutMs),
    ]);
    for (const observed of [hostDirectory, clientDirectory, clientConfirmed]) {
      for (const field of ['lobby', 'reference', 'epoch', 'seed']) {
        if (observed[field] !== hostConfirmed[field]) {
          throw new Error(`peers disagree on confirmed MatchStart ${field}`);
        }
      }
    }
    if (hostConfirmed.lobby !== service.lobbyId ||
        hostConfirmed.reference !== service.lobbyId || hostConfirmed.seed !== seed ||
        !Number.isInteger(hostConfirmed.epoch) || hostConfirmed.epoch <= 0) {
      throw new Error('confirmed MatchStart is not bound to the requested seed/reference/epoch');
    }
    const relay = new NativeTurnRelay(host, client, timeoutMs);
    return {
      handoff: Object.freeze({
        protocol: LOCAL_MATCH_PROTOCOL,
        lobbyId: hostConfirmed.lobby,
        sessionReference: hostConfirmed.reference,
        epoch: hostConfirmed.epoch >>> 0,
        seed,
        activePlayers: Object.freeze([0, 1]),
        teams: Object.freeze([0, 1, 8, 8]),
        teamStyle: 0,
        turnRelay: LOCAL_MATCH_TURN_RELAY,
      }),
      relay,
    };
  } catch (error) {
    closePeer(host);
    closePeer(client);
    throw error;
  }
}

function publicLobby(lobby, token) {
  const member = lobby.members.find((entry) => entry.token === token);
  if (!member) throw new Error('lobby token is invalid');
  const turn = lobby.turn ? {
    stamp: lobby.turn.stamp,
    phase: lobby.turn.phase,
    submitted: lobby.turn.submitted.slice(),
    agreement: lobby.turn.agreement ? {
      stamp: lobby.turn.agreement.stamp,
      hash: lobby.turn.agreement.hash,
      packages: lobby.turn.agreement.packages.map((package_) => ({ ...package_ })),
    } : null,
  } : null;
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
    turnRelay: lobby.handoff?.turnRelay ?? LOCAL_MATCH_TURN_RELAY,
    turn,
    lastConfirmed: lobby.lastConfirmed ? { ...lobby.lastConfirmed } : null,
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
      relay: null, turn: null, lastConfirmed: null, barrierTimer: null,
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
    this.#close(lobby);
    this.lobbies.delete(code);
    return true;
  }

  submitTurn(code, token, stamp, commandHex) {
    const { lobby, member } = this.#startedMember(code, token);
    if (!Number.isInteger(stamp) || stamp < 0 || stamp > 0xffff_ffff) {
      throw new Error('turn stamp must be a u32');
    }
    if (!lobby.turn || lobby.turn.stamp !== stamp || lobby.turn.phase !== 'waiting') {
      throw new Error(`turn ${stamp} is not the open native barrier`);
    }
    if (commandHex !== HALT_COMMAND_HEX) {
      throw new Error('turn relay admits exactly canonical one-byte HaltCommand 0x0c');
    }
    if (lobby.turn.submitted[member.seat]) return publicLobby(lobby, token);
    lobby.turn.submitted[member.seat] = true;
    lobby.turn.payloads[member.seat] = commandHex;
    this.#armTimeout(lobby, `turn ${stamp} timed out waiting for both browser seats`);
    if (lobby.turn.submitted.every(Boolean)) {
      lobby.turn.phase = 'agreeing';
      this.#armTimeout(lobby, `turn ${stamp} timed out in the native package relay`);
      void this.#completeTurn(lobby, stamp);
    }
    return publicLobby(lobby, token);
  }

  acknowledgeTurn(code, token, stamp, frame, digest, rngState) {
    const { lobby, member } = this.#startedMember(code, token);
    if (!Number.isInteger(stamp) || lobby.turn?.stamp !== stamp ||
        lobby.turn.phase !== 'agreed') {
      throw new Error(`turn ${stamp} has no agreed package set to acknowledge`);
    }
    if (!Number.isInteger(frame) || frame !== stamp + 1 ||
        typeof digest !== 'string' || !/^[a-f0-9]{16}$/.test(digest) ||
        !Number.isInteger(rngState) || rngState < 0 || rngState > 0xffff_ffff) {
      throw new Error('browser turn acknowledgement has an invalid frame/digest/RNG state');
    }
    const ack = { frame, digest, rngState: rngState >>> 0 };
    const previous = lobby.turn.acks[member.seat];
    if (previous && JSON.stringify(previous) !== JSON.stringify(ack)) {
      this.#fail(lobby, `seat ${member.seat} contradicted its turn ${stamp} acknowledgement`);
      throw new Error(lobby.error);
    }
    lobby.turn.acks[member.seat] = ack;
    this.#armTimeout(lobby, `turn ${stamp} timed out waiting for equal browser acknowledgements`);
    if (lobby.turn.acks.every(Boolean)) {
      if (JSON.stringify(lobby.turn.acks[0]) !== JSON.stringify(lobby.turn.acks[1])) {
        this.#fail(lobby, `browser Sims disagreed after turn ${stamp}`);
        throw new Error(lobby.error);
      }
      clearTimeout(lobby.barrierTimer);
      lobby.barrierTimer = null;
      lobby.lastConfirmed = Object.freeze({
        stamp,
        hash: lobby.turn.agreement.hash,
        ...ack,
      });
      lobby.turn = this.#newTurn(stamp + 1);
    }
    return publicLobby(lobby, token);
  }

  async #launch(lobby) {
    try {
      if (!(await this.available())) {
        throw new Error('configured service-match-peer binary is absent or not executable');
      }
      const started = await startServiceMatchRelay(this.peerPath, lobby.seed, {
        spawn: this.spawn, timeoutMs: this.timeoutMs,
      });
      lobby.handoff = started.handoff;
      lobby.relay = started.relay;
      lobby.turn = this.#newTurn(0);
      lobby.phase = 'started';
    } catch (error) {
      this.#fail(lobby, error.message);
    }
  }

  async #completeTurn(lobby, stamp) {
    try {
      const agreement = await lobby.relay.completeTurn(stamp, lobby.turn.payloads.slice());
      if (lobby.phase !== 'started' || lobby.turn?.stamp !== stamp ||
          lobby.turn.phase !== 'agreeing') {
        throw new Error(`turn ${stamp} completed outside its open browser barrier`);
      }
      lobby.turn.agreement = agreement;
      lobby.turn.acks = [null, null];
      lobby.turn.phase = 'agreed';
      this.#armTimeout(lobby, `turn ${stamp} timed out waiting for equal browser acknowledgements`);
    } catch (error) {
      this.#fail(lobby, error.message);
    }
  }

  #startedMember(code, token) {
    code = exactCode(code);
    const lobby = this.lobbies.get(code);
    if (!lobby) throw new Error('local lobby does not exist');
    if (lobby.phase !== 'started' || !lobby.relay) {
      throw new Error('local lobby has no live native turn relay');
    }
    const member = lobby.members.find((entry) => entry.token === token);
    if (!member) throw new Error('lobby token is invalid');
    return { lobby, member };
  }

  #newTurn(stamp) {
    if (!Number.isSafeInteger(stamp) || stamp > 0xffff_ffff) {
      throw new Error('local turn stamp space exhausted');
    }
    return {
      stamp, phase: 'waiting', submitted: [false, false], payloads: [null, null],
      agreement: null, acks: null,
    };
  }

  #armTimeout(lobby, message) {
    clearTimeout(lobby.barrierTimer);
    lobby.barrierTimer = setTimeout(() => this.#fail(lobby, message), this.timeoutMs);
  }

  #close(lobby) {
    clearTimeout(lobby.barrierTimer);
    lobby.barrierTimer = null;
    lobby.relay?.close();
    lobby.relay = null;
  }

  #fail(lobby, message) {
    this.#close(lobby);
    lobby.error = String(message);
    lobby.phase = 'failed';
  }
}
