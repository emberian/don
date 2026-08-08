# Derivation: porting the damage pipeline, and testing it against retail

Lane: **damage-port**. Follows `docs/derivation/combat.md`, which derived `FUN_00644130`
structurally but deliberately declined to implement it. This lane implements it and then
attacks the implementation.

Every claim is marked **[measured]** (I ran it against the retail machine code or the
binary) or **[reported]/[inferred]**.

---

## Headline

**The damage chain is ported and it is Tier B at 7,986,695 differential trials, 0
mismatches** [measured] — and, more usefully, **31 of the 32 step-level mutations were
each caught by the harness**, so the zero is evidence rather than decoration. The one
mutation that slipped through is step 0, and §5.2 shows why no test could ever catch it.

The port is `crates/don-sim/src/mechanics.rs` (`damage`, `damage_traced`, `flank_level`,
`entrench_dir_level`, `balance_index`, `get_attack`, `get_armor`). The harness is
`crates/oracle/src/damage_env.rs` + `crates/oracle/src/damage_test.rs`.

Three things this lane established that were not previously known:

1. **`FUN_00644130` *can* be driven with fabricated inputs.** It is not an ISLAND, but it
   is *constructible*: build the two objects, four vtables, `RULES`, the game object, the
   player array, the map, both object tables and the city table, and every one of its ~30
   object-graph predicates becomes an input you set per trial. Section 2.
2. **Ghidra's decompilation of this function drops an instruction.** The attacker-mask
   fixup at `0x006442A3` is `and eax,0xFFFDFFFF` **`or eax,0x40`**; the decompiled C in
   `re/decomp-all/00644130.c` has only the AND [measured]. Section 5.
3. **That fixup cannot change the answer.** It touches only mask bits 6 and 17, and
   neither is tested downstream — deleting the entire step produced **0 mismatches over
   199,629 trials** [measured]. It is faithful *and* structurally inert, and those are
   different claims. Section 5.

And the folklore formula is refuted with a number, not an argument: moving armor
subtraction from step 22 to the end of the chain — "attack × modifiers − armor" — diverges
from retail on **50,164 of 199,629 trials (25.1%)** [measured].

---

## 1. Fidelity, stated per piece

There is no single tier for "the damage pipeline". Averaging one would be the kind of
laundering `docs/CHARTER.md` forbids.

| piece | tier | basis |
|---|---|---|
| the arithmetic chain, steps 1–9, 12–26, 28–31 | **B** | 7,986,695 trials against retail `FUN_00644130`, 0 mismatches, 8 seeds; each step separately pinned by mutation |
| `flank_level` (`0x0092CFE0`) | **B** | 500,017 inputs standalone (combat lane) **and** pinned inside the chain by coarse-boundary mutation (334 / 237 mismatches) |
| `entrench_dir_level` (inlined `0x00644E0E`) | **B** | pinned by boundary mutation, 7 mismatches / 199,629 |
| `balance_index` stride 493 and `i16` sign extension | **B** | exhaustive 493×493 standalone (combat lane) **plus** 136,409 / 158,447 mismatches when the harness's own stride / sign is mutated |
| `get_attack` / `get_armor`, **base path** | **B** | driven as the real retail functions inside every trial; the `UnitType[+0x1E8]` / `[+0x214]` links pinned at 150,964 / 99,529 mismatches |
| `get_attack` / `get_armor`, **upgrade path** | **unverified** | structurally unreachable in the harness; never executed |
| step 0, the attacker-mask fixup | **transcribed, provably inert** | executed 2,195,827 times; deleting it changes nothing (§5) |
| steps 10, 11, 27 | **unverified** | never executed — see §6 |
| step 12's `FUN_006DA000` route | **unverified** | the `defObj[+0x68] & 0x400000` route *is* covered; the team-comparison route is not |
| the `out_kind` enum | **not ported** | out of scope; written by retail, never compared (§6) |
| **which** predicates fire in a real game | **not established at all** | they are inputs; see §2 |

Nothing here is Tier A. Nothing here is verified in the proof-assistant sense.

---

## 2. Making a non-ISLAND function drivable

