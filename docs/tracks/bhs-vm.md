# The BHS virtual machine — aggregates, builtins, and the first retail measurement

Lane: `bhs-vm`. Predecessor: `docs/tracks/bhs-engine.md`, which established the
decomposition, lifted the 73-entry opcode enum whole from the PDB, decoded all 873
builtin registrations, and shipped the scalar core of `crates/don-bhs`. This document
covers what came after: the aggregate opcodes, a measured builtin debt list over the
real 363-script corpus, and **the first time any part of this crate was executed
against retail machine code.**

---

## What a human can now do that they could not before

1. **Run BHS code that uses arrays and structs.** All nine aggregate opcodes are
   implemented — `int[] a; a[2] = 7;`, `a.length = n`, `s.field`, `{10,20,30}` — and
   subscripting an aggregate yields an *aliasable slot*, so writes land where retail
   puts them. 12 new tests, each pinned to the handler address it came from.
2. **Trust `don_bhs::ops` for int and float arithmetic.** It was the crate's weakest
   module — written from opcode *names* — and it is now **Tier B: 6,993 cases against
   retail's own `ScriptInt::do_operator` and `ScriptFloat::do_operator`, 100 %
   agreement, mutation-tested.** Reproduce with one command (§4).
3. **Ask what to build next and get a number instead of an opinion.**
   `cargo run -p don-bhs --bin bhs-census` scans all 363 shipped scripts and prints
   the builtin surface ranked by call count, marking what is implemented:
   *363 files, 93,649 lines, 549 of 873 builtins called, 39,957 call sites.*
4. **Stop confusing "retail rejects this" with "we have not written this yet."** The
   VM now separates a `RunTimeEnv::run_time_error` (reported in `RunOutcome::error`,
   with `err_count`, exactly as retail counts it) from a gap in our implementation
   (`VmError`). Those are different claims and were previously the same value.

`cargo test -p don-bhs`: **52 tests green** (22 unit, 12 aggregates, 6 corpus, 11 vm,
1 doctest), up from 21. `cargo check --workspace --all-targets` is clean, including
the sibling `don-bhs-cc` which depends on this crate.

---

## 1. The finding that reorganises the value model

**A BHS struct and a BHS array are the same runtime object.** [measured, `rise.pdb`]

```
ScriptType    sizeof 16   vptr | data_type +4 | scope +8 | ref_count +12
ScriptInt     sizeof 20   : ScriptType,  int   value      +16
ScriptFloat   sizeof 20   : ScriptType,  float value      +16
ScriptString  sizeof 36   : ScriptType,  String value     +16  (curr_len at +24)
ScriptObject  sizeof 44   : ScriptType,  PtrArray<ScriptType*> values +16
ScriptArray   sizeof 48   : ScriptObject, ScriptType*     blank_base  +44
```

`ScriptArray` *derives from* `ScriptObject`. Both keep their contents in the one
`values` pointer array (count at +20, data at +32 — the offsets `0x14`/`0x20` that
every aggregate handler uses). The only differences are `blank_base`, the prototype
element an array duplicates when it grows, and what `is_array()` answers.

That is why `OP_PUSH_STRUCT_FIELD` (0x2f) and `OP_PUSH_ARRAY_INDEX` (0x2d) are the
same instruction with the index coming from the instruction stream instead of the
stack: both compute `push &obj->values[i]`. Modelling structs separately from arrays
would have been more code and wrong.

`VarScope` also came out of the PDB whole: `VM_TEMP = 1, VM_CONST = 2, VM_VAR = 3,
VM_CLEARED = 4, VM_ARRAY = 128`.

### The nine aggregate opcodes, as read

