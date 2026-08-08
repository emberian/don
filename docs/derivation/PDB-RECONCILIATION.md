# PDB reconciliation — auditing a day of derivation against the compiler's own answers

**Date:** 2026-08-08. **Subject:** every substantive address- or function-naming claim in
`docs/derivation/*.md`, `docs/derivation/AUDIT*.md` and `docs/provenance-ledger.md`, checked
against `ron-bin/sbl/rise.pdb`.

---

## 0. What happened, and what the PDB is

The game **ships its own private PDB**. `ron-bin/sbl/rise.pdb`, 57,290,752 bytes, sat in the
install's `sbl\` folder all day. It was missed because an early recon command piped a
directory listing through `head -5`. Every document in this repo that says the game ships no
PDB is stale; those have been corrected (§8).

It is the PDB for **this exact binary**, not a near-miss build [measured]:

| | |
|---|---|
| exe CodeView `PdbFileName` | `E:\agent\_work\2\s\main\game\rise.pdb` |
| exe CodeView GUID / age | `{51D4F219-61C6-4F84-9D5B-C3361B0D291F}` / 1 |
| `rise.pdb` GUID / age | `{51D4F219-61C6-4F84-9D5B-C3361B0D291F}` / 1 |
| contents | 37,138 public symbols · 22,752 procedure records with undecorated names, code sizes and C++ signatures · full LF_CLASS/LF_STRUCTURE type information · parameter and local-variable names · line info |

### The authority boundary — read this before quoting anything below

**The PDB gives names, types, sizes and line numbers. It does not give semantics.** A
confirmed name proves we found the *right function*; it proves nothing about our reading of
what that function computes. Throughout this document a `CONFIRMED` verdict means "the
identity is right and the real name/signature is consistent with our reading" — it never
means "our reading is verified". Where the distinction bites, it is stated inline as
*consistent, not proof*.

Nothing here raises a fidelity tier. A symbol is a name, not a behaviour; values still come
from the oracle. Two further cautions, both paid for during this audit:

- **Identical COMDAT folding makes symbol lookup non-unique.** `0x0041BFE0` is a 3-byte
  `ret 4` stub carrying **38** names, one of which is `CheckSum::walk_test`; `0x0041BFF0`
  carries **186**. A tool that reports the first match will happily tell you a checksum
  routine is `SysMessageHandler::OnPaint`. `tools/pdb/lookup.py` reports all of them, and
  the tables below mark such addresses ⚠ICF. Naming a folded empty stub is meaningless.
- **A name can be more misleading than no name.** `ObjectData::train_time` genuinely is
  train time — but the value it scales is still underived, so the name invites wiring an
  underived quantity into a build queue. Do not let a symbol launder an open question.

### Tooling produced by this audit

| path | what |
|---|---|
| `tools/pdb/build_symtab.py` | parses `llvm-pdbutil dump --symbols/--publics` into a VA-indexed table (procs **with sizes** — the part `schema/rise-symbols.tsv` lacks) |
| `tools/pdb/lookup.py` | `lookup.py <va>` → every symbol covering that VA; `--name <regex>` → search |
| `tools/pdb/callers.py` | direct `E8`/`E9` callers of a VA, each attributed to its containing real function |
| `tools/pdb/xref.py` | references to a VA or to a UTF-16 string literal, attributed the same way |
| `schema/rise-procs.tsv` | 22,752 procedures: `va · size · name · signature` |
| `re/symtab.json` | the combined index the tools load |
| `re/docaddrs.tsv` | **every one of the 854 addresses cited anywhere in `docs/`**, pre-resolved |

```sh
python3 tools/pdb/lookup.py 644130 a1d110 c06184
python3 tools/pdb/lookup.py --name 'PathFinder::'
cd ron-bin && uv run --quiet --with pefile python ../tools/pdb/callers.py 0057fa60
cd ron-bin && uv run --quiet --with pefile python ../tools/pdb/xref.py --str flank_bonus
```

### Relationship to prior work

`docs/tooling/ledger-reconciliation.md` §1–§2 already used the PDB for a first pass and
already caught the caravan-pathfinder mis-scope, the `[0x00C06184]` RNG correction, and ~25
name confirmations. This document is the **exhaustive per-claim sweep** that pass did not
attempt, plus four findings it did not have. Where we agree, that is two independent passes
agreeing; nothing here silently overwrites it.

---

## 1. Scorecard — state it plainly

Two populations were counted, because they answer different questions.

### 1a. Machine-checkable: every address cited in the docs

Every `0x00xxxxxx` / `FUN_00xxxxxx` / `DAT_00xxxxxx` token in `docs/derivation/*.md`,
`docs/provenance-ledger.md`, `docs/binary-ground-truth.md`, `docs/replay-format.md`,
`README-LLM.md`, `docs/CHARTER.md` and `GOAL.md` was extracted and resolved. Full table:
`re/docaddrs.tsv`; the 269 cited more than once are reproduced in Appendix A.

| | count |
|---|---|
| distinct addresses cited | **854** |
| resolve to a named procedure (exact or interior) | **592** |
| resolve to a named data symbol (exact or +≤32) | **134** |
| resolve into a named data object at a larger offset (struct members) | **42** |
| no symbol — string literals in `.rdata`, sizes, sentinels, `0x00000000` | **86** |
| **addresses that landed on a real named thing** | **768 / 854 = 89.9 %** |

That number is *addressing accuracy*, not correctness: it says we pointed at real functions,
not that we read them right. The 86 unresolved are overwhelmingly benign (assert-filename
literals, opcode constants, and integers that merely look like addresses).

### 1b. Semantic: the substantive named claims

Each document was walked end to end and every claim naming an address, a function, a struct
layout or an asserted engine behaviour was given a verdict. Pure Rust-engineering and
benchmark claims were excluded rather than counted as free wins.

| documents | verdicts | CONFIRMED | RENAMED | WRONG | UNRESOLVED |
|---|---:|---:|---:|---:|---:|
| `provenance-ledger.md` + `AUDIT.md` | 241 | 222 | 13 | 6 | 0 |
| `combat.md` + `damage-port.md` | 71 | 28 | 26 | 7 | 10 |
| `economy.md` + `sim-economy.md` | 105 | 62 | 25 | 14 | 4 |
| `checksum.md` + `replay-checksum.md` | 98 | 41 | 38 | 14 | 5 |
| `replay-io.md` + `replay-stream.md` + `AUDIT-replay.md` + `replay-format.md` | 162 | 112 | 34 | 12 | 4 |
| `live-tables.md` + `simd-batch.md` + `gpu-architecture.md` | 84 (+10 N/A) | 63 | 8 | 7 | 6 |
| `pathfinding.md` + `rng.md` | 108 | 44 | 36 | 22 | 6 |
| **total** | **869** | **572** | **180** | **82** | **35** |

**Read that table honestly, in both directions.**

- **752 of 869 claims (86.5 %) are about the right function and survive intact** — either
  confirmed outright (572) or needing nothing but the real name substituted for an invented one
  (180). Across `live-tables.md`, `simd-batch.md`, `gpu-architecture.md` and all four replay
  documents, **every single cited address resolves to the symbol the document said it was.**
- **82 claims (9.4 %) are wrong**, and they are not evenly spread. They cluster in three places
  and each cluster has one cause:
  - **Deriving the general case from a specialised variant** (§2.4). Roughly half of
    `pathfinding.md`'s 22 wrong rows are downstream of one misidentification: the document
    analysed `astar_caravan_road` throughout while believing it was the engine's pathfinder.
  - **Following a string literal into a logger** (§2). `checksum.md` §5–§6, `economy.md` §1.4,
    `live-tables.md` §1, `binary-ground-truth.md`'s descriptor section and `GOAL.md`'s done-log
    are all downstream of one misidentified interface.
  - **Inferring purpose from arithmetic** — a period read as damage, a HUD renderer read as a
    sim site, a tooltip builder read as income composition, a log-format-string argument list
    read as a byte layout, a base-class virtual read as *the* implementation.
- **35 UNRESOLVED are honest gaps, not failures.** They are `.rdata` string literals, plain
  scalar members MSVC omits from the type record, and inlined functions with no symbol.

The variance between documents is itself the finding. `provenance-ledger.md` scores 92 %
CONFIRMED because it had already been reconciled once; `combat.md`/`damage-port.md` score
lowest on CONFIRMED (39 %) almost entirely because they invented the most names, and their
RENAMED rate is the highest at 37 % — those are the same claims, correct, wearing our labels.
`pathfinding.md` has the worst WRONG rate (20 %) for a single structural reason, not because
its work was sloppy: its instruction-level readings are vindicated down to the byte
(`div_3_table`, `move_x`/`move_y`, the `__xmm@…` constants, the rotation tie-break), and they
are readings of the wrong function.

The documents that separated `[measured]` from `[inferred]` and wrote down anomalies instead
of papering over them are the ones this audit could check at all: `combat.md` §5 flagged the
balance-table extent as an unresolved anomaly rather than asserting it, and that flag is
exactly what led to the correct answer.

One result deserves its own line because it inverts the expected direction: **`AUDIT.md`, the
adversarial pass, contains the two most damaging errors in the corpus.** Its `[measured]`
corrections "1,195 instructions, not 1,142" and "`walk_bytes` is 17 instructions" are both
wrong, both overturned *correct* claims, and one propagated into the ledger. Both came from
decoding a byte range whose end was guessed. An adversarial auditor with a wrong yardstick is
worse than no auditor, because their output carries more authority.

### 1c. The uncomfortable headline

**The two largest errors were systematic, not random**, and neither was a lapse of care:

1. **We identified an entire family of *loggers* as *loaders*** (§2.1), because the methodology
   was "follow the rule-name string", and in this binary the name strings are referenced *only*
   from the loggers. The methodology could not have found the loader. It cost roughly a day.
2. **We derived the engine's pathfinder from its caravan-road variant** (§2.4), because
   `astar_caravan_road` is what a reachability sweep surfaces first and nothing in an unsymbolised
   binary distinguishes "the A\*" from "an A\*".

Both consolations are real. The rules-constant artifact built on mistake (1) survives it almost
intact — 719 of 721 names, zero offset errors (§2.3). And where the same people read code
*behaviourally* rather than by name-chasing, they were right at a rate that is genuinely hard to
fault: five `DataWalk` field guesses out of five from ten instructions, 82/82 opcode sizes,
`GameInfo::walk_data` byte-exact, `div_3_table` named correctly by guess (§3).

The uncomfortable part is not the error rate. It is that **both systematic errors were invisible
from inside the methodology** — every check the lanes could run came back green, because the
derivations were internally correct descriptions of the wrong thing.

---

## 2. WRONG FUNCTION — the dangerous class

These are the ones that matter, because a derivation can be internally impeccable while
describing something else entirely.

### 2.1 `FUN_00570170` is `Constants::log_data`, a **logger**. The loader is `Constants::init`.

| our claim | reality [measured, rise.pdb] |
|---|---|
| `FUN_00570170` = "the rules.xml constants loader" | `public: void __thiscall Constants::log_data(class Log*) const`, 63,382 bytes |
| — | the loader is `public: int __thiscall Constants::init(void)` at **`0x00569A90`**, 26,336 bytes, ending exactly where `log_data` begins |
| `FUN_0061C490` = a loader | `UnitType::log_data` (loader: `UnitType::init` `0x0061AB50`) |
| `FUN_0065FC00` = "the combat-stats loader" | `ObjectType::log_data` |
| also | `BuildType::log_data` `0x00631810`, `TechType::log_data` `0x0066D630`, `Type::log_data` `0x006631C0`, `Balance::log_data` `0x00582BD0`, `GameInfo::log_data` `0x005D6040`, `CommandPackage::log_data` `0x0094B7C0` |

**Why we walked into it, precisely.** The methodology was "find the UTF-16 rule-name literal,
follow its xrefs". That methodology *cannot* find this loader:

```
$ xref.py --str flank_bonus
string 'flank_bonus' (utf-16le) at 0x00ac9410 in .rdata
      1  Constants::log_data          <- the ONLY reference in the whole image
  total refs: 1
```

`Constants::init` never touches a name literal. It fetches every name from the **runtime
`StringTable` at `[0x00C06378]` (`int_str_array`)** by fixed byte offset:

```
mov  eax, [0xC06378]      ; int_str_array
mov  eax, [eax + 0x10]    ; -> String list
add  eax, 0x91C8          ; -> the String for this constant (stride 0x14)
push 0xC0                 ; scale = 192
mov  ecx, esi             ; this = Constants
call 0x0057F950           ; Constants::get_fraction(const String&, int)
mov  [esi], eax           ; -> Constants+0x00
```

So string-chasing leads to the logger *every time*, deterministically. This is not a
careless error; it is a methodology with a blind spot, and the blind spot is now documented
in `README-LLM.md`.

**`schema/islands.jsonl` reachability confirms it in hindsight** and nobody read it that way:
a function reachable only from a logging path is not the loader, and `log_data` is virtual on
`Log`.

### 2.2 What actually breaks

