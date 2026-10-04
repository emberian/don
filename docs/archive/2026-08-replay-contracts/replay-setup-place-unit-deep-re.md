# Setup placement and initial Guy materialization

Status: source-only exact-prefix producer. This receipt does **not** install either replay
checksum channel and does not claim replay compatibility. It replaces two previously opaque
setup inputs—the placement-circle BSS tables and the new-Unit Guy initialization prefix—with
deterministic, runnable producers that stop at named external mutation boundaries.

## Authority and runnable receipt

The authority is the shipped supported executable `ron-bin/riseofnations.exe` (SHA-256
`30478a44...625079`) plus `ron-bin/sbl/rise.pdb`. Procedure extents come from the PDB and
instruction order/arguments from the shipped PE. The implementation and isolated receipt test
are:

- `crates/don-replay/src/setup_place_unit_deep_re.rs`
- `crates/don-replay/tests/setup_place_unit_deep_re.rs`
- `cargo test -p don-replay --test setup_place_unit_deep_re`

The test imports the exclusive source file by path. This is intentional: the producer is not
registered into a shared runtime module until its named residual owner exists.

## `circle_init` removes the placement-table unknown

`circle_init` is the complete 295-byte procedure at `0x006817F0`. It fills:

| BSS table | VA | meaning |
|---|---:|---|
| `circle_x` | `0x00CB7E90` | signed-byte X offsets |
| `circle_y` | `0x00CBB0E0` | signed-byte Y offsets |
| `circle_radius` | `0x00CBE330` | cumulative end/count by radius |

For each radius, retail scans X first and Y second from `-radius` through `+radius`. A point is
admitted when its approximate distance is exactly the current radius. With `big=max(abs(x),
abs(y))` and `small=min(...)`, the ordinary branch is `big + small*small/(2*big)`; the
large-coordinate fallback is `(small + 2*big)/2`. All operations retain x86 integer order.

The capacity check happens *after* append. Consequently the final stored length is 12,873, the
last admitted coordinate is `(46,-41)`, and the last end value is copied through radius 64.
Counts used by `Setup::place_unit` are exact:

| radius | cumulative count |
|---:|---:|
| 2 | 21 |
| 8 | 237 |
| 24 | 1,905 |

## `Setup::place_unit` (`0x005ABCA0`, 749 bytes)

`LeaderData::current_upgrade` at `0x006E3140` resolves the requested unit type before probing.
When a nonnegative center index is supplied, the center `Build` coordinates replace the passed
coordinates for the anchor. The tile anchor is the nonnegative generated-map equivalent of
`div_3_table[coord >> 8]`, namely `floor(coord/0x300)`.

The loop shape is:

| setup state | attempts | radius |
|---|---:|---:|
| `starting_town != 0` | 60 | 2 |
| `starting_town == 0 && LeaderData::active == 0` | 500 | 8 |
| `starting_town == 0 && LeaderData::active != 0` | 500 | 24 |

Every reachable count exceeds one. Therefore every attempt performs exactly one direct game-RNG
call `Random::get(0,0xffff)` at `0x005ABD76`, then reduces the result modulo the cumulative circle
count. The draw occurs before every candidate gate and remains consumed after rejection.

The candidate coordinates are WCoords: one cell spans 768 world units in each dimension.
The 28-byte candidate/anchor records come from `World::wdata` at `WorldData+0x134`. The
collision query is separate: retail reads the center TCoord word from `World::tdata` at
`WorldData+0x138`, index `(4*y+2)*tile_xs + (4*x+2)`. Candidate rejection order is exact:

1. X below zero, Y below zero, X at/above world width, Y at/above world height.
2. `WData+0x04` continent differs from the anchor continent.
3. `WData+0x00 & 0x30` is nonzero.
4. With `WData flags & 0x100 == 0`, signed land byte `WData+0x02` is 1 or 2.
5. `WData flags & 0x100` is nonzero.
6. Signed `WData+0x08` object short is nonnegative.
7. The center-TCoord collision bit `0x4000` is set or its low two bits equal 3.
8. The signed `WData` flags word is negative.

An admitted WCoord cell is converted back to its center `cell*0x300+0x180`, then calls the thin
`Objects::init_unit` thunk at `0x00461310` (body `0x0065E0C0`) with object identity/history
arguments `-1,-1,-1`. This call is the accepted-path first external residual.

After exhaustion, paths remain distinct:

- With a center, the object is recovered from the owner Build band, vcall `+0xAC` executes at
  `0x005ABF2F`, and the supported `Build` vtable `0x00B42174` resolves it to the three-byte
  identity getter `Build::get_build` at `0x0041C000`. Its receiver result enters
  `Build::train(type)` at call `0x005ABF3A`, body `0x0062F9B0`. The receipt freezes all five
  addresses rather than hiding the receiver conversion.
