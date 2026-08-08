# Tooling: a function hook that logs every real damage computation

Lane: **hook-dll**. Builds the instrument `README-LLM.md` lists as the second-highest-leverage
unbuilt tool — an inline detour on `FUN_00644130` that turns ordinary play into ground-truth
`(attacker, defender, situation) -> damage` samples.

Every claim is marked **[measured]** (I ran it here, against the live process or the shipped
binary) or **[inferred]**. Nothing in this document is verified in the proof-assistant sense.

---

## Headline

**The hook is built, it is validated, and it has run against the retail game.** Approach 1 —
an injected DLL that rewrites the function prologue with a `jmp` to a trampoline — works under
the ARM64 x86 emulator, needs no debugger, and costs **≤230 ns per intercepted call**
[measured]. Approaches 2 and 3 were not needed and were not built; §7 says what is and is not
known about them.

The live corpus is **138,909 records over 56,789 real `FUN_00644130` calls**, spanning game
frames 20,338 → 28,738 of a running match, with **0 dropped**, **9 (0.0065%) frames that never
returned through the stub**, and **all seven patched sites verified byte-identical to the
shipped image after removal — 16 bytes each, read out of the live process and diffed against
`riseofnations.exe`** [measured]. It lives at
`/Users/ember/dev/don/schema/live/damage-hook/` (gitignored).

Four things the corpus establishes that were not previously known, and one thing it refutes:

1. **`[0x00C0AB84]` and `[0x00C0AEC0]` are not aliases.** `docs/derivation/damage-port.md` §5.6
   flags this as a load-bearing, untested assumption of the oracle harness. Live, the two
   tables return the same defender in **32,074 / 56,780 calls (56.5%)** and different values in
   **24,706 (43.5%)** — and the split is exact: they agree on **every** defender whose type id
   is < 364 and disagree on **every** defender whose type id is ≥ 364. In 24,198 of the
   disagreements the second table yields `0xFFFFFFFF` [measured]. §4.1.
2. **The `attack` and `armor` operands do not come from `Object::get_attack` /
   `Object::get_armor` in a real game.** Those vtable slots are overridden. Across 56,789
   damage calls, the `call [eax+0x124]` at `0x006441B1` and the `call [edx+0x120]` at
   `0x006441BE` dispatched to `0x00647DB0` / `0x006469F0` **zero times** [measured]. §4.2.
3. **Both upgrade paths were dead for the whole match.** The base `get_attack` returned exactly
   `UnitType[+0x1E8]` in **56,805 / 56,805** invocations and the base `get_armor` returned
   exactly `UnitType[+0x214]` in **24,706 / 24,706** [measured]. §4.3.
4. **Live combat barely touches the splash path.** `splash_flag` was non-zero in **28 of 56,780**
   damage calls (0.05%), and `FUN_00644130` has exactly **two** call sites in this match
   [measured]. §4.4.

**What this lane did not do: produce an agreement rate against
`crates/don-sim/src/mechanics.rs::damage`.** That comparison is not decidable from this capture,
and §6 states exactly why and exactly what would close it. Reporting a number here would have
required inventing the predicate vector, which is the failure mode the charter exists to
prevent.

---

## 1. What was built

Four files, all under `/Users/ember/dev/don/tools/damage-hook/`:

| file | what |
|---|---|
| `donhook.c` | the hook DLL: config-driven multi-target inline detour, per-thread shadow stack, lock-free ring, CSV writer thread, runtime removal and re-arm |
| `donject.c` | 32-bit injector (`inject`) plus read-only observation modes (`base`, `chain`, `watch`, `peek`, `threads`) |
| `dontest.c` | a self-started target that mimics `FUN_00644130`'s ABI byte-for-byte, for validating the machinery before it touches the game |
| `.gitignore` | keeps the built PE binaries out of the repo |

Built on this Mac (arm64) — no Windows toolchain required, and none is installed in the guest:

```sh
cd /Users/ember/dev/don/tools/damage-hook
zig cc -target x86-windows-gnu -O2 -shared -o donhook.dll donhook.c
zig cc -target x86-windows-gnu -O2 -o donject.exe donject.c
zig cc -target x86-windows-gnu -O1 -o dontest.exe dontest.c
file donhook.dll   # PE32 executable (DLL) Intel 80386
```