`FUN_00644130` is `__thiscall`, six stack dwords, `ret 0x18`. Before it returns it
dereferences: the attacker `Object` and its `UnitType`, the defender `Object` (through
*two* different global tables) and its `UnitType`, four vtables, the `RULES` singleton at
`[0x00C061E4]`, the game object at `[0x00C061E8]`, the per-player array at
`[0x00C061E0]`, the map at `[0x00C061D0]`, a coordinate table at `[0x00CAE5FC]`, a city
table at `[0x00C061D4]`, and an aux object behind `[0x00E85DDC]`. Handing it random bytes
faults on the first virtual call.

The harness builds all of it in a 2 MiB arena and points the globals at it. The key moves,
each of which was necessary:

* **Generated stubs, not fake data.** Every vtable slot gets a stub emitted at runtime as
  `mov eax, [abs32]; ret`, where `abs32` is a cell in a control block. Setting a predicate
  is then a dword write. Slots we never expect to be called get `xor eax,eax; ret`, so a
  surprise call returns 0 instead of jumping into noise.
* **A tech stub that reads its argument.** `UnitType::vtbl[0x60]` answers *several*
  questions through one slot — the attacker's is asked about techs `0x42`, `0x139` and
  `0x83`. A constant-returning stub cannot tell them apart, so the stub is
  `mov eax,[esp+4]; and eax,0xFF; mov eax,[tab+eax*4]; ret 8`. The six tech ids the
  pipeline asks about have distinct low bytes (`42 39 83 16 43 09`), so the truncation is
  lossless here [measured].
* **Devirtualisation, exploited.** The retail code compares `obj->vtbl[0xB8]` against
  `FUN_00653790` and, when equal, calls `type->vtbl[0x60]` directly instead. Writing the
  **relocated** address of `0x00653790` into the fabricated vtable takes that branch every
  time and keeps the two-argument indirect call off the table. Same trick for
  `Build::vftable` (`0x00B42174`) at `0x00644453` — both immediates are relocated, and
  using the preferred-base value there silently takes the wrong branch.
* **One global switch retires three hazards.** `FUN_006E1370` tests
  `[[0x00C061E8]+0x20] & 4` before anything else and returns false when it is set
  [measured]. Setting it keeps `get_attack`/`get_armor` on their base paths and keeps
  steps 11 and 27 from calling into player/tech machinery we have not modelled. It is
  also precisely *why* those steps are unverified — a deliberate, stated trade.
* **The one fault this cost.** `attacker->vtbl[0x3C]`'s return is handed straight to
  `FUN_0062DD90`, which dereferences `obj[+0x18][+4]`. A zero-returning default stub is an
  immediate null deref. Giving it a real object with type id `0x1B9` (a switch case that
  returns before touching the player array) fixed it.

**What this buys and what it does not.** It makes the *arithmetic* testable at volume. It
says nothing about when the real `Object::vtbl[0x18]` returns 1. The predicates are inputs
in the port for exactly this reason — `DamagePredicates` is a shape forced by the
evidence, not a convenience.

---

## 3. Evidence

```
$ ssh hbox && cd ~/don-oracle
$ nice -n 15 taskset -c 0-3 cargo build --release --target i686-unknown-linux-musl -q
$ for S in 1 6364136223846793005 2862933555777941757 3202034522624059733 \
           12345678901234567 999 424242 8675309; do
    ./target/i686-unknown-linux-musl/release/oracle damage 1000000 $S
  done
```

**8 seeds × 1,000,000 requested → 7,986,695 trials, 0 mismatches, 0 unexpected panics.**
The Rust side under test is `crates/don-sim/src/mechanics.rs`, md5
`f7291090b46dd29306846f9510f0af04` — the exact bytes committed, not an earlier revision.

Excluded from the 8,000,000: **10,204 trials (0.13%)** where the port's `idiv_trapping`
says retail would raise `#DE` (a zero or overflowing divisor at `0x006448B9` or
`0x00644D78`), and **3,101 trials (0.04%)** where the generated type-id pair would place
the balance-table write on top of one of the harness's own globals. Both are counted and
printed; neither is silent.

The skip path is strict: only a panic whose message begins `retail raises #DE` is a skip.
Anything else increments `unexpected_panics` and fails the run, because counting an
unrelated panic as a skip is exactly how a suite launders a divergence into a pass.

### Input distribution

