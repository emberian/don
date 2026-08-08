# Lane report: `ron-ai` — replicating the shipped Rise of Nations AI player

**Deliverables:** this report; `crates/don-ai/` (new workspace member, 13 tests green);
`crates/don-ai/data/script-functions.json` (842-entry script API table recovered from the
binary) and its generator `crates/don-ai/tools/dump_script_funcs.py`.

---

## Headline, and the tier it deserves

**The shipped .bhs scripts are a thin build-order sequencer, not the AI.** They are one
stage out of twelve in the *production* AI's round-robin, and production is one of **four**
independently switchable AI subsystems (production / combat / unit / city) — the other three
have no script surface at all. Replicating "the RoN AI" is therefore an order of magnitude
bigger than transcribing 2,400 lines of BHS: the 2,400 lines are the cheap part and they are
now done. `[measured]`

**Confirmed in the running game, not just in Ghidra.** A live read of `riseofnations.exe`
pid 148 mid-skirmish shows the AI player's production-script `RString` at `player+0x6EA4`
resolving to the eight characters **`economic`**, its `ref int step` at `player+0x790`
reading **35**, its human/console flag bit clear, and its four AI-subsystem bits all
enabled — with the human player's script field all zeros. Full dump and derivation in §3.5.
`[measured]`

**The AI does cheat, and the cheat is a flat multiplier on resource income.**
`FUN_006D66A0` returns a per-difficulty percentage — **−35 / −15 / −7 / 0 / +25 / +50** for
Easiest…Toughest — and `FUN_006CE450`, the six-resource income accumulator, applies it as
`income = ((pct + 100) * income) / 100`. The human (console) player always gets 0. The bonus
is applied *after* the un-bonused rate has been stored into the displayed gather-rate field,
so it does not show up in the AI's reported economy. `[Tier C — decompiled, integer-only
path. The plumbing around it is confirmed live (§3.5), but the live game was set to Tough,
whose bonus is 0%, so the non-zero entries themselves are still decompilation-only.]`

Fidelity of the Rust crate: **Tier C**. It is a transcription of shipped script source plus
disassembled engine glue. Nothing in this lane is differentially tested against retail, and
nothing is verified.

---

## 1. The BHS interpreter

### 1.1 It is a real compiler, in the shipped binary, running at load time

`riseofnations.exe` contains `script\lexer.cpp`, `script\compiler.cpp`,
`script\runtimeenv.cpp` and `scriptfunctions.cpp` as profiling markers, plus RTTI class
names `Script`, `ScriptFile`, `ScriptFunc`, `ScriptFuncSet`, `ScriptType`, `ScriptInt`,
`ScriptFloat`, `ScriptString`, `ScriptObject`, `ScriptArray`, `ScriptGameInterface`,
`ScriptLogWin` (`schema/vtables.json`). `.bhs` is a **source** extension in the engine's
file-type table at `0x00C06920`/`0x00C06B18`/`0x00C06EC0`/`0x00C07190`/`0x00C07610`, and
`ai\scripts\` (UTF-16, `0x00C07AF4`) is one of the search-path roots. There is a
`"Script failed to compile"` string at `0x00AD4954`. So the game **parses the shipped .bhs
text at runtime**; there is no precompiled form in `ron-data/`. `[measured]`

| thing | address | evidence |
|---|---|---|
| lexer: open source file, walk include path | `FUN_009BFF30` | `_wfsopen(name, L"rb")`, `"script\\lexer.cpp"` at lines 0x154/0x178/0x188 |
| compiler entry (`Compile(RString&, int)`) | `FUN_009BF160` | `"script\\compiler.cpp"`; 30 call sites across the binary |
| runtime environment / executor | `FUN_009C3B00` | `"script\\runtimeenv.cpp"` |
| `RuntimeEnv::Run(name, …)` | `FUN_009C4460` | compiles, resolves (`FUN_009C6C80`), executes (`FUN_009C3B00`) |
| call-by-name thunk used by the game | `FUN_0043D0E0` | `Run(env, RString* name, int nargs, cell…)` |
| script-function registration table | `FUN_009C7570`, 52,300 bytes | see below |
| `ScriptFuncSet::AddFunc` | `FUN_009D4DA0` | `(this, tag, name, impl, arity) -> ScriptFunc*` |
| `ScriptFunc::AddParam` | `FUN_009D4F20` | `(this, tag, name, 0, 0)` |

**Note the include path is a real search path**: `FUN_009BFF30` tries the current directory,
then iterates `this+0xD4` entries of a 20-byte-stride path list. `economic.bhs` line 13 is
`include "aibestbuildlibrary.bhs"`, and the library resolves through that list.

### 1.2 The host API: 842 functions, recovered whole

The entire script API is registered in one 52 KB function that Ghidra refuses to decompile.
Registration is a fixed instruction pattern, so it can be lifted at the instruction level:

```
push  <arity> ; push <impl VA> ; push <name, UTF-16> ; push <type tag>
mov   ecx, <ScriptFuncSet*> ; call 0x009D4DA0        ; -> ScriptFunc* in eax
push  ecx ; push 0 ; push 0 ; push <param name> ; push <type tag>
mov   ecx, eax ; call 0x009D4F20
```

`crates/don-ai/tools/dump_script_funcs.py` walks it and emits
`crates/don-ai/data/script-functions.json`: **842 registrations**, each with name,
implementation VA, arity, and every parameter's name and type tag. 35 names are genuine C++
overloads distinguished by arity (e.g. `set_no_attack` at 2/3/4/5 params; `ping_group` for a
player, a group name, and a group object). Type tags: `0x00057BAD` = int, `0x00168174` =
string, plus five rare tags left unresolved.

Reproduce:
```sh
cd /Users/ember/dev/don/ron-bin && uv run --quiet --with capstone --with pefile python \
  ../crates/don-ai/tools/dump_script_funcs.py > ../crates/don-ai/data/script-functions.json
