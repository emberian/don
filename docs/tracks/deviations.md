# What our edition does differently, and why

*Descent of Nations* is a reimplementation of *Rise of Nations: Extended Edition* built by
reading the shipped binary rather than the community wiki. That gives it an unusual
property: we can tell you, to the instruction, what the original does — and therefore we
can tell you exactly where we depart from it, and refuse to depart anywhere else.

There are two editions, and they are the same program.

**Fidelity mode** reproduces retail, including its bugs. It is the default, it is what the
replay-validation harness measures against real recorded games, and nothing on this page is
switched on in it. If we ever claim "our simulation matched a real game for N turns", that
number was produced here.

**Improved mode** is the edition you play. Everything below is a decision — named, argued,
individually revocable, and traceable to the address in the retail binary where we read the
original behaviour.

Nothing on this page was inferred from a wiki. Every entry cites where it was measured.

```sh
cargo run -p don-sim --bin don-deviations                    # the registry, as it stands
cargo run -p don-sim --bin don-deviations -- --mode improved # what our edition turns on
cargo run -p don-sim --bin don-deviations -- --show ai-gather-handicap
```

---

## The short version

| | what the original does | what we do |
|---|---|---|
| **`ai-gather-handicap`** | The difficulty slider is an income cheat: the AI's gather rate is scaled −35 % to +50 % by difficulty | The AI gets no income bonus at any difficulty. Difficulty is how well it plays |
| **`gather-handicap-truncation`** | That scaling truncates, so low difficulties are quietly harsher than the number says | Round to nearest, if you keep the ladder at all |
| **`bhs-prereq-result-test`** | The shipped economic AI script can lock up forever trying to build a University it has no tech for | It notices the refusal and moves on |
| **`bhs-citizens-typo`** | Five steps of the shipped opening train `"Citizens"`, a unit type that does not exist | They train `Citizen`, and the opening works |
| **`tikal-border-rule-slot`** | Tikal's border bonus is wired to `TIKAL_TEMPLE_HP`; editing `TIKAL_TEMPLE_BORDERS` does nothing | Borders read the borders constant |
| **`refund-charges-player`** | Cancelling a queued item can *take* resources from you | Cancelling never bills you |
| **`refund-repeat-compounding`** | …and it rewrites its own record, so cancelling twice charges more | The record is left alone |
| **`caravan-heuristic-goal-y`** | The caravan pathfinder's heuristic ignores the node's own `y`, degenerating to Dijkstra | Available, off by default — see below |
| **`refinery-bonus-dead`** | `REFINERY_BONUS` is parsed from the rules and then never used | Available, off by default — see below |

Twelve further entries record divergences in **our** code that nobody chose, and two record
candidates we investigated and rejected. Both kinds are at the bottom, because a register
that only lists the flattering entries is not a register.

---

## The fixes

### `ai-gather-handicap` — the difficulty slider is an income cheat

**Retail.** `LeaderData::get_gather_handicap` (`0x006D66A0`) returns a signed percentage
chosen by difficulty — Easiest −35, Easy −15, Moderate −7, Tough 0, Tougher +25,
Toughest +50 — and `Leader::do_gather` (`0x006CE450`) applies it to every resource every
frame as `income = (100 + pct) * income / 100`. It is applied *after* the displayed gather
rate has been cached (`0x006CE723` precedes `0x006CE72C`), so it never appears in the AI's
own economy readout.

**What that is worth.** We ran six identical starting towns with the AI *switched off
entirely*, so the only difference between rows was this one function:

```
difficulty  bonus   food  timber   food ratio
Easiest      -35    1439     360       0.6485
Easy         -15    1859     480       0.8378
Moderate      -7    2039     540       0.9189
Tough          0    2219     600       1.0000
Tougher      +25    2759     720       1.2434
Toughest     +50    3299     900       1.4867
```

