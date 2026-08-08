# Oracle architecture (stage 3) — decided

Sources: a tooling survey (2026-08-08) plus local verification. Claims below are marked
**[measured]** where I verified them against our own binary/environment, and
**[reported]** where they come from the survey and have not been independently checked.
Do not promote a [reported] claim to [measured] without doing the check.

## The decision

**Primary oracle: native in-process PE mapping — map `riseofnations.exe`, apply `.reloc`,
`mprotect` RX, take a function pointer at an RVA, call it.** No emulator, no debugger,
never launch the game. Runs at native speed on **hbox** (x86_64 Linux) with an
`i686-unknown-linux-gnu` harness.

Why this over emulation: it is a real precedent, not a theory —
[sushi-shi/gruntz-decomp](https://github.com/sushi-shi/gruntz-decomp) does exactly this
against a 32-bit Windows game, and its `recomp/` harness is worth reading before we write
a line. Native is ~10³–10⁴× faster than Unicorn [reported], which is the whole argument
when we want millions of differential calls.

**Reachability governs what is testable.** Audit every target as gruntz does:
- `ISLAND` — no relocs, no calls. Fully testable with fabricated inputs.
- `SELF-CALL` — calls only into itself/other mapped code. Testable.
- `DATA-ONLY` — relocs only into constant tables. Testable.
- Anything touching imports or live globals is **out of reach** for fabricated inputs and
  needs the fallback path.

RTS sim kernels (damage resolution, resource ticks, cost ramps, RNG) are unusually likely
to be ISLAND/DATA-ONLY [reported] — which is exactly the set we most want under proof.

**Fallbacks, in order:**
1. **Minidump + Unicorn** for globals-touching functions — take a minidump of the live
   game so globals and the CRT are already initialized, then call in. Use
   `UC_CTL_CONTEXT_MEMORY` so `context_save`/`context_restore` snapshots CPU *and* memory
   and we can reset between every call. [dumpulator](https://github.com/mrexodia/dumpulator)
   is the reference implementation of the pattern (⚠ stale since 2024 but functional).
2. **Frida `Interceptor`** on the live game to harvest *realistic argument corpora* — far
   better than fabricated inputs for validating distributions. Use `Interceptor`, not
   `Stalker` (weak on IA-32).
3. **WinDbg TTD** for whole-execution ground truth. 32-bit x86 is fully supported; record
   on x86_64 Windows, replay anywhere including ARM64. `-module <name>` records only the
   sim module at ~5–20× overhead instead of the whole game. [reported]

## Environment constraints [measured]

- This Mac is **arm64** — cannot run x86-32 natively. Ghidra and software emulation only.
- **hbox** (x86_64 Ubuntu 24.04, 24 cores, 123G) is the only native x86-32 host, and it is
  **co-tenant with codex's HOL build**. Use `swarm-build`; keep waves small.
- Harness must target `i686-*`, whose rustc config is `cpu: pentium4` / `rustc_abi: X86Sse2`
  — SSE2, not x87. (The x87 unsoundness lives on `i586-*` targets.)

## Do not build on these

| Rejected | Reason |
|---|---|
| **Qiling** | Windows-API emulation in Python callbacks; strictly slower than raw Unicorn; needs a Windows DLL rootfs. If a function makes zero API calls you didn't need it; if it makes many it is catastrophic. |
| **QEMU full-system record/replay** | Single-vCPU TCG only, no D3D-capable devices, Windows guests a known sore spot, and no x86 hardware accel on Apple Silicon. A 2003 RTS under TCG+icount will not be an interactive harness. |
| **PANDA.re** | Ancient QEMU base; the Rust rewrite `panda-ng` has ~6 stars and no README. Inherits QEMU's problem anyway. |
| **rr** | Cannot record modern Wine — new-WoW64 mode trips an assertion, and Wine 11 made it the default. Would require pinning/building an old Wine *and* lowering `kernel.perf_event_paranoid` host-wide on a shared box. |
| **Windows-on-ARM instrumentation** | Register context unreliable when not actively debugged; emulator generates constant spurious exceptions; no single-step across x86/CHPE boundaries; **TTD on ARM64 records ARM64 only**. Use the VM for "does it still run", nothing more. |
| **Triton** (for FP) | Verified at source by the survey: 383 instruction cases, **zero floating-point arithmetic** — only moves/bitwise for SSE. Excellent for the integer core, disqualified for FP paths. |
| **SAW** | Machine-code backend is x86_64 raw binaries only — no 32-bit, no PE. |
| **RetDec / mcsema / anvill / rellic** | Limited-maintenance, archived (mcsema archived 2022 with no README notice — a genuine trap), or dead. |
| **Ghidrathon / Ghidra-Cpp-Class-Analyzer** | Both archived. PyGhidra is the supported Python path on Ghidra 12.x; use the built-in RTTI analyzer + `RecoverClassesFromRTTIScript.java`. |
| **byte-matching (reccmp/objdiff)** | Measures MSVC-vs-MSVC byte similarity. Says nothing about a Rust reimplementation. |

## ⚠ The warning that shapes everything

**Ghidra's decompiler is at its weakest on float-heavy 32-bit MSVC code**, with open
upstream bugs directly on our profile — most alarmingly
[#3935](https://github.com/NationalSecurityAgency/ghidra/issues/3935), where it **silently
reorders integer vs floating-point operations**. For a project whose entire value rests on
operation order and rounding, that is the worst possible failure mode: plausible C that is
wrong in exactly the dimension we care about.

**Rule: get structure from Ghidra; get values from behavior.** Never transcribe an
arithmetic expression from decompiled C into Rust and call it derived. The decompiled form
is a hypothesis; the oracle is the judge.

The gruntz project supplies the empirical backing [reported]: of five functions, one
carried two logic bugs that a 78.77% byte match never revealed, and other functions
verified behaviorally correct scored **0.00% match**. **A match percentage is neither
necessary nor sufficient for correctness.** This is the same claim our fidelity tiers
make; treat it as confirmation, not novelty.

## Floating point — refined [measured, refining earlier work]

The survey's histogram agrees with mine and sharpens it:
- **~28,700 SSE vs ~559 x87**, and **72% of x87 arithmetic sites sit in one 64KB bucket at
  `0xa5xxxx`** — i.e. the CRT blob at the end of `.text`, not sim code. [reported]
- **Zero `sqrtss`/`sqrtsd`** in the image, and **zero `_ftol`/`_ftol2`** — every sqrt is a
  CRT call, and float→int is inline `cvttss2si`. [reported, cheap to re-verify]

Consequences, now settled:
- **Drop x87 emulation entirely.** No softfloat, no f80. Target is IEEE binary32 scalar
  SSE2, round-nearest-even, no FMA, no reassociation. Rust `f32` maps 1:1.
- **No FPU control-word dependence on sim paths** — SSE has no precision-control field, so
  the entire `D3DCREATE_FPU_PRESERVE` / PC=24/53/64 class of hazard is irrelevant here.
- **angr is rehabilitated**: its disqualifying flaw (VEX stores x87 as 64-bit doubles and
  ignores precision control) is x87-specific. VEX models SSE with real `Ity_F32`/`Ity_F64`.
- **Z3 `Float32` = `(_ FloatingPoint 8 24)`** is exactly our type, so Tier-A FP proofs are
  genuinely on the table (bit-blasting limits them to kernels, not bulk).
- **The entire FP fidelity risk is eight named imports**:
  `_libm_sse2_{sin,cos,tan,asin,acos,atan,pow}_precise` are Microsoft/Intel-specific and
  not correctly rounded; Rust's libm will differ in the last ulp.
  `_libm_sse2_sqrt_precise` is the exception — sqrt is correctly rounded per IEEE-754, so
  `f32::sqrt()` is provably bit-identical. **Action: hook those eight, log (in, out) pairs,
  decide per-function.**

## The free oracle already inside the binary [partially measured]

**Measured in our binary:** `SyncLogger` (42 ASCII / 5 UTF-16 refs), `DataWalk`, `CheckSum`,
`RandomLogEntry`, and the UTF-16 channel names `scenario_data` (17), `script_run_time` (3),
plus `desync`. SkyBox shipped the multiplayer desync-diagnosis rig.

**[reported], and NOT found as strings in our binary:** `CheckSumsCommand`, `SyncTags`,
`SyncDefine`, `mCheckDesyncsEveryXFrames`, `mDesyncTrackingEnabled`,
`mRandomSeedHostOverride`, `mConfigName`. Member names would live in the PDB, not in a
release binary, so their absence is *expected* and is not evidence against — but it does
mean these names are unverified for our build and must not be cited as measured.

If the reported shape holds, this is a **per-frame, per-subsystem differential oracle
against the real binary, for free**: an adler32 `DataWalk` visitor checksumming
`units, builds, walls, ammo, deaths, groups, guys, leaders, cities, items, goods, world,
rules, scenario_data, script_run_time`, and a `RandomLogEntry { frame, file, line, seed }`
giving **per-call RNG provenance with source file and line**.

Note how this corroborates the descriptor/visitor framework in
`docs/binary-ground-truth.md`: `DataWalk` is a visitor, and the same descriptor tables that
bind XML names to fields plausibly drive checksumming too. That upgrades the "same tables
serve load, save and checksum" inference from speculation to *supported* — still not
verified. **Next action: find the checksum config toggle and the `DataWalk` call sites.**

## The PDB question — **CLOSED, we have it** [measured, 2026-08-08]

- Our build's debug directory: `rise.pdb`, GUID **51D4F219-61C6-4F84-9D5B-C3361B0D291F**,
  age **1**, symsrv key `51D4F21961C64F849D5BC3361B0D291F1`.
- **The Microsoft symbol server returns 404** for both `rise.pdb` and the compressed
  `rise.pd_` at that key. Verified by direct request. The PDB for our build is not public
  *on symsrv* — which is why this section previously concluded we did not have one.
- **But the game installs it.** `ron-bin/sbl/rise.pdb`, 57,290,752 bytes, GUID and age
  **identical** to the exe's CodeView record. It shipped in the install's `sbl\` folder the
  whole time and was missed because an early recon listing was truncated. 37,138 publics,
  22,752 procedures with names/sizes/signatures, full type information.
- Extracted to `schema/rise-symbols.tsv`, `schema/rise-procs.tsv`, `schema/pdb-types.json`,
  `schema/command-structs.txt`.

**This does not change the oracle's job.** The PDB gives names, signatures and layouts; it
gives no semantics and no values. Every Tier-B result still has to come from executing the
shipped code. What it does change is *targeting*: we no longer guess which `FUN_` to probe,
and struct layouts for probe arguments come from the compiler instead of from inference.
The claim-by-claim audit of pre-PDB derivations is `docs/derivation/PDB-RECONCILIATION.md`.

[StackAndPointer/Rise-of-Nations-Decomp](https://github.com/StackAndPointer/Rise-of-Nations-Decomp)
is worth understanding precisely, because it is easy to overvalue:
- Its **classes/functions are IDA Pro + Hex-Rays output** — the same epistemic category as
  our Ghidra output. **Not ground truth.** Under our methodology it is a hypothesis source,
  exactly like the wiki.
- Its **`Enums.h` is PDB-Insight output**, and PDB-Insight only *parses existing PDBs*. So
  a PDB existed for *some* build. Which build, and where it came from, is unknown.
- The repo states no version/build identifier, and says "do not expect this code to compile
  or run".

**PDB type data is categorically different from wiki folklore** — it is compiler-emitted
metadata about a real binary. That reasoning was right and the chase is over: we have the
genuine `rise.pdb` for our exact build (above), so the StackAndPointer repo is now
superseded for names and types and remains cross-check material only. The former open lead
about downgrading to older EE builds for their GUIDs is **closed — no longer needed**.

## Tier 3 pipeline

There is no off-the-shelf binary↔Rust equivalence prover, and **nothing lifts binaries to
Rust** (every "bin2rust" project found is AI-generated vaporware). The workable shape keeps
everything in one solver and one language:

1. Lift the target with **pypcode/SLEIGH** (or angr/VEX) to recover semantics.
2. **Mechanically transcribe** those semantics into a Rust `spec_fn`.
3. Prove `spec_fn(x) == our_fn(x)` for all `x` inside **Kani** with `kani::any()`.
4. **Differential-test the transcription itself** against the Tier-2 oracle on millions of
   inputs — testing validates the *lift*, proving validates the *reimplementation*.

Tooling: **Kani 0.67.0** is the best bet (f32/f64 bit-precise; ⚠ `sin`/`cos`/`sqrt` are
over-approximated, which is fine because those live in the CRT and are handled by hooking).
**Verus** gained IEEE FP theory in March 2026 and is now a real option but needs hand-written
proofs. **angr 9.3.2** for closed-form recovery.

Honest walls: loops get bounded unwinding only (a data-dependent pathfinder yields a proof
to depth *n* plus testing beyond); functions reading live globals are out of scope for
∀-input proofs; the eight CRT transcendentals are unprovable by construction. The *real*
fidelity risk in an integer RTS sim is division/modulo semantics and 32-bit wrapping — and
those are exactly what Kani proves well.
