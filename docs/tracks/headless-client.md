# Headless multiplayer client — design, transport, and what actually got built

**Lane:** `headless-client`. **Date:** 2026-08-08.
**Deliverables:** `crates/don-net/`, `re/scripts/pdb_read.py`, `re/scripts/pdb_symbols.py`,
`re/scripts/pdb_types.py`, `schema/rise-symbols.tsv`, `schema/command-wire.json`,
`schema/command-structs.txt`, `schema/net-structs.txt`, `ron-bin/sbl/*.pdb`,
`ron-data/replays/multi/` (62 files).

---

## 0. Lead with the thing that changes the project

**`riseofnations.exe` ships with its own full PDB, and it matches.** [measured]

```
C:\Program Files (x86)\Steam\steamapps\common\Rise of Nations\sbl\rise.pdb   57,290,752 bytes
sha256 334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5
```

The CodeView record in the retail binary names `E:\agent\_work\2\s\main\game\rise.pdb`
with GUID `51D4F219-61C6-4F84-9D5B-C3361B0D291F`, age 1. The shipped `rise.pdb`'s PDB
info stream carries **exactly that GUID and age**. Its section table also reproduces the
project's independently derived RVAs (`.rdata` `0x6c5000`, `.data` `0x806000`,
`docs/derivation/replay-io.md` §0). It contains **37,138 public symbols and 305,734 type
records covering 22,771 named tags**.

This is not my lane's target and it is far more important than my lane's target, so it goes
first. The project has spent its life reading `FUN_00644130` out of Ghidra while the
symbol file sat unopened in the install directory.

### The cross-check that could have failed, and didn't

Eighteen addresses this project derived independently — from switch structure, debug format
strings, RTTI and behaviour — looked up in the PDB. Every one is an **exact** symbol start:

| project's derivation | PDB symbol at that VA |
|---|---|
| `FUN_0094a700` = 82-opcode dispatcher | `?process@CommandPackage@@QAEHPAUCommand@@@Z` |
| `FUN_0094c500` = `process_all` | `?process_all@CommandPackage@@QAEXXZ` |
| `FUN_0094a0c0` = `process_group` | `?process_group@CommandPackage@@QAEHPAUGroupCommand@@@Z` |
| `FUN_009459d0` = `process_check_sums` | `?process_check_sums@CommandPackage@@QAEHPAUCheckSumsCommand@@@Z` |
| `FUN_00644130` = damage, `__thiscall`, pure integer | `?get_damage@ObjectData@@QBEHHHKHHPAH@Z` |
| `FUN_00a39cf0` = `next_float` | `?get@Random@@QAEMXZ` |
| `FUN_00a39d70` = `in_range` | `?get@Random@@QAEHHH@Z` |
| `FUN_00589600` = `Game::walk_data` | `?walk_data@Game@@QAEXPAUDataWalk@@@Z` |
| `FUN_00936560` = `check_all` | `?check_all@CheckSums@@QAEKXZ` |
| `FUN_00a46830` = adler-32 | `?adler32@@YAKKPBEK@Z` |
| `FUN_0043d730` = `SaveGame::walk` | `?walk_function@SaveGame@@UAEXPAX0@Z` |
| `FUN_0043d840` = `SaveGame::walk_tag` | `?walk_test@SaveGame@@UAEXABVString@@@Z` |
| `FUN_00a2dcd0` = `File::open` | `?open@File@@QAE?AW4Liberr@@ABVString@@H@Z` |
| `FUN_00952fb0` = replay record writer | `?write_package@RecordGame@@QAEXPBUCommandPackage@@@Z` |
| `FUN_00943730` = `process_player_speed` | `?process_player_speed@CommandPackage@@…` |
| `FUN_00943b00` = `process_camera` | `?process_camera@CommandPackage@@…` |

Sixteen shown; the remaining two are the interesting ones, because the PDB *corrects*
them. **Eighteen exact hits out of eighteen lookups, zero misses.** The methodology in
this repo works. The two corrections:
`FUN_00a1d110`, called `RString::AsScaled`, is **`?fraction@String@@QBEHH@Z`** =
`String::fraction(int) const`; and `[0x00846450]`, recorded as correctly-derived dead code,
is **`?get_num@Doober@@QAEHVTCoord@@0HH@Z`**.

`get_damage`'s mangled signature also pins the damage function's arity for free:
`int __thiscall ObjectData::get_damage(int, int, unsigned long, int, int, int*) const`
— six parameters, the last an out-pointer.

### Also shipped, also matching

| file | size | GUID | for |
|---|---|---|---|
| `CrossplayNetLib.pdb` | 16,683,008 | `51C234A4-72AC-40A1-9C6E-752F236961D7` | `CrossplayNetLib.dll` |
| `CrossplayProxy.pdb` | 16,838,656 | `D36F6BF3-9AA0-4388-895E-F3E8F776E191` | `CrossplayProxy.dll` |
| `PartyWin.pdb` | 2,609,152 | `6D2A00F5-421F-4510-8340-FD45122F14B5` | `PartyWin.dll` |
| `PlayFabMultiplayerWin.pdb` | 5,402,624 | — | `PlayFabMultiplayerWin.dll` |
| `dssl.pdb` | 15,306,752 | — | `dssl.dll` |

All copied to `ron-bin/sbl/` with guest-side SHA-256 verified against the host copy.

