# Replay-driven validation: the per-turn checksum harness

Lane: `replay-validate`. Every number here is **[measured]** — produced by
`tools/replay-validate.sh` against the 61 recordings in `ron-data/replays/`,
and reproducible with one command. Nothing here is verified in the
proof-assistant sense; see `docs/CHARTER.md`.

---

## Headline — what now works

**The loop exists and runs.** A real `.rcx` lockstep command stream is stepped
turn by turn, our fifteen `DataWalk` checksum channels are computed over our
state with a traversal *generated* from `schema/state-schema.json`, and each is
compared against the `CheckSumsCommand` the retail client recorded on that turn.
The first diverging turn and the diverging channel are reported per channel, per
file, and rolled up into `schema/replay-validation.json`.

Over the whole corpus — 61 files, 21 with checksums, **585,152 turns**,
**488,557 checksum packets**, 30 seconds wall clock:

| channel | best survival (turns) | matches / compares | of which trivial |
|---|---:|---:|---:|
| `walls` | **25,442** | 222,938 / 222,938 | 222,938 |
| `deaths` | **5,734** | 82,943 / 222,938 | 82,943 |
| `ammo` | **3,696** | 106,634 / 222,938 | 106,634 |
| `items` | 0 | 107,882 / 222,938 | 107,882 |
| every other channel | 0 | 0 / 222,938 | 0 |

`survived` = consecutive agreeing turns from the recording's first checksummed
turn. **The honest headline is 25,442 turns on `walls`, and it is a weak
number**: our simulation state is empty, `walls` was empty in every recorded
game, and adler-32 over nothing is 1 on both sides. `trivial` counts exactly
that case, and it is currently 100 % of our agreements. The number to watch is
not the biggest one; it is the first time `trivial` is less than `matches`.

The two numbers with real content today are `ammo` and `deaths`: for 3,696 and
5,734 consecutive turns respectively, our model asserts "no projectile is in
flight" / "nothing has died yet" and retail agrees. That assertion *can* fail,
and does — at turn 1,584 and turn 847 in the 2025 recording — which is exactly
the failure mode a validation harness is supposed to have.

`units`, `builds`, `leaders`, `cities`, `goods`, `world`, `rules`,
`scenario_data`, `script_run_time`, `groups`, `guys` diverge on the **first**
checksummed turn, because those are non-empty from game start and we hold none
of them. That is the expected result and it is the work queue.

---

## The correction this lane paid for: the cross-player floor is zero, not 21

`docs/tracks/headless-client.md` claim 7 records **"265,910 of 265,931
(0.999921) cross-player checksum agreement; the 21 disagreements are real
simulation drift, not decode error."**

**All 21 are a join error.** [measured]

A `CommandPackage` header carries two things that both look like "when":
`stamp` (`+0x00`, the local simulation frame the package was built on) and
`group` (`+0x0c`, the monotone per-turn serial). Joining two players' tuples on
`stamp` compares turn *N* of one client against turn *N±k* of the other whenever
their local frame counters have drifted apart — which they do, because `stamp`
is local wall-clock-driven and `group` is the lockstep turn.

```
aligned by CommandPackage::group (turn serial): 265619/265619 identical
aligned by CommandPackage::stamp (sim frame):   265910/265931 identical
  of the 21 by-stamp disagreements, 21 compare packages from DIFFERENT turns;
  8 stamp buckets mix turns
```

Twenty-one out of twenty-one. Not "mostly", not "probably" — every single
by-stamp disagreement is a comparison between two different turns. Under the
correct join the corpus is **265,619 / 265,619 = 100.0000 % identical**.

Consequences, and they are load-bearing:

1. **There is no retail desync in this corpus at all.** The four "offending"
   files (2018-11-17, 2018-12-01, 2024-03-20, 2024-04-10) are clean.
2. **There is no noise floor to hide behind.** Any mismatch our simulation
   produces is ours. The wave brief's caution — "do not treat a late-game
   mismatch as automatically our bug" — is, on this evidence, unnecessary.
