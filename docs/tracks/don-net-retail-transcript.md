# Shared don-net retail transcript acceptance

Status: executable without retail, credentials, or VM access.

The Phase-E owned-peer transcript is now consumed by the shared `don-net` state machine rather
than only by the standalone harness. The fixed transcript contains retail-style command packages
for stamps 23, 24, and 25, followed by a same-owned-ID reconnect epoch and stamp 26.

## Exact boundary

- `NETMSG_COMMANDPACKAGEDATA` is 8 bytes of message header plus payload.
- The checksum-only payload is one exact 65-byte `CheckSumsCommand`, opcode `0x39` followed by
  sixteen little-endian `u32` values.
- Each fixed transcript packet is therefore exactly 73 bytes before TCP's length prefix.
- Multiplayer XOR uses `(game_key >> 8) as u16` over whole words. Inter-command padding restarts
  from the same game key for every package.
- `Session` stores packages by `(stamp, play)` and refuses turn readiness until both slot 0 and
  slot 1 exist for that stamp.
- `decode_retail_checksum_package` applies the retail transform, requires exactly one checksum
  command, preserves the package stamp/slot, and rejects payloads beyond the measured 512-byte
  `CommandPackage::data` capacity.

## Membership epochs

`Session::announce_disconnect` emits exact `IPT_DESTROYPLAYER`: one type byte plus the local
four-byte ID. Receiving removal now also forgets the old transport announcement. Consequently a
new TCP channel using the same owned ID is a new membership epoch: the host republishes
`IPT_ADDPLAYER` and then its current `IPT_READYFLAG`, the reconnecting client returns its own
readiness, and only then can the next two-package turn become ready.

The same announcement reset occurs for authoritative roster removal and pulse timeout. An
unannounced socket close remains timeout-driven and is not mislabeled as orderly departure.

## Runnable proof

```sh
cargo test --manifest-path crates/don-net/Cargo.toml --test retail_transcript
```

The test uses real loopback TCP sockets and asserts:

1. `PlayerJoined` precedes `ReadyChanged(true)` for the first membership epoch;
2. all three captured checksum arrays cross the Session lockstep gate in exact 73-byte messages;
3. both slot packages decode to the same stamp and checksum through the shared retail decoder;
4. the client sends five-byte orderly destruction and the host observes `PlayerLeft`;
5. the same ID reconnects at slot 1 without harness-side roster repair;
6. add/readiness ordering repeats and stamp 26 completes after reconnect.

No authentication material, lobby identifier, Steam ticket, or network endpoint outside
`127.0.0.1` is used.

## Shared lockstep runner

`don_net::LockstepRunner` now owns the evidence boundary above `Session` and below simulation.
It accepts the `TurnPackage` values taken from a session, applies the shared retail checksum
decoder, and advances exactly one authoritative stamp only after every slot in the current epoch
has submitted one package. It does not decode or execute non-checksum simulation commands.

The runner provides these fail-closed rules:

- participant slots are sorted, unique, and limited to `0..7`;
- stale or future stamps, packages from slots outside the current epoch, and conflicting duplicate
  packages are refused without changing the clock;
- an exact duplicate package is idempotent;
- the lowest participating slot is the checksum reference, and every differing channel/slot pair
  is retained as `ChecksumDifference` evidence in deterministic channel order;
- each turn has a caller-clock deadline; reaching it records one `TurnTimeoutEvidence` with the
  exact received and missing slots, and late packages remain refused until an explicit membership
  epoch begins;
- a drop epoch may retain packages already received from surviving slots, while a reconnect epoch
  adds the returning slot without resetting the authoritative stamp;
- only `commit_ready` advances the stamp, and overflow is refused.

`LockstepRunner::export_json` emits canonical `don.lockstep-transcript.v1` JSON: chronological epoch,
timeout, and turn records; sorted slots; payload byte counts and FNV-1a hashes; all sixteen checksum
words; and named channel-level desync evidence. `transcript_fnv1a64` hashes those exact JSON bytes.
The adversarial unit transcript is pinned to `f4cbbb493b3a6d3e`. The real TCP transcript covering
stamps 23–25, drop/reconnect, and stamp 26 is pinned to `438bc8d31e903ea7`.

Run both boundaries with:

```sh
cargo test --manifest-path crates/don-net/Cargo.toml \
  lockstep::tests::authority_desync_timeout_drop_and_reconnect_are_evidentiary
cargo test --manifest-path crates/don-net/Cargo.toml --test retail_transcript
```

Timeout is evidence, not an invented retail drop vote. The caller must supply an authoritative
membership change before the runner removes a missing slot. Likewise, checksum differences prove
package disagreement but do not claim that a simulation state has been independently reproduced.

## Persisted binary evidence

`PersistedLockstepTranscript` is the bounded, versioned persistence boundary for the same runner.
Its v1 little-endian header is:

| Offset | Bytes | Field |
| ---: | ---: | --- |
| `0x00` | 8 | magic `DONLSTP\0` |
| `0x08` | 2 | version `1` |
| `0x0A` | 2 | flags, required zero |
| `0x0C` | 4 | action count |
| `0x10` | 4 | canonical outcome JSON byte count |
| `0x14` | 8 | outcome JSON FNV-1a-64 |
| `0x1C` | 4 | package transform/game key |
| `0x20` | 8 | turn deadline duration in milliseconds |

The header is followed by `u8 type + u32 payload_bytes + payload` action records and then the exact
canonical outcome JSON. Record types are initial setup, membership epoch, package, deadline
observation, and commit. Initial/epoch records carry their monotonic caller time and exact sorted
member list; each member is a slot `i8` plus its nonzero `unique_id i32`, so a reconnect can prove
that a newly active slot belongs to the same owned identity. Epoch records also carry
drop/reconnect/roster-change cause. Package records carry time, stamp, slot, `u16` payload length,
and every original payload byte. Deadline and commit records carry their caller time. Thus the
persisted source contains both the executable evidence inputs and the expected lockstep outcomes
rather than trusting a conclusion-only log.

The reader refuses unknown versions, flags, records or epoch causes; malformed lengths; trailing
bytes; non-monotonic time; repeated/missing initial setup; packages over the retail 512-byte
capacity; non-UTF-8 outcomes; and any stored hash mismatch. It is bounded to 8 MiB total, 65,536
actions, and 4 MiB of outcome JSON. Decode then replays every action through `LockstepRunner` and
requires the regenerated JSON, transcript hash, next stamp, epoch transitions, timeout evidence,
and channel-level desyncs to match exactly. Re-encoding a decoded transcript must reproduce every
input byte.

The adversarial binary lifecycle is pinned to `725cfd99ebe60b0c`. The real TCP package/epoch binary
for stamps 23–26 is pinned to `26d94d663d140bba`, while its re-executed canonical outcome remains
`438bc8d31e903ea7`.

```sh
cargo test --manifest-path crates/don-net/Cargo.toml evidence::tests
cargo test --manifest-path crates/don-net/Cargo.toml --test retail_transcript
```
