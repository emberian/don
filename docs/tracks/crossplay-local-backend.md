# A DoN-owned `ICrossPlayService`: lobbies without PlayFab

**Track owner:** crossplay local-backend lane. **Scope:** an implementation of
the shipped 58-slot `CrossplayProxy::ICrossPlayService` interface that serves
DoN's own lobbies. No PlayFab, no accounts, no WinHTTP, no Party network, no
TURN. **No DLL was built, no image installed, and no live process touched.**

Companion tracks: [`crossplay-abi.md`](crossplay-abi.md) (the interface, which
this lane consumes and does not re-derive) and
[`netcode-symbols.md`](netcode-symbols.md). Companion crate:
[`crates/don-crossplay`](../../crates/don-crossplay/README.md).

**[measured]** means read out of a shipped PDB or off shipped machine code in
this session, with the address given. **[DoN policy]** means DoN chose it,
because it is DoN's service and nothing about it is a claim regarding Q-LOC's.
**[inferred]** means it follows from a measurement but was not itself observed.

---

## 1. The framing that keeps this honest

`crossplay-abi.md` established the *interface* by measurement: 58 slots, full
signatures, 275 compile-time assertions, and the two-`ICrossPlayService` trap
recorded in checked code. That is retail's, and this lane changes none of it.

What this lane adds is *behaviour* — and behaviour is where a fidelity project
can quietly start inventing. So the rule here is explicit and inverted:

> The ABI is retail's and is measured. **The semantics are DoN's and are
> policy.** Where a shipped behaviour was measured it is reproduced and cited;
> everywhere else, the code says `[DoN policy]` and means it.

This is not a reimplementation of PlayFab. It is a different service that
presents the same binary interface, which is a much smaller and much more
defensible claim.

---

## 2. What was measured for this lane

Everything below is new in this session, from
`ron-bin/sbl/CrossplayProxy.pdb` (GUID `{d36f6bf3-9aa0-4388-895e-f3e8f776e191}`)
and `ron-bin/dll/CrossplayProxy.dll` (image base `0x10000000`).

### 2.1 The session state machine

| RVA | instruction | meaning |
|---|---|---|
| `0x146ba` | `mov [edi+0x930], 1` | `StartSession` sets `Starting` **synchronously** |
| `0x1492b` | `mov [eax+0x930], 0` | its completion lambda sets `Stopped` on failure |
| `0x14b51` | `mov [eax+0x930], 2` | …and `Started` on success |
| `0x14d73` | `test eax,eax; je` | `StopSession` returns at once when already `Stopped` |
| `0x14d7b` | `cmp eax,3; je` | …or already `Stopping` |
| `0x14d84` | `mov [esi+0x930], 3` | otherwise `Stopping` |
| `0x14f3e` | `mov [esi+0x930], 0` | then `Stopped`, in the same call |

**[measured]** A sweep of the whole `.text` section for `mov dword ptr
[reg+0x930], …` finds exactly these five and no others; a read-modify-write in
some other mnemonic would not have been caught, but none is plausible for a
four-value enum field. `crates/don-crossplay/src/local.rs` reproduces this
machine, and a test asserts each transition.

`GetSessionStatus` gates 17 distinct exe functions, so getting this wrong is not
a cosmetic error — it is the difference between the game proceeding and the game
sitting in a menu.

### 2.2 `std::_Func_base`'s vtable, and therefore how to call a callback

Reading a `std::function` the game hands us is not enough; a lobby service has
to *call* one. Resolving all 20 `??_7?$_Func_impl_no_alloc@…@std@@6B@` tables in
`.rdata` against the PDB's function names gives, **20 out of 20 identical**:

| slot | method |
|---|---|
| 0 | `_Copy` |
| 1 | `_Move` |
| 2 | **`_Do_call`** |
| 3 | `_Target_type` |
| 4 | `_Delete_this` |
| 5 | `_Get` |
| 6 | already outside `.text` — the table is six pointers |

**[measured]** Slots 0/1 resolve to one folded address in every table, so their
*order* is not distinguished by this measurement — but 2, 3, 4 and 5 are, and
they match the declaration order, which is what makes 0/1 `_Copy`/`_Move`.
`crates/netsys-shim` independently uses 0 = copy, 1 = move, 4 = delete; slot 4
is corroborated here by `~CrossPlayService`, which destroys the retained lobby
callbacks with `call dword ptr [edx+0x10]` at RVA `0x14435`.