`zig cc` cross-compiling to `x86-windows-gnu` is the load-bearing discovery here [measured]:
`rustup` has no `i686-pc-windows-*` target installed, there is no mingw on this Mac, and the
guest has no compiler. Zig ships the mingw-w64 headers and libs for i386 and produces a PE32
i386 DLL directly.

### 1.1 The detour

`FUN_00644130` opens `55 8B EC 83 EC 1C` — `push ebp; mov ebp,esp; sub esp,0x1c` — six bytes,
three instructions, no relative operands [measured, capstone over the shipped image and
confirmed against live memory]. The patch is `E9 rel32` plus one `nop`.

The trampoline is generated at runtime, one per target, because each needs its own resume
address and target id baked in:

```
  pushfd / pushad                                  ; the ctx block the C handler reads
  push <hook id> ; lea eax,[esp+4] ; push eax
  call donhook_on_enter ; add esp,8
  test eax,eax ; jz +4 ; mov [esp+36],eax          ; swap the caller's return address
  popad / popfd
  <stolen bytes, rel32 displacements relocated>
  jmp dword ptr [resume_cell]
```

Entry is at the function's very first byte, so the stack is exactly what the callee saw:
`[esp]` is the return address and `[esp+4 .. +0x18]` the six dwords, with `ECX` still holding
`this`. `EAX` is captured by **swapping the return address** for a shared stub and remembering
the original on a per-thread shadow stack:

```
  sub esp,4 ; pushfd ; pushad ; push esp
  call donhook_on_return ; add esp,4
  popad ; popfd ; ret          ; returns through the slot the C code filled
```

`ret 0x18` has already unwound the arguments when the stub runs, so `esp` is restored exactly.
Nesting is LIFO per thread and the shadow stack handles it; the record for a call is published
only when its return fires, so `depth` and `parent_seq` attribute nested getter calls to the
enclosing damage call.

### 1.2 The five decisions that matter

* **Every derived read goes through `ReadProcessMemory` on our own process handle.** It returns
  `FALSE` on a bad address instead of faulting. The hook walks the attacker and defender object
  graphs on every damage call; with a raw dereference, one stale pointer in a shipped game is a
  crash in Ember's process. This is the single most important safety property of the design.
* **No formatting and no I/O on the game thread.** The handler writes a fixed-size binary record
  into a ring and returns; a worker thread formats CSV and writes it. Ring overflow is *counted*,
  never silent, and a full ring makes the handler decline to record rather than corrupt a slot.
* **Threads are quiesced before any code write,** and each suspended thread's `EIP` is checked
  against every patch range first, retrying up to 20 times. Patching under a thread parked
  inside the stolen bytes is the one way an inline detour reliably kills a process.
* **`FlushInstructionCache` after every code write, twice, plus a `WriteProcessMemory` of the
  same bytes to ourselves.** Under the ARM64 x86 emulator the translation cache has to be told;
  `NtWriteVirtualMemory` is the path a debugger takes and is what the WOW64 pluggable-CPU layer
  wires to `BTCpuFlushInstructionCache2`. The empirical answer: with these calls in place the
  patch takes effect immediately, on already-executing code, in a process that had been running
  for hours [measured]. I did not isolate which of the three is necessary.
* **`rel32` displacements inside stolen bytes are relocated, and declared, not sniffed.**
  `Unit::get_attack` at `0x006103C0` is `push esi; push edi; mov esi,ecx; call get_attack` — a
  five-byte detour cannot avoid swallowing the call, and copying its displacement verbatim would
  send the trampoline to a wrong absolute address. The config names the byte offset
  (`hook=2103C0,9,unit_attack,4`) rather than the DLL guessing where an opcode starts.

### 1.3 Removability

The patch is removed by dropping a `STOP` file: the worker restores the original bytes, drains
the ring, closes the log, and then **parks**, re-reading its config and re-installing when the
`STOP` file is deleted again. Re-arming matters because `LoadLibrary` on an already-loaded DLL
does not re-run `DllMain`, so a "just inject again" design would silently do nothing. Each DLL
reads `<its own basename>.cfg`, so a second copy under a different filename gets its own config
and its own stop file and cannot fight the first over the same hooks.

---

## 2. Validating it before touching the game

