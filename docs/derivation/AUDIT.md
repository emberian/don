# Adversarial audit of the derivation wave

Lane: **audit**. I am the gate. Everything below is either something I re-derived myself
from `ron-bin/riseofnations.exe` with capstone on this Mac, or something I re-ran myself
against retail machine code on hbox. Where I accepted a lane's claim without checking it, I
say so.

Marks: **[measured]** = I did it. **[reported]** = a lane said it and I did not check.

---

## 0. Verdict

**No folklore found.** I hunted specifically for numbers that look like community figures
and for values that trace to a wiki rather than to an address. Every constant I spot-checked
resolved to either a byte in the image or a line in `ron-data/`. The suspiciously round ones
(`55, 100, 100, 540`, `240, 200, 60, 600`, `192`, `493`, `5552`) are all literally the image
bytes; I read them out myself.

**No tier inflation in the prose.** Every Tier-B claim in all six reports carries a sample
count and an input distribution, and every one of them says "testing, not verification" in
so many words. I re-ran **every** Tier-B harness in this wave on hbox and reproduced the
exact trial counts and zero mismatches (§2).

**One substantive error, and it is in a table that a downstream lane would implement from**
(§3.1). Two overstatements (§3.2, §3.3). One unresolved cross-lane contradiction that will
bite whoever implements next (§3.4). Plus a stale mandatory document (§5) and some
arithmetic slips (§6).

`cargo test` at `/Users/ember/dev/don` **PASSES**: 56 tests, 0 failed — but see §4, because
that number is worth less than it looks.

---

## 1. `cargo test` — passes, and what that does not mean

```
cd /Users/ember/dev/don && cargo test
```

```
don_gpu   (lib)      8 passed
don_gpu   flowbench  0
don_gpu   parity     5 passed
don_pe    (lib)      5 passed
don_rules (lib)     11 passed
don_sim   (lib)     27 passed
don_bench            0
doc-tests            0
```

**56 passed, 0 failed, 0 ignored.** [measured] The RED state the combat lane reported
(`error[E0425]: cannot find function active_path in module simd`, `crates/don-sim/src/bin/bench.rs`)
is gone — the simd lane landed `crates/don-sim/src/simd.rs` and the tree is green.

**What green does not cover.** `Cargo.toml` has `exclude = ["crates/oracle"]`. `cargo test`
at the repo root does not compile, let alone run, a single line of the oracle. **Every
Tier-B result in this wave lives in an oracle harness that the repo's test command never
touches.** A green `cargo test` is evidence about the Rust crates only; it is not evidence
for any fidelity claim. That is not a defect — the oracle is i686-only and this Mac is
arm64 — but nobody should cite "cargo test passes" as backing for a tier.

### Vacuity check on the tests that could pass trivially

Two suites can pass with zero evidence. Both actually executed here; both are silent in CI.

| test | skip condition | did it run? |
|---|---|---|
| `don_rules::value::tests::parses_the_whole_shipped_corpus` | returns early if `ron-data/rules.xml` is absent | **yes** — `ron-data/` is present, 49 files [measured] |
| `don-gpu` `tests/parity.rs` × 4 (`gpu_matches_cpu_bit_for_bit`, …) | `return` if `Gpu::new()` fails | **yes** — re-ran with `cargo test -p don-gpu --test parity -- --nocapture`; **no `SKIP` lines printed** [measured] |

Standing hazard, not a finding against this wave: both go green on a machine with no
`ron-data/` and no GPU adapter, and the skip notice goes to stderr where `cargo test` hides
it. Neither is admissible as CI evidence without a `--required`/env-gated failure mode.

`don_sim::mechanics::tests::matches_retail_on_captured_vectors` is the opposite of vacuous —
see §2.6.

---

## 2. Independent reproduction of the Tier-B evidence

I re-ran the harnesses myself on hbox rather than trusting the pasted output. Every count
matched the report exactly.

### 2.1 combat — `oracle combat 500000`

```
[oracle] mapped at 0xf2662000, 315865 relocations applied
  PASS  0x0092cfe0  flank_level(angle_delta)               500017 trials, 0 mismatches
  PASS  0x00581ca0  balance[atk*493 + def] (int16)         247049 trials, 0 mismatches
  balance-table value histogram (top 12 of 807 distinct): [(0, 240956), (2048, 288), ...]
```
**Reproduced. 500,017 and 247,049, 0 mismatches.** [measured] Note the histogram: 240,956
of 243,049 cells read 0 in the static image — which is exactly why the lane refused to
implement the table. That refusal is correct.

