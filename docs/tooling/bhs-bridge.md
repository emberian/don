# The BHS scripting bridge

Turning the retail game into a programmable laboratory using its own scripting
language, Big Huge Script.

**Status of the critical unknown: resolved, affirmatively.** BHS can write output that
an external process reads, by two independent channels, in the retail build, with no
debug flag. Details in §4. The fallback plan (script sets up state, we read process
memory) is no longer necessary, though it remains available and complementary.

Everything below is static analysis of `ron-bin/riseofnations.exe` and the shipped data
files. **Nothing here has been executed.** Claims are marked `[measured]` when verified
in the file by disassembly or by parsing the data, and `[inferred]` when reasoned from
structure. The experiment scripts in `experiments/bhs/` have not been run and have not
been compiled by the game — the first run is itself a test of this document.

---

## 0. The unlock: `ron-bin/sbl/rise.pdb` is the matching symbol file

Before anything BHS-specific, the finding that made the rest cheap and that changes work
far beyond this task.

`ron-bin/sbl/rise.pdb` is a 57 MB Microsoft PDB carrying **full C++ symbols** for
`riseofnations.exe`. It is not a stripped or partial file: `Has Globals: true`,
`Has Publics: true`, `Is stripped: false`.

It is **the** PDB for our exact binary, not a near-miss: `[measured]`

| | value |
|---|---|
| PE debug directory `PdbFileName` | `E:\agent\_work\2\s\main\game\rise.pdb` |
| PE debug directory GUID / age | `{51D4F219-61C6-4F84-9D5B-C3361B0D291F}` / 1 |
| `rise.pdb` GUID / age | `{51D4F219-61C6-4F84-9D5B-C3361B0D291F}` / 1 |

37,138 public symbols resolve to addresses. Sanity check against the project's
established ground truth — every one of these was derived the hard way, and every one
lands on a real name: `[measured]`

| our name | PDB symbol |
|---|---|
| `FUN_00644130` (damage) | `ObjectData::get_damage(int,int,unsigned long,int,int,int*) const` |
| `0x00a39cf0` (`next_float`) | `Random::get(void)` returning float |
| `0x00a39d70` (`in_range`) | `Random::get(int,int)` |
| `0x00a1d110` (`RString::AsScaled`) | `String::fraction(int) const` |
| `0x00a46830` | `adler32(unsigned long, unsigned char const*, unsigned long)` |
| `FUN_00936560` | `CheckSums::check_all(void)` |
| `FUN_009459d0` | `CommandPackage::process_check_sums(CheckSumsCommand*)` |
| `FUN_00570170` (Ghidra timeout) | `Constants::log_data(Log*) const` |
| `0x00846450` ("dead code") | `Doober::get_num(TCoord,TCoord,int,int) const` |

Extraction, no license concerns beyond the usual (the PDB is game content, so it stays
gitignored alongside `ron-bin/`):

```sh
/opt/homebrew/opt/llvm/bin/llvm-pdbutil dump --publics \
  /Users/ember/dev/don/ron-bin/sbl/rise.pdb > publics.txt
```

Records are `S_PUB32` with `addr = <section>:<offset>`; section 1 is `.text` at
`0x401000`, 2 is `.rdata` at `0xac5000`, 3 is `.data` at `0xc06000`. Add the section
base to the offset to get the VA. `llvm-undname` demangles, though it line-wraps, so
for bulk work parse the mangled form directly — `^\?(\w+)@(\w+)@@` gives
`method`/`class` for 12,452 of them.

`schema/rise-symbols.tsv` (2.5 MB, `EA \t section \t mangled`) already exists in the
tree from this work.

**This should be propagated well beyond the BHS task.** Ghidra's 47,177 unnamed
functions can be named wholesale; `schema/islands.jsonl`'s classification gaps and the
two combat-critical functions sitting in them can be identified by name; the "descriptor
type tag" style of dead end gets much cheaper. It does not weaken the project's one
rule — a symbol is a *name*, not a semantics, and values still come from behaviour —
but it removes an enormous amount of guessing about *which* function to look at.

Also present and unexamined: `CrossplayNetLib.pdb`, `CrossplayProxy.pdb`, `dssl.pdb`,
`PartyWin.pdb`, `PlayFabMultiplayerWin.pdb`.

---

## 1. The API

### 1.1 Decoding `scriptfunctions.xml`

The catalogue is one line of XML: 56 `GROUP`s, 626 `FUNC`s, 964 `PARAM`s. The `type`
attributes are integers, and the decoder is `ron-data/paramtypes.xml`: **a `type`
attribute is the zero-based ordinal index of a `PARAMTYPE` element in that file.**
`[measured]` — verified by checking that each `PARAM`'s `name` matches the decoded type
name: 964 / 964, zero mismatches.

That yields the return type of every function too, since `FUNC/@type` uses the same
encoding. `FUNC type="48"` is index 48 = `int_return`, documented as *"1 if true or
success, 0 if false, -1 if failed"* — and 428 of 626 functions return it.

Base types are the first six entries: `int`(10) `real`(11) `string`(12) `void`(13)
`anytype`(14) `params`(15). Everything after is a documented alias: `who` is an int that
means a nation index, `unit_o` an int that means a unit id, `x`/`y` ints that mean
*"game world X/Y position"*, `dist_radius` an int *"in tile units"*.

