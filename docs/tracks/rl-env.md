# Track: `rl-env` — the reinforcement-learning environment surface

**Status: working, with two explicit fidelity tiers.** `crates/don-env` + `python/don_env`
build, import, step, and mask through the compact `VecEnv` backend.
A 256-env batch runs **77,166 env-steps/s** in `step()` alone and **53,216 env-steps/s**
end to end including masked action sampling, on this M2 Max, measured by
`python/smoke_test.py`. Full numbers and the honest caveats are below.

The additive `AuthoritativeBackend` instead owns `don_sim::tick::Sim` directly. It is a
bounded migration surface, not yet a full-game vector environment: currently only NOOP and
fully hosted MOVE_TO are admitted. A fresh current-frame capture can now expose complete
non-cloaked external rows and bind ATTACK's one-based target ordinal through stable
`(Handle,who,o,uid)` identity plus the recovered fog predicate. ATTACK remains masked after
that preflight: the walked order and DoNSave v7 now retain target UID/Handle plus the prepared
visibility/episode revisions and hostile eligibility. The production tick consumes the exact
identity, and the RL preflight now proves the current executor's type/balance/damage inputs, but
production does not yet atomically consume that proof under authoritative Step-12 freshness. The
admitted verb delta is therefore **0**; the other policy verbs still fail at typed owner boundaries.

Root convergence validated the target-identity tranche in persvati job
`rl-target-entity-v2-20260809T232312Z-24206-11858-b8a284f806b0`: 22/22 focused backend, head,
scenario-source, target, and backend-selector tests passed. The first run exposed one older test
still expecting the generic `Unhosted` refusal; it was narrowed to the new exact visibility
refusal without weakening its digest/no-fallback assertion. No action-coverage increase is
claimed.

### Side-by-side authoritative migration contract

`EnvironmentBackend::compact(...)` constructs the unchanged batched `VecEnv`.
`EnvironmentBackend::authoritative(scenario)` constructs one deterministic `Sim` episode.
The enum deliberately has no common `step` method: selecting a backend does not imply that
their action, observation, reward, or fidelity contracts are interchangeable.

The authoritative contract is frozen at this boundary:

* `ScenarioSpec` completely determines seed, player activation, allocation identity, and
  initial unit state. `reset()` reconstructs that image; `step_frames(n)` executes exactly
  `n` retail-ordered `Sim::do_frame` calls and returns per-stage reachability counts.
* Unit/player NOOP is observational. MOVE_TO is the only non-NOOP route and reaches
  `Sim::issue` only after ownership, queue/flag, map, and live movement-host preflight. Its
  Sim-owned source state is revision-checked and changed to `{moving: true,
  action: MoveTo|FleeTo}` immediately before issue; stale prepared actions refuse without
  changing source, order, or path state. An episode revision also prevents deterministic reset
  from reviving a plan whose handle and fresh source revision happen to repeat.
  Every other generated unit/player verb returns `ApplyRefusal::Unhosted` with the missing
  authoritative owner and cannot mutate `Sim`.
* `observe()` and `reward_snapshot()` project directly from `Sim`. Observation contains own
  units and, after `capture_external_visibility()`, a stable one-based external row image.
  Capture refuses zero-filled default type flags and any viewer without an explicit captured
  `LeaderData::ally_mask`; it snapshots the Sim-owned object columns, fog/detection planes,
  territory and viewer policy, and invalidates on tick, reset, or order mutation.
  A validated retained fog-option source may select retail option 3 for current-fog visibility
  without pretending that Step 12 stamped either visibility plane.
  `external_entities_complete` stays false without that fresh image. The step-12
  detector producer still hardcodes `detector=false`, so any cloak requiring detection refuses
  capture instead of presenting an incomplete row image. Reward deltas use Sim-owned score,
  economy, alive, and won state.
* Strict ten-head decoding preserves `TargetEntity` instead of dropping it. ATTACK binds a
  non-zero ordinal only against the exact captured image shown to the policy, then revalidates
  the target's Handle generation and retail `(who,o,uid)` against the live Sim row. A prepared
  token retains those facts, both owner revisions, the visibility frame/ordinal, and the exact
  hostile relation; its non-lossy order survives queue conversion and save/load. No image is
  `TargetIdentityVisibilityUnavailable`; a missing/stale ordinal is `TargetVisibility`; a valid
  visible identity reaches `AttackTargetCommitUnavailable`. All three remain masked and change
  no order, path, digest, or reset state. Scenario allocation order and omniscient World
  traversal are forbidden substitutes. This tranche adds **zero** authoritative action
  coverage by design.
