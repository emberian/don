# Live runtime type tables — units, buildings, techs

Lane: `live-tables`. Date: 2026-08-08. Target: `riseofnations.exe`, PID 14644 in the
Parallels VM "Windows 11", module base `0x00D60000` (ASLR delta `+0x00960000`).

## Lead

**The missing link is found and it is not a bound attribute array — it is an array of
objects.** `FUN_0065fc00` / `FUN_0061c490` are not loaders that store pointers into a
container; they are the `DataWalk`/checksum methods of `ObjectType` and `UnitType`, and
the offsets in them (`+484 attack`, `+528 hits`, `+780 crew_size` …) are **fields of one
record**. The records live in ten global `PtrArray<T>` objects, all indexed by a single
806-entry global type-id space. RTTI is intact in the binary, which gives us the real
class names.

Everything below is **[measured]** unless marked otherwise.

| what | value |
|---|---|
| `PtrArray<UnitType>` | `0x00C0A264` — 364 records, ids 50–413 |
| `PtrArray<BuildType>` | `0x00C0AA90` — 129 records, ids 414–542 |
| `PtrArray<TechType>` | `0x00C0AAAC` — 85 records, ids 544–628 |
| `PtrArray<ItemType>` | `0x00C0AAC8` — 1 record, id 543 |
| `PtrArray<ObjectType>` | `0x00C0AAE4` — 544 records = units ∪ buildings ∪ goods ∪ items (exact set equality) |
| `PtrArray<SpellType>` | `0x00C0AB00` — 55 records, ids 629–683 |
| `PtrArray<BonusType>` | `0x00C0AB1C` — 122 records, ids 684–805 |
| `PtrArray<GovType>` | `0x00C0AB38` — 0 populated |
| `PtrArray<TypeBak>` | `0x00C0AB54` — 566 records, ids 50–628 |
| `PtrArray<GoodType>` | `0x00C096E4` — 50 records, ids 0–49 |
| `Types` singleton | `0x00E85DC8`; its master `PtrArray<Type>` at `0x00E85DCC`, data ptr `0x00E85DDC` |

Cross-check of the runtime tables against `ron-data/*.xml`:

| table | scalar cells | agree |
|---|---|---|
| UnitType | 12 239 | 12 236 (99.975 %) |
| BuildType | 2 838 | 2 838 (100 %) |
| TechType | 170 | 170 (100 %) |
| **total scalar** | **15 247** | **15 244 (99.980 %)** |
| structured (COST[6], TRIBE_MASK, RANGE) | 1 647 | 1 643 (99.757 %) |

And the fourth task: **`schema/live/balance-runtime.bin` is a misaligned window and must
not be used.** The real table is `final_balance_table` at **`0x00C12BF4`**, 493×493
`int16`, 486 098 bytes, **zero zeros, no negatives**, 367 distinct values in 5…2574.
Correct capture is `schema/live/final-balance-runtime.bin`. Derivation in §5.

---

## 1. How the tables were found

`FUN_0061c490` reads `*(undefined4 *)(in_ECX + 0x30c)` and hands the *value* to
`(**(code **)(*param_1 + 0x1c))` — a visitor method with a wide-string field name. That
is the `DataWalk` shape already established for CheckSum/SaveGame/LoadGame. So `in_ECX`
is a single record and `param_1` is the walker.

The class identity comes from MSVC RTTI, which this build ships intact
(`/Users/ember/dev/don/ron-bin/riseofnations.exe`, 2 133 `.?A*` type descriptors):

```
vtable 0x00B41FD4  slot 50 = 0x0061C490   .?AVUnitType@@
vtable 0x00B42B94  slot 50 = 0x00631810   .?AVBuildType@@
vtable 0x00B4456C  slot 50 = 0x0066D630   .?AVTechType@@
vtable 0x00B44B70  slot 50 = 0x0066F2C0   .?AVGoodType@@
vtable 0x00B43AAC  slot 50 = 0x0065FC00   .?AVObjectType@@
vtable 0x00B452BC  slot 50 = 0x0065FC00   .?AVItemType@@
vtable 0x00B44384  slot 50 = 0x006631C0   .?AVBonusType@@   (== the Type base walk)
```

