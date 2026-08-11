# don-crossplay — the checked ABI of `CrossplayProxy.dll`, and a DoN-owned service behind it

`CrossplayProxy.dll` is Q-LOC's PlayFab bridge: lobby lifecycle, lobby
attributes, matchmaking, chat, and the Party P2P plumbing that
`CrossplayNetLib.dll` sends turns over. `riseofnations.exe` reaches all of it
through **two** imported free functions and then virtual dispatch.

The crate is three things, in three layers:

| layer | module | what it is |
|---|---|---|
| the interface | [`src/abi.rs`](src/abi.rs) | generated from the shipped private PDBs; 58 slots, 15 DTOs, compile-time layout and alignment assertions. Retail's, and measured. |
| the implementation | [`src/local.rs`](src/local.rs), [`src/msvc.rs`](src/msvc.rs), [`src/func.rs`](src/func.rs), [`src/logger.rs`](src/logger.rs), [`src/service.rs`](src/service.rs) | a **DoN-owned** lobby/session/P2P service, and the `ICrossplayLogger` behind export ordinal 1, that present those interfaces. No PlayFab, no accounts, no WinHTTP, no Party, no TURN. |
| the opt-in match seam | [`src/match_bridge.rs`](src/match_bridge.rs) | exact Crossplay/don-net roster binding and the fail-closed StartGame → MatchStart → turn admission policy. DoN-owned, not part of the DLL or retail ABI. |
| the image | [`dll/`](dll) | a PE32/i386 `CrossplayProxy.dll` exporting the four shipped names at their shipped ordinals, plus a disposable PE32 loader that executes it |

**Nothing is installed into a game directory and no live process is touched.**
No slot here has ever been called by `riseofnations.exe`. What *has* run is
`dll/src/bin/crossplay-load-smoke.rs`, a purpose-built PE32 host, in a
disposable Wine prefix. See
[`docs/tracks/crossplay-proxy-dll.md`](../../docs/tracks/crossplay-proxy-dll.md)
for what that establishes,
[`docs/tracks/crossplay-local-backend.md`](../../docs/tracks/crossplay-local-backend.md)
for what the semantics do and do not claim, and
[`../netsys-shim`](../netsys-shim) for the precedent this image follows.

`--no-default-features` drops the `local` feature and leaves the crate the pure
interface description it started as.

## The image

[`dll/`](dll) is a separate package on purpose. A `cdylib` crate type forces a
panic handler and a global allocator into whichever crate carries it, and this
one is deliberately `no_std` and host-testable; keeping the image next door
leaves `cargo test --lib` on the development host untouched and lets the DLL
bring `std` — and therefore the CRT startup the shipped DLL also links — without
that becoming a property of the ABI crate.

```sh
cd crates/don-crossplay/dll
XWIN_CACHE_DIR=/Users/ember/Library/Caches/cargo-xwin-x86 XWIN_ARCH=x86 \
  cargo xwin build --release
# -> target/i686-pc-windows-msvc/release/CrossplayProxy.dll        (PE32 i386 DLL)
# -> target/i686-pc-windows-msvc/release/crossplay-load-smoke.exe

uv run --with pefile --with capstone python3 crates/don-crossplay/dll/check-exports.py
# name + ordinal parity, PE32/i386/DLL identity, image base, code-vs-data kind,
# and the shared-UCRT heap import

crates/don-crossplay/dll/run-wine-smoke.sh \
  crates/don-crossplay/dll/target/i686-pc-windows-msvc/release/CrossplayProxy.dll \
  crates/don-crossplay/dll/target/i686-pc-windows-msvc/release/crossplay-load-smoke.exe \
  /tmp/don-crossplay-evidence
```

Both environment variables are intentional: `cargo-xwin` splats one architecture
set per cache, and the shared default cache does not contain the complete x86
desktop CRT/SDK.

The four exports, copied verbatim from the shipped export directory:

| ord | name | kind |
|---|---|---|
| 1 | `?Logger@Logging@Crossplay@@YAPAVICrossplayLogger@12@XZ` | code, `__cdecl` |
| 2 | `?Service@Crossplay@@YAPAUICrossPlayService@1@XZ` | code, `__cdecl` |
| 3 | `AmdPowerXpressRequestHighPerformance` | **data**, `.data`, `= 1` |
| 4 | `NvOptimusEnablement` | **data**, `.data`, `= 1` |

The image also exposes one explicitly DoN-only extra,
`don_crossplay_shutdown` (ordinal 5). An explicit loader must quiesce every
interface caller, invoke it, and only then call `FreeLibrary`; it drops Service
first so the directory client closes its socket and joins its worker, then
stops and joins an owned authority and all accepted connections, before Logger
is dropped. The Wine smoke exercises exactly that configured-RPC unload path.
The statically imported game retains the image for process lifetime and never
resolves this diagnostic export.

Ordinals 3 and 4 are the hybrid-GPU hints a vendor driver finds by walking
export tables; they are `DWORD`s, not functions, and `build.rs` marks them
`,DATA`. `#[export_name]` does not reach a cdylib's export set on this target,
so `build.rs` emits explicit `/EXPORT:exported=internal,@ordinal` aliases with
the target spelled without the leading underscore — the trick
[`../netsys-shim/build.rs`](../netsys-shim/build.rs) measured.

Configuration, since there is no UI to hang it off: `DON_CROSSPLAY_USER_ID`,
`DON_CROSSPLAY_USER_NAME`, `DON_CROSSPLAY_TRACE` (a path; setting it enables a
synchronously flushed diagnostic log), and the optional DoN local-directory
endpoint:

```text
DON_CROSSPLAY_DIRECTORY=listen:127.0.0.1:PORT
DON_CROSSPLAY_DIRECTORY=connect:127.0.0.1:PORT
```

The first process owns the directory authority and the others connect to it.
Only numeric loopback addresses are accepted. When the variable is unset, the
original process-private `Directory` remains the default. A configured endpoint
failure answers requests asynchronously with an error rather than silently
falling back to private storage. `src/directory_rpc.rs` is a bounded, versioned
DoN protocol; it makes no PlayFab, Party, or retail-wire claim. One connection
is bound to the exact `Member` from its successful `StartSession`, duplicate
active ids and mismatched later frames are rejected, and stop/disconnect
releases the claim. Responses atomically select at most 1,024 mailbox notices
under the directory lock, return `more_notices`, and leave all later notices
queued. Detached send/stop results are still delivered to Tick because their
mailbox notices remain observable even though their outcome has no ABI
callback.

### The bug the runtime gate caught

`MsvcFunction` was declared `#[repr(C, align(8))]`. On `i686-pc-windows-msvc` an
aggregate aligned above 4 is passed **as a pointer**, not pushed, so the emitted
`ICrossplayLogger::Register` ended `ret 8` where the shipped one pops 44 — and
the first by-value `std::function` the game handed over would have returned into
a destroyed stack frame. Every one of the compile-time layout assertions passed,
export parity passed, and the crate's whole host suite was green. Only executing
the call found it. There are now `align_of` assertions on every by-value
aggregate, in the generator as well as the generated file. Full write-up in
[`docs/tracks/crossplay-proxy-dll.md`](../../docs/tracks/crossplay-proxy-dll.md)
§2.

## The rule that keeps the second layer honest

The ABI is retail's and is **measured**. The semantics are DoN's and are
**policy**. Where a shipped behaviour was measured — the session state machine,
the eleven inert slots, `IsConnectedToHub` — it is reproduced with its address
cited inline. Everything else says `[DoN policy]` in the source and means it.
This is not a reimplementation of PlayFab; it is a different service wearing the
same binary interface, which is a far smaller claim and a defensible one.

## The finding this lane exists to record

`CrossplayProxy.pdb` declares **two different interfaces both called
`ICrossPlayService`**, and only one of them is real:

| declaration | slots | offsets | is it the shipped vtable? |
|---|---|---|---|
| `CrossplayProxy::ICrossPlayService` | **58** | `0x000..=0x0e4` | **yes** |
| `Crossplay::ICrossPlayService` | 96 | `0x000..=0x17c` | no |

Three independent measurements agree that the 58-slot table is the one:

1. `CrossplayProxy::CrossPlayService` — the object the exported `Service()`
   returns — derives from `CrossplayProxy::ICrossPlayService` at offset 0.
2. `rise.pdb` and `CrossplayNetLib.pdb` declare *their*
   `Crossplay::ICrossPlayService` with a **slot-for-slot identical** layout:
   same 58 offsets, same names, same signatures, zero differences.
3. The pointer table the shipped DLL emits at `.rdata` RVA `0x9adc4` has
   **59 entries**: 58 that each resolve to the
   `CrossplayProxy::CrossPlayService::<declared name>` for that exact slot, then
   the class's own deleting destructor at `+0xe8`. Entry 60 is already outside
   `.text`.

The 96-slot record is a *newer vendor header* this build does not use. Its slot
0 is a destructor, slot 1 is `SetNew`, and `Init` — slot 0 in the real table —
sits at slot 3. Building an ABI from it would compile cleanly and be wrong in
every slot. It is emitted as `VendorICrossPlayServiceVtable` so the trap is
recorded in checked code rather than in prose; the extra `P2PEnableTcp`,
`SetP2PAllowedPorts` and matchmaking-ticket APIs it carries are what the vendor
SDK grew after RoN:EE shipped.

`docs/tracks/netcode-symbols.md` §3.2 already flagged this in prose. This lane
reproduces it from the PDBs independently and pins it with compile-time
assertions.

## What is in `src/abi.rs`

Everything is generated. Counts from the current run:

| | count | assertions |
|---|---|---|
| `ICrossPlayServiceVtable` | 58 slots, all with full `extern "thiscall"` signatures | slot index + x86 byte size |
| `ICrossplayLoggerVtable` | 4 slots | same |
| `ICrossplayPlayerVtable` | 14 slots (two `Send` overloads, kept distinct) | same |
| `INetworkClientVtable` | 15 slots | same |
| `VendorICrossPlayServiceVtable` | 96 slots, 3 opaque — **not the shipped ABI** | same |
| DTO structs | 15 | every field offset + total size |
| enums | 5, all 4-byte `int` | — |

279 compile-time assertions in total, four of which are the `align_of` checks
that pin the by-value calling convention (see § *The bug the runtime gate
caught*). The three opaque vendor slots are
`SetNew`, `SetDelete` and `P2PGetAllConnectedPlayers`: their by-value
`std::function` / `std::vector` parameter types are only forward-declared in
this PDB, so their sizes are not derivable here and are left explicitly absent
rather than assumed.

## Regenerating

Needs the local, gitignored `ron-bin/` (a legally owned install's binaries and
symbol store). The generator refuses to write if any check fails.

```sh
python3 crates/don-crossplay/gen/gen_abi.py

# adds a machine-code check of every derived argument size
uv run --with capstone python3 crates/don-crossplay/gen/gen_abi.py --check-ret
```

Checks the generator performs before writing:

- the 58-slot vtable is identical across `CrossplayProxy.pdb`, `rise.pdb` and
  `CrossplayNetLib.pdb`;
- slot offsets are contiguous and each holds exactly one method (a destructor
  slot may hold `~X` plus `__vecDelDtor`, which collapse to one pointer);
- every slot of the shipped `.rdata` vtable resolves to the matching
  implementation symbol, the deleting destructor follows it, and the next dword
  is outside `.text`;
- the 15 DTOs have identical size/field/base shape in every PDB that declares
  them;
