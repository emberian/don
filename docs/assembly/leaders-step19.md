# Leaders step 19 — event-rate post-pass

`Leader::process_event_frame` at retail VA `0x006EC180` is recovered in full by
`crates/don-sim/src/systems/leaders_process_event_frame_step19.rs`. The canonical owner in
`systems/leaders.rs` adapts its eight records through that receipt-bearing executor, and
`Sim::do_frame` supplies the production dispatcher directly: eight `LeaderData` records at
`0x00E3A390`, stride `0x6EEC`, each gated on `flags & 1`.

## PDB state and cadence

The 918-byte body returns before any write unless signed `Game::frame % 50 == 0`. On a due
frame it consumes the PDB-named block at `LeaderData +0xA4C..+0xA66`:

| offset | field |
|---:|---|
| `0xA4C` | `frame_battle` |
| `0xA50..0xA56` | `average_death_rate`, `average_kill_rate`, `average_damage_rate`, `average_hit_rate` |
| `0xA58..0xA5E` | `deaths_current_frame`, `kills_current_frame`, `hits_current_frame`, `damage_current_frame` |
| `0xA60..0xA66` | matching `*_fifteen_seconds` totals |

All rate fields are `unsigned short`. Retail first wrapping-adds each current count to its
total, then computes `scaled = current * 100` with a 16-bit store. A zero wrapped result
decays the prior average to `average*7/8`; otherwise the new average is
`(scaled+average)/2`, with unsigned shifts. Two dword stores at the tail zero all four
current counters.

## Local combat-music branch

Only the Leader whose `who` equals `Console::who` evaluates music:

- `average_hit_rate + average_damage_rate < 300` requests mood 2. The JukeBox boolean is
  one only when both rates are zero.
- `300..599` is a dead band: neither requested mood nor JukeBox changes.
- At 600 or above, start with `average_hit_rate - average_damage_rate`. Scan active enemy
  Leaders whose hit or damage average is nonzero, in record order, and retain the greatest
  exact `LeaderData::get_team_score` result. An own score at least four-thirds of that score
  adds 200; an own score at most three-quarters subtracts 200. A negative result requests
  mood 1, otherwise mood 0.
- Leaving current mood 2 passes a one to JukeBox only when the combat sum is at least 2000.

The scan is intentionally not hoisted out of the dispatcher. A local Leader observes new
rates for earlier records and prior rates for later records because retail calls each body
sequentially. The port preserves that asymmetry.

The reciprocal diplomacy expression indexes `leaders[local_who]` directly. It does not
search the array for a record whose `who` field matches. The live-tick regression makes those
two candidate records disagree and pins the direct array owner.

`JukeBox::set_next_mood` (`0x0097D5D0`) depends on wall-clock time and product audio. The
core writes the recovered requested-mood state and emits a typed `CombatMoodRequest` with
the exact boolean instead of importing that presentation subsystem.

## Lopsided-battle event

The achievement arm decodes the Leader's age from its economy block (`+0xDC ^ 0x62766`).
It requires:

1. `average_death_rate + average_kill_rate >= (age+1)*125`;
2. no prior stamp, or signed wrapping `frame - frame_battle >= 1800`; and
3. one rate at least `age*10 + 20` above the other.

Deaths-over-kills emits kind 1; kills-over-deaths emits kind 0. Retail then stores
`0xFC18` into both averages and stamps `frame_battle`. The port performs those state writes
and emits a typed `BattleAchievementEvent` for the external
`Achieve::add_event(kind, who, EMPTY_STRING)` call at `0x007AF660`.

## Live integration and product-tail ownership

The real `Sim::do_frame` step 19 now publishes the exact executor's event-queue and requested-
mood writes back into the canonical Leader owner. `Sim::last_event_frame_trace` retains the
single ordered receipt domain for team-score reads, encoded-age reads, local mutations,
residuals, and host tails.

`Leaders::event.product_outbox` is the installed headless owner for both reached product
calls. It retains the exact shared sequence number, Leader identity, call VA, and arguments;
the existing mood and achievement vectors are decoded convenience views of that outbox. The
outbox is cleared and repopulated on every dispatcher call, so stale requests cannot replay.
Wall-clock/audio playback remains deliberately outside the deterministic core, just as the
localized message/audio calls do for completed step 17; the call boundary itself is no
longer dropped or charged as a gap. Missing score/age facts suppress only their dependent
state transitions and remain typed residuals.

## Verification

Focused tests pin the independent IN_GAME/PROCESS gates, non-due no-write return, wrapping
16-bit arithmetic, all four smoothing arms and resets, the 300/600/2000 music boundaries,
hostile score bias and sequential observation, both presentation boundaries, the 1800-frame
cooldown, `0xFC18` sentinels, and fail-closed age lookup with no stale event replay. The
production-tick tests additionally pin direct eight-slot reciprocal ownership and prove that
both product calls are delivered into one ordered outbox, after which a non-due pass clears
the prior calls.

Evidence tier remains C: shipped PDB/type layout, disassembly/decompiler, and existing exact
team-score port, without a retail oracle comparison.

Root convergence validated the production reachability in persvati job
`gen7-integration-batch-v3-20260809T234324Z-74734-7021-05c01f206acb`: all five real-step tests
passed alongside the final PlayerSetup owner. The later ordered-outbox integration closes the
two typed product boundaries and promotes compiled schedule row 19; retail oracle comparison
remains open.
