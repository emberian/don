# netsys-shim — a replacement `CrossplayNetLib.dll`

`riseofnations.exe` does not import its network stack. `NetSys::load_dll`
(`0x00538490`) `LoadLibraryW`s the DLL, `GetProcAddress`es
`get_netsys_object_ptr`, and drives everything else through the returned
object's **65-slot `NetSys` vtable**, **eight `CrossplayNetLibSys` methods** and
**two free functions**. Retail imports eight methods plus
`is_connected_to_network`; it resolves the factory with `GetProcAddress`, and
the second free function is present in the shipped export surface.

Provide those 11 shipped exports and the retail binary talks to us instead of to
PlayFab, **with no patching of the game**.

## Build

```sh
cd /Users/ember/dev/don/crates/netsys-shim
XWIN_CACHE_DIR=/Users/ember/Library/Caches/cargo-xwin-x86 \
  XWIN_ARCH=x86 cargo xwin build --release
# -> target/i686-pc-windows-msvc/release/CrossplayNetLib.dll   (PE32 i386 DLL)

uv run --with pefile --with capstone python check-exports.py
# -> name + ordinal parity, PE32/i386/DLL identity, callback ret 0x78

# Execute the Windows loader and x86 __thiscall boundary in a disposable
# process. The executable and DLL are emitted beside one another.
MVK_CONFIG_LOG_LEVEL=0 WINEDEBUG=-all \
  wine target/i686-pc-windows-msvc/release/netsys-load-smoke.exe
# -> one `don.netsys-load-smoke.v4` JSON line with "status":"pass"

# Compile focused unit tests for the retail target. They cannot execute on the
# arm64 host; the layout assertions also run during the DLL build above.
XWIN_CACHE_DIR=/Users/ember/Library/Caches/cargo-xwin-x86 \
  XWIN_ARCH=x86 cargo xwin test --no-run
```

Both variables are intentional on the current build host. `cargo-xwin` splats one architecture
set per cache; the shared default cache currently contains `aarch64` and `x86_64` libraries but
not the complete x86 desktop CRT/SDK. The pinned cache contains the complete x86 set. Selecting
only `XWIN_ARCH=x86` against the mixed cache still fails closed at the linker with missing
`kernel32.lib`, `ntdll.lib`, and related imports.

### Generation-7 offline freeze (2026-08-09)

The frozen offline artifacts are
`/tmp/don-gen7-artifacts.bUTKzW/CrossplayNetLib.dll` (209,408 bytes,
SHA-256 `2fa40766c84d06cf5dd573bae912ae4339ee387e55843352041c808671e4c964`)
and `/tmp/don-gen7-artifacts.bUTKzW/netsys-load-smoke.exe` (187,904 bytes,
SHA-256 `1fc0fb5ef4833bd73695c62791db18b7be41da3627af7acf8b1e780eea95f695`).
The DLL passed the shipped 11-name/ordinal gate, PE32/i386/DLL/image-base
checks, and the measured `set_p2p_callbacks` `ret 0x78` gate. A warnings-as-errors
release build and PE32 test compilation completed on hbox; the native `don-net`
suite passed 44 unit tests plus all integration tests, and the retail controller
parser suite passed 74 tests.

Root convergence repeated the complete native suite independently in debug and release:
hbox `gen7-netsys-donnet-20260809T215705Z-40729-18486-4c5bd6428e72` and persvati
`gen7-netsys-donnet-release-20260809T215705Z-40730-21022-4c5bd6428e72`. Both exited 0
with 44 unit tests, the retail/setup transcripts, three roundtrip tests, and the real TCP
session test green. These native receipts do not substitute for the blocked PE32 smoke.

The generation-7 PE32 runtime smoke is deliberately **not recorded as a pass**.
The current macOS Wine Devel environment remained in `wineboot`/`start.exe`
before the smoke process loaded the DLL; no new trace file was created. The
disposable prefix was stopped cleanly and no Wine process remains. Treat the
v4 smoke as compiled-but-environment-blocked until it executes in a working
PE32 runtime; do not substitute older Wine evidence for that gate.

Excluded from the root workspace (see `../../Cargo.toml`), so `cargo test` at
the repo root never touches it.

## Configuration

There is no UI to hang settings off, so they come from the environment:

| variable | meaning | default |
|---|---|---|
| `DON_NET_ROLE` | `host` or `join` | `host` |
| `DON_NET_BIND` | host listen address | `0.0.0.0:31337` |
| `DON_NET_ADDR` | address to dial when joining | `127.0.0.1:31337` |
| `DON_NET_ID` | our unique id, an `i32` | process id |
| `DON_NET_NAME` | player name | `donnet` |
| `DON_NET_LOAD_ONLY` | `1` binds only `127.0.0.1:0`, refuses session/send/get operations, and enables the diagnostic log | unset |
| `DON_NET_TRACE` | explicit diagnostic log path; calls are logged once and flushed synchronously | `%TEMP%\don-netsys-shim-<pid>.log` in load-only mode |

