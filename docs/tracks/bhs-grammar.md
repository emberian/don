# The BHS grammar

Big Huge Script, as the shipped programs actually use it. Written from
`ron-data/bhs-corpus/` — 363 files, 93,649 lines by Big Huge Games — and cross-checked
against the retail compiler in `ron-bin/riseofnations.exe`. This is a description, not a
design. Where the corpus is ambiguous it says so, and where a construct exists in the
compiler but never in a shipped file it is marked **unattested**.

Implemented by `crates/don-bhs-cc/`. **363/363 files parse; 363/363 compile with zero
errors.** Progress and open questions: `docs/tracks/bhs-compiler.md`.

---

## 0. The single most important correction

The keyword-frequency survey that opened this lane reported `and` 845, `or` 138, `not`
394, `const` 11. **Every one of those is inside a comment or a string literal.** Blank the
comments and the string literals and the corpus contains **zero** occurrences of `and`,
`or`, `not`, `const`, `array`, `Array`, `bool`, `goto`, `new`, `delete` or `null` in code.
The only survivor of that list is `void` (65, always a return type).

The binary agrees. The retail front end is **flex + bison** — `Lexer::yylex` `0x009bc8e0`
with DFA tables at `.rdata` `0xb03500`ff, `yyparse` `0x009ba430` with LALR tables at
`0xb01640`ff — and none of `and`, `or`, `not` is a lexer rule. They lex as ordinary
identifiers. **BHS has no word operators.** `not(x)` is a function call.

Two consequences: never take a keyword count from a raw grep of this corpus, and a BHS
parser that accepts `a and b` is accepting a program retail would reject.

---

## 1. Lexical structure

### Encoding

ASCII with occasional Windows-1252 bytes inside strings and comments. Exactly one shipped
file, `conquest/Napoleon/leipzigsetup.bhs`, is not valid UTF-8. Decode UTF-8 and fall back
to Latin-1; the grammar itself is pure ASCII.

### Comments

`// to end of line` and `/* … */`. **Block comments nest** in retail (lexer start
condition 5, rules 104/105 increment and decrement a depth counter at `lexer+0x72`, capped
at 16). No shipped file nests them, so nesting is unattested in the corpus but real in the
compiler.

### Identifiers

`[A-Za-z_][A-Za-z_0-9]*`. Names are compared **case-insensitively** in practice — the
corpus writes both `String` and `string`, both `int` and `Int`.

### Keywords

Recovered from the retail flex DFA by walking it state by state. The complete reserved
list, with corpus attestation:

| keyword | attested | note |
|---|---|---|
| `if` `else` `while` `for` `switch` `case` `default` `break` `return` | yes | |
| `continue` | **no** | 15 raw hits, all in comments and strings |
| `do` | yes | `do { … } while (…);`, 208 uses |
| `static` | yes | 2,433 declarations |
| `ref` | yes | 75 parameters |
| `struct` | yes | 3 declarations, all in `game_structs.bhs` |
| `labels` | yes | 199 blocks |
| `trigger` | yes | 1,554 |
| `run_once` | yes | 182 |
| `include "…"` | yes | 232 |
| `true` `false` | yes | 616 |
| `enable_trigger` `disable_trigger` | yes | 2,081 — **keywords, not builtins**; see §7 |
| `$S` | yes | 2,927 |
| `parse` | yes | a keyword *and* a builtin; see §6 |
| `array` / `Array` | **no** | two spellings, one token |
| `=>` / `= ref` | **no** | same token; `= ref` needs exactly one space |
| `~~` | **no** | only lexed when the compiler is in watch-expression mode |

**Type names are not keywords.** `SymTable::init_root` (`0x009d8c80`) registers five
built-in data types — *int, float, string, void, bool* — into the block at `[0x00ebe408]`,
taking their spellings from the runtime `StringTable`. They lex as identifiers and are
resolved against the symbol table. A parser should treat them as ordinary names, which is
what makes user `struct` types usable as types with no special casing.

`include` is handled entirely in the lexer (`Lexer::start_include_file` `0x009c1960`) and
never reaches the grammar. Retail matches it as the literal two-token sequence
`include "…"` with **exactly one space**; `include"x"` lexes as the identifier `include`.
Our lexer is laxer about the whitespace, which accepts a superset.

### Literals

- **Integer** — decimal. Hex `0x…` is accepted by our lexer but unattested.
- **Real** — `1.5`, `360.0`, `100.0`. Eleven in the whole corpus. Binary32.
- **String** — `"…"`. Escapes: `\\` (16) and `\"` (14) are the only C escapes present.
  `\s`, `\c` and `\a` also occur and are **not** escapes; they belong to `parse()`'s markup
  and pass through with the backslash intact.