Note that `x`/`y` and `dist_radius` are documented in **different words**, and the engine
carries a 192-subunits-per-tile world coordinate (`String::fraction`, per-field scale).
Whether script `x`/`y` are tiles or sub-tile world units is **not settled** by the data
files. `experiments/bhs/combat_probe.bhs` measures it in one run by printing requested
and actual positions side by side.

A joined, machine-readable catalogue is at **`experiments/bhs/bhs-api.tsv`** — every
function with its group, decoded return type, decoded parameter list, the catalogue's
`func_index`, and the handler address and `FuncSet` class recovered from the PDB.

### 1.2 The catalogue is stale — trust the binary

The Extended Edition engine registers **845 distinct script functions**; the catalogue
lists 626. `[measured]`

- **7 catalogued functions do not exist in this build**: `debug_print`, `debug_break`,
  `call`, `is_script_loaded`, `is_string`, `is_int`, `is_real`. Confirmed twice over —
  no `FuncSet` symbol, and no UTF-16 name literal anywhere in the image. **A script
  calling `debug_print` will not compile.** This matters because `debug_print` is the
  obvious first guess for the output problem, and it is a dead end.
- **226 engine functions are absent from the catalogue.** Roughly half are the
  Conquer-the-World set (`ctw_*`). The remainder include several that are directly
  load-bearing for us (§1.4).
- The string `"scriptfunctions.xml"` does not appear in the executable at all. The game
  never reads this file; it is a trigger-editor authoring artifact. `[measured]`
- **The catalogue's `func_index` values are meaningless for the EE build.** Registration
  order in EE is Math (0–14), Trigger (15–17), String (18–23), Array (24–30), Scenario
  (31–872). The catalogue numbers `print_msg` 0 and `debug_print` 625. This does not
  matter for writing scripts, because names are resolved to indices at *compile* time by
  a case-insensitive linear scan (`ScriptGameInterfaceBase::find_func`, `0x009d5360`),
  but it means you cannot use catalogue indices to talk about engine functions.

### 1.3 Readers vs actuators vs control flow

BHS has no "control flow functions" — control flow is language syntax (§5). The
`Triggers` group (`enable_all_triggers`, `disable_all_triggers`, `is_trigger_enabled`)
is the closest thing, and it manipulates the enable-bitmask of inline `trigger` blocks
rather than directing execution.

Roughly, by group:

**State readers** — `Building Info`, `Cities`, `Comparison Checks`, `Game Settings Info`,
`Map Info`, `Nation & Player Stats`, `Object Location`, `Population`, `Search Functions`,
`Selection`, `Techs & Ages`, `Time`, `Unit Info`, `Mouse Over Objects`, most of
`Diplomacy`, plus the read half of `Health`.

**Actuators** — `AI Building Placement`, `Create Objects`, `Destroy/Kill Objects`,
`General AI Toggles`, `Groups`, `Team Change`, `Types Controls`, `Unit Orders`,
`Unit Training`, `Victory/Defeat`, `Map Visibility`, `Market`, the write half of
`Health` and of `Diplomacy`.

**Presentation / harness** — `Audio`, `Camera`, `Chat`, `Messages*`, `Objectives*`,
`Ping*`, `UI*`, `Screenshot`, `Flags`, `Scale`, `Keyboard/Mouse Events`.

**Runtime** — `File IO (XML)`, `Timers`, `Utilities, Math/String/System`.

### 1.4 The four primitives an experiment harness needs

All four exist. `[measured]` — present in the catalogue, present as a `ScenarioFuncSet`
symbol, and present as a registered UTF-16 name literal in `.rdata`.

**CREATE units** — `int_return create_unit(who, x, y, unit_type, num_units)` at
`0x009f4c50`. Also `create_unit_upgrade`, `create_unit_in_group`, `create_building`,
`create_building_near`.

> `create_unit*` returns the final `Objects::init_unit` attempt's object id, or `-1`.
> Allocation is non-atomic: `[41, -1]` returns `-1` while object 41 remains live, and
> `[-1, 42]` returns 42.  Every successful id is published before the next attempt.

**SET positions** — there is **no teleport/set-position function**. Position is set at
creation time by `create_unit`'s `x`/`y`, and changed thereafter only by ordering
movement (`unit_move_order`, `unit_waypoint_order`, `group_move_order`). Read back with
`object_position_x/y(who, object_o)` (`0x009f1360` / `0x009f1470`). For a controlled
experiment this is fine — place at creation, and never move.

**ORDER attacks** — `int_return unit_attack_order(who_attacker, unit_o, who_attacked, object_o)`
at `0x009f79e0`. Note it takes **both** nation indices; the target is addressed as
(owner, id). Also `unit_attack_to_order`, `unit_attack_ground_order`, and the group
forms (`group_attack_order` has three overloads in the binary).

**READ hit points** — and here is the sharp edge:

```
int_return object_health(who, object_o)      // 0x009f5910
int_return object_max_health(who, object_o)  // 0x009f5f60
```

**`object_health` returns a PERCENT, not hit points.** `[measured]` — the tail of
`0x009f5910` is literally

```c
iVar5 = (int)(((float)current / (float)max) * 100.0);
```

`object_max_health` returns the absolute maximum. So absolute current HP is
`pct * max / 100`, truncated — on a 145-HP Phalanx that is ~1.45 HP of quantisation,
which would destroy a damage-ground-truth run.

