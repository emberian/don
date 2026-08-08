# Two instrumentation runs, finished

**Lane:** instrumentation. **Date:** 2026-08-08. **Machines:** hbox (oracle sweep), Parallels
"Windows 11" guest + arm64 Mac (heap crawl).

Both runs had been started and abandoned. This finishes them, and in both cases the
interesting result is *not* the headline number but a methodological correction that the
partial run would have shipped as a fact.

| claim | tier | evidence |
|---|---|---|
| The first sweep did not "time out" — it **hung**, and was still burning CPU 9.5 h later | measured | `ps`: child pid 1157591, state `R`, `TIME 09:28:58` |
| Full sweep completes: **2,135 ISLANDs, 1,335 callable, 791 fault, 9 timeout**, 1,568 s | measured | `schema/sweep.jsonl` summary row |
| **Zero non-determinism at full scale — confirmed.** 1,335/1,335 stable across three back-to-back calls in one process; **1,316/1,316 stable across four independent processes** under matched probe parameters | measured | `schema/sweep.jsonl`, `schema/sweep-recheck.jsonl` |
| The partial run's "165 callable / 0 non-deterministic" understated callability; the real rate is 62.5% | measured | 1,335 of 2,135 |
| **676 of the 1,335 "callable" ISLANDs are `void`** and never write `eax`; 597 of them are a single `ret` | measured | all 676 returned the identical harness leftover `0xffe1e740` |
| Only **659 ISLANDs actually return a value**; of those, **119 are pure functions of their arguments** | measured | below |
| Live heap: **36 live `Unit` objects, not 600** — the pool is 94% free slots | measured | `Unit+0x18` type-pointer test, cross-checked against the running game |
| `UnitType`/`BuildType` hold their **type index at `+0x04`**, an exact permutation of 50..413 and 414..542 | measured | 493/493 objects, zero collisions |

---

# RUN 1 — the ISLAND characterisation sweep

## What was actually wrong

The brief said the run "stopped at 463 of 2135 when its shell timed out". It had not. The
process was **still alive nine and a half hours later**, and its forked child had
accumulated `09:28:58` of CPU in state `R`:

```
PID     PPID     ELAPSED STAT   TIME CMD
1138204 1138203  09:34:49 SN  00:00:00 oracle sweep islands.jsonl sweep.jsonl
1157591 1138204  09:29:14 RN  09:28:58 oracle sweep islands.jsonl sweep.jsonl   <- spinning
```

`in_child` waits on `waitpid(…, 0)` with **no timeout**. Fork isolation converts a *fault*
into a report, but it does nothing about a *non-terminating* probe, and probing unknown
functions with fabricated pointers produces plenty of those. One function wedged the run at
candidate 464 and the sweep would never have finished, on any shell budget.

The culprit is identifiable from the finished run: **8 of the 9 timeouts are the same
target**, `thunk_FUN_00516ce0`, aliased at eight addresses.

## What was rebuilt

`crates/oracle/src/main.rs`, mirrored to `hbox:~/don-oracle`. Three changes, none of them
cosmetic.

**1. A calling-convention-correct trampoline.** The old probe did `asm!("call {f}")` with
`ecx` set and *no arguments at all* — so its `varies_with_state` flag could only ever
measure `this`-dependence, and the `arg_dep` counter it declared was dead code. Worse, a
raw `call` on a `__stdcall`/`__thiscall` candidate (callee pops) leaves `esp` above where
the caller expects it. That corrupts the harness stack silently, on a large fraction of the
image, without ever crashing.

`oracle_call4` is a `global_asm!` trampoline that sets up a frame, pushes four dwords, loads
`ecx`, calls, and **restores `esp` from `ebp`** — correct for `__cdecl`, `__stdcall` and
`__thiscall` alike. It saves `ebx`/`esi`/`edi` in case the callee trashes them, and it
captures `eax`, `edx`, and the x87 status word (32-bit MSVC returns floats in `st(0)`, not
in a register, so `fninit` before the call and a `TOP != 0` test afterwards is what detects
a float return at the machine level).