`DON_NET_LOAD_ONLY=1` is the first-retail-load configuration. It returns a
fully formed `NetSys`, allowing the loader and multiplayer manager to exercise
the ABI, but returns measured `LIBERR_NOT_AVAILABLE` (26) from host/join and
refuses send/get. The log records the factory plus each export and each
`NetSys`/`NetPlayer` vtable slot on first use, so a menu-only run establishes
the real retail call frontier without entering a lobby or match.

## The two ABI facts that make this possible

**The factory takes two `StringTable*`, not a `MemMgr`.** This was the blocking
unknown in `docs/tracks/netcode-symbols.md` §5 Option A. Both ends settle it:

- Caller: `0x00538490` calls
  `get_netsys_object_ptr(&loc_str_array_orig, int_str_array)`. The PDB types
  those two globals (`0x00C8CCF0`, `0x00C06378`) as `StringTable` and
  `StringTable*`.
- Callee: the shipped DLL at `RVA 0x14B00` reads only `[ebp+8]`, stores it in a
  global, `operator new`s `0x3D4` bytes (`4 + sizeof(CrossplayNetLibSys)`), and
  `ret`s with no stack cleanup — so it is `__cdecl`, and **the second argument
  is never read**. A replacement may ignore both.

**Everything virtual is `__thiscall`.** `this` in `ECX`, callee cleans. Rust
spells that `extern "thiscall"` and supports it on `i686-pc-windows-msvc`. The
one exception is `log_connection_fmt` (vtable +0xBC), which is variadic; MSVC
compiles a variadic member function as `__cdecl` with `this` pushed first, so
that slot is `extern "C"` and the caller cleans the stack.

## Exporting decorated C++ names from Rust

`#[export_name = "?foo@Bar@@QAEXXZ"]` puts the symbol in the object file but
does **not** get it into a cdylib's export set on `i686-pc-windows-msvc` — all
ten came back as `undefined symbol` from `lld-link`. `build.rs` instead emits
explicit `/EXPORT:exported=internal` aliases. Note the alias target is written
*without* the x86 leading underscore, because lld-link prepends it itself;
`_shim_foo` produces a lookup for `__shim_foo` and fails.

The ten `shim_*` names are also exported as a side effect. They are harmless and
make the DLL self-describing under `dumpbin /exports`.

## Status — read this before believing anything

| claim | evidence |
|---|---|
| 11 exports match the shipped DLL exactly, PE32 i386 DLL | `check-exports.py`, PASS |
| shipped names occupy the exact ordinals 1..11; the callback export emits `ret 0x78` | `check-exports.py`, PASS |
| vtable is 65 slots / 260 bytes and every slot has its PDB byte offset; `NetPlayer` is 21 exact slots; `NetSysBase` is 88 bytes | fresh shipped-PDB extraction plus compile-time assertions in `abi.rs` |
| generation-7 DLL and v4 smoke compile warning-free for PE32/i386; all test executables link | hbox `cargo xwin build --release` and `cargo xwin test --release --no-run`, PASS; exact hashes above |
| generation-7 PE32 runtime boundary | **environment-blocked, not passed:** Wine never loaded the smoke EXE/DLL and emitted no new trace |
| callable-inert bind failure, close→init reuse reset, concrete flag/scalar getters, null-unicast refusal, callback clone/replace/destruction, heap-backed wstrings, Setup bridge order, and all non-destructor `NetPlayer` slots preserve ESP | encoded in `don.netsys-load-smoke.v4`; PE32 compile/link PASS, runtime execution remains the blocked gate above |
| Friend Game host materialization exposes coherent local/host pointers while pending, retains NetMessenger session data at `+0xB0`, defers `on_player_added` until the retail DTO ID is installed, then clears pending; repeat `OnPlayerJoined` is idempotent | shared production/smoke host lifecycle plus earlier PE32 smoke evidence; v4 revalidation pending |
| SetupWin bridge passes the remote ID as two exact by-value MSVC wstrings, resolves slot 1, writes `PlayerConnectionData[1].ready` at `59+58`, then calls `send_player(1,true)`; a pre-SetupWin remote remains pending and is retried | production bridge ABI plus earlier inert PE32 callback evidence; heap-backed v4 revalidation pending |
| `NetPlayer::{get_id,get_platform_id,get_platform}` return a complete MSVC `wstring` by value | shipped `get_id` `0x10027220`: return object `size=0` at `+0x10`, `capacity=7` at `+0x14`, NUL at `+0`; focused cross-target tests |
| receive copies cannot exceed the retail destination | `rise.pdb` `NetDaemon::data` type `0x8912`: `unsigned char[2048]` at `+8`; caller `0x00950F30`; shipped copier `0x10013550` |
| three by-value callbacks are cloned into owner-lifetime storage at concrete `+0x358/+0x380/+0x3A8`, replacements destroy the old target once, and inputs remain callee-destroyed | PDB size 40 each; shipped callee `0x10017420..0x100175a8`; emitted shim disassembly returns with `ret 0x78`; focused v4 inline-target ownership gate compiles |
| the session/transport underneath works between two processes | `don-net`'s `tcp_session` test and the `donnet-peer` binary |
| the pinned retail executable constructs the direct-access `LobbyDTO` at concrete `+0xD0` and survives the immediate post-`OnHostUpdated` copy-assignment | generation-5 PID 12080 trace records the retail constructor at exe RVA `0x4B1F0`, then `OnHostUpdated`; the process remained live where generations 3/4 faulted in `std::list::clear` |
| `get_ip_addresses` returns the shipped borrowed embedded empty `ObjectArray<String>` at concrete `+0x1A0` | PID 5056 faulted at retail `SetupWin::draw_ip_address` `0x005BD635` after the old null result; the replacement uses the pinned executable constructor at RVA `0x39E80`; the earlier PE32 smoke pinned non-null offset/layout plus ESP and v4 retains the gate |
| **the retail game reaches a match over this DLL** | **in progress.** Current live frontier is the owned Friend Game UI gate; match/turn/reconnect evidence is not yet claimed. |

