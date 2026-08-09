# balance-path — assembly report

Lane: **balance-path**. Wave: assembly. Written 2026-08-08.

---

## What now runs that did not before

1. **A differential case that reads the *real* balance table through retail machine code.**
   `balance_final_table` is registered in `crates/oracle/src/registry.rs` and passes on hbox:
   **243,049 trials, 0 mismatches**, exhaustive over `TypeIndex` 50..=542. It injects
   `schema/live/balance-real.bin` into the mapped image at `Balance::final_balance_table`
   `0x00C12BF4`, calls retail `Balance::return_modifier` `0x00581CA0`, and compares against
   the **shipped** `don_sim::balance::BalanceTable::get`. The pre-existing
   `balance_accessor` case ran against the zero-filled *file* image, where any indexing
   scheme agrees with any other; this one cannot.

2. **`crates/don-sim/src/balance_path.rs`** — the load-time path: the 399-row absolute
   index space, `Balance::return_pack`'s five-dword pack and its push order,
   `compute_modifier`'s per-step truncating fold, the `balance.xml` modifier-matrix loader,
   and `fill_tables`' 493×493 double loop with its 16-bit store. 15 tests, of which 4 are
   gated on shipped/captured game content and run here.

3. **The damage pipeline in `World::step` reads the right cell.** `BalanceTable::get` now
   applies the 50-row bias. Before this, over the whole type domain it returned a **wrong
   percentage for 61.9 % of matchups** and refused outright (`None` → `damage_skipped_no_tables`)
   for another **10.2 %**.

---

## The finding: our balance term was off by 100 rows

The lane brief asked whether the direct-table path and the `type_damage` path agree. They
are not two paths (see §2), but looking for the seam surfaced a live defect in the one path
that exists.

`Balance::return_modifier` `0x00581CA0` is seven instructions [measured, capstone]:

```
imul eax, dword ptr [ebp + 8], 0x1ed        ; atk * 493
add  eax, dword ptr [ebp + 0xc]             ; + def
movsx eax, word ptr [eax*2 + 0xc06afc]      ; <- 0x00C06AFC, the FOLDED base
ret 8
```

`0x00C12BF4 − 0x00C06AFC = 49,400 = 2 × (50·493 + 50)`. So retail's `atk*493 + def` is
correct **relative to `0x00C06AFC`**, and the same arithmetic against an array captured at
`0x00C12BF4` is off by 24,700 elements — exactly 100 rows.

`crates/don-sim/src/mechanics.rs::balance_index` is `atk*493 + def`. It is *right*: it is
retail's arithmetic. What was wrong was pairing it with `schema/live/balance-real.bin`,
which is captured at `0x00C12BF4`. Three sites did that:

| site | status |
|---|---|
| `crates/don-sim/src/balance.rs::BalanceTable::get` | **fixed this lane** — now `crate::balance_path::table_index` |
| `crates/don-sim/src/systems/combat.rs::balance_percent` | **still wrong**, no runtime caller (tests only) — one-line fix below |
| `crates/don-env/src/state.rs::balance_pct` | **still wrong**, and it *is* the env's live read — one-line fix below |

Not edited, because they belong to other lanes this wave. Both need:

```rust
// combat.rs balance_percent, and don-env state.rs balance_pct
let idx = don_sim::balance_path::table_index(attacker_type_id, defender_type_id)?;
```

The size of it, on real data (`schema/live/balance-real.bin`, `schema/live/live-tables-typeids.tsv`):

| matchup | was | is |
|---|---:|---:|
| Tercios vs Jet Fighter | 30 | **130** |
| Marine Riflemen vs Fort | 33 | **66** |
| Fighter vs Small City | 33 | **15** |
| Steppe Nomad vs Tower | 66 | **33** |
| Riflemen vs Steppe Nomad | 100 | **150** |
| Armed Caravan vs Small City | 66 | **100** |
| Citizen vs Citizen | 100 | 100 |

That last row is why nothing caught it. `world.rs`'s only integration test for the damage
pipeline spawns types 50 and 51, and `(50, 51)` is one of the **27.9 %** of pairs where the
two indices coincidentally return the same value. The test asserts only
`hits[defender] < before`, so it was green before the fix and is green after it.

The 7.99 M-trial `damage_pipeline` case could not have caught this either: it drives
`ObjectData::get_damage` with `balance_pct` supplied directly as an input, so it never
exercises the lookup. That is the shape the brief predicted, and it held.

### Mutation evidence — the new case bites

Reverting `BalanceTable::get` to the folded index and re-running:

```
FAIL  balance_final_table  0x00581ca0  218499 trials  150700 mismatches
      excl 24871  TypeIndex outside 50..=542
      atk_type=50 def_type=62 model=120 retail=100
```

**150,700 mismatches (69.0 % of trials)** plus 24,871 refusals. Restored immediately; the
mutation is not in the tree.

---

## §2 — Correction: `type_damage` and `compute_modifier` are load-time only

`docs/mechanics/COVERAGE.md` §6 item 7 reads *"the engine reads it through `type_damage` +
`compute_modifier` + `return_pack`"*. Right about the derivation, wrong about the tense.
Direct-call callers over `.text` [measured, `tools/pdb/callers.py`]:

```
Balance::type_damage      0x0057FB50   1 caller : Balance::compute_modifier
Balance::compute_modifier 0x00581CC0   1 caller : Balance::fill_tables
Balance::return_pack      0x005821C0   (compute_modifier only)
Balance::fill_tables      0x005823F0   1 caller : Balance::init
Balance::return_modifier  0x00581CA0   0 callers — inlined at 0x00644178..0x0064418E
```

The chain runs **once, at rules-load**, and its 243,049 results *are* `final_balance_table`.
Combat then reads that array and nothing else. So the two paths cannot disagree at runtime,
because only one of them is a runtime path. The real question the ranking was pointing at is
the one above: whether we index the producer's output the way the consumer does. We did not.

The corollary matters for the priority list: porting `type_damage` buys **derivability of
the table** (mod support, the `rules` checksum channel from shipped data, a table for a
modded ruleset), not fidelity of combat. Combat's fidelity depends on the capture and the
index, both of which are now pinned against retail.

---

## §3 — `balance_index = type_index − 50`: **promoted to [measured]**

The brief flagged this as a cross-check never promoted. Four independent measurements, any
one of which would have sufficed:

1. **`Balance::fill_tables`' own loop** `0x00582368..0x00582392`:
   `for a in 50..0x21F { for b in 50..0x21F { *p++ = compute_modifier(a, b, mods) } }`,
   writing from `0x00C12BF4` while `p < 0xC896C6`. `0xC896C6 − 0xC12BF4 = 0x76AD2 =
   493·493·2`. The loop *is* the definition of the row order.
2. **The bias constant** `0x00C12BF4 − 0x00C06AFC = 49,400 = 2·(50·493 + 50)`.
3. **The live type table** `schema/live/live-tables-typeids.tsv`: ids 0..=49 are `GoodType`;
   **50 = `UnitType` Citizen**; 401 = Boadicea, the last unit; 402..=413 are the animals;
   414 = `BuildType` Small City; **542 = Space Program**; 543 = the first `ItemType`.
   `542 − 50 + 1 = 493`.
4. **`return_pack` stores `pack.unit = type_index − 0x32`**, and `lookup_absolute_name`
   indexes the type array at `Constants+200 + i*4`.

Now a differential too: `balance_final_table` passes with `table_index` and fails 69 % of
trials with the unbiased index.

**The domain is `50..=542` inclusive.** Nothing outside it has a balance row. Ledger §5.4's
open question — "the live damage hook observed defender type ids 521, 522 and 526, so the
493×493 grid is not the whole id domain" — is **closed**: 521 is `BuildType` Lookout, 522
and 526 are also `BuildType`s, all inside `50..=542`. They looked out of domain only because
they were being compared against a 0..493 range, which is the folded space, not the type
space.

---

## §4 — `compute_modifier`, ported

```
compute_modifier(a, b, mods):
    if !valid_combat(a) || !valid_combat(b): return 100        # note: 100, not 0
    rows = return_pack(a).indices()
    cols = return_pack(b).indices()
    acc = 100
    for r in rows: for c in cols: acc = (mods[r*399 + c] * acc) / 100
    return (type_damage(a, b) * acc) / 100
```

Four things here are easy to get wrong and are pinned by tests in `balance_path.rs`:

* **The fold divides at every step.** `imul` then `cdq; idiv 100`, so it truncates toward
  zero per step and does not commute. Modifiers 33, 33, 150 fold to **15**; the obvious
  "multiply all, divide once" gives 16.
* **The reject value is 100, not 0.** An animal is not excluded from combat; it gets a flat
  100 %.
* **The store is 16-bit.** `compute_modifier` returns `int`; `fill_tables` stores `short`.
  Retail truncates and so do we.
* **Row is the attacker, column the defender**, and the two lists are built in a fixed push
  order (unit, class, kind, age, then flag bits low to high).

### The 399-row absolute index space [measured]

`Balance::lookup_absolute_name` `0x00582910` decodes it:

| absolute | count | meaning |
|---|---:|---|
| `0..352` | 352 | `type_index − 50`, units 50..=401 |
| `352..357` | 5 | SIEGE, FORTS, TOWERS, CITIES, OBSPOST |
| `357..359` | 2 | BUILDINGS, UNITS |
| `359..367` | 8 | AGE_0 .. AGE_7 |
| `367..399` | 32 | `Flag_<c>_OBJMASK_<name>`, `c = chr(0x41+bit)` with a `>'Z' → −0x2A` fixup |

Cross-checks that could have failed and did not:

* `401 − 50 + 1 = 352`, exactly the `0x160` boundary where the unit rows stop. The 12 types
  the guard rejects (402..=413) are exactly the animals.
* The 47-name tail of the shipped `ron-data/balance.xml` matches that layout **in order**,
  and the flag letters run `A..Z` then `1..6` — which is what the `chr` fixup emits.
* The four `BuildType` ids `return_pack` probes are `0x1BB`=443 **Fort**, `0x19E`=414 **Small
  City**, `0x209`=521 **Lookout**, `0x1B7`=439 **Tower** — FORTS / CITIES / OBSPOST / TOWERS,
  one for one.
