import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  canonicalPackageReceiptImage,
  decodeCanonicalPackageReceipt,
  GameModule,
} from '../public/js/play/wasmgame.js';
import { validateCanonicalReceiptImages } from '../local-match.mjs';

const COMMAND_PTR = 64;
const IDENTITY_PTR = 256;
const RECEIPT_PTR = 320;

function fakeModule() {
  const memory = new WebAssembly.Memory({ initial: 1 });
  const expectedBytes = Uint8Array.from({ length: 27 }, (_, index) => index);
  let captures = 0;
  let processes = 0;
  const x = {
    memory,
    game_cmd_capacity: () => 16 * 1024,
    game_cmd_ptr: () => COMMAND_PTR,
    game_has_gamedata: () => 1,
    game_object_command_identity: (_game, rendererId) => {
      captures++;
      assert.equal(rendererId >>> 0, 0x710000);
      new Int32Array(memory.buffer, IDENTITY_PTR, 4).set([9, 3, 17, 0x55aa]);
      return 1;
    },
    game_object_command_identity_ptr: () => IDENTITY_PTR,
    game_process_command_package: (_game, play, serial, len) => {
      processes++;
      assert.equal(play, 3);
      assert.equal(serial, 41);
      assert.equal(len, expectedBytes.length);
      assert.deepEqual(
        new Uint8Array(memory.buffer, COMMAND_PTR, len), expectedBytes);
      new Int32Array(memory.buffer, RECEIPT_PTR, 16).set([
        3, 41, 12, 3, 25, 1, 7, 2, 0x12345678, 99, 99,
        0x710000, 9, 3, 17, 0x55aa,
      ]);
      return 1;
    },
    game_package_receipt_ptr: () => RECEIPT_PTR,
    game_package_receipt_words: () => 16,
  };
  const game = Object.create(GameModule.prototype);
  Object.assign(game, { x, mem: memory, g: 7, playerCount: 4 });
  return { game, expectedBytes, counts: () => ({ captures, processes }) };
}

test('wrapper synchronously leases generation and binds the copied Sim receipt', () => {
  const { game, expectedBytes, counts } = fakeModule();
  const observed = [];
  game.observeCommands((event) => observed.push(event));
  const receipt = game.processCanonicalCommandPackage(3, 41, 0x710000, expectedBytes);
  assert.deepEqual(counts(), { captures: 1, processes: 1 });
  assert.equal(receipt.play, 3);
  assert.equal(receipt.lockstepSerial, 41);
  assert.equal(receipt.commandStateRevision, 0x00000002_00000007n);
  assert.equal(receipt.groupsChecksum, 0x12345678);
  assert.deepEqual(receipt.selected, [{
    rendererId: 0x710000, generation: 9, who: 3, o: 17, uid: 0x55aa,
  }]);
  assert.ok(Object.isFrozen(receipt));
  assert.ok(Object.isFrozen(receipt.selected[0]));
  assert.equal(observed.length, 1);
  assert.equal(observed[0].frame, 12);
  assert.equal(observed[0].who, 3);
  assert.deepEqual(observed[0].bytes, expectedBytes);
  assert.equal(observed[0].canonicalReceipt, receipt);
});

test('source receipt conversion produces the exact JSON-safe coordinator ACK image', () => {
  const receipt = (play) => Object.freeze({
    play,
    lockstepSerial: play + 1,
    frame: 0,
    who: play,
    groupSlot: play ? 65 : 1,
    commandStateRevision: BigInt(play + 1),
    groupsChecksum: 0x8000_0000 + play,
    randomStateBefore: -1,
    randomStateAfter: -1,
    selected: Object.freeze([Object.freeze({
      rendererId: play ? 8 : 0,
      generation: 0,
      who: play,
      o: 0,
      uid: play ? 8 : 0,
    })]),
  });
  const images = [0, 1].map((play) => canonicalPackageReceiptImage(receipt(play)));
  assert.deepEqual(validateCanonicalReceiptImages(images, 0), images);
  assert.deepEqual(Object.keys(images[0]), [
    'play', 'lockstepSerial', 'frame', 'who', 'groupSlot', 'commandStateRevision',
    'groupsChecksum', 'randomStateBefore', 'randomStateAfter', 'selected',
  ]);
  assert.equal(images[0].commandStateRevision, '0x0000000000000001');
  assert.equal(images[0].randomStateBefore, 0xffff_ffff);
});

