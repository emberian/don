# Authoritative external-entity visibility owner frontier

## Why ATTACK remains red

`don-env::authoritative_backend` has a real `SimAttackIssue` route, but both its mask and apply
path stop at `TargetIdentityVisibilityUnavailable`. Its current observation exposes own units
only. Resolving a policy `target_entity` ordinal through scenario allocation order or an
omniscient `World` scan would disclose hidden identities and would let mask/apply disagree after
dense-row compaction.

`external_entity_visibility_frontier.rs` freezes the smallest source-only owner needed at that
boundary. It does not wire ATTACK, alter `don-env`, or claim a completion delta.

## Retail ground truth

The shipped EXE/PDB fix the Unit path:

| routine | VA | owned rule |
|---|---:|---|
| `UnitData::is_seen(int,int)` | `0x00607A60` | cloak/detection first, then current fog, then `ObjectData::visible` fallback |
| `UnitData::is_detected(int)` | `0x0060A630` | owner is detected; otherwise `seen3 & LeaderData::ally_mask` |
| `UnitData::is_cloaked()` | `0x0060A6A0` | instance `0x800`, type `0x4000`, instance2 `0x8000`, or type idle-cloak `0x40000` with no order |
| `WorldData::is_detected` | `0x006B48C0` | raw `seen3` intersection, with no see-all/reveal/fog-option short circuit |
| `WorldData::is_seen` | `0x006B55C0` | fog option, Leader see-all/reveal/territory, then current `seen` plane |

The decompiled `UnitData::is_seen(viewer, 0)` body also proves that instance
`unit_masks & 0x1000` bypasses the detection query, but not the later current-fog query. The
fallback after a World fog miss is direct bit `1 << viewer` in walked `ObjectData::visible +0x40`;
it is not the Leader vision mask. `tools/retail-control` already calls this same shipped leaf with
the force argument zero for measured Unit vtables and fails closed on unknown classes.

## Owner shape and admission seam

One installed frame owns:

- stable Sim `Handle {id,generation}` plus retail `{who,o,uid}` for each active Unit;
- the public observation fields needed to materialize an external row;
- all four cloak inputs, the detection-bypass bit, exact order presence, current `seen` and
  `seen3` cell bytes, territory owner, and the per-object visible mask;
- each viewer's exact `ally_mask`, see-all/reveal/territory policy, mutual-alliance territory
  mask, and the global fog option.

Installation validates the complete candidate before publishing it. Duplicate handles,
duplicate retail identities, inactive rows, invalid owner/object/type fields, missing self bits in
viewer masks, and malformed territory sentinels reject without changing revision or content.

Rows are canonicalized by `(handle.id, handle.generation, who, o, uid)`. A per-viewer projection
removes own rows and hidden rows, then assigns one-based ordinals in that canonical order.
`bind_target(viewer, ordinal)` returns an opaque token containing owner revision, frame, ordinal,
and exact identity. `revalidate_target` refuses after any frame refresh or reset. A future
`don-env` adapter can therefore use the same binding for verb masking and apply preflight, then
pass the stable Handle plus `{who,o,uid}` to the existing Sim attack transaction.

This owner establishes identity plus visibility only. Hostility, target validity, attack range,
and the existing attack order commit remain their own admission gates. The frontier is Unit-only;
Build/Wall/Animal virtual visibility paths must not be guessed from the Unit layout.

## Compaction, reset, save, and digest semantics

Dense World row indices are never retained. Reordering the same live rows produces identical
ordinals and owner digest. Removing/recycling an entity changes either the canonical set or its
Handle generation and invalidates outstanding tokens.

Reset clears the installed image and increments the revision even when deterministic scenario
setup will recreate identical handles. External projection and digest then return `Uninstalled`
until a complete post-reset frame is captured. This derived observation image is not independent
save state; save/load must rebuild it atomically from restored Sim, fog, diplomacy, order, and
canonical type owners.

The content digest includes frame, fog option, every stable/retail identity field, public field,
cloak/detection/fog input, and viewer policy in canonical order. It excludes the revision and
source dense order, so equal content has an equal digest. It is a local determinism/staleness
diagnostic only—not a claim of byte-exact retail `DataWalk` or a new checksum channel.

## Source-only validation command

Root convergence formatted the two Rust files. Persvati job
`rl-visibility-owner-20260810T000049Z-19232-9988-cf4a3c9ea7c5` passed all nine focused tests while
hbox remained unscheduled. The focused reproduction is:

```sh
tools/swarm-cargo-remote submit persvati rl-visibility-owner \
  --path crates/don-sim/src/systems/external_entity_visibility_frontier.rs \
  --path crates/don-sim/tests/external_entity_visibility_frontier.rs \
  --jobs 12 -- test -p don-sim --test external_entity_visibility_frontier
```

No Sim registration, retail launch, or live-process mutation was performed. ATTACK remains red
until this owner is captured from a real Sim frame and consumed by `don-env`.
