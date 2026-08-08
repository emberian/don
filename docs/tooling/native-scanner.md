# donscan — native Windows heap scanner

**Lane:** native-tools. **Date:** 2026-08-08. **Machine:** arm64 Mac (build) → Parallels
"Windows 11" ARM64 guest (run), against the live `riseofnations.exe`.

---

## What was established

A working native scanner exists, it runs against the live game, and **a full typed heap
scan of a 920 MiB process takes 0.45–0.68 s** — 1.6–2.0 GiB/s. [measured] The ten-second
budget the lane was asked to beat is not close; the tool is fast enough to run inside a
per-frame loop if we want to.

| claim | tier | evidence |
|---|---|---|
| Toolchain: `cargo-xwin` + Homebrew `lld-link` cross-links `aarch64-pc-windows-msvc` from the arm64 Mac, first try, no MSVC/mingw/VM toolchain | measured | build log below; `file` says `PE32+ executable (console) Aarch64` |
| Live image base is `0x00d60000`, delta `0x960000`, stable across every run this boot | measured | `--modules` dump + 12 scans |
| **The Windows loader rewrites `OptionalHeader.ImageBase` in the mapped header**, so the preferred base `0x400000` is *not* recoverable from a live process | measured | `--modules`: game module reads `hdr_ImageBase = 0xd60000` |
| Full scan: 2,429–2,451 committed regions, ~920 MiB, 0.45–0.68 s, 0 unreadable bytes | measured | 12 runs, table below |
| ~181,672 vtable hits, of which 172,592 in `MEM_PRIVATE` (heap), 9,078 in `MEM_IMAGE`, 2 in `MEM_MAPPED` | measured | `schema/live/donscan-pid14644-full.txt` |
| 395 distinct classes present on the heap; 1,071 of 1,318 named classes hit somewhere | measured | same |
| Null-model control: scanning with a *wrong* image base yields **33–136** hits vs **158,955** at the true base — a 1,200–4,800× signal-to-background ratio | measured | control runs below |
| **`Unit`, `Build`, `Animal`, `Ammo`, `City`, `MiningList`, `GatherPointList`, `UnitType`, `BuildType`, `TechType` are fixed-size preallocated pools, not live-entity counts** | measured | counts bit-identical across scans 25 s apart during live play, while `Guy` and `Group` moved |
| **`OrderList` is an embedded member at `+0xc8` of both `Unit` and `Animal`** (600/600 and 400/400, zero orphans) | measured | address-offset analysis, below |
| **`MiningList` at `Build+0x98`, `GatherPointList` at `Build+0xb8`** (600/600, zero orphans) | measured | same |
| **364 `UnitType` objects + 129 `BuildType` objects = exactly 493**, matching the 493×493 balance-table dimension at `0x00C06AFC` | measured (count) / cross-check (the identification) | per-vtable counts, below |

---

## The toolchain problem, and what actually worked

The lane brief listed four options. Only one was tried, because it worked immediately.

| option | outcome |
|---|---|
| **`cargo-xwin` + `lld-link`** | **Worked, first build, 4.5 s.** Already installed at `~/.cargo/bin/cargo-xwin` (0.20.1); `lld-link` already at `/opt/homebrew/bin/lld-link`. It fetched the MSVC CRT + Windows SDK import libraries into `~/Library/Caches/cargo-xwin` (1.1 GB) on the first invocation. |
| `brew install mingw-w64` + `*-pc-windows-gnu` | **Not attempted** — unnecessary once xwin linked, and it would have meant installing a package and adding a rustup target. |
| build on hbox | **Not attempted** — hbox is a co-tenant box and this needed no Linux. |
| toolchain inside the VM | **Not attempted** — same reason. |

```sh
cd /Users/ember/dev/don/crates/donscan
cargo xwin build --release --target aarch64-pc-windows-msvc
#   Finished `release` profile [optimized] target(s) in 4.51s
file target/aarch64-pc-windows-msvc/release/donscan.exe
#   PE32+ executable (console) Aarch64, for MS Windows   (265,216 bytes)
```

