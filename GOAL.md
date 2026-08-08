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

1. `ExtractDescriptors.java` — instruction-level extractor; validate against the two
   loaders we already have decompiled C for, then run it on `FUN_00570170`.
2. Sweep the whole binary for every function using the descriptor pattern → full schema.
3. `don-rules` crate: typed Rust parsers driven by the recovered schema.

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
