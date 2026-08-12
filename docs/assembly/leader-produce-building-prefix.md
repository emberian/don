# `Leader::produce_building` City-gate prefix

This tranche owns the first exact terminal branch inside `Leader::produce_building` without
pretending that the remaining placement transaction succeeded. Ground truth is the supported
`riseofnations.exe` (SHA-256 `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`)
and matching `rise.pdb` (SHA-256
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`).

## Function and return polarity

The PDB fixes `Leader::produce_building(int type, int origin_o, int mode)` at `0x006E1400`,
7,406 bytes, with exclusive end `0x006E30EE`.

The native return convention is zero on success and one on failure. The BHS builtin-521 caller
at `0x009F5651..0x009F565B` executes `neg eax; sbb eax,eax; inc eax`, which is exactly
`native_return == 0`. Consequently an owned native early-return value of one becomes scenario
zero. Treating native nonzero as BHS success would invert the retail contract.

## Exact first gate

The entry through `0x006E150C` is read-only. It resolves the supplied origin object and follows
this exact order:

1. call the object-data virtual and test the active result (`BuildData::flags & 0x04` for a
   concrete Build);
2. when active, resolve it again and test the object CITY bit `flags & 0x20`;
3. when both are set, resolve it a third time and read signed `BuildData::city +0x72`;
4. when that index is nonnegative, read `CityData::city_flags +0x04 & 1` from the owner-local
   City row;
5. when no nonnegative City row was selected, read the requested
   `BuildTypeData::build_flags +0x2C0 & 0x10` instead.

An inactive selected City returns failure directly; it does not fall through to the target-Type
bit. The receipt records which source retail selected so this distinction is testable.

If the selected bit is zero, `0x006E1507` jumps to the common failure epilogue at `0x006E3076`
and returns native one. The BHS result is therefore zero. If nonzero, the exact next instruction
is `0x006E150D`, leaving 7,137 bytes.

The target-Type bit `0x10` is recorded neutrally as `BUILD_FLAG_NO_ACTIVE_CITY_REQUIRED`: it
allows placement search to proceed when the origin is not an active City-linked Build. The
installed Farm has `build_flags = 0x10000049`, so it does not take that fallback and depends on
its active Athens center.

## Canonical owners and receipt

The executable adapter reads:

- object-band identity from `Sim::world.objects`;
- active/CITY flags, object identity, and City index from `Sim::builds`;
- City activity and center identity from `Sim::cities`; and
- target class and `build_flags` from the canonical `LiveProductionRuntime` Type projection.

Out-of-range rows and broken cross-owner/center identities fail closed as invariant errors. They
are not converted into a retail terminal result because the native body directly indexes those
structures and assumes the caller-established object invariants.

`LeaderProduceBuildingPrefixReceipt` contains the native and scenario return values on the
terminal arm. The admitted arm contains neither scalar and instead carries the exact continuation
`{ va: 0x006E150D, bytes_remaining: 7137, owner, type, origin, mode }`.

## Replay integration and evidence

The same persistent replay-selected `ScriptRuntime` now executes this prefix immediately after
the owned builtin-520/521 cost prefix.

- A synthetic real-BHS call with an active but non-CITY origin and Farm target returns scenario
  zero normally. The receipt records native one, and Build, Group, and Leader resources do not
  change.
- Direct tests cover both admission routes: the Type `0x10` fallback and an active City-linked
  origin. They also prove the terminal receipt is identical after `save_sim` / `load_sim` plus
  reinstalling the existing external activation/resource projections.
- The replay-selected installed `economic.bhs` Farm/Athens call uses an active Athens center,
  crosses the gate, and remains stopped at builtin 520. The stop is now the placement-search
  instruction `0x006E150D`, not the function entry. Program refs, timers, research queues,
  Scenario cursor, Groups, Build image, resources, and all Leader mirrors still roll back.

The remaining placement search reads map/Terrain/Type/Leader policy, consumes RNG, and has paths
that clear terrain bits before it reaches `Objects::init_build`. It is deliberately not replaced
by the package-driven Group Build runtime or a scalar site-selection authority.

Focused gate:

```text
CARGO_TARGET_DIR=/Users/ember/.cache/don-bhs520c8-target \
  cargo test -p don-replay --test replay_bhs_research_runtime -- --nocapture
```
