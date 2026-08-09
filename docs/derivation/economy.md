# Economy derivation — gather, cost ramping, attrition, borders

Lane: **economy**. All addresses are preferred-base VAs in
`ron-bin/riseofnations.exe` (sha256 `30478a44…625079`, image base `0x00400000`).
Every claim below is marked **[measured]** (I verified it against this binary / this data /
the oracle on hbox) or **[reported]**. Nothing here is "verified" in the proof-assistant
sense; see `docs/CHARTER.md`.

Machine-readable companion: **`docs/derivation/rules-constants.json`** — all 719
`rules.xml` constants (845 slots) with struct offset, parse mode, scale, shipped value and
the integer the engine stores.

---

## 0. Lead result

**The rule-value tokenizer is fully recovered and differentially tested.** It is
`FUN_00a1d110` (`RString::AsScaled(int scale)`), and it is *not* a general rational parser:

```c
int RString::AsScaled(int scale) {          // VA 0x00A1D110, __thiscall, ret 4
    const wchar_t *s = data();
    if (!s) return 0;
    int num = _wtoi(s);                     // IAT 0x00AC54AC
    const wchar_t *sl = wcschr(s, L'/');    // IAT 0x00AC5434
    int den;
    if (sl) { den = _wtoi(sl + 1); if (den == 0) return 0; }
    else      den = 1;
    return (num * scale) / den;             // 32-bit signed mul, idiv truncating to zero
}
```

`scale` is a **per-field compile-time constant** chosen by the loader, not derived from the
value string: **192** for tile distances, **256** for 8.8 fixed-point ratios, **100** for
percent-style ratios. Every other constant (661 of the loader's 701 scalar
parse sites, plus every array entry) is parsed by plain `_wtoi` — scale 1 — which silently stops at the first non-digit,
so `"450 frames"` → `450` and a `/` in such a field would be ignored.

**Tier B**, 202,643 calls, 0 mismatches (§6). Answers the open question in
`crates/don-rules/src/value.rs`; §7 says exactly what that file should do.

Two project claims are **refuted** on the way (§1, §5): `FUN_00570170` is *not* the
rules.xml loader, and `BorderSpline` is *not* the territory computation.

---

## 1. The loader — and a correction to `docs/binary-ground-truth.md`

### 1.1 `FUN_00570170` is the GameLog dump walker, not the loader [measured]

`docs/binary-ground-truth.md` and `docs/provenance-ledger.md` both call `FUN_00570170`
"the rules.xml constants loader". It is not. Evidence:

- Every binding site **pushes the field by value**, not by address:
  `0x00573C69  push dword ptr [edi + 0x27C]` (`gather_rate`). A loader cannot write
  through a copy.
- Its callers pass a visitor whose vtable the compiler devirtualises against
  `0x00B39158` (guards at `0x005887DB`, `0x005887F6`, `0x00588816`, …). That vtable's RTTI
  complete-object locator is at `0x00B74364` → type descriptor `0x00C93854` →
  **`.?AVGameLog@@`**. Slot `+0x1C` (the "bind" method) is `0x0043CEE0`.
- Callers: `0x0058880F` (`this = *(0x00C061F0)`), `0x0092F3F8` and `0x0092FC84`
  (`this = *(0x00C061E4)`) — the log/desync path described in
  `docs/oracle-architecture.md`.

So `schema/bindings.json` is a **name → struct-offset schema recovered from the desync/log
walker**. It is still correct and load-bearing; it is just not the XML loader. That also
explains why the XML *tag* names never appear in `.text` (§1.3).

Bonus, and it retires another ledger entry: the descriptor "type tag" is **`wcslen(name)`**.
Checked over all of `schema/bindings.json`: 1195 non-null tags, **1195 equal to the name
length, 0 mismatches** [measured]. The 20-byte stack object is an `RString` built from a
literal (`{ const wchar_t* ptr; u16 len; u16 pad; u16 len; u8 1; u8 g_cp; u32 0; u32 0 }`),
destroyed after each call by `0x00A1CF40`. There is no type tag.

### 1.2 The real loader [measured]

`Rules::Load` occupies **`0x00569A90` – `0x0057016F`** (one 26 KB function; the `int3` run
before it ends at `0x00569A8D`). Per scalar field it emits exactly:

```asm
mov  eax, [0x00C06378]      ; string-table object
mov  eax, [eax + 0x10]      ; base of a runtime string blob
add  eax, 0xCA1C            ; per-field constant: the XML element name
push eax
mov  ecx, esi               ; esi = the Rules struct
call 0x0057FA60             ; or 0x0057F950 for scaled fields
mov  [esi + 0xD3C], eax     ; store at the field's offset
```

Call census inside the loader body [measured]: `0x0057FA60` ×**661**, `0x0057F950` ×**40**,
plus array loops. The two helpers are:

| helper | meaning |
|---|---|
| `0x0057FA60(name)` | `node = this->mXml(+0xD40)->GetChild(name,1,0); return node.GetAttr(L"value").ToInt()` — `ToInt` = `0x00A1D210` → `0x00A15FC0` → `_wtoi` (with a `0x`/`0b` prefix table at `0x00A16010`) |
| `0x0057F950(name, scale)` | same lookup, then `0x00A1D110` (`AsScaled(scale)`) |

`Rules + 0xD40` (= 3392) is the XML section object; the value slots therefore occupy
`0 … 3388`, i.e. 848 dwords, which matches the 845 slots recovered from the walker plus a
handful of stale entries (§1.4).

**Arrays** are loaded by an explicit loop, e.g. `COMMERCE_CAP` at `0x0056C261`:

```asm
xor  edi, edi
lea  ebx, [esi + 0x400]              ; &rules.commerce_cap[0]
loop:  g_scratch = L"entry";  RString::AppendInt(g_scratch, edi)   ; 0x00A1D180
       s = node.GetAttr(&out, g_scratch)                          ; 0x00A27900
       v = RString::ToInt(g_scratch)                              ; 0x0042D9E0  (== _wtoi path)
       *ebx = v; ebx += 4; inc edi; cmp edi, 8; jl loop
```

So attribute names are **built at runtime as `"entry" + i`** — which is why neither
`entry0` nor any uppercase XML tag exists as a string in the image, in any encoding
[measured: exhaustive byte search of every section for `GATHER_RATE`, `gather_rate` ASCII,
`COMMERCE_CAP`, `CONSTANTS`, `entry0` and their UTF-16LE forms — zero hits]. **Array
entries are parsed with plain `_wtoi`, scale 1** [measured: `0x0042D9E0` is
instruction-for-instruction the same `ToInt` shape as `0x00A1D210`].

### 1.3 Where the element names live — open [measured negative]

The element names are `*(0x00C06378) + 0x10 + K` with `K` a compile-time constant per
field. That blob is loaded at runtime (the image contains the diagnostic
`"internal_string.xml not loaded. File missing?"` at `0x00AD1AE0`). I did **not** identify
the file that supplies it; `ron-data/` does not contain it. Consequences:

- Element lookup is **by name, not positional** [measured]. Proof: the binary's field order
  and the XML's document order disagree in localized spots — `SHIP_DEFENSIVE_RESPOND_RANGE`
  is 3rd in the `<CONSTANTS>` block but last (offset 52) in the struct; `ATTRITION` is 3rd
  from the end in the XML but last (offset 3388) in the struct. Positional loading would
  put `UNIT_BUILD_RESPOND_RANGE`'s 12 into `UNIT_GATHER_RESPOND_RANGE`, etc. — a visible
  scramble in the shipped pair.
- The *authoritative* element-name list is therefore not in the exe, so mod compatibility
  and dead-entry analysis need that data file. **Open.**

### 1.4 Name drift between shipped XML and binary [measured]

Cross-checking the 719 walker names against the 723 `<CONSTANTS>` tags:

- in the binary, absent from `rules.xml`: `city_level_territory_bonus`, `taj_caravan`,
  `kremlin_spy_instant`, `german_light_cavalry`, `russian_uber_spies`, `ally_to_war_delay`,
  `ally_to_war_grace`
- in `rules.xml`, absent from the binary: `city_upgrade_terr`, `taj_caravan_limit`,
  `liberty_free_upgrades`, `eiffel_siege_range`, `spanish_extra_scout`,
  `japanese_aircraft_carriers_speed`, `korean_start_citizen`, `korean_free_citizen`

If the runtime name list matches the *binary's* internal names (untested — §1.3), those
eight XML entries are inert and those seven fields keep their `_wtoi` default of `-1`
(the default passed at `0x0057FB05` is `push -1`). Flagged, not asserted.

---

## 2. The rule-value tokenizer, precisely

### 2.1 Semantics [measured, Tier B in §6]

| aspect | answer |
|---|---|
| numerator | `_wtoi`: skip whitespace, optional `+`/`-`, decimal digits, stop at first non-digit |
| denominator | text after the **first** `/` anywhere in the string, again via `_wtoi` |
| no `/` | denominator = 1 |
| denominator 0 | **returns 0** (matches the `rules.xml` header: `"0" denom specifies no distance/speed`) |
| denominator limit | **there is none in the code.** The "largest denominator allowed is 192" comment is an *authoring* convention that keeps `num*192/den` exact; the parser will happily do `1/7` |
| rounding | C truncation toward zero (`idiv`), applied **after** the multiply: `(num*scale)/den` |
| result type | **`int32`**, not `f32`, not a general fixed-point type. The multiply is a wrapping 32-bit `imul` |
| trailing prose | ignored entirely; unit words (`tile`, `frames`, `tsx`, `rng`) are **not** in the binary and are **not** interpreted [measured] |
| leading non-digit | `_wtoi` returns 0 |

The three scales, with the fields that use them [measured, from the 40 `0x0057F950` sites]:

| scale | meaning | fields |
|---|---|---|
| **192** | distance, in **1/192 tile** | `unit_formation_spacing`(0) `unit_move_speed`(4) `unit_guy_spacing`(20) `target_radius`(56) `unit_train_distance`(132) `unit_train_max_distance`(136) `boat_train_distance`(140) `boat_train_max_distance`(144) `boat_garrison_max_distance`(148) `unit_board_distance`(152) `unit_disembark_distance`(156) |
| **256** | ratio in **8.8 fixed point** | `unit_turn_speed`(8) `range_inaccuracy`(64) `rocky_modifier`(88) `overkill_damage`(96) `entrenchment_modifier`(100) `river_modifier`(104) `recapture_city_modifier`(108) `peasant_rate`(640) `oil_rate`(668) `research_premium`(932) `research_tick_premium`(936) `lakota_cav_dmg_bonus`(2140) + 9 campaign-hero fields + `siege_out_of_supply_reload`(3272) `artillery_out_of_supply_reload`(3276) |
| **100** | ratio in **percent** | `unit_rate_base`(544) `unit_rate_progression`(548) `accel_train`(552) `accel_construct`(556) `accel_research`(560) |

Everything else is scale 1 (`_wtoi`), including all **31** array-valued rules.

### 2.2 Captured retail outputs [measured — from the oracle, not computed]

```
 192  "1/16 tile (calibration for unit spacing in formations)"   -> 12
 192  "1/192 tile (granularity for unit movement speeds)"        -> 1
 192  "1/2 tile (calibration for target sizes)"                  -> 96
 192  "3/2 tile"                                                 -> 288
 192  "8 tile"                                                   -> 1536
 256  "1/1 rate (master control for unit turn speed)"            -> 256
 256  "2/3 (light infantry in rocks)"                            -> 170
 256  "2/1 (units take more damage in rivers)"                   -> 512
 256  "1/3"                                                      -> 85
 256  "1/100 -percent per # tiles"                               -> 2
 256  "10 resources"                                             -> 2560
 256  "35 oil"                                                   -> 8960
 256  "12/10"                                                    -> 307
 256  "80/100"                                                   -> 204
 100  "6/5 base rate (See BR before adjusting)"                  -> 120
 100  "3/4 progression (See BR before adjusting)"                -> 75
 100  "1/1 speed  ..."                                           -> 100
```

Sanity witnesses that the scales are right, from independent read sites [measured]:
`disband_city_rate` (`"400%"` → 400, scale 1) is used at `0x00650951` as
`t = t*rules[0x380]/100`; `unit_move_speed` = 1 is used at `0x0067BAD6` as
`imul eax, [rules+4]` against `UnitType.MOVES`, i.e. it is the 1/192-tile granularity
multiplier the XML comment describes.

### 2.3 Quirks worth knowing [measured]

- Eight scale-1 fields have a `/` inside their prose (`"8 tiles (BR 1/16/2003 …)"`,
  `"25 resources / age"`, `"3 +/- to sell price"`, …). Because they are parsed by `_wtoi`
  and not `AsScaled`, the slash is harmless — none of them is mis-parsed. If a mod moved
  such a value into a scaled field it would be.
- No shipped `<CONSTANTS>` value fails to yield a leading integer.
- `AsScaled` never inspects the string length; it relies on NUL termination.
- `(num*scale)` wraps at 32 bits before the divide, and `idiv` traps on `INT_MIN / -1`.
  Reachable only from hand-edited data.

---

## 3. The resource tick

### 3.1 The accumulator [measured]

`Player::TickResource` at **`0x006CE450`**; the payout core at `0x006CE7AE`:

```asm
mov  eax, [0x00C061F0]        ; g_rules
mov  edi, [edi + 0x6EB8]      ; player -> econ block
mov  ecx, [eax + 0x27C]       ; rules.gather_rate  = 450
shl  ecx, 4                   ; PERIOD = gather_rate * 16 = 7200
mov  eax, esi                 ; esi = this tick's income, in 1/16-resource units
cdq
idiv ecx                      ; eax = whole resources, edx = remainder
...
econ[0x18 + res*4] ^= 0x3421  ; accumulator (obfuscated), += edx
...                           ; then: while (acc >= 7200) { acc -= 7200; whole++ }
```

So the engine keeps a **per-resource integer accumulator** at `econ + 0x18 + res*4` and
credits one whole resource each time it crosses **`GATHER_RATE * 16 = 7200`**. Income is
carried in **1/16-resource units** — every contribution is `shl eax, 4` before being added
(`0x006CE6DC`). Equivalently: an income of `R` resources per `GATHER_RATE` frames is
represented as `esi = 16*R` per tick.

`GATHER_RATE` = `"450 frames"` = 450 [measured], and 450 frames = **30 s** at the 15 fps
engine rate. Independent confirmation from the UI at `0x0072A5B0`: it divides
`rules.gather_rate` by 15 (magic `0x88888889`, `sar edx,3`) and prints
`(gather_rate/15) * 100 / (100 + bonus)` — "seconds per payout, reduced by the gather-rate
bonus percent".

### 3.2 Income pipeline for one resource, in order [measured, `0x006CE450`]

