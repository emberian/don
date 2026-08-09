# Retail team/setup mutation seam

## Scope and provenance

This lane closes the smallest deterministic mutation seam beyond the read-only
`setup_diplomacy` queries.  It is Tier C, instruction-derived from the supported PE32
`ron-bin/riseofnations.exe` (SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`) and its matching
`ron-bin/sbl/rise.pdb`:

- `Game::init_teams` is `0x0058AE70..0x0058C19C`, 4,909 bytes, `game.cpp:5147..5601`.
- PDB `Game` fixes `on_team[8]` at `+0x5E0`, `num_teams` at `+0x6A4`, `num_sides` at
  `+0x6A8`, and `BitMask<256> semaphore` at `+0x814`.  Its `flags` dword is `Game+0x81C`
  and its first storage byte is `Game+0x820`.
- PDB `Player` is 140 bytes.  In the global setup image used by this function, the measured
  stride is `0x8C`, presence is `flags & 1`, `who` is at relative `+0x77`, and signed `team`
  is at `+0x78`.
- PDB `LeaderData` is 28,388 bytes (`0x6EEC`).  `leader_flags`, `who`, and
  `chat_status[8]` are at `+0x00`, `+0x08`, and `+0x54` respectively.
- `Game::is_cooperative` is `0x00594810..0x00594874`, 101 bytes.

No retail function was executed.  The Ghidra output was read only as a navigation aid; the
addresses, branch order, signed comparisons, field stores, and callback arguments below were
checked against GNU `objdump -d -Mintel` over the shipped executable.

The deterministic core is necessarily noncontiguous: pure team aggregation precedes omitted
random/ranked bodies, while `0x0058BD78..0x0058C184` is the smallest contiguous final suffix.
Calling `Game::init_rules_and_teams` (`0x00589BB0`) would be materially broader because its
adjacent map, random-color, starting-resource, handicap, and tribe paths consume other state and
RNG.

## Admitted path

`team_setup_mutation.rs` admits only the path which is deterministic without product RNG or
ranked-account state:

1. At `0x0058AEAD`, retail zeroes all eight `Game::on_team` cells.
2. At `0x0058AEE0`, it publishes every present setup player's current signed team byte through
   callback-table offset `+0xD8E0`.
3. At `0x0058AFC2`, it walks the eight live leaders.  `LeaderData::get_player`'s exact
   immediate/deferred lookup semantics select a setup player.  Team style 3 with semaphore bit
   `0x04` clear overwrites that player's team at `0x0058B020..0x0058B047`: the local setup slot
   becomes team 0 and every other resolved slot becomes team 1.
4. Signed team bytes `0..=3` increment `on_team`; byte 5 enters a later random assignment body;
   every other nonnegative byte counts as an independent non-team side.  A negative byte would
   index before `Game+0x5E0` in retail, so the product seam rejects it.
5. `0x0058B1AE..0x0058B1F9` counts the nonempty first four team buckets into `num_teams`.
   `0x0058BD78..0x0058BD81` writes `num_sides = num_teams + non_team_players`.
6. `0x0058BDA0` republishes every present setup player's final team through `+0xD8E0`.
7. `0x0058BE31..0x0058BFBC` clears semaphore bit `0x80`, seeds a zero semaphore-flags dword to
   2, then counts each live leader's runtime teammates.  The first count greater than one sets
   bit `0x80`, clears the flags dword, emits `+0xD944`, and breaks the scan.
8. `0x0058C020..0x0058C184` visits only live leader rows.  Every one of their eight
   `chat_status` cells is first set to 1.  A valid target is then cleared to 0 when the game is
   ordinary noncooperative free-for-all, or when `LeaderData::is_team(target, 0)` succeeds.
   Inactive leader rows are untouched; invalid target columns retain the seeded value 1.

`Game::is_cooperative` returns true unconditionally for team styles 8, 9, and 10.  For styles
11 and 12, it returns true after encountering a repeated present setup `who`; all other styles
return false.  The Rust seam computes that fact from the canonical setup image.

This is victory **aggregation**, not victory-mode initialization: `GameInfo::victory` at
`Game+0x38` is an input and `LeaderData::victory_type` at `+0x7D8` is not written here.
Likewise this is diplomacy-related **chat-gate** initialization, not diplomacy initialization:
`LeaderData::diplos[8]` at `+0x74` and `LeaderData::dip[8]` are untouched.

## Atomicity and fail-closed gates

Planning clones and fully validates the input.  Application compares the whole expected state
before replacing it, so an absent setup record, malformed identity, query error, or concurrent
change leaves the state untouched.  In particular, this seam does **not** inherit retail's
dangerous “missing match means player zero” fallback: every live leader must resolve to a
present `PlayerSetup` whose `who` agrees with the leader.

The following paths remain explicitly red:

- signed team byte 5 unless team-style-3 assignment overwrites it first: the omitted body uses
  `random(0, 0xffff)`, ranked ELO sorting, and correlated team rewrites;
- ranked games: `0x0058B9xx..0x0058BD36` reads ELO/account facts and writes ranking scale fields;
- nonzero setup frame or malformed leader-slot identity;
- execution of the ordered script callback plan.  The receipt records exact callback-table
  offsets and arguments, but does not claim that a product BHS host ran them transactionally;
- later `Leader::set_diplo`, retarget/vision/event work, and checksum/save integration.

Closure delta is therefore **one executable deterministic setup mutation seam**, not the whole
4,909-byte function and not multiplayer-lobby completion.

## Validation

The focused test file covers configured teams, independent sides, cooperative chat behavior,
the deterministic team-style-3 rewrite, absent/fallback setup records, negative and random
team red gates, ranked rejection, stale-plan rejection, inactive-row preservation, and ordered
callback planning. Root convergence formatted the isolated Rust files and validated all ten tests
in both modes:

- hbox debug: `setup-team-mutation-20260809T225353Z-91621-15781-cb8b2b915d8e`;
- persvati release: `setup-team-mutation-release-20260809T225354Z-91628-25804-cb8b2b915d8e`.

Neither job ran retail.