Per trial, independently drawn:

* **Numeric fields** (attack, armor, rules constants, heights, frames, splash percent)
  from a mixture: 50% in `[-100, 100]`, 25% in `[-10000, 10000]`, 12.5% uniform over the
  whole `i32` range, 12.5% from `{0, ±1, ±100, 255, 256, i32::MAX, i32::MIN, i32::MAX/2,
  0x10000}`. The full-range slice is what exercises the wrapping `imul`s.
* **`balance_pct`** uniform over the `i16` range, written as an `i16` into the real table
  at `0x00C06AFC + 2*(atk*493 + def)` and read back by retail's own inlined arithmetic.
* **Angles** (`attack_dir`, both defender facings) from a mixture including the eleven
  boundary constants the two 0/1/2 classifiers can distinguish, because uniform noise
  lands on a sixth-of-a-circle boundary essentially never.
* **Object masks** biased onto the fourteen bit patterns the pipeline actually tests
  (`0x4 0x8 0x20 0x100 0x1000 0x2000 0x10000 0x10108 0x40000 0x200000 0x8000000
  0x10000000 0x80000000`), OR-ed with noise — uniform dwords would set each interesting
  guard about half the time but never in the rarer combinations.
* **Type ids** from a mixture that reaches the three id-keyed guards: `{0x32..0x35}`
  (step 6), `0x21D` (step 29), `{0x1BB, 0x1BC}` (the mask-fixup gate).
* **All 26 stubbed predicates** as independent fair coins.

### Per-step coverage, summed over the eight runs

"Coverage" here is not a proxy: `damage_traced` sets each bit **at the site that performs
the arithmetic, inside the same branch**, so a step cannot be reported as covered without
having run.

| step | trials in which it ran | | step | trials in which it ran |
|---|---:|---|---|---:|
| `0-maskfix` | 2,195,827 | | `16-mul25` | 997,503 |
| `2-armor133` | 3,573,361 | | `17-river` | 2,629,298 |
| `3-div3` | 173,730 | | `18-x1000` | 1,996,239 |
| `4a-mul3div4` | 172,935 | | `19-mul4` | 748,236 |
| `4b-div2` | 284,530 | | `20-flank` | 183,111 |
| `5-mul4` | 998,388 | | `23-overkill` | 109,211 |
| `6-div2` | 1,048,237 | | `23b-div2` | 27,375 |
| `7-div2` | 999,803 | | `24-rocky` | 2,626,989 |
| `8-redfort` | 463,386 | | `25-height` | 875,932 |
| `9a-mul2` | 1,994,206 | | `26-entrench` | 642,609 |
| `9b-armor+1` | 998,495 | | `28-floor1` | 975,667 |
| `12-mul2` | 1,219,876 | | `29-zero` | 164,704 |
| `13-idiv` | 1,995,838 | | `30-recapture` | 250,181 |
| `14-splashpct` | 3,994,983 | | **`10-add`** | **0** |
| `15-mul3` | 249,185 | | **`11-pct`** | **0** |

---

## 4. Mutation testing — why the zero means something

A differential test that agrees on everything might be testing nothing. So each step was
perturbed one at a time and the suite re-run. A step whose mutation produces **zero**
mismatches is a step this harness is blind to, and saying so is the deliverable.

`hbox:~/don-oracle/mutate.py` (50,000 trials per mutation) and `mut2.py` / `mut3.py`
(200,000 trials per control).

