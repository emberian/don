import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  CANONICAL_SINGLETON_GROUP_MOVE_BYTES,
  RETAIL_SINGLETON_GROUP_MOVE_HEX,
  bytesToHex,
  decodeCanonicalCommandPackage,
  discoverOwnerLocalSingleton,
  encodeCanonicalSingletonGroupMove,
  validateCanonicalCommandHex,
} from '../public/js/play/canonical-command-package.mjs';

test('retail singleton fixture reconstructs as one exact 27-byte package', () => {
  const built = encodeCanonicalSingletonGroupMove({ who: 0, o: 1, uid: 0x6a31 }, 47_435, 47_486);
  assert.equal(built.bytes.length, CANONICAL_SINGLETON_GROUP_MOVE_BYTES);
  assert.equal(built.hex, RETAIL_SINGLETON_GROUP_MOVE_HEX);
  assert.deepEqual(decodeCanonicalCommandPackage(built.bytes), {
    kind: 'group-move-singleton', who: 0, o: 1, x: 47_435, y: 47_486,
    setAngle: 0, angle: 0, orders: 1, queued: 2, form: 0, width: 50, disembark: 0,
    bytes: built.bytes, hex: RETAIL_SINGLETON_GROUP_MOVE_HEX,
  });
});

test('owner-local discovery never treats the renderer id as o', () => {
  const seen = [];
  const identity = discoverOwnerLocalSingleton([0x71_0000], (rendererId) => {
    seen.push(rendererId);
    return { who: 3, o: 17, uid: 0x55aa };
  }, 3);
  assert.deepEqual(seen, [0x71_0000]);
  assert.deepEqual(identity, { who: 3, o: 17, uid: 0x55aa });
  const built = encodeCanonicalSingletonGroupMove(identity, -1, 0x7fff_ffff);
  assert.equal(built.bytes[2], 3);
  assert.equal(new DataView(built.bytes.buffer).getInt16(3, true), 17);
  assert.notEqual(new DataView(built.bytes.buffer).getInt16(3, true), 0x71_0000);
});

test('Halt remains canonical and every malformed package fails closed', () => {
  assert.equal(validateCanonicalCommandHex('0c'), '0c');
  assert.equal(decodeCanonicalCommandPackage('0c').kind, 'halt');
  for (const text of ['0C', '0c00', '', '0', 'zz']) {
    assert.throws(() => validateCanonicalCommandHex(text));
  }

  const valid = encodeCanonicalSingletonGroupMove({ who: 1, o: 2, uid: 3 }, 10, 20).bytes;
  for (const [offset, value] of [[0, 1], [1, 0], [5, 4], [22, 2], [23, 1], [24, 0xff], [25, 49], [26, 1]]) {
    const changed = new Uint8Array(valid);
    changed[offset] = value;
    assert.throws(() => decodeCanonicalCommandPackage(changed), /canonical|admits|integer/);
  }
  assert.throws(() => decodeCanonicalCommandPackage(valid.subarray(0, 26)), /admits/);
  const extended = new Uint8Array(28);
  extended.set(valid);
  assert.throws(() => decodeCanonicalCommandPackage(extended), /admits/);
});

test('identity discovery and construction reject missing generational authority', () => {
  for (const resolve of [null, () => null, () => ({ who: 0, o: 1 }),
    () => ({ who: 0, o: 1, uid: 2, id: 1 })]) {
    assert.throws(() => discoverOwnerLocalSingleton([1], resolve), /resolver|identity|uid/);
  }
  assert.throws(() => discoverOwnerLocalSingleton([1, 2], () => ({ who: 0, o: 1, uid: 2 })),
    /exactly one/);
  assert.throws(() => discoverOwnerLocalSingleton([1], () => ({ who: 1, o: 1, uid: 2 }), 0),
    /does not match/);
  assert.throws(() => encodeCanonicalSingletonGroupMove({ who: 0, o: -1, uid: 2 }, 0, 0), / o /);
  assert.throws(() => encodeCanonicalSingletonGroupMove({ who: 0, o: 1, uid: 0x1_0000 }, 0, 0), /uid/);
});

test('signed coordinate endpoints survive canonical reconstruction', () => {
  const built = encodeCanonicalSingletonGroupMove(
    { who: 7, o: 0x7fff, uid: 0xffff }, -0x8000_0000, 0x7fff_ffff);
  const decoded = decodeCanonicalCommandPackage(built.hex);
  assert.equal(decoded.x, -0x8000_0000);
  assert.equal(decoded.y, 0x7fff_ffff);
  assert.equal(bytesToHex(decoded.bytes), built.hex);
});
