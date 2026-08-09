# BHS: everything we know

Big Huge Script — the scripting language shipped with Rise of Nations. This is the handoff
for anyone building an interpreter, whether that's a lane, Codex, or a future session.

Two lanes are live on this as of writing: one splicing the engine out (`docs/tracks/bhs-engine.md`)
and one driving the retail compiler under the oracle (`docs/tracks/bhs-compiler-oracle.md`).
Read those for current progress; read this for the standing picture.

---

## Why it is not optional

`script_run_time` is **channel 15 of the engine's own 15 lockstep checksum channels**, and
`scenario_data` is channel 14. Script state is therefore *simulation-critical* — any scripted
game is unvalidatable against a replay without a faithful VM. `docs/mechanics/COVERAGE.md`
lists both as runtime-orphans with no derived traversal at all; a VM is what closes them.

It is also the gate on two things Ember wants: running the **real** shipped AI rather than a
hand transcription, and the entire Steam Workshop mod library.

## Why it is smaller than it sounds

The decomposition that makes this tractable — **confirm it from symbols before relying on it**:

| part | where it runs | who must own it |
|---|---|---|
| **compiler** (source → bytecode) | once, offline, outside the tick | **retail can do it for us** |
| **VM** (bytecode → effects) | every frame, inside `Game::do_frame` | must be ours |
| **builtins** (~1,000 host functions) | called from the VM | must be ours — but they are the boundary to our sim, which we owe regardless |

There is a `Compiler` class and `Compiler::eval_command`, which is what suggests bytecode
rather than a tree-walker. If that turns out wrong, the plan changes and the parser becomes
ours too.

The prize: drive retail's own compiler under the oracle on hbox, feed it a `.bhs`, take the
bytecode out. The compiler then never needs reimplementing — it isn't in the tick loop, so its
performance and integration are both irrelevant.

---

## What is measured

### Scheduling

`Game::do_frame` calls `run_script(<name>, 0 args)` **once per simulation frame, before the
frame counter increments**. The script body *is* the tick handler — it takes no arguments and
must be a state machine over `static`s. That is why the shipped scripts are written as giant
step-number switches.

Attachment is a `Game`/`GameInfo` field, **not** a scenario-file field, so scripts are not
scenario-only. Whether the retail UI exposes a script picker in a skirmish is the one runtime
question that was never closed; the scenario route is the documented fallback.

### Output channels — both real

The prior survey concluded BHS had no output. That was wrong, twice over:

- **`print(v)` `0x00a048d0` and `print_line(v)` `0x00a04950`** are registered script functions
  whose entire body is an unconditional `OutputDebugStringW`. No debug flag, no build gate.
  Readable live out-of-process from `DBWIN_BUFFER`; `experiments/bhs/dbwin-reader.ps1` does it.
  **Neither appears in `ron-data/scriptfunctions.xml`**, which is why the survey missed them.
- **`open_file` / `file_write_*` / `close_file`** write a real XML file — `close_file` calls
  `XML::save` → MSXML `IXMLDOMDocument::save`. Lands in
  `C:\Users\ember\Documents\Microsoft Games\Rise of Nations\`. This is the channel that
  survives `prlctl exec`, since SYSTEM can read a file but cannot see another session's debug
  stream.

`debug_print` — the obvious candidate — **does not exist in this build**; calling it is a
compile error.

### The catalogue is incomplete

**`ron-data/scriptfunctions.xml` is missing 226 engine functions.** Enumerate builtins from the
binary, never from the XML. Among the missing are `set_object_type_attack` and
`set_object_type_armor` — which matter enormously, because they let us set damage *inputs* in
the live game rather than only observing outputs. That is what makes a real parameter sweep of
the damage pipeline possible inside the retail engine.

Total host functions: **783**. The three shipped scripts call **54** of them and can mutate the
world through only **12**.

### Primitives for experiments

- `create_unit` returns a **status, not an id** — recover the id with `find_unit`.
- There is **no teleport**; position is set at creation only.
- `unit_attack_order(who_atk, unit, who_def, obj)`.
- **`object_health` returns a percent, not HP** — its body is literally
  `(int)((float)cur/(float)max*100.0)`. The fix is `set_object_type_max_health(type, 100)`,
  which makes percent equal HP exactly.

### Failure behaviour

**Compile failure is nearly silent** — one red message, then the script name is cleared and it
never runs again that session. Know this before the first run or you will debug the wrong thing.

### A possible live REPL

`Compiler::eval_command` is reachable from `ConsoleWin::run_cmd`, and the chat box routes into
the same parser. There may be a live BHS REPL against the running simulation. **Ten minutes of
key-pressing would settle it**, and if it works it beats the file-based workflow outright.
Nobody has tried.

---

## What the scripts actually are

`ron-data/ai-scripts/` — `economic.bhs`, `defensive.bhs`, `aibestbuildlibrary.bhs`, 2,400 lines
total, by Mark Sobota and Mike Engle. They are a **one-shot opening book**, not the AI. The AI
is 306 compiled functions / 173,424 bytes in `main\game\leaders.cpp`.

Two measured facts about how the engine treats them:

- **`prod_script_run` (+0x78C) is a one-way latch** — set in `Leader::init`, cleared in
  `Leader::production_ai` on `SCRIPT_DONE`, never re-armed.
- **`BLOCK_ON_THIS` is a hard veto.** Players whose script stayed alive ran the ten compiled
  production stages **0 times in 30 game-minutes**; players whose script retired ran them
  **123 times**. Age advance escapes it via an every-30-ticks fast lane in `plan_strategy`.

Also: `boom_vs_rush` is `Personality::rush`, from a typed 24-int `Personality` struct at
`LeaderData+0x6DD4`, rolled by `Leader::random_personality`.

**A shipped bug, still there after 23 years:** `economic.bhs` step 20 can spin forever with its
watchdog disarmed — it tests `== 0` where the engine returns `-1` for an unmet prerequisite.
Observed generating 657 invalid orders for the Greeks in one match. A faithful VM must
reproduce this, not fix it.

---

## Assets on disk

| path | what |
|---|---|
| `ron-data/ai-scripts/*.bhs` | the three shipped scripts |
| `ron-data/scriptfunctions.xml` | the catalogue — **incomplete, see above** |
| `ron-data/triggerbuilder.xml`, `typenames.xml` | related data |
| `experiments/bhs/smoke.bhs`, `combat_probe.bhs` | written, **never run** |
| `experiments/bhs/dbwin-reader.ps1` | reads `print` output out of the guest |
| `experiments/bhs/bhs-api.tsv` | the API surface as catalogued |
| `docs/tooling/bhs-bridge.md` | the full bridge report |

The experiment scripts have never executed. **The first run tests the documentation as much as
the game** — all 34 called functions were verified present in the binary, and one candidate
(`valid_unit_o`) was caught as a C++ helper rather than a script function, which would have
failed to compile.

---

## Open questions, ranked

1. **Is it bytecode or a tree-walker?** Everything above assumes the former. Settle it first.
2. **Does the chat-box REPL work?** Ten minutes, potentially removes the whole file workflow.
3. **What is the value model** — how are ints, strings and object handles represented, and how
   does script state persist across frames? It must persist, since `run_script` takes no
   arguments and the scripts are state machines. That persistence is exactly what lands in the
   `script_run_time` checksum.
4. **The builtin call convention** — how arguments and returns cross the VM/host boundary.
5. **Can the retail compiler be driven under the oracle?** If yes, we never write a parser.
6. **What is the script-entry-point form for a skirmish?** Copying any shipped `skirmish.bhs`
   out of the VM would settle it.
