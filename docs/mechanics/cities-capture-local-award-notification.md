# `Cities::capture_city` local award notification

Implementation tranche:
`crates/don-sim/src/systems/combat/cities_capture_local_award_notification.rs`.

Fidelity tier: **C**. This is an instruction-bounded recovery of the complete local
new-owner presentation cone. All heap, `String`, recycler, color, and UI effects remain
request-bound host receipts; no retail differential has promoted the tranche.

## Provenance and exact boundary

- `ron-bin/riseofnations.exe` SHA-256:
  `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`
- `ron-bin/sbl/rise.pdb` SHA-256:
  `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`
- PDB procedure: `int Cities::capture_city(int new_owner, int old_city, int old_owner)`,
  VA `0x00733380`, size 7,998 (`0x1F3E`), exclusive end `0x007352BE`, source
  `cities.cpp:87..733`.
- This tranche: `0x00734152..0x0073432D`, 475 (`0x1DB`) bytes.
- Cumulative recovered prefix: `0x00733380..0x0073432D`, 4,013 (`0xFAD`) bytes.
- Exact residual: `0x0073432D..0x007352BE`, 3,985 (`0xF91`) bytes.

The start is the first zero-initialization of the local award-message `String`, reached
only after the preceding cone proved that the console player equals the new owner. The
exclusive end is the old-owner-refund local test at `0x0073432D`. The bounded cone has
no internal CFG exit: after successful presentation and reverse-order `String` cleanup,
it always converges on that test.

## Compiled String and localization order

Disassembly and PDB names freeze the following sequence:

1. initialize three 20-byte local `String` objects and read the single global
   `STR_MODULE_ID` byte at `0x00CC2311`;
2. copy the localized `String` at table byte offset `0x1504` into the bubble-text local;
3. call `String::parse(int)` at `0x00A1CDD0` with the new-owner award, assign its
   temporary result back to the bubble-text local, then immediately call
   `String::close` at `0x00A1CF40` on the temporary;
4. copy the localized `String` at table byte offset `0x1518` into the message local;
5. read the captured city record's name `String` at `+0x90` and call
   `String::parse(const String&, int)` at `0x00A1B9A0` with city name then award;
6. assign that temporary back to the message local and immediately close the temporary.

The implementation represents String values with opaque tokens. Every assignment binds
its phase, destination slot, exact before image, and source token; a host may not report
success with a different after image. Each temporary and final close is separately
receipted, preserving lifetime order without pretending Rust owns retail's allocator or
reference-count representation.

## Bubble, owner color, and MessageWin mutation order

After both formatted Strings are complete, retail performs these host effects in order:

1. `Recycler<TextBubble>::pop()` at `0x0046E620`;
2. `String::operator=` into `TextBubble + 0x04` from the formatted amount String;
3. write the new-owner byte to `TextBubble + 0x1A`;
4. read the new owner's color index, form `team_colors + color_index * 100`, and call
   `TeamColor::get_neon()` at `0x008C2CC0`;
5. only now read the captured city's coordinates at record offsets `+0x0C` and `+0x10`;
6. call `MessageWin::add_message()` at `0x007E9FB0` with the formatted city/award
   message, city coordinates, category `-7`, neon color, duration `1`, height `0`, and
   the allocated bubble;
7. set the capture driver's notification-raised local to one.

The shipped PDB signature is
`add_message(const String&, Coord, Coord, int, Color&, int, int, int, TextBubble*, int)`.
The compiled call passes the same `TeamColor*` identity in both otherwise-unused integer
positions. The model retains a typed `TeamColorKey { color_index, entry_stride: 100 }`
in both positions rather than freezing an ASLR-sensitive pointer value. The add-message
receipt binds the complete request and synchronous mutation completion.

City name and city coordinates deliberately use separate host reads. The name read
precedes bubble allocation; the coordinate read follows owner-color and neon lookup.
Bundling them into one city-presentation snapshot would falsely move one of those reads
across externally visible allocations and mutations.

## Cleanup and typed continuation

After `MessageWin::add_message`, retail closes the locals in exact destructor order:

1. unused third String (`0x00734304`),
2. formatted bubble String (`0x00734310`),
3. formatted message String (`0x0073431F`).

Only then does execution reach
`CitiesCaptureLocalAwardNotificationContinuation::OldOwnerRefund0x0073432d`.
The planner accepts only the prior
`LocalAwardNotification0x00734152` receipt, requires a present nonzero award, and binds
the captured city owner to the new owner. It also requires the prior notification local
to remain false and carries the old-owner-refund value unchanged as the exact input to
the `cmp [ebp-0x20], 0` at the continuation. Zero-award and nonlocal-player paths already
entered `0x0073432D` directly and cannot be replayed through this presentation cone.

The focused private path-import test freezes all addresses and constants, the four
String assignment phases, both parse argument shapes, city-name versus coordinate read
placement, bubble text/owner mutations, TeamColor identity reuse, exact add-message ABI
request, reverse cleanup, the single continuation, planner rejection, and fail-closed
assignment/UI/close receipts. No shared combat export or prior frozen file is changed.

The exact residual is `0x0073432D..0x007352BE`, 3,985 bytes. It begins with the
old-owner refund resource loop and its local notification, then retains alternate
non-capital plunder allocation, later capital/recapture diplomacy and scoring,
presentation tails, SimpleArray cleanup, and the returned city index. None of those
effects are claimed here.