The stack cleanup confirms the argument shape: `_Do_call` for
`void(const LeaveLobbyDTO&, bool)` is a `ret 8` body, and for
`void(std::wstring)` — a **by-value** parameter — it is `ret 4`, i.e. one
pointer, because `_Do_call(_Types&&...)` passes an rvalue reference.
**[measured]**

### 2.3 `unordered_map<wstring, wstring>` — the layout, and the lookup

This is the piece without which the interface is unusable: lobby attributes are
*only* reachable through the map parameter of `CreateLobby`/`UpdateLobby`
(there is no `SetLobbyAttribute` in this build — `crossplay-abi.md` §3.6), and
`game_seed` travels in it.

From the PDB, **[measured]**:

```text
std::unordered_map<wstring,wstring> == std::_Hash<...>       32 bytes
  +0   _Traitsobj                                              4
  +4   _List    (std::list)  { +4 _Myhead, +8 _Mysize }        8
  +12  _Vec     (std::_Hash_vec) { +12 _Myfirst, +16 _Mylast, +20 _Myend }  12
  +24  _Mask                                                   4
  +28  _Maxidx                                                 4

std::_List_node<pair<const wstring, wstring>>                56 bytes
  +0 _Next   +4 _Prev   +8 key (wstring)   +32 value (wstring)

std::basic_string<wchar_t>                                   24 bytes
  +0 _Bx { wchar_t _Buf[8] | wchar_t* _Ptr }   +16 _Mysize   +20 _Myres
```

Confirmed a second time by the shipped code: the `unordered_map` destructor at
RVA `0x3c90` `operator delete`s `[esi+0xc]`, zeroes `+0xc/+0x10/+0x14`, and
hands `[esi+4]` to the list destructor.

Then `std::_Hash<_Umap_traits<wstring,wstring,…>>::_Find_last` at RVA `0x3580`,
disassembled in full — the single lookup primitive under `find`, `at`, `count`,
`operator[]` and `equal_range`:

```text
0x3584  mov edx,[ecx+0x18]        ; _Mask
0x3587  and edx,[ebp+0x10]        ; & hashval  -> bucket
0x358a  mov eax,[ecx+0x0c]        ; _Vec._Myfirst
0x358d  mov ecx,[ecx+0x04]        ; _List._Myhead
0x3590  lea eax,[eax+edx*8]       ; TWO POINTERS PER BUCKET
0x3594  mov ebx,[eax+4]           ; _End(bucket)
0x3597  cmp ebx,ecx / jne         ; == _Myhead  ->  not found
0x35ae  mov eax,[eax]             ; _Begin(bucket)
0x35c0  cmp [ebx+0x1c],8 / jb     ; key._Myres < 8  ->  inline _Buf
0x35e4  movzx / cmp / jne         ; wchar-by-wchar key compare
0x3616  mov ebx,[ebx+4]           ; walk _Prev
0x3611  cmp ebx,[ebp-4] / je      ; stop at _Begin(bucket)
```

**[measured]** Two consequences, and the second is the whole reason this lane
could finish:

1. The small-string rule is `_Myres < 8` selects the inline buffer. That is the
   *shipped code's* own test, not a reading of the MSVC headers.
2. **With `_Mask == 0` the bucket index is `0` for every hash value.** So a map
   built with a single bucket is looked up by a linear scan of its whole list and
   resolves every key correctly *whatever* `std::hash<wstring>` computes.

That is what `crates/don-crossplay/src/msvc.rs` builds. No hash function was
derived, none is needed, and nothing was guessed in its place. `bucket_count()`
on such a map reads 1; nothing in the shipped interface exposes it to the game.
`_Traitsobj` is written as zero and documented as unobserved — `_Find_last`
reads only `+4`, `+0xc` and `+0x18`.

### 2.4 The three lobby broadcast callbacks are unobserved

`SetJoinLobbyCallback`(16), `SetLeaveLobbyCallback`(18) and
`SetUpdateLobbyCallback`(20) each take
`std::function<void(const DTO&, bool)>`, and **nothing establishes what that
`bool` means.** Measured:

* the setters at RVAs `0x17fd0` and `0x183a0` are three-instruction tail jumps
  that store into `this+0x120` and `this+0x170`;
* the only readers of `[this+0x144]` and `[this+0x194]` — those fields'
  `std::function` target pointers — are in `~CrossPlayService`, which destroys
  them. A full `.text` sweep finds **no invocation** in the DLL;
* `gen/slot_usage.py` finds **no dispatch of the installers** from
  `riseofnations.exe` either.

So DoN retains those three callbacks and **never calls them**, because calling
them means inventing that `bool`. Membership and attribute changes reach the
game through the polling path it *is* measured to use:
`LobbyManager::refresh_lobbies` → `FindLobbies`, and `GetLobby`. A test asserts
the three stay uninvoked, so the decision cannot rot silently.

---

## 3. What was built

`crates/don-crossplay` gains three modules behind a default-on `local` feature;
`--no-default-features` leaves the crate the pure interface description it was.

| module | contents | `unsafe` |
|---|---|---|
| `local.rs` | sessions, lobby directory, attributes, P2P routing. Pure data. | none |
| `msvc.rs` | the object boundary: reading and building `wstring`, `vector<T>`, `unordered_map<wstring,wstring>`, `LobbyDTO`, `LobbySearchResultDTO` | the raw-memory impl only |
| `service.rs` | the 58-slot vtable and the per-peer `ICrossplayPlayer` | pointer recovery and `_Do_call` |

### 3.1 Slot coverage

All 58 slots are assigned exactly one category, and they sum to 58.

| behaviour | n | slots |
|---|---|---|
| **real** | 28 | 0 `Init`, 2 `SetServiceErrorCallback`, 4 `StartSession`, 5 `StopSession`, 7 `GetSessionStatus`, 8 `GetCrossplayStatus`, 12 `GetLobby`, 13 `FindLobbies`, 14 `CreateLobby`, 15 `JoinLobby`, 17 `LeaveLobby`, 19 `UpdateLobby`, 21 `LobbyCancelPendingRequests`, 22 `StartGame`, 23 `CancelGameStart`, 33 `GetInvitationId`, 40–43 the four P2P connection/channel callbacks, 45 `SetReceivedP2PDataCallback`, 49 `P2PSendToAll`, 50 `P2PSend`, 51 `P2PCloseAll`, 53 `IsConnectedToHub`, 55 `SetUsername`, 56 `GetPlayerGuid`, 57 `Tick` |
| **retained callback, never invoked** | 11 | 6 `SetRenewTokenCallback`, 16/18/20 the lobby broadcast trio, 25 `SetLobbyMessageReceivedCallback`, 26 `SetCrossplayEnabled`, 35/37/39 the chat callbacks, 44 `SetReceivedP2PTextCallback`, 46 `SetP2PConnectionFailedCallback` |
| **retained scalar** | 2 | 3 `SetReliability`, 47 `SetP2PTimeoutDuration` |
| **reproduced inert** (the shipped body is a no-op or a constant) | 7 | 1 `SetServiceUrl`, 9 `BlockUser`, 10 `UnblockUser`, 11 `IsUserBlocked`, 48 `P2PStartConnection`, 52 `P2PClose`, 54 `CreateLocalPlayerLoopback` |
| **fail closed** | 10 | 24 `SendLobbyChat`, 27 `SetStats`, 28 `GetStats`, 29/30 leaderboards, 31/32 invitations, 34 `JoinChat`, 36 `LeaveChat`, 38 `SendChatMessage` |

Two entries deserve their footnotes. Slot 44
`SetReceivedP2PTextCallback` is retained and its delivery path is written, but
nothing in the directory produces a text payload — retail's text channel
(`CrossPlayService::SendP2PTextDataToAll`) is not modelled, and DoN's turn
traffic is binary. Slots 3 and 47 store a value the local transport has nothing
to apply it to; slot 3's shipped body is one of the folded no-ops, so retaining
it is already more than retail did.

A refusal is not a silent no-op. It queues a completion that runs the *caller's
own error callback* with `error::NOT_AVAILABLE` and a message naming the slot —
the `LIBERR_NOT_AVAILABLE` discipline `crates/netsys-shim` uses, transposed to a
callback interface. A game waiting on `GetStats` gets an answer.

