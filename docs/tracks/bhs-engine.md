# The BHS script engine

Splicing Big Huge Script out of `riseofnations.exe`: what it is, what now works, and
what is still owed.

Scope: `ron-bin/riseofnations.exe` (PE32 i386, image base `0x00400000`, sha256
`30478a44…625079`) and its matching private PDB `ron-bin/sbl/rise.pdb`
(GUID `{51D4F219-61C6-4F84-9D5B-C3361B0D291F}`, age 1). Companion document:
`docs/tooling/bhs-bridge.md`, which covers *using* BHS as an instrument; this one
covers *reimplementing* it.

---

## 0. The headline: the decomposition holds, and it holds strongly

**CONFIRMED.** BHS is **source → bytecode → stack VM**. It is not a tree-walking
interpreter. Every load-bearing piece of that claim is `[measured]`:

- `OpCodeTypes` is a **73-entry named enum recovered whole from the PDB**
  (`LF_ENUM 0x3C85`, field list `0x3C84`): `OP_ASSIGN = 0` … `OP_ERROR_TOKEN = 72`,
  `NUM_OP_CODES = 73`. We did not have to infer a single opcode name.
- `RunTimeEnv::exec` (`0x009c3600`) is a fetch/dispatch loop over a **byte array**:
  `op = vm->code[vm->bip]; vm->bip += 1; vm->execute_next(op); bytecodes_executed++`.
- `VirtualMachine::execute_next` (`0x009e0840`) is a 3,716-byte switch whose
  dispatch is a byte index table at `0x009e167c` (indexed by opcode `0x00..0x47`)
  into a 39-entry target table at `0x009e15e0`.
- The compiler is a separate, complete pipeline: `Lexer`/`yyFlexLexer` →
  `SyntaxNode::eval` (`0x009dc710`, 12,225 bytes) → an `OpCode` **linked list** →
  `OpCode::count_code` (a size/offset pass) → `OpCode::write_op` (the emitter).
- The output is **serialised to a chunk container** —
  `ScriptFile::read_script_chunk` (`0x009c5440`) and nine `load_*` chunk loaders —
  so bytecode is a persistable artifact, not a transient in-memory tree.

The three-way split therefore stands, with the costs the task predicted:

| part | must we build it? | why |
|---|---|---|
| **Compiler** | **No — borrow it** | Runs once at `Setup::build_game`, offline, outside the tick. `Compiler::compile` (`0x009bf160`) returns **0 on success**. |
| **VM** | **Yes** | Runs inside `Game::do_frame`, once per frame; its state is checksummed. |
| **Builtins** | **Yes, but owed anyway** | They *are* the boundary to our simulation. |

### Why this is worth more than it looks

`script_run_time` is one of the sixteen lockstep checksum channels, and the engine
says so by name: **`CheckSums::check_script_run_time`** exists as its own symbol, and
the object is `RunTimeEnv script_run_time` at `0x00ebeeb0`. `RunTimeEnv::walk_data`
(`0x009c41a0`) and `Script::walk_data` (`0x009c5f30`) put script state on the same
`DataWalk` interface as `CheckSum`/`SaveGame`/`LoadGame`, so — by the established
project identity *sim-critical state ≡ save-game state* — **script state is
sim-critical**. `COVERAGE.md` currently lists channels 14 (`scenario_data`) and 15
(`script_run_time`) as runtime-orphans with no derived traversal; the VM is what
closes 15.

A calibration that matters, from the ron-ai lane: the shipped `.bhs` scripts are a
2,400-line **opening book**, not the AI. The real AI is 306 compiled functions in
`main\game\leaders.cpp`. So the VM matters for **checksum fidelity, replay
validation, and the Workshop mod library** — not because the AI lives in it.

---

## 1. What now works

| artifact | status |
|---|---|
| `schema/bhs-builtins.json` | **873 builtins**, complete, from the binary. 466 KB. |
| `crates/don-bhs/` | New workspace member. Builds on arm64. **21 tests pass.** |
| Opcode table | All 73, named, with operand counts and handler VAs. |
| Bytecode encoding | Recovered exactly (§2). |
| Value model | Recovered exactly (§3). |
| Builtin call convention | Recovered exactly (§4). |
| Cross-frame persistence | Recovered exactly, and it is not what you would guess (§3.3). |
| Retail compiler under the oracle | **In flight** — see §7. |

`cargo test -p don-bhs` — 10 unit + 10 integration + 1 doctest, all green.

> ⚠ `cargo test --workspace` currently has **5 failures, none of them mine**, all in
> `crates/don-sim/src/systems/order_dispatch.rs` and `systems/target.rs`
> (`a_move_order_walks_and_then_retires_on_arrival`,
> `a_path_failure_does_not_cancel_a_follow_on_guard`,
> `an_unreachable_destination_cancels_the_follow_on_attack`,
> `work_survives_a_long_run_without_leaking_orders`,
> `a_unit_acquires_closes_and_hits`). Those files are owned by another live lane and
> this lane never touched them. `don-bhs` itself is green and adds no failures.

---

## 2. The bytecode format, exactly

### 2.1 Instruction encoding

**One opcode byte, then zero to two little-endian 32-bit operands, unaligned.**
`[measured]`

`RunTimeEnv::exec` fetches exactly one byte and increments `bip` by 1. Every operand
read inside `execute_next` is `dword ptr [code + bip]` followed by `bip += 4`.
`OpCode::count_code` (`0x009c2ed0`) confirms it from the emitter side: it adds `1`
for the opcode and `4` for each present `OpArg`. Maximum instruction length is 9
bytes.

Two special cases, both from `count_code`:

