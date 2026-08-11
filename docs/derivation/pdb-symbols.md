# `rise.pdb` → machine-readable ground truth

**What this file is.** The derivation record for `schema/symbols.json` and `schema/types.json`,
the two artifacts that turn the shipped PDB into something every other lane can query instead
of guessing. It also records the Ghidra application recipe, the spot-checks that validate the
project's pre-PDB method, and the places where the PDB **disagrees** with what we had written
down.

**Date:** 2026-08-08. **Tier:** these are *facts recorded by the compiler*, not behaviour.
They give names, offsets, sizes and line spans. **They raise no fidelity tier.** A symbol is
a name, not a semantics; values still come from the oracle. Never write "confirmed by the
PDB" about a behavioural claim.

---

## 1. Provenance

| | |
|---|---|
| PDB | `ron-bin/sbl/rise.pdb`, 57,290,752 bytes, MSF 7.00 |
| GUID / age | `{51D4F219-61C6-4F84-9D5B-C3361B0D291F}` / 1 |
| Matches | the `riseofnations.exe` CodeView record, byte-for-byte |
| Image base applied | `0x00400000` |
| Streams | 751; TPI 18.6 MB, DBI 1.3 MB, IPI 1.1 MB, Symbol Records 4.9 MB |
| Modules | 778 `.obj` translation units |
| Original build root | `E:\agent\_work\2\s\main\` (an Azure DevOps agent) |

Ghidra independently verified the identity: its PDB Universal analyzer checks the PDB GUID/age
against the program's CodeView record and refuses a mismatch. It accepted this pair.
**[measured]**

`README-LLM.md` and `docs/binary-ground-truth.md` had both already been corrected (by a
parallel lane) to say the PDB *is* shipped, so no doc edit was needed from this lane.

## 2. What was built

**Extractor:** `tools/pdb-extract/` — Rust, using the `pdb` crate 0.8 plus `msvc-demangler`.
It declares its own `[workspace]`, so it is deliberately **not** a member of the repo-root
workspace and `cargo test` at the root never builds it.

```sh
cd /Users/ember/dev/don/tools/pdb-extract && cargo build --release
./target/release/pdb-extract \
    /Users/ember/dev/don/ron-bin/sbl/rise.pdb 0x00400000 \
    /Users/ember/dev/don/schema/symbols.json \
    /Users/ember/dev/don/schema/types.json \
    /Users/ember/dev/don/schema/vtables.json     # optional 5th output
```

Use the **absolute** PDB path: it is recorded verbatim in `_meta.pdb`, and it is the only
thing in either output that varies between runs.

Runs in about 8 s cold, under 1 s warm, and is deterministic — re-running reproduces both files byte-for-byte. `llvm-pdbutil` (`/opt/homebrew/opt/llvm/bin/`) was used for
cross-checking stream and section headers; Ghidra's PDB Universal reader — pure Java, fine on
arm64 — did the application. The in-tree `README_PDB.html` describing a Windows-only
`pdb.exe`/XML path is years stale and was ignored.

### `schema/symbols.json` (17.6 MB)

```
_meta          counts, GUID, image base, and the caveats below
source_files   999 distinct source paths
modules        778 .obj names
functions[]    va, rva, name, mangled, demangled, signature, size, kind,
               module, file, line, line_end, type_index
globals[]      va, rva, name, mangled, demangled, type, type_index, size,
               kind, module
```

### `schema/types.json` (13.4 MB)

```
classes{}      kind, size, unique_name, bases[], fields[], statics[],
               methods[], methods_declared, nested[], has_vftable