- with `--check-ret`: each implementation's callee stack cleanup (`ret imm16`)
  equals the argument byte count derived from the PDB signature.
  **54 of 58 agree, 0 disagree, 4 indeterminate** (`SetJoinLobbyCallback`,
  `SetLeaveLobbyCallback`, `SetUpdateLobbyCallback`,
  `LobbyCancelPendingRequests` end in a tail jump, so a linear sweep finds no
  `ret`). This is the check that turns "the PDB says so" into "the shipped code
  agrees".

## The local backend

`src/local.rs` is the service semantics — sessions, a lobby directory,
attributes, P2P routing — as pure data with no `unsafe` and no MSVC types.
`src/msvc.rs` is the object boundary: reading the `wstring` and
`unordered_map<wstring, wstring>` retail hands over, and building the `LobbyDTO`
and `LobbySearchResultDTO` it reads back. `src/service.rs` is the 58-slot vtable
plus a per-peer `ICrossplayPlayer`.

Coverage, with every slot in exactly one bucket: **28 real, 11 retained
callbacks that are never invoked, 2 retained scalars, 7 reproduced inert, 10
failing closed** — 58. A refusal is not a silent no-op: it runs the *caller's
own* error callback with a code and a message naming the slot, which is
`netsys-shim`'s `LIBERR_NOT_AVAILABLE` discipline transposed to a callback
interface.

Three measurements this layer needed, all new and all recorded in
[`docs/tracks/crossplay-local-backend.md`](../../docs/tracks/crossplay-local-backend.md):

* the **session state machine**, from the five writes to `+0x930` that exist in
  the whole of `.text` (`StartSession` sets `Starting` synchronously at RVA
  `0x146ba`; the completion lambda sets `Started` at `0x14b51`);
* **`std::_Func_base`'s vtable** — 20 of 20 `_Func_impl_no_alloc` tables give
  slot 2 = `_Do_call`, which is how a callback gets called at all;
* **`_Find_last`** at RVA `0x3580`, which shows buckets are two pointers wide
  and — the useful part — that a map built with `_Mask == 0` resolves every key
  by linear scan *whatever* `std::hash<wstring>` computes. So no hash function
  was derived and none was invented.

The three lobby broadcast callbacks (`SetJoinLobbyCallback`,
`SetLeaveLobbyCallback`, `SetUpdateLobbyCallback`) are retained and
**deliberately never invoked**: their `bool` parameter is unobserved in both
images, and a test guards that decision.

## Gates

```sh
cd crates/don-crossplay && cargo test --lib
# 59 passed; 0 failed

cd crates/don-crossplay && cargo test --lib --features std-rpc
# 66 passed; 0 failed (includes detached delivery, paging, and identity gates)
cd crates/don-crossplay && cargo test --features std-rpc --test directory_rpc_process
# the second gate launches two OS processes and proves Create -> Find -> Join

cd crates/don-crossplay && cargo test --features local-match --test service_match_process
# two OS processes prove Create -> Find -> Join -> ready -> StartGame ->
# authoritative MatchStart -> a complete two-package turn
# service-match-peer host --seed U32 lets a bounded caller choose the exact lobby seed;
# omitting it preserves the historical process-fixture seed
# adding --relay keeps each peer alive for strict `TURN U32 HEX` stdin barriers; don-net's
# command decoder admits exactly one canonical HaltCommand (0x0c) before ServiceMatch submission

cd crates/don-crossplay && cargo check --lib --target i686-pc-windows-msvc
cd crates/don-crossplay && cargo check --lib --no-default-features --target i686-pc-windows-msvc

# the same suite compiled for the retail target and executed under Wine
cd crates/don-crossplay
XWIN_CACHE_DIR=/Users/ember/Library/Caches/cargo-xwin-x86 XWIN_ARCH=x86 \
  cargo xwin test --release --lib --features std-rpc --no-run --target i686-pc-windows-msvc
wine target/i686-pc-windows-msvc/release/deps/don_crossplay-*.exe
# 66 passed; 0 failed
```

All pass. The crate is excluded from the root workspace (see `../../Cargo.toml`)
exactly like `netsys-shim`, because `extern "thiscall"` only exists on x86.

