# 2024 frame-1 Citizen idle continuation after `Unit::set_anim`

Status: **exact detached continuation implemented; replay closure remains red**. A valid adjacent
SetAnim capture can now be composed through the reached local `do_idle -> check_idle` writes for
the supported Citizen shape. The repository still has no real frame-1 SetAnim capture, and the
following `Unit::think`/FindBuilds cone is not implemented.

## Instruction order and reached branch

The supported retail executable is SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
After `Unit::set_anim(0,0,1)` returns, `Unit::do_idle` executes:

| order | instruction | exact effect |
|---:|---:|---|
| 1 | `0x0060dd51` | clear the Unit's signed 16-bit `collide` field to zero |
| 2 | `0x0060dd58` | call `Unit::check_idle` at `0x006032c0` |
| 3 | `0x0060dd5f` | call `Unit::think` at `0x005f6e40` |
| 4 | `0x0060dd68` | restore temporary `unit_masks2 & 0x8000`, after Think returns |

For the exact supported Citizen entry, `idle == 1`, `unit_masks2 & 0x800 == 0`, and the object
idle bit (`object_flags & 8`) is already installed. `check_idle` therefore increments idle
`1 -> 2` at `0x006032f8` and reaches Think. Its SetAngle branch requires the incremented value
to equal four, so SetAngle and entrenchment are unreachable on this path. A missing object idle
bit requires the source-dependent type/Leader query and is not inferred.

## Detached atomic composition

`setup_2024_frame1_set_anim_continuation::continue_frame1_citizen_idle_after_set_anim` accepts a
complete frame-1 entry authority, an `IdleCitizenPrefixPlan`, and its adjacent
`Frame1IdleSetAnimReceipt`. Before using any public plan field it:

- recomputes the prefix from its complete before-image and requires byte-for-byte plan equality;
- validates the receipt's exact request, whole Guy before/after images, per-Guy call journal,
  whole-Sim capture hashes, Handle generation, RNG continuity, and composition digest;
- requires the receipt receiver to equal the detached prefix receiver and the temporary
  `0x8000` restoration obligation to be armed;
- merges only the receipt-owned `UnitGuys` and game RNG, then journals `collide -> 0` and, on the
  reached branch, `idle 1 -> 2`.

The result is another detached plan. No canonical `Sim`, Leader, option, World, or save-owned row
is mutated. Its digest covers the complete prefix images, receipt digest, postimage, ordered
write journal, and every field of its typed open request. The public validator recomputes the
entire result, so a stale prefix, stale receipt, changed journal, postimage, or request is rejected
as one transaction.

The exact branch stops at `IdleCitizenThinkRequest`, carrying the complete post-check_idle image
and `restore_mask2_bit8000 == true`. It neither restores the bit nor applies pending Leader state.
Both actions remain part of a later all-or-nothing Think continuation.

Non-golden shapes stop before any `check_idle` write with a complete
`IdleCheckIdleFactsRequest`, classified as `NonGoldenIdle`, `EntrenchFacts`, or
`ObjectIdleTypeQuery`. This preserves the source-dependent boundary instead of manufacturing a
SetAngle, entrenchment, or type-query result.

## Remaining red boundary

Replay closure remains red until a supported-retail trace supplies the actual frame-1 SetAnim
receipt and a Sim-backed Think binder consumes `IdleCitizenThinkRequest`. Think still owns its
Leader mirrors/options, position and Constants dependencies; any reached `find_build_spot ->
Objects::find_builds` call must return its complete ordered scratch-array mutation before the
temporary `0x8000` bit can be restored and the whole Citizen transaction can commit.