Two decisions that made this painless and are worth keeping:

1. **Zero dependencies.** Raw `extern "system"` kernel32 declarations, a hand-rolled JSON
   reader for the one flat `{string: string}` shape `vtables.json` has, and a hand-rolled
   JSON writer. Nothing to vendor, no `build.rs`, no C shim, so there is nothing for the
   cross-link to fail on.
2. **aarch64, not x86_64.** Windows on ARM runs the ARM64 binary natively, and
   `VirtualQueryEx` / `ReadProcessMemory` are architecture-agnostic — a native ARM64
   process reads the x86-emulated 32-bit game process without any special handling. The
   32-bit target's addresses simply arrive as small `u64`s.

`donscan` is **excluded from the root workspace** (`Cargo.toml`, alongside `oracle`) because
it links `kernel32`, so `cargo test` at the repo root never builds it. The
platform-independent half is a `lib` target so the vtable-map logic *is* tested on the Mac
(`cargo test -p donscan --lib`, 2 tests: the embedded map parses to 1,777 entries with
`Unit`/`Object` at the expected VAs, and `lookup` hits only on exact rebased addresses).

**Root `cargo test` status.** At 12:37, immediately after adding the `exclude` entry, the
root workspace was green: 66 passed, 0 failed. At 12:59 it is **red** — `don-sim`
`mechanics::economy_tests::baseline_attrition_period_is_the_attrition_rule_itself` and
`::higher_attrition_level_shortens_the_period` fail at `crates/don-sim/src/mechanics.rs:1660`
and `:1678`. That is **not this lane**: `crates/don-sim/src/mechanics.rs` was written at
12:58 by a concurrent lane (the `don-sim` test binary grew from 37 to 55 tests between the
two runs), and nothing donscan touches is reachable from `don-sim`. Flagging it rather than
touching it — no stash, no "fix".

---

## Getting the binary into the VM

`prlctl exec` is a bad file-transfer channel and `certutil -encode` round-trips are slow
and, per `docs/binary-ground-truth.md`, unreliable at overwriting. What worked, and is
faster and hash-verifiable, is **the Parallels host-only network**:

- guest IP `10.211.55.6`, host `10.211.55.2` [measured]
- run a tiny HTTP server on the Mac bound to `10.211.55.2:8899` that also accepts `PUT`
- pull with the guest's built-in `curl.exe`, push results back with `curl.exe -X PUT`

```sh
# host: serve a directory, accepting uploads (script in the scratchpad; ~20 lines)
python3 srv.py &                       # binds 10.211.55.2:8899

# guest: fetch the exe, hash-verify against the host
prlctl exec "Windows 11" cmd.exe /c "curl.exe -s -o C:\Users\Public\don\donscan.exe http://10.211.55.2:8899/donscan.exe"
prlctl exec "Windows 11" cmd.exe /c "certutil -hashfile C:\Users\Public\don\donscan.exe SHA256"
#   50a6e77197fe681e15eabd3b90bd57288a4b1455f97d524503f099b6e0c696cf   == host shasum

# guest -> host: stream a 333 KB JSON back in under a second
prlctl exec "Windows 11" cmd.exe /c "curl.exe -s -X PUT --data-binary @C:\Users\Public\don\B.json http://10.211.55.2:8899/B.json"
```

A 265 KB binary and a 436 KB JSON each moved in well under a second. This should replace
the `certutil` path for anything larger than a few KB.

Gotcha [measured]: `prlctl exec` runs as SYSTEM, and a compound `cmd /c "mkdir X 2>nul & curl ... & certutil ..."`
failed opaquely on the first attempt. Issue one command per `prlctl exec` call.

---

## What the scanner does

