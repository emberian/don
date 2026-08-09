# DONF telemetry protocol v1.0

## Transport and admission

A stream is a sequence of little-endian `u32 length` plus one complete DONF frame. A WebSocket
may instead carry exactly one frame per binary message. A `.donfeed` file begins with a `Hello`
frame at sequence zero and then records frames without rewriting them.

Every consumer performs these operations in order:

1. bound the outer length;
2. decode and verify magic, supported major, exact payload length, and CRC-32/ISO-HDLC over
   header metadata plus payload;
3. decode bounded TLVs, rejecting missing required tags, wrong known wire types, malformed
   booleans, invalid integer widths/ranges, and duplicate known scalar tags;
4. run semantic `Validate` before admitting the message to history or analyzers.

The CRC detects damaged or truncated recordings. Transport authentication is separate: the
host-only connection must use the run token described in the rontoy architecture. CRC is not
authentication.

## Fixed frame header

| Offset | Size | Field |
|---:|---:|---|
| 0 | 4 | ASCII `DONF` |
| 4 | 2 | protocol major |
| 6 | 2 | protocol minor |
| 8 | 2 | message kind (`1` hello, `2` snapshot, `3` events, `4` heartbeat) |
| 10 | 2 | reserved; nonzero is rejected in v1 |
| 12 | 4 | flags |
| 16 | 8 | sequence, monotonic within session |
| 24 | 8 | signed Unix milliseconds when sent |
| 32 | 16 | session/stream ID |
| 48 | 4 | payload byte length |
| 52 | 4 | CRC-32/ISO-HDLC over header bytes 0..52 plus payload |

The 56-byte header is followed by one TLV record. A TLV header is `tag:u16`, `wire_type:u8`,
`flags:u8`, `length:u32`, followed by `length` bytes. Wire types are bytes `0`, unsigned 64-bit
`1`, signed 64-bit `2`, UTF-8 `3`, and nested record `4`.

## Compatibility

- A larger major is rejected during decode. A larger minor is accepted.
- New optional data uses a new tag. Existing tag meanings and units never change within a
  major.
- Unknown message kinds retain their complete payload. Unknown fields retain tag, wire type,
  flags, and bytes and are emitted again by relays.
- Omitted required tags and known tags with a wrong wire type are rejected rather than
  normalized into plausible defaults.
- Known scalar tags may appear once. Repeated fields are explicitly designated by their
  record (`players`, `resources`, `entities`, queue items, warnings, advice, events, etc.).
- Absence means unavailable. Zero is an observed value, never a substitute for failed reads.

## Capture coherence and provenance

`SnapshotMeta` contains the capture timestamp/duration, frame counter sampled before and after
root reads, retry/drop counts, capability and component-validity bitsets, and read health.
`Coherent` requires equal frame guards and validated stable roots. `Paused` means the game was
already paused; the probe must never suspend the game to manufacture coherence.
`IncoherentDropped` and `SourceLost` are health-only states.

Advice is admitted only for confirmed single-player mode, one identified local human,
own-player-only scope, no opponent or hidden entities, and a process-memory source with build,
PID, process-start, and image-base identity. Coherent multiplayer telemetry remains valid
telemetry but cannot carry advice. Every entity declares its visibility basis.

`Source` identifies process memory, replay, network, derived, or user input and carries build,
PID, process-start identity, image base, and capture identity. `Evidence` references the source
table and adds data-quality confidence (0..10,000 basis points), freshness, flags, and a versioned
calibration/measurement method. A non-endpoint score without that method is invalid; it is not
an uncalibrated claim about advice success probability. Consumers must display or log provenance
for advice. Advice also carries a stable rule ID, rule version, and lifecycle.

## Retail units and economy semantics

The probe preserves retail values:

- resource indices: food `0`, timber `1`, wealth `2`, knowledge `3`, metal `4`, oil `5`;
- stockpile: decrypted retail integer from the leader data;
- income/spend: sixteenths of a resource per retail gather period;
- positions: retail fine-world coordinate integers;
- build/queue progress: parts per million where `1_000_000` means complete;
- confidence and advisor priority: basis points where `10_000` means 100%.

`RateBasis::EngineDirect` is the engine-maintained display/gather cache and the source detail
must say whether it is unbonused. `ObservedDelta` is confounded by spending, tribute, and
refunds. `Modeled` is a host calculation. These signals must not be silently substituted for
one another. `gather_stamp` and `gather_cache_age_frames` expose stale engine caches.
Per-resource optional fields carry gross, support, bonus, leftover, cap, and over-cap status;
`ResourceKind::Custom(6)` is the seventh commerce-cap entry. Player records also carry optional
city/unit/building aggregates and validity-marked per-type queue counts.
Over-cap remains a raw status: retail status 2 is the pre-interest commerce clamp when its cap
exceeds `0x3e6f`, not a global `income <= cap` invariant. Post-cap Dutch interest may make the
displayed income exceed that cap, so validators intentionally impose no such comparison.

The host is responsible for a single normalized JSON/SSE projection for the web UI. The schema
and NDJSON example beside this document specify that projection. JSON numbers which may exceed
JavaScript's safe integer range are strings.
