# Leader conquest powers and setup queries

Cycle 9 closes two complete named `LeaderData` children used below
`production_ai_setup` and the market parent. The standalone implementation is
`crates/don-sim/src/systems/leader_tribe_bonus_runtime.rs`; it does not mount step 11 or
change save format.

## Canonical conquest-power owner

The PDB locates `BitMask<24> conquest_racial_powers` at `LeaderData +0x6D88`. Its exact
layout is an eight-byte `(bits,size)` visitor header, four bytes of flags/inline-storage
metadata, and the three-byte payload beginning at `+0x6D94`. Retail
`LeaderData::walk_data` persists the header and three payload bytes, while
`has_tribe_bonus` reads the payload directly.

The proposed canonical Leader-row extension is therefore exactly the three payload bytes:
3 bytes per row and 24 bytes across eight Leaders. It excludes BitMask size, flags, and
pointer/inline-history metadata. The current step-8 Leader owner already exposes the same
semantic bonuses as a decoded word for unit and wall consumers. A future v15 mount must
project these 24 bits into that existing owner and must not create a second mutable bonus
cache.

## `LeaderData::has_tribe_bonus` `0x006E1370`

The complete 133-byte child preserves retail's short-circuit order:

1. `Game +0x20` bit 2 disables every nation power without reading Leader facts.
2. Before victory begins, a Leader with `city_num == 0` has no bonus.
3. A negative `LeaderData::tribe` has no bonus.
4. The requested bit in `conquest_racial_powers` grants the bonus immediately.
5. Low-byte `leader_flags2` bit `0x40` suppresses the base-Tribe fallback.
6. Otherwise the requested bonus is compared with PDB field `Tribe +0x54`.

The receipt records each conditional read. Detached calls reject bonus indices outside the
24-bit retail table and refuse a missing resolved Tribe row only when execution actually
reaches the final fallback.

## `LeaderData::get_diff` `0x006EC000`

The complete 58-byte child uses `LeaderData::multi_diff +0x50`, which is already saved by
the canonical Leader row. `Game +0x820` bit 2 forces the raw per-Leader dword, including a
negative two's-complement value. Otherwise retail returns the global difficulty byte when
either `Game +0x821` bit `0x10` is clear or `Game +0x822` bit 2 is set. Only when the first
bit is set and the second bit is clear does it read `multi_diff`; a negative value falls
back to global difficulty and a nonnegative value is returned.

This distinction matters to `LeaderData::get_mod_resource_cap`: an exact `get_diff` result
of 0 scales caps to 50%, 1 scales them to 75%, and other values leave them at 100%.

## Evidence and version policy

Focused executable tests pin payload bit order and rejection, all tribe-child early gates
and read omissions, the complete difficulty flag matrix (including forced negative raw
return), exact `get_diff -> get_mod_resource_cap` composition, and
`has_tribe_bonus(4) -> market_speculation` admission. Four deterministic seeds each run a
four-seat save/resume comparison over future setup queries using the exact 24-byte owner
projection.

No test grants resources, completes production, forces a market transaction, or mutates a
shared Sim owner. The 24-byte payload is another substantive v15 candidate beside the
planning-rate, production-economy, and nuclear-embargo owners; this isolated evidence does
not bump or mount a save format on its own.
