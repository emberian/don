# Descent of Nations — execution board

This is the live project board. The durable rules are in `docs/CHARTER.md`; agent
orientation is in `README-LLM.md`; the interrupted Claude-session recovery ledger is in
`docs/RECOVERY.md`.

## North star

Build a deterministic, batch-parallel Rust reimplementation of *Rise of Nations: Extended
Edition*, derived from the shipped binary and data, faithful enough for replay-checksum
agreement, and fast and complete enough to train a state-of-the-art player AI.

## The progress metric

The scoreboard is:

```sh
tools/replay-validate.sh
```

It compares our fifteen simulation-state checksum channels with the per-turn values in real
multiplayer recordings. Progress means a channel walks **non-empty state** and survives more
consecutive turns. Empty-on-both-sides agreement is useful harness coverage, but it is marked
`trivial` and does not count as mechanic fidelity.

Baseline measured 2026-08-08:

- 61 replay files; 585,152 turns; 488,557 structurally sound checksum packets.
- Retail control: 265,619 / 265,619 cross-player tuples agree when joined on turn `group`.
- Best survival: `walls` 25,442, `deaths` 5,734, `ammo` 3,696 turns.
- Every match is still trivial: `SimBridge::populate` receives an empty replay world and the
  harness walks zero bytes. The first real milestone is `trivial < matches` on any channel.

The generated record is `schema/replay-validation.json`; design and caveats are in
`docs/tracks/replay-validation.md`.

## Current snapshot

Measured on the shared worktree on 2026-08-08:

| Gate | State | Evidence |
|---|---|---|
| Rust workspace | green | `cargo test --workspace --all-targets`: 809 passed, 0 failed |
| Retail oracle regression | green, current | 12/12 cases, 16,236,396 trials, 0 fail/skip/crash; 7 Tier-B claims remain outside the suite |
| Replay harness | green but trivial | full corpus command above; all current matches walk zero bytes |
| Simulation derivation | broad | 563 functions / 495,573 bytes in a seeded retail potential-reachability closure are cited; the closure and citations are both approximations, not fidelity |
| Runnable tick wiring | not yet mechanically measured | direct review finds most `systems/*.rs` code isolated behind unit tests; the old “3 call sites” metric was only a textual grep and has been retired |
| RL surface | working over partial dynamics | `crates/don-env`, `python/don_env`, `docs/tracks/rl-env.md` |
| Headless networking | our peers work | two processes complete a 40-turn TCP lockstep run; retail internet join is not complete |
| AI / analytics / web | working prototypes | each has a lane report under `docs/tracks/`; none implies whole-game fidelity |
| Worktree | recovered, not checkpointed | 58 tracked paths (53 modified + 5 deleted), 46 untracked entries; preserve all until classified |

The coverage denominator and method live in `schema/coverage.json` and
`docs/mechanics/COVERAGE.md`. “Cited,” “compiled,” “called by a tick,” and “agrees with
retail” are four different states and must never be collapsed into “implemented.”

## Now — P0 integration tranche

These are ordered. Finish a vertical slice before opening more mechanic breadth.

1. **Make the static `rules` channel non-trivial.** Implement the complete
   `Game::walk_rules_data` root—Types, both Constants spans, the 493×493 signed-16-bit
   balance matrix, and 24 Tribe blocks—and require the shipped data to produce
   `0x12ba3104` with `bytes_walked > 0` and `complete = true`.
2. **Build honest bridge plumbing.** Generate row scatter/images, iterate objects in retail
   checksum order, replace rather than append channel contents, and propagate walker
   completeness. Synthetic bridge tests are infrastructure, not corpus progress while the
   replay simulation is empty.
3. **Initialize dynamic replay state from what `.rcx` actually contains.** Parse Game/GameInfo
   setup, rules, players, seed, and command packages before the first comparison. Replays do
   not contain a full initial World snapshot; dynamic channels require deterministic map and
   starting-object reconstruction. Full `.svx` saves are separate fixtures.