- **`OP_MARKER` (0x46) emits nothing at all** — `if (op != 0x46)` guards the entire
  emit body. It is a compiler-internal label placeholder.
- **`OP_SCRIPT_MARKER` (0x47) emits 1 + 4 bytes and is a runtime no-op** — its
  handler at `0x009e0f82` is a bare `add dword ptr [esi + 0xc], 4`. It marks a
  function boundary in the stream.

### 2.2 Operand encoding — `OpArg`

`OpArg` is `{ unsigned char op_arg_type; union { SymType* sym_type; OpCode*
op_pointer; int index_pointer; }; }` (PDB, `LF_STRUCTURE 0x1ACF`). `OpArg::write_arg`
(`0x009c27d0`) emits: `[measured]`

| `op_arg_type` | meaning | emitted |
|---|---|---|
| 0 | absent | nothing |
| 1 | `SymType*` | the symbol's index field (`SymType+0xc`), **OR-ed with `0x20000000` when the symbol kind is `0x123`** |
| 2 | boxed literal | the first dword of the pointed-to object |
| 3 | code label | the pointer slot itself, which by emit time holds a resolved absolute code offset |

That `0x20000000` at emit time is the same bit `VirtualMachine::get_value` tests at
run time — the two halves agree, which is a good consistency check on the reading.

### 2.3 The complete opcode table

All 73 names are the PDB's. Operand counts are from the dispatch handlers.

| # | name | operands | notes |
|---|---|---|---|
| 0x00 | `OP_ASSIGN` | — | assignment group, handler `0x009e10e9` |
| 0x01–0x08, 0x0a, 0x0b, 0x0f–0x11, 0x17, 0x1f–0x24 | binary operators | — | **all 19 share one handler**, `0x009e11e5` |
| 0x03, 0x0c–0x0e, 0x18–0x1e | compound assignments | — | assignment group |
| 0x09, 0x16, 0x25 | `OP_UNA_NOT`, `OP_UNA_NEGA`, `OP_UNA_TILD` | — | handler `0x009e11bd`, passes a null rhs |
| 0x12, 0x13 | `OP_INC_OP`, `OP_DEC_OP` | — | peeks and swallows a following `OP_POP` |
| 0x14, 0x15 | `OP_INC_OP_POST`, `OP_DEC_OP_POST` | — | same peek |
| 0x26 | `OP_PUSH` | VarRef | |
| 0x27 | `OP_POP` | — | |
| 0x28–0x2c | `OP_CREATE_*` | 1–2 | construction |
| 0x2d–0x31 | array/struct access | 0–1 | |
| 0x32, 0x33 | `OP_INIT_COPY`, `OP_INIT` | VarRef | `set_value(ref, popped)` |
| 0x34 | `OP_CAST` | type tag | |
| 0x35 | `OP_CAST_BOOL` | — | |
| 0x36 | `OP_CALL` | script index | `call_script(-1, idx)` — same file |
| 0x37 | `OP_CALL_INCLUDE` | script index, **file index** | note the order |
| 0x38 | `OP_CALL_GAME` | builtin index | arity from the declaration |
| 0x39 | `OP_CALL_GAME_VARIED` | builtin index, **argc** | note the order |
| 0x3a | `OP_CASE` | const index, target | |
| 0x3b | `OP_BREAK` | — | **debugger breakpoint**, not `break` |
| 0x3c | `OP_BIT_SET` | trigger index | **uses `btr` — CLEARS** |
| 0x3d | `OP_BIT_UNSET` | trigger index | **uses `bts` — SETS** |
| 0x3e | `OP_RETURN` | — | |
| 0x3f | `OP_JUMP` | target | absolute byte offset |
| 0x40 | `OP_JUMP_IF` | target | jumps when **true** |
| 0x41 | `OP_JUMP_IF_NOT` | target | jumps when **false** |
| 0x42 | `OP_JUMP_IF_SC_TRUE` | target | short-circuit `\|\|` |
| 0x43 | `OP_JUMP_IF_SC_FALSE` | target | short-circuit `&&` |
| 0x44 | `OP_JUMP_IF_INITED` | static ref, target | the `static` guard (§3.3) |
| 0x45 | `OP_JUMP_IF_BITSET` | trigger index, target | |
| 0x46 | `OP_MARKER` | — | never emitted |
| 0x47 | `OP_SCRIPT_MARKER` | 1 | runtime no-op |
| 0x48 | `OP_ERROR_TOKEN` | — | `write_op` raises a compile error |

**Three traps here, each of which would have produced a silently wrong VM**, and each
caught only by reading the machine rather than the names or the decompiler:

1. **`OP_BIT_SET` clears and `OP_BIT_UNSET` sets.** The handler at `0x009e1022`
   ends `btr eax, edx`; the one at `0x009e105f` ends `bts eax, edx`. The enum names
   are inverted relative to the machine. `crates/don-bhs` has a test named
   `trigger_bit_opcodes_follow_the_machine_not_the_enum_names` specifically so that
   "fixing" this to match the names breaks a test.
2. **`OP_BREAK` is a debugger breakpoint, not a loop `break`.** Its handler calls
   `Breakpoints::break_at(script_file, bip - 1)` (`0x009be380`) and **re-dispatches
   the returned opcode** — the classic patch-the-byte-out breakpoint. Loop `break`
   compiles to `OP_JUMP`.
3. **`OP_JUMP_IF_BITSET` jumps when the bit is *clear*.** `test eax, eax / je` →
   trigger enabled falls *into* the body; disabled jumps *past* it.

### 2.4 The on-disk container

`ScriptFile::read_script_chunk` (`0x009c5440`) switches on a 16-bit chunk tag; the
chunk header is 8 bytes, since `load_bytecode` is handed `*(int*)chunk - 8`.
`[measured]`