```

**Why a naive search finds nothing:** the names are UTF-16 literals in `.rdata`
(`num_cities` at `0x00B08810`). `strings riseofnations.exe | grep num_cities` returns
nothing, which is presumably why this table has not been lifted before.

**Cross-check that could have failed, and didn't.** `ron-data/scriptfunctions.xml` is an
independent artifact (the scenario editor's catalogue, edited in XMLSpy by BHG staff). For
the **430 names that appear exactly once in each**, the binary-derived table and the XML
agree on **arity and on every parameter name in order, 429/430**. The one disagreement is
`file_write_text`, where the XML names the second parameter `any_value` and the binary
`str_value`. The XML's fine-grained parameter types collapse cleanly onto the binary's two
tags — XML types 6/9/10/11/12/13/20/21 → int, 24/26/27/32 → string, with no crossovers.
`[measured]`

The XML's `func_index` is **not** the registration order (the offset drifts monotonically as
overloads and EE-era additions accumulate — 842 registered vs 626 catalogued), so the XML
cannot be used to index into the engine.

### 1.3 Host-function calling convention and failure discipline

`[measured, disassembly]` Implementations are `__stdcall` and receive **one pointer per
declared parameter**, each pointing at a value cell:

```asm
get_leader_difficulty  0x009E53B0:
    mov eax,[ebp+8] ; mov eax,[eax]        ; who = *arg0
    dec eax ; cmp eax,7 ; ja fail
    imul eax,eax,0x6eec
    mov edx,[eax+0xE3A390]                 ; player flags
    test ... 1 ... 2 ...
    mov eax,[eax+0xE3A3E0]                 ; player+0x50
    ret 4
fail: or eax,0xFFFFFFFF ; ret 4
```

String results are written through a hidden first pointer (`find_city_with_num`
`FUN_009EFF00` returns `param_1` after `FUN_00A1D590`).

Every player-taking host function opens with the same guard: `who - 1 < 8`, then
`player.flags0 & 1` (slot active) and usually `& 2` (in play), and returns **`-1`** if it
fails. Name→id lookups scan a **0x326 (806) entry** table and also yield `-1`.

**Player record**: base `0x00E3A390`, stride `0x6EEC`, 8 slots. This falls straight out of
every host function and is the single most reusable fact in this report.

### 1.4 Where I stopped

I did **not** extract the interpreter's expression evaluator. There is no large opcode
switch anywhere in `0x009C0000–0x009C7570`, and the value types are RTTI classes with
vtables, which points at an AST-walking interpreter over `ScriptInt`/`ScriptString`/… nodes
rather than a bytecode VM — but I did not confirm that, and I did not recover operator
precedence, short-circuit behaviour, integer truthiness, or the `trigger`/`enable_trigger`
mechanism. See §5.

---

## 2. Division of labour: the scripts are one twelfth of one quarter

### 2.1 Four AI subsystems, each independently switchable

`[measured]` The script API exposes exactly four AI on/off pairs, and they map to bits in
the **second** player flag word at `player+0x04` (`0x00E3A394 + who0*0x6EEC`). A *set* bit
means DISABLED.

| subsystem | bit | enable | disable | other script hooks |
|---|---|---|---|---|
| unit AI | `0x02` | `FUN_009FF8A0` | `FUN_009FF8E0` | `enable_unit_ai`/`disable_unit_ai` per unit |
| **production AI** | `0x04` | `FUN_009FF5E0` | `FUN_009FF7A0` | **`load_production_script(who, filename)` `FUN_009FF7E0`** |
| combat AI | `0x08` | `FUN_009FF820` | `FUN_009FF860` | — |
| city AI | `0x10` | `FUN_009FFBA0` | `FUN_009FFBF0` | — |

`load_production_script` is the giveaway: a .bhs file is *the production script*. There is no
`load_combat_script`, no `load_city_script`. Combat, unit and city AI have **zero** script
surface: they are compiled C++, full stop.

Also `[measured]`: `player.flags0` bit 0 = slot active, bit 1 = in play, bit 2 =
human/console-controlled (`num_ai_players` `FUN_009E5EB0` counts active-and-not-bit2;
`change_to_ai` `FUN_009E5D20` clears bit 2).

### 2.2 The production AI is a twelve-stage round robin; the script is stage 1

`FUN_006C1960` — `Player::ProductionAiTick`, `__thiscall`, `this` = the player record.
Decompilation is in `re/decomp-all/006c1960.c`. Structure:

```c
if ( ((flags0 & 4 /*human*/) && !(flags0 & 8)) || *(int*)PTR_00C061C4 != 0
     || (flags1 & 4 /*production AI disabled*/) )
    { player[0x788] = 0; return; }                    // stage := Idle

