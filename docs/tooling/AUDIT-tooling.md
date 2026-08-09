# AUDIT — tooling wave (native-tools, hook-dll, live-tables, sim-economy)

**Lane:** adversarial auditor / gate. **Date:** 2026-08-08. **Machine:** arm64 Mac only —
no oracle run, no live process touched, no VM contact.

Everything below marked **[measured]** I re-derived here from the shipped binary, the retained
capture artifacts, or by running the code. Everything marked **[reported]** is a lane's number
I did **not** re-run (chiefly the hbox oracle counts).

---

## Verdict

**The wave passes, with four corrections that must land before anything is built on top,
and a set of tier-vocabulary and reproducibility repairs.**

I could not find folklore. Every implemented constant I traced ends at an address or a live
read, and two of the four lanes are noticeably better-disciplined about tier language than the
project average. The strongest evidence of that: **two lanes captured the same two objects
with two different instruments and agree bit-for-bit** (§2.1, §2.2). That is the kind of
cross-check that actually rules out a shared reconstruction error, and it happened by
accident rather than by design — worth designing for next time.

The four things that must land:

| # | what | lane | severity |
|---|---|---|---|
| **A1** | `scholar_rate` (×256) and `caravan_attack_bonus` (×10) are **wrong in `don-rules`'s shipped table** — the live engine holds different values | sim-economy / don-rules | **high — wrong values shipped in Rust** |
| **A2** | `damage-hook.md`'s type-id space is mis-modelled; §4.1's "unit range / building range" boundary is mislabelled and §4.6's flagged anomaly is a **false alarm** | hook-dll | **high — would open a wasted lane** |
| **A3** | `native-scanner.md`: "fast enough to run inside a per-frame loop" is false by 7× | native-tools | medium |
| **A4** | `native-scanner.md`'s headline latency/throughput is contradicted by its own retained artifact | native-tools | medium |

`cargo test` at `/Users/ember/dev/don`: **PASSES. 115 passed, 0 failed, 0 ignored, exit 0.**
[measured] (`cargo test --workspace`, log at
`<local-recovery-scratchpad>/aud-cargotest.log`.)
The two `don-sim` attrition tests `native-scanner.md` reports red at 12:59 are **green**; that
was a transient mid-write by the concurrent economy lane, exactly as the lane guessed. Its
paragraph is now stale and should be deleted rather than left as a standing warning.

---

## 1. Findings that change a claim

### A1 — `don-rules` ships two constants the running game does not hold [measured]

The ledger and the sim-economy lane both cite "**828 of 834** constants confirmed against live
process memory". I reproduced that number **exactly** — 834 entries carry a `stored` value, 828
match the live `RULES` block, 6 do not. Neither the ledger, nor `docs/derivation/sim-economy.md`,
nor `crates/don-rules/src/rules.rs` says **which** 6, and all six are a systematic scale error,
not noise:

| constant | offset | `rules-constants.json` `stored` | live `RULES` | ratio |
|---|---|---:|---:|---|
| `scholar_rate[0..4]` | `0x284`–`0x294` | 5, 7, 10, 15, 20 | 1280, 1792, 2560, 3840, 5120 | **×256** |
| `caravan_attack_bonus` | `0xCD4` | 2 | 20 | **×10** |

×256 is the 8.8 fixed point the neighbouring slots already use — `SHIPPED` holds
`450, 2560, 5, 7, 10, 15, 20, 0, 8960`, i.e. `peasant_rate = 2560` (10.0 in 8.8) and
`oil_rate = 8960` (35.0) are scaled correctly and `scholar_rate`, sitting between them and
meaning the same kind of thing, is not. ×10 is the documented attack-stored-×10 convention.
Both are recorded with `parser: "wtoi"` (scale 1) where the binder plainly applies a scale.

This is live in Rust today: `crates/don-rules/src/rules.rs:991-995` and `:1641`, and therefore
`Rules::scholar_rate()` and `Rules::caravan_attack_bonus()` and `SHIPPED[161..166]`,
`SHIPPED[821]`. The sim-economy lane names `SCHOLAR_RATE` as an input to the undermined
worker→income composition — so this would have been consumed by the next economy lane at 1/256
of its real magnitude.

