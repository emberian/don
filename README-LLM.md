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

**The live process.** Read memory with `OpenProcess` + `ReadProcessMemory` via P/Invoke in
guest PowerShell:

```sh
prlctl exec "Windows 11" powershell.exe -EncodedCommand <base64 of UTF-16LE script>
```

Then **write results to a guest file and stream that out** — inline base64 return times out.
Use `certutil -encode` + `type` for binary, exactly as documented in
`docs/binary-ground-truth.md`.

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
- **Do not `git stash`** — the tree is shared with parallel lanes. Lanes must not commit;
  the orchestrator commits. Never `git add -A` while lanes are live.
- **`schema/islands.jsonl` is not a complete function list** — Ghidra left gaps, and two
  combat-critical functions sit in them.
- **Ghidra decompiler timeouts are real** — `FUN_00570170` exceeds a 600 s budget. Extract at
  the instruction level instead.

## Established ground truth (do not re-derive)

`riseofnations.exe` sha256 `30478a44…625079`, **PE32 i386**, image base `0x00400000`, a
**2024-06-20 MSVC-14 rebuild** of the 2003 code, unpacked, RTTI intact. `patriots.exe` is
only the MFC launcher.

- **Float**: SSE binary32, not x87 (~23.7 K scalar SSE vs ~560 x87). Entire IEEE hazard
  surface is eight CRT imports `_libm_sse2_{acos,asin,atan,cos,pow,sin,tan}_precise`; `sqrt`
  is IEEE-exact and safe. **The damage pipeline and A\* contain no floating point at all.**
- **RNG**: LCG `s ← s·1664525 + 1013904223`, `next_float` `0x00a39cf0`, `in_range`
  `0x00a39d70`, 414 call sites, ≥7 streams. Pathfinder uses a *different* `Random` via
  pointer global `[0x00C06184]`; script/sim uses fixed object `0x00EB697C`.
- **RULES**: `[0x00C061E4]` and `[0x00C061F0]` **alias the same object** (live-read).
- **Damage**: `FUN_00644130`, `__thiscall`, pure integer. Attack stored **×10**; rescale
  `(D+5)/10`; armor subtracted **mid-chain** at step 22 of 31; floor of 1 **conditional**.
  The community formula is wrong in structure.
- **Pathfinding**: pure-integer 8-connected grid A\*, draws RNG once per edge relaxation.
- **Checksum**: `CheckSum`/`SaveGame`/`LoadGame` are siblings of one `DataWalk` interface, so
  **sim-critical state ≡ save-game state**. adler-32 at `0x00a46830`, `check_all`
  `FUN_00936560`, `process_check_sums` `FUN_009459d0`.
- **Rule tokenizer**: `RString::AsScaled` `0x00a1d110` — `(num * scale) / den`, where `scale`
  is a *per-field compile-time constant* (192 tiles, 256 for 8.8 fixed point, 100 percent).
- **Balance table**: base `0x00C06AFC`, 493×493 int16. ⚠ The captured window has unexplained
  negatives — **bound its real extent before trusting it**.
- **Replays**: `.rcx` is a **plain gzip stream from offset 0** (this refutes the reported
  10-byte-header claim). Payload opens with a UTF-16 version string carrying the build date.
- **`[0x00846450]` is dead code** — correctly derived, called by nothing, *not* the RNG.
- **The descriptor "type tag" is the name's string length**, not a type tag. That is why the
  unit-encoding hypothesis was refuted.

## Artifacts

| path | what |
|---|---|
| `schema/bindings.json` | 1,224 rule-name → struct-offset bindings across 112 loaders |
| `schema/islands.jsonl` | every function classified by reachability (ISLAND 2,135 / DATA_ONLY 711 / SELF_CALL 42,748 / WRITES_GLOBAL 970) |
| `schema/vtables.json` | **1,777 RTTI vtable addresses → class names.** The key to typed live-heap crawling: every C++ object starts with its vtable pointer |
| `docs/derivation/rules-constants.json` | 719 constants with offsets, parsers, scales |
| `crates/don-rules/src/offsets.rs` | 1,223 generated offset constants |
| `docs/derivation/*.md` | per-subsystem derivation reports + `AUDIT*.md` |
| `docs/provenance-ledger.md` | every implemented mechanic, its source, tier, evidence |

Gitignored as copyrighted game content: `ron-bin/`, `ron-data/`, `re/ghidra/`,
`re/decomp-all/`, `schema/live/`.

## Crates

- **`don-rules`** — rule-value tokenizer + generated offsets. *Deliberately refuses* to
  convert values to numbers until the engine's tokenizer semantics are settled.
- **`don-pe`** — PE32 reader/mapper; applies 315,865 relocations. First half of the oracle.
- **`don-sim`** — SoA world, batch scheduler, `mechanics.rs` (derived mechanics only;
  placeholders live in `world.rs` and are marked). Parallel stepping must reproduce serial
  output bit-for-bit — there is a test, keep it passing.
- **`don-gpu`** — wgpu flow-field prototype. GPU output asserted bit-identical to CPU.
- **`oracle`** — 32-bit only, excluded from the workspace, runs on hbox.

## Where to look next

`GOAL.md` holds the current thrust and done-log. `docs/provenance-ledger.md` §"Not yet
derived" is the honest list of what must not be implemented from folklore.

The highest-leverage unbuilt tools, in order: a **native heap scanner** (use
`schema/vtables.json` to type every live object), a **function-hook DLL** on
`FUN_00644130` to log real damage calls with full inputs, and **`WriteProcessMemory`
control** to turn the live game from an observatory into a programmable laboratory.
