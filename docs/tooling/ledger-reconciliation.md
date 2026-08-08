# Ledger reconciliation — 2026-08-08

What changed in `docs/provenance-ledger.md`, and why. Written so the next reader can check the
work rather than trust it.

**Inputs.** All thirteen reports under `docs/derivation/` (`combat`, `pathfinding`, `rng`,
`checksum`, `economy`, `damage-port`, `replay-io`, `replay-stream`, `replay-checksum`,
`sim-economy`, `live-tables`, `simd-batch`, `gpu-architecture`), both adversarial audits
(`AUDIT.md`, `AUDIT-replay.md`), both tooling reports (`docs/tooling/damage-hook.md`,
`docs/tooling/native-scanner.md`), and the actual contents of `crates/`.

**Adjudication rule used.** Where lanes contradict, the audits win. Where the audits are
themselves superseded by a later wave's measurement, the measurement wins and the supersession
is stated. Where nothing settles it, it is recorded as OPEN in §5 rather than resolved by
preference.

**Nothing was committed or staged. No code was edited.** Three wrong statements found in
shipped code are flagged in ledger §7.3 and listed again at the end of this file.

---

## The shape of the change

The old ledger was 281 lines covering 8 mechanics, with a "not yet derived" list six items
stale. The new one is organised so the two things people get wrong are structurally impossible
to get wrong:

| section | what it is for |
|---|---|
| §1 Implemented | the mandatory charter list — mechanics that exist as Rust here |
| §2 Derived, tiered, **not implemented** | Tier-B results with no Rust. Previously these read as project assets |
| §3 Structural | layouts/formats/addresses. Real, and **not** a fidelity claim |
| §4 Corrected / refuted | each with the *wrong claim* stated, so nobody re-derives it |
| §5 Open | contradictions recorded as contradictions |
| §6 Not yet derived | the do-not-implement-from-folklore list |
| §7 Compliance sweep | `crates/` audited against §1, both directions |
| §8 Live-process log | captures, and the method traps that cost wall-clock |

---

## 1. Two findings this reconciliation produced itself

Both were found while cross-checking, not read out of a report. Both are load-bearing.

### 1.1 The ledger's own "pathfinding has its own RNG" section was wrong

The previous ledger claimed `[0x00C06184]` → a *heap-allocated pathfinder* `Random` and
`0x00EB697C` → the *script/sim* `Random`, and concluded "a pathfinding divergence does not
automatically desynchronise the scripted/sim stream — the pathfinding lane's headline overstated
the blast radius". `README-LLM.md` carries the same claim.

Measured here, with capstone on the shipped image:

```
static [0x00C06184] = 0x00E37A8C
0x00E37A8C + 0x00960000 (the PID-14644 ASLR delta) = 0x01797A8C   <- exactly what the live read saw
0x009E1890  mov ecx, dword ptr [0xc06184]   ; MathUtilFuncSet::rand_int -- the BHS script API
0x00A39D40  mov ecx, 0xeb697c               ; the *other* stream's fastcall wrapper
```

So the live read did not observe a separate heap object; it observed the **rebased static main
`Random`**, which is why it was already populated at the menu. `[0x00C06184]` → `0x00E37A8C` is
the one main simulation stream, shared by the road pathfinder's per-edge draw, map generation,
units/animals, and the script API. `0x00EB697C` is the secondary stream (`Surf.cpp`, `Scene`,
graphics), reached through `0x00A39D40`.

**Consequence: the pathfinding lane's determinism warning stands in full** — a divergence in how
many edges we relax desynchronises every later draw on the main stream, scripts included. This is
now ledger §4.1. `README-LLM.md` needs the same correction and did not get it here.

### 1.2 The pathfinding lane derived the **road/caravan** pathfinder, not "the" pathfinder

`ron-bin/sbl/rise.pdb` names the three functions the lane built its report on:

| address | lane called it | PDB name |
|---|---|---|
| `0x00685990` | "the A\* driver" | `PathFinder::astar_caravan_road` |
| `0x00686300` | "the per-edge cost" | `PathFinder::calc_road_cost` |
| `0x00688740` | "step legality / passability" | `PathFinderData::valid_roadcoord` |

Everything the lane measured is still true of *that* search. Its headline — "Rise of Nations'
pathfinder is a pure-integer 8-connected grid A\*" — is scoped to one entry point among several
(`0x00688A40`, `0x00688FC0`, `0x006897D0` remain unread). Ledger §3.6 and §4.12.

---

## 2. The shipped PDB, and how it is used

`ron-bin/sbl/rise.pdb` (57,290,752 B) is the PDB for **this exact binary**, verified here:

```
EXE  CodeView entry: E:\agent\_work\2\s\main\game\rise.pdb
     guid 51D4F219-61C6-4F84-9D5B-C3361B0D291F  age 1
PDB  guid 51D4F219-61C6-4F84-9D5B-C3361B0D291F  age 1
```

[measured, this reconciliation]. Artifacts already extracted by an in-flight lane:
`schema/rise-symbols.tsv` (37,138 public symbols) and `schema/command-structs.txt` (83
`*Command` layouts). That the files are the *shipped* PDBs rather than symbol-server downloads
is `[reported]`, from the docstring of `re/scripts/pdb_read.py`.

**How the ledger treats it.** As structural ground truth for *names and layouts only*. It
changes no tier — a name is not a behaviour. Rows carry `[measured, rise.pdb]` where a name came
from it. Two cautions are stated in the ledger and repeated here: there is **no derivation report
and no adversarial pass covering the PDB extraction**, and at least one name is actively
misleading if taken as semantics (`ObjectData::train_time` — the ramp *is* train time, but `x`,
the value it scales, is still underived, so `ramped_rate` still must not be wired to a build
queue).

What it settled, all cross-checked against work derived independently:

- **Confirmed ~25 function identities** the project derived the hard way — `adler32` at
  `0x00A46830`, `Random::get` at `0x00A39CF0`/`0x00A39D70`, `CheckSums::check_all` at
  `0x00936560`, `Balance::return_modifier`, `flanking`, `vector_dist`, `ObjectData::get_damage`,
  `Game::do_frame`, `RecordGame::write_package`/`read_package`, and more. That 25 independent
  derivations all land on the compiler's own names is the strongest cross-validation in the repo.
- **`Doober::get_num`** — `0x00846450`, the project's first derived mechanic, finally has an
  identity. It is a coordinate-keyed number generator for dropped-resource pickups, and its
  *not*-being-the-RNG is now settled by name as well as by xref.
- **`CommandPackage` layout** (`stamp`/`play`/`valid`/`group`/`size`/`data[512]`/
  **`Random padding` at `+0x214`**) — definitively ends the three-lane header dispute, and the
  `padding` member independently confirms `rng.md`'s otherwise-unattributed `[esi+0x214]` stream.
- **`CheckSumsCommand` sizeof = 65** with the sixteenth `unsigned long` named `all_checksum` at
  `+0x3d` — a fourth independent confirmation of the `0x41`/65 correction.
- **`TurnDataCommand` sizeof = 11** — closes `replay-checksum` §10's "MP body arithmetic wants 12
  sometimes"; the extra byte is the `in_range(0,2)` padding, not a mis-read field.
- **`Constants::init` vs `Constants::log_data`** — confirms the economy lane's refutation that
  `FUN_00570170` is the loader.
- **Name corrections** now recorded in ledger §4.9: `RString::AsScaled` → `String::fraction`;
  `Player::TickResource`/`UpdateCommerceCaps` → `Leader::do_gather`/`calc_resource_caps`;
  `DataWalk::walk`/`walk_tag` → `walk_function`/`walk_test`; `Object::setPosition` →
  `SubObject::set_new_location`; `Random::exchange_seed` → `Random::reseed`; `0x33 ping_line` →
  `process_spline`; `live-tables.md`'s "`FUN_0065FC00`/`FUN_0061C490` are the `DataWalk`/checksum
  methods" → they are `log_data` visitors, a different interface entirely.
- **One new open item**: the PDB signature is `flanking(unsigned long, unsigned long)` — two
  arguments — while the Tier-B model uses one in ECX. Recorded as OPEN (§5.6), not explained away.

---

## 3. Tier corrections — every downgrade made

