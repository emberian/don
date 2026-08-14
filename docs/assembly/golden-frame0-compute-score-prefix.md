# Golden frame-zero compute-score prefix

The first golden owner-zero `Leader::compute_score(0)` call is at step 11 immediately after
the same Leader's `plan_strategy` return. The planner is not complete. This tranche therefore
accepts only an independent supported-retail capture at the post-planner call boundary, joined
to the existing detached planner prefix and its first open-child request. It does not promote
the planner prefix into a completed call or mount either transaction on `Sim`.

## Exact local instructions

`Leader::compute_score` is `0x006EC560`. On frame zero, `0x006EC572..0x006EC574` bypasses
the ordinary ten-frame throttle before reading `force` or the game-over semaphore. The next
instructions are:

```text
006ec59a  test dword [esi],0x01800000  ; NEW_UNITS | NEW_TECH
006ec5a0  mov  dword [esi+0x1c],0     ; score_explored
006ec5a7  je   0x006ec5b6
006ec5ab  call 0x006bc500              ; compute_unit_score
```

Ordinary setup `Unit::init` ORs `NEW_UNITS` (`0x00800000`) into owner zero's Leader flags.
The source capture must retain that exact bit after `plan_strategy`; a zero/default claim is
refused. The local prefix therefore owns one instruction-ordered same-value-or-changing store
to `score_explored` and stops at `compute_unit_score`.

The first request originally names the complete child input surface: `num_queued[50..=401]`,
`num_units[352]`, the admitted type cost/support/attack projection, and the Game/Rules
Armageddon comparison. These inputs do not yet have one golden call-boundary owner, so no full
score component, total, flag clear, diplomacy call, or next-Leader dispatch is published.

## Unit-score second pass

The open child can nevertheless advance through two unconditional local instructions before it
needs any inventory:

```text
006bc50c  mov dword [edi+0x24],0       ; score_units
006bc513  mov dword [edi+0x28],0       ; score_units_2 / attack-unit score
006bc51a  call 0x00594020              ; Game::get_armageddon
006bc51f  mov edx,dword [0x00c061ec]   ; first post-child instruction
```

`plan_golden_frame0_owner0_compute_unit_score_prefix` replays and compares the entire parent
prefix, publishes the two ordered zero stores, and replaces the coarse Unit-score request with
the narrower exact first child. It does not invent before-values for the overwritten score cells:
the complete native call-entry image remains retained by digest, while only the two post-store
values are projected.

The new typed request asks for the three `RulesData` Armageddon constants, live
`Game::num_nations`, live `Game::num_sides`, and `GameInfo::starting_resources` read by
`Game::get_armageddon`. The separate live nuke counter is first read after the child returns.
Only if that comparison is open does retail begin the Unit/queue census at `0x006BC540`.

Replay settings and setup receipts contain pieces of this surface, but there is no joined
post-`plan_strategy` golden authority for the complete child and no complete post-plan Unit/type
inventory. Setup Unit rows are therefore not reused as a live score census. The second pass is
still detached and stops at `0x006BC51A`.

## Market chronology join

The binder consumes the exact `MarketLeaderAccountingReceipt` rather than accepting a caller
Market count. It reruns the production validator from the receipt before-image and requires the
golden row/owner/object/type/City identity and exact retail chronology:

- `buildings_built: 1 -> 2`;
- Market `num_buildings[22]` and regional cell `0 -> 1`;
- wealth `gather_slots[2]` and `gather_slots_high[2]` `0 -> 1`;
- `high_buildings[22]` `0 -> 1`; and
- dirty ORs `0x02000000` then `0x08000000`.

The native compute-score entry must still contain those cells. The local score prefix preserves
them byte-for-byte. In particular, the later exact planner gather-slot block writes resources
0, 1, 3, 4 and 5 but skips wealth resource 2; replacing all six slots with a reconstructed
array would erase the Dutch Market's source-owned value.

Once the Unit-score child closes, `compute_build_score` at `0x006BC3F0` is the next child and
will consume `num_buildings[22] == 1`. The request carries the Market receipt digest forward so
that continuation cannot substitute a default Building census.

## Truth boundary

This tranche is a typed chronology and first-child request, not a replay-checksum producer.
The Leader checksum channel remains uninstalled, its setup residual remains 619 bytes per
active row, and survival remains zero. The frame-zero strategy call, Unit-score child, later
score children, owner-zero diplomacy, and owner-one strategy sequence all remain red.
