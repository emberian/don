# Shared don-net setup transcript acceptance

Status: executable over loopback TCP without retail, credentials, or VM access.

The replacement-netlib setup/control transcript is now decoded and enforced by `don-net`'s
shared `InternalPacket` and `Session` code. It is no longer behavior owned only by the
`owned-peer` harness.

## Exact packet boundary

The layouts and packing are measured from `CrossplayNetLib.pdb`. Each internal packet is a
complete transport datagram and must have exactly its measured `sizeof`:

| Packet | ID | Wire bytes | State effect |
| --- | ---: | ---: | --- |
| `PlayerList` | 128 | 34 | Host-authoritative count plus eight `i32` IDs |
| `AddPlayer` | 132 | 70 | 64-byte narrow name, `i32` ID, one-byte host flag |
| `DestroyPlayer` | 133 | 5 | Orderly `PlayerLeft` for one `i32` ID |
| `ReadyFlag` | 136 | 2 | One-byte readiness level |

The decoder fails closed on short or trailing bytes, counts above eight, zero or duplicate active
roster IDs, zero add/destroy IDs, and boolean bytes other than zero or one. The shipped
`CrossplayNetLibSys::send_playerlist` (VA `0x10017f30`) writes the type/count at
`0x10018018..0x10018025` and only the active ID prefix in its
`0x10018042..0x1001833e` loop. Tail dwords beyond `num_players` are therefore decoded but
deliberately ignored rather than mistaken for members. A `Session` consumes a refused
datagram without mutation and emits
`Event::SetupRefused { reason: SetupRefusal::Malformed(..) }` so a controller can retain the
evidence instead of silently dropping it.

## Authority and idempotence

- `AddPlayer.unique_id` must identify its transport sender. A remote host claim is rejected by a
  host, and a client rejects a second host claim after it has established one. An exact duplicate
  fills no new slot and emits no second `PlayerJoined`; conflicting name or host data is refused.
- Only a client accepts `PlayerList`. Slot zero must be the sender/host, the local player must be
  present, and later lists remain pinned to the established host. The TCP star marks relayed frames
  and carries their original `i32` sender ID, preventing a third peer from acquiring host authority
  merely because its bytes traveled through the host socket. An exact duplicate list is level
  state and emits no join/leave event.
- `ReadyFlag` is level state for an already materialized sender. The first change emits
  `ReadyChanged`; a duplicate value is silent. A flag before membership is refused rather than
  cached under an unowned ID.
- `DestroyPlayer.unique_id` must identify its sender. The first valid packet removes that member,
  clears its prior-ready state, and emits `PlayerLeft`; a duplicate is silent. A peer cannot remove
  another player by naming that player's ID.

The host republishes `AddPlayer` before its current `ReadyFlag` whenever it sees a fresh transport
peer. Orderly destruction, authoritative roster removal, and pulse timeout all clear the prior
announcement. A later TCP connection using the same owned ID is therefore a new membership epoch:
the roster is rebuilt, `PlayerJoined` precedes `ReadyChanged`, and no harness-side repair is needed.

## Runnable proof

```sh
cargo test --manifest-path crates/don-net/Cargo.toml --test setup_transcript
```

The test uses real `127.0.0.1` TCP sockets and checks initial authoritative slot assignment, exact
and conflicting duplicates, repeated readiness, host-only roster publication, a relayed takeover
attempt, malformed packet refusal without mutation, false and duplicate departure, readiness after
removal, and same-ID reconnect ordering. It emits no lobby ID, ticket, credential, or non-loopback
network traffic.