**2. Two independent timeouts plus a parent-side kill.** In the child, `setitimer(ITIMER_REAL)`
for wall-clock and `RLIMIT_CPU` as a backstop; `RLIMIT_NPROC=1` so a probe cannot fork a
grandchild that would hold the result pipe open past its own death, and `RLIMIT_FSIZE=0` so
it cannot write to the tree. In the parent, `poll()` with its own deadline and a `SIGKILL`.
`SIGALRM`/`SIGXCPU` are reported as `timeout`, distinct from `fault`. Every output line is
flushed, so a killed run keeps everything it had already established.

**3. Sharper questions.** Intra-process determinism (three back-to-back calls in *one*
child) is now asked separately from inter-process determinism (a fresh fork, identical
inputs) — a function accumulating into a global is stable across forks and unstable within
one, and the old probe could not tell those apart. Argument-dependence and `this`-dependence
are varied independently. Returns are classified as in-image, in-arena, or a plain value,
and in-image returns are rebased to the preferred base so they are comparable with every
static address in the repo.

### The harness is proved before it is trusted

Per the project rule that a failure must be unambiguously the harness or unambiguously the
image, `oracle selftest` now also exercises the trampoline against **machine code we wrote
ourselves**, in every shape the sweep must survive:

```
selftest: OK (hand-written cdecl add returned 42 through fork isolation)
selftest: OK (trampoline: cdecl, stdcall callee-pops, thiscall, x87 float return,
              timeout guard, fault guard)
```

That is: a `__cdecl` add; a `__stdcall` `ret 8` multiply (which fails loudly if `esp` is not
restored); a `__thiscall` read of `[ecx+4]` checked against the exact arena fill; an
`fld`-and-return checked for `has_float` *and* value; `jmp $` classified as `timeout` in
under 2 s; and a call to address 4 classified as `fault`.

## Results

```sh
ssh hbox; cd ~/don-oracle
nice -n 15 taskset -c 0-3 cargo build --target i686-unknown-linux-musl
setsid nohup nice -n 15 taskset -c 0-3 \
  ./target/i686-unknown-linux-musl/debug/oracle sweep islands.jsonl sweep.jsonl &
```

2,135 ISLANDs in **1,568 s**, detached so no shell budget applies. Results at
`schema/sweep.jsonl` (one JSON object per function, plus a header and a summary row).

| outcome | n | share |
|---|---|---|
| callable | 1,335 | 62.5% |
| fault (`SIGSEGV` 790, `SIGTRAP` 1) | 791 | 37.0% |
| timeout | 9 | 0.4% |

**184 of the 791 faulting candidates become callable when the `this` buffer is zero-filled
instead of garbage-filled.** Those are functions blocked only by implausible pointers, not by
needing real game state — a cheap 23% expansion of the reachable set whenever someone wants
it. The zero-fill probe runs for every candidate and is recorded as `zero_ok`/`r_zero`.

### Confirming the zero-non-determinism finding — and correcting it

The raw sweep looks alarming: `det_intra: 1335`, but `det_inter: 659`. Taken at face value
that says half of all callable ISLANDs are non-deterministic across processes, which would
be very bad news for a bit-exactness ambition.

It is an artifact, and the artifact is mine. The sweep compares a **3-repetition** child
against a **1-repetition** child; those two children do not have identical stacks, because
the harness allocates a different result buffer in each.

Two tests settle it.

**Test 1 — matched parameters.** `oracle recheck` re-probes every callable candidate as
**four identical children**, same fill, same arguments, same repetition count:

```json
{"kind":"summary","rechecked":1316,"agree_with_matched_probes":1316,
 "disagree":0,"of_which_look_like_stack_addresses":0}
```

