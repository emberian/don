# Track: dual mode — the fidelity / improved split

**What a human can now do that they could not before.**

1. **Ask the program what our edition changes about Rise of Nations, and get a real answer.**
   `cargo run -p don-sim --bin don-deviations` prints twenty-three catalogued entries; `--show
   <slug>` prints one in full with the retail address it was derived from and the doc that
   measured it. Before this, that list existed only as scattered paragraphs in nine
   different derivation documents, and nobody could have told you how long it was.

2. **Make a fidelity or product-readiness claim that cannot be quietly contaminated.**
   `don-deviations --assert-ready replay|playable|rl-env|product` fails closed on reachable
   known drift, and on an active improved-mode fix until its real call site adopts the seam.
   Replay additionally requires fidelity mode. Research-only models stay usable without
   pretending to be product-complete.

3. **Turn one improvement on without turning the others on.** Each entry is an independent
   switch, so "does the AI still open sensibly with the gather cheat off but the script bug
   left in?" is now a question you can run rather than a refactor you have to do.

4. **Hand `docs/tracks/deviations.md` to someone who loves the original.** It leads with the
   nine things our edition does differently, in play terms, and every one of them cites the
   instruction it was read from.

---

## 1. The shape of it

`crates/don-sim/src/deviations.rs` — one module, no dependencies, 23 registry entries and
the scoped readiness policy. `crates/don-sim/tests/fidelity_mode.rs` — integration tests
for both mode immutability and reachable-gap gating. `crates/don-sim/src/bin/deviations.rs`
— the CLI, `--assert-fidelity`, and `--assert-ready` checks.

```rust
let retail = ModeConfig::default();          // fidelity. always.
assert!(!retail.is_active(Deviation::AiGatherHandicap));
assert_eq!(behaviour::gather_handicap_pct(&retail, 0), -35);   // retail's number

let mut ours = ModeConfig::improved();       // our edition
assert_eq!(behaviour::gather_handicap_pct(&ours, 0), 0);
ours.disable(Deviation::AiGatherHandicap);   // …individually revocable
```

The executable gates are:

```sh
don-deviations --assert-ready replay
don-deviations --mode fidelity --assert-ready playable
don-deviations --mode improved --assert-ready playable
don-deviations --assert-ready rl-env
don-deviations --mode improved --assert-ready product
```

`replay` requires fidelity mode and checks only gaps reachable by replay validation.
`playable` and `rl-env` check their concrete runtimes. `product` is their non-research
aggregate. A green replay result therefore does not claim the arena is complete, and an
unfinished research harness does not make an unrelated replay run red.
`tools/product-readiness.sh` runs that aggregate gate for both canonical modes and refuses
release until every listed blocker is removed rather than waived.

Three ideas carry the design.

**The fidelity invariant is structural, not remembered.** `ModeConfig` holds a mode and a
bitmask. `fidelity()` leaves the mask empty; `enable()` returns
`Err(FidelityIsImmutable)` in fidelity mode rather than complying; and `is_active()`
short-circuits to `false` in fidelity mode regardless of the mask. Three independent
barriers, because the failure being guarded against is silent — a fidelity number produced
under a fix looks exactly like a fidelity number.

**Every divergence is a named seam that keeps both branches.** `behaviour::` holds one
function per fix, each containing retail's behaviour *and* ours, with retail's address in
the doc comment. A subsystem that consults a seam keeps its derivation intact and gains a
switch; a subsystem that forks its own improved copy loses both. That is the whole
difference between a deviation and drift.

**An entry that cannot say where it came from is not admissible.** `Entry::derived_from` is
`&[&str]` of addresses, shipped-data lines or source sites, and a test fails if it is empty.
`Entry::evidence` names the document and the confidence. This is the charter's rule
("say where it came from — an address, a file and line, or a live read") expressed as a
type.

### The three kinds, and why `Drift` exists

| kind | toggleable | active in | meaning |
|---|---|---|---|
| `Fix` | yes | improved only | a deliberate improvement, argued and revocable |
| `Drift` | **no** | **both** | known approximation; blocks each reachable product surface |
| `Rejected` | no | never | investigated, turned out not to be a deviation |

