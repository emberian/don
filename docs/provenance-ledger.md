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

### `damage()` — the damage pipeline, `FUN_00644130`

| field | value |
|---|---|
| source | `riseofnations.exe` VA `0x00644130` (`__thiscall`, six stack dwords, `ret 0x18`) |
| implementation | `crates/don-sim/src/mechanics.rs` (`damage`, `damage_traced`) |
| tier | **B** for the arithmetic chain. Steps 10, 11 and 27 are **unverified** — never executed. The ~30 object-graph predicates are **inputs**, not derived. |
| evidence | **7,986,695 differential trials against the retail machine code, 8 seeds, 0 mismatches, 0 unexpected panics.** Distribution: numerics from a mixture (50% ±100, 25% ±10k, 12.5% full `i32`, 12.5% boundary constants), `balance_pct` uniform `i16`, angles biased onto the eleven classifier boundaries, masks biased onto the fourteen bit patterns the code tests, type ids drawn to reach the three id-keyed guards, all 26 stubbed predicates as fair coins. 10,204 trials (0.13%) excluded as retail `#DE`; 3,101 (0.04%) excluded as balance-write collisions — both counted, neither silent. |
| **mutation evidence** | **31 of 32 step-level mutations caught**: perturbing each step's arithmetic one at a time produces 7–36,984 mismatches per 49,912 trials. Full table in `docs/derivation/damage-port.md` §4. The one unpinned step is step 0 and it is *provably* inert (below). |
| harness | `crates/oracle`, `damage` command, i686-unknown-linux-musl on hbox. `FUN_00644130` is not an ISLAND; `crates/oracle/src/damage_env.rs` fabricates two objects, four vtables, `RULES`, the game object, the player array, the map, both object tables and the city table so every predicate becomes a settable dword. |
| caveats | `[0x00C0AB84]` and `[0x00C0AEC0]` are **assumed** to resolve to the same object (untested; needs a live read). The `out_kind` enum is written by retail and never compared — "0 mismatches" is about the return value only. |

Derivation and the honest gap list: `docs/derivation/damage-port.md`.

**Refuted by measurement [measured]:** the community formula "attack × modifiers − armor,
floor 1". Armor is subtracted at step 22 of 31, and moving it to the end of the chain
diverges from retail on **50,164 of 199,629 trials (25.1%)**.

**Corrections to other project documents [measured]:**

- Ghidra's decompilation of `0x00644130` (`re/decomp-all/00644130.c`) **drops the
  `or eax, 0x40` at `0x006442AB`** from the attacker-mask fixup. Structure from Ghidra,
  values from behaviour — on the exact function this project cares most about.
- That fixup **cannot change the result**: it writes a local and touches only mask bits 6
  and 17, neither of which is read downstream. Deleting it entirely produced **0
  mismatches over 199,629 trials**. Faithful and inert are different claims; both hold.
- The `out_kind` argument slot (`ebp+0x1C`) is **reused as scratch from `0x00644B2B`**
  onward. No write through that pointer occurs after the flank test.
- `docs/derivation/combat.md`'s "+10 × level / +1 × level" upgrade terms are more
  precisely `10 × (military_level × RULES[+0x8B8])` and `1 × (military_level ×
  RULES[+0x8B8])`.

### `flank_level(angle_delta)` — flank tier classifier

| field | value |
|---|---|
| source | `riseofnations.exe` VA `0x0092CFE0` (arg in ECX, 11 instructions, ISLAND) |
| implementation | `crates/don-sim/src/mechanics.rs::flank_level` |
| tier | **B** |
| evidence | 500,017 inputs standalone — 17 boundary values plus a 500,000-point stride-8191 sweep of the full `u32` domain — 0 mismatches (combat lane). Additionally pinned *inside* the damage chain: moving either boundary coarsely produced 334 and 237 mismatches per 199,629 trials. |
| note | It clobbers `EAX`/`ECX` and **preserves `EDX`**, which the caller at `0x00644B3A` depends on. |

### `entrench_dir_level` — the entrenchment direction classifier

