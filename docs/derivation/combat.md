# Derivation: the damage and combat pipeline

Lane: **combat**. Everything below is derived from `ron-bin/riseofnations.exe` and the
shipped XML under `ron-data/`. Community sources were used only to state hypotheses that
are then tested; where the derivation and the folklore disagree it is called out loudly.

Every claim is marked **[measured]** (I verified it myself against the binary or against
retail machine code running under the oracle on hbox) or **[reported]/[inferred]**.

---

## Headline

**`FUN_00644130` is the damage computation.** It is a `__thiscall` taking six stack
arguments (`ret 0x18`), it returns the damage to apply, and its entire body is **pure
32-bit integer arithmetic — zero SSE, zero x87, zero float** [measured]. Its caller
`FUN_0064a480` applies the result and maintains the overkill timestamp.

The pipeline is **not** "attack × modifiers − armor, floor 1". What the binary actually
does is:

```
D  = attack_after_upgrades * balance[atkType][defType] / 100
D  = ... 17 further guarded integer modifiers (masks, tech, terrain, splash, flank) ...
D  = (D + 5) / 10                <-- round-half-up rescale; attack is stored x10
D  = D - armor_after_upgrades    <-- armor subtracted HERE, mid-chain
D  = ... 6 further guarded modifiers (overkill, rocky, height, entrenchment, ...) ...
if (D < 1 && <three conditions>) D = 1
if (<city recapture>) D = D * RULES.recapture_city_modifier / 256
return D
```

Five multiplicative modifiers apply **after** armor subtraction, the floor of 1 is
**conditional**, and the internal attack scale is **ten times** the number in
`unitrules.xml`.

---

## How to reproduce

Disassembly (no Ghidra lock taken; capstone only):

```sh
cd /Users/ember/dev/don/ron-bin
uv run --quiet --with capstone --with pefile python - <<'PY'
import pefile
from capstone import *
pe = pefile.PE("riseofnations.exe")
t  = [s for s in pe.sections if s.Name.rstrip(b"\0") == b".text"][0]
base = pe.OPTIONAL_HEADER.ImageBase + t.VirtualAddress
data = t.get_data()
md = Cs(CS_ARCH_X86, CS_MODE_32); md.skipdata = True
ea, end = 0x00644130, 0x00645100          # FUN_00644130, the damage function
for i in md.disasm(data[ea-base:end-base], ea):
    print("%08x %-8s %s" % (i.address, i.mnemonic, i.op_str))
PY
```

Oracle (hbox, 32-bit musl):

```sh
ssh hbox
cd ~/don-oracle
nice -n 15 taskset -c 0-3 cargo build --target i686-unknown-linux-musl -q
nice -n 15 taskset -c 0-3 ./target/i686-unknown-linux-musl/debug/oracle combat 500000
```

The `combat` sub-command was added by this lane in `crates/oracle/src/main.rs`
(`combat_difftest`), mirrored to `hbox:~/don-oracle/crates/oracle/src/main.rs`.

---

## 1. The function map

| VA | what it is | how established | islands class |
|---|---|---|---|
| `0x00644130` | **damage computation**, `__thiscall`, `ret 0x18` | reads `balance[]`, `splash_percent`, and 11 distinct `RULES` combat constants; returns the value the applier subtracts from HP | SELF_CALL |
| `0x0064a480` | **damage applier** (`ret ...`, in a Ghidra function-list gap) | calls `0x644130` at `0x0064a4f7`, stores result in `[ebp-0x24]`, stamps the overkill frame at `[def+0x4c]` | *(unclassified — gap)* |
| `0x0064e5c0` | only direct caller of `0x00644130` | call-graph from `schema/islands.jsonl` + capstone | SELF_CALL |
| `0x00678060`, `0x0092bc80` | callers of the applier | same | SELF_CALL |
| `0x006469f0` | **`Object::get_attack()`**, vtable slot `+0x120` | `mov eax,[this+0x18]; mov edi,[eax+0x1E8]` = `UnitType::attack`; appears at slot 72 of 6 object vtables | SELF_CALL |
| `0x00647db0` | **`Object::get_armor()`**, vtable slot `+0x124` | `mov eax,[this+0x18]; mov esi,[eax+0x214]` = `UnitType::armor`; slot 73 of 3 vtables | SELF_CALL |
| `0x0092cfe0` | **flank-level classifier** (0/1/2 from an angle delta) | called at `0x00644b26`, 11 instructions, arg in ECX | **ISLAND** |
| `0x00581ca0` | **`balance(atk,def)`**, `__stdcall`, `ret 8` | `imul eax,[ebp+8],0x1ED; add eax,[ebp+0xC]; movsx eax,word [eax*2+0xC06AFC]` | WRITES_GLOBAL |
| `0x0061ab50` | **unitrules.xml `UNIT` parser** (in a Ghidra gap, `0x61ab47`–`0x61be50`) | writes every `UnitType` combat offset; owns the asserts `"Improper use of melee-and-ranged unitflag"` (`0xADA0D0`) and `" does not have a valid projectile speed."` (`0xADA0FC`) | *(gap)* |
| `0x0065fc00` | combat-stat descriptor walker (already known) | `schema/bindings.json` | SELF_CALL |
| `0x0065f880` | **`UnitType` constructor** | zero/`-1`-initialises exactly the 34 offsets of `FUN_0065fc00` | SELF_CALL |

