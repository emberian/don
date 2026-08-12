# Web canonical package source ABI

Date: 2026-08-11. Scope: source-only prerequisite for the browser Group → Move bridge.
This tranche does **not** rebuild or publish `web/public/wasm/don_web.wasm`, connect the live
client, advertise a capability bit, or claim browser execution. It does add a guarded JavaScript
wrapper over the source ABI; against the current artifact that wrapper explicitly reports that the
exports are absent.

## Authoritative command identity

`web/wasm/src/game_abi.rs` now exposes:

```text
game_object_command_identity(game, renderer_id:i32) -> 1 live Unit / 0 absent
game_object_command_identity_ptr(game) -> [generation:i32, who:i32, o:i32, uid:i32]
```

The renderer id is only a lookup key. The adapter finds the current dense row carrying that live
handle id, reconstitutes its full `Handle { id, generation }`, and requires the canonical sparse
`World::unit_row_at(who,o)` address to resolve to that same row. It also requires the Unit active
flag. Only then does it publish `(generation,who,o,uid)`. Build projection ids, negative ids, ids with no
current live Unit, tombstones, malformed sparse addresses, and inactive Units return zero and
reset the four-word scratch image to `[-1,-1,-1,-1]`.

The lookup also retains the full `UnitIdentity { Handle { id, generation }, who, o, uid }` as a
one-shot package lease. Every package attempt consumes and clears the lease and scratch image. The
entry point re-resolves the exact generational Handle, revalidates the live active row and sparse
owner-local address, decodes the staged package, and requires its explicit Group selection to be
the leased singleton. A renderer id retained across despawn/id reuse therefore names a stale lease;
it cannot silently command the replacement Unit even when id, `who`, `o`, and `uid` are identical.

No renderer/global id is narrowed into retail `o`. `uid` is discovery/ABA evidence for the
caller; Group wire bytes remain exactly `(who,o)`.

## Whole-package transaction

The existing bounded `game_cmd_ptr` allocation stages one plaintext package. The new immediate
entry point is:

```text
game_process_command_package(game, play:u32, lockstep_serial:i32, len:u32)
    -> 1 applied / 0 refused
game_package_receipt_ptr(game) -> receipt i32 words
game_package_receipt_words(game) -> word count
```

It calls the landed owner directly:

```rust,ignore
Sim::process_command_package(play, lockstep_serial, bytes)
    -> Result<GroupMovePackageReceipt, PackageError>
```

The call refuses before Sim when no one-shot identity lease exists, `len` is outside the staging
allocation, split commands are pending through `game_submit`, the package is not the exact
Group→Move cohort, the selection is not the leased explicit singleton, or the leased generational
identity changed. It clears the prior receipt before and consumes the lease on every attempt. Sim
then owns play→owner validation, exact external movement/type authority, Group allocation,
backlinks, order/path after-images, stale identity checks, and atomic commit/rollback. A typed Sim
refusal is copied to the existing error-text buffer and publishes zero receipt words.

The successful receipt image is:

```text
header = [play, lockstep_serial, frame, who, group_slot, selected_len,
          command_state_revision_lo, command_state_revision_hi,
          groups_checksum, random_state_before, random_state_after]
unit   = [handle.id, handle.generation, who, o, uid]
words  = 11 + selected_len * 5
```

The two revision words and checksum/Handle words preserve their raw `u32` bit images in Wasm
`i32` slots. Handle/generation is receipt evidence only and never becomes a future Group wire
field. The adapter refreshes browser render projections after success; it does not route the
package through the old split command queue.

## Lifecycle ownership

Successful `game_start_manual_teams` now mounts the live retail lifecycle `PlayerTable` from the
**applied** `PlayerSetup`, after team-style transformations have resolved. For each active browser
slot, `play == who`, `PLAYER_PRESENT` is set, and the applied team is retained; console play/who is
the requested active local slot. A failed setup still installs nothing. This table is Sim-owned,
included in the canonical save image, and survives load exactly.