* `AuthoritativeScenarioSpec` captures movement sources by deterministic scenario-unit
  ordinal and reinstalls them atomically on reset. `install_movement_source()` remains an
  explicitly out-of-band setup escape hatch and is not silently persisted. Masks read the
  Sim-owned source snapshot and retain no hidden sidecar.

Migration proceeds by moving one complete transaction at a time behind the authoritative
variant: next group/command decoding, then visibility and opponent observations, then
vectorisation. Compact `EnvWorld` remains available for
throughput comparison; it is not incrementally copied into `Sim` and does not become a
second source of truth for the authoritative variant.

---

## 1. What now works, and how it was measured

```sh
python3 crates/don-env/gen/gen_spec.py     # regenerate spec + capability table
cargo test -p don-env                      # compact + authoritative contracts
bash python/build.sh                       # -> python/don_env/_don_env.so
PYTHONPATH=python python3 python/smoke_test.py --envs 256
```

| capability | evidence |
|---|---|
| Factored `MultiDiscrete` action space, 10 unit heads + 5 player heads | `spec.rs`, printed by the smoke test |
| 33 unit verbs + 16 player verbs, an exact partition of the engine's 82 opcodes | `tests::every_opcode_is_classified_exactly_once` |
| Per-parameter masks, bit-packed, derived from shipped rules data | `tests::no_mask_head_is_ever_all_zero` |
| Retail patrol command routing, including plane vs helicopter | `command_bridge_agreement::env_patrol_routing_uses_retail_is_plane` |
| Dynamic patrol queue positions and reachable ground/air executor transitions | `command_bridge_agreement::env_patrol_queue_and_executor_preserve_retail_transitions` |
| Masked sampling never produces an illegal action | `tests::masked_sampling_produces_no_illegal_actions` — 0 illegal in ~486 k applied actions over the smoke test |
| Zero-copy observations (numpy views over Rust memory) | smoke test: `entities.flags.owndata == False`, pointer stable across `step`, contents change in place |
| Batch parallelism is bit-deterministic in thread count | `tests::stepping_is_deterministic_across_thread_counts` |
| Gymnasium `VectorEnv` + PettingZoo `ParallelEnv` front ends | smoke test exercises all three |
| Engine score components exposed, shaping pluggable | smoke test prints all 11 `LeaderData` fields and sets weights |
| Whole workspace still green | `cargo test --workspace --exclude don-net --exclude don-ai` |

### Throughput [measured]

Apple M2 Max, 12 cores, release build, `grid 64×64`, `max_controlled=32`,
`max_entities=64`, 2 agents, `frames_per_step=1`, 100 steps after 20 warmup.

| envs | env-steps/s (`step` only) | env-steps/s (whole loop) | real-time factor |
|---:|---:|---:|---:|
| 16 | 26,344 | 18,291 | 1,226× |
| 64 | 60,758 | 39,358 | 2,637× |
| 256 | 77,166 | 53,216 | 3,565× |
| 1024 | 98,505 | 69,489 | 4,656× |

*Real-time factor* uses the **measured** 67 ms Normal-speed tick
(`TurnControl::timings` `0x00AFC4A4`), not the folkloric 15 Hz. At 8 agents × 256 envs the
figure is 13,041 env-steps/s = **104,331 agent-steps/s**.

Two more numbers that matter more than the headline:

* **NOOP actions, 64 envs: 57,207 env-steps/s** vs 60,758 with random masked actions. So
  applying actions is nearly free; the cost is observation and mask writing.
* **numpy mask sampling instead of native: 607 env-steps/s** (9.79 s of sampling vs 0.15 s
  native, at 64 envs). Unpacking the 806-wide `Type` head in numpy costs **65×** the
  environment step. That measurement is why the masks are handed to Python bit-packed and
  why `sample_masked_actions_native()` exists — a naive numpy reference sampler would have
  made every reported steps/s a measurement of `np.unpackbits`.