1. Interest-style term (`0x006CE6AE`–`0x006CE703`): `surplus = (stockpile ^ 0x8221) - T`;
   if `surplus > 0` then `esi += ((surplus * rules.dutch_interest(0x8A8)) / 100) << 4`,
   and the result is clamped to
   `(econ.commerce_cap[res] + rules.dutch_interest_cap(0x8AC)) << 4`.
2. **Global hard cap** `esi = min(esi, 0x3E70 = 16000)` (`0x006CE706`) — i.e. **1000
   resources per 450 frames**, per resource type.
3. Displayed income cached at `econ[0x94 + res*4] ^ 0x90236`.
4. Gather-rate bonus: `esi = esi * (100 + B) / 100` where `B = FUN_006D66A0(player)`
   (`0x006CE72C`).
5. Difficulty (`*(0x00C061EC) + 0x2F`): for **knowledge only** (`res == 3`),
   difficulty 5–6 → `esi = esi*3/4`; difficulty > 6 → `esi = esi/2` (`0x006CE755`).
6. Two game-setting flags (`+0x20 & 2`, `+0x2A == 9`) → `esi = esi*3/2`.
7. Game speed `*(0x00C061C0)`: if > 1, `esi *= speed`.
8. Accumulate as in §3.1.

`PEASANT_RATE` is `"10 resources"` at scale 256 → **2560**, i.e. **10.0 in 8.8**;
`OIL_RATE` `"35 oil"` → **8960** = 35.0. Both feed the per-worker contribution as 8.8
values (the Lakota bounty at `0x0064BF8C` shows the same idiom explicitly:
`amount = lakota_cav_dmg_bonus(85) * dmg * (gather_rate<<4)`, then `>> 8`).

**Not established**: the exact expression that turns worker counts, `CITY_GATHER`,
`BASIC_GATHER`, `SCHOLAR_RATE`, and the building-bonus tables (`FISHERMEN_BONUS`(680),
`GRANARY_BONUS`(700), `LUMBERMILL_BONUS`(720), `SMELTER_BONUS`(740),
`MERCHANTS_BONUS`(784), `REFINERY_BONUS`(760)) into `esi`. The readers are indexed —
`0x00633E9C` (`city_gather`), `0x006336A9` (`farms_per_city_base`), `0x006E0A36`
(`fishermen_bonus`), `0x006E0902` (`merchants_bonus`), `0x006CF44C` (`refinery_bonus`),
`0x006CF64C` (`territory_taxes`) — but I did not reduce them to a formula. Next lane.

### 3.3 Commerce cap [measured]

`Player::UpdateCommerceCaps` at **`0x006CE900`**:

```asm
eax = econ[0xF0] ^ 0x63187                 ; the player's AGE, obfuscated
for (res = 0; res < 6; res++) {
    if (res == 3)  { econ[0x3C] = 0x1166; ... }        ; knowledge: fixed
    ecx = rules.commerce_cap[age]          ; [g_rules + age*4 + 0x400]
    econ[0x30 + res*4] = ecx ^ 0x1281
    if (British)  cap = cap * (100 + rules.british_commerce(0x6D0)) / 100
    if (res == 2) cap = cap * (100 + rules.inca_wealth_cap(0x598)) / 100   ; wealth
    cap += pyramids_commerce(0x428) | colossus_commerce(0x444) | taj_wealth_commerce(0x500)
         | eiffel_oil_commerce(0x544) | kremlin_commerce(0x50C) | tikal_timber_commerce(0x494)
         | angkor_metal_commerce(0x4D4) | republic_commerce_bonus{,2,3}(0x9A0,0x9A4,0x9A8)
         | diamonds_commerce(0x92C)      ; each gated by its own wonder/tech/resource check
}
```

Two hard facts fall out:

- **`COMMERCE_CAP` is indexed by age**, 8 entries at offset **1024**
  (`70,100,150,200,260,320,400,500`).
- **Knowledge has a hardcoded cap of 999**: `0x1166 ^ 0x1281 = 0x03E7 = 999`
  (`0x006CE92C`) [measured]. It is not in `rules.xml` at all.

**Sim state is XOR-obfuscated in the player block** [measured] — an anti-cheat measure that
directly affects any state mirroring or checksum work: age `^0x63187`, commerce cap
`^0x1281`, resource accumulator `^0x3421`, displayed income `^0x90236`, another age field
`^0x62766` (`0x006090D1`). Player blocks are a flat array at `*(0x00C061E0)` with stride
**`0x6EEC`**; the econ sub-block is `player + 0x6EB8`.

---

## 4. Cost / rate ramping

### 4.1 The shipped-data contract [measured — `ron-data/unitrules.xml` lines 60–90]

Authored documentation shipped with the game (BHG prose; treat as a strong hypothesis, not
as engine behaviour):