| what | was | now | why |
|---|---|---|---|
| Field offsets, 1,223 constants | **B** | **structural** | No retail code was executed. The cited evidence is "instruction extraction agrees with Ghidra decompiled C", and the charter says decompiled C is a hypothesis, not a second derivation. This was tier inflation sitting in the charter's own ledger (`AUDIT.md` §3.3). |
| `replay-checksum` corpus statistics (4 rows) | **B** | **C / [measured] observation** | Tier B is *our Rust agreeing with shipped code on N generated inputs*. **No replay lane executed shipped code at all.** Reading N files is Tier C. (`AUDIT-replay.md` §F2.) |
| "`CheckSumsCommand` is 65 bytes" in a structured summary | **A** | **structural [measured]** | Charter Tier A is an SMT equivalence over the entire input domain. Nothing in any replay lane is Tier A; the document's own text said "structural + B" and the inflation happened in the summary an orchestrator reads. |
| `install_fake_teb()` | **B** | **engineering** | A TEB fix is oracle plumbing, not a mechanic, and "1,500,012 calls, 0 faults" is a smoke test, not a differential result. |
| rng cosmetic-stream attribution | **C** | **structural** | Tier C means behaviourally faithful with divergence measured; no behaviour was observed. It is a `__FILE__`-proximity heuristic, which `rng.md`'s prose says correctly and its tier field did not. |
| Economy mechanics | (previously accurate) | **C at best**, kept and hardened | Language tightened to "transcribed from capstone output, never executed against retail. No harness, no sample count, no differential evidence. Do not describe it as tested." |
| The whole file | — | — | Added an explicit "**There is no Tier A anywhere in this project**" statement, and the standing note that `cargo test` cannot back a fidelity claim because `Cargo.toml` excludes `crates/oracle`. |

No tier was raised. Two Tier-B results gained *scope* corrections that narrow what they cover
(`damage`, `get_attack`/`get_armor`) rather than changing the tier.

---

## 4. Contradictions between lanes, and how each was disposed of

| contradiction | disposition |
|---|---|
| `cavalry_flank_bonus`/`vehicle_flank_bonus`: 1/256 fixed point (`combat.md` §8) vs plain `_wtoi` (`economy.md`, `rules-constants.json`) | **Settled for `_wtoi`.** The audit read the loader (`0x00569DD3`/`0x00569DEB` — the plain binder, no scale pushed, while neighbouring offsets all `push 0x100`), and the live `RULES` read from the damage hook shows 40 and 33. §4.2. The *residual* question — whether the resulting 7%/6% is a genuine RoN bug — stays OPEN (§5.3). |
| Which global is `RULES`: `0x00C061E4` vs `0x00C061F0` | **Closed by live read**, superseding `AUDIT.md` §3.4's "unresolved". Both pointers held `0x01798B88` in a match-loaded process: they alias one object. Recorded with the caveat that this is one process at one moment. §5.1. |
| Replay record header: three lanes, three field namings | **Settled for `replay-io`.** The writer, the reader, the `GameLog` name bindings and now the PDB struct all agree: disk `+0x00` is the **game frame** (a global), `+0x0C` is `stamp`. `replay-stream` and `replay-checksum` both inverted it, and `replay-checksum` additionally asserted the disk record *is* the in-memory struct, which the writer refutes at its first instruction. §4.7. |
| `check_sums` cadence: "one per two turns" (`replay-io`) vs "every package" (`replay-checksum`) | **Settled for `replay-checksum`**: 25,271 of 25,279 packages, measured by the auditor's own decoder. `replay-io`'s 1,390 for `mp2024` was a 37% undercount from a partial XOR probe, reported as a finding rather than as a symptom (true count 2,221). §4.11. |
| MP streams "NOT statically decodable" (`replay-stream`) / "THE BLOCKER" (`replay-io`) | **Both refuted**: 25,279/25,279 packages decode exactly, key `K = (u16)(GameInfo+0x04 >> 8)`, padding from a `Random` that is a member of the `CommandPackage` itself. §3.3, §4.11. |
| 15 fps: `[reported]` (`AUDIT-replay.md` §F7) vs `[measured]` (`sim-economy.md`) | **Resolved to `[measured]`** — the later wave found it at `0x005924CF`. The auditor said only that *he* could not find it in the time available, which is the honest form of the claim and not a contradiction. §1.9. |
| `0x33`/`0x44` opcode sizes "unresolved" (`replay-checksum`) | **Resolved** — both are recoverable from the bulk decompilation and are now confirmed by PDB struct layouts. §3.2. |
| Balance-table extent "unresolved, unexplained negatives" (`combat.md` §5, `AUDIT.md` §8) | **Resolved by `live-tables`**: the array is `final_balance_table` at `0x00C12BF4` and `0x00C06AFC` is a bias-folded base (`49,400 = 2 × (50×493 + 50)`). The negatives came from a capture taken 49,400 bytes too early. §4.5. **New** open item: the live type-id domain reaches 526, beyond 493 (§5.4). |
| Object tables `[0x00C0AB84]`/`[0x00C0AEC0]` "assumed to alias" | **Refuted by live capture**: they agree on every unit defender and differ on every building defender, with `0xFFFFFFFF` in 98% of the disagreements. §4.3. |
| `get_attack`/`get_armor` as the source of `attack`/`armor` | **Refuted as a model of live play**: 0 of 56,789 damage calls reached the base functions through the in-function vtable calls. Four class overrides supply the operands and are underived. §4.4. |
| Pathfinding "the blocker is not floating point" | **Not supported as a whole-sim claim** (`AUDIT.md` §3.2), and additionally **mis-scoped** to the road pathfinder (this reconciliation). §4.12. |

