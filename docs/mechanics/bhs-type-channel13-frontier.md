# BHS mutable type channel-13 frontier

Status: integrated into the canonical type runtime and opaque BHS session; independent builds,
the Constants/Balance/Tribes continuation, and DoNSave encoding remain pending. The source and
proof packs are
`crates/don-sim/src/systems/bhs_type_channel13_frontier.rs` and
`crates/don-sim/tests/bhs_type_channel13_frontier.rs`; executable owner integration is frozen in
`crates/don-sim/tests/bhs_channel13_integration.rs`.

## Result

The frontier joins the canonical `TypeBuiltinState` to the exact Type prefix of retail checksum
channel 13 without hashing Rust layout or a raw C++ heap image.  Its input is normalized into only
the ranges actually visited by `walk_rules_data`, followed by the exact two
`SimpleArray<unsigned short>` walks for ObjectType-derived rows.  `String` bodies, vptrs, backing
pointers, padding, and derived caches have no byte-shaped fallback.  Three explicit proofs are
required before projection:

- `String::walk_data` is checksum-gated and emits no bytes;
- `Types::walk_rules_data` dispatches each shipped slot through its fixed most-derived walker;
- only the named scalar ranges and ObjectType arrays are admitted, with all cache/pointer bytes
  absent from the normalized source.

Missing any proof is a typed error.  Missing a range, an unknown dynamic kind, an out-of-order
slot, a malformed array header/payload, or a relation-array mismatch also fails closed.
Every row also retains exact pristine `name`, `display_name`, and `type_name` values outside the
checksum byte stream.  Unknown Strings are rejected; immutable lookup/type-name drift is always
rejected, and pristine display-name equality is required whenever the owner claims to be clean.

## Exact retail traversal recovered