player[0x9E0] = FUN_006CE000() + player[0x940] + 1;

if (player[0x788] == 1 && (player[0x78C] == 0 || Game[0x2D] == 8))
    player[0x788] = 2;                                // skip the script stage

switch (player[0x788]) {
case 1:                                               // ---- THE BHS SCRIPT ----
    step_cell = new ScriptRefInt(player[0x790]);
    nl_cell   = new ScriptInt(5);                     // num_loops   == 5, hard-coded
    bvr_cell  = new ScriptInt(player[0x6DD4] + 2);    // boom_vs_rush
    who_cell  = new ScriptInt(player[0x008] + 1);     // who = index + 1
    r = ScriptRunByName(&g_RuntimeEnv, &player[0x6EA4], 4,
                        who_cell, step_cell, bvr_cell, nl_cell);
    if (r == 0) {                                     // the script ran
        rv = ScriptReturnValue();                     // FUN_0047D9E0
        player[0x790] = step_cell->value;             // write back `ref int step`
        if (rv == 1) { player[0x788] = 0; return; }   // BLOCK_ON_THIS
        if (rv != 3) goto advance;                    // not SCRIPT_DONE
    }
    player[0x78C] = 0;                                // SCRIPT_DONE clears "has work"
advance:
    player[0x788]++;  return;
case 2:  player[0x788]++; FUN_006C83E0(); FUN_006C9DB0(); return;
case 3:  player[0x788]++; FUN_006C7A60(); break;
case 4:  player[0x788]++; FUN_006C6BA0(); break;
case 5:  player[0x788]++; FUN_006C6430(); break;
case 6:
case 9:  player[0x788]++; FUN_006C40A0(); break;
case 7:  player[0x788]++; FUN_006C1BE0(); break;
case 8:  player[0x788]++; if (FUN_006C8AF0()) return;
         if (Game[0x2D] == 8) return; player[0x788] = 0; return;
case 10: player[0x788]++; FUN_006C1BE0(); return;
case 11: FUN_006C8AF0();                              // falls through
default: player[0x788] = 0; return;
}
if (Game[0x2D] == 8) { FUN_006C8AF0(); FUN_006C9DB0(); }
```

**The strongest evidence in this lane is right there.** `economic.bhs` opens with

```
labels { BLOCK_ON_THIS = 1, DONT_BLOCK_ON_THIS, SCRIPT_DONE, }
```

i.e. 1, 2, 3 — and the engine, in a function I found by an entirely independent route
(following `enable_production_ai`'s flag bit and then `ScriptRunByName` call sites),
compares the script's return value against exactly `1` and `3`, treating everything else as
"advance". Two independent artifacts, same three-value protocol. `[measured]`

**Semantics of the three return values** `[measured]`:
- `BLOCK_ON_THIS (1)` — the stage counter is reset to 0. The **remaining ten compiled
  stages are skipped this cycle.** The script is saying "I am saving for this; do not let
  the rest of the AI spend."
- `SCRIPT_DONE (3)` — clears `player+0x78C`, and the top-of-function test then *skips the
  script stage* on every subsequent cycle until something sets that flag again. This is how
  a build order retires.
- anything else, including `DONT_BLOCK_ON_THIS (2)` — advance to stage 2 and run the
  compiled stages.

So on a normal cycle the script gets to veto the entire compiled production pipeline. That
is real authority — but it is authority over *production only*, and only through a 1-bit
channel plus whatever `place_*`/`train_*`/`research_*` calls it made first.

**Ten compiled stages, unnamed.** I did not characterise `FUN_006C83E0`, `FUN_006C9DB0`,
`FUN_006C7A60`, `FUN_006C6BA0`, `FUN_006C6430`, `FUN_006C40A0`, `FUN_006C1BE0`,
`FUN_006C8AF0`. Two observations only: `FUN_006C83E0` reads the effective difficulty, the
player's age (`[player+0x6EB8]+0xDC ^ 0x62766`) and the per-age tech table at rule index
`0x220+age`, and contains a `11000` fallback — it looks like a needs/priority evaluation.
`FUN_006C6BA0` also branches on difficulty (`< 4`) and on a per-resource flag word at
`player+0x450`. **This is the single biggest remaining unknown for AI replication.**

### 2.3 What answers "how much effort is replication?"

The BHS layer decides: *which building to place next, in which city; which tech to research
next; how many citizens to have; when to found city #2 and #3*. That is it. It is ~2,400
lines and it is now transcribed.

Everything else — where a building physically goes, which citizen is assigned to which
gather slot, all unit micro, all army composition and attack decisions, city defence,
diplomacy — is compiled C++ reached through the ~50 host functions. `place_building_with_cost`
is a *one-line* script call and a large engine search. **Replicating the shipped AI's
observable play is dominated by the compiled side by an order of magnitude, exactly as the
lane brief suspected.**

---

## 3. The AI decision loop, difficulty scaling, and the cheat

### 3.1 Scheduling against the sim tick

`Player::UpdateAI` @ **`0x006B9620`** (11,058 bytes, `skipped_large`; read at the
instruction level). Prologue, verbatim:

```asm
0x6b9629  mov  eax, 0xC8                    ; 200
0x6b962e  mov  ecx, [0x00C061C0]            ; -> game speed cell
0x6b9636  imul esi, [ebx+8], 0x19           ; player_index * 25
0x6b963a  idiv dword ptr [ecx]              ; period = 200 / game_speed
0x6b963d  mov  edi, [0x00C061EC]            ; Game
0x6b9648  mov  edi, [edi+0x550]             ; tick
0x6b964e  add  esi, edi
0x6b9653  idiv ecx                          ; edx = (idx*25 + tick) % period
0x6b9655  cmp  dword ptr [ebx+0x788], 0     ; ai stage
0x6b965e  je   0x6b966e
0x6b9660  mov  ecx, ebx
0x6b9662  call 0x6c1960                     ; ProductionAiTick, one stage
0x6b966d  ret
0x6b966e  test edi, edi   ; je 0x6b96f9     ; tick == 0 -> start-of-cycle path
0x6b9676  test esi, esi   ; je 0x6b96f9     ; phase == 0 -> start-of-cycle path
0x6b967a  ...                               ; esi %= 30
0x6b9698  jne  0x6bc0f7                     ; not a multiple of 30 -> rest of Player::Update
0x6b969e  ...                               ; age/tech check
```

`[measured]` Therefore, at game speed 1:

- **AI period = 200 ticks**, divided by the game-speed cell at `[0x00C061C0]` (the same cell
  `FUN_006CE450` uses to multiply resource income, so it is a genuine speed factor).
- **Players are staggered 25 ticks apart** — 8 players × 25 = 200 = exactly one period, so
  each AI player starts its cycle on a distinct tick and no two ever start together.
- Once a cycle is running (`stage != 0`), **one stage runs per sim tick**, so a full
  12-stage cycle occupies 11 consecutive ticks and the script runs on the **first** of them.
- Between cycles, an **age/tech check runs every 30 ticks** of the phase.

That gives a clean answer to "how is the AI scheduled against the sim tick": it is a
tick-driven, per-player-staggered, one-stage-per-tick state machine with a 200-tick period,
not a per-frame or per-second job.

`SYSTEM_INCREASE_AI_SPEED` / `SYSTEM_DECREASE_AI_SPEED` exist as debug keybinds
(`ron-data/playerprofile.dtd`), consistent with `[0x00C061C0]` being tunable.

### 3.2 Difficulty: where it is stored and who reads it

`[measured, disassembly]`

| function | address | behaviour |
|---|---|---|
| `set_difficulty(d)` | `0x009E52F0` | accepts `1..=6`, stores `d-1` into `byte [Game+0x2B]` |
| `get_difficulty()` | `0x009E5320` | returns `Game[0x2B] + 1`; a campaign object at `[0x00C0617C]+0x1328` overrides it |
| `set_leader_difficulty(who,d)` | `0x009E5350` | accepts `1..=6`, stores **`d`** into `player+0x50` |
| `get_leader_difficulty(who)` | `0x009E53B0` | returns `player+0x50` raw |

⚠ **The two setters disagree on base.** Global difficulty is stored 0-based; per-leader
difficulty is stored 1-based; and the consumer below compares against `0..=5`. Either
`set_leader_difficulty` is off by one in the shipped game, or the UI writes `player+0x50`
with a different convention than the script function does. **I could not settle this**, and
it matters for any scenario that calls `set_leader_difficulty`.

`Player::GetEffectiveDifficulty` = **`FUN_006EC000`** (16 call sites), and the same idiom is
inlined at 22 more:

```c
if ((Game[0x820] & 4) == 0) {
    if ( (!(Game[0x821] & 0x10) && !(Game[0x822] & 2)) || (Game[0x822] & 2)
         || (d = player[0x50]) < 0 )
        return Game[0x2B];              // global
} else d = player[0x50];                // per-player
return d;
```

so per-player difficulty is used only when `Game[0x821]&0x10` is set, `Game[0x822]&2` is
clear, and `player+0x50 >= 0` (i.e. `-1` means "unset"), or unconditionally when
`Game[0x820]&4` is set. I did not name those three flag bits.

### 3.3 The resource cheat, quantified

**`FUN_006D66A0` — `Player::GetDifficultyBonusPercent`:**

```c
if (flags0 & 4 /* console/human */) {
    if (Game[0x820] & 4) return HandicapPercent();   // FUN_006DA740
    return 0;                                        // humans get nothing
}
switch (GetEffectiveDifficulty()) {
  case 0: return -35;   // Easiest
  case 1: return -15;   // Easy
  case 2: return  -7;   // Moderate
  case 3: return   0;   // Tough      (fall-through initialiser)
  case 4: return  25;   // Tougher
  case 5: return  50;   // Toughest
}
```

Difficulty names come from `ron-data/rules.xml` `<CATEGORIES id="difficulties">`:
Easiest / Easy / Moderate / Tough / Tougher / Toughest, in that order, and
`set_difficulty` accepts 1..6 into that list.

**Its only consumer is `FUN_006CE450`**, the per-tick resource accumulator. It loops the six
resources (`0..=5`), and, after storing the un-bonused rate into the display field:

```c
*(u32*)(player[0x6EB8] + 0x94 + res*4) = rate ^ 0x90236;   // displayed rate, NO bonus
pct = GetDifficultyBonusPercent();
if (pct != 0) rate = ((pct + 100) * rate) / 100;           // <-- the cheat
... further multipliers ...
gain = rate / (RULES[0x27C] * 16);
stockpile += gain;                                          // stored ^ 0x8221
```

So: **Toughest AI gathers 1.5× a human's rate at the same buildings; Easiest gathers 0.65×.
The bonus is invisible in the AI's own displayed gather rate.** `[Tier C]`

Independent handicaps also exist and are *not* AI-only:
`ron-data/rules.xml` `<CATEGORIES id="handicaps">` is `Standard=0, Skill +1=5, Skill +2=10, …`
(`5 * N`), indexed by `player+0x4C` into a 0x58-byte-stride table at `[0x00E80138]+0x3C`
(`FUN_006DA740`). `FUN_006508C0`, a cost function, applies it as `cost = ((200 - h) * cost) / 200`.
`FUN_006DA740` additionally shifts the handicap index by difficulty when
`Game[0x820]&4 == 0`: Easiest gives the **human** `+10` steps, Easy `+5`; Tougher gives the
**AI** `+10`, Toughest `+16`.

### 3.4 A confirmation the obfuscation story is right

Player counters are XOR-masked in memory. `age` is `[player+0x6EB8]+0xDC ^ 0x62766`;
the resource stockpile array is `[player+0x6EB8]+res*4 ^ 0x8221`. In `FUN_006CE450`, when
`Game[0x2D] == 8` the engine writes the literal `0x104BE` into the stockpile and zeroes the
rate. `0x104BE ^ 0x8221 = 99999`. And `Game+0x2D` is the *starting resources* setting, whose
index 8 in `ron-data/rules.xml` `<CATEGORIES id="startingresources">` is **"Infinite"**.
A literal, a mask and a data file that were all found separately agree. `[measured]` —
this is a unit test in `crates/don-ai/src/abi.rs`.

`Game[0x2D] == 8` also makes `FUN_006C1960` skip the BHS script stage entirely, so
**an "Infinite resources" game runs the AI with no build-order script at all.**

### 3.5 Live confirmation in the running game `[measured]`

The Parallels VM had `riseofnations.exe` (pid 148) mid-game. Reading its memory with
`OpenProcess`+`ReadProcessMemory` (image base `0x00D60000`, so rebase by `0x00960000`)
confirms the structure above directly, not by inference:

```
delta      = 0x960000
P1head     = 13 00 08 00 | 00 00 00 00 | 01 00 00 00 | 05 00 00 00
P0ai       = 00000000 00000000 01000000        (player+0x788,0x78C,0x790)
P1ai       = 00000000 00000000 23000000
P1name     = 6C817B23 00000000 08000200 ...    (RString at player+0x6EA4)
P0name     = 00000000 00000000 00000000 00000000
P0cities   = 02000000    P1cities = 02000000   (player+0x3F8)
P1diff     = 03000000    P1bvr    = FFFFFFFF   (player+0x50, player+0x6DD4)
*0x237B816C= 88F6D033 08000000 0F000000        (buf=0x33D0F688 len=8 cap=15)
*0x33D0F688= 65 00 63 00 6F 00 6E 00 6F 00 6D 00 69 00 63 00   ->  "economic"
game+0x2A.. = 00 03 02 01 01 03 01 05          (Game+0x2B = 3, Game+0x2D = 1)
game+0x550 = 71280000 = 10353                  (tick)
game+0x820 = 08 01 00 00
*[0x00C061C0] = 01000000                       (game speed = 1)
```

Read off that:

- **`player+0x6EA4` really is the production script name, and the live AI is running
  `economic`.** Eight UTF-16 characters, `e c o n o m i c`, reached through an `RString`
  whose header is `{ wchar_t* buf; u32 _; u16 len; u16 cap }` with `len = 8`. The human
  player's field is all zeros. This is the fact that turns §2.2 from a decompiled
  hypothesis into a measured one.
- **`player+0x790` is the live `ref int step`** and reads **35** for the AI player — inside
  `economic.bhs`'s 1..36 `switch` range, and 35 is its `//dock step`, which is reached from
  step 14 when `sea_map > 0`. The human player's reads 1, its initial value: the production
  tick returns immediately for a player whose `flags0` bit 2 is set, so a human's step never
  advances. A `step` outside 1..36 would have refuted the whole mapping; it is inside.
