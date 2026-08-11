# `schema/vtables.json` — derivation, and what the previous map got wrong

**Lane:** vtables (megaswarm wave 4). **Date:** 2026-08-11.
**Ground truth:** `ron-bin/riseofnations.exe` (the shipped PE, read directly),
`ron-bin/sbl/rise.pdb` via `schema/symbols.json`. Nothing here comes from a decompiler.

---

## 1. What the file is

A flat `{ "0x<va>": "<class>" }` map from vtable virtual address to class name. Its only
consumer is `crates/donscan`, which `include_str!`s it and reads it with a hand-rolled
parser that accepts exactly this shape (`crates/donscan/src/vtables.rs`). The class names
are the **mangled** class token — `?$ArrayBase@U?$RectTemplate@F@@`, not
`ArrayBase<RectTemplate<short>>` — because that is the form the map has always used and
`donscan`'s `Kind` classification and every published census matches on it.

**It is generated. Do not hand-edit it.**

```sh
cd /Users/ember/dev/don
cargo build --release --offline --manifest-path tools/pdb-extract/Cargo.toml
tools/pdb-extract/target/release/pdb-extract ron-bin/sbl/rise.pdb 0x00400000 \
    schema/symbols.json schema/types.json schema/vtables.json
# vtables=1888
```

The fifth argument is optional and new; passing four arguments reproduces `symbols.json`
and `types.json` byte-identically to before (verified: both bodies compare equal, the only
`_meta` difference is the recorded PDB path).

## 2. The rule

**One row per `??_7<class>@@6B…@` public symbol in `rise.pdb`, and nothing else.**

MSVC emits `??_7C@@6B@` for a class's primary vftable and `??_7C@@6BB@@@` for the vptr of
each secondary base subobject. The class token is everything before the **last** `@@6B` —
last, not first, because a template argument can itself contain `@@`
(`??_7?$ArrayBase@U?$RectTemplate@F@@@@6B@`).

That rule is not a guess about what the map *should* contain; §3 establishes that this
symbol set is exactly the image's vtable set.

## 3. Why that symbol set is the right one — two independent methods, full agreement

**Method A — the PDB.** 1,888 `??_7…` public data symbols, at 1,888 distinct addresses,
all 4-aligned. For every one of them the dword at that address lies inside `.text`
(`0x401000`–`0xac4230`, from the PE section headers). Zero exceptions. A vtable's first
slot is a function pointer, so this is the minimum test any candidate must pass, and the
whole symbol set passes it.

**Method B — the image's own RTTI, ignoring the `??_7` symbols entirely.** MSVC places a
pointer to a class's `RTTI Complete Object Locator` in the dword immediately *before* its
vtable. Taking the 1,887 `??_R4…` symbol addresses from the PDB and scanning `.rdata`,
`.data` and `_RDATA` for any 4-aligned dword equal to one of them yields **1,887 sites**,
and the dword after each site is in `.text` in every case — i.e. 1,887 vtables located
without consulting a single `??_7` symbol.

**All 1,887 of Method B's vtables carry a `??_7` symbol. Zero are unnamed.** The single
`??_7` symbol Method B does not reach is `_com_error` `0xac6e2c`, whose `-4` dword points
into `.data` rather than at a `??_R4` record; it is CRT/`comsupp` code, not engine code.

So: 1,888 = 1,887 RTTI-reachable vtables + `_com_error`. The two methods do not merely
overlap, they close on each other. **There is no vtable in this image that the map now
misses**, which is a stronger statement than the old map's "only the RTTI-named ones" and
is the reason `docs/tooling/native-scanner.md`'s "the map does not cover every vtable in
`.rdata`" caveat has been removed rather than softened.

## 4. What the previous 1,777-row map got wrong

The old map was produced by a scan (its provenance was never recorded in-tree, and there
was no generator — that is itself part of what this lane fixed). It was wrong in both
directions. Both defects were frozen by `assert_eq!(m.entries.len(), 1777)` in
`crates/donscan/src/vtables.rs`.

### 4.1 118 rows did not point at a vtable

Their first dword is outside `.text`. The PDB names every one of them, and the names are
RTTI/EH metadata, not vtables:

| PDB record kind at the address | rows |
|---|---|
| `RTTI Class Hierarchy Descriptor` | 47 |
| `RTTI Base Class Array` | 28 |
| `RTTI Complete Object Locator` | 23 |
| `RTTI Base Class Descriptor` | 19 |
| `__CTA1?AV_com_error@@` (EH catch-type array) | 1 |

