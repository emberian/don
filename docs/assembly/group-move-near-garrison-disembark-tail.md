# `Group::action_move_near`: garrison and disembark tail

Date: 2026-08-11. Lane: `group_move_near_tail`. Status: source-only, host-backed,
not registered in production. Confidence: Tier C (`[measured]` from the shipped PE/PDB and
the complete Ghidra corpus; not executed against retail).

This note recovers two previously mapped but unported regions of
`Group::action_move_near` (`0x00704990`, 9,205 bytes):

| retail interval | bytes | recovered seam |
|---|---:|---|
| `0x007054C7..0x00705749` | 642 | army/city garrison-or-move decision |
| `0x00706015..0x00706D5C` | 3,399 | disembark path creation, scatter, repair, append and inversion |

The executable model and focused tests are:

- `crates/don-sim/src/systems/group_move_near_garrison_disembark_tail.rs`
- `crates/don-sim/tests/group_move_near_garrison_disembark_tail.rs`

The module is intentionally not in `systems/mod.rs`. It needs the shared command ABI fix
and a production pathfinder/world transaction adapter before registration. Consequently
this lane changes no closure row and makes no claim that a live opcode is complete.

## Authorities

- PE: `ron-bin/riseofnations.exe`, `.text` at the VAs named below.
- PDB: `ron-bin/sbl/rise.pdb` names `Group::action_move_near`,
  `PathFinder::find_wpath`, `PathFinder::astar_path`, `PathData` and
  `Stack<PathData>`.
- Full decompilations: `re/decomp-all/00704990.c`, `00688FC0.c`, and `00683770.c`.
- The earlier prelude/map: `docs/mechanics/group-action-move-near.md` and
  `systems/group_move_near_split.rs`.

PDB names are naming evidence, not calling-convention authority. Capstone instruction
decoding of the PE settles the stack and side effects below.

## Correct `action_move_near` ABI

The existing Rust bridge drops a word. Retail has eleven stack dwords, confirmed by
`ret 0x2C` at `0x00706D82`:

| EBP offset | argument |
|---:|---|
| `+0x08` | destination X |
| `+0x0C` | destination Y |
| `+0x10` | tolerance |
| `+0x14` | queue position |
| `+0x18` | set angle |
| `+0x1C` | angle |
| `+0x20` | order index |
| `+0x24` | **group-order flag** |
| `+0x28` | form |
| `+0x2C` | width |
| `+0x30` | disembark |

The command handler at `0x00949742..0x009497A1` pushes, in reverse order,
`disembark`, `width`, `form`, literal `1`, and `orders`. Three independent consumers
name those positions by behaviour:

- `0x007050D9` reads `+0x28` for form resolution;
- `0x007050FD` reads `+0x2C` for width resolution;
- `0x00705F7D` forwards `+0x24` to the group flag slot of
  `Unit::add_move_facing_order`;
- `0x00705ED5` forwards `+0x30` into the created group order's flag word.

The recursive split calls at `0x00704CBF` and `0x00704D10` forward all eleven words, as
does the queue-first recursion at `0x00704F1B`. The source-only `MoveNearAbi` freezes the
correct five-word tail. Production must add `group: i32` between `orders` and `form`; using
the current four-word Rust signature shifts form, width and disembark.

## Army/city member fork

The fork applies only when the leader flags permit army handling and the receiver has a
nonnegative army. Invalid, off-map and plane members are skipped first.

When city lookup ran and found a city, only a supply, siege, or hero unit enters the
garrison branch. `UnitData::can_garrison(city)` then chooses:

1. success: install `GARRISON(target=city, who=actor, search=0, queue=New,
   group=0)` through `Unit::add_garrison_order` (`0x005E4080`);
2. failure: compute `find_angle(actor - city)`, then ask `find_nearby_spot` for radii
   `0x300..0x600`, step zero, filter three, actor identity, and raw tail
   `[0, 0, -1, 0, -1]`;
3. a zero nearby return selects its output coordinate; a nonzero return falls back to the
   city centre;
4. install `MOVE` through `Unit::add_move_order` (`0x00616ED0`) with `set_angle=1`,
   `angle=0`, `queue=New`, `group=0`, original coordinate `(-1,-1)`.

The unnamed seventh post-coordinate word of that last call is not a typo: Capstone shows
retail forwarding the selected destination Y there. The Rust plan names it `raw_arg7` and
tests it so a future typed adapter cannot erase it.

If city lookup did not produce a city, a siege unit already executing order 10 advances
without another order. Other members continue into the regular formation/order path.
Host facts are branch-lazy: `can_garrison` is required only for a candidate with a real
city, and is rejected if supplied on a branch where retail never calls it.

## `PathData`, stack ABI, and call shapes