---

## 2. The action space, and where each piece came from

The brief said *derived, not invented*. Concretely:

**The verb set is the engine's own opcode table.** `schema/command-wire.json` has 82
opcodes; the PDB's `CommandTypes` enum names all 82 and confirms `NUM_COMMANDTYPES = 82`.
Each opcode is classified exactly once, and `gen_spec.py` asserts the partition:

| group | n | examples |
|---|---:|---|
| `UNIT` — directed at owned entities | 33 | `MOVE_TO`, `ATTACK`, `BUILD`, `QUEUE_UP`, `GARRISON`, `SPELL` |
| `PLAYER` — one per actor per step | 16 | `TREATY`, `DECLARE`, `TRIBUTE`, `BUY`, `SELL`, `RESIGN` |
| `SELECTION` — expressed by the (entity, action) format itself | 2 | `GROUP`, `HOTKEY` |
| `UI` — no simulation effect | 8 | `PING`, `CAMERA`, `CHAT`, `RENAME_CITY` |
| `ADMIN` — lockstep/session protocol | 14 | `BEGIN`, `CHECK_SUMS`, `CHECK_RANDOM`, `PAUSE` |
| `CHEAT` | 9 | `CHEAT_GIVE_TECHS`, … |

**The head assignment is read off the wire layout.** `gen_spec.py` walks the field list of
every agent-emittable command and maps each field name to a head (`to_x`→`TargetX`,
`ox`/`whom`→`TargetEntity`, `type`→`Type`, `queued`→`QueuePos`, `stance`→`Stance`,
`form`/`rotate`→`Form`, `orders`→`OrderMods`, `good`→`Good`, …). Any field the generator
cannot classify is a hard error, so the table cannot silently drift.

**Head sizes are engine constants, not round numbers.** `Type` is 806 because
`TypeIndex::NUM_TYPES` is 806. `QueuePos` is 3 because the `QueuePos` enum has three
members. `Stance` is 4 from `StanceTypes`. `Form` is 10 from `FormIndex::NUM_FORM_ALL`.
`Treaty` is 3 from the `WAR/PEACE/ALLY` enum. `TargetPlayer` is 8 because every
`LeaderData` diplomacy array is `int[8]`.

```
unit  : Verb 34 | TargetX W | TargetY H | TargetEntity N+1 | Type 806
        | QueuePos 3 | Stance 4 | Form 10 | OrderMods 8 | Count 5
player: Verb 17 | TargetPlayer 8 | Good 6 | Amount 8 | Treaty 3
```

Not flattened, deliberately: the product is ~10^13, and worse, flattening destroys the
parameter sharing between `Attack(target=i)` and `Attack(target=j)` that makes the space
learnable at all.

**Fields no head supplies are enumerated, not fudged.** `VerbDef::unsupplied` lists them
per verb (`ignore`, `tolerance`, `set_angle`, `angle`, `width`, `disembark`, `shift`/`ctrl`
/`alt`, `oxx`/`whose`, …) and Python sees them in `spec()['unit_verbs']`. Adding a head
later is a generator change, not an archaeology exercise.

### Masking

Masks are the load-bearing part (the microRTS ablation: 0.82 win rate with full
parameter-level masking, 0.00 without), so they get three stated invariants and a test per
invariant:

1. **A masked-in value is applicable.** Verified adversarially:
   `masked_sampling_produces_no_illegal_actions` samples uniformly under the mask for 25
   steps × 8 envs and asserts `illegal == 0` *and* `applied > 0` (so it cannot pass
   vacuously). It fails loudly if a mask loosens.
2. **No head is ever all-zero**, including padded entity slots, so a masked categorical is
   never empty.
3. **Outcomes are counted, not swallowed.** `apply_stats()` splits every applied action
   into `noop / applied / stale / incoherent / accepted_no_effect / illegal`.
   - `stale` — the entity or target died earlier in the same step (a real multi-agent
     race, not a policy error). 0.3 % in the smoke test.
   - `incoherent` — every head value was individually legal but the conjunction is not.
     This is exactly the residue a factored mask cannot express, and it is **0.4 %**, which
     is the number that should be quoted rather than "small". The six known sources are
     listed in `mask::CROSS_HEAD_GAPS` and surface through `env.provenance()`.