| mutation (50,000 requested) | mismatches / 49,912 | | mutation | mismatches / 49,912 |
|---|---:|---|---|---:|
| `02-armor133` 133→134 | 10,350 | | `18-x1000` ×1000→×1001 | 8,880 |
| `03-div3` ÷3→÷4 | 544 | | `19-mul4` ×4→×5 | 3,633 |
| `04a-3over4` ×3/4→×3/5 | 580 | | `20-flankpct` +100→+101 | 688 |
| `04b-div2` ÷2→÷3 | 981 | | `21-round` `(D+5)/10`→`(D+4)/10` | 2,630 |
| `05-mul4` ×4→×5 | 4,278 | | `22-armorsub` `−ARM`→`−ARM+1` | 36,984 |
| `06-div2` ÷2→÷3 | 4,487 | | `23-overkill` +256 bias | 433 |
| `07-div2` ÷2→÷3 | 4,281 | | `23b-div2` ÷2→÷3 | 131 |
| `08-redfort` 100→101 | 1,679 | | `24-rocky` +256 bias | 13,564 |
| `09a-mul2` ×2→×3 | 7,039 | | `25-height` `+`→`−` | 4,350 |
| `09b-armor1` +1→+2 | 1,966 | | `26-entrench` +256 bias | 3,115 |
| `12-mul2` ×2→×3 | 4,277 | | `26b-b98` +256 bias | 1,857 |
| `13-idiv` numerator +1 | 209 | | `26-dirlevel` boundary +1 | 7 |
| `14-splashpct` +1 bias | 123 | | `28-floor1` 1→2 | 5,869 |
| `15-mul3` ×3→×4 | 785 | | `29-zero` 0→−1 | 999 |
| `16-mul25` ×25→×26 | 2,507 | | `30-recapture` +256 bias | 1,542 |

Follow-up controls at 199,629 trials each, for the three the first sweep did not pin and
for the harness's own wiring:

| control | mismatches / 199,629 | reading |
|---|---:|---|
| `flank_level` low boundary `0xD5555555`→`0xC0000000` | 334 | pinned; the first sweep's one-point move was simply too weak to sample |
| `flank_level` high boundary `0x40000000`→`0x30000000` | 237 | pinned |
| flank entry guard `>= 0x2AAAAAAA`→`>= 0x2AAAAAAB` | 12 | pinned |
| harness balance stride 493→494 | 136,409 | **confirms the stride inlined at `0x00644178`** |
| harness balance value sign-flipped | 158,447 | **confirms `movsx` from `i16`** at `0x0064418E` |
| harness `UnitType[+0x214]` armor +1 | 99,529 | confirms the `get_armor` source field |
| harness `UnitType[+0x1E8]` attack +10 | 150,964 | confirms the `get_attack` source field |
| **armor subtraction moved to the end of the chain** | **50,164 (25.1%)** | **the community formula is wrong, measurably** |
| **attacker-mask fixup deleted entirely** | **0** | **structurally inert — see §5** |
| `div256` as `>> 8` instead of truncating division | 34,632 | confirms the `cdq; and edx,0xFF; lea; sar` bias-then-shift is truncation toward zero, from behaviour rather than from reading the idiom |

Everything the port does that can affect the answer is pinned by at least one mutation,
except the one step that provably cannot.

---

## 5. Corrections and new findings

### 5.1 Ghidra drops `or eax, 0x40` [measured]

At `0x006442A3` the machine code is:

```
006442a3 mov  eax, dword ptr [ebp - 0xc]      ; AM
006442a6 and  eax, 0xfffdffff                 ; clear bit 17
006442ab or   eax, 0x40                       ; set bit 6      <-- absent from the decomp
006442ae mov  dword ptr [ebp - 0xc], eax
```

`re/decomp-all/00644130.c` line 65 has only `local_10 = local_10 & 0xfffdffff;`. This is
the charter's "decompiled C is a hypothesis" rule earning its keep on the exact function
the project cares most about. Anyone reconstructing this pipeline from the decomp alone
inherits the omission silently.

`docs/derivation/combat.md` §3 also does not list this step; its table starts at step 1.

### 5.2 …and the fixup cannot change the result anyway [measured]

The fixup writes a **local** (`[ebp-0xC]`), and it only touches bits 6 and 17. The bits of
that local that are subsequently tested are 3 (step 2), 2 / 13 / 28 (step 20) and 31
(step 28). Bits 6 and 17 are never read. Step 8 re-reads the mask **from memory**
(`0x006445A4`), not from the local, so it does not see the mutation either.

Prediction: deleting the step changes nothing. Measured: **0 mismatches / 199,629**.

Both statements matter and they are different. The port keeps the step because it is what
the binary does; the ledger records that no execution can distinguish it. If a future lane
finds `[ebp-0xC]` escaping into a caller, this is where to look first.

### 5.3 The `out_kind` pointer is destroyed mid-function [measured]