Reproduce:

```sh
cd /Users/ember/dev/don && uv run --quiet python - <<'EOF'
import json, base64, struct, re
rc = json.load(open('docs/derivation/rules-constants.json'))
blk = base64.b64decode(re.search(r'BLK=(\S+)', open('schema/live/rules-block-pid14644.txt').read()).group(1))
for c in rc:
    for e in c['entries']:
        if e.get('stored') is None: continue
        live = struct.unpack_from('<i', blk, e['offset'])[0]
        if live != e['stored']:
            print(c['name'], e['index'], hex(e['offset']), c['parser'], 'stored', e['stored'], 'live', live)
EOF
```

**Required:** fix the two scales in `rules-constants.json`, regenerate `rules.rs` via
`re/scripts/gen_rules.py`, and change the ledger line from "828 of 834" to "828 of 834; the 6
residuals are `scholar_rate` (×256) and `caravan_attack_bonus` (×10), both extraction-scale
errors, both corrected on <date>". A bare "828/834" invites the reader to assume the residuals
are unrecovered placeholders. They are not — they are wrong values with a confident-looking
provenance chain, which is the exact failure mode the charter's tripwire exists for.

### A2 — the damage corpus's type ids are **global** ids; §4.1 and §4.6 mis-model the space [measured]

`docs/tooling/damage-hook.md` §4.1 states the table-A/table-B split as "they agree on **every**
defender whose type id is < 364 and disagree on **every** defender whose type id is ≥ 364", and
labels < 364 "the unit range". §4.6 then flags a "loose thread": "defender type ids reach **521,
522 and 526** … beyond the 493 = 364 + 129 combined type space", and recommends a lane for it.

The id in `atk_type_id` / `def_type_id` is the engine's **global** type id, whose class ranges
the live-tables lane independently established as GoodType 0–49, **UnitType 50–413**,
**BuildType 414–542** (`schema/live/live-tables-classmap.tsv`). Resolving the corpus's ids
against that table is decisive:

| top defender ids | global reading | "relative + 50" reading |
|---|---|---|
| 50 (17,104) | **Citizen** | Riflemen |
| 444 (12,857) | **Castle** | Apartments |
| 210 (5,536) | **Light Cavalry** | T80 Tank |
| top attacker 268 (29,579) | **Cannon** | Merchant Fleet |

The global reading is a coherent Dutch Classical→Gunpowder match. The relative reading is not.
[measured]

Three consequences:

1. **The split is real, the boundary is not 364.** I measure `max(def_type_id)` among the 32,074
   agreeing calls = **354**, and `min(def_type_id)` among the 24,706 differing calls = **414**.
   Classified against the live class map, the agreeing set is **32,074/32,074 UnitType** and the
   differing set is **24,706/24,706 BuildType**. The finding — *the tables split exactly on
   unit-vs-building* — **survives and is stronger than stated**. But 364 is the *count* of unit
   types, not a threshold; no defender in this corpus falls in 355–413, so the data localises the
   boundary only to `(354, 414]`. Stating "364" as the measured boundary is over-precision resting
   on a wrong model of the space.
2. **§4.6's loose thread is a false alarm and must be struck.** 521, 522, 526 are BuildTypes
   **Lookout**, **Observation Post**, **Pyramids** — ordinary buildings inside 414–542. Nothing is
   beyond any type space. Left standing, this sentence commissions a lane to investigate a
   non-event.
3. **The balance-index arithmetic is nonetheless correct**, and this is the good news: the hook's
   `0x00C06AFC + 2*(atk*493 + def)` with *global* ids is algebraically identical to indexing the
   real array at `0x00C12BF4 + 2*((atk−50)*493 + (def−50))`, which is exactly the biased-base
   result the live-tables lane derived. I verified it end-to-end (§2.1).

Reproduce: the snippet in §2.1 below prints both readings and the class classification.

### A3 — "fast enough to run inside a per-frame loop" is false [measured]