### 3.2 Everything completes on `Tick`

Every lobby entry point takes an ok/error callback pair instead of returning a
result, and `Tick` is pumped from `NetDaemon::process_all` **[measured]**. The
game is written against an asynchronous service, so requests are queued and
drained one per `Tick` in submission order. Answering synchronously inside the
call would let the game depend on re-entrancy retail never offered.

### 3.3 The DoN policy decisions, listed

Each of these is a choice, not a finding, and each is marked in the source:

* `_availableSlots` = `max_members - bot_count - members`, floored at zero, and
  the same expression filters `LobbySearchCriteriaDTO::_minAvailableSlots`.
* Error codes are DoN's own negative integers. Nothing was measured about what
  the shipped service produced or whether the exe branches on specific values.
* Empty attribute values are **deletions**, which is what makes the map usable
  as the delta publish `ConnectionData::send_player` performs.
* Owner migration on leave goes to the next member; an emptied lobby is deleted.
* `ELobbyProximity` is accepted and ignored — DoN has no geography for a peer.
* `_turnServer` is three empty strings; DoN peers address each other directly.
* `StartGame` publishes the lobby id as the session reference.
* A peer's connection and its data channel open back to back, because a local
  directory has no separate negotiation phase.
* `GetCrossplayStatus` is unconditionally `Enabled`; DoN's directory has no
  platform distinction to gate on.

And two **[inferred]** readings, flagged as such at the call site:

* `UpdateLobby(const wstring&, int, const map&, int, int, …)` — the PDB names
  none of the three `int`s. DoN reads them as max members, bot count, and one
  ignored, by analogy with `CreateLobby(int maxMembers, …)` and
  `LobbyDTO::_botCount`.
* `StartGame`'s unnamed `bool` is accepted and not acted on.

---

## 4. Gates

```sh
tools/swarm-cargo crossplay-local test --manifest-path crates/don-crossplay/Cargo.toml --lib
# 42 passed; 0 failed

cd crates/don-crossplay && cargo check --lib --target i686-pc-windows-msvc
cd crates/don-crossplay && cargo check --lib --no-default-features --target i686-pc-windows-msvc
# both clean — the shipped ABI is x86 and `extern "thiscall"` only exists there
```

The vtable is emitted with `extern "thiscall"` on x86 and `extern "C"`
elsewhere, exactly as `abi.rs` does, so the host tests call all 58 slots through
a real function-pointer table. Guest memory is abstracted (`GuestMem`): the CRT
heap on i686, a byte arena with a synthetic 32-bit base on the arm64 host. Both
run the same layout code — the pointer arithmetic that ships is the pointer
arithmetic the tests execute.

### 4.1 The assertions are not vacuous

Mutation-tested by hand, each reverted immediately after:

| mutation | result |
|---|---|
| `_Mask` `0` → `7` (claim eight buckets, allocate two entries) | **fails** 1 test |
| swap the bucket pair's `{first, last}` | **fails** 1 test |
| move `LobbyDTO::_attributes` from `+104` to `+100` | **fails** 4 tests |
| move the `wstring` small-string threshold from 7 to 8 units | **fails** 11 tests |
| shift the `LobbyDTO` inside `UpdateLobbyResultDTO` from `+4` to `+8` | **fails** 1 test |
| bucket stride `bucket * 8` → `bucket * 4` in the lookup transcription | **passes** |

The last row is the honest one. With `_Mask == 0` the bucket index is always
zero, so no test can distinguish a stride of 8 from any other; the `edx*8` in
the disassembly is the only evidence for it. Nothing in the code depends on the
stride either — `read_attributes` walks the intrusive list and never reads a
bucket — so the single unpinned constant is also the one with no reachable
consequence.

---

## 5. Fidelity tier, stated plainly

**Tier C, with a boundary at the guest call.**

* The *layouts* are Tier C evidence about structure: PDB transcriptions
  cross-checked against shipped machine code, executed only by DoN's own reader.
* The *lookup check* runs a hand transcription of `_Find_last` over a map this
  crate built. It proves the builder agrees with **this reading of the
  disassembly**. It cannot fail if the transcription and the builder share a
  misreading, and it is not the shipped code executing.
