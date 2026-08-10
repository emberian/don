# Tick step 19 frontier — `Leader::process_event_frame`

This is an independent, path-importable reconstruction of the still-open step-19 proof
frontier. It covers the exact `Game::do_frame` dispatcher and the full deterministic body
of `Leader::process_event_frame` at retail VA `0x006EC180`. It deliberately does not edit
`tick.rs`, the systems module table, or the existing integrated Leader owner.

## Ground truth

The pinned artifacts are:

| artifact | bytes | SHA-256 |
|---|---:|---|
| `ron-bin/riseofnations.exe` | 9,925,120 | `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079` |
| `ron-bin/sbl/rise.pdb` | 57,290,752 | `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5` |

The PDB identity is GUID `{51d4f219-61c6-4f84-9d5b-c3361b0d291f}`, age 1. It names the
918-byte procedure at `0x006EC180`, `LeaderData::get_team_score` at `0x006D6520`,
`JukeBox::set_next_mood` at `0x0097D5D0`, and `Achieve::add_event` at `0x007AF660`.
The `Leader` type is 28,396 bytes (`0x6EEC`).

The corresponding source and focused tests are:

- `crates/don-sim/src/systems/leaders_process_event_frame_step19.rs`
- `crates/don-sim/tests/leaders_process_event_frame_step19.rs`

The test imports the source by path. This keeps the reversal compile-isolated until the
shared tick owner chooses to integrate it.

## Scheduled eight-Leader dispatcher

`Game::do_frame` `0x005924A0..0x005924BF` starts at `leaders` `0x00E3A390`, tests the low
byte of `LeaderData::leader_flags & 1`, calls the body at `0x005924AC`, adds `0x6EEC`, and
continues while the cursor is below `0x00E71AF0`. The visited object addresses are therefore:

`E3A390, E4127C, E48168, E4F054, E55F40, E5CE2C, E63D18, E6AC04`.

The array has exactly eight records. It is not the ten-owner object iteration domain, and
the gate is `leader_flags & 1`, not step 17's `leader_flags & 2` gate. Calls are sequential:
the body for an earlier record completes before the next record is tested.

## Event queue and frame math

The body reads signed `Game::frame` and executes `cdq; idiv 50; test edx,edx` at
`0x006EC1A7..0x006EC1B5`. A non-zero signed remainder returns before any Leader write.
On a due frame it processes the PDB-named block:

| offset | PDB field |
|---:|---|
| `+0xA4C` | `frame_battle` (`int`) |
| `+0xA50..+0xA56` | death, kill, damage, hit averages (`unsigned short`) |
| `+0xA58..+0xA5E` | death, kill, hit, damage current events (`unsigned short`) |
| `+0xA60..+0xA66` | matching fifteen-second totals (`unsigned short`) |

The exact queue lifecycle is:

1. At `0x006EC1BB..0x006EC1EC`, wrapping-add each raw current count into its
   fifteen-second total (machine order death, damage, kill, hit).
2. Multiply each current count by 100 and store only the low word back to the current slot.
3. Test that low word. Zero decays the old average with unsigned `(average*7)>>3`; non-zero
   replaces it with unsigned `(scaled+average)>>1`.
4. Leave the scaled low words observable throughout music and achievement work.
5. Only at `0x006EC501` and `0x006EC507`, after all product calls, clear the four current
   slots with two dword stores.

The typed `FoldEventQueue` and `ClearCurrentEvents` receipts expose both sides of that
lifecycle. The focused proof includes a non-zero raw count whose `*100` low word is zero,
so the retail low-word zero-test cannot be replaced by a source-count test.

## Local combat-mood path

Only a due record whose PDB `who` equals `Console::who` (`Console +0x298`) enters the mood
branch. After the fold, retail adds zero-extended `average_hit_rate` and
`average_damage_rate`:

- sum below 300 stores mood 2. The boolean passed to JukeBox is one only when both rates
  are zero;
- sum 300 through 599 is a dead band: no mood-global write and no JukeBox call;
- sum 600 or greater performs the team-score path.

