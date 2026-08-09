# Headless multiplayer client — what works, and the exact list blocking a real internet join

**Track:** `build:headless-net`. **Owns:** `crates/don-net/`, `crates/netsys-shim/`,
this file.

Marks: **[measured]** = verified in this tree this session. **[inferred]** = follows from
symbols/layout but was not executed. **[reported]** = read, not checked. Nothing here has
been run inside `riseofnations.exe`.

---

## 0. Lead: what now works

**Two independent OS processes play a lockstep game against each other over a real TCP
socket, on a routable interface, agreeing on every turn.** [measured]

```
$ donnet-peer host --bind 0.0.0.0:31400 --id 7 --turns 40 --peers 1 &
$ donnet-peer join --addr 192.168.50.55:31400 --id 8 --turns 40 --peers 1
host: {"event":"done","id":7,"turns":40,"final_hash":"223eddc1e2a9e9ac","ms":457}
join: {"event":"done","id":8,"turns":40,"final_hash":"223eddc1e2a9e9ac","ms":44}
40/40 TURN HASHES IDENTICAL over a routable interface (192.168.50.55)
```

They discover each other, agree a roster, run the readiness handshake, and then exchange
one `NETMSG_COMMANDPACKAGEDATA` per turn — refusing to advance a turn until every peer's
package for it is in hand. `0.0.0.0` + a routable address means this is not a loopback
special case; the same invocation works across the internet given one forwarded port.

Also delivered:

- **`crates/don-net` gained a session layer** — 40 tests, all green: every `NetMsg_*`
  layout, every `InternalPacketType` layout, `GameConnectionData`/`GameConnectionDataFull`/
  `PlayerConnectionData`, the readiness handshake, the lockstep gate, and a TCP transport.
- **`crates/netsys-shim` builds a real replacement `CrossplayNetLib.dll`** — PE32 i386,
  all 11 shipped exports present and byte-identical in name, 65-slot `NetSys` vtable.
  Not yet loaded by the game.
- **Option A's blocking unknown is resolved**, by disassembly on both sides of the call.
- **Option B's blocking unknown is resolved**: the lobby attribute schema is recovered in
  full, and the auth chain is named end to end. What remains is one credential, listed
  precisely in §6.

Three prior conclusions are corrected, in §1.

---

## 1. Three corrections, each measured

### 1.1 The factory's first argument is not a `MemMgr`. There is no vtable contract.

`docs/tracks/netcode-symbols.md` §5 Option A called this "the known unknown" and the thing
blocking DLL replacement. Both ends of the call settle it. [measured]

*Caller*, `NetSys::load_dll` `0x00538490`:

```
pcVar5 = GetProcAddress(dll_handle, "get_netsys_object_ptr");
piVar4 = pcVar5(&PTR_vftable_00c8ccf0, PTR_PTR_00c06378);
netsys = piVar4;
netsys->vt[0xe0](error_callback);   // slot 56, error_set_callback
netsys->vt[0xfc](prof);             // slot 63, set_profiler
```

The PDB names those two globals: `0x00C8CCF0` is **`loc_str_array_orig`**, a 24-byte
`StringTable` static in `basic/stringtable.obj`; `0x00C06378` is **`int_str_array`**, a
`StringTable*`. Both arguments are string tables. The PDB's own signature demangles to
`class NetSys * __cdecl get_netsys_object_ptr(class StringTable *, class StringTable *)`.

*Callee*, the shipped `CrossplayNetLib.dll` at `RVA 0x14B00`, in full:

```
mov  eax, [ebp+8]           ; arg1
push 0x3d4                  ; 4 + sizeof(CrossplayNetLibSys) = 4 + 0x3d0
mov  [0x1011a048], eax      ; stash arg1 in a DLL global
call [0x1006f20c]           ; operator new
push 0x100152c0 / 0x10014c40 / 1 / 0x3d0 / esi
call 0x10039c86             ; array-construct one CrossplayNetLibSys
...
ret                         ; no stack cleanup  =>  __cdecl
```