**The fix, and it is a clean one:** `set_object_type_max_health(object_type, num_hitpoints)`
(`0x009f5da0`) is a registered script function. Force max health to **100** and the
percent equals the absolute hit points exactly. This is what `combat_probe.bhs` does.

Two further useful properties of `object_health`, both `[measured]`:

- It calls `ScenarioFuncSet::valid_object_o` first and **returns −1 for an invalid or
  destroyed object**, so one call is both the sample and the liveness check. There is no
  registered `valid_unit_o` script function — that name exists only as a C++ helper, and
  referencing it in a script is a compile error.
- It emits a `Log::say("object health", …, <frame>, <value>, …)` record. That is the
  multiplayer sync log, gated by `Log::global_logging` (`0x00b1b6ec`); we have not found
  a way to enable it from outside, so treat it as unavailable but noted.

### 1.5 The undocumented functions that matter most

Absent from `scriptfunctions.xml`, present and registered in the binary — these are the
best find in the catalogue work, because they let us set the *inputs* of the damage
computation rather than only observing its outputs: `[measured]`

| function | handler | name literal |
|---|---|---|
| `set_object_type_attack(object_type, num)` | `0x009f6170` | `0x00b0cbcc` |
| `set_object_type_armor(object_type, num)` | `0x009f5fb0` | `0x00b0cb60` |
| `set_object_type_max_health(object_type, num)` | `0x009f5da0` | `0x00b0cb1c` |
| `set_object_type_max_range` / `_min_range` | `0x009f6330` / `0x009f64f0` | |
| `set_unit_type_speed(unit_type, num)` | `0x009f66b0` | |
| `set_type_line_of_sight`, `set_type_job_time` | `0x00a00550`, `0x009ea880` | |
| `print(value)`, `print_line(value)` | `0x00a048d0`, `0x00a04950` | see §4 |
| `show_notice`, `bubble_text` | `0x00a02740`, `0x009ff4f0` | |
| `get_last_unit_built`, `get_last_building_built`, `get_last_razed_building` | | |
| `rename_type`, `rename_city`, `set_capital`, `set_preq` | | |
| `object_ignore_orders` / `object_obey_orders` | | |
| `unit_garrison_order`, `unit_eject_order`, `get_stance`, `set_no_attack` | | |

`set_object_type_attack` and `set_object_type_armor` are the two that turn this from an
observatory into an experiment. Both write a per-nation type record and set a dirty flag
(offsets `+0x1e8` for attack and `+0x214` for armor on the type object) across the
whole type range. `[measured]` The **scale** they expect is an open question: `unitrules.xml`
lists Hoplites `ATTACK` as 13 while the engine stores attack ×10 (established ground
truth). `combat_probe.bhs` sets a round value and reads back the resulting per-hit damage,
which settles it.

---

## 2. How a script is attached, and to what kind of game

### 2.1 Attachment

There is **one** script slot on the running game object, and it is a *game* setting, not
a scenario-file setting: `[measured]`

- `Game+0x500` — `String`, the script's **base name with the extension stripped**
- `Game+0x528` — `String`, the script's **directory**

`SetupWin::set_script_path` (`0x005b7aa0`) writes both: it takes the selected path,
splits it with `String::get_directory()` / `String::get_file()` into `Game+0x528` and
`Game+0x500`, then finds the last `.` with `String::find_index_reverse('.')` and
`String::truncate`s it away.