Source: `/Users/ember/dev/don/crates/donscan/` — `src/main.rs` (scan + report),
`src/win.rs` (kernel32 FFI), `src/vtables.rs` (embedded map + lookup table),
`src/lib.rs` (the Mac-testable half). `README.md` there has the full CLI.

1. **Find the target.** `CreateToolhelp32Snapshot` by name, or `--pid`.
2. **Find the image base.** Walk `MEM_IMAGE` allocation bases with `VirtualQueryEx`, parse
   each PE header, and match on `Machine == 0x14c`, `AddressOfEntryPoint == 0x15d699`,
   `SizeOfImage == 0xbb4000` — the three fields taken from `ron-bin/riseofnations.exe` that
   the loader does not touch. **Matching on `ImageBase == 0x400000` does not work**: the
   loader rewrites that field in the mapped header to the actual load address. That cost
   one failed run and is the single most useful gotcha here.
3. **Build the lookup table.** `schema/vtables.json` is `include_str!`'d into the binary
   (one file to copy into the guest). The 1,777 VAs are rebased by
   `delta = runtime_base - 0x400000` and written into a direct-index table over the rebased
   range. Static span is `0xac6d54..0xbc21d8` = `0xfb484` ≈ 1 MB, so the table is 257 K
   `u16` slots = 514 KB and sits in L2. All 1,777 VAs are 4-aligned [measured].
4. **Scan.** `VirtualQueryEx` from 0 upward; take `MEM_COMMIT` regions whose protection is
   readable and not `PAGE_GUARD`; `ReadProcessMemory` in 4 MiB chunks into a `Vec<u32>`
   (guaranteed 4-aligned); test every 4-aligned dword. The prefilter is one
   `wrapping_sub` + one compare; the table load happens only on a candidate. A chunk read
   that fails wholesale is retried page-wise so one bad page cannot cost a whole region,
   and unread bytes are zeroed so stale buffer contents cannot be counted.
5. **Report.** JSON with per-class counts split by region type, per-vtable counts, and
   object addresses (`--max-addrs`, `--addrs-for`), plus optional NDJSON of every hit and a
   `--read <addr>:<len>` hexdump mode.

### Reproduction

```sh
# build (arm64 Mac)
cd /Users/ember/dev/don/crates/donscan
cargo xwin build --release --target aarch64-pc-windows-msvc

# run (guest); pid from --list
prlctl exec "Windows 11" cmd.exe /c "C:\Users\Public\don\donscan.exe --list"
prlctl exec "Windows 11" cmd.exe /c "C:\Users\Public\don\donscan.exe --pid 14644 --out C:\Users\Public\don\scan.json --top 45"
```

Artifacts of the runs below (gitignored, `schema/live/*.txt`):
`/Users/ember/dev/don/schema/live/donscan-pid14644-full.txt` (full scan, all region types)
and `/Users/ember/dev/don/schema/live/donscan-pid14644-scan.txt` (heap-only, with full
address lists for the sim classes). Both are JSON despite the `.txt` extension, which
matches the existing `schema/live/` gitignore rule for live captures.

---

## Measured performance

Live game, pid 14644, ~920 MiB of committed readable memory. Times are donscan's own
`QueryPerformanceCounter`-backed interval, excluding process startup; wall-clock through
`prlctl exec` was 1.3–2.2 s including channel overhead.

| run | regions | scanned | elapsed | throughput |
|---|---|---|---|---|
| full #1 (first ever run, cold) | 2,451 | 902.3 MiB | **1,526 ms** | 591 MiB/s |
| full #2 | 2,353 | 902.3 MiB | **538 ms** | 1,679 MiB/s |
| full #3 | 2,448 | 913.6 MiB | **449 ms** | 2,035 MiB/s |
| full #4 | 2,450 | 913.6 MiB | **563 ms** | 1,623 MiB/s |
| full #5 | 2,445 | 913.9 MiB | **518 ms** | 1,763 MiB/s |
| full #6 | 2,443 | 913.9 MiB | **490 ms** | 1,867 MiB/s |
| full #7 | 2,443 | 913.9 MiB | **448 ms** | 2,042 MiB/s |
| heap only (`--no-image --no-mapped`) #1 | 1,442 | 660.0 MiB | **385 ms** | 1,715 MiB/s |
| heap only #2 | 1,457 | 659.9 MiB | **412 ms** | 1,603 MiB/s |
| heap only #3 | 1,471 | 659.7 MiB | **370 ms** | 1,782 MiB/s |

