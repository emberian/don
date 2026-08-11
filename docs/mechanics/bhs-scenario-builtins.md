# `ScenarioFuncSet`: the first twenty-one, and the shipped script they make run

*Written 2026-08-11 by the `bhs-builtins` lane. Tier C throughout — static analysis of
`ron-bin/riseofnations.exe` (PE32 i386, image base `0x00400000`) with Capstone, plus the
matching private PDB. Nothing here has been executed against retail. Not verified, not
proven, not a refinement.*

## 1. What was closed, stated exactly

`schema/bhs-builtins.json` records 873 registrations. `ScenarioFuncSet` owns 842 of them
(`OP_CALL_GAME` indices 31..872).

**What was already there.** `crates/don-bhs` implemented none of them —
`crates/don-bhs/src/builtins.rs` covers the four *utility* function sets and stops at the
simulation seam. But `crates/don-sim/src/script_runtime.rs` already answers **81**
`ScenarioFuncSet` indices from a private `SimScriptHost`, including its own private
`ScriptTimers` and the ten `is_victory_*` gates. That code is not reachable from `don-bhs`
or `don-bhs-cc` — `don-sim` depends on them, not the other way round — which is exactly
why the shipped-script load path could load `general_powers.bhs` and not run it.

**What this lane added.** `crates/don-bhs/src/scenario.rs` implements **21**
`ScenarioFuncSet` entries, taking `don-bhs` from 25 implemented builtins to 46. Of the 21,
**seven are new to the tree** — `get_is_no_nation_powers` (94), `get_rush_rules` (95),
`is_conquest_scenario` (147), `find_unit` (311), `object_type_selected` (390),
`num_objects_selected` (392), `bubble_text_obj` (783) — and fourteen (77, 78, 79, 96–105,
298) are a second, independently derived implementation of entries `don-sim` already had
privately. Tree-wide `ScenarioFuncSet` coverage goes **81 → 88**.

The duplication is real and should be collapsed; see §10. It is written down here rather
than glossed because "21 new builtins" would be the wrong number.

The cohort is not a scatter across the index space — it is the exact closure of one shipped
script plus its cheapest neighbours:

| index | name | handler VA | size | tier |
|---|---|---|---|---|
| 77 | `set_timer` | `0x009e4bc0` | 65 | executes here in full |
| 78 | `stop_timer` | `0x009e4c10` | 104 | executes here in full |
| 79 | `timer_expired` | `0x009e4c80` | 37 | executes here in full |
| 94 | `get_is_no_nation_powers` | `0x009e5230` | 16 | one `Game` byte |
| 95 | `get_rush_rules` | `0x009e5240` | 10 | one `Game` byte |
| 96 | `is_victory_standard` | `0x009e5250` | 15 | one `Game` byte |
| 97 | `is_victory_conquest` | `0x009e5260` | 16 | one `Game` byte |
| 98 | `is_victory_economic` | `0x009e5270` | 16 | one `Game` byte |
| 99 | `is_victory_musical_chairs` | `0x009e5280` | 16 | one `Game` byte |
| 100 | `is_victory_score` | `0x009e5290` | 16 | one `Game` byte |
| 101 | `is_victory_sudden_death` | `0x009e52a0` | 16 | one `Game` byte |
| 102 | `is_victory_tech_race` | `0x009e52b0` | 16 | one `Game` byte |
| 103 | `is_victory_territory` | `0x009e52c0` | 16 | one `Game` byte |
| 104 | `is_victory_time_limit` | `0x009e52d0` | 16 | one `Game` byte |
| 105 | `is_victory_wonder` | `0x009e52e0` | 16 | one `Game` byte |
| 147 | `is_conquest_scenario` | `0x009e6040` | 18 | one semaphore bit |
| 298 | `time_sec` | `0x009ead40` | 12 | one `Game` word |
| 311 | `find_unit` | `0x009ebe10` | 206 | guards + the `0x009e2850` scan, both here |
| 390 | `object_type_selected` | `0x009f04e0` | 415 | guards + the loop, both here |
| 392 | `num_objects_selected` | `0x009f06f0` | 107 | guards + `0x0070f960`, both here |
| 783 | `bubble_text_obj` | `0x009ff550` | 105 | guards + `0x009e32a0`, both here |