**Recommendation to the orchestrator, outside my lane:** re-run the Ghidra analysis with
PDB Universal loading `rise.pdb`, or apply `schema/rise-symbols.tsv` as a symbol map, and
re-audit every derivation that rests on a guessed function identity. The parsers are in
`re/scripts/pdb_read.py` (MSF/streams/publics) and `re/scripts/pdb_types.py` (TPI: struct,
union and enum layouts with member names and offsets). No dependencies.

---

## 1. Headline results for this lane

| # | result | tier |
|---|---|---|
| 1 | The `.rcx` command **payload** is the network protocol; the **framing is not**. File records are 18 bytes (`stamp, play, valid, group, size`); the wire message `NetMsg_CommandPackageData` is 8 (`type, stamp, play, data_size`). `valid` and `group` never leave the machine | C [measured] |
| 2 | Transport is **PlayFab Party + PlayFab Lobby**, brokered by `CrossplayNetLib.dll` / `CrossplayProxy.dll`, with a TURN relay carried in the lobby record. Not raw sockets | C [measured, from PDB symbols] |
| 3 | Complete `NetMsgType` (33 game messages) and `InternalPacketType` (9 transport messages) enums, with the engine's own names and values | structural [measured] |
| 4 | Complete `CommandTypes` enum (82) and the **exact `sizeof` and field layout of all 82 command structs**, from the PDB type stream. 79/79 fixed sizes agree with the independently derived table | C [measured] |
| 5 | **Multiplayer command streams are statically decodable.** The pad RNG is a stack-local `Random` reseeded per package from one game global, not the simulation RNG. This refutes `docs/derivation/replay-stream.md` §5 | C [measured] |
| 6 | `crates/don-net` round-trips **1,296,192 of 1,296,194 command packages** (5,055,253 commands, 59 opcodes) across 61 recordings, byte for byte | C [measured] |
| 7 | Cross-player checksum agreement across the whole corpus: **265,910 of 265,931 (0.999921)**. The 21 disagreements are **real simulation drift, not decode error** — they never touch `rules`, `walls`, `items`, `scenario_data` or `script_run_time` | C [measured] |
| 8 | `GameInfo+0x04` **is** `seed` — settled by name, closing an open question in `replay-format.md` | structural [measured] |
| 9 | `.rcx` is **not always gzipped**: 3 of 63 files are stored raw | C [measured] |
| 10 | **A headless client cannot be built from this evidence alone.** §6 says why, precisely | — |

Nothing here is verified in the proof-assistant sense. Item 4 is the strongest: it is a
type-stream layout cross-checked against behaviour on five million real commands.

---

## 2. The transport [measured, from PDB symbols; no live capture]

### 2.1 Layering

`riseofnations.exe` imports **nine** symbols from `CrossplayNetLib.dll` and **two** from
`CrossplayProxy.dll` (read from the PE import table):

```
CrossplayNetLib.dll:  CrossplayNetLibSys::{OnHostUpdated, OnPlayerJoined, OnPlayerLeft x2,
                      IsHost, set_p2p_callbacks, reset_ready_flags, send_ready_flag},
                      CrossplayNetLib::is_connected_to_network
CrossplayProxy.dll:   Crossplay::Logging::Logger(), Crossplay::Service()
```

The netcode itself lives in the DLL. `NetSys` is an abstract base in the game
(`?init_system@NetSys@@SAPAV1@ABVString@@@Z` at `0x00538490` returns `NetSys*`;
`?dll_handle@NetSys@@2PAUHINSTANCE__@@A` at `0x00E335B8` is an `HINSTANCE`, so the
implementation is loaded dynamically). Its layout, from `CrossplayNetLib.pdb`:

```c
struct NetSys {                       // sizeof 88
  void**      __vfptr;
  int         num_players;
  NetPlayer*  players[8];             // <= 8 players, hard
  NetPlayer*  local_player;
  NetPlayer*  host_player;
  eLocalPlayerDisconnect local_player_connection;
  float       local_player_disconnect_pct;
  Array<NetSession*> net_sessions;
  Log*        log;
};
```

`CrossplayNetLibSys : NetSys` (sizeof 976) is the concrete implementation. Its members name
the architecture outright:

```c
  Crossplay::ICrossPlayService*   m_crossplay;
  Crossplay::Lobby::DTO::LobbyDTO m_lobby;
  NetSysFifo fifo_sys;  NetFifo fifo_receive, fifo_host, fifo_pulse;
  std::function<void(P2P::ICrossplayPlayer*)> m_dataChannelOpenedCallback;
  std::function<void(P2P::ICrossplayPlayer*)> m_dataChannelClosedCallback;
  std::function<void(wstring,wstring)>        m_connectionFailedCallback;
  unsigned long host_port, local_port, time_out, game_version;
  bool lobby_launched;  int matchmaking_id, num_allowed_players;
```

### 2.2 What the transport actually is

`CrossplayProxy::CrossPlayService` (the shipped implementation behind
`Crossplay::ICrossPlayService`) exposes, among ~70 methods:

```
CreateLobby / JoinLobby / LeaveLobby / FindLobbies / GetLobby
HandlePFLobbyCreated / HandlePFLobbyJoined            <- PlayFab Lobby
HandlePartyNetworkCreated / HandlePartyNetworkJoined / PartyNetworkRejoin
P2PStartConnection / P2PSend / P2PSendToAll / P2PClose / P2PCloseAll
SetP2PDataChannelOpenedCallback / SetP2PDataChannelClosedCallback
SetReceivedP2PDataCallback / SetReceivedP2PTextCallback / SetP2PTimeoutDuration
```

