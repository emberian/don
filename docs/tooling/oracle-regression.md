# Oracle regression harness

**Status:** built and green. 12 registered differential cases, **16,236,396 trials, 0
mismatches, 0 skipped**, 59 s wall on hbox. Machine-readable record at
`schema/oracle-regression.json`; the provenance ledger's Tier-B evidence column can now be
generated from it with `tools/ledger-from-measurements.py`.

---

## 1. The gap this closes

Before this, every Tier-B claim in `docs/provenance-ledger.md` rested on **one manual run
that never repeated**. `hash_into_range` at 500,008 inputs, `flank_level` at 500,017, the
balance accessor exhaustively over 243,049 pairs, the damage pipeline at 7,986,695 trials —
each established once, by hand, on hbox, and then quoted from memory forever after.

`cargo test` at the repo root excludes `crates/oracle` (it is i686-only; this Mac is
arm64), so a green suite is evidence about the Rust crates and says nothing about any
fidelity claim. If `don-sim` drifted, nothing would catch it.

Three things were wrong, and the third was the worst:

1. **No re-runner.** The evidence was a paragraph in a document, not a measurement.
2. **Harnesses stranded on one box.** Ledger §7.4 already recorded this: `vector_dist` lived
   only at `hbox:~/lane-pathfinding`, `adler32` only at `hbox:~/don-oracle-checksum`, the
   rule tokenizer only at `hbox:~/don-oracle-econ`. Rebuild hbox and three Tier-B results
   become unreproducible.
3. **The tests did not test the shipped code.** *(finding — see §5.1)* Every model in
   `oracle/src/main.rs` was an inline copy. `difftest`'s `hash_into_range` model was a
   lambda, not `don_sim::hash_into_range`; `combat_difftest`'s flank model was a lambda, not
   `don_sim::flank_level`; the balance case did raw pointer arithmetic rather than calling
   `don_sim::balance_index`. Those tests could only ever tell you that the copy inside the
   oracle was right. **`don-sim` could have drifted arbitrarily and every one of them would
   still have printed PASS.**

---

## 2. What was built

### A declarative case registry — `crates/oracle/src/registry.rs`

Adding a differential case is editing data. A `Case` carries the target VA, the calling
convention, the input plan, **a reference to the Rust function we ship**, and the metadata
that makes the resulting number citable: the ledger heading it is evidence for, the
derivation document, the reachability class, and the caveat that must travel with it.

Ten `Plan` variants cover the ABI shapes present so far — `Stdcall4`, `Ecx1`,
`ThiscallScratch`, `Stdcall2Table`, `Fastcall2`, `Adler32`, `RngNextFloat`, `RngInRange`,
`AsScaled`, `Damage`. A new target whose ABI matches an existing variant needs **no new
code at all**.

The rule that makes the registry worth having: `model` names the shipped function, not a
copy. Where no shipped implementation exists (`Random::get`, `vector_dist`, `adler32`) the
model lives in `src/models.rs` and the `model` string says so — `"NO implementation in this
repo (ledger §2.2)"` — so the JSON carries the gap instead of hiding it.

### One command — `regress`

```sh
regress [--json PATH] [--only ID[,ID..]] [--scale F] [--seed HEX] [--list]
```

Runs every registered case, prints a table with per-phase sample counts and input
distributions, writes the JSON record, and exits:

| exit | meaning |
|---|---|
| 0 | every registered case ran **and** agreed |
| 1 | a mismatch, a crash, or unreadable output from a case |
| 2 | a case was SKIPPED — it produced no evidence |
| 3 | the harness could not start (image missing/unmappable, selftest failed, bad `--only`) |

It is a separate binary rather than a subcommand of `oracle` because `oracle/src/main.rs`
was being actively rewritten by the sweep lane while this was built; see §6.

### The Mac-side driver — `tools/oracle-regress.sh`

Checks hbox is reachable, verifies the remote image's SHA-256 (and warns loudly if it
differs from `ron-bin/riseofnations.exe`), mirrors `rules.xml` if the box lacks it, rsyncs
`crates/{oracle,don-pe,don-sim,don-rules}`, installs the remote workspace manifest, builds
for `i686-unknown-linux-musl`, runs the suite, and scps the JSON back to
`schema/oracle-regression.json`. Everything on hbox runs `nice -n 15 taskset -c 0-3`; the
script installs nothing.

`--status` reads the local record and reports its age and contents without running
anything — including exiting non-zero when there is no record at all.

The remote workspace manifest is version-controlled at `tools/oracle/remote-Cargo.toml`
rather than heredoc'd, so the environment a measurement was taken in is reviewable
alongside the measurement.

### Machine-readable output — `schema/oracle-regression.json`

