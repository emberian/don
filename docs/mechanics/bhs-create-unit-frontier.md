# BHS unit-creation frontier

Status: exact wrapper and shared-receiver prefix reversal; deliberately **not runtime-wired**.
The isolated source is `crates/don-sim/src/systems/bhs_create_unit_frontier.rs`, with a
path-import proof pack at `crates/don-sim/tests/bhs_create_unit_frontier.rs`.  These are new-only
files and do not modify the type-stat, session, or channel-13 integration surfaces.

## Why this cohort

The largest coherent untouched BHS cohort sharing one receiver is the unit-creation triad:

| global registration | handler | bytes | shipped calls | files |
|---:|---:|---:|---:|---:|
| 508 `create_unit` | `0x009F4C50` | 37 | 548 | 91 |
| 509 `create_unit_upgrade` | `0x00A030E0` | 116 | 3,046 | 182 |
| 510 `create_unit_in_group` | `0x009F4C80` | 165 | 1,678 | 109 |
| | | | **5,272** | **232 unique** |

The count is the comment/string-stripped census over 363 shipped files, 93,649 lines, and
39,957 registered-builtin call sites.  Pairwise file overlaps are 60 (508/509), 36 (508/510),
and 86 (509/510), with 32 files containing all three.  The union is therefore 232 files.
All three wrappers call `ScenarioFuncSet::add_unit` `0x009E2220` (1,428 bytes).

This is 13.194 percentage points of lexical reachability.  If a future integration closes every
owner below, the last documented 9,444/39,957 strict static baseline would become
14,716/39,957 = 36.83%.  This proof pack itself changes executed coverage by **zero** and does
not register a builtin.  The newer type-stat work's 194 lexically reachable calls are a separate
baseline question; they are not folded into either number here.