Toughest gathers 1.49× what Tough gathers and Easiest 0.65×: a **2.29× spread** end to end,
on the same buildings, with the same workers, playing the same way. Beating Toughest means
beating a richer opponent, not a better one.

**Ours.** The handicap is zero at every difficulty. Difficulty has to come from the
opponent playing better, which is the whole point of the project's AI work — and it is the
only honest basis for an RL agent, which otherwise learns to beat an economy bonus.

*Measured in `docs/tracks/ron-ai-impl.md` §2 and `docs/mechanics/economy.md` §4.5. The
percentage table is Tier C (read from the decompilation, not yet oracle-checked); the 2.29×
spread is measured in our own runner.*

### `gather-handicap-truncation` — the penalty is worse than it reads

`income * (100 + pct) / 100` is an integer divide truncating toward zero. At a negative
handicap the leftover fraction is *always* discarded, so the penalty is strictly worse than
the table promises; at a positive one the bonus is merely rounded down. On the measured
timber column, whose per-frame income is a single digit, Easiest lands on 0.60× against a
nominal 0.65×, and Easy on 0.80× against 0.85×. The error is largest when income is
smallest — the opening, which is exactly when a low difficulty is supposed to be gentle.

Ours rounds to nearest. This entry is inert while `ai-gather-handicap` is on, because a 0 %
handicap divides exactly; it exists for anyone who wants the ladder back without its bias.

*It fell out of running the derived code. Hand-computing the expected ratios would have
hidden it — which is why this project does not hand-compute expected values.*

### `bhs-prereq-result-test` — the shipped AI can lock up forever

**Retail.** The script host returns a tri-state: `>0` done, `0` refused but affordable,
`-1` invalid — and an unmet prerequisite returns `-1`. Step 20 of the shipped
`economic.bhs` places a University, whose `PREQ0` is Classical Age, *before holding
Classical Age*, and tests the result with `== 0` (`economic.bhs:638` and `:642`). So `-1`
falls straight through, the step sets `old_step = 0`, and it retries the illegal placement
forever — and `old_step = 0` is precisely the signal that stops the 300-second hang
watchdog from arming. The script has disarmed its own safety net.

We watched this produce **657 invalid orders for the Greeks in one 30-minute match**, all
from that single call. In retail it is survivable only because of an every-30-ticks
age-tech fast lane in `Leader::plan_strategy` that sits outside the production cycle and
eventually hands the script the tech it is waiting for.

**Ours.** Any result `≤ 0` is a failed placement, so the script yields its cycle exactly as
it does for an ordinary refusal.

*`docs/tracks/ron-ai-impl.md` §4.3. A shipped script bug still present 23 years later.*

### `bhs-citizens-typo` — five opening steps train a unit that does not exist

`economic.bhs` calls `train_unit_with_need(who, needed_citizens, "Citizens")` at lines 475,
510, 568, 674 and 762. The unit type is `Citizen`. `"Citizens"` is absent,
case-sensitively, from both shipped data files, so those five steps of the shipped opening
silently do nothing.

It is partly masked, which is why it survived: the maintenance call at `economic.bhs:297`
uses the correct singular with the *same* `needed_citizens` value, so the next script
invocation trains what the previous one asked for. The opening still gets its citizens a
cycle late, every single time.

Ours resolves the name at lookup. *`docs/tracks/AUDIT-frontier.md` V17.*

### `tikal-border-rule-slot` — a wonder wired to the wrong constant

Under `has_wonder(0x214)` the border scorer loads `[rules + 0x4A0]` at `0x006B0DC9`. XML
declaration order puts `TIKAL_TEMPLE_BORDERS` at `+0x498`, `TIKAL_TEMPLE_RANGE` at `+0x49C`
and `TIKAL_TEMPLE_HP` at `+0x4A0`. Tikal's *border* bonus is reading its *hit points*
constant.

