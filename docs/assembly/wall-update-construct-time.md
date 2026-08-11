# `Wall::update_construct_time` `0x0063D560` — how long a building takes to go up

Lane: `tick8-construct-time`, closing part of `Gap::LeaderCalcWallStats` on tick step 8
(`Leaders::process_all` `0x006ED2A0`). Read [`economy-step8.md`](economy-step8.md) first;
this is the one body inside `Leader::calc_wall_stats` that lane left unported.

**Tier C.** Structure, constants, branch order and rounding read from
`ron-bin/riseofnations.exe` (sha256 `30478a44…625079`) with capstone, cross-read against
`re/decomp-all/0063d560.c`, `ron-bin/sbl/rise.pdb`, `schema/vtables.json`,
`schema/pdb-types.json` and `docs/derivation/rules-constants.json`. **Nothing here has been
executed against retail and no oracle case was added.** The tests below drive the real
`Sim::do_frame`; that makes them integration tests of the port, not evidence about the game.

---

## 1. Why this function and not another

`Leader::calc_wall_stats` `0x006CF7C0` (429 B) is two loops with identical bodies over the
2000 (building) and 3000 (wall) bands. Per active object it makes three calls:

| call | building band | wall band |
|---|---|---|
| `Wall::update_construct_time`, if `!is_active` | **direct** `call 0x63d560` @`0x006CF838` | **direct** `call 0x63d560` @`0x006CF908` |
| vtable `+0x15C` | `Wall::update_hits` `0x0063F0D0` | `Object::update_hits` `0x00647010` |
| vtable `+0x160` | `Wall::update_los` `0x0063EEB0` | `Object::update_los` `0x00646FE0` |

All four vtable bodies were already ported. The construct-time call is the odd one out —
it is not a vtable slot, it is a plain direct call, and it was the last body in the pass
with no Rust behind it, charged unconditionally to `Gap::LeaderCalcWallStats` once per
under-construction object per armed pass.

Because it is a **direct** call, the *same* 545-byte body runs for both bands; only the
object's own vtable slots inside it differ. The port therefore has one function and both
band passes call it.

### The gate is the under-construction predicate

`0x006CF817` / `0x006CF8E7` call vtable `+0x4C` = `WallData::is_active` `0x00472350`
(8 bytes, `flags & 4`) and skip the call when it answers non-zero. So this runs for every
object *still being built*, on every stat-pass edge. `Wall::update_hits` then quantises the
construction ramp against `constr_time` in the very next call — which is why the order
inside the loop is load-bearing and is asserted directly.

---

## 2. The body, exactly

`this` is the object (`Wall`/`BuildData`/`WallData` all share the `+0x08 flags`,
`+0x09 who`, `+0x18 ptype`, `+0x50 constr_time` layout the PDB gives them). `ebx` holds the
running value and **everything is `u32`**: `div`, not `idiv`; the two magic-constant
divisions are unsigned `mul`+`shr`. This is the opposite of `Wall::update_hits`'s signed
`pct_scale` chain sitting three instructions away, and it is the easiest thing here to get
wrong.

```text
v = ptype->vtbl[0x6C](who)                     TypeData::time         0x0063D56F
if has_tribe_bonus(1)  && !this->vtbl[0x2C]()  v = v*100/(MAYA_BUILDING_SPEED+100)
if has_preq(0x313)                             v = (v*3) >> 2                0x0063D5CF
if has_wonder(0x218)                           v = v*100/(VERSAILLES_BUILDING_SPEED+100)
if rare13(effective) || rare13(maskB)          v = v*100/(TOBACCO_BUILDING_SPEED+100)
if has_tribe_bonus(0xB) && this->is(0x20B,0)   v = v*100/(BRITISH_AA_SPEED+100)
if has_tribe_bonus(0x16)&& ptype->vtbl[0xFC]() && !this->vtbl[0x2C]()
                                               v = v*100/(DUTCH_FORT_SPEED+100)
if has_tribe_bonus(6)   && ptype->vtbl[0xFC]() && !this->vtbl[0x2C]()
                                               v = v*100/(ROMAN_FORT_SPEED+100)
if leader->city_num == 0                       v = CAPITAL_BUILD_TIME * v / 100   0x0063D746
v = (10 - get_building_speed_upgrade()) * v / 10                                  0x0063D767
this->constr_time = v ; return v                                                  0x0063D76F
```

The reduction shape repeated six times is literally

```
imul eax, ebx, 0x64      ; v * 100, 32-bit truncating
add  ecx, 0x64           ; RULE + 100
xor  edx, edx
div  ecx                 ; UNSIGNED
```

