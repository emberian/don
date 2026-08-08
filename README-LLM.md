# README-LLM

Orientation for an agent landing on this repo cold. `README.md` is Ember's, human-written;
this file is ours. Read this first, then `docs/CHARTER.md`, then whatever your task touches.

## What this is

**Descent of Nations** — a deterministic, batch-parallel Rust reimplementation of the
*Rise of Nations: Extended Edition* simulation, derived from the binary rather than from
community documentation, bit-exact wherever we can prove it, built to become an RL
environment capable of training a strong self-play agent.

## The one rule

**Ground truth is the binary, the shipped data files, or the live process. Nothing else.**

Community documentation (RoN Heaven, the Fandom wiki, Vanshilar, MHLoppy) is a
*cross-check only* and is never the source of a value we implement. Before writing any
constant or formula, say where it came from — an address, a file and line under
`ron-data/`, or a live read. If the honest answer is "a wiki says so", **stop and go
derive it**. This is not fussiness; the whole project fails quietly if folklore gets in,
because folklore is plausible, compiles fine, and is wrong in ways tests written from the
same folklore will never catch.

Corollaries, each of which was paid for:

- **Mark every claim `[measured]` or `[reported]`.** Measured means *you* verified it here.
  Reported means you read it and have not checked. Never silently promote one to the other.
- **Capture, do not calculate.** Never hand-compute an expected value for a test. Take it
  from the binary via the oracle. (A hand-computed `-47` where retail returns `-247` is why
  this rule exists.)
- **Decompiled C is a hypothesis, never a derivation.** Ghidra silently reorders integer
  and floating-point operations on 32-bit MSVC float code — wrong in exactly the dimension
  this project cares about. Get *structure* from Ghidra; get *values* from behaviour.
- **Fidelity tiers.** A = proven over the whole input domain by SMT. B = differentially
  tested against retail, **always** with sample count and input distribution — this is
  testing, *not* verification. C = behaviourally faithful, divergence measured. We hold no
  formal semantics of Rust or x86, so nothing here is "verified" in the proof-assistant
  sense. Never write *verified*, *proven*, or *refinement* about a differential test.

## Machine topology — three machines, three jobs

| machine | is | used for |
|---|---|---|
| **this Mac** | arm64, 12 cores, 96 GB | Ghidra, capstone, the Rust workspace, orchestration. **Cannot execute 32-bit x86 at all** (Rosetta is x86-64 only). |
| **hbox** (`ssh hbox`) | x86_64 Ubuntu, 24 cores, 123 GB | The **oracle** — executes retail machine code. The only machine that can. **Co-tenant with another agent's HOL build**: always `nice -n 15 taskset -c 0-3`, never install packages, never run unbounded parallel builds. |
| **Parallels VM "Windows 11"** | ARM64 Windows, x86 emulation | The **live game**. Runtime state, memory reads, replays. Ember plays here. |

## Toolchain, with exact invocations

**Ghidra** 12.1.2 (Homebrew). Project at `re/ghidra`, program `riseofnations.exe`,
47,177 functions.

```sh
/opt/homebrew/Cellar/ghidra/12.1.2/libexec/support/analyzeHeadless \
  /Users/ember/dev/don/re/ghidra ron -process riseofnations.exe -noanalysis \
  -scriptPath /Users/ember/dev/don/re/scripts -postScript <Script>.java <args>
```

⚠ **Single-writer lock.** Do not run this concurrently with other lanes. If you need
Ghidra while others are working, `cp -r re/ghidra /tmp/gh-<yourlane>` and use the copy.

**Prefer the bulk-decompiled corpus** at `re/decomp-all/<EA>.c` — one C file per function
plus `MANIFEST.jsonl`. Grep it before decompiling anything; it is far cheaper. It skips
functions over 8192 bytes, and the manifest records every skip.

**Capstone** for disassembly — no locks, always safe:

```sh
cd /Users/ember/dev/don/ron-bin && uv run --quiet --with capstone --with pefile python - <<'EOF'
import pefile
from capstone import *
pe = pefile.PE("riseofnations.exe")
txt = [s for s in pe.sections if s.Name.rstrip(b"\x00") == b".text"][0]
base = pe.OPTIONAL_HEADER.ImageBase + txt.VirtualAddress
data = txt.get_data()
md = Cs(CS_ARCH_X86, CS_MODE_32)
for i in md.disasm(data[0x644130-base:0x644130-base+200], 0x644130):
    print(hex(i.address), i.mnemonic, i.op_str)
EOF
```

**The oracle** — calls retail functions with controlled inputs, fork-isolated so a faulting
probe reports a signal instead of killing the run:

```sh
ssh hbox
cd ~/don-oracle
nice -n 15 taskset -c 0-3 cargo build --target i686-unknown-linux-musl -q
./target/i686-unknown-linux-musl/debug/oracle selftest      # hand-written code; proves the harness
./target/i686-unknown-linux-musl/debug/oracle difftest 500000
./target/i686-unknown-linux-musl/debug/oracle vectors       # capture test expectations
./target/i686-unknown-linux-musl/debug/oracle call <va-hex> [args]
./target/i686-unknown-linux-musl/debug/oracle sweep islands.jsonl sweep.jsonl
```

Source is `crates/oracle/src/main.rs`, mirrored at `hbox:~/don-oracle/crates/oracle/src/`.
Edit local → `scp` → rebuild → run. It is **excluded from the workspace** because it cannot
build on arm64, so `cargo test` at the repo root never touches it — *a green `cargo test`
is evidence about the Rust crates only, never about a fidelity claim*.

**Re-run every Tier-B claim, from the Mac, with one command:**

```sh
tools/oracle-regress.sh              # sync → hbox, build i686, run all 12 cases, fetch JSON
tools/oracle-regress.sh --status     # age + contents of the last record; runs nothing
tools/ledger-from-measurements.py    # ledger evidence rows, generated from the measurements
```

12 registered differential cases, 16.2 M trials, ~59 s. Exit 0 only if every case *ran*
**and** agreed; 1 mismatch/crash, 2 something skipped, 3 harness failure, 4 hbox
unreachable. A case that cannot run is SKIPPED and is never green — vacuous green is the
failure mode the whole harness exists to prevent. Adding a case is data in
`crates/oracle/src/registry.rs`; last record in `schema/oracle-regression.json`; the design,
the mutation test that proves it bites, and the honest gap list are in
`docs/tooling/oracle-regression.md`.

⚠ `oracle difftest` / `oracle combat` are **superseded**: their models are copies typed into
`main.rs`, so they test the copy, not `don-sim`. Use `regress`.

**The live process.** Read memory with `OpenProcess` + `ReadProcessMemory` via P/Invoke in
guest PowerShell:

```sh
prlctl exec "Windows 11" powershell.exe -EncodedCommand <base64 of UTF-16LE script>
```

Then **write results to a guest file and stream that out** — inline base64 return times out.

**For file transfer, use the SHARED FOLDER, not certutil.** Guest `\\Mac\deos` maps to host
`/Users/ember/dev/breadstuffs`, read-write, and moves 57 MB in seconds. `prlctl exec` runs
as SYSTEM so the `Z:` drive letter is invisible, but the UNC path works. The
`certutil -encode` + `type` hop in `docs/binary-ground-truth.md` is the slow fallback; it
was used all day only because an early check of `\\Mac\Home` failed and absence was wrongly
concluded from it.

## Gotchas, each paid for in real debugging

- **`prlctl exec` runs as SYSTEM.** `%USERPROFILE%` is `C:\WINDOWS\system32\config\systemprofile`.
  User paths must be explicit: `C:\Users\ember\...`.
- **Windows randomises ASLR per *boot*, not per process.** All instances of the exe share an
  image base within a session, so addresses are stable across probes. Rebase every static VA
  by `runtime_base - 0x400000`.
- **`certutil -encode` does not reliably overwrite.** Use a fresh temp filename per file and
  **always hash-verify**. A silently duplicated binary was caught only by hashing.