| op | name | handler | behaviour |
|---|---|---|---|
| 0x29 | `OP_CREATE_ARRAY` | `0x009e0aa4` | pop prototype; empty array, `blank_base = prototype` |
| 0x2a | `OP_CREATE_ARRAY_DYN` | `0x009e0ac2` | **size on top**, prototype beneath; array of `size` copies |
| 0x2b | `OP_CREATE_ARRAY_INITER` | `0x009e0b47` | operands `[count][type]`; pops `count` values, `values[0]` is the **top of stack**; `blank_base = values[0]->duplicate()->clear()`; `count == 0` is a runtime error |
| 0x2c | `OP_CREATE_STRUCT` | `0x009e0b67` | operands `[member_count][struct_unique_hash]`, then `ScriptObject::init_struct` `0x009d8410`; the hash token is `<name>$<field display type>^...`; no `blank_base` |
| 0x2d | `OP_PUSH_ARRAY_INDEX` | `0x009e0b87` | rvalue subscript. **Never grows**; `idx >= count` or `idx < 0` is a runtime error. Does *not* check `is_array()` |
| 0x2e | `OP_CREATE_ARRAY_INDEX` | `0x009e0cc3` | lvalue subscript. Checks `is_array()`; past the end it calls `resize_array(idx+1, /*grow_only=*/1)` |
| 0x2f | `OP_PUSH_STRUCT_FIELD` | `0x009e0dc5` | one operand = field index; `idx >= count` is an error; **no negative check** |
| 0x30 | `OP_PUSH_ARRAY_LENGTH` | `0x009e0e95` | fresh `ScriptInt` = `values.count`; on a non-aggregate it errors **and still pushes 0**, so the stack stays balanced |
| 0x31 | `OP_SET_ARRAY_LENGTH` | `0x009e0f0c` | **array on top**, length beneath; `resize_array(n, /*grow_only=*/0)` — this is the only path that can shrink — then **pushes the length back** |

`ScriptArray::resize_array` (`0x009d5cd0`) refuses with a runtime error when
`blank_base` is null, which is exactly how the engine stops you resizing a struct.

Two traps worth stating because they will silently corrupt a code generator:

- **`values[0]` is whatever was on top of the stack.** `init_array` and `init_struct`
  both pop and append in pop order. A compiler emitting an initialiser list left to
  right therefore has to push it **in reverse**, or every field is mirrored.
- **`OP_INIT_COPY` (0x32) calls `duplicate()` and `OP_INIT` (0x33) does not**
  (`call [eax+0x1c]` at `0x009e0f6e`). On a scalar that is invisible. On an aggregate
  it is the difference between two arrays and two names for one array; there is a
  test that fails if the two are made the same.

---

## 2. The open question from §8.1 of the predecessor: settled

> *"Assignment operand order… `don-bhs` takes 'first popped is the assignment target',
> the idiomatic reading. One compiled `a = 1;` settles it instantly."*

It did not need a compiler. The two handlers take their receiver from **opposite
ends**, and reading both at the instruction level is decisive: [measured]

- **Assignment family, `0x009e10e9`.** Two `Stack::pop` calls. The **first** popped
  goes into `ecx` — the `this` of `do_operator` — and is stashed at `[ebp+8]`, which
  the shared epilogue `0x009e0a11` pushes back. So the **assignment target is on top
  of the stack**, the value beneath it, and the value pushed back is the target.
- **Binary operators, `0x009e11e5`.** Two *inline* pops. The **first** popped becomes
  the `rhs` argument, the **second** becomes `ecx`. So the left operand is beneath
  the right — the ordinary stack-machine convention.

`OP_SET_ARRAY_LENGTH` corroborates the first independently: it too takes its
assignment target (the array) from the top and the value from beneath.

The existing implementation was right. It is now right *for a stated reason*, and the
comment that said "both readings are self-consistent" is gone.

---

## 3. What reading `do_operator` changed

`ops.rs` was written from opcode names. Reading the five implementations — the switch
structure from `re/decomp-all/`, and every numeric decision re-checked at the
instruction level because Ghidra reorders exactly this kind of code — produced nine
corrections. Each is now a test that fails if someone "fixes" it back.

1. **There is no type promotion. At all.** Every implementation opens with
   `if (rhs && rhs->data_type != this->data_type) run_time_error(...)`. `int + real`
   is a *runtime error*; the compiler is expected to have inserted `OP_CAST`
   (`SyntaxNode::auto_cast`, `0x009dfee0`). The old code promoted silently.
2. **`is_false` on an int is `value <= 0`**, not `== 0` (`0x004cf230`, `setle`). A
   negative int is FALSE, so `if (-1)` does not run — and since the engine's error
   return for a failed builtin is `-1`, `if (some_failed_call())` never runs either.
   Float is the same rule (`0x004cf360`); an **object or array is always true**
   (`0x004bcab0` is `xor al,al; ret`), empty or not.
3. **`OP_UNA_NOT` on an int is `value < 1`** — the same rule, so `!(-5) == 1`.
4. **`OP_AND_OP` and `OP_OR_OP` on an int ignore the right operand entirely** and
   both return `value > 0`. The real short-circuit logic is in the two SC jumps.
   On a *string* they do read both operands.
5. **String comparison is case-insensitive.** `String::operator==` (`0x00a1f140`)
   ends in `_wcsicmp` (IAT `0x00ac5640`), and its const-string fast path compares
   `hash_insensitive` at `String+16`.
