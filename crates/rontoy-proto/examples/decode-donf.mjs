// Dependency-free generic DONF v1 frame decoder. It intentionally returns TLV
// tags instead of interpreting the Rust model, making it useful as an independent
// golden-vector checker and a forward-compatible browser/debug reader.
import { readFileSync } from "node:fs";

const HEADER_LEN = 56;
const text = new TextDecoder("utf-8", { fatal: true });

function crc32Parts(parts) {
  let crc = 0xffffffff;
  for (const bytes of parts) {
    for (const byte of bytes) {
      crc ^= byte;
      for (let bit = 0; bit < 8; bit++) {
        const mask = -(crc & 1);
        crc = (crc >>> 1) ^ (0xedb88320 & mask);
      }
    }
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function decodeRecord(bytes) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const fields = [];
  let offset = 0;
  while (offset < bytes.length) {
    if (bytes.length - offset < 8) throw new Error("truncated TLV header");
    const tag = view.getUint16(offset, true);
    const wireType = view.getUint8(offset + 2);
    const flags = view.getUint8(offset + 3);
    const length = view.getUint32(offset + 4, true);
    offset += 8;
    if (offset + length > bytes.length) throw new Error("truncated TLV payload");
    const data = bytes.subarray(offset, offset + length);
    offset += length;
    let value;
    if (wireType === 1 && length === 8) value = new DataView(data.buffer, data.byteOffset, 8).getBigUint64(0, true);
    else if (wireType === 2 && length === 8) value = new DataView(data.buffer, data.byteOffset, 8).getBigInt64(0, true);
    else if (wireType === 3) value = text.decode(data);
    else if (wireType === 4) value = decodeRecord(data);
    else value = Array.from(data);
    fields.push({ tag, wireType, flags, value });
  }
  return fields;
}

export function decodeDonf(input) {
  const bytes = input instanceof Uint8Array ? input : new Uint8Array(input);
  if (bytes.length < HEADER_LEN) throw new Error("truncated DONF header");
  if (text.decode(bytes.subarray(0, 4)) !== "DONF") throw new Error("bad DONF magic");
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const payloadLength = view.getUint32(48, true);
  if (bytes.length !== HEADER_LEN + payloadLength) throw new Error("DONF length mismatch");
  const payload = bytes.subarray(HEADER_LEN);
  const expectedCrc = view.getUint32(52, true);
  const actualCrc = crc32Parts([bytes.subarray(0, 52), payload]);
  if (actualCrc !== expectedCrc) throw new Error("DONF CRC mismatch");
  return {
    protocolMajor: view.getUint16(4, true),
    protocolMinor: view.getUint16(6, true),
    messageKind: view.getUint16(8, true),
    flags: view.getUint32(12, true),
    sequence: view.getBigUint64(16, true),
    sentUnixMs: view.getBigInt64(24, true),
    streamId: Array.from(bytes.subarray(32, 48)),
    fields: decodeRecord(payload),
  };
}

if (process.argv[2]) {
  const file = readFileSync(process.argv[2]);
  const input = process.argv[2].endsWith(".hex")
    ? Uint8Array.from(Buffer.from(file.toString("utf8").replace(/\s/g, ""), "hex"))
    : file;
  const frame = decodeDonf(input);
  console.log(JSON.stringify(frame, (_, value) => typeof value === "bigint" ? value.toString() : value, 2));
}