`PFLobby` + `PartyNetwork` is **PlayFab**. `PartyWin.dll` is the **PlayFab Party** SDK —
its PDB is full of `PartyQosServersRequest/Response`, `PartyStateChangeManager`,
`PartyChatControl`, `PartyBuildAliasParams`. `PlayFabMultiplayerWin.dll` is the PlayFab
Multiplayer (lobby/matchmaking) SDK. The lobby record itself carries relay credentials:

```c
struct Crossplay::Lobby::DTO::TurnServerDTO { wstring _address, _userName, _password; };
struct Crossplay::Lobby::DTO::LobbyDTO {
  wstring _id, _ownerUserId, _sessionReference;
  int _maxMembers, _availableSlots, _botCount, _attributeVersion;
  Visibility _visibitily;                      // sic
  vector<LobbyMemberDTO> _members;
  unordered_map<wstring,wstring> _attributes;
  TurnServerDTO _turnServer;
};
```

So the picture is: **PlayFab Lobby for discovery and membership; PlayFab Party for the
P2P data channels; a TURN server for relay when direct fails.** The supporting DLLs in the
install directory corroborate it — `boringssl.dll`, `signalrclient.dll`, `protobuf_lite.dll`,
`cpprest140_2_9.dll`.

There is a **second, older transport still present**: `CNetworkTransport` in the game holds
`CCallback<CNetworkTransport, P2PSessionRequest_t>` and `P2PSessionConnectFail_t` — Steam
P2P — and `GameSpySession` / `NetMsg_GameSpyChallenge` are the legacy path.
`?join_ip@CrossplayNetLibSys@@UAE?AW4Liberr@@ABVString@@000HH@Z` also exists, taking four
strings and two ints, alongside `set_ip_override`, `get_host_port`, `set_local_port`,
`get_ip_addresses`. **I did not establish whether the direct-IP path still works**; it is
the most promising lead for a headless client and it is untested. See §6.

### 2.3 The message surface

`NetMsgType` — first byte of every `GenericNetPacket` (`struct GenericNetPacket { u8 type; }`).
Complete, from the PDB enum:

```
 0 GENERIC                     11 DROPSTAMP                22 SYNCFILEERROR
 1 PLAYERCONNECTIONDATA        12 TIMESYNC                 23 SYNCDIRERROR
 2 ALLPLAYERCONNECTIONDATA     13 DROPVOTE                 24 SYNCDIRREQUEST
 3 GAMECONNECTIONDATA          14 DROPDECISION             25 SYNCDIRINFO
 4 GAMECONNECTIONDATAFULL      15 GAMEMODSYNCREQUEST       26 GAMESPYCHALLENGE
 5 CHAT                        16 GAMEMODSYNCRESPONSE      27 PLAYER_STATUS_REQUEST
 6 PING                        17 SYNCFILEBEGIN            28 PLAYER_STATUS_RESPONSE
 7 COMMANDPACKAGEDATA          18 SYNCFILERESPONSE         29 DROP_FLAG
 8 PAUSE                       19 SYNCFILEDATA             30 SPLINE
 9 TAUNT                       20 SYNCFILEVERIFY           31 GAMECONNECTIONSCENARIO
10 SYNCSIGNAL                  21 SYNCFILEEND              32 GAMECONNECTIONMODINFO
                                                           64 RESPONSE_FLAG (bit)
```

`InternalPacketType` — handled by `CrossplayNetLibSys`, never reaching the game. All ≥ 128,
which is why the game ids stay below it:

```
128 IPT_PLAYERLIST   131 IPT_PULSEPACKET   134 IPT_MIGRATEHOST
129 IPT_DROPREQUEST  132 IPT_ADDPLAYER     135 IPT_DSYNCMSG
130 IPT_CANCELDROPREQUEST  133 IPT_DESTROYPLAYER  136 IPT_READYFLAG
```

with layouts (all `#pragma pack(1)`, all prefixed by the 1-byte type):

```c
AddPlayerRequest      { char player_name[64]; int unique_id; bool is_hosting; }  // 70
DestroyPlayerRequest  { int unique_id; }                                          // 5
DropRequest           { int unique_id; }                                          // 5
MigrateHostRequest    { int new_host; }                                           // 5
DsyncNotification     { int frame; }                                              // 5
ReadyFlagRequest      { bool ready; }                                             // 2
HostPlayerList        { u8 num_players; int unique_ids[8]; }                      // 34
```

### 2.4 The command-package message

```c
struct NetMsg_CommandPackageData {   // sizeof 9 with data[1]
  u8   type;        // = 7
  u32  stamp;       // +0x01  simulation frame
  char play;        // +0x05  player slot
  short data_size;  // +0x06
  u8   data[];      // +0x08
};
```
Receiver: `?process_command_package_data@CommandManager@@QAEXPAUNetMsg_CommandPackageData@@K@Z`
at `0x0093FCD0`. Sender: `?send_local_package@CommandManager@@QAEXXZ` at `0x00940120`.

**This is the single most important structural correction this lane makes.** The project's
established fact reads "the `.rcx` command stream IS the engine network protocol". That is
true of the *payload* and false of the *framing*: the file writes an 18-byte header with
two extra fields (`valid`, `group`) that the wire does not carry. `crates/don-net` now
models both and converts between them.

---

