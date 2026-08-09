# BHS compiler

**363/363 shipped scripts parse. 363/363 compile to bytecode with zero errors. Zero
decode failures across 313,880 emitted instructions.**

The corpus is `ron-data/bhs-corpus/` — 363 `.bhs` files, 93,649 lines, written by Big Huge
Games across the `ai/`, `conquest/` and `scenario/` trees. It is this lane's language
specification and its test suite at once.

Crate: `crates/don-bhs-cc/`. Grammar: `docs/tracks/bhs-grammar.md`. Sibling lanes:
`crates/don-bhs/` (VM, opcode table, builtin table) and the retail-compiler-under-oracle
lane.

```sh
cargo run -p don-bhs-cc --bin bhsc -- parse   ron-data/bhs-corpus
cargo run -p don-bhs-cc --bin bhsc -- compile ron-data/bhs-corpus
cargo run -p don-bhs-cc --bin bhsc -- stats   ron-data/bhs-corpus
cargo test -p don-bhs-cc          # includes the corpus gate; skips loudly if absent
```

---

## The question that decided this lane's freedom

> *Does `script_run_time` hash VM internals?*

**No.** `RunTimeEnv::walk_data` (`0x009c41a0`) is checksum channel 15, and it was read at
the instruction level. It calls `RunTimeEnv::close` **first**, which frees the frame stack
and the operand stack and zeroes `cur_vm`, `bytecodes_executed`, `err_count` and
`script_status`; `VirtualMachine` has **no `walk_data` at all** and no walker reaches a
`VirtualMachine*`. The function never even reads `this` — `CheckSums::check_all` passes it
garbage in `ECX`. Program counter, operand stack, locals and instruction count are
invisible to the checksum.

**But the compiled image is hashed.** `ScriptFile::walk_data` (`0x009c63b0`) walks `code`
— the literal bytecode byte array — plus `const_pool` and `linked_files`.
`Script::walk_data` (`0x009c5f30`) walks `static_vars`, `trigger_bits`, `params`, `refs`,
`trigger_names`, `var_names`, `static_var_names`, `name`, `offset`, `return_type` and
`script_type`, and container capacity/grow metadata along with them.

So the channel decomposes into a **constant prefix** fixed at load (the whole compiled
image) and a **mutable part** that moves frame to frame (`static_vars` and
`trigger_bits`). Two consequences, and they point in opposite directions:

- **Execution is free.** Any instruction sequence reaching the same script-visible state
  hashes identically. We are compiling, not transcribing.
- **The image is not free.** Reproducing retail's channel-15 word for a scripted match
  requires byte-identical output, including per-script `offset` and the array capacity and
  grow values retail's `ScriptFile::load_*` happens to produce.

This lane therefore aims at retail's **opcode set and encoding** — which is free, because
the sibling engine lane already recovered them whole — and treats byte-identity as a
**measurable** goal that needs reference bytecode to diff against. Nothing here claims to
have met it. The cheap alternative remains open: load retail's compiled bytecode for the
channel and use this compiler for everything else.

---

## What was built

| module | what |
|---|---|
| `lex.rs` | tokens, `$S`, adjacent-string concatenation, Latin-1 fallback |
| `ast.rs` | the AST, shaped by the corpus rather than by design |
| `parse.rs` | recursive descent + precedence climbing |
| `sema.rs` | `include` closure, struct layout, `labels` evaluation, script table, builtin resolution |
| `codegen.rs` | emission to the engine's `OpCodeTypes` |
| `bin/bhsc.rs` | `parse` / `compile` / `stats` drivers |
| `tests/corpus.rs` | the corpus gate |

There is **no invented instruction set**. Every byte emitted is an `OpCodeTypes` value from
`ron-bin/sbl/rise.pdb` (`LF_ENUM 0x3c85`, 73 enumerators), encoded as
`RunTimeEnv::exec` fetches it: one opcode byte, then 0/1/2 unaligned little-endian 32-bit
operands. Opcode semantics come from `don_bhs::vm`, which mirrors
`VirtualMachine::execute_next` handler by handler.

### Corpus census (AST, not grep)

```
files 363 · include 232 · labels 4 file / 195 statement · struct 3
script decl 148 / def 230 · anonymous entry 319 · file-scope var 0
statements 76,435
  if 10,596 · else 4,991 · for 1,442 · while 88 · do-while 208
  switch 336 · case 1,574 · default 168 · break 1,906 · continue 0 · return 793
  decl 4,830 (2,433 static) · trigger 1,554 · run_once 182
calls 42,417 · method calls 492 · casts 140 · array literals 448 · $S 2,927
ref params 75 · untyped params 51
```