Running the suite on **both** targets is not redundant: two defects surfaced
only on `i686-pc-windows-msvc` — the `MsvcFunction` alignment above, and an
associated-`const` vtable that promoted to two different addresses. A host-only
green is a green about layout arithmetic, not about the ABI.

The vtable is emitted `extern "thiscall"` on x86 and `extern "C"` elsewhere, so
the host tests drive all 58 slots through a real function-pointer table. Guest
memory is behind a `GuestMem` trait — the CRT heap on i686, a byte arena with a
synthetic 32-bit base on the arm64 host — so the pointer arithmetic that ships
is the pointer arithmetic the tests execute.

The layout and slot-order assertions are written in `size_of::<*const ()>()`
units, so they are meaningful on the arm64 host too; the exact byte-offset and
byte-size assertions are additionally checked under `target_arch = "x86"`.

### The assertions are not vacuous

Mutation-tested by hand, each reverted immediately afterwards:

| mutation | result |
|---|---|
| swap the `StartGame` / `CancelGameStart` fields | fails on **both** i686 and arm64: `offset_of!(ICrossPlayServiceVtable, StartGame) == 22 * PTR_SIZE` |
| swap `LobbyDTO::_botCount` / `_attributeVersion` | fails: `offset_of!(LobbyDTO, bot_count) == 80` |
| shrink `MsvcUnorderedMap32` to 28 bytes | fails in three places, including `size_of::<LobbyDTO>() == 208` |
| delete the explicit 3-byte pad in `UpdateLobbyDTO` | **passes** — Rust's own `repr(C)` padding happens to coincide there |

The last row is the honest caveat: the emitted `_padN` fields document MSVC's
alignment gaps but are not themselves what pins the layout. The per-field
`offset_of!` and the total `size_of!` assertions are.

The backend's marshalling was mutation-tested the same way:

| mutation | result |
|---|---|
| `_Mask` `0` → `7` (claim eight buckets, allocate two entries) | fails, 1 test |
| swap the bucket pair's `{first, last}` | fails, 1 test |
| move `LobbyDTO::_attributes` from `+104` to `+100` | fails, 4 tests |
| move the `wstring` small-string threshold from 7 to 8 units | fails, 11 tests |
| shift the `LobbyDTO` inside `UpdateLobbyResultDTO` from `+4` to `+8` | fails, 1 test |
| bucket stride `bucket * 8` → `bucket * 4` in the lookup transcription | **passes** |

And its caveat: with `_Mask == 0` the bucket index is always zero, so no test can
distinguish a stride of 8 from any other. The `edx*8` in the disassembly is the
only evidence for it — and nothing in the code depends on it, because the reader
walks the intrusive list and never touches a bucket.

## What this crate deliberately does not claim

- **The ABI carries no behaviour.** A signature says how to call a method, not
  what the shipped implementation did.
- **The backend's behaviour is DoN's, not retail's.** Where a shipped behaviour
  was measured it is reproduced and cited; everything else is policy and says so.
  Do not read `src/local.rs` as a description of Q-LOC's service.
- **Not verified.** PDB transcriptions cross-checked against shipped machine
  code — Tier C about layout — plus unit-tested DoN semantics. The lookup check
  runs a hand transcription of `_Find_last`, so it proves the builder agrees with
  *this reading of the disassembly*, not that the shipped code accepts the map.
- **Nothing has been executed by `riseofnations.exe`.** No slot has been called
  by the game and no image has been installed into a game directory. A PE32 DLL
  now exists and has been loaded and driven — by
  `dll/src/bin/crossplay-load-smoke.rs`, in a disposable Wine prefix, which is a
  claim about the loader, the calling convention and object lifetime, and about
  nothing else.
- **The `std::function` probes in that smoke are ours, not MSVC's.** They
  establish that this DLL calls `_Copy`, `_Move` and `_Delete_this` in the right
  order with the right `deallocate` flag; they do not establish how the shipped
  `_Func_impl` behaves.
- **No live process** was read or modified, and nothing here authorises one.