---

## 5. Six items removed from "not yet derived" because they landed

The old list still forbade implementing things that had already been derived. Removed, with
where they went:

| stale entry | now |
|---|---|
| the damage pipeline's operation order, rounding, and per-mask modifier table | §1.2, Tier B at 7,986,695 trials; "the table" is refuted — it is an inline chain |
| the engine's RNG algorithm and its call sites | §2.1, Tier B at ~2.5 M trials, reproduced by the audit — **but not implemented** |
| gather rates, cost ramping, attrition timing | §1.9, Tier C |
| pathfinding | §3.6 structural + §2.2 (`vector_dist`, Tier B, 4,000,026 trials) |
| the lockstep checksum's field set | §3.1 mechanism closed; the *closed traversal* remains open |
| rule-value tokenizer semantics | §1.7, Tier B at 202,643 calls |

Nine new entries were added to the list, most of them created *by* the new findings: the four
attack/armor override functions, the composition of `final_balance_table` from `balance.xml`, the
non-road pathfinder entry points, `peace_attrition`'s site, nation ids, and the replay
type-id→name mapping.

---

## 6. Rows whose evidence was tightened rather than changed

- **`damage`** — the "0 mismatches" figure now states explicitly that it is *about the return
  value only* (retail writes `out_kind` and the harness ignores it), and carries the two new
  scope limits from live capture (§4.3, §4.4).
- **`hash_into_range`** — `reachability: ISLAND` was technically true and misleading. It now says
  **dead code**, with the rng lane's counter-caveat that an unreferenced COMDAT can still be
  semantically live (`MathUtilFuncSet::rand_int` is also unreferenced and certainly live).
- **`balance_index`** — the "table contents are runtime-loaded and untested" caveat is replaced
  by the live capture and its exact statistics, plus the new type-id-domain open item.
- **`Rules`** — gained the independent corroboration that 18 combat `RULES` offsets re-read from
  a *different* live match all match the extracted table.
- **`as_scaled`** — gained the `wcschr`-searches-the-whole-string hazard and the UTF-16-vs-`&str`
  boundary from `sim-economy.md` §2.4.
- **Replay corpus numbers** — the lane's totals and the auditor's independently-measured totals
  are now presented as **two separate columns** rather than merged, because they cover different
  specimen sets. The lane's "824,205 channel values, all adler-32-shaped, zero exceptions" is
  recorded as **false and a category error**: `total` is a wrapping sum of fifteen adler-32s and
  has no reason to look like one (14 counter-examples in `h1` alone). The defensible form is
  "the *fifteen* channels are adler-32-shaped, 379,065/379,065; the sixteenth is their wrapping
  sum, 25,271/25,271".
- **"Parses exactly to EOF with zero residue"** is now recorded as **not evidence**: 7,440 of
  8,192 candidate start offsets produce a chain terminating exactly on the last byte. The framing
  is settled by the disassembly, not by the parse.

---

## 7. What the sweep of `crates/` found

**Implemented with no ledger row: none.** Every public item in `don-sim/src/mechanics.rs` and
`don-rules/src/{value,rules,offsets}.rs` maps to a row.

**Claimed but not implemented: three**, now in ledger §2 with implementation path "none" —
`Random::get` (exists only in the oracle harness `crates/oracle/src/bin/rng.rs`), `vector_dist`
and `adler32` (exist nowhere in this repo).

**Reproducibility gap:** the `vector_dist` and `adler32` harnesses live **only on hbox**
(`~/lane-pathfinding`, `~/don-oracle-checksum`) and are not in the tree. If hbox is rebuilt,
those two Tier-B results become unreproducible. `~/don-oracle-econ` is nearly in the same
position. The damage, combat and rng harnesses *are* in-tree.

**Three wrong statements in shipped code — flagged, not edited** (this reconciliation touched no
code):

1. `crates/don-sim/src/mechanics.rs:158-161` — `cavalry_flank_bonus` / `vehicle_flank_bonus`
   documented as "8.8 fixed point". Refuted; the loader uses the plain `_wtoi` binder and live
   `RULES` holds 40 and 33. **This is the exact claim `AUDIT.md` §3.1 told both lanes not to
   carry forward, and it is still in the tree.** The arithmetic is right; only the doc is wrong.
