# Derivation: the engine RNG and its call sites

Lane: `rng`. Every claim below is marked **[measured]** (verified by me against
`ron-bin/riseofnations.exe` sha256 `30478a44…625079`, or against retail machine code
executing under the oracle on hbox) or **[reported]** (read somewhere, not checked).
Fidelity tiers per `docs/CHARTER.md`. Nothing here is "verified" in the proof-assistant
sense.

---

## Headline

**The engine RNG is a 32-bit Numerical-Recipes LCG — `s ← s·1664525 + 1013904223` — with a
16-bit multiply-shift bound mapping, exposed as a tiny `Random` class whose entire object
state is one `u32`.** [measured]

```
Random::next_float()          VA 0x00a39cf0   __thiscall(this = &state) -> f32 in [0,1)
Random::in_range(lo, hi)      VA 0x00a39d70   __thiscall(this = &state), stdcall args, ret 8
Random::exchange_seed(s)      VA 0x00a39d30   __thiscall, returns previous state
Random::Random() / reset()    VA 0x00a39d50 / 0x00a39d60   state = 0
Random::in_range on global A  VA 0x00a39d40   __fastcall(lo, hi) wrapper over 0x00eb697c
```

`in_range` has **414 call sites in 170 functions** [measured] — it is the workhorse. There
are **at least seven independent streams**, and the one used by map generation, animals,
units and the BHS script API is a distinct object from the ones used by water rendering
and the music jukebox. That split is the load-bearing determinism fact.

**`0x00846450` is not the RNG, and is not called by anything.** See "Refuted", below.

---

## 1. The generator, exactly

### 1.1 `Random::next_float()` — VA `0x00a39cf0` [measured]

```asm
0xa39cf0  push ebp
0xa39cf1  mov  ebp, esp
0xa39cf3  push ecx
0xa39cf4  imul eax, dword ptr [ecx], 0x19660d      ; state * 1664525
0xa39cfa  add  eax, 0x3c6ef35f                     ;       + 1013904223
0xa39cff  mov  dword ptr [ecx], eax                ; store back  (this+0 IS the state)
0xa39d01  and  eax, 0x7fffff
0xa39d06  or   eax, 0x3f800000                     ; reinterpret as f32 in [1,2)
0xa39d0b  mov  dword ptr [ebp - 4], eax
0xa39d0e  movss  xmm0, dword ptr [ebp - 4]
0xa39d13  cvtps2pd xmm0, xmm0
0xa39d16  subsd  xmm0, qword ptr [0xb695a8]        ; - 1.0  (f64 constant)
0xa39d1e  cvtpd2ps xmm0, xmm0
0xa39d22  mov  esp, ebp
0xa39d24  pop  ebp
0xa39d25  ret                                      ; result left in xmm0
```

Note the widen-subtract-narrow. It is **exactly** representable at every step (the operand
lies in `[1,2)`, so `x − 1.0` is exact in binary64 and the result fits binary32 exactly),
so a plain `f32` subtraction is bit-identical — and that is *tested*, not asserted (§2).

`0x00a39cf0` leaves its result in `xmm0`, not `st(0)`. That is not the normal MSVC x86
float return convention; the difftest confirms the register empirically.

### 1.2 `Random::in_range(lo, hi)` — VA `0x00a39d70` [measured]

Fast path (the SEH prologue and the out-of-range warning branch are elided):