## 3. Join, readiness, and the turn clock

### 3.1 Join [measured signatures; sequence is inference from names]

```
NetSys* NetSys::init_system(String const&)                                      0x00538490
Liberr  CrossplayNetLibSys::init(NetMessenger*, GUID*, int, int, u32, u32)
Liberr  CrossplayNetLibSys::setup_player_info(String const&, String const&, int)
Liberr  CrossplayNetLibSys::poll_sessions(String const&)      // -> lobby search
Liberr  CrossplayNetLibSys::join(NetSession const*, String const&, String const&, String const&, int)
Liberr  CrossplayNetLibSys::host(String const&, String const&, String const&, String const&, String const&)
Liberr  CrossplayNetLibSys::join_ip(String const&, String const&, String const&, String const&, int, int)
void    CrossplayNetLibSys::set_p2p_callbacks(function<void(ICrossplayPlayer*)>,
                                              function<void(ICrossplayPlayer*)>,
                                              function<void(wstring,wstring)>)
Liberr  CrossplayNetLibSys::poll_players(NetSession const*, Array<NetPlayer*>)
void    CrossplayNetLibSys::process_system_messages()
bool    CrossplayNetLibSys::get(GenericNetPacket*, NetPlayer const**, u32*)
bool    CrossplayNetLibSys::send(GenericNetPacket*, int, NetPlayer const*, int)
bool    CrossplayNetLibSys::send_all(GenericNetPacket*, int, int)
```

`Liberr` is the engine-wide result enum (`LIBERR_OK = 0`, …). `get`/`send`/`send_all` are
the **entire game-facing transport API** — everything above §2.4 goes through these three
virtuals. A headless client that could implement `NetSys` would need nothing else from the
game side.

Once joined, `CrossplayNetLibSys` runs a membership protocol over the internal packets:
`send_playerlist` / `process_playerlist` (host authoritative), `process_create_player_message`
(`AddPlayerRequest`), `process_destroy_player_message`, `send_pulse` / `process_pulse`
(keepalive; `CrossplayNetLibPlayer::last_pulse`, `is_timed_out(player, u32)`),
`process_host_migrate_message`, and drop voting.

### 3.2 Readiness [measured]

Two independent mechanisms, at two layers:

- **Transport**: `CrossplayNetLibSys::send_ready_flag(bool)` → `IPT_READYFLAG` /
  `ReadyFlagRequest { bool ready }` → `process_ready_flag(player, req)`;
  `reset_ready_flags()` clears them. `CrossplayNetLibPlayer::ready` is a `bool` at +0x28.
  These three are among the nine symbols the *game* imports from the DLL, so this is the
  handshake the game actually drives.
- **Lobby/game setup**: `PlayerConnectionData::ready` (u8 at +0x3A of a 59-byte record),
  replicated by `ConnectionData::send_player` / `send_game` as
  `NETMSG_PLAYERCONNECTIONDATA` / `NETMSG_GAMECONNECTIONDATAFULL`.
  `ConnectionData::unready_all_players()` resets them.

```c
struct PlayerConnectionData {          // 59 bytes
  wstring player_id, platform_player_id;
  int  elo;
  u8   slot_type, tribe, who, team, handicap, diff, ready;
};
```

### 3.3 The turn clock [measured]

```c
struct TurnControl {                   // 376 bytes
  BitMask<8> semaphore;                // +0x04  per-player "package received"
  u32 frame_base_time, frame_counter, frame_average, frame_timing_clock;
  int cannon_time_who, cannon_time_stamp, cannon_saved_speed, speed, playback_speed;
  u32 target_frame_time;
  int turn_length;                     // +0x3C  frames per command turn
  int turn_counter;
  ...
  u32 last_package_sizes[8], last_wait_times[8], last_lag_times[8],
      last_average_frame_times[8], last_ping_times[8], last_diff_times[8],
      last_forced_loads[8], max_wait_times[10];
  int max_wait_index; u32 average_max_wait_time, extra_time;
  u8  turn_flags; u32 last_lag_adjust, min_turn_time; int turn_index;
};
```

Three separate frame drivers exist and are named:
`do_frame_solo` / `do_frame_multi`, `check_new_frame_solo` / `check_new_frame_multi` /
`check_new_frame_playback`, `compute_next_turn_solo` / `compute_next_turn_multi`.
`turn_length` is a **variable**, which is why the corpus shows 8 frames per turn in the
2024 solo specimen and 6 in the 2017 multiplayer one. The measured tick is 15 fps
(`docs/derivation/replay-stream.md` §2, confirmed against a known 11:40 game time).

Per turn the local client issues, via `CommandManager`: `issue_turn_data` →
`TurnDataCommand { u16 ping_time, frame_average, wait_time, game_lag, forced_loads }`
(opcode 0x4A), `issue_camera` (0x48), `issue_player_speed` (0x4F, 8 accumulators), and in
multiplayer `issue_check_sums` (0x39). `send_local_package` then ships the whole
`CommandPackage` as `NETMSG_COMMANDPACKAGEDATA`.

**Note the field-order correction:** the log format string reads
`frame_average, ping_time, wait_time, game_lag, forced_loads`, but the struct is
`ping_time, frame_average, wait_time, game_lag, forced_loads` — the first two are swapped
relative to the printf. The struct is authoritative.

---

## 4. The lockstep contract

### 4.1 What a client must compute [measured]

Every turn, each multiplayer client serialises a `CheckSumsCommand`:

```c
struct CheckSumsCommand {              // 65 bytes, opcode 0x39
  u8  command_type;
  u32 units_checksum, builds_checksum, walls_checksum, ammo_checksum,
      deaths_checksum, groups_checksum, guys_checksum, leaders_checksum,
      cities_checksum, items_checksum, goods_checksum, world_checksum,
      rules_checksum, scenario_data_checksum, script_run_time_checksum,
      all_checksum;
};
```

The 16 field names are the engine's, and they are exactly the `check_all` channels. That
list **is the engine's own definition of sim-critical state**, decomposed by subsystem, and
therefore the exact fidelity contract a headless client must meet: reproduce all sixteen,
bit for bit, every turn. `CheckSums::check_all` is `?check_all@CheckSums@@QAEKXZ` at
`0x00936560`; the channel algorithms are still not recovered (`docs/derivation/checksum.md`).

Policy knobs live in `GameInfo` and are negotiated at game creation:

```c
struct GameInfo {                       // 1348 bytes
  u32 version;                          // +0x00
  u32 seed;                             // +0x04   <-- the seed, settled by name
  int checksum_deep;                    // +0x08
  int checksum_window_size;             // +0x0C
  int checksum_failure_threshold;       // +0x10
  u32 flags;                            // +0x14
  u8  team_style, map_style, map_size, players, max_observers, game_speed,
      game_rules, difficulty, starting_town, starting_resources, starting_resources2,
      tech_cost, reveal_map, pop_limit, rush_rules, cannon_times,
      starting_technology, starting_technology2, ending_technology, elimination,
      victory, wonderwin, score_goal, popwin, time_limit, chairs, econwin,
      scenario_type, script_type, mods;
  ...
};
```

`GameInfo+0x04 = seed` closes the "leading seed candidate" question in
`docs/replay-format.md`. It also matches `crates/don-rules/src/offsets.rs::fun_005d6040`,
whose six bindings (`version, seed, checksum_deep, checksum_window_size,
checksum_failure_threshold, flags`) are exactly this struct's first six members — an
independent confirmation from the rule-binding extractor, which never saw the PDB.
`0x005D6040` itself is `?log_data@GameInfo@@QBEXPAVLog@@@Z`.

### 4.2 What happens on mismatch [measured]

`?recover_from_oos@CommandManager@@QAEXXZ` at `0x0093E9E0`. It reads the eight per-player
`all_checksum` values stashed by `process_check_sums` at `DAT_00CBEE90[0..7]`
(`(&DAT_00cbee90)[*(int *)(in_ECX + 4)] = *(u32 *)(param_1 + 0x3d);` — index by `play`,
value from offset `+0x3D`, i.e. `all_checksum`), then:

1. For each active player *i*, count how many of the eight values equal player *i*'s.
2. Take `argmax` of that count — a **plurality vote**.
3. Index `PTR_DAT_00c0622c + winner * 0x3B`. `0x3B` = 59 = `sizeof(PlayerConnectionData)`,
   so the winner is resolved to a `ConnectionData::player_data_server[]` entry.

**The majority state wins and the minority is the desynced party.** It also clears a flag
in `PTR_DAT_00c061ec[0x823]` and sets `*(int*)(base+0x81c) = 2` when it was 0 — a mode
change I did not identify.

Separately, `CrossplayNetLibSys::send_dsync(int)` / `process_dsync` carry
`DsyncNotification { int frame }` (`IPT_DSYNCMSG`), and `CrossplayNetLibPlayer` holds
`sync_counter` and `dsync_frame`. `CommandPackage` carries statics `checksum_recheck`,
`checksum_fail_count` and `checksums`; `checksum_failure_threshold` gates how many
consecutive failures are tolerated. Telemetry strings confirm this is instrumented in the
shipped build: `DesyncGame`, `DesyncCategoryMask`, `CheckDesyncsEveryXFrames`,
`DesyncTrackingEnabled`, `DesyncTrackingFrameHistorySize`, `%d/%d prior games have desynched`.

**I did not trace the full state machine from "checksums differ" to a user-visible outcome,**
and I do not assert what the losing client does — resync, drop, or continue.

### 4.3 Corpus evidence for determinism [measured]

Across the 62-file multiplayer corpus, comparing the two 16-tuples that different players
serialised for the same turn from different machines:

```
265,931 comparisons in 21 files   ->   265,910 identical   (0.999921)
```

21 disagreements, all in 4 files, each first appearing **mid-game** with the recording
continuing for tens of thousands of frames afterwards:

| file | disagreeing turns | first at stamp | last stamp | channels that differed |
|---|---|---|---|---|
| `Playback - 2018.11.17 13'21'42` | 7 | 259 | 108,157 | units 5, builds 5, groups 4, guys 3, leaders 5, world 3, **all 7** |
| `Playback - 2018.12.01 18'33'16` | 4 | 727 | 57,007 | units 4, builds 4, ammo 1, deaths 1, groups 1, guys 2, leaders 4, cities 1, world 3, **all 4** |
| `Playback - 2024.03.20 17'28'53` | 4 | 11,357 | 61,655 | units 2, builds 3, groups 4, guys 1, leaders 3, world 1, **all 4** |
| `Playback - 2024.04.10 17'05'19` | 6 | 17,701 | 78,193 | units 4, builds 5, ammo 2, deaths 2, groups 3, guys 2, leaders 6, cities 1, goods 1, world 3, **all 6** |