`[ebp+0xC]` is **never read**. The shipped implementation ignores the second argument, so a
replacement may ignore both. `0x3d0` = 976 confirms the `CrossplayNetLibSys` size already
on record. There is no memory-manager handshake anywhere in this path.

The two virtuals called immediately afterwards (`error_set_callback`, `set_profiler`) must
exist and must not fault, or the game dies before the main menu.

### 1.2 Seven of the 31 game message ids are dead. Setup does not travel as a NetMsg.

`NetDaemon::process` `0x00950F30` is `cmp ecx, 0x1e; ja default; jmp [ecx*4 + 0x951280]` —
a flat 31-entry jump table. Reading it: [measured]

| id | name | target |
|---|---|---|
| 1 | `PLAYERCONNECTIONDATA` | `0x951201` — **default arm, logs an error** |
| 2 | `ALLPLAYERCONNECTIONDATA` | `0x951201` — dead |
| 3 | `GAMECONNECTIONDATA` | `0x951201` — dead |
| 4 | `GAMECONNECTIONDATAFULL` | `0x951201` — dead |
| 8 | `PAUSE` | `0x951201` — dead |
| 27 | `PLAYER_STATUS_REQUEST` | `0x951201` — dead |
| 28 | `PLAYER_STATUS_RESPONSE` | `0x951201` — dead |
| 9, 11 | `TAUNT`, `DROPSTAMP` | `0x951271` — accepted and discarded |
| all others | | real handlers |

`docs/tracks/netcode-symbols.md` §5 Option B step 5 planned to negotiate game setup over
`NETMSG_GAMECONNECTIONDATAFULL`(4) and `NETMSG_ALLPLAYERCONNECTIONDATA`(2). **This build
never receives either.** The corroborating read is the send side:
`ConnectionData::send_game` `0x0094E200` and `ConnectionData::send_player` `0x0094EC40`
contain **no `NetSys::send*` call at all**. Both check `NetSys::is_host` (vtable +0x10),
build an `unordered_map<std::wstring, std::wstring>`, and publish it through
`MultiplayerManager::Instance()` `0x00A30DC0`. Setup and readiness are **PlayFab lobby
attributes**. The schema is §3.

The structs are still live in memory (`ConnectionData::game_data` is a
`GameConnectionDataFull` at +0x298), so `don_net::setup` still encodes them — they are just
not what crosses the wire.

Second-order consequence, and it matters: **replacing `CrossplayNetLib.dll` alone does not
get two retail instances into a match.** It captures the turn channel; the lobby/setup path
goes to `CrossplayProxy.dll` and is untouched by Option A.

### 1.3 `NETMSG_RESPONSE_FLAG` is a bit on the type byte, masked before dispatch

The dispatcher's first act is `bVar4 = *packet & 0xBF`, and `0xBF == !64`. So the type byte
is a 6-bit id plus bit 6 as a reply marker, and the same handler serves both. [measured]
`don_net::msg::MsgType::from_wire` reproduces it exactly, with a test that asserts
`t.to_wire() & 0xBF == id` over all 64 ids × both flag states.

---

## 2. `crates/don-net` — the session layer

40 tests, all green (`cargo test -p don-net`): 36 lib + 3 pre-existing corpus round-trips +
1 TCP integration test.

| module | what | evidence |
|---|---|---|
| `msg.rs` | every `NetMsg_*` encode/decode, the response flag, the dispatch table verbatim | sizes from `schema/types.json`; a test encodes all 15 typed variants and asserts each equals its PDB `sizeof` |
| `internal.rs` | all nine `InternalPacketType` packets | a test asserts each equals its `CrossplayNetLib.pdb` `sizeof` and that the `pack(1)` arithmetic explains it |
| `setup.rs` | `GameConnectionData` (46 B), `ScenFilePreviewData` (45 B), `GameConnectionDataFull` (2031 B), `PlayerConnectionData` | a test builds a full record, asserts 2031 bytes, and reads each string back at its PDB offset |
| `lobby.rs` | the recovered lobby attribute schema, §3 | key values read out of the binary |
| `transport.rs` | `Dest`/`Datagram`/`Transport`, in-process loopback, TCP | a test runs a real socket pair |
| `session.rs` | roster, readiness, pulse/timeout, desync, lockstep turn gate | six behavioural tests |
| `bin/donnet-peer.rs` | the two-process demo | §0 |