`native-scanner.md` line 13: *"the tool is fast enough to run inside a per-frame loop if we want
to."* The engine runs at 15 fps (confirmed in the binary at `0x005924CF`, §2.4), i.e. **66.7 ms
per frame**. The fastest scan in the lane's own table is **448 ms**, and the retained heap-only
artifact is **678.9 ms**. That is 7–10× the frame budget. Delete the sentence; "two scans plus a
diff costs about a second", two lines later, is the accurate framing and is fine.

### A4 — the headline latency/throughput is contradicted by the lane's own artifact [measured]

Headline: *"0.45–0.68 s (1.6–2.0 GiB/s)"*. The timing table lists full scans of
1526/538/449/563/518/490/448 ms and heap-only 385/412/370 ms — **no 0.68 s run appears in it**.
The retained heap-only artifact `schema/live/donscan-pid14644-scan.txt` self-reports
`elapsed_ms = 678.9`, `throughput_mib_s = 999.0` over 711 MiB. So the 0.68 s upper bound is real
but untabulated, and the "1.6–2.0 GiB/s" band is falsified by the one heap-only run that was kept
(999 MiB/s). The retained full scan (`490.9 ms`, `1876 MiB/s`) does match table row "full #6".

Fix by adding the missing run to the table and restating throughput as **1.0–2.0 GiB/s**. The
conclusion ("well under the ten-second budget") is unaffected; the specific numbers are what a
later lane will quote.

---

## 2. What I independently reproduced — and what it confirms

This is the part that matters more than the corrections. I re-derived the load-bearing results
rather than reading them.

### 2.1 Cross-lane, cross-instrument: the balance table [measured]

The hook lane read `balance_pct` out of the live process per damage call. The live-tables lane
separately captured the whole array and wrote `schema/live/live-tables-balance-493x493.bin`.
Different instrument, different session, different code path.

**All 56,780 complete damage records' `balance_pct` equal the bin at
`(atk_type_id − 50) * 493 + (def_type_id − 50)`. 0 mismatches.** And all 242 (attacker, defender)
pairs yield a single constant value across the whole match. [measured]

That simultaneously confirms (a) the hook's derived reads are landing on the right addresses,
(b) live-tables' biased-base correction (`0x00C12BF4 = 0x00C06AFC + 2 × 24700`), and (c) that the
ids are global (§A2) — three claims from two lanes, cross-validated.

I also confirmed the bin's own properties exactly: **486,098 bytes** (= 493×493×2),
sha256 `501b47edc9f05f1c…`, **0 zeros, 0 negatives**, range **5…2574**, 367 distinct, modal 100
(120,872 cells). And the earlier `balance-runtime.bin`'s 15,477 zeros and 2,071 negatives lie
**entirely** in its first 24,700 cells, with `old[24700:] == new[:218349]` byte-for-byte.
**`README-LLM.md`'s standing warning "the captured window has unexplained negatives — bound its
real extent before trusting it" is resolved and should be updated.**

```sh
cd /Users/ember/dev/don && uv run --quiet python - <<'EOF'
import csv, struct
names = {int(f[0]): (f[1], f[2]) for f in
         (l.rstrip('\n').split('\t') for l in open('schema/live/live-tables-typeids.tsv')) if f[0] != 'type_id'}
arr = struct.unpack('<243049h', open('schema/live/live-tables-balance-493x493.bin','rb').read())
rows = [r for r in csv.DictReader(open('schema/live/damage-hook/live-damage-snap2.csv'))
        if r['target'] == '0' and int(r['derived_mask']) == 127]
ok = sum(arr[(int(r['atk_type_id'])-50)*493 + int(r['def_type_id'])-50] == int(r['balance_pct']) for r in rows)
print('balance agreement', ok, '/', len(rows))
ag = [r for r in rows if r['def_obj'] == r['def_obj_b']]; df = [r for r in rows if r['def_obj'] != r['def_obj_b']]
print('agree', len(ag), 'max def id', max(int(r['def_type_id']) for r in ag),
      set(names[int(r['def_type_id'])][0] for r in ag))
print('differ', len(df), 'min def id', min(int(r['def_type_id']) for r in df),
      set(names[int(r['def_type_id'])][0] for r in df))
print({i: names[i] for i in (521, 522, 526)})
EOF
```