`ebp+0x1C` arrives as the `int* out_kind` argument and is written through at
`0x006441A0`, `0x00644659`, `0x00644668`, `0x006446AC` and `0x006446B8`. At
`0x00644B2B` the compiler **reuses the argument slot** as scratch — it stores the flank
level there (`mov dword ptr [ebp+0x1c], eax`), and later the defender slot address
(`0x00644BE1`) and the attacker height (`0x00644D00`). So no write through `out_kind` can
occur after `0x00644B2B`, and any future port of the damage-kind enum is complete once it
covers the five sites above. Reading `param_6` as a live pointer past that point — which
the decompiled C's variable naming invites — would be wrong.

### 5.4 The entrenchment direction classifier is *not* `flank_level` [measured]

`0x00644E0E` inlines a 0/1/2 classifier that looks like `flank_level` (`0x0092CFE0`) and
shares its `0x40000000` split, but its reject test is `(x - 0x2AAAAAAA) > 0xAAAAAAAB`
rather than `x > 0xD5555555`. They disagree — at `x = 0`, `flank_level` returns 2 and the
entrenchment classifier returns 0. Both are ported separately (`flank_level`,
`entrench_dir_level`) and a unit test pins the disagreement, because collapsing them into
one helper is the kind of tidy, invisible error this project dies of.

### 5.5 `get_attack`'s upgrade term carries a rules multiplier [measured]

`docs/derivation/combat.md` reports "+10 × level" attack and "+1 × level" armor. The
machine code at `0x00646AD5`–`0x00646AE5` and `0x00647E62`–`0x00647E6F` is more specific:
the level is multiplied by `RULES[+0x8B8]` first, so the terms are
`10 × (military_level × RULES[+0x8B8])` and `1 × (military_level × RULES[+0x8B8])`. The
×10 / ×1 asymmetry — which is the ×10 attack scale showing through — is confirmed;
`RULES[+0x8B8]` was previously unnamed and is unchanged by this lane. This path is
**unverified**: the harness disables it.

### 5.6 The two object tables

`[0x00C0AB84]` and `[0x00C0AEC0]` are both indexed `base[player*28] -> ptr_array;
ptr_array[index]`, and the same defender is fetched through both within one call. The
harness **assumes** they resolve to the same object and points them at the same array.
That assumption is load-bearing and untested — if retail keeps two different pointers (a
base and an adjusted `this` for a multiple-inheritance sub-object, say), every field read
through `0x00C0AEC0` in this port reads the wrong offset. Flagged, not resolved. The
2026-08-08 live-process method (`docs/provenance-ledger.md`) is the way to settle it.

---

## 6. What I could NOT establish

* **Steps 10, 11 and 27 were never executed.** Step 10 needs `FUN_00646B00` and a real
  player/tech object; steps 11 and 27 need `FUN_006E1370` to return true, which the
  harness disables globally to keep `get_attack`/`get_armor` on their base paths. They are
  transcribed from the disassembly and marked `UNVERIFIED` in the port, with their two
  rules constants (`+0xBBC`, `+0x794`) quarantined in a separate `UnreachedTerms` struct
  so they cannot be mistaken for Tier-B material.
* **The upgrade branches of `get_attack` and `get_armor`** — same cause, same status.
* **Step 12's `FUN_006DA000` route.** The `defObj[+0x68] & 0x400000` route is covered
  1,219,876 times; the team-comparison route (which also needs `game[+0x24] == 2`) is
  pinned off.
* **`attacker_vf_0x20` vs `attacker_table_vf_0x20`.** Retail reads the same virtual
  through two different paths (`0x0064435F` directly on `this`, `0x00644307` via the
  object table). The harness resolves both to one object, so it cannot drive them apart.
  In a consistent world they agree; that they *are* the same object in retail is assumed.
* **The `out_kind` enum is not ported and not compared.** Retail writes it into the
  harness's buffer on every trial and the harness ignores it. It does not affect the
  magnitude, but "0 mismatches" is a claim about the return value only.
* **Which predicates fire in a real game.** Completely open. This is the largest
  remaining piece of combat and it is a world-state problem, not an arithmetic one.
* **The balance table's contents and storage extent.** Unchanged from
  `docs/derivation/combat.md` §5 and still needs a live read. This lane confirms the
  *address arithmetic* twice over (§4) and touches the contents question not at all.