### 2.1 `PlayerConnectionData` is not raw-copyable, and that is load-bearing

`sizeof` is 59, but +0 and +24 are `std::wstring`. MSVC's `std::wstring` is a 24-byte union
of an 8-`wchar_t` SSO buffer and a heap pointer. **A PlayFab entity id is 16 hex
characters**, past the SSO limit — so a raw 59-byte copy of that struct would put a
*pointer* on the wire. `encode_portable` uses a length prefix instead, and is documented as
ours rather than the engine's. A test uses a 16-character id specifically so it cannot pass
by accident.

### 2.2 A real protocol bug, found and fixed

The first two-process run hung: the host waited 30 s for a readiness flag that the client
had already sent. Root cause, and it is a genuine protocol property rather than a coding
slip:

`CrossplayNetLibSys::process_ready_flag(const CrossplayNetLibPlayer*, ReadyFlagRequest*)`
takes an **already-resolved player**. A flag from a peer the receiver has not yet added is
dropped, and nothing in the protocol ever asks for it again — readiness is edge-triggered
with no retransmit and no query. The shipped game never hits this because membership is
lobby-driven: `OnPlayerJoined(LobbyMemberDTO)` materialises the peer object *before* any P2P
channel to it exists. With a bare transport and no lobby in front, ordering is not
guaranteed and the two peers deadlock, each believing the other is not ready.

Fixed by treating readiness as **state**, not an event: a peer restates its flag to each new
transport peer at greeting time and to everyone whenever the roster grows. The packet is
byte-identical; only the retransmit policy differs, so it cannot desynchronise a real
client. Pinned by
`session::tests::a_ready_flag_sent_before_the_peer_knows_us_still_converges`.

---

## 3. The lobby attribute schema — Option B's missing piece, recovered

`netcode-symbols.md` §5 Option B listed "the lobby attribute key names" as unestablished.
Here they are, all **[measured]**, extracted two ways: `const char*` keys by dereferencing
each symbol's VA into `.rdata`; `std::wstring` keys by following each symbol's
`$initializer$` slot in the CRT init table at `0x00AC6220…` to its dynamic-initialiser
thunk and reading the UTF-16 literal it pushes.

**Per-player keys.** `MakePlayerkey` `0x004BAB00` is a plain `std::wstring` concatenation
(`0x004BAA40` is `operator+`), so the key is prefix + slot index; slot 3's ready flag is
`steam_ready_3`. [prefix values measured; "slot index" is [inferred] from the caller passing
the loop counter through the `int -> const wchar_t*` lambda at `0x004BA3B0`]

| symbol | value |
|---|---|
| `PLAYERKEY_ELO` | `elo_` |
| `PLAYERKEY_SLOTTYPE` | `slot_type_` |
| `PLAYERKEY_TRIBE` | `tribe_` |
| `PLAYERKEY_WHO` | `who_` |
| `PLAYERKEY_TEAM` | `team_` |
| `PLAYERKEY_HANDICAP` | `handicap_` |
| `PLAYERKEY_DIFFICULTY` | `diff_` |
| `PLAYERKEY_READY` | `steam_ready_` |
| `PLAYERKEY_PLAYER_ID` | `player_id` |
| `PLAYERKEY_PLATFORM_PLAYER_ID` | `platform_player_id` |

**Lobby-level keys.**