```asm
0xa39da3  cmp  ebx, 0xffff        ; ebx = lo
0xa39da9  jg   warn_check
0xa39dab  cmp  edi, 0xffff        ; edi = hi
0xa39db1  jle  body
warn_check:                       ; a RUN TIME WARNING, once per session, gated on
0xa39db7  cmp  byte ptr [0xee13a8], 0     ; the byte at 0x00ee13a8
...
body:
0xa39e83  cmp  ebx, edi
0xa39e85  jne  0xa39e9d
0xa39e87  mov  eax, ebx           ; lo == hi -> return lo, STATE NOT ADVANCED
          ...ret 8
0xa39e9d  jle  0xa39ea5
0xa39e9f  xor  ebx, edi           ; lo > hi -> xor-swap so lo < hi
0xa39ea1  xor  edi, ebx
0xa39ea3  xor  ebx, edi
0xa39ea5  imul ecx, dword ptr [eax], 0x19660d   ; eax = this
0xa39eab  sub  edi, ebx                          ; range = hi - lo
0xa39ead  add  ecx, 0x3c6ef35f
0xa39eb3  mov  dword ptr [eax], ecx               ; state advanced exactly once
0xa39eb5  movzx eax, cx                           ; low 16 bits of the NEW state
0xa39eb8  imul eax, edi                           ; * range, low 32 bits
0xa39ebb  shr  eax, 0x10                          ; >> 16, logical
0xa39ebe  add  eax, ebx                           ; + lo
          ...ret 8
```

Semantics, all of which are easy to get wrong and none of which are assumed:

* **The range is half-open, `[lo, hi)`.** `in_range(0, 10)` returns 0..9 and never 10.
  Confirmed by a 200 000-draw histogram through the oracle (§3), not by reading the code.
* **`lo == hi` returns `lo` and does *not* advance the state.** This is a real determinism
  hazard for a reimplementation: getting it wrong desynchronises the stream, not just one
  draw.
* **`lo > hi` is silently swapped**, and the state *is* advanced.
* Only the **low 16 bits** of the new state feed the mapping. The generator therefore
  yields at most 65 536 distinct outcomes per draw regardless of range width, and the
  engine knows it: bounds above `0xFFFF` trip a "RUN TIME WARNING" (BHG's assert rig,
  strings at `0xb1771c`). With `range > 0xFFFF` the `imul` overflows and the low 32 bits
  are kept — still deterministic, but non-uniform. Measured, §3.
* `0x00a39d70` sets up a Windows SEH frame: it reads and writes `fs:[0]`. That matters for
  the oracle (§2) and for anyone lifting it.

### 1.3 The class is four bytes

`this+0` is the whole object. Every method touches `[ecx]` and nothing else [measured], so
a `Random` is a bare `u32` seed. The statically-constructed instances are zeroed by their
CRT initialisers (`0x00ac0b90`, `0x00abde30`, `0x00abde40`, `0x00ab4dd0` — each a
one-instruction `mov dword ptr [addr], 0`) [measured].

### 1.4 A second, independent confirmation of the constants

`0x00542579`–`0x0054258f` (compiland unattributed — the nearest `__FILE__` marker is
30 kB away, so I am not naming it) contains a **constant-folded first
draw**: the compiler evaluated `Random(0).next_float()` at compile time and emitted
`mov dword ptr [ebp-0x14], 0x3feef35f` next to `mov dword ptr [ebp-0x10], 0x3c6ef35f`.
`0*1664525 + 1013904223 = 0x3C6EF35F`, and `0x3F800000 | (0x3C6EF35F & 0x7FFFFF) =
0x3FEEF35F`. Both constants appear in the binary as literals, independently of the LCG
code itself. [measured]

---

## 2. Tier B evidence — differential test through the oracle

Harness: **`crates/oracle/src/bin/rng.rs`** (new file, this lane), built for
`i686-unknown-linux-musl` and run on hbox against the real mapped image.

> **A prerequisite this lane had to discover and fix, which applies to every SEH-carrying
> retail function, not just this one:** MSVC x86 SEH prologues execute `mov eax, fs:[0]`
> and `mov fs:[0], eax`. On i386 Linux, TLS lives in `%gs` and `%fs` is a null selector, so
> the *first* attempt at `in_range` died with SIGSEGV in the prologue — nowhere near the
> RNG math. `install_fake_teb()` mmaps a page, installs it as a segment base with
> `set_thread_area(2)`, writes the `0xFFFFFFFF` end-of-chain sentinel at offset 0, and
> loads the selector into `%fs`. After that the retail prologue is ordinary memory
> traffic. Any future oracle work on non-leaf sim functions will need this.