Both ship as `50%`, so no shipped number moves and no retail replay can tell the
difference. It matters for anyone who edits the rules: changing `TIKAL_TEMPLE_BORDERS` has
no effect, and changing `TIKAL_TEMPLE_HP` silently changes borders. The Workshop mod
library is one of this project's goals, and a rules file whose names lie to you is a bad
foundation for it.

*`docs/mechanics/borders-fog.md` §4.2(ii), measured at the instruction level.*

### `refund-charges-player` — cancelling can cost you resources

`Build::refund_cost` (`0x00620490`) age-adjusts the amount you paid and then credits
`stockpile += amt − adj`:

```
d   = player_age − type_age
adj = amt * 100 / (100 − TECH_SCIENCE_DISCOUNT * d)
adj = adj + (d + 1) * TECH_SCIENCE_DISCOUNT * adj / 100
stockpile += amt − adj
```

With the shipped `TECH_SCIENCE_DISCOUNT = 10` and **zero ages elapsed**, `amt = 100` gives
`adj = 110` and a credit of **−10**. Cancelling a queued item takes resources away from
you, at the same age, and more as ages pass. There is no message; the number just goes
down.

Ours clamps the credit at zero: cancelling can refund nothing, but it can never bill you.

### `refund-repeat-compounding` — and it gets worse each time

The same function writes `adj` back over the record's amount, so re-entering through
`Build::action_unqueue` (`0x00620280`) adjusts an already-adjusted amount and the charge
grows. Separate entry, separately revocable, because it is a separate mistake: one is a
sign error, the other is state.

*Both from `docs/mechanics/production.md` §3.6. The retail arithmetic is reproduced
literally in `crates/don-sim/src/systems/production.rs::refund_amount`; this registry owns
only what is done with the result.*

### `caravan-heuristic-goal-y` — available, and deliberately off