Schema `don/oracle-regression` v1. Run metadata (unix time, host, target, **SHA-256 of the
bytes the process actually mapped**, seed, scale, selftest result, exit code), a summary,
and per case: id, VA, ABI, model path, subsystem, ledger heading, derivation path,
reachability, caveat, tier, status, trials, mismatches, wall time, per-phase counts with
their distributions, counted exclusions, and the first mismatch if any.

Plus a `known_gaps` array: the Tier-B claims this suite *cannot* re-run, with the reason
for each. An unregistered claim that nobody records is indistinguishable from a passing one.

### The ledger generator — `tools/ledger-from-measurements.py`

Renders ledger-style evidence rows straight from the JSON, and `--check` exits non-zero if
the record is missing, stale (> 30 days), from a filtered run, or not all-pass. It prints
rather than rewriting `docs/provenance-ledger.md`: the ledger also carries prose, caveats
and refutations that no measurement produces, and a script that overwrote them would
quietly delete the most valuable part of the document.

---

## 3. The registered cases

| id | VA | PDB name | model | trials | result |
|---|---|---|---|---:|---|
| `hash_into_range` | `0x00846450` | `Doober::get_num` | `don_sim::hash_into_range` | 500,008 | pass |
| `flank_level` | `0x0092CFE0` | `flanking` | `don_sim::flank_level` | 500,017 | pass |
| `balance_accessor` | `0x00581CA0` | `Balance::return_modifier` | `don_sim::balance_index` | 247,049 | pass |
| `accessor_movsx_word_0xa` | `0x00472400` | — | oracle shape probe | 100,000 | pass |
| `accessor_diff_0x12c_0x12a` | `0x0048F770` | — | oracle shape probe | 100,000 | pass |
| `damage_pipeline` | `0x00644130` | `ObjectData::get_damage` | `don_sim::damage_traced` | 7,986,675 | pass |
| `rng_next_float` | `0x00A39CF0` | `Random::get` | `oracle::models::rng` † | 999,950 | pass |
| `rng_in_range` | `0x00A39D70` | `Random::get(lo,hi)` | `oracle::models::rng` † | 1,000,012 | pass |
| `rng_in_range_wide` | `0x00A39D70` | `Random::get(lo,hi)` | `oracle::models::rng` † | 500,003 | pass |
| `vector_dist` | `0x0046CFF0` | `vector_dist` | `oracle::models` † | 4,000,026 | pass |
| `adler32` | `0x00A46830` | `adler32` | `oracle::models` † | 100,013 | pass |
| `rules_as_scaled` | `0x00A1D110` | `String::fraction` | `don_rules::as_scaled` | 202,643 | pass |

† No implementation exists in `crates/` for these four. The case tests a transcription that
lives only in the oracle, which is exactly what ledger §2 says, and the JSON repeats it in
the `model` field so the distinction cannot be lost downstream.

Three previously-stranded harnesses are now in-tree, closing ledger §7.4:
`~/lane-pathfinding` → `vector_dist`, `~/don-oracle-checksum` → `adler32`,
`~/don-oracle-econ` → `rules_as_scaled`. The tokenizer case is a real improvement on its
predecessor: the original compared retail against a model transcribed in `econ_main.rs`;
this one compares retail against **`don_rules::value::as_scaled`, the parser we ship**.

Trial counts reproduce the ledger's numbers where they can — `hash_into_range` 500,008,
`flank_level` 500,017, `balance_accessor` 247,049, `rules_as_scaled` 202,643 and
`vector_dist` 4,000,026 are exact matches, and `damage_pipeline` lands at 7,986,675 against
the recorded 7,986,695 (the difference is exclusion counts on a re-randomised draw). Only
`adler32` differs deliberately: 100,013 cases instead of 500,000, because 500,000 costs
78 s of the suite's 59 s budget. `--scale 5` reproduces the original count.

### `--scale`

Multiplies randomised and swept phases. **Exhaustive phases never scale** — a shrunk
exhaustive phase is a different claim, so the 493×493 grid is 243,049 pairs at any scale.
`--scale 0.002` gives a 0.5 s smoke run over all 12 cases.

---

## 4. How vacuous green is prevented

This is the whole reason the harness exists, so it is worth being explicit about the
mechanisms rather than asserting the property.

**A case that cannot run is SKIPPED, and skipped is never green.** Missing shipped corpus,
VA outside the mapped image, `PROT_EXEC` refused, `set_thread_area` refused, filtered out by
`--only` — every one produces a SKIPPED record with a reason and a non-zero exit. The
summary prints *"SKIPPED cases produced NO evidence. They are not passes, and this run exits
non-zero because of them."*

**Missing prerequisites are exit 3, not a quiet zero.** No image, or an unmappable one,
prints `NOTHING WAS TESTED` and exits 3. The suite never emits a record that could be
mistaken for evidence.

