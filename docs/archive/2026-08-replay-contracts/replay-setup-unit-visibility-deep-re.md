# Replay setup Unit visibility / reveal-fog deep RE

Status: source-only atomic producer; replay-library module registered, no channel installed.

## Provenance

The producer is transcribed from the shipped executable
`30478a44bb1f5697c9c294367344d2231ae35db8c35edddc03204ec0f625079` and PDB
`334a3e976dc6d33feb83f41c4afcbb250a8c9d20fe5c5938645100998d9bff5`. The inspected
PDB extents were:

| body | VA | bytes |
|---|---:|---:|
| `Object::update_seen(int)` | `0x00651B80` | 955 |
| `World::reveal_fog(FCoord,FCoord,int,int)` | `0x006B3D30` | 1091 |
| `ObjectsData::find_good_at` | `0x0065BEC0` | 372 |
| `Leader::new_rare` | `0x006D9E70` | 316 |
| `Unit::update_local_seen` | `0x0060E410` | 186 |
| `Unit::get_goody_box` | `0x005F7690` | 172 |

The first three bodies were recovered as complete PDB extents and checked against PE
disassembly. `Leader::new_rare` and `Unit::get_goody_box` were likewise read as complete
functions. No VM, retail process, replay checksum, or fitted value was used.

## Exact setup seam

`Unit::init` calls virtual `Object::update_seen(0)` at `0x00612DB3` after `update_los` has
stored the fresh Unit's `mylos`. The preceding collision-tail receipt binds:

- sparse `{owner,o,id,generation}` identity;
- de-obfuscated `x/y`, angle, domain, type `unit_flags2`, instance `unit_masks`, object flags;
- the pre-default-map-bit mask word that exists at this exact call;
- `infiltrated = 0`; and
- unchanged setup RNG state.

The new producer rejects any other residual, incremental call, non-land domain, invalid source
object, identity mismatch, or changed RNG seam.

The first virtual in `Object::update_seen` is `is_wonder`. A Unit dispatches to the folded
three-byte false stub `UnitData::is_wonder` at `0x0041BFF0`, so the Build/Wonder special arm is
not reachable. LOS dispatches to `UnitData::los` at `0x006100C0`; the existing exact Ptolemy / The
CEO receipt is consumed rather than treating raw `mylos` as final.

For a positive LOS, retail calculates `(los * 0xC0) / 0x180`, capped by the circle table at
64. Radius `<= 3` uses `project(x,y,angle,0x180)` at `0x0092CF40` only for a domain-zero Unit
with clear `type.unit_flags2 & 4` and clear `unit_masks & 1`. The source computes the shipped
projection with the exact integer `sinx/cosx` implementation. It then uses the regenerated
`circle_init` ordering and `World::set_seen` implementation; newly changed `seen2` cells are
therefore retained in the exact order in which `World::reveal_fog` is called.

One apparent extra mutation is provably absent for the fresh cohort. At `0x00651DEF`, a full
restamp calls virtual `Unit::update_local_seen` only when `ObjectData::visible +0x40` is nonzero.
`Object::init` stores the adjacent `visible/launch_frames` word to zero at `0x006477A1`, and no
intervening setup instruction changes it. The receipt records `visible 0 -> 0` and the skipped
`0x0060E410` / `World::set_seen2 0x006B4BB0` branch.

## `World::reveal_fog` transaction

Every newly explored fog cell reaches the following synchronized branches in this order.

### Rare Good

Both current `World` and canonical `worldc` must carry `TData::RESOURCE (0x0200)` at tile
`(2*fx+1, 2*fy+1)`. `ObjectsData::find_good_at(wx,wy,who,1,0)` receives
`wx=fx>>1, wy=fy>>1`. Because its final argument is zero, it does not scan the Good array:
it walks `WData::{down,down_who}` and then heterogeneous
`ObjectData::{next,next_who}` links until a sentinel. Only sentinel `-2` names a Good; inactive
Goods and type 5 Oil are rejected.