| claim | status |
|---|---|
| "`FUN_00570170` is the rules.xml loader" (`binary-ground-truth.md`, `GOAL.md` done-log, `economy.md`) | **WRONG FUNCTION.** Corrected in place. |
| "the descriptor/visitor framework binds rules to fields" | **Right pattern, wrong visitor.** `param_1` is a `Log*`; `vtable+0x1c` records one named value. |
| "the same descriptor tables are plausibly reused for save-game serialization and the checksum" | **REFUTED as stated.** `log_data(Log*)` and `walk_data(DataWalk*)` are separate virtual interfaces implemented separately on each class. The *conclusion* survives — one traversal does define sim-critical state — but it is the `DataWalk` family that does it. |
| "`this+offset` holds a **pointer to** the storage, not the storage" (`GOAL.md`, and an SoA inference built on it) | **REFUTED.** A logger passes the field's **value**. `this+offset` **is** the storage. Verified at `Constants::log_data+2764`: `push [edi+0x4C]` beside `L"flank_bonus"`. |
| "112 binding functions = 112 loaders" | **112 `log_data` visitors.** The bindings are fine (below); the count is not a count of loaders. |

### 2.3 …and what survives, which is most of it

A `log_data` function reads each field **at its true offset** in order to print it. So the
name → offset binding extracted from it is a *correct* name → offset binding — arguably a
cleaner source than the loader, which reaches its names indirectly. This was checked two
independent ways, neither of them assumed.

**Check 1 — names, offsets and array shapes, against the `Constants` type record.** The PDB
carries the whole class: `LF_CLASS Constants`, `sizeof 3432`, field list `0x1A500`, **722
members** (721 value fields + `curr_element : XMLElement` @3392), of which 690 are scalar
`int` and **31 are `int[n]` arrays**.

| | |
|---|---|
| value members in the PDB | **721** |
| entries in `docs/derivation/rules-constants.json` | 719 |
| our names that exist in the PDB | **719 / 719** |
| **offset mismatches** | **0** |
| array element counts matching | **30 / 31** |
| members we are missing | 4 — `liberty_free_upgrades`@1332, `eiffel_siege_range`@1352, `aztec_move_speed`@1396, `spanish_extra_scout`@1664 |
| array we sized wrong | `scholar_rate` is `int[6]`; we have 5 (slot 664 dropped) |

**719 of 721 names, zero offset errors.** For an artifact reverse-engineered out of a
function we had misidentified, that is as good as it was ever going to get.

**Check 2 — parser and scale, against the real loader.** The type record cannot tell you how
a field is *parsed*. `Constants::init` was disassembled end to end and every call to
`Constants::get_item(const String&)` `0x0057FA60` (plain `_wtoi`) and
`Constants::get_fraction(const String&, int scale)` `0x0057F950` was recovered with its
`push imm32` scale and its `mov [esi+disp], eax` destination.

| | |
|---|---|
| call sites in `Constants::init` | **701** — 661 `get_item`, 40 `get_fraction` |
| scale immediates, complete universe | **256 ×24 · 192 ×11 · 100 ×5** |
| distinct scalar destination offsets recovered | 687 |
| **our entries agreeing exactly on (offset, parser, scale)** | **686** |
| entries disagreeing on parser or scale | **0** |

The 33 of ours with no recovered loader slot are the 31 array-valued constants (stored
through indexed addressing this extractor does not follow) plus 2 already flagged
`parser: None`. **The scale set {192, 256, 100} our lanes derived is not merely correct, it
is exhaustive** — there is no fourth scale anywhere in the binary.

So: the function was misidentified, the *artifact* is sound, and there are now three
independent sources for the rules schema — the `log_data` bindings, the `Constants::init`
call sites, and the PDB type record. Keep all three; they cross-check on different axes.

**One caveat the type record exposes, and it is a real one.** `log_data` gives the **C++
member name**, which is not always the **XML tag name**. At least two pairs differ
(`CITY_UPGRADE_TERR` ≡ `city_level_territory_bonus[3]`, `TAJ_CARAVAN_LIMIT` ≡ `taj_caravan`).
`economy.md` §1.4's "name drift between binary and XML — those eight XML entries are inert"
is therefore **unsound in both directions**: three of its "binary-only" names are real
members, and some of its "XML-only" names are the same field under its XML spelling. Only
`internal_string.xml` carries the tag→member join.

### 2.4 The pathfinder we read is the **caravan road** pathfinder

Already caught in `docs/tooling/ledger-reconciliation.md` §1.2; restated because it is the
second-most-consequential mis-scope and this audit adds the call graph.

| address | lane called it | real symbol |
|---|---|---|
| `0x00685990` | "the A* driver" / "the pathfinder" | `PathFinder::astar_caravan_road(Stack<PathData>*, int, int, int, int, int, int)` |
| `0x00686300` | "the per-edge cost" | `PathFinder::calc_road_cost` |
| `0x00688740` | "step legality / passability" | `PathFinderData::valid_roadcoord` |

Call graph [measured, `callers.py`]:

```
PathFinder::astar_path          0x00683770  5845 B  <- find_upath, find_wpath, find_tpath   <-- THE general pathfinder, UNREAD
PathFinder::astar_caravan_road  0x00685990  2411 B  <- find_road                            <-- what we read
PathFinder::astar_river         0x00686690  4762 B  <- Map::make_riverlist, ConsoleWin::run_cmd
```

Everything measured about `astar_caravan_road` remains true **of that search**. Which parts of
the headline transfer to `astar_path` was checked instruction by instruction, not assumed:

| headline claim | `astar_caravan_road` | `astar_path` (the real one) |
|---|---|---|
| pure integer, no FP | yes | **yes** — 0 FP instructions in `astar_path` (1,645) *or* `calc_cost` (860). Across all 45 `PathFinder::` methods (9,063 instructions) the only 20 SSE ops are `xorps`/`movups`/`movaps` struct zeroing and constant copies |
| 8-connected, fixed rotation tie-break | yes | **yes** — same `move_x`/`move_y` rotation, same cardinal-toward-goal seed |
| **"draws RNG once per edge relaxation"** | yes | **NO.** `PathFinder::calc_cost` `0x00684E50` makes **zero** `Random::get` calls. `astar_path`'s only two draws are in unit-order setup (`% 3 + 6`), not per edge |
| one tile (192 units) per step | yes | **no** — parameterised by step granularity, `{0x30, 0xC0, 0x300}` = UCoord / TCoord / WCoord |
| 3,200-expansion node budget | yes | **no** — bounded by `\|dx\|+\|dy\|` and a field at `PathFinder+0x80` |
| the eight cost constants 55/100/100/540/240/200/60/600 | yes | **no** — an exhaustive `.text` scan shows `[0x00E85EE0…0x00E85EFC]` is read by `calc_road_cost` **and nothing else**. They are *road-building* costs |
| the `-goalY` heuristic ⇒ "to within a negligible term this is plain Dijkstra" | yes | **not applicable** — caravan-road-only, and must be de-escalated from a headline |

**The determinism story for unit movement is therefore materially *better* than the document
claims**, which is a pleasant way to be wrong: pathfinding does perturb the shared stream, but
not once per edge. The per-edge coupling is real for caravan road-building and for
`astar_river`/`calc_river_cost`.

Structural bonuses from the symbol table. The open list is a `BRTree<PathNode*, unsigned long>`
behind a `Recycler` pool (`0x0047A160`, `0x0047A2C0`, `0x0047A370`) — not the flat array a
reimplementation would reach for. `PathFinderData` holds **five** ordered containers, not the
three the document found; the two it missed (`Tree<CollBlock*,int>` `0x00E85E8C`,
`BRTree<int,unsigned long>` `0x00E85E90`) are used by `astar_path` and were invisible because
`astar_caravan_road` does not touch them — so the "no iteration-order nondeterminism" sweep
never covered them. And two opaque cost branches now have meanings: `0x006B53F0` is
`WorldData::was_seen`, so **unexplored terrain costs double**, and `0x006EDB50` is
`LeaderData::is_ally`, so the territory surcharge is an **alliance** test.

### 2.5 The `log_data` mistake spread further than the loader

The same misread interface produced confident conclusions in three other documents. All of
these describe `X::log_data(Log*)` calling `Log::say` (the `Log` vtable's slot 7,
`vtable+0x1c`) while believing it to be a data-binding visitor:

| doc | claim built on the misread | status |
|---|---|---|
| `checksum.md` §5 "The refutation" | "the rules-loader visitor class is `TypeOut`, vftable `0x00B43DA4`, slot 7 = `0x00470780` binds `(name, tag, field)`" | **REDO.** `TypeOut` is the type-**display** class (`UnitTypeOut::draw`, `GoodTypeOut::say_income_bonus`). `0x00470780` is `TypeData::is_wonder_type(void) const` — a 26-byte zero-argument predicate that cannot bind anything. The section's *conclusion* (checksum traversal ≠ rules-descriptor traversal) happens to be true, but on entirely different evidence, so it is **untested, not refuted**. |
| `checksum.md` §6 "settings binder" | "`FUN_005D6040` binds `checksum_deep` → `this+0x08` … the third argument really is a *load*, a second instance of the open question" | **REDO.** It is `GameInfo::log_data(Log*) const`; it *prints* three ints. The "open question" was never a question — you load a value to print it. The offsets survive: `GameInfo::checksum_deep`@8, `checksum_window_size`@12, `checksum_failure_threshold`@16, `Game::info`@`Game+0x0C`. |
| `live-tables.md` §1 | "`FUN_0065FC00`/`FUN_0061C490` are the `DataWalk`/checksum methods" | **RENAME + REDO.** They are `ObjectType::log_data` and `UnitType::log_data`. `log_data(Log*)` and `walk_data(DataWalk*)` are different interfaces on the same classes. |
| `combat.md` §10.1 | "`FUN_0065FC00` is the combat-stat descriptor walker" | **RENAMED.** Its *finding* — that the "type tag" is `wcslen(name)` — is confirmed and is exactly the `{const wchar_t*, size_t}` string-view shape a logger would build. |

The lesson is one line: **`log_data(Log*)` and `walk_data(DataWalk*)` are two different virtual
interfaces implemented on the same classes, and the rule-name string literals live only in the
first.** Any future work that finds a name literal has found a logger.

### 2.6 Other wrong-function findings, by blast radius

