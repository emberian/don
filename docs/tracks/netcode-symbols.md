# Netcode & subsystem symbols — the eight non-`rise` PDBs

**Track owner:** symbols lane. **Scope:** everything in `ron-bin/sbl/` *except* `rise.pdb`, plus
targeted cross-references into `rise.pdb` where the transport story is incomplete without them.

Everything below marked **[measured]** was read out of a PDB, a PE header, or a disassembly *in
this session*. **[inferred]** means it follows from symbols but was not executed. **[reported]**
means external knowledge, not verified here. Nothing here has been run against a live session.

---

## 0. What changed on disk

The install ships its own symbol store — `…/Rise of Nations/sbl/` — with **15 files**, not nine.
Two PDBs and six linker `.map`s were missing locally. All are now mirrored to
`ron-bin/sbl/`  **[measured]**:

| newly fetched | size | note |
|---|---|---|
| `xasl.pdb` | 15,970,304 | was missing |
| `version_maker.pdb` | 14,831,616 | was missing |
| `rise_z.map` | 13,498,927 | **full linker map for `riseofnations.exe`** — nobody had this |
| `CrossplayNetLib.map` | 3,037,617 | |
| `xasl.map` | 2,514,255 | |
| `dssl.map` | 2,259,996 | |
| `d3dgl.map` | 1,374,298 | |
| `wmstubber.map` | 60,372 | |

`rise_z.map` gives section-relative addresses for every symbol in the exe with no PDB parsing at
all; it is probably the cheapest ground truth in the repo for the rise lane. **[measured]**

Seven runtime DLLs are also now mirrored at `ron-bin/dll/` (`CrossplayNetLib`, `CrossplayProxy`,
`PartyWin`, `PlayFabMultiplayerWin`, `d3dgl`, `dssl`, `xasl`) so import/export tables and
disassembly are available without the VM. **[measured]**

### Transfer route (reusable)