The census is a falsifiability check, not decoration. It cross-checks against greps of the
same corpus once comments and string literals are blanked, and the two places it did
*not* agree turned out to be corrections to the brief — see "Things the brief had wrong"
below.

### Emitted code

```
557 script slots (549 with bodies; the rest are forward declarations)
1,280,240 bytes · 313,880 instructions · 47 of 73 opcodes used
0 decode failures · 0 out-of-range jump targets · 33 auto-casts · 14,356 implicit declarations
```

The counts are self-checking. `OP_BIT_UNSET` 1,635 is exactly the corpus's
`enable_trigger` count and `OP_BIT_SET` 436 its `disable_trigger` count; `OP_CASE` 1,574 is
the `case` count; `OP_JUMP_IF_BITSET` 1,554 is the trigger count; `OP_JUMP_IF_INITED` 2,615
is 2,433 statics + 182 `run_once`; `OP_RETURN` 1,342 is 549 bodies + 793 explicit
`return`s; `OP_CAST` 173 is 140 written casts + 33 inserted ones. Any of those could have
disagreed.

Top of the histogram: `OP_PUSH` 151,176 · `OP_CALL_GAME` 39,896 · `OP_POP` 39,269 ·
`OP_ASSIGN` 14,332 · `OP_JUMP_IF_NOT` 13,537 · `OP_JUMP` 8,763 · `OP_EQ_OP` 5,308 ·
`OP_INIT` 4,999 · `OP_JUMP_IF_INITED` 2,615 · `OP_JUMP_IF_SC_FALSE` 2,440.