**1,316 of 1,316 agree. Zero disagreements.**

**Test 2 — what the 676 actually are.** Every one of the 676 flagged functions returned the
*identical* value `0xffe1e740` — a stack address, and specifically the leftover `eax` my own
trampoline holds on entry. Their instruction counts give it away:

| instructions | flagged as "non-deterministic" | stable |
|---|---|---|
| 1 | **597** | 17 |
| 2 | 19 | 202 |
| ≥10 | 40 | 220 |

597 of 676 are a **single `ret`**, and 294 are SEH `Catch@`/`Unwind@` funclets.
`FUN_0041bfd0` is literally `ret 8`. `FUN_0041b100` zeroes fourteen fields of `[ecx]` and
returns. These are **`void` functions**: they never write `eax`, so reading `eax` returns
whatever the caller left there.

I also tested and **refuted** the more interesting hypothesis that these were functions
taking more than four arguments and reading a fifth off uninitialised stack: disassembling
all of them, **0%** of the flagged set references `[ebp+0x18]` or beyond.

> **Verdict: the zero-non-determinism finding is confirmed at full scale.** No ISLAND
> exhibits internal non-determinism. What the partial run could not see is that
> `det_inter` is not a determinism test at all — it is an excellent **`void`-function
> detector**, and 676 of the 1,335 "callable" ISLANDs return nothing.
>
> Scope, stated plainly: this shows these functions have no internal entropy source under a
> fixed fabricated environment. It is Tier-B evidence about the sampled inputs, not a proof
> of determinism over the input domain, and it says nothing about the parts of the engine
> that are not ISLANDs.

### How many are genuinely callable, and what do they depend on

Of 2,135 ISLANDs, **659 return a value** (1,335 callable minus 676 `void`). Restricting the
dependence question to those 659 — it is meaningless for a `void` function, and the raw
summary's `varies_this: 806` is contaminated by exactly that (671 of the 676 `void`
functions are spuriously flagged) —

| the result depends on | n | share of 659 |
|---|---|---|
| **neither** — constant, or reads only globals | 405 | 61% |
| **arguments only** — pure function of its arguments | **119** | 18% |
| **`this` only** — object accessor / computation | 110 | 17% |
| both arguments and `this` | 25 | 4% |

### Address computations versus formulas

| return shape | n |
|---|---|
| plain computed value | 599 |
| address inside the fabricated `this` arena | 51 |
| address inside the mapped image | 9 |
| float in `st(0)` | **0** |

Address-returning functions are **60 of 659 (9%)**, and they are not the interesting kind.
Eight of the nine in-image returns are SEH `Catch@` funclets returning their own landing-pad
address — which is exactly what a catch funclet should do, and a nice incidental check that
the trampoline is reporting real `eax`. The 51 arena returns are `return this;`-style
constructors and accessors. **There is no population of "address computation" ISLANDs
masquerading as formulas**; the 405 constant-returners are the real degenerate class.

No ISLAND returns a float. Given that the entire IEEE hazard surface is eight CRT imports
and that the damage pipeline and A\* contain no floating point, this is consistent — but it
also means the ISLAND set is not where the float risk lives, and float work should not be
prioritised from this list.

## RANKED WORKLIST — best differential-testing targets

Ranking prefers: controllable from arguments (50) over `this` (15); penalises returning a
pointer (−35) and returning a constant (−40); rewards real arithmetic (`int_muldiv` ×3),
substance (`insns`/10) and floating point (+8). SEH funclets and CRT helpers are excluded
from the game-logic list. **248 game-logic candidates** score positive; **113 are pure
deterministic functions of their arguments**, which is the tier that can be difftested with
no object fabrication at all.

Full machine-readable list: `schema/sweep.jsonl` (add `worklist.json` regeneration from the
analysis in this doc if needed).