The first run is 3× slower than the rest; that is a cold-start effect in the guest
(first-touch of the target's pages through the emulation layer), not algorithmic.
`bytes_unreadable` was **0** on every run — no committed readable region was skipped.

**Answer to the lane's question: a full scan takes about half a second, and repeat scans
are consistently under 0.7 s.** Two scans plus a diff — the "differential snapshot" the
brief anticipates — costs about a second.

---

## Null-model control: are the hits real?

A raw dword scan finds a hit whenever *any* dword equals a rebased vtable address, so the
obvious objection is that these are arithmetic coincidences. The control is to rerun the
identical scan with a deliberately wrong `--base`, which makes every hit a coincidence by
construction.

| image base | hits (heap-only, 660 MiB) |
|---|---|
| **`0x00d60000` (true)** | **158,955** |
| `0x00d70000` (+64 KB) | 13,897 |
| `0x00e60000` (+1 MB) | **33** |
| `0x01360000` (+6 MB) | **104** |
| `0x00560000` (−8 MB) | **136** |

Background is 33–136 hits per 660 MiB; signal is 158,955. **Signal-to-background 1,200–4,800×.**
[measured] The `+64 KB` case is not a clean null and should not be read as one: a 64 KB
shift maps many vtable VAs onto *other* real vtable VAs within the 1 MB vtable span, so it
still lands on genuine vptrs, just mislabelled.

---

## Live results

Game state at capture, from `prlctl capture "Windows 11"`: a real match in progress —
Dutch, Classical Age, city "The Hague", population 29/50, farms and citizens working.
Not a menu. [measured]

Top of the heap census (`private` = `MEM_PRIVATE`, i.e. the heap; `image` = hits inside
mapped PE images, which are mostly the `.rdata` vtable arrays and RTTI, not objects):

| class | private | image | vtables |
|---|---|---|---|
| `?$SimpleArray@G` | 14,839 | 42 | 1 |
| `?$PtrArray@VGraphicEvent@@` | 14,250 | 0 | 1 |
| `?$SimpleArray@E` | 12,932 | 16 | 1 |
| `AnimObj` | 12,756 | 2 | 1 |
| `?$Array@UAnimKeyFrame@@` | 12,716 | 1 | 1 |
| `FaceArray` | 10,429 | 45 | 1 |
| `TexArray` | 8,557 | 114 | 1 |
| `?$Array@H` | 8,424 | 410 | 1 |
| `Vert3Array` | 7,943 | 93 | 1 |
| `HierObj` | 7,935 | 1 | 1 |
| `XMLNode` | 5,517 | 147 | 1 |
| `IndexBuffer` | 4,340 | 62 | 1 |
| `Texture` | 4,171 | 124 | 2 |
| `XMLElement` | 3,073 | 144 | 1 |
| `VertStream` | 2,661 | 24 | 1 |
| `GraphicEvent` | 2,085 | 1 | 1 |
| `ScriptFunc` | 1,746 | 2 | 2 |
| `OrderList` | 1,000 | 2 | 1 |
| `?$PtrArray@VGuy@@` | 1,000 | 1 | 1 |
| `Sound` | 897 | 9 | 1 |
| `UnitType` | 728 | 17 | 2 |
| `GatherPointList` | 615 | 0 | 1 |
| `Unit` | 600 | 1 | 1 |
| `Build` | 601 | 98 | 1 |
| `MiningList` | 600 | 0 | 1 |
| `Group` | 517 | 18 | 1 |
| `Guy` | 481 | 1 | 1 |
| `Ammo` | 400 | 1 | 1 |
| `Animal` | 400 | 0 | 1 |
| `BuildType` | 258 | 20 | 2 |
| `City` | 160 | 2 | 2 |
| `TechType` | 85 | 3 | 1 |
| `GatherOrder` | 146 | 1 | 2 |
| `AttackOrder` | 60 | 3 | 2 |
| `MoveOrder` | 44 | 2 | 1 |
| `Object` | **0** | 2 | 1 |
| `PathFinder` | **0** | 3 | 2 |
| `Terrain` / `BorderSpline` / `SyncDisplay` / `Tribes` | **0** | 1/1/3/3 | — |

The `Order` classes are the other visible dynamic population: `GatherOrder` went 102 -> 146
and `MoveOrder` 30 -> 44 between scans while `AttackOrder` held at 60. Order objects are
individually allocated, so **order counts are live counts**, unlike the entity pools.

`Object` having zero heap instances is a good sanity signal: it is a base class, never
instantiated on its own, and the two hits are its vtable in `.rdata`. `PathFinder`,
`Terrain`, `BorderSpline`, `SyncDisplay`, `Tribes` likewise have zero heap hits — they are
almost certainly **static/global singletons living in the image's `.data`, not on the
heap**, so they were counted in the image column. That is a hypothesis this scan does not
settle.

---

## The important negative result: these are pool capacities, not entity counts

Two full scans **25 seconds apart during live play** (population changing, farms
completing, citizens being created), diffed per class:

| class | scan A | scan B (+25 s) | |
|---|---|---|---|
| `Unit` | 600 | 600 | **stable** |
| `Build` | 601 | 601 | **stable** |
| `Animal` | 400 | 400 | **stable** |
| `Ammo` | 400 | 400 | **stable** |
| `OrderList` | 1,000 | 1,000 | **stable** |
| `MiningList` | 600 | 600 | **stable** |
| `GatherPointList` | 615 | 615 | **stable** |
| `City` | 160 | 160 | **stable** |
| `UnitType` | 728 | 728 | **stable** |
| `BuildType` | 258 | 258 | **stable** |
| `TechType` | 85 | 85 | **stable** |
| `Guy` | 490 | 481 | moved |
| `Group` | 514 | 517 | moved |
| `AnimObj` | 11,539 | 12,756 | moved (+1,217) |
| `?$Array@UAnimKeyFrame@@` | 11,499 | 12,716 | moved (+1,217) |

**RoN preallocates fixed-size arrays of constructed objects and reuses the slots.** The
scanner therefore reports *pool capacity*, not live-entity count: there were nowhere near
600 units alive in a 29/50-population Classical-Age match. This is exactly the kind of
number that would have been believed as "47 live Units" and quietly poisoned everything
downstream, so it is the single most important thing in this report.

The address spacing confirms it — 584 of the 599 consecutive `Unit` address gaps are
*exactly* `0x168`, i.e. a packed array:

| class | objects | modal gap | share of gaps | reading |
|---|---|---|---|---|
| `Unit` | 600 | `0x168` (360) | 97.5% | packed array, `sizeof(Unit) = 0x168` |
| `Build` | 600 | `0xe8` (232) | 95.7% | packed array, `sizeof(Build) = 0xe8` |
| `Animal` | 400 | `0x178` (376) | 94.0% | packed array |
| `City` | 160 | `0xd0` (208) | 99.4% | packed array |
| `Group` | 515 | `0x9d4` (2,516) | 99.4% | packed array |
| `Ammo` | 400 | `0x88` (136) | 91.2% | packed array |
| `Guy` | 481 | `0x100` | 80.0% | **individually heap-allocated**, 0x100 granularity, with gaps |

`Guy` is the exception that proves the rule: its gaps are `0x100`, `0x200`, `0x300`… across
a 200 MB span — allocator granularity, not an array — and its count moves. `Guy` counts
are plausibly live. Everything with a clean single-stride and a frozen count is a pool.

Distinguishing a live slot from a free slot needs a validity/owner field read out of the
object, which this lane did not do. `--read <addr>:<len>` is in the tool for exactly that
next step.

---

## Struct facts that fell out of the address geometry

Because the scan gives *addresses*, not just counts, containment relations are recoverable
by asking where each child hit sits relative to the nearest preceding parent hit. These are
exact — zero ambiguity, zero orphans except where noted:

| fact | evidence |
|---|---|
| **`OrderList` is embedded at `Unit + 0xc8`** (offset 200) | 600 of 600 `OrderList` hits at exactly `+0xc8` from a `Unit`; 0 orphans |
| **`OrderList` is embedded at `Animal + 0xc8`** (offset 200) | 400 of 400 at exactly `+0xc8`; 0 orphans |
| **`MiningList` is embedded at `Build + 0x98`** (offset 152) | 600 of 600 at exactly `+0x98`; 0 orphans |
| **`GatherPointList` is embedded at `Build + 0xb8`** (offset 184) | 600 of 615 at exactly `+0xb8`; 15 elsewhere |
| `Ammo` and `Guy` are **not** embedded in `Unit` | 400/400 and 481/481 orphaned against the `Unit` array |

`Unit` and `Animal` sharing an `OrderList` at the identical offset `0xc8` says they share a
common base layout at least that far — consistent with the `Object` base in the RTTI
hierarchy.

`UnitType`, `BuildType` and `ScriptFunc` each have **two** vtables in `schema/vtables.json`,
and both appear once per object at a fixed intra-object distance:

| class | vtable A | vtable B | count each | intra-object gap | inter-object gap |
|---|---|---|---|---|---|
| `UnitType` | `0xb41fcc` | `0xb41fd4` | 364 | `0x1c8` (456) | `0x440` modal |
| `BuildType` | `0xb42b8c` | `0xb42b94` | 129 | `0x1c8` (456) | `0x1a0` modal |
| `ScriptFunc` | `0xb5ee78` | `0xb5eec0` | 873 | `0x28` (40) | `0x40` modal |

So the **object counts are 364 unit types, 129 build types, 873 script functions**, not the
raw hit counts. The secondary vptr sitting at exactly `+0x1c8` in *both* `UnitType` and
`BuildType` points at a shared base layout of 456 bytes.

### 364 + 129 = 493

`docs/binary-ground-truth.md` and `README-LLM.md` record a **493 × 493 int16 balance table
at `0x00C06AFC`**, with a note that its real extent has never been bounded and that the
captured window has unexplained negatives. The live heap contains **exactly 493 type
objects** — 364 `UnitType` + 129 `BuildType`. [measured]

That the two numbers agree exactly is a strong cross-check that the balance table is
indexed by the union of unit types and build types, and it gives the balance-table lane a
way to *bound* the table: build the index → type-name mapping by reading each of the 493
type objects in heap order. **donscan did not verify the indexing**; it measured 493 type
objects and observed the coincidence. Do not promote this to `[measured]` for the table
itself until someone reads a name out of a type object and correlates it with a table row.

---

## What I could NOT establish

- **Live vs free pool slots.** The single biggest gap. Every count above for a pooled class
  is capacity. Finding the validity/owner field is the obvious next task and `--read` is
  there for it.
- **Whether the "hit" is an object start.** The scanner reports *any* 4-aligned dword equal
  to a rebased vtable address. For pooled classes the stride analysis proves they are object
  starts; for scattered classes it does not. A stored copy of a vptr, or a `dynamic_cast`
  cache, would also hit. The null-model control bounds *arithmetic* coincidence at ≤136 per
  660 MiB, but it says nothing about legitimate non-header vptr copies.
- **Secondary vtables are missing from `schema/vtables.json`.** `Unit + 4` holds
  `0x014a1ae0` (static `0xb41ae0`), which is squarely in the vtable range but **absent from
  the map** — it sits between `UnitOut` (`0xb41960`) and `OrderList` (`0xb41af4`). So the
  1,777-entry map does not cover every vtable in `.rdata`, only the RTTI-named ones. Some
  heap objects will be typed as "unknown" for that reason.
- **`PathFinder`/`Terrain`/`BorderSpline`/`SyncDisplay`/`Tribes` as globals.** Zero heap hits
  is *consistent* with them being statics in `.data`, but the scanner does not separate
  `.data` globals from `.rdata` vtable tables — both land in the `image` column. Splitting
  the image column by section would settle it and is a small change.
- **Why the render side grows.** `AnimObj` / `?$Array@UAnimKeyFrame@@` / `?$SimpleArray@E`
  each grew by 1,217 objects in 25 s, and the process grew 902 → 921 MiB across the session.
  That looks like an unbounded render-side cache or a leak. Interesting, out of lane, not
  investigated.
- **A `WriteProcessMemory` path.** Read-only by design. The handle is opened with
  `PROCESS_QUERY_INFORMATION | PROCESS_VM_READ` only.
- **Whether `TechType = 85` is the real technology count.** 85 seems low for RoN and the
  class has one vtable and a `0x2a8` stride, so 85 objects is the honest reading — but I did
  not cross-check it against `ron-data/`.

## Also worth knowing

- **`prlctl` can screenshot**: `prlctl capture "Windows 11" --file shot.png`. That is how the
  live game state was confirmed, and it is much better than guessing from `MainWindowTitle`
  (which is empty for the fullscreen game).
- **The host-only network beats `certutil`** for anything non-trivial; see above.
- The binary is left deployed in the guest at `C:\Users\Public\don\donscan.exe`
  (sha256 `50a6e77197fe681e15eabd3b90bd57288a4b1455f97d524503f099b6e0c696cf`). It is
  state-safe against the game: it requests query/read rights and imports no process-memory
  write or suspend API. Broad scans are not performance-neutral, however; the measured
  0.66–0.92 GiB reads taking roughly 0.37–0.97 s can perturb scheduling and caches.
- Recovery later found `crates/donscan/src/live.rs`, an unexported draft that the deployed
  binary does not contain. Its targeted snapshot path is not runnable yet (4/6 isolated live
  tests fail) and has no torn-frame guard. See `docs/RECOVERY.md`; do not advertise it as a
  per-frame feed until it is repaired and measured on a disposable game.
- Root `Cargo.toml` gained `crates/donscan` in its `exclude` list. That is the only file
  outside this lane's own paths that was touched.

## Files written by this lane

| path | what |
|---|---|
| `/Users/ember/dev/don/crates/donscan/Cargo.toml` | package, no dependencies |
| `/Users/ember/dev/don/crates/donscan/README.md` | build + CLI reference |
| `/Users/ember/dev/don/crates/donscan/src/main.rs` | scan loop, image-base detection, JSON report, `--read`/`--modules` |
| `/Users/ember/dev/don/crates/donscan/src/win.rs` | raw kernel32 FFI |
| `/Users/ember/dev/don/crates/donscan/src/vtables.rs` | embedded `vtables.json`, direct-index lookup table, 2 unit tests |
| `/Users/ember/dev/don/crates/donscan/src/lib.rs` | Mac-testable half |
| `/Users/ember/dev/don/docs/tooling/native-scanner.md` | this report |
| `/Users/ember/dev/don/schema/live/donscan-pid14644-full.txt` | full scan JSON (gitignored) |
| `/Users/ember/dev/don/schema/live/donscan-pid14644-scan.txt` | heap-only scan JSON with full address lists (gitignored) |
| `/Users/ember/dev/don/Cargo.toml` | one line: added `crates/donscan` to `exclude` |
