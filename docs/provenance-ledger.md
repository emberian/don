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

**CORRECTION [measured, 2026-08-08 derivation wave]: this function is called by nothing.**
It is dead code, and it is *not* the engine RNG. The real generator is a Numerical-Recipes
LCG, `s <- s*1664525 + 1013904223`, at `0x00a39cf0` / `0x00a39d70` — constants verified
independently by capstone. Declining to call `0x00846450` "the RNG" was the right call; the
derivation itself remains correct, it simply describes an unreachable function.

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

---

## Live-process validation (2026-08-08)

The RULES object was read out of a **running, match-loaded** `riseofnations.exe`
(PID 14644, image base `0xD60000` — Windows randomises ASLR per *boot*, not per process,
so both concurrent instances shared it). `[0x00C061E4]` and `[0x00C061F0]` both held
`0x01798B88`: **two pointer globals aliasing one RULES object**, which retires the
cross-lane contradiction the audit raised — neither lane was wrong and there is no second
rules object.

**828 of 834 extracted constants match live memory byte-for-byte (99%).** [measured]
This validates `docs/derivation/rules-constants.json`, the loader-derived scale table, and
the offset extraction, all at once and against ground truth rather than against itself.

The six mismatches are not extraction errors so much as *missing scale annotations*, and
each corroborates a finding from a different lane:

| field | extracted | live | ratio | explanation |
|---|---|---|---|---|
| `scholar_rate` ×5 slots | 5, 7, 10, 15, 20 | 1280, 1792, 2560, 3840, 5120 | **256** | 8.8 fixed point — the scale-256 binder, recorded as unscaled |
| `caravan_attack_bonus` | 2 | 20 | **10** | attack values are stored ×10, exactly as the combat lane derived |

Raw capture: `schema/live/rules-block-pid14644.txt` (4096 bytes, base64).

Method, which is now the standard one for runtime questions: `OpenProcess` +
`ReadProcessMemory` via P/Invoke in guest PowerShell, driven by
`prlctl exec "Windows 11" powershell.exe -EncodedCommand <base64-utf16le>`, writing results
to a guest file and streaming that out. Inline base64 return times out; the file hop does
not.

### Streams — pathfinding has its own RNG [measured]

| global | menu | loaded match |
|---|---|---|
| `[0x00C06184]` → pathfinder `Random` | `0x01797A8C` (heap, populated) | `0x01797A8C` |
| `0x00EB697C` script/sim `Random` state | `0x00000000` | `0x085725D3` |

Distinct objects with distinct lifetimes: the pathfinder's is heap-allocated and live at
the menu, the script/sim one is a fixed `.data` object seeded when a match starts. So a
pathfinding divergence does **not** automatically desynchronise the scripted/sim stream —
the pathfinding lane's headline overstated the blast radius, as the audit suspected.
