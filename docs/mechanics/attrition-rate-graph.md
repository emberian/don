# The attrition rate graph: `get_attrition`'s five returns and where `att` comes from

**Lane:** `mech:borders-fog` · **Module:** `crates/don-sim/src/systems/borders_fog.rs` ·
**Fidelity tier: C** throughout — transcription from the instruction stream and the shipped
loader-derived constants, with no oracle coverage. Nothing here is verified.

This note closes three defects `docs/mechanics/arena-attrition-selection.md` raised against
the don-sim kernel, and advances the leader-scalar graph that gates the whole system.

## 1. `UnitData::get_attrition` `0x00608FD0` has five returns, not four

The port carried four. The fifth is an **exemption**, and it is the kind of missing branch
that a period-shaped test never notices, because it changes a period into *no period*:

```asm
00609122  ; the non-militia branch begins
00609126  imul   ecx, eax, 0x6eec              ; eax = UnitData +0x09, the unit's owner
0060912c  movd   xmm0, edi                     ; edi = scale
00609130  cvtdq2ps xmm0, xmm0
00609133  push   0x2fe
00609138  add    ecx, dword ptr [0xc061e0]     ; ecx = &leaders.list[unit owner]
0060913e  mulss  xmm0, dword ptr [ecx + 0x7f4] ; anti_att
00609146  mulss  xmm0, dword ptr [0xb69430]    ; 1/256
0060914e  cvttss2si edi, xmm0
00609152  call   0x6db810                      ; LeaderData::has_preq(0x2FE)
00609157  test   eax, eax
00609159  je     0x60907f                      ; -> the merchant/air divisors
0060915f  mov    ecx, esi
00609161  call   0x46fa40                      ; UnitData::is_idle()
00609166  test   eax, eax
00609168  je     0x60907f
0060916e  xor    eax, eax                      ; return 0
```

**An idle unit whose own owner holds the Forage-tier attrition bonus takes no attrition at
all.** Three details the shape hides:

* **`this` for `has_preq` is the unit's own owner**, not the territory owner — the same
  `&leaders.list[unit.owner]` the `anti_att` load just used (`ecx` is live across the
  `mulss`, which only reads it). The two leader scalars in this function come from
  *different* leaders; this predicate belongs to the victim's side.
* **Non-militia only.** The militia branch at `0x00609069` computes
  `(scale * 100) / (militia_attrition + 100)` and jumps to `0x0060907F`, never reaching
  either call. A standing militia unit of a Forage nation still takes attrition, at the
  300%-increased militia rate.
* **The pair is short-circuited, and the second call has side effects.**
  `UnitData::is_idle` `0x0046FA40` is `const` in the PDB signature yet writes three cached
  `OrderList` words (`+0xD4`, `+0xCC`, `+0xD0` at `0x0046FA60..0x0046FA78`) before returning
  `[+0xCC] == 0`. `0x00609159` skips the call entirely when `has_preq` is zero, so a host
  that models those writes must not perform them unconditionally.

`0x2FE` is identified as the Forage tier by `Leader::calc_anti_attrition` itself: it selects
`Constants::attrition_upgrade` index `2` for `has_preq(0x300)`, `1` for `(0x2FF)` and `0` for
`(0x2FE)` (`0x006CDCF1..0x006CDD53`), and index `0` ships as
`"25% (decrease from Forage)"` (`docs/derivation/rules-constants.json`, `+0x1C8`).

`AttritionInput` gains `owner_has_preq_0x2fe` and `unit_is_idle` for this. They are inputs,
not defaults: `LeaderData::has_preq` `0x006DB810` walks a BonusType prerequisite graph the
executable builds and `ron-data/` does not ship, so a host that cannot answer must say so.

### An unclaimed divergence, recorded not fixed

`cvttss2si` yields the x86 *integer indefinite* value `0x8000_0000` when the float does not
fit an `i32`; Rust's `as i32` saturates to `i32::MAX`. Unreachable with shipped rules;
reachable with `attrition_upgrade` percentages near 99. Deciding it needs the oracle.

## 2. `ObjectTypeData +0x218` is `domain`, and the field is now named that

`schema/types.json` names `ObjectTypeData +0x218` **`domain`** — the movement domain, `0`
land, `1` sea, `2` air. `borders_fog.rs` documented it as an unnamed "type class" whose `1`
"suppresses attrition" and whose `2` "halves". The arithmetic was right; the *reading* was
not, in a way that mattered:

* `0x0060909F` tests `== 2` and nothing else. `get_attrition` never reads `1`.
* The sea return is the **caller's**: `Unit::process_attrition` `0x005E1456` returns when the
  domain is `1`, long before the rate tail (`0x005E18F1` re-tests it for the land path).
  Putting that return in the kernel would have double-applied it for any host that already
  models the caller, and it would have described `get_attrition` as having a branch it does
  not have.
* So the halving at `0x0060909F` and in `special_attrition_period` is the **air** discount:
  air units take half attrition, and the two 8-frame trespass periods become 4 for them.

The neighbouring `type_id` triple is also named now: `0x3D`, `0x3E`, `0x190` is exactly the
set `ObjectData::is_merchant` `0x0046D370` accepts, so that halving is the merchant discount.

## 3. `Leader::calc_attrition` `0x006CDEA0` — the scalar the whole system waits on

`get_attrition` returns `0` when `LeaderData::att +0x7F0` of the **territory owner** is zero,
so `att == 0` is why no unit anywhere has ever taken attrition in this codebase. It is now
ported (inputs still booleans; see §4).