`GameInfo::get_full_script_path` (`0x005d4c80`) reverses it: `Game+0x528`, a `\` if
needed, `Game+0x500`, then `String::check_ext` to re-add the extension.

`Setup::build_game` (`0x005ac190`) compiles it at game start, at call site `0x005ad83a`:

```
lea  eax,[ebp-0x6c] ; push eax ; call GameInfo::get_full_script_path
push 1              ; ScriptReloadType
mov  ecx, 0xeb6a90  ; script_compiler
call Compiler::compile
test eax,eax
je   <continue>                 ; compile() returns 0 on SUCCESS
mov  ecx,[game] ; add ecx,0x500 ; call String::close   ; on failure, CLEAR the name
... MessageWin::add_message(<...>, RED, ...)           ; and show a red in-game message
```

Two consequences worth having in hand before the first run: **`Compiler::compile` returns
zero on success**, and **a script that fails to compile is silently disabled for the rest
of the session** (the name is cleared, so `do_frame` will never call it) after one red
message. If nothing at all happens, suspect a compile error, not a logic error.

`SetupWin::do_scen_script_sync` (`0x005b78b0`) also touches `Game+0x500`/`+0x528`, which
is consistent with the scenario picker defaulting the script to match the chosen
scenario. `[inferred]`

**The entry function name equals the file base name, and takes zero parameters.** This is
not folklore: `set_script_path` strips the extension to produce `Game+0x500`, and
`do_frame` passes that same `Game+0x500` to `run_script` as the *script name*. So
`combat_probe.bhs` must define `combat_probe()`. `[measured, by construction]` The
shipped AI scripts corroborate the naming convention at their own layer —
`economic.bhs` defines `int ai economic(...)`, `defensive.bhs` defines `int ai defensive(...)`.

### 2.2 Skirmish or scenario?

**The script is attached to the `Game`/`GameInfo` record, not to the scenario file**, and
`Setup::build_game` runs for every game start. So the mechanism is not scenario-only.
`[measured]`

What is **not** settled statically is whether the retail UI *exposes* the script field
outside the scenario flow. `SetupWin::set_script_path`, `validate_script` (`0x005b75c0`)
and `validate_scenario_select` (`0x005b8b70`) exist, and the file-dialog filter strings
`scx,bhs,xml` / `bhs,xml` / `bhs` are present in `.rdata`, but which control surfaces
them in EE's menus is a runtime question. Treat "attach a `.bhs` to a plain skirmish from
the setup screen" as **plausible and worth trying first**, with the scenario route as the
guaranteed fallback (§6).

Related: `ScenarioRead::load_triggers_chunk` (`0x009a9b30`) and
`ScenarioWrite::save_triggers_chunk` (`0x009a5b50`) are both 16-byte stubs. Scripts in EE
are referenced by filename rather than embedded in the `.scn`. `[measured]`

Separate script slots exist and are compiled alongside: `ScenarioData::general_powers_script`
(`0x00e8d41c`), the Conquer-the-World setup/strategy/diplomacy scripts, and
`Leaders::init_production_script` for AI scripts like the three in `ron-data/ai-scripts/`.
Those are different entry points with different signatures; do not confuse the AI-script
form (`int ai name(int who, ref int step, …)`) with the game-script form.

### 2.3 Scheduling — this is the part that matters for experiment timing

`Game::do_frame` (`0x00591EF0`), lines 104–111 of the decompilation: `[measured]`

```c
if (*(short *)(game + 0x508) != 0) {                  // script name is non-empty
  run_script(&script_run_time, game + 0x500, 0);      // 0 arguments
}
...
if ((0 < *(int *)(game + 0x550)) && (general_powers_script_len != 0)) {
  run_script(&script_run_time, &general_powers_script, 0);
}
...
*(int *)(game + 0x550) = *(int *)(game + 0x550) + 1;  // frame counter, LAST
```

**The script body is the per-simulation-frame tick handler.** It is called once per frame,
synchronously, from inside `do_frame`, *before* the frame counter at `Game+0x550`
increments. `TurnControl::do_frame_solo` (`0x009557d0`) also calls `run_script`.

Practical consequences:

- A script is a **state machine over `static` locals**, never a linear program. `static`
  initialisers run once; the body runs every frame. The shipped AI scripts are written
  exactly this way (`static int prev_step0 = 0;` and friends).
- Because it runs inside `do_frame` and before the counter bumps, **anything a script
  reads is the state of the frame it is running in**, and anything it writes lands before
  that frame's simulation completes. There is no lag to model.
- The script is on the deterministic path. `RunTimeEnv::walk_data` and `Script::walk_data`
  exist, i.e. script state participates in `DataWalk` — the same interface as
  `CheckSum`/`SaveGame`/`LoadGame`. Script state is therefore sim-critical state, and a
  script that reads the RNG (`rand_int`, `rand_real`) **will perturb the RNG streams**
  the project is trying to reproduce. Avoid `rand_*` in probes.
- Cost per frame matters. `close_file()` re-serialises the whole XML DOM through MSXML;
  do not call it every frame.

### 2.4 The interpreter, briefly

BHS is compiled to bytecode by an in-process compiler (`Compiler`, `Lexer`, `yyFlexLexer`,
`SyntaxNode`, `OpCode`, `SymTable`) and executed by a stack VM. `[measured]`

- `RunTimeEnv::exec` (`0x009C3600`) is the fetch/dispatch loop: read `bytecode[pc]`,
  `pc++`, call `VirtualMachine::execute_next`, bump an instruction counter, repeat while
  `check_vm_running`.
- `VirtualMachine::execute_next` (`0x009E0840`) is a switch over opcodes `0x00`–`0x47`.
  Native calls are `0x38`/`0x39` (`call_func` with default / explicit arity); script-to-script
  calls are `0x36`/`0x37`; trigger enable bits are `0x3C`/`0x3D`; `0x45` is the inline
  trigger guard.
- Native dispatch is **not** a static jump table. It is
  `script_game_interface->funcs[func_index]->native_fn()` — a runtime-built
  `PtrArray<ScriptFunc>` with a raw code pointer at `ScriptFunc+0x4C`
  (`ScriptFuncSet::call_func`, `0x009D5500`).
- Registration is `ScriptFuncSet::add_new_func(rettype, L"name", handler, nparams)`
  (`0x009D4DA0`), called 873 times, 842 of them from the 52 KB
  `ScenarioFuncSet::init_funcs` at `0x009C7570` — which is why that function is
  `skipped_large` in `re/decomp-all/MANIFEST.jsonl` and why nothing in the corpus shows
  the table.
- Return-type tags seen at registration sites: `0x57BAD` int, `0x168174` String,
  `0x84048` void, `0x12F35F` float, `0x27B2C1` anytype, `0x64EA2A` varargs sentinel.

There is also a full in-game IDE compiled in — `ScriptEditor`, `ScriptEditBox`,
`ScriptWatchWin`, `ScriptLogWin`, `TriggerFuncBrowser`, plus `Compiler::eval_command`
reachable from `ConsoleWin::run_cmd` (`0x007da9b6`). See §6.4.

---

## 3. `who` is 1-based

`object_health`'s first act is `iVar6 = *param_1 + -1;` before indexing the nation array.
`[measured]` The shipped AI scripts corroborate: `switch (who-1)` in both `economic.bhs`
and `defensive.bhs`. In a two-player game the nations are `1` and `2`.

---

## 4. THE CRITICAL UNKNOWN: can BHS write output we can read?

**Yes. Two independent channels, both in retail, neither gated by a debug flag.**

### 4.1 Channel A — `print` / `print_line` → `OutputDebugStringW` (real-time)

The best answer available, and it was hiding in the 226 undocumented functions.

`print(value)` and `print_line(value)` are registered script functions taking one
`anytype` argument. `[measured]` — registration at `0x009c6fda` and `0x009c7008` inside
`StringUtilFuncSet::init_funcs`, with name literals `L"print"` (`0x00b12878`) and
`L"print_line"` (`0x00b12854`).

Their bodies are three instructions of substance. `StringUtilFuncSet::print`
(`0x00a048d0`): `[measured]`

```
00a048f4  call dword ptr [eax + 0x14]     ; ScriptType::get_string()  -- any type -> String
...
00a04923  call dword ptr [0xc8dc4c]       ; __imp__OutputDebugStringW@4
```

`print_line` (`0x00a04950`) is identical plus a second `OutputDebugStringW` of the
newline literal at `0x00b1389c`.

**There is no conditional.** No `IsDebuggerPresent`, no `global_logging` check, no
debug-build guard. The argument is `anytype` and is converted via the `ScriptType`
vtable, so `print_line(42)`, `print_line("text")` and `print_line(some_float)` all work.

`OutputDebugStringW` is reached through a resolved pointer at `0x00c8dc4c` rather than a
static import (the image has a `_kernel32_OutputDebugStringW_Thunk` at `0x00401000`),
which is why it does not appear in the PE import table.

**Reading it:** Windows funnels `OutputDebugString` through a session-scoped 4 KB shared
section `DBWIN_BUFFER` plus the events `DBWIN_BUFFER_READY` and `DBWIN_DATA_READY`. Any
process that owns those objects tails the stream live, with no debugger attached.
`experiments/bhs/dbwin-reader.ps1` does exactly this.

> **The one real constraint.** These objects are **per-session**
> (`\Sessions\<n>\BaseNamedObjects\DBWIN_BUFFER`). `prlctl exec` runs as SYSTEM in
> session 0; the game runs in Ember's interactive session. **A reader launched via
> `prlctl exec` will see nothing.** The reader must be started from a normal PowerShell
> window inside the VM. This is why channel B exists.

Also note only one process may own `DBWIN_BUFFER` — Sysinternals DebugView and
`dbwin-reader.ps1` cannot both run.

### 4.2 Channel B — `open_file` / `file_write_*` / `close_file` → an XML file (batch)

Sixteen `File IO (XML)` functions, all registered and implemented. `[measured]`

```
open_file(filename)                     close_file()
file_write_text(element, any_value)     file_write_attrib(attrib, any_value)
file_read_text(element)                 file_read_attrib(attrib, default)
file_set_category(element, use_existing) file_up_category()  file_get_category()
file_next_entry()  file_traverse_entries(element)  file_sort_entries(...)
file_remove(element)  file_exists(filename)  file_entry_exists(element)
get_curr_filename()
```

The mechanism, end to end: `[measured]`

- `open_file` (`0x009fe080`) first calls `close_file`, then builds a path from
  `PlayerProfile::get_app_directory("")` + a string-table fragment + the supplied
  filename, stores it in `ScenarioData::xml_filename` (`0x00ea535c`), and calls
  `XML::init`. If the filename is empty it falls back to a `prefs_get` default.
- `file_write_text` (`0x009fe5f0`) calls `check_xml_valid` — which **opens the default
  file itself if none is open**, so a bare write works without `open_file` — then
  `XMLNode::add_element` and `XMLElement::set_text`.
- `close_file` (`0x00a034e0`) opens with

  ```
  cmp dword ptr [0xed5aa8],0 / jne / cmp dword ptr [0xed5aac],0 / je
  mov ecx, 0xed5a88          ; ScenarioData::xml
  call 0xa27480              ; XML::save
  ```

- `XML::save` (`0x00a27480`) builds a `VARIANT` of type `VT_BSTR` around the stored
  filename via `SysAllocString`, and calls the MSXML `IXMLDOMDocument` vtable slot at
  `+0x108` — `save(VARIANT)`. That is the flush to disk.

**`close_file()` is the flush. Without it nothing reaches disk.**

**Where the file lands.** `PlayerProfile::get_app_directory` resolves through a helper
that calls `SHGetFolderPathW(CSIDL_PERSONAL)` and appends `\Microsoft Games` then
`\Rise of Nations`, `_wmkdir`-ing each. `[measured]` So:

```
C:\Users\ember\Documents\Microsoft Games\Rise of Nations\<filename>
```

`[inferred]` for the exact final separator — one string-table fragment in the path
concatenation is loaded at runtime and cannot be read statically. It is almost certainly
`"\"`; the smoke test confirms it in one run by simply looking for the file.