**Ghidra's function list has real holes.** Both the damage applier and the unit parser sit
in gaps in `schema/islands.jsonl` (`0x64a47b`→`0x64c000` and `0x61ab47`→`0x61be50`). Any
analysis that iterates that file alone will silently skip them. [measured]

Load-bearing globals [measured]:

| global | contents |
|---|---|
| `0x00C061E4` | **`RULES`** — the rules.xml constants object the damage code reads. Confirmed by taint-tracing every `mov reg,[0xC061E4]` in `.text`: 457 distinct offsets are read, and dozens land exactly on `FUN_00570170`'s bindings (`+4` `unit_move_speed`, `+0x4C` `flank_bonus`, `+0x1E8` `siege_attrition`, `+0x27C` `gather_rate`, `+0x29C` `oil_rate`, `+0x2F8` `refinery_bonus`, `+0x2FC` `base_tribute`, …) |
| `0x00C061F0` | a **second** pointer walked by `FUN_00570170` (`FUN_005887d0`: `mov ecx,[0xC061F0]; call 0x570170`). The script-side getter at `0x0057FC74` reads `one_age_down…five_ages_down` (`+0x70…+0x80`) from *this* one, while the sim reads `flank_bonus` etc. from `0xC061E4`. Two distinct rule objects, relationship **not established** |
| `0x00C061E8` | game object (`+0x550` current frame, `+0x821` flags) |
| `0x00C061EC` | game/world object (`+0x550` current frame) |
| `0x00C061E0` | per-player array base, **stride `0x6EEC` = 28,396 bytes/player** |
| `0x00C0AB84`, `0x00C0AEC0` | object tables: `base[player*28]` → pointer array, then `[index*4]` |
| `0x00C06AFC` | **balance table** base, `int16` |

---

## 2. `UnitType` combat layout, and the ×10 attack scale

`schema/bindings.json` gives the offsets; `FUN_0061ab50` gives the parse, and it is where
the interesting part is. Offsets confirmed three ways: the descriptor walker
(`FUN_0065fc00`), the constructor (`FUN_0065f880`, which sets `+0x210/+0x214/+0x218` to
`-1` and the rest to 0), and the parser. [measured]

| offset | field | how it is parsed at `FUN_0061ab50` |
|---|---|---|
| `+0x1E4` | `obj_masks` | bit-set from the `OBJ_MASK` string, `bit = ch - 'A'` (`0x61b358`: `sub ecx,0x41; bts eax,ecx`) |
| `+0x1E8` | `attack` | **`raw * 10`** — `0x61b01b: lea eax,[eax+eax*4]; add eax,eax` |
| `+0x1EC` | `to_hit` | raw int |
| `+0x1F0` | `attenuate` | **`abs(raw)`** — `0x61afeb: cdq; xor eax,edx; sub eax,edx` |
| `+0x1F4` | `recharge` | raw int |
| `+0x1F8`/`+0x1FC` | `min_range`/`max_range` | the `RANGE` string split on `'-'` (`0x61b0cd: push 0x2D`); `+0x2D0` also gets `max_range` |
| `+0x200` | `splash_area` | raw int |
| `+0x204` | `splash_percent` | raw int (percent) |
| `+0x208` | `ammo_per_att` | raw int |
| `+0x20C` | `proj_speed` | raw int; **forced to 200** at `0x61b512` if `attack != 0 && max_range != 0 && proj_speed == 0` |
| `+0x210` | `hits` | **raw int, not scaled** |
| `+0x214` | `armor` | **raw int, not scaled** |
| `+0x218` | `domain` | 1 / 2 / 0 by string match; usage implies **Land=0, Sea=1, Air=2** [inferred from `0x644f3e`/`0x6445a2`] |
| `+0x2B4` | `FLAGS` bit-set, `bit = ch - 0x61` for letters, `ch - 0x17` for `'1'..'9'` |
| `+4` | the global type id used as the balance-table index |