| field | value |
|---|---|
| source | `riseofnations.exe` `0x00644E0E`–`0x00644E2D`, inlined; no standalone function |
| implementation | `crates/don-sim/src/mechanics.rs::entrench_dir_level` |
| tier | **B** |
| evidence | ran in 642,609 of 7,986,695 trials; boundary mutation pinned at 7 mismatches / 199,629 |
| finding | It is **not** `flank_level` despite the shared 0/1/2 shape and `0x40000000` split — the reject test is `(x - 0x2AAAAAAA) > 0xAAAAAAAB`, not `x > 0xD5555555`. They disagree at `x = 0` (2 vs 0). |

### `balance_index(atk, def)` — pairwise damage percentage lookup

| field | value |
|---|---|
| source | `riseofnations.exe` VA `0x00581CA0`; identical code inlined at `0x00644178`–`0x0064418E` |
| implementation | `crates/don-sim/src/mechanics.rs::balance_index` |
| tier | **B** for the address arithmetic only |
| evidence | 247,049 inputs against `0x00581CA0` — exhaustive over the 493×493 type domain plus 4,000 out-of-domain rows — 0 mismatches (combat lane). The *inlined* copy is independently confirmed by the damage harness: mutating the stride 493→494 produced 136,409 mismatches per 199,629, and sign-flipping the stored value produced 158,447, confirming the stride and the `movsx` from `i16`. |
| caveat | Table **contents** are runtime-loaded and untested; the storage-extent anomaly in `docs/derivation/combat.md` §5 is unresolved and still needs a live read. |

### `get_attack` / `get_armor` — `0x006469F0` / `0x00647DB0`

| field | value |
|---|---|
| source | `Object` vtable slots `+0x120` / `+0x124`; read `UnitType[+0x1E8]` / `[+0x214]` |
| implementation | `crates/don-sim/src/mechanics.rs` |
| tier | **B** for the base path; **unverified** for the upgrade path |
| evidence | Called as the real retail functions in all 7,986,695 damage trials. The source-field links are pinned by mutation: perturbing `UnitType[+0x1E8]` produced 150,964 mismatches and `UnitType[+0x214]` produced 99,529, per 199,629 trials. |
| gap | The upgrade branch needs `FUN_006E1370(0x16)` to return true; the harness sets `game[+0x20] |= 4`, which makes `FUN_006E1370` return false unconditionally. Never executed. |

### `as_scaled` / `as_int` — the rule-value tokenizer, `RString::AsScaled`

| field | value |
|---|---|
| source | `riseofnations.exe` VA `0x00A1D110` (`__thiscall`, one dword arg, `ret 4`); the scale-1 path is `0x0057FA60` → `RString::ToInt` `0x00A1D210` → `_wtoi` |
| implementation | `crates/don-rules/src/value.rs` (`as_scaled`, `as_int`, `wtoi`) |
| tier | **B**, conditional on two substituted CRT leaves |
| evidence | 202,643 calls against the retail instructions, **0 mismatches** (economy lane). Distribution: exhaustive over the shipped corpus — every `value=`/`entryN=` string in `ron-data/rules.xml` × 3 scales — plus 31 hand-chosen edges and 200,000 generated `num[/den][prose]` strings (`num ∈ [-2000,2000]`, `den ∈ [-200,200]`, fixed seed `0x9E3779B97F4A7C15`); `INT_MIN / -1` excluded as a retail `#DE`. **Additionally**: the Rust port reproduces all **832 recovered shipped slots** exactly (`don-rules` test `engine_tokenizer::reproduces_the_whole_shipped_corpus`), and those expectations are the live-validated table below. |
| harness | `~/don-oracle-econ` on hbox — maps the retail image, then patches IAT slots `0x00AC54AC` (`_wtoi`) and `0x00AC5434` (`wcschr`) to its own implementations |
| caveat | The Tier-B claim covers the slash search, the 32-bit multiply, the truncating divide and the zero-denominator early-out — **not** `_wtoi` itself, which was substituted. Overflowing digit strings have never been compared against MSVC's CRT; no shipped value comes close. |