The Parallels VM has a **read-write host shared folder**: guest `\\Mac\deos` → host
`/Users/ember/dev/breadstuffs`. `prlctl exec` runs as SYSTEM so the mapped drive `Z:` is *not*
visible, but the UNC path is. `copy /Y "<src>" \\Mac\deos\<dir>\` works and moves 57 MB in
seconds. This is far better than the `certutil -encode` + `type` route in `README-LLM.md`, which
should be treated as the fallback. **[measured]**

### Build-tree facts, free from the CodeView records

Every shipped binary's PDB path is `E:\agent\_work\2\s\main\game\<name>.pdb` — an Azure DevOps
agent checkout, source root `main/`, with `main/Build/Production/<lib>/*.obj` as the intermediate
tree. `patriots.exe` is older: `D:\Projects\RoN\main\Game\launcher.pdb`. **[measured]**

CodeView keys (for symbol-server style lookup; note **msdl.microsoft.com does not have these** —
tried, 404 — the game's own `sbl/` is the only source): **[measured]**

```
riseofnations.exe    rise.pdb              51D4F21961C64F849D5BC3361B0D291F 1
CrossplayNetLib.dll  CrossplayNetLib.pdb   51C234A472AC40A19C6E752F236961D7 1
d3dgl.dll            d3dgl.pdb             BD52EA047A134BA0B95CC7C0DCE4A6F6 1
dssl.dll             dssl.pdb              F53880C2466C47BFBE684FFFFE62411C 1
xasl.dll             xasl.pdb              B33DA713A5DD464EBEA6B5F3764E7A1A 1
version_maker.pdb    (no shipped binary)   CCEADF6FB9264C738C64EE1D3FA14141 1
```

### PDB quality

| PDB | stripped? | types? |
|---|---|---|
| `rise`, `CrossplayNetLib`, `CrossplayProxy`, `d3dgl`, `dssl`, `xasl`, `version_maker` | **no** | full TPI — struct layouts, vtable offsets, enums |
| `PartyWin`, `PlayFabMultiplayerWin` | **yes** | publics only (Microsoft-shipped) |

Tooling: `llvm-pdbutil` from `/opt/homebrew/Cellar/llvm/22.1.8/bin/` (`dump --publics`,
`dump --types`, `dump --modules`). `pretty` does **not** work on macOS — it needs DIA. Demangle
with `llvm-undname`. A small layout/vtable printer for `dump --types` text lives in the scratchpad
(`work/lay.py`, `work/vt.py`) if it needs rebuilding. **[measured]**

---

## 1. Answers to the two direct questions

### 1a. `xasl.pdb` is **not** the script layer. It is XAudio2.

**XASL = XAudio2 Sound Library.** It is the sibling of `dssl` = DirectSound Sound Library. This is
settled by the compiland list, which is the source-file list: **[measured]**

```
xasl\xasl.obj  xasl\xasl_dll.obj  xasl\xasound.obj  xasl\sscommon.obj  xasl\ssobject.obj
xasl\ADPCM.obj  xasl\WaveData.obj  xasl\VoicePool.obj  xasl\VoicePoolRecord.obj
xasl\VoiceProcessingCallback.obj  xasl\VolumeControlXAPO.obj
```

versus `dssl\dssl.obj dssl\dsslsound.obj dssl\dsslstaticsound.obj dssl\sscommon.obj
dssl\ssobject.obj dssl\pathpool.obj`. `sscommon.obj`/`ssobject.obj` are shared: both DLLs
implement the same `SoundSys` interface. Classes: `XASLSys`, `XASound`, `VoicePool`,
`UVoicePoolRecord`, `SSSound`, `CXAPOBase` (XAudio2 Audio Processing Object). Both export exactly
`get_soundsys_object_ptr` / `delete_soundsys_object_ptr`. **[measured]**

There is **no** BHS/script content in `xasl.pdb` — no compiler, no VM, no `scriptfunctions`
reference. The name collision with "script language" is a coincidence. **Clean negative; do not
revisit.** The BHS script engine lives in `riseofnations.exe` (`cScriptSync` is one of the 37
`SyncTags`, and the script system is in-process).

### 1b. `version_maker.pdb` is a build-time console tool. Irrelevant.

Its entire non-library content is **two objects**: `version_maker\version_maker.obj` and
`version_maker\version.obj`. Everything else is BHG `basic.lib` (`str/xml/file/text/tokenstring/
arith`), `zlib`, and CRT. It links `_main`, `__set_app_type`, `_seh_filter_exe` — a console EXE,
not a DLL — and touches `GetUserNameW`, `GetPrivateProfileStringW`, `GetCurrentDirectoryW`,
`WINHTTP`, `steam_api`. It stamps build/version metadata at build time. **No shipped binary
matches it** (no `version_maker.exe` in the install). **[measured]**

**Clean negative.** The only residual value is that it is a *pure* sample of BHG `basic.lib`, so
diffing another PDB's symbol set against it isolates that DLL's own code.

---

## 2. What each of the eight DLLs actually is

| PDB | what | own source dir | export ABI |
|---|---|---|---|
| `CrossplayNetLib` | **`NetSys` implementation** — session/player/pulse/ready/drop state machine. No sockets. | `CrossplayNetLib\` | `get_netsys_object_ptr` + 10 direct calls |
| `CrossplayProxy` | Q-LOC's PlayFab bridge: Lobby + Matchmaking + **PlayFab Party** P2P, behind `ICrossPlayService` | `CrossplayProxy\` | `Service()`, `Logger()` |
| `PartyWin` | **Microsoft PlayFab Party** — the actual UDP/DTLS transport | (MS binary) | 145 `Party*` C exports |
| `PlayFabMultiplayerWin` | **PlayFab Multiplayer SDK** — Lobby + Matchmaking + PubSub (SignalR/asio/websocketpp) | (MS binary) | 53 `PFLobby*`/`PFMultiplayer*` exports |
| `d3dgl` | D3D11 `GraphSys` (BHG "SGL" = Sky Graphics Library) + FreeType | `d3d11gl\`, `freetype\` | `get_graphsy_object_ptr` |
| `dssl` | DirectSound `SoundSys` | `dssl\` | `get_soundsys_object_ptr` |
| `xasl` | XAudio2 `SoundSys` | `xasl\` | `get_soundsys_object_ptr` |
| `version_maker` | build tool | `version_maker\` | n/a |

All of them statically link the same BHG engine core (`basic.lib`: `String`, `SkyString`,
`TFileSystem`, `TFile`, `ModManager`, `ModPackage`, `XMLNode`, `XMLElement`, `Log`, `M::Matrix`,
`M::Vector`, `Text`, `CloudFile`, zlib) — which is why a symbol dump of any of them is ~70 %
duplicate engine code. **[measured]**

### The subsystem plug-in ABI — important

`riseofnations.exe` does **not** import `d3dgl`, `dssl`, or `xasl`. It `LoadLibrary`s them and
`GetProcAddress`es a factory. All three factories share one signature:
`void* get_<X>_object_ptr(void* mem_vtable, void* sys_globals)`. **[measured]**

| loader | `GetProcAddress` name | in `re/decomp-all/` |
|---|---|---|
| graphsys | `get_graphsy_object_ptr` | `0054abf0.c` |
| soundsys | `get_soundsys_object_ptr` | `0053fb90.c` |
| netsys | `get_netsys_object_ptr` | `00538490.c` |

In all three the DLL **base name is a `String` argument** (`this` = `String` at `in_ECX`), not a
literal — no `"d3dgl"`, `"dssl"`, `"xasl"` string exists anywhere in `riseofnations.exe`
**[measured]**. The graphsys loader additionally consults `rise.ini` / a
`RISE_OF_NATIONS` registry key before loading (`PTR_u__rise_ini_00c8d384`,
`PTR_u_RISE_OF_NATIONS_00c8d370` at `0x54ac60`) **[measured]** — so the subsystem DLL choice looks
externally overridable. **Confirming that override path is the single highest-value follow-up for
headless.** **[inferred]**

---

## 3. The networking stack, top to bottom

```
  riseofnations.exe
    TurnControl / NetDaemon / CommandManager / DropControl / ConnectionData
      │  GenericNetPacket, type = NetMsgType 0..32,64          ← game protocol
      ▼
    NetSys  (abstract, 65 virtuals, vtable 0..0x100)
      │  send / send_all / get / host / join / poll_*          ← engine interface
      ▼
    CrossplayNetLibSys  (CrossplayNetLib.dll) — the ONLY implementation in this build
      │  own packets, type = InternalPacketType 128..136       ← netlib protocol
      │  ICrossPlayService::P2PSend / SetReceivedP2PDataCallback
      ▼
    CrossplayProxy::CrossPlayService  (CrossplayProxy.dll)
      │  NetworkFSM → INetworkClient::SendP2PData
      ▼
    PlayFab Party  (PartyWin.dll)   PartyEndpointSendMessage / OnEndpointMessageReceived
      │  UDP + DTLS, direct P2P where possible, PlayFab relay otherwise
      ▼
    WS2_32 / IPHLPAPI / bcrypt / SspiCli / CRYPT32
```

### 3.1 Transport: PlayFab Party only. No sockets in the game's own code.

`CrossplayNetLib.dll`'s import table contains **`WININET`, `steam_api`, `SHFOLDER`, `KERNEL32`,
`USER32`, `ADVAPI32`, `ole32`, `OLEAUT32`, `MSVCP140`, `VCRUNTIME140`, CRT, `WINMM`** — and
**no `WS2_32`** **[measured]**. `CrossplayProxy.dll` imports only `PlayFabMultiplayerWin.dll`,
`PartyWin.dll`, `WINHTTP`, CRT — also no `WS2_32` **[measured]**. `PartyWin.dll` imports
`WS2_32`, `IPHLPAPI`, `bcrypt`, `SspiCli`, `CRYPT32`, `WINHTTP` **[measured]**.

So *all* datagram I/O for a multiplayer game happens inside `PartyWin.dll`. `WININET` in
`CrossplayNetLib` is for session enumeration/HTTP only (`m_requesting_http`,
`m_last_http_request_time`, `fill_url_string`, `get_url_string`).

Party's internals (stripped PDB, publics only): `CXrnmLink`, `CXrnmEndpoint`, `CXrnmSendChannel`,
`CXrnmSendPkt`, `CXrnmNatTraverser`, `CXrnmDtlsState`, `CXrnmNetworkPathEvaluator`,
`CXrnmNetworkPathHop`, `CXrnmSendThrottle`, `BumblelionNetwork` (Party's internal codename),
`NetworkModelImpl`, `EndpointModelImpl`, `JitterBufferImpl` **[measured]**. Strings confirm relay
fallback: `RelayServer`, `RelaySaidToUseDirectPeerConnectivity`, `RelayRoundTripLatency`,
`AverageRelayServerRoundTripLatencyInMilliseconds`, `.playfabapi.com`, `.playfabsandbox.com`
**[measured]**.

**Answer to "sockets? PlayFab relay? both?": DTLS-over-UDP, direct peer-to-peer when NAT
traversal succeeds, PlayFab relay servers otherwise, chosen inside Party. The game has no socket
code of its own and no LAN/direct-IP path that is still wired up** (see §3.3). **[measured]**

### 3.2 Session lifecycle — three layers, named

**Discovery / lobby (PlayFab Multiplayer, `PlayFabMultiplayerWin.dll`)** — 53 C exports, of which
the lifecycle ones are: **[measured]**

```
PFMultiplayerInitialize / PFMultiplayerUninitialize / PFMultiplayerSetEntityToken
PFMultiplayerCreateAndJoinLobby   PFMultiplayerJoinLobby   PFMultiplayerJoinArrangedLobby
PFLobbyAddMember  PFLobbyPostUpdate  PFLobbyLeave  PFLobbyForceRemoveMember
PFLobbyGetConnectionString  PFLobbyGetMembers  PFLobbyGetOwner  PFLobbyGetLobbyProperty
PFMultiplayerCreateMatchmakingTicket  PFMatchmakingTicketGetMatch  PFMatchmakingTicketGetStatus
PFMultiplayerStartProcessingLobbyStateChanges / …Finish… (pump)
PFMultiplayerStartListeningForLobbyInvites  PFLobbySendInvite
```

**Transport session (PlayFab Party, `PartyWin.dll`)** — the ones that matter: **[measured]**

```
PartyInitialize / PartyCleanup
PartyCreateLocalUser / PartyLocalUserUpdateEntityToken / PartyDestroyLocalUser
PartyCreateNewNetwork          → produces a PartyNetworkDescriptor
PartySerializeNetworkDescriptor / PartyDeserializeNetworkDescriptor
PartyConnectToNetwork          → JOIN, takes the deserialized descriptor
PartyNetworkAuthenticateLocalUser
PartyNetworkCreateEndpoint / PartyNetworkDestroyEndpoint
PartyEndpointSendMessage       → the actual datagram send
PartyNetworkLeaveNetwork
PartyStartProcessingStateChanges / PartyFinishProcessingStateChanges  (pump)
```

The **join token is the serialized network descriptor string** — `NetworkData::m_networkDescriptor`
is a `std::string` and `INetworkClient::JoinNetwork(const std::string&, callback)` takes exactly
that. It is published into the lobby, which is why `PFLobbyGetConnectionString` and the
`LobbyDTO::_attributes` map exist. **[measured]** / **[inferred]** for the publish path.

**Q-LOC wrapper (`CrossplayProxy.dll`)** — `CrossplayProxy::NetworkFSM` implements
`INetworkClient` and owns a `unique_ptr<AbstractNetworkState>`. The state set is exactly:
`NullNetworkState`, `DisconnectedNetworkState`, `CreatingNetworkState`, `ConnectingNetworkState`,
`ConnectedNetworkState`, `LeavingNetworkState`, `ReconnectingNetworkState` **[measured]**.
`AbstractNetworkState` has **66 vtable slots, 58 of them `On*(const PartyStateChange* const)`
handlers** — a 1:1 dispatch of the Party state-change enum. The other eight are the dtor,
`OnEnter`, and the six `INetworkClient` network/send operations. `NetworkManager` holds
`unordered_map<string, shared_ptr<NetworkFSM>>` — one FSM per network; the game uses two
(`m_gameNetwork`, `m_chatNetwork`). **[measured]**

`INetworkClient` vtable (byte offsets from `CrossplayProxy.pdb` TPI, type `0xE7EC`) **[measured]**:

```
+ 0 GetNetworkID()                        + 4 GetNetworkDescriptor()
+ 8 CreateNetwork(uint maxPlayers, cb(bool, networkId, descriptor))
+12 JoinNetwork(const string& descriptor, cb(bool, err))
+16 LeaveNetwork(cb)
+20 SendP2PData(const string& peerId, uint size, const void* data)  → bool
+24 SendP2PDataToAll(uint size, const void* data)                   → bool
+28 SendP2PTextData(...)                 +32 SendP2PTextDataToAll(...)
+36 SetPlayerJoinedCallback  +40 SetPlayerLeftCallback
+44 SetP2PDataReceivedCallback(cb(peerId, endpointId, uint size, const void* data))
+48 SetP2PTextDataReceivedCallback  +52 SetFailedConnectionToNetworkCallback
+56 ResetCallbacks
```

`Crossplay::ICrossPlayService` (type `0xE88D`/`0x51C0`) is the game-facing façade, **58 pure
virtuals**, vtable `+0` … `+228`. The lifecycle-relevant slots **[measured]**:

```
+  0 Init                       +  4 SetServiceUrl              +  8 SetServiceErrorCallback
+ 12 SetReliability(bool)       + 16 StartSession(userId, onOk, onErr)   + 20 StopSession
+ 24 SetRenewTokenCallback      + 28 GetSessionStatus           + 32 GetCrossplayStatus
+ 48 GetLobby                   + 52 FindLobbies(LobbySearchCriteriaDTO, …)
+ 56 CreateLobby(int maxMembers, Visibility, attributes, onOk, onErr)
+ 60 JoinLobby(const wstring& lobbyId, onOk(LobbyDTO), onErr(int, wstring))
+ 64 SetJoinLobbyCallback(cb(JoinLobbyDTO, bool))
+ 68 LeaveLobby                 + 72 SetLeaveLobbyCallback
+ 76 UpdateLobby                + 80 SetUpdateLobbyCallback     + 84 LobbyCancelPendingRequests
+ 88 StartGame(lobbyId, bool, onOk, onErr)      + 92 CancelGameStart
+ 96 SendLobbyChat              +100 SetLobbyMessageReceivedCallback
+124 CreateInvitation           +128 AcceptInvitation           +132 GetInvitationId
+160 SetP2PConnectionOpenedCallback     +164 SetP2PConnectionClosedCallback
+168 SetP2PDataChannelOpenedCallback    +172 SetP2PDataChannelClosedCallback
+176 SetReceivedP2PTextCallback
+180 SetReceivedP2PDataCallback(cb(ICrossplayPlayer*, const uchar*, uint))
+184 SetP2PConnectionFailedCallback     +188 SetP2PTimeoutDuration(int)
+192 P2PStartConnection(const wstring& a, const wstring& b)
+196 P2PSendToAll(uchar*, uint) → bool  +200 P2PSend(ICrossplayPlayer*, uchar*, uint) → bool
+204 P2PCloseAll                        +208 P2PClose(const wstring&)
+212 IsConnectedToHub → bool            +216 CreateLocalPlayerLoopback
+220 SetUsername                        +224 GetPlayerGuid → wstring&
+228 Tick                               ← must be pumped every frame
```

> **Trap.** `CrossplayProxy.pdb` also contains a *second, larger* declaration named
> `Crossplay::ICrossPlayService` — 96 slots, `+0 ~ICrossPlayService`, `+4 SetNew`, `+8 SetDelete`,
> and extra APIs this build does not use (`P2PEnableTcp(bool)`, `SetP2PPacketOrderedDelivery`,
> `SetP2PMaxPacketLifetimeMs`, `SetP2PAllowedPorts`, `CreateTicket`/`CreateLobbyTicket`/
> `CancelTicket`/`SetMatchFoundCallback` matchmaking). It is **not** the shipped vtable:
> `CrossplayProxy::CrossPlayService` derives from `CrossplayProxy::ICrossPlayService`, whose
> 58-slot layout is byte-identical to the table above and to the copy in `CrossplayNetLib.pdb`.
> Use the 58-slot layout. The 96-slot record is evidence that the vendor SDK has a TCP P2P mode
> and a matchmaking-ticket API that RoN:EE leaves unused. **[measured]**

`Crossplay::P2P::ICrossplayPlayer` (14 virtuals) is the per-peer handle: `Send(uchar*, int)`,
`Send(const wstring&)`, `CloseConnection`, `IsDataChannelOpen`, `SetReceivedDataCallback`,
`GetId() → const wstring&`, plus mute/unmute (voice). **[measured]**

`CrossplayProxy::CrossplayProxyPlayer` is the implementation and is **28 bytes: vptr + one
`wstring m_playerID`** — the peer identity is a PlayFab entity-ID string, nothing else.
**[measured]**

### 3.3 `NetSys` — the engine's abstract network interface (65 virtuals)

Defined in `CrossplayNetLib.pdb` type `0x4572`, base object 88 bytes: **[measured]**

```
+ 0 __vftable   + 4 int num_players   + 8 NetPlayer* players[8]   +40 NetPlayer* local_player
+44 NetPlayer* host_player  +48 eLocalPlayerDisconnect local_player_connection
+52 float local_player_disconnect_pct  +56 Array<NetSession*> net_sessions  +84 Log* log
enum NetSys::eLocalPlayerDisconnect { LOCAL_PLAYER_CONNECTED=0, _DISCONNECTING=1, _DISCONNECTED=2 }
```

The vtable, by byte offset — **this is the entire contract a headless client must satisfy or
replace** **[measured]**:

```
  0 ~NetSys                                 4 init(NetMessenger*, GUID*, int, int, ulong, ulong)
  8 close                                  12 get_memory_manager
 16 is_host                                 20 enable_join(int)
 24 set_playing(int)                        28 is_playing
 32 is_joining_in_process                   36 is_session_full
 40 get_url_string                          44 accept_host_messages(int)
 48 set_number_players(int)                 52 set_number_observers(int)
 56 send_dsync(int)                         60 check_pulse
 64 set_time_out(ulong)                     68 get_time_out
 72 set_allow_timeout(int)                  76 get_allow_timeout
 80 get_num_allowed_players
 84 send(GenericNetPacket*, int size, const NetPlayer* to, int flags) → bool
 88 send_all(GenericNetPacket*, int size, int flags)                 → bool
 92 get(GenericNetPacket*, const NetPlayer**, ulong*)                → bool
 96 poll_services(Array<NetService>*)
100 host(const String&, const String&, const String&, const String&, const String&)
104 join(const NetSession*, const String&, const String&, const String&, int)
108 join_ip(const String&, const String&, const String&, const String&, int, int)
112 cancel_joining     116 cancel_join      120 cancel_join_skybox   124 disconnect(bool)
128 poll_sessions(const String&)            132 stop_poll_sessions
136 clear_net_sessions(ulong)               140 poll_players(const NetSession*, Array<NetPlayer*>)
144 find_player_from_id(wstring)            148 validate_player(const NetPlayer*)
152 update_recently_played_with_list        156 process_system_messages
160 send_drop_player   164 drop_player      168 cancel_drop_player
172 set_log   176 log_set_frame   180/184 log_connection   188 log_connection_fmt
192 get_ip_addresses   196 get_host_port    200 set_host_port
204 get_local_port     208 set_local_port   212 set_ip_override(const String&)
216 set_matchmaking_id(int)                 220 get_num_players
224 error_set_callback(void(*)(int))
228 get_group_data   232 get_service_data   236 delete_group_data   240 delete_service_data
244 log_state         248 notify_waiting_on_player(int, ulong)
252 set_profiler      256 cleanup_system
```

`join_ip` and `enum { SESSION_INVALID=0, SESSION_LAN=1, SESSION_GAMESPY=2 }` survive in the
headers, but **`CrossplayNetLibSys` is the only `NetSys` subclass present in this build**: a
publics sweep of `rise.pdb` finds `@NetSys@@` (the abstract base) and `@CrossplayNetLibSys@@`
(imports) and nothing else. **[measured]** The DirectPlay/GameSpy implementations are gone.

Companion interfaces **[measured]**:

- `NetPlayer` (21 virtuals): `is_local/is_host/is_pending/is_observer`, `get_internal_name`,
  `get_id() → wstring`, `get_platform_id`, `get_platform`, `get_player_index`, `get_game_version`,
  `get_ping_time`, `get_time_since_last_pulse`, `get_send_queue_info(ulong*, ulong*)`,
  `reset_sync_counter`/`inc_sync_counter`/`get_sync_counter`.
- `NetMessenger` (12 virtuals — the **callback interface the game implements**):
  `on_send_failed`, `on_player_added`, `on_name_changed`, `on_session_lost`, `on_host_migrate`,
  `on_join(int, ulong)`, `on_player_deleted`, `on_player_timed_out`, `on_player_pulse`,
  `allow_connection(const GenericSessionData*) → int`, `get_session_data()`.
  **`rise.pdb` shows `NetDaemon` implements exactly this vtable** — `NetDaemon` *is* the game's
  `NetMessenger`. **[measured]**
- `NetSession` (6 virtuals): `get_internal_name`, `get_max_players`, `get_current_players`,
  `get_latency`, `get_session_data`.

### 3.4 `CrossplayNetLibSys` — 976 bytes, the whole session state

Type `0x5024`. Selected fields **[measured]**:

```
+  0 : NetSys                       + 88 int flags                 + 92 NetMessenger* net_messenger
+ 96 CrossplayNetLibSession current_session       +160 void* peer
+176 GenericSessionData session_data              +188 _GUID app_guid
+204 Crossplay::ICrossPlayService* m_crossplay     +208 Crossplay::Lobby::DTO::LobbyDTO m_lobby
+416 ObjectArray<String> ip_addresses  +440 String ip_override
+460 ulong host_port   +464 ulong local_port      +468 bool lobby_launched
+472 NetSysFifo fifo_sys        +508 NetFifo fifo_receive
+544 NetFifo fifo_host          +580 NetFifo fifo_pulse
+616 ulong relative_time  +620 ulong time_out  +624 ulong game_version  +628 int matchmaking_id
+732 ulong last_pulse_time  +740 Log connection_log
+804 Array<HostPlayerList> cached_player_lists
+840 bool m_requesting_http  … HTTP session-enumeration timers …
+856 std::function<void(ICrossplayPlayer*)> m_dataChannelOpenedCallback
+896 std::function<void(ICrossplayPlayer*)> m_dataChannelClosedCallback
+936 std::function<void(wstring, wstring)>  m_connectionFailedCallback
```

Four mutex-guarded FIFOs (`NetFifo`/`NetSysFifo` = `{head, tail, count, CRITICAL_SECTION}`) —
receive, host, pulse, system. Party's receive callback pushes into these; `get()` pops.
**[inferred from field names + FIFO types]**

`CrossplayNetLibPlayer` (172 bytes) **[measured]**:

```
+ 4 Array<const CrossplayNetLibPlayer*> drop_requests
+32 int flags  (enum Flags { SNLPLAYER_HOST=1, SNLPLAYER_PENDING=2, SNLPLAYER_LOCAL=4,
                             SNLPLAYER_OBSERVER=8 })
+36 Crossplay::P2P::ICrossplayPlayer* crossplay_player     ← the Party peer handle
+40 bool ready                                             ← the readiness flag
+44 wstring unique_id   +68 wstring platform   +92 wstring platform_id
+116 String name  +136 String description
+156 ulong game_version  +160 ulong last_pulse  +164 ulong sync_counter  +168 int dsync_frame
```

`CrossplayNetLibSession` (64 bytes): `: NetSession`, `GenericSessionData session_data`,
`String session_name`, `int max_players`, `int current_players`, `ulong latency`,
`wstring session_id`. **[measured]**

### 3.5 The netlib's own wire protocol — `InternalPacketType`

Every packet on the wire starts with `struct GenericNetPacket { unsigned char type; }` (sizeof 1).
**Types ≥ 128 are the netlib's internal control messages; types < 128 are the game's.**
**[measured]**

```c
enum InternalPacketType {          // CrossplayNetLib.pdb, type 0x46AA
  IPT_BASE              = 128,
  IPT_PLAYERLIST        = 128,     // struct HostPlayerList        sizeof 34
  IPT_DROPREQUEST       = 129,     // struct DropRequest           sizeof 5
  IPT_CANCELDROPREQUEST = 130,     // struct CancelDropRequest     sizeof 5
  IPT_PULSEPACKET       = 131,     // bare GenericNetPacket        sizeof 1
  IPT_ADDPLAYER         = 132,     // struct AddPlayerRequest      sizeof 70
  IPT_DESTROYPLAYER     = 133,     // struct DestroyPlayerRequest  sizeof 5
  IPT_MIGRATEHOST       = 134,     // struct MigrateHostRequest    sizeof 5
  IPT_DSYNCMSG          = 135,     // struct DsyncNotification     sizeof 5
  IPT_READYFLAG         = 136,     // struct ReadyFlagRequest      sizeof 2
};
```

Exact layouts, all little-endian, all `#pragma pack(1)` (offsets prove it — `int` at +1)
**[measured]**:

```c
struct GenericNetPacket      { u8 type; };                                     // 1
struct AddPlayerRequest      { u8 type; char player_name[64]; i32 unique_id;
                               bool is_hosting; };                             // 70
struct DestroyPlayerRequest  { u8 type; i32 unique_id; };                      // 5
struct DropRequest           { u8 type; i32 unique_id; };                      // 5
struct CancelDropRequest     { u8 type; i32 unique_id; };                      // 5
struct DsyncNotification     { u8 type; i32 frame; };                          // 5
struct MigrateHostRequest    { u8 type; i32 new_host; };                       // 5
struct HostPlayerList        { u8 type; u8 num_players; i32 unique_ids[8]; };  // 34
struct ReadyFlagRequest      { u8 type; bool ready; };                         // 2
struct GenericSessionData    { u8 type; u8 validity_number; };                 // 2
struct Safe_CreateEnumHostsResponse {                                          // 1060
    void*  pAddressSender;  GenericSessionData session_data;
    u32 max_players; u32 current_players;
    wchar_t session_name[260]; wchar_t password[260]; u32 dwRoundTripLatencyMS; };
```

Handlers, one per type, all `CrossplayNetLibSys::` **[measured]**:
`process_playerlist(HostPlayerList*)`, `process_drop_request`, `process_cancel_drop_request`,
`process_pulse`, `process_create_player_message(AddPlayerRequest*)`,
`process_destroy_player_message`, `process_host_migrate_message`, `process_dsync`,
`process_ready_flag(const CrossplayNetLibPlayer*, ReadyFlagRequest*)`, plus
`process_host_enum_message(Safe_CreateEnumHostsResponse*)` and the dispatcher
`process_system_message()` / `process_system_messages()`.

Senders: `send_playerlist()`, `send_pulse()`, `send_ready_flag(bool)`, `reset_ready_flags()`,
`send_dsync(int)`, `send_drop_player(const NetPlayer*)`. **[measured]**

### 3.6 The game's wire protocol — `NetMsgType`

From `rise.pdb` type `0x57C3`. These are the `GenericNetPacket::type` values **below** 128:
**[measured]**

```c
enum NetMsgType {
  NETMSG_GENERIC=0, NETMSG_PLAYERCONNECTIONDATA=1, NETMSG_ALLPLAYERCONNECTIONDATA=2,
  NETMSG_GAMECONNECTIONDATA=3, NETMSG_GAMECONNECTIONDATAFULL=4, NETMSG_CHAT=5,
  NETMSG_PING=6, NETMSG_COMMANDPACKAGEDATA=7, NETMSG_PAUSE=8, NETMSG_TAUNT=9,
  NETMSG_SYNCSIGNAL=10, NETMSG_DROPSTAMP=11, NETMSG_TIMESYNC=12, NETMSG_DROPVOTE=13,
  NETMSG_DROPDECISION=14, NETMSG_GAMEMODSYNCREQUEST=15, NETMSG_GAMEMODSYNCRESPONSE=16,
  NETMSG_SYNCFILEBEGIN=17, NETMSG_SYNCFILERESPONSE=18, NETMSG_SYNCFILEDATA=19,
  NETMSG_SYNCFILEVERIFY=20, NETMSG_SYNCFILEEND=21, NETMSG_SYNCFILEERROR=22,
  NETMSG_SYNCDIRERROR=23, NETMSG_SYNCDIRREQUEST=24, NETMSG_SYNCDIRINFO=25,
  NETMSG_GAMESPYCHALLENGE=26, NETMSG_PLAYER_STATUS_REQUEST=27,
  NETMSG_PLAYER_STATUS_RESPONSE=28, NETMSG_DROP_FLAG=29, NETMSG_SPLINE=30,
  NETMSG_GAMECONNECTIONSCENARIO=31, NETMSG_GAMECONNECTIONMODINFO=32,
  NETMSG_RESPONSE_FLAG=64,
};
```

Layouts (packed, little-endian) **[measured]**:

```c
// ── THE TURN / COMMAND CHANNEL ──────────────────────────────────────────────
struct NetMsg_CommandPackageData {  // type = 7, sizeof 9 + payload
    u8   type;          // NETMSG_COMMANDPACKAGEDATA
    u32  stamp;         // turn stamp
    i8   play;          // player slot
    i16  data_size;
    u8   data[1];       // flexible: data_size bytes of CommandPackage payload
};

struct CommandPackage {             // rise.pdb 0x16081, sizeof 536, in-memory
    u32 stamp; i32 play; i32 valid; i32 group; i16 size; u8 data[512]; Random padding;
};

struct TurnDataCommand {            // rise.pdb 0x14FE1, sizeof 11 — a *Command*, not a NetMsg
    /* Command base, 1 byte */  u16 ping_time; u16 frame_average;
    u16 wait_time; u16 game_lag; u16 forced_loads;
};

