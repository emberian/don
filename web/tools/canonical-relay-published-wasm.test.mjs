import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';

import { encodeCanonicalSingletonGroupMove } from
  '../public/js/play/canonical-command-package.mjs';
import { GameModule } from '../public/js/play/wasmgame.js';

const WASM = new URL('../public/wasm/don_web.wasm', import.meta.url);
const DATA = process.env.DON_WEB_PROOF_DATA;

async function peer(compiled, gamedata, playdata, localPlayer) {
  const instance = await WebAssembly.instantiate(compiled, {});
  const mod = new GameModule(instance);
  assert.equal(mod.create(gamedata, playdata, 0x89abcdef), true);
  assert.equal(mod.startManualTeams([0, 1], [0, 1, 8, 8], 0, localPlayer, false), true);
  return mod;
}

function identityFor(mod, who, ownerLocal = null) {
  const views = mod.views();
  for (let row = 0; row < mod.live; row++) {
    const tag = views.tag[row] >>> 0;
    if ((tag & 0x80000000) === 0 || (tag & 0xf) !== who ||
        (tag & 0x20000000) !== 0) continue;
    const rendererId = mod.idAtRow(row);
    try {
      const identity = mod.commandIdentity(rendererId);
      if (identity.who === who && (ownerLocal === null || identity.o === ownerLocal)) {
        return { rendererId, identity, info: mod.info(rendererId) };
      }
    } catch {
      // Renderer bands include rows without owner-local Unit identity.
    }
  }
  throw new Error(`no command Unit for P${who}`);
}

function packetFor(mod, who) {
  const selected = identityFor(mod, who);
  const xDirection = selected.info.x < mod.span / 2 ? 1 : -1;
  const yDirection = selected.info.y < mod.span / 2 ? 1 : -1;
  return encodeCanonicalSingletonGroupMove({
    who: selected.identity.who,
    o: selected.identity.o,
    uid: selected.identity.uid,
  }, selected.info.x + xDirection * mod.subtile * 8,
  selected.info.y + yDirection * mod.subtile * 4);
}

function entry(mod, play, lockstepSerial, packet) {
  const selected = identityFor(mod, play, packet.identity.o);
  return {
    play,
    lockstepSerial,
    rendererId: selected.rendererId,
    bytes: packet.bytes,
  };
}

test('published Wasm batch rolls back P0 when a later package fails and permits exact retry',
  { skip: !DATA }, async () => {
    const [wasmBytes, gamedata, playdata] = await Promise.all([
      readFile(WASM),
      readFile(`${DATA}/gamedata.bin`),
      readFile(`${DATA}/playdata.bin`),
    ]);
    const compiled = await WebAssembly.compile(wasmBytes);
    const mod = await peer(compiled, gamedata, playdata, 0);
    const packets = [packetFor(mod, 0), packetFor(mod, 1)];
    const before = {
      frame: mod.frame,
      digest: mod.digest(),
      rngState: mod.rngState,
      positions: [identityFor(mod, 0).info, identityFor(mod, 1).info],
    };
    const observed = [];
    mod.observeCommands((event) => observed.push(event));
    assert.throws(() => mod.processCanonicalCommandBatch([
      entry(mod, 0, 1, packets[0]),
      { play: 1, lockstepSerial: 2, rendererId: 0x7fff_ffff, bytes: packets[1].bytes },
    ]), /rolled back: renderer id has no live owner-local Unit identity/);
    assert.equal(mod.frame, before.frame);
    assert.equal(mod.digest(), before.digest);
    assert.equal(mod.rngState, before.rngState);
    assert.deepEqual([identityFor(mod, 0).info, identityFor(mod, 1).info], before.positions);
    assert.deepEqual(observed, []);

    const receipts = mod.processCanonicalCommandBatch([
      entry(mod, 0, 1, packets[0]),
      entry(mod, 1, 2, packets[1]),
    ], () => { mod.step(1); return true; });
    assert.deepEqual(receipts.map((receipt) => receipt.play), [0, 1]);
    assert.equal(observed.length, 1);
    assert.deepEqual(observed[0].canonicalBatch.map((event) => event.who), [0, 1]);
    assert.deepEqual(observed[0].canonicalBatch.map(
      (event) => event.canonicalReceipt.play), [0, 1]);
    assert.equal(mod.frame, 1);
  });

test('published Wasm committed batches remain equal across independent instances',
  { skip: !DATA }, async () => {
    const [wasmBytes, gamedata, playdata] = await Promise.all([
      readFile(WASM),
      readFile(`${DATA}/gamedata.bin`),
      readFile(`${DATA}/playdata.bin`),
    ]);
    const compiled = await WebAssembly.compile(wasmBytes);
    const mods = await Promise.all([
      peer(compiled, gamedata, playdata, 0),
      peer(compiled, gamedata, playdata, 1),
    ]);
    for (let stamp = 0; stamp < 32; stamp++) {
      const packets = [packetFor(mods[0], 0), packetFor(mods[0], 1)];
      const receipts = mods.map((mod) => mod.processCanonicalCommandBatch([
        entry(mod, 0, stamp * 2 + 1, packets[0]),
        entry(mod, 1, stamp * 2 + 2, packets[1]),
      ], () => { mod.step(1); return true; }));
      assert.deepEqual(receipts[0], receipts[1]);
      assert.equal(mods[0].digest(), mods[1].digest());
      assert.equal(mods[0].rngState, mods[1].rngState);
    }
    assert.equal(mods[0].frame, 32);
    assert.equal(mods[0].digest(), '44baeaa8f09b9ec1');
  });