At `0x00685FD9` the caravan A\*'s mode-A child heuristic is `pf_dist(child.x − goalX,
−goalY)` — `neg edx`, checked in raw bytes because the two shift amounts nearby differ by
one and that is exactly what a decompiler smooths over. `child.y − goalY` was plainly
intended. Since `pf_dist` takes the absolute value of both arguments, `h` is a large
per-search constant plus a term of order `dx²/goalY`, and a constant added to every node's
`h` does not change A\* ordering. **Mode A is, to within a negligible term, plain
Dijkstra.**

Fixing it is a real heuristic instead of an accidental one, and fewer node expansions for
the same path. It is **off by default even in improved mode**, because it changes which
path is found and not merely how fast it is found, and we have not measured the effect on
unit behaviour. Turn it on with a measurement in hand.

*`docs/derivation/pathfinding.md`. Measured that the instructions are these; the inference
that it is a mistake is ours.*

### `refinery-bonus-dead` — available, and deliberately off

`City::calc_gather` (`0x00737C60`) writes the granary, lumber-mill and smelter enhancers
from their rule tables and stores `CityData::refinery` as a **literal 0** in every path.
`REFINERY_BONUS = 33%` is parsed out of the rules and is dead in the city path.

We do not know whether that is a bug or an unannounced balance decision — a Refinery may
well be intended to earn its keep somewhere else. So it is registered, so the question gets
asked out loud, and it is **off by default**: balance changes need a measurement and an
argument, not a toggle.

*`docs/mechanics/tech-cities.md` §3.1, which records it as a fact and explicitly not as a
bug to work around.*

---

## Balance, and why it is not here

Retuning `rules.xml` is not a deviation. Rules values are data; the tokenizer and all 719
constants are derived, and swapping a number is a data change that both modes read
identically. This register is for places where the *code* behaves differently from retail
given the same data. If we ever ship a rebalanced data set, it gets its own document and
its own argument.

Likewise, a performance choice that produces identical bits is not a deviation. The SIMD
kernels are asserted bit-identical to their scalar references; that is engineering, not
divergence.

---

## Drift — divergence nobody chose

These are in our code today, in **both** modes. They are not features and they are not
toggleable. Each product drift names the execution surface it can reach, and the readiness
gate refuses to launch a claim-bearing run on that surface. Research-only drift is labelled
as such: it remains usable as a bounded experiment, but cannot be promoted to a product or
fidelity claim.

### `env-patrol-execution`

The command-to-order path and queue ownership are now exact. Opcode 10 installs
`GROUP_PATROL` (22) for ground units and helicopters, but follows retail's
`Group::action_patrol` branch to `AIR_PATROL` (17) for true planes. Opcode 11 installs
`AIR_PATROL` only for true planes. The mask uses retail's `UnitData::is_plane` predicate
(`AIR` domain and no `FLAGS f`), not the broader air-domain capability, and
`OrderIndex::PATROL` (5) remains the dead arm. The environment now retains dynamic
waypoint arrays and the real front/current queue shape: ground `QUEUE_FIRST` replaces,
compatible `QUEUE_LAST` extends with the raw command coordinate, air `QUEUE_LAST` extends
only a compatible active patrol, and the true-plane installer replaces otherwise.

Both recovered executor transitions are reachable. `GROUP_PATROL` advances before reading
its waypoint and inserts the exact `ATTACK_TO` body ahead of itself, then resumes when that
leg retires. `AIR_PATROL` retains its cursor and post-physics retirement rules. The
remaining drift is narrower but still product-blocking: EnvWorld has not ported
`Unit::do_air_physics`, and its mod-16/mod-32 air/building target-search callbacks currently
produce no target. Its existing explicitly scaffolded mover serves that host boundary; it
is not a retail airframe implementation. RL readiness stays blocked until those adjacent
air systems are derived and wired.
*`docs/mechanics/COVERAGE.md` §3.1; `docs/assembly/command-bridge.md`.*

### `ai-model-simplifications`

`crates/don-ai/src/game.rs` is a research harness, and it marks six numbered `DEVIATION`s
at their sites: placement never fails for spatial reasons, worker slots come from a table
rather than `BuildData::gather_max`, gather targets come from a table rather than the map,
construction assumes a builder is present, there is no combat or diplomacy, and the ten
compiled production stages are stubs. Indexed here so `don-ai` output is never mistaken for
a fidelity result. This particular runner is explicitly **research-only**, so its incomplete
systems do not falsely block unrelated replay, playable, or RL entrypoints.
*`docs/tracks/ron-ai-impl.md` §3.*

### Arena MODEL 2–6 blockers

The former aggregate `arena-model-simplifications` entry is split so one successful adapter
cannot silently clear unrelated product gaps. All ten entries below independently block the
playable surface while their runtime path remains incomplete:

| registry slug | declared model | literal remaining system |
|---|---:|---|
| `arena-construction-model` | 2 | build-site, builder-order, interruption and completion state |
| `arena-gather-model` | 3 | resource-object ownership, capacity, occupancy and depletion |
| `arena-target-acquisition-model` | 4 | complete stable spatial scan with diplomacy, fog, validity, region and priority gates |
| `arena-guy-turret-model` | post-5 prerequisite | graphics-turret Guy materialization and state |
| `arena-water-model` | 6a | water generation/regions plus tile/water A* domains |
| `arena-naval-model` | 6b | exact water path, dock/queue, boarding, containment, fishing, territory and supply runtime |
| `arena-air-model` | 6c | `do_air_physics`, target/host scans, Ammo RNG insertion, orders and walked state |
| `arena-diplomacy-model` | 6d | declaration command plus retargeting, shared vision, event/chat and strategy side effects |
| `arena-attrition-model` | 6e | per-unit period state/recomputation and exact fractional damage host |
| `arena-supply-model` | 6f | `Supplies::find_supply`, building scans, reload call site and healing |

The former greedy-movement model has already been replaced by the derived retail A*
pathfinder and is not on this list. `arena::retail_systems::MODEL6_INVENTORY` is the
executable integration inventory: it maps recovered `don-sim` kernels to the prerequisites
still missing. Its air, diplomacy, attrition and supply adapters are deliberately fail-closed:
they require live type-table fields, the caller's main RNG, explicit bilateral declaration
state, and explicit world-query results. They do not spawn an aircraft, pretend every unit is
supplied, or promote `naval::*_proxy` functions into product behavior.

MODEL 5 itself is no longer a blocker. The arena now calls the exact direct-land volley
planner, `target::attack_dir`/`flank_tier`, live Guy marks and per-Guy destination angles,
with retail one-byte cadence. The focused don-sim fight suite and arena combat suite are
green. Types carrying `GUY_FLAG_TURRETS` remain hard-rejected because their graphics-turret
Guys are not materialized; that honest roster gap is the separate
`arena-guy-turret-model` entry above, not an excuse to retain or reintroduce a flank heuristic.

Unlike `ai-model-simplifications`, these are **playable-product blockers** because the arena
feeds head-to-head matches and the WebGPU client. Clear an entry only after its real arena
tick/command path and focused tests are wired; an isolated adapter is necessary integration
work, not completion. *Self-declared in `crates/don-ai/src/arena/world.rs`; subsystem evidence
is listed in the registry and `docs/mechanics/{air,naval,borders-fog}.md`.*

---

## Rejected — investigated, and not a deviation

Recorded so nobody rediscovers them and "fixes" something that is right.

### `attack-dir-semantics`

Carried for a while as a suspected mis-named argument: with the defender facing angle 0, an
`attack_dir` of 0 scores flank tier 1 and a half turn away scores tier 0, which reads
backwards. It is not. `Unit::fight` computes `attack_dir = find_angle(target.x −
attacker.x, target.y − attacker.y)` (`0x005FE872`–`0x005FE89B`): the direction the attack
*travels*. So `attack_dir == defender_facing` means the attack is going the way the
defender is looking — the attacker is behind it. Substituting `bearing = attack_dir − half
turn` gives the arcs you would expect:

| attacker's bearing from the defender's nose | width | tier | infantry bonus |
|---|---|---|---|
| within ±60° of dead ahead | 120° | none | +0 % |
| 60°–135° off, either side | 2 × 75° | 2 | +100 % |
| within ±45° of dead astern | 90° | 1 | +50 % |

Sides worth twice the rear, front worth nothing — exactly what `rules.xml`'s own "max bonus
is twice this number" comment describes. "Fixing" it would make a frontal charge earn the
flanking bonus. *`docs/assembly/target-selection.md`.*

### `gather-enhancer-table-base`

`City::calc_gather` pushes base `+0x2B8` against a `granary_bonus[5]` that starts at
`+700`, and `SCHOLAR_RATE` is read as `RULES[0x284 + (level−1)*4]`. Two lanes found this
independently and both were briefly tempted to call it an off-by-one. It is not: the level
accessors are 1-based, 0 means "no such building" and is short-circuited to a zero bonus by
the building flags. A port that indexes by `level` reads the next tier's number for every
enhancer building in the game. *`docs/mechanics/tech-cities.md` §3.1 and
`docs/mechanics/economy.md` §4.4 — two independent confirmations.*

---

## How to be sure this page is true

The registry is code — `crates/don-sim/src/deviations.rs` — and this page is checked
against it. `crates/don-sim/tests/fidelity_mode.rs` fails if a fix can activate in fidelity
mode, if entry metadata is inconsistent, if any entry lacks its derivation, or if any entry
is missing from this document. `don-deviations --assert-ready <surface>` then rejects
reachable known drift and active-but-unwired improvements. `tools/replay-validate.sh` uses
the `replay` surface before it may write a scoreboard; playable and RL release checks use
`playable`, `rl-env`, or the aggregate `product` target. Research-only gaps are deliberately
outside those surfaces.

The design, the seams, and what is still unwired are in `docs/tracks/dual-mode.md`.