- **`$S("…")`** — a localised string; see §6.
- `true` / `false` — the integer constants 1 and 0 (retail returns token `0x102`, the same
  token as an integer constant).

### Adjacent string literals concatenate

C-style, and it matters. `scenario/Custom/italy_mp/italy_mp.bhs:334-336`:

```bhs
message_1 = "In 1453 Constantinople finally falls to the Ottoman Turks. Afraid of the new Turkish rule many Greek residents "
            "flee the city to the Italian peninsula. They bring with them many classic works of art and literature. All factions "
            "gain 500 ";
```

That settles a second site. `scenario/scriptlibrary/ctw_lib.bhs:326` reads

```bhs
house_names = ["House D" "House D1", "House D2", …];
```

— a **missing comma**. Under concatenation, element 0 is `"House DHouse D1"` and the array
is one shorter than its author intended. This is a shipped bug and a faithful compiler
reproduces it rather than guessing at a comma.

### Operators

```
=  +=  -=  *=  /=  %=  **=  <<=  >>=  &=  ^=  |=
==  !=  <  >  <=  >=
+  -  *  /  %  **
&&  ||  !  &  |  ^  ~  <<  >>
++  --
( ) [ ] { } ; , . :
```

Attested in code: `==` 8326, `&&` 2521, `++` 1668, `--` 893, `>=` 745, `||` 640, `+=` 488,
`!=` 416, `<=` 407, `/=` 77, `-=` 16, `&` 13, `>>` 2, `<<` 2, `^` 1. **Unattested:** `**`,
`**=`, `%=`, `*=`, `<<=`, `>>=`, `&=`, `^=`, `|=`, `~`, and the ternary `?:` — the 152 `?`
hits are all prose.

Two lexer facts with grammar consequences:

- **`&` and `&&` are the same token** (`0x10f`), and so are `|` and `||` (`0x110`). The
  parser cannot tell them apart; bitwise versus logical is decided by the operand type
  inside `do_operator`. Our parser keeps them distinct (with C precedence) because the
  corpus never mixes them in one expression; this is a **known divergence** from retail
  and is listed in `bhs-compiler.md`.
- `**` is `powf` on binary32, not an integer power; see §8.

---

## 2. File structure

```
file        := item*
item        := include | labels-block | struct-decl | script | file-var
include     := 'include' STRING ';'?          // the ';' is absent in all 232 uses
```

A file contains any number of named scripts and **at most one anonymous entry script**.
319 of the 363 files have one. It is the per-frame tick handler: `Game::do_frame` calls
`run_script(<name>, 0 args)` once per simulation frame, before the frame counter
increments, so a BHS program is a state machine over its `static`s. That is exactly why
the shipped scripts are giant step-number switches.

File-scope variable declarations are grammatical (`VarType::init` `0x009da870` allocates
them a file global or file static slot) but **the corpus contains none**.

---

## 3. Scripts

```
script      := ret-type? script-type? NAME '(' params ')' ( ';' | block )
             | ret-type? script-type    block                  // the anonymous entry
script-type := 'ai' | 'scenario' | 'conquest'
params      := ( 'void' | param ( ',' param )* )?
param       := 'ref'? type? NAME '[]'?
```

Both qualifiers are optional and both are plain identifiers at the token level, so the
parse is positional: a run of identifiers followed by `(` or `{`, of which the last is the
name. Attested shapes:

```bhs
void conquest place_new_world_army(int who, int x, int y);   // ret + script-type + name
String[] conquest tactics_deletion_list(String list_name);   // array return type
int ai economic (int who, ref int step, int boom, int loops) // `ref` parameter
scenario all_gen_entrench (Player) { … }                     // no return type, untyped param
float scenario civil_war_by_percent (Empire, Rebels, percent) { … }
scenario { … }                                               // anonymous entry
void scenario { … }                                          // …with a return type
int scenario { … }
```

Counts: `conquest` 262, `scenario` 63, `ai` 11 in the three-word form. 148 forward
declarations, 230 definitions, 319 anonymous entries. 51 parameters carry no type; 75 are
`ref`.

Retail's default-type reduction gives every omitted type root `int` (`0x00057bad`),
including an omitted return type and an untyped parameter or variable. The later switch to
`void` when entering a script body is scope setup, not the signature's return type.

`(void)` is the empty parameter list.

**The anonymous entry script's name is unresolved.** Attachment is a `Game`/`GameInfo`
field, so the engine looks the script up by name; which name it registers for an anonymous
body is unread. `don-bhs-cc` uses the file stem.