| # | VA | name | insns | mul/div | fp | depends on |
|---|---|---|---|---|---|---|
| 1 | `0x004fe0c0` | `FUN_004fe0c0` | 34 | 1 | 1 | args+this |
| 2 | `0x00a48000` | `FUN_00a48000` | 115 | 4 | 0 | **args** |
| 3 | `0x004fa930` | `FUN_004fa930` | 42 | 1 | 0 | args+this |
| 4 | `0x005503f0` | `FUN_005503f0` | 249 | 0 | 0 | **args** |
| 5 | `0x0047bbf0` | `FUN_0047bbf0` | 43 | 0 | 0 | args+this |
| 6 | `0x008fd9b0` | `FUN_008fd9b0` | 10 | 1 | 0 | args+this |
| 7 | `0x00541120` | `FUN_00541120` | 34 | 0 | 0 | args+this |
| 8 | `0x005528b0` | `FUN_005528b0` | 123 | 2 | 0 | **args** |
| 9 | `0x00902520` | `FUN_00902520` | 7 | 1 | 0 | args+this |
| 10 | `0x0046c340` | `FUN_0046c340` | 23 | 0 | 0 | args+this |
| 11–19 | `0x005237a0`, `0x005238d0`, `0x00523950`, `0x00523990`, `0x00523a10`, `0x00523a50`, `0x00523a90`, `0x0041fe70`, `0x00681e30` | — | 11–29 | 0 | 0 | args+this |
| 27 | `0x00550840` | `FUN_00550840` | **495** | **25** | 0 | this |
| 28 | `0x00550eb0` | `FUN_00550eb0` | **1277** | **46** | 0 | this |
| 31 | `0x006f9a50` | `FUN_006f9a50` | 29 | 0 | 1 | **args** |
| 32 | `0x0073d3d0` | `FUN_0073d3d0` | 23 | 0 | 4 | **args** |
| 33 | `0x00846450` | `FUN_00846450` | 18 | 3 | 0 | **args** |
| 35 | `0x00738360` | `FUN_00738360` | 25 | 2 | 0 | **args** |

Three notes on this list.

- **`0x00550840` and `0x00550eb0` are the standouts.** 495 and 1,277 instructions with 25
  and 46 multiply/divide operations, deterministic, `this`-driven, returning a plain value.
  That is by far the densest integer arithmetic reachable in the ISLAND set. They cost an
  object fabrication, and they are worth it.
- **`0x00846450` is on the list and should stay off the roadmap.** It is already recorded as
  correctly derived dead code called by nothing and *not* the RNG. Its appearance here is a
  useful negative control: the ranking finds it precisely because it is a clean pure integer
  function of its arguments, which is exactly what makes it easy and exactly what makes it
  useless.
- **Six CRT helpers with fully known semantics** — `__alldiv`, `__alldvrm`, `__allmul`,
  `__aulldiv`, `__aulldvrm`, `___common_srl` — are callable and pure. They are not game
  logic, but they are free end-to-end validation of the whole oracle path: 64-bit divide
  against a Rust `i64` model should be exact over millions of samples, and if it is not, the
  harness is wrong rather than the model.

Also worth knowing: `schema/islands.jsonl` is not a clean function list. 294 of the entries
are SEH funclets and 597 are single-`ret` stubs; roughly 40% of the "ISLAND" population is
not a function anyone would want to test.

---

# RUN 2 — the typed live-heap crawl

## The scanner already existed; this used it

Both `crates/donscan/` and `docs/tooling/native-scanner.md` were already present from the
native-tools lane, which established the hard parts: `cargo-xwin` cross-linking from the
Mac, image-base detection via `AddressOfEntryPoint`/`SizeOfImage` (the loader rewrites
`ImageBase` in the mapped header), a ~0.5 s full scan at ~2 GiB/s, and a null-model control
putting arithmetic-coincidence background at 33–136 hits per 660 MiB against 158,955 real
ones. **Neither failed approach from the brief was repeated**, and no scanner was rebuilt.