enums{}        underlying, size, values[]
```

Each `fields[]` entry carries `name`, `type` (rendered C++), `type_index`, `offset`, `size`
(resolved through modifier/array/enum/typedef) and, for bitfields, `bit_offset`/`bit_length`.
Forward references are resolved to their definitions.

**One deliberate omission, stated so it is not a silent gap:** `methods[]` lists *virtual*
methods only, each with its introducing vtable slot — information that exists nowhere else.
Non-virtual methods are dropped and `methods_declared` records the true declared count.
Every *emitted* method already appears in `symbols.json` with an address and a full signature;
carrying them here too tripled the file (33 MB → 13 MB) for no new facts. What is genuinely
lost is the declaration of methods that were fully inlined and never emitted.

**A second omission, previously silent, now declared in the file:** `classes`/`enums` are
keyed by the **bare tag name** and are first-record-wins, so the **208** definitions the PDB
holds under an already-used name are not emitted — 194 colliding names, all Win32/COM/CRT/zlib
headers duplicated across translation units, **no engine class among them**, 13 with genuinely
divergent shapes. `_meta.counts` now carries the arithmetic and `_meta.collisions` lists every
affected name with its definition count and distinct shapes. Field-type *resolution* is
unaffected — it goes through the collision-free COMDAT unique name. Full derivation and the
reasoning for declaring rather than restructuring:
`docs/derivation/pdb-extract-name-collisions.md`.

## 3. Headline numbers **[measured]**

| | |
|---|---|
| Function records | **22,750** |
| Distinct function addresses | **20,413** |
| Addresses carrying more than one name | **586** (identical-COMDAT folding) |
| …with a source file **and** line | **22,500** (98.9 %) |
| …with a C++ signature | 22,196 |
| …with an unambiguous mangled name | 17,920 |
| Global / static data symbols | **20,811** (1,817 `S_GDATA32`, 3,219 `S_LDATA32`, 16,313 public-only) |
| Public symbols in the PDB | 37,144 |
| Type records in the TPI | 305,734 |
| Class/struct/union definitions | **19,914** (5,928 with at least one field) |
| Field records | 26,469 |
| Base-class records | 12,481 |
| Virtual method records (with vtable slots) | 21,997 across 5,338 classes |
| Enums / enumerators | 2,857 / 16,647 |

### Source tree layout

The 999 files sit under four game roots plus toolchain headers:

| functions | code bytes | root |
|---|---|---|
| 11,265 | 5,074,317 | `e:\agent\_work\2\s\main\game\` |
| 3,918 | 611,289 | `e:\agent\_work\2\s\main\basic\` |
| 1,588 | 318,502 | `e:\agent\_work\2\s\main\bighuge\` |
| 472 | 120,418 | `e:\agent\_work\2\s\main\game\script\` |
| 2,231 | 268,499 | MSVC 14.0 `vc\include` (STL) |
| — | — | plus `pnglib`, `zlib`, `steamworks_sdk`, `cpprestsdk`, `rapidjson`, `cellsdk`, `cpclib`, `crossplaynetlib` |

`game/` is the simulation, `basic/` is the string/array/random/math substrate, `bighuge/` is
the window and UI toolkit, `game/script/` is the scenario scripting layer. So the split we
care about is roughly **5.0 MB of simulation code against 0.6 MB of substrate**.

### Richest subsystems, by emitted code bytes

| bytes | fns | class | `sizeof` | fields |
|---|---|---|---|---|
| 193,542 | 184 | `Unit` | 344 | — |
| 189,140 | 872 | `ScenarioFuncSet` | 44 | — |
| 169,497 | 165 | `GraphicPieces` | 4,172 | 119 |
| 143,373 | 128 | `Leader` | 28,396 | — |
| 141,802 | 142 | `TerrainOut` | 27,328 | — |
| 110,362 | 113 | `SetupWin` | 13,484 | 60 |
| 90,428 | 6 | `Constants` | 3,432 | **722** |
| 85,405 | 69 | `Options` | 232 | 23 |
| 80,929 | 76 | `Group` | 2,516 | — |
| 75,160 | 70 | `ConquestGame` | 5,308 | 146 |
| 72,561 | 109 | `Game` | 3,184 | 75 |
| 56,301 | 173 | `LeaderData` | 28,388 | **299** |

Largest single source files: `unit.cpp` (236 KB), `leaders.cpp` (218 KB), `graphicpieces.cpp`
(171 KB), `scriptfunctions.cpp` (134 KB, 843 functions), `groups.cpp` (107 KB), `map.cpp`
(95 KB), `constants.cpp` (90 KB in five functions).

## 4. Spot-checks against independently derived addresses

The evidence standard for this task. Every address this project had derived by other means
resolves, and none contradicts the *address*; four contradict the **name or role** we had
attached to it. **[measured]**

| VA | our prior label | PDB says | verdict |
|---|---|---|---|
| `0x00644130` | damage, `__thiscall`, pure integer | `ObjectData::get_damage(int,int,unsigned long,int,int,int*) const` — `object.cpp:614‑1088`, 3,954 B | ✅ confirms |
| `0x00a39cf0` | `next_float` | `float Random::get()` — `basic/random.cpp:58`, 54 B | ✅ confirms |
| `0x00a39d70` | `in_range` | `int Random::get(int,int)` — `random.cpp:28`, 356 B | ✅ confirms |
| `0x0094a700` | `CommandPackage::process` | `int CommandPackage::process(Command*)` — `commandpackage.cpp:676`, 4,156 B | ✅ exact, name and all |
| `0x00936560` | `check_all` | `unsigned long CheckSums::check_all()` — `checksums.cpp:16`, 1,614 B | ✅ exact |
| `0x009459d0` | `process_check_sums` | `int CommandPackage::process_check_sums(CheckSumsCommand*)` | ✅ exact |
| `0x00a46830` | adler-32 | `unsigned long adler32(unsigned long, const unsigned char*, unsigned long)` — `basic/misc.cpp:464` | ✅ exact |
| `0x00846450` | dead code, *not* the RNG | `int Doober::get_num(TCoord,TCoord,int,int)` — `doober.cpp:826` | ✅ confirms it is not the RNG |
| `0x00a1d110` | `RString::AsScaled` | `int String::fraction(int) const` — `basic/str.cpp:2569`, 111 B | ⚠ **right function, wrong name** |
| `0x00570170` | "the rules.xml constant loader" | `void Constants::log_data(Log*) const` — `constants.cpp:47‑907`, 63,382 B | ❌ **it is a logger** |
| `0x0065fc00` | "combat stats loader" | `void ObjectType::log_data(Log*)` — `objecttype.cpp:394` | ❌ **logger** |
| `0x0061c490` | loader that xrefs `progression` | `void UnitType::log_data(Log*)` — `unittype.cpp:459` | ❌ **logger** |

Two structural cross-checks, both clean:

- **`schema/vtables.json`**: of the 1,659 addresses in the then-current map that also carry a
  PDB `??_7…@@6B@` public symbol, **1,659 agree and 0 disagree**.
  > **CORRECTION (2026-08-11, lane `vtables`).** The sentence that used to follow —
  > "the remaining 118 have no public vftable symbol to compare against" — was wrong, and
  > it read as reassurance when it was the opposite. Those 118 addresses *do* carry PDB
  > symbols, and the symbols say they are **not vtables**: 47 `RTTI Class Hierarchy
  > Descriptor`, 28 `RTTI Base Class Array`, 23 `RTTI Complete Object Locator`, 19 `RTTI
  > Base Class Descriptor`, 1 `__CTA1?AV_com_error@@`. Their first dword is outside `.text`
  > [measured, `ron-bin/riseofnations.exe`]. The map has since been regenerated from the
  > `??_7` symbol set alone — 1,888 rows — and `docs/derivation/vtable-map.md` has the
  > derivation and the independent RTTI cross-check that bounds its completeness. The
  > 1,659/1,659 agreement above is unaffected and is now the *whole* overlap by
  > construction.
- **`docs/derivation/rules-constants.json`** (719 rule constants with struct offsets, recovered
  from binding call sites): **719 / 719 offsets agree exactly** with the PDB's `Constants`
  class layout. The offset-recovery method is vindicated end to end.

A third, unplanned cross-check: a parallel lane wrote an independent extractor and produced
`schema/rise-procs.tsv`. Comparing (VA, name) pairs — **22,749 vs 22,749, intersection 22,749,
zero rows on either side alone.** Two independently written parsers, exact agreement.

## 5. Findings — where the PDB contradicts what we had

### 5.1 The 112 "rule loaders" are `log_data` dumpers, not loaders

Of the 112 loader addresses in `schema/bindings.json`, **111 resolve to `SomeClass::log_data`**
and one to `Constants::init`. `Log` is unambiguously a *writer*: it holds a `FILE*` (`_iobuf*
handle`), and its vtable is `init / reset / flush / begin / end / say / say_hex / set_type /
set_detail / check_accept`. Disassembling `Constants::log_data` and counting call targets:
**509 × `Log::say(const String&, int)`**, **508 × `String::String(const wchar_t*)`** (building
the rule-name literals it prints), 719 × `String::close` (destroying those temporaries) — and
**zero** calls to `Constants::get_fraction`. It writes; it does not read. **[measured]**

The real parse path is `Constants::init` at **`0x00569a90`** (`constants.cpp:911‑1964`,
26,336 B), which calls `Constants::get_item(const String&)` **661×** and
`Constants::get_fraction(const String&, int)` **40×**; `get_fraction` is the one that calls
`String::fraction` at `0x00a1d110`. 38 of the 112 classes have a sibling `::init`
(`UnitType::init` `0x0061ab50`, `GoodType::init` `0x0066ea20`, `BuildType::init`
`0x00632340`, `SpellType::init` `0x00674a80`, `TurnControl::init` `0x00957e20`, …).

**What this does and does not invalidate.** The *offsets* are unaffected — `log_data` reads
the same field under the same name, and the 719/719 agreement above proves it. What is now
[reported] rather than [measured] is the **`parser` and `scale` attribution** in
`rules-constants.json`: those were read off the *output-formatting* path, and nothing here
establishes that the display scale equals the parse scale. That has to be re-derived from
`Constants::init` before any of it is treated as parser semantics.

A parallel lane reached the same conclusion from the string side and traced it further —
the real loaders pull names from the runtime `StringTable` at `[0x00C06378]` and reference no
literal, which is why following a rule-name string in `.rdata` always lands in the logger. See
`docs/derivation/PDB-RECONCILIATION.md` §2.

### 5.2 The balance table address is wrong; the real one is `0x00C12BF4`

`README-LLM.md` carried "Balance table: base `0x00C06AFC`, 493×493 int16" with a warning about
"unexplained negatives". The PDB explains them: **`0x00C06AFC` falls inside
`s_SteamWorkshopTagLinks`**, a 4,608-byte static array of Steam Workshop UI tag descriptors at
`0x00C068D0`. It is not balance data at all.

The real table is the global **`combat_table`** at **`0x00C12BF0`**, of class `Balance`,
size **486,104** bytes, whose sole field is:

```
Balance  (sizeof 486104, bases GameAccess, MiscAccess)
    +0x04   short[493][493]   final_balance_table     // 486,098 bytes