### 2.2 Cross-lane, cross-instrument: the RULES block [measured]

The hook lane's `donject peek` hex dump (`schema/live/damage-hook/rules-full.txt`, 3,072 bytes at
runtime `0x01798B88`) is **byte-identical** to the economy/live-tables base64 capture
(`schema/live/rules-block-pid14644.txt`, 4,096 bytes) over the whole 3,072-byte overlap. Two
instruments, one object, zero divergence.

**All 16 constants in `EconomyRules::shipped()` verified against that block** [measured]:
`gather_rate 450` (`0x27C`), `commerce_cap [70,100,150,200,260,320,400,500]` (`0x400`),
`attrition 48` (`0xD3C`), `assassin_attrition 8`, `peace_attrition 8`, `militia_attrition 300`,
`siege_attrition 50`, `attrition_aged_up 25`, `british_commerce 25`, `unit_rate_base 120`,
`unit_rate_progression 75`, `dutch_interest 5`, `dutch_interest_cap 50`, `inca_wealth_cap 33`,
`egyptian_food_commerce 10`, `french_timber_commerce 10`. **No folklore in the economy
constants.**

### 2.3 The hook instrument itself [measured]

The single claim everything else in that lane rests on is that the detour captures `ECX`,
argument order and `EAX` correctly. I checked it against the retained validation captures rather
than taking the lane's word:

- `dontest-hook.csv`: **all 39,687 records** satisfy `result − (a1 + 2a2 + 3a3 + 4a4 + 5a5 + 6a6)
  = 1000`, a single constant (= `*(int*)this`), with `seq` **1…39,687 unique and monotonic**, one
  distinct `this`, and `flags = 1` on every row.
- `dontest-rearm.csv`: **all 3,500 records**, same residual.

39,687 + 3,500 = **43,187**, matching the lane's stated offline-recheck count exactly.

And the live corpus's return-address histogram is exactly what static disassembly predicts,
which is a much stronger check than the lane claimed for itself:

| retaddr (static) | records | disassembly at the call |
|---|---:|---|
| `0x6103c9` | 56,540 | `0x6103c0: push esi; push edi; mov esi,ecx; call 0x6469f0` |
| `0x62e61d` | 844 | same shape at `0x62e610` |
| `0x644a09` | 25 | `0x644a04: call 0x6469f0` — the direct call inside damage |
| `0x63fa69` | 24,711 | `0x63fa60: push esi; push edi; mov esi,ecx; call 0x647db0` |

Corpus totals reproduce: **138,909 rows, 56,789 damage (56,780 with `derived_mask == 127`),
57,409 `get_attack`, 24,711 `get_armor`, one tid (14116), 9 rows with `flags & 4`, frames
20,338 → 28,738, `seq` unique 1…138,909**. Damage call sites `0x64ec15` (53,885) and `0x64a4fc`
(2,904); `a4` non-zero 28; `a5` and `a6` non-zero 2,904 each; result −3…40,400 over 148 values.
All [measured], all matching.

### 2.4 Address citations spot-checked with capstone [measured]

Every address I sampled across two lanes disassembles to what the lane says:

| citation | lane | what it actually is |
|---|---|---|
| `0x005924BF` / `0x005924CF` | sim-economy | `inc [ebx+0x550]` … `mov ecx,0xf; cdq; idiv ecx` — **15 fps confirmed in the binary** |
| `0x006CE7B9` | sim-economy | `mov ecx,[eax+0x27c]; shl ecx,4` |
| `0x006CE92C` | sim-economy | `mov [eax+0x3c], 0x1166`; `0x1166 ^ 0x1281 = 0x3E7 = 999` |
| `0x006117C8`+ | sim-economy | `movzx edx,[ebx+0x9e]` … `add eax,ecx; cdq; idiv ecx; test edx,edx` — the modulo gate as described |
| `0x006115EA`+ | sim-economy | `[game+0x550] + [ebx+0xa]`, `and eax,0x8000001f` + sign fixup → the %32 stagger, then `call 0x5e11a0` |
| `0x00650AF6` | sim-economy | `mov ecx,[esi+0x220]` with `esi = [0xc061e4]` = `RULES[UNIT_RATE_BASE]`; `0x650b17` confirms the `player*0x3776 + 0x56fe` per-type stride |
| **`0x006508EB`** | sim-economy | `inc ecx; add al,0x83` — **not an instruction boundary**, so the lane's correction of `economy.md` is right |
| `0x0065F4B0` | live-tables | `mov eax,[0xc061f8]; mov eax,[eax+0x10]; mov esi,[ebx+eax]` — exactly as claimed |
| `0x00581CA0` | live-tables | `imul eax,[ebp+8],0x1ed` (**493**); `movsx eax, word [eax*2+0xc06afc]` — stride and biased base |
| `0x0065FCB8` | live-tables | `push dword ptr [ebx+0x1e4]` — inline scalar, not a pointer |
| `0x00644130` | hook-dll | `55 8B EC 83 EC 1C`, six bytes, no relative operands |
| `0x006441B1` / `0x006441BE` | hook-dll | `call [eax+0x124]` (ret `0x6441b7`), `call [edx+0x120]` (ret `0x6441c4`); `0x6441c4` is `mov edx,[esi+0xc0ab84]` |

**Vtable table verified out of the shipped image**, not from a summary — reading `+0x120`/`+0x124`
at each class's vtable VA from `schema/vtables.json`:
`Unit 0xb417d0 → 0x6103c0 / 0x610160`; `Animal 0xb4145c → 0x6103c0 / 0x610160`;
`Build 0xb42174 → 0x62e610 / 0x63fa60`; `Wall 0xb42cf8 → 0x6469f0 / 0x63fa60`;
`Object 0xb434ac → 0x6469f0 / 0x647db0`. **Identical to `damage-hook.md` §4.2.** The refutation
of "attack/armor come from `get_attack`/`get_armor`" is sound and load-bearing for the damage
port.

### 2.5 donscan geometry re-derived from the retained address lists [measured]

I recomputed the containment and stride analysis from `donscan-pid14644-scan.txt` rather than
trusting the tables:

| claim | my recomputation |
|---|---|
| `OrderList` at `Unit+0xc8` | **600/600 at offset 200, 0 orphans** ✓ |
| `OrderList` at `Animal+0xc8` | **400/400 at offset 200** ✓ (600 + 400 = the full 1,000) |
| `MiningList` at `Build+0x98` | **600/600 at offset 152, 0 orphans** ✓ |
| `GatherPointList` at `Build+0xb8` | **600/615 at offset 184**, 15 elsewhere ✓ |
| `Ammo`, `Guy` not embedded in `Unit` | ✓ offsets in the millions |
| `Unit` stride `0x168` | 584/599 gaps = 360 ✓ (97.5%) |
| `Build 0xe8` / `Animal 0x178` / `City 0xd0` / `Group 0x9d4` / `Ammo 0x88` | 573, 375, 158, 511, 364 ✓ |
| `Guy` allocator granularity `0x100` | 384/480 = **80.0%** ✓ |
| per-vtable 364 / 129 / 873 | ✓ **in the heap-only scan** (see D1) |
| merged UnitType+BuildType has exactly 493 gaps of `0x1c8` | **493 exactly** ✓ |
| `vtables.json`: 1,777 entries, all 4-aligned, span `0xac6d54…0xbc21d8`, 1,318 distinct names | ✓ all four |
| `0xb41ae0` absent from the map, between `UnitOut 0xb41960` and `OrderList 0xb41af4` | ✓ |

**Two lanes independently arrive at 364 UnitType and 129 BuildType** — donscan by halving heap
vtable hits, live-tables by walking the pointer arrays at `[0x00C061F8]+0x10` and counting XML
rows. That agreement is worth more than either alone, and donscan was right to refuse to promote
"493 = the balance-table dimension" to `[measured]` for the table itself. It now *is* established,
by §2.1 — via the accessor's `imul 0x1ed`, not via the coincidence.