| target | tier | trials | mismatches |
|---|---|---|---|
| `0x00a39cf0` `Random::next_float` | **B** | **999 950** | **0** |
| `0x00a39d70` `Random::in_range` (bounds in ±0xFFFF) | **B** | **1 000 012** | **0** |
| `0x00a39d70` `Random::in_range` (bounds to ±2^29, warning branch suppressed) | **B** | **500 000** | **0** |

Input distributions, stated because Tier B is meaningless without them:

* **`next_float`** — 70 starting seeds: six hand-chosen (`0`, `1`, `0xFFFFFFFF`,
  `0x80000000`, `0x7FFFFFFF`, `0x3C6EF35F`) plus 64 from a fixed-seed xorshift64; each
  walked **14 285 chained steps**, so the test exercises the recurrence as a *sequence*,
  not just single steps. **Both** the resulting state *and* the `xmm0` f32 **bit pattern**
  are compared.
* **`in_range`** — 12 hand-chosen edges (empty range, inverted range, `±0xFFFF` boundary,
  `lo == hi`, negatives) plus 1 000 000 random: state uniform over `u32`; `lo`, `hi`
  independent in `(−65535, 65535)` so the warning branch is never taken. **Both** the
  return value *and* the resulting state are compared.
* **out-of-range** — 500 000 with `lo`, `hi` uniform in ±2^29. The once-per-session warning
  byte at VA `0x00ee13a8` is pre-set to 1 in the mapped image so the warning branch is
  skipped and only the arithmetic runs.

**This is testing, not verification.** It says nothing about inputs outside those
distributions, and it is bounded by the fidelity of the mapped-PE environment.

Commands (exactly reproducible):

```sh
scp crates/oracle/src/bin/rng.rs hbox:~/don-oracle/crates/oracle/src/bin/rng.rs
ssh hbox 'cd ~/don-oracle && nice -n 15 taskset -c 0-3 \
    cargo build --target i686-unknown-linux-musl -q --bin rng'
ssh hbox 'cd ~/don-oracle && nice -n 15 taskset -c 0-3 \
    ./target/i686-unknown-linux-musl/debug/rng vectors'
ssh hbox 'cd ~/don-oracle && nice -n 15 taskset -c 0-3 \
    ./target/i686-unknown-linux-musl/debug/rng difftest 1000000'
ssh hbox 'cd ~/don-oracle && nice -n 15 taskset -c 0-3 \
    ./target/i686-unknown-linux-musl/debug/rng oob 500000'
```

---

## 3. Captured vectors — use these, do not recompute them

All produced by `rng vectors` from retail machine code. **Capture, do not calculate.**

