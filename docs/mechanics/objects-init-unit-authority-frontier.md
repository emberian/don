# `Objects::init_unit` authority frontier

Status: source-only, mutation-sensitive proof for the all-`-1` receiver path used by the direct
BHS 508--510 create-unit cohort. The isolated validator is
`crates/don-sim/src/systems/objects_init_unit_authority_frontier.rs`; its path-import proof is
`crates/don-sim/tests/objects_init_unit_authority_frontier.rs`. It is intentionally absent from
`systems/mod.rs` pending a shared allocator ownership decision.

## Recovered owner-band allocation

`Objects::init_unit` at `0x0065E0C0` reads `UnitTypeData::uber_size +0x308`. For the BHS tail
`(-1,-1,-1)`, it calls

```text
Objects::find_free(owner, 0, 2000, &unit_mark[owner], -1)
```

once for every member. `unit_mark` is `ObjectsData +0x15C`; it is a per-owner Unit-band
high-water mark, not the length of a freshly appended dense vector.

`find_free` first scans `[0, unit_mark)` in increasing index order. A slot is reusable only
when all of these hold:

- `SubObjectData::flags & 1 == 0`;
- `ObjectData::hold_frames +0x32 == 0`;
- the object is not a Unit, or its `UnitData::o_up +0x8E` is negative.

The final condition prevents an inactive subordinate squad member from being reused on its
own. Reusing a slot does not change `unit_mark`. If no reusable slot exists and the mark is
below 2,000, retail uses the slot at the mark and advances the mark. A null object slot causes
construction of a `Unit` for owner slots 0--7 or an `Animal` for slots 8--9, registration in
`Objects::lists[owner]`, `Units::make_valid(owner,o)`, and registration of the object's Unit
projection in the parallel `Units::lists[owner]`. A non-null sparse slot above a previously
lowered mark is reused as storage and the mark is still advanced.

When every existing slot is ineligible and `unit_mark >= 2000`, this path returns exactly
`-1`. It does not return arbitrary negative error codes. Heap-allocation failure is not a clean
allocator result in this routine: the following virtual access would fault. That distinction is
why the detailed model admits only the capacity `-1` as a native partial-completion result.

## Per-member commit chronology

For each successful `find_free` result `o`, the BHS path commits in this order:

1. The object slot and parallel Unit projection already exist because `find_free` owns them.
2. Invoke the complete `Unit::init` virtual body at vtable `+0x8C` with
   `(owner,type,o,x,y)`. The shipped Unit body is 3,732 bytes at `0x00612100`. Its integer
   return is discarded, including a negative value.
3. Store `current.o_up = previous` at `UnitData +0x8E`.
4. The first member has `o_up=-1`, so it skips every internal Leader/location operation. It
   remains at the coordinate established by `Unit::init`.
5. For a linked member, apply the Leader correction gate. It runs when `control_cost != 0`,
   when the type `is(FIGHTERBOMBER=0x134,0)`, or when `UnitData::is_gov_hero` is true. The
   concrete `is_gov_hero` fast path is `unit_flags & 0x04000000`. Earlier labels such as
   “helicopter” or “spy” are incorrect for this code.
6. When admitted, call the complete 326-byte
   `Leader::track_unit_type(type,-1,current_o)` body at `0x006E0DD0`, then directly subtract
   `control_cost` unless `current.unit_masks & 1`, decrement `LeaderData::active`, and decrement
   `LeaderData::units_built`.
7. Resolve the current member's captain through the `get_captain` virtual. The concrete
   `UnitData::get_captain` at `0x00610AB0` follows `o_up`; derived implementations remain a
   virtual authority.
8. Call the originally requested UnitType's `find_nearby_spot`, but center it on the resolved
   captain. The radius source is the **captain's current type**
   `ObjectTypeData::new_block_radius +0x248`: `min = radius*0x30`,
   `max = radius*0x60+0xC0`. The angle is the captain object's `+0x50` field. The remaining
   arguments are `(-1, angle, 3, current_o, owner, 0, 0, -1, 0, -1)`.