**What a shipped script can now do that it could not.**
`ron-data/bhs-corpus/scenario/scriptlibrary/general_powers.bhs` — the second script retail
runs at tick step 4 — calls exactly seven builtins: `time_sec`, `set_timer`,
`timer_expired`, `object_type_selected`, `find_unit`, `length` and `bubble_text_obj`.
`length` was already implemented in `don-bhs`; three (`find_unit`, `object_type_selected`,
`bubble_text_obj`) existed **nowhere in the tree**, and three (`time_sec`, `set_timer`,
`timer_expired`) existed only inside `don-sim`, which the load path cannot reach. All six
are now in `don-bhs`, so the script runs frame by frame to its last statement instead of
stopping on its first `OP_CALL_GAME`.
`crates/don-bhs-cc/tests/general_powers_runtime.rs` drives it for sixteen game seconds and
asserts the arming, the six re-arms, the `find_unit` hit and the bubble.

The other ten entries are the rules gates `defensive.bhs` and `economic.bhs` branch the
opening book on (`get_rush_rules`, the ten `is_victory_*`, `is_conquest_scenario`,
`get_is_no_nation_powers`), which are single-field `cmp`s and cost nothing once the field
is named.

## 2. `ScenarioData::timers` — the one subsystem that is entirely ours

`ScriptTimers` is script-engine state, not simulation state, so it is reimplemented in
full rather than delegated.

The object is `ScenarioData::timers` at `0x00ed6650`, a
`LinkListBase<String,int,LLNode<String,int>>`. Its methods address their own fields as
absolute globals, which gives the layout directly:

```text
ScriptTimers   +0x00  String  current_key     0x00ed6650   (a 20-byte String mirror)
               +0x14  int     current_value   0x00ed6664
               +0x18  LLNode* current         0x00ed6668
               +0x1c  int     count           0x00ed666c
               +0x20  LLNode* head            0x00ed6670

LLNode         +0x00  next        +0x04  prev        +0x08  String key   +0x1c  int value
```

The list is **circular and doubly linked**, and `LinkListBase::ordered_insert`
`0x004d2120` keeps it **ascending by value**, so `head` is always the soonest timer.

### The three handlers

`ScenarioFuncSet::set_timer(timer_id, seconds)` `0x009e4bc0`:

```text
0x009e4bcd  msg = *timer_id                      // the global String at 0x00ed6730
0x009e4bd9  if (seconds <= 0)          return -1
0x009e4be0  if (timer_id->curr_len==0) return -1
0x009e4be2  eax = Game::tick (Game+0x560)
0x009e4bee  eax += seconds                       // a plain `add`: this wraps
0x009e4bf4  jmp ScriptTimers::add_timer
```

`stop_timer` `0x009e4c10` is `remove(msg) ? 1 : -1` — and note it has **no empty-name
guard**, unlike `set_timer`, so `stop_timer("")` really does look for a timer named `""`.

`timer_expired` `0x009e4c80` is `check(msg, Game::tick)`.

Every one of the three copies its argument into the global `String msg` at `0x00ed6730`
first, and `add_timer` / `check` then rebuild their key from *that global* and ignore the
argument they were passed. The key travels by global, not by parameter.

### The container

`ScriptTimers::add_timer` `0x00a049e0`:

```text
if (count >= 0x64) { Error::report(...); return -1; }   // 0x00a049fb
if (seek(msg)) remove_current();                        // 0x00a04a25..0x00a04a2e
ordered_insert(msg, expiry);                            // 0x00a04a56
return 1;
```

So a hundred live timers is a hard cap, the hundred-and-first is an `Error::report`
`0x00a2e550` rather than a silent drop, **and the cap is checked before the seek** — so
re-arming an existing timer at the cap is also refused.

`ScriptTimers::check` `0x00a04b80`:

```text
if (!seek(msg)) return -1;
if (now >= current_value) { remove_current(); return 1; }
return 0;
```

`-1` / `0` / `1` are three different answers and a script can tell them apart. A due timer
is **consumed**, which is exactly why `general_powers.bhs` re-arms inside the
`if (timer_expired("pop"))` body.

`seek` `0x004d2340` on a miss returns 0 and **leaves `current` where it was** — it does not
clear the cursor. `remove_current` `0x004c9060` advances `current` to the successor and
wraps to `head`; a one-element list clears the whole container, cursor included.
`ordered_insert` inserts *before* the first node whose value is `>=` the new one, so equal
expiries put the newest first; a value greater than every node inserts before `head` (the
tail of the circular list) without moving `head`.

