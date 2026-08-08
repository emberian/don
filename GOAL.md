# GOAL

**Build Descent of Nations**: a deterministic, batch-parallel Rust reimplementation of the
Rise of Nations: Extended Edition simulation, derived from the binary rather than from
community documentation, bit-exact wherever we can prove it, and shaped to become an RL
environment capable of training a SOTA self-play player-AI.

Constitution: `docs/CHARTER.md`. Ground truth so far: `docs/binary-ground-truth.md`.
Stage-3 architecture: `docs/oracle-architecture.md`. Cross-check only (never a source):
`docs/prior-art-survey.md`.

## Current thrust

**Stage 1→2.** Schema extraction is working (1,224 confirmed bindings). Now growing
`don-rules` from the shipped data while the remaining binary semantics are recovered.

## Next 3 moves

1. Verify the `tag` field's meaning. It matches ground truth in the two validated loaders
   (8=recharge, 9=crew_size) but the values in `FUN_00570170` (15, 20, 17, 16, 18, 28…)
   are non-monotonic and unexplained — currently **[unverified]**, do not build on them.
   Likely encodes the value *parser* to use (tile-fraction vs frames vs percent); test by
   correlating tags against the unit words in the shipped XML.
2. Recover the loader's own tokenizer semantics from the binary so `RuleValue` can gain a
   sound numeric conversion: denominator limit, rounding, and whether the result lands in
   `f32` or fixed point. Until then `don-rules::value` deliberately refuses to convert.
3. Identify the remaining loaders by name (map the 112 binding functions to the XML files
   they load) and grow typed structs from `schema/bindings.json`.

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
- **Stage 2 begun.** Rust workspace + `don-rules` crate. `value::RuleValue` tokenizes the
  prose rule-value grammar (rational, percent, number+unit, bare, suffix-unit); 8 tests
  green including a corpus test over all **690** value-bearing elements of the shipped
  `rules.xml`. The corpus test was canary-checked to confirm it reads real data rather
  than skipping. The module deliberately refuses to convert values to numbers: the
  engine's tokenizer semantics are not yet recovered, and inventing a plausible
  conversion is the folklore the charter forbids.
- **Hypothesis refuted (recorded, not buried):** the descriptor type tag does *not* encode
  the value's unit/parser. Joined 178 rules-loader bindings against the shipped XML unit
  words; tag 19 spans `%`, `tile`, none and `tiles`, tag 16 spans none, `bonus`,
  `resources`, `tile`. No correlation. Tag meaning remains **[unverified]** outside the two
  loaders where it was checked against decompiled C.
- **Extractor bug found and fixed:** displacements were read unsigned, so `support[scan]`
  reported offset 4294967288 instead of −8. 58 of 1,224 bindings were affected.
- `schema/bindings.json` → `crates/don-rules/src/offsets.rs` via `re/scripts/gen_offsets.py`:
  **1,223 offset constants across 112 modules**, generated not hand-written. 11 tests green,
  including ground-truth anchors that pin the extractor against the independent
  decompiled-C derivation.
- **Stage 3 begun.** `don-pe`: PE32 reader + image mapper, the architecture-independent
  half of the oracle harness. Parses headers, maps sections at their virtual addresses,
  and applies base relocations (**315,865** HIGHLOW fixups on the real image, canary-
  verified). 5 tests green including a relocation round-trip (relocate away, relocate
  back, assert byte-identical) — the strongest cheap check that the fixup arithmetic is
  right. Deliberately not `LoadLibrary`: that would run the entry point as DllMain and
  drag in loader import/TLS/CFG processing we need to stay out of.
- **Stage 3 COMPLETE — the oracle executes retail code.** `crates/oracle` maps
  riseofnations.exe into a live i686 process on hbox (315,865 relocations, per-section
  mprotect), and calls retail functions with fabricated inputs. Differential test against
  Rust models: `0x00472400` (`movsx eax,[this+0xA]`) and `0x0048F770`
  (`[this+0x12C] - [this+0x12A]`), **200,000 trials each, 0 mismatches** — Tier B.
  Built for `i686-unknown-linux-musl`, whose self-contained CRT objects avoid installing
  32-bit dev packages on a co-tenant machine. Every call runs in a forked child, so
  probing a function that needs live globals reports a signal instead of killing the run.
- `re/scripts/FindIslands.java` classifies all functions by reachability:
  **2,135 ISLAND** (callable with fabricated inputs), 711 DATA_ONLY, 42,748 SELF_CALL,
  970 WRITES_GLOBAL. 141 ISLANDs carry arithmetic — the differential-testing worklist.

## Where the roadmap actually stands

Stages 0–3 done. Stage 4+ (sim core, combat/movement/pathfinding/borders, determinism,
batch scaling, RL surface, player-AI) not begun. **No simulation, no mechanics, no
benchmarks yet.** The oracle now makes each mechanic *derivable* rather than guessable,
which is what stages 4–6 depend on.