Semantics: `(_wtoi(s) * scale) / _wtoi(after the first '/')`, `i32` throughout, wrapping
multiply, `idiv` truncating toward zero, **zero denominator returns 0 before the multiply**,
no denominator limit, trailing prose ignored, no decimal point. `scale` is a **compile-time
property of the field** (100 / 192 / 256 at the 40 `0x0057F950` sites; 1 everywhere else),
never of the string.

### `Rules` — the typed rules block

| field | value |
|---|---|
| source | `Rules::Load` `0x00569A90`–`0x0057016F`; schema in `docs/derivation/rules-constants.json` |
| implementation | `crates/don-rules/src/rules.rs`, generated by `re/scripts/gen_rules.py` |
| tier | **B** for the 828 constants confirmed against live memory; the block layout is [measured] structurally |
| evidence | 717 named fields / 832 recovered slots, laid out as the engine lays them out (a flat `[i32; 848]` at `RULES + 0`). 828 of 834 extracted constants matched a running match's `RULES` object byte-for-byte; the six that differed were missing scale annotations, since corrected. |
| note | 5 slots have no recovered field and 11 have no recovered value; both sets are listed in `UNRECOVERED` rather than filled in. |

### Economy — resource tick, commerce caps, rate ramp, attrition timing

| field | value |
|---|---|
| source | `Player::TickResource` `0x006CE450`, `Player::UpdateCommerceCaps` `0x006CE900`, the rate ramp at `0x00650AB5` in `FUN_006508C0`, `FUN_00608FD0` + `0x005E193D` / `0x006117C8` / `0x006115EA`, and the cost ramp ceiling at `0x006656AA` / `0x00665452` in `FUN_00664090` |
| implementation | `crates/don-sim/src/mechanics.rs` (`resource_tick`, `credit_resource`, `resource_period`, `commerce_cap`, `ramped_rate`, `rate_after_game_option`, `cost_ramp_ceiling`, `clamp_cost_to_ramp_ceiling`, `attrition_interval_scale`, `attrition_period_frames`, `merge_attrition_period`, `attrition_fires`, `attrition_recompute_due`) |
| tier | **C at best — transcribed from capstone output, never executed against retail.** No oracle harness exists for any of these; there is no differential evidence and no sample count to quote. Do not describe it as tested. |
| evidence | Instruction-level reading of each cited address (this lane, `docs/derivation/sim-economy.md`), plus the shipped constants from the live-validated table. Ghidra's C was used for control-flow shape only and disagreed with the machine code about the ordering in `0x006CE450`, which is why the port follows the disassembly. |
| inputs, not derivations | The composition of gross income and expense (`econ + 0x64` / `+0x7C`), the interest threshold, `FUN_006D66A0`'s bonus, the player-property gates `FUN_006E1370(…)`, the four attrition object-graph predicates, the additive wonder terms on the commerce cap, and the meaning of `x` in the rate ramp. All are function parameters. |

Facts worth stating separately because they are load-bearing and easy to assume wrongly:

- **The resource tick runs once per frame per player.** `FUN_00591EF0` (the frame update;
  it increments `game + 0x550` at `0x005924BF`) → `FUN_006ED2A0` → `FUN_006CE280` →
  `0x006CE900` then `0x006CE450`, with no modulo gate anywhere on that chain. The
  periodicity is entirely in the accumulator: `period = GATHER_RATE * 16 = 7200`, credited
  from `econ + 0x18 + res*4`.
- **15 fps is confirmed in the binary** [measured], not only in the `rules.xml` header:
  `0x005924CF` divides the frame counter by 15 and increments a second counter at
  `game + 0x560` on a zero remainder.
- **`unit + 0x9E` is an attrition *period in frames*, not a damage value**, and
  `FUN_00608FD0` returns an *interval* scale — which is why it divides by the attrition
  level. Attrition fires when `(frame + unit[0x0A]) % period == 0` (`0x006117C8`) and the
  period is recomputed every 32 frames per unit (`0x006115EA`). Decisive witness: the
  assassin path at `0x005E15DD` stores `ASSASSIN_ATTRITION` (`"8 frames"`) into that word
  unscaled.
- **Knowledge's commerce cap is hardcoded 999** (`0x006CE92C`, `0x1166 ^ 0x1281`) and skips
  every civ and wonder bonus.
