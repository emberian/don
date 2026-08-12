# Web canonical package source ABI

Date: 2026-08-11. Scope: source-only prerequisite for the browser Group → Move bridge.
This tranche does **not** rebuild or publish `web/public/wasm/don_web.wasm`, add a JavaScript
wrapper, advertise a capability bit, or claim browser execution.

## Authoritative command identity

`web/wasm/src/game_abi.rs` now exposes:

```text
game_object_command_identity(game, renderer_id:i32) -> 1 live Unit / 0 absent
game_object_command_identity_ptr(game) -> [who:i32, o:i32, uid:i32]
```

The renderer id is only a lookup key. The adapter finds the current dense row carrying that live
handle id, reconstitutes its full `Handle { id, generation }`, and requires the canonical sparse
`World::unit_row_at(who,o)` address to resolve to that same row. It also requires the Unit active
flag. Only then does it publish `(who,o,uid)`. Build projection ids, negative ids, ids with no
current live Unit, tombstones, malformed sparse addresses, and inactive Units return zero and
reset the three-word scratch image to `[-1,-1,-1]`.

The current renderer key contains only `Handle.id`, not `Handle.generation`. This resolver proves
the key's **current** live binding; it cannot distinguish a browser key retained across a later id
reuse from a key freshly observed for the replacement Unit. Product activation therefore still
requires either generational renderer keys or caller-side selection invalidation on identity
turnover. The canonical Sim transaction remains ABA-safe once invoked because its plan captures
and revalidates the full Handle plus `(who,o,uid)`.

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

The call refuses before Sim when `len` is outside the staging allocation or split commands are
pending through `game_submit`. It clears the prior receipt before every attempt. Sim then owns
decoding, play→owner validation, exact external movement/type authority, Group allocation,
backlinks, order/path after-images, stale identity checks, and atomic commit/rollback. A typed
Sim refusal is copied to the existing error-text buffer and publishes zero receipt words.

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

## Honest activation boundary

The default playable `Game` still lacks two required external owners:

- the retail lifecycle `play → who` `PlayerTable` used by package receive; and
- a complete `GroupMoveAuthority` with exact per-Unit formation/type/movement facts.

Consequently the source ABI fails closed with `MissingPlayerMap` and then `MissingAuthority`
until those owners are mounted. The focused native test installs explicit fixture authority only
after proving both refusals leave the core digest and receipt unchanged. It then applies one real
27-byte Group+Move package through `Sim`, verifies the complete 16-word singleton receipt and RNG
equality, and mutation-kills the second opcode while preserving the post-success digest.

Browser activation remains gated on exact product lifecycle and movement-authority installation,
a JavaScript wrapper that copies scratch words only after each possibly allocating call, a newly
built Wasm artifact with an audited export list, two-tab receipt/state convergence, and unchanged
Halt-cohort evidence.