Never emitted (26): `OP_AND_OP` `OP_OR_OP` `OP_MUL_ASSIGN` `OP_DEC_OP` `OP_POW_OP`
`OP_POW_ASSIGN` `OP_MOD_ASSIGN` `OP_LEFT_ASSIGN` `OP_RIGHT_ASSIGN` `OP_AND_ASSIGN`
`OP_XOR_ASSIGN` `OP_OR_ASSIGN` `OP_LEFT_OP` `OP_RIGHT_OP` `OP_AND_BIT` `OP_XOR_BIT`
`OP_OR_BIT` `OP_UNA_TILD` `OP_CREATE_ARRAY_DYN` `OP_CREATE_ARRAY_INDEX`
`OP_SET_ARRAY_LENGTH` `OP_CAST_BOOL` `OP_BREAK` `OP_MARKER` `OP_SCRIPT_MARKER`
`OP_ERROR_TOKEN`. Most are simply unused by the corpus; `OP_AND_OP`/`OP_OR_OP` are an open
question (below), and `OP_MARKER`/`OP_BREAK` are by design (a marker emits no bytes;
`OP_BREAK` is the debugger's patched breakpoint).

---

## Findings

### 1. `enable_trigger` and `disable_trigger` are compiler keywords, not builtins

The corpus calls them 2,081 times. They are in **neither** the engine's 873 registered
builtins **nor** `ron-data/scriptfunctions.xml`. They are flex rules 19 and 20 (tokens
`0x12c`, `0x12d`) and lower to the only two opcodes that touch `Script::trigger_bits`:
`OP_BIT_UNSET` (0x3d, `bts` — sets) and `OP_BIT_SET` (0x3c, `btr` — clears), whose operand
is a literal trigger index. Meanwhile `is_trigger_enabled(String)` **is** builtin 17,
because a runtime by-name lookup needs `Script::trigger_names`.

This was found by compiling: it was the *only* unresolved-name error left after builtin
resolution, 2,081 of 2,081. The argument may be a bare identifier —
`disable_trigger(pause_time_up)` in `scenario/Scripts/Auto_Pause/auto_pause.bhs` — which is
not a variable anywhere and settles it beyond doubt.

**Triggers start enabled**, on three independent corpus arguments: 146 `disable_trigger`
calls sit inside `run_once` initialisation blocks (meaningless otherwise); 31 triggers are
disabled and never enabled (permanently dead otherwise); and `auto_pause.bhs` disables one
in `run_once` and enables it later.

### 2. Adjacent string literals concatenate, and that is a shipped bug

`scenario/Custom/italy_mp/italy_mp.bhs:334-336` assigns one string across three quoted
fragments with no operator. That settles `scenario/scriptlibrary/ctw_lib.bhs:326`, which
reads `["House D" "House D1", "House D2", …]` — a **missing comma**. Element 0 compiles to
`"House DHouse D1"` and the New World house list is one shorter than intended. A faithful
compiler reproduces that; it does not insert the comma the author meant.

### 3. A second shipped typo: `.lenght`

`conquest/Napoleon/napoleon_diplo.bhs:182` reads
`for (z = 0; z < offer.tribe_terr.lenght; z++)`. It is the **single** warning our compiler
emits across the whole corpus. What retail does with it is unresolved, and the two
possibilities are far apart: either its compiler is lax here, or `napoleon_diplo.bhs`
**never compiled in the shipped game**, which per `bhs-what-we-know.md` fails almost
silently and would have disabled the Napoleon diplomacy script for the whole session. That
is a ten-minute experiment in the live game and worth doing.

### 4. Things the brief had wrong

The lane brief's keyword frequencies were raw greps and counted comments and strings.
Blanking those:

| brief | actual in code |
|---|---|
| `and` 845, `or` 138, `not` 394 | **0, 0, 0** — BHS has no word operators, and the retail flex DFA has no rule for any of them |
| `continue` 15 | **0** statements |
| `const` 11 | **0** — only inside `/* */` |
| `if` 11,129 / `for` 1,906 / `switch` 374 | 10,596 / 1,442 / 336 |

`ref` 75, `struct` 3 and `include` 232/233 matched. A parser that accepts `a and b` accepts
programs retail rejects, so this correction is in the parser, not just the doc.

### 5. `labels` auto-numbers from 1

Ten shipped blocks mix implicit and explicit entries, and every one is a player-number
table whose leading implicit entries are the playable sides. `skirmishsetup.bhs` puts
`labels { ATTACKER, DEFENDER }` immediately above `for (i = 1; i < 3; i++) gain_tech(i, …)`
granting the same two sides the techs later granted to `ATTACKER` and `DEFENDER`.
Zero-based numbering would make every such table address RoN player 0, which is Gaia.
[inferred from the corpus; one `labels` block under the retail compiler would make it
measured]

### 6. Implicit declaration is a grammar rule, and the variable is an `int`

14,356 variables in the corpus are used with no declaration. This is not error recovery:
bison rule 92 is an **empty type-specifier** whose action loads the built-in `int`
`SymType` from `[0x00ebe408]+0x00`, and rule 93 calls `SymTable::create_var_type`. Driving
the shipped LALR tables on `scenario { run_once { NAME = INT ; } }` reduces 92 then 93 and
accepts. `VarType::init` (`0x009da870`) then picks storage by position: a frame **local**
inside a body, a file global at file scope, a file static when written `static`. Every
shipped implicit declaration is inside a body.

This matters for the checksum: had they been statics, 14,356 slots would persist across
frames and land in channel 15. They do not.

### 7. Integer semantics that must not be taken from Rust by accident

From `ScriptInt::do_operator` (`0x009d7760`), read at the instruction level:

- `/` is `idiv` — truncation toward zero, **same as Rust**. Divide by zero calls
  `run_time_error(L"Can't Divide 0")` and yields **0**, then execution continues.
  `INT_MIN / -1` is unguarded and traps.
- `%` is the same `idiv`'s remainder — sign of the dividend, **same as Rust**.
- `>>` is `sar` — arithmetic, **not** logical.
- `**` is `cvtdq2ps` → CRT `powf` in **binary32** → `cvttss2si`. This is one of the seven
  non-IEEE transcendental hazards from `README-LLM.md`, sitting inside an integer
  operator.
- `!x` is `x <= 0`, **not** `x == 0`. `&&`/`||` on ints are `x > 0`.
- There is **no runtime coercion**: a type mismatch errors and leaves the left operand
  unchanged. `ScriptString`'s `+` is concatenation and `+=` appends in place.

These belong to `don-bhs`'s `ops.rs`, not to this crate, but they are recorded here because
they were measured for this lane and because "same as Rust" is exactly the kind of claim
that must be checked rather than assumed.

---

## Evidence, and what it is worth

**Strong.** 363/363 compile with zero errors and zero decode failures over 313,880
instructions, with every jump target bound-checked and every script entry asserted to land
on an instruction boundary. This is not "parses to EOF with no residue" — the parser was
deliberately tightened until it *broke*, and each break was a real language fact:

1. Requiring `;` after every statement dropped the rate to 360/363. Two of the three
   failures were shipped sources missing a semicolon; the third was string concatenation.
   Both are now documented properties rather than accidental permissiveness.
2. Requiring commas in array literals exposed `ctw_lib.bhs:326`.
3. Resolving every call against the 873-registration builtin table left exactly two
   unresolved names, both of which turned out to be keywords.
4. The AST census cross-checks against greps of the same corpus, and its two disagreements
   were corrections to the brief.

**Weak or absent.** No emitted bytecode has been **executed**, by our VM or retail's. No
byte of our output has been compared against retail's compiler. The lowering choices marked
`[inferred]` below are consistent with the opcode semantics but are not the only sequences
that would be. Per `docs/CHARTER.md` this is Tier C throughout: behaviourally motivated,
divergence unmeasured. Nothing in this lane is verified.

---

## Open, in priority order

Each of these is settled by one pass over reference bytecode from the retail-compiler
lane, or by one run under the oracle.

1. **Assignment operand order.** `OP_ASSIGN`'s handler pops two slots and calls
   `do_operator` on the first. `don-bhs`'s VM reads the first-popped as the *target*, so
   this compiler emits value-then-target. **If that convention is wrong, the compiler and
   the VM are wrong together and agree with nothing else** — the failure mode our own tests
   cannot see. Gated behind one constant, `codegen::ASSIGN_EMITS_VALUE_THEN_TARGET`; flip
   it and the matching arm in `don_bhs::vm` together, never one alone.
2. **How retail lowers `&&` and `||`.** We emit `OP_JUMP_IF_SC_TRUE`/`SC_FALSE`, which
   yields the *operand* rather than a normalised 0/1 (Lua semantics, not C). But
   `OP_AND_OP`/`OP_OR_OP` exist and `ScriptInt` implements them as `x > 0`, and the retail
   lexer gives `&`/`&&` a single token. Something has to reconcile those three facts.
3. **Struct and array opcode operands.** `OP_CREATE_STRUCT` and `OP_CREATE_ARRAY_INITER`
   each take two raw 32-bit operands whose meaning is unread; we emit
   `(struct index, field count)` and `(type tag, element count)`. `OP_CREATE_ARRAY` vs
   `OP_CREATE_ARRAY_DYN` and `OP_CREATE_ARRAY_INDEX` vs `OP_PUSH_ARRAY_INDEX` are likewise
   split by inference. `don-bhs`'s VM does not implement these yet either, so nothing
   currently exercises them.
4. **`run_once` lowering.** We use a hidden static plus `OP_JUMP_IF_INITED`. The opcode
   semantics are measured; retail's choice is not. A trigger bit is the obvious
   alternative.
5. **`SyntaxNode::auto_cast`'s rules** (`0x009dfee0`). We insert casts only for
   string-on-the-left, which the corpus needs and which is unambiguous — 33 insertions.
   Int/real mixing is left alone because the corpus writes those casts by hand, which is
   itself evidence the compiler declines them.
6. **`Script::script_type` and `return_type` encodings.** Our `script_type` ids for
   `ai`/`scenario`/`conquest` are ours; `return_type` uses the measured `SymType` tags.
7. **The anonymous entry script's registered name.** We use the file stem.
8. **`labels` starting value** (§5 above).
9. **Container capacity and grow values.** These are checksummed. Even byte-identical code
   will not reproduce channel 15 without matching what retail's loaders allocate — the same
   `Array<T>` hazard `CODEX.md` flags for the rest of the sim.

---

## Interfaces

**Builtins are not ours.** `schema/bhs-builtins.json` and the host boundary belong to a
sibling lane; this crate consumes `don_bhs::builtin_table` read-only and emits
`OP_CALL_GAME` / `OP_CALL_GAME_VARIED` with the registration index. Resolution is by name
then arity, with the varargs tail (`parse`) handled by its `ScriptTy::Params` tag. Nothing
in this crate implements a builtin.

**Output type** is `don_bhs::program::Program` — `ScriptFile` with `code`, `const_pool`,
`scripts`, `linked_file_names`, `line_to_op`; `Script` with `entry`, `arity`,
`return_type`, `script_type`, `var_names`, `static_var_names`, `statics`, `trigger_names`,
`trigger_bits`. Those are the shipped field names and offsets from the PDB, so the VM lane
can run our output and the chunk-file reader can produce the same shape.

**Files this lane owns:** `crates/don-bhs-cc/**`, `docs/tracks/bhs-grammar.md`,
`docs/tracks/bhs-compiler.md`, and one line of the workspace `Cargo.toml` members list.

---

## Next

1. Get one `.bhs` through the retail compiler under the oracle and diff the bytecode. That
   single artefact closes items 1-6 above at once and converts this lane from Tier C to a
   measured comparison.
2. Run compiled corpus output on `don-bhs`'s VM with a stub host and count how far each
   script gets. `don-bhs` already has a coverage recorder for unimplemented builtins, so
   the first run produces a ranked list of what the host boundary owes.
3. Implement the aggregate opcodes in the VM (`0x29`-`0x31`) — **4,267** of our emitted
   instructions are aggregate ops `don_bhs::vm` currently rejects as `Unimplemented`
   (`OP_PUSH_STRUCT_FIELD` 1,746, `OP_PUSH_ARRAY_INDEX` 1,464, `OP_PUSH_ARRAY_LENGTH` 463,
   `OP_CREATE_ARRAY_INITER` 448, `OP_CREATE_STRUCT` 79, `OP_CREATE_ARRAY` 67). Nothing
   scripted runs end to end until they exist.
4. Settle `.lenght` in the live game.
