# Local multiplayer service lifecycle

This track asks one narrow question: can two DoN-owned peers execute an
observable `host → join → ready → start → turn` lifecycle without a retail
process or a remote account service?

## Current answer

**Yes at the connected-session boundary and at the explicitly configured
discoverable-lobby boundary. The two are not yet one stitched product path.**
Those are deliberately separate claims.

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
cd crates/don-crossplay
cargo test --features std-rpc --test directory_rpc_process
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

## The discoverable-lobby boundary

`don-crossplay` now has an optional DoN-owned loopback directory authority and
a worker-thread client behind `AsyncDirectory`. The subprocess gate launches
two real OS processes with independent `Backend` service state:

1. process A starts the authority, starts its service, and creates a public
   lobby carrying `game_seed`;
2. process B starts a separate service and requires `FindLobbies` to return
   A's lobby id;
3. B joins that id and requires the returned roster to be exactly A then B.

That test necessarily fails with process-private storage. A separate vtable
test constructs `LocalCrossPlayService::remote`, calls `StartSession` through
the shipped 58-slot table, and proves the first `Tick` only submits the RPC:
the completion journal and `Started` transition appear on a later vtable
`Tick`. The socket worker never invokes a game callback and only one answered
operation is in flight, so callback order remains submission order. No slot,
DTO, or calling convention changed.

The protocol is length-framed, versioned, bounded, and carries DoN semantic
types rather than MSVC DTO bytes. It is explicitly **not** a claim about a
retail PlayFab, Party, or `CrossplayProxy.dll` wire format.

Mailbox delivery is paged under the same directory mutex as the operation:
one response carries at most 1,024 notices and never exceeds the 2 MiB frame
bound. Selection sizes every notice before removal and returns a
`more_notices` bit, so the Tick adapter polls continuation pages before
submitting the next ABI request. Mutation-sensitive socket tests pin the two
previous failure shapes: two legal 1 MiB packets arrive in order across two
pages, and 4,097 notices cross five pages without exceeding the decoder's
4,096-item collection cap. Detached send/stop results remain observable events;
their outcome has no ABI callback, but their inbound PeerOpened/Data notices
still run on Tick.

Each connection must establish one exact `Member` identity with
`StartSession`. The authority rejects an already-active user id, rejects every
later frame whose Member differs, and does not drain the claimed user's mailbox
on rejection. Stop and disconnect release membership, mailbox state, and the
active-id claim. This is local-process isolation for DoN's loopback protocol,
not authentication against a hostile user on the same machine.

The replacement DLL selects the adapter only when configured:

```text
DON_CROSSPLAY_DIRECTORY=listen:127.0.0.1:PORT   # authority/host process
DON_CROSSPLAY_DIRECTORY=connect:127.0.0.1:PORT  # joining process
```

Only numeric loopback addresses are accepted. An unset variable preserves the
old process-private directory. A configured endpoint that cannot bind/connect
fails requests asynchronously; it does not silently fall back to a private map.

The DLL owns and joins its RPC worker, authority accept thread, and accepted
connection threads. A DoN-only `don_crossplay_shutdown` export releases Service
and Logger after the caller has quiesced interface calls; the PE32 Wine gate
configures `listen:127.0.0.1:0`, calls that hook, verifies callback ownership is
empty, and then succeeds at `FreeLibrary`. The statically imported game has a
process-lifetime DLL and does not use this diagnostic export.

## What remains

This tranche proves only the service discovery transaction
`CreateLobby → FindLobbies → JoinLobby` across processes. The same protocol has
operations for the existing update/start/P2P directory semantics; P2P paging,
detached completion, and identity isolation have real loopback-socket tests but
not yet an equivalent two-executable match test. The service-discovery result
is not yet wired to `don-net::LocalMatch` as one executable
`host → join → ready → start → turn` product path. Do not call
`CrossplayProxy.dll` a complete local multiplayer service yet.

No VM, retail process, injected image, or protected live-state file was touched
for this track.