That report also names its own biggest gap: *"Live vs free pool slots. The single biggest
gap."* It measured `Unit = 600` and said, correctly and emphatically, that 600 is **pool
capacity, not a live count**. Closing that gap is exactly the caveat this run was asked to
apply, so that is what this run did.

One addition to the tool: **`--dump <hex>:<len>:<file>`**, writing raw target memory to a
file in the guest. `--read` only hexdumps, a 5× text expansion, and reading object interiors
is the next step the scanner doc explicitly anticipates. On a short read it falls back to
page-at-a-time so one unreadable page cannot cost a whole region, zero-fills what it could
not read, and reports the shortfall on stderr rather than passing zeros off as memory.

Transfer used the host-only network per the existing doc (guest `10.211.55.6`, host
`10.211.55.2:8899`, `curl.exe` `GET`/`PUT`), hash-verified:
`f079b90c…6f52` matched on both sides.

## Game state at capture — the ground truth to check against

`prlctl capture` of the running game (pid 148, **not killed, not restarted**):

> **Dutch: Ancient Age. Population 13/25. Cities 1/1.**

A small early-game skirmish. This is the state every count below has to be plausible for.

## Finding the live/free discriminator

Rather than guess at a validity flag, the discriminator was derived from ground truth we
already had. Every pool slot was dumped (Unit 216 KiB, Build 168 KiB, Animal 166 KiB,
City 38 KiB, in 25 clusters), and **the dump was verified before it was analysed**: all
1,760 slots begin with their own class's rebased vtable pointer, 1,760/1,760, zero
mismatches.

Then, for every 4-byte offset in each struct, count how many slots hold a value that is a
**known type-object address**:

| class | offset | slots whose `+0x18` is a type pointer | null | other |
|---|---|---|---|---|
| `Unit` | `+0x18` → a `UnitType` | **36** / 600 | 564 | 0 |
| `Build` | `+0x18` → a `BuildType` | **28** / 600 | 572 | 0 |
| `Animal` | `+0x18` → a `UnitType` | **135** / 400 | 265 | 0 |

The field is at the **same offset `+0x18` in all three classes** — consistent with the
shared `Object` base the RTTI hierarchy implies, and with the native-scanner lane's finding
that `Unit` and `Animal` both carry `OrderList` at `+0xc8`. The class correspondence is
right in each case (`Build`→`BuildType`, `Unit`/`Animal`→`UnitType`), there are **zero**
slots holding anything other than a valid type pointer or null, and the split is clean.

### Naming every object: the type index is at `+0x04`

`schema/live/type-names.txt` holds 364 `UnitType` names at indices 50–413 and 129
`BuildType` names at 414–542 — exactly matching the 364 and 129 type objects on the heap.

Heap-address order is **not** index order (the type objects sit in 9 and 5 clusters, so
sorting by address interleaves them; testing that hypothesis produced "30 Dreadnoughts" in
the Ancient Age, which is how it was caught). The index is stored in the object. Searching
every offset for one whose values form an *exact permutation* of the expected range:

> **`UnitType+0x04` and `BuildType+0x04` hold the type index** — an exact permutation of
> 50..413 over the 364 `UnitType` objects and of 414..542 over the 129 `BuildType` objects.
> 493/493, no collisions, no gaps. Self-validating: a wrong offset cannot produce a
> permutation.

## The live census, and the caveat applied

Two snapshots 30 s apart during live play:

**`Unit`: 36 live of 600 slots** — not 600.

| n | Δ30s | type |
|---|---|---|
| 30 | +4 | Citizen |
| 3 | | Scout |
| 2 | | **Armed Merchant (`MERCHANTDUTCH`)** |
| 1 | | Bark |
| | +3 | Fishermen |

**`Build`: 28 live of 600 slots.** 12 Farm (+2), 5 Small City, 5 Woodcutter's Camp (+1),
3 Library, 1 Dock, 1 Market, 1 University, +1 Temple.