## 3. `String::operator==` is case-insensitive, and has a hole

`String::operator==` `0x00a1f140` is what compares every timer key and every type name.
Read at instruction level:

1. `curr_len` (`String+8`, a `short` count of UTF-16 code units) must match, else false;
2. if **both** operands carry generated hashes (`String+0xa & 2`, set by
   `String::generate_hashes` `0x00a1ee60`), it returns `hash2 == hash2` (`String+0x10`)
   **and nothing else** — a hash collision reads as equal;
3. otherwise it forwards to the CRT `_wcsicmp` (import `0x00ac5640`), i.e. a
   **case-insensitive** compare.

Point 3 is the headline: BHS string comparison is case-insensitive, so
`set_timer("POP", 2)` re-arms the timer `set_timer("pop", 5)` created.

**Deliberately not written.** Step 2 is reproducible in principle — `String::generate_hash`
`0x00a1b6b0` is 240 bytes and reads

```text
h1 = h2 = 0;  n = wcslen(p);  cnt = n
for (i = n-1; i >= 0; i--, cnt--):
    c  = p[i];  lc = towlower(c);  T = table[i % 50]        // table at 0x00b14500
    h1 += T*c  + table[c  % 50]*cnt
    h2 += T*lc + table[lc % 50]*cnt
return h1        // and *out = h2
```

— but reproducing the *predicate* also needs a model of **which** `String`s carry hashes,
and `don_bhs::Value::Str` is an `Rc<String>` with no flags word, so the crate cannot
represent that state at all. `String::operator=` `0x00a1eeb0` propagates `+0xc`/`+0x10`
and the flags byte at `+0xb` on assignment, so hash-carrying strings do reach the timer
map in retail. This is recorded as a boundary, not approximated: `scenario::string_eq`
implements the `_wcsicmp` predicate and, for a pair that is neither byte-identical nor
wholly ASCII, **poisons** rather than guessing MSVCRT's `towlower` outside the C locale.

## 4. The `Game` and `GameInfo` scalars

`GameInfo` is `Game+0x0C` (PDB `Game::info`), so the byte offsets in the handlers resolve:

| handler reads | is | builtin |
|---|---|---|
| `Game+0x20` bit 2 | `GameInfo::flags` (`+0x14`) bit 2 | `get_is_no_nation_powers` |
| `Game+0x32` | `GameInfo::rush_rules` (`+0x26`), a `RushRulesIndex` | `get_rush_rules` |
| `Game+0x38` | `GameInfo::victory` (`+0x2c`), a `VictoryIndex` | the ten `is_victory_*` |
| `Game+0x560` | `Game::tick`, **game seconds** | `time_sec`, `set_timer` |
| `Game+0x822` bit 1 | `Game::semaphore` (`+0x814`) bit **17** | `is_conquest_scenario` |
| `Game+0x820` bit 2 | `Game::semaphore` bit **2** | the two selection builtins |

The semaphore rows corroborate op-life's standing finding that `Game::semaphore` is a
`BitMask<256>` at `Game+0x814` whose bit bytes start at `Game+0x820`: byte 2 bit 1 is bit
17, byte 0 bit 2 is bit 2.

### FINDING — `is_victory_territory` is misnamed, and there is no territory mode

The ten `is_victory_*` handlers are `cmp byte ptr [Game+0x38], K / sete al`. Lining the
`K`s up against the PDB's `VictoryIndex` (`schema/types.json`):

| builtin | K | `VictoryIndex` |
|---|---|---|
| `is_victory_standard` | 0 | `VICTORY_STANDARD` |
| `is_victory_sudden_death` | 1 | `VICTORY_SUDDEN_DEATH` |
| `is_victory_conquest` | 2 | `VICTORY_CONQUEST` |
| `is_victory_score` | 3 | `VICTORY_SCORE` |
| `is_victory_time_limit` | 4 | `VICTORY_TIME_LIMIT` |
| `is_victory_musical_chairs` | 5 | `VICTORY_MUSICAL_CHAIRS` |
| `is_victory_wonder` | 6 | `VICTORY_WONDER` |
| **`is_victory_territory`** | **7** | **`VICTORY_POPULATION`** |
| `is_victory_economic` | 8 | `VICTORY_ECONOMIC` |
| `is_victory_tech_race` | 9 | `VICTORY_TECH_RACE` |

