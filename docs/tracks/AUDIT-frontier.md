# Adversarial audit — frontier wave (headless-client, ron-ai, analytics, web-frontend)

**Lane:** adversarial auditor / gate. **Date:** 2026-08-08.
**Scope:** the four lane reports named in the brief, plus `docs/tracks/netcode-symbols.md`
(a fifth report present on disk and not in the claim set I was handed).
**Method:** I read all five reports, then re-derived what I could from the binary, the
PDB, the shipped data and the corpus **without using the lanes' own scripts** wherever an
independent route existed.

Everything below marked **[measured]** was executed by me on this tree in this session.
**[reported]** means I read it and did not check it.

---

## 0. Verdict

**Pass, with corrections — but one lane must not enter the ledger unedited.**

| lane | verdict | why |
|---|---|---|
| `web-frontend` | **PASS** — cleanest tier discipline in the wave | every number I sampled reproduces from `web/results.json`; the placeholder caveat is in paragraph one; one sentence outruns its data (§5) |
| `analytics` | **PASS, and one finding should be *upgraded*** | all 13 function identities corroborated by the PDB the lane never used; S1 and S6 reproduce exactly; F1 is now settled by the type stream (§4) |
| `headless-client` | **PASS on substance, FAIL on framing** | the headline is its weakest evidence; the strong evidence is real, falsifiable, and buried (§3) |
| `ron-ai` | **HOLD** — do not ledger without the corrections in §6 | did not use `rise.pdb`; two functions misidentified; one crate test is a tautology; a stated cross-check does not reproduce; a cross-lane claim is false on this tree |

`cargo test` at `/Users/ember/dev/don`: **passes.** 118 tests, 9 binaries, 0 failed,
0 ignored. What it does *not* cover is in §7 and it is a longer list than the pass suggests.

---

## 1. What I established independently [measured]

These are my own re-derivations, not confirmations of a lane's summary.

| # | established | how |
|---|---|---|
| V1 | `rise.pdb` really is this binary's PDB: exe CodeView `51D4F219-61C6-4F84-9D5B-C3361B0D291F` age 1 == PDB stream header GUID/age | `llvm-pdbutil dump --summary` + `pefile` on the debug directory — neither is a lane script |
| V2 | Root `cargo test` green: 118 tests, 0 failed, 0 ignored | `cargo test` |
| V3 | don-net full corpus reproduces **exactly**: 1,296,192/1,296,194 packages, 5,055,253 commands, 59 opcodes, 59/61 files at 100%, checksums 265,910/265,931 = 0.999921 | `DON_NET_FULL_CORPUS=1 cargo test -p don-net --release` |
| V4 | **The corpus test bites.** +1 on one opcode's size (`CameraCommand` 10→11) drops the round-trip rate to **0.4685** and fails the test | mutation test in a scratch copy of the crate |
| V5 | **79/79** fixed-size opcodes in `schema/command-wire.json` agree with the independently written table in `re/scripts/rcx_parse.py` | my own comparison script |
| V6 | 63 `.rcx`: **60 gzip, 3 raw** starting `16 42 1a 00` | magic-byte census over `ron-data/replays/` |
| V7 | `0x006D66A0` returns **−35 / −15 / −7 / 0 / +25 / +50** — read at instruction level (`0xFFFFFFDD`, `0xFFFFFFF1`, `0xFFFFFFF9`, fallthrough `esi=0`, `0x19`, `0x32`), not from Ghidra C | capstone |
| V8 | The AI scheduler prologue is exactly as `ron-ai` describes: `200 / *[0xC061C0]`, `who*25 + tick`, `% 30` via the `0x88888889 / sar 4` divide-by-30 idiom, one stage per tick | capstone at `0x006B9620` |
| V9 | `LeaderData` layout confirms **every** player-record offset `ron-ai` claims (§6 table) | `re/scripts/pdb_types.py --struct LeaderData` |
| V10 | **`LeaderDataEncrypt+0xDC` is `ages` and `+0xE8` is `int[4] epoch`** — the age is *not* at `+0xF0`, settling the load-bearing half of analytics F1 from a source that lane never opened | same |
| V11 | `analysis/study.py S1` and `S6` reproduce exactly, including **5:05 vs 4:00 = 66 s** | ran them |
| V12 | `schema/live/gamestate-pid148.png` really reads "Dutch: Ancient Age", **13/25**, **1/1**, green **+100** | I opened the image |
| V13 | Every analytics constant traces to `rules-constants.json` with a binder EA, and the offset arithmetic checks (`0x400`=1024 COMMERCE_CAP, `0x3C4`=964 POP_CAP, `0x264`=612 CITY_GATHER, `0x280`=640 PEASANT_RATE, `0x29C`=668 OIL_RATE, `0x27C`=636 GATHER_RATE) | `rules-constants.json` |
| V14 | The commerce-clamp asymmetry is real: the cap is written unshifted (`0x006CE940`/`0x006CE953`), the income is `<<4` (`0x006CF075`), and they are compared at `0x006CE512` | capstone |
| V15 | Web: native digest `0x1b07da068f50ba66` == the browser digest recorded in `results.json`; wasm is 23,802 bytes with **no import section at all** | ran the digest binary; parsed the wasm sections myself |
| V16 | `riseofnations.exe` imports exactly **9** `CrossplayNetLib` + **2** `CrossplayProxy` symbols, including `send_ready_flag` and `reset_ready_flags` | `pefile` import table |
| V17 | `"Citizens"` (case-sensitive) appears in **neither** `typenames.xml` nor `unitrules.xml`; `economic.bhs` uses it at lines 475/510/568/674/762 and the correct `"Citizen"` at 297/430 | grep |

