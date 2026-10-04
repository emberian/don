# Replay starting Units/Guys producer frontier

Lane: `replay-setup-units-guys` · replay-first · 2026-08-11.

This tranche recovers the deterministic outer schedule of `Setup::build_units`
`0x005aafc0` and freezes the exact evidence a future mutating host must return. It does not
install either checksum channel, synthesize Guy bytes, or claim a corpus match.

## Instruction-derived schedule

The complete 1,952-byte PE body and the matching PDB signature were read directly. The
decompiler was used only as a control-flow map; Capstone settled the four-entry jump table,
all six `Setup::place_unit` arguments, and the direct RNG site.

The PDB names matter here: `Game+0x2c` is `GameInfo::starting_town`, `Game+0x2d` is
`GameInfo::starting_resources`, and `Game+0x30` is `GameInfo::reveal_map`.
`Setup::get_starting_citizens` `0x005aaf50` maps `starting_town` to these two outputs:

| `starting_town` | total citizen iterations | fixed-building prefix |
|---:|---:|---:|
| 0 | 3 | 0 |
| 1 | 4 | 0 |
| 2 | 5 | 2 |
| 3 | 10 | 5 |

The second output selects the first object-2001 branch only in modes 2 and 3. It is not
an extra citizen count.

Before citizens, nonzero `starting_town` modes place one Scout, bonus-9 rule Scouts, the
bonus-9 `reveal_map` Scout, then exactly two type-62 Dutch Merchants. Type selection is a
three-step identity chain: base-or-nation variant, `current_upgrade` in `build_units`, then
a second `current_upgrade` at the head of `place_unit`. The source type and both results
are kept in every call record.

Citizen modifiers preserve retail's wrapping signed arithmetic and order:

1. `starting_resources` 7 adds 8; value 8 adds 12;
2. in `starting_town` mode 2 only, bonus 20 adds `Rules+0x878 - 3` when the rule is above 3;
3. bonus 19 subtracts 3 in mode 2 or 5 in mode 3; and
4. after the scholar branch, bonus 16 adds `Rules+0x7b0`.

`starting_town` mode 0 skips the Scout block but still reaches scholars and citizens. A
negative scholar rule is load-bearing: the condition is `!= 0`, so retail performs its
building search even though the later positive-count loop would be empty.

## Exact admitted and stopped regions

The planner emits complete `place_unit` schedules for `starting_town` modes 0 and 1 when
the scholar search is not selected. It also requires the returned center identity to lie
in the per-owner Build band `2000..3000`; a missing city is not silently converted into
the separate city-less `place_unit` fallback. It returns a nonmutating stopped prefix at:

- `0x005ab24d`, before bonus-5 scholar building search, containment, and `go_inside`; or
- `0x005ab3cc`, before modes 2/3 select existing buildings, query virtual type/gatherer
  predicates, spawn at building coordinates, and install gather orders.

This means the caller can inspect an exact prefix without accidentally committing it.
Execution remains red until the stopped suffix is owned atomically.

## `place_unit` and receipt boundary

`Setup::place_unit` is 749 bytes at `0x005abca0`. It chooses candidate offsets around the
center Build, calls `Random::get(0,0xffff)` directly at `0x005abd76` when the runtime offset
table has multiple entries, and either calls `Objects::init_unit` or queues
`Build::train`. The offset-count and displacement tables are runtime-initialized BSS, not
static PE constants, so this tranche does not invent them.

Receipts preserve:

- every direct RNG pre-state/result/post-state, recomputed by `don_sim::rng::Random`;
- the separately bounded nested `Objects::init_unit` RNG span;
- the existing detailed-validator extent (`0x0065e0c0`, 1,603 bytes), Unit-band mark, linked
  member order, and returned captain;
- stable `{id,generation,owner,o}` identity for every member;
- live ptype, null launching, exact empty `Stack<PathData>` history, empty orders, and the
  full `PtrArray<Guy>` allocation/identity shape; and
- identical stable keys installed into the landed `UnitsWalkAuthority` and
  `GuysWalkAuthority` owners.

The receipt validator rejects RNG discontinuities, wrong direct draws, malformed container
history, missing/aliased authority keys, duplicate sparse identity, and a queued type/city
that differs from the call plan. No recorded checksum is an input.

## Remaining exact next callee

The immediate residual is `Setup::place_unit`'s runtime offset-table/candidate-map owner,
followed by the RNG-bearing `Unit::init -> Guy::init_real` animation/graphics path inside
`Objects::init_unit`. The landed Units adapter still refuses a nonempty Guy pointer array;
the Guys adapter can own it, but the two must be joined before starting citizens can be
legally hashed by the Units channel.

Shared integration was intentionally not edited. It requires one
`pub mod setup_units_producer;` line in `don-replay`, and—if the receipt extent is to be
checked cross-crate rather than copied as evidence—the existing source-only
`objects_init_unit_authority_frontier` module must be registered in `don-sim`.