```

493 × 493 × 2 = 486,098, and 4 + 486,098 = 486,102, padded to 486,104. So the table base is
**`0x00C12BF4`**. Its accessors are `Balance::type_damage(TypeIndex, TypeIndex)`
`0x0057fb50`, `Balance::compute_modifier(TypeIndex, TypeIndex, short*)` `0x00581cc0`, and it
is filled by `Balance::fill_tables(XMLElement)` `0x005823f0`. (A parallel lane reached the
same address independently; `README-LLM.md` already carries the correction.)

### 5.3 `0x00a1d110` is `String::fraction`, not `RString::AsScaled`

Right function, wrong name — the name came from outside this project. There is no `RString`
class in the binary at all; the class is `String` (`sizeof` 20, `basic/str.cpp`) with 277
declared methods. Rename it in the ledger; the semantics claim is untouched.

### 5.4 Ghidra's function list is mostly not functions

Against `re/decomp-all/MANIFEST.jsonl` (46,727 Ghidra functions):

- 18,350 addresses match a PDB procedure exactly.
- **28,377 are Ghidra-only, and 28,327 of those are under 40 bytes** — C++ EH unwind funclets
  and `catch` blocks (`mov ecx,[ebp-0x18]; call …; push 0; push 0; call …`) and 6-byte EH
  state markers. They are not functions. Only 50 Ghidra-only entries exceed 40 bytes.
- **2,063 PDB procedure addresses have no Ghidra function at all** — this is the concrete list
  behind the README's "Ghidra left gaps", and `schema/symbols.json` now enumerates it. Among
  them: `Leader::close` (602 B), `Leader::process` (353 B), `basic_idle_modal` (266 B), and
  four `ScenarioFuncSet` scripting entry points.
- Of the 18,350 shared addresses, **827 disagree on size**, and in **820** of those the PDB's
  extent is *larger* — Ghidra truncated the tail (`ListBox::on_key_click`: 852 B vs 299 B).

**`MANIFEST.jsonl`'s `size` is not a code size.** It is Ghidra's count of addresses in the
function *body* it reconstructed, so it undercounts whenever control flow leaves the body and
comes back, and the gap is not a bounded rounding error: `ConsoleWin::run_cmd` `0x007d6a70` is
`"size": 664` in the manifest and **43,008** bytes in the PDB — a 65× difference, and it
decompiles to 5,427 lines. Twelve of the 820 differ by ≥1 KiB (`ConquestFinalWin::on_redraw`
1,758 vs 5,900; `Search::valid_filter` **19** vs 2,629; `Scene::draw_overlays` 9,350 vs 12,564)
[measured, `MANIFEST.jsonl` × `schema/symbols.json`, 2026-08-11]. Never size a body, a budget or
a coverage residual off a manifest `size`; take it from `schema/symbols.json`, which carries the
PDB's `S_GPROC32` extent.

`schema/islands.jsonl` is separately incomplete in a way worth knowing: its largest recorded
function is 4,048 bytes, so every larger function — including `Constants::log_data` and
`Constants::init` — is simply absent from it.

### 5.5 Four rule constants were missed

`Constants` has 722 fields; `rules-constants.json` has 719. The four game-rule fields absent
from our recovery are `liberty_free_upgrades` (+0x534), `eiffel_siege_range` (+0x548),
`aztec_move_speed` (+0x574), `spanish_extra_scout` (+0x680). (The fifth extra field,
`curr_element` at +0xd40, is an `XMLElement` cursor, not a rule.)

### 5.6 Newly available high-leverage structure

- **`TypeIndex`** — a 869-value enum: the canonical type ID space, resources first
  (`FOOD`=0 … `SILK`=11), then the counts `NUM_UNITTYPES`=352, `NUM_BUILDTYPES`=129,
  `NUM_TECHTYPES`=85, `NUM_SPELLTYPES`=55, `NUM_GOODTYPES`=50, `NUM_RARES`=44,
  `NUM_EPOCHTYPES`=28, `NUM_WONDERTYPES`=17, `NUM_GAIATYPES`=12, `NUM_AGETYPES`=7.
- **82 `Command` subclasses** and an 83-value `CommandTypes` enum — the multiplayer wire
  action space, with exact struct layouts (`Command` is 1 byte: `unsigned char command_type`).
- **32 `*Order` classes**, rooted at `UnitOrder` (30 derive from it), with `TargetOrder`,
  `MoveOrder`, `AirOrder`, `GroupOrder`, `PatrolOrder` as intermediate bases — the engine's
  own order hierarchy, which is the RL action space.
- **`OptionIndex`** (333 values) — the AI/player option space. **`KeyMapType`** (463).
- Named globals for things we had only as addresses: `[0x00C061E4]` =
  `GameAccessConst::constantsc` and `[0x00C061F0]` = `GameAccess::constants` — two static
  references to one `Constants`, which is exactly the aliasing the live read found;
  `[0x00C06184]` = `GameAccess::game_random` (a `Random&`); `0x00EB697C` = `internal_random`,
  an actual `Random` object (`sizeof` 4 — one `unsigned long random_seed`).

## 6. Applying the PDB to Ghidra

Done on a **copy** (`/tmp/gh-pdb`); the canonical project at `re/ghidra` was not touched.

Two scripts were added under `re/scripts/`:

- **`ApplyPdb.java`** — run as a **preScript**. Sets the PDB file option, allows an untrusted
  path, disables every other analyzer, and enables all PDB sub-options it finds.
- **`PdbReport.java`** — run as a **postScript**. Reports counts and re-checks the spot addresses.

**Gotcha, paid for once:** calling `PdbUniversalAnalyzer.doAnalysis()` directly from a
postScript with `-noanalysis` dies with `IllegalArgumentException: No active analysis session`
— the applicator reads `TransientProgramProperties` scoped to an analysis session. It must run
*as an analyzer*, which means letting headless analysis run (no `-noanalysis`) with everything
else switched off.

### The exact sequence, to redo on the canonical project when no other lane holds the lock

```sh
# 0. Nothing else may be using re/ghidra — it is single-writer.
/opt/homebrew/Cellar/ghidra/12.1.2/libexec/support/analyzeHeadless \
  /Users/ember/dev/don/re/ghidra ron \
  -process riseofnations.exe \
  -scriptPath /Users/ember/dev/don/re/scripts \
  -preScript  ApplyPdb.java /Users/ember/dev/don/ron-bin/sbl/rise.pdb \
  -postScript PdbReport.java \
  2>&1 | tee /tmp/ghidra-pdb.log