**Tier of this audit:** none of the above raises any lane above the tier it claimed.
V1/V5/V9/V10/V16 are structural (PDB/PE facts — names, types, sizes; no semantics).
V3/V4/V6/V7/V8/V11/V12/V13/V14/V17 are [measured] observations of files, instructions or
test behaviour. **Nothing in this wave was executed against retail machine code.** The
oracle did not run; no claim here is Tier B.

---

## 2. The single most useful thing this audit did

**V4, the mutation test.** The brief warns that "parses exactly to EOF with zero residue"
is nearly worthless standalone, and `don-net`'s pipeline has exactly that shape: the
framing is *found* by a longest-chain-tiling-to-EOF search, the XOR key is *guessed* by
ciphertext word frequency, and the pad seed is a 256-way search — each accepted on the
criterion "the stream decodes". Given a successful parse, a byte-exact re-encode is close
to tautological, because the XOR is an involution.

So I perturbed the one input that is *externally* fixed — the PDB-derived size table —
and re-ran:

```
CameraCommand size 10 -> 11
package round-trip rate 0.468516 below floor 0.9999   -> FAILED
```

A single byte of drift in one of 82 sizes halves the corpus. The test is not vacuous; the
size table is genuinely load-bearing and the corpus genuinely constrains it. That result
is what the headless-client lane's headline *should* have been.

Reproduce:
```sh
cp -r /Users/ember/dev/don/crates/don-net /tmp/mut && cd /tmp/mut
# make Cargo.toml standalone (edition/license are workspace-inherited), symlink ron-data
# next to the crate's grandparent, bump one entry of COMMAND_SIZES, then:
cargo test --release --test roundtrip
```

---

## 3. `headless-client` — substance solid, framing inverted

### What holds [measured, by me]

V1, V3, V4, V5, V6, V16 all reproduce. The lane's methodology cross-check (18 independent
derivations, 18 exact PDB symbol starts) is the kind of evidence that could have failed in
18 ways, and V5 is a second one of the same shape that I recomputed myself.

I also confirmed a detail the report understates: the pad seed is not searched freely, it
is constructed as `((key & 0xFF) << 8) | lo` (`tests/roundtrip.rs:214`), so the XOR key and
the pad generator must be **mutually consistent through a single global `G`**. Every large
file in my run pinned exactly **1** of 256 candidate seeds. That is a could-have-failed
check and it is not called out.

### Finding H-1 — the headline is the weakest evidence in the report

"Round-trips 1,296,192 of 1,296,194 packages byte-for-byte" is a coverage statistic, not a
falsifier. The falsifiers are: exact tiling under externally-fixed PDB sizes (V4); 79/79
agreement with an independent parser (V5); the key/seed consistency above; cross-player
checksum agreement at 0.999921, which no wrong decode produces; and a 2014-era build
decoding with the 2024 table. **All five are in the report** (§4.3, §5.3, §5.4) — they are
just not what §1 leads with. Demote the round-trip count to coverage and lead with V4/V5.

### Finding H-2 — an unreported residual

Small files admit up to **11** pad seeds (my run: 11 on three 2014 files, 2–3 on several
others). The pad model is underdetermined there. Harmless, but it belongs in the measured-
divergence statement rather than being invisible behind "ALL".