so the inheritance is `Type → ObjectType → {UnitType, BuildType, GoodType, ItemType}`
and `Type → {TechType, SpellType, BonusType}`, with `FUN_006631c0` = `Type::DataWalk`,
`FUN_0065fc00` = `ObjectType::DataWalk`.

The container globals fall out of the CRT static-init blob, which stores the
`PtrArray<T>` vtable straight into the global object:

```
0x004031AF  mov dword ptr [0xC0A264], 0xB243FC   ; PtrArray<UnitType>::vftable
0x004031FF  mov dword ptr [0xC0AA90], 0xB24404   ; PtrArray<BuildType>::vftable
0x0040324F  mov dword ptr [0xC0AAAC], 0xB2440C   ; PtrArray<TechType>::vftable
```

`ArrayBase` layout, read off the destructor at `0x0042E490` (which frees `[base+0x10]`):

| off | meaning |
|---|---|
| +0x00 | vftable |
| +0x04 | count |
| +0x08 | capacity |
| +0x0C | grow (int16, `0xFFFF`) |
| +0x10 | `T** data` |
| +0x14 | owns-data flag |

Confirmed by ordinary game code doing element access, e.g.

```
0x005DDA2C  mov eax, dword ptr [0xC0A274]   ; UnitType** data
0x005DDA34  mov ecx, dword ptr [eax+ecx*4]  ; UnitType* = data[unit_id]
0x005DDA37  cmp dword ptr [ecx+0x218], 1    ; ->domain
0x005DDA4E  test dword ptr [ecx+0x2B4], 0x100000  ; ->unit_flags
```

`[0xC0A274]` alone has 212 references in `.text`, which is the strongest single signal
that this is the unit-table base.

**All ten arrays have `count == capacity == 806`** and are indexed by the *global* type
id; slots that are not of that class hold `NULL`. Non-null counts: 364 / 129 / 85 / 1 /
544 / 55 / 122 / 0 / 566 / 50 — 364, 129 and 85 are exactly the `<UNIT>`, `<BUILDING>`
and `<TECH>` counts in the shipped XML.

## 2. Record layout

Field names and offsets are transcribed from the four `DataWalk` methods (structure from
Ghidra) and every one of them is confirmed against live bytes and the XML (values from
behaviour). `[scan]` in the walker marks an array.

### `Type` (base, `FUN_006631C0`)

| off | field | width |
|---|---|---|
| +0x04 | `type` (global type id) | 4 |
| +0x08 | `job_time` | 4 |
| +0x0C | `res_time` | 4 |
| +0x10 | `tribe_mask` | 4 |
| +0x14 | `cat` | 4 |
| +0x18…+0x2C | `costs[6]` | 4 each |
| +0x30…+0x38 | `preq[3]` | 4 each |
| +0x3C | `from` | 4 |
| +0x40 | `where` | 4 |
| +0x44 | `upgrade` | 4 |
| +0x48 | `jump` | 4 |
| +0x4C | `obs` | 4 |
| +0x50…+0x54 | `show[2]` | 4 each |
| +0x58 | `modified` | 4 |
| +0x5C | `grid_x` | 1 (signed) |
| +0x5D | `grid_y` | 1 (signed) |
| +0x60 | `String* name` → `wchar_t*` at +0 | 4 |
| +0x88 | `String* graph` → `wchar_t*` at +0 | 4 |

`name`/`graph` are not in the walker (strings are not checksummed); they were found by
pointer-chasing a live record and reading UTF-16 at the far end. Sentinels: `-1` = "none",
`-2` = "disable".

### `ObjectType` (`FUN_0065FC00`), all 4-byte

