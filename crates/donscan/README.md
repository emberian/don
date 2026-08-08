# donscan

A native Windows binary that types the live `riseofnations.exe` heap by vtable pointer.

Every C++ object begins with its vtable pointer. `schema/vtables.json` maps 1,777 RTTI
vtable VAs to class names. Scanning committed memory for those addresses, rebased by the
live ASLR delta, tells you *what kind of object* sits at every address it finds.

**Excluded from the root workspace** (see `../../Cargo.toml`): it links `kernel32` and
cannot build for `aarch64-apple-darwin`. `cargo test` at the repo root never touches it.

Full derivation, measured results and caveats: `docs/tooling/native-scanner.md`.

## Build (arm64 macOS -> Windows on ARM)

```sh
cd /Users/ember/dev/don/crates/donscan
cargo xwin build --release --target aarch64-pc-windows-msvc
# -> target/aarch64-pc-windows-msvc/release/donscan.exe   (PE32+ Aarch64 console)
```

`cargo-xwin` downloads the MSVC import libraries into
`~/Library/Caches/cargo-xwin` (~1.1 GB) and links with Homebrew `lld-link`. No MSVC, no
mingw, no VM toolchain needed. Windows on ARM runs the aarch64 binary natively and can
still read the emulated 32-bit x86 game process — `ReadProcessMemory` /`VirtualQueryEx`
are architecture-agnostic.

Platform-independent parts are testable on the Mac:

```sh
cargo test -p donscan --lib     # exercises the embedded vtable map + lookup table
```

## Use

```
donscan --list                       # find the pid
donscan --modules                    # every loaded PE in the target (base/machine/entry/size)
donscan --pid <n> --out scan.json    # full scan
donscan --pid <n> --no-image --no-mapped --out heap.json      # private (heap) regions only
donscan --pid <n> --addrs-for Unit,City --max-addrs 8 --out s.json
donscan --pid <n> --hits hits.ndjson --out scan.json          # every hit, one JSON per line
donscan --pid <n> --read 0xb76558c:96                         # hexdump target memory
donscan --pid <n> --base 0xd60000 ...                         # override image-base detection
```

## How it finds the image base

The Windows loader **rewrites `OptionalHeader.ImageBase` in the mapped header** to the
actual load address, so the preferred base `0x00400000` is *not* recoverable from a live
process. donscan instead walks `MEM_IMAGE` allocation bases with `VirtualQueryEx`, parses
each PE header, and identifies the game by the three fields the loader does not touch:
`Machine == 0x14c`, `AddressOfEntryPoint == 0x15d699`, `SizeOfImage == 0xbb4000` — all
taken from `ron-bin/riseofnations.exe`. Those constants live at the top of `src/main.rs`
and must be updated if the game binary is ever patched.

## Why it is fast

`VtMap` builds a direct-index table over the rebased vtable range (`0xac6d54..0xbc21d8`
static, ~1 MB span, 257 K u16 slots = 514 KB, fits in L2). The inner loop is one subtract,
one compare, and a table load only on a candidate. Measured 1.6-2.0 GiB/s against the live
game, ~0.5 s for a 900 MiB process.