so a rule of `-100` is a `#DE` fault in retail. The port refuses that state
(`construct_time_pct` returns `None`) rather than inventing an answer; the refusal is what
`Gap::LeaderCalcWallStats` counts.

### Four things a paraphrase loses

**1. `!is_wonder` is three separate calls, not a hoisted flag.** Maya, Dutch and Roman each
re-issue vtable `+0x2C`. Versailles, Tobacco and the British airdefense reduction never
consult it. A Wonder therefore still gets the last three. Hoisting the check is the
plausible "tidy" refactor and it changes the game; the mutation test in §5 exists for it.

**2. The last two steps divide by literals, not by a rule.** `0x0063D741` loads
`0x51EB851F` and `0x0063D762` loads `0xCCCCCCCD` — the unsigned magic divisions by 100 and
by 10. So `CAPITAL_BUILD_TIME` is a *multiplier over a fixed 100* (300 triples the time)
and the upgrade step is over a fixed 10, both in the **opposite direction** from the six
`RULE + 100` divisors above them, where a bigger rule means faster.

**3. `has_preq(BUILDINGS_CREATED_FASTER)` is the one non-percentage step.**
`lea ebx,[ebx+ebx*2]; shr ebx,2` = a flat `× 3/4`, with no rule at all.

**4. `CAPITAL_BUILD_TIME` has no type gate.** Its captured XML text is *"300% of normal
city build time (this is for nomad games only)"*, but `0x0063D72D` tests only
`LeaderData::city_num == 0` and then scales whatever object it was handed. Recorded as
measured; the word "city" is the rule's, not the code's.

---

## 3. Every constant, named

### Rules — `docs/derivation/rules-constants.json`, captured from `Constants::init`

| offset | name | shipped | XML text | read at |
|---|---|---|---|---|
| `+0x3BC` = 956 | `capital_build_time` | 300 | "300% of normal city build time (this is for nomad games only)" | `0x0063D73B` |
| `+0x4E4` = 1252 | `versailles_building_speed` | 0 | "0% faster" | `0x0063D5FA` |
| `+0x590` = 1424 | `maya_building_speed` | 20 | "20% bonus" | `0x0063D5A1` |
| `+0x610` = 1552 | `roman_fort_speed` | 50 | "50% faster" | `0x0063D713` |
| `+0x6E4` = 1764 | `british_aa_speed` | 33 | "33% faster creation" | `0x0063D67B` |
| `+0x8B4` = 2228 | `dutch_fort_speed` | 0 | "0% faster" | `0x0063D6C7` |
| `+0x90C` = 2316 | `tobacco_building_speed` | 10 | "10%" | `0x0063D633` |

The rule *names* are the cross-check that carries the tribe-bonus indices. Nothing in the
disassembly says `has_tribe_bonus(1)` is the Maya; the fact that index 1 gates
`maya_building_speed` does, and the same argument fixes 6 = Roman, 0xB = British,
0x16 = Dutch. 0x16 independently agrees with `UnitArmorInputs::dutch`, already landed from
`ObjectData::armor`.

### `TypeIndex` — `schema/pdb-types.json`

| value | name | where |
|---|---|---|
| `0x1BB` = 443 | `FORTX` | `BuildTypeData::is_fort` `0x00472BA0` |
| `0x20B` = 523 | `AIRDEFENSE` | the British arm, `0x0063D652` |
| `0x20E`..`0x21E` | `BASE_WONDERTYPES`..`SPACEPROGRAM` | `BuildData::is_wonder` / `TypeData::is_wonder_type` |
| `0x218` = 536 | `VERSAILLES` | `0x0063D5DF` |
| `0x275`..`0x2AB` | `BASE_SPELLTYPES`..`CANCEL_ALLIANCE7` | `TypeData::is_spell_type` |
| `0x2AD` = 685 | `BUY_SELL` | the dead compare in `get_building_speed_upgrade` |
| `0x2F2`..`0x2F4` | `BUILDINGS_FASTER_1..3` | the upgrade count |
| `0x313` = 787 | `BUILDINGS_CREATED_FASTER` | `0x0063D5BB` |

### Leader layout

`0x0063D578` is `imul ecx, ecx, 0x6EEC; add ecx, 0xE3A390` — the same array base and stride
`Leaders::process_all` walks, and `sizeof(Leader)` in the PDB is 28,396 = `0x6EEC`, which is
the independent confirmation.

