# Provenance ledger

Every mechanic implemented in this project appears here with where it came from, what
evidence backs it, and what that evidence does *not* cover. A mechanic without an entry is
not done (`docs/CHARTER.md`).

Reconciled **2026-08-08** against every report under `docs/derivation/`, both adversarial
audits (`AUDIT.md`, `AUDIT-replay.md`), both tooling reports under `docs/tooling/`, and the
actual contents of `crates/`. Change log: `docs/tooling/ledger-reconciliation.md`.

**How to read this file.**

- §1 is the mandatory list: mechanics that exist as Rust in this repo.
- §2 is derived-and-tiered work that is **not implemented**. Those rows are not a licence to
  implement from the prose; they are a record that the harness exists.
- §3 is structural knowledge (layouts, formats, addresses) — real, measured, and **not** a
  fidelity claim about any Rust we ship.
- §4 is corrections and refutations, stated with the wrong claim, so nobody re-derives them.
- §5 is where lanes still disagree, recorded as OPEN rather than adjudicated.
- §6 is the standing do-not-implement-from-folklore list.
- §7 is the compliance sweep of `crates/` against this file.

## Fidelity tiers, and the words we are not allowed to misuse

| tier | meaning |
|---|---|
| **A** | Proven equivalent over the entire input domain by an SMT/bitvector check. Bounded by the fidelity of the lifted semantics — state that whenever citing it. |
| **B** | Differentially tested against retail machine code. **Testing, not verification.** Sample count and input distribution are mandatory. |
| **C** | Behaviourally faithful; divergence measured and bounded. Includes corpus observation and live capture. |
| **structural** | Layout / address / format recovered by reading the image, the shipped data, or the shipped debug info. No behaviour was compared. Not a tier in the fidelity sense; recorded so it cannot be mistaken for one. |
| **engineering** | Our own code, our own design. Makes no claim about Rise of Nations at all. |

**There is no Tier A anywhere in this project.** Nothing has been proven over a whole input
domain. We hold no formal semantics of Rust or of x86, so nothing here is "verified" in the
proof-assistant sense, and a green `cargo test` is evidence about the Rust crates only —
never about a fidelity claim, because `Cargo.toml` excludes `crates/oracle` and every Tier-B
result in this project lives in an oracle harness the repo's test command never runs.

Repo test state at reconciliation: **`cargo test` → 91 passed, 0 failed, 0 ignored**
(don-gpu 8 + parity 5, don-pe 5, don-rules 18, don-sim 55). [measured, 2026-08-08]

## Ground-truth sources

| source | what it is | provenance |
|---|---|---|
| `ron-bin/riseofnations.exe` | PE32 i386, image base `0x00400000`, sha256 `30478a44…625079`, a 2024-06-20 MSVC-14 rebuild of the 2003 code | [measured] |
| `ron-data/*.xml` | 49 shipped data files, hash-verified out of the game install | [measured] |
| **`ron-bin/sbl/rise.pdb`** | **the shipped PDB for this exact binary** — 57,290,752 B, GUID `51D4F219-61C6-4F84-9D5B-C3361B0D291F`, age 1, **byte-identical to the CodeView debug-directory entry in `riseofnations.exe`** (`E:\agent\_work\2\s\main\game\rise.pdb`). 37,138 public symbols → `schema/rise-symbols.tsv`; 83 `*Command` struct layouts → `schema/command-structs.txt` | GUID/age match **[measured, this reconciliation]**; that the files are the *shipped* PDBs rather than fetched from a symbol server is **[reported]** by the lane that placed them there |
| live process (Parallels "Windows 11") | `ReadProcessMemory` via P/Invoke; PID 14644, base `0x00D60000`, ASLR delta `+0x00960000` | [measured] |
| `tools/damage-hook/` | inline detour on `ObjectData::get_damage`; 138,909 records over 56,789 real calls | [measured] |
| `crates/donscan` | native typed heap scanner; full 920 MiB scan in 0.45–0.68 s | [measured] |
| `.rcx` replay corpus | ~60 replays on the VM, 2014–2025, two engine builds | [measured] |
| hbox oracle | maps the retail image i686-native and calls its functions | the spine of every Tier-B row |

**What the PDB does and does not do.** It gives *names and struct layouts* straight from the
compiler, which is the strongest structural ground truth available and it independently
corroborates ~25 function identities this project derived the hard way (adler32 at
`0x00A46830`, `Random` at `0x00A39CF0`/`0x00A39D70`, `CheckSums::check_all` at `0x00936560`,
and so on). It **changes no tier**: a name is not a behaviour, and several names are actively
misleading if taken as semantics (see §4.9, `ObjectData::train_time`). It is also **not yet
audited** — no derivation report or adversarial pass covers the PDB extraction, so treat
`schema/rise-symbols.tsv` and `schema/command-structs.txt` as high-confidence but unreviewed.

---

## 1. Implemented mechanics

Every row's implementation path is a file in this repo. Function names in the *source*
column are the shipped PDB's own names where one exists.

| mechanic | source | implementation | tier |
|---|---|---|---|
| `hash_into_range` | `Doober::get_num` `0x00846450` | `don-sim/src/mechanics.rs` | **B** |
| `damage` / `damage_traced` | `ObjectData::get_damage` `0x00644130` | `don-sim/src/mechanics.rs` | **B** (arithmetic chain only) |
| `flank_level` | `flanking(u32,u32)` `0x0092CFE0` | `don-sim/src/mechanics.rs` | **B** |
| `entrench_dir_level` | inlined at `0x00644E0E`, inside `ObjectData::get_damage` | `don-sim/src/mechanics.rs` | **B** |
| `balance_index` | `Balance::return_modifier` `0x00581CA0` | `don-sim/src/mechanics.rs` | **B** (address arithmetic only) |
| `get_attack` / `get_armor` | `ObjectData::attack` `0x006469F0` / `ObjectData::armor` `0x00647DB0` | `don-sim/src/mechanics.rs` | **B** (base path), **unverified** (upgrade path), **incomplete** (§4.4) |
| `as_scaled` / `as_int` / `wtoi` | `String::fraction` `0x00A1D110`; `Constants::get_item` `0x0057FA60` → `String::number` `0x00A1D210` → `String::convert_int` `0x00A15FC0` → `_wtoi` | `don-rules/src/value.rs` | **B**, conditional on two substituted CRT leaves |
| `Rules` typed block | `Constants::init` `0x00569A90`–`0x0057016F` | `don-rules/src/rules.rs` (generated) | **B** for 828 live-confirmed constants; layout structural |
| Economy: tick, caps, ramp, attrition timing | `Leader::do_gather` `0x006CE450`, `Leader::calc_resource_caps` `0x006CE900`, `ObjectData::train_time` `0x006508C0`, `UnitData::get_attrition` `0x00608FD0`, `Unit::process_attrition` `0x005E11A0`, `Unit::process` `0x006117C8`/`0x006115EA`, `TypeData::get_cost` `0x00664090` | `don-sim/src/mechanics.rs` | **C at best** — transcribed, never executed against retail |
| Field-offset table, 1,223 constants | `log_data` walkers across 112 functions | `don-rules/src/offsets.rs` (generated) | **structural** (downgraded from B — §4.10) |
| `RuleValue::parse` | the *prose* of `ron-data/rules.xml`; no engine counterpart | `don-rules/src/value.rs` | **engineering** — documentation/linting layer, never a value source |
| Active-team diplomacy initialization prefix | `Leader::init` `0x006E3C52..0x006E3CB7`; exact `is_team(target,0)` call and true-arm value-2 declaration write | `don-sim/src/systems/leader_init_diplomacy.rs`; integrated by `player_setup.rs` | **C, instruction-derived; no retail execution** — only active team-true cells; option-dependent non-team arm and remainder of `Leader::init` excluded |

### 1.1 `hash_into_range(a, b, lo, hi)` — `Doober::get_num`

| field | value |
|---|---|
| source | `0x00846450`, `__stdcall`, 4 dword args, `ret 0x10`. PDB: `?get_num@Doober@@QAEHVTCoord@@0HH@Z` — `Doober::get_num(TCoord, TCoord, int, int)` [measured, rise.pdb] |
| implementation | `crates/don-sim/src/mechanics.rs::hash_into_range` |
| tier | **B** |
| evidence | 500,008 inputs — 8 hand-chosen edges (`i32::MIN`, `i32::MAX`, zero range, inverted range, negative operands) + 500,000 pseudo-random from a fixed seed — **0 mismatches** |
| harness | `crates/oracle`, `difftest`, i686-unknown-linux-musl on hbox |
| reachability | **dead code in this image**: 0 `E8`/`E9` rel32 targets and 0 occurrences of the LE dword `0x00846450` in any section [measured, independently by the audit] |

Computes `((a*a*b) mod (|hi-lo| + 1)) + lo`, wrapping signed 32-bit, `idiv` truncating toward
zero. Multiplication wraps (`a*a` alone can go negative); the remainder takes the sign of the
dividend, so results can fall *below* `lo`; `hi` need not exceed `lo`.