`Drift` is the entry type that keeps the register honest. Product drift names the exact
execution surfaces it reaches and `assert_ready` refuses those runs. The missing RL patrol
executors block `rl-env`; the arena's five remaining simplified game systems block `playable`; both block the
aggregate `product` target. The older `don-ai::game` simplifications are explicitly
`ResearchOnly`: useful for bounded experiments, never evidence about the product.

`Rejected` exists because two candidates in this lane's own brief turned out **not to be
deviations** (§3), and both had already been rediscovered more than once. Writing down what
is *right* is cheaper than watching a third lane "fix" it.

---

## 2. The registry as seeded

Nine fixes, twelve drift, two rejected. Full prose in `docs/tracks/deviations.md`; the short
form:

| slug | retail | ours | improved default |
|---|---|---|---|
| `ai-gather-handicap` | income scaled −35 %…+50 % by difficulty (`0x006D66A0` → `0x006CE450`) | no income bonus at any difficulty | **on** |
| `gather-handicap-truncation` | truncating idiv makes low difficulties harsher than nominal | round to nearest | **on** |
| `bhs-prereq-result-test` | `economic.bhs:638` tests `== 0`; engine returns `-1` → forever loop, watchdog disarmed | treat `≤ 0` as failure | **on** |
| `bhs-citizens-typo` | five steps train `"Citizens"`, a type that does not exist | resolve to `Citizen` | **on** |
| `tikal-border-rule-slot` | borders read `TIKAL_TEMPLE_HP` (`+0x4A0`) at `0x006B0DC9` | read `TIKAL_TEMPLE_BORDERS` (`+0x498`) | **on** |
| `refund-charges-player` | `stockpile += amt − adj` goes negative when `adj > amt` | clamp the credit at 0 | **on** |
| `refund-repeat-compounding` | `adj` written back over the record, so repeats compound | leave the record alone | **on** |
| `caravan-heuristic-goal-y` | `pf_dist(dx, −goalY)` at `0x00685FD9`; mode A ≈ Dijkstra | pass `child.y − goalY` | off |
| `refinery-bonus-dead` | `CityData::refinery` stored as literal 0 | apply `REFINERY_BONUS` | off |
| `env-patrol-execution` | orders 17/22 route, queue, execute, and cross retail airframe/search boundaries | *drift*: exact dynamic queue and patrol transitions; env air physics/target-search host remains incomplete | — |
| `ai-model-simplifications` | full dynamics | *drift*: six numbered model gaps in `don-ai` | — |
| `arena-construction-model` | retail build-site/worker state | *drift*: MODEL 2 builder-frame construction | — |
| `arena-gather-model` | retail resource objects/occupancy | *drift*: MODEL 3 inferred gather state | — |
| `arena-target-acquisition-model` | complete retail spatial target scan | *drift*: MODEL 4 incomplete host | — |
| `arena-guy-turret-model` | graphics-turret Guy materialization | *drift*: exact MODEL 5 path hard-gates unresolved turret types | — |
| `arena-water-model` | water terrain/regions and water A* | *drift*: MODEL 6a absent host | — |
| `arena-naval-model` | naval objects, orders, queues and supply | *drift*: MODEL 6b absent host | — |
| `arena-air-model` | air physics, orders, ammo RNG and walked state | *drift*: MODEL 6c local adapters only | — |
| `arena-diplomacy-model` | declaration state and all set/strategy side effects | *drift*: MODEL 6d relation adapter only | — |
| `arena-attrition-model` | per-unit period/recompute/damage state | *drift*: MODEL 6e arithmetic adapter only | — |
| `arena-supply-model` | source/building queries, reload and healing | *drift*: MODEL 6f predicate adapter only | — |
| `attack-dir-semantics` | attacker→target bearing | *rejected*: retail is right | — |
| `gather-enhancer-table-base` | tables indexed `level − 1` | *rejected*: deliberate, 1-based accessors | — |

