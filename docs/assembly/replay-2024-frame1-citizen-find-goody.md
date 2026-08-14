# 2024 frame-1 Citizen `Unit::find_goody_box`

Supported executable SHA-256:
`30478a44d612d386c1ebb6b552d09c5e731e78e808102db6633ceb1a4a71fd6e`.

This tranche consumes the exact `Frame1CitizenFindGoodyBoxRequest` emitted by the detached
`Unit::set_idle(0)` entry. It recomputes the complete replay/post-command/SetAnim/Think/SetIdle
chain, binds the whole post-SetAnim DoNSave image, and executes only read-only instructions.

## Exact control flow

`Unit::find_goody_box` is `0x005F2540..0x005F2790` (593 bytes). The golden type-50 domain is
land. Stored Unit coordinates are XOR-decoded, converted through the shipped divide-by-three
table, and the actor's WData region is retained. The body then walks `MOVE_X/MOVE_Y[0..49]`
in shipped order.

For each in-bounds, same-region cell whose signed flags word is negative (`0x8000` set),
retail calls `WorldData::was_seen` in this exact order:

1. `(2*wx+1, 2*wy+1)`
2. `(2*wx,   2*wy+1)`
3. `(2*wx+1, 2*wy)`
4. `(2*wx,   2*wy)`

The assembly at `0x005F26B3` skips the candidate when all four are false. This corrects the
older generic helper's inferred "visibility fallback" shape: item visibility is evaluated
only after a successful history probe. On that reached arm, water land values 1/2 without
flag `0x100` jump directly to acceptance. Ordinary land calls
`ObjectsData::find_goody_at` (`0x0065C040`); a missing item accepts, while a found item must
pass virtual `ItemData::is_seen` (`0x00677850`).

The WData/object chain is resolved through the canonical sparse Unit/Build/Wall identities.
Item shared-vision, Spanish owned-cell history, current visibility, fog option, Leader flags,
diplomacy, terrain planes, and City registry reads all come from the bound Sim. An allied
territory with no City witness reaches `LeaderData::reg_forts[region]` at `+0x12DE`, which the
current Sim owner does not materialize independently. That arm emits
`Frame1CitizenRegionSeenRequest`; the adjacent regional-visibility continuation joins the exact
frame-zero census to the unchanged golden frame-one Build band and resumes with its zero Fort
answer.

The complete Sim save also binds item-registry producer state before the scan. Registry absence
is coherent only when the entire WData plane has no item marker or item sentinel; therefore any
reached item-marked cell proves that a synchronized registry is present. The plan records absent
marker-free versus present-save-validated authority rather than exposing a caller-supplied empty
registry.

## Return and Groups boundary

An exhausted spiral or a target equal to stale `UnitData::orders_x/orders_y` returns zero to
SetIdle at `0x005F6039`. Otherwise `0x005F2780` calls `Unit::get_goody_box(wx, wy)`.
The child clears and fills a scratch Group, calls `Groups::push_group`, then
`Group::action_move_to` with order 3 and the centered fine coordinates
`(wx*0x300+0x180, wy*0x300+0x180)`. This tranche emits the exact
`Frame1CitizenGetGoodyBoxRequest` before any of those operations. Every typed child request
binds the exact ordered scan-prefix digest as well as the complete parent digest.

## Atomicity

No Unit, Guys, order, path, item, fog, RNG, Leader, Group, or Groups field is mutated. The
staged Leader pending write and the outer `unit_masks2 |= 0x8000` restoration remain armed in
the detached parent image. Every plan is recomputed against the original whole-Sim hash;
stale WData, fog, diplomacy, City, object-link, item, order-target, or RNG state rejects the
continuation before publication.