| our claim | reality | consequence |
|---|---|---|
| `0x00822D6D` is the `peace_attrition` site (`economy.md` §5.3, `sim-economy.md` §5, ledger §5.7) | inside `IFaceSelected::draw_stat` `0x008214D0` — a **HUD stat-panel renderer** computing `attrition / peace_attrition` for display | Found independently by two auditors. Delete the site *and* the "same arithmetic shape as the assassin path" guess. The real border/peace logic is in `Unit::process_attrition` `0x005E11A0` (locals `grace`, `former_enemy`, `former_ally`, `no_rush`). |
| `economy.md` §5.3: "`damage = ATTRITION × strength / 256`, min 1" at `0x005E192B` | `Unit::process_attrition+1931` stores a **period** into `UnitData::attrition : short` @+0x9E under a min-nonzero merge (`if (a == 0 \|\| v < a) a = v`) | Withdraw §5.3. `sim-economy.md` §3.4 already had it right and should be the surviving version. |
| `combat.md` L81/82 + `damage-port.md`'s Tier-B ledger row: `0x006469F0`/`0x00647DB0` are "*the*" attack/armor getters | `ObjectData::attack`/`ObjectData::armor` are **base virtuals**. Vtable slots `+0x120`/`+0x124` for `Unit`/`UnitData` hold `UnitData::attack` `0x006103C0` (1,219 B) and `UnitData::armor` `0x00610160` (593 B); buildings get `BuildData::attack` / `WallData::armor`. `ObjectData::armor` is reached by **no shipped vtable but `ObjectData`'s own**. | The 7,986,695-trial Tier-B row must be **scoped**: it tested the base implementations. For a real unit-vs-unit fight the operand comes from 1,219 bytes of unread code. Independently corroborated by the live damage hook: 0 of 56,789 real calls reached the base functions. |
| `damage-port.md` §5.6 assumed `[0x00C0AB84]` and `[0x00C0AEC0]` alias | `objects.lists+16` (`class Objects objects` @0x00C0AB70) and `units.lists+16` (`class Units units` @0x00C0AEB0) — **two different global containers** | The aliasing assumption is very likely false in general; a building defender would not appear in `units.lists`. Already refuted behaviourally by live capture (§4.3 of the ledger); the symbols say *why*. |
| `checksum.md` §6: the sync-category table is at `0x00C06370`, 38 entries | `0x00C06370` is `String::output_char_buffer_length`. The real symbol is **`sSyncDefines` @ `0x00C06380`**, records `{int id; char abbrev[8]; const wchar_t* name; int flags}` = 20 bytes, **37 entries, ids 0…36**, ending exactly at `0x00C06664` where `LOBBY_DATA_KEY_*` begins. | Every id in the printed table is **off by one**. Anything computing a `DesyncCategoryMask` bit position from that column is wrong. The section's *substantive* point — the categories are a strict superset of the 15 checksum channels (`Terrn`, `Pthfd`, `Animl`, `Sound`, `GpcCd` have categories but no channel) — survives intact. |
| `checksum.md` §2: "`adler32` is called from only 8 sites in the whole image" | **Two `adler32`s ship.** `0x00A46830` (C++ linkage `?adler32@@YAKKPBEK@Z`, 295 B, **register ABI**: `adler` in ECX, `buf` in EDX) has 9 callers, all on the CheckSum/Save/Load path. `_adler32` `0x005089D0` (C linkage, 301 B, **stack ABI** `[ebp+8]`/`[ebp+0xC]`) has 13 — including **`CheckSums::check_groups`**, one of the 15 lockstep channels. | Verified here in both prologues. Any harness must key on the *address*, not on the name; and a reimplementation must not assume all 15 channels go through one primitive. |
| `checksum.md` §3: `DataWalk → WalkDataGame → {SaveGame, LoadGame}`, "exactly six classes" | `SaveGame`/`LoadGame` derive from `DataWalk` **directly** and take `WalkDataGame` (sizeof 4, bases `GameAccess`+`MiscAccess`) as a **virtual base mixin** — it is not a `DataWalk` at all. Transitive closure over the whole TPI gives **five** implementors: `CheckSum`, `SaveGame`, `LoadGame`, `ConquestSaveGame`, `ConquestLoadGame`. | The RTTI base-list parser flattens virtual bases into a chain. Fix the parser, not just the diagram. |
| `replay-checksum.md` §2: "`CommandPackage::process` = `0x0094C500`" | `0x0094C500` is `CommandPackage::process_all`; `CommandPackage::process(struct Command*)` is `0x0094A700` — a *different real function the same document also describes*. | Pure name collision; the behaviour described is correct for `process_all`. Rename before the two get conflated in code. |
| `economy.md`/`sim-economy.md`: "**do not** treat `0x006508C0` as train time" | `public: int __thiscall ObjectData::train_time(int t) const`, locals `time` and `basetime`; every modifier it applies is a `*_speed` / `*_build_speed` / `disband_*_rate` constant | The caveat is **refuted**. `mechanics.rs::ramped_rate` can be named and wired. A rare case where the PDB *removes* a blocker rather than adding one. |
| `sim-economy.md` §3.5: "`SUPPORT` and `PROGRESSION` are not implemented at all — no engine site tied to either name" | `UnitType::support : TypeIndex[2]`@0x268, `UnitType::support_cost : int[2]`@0x270, `UnitType::progression : int`@0x2F4, `Leader::calc_support(int*)` `0x006CEEA0`, `LeaderDataEncrypt::support[6]`@econ+0x7C, `Constants::build_support_factor`@892 | The lane had **already disassembled the SUPPORT table** at `0x00665443` and described it as "a 2-resource-slot cost table" without knowing its name. The abstention can be lifted. |
| `economy.md` §3.2's "next lane" pointers for worker→income | 4 of the 6 cited readers are `BuildTypeOut::set_parse_data` (**presentation**) and `LeaderData::calc_rare` (**rare resources**) | Redirect to `Leader::calc_gather(int* resources)` `0x006CEEE0`, `LeaderData::calc_city_resources` `0x006D5530`, `UnitData::calc_gather` `0x00609180`. `sim-economy.md` §6 had already guessed `0x006CEEE0` correctly. |
| `*(0x00C061C0)` is "the game speed" (`economy.md`, `sim-economy.md`, **and shipped in `crates/don-sim/src/mechanics.rs:1014`**) | `public: static int& GameAccess::ai_speed` | It multiplies income in code we ship. Rename and re-derive what feeds it. |
| ledger §3.1: "`[[0x00C0618C]+0x1F4]` gates 4 object channels; `checksum_deep` is the obvious candidate" | `GameAccess::objects` → `ObjectsData::valid : int` @ offset **500 = 0x1F4** | **Resolved and the hypothesis refuted.** It means "the object lists are initialised", nothing to do with `checksum_deep`. |
| `AUDIT.md` §6 `[measured]`: "`ObjectData::get_damage` decodes to **1,195** instructions, not 1,142" | The function is **3,954 bytes** (`0x00644130`–`0x006450A1`). The audit decoded a **4,048-byte** window to `0x00645100`, sweeping in 14 `int3`, two unrelated leaf functions, padding, and 48 bytes of `ObjectsArray::get_new_data`. | **The audit's correction inverted the truth.** Verified here: exactly 3,954 bytes → **1,142** instructions; the 4,048-byte window → 1,195. `combat.md` was right, and its figure was overturned by a `[measured]` claim and propagated into the ledger. Zero-float is unaffected (0 over both ranges). |
| `AUDIT.md` §6 `[measured]`: "`CheckSum::walk_bytes` is 17 instructions, tail-calls `0x00A46830`" | `CheckSum::walk_function`, 43 bytes → **20** instructions, and it is a regular `call` + `add esp,4` + store, **not** a tail call | Verified here. Every *data-flow* fact the audit lists is right; the count and the tail-call are not. |

**Both of the `AUDIT.md` numeric errors have one cause: decoding a byte range whose end was
guessed.** That failure mode is now permanently retired — `schema/rise-procs.tsv` carries an
exact size for all 22,752 procedures.

---

## 3. CONFIRMED — where a day of work landed exactly on the compiler's own name

These were derived with no symbols, from behaviour and disassembly. That they land on the
real names is the strongest cross-validation this project has.

| our claim | real symbol | note |
|---|---|---|
| `FUN_00644130` is the damage function, `__thiscall`, pure integer | `public: int __thiscall ObjectData::get_damage(int, int, unsigned long, int, int, int*) const` | **arity, calling convention and const-ness all as derived** |
| adler-32 at `0x00A46830` | `unsigned long __cdecl adler32(unsigned long, unsigned char const*, unsigned long)` | zlib's, exactly |
| `FUN_00936560` = lockstep checksum entry | `public: unsigned long __thiscall CheckSums::check_all(void)` | |
| `FUN_009459D0` = checksum command handler | `public: int __thiscall CommandPackage::process_check_sums(struct CheckSumsCommand*)` | the struct name was inferred and is right |
| `0x00A39CF0` / `0x00A39D70` are the RNG | `float Random::get(void)` / `int Random::get(int, int)` | |
| LCG is `s ← s·1664525 + 1013904223` | `imul eax, [ecx], 0x19660D` / `add eax, 0x3C6EF35F` | exact |
| `0x00581CA0` is the balance-table accessor | `public: int __thiscall Balance::return_modifier(TypeIndex, TypeIndex)` | |
| `0x0092CFE0` is the flank classifier | `int __cdecl flanking(unsigned long angle, unsigned long)` | |
| `0x0046CFF0` is a distance helper | `int __cdecl vector_dist(int dx, int dy)` | PDB param names are literally `dx`, `dy` |
| `0x00846450` is dead code, called by nothing, **not** the RNG | `public: int __thiscall Doober::get_num(TCoord, TCoord, int, int)` | **0 direct callers and 0 address-taken references in the entire image** — the "dead code" call was right, and is now stronger than when it was made |
| `[0x00C061E4]` and `[0x00C061F0]` alias one object (live-read) | `GameAccessConst::constantsc` and `GameAccess::constants` | the PDB explains *why*: a const/non-const static reference pair to one `Constants` singleton |
| rule tokenizer is `(num × scale) / den`, scale a per-field compile-time constant of 192 / 256 / 100 | `public: int __thiscall String::fraction(int) const` | see below — confirmed instruction by instruction, and the scale set is **exhaustive** |
| balance table is `int16`, stride 493, 493×493 | `Balance::final_balance_table`, PDB type record `short[493][493]`, 486,098 bytes | our dimensions were exactly right |
| replay writer/reader identities | `RecordGame::write_package` `0x00952FB0`, `RecordGame::read_package` `0x00952D90` | |
| the replay stream has **82** command opcodes `0x00`–`0x51`, each handler returning the packet's byte length | `enum CommandTypes { COMMAND_GROUP = 0 … COMMAND_MARWAN = 81, NUM_COMMANDTYPES = 82 }`; all 82 handlers are `CommandPackage::process_*(XxxCommand*)` | **82/82 handler addresses and 82/82 byte sizes match the `*Command` `sizeof`s.** The single largest exact-agreement result in the audit |
| `CommandPackage` disk order is `frame, play, valid, stamp, u16 size, data` (`replay-io`, against two dissenting lanes) | `struct CommandPackage` sizeof 536: `stamp`@0, `play`@4, `valid`@8, `group`@12, `short size`@16, `u8 data[512]`@18, **`Random padding`**@532 | `replay-io` and `AUDIT-replay` were right; the `padding` member is the engine's own name for the inter-command RNG that `replay-stream` called undecodable |
| `GameInfo::walk_data` writes a tag, a version string, 4/16/4 bytes, 29 single-byte walks, then 8 × 57-byte player blobs at `gi+0x68+i*0x8C` | `GameInfo` field list: `version`@0, `seed`@4, `checksum_deep`@8 … `script_type`@52 (29 bytes), `Player player[8]`@56 stride 140 | **byte-exact**, and the 29 anonymous bytes are now named settings (`map_style`, `map_size`, `difficulty`, `pop_limit`, …) |
| `MoveNearCommand`, `PlayerSpeedCommand`, `CameraCommand`, `GroupCommand` byte layouts | the type records | **byte-exact**, field for field |
| the 10 type-table globals and their populations (364 / 129 / 85 / 1 / 544 / 55 / 122 / 0 / 566 / 50) | `PtrArray<T>` publics at exactly the cited addresses; every population equals a `NUM_*` enumerator | 10/10, and the arithmetic falls out of `enum TypeIndex` |
| `Type` / `ObjectType` / `UnitType` / `BuildType` / `TechType` / `GoodType` field offsets (~100 in total) | the type records | essentially all confirmed; the misses are *omissions* (`display_name`, `to`, `cur_index`, `sound_lookups`) rather than errors |
| `DataWalk` layout guessed from ten instructions: `+4` direction, `+8` set by `CheckSum`, `+0xC` section mask, `+0x10` running adler, `+0x14` byte count | `DataWalk{ input@4, checksum@8, flags@12 }`, `CheckSum{ accum@16, size@20 }` | **five field guesses out of five** |
| the 15 checksum channel walkers, in order | `CheckSums::check_units/…/check_guys`, `LeaderData::walk_data`, `check_cities/items/goods`, `World::walk_data`, `Game::walk_rules_data`, `ScenarioData::walk_data`, `RunTimeEnv::walk_data`; and `struct CheckSumsCommand`'s members are named `units_checksum … script_run_time_checksum, all_checksum` | **15/15**, label for label, from a log string — and the wire struct names them identically |
| the cost constants 55/100/100/540 and 240/200/60/600 live at `.rdata 0x00B69A30`/`0x00B69A60` | the symbols are literally `__xmm@0000021c000000640000006400000037` and `__xmm@000002580000003c000000c8000000f0` — **the constants are spelled out in the symbol names** | 0x37=55, 0x64=100, 0x21C=540; 0xF0=240, 0xC8=200, 0x3C=60, 0x258=600. `PathFinder::init` `0x00689EC0` `movaps` them into `pathfinder+0x160`/`+0x170`. ⚠ **scope**: an exhaustive scan shows they are read by `calc_road_cost` and nothing else — they are **road-building** costs, not general pathfinding costs |
| `init_coord_lookup_array` builds `T[i] = i/3` at `[0x00CAE5FC]` | the function's real name **is** `init_coord_lookup_array`, and the data symbol is literally `int *div_3_table` | the lane guessed the function name correctly |
| the charter's "**27-class Order hierarchy** = the real action space" | `enum OrderIndex` has 28 values (`NONE` + 27); 31 `*Order` classes exist of which 4 (`UnitOrder`, `TargetOrder`, `AirOrder`, `GroupOrder`) are abstract bases ⇒ **exactly 27 concrete** | **CONFIRMED.** Two auditors disagreed here (27 vs 31) and the enum settles it. `FlightCommand::orders` is typed `OrderIndex`, so the sim-side-27 / wire-side-82 distinction is real |
| float story: SSE binary32, no x87 to speak of | per-function census over 22,199 procedure bodies: **22,077 scalar-single**, 829 scalar-double, 71 packed, **0 FMA**, 523 x87 | confirmed, with one honest caveat: ~829 scalar `f64` ops do exist (CRT / UI / timing), so "single precision throughout" is true of the *sim path*, not literally of the image |
| the pathfinder contains no floating point | scanned all **45** `PathFinder::` methods, 9,063 instructions: `astar_path` 0 · `calc_cost` 0 · `astar_caravan_road` 0. The 20 SSE ops in the whole class are `xorps`/`movups`/`movaps` **struct zeroing and constant copies**, not arithmetic | confirmed and *strengthened* — the original claim was scoped to one entry point; it holds for the entire class |

**`String::fraction` in full** [measured, disassembly], because this is the one place a
"confirmed name" is backed by a confirmed *reading*:

```
if (!data)                  return 0;
ebx = _wtoi(text);                          // numerator
p   = wcschr(text, L'/');                   // searches the WHOLE string
ecx = p ? _wtoi(p + 1) : 1;                 // denominator
if (p && ecx == 0)          return 0;       // denominator 0 -> 0, not a trap
return (int)((ebx * scale) / ecx);          // imul then cdq/idiv: signed, truncating
```

`scale` is `[ebp+8]`, the caller's argument — pushed as an immediate at each of the 40
`Constants::get_fraction` sites. Our derivation of this function was right in every
particular including the two edge cases.