* `Leader + 0x3F8` is PDB `city_num` (`int`), read at `0x0063D72D`.
* The two rare tests are `test byte ptr [.. + 0xE41135], 0x20` (`0x0063D614`) and
  `test byte ptr [ecx + 0x6DCD], 0x20` (`0x0063D623`). `0xE41135 - 0xE3A390 = 0x6DA5`, so
  they are payload byte 1 of the effective `BitMask<44>` at `+0x6D98` and of mask B at
  `+0x6DC0` — bit 13. They sit exactly one byte below the `& 0x40` pair at `+0x6DA7`/
  `+0x6DCF` that `calc_anti_attrition` reads for Titanium (bit 30), which is the
  cross-check that both functions index the same 12-byte-header payload.

---

## 4. The three helper functions, resolved rather than assumed

### `ObjectData::is` and the devirtualisation at `0x0063D65D`

```
0063d650  push 0
0063d652  push 0x20b
0063d657  mov  eax, [vtbl + 0xb8]
0063d65d  cmp  eax, 0x653790          ; ObjectData::is
0063d662  jne  0x63d778               ; -> mov ecx,edi ; call eax  (through the trampoline)
0063d668  mov  ecx,[edi+0x18] ; mov eax,[ecx] ; call [eax+0x60]    ; straight to the type
```

`ObjectData::is` `0x00653790` is 12 bytes and *is* `jmp [this->ptype->vtbl[0x60]]`. So both
arms are the same predicate `this->is(AIRDEFENSE, 0)` and the fast arm just skips one jump.
The decompiler drops the two pushed arguments on the fast arm; they are consumed by the
callee. Every object-band vtable's `+0xB8` is `ObjectData::is`, so in this build the fast
arm is always the taken one.

### `TypeData::time` `0x00663F20` — the seed, and why it stays an input

```
if (this->vtbl[0x48] == TypeData::is_spell_type)   spell = (0x275 <= type < 0x2AC)
else                                              spell = this->is_spell_type()
if (!spell || leaders[who].has_spell(type))       return job_time * 100
else                                              return res_time * 100
```

`BuildType`'s vtable `0xB42B94 + 0x48` **is** `TypeData::is_spell_type` `0x00470590`, and a
building type is never in the spell range, so for both bands this reduces to
`BuildTypeData::job_time * 100`. That is a read of the **global type table**, which
`don-sim` does not own — the same boundary `UnitSpeedInputs::type_moves` and
`ObjectHitInputs::base_hits` already sit behind — so it is carried as
`WallConstructTimeInputs::type_time` rather than derived. See §6.

### `LeaderData::get_building_speed_upgrade` `0x006DAE90` — ported whole

```
count = 0
for t in 0x2F2 ..= 0x2F4:                 ; BUILDINGS_FASTER_1..3
    if t == 0x2AD: c = has_tribe_bonus(4) ; unreachable: 0x2AD is not in the range
    else:          c = has_preq(t)
    count = c ? count + 1 : count         ; 0x006DAECA: lea/test/cmove, unconditional
return count
```

Two things worth stating. It is **not** a consecutive run — `calc_attrition`'s
`ATTRITION1..4` loop *is*, and copying that shape here loses an upgrade whenever the middle
one is missing. And the `0x2AD` compare is the same dead `BUY_SELL` branch `calc_attrition`
carries at `0x006CDEB8`; it is recorded in the port's doc comment rather than dropped.

### The vtable resolution, and a trap in `schema/vtables.json`

| class | vtable | `+0x2C` | `+0x4C` | `+0x15C` | `+0x160` |
|---|---|---|---|---|---|
| `UnitData` (band 0) | `0xB41B08` | `0x0041BFF0` → 0 | `0x0046CDA0` | `Object::update_hits` | `Object::update_los` |
| `BuildData` (band 2000) | `0xB426DC` | `BuildData::is_wonder` | `WallData::is_active` | `Wall::update_hits` | `Wall::update_los` |
| `WallData` (band 3000) | `0xB43054` | `0x0041BFF0` → 0 | `WallData::is_active` | `Object::update_hits` | `Object::update_los` |

So `is_wonder` is *only* answerable on the building band; a plain wall's `+0x2C` is
`xor eax,eax; ret`, which is why the port's wall-band package always carries
`is_wonder: false`.

`schema/vtables.json` lists the *type* classes twice, eight bytes apart
(`0xB428CC`/`0xB428D4` for `BuildTypeData`, `0xB42B8C`/`0xB42B94` for `BuildType`). **The
higher address is the real vtable**: `BuildData::is_wonder` compares `*ptype` against
`0xB42B94`. Reading the lower one shifts every slot by two and silently returns the wrong
function — at `+0x60` it gives `TypeData::is_declare_war` instead of `ObjectTypeData::is`.

---

## 5. Measured