---

## 4. Declarations

```
decl        := 'static'? type? declarator ( ',' declarator )* ';'
declarator  := NAME array-suffix? ( '=' initialiser )?
array-suffix:= '[' ']' | '[' expr ']'
initialiser := expr | array-literal
array-literal := '[' ( expr ( ',' expr )* ','? )? ']'
```

Attested:

```bhs
int i;
String city_name;
UnitGroup first_battalion;                       // a user struct type
static int end_time = get_time_limit();
static have_objective = false;                   // NO TYPE
static int infantry[] = [1, 0, 0, 0];
static String inf_names[] = ["Greek Mercenaries", "Legions", "Scutari", "Barbarians"];
```

Declaration is distinguished from expression by **shape**, not by a type table:
`Ident Ident` and `Ident [ ] Ident` start declarations, `Ident (`, `Ident .`, `Ident =` and
`Ident [ expr ]` start expressions. That rule is what lets a user `struct` name be used as
a type without the parser knowing it, and it is what a one-pass 2003 compiler can do.

### Implicit declaration

An assignment to an undeclared name **declares it**, as an `int`. This is a grammar rule,
not error recovery: bison rule 92 is an empty type-specifier whose action loads the
built-in `int` `SymType` from `[0x00ebe408]+0x00`, and rule 93 then calls
`SymTable::create_var_type`. Driving the shipped LALR tables on
`scenario { run_once { NAME = INT ; } }` reduces 92 then 93 and accepts.

`VarType::init` picks the storage by position: inside a script body it takes a frame slot
(a **local**, fresh every frame); at file scope a file global, or a file static when
written `static`. Every implicit declaration in the corpus is inside a body, so they are
all locals. There are **14,356** of them.

Corpus examples: `for (j = 1; j < 3; j++)` in `ctw_lib.bhs` with no declaration of `j`;
`my_capital = find_city_with_num(who, 1);` in `economic.bhs`.

A related trap, from `Lexer::resolve_token` (`0x009c1460`): **another script's locals are
invisible**, and referring to such a name silently declares a fresh one rather than
erroring.

### `static`

`static` storage is `Script::static_vars` (`Script+0x3c`, a `PtrArray<ScriptType>`), and it
is the **entire cross-frame memory of a BHS program**. A `static` initialiser runs exactly
once ever: the compiler guards it with `OP_JUMP_IF_INITED`, which tests the slot for null.
This is the storage the `script_run_time` checksum channel hashes.

---

## 5. Statements

```
stmt := ';'
      | block
      | decl
      | expr ';'?                          // the ';' is OPTIONAL — see below
      | 'if' '(' expr ')' stmt ( 'else' stmt )?
      | 'while' '(' expr ')' stmt
      | 'do' stmt 'while' '(' expr ')' ';'
      | 'for' '(' (decl | expr)? ';' expr? ';' expr? ')' stmt
      | 'switch' '(' expr ')' '{' ( ('case' const ':' | 'default' ':') | stmt )* '}'
      | 'break' ';' | 'continue' ';' | 'return' expr? ';'
      | 'labels' labels-block
      | 'trigger' NAME? ( '(' expr? ')' )? stmt
      | 'run_once' block
      | struct-decl
```

**The terminating `;` on an expression statement is optional.** Not a design choice:
`conquest/Alexander/thrace.bhs:269` and `thrace2.bhs:189` both ship a bare
`enable_trigger("add_merchant")` with no semicolon, followed by another statement. Every
other statement form requires its terminator and the whole corpus sustains that.

`switch` labels that are adjacent with no statements between them share a body — ordinary
C fallthrough grouping. 336 switches, 1,574 `case`s, 168 `default`s.

`trigger`'s body is any statement, not necessarily a block:
`trigger (num_units(attacker) < 1) defeat(attacker);` is attested. The name is optional
(`trigger (num_cities(1) > 0) { … }`) and so is the condition (`trigger allies() { … }`).

### `labels`

```bhs
labels {
  BLOCK_ON_THIS = 1,
  DONT_BLOCK_ON_THIS,
  SCRIPT_DONE,
}
```

Enum-like integer constants; a trailing comma and an empty block are both fine. It occurs
at file scope (4) and as a statement inside a script body (195). The statement form is
script-scoped and is visible throughout the script, not only after its position.