Two of the nine fixes ship **off even in improved mode**, and that is a deliberate part of
the design rather than a hedge. `caravan-heuristic-goal-y` changes *which* path is found and
its effect on unit behaviour is unmeasured; `refinery-bonus-dead` is a balance change and
balance changes need an argument, not a toggle. `default_in_improved: false` is the honest
state for "we found it, we can fix it, we have not earned the right to ship it".

The seeded entries came from the brief plus a sweep of `docs/mechanics/*.md` and
`docs/derivation/*.md`; the four the sweep added are `bhs-citizens-typo`,
`caravan-heuristic-goal-y`, `refinery-bonus-dead` and `gather-enhancer-table-base`.

---

## 3. Two of the brief's candidates are not deviations

The lane brief listed `attack_dir` "semantics that do not match the name" as a measured
candidate. **It matches.** `docs/assembly/target-selection.md` settled it after the brief
was written: `Unit::fight` computes `attack_dir = find_angle(target − attacker)`, the
direction the attack *travels*, traced in capstone at `0x005FE872..0x005FE89B`. The arcs
then read exactly as `rules.xml`'s own comment describes — front ±60° no bonus, sides 2×75°
tier 2, rear ±45° tier 1, sides worth twice the rear. "Fixing" it would make a frontal
charge earn the flanking bonus. It is registered as `Rejected` so the next lane to read the
raw arc table does not re-open it.

Similarly, the gather-enhancer table bases *look* like an off-by-one and are not; two lanes
have now found them independently and both were briefly tempted. Registered as `Rejected`
with both citations.

Recording these two as entries rather than deleting them is the point: the registry's job is
to be the place a question gets asked once.

---

## 4. What was verified, and how