Also at parse time: if `FLAGS` bit `0x400` is set, `max_range` is forced to 0, and if the
parsed range was `> 0` the engine asserts *"Improper use of melee-and-ranged unitflag"*.

### The ×10 attack scale — three independent confirmations [measured]

1. The parser multiplies `ATTACK` by 10 into `+0x1E8` (`0x0061b01b`).
2. `Object::get_attack()` adds **`10 × level`** per military upgrade level
   (`0x00646ae2: lea eax,[ecx+ecx*4]; lea eax,[edi+eax*2]`), while
   `Object::get_armor()` adds **`1 × level`** (`0x00647e6f: lea eax,[ecx+esi]`).
3. The damage function divides by 10 (`(D+5)/10`, `0x00644b7d`) immediately before
   subtracting armor.

So the engine carries damage in tenths internally so that the percentage modifiers have
one decimal digit of headroom, and rescales — **rounding half up** — exactly once, right
before armor. HP (`hits`) and `armor` live on the unscaled display scale throughout.

---

## 3. The damage pipeline, in order

Signature reconstructed from the call site at `0x0064a4e2`–`0x0064a4f7` [measured]:

```
int __thiscall Damage(Object* attacker /*ECX*/,
                      int  def_index,      // ebp+0x08  (scaled to *4 immediately)
                      int  def_player,     // ebp+0x0C  (scaled to *28)
                      int  a3,             // ebp+0x10  used in two position tests
                      int  splash_flag,    // ebp+0x14
                      int  a5,             // ebp+0x18  gates the overkill block
                      int* out_kind)       // ebp+0x1C  written 2 or 3
```

Locals: `D` = EDI (the accumulator), `A` = `[ebp-0x14]` = `attacker->get_attack()`,
`ARM` = `[ebp-4]` = `defender->get_armor()`, `B` = `[ebp-0x18]` = balance percentage,
`AM` = `[ebp-0xC]` = attacker `obj_masks`, `DM` = `[ebp-8]` = defender `obj_masks`.

`this` is the **attacker** (it supplies `get_attack` and the balance-table *row*); the two
index arguments name the **defender** (armor, and the balance-table *column*). [measured]