**Auto-numbering starts at 1**, and an unvalued entry is the previous plus one. From the
corpus, not from taste: ten blocks mix implicit and explicit entries and every one is a
player-number table whose leading implicit entries are the playable sides —
`AMERICANS, COLOMBIANS, REBELS = 7` (`colombiaruntime.bhs`),
`MACEDONIANS, PERSIANS, BARON1 = 8, …` (`sogdiana.bhs`),
`PLAYER, PORTUGUESE, FRENCH = 8` (`portsetup.bhs`),
`ALEXANDER, BESSUS, SPITAMENES, SATRAP, DARIUS` (`sogdiana`). In
`conquest/ColdWar/skirmishsetup.bhs`, `labels { ATTACKER, DEFENDER }` is immediately
followed by `for (i = 1; i < 3; i++) gain_tech(i, …)` granting the same two sides the same
techs that lines 65-68 grant to `ATTACKER` and `DEFENDER`. Zero-based numbering would make
every one of those tables address RoN player 0, which is Gaia.

Still worth confirming against the retail compiler; it is one `labels` block away from
being `[measured]`.

---

## 6. `$S` and `parse`

`$S("…")` is a **lexer token** (flex rule 82, token `0x127`), and grammar production 8 is
`$S '(' STRING ')'`. Its action is

```
SymTable::add_const_type(0x168174 /* string const */,
                         Lexer::localize_string(Lexer::get_str($3)))
```

so it compiles to a plain string **constant** whose *value* is a **compile-time
localisation lookup**. `Lexer::localize_string` (`0x009c1bd0`) keys the campaign's
localisation XML by `String::get_hash()` of the literal and falls back to the literal when
the lookup misses. The shipped tables are visible in the corpus:
`$S("Keep Alexander alive.")` ↔ `<STRING hash="25311551">Keep Alexander alive.</STRING>` in
`conquest/CTW_Alexander_Map_01_strings.xml.*`.

`parse` is *also* a lexer keyword (rule 25) whose action sets a flag that pushes its string
argument through the same localisation path — a second, implicit entry point.

`$NUM`, `$STRING`, `$d`, `$s`, `$NAME`, `$SCORE` have **no lexical role**; they live only
inside string literals and are `String::parse` (`0x00a1a0a0`) format specifiers. The
alphabetic part is documentation and **the trailing digit is the 0-based argument index**,
which is why `$NUM0`, `$STRING0`, `$d0` and `$s0` all work identically. Corpus counts:
`$NUM0` 742, `$STRING0` 632, `$d0` 454, `$s0` 289.

---

## 7. `enable_trigger` / `disable_trigger` are keywords

Flex rules 19 and 20, tokens `0x12c` and `0x12d`. They are **not** among the engine's 873
registered builtins and are not in `ron-data/scriptfunctions.xml` either, yet the corpus
calls them 2,081 times. They lower to the two opcodes that touch `Script::trigger_bits`:
`OP_BIT_UNSET` (0x3d, `bts` — sets the bit) and `OP_BIT_SET` (0x3c, `btr` — clears it),
whose operand is a *literal trigger index* only a compiler can derive from a name. The
asymmetry is the tell: `is_trigger_enabled(String)` **is** a registered builtin (index 17),
because a by-name lookup at runtime needs `Script::trigger_names`.

The argument is a trigger *name*, written either as a string literal (the usual form) or as
a **bare identifier**: `scenario/Scripts/Auto_Pause/auto_pause.bhs` has
`disable_trigger(pause_time_up)` where `pause_time_up` is a trigger declared later in the
same script and is not a variable anywhere.

**Triggers start enabled.** Three independent corpus arguments: 146 `disable_trigger` calls
sit inside `run_once` initialisation blocks, which is meaningless unless the default is
enabled; 31 triggers are disabled and never enabled, which would make them permanently
dead otherwise; and `auto_pause.bhs` disables one in `run_once` and enables it later. The
initial bits are compiled into the `load_trigger` chunk (tag 5, `0x009c4f50`), so this is
confirmable against retail bytecode in one shot.

---

## 8. Expressions and operator semantics

Precedence, lowest first: assignment (right-assoc) < `||` < `&&` < `|` < `^` < `&` <
`== !=` < `< > <= >=` < `<< >>` < `+ -` < `* / %` < `**` (right-assoc) < unary
`! - ~ ++ --` < postfix `() [] . ++ --`.

Caveats: `**`'s precedence is **unattested** (no `**` in code anywhere), and retail merges
`&`/`&&` and `|`/`||` into single tokens so their relative precedence cannot be as written
here — see §1.

Postfix forms:

- `a[i]` — array index. `OP_PUSH_ARRAY_INDEX` pushes the *element slot*, so a write through
  an index aliases the array.
- `s.field` — struct field, by index; field order in the declaration **is** the layout.
- `a.length` — array length, not a field (`OP_PUSH_ARRAY_LENGTH`). Note the separate
  builtin `length(String)` for strings.