**100 of the 118 also carried a class name that does not match the metadata's own owner.**
`0xb6c360` was listed as `?$ObjectArray@VForm@@`; it is `PtrArray<class Good>`'s Class
Hierarchy Descriptor. `0xb6a760` was listed as `?$Array@E`; it is `SimpleArray<unsigned
short>`'s Complete Object Locator. These rows were not "a vtable with a slightly wrong
label" — they were address/name pairs with no relationship to each other, and any hit on
one was an outright misattribution.

They did produce hits. Every one of the 118 addresses occurs at least once as a 4-aligned
dword inside the mapped image — **183 occurrences in total** — because a vtable's `-4`
slot references its COL and a Class Hierarchy Descriptor references its Base Class Array.
So the `image` column of every published census carried 183 false hits, spread over 84
classes that also had a correct row (whose counts were therefore inflated) plus 33 names
that existed in the map *only* at such an address.

### 4.2 The 33 names that existed only at a non-vtable address are not a loss

This is the question that decides whether any of those rows were load-bearing. They were
not, and the evidence is structural rather than a judgement call:

- `MountainRangeData` (232 B), `GameAccessConst` (1 B), `MiscAccess` (1 B) have
  **zero virtual methods** in `schema/types.json`. No virtuals, no vptr — a vptr scan can
  never type them, whatever the map says.
- The other 30 (`ISteamMatchmakingPingResponse`, `_com_error`, and 28
  `ArrayBase<…>` / `ArrayBaseSimpleCopy<…>` instantiations) have a `??_R3` Class Hierarchy
  Descriptor but **no `??_R4` Complete Object Locator and no `??_7` vftable**. A COL is
  emitted for a class whose objects are ever a *most-derived* object with RTTI; a bare CHD
  means the name appears only as a base-class entry inside some derived class's hierarchy.
  `ArrayBase<Coord>` is real, but every live one of them is a subobject of an
  `Array`/`SimpleArray`/`PtrArray` that *does* have a row — `?$SimpleArray@VCoord@@`
  `0xb48c18`, `?$PtrArray@VOilWell@@` `0xb4a66c`, `?$Array@VTCoord@@` `0xb542d4`, and so on.

`_com_error` is the one name that survives the drop by being re-added correctly: its real
vftable is `0xac6e2c`.

### 4.3 229 real vftables had no row

204 are `std::` / lambda / `Concurrency::` (PPL) / `Microsoft::Xbox::Telemetry` template
instantiations. **25 are engine classes**, and four of them are the classes
`schema/state-schema.json` is *defined by*:

| class | vtable | note |
|---|---|---|
| `DataWalk` | `0xb2bcd8` | exactly two slots, **both `_purecall` `0x55e0a6`** — literally the `walker->vt[0]`/`vt[1]` pair the state-schema method is built on. Slot 2 is already the next object's COL pointer. |
| `SaveGame` | `0xb35ac4` | |
| `LoadGame` | `0xb30c88` | |
| `CheckSum` | `0xb3f920` | slot 0 is `CheckSum::walk_function` `0x936ff0` |
| `Type` / `TypeOut` / `TypeData` | `0xb43cbc` / `0xb43da4` / `0xb43e48` | the bases of the `UnitType`/`BuildType` pairs |
| `SoundType` | `0xb43944` | |
| `TerrainGroups` | `0xb45dd8` | |
| `ParticleSystem` | `0xb55740` | |
| `IncrementalLoad` | `0xb66bd0` | |
| `ComboBox` | `0xb2258c` + `{for BufferBase}` `0xb22a80`, `{for ImageIO}` `0xb22f8c` | |
| `WorldMapBackground` | `{for TextureBase}` `0xb54ef8`, `{for ImageIO}` `0xb54f08` | |
| `GameSpyPlayer` / `GameSpyBuddy` / `Lobby::LobbyData` / `SteamLobby::SteamLobbyData` | `0xb3014c` / `0xb5735c` / `0xb6397c` / `0xb63a74` | |
| `PopupRequest` / `BasicPopupRequest` / `BuddyPopupRequest` | `0xb5737c` / `0xb57934` / `0xb57370` | |
| the two Steam callback thunk classes | `0xb62e74`, `0xb6383c` | |

### 4.4 `0xb41ae0` is not a missing vtable

