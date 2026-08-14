# Citizen `Unit::think` Leader pending-bit owner

The golden 2024 setup reaches four type-50 Citizens. Their exact idle continuation enters
`Unit::think` `0x005F6E40`; after accepting type 50 or 51 at `0x005F6F83`, retail clears the
instance Unit masks at `0x005F6F91` and then executes:

```text
005f6f8d  movzx eax, byte [unit+0x09]      ; owner
005f6f98  imul  eax, eax, 0x6eec
005f6f9e  add   eax, 0x00e3a390            ; Leaders::list
005f6fa3  or    dword [eax], 0x00080000    ; LeaderData::leader_flags
```

The later `leader_flags2 & 2` test at `0x005F6FDE` can stop the AI continuation but cannot
undo or suppress this earlier write.

DoN has one retail `leader_flags` dword mirrored in
`Sim.vic_leaders.slots[owner].leader_flags` and `Sim.step8.leaders[owner].flags`. The Leaders
checksum frontier agreement-checks those mirrors, so updating only one is not a valid partial
port. `leader_unit_think_pending` provides a revision/digest/actor/owner-bound prepare/commit
seam:

- prepare refuses a missing authority, a non-type-50/51 actor, an out-of-range or inactive
  owner, and drift in either the `leader_flags` mirrors or the later `leader_flags2` gate;
- commit rechecks the exact source and both before-images before either store;
- the successful commit ORs `leaders::flag::PENDING` into both mirrors and returns their exact
  before/after image; and
- repeated retail ORs are represented honestly as successful idempotent transactions.

The receipt also retains the agreed `leader_flags2` value and whether its `UNIT_AI_OFF` bit is
set, so the caller can reproduce the later `0x005F6FDE` return without rereading an unbound
mirror.

This seam does not claim the remaining Citizen idle/search/animation body and does not itself
advance the golden replay chronology. It is the atomic Leader-side child receipt required by
that continuation. Step 8 later clears the same request bit at `0x006ED407`, which remains the
existing live Leader runtime's responsibility.

Focused gate:

```sh
cargo test -p don-sim leader_unit_think_pending
```