- `g.add_to_group(u)` — receiver-first sugar for `add_to_group(g, u)`. 492 uses. The
  receiver is a `group`-typed value; `add_to_group`, `clear_group`, `group_move_order`,
  `remove_from_group` and friends are all ordinary builtins whose first parameter is
  `ScriptTy::Group`.
- `(int)x`, `(float)y`, `(void)f()` — C-style casts, 144 uses, always to a scalar.

**Argument evaluation is left to right.** `VirtualMachine::call_func` pops `argc` values
and reverses them. This is observable wherever an argument draws RNG — `rand_int` is
builtin 9 and the corpus calls it 566 times — so it is not a free choice.

### Operator semantics, from `ScriptInt::do_operator` `0x009d7760`

| operator | machine |
|---|---|
| `/` `/=` | `idiv` — **truncation toward zero, identical to Rust**. Divide by zero calls `run_time_error(L"Can't Divide 0")` and yields **0**; execution continues. `INT_MIN / -1` is an unguarded `idiv` and traps. |
| `%` `%=` | remainder of the same `idiv` — **sign of the dividend, identical to Rust**. Same zero handling. |
| `**` `**=` | `cvtdq2ps` both operands, CRT `powf` in **binary32**, `cvttss2si` back. One of the seven non-IEEE transcendental hazards flagged in `README-LLM.md`. |
| `>>` `>>=` | `sar` — **arithmetic**, not logical. |
| `<<` `+` `-` `*` | `shl` / `add` / `sub` / `imul`, wrapping. |
| `!` | `v <= 0` (`cmp v,0; setle`) — **not** `v == 0`. |
| `&&` `\|\|` (on ints) | `v > 0` (`cmp v,0; setg`), not a general truthiness test. |
| `~` | `not`. |

`ScriptString::do_operator` (`0x009d6b10`) handles ops `0x00`–`0x0b` only: `+` is
concatenation, `+=` appends in place, `<` `>` `<=` `>=` are lexicographic, `!` is
"is empty", `&&`/`||` are "both/either non-empty". Anything above `0x0b` is a runtime error
returning a fresh empty string.

**There is no runtime coercion.** Both classes begin with
`if (rhs && rhs->type != this->type) { run_time_error(…); return this; }` — a mismatched
pair errors and leaves the left operand *unchanged*. The compiler is expected to have
inserted the conversion (`SyntaxNode::auto_cast` `0x009dfee0`), whose failure message is
`"Can't convert \"$TYPE0\" to \"$TYPE1\". Explicit cast required"`. That the corpus writes
`(float)` casts by hand around int/real mixing is evidence that `auto_cast` declines that
particular pair.

---

## 9. Structs

```bhs
struct UnitGroup {
  int nation;
  int[] units;          // dynamic array field
};
```

Three declarations exist, all in `scenario/scriptlibrary/game_structs.bhs`: `Vector`
(three floats), `UnitGroup`, and `ConquestDiploOffer` (18 fields, six of them `string[]`).
Field order is the layout. `int grid[8]` (a fixed count) is grammatical but unattested.

---

## 10. What the language does not check

Almost nothing at the type level, and the engine is the reason: every value is a
`ScriptType*` whose accessors are virtual coercions and whose every operator funnels
through one virtual `do_operator`. Declared types are **allocation** information — they
pick the `OP_CREATE_SIMPLE` type tag, hence the initial value and the coercion behaviour —
not a checked contract. 51 shipped parameters have no type at all and 14,356 variables are
never declared.

What *is* constrained: `include` targets must exist, called names must resolve to a script
or a registered builtin, builtin **arity** must match one of that name's registrations (36
of the 873 names are overloaded), and operand types must match at `do_operator` time or be
cast by the compiler.

---

## 11. Ambiguities this document cannot resolve

Each needs either retail bytecode to diff against or a run under the oracle.

1. **`labels` starting value.** The corpus argues overwhelmingly for 1. Not read from the
   compiler.
2. **`&`/`&&` precedence.** Retail merges the tokens; our parser separates them. No corpus
   expression mixes them, so no shipped file distinguishes the readings.
3. **`**` precedence and associativity.** No `**` in code anywhere.
4. **The anonymous entry script's registered name.**
5. **`SyntaxNode::auto_cast`'s exact rules.** We reproduce only the string-on-the-left case
   the corpus needs (33 insertions across the whole corpus).
6. **Whether `$S` and a plain string literal are distinguishable at runtime.** They are not
   in the English build, where `localize_string` falls back to the literal.