- **`player+0x788` (AI stage) reads 0** for both players. With `tick = 10353`, speed 1 and
  player index 1, `phase = (1*25 + 10353) % 200 = 178`, which is neither 0 nor a multiple of
  30 — exactly the case where §3.1 says the scheduler does nothing. Self-consistent.
- **Player flags**: human `flags0 = 0x00080707` (bits 0,1,**2**), AI `flags0 = 0x00080013`
  (bits 0,1,4 — **bit 2 clear**). Confirms bit 2 = human/console.
- **`flags1 = 0x00000000` for both** — all four AI subsystems enabled, as expected in a
  normal skirmish.
- `player+0x08` reads 1 for the second player: the 0-based index, as the decompilation says.
- `player+0x3F8 = 2` for both: `num_cities`.
- `player+0x6DD4 = -1`, so `boom_vs_rush` handed to the script is `-1 + 2 = 1`. **And
  `economic.bhs` never reads its `boom_vs_rush` parameter**, so this channel is dead in the
  shipped economic script (`defensive.bhs` takes the same parameter).
- `Game+0x2B = 3` = **Tough**, and `Game+0x2D = 1` = **Standard** starting resources — both
  consistent with `<CATEGORIES id="difficulties">` and `<CATEGORIES id="startingresources">`.
  `Game+0x820 = 8` so `&4 == 0`, and `Game+0x821 = 1` so `&0x10 == 0`: the effective
  difficulty comes from the global byte, which matches `player+0x50 = 3` here.