There is no visible sanitisation of the filename in `open_file`, so subdirectories may
work. Unverified.

**This is the channel that survives `prlctl exec`**, because the result is an ordinary
file that a SYSTEM-context process reads without any session concerns.

### 4.3 Channel C — `take_screenshot(filename)`

`0x009fe010` → `_wmkdir(L".\ScreenShots")` and a write into the game directory.
`[measured]` A cheap edge-trigger if all you need is "did event X happen".

### 4.4 What does NOT work, and why the prior survey stalled

- **`debug_print` does not exist in this build.** Not registered, no name literal, no
  symbol. `[measured]` It is the obvious candidate and it is a dead end — this is
  probably exactly what the earlier survey ran into.
- **Chat and on-screen messages are screen-only.** `chat` (`0x009e3d80`) and `chat_all`
  (`0x009e3e00`) format a line and hand it to the message-window renderer; there is no
  file handle or stream anywhere in the call chain. Same for `print_msg`,
  `print_game_msg`, `popup_dialog`, objectives and notices. `[measured]`
- **`bhs.log`** exists (`L"bhs.log"` at `0x00b04f3c`) but is written by
  `OpCode::write_code` — a *compiler* bytecode dump, not a runtime channel. `[measured]`
- **`Logs\GlobalLog.txt`, `profile_log.txt`, the MP `SyncLog`** all exist, but the
  general `Log` framework is gated by `Log::global_logging` (`0x00b1b6ec`) and we found
  no way to set it from outside.