`Leader::new_rare` then:

1. returns immediately when `(source.leader_flags & 0x0C) == 4`;
2. scans candidate leaders `0..7`;
3. requires candidate flag bit 0;
4. for a different candidate, requires both diplomacy cells to equal 2;
5. rejects a human candidate unless source flag bit 3 is set;
6. skips an already-present sparse Good slot;
7. consumes the next address-bound `LeaderData::type_avail(type,1)` receipt;
8. rejects `Good::is(6,0)` and then `Good::is(31,0)`; and
9. appends the stable Good slot through exact `ArrayBase<int>::add` growth.

After `new_rare`, `GoodData::ever_seen` is always ORed with the viewer bit. The local-player
popup/sound block between these synchronized operations is presentation-only and owns no replay
checksum byte.

### Oil patch

When current `WData.flags & 0x0800` is set, retail scans active Goods in ascending
`0..good_mark` slot order and selects the first whose decoded fine coordinates map to this WCoord.
It appends that stable slot to the viewer's `LeaderData::oil_patches` only when absent.

Both `new_rares` and `oil_patches` validate `length`, `capacity`, `increment`, flags, and element
cardinality before any owner is committed. Default empty arrays grow `0 -> 4`, matching
`increase_by(-1,0)`, then append at the old length. A zero increment, malformed header, or
non-growing capacity fails atomically.

### Item and source Unit

The item clause first requires signed current `WData.flags < 0`, i.e. bit `0x8000`. The
canonical `worldc` cell rejects land values 1/2 unless override bit `0x0100` is set. The surviving
path calls `ObjectsData::find_goody_at` at `0x0065C040`, walking the same heterogeneous object
chain until sentinel `-3`; a live Item then receives the viewer bit in `ItemData::ever_seen`.

Finally, a nonnegative source object is tested as a Unit. The setup seam proves it is the same
fresh Unit. When its call-time `unit_masks & 0x100` is set, retail calls
`Unit::get_goody_box(wx,wy)` at `0x005F7690`. That wrapper immediately crosses into the unjoined
Groups/order owner:

```text
Group::clear(-1)                         0x00713E80
Group::add(source o, source who, 0, 0)  0x00714350
Groups::push_group(who, scratch, 1)     0x0070F9E0
Group::action_move_to(                  0x0070FBA0
    wx*0x300+0x180, wy*0x300+0x180,
    queue=0, set_angle=0, angle=0, order=3,
    user=0, form=-1, front=-1, final=0)
```

The visibility owner emits one `AutoExploreGoodyRequest` per actually reached reveal call, in
chronology. Its exact first external residual is the `World::reveal_fog` call at `0x006B4163` to
`Unit::get_goody_box 0x005F7690`; it does not pretend the Groups or Unit order bytes were
committed.

## Atomicity and receipts

World fog/WData planes, Goods, Items, and dynamic Leader children are cloned, fully validated,
and committed together. Missing/cyclic occupancy links, stale type-availability chronology,
invalid sparse slots, malformed array history, or any other reached external fact returns an
error without publishing a fog bit or partial leader/item/good write. The receipt retains:

- resolved LOS and exact projected stamp centre;
- newly explored fog cells in circle-table order;
- one structured rare/oil/item/auto-explore receipt per `reveal_fog` call;
- each `ArrayBase<int>::add` before/after header and stable index;
- visible `0 -> 0` and skipped local-seen body; and
- RNG before/after equality.

The module remains source-only. Replay-library registration exposes the authority API for the
first-Scout composition; it installs no replay channel and edits no World/tick/save registration.

## Runnable validation

The exclusive test covers the combined rare-Good, oil-patch, and auto-explore boundary; direct
Item-ever-seen mutation; and rollback after a missing heterogeneous occupancy link:

```sh
cargo test -p don-replay --test setup_unit_visibility_deep_re \
  --target-dir /tmp/codex-setup-vis-target
```