### Finding H-3 — `README-LLM.md` is now stale in two load-bearing places

Both corrections are the lane's, and I verified both:

* "**Established ground truth**: the `.rcx` command stream IS the engine network protocol" —
  true of the *payload*, false of the *framing* (file record 18 bytes with `valid`/`group`;
  wire `NetMsg_CommandPackageData` 8 bytes without them).
* "**Replays**: `.rcx` is a plain gzip stream from offset 0" — **false for 3 of 63
  specimens** [V6, measured by me].

These are in the file agents are told to read first. Fix them.

### Finding H-4 — the "structural" tier is undefined

Three reports now use a tier called `structural` that does not exist in `docs/CHARTER.md`.
It is *defensible* (README-LLM says the PDB "raises no fidelity tier", and the lane says so
explicitly), but an undefined tier in three documents is how a tier gets inflated later.
Either define it in the charter or fold it into "[measured], not tiered".

### Not audited

The PlayFab/Party transport story, the join sequence, and the feasibility analysis in §6
rest on symbol reading I did not independently repeat beyond the import table (V16). The
lane is explicit that the join sequence is *inferred from names* and that no live capture
was attempted — that is the correct label and I have no quarrel with it. The lane also
states plainly that decoding a recorded stream is not speaking a live protocol, and that
`don-net` reproduces the **reader's** model with the writer side untraced. **The blur the
brief warned about did not happen.**

---

## 4. `analytics` — the trap did not fire, and one finding should be upgraded

The brief's worry was "beautiful numbers from invented assumptions". I checked for it
specifically and did not find it.

* `analysis/derive.py` reads only `rules-constants.json` and `schema/live/*-attributes.txt`,
  and its header ranks its sources by authority. `analysis/econ.py` pulls constants out of
  `derived.json` (`R["peasant_rate"]`) rather than hard-coding them. Every quantity in the
  §2.6 table traces to a rules constant with a binder EA [V13].
* Assumptions are in a named `Assumptions` block, are listed in §4 with a stated *reason*
  for each default, and are swept in §5.2 against a **self-measured** search-noise floor
  (~30 s at beam widths 100/250/500/700). The report then says out loud that most of its
  own sensitivity deltas do **not** clear that floor. That is unusually good practice.
* The one place the lane could have smuggled an assumption — the commerce clamp scale — is
  labelled as an assumption, has its rejected alternative (H2) falsified against a shipped
  replay with a floor argument that uses *no cost model at all*, and is named as the single
  largest lever in the sensitivity sweep. The lane also records that its surviving
  hypothesis is *uncomfortable* (63.4/30 s against a cap of 70). I confirmed the underlying
  asymmetry at instruction level [V14].
* **Every one of the 13 addresses this lane cites is corroborated by the PDB**, which the
  lane did not use: `Leader::calc_gather`, `City::calc_gather`, `LeaderData::calc_city_resources`,
  `BuildTypeData::calc_gather`, `TypeData::get_cost`, **`LeaderData::get_city_limit`**,
  `CityData::enhancer_amount`, `CityData::get_taxes`, `CityData::get_literacy`,
  `Leader::calc_resource_caps`, `Leader::gather`, `LeaderData::calc_rare`, `Leader::do_gather`.
  The semantic guesses match the engine's own names, including the hard one (`get_city_limit`).

### Finding A-1 (positive) — F1 should be **upgraded**, not just accepted

The lane rested F1 on structure plus one HUD screenshot and left open "is `econ+0xF0`
Commerce or Science". The type stream settles the load-bearing half outright [V10]:

```
struct LeaderDataEncrypt  // sizeof 248
    +0xdc  int      ages
    +0xe0  int      epochs
    +0xe4  int      discovered
    +0xe8  int[4]   epoch      <-- econ+0xE8/0xEC/0xF0/0xF4, a named four-element array
```