**Capability data is derived from shipped game data.** `unitrules.xml` and
`buildingrules.xml` are the source; the key was establishing the mapping to `TypeIndex`.
`buildingrules.xml` has exactly 129 `<BUILDING>` entries = `NUM_BUILDTYPES`, and
`unitrules.xml` exactly 364 `<UNIT>` = `NUM_UNITTYPES` 352 + `NUM_GAIATYPES` 12. Positional
correspondence then holds and cross-checks on every boundary:
`unitrules[0]`→`PEASANTS`(50), `[1]`→`PEASANTSKOREAN`, `[2]`→`SCHOLARS`,
`[352]`→`BIRD`(402), `buildingrules[0]`→`VILLAGE`(414), `[128]`→`SPACEPROGRAM`(542).
`typecaps::tests::real_table_agrees_with_the_typeindex_partition` asserts a sample of this
and skips loudly (never vacuously green) if the table is absent.

From that: 16 capability flags per type, the exact `UnitData::is_plane` predicate, plus 312
producer→product edges recovered from the unit `WHERE` column joined against building
`NAME`. Those edges are what makes the `Type` head mask exact — a Barracks offers exactly
the units the shipped data says it trains, and only those the player can currently afford
and has pop headroom for. The plane predicate is the retail test (`AIR` domain and no
`FLAGS f`), so an air-domain helicopter is not silently treated as a plane.

Because the XML is copyrighted, the generator writes the table to
`schema/live/env-typecaps.bin` (gitignored) rather than into the committed `generated.rs`.
Absent that file the env still runs with permissive masks and `provenance()` says
`ABSENT — masks are PERMISSIVE` in that exact wording. Evidence-dependent branches are
not guessed: launch-patrol is not offered and ordinary patrol reports no effect until the
real table is present.

**Masks are bit-packed** (LSB-first, each head byte-aligned): 137 B per entity versus
1,063 B as `bool`. That is not micro-optimisation. The `Type` head row is
`produces[type] AND affordable[player]`, so writing it is a 101-byte AND of two
precomputed bitsets instead of 806 conditional stores.

---

## 3. Observation space

Chosen from `schema/state-schema.json` (i.e. from what the engine's own `DataWalk`
checksums as sim-critical), not from what looked useful.

**Spatial**, `(12, H, W)` f32: own/ally/enemy units, own/enemy buildings, mean HP fraction,
mean cooldown, resource nodes, blocking terrain, water, visible, explored. The first seven
are live; the last five are structurally present and currently zero because there is no
map — `obs::LIVE_PLANES` names the live ones and `provenance()` says so. **A silently-zero
plane is indistinguishable from a real one to a network**, so it is named rather than
quietly emitted.

**Entities**, `(N, 16)` f32, columns named after the engine's own fields: `myhits`
(`Object::myhits`), `myarmor`/`myspeed`/`recharging`/`stance`/`form` (`Unit`), `x`/`y`
(`GuyData`), `orders_x`/`orders_y` (`Unit`), plus `relation`, `type_index`, `category`,
`alive`, `controllable`. Selection is own-entities-first then nearest-by-centroid, so the
truncation to `max_entities` is a locality choice rather than an arbitrary one.

**Globals**, `(24,)` f32, every entry a `LeaderData` field: `econ[6]`, `base_rate[6]`,
`pop`, `pop_cap`, `city_num`, `units_built/killed/lost`, `score`, `territory`, `explored`,
plus frame/step/alive-count.

**Fog** is a constructor flag (`fog=True`) implementing an LOS-radius reveal from the
shipped `LOS` column. The engine's actual fog model is underived and the flag says so.

**Zero copy** is the buffer-ownership contract: every array is allocated once at
construction and rewritten in place; Python gets `(pointer, shape, dtype)` and builds a
numpy view through a minimal `__array_interface__` alias object. The smoke test asserts
`owndata == False`, a stable pointer across `step`, and changed contents.

*(Paid-for detail: `np.ctypeslib.as_array` was the obvious way to build those views and is
wrong here — a ctypes-backed array exports its buffer with format `<i` instead of `i`, and
PyO3's typed `PyBuffer` rejects it, so the sampler's own output could not be handed back to
`step`. The `__array_interface__` path produces an ordinary numpy array.)*