- **There are no dev command-line switches.** A full ASCII + UTF-16 scan for
  switch-shaped strings finds exactly three: `+skipIntro`, `+connect_lobby`, `+temp_mod`.
  No `-dev`, `-debug`, `-log`, `-editor`. `[measured]`

### 4.5 Recommendation

Use **both**. `print_line` for the sample stream (cheap, real-time, ordered, no disk
churn), and one `open_file`/`close_file` summary at the end (SYSTEM-readable, survives a
crash of the reader, provides a completion signal for automation). The experiment scripts
do this.

---

## 5. The language

From the 2,400 shipped lines in `ron-data/ai-scripts/`. C-like, statically typed.
`[measured]` from usage; where a construct does not appear in the shipped corpus it is
marked.

```c
labels {                      // enum; first defaults to 0, trailing comma allowed
  BLOCK_ON_THIS = 1,
  DONT_BLOCK_ON_THIS,
  SCRIPT_DONE,
}

include "aibestbuildlibrary.bhs"

int ai economic (int who, ref int step, int boom_vs_rush, int num_loops)
{
  static int prev_step0 = 0;          // persists across frames
  String my_capital = find_city_with_num(who, 1);
  int player_age = age(who);
  ...
  switch (who-1) { case 0: ... break; }
  if (a && b) { } else if (c) { }
  for (i = 0; i < n; i = i + 1) { }
  while (cond) { }
  return SCRIPT_DONE;
}
```

- Types seen: `int`, `String` (capital S), `float`/`real`. `ref` for by-reference params.
- `static` locals persist across invocations — **the mechanism a per-frame script needs**.
- `//` comments. `include "file.bhs"`.
- Forward declarations: `int ai assign_idle (int who);`
- `trigger name() { ... }` blocks may be declared *inside* a function and are gated by
  `enable_trigger("name")` (a language construct compiling to opcodes `0x3C`/`0x3D`/`0x45`,
  not a catalogued function).
- The `ai` keyword marks an AI-script entry point. A **game** script (the `Game+0x500`
  slot) takes zero parameters and is called by name; write it as `int name()`. `[inferred]`
- String concatenation does not appear anywhere in the shipped corpus. Do not rely on it
  — emit several `print()` calls and one `print_line()` instead. `parse(string, params)`
  exists and returns a String, presumably a formatter, but its format syntax is
  undocumented and unused in the corpus.
- Nested calls as arguments do not appear either; the shipped code always binds an
  intermediate (`String my_capital = find_city_with_num(who, 1);` then passes it). The
  experiment scripts follow that form.

---

## 6. The worked experiment

`experiments/bhs/`

| file | what |
|---|---|
| `smoke.bhs` | Minimal bridge validation: proves the script is being called, that `print_line` reaches the debug channel, and that `close_file` produces a file. Run this first. |
| `combat_probe.bhs` | One controlled melee engagement, fully instrumented. |
| `dbwin-reader.ps1` | Owns `DBWIN_BUFFER` and tails `print`/`print_line` output. |
| `bhs-api.tsv` | The joined catalogue: 852 rows, XML metadata + binary handler addresses. |

### 6.1 What `combat_probe.bhs` does

Two `Hoplites`, nation 1 versus nation 2, on tiles found by walking outward from nation
1's capital until `map_is_passable` succeeds twice in a row.

Everything reachable that could perturb the result is switched off first:
`disable_production_ai`, `disable_combat_ai`, `disable_all_unit_ai`, `disable_city_ai`,
`disable_city_defeat` on both nations, then `disable_unit_ai` on each individual unit so
the defender never retaliates and neither unit repositions. `declare_war` both ways so
the order is legal. `add_reveal_point` + `set_explored` for both nations so the attack
order is not refused for want of visibility.

Then the three type stats are forced to round numbers:

```
set_object_type_max_health("Hoplites", 100)   // percent == absolute HP
set_object_type_armor     ("Hoplites", 0)     // isolate the armor term
set_object_type_attack    ("Hoplites", 20)    // known input
```

`unit_attack_order`, then one line per **health change** (not per frame), each stamped
with the frame number, so the output is a damage-event trace:

```
DONP sampling: frame delta_hp hp_pct
DONP hit 412 7 93
DONP hit 447 7 86
...
```

Finally a summary to `don_combat_probe.xml`.