### 2.2 rng — `rng difftest 1000000`

```
  PASS  0x00a39cf0  Random::next_float  999950 trials, 0 state mismatches, 0 float mismatches
  PASS  0x00a39d70  Random::in_range    1000012 trials, 0 mismatches
```
**Reproduced. 999,950 and 1,000,012, 0 mismatches.** [measured]

I also re-ran `rng vectors`: the twelve states from seed 0 come back
`0x3c6ef35f, 0x47502932, 0xd1ccf6e9, 0xaaf95334, 0x6252e503, 0x9f2ec686, 0x57fe6c2d,
0xa3d95fa8, 0x81fdbee7, 0x94f0af1a, 0xcbf633b1, 0xbcd1195c` — identical to the report — and
the 200,000-draw histogram of `in_range(0,10)` from seed 1 is
`[20005, 20022, 19980, 19989, 20000, 19994, 20007, 19980, 20011, 20012, 0, …]`, so the
half-open `[lo, hi)` claim is behaviourally confirmed. `in_range(0, 0, 0)` returns 0 with
**state unchanged**, as claimed. [measured]

### 2.3 economy tokenizer — `don-oracle-econ`

```
[econ-oracle] IAT patched: _wtoi -> 0x08054040, wcschr -> 0x080543c0
  corpus: 850 shipped rules.xml value strings x 3 scales, 31 edge cases x 3 scales, 200000 generated
  total 202643 calls, 0 mismatches
```
**Reproduced. 202,643 calls, 0 mismatches.** [measured] The lane's scoping caveat — two CRT
leaves are substituted, so the Tier-B claim covers the slash search, the wrapping multiply,
the truncating divide and the zero-denominator early-out *conditional on* `_wtoi`/`wcschr` —
is stated in the report and is the right way to say it.

### 2.4 pathfinding — `lane-pathfinding difftest 4000000`