- `SUPPORT` = "Ramping cost of unit (f=food,t=timber,m=metal,g=wealth,k=knowledge,o=oil)"
- `COST` = "Base cost of unit (multiply by 10)" — and `UNIT_COST_FACTOR` = `"10 resources"`
- `PROGRESSION` = "0 = ramp linearly on # of units of this type; 1 = linearly on # of
  units of group; 2 = progressively on # of this type; 3 = progressively on # of group"
  (shipped distribution: **232× `3`, 128× `0`, 4× `2`**)
- `JOB_EXTRA_TIME` = "extra JOB_TIME per unit of this type" (shipped: **360× `1/10tsx`,
  4× `0/10tsx`**)

### 4.2 Time ramping — recovered [measured]

`FUN_006508C0` (`this` = a build/queue object, arg = type index) computes a per-item rate.
The ramp core at `0x006508EB`:

```asm
esi   = g_rules
ecx   = (X * rules.unit_rate_base(0x220)) / 100          ; magic 0x51EB851F, sar 5 => /100
edi   = playerBlock[0x56FE + type*2]                     ; u16 count of this type
edi  *= UnitType[0x2E8]                                  ; = JOB_EXTRA_TIME  (offset 744)
eax   = 3 * ecx                                          ; lea eax,[ecx+ecx*2]
edi  *= rules.unit_rate_progression(0x224)
edi  += ecx
if (edi < 0 || eax < 0) edi = 0; else if (edi > eax) edi = eax     ; clamp to [0, 3*ecx]
```

i.e.

```
base  = X * UNIT_RATE_BASE / 100                     ; UNIT_RATE_BASE = 120  => 1.2x
ramp  = count(type) * JOB_EXTRA_TIME * UNIT_RATE_PROGRESSION
value = clamp(base + ramp, 0, 3 * base)
```

`UnitType + 0x2E8 = 744 = job_extra_time` is confirmed independently from
`schema/bindings.json` (`FUN_0061C490`, `ref_ea 0061CAD9`) [measured].
The `/100` divides are all `imul 0x51EB851F; sar edx,5; +sign` = signed truncating `/100`.

Then follow the modifiers, each `v = v*100/(100+P)` or `v = v*(100-P)/100`:
`disband_city_rate`(896) and `disband_senate_rate`(900) at `0x00650951`/`0x00650989`;
a `rules[0x850]` term; `/2` for certain classes; `rules[0x958]`, `rules[0xC84]`,
`rules[0x7EC]` gated by tech checks.

**Caveat I will not paper over:** with `JOB_EXTRA_TIME = 1/10 → _wtoi → 1` (scale 1,
because `job_extra_time` is *not* one of the 40 scaled fields) and
`UNIT_RATE_PROGRESSION = 75`, the ramp term is `75 * count` against a base of
`1.2 * JOB_TIME`, which saturates the `3×` clamp after ~3–10 units. That is arithmetically
what the code says, but I have not confirmed the units of `X` (the value returned by
`UnitType` vtable `+0x6C` / `+0x70` at `0x00650907`/`0x00650AA8`), so **do not implement
this as train time yet**. `X` and the identity of the caller are the missing pieces.

### 4.3 Cost ramping — located, not reduced [measured]

`FUN_00664090` is the cost function. It selects a ramp ceiling by unit class:

| rules offset | rule | selected at |
|---|---|---|
| 916 `0x394` | `unit_scholar_ramp_max` (`"2000%"`) | `0x006656AF` |
| 920 `0x398` | `unit_worker_ramp_max` (`"500%"`) | `0x0066569F` |
| 924 `0x39C` | `unit_other_civilian_ramp_max` (`"200%"`) | `0x006653FF` |
| 928 `0x3A0` | `unit_military_ramp_max` (`"125%"`) | `0x0066568F` |

then `esi = base * ramp_max / 100` (the `0x51EB851F` idiom) and the accumulation loop at
`0x00665440` runs over **2 resource slots** (`ebp-0x20 = 2`), reading a cost table at
`UnitType + 0x270`:

```asm
if (costTable[i-2] == resourceId) {
    eax = costTable[i] * edi + ebp[-0x18]     ; per-resource cost * multiplier + ramp accum
    if (esi != 0 && esi < eax) eax = esi      ; clamp to ramp_max
    ... tech/tribe discounts: eax = eax * (100 - rules[0x98C]) / 100 ...
    ebx += eax
}
```

`tech_cost_factor`(860) is read at `0x0066630C`, `build_support_factor`(892) at
`0x006BC47D`. The full reduction of `FUN_00664090` (≈9 KB, dozens of gated modifiers) is
**not done** — it is the single biggest remaining economy target and deserves its own lane.

---

## 5. Attrition

### 5.1 The rule constants [measured]