| # | VA | operation | guard |
|---|---|---|---|
| 0 | `0x64418e` | `B = (i16)balance[atkType*493 + defType]` | always |
| 0 | `0x6441b1` | `ARM = defender->vtbl[0x124]()` (`get_armor`) | always |
| 0 | `0x6441be` | `A = attacker->vtbl[0x120]()` (`get_attack`) | always |
| 1 | `0x6442b1` | **`D = A * B / 100`** | always |
| 2 | `0x6442c9` | `ARM = ARM * 133 / 100` | `AM & 0x8` |
| 3 | `0x644352` | `D = D / 3` | attacker `vf+0x20`, `DM & 0x40000`, defender alive, defender-type `+0x2B8 & 4`, `!(defObj+0x68 & 0x80000)` |
| 4a | `0x6443bd` | `D = D * 3 / 4` | (same outer guard) **and** `DM & 0x20` |
| 4b | `0x64441e` | `D = D / 2` | else, and defender-type `!vf+0x10C`, `!vf+0xCC`, `vf+0xD0` |
| 5 | `0x64446a` | `D = D * 4` | both `vf+0x20`, defender `vf+0x3C` object is class `0xB42174` with `!(flags&4)` |
| 6 | `0x64451d` | `D = D / 2` | defender `vf+0x20`; attacker type id ∈ {`0x32`,`0x33`,`0x34`,`0x35`} **or** attacker-type `vf+0x60` after tech `0x42`; and the tile-owner byte (`[tileRec+0xF]`) ≠ attacker player |
| 7 | `0x64458b` | `D = D / 2` | attacker-type `vf+0x60` after tech `0x139`; defender alive; defender `vf+0x3C` object is class `0xB42174` with `!(byte[+8] & 4)` |
| 8 | `0x6445f7` | `D = D * (100 - RULES[0x4C4]) / 100` | attacker alive, `domain==2` (air), `!(AM & 0x8000000)`, defender has tech `0x216`. `RULES[0x4C4]` = **`red_fort_air_defense`** |
| 9a | `0x644632` | `D = D * 2` | `defObj+0x68 & 0x80000` |
| 9b | `0x644644` | `ARM = ARM + 1` | else, defender-type `+0x2B8 & 4` |
| 10 | `0x64473d` | `D = D + RULES[0xBBC]` | attacker `+0x59DE` non-zero, `0x646b00` ≥ 0, target-type `+0x40` valid, that type `vf+0x60` |
| 11 | `0x6447f0` | `D = D * (m + 100) / 100`, `m = -(RULES[0x794] * min(x,y))` if `RULES[0x794] < 0` else `RULES[0x794]` | attacker alive, attacker-type `+0x40 == 0x1AB`, and player-property `0x6E1370(player, 0xF)` |
| 12 | `0x644889` | `D = D * 2` | domain/tech/game-mode chain around `[0xC061E8]+0x24 == 2` |
| 13 | `0x6448b9` | **`D = D idiv defenderType[+0x308]`** (real `idiv`; divisor never checked) | `splash_flag != 0` and defender alive |
| 14 | `0x6448ea` | `D = D * attackerType.splash_percent / 100` | `splash_flag != 0` |
| 15 | `0x64494e` | `D = D * 3` | defender alive, defender-type `vf+0x10C`, `+0x2B8 & 4`, `!(defObj+0x68 & 0x80000)` |
| 16 | `0x644980` | `D = D * 25 / 100` | defender-type `vf+0x60` after tech `0x143` |
| 17 | `0x6449c7` | `D = D * RULES.river_modifier / 256` | defender alive and `(defObj+0xC ^ 0x63637) < 0` |
| 18 | `0x644a11` | `ARM = 0; D = max(D, get_attack(attacker)) * 1000` | `defObj+0x68 & 1` |
| 19 | `0x644a98` | `D = D * 4` | defender `vf+0x1C`, `vf+0x40 -> vf+0x184`, `!( [0xC061E8]+0x821 & 2 )`, target-type `attack == 0` |
| 20 | `0x644b26`–`0x644b7b` | **flank**: `lvl = flank_level(ECX)`; `pct = (DM & 0x200000) ? RULES.vehicle_flank_bonus * RULES.flank_bonus / 256 : (DM & 0x1000) ? RULES.cavalry_flank_bonus * RULES.flank_bonus / 256 : RULES.flank_bonus`; **`D = D * (100 + pct*lvl) / 100`** | both alive, `!(AM&4)`, `!(DM&4)`, `!(AM&0x10000000)`, `!(DM&0x10000000)`, `(AM&0x2000)==(DM&0x2000)==0`, plus the angle test |
| **21** | `0x644b7d` | **`D = (D + 5) / 10`** | always |
| **22** | `0x644b91` | **`D = D - ARM`** | always |
| 23 | `0x644c35` | `D = D * RULES.overkill_damage / 256` | `a5 != 0`, attacker `vf+0x130`, both alive, `defObj+0x4c != 0`, `curFrame - defObj[+0x4c] < RULES.overkill_frames`, and attacker `vf+0xE4` ≠ defender's `[vtbl+0xA4]` |
| 23b | `0x644c83` | `D = D / 2` | (inside 23) defender-type `vf+0x60` after tech `0x109`, `!attackerType vf+0x10C` |
| 24 | `0x644ce5` | `D = D * RULES.rocky_modifier / 256` | `DM & 0x10108` and terrain byte `& 8` |
| 25 | `0x644d74` | **`D = D + ((Δz * RULES.height_bonus) * D) idiv (RULES.height_increment * 100)`**, `Δz = (attacker[+0xC]^0x63637) - (defender[+0xC]^0x63637)` | neither domain is 2, `!attackerType vf+0x10C`, `Δz > 0` |
| 26 | `0x644e44` | `D = D * RULES.entrenchment_modifier / 256` | defender alive, `defObj+0x68 & 0x2000000`, direction test, `splash_flag != 0 \|\| !dir` |
| 26b | `0x644e66` | `D = D * RULES[0xB98] / 256` | (inside 26) `defObj+0x6C & 0x1000` |
| 27 | `0x644f04` | `D = D * (RULES[0x76C] + 100) / 100` | `RULES[0x76C] != 0`, attacker tech `0xD`, attacker alive, attacker-type `+0x40 == 0x1AC`, defender `vf+0xCC` or defender-type `vf+0x10C` |
| **28** | `0x644f13` | **`if (D < 1) D = 1`** — but only if `((DM & 0x10000000) == ((AM >> 3) & 0x10000000))` **and** `splash_flag == 0` **and not** (attacker.domain == 0 && defender.domain == 1) | `D < 1` |
| 29 | `0x644f9a` | `D = 0` | defender type id `== 0x21D`, attacker `domain == 2`, `RULES[0x558] != 0` |
| 30 | `0x645024` | **`return D * RULES.recapture_city_modifier / 256`** | defender `vf+0x20`, its `vf+0x3C` object is class `0xB42174` with `flags & 4` and `& 0x20`, city index ≥ 0, and that city's owner `== attacker player` |
| 31 | `0x64503c` | `return D` | otherwise |

`out_kind` (`ebp+0x1C`) is written `2` at entry, then `3` if `AM & 0x800000` or `AM & 2`,
and `3`/`2` by a class test at `0x6446a0`. It is a *damage-kind* enum, not part of the
magnitude.