Load deliberately clears adapter-only command identity scratch, the one-shot lease, selections,
and receipt. The loaded play→who table remains authoritative, while externally derived
`GroupMoveAuthority` remains unavailable until its product owner reinstalls it. Thus a freshly
captured post-load package reaches `MissingAuthority`, not `MissingPlayerMap`.

## Guarded JavaScript wrapper

`GameModule.commandIdentity(rendererId)` copies the four identity words. The combined
`processCanonicalCommandPackage(play, serial, rendererId, bytes)` captures that lease and stages
and processes the bytes synchronously, then copies the receipt from the current Wasm memory buffer.
It validates play, serial, unchanged RNG, and exact Handle-generation/owner-local identity before
returning an immutable decoded receipt. These methods do not participate in the public Wasm
contract or capability bits, and fail explicitly while the checked-in artifact lacks the exports.

Focused native tests prove lifecycle installation, exact receipt binding, and atomic refusals for
second lease use, altered bytes, wrong play→who, renderer id reuse at a new generation, and load
invalidation. Node tests use fake exports to prove staging/receipt copying and mutation-kill
identity, RNG, and extent mismatches without implying browser execution.

Browser activation remains gated on complete product movement-authority installation, a newly
built Wasm artifact with an audited export list, live client integration, two-tab receipt/state
convergence, and unchanged Halt-cohort evidence.

## Product authority prerequisite (`DONPACK3`)

The checked-in source now has an isolated product join in
`don_sim::systems::group_move_authority`. It constructs `GroupMoveAuthority` from the live
generational `World`, Sim order/path owners, live `LeaderData` flags and tech bit 0x12, terrain,
and one immutable content interface. The producer includes every live active Unit because the
retail fixed Group allocator may normalize a previous small Group while choosing a slot. Its
composition digest covers every instance identity and every projected dynamic/static field.

`DONPACK3` is deliberately incompatible with `DONPACK2`. Each of its 364 dense TypeIndex rows now
also carries the exact postload `unit_flags2`, `guy_spacing`, `x_spacing`, `y_spacing`, and
`uber_size` inputs needed by `FormData::type_cat` `0x0072DFC0` and `Form::categorize`
`0x0072E250`. The parser requires all type ids 50..413 exactly once, positive formation geometry,
the exact 16-rule and 493x493 balance shapes, exact total length, and authority schema 1. A stale,
short, duplicate, synthetic, or old-version pack cannot supply command authority. It is rejected;
no formation defaults are installed.

Dynamic predicates remain instance-owned. The producer reads `is_on_map`, captain/subordinate and
containment links, `unit_masks & 0x1000` (`is_blown`), the concrete SpecialAnim enter/exit
discriminator, current form width/angle, path-row availability, and Handle generation from Sim.
A malformed SpecialAnim fails the whole projection. A missing path leaves the member unable to
install an order. A recycled object id cannot consume a resolved fact for the old generation.

One blocker is now explicit rather than papered over. `UnitData::speed` `0x0060AAE0` returns the
live `myspeed` field immediately only when the effective type domain is non-land. Land Units then
read terrain rule predicates A6..A9, nearby-object/type predicates 0x166/0x167, leader flag 0x8000,
four leader counters, type predicates 0x105/0x167, and eleven Constants fields. The browser Sim
does not yet own that complete evaluation, and its scenario spawn currently initializes
`myspeed` from a deterministic random draw. Therefore `MOVES` and `myspeed` are not accepted as a
retail land-speed substitute. `GroupMoveContent::resolved_land_speed` must return a value bound to
the exact `Handle`; the real `GameData` adapter intentionally returns unavailable today.

Consequently this prerequisite still does not activate the client, build/publish a Wasm artifact,
or advertise a capability. A real browser land Group→Move remains fail-closed at
`MissingResolvedLandSpeed` until the remaining retail speed owners are recovered and installed.