- Without a center, retail calls the `Objects::init_unit` body at the originally requested
  coordinates. If allocation returns nonnegative, it then calls `Unit::come_out(0)` at
  `0x00617C10`. The producer records that conditional tail without inventing an allocation.

## `Unit::init` new-Unit Guy prefix

`Objects::init_unit` eventually enters `Unit::init` at `0x00612100` (3,732 bytes). The full
procedure also owns unit type, object, order, location, and registration mutations; this receipt
does not pretend those are already hosted. It isolates the exact new-Unit block at
`0x00612A7A..0x00612CC1` once that surrounding state is supplied.

`Unit::Unit` initializes the inherited `PtrArray<Guy>` with length/capacity zero, null list,
flags zero, and changes its increment to 1. `Unit::init` computes
`squad_size + crew_size`, grows capacity to exactly that total, and fills every slot in ordinal
order through `Recycler<Guy>::pop`, `Guy::clear`, and `Guy::init_real(0)`. It then sets array
length to the total and `guy_mark` to `squad_size`. Each Guy receives the current unit type,
owner, object index, ordinal `guy_num`, and post-init variation zero.

The receipt carries two identities deliberately:

- stable host identity `(id,generation,owner,o,type)` for the Unit;
- stable Guy identity `(unit id,unit generation,owner,o,guy_num)` for every dense native pointer
  slot.

Native replay identity is still the sparse `(owner,o)` object slot. The host generation is not
written into retail bytes; it prevents recycled sparse slots from aliasing in the local arena.

## `Guy::clear -> Guy::init_real(0)`

`Guy::clear` is the complete 276-byte procedure at `0x005DB590`. It creates the synchronized
155-byte Guy checksum image with type 50, X/Y `-1536`, `last_time=-1`, `gpiece=-1`, `ox=-1`,
`whom=-1`, and every other field zero. `Unit::init` overwrites type/owner/object/guy number before
calling `Guy::init_real`.

`Guy::init_real` is 1,442 bytes at `0x005DB6B0`. Its first substantive call is
`Guy::update_gpiece` (`0x005D8530`), so the producer requires a coherent, hash-gated
`ExtractedGuyGraphics` receipt rather than guessing a gpiece or pivot lattice. It then consumes
one direct game-RNG draw at `0x005DB6FD` per Guy in pointer-array order. `draw % 100` selects:

| remainder | initial animation |
|---:|---:|
| 0..69 | 0 |
| 70..79 | 1 |
| 80..89 | 2 |
| 90..99 | 3 |

An out-of-range/unloaded selected animation packet resets the selection to zero. The producer
requires the four packet-validity predicates explicitly and records selection before and after
validation. It also records the native predicates that set synchronized `guy_flags`: pivot
restrictions (`0x100`), animation 22 or unit flag 2 bit 4 (`0x8`), the fast-facing predicate
(`0x10`), air (`0x40`), and base types 52/53 (`0x80`). Graphics receipts are rejected before a
draw if the PE/data hashes, capture coherence, guy slot, gpiece, restriction count, pivot flags,
or non-turret pivot state disagree.

At return, current/end time, last/average speed, track offsets, attack bytes, and variation are
zero; `last_time`, `ox`, and `whom` retain `-1`; `stopped` is one; graphics-derived gpiece/pivot
state and predicate-derived flags are installed. “Complete” here means the synchronized
`GuyData +0x08..+0xA2` image, not the non-checksummed render/output tail of the 240-byte object.

## Exact first external residual

The runnable placement producer stops first at `Objects::init_unit` on acceptance, at
`Build::get_build -> Build::train` after centered exhaustion, or at
`Objects::init_unit -> conditional Unit::come_out(0)` after centerless exhaustion. It performs no
allocation or shared-world write.

The runnable Guy prefix stops at the next `Unit::init` calls:

1. `Unit::update_gpiece` at `0x005E2920`;
2. `Unit::set_new_location` at `0x005F8D20`.

The latter owns the initial squad formation and recursively derives crew placement from the
exact graphics hierarchy. That location/crew lattice is the first remaining external state
residual for the synchronized Guy image. Until it is ported or retail-oracled, this source must
remain a prefix producer and must not be installed as a checksum authority.

## Replay evidence and limit

The existing local replay Guys corpus test decodes 61 of 63 `.rcx` files, reaches 21
checksum-bearing replays, and compares 222,938 Guy entries with 21 distinct nonzero first-Guy
values. Those replay deadlines establish that Guys are live synchronized state and that a
zero/default substitute is invalid. They do not by themselves prove this constructor prefix;
the prefix values and RNG chronology above come from the shipped PE. No sample fitting is used.
