# ScenarioFuncSet::num_type_queued (builtin 436)

## Retail identity

- Registration: `num_type_queued(int who, int build_o, String type) -> int`
- Handler: `0x009F25E0`
- Size: 499 bytes (`0x009F25E0..0x009F27D3`)
- Build validator: `ScenarioFuncSet::valid_build_o` at `0x009E3300`
- Type resolver: `ScenarioFuncSet::get_type_index` at `0x00A03480`

The handler subtracts one from `who` and calls `valid_build_o(who - 1, build_o)`. That helper
requires an unsigned Leader slot below eight, both low `LeaderData::leader_flags` bits, an object
index in `2000..=2999`, a non-null object slot, and `BuildData::flags & 1`. Failure returns `-1`.
The handler does not require the Build to be completed or active.

## Exact queue census

The queue owner is the selected Build itself:

- `BuildData::queued` at `+0x82` is the logical `u8` slot count;
- `BuildQueue::num` at `+0x88` is the allocated entry count;
- `BuildQueue::data` at `+0x8C` points at 20-byte entries; and
- each entry's signed `TypeIndex` is at `entry +0x04`.

Retail loops over every logical slot below `queued`. The normal accessor returns `-1` when the
slot is beyond `BuildQueue::num`.

There are two query modes. An empty String, or any non-empty String whose first UTF-16 character
is a space (`U+0020`), is a wildcard: the handler counts every slot whose accessor result is not
`-1`. This is a first-character test, not an equality test for the one-character string `" "`.

Every other query is resolved against the internal names of all 806 Type rows. A missing name
returns `-1`. For each logical queue slot the handler calls the queued row's
`TypeData::is(requested_type, 0)` virtual and counts a nonzero result. The second argument zero
makes this the canonical non-strict ancestry relation. It therefore counts a derived queued Unit
when the requested Type is one of its ancestors.

The installed adapter preserves the wildcard `-1` sentinel exactly. A named query encountering a
missing or out-of-range queued Type fails closed: such a queue violates the reachable retail
invariant and cannot safely index the canonical Type table.

## Canonical owners and transaction boundary

- Build object mapping: `Sim::world.objects`, Build band
- Build identity and queue: `Sim::builds`
- Type names and non-strict ancestry: `TypeBuiltinState::types`
- Leader activation mirrors: Type owner, victory owner, and step-8 owner

The retail globals anchoring those reads are `GameAccess::objects` at `0x00C0618C` and the Type
pointer table at `0x00E85DDC`. The implementation does not use `LeaderData::num_queued` or the
production runtime's aggregate `queued_counts`: those are separate derived owners and the retail
handler never reads them.

Builtin 436 is mounted into the same replay-selected persistent `ScriptRuntime` as builtins 357,
386, and 455. The census itself is read-only. A later VM failure still restores Program refs,
timers, ScenarioData, Groups, the full Build queue, production state, and all Leader resource
mirrors. No scalar queue facade or second Build owner is introduced.

## Save and installed-content evidence

Focused tests cover direct and derived Unit Types, both wildcard forms, invalid Leader/Build/Type
returns, logical-versus-allocated queue lengths, fail-closed named corruption, read-only behavior,
and queue preservation across `save_sim` / `load_sim`.

The installed replay `Playback___2020.07.25_19_30_12__Sat_.rcx` selects shipped `economic.bhs`.
After the Written Word / City State research cohort, builtin 455, and three builtin-386 City
counts, the script calls `num_type_queued(3, 2000, "Citizens")`. The canonical Library queue
contains the two research items rather than Citizen production, so builtin 436 returns zero.
Execution then reaches the actual next unowned builtin, `place_building_with_cost` (registration
520).