---

## 4. Reward

`RewardSpec` is a weight vector over 20 named terms: the eleven `LeaderData` score fields
(offsets 24..68) as per-step deltas, six outcome-counter deltas
(`units_built/killed/lost`, `buildings_built/lost`, `econ_total`), and
`win`/`loss`/`alive`. `env.set_reward_weights(**w)`; the raw term vector is in
`info['reward_terms']` every step so non-linear shaping is still possible in Python.

Shaping is **not** a Python callback on purpose: at 100 k agent-steps/s a per-agent
callback would put the GIL on the hot path and eat the entire budget.

The default is sparse (`win=+1, loss=-1`). A shaped default would be a hidden prior on how
the game should be played and this project has no measurement that would justify one.

**Honest gap.** `env.scores()` returns the eleven engine components *plus a twelfth column
that is ours*: `Leader::compute_score` `0x006EC560` has not been read, so the engine's
weighting is underived and the total is an unweighted sum, labelled `total(ours)`
everywhere it appears. Five of the eleven components are currently computed
(`score_units`, `score_buildings`, `score_economy`, `score_pop`, `score_combat`); the other
six are structurally present and zero, listed in `ScoreTerms::LIVE`.

---

## 5. What is scaffolding

The env prints this itself (`env.provenance()`); repeated here so it is not only in a log:

* **movement** — straight-line integer approach with an octagonal distance approximation.
  Not `Unit::move_step` (which uses `sin_table`/`cosx`/`find_angle`) and not
  `PathFinder::astar_path` `0x00683770`, which is unread.
* **patrol airframe host** — commands install the exact `AIR_PATROL` (17) or
  `GROUP_PATROL` (22) order into a dynamic queue with the recovered patrol-specific
  `QUEUE_FIRST`/`QUEUE_LAST` rules. Ground patrol executes and inserts its exact `ATTACK_TO`
  node ahead of itself. AIR_PATROL's local transition is available only through the
  fail-closed `AirPatrolHost`: ordinary frames preserve the order and aircraft position.
  Registered executable frontiers now recover `Unit::do_air_physics`'s top-level transaction,
  the mod-16 AIR/BOMBER search plans/folds, and the mod-32 nine-cell building traversal. Their
  dynamic world facts are not yet adapted into `EnvWorld`, so no straight-line movement,
  unordered scan, or always-empty target-search substitute executes. Captured postload
  `AirTypeData` is available through `Rules::air_type_data`; absent live tables fail closed.
* **pathfinding, gathering, economy rates, build-queue timing, tech tree, terrain, map
  generation, real fog** — absent. `QueueUp` and `Build` complete instantly with cost and
  pop enforced from the shipped tables; the timing would otherwise be invented.
* **start positions** — a placeholder ring.

What is *not* scaffolding: storage/identity/tick kernels are `don-sim`'s; damage is
`don_sim::mechanics::damage` (`ObjectData::get_damage` `0x00644130`) driven by the real
493×493 balance table from `schema/live/balance-real.bin` — though with `DamagePredicates`
at default, so only the spine of the chain runs (balance term, ×10 attack, mid-chain armor
subtraction, conditional floor of 1) and the guarded terms (overkill, flank, entrenchment,
height, river, recapture) are unreached. And the owner-slot rotation `(frame + i) % 10`
from `Objects::process_all` is reproduced, including for the order in which agents' actions
are applied.

Every verb with no dynamics is counted per verb: `env.unimplemented()`. In the smoke test
**34 %** of applied actions are `accepted_no_effect`, dominated by `GARRISON`, `GUARD`,
`FOLLOW`, `REPAIR`, `GATHER`. That percentage is the single best summary of how much of
this environment is still surface.

---

## 6. Design decisions that keep the endgame open

* **Full-game action space from day one.** Diplomacy, tribute, market, research and
  building all have verbs and masks; they are counted as unimplemented rather than absent
  from the space, so adding dynamics never reshapes the policy head.
* **Self-play / league.** `DonGymVectorEnv` takes an `opponent` callable, so agent slots
  1.. can be a frozen checkpoint, a scripted bot, or `random_masked_opponent`. The batched
  PettingZoo env keeps all agents on one policy forward pass.