PDB layout makes `PathData` exactly 16 bytes:

```text
+0  to_x       int
+4  to_y       int
+8  tolerance  int
+C  flags      int
```

The global temporary `Stack<PathData>` uses data `0x00EE1538`, capacity `0x00EE153C`,
count `0x00EE1540`, and element-size byte `0x00EE1544`. Unit-local paths begin at unit
`+0xB8`.

Although PDB presentation can suggest a `thiscall`, emitted `find_wpath` calls push five
dwords and `0x006897B5` returns with `ret 0x14`. The entry saves its incoming stack in EBX
and reads all five values there; it does not consume an incoming pathfinder `this` in ECX.
The effective ABI is:

```text
find_wpath(Stack<PathData>*, start_x, start_y, owner, object)
```

There are three call sites in the recovered tail:

- `0x0070619A`: leader/group request while the global army-path toggle is one;
- `0x007061FF`: the same leader/group request without the toggle;
- `0x00706C3C`: per-member fallback request when no group path survived.

The leader request starts at the last point of its existing unit path, or at the group's
recorded origin when that path is empty. Per-member fallback uses the same rule for each
unit. The stack is seeded with the corresponding formation destination, tolerance and
corner flag one. A leader distance below `0x900` is the straight-path shortcut and avoids
`find_wpath` entirely.

## Exact disembark transaction

The host-backed preflight performs the recovered interval as one recomputable receipt:

1. validate owner/member identities, exact unit set, leader, formation arrays and
   permutation, positive world dimensions, and the exact `0xE60` form scratch size;
2. seed or obtain the leader/group path;
3. pop each global path node from the back;
4. scatter it to every eligible member by its formation offset from the leader;
5. for form 8, use the static permutation only on non-corner nodes of a non-straight path;
6. clamp coordinates with the retail coarse (`*0xC0`) test and tile (`*0x300`) bounds;
7. compare terrain regions for the original and scattered node; on a changed region call
   `invalid_loc(actor,x,y,[1,1,0,0,0,0])`, and for an invalid non-straight point rehome it
   to the original node's `0x300` tile;
8. apply the sea/non-corner leader-only filter, MOVE_TO movement gate, form-9 exception,
   and the `0x600` leader distance cut;
9. append each accepted point to the unit path;
10. after a group path, invert every valid on-map non-plane unit path, even if an army
    predicate filtered that unit from scatter;
11. if the group path is empty, call `find_wpath` once per eligible member, use its seed
    when the returned stack is empty, otherwise pop returned nodes, then invert that unit;
12. increment `GroupData::order_num` and zero `form + 0x30` for `0xE60` bytes.

Each world/pathfinder answer is recorded. Commit requires byte-for-byte equality with the
preflight state, the same host epoch, and a full identical recomputation. Missing host
facts and malformed effects are typed errors; neither preflight failure nor commit
failure partially mutates simulation state.

## Transitive RNG and coupled side effects

`Group::action_move_near` and `PathFinder::find_wpath` contain no direct call to canonical
RNG. That is not a zero-RNG proof: `find_wpath` calls `PathFinder::astar_path`
(`0x00683770`) once, and that child contains two mutually exclusive RNG-bearing exits.

At `0x006848BD..0x006848EB` and `0x00684DF5..0x00684E29`, Capstone proves the same effect:

```text
Random::get(0, 0xffff)
current_order->field_1c = draw % 3 + 6
unit->byte_b2 += 30
```

The byte increment can occur on the same exits without the order write and without an RNG
draw. Therefore the pathfinder host answer carries, atomically:

- the exact RNG values and before/after RNG epoch;
- optional unit byte `+0xB2` before/after (`wrapping_add(30)`);
- optional current-order `+0x1C` before/draw/after (`draw % 3 + 6`);
- the resulting `Stack<PathData>`.

Preflight rejects a draw without its coupled order mutation, a mutation for any actor
other than the path request's unit, a bad counter delta, a bad `% 3 + 6` result, or a bad
RNG epoch. This corrects the tempting but false conclusion that the tail consumes zero
RNG.

## Validation and remaining hook

Focused tests cover the ABI, both garrison outcomes, branch-lazy refusal, straight and
pathfinder group paths, pop/append/invert order, per-member fallback, sea filtering,
terrain repair, transitive RNG/order/counter commit, malformed-effect refusal, stale
state/host rejection, and structural mutation failures.

Closure remains unchanged until all of these exist together in production:

1. correct five-word action tail in `command.rs` and every recursive/wire caller;
2. formation/permutation adapter;
3. pathfinder plus world-region host with exact RNG and unit/order effects;
4. atomic path/group state commit;
5. command/save round trip;
6. live tick and replay checksum evidence.
