# 2024 frame-1 Citizen `Unit::think` suffix boundary

Supported executable SHA-256:
`30478a44d612d386c1ebb6b552d09c5e731e78e808102db6633ceb1a4a71fd6e`.

This tranche consumes the typed `ThinkSuffix` request left after the exact
`think_peasant(0)` wait-gate miss. It recomputes the complete preceding source join, keeps the
Leader pending OR and all Unit writes detached, and resumes retail at `0x005F71AA`.

## Exact reached path

| VA | Read/call | Supported result |
|---|---|---|
| `0x005F71C2` | `UnitTypeData::is_caravan` | false: replay type 50 has `unit_flags2 == 2`, so bit 8 is clear |
| `0x005F71DF` | `UnitData::is_rare_collector` | false |
| `0x0046FB00` | non-strict `ObjectTypeData::is(0x13d, 0)` | false from the exact replay Rules `graft`/`from` chain |
| `0x005F7317` | `LeaderData::leader_flags & 4` | human branch from the already-bound dual Leader mirrors |
| `0x005F7337` | `UnitData::unit_masks & 0x100` | set on the exact worker receiver |
| `0x005F7344` | `Unit::set_idle(0)` | first stateful child; exposed as a typed request |

`is_rare_collector` first accepts only concrete Types 61, 62, and 400. Type 50 therefore
reaches the non-strict relation query. The proof walks identity, direct `graft`, then each
replay-carried `from` ancestor and its `graft`, matching retail's cached `is_list` relation.
Missing rows and cycles are hard errors. The proof carries the replay file, payload, and full
Rules hashes plus every walked row.

## Atomicity and residual

`Unit::set_idle` at `0x005F6010` is 3,412 bytes, reads terrain/global scheduling state, and may
draw RNG or call further Unit children. Treating its argument as a plain `idle=0` field store
would be incorrect. The suffix therefore emits `Frame1CitizenSetIdleRequest` with the complete
detached Citizen image, staged dual-mirror Leader transaction, parent plan digest, stable Handle,
owner/ordinal, and the still-armed restore obligation.

No canonical `Sim` field changes in this tranche. In particular, retail's outer
`unit_masks2 |= 0x8000` at `0x0060DD68` is **not** executed before SetIdle returns and the
remaining `Unit::think` suffix reaches a real return. A later SetIdle receipt must be composed,
the suffix must resume at `0x005F7349`, and only then may the pending Leader write, accumulated
Unit image, common `ObjectData::flags &= 0xef` tail, and final mask restore be published as one
stale-checked transaction.
