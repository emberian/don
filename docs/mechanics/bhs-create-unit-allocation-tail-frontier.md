# BHS create-unit allocation-tail frontier

Status: source-only, mutation-sensitive owner boundary for the repeated direct-route tail of
BHS registrations 508--510. The isolated model is
`crates/don-sim/src/systems/bhs_create_unit_allocation_tail_frontier.rs`; its path-import test is
`crates/don-sim/tests/bhs_create_unit_allocation_tail_frontier.rs`. It is intentionally absent
from `systems/mod.rs` until the live owner described below is available.

## Recovered direct attempt

Ground on land, Sea on ocean, and every Air route meet at `0x009E23B6`. Each positive-count
iteration performs this exact pair:

1. Call the **graft** `UnitType::find_nearby_spot` at `0x009E23F4`, not the originally named
   type. The origin is the wrapping fine coordinate `script * 192 + 96`. The twelve arguments
   after the two output pointers are
   `[0, 0xC00, 0, 0x55555555, 3, -1, -1, 0, 0, -1, 0, -1]`.
2. Ignore the nearby function's integer return unconditionally. The output cells are fields in
   the stack-local Group storage; whatever values remain there become the allocation position.
3. Call `Objects::init_unit` at `0x009E2412` with
   `(owner=who-1, graft_type, output_x, output_y, -1, -1, -1)`.
4. If the return is nonnegative, immediately append that captain id to the local Group and
   persistent scenario numeric group. A negative return skips publication but does not stop the
   count loop. The call returns the last attempt's value.

Two tempting substitutions are therefore wrong: treating nearby nonzero as a failed BHS
attempt, and allocating the originally named/effective type instead of its Leader graft.

## Why `Objects::init_unit` must remain one authority

The 1,603-byte receiver at `0x0065E0C0` is a multi-object mutation, not a row insertion. For the
all-`-1` BHS tail it reads `UnitTypeData::uber_size +0x308`, then repeatedly:

- calls `Objects::find_free` at `0x0065E137` in the owner's Unit band;
- invokes the new object's initialization vcall at `+0x8C` with owner, type, object id, x, y;
- links members through Unit offsets `+0x8E/+0x90`;
- updates Leader tracking/counters where the native type/object gates admit it;
- resolves the captain and performs the receiver's internal nearby/location transaction.

If `find_free` becomes negative, that value returns immediately. Earlier members are not
removed. On full success the public result is the final member's resolved captain, not
necessarily the last allocated member. The source model therefore requires a
`CompleteRetail1603ByteBody` receipt and separately records required member count, initialized
member ids, a terminal allocator failure, and the captain-or-failure return. This is the minimum
shape that preserves a negative result with live partial effects.

## Ownership audit

No current owner can honestly implement the new authority trait:

- `World::allocate_typed_at` creates one zeroed SoA row. It does not execute graft nearby
  placement, `uber_size` member construction, native object initialization, linking, Leader
  tracking, internal location/collision work, or partial allocator failure.
- `production::UnitCompletionHost::allocate_unit` has the seven-argument call shape, but the
  live implementation delegates to `World::allocate_typed_at` and its receipt reports only one
  integer. It cannot attest the complete native receiver.
- opcode-67's `CheatInitUnitTransaction` owns a different caller policy: its all-player branch
  allocates only when nearby returns the success code. BHS deliberately ignores that code.
- containment/gathering nearby ports own useful geometry but not the joined UnitType, RNG,
  object-band, Leader-counter, Unit-column, collision, and linkage transaction.

This audit also exposes a broader convergence seam: production, cheat init, carrier payload,
and BHS all need a canonical complete `Objects::init_unit` owner. That owner should replace the
single-row production adapter rather than adding a second approximate allocator for BHS.

## Exact integration seam

After the shared BHS runtime's route plan and persistent `who-1` clear, the direct route can call
one new mutable host method with:

```text
(attempt ordinal, owner=leader_slot, graft_type, fine origin x/y)
    -> nearby receipt
    -> complete Objects::init_unit receipt
    -> captain id or negative allocator failure
```

Concretely, `CreateUnitLiveHost` must become mutation-capable for this method, and
`BhsCreateUnitRuntime::dispatch` must accept `&mut H`. `SimScriptHost` already owns `&mut H`, so
the borrow topology is compatible. After each validated nonnegative receipt the runtime must
publish to the local Group and numeric key `who` before requesting the next attempt. Invalid or
unavailable receipts are owner faults with all earlier effects retained; they cannot be mapped
to native `-1`.

This seam closes only the repeated allocation spine. A completed registration still requires
the Air strafe/timer suffix, Carrier child payload, final Groups push/form/jump path, and the
Ground-on-ocean transport branch. Until those owners and save/checksum coverage exist, the
5,272-call cohort remains prefix-reachable rather than fully handled.

## Verification

```sh
cargo test -p don-sim --test bhs_create_unit_allocation_tail_frontier
```

The proof pack freezes the native ABIs and call sites, ignores nearby return while forwarding
its outputs, rejects request/extent substitution, admits `Objects::init_unit` partial failure,
preserves immediate publication and last-result semantics, and retains earlier receipts on an
authority fault.