* `cargo test -p don-sim` — **968 lib** (22 of them this module's) **+ 5 integration + 1 doc
  test**, 0 failed. `cargo check --workspace --all-targets` clean.
  `cargo test -p don-replay -p don-ai -p don-env -p don-sim` — all green, so nothing
  downstream of the `lib.rs` and `Cargo.toml` additions moved. The only failing crate in the
  workspace is `don-bhs-cc`, which does not depend on `don-sim` and whose failure count
  changed between two consecutive runs — it is being edited by a live lane.
* **The fidelity gate was mutation-tested**, because a gate nobody has watched fail is not
  known to bite:
  * mutating `ModeConfig::default()` to return `improved()` → 2 of 5 integration tests fail
    (`no_registry_entry_is_active_in_fidelity_mode`, `fidelity_cannot_be_switched_at_runtime`).
  * deleting the fidelity check inside `enable()` → 2 of 5 fail
    (`fidelity_cannot_be_switched_at_runtime`,
    `the_environment_cannot_smuggle_a_fix_into_a_fidelity_run`).
  * Both mutations reverted; suite green after.
* **The harness gate was exercised end to end.** `DON_MODE=improved tools/replay-validate.sh
  --limit 1` → exit 3, naming all seven active entries, **before** the validator is built,
  and `schema/replay-validation.json` is not written. `DON_DEVIATIONS=+tikal-border-rule-slot`
  with no `DON_MODE` → also exit 3, because a `+` in fidelity mode is an error rather than a
  silent no-op.
* `every_seam_produces_retail_behaviour_in_fidelity_mode` checks each seam's retail branch
  against the value its derivation document records — `[-35,-15,-7,0,25,50]`, `-1` invisible
  to `== 0`, `"Citizens"` passed through, `+0x4A0` read for Tikal, a `−10` refund credit,
  `−goalY` regardless of the node, a `0` refinery. A mode system that is correctly inert but
  that nothing consults would pass every other test in this lane; this is the one that would
  not.
* `the_changelog_covers_every_registry_entry` reads `docs/tracks/deviations.md` and fails if
  a slug is missing from it. The document cannot silently fall behind the code.

**What is not verified.** Every seam is tested against *our derivation documents*, not
against retail machine code — no oracle case was added, so nothing here moves a fidelity
tier. The percentage table itself is Tier C (decompiled). The 2.29× spread is `[measured]`
in our own runner, which is a measurement of our port, not of the shipped game.

---

## 5. What is wired, and what is not — the honest part

The seven fixes enabled by default in improved mode are now wired through their real
subsystems, with the fidelity branch preserved beside each improvement:

| subsystem | wired deviations | mode proof |
|---|---|---|
| `systems::economy::do_gather` | `ai-gather-handicap`, `gather-handicap-truncation` | retail keeps the caller's percentage and C truncation; improved removes the cheat, or independently rounds when the ladder is re-enabled |
| `don-ai::economic` through `ScriptWorld::mode_config` | `bhs-prereq-result-test`, `bhs-citizens-typo` | fidelity sends `"Citizens"` and ignores `-1`; improved sends `"Citizen"` and treats every non-positive placement as failure |
| `systems::borders_fog::leader_border_params` | `tikal-border-rule-slot` | fidelity reads the separate HP value; improved reads the named borders value |
| `systems::production::refund_cost` | `refund-charges-player`, `refund-repeat-compounding` | fidelity returns the negative credit and rewritten amount; improved returns zero credit and preserves the paid amount |

`Game` and the simulation contexts own a `ModeConfig`; there is no process-global mode.
The old `apply_difficulty_bonus` boolean was removed, eliminating two switches for one
behaviour. Each subsystem test drives both modes through the public runtime function rather
than merely unit-testing the registry helper.

The two optional, improved-default-off candidates remain honestly `Unwired`:
`caravan-heuristic-goal-y` awaits the caravan-road A* port, and `refinery-bonus-dead`
awaits a measured balance decision and its city-gather call site. Because neither is active,
they do not masquerade as shipped improvements and do not block the canonical improved
edition. Product readiness is now blocked only by registered product drift.

### Where `ModeConfig` should live

It is `Copy`, 8 bytes, and depends on nothing. The natural home is a field on `World` (and
on `don-ai`'s `Game`), set once at construction and read by the seams — never a global, and
never a process-wide mutable default, because a global is precisely how a fidelity run
inherits somebody else's mode. `ModeConfig::from_env()` exists for the *client* to call at
startup; harnesses should construct `ModeConfig::fidelity()` explicitly.

---

## 6. Open questions this lane surfaced

1. **`Build::refund_cost`'s divisor is not guarded in our port the way retail behaves.**
   `production.rs::refund_amount` returns 0 when `100 − TECH_SCIENCE_DISCOUNT * d == 0`;
   retail has a real `idiv` there. Whether retail faults, or the state is unreachable, is
   **not established** — so it is not a registry entry. It is a Ghidra-visible divergence
   waiting for a measurement, and the answer decides whether it becomes a `Drift` entry.
2. **How many more `Drift` entries are there?** Two were found by reading `COVERAGE.md` and
   `don-ai`'s own headers. Nobody has swept the tree for undeclared divergence, and the
   entries most likely to exist are the ones no lane wrote down.
3. **Does improved mode need its own validation corpus?** Fidelity mode is measured against
   real replays. Improved mode has no external oracle by construction, so its regressions can
   only be caught by self-play stability and by the fidelity suite continuing to pass with
   every entry off. That asymmetry is inherent, and worth naming before someone reports an
   improved-mode number as if it had been validated.
4. **The `no-rush` gate on the human handicap is still an input boundary.** Retail gives
   humans 0 unless the no-rush option is set. The wired payout seam therefore accepts the
   caller's already-gated retail percentage rather than reconstructing it from difficulty;
   improved mode can zero that value without ever applying the AI table to a human. The
   upstream game-option predicate still needs derivation before a world can synthesize the
   percentage rather than reading it from leader state.

---

## Files this lane wrote

* `crates/don-sim/src/deviations.rs` — the mode system and the registry
* `crates/don-sim/src/bin/deviations.rs` — `don-deviations`
* `crates/don-sim/tests/fidelity_mode.rs` — the gate
* `docs/tracks/deviations.md` — the human-readable changelog
* `docs/tracks/dual-mode.md` — this report

and three surgical additions to shared files: one `pub mod` line in
`crates/don-sim/src/lib.rs`, one `[[bin]]` block in `crates/don-sim/Cargo.toml`, and the
five-line fidelity gate in `tools/replay-validate.sh`.