`VictoryIndex` has **no** `VICTORY_TERRITORY` enumerator. Builtin 103 selects the
population mode. `VictoryTypeIndex` — a *different* enum — does have
`VICTORY_BY_TERRITORY = 2`, which is presumably where the name came from and is not what
this byte holds. Anyone reading the builtin's name to decide which mode it gates gets the
wrong mode. Also: `VICTORY_SCENARIO` (10) has no builtin at all, so under a scenario
victory condition all ten gates answer 0.

## 5. `who` is 1-based everywhere, and the bound is unsigned

Every scenario builtin that takes a `who` begins with `dec` and then an **unsigned**
`cmp .., 7 / ja`:

- `find_unit` `0x009ebe22` / `0x009ebe75`
- `object_type_selected` `0x009f04f3` / `0x009f054d`
- `num_objects_selected` `0x009f06f8`
- `bubble_text_obj` `0x009ff55c` (through `valid_object_o` `0x009e32a8`)

So `who = 0` becomes `0xffffffff` and is refused, and the valid range is `1..=8`.

The `Leaders` gate that follows is `leaders[who-1]`'s first dword (`Leaders 0x00e3a390`,
stride `0x6eec`), bits 0 and 1.

### FINDING — the leader-flag gate is not uniform

`num_objects_selected` requires **both** bits (`0x009f070c` bit 0, `0x009f0711` bit 1) and
so do `find_unit` and `valid_object_o`. `object_type_selected`, four instructions away in
the same source file, requires **bit 0 only** (`0x009f055e`). This is reachable: a leader
that is valid-but-not-present answers `object_type_selected` normally and `-1` to
`num_objects_selected`. Pinned by
`crates/don-bhs/tests/scenario_builtins.rs::only_num_objects_selected_requires_the_present_leader_bit`.

## 6. `find_unit` `0x009ebe10` and the scan it drives

The outer handler:

```text
who0 = who - 1
cursor = [0x00cc2214];  if (cursor < 0) cursor = 0;  [0x00cc2214] = cursor
if (unit_type->curr_len != 0) {
    ti = ScenarioFuncSet::get_type_index(unit_type, 0)     // 0x00a03480
    if (ti < 0) return -1
    if (!types[ti]->is_unit_type()) return -1              // devirt: 0x32 <= +4 < 0x19e
} else ti = -1                                             // 0x009ebe9a: "any unit"
if ((unsigned)who0 > 7) return -1
lf = leaders[who0];  if (!(lf&1) || !(lf&2)) return -1
r = find_unit(ti, cursor, 1)                               // 0x009e2850, who0 in ECX
if (r < 0) return -1
[0x00cc2214] = r;  return r
```

### FINDING — `find_unit` is stateful, and the state is a single file-static

`[0x00cc2214]` is one global cursor, **not** per player and **not** per type. It is read,
clamped at zero and written back before any refusal path, and it is set to every hit. The
inner scan starts at `cursor + 1`, so a repeated call with an unchanged cursor finds the
*next* match rather than the same object, and a script that calls `find_unit` for thirteen
different leader types in one frame — which is precisely what `general_powers.bhs` does —
walks the band forward across those thirteen calls. A host that resets this per call, or
gives each player its own, changes what shipped scripts select.

### `ScenarioFuncSet::find_unit` `0x009e2850`

```text
n = Objects+0x15c+who0*4                            // the band high-water
if (start >= n || start < 0) start = 0              // 0x009e286b
i = first = start + 1
do {
    idx = (i < n) ? i : 0                           // 0x009e2897, cmovl — the wrap
    o = Units::lists[who0][idx]                     // 0x00c0aec0 + who0*0x1c
    accept = (o->[8] & 1) && o->is_captain()        // vtable +0xe8
    if (accept && ti >= 0) accept = o->is(ti, 0)    // vtable +0xb8
    if (accept) switch (mode) {
        case 0: break;                                       // accept
        case 1: accept = o->is_valid_unit() && o->is_on_map();  // +0x08, +0xbc
        case 2: accept = !o->is_on_map() && o->is_active();     // +0xbc, +0x4c
        default: accept = false;
    }
    if (accept) return idx
    i = idx + 1
} while (i != first)
return -1
```