9. Nearby return zero selects the output cells. Any nonzero return selects the member's
   post-`Unit::init` x/y instead. In either case call the complete 1,757-byte
   `Unit::set_new_location(x,y,1,1)` at `0x005F8D20`; its return is also ignored.
10. Only after location commits, store `previous.o_down = current` at `UnitData +0x90`.

After the last member, retail resolves captain again from the last member and returns that
second resolution. The internal resolution used for location is not reused as the public
result.

This order is non-atomic. A later `find_free == -1` returns immediately and leaves every
earlier member initialized, corrected, located, and linked. The failed attempt contributes only
its allocator scan; there is no rollback. Conversely, nearby nonzero and negative returns from
`Unit::init` or `set_new_location` are not clean transaction failures.

## Leader subtransaction

The proof models both `track_unit_type(type,-1,o)` and the three direct corrections. The
tracked `num_units[type]` value is an unsigned-short wrapping decrement. With delta `-1`, the
routine also decrements the following applicable bands:

- attacking Barracks/Stable units decrement their training counter and `combat_units`;
- attacking Factory, Dock, or fallback domain-2 units decrement `factory_units`, `dock_units`,
  or `air_units` respectively;
- `is_peasant`, otherwise `is_scholar`, otherwise role bit `0x10`, decrements `peasants`,
  `scholars`, or `scouts`;
- type relation `0x42` with a member former-type relation `0x34` decrements
  `scholar_militia`;
- otherwise type relation `0x4B` clears or sets Leader flag `0x20000` according to the new
  unsigned-short type count.

The direct corrections that follow are exact PDB fields, not unnamed array cells:
`units_built +0x808`, `active +0x93C`, and `control +0x940` relative to `LeaderData`.

## Scope boundary: explicit-object mode

The other `Objects::init_unit` mode (`exact_o >= 0`) is recovered but intentionally not claimed
by the source validator. It forces one member, tells `find_free` to fill sparse slots through
the requested object index, writes caller-provided `o_up/o_down`, may return immediately when
`o_up < 0`, and guards its optional internal placement with a map-cell equivalence test. Its
return/location shape is different from the BHS all-`-1` path. Merging it into the BHS proof
would widen the integration claim without helping the 5,272-call cohort.

## Ownership audit and exact seam

No current shared Sim owner can implement this receipt:

- `ObjectRegistry` stores dense per-band vectors. `insert` always appends and `remove` swaps a
  tail row into the hole. It has no sparse retained storage, inactive-slot scan, independent
  high-water mark, or `o_up`-aware reuse rule.
- `World::allocate_typed_at` creates exactly one zeroed SoA row, appends it to that dense
  registry, fills a small field prefix, and increments `live`. It cannot construct `uber_size`
  members, reuse a captain slot, run `Unit::init`, preserve parallel object/Unit bands, correct
  subordinate Leader counts, run internal nearby/location, or expose the native partial
  chronology.
- `production_runtime::SimFinishedHost::allocate_unit` delegates to
  `World::allocate_typed_at`, then increments only its approximated control and type count. The
  trait comment names `Objects::init_unit`, but its `UnitAllocationReceipt { object_id }` cannot
  attest this transaction.

The integration seam is therefore one canonical mutable `Objects::init_unit` authority shared
by production, cheat init, carrier payload, and BHS. Its minimum BHS receipt must include:

```text
request + uber_size/type facts
per member:
  exact find_free scan/disposition and unit_mark transition
  complete Unit::init receipt (return retained but ignored)
  o_up store
  optional complete Leader track + direct corrections
  captain + exact internal nearby + complete set_new_location
  prior o_down store
terminal capacity -1 OR final second captain resolution
```

A future live adapter should replace the production single-row implementation at this seam;
adding a BHS-only second allocator would duplicate the same authority gap. No shared
`World`, production, BHS runtime, tick, order, or schedule file was changed in this tranche.

## Verification

```sh
cargo test -p don-sim --test objects_init_unit_authority_frontier
```

The ten tests freeze native addresses, sizes, and offsets; the owner 0--7 / 8--9 storage-class
split; sparse reuse eligibility; dual-band registration; ignored nested returns; Leader
wrapping/corrections; exact internal placement fallback; link chronology; capacity-only `-1`;
and the final second captain resolution.