- `[0x00C061C0] -> 1`, so the AI period in this game is `200 / 1 = 200` ticks.

Reproduce (guest, as SYSTEM):
```sh
prlctl exec "Windows 11" powershell.exe -NoProfile -EncodedCommand <base64 UTF-16LE of>
  Add-Type -TypeDefinition '<ReadProcessMemory P/Invoke wrapper>'
  $p = Get-Process riseofnations; $h = [M]::OpenProcess(0x10,$false,$p.Id)
  $d = ([int64]$p.MainModule.BaseAddress) - 0x400000
  [M]::Rd($h, (0x00E3A390 + $d + 0x6EEC + 0x6EA4), 16)     # then chase the RString
```
Script: `crates/don-ai/tools/live_probe.ps1`. **Keep each invocation to under ~10
reads** — a PowerShell script issuing ~90 `ReadProcessMemory` calls against the emulated x86
process never returned inside 5 minutes, while 7 reads return in ~40 s. That is a new
instance of the README's "never scan memory in a PowerShell loop" rule, and the working
budget is much smaller than it sounds.

### 3.6 What the AI reads each tick

Rather than guess, here is exactly what `economic.bhs` observes, i.e. the state the
production layer is a function of: city count and city names/ids; buildings per city by type
(`num_city_buildings`, `find_build_at_city`) and unfilled worker slots per building
(`max_workers_at_building` − `num_workers_at_building`); unit counts including queued
(`num_type_with_queued`), idle counts (`find_num_idle_unit`); population; age; tech
possession and in-progress research; per-resource stockpile thresholds
(`at_least_type(who, N, "Wealth"|"Timber")`); affordability (`can_pay_cost`); whether any
city has ever been attacked or raided; nation, map style, starting town size, starting
resources, no-nation-powers, conquest-scenario, and rare resources seen.