// ── SESSION / SETUP ─────────────────────────────────────────────────────────
struct GameConnectionData {         // 46 bytes — the whole game setup
    u8 team_style, map_style, map_size, players, max_observers, game_speed, game_rules,
       difficulty, starting_town, starting_resources, starting_resources2, tech_cost,
       reveal_map, pop_limit, rush_rules, cannon_times, starting_technology,
       starting_technology2, ending_technology, elimination, victory, wonderwin,
       score_goal, popwin, time_limit, chairs, econwin, scenario_type, script_type, mods;
    /* union: u8 data[30] */
    i32 lobby_elo; u32 seed; i32 flags;
    u16 checksum_window_size; u8 checksum_deep; u8 checksum_failure_threshold;
};
struct GameConnectionDataFull {     // 2031 bytes = GameConnectionData + scenario/script/save/mod
    GameConnectionData data;
    char scenario_name[180];        // union'd with script_name[180]
    wchar_t save_name[260]; char desc[512];
    char mod_name[180]; char mod_desc[512];
    u32 mod_size; u32 mod_checksum; u64 mod_workshop_id;
    ScenFilePreviewData scenario_data;
    u32 scenario_size; u32 scenario_checksum; u32 scenario_num_files; u64 scenario_workshop_id;
};
struct PlayerConnectionData {       // 59 bytes
    wstring player_id; wstring platform_player_id;   // 24 B each (MSVC SSO string)
    i32 elo; u8 slot_type, tribe, who, team, handicap, diff, ready;
};