3. `docs/tracks/headless-client.md` claim 7 and the `MIN_CHECKSUM_AGREEMENT`
   floor in `crates/don-net/tests/roundtrip.rs` should be restated: the correct
   join key is `group`, and the correct floor is 1.0.

The harness reports both alignments on every run so this cannot quietly regress:
`don-replay crossplay --corpus`, and the assertion
`cross_player_tuples_agree_when_joined_on_the_turn_serial` in
`crates/don-replay/tests/corpus.rs` fails if any by-stamp disagreement is *not*
a turn mismatch.

---

## How to run it

```sh
tools/replay-validate.sh                 # full corpus -> schema/replay-validation.json
tools/replay-validate.sh --status        # age + headline of the last record, runs nothing
tools/replay-validate.sh --limit 5       # quick pass

cargo run --release -p don-replay -- validate <file.rcx>     # one recording, full table
cargo run --release -p don-replay -- scan --corpus           # decode only
cargo run --release -p don-replay -- crossplay --corpus      # the control experiment
cargo run --release -p don-replay -- walkers                 # generated-table coverage

cargo test --release -p don-replay                           # 17 unit + 5 corpus tests
```

Exit codes mirror `tools/oracle-regress.sh`: `0` ran, `2` corpus missing
(**SKIPPED is never a pass** — the banner says so), `3` harness failure.

Sample output, the one recording built by *our exact binary*
(`Playback___2025.02.10 21'26'50`, `Version: 00.2024.06.20`, 11,322 checksummed
turns, 6.0 frames/turn):

```
channel           survived first-div   expected        got  matches
units                    0         2 0xa204dc4c 0x00000001        0
builds                   0         2 0x3b5dcbf0 0x00000001        0
walls                11322      None                            11322
ammo                  1582      1584 0xe0a21695 0x00000001     6150
deaths                 845       847 0x8f2c0d2d 0x00000001     4979
groups                   0         2 0x0cd4f2a9 0x00000001        0
guys                     0         2 0x3e54eb9e 0x00000001        0
leaders                  0         2 0x231d5aec 0x00000001        0
cities                   0         2 0xd2251759 0x00000001        0
items                    0         2 0x7bfcb73b 0x00000001     4752
goods                    0         2 0x307b7792 0x00000001        0
world                    0         2 0x4f651430 0x00000001        0
rules                    0         2 0x12ba3104 0x00000001        0
scenario_data            0         2 0x09922b90 0x00000001        0
script_run_time          0         2 0x6a91bf5d 0x00000001        0
all                      0         2 0xa08a57b9 0x0000000f        0
```

`all = 0x0000000f = 15` on our side is the arithmetic self-check: fifteen
channels each holding adler-32's initial value of 1, summed. `rules` reads
`0x12ba3104`, the shipped-rule-set constant, exactly as
`docs/derivation/replay-checksum.md` §5 predicts.

---

## What was built

`crates/don-replay`, no external dependencies (same rule as `don-net`: a green
`cargo test` must never touch the registry; gzip shells out to the system tool).

| file | what |
|---|---|
| `src/checksum.rs` | `adler32` mirroring `0x00a46830` including the `NMAX = 5552` chunk boundary; the two-method `DataWalk` trait; `CheckSum` with `+0x0c` mask, `+0x10` adler, `+0x14` byte counter; `Channel`, `Channels`, `computed_total` |
| `src/walk.rs` + `src/walk_gen.rs` | **generated** traversal: 278 classes, all 1,421 ordered ops from `schema/state-schema.json`, run table-driven over object byte images |
| `src/state.rs` | per-channel object lists in PDB layout; `check_all` over them; the explicit `don-sim` bridge and its gap list |
| `src/wire.rs` + `src/wire_gen.rs` | **generated** field table for all 82 `*Command` structs; typed field reads; `Order`; `CommandClass` |
| `src/replay.rs` | `.rcx` → turns: framing, XOR/pad recovery, per-turn per-player commands and checksums, `frames_per_turn`, both crossplay joins |
| `src/harness.rs` | the loop, the `Simulation` trait, `NullSim`, the divergence profile |
| `src/report.rs` | `schema/replay-validation.json` |
| `gen/gen_walk.py`, `gen/gen_wire.py` | the generators; re-run after any schema change |
| `tests/corpus.rs` | 5 corpus tests including a mutation test that proves the comparator bites |