Notably it never reads enemy positions, army strength, or the map. Threat response in the
production script is a single binary: `was_city_attacked(who,"",-1) → build a Barracks`.

---

## 4. `crates/don-ai/` — the executable model

New workspace member, no dependencies, 11 tests, `cargo test -p don-ai` green.

| file | what |
|---|---|
| `src/abi.rs` | engine contract: `ScriptResult`, `AiStage` (12), `AiSubsystems`, `Difficulty` + income-bonus table, `schedule::{period,phase,starts_cycle}`, player offsets, XOR masks. All doc-commented with the address they came from. |
| `src/api.rs` | `trait ScriptWorld` — the 50 host functions the shipped scripts call, with each implementation VA, exact parameter names and types from the binary. |
| `src/library.rs` | `aibestbuildlibrary.bhs`: `place_farm`, `place_mine`, `place_dock`, `place_woodcutter`, `train_unit_with_need`, `woodcutter_check`, and honest stubs for `assign_idle` / `city_placement`. |
| `src/economic.rs` | `economic.bhs` end to end: guards, the "ghetto array" of per-player statics, sea-map classification, the CtW large-start branch, per-call maintenance, the hang watchdog, and the 36-case build-order machine driven `num_loops` times. |
| `src/probe.rs` | a recording test double. **Not a simulation** — it logs calls and answers from a table so control flow can be tested. |
| `data/script-functions.json` | the 842-entry API table. |
| `tools/dump_script_funcs.py` | its generator. |
| `tools/live_probe.ps1` | the guest-side memory probe used for §3.5, with the read-budget warning baked in. |