```
PASS  0x0046cff0  pf_dist(dx,dy)  4000026 trials, 0 mismatches
```
**Reproduced. 4,000,026 trials, 0 mismatches.** [measured] (The binary defaults to
1,000,000; the report's figure needs the explicit argument. I ran both.)

### 2.5 checksum — `don-oracle-checksum adler`

```
adler32 @0x00a46830: 40000 random cases, 0 mismatches, 0 faults
  null-buffer call returned Ok(1) (disassembly predicts Ok(1))

adler32 @0x00a46830: 500000 random cases, 0 mismatches, 0 faults
  null-buffer call returned Ok(1) (disassembly predicts Ok(1))
```
**Reproduced at both sizes: 40,000 and 500,000, 0 mismatches, 0 faults, NULL→1.** [measured]
(One forked child per case with buffers to 24 KB, so the 500,000 run takes several minutes.)

### 2.6 The captured vectors are genuinely captured

`crates/don-sim/src/mechanics.rs` carries eight hardcoded expectations with a comment saying
they came from `oracle vectors` and a note that an earlier hand-computed `-47` was really
`-247`. This is the exact failure mode the charter names, so I checked it rather than
believing the comment. `oracle vectors` on hbox emits, verbatim:

```
assert_eq!(hash_into_range(0, 0, 0, 0), 0);
assert_eq!(hash_into_range(1, 1, 0, 1), 1);
assert_eq!(hash_into_range(-1, -1, -5, 5), -6);
assert_eq!(hash_into_range(7, -3, -100, 100), -247);
assert_eq!(hash_into_range(123456, 789, 0, 0), 0);
assert_eq!(hash_into_range(5, 5, 10, -10), 30);
assert_eq!(hash_into_range(100, 3, 1, 6), 1);
assert_eq!(hash_into_range(-9, 4, 0, 100), 21);
```

Byte-identical to the test file, `-247` included. **No hand-computed expectation anywhere in
the tree.** [measured]

---

## 3. Findings

### 3.1 OVERSTATED — `combat.md` §8 mis-states two rule representations, and the measured loader refutes it

`combat.md` §8 ("Runtime representation of the rules.xml combat constants") lists:

| rule | RULES offset | combat.md's "implied representation" |
|---|---|---|
| `CAVALRY_FLANK_BONUS` `"40% (of base flank bonus)"` | `+0x50` | **1/256 fixed point** |
| `VEHICLE_FLANK_BONUS` `"33% (of base flank bonus)"` | `+0x54` | **1/256 fixed point** |

That column is an **inference from the divisor at the use site**, printed in a table whose
other rows are measurements. It is wrong. I read the loader:

```
00569dc3  mov   ecx, esi
00569dcd  add   eax, 0x936c
00569dd3  call  0x57fa60            <-- plain _wtoi binder, NO scale pushed
00569dd8  mov   dword ptr [esi + 0x50], eax      ; cavalry_flank_bonus
00569ddb  mov   ecx, esi
00569de5  add   eax, 0x9380
00569deb  call  0x57fa60            <-- plain _wtoi binder
00569df0  mov   dword ptr [esi + 0x54], eax      ; vehicle_flank_bonus
```

versus the scaled binder, which is used at the *neighbouring* offsets and always pushes its
scale:

```
00569e08  push  0x100  ... call 0x57f950 ... mov [esi + 0x58], eax   ; rocky_modifier
00569e3d  push  0x100  ... call 0x57f950 ... mov [esi + 0x60], eax   ; overkill_damage
00569e5a  push  0x100  ... call 0x57f950 ... mov [esi + 0x64], eax   ; entrenchment_modifier
00569e77  push  0x100  ... call 0x57f950 ... mov [esi + 0x68], eax   ; river_modifier
00569e94  push  0x100  ... call 0x57f950 ... mov [esi + 0x6c], eax   ; recapture_city_modifier
```
[measured, capstone, this Mac]

`docs/derivation/rules-constants.json` agrees independently: `cavalry_flank_bonus
parser=wtoi scale=None stored=40`, `vehicle_flank_bonus parser=wtoi scale=None stored=33`.
**Two lanes measured this and contradicted each other; the loader settles it in economy's
favour.**

The consequence is not cosmetic. I confirmed the use site myself:

```
00644b32  mov   eax, dword ptr [0xc061e4]
00644b37  mov   ecx, dword ptr [eax + 0x4c]     ; flank_bonus = 50
00644b3a  test  edx, 0x200000
00644b42  mov   eax, dword ptr [eax + 0x54]     ; vehicle = 33
00644b4f  mov   eax, dword ptr [eax + 0x50]     ; cavalry = 40
00644b52  imul  eax, ecx
00644b56  and   edx, 0xff
00644b5c  lea   ecx, [edx + eax]
00644b5f  sar   ecx, 8                          ; /256, truncating
```

So the per-level cavalry flank percentage the engine actually uses is `(40 * 50) / 256 =
**7**`, and vehicle is `(33 * 50) / 256 = **6**` — against the 20 and 16 the XML prose
("40% of base flank bonus") implies. Either RoN has a real off-by-a-factor here, or one of
the two readings is still incomplete. **Neither lane may implement `1/256` for these two
fields.** The `§8` row must be rewritten as "plain integer, fed through a `/256` divide,
producing a value far below what the XML prose implies — engine quirk, resolve before
implementing".

Every other row in `combat.md` §8 that I checked is correct: `+0x44`, `+0x48`, `+0x4C`,
`+0x5C`, `+0x4C4` are `_wtoi`; `+0x58`, `+0x60`, `+0x64`, `+0x68`, `+0x6C` are scale-256.
All eleven offsets agree exactly with `rules-constants.json`. [measured]

### 3.2 OVERSTATED — pathfinding's headline extrapolates past its own evidence

Headline: *"…so whole-sim bit-exactness is achievable and the blocker is the RNG stream,
**not floating point**."*

The **scoped** claim is solid and I verified it: a clean linear disassembly (no `skipdata`,
so no resync artifacts — my first pass with `skipdata` produced a phantom `movntps` that
does not exist) decodes `FUN_00685990` in **669 instructions** and `FUN_00686300` in **293**,
with **zero `xmm` operands and zero `f*` mnemonics in either**. [measured] Same for the
damage function: `0x00644130..0x00645100` decodes to 1,195 instructions, **zero float**.
[measured]

The **extrapolation** is not supported, and the report's own body says so ("the per-frame
movement integrator was NOT located", "247 indirect call sites", "that it is a BUG is
inference"). Counter-evidence I found in five minutes:

```
0067bad0  mov       eax, dword ptr [eax + 0x2c0]   ; UnitType.MOVES
0067bad6  imul      eax, dword ptr [ecx + 4]       ; * rules.unit_move_speed  (ecx = [0xc061f0])
0067badf  movd      xmm1, eax
0067bae3  cvtdq2ps  xmm1, xmm1
0067bae6  divss     xmm0, xmm1                     ; <-- single-precision divide
0067baea  cvtss2sd  xmm0, xmm0
0067baee  cvttsd2si eax, xmm0
0067baf7  mov       ecx, dword ptr [0xc06184]      ; the SIM RNG stream
0067bb00  call      0xa39d70                       ; Random::in_range
0067bb05  cdq  /  mov ecx, 7  /  idiv ecx
```
[measured]

That is a float divide plus a double round-trip in a movement-speed/timing computation,
immediately before a draw on the simulation RNG stream. It does not touch anything in the
pathfinding body. It does refute "the blocker is not floating point" as a **whole-sim**
statement.

Worth noting: **`0x0067BAD6` is `economy.md`'s own "sanity witness" citation** for
`unit_move_speed`. Two lanes cited this address and neither reported the `divss` three
instructions later. Retitle the pathfinding headline to scope it to the A* driver and cost
function, where it is fully earned.

### 3.3 Tier mislabels in the structured claims (the prose documents are fine)

- **rng**: `install_fake_teb()` is submitted as `tier: B` with evidence "1,500,012 calls with
  0 faults". A TEB fix is oracle plumbing, not a mechanic, and "did not crash" is a smoke
  test, not a differential result. There is nothing to be Tier-B *about*. Relabel
  `engineering`.
- **rng**: the cosmetic-stream attribution (`0xecba54`, `0xe87d3c` → `jukebox.cpp`) is
  submitted as `tier: C`. Tier C means "behaviourally faithful, divergence measured"; no
  behaviour was observed and no divergence measured. `rng.md` itself gets this right —
  "[measured, by compiland attribution]" with an explicit proximity-heuristic caveat. The
  document is honest; the claim's tier field is not.
- **pre-existing, `docs/provenance-ledger.md`**: the "Field offsets — 1,223 constants" entry
  is `tier B`, evidence "agree between instruction-level extraction and Ghidra decompiled C".
  **No retail code was executed.** Tier B is defined in the same file as differential testing
  against the shipped code. And per the charter, decompiled C is a hypothesis, not a second
  derivation — so this is precisely the shape the charter warns about, sitting in the
  charter's own ledger. Relabel `structural`.

### 3.4 UNRESOLVED CONTRADICTION — which global is "RULES"

`combat.md` lists as **refuted**: *"the rules-object pointer used by the damage code is
`0x00C061F0` — refuted … the sim reads them off `0x00C061E4`."*

`economy.md` §3 reads `gather_rate` off `0x00C061F0`.

I disassembled both. **Both are real sim code:**

```
006ce7ae  mov  eax, dword ptr [0xc061f0]     ; Player::TickResource
006ce7b9  mov  ecx, dword ptr [eax + 0x27c]  ; gather_rate = 450
006ce7c1  shl  ecx, 4                        ; PERIOD = 7200

0065094c  mov  eax, dword ptr [0xc061e4]     ; disband_city_rate path
00650951  mov  ecx, dword ptr [eax + 0x380]
```
[measured]

`combat.md`'s refutation is only valid **for offsets `0x40..0x84`** — which is how the body
of the report phrases it, and the body also flags "two distinct rule objects, relationship
not established" as open. The structured `refuted` line drops the qualifier and reads as a
general result. It is not one.

**Nobody should implement "RULES = the object at X" from either report.** Both lanes list
this as open; that is the correct state, and it needs a live-process read to close. Elevating
it here because two reports now assert different globals in their headline-adjacent text and
a third lane reading only one of them will build a mirror.

---

## 4. Folklore sweep — what I checked and found clean

The cardinal sin is a number that traces to the wiki. I went looking for it, biasing toward
round numbers and toward figures the community publishes. All clean:

| value | claimed source | I checked |
|---|---|---|
| `493` balance stride | 364 `<UNIT>` + 129 `<BUILDING>` | `grep -o "<UNIT[ >]" unitrules.xml \| wc -l` = **364**; `<BUILDING` = **129**. And `00581ca3 imul eax, [ebp+8], 0x1ed` — `0x1ed = 493` [measured] |
| pathfinding costs `55,100,100,540` / `240,200,60,600` | `.rdata 0xB69A30` / `0xB69A60` | read the bytes: `(55,100,100,540,544,544,544,544,572)` and `(240,200,60,600,…)` [measured] |
| neighbour tables `dx`/`dy` | `.rdata 0xADCAF0` / `0xADC400` | `(0,-1,0,1,1,1,0,-1,-1)` and `(0,-1,-1,-1,0,1,1,1,0)` — exact [measured] |
| LCG `1664525` / `1013904223` | `0x00a39cf0` | `imul eax, [ecx], 0x19660d` / `add eax, 0x3c6ef35f` [measured] |
| adler `NMAX 5552`, `mod 65521`, NULL→1 | `0x00a46830` | `mov edx, 0x15b0`; DO16 unrolled; `lea eax,[edx+1]` on NULL [measured] |
| `192` world units per tile | `rules.xml` + `FUN_00681DB0` | `<UNIT_MOVE_SPEED value="1/192 tile (granularity …)"/>` in the shipped file [measured] |
| `GATHER_RATE = 450 frames` | `rules.xml` | `<GATHER_RATE value="450 frames"/>`; `shl ecx,4` at `0x006ce7c1` gives 7200 [measured] |
| all eleven damage-path RULES offsets | `combat.md` §8 | every one agrees with `rules-constants.json` and with the loader binder I disassembled [measured] |
| `0x00846450` "is not called by anything" | rng lane | **0** occurrences of the LE dword `0x00846450` in **any** section; **0** `E8`/`E9` rel32 targeting it [measured] |

One consequence of that last row that nobody stated out loud: `hash_into_range` — the **only
mechanic in the provenance ledger** and the only non-placeholder function in `don-sim` — is
derived from a function with **zero references in the image**. `mechanics.rs` is scrupulously
honest about this ("what the engine *uses* it for is not established"), but the ledger entry
still says `reachability: ISLAND (no calls, no off-.text references)` without saying that
this now means *dead code*, and the rng lane's own caveat applies (`0x009e1890`, certainly
live, is also unreferenced — this build retains unreferenced COMDATs). Record the rng lane's
finding on that entry.