The active path first calls `get_team_score` on the current dispatcher object. It then scans
the same eight records in address order. An enemy candidate must have flag bit 1, a different
`who`, a hostile relation, and a non-zero hit or damage average. Hostility uses the exact
short circuit visible at `0x006EC340..0x006EC35A`:

```text
other.diplos[local_who] == 0
    || leaders[local_who].diplos[other.who] == 0
```

The reciprocal read indexes the global array by `local_who`; it does not search for the
current record's identity. The focused proof intentionally makes those two answers disagree.
Because dispatch is sequential, the scan sees folded averages for earlier records and old
averages for later records. Only qualifying records call `get_team_score`, and the receipt
stream pins the current-object query before enemy queries.

Retail retains the greatest enemy score, initially zero. It compares the current score to
signed truncating `enemy*4/3` and `enemy*3/4`. The former boundary adds 200 to
`hit-damage`; the latter (inclusive) subtracts 200. Negative selects losing mood 1;
non-negative selects winning mood 0. Leaving current mood 2 passes a true boolean only at a
combat sum of at least 2,000.

The requested mood is stored at `0x00ECBA2C` even when it equals current mood
`0x00ECBA20`. Equality suppresses the call. A reached call becomes a typed
`JukeBoxSetNextMood` receipt because the callee immediately enters wall-clock/audio-owned
behavior. The global store is a sequenced deterministic mutation before that receipt.

## Encrypted age and battle event

After the music branch, every due Leader reads age before testing battle rates. The address
at `0x006EC452..0x006EC475` is precisely:

```text
leaders[leader.who].data_encrypted->ages ^ 0x00062766
```

That is a product/owner read keyed by `who`, not by dispatcher record. It is represented as
an `EncryptedAge` receipt including the encrypted word, XOR key, and signed decoded result.
Missing or out-of-range owner facts become typed residuals; they never become invented age
zero.

With decoded age `a`, retail requires:

1. `average_death_rate + average_kill_rate >= (a+1)*125`;
2. `frame_battle == 0`, or signed wrapping `frame-frame_battle >= 1800`;
3. one average is at least `a*10+20` above the other.

Deaths over kills pushes achievement kind 1; kills over deaths pushes kind 0. The exact tail
order is important: `Achieve::add_event(kind, who, EMPTY_STRING)` runs first, then retail
stores dword `0xFC18FC18` over the two averages, reads the frame again, stamps
`frame_battle`, and finally clears current events. One shared receipt sequence domain pins
that ordering.

## Typed boundaries and honest closure

The isolated executor supplies receipts for every reached external boundary:

- ordered `LeaderData::get_team_score` reads;
- encrypted age owner reads;
- `JukeBox::set_next_mood` wall-clock/audio tails;
- `Achieve::add_event` product-achievement tails;
- local folds, mood stores, battle sentinels/stamps, and final queue clears;
- invalid `who`, absent scores, and absent encrypted age words as fail-closed residuals.

The shared systems/tick owner now adopts this executor through the canonical `Leaders` event
block. `Leaders::event.product_outbox` is the installed typed owner for both product calls:
it retains their exact shared sequence, Leader identity, call VA, and arguments, while
leaving wall-clock/audio playback outside the deterministic core. The schedule row is green
because no reached boundary is dropped or charged; decoded mood and achievement vectors are
convenience projections of the ordered outbox. Root convergence formatted the isolated files
and validated all 12 focused tests in both modes:

- hbox debug: `tick-step19-20260809T225229Z-90306-2033-cb8b2b915d8e`;
- persvati release: `tick-step19-release-20260809T225229Z-90315-27682-cb8b2b915d8e`.

Neither job ran retail. The reproducible focused command is:

```text
cargo test -p don-sim --test leaders_process_event_frame_step19
```

The later live scheduler convergence was validated together with the shared Leader/PlayerSetup
owners: persvati job `gen7-integration-batch-v3-20260809T234324Z-74734-7021-05c01f206acb`
passed all five original real-step-19 tests. The later outbox integration also pins combined
JukeBox/achievement delivery, exact order, schedule promotion, and no stale replay.