`dontest.exe` exports a function with `FUN_00644130`'s exact ABI — the same `push ebp; mov
ebp,esp; sub esp,0x1c` prologue shape, `ECX = this`, six stack dwords, `ret 0x18` — and calls it
with arguments whose return is a closed form:

```
result = *(int*)this + a1 + 2*a2 + 3*a3 + 4*a4 + 5*a5 + 6*a6
```

A hook that logs the wrong `ECX`, permutes the arguments, or corrupts the return disagrees with
that immediately, from **both** sides: the target checks its own return, and the captured CSV is
checked against the same closed form offline.

| run | target calls | target-reported mismatches | records checked offline | offline mismatches |
|---|---:|---:|---:|---:|
| 1 — base detour | 214,934 | **0** | 39,687 | **0** |
| 2 — re-arm build | 206,328 | **0** | 3,500 | **0** |
| 3 — `rel32`-relocating build, two targets | 115,092 | **0** | — | — |

[measured, all three]. Run 3's second target has the `56 57 89 CE E8 ...` shape — four bytes then
a `call rel32` — hooked with `stolen=9, relfix=4`, which is exactly the shape the live
`Unit::get_attack` and `Build/Wall::get_armor` hooks need.

Run 1 also pins the record semantics: **seq 1..39,687, monotonic, unique, every one flagged
result-valid**, and every one satisfying the closed form — so `ECX`, argument order and `EAX`
are all captured correctly [measured].

**Overhead.** 200,000 tight calls took **46 ms hooked** against **<15 ms unhooked** (the
unhooked figure is below `GetTickCount` resolution) — **≤230 ns added per intercepted call**
[measured]. During that burst 167,310 records were dropped by the ring and **counted**; the
flusher never silently loses a record.

Reproduction:

```sh
prlctl exec "Windows 11" cmd.exe /c "cd /d C:\Users\ember\donhook && dontest.exe 120 200000 > dontest.out 2>&1"
prlctl exec "Windows 11" cmd.exe /c "type C:\Users\ember\donhook\dontest_rva.txt"   # pid + rvas
prlctl exec "Windows 11" cmd.exe /c "cd /d C:\Users\ember\donhook && donject.exe inject <pid> C:\Users\ember\donhook\donhook.dll"
```

---

## 3. Running it against the live game

The game was already running as PID 14644. Before patching anything, a **read-only** check that
it was actually simulating — `donject watch` following `[[0x00C061E8]+0x550]`, the sim frame
counter:

```
base=00D60000 delta=00960000
[015661E8+0]=01797EC0 [01797EC0+550]=000032D1
[015661E8+0]=01797EC0 [01797EC0+550]=000032DC   # +11 frames / 700 ms => ~15.7 fps
```

and a read-only comparison of the live prologue bytes against the shipped image at all seven
addresses, which matched exactly [measured].

Injection is the classic `VirtualAllocEx` + `WriteProcessMemory` + `CreateRemoteThread`
(`LoadLibraryA`). Two things worth recording:

* On ARM64 Windows, **every x86 process in a boot session maps `SysWOW64\kernel32.dll` at the
  same base** — 0x76650000 for both the injector and the game [measured] — so a 32-bit injector's
  local `LoadLibraryA` address is valid remotely. `donject` verifies this and **refuses to inject
  if the bases differ** rather than firing a remote thread at a wrong address.
* `CreateRemoteThread` into an x86 process from an x86 process works fine under the emulator
  [measured]; the remote `LoadLibraryA` returned the DLL's base.

Session 1 hooked three functions and ran for roughly a quarter of an hour of real play
(8,400 sim frames):

```
donhook boot: main base=00D60000 delta=00960000 pid=14644
hooked damage     at 00FA4130 (rva 244130) stolen=6 orig=55 8B EC 83 EC 1C
hooked get_attack at 00FA69F0 (rva 2469F0) stolen=5 orig=53 56 8B F1 57
hooked get_armor  at 00FA7DB0 (rva 247DB0) stolen=6 orig=56 57 8B F9 6A 16
...
session done: written=138909 dropped=0 forced=9 head=138909
```

Session 2 added the four class overrides (seven hooks total) and installed cleanly, but the
match ended before it recorded a single damage call (`head=0`) — see §5.

Reproduction, end to end:

```sh
# 1. serve the tools to the guest (the VM reaches the host at 10.211.55.2)
cd /Users/ember/dev/don/tools/damage-hook && python3 -m http.server 18080 --bind 10.211.55.2 &
prlctl exec "Windows 11" cmd.exe /c "cd /d C:\Users\ember\donhook && curl.exe -s -o donhook.dll  http://10.211.55.2:18080/donhook.dll  && curl.exe -s -o donject.exe http://10.211.55.2:18080/donject.exe && curl.exe -s -o donhook.cfg http://10.211.55.2:18080/cfg-game.txt"
prlctl exec "Windows 11" cmd.exe /c "cd /d C:\Users\ember\donhook && certutil -hashfile donhook.dll SHA256"   # hash-verify, always