6. **`OP_ADD` on two strings concatenates** (`operator+`, `0x00a1c3d0`) and
   `OP_ADD_ASSIGN` is `operator+=` — but only string-with-string, because of (1).
7. **Divide by zero is not a trap**: `run_time_error("Can't Divide 0")` and a fresh
   zero of the receiver's type.
8. **`>>` is `sar`** and both shifts mask the count to 5 bits. Division and modulo
   are `idiv` — C truncation toward zero.
9. **Float `<` and `>` are written as `!(x<y) && !(x==y)`** (`0x009d721d`), so an
   unordered (NaN) comparison yields **1**.

Also recovered: **a `run_time_error` ends the run.** `RunTimeEnv::run_time_error`
(`0x009c31e0`) finishes with `err_count++` (`RunTimeEnv+44`) and `script_status = 3`
(`RunTimeEnv+48`) — both member names verbatim from the PDB — and `exec`'s inner loop
is gated on `check_vm_running()`. Errors are fatal to the frame's script execution,
not warnings, and the VM now models that.

---

## 4. The measurement: `ops.rs` is Tier B

**`crates/don-bhs/ops-oracle/` — a new i686 harness that calls retail's own
`do_operator` with no game and no world.**

It works because the receiver reads exactly two fields of each operand (`data_type`
+4, `value` +16) and allocates results through a static recycler that falls back to
`malloc`. Fabricate two 20-byte objects with a relocated vtable pointer, call one
address. No `Game`, no `Constants`, no `StringTable`, no dynamic initialisers.