The wrap sentinel is compared against `start + 1`, so index `start` itself is the **last**
candidate examined, not a skipped one. The `find_unit` builtin passes mode 1.

Vtable slot names are from `schema/types.json`: `+0x08 SubObjectData::is_valid_unit`,
`+0x20 SubObjectData::is_build`, `+0x4c SubObjectData::is_active`, `+0xb8 SubObject::is`,
`+0xbc ObjectData::is_on_map`, `+0xe8 ObjectData::is_captain`.

## 7. The two selection builtins read *local* state

`object_type_selected` `0x009f04e0` and `num_objects_selected` `0x009f06f0` both resolve
the select group as

```text
[0x00e8d444] + Console(0x00c06210)->[0x2a0] * 0x9f0
```

i.e. the **local console's** current select group, with owner byte at `GroupData+0x4a`,
member count at `+0xc` and a `short[]` of handles at `+0x8cc`. Both refuse unless that
group's owner equals `who - 1`.

### FINDING — a script branching on `object_type_selected` is not replay-reproducible

That group is local UI state. `general_powers.bhs`'s entire body is gated on
`object_type_selected(player, "<leader>")`, so what it does on a given frame is a function
of what the person at the keyboard has selected — while the statics it writes and the
timers it sets are `RunTimeEnv`/`ScenarioData` state that sits on the same `DataWalk`
interface as `CheckSum`. Whoever wires channel 14 or channel 13 to a live producer should
expect this script's writes to be *inputs* to the checksum that are not derivable from the
command stream. It is a single-player conquest scenario script, so this is a statement
about our reproduction boundary, not a claim that retail desyncs.

### `object_type_selected`'s loop returns on the first type match

```text
for each member o:
    if (!o->is(ti, 0)) continue
    if (!o->is_build()) return 1                   // 0x009f0617
    return o->is_active() ? 1 : 0                  // 0x009f062e, then 0x009f0586
return 0
```

A matching **building** answers with `is_active` and **stops the scan either way**. A
selection holding an unfinished Barracks and a finished one answers on whichever comes
first in the group, so the answer depends on selection order.

### `num_objects_selected` is not the group's member count

It returns `GroupData::get_num_cap_const` `0x0070f960`, which walks the members and
decrements for every one that is not `(o->[8] & 1) && o->is_captain()`. That body indexes
`[0x00c0ab84 + owner*0x1c]`, a base this lane did **not** identify — it is neither
`Objects` (`[0x00c0618c]`) nor `Units::lists` (`0x00c0aec0`). The predicate is reproduced
over the object band, which is the same test on the same handles; if a host's two bands
can disagree, that is where to look.

## 8. `bubble_text_obj` `0x009ff550`

```text
who0 = who - 1
if (!valid_object_o(who0, o)) return -1            // 0x009e32a0, inline below
obj = Objects::lists[who0][o]                      // [0x00c0618c] + 0x14 + who0*0x1c
x = obj->[0x10] ^ 0x63637                          // SubObjectData::x_internal
y = obj->[0x14] ^ 0x63637                          // SubObjectData::y_internal
MessageWin::add_bubble_message(text, x, y, Console->[0x298], y)   // 0x007e7e20
return 1
```

`valid_object_o` `0x009e32a0` is: `who0 <= 7` unsigned, `leaders[who0] & 3 == 3`,
`(unsigned)o <= 0xbb7` (so handles run `0..=2999`), the slot pointer is non-null, and
`obj->[8] & 1`.

The `^ 0x00063637` corroborates the tick12 lane's finding that object positions are the
stored `x_internal`/`y_internal` XOR `0x00063637`.

`MessageWin::add_bubble_message` receives five arguments and the fifth is `y` a second
time (`push ecx` at `0x009ff591` and again at `0x009ff59d`); the fourth is `Console+0x298`,
the local display player. This is presentation and is modelled as a receipt.

### FINDING — `Units::lists` and `Objects::lists` share a slot space

`find_unit` returns a `Units::lists` band **index**; `general_powers.bhs` assigns it to
`o` and passes it straight to `bubble_text_obj`, which resolves `o` through
`Objects::lists`. Those are two distinct symbols and this lane did not prove they alias,
but a shipped script feeding one to the other establishes that the slot number means the
same thing in both. `ScenarioWorldImage` therefore keeps one map keyed `(who0, handle)`
and says so; a host that can distinguish the two arrays should not inherit the
simplification silently.