* 240 of the 291 `balance.xml` row names resolve to a unit row via
  `display.replace('\'', "").replace(' ', "_")` — which is `String::replace(0x20, 0x5F)` at
  `0x00A1D0A0` plus the apostrophe strip at `0x00A16F00(0x27)`, and is why the file spells it
  `Caesars_Legions`. The resolved rows are **strictly increasing in absolute index**.

### A prediction that could have failed

If `compute_modifier` returns 100 for the animal window, then every cell of the captured
`final_balance_table` in rows *or* columns 402..=413 must be exactly 100. Against a base
rate of 49.7 % for the value 100 across the whole table: **11,832 of 11,832 cells are 100.**
Gated as `balance_path::tests::the_captured_table_is_neutral_across_the_animal_window`.

### The fold is load-bearing

`ron-data/balance.xml` carries **525 non-100 cells** across 42 distinct modifier values
(95, 90, 66, 400, 150, 700 …); 516 of them resolve into the matrix (9 sit in rows or columns
whose names have no type in the live capture: Marines, Pathfinder, Pioneer, Ranger).
So `final_balance_table ≠ type_damage` — anyone reconstructing the table from `type_damage`
alone would be wrong for those pairs.

---

## §5 — What is **not** done, stated plainly

* **`Balance::type_damage` `0x0057FB50` is not ported and not tested.** 8,524 B, over the
  bulk decompiler's limit (`MANIFEST.jsonl`: `skipped_large`), and it reads two `Type`
  objects through virtual dispatch off `[0x00E85DDC]`. Calling it in the oracle needs a
  fabricated **type array** — 493 objects with vtables — not a fabricated pair of objects,
  so it is a `damage_env.rs`-scale build, larger than this lane. `balance_path.rs` takes it
  as an **input** (`trait TypeDamage`), the way the damage harness takes its 26 predicates.
  **No `type_damage` number in this repo is measured.**
* **`Balance::compute_modifier` is ported but not differentially tested**, for the same
  reason: it calls `return_pack`, which does the same virtual dispatch, and then
  `type_damage`. The port is Tier C.
* **`return_pack`'s field semantics are structural, not measured.** `UnitType[+0x2B4] &
  0x20000` and the age getter at vtable `+0xEC` are named by offset, not by meaning.
* **`balance_final_table` pins the accessor, not the contents.** It injects a live capture,
  so agreement says our loader and index reproduce retail's read of bytes we already had.
  It does not say those bytes are what `fill_tables` would produce.
* The 15 category names at absolute 352..366 come from three `.data` `String` arrays
  (`0x00E37B10`, `0x00E37B74`, `0x00E37BA0`, stride `0x14`) that are BSS-initialised at
  runtime and unreadable from the file image. They are taken from the shipped
  `balance.xml`, whose tail matches the decoded layout exactly — strong, but it is
  corroboration, not a read of the array.

---

## Files

Written by this lane:

* `crates/don-sim/src/balance_path.rs` — new, the load-time path.
* `crates/don-sim/src/balance.rs` — `get` bias-corrected, `get_folded` added, docs and
  tests updated.
* `docs/assembly/balance-path.md` — this file.

Shared files, smallest possible edit, called out per the wave rule:

* `crates/don-sim/src/lib.rs` — one line, `pub mod balance_path;`.
* `crates/oracle/src/registry.rs` — one `Plan::BalanceTable` variant and one `Case`.
* `crates/oracle/src/run.rs` — one executor arm for that variant.
* `tools/oracle-regress.sh` — one `if` block mirroring the existing `rules.xml` mirror,
  copying `schema/live/balance-real.bin` to `hbox:~/don-oracle/data/`. Without it the case
  SKIPs; it never falls back to the zero image.

Not edited, defect reported instead: `crates/don-sim/src/systems/combat.rs::balance_percent`,
`crates/don-env/src/state.rs::balance_pct`.

## Reproducing

```sh
tools/oracle-regress.sh --only balance_final_table   # 243,049 trials, 0 mismatches
tools/oracle-regress.sh                              # 13 cases, 16,479,445 trials, exit 0
cargo test -p don-sim --lib balance                  # 22 tests
```

`schema/oracle-regression.json` is a **shared** artifact — whichever lane ran the suite last
owns the file on disk, and a `--only` run leaves twelve cases SKIPPED in it. This lane left
a complete 13-case record there (`exit 0`); if you find it showing skips, a sibling ran
`--only` afterwards, and a plain `tools/oracle-regress.sh` restores it.

Tree state at hand-off: `cargo test -p don-sim --lib` is **940 passed, 3 failed**. All three
failures are in `systems/order_dispatch.rs`, a sibling lane's in-flight file which contains
no reference to the balance table; the failing set changed between two runs ten minutes
apart (`systems/target.rs` and `tick.rs` earlier), which is what a live wave looks like.
All 22 balance tests pass, including
`world::tests::attacks_use_the_derived_pipeline_or_refuse`, and the balance change is
independently pinned by the oracle rather than only by local tests.