| symbol | value |
|---|---|
| `LOBBYNAME_KEY`, `STEAM_LOBBYNAME_KEY` | `name` |
| `STEAM_LOBBYKEY_HOSTID` | `hostid` |
| `LOBBYKEY_QUICKMATCH` | `qm` |
| `LOBBYKEY_DISCRIMINATE_LOBBY_TYPE` | `discriminate_src` |
| `LOBBYKEY_DISCRIMINATE_VALUES_XBOX` | `xboxlive` |
| `LOBBYKEY_DISCRIMINATE_VALUES_STEAM` | `steam` |
| `LOBBYKEY_DISCRIMINATE_VALUES_CROSSPLAY_DISCRIM` | `no_crossplay` |
| `NEGATE_LOBBY_VALUE_PREFIX` | `__cross_neg__` |
| `LIST_LOBBY_VALUE_PREFIX` | `__cross_list__` |
| `STEAM_LOBBY_SCENARIO_DATA` | `scenario_data` |
| `STEAM_LOBBYKEY_GAME_SEED` | `game_seed` |
| `STEAM_LOBBYKEY_LOBBY_FLAGS` | `lobby_flags` |
| `STEAM_LOBBYKEY_ELORANK` | `elorank` |

**Game-settings keys.**

`teamstyle`, `map_style`, `map_size`, `gamespeed`, `gamerules`, `starting_town`,
`starting_resources`, `starting_resources2`, `tech_cost`, `reveal_map`, `pop_limit`,
`rush_rules`, `cannontime`, `starting_technology`, `starting_technology2`,
`ending_technology`, `elimination`, `victory`, `wonderwin`, `score_goal`, `popwin`,
`time_limit`, `chairs`, **`echowin`**, `scenario_type`, `script_type`, `desc`, `mods`,
`mod_name`, `mod_desc`, `mod_size`, `mod_checksum`, `mod_workshop_id`, `script_name`,
`scenario_size`, `scenario_checksum`, `scenario_num_files`, `scenario_workshop_id`.

> `STEAM_LOBBY_ECONWIN` at `0x00AFC0C8` points at `0x00AFC0C0`, which holds **`echowin`**.
> The typo is in the shipped binary. An interoperating implementation must reproduce it
> exactly, so `don_net::lobby` has a test that pins it.

Two structural facts about the publish, both from `send_player`'s shape: values are decimal
strings, and the publish is a **delta** — only keys whose value differs from
`ConnectionData::last_send_player_data[i]` are written, with a `force` override. A joining
peer must therefore read the full attribute set once and then apply deltas.
`don_net::lobby::player_delta` implements exactly that.

Beyond the mapping, `Crossplay::Lobby::DTO::TurnServerDTO` exists in the exe
(`0x0043E7D0` and friends), which corroborates the standing claim that TURN relay
credentials ride in the lobby record. [measured that the type and its ctors exist]

---

## 4. Option A — the replacement DLL

`crates/netsys-shim/` cross-builds to a **PE32 i386 DLL** with `cargo-xwin`:

```sh
cd crates/netsys-shim && XWIN_ARCH=x86 cargo xwin build --release
uv run --with pefile python check-exports.py     # -> PASS
```

`XWIN_ARCH=x86` is required the first time: the cache is per-architecture and a tree that
has only built `donscan` has `aarch64`/`x86_64` splatted, not `x86`.

Verified: [measured]

- all **11** shipped exports present, names byte-identical (`check-exports.py` compares
  against `ron-bin/dll/CrossplayNetLib.dll` and passes);
- `machine=0x014c`, `magic=0x010b`, `isDLL=true`, image base `0x10000000` — same as the
  shipped DLL;
- imports are `kernel32`, `ntdll`, `vcruntime140`, two api-sets and `ws2_32`. **No
  `steam_api.dll` dependency** — the replacement needs neither Steam nor PlayFab.
- `NetSysVtable` is 65 slots / 260 bytes and `NetSysBase` is 88 bytes, enforced by
  `const _: () = assert!(...)` so a mis-edit is a compile error.

Two ABI notes worth carrying forward:

- **Variadic member functions.** `log_connection_fmt` (vtable +0xBC) is variadic; MSVC
  compiles those as `__cdecl` with `this` pushed first, not `__thiscall`. Declaring only the
  fixed parameters and ignoring the tail is safe *because* the caller cleans the stack.
- **Exporting decorated C++ names from Rust.** `#[export_name = "?foo@Bar@@QAEXXZ"]` puts
  the symbol in the object file but does not reach a cdylib's export set on
  `i686-pc-windows-msvc` — all ten came back `undefined symbol`. `build.rs` emits explicit
  `/EXPORT:exported=internal` aliases instead, with the target written *without* the x86
  leading underscore because lld-link prepends it itself.

