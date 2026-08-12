import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  decodeCanonicalPackageReceipt,
  GameModule,
} from '../public/js/play/wasmgame.js';

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

test('unpublished artifact and invalid wrapper inputs fail before staging', () => {
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
});