2. `crates/don-sim/src/world.rs:26` — `TICK_HZ` documented as "not yet confirmed against the
   binary". It is, at `0x005924CF`.
3. `crates/don-sim/src/world.rs:33-38` — `SUBTILE`'s doc says the binary is "float-heavy" and
   that `f32`-vs-fixed-point for rule values is open. The damage pipeline and the road A\* have
   zero float, and the tokenizer question is closed (`i32`).

**Test state:** `cargo test` → **91 passed, 0 failed, 0 ignored** (don-gpu 8 + parity 5, don-pe
5, don-rules 18, don-sim 55) [measured]. The two standing vacuity hazards the audit raised are
unchanged: `parses_the_whole_shipped_corpus` skips silently without `ron-data/`, and don-gpu's
four parity tests skip silently without a GPU adapter, with the notice going to stderr where
`cargo test` hides it.

---

## 8. Other documents that are now stale

Not touched here; listed so they get fixed.

| file | what is stale |
|---|---|
| `README-LLM.md` | ~~"Pathfinder uses a *different* `Random` via pointer global `[0x00C06184]`; script/sim uses fixed object `0x00EB697C`" — **refuted**, §1.1 above. Also: no mention of `ron-bin/sbl/rise.pdb`.~~ **FIXED** by `docs/derivation/PDB-RECONCILIATION.md` §8 (2026-08-08): the RNG entry is corrected with the stream census, and a §"The shipped PDB" section was added. |
| `docs/binary-ground-truth.md` | still calls `FUN_00570170` the rules loader (two places); still calls the descriptor's second word a "type tag"; the `BorderSpline` line needs the drawn-ribbon-vs-integer-radius split. |
| `docs/derivation/checksum.md` §4 | `CheckSumsCommand` "0x3d = 61 bytes" — it is `0x41` = 65 with `all_checksum` at `+0x3d`. |
| `docs/replay-format.md` | `0x28` is the UTF-16 `'('`, not a length; the length is the u32 at `+2`. |
| `docs/derivation/replay-checksum.md` | `today.rcx` has 10,544 records (not 10,532/10,541); header field naming inverted; the "all channel values adler-32-shaped, zero exceptions" claim; four Tier-B rows. |
| `docs/derivation/replay-io.md` | "THE BLOCKER"; `check_sums` cadence; the `mp2024` count of 1,390; which channels read 1. |
| `docs/derivation/replay-stream.md` | "MP streams are NOT statically decodable"; the zoom and pairing universals; 15 fps is now `[measured]`, so that `[reported]` mark can be lifted. |
| `docs/derivation/combat.md` | §8's two 1/256 rows; §1's "1142 instructions" (1,195); §1's `islands.jsonl` gap boundaries; §5's balance-table extent (now resolved). |
| `docs/derivation/pathfinding.md` | headline scope — it is the road/caravan pathfinder; the float claim as a whole-sim statement. |
| `docs/derivation/live-tables.md` | §1 calls `FUN_0065FC00`/`FUN_0061C490` the `DataWalk`/checksum methods; the PDB says `log_data`. |
| `GOAL.md` | badly stale — still says "Stage 4+ not begun. **No simulation, no mechanics, no benchmarks yet.**" and its "next 3 moves" are all closed (the tag question, the tokenizer, the loader identification). |

---

## 9. Reproducing the two new results

```sh
# 1.1 — the RNG stream globals
cd /Users/ember/dev/don/ron-bin && uv run --quiet --with capstone --with pefile python - <<'PY'
import pefile
from capstone import *
pe = pefile.PE("riseofnations.exe"); base = pe.OPTIONAL_HEADER.ImageBase
img = pe.get_memory_mapped_image(); md = Cs(CS_ARCH_X86, CS_MODE_32)
print("static [0x00C06184] =", hex(int.from_bytes(img[0xc06184-base:0xc06184-base+4],'little')))
print("rebased            =", hex(0x00e37a8c + 0x960000))
for ea, n in ((0x009e1890, 32), (0x00a39d40, 16)):
    for i in md.disasm(img[ea-base:ea-base+n], ea):
        print(f"{i.address:08x} {i.mnemonic} {i.op_str}")
PY

# 2 — the PDB matches this exact binary
cd /Users/ember/dev/don/re/scripts
python3 pdb_symbols.py /Users/ember/dev/don/ron-bin/sbl/rise.pdb --base 0x400000 \
        --va 00644130 00846450 0092cfe0 0046cff0 00a1d110 00685990 006508c0
python3 pdb_types.py   /Users/ember/dev/don/ron-bin/sbl/rise.pdb --struct CommandPackage
```
