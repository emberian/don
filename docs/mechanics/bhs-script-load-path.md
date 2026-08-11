# How a shipped BHS script becomes a running program

Lane: `bhs-scenario-runtime`, 2026-08-11, against `cv task` row
`closure/stage: scenario_runtime`. Every address below was read off
`ron-bin/riseofnations.exe` (PE32 i386, sha256 `30478a44…625079`, image base `0x00400000`)
with Capstone; names come from the GUID-matched `ron-bin/sbl/rise.pdb`. `re/decomp-all/`
was used only for orientation, and every load-bearing branch was re-read at the
instruction level. **Tier C.** Nothing here has been executed against retail machine code
and no byte of our compiler's output has been compared with retail's.

---

## 0. The gap this closes

The VM, the compiler, the builtin registry, and the step-4 producer in
`crates/don-sim/src/script_runtime.rs` all existed. What did not exist was a way to get a
**shipped** `.bhs` into any of them: every consumer in the tree built its `Program` from a
BHS source string written inside a test. `ScriptRuntime::new` takes a `don_bhs::Program`
and nothing in the repository produced one from `ron-data/`.

Two things were missing, and both are engine behaviour rather than plumbing:

1. **file resolution** — retail does not search for a script by name; it tries three
   specific candidates through the mod stack, in order;
2. **the global script-file set** — retail's step 4 runs two scripts from two separately
   compiled files, resolved by name across one global vector.

Both are now derived and implemented.

## 1. `Compiler::compile` `0x009bf160` — the entry contract

| step | instructions | behaviour |
|---|---|---|
| reject empty | `0x009bf186..0x009bf199` | `path == EMPTY_STRING` returns **-1** before anything else. An empty script name is a failed compile, not "no script". |
| content dir | `0x009bf1f5..0x009bf206` | `path.prepend_content_dir(0, "script\\compiler.cpp", 100)` `0x00A1D690` |
| extension | `0x009bf248..0x009bf263` | `path.check_ext(L"bhs", 1)` `0x00a1bce0` (wide literal `0xb04758`) |
| reuse | `0x009bf2cb..0x009bf308` | `ScriptFile::find_script_file` `0x009c6a10`; if found and `ScriptFile::has_changed` `0x009c4a70` is 0, **the file is not recompiled** and `compile` returns 0 |
| lex | `0x009bf4d0..0x009bf4e3` | `Lexer::open_file` on the global `Lexer` at **`0x00ebe380`** |
| parse | `0x009bf62f` | `yyparse` `0x009ba430` |

`Compiler::compile` has 27 callers, and one of them matters here: **`RunTimeEnv::run_script`
itself**. See §4.

### `String::check_ext` `0x00a1bce0`, single-token form

Read at `0x00a1be54..0x00a1c168`:

* the "already has this extension" test only runs when `curr_len >= 4` **and** the
  character at `curr_len - 4` is `'.'`. The `4` is a literal, so the test is hard-wired to
  three-character extensions;
* on a match (case-insensitive, `String::operator==` `0x00a1f140` → `_wcsicmp`) the string
  is returned unchanged;
* otherwise, with `force != 0`, the existing three-character extension is **truncated**
  (`String::truncate` `0x00a1afb0` with `curr_len - 4`) and `"." + token` appended;
* with no three-character extension, `"." + token` is simply appended.

So `general_powers` → `general_powers.bhs`, `editor_scratch.svx` → `editor_scratch.bhs`,
and `a.jpeg` → `a.jpeg.bhs` — the last because `'.'` is not at `curr_len - 4`. That is
retail behaviour, and `don_bhs_cc::load::check_ext` reproduces it including that case.

**Not recovered:** the general multi-token form. `check_ext` tokenises `exts` on `' '` and
loops, and two further literals it can append (`0xb13f98`, `0xb13f9c`) were not decoded.
Every call this project makes is the single-token `L"bhs"` form, and the Rust function
asserts rather than guessing when handed anything else.

## 2. `Lexer::open_file` `0x009bff30` — the whole search

`__thiscall`, `ret 4`, on the global `Lexer` at `0x00ebe380`. In order:

1. `if (name.curr_len == 0) return 1;` (`0x009bff7d`).
2. the `included_files` guard — see §2.2.
3. **candidate A**, only when `Lexer+0x14` (the current `LexerFileEntry`) is non-null, i.e.
   for an `include` and never for the root: `current->path.get_directory()`
   (`String::get_directory` `0x00a1dcd0`) `+= name`, then `prepend_content_dir(…,
   "script\\lexer.cpp", 0x154)`, then `_wfsopen(path, L"rb", 0x40)`
   (`0x009bffcf..0x009c0079`).
4. **candidate B**: `name` alone → `prepend_content_dir(…, 0x178)` → `_wfsopen`
   (`0x009c0090..0x009c00f7`).
5. **candidate C(i)**: for `i` in `0..Lexer::include_paths.count`,
   `include_paths[i] += name` → `prepend_content_dir(…, 0x188)` → `_wfsopen`
   (`0x009c010a..0x009c01d3`; the array is `ObjectArray<String>`, 20-byte stride, count at
   `Lexer+0xd4`, data at `Lexer+0xe0`, base `Lexer+0xd0` = PDB member `include_paths`).
6. otherwise return 1.

On success a `LexerFileEntry` is popped from `Recycler<LexerFileEntry>::pop` `0x004cbdd0`,
its first member (a `String`) is assigned the **resolved** path (`0x009c020f`), and the
entry is appended to `included_files` (`Lexer+0xe8`, `0x009c028f..0x009c0298`).

**Nothing scans the install tree.** An earlier version of `don-bhs-cc` resolved an
`include` by walking the tree for a matching basename. That accepts programs retail
rejects, and picks a different file than retail whenever two directories hold the same
name — which is precisely the situation a mod creates.

### 2.1 `Lexer::include_paths` is a preference with one shipped default

`Lexer::init_once` `0x009c0f90` at `0x009c106a..0x009c1088`:

```
mov eax, [0x00c06378]        ; int_str_array
mov edx, [eax + 0x10]        ; the 20-byte String array
lea eax, [edx + 0x23118]     ; 0x23118 / 0x14 = ordinal 7182
add edx, 0x23104             ; 0x23104 / 0x14 = ordinal 7181
call prefs_get               ; 0x00a2b170
```

Those two `internal_strings.xml` ordinals are, in shipped document order:

| ordinal | value |
|---:|---|
| 7181 | `ScriptIncludePath` |
| 7182 | `.\scenario\scriptlibrary\` |

The result is tokenised on the separator set `",;"` (wide literal `0xb04b1c`) by
`TokenString::next` `0x00a44000`, and each token is appended to `include_paths`
(`0x009c1160..0x009c11ae`). So **the shipped default include path is exactly one
directory**, `.\scenario\scriptlibrary\`, and it is user-overridable through the
`ScriptIncludePath` preference.

Two ordinals further along, `script_loc_file` → `scen_script_loc.xml` (7183/7184) is the
`$S(...)` localisation table `Lexer::localize_string` `0x009c1bd0` reads. Not used here;
recorded so the next lane does not re-derive it.

### 2.2 The `included_files` guard cannot fire — read the code

`0x009bffa0..0x009bffb4`:

```
mov eax, [edi + 0xf8]        ; included_files.data
lea ecx, [ebp - 0x28]        ; &local String
push ecx                     ;   ... as the ARGUMENT
mov ecx, [eax + esi*4]       ; this = included_files[i]
call String::operator==      ; 0x00a1f140
```

`[ebp-0x28]` is the function's working `String`, and it is **zero-initialised at entry**
(`0x009bff54..0x009bff77`) and not assigned until candidate A begins at `0x009bfff7`. The
comparison is therefore `included_files[i]->path == ""`, which cannot be true: the entry's
path is assigned from a successfully opened candidate at `0x009c020f`.

So as emitted, a file included twice in one compilation is **opened twice**, bounded only
by the depth cap in §2.3. This is a "the PDB names it, the code does something else" case:
the member is called `included_files` and the loop looks exactly like an include guard.

**We do not guess what retail then does with the duplicate declarations.**
`don-bhs-cc` still processes each file once and emits a `Note` diagnostic wherever retail
would diverge, so the divergence is visible instead of silent. Measured on the shipped
corpus: **5 of 363 roots** reach a file twice, all of them `game_structs.bhs` through
their own `include` plus `ctw_lib.bhs` —

```
conquest/Alexander/hideawayruntime.bhs   conquest/ColdWar/coldwar_diplo.bhs
conquest/Napoleon/syriasetup.bhs         conquest/Napoleon/wagramsetup.bhs
conquest/scripts/diplo_response.bhs
```

Maximum include depth across the corpus is 2 and there are no cycles. One reference image
from the retail compiler for any of those five roots settles it; until then the count is
pinned by a test so it cannot drift unnoticed.

### 2.3 Nesting stops at 16

`Lexer::start_include_file` `0x009c1960` reads the include token
(`Lexer::YYText` `0x009c2560`, then strip 9 leading and 1 trailing character), and at
`0x009c19a6` refuses when `Lexer::file_stack`'s count is `>= 0x10` — **before** calling
`open_file`. `file_stack` is PDB offset 284 (`0x11c`) with layout `{data +0x11c, cap
+0x120, count +0x124, grow +0x128}`, read from the `cmp eax, [edi+0x120]` / `inc dword
[esi+8]` pair in `Lexer::set_curr_buffer` `0x009bfeb0`.

`set_curr_buffer` pushes the **outgoing** current file and then installs the new one, and
pushes nothing when there is no outgoing file — so the root sits at count 0 and 17 files
can be open before the 17th's own `include` is refused.

## 3. Resolution goes through the mod stack

Every candidate above ends in `String::prepend_content_dir` `0x00A1D690` →
`ModManager::calcFilePath` `0x00A22910`, which `crates/don-content/src/vfs.rs` already
reproduces. `crates/don-content/src/script.rs` joins the two: a mod that declares
`scenario/scriptlibrary/ctw_lib.bhs` now replaces the shipped one for every script that
includes it, with the same first-match-wins precedence as `data/rules.xml`.

Two details that a naive bridge gets wrong, and that are covered by tests:

* `calcFilePath` **never stats**. It returns a path and the caller's `_wfsopen` decides.
  A probe that reported success for a declared-but-absent mod file would stop the include
  search at candidate B instead of falling through to candidate C.
* the shipped tree mixes case (`Cliffs.xml`, `IME.xml`, `scenario/Chess Exercise/`) and
  `ContentStack` lower-cases its *lookup keys*. The disk probe therefore folds case **per
  path segment**; a lower-cased whole path does not exist on a case-sensitive host.

### Stated boundary

Candidate A joins the concrete directory of the including file rather than re-running
`calcFilePath` on `dirname(resolved_path) + name`. For the shipped tree the two are
identical. They can differ when the including file itself came from a mod and a
*different* mod owns the sibling name; `calcFileNameAndCategoryFromPath` `0x00A21E40`'s
behaviour on an already-`mods/`-prefixed path was not derived, so that case is a known
boundary rather than a claim.

## 4. What tick step 4 actually calls

`RunTimeEnv::run_script` `0x0043d0e0` is a 32-byte varargs thunk into
`RunTimeEnv::run_script(const String& script_name, const String& file_name, ScriptRunMode,
int nargs, char* va)` `0x009c4460`, passing `file_name = EMPTY_STRING`. The full function:

1. `if (file_name.curr_len != 0) Compiler::compile(file_name, 0);` — a named-file call
   compiles on demand. **The step-4 thunk passes the empty string, so step 4 never
   compiles.** The program must already be loaded when the tick runs.
2. `ScriptFile::find_script(script_name, file_name, &file_index)` `0x009c6c80`.
3. missing script → `RunTimeEnv::run_time_error` `0x009c31e0` and **return 3**, the same
   `script_status = 3` an in-script runtime error sets. It is not silent.
4. parameter setup (`0x009c3b00`); a mismatch returns **4**.
5. `Recycler<VirtualMachine>::pop`, `VirtualMachine::init` `0x009e16d0`, push onto the
   `RunTimeEnv` VM stack, `RunTimeEnv::exec` `0x009c3600`.
6. afterwards, `err_count` (`RunTimeEnv+0x2c`) non-zero reports a runtime error.

### `ScriptFile::find_script` `0x009c6c80` — binding is by name, globally, backwards

* the file loop runs **backwards**, `script_files.count - 1` down to 0 (count `0x00c8cba4`,
  data `0x00c8cbb0`), so when two loaded files define the same script name the
  **later-loaded file wins**;
* null slots are skipped;
* a non-empty `file_name` filters by file; the step-4 caller passes empty, so every loaded
  file is eligible;
* the per-file scan is forward over `scripts` (count `+0x20`, data `+0x2c`) comparing
  `Script::name` at `+0xac` with `String::operator==` — **case-insensitive**.

`don_bhs::Program::find_script` implements exactly this, and `Program::append` makes one
`Program` hold several compiled units the way `ScriptFile::script_files` does. That matters
because step 4 runs **two** scripts from **two** `Compiler::compile` calls: the selected
game script every frame, and `general_powers` when `Game::frame > 0`. A runtime that could
hold only one compiled unit cannot express retail's step 4.

`append` is refused, not half-applied, when either side carries a channel-15 sidecar (its
`linked_file_indices` are global and would silently repoint) or tag-9 global type names
(whose cached `String` hash words are not recoverable from the name text).

## 5. The second slot's path is shipped data, not a guess

`ScenarioFuncSet::init` `0x00a03c30` assigns
`ScenarioData::general_powers_script_file` from the runtime string table at
`0x00a04014..0x00a04021`: `[[0x00c06378]+0x10] + 0x1d178`, and `0x1d178 / 0x14` is ordinal
**5958**, whose shipped value is `./scenario/scriptlibrary/general_powers.bhs`. The
derivation is `docs/assembly/scenario-initial-state.md` §7; `don_bhs_cc::load::GENERAL_POWERS_SCRIPT_FILE`
carries the constant and a test reads the ordinal back out of
`ron-data/internal_strings.xml` in document order rather than trusting the transcription.

## 6. What this makes possible, and what it does not

**Possible now.** `don_bhs_cc::load::load_script(&include_path, path)` takes the path
retail would have in its `String` — a scenario's script name, `Game+0x500`, or the
general-powers constant — and returns a `don_bhs::Program` plus the zero-argument entry
name that `Game::do_frame` would pass to `run_script`. The shipped
`scenario/scriptlibrary/general_powers.bhs` loads this way, resolves its own entry with
arity 0, and carries its 14 shipped `static` slots and one `run_once` trigger. Those
statics are the entire cross-frame memory of a BHS script and are what lands in checksum
channel 15.

**Not possible yet, and not claimed.** Loading is not running. The tick's step-4 producer
still needs the setup path described in §7, and `ScenarioFuncSet` remains 842 unimplemented
builtins deep — `general_powers` alone calls `time_sec`, `set_timer`, `timer_expired`,
`object_type_selected`, `find_unit`, and the bubble-text family, and every one that is
unrecovered fails the tick closed by design.

## 7. The tick hook this lane could not make

`crates/don-sim/src/tick.rs` and `crates/don-sim/src/script_runtime.rs` belong to sibling
lanes. What they need, stated so it can be implemented without re-deriving anything:

1. **Build the runtime from shipped content, not a source string.**
   ```text
   let inc = don_content::ContentScriptSource::new(install_dir, stack).include_path();
   let mut game = don_bhs_cc::load::load_script(&inc, &selected_script_name)?;
   let gp       = don_bhs_cc::load::load_script(&inc, don_bhs_cc::load::GENERAL_POWERS_SCRIPT_FILE)?;
   let gp_base  = game.program.append(gp.program)?;      // one global ScriptFile set
   ScriptRuntime::new(
       game.program,
       Some(ScriptBinding::new(0, game.entry)),
       Some(ScriptBinding::new(gp_base, gp.entry)),
   )
   ```
   `don-sim` currently has `don-bhs-cc` only as a **dev-dependency**; this needs it as a
   real one. `don-content` gained a `don-bhs-cc` dependency for the same reason.

2. **Bind by name across the whole set, not by `(file, name)`.** `ScriptBinding` names a
   file index, but `RunTimeEnv::run_script` resolves the name across every loaded file,
   **last file first** (§4). `Program::find_script` is that lookup; a binding that pins a
   file index diverges as soon as two loaded files share a script name.

3. **A missing script is `script_status = 3`, not a skipped step.** `run_script` returns 3
   through `run_time_error`. A step-4 producer that treats an absent binding as a no-op is
   modelling something the engine does not do.

4. **`general_powers` runs only when `Game::frame > 0`**, and it is the *second* of the two
   step-4 calls. `script_runtime.rs`'s module docs already state this; the loader now
   supplies the file it needs.

## 8. Files

| path | what |
|---|---|
| `crates/don-bhs-cc/src/sema.rs` | `IncludePath` rewritten to `Lexer::open_file`'s three candidates; `ContentProbe`, `InstallRoot(s)`, the `ScriptIncludePath` default, the depth cap, the re-inclusion note |
| `crates/don-bhs-cc/src/load.rs` | **new** — `check_ext`, `load_script`, `LoadedScript`, `GENERAL_POWERS_SCRIPT_FILE` |
| `crates/don-bhs-cc/src/lib.rs` | module + re-exports |
| `crates/don-bhs-cc/tests/script_load.rs` | **new** — 7 tests, §9 |
| `crates/don-bhs/src/program.rs` | `Program::append`, `Program::find_script`, `ProgramMergeError` |
| `crates/don-content/src/script.rs` | **new** — `ContentScriptSource`, the mod-stack probe |
| `crates/don-content/src/lib.rs`, `Cargo.toml` | module, re-export, `don-bhs-cc` dependency |

## 9. Evidence

`cargo test -p don-bhs-cc` — 50 tests, including the 363-file corpus gate, which still
compiles **363/363** under the new resolver: retail's three candidates resolve all 232
shipped `include` statements and pick the same file the old basename walk picked. The rule
change is therefore a fidelity correction with a measured zero-diff on shipped data; it
bites on mods and on any tree with a duplicated basename.

`crates/don-bhs-cc/tests/script_load.rs`, and what each test catches:

| test | catches |
|---|---|
| `the_general_powers_path_is_the_shipped_string_table_ordinal_5958` | a hand-edited constant; the expectation is read from `ron-data/internal_strings.xml` at the ordinal the instruction stream names |
| `shipped_general_powers_loads_as_a_zero_argument_step_four_entry` | a loader that compiles but drops the statics, the bytecode, or the arity-0 entry |
| `a_bare_script_name_gets_the_retail_extension_and_resolves` | losing `check_ext` or candidate C |
| `every_shipped_include_resolves_under_the_retail_candidate_order` | a candidate order that cannot resolve the shipped corpus; also pins the 5-root re-inclusion count |
| `a_same_named_file_outside_the_candidates_is_never_used` | a regression to the basename walk, in both directions: the wrong file chosen, and an out-of-candidate name wrongly accepted |
| `the_including_files_own_directory_is_tried_first` | candidate order, by removing one candidate at a time and asserting the next takes over |
| `include_nesting_stops_at_the_retail_limit_of_sixteen` | an off-by-one in the depth cap, pinned by both the refusing file and the opened count |

**Mutation-tested.** Deleting candidate C from `IncludePath::resolve_from` fails 4 of the 7
(`a_bare_script_name…`, `a_same_named_file…`, `the_including_files_own_directory…`,
`every_shipped_include…`); the suite is not vacuous.

`cargo test -p don-bhs --lib` — 33 tests, including three new ones for `append`/
`find_script` (backwards file order, refusal paths, linked-file rebasing).
`cargo test -p don-content --lib` — 78 tests, including three for the mod-stack probe.

None of this is verified, proven, or differentially tested against retail. It is Tier C:
structure read off the shipped binary, with the divergences we know about named in §2.2 and
§3.