* **Actions are resolved through `don-sim` handles**, not row indices. Within one step
  another agent's `DISBAND` or `QUEUE_UP` compacts the SoA rows, so a row index the policy
  saw can name a different entity by application time. Handles make simultaneous
  multi-agent action semantics well defined instead of order-dependent — and a stale
  reference is *reported*, not silently redirected.
* **`generated.rs` is generated.** Re-running `gen_spec.py` after any schema change
  re-derives verbs, heads and enum values, and fails loudly on an unclassified opcode or
  field.

---

## 7. Errors found in existing artefacts

* `crates/don-sim/src/world.rs` still declares `TICK_HZ = 15` with a comment sourcing it to
  the `rules.xml` header. The measured value is `TurnControl::timings` `0x00AFC4A4` =
  `{200, 125, 67, 50, 1}` ms, so Normal is 67 ms (≈14.9 Hz) and the array is the real
  object. `don-env` uses the measured constant (`generated::TICK_MS`) and does not consume
  `TICK_HZ`; `don-sim` is another lane's file so this is reported, not edited.
* The unitrules `RANGE` column is not safely `i16`: the two nuclear types ship
  `0-99999rng`. The capability record uses `i32` for range; anything else extracting that
  column should too.

---

## 8. Files

```
crates/don-env/
  gen/gen_spec.py     generator: schema/ -> src/generated.rs, ron-data/ -> schema/live/env-typecaps.bin
  src/generated.rs    @generated: verbs, heads, engine enums, opcode classification
  src/spec.rs         EnvConfig, head sizes, mask layout, plane/feature names
  src/typecaps.rs     capability records + producer edges, loader, permissive fallback
  src/state.rs        EnvWorld: don-sim world + LeaderData-shaped player state
  src/action.rs       action decode + apply, outcome accounting
  src/mask.rs         per-parameter mask writer, invariants, CROSS_HEAD_GAPS
  src/obs.rs          spatial / entity / global encoders
  src/reward.rs       20 named terms + linear shaping
  src/env.rs          VecEnv: owned buffers, parallel step, native masked sampler
  src/authoritative_episode.rs  deterministic scenario -> sole Sim owner -> frame receipts
  src/authoritative_backend.rs  fail-closed action + direct observation/reward adapter
  src/backend.rs      explicit compact/authoritative ownership selector
  src/py.rs           PyO3 bindings (feature `python` / `extension-module`)
  build.rs            macOS extension-module link args

python/
  don_env/_native.py          module loading + zero-copy numpy views + mask unpacking
  don_env/core.py             DonVecEnv
  don_env/spaces.py           Gymnasium spaces with a dependency-free fallback
  don_env/gym_vector.py       Gymnasium VectorEnv + opponent hook
  don_env/pettingzoo_env.py   PettingZoo ParallelEnv, batched and single
  build.sh                    cargo -> python/don_env/_don_env.so
  smoke_test.py               throughput + property checks
  README.md
```

`crates/don-env` was added to the workspace `members`; nothing else outside this lane was
modified.

## 9. Next, in order of leverage

1. **Host group/command decoding over the authoritative backend.** Decode the existing
   generated factored heads into fail-closed typed transactions; do not route unsupported
   verbs through compact `action.rs` behavior.
2. **Finish cloak/detection and ATTACK target commit.** Replace step 12's hardcoded
   `detector=false` with authoritative per-object detector facts; then make the production ATTACK
   executor atomically consume the retained identity plus the prepared combat-dependency/can-hurt
   proof. Only then may cloaked rows pass capture or ATTACK become an admitted verb.
3. **Gathering and the build queue** — the two scaffolded verbs that most distort what a
   policy learns, and both have derivable rules data (`SUPPORT`, `JOB_TIME`,
   `JOB_EXTRA_TIME`, `PROGRESSION`).
4. **Terrain + `PathFinder::astar_path`** — unlocks 5 of 12 spatial planes and the
   reachability mask.
5. **`Leader::compute_score` `0x006EC560`** — one function; removes the last "ours, not the
   engine's" caveat from the reward.
6. **Prerequisites** (`PREQ0/1/2` columns) — turns the `Type` head mask from
   affordability-only into the real tech-gated set.