All numbers produced by running the code on this Mac, arm64.

`tools/swarm-cargo sim-leaders test -p don-sim` — **2,567 passed, 0 failed, 2 ignored**
across 137 targets, including the new work:

* `systems::leaders` unit tests: **68 passed** (was 64; four added).
* `tests/tick_step8_construct_time.rs`: **6 passed**, every one driving `Sim::do_frame`.

The gap movement, asserted directly rather than described. With one under-construction
building and one under-construction wall, on a frame whose rare-mask edge arms the pass:

| state | `Gap::LeaderCalcWallStats` | `cover.leader_construct_time_updates` |
|---|---|---|
| no construct-time package (before this lane's behaviour) | 6 | 0 |
| package supplied | 4 | 2 |

The residual 4 is honest and named: `Object::update_hits`/`update_los` on the wall band and
`Wall::update_hits`/`update_los` on the building band, all still waiting on automatic query
population (§6).

### Mutation tests

Four one-line mutations were applied to the landed code and the observed failure recorded,
so a green test is known to be able to go red.

| mutation | observed failure |
|---|---|
| add `&& !input.is_wonder` to the Tobacco arm (the plausible hoist) | `real_tick_suppresses_only_the_three_wonder_gated_reductions`, `left: 100000` vs `right: 90909` |
| make `CAPITAL_BUILD_TIME` a `RULE + 100` divisor like the six above it | `real_tick_applies_capital_build_time_only_while_city_num_is_zero` (`25000` vs `300000`) **and** `real_tick_composes_the_whole_reduction_chain_in_emitted_order` (`5695` vs `68349`) |
| move the construct-time call after `Wall::update_hits` in the loop | `construct_time_is_stored_before_the_hit_ramp_consumes_it`, `construct_hits` `1000` (unramped) vs `250` |
| make `get_building_speed_upgrade` a consecutive run | `building_speed_upgrade_counts_all_three_preqs_not_a_consecutive_run` (`1` vs `2`) **and** `real_tick_composes_the_whole_reduction_chain_in_emitted_order` (`76893` vs `68349`) |

---

## 6. Boundaries, stated so they are not mistaken for coverage

* **`WallConstructTimeInputs` is not populated automatically.** `type_time`, `is_wonder`,
  `is_airdefense` and `is_fort` are global-type-table reads, and `don-sim` has no shipped
  building-type source: `UnitTypeStatSource` covers Type slots 50..414 (units) and carries
  no `job_time`, `hits` or `los` column, and its provenance digest is pinned to an exact
  local capture that cannot be extended from here. Until a `BuildTypeData` source exists on
  the same provenance-bound footing, an absent package is a charged, counted miss and
  `constr_time` is left alone.
* **The same missing source is now the *whole* of `Gap::LeaderCalcUnitStats`.** Every body
  in `calc_unit_stats` — `Object::update_hits/update_los`, `Unit::update_speed`,
  `ObjectData::armor`, `Unit::update_armor` — is ported. What remains charged is
  `ObjectHitInputs::base_hits` and `StatObject::type_los`, i.e. `ObjectTypeData::hits` and
  `ObjectTypeData::los` (`type + 0x21C`), which no checked-in table supplies. That gap
  cannot shrink from inside step 8; it is a rules/type-loading row.
* **`Leader::process_taunt` `0x006B8CC0` (2,340 B) is still not ported**, and the earlier
  "it is AI chat" framing understates it: cases 1–5 read the leader's encrypted resource
  block (`(&DAT_00E41248)[slot*0x1BBB] + res*4 ^ 0x8221`) and, above a threshold of `0x95`,
  call three functions at `0x006D15E0` / `0x006D1780` / `0x006D03C0` with `amount / 3` —
  that is a **resource transfer**, i.e. simulation state, interleaved with `MessageWin`,
  localized `String` and `SoundGlobal` calls. It also writes `+0x354`/`+0x374`, arrays the
  step-8 dispatcher does not touch. It deserves its own lane, not a footnote.
* **`Object::eject_contents`** remains independently red only when the recovered tail
  predicate in `Wall::update_hits` reaches it. Unchanged by this lane.
* **Step 8's `StepStatus` stays `Stub`.** It has charged children; flipping it would be tier
  inflation.
* **Nothing here is comparable to a retail checksum**, and no oracle case exists for it. The
  natural Tier-B upgrade is an i686 oracle case that calls the mapped `0x0063D560` with a
  generated `(type_time, tribe/tech/rare state, rules)` domain and compares the stored
  `constr_time`; that is one `tools/oracle-regress.sh` row away and is the obvious next
  step.