**donscan's open question about `TechType = 85` ("reads low for RoN, did not cross-check against
`ron-data/`") is closed by the sibling lane**: `techrules.xml` has exactly 85 `TECH` rows and the
live `TechType` array holds 85 non-null pointers at ids 544–628. Strike the open item.

### 2.6 The economy port's honesty [measured]

`docs/derivation/sim-economy.md` and the `mechanics.rs` doc comments describe the economy work as
**Tier C, transcribed and never executed against retail, with no oracle harness and therefore no
sample count**. That is accurate and it is the right call. The lane also declined to implement
`SUPPORT`/`PROGRESSION` because the only source is BHG prose in `unitrules.xml` — correct
application of the tripwire, and I want that on the record as the behaviour to copy.

**No vacuous tests anywhere in the workspace** [measured]: zero `#[ignore]`, zero `assert!(true)`,
zero `todo!()`/`unimplemented!()` across `crates/`. The two `#[should_panic]` tests carry specific
`expected =` strings (`"0x006CE7C5"`, `"COMMERCE_CAP has 8 entries"`), so they cannot pass on the
wrong panic. `don-rules`'s `reproduces_the_whole_shipped_corpus` asserts 832 slots against
live-validated expectations and additionally pins `scaled == 40` — it is not the parser grading
its own homework.

---

## 3. Tier-vocabulary problems

The charter reserves **A** for SMT equivalence over the whole input domain, **B** for
differential testing against shipped code *with N and a distribution*, **C** for measured
behavioural fidelity. Applying those letters to anything else devalues them.

**B1 — native-tools' returned claims label non-fidelity facts Tier A.** "Toolchain solved:
cargo-xwin cross-links aarch64-pc-windows-msvc" → `tier: A`. Likewise the live image base, the
loader's `ImageBase` rewrite, and the HTTP file-transfer channel. None of these are theorems over
an input domain; they are `[measured]` engineering facts. **`native-scanner.md` itself gets this
right** and uses `measured` throughout — the inflation exists only in the claim JSON the
orchestrator reads, which is arguably worse, because that is the summary that propagates.

**B2 — native-tools labels a stopwatch reading Tier B** ("Full typed heap scan takes 0.45–0.68 s",
`tier: B`, "12 runs"). Fidelity tiers do not apply to performance. Call it `[measured], n = 12`.