| tag | loader | contents |
|---|---|---|
| 0 | — | skip |
| 2 | `load_script_info` `0x009c51c0` | one `Script`: name, params, statics, entry offset |
| 3 | `load_const` `0x009c4fb0` | one constant-pool entry |
| 4 | `load_bytecode` `0x009c50c0` | the raw code bytes |
| 5 | `load_trigger` `0x009c4f50` | a trigger declaration |
| 6 | `load_links` `0x009c5120` | `include` links |
| 7 | `load_line_info` `0x009c4ec0` | line → code offset map |
| 8 | `load_variable` `0x009c4e30` | a variable name record |
| 9 | `load_struct_types` `0x009c4d70` | one process-global struct type name |

Tag 3 has no aggregate form. `ConstType::write` (`0x009db920`) writes only int/enum,
float, and String payloads; `ScriptFile::load_const` (`0x009c4fb0`) compares those same
three tags and executes `int3` for every other value. Arrays and structs are constructed
by `OP_CREATE_ARRAY*`/`OP_CREATE_STRUCT` at runtime, not deserialized from the constant
pool. An actual supported-image capture of the synthetic `array_runtime.bhs` fixture
confirms the compiler side: `[1, 2, 3]` emits `OP_CREATE_ARRAY_INITER`, while its constant
pool is four scalar `ScriptInt`s (`1, 2, 3, 0`), each with `scope=VM_CONST` and
`ref_count=0`. Those scalar ownership fields are sufficient for the normal compiler
producer and channel 15. A custom-struct version returned compile status 2 with no
`ScriptFile` in the bounded oracle environment, so it is retained only as a negative
harness boundary—not evidence for a struct constant encoding.

`don_bhs::chunk` now parses the exact pointer-free subset: the tag-0 root and tags
2–9, including UTF-16 Strings and loader-created channel-15 metadata.
`load_program_files` resolves non-empty tag-6 links by scanning already-loaded source
names backward, matching `ScriptFile::find_script_file` (`0x009c6a10`); the VM then
resolves `OP_CALL_INCLUDE` through that table. `StructType::write` (`0x009db230`)
writes only the declaration name at `StructType+0x10` into each tag-9 leaf.
`load_struct_types` registers it through `ScriptGameInterfaceBase::add_new_type`
(`0x009d5250`), which preserves first insertion order and deduplicates names by
case-insensitive full-string equality. It does **not** serialize field layout, the
schema-signature tag, or aliases. The registry is process-global and is not part of
`RunTimeEnv::walk_data`/channel 15. The loaded temporary `String` has both cached hash
words zero, and `add_new_type` copies them unchanged; `get_type_name` compares the
cached insensitive word directly rather than deriving a struct identity from the
name. Malformed sizes, child counts, ordering, trailing payload, and the unresolved
locale-sensitive non-ASCII comparison are rejected.

### 2.5 The free disassembly channel

`OpCode::write_code` (`0x009c2d90`) takes a `String` directory argument, and **if it
is non-empty it opens `<dir>/bhs.log` and writes a symbolic listing** of every
instruction: `"%d - %d %s%s\n"` = source line, code offset, opcode name, argument
text, with a per-script banner. Opcode names come from the static `String` array
`OpCode::op_string` (initialiser at `0x00413670`), reachable directly via
`OpCode::get_op_name(int)` at `0x004cf5d0`.

This is the cheapest imaginable differential test of our decode layer: same names,
same offsets, no simulation state required. `crates/don-bhs/src/disasm.rs` emits a
deliberately similar format for that comparison.

---

## 3. The value model

### 3.1 `ScriptType`

From the PDB, in full: `[measured]`

```
ScriptType  +0  vptr
            +4  int            data_type    // type tag, e.g. 0x57bad = int
            +8  ScriptScope    scope        // 1 = temporary, 3 = owned by a variable
            +12 unsigned short ref_count
```

vtable: `close, clear, push, get_int, get_float, get_string, get_object, duplicate,
is_false, is_array, get_array_type, do_operator, walk_data, log_data` at +0…+52.

**The single most useful structural fact: every arithmetic, comparison, logical and
assignment opcode funnels through one virtual call, `do_operator(OpCodeTypes op,
ScriptType* rhs)` at vtable slot +44.** The VM loop contains no arithmetic
whatsoever. So the entire operator semantics of the language lives in three or four
`ScriptInt` / `ScriptFloat` / `ScriptString` / `ScriptArray` overrides — a small,
well-bounded differential-testing target.

`scope` is a lifetime tag, not a value: `execute_next` releases an operand (vtable+8,
`push`, which returns it to a `Recycler` pool) exactly when `scope == 1`.
`set_value` promotes to `scope = 3` and bumps `ref_count`. This is not observable in
any value, only in allocation, so `don-bhs` models it with Rust ownership instead.

### 3.2 The type universe is *ten* tags, and the catalogue is lossy

Across all 873 registrations there are exactly ten distinct type tags: `[measured]`

| tag | type | uses |
|---|---|---|
| `0x00057bad` | `int` | 1761 |
| `0x00168174` | `string` | 452 |
| `0x00084048` | `void` | 46 |
| `0x0012f35f` | `real` | 28 |
| `0x0139fd8d` | **`group`** | 20 |
| `0x00153c88` | `array` | 7 |
| `0x0027b2c1` | `anytype` | 6 |
| `0x0020d693` | `string_array` | 4 |
| `0x0064ea2a` | `params` (varargs sentinel) | 1 |
| `0x1d0655f3` | `offer` (CTW) | 1 |