### Overkill bookkeeping (in the applier, not the computer)

`FUN_0064a480`, `0x0064a5dd`–`0x0064a61a` [measured]:

```
if (defObj[+0x68] & 0x10)                 D *= 2          ; 0x64a5e3
last = defObj[+0x4C]
if (last == 0 || (GAME[+0x550] - last) >= RULES.overkill_frames)
        defObj[+0x4C] = GAME[+0x550]                       ; stamp
```

So the overkill *timestamp* is only refreshed once the window has elapsed, which is why
the third and later hits inside a window still attenuate.

---

## 4. Rounding and division idioms [measured]

Every division in the pipeline truncates toward zero (C semantics). The compiler emitted
four distinct magic sequences; a faithful port must reproduce the **wrapping 32-bit
multiply before the divide**, not promote to 64-bit.

| idiom | instruction shape | meaning |
|---|---|---|
| ÷100 | `imul ecx, x, k` / `mov eax,0x51EB851F` / `imul ecx` / `sar edx,5` / `mov r,edx` / `shr r,31` / `add r,edx` | `r = (i32)(x*k) / 100`, truncating |
| ÷10 | `lea ecx,[edi+5]` / `mov eax,0x66666667` / `imul ecx` / `sar edx,2` / `+signbit` | `r = (D+5) / 10`, truncating. Round-half-**up** for `D ≥ 0`; for `D < 0` it is a shifted truncation, not symmetric rounding |
| ÷3 | `mov eax,0x55555556` / `imul edi` / `+signbit` | `r = D / 3`, truncating |
| ÷256 | `imul eax, m, D` / `cdq` / `and edx,0xFF` / `lea r,[edx+eax]` / `sar r,8` | `r = (i32)(m*D) / 256`, truncating |
| ×3/4 | `lea eax,[edi+edi*2]` / `cdq` / `and edx,3` / `lea edi,[edx+eax]` / `sar edi,2` | `r = 3*D / 4`, truncating |
| ÷2 | `mov eax,edi` / `cdq` / `sub eax,edx` / `sar edi,1` | `r = D / 2`, truncating |

Two sites use a genuine `idiv` with an unchecked divisor: `0x006448b9`
(`D / defenderType[+0x308]`) and `0x00644d78` (height). A divisor of 0 there is a `#DE`;
the engine relies on the data never producing one.

**There is no floating point anywhere in `FUN_00644130`** — no `xmm` operand, no `f*`
mnemonic in `0x00644130..0x00645100` [measured]. Damage is therefore trivially
bit-reproducible in Rust `i32`; the whole IEEE hazard surface documented in
`docs/binary-ground-truth.md` is irrelevant to this subsystem.

**Obfuscated scalars.** Several sim scalars are stored XOR-masked and must be unmasked
before use: `^0x63637` (object `+0xC`, `+0x10`, `+0x14` — position/height),
`^0x62766` (player `+0xDC` — military level), `^0x63187` (player `+0xE8`). [measured] A
reimplementation does not need the masks, but anything that *reads live memory* does.

---

## 5. The balance table

`0x00581ca0`, seven instructions [measured]:

```
int __stdcall balance(int atk_type, int def_type)
{ return (int16) *(short*)(0x00C06AFC + 2*(atk_type*493 + def_type)); }
```

The damage function inlines exactly the same computation at `0x00644178`–`0x0064418e`,
taking both type ids from `UnitType[+4]`.

**`493 = 364 + 129`** — the number of `<UNIT>` entries in `ron-data/unitrules.xml` plus the
number of `<BUILDING>` entries in `ron-data/buildingrules.xml` [measured]. So the row/column
space is the combined unit+building type space, and the attacker is the row.

`ron-data/balance.xml` is a **291 × 291 matrix of integer percentages** whose rows and
columns are in identical order [measured]. 236 of the 291 names match `unitrules.xml`
`NAME` values exactly; the remaining 55 are **category names** — `SIEGE`, `FORTS`,
`TOWERS`, `CITIES`, `OBSPOST`, `BUILDINGS`, `UNITS`, `AGE_0`…`AGE_7` — plus a few spelling
variants (`Kings_Longbowmen`, `Man_o_War`). 84,156 of the 84,681 cells are `100`; there
are 42 distinct values, from `5` to `400`. So **balance.xml is a compact, category-expanding
specification and the runtime object is a dense per-type-pair `int16` table**.

That resolves half of the folklore: there is no *separate* hidden per-mask table for the
pairwise multiplier — the pairwise multiplier is balance.xml, expanded. (See §7 for the
part of the folklore that *is* true.)

### An unresolved anomaly, flagged rather than papered over