4. **Wire the derived systems into the retail-ordered tick.** `World::step` has the 29-step
   skeleton and owner rotation, while tens of thousands of lines under `systems/` remain
   isolated. Connect one end-to-end slice: decoded command -> order -> system -> generated
   state -> checksum.
5. **Close RNG stream consumers as their systems become live.** Air's anti-air dud gate is
   present and compiled; pathfinding's failure epilogue and wildlife/world generation remain
   known stream obligations. Count skipped draws rather than inventing them.

Acceptance gate for this tranche:

```sh
cargo test --workspace --all-targets
tools/oracle-regress.sh
tools/replay-validate.sh
```

The replay result must improve in non-trivial matches or expose a narrower, recorded first
divergence. A merely green Rust suite is not completion.

## Next — P1 runnable game

- Complete or replace the partial `Unit::work` / `do_job` driver, then wire the
  command-to-order bridge for the high-frequency replay verbs: queue, build, move, attack.
- Consolidate the world-channel walker and the duplicate adler implementations.
- Connect leader economy, production, cities, combat, movement, groups/guys, borders/fog,
  victory, walls, air, naval, and items only as their prerequisites enter the tick.
- Replace placeholder action effects in `don-env` and `don-ai` with the same command path the
  replay harness uses; keep `accepted_no_effect` visible until it reaches zero.
- Turn the partial live attach and replay viewer into repeatable tools with reports and smoke
  tests; do not treat “file exists” as a landed lane.

## Later — P2 product tracks

- Join a real retail internet game: the PlayFab title id is recovered (`84214`); obtain a
  Steam auth ticket, load-test the replacement DLL in retail, then attempt matchmaking.
- Complete the shipped AI baseline: the BHS economic script is only one of twelve production
  stages and only one of four AI subsystems.
- Promote analytics from Ancient-age opening studies to full build-order and tactical search.
- Live browser spectating, replay playback, cluster views, and eventual drop-in play.
- Benchmark the smallest fidelity relaxation that unlocks the next throughput order of
  magnitude; keep the bit-exact path as the reference.

## Recovered work that still needs landing

The final Claude coverage wave died at the session limit. Its exact transcript and artifact
state are recorded in `docs/RECOVERY.md`. Immediate integration facts:

- `air.rs` and `walls.rs` compile in the 809-test umbrella suite.
- `naval.rs` and `items.rs` exist but remain quarantined outside `systems/mod.rs`: isolated
  audits found one compile blocker plus one bad roster fixture in naval, and one caller-contract
  fixture failure plus known visibility/unlink gaps in items.
- `donscan/src/live.rs` and the replay-viewer edits are partial and lack their lane reports.
- casters/animals stopped after derivation, before writing code.
- wonders/nations produced `schema/effects.json`, but its generator is still temp-only and no
  runtime module or report exists.
- the real-game join lane produced no report, but its transcript contains the recovered
  PlayFab title id (`84214`) and a completed `steam_api.dll` transfer in scratch space.

## Wave protocol

Wide waves are welcome when the tasks are independent, but every wave has a landing phase:

1. Assign each lane exclusive code and report paths.
2. Require a return state: `landed`, `research-only`, `partial`, or `no artifact`.
3. Harvest files and transcripts before restarting or discarding anything.
4. Run the umbrella build after shared-struct changes; run the replay gate after sim changes.
5. Record the new measured snapshot here and the interrupted-lane details in
   `docs/RECOVERY.md`.
6. Commit named files only. Never `git add -A`, `git stash`, or erase an unclassified lane.

## Definition of done for a mechanic

A mechanic is done only when all applicable layers exist:

- binary/data provenance and fidelity tier in `docs/provenance-ledger.md`;
- engine-faithful state, including container capacity/growth where walked;
- execution from the retail-ordered tick or command path;
- captured expectations or retail differential tests (never hand-calculated fixtures);
- replay-channel impact measured, including first divergence and bytes walked;
- workspace gate green.

Anything less may still be valuable, but its state is “derived,” “ported,” or “isolated,” not
“implemented end to end.”
