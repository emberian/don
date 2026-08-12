# Nuclear embargo owner and market child

`crates/don-sim/src/systems/leader_nuke_embargo_runtime.rs` closes the two embargo children
consulted by `Leader::market_speculation` without mounting step 11 or changing save format.

## Canonical owner

The complete arithmetic reads two mutable plain PDB fields per Leader row:

| offset | field | meaning |
|---:|---|---|
| `+0x7B4` | `nuke_stamp` | timer origin; zero disables the personal embargo |
| `+0x7BC` | `nukes_used` | nation contribution to the timer |

The proposed Leader-row extension is those two decoded/plain little-endian i32 values in PDB
order: 8 bytes per row, 64 bytes across eight leaders. It is a substantive v15 candidate beside
the rate and production-economy owners, but this standalone patch does not bump or mount it.

## `get_my_nuke_embargo` `0x006D5350`

The complete 98-byte child returns zero without calling `has_wonder(0x21E)` when `nuke_stamp`
is zero. A present anti-nuke wonder also returns zero. Otherwise retail computes, with wrapping
x86 i32 arithmetic:

```text
timer = NUKE_EMBARGO_WORLD * Game.armageddon
      + NUKE_EMBARGO_NATION * nukes_used
      - Game.frame
      + NUKE_EMBARGO_BASE
      + nuke_stamp
return max(timer, 0)
```

Shipped rule values are base 900 frames, nation 900 frames, and world 0 frames.

## `get_nuke_embargo` `0x006D52C0`

The complete 129-byte parent first queries the requesting leader's anti-nuke wonder and returns
zero immediately when present. Otherwise it visits all eight leader slots in order. Inactive
slots are skipped. The requesting slot is admitted directly; every other active slot calls
`is_ally`, whose exact rule is relation value 2 in both directions. The return is the signed
maximum of `get_my_nuke_embargo` over the requester and mutually allied active leaders, starting
from zero.

The executable receipt pins the outer wonder call, active visit/admission masks, ordered ally
queries, every personal-child result, and the slot that established the maximum. A detached
`who` outside the eight-row retail table is rejected rather than converted into an unsafe
pointer read.

## Evidence

Focused tests cover the exact 64-byte codec, both personal early gates, shipped arithmetic,
wrapping overflow and signed clamp, mutual-versus-one-way alliance admission, inactive rows,
outer-wonder early return, invalid identity, and direct gating of the recovered market opening
before its scarcity mutation. Three deterministic four-seat matrices cross the standalone
owner save projection and compare future-frame results and complete receipts with uninterrupted
execution.

No test grants resources, completes production, forces an embargo result, or mutates shared
market state. Existing activity, relationship, frame, armageddon, rule, and wonder facts remain
their canonical owners; this runtime does not serialize them again.