**Both tables are generated, not typed.** Hand-writing a 1,421-op traversal is
precisely how a walker silently drifts from the binary; the generators are two
short Python scripts and re-running them is the only supported way to change the
traversal.

### The checksum model, stated plainly

Fifteen channels. Each one gets a **fresh** `CheckSum` whose adler starts at 1;
`check_all` sums the fifteen accumulators with wrapping 32-bit addition into
`all`. It is not one rolling hash, and `all` is not a hash of hashes. The wire
command carries sixteen `u32` — the fifteen channels plus that sum — in 65
bytes. All 488,557 decoded packets satisfy `word16 == Σ(words 1..15)` and all
fifteen values in all 488,557 are adler-32-shaped (both halves < 65521). At a
≈2⁻³² false-positive rate per packet, that is a strong statement about the
decode.

---

## Measurements that fell out

**Frames per multiplayer lockstep turn is 2, 4, 6 or 8, and it is a property of the
recording.**
Measured per file from `stamp` deltas across consecutive `group` values: 6.0 in
the 2025 and 2018/2019 games, 4.0 in most 2024 games, 2.0 in the
2024-02-23 game. The lockstep turn is **not** the simulation frame, and a
harness that assumes one frame per turn is wrong by a factor of 2 to 6. This is
why `frames_per_turn` is measured and passed to `Simulation::step_turn` rather
than hard-coded.

**The command worklist, ranked by real frequency** across 5,055,253 commands
(`schema/replay-validation.json` → `totals.opcode_counts`):

| opcode | struct | class | count |
|---|---|---|---:|
| `0x48` | `CameraCommand` | Presentation | 1,296,016 |
| `0x4a` | `TurnDataCommand` | Sim | 1,285,782 |
| `0x4f` | `PlayerSpeedCommand` | Sim | 1,011,134 |
| `0x3a` | `NextCheckSumCommand` | Lockstep | 796,957 |
| `0x39` | `CheckSumsCommand` | Lockstep | 488,557 |
| `0x00` | `GroupCommand` | Sim | 78,197 |
| `0x18` | `QueueUpCommand` | Sim | 28,993 |
| `0x19` | `BuildCommand` | Sim | 14,042 |
| `0x15` | `DisbandCommand` | Sim | 10,916 |
| `0x07` | `MoveToCommand` | Sim | 9,178 |
| `0x04` | `AttackCommand` | Sim | 5,715 |
| `0x2e` / `0x2f` | `BuyCommand` / `SellCommand` | Sim | 4,427 / 2,592 |

Totals: 2,468,461 Sim, 1,285,514 Lockstep, 1,301,278 Presentation. The three
per-turn bookkeeping opcodes (`0x4a`, `0x4f`, `0x48`) are 71 % of all traffic;
**the entire player-order stream is under 160,000 commands corpus-wide.** For an
order-application layer, `QueueUpCommand`, `BuildCommand`, `MoveToCommand` and
`AttackCommand` are the whole game.

`0x4a` and `0x4f` are classified `Sim` **conservatively** — their handlers are
unread here, and over-reporting the worklist is the safe direction. Settling
them is cheap and would move a million commands out of the sim set.