---

## 4. RENAMED — every invented name, and its real one

Every ``Class::method`` token in the derivation corpus was checked against the symbol table:
**111 distinct tokens, 35 with no matching symbol.** Of those 35, seven are our own Rust
(`Batch::step_parallel`, `World::new`, `RuleValue::parse`, …) and five are real data symbols
our tooling classified as procedures. The rest are invented names for real functions:

| our invented name | real symbol | address |
|---|---|---|
| `RString::AsScaled` | `String::fraction(int) const` | `0x00A1D110` |
| `Random::next_float` | `float Random::get(void)` | `0x00A39CF0` |
| `Random::in_range` | `int Random::get(int, int)` | `0x00A39D70` |
| `Random::exchange_seed` | `Random::reseed(unsigned long)` | `0x00A39D30` |
| `Random::in_range on global A` ("`__fastcall`") | `int __cdecl random(int, int)` — **`__cdecl`**, wraps `internal_random` | `0x00A39D40` |
| `Player::TickResource` | `Leader::do_gather(void)` | `0x006CE450` |
| `Player::UpdateCommerceCaps` | `Leader::calc_resource_caps` | `0x006CE900` |
| `Object::setPosition` | `SubObject::set_new_location(const Coord, const Coord, …)` | `0x00662680` |
| `Rules::walk_data` | `Game::walk_rules_data(DataWalk*)` | `0x00589550` |
| `DataWalk::walk` / `walk_tag` | `walk_function(void*, void*)` / `walk_test(const String&)` | `SaveGame` `0x0043D730`/`0x0043D840`, `LoadGame` `0x0043D950`/`0x0043DA60`, `CheckSum` `0x00936FF0`/⚠ICF |
| `Type::DataWalk`, `ObjectType::DataWalk` (`live-tables.md` §1) | `*::log_data(Log*)` — a **different interface** | `0x006631C0`, `0x0065FC00` |
| `CommandPackage::process_one` | `CommandPackage::process` | `0x0094A700` |
| `Number::Rational`, `Rules::Load` | narrative names, no counterpart | — |
| `0x33 ping_line` | `process_spline` | — |
| `out_kind` (damage 6th arg) | `int* death_type` | — |

`Random::reseed` deserves its own line: it is a **three-instruction XOR swap**. It installs
the new seed *and returns the previous one*. Our invented name `exchange_seed` described the
behaviour better than the shipped name does, and the behaviour was measured correctly.

**Parameter names now available for the load-bearing functions** (from `S_LOCAL … flags =
param` records — these are compiler output, not inference):

```
int ObjectData::get_damage(int ox, int whom, unsigned long angle,
                           int splash, int overkill, int* death_type) const;
// locals: mask_me, mask_you, flank, z_me, z_you, z_diff, city2, armor,
//         damage, percent, age, japanimation, t, temp
int  flanking(unsigned long angle, unsigned long);   // see §6
int  vector_dist(int dx, int dy);
int  Constants::get_fraction(const String&, int scale);
int  Balance::return_modifier(TypeIndex, TypeIndex);
int  Balance::compute_modifier(TypeIndex, TypeIndex, short*);
```

`angle` as the third `get_damage` argument, feeding `flanking`, is the kind of thing that
takes a day to derive and a second to read.

---

## 5. The RNG streams — the refuted claim, settled

**Refuted claim** (`README-LLM.md`, the ledger's former "Streams" section, `replay-io.md`
§final): *"Pathfinder uses a different `Random` via pointer global `[0x00C06184]`; script/sim
uses fixed object `0x00EB697C`."*

**Reality** [measured, rise.pdb + disassembly]:

| symbol | address | what it is |
|---|---|---|
| `static class Random& GameAccess::game_random` | `0x00C06184` | a **static reference**, statically initialised to `0x00E37A8C` (file bytes `8c 7a e3 00`, with a base relocation) |
| `class Random game_random` | `0x00E37A8C` | **the main simulation stream** — road pathfinder per-edge draw, map generation, units/animals, BHS script API (`MathUtilFuncSet::rand_int` `0x009E1890` loads `[0xC06184]`) |
| `class Random internal_random` | `0x00EB697C` | a **secondary** stream, reached through the free `int __cdecl random(int,int)` `0x00A39D40` (`mov ecx, 0xEB697C`). `Surf.cpp` water, `Scene`, graphics |
| `SoundGlobal::random` | `0x00E85F0C` | audio |
| `SoundType::random` | `0x00E87D3C` | audio |
| `TerrainData::random_frac` | `0x00C8BE38` | a `Fractal`, not a `Random` |

Exactly **four named global `Random` objects** exist (plus `TerrainData::random_frac`, which is
a `Fractal`), enumerated from their dynamic initialisers. Six more are *embedded* — members or
function statics — which is where the old "≥7 streams" figure was really pointing.

**Consequence, and it is the opposite of what the old claim implied:** the pathfinder draws
from the *main* stream, so a pathfinding divergence desynchronises every later draw on that
stream, scripts included. The pathfinding lane's determinism warning stands in direction; the
ledger's mitigation of it was wrong. (Its *magnitude* is smaller than claimed — see §2.4: only
caravan road-building draws per edge.)

**Full census** [measured — capstone walk of all 22,752 procedures, tracking the last write to
`ECX` before each `Random::` call, attributed by the PDB proc table]. `Random::get(int,int)`
has **538** direct call sites, not the 414 in `rng.md`. `Random::get(void)`, the free
`random(int,int)` and `Random::reseed` have **zero** direct callers — the float form is inlined
everywhere.

| object | address | sites | fns | who draws |
|---|---|---:|---:|---|
| **`game_random`** (via `GameAccess::game_random`) | `0x00E37A8C` | **307** | **118** | the simulation: `Leader::random_personality` 27, `Objects::add_flock` 9, `Ammo::init` 8, `Unit::think_scout` 7, 20 `Map*::make_continents` subclasses, `TerrainGroups::*`, `Animal::*`, `Farms::add_animals`, `MathUtilFuncSet::rand_int` (script API), `GameAccess::rnd`, and **every RNG-using `PathFinder` routine** |
| **`internal_random`** | `0x00EB697C` | **92** | **31** | mostly presentation (`RiverSectionOut`, `GraphicPieces`, `GuyOut`, `Surf`, `Scene`, `Particle`) — **but its single largest consumer is `Leader::diplomacy` with 18 sites**, plus `Game::do_frame` and `Unit::repair_damage` |
| `SoundGlobal::random` | `0x00E85F0C` | 23 | 18 | sound-variant selection, including from sim functions (`Nuke::add_nuke`, `Wall::kill_at_tile`, `Group::action_queue_up`) — which is exactly right, since those draws do **not** perturb `game_random` |
| `SoundType::random` | `0x00E87D3C` | 3 | 2 | `SoundType::play`, `get_file_index` |
| embedded: `conquest_game+0x18` | `0x00E86898` | ~48 | ~20 | the whole Conquer-the-World layer |
| embedded: `CommandPackage+0x214` | — | 5 | 4 | `add_command` / `add_spline` / `add_chat` / `add_group` — this is the replay padding RNG, and the PDB names the member `padding` |
| embedded: `juke_box+0x44`, `GameSpy+0x63D0`, `Fractal+0x64`, `Particle::init`'s static `r` | — | 3–6 each | | cosmetic |

⚠ **A new risk the documents do not carry.** `internal_random` was filed as a cosmetic /
graphics stream. Its top consumer is **`Leader::diplomacy`**. If AI diplomacy participates in
the lockstep simulation — and it almost certainly does — then `internal_random` is
**sim-critical too, and it is a second stream a reimplementation must reproduce.** This should
be resolved before anyone builds on "one sim stream".

Three further facts the PDB volunteers, all of which correct `rng.md`:

- **The lockstep protocol tracks RNG state on its own channel**, separate from the checksum:
  `CommandPackage::process_check_random(CheckRandomCommand*)` `0x00946020` and
  `static unsigned long* CommandPackage::random_seeds` `0x00CC02C8`, zeroed each turn by
  `CommandPackage::begin_process`, sitting beside `CommandPackage::checksums`. `rng.md`'s open
  question ("is the RNG state inside the lockstep checksum?") is **answered: no, it rides
  beside it.**
- **`rng.md`'s own refutation is refuted.** It concluded "the RNG has no `__FILE__`/`__LINE__`
  parameter, so the `RandomLogEntry {frame,file,line,seed}` shape is uncorroborated". There is
  such an entry point: `public: static int __cdecl GameAccess::rnd(int, char const*, int,
  char const*)` `0x0043CCA0`. In retail it compiles to `game_random.get(0,0xFFFF) % n` with the
  file/line arguments dead, and `RandomLogEntry`'s ctor/dtor and `ObjectArray<RandomLogEntry>`
  are real emitted code. Reinstate the shape as corroborated-by-signature, logging compiled out.
- **Two seeding claims are mis-scoped.** `0x009A19D0` / `0x009A0950` are
  `ScenarioEditor::init_terrain` / `ScenarioEditor::generate_map` — map-*editor* routines, so
  "the replay/save format already carries the seed word" is **unsupported** by that evidence.
  And of the two "reset to zero on teardown" sites, `0x00586D26` is inside
  **`Game::run_oos_recovery`** — "the RNG is zeroed during out-of-sync recovery" is a different
  and far more interesting fact than "on teardown", and deserves its own investigation.

---

## 6. Open items this audit closed or sharpened

### 6.1 CLOSED — `flanking`'s arity (was ledger §5.6 OPEN)

The PDB declares `int flanking(unsigned long, unsigned long)` — two parameters — while our
Tier-B model passes one in ECX. Resolved: the **emitted** function reads exactly one runtime
argument. `S_LOCAL angle, flags = param` has `S_DEFRANGE_REGISTER ECX` live from the first
instruction; the second declared parameter survives only as two optimised-away frame locals
(`angle_target`, `angle_damage`, both at `EBP+0`). The 30-byte body confirms it — it reads
only ECX. **Our one-argument model matches the shipped code.** The source had two parameters;
this build does not.

### 6.2 CLOSED — the balance table's index domain (was ledger §5.4 OPEN: "live type-ids reach 526, beyond 493")

`0x00C06AFC` is **arithmetically correct and interpretively wrong**, and the PDB says exactly
how. `Balance::return_modifier` is:

```
imul eax, [ebp+8], 0x1ED          ; a * 493
add  eax, [ebp+0xC]               ; + b
movsx eax, word ptr [eax*2 + 0xC06AFC]
```

and `ObjectData::get_damage` inlines the identical form at `0x00644178`–`0x0064418E`. But the
array is `Balance::final_balance_table` at **`0x00C12BF4`** (`combat_table` + 4; PDB type
`short[493][493]`), and

```
0x00C12BF4 − 0x00C06AFC = 49,400 = 2 × (50 × 493 + 50)
```

MSVC folded a **+50 row and +50 column bias** into the constant. Therefore
`return_modifier(a, b) == final_balance_table[a − 50][b − 50]`, and **the table's domain is
TypeIndex 50 … 542**, not 0 … 492. A live type id of 526 is inside the table, not beyond it.
The open item dissolves.

Cross-check by reference counting [measured, `xref.py`]: `0x00C12BF4` is referenced from
`Balance::fill_tables`, `Balance::log_data` and `Balance::walk_rules_data` (so the table is
in the save/checksum traversal); `0x00C06AFC` is referenced from exactly two places,
`Balance::return_modifier` and `ObjectData::get_damage` — the two read paths, and nothing
else. `live-tables.md` and `provenance-ledger.md` §4.5 had already derived all of this
without the PDB; the PDB confirms it name for name and number for number.

`combat.md` §5's "unresolved anomaly" and `AUDIT.md` §8 are therefore **resolved**: the
`0x48`-stride `.bhs`/`.xml`/`.dtd` records seen at `0x00C06AFC` are simply what lives 49,400
bytes before the table.

### 6.3 SHARPENED — the loader/logger split gives a second constant extractor

See §2.3. `Constants::init` yields (offset → parser → scale) independently of the name
bindings from `log_data`. Two extractors keyed on two different functions, agreeing on 686
entries, is a materially better position than one extractor whose function we had misnamed.

### 6.4 CLOSED — `DataWalk`'s member names, and the honest form of "sim-critical ≡ save-game"

The layout `checksum.md` §3 guessed from ten instructions is confirmed field for field, and
the PDB supplies the names:

```
struct DataWalk                                  // abstract, sizeof 16
    vfptr                                @+0x00
    int  input                           @+0x04  // we guessed "direction: non-zero => reading"
    int  checksum                        @+0x08  // we guessed "0 by ctor, 1 by CheckSum"
    int  flags                           @+0x0C  // we guessed "section mask"
    virtual void walk_function(void*, void*)        vft+0   pure
    virtual void walk_test(class String const&)     vft+4   pure
struct CheckSum : DataWalk                       // sizeof 24
    unsigned long accum                  @+0x10  // we guessed "running adler-32"
    unsigned long size                   @+0x14  // we guessed "bytes walked"
```

Three field guesses out of three, from ten instructions. `walk_test` taking a
`String const&` — a **section name** — also explains why `CheckSum::walk_test` is a 3-byte
no-op: section markers are excluded from the hash by construction. That is a better story
than the document had.

