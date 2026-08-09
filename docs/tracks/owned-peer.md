# Owned peer acceptance and retail TCP client

Status: the synthetic two-peer acceptance remains runnable, and the same binary now has a
bounded client mode for a retail process loaded with our replacement `CrossplayNetLib.dll`.
Neither mode carries an auth ticket, contacts matchmaking, or discovers strangers.

## Synthetic acceptance

From the repository root:

```sh
cargo run --quiet --manifest-path tools/owned-peer/Cargo.toml -- --turns 50
```

Success is one JSON line with `"status":"pass"`. The tool binds only `127.0.0.1`, creates
two peers whose visible name is exactly `Ai`, and fails closed unless all of these hold:

- the PDB-derived 70-byte `IPT_ADDPLAYER`, 34-byte `IPT_PLAYERLIST`, and 2-byte
  `IPT_READYFLAG` records encode/decode exactly;
- host-authoritative membership assigns unique IDs to slots 0 and 1 while both visible
  labels remain `Ai`;
- both peers cross the all-ready gate;
- the 46-byte `GameConnectionData` record and the recovered retail lobby-attribute fields
  round-trip;
- every turn crosses real TCP framing as two 73-byte `NETMSG_COMMANDPACKAGEDATA` messages;
- each message carries an exact 65-byte opcode-`0x39` `CheckSumsCommand` whose first fifteen
  words are Adler-32 values and whose final word is their wrapping sum;
- both peers receive one package from each slot and finish with the same transcript hash.

## Retail-connect mode

The retail process must be using the replacement `CrossplayNetLib.dll` as a TCP host with:

```text
DON_NET_ROLE=host
DON_NET_BIND=0.0.0.0:31337
DON_NET_ID=1
DON_NET_NAME=Ai
DON_NET_LOAD_ONLY unset
```

For the current Parallels VM (`10.211.55.6`), launch the owned client on the Mac with:

```sh
cargo run --quiet --manifest-path tools/owned-peer/Cargo.toml -- \
  --retail-connect 10.211.55.6:31337 --id 2 --turns 10 --timeout-secs 300 \
  --evidence owned-retail-run.donlstp
```

The command is deliberately bounded: it exits after ten checksum-bearing retail turns or
fails after 300 seconds with the partial roster/readiness/traffic counts. It connects only to
the supplied endpoint. There is no lobby search or credential path.

The client performs the replacement transport's exact direct contract:

1. the four-byte TCP ID handshake announces ID 2;
2. `Session::poll` sends `IPT_ADDPLAYER("Ai", 2, false)` and its current ready flag;
3. it refuses to proceed until the host's authoritative `IPT_PLAYERLIST` gives retail slot 0
   and the owned client slot 1, with exactly two members named `Ai`;
4. it sends `IPT_READYFLAG(true)` and reports whether the two-member all-ready gate fired;
5. it emits no game-layer package merely because the roster became ready, and refuses any
   incoming game traffic until the authoritative two-member all-ready transition;
6. it receives the first verbatim retail `NETMSG_COMMANDPACKAGEDATA`, decodes command opcodes and all
   sixteen checksum words, and emits one JSON event per turn;
7. it sends a slot-1 package for that same first stamp containing only a byte-identical copy of the
   retail checksum command.

Direct host/client frames retain the original `u32 length + payload` TCP contract. When the host
relays one client's packet to another client, the shared transport sets the high length bit and
prefixes the payload with the original sender's `i32` ID. The receiving transport removes that
four-byte envelope before setup or game decoding. This keeps `AddPlayer`, readiness, destruction,
and host-authority checks tied to the actual origin in sessions with more than two peers. The shim
and owned-peer binaries must therefore be rebuilt from the same `don-net` revision; a host rejects
a client-supplied relayed-frame marker instead of accepting a forged origin.

The first package is fail-closed: if its key cannot be recovered, use `--game-key`; if it has no
checksum command, the client stops instead of skipping the stamp or fabricating a reply.

`--evidence PATH` is opt-in and never changes the reactive safety boundary. The peer still emits no
game package until retail supplies the first authoritative stamp and checksum. After each accepted
host package it records those exact payload bytes, records the successfully sent slot-1 reply,
observes the configured turn deadline, and commits only when both packages are present. The first
roster becomes the initial setup epoch with `(slot, unique_id)` members. The final orderly
`IPT_DESTROYPLAYER` becomes a drop epoch.

The output is the shared `DONLSTP\0` v1 format decoded by
`don_net::PersistedLockstepTranscript`. Before publication, the peer replays every action through
`LockstepRunner`, requires the canonical outcome/desync JSON to equal the live recorder, then
requires decode/re-encode byte equality. Publication creates a synchronized temporary file beside
the destination and atomically links it into place. An existing destination is never replaced; a
failed or partial run produces no destination artifact. The format remains bounded to 8 MiB,
65,536 actions, 4 MiB of outcome JSON, and 512 bytes per command payload.

To exercise one explicit same-ID reconnect inside a longer bounded run, pass a completed-turn
boundary smaller than `--turns`:

```sh
cargo run --quiet --manifest-path tools/owned-peer/Cargo.toml -- \
  --retail-connect 10.211.55.6:31337 --id 2 --turns 10 --timeout-secs 300 \
  --reconnect-after 5 --evidence owned-retail-reconnect.donlstp
```

At turn 5 the peer sends the exact five-byte destroy record, records a host-only drop epoch,
reconnects with the same owned ID, waits for a fresh authoritative roster/readiness transition,
records the reconnect epoch, and only then resumes reactive packages. The option is active-mode
only and must satisfy `1 <= N < --turns`. Passive evidence is limited to one observed turn because
passive mode deliberately emits no slot-1 package with which to commit and advance the lockstep
stamp.

Multiplayer command packages are XORed and have a deterministic 0/1-byte pad after each
command. If `--game-key` is omitted, the client recovers a wire-compatible key by validating
candidate XOR keys and all 256 relevant pad seeds against the complete 82-opcode decoder and
the checksum invariant. The recovered key may be a canonical equivalent rather than the
literal 32-bit global: the transform exposes only bits 0..23, and a short package can leave
multiple low-byte seeds with the same observed pad prefix. Any accepted key reproduces the
exact observed package and the outgoing checksum-only payload. A live-read key can instead be
forced with `--game-key 0xG`.

`--passive` performs membership/readiness and reports traffic but sends no command package:

```sh
cargo run --quiet --manifest-path tools/owned-peer/Cargo.toml -- \
  --retail-connect 10.211.55.6:31337 --id 2 --turns 1 --timeout-secs 300 --passive
```

## Evidentiary boundary

Mirroring the host's checksum is a transport-splice diagnostic. It prevents the dummy peer
from inventing a different oracle value, but it is not an independently computed simulation
checksum and the JSON explicitly records `"simulation_equivalence_claimed":false`. Passing a
retail run establishes direct membership, readiness, inbound retail turn traffic, checksum
decoding, and an outbound package reaching retail through `NetSys::get`. It does not establish
PlayFab discovery, original Party transport, or headless simulation equivalence.

The repository test suite includes a mock-retail host over a real TCP socket. That test proves
the new mode observes `PlayerJoined` before `ReadyChanged(true)`, reaches an authoritative slot-1
roster, crosses all-ready, stays game-silent for multiple polls, automatically recovers the first
package transform, and returns an exactly decodable client checksum package for the identical
first stamp while preserving the original synthetic harness. The same run now atomically persists
one initial epoch, eight exact packages across four committed stamps, four deadline observations,
two orderly drop epochs, and one same-ID reconnect epoch; the test decodes and replays that file,
checks its binary/outcome hashes, requires byte-exact re-encoding, then removes the test artifact.

## Retail start and packet-loop boundary

The shipped start handoff is independent of the replacement transport. The exact host path is:

1. `SetupWin::on_button_clicked` reaches the start arm at `0x005C5F4A`, requires
   `check_all_ready(0)` at `0x005C5F70`, requires a non-observer local `NetPlayer`, verifies
   `NetSys::is_host`, then calls the SetupWin countdown virtual at `+0x31C`.
2. At countdown completion, `SetupWin::countdown` sets `NetSys::set_playing(1)` through vtable
   `+0x18` at `0x005BC5C0`, obtains the lobby ID, and calls
   `ICrossPlayService::StartGame` at service vtable `+0x5C` at `0x005BC855`.
3. `SetupWin::start_game_success(false)` at `0x005B7530` changes lobby type to `playing`, writes
   local `PlayerConnectionData.ready=2` at `0x005B7589`, calls
   `ConnectionData::send_player(0,false)` at `0x005B758D`, and transitions away from SetupWin.
4. `NetDaemon::process_all` at `0x00951300` repeatedly calls `NetDaemon::process` until it returns
   false. Each `process` calls NetSys `check_pulse` (`+0x3C`),
   `process_system_messages` (`+0x9C`), then `get` (`+0x5C`) at
   `0x00950F67..0x00950FA4`. Command-package case 7 relays client packages through
   `NetSys::send_all` at `0x009510E5..0x009510FA`; the turn gate consumes one package per slot.

The owned-peer mock-retail test now crosses that packet boundary for three consecutive stamps,
verifies silence before the first host package, mirrors only each stamp's extracted checksum,
sends the exact five-byte `IPT_DESTROYPLAYER`, observes authoritative `PlayerLeft`, reconnects
the same owned ID into slot 1, explicitly republishes readiness for the new setup epoch, and
completes a fourth stamp. Run it with:

```sh
cargo test --manifest-path tools/owned-peer/Cargo.toml \
  retail_connect_replies_repeatedly_disconnects_and_rejoins_cleanly
```

An unannounced socket loss is intentionally not called an orderly disconnect: the current
session protocol detects that case through its configured pulse timeout. Reconnect acceptance
therefore requires `IPT_DESTROYPLAYER` first and a new readiness exchange after membership.
