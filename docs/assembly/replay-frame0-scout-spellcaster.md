# Golden frame-zero Scout spellcaster frontier

Status: source-exact detached transaction for the supported 2024 human Scout path. Replay Rules
plus an adjacent call-entry image now close `SpellTypeData::is_castable`, `UnitData::mana`, and
`SpellTypeData::get_range`. A complete ordered `ObjectsData::find` spatial traversal remains a
typed receipt. After a target hit, the successful Unit-order cone now owns
`OrdersMemManager::get_obj(14)` through its exact pool-14 clean-stack prefix and stops before the
next child on the recycled branch. For an exactly empty pool, it also owns the
`get_new_order(14)` switch prefix and stops before CRT allocation.

## The branch correction

`Unit::think_spellcaster` is `0x005F27A0..0x005F2EDE`, 1,854 bytes, SHA-256
`fafa5e595e78e840dc8369293fb56909638d41e458fd389354f7d2f987533904`. Its first branch is
decisive:

```text
0x005F27AC  who = zero_extend(Unit+0x09)
0x005F27B6  test LeaderData[who].flags, 4
0x005F27BD  je   0x005F28CF
```

The owner-zero Scout in the supported replay belongs to a human leader, so bit 4 is set and
execution falls through to `0x005F27C3`. The later `0x005F28CF` arm—difficulty selection, Spy
checks, Sniper 641, `Random::get(0,65535)`, signed `% 3`, and `Object::get_army`—is the
**non-human/AI** arm. None of those calls or draws occur on the golden human Scout path.

This distinction is checksum-critical: a port that runs the later arm for a human Scout consumes
at least one false RNG draw after a rejected Sniper attempt.

## Exact human arm

The admitted receiver is stable identity `(who=0,o=0)`, Scout type 69. Its ordinary
`UnitData::is_special` vtable slot is `0x0046CEA0`, whose complete body reads
`UnitTypeData+0x2B8 & 0x10`. The replay Rules row is bound as `unit_flags2=0x12`, domain zero,
and base mana 500; Counterintel's replay-carried mana cost is also 500. The path is:

```text
is_special()                                                     0x005F27C3
if false: return 0

counterintel = TypesData slot +0x09DC, TypeIndex 0x277 (631)
if !counterintel.is_castable(o, who, 0): return 0                0x005F2804

capacity = unit.mana()                                           0x005F281B
if sign_extend(Unit+0x96) + counterintel[+0x1D0] > capacity:
    return 0

range = counterintel.get_range(&TypesData.slot_09DC, who,-1,-1)  0x005F2875
target_o = ObjectsData.find(decoded_x, decoded_y,
                            SearchIndexBH(0), who, range,
                            &TypesData.slot_09DC, FilterIndex(20),
                            631, who, <three unread words>, 0)     0x005F2884
if target_o < 0: return 0

unit.add_cast_order(target_o, ObjectsData[+0x200],
                    decoded_x, decoded_y, 631, 0, 0)              0x005F28BE
return 1                                                         0x005F28C3
```

Coordinates are the signed dwords at `ObjectData+0x10/+0x14` decoded with `^ 0x00063637`.
The three `ObjectsData::find` words at argument positions 10--12 are compiler alignment storage:
the complete 964-byte callee never reads `[ebp+0x2C..=0x34]`. The typed request records them as
unread instead of inventing values.

For Scout 69, `UnitData::mana` returns the exact `UnitTypeData+0x2EC` base. Scout is not domain 2,
its direct `is_supply` result has no `+0x40` flag, and the only later ground modifier is restricted
to General type 54. The comparison is signed and includes the signed-short `mana_burn` value.

## Closed static children

`SpellTypeData::is_castable` is `0x00675BC0`, 2,033 bytes. Counterintel's replay Rules row has
`from=Spy(58)` and `from2=Scout(69)`, so the live type-69 receiver takes the second direct
`ObjectData::is` match. Its Unit vtable `+0x18` is the true `is_unit` stub at `0x0041E0E0`.
When live `UnitData+0x68 & 1` is clear, type 631 falls through the function's default return `3`.
When that bit is set, retail calls `TypeData::is_pack` `0x00470540` and `is_unpack`
`0x00470510`; Counterintel is neither packed nor unpacked, so the result is zero. Neither path
writes state or consumes RNG.

The exact replay-carried Counterintel flags are `0x10B6`: the XML letters `febchm` set bits
5, 4, 1, 2, 7, and 12. This value and the exact serialized type/spell spans are retained in the
Rules authority; `0x30` is not a lawful substitute.

`SpellTypeData::get_range` is `0x00676A80`, 435 bytes. For type 631 and target `-1`, its result is