`+0x1E4 obj_masks`, `+0x1E8 attack` (**stored ×10**), `+0x1EC to_hit`, `+0x1F0 attenuate`,
`+0x1F4 recharge`, `+0x1F8 min_range`, `+0x1FC max_range`, `+0x200 splash_area`,
`+0x204 splash_percent`, `+0x208 ammo_per_att`, `+0x20C proj_speed`, `+0x210 hits`,
`+0x214 armor`, `+0x218 domain`, `+0x21C los`, `+0x220 science_los`, `+0x224 guy_spacing`,
`+0x228 x_spacing`, `+0x22C y_spacing`, `+0x230 abil`, `+0x234 x_size`, `+0x238 y_size`,
`+0x23C guy_radius`, `+0x240 block_radius`, `+0x244 big_radius`, `+0x248 new_block_radius`,
`+0x24C new_big_radius`, `+0x250 fly_high`, `+0x254 fly_low`, `+0x258 block_points`,
`+0x25C graft`, `+0x260 special_upgrade`, `+0x264 special_upgrade_cost`,
`+0x268…+0x26C support[2]`, `+0x270…+0x274 support_cost[2]`, `+0x278 age`.

**`hits` and `armor` are `int32`, not `int16`** — the walker reads a dword and 364/364
units plus 129/129 buildings agree with the XML at that width.

### `UnitType` (`FUN_0061C490`), all 4-byte unless noted

`+0x2B4 unit_flags`, `+0x2B8 unit_flags2`, `+0x2BC mode`, `+0x2C0 moves`,
`+0x2C4 turn_speed`, `+0x2C8 role`, `+0x2D4 carry`, `+0x2D8 carry_size`,
`+0x2DC military_level`, `+0x2E0 research_premium_cost`, `+0x2E4 research_premium_time`,
`+0x2E8 job_extra_time`, `+0x2EC mana`, `+0x2F0 control_cost` (= XML `POP`),
`+0x2F4 progression`, `+0x2F8 push_size`, `+0x2FC push_circles`, `+0x300 target_size`,
`+0x304 squad_size`, `+0x308 uber_size`, `+0x30C crew_size`, `+0x310 base_form`,
`+0x314 relative_value[352]` (**int16**).

`+0x2CC` and `+0x2D0` are not in the walker (excluded from the desync checksum).
Allocator stride between consecutive records is most commonly 1544 bytes → `sizeof(UnitType)`
is very likely 0x600 = 1536 with an 8-byte heap header. That is a hint, not a measurement.

### `BuildType` (`FUN_00631810`)

`+0x2B4 town_hits`, `+0x2B8 min_city_size`, `+0x2BC misery_rate`, `+0x2C0 build_flags`,
`+0x2C4 most_shots`, `+0x2C8 garrison_max`, `+0x2CC base_arrows`, `+0x2D0 wonder_val`,
`+0x2D4 plunder_value`, `+0x2D8 plunder_good`, `+0x2DC behind_height`, `+0x2E0` (unnamed),
`+0x2E4 civ_graph_mask` (**1 byte**). Allocator stride 872 → `sizeof` ≈ 0x360.

### `TechType` (`FUN_0066D630`)

`+0x1C8 age`, `+0x1CC ai[11]` (**int16**), `+0x1E2 leader_off` (**1 byte**). Note
`TechType` derives from `Type` directly, so it has no combat block. Stride 680 → `sizeof`
≈ 0x2A0.

### `GoodType` (`FUN_0066F2C0`)

`+0x2B4 undiscovered_cost_good`, `+0x2B8 obsolete_cost_good`, `+0x2BC undiscovered_support_good`,
`+0x2C0 obsolete_support_good`, `+0x2C4 obsolete_production_good`, `+0x2C8 undiscovered_cost_rate`,
`+0x2CC obsolete_cost_rate`, `+0x2D0 undiscovered_support_rate`, `+0x2D4 obsolete_support_rate`,
`+0x2D8 obsolete_production_rate`, `+0x2E4… bonus_type[2]/bonus_num[2]`,
`+0x2EC largest_gather`, `+0x2F0 block`, `+0x2F4 exclude_ctw`.

## 3. Scale factors, derived by fitting the live/XML ratio (not assumed)

