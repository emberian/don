# GOAL

**Build Descent of Nations**: a deterministic, batch-parallel Rust reimplementation of the
Rise of Nations: Extended Edition simulation, derived from the binary rather than from
community documentation, bit-exact wherever we can prove it, and shaped to become an RL
environment capable of training a SOTA self-play player-AI.

Constitution: `docs/CHARTER.md`. Ground truth so far: `docs/binary-ground-truth.md`.
Stage-3 architecture: `docs/oracle-architecture.md`. Cross-check only (never a source):
`docs/prior-art-survey.md`.

## Current thrust

**Stage 1 — loader recovery.** Extract the complete (rule name → type tag → struct offset)
schema mechanically from the descriptor/visitor call sites, for every loader. The
decompiler times out on `FUN_00570170` (the rules.xml loader), so extraction happens at
the instruction level, which is the mechanical path we want regardless.

## Next 3 moves

1. Raise binding recall in `FUN_00570170` — only 45 of ~719 name records resolved an
   offset there, vs 32/35 and 22/23 in the validated loaders. Find the variant pattern
   (likely a different `this` access form or a global rather than `this+disp`).
2. Verify the `tag` field's meaning. It matches ground truth in the two validated loaders
   (8=recharge, 9=crew_size) but the values in `FUN_00570170` (15, 20, 17, 16, 18, 28…)
   are non-monotonic and unexplained — currently **[unverified]**, do not build on them.
3. `don-rules` crate: typed Rust parsers driven by the recovered schema, including the
   prose-rational tokenizer (`"1/16 tile (calibration…)"`).

## Done-log

- Extracted + hash-verified all 45 data XMLs and both binaries out of the Parallels VM.
- Established PE ground truth: 2024 MSVC-14 rebuild, PE32 i386, unpacked, 620 RTTI classes.
- Settled the FP question: SSE binary32, not x87 → bit-exactness is achievable; entire risk
  surface is 8 named CRT transcendental imports.
- Ghidra project built and analyzed: 47,177 functions, 14,441 strings.
- Proved the UTF-16-lowercase string-anchor methodology; found the rules.xml loader
  (`FUN_00570170`) and two unitrules-side loaders.
- Discovered the descriptor/visitor binding framework (name + type tag + `this+offset` via
  `vtable+0x1c`) — the schema is mechanically extractable.
- Decided the stage-3 oracle architecture (native in-process PE mapping; documented
  skip-list), and adopted `[measured]`/`[reported]` provenance discipline after three
  claims dissolved on contact.
- **Stage 1 in progress.** `ExtractDescriptors.java` written and *validated against
  known-good decompiled C* (recharge→tag 8/off 500, crew_size→tag 9/off 780,
  base_form→tag 9/off 784, all exact). Whole-binary sweep: 197 candidate loader
  functions, 750 confirmed bindings (tag+offset). Recovered the rules.xml constant table
  from `FUN_00570170` — the function the decompiler could not handle — with offsets on a
  clean 4-byte stride matching rules.xml declaration order.
- Resolved an open question from `docs/binary-ground-truth.md`: `push dword ptr
  [ebx+0x1f4]` is a *load*, so `this+offset` holds a **pointer to** the storage, not the
  storage itself. Consistent with the engine keeping parallel per-attribute arrays
  (an SoA layout) — that latter part is still **[unverified]**.