It re-uses the compiler lane's PE mapper and Win32 shim through `#[path]` includes and
**never edits them**. Crucially, `don-bhs` is a *path dependency* and the model side
of every case is `don_bhs::ops::do_operator` itself — not a copy typed into the
harness. That is the failure recorded in `README-LLM.md` ("a differential test whose
model is an inline copy tests the copy"), designed out rather than remembered.

```sh
# on hbox (x86_64 Linux; this Mac cannot execute 32-bit x86 at all)
cd ~/don-bhs-ops/crates/don-bhs/ops-oracle
nice -n 15 taskset -c 0-3 cargo build --target i686-unknown-linux-musl -q
RON_EXE=../oracle/data/riseofnations.exe \
  ./target/i686-unknown-linux-musl/debug/bhsops sweep
```

### Result [measured, 2026-08-08]

```
cases 6993 agree 6993 disagree 0
```

**Sample count and input distribution, stated as the charter requires:**

| receiver | opcodes | operand grid | cases |
|---|---|---|---|
| `ScriptInt::do_operator` `0x009d7760` | 36 (every case the switch has) | `{0,1,2,3,5,7,-1,-2,-7,31,32,INT_MIN,INT_MAX}²`, unary ops once per lhs | 4,936 |
| `ScriptFloat::do_operator` `0x009d7040` | 19 | `{0,1,2,0.5,-0.5,-1,3.25,-3.25,1e9,-1e9,1e-8}²` | 2,057 |

Excluded and therefore **not** covered: division/modulo by zero and `INT_MIN / -1`
(their handlers format a message out of the `StringTable`, which is not initialised in
this environment), and every mixed-type case (same reason). `ScriptString`,
`ScriptArray` and `ScriptObject` are untested — a `ScriptString` operand needs a real
retail `String`, which is the obvious next increment.

### The harness bites — mutation-tested

Reverting one line of `ops.rs` from the measured `a < 1` back to the naive `a == 0`
for `OP_UNA_NOT`:

```
cases 6993 agree 6989 disagree 4
  MISMATCH int op 0x09 a=-1 b=0: retail Int(1) model Int(0)
  MISMATCH int op 0x09 a=-2 b=0: retail Int(1) model Int(0)
  MISMATCH int op 0x09 a=-7 b=0: retail Int(1) model Int(0)
  MISMATCH int op 0x09 a=-2147483648 b=0: retail Int(1) model Int(0)
```

Exactly the negative operands, and retail is the one saying `1`. This is both proof
the sweep is not vacuous and an independent confirmation of finding (2)/(3) **on
retail hardware**, not from a decompiler.

This is testing, not verification. It says these 6,993 points agree; it says nothing
about the rest of the domain, and nothing about strings or aggregates.

---

## 5. Builtins: the measured debt list

`cargo run -p don-bhs --bin bhs-census` (`--all`, `--json`) over
`ron-data/bhs-corpus/`. It is a **lexical** scan, not a parser — the compiler front
end belongs to a sibling lane — and it says so: strip comments and string literals,
match `identifier (`, filter keywords, subtract the 1,133 names the corpus defines
itself (script functions and named `trigger` blocks), join against the 873-entry
table. 130 call-shaped identifiers are left unresolved and are printed rather than
hidden.

```
363 files, 93649 lines, 549 of 873 registered builtins called, 39957 call sites
implemented here: 25 builtins, covering 1.98% of measured call sites
```

One of those unresolved names is worth its own sentence. **`enable_trigger` (1,635
uses) and `disable_trigger` (436) are not registered builtins in this build** — they
are language statements, and they compile to `OP_BIT_UNSET` and `OP_BIT_SET`
respectively. That is an independent corroboration, from the shipped source side,
that those two opcode names are inverted relative to the machine (`0x3d` is `bts`,
`0x3c` is `btr`), which the predecessor lane found by disassembly alone.

Top of the list, which is the order the work should be done in:

| calls | idx | name | set |
|---:|---:|---|---|
| 3046 | 509 | `create_unit_upgrade` | Scenario |
| 1678 | 510 | `create_unit_in_group` | Scenario |
| 1547 | 77 | `set_timer` | Scenario |
| 1221 | 518 | `place_building_upgrade` | Scenario |
| **749** | **9** | **`rand_int`** | **MathUtil** |
| 717 | 411/412 | `object_position_x` / `_y` | Scenario |
| 659 | 311 | `find_unit` | Scenario |
| 655 | 661 | `give_good` | Scenario |
| 597 | 248 | `age` | Scenario |
| 311 | 21 | `parse` | StringUtil |

**The honest shape of this:** 1.98 % is small, and it is small for a structural
reason rather than a lack of effort. The engine puts its own seam between four
utility `FuncSet`s (31 functions, no simulation state) and `ScenarioFuncSet` (842
functions, *all* of which are the boundary to `don-sim`). This lane must not edit
`don-sim`, so the 25 it can honestly implement are the utility ones. Anyone who wants
the number to move implements `ScenarioFuncSet` entries against the simulation, in
exactly the order the census prints.

`rand_int` at #5 is the one that matters most for lockstep: it is
`Random::get(min,max)` (`0x00a39d70`) on `[0x00c06184]` — `GameAccess::game_random`,
**the main simulation stream**. `rand_real` (`0x009e18b0`) inlines the same LCG
(`s*1664525 + 1013904223`) against the same object, and `rand_get_seed` is a bare
`mov eax, [[0xc06184]]`. All three route through `Host` so the stream can never
accidentally fork. `UtilHost::game_random` implements retail's equal-bound,
inverted-bound, exclusive-upper-bound, and low-16-bit scaling rules; the derivation
records 1,500,012 retail cases with zero mismatches.

### Implemented, each from its handler

`sin` `cos` `tan` `asin` `acos` `atan` `sqrt` `absl_float` `absl_int` `rand_int`
`rand_real` `rand_seed` `rand_get_seed` `min_val` `max_val` (MathUtil);
`enable_all_triggers` `disable_all_triggers` `is_trigger_enabled` (TriggerUtil,
answered by the VM because their handlers read the running script through
`[0x00ebeed0]`); `length` `char_at` `char_from_int` `print` `print_line`
(StringUtil); `add`/1 `add`/2 `remove_index` `find` `clear` (ArrayUtil).

**BHS trigonometry is in degrees**: `sin` is `sinf(x * pi / 180)` with the two float
constants at `0x00b695cc` and `0x00b696c0`, and `asin` converts back. A radians
implementation would have been wrong and would have looked completely fine.

### Deliberately not implemented

- **`parse`** (`0x00a04720`, 311 calls). The `$NUM0` / `$STRING0` substitution the
  scenarios use for UI text. Its handler is undecoded and the placeholder vocabulary
  is not derivable from call sites alone, so it reports `Unimplemented` and appears in
  the debt list rather than being invented. It is UI text, not sim state.
- **`remove(array, value)`** (`0x00a04540`) and **`insert`** (`0x00a045b0`) — read
  only as far as their type guards. Zero corpus calls, so guessing buys nothing.

---

## 6. Files

Owned and written by this lane:

| path | what |
|---|---|
| `crates/don-bhs/src/value.rs` | rewritten: `Obj`/`Cell` aggregate model, measured `is_false`/`get_int`/`get_float`, `VarScope` |
| `crates/don-bhs/src/ops.rs` | rewritten from the five `do_operator` implementations |
| `crates/don-bhs/src/vm.rs` | nine aggregate opcodes, `Slot::Cell`, runtime-error/`err_count` model, trigger builtins |
| `crates/don-bhs/src/builtins.rs` | **new** — the 31 utility registrations, `UtilHost` |
| `crates/don-bhs/src/corpus.rs` | **new** — the lexical census |
| `crates/don-bhs/src/bin/bhs-census.rs` | **new** — the CLI |
| `crates/don-bhs/src/host.rs` | RNG-stream and `script_print` hooks |
| `crates/don-bhs/src/program.rs` | `enable_all_triggers`, `find_trigger` |
| `crates/don-bhs/tests/aggregates.rs` | **new** — 12 tests |
| `crates/don-bhs/tests/corpus.rs` | **new** — 6 tests, skip loudly without the corpus |
| `crates/don-bhs/ops-oracle/` | **new** — the i686 differential harness |

`crates/don-bhs/oracle/` belongs to the lane driving retail's compiler and was **not
touched**; `ops-oracle` includes its `image.rs`/`win.rs` read-only via `#[path]`.
⚠ That is a live coupling: as of writing, that lane is still editing those files, so
`ops-oracle` can transiently fail to build through no fault of its own. If it ever
needs decoupling, the two files are ~2,300 lines of PE mapping and Win32 shim with no
BHS content and can simply be vendored.

The hbox side lives in its own tree, `~/don-bhs-ops/`, precisely so that the compiler
lane's `~/don-bhs-oracle/` is never disturbed; it needs a workspace root above
`crates/don-bhs` because `don-bhs`'s manifest inherits `edition` from one.
Nothing outside `crates/don-bhs/` and this document was modified. Nothing was
committed.

---

## 7. What is still owed, in the order it blocks things

1. **`ScenarioFuncSet`.** 842 functions, 98 % of the measured call sites, and the
   only thing between this VM and running a real scenario. The census prints the
   order. This is `don-sim` work, not VM work.
2. **Extend the sweep to `ScriptString` / `ScriptArray` / `ScriptObject`.** Needs a
   fabricated retail `String` (20 bytes, layout already in
   `oracle/src/main.rs::RStr`) and one for the aggregates. The error paths — divide
   by zero, mixed types, bad opcode — need the `StringTable` at `[0x00C06378]` alive;
   that is the same blocker the compiler lane is working through, so the two should
   land together.
3. **Finish the chunk container.** `don_bhs::chunk` now mirrors the tag-0 root and
   scalar tags 2–8, including backward global-file resolution for non-empty tag 6,
   and produces checksum metadata without a compiler. Tag 9's global struct
   registration remains an explicit error rather than a partial load.
4. **The `bhs.log` decode differential.** `OpCode::write_code` (`0x009c2d90`) emits a
   symbolic listing whenever handed a non-empty directory. It remains free the moment
   the compiler lane produces bytecode, and `disasm::format_all` is already shaped for
   it. This lane could not run it: there is no compiled `.bhs` anywhere to compare
   against yet, and inventing one would have tested nothing.
5. **`ref` call aliasing.** The retail writer/loader pair now settles the metadata as
   `[SymType.type, is_ref ? 1 : 0]`, and `Script::script_type` is literal zero for every
   local script. The caller lowering and ownership transition that preserve a mutable
   argument alias are not recovered. `don-bhs` records the bits and refuses such calls;
   it does not silently pass them by value. A reference compiled image must settle both
   that lowering and the surrounding chunk details byte-for-byte.
6. **`ScriptObject::get_string`** (`0x009d6370`, 353 bytes) — the aggregate's string
   rendering. It is not recovered, so aggregate-to-string conversion now fails
   explicitly instead of inventing a comma-joined representation.

## 8. Provenance

Addresses are from `ron-bin/riseofnations.exe` (PE32 i386, image base `0x00400000`),
named via `ron-bin/sbl/rise.pdb`. Layouts, `VarScope`, and the `RunTimeEnv` field
offsets are read out of the PDB type stream with `llvm-pdbutil dump --types`. Handler
behaviour is read from `re/decomp-all/` for structure and re-checked at the
instruction level with capstone `CS_MODE_32` for every numeric decision. The builtin
table is the predecessor lane's mechanical decode and was **not** re-derived.

Fidelity: `ops.rs` is **Tier B for `int` and `float`** — 6,993 differential cases
against retail, distribution above, mutation-tested. Everything else in the crate is
**Tier C**: behaviourally faithful to our reading, divergence unmeasured. Nothing here
is verified, proven, or a refinement.
