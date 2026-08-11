# "Starting Game" forever: `SyncPoint::sync` is a blocking rendezvous

Status: **read off the supported executable** (SHA-256 `30478a44…625079`), 2026-08-10.
This documents retail's own code. It is not a tier claim about any DoN Rust code, and no
live match has yet crossed the barrier described here.

## The observation this explains

A DoN peer joined a retail-hosted lobby through the replacement `CrossplayNetLib.dll`, took
slot 1, readied, and crossed the two-member all-ready gate. The human host pressed Start.
**Retail then hung on "Starting Game" forever.**

The shim trace ends:

```text
seq=52 call=netplayer.is_local
seq=53 call=vtable.ns_update_recently_played_with_list
seq=54 call=vtable.ns_get_allow_timeout
seq=55 call=netplayer.inc_sync_counter
seq=56 call=vtable.ns_send_all
seq=57 call=netplayer.get_sync_counter
```

and the peer logged exactly one thing after all-ready:

```json
{"event":"game-message","from":1,"id":10,"bytes":5}
```

Those six lines are `SyncPoint::sync` `0x0093a2d0`, in order, and the trace stops there
because `trace_once` had already fired for every slot the loop revisits. The hang is not a
crash and not a lost packet: retail is spinning, waiting for a number only the peer can
move.

## `SyncPoint::sync` `0x0093a2d0`

`void __thiscall SyncPoint::sync(class String const &)`, from the PDB. Reading the
instructions rather than the decompiler, because the loop structure is the whole point:

```text
0093a2e9  cmp  dword ptr [0xee12c8], 0     ; re-entrancy flag; already syncing -> return
0093a351  call dword ptr [eax + 0xb8]      ; NetSys::log_connection2(name, SyncPoint::counter)
0093a35f  call dword ptr [eax + 0x4c]      ; NetSys::get_allow_timeout()   -> saved
0093a370  call dword ptr [edx + 0x48]      ; NetSys::set_allow_timeout(1)
0093a3f1  mov  ecx, dword ptr [ecx + 0x28] ; NetSys::local_player
0093a3fa  call dword ptr [eax + 0x48]      ;   local_player->inc_sync_counter()
0093a3fd  inc  dword ptr [0xcbee7c]        ; ++SyncPoint::counter
0093a403  call 0x950c50                    ; NetDaemon::send_sync_signal()

0093a415:                                  ; <-- the spin
0093a41a  call 0x951300                    ; NetDaemon::process_all()
0093a41f  call 0x959e40                    ; DropControl::process_time_outs()
0093a42d  call 0x5917e0                    ; Game::loop_render(_, 2)
          ; for i in 0 .. NetSys::num_players:
0093a453  mov  eax, dword ptr [ebx + edi]  ;   NetSys::players[i]
0093a468  cmp  dword ptr [eax], ecx        ;   find j with game.info.player[j].net_player == players[i]
0093a46d  add  eax, 0x8c                   ;     sizeof(Player) = 140
0093a47f  call 0x95a090                    ;   DropControl::check_for_drop(j) — nonzero skips the player
0093a48d  call dword ptr [eax + 0x4c]      ;   players[i]->get_sync_counter()
0093a490  cmp  eax, dword ptr [0xcbee7c]   ;   ... vs SyncPoint::counter
0093a496  jge  0x93a4a2                    ;   caught up -> next player
0093a4a0  je   0x93a4db                    ;   behind and not dropping -> jmp back to 0093a415
0093a4c5  call dword ptr [eax + 0x48]      ; NetSys::set_allow_timeout(saved)
```

Symbols, from `ron-bin/sbl/rise.pdb` and `schema/types.json`:

| address | symbol |
|---|---|
| `0x0093a2d0` | `public: void __thiscall SyncPoint::sync(class String const &)` |
| `0x00cbee7c` | `public: static unsigned long SyncPoint::counter` |
| `0x00e335c8` | `class NetSys *netsys` |
| `0x00c061ec` | `public: static class Game &GameAccess::game` |
| `Game::info` +12, `GameInfo::player` +56 | `Player[8]`, `sizeof(Player)` = 140 |
| `Player::net_player` +124 | `const NetPlayer*` — hence `game + 0xc0 + j*0x8c` |
| `NetPlayer` vtable `+0x44/+0x48/+0x4c` | `reset_sync_counter` / `inc_sync_counter` / `get_sync_counter` |

