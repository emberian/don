# Replay Groups fixed-pool normalization cone

Status: bounded exact detached transaction required at frame 384 before the first changed
recorded checksum at frame 391.

## Exact 2024 witness chronology

The source is
`ron-data/replays/multi/Playback___2024.02.23_20_49_35__Fri_.rcx` (SHA-256
`1690431a5ef19b38a3425d3dd7311e8e83ca0d27c56fabe49d776a9f1421b251`). The
instruction-derived parser `re/scripts/rcx_parse.py --commands` gives:

| lockstep serial | processing frame | relevant package contents | embedded Groups word |
|---:|---:|---|---:|
| 64 | 379 | Group owner 0 objects `[3,4,5,6]`, then MoveTo `(5186,72095)` | `0x1c78f3f5` |
| 65 | 385 | next checksum | `0x1c78f3f5` |
| 66 | 391 | next checksum | `0x22a5074d` |

Retail playback `CommandManager::process_turn` (`0x0093EF10`) reads each due package, runs
`CheckSums::check_all`, and only then calls `CommandPackage::process_all`. The first RCX dword is
the live `Game.frame` recorded when retail processes the package; the package's fourth dword is
its lockstep serial. Therefore serial 64 is checked and then applied at frame 379. Its embedded
checksum is an earlier package-send-time observation, not a post-command image. In particular,
the unchanged embedded word at frame 385 does **not** prove that the live Groups pool is clear.

Retail `Groups::process` advances `proc_group` through one of 64 slots per simulation frame.
Independent SVX evidence establishes `proc_group == Game.frame % 64`; therefore slot zero is
visited at frame 384. At that point serial 64's Group+Move has already installed slot zero, so the
pass reaches the nonempty normalization/recompute tail. Exact frame-379 command publication and
an exact current-state frame-384 authority are both required before the changed frame-391
checkpoint can be evaluated.

## The bounded frame-384 cone

`systems::groups_process_authority` prepares the fixed-pool pass as detached before/after
images and commits only if the complete 512-slot pool is unchanged. It composes existing owners:

1. `World` resolves Unit-band `(who,o)` addresses, active bits, stable Handles and live Unit
   backlinks.
2. The current `GroupMoveAuthority` supplies Handle-bound Unit role, captain/on-map formation facts,
   and the dynamic `UnitData::speed` result.
3. `GroupData::normalize` preserves retail's reverse scan and six-plane compaction.
4. The canonical Group-Move host's shared recomputation performs `find_role`, `find_leader(0)` and
   the two speed writes.

The object branches are from the supported executable:

| evidence | result |
|---|---|
| `Groups::process` `0x006FA210`; `Group::normalize` `0x00711540` | active bit, virtual `+0x18`, Unit backlink `+0x80`, then virtual `+0x20` |
| UnitData vtable `0x00B41B08`, slot `+0x20` -> `0x0041BFF0` | constant false; a live Unit remains subject to its backlink |
| BuildData vtable `0x00B426DC`, slot `+0x20` -> `0x0041E0E0` | constant true; a Build-band member is removed |
| `Group::find_role` `0x007081F0` | OR the retained Unit type roles, zeroing the prior value first |
| `Group::find_leader` `0x0070CCB0`; `UnitData::speed` `0x0060AAE0` | lowest-category captain, on-map pass then fallback; write `speed` and `new_speed` |

Wall-band members remain a typed refusal because this tranche did not derive their virtual
`+0x20` result. The plan also refuses a retained current Unit whose Handle is absent from the
supplied authority. Dead/tombstoned and wrong-backlink members need no invented type facts.

## Integration boundary

The transaction is intentionally not wired to `Sim::do_frame` without a current-state authority.
`MoveMemberAuthority::speed` is a dynamic same-state answer: terrain, relations, Leader state,
heroes and Constants can change it. The frame-379 setup/command authority therefore cannot
silently authorize the frame-384 scheduler pass. The replay driver must regenerate and bind
land-speed/Group authority from the canonical frame-384 Sim, then commit this prepared transaction
at the exact step-12 position.

`don-replay::setup_2024_frame384_groups_process::mount_frame384_groups_process` is that fail-closed
join. It requires the exact serial-64 mount receipt plus an independently source-backed Sim at the
instant of frame 384's Groups callback. It verifies replay/setup/chronology digests, World/RNG,
active Leaders, command-cache revision, slot-zero cursor, and equality with serial 64's complete
Groups after-image. It then rebinds all seven setup identities at frame 384, regenerates land speed
and complete live Unit authority from that same immutable state, and commits this transaction.
No recorded checksum is an input.

The live scheduler's conservative `Keep`/no-speed callbacks remain checksum-relevant drift when
that chronology authority is absent. The real 2024 corpus cannot enter this adapter yet because
the Great Lakes mode-5 worldgen boundary prevents a real setup receipt, frame-0 Sim publication,
and the exact frame-0-to-384 chronology. The mount therefore narrows the first residual without
claiming survival beyond 64.

## Gates

```sh
cargo test -p don-sim --test groups_process_authority -- --nocapture
cargo test -p don-sim --lib groups_process -- --nocapture
cargo test -p don-replay --lib setup_2024_frame384_groups_process -- --nocapture
cargo test -p don-replay --test setup_group_move_authority -- --nocapture
```