The last row is the honest gap. The loader, direct lobby callback, and exact
mapped module have now run inside `riseofnations.exe`; a completed owned match
has not. The earlier vtable prototype was not load-safe:
`NetPlayer` slots from `+0x14` onward and eight `NetSys` signatures disagreed
with the shipped PDB. Those definitions are now corrected and crossed in a
disposable PE32 loader with an ESP-preservation check around each call. The
generation-7 extension is compiled but has not crossed a working Wine runtime,
and it is still not retail live-load evidence. A future exercise must first
complete the v4 disposable smoke and then, only after explicit authorization,
perform a load-only run with `DON_NET_LOAD_ONLY=1` and inspect the flushed
trace before enabling transport.

The architectural blocker remains: game setup and readiness do **not** flow
through `NetSys` in this build; they are PlayFab lobby attributes driven by
`MultiplayerManager` (see `don_net::lobby`). Replacing this DLL alone gets the
turn channel under our control but leaves lobby/setup unserved. Getting two
retail instances into a match still requires the lobby path.

## Load-only smoke executable

`netsys-load-smoke.exe` is a PE32 host which uses `LoadLibraryW` and
`GetProcAddress`, rather than linking against the replacement. It resolves all
11 shipped exports, proves a forced transport-bind failure still returns a
callable inert 65-slot object, calls `get_netsys_object_ptr(nullptr, nullptr)`,
verifies all 65 vtable entries are non-null, and calls the same two slots retail's
`NetSys::load_dll` calls immediately: `error_set_callback` (56 / `+0xE0`) and
`set_profiler` (63 / `+0xFC`). It also crosses direct decorated `__thiscall`
exports for readiness and role. It checks concrete flag-based hosting,
retail-base session fullness, shipped close/re-init scalar reset, null-unicast
refusal, and the formerly mismatched return/argument/cleanup slots. It clones,
replaces, and clears stable inline callback targets through the MSVC
`std::function` ABI; drives heap-backed (>7 UTF-16 unit) player, platform, and
Setup bridge strings; checks ESP around every ABI call; materialises the local
player; and exercises all 20 non-destructor `NetPlayer` slots including hidden
`String` and `wstring` return buffers. A successful execution emits the
`don.netsys-load-smoke.v4` JSON record embedded in
`src/bin/netsys-load-smoke.rs`.

The host sets `DON_NET_LOAD_ONLY=1` before loading. It requires host/join to
return `LIBERR_NOT_AVAILABLE` (26) and send/get to return false. Separately, it
uses the explicit load-only-only `DON_NET_CONNECTIVITY_OVERRIDE` to prove
`is_connected_to_network` reports online before and after the shipped no-op
setter, then reports offline when the override changes. Production uses the
exact shipped `InternetGetConnectedState(&flags, 0)` pre-session availability
test recovered at `0x10018550`; it is deliberately independent of NetSys roster
state. The flushed trace must prove the only listener is
`127.0.0.1:ephemeral`. The host refuses an existing trace instead of appending
ambiguous evidence. No ticket, lobby id, credential, game directory, or
running retail process is read.

This closes the Windows PE/export/factory/vtable calling boundary in a
disposable process. It still does not make the final status row above green:
only a separately approved disposable `riseofnations.exe` run can establish
the rest of retail's menu-time call frontier.

The generation-7 Rust cdylib exposes 13 `shim_*` alias/test targets in addition to the exact
shipped name/ordinal surface. They are not imported by retail. The parity gate
reports them explicitly instead of pretending the export table is byte-for-byte
identical.