```

Note: **no `-noanalysis`.** `ApplyPdb.java` turns off the other 29 analyzers itself, so this
does not re-analyze the program; it only layers the PDB on. Takes about 7 minutes.

### Result on the copy **[measured]**

| | before | after |
|---|---|---|
| functions | 47,177 | **48,791** (+1,614 created from PDB) |
| still named `FUN_…` | — | **37** |
| non-default symbols | — | 156,486 |
| data types | — | 132,197 |

`resolveCount: 216388`, `conflictCount: 188`. 234 composites logged
`PDB STRUCTURE reconstruction failed to align` — mostly the packed `*Command` structs and
WinRT/STL template instantiations; those types land with imperfect field layout and should be
read from `schema/types.json` rather than from Ghidra.

Decompiler before and after, `0x00936560`:

```c
/* before */  int FUN_00936560(void)
              { undefined4 extraout_ECX; undefined4 extraout_ECX_00; ... }

/* after  */  ulong __thiscall CheckSums::check_all(CheckSums *this)
              { CheckSums *this_00; World *this_10; Game *this_11;
                Leaders *this_13; DataWalk *unaff_EDI; CheckSum local_2c; ... }
```

## 7. Limitations — read before citing this

- **No fidelity tier is earned here.** Names, offsets, sizes and line numbers are what the
  compiler recorded. They do not tell you what a function computes. Every value claim still
  goes through the oracle.
- **586 addresses carry more than one name** because MSVC folded identical COMDATs. Any tool
  that resolves a VA to a single symbol must say which one it picked and why; picking
  arbitrarily is how a 3-byte `walk_test` gets reported as somebody's `OnPaint`.
  `symbols.json` therefore emits *one record per procedure symbol*, not per address.
- **4,830 procedures have no unambiguous mangled name** (2,509 are `S_LPROC32` statics, which
  have no public symbol by construction; 4,556 of the total are templates, lambdas or
  compiler-generated names that MSVC does not mangle in the simple `?m@C@@` form the matcher
  keys on). Their `name`, `size`, `file` and `line` are unaffected — only `mangled` and
  `demangled` are withheld rather than guessed.
- **`types.json` omits non-virtual methods** (see §2). Inlined-only methods are genuinely not
  represented anywhere.
- **Field `size` is computed**, not stored: resolved through modifier/array/enum/pointer with
  a 4-byte pointer width. Offsets and class `size` are read straight from the TPI.
- The PDB describes the **build**, not the running game. Anything about live heap layout,
  ASLR-rebased addresses or runtime values remains a live-read question.

## 8. Files

| path | what |
|---|---|
| `schema/symbols.json` | 22,750 functions + 20,811 globals, with names/sizes/lines/types |
| `schema/types.json` | 19,914 classes, 26,469 fields, 21,997 virtual slots, 2,857 enums |
| `schema/vtables.json` | 1,888 vtable VA → class, one row per `??_7…@@6B…@` symbol (optional 5th output; see `docs/derivation/vtable-map.md`) |
| `tools/pdb-extract/` | the Rust extractor that produces all three (standalone workspace) |
| `re/scripts/ApplyPdb.java` | preScript: point Ghidra at the PDB, disable other analyzers |
| `re/scripts/PdbReport.java` | postScript: report what landed, re-check spot addresses |