`docs/tooling/native-scanner.md` recorded `Unit + 4` holding `0xb41ae0` as "squarely in the
vtable range but absent from the map", and read that as evidence the map was incomplete.
The dword at `0xb41ae0` is `0xfffffffc`, and the PDB names the address
``const Unit::`vbtable'`` — a virtual **base** table, which holds offsets, not code
pointers. It is correctly absent, and it is now pinned absent by a test.

## 5. What changed, exactly

| | old | new |
|---|---|---|
| rows | 1,777 | **1,888** (118 dropped, 229 added, 1,659 kept) |
| rows whose name changed | — | **0** |
| distinct class names | 1,318 | 1,511 (32 gone, 225 new) |
| address span | `0xac6d54..0xbc21d8` = `0xfb484` | `0xac6d54..0xb67d9c` = `0xa1048` |
| `donscan` lookup table | 257,314 `u16` slots, 514,628 B | **164,883 slots, 329,766 B** |
| names shared by more than one row | 283 | 205 |
| file size | 65,089 B | 91,107 B |

The upper bound moved *down* because the old one, `0xbc21d8`, was a bogus `_com_error` RTTI
row. The prefilter window is derived from the map, so the scan's hot loop gets a strictly
smaller table; there is no performance cost to the added rows.

### Behavioural consequences for `donscan` — stated, not silent

1. **183 false `image`-region hits disappear**, across 84 classes that were being
   over-counted and 33 that vanish entirely. The `image` column of
   `docs/tooling/native-scanner.md`'s census predates this fix and is affected; the
   `private` (heap) column is affected only where a COL pointer had been copied onto the
   heap, which this lane cannot measure statically.
2. **229 classes become typeable.** Objects whose vptr is one of the added addresses were
   previously counted as nothing at all. Statically, the added rows account for 163
   dword occurrences inside the mapped image; their heap population is unknown until the
   next live run.
3. **`crates/donscan/src/live.rs` is unaffected.** Its `kind_of` matches only `Unit`,
   `Build`, `Wall`, `Animal`, `Ammo`, `Caravan`; the first five keep their exact addresses
   (`0xb417d0`, `0xb42174`, `0xb42cf8`, `0xb4145c`, `0xb45418`) and all 18 `donscan --lib`
   tests pass unchanged.
4. **`Caravan` was already dead and still is.** There is no `Caravan` vftable in either
   map, and `schema/types.json` gives `Caravan` (80 B) **zero virtual methods**, so
   `kind_of`'s `Some("Caravan") => Kind::Caravan` arm cannot fire. Pre-existing; not
   touched by this lane. The map does carry `?$Array@PAVCaravan@@`, `?$PtrArray@VCaravan@@`
   and `?$Array@VCaravanLink@@`.
5. **The name of a *secondary* vtable is still the derived class, unqualified.** `UnitType`
   `0xb41fcc` is `??_7UnitType@@6BSoundType@@@` and `0xb41fd4` is `??_7UnitType@@6BType@@@`;
   both are named `UnitType`, as before. 205 names are shared by more than one row for this
   reason — down from 283, because 84 of the dropped bogus rows were duplicating a class
   that already had a correct one. Changing the convention would have re-partitioned every
   published per-class count, so it was kept and the qualifier is left recoverable from
   `schema/symbols.json` by
   address. **This is the trap behind the `BuildType` note on the megaswarm board**: when
   two rows share a name and sit 8 bytes apart, they are two base subobjects of one class
   and picking the wrong one shifts every slot. Resolve it by looking the address up in
   `schema/symbols.json`, never by taking the first row with the right name.

## 6. Tier and limits

Tier: **[measured]** for every count and address above — each is a direct read of the
shipped PE or of `schema/symbols.json` (which `docs/derivation/pdb-symbols.md` establishes
against an independently written extractor, 22,749/22,749).

What this does **not** establish:

- **That a hit is an object start.** Unchanged from before: `donscan` reports any 4-aligned
  dword equal to a rebased vtable address. A stored vptr copy or a `dynamic_cast` cache
  hits too.
- **Live heap effects.** Every delta in §5 that concerns the heap is a prediction from
  static structure. Nobody has re-run a live scan with the new map; the numbers in
  `docs/tooling/native-scanner.md`'s census tables are from the old one and are labelled as
  such there.
- **Names for vtables MSVC folded.** If two classes' vtables were ICF-identical the linker
  keeps one address and, in this image, one `??_7` symbol per address — 1,888 symbols at
  1,888 addresses, so no folding is visible here, but `donscan` would report the surviving
  name if it happened.