`econ+0xF0` **cannot** be the age; the age is `+0xDC`. The correction to
`docs/derivation/economy.md` §3.3 no longer rests on a screenshot. The Commerce-vs-Science
residual stays open (the array's elements are unnamed), but with `epoch[0]`→POP_CAP and
`epoch[1]`→city limit both pinned, `epoch[2]` under the standard library ordering is
Commerce.

The same struct independently corroborates three more of the lane's behavioural readings:
`+0x18 leftover` (the lane's accumulator), `+0x30 int[7] resource_cap` (the lane's cap),
and `+0x6EB8 → LeaderDataEncrypt*` (the lane's "econ is a pointer" correction).

### Finding A-2 — name the block correctly, or it collides

`LeaderData+0x450` is **already** `int[6] econ`. The block the lane calls "econ" is
`LeaderData+0x6EB8 → LeaderDataEncrypt*`. Ledger it as `LeaderDataEncrypt`, or a future
lane will read "econ+0x30" and go to the wrong place. (`ron-ai` calls `player+0x450` "a
per-resource flag word"; it is this `econ`.)

### Finding A-3 — three field labels disagree with the engine's names

`LeaderDataEncrypt+0x64` is named `resources` (the lane calls it gross income), `+0x94` is
`income` (`ron-ai` calls it the displayed gather rate), `+0xAC` is `rate`. The lane's
*behavioural* readings check out — `+0x00 bucket` is the stockpile (the `0x104BE ^ 0x8221
= 99999` literal proves it), `+0x18` is the accumulator, `+0x30` is the cap — so this is a
naming reconciliation, not a refutation. But three semantic labels sitting on fields whose
engine names say something else is exactly how folklore enters a ledger. Reconcile before
ledgering.

### Finding A-4 — the tool's own label contradicts the report's headline

`analysis/study.py S6` prints:

> `the shipped order is 66 s slower to the Classical Age`

It is not. The search reaches the Classical Age at 3:33 (92 s earlier); **66 s is the
target-state gap** (5:05 vs 4:00). The report's §7 table is careful and correct; the tool's
print label is wrong, and the tool is what the next lane will re-run. Fix the string.

### Finding A-5 — asymmetric caution on one live artefact

The lane declines to identify which HUD widget the green "+100" belongs to (correctly), but
treats "1/1" as certainly cities/city-limit without the same caution. Both are widget
identifications of equal strength. I read the PNG myself and it says exactly what the lane
says [V12]; the claim now survives on V10 regardless, but note the asymmetry.

---

## 5. `web-frontend` — measured what it claims; one sentence outruns the data

The brief's worry was "looks fine at 100 entities, collapses at 10,000, and doesn't say
which". That did not happen, and the report is the wave's best on tier discipline:

* Paragraph one states that **no RoN mechanic is implemented**, that everything simulated
  is `don-sim`'s **PLACEHOLDER** systems, and that A/B/C tiers are *not engaged*. I checked
  `crates/don-sim/src/`: `PLACEHOLDER` markers are on the integrator, the map extent and the
  RNG. The disclosure is accurate.
* Every number I sampled reproduces from `web/results.json`: the inline-rAF sweep to 4,096
  worlds / 262,144 instances at 120.4 fps; the `sim_rate` sweep 164.7 → 1028.5 M unit-steps/s
  over 1→12 shards with per-row load averages; the draw-only units-vs-aggregate table.
  `pageErrors` is empty.
* The cross-target determinism check is real and I reproduced it [V15]: native
  `0x1b07da068f50ba66` == the browser digest in `results.json`.
* It refuses to state a wasm-vs-native ratio because the native reference varies 4×, and it
  records load average 149→77 with every row. It reports what it did **not** measure
  (WebGL2 GPU time, >4,096 worlds on the `sab` path).
* One suspicion of mine was **wrong**: I thought the 3.049 → 0.943 ms render-thread CPU
  comparison mixed a rAF run with an uncapped run. It does not — both come from the uncapped
  suites (`worlds_inline`: 1.837+0.315+0.836+0.061 = 3.049). Fair comparison.

### Finding W-1 — the only unmeasured claim I found

§4.2: *"The display cap holds at every size measured, up to 4,096 worlds and 262,144 units,
**on both paths**."*

`results.json` has `units_zerocopy_raf` and `worlds_inline_raf` (kind `raf`), but there is
**no `worlds_sab_raf` suite** — the `sab` path was measured only `uncapped`. The 120.4 fps
column in that table is the `inline` path. The `sab` path holding refresh rate is not
measured. Either measure it or narrow the sentence to the `inline` path. (The claim
*is* plausible — `sab` uncapped hits 299 fps at the top row — but plausible is not measured.)

### Finding W-2 — the orchestrator's summary drops the caveat the report leads with

The claim JSON headline reads "simulating and drawing 4,096 worlds / 262,144 units" with no
mention that the simulation is placeholder. The *report* is scrupulous about this. Do not
let the summary travel without the caveat.

### Operational

`node web/serve.mjs` is **still running** (pid 16748). The lane flagged it; it is still up.

---

## 6. `ron-ai` — HOLD. Corrections required before ledgering

This lane did good work and several of its headline claims survive my independent
re-derivation at instruction level [V7, V8] and against the PDB type stream [V9]. The
problem is not the reasoning; it is that the lane worked without `rise.pdb`, which was on
disk (13:28) and documented in `README-LLM.md` under *"read this before doing any RE"*,
with `tools/pdb/lookup.py` sitting next to it. **This is a concurrency artefact, not
dishonesty** — the lane report is timestamped 14:21 and other lanes were fetching the PDB
in parallel. But the corrections are free and two of them change claims.

### R-1 — the "single biggest remaining unknown" is largely already named

The report calls the ten compiled production stages "unnamed" and "the largest gap for
behaviour cloning". `rise.pdb` names all of them:

| lane's `FUN_` | engine name (PDB) |
|---|---|
| `FUN_006C1960` | `Leader::production_ai` |
| `FUN_006B9620` | `Leader::plan_strategy` |
| `FUN_006C83E0` | `Leader::production_ai_setup` |
| **`FUN_006C9DB0`** | **`MakeList::clear`** — *not a strategy stage at all* |
| `FUN_006C7A60` | `Leader::found_cities` |
| `FUN_006C6BA0` | `Leader::research_techs` |
| `FUN_006C6430` | `Leader::upgrade_units` |
| `FUN_006C40A0` | `Leader::create_units` |
| `FUN_006C1BE0` | `Leader::create_buildings` |
| `FUN_006C8AF0` | `Leader::make_stuff` |
| `FUN_006D66A0` | `LeaderData::get_gather_handicap` |
| `FUN_006CE450` | `Leader::do_gather` |
| `FUN_006EC000` | `LeaderData::get_diff` |
| `FUN_009C7570` | `ScenarioFuncSet::init_funcs` |
| `FUN_009D4DA0` / `FUN_009D4F20` | `ScriptFuncSet::add_new_func` / `ScriptFunc::add_param` |

Listing `MakeList::clear` as one of the ten compiled AI stages is a misidentification.
Separately, `Leader::found_cities` being a *compiled* stage sharpens the lane's §5.2 open
item: the engine founds cities in C++, so the unresolved `trigger`/`city_placement`
mechanism may matter considerably less than the report fears. That is a lead, free.

### R-2 — two functions misidentified, and two claims fall with them

* **`FUN_006508C0` is `ObjectData::train_time(int) const`, not a cost function.** The
  report says the handicap applies to "unit cost in `FUN_006508C0`" as
  `cost = ((200 − h) × cost) / 200`. It is **train time**. (The `analytics` lane read the
  same function correctly.) Cross-lane conflict; `analytics` is right.
* **The "unit-level AI is staggered per unit at 32 ticks" claim is sourced to the wrong
  function.** `re/decomp-all/0064f3b0.c` is `Object::do_launch`; `0x00652020` is
  `Object::take_damage`. The gate `(Game[0x550] + field_0x0A) & 0x8000001F == 0` is real
  and the difficulty>1 branches at `0x00652585`/`0x00652724` are real, but they live in a
  launch routine and a damage handler. Calling them "unit-level AI re-evaluation" and
  "tactical behaviours Moderate and above unlock" is an unsupported label. The report does
  hedge ("incidental, not quantified, not confirmed live") — but the claim JSON restates it
  as a confirmed Tier-C finding. **Drop or re-derive.**

### R-3 — one crate test is a tautology; the rest are change-detectors

`crates/don-ai/src/abi.rs`:

```rust
#[test]
fn live_script_name_length_matches() {
    assert_eq!("economic".len(), 8);
}
```

This asserts the length of a Rust string literal. It touches neither the crate's code nor
the live capture it is named after. **This is precisely the vacuous-green pattern the
charter forbids** and it is dressed as live confirmation. Delete or replace.

`difficulty_income_bonus_matches_fun_006d66a0` asserts the crate's transcribed table against
itself; the *name* claims agreement with a binary function the test never reads.
`only_one_of_twelve_stages_is_scripted` tests the crate's own enum. Both are legitimate
change-detectors and illegitimate as evidence — yet the claim JSON cites "Unit test in
`crates/don-ai/src/abi.rs`" as **evidence** for both the scheduler finding and the
infinite-resources finding. A unit test over your own transcription is not evidence for the
transcription. (`infinite_resources_constant_decodes_to_99999` *does* carry real content:
`0x104BE ^ 0x8221 = 99999` ties a code literal to a `rules.xml` category, three
independently found things agreeing. Keep that one and say why it is different.)

Also: the header says "13 tests green", §4 says "11 tests". It is 13.

### R-4 — the 429/430 cross-check does not reproduce

The report claims: *"For the 430 names that appear exactly once in each, the binary-derived
table and the XML agree on arity and on every parameter name in order, 429/430. The one
disagreement is `file_write_text`."*

By the obvious method — names appearing exactly once in `crates/don-ai/data/script-functions.json`
and exactly once as a `<FUNC name=…>` in `ron-data/scriptfunctions.xml`, comparing arity and
every `<PARAM name=…>` in order — I get:

```
names appearing exactly once in each: 574
agree on arity AND every param name in order: 572 of 574
disagreements: file_write_text  (str_value vs any_value)
               file_write_attrib (str_value vs any_value)
```

**574, not 430; two misses, not one.** The *substance* is strongly corroborated — 99.65%
agreement between a binary-lifted table and an independent designer artefact is a real
could-have-failed check and it passed. But the stated numbers are not reproducible from the
shipped artefacts by the natural method, so the report must state its filtering criterion
alongside the number, or restate the number. (The extra miss, `file_write_attrib`, is the
same `str_value`/`any_value` disagreement — which makes the pattern *more* interesting, not
less: the XML widens the type for both file-write functions.)

### R-5 — a cross-lane claim that is false on this tree

§6 and the claim JSON: *"`crates/don-net`'s two corpus tests fail on this tree."*
**They pass.** Both the default subset and the full 63-file corpus pass on this tree [V2, V3].
Stale concurrent-lane observation. It must not travel into the ledger.

### R-6 — invented class names presented as engine names

The report tabulates `Player::ProductionAiTick`, `Player::UpdateAI`,
`Player::GetDifficultyBonusPercent`, `Player::GetEffectiveDifficulty` in a
`function | address | behaviour` table with no marker that these are the lane's own coinage.
The engine's class is **`Leader` / `LeaderData`**, not `Player`. The names are semantically
close — which is a genuine credit to the method and worth saying — but a reader will take
them for engine names. `analytics` has the same issue (`Player::TickResource` is
`Leader::do_gather`; `Player::UpdateCommerceCaps` is `Leader::calc_resource_caps`).
**Convention needed: invented names get a marker, engine names do not.**

### What survives untouched, and is strong

* **V7:** the −35/−15/−7/0/+25/+50 table read at instruction level, not from Ghidra C. This
  *raises* the lane's confidence — but only in the transcription. It is still Tier C about
  behaviour, the live game was Tough (0%), and the lane says so. Correct labelling.
* **V8:** the scheduler, confirmed instruction by instruction including the divide-by-30 idiom.
* **V9:** every player-record offset confirmed by the PDB — `+0x08 who`, `+0x50 multi_diff`,
  `+0x3F8 city_num`, `+0x788 production_step`, `+0x78C prod_script_run`, `+0x790 script_step`,
  `+0x7E4 pop_cap`, `+0x6DD4 Personality`, `+0x6EA4 String prod_script`,
  `+0x6EB8 LeaderDataEncrypt*`. Ten for ten. The live-memory work was right.
  Note `+0x6EB8`'s type name is literally `LeaderDataEncrypt` — the obfuscation story is the
  engine's own design, not an inference.
* **V17:** the `"Citizens"` shipped bug is real (case-sensitively absent from both data files).
* The `BLOCK_ON_THIS = 1 / SCRIPT_DONE = 3` cross-check — a script constant and an engine
  comparison found by independent routes — is a genuine could-have-failed check.

---

## 7. `cargo test` — what a green run does **not** cover

`cargo test` at `/Users/ember/dev/don` **passes** [V2]: 118 tests across
`don-ai` (13), `don-net` (8 lib + 3 integration), `don-gpu` (5 + 5 parity), `don-pe` (8),
`don-rules` (18), `don-sim` (55). 0 failed, 0 ignored.

It does not cover:

1. **Any fidelity claim.** `crates/oracle` is excluded from the workspace (32-bit only, runs
   on hbox), so a green run **never executes retail machine code**. Nothing in this wave is
   Tier B and `cargo test` cannot make anything Tier B.
2. **The don-net headline numbers.** The default run sweeps a 12-file subset in 2.6 s. The
   1,296,192-package figure needs `DON_NET_FULL_CORPUS=1` (I ran it — it reproduces).
3. **Anything in `web/`.** `web/wasm` is its own workspace by design; the browser bench, the
   wasm build and the digest cross-check are un-regressed by CI. If `don-sim` changes, the
   web digest silently stops matching until someone re-runs `bench.mjs`.
4. **Anything in `analysis/`.** No Python is tested. The 66-second result is reproducible by
   hand (I did it) and regressed by nothing.
5. **Every Python tool in the repo.** `pdb_read.py`, `pdb_symbols.py`, `pdb_types.py`,
   `dump_script_funcs.py`, `rcx_parse.py`, `tools/pdb/*` — no tests.
6. **`crates/donscan`** — also excluded (Windows-only).
7. **The binary, the PDB, and `ron-data`**, except through don-net's corpus test — which
   *skips loudly* when the corpus is absent (verified in the source: `SKIPPED — NOT A PASS`).
   That skip discipline is correct and worth keeping.
8. **`don-ai`'s 13 tests are assertions about `don-ai`'s own transcribed constants** (§6, R-3),
   not about the game.

---

## 8. Cross-lane conflicts, resolved

| conflict | resolution |
|---|---|
| `ron-ai`: "don-net's corpus tests fail" vs reality | **They pass** [V2, V3]. `ron-ai` is stale. |
| `ron-ai` "FUN_006508C0 is a cost function" vs `analytics` "train time" | **`analytics` is right**: PDB says `ObjectData::train_time(int) const`. |
| `ron-ai` "player+0x450 is a per-resource flag word" vs PDB | PDB: `LeaderData+0x450 = int[6] econ`. |
| `analytics` "COMMERCE_CAP is age-indexed" (economy.md) vs F1 | **F1 wins, decisively** [V10]: `ages` is at `+0xDC`, `+0xE8` is `int[4] epoch`. |
| `headless-client` ".rcx not always gzip" vs `README-LLM` | **`headless-client` wins** [V6]: 60 gzip, 3 raw of 63. |
| `headless-client` "framing is not the wire format" vs `README-LLM` established fact | **`headless-client` wins**: 18-byte file record vs 8-byte `NetMsg_CommandPackageData`. |
| two lanes both built PDB tooling (`schema/rise-symbols.tsv` vs `schema/symbols.json`/`rise-procs.tsv`) | Duplicated effort, no contradiction. Pick one as canonical before a third lane picks the other. |

---

## 9. Actions for the orchestrator, in priority order

1. **Re-audit every lane's function identities against `rise.pdb` / `schema/rise-procs.tsv`
   before anything enters `docs/provenance-ledger.md`.** `analytics` passes clean (13/13);
   `ron-ai` needs §6 applied. Make this a gate step, not a lane's discretion — one
   `python3 tools/pdb/lookup.py <va>` per cited address.
2. **Fix the two stale bullets in `README-LLM.md`** (§3, H-3). It is the file every agent
   reads first and it currently teaches two things I measured to be wrong.
3. **`ron-ai`: apply R-1…R-6.** Delete the tautological test, rename the transcription
   tests, drop or re-derive the unit-AI claim, correct `FUN_006508C0`, correct the
   `MakeList::clear` stage, restate the XML cross-check with its method, and remove the
   false don-net status line.
4. **Upgrade `analytics` F1 with `ages` / `epoch[4]`** (§4, A-1) and rename the block to
   `LeaderDataEncrypt` (A-2) before ledgering.
5. **`headless-client`: re-lead §1 on V4/V5**, add the 11-seed residual (H-2).
6. **`web-frontend`: measure `worlds_sab` under rAF or narrow the sentence** (W-1).
7. **Define `structural` in the charter, or stop using it** (H-4).
8. **Adopt a marker convention for invented function names** (R-6). Three reports currently
   present coinages in the same typography as engine names.
9. Kill or adopt `node web/serve.mjs` (pid 16748), still running.

---

## 10. What I could **not** establish

* **Nothing was run against retail.** The oracle did not run in this audit; hbox was not
  touched. No claim in this wave is Tier B, and I did not raise any.
* **No live-process verification.** I did not read the VM's memory. `ron-ai`'s pid-148 dump
  and `analytics`' `derived.json` live tables (pid 14644) are taken as [reported] by me,
  with the single exception of the HUD screenshot, which I opened myself [V12]. The live
  reads are the least-audited evidence in this wave and they carry real weight in two lanes.
* **The PlayFab/Party transport story** beyond the PE import table [V16]. I did not
  independently read `CrossplayNetLib.pdb` or `PartyWin.pdb`.
* **`don-ai`'s BHS transcription fidelity.** I did not diff `economic.rs` / `library.rs`
  against `economic.bhs` / `aibestbuildlibrary.bhs` line by line. I spot-checked one shipped
  bug (V17). The transcription of ~2,400 lines is unaudited.
* **The analytics beam search's optimality.** I reproduced S1 and S6 exactly, so the result
  is deterministic and the *comparison* is like-for-like. I did not re-implement the model,
  and "best found" remains heuristic — as the lane says.
* **Whether `epoch[2]` is Commerce or Science.** V10 kills the age reading; it does not name
  the column. Still open, as `analytics` §8 says.
* **The 21 checksum divergences' cause.** I reproduced the counts [V3] and agree the channel
  asymmetry (`rules`/`walls`/`items`/`scenario_data`/`script_run_time` never differ) is
  one-sided in a way a decode artefact could not be. The *mechanism* is untouched.
* **`docs/tracks/netcode-symbols.md`** — read, not audited. It was not in my claim set. It
  looks disciplined ([measured]/[inferred]/[reported] used correctly; `xasl` = XAudio2
  settled by compiland list rather than by name) and it explains a failure I hit myself
  (`llvm-pdbutil pretty` needs DIA and does not work on macOS — my own attempt produced an
  empty file). It deserves a pass of its own.

---

## Reproduction

```sh
cd /Users/ember/dev/don

# V1 — the PDB is this binary's
/opt/homebrew/opt/llvm/bin/llvm-pdbutil dump --summary ron-bin/sbl/rise.pdb
cd ron-bin && uv run --quiet --with pefile python -c "
import pefile,struct
pe=pefile.PE('riseofnations.exe'); pe.parse_data_directories()
for d in pe.DIRECTORY_ENTRY_DEBUG:
    b=pe.get_data(d.struct.AddressOfRawData,d.struct.SizeOfData)
    if b[:4]==b'RSDS': print(b[4:20].hex(), struct.unpack('<I',b[20:24])[0])"

# V2 / V3 — the test suites
cargo test
DON_NET_FULL_CORPUS=1 cargo test -p don-net --release -- --nocapture

# V4 — the mutation test (scratch copy; never mutate the tree)
#   bump one entry of COMMAND_SIZES in a copy of crates/don-net, then run the roundtrip test

# V6 — the gzip census
python3 -c "
import os
for r,_,fs in os.walk('ron-data/replays'):
  for f in fs:
    if f.lower().endswith('.rcx'):
      h=open(os.path.join(r,f),'rb').read(2)
      print(('GZIP' if h==b'\x1f\x8b' else 'RAW '), f)"

# V7 / V8 / V14 — instruction-level reads
cd ron-bin && uv run --quiet --with capstone --with pefile python - <<'PY'
import pefile
from capstone import *
pe=pefile.PE("riseofnations.exe")
t=[s for s in pe.sections if s.Name.rstrip(b"\0")==b".text"][0]
base=pe.OPTIONAL_HEADER.ImageBase+t.VirtualAddress; d=t.get_data()
for a,b in [(0x6d66a0,0x6d673a),(0x6b9620,0x6b96a5),(0x6ce900,0x6ce960)]:
    print("==",hex(a))
    for i in Cs(CS_ARCH_X86,CS_MODE_32).disasm(d[a-base:b-base],a):
        print(" %08x %-8s %s"%(i.address,i.mnemonic,i.op_str))
PY

# V9 / V10 — the type stream
python3 re/scripts/pdb_types.py ron-bin/sbl/rise.pdb --struct LeaderData LeaderDataEncrypt
python3 tools/pdb/lookup.py 006c1960 006b9620 006c9db0 006508c0 0064f3b0 00652020

# V11 — the analytics results
python3 analysis/study.py S1
python3 analysis/study.py S6

# V15 — the web digest
cd web/wasm && cargo run --release --bin digest -- digest 4 64 64 1000 0xC0FFEE
```

---

*Files written by this lane: this file only. No Rust, no data, no other lane's artefacts
were modified; the mutation test ran on a copy under the session scratchpad and was deleted.*