**`Animal`: 135 live of 400 slots.** 120 Herd Fish, 9 Wild Bird (+1), 4 Herd Horse,
1 Herd Whales, 1 Gull Bird, plus +5 Farm Pig and +5 Farm Chicken.

Five independent things agree, none of which was used to derive the discriminator:

1. The units include **`MERCHANTDUTCH`** and the HUD says **Dutch**.
2. Villages, farms, woodcutter's camps, library, market, university — an **Ancient Age**
   build set, and the HUD says Ancient Age.
3. A **Dock** and a **Bark** and **120 Herd Fish and Herd Whales** — a water map, agreeing
   with each other.
4. Over 30 s the game built **+2 Farms** and gained **+5 Farm Pigs and +5 Farm Chickens**.
   Farms spawn farm animals; the causal link is visible in the diff.
5. **Zero objects died** and all 36 persisted in their slots — a peaceful early economy,
   which is what the screen shows.

> **The caveat, applied to my own numbers.** A vtable-pointer scan reports 600 `Unit` hits.
> **36 are live.** The raw hit count overstates live objects by **16.7×** for `Unit`, 21×
> for `Build`, 3.0× for `Animal`. Anyone reading a scan total as an entity count will be
> wrong by more than an order of magnitude. The correct procedure is: scan for candidates,
> then read `+0x18` and require a valid type pointer.

Two further limits I did not clear, stated rather than papered over:

- **The owner/player field is not established.** `Unit+0x2e`, `+0x3e` and `+0xb4` partition
  the 36 live units into 3 groups (16/10/10) and are plausible candidates, but no offset in
  `Build` gives the 5 Small Cities five distinct values, so the "one village per player"
  reading is unconfirmed. Without owner attribution, 30 Citizens cannot be reconciled
  against our own 13/25 population — the 36 span all players, and how many players there are
  is not settled here. **Do not attribute any of these objects to a player yet.**
- **`City` does not use the `+0x18` convention.** Its `+0x18` holds `0xffffff`/`0xffffffff`
  patterns and no type pointer, so live `City` count is still unknown. Cities also appear as
  `Build` objects of type `Small City`, which is probably the more useful handle.

Incidentally, `364 + 129 = 493` type objects with indices spanning **50..542 — exactly 493
consecutive values** — which makes `balance_index = type_index − 50` the obvious hypothesis
for the 493×493 table at `0x00C06AFC`. That is a *cross-check, not a derivation*: nobody has
yet read a name out of a type object and correlated it with a table row. It should not be
promoted until someone does.

---

## Artifacts

| path | what |
|---|---|
| `schema/sweep.jsonl` | 2,135 ISLAND characterisations + header + summary |
| `schema/sweep-recheck.jsonl` | 1,316 matched-parameter determinism re-probes |
| `schema/live/donscan-pid148-live.json` | full typed scan of the live process |
| `schema/live/gamestate-pid148.png` | the screen the counts were checked against |
| `crates/oracle/src/main.rs` | trampoline, timeouts, `sweep`, `recheck`, extended selftest |
| `crates/donscan/src/main.rs` | `--dump` raw memory extraction |

Nothing was committed and nothing was staged.

## What to do next

1. **`FUN_00550840` / `FUN_00550eb0`** — 495 and 1,277 instructions, 25 and 46 mul/div,
   deterministic. Fabricate the object and difftest. Highest arithmetic density available.
2. **The 113 pure-argument game functions** need no object at all. Difftest in bulk.
3. **`__alldiv` and friends** as an end-to-end oracle validation before trusting any of it.
4. **The 184 zero-fill-rescued faulters** are a cheap 23% expansion of the reachable set.
5. **The owner field.** Everything about per-player state is blocked on it, and the live
   heap is now typed well enough to hunt it properly.