// ── CONTROL ─────────────────────────────────────────────────────────────────
struct NetMsg_Chat     { u8 type; u8 observer_to_all; wchar_t message[256]; }; // 514
struct NetMsg_Ping     { u8 type; i32 cx; i32 cy; };                           // 9
struct NetMsg_Taunt    { u8 type; u8 taunt; };                                 // 2
struct NetMsg_Generic  { u8 type; char buffer[512]; };                         // 513
struct NetMsg_Pause    { u8 type; i32 play; u32 pause_time; u8 requested_state; }; // 10
struct NetMsg_TimeSync { u8 type; u32 time_stamp_sent; u32 time_stamp_received; }; // 9
struct NetMsg_SyncSignal { u8 type; i32 play; };                               // 5
struct NetMsg_DropStamp  { u8 type; i32 play_from; i32 stamp; i32 play; };      // 13
struct NetMsg_DropFlag   { u8 type; i32 pid; };                                // 5
struct NetMsg_DropVote   { u8 type; u8 play; char vote; };                     // 3
struct NetMsg_DropDecision { u8 type; char vote; };                            // 2
struct NetMsg_Spline   { u8 type; u8 spline_type, spline_flags, spline_cmd;
                         u16 len; /*8B*/ vert_data; };                         // 14
struct NetMsg_PlayerStatusRequest  { u8 type; };                               // 1
struct NetMsg_PlayerStatusResponse { u8 type; u32 player_id[8];
                                     u32 time_since_last_pulse[8]; };          // 65
