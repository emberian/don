# Armies replay/save runtime frontier

This tranche audits the earliest exact `Armies` walk boundary without adding a replay
checksum channel. The result is intentionally an orphan save/log checksum producer.

## Reachability finding

`CheckSums::check_armies` `0x00936CF0` is a 15-byte thunk to `Armies::walk_data`
`0x006F3700`. It has no callers in the shipped executable. `Armies::walk_data` is reached by
the whole-game save/load verification and `GameLog::say_checksum`, but not by
`CheckSums::check_all` `0x00936560`. The latter's fifteen wire channels remain exactly the
ones in `don_replay::check_all::WALKERS`; Armies is not a sixteenth replay channel.

This does not make Army divergence harmless. Army state drives Groups and Unit orders, so
its effects eventually enter the Groups, Guys, and Units channels. The Army state itself is
simply invisible to the per-turn checksum tuple.

## Earliest exact authority

The first complete boundary is immediately after `Armies::init` `0x006F3C10`:

- eight owner `PtrArray<Army>` records;
- sixteen preallocated, non-null Army pointers per owner;
- array length and capacity both 16, increment -1, and masked flags zero; and
- 128 complete `ArmyData` owners in `don_sim::systems::armies::Armies`.

`armies_runtime::armies_walk_bytes` accepts only that fixed 8-by-16 shape. A different
shape is refused because Rust `Vec` does not preserve the retail array's capacity,
increment, flags, or null-pointer topology and no other authority supplies them.

For every owner, the exact stream is:

```text
length:i32 = 16
capacity:i32 = 16
increment:i16 = -1
flags:u8 = 0                         # persistent flags &= 0xbf
pointer_present:u8[16] = [1; 16]
capacity:i32 = 16                    # repeated by PtrArray::walk_data
increment:i16 = -1                   # repeated by PtrArray::walk_data
Army::walk_data for slots 0..15
```

An invalid Army hashes only `valid:i16`; a live Army hashes the full 152-byte `ArmyData`
image. Fresh post-init state therefore walks `8 × (33 + 16 × 2) = 520` bytes. Each live
Army adds 150 bytes.

## Shared hooks deliberately not changed

The existing `don_sim::systems::armies::Armies::walk` and `walked_len` omit the sixteen
pointer-presence bytes and the repeated capacity/increment pair for each owner. Correcting
those shared helpers is a narrow future Sim change; this tranche uses an independent exact
walk so it does not collide with active mechanics work.

No `don-replay/src/lib.rs`, `SimBridge::PRODUCES`, `CHANNEL_SOURCE`, or replay-state hook is
changed. Installing Armies as a replay channel would contradict the shipped call graph.

## Evidence gates

Focused tests freeze the 520-byte initialized image, both container-history passes, all 128
presence bytes, full live-Army recursion, dormant invalid-state exclusion, mutation
sensitivity, malformed-shape refusal, and absence from the fifteen-channel walker table.
There is no retail match claim and no VM observation in this tranche.
