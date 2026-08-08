// Shared memory layout between the sim workers and the render worker.
//
// One SharedArrayBuffer holds, for each shard: a small control block and three banks of
// column bytes. There is no cross-shard barrier — shards run free and publish
// independently, so the renderer can be showing world 0 from sim frame 9001 and world 900
// from 9003. That is deliberate. This is a *spectator*, not a lockstep client; forcing a
// barrier would make the slowest shard set the throughput of every other one, which is
// exactly the property the batch architecture exists to avoid.
//
// # The triple buffer, and the race it removes
//
// Two banks are not enough. With two, the renderer reads bank A while the writer fills B;
// the writer then publishes B and immediately starts filling A — the bank still being
// read — and the renderer tears mid-upload. With three, the writer excludes both the
// published bank and the bank the reader has claimed, so a free bank always exists and
// the reader's bank is never written. The reader claims by storing its bank index into
// the control block before it reads and clearing it after, which is one atomic store on
// each side of an upload that is itself hundreds of microseconds.
//
// Everything here is `Int32Array` because `Atomics` only operates on integer views.

/** i32 slots per shard control block. Padded to 16 so each block is 64 B — one cache line,
 *  so two shards' counters never share one and false-share on every publish. */
export const CTRL_I32 = 16;

export const CTRL = {
  /** Bank index holding the newest complete frame, or -1 before the first publish. */
  PUBLISHED: 0,
  /** Bank the renderer has claimed for reading, or -1. Writers must not choose it. */
  READER_HELD: 1,
  /** Monotonic publish counter; the renderer uses it to detect a stalled shard. */
  SEQ: 2,
  /** Sim frame number of the published bank. */
  SIM_FRAME: 3,
  /** Live units in this shard. */
  LIVE: 4,
  /** Microseconds spent in `sim_step` for the last published round. */
  STEP_US: 5,
  /** Microseconds spent in `sim_gather` (the cluster mirror memcpy). */
  GATHER_US: 6,
  /** Microseconds spent copying wasm memory into the shared bank. */
  COPY_US: 7,
  /** Total sim frames advanced since start. */
  FRAMES: 8,
  /** Non-zero once the shard has finished creating its world set. */
  READY: 9,
  /** Set by the main thread to ask the shard to stop. */
  QUIT: 10,
  /** Sim frames the shard should advance per publish. */
  SPEED: 11,
  /** Reserved. Was a tag-column dirty flag back when the tag held only owner and
   *  occupancy; the tag now carries hit points, so it changes every frame and is published
   *  in the same triple buffer as the positions. */
  RESERVED_12: 12,
};

export const BANKS = 3;

/** Byte size of one bank for a shard covering `worlds` worlds at `stride` rows each.
 *  Three 4-byte columns: x, y, tag. All three are written every publish. */
export function bankBytes(worlds, stride) {
  return worlds * stride * 4 * 3;
}

/** Byte offsets, within a shard's region, of one bank's three columns. */
export function bankLayout(worlds, stride) {
  const cells = worlds * stride;
  return { xOff: 0, yOff: cells * 4, tagOff: cells * 8, cells };
}

/** Total SharedArrayBuffer size for `shards` shards of `worldsPerShard` worlds. */
export function planSab(shards, worldsPerShard, stride) {
  const ctrlBytes = shards * CTRL_I32 * 4;
  const shardBytes = BANKS * bankBytes(worldsPerShard, stride);
  return { ctrlBytes, shardBytes, total: ctrlBytes + shards * shardBytes };
}

/** Byte offset of shard `s`'s bank `b`. */
export function bankOffset(plan, s, b, worldsPerShard, stride) {
  return plan.ctrlBytes + s * plan.shardBytes + b * bankBytes(worldsPerShard, stride);
}

/** Pick a bank to write: neither the published one nor the one the reader holds. */
export function chooseWriteBank(ctrl, base) {
  const pub = Atomics.load(ctrl, base + CTRL.PUBLISHED);
  const held = Atomics.load(ctrl, base + CTRL.READER_HELD);
  for (let b = 0; b < BANKS; b++) if (b !== pub && b !== held) return b;
  return 0; // unreachable with BANKS === 3
}