| rule | offset | shipped value | parsed |
|---|---|---|---|
| `attrition_upgrade[0..3]` | 456 | `25% / 50% / 75% / 100%` | 25, 50, 75, 100 |
| `attrition_improved[0..3]` | 472 | `1 / 2 / 4 / 8` | 1, 2, 4, 8 |
| `siege_attrition` | 488 | `"50% reduction"` | 50 |
| `militia_attrition` | 492 | `"300% increase"` | 300 |
| `attrition_aged_up` | 496 | `"25% increase"` | 25 |
| `colosseum_attrition` | 1136 | `"50% increase to attrition caused"` | 50 |
| `kremlin_attrition` | 1296 | `"100% increase"` | 100 |
| `liberty_attrition` | 1316 | `"100% reduction of attrition received"` | 100 |
| `russian_attrition` | 1868 | `"100% bonus"` | 100 |
| `mongol_attrition` | 2032 | `"50% reduction"` | 50 |
| `titanium_attrition` | 2396 | `"50%"` | 50 |
| `peace_attrition` | 3380 | `"8 frames …border-violation attrition"` | 8 |
| `assassin_attrition` | 3384 | `"8 frames …assassin attrition"` | 8 |
| `attrition` | **3388** | `"48 frames -> baseline level for regular attrition"` | **48** |

### 5.2 The per-unit strength function `FUN_00608FD0` [measured]

`__thiscall(Unit* this, int attackerPlayerIdx)`, `ret 4`:

```
ebx = players[attackerPlayerIdx].attritionLevel   ; playerBlock + 0x7F0 (int)
if (ebx == 0)                 return 0
if (rules.attrition(0xD3C) == 0) return 0         ; master switch
edi = 0x100                                        ; 1.0 in 8.8

if (unit is siege-class)      edi = 25600 / (100 - rules.siege_attrition)   ; >=100% => return 0
if (unit has ability 0x42)    edi = edi * 100 / (100 + rules.militia_attrition)
if (type in {0x3D,0x3E,0x190}) edi /= 2
if (type[0x218] == 2)          edi /= 2

ageDiff = (attackerAge ^ 0x62766) - (defenderAge ^ 0x62766)
if (ageDiff >= 0)
    ebx = (ebx * (100 + rules.attrition_aged_up * ageDiff) + 99) / 100     ; CEILING
return edi / ebx                                                           ; signed trunc
```

The alternate branch at `0x00609122` converts to float:
`edi = (int)((float)edi * players[p].f7F4 * 0.00390625f)` — `0.00390625f = 1/256`
(`0x00B69430`) [measured], i.e. the same 8.8 scale, and `player + 0x7F4` is a **float**
attrition multiplier built at `0x006CDD20` from `attrition_upgrade[]`,
`liberty_attrition`, `colosseum_attrition` and `kremlin_attrition` (`100.0f` at
`0x00B696AC`).

### 5.3 The damage site [measured]

`0x005E192B`:

```asm
push edi
mov  ecx, ebx                  ; the unit
call 0x00608FD0
mov  ecx, eax
test ecx, ecx / je  skip
mov  eax, [0x00C061F0]
mov  eax, [eax + 0xD3C]        ; rules.attrition = 48
imul eax, ecx
cdq / and edx, 0xFF / add eax, edx / sar eax, 8     ; = (48 * strength) / 256, toward zero
cmp  eax, 1                    ; floor of 1
```

so **`damage = ATTRITION * strength / 256`, minimum 1**, with `ATTRITION = 48` and the
`>>8` being the 8.8 unscale. The same shape appears at `0x00822D6D` using
`peace_attrition`(3380) as the divisor, and `assassin_attrition`(3384) at `0x005E15DD`.

**Not established:** the tick *period* (how often `0x005E18xx` runs per unit), and the
exact construction of `playerBlock + 0x7F0`. `attrition_improved` (1,2,4,8) is the obvious
candidate for that integer given `attrition_upgrade` (25/50/75/100) drives the float at
`+0x7F4`, but I did not confirm it, so it stays a hypothesis. Note also that `edi / ebx`
makes the returned strength *decrease* as the level integer rises, which is the opposite of
the intuitive reading — resolve that before implementing.

---

## 6. Differential test (Tier B)

Target `FUN_00A1D110` (`RString::AsScaled`). It calls exactly two CRT imports; the harness
maps the retail image, then patches those two IAT slots to point at its own
implementations, so **the retail instructions run unmodified while `_wtoi` and `wcschr` are
substituted**. The Tier-B claim therefore covers the slash search, the 32-bit multiply, the
truncating divide and the zero-denominator early-out — *conditional on* those two leaves.
That conditionality is part of the claim, not a footnote.

```
[econ-oracle] mapped at 0xf64d7000, 315865 relocations applied
[econ-oracle] IAT patched: _wtoi -> 0x08054040, wcschr -> 0x080543c0
rule-value tokenizer FUN_00a1d110 (RString::AsScaled)
  corpus: 850 shipped rules.xml value strings x 3 scales, 31 edge cases x 3 scales, 200000 generated
  total 202643 calls, 0 mismatches
```

Input distribution: (a) **exhaustive over the shipped corpus** — every `value=` and
`entryN=` string in `ron-data/rules.xml`, at all three scales; (b) 31 hand-chosen edges
(`""`, `"/"`, `"0/0"`, `"1/0"`, `"5/"`, `"-3/4"`, `"3/-4"`, `"  7  /  2 "`, `"1/2/3"`,
`"abc/2"`, `INT_MAX/1`, `1/INT_MAX`, `INT_MIN/1`, …); (c) 200,000 generated
`num[/den][prose]` strings, `num ∈ [-2000,2000]`, `den ∈ [-200,200]`, 8 prose tails, from
a fixed xorshift seed `0x9E3779B97F4A7C15`. The single trapping input `INT_MIN / -1` is
excluded by construction.