Labels were established by joining the binary table against the *shipped*
`ron-data/paramtypes.xml` and `scriptfunctions.xml` (shipped data files, which the
project's one rule permits as ground truth) — 1,259 independent votes agree that
`0x57bad` is the int family, 218 that `0x168174` is the string family, and so on.

**The 53 `PARAMTYPE` entries are authoring aliases with no runtime existence.**
`who`, `unit_o`, `object_o`, `x`, `y`, `dist_radius`, `int_return` are all
`0x57bad` — plain `int` — to the engine.

One place the catalogue is actively **wrong**: all 18 catalogued uses of the `group`
tag are typed `who` in the XML, but the engine gives unit groups a distinct tag.
Anything that trusts the XML's type here will mis-model group parameters.

The tags are `String::generate_hash` values (`0x00a1b6b0`, case-insensitive, driven
off a 50-entry table at `0xb14500`), stored in `SymType+8` by
`SymTable::add_data_type`. We did not reimplement the hash — the ten tags are
enumerable and named, so there was no need.

### 3.3 Cross-frame persistence — the mechanism, measured

`Game::do_frame` calls `run_script` **once per frame with zero arguments**, so every
shipped script is a state machine over `static` locals. The mechanism is exact and
worth stating precisely, because it is not an obvious design:

`VirtualMachine::get_value` (`0x004d1010`) decodes a **tagged 32-bit reference**:
`[measured]`, read at the instruction level because the decompiler dropped one of the
two masks:

```
test edx, 0x20000000 ; jne -> const_pool[ref & 0xdfffffff]  (ScriptFile+0x48)
test edx, 0x40000000 ; jne -> static_vars[ref & 0xbfffffff] (Script+0x4c, bound Script+0x40)
otherwise            ->        locals[ref]                  (VM+0x28,     bound VM+0x1c)
```

`Script::static_vars` is at `Script+60`, so its count is `Script+0x40` and its data
pointer `Script+0x4c` — the offsets the disassembly uses, confirming the PDB layout
independently.

**`OP_JUMP_IF_INITED` (0x44) is the `static` initialiser guard.** Handler
`0x009e0fca`: if the static slot index is in range *and the slot pointer is non-null*,
jump past the initialiser; otherwise fall through and run it. A `static int x = 5;`
therefore initialises on the first frame and never again — and the *slot being
non-null* is the entire persistence bit. This is exactly the state that rides
checksum channel 15.

---

## 4. The builtin table — 873, from the binary

**Artifact: `schema/bhs-builtins.json`.**

### 4.1 How it was recovered

Registration is a completely regular push/call pattern that decodes mechanically:
`[measured]`

```
push <arity>            ; ScriptFuncSet::add_new_func(int rettype, wchar_t const* name,
push <handler_va>       ;                             int handler_va, int nparams)  @0x009d4da0
push <name_ptr>
push <rettype_tag>
mov  ecx, <funcset>
call 0x9d4da0           -> eax = ScriptFunc*
  ; then, per parameter:
  push ecx / push 0 / push 0 / push <name_ptr> / push <type_tag>
  mov ecx, <ScriptFunc*>
  call 0x9d4f20         ; ScriptFunc::add_param(int, wchar_t const*, wchar_t const*, int, wchar_t const*)
```

`ScriptGameInterface::init` (`0x009e1a20`) constructs the five `FuncSet`s in a fixed
order, and each records `begin_func = script_game_interface->funcs.count` before
calling its `init_funcs`, so **registration order is the index space**:

| FuncSet | first index | count |
|---|---|---|
| `MathUtilFuncSet` | 0 | 15 |
| `TriggerUtilFuncSet` | 15 | 3 |
| `StringUtilFuncSet` | 18 | 6 |
| `ArrayUtilFuncSet` | 24 | 7 |
| `ScenarioFuncSet` | 31 | 842 |

**Decode integrity, all four checks clean:** 873 registrations, **0 unnamed**,
**0 arity mismatches** (declared arity equals `add_param` count for every single
one), **0 stray calls** inside the five `init_funcs` bodies. A dirty decode would
have shown up in all four.

### 4.2 What the catalogue gets wrong

- **813 distinct names, 873 registrations, 36 overloaded names** (60 extra
  registrations). `ScriptFunc::next_overload` exists for exactly this. Examples:
  `set_no_attack` has four arities (2–5); `ping_group` has three, one per parameter
  type (`int`, `string`, `group`).
- **213 registrations are absent from `ron-data/scriptfunctions.xml`**, including the
  two that turn the live game from an observatory into an experiment:
  `set_object_type_attack` (index **532**, handler `0x009f6170`) and
  `set_object_type_armor` (index **531**, handler `0x009f5fb0`).
- **7 catalogued names do not exist in this build**: `call`, `debug_break`,
  `debug_print`, `is_int`, `is_real`, `is_script_loaded`, `is_string`. A script
  calling any of them will not compile.
- **The catalogue's `func_index` values are meaningless for this build.** Use the
  index in `schema/bhs-builtins.json`.

### 4.3 Call convention

`VirtualMachine::call_func(int nargs, int func_index)` (`0x009e0550`): `[measured]`

- Arguments are already on the **shared run stack**, pushed left to right.
- `nargs < 0` — which is how `OP_CALL_GAME` encodes "default" — is replaced by
  `ScriptFunc::params.count`.
- The handler receives `ScriptParamStack { Stack* stack; int num_params; int
  orig_stack_size; }` and `ScriptFuncSet::call_func` (`0x009d5500`) validates and
  converts each parameter via `get_param`, stopping at the `0x64ea2a` varargs
  sentinel.
- **The return value is pushed back only if `return_type != 0x84048` (void).** The VM
  then asserts the stack landed at `orig - nargs + (ret ? 1 : 0)` and raises
  `run_time_error("wrong number of params")` otherwise.
- A rejected call substitutes `ScriptFuncSet::get_err_return` (`0x009d41d0`).

---

## 5. The Rust VM — `crates/don-bhs/`

New workspace member; builds on arm64, so it is **not** excluded the way `oracle` is.

```
crates/don-bhs/
  src/opcode.rs         73 opcodes, operand kinds, handler VAs
  src/value.rs          ScriptType value model + the ten type tags
  src/ops.rs            do_operator — the one virtual call everything funnels through
  src/program.rs        ScriptFile / Script, incl. statics and trigger bits
  src/vm.rs             the fetch/dispatch loop and frame machinery
  src/host.rs           the simulation boundary + Coverage
  src/builtin_table.rs  generated: all 873 declarations
  src/disasm.rs         disassembler + a small assembler for tests
  tests/vm.rs           behaviour tests
```

Implemented and tested: the whole scalar core — push/pop, all 19 binary operators,
the assignment family, unary ops, pre/post increment with the `OP_POP` peek, `OP_INIT`
/ `OP_INIT_COPY`, `OP_CAST` / `OP_CAST_BOOL`, all six jumps including both
short-circuit forms, `OP_CASE`, `OP_JUMP_IF_INITED`, the trigger-bit ops,
`OP_CALL` / `OP_CALL_INCLUDE` / `OP_CALL_GAME` / `OP_CALL_GAME_VARIED`, `OP_RETURN`
with frame close and stack-balance assertion, and both marker opcodes.

Also implemented: all nine aggregate opcodes, including aliasable array/struct slots,
array growth from a retained prototype, struct fields, and array length reads/writes.
Twelve focused aggregate tests cover the handlers, and compiler-to-VM tests execute
array literals, sized-array mutation, and default-constructed struct field mutation.

### 5.1 Two design decisions worth recording

**The run stack holds references, not copies.** The engine's stack holds
`ScriptType*` — pointers to the variable's own storage — so `OP_PUSH` pushes an alias
and assignment mutates through it. `don-bhs` models a stack entry as
`Slot::Val(Value) | Slot::Ref(VarRef)`. A naive `Vec<Value>` would turn every
assignment into a silent no-op and still pass casual tests.

**Coverage is measured, not guessed.** `Host::call` returns
`Err(HostError::Unimplemented)` for anything a host does not implement. The VM records
`(index, name, count)` in `Coverage` and fails by default with the builtin index and
name. An explicitly lossy `MissingBuiltinPolicy::Survey` substitutes retail's typed
error return only for debt discovery; it is never the execution default.

### 5.2 Builtin coverage today

`don_bhs::call_util` implements 25 of the 31 utility registrations. The exclusions are
explicit: the three VM-owned trigger functions, unrecovered `parse`, and two unresolved
aggregate operations. `rand_int`, `rand_real`, seed reads/writes, and string printing are
routed through the host; in `don-sim` this is the main `World::random` stream, never a
private script RNG.

`don-sim::script_runtime::ScenarioHost` is now mandatory for every step-4 execution.
The normal source compiler produces a `Program`, the chunk loader produces the same
`Program`, and `ScriptRuntime` runs either producer against that live host. Forty-three
`ScenarioFuncSet` registrations have exact executable bodies:

| index | builtin | recovered state/action |
|---:|---|---|
| 77 | `set_timer` | `Game::seconds + duration`; replace by case-insensitive name; 100-entry pre-replacement cap |
| 78 | `stop_timer` | remove by case-insensitive name; return 1 or -1 |
| 79 | `timer_expired` | compare against `Game::seconds`; expired checks consume the timer |
| 80 | `get_map_size` | `WorldData::xs << 2` (`0x009e4cb0`) |
| 83 | `map_is_land` | tile bounds, then the exact `tdata` mask (`0x009e4d90`) |
| 84 | `map_is_passable` | tile bounds, then containing `WData::flags & 0x70 == 0` (`0x009e4de0`) |
| 85 | `map_is_buildable` | tile bounds, then reject mountain, forest, ocean, rocks, or bit `0x40` (`0x009e4e50`) |
| 86 | `world_x_size` | direct `WorldData::tile_xs` read at `+0x18` (`0x009e4ee0`) |
| 87 | `world_y_size` | direct `WorldData::tile_ys` read at `+0x1c` (`0x009e4ef0`) |
| 88 | `territory_owner` | tile bounds, then the containing WData `who` byte plus one (`0x009e4f00`) |
| 142 | `num_players` | count `Leader::flags & 1` across the eight slots (`0x009e5df0`) |
| 245 | `population` | active Leader's live control total at `+0x940` (`0x009e8e70`) |
| 246 | `population_cap` | active Leader's direct `pop_cap` field at `+0x7e4` (`0x009e8eb0`) |
| 248 | `age` | both Leader flag bits, then decoded `LeaderDataEncrypt+0xdc` (`0x009e8f50`) |
| 249 | `score` | active Leader's direct score field at `+0x18` (`0x009e8fa0`) |
| 250 | `get_territory` | active Leader's owned-tile count times 100 divided by `WorldData::land_size` (`0x009e8fe0`) |
| 252 | `is_defeated` | bit 6 of the active Leader's low flags byte (`0x009e9070`) |
| 253 | `gather_rate` | active Leader's displayed primary-resource income divided by 16 (`0x009e90b0`) |
| 256 | `get_starting_loc_x` | in-game Leader and start-array bounds, then `start_x[who] << 2` (`0x009e9250`) |
| 257 | `get_starting_loc_y` | in-game Leader and start-array bounds, then `start_y[who] << 2` (`0x009e92a0`) |
| 267 | `get_last_unit_built` | in-game Leader's last completed unit object id (`0x009e9a70`) |
| 270 | `num_buildings` | sum all 129 `unsigned short` building counters at `+0x555e` (`0x009e9bf0`) |
| 273 | `num_units` | sum all 352 `unsigned short` unit counters at `+0x5762` (`0x009e9d60`) |
| 296 | `time` | signed `Game::seconds / 60` (`0x009ead00`) |
| 297 | `time_min` | instruction-identical alias of `time` (`0x009ead20`) |
| 298 | `time_sec` | direct `Game::seconds` read (`0x009ead40`) |
| 351 | `time_later_than` | signed `Game::seconds / 60 >= argument` (`0x009ee120`) |
| 352 | `time_earlier_than` | signed `Game::seconds / 60 < argument` (`0x009ee160`) |
| 411 | `object_position_x` | validate `(who,o)`, resolve captain and outer container, then `div_3_table[(x ^ 0x63637) >> 6]` (`0x009f1360`) |
| 412 | `object_position_y` | the same object walk over the encrypted y coordinate (`0x009f1470`) |
| 446 | `is_garrisoned` | exact object validation and `get_inside` walk; true only for an outer building container (`0x009f2f20`) |
| 525 | `object_health` | valid object, unit-captain `total_damage` or building `hits_left`, divided by dynamic `hits(0)` as an f32 percent (`0x009f5910`) |
| 530 | `object_max_health` | valid object followed directly by dynamic `hits(0)`, without captain redirection (`0x009f5f60`) |
| 583 | `is_idle` | no current order and the originally addressed unit is a captain (`0x009f9370`) |
| 592 | `has_move_order` | current order's virtual `is_move`; exact seven-class override set (`0x009f9840`) |
| 661 | `give_good` | wrapping add to one of the six decoded stockpiles (`0x009fb590`) |
| 662 | `take_good` | wrapping subtract from decoded stockpile, then clamp at zero (`0x009fb630`) |
| 663 | `set_good` | non-negative replacement of one decoded stockpile (`0x009fb6f0`) |
| 669 | `set_base_rate` | `num << 4` at `LeaderData+0x4b0`, the live gather extra-income term (`0x009fbb80`) |
| 670 | `get_base_rate` | signed `/16` readback of that same live base-income term (`0x009fbc20`) |
| 706 | `have_alliance` | both Leaders active, then directed diplomacy slot equals 2 (`0x009fcf50`) |
| 707 | `have_peace` | directed diplomacy slot is peace or alliance (`0x009fcfc0`) |
| 708 | `have_war` | directed diplomacy slot equals war (`0x009fd040`) |

The current 363-file census contains 7,807 calls to those forty-three registrations. Together
with the 791 calls already covered by utility builtins, the strict runtime now handles
8,598 of 39,957 measured shipped-corpus call sites (**21.52%**, up from **1.98%**).
That is reachability coverage, not a claim that any complete retail scenario runs yet.

The unit-status cohort contributes 58 shipped calls. Its object gate is the exact
`valid_object_o` body at `0x009e32a0`; missing object/container links fail closed. The
idle reader deliberately tests the originally addressed unit's `o_up` rather than resolving
its captain. `has_move_order` follows the current order's vtable `+0x14`: the recovered order
vtables point the seven movement classes to the shared true stub at `0x0041e0e0` and all
others to the false base implementation.

The health pair contributes 195 shipped calls. `object_health` is a percentage reader,
not an absolute-hit-point reader: retail converts current and maximum to `f32`, divides,
multiplies by 100, then applies `cvttss2si`. A unit address is first redirected through
`UnitData::get_captain`; the current numerator subtracts `UnitData::total_damage`, including
live `o_down` members, sixteenth-damage carry, and the missing-member charge derived from
the installed type's `uber_size`. `object_max_health` deliberately does not redirect the
addressed subordinate. Buildings use `BuildData::hits(0)`, so an unfinished site exposes
`construct_hits` and a live razing pseudo-item applies the existing f32 queue-progress
interpolation before either query. Missing unit-type `uber_size`, razing train time, broken
formation links, and cycles fail closed. The ordinary-source and loaded-chunk fixture tests
cover all three distinctions: captain aggregation versus direct subordinate maximum,
construction-scaled health, and active razing-scaled health.

Timer storage follows the PDB's `ScriptTimers : LinkList<String,int>` and the shipped
`add_timer` / `remove_timer` / `check` bodies at `0x00a049e0`, `0x00a04b20`, and
`0x00a04b80`. The ASCII timer-name subdomain covers the shipped reachable IDs; a
non-ASCII case fold remains fail-closed because retail delegates it to locale-sensitive
`_wcsicmp`.

`crates/don-sim/tests/fixtures/scenario_runtime.bhs` is compiled through `don-bhs-cc`
and run through the real tick. It proves non-vacuous bytecode work, shared map/clock/age
queries, resource mutations that survive step 8, a live base-rate write, case-insensitive
timer stop, delayed expiry, and consume-on-expiry. Changing the timer's comparison,
removal, or tick placement changes the asserted world stockpile. A second focused test
builds the recovered tag-0/tag-2/tag-3/tag-4 retail container, loads it through
`don_bhs::chunk::load_program`, requires its channel-15 walk metadata, and executes its
`give_good` call through the same mandatory host.

The position pair is the highest-frequency coherent read-only cohort whose required state
already has an authoritative simulation owner: 717 shipped calls apiece. The bodies were
recovered from `0x009f1360` / `0x009f1470`, with their shared
`active_unit_slot` (`0x009e3160`), `valid_build_o` (`0x009e3300`),
`UnitData::get_captain` (`0x00610ab0`), and `ObjectData::get_inside`
(`0x00651a80`) callees checked independently. The host follows owner-local object bands,
formation captain links, and nested `inside_up` links before converting the resolved
world coordinate to a tile. Missing link targets, cycles, and negative coordinates fail
closed instead of producing a synthetic location. Ordinary source executes direct unit
and building reads; a loaded retail chunk proves captain-to-container redirection through
the same handler.

The map-dimension and clock family adds another 407 shipped calls without introducing a
second state owner. `world_x_size` / `world_y_size` are the nine-byte reads of the live
tile dimensions, not the world-cell dimensions used internally by `get_map_size`.
`time` and `time_min` are instruction-identical signed divisions, `time_sec` returns the
unscaled clock, and `time_earlier_than` is strictly less-than. Both ordinary source and a
tag-0/tag-2/tag-3/tag-4 loaded chunk execute all six handlers against two nonzero clocks.

Four player-state readers add 395 more shipped calls directly from the stores already
driven by the tick. `population_cap` and `score` are direct Leader fields, `num_units`
reproduces retail's paired loop over all 352 unsigned-short counters, and
`have_alliance` deliberately reads the first player's directed diplomacy slot rather
than replacing it with the later victory subsystem's mutual-alliance helper. The source
and loaded-chunk tests cover an asymmetric alliance and the invalid-player `-1` sentinel.

Six disjoint map, economy, and diplomacy handlers add 272 shipped calls while retaining
the same authoritative stores. The map pair bounds tile coordinates before reading the
containing four-tile `WData` cell. `gather_rate` exposes the signed `/16` displayed-income
value; `take_good` preserves wrapping subtraction, the zero clamp, and negative-amount
addition. Peace and war remain directed predicates, with alliance also satisfying peace.
Source and loaded-chunk tests exercise rock that is passable but not buildable, negative
income truncation, both stockpile edges, and asymmetric diplomacy.

Five live player/map readbacks add 126 shipped calls without adding a second state owner.
`population` reads the control total maintained by live production; the starting-location
pair preserves retail's weaker one-bit in-game gate, independent x/y array-length checks,
and wrapping WCoord-to-tile shift. `get_last_unit_built` reads the object id published by
queue completion, while `get_base_rate` is the signed readback paired with `set_base_rate`
over the same gather term. Source and loaded-chunk tests cover both one-bit and two-bit Leader
gates, negative-rate truncation, and the absent-start-array `-1` sentinel.

Three territory/building readers add 96 shipped calls from checksum-owned live state.
`territory_owner` reads the signed owner byte from the containing WData cell after tile
bounds; `get_territory` preserves the wrapping multiply and signed land-size division;
`num_buildings` sums all 129 unsigned-short counters rather than a sampled type range.
Source and loaded-chunk tests cover the script one-based owner convention, map and player
sentinels, signed integer truncation, the two-bit active gate, and both ends of the counter
array.

The formal `scenario_runtime` closure row remains **required/incomplete**. The remaining
799 scenario registrations are still hard failures; notably `get_difficulty` lacks an
authoritative game/scenario difficulty owner and `num_cities` lacks the live
`LeaderData::city_num` field. They are not synthesized from nearby state.

---

## 6. Differential testing — the design

The same shape as the damage-pipeline validation, in three layers of increasing cost.
None is finished; the first is nearly free once §7 lands.

**Layer 1 — decode, against `bhs.log`. No simulation state needed.**
Retail's `OpCode::write_code` emits a symbolic listing when handed a non-empty
directory (§2.5). Compile a corpus through retail, capture `bhs.log`, and compare it
line-for-line with `don_bhs::disasm::format_all` on the same bytecode: opcode names
and code offsets must match exactly. This tests the opcode table, the operand counts
and the encoding in one shot, and any disagreement localises to a single instruction.
Corpus: the three shipped scripts plus generated single-construct probes.

**Layer 2 — operator semantics, against `do_operator` under the oracle.**
Because *all* arithmetic funnels through `ScriptType::do_operator` (vtable +44), the
entire operator surface is reachable by calling four vtable slots by RVA on hbox —
no game, no world. For each `(lhs_type, opcode, rhs_type)` triple, sweep operand
values and compare against `don_bhs::ops::do_operator`. This is the layer that
converts `src/ops.rs` from Tier C guesswork to Tier B, and it is the highest-value
unstarted piece. Register the cases in the existing `oracle-regress` harness so they
re-run with one command and a case that cannot run is SKIPPED, never green.

**Layer 3 — whole-VM, against the live process.**
Run a script in the retail game with the `.bhs` bridge (`docs/tooling/bhs-bridge.md`),
have it `print_line` a trace of `(frame, static values)` each frame, then run the same
bytecode in `don-bhs` with a `Host` replaying the same builtin return values captured
from that trace, and require the static-variable vector to match frame for frame. The
strong form is comparing `RunTimeEnv::bytecodes_executed` (`RunTimeEnv+40`), which is
a single integer that diverges on the *first* control-flow disagreement — a far
sharper signal than comparing final state.

Always with sample count and input distribution recorded. Layer 2 is testing, not
verification, and must never be described as proving anything.

---

## 7. Driving the retail compiler — status

A recovered hbox oracle harness now exercises this route from the following ground
truth: `script_compiler` at `0x00eb6a90`, `Compiler::compile` at `0x009bf160`
(returns 0 on success), `Compiler::eval_command` at `0x009befe0`,
`ScriptGameInterface::init` at `0x009e1a20`, `real_script_game_interface` at
`0x00eb1ac8`, `script_game_interface` at `0x00cab36c`,
`ScriptFile::script_files` at `0x00c8cba0`, and the `bhs.log` channel of §2.5. The
harness maps retail, runs 1,115 of 1,117 process initializers cleanly, installs the
fabricated environment, and initializes both `ScriptGameInterface` and the compiler.
The five scalar keyword slots read from the retail `internal_strings.xml` are now
installed as a table separate from translated diagnostics: `6114=string`, `6115=int`,
`6966=float`, `7192=void`, `7193=bool` (plus `7177=UseBytecodeDump`). A labels-only file
returns success from retail `Compiler::compile` and creates a `ScriptFile`; no script body
has yet compiled, so reference bytecode and byte-identity comparison remain open.

The current dynamic blocker is precise rather than silent: the half-built environment has
not registered the game-specific `scenario` / `conquest` script qualifiers. A bounded
single-step run snapshots live retail `String` objects in the SIGTRAP handler before their
stack temporaries are destroyed. For `scenario {}`, it records `"{"` in both the lexer and
`yyerror`, followed by diagnostic slot 3739 in `Compiler::comp_error`. That proves the
parser consumed `scenario` as an unresolved identifier and failed at the following brace;
it is not an initializer crash or a missing scalar keyword.

The escalation ladder it was given, cheapest first: (1) call
`OpCode::get_op_name(0..73)` and recover the 73 mnemonics as an independent
confirmation of the table; (2) run the MSVC `.CRT$XC*` dynamic initialisers the
script module needs; (3) call `ScriptGameInterface::init` to build the builtin table;
(4) `Compiler::compile` on a trivial file; (5) extract `ScriptFile::code`.

Known hazards, stated in advance: the compiler is **not an island** — it allocates,
touches `script_game_interface`, and name resolution scans the registered table, so
step 3 gates step 4. The `StringTable` pointer at `[0x00C06378]` is the usual fault
site on this codebase. `install_fake_teb()` already exists in the oracle's
`image.rs`.

If the in-process route proves intractable, the fallback is not bad: the retail game
in the Parallels VM compiles `.bhs` at every game start, and `bhs.log` is written by
the shipped code path — so a compiled corpus may be obtainable by arranging for a
non-empty directory argument rather than by hosting the compiler at all.

---

## 8. Open questions, ordered by how much they block

1. **Compiler byte identity, including assignment operand order.** The assignment handler (`0x009e10e9`) makes the
   *first-popped* operand the `this` of `do_operator` and pushes it back, while the
   binary-operator handler (`0x009e11e5`) makes the *second-popped* operand `this`.
   Both readings of the compiler's push order are self-consistent, and they differ
   only when the two sides have different types (which side's `do_operator` runs).
   `don-bhs` takes "first popped is the assignment target", the idiomatic reading.
   **One compiled `a = 1;` settles it instantly** — this is the first thing to check
   the moment §7 produces bytecode.
2. **String and aggregate operator differentials.** Integer and float operators are
   Tier B after 6,993 retail cases with zero mismatches. Retail comparisons for
   strings and aggregates, including `ScriptObject::get_string`, remain open.
3. **Register game script qualifiers in the hbox compiler environment.** This is now the
   direct blocker to compiling a body and extracting reference bytecode.

The compiled-constant question is closed rather than deferred: the shipped tag-3
writer/reader pair has no aggregate arm, and the supported-image array capture constructs
the aggregate in bytecode from scalar constants. Non-scalar tag-3 leaves therefore remain
an exact retail assertion boundary. Tag 9 still supplies only a global type name, so no
layout or alias relation is inferred from it.

Two formerly open fields are decoded from the retail writer/loader pair. Despite its name,
`LocalScriptType::write` (`0x009da900`, store at `0x009daa91`) serializes literal zero to
`Script::script_type` for `ai`, `scenario`, and `conquest`; those words remain compile-time
strings at `LocalScriptType+0x7c`, not runtime IDs. Each parameter is serialized as two
dwords `[SymType.type, is_ref]`, where the second is exactly `0` or `1`; the loader at
`0x009c51c0` restores the first into `Script::params` and compacts the second's low byte
into `Script::refs`; both tables are retained by the recovered program image. The
default-type grammar reduction loads root `int` for omitted parameter, variable, and
return types. Concrete array tags hash `"@" + element.get_name()`. Struct tags and `OP_CREATE_STRUCT`'s second
operand hash the full unique token
`<StructName>$<field1 display-type>^<field2 display-type>^...`, not the bare struct name;
a fixed array field contributes `T[]^` without its bound. The caller-side lowering that
preserves a mutable alias is still open, so the VM now refuses a script with any set ref
bit instead of approximating it as pass-by-value.

---

## 9. Provenance

Every address is from `ron-bin/riseofnations.exe`, named via `ron-bin/sbl/rise.pdb`.
Structure layouts and the opcode enum are read directly out of the PDB type stream
(`llvm-pdbutil dump --types`). Dispatch tables, operand counts and the three traps in
§2.3 are read at the instruction level with capstone `CS_MODE_32`, because the
decompiled C was demonstrably lossy in at least two places (`get_value`'s
`0xbfffffff` mask, and the operand aliasing in the assignment handler). The builtin
table is a mechanical decode of the five `init_funcs` bodies; type-tag *labels* are
joined against the shipped `ron-data/paramtypes.xml` and `scriptfunctions.xml`.

**No fidelity claim is made here.** Nothing in this document or in `crates/don-bhs`
has been executed against retail. The crate's green test suite is evidence that the
implementation matches *our reading* of the engine, not that the reading is correct.
Nothing here is verified, proven, or a refinement.