`Hoplites` is chosen because it is melee (`RANGE 0-0rng`), has `TO_HIT 0` (no ranged
to-hit roll), and has `PREQ0 none` so it is available with no research. Base stats from
`ron-data/unitrules.xml`: `ATTACK 13, HITS 120, ARMOR 4`. Note the name is **plural** —
the engine's internal `NAME` strings are `Hoplites`, `Bowmen`, `Slingers`.

### 6.2 What one run settles

- **Coordinate scale.** The script prints requested and actual `x`/`y` side by side. If
  they match, `x`/`y` are whatever `object_position_*` returns and we are done; if actual
  is ~192× requested, script coordinates are tiles and world coordinates are sub-tile.
- **Attack scale.** Attack forced to 20 with armor 0. If the observed per-hit damage is
  ~20, `set_object_type_attack` takes stored (×10) units and the rule-file value is
  ×10-scaled on load; if it is ~2, it takes rule-file units. Either way we learn it, and
  we learn it by *capture*, not calculation.
- **Whether `object_health`'s percent really is 1:1 at max 100** — `object_max_health` is
  printed so the mapping is checked, not assumed.
- **Attack cadence** — the frame deltas between hits give the recharge interval directly.

### 6.3 Then what

Once the scales are known, the same harness sweeps: vary `set_object_type_attack` and
`set_object_type_armor` across a grid, one engagement per run, and capture
(attack, armor, damage) triples straight from the retail engine. That is exactly the
input domain `ObjectData::get_damage` takes and that the binary oracle cannot reach,
because it reads a live object graph. Feed the triples to the differential test as
tier-B evidence, always with the sample count and the input distribution recorded.

**Capture, do not calculate** applies with full force here. The point of this harness is
that no expected value is ever computed by hand.

### 6.4 The other route, if attaching a script proves awkward

`Compiler::eval_command(String const&, String&)` (`0x009befe0`) evaluates a BHS
expression and returns the result as a string. It has exactly one caller:
`ConsoleWin::run_cmd` (call site `0x007da9b6`), which appends the result to the console's
line list. `ConsoleWin::parse_cmd` is in turn reached from `ConsoleWin::on_key_click`,
**`ChatBox::on_modal_end`**, `CommandPackage::process_console_cmd` and
`CommandPackage::process_chat`. `[measured]`

So there is an in-game console — and the chat box routes into the same parser, which is
the classic RoN cheat-code path — capable of evaluating arbitrary BHS expressions
interactively. `ron-data/playerprofile.dtd` enumerates the bindable command names, and
they include `SYSTEM_TOGGLE_DEBUG_CONSOLE`, `SYSTEM_TOGGLE_GAME_CONSOLE`,
`SYSTEM_SCRIPT_EDITOR`, `SYSTEM_SCRIPT_LOG`, `SYSTEM_TOGGLE_SCENARIO_EDITOR`,
`SYSTEM_STEP_FRAME`, `SYSTEM_PLACE_NEW_UNIT`, `SYSTEM_DAMAGE_OBJECT`. `[measured]`

If this is reachable in retail it is strictly better than file-based scripts for
exploration — you get a REPL against the live simulation, and `SYSTEM_STEP_FRAME` gives
single-stepping. It is worth ten minutes of trying keys before committing to the file
workflow. We could not determine statically whether the bindings are active by default,
or how `ConsoleWin` decides a line is a command rather than chat.

---

## 7. Running it — exact instructions

Ember runs these by hand in the Parallels VM. `prlctl exec` runs as **SYSTEM**, so every
user path is written out in full.

### Step 0 — find the install and the user data directory

```powershell
# In a normal PowerShell window inside the VM (NOT prlctl exec).
dir "C:\Users\ember\Documents\Microsoft Games\Rise of Nations"
```

That directory is where `open_file` writes. It should already exist (the game `_wmkdir`s
it at startup). Also locate the install directory — the Steam default is
`C:\Program Files (x86)\Steam\steamapps\common\Rise of Nations`.

**Bring back one shipped game script.** The engine references `skirmish.bhs`,
`capture.bhs`, `europe.bhs`, `missile.bhs`, `italy_mp.bhs`, `carribean.bhs` by name (in
`ModManager::isScenarioBuiltIn`). We do not have any of them in `ron-data/`, and they are
the ground truth for the game-script entry-point form — the one thing in this document
marked `[inferred]` that a single file would settle.

```powershell
Get-ChildItem -Path "C:\Program Files (x86)\Steam\steamapps\common\Rise of Nations" `
  -Recurse -Filter *.bhs | Select-Object FullName
```

Copy `skirmish.bhs` out and into `ron-data/ai-scripts/` (or anywhere in the repo) before
going further. Read its first function's declaration; if it is not `int skirmish()`,
adjust both experiment scripts to match and tell me.

### Step 1 — start the debug-output reader

Copy `experiments/bhs/dbwin-reader.ps1` to `C:\Users\ember\don\dbwin-reader.ps1`, then in
a **normal PowerShell window inside the VM** — not `prlctl exec`, see §4.1:

```powershell
powershell -ExecutionPolicy Bypass -File C:\Users\ember\don\dbwin-reader.ps1 `
  -Filter DON -Out C:\Users\ember\don\probe.log
```

It prints `dbwin-reader: listening.` Leave it running. If it reports it cannot own the
channel, something else (DebugView, a debugger) holds `DBWIN_BUFFER` — close it.