Reproduce (harness lives in **my own directory on hbox**, so it cannot collide with the
combat lane's edits to `crates/oracle/src/main.rs`):

```sh
scp econ_main.rs hbox:~/don-oracle-econ/crates/oracle/src/main.rs
scp ron-data/rules.xml hbox:~/don-oracle-econ/data/rules.xml
ssh hbox 'cd ~/don-oracle-econ && nice -n 15 taskset -c 0-3 \
    cargo build --target i686-unknown-linux-musl -q && \
    nice -n 15 taskset -c 0-3 ./target/i686-unknown-linux-musl/debug/oracle 200000'
```

Source: `~/don-oracle-econ/crates/oracle/src/main.rs` on hbox (copy also in this session's
scratchpad as `econ_main.rs`). No repo file was modified for this test.

---

## 7. What `crates/don-rules/src/value.rs` should do

Do **not** add a general `to_f32()`. The engine's semantics are integer and per-field:

```rust
/// Exactly `FUN_00a1d110`. `scale` is 192, 256 or 100 and comes from the FIELD,
/// never from the value string.
pub fn as_scaled(raw: &str, scale: i32) -> i32 {
    let num = wtoi(raw);
    let den = match raw.find('/') {
        None => 1,
        Some(i) => { let d = wtoi(&raw[i + 1..]); if d == 0 { return 0; } d }
    };
    num.wrapping_mul(scale).wrapping_div(den)   // idiv: truncates toward zero
}

/// Exactly `_wtoi` as the engine uses it (scale-1 fields and every array entry).
pub fn as_int(raw: &str) -> i32 { wtoi(raw) }   // ws, sign, digits, stop at non-digit
```

- `Number::Rational` must stay **unevaluated** in the token type; evaluation is
  `(num*scale)/den` and needs the field's scale.
- The current tokenizer's `percent` and `unit` fields describe the *text*, not engine
  behaviour — the engine never reads a `%` or a unit word. Keep them for documentation and
  linting; never let a value depend on them.
- The current parser accepts a decimal point (`1.5`). The engine does not: `_wtoi` stops at
  the `.`. No shipped value has one, but a mod could, and our parser must diverge the same
  way (i.e. yield 1).
- Which scale each field uses is in `docs/derivation/rules-constants.json`
  (`parser`, `scale`), so a generated table can carry it.

---

## 8. Borders

### 8.1 `BorderSpline` is rendering, not territory [measured — refutes a working assumption]

- Type descriptor `.?AVBorderSpline@@` at `0x00C9BC00`; vtable `0x00B5419C`; **exactly one**
  code reference, the constructor at `0x00883080`.
- Base classes, via their complete-object locators: **`.?AVSpline@@`** (vtable `0x00B24798`)
  and **`.?AVSplineOut@@`** (vtable `0x00B557CC`).
- The constructor stores only float tessellation parameters:
  `+0x10C = 0.25f`, `+0x110 = 100.0f`, `+0x114 = 2.0f`, `+0x118 = 8.0f`, `+0x11C = 8.0f`,
  `+0x120 = 2.0f`, `+0x48 = 3.5f`.
- It touches **no** `rules.xml` territory constant.

So the "borders are splines, hence no closed-form radius" story is about the **drawn border
ribbon**. Territory ownership itself is computed in integers from the territory rules.

### 8.2 The territory radius formula [measured — structure; values not yet difftested]

Readers, from the rules-reader index (§9):

```
0x006B10E2  territory_base (276)
0x006B1489  capital_territory_bonus (252)
0x006B1497  imul  city_territory_multiplier (272)
0x006B149E  add   territory_num (296)
0x006B14A4  mov   territory_den (292)

0x006B1AF9  city_level_territory_bonus[level] (256 + level*4)
0x006B1B29  imul  fort_territory_multiplier (268)
0x006B1B30  add   territory_num (296)
0x006B1B36  mov   territory_den (292)
```

This is exactly the formula the shipped comment states —
`TERRITORY_NUM value="11  (Numerator + (CityorFortMultiplier * BorderBonuses))/Denominator"`
[`ron-data/rules.xml:78`] — i.e.

```
radius_contribution = (TERRITORY_NUM + MULTIPLIER * bonuses) / TERRITORY_DEN
```

with `TERRITORY_NUM = 11`, `TERRITORY_DEN = 5`, `CITY_TERRITORY_MULTIPLIER = 4`,
`FORT_TERRITORY_MULTIPLIER = 4`, `TERRITORY_BASE = 24` (all scale 1, `_wtoi`), and the
bonus sources being `city_level_territory_bonus[]`(256, 3 entries),
`capital_territory_bonus`(252), `civic_upgrade_terr[]`(220, 8),
`temple_upgrade_terr[]`(200, 5), `fort_upgrade_terr[]`(184, 4),
`colosseum_territory_bonus`(1132), `eiffel_tower_territory_bonus`(1340),
`gems_territory_bonus`(2352), `roman_fort_borders`(1540), `russian_borders`(1888) +
`russian_borders_per_age`(1892), `tikal_temple_borders`(1176). Growth limits are
`territory_limit_base`(280) `= 44`, `territory_limit_civic`(284) `= 4`,
`territory_limit_city`(288) `= 4`, copied into a config object at `0x0069DAF7` and
`0x006B77AC`.

**Not established:** the exact integer expression and rounding around `0x006B1480`, and how
per-tile ownership is resolved between competing claims. Structure only.

---

## 9. Artifacts produced

| file | contents |
|---|---|
| `docs/derivation/rules-constants.json` | 719 fields / 845 slots (31 of them arrays): name, struct offset, entry count, parser (`wtoi` \| `scaled`), scale, binder address, shipped XML value, and the integer the engine stores |
| this document | derivations, addresses, tiers |

The JSON is the first **complete** `rules.xml` schema for this build: `schema/bindings.json`
covers 210 of these fields and no array bases. Recovered by walking `FUN_00570170`
(`0x00570170`–`0x00577F77`, virtual-call form, 211 names incl. array counts) and
`0x00577F93`–`0x0057F906` (direct-call form via `0x0042DA30`, 509 names), then joining to
the loader's 701 scalar parse sites. All 31 array **base offsets** are confirmed a second,
independent way: the loader materialises each one as `lea ebx, [esi + base]` immediately
before its `entry`-loop (`0x0056A123` … `0x0056C476`), and all 31 agree with the walker.

Two fields have a loader site I did not classify: `unit_block_radius`(16) and
`americans_marine_entrench`(2164) — they use neither `0x0057FA60` nor `0x0057F950` (13
calls to `0x0042D9E0` and 17 direct `0x00A15FC0` calls in the loader are unaccounted for).
Marked `parser: null` in the JSON rather than guessed.

---

## 10. Reproduction

```sh
# schema extraction (capstone, no Ghidra lock)
cd /Users/ember/dev/don/ron-bin
uv run --with capstone --with pefile python rebind2.py 0x570170 0x578200   # virtual form
uv run --with capstone --with pefile python rebind4.py 0x577f93 0x57f906 0x42da30
# loader parse sites
uv run --with capstone --with pefile python  # scan 0x569a90..0x570170 for calls to
                                             # 0x57fa60 / 0x57f950 + the following store
# every read of a rules constant, across .text (1,466 sites)
uv run --with capstone --with pefile python allreaders.py   # -> rules_readers.txt
# tokenizer difftest
ssh hbox 'cd ~/don-oracle-econ && ./target/i686-unknown-linux-musl/debug/oracle 200000'
```

Scripts used are in this session's scratchpad
(`<local-recovery-scratchpad>/`):
`pe.py`, `rebind2.py`, `rebind3.py`, `rebind4.py`, `disp.py`, `readers.py`, `allreaders.py`,
`econ_main.rs`. They are throwaway analysis tools, not repo code; if the project wants them
kept they belong under `re/scripts/`.

---

## 11. Ledger deltas requested

Additions:

- **Rule-value tokenizer** — `FUN_00A1D110`, Tier **B**, 202,643 calls / 0 mismatches,
  distribution in §6. Note the two substituted CRT leaves.
- **Rules struct schema** — 718 fields, `docs/derivation/rules-constants.json`, Tier B for
  the 686 scalars whose parse mode is pinned to a loader call site; the 31 array fields'
  `_wtoi` mode is Tier B via `0x0042D9E0` ≡ the `ToInt` shape.
- **`GATHER_RATE * 16 = 7200` accumulator**, `0x006CE7AE` — structural, [measured], not
  difftested.
- **Knowledge commerce cap = 999**, hardcoded at `0x006CE92C` — [measured], not difftested.
- **Attrition damage `= ATTRITION * strength / 256`, min 1**, `0x005E193D` — [measured],
  not difftested.

Corrections:

- `FUN_00570170` is **not** the rules.xml loader; it is the `GameLog` dump walker. The
  loader is at `0x00569A90`. Update `docs/binary-ground-truth.md` (two places) and
  `docs/provenance-ledger.md`.
- The descriptor "type tag" is **`wcslen(name)`**. Close the open question; it carries no
  type information. (1195/1195 checked.)
- `BorderSpline` derives from `Spline`/`SplineOut` and holds only float tessellation
  parameters; it does not compute territory. The `binary-ground-truth.md` line
  "`BorderSpline` ⇒ borders are splines — which explains why no closed-form radius formula
  was ever published" should say that the *drawn* border is a spline while the radius is an
  integer formula (§8.2).
- New, and load-bearing for stages 4/6: **player sim state is XOR-obfuscated** with
  per-field constants (§3.3). Any `XData` mirroring or checksum work must account for it.