**But "sim-critical state ≡ save-game state" must be weakened to ⊆.** The *interface* and the
per-class `walk_data` bodies are shared; the **roots are not**. Save/load walks from
`WalkDataGame::walk_data` `0x005A2360` (4,655 bytes — the whole game). The checksum walks
from `CheckSums::check_all`, which calls **15 hand-picked** channel walkers. And
`DataWalk::flags` is passed per call (`World::walk_data(DataWalk*, int)`,
`PtrArray<T>::walk_data(DataWalk*, int)`), so even a shared body can cover different bytes.

Roughly 180 classes implement `walk_data` — including `Camera`, `Console`, `MessageWin`,
`Scene`, `GraphicEvents`. **Those are saved and are not sim-critical.** The
simulation/presentation line is drawn by the 15 checksum roots, not by the existence of a
`walk_data`. A reimplementation must mirror the *checksum* roots.

Two braces the documents did not have, both stronger than the base-class argument:
`SaveGame::verify_save` and `LoadGame::verify_load` call the `CheckSums::check_*` channel
walkers **directly**, so the save path validates itself with the same channels the lockstep
check uses; and `GameLog::say_checksum` is a third consumer. Also worth knowing before
anyone goes hunting: `CheckSums` declares ~55 `check_*` methods (`check_pathfinder`,
`check_terrain`, `check_regions`, `check_storm`, …) and **~40 have zero callers** — dead
code, not a hidden deeper mode.

### 6.5 CLOSED — three replay opcode sizes, and a whole missed channel

`replay-checksum.md` §10's open items dissolve against the command struct type records:
`0x4A` = `TurnDataCommand` **sizeof 11** (so the MP residue is a framing/trailer issue, not a
size error), `0x33` = `SplineCommand` **14**, `0x44` = `ChatCommand` **19**.

And a real gap: opcode **`0x38`** is `CommandPackage::process_check_random(CheckRandomCommand*)`
with `struct CheckRandomCommand` = packed **5 bytes** (`Command` opcode + `unsigned long
seed` @+1). **Neither replay document noticed that the RNG seed crosses the wire.** In this
build the handler is *log-only* — it calls `SyncLogger::logToMemory(...L"process_check_random
seed: %u game.frame: %u"...)` and returns 5, with no comparison and no store; and
`CommandPackage::random_seeds` `0x00CC02C8` is written on the `begin_process` /
`CommandManager::process_turn` path instead. So "the RNG seed is part of the lockstep check"
is directionally right and mechanically *not* what the names suggest. Either way, **every MP
replay we already hold contains per-turn RNG ground truth we have not harvested** — the
cheapest RNG oracle available, one opcode away from work already done. Likewise opcode
`0x3A` = `process_next_check_sum(NextCheckSumCommand*)`, 6 bytes, a second lockstep-check
channel no corpus scan has looked at.

### 6.6 A cross-cutting hazard: this build is LTCG'd, so declared ≠ emitted ABI

The PDB's mangled names record the **source** prototype; link-time code generation changed
several actual calling conventions. Confirmed cases:

| symbol | declared | emitted |
|---|---|---|
| `?adler32@@YAKKPBEK@Z` `0x00A46830` | `__cdecl(unsigned long, unsigned char const*, unsigned long)` | `adler` in **ECX**, `buf` in **EDX** |
| `_adler32` `0x005089D0` | `__cdecl` | genuinely stack (`[ebp+8]`, `[ebp+0xC]`) — *the two copies differ* |
| `?vector_dist@@YAHHH@Z` | `__cdecl(int, int)` | `dx`/`dy` in ECX/EDX |
| `?flanking@@YAHKK@Z` | `__cdecl(unsigned long, unsigned long)` | one argument, ECX, plain `ret` |
| `?get_num@Doober@@QAEHVTCoord@@0HH@Z` | `__thiscall` | ECX never read; `ret 0x10` |
| `?return_modifier@Balance@@QAEHW4TypeIndex@@0@Z` | `__thiscall` | `this` never read; `ret 8` |

**The measured convention wins.** Do not "fix" a working oracle harness to match a mangled
name. This is the sharpest illustration in the whole audit of the authority boundary: the
PDB is authoritative about *what the source said*, and the source is not what runs.

---

## 7. Prioritised list — derivations that must be REDONE

Ordered by blast radius. "Redo" means *the derivation described the wrong thing*; work that
is merely renamed is not on this list.

### Tier 1 — a section describes the wrong subsystem. Rewrite, do not patch.

1. **`checksum.md` §5 "The refutation" (L252–285) and §6 "settings binder" (L280–285, L416–428).**
   Built entirely on `X::log_data(Log*)` / `Log::say` misread as a data-binding visitor.
   `TypeOut` is the type-*display* class; `0x00470780` is `TypeData::is_wonder_type(void) const`,
   a zero-argument predicate. **Consequence: `binary-ground-truth.md`'s descriptor inference was
   never actually refuted — it was untested.** The conclusion (checksum traversal ≠ rules
   traversal) is true on *different* evidence and must be re-argued from `walk_data` vs
   `log_data`. Retract the "refutation banked" bullet and the ledger row it feeds.
2. **`economy.md` §5.3 (attrition).** `0x005E192B` stores an attrition **period**, not damage;
   `0x00822D6D` is a HUD renderer. Withdraw the section; `sim-economy.md` §3.4 is the correct
   version and should be the only one left standing.
3. **`economy.md` §3.2 (income steps).** Step order, the unconditional 16,000 cap, and the
   `(cap + rule) << 4` clamp are all wrong; `sim-economy.md` §4 is right and PDB+decomp back it.
   Fold §4 back into §3.2 so nobody reads the wrong one first.
4. **`economy.md` §1.4 (name drift) and the "first complete rules.xml schema" claim.** Unsound
   in both directions — `log_data` does not enumerate every member, and XML tag ≠ C++ member
   name. Replace with the PDB field list, which is complete by construction.
5. **`replay-stream.md` §"engine name and parameters" column (L132–214).** It is **log-format-string
   order, not byte order**, and it reads like a layout. Six commands are wrong as layouts:
   `MoveToCommand` (5 of 9 fields misplaced), `EjectAllCommand`, `FlightCommand`, `TurnDataCommand`,
   `NextCheckSumCommand` (3 fields listed, 2 exist), `HotKeyCommand` (2 fields missing, x/y
   misplaced). Replace the column wholesale with the `*Command` type records.

### Tier 2 — a specific derived value is wrong and would be implemented wrong

6. **`replay-io.md` L134: the per-player Leader stride `0x1bbb`.** The real byte stride is
   `sizeof(Leader) = 0x6EEC` (`imul ecx, edi, 0x6eec` in `write_random_game_info`); `0x1bbb` is
   the stride in **dwords**. Any decoder written from that line indexes the wrong Leader by 4×.
   Single most implementable error found.
7. **`AUDIT.md` §6 + ledger L846: revert "1,195 instructions" to 1,142**, and
   `CheckSum::walk_function` to 20 instructions with a regular call. Both `[measured]`, both
   propagated, both from decoding a guessed byte range. Verified here.
8. **`checksum.md` §6 L385–405: sync-tag table.** Base `0x00C06380` (`sSyncDefines`), **37**
   entries, ids **0…36**. Every id in the printed table is +1; any `DesyncCategoryMask` bit
   computed from that column is wrong.
9. **`checksum.md` §4 L231: `CheckSumsCommand` is 65 bytes**, `all_checksum` at `+0x3d`. Fix the
   doc and ledger L487. (`replay-checksum.md` was right; the PDB settles it independently.)
10. **`checksum.md` §2 L97: two `adler32`s ship**, with different ABIs, and
    `CheckSums::check_groups` uses the *other* one. Any harness must key on the address.
11. **`checksum.md` §3 L130–147: redraw the class tree.** `WalkDataGame` is a virtual-base mixin,
    not a `DataWalk`; there are **five** implementors, not six. The RTTI base-list parser is
    flattening virtual bases — fix the parser, not just the diagram.
12. **Five wrong opcode names across both replay docs**, three semantically misleading:
    `0x0F` = `board_ship` (not `set_transport_o`); `0x19` = `build` (not `queue_up_build`);
    **`0x2D` = `propose_attack`, not any kind of tribute**; `0x33` = `spline` (not `ping_line`);
    `0x44` = `chat` and `0x45` = `chat_set` (`replay-io` calls `0x45` `chat_stats`).
13. **`replay-format.md` L82: the `recordgame.tmp` direction is inverted** relative to
    `replay-io` §2, which is the correct reading.
14. **`AUDIT-replay.md` L147: the slot-index byte is at `name_len − 6`** (`Player::who`), not
    `− 4` (`Player::handicap`, which measures 0,0,0).
15. **`sim-economy.md`/`economy.md`: `*(0x00C061C0)` is `GameAccess::ai_speed`, not game speed** —
    and it multiplies income in shipped `crates/don-sim/src/mechanics.rs:1014`.
16. **`replay-io.md` L63: `0x00B67F24` is the ASCII string `"zlib error"`**, not a version string.
    The conclusion (zlib is statically linked) survives; the evidence row does not.

### Tier 2b — further wrong values, from `live-tables`, `gpu-architecture`, `pathfinding`, `rng`

42. **`domain` is not a three-valued Land/Sea/Air enum.**
    `enum DomainIndex { GROUND = 0, SEA = 1, BOTH = 2, AIR = 2, NUM_DOMAIN = 3, REAL_AIR = 4 }`.
    Value 2 is **aliased** between `BOTH` and `AIR`, and there is a separate `REAL_AIR = 4`.
    Anything that branches on `domain == 2` meaning "air" mis-handles amphibious types.
    (`live-tables.md` §4h.) This is the most implementable wrong *value* in that document.
43. **Three `sizeof` hints are wrong** (they were honestly labelled hints, so this is cheap):
    `UnitType` is **1496**, not 1536; `BuildType` **844**; `TechType` **648**. Any record-stride
    assumption built on them should take the PDB number.
44. **`preq1` ← `military_level` has the causality backwards.** The numeric bijection is exactly
    right and now has an engine constant behind it (`BASE_MILITARYTYPES = 572`), but
    `UnitTypeData::get_military_level_slow` `0x0061D4D0` derives `military_level = mil_tech_id − 571`
    **from** the prerequisite. The practical conclusion (XML alone under-gates units) survives;
    the open question is now *what writes `preq[1]`*.
45. **`live-tables.md` §4f's "lossy" name→id map has a named root cause.** `Type` carries seven
    `String`s; the doc matched on `display_name` (+0x74, which collides) instead of `name`
    (+0x60, which is unique). Redoing the map on `name` should take `JUMP` and `FROM` to 364/364
    mechanically.
46. **`gpu-architecture.md` §1a: withdraw the `BorderSpline` fidelity caveat as written.**
    The class is real, but its entire cluster is render-side (`BorderSplineRender::draw`,
    `Border::generate_strip`, `TerrainBorder::render`). Simulation territory is
    `World::compute_reg_territory` `0x006B0BB0` over a `Region` grid at **half** WCoord
    resolution. A territory *field* is therefore a **closer** analogue to the engine than the
    doc believed — the caveat argues the wrong way round.
47. **`gpu-architecture.md` §7: `PathFinder` is no longer "a measured RTTI class name and nothing
    more".** 34 named methods including `astar_path`, `add_to_openlist`, `first_open_node`,
    `calc_cost`, `calc_river_cost`, `kill_tree`, over three coordinate spaces (T/U/W). **The
    engine uses A\* with an open list, not a relaxation field** — so the flow-field prototype is
    a deliberate divergence, not an unknown. Say so.
48. **`gpu-architecture.md` §2/§6.8/§7: the map-size unit is settled.** `Game::init` `0x0058C480`
    loads the `mapsizes` `DATA` field (40…100), optionally ×1.1, hard-caps at 100, and passes it
    as `World::init(xs, ys)` `0x006B76F0`, which sets `tile_xs = xs*4`, `fog_xs = xs*2`,
    `reg_xs = xs/2`. The conversions are explicit: `WCoord::operator TCoord` `0x004613B0` is
    `w*4 + 2`; `TCoord::operator WCoord` `0x0046FAB0` is `t >> 2`. **`DATA` is in WCoords, so the
    largest map is 100×100 WCoords = 400×400 TCoords = 160,000 tiles.** The doc's own "at 400²
    the picture changes" branch is the live one, and the 64²/128²/256² benchmark sweep
    under-samples the real top end.
49. **`live-tables.md` §1's "`DataWalk`" functions are `log_data`, and there are *three* walkers
    per class, not one.** `Type::log_data` `0x006631C0` vs `Type::walk_rules_data` `0x00663190`;
    `ObjectType` `0x0065FC00`/`0x0065FBA0`; `UnitType` `0x0061C490`/`0x0061D190`; `BuildType`
    `0x00631810`/`0x00631F50`; `TechType` `0x0066D630`/`0x0066D5C0`; `GoodType`
    `0x0066F2C0`/`0x0066FAB0`; `Balance` `0x00582BD0`/`0x00582CC0`; `Types::walk_data`
    `0x00669780` vs `Types::walk_rules_data` `0x00669800`. **Every "in the walker ⇒ in the desync
    checksum" inference must be re-sourced**, and note further that `walk_rules_data` is the
    *rules* traversal, not the per-frame one. (The doc's specific finding that `UnitType+0x2CC`
    and `+0x2D0` are skipped happens to be *right* — `walk_rules_data` really does skip
    `[0x2CC, 0x2D4)` — but it was right by accident.)