```
next_float(0x00000000) -> state 0x3c6ef35f, f32 bits 0x3f5de6be (0.8668021)
next_float(0x00000001) -> state 0x3c88596c, f32 bits 0x3d8596c0 (0.06522894)
next_float(0x3c6ef35f) -> state 0x47502932, f32 bits 0x3f205264 (0.6262572)
next_float(0xffffffff) -> state 0x3c558d52, f32 bits 0x3f2b1aa4 (0.66837525)
next_float(0x80000000) -> state 0xbc6ef35f, f32 bits 0x3f5de6be (0.8668021)
next_float(0x00001571) -> state 0x5d04101c, f32 bits 0x3d020380 (0.03174162)

first 12 states of a Random seeded 0 (i.e. every freshly constructed static Random):
0x3c6ef35f, 0x47502932, 0xd1ccf6e9, 0xaaf95334, 0x6252e503, 0x9f2ec686,
0x57fe6c2d, 0xa3d95fa8, 0x81fdbee7, 0x94f0af1a, 0xcbf633b1, 0xbcd1195c

in_range(0x00000000,      0,      0) ->      0, state 0x00000000   <- NOT advanced
in_range(0x00000000,      0,      1) ->      0, state 0x3c6ef35f
in_range(0x00000000,      1,      0) ->      0, state 0x3c6ef35f   <- swapped
in_range(0x00000000,      0,    100) ->     95, state 0x3c6ef35f
in_range(0x00000000,      1,      6) ->      5, state 0x3c6ef35f
in_range(0x00000000,     -5,      5) ->      4, state 0x3c6ef35f
in_range(0x00000000,      5,     -5) ->      4, state 0x3c6ef35f
in_range(0x00000000,      0,  65535) ->  62302, state 0x3c6ef35f
in_range(0x00003039,      0,     10) ->      1, state 0x05391c44
in_range(0xdeadbeef,  -1000,   1000) ->    746, state 0x6aabdf82

out of range (warning branch suppressed):
in_range(0x00000001,      0, 1000000) ->  21624
in_range(0x00000001,      0,  100000) ->  34930
in_range(0x00000001,-1000000, 1000000) -> -956752
```

Half-open range, measured: 200 000 draws of `in_range(0, 10)` from seed 1 —

```
value  0      1      2      3      4      5      6      7      8      9     10   11..
count  20005  20022  19980  19989  20000  19994  20007  19980  20011  20012  0    0
```

Bucket 10 is empty. `rand_int(lo, hi)` is `[lo, hi)`.

---

## 4. Streams — there are at least seven, and they are not equivalent

`in_range`'s `this` at each of its 414 call sites, taken as the last `mov/lea ecx, …`
before the call *in linear order* (a syntactic heuristic, not dataflow — the small
`[ebp-…]` / `0x18` rows below are almost certainly mis-attributions from an earlier basic
block, and are reported as-is rather than cleaned up):

| `this` | sites | callers | what lives there |
|---|---:|---:|---|
| `[0x00c06184]` → object at **`0x00e37a8c`** | **239** | **93** | **the simulation stream** |
| `0x00eb697c` (static) | 69 | 26 | `Surf.cpp` (water), `Scene`, graphics `0x88…–0x90…`, one `Animal` |
| `[ecx+0x18]` (member) | 23 | 12 | `conquestgraphicpieces.cpp` — Conquer-the-World map screen |
| `0x00e85f0c` (static) | 17 | 16 | `jukebox.cpp`, `gamelog.cpp`, `conquestgraphicpieces.cpp`, `ConquestWonderWin` |
| `0x00ee1aec` (static) | 6 | 1 | one graphics function, `0x008db7c0` |
| `[esi+0x63d0]` | 5 | 1 | `0x0095d840` |
| `0x00ecba54` (static) | 3 | 1 | `jukebox.cpp` (`0x0097d770`) |
| `0x00e87d3c` (static) | 3 | 3 | `jukebox.cpp` |
| `[0x00c0617c]` → `0x00e86880` | 3 | 3 | `0x0078…–0x0079…` (CtW) |
| `[esi/ebx+0x214]` (member) | 5 | 4 | `commandpackage.cpp` |
| assorted `[reg+…]` | 41 | — | heuristic noise / per-object streams |

`0x00c06184` holds a **statically initialised pointer** to `0x00e37a8c` — the file bytes at
`0x00c06184` are `8c 7a e3 00` and there is a base relocation there [measured]. The only
code that touches `0x00e37a8c` by absolute address is its CRT initialiser at `0x00ab4dd0`
(`mov dword ptr [0x00e37a8c], 0`); everything else goes through the pointer [measured].

### Simulation-critical vs cosmetic

The stream-level split is what is established, and it is clean at the two ends:

**Simulation-critical — `[0x00c06184]` / `0x00e37a8c`** [measured]:

* **Map generation.** 24 of the 93 callers are virtual methods on `Map` and its 20
  subclasses (`MapGreatLakes`, `MapNileDelta`, `MapSeaPower`, `MapEastIndies`,
  `MapAmazonBasin`, `MapHimalayas`, `MapConquest`, `MapScenario`, …), plus `map.cpp` and
  `TerrainGroups.cpp` free functions. Recovered from RTTI vtables, not guessed.
* **Units and animals.** `0x005d7460`, `0x005d79e0` (`Animal` vtable), `0x0060ee50`
  (`Unit` / `AnimalData` / `AnimalOut` vtables).
* **The BHS script API.** `rand_int` and `rand_real` (§5) read this object. Scripts run
  inside the lockstep simulation.
* `game.cpp` (`0x00586440`, `0x00589bb0`, `0x0058a500`, `0x0058a600`), `setupwin.cpp`,
  `scenariofile.cpp`.

**Cosmetic** [measured, by compiland attribution]: `0x00ecba54` and `0x00e87d3c` are
touched only from `jukebox.cpp` — music selection. `0x00ee1aec` is touched by exactly one
graphics function.

**Not established:** the middle of the table. `0x00eb697c` is dominated by `Surf.cpp` and
`Scene` (presentation) but has one `Animal` caller and one `game.cpp` caller; `0x00e85f0c`
mixes `jukebox.cpp` with `commandpackage.cpp` and `gamelog.cpp`. Whether those specific
callers feed simulation state is an open question (§8), and I will not guess it.

Method note: source-file attribution comes from the `__FILE__` string literals the BHG
assert rig leaves in each compiland (515 such markers, string pooling is off in this
build), mapped to the nearest code address. It is a *proximity* heuristic over MSVC's
per-object contiguous layout — good enough to name a module, not good enough to name a
function. 170 of the 239 main-stream call sites fall outside 0x6000 of any marker and are
reported as unattributed rather than invented.

---

## 5. The script API pins the main stream down

`ron-data/scriptfunctions.xml` declares four RNG functions [measured, shipped data file]:

```xml
<FUNC type="48" name="rand_int"      func_index="614"><PARAM name="num_min" type="6"/>…</FUNC>
<FUNC type="1"  name="rand_real"     func_index="615"/>
<FUNC type="3"  name="rand_seed"     func_index="616"><PARAM name="num_new_seed" type="6"/></FUNC>
<FUNC type="48" name="rand_get_seed" func_index="617"/>
```

Four functions sit **contiguously, in that index order**, at `0x009e1890`–`0x009e1937`
(the block preceding the nearest `scriptfunctions.cpp` `__FILE__` marker at `0x009e59e1`;
the compiland attribution is proximity, the identification below is not), and their bodies
match the declared names and arities exactly [measured]:

| VA | script name | body |
|---|---|---|
| `0x009e1890` | `rand_int(min, max)` | `ecx = [0xc06184]; push [max]; push [min]; call 0xa39d70` |
| `0x009e18b0` | `rand_real()` | LCG + float mapping on `[0xc06184]`, returns via `st(0)` |
| `0x009e1900` | `rand_seed(n)` | `if (*n < 0) [0xc06184]->state = timeGetTime(); else = *n;` |
| `0x009e1930` | `rand_get_seed()` | `return [0xc06184]->state;` |

That is four independent agreements (order, count, arity, semantics) between a shipped data
file and the binary. It is the strongest single piece of naming evidence in this report.

`rand_real` is the only user of the float form outside the class itself — note it returns
in `st(0)` (an ordinary `__cdecl` float return) while `0x00a39cf0` returns in `xmm0`.

---

## 6. Seeding

**Seeding is deterministic per match, and the seed is an explicit integer.** [measured]