The oracle confirms the address arithmetic against retail code exactly (§6), so the base
`0x00C06AFC` and stride 493 are not in doubt. But a dense `int16[493][493]` starting there
would span `0x00C06AFC..0x00C7D06E`, and **that range contains other statically initialised,
code-referenced globals** — the first is at `0x00C07AD0` (`cmp esi, 0xC07AD0` at `0x00A3566C`),
which is only row 4 of the table. The file image at the base is also occupied by a table of
`0x48`-stride records holding `".bhs"`, `".xml"`, `".dtd"`, `".txt"`, `".cur"` … [measured].

So one of these is true and I cannot tell which from the static image:

* the table is smaller than the full type square and attacker rows above ~4 read
  neighbouring globals (an engine quirk that would matter enormously), or
* the region is overwritten at load time, or
* the row stride 493 is right but the row index is not the same id space I assume.

**This needs a live-process read** (Frida on the VM, or a minidump) before anyone
implements the lookup. Do not implement it from this document alone.

---

## 6. Tier-B evidence (oracle, hbox, i686-unknown-linux-musl)

```
$ ./target/i686-unknown-linux-musl/debug/oracle combat 500000
[oracle] mapped at 0xeafe4000, 315865 relocations applied
combat differential test: retail machine code vs Rust model
  PASS  0x0092cfe0  flank_level(angle_delta)               500017 trials, 0 mismatches
  PASS  0x00581ca0  balance[atk*493 + def] (int16)         247049 trials, 0 mismatches
```

**This is testing, not verification.** Both entries are new provenance-ledger rows.

### `flank_level` — `0x0092CFE0`, tier **B**

Reachability **ISLAND** (`schema/islands.jsonl`). Single argument in ECX, no memory.

```rust
fn flank_level(x: u32) -> u32 {
    if x > 0xD555_5555 { 0 }
    else if 0x4000_0000u32 < x.wrapping_sub(0x6000_0000) { 2 }
    else { 1 }
}
```

Evidence: **500,017 inputs — 17 hand-chosen boundary values** (`0`, `1`, `0x3FFFFFFF`,
`0x40000000`, `0x40000001`, `0x5FFFFFFF`, `0x60000000`, `0x60000001`, `0x9FFFFFFF`,
`0xA0000000`, `0xA0000001`, `0xD5555554`, `0xD5555555`, `0xD5555556`, `0x7FFFFFFF`,
`0x80000000`, `0xFFFFFFFF`) **plus a 500,000-point sweep of the full `u32` domain on a
stride of 8191** (coprime with 2³², so the sample is spread uniformly and hits every
residue class mod 8191) — **0 mismatches**.

Reading: the input is an unsigned wrapped angle delta. Flank level 0 means "no flank"
(front arc), 1 the side arc, 2 the rear arc; the damage code then applies
`(100 + pct*level)/100`, which is exactly why `FLANK_BONUS` says
*"50% per level of flank (max bonus is twice this number)"*. [measured arithmetic;
front/side/rear naming is inferred]

### `balance` lookup — `0x00581CA0`, tier **B**

Evidence: **247,049 inputs — exhaustive over the entire defined type domain**
(`493 × 493 = 243,049` pairs), plus 4,000 pairs with the attacker index in `500..1500`
to exercise the multiply/add past the type range — **0 mismatches** against a Rust model
that reads the relocated image at `0x00C06AFC + 2*(a*493 + b)` as `i16`. This confirms the
base address, the stride, the element type, the sign extension, and the 32-bit wrapping —
it does **not** confirm the table's contents, which are loaded from `balance.xml` at
runtime and are therefore absent from the static image (99.1% of the extent reads 0, and
the non-zero residue moves between runs because relocations land inside it).

---

## 7. Where the folklore is wrong, and where it is right

The lane brief carried two community claims. Both were treated as hypotheses.

**Claim 1: "damage = attack × modifiers − armor, floor of 1."**
**Partly refuted.** [measured]

* Armor is subtracted at step 22 of 31, **not last**. Overkill attenuation, the rocky-terrain
  modifier, the height bonus, entrenchment and the Red Fort / tech percentage all apply
  **after** armor. A unit whose armor exceeds the pre-armor damage can therefore end up with
  a *negative* intermediate that the later multipliers scale further, and only then meets
  the floor.
* The rescale immediately before armor is `(D + 5) / 10` — **round-half-up** for
  non-negative `D` — because attack is stored ×10. Any formula written in display units
  silently drops that rounding.
* The floor of 1 is **conditional** on three tests (`obj_masks` bit-28/bit-31 agreement,
  `splash_flag == 0`, and *not* land-attacker-vs-sea-defender). Outside those, damage of 0
  or negative is passed through — and step 29 explicitly forces 0 for one type/domain pair.