**The barrier condition is exactly**: for every `NetSys::players[i]` that is bound to some
`GameInfo::player[j].net_player` with `j < 8` and is not being dropped,
`NetPlayer::get_sync_counter() >= SyncPoint::counter`. The *local* player satisfies it
trivially — `0x0093a3fa` bumps it one instruction before `0x0093a3fd` bumps the global. A
remote player satisfies it only when something advances its counter.

`SyncPoint::clear_counters` `0x0093a290` is the only reset: it zeroes the global and calls
vtable `+0x44` on every player. Its sole caller is `Game::close`.

## What retail sends, and what it does on receipt

`NetDaemon::send_sync_signal` `0x00950c50` builds the record on its own stack:

```text
00950c5f  mov  byte ptr [ebp - 8], 0xa      ; GenericNetPacket::type = 10 (NETMSG_SYNCSIGNAL)
00950c7c  mov  ecx, dword ptr [eax + 0x2a0] ; Console::play  ([0x00c06210] is MiscAccess::console)
00950c9c  mov  dword ptr [ebp - 7], ecx     ; NetMsg_SyncSignal::play, offset 1
00950ce9  push 5                            ; the wire length
00950cec  call dword ptr [eax + 0x58]       ; NetSys::send_all(record, 5, 1)
```

Five bytes; `play` is the sender's index into `GameInfo::player[8]`. `NetMsg_SyncSignal` in
the PDB is `size = 5`, `play` at offset 1 — which is what `don_net::msg` already encodes.

On the receive side, `NetDaemon::process` `0x00950f30`:

```text
00950f9c  mov  eax, dword ptr [eax + 0x5c]  ; NetSys::get(&data, &sender_netplayer, &size)
00950fa6  mov  eax, dword ptr [esp + 8]     ; the resolved NetPlayer*
00950fac  je   0x951271                     ;   null sender -> consume and ignore
00950fb2  movzx ecx, byte ptr [esi]
00950fb5  and  ecx, 0xffffffbf              ; mask off NETMSG_RESPONSE_FLAG (64)
00950fc1  jmp  dword ptr [ecx*4 + 0x951280]

009511b8:                                   ; jump-table entry 10
009511c5  mov  ecx, eax                     ; ECX = the sender's NetPlayer*
009511c7  call 0x93a150                     ; SyncPoint::process_sync_signal
```

`SyncPoint::process_sync_signal` `0x0093a150`, in full:

```text
0093a16c  mov  ebx, ecx                     ; the NetPlayer* — the ONLY input it reads
0093a170  test ebx, ebx / je  0093a276      ; null sender -> nothing
0093a17d  cmp  ebx, dword ptr [eax + 0x28]  ; == NetSys::local_player -> nothing
0093a18a  call 0x460c70                     ; NetPlayer::get_name  (for the log line)
0093a209  call dword ptr [eax + 0x4c]       ; player->get_sync_counter()  (logged)
0093a273  call dword ptr [eax + 0x48]       ; player->inc_sync_counter()
0093a286  ret                               ; __cdecl, no stack args touched
```

Two facts fall out of that listing, and both are load-bearing:

1. **The receiver advances the sender's counter.** Nothing else in the build calls
   `inc_sync_counter` on a remote player. `SyncPoint::sync` bumps only `NetSys::local_player`.
2. **The message body is never read.** The PDB signature is
   `static void __cdecl SyncPoint::process_sync_signal(struct NetMsg_SyncSignal *, class
   NetPlayer const *)`, but the emitted function takes its `NetPlayer*` in ECX and never
   dereferences a message pointer — the call site at `0x009511c5` does not even pass one.
   `NetMsg_SyncSignal::play` is therefore inert on the only receive path this build has.