---

## 5. `docs/provenance-ledger.md` is stale, and it is mandatory

The charter: *"The provenance ledger is mandatory. Every implemented mechanic carries: the
source, the fidelity tier, and — if tier B — the sample count and input distribution. A
mechanic with no ledger entry is not done."*

No lane updated it. It still lists under **"Not yet derived (do not implement from
folklore)"**:

- Damage pipeline: operation order, rounding, the hardcoded per-mask modifier table — *now
  a 31-step instruction-level derivation, and the "table" is refuted*
- The engine's RNG algorithm and its call sites — *now Tier B, 2,000,000 trials, reproduced*
- Gather rates, cost ramping, attrition timing — *now derived*
- Pathfinding — *now derived; `pf_dist` Tier B, 4,000,026 trials, reproduced*
- The lockstep checksum's field set — *mechanism derived; adler-32 Tier B, reproduced*
- Rule-value tokenizer semantics: denominator limit, rounding, `f32` vs fixed point — *now
  Tier B, 202,643 calls, reproduced; answer is int32, no denominator limit, truncating*

Six new Tier-B rows are owed to this file (`flank_level`, `balance` address arithmetic,
`Random::next_float`, `Random::in_range`, `RString::AsScaled`, `adler32`, `pf_dist` — seven,
in fact). Its two existing entries need the corrections in §3.3 and §4. **It is currently the
least accurate document in the repo**, which is the opposite of its job.