* Armor itself is modified inside the damage function: `×133/100` when the attacker's
  `obj_masks` has bit 3, `+1` in one defender case, `= 0` in another.

**Claim 2: "a hardcoded per-mask modifier table exists that is absent from balance.xml."**
**Half true, and the half that is true is worse than a table.** [measured]

There is no hidden *table*. What exists is an inline chain of **hardcoded integer
multipliers driven by `obj_masks` bits and virtual predicates**, none of which appears in
any shipped XML: `×133/100` (armor, `AM & 8`), `÷3`, `×3/4` (`DM & 0x20`), `÷2` (three
separate sites), `×4` (two sites), `×2` (three sites), `×3`, `×25/100`, and `×1000`. These
are compiled constants in `FUN_00644130`. Anyone who implements combat from balance.xml
plus rules.xml alone will be wrong by factors of 2–4 on a large fraction of matchups.

**A third thing the community formula omits entirely:** `RULES.overkill_damage`,
`rocky_modifier`, `river_modifier`, `entrenchment_modifier` and `recapture_city_modifier`
are applied as `value/256`, not as percentages — see below.

---

## 8. Runtime representation of the rules.xml combat constants

The divisor the code uses tells us the fixed-point format the tokenizer produced. This is
new information for the rule-value-parser question in `docs/binary-ground-truth.md`.
[measured — the arithmetic; the concrete stored integers are **not** measured]

| rule (rules.xml text) | RULES offset | applied as | implied representation |
|---|---|---|---|
| `FLANK_BONUS` `"50% per level"` | `+0x4C` | `(100 + pct*lvl)/100` | plain integer percent |
| `CAVALRY_FLANK_BONUS` `"40% (of base)"` | `+0x50` | `(v * flank_bonus)/256` | **1/256 fixed point** |
| `VEHICLE_FLANK_BONUS` `"33% (of base)"` | `+0x54` | `(v * flank_bonus)/256` | **1/256 fixed point** |
| `ROCKY_MODIFIER` `"2/3"` | `+0x58` | `(v * D)/256` | **1/256 fixed point** |
| `OVERKILL_FRAMES` `"30"` | `+0x5C` | compared to a frame delta | frames |
| `OVERKILL_DAMAGE` `"1/3"` | `+0x60` | `(v * D)/256` | **1/256 fixed point** |
| `ENTRENCHMENT_MODIFIER` `"2/3"` | `+0x64` | `(v * D)/256` | **1/256 fixed point** |
| `RIVER_MODIFIER` `"2/1"` | `+0x68` | `(v * D)/256` | **1/256 fixed point** |
| `RECAPTURE_CITY_MODIFIER` `"2/1"` | `+0x6C` | `(v * D)/256` | **1/256 fixed point** |
| `HEIGHT_INCREMENT` `"200 z"` | `+0x44` | denominator `× 100` | plain integer |
| `HEIGHT_BONUS` `"10% per increment"` | `+0x48` | numerator | plain integer percent |
| `RED_FORT_AIR_DEFENSE` `"33% less damage"` | `+0x4C4` | `(100 - v)/100` | plain integer percent |
| unidentified | `+0x76C` | `(100 + v)/100` | integer percent |
| unidentified | `+0x794` | `(100 + m)/100` | integer percent, negative = per-something |
| unidentified | `+0xB98` | `(v * D)/256` | 1/256 fixed point |
| unidentified | `+0xBBC` | `D += v` | additive, in ×10 damage units |
| unidentified | `+0x8B8` | per-level upgrade step | `+10×lvl` attack, `+1×lvl` armor |
| unidentified | `+0xCD4` | attack override | ×10 damage units |

So the parser produces **two different fixed-point formats** depending on the field:
`"50%"` → `50`, but `"2/3"` and `"40% (of base flank bonus)"` → a 1/256 fraction. Whether
`2/3` becomes 170 or 171 is a rounding question this lane cannot settle statically, and it
changes damage by one unit at the boundaries. It is the highest-value remaining question
for the rules-parser lane.

---

## 9. What I could NOT establish

* **The balance table's storage extent** (§5). The base and stride are confirmed against
  retail; the dense-`[493][493]` reading conflicts with other globals inside that range.
  Needs a live read.
* **The contents** of the balance table at runtime — it is populated from `balance.xml`,
  and the loader that expands the category rows (`SIEGE`, `AGE_3`, …) into per-type rows
  was not located.