# 2. read-only pre-flight: is the sim actually running, and are the prologues stock?
prlctl exec "Windows 11" cmd.exe /c "cd /d C:\Users\ember\donhook && donject.exe watch 14644 riseofnations.exe 8061E8 0 550 8 700"
prlctl exec "Windows 11" cmd.exe /c "cd /d C:\Users\ember\donhook && donject.exe chain 14644 riseofnations.exe 244130 0"

# 3. arm  (STOP must not exist)
prlctl exec "Windows 11" cmd.exe /c "cd /d C:\Users\ember\donhook && donject.exe inject 14644 C:\Users\ember\donhook\donhook.dll"

# 4. disarm — restores the original bytes and drains the log
prlctl exec "Windows 11" cmd.exe /c "echo x > C:\Users\ember\donhook\STOP"

# 5. stream the CSV back (PUT receiver on the Mac; inline base64 return times out)
python3 <scratch>/uprecv.py /Users/ember/dev/don/schema/live/damage-hook &
prlctl exec "Windows 11" cmd.exe /c "cd /d C:\Users\ember\donhook && copy /Y damage.csv snap.csv"
prlctl exec "Windows 11" cmd.exe /c "cd /d C:\Users\ember\donhook && curl.exe -s -T snap.csv http://10.211.55.2:18081/live-damage.csv"
```

`cfg-game.txt`, the live configuration, is checked in next to the sources:

```
log=C:\Users\ember\donhook\damage.csv
stop=C:\Users\ember\donhook\STOP
derive=1
maxrec=300000
hook=244130,6,damage
hook=2469F0,5,get_attack
hook=247DB0,6,get_armor
```

### 3.1 What a record contains

One CSV row per intercepted call: `seq, tid, target, depth, parent_seq, this, retaddr, a1..a6,
result, flags, derived_mask`, and for damage calls a snapshot read out of the live heap —
`atk_obj/atk_obj_tab/atk_type`, `def_obj/def_obj_b/def_type`, both players and indices, both
type ids, the live `balance_pct`, `type_attack` (`+0x1E8`), `type_armor` (`+0x214`), both mask
words (`+0x1E4`), both domains (`+0x218`), the object flag/height/facing/stamp words the chain
reads (`+0x08 +0x0C +0x4C +0x50 +0x5C +0x68 +0x6C +0xA4`), `SPLASH_PERCENT` (`+0x204`), the
splash divisor (`+0x308`), `+0x2B8`, `+0x40`, and the sim frame. `derived_mask` says which of
the seven block reads succeeded; **56,780 of 56,789 damage records have all seven** [measured].

Offsets are the ones `FUN_00644130` itself reads, taken from `mechanics.rs`'s `DamageInput` doc
comments and re-checked against the disassembly. The hook **copies** what retail read; it never
recomputes anything retail computed.

---

## 4. What the live corpus says

`schema/live/damage-hook/analysis-snap2.txt` is the generated summary; the numbers below come
from it. 56,789 damage calls, 56,780 with a complete derived read, frames 20,338 → 28,738, one
thread (**tid 14116 issued every single record — the sim is single-threaded** [measured]).

### 4.1 The two object tables are not aliases — `damage-port.md` §5.6 is refuted [measured]

`FUN_00644130` fetches the defender through **two** globals, `[0x00C0AB84]` and `[0x00C0AEC0]`,
both indexed `base[player*28] -> ptr_array; ptr_array[index]`. The oracle harness points both at
one array and `damage-port.md` §5.6 flags the assumption as load-bearing and untested. The hook
reads both per call:

| | count | share |
|---|---:|---:|
| table A == table B | 32,074 | 56.5% |
| table A != table B | 24,706 | 43.5% |
| …of those, table B is `0xFFFFFFFF` | 24,198 | 98.0% of the disagreements |

and the split is not statistical, it is structural:

* every call where they **agree** has `def_type_id < 364` — the unit range — **32,074 / 32,074**
* every call where they **differ** has `def_type_id >= 364` — the building range — **24,706 / 24,706**

Dumping the arrays confirms it: for player 0 the first sixteen entries of both are byte-identical
pointers, and they diverge only where buildings live [measured].

`0xFFFFFFFF` is a sentinel, not an object, so retail cannot be dereferencing it — the reads
through table B sit behind `attacker_masks & 0x40000` and two virtual predicates, which must be
closed for building defenders. Two consequences, and they point in opposite directions:

* The port's `defender_type_0x2b8_bit2` and part of `defender_flags_0x68` are read through table B
  in retail. For **building defenders the port's model of where those come from is wrong**, and it
  cannot be right by luck because the pointer is `-1`.
* The harness, by aliasing the tables, explores a state retail may never reach, and by
  construction **cannot test the guard that stops retail dereferencing `-1`**.

This does not disturb the 7,986,695-trial Tier-B result — it is a statement about which inputs
that result covers.

### 4.2 The attack/armor getters are overridden in live play [measured]

`0x006441B1` is `call [eax+0x124]` on the defender and `0x006441BE` is `call [edx+0x120]` on the
attacker, so a hook on `Object::get_attack` (`0x006469F0`) or `Object::get_armor` (`0x00647DB0`)
should see return addresses `0x006441C4` and `0x006441B7`. Over 56,789 damage calls it saw them
**zero times**. What it saw instead:

| function | records | call site | which is |
|---|---:|---|---|
| `get_attack` `0x006469F0` | 56,540 | `0x006103C9` | inside `0x006103C0`, which is `push esi; push edi; mov esi,ecx; call 0x6469F0` |
| | 844 | `0x0062E61D` | inside `0x0062E610`, likewise |
| | 25 | `0x00644A09` | the **direct** `call 0x6469f0` at `0x00644A04` inside the damage function |
| `get_armor` `0x00647DB0` | 24,711 | `0x0063FA69` | inside `0x0063FA60`, likewise |

A static read of `schema/vtables.json` against the shipped image settles the mechanism
[measured]:

| class | vtbl `+0x120` (attack) | vtbl `+0x124` (armor) |
|---|---|---|
| `Unit` / `UnitData` / `UnitOut` / `Animal*` | `0x006103C0` | `0x00610160` |
| `Build` / `BuildData` / `BuildOut` | `0x0062E610` | `0x0063FA60` |
| `Wall` / `WallData` / `WallOut` | `0x006469F0` (the base) | `0x0063FA60` |

`0x006469F0` and `0x00647DB0` are the **base-class leaves**. `Unit::get_armor` (`0x00610160`)
does not call the base at all — it reads `[this+0x9C]` and `[this+0xA]` behind two virtual
gates — which is exactly why unit defenders produced no `get_armor` records while building
defenders produced 24,711.

`mechanics.rs` documents `DamageInput::attack` as "`attacker->vtbl[0x120]()`, i.e. `get_attack`"
and `armor` as "`defender->vtbl[0x124]()`, i.e. `get_armor`". The Tier-B claim for those two
functions stands — the harness drove the real retail code. But **in a real game they are not
what supplies `attack` and `armor`** for any `Unit` attacker or any `Unit`/`Build`/`Wall`
defender, which is every combatant observed here. Modelling `attack`/`armor` as `get_attack`/
`get_armor` is incomplete, and the missing piece is four override functions.

### 4.3 Both upgrade paths were dead for the entire match [measured]

Restricted to invocations on the object the damage record names:

* base `get_attack` returned exactly `UnitType[+0x1E8]` in **56,805 / 56,805**
* base `get_armor` returned exactly `UnitType[+0x214]` in **24,706 / 24,706**

So over a full match neither upgrade term ever fired at the base level. `damage-port.md` lists
both upgrade paths as **unverified**; this does not verify them, it measures that they are
**unreached in ordinary play at this layer** — consistent with the upgrade arithmetic having
moved into the overrides in §4.2. `RULES[+0x8B8]`, the multiplier those paths use, is **1** in
this game [measured] — so a live sample would be indistinguishable from `+10 x level` /
`+1 x level` anyway, and a lane wanting to pin the multiplier needs a game where it is not 1.

### 4.4 What live play actually exercises [measured]

| | |
|---|---|
| call sites of `FUN_00644130` | **two**: `0x0064EC10` (53,885 calls) and `0x0064A4F7` (2,904) |
| `splash_flag` (`ebp+0x14`) non-zero | **28 / 56,780** (0.05%) |
| `overkill_gate` (`ebp+0x18`) non-zero | 2,904 — exactly the calls from `0x0064A4F7` |
| `out_kind` pointer non-null | 2,904 — the same calls; `0x0064EC10` always passes null |
| `attack_dir` distinct values | 14,914 |
| attacker type ids | 25 distinct |
| defender type ids | 50 distinct |
| (attacker, defender) type pairs | 242 distinct |
| domains seen | 0 and 1 only — **no domain-2 (air) combat in this match** |
| `result` | −3 … 40,400 over 148 distinct values; 2 zeros; **negatives do occur** |
| `type_attack` / `type_armor` | 120…320 / 0…7 |

The splash figure is the useful one: the port's steps 13–16 are Tier B from 3,994,983 oracle
trials, and live play reached them **28 times in a whole match**. Oracle coverage and live
coverage are close to inverses of each other; neither substitutes for the other.

Negative returns confirm that the floor of 1 really is conditional, from behaviour rather than
from reading the branch.

### 4.5 Live RULES constants [measured]

Read read-only from `[0x00C061E4]` (runtime `0x01798B88`) with `donject peek`:

| offset | name (from `mechanics.rs`) | live value |
|---|---|---:|
| `+0x044` | `HEIGHT_INCREMENT` | 200 |
| `+0x048` | `HEIGHT_BONUS` | 10 |
| `+0x04C` | `FLANK_BONUS` | 50 |
| `+0x050` | `CAVALRY_FLANK_BONUS` | 40 |
| `+0x054` | `VEHICLE_FLANK_BONUS` | 33 |
| `+0x058` | `ROCKY_MODIFIER` | 170 |
| `+0x05C` | `OVERKILL_FRAMES` | 30 |
| `+0x060` | `OVERKILL_DAMAGE` | 85 |
| `+0x064` | `ENTRENCHMENT_MODIFIER` | 170 |
| `+0x068` | `RIVER_MODIFIER` | 512 |
| `+0x06C` | `RECAPTURE_CITY_MODIFIER` | 512 |
| `+0x4C4` | `RED_FORT_AIR_DEFENSE` | 33 |
| `+0x558` | (unnamed) | 0 |
| `+0x76C` | (unnamed) | 25 |
| `+0x794` | (unnamed, step 11) | −5 |
| `+0x8B8` | (unnamed, upgrade multiplier) | 1 |
| `+0xB98` | (unnamed) | 204 |
| `+0xBBC` | (unnamed, step 10) | 1 |

These are the values *this* match ran with. They are a live read, not a claim about the shipped
`rules.xml` defaults; a mod or a different ruleset would change them. Full hex dump:
`schema/live/damage-hook/rules-full.txt`.

### 4.6 Balance table, live [measured]

Read from `0x00C06AFC + 2*(atk*493 + def)` per call, using the port's own index arithmetic:

* 51 distinct values over 242 type pairs, **all positive**, range 33…430
* **every (attacker, defender) pair yielded a constant value across the whole match** — the table
  is static once loaded
* **no negatives** anywhere in the region live combat touches

`docs/derivation/combat.md` §5 warns that the captured 493×493 window has unexplained negatives
and that the table's real extent is unbounded. The live reads do not reproduce any negative in
the 242 cells real combat used, which narrows the question to cells combat never reaches.

One loose thread the corpus surfaces: **defender type ids reach 521, 522 and 526** (237 records)
— beyond the 493 = 364 + 129 combined type space the stride assumes. Attacker ids stay ≤ 445. A
column index above the row stride aliases into the next row; the maximum flat index observed is
219,613, still inside 493×493 = 243,049 cells, so nothing read out of the captured window. Worth
a lane: the stride of 493 is pinned Tier B (a 493→494 mutation produced 136,409 mismatches), so
it is the *type-id domain*, not the stride, that is larger than assumed.

---

## 5. The match ended mid-experiment, and the hook did not do it

Session 2 (seven hooks, including the four overrides) installed cleanly and then recorded
nothing. The sim frame counter had stopped at `0x7043` and stayed there. Evidence that this was
the match ending and not the hook:

* the counter **remained frozen at `0x7043` after every hook was removed** and all seven sites
  were verified byte-identical to the shipped image, 16 bytes each [measured]
* the process stayed `Responding = True` and kept burning ~1 core — rendering, not wedged
* the **last damage record in session 1 is at frame 28,738 = `0x7042`**, one frame before the
  freeze: the capture ran right up to the final tick of combat and the sim stopped after it
* session 1 had already shut down cleanly over 8,400 hooked sim frames with `dropped=0`

I cannot prove a negative, and I am not claiming one: what I can say is that the freeze survived
complete removal of the instrumentation, which is not how a hook-induced hang behaves.

**Residual state in the guest.** Two `donhook` DLLs remain loaded in PID 14644, both parked with
zero patches installed, sleeping 500 ms per poll; `STOP` and `STOP2` are both present so neither
can re-arm. Nothing in the game's code is modified. They disappear when the game restarts.

---

## 6. Why there is no agreement rate against `mechanics.rs` — and what would produce one

The lane brief asks for a comparison against `crates/don-sim/src/mechanics.rs::damage` with a
sample count. **That number cannot be computed from this capture**, and here is the precise
reason rather than a hedge.

`damage()` takes four structs. The capture supplies most of `DamageInput` and all of
`CombatRules` (§4.5). It supplies **none of `DamagePredicates`** — 30 booleans, every one of
them the result of a *virtual call* the damage function makes (`attacker->vtbl[0x18]`,
`defender->vtbl[0x20]`, six tech queries, the `Build::vftable` test, the recapture owner
comparison, and so on). A hook on the function entry and exit cannot observe them; they are
computed inside. Three more inputs are also unobserved: `attacker_vf_0xe4`, and `tile_rocky` /
`tile_owner`, which come from a map record the hook does not walk.

And §4.2 adds a second, independent blocker: `attack` and `armor` are supplied by class
overrides the port does not model, so even a full predicate vector would leave the chain's first
operand unknown.

Searching over the predicate vector is not a way out. 2^30 per record is infeasible by brute
force, and a "did *some* assignment reproduce the observed damage?" test is a weak claim that
also requires a second transcription of the chain to drive the search — a transcription whose
own bugs would masquerade as findings. That is the shape of error this project is built to
avoid, so I did not build it.

**What closes it, in order of leverage:**

1. **Hook the four overrides** — `0x006103C0`, `0x00610160`, `0x0062E610`, `0x0063FA60`. This is
   already written, built and validated: `donhook2.cfg` next to the sources declares all seven
   hooks, the `rel32` relocation the two four-byte-preamble functions need is validated at
   115,092 calls with 0 mismatches (§2), and the seven-hook build installed cleanly on the live
   game. It only needs a running match. That converts `attack` and `armor` from unmodelled to
   observed on every damage call.
2. **Capture the predicates.** Two viable routes. (a) Hook each predicate virtual and attribute
   by the same shadow-stack nesting the getters already use — the machinery is done, it is
   ~25 more `hook=` lines once each slot's implementation and stolen-byte count is resolved per
   class (`schema/vtables.json` gives the slots directly). (b) Patch the seven `test eax,eax`
   sites after the predicate calls in `FUN_00644130` to also store `eax` into a scratch buffer —
   smaller and faster, but it edits mid-function code, which is a real step up in risk.
3. **Then, and only then**, run `damage()` per record and report an agreement rate with N.

Until step 1 and 2 land, the honest description of the live corpus is: **ground truth for the
output and for most of the arithmetic inputs, and no observation at all of the branch vector.**

---

## 7. What I could not establish

* **No agreement rate against the Rust port.** §6. This is the lane's headline gap and it is a
  missing-observation problem, not a disagreement.
* **Approaches 2 and 3 were not tried.** Approach 1 worked on the first live attempt, and the
  brief ranks it first. So:
  * the **`INT3` + `DebugActiveProcess` loop** is untested here. It is also unattractive on
    evidence: 56,789 damage calls in a quarter of an hour, each costing a debug-event round trip
    and two context operations, against 230 ns for the detour.
  * **hardware breakpoints via the DR registers under x86-on-ARM64 emulation remain UNKNOWN.**
    I did not test them and I am not going to infer an answer. What I *can* report adjacent to it:
    `GetThreadContext` on an emulated x86 thread from an emulated x86 thread returns a usable
    `CONTEXT` with a meaningful `Eip` — the quiesce path relies on it and the patches landed
    safely 10 times over — so the emulator does surface x86 thread state to `CONTEXT`. Whether
    `Dr0..Dr7` in that `CONTEXT` are honoured by `xtajit` is a separate question and is the one
    still open.
* **Which of the three cache-flush calls is required.** All three are issued together and the
  patch works. I did not bisect them, so "you must call `FlushInstructionCache`" is inherited
  advice I followed, not a result I measured.
* **The mechanism behind the two object tables.** §4.1 measures the split exactly and shows
  table B holds `0xFFFFFFFF` at building indices. Whether table B is a units-only table, a
  shorter allocation being read past its end, or something else is not settled — only that it is
  not an alias.
* **Why the sim stopped.** §5. Most likely the match ended; I have exonerating evidence, not a
  cause.
* **The `out_kind` enum is still not compared.** The hook records the pointer argument, not the
  value written through it. Adding a post-call read of `*out_kind` in the return stub is easy and
  would close `damage-port.md` §5.3's open item; I did not do it.
* **Only one match, one ruleset, one map.** 25 attacker types, 50 defender types, 242 pairs, no
  air units, one civilisation matchup. The corpus is real but it is not a sample of the game.

---

## 8. Files

| path | what |
|---|---|
| `/Users/ember/dev/don/tools/damage-hook/donhook.c` | the hook DLL |
| `/Users/ember/dev/don/tools/damage-hook/donject.c` | injector + read-only `chain`/`watch`/`peek` |
| `/Users/ember/dev/don/tools/damage-hook/dontest.c` | ABI-matched validation target |
| `/Users/ember/dev/don/tools/damage-hook/cfg-game.txt` | live 3-hook config |
| `/Users/ember/dev/don/tools/damage-hook/donhook2.cfg` | live 7-hook config, incl. the four overrides |
| `/Users/ember/dev/don/schema/live/damage-hook/live-damage-snap2.csv` | 138,909 live records (gitignored) |
| `/Users/ember/dev/don/schema/live/damage-hook/analysis-snap2.txt` | the generated summary behind §4 |
| `/Users/ember/dev/don/schema/live/damage-hook/rules-full.txt` | live RULES hex dump |
| `/Users/ember/dev/don/schema/live/damage-hook/dontest-hook.csv` | the 39,687-record validation capture |
| guest: `C:\Users\ember\donhook\` | the deployed copies, the raw logs, `STOP` / `STOP2` |

`cargo test --workspace` passes at the repo root (exit 0) — this lane wrote no Rust.

---

## 9. Proposed provenance-ledger entries

### Live damage capture — `FUN_00644130`

| field | value |
|---|---|
| source | live `riseofnations.exe` PID 14644, runtime base `0x00D60000`, delta `0x00960000` |
| instrument | `tools/damage-hook/donhook.c`, inline detour, 6 stolen bytes at `0x00644130` |
| tier | **C** — behavioural capture of a real process. Not a differential test; nothing is compared here |
| evidence | 56,789 damage calls / 138,909 records, frames 20,338–28,738, `dropped=0`, `forced=9` (0.0065%), 56,780 with a complete derived read |
| instrument validation | 536,354 calls on an ABI-matched self-started target, 0 divergence; 43,187 captured records re-checked offline against a closed form, 0 mismatches; ≤230 ns/call overhead |
| caveats | one match, one ruleset; no air combat; predicates unobserved; `out_kind` value not read |

### Correction to `damage-port.md` §5.6 — the object tables

| field | value |
|---|---|
| claim | `[0x00C0AB84]` and `[0x00C0AEC0]` **do not alias** |
| tier | **C** [measured] |
| evidence | 32,074 / 56,780 agree (all `def_type_id < 364`), 24,706 differ (all `def_type_id >= 364`), 24,198 of those return `0xFFFFFFFF` |
| consequence | the oracle harness's aliasing is an untested simplification, and the port's table-B-sourced fields are unmodelled for building defenders |

### Correction to `DamageInput::attack` / `::armor`

| field | value |
|---|---|
| claim | vtable slots `+0x120` / `+0x124` dispatch to class overrides, not to `0x006469F0` / `0x00647DB0` |
| tier | **C** [measured] for the live dispatch; the vtable contents are a static read of the shipped image |
| evidence | 0 / 56,789 damage calls reached the base functions through the two in-function vtable calls; `Unit -> 0x006103C0 / 0x00610160`, `Build -> 0x0062E610 / 0x0063FA60`, `Wall -> 0x006469F0 / 0x0063FA60` |
| consequence | `get_attack` / `get_armor` are base-class leaves; the operands the chain consumes are produced one level up |