1. **The seed is a lobby key.** `game_seed` appears as an ASCII key string in 13 places in
   `.rdata` (once per compiland that references it), alongside `map_size`, `gamespeed`,
   `starting_resources`, `pop_limit`, `is_ranked`, … — the Steam lobby metadata dictionary.
   At `0x009636d1` the string at `0xafd3fc` is looked up and the value run through `strtol`
   (`[0xac54a8]`) into field `+0x22` of two peer structs [measured]. So every client in a
   multiplayer match receives the same integer seed out of band.
2. **`Map` vtable slot 25 — `0x0068bc90` (`map.cpp`) — installs it:**
   ```asm
   0x68bcb9  mov  dword ptr [ebx + 0x110], eax     ; arg0 -> map object
   0x68bcbf  test ecx, ecx                          ; ecx = arg1 = seed
   0x68bcc1  js   skip                              ; negative seed -> leave both alone
   0x68bcc3  mov  eax, dword ptr [0xc06188]         ; -> game object at 0x00c097e8
   0x68bcc8  mov  dword ptr [eax + 0x7c], ecx       ; persistent record of the seed
   0x68bccb  mov  eax, dword ptr [0xc06184]
   0x68bcd0  mov  dword ptr [eax], ecx              ; live simulation RNG state
   ```
   Both the *record* and the *live state* are set from the same value.
3. **Map generation derives sub-seeds from the record, not from the stream.** `map.cpp`
   reads `[0x00c06188]+0x7c` and multiplies it by small odd constants —
   `imul eax, [ecx+0x7c], 0x13` / `0x1d` / `0x25` (19, 29, 37) at `0x00694c5b`,
   `0x00694c7b`, `0x00694cc0` — so different map features get decorrelated but reproducible
   sub-seeds [measured].
4. **Saves / scenarios restore it.** `0x009a19d0` (`scenariofile.cpp`) does
   `[0x00c06184]->state = [eax+0x10]`, i.e. it reloads a stored state word rather than
   re-deriving it [measured]. `0x009a0950` writes `[0x00c06188]+0x7c` out.
5. **Reset to zero on teardown.** `0x00594ecf` and `0x00586d26` (`game.cpp`) set the live
   state to 0 [measured].
6. **The only non-deterministic path is opt-in and script-visible:** `rand_seed(n)` with
   `n < 0` seeds from `timeGetTime()` (`0x009e190c`). `0x0068bc90` explicitly refuses to
   touch anything when the seed is negative. There is **no** `srand`, no `rand`, no
   `QueryPerformanceCounter` and no `GetTickCount` on any seeding path — the import table
   has no `rand`/`srand` at all [measured].

Consequence for us: an offline batch environment can set the seed directly (one `u32`) and
get a bit-reproducible match, and the replay/save format already carries the seed word.

---

## 7. What else is in there (and is not the RNG)

**MT19937 is present but is used exactly once, for one shuffle, with the default seed.**
[measured]
`0x00456890` and `0x004568e0` are the two halves of the standard MT19937 twist (N = 624,
M = 397, `0x9908B0DF`, `0x7FFFFFFF` lower mask; loop trip counts 227 / 396 / 1 exactly as
in the reference implementation), and `0x00456820` tempers (`>>11 & d`, `<<7 & 0x9D2C5680`
encoded as `(y & 0xFF3A58AD) << 7`, `<<15 & 0xEFC60000` as `(y & 0xFFFFDF8C) << 15`,
`>>18`) with a rejection loop. Its **only** caller is `0x00455560`, which is
`std::shuffle`'s Fisher–Yates loop, whose **only** caller is `0x0058c1a0` in `game.cpp`.
That function inlines `init_genrand(5489)` — `mov esi, 0x1571` at `0x0058c2a1`, then
`esi = (esi ^ (esi >> 30)) * 0x6C078965 + i` — i.e. a **default-constructed
`std::mt19937`**, so the permutation it produces is the same on every run. It shuffles up
to 8 entries drawn from the player array at `[0x00c061ec]` (stride `0x8c`). It is
deterministic, but it is *not* seeded from `game_seed` and it is not the general RNG.

