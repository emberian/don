# Lifecycle opcode cohort: the capital boundary

This tranche targets opcodes 70 (`Resign`) and 71 (`Quit`) without weakening the red
boundaries on 38, 41, 73, or 80.  The already-landed lifecycle host reaches
`Player::leave_game` and `Leader::defeat`; its last simulation callee is
`LeaderData::find_capital` `0x006EB930` in capital-elimination matches.

Evidence is the supported executable `ron-bin/riseofnations.exe`, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`, the shipped PDB
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`, and Capstone over
`0x006EB930..0x006EBA4A`.

The exact lookup is:

1. Scan `[0, LeaderData::city_mark +0x408)` in the departing player's City PtrArray and
   return the first active `city_flags & 0x10` row not equal to the skip pair.
2. If none exists, visit the other seven player arrays in slot order and return the first
   active City whose `was_capital_flags +0x68` contains the departing player bit.
3. Otherwise return `city = -1, who = departing who`.

The unusual pass-two validity guard is proven, not inferred: instructions
`0x006EB98A..0x006EB990` compute `departing_who * 0x6EEC` once, and
`0x006EB9A4..0x006EB9B0` reread that same leader's `leader_flags & 3` during every candidate
iteration.  Candidate validity is not consulted.  The receipt exposes this guard so a
candidate-index rewrite is mutation-visible.

`CityPool::city_mark` is the exact `LeaderData+0x408` owner.  The fixed 20-row backing
vectors are PtrArray capacity, not the scan length.  Malformed marks refuse before lookup;
retail cannot form them, but the local host must not let one make lifecycle mutation depend
on where a prior capital happens to be found.

This module only resolves `LifecycleBoundary::FindCapitalForDefeat`; all other boundaries
remain typed red. The real Bridge now accepts the Sim host's discharged receipt only when its
request, facts, source lifecycle decision, resolved plan, exact executed calls, and embedded
capital lookup proof all recompute. Non-Sim hosts keep the former Fleet callback unchanged.

Both wire rows are closed through that transaction. Opcode 70 is exercised with a present
former capital; opcode 71 is exercised through the no-capital self fallback and its distinct
post-defeat `playing = 0` continuation. A valid but changed CityPool between facts and apply is
an exact-CAS refusal that leaves Players, Leaders, and Match unchanged. A separately tampered
lookup proof is rejected before mutation and proves the same rollback invariant.

DoNSave v12 preserves every CityRecord field, City PtrArray logical metadata, per-owner
`city_mark`, and the leader city-capture counters. A commanded capital Resign is saved after
the real Bridge transaction, loaded, resaved byte-identically, and reproduces its checksum
channels. Formats v7 through v11 retain their previous bytes and accept only the pristine
CityPool/zero-counter image.

Opcodes 38, 41, 73, and 80 remain red: this tranche does not manufacture their declaration,
diplomacy/resource, object/unit cascade, or ungraceful-drop branches.
