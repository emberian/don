# Provenance ledger

Every mechanic implemented in this project appears here, with where it came from and what
evidence backs it. A mechanic without an entry is not done (`docs/CHARTER.md`).

## Fidelity tiers

| tier | meaning |
|---|---|
| **A** | Proven equivalent over the entire input domain by an SMT/bitvector check. Bounded by the fidelity of the lifted semantics — state that when citing it. |
| **B** | Differentially tested against the retail code. **Testing, not verification.** Sample count and input distribution are mandatory. |
| **C** | Behaviourally faithful; divergence measured and bounded. |

Nothing here is "verified" in the proof-assistant sense — we hold no formal semantics of
Rust or of x86.

---

## Implemented

### `hash_into_range(a, b, lo, hi)` — bounded integer mapping

| field | value |
|---|---|
| source | `riseofnations.exe` VA `0x00846450` (`__stdcall`, 4 dword args, `ret 0x10`) |
| implementation | `crates/don-sim/src/mechanics.rs` |
| tier | **B** |
| evidence | 500,008 inputs — 8 hand-chosen edge cases (`i32::MIN`, `i32::MAX`, zero range, inverted range, negative operands) + 500,000 pseudo-random from a fixed seed — **0 mismatches** |
| harness | `crates/oracle`, `difftest` command, i686-unknown-linux-musl on hbox |
| reachability | ISLAND (no calls, no off-`.text` references) — callable with fabricated inputs |

Computation: `((a*a*b) mod (|hi-lo| + 1)) + lo`, all arithmetic wrapping signed 32-bit,
`idiv` truncating toward zero.

Confirmed semantics that are easy to assume wrongly:
- Multiplication **wraps**; `a*a` alone can go negative.
- The remainder takes the sign of the **dividend**, so results can fall *below* `lo`.
  Faithful, not a porting bug.
- `hi` need not exceed `lo`; the divisor uses `|hi - lo|`.

**What it is used for is not established.** The name describes the computation only. It is
deliberately not called "the RNG" or "the damage roll" — that would be a claim we have not
earned.

*Process note:* the first version of the unit test carried hand-computed expectations, and
one was wrong (`(7,-3,-100,100)` → I wrote -47; retail returns **-247**). Expectations are
now captured from the binary via `oracle vectors`. **Capture, do not calculate.**

### Field offsets — 1,223 constants

| field | value |
|---|---|
| source | descriptor/visitor binding sites across 112 loader functions |
| implementation | `crates/don-rules/src/offsets.rs` (generated) |
| tier | **B** for the three with two independent derivations; the rest are mechanically extracted and unvalidated individually |
| evidence | `RECHARGE`=500, `CREW_SIZE`=780, `BASE_FORM`=784 agree between instruction-level extraction and Ghidra decompiled C |

---

## Not yet derived (do not implement from folklore)

- Damage pipeline: operation order, rounding, the hardcoded per-mask modifier table.
- The engine's RNG algorithm and its call sites.
- Gather rates, cost ramping, attrition timing.
- Border geometry (`BorderSpline`).
- Pathfinding.
- The lockstep checksum's field set (`CheckSum` / `DataWalk`) — which would *define*
  sim-critical state.
- Rule-value tokenizer semantics: denominator limit, rounding, `f32` vs fixed point.
- Descriptor type-tag meaning outside two validated loaders. A hypothesis that it selects
  the value parser was tested against the shipped XML unit words and **refuted**.