**B3 — sim-economy labels the typed rules block Tier B on the strength of a live memory read.**
"828/834 constants confirmed against live process memory" is a **live read** — C / `[measured]` —
not a differential test against executing shipped code. The lane's *other* labels are exemplary
(everything transcribed-not-executed is explicitly C with "NEVER executed against retail — no
sample count and no differential evidence"), which makes this one stand out as sloppiness rather
than intent.

**B4 — hook-dll and live-tables are clean.** hook-dll uses `structural` / `C` / `[measured]`,
opens with "Nothing in this document is verified in the proof-assistant sense", and — the part I
want to praise — **refused to report an agreement rate against `mechanics.rs::damage`** and
explained precisely why (the ~30 `DamagePredicates` are virtual-call results a function-entry hook
cannot observe; a 2^30 search would need a second transcription whose bugs would masquerade as
findings). That is the single best judgement call in the wave. live-tables' Tier-B claims all
carry counts (364/364, 129/129, 578/578, 19,293/19,327). Neither lane wrote *verified*, *proven*,
or *refinement* about a differential test anywhere.

**One Tier-B claim I did not re-run** and therefore mark `[reported]`: the tokenizer's
"202,643 retail calls / 0 mismatches", the damage port's 7,986,695 and 3,994,983 oracle trials.
These are hbox oracle results from earlier waves; this audit had no oracle access. Their sample
counts and distributions *are* stated in the ledger, which is what the charter requires.

---

## 4. Reproducibility gaps

**C1 — load-bearing analysis code lives only in an ephemeral scratchpad.** Three lanes:

| lane | code referenced | where it is |
|---|---|---|
| live-tables | `pstools.py`, `dump_types.ps1`, `dumpnames.ps1`, `dumpbal.ps1`, `parse_dump.py`, `validate.py`, `validate2.py` | `<local-recovery-scratchpad>/` — cited *by that path* in `live-tables.md` §8 "Reproduction" |
| hook-dll | the script that generated `analysis-snap2.txt` (every number in §4) | not in `tools/damage-hook/`, not named anywhere |
| native-tools | `srv.py` (PUT server), `uprecv.py` | "script in the scratchpad; ~20 lines" |

They all exist **right now** [measured] — I checked. They are in `/tmp`. A reproduction section
whose commands point into a session temp directory documents nothing durable. **Move them under
`re/scripts/` or `tools/damage-hook/` and repoint the docs.** This is the cheapest high-value fix
in the wave.

**C2 — the scanner's headline negative result is not reproducible from retained artifacts.** The
"two full scans 25 s apart" diff is the lane's most important result. Only **scan B** survives;
scan A (`Guy 490`, `Group 514`, `AnimObj 11,539`) exists in no file. The null-model control
(158,955 vs 33 / 104 / 136) likewise has no retained artifact — and it is the control that makes
the whole census believable. The *conclusion* still holds on the two artifacts that do exist
(`Unit 600/600`, `Animal 400/400`, `Ammo 400/400`, `City 160/160`, `OrderList 1000/1000`,
`MiningList 600/600`, `GatherPointList 615/615`, `UnitType 728/728`, `BuildType 258/258`,
`TechType 85/85`, against `Guy 481`/`Group 515–517`/`AnimObj 12,756→13,754` moving) — but keep the
JSON next time; it costs 300 KB.

**C3 — `damage-hook.md` §4.3's numbers need an unstated filter.** "base `get_attack` returned
exactly `UnitType[+0x1E8]` in **56,805 / 56,805**" reproduces **only** when the join is restricted
to parent damage records with `derived_mask == 127`. Unfiltered I get **56,811 / 56,812** and
**24,707 / 24,709**. The three "mismatches" are all against parents whose derived read failed, so
`type_attack`/`type_armor` read 0 — the filter is correct and the lane's number is honest. **State
it.** A perfect `N / N` with no stated exclusion is exactly the shape a reader should distrust,
and this one did not deserve the distrust.

---

## 5. Internal inconsistencies (minor, but they will be quoted)

**D1 — `native-scanner.md` mixes three unlabelled scans.** `Build` is **601** in the census table
and **600** in the stride table and in the retained heap-only artifact; the pool-stability table
asserts "`Build` 601 → 601 **stable**". `Group` appears as 517 (census), 515 (stride), and
514 → 517 (diff). The per-vtable table's "count each: 364 / 129 / 873" is the **heap-only** scan;
the **full** scan gives 367 + 378, 129 + 149, 873 + 875, because image-region hits inflate one
vtable per class. Nothing here is wrong; the report just never says which scan each table came
from. Label them, and reconcile `Build` 600 vs 601 (most likely one non-object vptr copy — the
lane's own "is a hit an object start?" open item).

**D2 — live-tables' 99.82% and its own artifact's 99.59% differ.** `live-tables-validation.json`
sums to `direct 19,237 + graft 11 = 19,248 / 19,327` with an **`unexplained` column of 79**. The
doc's headline is **19,293 / 19,327**, reconcilable only by counting the 45 flag cells (6 `FLAGS`,
39 `BUILD_FLAGS`) as explained by the documented loader-superset rule. §7 says "the 34 residuals
being the 33 `PUSH_SIZE` cells and the one `PIKEMENELITE` `tribe_mask` cell" without noting that
it excluded the flag cells the artifact still counts. One sentence fixes it; otherwise an auditor
reading the artifact gets a different number from the one in the headline.

---

## 6. Hand-computed test expectations

The charter says **capture, do not calculate**. `crates/don-sim/src/mechanics.rs`'s
`economy_tests` expectations are calculated, and the comments say so out loud:

```
// Shipped MILITIA_ATTRITION is 300, so the ability branch quarters the interval:
// 256*100/400 = 64, and 48*64/256 = 12 frames.
assert_eq!(attrition_interval_scale(&i, &p, &r), 64);
assert_eq!(attrition_period_frames(r.attrition, 64), 12);
```

and `120 // 100 * 120/100`, `87 // 70 * 125/100`, `115 // 87 * 133/100`, `360 // clamped: 3 * 120`.

I am **not** calling this a violation to hide, because the lane is explicit that the whole economy
port is Tier C, transcribed from instructions, **never executed against retail**, with no oracle
harness in existence for `0x006CE450` — so there is nothing to capture *from* yet. The
**consequence** must be written down where a future reader will hit it: these tests pin the
transcription against the transcriber's own arithmetic and **cannot detect a transcription
error**. They are regression tests for refactors, not evidence of fidelity. The inputs they
consume are properly measured (§2.2, 16/16 constants), which is the part that would have been
fatal to get wrong.

The lane already names the fix as its highest-leverage next step — a `TickResource` oracle harness
modelled on `damage_env.rs`. Concur; that is the one thing that turns this whole subsystem from C
to B, and until it exists no economy number should be quoted as anything but C.

---

## 7. What I could NOT establish

- **Live vs free pool slots.** Unchanged and still the biggest open item in the scanner lane. Every
  pooled count is capacity. I confirmed the *stability* and the *strides* but read no validity field.
- **Whether a vtable hit is an object start** for scattered classes. The null model bounds arithmetic
  coincidence; it says nothing about legitimate stored vptr copies, and the `Build` 600/601 wobble is
  probably exactly one of those.
- **The null-model control and scan A** — no artifacts (C2). I take 158,955 vs 33–136 as `[reported]`.
- **Whether the hook perturbed the sim.** The lane's exonerating evidence is good (freeze survived
  full removal; all 7 sites byte-identical to the shipped image; last damage record one frame before)
  but there is no counterfactual and none is obtainable.
