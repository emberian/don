# Scenario checksum channel

Status: **compiled primitive, not integrated**. Fidelity: **Tier C** for traversal
structure; no claim is made yet that a reconstructed simulation supplies retail-identical
scenario state.

## Result

`crates/don-replay/src/scenario_channel.rs` implements the checksum branch of
`ScenarioData::walk_data` at `0x00997AD0`. It accepts a complete typed snapshot, validates
all fixed and dynamic boundaries before hashing, and returns the Adler-32 value and exact
byte count. It does not execute BHS, simulate scenario behavior, follow process pointers,
or fill absent data with guessed defaults.

The 21 checksum-bearing replay files currently all begin with
`scenario_data = 0x09922B90` [measured by opening the corpus with `don_replay::Replay`]. A
full scan found 2,458 distinct values among 488,557 checksum packets, so the channel is
demonstrably mutable. `0x09922B90` is exposed as an initial-state target, never as a
constant result.

## Traversal provenance

The root name/signature/size come from the matching shipped PDB:

- `ScenarioData::walk_data(DataWalk*)`, `0x00997AD0`, 1,176 bytes.
- Caller: `CheckSums::check_all`, `0x00936560`; it resets the per-channel `CheckSum` to
  Adler initial state 1 immediately before this call.
- `schema/state-schema.json` and instruction disassembly recover the program order. The
  PDB supplies the identities and dimensions of the static globals.

The checksum walk is, in order:

1. eight direct `i32` globals;
2. eight interleaved triples of `camera_init_x`, `camera_init_y`, and
   `camera_init_zoom`;
3. `custom_time_limit`, 31 find counters, eight `last_razed`, eight `city_lost_to`;
4. `short units_killed[352][8]`, `short builds_destroyed[129][8]`, eight
   reinforcement bytes, `war_blocked[8][8]`, eight ally bytes, eight diplomacy bytes;
5. fourteen named scenario flag bytes in instruction order;
6. six `String` walks and three ten-byte `Color` images;
7. timers, components, messages, extra starting locations, and the city-lost bit mask;
8. eight objective lists, named scenario groups, involved objects;
9. eight reveal-point arrays, eight attrition-free-point arrays, eight ignored-object
   arrays, then `ignore_orders`.

`walk_test` tags produce zero bytes under `CheckSum`. At `0x00997E4C`, the function tests
`DataWalk+0x08`; `CheckSum` takes the branch that skips `ScenarioData::user_warnings`.
The module therefore does not expose that save-only input.

## Container rules

The implementation retains retail storage metadata because it is checksum-visible.
Non-empty `Array`, `ObjectArray`, `NamedObjectArray`, and `SimpleArray` values walk:

```text
i32 length, i32 capacity, i16 increment, u8 (flags & 0xbf), elements...
```

An empty array walks only its zero length. Linked lists walk their `i32` count and their
nodes in list order. `String::walk_data` emits a zero-extended four-byte character count
followed by exactly that many UTF-16LE units. The city-lost `BitMask` walks `bits`, `size`,
then exactly `size` payload bytes; its flags are not part of this path.

Nested object order also matters:

- a scenario objective walks `completed`, `print`, **sound string before id string**, then
  its base scenario-message text and ten-byte color;
- a named group walks `who`, `find_counter`, its full `SimpleArray<int>`, then its name;
- an involved-object list node walks its one-byte key before its four-byte object id.

## Completeness boundary

Before consuming any checksum byte, `scenario_checksum` rejects:

- `units_killed` or `builds_destroyed` with anything other than their exact PDB sizes;
- a non-empty array with negative capacity or capacity below logical length;
- any list/array count that does not fit retail's signed 32-bit count;
- a string longer than retail's 16-bit character-count field;
- a bit-mask payload whose byte length differs from its walked `size`.

The types pair elements with their names, ids, or keys, so those counts cannot silently
diverge. Object ids for objective pointers remain caller inputs: retail obtains them by a
virtual call, and this primitive does not invent pointee identity.

## Evidence and open gaps

- [measured] Disassembly of `0x00997AD0` provides every root call and direct byte range.
- [measured] The exact-build PDB supplies global names, element counts, and nested layouts.
- [measured] Standalone tests cover direct-field order, little-endian integers and UTF-16,
  array metadata order/masking, mutation sensitivity, and rejection of truncated inputs.
- [reported] The shared project Adler primitive has Tier-B differential coverage against
  retail; this module locally mirrors that algorithm so it remains standalone.

Still missing before integration:

- a complete snapshot extractor from replay/save initialization or from a read-only live
  process;
- a fixture that supplies every checksum-visible field and reaches `0x09922B90`;
- integration in `don-replay/src/lib.rs`, `SimState`, and the replay harness (outside this
  lane's file ownership);
- differential execution of the whole `ScenarioData::walk_data` visitor. Until that exists,
  the traversal stays Tier C.

No live validation was attempted in this lane. Reconstructing the pointer-rich list state
would require exporting substantially more runtime scenario memory than a narrow scalar
read, while the current task did not need those raw process contents.