Design decision: the crate models **the script layer and its engine contract**, and stops at
the `ScriptWorld` seam. Implementing `place_building_with_cost` would mean inventing the
engine's building-placement search, which this lane has not derived — so it is a trait
method, not a guess.

Faithfulness choices worth naming:

- **Early `return SCRIPT_DONE` inside the state machine skips the write-back** of
  `prev_step`/`needed_citizens`/`timer_started`/`fishermen_total` — the save-back switch is
  after the loop. That is load-bearing behaviour and is preserved, with a comment saying so.
- **`static int needed_techs` is genuinely shared across all eight players** (BHS `static`
  is per script function; that is exactly why the authors hand-rolled the per-player
  arrays). Preserved, with a test.
- Redundant re-queries (`num_cities(who)` called twice in one expression), dead locals
  (`player_age`, `build_merchant`, `boom_vs_rush` — **the `boom_vs_rush` parameter is never
  read by `economic.bhs`**), and the odd `place_mine` typo are all reproduced.

### Shipped bugs reproduced, not fixed

1. **`"Citizens"` is not a unit type.** `economic.bhs` calls
   `train_unit_with_need(who, needed_citizens, "Citizens")` at lines 475, 510, 568, 674, 762
   (steps 8, 11, 16, 22, 27) — but the token `Citizens` appears nowhere in
   `ron-data/typenames.xml` or `ron-data/unitrules.xml`; only `Citizen` does. Step 4 uses the
   correct `"Citizen"`. Name lookups that fail return `-1`, so those five steps train
   nothing. `[measured on the data files + the decompiled failure path; not confirmed live]`
2. **Truthiness on a `-1` sentinel.** `place_mine`/`place_dock` test
   `else if (find_inactive_build(who, "Small City"))` rather than `> 0`. If BHS truthiness is
   C's, `-1` is *true* and the branch is taken when there is no city under construction,
   which then skips the whole orphan-placement sweep. `city_placement` and `economic.bhs`
   step 1 have the same shape. This is contingent on §5.1.
3. **`can_pay_cost(who, "Dock")` in `place_mine`'s three-city branch** — should be `"Mine"`.
4. **`(my_capital > -1)`** in `economic.bhs` step 2 compares a `String` against `-1`.
   Modelled as "the capital name is non-empty" and flagged.
5. **`set_timer(who, 300)`** passes an int where the binary declares a `String timer_id`.

---

## 5. What I could **not** establish

1. **BHS integer truthiness.** `if (f(...))` where `f` can return `-1` occurs throughout,
   and the interpreter's rule decides real behaviour (see bug 2 above). The crate routes
   every such site through `api::bhs_true`, currently `v != 0`, so the assumption is
   swappable in one place. **Unverified.**
