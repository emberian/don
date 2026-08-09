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