| XML tag | field | rule | agreement |
|---|---|---|---|
| `ATTACK` | `attack +0x1E8` | **×10** | 364/364 units, 129/129 buildings |
| `HITS`, `ARMOR`, `TO_HIT`, `RECHARGE`, `SPLASH`, `SPLASH_PERCENT`, `AMMO_PER_ATT`, `PROJ_SPEED`, `LOS`, `SCIENCE_LOS`, `MOVES`, `CARRY`, `CARRY_SIZE`, `MANA`, `POP`, `PROGRESSION`, `PUSH_CIRCLES`, `UBER_SIZE`, `CREW_SIZE`, `JOB_TIME`, `GRID_Y`, `X_SIZE`, `Y_SIZE`, `TOWN_HITS`, `MOST_SHOTS`, `GARRISON_MAX`, `BASE_ARROWS`, `WONDER_VAL`, `BEHIND_HEIGHT`, `AGE` | as named | **×1** | all 100 % |
| `GUY_SPACING`, `X_SPACING`, `Y_SPACING` | +0x224/+0x228/+0x22C | **×12** | 364/364 |
| `BLOCK_RADIUS` | `block_radius +0x240` | **×48** | 364/364 |
| `BLOCK_RADIUS` | `new_block_radius +0x248` | **×1** | 364/364 |
| `TARGET_SIZE` | +0x300 | **×48** | 364/364 |
| `RESEARCH_PREMIUM_COST/TIME` | +0x2E0/+0x2E4 | **×256** | 362/362, 364/364 |
| `RANGE` `"a-brng"` | `min_range`/`max_range` | **unscaled** (tiles) | 359/362 |
| `COST` `"5f/3m"` | `costs[0..5]` | letter→index `f0 t1 g2 k3 m4 o5` | 364+129+85 = 578/578 |
| `SUPPORT` `"1f/1m support"` | `support[2]`, `support_cost[2]` | same letters; `-1` = unused | 364/364 units |
| `TRIBE_MASK` `"1111…"` | +0x10 | binary, MSB-first, 24 bits | 577/578 |
| `ATTENUATE` | +0x1F0 | **units: `|x|`; buildings: `x` signed** | 364/364, 129/129 |

`48` is the sub-tile resolution: `guy_radius == new_big_radius × 48` for 364/364, and
`big_radius == guy_radius` for 364/364. `12` (= 48/4) is the guy-spacing sub-unit.

`turn_speed +0x2C4` is a **32-bit binary angle**: 45° → `0x20000000`, 90° → `0x40000000`,
30° → 357 913 941 = ⌊2³²/12⌋. It is a pure function of the XML degrees (52 distinct values,
each mapping to exactly one runtime value) and always within ±4 ULP of `deg·2³²/360`, but
I could **not** pin the exact rounding — see §6.

## 4. Every mismatch, explained

### 4a. Same-`NAME` canonicalisation (6 cells)

Runtime records whose XML row shares a `<NAME>` with an earlier row take the **first**
row's value for some fields:

| unit | field | XML | live | first row with that NAME |
|---|---|---|---|---|
| `RIFLEMENGERMAN` | ARMOR | 1 | 3 | `RIFLEMEN` ARMOR 3 |
| `ANTITANKRIFLEGERMAN` | LOS | 12 | 11 | `ANTITANKRIFLE` LOS 11 |
| `BAZOOKAGERMAN` | LOS | 14 | 13 | `BAZOOKA` LOS 13 |
| `HUMMEL` | SPLASH_PERCENT | 33 | 25 | `HOWITZER` SPLASH_PERCENT 25 |

(the remaining two are the same rows counted in a second field). This is a **finding, not
noise**: the XML rows for the German national variants are *edited but ineffective*. A
sim that reads `unitrules.xml` naively will give German Riflemen 1 armour where the game
gives them 3. `ron-data/unitrules.xml` is byte-identical to the shipped
`…\Rise of Nations\Data\unitrules.xml` (MD5 `dd249e429f74c73b2e1c57547c4a6033`), so this
is not version drift.

### 4b. `TRIBE_MASK` inherited along the upgrade chain (1 cell)

`PIKEMENELITE` XML `111110110110110110111111`, live `011110110110110110111111` — exactly
the mask of `PIKEMEN`, whose `<NAME>` differs ("Elite Pikemen" vs "Pikemen"). So there is
a **second** canonicalisation channel beyond `<NAME>`, most likely the `from`/`upgrade`
chain. Hypothesis, not measured.

### 4c. `RANGE` zeroed on crew-carrier units (3 cells)