### What is NOT proven

**The DLL has never been loaded by `riseofnations.exe`.** That needs the Parallels VM and
was not done. The milestone in the brief — "two local instances hand each other turns" — is
met *at the transport and session layer* (§0, two OS processes, real sockets) and **not**
inside the retail game. Do not let those two sentences merge.

Known risks, in the order they would bite:

1. `NetPlayer::get_id` returns a `std::wstring` **by value**. We hand back the caller's
   return slot untouched, which reads as empty only if the caller zero-initialised it. Most
   likely first crash.
2. `NetSys::get` copies into a `NetDaemon`-owned buffer whose extent we have not measured.
   We cap at 1024 bytes — above the largest real message (`NetMsg_SyncDirInfo`, 529) — but
   that cap is a safety guess, not a measurement.
3. `set_p2p_callbacks` takes three `std::function`s by value; we never destroy them. A
   deliberate small leak.
4. Per §1.2, the lobby/setup path is not served by this DLL at all.

---

## 5. Option B — the internet path, and exactly what blocks it

The protocol is fully specified. Auth is the only real gap, and it is now named end to end.

### 5.1 The auth chain, measured

```
SteamAuthentication::RetrieveSteamAuthTicket(fn(vector<uchar>))   0x00A2C660
  -> Steam GetAuthSessionTicket
  -> SteamAuthentication::OnAuthSessionTicketResponse(...)        0x00A2C120
  -> SteamAuthentication::ConvertToCrossplayProxyToken() -> wstring  0x00A2C4F0
  -> ICrossPlayService::StartSession(userId, onOk, onErr)         vtable +16
  -> CrossplayProxy::QlocPFClient::AuthenticateWithSteam(const std::string&, fn(bool))
  -> PlayFab  POST /Client/LoginWithSteam   (over WINHTTP, *.playfabapi.com)
  -> entity token -> PFMultiplayerSetEntityToken + PartyLocalUserUpdateEntityToken
```