```text
if give_att_disabled (+0x7F8) != 0                         -> att = 0        0x006CDEA6
n = length of the satisfied prefix of
    has_preq(0x2DD), (0x2DE), (0x2DF), (0x2E0)             -> stops at the first false
att = if n == 0 { 0 } else { attrition_improved[n - 1] }   {1,2,4,8}         0x006CDEEC
then, in this order, for each held bonus:
    has_wonder(0x212)      Colosseum   colosseum_attrition +0x470  50%       0x006CDEFA
    has_tribe_bonus(0xD)   Russians    russian_attrition   +0x74C 100%       0x006CDF32
    CTW bonus              (see below) ctw_attrition       +0xA0C  50%       0x006CDF65
    has_wonder(0x21A)      Kremlin     kremlin_attrition   +0x510 100%       0x006CDFAC
  att = ((pct + 100) * att) / 100;   if att == 0 { att = 1 }
```

Three things only the instruction stream says:

1. **The zero floor fires on the product, and it is the headline.** `imul ecx, edi` with
   `edi == 0` is zero, and `cmove edi, 1` at `0x006CDF2B` (plus three siblings) replaces that
   zero with **one**. So the Colosseum, the Russian tribe bonus, the Conquer-the-World bonus
   or the Kremlin each give a nation with *no attrition research whatsoever* a rate of 1 —
   the shipped 48-frame baseline period. A "percentage increase" that manufactures a rate out
   of nothing is not what the rules text implies, and dropping the `cmove` would leave those
   four bonuses inert.
2. **The four multipliers do not commute.** `/100` is MSVC's truncating signed magic-number
   divide (`imul 0x51EB851F; sar edx, 5; edx + (edx >>> 31)`). At `att = 1` the shipped order
   gives `1 → 1 → 2 → 3 → 6`; Kremlin-first gives `9`.
3. **`cmp esi, 0x2AD` / `has_tribe_bonus(4)` at `0x006CDEB8..0x006CDECB` is unreachable.** The
   counter starts at `0x2DD` and only increments. It is not modelled; modelling it would be
   inventing a branch retail cannot take.

The CTW predicate is `Game +0x822 & 2` **and** `LeaderData +0x6900 != 0` — `num_bonus_cards[10]`,
where `LeaderData::num_bonus_cards` is `unsigned char[38]` at `+0x68F6`.

`Leader::calc_anti_attrition` `0x006CDCC0` was already ported and re-checked against the same
instruction stream this pass; `take_att_disabled +0x7FC != 0` zeroes `anti_att` outright
(`0x006CDCC7`), which makes the unit immune, and the seed is the immediate `0x43800000` =
`256.0f`.

## 4. What is still not derivable, and was not written

* `LeaderData::has_preq` `0x006DB810`, `has_wonder` `0x006EBC10`, `has_tribe_bonus`
  `0x006E1370` are **not ported**. `calc_attrition` and `calc_anti_attrition` take their
  answers as booleans. `has_preq` is a recursive walk over each BonusType's prerequisite
  list; those lists are built by the executable, `typenames.xml` has no bonus section, and
  `schema/live/live-tables-typeids.tsv` dumps the `BonusType` rows with empty names. Passing
  `[false; 4]` is **not** a default — it asserts that no nation in the game has attrition
  research, which is exactly what a sibling lane refused to do and this lane also refuses.
* The rare-resource and wonder *flags* likewise stay caller inputs.
* The unowned-territory continuation for `WData::who == -2`. See
  `docs/assembly/attrition-unowned-territory-fallthrough.md`.

## 5. What would close the graph

One of two things, in order of cost:

1. **A live-process read** of `LeaderData +0x7F0` / `+0x7F4` for each of the eight leaders in
   the Windows VM, in a game where a player has researched Military line techs. That produces
   real values and, taken across a research event, also cross-checks §3's ladder without
   recovering the prerequisite graph at all.
2. **Recovering the BonusType prerequisite graph** from the executable — the general and
   correct fix, and much larger.

## 6. Tests

`crates/don-sim/tests/attrition_kernel_returns.rs`, 15 tests, all mutation-checked (eleven
mutants applied to the shipped file, all eleven killed):

| area | tests |
|---|---|
| the fifth return | `an_idle_unit_whose_owner_holds_the_forage_bonus_takes_no_attrition`, `the_idle_exemption_needs_both_operands`, `the_idle_exemption_is_the_non_militia_branch_only`, `the_idle_exemption_beats_every_later_divisor` |
| `domain` | `only_the_air_domain_is_read_by_the_rate_kernel`, `the_trespass_periods_halve_for_air_only` |
| the fall-through | `an_unowned_tile_reads_the_win32_keyboard_state_buffer`, `the_contested_marker_lands_somewhere_we_have_not_derived`, `an_owned_tile_is_not_a_fallthrough_at_all` |
| `calc_attrition` | `the_research_ladder_indexes_attrition_improved_from_the_level_count`, `a_gap_in_the_prerequisite_chain_stops_the_count`, `give_att_disabled_zeroes_the_rate_before_any_bonus`, `a_single_bonus_lifts_a_researchless_nation_off_zero`, `the_four_multipliers_compose_in_the_shipped_order`, `each_bonus_uses_its_own_shipped_percentage` |

These are Rust tests over the don-sim kernels. They pin the transcription against the
instruction stream; they are not retail differential evidence and do not move the tier.

## 7. Cross-lane consequence

`don_sim::systems::borders_fog::AttritionInput` gained two fields and renamed `type_class` to
`domain`. `crates/don-ai/src/arena/retail_systems.rs` builds that literal in two places
(`execute_attrition_recompute` and `attrition_period_adapter_preserves_rate_and_special_source_mutations`)
and must add `owner_has_preq_0x2fe` / `unit_is_idle` and rename the field. The arena already
computes both values at the call site — `rate.victim_upgrade_0x2fe` and `facts.is_idle` — so
its pre-application of the exemption immediately above the call can be deleted in favour of
the kernel's own return.