```text
replay_range
  + LeaderData::get_spy_upgrade() * Constants.spy_bribe_upgrade_range * 192
  + has_wonder(TerraCotta=0x211) * Constants.terra_cotta_range * 192
```

The supported Rules provide `replay_range=1920`, Spy upgrade range `2`, and Terra Cotta range
`0`. The live Spy upgrade and Wonder answer still come from the adjacent call entry; the Wonder
call is retained even though its shipped contribution is zero. Arithmetic uses retail wrapping
signed dwords.

## Global search scratch

Despite its PDB `const` signature, `ObjectsData::find` initializes two global scratch words before
enumeration:

| word | entry write | later meaning |
|---|---:|---|
| `ObjectsData+0x1FC` | `0x05F5E0FF` | best metric |
| `ObjectsData+0x200` | query owner (`who`) | selected target owner |

A no-target return is exactly `-1` and retains those entry values. A target return is nonnegative;
the second word supplies `add_cast_order`'s target-owner argument. The Rust transaction accepts
this mutation only through a revision- and call-entry-composition-bound retail receipt. That
receipt must also carry a nonzero SHA-256 over the complete ordered spatial-cell candidate chain
and every live field read by SearchIndexBH(0), FilterIndex(20), and Counterintel
`is_valid_target`; an opaque result without that traversal identity is rejected. The mutation is
staged until atomic commit.

Linear reversal of `ObjectsData::find` (`0x0065C6B0`, 964 bytes) proves the exact remaining cone:
SearchIndexBH(0) accepts the enumerated active objects, FilterIndex(20) dispatches through
`Search::valid_filter` table entry 19, and that entry first calls Counterintel
`SpellTypeData::is_valid_target`. Candidate distance is retail `vector_dist`; a later candidate
replaces an equally distant earlier candidate. The first source not yet mounted in Don is the
complete ordered live spatial-cell candidate chain together with candidate type/owner/mask,
infiltration, and diplomacy facts. The code freezes that precise authority rather than guessing a
no-target after-image from setup state.

## Unit order write, not Caster active-spell write

`Unit::add_cast_order` is `0x005E4A60`, 541 bytes, SHA-256
`6acb03ec4d923263a59111d4b84f54c9f7dcffb4540def5a946bd6acdd327158`. For
Counterintel 631 and queue position zero, retail skips the queue-2 close prefix and both
PACK/DEPLOY canonicalization arms, then:

- allocates `OrderIndex 0x0E` through `OrdersMemManager::get_obj` `0x00730AC0`;
- writes target object/owner at CastOrder `+0x08/+0x0C`;
- reads the live target's UID word at `ObjectData+0x30` into CastOrder `+0x10`;
- writes decoded coordinates at `+0x14/+0x18`, zero at `+0x1C`, and spell 631 at `+0x20`;
- clears the order byte's `0x04` bit because the final argument is zero;
- links the order into `Unit+0xCC` through `LinkListBase::add` `0x0046D5A0`;
- because queue position is zero, does **not** call `Unit::close_orders`, calls
  `Unit::clear_partial_path` `0x005E3920`, rewrites the `Unit+0xDC` current-order link to its first
  pointer, and reaches `Unit::update_action` `0x0060A870`.

The first child is exactly `push 0x0E; call 0x00730AC0` at `0x005E4BA3/0x005E4BA5`.
`OrdersMemManager::get_obj` is 236 bytes, SHA-256
`055847edeebe03094c7e795bb0d1de368027ad9f97e5533a0ed9bec4ba462586`. It indexes the
pool array at `0x00EB4390` with 32-byte stride, so pool 14 starts at `0x00EB4550`. The exact
branch chronology is:

- length zero: no pool write or slot read; `mov ecx,edx; call get_new_order` at
  `0x00730B27/29`, where saved `edx` is OrderIndex 14;
- positive length: decrement first, then read `free_array[new_length]`;
- negative nonzero length: write one, decrement to zero, then read `free_array[0]`;
- null popped pointer: fall through to the same `get_new_order(14)` child;
- nonnull pointer: call the UnitOrder vtable slot `+4` at `0x00730B10`, then return that pointer.

