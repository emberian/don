# Special animation order state

Status: **complete concrete payload; executor remains separate**.

`SpecialAnimOrder` is 44 bytes in the shipped PDB: the eight-byte `UnitOrder` base followed
by nine walked 32-bit fields. `SpecialAnimOrder::walk_data` `0x004849A0` walks `flags` and
the contiguous `+0x08..+0x2C` payload, so the discriminator and every tail word are save and
checksum state.

| offset | field | clear value |
|---:|---|---:|
| `0x08` | `SpecialType type` | `SPECIAL_UNIT` (2) |
| `0x0C` | `started` | 0 |
| `0x10` | `frames` | 0 |
| `0x14` | `data1` | -1 |
| `0x18` | `data2` | -1 |
| `0x1C` | `data3` | -1 |
| `0x20` | `data4` | -1 |
| `0x24` | `ox` | -1 |
| `0x28` | `whom` | -1 |

`SpecialAnimOrder::clear` `0x00484EC0` supplies those defaults. `Unit::add_spec_anim_order`
`0x005E4160` sets the Group-order flag, writes its first argument to `type`, and writes the
next two arguments to `data1` and `data2`.

`Order` and executable `OrderRec` now retain the complete payload, and DoNSave format 3
round-trips it. `UnitData::is_entering_or_exiting` `0x0060A6F0` resolves the current order
through virtual `get_spec_anim_order` and returns true only for `SPECIAL_ENTER` (0) and
`SPECIAL_EXIT` (1). Defeated-player Army stop uses that exact predicate: ENTER/EXIT members
are preserved by the Group halt, while `SPECIAL_UNIT` members are cleared normally.

The remaining order-class boundary is `Unit::do_spec_anim` `0x005E5880`; retaining its full
payload removes the former state-shape blocker but does not claim that executor.