```

### 3.7 How turn data is sent and received — the named path

**Send (per turn):** `TurnControl::do_frame_multi()` → `TurnControl::compute_next_turn_multi()` →
`CommandManager` builds a `CommandPackage` (`stamp`, `play`, `group`, `size`, `data[512]`) →
wrapped as `NetMsg_CommandPackageData` (type 7) → `NetSys::send_all(GenericNetPacket*, size,
flags)` → `CrossplayNetLibSys::send_all` → `ICrossPlayService::P2PSendToAll(uchar*, uint)` →
`INetworkClient::SendP2PDataToAll` → `PartyEndpointSendMessage`. **[measured]** for every named
symbol and every vtable slot; **[inferred]** for the exact call chain, which was not traced
instruction-by-instruction.

**Receive:** Party `OnEndpointMessageReceived` (`AbstractNetworkState` vft+120) →
`NetworkClientCallbacks::m_onDataReceived(peerId, endpointId, uint size, const void* data)` →
`CrossPlayService::m_onReceivedP2PData` → `CrossplayNetLibSys` pushes into `fifo_receive` →
game calls `NetSys::get(GenericNetPacket*, const NetPlayer**, ulong*)` → `NetDaemon::process_all()`
dispatches on `type` → `TurnControl::process_turn_data(const TurnDataCommand*, int)` and
`CommandManager`. **[measured]** symbols; **[inferred]** wiring.

`TurnControl` (376 bytes) carries the lag/latency model: `turn_length`, `turn_counter`,
`target_frame_time`, `min_turn_time`, `extra_time`, and six 8-entry per-player ring buffers
(`last_package_sizes`, `last_wait_times`, `last_lag_times`, `last_average_frame_times`,
`last_ping_times`, `last_diff_times`, `last_forced_loads`) plus `max_wait_times[10]`.
`static int* TurnControl::timings`. **[measured]**

Desync detection: `enum SyncTags` (37 tags + `cInvalidTag=255`) is present in **every** PDB
(shared header) — `cCommandManagerSync=4`, `cTurnTuningSync=5`, `cTurnControlSync=6`,
`cTimeSync=7`, `cNetDaemonSync=8`, `cDropControlSync=9`, `cConnectionDataSync=10`, …,
`cChecksumSync=31`, `cScriptSync=34`, `cGpieceVerifySync=35`. **[measured]**

### 3.8 How a client signals readiness — **fully resolved**

Two separate readiness mechanisms, both named:

1. **Netlib-level ready flag.** `CrossplayNetLibSys::send_ready_flag(bool)` emits
   `ReadyFlagRequest { u8 type=IPT_READYFLAG(136); bool ready; }`. The peer's
   `process_ready_flag(const CrossplayNetLibPlayer*, ReadyFlagRequest*)` sets
   `CrossplayNetLibPlayer::ready` at offset +40. `reset_ready_flags()` clears all.
   **These two are among the nine functions `riseofnations.exe` imports directly** — see §4.
   **[measured]**
2. **Lobby-level ready.** `PlayerConnectionData::ready` (u8 at +58) inside
   `NETMSG_PLAYERCONNECTIONDATA`(1)/`NETMSG_ALLPLAYERCONNECTIONDATA`(2), with
   `ConnectionData::send_player(int, bool)`, `ConnectionData::send_game()`,
   `ConnectionData::unready_all_players()`. **[measured]**

Liveness is separate: `send_pulse()`/`process_pulse()`/`check_pulse()` with
`CrossplayNetLibPlayer::last_pulse`, `NetSys::set_time_out(ulong)`,
`CrossplayNetLibSys::is_timed_out(CrossplayNetLibPlayer*, ulong)`,
`NetMessenger::on_player_pulse`. **[measured]**

---

## 4. The exe's crossplay surface — all 11 imports, decoded

`riseofnations.exe` imports **9** symbols from `CrossplayNetLib.dll` and **2** from
`CrossplayProxy.dll`. Everything else in `CrossplayNetLib` is reached through the `NetSys`
vtable obtained from `get_netsys_object_ptr`. IAT addresses **[measured]**:

```
0xac5010  CrossplayNetLibSys::OnHostUpdated(const std::wstring&)
0xac5014  CrossplayNetLibSys::set_p2p_callbacks(
              std::function<void(Crossplay::P2P::ICrossplayPlayer*)> dataChannelOpened,
              std::function<void(Crossplay::P2P::ICrossplayPlayer*)> dataChannelClosed,
              std::function<void(std::wstring, std::wstring)>        connectionFailed)
