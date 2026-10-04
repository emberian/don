# 2024 frame-1 Citizen `Unit::set_idle(0)` entry

Supported executable SHA-256:
`30478a44d612d386c1ebb6b552d09c5e731e78e808102db6633ceb1a4a71fd6e`.

This tranche consumes the typed `Frame1CitizenSetIdleRequest` reached by the golden
`Unit::think` suffix. It recomputes the entire replay/post-command/SetAnim/Think chain before
executing any local instruction.

## First reached child

The exact SetIdle body begins at `0x005F6010`. Its only instructions before the first child are
stack/register setup and the type-domain branch:

| VA | operation | golden result |
|---|---|---|
| `0x005F6028` | read `UnitTypeData::domain +0x218` | replay-bound type 50 is land (`0`) |
| `0x005F6034` | call `Unit::find_goody_box` | first child |

`Unit::find_goody_box` is the 593-byte body `0x005F2540..0x005F2790`. It scans 49 WData
cells in shipped spiral order, binds the actor's home region, and only for an item-marked cell
reads four fog-history cells, the heterogeneous item/object chain, and current/shared item
visibility. A found target calls `Unit::get_goody_box` (`0x005F7690`), whose scratch Group is
pushed into `Groups` before `Group::action_move_to` issues order 3. Those Group/order/path
owners are not inferred from the Unit receiver or from WData flags.

## Atomicity

No animation, Guy, order, path, object flag, Unit mask, idle byte, RNG, or Leader field is
written before this child. `Frame1CitizenFindGoodyBoxRequest` consequently carries:

- the complete detached Citizen image, including all Guys and the empty-order/path attestations;
- stored and decoded actor coordinates;
- stable Handle, owner, ordinal, type, frame, and full parent digests;
- the exact staged dual-mirror Leader pending transaction; and
- the still-armed outer `unit_masks2 |= 0x8000` restore.

The request is a red continuation boundary, not a false no-goody answer. A later receipt must
bind the exact item/WData/fog/object-chain sources and any movement mutation. Only after that
receipt and the remaining SetIdle/Think suffix return may all accumulated effects and the final
`0x8000` restoration publish atomically.