Ground truth is `ron-bin/riseofnations.exe`, 9,925,120 bytes, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`, plus its matching
`ron-bin/sbl/rise.pdb`, `schema/bhs-builtins.json`, `schema/rise-procs.tsv`,
`schema/types.json`, and shipped `schema/live/live-tables-typeids.tsv`.

## Wrapper differences

Registration 508 is a 37-byte forwarding wrapper.  It passes the written type name unchanged
and passes zero as `add_unit`'s sixth argument.  Registration 509 first resolves the written
type, checks one-based `who` with an unsigned `0..7` slot test, requires only Leader flag bit 0,
calls `LeaderData::current_upgrade` `0x006E3140`, substitutes the canonical name of that result,
and passes sixth-argument zero.  Registration 510 performs the same upgrade resolution but
passes one.

The sixth argument is not an upgrade flag.  Zero calls `clear_group(who-1)`; one skips that
call.  Every successful allocation later calls `add_to_group(who, object_id)`, and the ordinary
`group_*_order(who, ...)` wrappers also derive their default group name from the unmodified
`who`.  The one-off difference in numeric group keys is literal native behavior, not a typo in
this proof.  It may be a retail off-by-one quirk; assigning a stronger intent would require a
live group capture.  The shipped scripts commonly use 509 followed by repeated 510 calls before
issuing `group_*_order(who, ...)`.

Two sequencing traps matter:

1. Upgrade wrappers resolve the requested type name **before** validating `who` and gate only on
   Leader bit 0.  The shared receiver then repeats the player check and requires both low bits.
2. The zero-flag `clear_group(who-1)` occurs after type/world/graft prefix validation but
   **before** Transport Barge and domain/ocean rejection.  A failed call can therefore erase a
   group even though it creates nothing.

## Shared `add_unit` prefix

The exact prefix at `0x009E2220` is:

1. derive `leader_slot = who - 1`; reject unsigned values above 7;
2. require `(LeaderData::leader_flags & 3) == 3`;
3. reject `(unsigned)num_units > 2000`, which rejects every negative count but admits zero;
4. call `ScenarioFuncSet::get_type_index(name,0)` `0x00A03480` and require nonnegative;
5. call `TypeData::is_unit_type` through vtable `+0x0C` and require true;
6. derive world coordinates with arithmetic `x >> 2`, `y >> 2`, and placement coordinates with
   wrapping `x*192+96`, `y*192+96`;
7. require `WorldData::is_valid` `0x00461420`;
8. call `LeaderData::get_graft(type)` `0x0047DF40` and retain that type for allocation;
9. construct the local `Group`; zero-flag registrations call `clear_group(who-1)`;
10. reject `TypeData::is(TRANSPORT_BARGE=0x140,0)`;
11. classify by `ObjectTypeData::domain +0x218` and `WorldData::is_ocean`:
    Ground/non-ocean is direct, Sea/ocean is direct, Air is direct regardless of ocean,
    Ground/ocean requires `LeaderData::can_transport`, and every other pair rejects.

The original/effective type supplies domain and Unit flags.  `get_graft` supplies the type passed
to `Objects::init_unit`; combining those identities loses mod and nation-specific behavior.
For Air, clear bit `UnitTypeData::unit_flags +0x2B4 & 0x20` selects the post-create clear-orders,
strafe-order, and terrain-height timer path.

The PDB procedure extents and direct calls pin the remaining mutation graph:

| procedure | VA | bytes |
|---|---:|---:|
| `ScenarioFuncSet::clear_group(int)` | `0x004CE470` | 128 |
| `ScenarioFuncSet::add_to_group(int,int)` | `0x004CE5F0` | 101 |
| `ScenarioFuncSet::add_to_group(String,int,int)` | `0x009FA120` | 378 |
| `UnitType::find_nearby_spot` | `0x0061DE70` | 1,433 |
| `Objects::init_unit(int,TypeIndex,...)` | `0x0065E0C0` | 1,603 |
| `Group::add` | `0x00714350` | 616 |
| `Group::get_num` | `0x00714700` | 320 |
| `Groups::push_group` | `0x0070F9E0` | 448 |
| `Group::action_transport` | `0x00702620` | 932 |
| `Group::action_form` | `0x00707220` | 746 |
| `Group::dbg_jump_to_action` | `0x00707A00` | 160 |
| `Unit::clear_orders` | `0x005E3860` | 42 |
| `Unit::add_strafe_order` | `0x005E48C0` | 405 |
| `TerrainOut::find_tcoord_z` | `0x00866710` | 47 |
| `Unit::action_unqueue` | `0x005E1F20` | 389 |
| `ObjectData::num_aircraft_limit` | `0x006454A0` | 169 |
| child `Objects::init_unit(int,int,...)` | `0x00461310` | 39 |
| `Unit::go_inside` | `0x0061A2E0` | 722 |

## Mutation path and ownership needs

The receiver is not a single object-table insert.  Closing it requires one transaction host that
can preserve retail's partial effects and call order across:

- canonical 806-row type-name lookup, `is` relations, current-upgrade and graft projections;
- the two-bit Leader gate and `can_transport`;
- exact `WorldData::is_valid`, `is_ocean`, and `UnitType::find_nearby_spot` placement;
- `Objects::init_unit` `0x0065E0C0`, including allocator/object-id state, player object bands,
  counters, checksum-visible UnitData, and every init side effect;
- persistent scenario numeric group names (`who-1` clear versus `who` append/order), local
  `Group::add`, `Groups::push_group`, `Group::action_form`, and `Group::dbg_jump_to_action`;
- Ground-on-ocean `Group::action_transport(1)`, captain canonicalization through
  `UnitData::o_up +0x82`, and group append order;
- the Air clear-orders/strafe/timer suffix;
- the Aircraft Carrier `is(0x15F,0)` payload: `Unit::action_unqueue(1)`, current Helicopter
  upgrade from base `0x134`, `ObjectData::num_aircraft_limit`, child `init_unit`, and
  `Unit::go_inside`.

The carrier unqueue receiver is already recovered separately; that does not by itself own
carrier child allocation or the surrounding add-unit sequence.  The existing group and
production systems have many leaves, but no current adapter joins all of the state above into
one BHS-visible transaction.

## Non-atomic return semantics: a corrected pitfall

`add_unit` initializes its return local to `-1` and overwrites it with every
`Objects::init_unit` result.  Each successful id is immediately added to the local and persistent
groups; a failed allocation is skipped and the loop continues.  There is no rollback.  The final
return is the **last allocation attempt's object id or -1**, so `[success 41, failure -1]` returns
`-1` while object 41 remains, and `[failure -1, success 42]` returns 42.

This directly contradicts the older `docs/tooling/bhs-bridge.md` statement that `create_unit`
returns only a status and that callers must recover the object with `find_unit`.  The native data
flow (`Objects::init_unit` result -> Group::add object argument -> function return local) and
shipped assignments such as `unit_id = create_unit(...)` establish object-id semantics.  The old
note should be corrected only when a shared-doc owner is available.

No build, formatter, retail process, VM, stage, commit, or push was used in this archaeology
lane.  The new proof pack remains source-only until root convergence validates it on an
independent host.