The `0xBF` mask means id 10 and id `10|64` land on the same handler, so a reply marked with
`NETMSG_RESPONSE_FLAG` and a bare signal are indistinguishable to retail. Retail's own
emission at `0x00950c5f` writes a bare `0x0a`.

## So the hang is one missing 5-byte packet

Retail's `NetPlayer` objects for this session **live inside our shim** (`NetPlayerObj`,
`crates/netsys-shim/src/abi.rs`, `sync_counter` at +164). The barrier reads
`sync_counter` through `np_get_sync_counter`; the peer never replied, so it stayed 0 while
`SyncPoint::counter` was 1, and `0x0093a4a0` jumped back to `0x0093a415` forever.

**Who advances the counter: retail does, on receipt, inside its own dispatcher.** The shim
must not do it. The peer's entire obligation is to put one sync signal back on the wire per
sync signal received; retail then runs `SyncPoint::process_sync_signal` against the
`NetPlayer*` that `ns_get` resolved for the sender and does its own arithmetic.

That is implemented in `tools/owned-peer/src/main.rs`: on `NetMsg::SyncSignal { play }` from
the authoritative host, the peer sends `NetMsg::SyncSignal { play }` straight back and counts
it in `sync_signals_answered`. The `play` word is echoed verbatim rather than invented: it is
provably unread by the receiver, and a peer outside the process cannot read
`Console::play`.

## How many barriers a start crosses

`SyncPoint::sync` has 17 call sites. On the start path:

| caller | sites | note |
|---|---|---|
| `Game::run` `0x00584590` | 4 (`0x584609`, `0x584ccd`, `0x584cfc`, `0x585025`) | all at addresses preceding the `CommandManager::start` call at `0x5851c0` |
| `CommandManager::start` `0x00942e10` | 3 (`0x942e79`, `0x942f1f`, `0x942f49`) | the first is **before** `issue_turn_data`/`send_local_package` at `0x942f01`/`0x942f0b` |
| `TimeSync::sync` `0x00955060` | 2 | reached from `CommandManager::start` `0x00942e86` |
| `GameMods::download`, `SyncFile::sync`, `CommandManager::recover_from_oos`, `DropControl::process_need_syncs` | 8 (2 each) | on demand, not on a clean start |

That is seven barriers before the first command package leaves `CommandManager::start`, and
two more immediately after it, which is why the peer must answer **one-for-one and keep
answering** rather than answer once.

`TimeSync::sync` also contains a bounded ping wait, but only on the **non-host** arm
(`0x00955060` branches on `NetSys::is_host`), and that loop breaks after 5000 ms regardless.
It cannot be the cause of an indefinite host-side hang.

## Consequences and non-claims

- The first command package is downstream of at least one barrier, so nothing about
  `GameInfo::seed`, the package key, or the checksum cadence was exercised or disproved by
  the hang recorded above. See
  [`multiplayer-command-package-key.md`](multiplayer-command-package-key.md) and
  [`retail-command-package-cadence.md`](retail-command-package-cadence.md).
- Ids 9 (`TAUNT`) and 11 (`DROPSTAMP`) reach the no-op tail `0x00951271`; id 10 does not.
  A peer that treats every unrecognised game message as opaque will hang exactly here.
- Nothing here has been executed against a live match. The barrier logic is read off the
  disassembly and the PDB layout; the peer's reply is covered by unit tests and a
  mock-retail TCP test, which is Tier C evidence about *our* behaviour and no evidence at
  all about retail's until a live Start proceeds.

## Not determined

- Whether any producer in this build sets `NETMSG_RESPONSE_FLAG` on a sync signal. The
  dispatcher masks it before the switch and `0x0093a150` never reads the byte, so it cannot
  matter for id 10; whether some other id's handler distinguishes it is untested.
- Whether a barrier can be crossed by `DropControl::check_for_drop` returning nonzero for a
  silent peer instead of by that peer replying. The call at `0x0093a47f`/`0x0093a499` makes
  it structurally possible; the live run hung rather than dropping, so on that timeline it
  did not happen within the observed window.
- What `Console::play` holds for an observer or a not-yet-assigned slot. Irrelevant to the
  receive path, but it would matter if a peer ever had to originate a barrier of its own.
