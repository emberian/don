# `market_speculation` production-economy owner and opening cohort

Cycle 5's second standalone owner is the plain PDB block
`LeaderData +0x450..+0x4D4`. It is implemented in
`crates/don-sim/src/systems/leader_market_speculation_runtime.rs` without a module mount or
save-format change.

## Canonical field map

| offset | PDB field | shape |
|---:|---|---:|
| `+0x450` | `econ` | `i32[6]` |
| `+0x468` | `escrow` | `i32[6]` |
| `+0x480` | `escrow_rate` | `i32[6]` |
| `+0x498` | `tributes` | `i32[6]` |
| `+0x4B0` | `base_rate` | `i32[6]` |
| `+0x4C8` | `worst_good` | `i32` |
| `+0x4CC` | `best_good` | `i32` |
| `+0x4D0` | `shortages` | `i32` |

These 33 plain integers are not `LeaderDataEncrypt`. In particular, `econ` is the planning
target/flag array consulted by `market_speculation`; it is not the encrypted stockpile named
`bucket`. The proposed save projection is 132 decoded/plain little-endian bytes in the PDB
order above.

## Whole child: `LeaderData::has_market`

The complete 86-byte function at `0x006D5410` first returns false if the leader's market type
count at `+0x558A` is zero. It otherwise walks the leader's compact Build list in order and
returns on the first row whose object flags have both active bit 0 and market bit 11 and whose
signed owner byte at `ObjectData +0x5F` equals `LeaderData::who`. The executable records the
exact visit count and winning row.

## Exact parent boundary

The bounded `Leader::market_speculation` `0x006C8110` opening preserves retail's short-circuit
order:

1. `has_tribe_bonus(4)`, otherwise `has_preq(0x2AD)`;
2. the complete `has_market` child;
3. `get_nuke_embargo() == 0`;
4. `starting_resources != 8`.

After admission, its first loop visits only available resource types. A signed `econ[r] > 4000`
is reset to 2000. Available decoded buckets below 200 raise scarcity from 0 to 1, and any below
100 raises it to 2. No unavailable bucket affects scarcity.

The executable stops before the first `calc_market_prices` call. It performs no trade, modifies
no encrypted stockpile, market price, or global counter, and does not synthesize an embargo
notification. The two price/trade loops remain the next frontier; existing canonical
`economy::{calc_market_prices,do_buy,do_sell}` functions own those transactions and should be
joined rather than copied when the parent mounts.

## Evidence and version policy

The focused suite pins all 33 codec positions, the complete `has_market` gates and early return,
parent short-circuit order, signed clamp/scarcity semantics, unavailable resource handling, and
three deterministic four-seat save/resume sequences. Those sequences mutate only the planning
owner; they grant no resources and execute no purchase, sale, or build completion.

This owner and the separate `rate[6]` owner are substantive candidates for one coordinated v15
Leader-row extension. The standalone evidence does not itself justify a format bump, so neither
owner is mounted or saved yet.
