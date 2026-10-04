# Replay Leaders live tribe frontier

This extension closes the first fixed-body gap exposed by `leaders_runtime_frontier` without
installing a Leaders checksum producer.

## Exact writer and field

Instruction-level disassembly of shipped `riseofnations.exe` identifies the active setup store:

```text
006e393f  mov esi, dword ptr [ebp+0x0c]   ; Leader::init tribe argument
006e3af9  mov dword ptr [ebx+0x08], eax   ; who
006e3aff  mov dword ptr [ebx+0x0c], esi   ; LeaderData::tribe
006e3b0f  mov dword ptr [ebx+0x14], -1    ; gov
```

The setup replay owns the selected tribe and its exact Player-body source span, but setup alone is
not a live-state claim. The smallest existing current owner is
`bhs_type_table::TypeBuiltinState::leaders[8]`: retail builtins 815..=819 query its live
`leader_flags` and `tribe`, and their mutations change type masks rather than tribe.

`leaders_runtime_tribe_frontier::bind_live_tribes` therefore requires three coherent views:

1. the replay-bound `InitialLeaderPrefix` row and Player body;
2. the current validity gate in `RuntimeLeadersFrontier`;
3. the current `LeaderTypeMasks` row in the canonical `TypeBuiltinState` owner.

An active live tribe must always be in `0..24`. A replay selector in `0..24` names a concrete
tribe and must equal that live value. Replay selector `24` is the observed one-past-the-table
random-nation sentinel: it retains the exact replay source lineage, but the resolved tribe dword
is owned solely by current `TypeBuiltinState` and may be any value in `0..24`. A replay selector
above `24`, roster drift, concrete-tribe drift, a missing replay source, or a changed base-frontier
shape refuses before an extension is returned.

## Coverage delta

The extension owns exactly four new bytes per active Leader. More importantly, filling `+0x0c`
makes the already-owned `defeated_by` dword at `+0x10` reachable by the exact prefix walk:

```text
old boundary: LeaderData +0x0c  tribe
new bytes:    [0x0c,0x10)       tribe
unlocked:     [0x10,0x14)       defeated_by (already live-owned)
new boundary: LeaderData +0x14  gov
```

For `n` inactive rows before the first active row, the lawful Adler prefix advances from
`8*n + 12` bytes to `8*n + 20` bytes. Only four bytes are newly owned; the eight-byte prefix delta
includes the four bytes which were already owned but unreachable beyond the former gap.

## Scoreboard boundary

`checksum()` still returns `Err(LeadersWalkFrontier)` for every real replay setup because `gov`
and many later fixed/dynamic fields remain absent. The module has no `SimState` installation hook,
and `installed_in_scoreboard()` is pinned false. Thus registration changes neither
`leaders.substantive_compares` nor `leaders.substantive_matches`; both remain zero.

A future installer must require a complete traversal (`checksum() == Ok`) and the ordinary exact
producer gates. The partial tribe extension itself can never satisfy them.