For a valid recycled pool-14 node, the UnitOrder secondary vtable is `0x00B4976C`. Its slot `+4`
targets the eight-byte `CastOrder::clear` adjustor thunk `0x00486091`, SHA-256
`7dbba2bd7b5cebd3608b659b5f1b8f63a5236a301c4dbf0b0612b3a467e323f1`, which enters the
63-byte body `0x00486350`, SHA-256
`239eceae02ed1b1b811de41e7af9a0e17d892ca6f1f5a99630447abfe010f70a`. The fresh child
`get_new_order` is `0x00730550`, 1,392 bytes, SHA-256
`921305ab491316cd0e767bef3b88b1622f42c05470232fea1f90ece7984d4eae`.
On the exact zero-length branch, its ECX input is 14. The prologue decrements that to switch index
13 and jumps through table `0x00730A54`, entry `0x00730A88`, to case `0x00730837`. That case's
first child is exactly `push 0x30; call [0x00AC54F0]` at `0x00730837/39`; the IAT slot is the
retail CRT `malloc` import. There is no simulation-owner write before this call. The Rust
transaction stops before malloc and therefore supplies neither a heap result nor the later
CastOrder constructor state.

The allocation, target-UID read, list insertion, path clear, current-order/link mutation, and
action/Guy after-image therefore form one product-host transaction. The Rust frontier binds the
whole pool, clean-array, selected slot, and recycled-node identities. It stages the exact length
normalization/decrement and pop, but exposes no partial pool commit. It stops before either
`CastOrder::clear` or, on the zero-length path, CRT `malloc(48)` inside `get_new_order(14)`. A
null popped slot still stops at the `get_new_order` entry because that path has already staged a
pool write. Every residual retains the exact later AddCastOrder continuation.
Any caller, pool, list, or node change fails stale validation without publishing the staged search
scratch/pop or touching allocator, Unit, order, path, Guy, World, RNG, or Caster state.

`get_obj` itself does not allocate or read an object UID. The target UID remains the later live
`ObjectData+0x30` read in `Unit::add_cast_order`; the typed continuation preserves that requirement
instead of inventing a UID inside the allocator.

There is no access to `CasterData::active_spells` in `Unit::think_spellcaster` or this
Counterintel `add_cast_order` arm. A queued CastOrder is work for the Unit order dispatcher; it is
not an `ActiveSpell { type,start,end }` in `CasterData+0x04`. Therefore the existing frame-one
`Caster::process_spells` authority composes as follows:

| frame-zero result | Unit order/path | Objects search scratch | RNG | frame-one active spells |
|---|---|---|---|---|
| not special / not castable / insufficient mana | unchanged | unchanged | unchanged | unchanged |
| search finds no target | unchanged | sentinel/query-owner write | unchanged | unchanged |
| search finds target | residual before recycled clear, or before CRT malloc on the zero-length pool path; pool pop/allocation and full CastOrder suffix remain atomic | selected metric/owner staged only | unchanged | unchanged |

In particular, a setup-empty Caster array remains empty across this function on every branch.
That is a proof from the reached write set, not a replay-state guess.

## Implementation and gate

`crates/don-replay/src/groups_pre_pair_unit_authority.rs` projects the exact Scout UnitType row,
Counterintel SpellType row, and two Constants fields from the admitted replay Rules span.
`crates/don-sim/src/systems/frame0_scout_spellcaster.rs` binds those spans and digests, executable
and replay SHA, stable golden identity, independent adjacent call-entry composition, complete
Objects request, search scratch, and all checksum-relevant invariant revisions.
`prepare_golden_scout_spellcaster` is pure. A complete no-cast result can be installed only by
`commit_no_cast`, which revalidates the whole before-image. A target hit returns an
`OrdersGetObjectRequest` for the exact pool-14 child, with the full `AddCastOrderRequest`
continuation nested inside it. `prepare_orders_get_object` then accepts only a composition-bound
retail pool/list/node capture and returns the exact next-child request plus an uncommitted pop
delta. `prepare_empty_pool_get_new_order14` accepts only the no-pop zero-length result, validates
that result by recomputation, and returns the exact 48-byte malloc request with no allocator
return value.

The focused gate pins both top-level branches, the owned castability result, signed mana gate,
wrapping range formula, complete find ABI, no-target scratch, zero/positive/negative pool lengths,
null and recycled slots, exact fresh/clear child identities, the unopened target-UID continuation,
the index-14 jump-table row and CRT malloc ABI, Rules/call-entry/traversal/recycler provenance
refusal, stale no-publication, and atomic no-target commit invariants.

## Remaining golden evidence

This closes the static child results and prevents a false Caster/RNG dependency, but it does not
invent the golden result of the spatial `ObjectsData::find` traversal. An adjacent retail capture
must supply that complete traversal. If it finds a target, a product host must still own the
recycled clear or CRT allocation/fresh construction child, target UID read, Unit list/path/action
suffix, and their single atomic commit.
The call-entry composition is intentionally independent of the completed-setup digest: earlier
frame-zero receivers, including Merchant work, may change live orders, masks, coordinates,
Leader/World state, object-search scratch, and RNG before the Scout call.