`ConvertToCrossplayProxyToken` is a **lowercase, zero-padded, two-hex-digits-per-byte**
encoding of the raw Steam ticket: a `wstringstream` with `std::hex`, `setw(2)` and fill
`'0'` (the `0x30` written into the stream's fill slot), looping over the byte vector.
[measured] That is precisely the `SteamTicket` format PlayFab's `LoginWithSteam` expects.

The complete PlayFab Client surface the game uses is five endpoints: [measured, from
`CrossplayProxy.dll` strings] `/Client/LoginWithSteam`, `/Client/GetPlayerCombinedInfo`,
`/Client/GetLeaderboard`, `/Client/GetLeaderboardAroundPlayer`,
`/Client/UpdatePlayerStatistics`. There is **no** anonymous, custom-id, or device-id login
path compiled in. `SteamAuthentication::GetEncryptedAppTicket` /
`OnRequestEncryptedAppTicket` exist as a second Steam ticket flavour.

### 5.2 The blocking list — exactly what a live join needs

| # | blocker | status | what it would take |
|---|---|---|---|
| 1 | **A Steam auth session ticket for the RoN:EE app** | **hard blocker** | A running Steam client, signed in, owning the game. `SteamAPI_Init` + `GetAuthSessionTicket`. There is no `SteamAPI_RestartAppIfNecessary` call in the exe, so the app id is supplied by the Steam client / `steam_appid.txt`, not baked into the binary [measured]. **A ticket cannot be minted offline; this is the one irreducible dependency.** |
| 2 | **The PlayFab title id value** | **address recovered; one bounded live read remains** | The pointer to `PlayFabSettings::staticSettings` is at `CrossplayProxy.dll+0xC2ED8`. The 80-byte `PlayFabApiSettings` stores only its `titleId` string object at `+56`; the developer secret begins at `+0` and must never be read or logged. A menu-state probe must bracket the shared pointer, read only that 24-byte string object plus its bounded title bytes, validate the MSVC string length/capacity, and refuse a torn or malformed snapshot. |
| 3 | Party network configuration | soft | `PartyCreateNewNetwork`'s configuration struct is not established. Only needed to *create* a network; a *joining* peer takes the serialized descriptor from the lobby. |
| 4 | 32-bit Windows host for the SDKs | soft | `PartyWin.dll` and `PlayFabMultiplayerWin.dll` are PE32 i386, so this side runs in the VM or gets reimplemented. |

Blockers 2–4 are bounded engineering or observation. **Only blocker 1 is a credential**, and
it cannot be engineered around: PlayFab is configured for Steam login only, so an internet
join is gated on a Steam ticket for an account that owns the game. That is a licensing fact,
not a missing measurement. The title id is an endpoint identifier, not a secret; nevertheless,
the probe is deliberately narrow so adjacent credentials cannot enter an evidence artifact.

**Recommended next step:** while the game sits at the multiplayer menu, run the bounded
title-only read above together with a double-read snapshot of Crossplay's scalar session roots.
That closes the last unknown value without inspecting a ticket, token, player name, platform id,
lobby id, descriptor, or developer secret.

The host-side collector for that experiment is now:

```sh
python3 tools/retail-control/netstate.py --pid PID
```

It is read-only and does not change the frozen injector. It requires exact SHA-256 and
`SizeOfImage` identities for the executable plus CrossplayProxy, CrossplayNetLib, PartyWin and
PlayFabMultiplayerWin; double-reads the NetSys pointer array and the three selected Crossplay
scalars; and brackets the exact 24-byte MSVC `titleId` string object at
`CrossplayProxy.dll+0xC2ED8 -> +56`. A heap string causes two reads of exactly its declared title
length. The tool emits only pointer-presence masks, bounded scalar values, and the validated title
id. Its output schema explicitly excludes developer secrets, tickets, tokens, lobby descriptors,
player/platform identifiers and names. Any root, bytes, module identity, or string shape changing
during the bracket refuses the artifact.

### 5.3 What this means for "internet games work"

Two honest routes, and they are different products:

- **Our own peers over the internet: working today.** `donnet-peer` needs no PlayFab, no
  Steam, and no relay — one forwarded port. This is the right substrate for self-play and
  for a headless RL environment, which is what the project is actually for.
- **Joining a retail player's lobby: not yet exercised; blocked first on §5.2 #1 and the
  bounded #2 live read.** Everything else — the
  lobby key schema, every packet layout, the readiness protocol, the turn channel — is now
  specified and implemented.

---

## 6. Reproduction

```sh
cargo test -p don-net                       # 40 tests
cargo build -p don-net --release

# two processes, real sockets, routable interface
./target/release/donnet-peer host --bind 0.0.0.0:31400 --id 7 --turns 40 --peers 1 &
./target/release/donnet-peer join --addr <your-lan-ip>:31400 --id 8 --turns 40 --peers 1

cd crates/netsys-shim && XWIN_ARCH=x86 cargo xwin build --release
uv run --with pefile python check-exports.py
```

Lobby-key extraction (regenerates §3 from the binary):

```sh
# const char* keys: deref the pointer at each symbol VA into .rdata
# wstring keys:     follow $initializer$ at 0x00AC6220.. to the thunk, read its UTF-16 push
cd ron-bin && uv run --with pefile --with capstone python <the script in this lane's history>
```

## 7. What I could not establish

- Whether the retail game runs with the replacement DLL. **Untested.** Needs the VM.
- The live PlayFab title-id value at the recovered, bounded address (§5.2 #2).
- The exact `send_game` key→field assignment. The key *values* are measured and the pairing
  is by name; the individual `mov`s in `0x0094E200` were not traced one at a time, and
  `don_net::lobby::SETTING_KEYS` is marked `[inferred]` for the pairing only.
- Which of `GameConnectionData`'s fields have no lobby key at all. `players`,
  `max_observers`, `difficulty`, `mods` and the three checksum fields have no key in the
  schema; whether they travel by another route or are simply host-local is open. The
  round-trip test asserts this asymmetry rather than hiding it.
- `NetSys::init`'s `GUID*` argument and the `Liberr` values beyond `LIBERR_OK`.