Two facts that supersede the original entry. **It is not the RNG** — the engine RNG is the LCG
at `Random::get` (§2.1), and this function is called by nothing. **It is now named**: the PDB
says `Doober::get_num`, taking two `TCoord`s, so it is a coordinate-keyed number generator for
"doobers" (RoN's dropped-resource pickups), not a general hash. Its *purpose* remains
underived; the caveat that an unreferenced COMDAT can still be semantically live stands
(`MathUtilFuncSet::rand_int` at `0x009E1890` is also unreferenced and is certainly live).

*Process note, kept because it is the reason a rule exists:* the first unit test carried
hand-computed expectations and one was wrong — `(7,-3,-100,100)` was written as `-47`; retail
returns **`-247`**. Expectations now come from `oracle vectors`. **Capture, do not calculate.**
The audit re-ran `oracle vectors` and confirmed the committed vectors are byte-identical to
retail's output, `-247` included.

### 1.2 `damage()` — `ObjectData::get_damage`, `0x00644130`

| field | value |
|---|---|
| source | `0x00644130`, `__thiscall`, six stack dwords, `ret 0x18`. PDB: `?get_damage@ObjectData@@QBEHHHKHHPAH@Z` [measured, rise.pdb]. Applier is `Object::do_damage` `0x0064A480`; the two live call sites are inside `Object::compare_target` `0x0064E5C0` (at `0x0064EC10`) and inside `do_damage` (at `0x0064A4F7`) |
| implementation | `crates/don-sim/src/mechanics.rs` (`damage`, `damage_traced`) |
| tier | **B** for the arithmetic chain (steps 1–9, 12–26, 28–31). Steps **10, 11, 27 unverified** — never executed. Step 0 transcribed and provably inert. The ~30 object-graph predicates are **inputs**, not derived. |
| evidence | **7,986,695 differential trials against retail machine code, 8 seeds, 0 mismatches, 0 unexpected panics.** Distribution: numerics from a mixture (50% ±100, 25% ±10k, 12.5% full `i32`, 12.5% boundary constants); `balance_pct` uniform `i16` written into the real table and read back by retail's own inlined arithmetic; angles biased onto the eleven classifier boundaries; masks biased onto the fourteen tested bit patterns; type ids drawn to reach the three id-keyed guards; all 26 stubbed predicates fair coins. 10,204 trials (0.13%) excluded as retail `#DE`, 3,101 (0.04%) as balance-write collisions — both counted and printed, neither silent. |
| mutation evidence | **31 of 32 step-level mutations caught**, 7–36,984 mismatches per 49,912 trials. Full table in `docs/derivation/damage-port.md` §4. The unpinned one is step 0 and it is *provably* inert. |
| harness | `crates/oracle` (`damage`), i686-unknown-linux-musl on hbox. `damage_env.rs` fabricates two objects, four vtables, `RULES`, the game object, the player array, the map, both object tables and the city table, so every predicate is a settable dword. |
| caveats | (a) `out_kind` is written by retail and never compared — "0 mismatches" is about the return value only. (b) The harness aliases `[0x00C0AB84]` and `[0x00C0AEC0]`; **live capture refutes that they alias** (§4.3), so the covered input region excludes how retail behaves for building defenders. (c) `attack`/`armor` are modelled as the base getters, which real combat never reaches (§4.4). |

Derivation and the honest gap list: `docs/derivation/damage-port.md`; structure in
`docs/derivation/combat.md`.

**Refuted by measurement:** the community formula "attack × modifiers − armor, floor 1".
Armor is subtracted at step 22 of 31, and moving it to the end of the chain diverges from
retail on **50,164 of 199,629 trials (25.1%)**. The rescale immediately before armor is
`(D+5)/10` — round-half-up for `D ≥ 0` — because attack is stored ×10. The floor of 1 is
conditional on three tests, and step 29 explicitly forces 0 for one type/domain pair. There
is no hidden per-mask modifier *table*: it is an inline chain of compiled multipliers driven
by `obj_masks` bits and virtual predicates. **Live capture independently confirms the floor is
conditional**: negative returns occur in real play (results range `−3 … 40,400`).

### 1.3 `flank_level(angle_delta)` — `flanking(u32, u32)`

| field | value |
|---|---|
| source | `0x0092CFE0`, arg in ECX, 11 instructions, ISLAND. PDB: `?flanking@@YAHKK@Z` [measured, rise.pdb] |
| implementation | `crates/don-sim/src/mechanics.rs::flank_level` |
| tier | **B** |
| evidence | 500,017 inputs standalone — 17 boundary values plus a 500,000-point stride-8191 sweep of the full `u32` domain (8191 coprime with 2³², so every residue class mod 8191 is hit) — 0 mismatches. Reproduced independently by the audit. Additionally pinned *inside* the damage chain: moving either boundary coarsely produced 334 and 237 mismatches per 199,629 trials; the entry guard produced 12. |
| note | Clobbers `EAX`/`ECX`, **preserves `EDX`**, which the caller at `0x00644B3A` depends on. |

The PDB name is a free-function `flanking(unsigned long, unsigned long)` — two arguments,
where the derivation found one in ECX. The second argument is unread on the path the difftest
exercised; that discrepancy is recorded in §5.6 rather than explained away.

### 1.4 `entrench_dir_level`

| field | value |
|---|---|
| source | `0x00644E0E`–`0x00644E2D`, inlined inside `ObjectData::get_damage`; no standalone function |
| implementation | `crates/don-sim/src/mechanics.rs::entrench_dir_level` |
| tier | **B** |
| evidence | ran in 642,609 of 7,986,695 trials; boundary mutation pinned at 7 mismatches / 199,629 |
| finding | It is **not** `flank_level`, despite the shared 0/1/2 shape and `0x40000000` split — the reject test is `(x − 0x2AAAAAAA) > 0xAAAAAAAB`, not `x > 0xD5555555`. They disagree at `x = 0` (2 vs 0). A unit test pins the disagreement; collapsing them into one helper is exactly the tidy invisible error this project dies of. |

### 1.5 `balance_index(atk, def)` — `Balance::return_modifier`

| field | value |
|---|---|
| source | `0x00581CA0`, `__stdcall`, `ret 8`. PDB: `?return_modifier@Balance@@QAEHW4TypeIndex@@0@Z` [measured, rise.pdb]. Identical code inlined at `0x00644178`–`0x0064418E` |
| implementation | `crates/don-sim/src/mechanics.rs::balance_index` |
| tier | **B** for the address arithmetic only |
| evidence | 247,049 inputs against `0x00581CA0` — exhaustive over the 493×493 type domain plus 4,000 out-of-domain rows — 0 mismatches; reproduced by the audit. The inlined copy is independently confirmed by the damage harness: mutating the stride 493→494 produced 136,409 mismatches per 199,629, and sign-flipping the stored value produced 158,447, confirming the stride and the `movsx` from `i16`. |
| contents | **Now captured, not open.** See §4.5: the array is `final_balance_table` at `0x00C12BF4`, and `0x00C06AFC` is a bias-folded base. `schema/live/final-balance-runtime.bin` (486,098 B, MD5 `7471919b6a8ac68e04fbabb256a1118c`) is the live capture: 243,049 `int16`, **0 zeros, no negatives**, min 5, max 2574, 367 distinct values. |
| open | The **type-id domain is wider than 493**: the live damage hook observed defender type ids 521, 522 and 526 (237 records). The stride 493 is Tier-B pinned, so it is the id space, not the stride, that exceeds the assumed `364 units + 129 buildings`. Unresolved — §5.4. |

### 1.6 `get_attack` / `get_armor` — `ObjectData::attack` / `ObjectData::armor`

| field | value |
|---|---|
| source | `0x006469F0` / `0x00647DB0`, `Object` vtable slots `+0x120` / `+0x124`; read `UnitType[+0x1E8]` / `[+0x214]`. PDB: `?attack@ObjectData@@UBEHXZ` / `?armor@ObjectData@@UBEHXZ` [measured, rise.pdb] |
| implementation | `crates/don-sim/src/mechanics.rs` |
| tier | **B** for the base path; **unverified** for the upgrade path; **incomplete as a model of live combat** |
| evidence | called as the real retail functions in all 7,986,695 damage trials. Source-field links pinned by mutation: perturbing `UnitType[+0x1E8]` produced 150,964 mismatches and `UnitType[+0x214]` produced 99,529, per 199,629 trials. Live: the base `attack` returned exactly `UnitType[+0x1E8]` in 56,805/56,805 invocations and base `armor` returned exactly `UnitType[+0x214]` in 24,706/24,706 [measured, damage hook]. |
| gap | The upgrade branch needs `LeaderData::has_tribe_bonus` (`0x006E1370`, arg `0x16`) to return true; the harness sets `game[+0x20] |= 4`, which makes it return false unconditionally. Never executed. In the live match `RULES[+0x8B8]` — the multiplier those paths use — was **1**, so a live sample could not distinguish `+10×level` from `+10×(level×RULES[+0x8B8])` anyway. |
| **incompleteness** | These two are **base-class leaves**. Over 56,789 live damage calls the two in-function vtable calls dispatched to them **zero times**; they went to `UnitData::attack` `0x006103C0`, `UnitData::armor` `0x00610160`, `BuildData::attack` `0x0062E610`, `WallData::armor` `0x0063FA60` [measured, damage hook + `schema/vtables.json` + rise.pdb]. See §4.4. |

### 1.7 `as_scaled` / `as_int` / `wtoi` — the rule-value tokenizer

| field | value |
|---|---|
| source | `0x00A1D110`, `__thiscall`, one dword arg, `ret 4`. PDB: `?fraction@String@@QBEHH@Z` — **`String::fraction(int)`** [measured, rise.pdb]. The scale-1 path is `Constants::get_item` `0x0057FA60` → `String::number` `0x00A1D210` → `String::convert_int` `0x00A15FC0` → `_wtoi`. Array entries go through `String::num` `0x0042D9E0`. Scaled fields go through `Constants::get_fraction` `0x0057F950`. |
| implementation | `crates/don-rules/src/value.rs` (`as_scaled`, `as_int`, `wtoi`) |
| tier | **B**, conditional on two substituted CRT leaves |
| evidence | 202,643 calls against the retail instructions, **0 mismatches**; reproduced independently by the audit. Distribution: exhaustive over the shipped corpus — every `value=`/`entryN=` string in `ron-data/rules.xml` × 3 scales — plus 31 hand-chosen edges and 200,000 generated `num[/den][prose]` strings (`num ∈ [-2000,2000]`, `den ∈ [-200,200]`, fixed seed `0x9E3779B97F4A7C15`); `INT_MIN / -1` excluded as a retail `#DE`. **Additionally:** the Rust port reproduces all **832 recovered shipped slots** exactly (`don-rules` test `engine_tokenizer::reproduces_the_whole_shipped_corpus`), and those expectations were read out of a *running match's* `RULES` object, not computed here. |
| harness | `hbox:~/don-oracle-econ` — maps the retail image, then patches IAT slots `0x00AC54AC` (`_wtoi`) and `0x00AC5434` (`wcschr`) to its own implementations |
| caveat | The Tier-B claim covers the slash search, the 32-bit multiply, the truncating divide and the zero-denominator early-out — **not** `_wtoi` itself, which was substituted. MSVC's behaviour on overflowing digit strings has never been compared; our `wtoi` wraps, and no shipped value comes close. UTF-16-vs-`&str` is identical on every ASCII input, which is all of the shipped corpus. |

Semantics: `(_wtoi(s) * scale) / _wtoi(after the first '/')`, `i32` throughout, wrapping
multiply, `idiv` truncating toward zero, **zero denominator returns 0 before the multiply**,
no denominator limit, trailing prose ignored, no decimal point. `scale` is a **compile-time
property of the field** (100 / 192 / 256 at the 40 `Constants::get_fraction` sites; 1
everywhere else), never of the string. `wcschr` searches the whole string, so a `/` in
trailing prose *would* open a denominator in a scaled field — in shipped data it never does,
but a mod moving such a value into a scaled field would break.

### 1.8 `Rules` — the typed rules block

| field | value |
|---|---|
| source | `Constants::init` `0x00569A90`–`0x0057016F` [PDB name measured]; schema in `docs/derivation/rules-constants.json` |
| implementation | `crates/don-rules/src/rules.rs`, generated by `re/scripts/gen_rules.py` |
| tier | **B** for the 835 distinct recovered slots confirmed against retained live memory; the block layout is structural [measured] |
| evidence | 717 named fields / 835 recovered slots, laid out as the engine lays them out (a flat `[i32; 848]` at `RULES + 0`, with the XML section object immediately after at `Rules + 0xD40`). All 835 now match the retained running-match block byte-for-byte. The old six scale mismatches (`scholar_rate[0..4]`, `caravan_attack_bonus`) are corrected, `scholar_rate[5]` is recovered, and the two former unclassified post-parse transforms are instruction-derived and live-matched. |
| note | 13 dword indices remain in `UNRECOVERED` rather than being filled with plausible values. |
| corroboration | The live damage hook re-read 18 combat-relevant `RULES` offsets out of a *different* match and every one matches the extracted table (`FLANK_BONUS` 50, `CAVALRY_FLANK_BONUS` 40, `VEHICLE_FLANK_BONUS` 33, `ROCKY_MODIFIER` 170, `OVERKILL_DAMAGE` 85, `RIVER_MODIFIER` 512, …) [measured]. |

### 1.9 Economy — resource tick, commerce caps, rate ramp, attrition timing

| field | value |
|---|---|
| source | `Leader::do_gather` `0x006CE450`, `Leader::calc_resource_caps` `0x006CE900`, the rate ramp at `0x00650AB5` inside `ObjectData::train_time` `0x006508C0`, `UnitData::get_attrition` `0x00608FD0` + `Unit::process_attrition` `0x005E11A0` + the timing sites at `0x006117C8` / `0x006115EA` inside `Unit::process`, and the cost-ramp ceiling at `0x006656AA` / `0x00665452` inside `TypeData::get_cost` `0x00664090`. Frame chain: `Game::do_frame` `0x00591EF0` → `Leaders::process_all` `0x006ED2A0` → `Leader::gather` `0x006CE280` → the two above. [all PDB names measured] |
| implementation | `crates/don-sim/src/mechanics.rs`: `resource_tick`, `credit_resource`, `resource_period`, `commerce_cap`, `ramped_rate`, `rate_after_game_option`, `cost_ramp_ceiling`, `clamp_cost_to_ramp_ceiling`, `attrition_interval_scale`, `attrition_period_frames`, `merge_attrition_period`, `attrition_fires`, `attrition_recompute_due` |
| tier | **C at best — transcribed from capstone output, never executed against retail.** No oracle harness exists for any of these. There is no differential evidence and no sample count to quote. **Do not describe it as tested.** |
| evidence | Instruction-level reading of each cited address (`docs/derivation/sim-economy.md`, which re-derived every address rather than transcribing `economy.md`'s prose and found five corrections doing so), plus the shipped constants from the live-validated table. Ghidra's C was used for control-flow shape only and disagreed with the machine code about the ordering in `0x006CE450`, which is why the port follows the disassembly. |
| inputs, not derivations | The composition of gross income and expense (`econ + 0x64` / `+0x7C`), the interest threshold, `LeaderData::get_gather_handicap`'s bonus, every `LeaderData::has_tribe_bonus(…)` gate, the four attrition object-graph predicates, the additive wonder terms on the commerce cap, and the meaning of `x` in the rate ramp. All are function parameters. |
| how to raise it | A `Leader::do_gather` harness — fabricated player array (stride `0x6EEC`), econ block at `+0x6EB8`, game object, `RULES`, stubs for `0x006E33A0`/`0x006E1370`/`0x006D66A0`. Same shape of work `damage_env.rs` already did. |

Load-bearing facts, stated separately because they are easy to assume wrongly:

- **The resource tick runs once per frame per player.** No modulo gate anywhere on the
  `Game::do_frame` chain. The periodicity is entirely in the accumulator: `period =
  GATHER_RATE * 16 = 7200`, credited from `econ + 0x18 + res*4`.
- **15 fps is confirmed in the binary** [measured]: `0x005924CF` divides the frame counter at
  `game + 0x550` by 15 and increments a second counter at `game + 0x560` on a zero remainder.
  This closes the item `AUDIT-replay.md` §F7 correctly marked `[reported]`.
- **The income pipeline has a negative-income early-out and a commerce-cap clamp** that
  `economy.md` §3.2 omitted, and **the 16,000 income ceiling is conditional**, not global:
  `0x006CE62F` and `0x006CE643` jump past it, so knowledge — and any player without property
  `0x16` — never meets it.
- **The Dutch interest clamp shifts only the rule constant**: `limit = (DUTCH_INTEREST_CAP << 4)
  + cap`, not `(cap + DUTCH_INTEREST_CAP) << 4`. A factor of 2.2 on the Dutch economy.
- **`unit + 0x9E` is an attrition *period in frames*, not a damage value**, and
  `UnitData::get_attrition` returns an *interval scale* — which is why it divides by the
  attrition level. Attrition fires when `(frame + unit[0x0A]) % period == 0` and the period is
  recomputed every 32 frames per unit. Decisive witness: the assassin path at `0x005E15DD`
  stores `ASSASSIN_ATTRITION` (`"8 frames"`) into that word unscaled. This resolves
  `economy.md` §5.2's "the returned strength decreases as the level rises" puzzle — it is not
  a strength.
- **The militia and float attrition branches are an if/else, not a sequence**, and the float
  arm carries two further immunity predicates.
- **Knowledge's commerce cap is hardcoded 999** (`0x006CE92C`, `0x1166 ^ 0x1281`) and the
  `jmp` after it skips every civ and wonder bonus.
- **A zero cost-ramp ceiling means *no* ceiling** (`test esi,esi; je`), the opposite of the
  natural reading and exactly what a `min()` would get silently wrong.
- **`game[0x2D] == 8` sets every stockpile to 99999 and skips the tick** — an
  infinite-resources mode. Not implemented; its gate is an underived game-config byte.
- **Player sim state is XOR-obfuscated**: `^0x872` gross, `^0x26076` expense, `^0x1281` cap,
  `^0x3421` accumulator, `^0x90236` displayed income, `^0x8221` stockpile, `^0x63187` age,
  `^0x62766` a second age field. Any state mirroring or checksum work must account for it.
- Only four resource slots are named (`RES_FOOD` 0, `RES_TIMBER` 1, `RES_WEALTH` 2,
  `RES_KNOWLEDGE` 3), each pinned by a per-resource branch. Slots 4 and 5 are deliberately
  unnamed — the wiki's ordering is not evidence.

### 1.9a Gathering occupancy and site capacity

| field | value |
|---|---|
| source | `Build::add_gatherer` `0x0062F640`, `check_gatherers` `0x0062F710`, `remove_gatherer` `0x0062F8D0`, `BuildData::num_gatherers` `0x00630450`, `UnitData::is_gathering_at` `0x00608880`, `BuildTypeData::max_gatherers` `0x0063C430`, `calc_gather` `0x00639E40`, and `GatherOrder::walk_data` `0x00486E60` |
| implementation | `crates/don-sim/src/systems/gathering.rs`; report `docs/mechanics/gathering.md` |
| tier | **C** — instruction/PDB/live-rule derived and covered by exact unit tests, not differentially executed against retail |
| evidence | Owner-local `i16` chain, signed `i8` persistent capacity, assigned/active count modes, UID target identity, 9-byte command, 31-byte order payload, reservation bit `0x1000`, sixteenth-unit worker/scholar gross, and 450-frame remainder credit are recovered. |
| fail-closed boundary | The complete 7,668-byte terrain/type evaluator is not ported. Callers must supply and persist its result; no terrain label, radius, or building-type capacity table is inferred. Non-flat worker choreography and rare-Good spatial targeting remain explicit gaps. |

### 1.9b Held-target attack positioning

| field | value |
|---|---|
| source | `Unit::find_attack_pos` `0x00601280`; unit nearby request at `0x00602AB9..0x00602B31`; building direction/step setup from `0x0060197B`; candidate gates/calls at `0x00602090`, `0x006020E1`, `0x006020FA`, and `0x00602124` |
| implementation | `crates/don-sim/src/systems/attack_position.rs` |
| tier | **C** — instruction-derived and covered by focused Rust transaction tests; not executed against retail |
| covered behavior | Ordinary unit target: exact sixteen-argument `UnitType::find_nearby_spot` tuple, candidate terrain/range recheck, and domain fallback. Building target: initial side, `0x20/0x40/0xC0` step selection, capped angular step, 48-unit snapping, `invalid_loc` → terrain `0x4000` → bitmap collision → ordered collision → RNG call order, strict-low score, and adaptive 100-probe budget. |
| fail-closed boundary | The alternating building-perimeter transition stream remains a mandatory provider. The module rejects contradictory held-target range facts and does not substitute a raster, circle, reflection, or nearest-free-cell heuristic. Earlier activity/duty/retarget arms in the 7,124-byte function are outside this ordinary transaction. |

### 1.10 Field offsets — 1,223 constants

| field | value |
|---|---|
| source | descriptor/visitor binding sites across 112 functions. The PDB names them `log_data` methods — `Constants::log_data` `0x00570170`, `ObjectType::log_data` `0x0065FC00`, `UnitType::log_data` `0x0061C490`, `BuildType::log_data` `0x00631810`, `TechType::log_data` `0x0066D630`, `Type::log_data` `0x006631C0`, `Balance::log_data` `0x00582BD0` [measured, rise.pdb] |
| implementation | `crates/don-rules/src/offsets.rs` (generated by `re/scripts/gen_offsets.py`) |
| tier | **structural** — downgraded from B, see §4.10 |
| evidence | `RECHARGE`=500, `CREW_SIZE`=780, `BASE_FORM`=784 agree between instruction-level extraction and Ghidra decompiled C. **No retail code was executed**, and per the charter decompiled C is a hypothesis, not a second derivation. |
| independent corroboration | the offsets are real record fields, confirmed elsewhere at scale: 15,244 of 15,247 runtime scalar cells agree with `ron-data/*.xml` when read through these offsets out of a live process (99.980%), and all three mismatch classes are explained (§3.5). |

### 1.11 `RuleValue::parse` — the prose layer

Not a mechanic. It tokenizes the *English* of `ron-data/rules.xml` (rational, percent sign,
unit word, trailing prose) and its `percent`/`unit` fields describe the text, not engine
behaviour — the engine reads neither. **No computed value may depend on it.** A test pins the
two layers deliberately disagreeing on `"1.5"`: the engine truncates at the decimal point,
`RuleValue` does not. Kept for documentation and linting. Tier: **engineering**.

---

## 2. Derived and tiered, but NOT implemented in this repo

These carry real Tier-B evidence. **None of them exists as Rust in `crates/`.** Anyone
implementing one must port the exact model the harness tested, not the prose.

### 2.1 `Random::get()` / `Random::get(lo, hi)` — the engine RNG

| field | value |
|---|---|
| source | `0x00A39CF0` `?get@Random@@QAEMXZ` (returns f32) and `0x00A39D70` `?get@Random@@QAEHHH@Z` (`__thiscall`, 2 stdcall dwords, `ret 8`); `Random::reseed` `0x00A39D30`; the whole object is one `u32`. [PDB names measured] |
| implementation | **none in this repo.** Model exists only in `crates/oracle/src/bin/rng.rs` (the harness) and as source in `docs/derivation/rng.md` §"For whoever owns mechanics.rs" |
| tier | **B** |
| evidence | `next_float`: **999,950** chained trials — 70 seeds (6 hand-chosen + 64 xorshift64), each walked 14,285 chained steps, comparing **both** resulting state and the `xmm0` f32 **bit pattern** — 0 mismatches. `in_range`: **1,000,012** trials with bounds inside ±0xFFFF plus **500,000** with bounds to ±2^29 (warning byte pre-set), comparing both return value and resulting state — 0 mismatches. All reproduced independently by the audit. |
| harness | `crates/oracle/src/bin/rng.rs`, i686-unknown-linux-musl on hbox |

`s ← s·1664525 + 1013904223` (`0x19660D` / `0x3C6EF35F`, both read out of the image by the
audit). Semantics that are easy to get wrong and are all measured: the range is **half-open
`[lo, hi)`** (confirmed by a 200,000-draw histogram, bucket 10 empty for `in_range(0,10)`);
**`lo == hi` returns `lo` and does *not* advance the state** — a determinism hazard that
desynchronises the stream, not just one draw; `lo > hi` is silently swapped *and* the state
advances; only the low 16 bits of the new state feed the mapping, so bounds above `0xFFFF`
overflow the `imul` and stop being uniform (the engine itself warns about this). `0x00A39D70`
sets up a Windows SEH frame — any oracle work on non-leaf sim functions needs the
`install_fake_teb()` fix (that fix is **engineering**, not a mechanic; see §4.11).

**Seeding is deterministic per match.** `game_seed` is a Steam lobby key run through `strtol`;
`Map::make` (`0x0068BC90`, vtable slot 25) writes it to both `Game+0x7C` and the live state;
map generation derives sub-seeds by multiplying the *record* by 19/29/37; saves and scenarios
restore the state word. The only clock-seeded path is opt-in and script-visible
(`rand_seed(n < 0)` → `timeGetTime()`). There is no `rand`/`srand` in the import table at all.

**MT19937 is present but used exactly once**, default-seeded (`init_genrand(5489)`), for one
`std::shuffle` of up to 8 player-array entries in `game.cpp`. Deterministic, but not the
general RNG and not seeded from `game_seed`.

### 2.2 `vector_dist(a, b)` — the engine's integer distance kernel

| field | value |
|---|---|
| source | `0x0046CFF0`, `__fastcall(ecx, edx)`, 105 bytes, ISLAND, called from 105 distinct functions engine-wide. PDB: `?vector_dist@@YAHHH@Z` [measured] |
| implementation | **none in this repo** |
| tier | **B** |
| evidence | **4,000,026 inputs, 0 mismatches** — 26 hand-chosen edges (`0`, `±1`, the `0xEA60` guard from both sides, `i32::MIN`/`i32::MAX` in every combination) + 2,000,000 uniform 32-bit pairs + 1,000,000 in ±65536 + 1,000,000 straddling the 60000 guard. Seeded xorshift64, fixed seed `0x2545F4914F6CDD1D`. Reproduced independently by the audit. |
| harness | `hbox:~/lane-pathfinding` — **not in this repo**, which is a reproducibility gap (§7.4) |

`max + min²/(2·max)` with an unsigned fallback `(min + 2·max) >> 1` above `min ≥ 60000`. No
`sqrt`, no float. Traps a careless port falls into, all exercised: the `hi > lo` compare is
**signed** while the `0xEA60` guard is **unsigned**; the divide is unsigned `div`, not `idiv`;
the `>> 1` is `shr`, not `sar`; `abs` is the wrapping kind, so `i32::MIN` stays `i32::MIN`.

### 2.3 `adler32(adler, buf, len)` — the lockstep checksum primitive

| field | value |
|---|---|
| source | `0x00A46830`, `__fastcall` (ECX = running sum, EDX = buffer, one stack dword = length). PDB: `?adler32@@YAKKPBEK@Z` [measured] |
| implementation | **none in this repo** |
| tier | **B** |
| evidence | **500,000 cases, 0 mismatches, 0 faults** (an earlier 40,000-case run also gave 0/0); both reproduced independently by the audit. Distribution: 12 hand-chosen lengths straddling both structural boundaries (`0,1,2,15,16,17,31,5551,5552,5553,11104,11105`), then uniform lengths in `[0, 24576]` with uniform random bytes and a uniform random 32-bit initial value, fixed xorshift seed `0x9E3779B97F4A7C15`, plus the `buf == NULL` case (returns 1, as the disassembly predicts). |
| harness | `hbox:~/don-oracle-checksum`, `oracle adler` — **not in this repo** (§7.4) |

Plain zlib adler-32: `NMAX 5552`, DO16 unrolled, `% 65521` by the `0x80078071` magic. Called
from only 8 sites in the whole image — 6 inlined `CheckSum` fast paths, 2 in the
save/load "Incorrect Load Checksum" path. Same primitive for saves and for desync detection.

---

## 3. Structural results — measured, no behavioural test

Real knowledge, no fidelity claim. Nothing here is Tier A/B/C.

### 3.1 The lockstep checksum defines sim-critical state

`CheckSum`, `SaveGame` and `LoadGame` are sibling subclasses of one two-method pure-virtual
interface `DataWalk`, from the RTTI class-hierarchy descriptors (1,510 hierarchies parsed;
exactly six classes derive from `DataWalk`). **So sim-critical state ≡ save-game state.**

PDB names for the two virtuals, which supersede the derivation's guesses: slot 0 is
`walk_function(void*, void*)` (`CheckSum::walk_function` `0x00936FF0`, `SaveGame::walk_function`
`0x0043D730`, `LoadGame::walk_function` `0x0043D950`) and slot 1 is `walk_test(const String&)`
(`CheckSum::walk_test` `0x0041BFE0`, a bare `ret 4` no-op). `checksum.md` called them
`walk(begin,end)` and `walk_tag(p)`.

`CheckSums::check_all` `0x00936560` walks fifteen channels in a fixed order — `units, builds,
walls, ammo, deaths, groups, guys, leaders, cities, items, goods, world, rules, scenario_data,
script_run_time` — then a `total` that is a plain **wrapping 32-bit sum**, not a hash of
hashes. Confirmed exactly, including the two easy-to-miss facts: the `leaders` channel covers
**8** leaders while the object channels cover **9**, and `check_all` returns the *last*
channel's value, not the total. The `DataWalk+0x0C` section mask gates optional sub-walks, so
"the checksum covers X" is mask-dependent.

`docs/derivation/extract_datawalk.py` yields 106 functions with 183 `this`-relative byte
ranges; only the 8 anchored to vtable slot `+0x7C` are reported as measured, and ~60 of the
106 have bad `this` taint. `LeaderData::walk_data` `0x006D6750` walks 26,914 contiguous bytes
of a 28,396-byte leader record — essentially the whole player block is sim-critical.

Desync tracking is file-configured: `.\synclogger.ini`, section `[Settings]`, keys
`DesyncTrackingEnabled`, `CheckDesyncsEveryXFrames`, `DesyncUploadsWanted`,
`DesyncTrackingFrameHistorySize`, `SkipCountdown`, read by `SyncLogger::setupWithConfigSettings`
`0x00A30080` and `SyncLogger::reportSettingsAndOptions` `0x00A2E880` [PDB names measured].
38 sync categories at `0x00C06370` are a **superset** of the checksum channels — `Terrain`,
`Pathfinder`, `Animals`, `Sound`, `GraphicChads` have categories but no `check_all` channel, so
"has a sync category" is *not* the sim/presentation line; the fifteen `check_all` channels are.

### 3.2 The `.rcx` replay format

A `.rcx` is **one gzip member from offset 0** (the game writes uncompressed, then memory-maps
and streams through gzwrite into `recordgame.tmp` and renames — `RecordGame::finalize`
`0x00952B40`). Decompressed: a `SaveGame`-serialised header (`Game::walk_data` `0x00589600` →
`GameInfo::walk_data` `0x005D6570`), a ~1 MB fixed-size state blob, then command-package
records to the last byte.

**The on-disk record header is settled** — from the writer `RecordGame::write_package`
`0x00952FB0`, the reader `RecordGame::read_package` `0x00952D90`, the `GameLog` name bindings
in `CommandPackage::log_data` `0x0094B7C0`, **and now the PDB's own `CommandPackage` layout**:

```
struct CommandPackage  // sizeof = 536 (0x218)
    +0x00 unsigned long stamp     +0x0c int group      +0x214 Random padding
    +0x04 int play                +0x10 short size
    +0x08 int valid               +0x12 unsigned char data[512]
```

and the writer emits, in this order: `[[0x00C061EC]+0x550]` (the **game frame**, a global that
is not a package field at all), `pkg+0x04 play`, `pkg+0x08 valid`, `pkg+0x00 stamp`,
`pkg+0x10 size` (u16), then `size` payload bytes. **18-byte header.** Three lanes gave three
different names for these fields and two were wrong — see §4.7.

The dispatcher `CommandPackage::process` `0x0094A700` is a dense switch over **82 opcodes
`0x00`–`0x51`**, each handler returning the bytes consumed. The full table is in
`docs/derivation/replay-io.md` §5 and `replay-stream.md` §3, and `schema/command-structs.txt`
now carries the PDB's own layouts for 83 `*Command` structs, which confirms every size the
lanes recovered and settles the three variable-length forms (`GroupCommand` `3 + 2*num`,
`SplineCommand` `6 + 8*len`, `ChatCommand` `0x13 + 2*len`) and the disputed `TurnDataCommand`
(**11 bytes**, five `unsigned short` — the "sometimes 12" was the padding of §3.3, not a
mis-read field).

Parser: `re/scripts/rcx_parse.py`. Tier **C**, corpus-measured. Note the audit's warning: "the
stream parses exactly to EOF with zero residue" is **nearly worthless as evidence** — of 8,192
candidate start offsets in a 0x2000 window of `today.rcx`, **7,440** produce an 18-byte-header
chain terminating exactly on the last byte. The framing is self-synchronising; landing on EOF
is the default outcome. The framing is settled by the *disassembly*, not by the parse.

### 3.3 Multiplayer replay decoding — closed

`CommandPackage::process_all` `0x0094C500`, gated on `game[0x820] & 4` (the network-game flag):

- **XOR key** `K = (u16)(game[0x10] >> 8)`, applied to `size/2` u16 words from `pkg+0x12`.
- **`game[0x10]` is `GameInfo+0x04`**, which sits in the replay's own plaintext header.
- **Inter-command padding** is `in_range(0, 2)` drawn from a `Random` that is a **member of the
  `CommandPackage` itself** (`+0x214`, named `padding` in the PDB — which independently
  confirms `rng.md` §4's otherwise-unattributed `[esi/ebx+0x214]` stream, 5 sites in
  `commandpackage.cpp`), seeded once per `process_all` call from `game[0x10]`. It is **not**
  the shared sim `Random`, so the padding sequence restarts identically at every package and is
  fully reproducible offline.

Result: **25,279 of 25,279 command packages decoded exactly, zero residue, zero undecodable
commands, 98,117 commands, across 4 specimens and 2 engine builds** [measured, audit]. Tier
**C**.

An **un-scrambled fast path** at `0x0094C5A2` (when `(game[0x820] & 0x14) == 0x14`,
`!(game[0x823] & 2)`, `size != 0`, `data[0] == 0x50`, `size <= 4`) never fired in the corpus.
Any decoder that assumes universal scrambling has a latent bug there.

Method caution worth keeping: brute-forcing the key found **1,280–4,864** seeds consistent with
a 30-package sample per file. Only `GameInfo+0x04` survives the whole file. A search that stops
at the first consistent candidate ships a wrong constant.

### 3.4 The per-turn checksum oracle in multiplayer replays

`CommandManager::issue_check_sums` `0x00940770` [PDB name measured] builds a
`CheckSumsCommand`, opcode `0x39`. It has exactly one caller (`0x0093F2E9`, in the
`PROCESS_TURN` path) and **no frame divisor** — all gating is `game[0x820] & 0x04` set and
`& 0x10` clear, plus per-player bits. The same bit gates the XOR scrambling, which is why MP
replays are both scrambled and checksummed and solo replays are neither.

**`CheckSumsCommand` is `0x41` = 65 bytes**, not `0x3d` = 61. Confirmed four ways: `push 0x41`
at the send site `0x009409F8`, `mov eax, 0x41` / `ret 4` at `0x00945E0E` in the handler, the
corpus, and now the PDB type record — `sizeof = 65`, with the sixteenth `unsigned long` at
`+0x3d` named **`all_checksum`**. See §4.6.

Corpus results, keeping the two independent measurements separate:

| | `replay-checksum` lane [reported] | audit, own decoder [measured] |
|---|---|---|
| corpus | 3 MP replays | 4 MP specimens, 2 engine builds |
| `check_sums` packets | 54,947 | 25,271 of 25,279 packages |
| `total == Σ(15) mod 2³²` | 54,947 / 54,947 | 25,271 / 25,271 |
| 15 real channels adler-32-shaped | — | **379,065 / 379,065** |
| turns with ≥2 reporters, divergent | 27,473 / **0** | 12,589 / **0** |
| `rules` channel | `0x12ba3104`, constant | `0x12ba3104`, constant across both builds |

`rules = 0x12ba3104` is a free, concrete target: a Rust `DataWalk` over our loaded rule set
must produce that adler-32 at `Game::walk_rules_data` `0x00589550`'s traversal. Tier **C**
(corpus observation) — *not* Tier B; no replay lane executed shipped code (§4.8).

### 3.5 Live runtime type tables

Ten global `PtrArray<T>` objects, all `count == capacity == 806`, indexed by a single global
type-id space with `NULL` in slots of other classes: `UnitType` `0x00C0A264` (364 records, ids
50–413), `BuildType` `0x00C0AA90` (129, 414–542), `TechType` `0x00C0AAAC` (85, 544–628),
`ItemType` `0x00C0AAC8` (1), `ObjectType` `0x00C0AAE4` (544), `SpellType` `0x00C0AB00` (55),
`BonusType` `0x00C0AB1C` (122), `GovType` `0x00C0AB38` (0), `TypeBak` `0x00C0AB54` (566),
`GoodType` `0x00C096E4` (50). Record layouts transcribed from the `log_data` walkers and
confirmed against live bytes and the XML: **15,244 of 15,247 scalar cells agree (99.980%)**.

Scale factors *fitted from the live/XML ratio*, not assumed: `ATTACK` ×10 (364/364 units,
129/129 buildings), `GUY_SPACING`/`X_SPACING`/`Y_SPACING` ×12, `BLOCK_RADIUS`→`block_radius`
×48, `TARGET_SIZE` ×48, `RESEARCH_PREMIUM_*` ×256, everything else ×1.
`hits` and `armor` are **`int32`, not `int16`**.

Three mismatch classes, all explained and all findings rather than noise:

- **Consecutive equal-`NAME` pass-2 source reuse.** `Types::init`
  `0x0066B620..0x0066B647` retains the first XML element and TypeIndex in a consecutive
  equal-name run for passes 2 through 4. German `RIFLEMENGERMAN` therefore reads scalar
  `ARMOR 3` from `RIFLEMEN`, not its own XML `ARMOR 1`. `GRAFT` resolves independently in
  pass 1 and does not cause this scalar reuse. `UnitRuntimeCatalog` reproduces the recovered
  scalar tranche for all 364 captured live rows. Fidelity tier C: static instruction
  recovery plus captured live-state comparison, not a retail differential.
- **`RANGE` zeroed on crew-carrier units** (the three Mahouts) — hypothesis, not measured.
- **`GRID_X` reassigned** for three units — unexplained.

Three formerly opaque fields now have loader rules. `CIRCLE_RADIUS` stores directly into
`x_size`/`y_size`; `PUSH_SIZE` is clamped to 1..100 and scaled by `UNIT_BLOCK_RADIUS`, except
that a clamped `PUSH_CIRCLES == 1` selects the row's `BLOCK_RADIUS` before scaling; and
`PREQ1` is later synthesized from `military_level` for most units. Do not treat any of them
as a raw field-to-offset copy.

Live heap census via `donscan`: **the entity classes are fixed-size preallocated pools, not
live counts.** `Unit` 600, `Build` 601, `Animal` 400, `Ammo` 400, `City` 160 were bit-identical
across two scans 25 s apart during live play, with 97.5%/95.7%/94.0%/91.2%/99.4% single-stride
address gaps. `Guy` and `Group` and the `Order` classes move and are plausibly live. This is
the number that would have been believed as "47 live Units" and quietly poisoned everything
downstream. Struct facts from address geometry, zero orphans: `OrderList` embedded at
`Unit+0xC8` (600/600) and `Animal+0xC8` (400/400); `MiningList` at `Build+0x98` (600/600);
`GatherPointList` at `Build+0xB8` (600/615).

### 3.6 Pathfinding structure — scoped to the road/caravan search

**Scope correction, and it is large.** The PDB names the functions the pathfinding lane
derived: `0x00685990` is `PathFinder::astar_caravan_road`, `0x00686300` is
`PathFinder::calc_road_cost`, `0x00688740` is `PathFinderData::valid_roadcoord`. The lane
called them "the A\* driver", "the per-edge cost" and "step legality" and generalised its
headline to "Rise of Nations' pathfinder". **What was derived is the caravan/road pathfinder**,
which is one entry point among several (`0x00688A40`, `0x00688FC0`, `0x006897D0` are unread).
See §4.12.

What stands, scoped to that search [measured]: pure-integer 8-connected grid A\* over the tile
grid; 36-byte nodes from a LIFO free-list pool; a red-black-tree open list keyed on `f` plus
two cell-id indexes, so **no hash map and no pointer-ordered container anywhere in the
187-function cone** — pathfinding introduces no container-order nondeterminism; a fixed
neighbour rotation seeded by the cardinal pointing at the goal, so tie-breaking is fully
determined; a 3,200-expansion budget with searches parked in per-slot records and resumed
across frames; the eight cost constants `55, 100, 100, 540, 240, 200, 60, 600` (+8) read out of
`.rdata 0x00B69A30`/`0x00B69A60` and installed by `PathFinder::init` `0x00689EC0` — the audit
re-read those bytes itself; the diagonal multiplier is exactly **7/5**, not `sqrt(2)`; and a
**retail heuristic bug worth replicating** — the mode-A child heuristic passes `-goalY` rather
than `child.y - goalY`, which makes `h` a large near-constant and reduces mode A to
approximately plain Dijkstra. That the instructions are these is [measured]; that it is a *bug*
is inference.

**Coordinate system, settled** [measured, two independent derivations]: `init_coord_lookup_array`
`0x00681DB0` builds `T[i] = i/3` at `[0x00CAE5FC]`; `T[pos >> 6] = pos/192` is the tile index
and `T[pos >> 8] = pos/768` the 4×4-block index. **1 tile = 192 world units, positions are
`int32`**, corroborated by `rules.xml`'s own header (`UNIT_MOVE_SPEED = "1/192 tile"`, largest
denominator 192) and by the bounds check against `192 * mapWidthTiles`.

**Object position is XOR-obfuscated**: `SubObject::set_new_location` `0x00662680` [PDB name;
the lane called it `Object::setPosition`] stores `x ^ 0x00063637` at `+0x10`, `y` at `+0x14`,
and a **derived** `z` at `+0x0C` — Z is re-sampled from the heightmap on every position write,
never integrated. The immediate `0x00063637` appears 2,548 times in `.text`, so it is a
compile-time constant, not a per-run cookie.

**The pathfinder draws from the main simulation RNG once per edge relaxation**
(`calc_road_cost` calls `Random::get(0, 0xFFFF)` on `[0x00C06184]` and takes `% 20`). See §4.1
— an earlier ledger claim that this is a *separate* stream was wrong, and the blast-radius
warning stands.

### 3.7 Live damage capture

| field | value |
|---|---|
| source | live `riseofnations.exe` PID 14644, runtime base `0x00D60000` |
| instrument | `tools/damage-hook/donhook.c` — inline detour, 6 stolen bytes at `0x00644130`, ≤230 ns/call |
| tier | **C** — behavioural capture. Nothing is compared; this is not a differential test |
| evidence | 56,789 damage calls / 138,909 records, frames 20,338–28,738, `dropped=0`, `forced=9` (0.0065%); all seven patched sites verified byte-identical to the shipped image after removal |
| instrument validation | 536,354 calls on an ABI-matched self-started target, 0 divergence; 43,187 captured records re-checked offline against a closed form, 0 mismatches |
| caveats | one match, one ruleset, one map; 25 attacker types, 50 defender types, 242 pairs; **no air combat**; predicates unobserved; `out_kind` value not read |

**There is deliberately no agreement rate against `mechanics.rs::damage`.** The capture
supplies most of `DamageInput` and all of `CombatRules`, and **none of the 30
`DamagePredicates`** — every one is the result of a virtual call made *inside* the function. A
2³⁰ search per record with a second transcription to drive it is precisely the shape of error
this project exists to avoid, so it was not built. The honest description is: ground truth for
the output and most arithmetic inputs, and no observation at all of the branch vector.

What live play actually exercises, which is close to the *inverse* of the oracle's coverage:
`splash_flag` non-zero in **28 of 56,780** calls (0.05%) against 3,994,983 oracle trials
through those steps; domains 0 and 1 only.

---

## 4. Corrected and refuted

Each item states the **wrong claim** so nobody re-derives it.

### 4.1 `[0x00C06184]` is the *main* simulation RNG, not a separate pathfinder stream

**Wrong claim (this ledger's own previous "Streams — pathfinding has its own RNG" section, and
`README-LLM.md`):** "`[0x00C06184]` → pathfinder `Random`, heap, populated at the menu;
`0x00EB697C` is the script/sim `Random`. So a pathfinding divergence does not automatically
desynchronise the scripted/sim stream — the pathfinding lane's headline overstated the blast
radius."

**Refuted [measured, this reconciliation].** The live value `0x01797A8C` is not a heap
allocation: the static file bytes at `0x00C06184` are `8c 7a e3 00`, i.e. the pointer holds
`0x00E37A8C`, and `0x00E37A8C + 0x00960000` (the PID-14644 ASLR delta) `= 0x01797A8C` exactly.
The live read observed the rebased **static** object, which is why it is populated at the menu.

The object at `0x00E37A8C` is the **one main simulation stream**, shared by:

- the road/caravan pathfinder's per-edge cost draw (`0x00686341`),
- map generation (24 of 93 callers are `Map` subclass virtuals) and `TerrainGroups.cpp`,
- units and animals (`0x005D7460`, `0x005D79E0`, `0x0060EE50`),
- **the BHS script API** — `MathUtilFuncSet::rand_int` `0x009E1890` is
  `mov ecx, [0xC06184]; call 0xA39D70`, verified here instruction by instruction.

`0x00EB697C` is a *different* stream (69 sites: `Surf.cpp` water, `Scene`, graphics, one
`Animal`), reached through the `__fastcall` wrapper `0x00A39D40` which does
`mov ecx, 0xEB697C`. Its going from 0 at the menu to non-zero in a match is consistent with any
per-match stream, presentation included, and is **not** evidence that it is the script/sim one.

**Consequence: the pathfinding lane's determinism warning stands in full.** Any divergence in
how many edges we relax desynchronises every later draw on the main stream, including scripts.
`README-LLM.md` carries the same error and needs the same correction.

### 4.2 `cavalry_flank_bonus` / `vehicle_flank_bonus` are plain integers, not 1/256 fixed point

**Wrong claim (`combat.md` §8, and still live in `crates/don-sim/src/mechanics.rs:158-161`):**
`CAVALRY_FLANK_BONUS` `+0x50` and `VEHICLE_FLANK_BONUS` `+0x54` are "1/256 fixed point" /
"8.8 fixed point".

**Refuted [measured, audit].** The loader binds both with the **plain `_wtoi` binder**
`Constants::get_item` `0x0057FA60` with **no scale pushed**, at `0x00569DD3` and `0x00569DEB`,
while the *neighbouring* offsets `+0x58`/`+0x60`/`+0x64`/`+0x68`/`+0x6C` all `push 0x100` into
`Constants::get_fraction` `0x0057F950`. `rules-constants.json` agrees independently
(`parser=wtoi scale=None stored=40` and `stored=33`), and the **live `RULES` read from the
damage hook confirms the stored values are 40 and 33** — not 10240 and 8448.

The *use site* still divides by 256 (`0x00644B52`–`0x00644B5F`), and that arithmetic is
Tier-B-pinned (the step-20 mutation caught 688 mismatches per 49,912). Composing two measured
facts: the per-level cavalry flank percentage the engine actually uses is `(40 × 50)/256 = **7**`
and vehicle is `(33 × 50)/256 = **6**`, against the 20 and 16 the XML prose ("40% of base flank
bonus") implies. **That is what the code does.** Whether it is a genuine RoN bug is not
established, and nobody has confirmed the ~7% end-to-end in play — see §5.3.

`combat.md` §8's other ten rows were spot-checked by the audit and are correct.

### 4.3 `[0x00C0AB84]` and `[0x00C0AEC0]` do **not** alias

**Wrong claim (`damage-port.md` §5.6, and this ledger's previous damage caveat):** the two
object tables are *assumed* to resolve to the same object; the harness points both at one array.

**Refuted [measured, live damage hook].** Over 56,780 calls they agree **32,074 times (56.5%)**
and differ **24,706 times (43.5%)**, and the split is structural, not statistical: they agree
on **every** defender with `def_type_id < 364` (units) and differ on **every** defender with
`def_type_id ≥ 364` (buildings). In 24,198 of the disagreements table B holds `0xFFFFFFFF` — a
sentinel, so retail cannot be dereferencing it; the reads through table B sit behind
`attacker_masks & 0x40000` and two virtual predicates that must be closed for building
defenders.

Two consequences pointing opposite ways, and both matter: for **building defenders the port's
model of where `defender_type_0x2b8_bit2` and part of `defender_flags_0x68` come from is
wrong** and cannot be right by luck; and the harness, by aliasing, explores a state retail may
never reach and **cannot test the guard that stops retail dereferencing `-1`**. This does not
disturb the 7,986,695-trial result — it bounds which inputs that result covers.

Whether table B is a units-only table, a shorter allocation read past its end, or something
else, is still open.

### 4.4 `get_attack` / `get_armor` are base-class leaves that live combat never reaches

**Wrong claim (`mechanics.rs` field docs on `DamageInput::attack` / `::armor`):** "`attacker->vtbl[0x120]()`,
i.e. `get_attack`" / "`defender->vtbl[0x124]()`, i.e. `get_armor`".

**Refuted as a model of live play [measured, damage hook + `schema/vtables.json` + rise.pdb].**
Over 56,789 damage calls the two in-function vtable calls dispatched to `0x006469F0` /
`0x00647DB0` **zero times**. The real dispatch:

| class | vtbl `+0x120` (attack) | vtbl `+0x124` (armor) |
|---|---|---|
| `Unit`/`UnitData`/`UnitOut`/`Animal*` | `UnitData::attack` `0x006103C0` | `UnitData::armor` `0x00610160` |
| `Build`/`BuildData`/`BuildOut` | `BuildData::attack` `0x0062E610` | `WallData::armor` `0x0063FA60` |
| `Wall`/`WallData`/`WallOut` | `ObjectData::attack` `0x006469F0` (the base) | `WallData::armor` `0x0063FA60` |

`UnitData::armor` does not call the base at all — it reads `[this+0x9C]` and `[this+0xA]`
behind two virtual gates, which is exactly why unit defenders produced no base-`armor` records
while building defenders produced 24,711. The Tier-B claim for the two base functions stands
(the harness drove the real retail code); **modelling the chain's first operands as those base
functions is incomplete**, and the missing piece is four override functions that are already
written into `tools/damage-hook/donhook2.cfg` and only need a running match.

### 4.5 The balance table's base is bias-folded; `schema/live/balance-runtime.bin` is wrong

**Wrong claim (`combat.md` §5, and this ledger's previous caveat):** a dense `int16[493][493]`
starts at `0x00C06AFC`, whose extent collides with other globals and whose captured window has
"unexplained negatives" — extent unresolved.

**Resolved [measured, live-tables].** The array is named by its own walker
`Balance::log_data` `0x00582BD0` as `final_balance_table`, running `0x00C12BF4 …
0x00C896C6` = **486,098 bytes = 493 × 493 × 2** exactly. And
`0x00C12BF4 − 0x00C06AFC = 49,400 = 2 × (50 × 493 + 50)`: unit ids start at **50**, so MSVC
folded the `−(50·493 + 50)` bias into the base address. The real indexing is

```
final_balance_table[(attacker_id − 50) * 493 + (defender_id − 50)]      ids 50 … 542
```

and `542 − 50 + 1 = 493 = 364 units + 129 buildings`. Semantic spot-checks confirm it
(Pikemen→Knight 166, Knight→Pikemen 100, Catapult→Small City 430, Citizen→Citizen 100); under
the unbiased reading the same lookups give 75/170/100/1075, which is nonsense.

`schema/live/balance-runtime.bin` was captured **49,400 bytes too early** and runs through
unrelated `.data` (it contains the rebased `PtrArray<UnitType>::vftable` and the ASCII
`"errain art"`). **That is where the negatives and the 15,477 zeros came from. Do not use it.**
The correct capture is `schema/live/final-balance-runtime.bin`. The live damage hook
independently found no negatives in any of the 242 cells real combat touched.

The table is `final_` because it is **derived at load**: `ron-data/balance.xml` is a 291×291
name-keyed matrix, only 27.6% of whose comparable cells appear verbatim; its last 55 row keys
are class names (`SIEGE`, `FORTS`, `Flag_3_OBJMASK_AIR`, …) composed into the id space at load.
Deriving that composition is a separate lane; the captured table is ground truth meanwhile.

### 4.6 `CheckSumsCommand` is 65 bytes, not 61

**Wrong claim (`checksum.md` §4):** "total size `0x3d` = 61 bytes", fifteen `u32` after a
one-byte header.

**Refuted [measured, four independent ways].** `push 0x41` at the send site `0x009409F8`;
`mov eax, 0x41` / `ret 4` at `0x00945E0E` in `CommandPackage::process_check_sums`; the
`total == Σ(15)` relation holding across the entire corpus; and the PDB's own type record —
`struct CheckSumsCommand // sizeof = 65 (0x41)` with a sixteenth `unsigned long` at `+0x3d`
named **`all_checksum`**. `checksum.md` §4 must be corrected.

### 4.7 The replay record header — three lanes, three names, two wrong

**Wrong claims:** `replay-stream` `[stamp][from][?][serial]`; `replay-checksum`
`stamp, from, ?, turn` **plus** "the record layout *is* the in-memory `CommandPackage` layout".

**Settled.** Disk `+0x00` is the **game frame** (`[[0x00C061EC]+0x550]`, a *global*, which
refutes the "record layout is the struct layout" claim at `0x00952FB3` where it is the first
thing written); `+0x04` is `play`; `+0x08` is `valid`; `+0x0C` is `stamp`, a per-player package
serial; `+0x10` is `size`. `replay-io` had this exactly right. Confirmed by the writer, the
reader, `CommandPackage::log_data`'s name bindings, and the PDB struct (§3.2).

This is not cosmetic: a `don-replay` crate written from `replay-checksum`'s table would label
the game frame `stamp` and the package serial `turn`, and its "turn" would reach 10,544 in a
10,500-frame game.

### 4.8 Replay lane tier inflation

- **A `"tier: A"` label** was attached in a structured summary to *"The `CheckSumsCommand` is
  `0x41` = 65 bytes"*, with an evidence string that contradicted its own label. **Nothing in any
  replay lane is Tier A.** Charter Tier A is an SMT equivalence over the entire input domain.
- **`replay-checksum` labelled corpus statistics Tier B throughout** ("MP replays embed a
  `check_sums` command in every package — B"; "`rules` = `0x12ba3104` — B"). Tier B is *our Rust
  agreeing with shipped code on N generated inputs*. **No replay lane executed shipped code at
  all.** Reading N files is Tier C / [measured] observation. Four rows affected; corrected in
  §3.4.
- **`rng` lane**: `install_fake_teb()` was submitted as `tier: B` with evidence "1,500,012 calls
  with 0 faults". A TEB fix is oracle plumbing, not a mechanic, and "did not crash" is a smoke
  test. Relabelled **engineering** (§4.11). The cosmetic-stream attribution was submitted as
  Tier C when no behaviour was observed and no divergence measured; it is **structural**, by a
  proximity heuristic over `__FILE__` markers, and `rng.md`'s own prose says so correctly.

### 4.9 `FUN_00570170` is not the rules loader — and other name corrections

**Wrong claim (`docs/binary-ground-truth.md`, and this ledger before reconciliation):**
`FUN_00570170` is "the rules.xml constants loader".

**Refuted [measured, economy lane; confirmed by the PDB].** Every binding site pushes the field
**by value** (`0x00573C69 push dword ptr [edi+0x27C]`) — a loader cannot write through a copy —
and its visitor devirtualises against the `GameLog` vtable. The PDB settles it:
`0x00570170` is `Constants::log_data(Log*)`. The **loader** is `Constants::init` `0x00569A90`.
So `schema/bindings.json` is a name→offset schema recovered from the *log/desync dump walker*,
which is still correct and load-bearing — and explains why XML tag names never appear in
`.text` (element names are `*(0x00C06378) + 0x10 + K`, from a runtime data file, and array
attribute names are built at runtime as `"entry" + i`).

Names corrected by the PDB across this ledger and the derivation reports:

| was called | actually |
|---|---|
| `RString::AsScaled` | `String::fraction(int)` (`RString` is `String` throughout) |
| `Player::TickResource` / `Player::UpdateCommerceCaps` | `Leader::do_gather` / `Leader::calc_resource_caps` |
| `DataWalk::walk(begin,end)` / `walk_tag(p)` | `walk_function(void*,void*)` / `walk_test(const String&)` |
| `Object::setPosition` | `SubObject::set_new_location(Coord, Coord)` |
| `Random::exchange_seed` | `Random::reseed` |
| `0x33 ping_line` | `CommandPackage::process_spline(SplineCommand*)` |
| `FUN_0065FC00` / `FUN_0061C490` "the `DataWalk`/checksum methods" (`live-tables.md` §1) | `ObjectType::log_data` / `UnitType::log_data` — `Log` visitors, same family as `Constants::log_data`, **not** the checksum interface |

**A name is not a semantics.** `ObjectData::train_time` `0x006508C0` is the clearest case: the
PDB says the rate ramp *is* train time, which resolves an open question in `sim-economy.md`
§3.3 — but `x`, the value from `UnitType` vtable `+0x6C`/`+0x70` that the ramp scales, is still
underived, so `ramped_rate` still must not be wired to a build queue and called train time.

### 4.10 Ledger self-corrections carried forward

- **"Field offsets — 1,223 constants" was `tier B`** with evidence "agree between
  instruction-level extraction and Ghidra decompiled C". **No retail code was executed**, and
  the charter says decompiled C is a hypothesis, not a second derivation. Relabelled
  **structural** (§1.10). This was tier inflation sitting in the charter's own ledger.
- **The `hash_into_range` row said `reachability: ISLAND`** without saying that this now means
  *dead code* — 0 references of any kind in the image. Corrected (§1.1).
- **The "not yet derived" list was six items stale**, still listing the damage pipeline, the
  RNG, gather rates/cost ramping/attrition, pathfinding, the checksum field set and the
  tokenizer as underived after all six had landed.

### 4.11 Other refutations banked

- **`BorderSpline` is rendering, not territory.** It derives from `Spline`/`SplineOut`, has
  exactly one code reference (`BorderSpline::registration` `0x00883080`), stores only float
  tessellation parameters (`0.25f`, `100.0f`, `2.0f`, `8.0f`, `3.5f`), and touches no
  `rules.xml` territory constant. The *drawn* border is a spline; the radius is an integer
  formula read at `World::compute_reg_territory` `0x006B10E2`.
- **The descriptor "type tag" is `wcslen(name)`.** 1,195 of 1,195 non-null tags equal the wide
  name length, 0 mismatches. The 20-byte stack object is an `RString`/`String` view built from a
  literal. There is no type tag — which is *why* the earlier "the tag selects the value parser"
  hypothesis was refuted against the shipped XML unit words. `schema/bindings.json`'s `tag`
  field should be renamed `name_len`.
- **Rules-descriptor tables ≢ checksum tables.** `docs/binary-ground-truth.md` inferred the
  descriptor tables were "plausibly reused for save-game serialization and possibly the lockstep
  checksum". Save/load ≡ checksum is **true and stronger than "plausibly"** (same base class);
  rules-descriptor ≡ checksum is **false** — `TypeOut`/`Log` visitors (40 slots, method at
  `vtable+0x1C`, implemented on *type* classes at `+0xC8`) versus `DataWalk` (2 slots, method at
  `vtable+0x00`, implemented on *instance* classes at `+0x7C`). "Enumerate the callers of the
  descriptor pattern and you get the sim-state schema" does not hold.
- **`SyncDisplay` is not the checksum mechanism.** The conclusion ("a lockstep checksum routine
  exists") is right; the mechanism is `CheckSum`/`checksums.cpp`, and `SyncDisplay` is not in
  `DataWalk`'s hierarchy.
- **`.rcx` has no 10-byte header.** It is a plain gzip member from offset 0. The `0x28`
  "plausible length field" in `docs/replay-format.md` is the UTF-16 code unit `'('`, the first
  character of `"(Version: …)"`; the length is the u32 at `+2` (26 code units).
- **Solo replays contain no AI commands.** `play == 0` for all 10,544 records of `today.rcx`
  (and 14,197 + 74 in two more solo files). In a lockstep engine the AI is recomputed during
  playback. **Charter stage 9 cannot be fed from `.rcx` AI orders — there are none**, and replay
  playback fidelity depends on the AI being bit-reproducible, which raises the bar on the sim.
- **The multiplayer stream is not undecodable.** `replay-io`'s "THE BLOCKER: multiplayer command
  payloads are obfuscated… This blocks the highest-value use of the corpus" and
  `replay-stream`'s "MP command streams are **NOT** statically decodable — a faithful reader must
  advance the same Random in lockstep" are **both refuted**: 25,279/25,279 packages decode
  exactly (§3.3). `replay-io`'s diagnosis "the keystream is not constant" was wrong; the
  keystream is perfectly constant and the missing piece was the per-command padding.
- **Ghidra drops an instruction from the damage function.** `re/decomp-all/00644130.c` omits the
  `or eax, 0x40` at `0x006442AB`. The fixup writes a local and touches only mask bits 6 and 17,
  neither read downstream — deleting it entirely produced **0 mismatches over 199,629 trials**.
  Faithful and inert are different claims; both hold. Anyone reconstructing this pipeline from
  the decompilation inherits the omission silently.
- **The `out_kind` argument slot is reused as scratch** from `0x00644B2B` onward. No write
  through that pointer occurs after the flank test; a future port of the damage-kind enum is
  complete once it covers the five earlier sites.
- **`get_attack`'s upgrade term carries a rules multiplier.** `combat.md`'s "+10 × level /
  +1 × level" is more precisely `10 × (military_level × RULES[+0x8B8])` and
  `1 × (military_level × RULES[+0x8B8])`. Unverified; the harness disables the path, and
  `RULES[+0x8B8]` was 1 in the one live match.
- **`install_fake_teb()` is engineering, not a mechanic.** MSVC x86 SEH prologues execute
  `mov eax, fs:[0]`; on i386 Linux `%fs` is a null selector, so the first `in_range` attempt
  died with SIGSEGV in the prologue. The fix mmaps a page, installs it with `set_thread_area(2)`
  and writes the end-of-chain sentinel. Every future oracle run against a non-leaf sim function
  needs it. No tier applies.
- **"Parses exactly to EOF with zero residue" is not evidence** (§3.2). Any future lane citing
  it as proof of a framing should be sent back.
- **Small factual corrections the audit made and this ledger carries:** the damage function is
  **1,195** instructions, not 1,142 (zero float either way); `CheckSum::walk_function` is 17
  instructions, not 9; `schema/islands.jsonl` has **46,564** entries (not the charter's 47,177)
  and the two combat-critical gaps are between `0x64A270`/`0x64C880` and `0x61AAE0`/`0x61BE50`;
  `today.rcx` has **10,544** records, not `replay-checksum`'s 10,532 or 10,541 (its anchor
  landed 12 records late and it read the package serial as a turn number); `mp2024` has
  **2,221** `check_sums` packets, not `replay-io`'s 1,390 (a 37% undercount reported as a
  finding rather than as a symptom); MP replays carry `check_sums` in essentially **every**
  package, not "one per two turns"; the channels reading `1` early are `walls`/`ammo`/`deaths`,
  not the first three; `frames_zoomed_in + frames_zoomed_out == 8` fails on 1 of 1,313 packets;
  the 63 selections pair with 71 orders, not 63; `"2/3"` at scale 256 parses to **170**,
  captured from retail.

### 4.12 Pathfinding's headline over-extrapolated, twice

**Wrong claim:** "…so whole-sim bit-exactness is achievable and the blocker is the RNG stream,
**not floating point**."

**Not supported [measured, audit].** The *scoped* claim is solid and was re-verified by clean
linear disassembly: `0x00685990` decodes in 669 instructions and `0x00686300` in 293, with
**zero `xmm` operands and zero `f*` mnemonics in either**; the damage function likewise has zero
float in 1,195 instructions. But five minutes of searching found, inside `Ammo::init_crash`
(`0x0067BAD0`+):

```
0067bad6  imul      eax, dword ptr [ecx + 4]   ; MOVES * rules.unit_move_speed
0067bae6  divss     xmm0, xmm1                 ; single-precision divide
0067baea  cvtss2sd  xmm0, xmm0
0067baee  cvttsd2si eax, xmm0
0067baf7  mov       ecx, dword ptr [0xc06184]  ; the main sim RNG stream
0067bb00  call      0xa39d70
```

a float divide plus a double round-trip in a movement-speed/timing computation, immediately
before a draw on the sim RNG. **`0x0067BAD6` is also `economy.md`'s own "sanity witness"
citation for `unit_move_speed`, and neither lane reported the `divss` three instructions
later.** The transcendental-sweep caveat compounds it: the 187-function cone contains 247
*indirect* call sites the direct-call closure does not follow.

**Second over-extrapolation**, found in this reconciliation: the derived functions are the
**road/caravan** pathfinder (§3.6), not "Rise of Nations' pathfinder".

Retitle the headline to scope it to the road A\* driver and cost function, where it is fully
earned.

---

## 5. Open — recorded as disagreement, not adjudicated

### 5.1 Which global is `RULES` — **closed by live read, with a caveat**

`combat.md` listed as refuted: "the rules-object pointer used by the damage code is
`0x00C061F0`"; `economy.md` §3 reads `gather_rate` off `0x00C061F0`. The audit disassembled both
and found **both are real sim code** — `0x006CE7AE` reads `[0xC061F0]` in `Leader::do_gather`,
`0x0065094C` reads `[0xC061E4]` in the train-time path — and elevated it as an unresolved
cross-lane contradiction.

**The live read closes it**: in a running, match-loaded process both `[0x00C061E4]` and
`[0x00C061F0]` held `0x01798B88`. **Two pointer globals aliasing one `RULES` object.** Neither
lane was wrong and there is no second rules object. Caveat: one process, one moment; if a lane
ever sees them diverge, this is the first thing to re-check.

### 5.2 Is `GameInfo+0x04` also the *simulation* RNG seed?

**Measured:** `GameInfo+0x04` == `game[0x10]` is the sole source of the MP XOR key and of the
per-package padding keystream, and decodes 25,279/25,279 packages across two engine builds.
**Open:** whether that same word is the value `Map::make` installs into `[0x00C06184]->state`
and `Game+0x7C`. The rng lane traced the sim seed from the `game_seed` lobby key through a
*virtual* call it did not chase, so the link is inferred from both ends. No solo replay contains
a `0x38 check_random` packet to check the LCG relation against; running the seed test on a
decoded multiplayer payload would settle it and is now cheap.

### 5.3 Is the cavalry/vehicle flank divide-by-256 a genuine RoN bug?

§4.2 establishes the loader stores 40 and 33 and the use site divides by 256, so the engine
computes 7% and 6% per flank level against the 20% and 16% the XML prose implies. **What is not
established** is whether that is a real, observable bug or whether the use site does something
else that neither reading captures. Nobody has watched cavalry flank damage in play. Settling
it wants the oracle with a populated `RULES`, or the damage hook extended to log the flank term.

### 5.4 The type-id domain is wider than 493

The stride 493 is Tier-B pinned (a 493→494 mutation produced 136,409 mismatches per 199,629),
and `final_balance_table` is exactly 493×493 covering ids 50…542. But the live damage hook
observed **defender type ids 521, 522 and 526** (237 records; attacker ids stay ≤ 445). A column
index above the row stride aliases into the next row; the maximum flat index observed was
219,613, still inside 243,049 cells, so nothing read outside the array. **So it is the type-id
domain, not the stride, that exceeds the assumed `364 units + 129 buildings`.** Unresolved.
Separately, `0x00581CC0` (the wrapper above the accessor) short-circuits ids in `[0x192, 0x19E)`
= 402…413 to a flat 100.

### 5.5 `[0x00C0618C]+0x1F4` and `checksum_deep`

Four object channels (`units`, `builds`, `walls`, `guys`) open with
`cmp dword ptr [<[0x00C0618C]>+0x1F4], 0 / je <skip whole channel>`. A runtime flag switches
them off entirely. `checksum_deep` (bound to `settings+0x08` by `FUN_005D6040`) is the obvious
candidate and the link is **not** established — `+0x1F4` has 20+ unrelated writers. Named as a
hypothesis only.

### 5.6 `flanking` takes two arguments

The PDB signature is `flanking(unsigned long, unsigned long)`; the derivation and the Tier-B
model use a single argument in ECX. The 500,017-trial difftest passed with whatever the second
argument happened to be, so either it is unread on that path or the harness held it constant.
Recorded as a discrepancy rather than explained away; it wants one look at the call site at
`0x00644B26` and the calling convention.

### 5.7 Smaller open items carried from the lanes

- **Which predicates fire in a real game** — the largest remaining piece of combat, and a
  world-state problem, not an arithmetic one.
- **The `DataWalk+0x0C` mask values in production.** `check_units` passes `-1`; nobody
  enumerated what other callers pass, so "the checksum covers range X" is mask-dependent and
  currently unqualified.
- **The closed `walk_data` traversal.** 183 first-order byte ranges recovered; ~60 of 106
  functions have bad `this` taint. Completing it is now mechanical (walk the call graph from
  `check_all` with a proper abstract interpreter, resolving `+0x7C`/`+0xAC`/`+0xB0` per receiver
  class), not research.
- **`to_hit` (`+0x1EC`) and `attenuate` (`+0x1F0`) are never read by the damage function.** Both
  are clearly live in the shipped data (`TO_HIT` is 300 for 220 of 364 units); they most likely
  belong to projectile/hit resolution. Unresolved.
- **`defenderType[+0x308]`**, the unchecked `idiv` divisor at damage step 13. Written by the unit
  parser, named by no loader binding.
- **`turn_speed` exact arithmetic.** It is a 32-bit binary angle and a deterministic function of
  the XML degrees (52 distinct inputs → 52 distinct outputs, always within ±4 ULP of
  `deg·2³²/360`), but none of floor, round, `(deg<<32)/360`, f32 or 16.16 reproduces all 52.
  **Use the measured table in `schema/live/unit-attributes.txt`, not a formula.**
- **`UnitType+0x314 relative_value[352]`** in a 364-unit space. A 352-vs-364 mismatch in a
  per-unit-type array is exactly the kind of latent overflow worth knowing about; the index space
  was not determined.
- **`mp2014` (engine `03.02.03`) opcode numbering** is unverified — 12 records is not enough.
- **`Random::reseed` has no callers**; the `RandomLogEntry {frame, file, line, seed}` shape
  remains `[reported]` and could not be corroborated (the type exists in RTTI, but whatever
  populates it does not go through `Random::get`).
- **Whether the RNG state is inside the lockstep checksum.** No function in the `checksums.cpp`
  compiland references `0x00C06184` or `0x00E37A8C` — absence of evidence in one place, not a
  negative result.
- **Score 766** for `today.rcx` is validated by no lane and nothing in the header. It is the
  cheapest remaining independent check on the header decode and it is unspent.
- **Live vs free pool slots.** Every `donscan` count for a pooled class is *capacity*. Finding
  the validity/owner field is the obvious next task.
- **Whether the shipped-PDB extraction is sound.** No derivation report and no adversarial pass
  covers it.

---

## 6. Not yet derived — do not implement from folklore

- **Which predicates fire in a real game** (see §5.7). The damage *arithmetic* is Tier B; the
  ~30 object-graph guards that select the modifiers are inputs.
- **Damage steps 10, 11 and 27**, and the upgrade branches of `get_attack`/`get_armor` — all
  transcribed from the disassembly, none ever executed.
- **The four attack/armor override functions** `0x006103C0`, `0x00610160`, `0x0062E610`,
  `0x0063FA60` — now known to be what live combat actually calls, and completely underived.
- **The damage-kind enum** written through `out_kind`, at the five sites before `0x00644B2B`.
- **How income is composed.** The resource tick and its accumulator are ported; the expression
  that turns worker counts, `CITY_GATHER`, `BASIC_GATHER`, `SCHOLAR_RATE` and the six
  building-bonus tables into the gross at `econ + 0x64` is not derived. `PEASANT_RATE` (2560)
  and `OIL_RATE` (8960) are 8.8 fixed point and are read by indexed sites, none reduced to a
  formula. `FUN_006CEEE0` is the doorway.
- **Cost ramping.** `TypeData::get_cost` `0x00664090` is ~12 KB and was not reduced; only the
  class ramp ceiling and the per-slot clamp are ported. **`SUPPORT` and `PROGRESSION` are
  described only by BHG's prose in `unitrules.xml` — no engine site has been tied to either
  name — so the ramp *curve* must not be implemented in any form.**
- **The units of `x` in the rate ramp** (`UnitType` vtable `+0x6C`/`+0x70`), which is why
  `ramped_rate` must not be wired to a build queue even though the PDB calls the containing
  function `train_time`.
- **The construction of the attrition level at `player + 0x7F0`** and the **scale** of the float
  multiplier at `player + 0x7F4` (built at `Leader::calc_anti_attrition` `0x006CDCC0`).
  `ATTRITION_IMPROVED[] = 1,2,4,8` is a plausible source for the former and stays a hypothesis.
- **`peace_attrition` at `0x00822D6D`** — the constant is in `EconomyRules` and the arithmetic
  shape is `[reported]` as the same as the assassin path; that site was never disassembled.
- **Territory ownership.** The radius contribution `(TERRITORY_NUM + MULTIPLIER * bonuses) /
  TERRITORY_DEN` is structural only; the exact integer expression and rounding around
  `0x006B1480`, and how per-tile ownership is resolved between competing claims, are open.
- **The per-frame movement integrator.** Nobody located the code that reads `moves`, consumes
  the waypoint array and writes the new x/y — every writer found was a teleport or a bulk Z
  refresh. Facing/turn gating, collision/push resolution order (which is determinism-relevant)
  and formation slot assignment are all open. The representation is integer and there is no
  float in the road pathfinder that feeds it, but that is an inference, and §4.12 shows float
  *does* live in adjacent sim code.
- **The non-road pathfinder entry points** `0x00688A40`, `0x00688FC0`, `0x006897D0` — unread.
- **The composition of `final_balance_table` from `balance.xml`** — the loader that expands the
  55 category/`obj_mask` rows into per-type rows was not located.
- **The element-name blob** the loader indexes (`*(0x00C06378) + 0x10 + K`) is supplied by a
  runtime data file not present in `ron-data/` (`internal_string.xml`), so the authoritative name
  list — and therefore dead-entry and mod analysis — is out of reach. Note element lookup is by
  **name, not position** [measured], and 7 binary fields / 8 XML entries do not match up.
- **Nation ids.** The `.rdata` stat-suffix table ordering (`_DUTCH` = 22 = `0x16`) matches one
  specimen with known ground truth. The table is a *telemetry* list whose ordering has not been
  proven equal to the sim's enumeration. Do not implement a nation id from it.
- **Type-id → type-name mapping for replay command arguments.** `process_queue_up` type ids of
  544–572 exceed the 493 space. The replay's own embedded type table is the right ground truth.
- **Map settings, map-size units, and the tiles-per-unit constant in replay coordinates.**
  `rules.xml` gives the seven map sizes as 40…100 but not whether that is TCoords or WCoords —
  a factor of 16 in cell count, and a one-line derivation for whoever touches the map loader.
- **Border geometry** beyond "the drawn ribbon is a spline".
- ~~The engine's RNG algorithm and its call sites~~ — **closed**, §2.1. Not implemented.
- ~~Rule-value tokenizer semantics~~ — **closed**, §1.7. No denominator limit; rounding is
  `idiv` truncation applied after the multiply; the result is `i32`, never `f32`.
- ~~The lockstep checksum's field set~~ — **mechanism closed**, §3.1; the *closed traversal*
  remains open (§5.7).

---

## 7. Charter-compliance sweep of `crates/` — 2026-08-08

Every non-test Rust file in the workspace was checked against this ledger.

### 7.1 Implemented with no ledger row — **none**

Every public item in `crates/don-sim/src/mechanics.rs` (34 functions, structs and constant
groups) and every public item in `crates/don-rules/src/{value,rules,offsets}.rs` maps onto a
row in §1. No charter violation of this kind found.

### 7.2 Claimed in the ledger but not implemented — three, now moved to §2

The previous ledger's prose implied the RNG, `pf_dist` and `adler32` were project assets. They
are **derived and Tier B but absent from `crates/`**: `lcg_step`/`rand_real`/`rand_int` exist
only in the oracle harness `crates/oracle/src/bin/rng.rs`, and `pf_dist` and `adler32` exist
nowhere in this repo. Moved to §2 with implementation path stated as "none".

### 7.3 Wrong statements in shipped code — three, all doc comments, all outstanding

1. **`crates/don-sim/src/mechanics.rs:158-161`** — `cavalry_flank_bonus` and
   `vehicle_flank_bonus` are documented as "8.8 fixed point". **Refuted** (§4.2); the loader
   uses the plain `_wtoi` binder and the live `RULES` holds 40 and 33. The *arithmetic* is
   right; the representation comment is wrong and is the exact claim `AUDIT.md` §3.1 told both
   lanes not to carry forward.
2. **`crates/don-sim/src/world.rs:26`** — `TICK_HZ` is documented as "Shipped game data … but
   **not yet confirmed against the binary**". It **is** confirmed, at `0x005924CF` (§1.9).
3. **`crates/don-sim/src/world.rs:33-38`** — `SUBTILE`'s doc says "measurement shows the binary
   is float-heavy" and "whether parsed rule values land in `f32` or in fixed point is an open
   question". Both are now wrong: the damage pipeline and the road A\* contain **zero** float,
   and the tokenizer question is closed (`i32`, §1.7).

None of these changes a computed value; all three would mislead the next lane. They are flagged
rather than edited here, because this reconciliation deliberately touched no code.

### 7.4 Reproducibility gaps

Two Tier-B harnesses live **only on hbox** and are not in this repo:
`~/lane-pathfinding` (`vector_dist`) and `~/don-oracle-checksum` (`adler32`). If hbox is
rebuilt, those two results become unreproducible. `~/don-oracle-econ` (the tokenizer) is in the
same position, though its `econ_main.rs` was copied to a session scratchpad. The damage and
combat harnesses *are* in-tree (`crates/oracle/src/damage_env.rs`, `damage_test.rs`, `bin/rng.rs`).

### 7.5 Standing test hazards (not new, still open)

- `don_rules::value::tests::parses_the_whole_shipped_corpus` returns early when `ron-data/` is
  absent, and `don-gpu`'s four parity tests `return` when `Gpu::new()` fails. Both go green on a
  machine with no game data and no GPU adapter, and the skip notice goes to stderr where
  `cargo test` hides it. **Not admissible as CI evidence without a `--required`/env-gated
  failure mode.** Both were confirmed to actually run on this machine.
- `parses_the_whole_shipped_corpus` asserts "every shipped value yields a numeric", which is a
  property of the *prose* layer, not of the engine (`_wtoi` returns 0 for a leading non-digit).
  The engine-level replacement `engine_tokenizer::reproduces_the_whole_shipped_corpus` now
  exists and is authoritative; the prose-layer test is only sound because §1.11 narrows what
  `RuleValue` claims.
- `don_sim::mechanics::tests::matches_retail_on_captured_vectors` is the opposite of vacuous:
  the audit re-ran `oracle vectors` and confirmed the eight committed expectations are
  byte-identical to retail's output.

### 7.6 Engineering, deliberately outside the fidelity system

`crates/don-sim/src/{world,batch,simd,interleave}.rs`, `crates/don-gpu`, `crates/don-pe`,
`crates/donscan`, `crates/oracle`, and `crates/don-sim/src/bin/bench.rs` implement **no game
mechanic**. `world.rs`'s tick systems are marked `PLACEHOLDER` and compute nothing faithful;
`don-gpu`'s chamfer weights and cost model are ours and are benchmark fixtures. Their
measurements are engineering results and are recorded in `docs/derivation/simd-batch.md` and
`docs/derivation/gpu-architecture.md`, not here.

Two of their findings do constrain fidelity work and belong on the record:

- **Moving a mechanic to a GPU kernel currently costs us the ability to make a tier claim about
  it** — a WGSL kernel is not callable from the oracle. The GPU is for systems we are content to
  hold at Tier C, or for helpers whose output the CPU re-derives cheaply.
- **Parallel `f32` reduction is not deterministic and no amount of care fixes it.** Where a sum
  must be parallel and deterministic, accumulate in fixed-point integers. Where the *semantics*
  depend on order — a death threshold, a resource cap — the parallel reduction is wrong
  regardless of numerics.

---

## 8. Live-process validation log

### 8.1 The `RULES` block — 2026-08-08, PID 14644

Read out of a running, match-loaded `riseofnations.exe` (image base `0xD60000`; Windows
randomises ASLR per *boot*, not per process, so concurrent instances share it).
`[0x00C061E4]` and `[0x00C061F0]` both held `0x01798B88` (§5.1).

The original audit found **828 of 834 extracted constant records** matching live memory. Its
six failures were systematic extraction errors, not noise. After correcting them, recovering
the sixth scholar slot, and closing the two bespoke post-parse transforms, **all 835 distinct
recovered slots match the retained block byte-for-byte.** [measured]

The corrected old mismatches corroborate two independent lanes:

| field | extracted | live | ratio | explanation |
|---|---|---|---|---|
| `scholar_rate` ×6 slots | 5, 7, 10, 15, 20, 25 | 1280, 1792, 2560, 3840, 5120, 6400 | **256** | 8.8 fixed point — five entries were recorded unscaled and the sixth was omitted |
| `caravan_attack_bonus` | 2 | 20 | **10** | attack values are stored ×10, exactly as the combat lane derived |

Raw capture: `schema/live/rules-block-pid14644.txt`.

### 8.2 Method, now standard for runtime questions

`OpenProcess` + `ReadProcessMemory` via P/Invoke in guest PowerShell, driven by
`prlctl exec "Windows 11" powershell.exe -EncodedCommand <base64-utf16le>`, **writing results to
a guest file and streaming that out** — inline base64 return times out; the file hop does not.

Traps paid for in wall-clock, all [measured]: `prlctl exec` **silently hangs with no output and
no timeout** once `-EncodedCommand` gets long (~3,400 base64 chars works, ~5,600 hangs forever)
— upload real scripts in ~1,100-char `Add-Content` chunks and run them from a guest file, and
always have the script write a progress log. PowerShell **variable names are case-insensitive**,
so `$K` and `$k` collide silently. `Add-Type` compiles via `csc.exe` and is slow under the
SYSTEM context `prlctl exec` gives you; `Reflection.Emit` `DefinePInvokeMethod` does the same
job in ~0.2 s. `certutil -encode` does not reliably overwrite — fresh temp name per file, and
always hash-verify. `prlctl capture "Windows 11" --file shot.png` screenshots the guest, which
is how live game state gets confirmed.

### 8.3 Other live captures on record

| capture | file | what it settles |
|---|---|---|
| type tables | `schema/live/types-runtime.bin`, `type-names.txt`, `unit-attributes.txt`, `building-attributes.txt`, `tech-attributes.txt` | §3.5 |
| balance table | `schema/live/final-balance-runtime.bin` (**use this**) | §4.5 |
| balance table, wrong window | `schema/live/balance-runtime.bin` (**do not use**) | §4.5 |
| damage calls | `schema/live/damage-hook/live-damage-snap2.csv`, `analysis-snap2.txt`, `rules-full.txt` | §3.7, §4.3, §4.4 |
| heap census | `schema/live/donscan-pid14644-full.txt` | §3.5 |

All are gitignored as game-derived content.