**There is no other LCG in the image.** A full linear sweep of `.text`
(2 126 250 instructions, capstone `skipdata`) found every `imul`/`mul` with an odd
immediate above `0x10000`. The complete list of plausible-PRNG constants is:
`0x0019660D` (4 sites — the LCG above), `0x6C078965` (1 site — the MT seeder), `0x01000193`
(6 sites — FNV-1a string hashing). **`214013`, `1103515245`, `22695477`, `69069`, `16807`
and `1013904223` as a multiplier appear nowhere.** [measured]

---

## Refuted

* **`0x00846450` is not the RNG, and is dead code in this image.** [measured] An
  exhaustive scan finds **zero** `E8 rel32` calls to it, **zero** `E9 rel32` jumps to it,
  and **zero** occurrences of the little-endian dword `0x00846450` anywhere in any section
  (so it is in no vtable and no jump table). It computes `((a*a*b) mod (|hi−lo|+1)) + lo`
  and nothing in the shipped binary invokes it. Caveat worth keeping: `0x009e1890`
  (`rand_int`) is *also* unreferenced despite certainly being live, so this build evidently
  retains unreferenced COMDATs — "unreferenced" means "not called *by this symbol*", not
  "semantically absent". Either way, `hash_into_range` is **not** the generator, and
  `docs/provenance-ledger.md`'s refusal to call it "the RNG" was correct.
* **The RNG is not a hidden global-state function.** I scanned every function under 400
  bytes for the read-modify-write-the-same-global shape with a multiply; the hits were all
  struct-stride arithmetic (`0x2c`, `0x88`, `0x6eec`, …). The generator is a *class*, and
  its state is an object field.
* **The RNG has no `__FILE__`/`__LINE__` parameter.** The `RandomLogEntry { frame, file,
  line, seed }` shape is **[reported]** and I could not corroborate it: `in_range` takes two
  arguments and no file/line, and the 1 265 `.cpp` literals in `.rdata` belong to the BHG
  run-time-warning rig (`RUN TIME WARNING: `, ` run time ERROR`, `BHG RTS` at
  `0xb1771c`–`0xb177bc`), not to RNG logging. The RTTI names
  `ObjectArray<RandomLogEntry>` and `ArrayBase<RandomLogEntry>` **are** present at
  `.data:0xc9425c` / `0xc94ad8` [measured], so the *type* exists — but whichever code
  populates it does not go through `Random::in_range`.

---

## Open — what I could not establish

* **A per-call-site sim/cosmetic partition of the 93 main-stream callers.** The stream-level
  split is measured; the middle-tier streams (`0x00eb697c`, `0x00e85f0c`) genuinely mix
  presentation and non-presentation callers and I have no evidence that resolves them.
  Resolving it wants the reachability answer from the tick loop, which is not this lane.
* **Whether the RNG state is inside the lockstep checksum.** No function in the
  `checksums.cpp` compiland (`0x00936560`–`0x00936b70`) references `0x00c06184` or
  `0x00e37a8c` [measured, from an xref index over 46 564 functions], and none of the 38
  `…Sync` channel names is RNG-related. That is *absence of evidence in one place*, not a
  negative result — a `DataWalk` descriptor could reach the seed indirectly. Worth one
  hour from whoever owns the checksum lane.
* **Where `rand_int`, `rand_real`, `rand_seed`, `rand_get_seed` are dispatched from.** They
  have no direct callers and no pointer-table entry that I can find, so the script VM must
  reach them some way I have not identified (or inline them). Does not affect the
  semantics, but it blocks tracing script-driven draws.
* **Who calls `Map` vtable slot 25** (`0x0068bc90`), i.e. where the seed argument comes
  from at match start. It is a virtual call; I did not chase the indirect call sites, so
  the `game_seed` lobby value → `Map::…(…, seed, …)` link is *inferred from both ends*,
  not traced end to end.
