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

uv run --with pefile python check-exports.py    # export-table parity gate
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
| vtable is 65 slots / 260 bytes, `NetSysBase` is 88 bytes | compile-time `const _: () = assert!(...)` in `abi.rs` |
| the session/transport underneath works between two processes | `don-net`'s `tcp_session` test and the `donnet-peer` binary |
| **the retail game loads this DLL and reaches a match** | **untested.** Needs the Parallels VM. |

The last row is the honest gap. Nothing here has been run inside
`riseofnations.exe`. The known risks, in the order they would bite:

1. `NetSys::get` copies into a `NetDaemon`-owned buffer whose extent we have
   not measured; we cap deliveries at 1024 bytes, above the largest real
   message (`NetMsg_SyncDirInfo`, 529), but the cap is a guess at safety, not a
   measurement of the buffer.
2. `NetPlayer::get_id` must return a `std::wstring` **by value**. We hand back
   the caller's own return slot untouched, which reads as an empty string only
   if the caller zero-initialised it. This is the most likely first crash.
3. `set_p2p_callbacks` takes three `std::function`s by value and we never run
   their destructors — a deliberate small leak, but it also means we never
   inspect them.
4. Game setup and readiness do **not** flow through `NetSys` at all in this
   build; they are PlayFab lobby attributes driven by `MultiplayerManager`
   (see `don_net::lobby`). Replacing this DLL alone therefore gets the *turn
   channel* under our control but leaves lobby/setup unserved. Getting two
   instances all the way into a match needs the lobby path stubbed too.
