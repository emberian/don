# 2024 frame-zero Dutch Merchant unpack boundary

This tranche covers the exact bounded success cone

`Unit::think` → `Unit::think_merchant` `0x005f4740` →
`Unit::unpack_merchant(3)` `0x006038e0`

for the supported golden replay's owner-0 Merchant Units `(who=0,o=1)` and `(who=0,o=2)`,
both concrete TypeIndex 62. It is derived from `riseofnations.exe` sha256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079` and the matching PDB.
The executable code sizes are 1,302 bytes for `think_merchant`, 452 bytes for
`unpack_merchant`, and 317 bytes for `find_merchant_spot`.

The transaction is detached in
`crates/don-replay/src/setup_2024_frame0_merchant_unpack.rs`; its source-owned search prefix is
in `crates/don-replay/src/setup_2024_frame0_merchant_search.rs`. It is not mounted in
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

## Read-only search prefix and first unsourced child

`unpack_merchant` calls

`Unit::find_merchant_spot(decoded_x, decoded_y, 3, &tile_x, &tile_y)`

at `0x0060390e`. `find_merchant_spot` is read-only for this call shape. Its exact read order is:

1. `UnitData::calc_gather` `0x00609180` with the fixed `0,0,0,...,0,0,1,1,x,y` call shape;
2. the radius-3 count at `0x00add1e0` and ordered offsets at `0x00adcaf0` / `0x00adc400`;
3. candidate bounds against the live World tile dimensions;
4. `UnitData::good_merchant_spot` `0x006068a0`;
5. `UnitData::invalid_loc` `0x00607c30` with all five optional flags zero;
6. `Unit::detect_unit_collision` `0x00617060` at `(tile_x*192,tile_y*192)`, footprint 1×1,
   with the three trailing flags zero.

The first layer of that search is now locally evaluated rather than hidden behind the whole-call
receipt. For concrete TypeIndex 62, `ObjectTypeData::upgrade_level` `0x00661090` reads the
replay-carried `TypeData::from` row. The supported row is terminal (`from < 0`), so the exact
level is zero and `calc_gather` uses radius four. A nonterminal row is rejected instead of
guessing the missing live type-relation graph.

`calc_gather` first probes a nonnegative `UnitData::good_obj` circle index when it is inside the
radius-four endpoint, then scans `circle_x/circle_y[0..circle_radius[4])`. Each coordinate uses
the signed `div_3_table` TCoord conversion, skips out-of-bounds tiles, rejects the Merchant
surface value `(tmask & 0x30) == 0x20`, and requires `tmask & 0x0200`. These tables and every
TData word come from canonical owners already in `Sim`.

The first qualifying tile calls

`ObjectsData::find_good_at(tile_x >> 2, tile_y >> 2, who, 0, 0)` at `0x0065bec0`.

That return value is the first child emitted by the terrain prefix. Its fifth argument is zero,
which selects a materially narrower retail arm than a whole-array Good scan. The continuation in
`setup_2024_frame0_merchant_good_lookup.rs` reads the requested WData cell's `down/down_who`,
follows every nonnegative Object address in exact chain order, and accepts only terminal
`down == -2`. Live Unit links come from the canonical World columns, Build links from the exact
opaque Build image, Wall links from `WallState`, and every address is resolved through the
canonical sparse object registry before its link is read.

The terminal `down_who` is the base-Good slot. This arm does not read `good_mark`, array order,
or Good coordinates. It filters the selected Good's active bit, dereferences its type pointer,
rejects TypeIndex 5 (Oil), then—because the fourth argument is zero and `who` is nonnegative—
calls `LeaderData::type_avail(good_type, 1)` at `0x006e33a0`. `OilGoodRuntime` supplies the exact
base-Good slot image; the historical name does not restrict its rows to Oil. The first remaining
typed child is that exact Leader/type query, bound to the complete WData/Object/Good read prefix.

That child is now source-owned for the golden replay. `GoodTypeData::num_preq` at `0x00470810`
returns the constant two. Every replay-carried Good row 6..49 has `tribe_mask == 0xffffffff`,
`preq == [-1,-1,-1]`, and `obs == -2`. Consequently `has_preq` resolves its two ordered
`get_preq` reads to `-1`, for which `has_tech(-1)` is true. `type_eligible(type,1)` then returns
four: `tribe_can_type` admits the live Leader's replay-bound tribe, and the strict Good arm sees
`has_tech(obs=-2) == false`. Finally, Good is neither Unit, Build, nor government, so
`type_avail` returns four before the Unit-only availability-bit read at `LeaderData + 0x6c18`.
No Leader tech-mask bit or `Type::is(123,0)` relation is reached on this path.

The source receipt binds the replay file and payload digests, admitted Rules digest and exact
Good spans, current type-row agreement, live Leader/tribe identity, ordered prerequisite reads,
raw `has_preq`/`tribe_can_type`/`type_eligible`/`type_avail` results, and unchanged RNG. Its digest
is installed as the existing detached capture's query-input digest, after which the whole
WData/Object/Good prefix is rerun and the transaction resolves to the Good TypeIndex. The legacy
capture-only API remains for native composition; the golden path no longer needs it to guess
availability. Neither branch mutates state or consumes RNG.

Malformed or cyclic object chains, an absent terminal Good slot, and an active Good with a null
type pointer fail closed. No WData resource/occupancy bit is treated as proof of a Good identity,
and no `good_mark`-bounded scan is invented for this call shape. The original search child still
binds the parent request, setup authority revision, actor identity, exact scan kind/index,
TCoord/WCoord, TData mask, and all five call arguments.

If the entire radius-four terrain scan contains no reachable call, `calc_gather` returns zero,
so `find_merchant_spot` returns `NotFound` before its outer candidate loop; the local proof is
composed directly into the existing `FAILED_UNPACK_TAIL_VA` boundary.

The completed Good receipt now resumes `calc_gather` rather than ending at the child. A `-1`
answer falls through from a cached probe to ordered index zero, or from an ordered probe to the
following index. A found Type returns one immediately. Every completed prefix is rerun against
the current World/Object/Good owners, so a stale object chain or changed Good slot is rejected.

The outer pieces are source-exact as well:
`move_x/move_y[0..radius[3])` supplies all 49 candidates in shipped order, and
`good_merchant_spot` checks the four `(tx,ty)` / minus-one corner masks in order. Each requires
valid TCoord bounds, no `0x4000`, `(low & 3) != 3`, and a nonnegative signed low byte before its
own fixed-shape `calc_gather` at `(tx*192,ty*192)`.

After that gather succeeds, `invalid_loc(tx,ty,0,0,0,0,0)` is locally complete for the golden
land Merchant. The empty order queue leaves both optional flags clear. Retail reads the WData
flags, then the TData surface/low bits and conditionally `WorldData::is_cliff_at`; the admitted
spot has low bits zero, surface `0x00` or `0x10`, and returns zero. Its receipt binds all reached
words and records the unchanged RNG.

The next exact residual is `Unit::detect_unit_collision(tx*192,ty*192,1,1,0,0,0)` at
`0x00617060`. Type 62's replay row has domain zero, `unit_flags & 0x20000 == 0`, and
`unit_flags2 & (0x20|0x40) == 0`: it is neither siege, hero, nor supply. Retail therefore jumps
past the flag-based early-zero arm and enters the spatial detector. The continuation emits this
call with the complete ordered Good trace, candidate identity, source-owned `invalid_loc`
receipt, raw type flags, and unchanged RNG. No empty-map collision result is guessed. A
collision answer is now the precise prerequisite before the successful spot can reach
`clear_partial_path`.

The collision child now has its own smallest source-owned prefix in
`setup_2024_frame0_merchant_collision_prefix.rs`. After the static Type predicates, retail
checks `UnitData::path` length at `+0xc0`; the golden request's empty `PathStack` means it does
not read a top-record flag. It next reads the canonical Unit SoA's signed `safe` byte at
`+0xb2`. A nonzero retry delay returns zero before converting either coordinate. With `safe ==
0`, the target and current coordinates are converted to quarter-tile UCoords in order; an
unchanged cell also returns zero.

For distinct UCoords, the first remaining call is
`CollCheck::collide_here(o,who,target_ux,target_uy,new_block_radius,&hit_x,&hit_y,0)` at
`0x00682540`. The radius comes from the replay-bound Type-62 `ObjectTypeData+0x248` row. This
child is the first mixed footprint/CollBlock operation: its non-overlay arm reads the mutable
World collision bitmaps and can memoize `CollBlock::flags` while testing emptiness. The typed
request binds the complete Merchant chronology, both UCoord pairs, radius, output-pointer shape,
the non-scratch arm, and unchanged RNG. It does not claim the current CollCheck slots or an
empty bitmap result.

No RNG call occurs. The legacy whole-call capture remains available for native composition and
carries an independently captured canonical-Sim SHA-256; Don does not manufacture that snapshot
or replace it with a locally guessed checksum.

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
