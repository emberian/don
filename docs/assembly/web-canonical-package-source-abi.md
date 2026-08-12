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
<<<<<<< HEAD
and receipt. The loaded play→who table remains authoritative. The collision runtime is not a
canonical save section, so `game_load_commit` resolves a complete generation-bound DONPACK5 source
batch against the decoded core before swapping it live. The Sim rehydration transaction verifies
every active Handle, immutable source, saved WData anchor, and one-Guy position/angle before one
assignment installs the pointer-free sidecar. It neither relinks anchors nor restamps the saved
collision bitmap. Any missing, duplicate, stale, contained, foreign, or altered source refuses the
whole load while the previous live core remains unchanged.

The browser constructor opens all four object registries to allocate their starting rows. The
manual-setup adapter captures those bits, returns the registries to the inactive pre-setup image,
and then lets `PlayerSetup` activate exactly its requested cohort. A refusal restores every prior
bit; success no longer leaves construction-only activity on slots two and three that makes
canonical save validation diverge from its applied setup receipt.
=======
and receipt. The loaded play→who table remains authoritative. The collision runtime is not yet a
canonical save section, however, so the product-authority join now preflights it and fails closed
at `Movement(MissingSource)` after load. A freshly captured post-load package cannot publish a
movement receipt until exact collision-source rehydration is implemented.
>>>>>>> 37d63cf ((sweep-up commit due to codex wall))

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

## Product authority lifecycle (`DONPACK5`)

The product join in `don_sim::systems::group_move_authority` now consumes the live generational
`World`, order/path owners, terrain, diplomacy/leader/hero facts, exact recovered land-speed
producer, and immutable type content. `DONPACK5` extends the fail-closed `DONPACK4` record from 28
to 34 words with the six collision-body inputs `new_block_radius`, `big_radius`, `push_size`,
`push_circles`, `ABIL`, and `SQUAD_SIZE`. The packer admits the measured one-Guy cohort only;
synthetic, stale-magic, short, duplicate, or malformed content cannot install movement authority.

`GameData::browser_collision_source` constructs the explicit live one-Guy land body. It binds the
type domain and radii, push facts, both unit-flag words, attack/spell inputs, and the current
generational Unit position/angle. `Game::new` resolves every row before its first spatial write,
then lets the Sim collision owner validate and atomically publish the WData anchor, Guy footprint,
and identity-bound source. The Group-Move product join also preflights the complete collision
runtime before producing authority, so a missing, foreign, unlinked, or malformed source refuses
the package before the Sim transaction.

The browser lifecycle's default terrain has no adapter-supplied invalid-tile overlay and every
spawned browser Unit is one on-map land Guy. Other formation bodies and post-load collision-source
rehydration remain outside this bounded producer and therefore fail closed.

## One coordinate scale and atomic initial path

Canonical command coordinates, browser projections, terrain lookups, and movement paths now use
the terrain/movement tile scale throughout:

```text
COORD_PER_TILE = 192
MAP_TILES       = 128
MAP_SPAN        = 24,576
```

The prior browser projection used `world::COORD_PER_TILE == 768` while the pathfinder and collision
terrain used 192. That made an accepted Group-Move destination valid to the command adapter but
off-map to movement. `game_abi` and `canonical_group_move_host` now share the 192-unit constant and
clamp before formation placement. The remaining `0x300` arithmetic in the host is only the
recovered retail order-remainder field, not a map-bound scale.

When a MoveTo becomes the current order, the same package after-image now publishes a one-record
`PathStack` to the exact formation-adjusted destination with tolerance zero and `FLAG_MORE`. Order,
orders-x/y, and path therefore commit or roll back together. Appending a queued order does not
replace the current path early. This direct initial path is the authoritative repath for the
bounded flat browser lifecycle; collision remains responsible for rejecting or detouring an
occupied step.

Focused source evidence covers:

- 12 pure host tests, including 192-scale edge clamping and order/path atomicity;
- exact DONPACK5 parsing plus stale DONPACK4 rejection and collision-row projection;
- two independently created native ABI `Game` instances processing both players' leased
  Group→Move packages with identical receipts, frame digests, and RNG after every step; and
<<<<<<< HEAD
- actual position change for both selected land Units within 32 frames in both instances, followed
  by canonical save/load, a second receipt pair, and a second position change in both instances.

Focused collision-runtime tests also rehydrate a complete two-Unit loaded image without changing
its saved anchors or bitmap and prove that altering the second Guy body rejects atomically before
the first source becomes visible.
=======
- actual position change for both selected land Units within 32 frames in both instances.
>>>>>>> 37d63cf ((sweep-up commit due to codex wall))

The dormant client helper now chooses an inward destination from the authoritative exported span
and passes only strict `{who,o,uid}` input to the package encoder; renderer id and generation remain
lease evidence and never become retail `o`.

This remains source-only. No checked-in Wasm artifact, capability/version bit, or client default is
changed. Activation still requires an exact-commit reproducible Wasm build and the same two-instance
receipt/frame/digest/RNG **and position-change** proof against the compiled artifact. Browser proof
<<<<<<< HEAD
is additionally unavailable while the required Chrome target is absent.
=======
is additionally unavailable while the required Chrome target is absent, and load/resume remains
red until the collision runtime has a generation-safe canonical rehydration transaction.
>>>>>>> 37d63cf ((sweep-up commit due to codex wall))