### Step 2 — place the script

Copy `experiments/bhs/smoke.bhs` into the VM. **The filename must stay `smoke.bhs`**, and
the function inside it is `smoke()` — the engine derives the entry-point name from the
file's base name (§2.1). Try these locations in order:

1. Next to the scenarios, so the setup screen's script picker can see it. Look for a
   `Scenario` folder under both the install directory and
   `C:\Users\ember\Documents\Microsoft Games\Rise of Nations\`.
2. `C:\Users\ember\Documents\Microsoft Games\Rise of Nations\StandaloneScripts\`
   — `PlayerProfile::get_custom_script_directory` defaults to `\StandaloneScripts`
   (`[measured]` for the literal, `[inferred]` for the parent). Create it if absent.
3. Wherever `skirmish.bhs` turned out to live.

### Step 3 — attach and start a game

Start a **2-player skirmish**, human as player 1, one AI as player 2, smallest map,
Classical Age start.

On the setup screen, look for a **Script** field or picker (distinct from the Scenario
picker) and select `smoke.bhs`. If there is no such control in the skirmish flow, use the
scenario route: open the Scenario Editor, save a minimal 2-player scenario as
`smoke.scn` **in the same folder and with the same base name** as `smoke.bhs`, then start
that scenario — `SetupWin::do_scen_script_sync` is what makes the pairing work (§2.1).

### Step 4 — read the result

Within five simulation frames of the game starting you should see, in the reader window:

```
HH:MM:SS.mmm [1234] DON smoke frame=1 time_sec=0
...
HH:MM:SS.mmm [1234] DON smoke wrote don_smoke.xml -- latching off
```

Then, from the Mac:

```sh
prlctl exec "Windows 11" cmd.exe /c \
  "type \"C:\\Users\\ember\\Documents\\Microsoft Games\\Rise of Nations\\don_smoke.xml\""
```

- **Both appear** — the bridge is live on both channels. Go to step 5.
- **Debug lines but no file** — channel A works, the `open_file` path is wrong. Search
  the whole user profile for `don_smoke.xml` and tell me where it landed.
- **Neither, and a red in-game message at game start** — a compile error. The message is
  the only diagnostic the engine gives, and after it the script is disabled for the whole
  session (§2.1). Note the exact text.
- **Neither, and no message** — the script was never attached. `Game+0x500` is empty and
  `do_frame` is skipping the call. Try a different location or the scenario route.

### Step 5 — the real experiment

Same procedure with `combat_probe.bhs` (and `combat_probe.scn` if using the scenario
route). Start the game and leave it running; the probe takes a few hundred frames.
Collect:

```sh
# the sample trace
prlctl exec "Windows 11" cmd.exe /c "type C:\\Users\\ember\\don\\probe.log"
# the summary
prlctl exec "Windows 11" cmd.exe /c \
  "type \"C:\\Users\\ember\\Documents\\Microsoft Games\\Rise of Nations\\don_combat_probe.xml\""
```

Every line the probe emits is prefixed `DONP`. The `ABORT` lines name their own cause.

---

## 8. Open questions

Ordered by how much they block the next step.

1. **Does the retail UI expose a script picker outside the scenario flow?** (§2.2) Decides
   whether the harness is a two-minute skirmish setup or requires authoring a scenario per
   experiment. Runtime question, five minutes to answer.
2. **The game-script entry-point form.** `[inferred]` that it is `int name()` with zero
   parameters. One shipped `skirmish.bhs` settles it (§7 step 0).
3. **Are `x`/`y` tiles or sub-tile world units?** (§1.1) Measured by the first
   `combat_probe` run.
4. **What scale does `set_object_type_attack` take?** (§1.5) Measured by the first run.
5. **Is the in-game console / script editor reachable in retail?** (§6.4) If yes, it is a
   better tool than everything else in this document.
6. **The exact `open_file` directory separator fragment** — one runtime string-table load
   we cannot read statically (§4.2). The smoke test answers it.
7. **Can `Log::global_logging` be enabled?** Would turn `object_health` and many other
   script calls into an automatic frame-stamped file trace (§1.4).
8. **`ScenarioFuncSet::init_funcs` (`0x009C7570`)** is 52 KB and skipped by the bulk
   decompiler. The `push`-sequence decode recovers all 873 `(index, name, rettype,
   handler, arity)` tuples without decompiling it; `experiments/bhs/bhs-api.tsv` has the
   handler addresses but not yet the arity or the per-parameter types from `add_param`.

---

## 9. Provenance

Every address in this document is from `ron-bin/riseofnations.exe` (sha256 `30478a44…625079`,
PE32 i386, image base `0x00400000`), named via `ron-bin/sbl/rise.pdb` (GUID
`{51D4F219-61C6-4F84-9D5B-C3361B0D291F}`, age 1, matching the PE debug directory exactly).
Disassembly is capstone `CS_MODE_32`; decompilation is `re/decomp-all/<EA>.c`. Data-file
claims are from `ron-data/scriptfunctions.xml`, `paramtypes.xml`, `unitrules.xml`,
`playerprofile.dtd`, `triggerbuilder.xml` and the three scripts in `ron-data/ai-scripts/`.

No fidelity claim is made here. Nothing in this document is verified, proven, or a
refinement — it is a description of a mechanism, and the mechanism has not yet been run.