0xac5018  CrossplayNetLibSys::OnPlayerLeft(const Crossplay::Lobby::DTO::LobbyMemberDTO&)
0xac501c  CrossplayNetLibSys::IsHost(const Crossplay::Lobby::DTO::LobbyMemberDTO&) → bool
0xac5020  CrossplayNetLib::is_connected_to_network() → bool                 [free function]
0xac5024  CrossplayNetLibSys::OnPlayerJoined(const LobbyMemberDTO&, const std::wstring&)
0xac5028  CrossplayNetLibSys::OnPlayerLeft(const CrossplayNetLibPlayer*)
0xac502c  CrossplayNetLibSys::reset_ready_flags()
0xac5030  CrossplayNetLibSys::send_ready_flag(bool)

0xac5038  Crossplay::Logging::Logger() → ICrossplayLogger*
0xac503c  Crossplay::Service() → ICrossPlayService*     [the whole 58-virtual façade]
```

`CrossplayNetLib.dll` exports one more symbol not in the exe's import table:
`CrossplayNetLib::set_network_connection_state(bool)`, plus the factory `get_netsys_object_ptr`.
**[measured]**

The shape of this list is the story: **the exe drives the lobby, and the lobby drives the
netlib.** The game gets a `LobbyMemberDTO` from PlayFab, hands it to `OnPlayerJoined`/
`OnPlayerLeft`/`IsHost`, and the netlib materialises `CrossplayNetLibPlayer` objects from it.
`Crossplay::Service()` is the single entry point to everything else.

---

## 5. What a minimal headless client must implement

Three architectures, in increasing order of fidelity and effort.

### Option A — replace `CrossplayNetLib.dll` (recommended for interaction testing)

Ship our own DLL exporting `get_netsys_object_ptr` and the ten direct symbols, implementing the
65-slot `NetSys` vtable over whatever transport we like (loopback, TCP, a file). The real game
binary then runs unmodified and plays a real match against our harness.

- **Established:** the complete `NetSys`/`NetPlayer`/`NetMessenger`/`NetSession` vtables with byte
  offsets (§3.3), the exact mangled names of the 11 imports (§4), the `Liberr` return enum (41
  values, `LIBERR_OK=0`), the `NetSys` base-object layout, the packet framing (§3.5, §3.6).
- **Unestablished:** the `MemMgr`/`sys_globals` argument pair passed to
  `get_netsys_object_ptr` — we know it is `(&PTR_vftable_00c8ccf0, PTR_PTR_00c06378)` at
  `0x538490` but not the `MemMgr` vtable contract. Also `NSService`/`NSGroup`/`SysProfile`.
- **Cost:** ~65 stubs, of which maybe 20 need real behaviour. Does not need Party, PlayFab, or the
  internet. **This is the cheapest path to a live opponent for protocol testing.**

### Option B — a native peer that speaks the real protocol

A Rust process that joins a real Party network and exchanges real packets with a retail client.

Must implement, in order:

1. **PlayFab auth** — obtain an entity token. `CrossplayProxy` uses `PlayFab::PlayFabClientAPI`
   over `WINHTTP` to `*.playfabapi.com`; the title ID is in `CrossplayProxy.dll` data.
   **[measured]** that the machinery exists; **[unestablished]** the title ID value and the login
   flow the game actually uses.
2. **PlayFab Lobby** — `PFMultiplayerInitialize`, `PFMultiplayerSetEntityToken`,
   `PFMultiplayerJoinLobby` / `PFMultiplayerCreateAndJoinLobby`, pump with
   `PFMultiplayerStartProcessingLobbyStateChanges`. The lobby carries the Party **network
   descriptor** in its attributes. **[measured]** API; **[unestablished]** the attribute key names
   (`QlocLobbyPropertyData::SEARCH_DATA_PAIR_SEPARATOR` and
   `QlocLobbyMemberData::ATTRIBUTE_PREFIX` are static consts in `CrossplayProxy` — read their
   values from the DLL's `.rdata` to get them).
3. **PlayFab Party** — `PartyInitialize`, `PartyCreateLocalUser`,
   `PartyDeserializeNetworkDescriptor(descriptor)`, `PartyConnectToNetwork`,
   `PartyNetworkAuthenticateLocalUser`, `PartyNetworkCreateEndpoint`, then
   `PartyEndpointSendMessage` / drain `PartyStartProcessingStateChanges`. Both DLLs are 32-bit
   Windows binaries, so this side has to run on Windows (the VM) or be reimplemented.
   **[measured]** API surface; **[unestablished]** the Party network configuration the game uses.
4. **Netlib framing** — `AddPlayerRequest`(132) to announce, answer `HostPlayerList`(128), reply
   to `PULSEPACKET`(131), emit `ReadyFlagRequest`(136). Layouts are exact (§3.5). **[measured]**
5. **Game framing** — `NETMSG_GAMECONNECTIONDATAFULL`(4) + `NETMSG_ALLPLAYERCONNECTIONDATA`(2) to
   agree setup and RNG `seed`, then `NETMSG_COMMANDPACKAGEDATA`(7) per turn wrapping the
   already-decoded `CommandPackage` stream (`docs/replay-format.md`,
   `docs/derivation/replay-io.md`), with `NETMSG_TIMESYNC`(12) and `NETMSG_SYNCSIGNAL`(10) for the
   clock. Layouts are exact (§3.6). **[measured]**
6. **Determinism** — `GameConnectionData.seed`, `checksum_window_size`, `checksum_deep`,
   `checksum_failure_threshold` are negotiated in the setup message, and the existing checksum
   work (`check_all` `FUN_00936560`, `process_check_sums` `FUN_009459d0`) is what
   `IPT_DSYNCMSG`(135) reports against. **[measured]** the fields; **[inferred]** the linkage.

**The gap that remains is authentication and the Party session configuration, not the protocol.**
The protocol is now fully specified above.

### Option C — LAN / direct IP

`NetSys::join_ip` and `SESSION_LAN` exist in the interface but **no implementation of them
survives in this build** (§3.3). Do not plan around it.

---

## 6. `d3dgl.pdb` and running without a GPU

### The bad news, measured

`D3D11Context::create_device` (`d3dgl.dll` `.text+0x100304`, VA `0x100197d0`) calls
`D3D11CreateDevice` with these arguments — read directly off the push sequence **[measured]**:

```
pAdapter        = NULL
DriverType      = 1   (D3D_DRIVER_TYPE_HARDWARE)      ← hard-coded, no fallback
Software        = NULL
Flags           = 0x20 (D3D11_CREATE_DEVICE_BGRA_SUPPORT)
pFeatureLevels  = { 0xB000, 0xA100, 0xA000 }  (11_0, 10_1, 10_0)
FeatureLevels   = 3
SDKVersion      = 7   (D3D11_SDK_VERSION)
ppDevice / pFeatureLevel / ppImmediateContext → mD3DDevice(+0x10), mFeatureLevel(+0x1c),
                                                 mD3DDeviceContext(+0x14)