test('canonical package batch rolls back a later failure and publishes no observer event', () => {
  let state = { frame: 7, digest: '1111222233334444', rng: 99 };
  let applied = 0;
  let loaded = 0;
  const observed = [];
  const game = Object.create(GameModule.prototype);
  Object.assign(game, {
    x: {
      game_frame: () => state.frame,
      game_rng_state: () => state.rng,
    },
    g: 1,
    _submitted: 4,
    digest: () => state.digest,
    saveCore: () => Uint8Array.of(state.frame, state.rng),
    loadCore: () => {
      loaded++;
      state = { frame: 7, digest: '1111222233334444', rng: 99 };
      return { frame: state.frame, digest: state.digest, rngState: state.rng };
    },
    processCanonicalCommandPackage: (_play, _serial, _renderer, _bytes, options) => {
      assert.equal(options.observe, false);
      applied++;
      state = { frame: 7, digest: `aaaa22223333444${applied}`, rng: 99 };
      if (applied === 2) throw new Error('P1 mutation gate refused');
      return { play: 0 };
    },
    observeCanonicalCommandPackage: (...args) => observed.push(args),
  });
  assert.throws(() => game.processCanonicalCommandBatch([
    { play: 0, lockstepSerial: 1, rendererId: 10, bytes: Uint8Array.of(1) },
    { play: 1, lockstepSerial: 2, rendererId: 20, bytes: Uint8Array.of(2) },
  ]), /rolled back: P1 mutation gate refused/);
  assert.equal(loaded, 1);
  assert.deepEqual(state, { frame: 7, digest: '1111222233334444', rng: 99 });
  assert.equal(game._submitted, 4);
  assert.deepEqual(observed, []);
});

test('canonical package batch publishes each package once only after commit succeeds', () => {
  let frame = 3;
  let digest = '1111222233334444';
  const observed = [];
  const game = Object.create(GameModule.prototype);
  Object.assign(game, {
    x: { game_frame: () => frame, game_rng_state: () => 77 },
    g: 1,
    _submitted: 0,
    digest: () => digest,
    saveCore: () => Uint8Array.of(3),
    loadCore: () => { throw new Error('success path must not load'); },
    processCanonicalCommandPackage: (play, lockstepSerial, _renderer, _bytes, options) => {
      assert.equal(options.observe, false);
      return { play, lockstepSerial, frame: 3 };
    },
    observeCanonicalCommandBatch: (applied) => observed.push(...applied.map(({ entry, receipt }) => ({
      bytes: Array.from(entry.bytes), receipt,
    }))),
  });
  const receipts = game.processCanonicalCommandBatch([
    { play: 0, lockstepSerial: 7, rendererId: 10, bytes: Uint8Array.of(1) },
    { play: 1, lockstepSerial: 8, rendererId: 20, bytes: Uint8Array.of(2) },
  ], (prepared) => {
    assert.equal(observed.length, 0);
    assert.deepEqual(prepared.map((value) => value.play), [0, 1]);
    frame = 4;
    digest = '5555666677778888';
    return true;
  });
  assert.deepEqual(receipts.map((value) => value.play), [0, 1]);
  assert.deepEqual(observed.map((value) => value.bytes), [[1], [2]]);
  assert.deepEqual(observed.map((value) => value.receipt.play), [0, 1]);
});

test('receipt decoder rejects altered identity, random consumption and malformed extent', () => {
  const valid = Int32Array.from([
    3, 41, 12, 3, 25, 1, 7, 2, 0x12345678, 99, 99,
    0x710000, 9, 3, 17, 0x55aa,
  ]);
  const identity = { rendererId: 0x710000, generation: 9, who: 3, o: 17, uid: 0x55aa };
  assert.throws(
    () => decodeCanonicalPackageReceipt(valid, { play: 2, lockstepSerial: 41, identity }),
    /receipt play/);
  const stale = new Int32Array(valid);
  stale[12] = 10;
  assert.throws(
    () => decodeCanonicalPackageReceipt(stale, { play: 3, lockstepSerial: 41, identity }),
    /leased Unit identity/);
  const random = new Int32Array(valid);
  random[10] = 100;
  assert.throws(() => decodeCanonicalPackageReceipt(random), /random state/);
  assert.throws(() => decodeCanonicalPackageReceipt(valid.subarray(0, 15)), /extent/);
});

test('missing artifact exports and invalid wrapper inputs fail before staging', () => {
  const absent = Object.create(GameModule.prototype);
  Object.assign(absent, { x: {}, g: 1, playerCount: 4 });
  assert.throws(
    () => absent.processCanonicalCommandPackage(0, 1, 1, Uint8Array.of(0)),
    /absent from this compiled artifact/);

  const { game, expectedBytes, counts } = fakeModule();
  assert.throws(
    () => game.processCanonicalCommandPackage(4, 41, 0x710000, expectedBytes),
    /in-range play/);
  assert.deepEqual(counts(), { captures: 0, processes: 0 });

  const missingData = fakeModule();
  missingData.game.x.game_has_gamedata = () => 0;
  assert.equal(missingData.game.canonicalPackageReady(), false);
  assert.throws(
    () => missingData.game.processCanonicalCommandPackage(
      3, 41, 0x710000, missingData.expectedBytes),
    /DONPACK5/);
  assert.deepEqual(missingData.counts(), { captures: 0, processes: 0 });
});