**hbox unreachable is exit 4.** The Mac script prints `NOTHING WAS TESTED. No fidelity claim
was re-established by this run.` and leaves the previous record untouched — and now older,
which `--status` will say.

**The harness proves itself first.** `selftest` executes machine code we wrote (a `cdecl`
add returning 42 through fork isolation) before any case runs. If it fails, every case is
reported SKIPPED rather than run, because a broken mapping mechanism produces
agreement-shaped output for the wrong reason.

**A crashing case is CRASHED, not absent.** Each case runs in a forked child; if the child
dies, the parent reports the signal. A child that exits without reporting a status is
`ERROR`. Both are failures, never passes.

**Exclusions are counted and printed, never silent.** The damage case excluded 10,304 trials
as retail `#DE` and 3,021 as balance-write collisions; the tokenizer excludes `INT_MIN / -1`.
Each appears in the JSON with its reason and count. A `catch_unwind` that is not an `idiv`
trap is added to the **mismatch** column, because counting an unexpected panic as a skip is
precisely how a suite launders a divergence into a pass.

**A shortened claim is not the claim.** If `data/rules.xml` is absent, `rules_as_scaled`
skips rather than running only its generated half — "exhaustive over the shipped corpus" and
"some of the shipped corpus" are different claims and must not share a label.

**Untaken branches are reported.** The damage case prints `steps_never_taken`, currently
`10-add,11-pct` — the two steps the fabricated world is structurally unable to reach.
"0 mismatches" over a corpus that only ever ran the spine would be a green suite that tested
nothing.

**Verified adversarially, not asserted.** Every path above was exercised: `--only` → exit 2;
absent image → exit 3; absent corpus → the case SKIPs and the run exits 2; unknown `--only`
id → exit 3; unreachable host → exit 4.

And the one that matters most — **the suite was mutation-tested**. Two drifts were injected
into hbox's copy of the sources (never the repo) and both were caught:

| injected drift | caught by | result |
|---|---|---|
| `don_sim::flank_level`: boundary `0xD555_5555` → `0xD555_5556` | edge phase, `x = 0xD5555556` | `FAIL`, exit 1 |
| `don_rules::as_scaled`: zero-denominator early-out returns 12345 | shipped-corpus phase | `FAIL` 180/4,643, exit 1 |

The flank drift was caught by the **edge** phase and not by the 5,000-point sweep, which is
the argument for keeping hand-chosen boundary values in every case: a one-point change to a
2³²-wide domain is invisible to random sampling.

---

## 5. Findings

### 5.1 The existing differential tests did not test the shipped code

Every model in `crates/oracle/src/main.rs` is an inline copy rather than a call into
`don-sim`. `difftest` re-types the `hash_into_range` computation as a closure;
`combat_difftest` re-types `flank_level` as a closure and does the balance lookup with raw
pointer arithmetic. Each proves its own copy correct and would print PASS against an
arbitrarily drifted `don-sim`. The same was true of the tokenizer harness on hbox, which
compared retail against a model transcribed into `econ_main.rs` rather than against
`don_rules::as_scaled`.

Every registry case now points at the shipped function where one exists. `difftest` and
`combat_difftest` are marked SUPERSEDED in place, with the replacement command named; they
should be deleted once the sweep lane's work in that file settles.

### 5.2 The RNG ledger entry named functions that do not exist

`docs/derivation/rng.md` proposes a ledger entry whose *implementation* row reads
`crates/don-sim/src/mechanics.rs (lcg_step, rand_real, rand_int)`. **None of those three
functions exists in `don-sim`** — `grep` finds `1664525` nowhere in the crate. The
independently-rewritten ledger §2.1 gets this right ("none in this repo"), so this is a
stale row in the derivation document rather than a live error, but it is exactly the shape
of claim that becomes folklore. The registry records it in the `model` field of all three
RNG cases.

### 5.3 The measured counts corroborate the ledger

Five of the six re-runnable historical counts reproduced exactly, and `damage_pipeline`
landed within 20 trials of its recorded 7,986,695. The tokenizer's 202,643 is an exact
match from an independently written corpus extractor. That is a real, if modest,
cross-check on the numbers the ledger has been quoting.

---

## 6. Deliberate compromises, and the follow-ups they imply

**`regress` is a separate binary, not `oracle regress`.** `crates/oracle/src/main.rs` was
being rewritten by the ISLAND-sweep lane throughout this work (a trampoline, `probe_calls`,
a rewritten `sweep`). Rewriting it would have destroyed live work, so the suite went into
`src/bin/regress.rs` and `main.rs` received only two additive doc comments. **Follow-up:**
once the sweep lane lands, delete `difftest` and `combat_difftest`, change
`mod damage_env; mod damage_test;` to `use oracle::{damage_env, damage_test};`, and add a
`regress` arm that delegates to `oracle::run`.