- **The hbox oracle counts** (202,643 / 7,986,695 / 3,994,983) — no oracle access this session;
  `[reported]`.
- **`donhook.c`'s derived-read offsets.** I audited the corpus columns and their cross-consistency,
  not the C that produced them. The balance/vtable/retaddr agreement (§2.1, §2.3, §2.4) is strong
  circumstantial evidence they are right, and it is not a line-by-line review.
- **`donscan` cross-build and live run.** I read the source and re-derived every number from the
  retained JSON; I did not rebuild the `.exe` or touch the VM.
- **`docs/tooling/bhs-bridge.md`, `oracle-regression.md`, `ledger-reconciliation.md`** — outside the
  four lanes I was given claims for. Not audited.

---

## 8. Required actions, in order

1. **Fix `scholar_rate` (×256) and `caravan_attack_bonus` (×10)** in `rules-constants.json`,
   regenerate `rules.rs`, and name the 6 residuals in the ledger instead of leaving "828/834" bare. (A1)
2. **Strike `damage-hook.md` §4.6's "loose thread"** (521/522/526 are Lookout / Observation Post /
   Pyramids) and **restate §4.1's boundary** as unit-vs-building in the global id space (units 50–413,
   buildings 414–542), measured only to `(354, 414]`. (A2)
3. **Move the scratchpad analysis scripts into the repo** and repoint all three reproduction
   sections. (C1)
4. **Delete the per-frame-loop sentence** and add the missing 678.9 ms run to the timing table;
   restate throughput as 1.0–2.0 GiB/s. (A3, A4)
5. **State the `derived_mask == 127` filter** in `damage-hook.md` §4.3. (C3)
6. **Downgrade the Tier A/B labels** on toolchain, image-base, transfer-channel, performance, and
   live-read claims to `[measured]` / C. (B1–B3)
7. **Update `README-LLM.md`**: the balance table's "unexplained negatives" warning is resolved
   (§2.1); the type-object count 364 + 129 = 493 is now measured two ways; `TechType = 85` is
   confirmed against `techrules.xml`.
8. **Delete the stale red-`cargo test` paragraph** in `native-scanner.md` — the tree is green.
