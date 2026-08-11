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
   retail checksum command — or, when the stamp's package legitimately carries no checksum, a
   package carrying no commands.

Direct host/client frames retain the original `u32 length + payload` TCP contract. When the host
relays one client's packet to another client, the shared transport sets the high length bit and
prefixes the payload with the original sender's `i32` ID. The receiving transport removes that
four-byte envelope before setup or game decoding. This keeps `AddPlayer`, readiness, destruction,
and host-authority checks tied to the actual origin in sessions with more than two peers. The shim
and owned-peer binaries must therefore be rebuilt from the same `don-net` revision; a host rejects
a client-supplied relayed-frame marker instead of accepting a forged origin.

The first package is fail-closed on the key: if none is announced and none can be recovered, the
client stops instead of answering a turn it cannot read. Use `--game-key`, or run a shim host
that announces `GameInfo::seed`.

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

## The match key

Multiplayer command packages are XORed and have a deterministic 0/1-byte pad after each
command. Both transforms are driven by one 32-bit word, `Game::info.seed`, which
`CommandPackage::send` `0x0094c1e0` reads six instructions before it calls the `NetSys` send
slot. See [`docs/assembly/multiplayer-command-package-key.md`](../assembly/multiplayer-command-package-key.md).

The client takes that key from three places, in this order, and reports which one it used in
`"game_key_source"`:

1. **`retail-gameinfo-seed`** — the host announced it. A shim host inside
   `riseofnations.exe` reads the live seed inside its `NetSys` send slot and hands it to each
   peer over the `don_net::extension` transport extension (packet id `0xF0`, six bytes),
   immediately before the package that needs it. `Session` refuses an announcement from anyone
   but the authoritative host.
2. **`operator-supplied`** — `--game-key 0xG`. If the host later announces a key that is not
   wire-equivalent to it, the run fails rather than choosing between them. Equivalence is
   `key & 0x00ffffff`, because the XOR key is bits 8..23 and the pad generator reads only bits
   0..15; bits 24..31 cannot reach the wire.
3. **`ciphertext-ranking`** — the offline fallback, which validates candidate XOR keys and all
   256 relevant pad seeds against the complete 82-opcode decoder and the checksum invariant.
   The key it returns is a representative of the equivalence class above, not the literal
   global, and it reproduces the exact observed package and outgoing payload.

Ranking **cannot** settle the first turn of a live match, at any payload length: it is anchored
on the checksum invariant, and `CommandManager::start` `0x00942E10` sends the first package of
every match without ever calling `CommandManager::issue_check_sums` `0x00940770`. See
[`docs/assembly/retail-command-package-cadence.md`](../assembly/retail-command-package-cadence.md).

## The pre-match sync barrier

Before any of that runs, retail has to *start*. `SyncPoint::sync` `0x0093a2d0` is a blocking
rendezvous: it bumps the local `NetPlayer`'s counter, increments the global
`SyncPoint::counter` `0x00cbee7c`, broadcasts one 5-byte `NETMSG_SYNCSIGNAL` (id 10) through
`NetDaemon::send_sync_signal` `0x00950c50`, then spins on `NetDaemon::process_all` until every
`NetSys` player bound to a `GameInfo::player[j].net_player` reports
`NetPlayer::get_sync_counter() >= SyncPoint::counter`. A remote peer's counter moves only when
retail *receives* a sync signal from it: `NetDaemon::process` `0x00950f30` jump-table entry 10
calls `SyncPoint::process_sync_signal` `0x0093a150`, which increments the counter on the
sender's `NetPlayer` — an object that lives inside our shim.

A live peer that classified id 10 as opaque and stayed silent left a retail host on "Starting
Game" forever, with the shim trace ending at `netplayer.get_sync_counter`. The whole fix is
one-for-one: **on every sync signal from the authoritative host, the peer sends one back**,
echoing the received `play` verbatim. Retail does its own counting; the shim deliberately does
not touch `sync_counter` on the peer's behalf. A clean start crosses seven of these barriers
before the first command package (`Game::run` `0x00584590` issues four, `CommandManager::start`
`0x00942e10` one, and `TimeSync::sync` `0x00955060` two) and two more right after it, so the
peer must keep answering rather than answer once.

The run record reports `"sync_signals_answered"`, each reply prints a `"sync-signal"` event,
and the shim's diagnostic log carries `sync_signal=out`, `sync_signal=in`, and
`sync_counter=inc` lines. The derivation is
[`docs/assembly/retail-sync-point-barrier.md`](../assembly/retail-sync-point-barrier.md).

Answering is not gated on `--passive`: the reply carries no simulation state — retail's
handler provably never dereferences the record — and a passive observer that withholds it
stalls the host it is observing.

## Reply policy

One package per stamp, in one of two shapes, neither of which invents simulation state:

- the stamp's package carried a `0x39` checksum → a byte-identical copy of it, re-obfuscated
  for slot 1 (`"policy":"mirror-retail-checksum"`);
- it decoded exactly but carried no checksum → a package carrying no commands
  (`"policy":"empty-package"`, counted in `"empty_replies"`). This is the truthful "this peer
  issued nothing this turn", it is a legal retail record — `CommandPackage::send` handles
  `size == 0` and emits the 8-byte header alone — and it is the one package a peer can always
  form correctly, because an empty payload has no words to XOR and no pad to get wrong.
  `CommandManager::start` produces exactly one such stamp per game.

The peer still fails closed on everything it cannot read: no key at all, a package that does
not decode under the key in use, a checksum whose sixteenth word is not the wrapping sum of
the first fifteen, or two mutually incompatible keys.

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
first stamp while preserving the original synthetic harness. Two further tests cover the key
path: one announces a seed and then sends a checksum-less `TurnDataCommand`-only start package,
requiring the peer to decode it from the announced key and answer with a zero-length package
before mirroring the next stamp's checksum; the other announces a key that contradicts
`--game-key` and requires the run to fail with both values named. The same run now atomically persists
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

Between step 3 and step 4 sits the sync barrier described above: `SetupWin` hands off, then
`Game::run` and `CommandManager::start` each block in `SyncPoint::sync` until every peer's
`NetPlayer::get_sync_counter()` has caught up with `SyncPoint::counter`. `NetDaemon::process_all`
is what pumps the network inside that spin, which is why the barrier and the packet loop share
the same call frontier.

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