* **`to_hit` (`+0x1EC`) and `attenuate` (`+0x1F0`) are never read by `FUN_00644130`.** I
  scanned every function in `islands.jsonl` for `obj->type->field` reads of those offsets
  and found none; every apparent hit was a different class at a coincident offset. In the
  shipped data `TO_HIT` is `300` for 220 of 364 units and `ATTENUATE` is negative for 176 —
  they are clearly live, just not in the damage-magnitude path. They most likely belong to
  the projectile/hit-resolution code (`Ammo.cpp`). **Unresolved.**
* **What `defenderType[+0x308]` is** (the `idiv` divisor at step 13). It is written by the
  unit parser at `0x0061b8b8`/`0x0061b8df` and copied by the `FUN_0065fac0`/`FUN_0065fb30`
  pair alongside attack/ranges/hits/armor. Not named by any loader binding I could find.
* **The two rule objects.** `0x00C061E4` and `0x00C061F0` are both walked by
  `FUN_00570170`'s binding set but are read by different code. Whether they are two copies
  (base vs. effective) or the same allocation is not established.
* **The meaning of the individual `obj_masks` letters.** The bit positions are measured
  (`bit = ch - 'A'`), and the damage function tests bits 2, 3, 5, 12, 13, 18, 21, 27, 28,
  and 0x10108 as a group — but mapping bit → letter → game concept needs the
  `OBJ_MASK` strings cross-referenced against unit categories, which I did not do.
* **Which string-table id is which XML attribute** in `FUN_0061ab50`. The parser addresses
  attribute names as `[[0xC06378]+0x10] + id` with a uniform 20-byte record stride; the
  table is built at runtime, so the ids could not be resolved statically. Every field
  binding above was instead established from the *destination* offset, which is
  independently known from `schema/bindings.json`.
* No **Tier A** result. Nothing here is proven over a whole input domain.

---

## 10. Corrections to existing project documents

1. **`docs/binary-ground-truth.md` calls the descriptor's second word a "type tag"** and
   reports "observed type tags so far: 8 (`recharge`), 9 (`crew_size`, `base_form`)". It is
   not a type tag — **it is the length of the wide rule name** [measured]:
   `recharge`=8, `crew_size`=9, `base_form`=9, `obj_masks`=9, `attack`=6, `to_hit`=6,
   `hits`=4, `armor`=5, `splash_percent`=14, `special_upgrade_cost`=20. It is stored twice
   (a `uint16` at `+4` and again in the high half of the dword at `+6`), which is the
   classic `{const wchar_t* ptr; size_t len;}` string-view shape. `schema/bindings.json`'s
   `tag` field should be renamed `name_len`, and the note in
   `docs/provenance-ledger.md` that a "type-tag selects the value parser" hypothesis was
   *refuted* is explained by this: the field was never a type tag.
2. **`schema/islands.jsonl` is not a complete function list.** At least two combat-critical
   functions sit in gaps (§1).
3. `docs/binary-ground-truth.md` open question 4 ("the damage pipeline: exact operation
   order, rounding, and where the hardcoded per-mask modifier table lives") is answered by
   §3, §4 and §7 — with the caveat that there is no table, only an inline chain.

---

## 11. Proposed provenance-ledger entries

### `flank_level(angle_delta)` — flank tier classifier

| field | value |
|---|---|
| source | `riseofnations.exe` VA `0x0092CFE0` (arg in ECX, 11 instructions) |
| implementation | not yet ported; model in `crates/oracle/src/main.rs::combat_difftest` |
| tier | **B** |
| evidence | 500,017 inputs — 17 hand-chosen boundary values + a 500,000-point stride-8191 sweep of the full `u32` domain — **0 mismatches** |
| harness | `crates/oracle`, `combat` command, i686-unknown-linux-musl on hbox |
| reachability | ISLAND |

### `balance(atk_type, def_type)` — pairwise damage percentage lookup

| field | value |
|---|---|
| source | `riseofnations.exe` VA `0x00581CA0`; identical code inlined at `0x00644178`–`0x0064418E` |
| implementation | not yet ported; model in `crates/oracle/src/main.rs::combat_difftest` |
| tier | **B** for the *address arithmetic only* |
| evidence | 247,049 inputs — exhaustive over the 493×493 type domain + 4,000 out-of-domain rows — **0 mismatches**. Table *contents* are runtime-loaded and were not tested |
| reachability | WRITES_GLOBAL (reads a static array; callable with fabricated inputs) |
| caveat | the storage extent is unresolved — see §5 |

### `Damage()` — the pipeline

Not implemented, deliberately. §3 is a **structural** derivation (Tier C at best once
ported) with 31 guarded steps, ~15 virtual predicates and 6 unidentified `RULES` offsets
still open. Porting it before the balance-table extent, the `to_hit`/`attenuate` consumers
and the `1/256` rounding are settled would bake in exactly the kind of plausible-looking
error this project exists to prevent.