50a. **`pathfinding.md` — kill the "RNG once per edge relaxation" headline** and rewrite §3, §6
    and §7 against `PathFinder::astar_path` `0x00683770` → `PathFinder::calc_cost` `0x00684E50`,
    which is **unread**. The cost model, the eight constants, the 3,200-node budget, the
    resumable-slot machinery (those "slots" are `Caravan` objects in `class Caravans caravans`
    @`0x00E3A2A0`), the mode A/B split and the `-goalY` Dijkstra finding are all
    caravan-road-only. Also: the entry-point list must be rebuilt (`0x0068BC90` is `Map::make`,
    not a pathfinder entry at all), and the 187-function FP cone must be recomputed from the
    real roots.
50b. **`rng.md` — recount, and fix four attributions.** 538 `Random::get(int,int)` sites (not
    414); `vector_dist` 216 sites in 122 functions (not 105); `game_random` 307/118 (not
    239/93); `internal_random` 92/31 (not 69/26). The four "CRT initialisers" at `0x00AC0B90`
    etc. are `dynamic atexit destructor` thunks, not initialisers — the effect (state = 0) is
    unchanged and, pleasingly, that list is exactly the four global `Random`s, so the document
    found the right set for the wrong reason.
50c. **`live-tables.md` §5's `0x00581CC0` "wrapper above the accessor" is backwards.**
    `Balance::compute_modifier(TypeIndex, TypeIndex, short*)` is the **fill-side** computation
    called by `fill_tables`; it never calls `return_modifier`. And its `[0x192, 0x19E)`
    short-circuit to 100 is a devirtualised `TypeData::is_gaia_type` `0x004707A0` over exactly
    `[BASE_GAIATYPES = 402, END_GAIATYPES = 414)` — gaia types have no combat class.

---

### Tier 3 — scope corrections: right function, over-broad claim

17. **Pathfinding headline.** Everything measured is about `PathFinder::astar_caravan_road`. The
    general search is `PathFinder::astar_path` `0x00683770` ← `find_upath`/`find_wpath`/`find_tpath`,
    with `PathFinder::calc_cost` `0x00684E50` — together ~5× the code the lane read, and
    **named nowhere in the ledger**. Also `0x00688A40` is `find_road`, so listing it as a
    "non-road entry point" is wrong.
18. **`get_attack`/`get_armor` Tier-B scope.** The 7,986,695-trial row tested
    `ObjectData::attack`/`armor`, the **base virtuals**. Units dispatch to `UnitData::attack`
    (1,219 B) and `UnitData::armor` (593 B); buildings to `BuildData::attack` / `WallData::armor`.
    Re-label the row and add the four overrides to the underived list.
19. **"sim-critical state ≡ save-game state" → "⊆".** Shared interface and shared per-class
    bodies, **different roots** and a per-call `flags` mask. ~180 classes have `walk_data`,
    including `Camera`, `Console`, `Scene`, `MessageWin`. Mirror the 15 **checksum** roots.
20. **`replay-stream.md` L418: the `*Sync` strings are SyncLogger channel names**, a superset
    logging decomposition — not the checksum definition. Cite `CheckSumsCommand`'s members.
21. **15 fps is the *display clock*.** `IFaceMainBase::draw_game_timer` computes
    `(frames + 14) / 15` seconds. Wall pacing is `TurnControl::timings = {200,125,67,50,1}` ms
    indexed by `GameInfo::game_speed`. State them as two separate facts.

### Tier 4 — open items that can simply be closed (free wins)

22. `GameInfo+0x04` **is** `GameInfo::seed` — promote from "leading candidate" in three docs.
23. Nation ids: `enum TribeIndex`, `DUTCH = 22`, `RANDOM_TRIBE = 24`. `replay-stream.md` §7 closes.
24. Map settings: all 30 `GameInfo` bytes are named fields (`team_style` … `script_type`).
    `replay-stream.md` §7 "not established" closes.
25. Type ids: `enum TypeIndex`, `NUM_TYPES = 806`, `BASE_UNITTYPES = 50`, `NUM_UNITTYPES = 352`,
    `BASE_BUILDTYPES = 414`, `NUM_BUILDTYPES = 129`, `BASE_TECHTYPES = 544`, `END_BUILDTYPES = 543`.
    Note this closes §6.2 cleanly: the balance table's domain `[50, 543)` is exactly
    *units + gaia + buildings, excluding techs* — 352 + 12 + 129 = **493**.
26. `game+0x550` is `Game::frame`; `game+0x820` is `Game::semaphore` (`BitMask<256>`);
    `game+0x10` is `Game::info.seed`. AUDIT-replay §4 closes.
27. Replay section tag = **low byte of `String::hash_value_insensitive`** (hence the `towlower`
    import). `replay-io.md` §7.4 largely closes.
28. Opcode sizes `0x4A` = 11, `0x33` = 14, `0x44` = 19. `replay-checksum.md` §10 closes; the MP
    residue is a framing/trailer issue, not a size error.
29. `flanking`'s declared-vs-emitted arity (ledger §5.6) — resolved, keep the one-argument model.
30. `[[0x00C0618C]+0x1F4]` is `ObjectsData::valid`, not `checksum_deep` — hypothesis refuted.
31. `Balance::fill_tables(XMLElement)` `0x005823F0` ← `Balance::init` `0x00582D20` is the
    `balance.xml` expander the ledger lists as "not located".
32. `Leader::calc_gather(int* resources)` `0x006CEEE0` is the worker→income doorway
    (`sim-economy.md` §6 guessed the address correctly).
33. `SUPPORT`/`PROGRESSION` have engine sites; lift `sim-economy.md` §3.5's abstention.
34. `ObjectData::train_time(int t)` — lift the "do not treat as train time" caveat; `x` is
    `basetime`.
35. Resource slots 4 and 5 are `METAL` and `OIL` (`enum` order `FOOD, TIMBER, WEALTH, KNOWLEDGE,
    METAL, OIL`). `sim-economy.md` §3.2's refusal to name them can be lifted **from the binary**.
36. The charter's "27-class Order hierarchy = the real action space" is **confirmed**:
    `enum OrderIndex` has 28 values (`NONE` + 27), backed by 31 `*Order` vftables of which 4 are
    abstract bases. And `replay-stream.md`'s conclusion is right — mask against the **82**
    `CommandTypes`, not the 27 orders.

### New work this audit created

37. **Opcode `0x38` `CheckRandomCommand{u32 seed}` is in every MP replay we already hold** and no
    corpus scan has harvested it. Free per-turn RNG ground truth. Likewise `0x3A`
    `NextCheckSumCommand` — a second lockstep-check channel nobody has looked at.
38. **`ConsoleCmdCommand` is 521 bytes but `CommandPackage::data` is `unsigned char[512]`.**
    Either `0x4E` is never wire-transmitted or the engine overruns. Treat `0x4E` as suspect.
39. **`schema/islands.jsonl`'s two known gaps now have names**: `Object::do_damage` `0x0064A480`
    (9,214 B) and `UnitType::init` `0x0061AB50` (4,855 B) — two of the largest functions in the
    combat/type region. The extraction gap is worse than "two addresses missing" sounds.
40. **Coverage denominators, now numbers rather than feelings**: the image has ~246 `::log_data`
    methods (the binding extraction used 112) and ~263 `::walk_data` methods (the checksum
    traversal reached ~106). Both passes are under half-covered.
41. **`CommandManager::issue_*`** mirrors all 82 commands on the send side — the right place to
    learn how commands are *built*, which no lane has looked at.

## 8. Stale statements corrected in this pass

Both statements named in the audit brief were refuted and have been corrected in place.
Nothing was committed or staged.

### 8.1 "`riseofnations.exe` has no shipped PDB"

| file | was | now |
|---|---|---|
| `docs/binary-ground-truth.md` §PE | "Shipped PDBs exist for SkyBox's support libraries … but **not** for the game itself. `rise.pdb` is not shipped." | full correction with the GUID/age match table, the artifact list, and the authority boundary |
| `docs/oracle-architecture.md` §"The PDB question" | "The PDB for *our* build is not public." → treated as an open lead, with a plan to downgrade Steam depots to hunt older GUIDs | retitled **CLOSED — we have it**; the symsrv-404 fact is kept (it is still true, and it is *why* the wrong conclusion was reasonable) and the depot-downgrade lead is marked closed |
| `README-LLM.md` | no mention of the PDB at all | new §"The shipped PDB — read this before doing any RE", with invocations and the explicit "gives names, not semantics; raises no tier" boundary |

### 8.2 "pathfinding has its own RNG object separate from the main sim stream"

| file | was | now |
|---|---|---|
| `README-LLM.md` §Established ground truth | "Pathfinder uses a *different* `Random` via pointer global `[0x00C06184]`; script/sim uses fixed object `0x00EB697C`." | corrected to `GameAccess::game_random` → `game_random` `0x00E37A8C` = the main stream; `internal_random` `0x00EB697C` = the secondary stream; **and** the determinism consequence stated in the correct direction |
| `docs/derivation/rng.md` §1 | "`Random::in_range on global A` … `__fastcall` wrapper over `0x00eb697c`" | real names, `__cdecl`, and a flagged correction that `0x00EB697C` is `internal_random` and **not** the sim stream; the five-`Random` enumeration added |
| `docs/derivation/replay-io.md` §open-items | "the sim `Random` object is `0x00EB697C`" | flagged correction to `game_random` `0x00E37A8C` via `[0x00C06184]` |
| `docs/provenance-ledger.md` §4.1 | — | already corrected before this audit; PDB names now corroborate it independently |

### 8.3 Other stale statements corrected while here (same error class)

| file | correction |
|---|---|
| `docs/binary-ground-truth.md` §descriptor/visitor | `FUN_00570170`/`FUN_0061C490`/`FUN_0065FC00` are `*::log_data`, not loaders; the visitor is `Log*`; the "descriptor tables are reused for save/checksum" inference refuted and redirected to `walk_data(DataWalk*)`; the "`this+N` is a pointer to storage" inference refuted |
| `GOAL.md` | correction block: the loader identity, the `tag`-field question (closed — it is `strlen(name)`), the tokenizer question (closed — `String::fraction`), and the refuted SoA inference |
| `README-LLM.md` | damage/pathfinding/checksum/tokenizer/balance-table entries rewritten with real symbols; the caravan-vs-general pathfinder scope made explicit; the string-chasing blind spot added to Gotchas |

### 8.4 Not corrected — flagged for the owner

`crates/don-rules/src/rules.rs` carries `RString::AsScaled` in ~40 doc comments (lines 13,
71, and one per scaled constant). The **arithmetic is right and the offsets are right**; only
the name is invented. Not edited here: this audit touched no code, and parallel lanes are
live in the tree. One mechanical rename, `RString::AsScaled` → `String::fraction`.

---

## Appendix A — every address cited more than once, resolved

269 rows. The full 854 (including single mentions and the 86 unresolved) are in
`re/docaddrs.tsv`. `fn` = inside a named procedure; `data` = at or within 32 bytes of a named
data symbol; `data≈` = a member offset inside a larger named object; `+n` = bytes into the
symbol; ⚠ICF = the address is COMDAT-folded and carries multiple names, so this one is
arbitrary.

Doc keys: `cbt` combat · `pth` pathfinding · `rng` rng · `chk` checksum · `eco` economy ·
`dmg` damage-port · `rio` replay-io · `rst` replay-stream · `rck` replay-checksum ·
`sec` sim-economy · `ltb` live-tables · `smd` simd-batch · `gpu` gpu-architecture ·
`AUD` AUDIT · `AUR` AUDIT-replay · `LDG` provenance-ledger · `bgt` binary-ground-truth ·
`RDM` README-LLM · `GOL` GOAL · `rfm` replay-format · `pdt` pdb-types

