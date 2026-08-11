// Canonical plaintext command-package shapes admitted by the browser lockstep bridge.
//
// Browser renderer ids are deliberately absent from this module. GroupCommand carries the
// retail owner-local `o`; the accompanying `uid` is a generational discovery witness used to
// authorize construction, not another wire field. Callers must obtain `(who,o,uid)` from the
// authoritative core and must never derive `o` from a renderer/global adapter id.

export const HALT_COMMAND_HEX = '0c';
export const GROUP_OPCODE = 0x00;
export const MOVE_TO_OPCODE = 0x07;
export const HALT_OPCODE = 0x0c;
export const CANONICAL_SINGLETON_GROUP_MOVE_BYTES = 27;
export const RETAIL_SINGLETON_GROUP_MOVE_HEX =
  '0001000100074bb900007eb9000000000000000000000102003200';

const CANONICAL_MOVE_TAIL = Object.freeze({
  setAngle: 0,
  angle: 0,
  orders: 1,
  queued: 2,
  form: 0,
  width: 50,
  disembark: 0,
});

function integer(value, min, max, label) {
  if (!Number.isInteger(value) || value < min || value > max) {
    throw new Error(`${label} must be an integer in ${min}..${max}`);
  }
  return value;
}

function exactIdentity(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value) ||
      Object.keys(value).some((key) => !['who', 'o', 'uid'].includes(key))) {
    throw new Error('command identity must contain only owner-local who/o/uid');
  }
  return Object.freeze({
    who: integer(value.who, 0, 7, 'command identity who'),
    o: integer(value.o, 0, 0x7fff, 'command identity o'),
    uid: integer(value.uid, 0, 0xffff, 'command identity uid'),
  });
}

function inputBytes(value) {
  if (value instanceof Uint8Array) return new Uint8Array(value);
  if (ArrayBuffer.isView(value)) {
    return new Uint8Array(value.buffer.slice(value.byteOffset, value.byteOffset + value.byteLength));
  }
  if (value instanceof ArrayBuffer) return new Uint8Array(value.slice(0));
  throw new Error('command package must be bytes');
}

function readI32(bytes, offset) {
  return new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getInt32(offset, true);
}

function readI16(bytes, offset) {
  return new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getInt16(offset, true);
}

function writeI32(bytes, offset, value) {
  new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).setInt32(offset, value, true);
}

function writeI16(bytes, offset, value) {
  new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).setInt16(offset, value, true);
}

export function bytesToHex(bytes) {
  return Array.from(inputBytes(bytes), (byte) => byte.toString(16).padStart(2, '0')).join('');
}

export function hexToBytes(text) {
  if (typeof text !== 'string' || !text.length || text.length % 2 !== 0 ||
      !/^[a-f0-9]+$/.test(text)) {
    throw new Error('command package hex must be nonempty, even-length lowercase hexadecimal');
  }
  return Uint8Array.from(
    { length: text.length / 2 },
    (_, index) => Number.parseInt(text.slice(index * 2, index * 2 + 2), 16),
  );
}

/**
 * Resolve one renderer selection through a future authoritative identity hook.
 *
 * `rendererId` is used only as the lookup key. It is never returned as, compared to, or
 * encoded as `o`. The resolver may return null for a stale/dead id; that is a refusal.
 */
export function discoverOwnerLocalSingleton(rendererIds, resolveIdentity, expectedWho = null) {
  if (!Array.isArray(rendererIds) || rendererIds.length !== 1 ||
      !Number.isInteger(rendererIds[0]) || rendererIds[0] < 0) {
    throw new Error('canonical 27-byte Group+Move requires exactly one renderer selection id');
  }
  if (typeof resolveIdentity !== 'function') {
    throw new Error('authoritative owner-local identity resolver is unavailable');
  }
  const identity = exactIdentity(resolveIdentity(rendererIds[0]));
  if (expectedWho !== null && identity.who !== integer(expectedWho, 0, 7, 'expected who')) {
    throw new Error(`selected object owner ${identity.who} does not match command owner ${expectedWho}`);
  }
  return identity;
}