**Two channel walkers have no derived traversal.** `ScenarioData::walk_data`
(`0x00997ad0`) is `static __cdecl`, so `schema/state-schema.json`'s `this`-taint
never fires on it, and `RunTimeEnv::walk_data` (`0x009c41a0`) is likewise absent
from the 278 resolved classes. `don-replay walkers` prints this, and
`state::tests::channel_walker_resolution_is_thirteen_of_fifteen` fails if the
count changes in either direction.

---

## Honest limits

- **`matches` is not `survived`, and the gap is accidental agreement.** `items`
  has 107,882 matches corpus-wide and 0 survival: the channel returns to 1
  mid-game when the item list empties, and our permanently-empty model
  coincidentally agrees. Only `survived` is a progress metric.
- **Every current agreement is `trivial`** — zero bytes walked on our side.
  We are matching "nothing" against "nothing". This is real (it fails the moment
  retail acquires an object) but it is not evidence about any mechanic.
- **The checksum phase is a parameter, not a finding.** The sender builds its
  package during `PROCESS_TURN` and appends the tuple to the same package that
  carries that turn's new commands, but lockstep executes a turn's commands some
  turns later. `--phase before|after` and `--latency N` exist; the default is
  `before_commands, latency 0` and the run records which was used. **The latency
  is unmeasured.** It does not affect any number above, because our state is
  empty and never moves — it will matter the day it does, and settling it is the
  first thing the next lane should do.
- **The generated traversal is 216 executable ranges out of 1,421 ops**
  (`don-replay walkers`):

  | op kind | count | executable |
  |---|---:|---|
  | `this`-relative byte range | **216** | yes |
  | resolved length, unresolved base | 369 | no — a stack temporary or an untracked pointer |
  | resolved range on a **global** object | 62 | no — real offsets, but not into an object image |
  | fully unresolved | 252 | no |
  | sub-object call, class resolved | 361 | yes (see below) |
  | tag | 145 | yes (a no-op for `CheckSum`) |
  | virtual dispatch `[reg+0x7c]` | 9 | no — needs a concrete receiver |
  | sub-object, class unknown | 7 | no |

  `schema/state-schema.json` reports 647 "resolved byte ranges"; that counts
  every op with a known *length*. Only 216 also have a known `this`-relative
  *base*, which is what an image walk needs. The 62 global-base ranges were
  initially emitted as `this`-relative and would have hashed the wrong bytes
  with full confidence — they are now a distinct `WalkOp::Global` that refuses
  to execute. Closing the remaining gap means a proper abstract interpreter over
  the call graph from `check_all`, which the schema's own caveats already flag.
- **`WalkOp::Sub` recurses on the same image.** Correct for base-class chains
  (`Unit → Object → ObjectData`, all at `this+0`), wrong for member sub-objects
  at a non-zero offset; the schema records the target class but not the member
  offset. `WalkOutcome` counts every op it could not execute
  (`ops_unresolved`, `ops_global`, `ops_virtual`, `ops_sub_unknown`,
  `ops_out_of_range`) so a partial walk can never be mistaken for a complete one.
- **`SimBridge::populate` is a deliberate no-op returning 0.** `don-sim::World`
  holds `pos/vel/hits/armor/attack/cooldown` over anonymous rows and no
  engine-layout record. Converting it would mean *inventing* a layout, and an
  invented layout that hashes to a plausible number is the exact failure this
  project exists to avoid. `SimBridge::CARRIED` and `SimBridge::MISSING` are the
  gap, as data rather than prose.
- **`cargo test --workspace --exclude don-net --exclude don-ai` currently has 4
  failures in `don-sim::trig`** (`sin_table`, `find_angle`, the index-255 wrap).
  Those are a concurrent lane's in-flight module; this lane touched no shared
  crate except adding `crates/don-replay` to the workspace members list.
  `cargo test -p don-replay` is 22/22 green.

---

## The next three moves, in order