* **`Random::exchange_seed` (`0x00a39d30`) has no callers.** It swaps `*this` with its
  argument and returns the old value — a save/restore idiom — but nothing in this image
  uses it.
* **`in_range`'s SEH frame was not exercised.** The difftest never raises an exception, so
  the unwind path is untested.

---

## For whoever owns `crates/don-sim/src/mechanics.rs`

I did not write to that file (lane discipline). The model below is the exact one that was
differentially tested; drop it in with the ledger entry from §2.

```rust
/// `s <- s*1664525 + 1013904223`. Derived from VA 0x00a39cf0 / 0x00a39ea5.
#[inline]
pub fn lcg_step(s: u32) -> u32 {
    s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223)
}

/// `Random::next_float()` at VA 0x00a39cf0 — uniform in [0, 1) with 23 bits of entropy.
#[inline]
pub fn rand_real(s: &mut u32) -> f32 {
    *s = lcg_step(*s);
    f32::from_bits((*s & 0x007f_ffff) | 0x3f80_0000) - 1.0
}

/// `Random::in_range(lo, hi)` at VA 0x00a39d70 — HALF-OPEN [lo, hi).
///
/// `lo == hi` returns `lo` **without advancing the state**; `lo > hi` is swapped.
/// Bounds outside +/-0xFFFF are what the engine itself warns about: the product
/// overflows and the result stops being uniform. Faithful, not a porting bug.
#[inline]
pub fn rand_int(s: &mut u32, lo: i32, hi: i32) -> i32 {
    if lo == hi {
        return lo;
    }
    let (lo, hi) = if lo > hi { (hi, lo) } else { (lo, hi) };
    let range = (hi as u32).wrapping_sub(lo as u32);
    *s = lcg_step(*s);
    let prod = (*s & 0xffff).wrapping_mul(range);
    ((prod >> 16) as i32).wrapping_add(lo)
}
```

Ledger entry to add to `docs/provenance-ledger.md`:

| field | value |
|---|---|
| source | `riseofnations.exe` VA `0x00a39cf0` (`__thiscall`), VA `0x00a39d70` (`__thiscall`, 2 stdcall dwords, `ret 8`) |
| implementation | `crates/don-sim/src/mechanics.rs` (`lcg_step`, `rand_real`, `rand_int`) |
| tier | **B** |
| evidence | 999 950 chained `next_float` trials (state + f32 bit pattern) and 1 500 012 `in_range` trials (return value + resulting state), 0 mismatches; distributions in `docs/derivation/rng.md` §2 |
| harness | `crates/oracle/src/bin/rng.rs`, `i686-unknown-linux-musl` on hbox |
| streams | main simulation stream is the object at VA `0x00e37a8c`, reached via the pointer at `0x00c06184`; six further independent streams enumerated in §4 |
| seeding | `game_seed` lobby key → `Map` vtable slot 25 (`0x0068bc90`) → both `Game+0x7c` and the live state. Deterministic; the only clock-seeded path is `rand_seed(n < 0)` |

---

## Reproduction

Static analysis (no Ghidra lock taken; capstone only):

```sh
cd /Users/ember/dev/don/ron-bin
uv run --with pefile --with capstone python -   # scripts inline in this lane's transcript
```

The three artefacts the analysis built, in
`/private/tmp/claude-501/-Users-ember-dev-don/…/scratchpad/`:
`xref.pkl` (call graph + immediate/displacement xrefs over 46 564 functions from
`schema/islands.jsonl`, 1 540 059 instructions, 104 625 call sites),
`rtti.pkl` (1 871 type descriptors → 1 750 vtables → 4 000 virtual functions),
`markers.pkl` (515 `__FILE__` compiland markers).

Oracle commands are in §2.