---

## 6. Smaller corrections (none load-bearing)

**`combat.md` §5** — a dense `int16[493][493]` from `0x00C06AFC` spans to **`0x00C7D5CE`**,
not `0x00C7D06E` (`493² × 2 = 0x76AD2`). The anomaly the section flags is real and I
confirmed it: the file image at `0x00C06AFC` holds `0x48`-stride records containing `.bhs`
and `.xml`, and `0x00C07AC0`+ holds `"AI"` and wide `"ai\scr"`. The conclusion — do not
implement the lookup without a live read — is right and should stand. [measured]

**`combat.md` §1** — "1142 instructions". A clean linear decode of
`0x00644130..0x00645100` gives **1,195**. Zero float either way. [measured]

**`combat.md` §1** — the `islands.jsonl` gap boundaries are misquoted. The real neighbours
are `0x64A270`/`0x64C880` and `0x61AAE0`/`0x61BE50`, not `0x64A47B`/`0x64C000` and
`0x61AB47`/`0x61BE50`. **The claim itself holds**: `islands.jsonl` has **46,564** entries
(not the charter's 47,177) and neither `0x64A480` nor `0x61AB50` is present. [measured]

**`combat.md` §8 / open questions** — *"Whether `2/3` parses to 170 or 171 … the
highest-value remaining question for the rules-parser lane"* was **answered in the same wave**
by the economy lane at Tier B, captured from retail: **170**. I reproduced the capture myself
(`256 "2/3 (light infantry in rocks)" -> 170`). Cross-lane staleness, not an error.

**`checksum.md`** — `CheckSum::walk_bytes` at `0x00936ff0` is described as "9 instructions";
it is 17. Everything structural about it checks out: `begin=[ebp+8]`, `end=[ebp+0xc]`,
`len = end-begin`, `[this+0x14] += len`, running sum threaded through `[this+0x10]`, tail
call to `0x00a46830`. [measured] Also confirmed: `DataWalk` vftable `0x00b2bcd8` =
`{0x55e0a6, 0x55e0a6}` and `0x55e0a6` is `jmp dword ptr [0xac544c]` (the purecall thunk);
`CheckSum` vftable `0x00b3f920` = `{0x936ff0, 0x41bfe0}` and `0x41bfe0` is a bare `ret 4`.
The leaders arithmetic checks out exactly: `(0xE71AF0-0xE3A390)/0x6EEC = 8` and
`0xE3A390 + 9×0x6EEC = 0xE789DC`. The desync INI keys exist as **UTF-16LE** at
`0xB179E8`/`0xB17A1C`/`0xB17A3C`/`0xB17A58`/`0xB17A9C`/`0xB17ADC`, and the pointer array at
`0xC06664` holds exactly `{0xB17A58, 0xB17A9C, 0xB17ADC, 0xB17A3C, 0xB179E8}`. [measured]

**`crates/don-rules/src/value.rs` is now known-wrong and still shipped green.** Its grammar
(f64 decimals, `+` sign, fractional part, `%` flag, unit words) has no counterpart in the
retail tokenizer. I disassembled `FUN_00A1D110` myself and it matches economy's C exactly,
including the `den == 0 → return 0` early-out at `0x00A1D163` and the no-slash `ecx = 1`
path at `0x00A1D16B`. The module is explicit that it deliberately does not decide semantics,
so this is not a charter violation — but `parses_the_whole_shipped_corpus` now asserts a
property the engine does not have (that every value yields a numeric; `_wtoi` returns `0` for
a leading non-digit, which is not the same thing). Replace with `as_scaled` per `economy.md`
§7 before it sits green through another wave.

---

## 7. Claims I confirmed at the instruction level

Listed so nobody re-does this work. All [measured] on this Mac with capstone, all
independently of the reporting lane.

| claim | site | result |
|---|---|---|
| damage fn is pure integer | `0x644130..0x645100` | 1,195 insns, **0** xmm / `f*` |
| flank chain: `(vehicle\|cavalry × flank)/256`, then `×lvl`, `+100`, `/100` | `0x644b32..0x644b7b` | exact |
| rescale `(D+5)/10` then `D -= armor` | `0x644b7d`, `0x644b91` | exact — armor **is** mid-chain |
| conditional floor of 1 | `0x644f13..0x644f66` | exact: `cmp edi,1 / jge`, mask agreement `cmove`, `splash==0` `sete`, land-vs-sea skip, `cmovne edi,1` |
| forced-zero branch | `0x644f78..0x644f9a` | exact: type id `0x21D`, attacker domain `2`, `rules[0x558]` |
| `attack` stored ×10 | `0x61b01b` `lea eax,[eax+eax*4]` / `0x61b01e` `add eax,eax` → `[esi+0x1e8]` | exact |
| `attenuate = abs(raw)` | `0x61afeb` `cdq/xor/sub` → `[esi+0x1f0]` | exact |
| `balance(a,d)` | `0x00581ca0` | `imul eax,[ebp+8],0x1ed; add; movsx eax, word [eax*2+0xc06afc]; ret 8` |
| `pf_dist` | `0x0046cff0` | abs both; signed `jle` picks max; unsigned `cmp 0xea60` gates; `min²/(2·max)+max` via `div`, else `(min+2·max)>>1` |
| A* start `h = 60·d/192`, mode-B child `h = 60·d/384` | `0x685c33..0x685c47`, `0x686010..0x686021` | exact — the two `sar` amounts really do differ by one (5 vs 6) |
| mode-A child passes **`-goalY`**, not a delta | `0x685fd9 mov edx,[ebp-0x1c]; 0x685fdc neg edx` | exact; `[ebp-0x1c]` is written only at `0x685afa` and `0x685b5a` and is subtracted as the goal Y at `0x685c0b` and `0x685ff4` |
| `RString::AsScaled` | `0x00a1d110` | `_wtoi` (IAT `0xac54ac`), `wcschr(s,0x2f)` (IAT `0xac5434`), `den==0→0`, no-slash `ecx=1`, `imul ebx,[ebp+8]`, `cdq/idiv` |
| `FUN_00570170` pushes by value | `0x00573c69 push dword ptr [edi+0x27c]` | exact — economy's refutation of "it is the loader" holds |
| `Random::next_float` | `0x00a39cf0` | `imul eax,[ecx],0x19660d; add eax,0x3c6ef35f`; `and 0x7fffff / or 0x3f800000`; `subsd` against the `1.0` double at `0xb695a8` |
| constant-folded `Random(0).next_float()` | `0x00542579`/`0x0054258f` | genuine: `0x3feef35f` literal + the same `cvtps2pd/subsd/cvtpd2ps`, with `0x3c6ef35f` stored as the new state |
| `Random::in_range` | `0x00a39e83..0x00a39ec0` | `lo==hi → lo` with **no** state advance; xor-swap if `lo>hi`; `movzx eax,cx; imul eax,edi; shr eax,0x10; add eax,ebx` |
| `adler32` | `0x00a46830` | fastcall `ecx`=sum split `movzx esi,cx`/`shr ecx,0x10`; `NMAX 0x15b0`; DO16; NULL→1 |
| `DataWalk`/`CheckSum` vftables and `walk_bytes` | `0xb2bcd8`, `0xb3f920`, `0x936ff0` | exact, see §6 |

**One methodology note worth propagating**: capstone with `skipdata=True` started mid-stream
in my first passes and produced instructions that do not exist — including a phantom
`movntps xmmword ptr [ebp-0x20], xmm1` inside `FUN_00685990`, which would have looked like a
refutation of the pathfinding lane's zero-float claim. Always start a linear decode at a
known function entry and let it run; never trust a `skipdata` window opened at an arbitrary
address. I nearly filed a false finding on this.

---

## 8. What I could NOT establish

- **The balance table's runtime contents or extent.** The static image is 99% zeros there
  (240,956 of 243,049 cells). The combat lane's refusal to implement it is the right call and
  I could not improve on it without a live-process read.
- **`checksum.md`'s 183 byte-ranges.** I confirmed the *mechanism* end to end (vftables,
  `walk_bytes`, adler, the 8 call sites' shape) but not the extraction. The lane labels it
  incomplete and says ~60 of 106 functions have bad `this`-taint; I take that at face value
  and did not re-run `extract_datawalk.py`.
- **The call-cone sweeps** (pathfinding's 187/859 functions, rng's 414 call sites in 170
  functions). Both are direct-call closures; both lanes say so explicitly. I did not rebuild
  either graph. The pathfinding cone's 247 indirect call sites remain the honest hole in the
  transcendental sweep, and §3.2 shows float *does* live in adjacent sim code.
- **Whether the cavalry/vehicle flank divide-by-256 is a genuine RoN bug or a still-incomplete
  reading.** §3.1 establishes that the two readings are inconsistent and that the loader
  favours economy's. Which of "the engine is buggy" and "the use site does something else"
  is true needs the oracle with a populated RULES object, i.e. a live read.
- **No Tier A anywhere in this wave.** Nothing is proven over a whole input domain. Seven
  Tier-B results, all reproduced except one, and everything else is structural.
