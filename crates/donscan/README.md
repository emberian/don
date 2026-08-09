# donscan

Native, read-only Windows probes for `riseofnations.exe`:

- `donscan` types the heap by vtable pointer for occasional diagnostics.
- `donfeed` emits a coherent, least-data economy observation stream for RoNtoy.

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
# -> target/aarch64-pc-windows-msvc/release/{donscan,donfeed}.exe
```

`cargo-xwin` downloads the MSVC import libraries into
`~/Library/Caches/cargo-xwin` (~1.1 GB) and links with Homebrew `lld-link`. No MSVC, no
mingw, no VM toolchain needed. Windows on ARM runs the aarch64 binary natively and can
still read the emulated 32-bit x86 game process — `ReadProcessMemory` /`VirtualQueryEx`
are architecture-agnostic.

Platform-independent parsing, coherence, bounds, identity, and encoding are testable
on the Mac:

```sh
cargo test --lib                # live parser fixtures + embedded vtable lookup table
```

## RoNtoy economy feed

`donfeed` defaults to one sample per second and NDJSON on stdout:

```text
donfeed --pid 5236 --base d60000 --once
donfeed --pid 5236 --hz 2 --out C:\Users\Public\don\observations.ndjson
```

It always identifies the supported game image by PE machine, entry RVA, and image size;
`--base` is only an assertion and cannot bypass that check. At attachment it also records
the process creation time and SHA-256/size of the target executable on disk, and refuses
the process unless that digest exactly matches the supported retail build. `--hz` is hard-
capped at 15; 1–2 Hz is the intended advice cadence.

Each observation is `rontoy.observation` schema version 1.0. Resource arrays explicitly
declare retail order: food, timber, wealth, knowledge, metal, oil. The capture reads eight
leader flag dwords, then exactly one uniquely selected active console/human `LeaderData`
and its 248-byte encrypted economy block. It does not read World, Objects, the heap, or
opponents' encrypted economy. Missing/ambiguous human selection and unreadable fields are
reported as an unavailable health observation, never zero-filled as facts.
`fixtures/rontoy-observation-v1.ndjson` is a redacted synthetic golden for host adapters;
`cargo run --example make_fixture` verifies it against the real encoder before printing.

The observation labels `single_player` versus `multiplayer` from the guarded
`Game::semaphore` network-command-stream bit used by the engine's multiplayer packet
scrambling and checksum paths. Pause state comes from the exact bit returned by the
engine's 13-byte `ScenarioFuncSet::is_paused` implementation; it remains `null` only if
`GameAccess::turn_control` or that bit is unreadable.

The game frame/pointer, mode evidence, TurnControl/pause evidence, selected leader
identity/flags, and encrypted-data pointer are read before and after the observation. A
torn capture is retried at most three times. The target is never suspended: both binaries
open it with query/read rights only and import no process-write, remote-thread, or suspend
API.

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
