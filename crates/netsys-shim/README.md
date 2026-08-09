# netsys-shim — a replacement `CrossplayNetLib.dll`

`riseofnations.exe` does not import its network stack. `NetSys::load_dll`
(`0x00538490`) `LoadLibraryW`s the DLL, `GetProcAddress`es
`get_netsys_object_ptr`, and drives everything else through the returned
object's **65-slot `NetSys` vtable** plus **nine `CrossplayNetLibSys` methods and
two free functions** resolved through the ordinary import table.

Provide those twelve symbols and the retail binary talks to us instead of to
PlayFab, **with no patching of the game**.

## Build

```sh
cd /Users/ember/dev/don/crates/netsys-shim
XWIN_ARCH=x86 cargo xwin build --release
# -> target/i686-pc-windows-msvc/release/CrossplayNetLib.dll   (PE32 i386 DLL)

uv run --with pefile --with capstone python check-exports.py
# -> name + ordinal parity, PE32/i386/DLL identity, callback ret 0x78

# Compile focused unit tests for the retail target. They cannot execute on the
# arm64 host; the layout assertions also run during the DLL build above.
XWIN_ARCH=x86 cargo xwin test --no-run
```

`XWIN_ARCH=x86` is required the first time: `cargo-xwin`'s cache is per
architecture and a tree that has only built `donscan` has `aarch64` and `x86_64`
splatted, not `x86`. Without it the link fails with `could not open
'kernel32.lib'`.

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
| vtable is 65 slots / 260 bytes and every slot has its PDB byte offset; `NetPlayer` is 21 exact slots; `NetSysBase` is 88 bytes | compile-time assertions in `abi.rs` |
| `NetPlayer::{get_id,get_platform_id,get_platform}` return a complete MSVC `wstring` by value | shipped `get_id` `0x10027220`: return object `size=0` at `+0x10`, `capacity=7` at `+0x14`, NUL at `+0`; focused cross-target tests |
| receive copies cannot exceed the retail destination | `rise.pdb` `NetDaemon::data` type `0x8912`: `unsigned char[2048]` at `+8`; caller `0x00950F30`; shipped copier `0x10013550` |
| three by-value callbacks are consumed and destroyed with the shipped ABI | PDB size 40 each; shipped callee `0x10017420..0x100175a8`; emitted shim disassembly returns with `ret 0x78` |
| the session/transport underneath works between two processes | `don-net`'s `tcp_session` test and the `donnet-peer` binary |
| **the retail game loads this DLL and reaches a match** | **untested.** Needs the Parallels VM. |

The last row is the honest gap. Nothing here has been run inside
`riseofnations.exe`. The three previously stated ABI risks are now resolved
from the shipped PDB/caller/callee instructions, but that is not live-load
evidence. The next exercise is load-only, with `DON_NET_LOAD_ONLY=1`, and must
inspect the flushed trace before enabling transport.

The architectural blocker remains: game setup and readiness do **not** flow
through `NetSys` in this build; they are PlayFab lobby attributes driven by
`MultiplayerManager` (see `don_net::lobby`). Replacing this DLL alone gets the
turn channel under our control but leaves lobby/setup unserved. Getting two
retail instances into a match still requires the lobby path.

The Rust cdylib exposes ten `shim_*` alias targets in addition to the exact
shipped name/ordinal surface. They are not imported by retail. The parity gate
reports them explicitly instead of pretending the export table is byte-for-byte
identical.