**These are real simulation divergences, not decode artefacts.** [measured] The channel
breakdown is the discriminator, and it is one-sided in a way a decode error could not be:

- `all` differs on **every** disagreeing turn (7/7, 4/4, 4/4, 6/6), exactly as an aggregate
  channel must.
- The channels that ever differ are precisely the **mutable simulation** ones — `units`,
  `builds`, `groups`, `guys`, `leaders`, `world`, and occasionally `ammo`, `deaths`,
  `cities`, `goods`.
- `rules`, `scenario_data`, `script_run_time`, `walls` and `items` **never** differ, across
  all 21 events. `rules` in particular is constant for a whole match, so a byte-level decode
  error — which would scramble the 65-byte packet arbitrarily — would corrupt it about as
  often as anything else. It never does.

That the games then ran on for tens of thousands of frames is consistent with §4.2: a
transient divergence is put to a plurality vote and the match continues. So the corpus shows
both that RoN lockstep is near-perfectly deterministic (0.999921 over a quarter-million
cross-machine comparisons) and that it does drift occasionally and has machinery to absorb
it. **What I still have not established is the mechanism of the drift** — whether it is
floating point, timing, or a genuine engine bug.

---

## 5. The codec: `crates/don-net`

Dependency-free (the corpus test shells out to `gzip`, so a green `cargo test` never touches
the registry). Modules:

| module | contents |
|---|---|
| `opcodes.rs` | generated: 82 opcode names, struct names, and `sizeof` from `rise.pdb` |
| `obfuscate.rs` | `PadRandom` (the engine LCG), XOR, key ranking |
| `stream.rs` | locating the command stream in a decompressed `.rcx` |
| `lib.rs` | `Command`, `PackageHeader`/`PackageRecord`/`PackageStream`, `NetCommandPackage`, `CheckSums`, `NetMsgType`, `InternalPacketType` |

### 5.1 The wire format, from the type stream

`schema/command-wire.json` and `schema/command-structs.txt` hold all 82 layouts with field
names and offsets. Everything is packed — `MoveToCommand::to_x` sits at `+0x01`. Examples:

```c
struct CommandPackage {         // 536 bytes in memory
  u32   stamp;  int play;  int valid;  int group;  short size;
  u8    data[512];
  Random padding;               // +0x214   <-- see §5.2
};
struct MoveToCommand { u8 op; int to_x, to_y, set_angle, angle;
                       char orders, queued, form, width, disembark; };   // 22
struct CameraCommand { u8 op; u8 zoom; int x_loc, y_loc; };              // 10
struct TurnDataCommand { u8 op; u16 ping_time, frame_average, wait_time,
                         game_lag, forced_loads; };                      // 11
```

**Opcode-table corrections against `docs/derivation/replay-stream.md` §3**, all from the
`CommandTypes` enum and the handlers' own parameter types:

| op | doc said | actually |
|---|---|---|
| 0x0F | `process_set_transport ox, queued` | `process_board_ship` / `BoardShipCommand` |
| 0x19 | `process_queue_up x, y, …` | `process_build` / `BuildCommand` |
| 0x2D | `process_deman_tribute who, whom, whose, onoff` | `process_propose_attack` / `ProposeAttackCommand` |
| 0x44 | `process_chat_set bits, taunt, …` | `process_chat` / `ChatCommand` |
| 0x50 | `process_ungraceful_player_drop` | struct is `UngracefulPlayerDrop` (no `Command` suffix) |

**79 of 79 fixed-size opcodes agree** between the PDB `sizeof` and the hand-derived table.
That is the cross-check that could have failed and is worth more than either source alone.

### 5.2 Multiplayer streams are statically decodable — refuting an earlier conclusion

`docs/derivation/replay-stream.md` §5 concluded that "a multiplayer command stream is **not
statically decodable** — a faithful reader has to advance the same Random object in lockstep
with the sim." **That is not what the code does.** [measured]

In `process_all`, the padding generator is a **stack local**:

```
0094c6a4  mov  eax, [0xc061ec]
0094c6a9  test byte [eax + 0x820], 4        ; multiplayer flag
0094c6b0  je   0x94c6c1
0094c6b2  mov  ecx, [eax + 0x10]            ; G, the same global the XOR key comes from
0094c6b7  mov  [ebp - 0x18], eax            ; seed a LOCAL Random with G
...
0094c6fe  lea  ecx, [ebp - 0x18]            ; <- this, not the sim RNG
0094c701  call 0xa39d70                     ; Random::get(0, 2)
```

So a single 32-bit global `G = *(u32*)(*(u8**)0x00C061EC + 0x10)` determines everything:
`xor_key = (G >> 8) & 0xFFFF`, and the pad generator is **reseeded to `G` at the start of
every package**. There is no dependence on simulation history.

Better still, the search space is 256, not 2^32. `Random::get`, read off `0x00A39EA5`:

```
s = s*0x19660D + 0x3C6EF35F;  return lo + (((u16)s * (hi - lo)) >> 16);
```

Only the **low 16 bits** of `s` are consumed, and the low 16 bits of an LCG state under
multiply-add mod 2^32 depend only on the low 16 bits of the previous state. Bits 8..15 of
`G` are pinned by the XOR key, so only `G & 0xFF` is free. This is asserted as a unit test
(`only_the_low_sixteen_bits_of_the_seed_can_change_a_pad`).

### 5.3 Measured results