Ground truth remains `ron-bin/riseofnations.exe`, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`, and the PDB-backed
procedure table.

| function | VA | normalized visited bytes |
|---|---:|---|
| `Types::walk_rules_data` | `0x00669800` | 806 virtual calls in global TypeIndex order |
| `Type::walk_rules_data` | `0x00663190` | `[+4,+94)`; following String is checksum-gated |
| `ObjectType::walk_rules_data` | `0x0065FBA0` | base, `[+484,+636)`, arrays at `+0x27C/+0x298` |
| `UnitType::walk_rules_data` | `0x0061D190` | Object plus `[+692,+716)`, `[+724,+732)`, `[+732,+736)`, `[+736,+1492)` |
| `BuildType::walk_rules_data` | `0x00631F50` | Object plus `[+692,+741)` |
| `TechType::walk_rules_data` | `0x0066D5C0` | Type plus `[+456,+483)`; eight Strings gated |
| `SpellType::walk_rules_data` | `0x00675400` | Type plus `[+456,+504)` |
| `GoodType::walk_rules_data` | `0x0066FAB0` | Object plus `[+692,+760)` |

The fixed slot order and cardinality are:

| slots | effective walker | rows |
|---|---|---:|
| `0..50` | GoodType | 50 |
| `50..414` | UnitType | 364 |
| `414..543` | BuildType | 129 |
| `543` | ObjectType (ItemType inheritance) | 1 |
| `544..629` | TechType | 85 |
| `629..684` | SpellType | 55 |
| `684..806` | Type (BonusType inheritance) | 122 |

This is intentionally stronger than validating seven aggregate counts: swapping a Unit and Build
row is rejected at the first slot even though the totals remain unchanged.

The live read-only retail walk established **473,984 Type bytes**, **2,363 ObjectType array
elements**, cumulative `after Types = 0x72E0C3B6`, and the complete channel total described in
`rules-channel.md`.  `ShippedRetail` mode requires all three Type checkpoints.  An exact mod
composition may use its independently retained pristine Type checkpoint, but it keeps the same
806-slot virtual bands and provenance gate.

## Canonical owner overlay

The projection begins with one complete pristine normalized source and overlays every
checksum-visible field owned by `TypeBuiltinState` at its executable/PDB offset:

- common Type fields: `type +4`, `job_time +8`, `tribe_mask +16`, six costs `+24..+48`, three
  prerequisites `+48..+60`, `from +60`, `where +64`, `modified +88`, and grid bytes `+92/+93`;
- Object fields: attack `+488`, ranges `+504/+508`, hits/armor `+528/+532`, and LOS/science LOS
  `+540/+544`;
- Unit fields: moves/turn speed `+704/+708`, mana/control cost `+748/+752`;
- Build fields: town hits `+692`, most shots/garrison/base arrows/wonder `+708..+724`, and
  plunder value/good `+724/+728`.

For every ObjectType-derived row, the pristine non-strict array must exactly equal the canonical
owner's ordered `is_list`; the strict array and both array headers remain provenance-bound
pristine input.  BHS does not mutate those arrays, so copying one side over the other would hide
an owner split.

## Provenance, clean/dirty, and persistence

The pristine normalized table and installed owner receipt must carry the identical
`TypeBuiltinProvenance`: composition, ordered manifest, type-row component, tribe roster, and
Leader-mask digests must all be nonzero and match.  Projection also requires an explicit dirty
bit and exact mutation revision.  Unknown values, a stale receipt, `dirty=false/revision>0`, or
`dirty=true/revision=0` are refused.

A clean owner must project byte-for-byte to its pristine Type checkpoint.  This catches an
unreceipted direct scalar write.  Its separately retained pristine Strings also catch an
unreceipted checksum-neutral display-name write.  A dirty owner is allowed to retain the pristine checksum—for
example registration 288 mutates `display_name`, whose String is gated out of channel 13.  That
case is why `ProjectedTypeRules` does not expose only a digest: its `persistence_owner()` retains
an immutable borrow of the entire `TypeBuiltinState`, exact provenance, dirty bit, revision, and
projected checksum together.  This is the minimal save ownership contract, not a claim that the
current DoNSave v6 wire format can serialize it.

Leader `tech` and `obs_flags` masks remain outside channel 13, but they remain inside the borrowed
canonical owner for future persistence.  They are not folded into Leader checksum channel 8.

## Proof pack

The source-only proof pack covers:

1. exact 806-slot ordering, seven walker bands, 2,363 array elements, and 473,984 walked bytes;
2. common/Object/Unit/Build overlay offsets including signed grid-byte bit patterns;
3. checksum-neutral rename retaining its dirty String in the persistence owner;
4. canonical non-strict relation versus pristine ObjectType-array equality;
5. missing String values or String/virtual/cache proofs;
6. virtual-kind and slot-order substitutions that preserve aggregate counts;
7. provenance mismatch and unknown/ambiguous dirty-revision receipts;
8. clean scalar/String divergence and shipped array-cardinality refusal.

The integration authoring lane remained source-only and did not run Cargo, a remote harness, or
retail. Root convergence owns formatting and independent execution of the focused commands below.

## Integration map

1. `systems/mod.rs` now registers the frontier and the proof pack imports that same canonical
   library module rather than compiling a path-local duplicate.
2. Have the synchronized rules/mod composer emit normalized visited segments and exact
   ObjectType array metadata/payloads under the same `TypeBuiltinProvenance` used by
   `BhsSession`.
3. `TypeBuiltinRuntime::new_with_channel13` now validates and stores that immutable source beside
   the installed `TypeBuiltinState`. `BhsSession::new_with_channel13` is the opaque setup entry;
   state-only compatibility construction stays explicitly projection-unowned.
4. Every checksum admission now calls `project_type_owner` against the current dirty bit and
   mutation revision. `BhsSession::type_channel13_checkpoint` exposes the exact cumulative
   checkpoint, while `type_persistence_owner` retains the full canonical owner/provenance/revision
   contract. Next feed its 806 projected rows into the existing
   `don-replay::rules_channel` Types prefix before Constants, Balance, and Tribes.  Compare the
   normalized frontier traversal to the existing walker byte-for-byte once, then keep one
   implementation.
5. The former unconditional BHS admission refusal is now conditional: a source-owned live
   projection passes; missing sources and rejected live projections remain typed red boundaries.
   This admits the existing non-retail partial Sim digest but does not pretend that checkpoint is
   the complete retail Rules channel.
6. Extend the next DoNSave format with the complete canonical mutable owner plus provenance and
   revision, and restore it atomically before scripts resume.  DoNSave v6 remains red.
7. Independently validate the focused pack in separate compute lanes, then compare a pristine
   shipped projection against `0x72E0C3B6` and a narrow live post-mutation capture before marking
   channel 13 complete.

## Focused convergence commands

Run the canonical frontier and session integration together so the module registry, setup gate,
live mutation projection, persistence borrow, state-only refusal, and DoNSave-v6 refusal are all
compiled in one crate shape:

```text
cargo test -p don-sim --test bhs_type_channel13_frontier --test bhs_channel13_integration
cargo test -p don-sim --test bhs_session_owner --test bhs_type_runtime_integration --test bhs_type_stat_integration
cargo check -p don-sim --all-targets
```

Repeat the two focused test commands with `--release`. None of those commands supplies the still
missing production composer or a live retail post-mutation capture.

Root's first local production-shape pass after integration completed the frontier and session
packs **14/14** (11 frontier plus 3 session integration). Release-mode, all-target, remote-host,
and retail capture gates remain pending.
