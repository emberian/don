# Opcode 49 canonical `Unit::come_out` runtime

Opcode 49 (`ComeOutCommand`, 11 wire bytes) now has one executable canonical cohort without
claiming whole-row closure.  An active ordinary-land captain with exactly one live Guy may leave
an active same-owner Build whose gather list is empty.  Every other receiver remains fail-closed.

The transaction composes the 532-byte `Unit::action_come_out` wrapper with all four landed
`Unit::come_out` planner tranches.  `unit_come_out_body_map` supplies canonical identity, point,
RNG, and continuation conversions at each source-file seam; its existing map still accounts for
all 9,925 retail bytes.  The admitted route is:

1. contained placement in the prefix;
2. common `set_new_location` and Build/captain release;
3. the empty gather-list fallback;
4. the release tail's container facing and unconditional `options.rebuild` store.

Before the first write, the host binds the decoded `(who, object, uid)` to the live Unit row,
installed type facts, containing Build, Group membership, Order list, path stack, contained
collision source, and exact one-Guy graphics source.  It recomputes the wrapper and every reached
planner, validates the continuation dialects, and constructs a complete before/after receipt.
The collision owner then performs the only fallible commit: clearing containment, linking the
World anchor, stamping the Guy footprint, publishing location/facing, clearing movement/action,
and advancing its revision.  Remaining Group, Order, path, mask, graphics, height, animation-hint,
and rebuild-latch writes are infallible after that preflight.

`UnitComeOutResumePayload` is a versioned `CO49` binary payload for the external type/search facts
and epochs which the compact Sim does not store in generated columns.  Decode rejects truncation,
unknown versions, invalid identities/booleans, oversized Guy arrays, and trailing bytes.  The real
Bridge test saves and resumes this payload before sending an actual opcode-49 packet, and a second
test proves that a missing graphics owner returns `Unavailable` without changing canonical state.

This does **not** promote opcode 49's schema row.  Scholar/University repair, uncontained release,
Oil Platform transport, multi-Guy formations, non-empty gather selection, order-dispatch branches,
special animation, army insertion/RNG, and other body branches are not yet hosted end to end.