`MAHOUT` 0-8, `GUNMAHOUT` 0-9, `CULMAHOUT` 0-10 in the XML; all three have
`min_range = max_range = 0` at runtime, like the plain `HVYELEPHANT`. These are the
elephant carriers whose shots come from the crew. Hypothesis, not measured.

### 4d. `GRID_X` reassigned (3 cells)

`BANDEIRANTES` 0→1, `BANDEIRANTESELITE` 0→1, `IRONCLAD` 3→1. `GRID_Y` matches 364/364.
Unexplained; these are build-menu coordinates so a de-collision pass at load is the
obvious guess, but I did not confirm it.

### 4e. Fields that are **not** sourced from `unitrules.xml` at all

These are the important ones for anyone building a sim off the XML:

* `CIRCLE_RADIUS` → `guy_radius +0x23C` / `big_radius +0x244` / `new_big_radius +0x24C`.
  Only 208/364 fit `×48`; e.g. every `GENERAL*` has XML 3 but runtime 48 (= 1×48), and
  XML 3 maps variously to 48, 96, 144 and 192. **The runtime radius comes from somewhere
  else** (art/graphics is the natural candidate). Do not use `CIRCLE_RADIUS`.
* `PUSH_SIZE` → `push_size +0x2F8`. 327/364 fit `×48`; the other 37 (all leaders/heroes,
  machine guns, mortars, fishermen, herd bison) do not — e.g. `FISHERMEN` XML 3 → 192,
  `HERDBISON` XML 1 → 96. `guy_radius == push_size` for 322/364, so the same external
  source probably drives both.
* `PREQ1` on units. Only 125/364 of the XML values resolve; where the XML says `none`
  the runtime frequently holds a real tech id (`ARMEDSUPPLYWAGON` → 573 "Mercenaries",
  `MILITIA` → 572 "The Art of War", `MINUTEMAN` → 575 "Conscription"). `PREQ0` is
  362/364, `WHERE` 364/364, `GRAFT` 364/364.

### 4f. Not mismatches, just my lookup being lossy

`JUMP` 357/364 and `FROM` 360/364 "failures" are all cases where two types share a
display `NAME` (e.g. "Marines", "akweks") and my name→id map kept the first. The runtime
ids are self-consistent.

## 5. The balance table — bounding the real extent

This is the item the lane was told not to let become folklore, and the existing artifact
was indeed wrong.

**What `0x00C06AFC` actually is: a biased base, not an array start.** The accessor is a
five-instruction leaf:

```
0x00581CA0  push ebp
            mov  ebp, esp
0x00581CA3  imul eax, dword ptr [ebp+8], 0x1ED      ; attacker_type_id * 493
0x00581CAA  add  eax, dword ptr [ebp+0xC]           ;  + defender_type_id
0x00581CAD  movsx eax, word ptr [eax*2 + 0xC06AFC]
            pop  ebp
            ret  8
```

and the same pattern is inlined in the damage function:

```
0x00644178  imul ecx, dword ptr [eax+4], 0x1ED      ; [Type+4] is the global type id
0x0064418E  movsx eax, word ptr [ecx*2 + 0xC06AFC]
```

The array itself is named by its own `DataWalk`, `FUN_00582BD0`:

```c
psVar2 = &DAT_00c12bf4;
do { iVar1 = 0x1ed;
     do { visit(L"final_balance_table[scan][scan2]", (int)*psVar2);
          psVar2 = psVar2 + 1; iVar1--; } while (iVar1 != 0);
} while ((int)psVar2 < 0xc896c6);
```

so **`final_balance_table` starts at `0x00C12BF4` and ends at `0x00C896C6`**, and
`0xC896C6 − 0xC12BF4 = 486 098 = 493 × 493 × 2` exactly — 493 rows of 493 `int16`.

The two constants reconcile perfectly:

```
0xC12BF4 − 0xC06AFC = 49 400 = 2 × (50 × 493 + 50)
```

Units start at global type id **50**, so the minimum real index `a·493 + d` is
`50·493 + 50 = 24 700`. MSVC folded the `−(50·493 + 50)` bias into the base address. The
real indexing is

