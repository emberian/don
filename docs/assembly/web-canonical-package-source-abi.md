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
