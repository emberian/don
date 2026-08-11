# Local multiplayer service lifecycle

This track asks one narrow question: can two DoN-owned peers execute an
observable `host → join → ready → start → turn` lifecycle without a retail
process or a remote account service?

## Current answer

**Yes at the connected-session boundary; no at the discoverable-lobby
boundary.** Those are deliberately separate claims.

`crates/don-net::LocalMatch<TcpTransport>` and the `donnet-peer` executable now
exercise the complete connected sequence on loopback:

1. a host binds and a client joins;
2. the recovered `IPT_ADDPLAYER` / `IPT_PLAYERLIST` path converges on one roster;
3. both peers publish the recovered two-byte `IPT_READYFLAG` and reach
   all-ready;
4. only the host can emit the DoN-owned `MatchStart { epoch, seed }`
   transaction, and the receiver accepts it only after all-ready;
5. `LocalMatch::send_turn` refuses before that transaction and then both peers
   exchange one package per slot for the same turn.

The focused gate is:

```sh
cargo test -p don-net --test local_match_lifecycle
cargo test -p don-net --test donnet_peer_cli
```

The match-start packet is explicitly DoN policy. It occupies extension id
`0xF1`, outside the shipped `CrossplayNetLib` internal range `128..=136`; it is
consumed by `Session` and never reaches the game-message queue. No shipped
packet or PlayFab behavior is claimed for it.

## Why this was the first connected-session gap

Before this tranche, the pieces on either side were already real:

- `don-net` and `tools/owned-peer` hosted/joined over TCP, established an
  authoritative roster, converged readiness, and exchanged lockstep turns;
- `don-crossplay` exposed the shipped 58-slot ABI and implemented
  `ICrossPlayService::StartGame` in its DoN-owned backend.

But the synthetic local peer jumped directly from `all_ready()` to
`send_command_package(0, ...)`. There was no transaction joining the two
states. A passing turn test therefore could not prove a start occurred.
`LocalMatch` makes that absence unrepresentable at its API boundary.
`donnet-peer` uses that boundary directly and emits `match_started` before its
first `turn`; the CLI test launches the real host and join executables and
fails if that ordering disappears.

## The remaining first service blocker

`don-crossplay` still constructs the DLL singleton with
`LocalCrossPlayService::new`, which owns a private `Directory`. Two service
instances can share a directory only when a test passes the same pointer to
`LocalCrossPlayService::joined`; two OS processes cannot. Consequently:

- process A can `CreateLobby` successfully;
- process B's first `FindLobbies` necessarily reads a different empty map;
- no cross-process `JoinLobby`, attribute update, `StartGame`, or P2P mailbox
  can follow through that service.

The next service transaction is therefore not another turn packet. It is a
bounded local directory RPC—at minimum `CreateLobby` followed by
`FindLobbies`/`JoinLobby` across two processes—then an adapter that lets the
DLL's existing asynchronous `Tick` completion path use it. Until that exists,
do not describe `CrossplayProxy.dll` itself as a complete local multiplayer
service.

No VM, retail process, injected image, or protected live-state file was touched
for this track.