## 9. Where the boundary sits, and what refuses

Everything above executes in `crates/don-bhs/src/scenario.rs`. What crosses into the host
is a defaulted `Host` method per *field*, each naming the retail address it stands for, and
each `Err(HostError::Unimplemented)` by default:

`game_seconds`, `game_info_flags`, `game_info_rush_rules`, `game_info_victory`,
`game_semaphore_bit`, `script_timers`, `find_unit_cursor` / `set_find_unit_cursor`,
`leader_flags`, `type_index_by_name`, `type_is_unit_type`, `unit_band_count`,
`unit_band_probe`, `unit_band_is_type`, `object_band_probe`, `object_band_is_type`,
`object_band_position_internal`, `selection_group_owner`, `selection_group_members`,
`local_display_player`, `add_bubble_message`.

`crates/don-bhs/tests/scenario_builtins.rs::a_host_that_models_nothing_poisons_every_entry`
walks all 21 against `NullHost` and requires `Unimplemented` from every one. The VM's
default `MissingBuiltinPolicy::Fail` then stops the script rather than substituting
`ScriptFuncSet::get_err_return`.

`ScenarioHost` in the same module is a host that answers the cohort from an explicit
`GameImage` + `ScenarioWorldImage`. It is a fixture, not a simulation: nothing in it
advances, decays or interacts. It exists so a shipped script can be run without `don-sim`.

## 10. The duplication against `don-sim`, and who should collapse it

`crates/don-sim/src/script_runtime.rs` holds a private `ScriptTimers` (line ~183) and a
`SimScriptHost` whose `call` answers 77/78/79 and 96–105 and 298 directly. The two
implementations were derived independently and **agree** on everything both cover: the
100-entry cap, insertion ordered by expiry with equals inserted before, case-insensitive
name matching, the `-1`/`0`/`1` trichotomy, expiry consuming the timer, and — notably —
that builtin 103 selects `Victory::Population`. That agreement is worth something as a
cross-check and worth nothing as a maintenance plan.

The collapse direction is `don-sim` → `don-bhs`, because `don-bhs` is the crate the load
path and any standalone script runner can reach, and because `ScenarioData::timers` is
script-engine state rather than simulation state. Concretely: `don-sim`'s `SimScriptHost`
should own a `don_bhs::scenario::ScriptTimers`, implement the new `Host` reads listed in
§9, and delegate through `don_bhs::scenario::call_scenario` before its own 81-index table.
That is a `don-sim` edit and this lane does not own `don-sim`.

One naming hazard for whoever does it: `don-sim` already has a **different** trait called
`ScenarioHost` and a **different** method called `call_scenario` in `script_runtime.rs`.
`don_bhs::scenario` now exports both names too. Nothing collides today (no single type
implements both traits, and `don-sim` imports `don_bhs` items by name rather than by glob),
but a `use don_bhs::*` in that file would break immediately.

## 11. Deliberately not written

- **The hash fast path of `String::operator==`** — §3. The hash function is derived and
  written down; the *predicate* is not implementable without a `String` flags model that
  `don_bhs::Value` does not have.
- **MSVCRT `towlower` outside ASCII** — §3. Poisons instead of folding.
- **`[0x00c0ab84]`**, the band base `GroupData::get_num_cap_const` indexes — §7. Named,
  unidentified.
- **The remaining 754 `ScenarioFuncSet` entries.** `aibestbuildlibrary.bhs` needs 29
  builtins, `economic.bhs` 33 and `defensive.bhs` 42 (from
  `schema/bhs-builtins.json`'s `called_by_shipped_scripts`); this cohort covers 12 of
  `defensive.bhs`'s 42. The AI opening book's remaining gap is the counting and ordering
  family — `num_cities`, `num_type`, `num_type_with_queued`, `find_build`, `have_tech`,
  `can_pay_cost`, `place_building_with_cost`, `train_unit_with_cost` — every one of which
  needs a real `don-sim` object band and leader economy, not an image.
- **`get_mapstyle` `0x009e4cc0`**, which both AI scripts call. It is
  `[0x00e7fc18] + World+0x30 * 0x58 + 0x14`, a `String` inside a 0x58-stride shipped-data
  record. That is a `don-content` table, not a `Game` field, so it belongs with whoever
  owns the map-style table rather than being invented here.