- **Never scan large memory in a PowerShell loop.** It is orders of magnitude too slow. Put
  the loop in compiled C# via `Add-Type`, add a cheap range prefilter before any hash lookup,
  or use a native binary.
- **NEVER conclude absence from a narrow or truncated listing.** This cost the project a
  full day, three times over: `head -5` hid `rise.pdb`; `dir /b *.pdb` hid `rise_z.map`;
  one failed `\\Mac\Home` probe hid the working shared folder. Enumerate fully, then
  conclude.
- **Fork isolation without a timeout only handles crashes, not hangs.** A probe that never
  returns wedges the run forever and looks exactly like a shell timeout.
- **A differential test whose model is an inline copy tests the copy.** Point every case at
  the shipped function, and mutation-test the harness to prove it bites.
- **Do not `git stash`** — the tree is shared with parallel lanes. Lanes must not commit;
  the orchestrator commits. Never `git add -A` while lanes are live.
- **`schema/islands.jsonl` is not a complete function list** — Ghidra left gaps, and two
  combat-critical functions sit in them.
- **Ghidra decompiler timeouts are real** — `Constants::log_data` `0x00570170` is 63,382 bytes
  and exceeds a 600 s budget. Extract at the instruction level instead.
- **Following a rule-name string leads to the LOGGER, not the loader.** The UTF-16 rule names
  in `.rdata` are xref'd only from the `*::log_data(Log*)` functions. The real loaders
  (`Constants::init` `0x00569a90`, `UnitType::init` `0x0061ab50`) fetch names from the runtime
  `StringTable` at `[0x00C06378]` and touch no literal. This cost a day; see
  `docs/derivation/PDB-RECONCILIATION.md` §2.

## The shipped PDB — read this before doing any RE

