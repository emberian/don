# Golden frame-zero Scout spellcaster frontier

Status: source-exact detached transaction for the supported 2024 human Scout path; dynamic
`SpellTypeData::is_castable`, range authority, global object enumeration, and a successful Unit
order mutation remain typed host boundaries.

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

## Global search scratch

Despite its PDB `const` signature, `ObjectsData::find` initializes two global scratch words before
enumeration:

| word | entry write | later meaning |
|---|---:|---|
| `ObjectsData+0x1FC` | `0x05F5E0FF` | best metric |
| `ObjectsData+0x200` | query owner (`who`) | selected target owner |

A no-target return is exactly `-1` and retains those entry values. A target return is nonnegative;
the second word supplies `add_cast_order`'s target-owner argument. The Rust transaction accepts
this mutation only through a revision-bound retail receipt and stages it until atomic commit.

## Unit order write, not Caster active-spell write

`Unit::add_cast_order` is `0x005E4A60`, 541 bytes. For Counterintel 631 and queue position zero it:

- allocates `OrderIndex 0x0E` through `OrdersMemManager::get_obj` `0x00730AC0`;
- writes target object/owner at CastOrder `+0x08/+0x0C`;
- reads the live target's UID word at `ObjectData+0x30` into CastOrder `+0x10`;
- writes decoded coordinates at `+0x14/+0x18`, zero at `+0x1C`, and spell 631 at `+0x20`;
- clears the order byte's `0x04` bit because the final argument is zero;
- links the order into `Unit+0xCC` through `LinkListBase::add` `0x0046D5A0`;
- because queue position is zero, does **not** call `Unit::close_orders`, calls
  `Unit::clear_partial_path` `0x005E3920`, rewrites the `Unit+0xDC` current-order link to its first
  pointer, and reaches `Unit::update_action` `0x0060A870`.

The allocation, target-UID read, path clear, current-order/link mutation, and action/Guy
after-image form one product-host transaction. The Rust frontier stops at that exact request. It
does not publish the preceding search scratch alone and does not guess any post-order Unit, path,
Guy, or World state.

There is no access to `CasterData::active_spells` in `Unit::think_spellcaster` or this
Counterintel `add_cast_order` arm. A queued CastOrder is work for the Unit order dispatcher; it is
not an `ActiveSpell { type,start,end }` in `CasterData+0x04`. Therefore the existing frame-one
`Caster::process_spells` authority composes as follows:

| frame-zero result | Unit order/path | Objects search scratch | RNG | frame-one active spells |
|---|---|---|---|---|
| not special / not castable / insufficient mana | unchanged | unchanged | unchanged | unchanged |
| search finds no target | unchanged | sentinel/query-owner write | unchanged | unchanged |
| search finds target | external atomic CastOrder transaction | selected metric/owner staged | unchanged | unchanged |

In particular, a setup-empty Caster array remains empty across this function on every branch.
That is a proof from the reached write set, not a replay-state guess.

## Implementation and gate

`crates/don-sim/src/systems/frame0_scout_spellcaster.rs` binds the executable SHA, stable golden
identity, Scout Rules shape, child-call requests, source revision, search scratch, and all
checksum-relevant invariant revisions. `prepare_golden_scout_spellcaster` is pure. A complete
no-cast result can be installed only by `commit_no_cast`, which revalidates the whole before-image;
a target hit returns the final typed `AddCastOrderRequest`.

The focused standalone gate pins both top-level branches, the first external child, signed mana
gate, complete range/find ABI, no-target scratch, exact CastOrder request, stale-receipt refusal,
and atomic commit invariants.

## Remaining golden evidence

This closes the source shape and prevents a false Caster/RNG dependency, but it does not invent
the golden retail results of `SpellTypeData::is_castable`, `SpellTypeData::get_range`, or the
spatial `ObjectsData::find` traversal. The frame-one command-entry oracle should capture those
typed child results (and the final Unit order transaction if a target is found). Merchant frame
zero work remains an independent reason that the full command-entry image cannot yet be derived
from the completed setup image alone.