2. **`trigger` / `enable_trigger` semantics.** `city_placement` in the library `return -1;`s
   immediately and arms two `trigger` bodies; one of them `return 1;`s. How that value
   reaches `city_placement`'s caller — or whether it ever does — is not established. I left
   `city_placement` as a stub rather than invent a coroutine. This directly affects when
   cities 2 and 3 are founded, which is the most important decision in the whole build order.
3. **The ten compiled production stages** (`FUN_006C83E0`, `FUN_006C9DB0`, `FUN_006C7A60`,
   `FUN_006C6BA0`, `FUN_006C6430`, `FUN_006C40A0`, `FUN_006C1BE0`, `FUN_006C8AF0`). Not
   characterised. This is the largest gap for behaviour cloning.
4. **Combat / unit / city AI.** Untouched. One incidental measurement: unit-level AI is
   staggered per unit — `0x0064F858` gates on `(Game[0x550] + unit_field_0x0A) & 0x8000001F
   == 0`, i.e. **each unit re-evaluates every 32 ticks**, and several tactical branches are
   gated on `GetEffectiveDifficulty() > 1` (`0x00652585`, `0x00652724`), so **Moderate and
   above unlock tactical behaviours that Easiest/Easy do not have**. Not quantified.
5. **How a player is assigned a production script.** `player+0x6EA4` holds the script name,
   and `load_production_script(who, filename)` sets it, but I did not find the default
   skirmish assignment path. The three shipped scripts (`economic`, `defensive`,
   `aibestbuildlibrary`) are not named anywhere in the executable, so the choice is
   data-driven or UI-driven.
6. **The interpreter's evaluator**, operator precedence, `switch` semantics, implicit
   variable declaration, and `static` initialiser timing (assumed C: once).
7. **The `set_leader_difficulty` base mismatch** in §3.2.
8. **The difficulty income percentages themselves.** §3.5 confirms the *plumbing* live
   (player record, script name, step, flags, difficulty byte), but the live game was set to
   **Tough**, whose bonus is 0%. The −35/−15/−7/+25/+50 entries are still read out of the
   decompilation only. Confirming them needs either a game set to Toughest with the
   stockpile watched over N ticks, or an oracle call into `FUN_006D66A0` with a synthetic
   player record. **This is the highest-value remaining follow-up.**
9. **`Game+0x820/0x821/0x822` bit meanings**, and `Game+0x2F` (read live as 3; it scales
   Wealth income in `FUN_006CE450` and unit cost in `FUN_006508C0`).

---

## 6. Consequences for the RL lane

- **The baseline is beatable and cheap to beat on Moderate.** At difficulty ≤ 3 the AI has
  no income bonus at all (Tough = 0%, Moderate = −7%). An agent that matches the shipped
  build order and plays better tactically beats it without any economy edge.
- **Behaviour cloning from the script layer alone buys you a build order, not a player.**
  Everything the script emits is a `place_*`/`train_*`/`research_*` command; the interesting
  part of RoN play is in the compiled stages and combat AI. A cloned policy trained on
  script output will be an opening book.
- **The scripts are a legitimate curriculum.** They encode a hand-authored, per-nation,
  per-map-style opening with 36 named milestones. Using `economic.bhs` step transitions as
  a shaped reward or a scripted opponent for early self-play is cheap and now available in
  Rust.
- **`don-net` note:** `crates/don-net`'s two corpus tests fail on this tree. That crate is a
  concurrent lane's work, untouched by this one; `cargo test -p don-ai -p don-sim -p don-rules
  -p don-pe -p don-gpu` is green.

---

## Reproduction quick reference

```sh
# 842-function script API table
cd /Users/ember/dev/don/ron-bin && uv run --quiet --with capstone --with pefile python \
  ../crates/don-ai/tools/dump_script_funcs.py > /tmp/sfuncs.json

# the production stage machine
cat /Users/ember/dev/don/re/decomp-all/006c1960.c

# the difficulty bonus table and its consumer
cat /Users/ember/dev/don/re/decomp-all/006d66a0.c /Users/ember/dev/don/re/decomp-all/006ce450.c
cat /Users/ember/dev/don/re/decomp-all/006ec000.c   # effective difficulty

# the AI scheduler (Ghidra skips it; read the instructions)
cd /Users/ember/dev/don/ron-bin && uv run --quiet --with capstone --with pefile python - <<'EOF'
import pefile
from capstone import *
pe = pefile.PE("riseofnations.exe")
t = [s for s in pe.sections if s.Name.rstrip(b"\x00") == b".text"][0]
base = pe.OPTIONAL_HEADER.ImageBase + t.VirtualAddress
d = t.get_data()
for i in Cs(CS_ARCH_X86, CS_MODE_32).disasm(d[0x6b9620-base:0x6b9700-base], 0x6b9620):
    print(hex(i.address), i.mnemonic, i.op_str)
EOF

cargo test -p don-ai
```