```

On `FAILED(hr)` it asserts (`d3d11context.cpp`, `"Could not initialize DirectX! Please make sure
your system supports DirectX 10 or higher!"`, guarded by a once-flag at `0x1010756b`, `int3`) and
returns `false`. There is **no WARP (5) attempt, no REFERENCE (3) attempt, no software rasteriser
path**. The only "software" strings in the binary are
`"D3D11 does not support software …"` and `"Why are we calling a software sp…"` — both negative.
`_CLSID_NullRenderer` is a Media Foundation symbol for video playback, not a render path.
**[measured]**

`GraphSys` (`d3dgl.pdb` type `0x4DD8`, 888 bytes base) has **142 virtual slots** (`vft+0` …
`vft+564`), before counting the `GSGraphicCard` / `GSBuffer` / `GSSprite` / `GSTexture` /
`GSFont` / `GSVertStream` sub-objects it must vend. **[measured]**

### The three usable options, in order

1. **Make WARP the adapter, not the code path.** With `pAdapter = NULL` and
   `D3D_DRIVER_TYPE_HARDWARE`, D3D11 enumerates whatever DXGI presents as adapter 0. Windows
   presents the *Microsoft Basic Render Driver* (WARP) as an adapter on machines with no GPU, and
   feature level 11_0 is satisfied by it. **This costs zero code if it works.** **[reported —
   standard Windows behaviour, NOT verified here]**. Test it by removing/disabling the Parallels
   display adapter and launching; the assert string above is the exact failure signature to watch
   for. This is also the likely fix for the "lost GPU device during a VM update" crash — see
   `GraphSys::device_lost_reminder` (+872) and `D3D11GLGraphicCard::reset(int)`, which exist but
   whose trigger conditions are not established.