- **The 16,000 income ceiling is conditional**, not a global clamp: `0x006CE62F` and
  `0x006CE643` jump past it, so knowledge and any player without property `0x16` never meet
  it.
- **Player sim state is XOR-obfuscated** (`^0x872` gross, `^0x26076` expense, `^0x1281` cap,
  `^0x3421` accumulator, `^0x90236` displayed income, `^0x8221` stockpile, `^0x63187` age).
  Any state mirroring or checksum work must account for it.

### Field offsets — 1,223 constants

| field | value |
|---|---|
| source | descriptor/visitor binding sites across 112 loader functions |
| implementation | `crates/don-rules/src/offsets.rs` (generated) |
| tier | **B** for the three with two independent derivations; the rest are mechanically extracted and unvalidated individually |
| evidence | `RECHARGE`=500, `CREW_SIZE`=780, `BASE_FORM`=784 agree between instruction-level extraction and Ghidra decompiled C |

---

## Not yet derived (do not implement from folklore)

- **Which predicates fire in a real game.** The damage *arithmetic* is ported and Tier B;
  the ~30 object-graph guards that select the modifiers are inputs. Resolving them is a
  world-state problem and is the largest remaining piece of combat.
- Damage steps 10, 11 and 27, and the upgrade branches of `get_attack`/`get_armor` — all
  transcribed from the disassembly, none ever executed. See `docs/derivation/damage-port.md` §6.
- Whether `[0x00C0AB84]` and `[0x00C0AEC0]` resolve to the same object. The damage harness
  assumes they do; if they do not, every field read through the second table in the port
  reads the wrong offset.
- The damage-kind enum written through `out_kind` (`ebp+0x1C`), at the five sites before
  `0x00644B2B`.
- The engine's RNG algorithm and its call sites.
- **How income is composed.** The resource *tick* and its accumulator are ported, but the
  expression that turns worker counts, `CITY_GATHER`, `BASIC_GATHER`, `SCHOLAR_RATE` and the
  building-bonus tables into the gross at `econ + 0x64` is not derived. `PEASANT_RATE`
  (2560) and `OIL_RATE` (8960) are 8.8 fixed point and are read by indexed sites
  (`0x00633E9C`, `0x006336A9`, `0x006E0A36`, `0x006E0902`, `0x006CF44C`), none of them
  reduced to a formula.
- **Cost ramping.** `FUN_00664090` is ~12 KB and was not reduced; only its class ramp
  ceiling and the per-slot clamp are ported. `SUPPORT` and `PROGRESSION` are described only
  by BHG's prose in `unitrules.xml` — **no engine site has been tied to either**, so the
  ramp *curve* must not be implemented.
- The units of `x` in the rate ramp (`UnitType` vtable `+0x6C` / `+0x70`), which is why
  `ramped_rate` must not be wired to a build queue and called train time.
- The construction of the attrition level at `player + 0x7F0` and the **scale** of the float
  multiplier at `player + 0x7F4` (built at `0x006CDD20`). `ATTRITION_IMPROVED[] = 1,2,4,8`
  is a plausible source for the former and remains a hypothesis.
- Border geometry (`BorderSpline`).
- Pathfinding.
- The lockstep checksum's field set (`CheckSum` / `DataWalk`) — which would *define*
  sim-critical state.
- ~~Rule-value tokenizer semantics: denominator limit, rounding, `f32` vs fixed point.~~
  **Closed** — `0x00A1D110`, implemented and Tier B; see the entry above. There is no
  denominator limit, rounding is `idiv` truncation applied after the multiply, and the
  result is `i32`, never `f32`.
- Descriptor type-tag meaning outside two validated loaders. A hypothesis that it selects
  the value parser was tested against the shipped XML unit words and **refuted**. (The
  economy lane closed the related question separately: the descriptor "type tag" is
  `wcslen(name)`, 1195/1195.)
- The element-name blob the loader indexes (`*(0x00C06378) + 0x10 + K`) is supplied by a
  runtime data file not present in `ron-data/`, so the authoritative name list — and
  therefore dead-entry and mod analysis — is still out of reach.

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