```
DON_NET_FULL_CORPUS=1 cargo test -p don-net --release -- --nocapture      # 57 s
```

The **default** run sweeps a deterministic 12-file spread instead (2.6 s), because this
crate sits in a workspace several lanes run `cargo test` on and the full sweep was costing
each of them minutes. The subset spans four build eras and both game modes and still
catches every size-formula regression: `12 files, 186,113/186,113 packages, 12/12 at 100%,
40,788/40,788 checksum comparisons`. The numbers below are from the full sweep.

```
63 files; 61 carry a command stream (1 solo, 60 multiplayer), 2 carry none
1,296,192 / 1,296,194 packages round-tripped byte-exactly   (1.00000)
5,055,253 commands, 59 distinct opcodes
59/61 files at 100%
96,197 recorded packages converted to NetMsg_CommandPackageData and back
cross-player checksum tuples: 265,910 / 265,931 identical  (0.999921)
```

For each package the test asserts (a) the record framing tiles the payload to EOF with zero
residue, (b) the header survives encode/decode identically, (c) every command's bytes sit
exactly where the walk predicts, (d) the walk tiles the payload exactly, (e) re-encoding
reproduces the payload length, (f) the XOR is an exact involution back to the file bytes.

**The two remaining bad packages, named:** a 3-byte payload at stamp 19,879 in
`playback - 2014.04.26 15'32'51` and a 4-byte payload at stamp 57,043 in
`playback - 2014.08.08 20'54'48`, each too short to hold the command its opcode names.
Both are single anomalies in files that otherwise decode 7,986/7,987 and 19,042/19,043.
I believe they are truncated writes, but **I did not prove that**; they are reported by the
test rather than suppressed.

**The two files with no command stream, named:** `playback - 2014.08.08 22'02'25` is
801 bytes total (an aborted recording), and `playback - 2014.08.12 19'59'21` is a 4.7 MB
uncompressed file whose tail is a repeating `0c 00 19 00` state pattern, not a command
stream — consistent with a `recordgame.tmp` that was never gz-copied.

### 5.4 The test earning its keep

I initially took `ChatCommand`'s length as `17 + 2*len` by reasoning from
`sizeof(ChatCommand) == 19` with `wchar_t string[1]`. **17 of 63 files round-tripped.**
The failures pointed at chat packets; `process_chat`'s own return instruction settles it:

```
009458b4  lea esi, [eax*2 + 0x13]      ; 19 + 2*len   (the string is NUL-terminated)
```

With that one constant fixed, 59 of 61 files went to 100%. This is exactly why the corpus
test exists and why a hand-built fixture would have been worthless: a fixture built from my
wrong assumption would have passed. All three variable-length formulas are now taken from
the handlers' own `lea`s, not from `sizeof`:

| op | handler | instruction | formula | `sizeof` |
|----|---------|-------------|---------|----------|
| 0x00 | `process_group` @ `0x0094A6F3` | `lea eax,[eax*2+3]` | `3 + 2*num` | 5 |
| 0x33 | `process_spline` @ `0x00945396` | `lea esi,[eax*8+6]` | `6 + 8*len` | 14 |
| 0x44 | `process_chat` @ `0x009458B4` | `lea esi,[eax*2+0x13]` | `19 + 2*len` | 19 |

`re/scripts/rcx_parse.py` already had all three right (`0x44: (…, 0x13)`); the narrative
table in `replay-stream.md` §8 omits `0x44`, `0x33` and `0x00` from its `SIZE` dict, which
is what I built from. Worth folding the parser's values back into the doc.

### 5.5 Other format corrections

- **`.rcx` is not always gzip.** 3 of 63 files start at `16 42 1a 00` — the payload itself.
  `File::write` picks `fwrite` or `gzwrite` off a mode bit, so both are legal output of the
  same path. `docs/replay-format.md`'s "the whole file is a single gzip stream" holds for
  the specimen it was derived from, not for the format.
- **The 2003-era build decodes with the 2024 opcode table.** `playback - 2014.04.26 15'32'51`
  carries `Version: 03.02.03.29` — a different versioning scheme entirely from the
  `00.20xx.mm.dd` of every other file — and still decodes 7,986/7,987 packages. Command
  numbering and sizes have been stable across the entire life of the game.
- **Header field names are the engine's**: `stamp, play, valid, group, size`. The doc's
  `from`/`serial` are `play`/`group`; the "unnamed `u32` at `+0x08`" is `valid`. Its
  *meaning* is still unknown — naming is not semantics.

---

## 6. Feasibility: can we build the headless client?

Honestly: **not from this evidence, and not the way the brief imagined.** The gap between
decoding a recorded stream and speaking a live protocol is large and I want it stated
plainly rather than papered over.

**What is done and solid.** Everything from `GenericNetPacket` upward. We can construct a
byte-correct `NetMsg_CommandPackageData` carrying any of the 82 commands, parse any
incoming one, and read the per-turn checksum tuple. The membership and readiness messages
are fully specified. That is the *entire* game-side protocol.

**What is missing is everything below it.** `CrossplayNetLibSys::{get, send, send_all}` are
the only doors, and behind them sits PlayFab Party — a proprietary, authenticated,
account-bound relay SDK (`PartyWin.dll`) plus PlayFab Lobby. Reimplementing that is not
reverse engineering a wire format; it is reimplementing a commercial SDK against a live
service, and it requires credentials.

Three viable routes, in descending order of how much I trust them:

1. **Host the transport inside the real client.** Inject a DLL, hook
   `CommandManager::send_local_package` and `process_command_package_data`, and drive the
   game from outside. `WriteProcessMemory` control is already the project's stated
   highest-leverage unbuilt tool and this rides on it. It gets a programmable client
   without speaking PlayFab at all. **Recommended.**
2. **`join_ip` + `set_ip_override`.** The direct-IP path still exists in the shipped
   `CrossplayNetLibSys`, with `set_local_port`/`get_host_port` alongside it. If it still
   works on LAN, a headless client only needs the transport framing beneath
   `GenericNetPacket`, which is small and reachable. **I did not test this and cannot say
   whether the path is live.** It is the highest-value next experiment: run two instances
   in the VM, capture loopback traffic, and see what the transport puts around a
   `NETMSG_COMMANDPACKAGEDATA`.
3. **Link `CrossplayNetLib.dll` directly** from a headless x86 host process, implementing
   `NetMessenger` and the `NetPlayer`/`NetSession` interfaces. We now have the full type
   layouts for all of them. This is the "correct" route and by far the most work, and it
   still needs a PlayFab identity.

**The fidelity bar, independent of transport.** §4.1 is unforgiving: to stay in a lockstep
game a client must reproduce all sixteen `check_all` channels bit-exactly every turn. Those
algorithms are not yet recovered. Until they are, a headless client can *speak* but cannot
*play* — it would desync on the first turn its state diverged, and §4.2 says the majority
votes it out. A headless **observer** that only sends `turn_data` and camera commands and
never issues orders is a much nearer-term target and would still be a real capability.

---

## 7. Reproduction

```sh
# copy the PDBs out of the VM (host listener + guest PUT; SHA-256 verified both sides)
#   see the upload receiver pattern in this lane's scratchpad; VM is 10.211.55.6,
#   host 10.211.55.2. prlctl exec runs as SYSTEM, so shared folders are invisible
#   and user paths must be spelled C:\Users\ember\...

# confirm rise.pdb belongs to the retail binary
python3 /Users/ember/dev/don/re/scripts/pdb_read.py /Users/ember/dev/don/ron-bin/sbl/rise.pdb
#   -> guid 51D4F219-61C6-4F84-9D5B-C3361B0D291F age 1, matching the exe's CodeView record

# symbol table for the whole game
python3 /Users/ember/dev/don/re/scripts/pdb_symbols.py \
    /Users/ember/dev/don/ron-bin/sbl/rise.pdb --base 0x400000 \
    --dump /Users/ember/dev/don/schema/rise-symbols.tsv
python3 /Users/ember/dev/don/re/scripts/pdb_symbols.py \
    /Users/ember/dev/don/ron-bin/sbl/rise.pdb --base 0x400000 --va 0094a700 00644130

# struct / enum layouts
python3 /Users/ember/dev/don/re/scripts/pdb_types.py \
    /Users/ember/dev/don/ron-bin/sbl/rise.pdb --struct CommandPackage GameInfo TurnControl
python3 /Users/ember/dev/don/re/scripts/pdb_types.py \
    /Users/ember/dev/don/ron-bin/sbl/rise.pdb --all-matching 'Command$'

# the codec, against the real corpus
cd /Users/ember/dev/don && cargo test -p don-net --release -- --nocapture
#   -> 12-file deterministic subset, 2.6 s (kept cheap: several lanes run `cargo test` here)
cd /Users/ember/dev/don && DON_NET_FULL_CORPUS=1 cargo test -p don-net --release -- --nocapture
#   -> all 63 recordings, 57 s; this is where the headline numbers come from
```

The corpus tests skip loudly (never silently pass) when `ron-data/replays/` is absent.

---

## 8. What I could not establish

- **Whether `join_ip` still functions.** The symbol exists; the path is untested. This is
  the single most valuable unknown for the lane's actual goal.
- **The transport framing beneath `GenericNetPacket`** — how PlayFab Party segments,
  orders, reliably delivers, and whether the game adds a header of its own. Needs a live
  capture, which I did not attempt.
- **The join handshake as a sequence.** I have every function's name and signature; I
  inferred the ordering from names and did not trace a single call graph or observe a live
  join.
- **The sixteen `check_all` algorithms.** Still the binding constraint on any real client.
  `CheckSums::check_all` at `0x00936560` is now named; the channels are not derived.
- **The desync state machine** past `recover_from_oos`'s plurality vote — what the losing
  client actually does.
- **The cause of the 21 checksum divergences.** §4.3 now establishes they are real
  simulation drift rather than decode error, and localises them to `units`/`builds`/
  `groups`/`guys`/`leaders`/`world`. *Why* those channels drift is open.
- **The meaning of `CommandPackage::valid`** (`+0x08`). We now have the engine's name for
  it; a name is not a semantics.
- **The two malformed packages and two stream-less files** are characterised but not
  explained.
- **`CommandPackage::padding` (`+0x214`)** — the reader uses a stack local seeded from the
  same global, so I never established what the *member* is for. The writer side
  (`add_group` / `send_local_package`) was not traced, which means I have not confirmed the
  encoder would produce padding the retail engine accepts. `don-net` reproduces the
  *reader's* model, validated on five million real commands; that is not the same as being
  able to *transmit*.
- **`PTR_DAT_00c061ec[0x820]`, `[0x821]`, `[0x823]` and `+0x81C`** — flag words used by the
  multiplayer gates. Not identified.