```
final_balance_table[(attacker_id − 50) * 493 + (defender_id − 50)]      ids 50 … 542
```

and 542 − 50 + 1 = 493 = 364 units + 129 buildings. The table covers exactly the
attackable object types.

**Consequences for the old artifact.** `schema/live/balance-runtime.bin` was captured as
486 098 bytes starting at `0x00C06AFC`, i.e. **49 400 bytes too early**. It runs through
unrelated `.data`: at offset 14 184 it contains `0x014843FC`, the rebased
`PtrArray<UnitType>::vftable`; at offset 11 240, `PtrArray<GoodType>`; at row 5 it
contains the ASCII `"errain art\"`. That is where the negatives, the 15 477 zeros and
the −32768…32767 span came from. Only the last 436 698 bytes of it are balance data.

**The correct capture** (`schema/live/final-balance-runtime.bin`, MD5
`7471919b6a8ac68e04fbabb256a1118c`, verified against the guest-side hash) is clean:

* 486 098 bytes, 243 049 `int16`
* **0 zeros**, **no negatives**, min 5, max 2574
* 367 distinct values; `100` occurs 120 872 times, then 33, 170, 66, 115, 120, 160, 150…

Semantic spot-checks confirm the `id − 50` indexing:

| attacker → defender | value |
|---|---|
| Pikemen → Knight | 166 |
| Knight → Pikemen | 100 |
| Knight → Bowmen | 204 |
| Elite Pikemen → Heavy Knight | 166 |
| Catapult → Small City | 430 |
| Citizen → Citizen | 100 |

Under the raw (unbiased) indexing the same lookups give 75 / 170 / 100 / 75 / 1075, which
is nonsense.

**The table is not in the image.** `0x00C12BF4 … 0x00C896C6` is all zeros in
`riseofnations.exe` (it is inside the raw `.data` but zero-filled). It is built at load,
so **only a live read or an emulated load gives real values.**

**It is `final_`, i.e. derived.** `ron-data/balance.xml` is a 291 × 291 matrix keyed by
display `NAME`, values 10…700, 42 distinct. Expanding it to the 493-id space, only
15 383 / 55 696 (27.6 %) of comparable cells appear verbatim; 55 400 of the compared XML
cells are the default `100` while the runtime has a class-derived value. The last 55 XML
row keys are not unit names at all — `SIEGE`, `FORTS`, `TOWERS`, `CITIES`,
`Flag_2_OBJMASK_MISSILE`, `Flag_3_OBJMASK_AIR`, `Flag_4_OBJMASK_LIGHT_CAV`,
`Flag_5_OBJMASK_PIKE`, `Flag_6_OBJMASK_ANTI_AIR`, … — so the final table is
`balance.xml` name rows **plus** `obj_mask`-class rows composed at load. Deriving that
composition is a separate lane; the captured table is the ground truth in the meantime.

Also spotted at `0x00581CC0` (the wrapper above the accessor): global type ids in
`[0x192, 0x19E)` = 402…413 short-circuit to a flat `100`.

## 6. What I could not establish

* **`turn_speed` exact arithmetic.** It is a 32-bit binary angle and a deterministic
  function of the XML degrees (52 distinct inputs → 52 distinct outputs, verified
  functional), always within ±4 ULP of `deg·2³²/360`, but none of ⌊·⌋, round(·),
  `(deg<<32)/360`, float32 or 16.16 fixed-point reproduces all 52. Samples: 1 →
  11 930 464 (= ⌊2³²/360⌋), 3 → 35 791 392 (= 3×⌊2³²/360⌋, **not** ⌊3·2³²/360⌋ =
  35 791 394), 5 → 59 652 323 (= ⌊5·2³²/360⌋), 45 → 536 870 912, 90 → 1 073 741 824. The
  full measured mapping is in `schema/live/unit-attributes.txt`; use the table, not a
  formula.
* **`relative_value[352]`** at `UnitType+0x314`. 291 of 364 units have non-zero entries
  (e.g. `MILITIA` has −128 at indices 166–173 and 189–192). The array is 352 wide but
  there are **364** unit types — if it is indexed by unit ordinal, the last 12 EE units
  have no slot. I did not determine the index space. Flagging it because a 352-vs-364
  mismatch in a per-unit-type array is exactly the kind of latent overflow worth knowing
  about.