**`ron-bin/sbl/rise.pdb` is the private PDB for our exact binary.** 57 MB, CodeView GUID
`{51D4F219-61C6-4F84-9D5B-C3361B0D291F}` age 1, identical to the exe's debug directory.
37,138 publics, 22,752 procedures with real names, code sizes and C++ signatures, plus full
type info. (Any doc claiming the game ships no PDB is stale — it sat in `sbl\` all along and
an early recon command truncated the listing that would have shown it.)

```sh
python3 tools/pdb/lookup.py 644130 a1d110       # VA -> real symbol + signature
python3 tools/pdb/lookup.py --name 'PathFinder::'   # search names
cd ron-bin && uv run --quiet --with pefile python ../tools/pdb/callers.py 57fa60
cd ron-bin && uv run --quiet --with pefile python ../tools/pdb/xref.py --str flank_bonus
/opt/homebrew/opt/llvm/bin/llvm-pdbutil dump --types ron-bin/sbl/rise.pdb   # struct layouts
```

Pre-extracted: `schema/rise-symbols.tsv` (publics), `schema/rise-procs.tsv` (procedures with
sizes — the one that resolves an arbitrary VA), `schema/pdb-types.json`,
`schema/command-structs.txt`, `re/symtab.json`, `re/docaddrs.tsv`.

**It gives names, types, sizes and line info. It does not give semantics and it raises no
fidelity tier.** Finding the right function is not the same as understanding it; values still
come from the oracle. Never write "confirmed by the PDB" about a behavioural claim.

## Established ground truth (do not re-derive)

`riseofnations.exe` sha256 `30478a44…625079`, **PE32 i386**, image base `0x00400000`, a
**2024-06-20 MSVC-14 rebuild** of the 2003 code, unpacked, RTTI intact. `patriots.exe` is
only the MFC launcher.

- **Float**: SSE binary32, not x87 (~23.7 K scalar SSE vs ~560 x87). The eight precise CRT
  imports are seven non-IEEE transcendental hazards
  (`_libm_sse2_{acos,asin,atan,cos,pow,sin,tan}_precise`) plus `_sqrt_precise`, which is
  IEEE-exact and safe. **`ObjectData::get_damage` and the road A\* contain no floating
  point at all** (not a whole-sim claim — see `docs/derivation/AUDIT.md` §3.2).
- **RNG**: LCG `s ← s·1664525 + 1013904223` [measured, in `Random::get`]. `float Random::get()`
  `0x00a39cf0`, `int Random::get(int,int)` `0x00a39d70`, `Random::reseed` `0x00a39d30` (an
  XOR-swap: installs the new seed, **returns the old one**), free `random(int,int)`
  `0x00a39d40`. 414 call sites, ≥7 streams.
  ⚠ **Corrected**: the pathfinder does **not** have its own RNG. `[0x00C06184]` is
  `GameAccess::game_random`, a static *reference* to the `Random` object `game_random` at
  `0x00E37A8C` — **the main simulation stream** (307 sites in 118 functions): map generation,
  units, animals, AI leaders, the BHS script API, and every RNG-using `PathFinder` routine.
  `0x00EB697C` is `internal_random`, a *different* stream (92 sites in 31 functions) —
  mostly `Surf`/`Scene`/`Particle`/`GraphicPieces`, **but its single largest consumer is
  `Leader::diplomacy` with 18 sites**, so do not assume it is cosmetic; treat "is
  `internal_random` sim-critical?" as open. Sound draws (`SoundGlobal::random` `0x00E85F0C`,
  `SoundType::random` `0x00E87D3C`) are cosmetic and correctly do not perturb `game_random`.
  RNG state rides its **own** lockstep channel, not the checksum:
  `CommandPackage::process_check_random(CheckRandomCommand*)` `0x00946020` +
  `CommandPackage::random_seeds` `0x00CC02C8`.
- **RULES**: `[0x00C061E4]` and `[0x00C061F0]` **alias the same object** (live-read; the PDB
  says why — they are `GameAccessConst::constantsc` and `GameAccess::constants`, the
  const/non-const reference pair to the one `Constants` singleton).
- **Damage**: `ObjectData::get_damage` `0x00644130`,
  `int __thiscall (int, int, unsigned long, int, int, int*) const`, pure integer. Attack
  stored **×10**; rescale `(D+5)/10`; armor subtracted **mid-chain** at step 22 of 31; floor
  of 1 **conditional**. The community formula is wrong in structure.
- **Pathfinding**: pure-integer 8-connected grid A\* with an ordered-tree open list.
  ⚠ **Everything we measured is `PathFinder::astar_caravan_road` `0x00685990`, the caravan/road
  search.** The general pathfinder is `PathFinder::astar_path` `0x00683770` ←
  `find_upath`/`find_wpath`/`find_tpath`, with `PathFinder::calc_cost` `0x00684E50`, and it is
  **unread**; there is also `PathFinder::astar_river` `0x00686690`. What transfers:
  pure-integer (0 FP instructions in the entire `PathFinder` class — the 20 SSE ops across
  9,063 instructions are struct moves) and the 8-way rotation tie-break. What does **not**:
  *"draws RNG once per edge relaxation"* — `calc_cost` makes **zero** RNG calls; the 192-unit
  fixed step (the general search is parameterised 48/192/768); the 3,200-node budget; and the
  eight cost constants, which are read only by `calc_road_cost`.
- **Checksum**: `CheckSums::check_all` `0x00936560`, `CommandPackage::process_check_sums`
  `0x009459d0`, `adler32` `0x00a46830`. `CheckSum`/`SaveGame`/`LoadGame` are siblings of one
  `DataWalk` interface (`walk_data(DataWalk*)` / `walk_function` / `walk_test`), so
  **sim-critical state ≡ save-game state**. Note `log_data(Log*)` is a *different* visitor
  interface and does not carry this property.
- **Rule tokenizer**: `String::fraction(int scale) const` `0x00a1d110` (we had called it
  `RString::AsScaled`) — `(_wtoi(s) * scale) / _wtoi(strchr(s,'/')+1)`, denominator 0 → 0,
  no '/' → denominator 1. `scale` is a per-field compile-time constant pushed by the caller;
  the **complete** scale universe across the 40 fraction constants is
  **{256 ×24, 192 ×11, 100 ×5}** [measured, `Constants::init` call sites].
- **Balance table**: `Balance combat_table` `0x00C12BF0`; the array is
  `Balance::final_balance_table` at **`0x00C12BF4`**, and the PDB type record says
  `short[493][493]` — 486,098 bytes, exactly as derived. The old `0x00C06AFC` figure is a
  **bias-folded base** (`0x00C12BF4 − 0x00C06AFC = 49,400 = 2 × (50 × 493 + 50)`, unit ids
  start at 50); capturing at it is 49,400 bytes early and is where the "unexplained
  negatives" came from.
- **Replays**: `.rcx` is a **plain gzip stream from offset 0** (this refutes the reported
  10-byte-header claim). Payload opens with a UTF-16 version string carrying the build date.
- **`0x00846450` is `Doober::get_num`** and it is **dead code** — correctly derived, zero
  direct callers *and* zero address-taken references in `.text`, *not* the RNG. A `Doober` is
  **terrain clutter/scenery**, not a resource pile (`Doober::draw`, `add_cover_doober`,
  `BuildType::init_doober_mask`); resource nodes are `Good`/`GoodType`.
- **The descriptor "type tag" is the name's string length**, not a type tag. That is why the
  unit-encoding hypothesis was refuted.

## Artifacts

| path | what |
|---|---|
| `schema/bindings.json` | 1,224 rule-name → struct-offset bindings across 112 `log_data` visitors (field offsets are sound; these are not loaders) |
| `schema/islands.jsonl` | every function classified by reachability (ISLAND 2,135 / DATA_ONLY 711 / SELF_CALL 42,748 / WRITES_GLOBAL 970) |
| `schema/vtables.json` | **1,777 RTTI vtable addresses → class names.** The key to typed live-heap crawling: every C++ object starts with its vtable pointer |
| `docs/derivation/rules-constants.json` | 719 constants with offsets, parsers, scales |
| `crates/don-rules/src/offsets.rs` | 1,223 generated offset constants |
| `docs/derivation/*.md` | per-subsystem derivation reports + `AUDIT*.md` |
| `docs/provenance-ledger.md` | every implemented mechanic, its source, tier, evidence |

Gitignored as copyrighted game content: `ron-bin/`, `ron-data/`, `re/ghidra/`,
`re/decomp-all/`, `schema/live/`.

## Crates

- **`don-rules`** — rule-value tokenizer + generated offsets. `String::fraction` numeric
  conversion is recovered and covered by captured retail vectors and the shipped corpus.
- **`don-pe`** — PE32 reader/mapper; applies 315,865 relocations. First half of the oracle.
- **`don-sim`** — PDB-generated SoA state, retail-ordered tick skeleton, derived mechanics,
  and many subsystem ports. Most `systems/*.rs` modules are compiled but not yet driven by
  the tick; read `GOAL.md` before calling a port implemented end to end.
- **`don-replay`** — `.rcx` command decoder, generated checksum walkers, and the replay
  validation scoreboard. This is the primary integration gate.
- **`don-net`** — command wire format, lockstep session, TCP transport, and replacement
  netcode-shim work. Our peers connect; a retail internet join is not complete.
- **`don-ai`** — shipped economic-script/runtime work plus a deterministic AI-vs-AI harness.
- **`don-env`** — PyO3/Gymnasium/PettingZoo-facing batched RL surface over partial dynamics.
- **`don-gpu`** — wgpu flow fields and batch-order prototypes with CPU/GPU parity tests.
- **`donscan`** — Windows live-process scanner; excluded from the arm64 workspace build.
- **`oracle`** — 32-bit only, excluded from the workspace, runs on hbox.

## Where to look next

`GOAL.md` is the live execution board. `docs/RECOVERY.md` maps the interrupted Claude
session to the files it left behind. `docs/provenance-ledger.md` §"Not yet derived" is the
honest list of what must not be implemented from folklore.

The native heap scanner and damage-hook DLL now exist and have run against retail. The
highest-leverage work is integration: load real initial replay state, make the generated
SoA→PDB-image bridge non-empty, wire derived systems into the tick, and move the replay
scoreboard from trivial matches to bytes-walked matches. The exact order and acceptance gates
are in `GOAL.md`.
