# Construction site lifecycle

`crates/don-sim/src/systems/construction_lifecycle.rs` is the executable transaction
boundary for the retail site bodies which construction progress enters. It sequences
mandatory host operations for:

| body | VA |
|---|---:|
| `Wall::start` | `0x0063E810` |
| rejected `Object::disband(1)` | `0x006455C0` |
| `Wall::activate` | `0x0063E4B0` |
| `Build::activate` | `0x00623E20` |
| `Farms::add_animals` | `0x008D8F30` |

The module does not replace terrain, leaders, cities, visibility, object pools, or the
shared RNG with compact stand-ins. Those stores are required through
`ConstructionLifecycleHost`; there are no default methods. The public runtime fidelity
gate remains false until one host implements every operation and coherent retail captures
verify the resulting channel and RNG receipts.

## Admission and start

`check_site_admission` preserves the exact allowed codes `0`, `0x27`, `0x28`, `0x29`, and
`0x2B`. Code `0x2A` is admitted only with a linked city and:

```text
CityData::num_wonders(1) <= 1 + (LeaderData::has_tribe_bonus(7) != 0)
```

An accepted first touch starts and contributes progress in the same `do_construct` call.
`start_wall` writes STARTED and `frame_started`, scans the footprint in x-outer/y-inner
order to remove competing sites, places and masks terrain, writes coalesced half-resolution
owner cells, updates visibility, and marks behind tiles. Wonder notice work runs only after
the deterministic visibility transaction. Start does not reset either progress counter.

Footprint dimensions must be positive; invalid dimensions fail loudly. Removing a local
competing site can consume one shared RNG draw when the retail sound-choice table is
nonempty. The removal callback must report those draws even though the selected sound is a
presentation sink.

## Rejected sites

`disband_rejected_build` first performs the Build close and requires VALID to be cleared.
It then visits all six goods in index order, applies `LeaderData::type_avail(good,1)`,
recomputes the full type cost, and refunds every nonzero amount through the owner's
XOR-`0x8221` resource slot. Rejection has zero direct shared RNG draws, but it mutates Build,
leader-resource, terrain, targeting, and object state; the host receipt must retain every
transitive checksum effect.

## Activation

`activate_build` preserves the measured local ordering:

1. Library/pre-activation work, then `recharging=0`.
2. Captured/Chinese and last-built work.
3. Completion visibility; defensive start if STARTED is absent.
4. ACTIVE, build mask `0x1000`, and both progress counters reset. Air Defense then writes
   `job_counter=0x40000000`.
5. Wall stats, in-progress counters, leader dirty flags, local completion event, and cover
   removal.
6. City/fort/dock/oil/wonder registries, leader/category/gather-slot state, roads, borders,
   training, tech, and bonuses through the Farm insertion point.
7. Farm animal creation when the authoritative type is `0x1A1`.
8. The remaining wonder, diplomacy, visibility, hits, LOS, transport, and city-upgrade
   transaction.

The broad phases remain mandatory host methods because their stores are not all owned by
`don-sim`; returning an incomplete receipt is a host contract violation and cannot open the
runtime gate. A flags-only activation is not a supported implementation.

## Farm RNG

A live Farm parent creates five animals. Each animal consumes exactly three calls to the
shared `Random::get(0, 0xffff)` in species, y, x order, for 15 draws total. Retail's upper
bound is half-open. The species predicate is `(draw & 0x80000001) == 0`; chicken is type
`0x196`, pig is `0x195`. Coordinates use wrapping signed arithmetic:

```text
y = parent.y + (draw % 0x180) - 0xc0
x = parent.x + (draw % 0x180) - 0xc0
```

Each spawned owner-9 Unit receives the Farm parent's `(o,who)` and ordinal in its
`+0x150/+0x152/+0x154` fields. An invalid or inactive Farm parent follows retail's early
return and consumes no draws; a normal completed Farm must take the five-animal path.

## Evidence boundary

Structure and constants are PDB/decompiler/Capstone-derived Tier C evidence. Promotion
requires before/after capture of the full Build image, shared RNG seed, isolated checksum
channels, object identities, order target, and callback order described in
`construction.md`. The currently deployed retail hook does not expose that boundary.