* **Where `guy_radius` / `big_radius` / `push_size` really come from** (§4e).
* **`GRID_X` reassignment** for the three units in §4d.
* `GovType` is empty in this process; whatever populates it had not run.
* `TypeBak` (566 records, ids 50–628 = units ∪ buildings ∪ items ∪ techs) — captured but
  not analysed.

## 7. Artifacts written (all gitignored)

| path | bytes | content |
|---|---|---|
| `schema/live/types-runtime.bin` | 14 476 096 | raw dump: `"DONTYPE1"`, module base, then per array: name[16], static VA, count, recsize(1792), data ptr, `count×u32` slot pointers, `count×1792` record bytes |
| `schema/live/type-names.txt` | 80 613 | TSV `array ⇥ slot ⇥ type_id ⇥ name ⇥ name2 ⇥ graph`, 1916 rows |
| `schema/live/unit-attributes.txt` | 103 960 | 364 rows, every decoded `Type`+`ObjectType`+`UnitType` scalar |
| `schema/live/building-attributes.txt` | 30 071 | 129 rows |
| `schema/live/tech-attributes.txt` | 10 365 | 85 rows incl. `ai[11]` |
| `schema/live/final-balance-runtime.bin` | 486 098 | **correct** balance table, base `0x00C12BF4` |
| `schema/live/balance-runtime.bin` | 486 098 | **pre-existing and wrong** — window at `0x00C06AFC`; left in place, do not use |

## 8. Reproduction

Static side (no Ghidra lock needed):

```bash
cd /Users/ember/dev/don/ron-bin && uv run --quiet --with capstone --with pefile python - <<'PY'
import pefile, struct
from capstone import *
pe = pefile.PE("riseofnations.exe")
base = pe.OPTIONAL_HEADER.ImageBase
txt  = [s for s in pe.sections if s.Name.rstrip(b"\x00") == b".text"][0]
tva, td = base + txt.VirtualAddress, txt.get_data()
md = Cs(CS_ARCH_X86, CS_MODE_32)
for ins in md.disasm(td[0x581ca0 - tva:0x581ca0 - tva + 24], 0x581ca0):
    print(hex(ins.address), ins.mnemonic, ins.op_str)     # the balance accessor
PY
```

Live side: the guest scripts are
`/private/tmp/claude-501/-Users-ember-dev-don/62b78482-846c-4ffd-a44c-2199d3744a8e/scratchpad/`
→ `pstools.py` (transfer), `dump_types.ps1`, `dumpnames.ps1`, `dumpbal.ps1`; the parsers
are `parse_dump.py`, `validate.py`, `validate2.py`.

```bash
python3 pstools.py put dump_types.ps1 'C:\Users\ember\dt.ps1'
python3 pstools.py run 'C:\Users\ember\dt.ps1' 900
python3 pstools.py get 'C:\Users\ember\dontypes.bin.gz' dontypes.bin.gz
```

### Two traps in the live-probe path, paid for in wall-clock

1. **`prlctl exec` silently hangs — no output, no error, no timeout — once the
   `-EncodedCommand` argument gets long.** ~3 400 base64 chars works; ~5 600 hangs
   forever. Both `Add-Type` and `Reflection.Emit` probes appeared to hang; the scripts
   were fine, the *argument length* was the problem. `pstools.py` therefore uploads any
   real script as ~1 100-char `Add-Content` chunks and then runs it from a guest file.
   Output is fully buffered until the process exits, so a hung call shows you nothing —
   always have the guest script write a progress log you can read with a second call.
2. **PowerShell variable names are case-insensitive.** A `$K = $tb.CreateType()` holding
   the P/Invoke type was silently clobbered by the loop variable `$k = $names[$j]`, and
   every read came back zero with `ok=` blank. Rename to something that cannot collide.

Also: `Add-Type` compiles via `csc.exe` and is slow under the SYSTEM context `prlctl exec`
gives you; `Reflection.Emit` `DefinePInvokeMethod` does the same job in ~0.2 s.
