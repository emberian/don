# BHS builtin 357: single-Library research runtime

This tranche admits the installed `economic.bhs` call
`research_tech_with_cost(1, "Written Word")` through its single completed-Library path. It
does not claim the whole builtin, a ScenarioData checksum closure, or a generic production
allocator.

## Retail trace

The supported PE/PDB image places
`ScenarioFuncSet::research_tech_with_cost(int const&, String const&)` at `0x009ee700`
(422 bytes). The reached native calls are:

1. normalize negative `ScenarioData::find_counters[30]` (`0x00cc2288`) to object id 2000;
2. resolve the first case-insensitive internal type name and require both low Leader flags;
3. return 1 immediately when `LeaderData::has_tech` is already true;
4. resolve `LeaderData::current_upgrade(TypeData::where)`;
5. circularly call `find_build` (`0x009e2970`) after the retained cursor, selecting a valid,
   completed Building whose type is the current producer;
6. call `BuildData::can_queue` (`0x004d2030`);
7. write `find_counters[30] = object_id`;
8. clear and add the Building to a temporary singleton Group, then call
   `Groups::push_group(..., singular=1)` (`0x0070f9e0`);
9. call `Group::action_queue_up(type, 1)` (`0x006fdbb0`), which reaches
   `Build::queue_up` (`0x00620f40`), and return the Building object id.

The native handler does not inspect `Build::queue_up`'s return, but that does not create a
payment-refusal-after-Group arm. `BuildData::can_queue` first calls the exact same
`TypeData::can_pay_cost` vtable slot (`TechTypeData::vftable +0x84 = 0x00667570`) and
then `BuildData::could_queue`. It returns before the cursor write and temporary Group when
either check fails. With no intervening gameplay mutation, insufficient resources therefore
return 0 with the retained cursor and every Group/queue/resource/counter field unchanged.

On a successful payment, the exact `Build::queue_up` tail increments `ages_queued` only
for the age band `0x220..0x227` (`0x00621A8A..0x00621ABF`) and increments
`epochs_queued` only for `0x227..0x243` (`0x00621AC7..0x00621AFC`). Written Word and
City State are types 551/565 in the latter band, so this cohort increments
`epochs_queued` and leaves `ages_queued` unchanged.

Payment still consumes all six goods. The concrete queue record has room for only three
cost pairs: `BuildQueue::set_queue` `0x006309F0` scans goods 0 through 5 and returns after
writing the third non-zero pair. The transaction and receipt therefore retain the first
three pairs without refusing a valid cost that uses four or more goods.

`Groups::copy_group` preserves destination identity, army, formation, and all other unlisted
fields. Only `who`, `num`, `ox/oy`, `o_dist/o_angle`, `buildings`, `speed`, `stamp`, and the
live prefixes of `list`, `angles`, `off_x/off_y`, and `curr_x/curr_y` are copied. The following
`Group::action_begin` clears `disband` on the admitted successful queue path.

## Installed facts and measured continuation

The installed `ron-data/techrules.xml` row for Written Word is type 551, age 0, Library
producer 435, job time 200, no prerequisites, and exact six-good cost
`[0, 12, 5, 0, 0, 0]`. Library has no jump/upgrade, so the current producer remains 435.
City State is type 565 on the same producer with cost `[12, 0, 0, 0, 0, 0]`.

The installed `economic.bhs` Romans/Mediterranean success trace is asserted, not inferred:
the positive Written Word 357 call is immediately followed by builtin 258 `num_cities`,
builtin 362 `have_tech("City State")`, and a second builtin 357 for City State. The replay
test compiles the shipped BHS file and loads the installed Mediterranean map-style owner.

The measured Written Word miss arm is builtin 332
`at_least_type(who, 75, "Wealth")`. The exclusive wrapper implements only the six-primary-good
form over reconciled production/Sim/step-8/victory resource mirrors.

## Atomic boundary

`run_production_research_call` runs the Program, ref arguments, and timer container through
`ScriptRuntime::run_external_timer_transaction`. It also snapshots the exact Sim/production
owners changed by builtin 357: ScenarioData scalars, Groups, Build queues,
`LiveProductionRuntime`, Sim and step-8 stockpiles, and victory Leader resources/counters.
Any VM or validation failure restores every one of those owners before returning the error.

The rollback test removes timer `"1"`, queues Written Word, mutates the retained cursor and
Group, then deliberately reaches unsupported builtin 94. It proves that Program statics,
the ref step, timer plus cursor, ScenarioData cursor, Group pool, Build queue, resources,
`num_queued`, `ages_queued`, and `epochs_queued` all return to their entry images.

## Deliberately red boundaries

- `crates/don-replay/src/scenario_channel.rs` currently carries
  `ScenarioDirect::find_counters` by value and constructs the frozen initial `[-1; 31]` image.
  It was already concurrently modified and is not changed here. The required narrow follow-up
  is for the live ScenarioData walk to borrow `&Sim::scenario_data.find_counters` (the exact
  `[i32; 31]` mutated by builtin 357), with a test showing that a successful 357 changes that
  borrowed walk. Copying the array into another retained channel owner is not an acceptable
  join. Until then there is no ScenarioData checksum claim.
- The uncommon full transient-Group pool fallback needs the all-object-band captain/LRU
  authority. This tranche admits the immediate empty/building slot path reached by the
  installed trace and fails closed before mutation at that boundary.
- Generic type `Object::is` composition is not claimed. The admitted installed path requires
  the live Build's exact registered producer type to be the resolved current Library.
- No retail process memory is written and no scalar placeholder is used as production state.

## Gates

- `cargo test -p don-sim --test bhs_research_queue_runtime`
- `cargo test -p don-replay --test replay_bhs_research_runtime`