* The *behaviour* is DoN's own and is unit-tested as such. It is not a claim
  about Q-LOC's service, and no test in this lane observed the shipped game.

Nothing here is verified or proven. No slot has been called by
`riseofnations.exe`.

---

## 6. Open, in priority order

1. **`std::function` ownership.** Retaining a callback copies the caller's 40
   bytes and calls neither `_Copy` (slot 0) nor `_Delete_this` (slot 4). That is
   correct for a target living in the object's own inline buffer — the common
   small-lambda case, where `target == &self` — and **not correct in general**
   for a by-value `std::function` with a heap `_Func_impl`. `netsys-shim`'s
   `replace_msvc_function` is the worked pattern. Closing it is one call to each
   of two measured slots, and is left until something can execute guest code.
2. **Nobody has observed a live call.** The load-only run described in
   `crossplay-abi.md` §5 — a replacement that logs first use per slot, as
   `netsys-shim`'s `DON_NET_LOAD_ONLY=1` did for `NetSys` — would settle both
   the slot inventory and the item above. It needs separate authorisation.
3. **The `bool` on the three broadcast callbacks** (§2.4). Unobserved in both
   images. Until it is derived, the callbacks stay retained and uninvoked.
4. **Cross-process directory.** `Directory` is a set of pure state transitions
   over `&mut self`, which is the seam a transport replaces; today two peers
   share one only inside a single address space. `crates/don-net`'s TCP session
   is the obvious carrier, and `netsys-shim` is the precedent for wiring one in.
5. **`QlocLobbyPropertyData`'s key values** are still unread — the static consts
   `PLAYFAB_SEARCH_KEY_PREFIX`, `SEARCH_DATA_VALUE_SEPARATOR`,
   `SEARCH_DATA_PAIR_SEPARATOR`, `QLOC_PROPERTY_KEYS_MAP`. They matter only for
   interoperating with the real service, which a DoN-owned directory does not
   do; recorded so the omission is deliberate.
6. **The exported `Crossplay::Service()` entry point** is not implemented here.
   A replacement DLL must export
   `?Service@Crossplay@@YAPAUICrossPlayService@1@XZ` at ordinal 2 and
   `?Logger@Logging@Crossplay@@YAPAVICrossplayLogger@12@XZ` at ordinal 1
   (`crates/don-crossplay/src/lib.rs::SHIPPED_EXPORTS`), plus the four-slot
   `ICrossplayLogger`. This lane deliberately built no DLL.

---

## 7. Amendment from the crossplay-dll lane (wave 3)

Appended, not rewritten — the sections above are the backend lane's record and
stay as they were written.

**§6 item 1 (`std::function` ownership) is closed.**
`crates/don-crossplay/src/func.rs` now runs the measured `_Copy`, `_Move` and
`_Delete_this`, and `OwnedFunction` keeps every retained callback at a fixed
address, because an inline target is a pointer into the object's own buffer and
a Rust move of the 40 bytes invalidates it. The by-reference setters use
`retain`, the by-value completion pairs use `adopt`, and both directions were
executed against synthetic `_Func_impl` targets in a PE32 process. The
diagnosis in §6 was slightly optimistic in one respect: the byte copy was not
merely wrong "for a heap `_Func_impl`" — because `Pending` holds the pair until
a later `Tick`, an *inline* target left a pointer into a stack frame that no
longer existed, which is a use-after-free rather than a leak.

**§6 item 6 is closed.** `crates/don-crossplay/dll/` emits a PE32/i386
`CrossplayProxy.dll` with all four shipped names at their shipped ordinals, and
`crates/don-crossplay/src/logger.rs` is the four-slot `ICrossplayLogger` behind
ordinal 1.

**§6 item 2 is unchanged.** Nothing has been called by `riseofnations.exe`, and
nothing has been installed into a game directory. What now exists is a
disposable PE32 loader that executes the image outside the game.

See [`crossplay-proxy-dll.md`](crossplay-proxy-dll.md), including §2 — a
`#[repr(C, align(8))]` on `MsvcFunction` was silently changing every by-value
slot on this interface into a different calling convention, and no compile-time
assertion in `abi.rs` could see it.
