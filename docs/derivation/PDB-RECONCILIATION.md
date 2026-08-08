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

<!--SCORECARD-->

### 1c. The uncomfortable headline

**The single largest error was systematic, not random: we identified an entire family of
*loggers* as *loaders*, and it cost roughly a day.** Details in §2. The consolation — and it
is a real one — is that the artifact built on the mistake survives it almost intact (§2.3),
and that when the same methodology was pointed at code it read *behaviourally* rather than
by string-chasing, it was right at a rate that is genuinely hard to fault (§3).

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
cleaner source than the loader, which reaches its names indirectly. Checked, not assumed:

`Constants::init` was disassembled end to end and every call to `Constants::get_item(const
String&)` `0x0057FA60` (plain `_wtoi`) and `Constants::get_fraction(const String&, int
scale)` `0x0057F950` was recovered with its `push imm32` scale and its `mov [esi+disp], eax`
destination.

| | |
|---|---|
| call sites in `Constants::init` | **701** — 661 `get_item`, 40 `get_fraction` |
| scale immediates, complete universe | **256 ×24 · 192 ×11 · 100 ×5** |
| distinct scalar destination offsets recovered | 687 |
| our `docs/derivation/rules-constants.json` | 719 entries (688 scalar + 31 array-valued) |
| **entries agreeing exactly on (offset, parser, scale)** | **686** |
| entries disagreeing on parser or scale | **0** |

**686 exact agreements, zero contradictions.** The 33 of ours without a recovered loader slot
are 31 array-valued constants (`starting_goods`, `basic_gather`, `city_gather`, `pop_cap`, …
stored through indexed addressing this extractor does not follow) and 2 already flagged
`parser: None`. Three loader slots (`+0x534`, `+0x548`, `+0x680`) have no entry in ours and
are a genuine gap. **The scale set {192, 256, 100} our lanes derived is not merely correct,
it is exhaustive** — there is no fourth scale in the binary.

So: the function was misidentified, the *artifact* is sound, and we now have a second,
independent extractor keyed on the real loader. Both should be kept; they cross-check.

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

Everything measured about `astar_caravan_road` remains true **of that search**. The headline
"Rise of Nations' pathfinder is a pure-integer 8-connected grid A\*" is one entry point of
three, and the one units actually move on is the unread one. Structural bonus from the
symbol table: the open list is a `BRTree<PathNode*, unsigned long>` behind a `Recycler` pool
(`0x0047A160`, `0x0047A2C0`, `0x0047A370`) — not the flat array a reimplementation would
reach for by default.

<!--WRONGFN-EXTRA-->

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

Exactly **five** `Random`-family globals exist, enumerated by their dynamic initialisers —
which is a stronger statement than the old "≥7 streams" and supersedes it as a count of
*objects* (the old figure counted call-site clusters).

**Consequence, and it is the opposite of what the old claim implied:** the pathfinder draws
from the *main* stream, so a divergence in how many edges we relax desynchronises every later
draw on that stream — scripts included. The pathfinding lane's determinism warning stands in
full; it was the ledger's mitigation of it that was wrong.

Two further facts the PDB volunteers: `CommandPackage::process_check_random(struct
CheckRandomCommand*)` `0x00946020` and `static unsigned long* CommandPackage::random_seeds`
exist, so **RNG state is carried in the lockstep protocol**; and `RandomLogEntry` +
`ObjectArray<RandomLogEntry>` exist, so the engine can log individual draws — a ready-made
desync-debugging surface nobody has looked at.

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

<!--OPEN-EXTRA-->

---

## 7. Prioritised list — derivations that must be REDONE

Ordered by blast radius. "Redo" means *the derivation described the wrong thing*; work that
is merely renamed is not on this list.

<!--REDO-->

---

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
<!--APPENDIX-->
