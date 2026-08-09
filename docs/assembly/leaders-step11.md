# leaders-step11 — strategy dispatcher and exploration prefix

Step 11 of `Game::do_frame` now executes the complete 93-byte
`Leaders::strategy_all` dispatcher at retail `0x006ED430` and the complete 227-byte
`Leader::check_explore` body at `0x006BC860`. The two very large AI policy bodies remain
explicit reached-call gaps; score and victory reuse the existing exact ports.

## Exact call order

The dispatcher walks the eight `Leader` records at `0x00E3A390` in `0x6EEC` strides.
A slot enters only when `(leader_flags & 3) == 3`. For every entered slot retail calls:

1. `Leader::check_explore` `0x006BC860`;
2. `Leader::plan_strategy` `0x006B9620`;
3. `Leader::compute_score(0)` `0x006EC560`;
4. `Leader::diplomacy` `0x006BC950`.

After the loop, independently of whether any slot entered, semaphore bit 9
(`game[0x821] & 2`) gates `Game::check_victory` `0x005926B0`. The earlier tick adapter
incorrectly required an active leader before that tail call and charged each AI gap once
even for an empty loop. The recovered call trace now preserves the per-slot interleaving,
charges the two unresolved AI calls only when reached, and executes the tail gate in its
actual position.

## `Leader::check_explore`

Frame zero always recomputes. Later frames use the exact signed integer phase:

```text
(leader.who * 25 + game.frame + 12) % (200 / ai_speed) == 0
```

The function queries `LeaderData::has_preq(0x2B2)`. The captured shipped `BonusType 690`
row has one prerequisite, Electronics (`TypeIndex 556`), so the live tick adapter supplies
that decoded per-leader fact from the existing tech state. When true, `Leader +0x9D4`
becomes `WorldData::reg_size`.

Otherwise the body sets the count to zero and visits every region coordinate. It samples
the persistent `WorldData::seen2` plane at fog coordinate `(4*x + 3, 4*y + 3)`, testing
the low byte of `1 << (leader.who & 31)`, and increments once per matching region cell.
An incomplete plane or impossible zero AI period remains explicit `MissingFacts`; it does
not claim that the retained count was refreshed.

## Runtime proof

`crates/don-sim/tests/tick_step11_strategy.rs` drives the real `Sim::do_frame` path and
mutation-pins three boundaries:

- one active leader reaches four calls, recomputes explored cells on its exact phase, and
  charges one planner plus one diplomacy gap;
- semaphore bit 9 executes the victory tail with zero active leaders;
- a slot with only one of the two low flag bits is skipped and step 11 is vacuous.

Focused pure tests in `systems::leaders` additionally pin slot-order interleaving, sampled
fog indices, frame-zero period bypass, full-map prerequisite handling, refreshed counts,
and fail-closed incomplete host state.

## Remaining red

- `Leader::plan_strategy` is 11,108 bytes of AI policy.
- `Leader::diplomacy` is 20,348 bytes of AI policy.

Those are still visible in coverage at each reached retail boundary. The step-11
dispatcher and exploration prefix are no longer represented as absent.