| address | kind | real symbol | cites | docs |
|---|---|---|---|---|
| `0x0041bfe0` | fn | `Buffer::set_pending_load` ⚠ICF | 4 | chk·LDG |
| `0x0042d9e0` | fn | `String::num` | 5 | eco·LDG |
| `0x0043d730` | fn | `SaveGame::walk_function` | 5 | chk·LDG·rio |
| `0x0043d840` | fn | `SaveGame::walk_test` | 4 | rfm·rio |
| `0x0043d950` | fn | `LoadGame::walk_function` | 4 | chk·LDG·rio |
| `0x0043da60` | fn | `LoadGame::walk_test` | 2 | rio |
| `0x0046cff0` | fn | `vector_dist` | 10 | AUD·pth·LDG |
| `0x0046d060` | fn | `vector_dist` | 2 | pth |
| `0x0046f020` | fn | `vector_dist` | 2 | pth |
| `0x0046fa40` | fn | `UnitData::is_idle` | 2 | sec |
| `0x0047a160` | fn | `Recycler<BRTree<PathNode *,unsigned long> >::pop` | 2 | pth |
| `0x0047a2c0` | fn | `Recycler<BRTree<TreeNode<PathNode *,int> *,unsigned long> >::pop` | 2 | pth |
| `0x0047a370` | fn | `Recycler<Tree<PathNode *,int> >::pop` | 2 | pth |
| `0x00509140` | fn | `gzread` | 2 | rio |
| `0x00509350` | fn | `gzwrite` | 2 | rio |
| `0x00542579` | fn | `TextureEffect::increment` +121 | 2 | AUD·rng |
| `0x0054258f` | fn | `TextureEffect::increment` +143 | 2 | AUD·rng |
| `0x0055e0a6` | data | `__purecall` | 2 | chk |
| `0x00569a90` | fn | `Constants::init` | 9 | GOL·RDM·bgt·eco·LDG |
| `0x0057016f` | fn | `Constants::init` +26335 | 3 | eco·LDG |
| `0x00570170` | fn | `Constants::log_data` | 26 | AUD·CHT·GOL·RDM·bgt·cbt·eco·pdt·LDG |
| `0x00573c69` | fn | `Constants::log_data` +15097 | 3 | AUD·eco·LDG |
| `0x0057f950` | fn | `Constants::get_fraction` | 8 | bgt·eco·LDG |
| `0x0057fa60` | fn | `Constants::get_item` | 9 | bgt·eco·LDG·sec |
| `0x00581ca0` | fn | `Balance::return_modifier` | 11 | AUD·cbt·ltb·LDG |
| `0x00581cc0` | fn | `Balance::compute_modifier` | 2 | ltb·LDG |
| `0x00582bd0` | fn | `Balance::log_data` | 4 | ltb·LDG |
| `0x00589550` | fn | `Game::walk_rules_data` | 8 | chk·LDG·rck·rio |
| `0x00589600` | fn | `Game::walk_data` | 8 | AUR·bgt·LDG·rfm·rio |
| `0x00591ef0` | fn | `Game::do_frame` | 2 | LDG·sec |
| `0x005924cf` | fn | `Game::do_frame` +1503 | 3 | LDG·sec |
| `0x005d6040` | fn | `GameInfo::log_data` | 3 | chk·LDG |
| `0x005d6570` | fn | `GameInfo::walk_data` | 5 | LDG·rio |
| `0x005d7460` | fn | `Animal::do_idle` | 2 | LDG·rng |
| `0x005d79e0` | fn | `Animal::think_bird` | 2 | LDG·rng |
| `0x005e11a0` | fn | `Unit::process_attrition` | 2 | LDG |
| `0x005e15dd` | fn | `Unit::process_attrition` +1085 | 3 | eco·LDG·sec |
| `0x00608fd0` | fn | `UnitData::get_attrition` | 5 | eco·LDG·sec |
| `0x00609122` | fn | `UnitData::get_attrition` +338 | 2 | eco·sec |
| `0x0060ee50` | fn | `Unit::close` | 2 | LDG·rng |
| `0x00610160` | fn | `UnitData::armor` | 3 | LDG |
| `0x006103c0` | fn | `UnitData::attack` | 3 | LDG |
| `0x006115ea` | fn | `Unit::process` +2602 | 3 | LDG·sec |
| `0x006117c8` | fn | `Unit::process` +3080 | 3 | LDG·sec |
| `0x0061ab50` | fn | `UnitType::init` | 6 | RDM·bgt·cbt |
| `0x0061c490` | fn | `UnitType::log_data` | 15 | CHT·bgt·chk·eco·ltb·pth·pdt·LDG |
| `0x0062e610` | fn | `BuildData::attack` | 3 | LDG |
| `0x00631810` | fn | `BuildType::log_data` | 3 | ltb·LDG |
| `0x0063fa60` | fn | `WallData::armor` | 4 | LDG |
| `0x00644130` | fn | `ObjectData::get_damage` | 24 | AUD·RDM·cbt·dmg·pdt·LDG |
| `0x00644178` | fn | `ObjectData::get_damage` +72 | 5 | cbt·dmg·ltb·LDG |
| `0x0064418e` | fn | `ObjectData::get_damage` +94 | 5 | cbt·dmg·ltb·LDG |
| `0x006442a3` | fn | `ObjectData::get_damage` +371 | 2 | dmg |
| `0x006448b9` | fn | `ObjectData::get_damage` +1929 | 2 | cbt·dmg |
| `0x00644b26` | fn | `ObjectData::get_damage` +2550 | 2 | cbt·LDG |
| `0x00644b2b` | fn | `ObjectData::get_damage` +2555 | 4 | dmg·LDG |
| `0x00644d78` | fn | `ObjectData::get_damage` +3144 | 2 | cbt·dmg |
| `0x00644e0e` | fn | `ObjectData::get_damage` +3294 | 6 | dmg·LDG |
| `0x00644e2d` | fn | `ObjectData::get_damage` +3325 | 2 | dmg·LDG |
| `0x00645100` | fn | `ObjectsArray::get_new_data` +48 | 4 | AUD·cbt |
| `0x006469f0` | fn | `ObjectData::attack` | 6 | cbt·dmg·LDG |
| `0x00647830` | fn | `Object::walk_data` | 2 | chk |
| `0x00647db0` | fn | `ObjectData::armor` | 5 | cbt·dmg·LDG |
| `0x00647e6f` | fn | `ObjectData::armor` +191 | 2 | cbt·dmg |
| `0x0064a480` | fn | `Object::do_damage` | 4 | cbt·LDG |
| `0x0064a4f7` | fn | `Object::do_damage` +119 | 3 | cbt·LDG |
| `0x0064e5c0` | fn | `Object::compare_target` | 2 | cbt·LDG |
| `0x006508c0` | fn | `ObjectData::train_time` | 5 | eco·LDG·sec |
| `0x006508eb` | fn | `ObjectData::train_time` +43 | 2 | eco·sec |
| `0x00650951` | fn | `ObjectData::train_time` +145 | 2 | eco |
| `0x00650ab5` | fn | `ObjectData::train_time` +501 | 3 | LDG·sec |
| `0x00653790` | fn | `ObjectData::is` | 2 | dmg |
| `0x0065f880` | fn | `ObjectType::ObjectType` | 2 | cbt |
| `0x0065fc00` | fn | `ObjectType::log_data` | 19 | CHT·bgt·chk·cbt·ltb·pth·pdt·LDG |
| `0x006621d0` | fn | `SubObject::walk_data` | 2 | chk |
| `0x00662300` | fn | `SubObject::init` | 3 | pth |
| `0x00662680` | fn | `SubObject::set_new_location` | 5 | pth·LDG |
| `0x006631c0` | fn | `Type::log_data` | 4 | ltb·LDG |
| `0x00664090` | fn | `TypeData::get_cost` | 7 | eco·LDG·sec |
| `0x00665452` | fn | `TypeData::get_cost` +5058 | 2 | LDG·sec |
| `0x006656aa` | fn | `TypeData::get_cost` +5658 | 2 | LDG·sec |
| `0x0066d630` | fn | `TechType::log_data` | 3 | ltb·LDG |
| `0x0066f2c0` | fn | `GoodType::log_data` | 3 | ltb·pdt |
| `0x0067bad6` | fn | `Ammo::init_crash` +726 | 3 | AUD·eco·LDG |
| `0x00681db0` | fn | `init_coord_lookup_array` | 6 | AUD·pth·LDG |
| `0x00685990` | fn | `PathFinder::astar_caravan_road` | 11 | AUD·RDM·pth·LDG |
| `0x00686300` | fn | `PathFinder::calc_road_cost` | 8 | AUD·pth·LDG |
| `0x00686341` | fn | `PathFinder::calc_road_cost` +65 | 3 | pth·LDG |
| `0x00687970` | fn | `PathFinder::first_open_node` | 2 | pth |
| `0x00687e40` | fn | `PathFinderOut::registration` | 3 | pth |
| `0x00688150` | fn | `PathFinder::clear` | 2 | pth |
| `0x00688360` | fn | `PathFinderOut::dbg_tree_depth` | 2 | pth |
| `0x00688740` | fn | `PathFinderData::valid_roadcoord` | 3 | pth·LDG |
| `0x00688770` | fn | `PathFinderData::valid_roadcoord` +48 | 2 | pth |
| `0x00688a40` | fn | `PathFinder::find_road` | 4 | pth·LDG |
| `0x00688fc0` | fn | `PathFinder::find_wpath` | 5 | pth·LDG |
| `0x006897d0` | fn | `PathFinder::find_tpath` | 5 | pth·LDG |
| `0x00689ec0` | fn | `PathFinder::init` | 5 | pth·LDG |
| `0x00689f29` | fn | `PathFinder::init` +105 | 2 | pth |
| `0x00689ff2` | fn | `PathFinder::init` +306 | 2 | pth |
| `0x0068a030` | fn | `PathFinder::PathFinder` | 2 | pth |
| `0x0068bc90` | fn | `Map::make` | 6 | pth·LDG·rng |
| `0x006b10e2` | fn | `World::compute_reg_territory` +1330 | 2 | eco·LDG |
| `0x006b1480` | fn | `World::compute_reg_territory` +2256 | 2 | eco·LDG |
| `0x006b53f0` | fn | `WorldData::was_seen` | 3 | pth |
| `0x006b5cf0` | fn | `World::walk_data` | 2 | chk·rck |
| `0x006cdd20` | fn | `Leader::calc_anti_attrition` +96 | 2 | eco·sec |
| `0x006ce280` | fn | `Leader::gather` | 4 | LDG·sec |
| `0x006ce450` | fn | `Leader::do_gather` | 8 | eco·LDG·sec |
| `0x006ce4c8` | fn | `Leader::do_gather` +120 | 2 | sec |
| `0x006ce4e7` | fn | `Leader::do_gather` +151 | 2 | sec |
| `0x006ce512` | fn | `Leader::do_gather` +194 | 3 | sec |
| `0x006ce62f` | fn | `Leader::do_gather` +479 | 2 | LDG·sec |
| `0x006ce643` | fn | `Leader::do_gather` +499 | 2 | LDG·sec |
| `0x006ce6dc` | fn | `Leader::do_gather` +652 | 2 | eco·sec |
| `0x006ce706` | fn | `Leader::do_gather` +694 | 3 | eco·sec |
| `0x006ce72c` | fn | `Leader::do_gather` +732 | 2 | eco·sec |
| `0x006ce755` | fn | `Leader::do_gather` +773 | 2 | eco·sec |
| `0x006ce7ae` | fn | `Leader::do_gather` +862 | 4 | eco·LDG·sec |
| `0x006ce900` | fn | `Leader::calc_resource_caps` | 5 | eco·LDG·sec |
| `0x006ce92c` | fn | `Leader::calc_resource_caps` +44 | 4 | eco·LDG·sec |
| `0x006ceee0` | fn | `Leader::calc_gather` | 2 | LDG·sec |
| `0x006d66a0` | fn | `LeaderData::get_gather_handicap` | 4 | eco·LDG·sec |
| `0x006d6750` | fn | `LeaderData::walk_data` | 5 | bgt·chk·LDG·rck |
| `0x006da000` | fn | `LeaderData::get_target` | 2 | dmg |
| `0x006db810` | fn | `LeaderData::has_preq` | 2 | sec |
| `0x006e1370` | fn | `LeaderData::has_tribe_bonus` | 9 | dmg·LDG·sec |
| `0x006e33a0` | fn | `LeaderData::type_avail` | 2 | LDG·sec |
| `0x006ed2a0` | fn | `Leaders::process_all` | 3 | LDG·sec |
| `0x006edb50` | fn | `LeaderData::is_ally` | 2 | pth |
| `0x00822d6d` | fn | `IFaceSelected::draw_stat` +6301 | 3 | eco·LDG·sec |
| `0x00846450` | fn | `Doober::get_num` | 10 | AUD·GOL·RDM·LDG·rng |
| `0x008544a0` | fn | `TerrainOut::find_tcoord_z` | 6 | pth |
| `0x00883080` | fn | `BorderSpline::registration` +688 | 2 | eco·LDG |
| `0x0092cfe0` | fn | `flanking` | 9 | AUD·cbt·dmg·LDG |
| `0x00936560` | fn | `CheckSums::check_all` | 15 | AUR·RDM·chk·pdt·LDG·rck·rio·rng |
| `0x00936bb0` | fn | `CheckSums::check_deaths` | 2 | chk·rck |
| `0x00936ff0` | fn | `CheckSum::walk_function` | 7 | AUD·chk·LDG |
| `0x00937040` | fn | `CheckSums::check_wdata` | 2 | chk |
| `0x009370a0` | fn | `CheckSums::check_tdata` | 2 | chk |
| `0x009370f0` | fn | `CheckSums::check_seen` | 2 | chk |
| `0x009371d0` | fn | `CheckSums::check_units` | 2 | chk·rck |
| `0x00937290` | fn | `CheckSums::check_builds` | 2 | chk·rck |
| `0x00937360` | fn | `CheckSums::check_walls` | 2 | chk·rck |
| `0x00937430` | fn | `CheckSums::check_guys` | 2 | chk·rck |
| `0x009374e0` | fn | `CheckSums::check_ammo` | 2 | chk·rck |
| `0x00937530` | fn | `CheckSums::check_groups` | 2 | chk·rck |
| `0x00937600` | fn | `CheckSums::check_cities` | 2 | chk·rck |
| `0x00937710` | fn | `CheckSums::check_goods` | 2 | chk·rck |
| `0x00937790` | fn | `CheckSums::check_items` | 2 | chk·rck |
| `0x0093ef10` | fn | `CommandManager::process_turn` | 4 | chk·rck·rio |
| `0x0093f2e9` | fn | `CommandManager::process_turn` +985 | 3 | AUR·LDG·rck |
| `0x00940770` | fn | `CommandManager::issue_check_sums` | 10 | AUR·LDG·rck |
| `0x009409f8` | fn | `CommandManager::issue_check_sums` +648 | 3 | LDG·rck |
| `0x00943730` | fn | `CommandPackage::process_player_speed` | 3 | rio·rst |
| `0x00943b00` | fn | `CommandPackage::process_camera` | 3 | rst |
| `0x00943d20` | fn | `CommandPackage::process_turn_data` | 2 | rck·rst |
| `0x00945140` | fn | `CommandPackage::process_spline` | 2 | rio·rst |
| `0x009454f0` | fn | `CommandPackage::process_chat` | 2 | rio·rst |
| `0x009459d0` | fn | `CommandPackage::process_check_sums` | 15 | AUR·RDM·chk·pdt·rck·rio·rst |
| `0x00945e0e` | fn | `CommandPackage::process_check_sums` +1086 | 2 | LDG |
| `0x009495c0` | fn | `CommandPackage::process_move_near` | 2 | rio·rst |
| `0x00949ed0` | fn | `CommandPackage::process_stance` | 2 | rio·rst |
| `0x0094a0c0` | fn | `CommandPackage::process_group` | 2 | rio·rst |
| `0x0094a700` | fn | `CommandPackage::process` | 15 | LDG·rck·rfm·rio·rst |
| `0x0094b7c0` | fn | `CommandPackage::log_data` | 4 | AUR·LDG·rio |
| `0x0094c500` | fn | `CommandPackage::process_all` | 12 | AUR·LDG·rck·rst |
| `0x0094c5a2` | fn | `CommandPackage::process_all` +162 | 3 | AUR·LDG |
| `0x0094c5e0` | fn | `CommandPackage::process_all` +224 | 2 | AUR |
| `0x0094c6a4` | fn | `CommandPackage::process_all` +420 | 2 | AUR |
| `0x00952990` | fn | `RecordGame::write_ctw_header` | 2 | rio |
| `0x00952a50` | fn | `RecordGame::write_header` | 3 | rio |
| `0x00952b40` | fn | `RecordGame::finalize` | 6 | LDG·rio |
| `0x00952d90` | fn | `RecordGame::read_package` | 8 | AUR·LDG·rio |
| `0x00952fb0` | fn | `RecordGame::write_package` | 11 | AUR·LDG·rio |
| `0x00952fb3` | fn | `RecordGame::write_package` +3 | 2 | AUR·LDG |
| `0x009533e0` | fn | `RecordGame::read_random_game_info` | 2 | rio |
| `0x009534e0` | fn | `RecordGame::write_random_game_info` | 2 | rio |
| `0x00953610` | fn | `RecordGame::read_rules` | 2 | rio |
| `0x00953630` | fn | `RecordGame::write_rules` | 2 | rio |
| `0x00953710` | fn | `RecordGame::read_header` | 2 | rio |
| `0x009537a0` | fn | `RecordGame::init_record` | 3 | rio |
| `0x00953990` | fn | `RecordGame::close` | 3 | rio |
| `0x00960000` | fn | `GameSpy::check_steam_command_line` +96 | 4 | ltb·LDG |
| `0x00997ad0` | fn | `ScenarioData::walk_data` | 2 | chk·rck |
| `0x009c41a0` | fn | `RunTimeEnv::walk_data` | 2 | chk·rck |
| `0x009e1890` | fn | `MathUtilFuncSet::rand_int` | 6 | AUD·LDG·rng |
| `0x00a15fc0` | fn | `String::convert_int` | 4 | eco·LDG |
| `0x00a1b2d0` | fn | `String::walk_data` | 2 | rio |
| `0x00a1b6b0` | fn | `String::generate_hash` | 2 | rio |
| `0x00a1d110` | fn | `String::fraction` | 17 | AUD·GOL·RDM·eco·pdt·LDG·sec |
| `0x00a1d210` | fn | `String::number` | 4 | eco·LDG |
| `0x00a2e880` | fn | `SyncLogger::reportSettingsAndOptions` | 3 | chk·LDG |
| `0x00a2f580` | fn | `SyncLogger::writeToFileAndReset` | 3 | chk |
| `0x00a30080` | fn | `SyncLogger::setupWithConfigSettings` | 7 | chk·LDG·rck |
| `0x00a39cf0` | fn | `Random::get` | 16 | AUD·RDM·pdt·LDG·rio·rng |
| `0x00a39d30` | fn | `Random::reseed` | 4 | RDM·LDG·rng |
| `0x00a39d40` | fn | `random` | 3 | RDM·LDG·rng |
| `0x00a39d70` | fn | `Random::get` | 19 | AUR·AUD·RDM·pth·pdt·LDG·rst·rng |
| `0x00a39ea5` | fn | `Random::get` +309 | 3 | AUR·rng |
| `0x00a46830` | fn | `adler32` | 13 | AUD·RDM·chk·pdt·LDG |
| `0x00ab4dd0` | fn | ``dynamic atexit destructor for 'game_random''` | 2 | rng |
| `0x00ac5434` | data | `__imp__wcschr` | 2 | eco·LDG |
| `0x00ac54ac` | data | `__imp___wtoi` | 3 | eco·LDG·sec |
| `0x00b2bcd8` | data | `const DataWalk::`vftable'` | 6 | AUD·chk·pdt·rio |
| `0x00b30c88` | data | `const LoadGame::`vftable'` | 3 | chk·pdt·rio |
| `0x00b35ac4` | data | `const SaveGame::`vftable'` | 3 | chk·pdt·rio |
| `0x00b3f920` | data | `const CheckSum::`vftable'` | 5 | AUD·chk·pdt |
| `0x00b41fd4` | data | `const UnitType::`vftable'{for `Type'}` | 2 | ltb |
| `0x00b69a30` | data | `__xmm@0000021c000000640000006400000037` | 6 | pth·LDG |
| `0x00b69a60` | data | `__xmm@000002580000003c000000c8000000f0` | 6 | pth·LDG |
| `0x00c06000` | data | `_m_rgDLLMap` | 2 | ltb·rio |
| `0x00c06184` | data | `public: static class Random &GameAccess::game_random` | 19 | RDM·pth·pdt·LDG·rio·rng |
| `0x00c06188` | data | `public: static class World &GameAccess::world` | 7 | chk·pth·rck·rio·rng |
| `0x00c0618c` | data | `public: static class Objects &GameAccess::objects` | 6 | chk·LDG·rck |
| `0x00c061d0` | data | `public: static class World const &GameAccessConst::worldc` | 3 | dmg·pth |
| `0x00c061e0` | data | `public: static class Leaders const &GameAccessConst::leadersc` | 3 | cbt·dmg·eco |
| `0x00c061e4` | data | `public: static class Constants const &GameAccessConst::constantsc` | 11 | AUD·RDM·cbt·dmg·eco·pdt·LDG·rck |
| `0x00c061e8` | data | `public: static class Game const &GameAccessConst::gamec` | 3 | cbt·dmg |
| `0x00c061ec` | data | `public: static class Game &GameAccess::game` | 21 | AUR·chk·cbt·eco·LDG·rck·rst·rng·sec |
| `0x00c061f0` | data | `public: static class Constants &GameAccess::constants` | 14 | AUD·RDM·cbt·eco·pdt·LDG |
| `0x00c06370` | data | `public: static int String::output_char_buffer_length` | 3 | chk·LDG·rck |
| `0x00c06378` | data | `class StringTable *int_str_array` | 6 | RDM·bgt·eco·LDG |
| `0x00c06664` | data | `wchar_t const *LOBBY_DATA_KEY_DESYNC_UPLOADS_WANTED` | 3 | chk·rck |
| `0x00c06674` | data | `wchar_t const *LOBBY_DATA_KEY_DESYNC_DIAGNOSTICS_CHECK_DESYNCS_EVERY_X_FRAMES` | 2 | chk·rck |
| `0x00c06afc` | data≈ | `public: static int IncrementalLoad::read_increment` +584 | 18 | AUD·RDM·cbt·dmg·ltb·pdt·LDG·rst |
| `0x00c096e4` | data | `class PtrArray<class GoodType> goodtypes` | 3 | ltb·LDG |
| `0x00c0a264` | data | `class PtrArray<class UnitType> unittypes` | 3 | ltb·LDG |
| `0x00c0aa90` | data | `class PtrArray<class BuildType> buildtypes` | 3 | ltb·LDG |
| `0x00c0aaac` | data | `class PtrArray<class TechType> techtypes` | 2 | ltb·LDG |
| `0x00c0aac8` | data | `class PtrArray<class ItemType> itemtypes` | 2 | ltb·LDG |
| `0x00c0aae4` | data | `class PtrArray<class ObjectType> objecttypes` | 2 | ltb·LDG |
| `0x00c0ab00` | data | `class PtrArray<class SpellType> spelltypes` | 2 | ltb·LDG |
| `0x00c0ab1c` | data | `class PtrArray<class BonusType> bonustypes` | 2 | ltb·LDG |
| `0x00c0ab38` | data | `class PtrArray<class GovType> govtypes` | 2 | ltb·LDG |
| `0x00c0ab54` | data | `class PtrArray<struct TypeBak> typesbak` | 2 | ltb·LDG |
| `0x00c0ab84` | data | `class Objects objects` +20 | 5 | cbt·dmg·LDG |
| `0x00c0aec0` | data | `class Units units` +16 | 6 | cbt·dmg·LDG |
| `0x00c12bf4` | data | `class Balance combat_table` +4 | 11 | RDM·ltb·LDG |
| `0x00c8da70` | data | `public: static class Stack<class PathNode *> Recycler<class PathNode>::temp_pool` | 2 | pth |
| `0x00c9425c` | data | `class ObjectArray<struct RandomLogEntry> `RTTI Type Descriptor'` +8 | 2 | chk |
| `0x00c952e0` | data | `struct DataWalk `RTTI Type Descriptor'` | 3 | chk |
| `0x00c96814` | data | `struct CheckSum `RTTI Type Descriptor'` | 2 | chk |
| `0x00cae5fc` | data | `int *div_3_table` | 7 | dmg·pth·LDG |
| `0x00cbee90` | data | `public: static unsigned long *CommandPackage::checksums` | 6 | rck·rst |
| `0x00e37a8c` | data | `class Random game_random` | 15 | RDM·LDG·rio·rng |
| `0x00e3a2a0` | data | `class Caravans caravans` +16 | 3 | pth |
| `0x00e3a390` | data | `class Leaders leaders` | 2 | chk |
| `0x00e85ddc` | data | `class Types types` +20 | 2 | dmg·ltb |
| `0x00e85e40` | data | `class PathFinder pathfinder` | 3 | pth |
| `0x00e85e80` | data≈ | `class PathFinder pathfinder` +64 | 3 | pth |
| `0x00e85ee0` | data≈ | `class PathFinder pathfinder` +160 | 4 | pth |
| `0x00e85ee4` | data≈ | `class PathFinder pathfinder` +164 | 2 | pth |
| `0x00e85ee8` | data≈ | `class PathFinder pathfinder` +168 | 2 | pth |
| `0x00e85eec` | data≈ | `class PathFinder pathfinder` +172 | 2 | pth |
| `0x00e85ef0` | data≈ | `class PathFinder pathfinder` +176 | 2 | pth |
| `0x00e85ef4` | data≈ | `class PathFinder pathfinder` +180 | 2 | pth |
| `0x00e85ef8` | data≈ | `class PathFinder pathfinder` +184 | 2 | pth |
| `0x00e85efc` | data≈ | `class PathFinder pathfinder` +188 | 3 | pth |
| `0x00e85f00` | data≈ | `class PathFinder pathfinder` +192 | 2 | pth |
| `0x00e85f0c` | data | `public: static class Random SoundGlobal::random` | 4 | rng |
| `0x00e87d3c` | data | `public: static class Random SoundType::random` | 3 | rng |
| `0x00e8f7c0` | data | `struct RecordGame record_game` +24 | 4 | rio |
| `0x00e8f7e8` | data≈ | `struct RecordGame record_game` +64 | 3 | rio |
| `0x00e8f80c` | data≈ | `struct RecordGame record_game` +100 | 2 | rio |
| `0x00eb697c` | data | `class Random internal_random` | 11 | RDM·pdt·LDG·rio·rng |
| `0x00ecba54` | data≈ | `class JukeBox juke_box` +68 | 2 | rng |
| `0x00ee13a8` | data≈ | `void **`class CSteamGameServerAPIContext & __cdecl SteamGameServerInternal_ModuleContext(void)'::`2'::ctx` +36 | 2 | rng |
| `0x00ee1aec` | data≈ | `double `public: static double __cdecl ProfileLog::get_frequency(void)'::`2'::ffreq` +1852 | 2 | rng |