2. **Substitute the graphsys DLL.** The graphsys loader (`FUN_0054abf0`) takes the DLL base name
   as a `String` and consults `rise.ini` / `RISE_OF_NATIONS` first (§2). If that turns out to be a
   name override, a stub DLL exporting `get_graphsy_object_ptr` and returning a null `GraphSys`
   makes the game headless with **no** patching. **142 virtuals is a real but bounded job.**
   **Establishing whether the ini/registry key is a DLL-name override is the single highest-value
   next experiment on this track.** **[inferred]**
3. **DLL shim.** Drop a `dxgi.dll`/`d3d11.dll` proxy next to the exe that forces
   `DriverType = D3D_DRIVER_TYPE_WARP`. Crude, but a five-line change and it needs no game
   knowledge. **[inferred]**

Note that headless *rendering* is not the same as headless *simulation*: `SGLSys` (the `GraphSys`
subclass `d3dgl` actually instantiates) holds an `HWND`, `HDC`, and a `MessageHandler` base, so
the game still wants a window and a message pump even with a null renderer. **[measured]**

---

## 7. Loose ends worth someone's time

- **`rise_z.map`** (13.5 MB, now at `ron-bin/sbl/`) — a full linker map for the exe. Nobody has
  looked at it. It should collapse a lot of `FUN_xxxxxxxx` naming work in one pass.
- **`QlocLobbyPropertyData` / `QlocLobbyMemberData` static string constants** in
  `CrossplayProxy.dll` `.rdata` — these are the lobby attribute keys, i.e. the exact schema for
  publishing/reading a joinable session. Cheap to read, directly unblocks Option B step 2.
- **The `rise.ini` / `RISE_OF_NATIONS` lookup at `0x54ac60`** — decides whether Option 2 of §6
  works. Half an hour of Ghidra.
- **`MemMgr` vtable at `PTR_vftable_00c8ccf0`** — needed by any replacement subsystem DLL
  (Option A of §5 and Option 2 of §6 both depend on it).
- **`ConnectionData::send_game` / `send_player`** — the last unmapped piece of the join handshake
  is the exact serialisation of `GameConnectionDataFull` onto the wire (`wstring` members inside
  `PlayerConnectionData` cannot be memcpy'd, so there is a marshaller somewhere).
- **`NetSysFifo` / `NetFifo` link structs** (`SysFifoLink`, `FifoLink`) were not dumped; they hold
  the buffered packet plus sender, and would pin down `NetSys::get`'s exact contract.