1. **Bridge `don_sim::generated::state`.** A sibling lane landed
   `crates/don-sim/src/generated/state.rs` — real PDB-layout SoA columns with
   `FieldDesc { name, offset, size, repr }` per class. A `row_image(row) ->
   Vec<u8>` that scatters a row's columns back to their PDB offsets plugs
   straight into `SimState::channels[i].objects` and the harness starts walking
   real bytes with no change to the loop. That is the single highest-leverage
   follow-on and it is mechanical.
2. **`rules` first — it is one 32-bit word.** `Game::walk_rules_data`
   (`0x00589550`) over the loaded `Constants` must produce `0x12ba3104`. It is
   static, so it needs no simulation at all, and it validates the entire loaded
   rule set's sim-critical bytes *and* their traversal order in one comparison.
   `checksum::SHIPPED_RULES_CHANNEL` is already the target and the corpus test
   already asserts retail's side of it.
3. **Then `deaths` and `ammo`**, because their divergence turn (847 / 1,584 in
   the 2025 game) is the first place a real mechanic has to be right, and both
   channels start from a state we already reproduce.

### Hazards the next lane will hit, from sibling lanes

- **`Array<T>` capacity and growth hint are checksummed** — length, capacity,
  grow, flags, *then* elements. A Rust `Vec` with a different growth policy
  desyncs on identical logical state. Every channel that walks a growable array
  is exposed. Byte images sidestep this only if whoever fills them reproduces
  the engine's capacity, not just its length.
- **`Objects::process_all` rotates owner-slot order every frame** by
  `(frame + i) % 10`; a fixed-order scheduler diverges within one tick.
- **Ammo is not driven by `Objects::process_all`** — projectile motion and
  impact run in `Ammo::inc_time` at step 15 of `do_frame`, so projectile deaths
  land at the *end* of a tick in pool-slot order. Relevant directly to move (3).
- **Borders settle over many frames** under a hard global budget of 256
  cells/frame; expecting one-tick settling desyncs the `world` channel.
- **Score refreshes only when `(who + frame) % 10 == 0`.**
- **Sim-critical is a strict subset of save-game state**: some fields are gated
  on `DataWalk::checksum == 0` (city names are the known case), so a walker that
  emits everything `SaveGame` emits will over-hash. The generated table takes
  the `SaveGame`/`LoadGame` ranges; the `CheckSum`-only branches are not yet
  distinguished, and `schema/state-schema.json`'s own caveats say so.

---

## Ledger entries to add to `docs/provenance-ledger.md`

| mechanic | source | tier | evidence |
|---|---|---|---|
| Cross-player checksum tuples are **identical when joined on `CommandPackage::group`**; the 21 known disagreements are a `stamp`-join artifact | `crates/don-replay`, corpus | **C [measured]** | 265,619/265,619 identical by group; 265,910/265,931 by stamp, and **21 of 21** by-stamp disagreements compare packages from different turns; 8 stamp buckets mix turns |
| All 488,557 recorded `CheckSumsCommand` packets satisfy `word16 == Σ(words 1..15)` and are adler-32-shaped | `crates/don-replay/src/replay.rs`, corpus | **C [measured]** | 488,557/488,557 both tests; ≈2⁻³² false-positive rate per packet |
| `rules` channel = `0x12ba3104` in every recording that carries checksums | corpus | **C [measured]** | constant within and across all 21 checksummed files, two engine builds |
| A multiplayer lockstep turn spans **2, 4, 6 or 8 simulation frames**, per recording | `stamp`/`group` deltas | **C [measured]** | current corpus distribution: 1 / 6 / 37 / 16 files; the solo recording measures 1.0; 6.0 for the 00.2024.06.20 recording |
| Our simulation survives **25,442 turns on `walls`, 5,734 on `deaths`, 3,696 on `ammo`, 0 on every other channel** | `tools/replay-validate.sh` | **C [measured]** | 222,938 comparisons per channel; every current agreement is `trivial` (zero bytes walked) |

And **correct** in `docs/tracks/headless-client.md` claim 7: the 21 cross-player
disagreements are not simulation drift. Under the correct join key the corpus
shows no desync at all.