/** Build the direct-Web retail singleton shape: Group(who,o) then complete MoveTo. */
export function encodeCanonicalSingletonGroupMove(identityValue, xValue, yValue) {
  const identity = exactIdentity(identityValue);
  const x = integer(xValue, -0x8000_0000, 0x7fff_ffff, 'MoveTo x');
  const y = integer(yValue, -0x8000_0000, 0x7fff_ffff, 'MoveTo y');
  const bytes = new Uint8Array(CANONICAL_SINGLETON_GROUP_MOVE_BYTES);
  bytes[0] = GROUP_OPCODE;
  bytes[1] = 1;
  bytes[2] = identity.who;
  writeI16(bytes, 3, identity.o);
  bytes[5] = MOVE_TO_OPCODE;
  writeI32(bytes, 6, x);
  writeI32(bytes, 10, y);
  writeI32(bytes, 14, CANONICAL_MOVE_TAIL.setAngle);
  writeI32(bytes, 18, CANONICAL_MOVE_TAIL.angle);
  bytes[22] = CANONICAL_MOVE_TAIL.orders;
  bytes[23] = CANONICAL_MOVE_TAIL.queued;
  bytes[24] = CANONICAL_MOVE_TAIL.form;
  bytes[25] = CANONICAL_MOVE_TAIL.width;
  bytes[26] = CANONICAL_MOVE_TAIL.disembark;
  return Object.freeze({
    kind: 'group-move-singleton',
    identity,
    x,
    y,
    bytes,
    hex: bytesToHex(bytes),
  });
}

/** Decode and prove one of the two canonical browser relay payloads. */
export function decodeCanonicalCommandPackage(value) {
  const bytes = typeof value === 'string' ? hexToBytes(value) : inputBytes(value);
  if (bytes.length === 1 && bytes[0] === HALT_OPCODE) {
    return Object.freeze({ kind: 'halt', bytes, hex: HALT_COMMAND_HEX });
  }
  if (bytes.length !== CANONICAL_SINGLETON_GROUP_MOVE_BYTES ||
      bytes[0] !== GROUP_OPCODE || bytes[1] !== 1 || bytes[5] !== MOVE_TO_OPCODE) {
    throw new Error('relay admits only canonical Halt or 27-byte singleton Group+Move packages');
  }
  const decoded = {
    kind: 'group-move-singleton',
    who: bytes[2],
    o: readI16(bytes, 3),
    x: readI32(bytes, 6),
    y: readI32(bytes, 10),
    setAngle: readI32(bytes, 14),
    angle: readI32(bytes, 18),
    orders: bytes[22],
    queued: bytes[23],
    form: bytes[24] << 24 >> 24,
    width: bytes[25] << 24 >> 24,
    disembark: bytes[26] << 24 >> 24,
  };
  integer(decoded.who, 0, 7, 'Group who');
  integer(decoded.o, 0, 0x7fff, 'Group o');
  for (const [field, expected] of Object.entries(CANONICAL_MOVE_TAIL)) {
    if (decoded[field] !== expected) {
      throw new Error(`27-byte Group+Move has noncanonical ${field}=${decoded[field]}`);
    }
  }
  const rebuilt = encodeCanonicalSingletonGroupMove(
    { who: decoded.who, o: decoded.o, uid: 0 }, decoded.x, decoded.y).bytes;
  if (bytesToHex(rebuilt) !== bytesToHex(bytes)) {
    throw new Error('27-byte Group+Move did not survive canonical reconstruction');
  }
  return Object.freeze({ ...decoded, bytes, hex: bytesToHex(bytes) });
}

export function validateCanonicalCommandHex(text) {
  return decodeCanonicalCommandPackage(text).hex;
}

