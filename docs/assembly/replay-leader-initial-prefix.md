# Replay-owned initial Leader prefix

Lane: `replay-initial-leader-prefix`. All addresses below are instruction-derived from the
supported PE32 and cross-checked against the PDB layout. This is a sparse ownership result,
not a completed checksum channel.

## Result

`crates/don-replay/src/leader_initial_prefix.rs` derives exact setup-time bytes for each of
the eight Leader rows from the replay's `Player[8]`, settings, frame and semaphore. It owns

```text
8 + 704 * active_leaders
```

checksum-visible bytes. An inactive row owns one byte; an active row owns 705. Unknown
bytes in the module's fixed-size storage are explicitly not claims. A future complete
Leader owner must merge only `InitialLeaderRow::owned_spans()`/`owned_slice()` into its
record image.

The producer is frozen at setup. `is_human_only_first_checksum_candidate()` is an
expiration classifier, not an agreement claim. A recorded Leader checksum is one Adler-32
over the complete eight-row state, so the corpus cannot validate a partial byte image by
comparing it with that final word.

## Why Leaders, and why only a prefix

Among Units, Builds, Leaders, Cities and Guys, Leaders have the largest exact prefix which
does not first require generated map objects:

| Candidate | first missing owner |
|---|---|
| Units | starting types/positions and the retail sparse `Objects::find_free` allocation result |
| Builds | concrete starting-town Build objects, owner-local indices and generated coordinates |
| Cities | the starting Build identity, region and the remaining `City::init`/`City::fill` state |
| Guys | Units plus squad/crew composition and graphics-track offsets |
| Leaders | full economy/tech/object-dependent state is missing, but replay Player setup and `Leader::init` own an independent prefix |

The source scan also found `objects_init_unit_authority_frontier.rs` and
`bhs_create_unit_allocation_tail_frontier.rs` present but unregistered. Both files say why
they do not close initial Units: they are source-only receipt/authority boundaries and do
not own the canonical sparse allocator or execute the full `Unit::init` product adapter.
The recovered `leader_init_diplomacy_loop.rs`, by contrast, is registered and supplies the
exact instruction-ordered reset body used here. Other unregistered Leader files found by
the scan (`leader_match_host.rs` and `leaders_end_process_step17.rs`) are runtime/lifecycle
bodies, not initial-state producers.

DoNSave v10 does not fill this gap. It is the project's deterministic internal save format,
not a retail `.svx` or `.rcx` snapshot. Version 10 lacks the later `LEADER_MATCH` chunk and
can reconstruct only the representable frame-zero setup shape; it cannot be used as
evidence for replay Leader bytes which retail did not serialize.

## Shipped setup schedule

The relevant order is:

1. `Leaders::Leaders` `0x006ED900` constructs ten slots.
2. `Leaders::init` `0x006ED850` calls `Leader::init(slot, -1, slot)` for all ten. The
   negative-tribe body zeroes `0x6929` bytes, installs identity, and executes the eight-row
   diplomacy reset.
3. In the replay/network arm (`Game+0x820 & 0x04`) of
   `Game::init_rules_and_teams` `0x00589BB0`, the eight Player rows activate the Leader
   selected by `Player::who`. The first row for a `who` supplies tribe and HUMAN; a later
   non-human duplicate clears HUMAN without replacing tribe.
4. `Setup::build_game` `0x005AC190` invokes active `Leader::init(who, tribe, who)` in an
   RNG-derived order.
5. `LeaderData::walk_data` `0x006D6750` always walks header `[0,8)` and, when flag bit zero
   is set, fixed body `[8,0x692A)` followed by dynamic children.

`Setup::build_game`'s order matters for `ally_mask`: the mutual `is_ally` query can observe
rows initialized earlier in the shuffle. Ranked/random team resolution is also not fully
replay-owned. The producer executes the recovered body but publishes only bytes invariant
under active-row order, shared-vision prerequisite, and team assignment. Tests run forward
and reverse orders, flip all shared-vision prerequisite results, and materially change
team style/team bytes/rush rules; every published byte remains identical.

## Byte ownership

Offsets are relative to one `LeaderData` record.

| Row | interval | bytes | source |
|---|---:|---:|---|
| every row | `[0,1)` | 1 | replay Player activation scan; exact low `leader_flags` byte |
| active | `[8,24)` | 16 | `who`, `tribe`, `defeated_by=-1`, `gov=-1` in `Leader::init` |
| active | own `diplos[who]` cell in `[116,148)` | 4 | always Ally, including scenario-preserve path |
| active | own `treaties[who]` cell in `[148,180)` | 4 | always bit zero set by `is_team(who, who, 0)` |
| active | `[180,500)` | 320 | unconditional agenda/deed/stamp/hire reset stores |
| active | own `aggression[who]` cell in `[528,560)` | 4 | self relation is never War |
| active | `[560,916)` | 356 | unconditional strong/weak/DOW/invader/message/taunt reset stores |

The active-row total is `1 + 16 + 4 + 4 + 320 + 4 + 356 = 705` bytes.

The exclusions are load-bearing:

- header `[1,8)` can acquire NEW_UNITS/NEW_TECH/runtime flags and contains all of
  `leader_flags2`;
- `[24,116)` contains score, cannon/handicap and chat state;
- non-self `diplos`, non-self `treaties`, and non-self `aggression` depend on team/rush/
  scenario setup;
- `[500,528)` is outside the recovered diplomacy loop;
- `[916,0x692A)` contains the remaining economy, technology, AI, production and object
  state, including order-dependent `ally_mask` at `+0x6929`;
- every dynamic child after the fixed body remains unowned.

The `0x02000000` store performed by `Leader::init` is intentionally not published as byte
three of `leader_flags`: the same byte also holds NEW_TECH, whose setup/runtime lifecycle is
not owned here. Bit-level knowledge cannot be handed to an Adler visitor as a complete byte.

## Replay evidence and corpus boundary

For every row the receipt retains the exact two-byte Player flag gate and optional `0x39`
Player body span exported by `InitialState`, plus the complete decompressed-payload SHA-256.
The parser does not yet export a local semaphore span, so the receipt retains the observed
semaphore bytes and binds them through the whole-payload hash instead of inventing an
offset. The corpus test rereads each span from the decompressed payload and checks flags,
tribe, `who`, and team. A mutation of the owned tribe byte reparses and changes exactly the
claimed identity field.

The local 61-recording corpus contains 21 checksum-bearing recordings. Seven have no
non-human active Leader and all seven still carry the independently derived `Game::init`
Groups image on their first checksummed turn; the other fourteen already have AI-mutated
Groups state. The module records the same 7/14 split as a conservative Leader-prefix
expiration boundary:

- human-only: candidate for later first-turn installation after command/lifecycle audit;
- any non-human active Leader: frozen setup prefix is expired before the first checksum.

This is corpus evidence about the deadline, not a tuned Leader checksum. No recorded Leader
word is an input to the producer and no field was changed to improve an Adler residual.

## Integration boundary

The exclusive module API is:

```rust
leader_initial_prefix::derive(&InitialState)
    -> Result<InitialLeaderPrefix, InitialLeaderPrefixError>
```

`InitialLeaderPrefix` exports the eight rows, active/human/non-human masks, replay source
evidence and exact byte count. Each row exports only the ownership ledger and corresponding
slices. Registration requires one `pub mod leader_initial_prefix;` in
`crates/don-replay/src/lib.rs`. Harness/state integration must remain a later completeness-
aware merge; installing this sparse image as the complete Leaders channel would be wrong.