**`damage_env.rs` and `damage_test.rs` compile twice** — once into the new `oracle` lib and
once into the `oracle` bin, because `main.rs` still declares them as its own modules. No
source is duplicated and no types mix; it costs build time only, and the `use` change above
removes it.

**`src/bin/rng.rs` still carries its own copy of the RNG models and of `Mapped`.** The
registry uses `oracle::models::rng`, so the *tested* model is the shared one, but two
oracle-local copies can still drift apart. Left untouched to keep the conflict surface with
live lanes small. **Follow-up:** point `rng.rs` at `oracle::models::rng` and
`oracle::image::Mapped`.

**`adler32` runs 100,013 cases rather than the historical 500,000**, to keep the suite under
a minute. Recorded in the case's caveat, in the JSON, and above; `--scale 5` restores it.

**The suite builds debug, not release.** Debug keeps Rust's overflow checks on, so an
accidentally non-wrapping arithmetic op in a model panics and is counted as a mismatch
rather than silently wrapping into agreement. `--release` is available; the default is the
stricter one.

---

## 7. What the harness still does not cover

Seven entries, machine-readable in the JSON's `known_gaps`:

- **§1.4 `entrench_dir_level`** — inlined at `0x00644E0E`; no standalone function exists to
  call. Exercised only indirectly inside `damage_pipeline`.
- **§1.6 `get_attack` / `get_armor`** — the upgrade branches are structurally unreachable in
  the fabricated world, and ledger §4.4 shows live combat dispatches to these base-class
  leaves *zero* times in 56,789 captured calls, so even the tested path is not the live one.
- **§1.8 `Rules`** — validated against a running process on the VM, not the oracle.
- **§1.10 field-offset table** — mechanically extracted, downgraded to structural; nothing
  callable to test.
- **§1.9 economy** — Tier C, transcribed and never executed against retail. **No harness
  exists.** This is the largest genuinely-missing piece.
- **damage mutation evidence** — a property of a deliberately modified tree; the harness
  cannot assert it about itself. Re-running `hbox:~/don-oracle/mutate.py` periodically is
  the right way to check the suite still bites.
- **§3 structural results** — checksum field set, replay format, live type tables,
  pathfinding structure, live damage capture. Measured, but not behavioural claims about a
  Rust function, so there is nothing for a differential case to compare. They are not
  weakened by this suite's silence and must not be strengthened by its green.

Two caveats that bound cases which *do* pass, both from the rewritten ledger and both
carried in the JSON so they travel with the number:

- **`damage_pipeline`** aliases `[0x00C0AB84]` and `[0x00C0AEC0]`; a live capture refutes
  that they alias (§4.3). The 7,986,675 trials stand — what changes is *which inputs they
  cover*: building defenders are excluded.
- **`balance_accessor`** tests address arithmetic against the *file* image. `0x00C06AFC` is
  a bias-folded base (§4.5); the live array is `final_balance_table` at `0x00C12BF4`, and
  the type-id domain is wider than 493 (§5.4).

---

## 8. Reproducing

```sh
tools/oracle-regress.sh                 # full suite, ~59 s, writes schema/oracle-regression.json
tools/oracle-regress.sh --scale 0.002   # 0.5 s smoke run over all 12 cases
tools/oracle-regress.sh --status        # age and contents of the last record; runs nothing
tools/ledger-from-measurements.py       # ledger evidence rows, from the measurements
tools/ledger-from-measurements.py --check   # non-zero if missing, stale, filtered, or not all-pass
```

On hbox directly, from `~/don-oracle`:

```sh
nice -n 15 taskset -c 0-3 cargo build --target i686-unknown-linux-musl --bin regress
nice -n 15 taskset -c 0-3 ./target/i686-unknown-linux-musl/debug/regress --json out.json
./target/i686-unknown-linux-musl/debug/regress --list
```

Last clean run: **12/12 pass, 16,236,396 trials, 0 mismatches, 58,687 ms**, image sha256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.

## 9. Effect on `cargo test`

None. This work touches `crates/oracle` (excluded from the workspace), `tools/`, `schema/`,
`README-LLM.md` and this file. `cargo test --workspace --exclude don-net --exclude don-ai`
is green: don-rules 8, don-pe 5, don-sim 5 + 18 + 55, don-gpu 5, all passing.

`crates/don-net` and `crates/don-ai` are untracked crates added to the workspace by other
live lanes while this was built, and `don-net`'s replay round-trip tests currently fail
(46 of 63 recorded games do not round-trip; 4 cross-player checksum tuples disagree). That
is their in-progress work, not a regression from this change — nothing here touches a
workspace crate.

Tier B is differential testing, not verification. Every count above is a sample size over a
stated distribution and says nothing about inputs outside it.