* **`to_hit` (`+0x1EC`) and `attenuate` (`+0x1F0`)** remain unread by this function.
* **No Tier A.** Nothing here is proven over a whole input domain.

---

## 7. Reproduction

Disassembly (no Ghidra lock; capstone only):

```sh
cd /Users/ember/dev/don/ron-bin
uv run --quiet --with capstone --with pefile python - <<'PY'
import pefile
from capstone import *
pe = pefile.PE("riseofnations.exe")
t  = [s for s in pe.sections if s.Name.rstrip(b"\0") == b".text"][0]
base = pe.OPTIONAL_HEADER.ImageBase + t.VirtualAddress
data = t.get_data()
md = Cs(CS_ARCH_X86, CS_MODE_32); md.skipdata = True
for i in md.disasm(data[0x644130-base:0x645060-base], 0x644130):
    print("%08x %-8s %s" % (i.address, i.mnemonic, i.op_str))
PY
```

Differential test, vector capture, and the mutation sweep:

```sh
ssh hbox && cd ~/don-oracle
nice -n 15 taskset -c 0-3 cargo build --release --target i686-unknown-linux-musl -q
./target/i686-unknown-linux-musl/release/oracle damage 1000000 <seed>
./target/i686-unknown-linux-musl/release/oracle damage-vectors    # unit-test expectations
nice -n 15 taskset -c 0-3 python3 mutate.py 50000                 # per-step mutation sweep
```

`DAMAGE_TRACE=1` prints each scenario before it is handed to retail — that is how the
null-deref in §2 was found in one run.

Unit tests (all expectations captured, none computed):

```sh
cd /Users/ember/dev/don && cargo test -p don-sim
```

Files this lane owns:

* `/Users/ember/dev/don/crates/don-sim/src/mechanics.rs` — the port and its tests
* `/Users/ember/dev/don/crates/oracle/src/damage_env.rs` — the fabricated world
* `/Users/ember/dev/don/crates/oracle/src/damage_test.rs` — scenario generation and the test loop
* `/Users/ember/dev/don/docs/derivation/damage-port.md` — this file

Mirrored to `hbox:~/don-oracle/`, together with `crates/don-sim` (added to that
workspace so the oracle tests the *shipped* implementation rather than a second
transcription of it — a copy would have made the whole exercise circular).

---

## 8. Proposed provenance-ledger entries

### `damage()` — the pipeline, `FUN_00644130`

| field | value |
|---|---|
| source | `riseofnations.exe` VA `0x00644130` (`__thiscall`, six stack dwords, `ret 0x18`) |
| implementation | `crates/don-sim/src/mechanics.rs::damage` / `damage_traced` |
| tier | **B** for the arithmetic chain; **unverified** for steps 10, 11, 27; predicates are inputs, not derived |
| evidence | 7,986,695 trials against retail machine code, 8 seeds, 0 mismatches; distribution in §3; per-step coverage in §3; 31 of 32 step-level mutations caught, §4 |
| harness | `crates/oracle`, `damage` command, i686-unknown-linux-musl on hbox, fabricated world in `damage_env.rs` |
| reachability | not an ISLAND — constructed environment (§2) |
| caveats | `0x00C0AB84` / `0x00C0AEC0` assumed to alias; `out_kind` not compared; step 0 inert |

### `get_attack` / `get_armor` — `0x006469F0` / `0x00647DB0`

| field | value |
|---|---|
| source | vtable slots `+0x120` / `+0x124`; read `UnitType[+0x1E8]` / `[+0x214]` |
| implementation | `crates/don-sim/src/mechanics.rs` |
| tier | **B** for the base path; **unverified** for the upgrade path |
| evidence | called as the real retail functions in all 7,986,695 trials; source-field links pinned at 150,964 and 99,529 mismatches under mutation |

### `entrench_dir_level` — inlined at `0x00644E0E`

| field | value |
|---|---|
| source | `riseofnations.exe` `0x00644E0E`–`0x00644E2D`, inlined (no standalone function) |
| implementation | `crates/don-sim/src/mechanics.rs::entrench_dir_level` |
| tier | **B** |
| evidence | ran in 642,609 trials; boundary mutation pinned at 7 mismatches / 199,629 |
| note | **not** the same classifier as `flank_level`; they disagree at `x = 0` |
