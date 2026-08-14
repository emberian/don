# 2024 frame-zero Dutch Merchant unpack boundary

This tranche covers the exact bounded success cone

`Unit::think` → `Unit::think_merchant` `0x005f4740` →
`Unit::unpack_merchant(3)` `0x006038e0`

for the supported golden replay's owner-0 Merchant Units `(who=0,o=1)` and `(who=0,o=2)`,
both concrete TypeIndex 62. It is derived from `riseofnations.exe` sha256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079` and the matching PDB.
The executable code sizes are 1,302 bytes for `think_merchant`, 452 bytes for
`unpack_merchant`, and 317 bytes for `find_merchant_spot`.

The implementation is detached in
`crates/don-replay/src/setup_2024_frame0_merchant_unpack.rs`. It is not mounted in
`Sim::do_frame`: the complete frame-zero object scheduler still needs the two native child
receipts and the surrounding Scout/Unit chronology.

## Reachability and order

The early Type-62 arm of `Unit::think` is at `0x005f70fd..0x005f7132`:

1. the type virtual reports the Merchant-62 predicate;
2. `unit_masks & 0x0008_0000` is tested;
3. only when that bit is set does `Unit::think` call `think_merchant`;
4. a nonzero return goes directly to the common success tail at `0x005f761a`, which clears
   `ObjectData::flags & 0xef`.

`think_merchant` repeats the `0x0008_0000` test at `0x005f4752`, then immediately calls
`unpack_merchant(3)` at `0x005f475f`. A successful unpack branches to `0x005f4c41` and returns
one. A failed unpack falls through at `0x005f476e` into the much larger Merchant Good-object
scan. Therefore a false spot result is not an idle/no-op frame: it is the first unimplemented
tail request.

The direct caller makes the repeated mask-clear arm unreachable in this golden cone. The
detached request consequently rejects a missing `0x0008_0000` bit instead of claiming that
`think_merchant` ran.

## First unsourced child

`unpack_merchant` calls

`Unit::find_merchant_spot(decoded_x, decoded_y, 3, &tile_x, &tile_y)`

at `0x0060390e`. `find_merchant_spot` is read-only for this call shape, but its answer is not
derivable from the replay file or the current incomplete Great Lakes World owner. Its exact
read order is:

1. `UnitData::calc_gather` `0x00609180` with the fixed `0,0,0,...,0,0,1,1,x,y` call shape;
2. the radius-3 count at `0x00add1e0` and ordered offsets at `0x00adcaf0` / `0x00adc400`;
3. candidate bounds against the live World tile dimensions;
4. `UnitData::good_merchant_spot` `0x006068a0`;
5. `UnitData::invalid_loc` `0x00607c30` with all five optional flags zero;
6. `Unit::detect_unit_collision` `0x00617060` at `(tile_x*192,tile_y*192)`, footprint 1×1,
   with the three trailing flags zero.

The first passing candidate is returned. No RNG call occurs. The authority seam records a
nonzero native digest over that complete terrain/gather/location/ordered-collision input and
binds the result to the exact request. It also carries an independently captured canonical-Sim
SHA-256 for the pre-call retail image; Don does not manufacture that snapshot.

`Unit::clear_partial_path` `0x005e3920` is the next runtime-only hazard. When the pointer at
`Unit+0x104` is null, it returns immediately. When nonnull, it retires search objects into five
global recyclers, nulls `Unit+0x104..+0x114`, and clears search scalars through `Unit+0x148`.
The current bounded transaction accepts only a native `Unit+0x104 == NULL` receipt. A nonnull
receipt fails before mutation.

## Exact successful suffix

After a found spot, the source order is:

1. clear `unit_masks & 0x100`;
2. call `add_cast_order(-1,-1,-1,-1,0x28c,QueuePos::New,0)`;
3. Queue-New clears `unit_masks & 0x0400_0000`, clears the path length while preserving its
   capacity/increment, closes the already-attested empty order list, and installs Cast;
4. normalize spell `0x28c`: query attribute `0x7b` first; true selects `0x28e`. Otherwise
   concrete TypeIndex 62 matches the special-object set `{61,62,400}` and selects `0x290`.
   The attribute-`0x13d` query is not reached for TypeIndex 62;
5. allocate and clear Move, append it behind Cast, rotate the retail linked-list head once,
   then call `update_action`. The canonical current-first order is `[MoveTo, CastSpell]`;
6. return one from `unpack_merchant`, return one from `think_merchant`, and clear flags bit
   `0x10` in `Unit::think`'s common success tail.

For a returned tile `(tx,ty)`, angle uses the uncentred goal
`(tx*192,ty*192)`. The Move endpoint is `(tx*192+24,ty*192+24)`. This is not a guessed scale:
`div_3_table` is constructed at `0x00681db0`; its positive arm stores integer `index/3`, so
`div_3_table[(tx*192)>>4] * 48 + 24 == tx*192 + 24` for a valid nonnegative candidate.

The exact flattened nodes are:

| current-first node | exact payload |
|---|---|
| MoveTo | flags/tolerance and pause/retry/attempts/timer zero; facing -1; destination and duplicate destination set to the centered coordinate; last and origin x/y -1; collision x/y zero; offsets are signed C remainders modulo `0x300`; angle is `find_angle(raw_goal-current)`; node metric zero |
| CastSpell | target `(who=-1,o=-1,uid=0xffff)`, x/y -1, paid zero, normalized spell 0x28e or 0x290, flags and node metric zero |

## Mutation ledger

| owner | successful found-spot effect |
|---|---|
| World Unit columns | clear flags bit `0x10`; clear mask bits `0x100` and `0x0400_0000`; write `orders_x`, `orders_y`, and `dest_angle` from current Move; `unit_masks2` is unchanged |
| World Unit order list | replace the attested empty list with current-first `[MoveTo, CastSpell]` |
| `Sim.paths[row]` | records become empty; capacity and increment are preserved |
| Object/terrain World | read by `find_merchant_spot`; no direct write |
| Guy / `unit_guys` | no access, no write |
| Caster / spell queue | no access; CAST_SPELL here is a Unit order, not an active Caster spell |
| Leader / Groups | no access, no write |
| game RNG | no draw and no write; the request and after-image retain the same state |
| OrdersMemManager | native Cast, Move, and two recycled-list-node allocations occur; pointer/freelist identity is outside the current canonical checksum owner |

The commit repeats the current actor, object-World digest, complete terrain-World checksum,
orders, path header, and RNG checks. The caller must also reinstall the capture's opaque
query-input digest at commit time; a changed collision/query authority is rejected. All fallible
work precedes the first store.

## Replay/checksum handoff

The request consumes the canonical setup-composition digest already owned by
`Frame379SetupReceipt`, the live o1/o2 Handle/type projections, `Sim.world`, `Sim.map.world`,
`Sim.paths`, and game RNG. It produces neither a post-frame-zero snapshot nor a retail channel
checksum.

The golden first-divergence/capture lane should answer the two requests in actual owner traversal
order: o1's pre-call image precedes its mutation; o2's pre-call image must include o1's committed
after-image. The existing independent frame-1 command-entry snapshot remains the enclosing
after-image and should reject the composed frame-zero schedule if either receipt is absent or
stale.

The Groups checksum lane consumes no mutation from this transaction: `Sim.groups` is untouched.
It does, however, require the post-frame-zero Unit owner to retain the exact two-order queues,
mask values, path headers, and stable o1/o2 identities before any later Group formation binds
those Units. Empty/frozen Groups agreement cannot substitute for this Unit chronology.
