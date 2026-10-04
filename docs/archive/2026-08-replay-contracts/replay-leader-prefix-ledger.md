# Replay Leader-prefix ownership ledger

The setup-time Leader producer now has a separate byte-span accounting layer in
`crates/don-replay/src/leader_prefix_ledger.rs`. This is deliberately not a Leaders-channel
producer. It makes the exact prefix measurable without allowing the partial image to enter the
substantive checksum scoreboard.

## Ownership result

For each replay, `LeaderPrefixSpanLedger::derive` validates every span published by
`leader_initial_prefix` against the row roster, the active/human masks, the retail fixed-walk
boundary, sorted non-overlap, the row's private `owned_slice`, and the expected `705/1` active/
inactive byte totals. Every admitted span is content-addressed with SHA-256 and retains its exact
setup provenance.

The per-file total remains:

```text
8 + 704 * active_leaders
```

The 61-file report baseline has 21 checksum-bearing recordings and 89 present Player rows. A
slot-count projection therefore gives:

```text
21 * 8 + 89 * 704 = 62,824 bytes
```

That **62,824-byte number is not exact ownership**. In
`Playback___2020.07.25_19_30_12__Sat_.rcx`, four present Player rows contain duplicate
`Player::who` values and collapse to two active checksum Leader rows. This agrees with the existing
channel evidence for that recording: it has two owners. The validated ledger therefore has 87
distinct active Leader rows, not 89:

```text
21 * 8 + 87 * 704 = 61,416 exact owned checksum bytes
62,824 - 61,416       =  1,408 bytes of duplicate-who projection overcount
```

The report retains 62,824 as `checksum_bearing_player_row_projection_bytes` so the historical
number is quantified and its two-row discrepancy is visible. Only 61,416 is labelled exact
checksum ownership. Neither number represents complete channel comparisons, and neither is
derived from the recorded Adler words.

## Why the scoreboard remains zero

The ledger reports fixed-walk coverage and provenance beside replay metadata, but never calls a
`SimState` channel-installation API. Its three admission fields are pinned false:

- `walk_complete`
- `exact_channel_producer`
- `substantive_scoreboard_eligible`

The unowned fixed-header/body gaps and every dynamic child remain explicit. Consequently the
standard report can expose the 61,416-byte exact corpus tranche (and the separate 62,824-byte
Player-row projection) while the actual `leaders`
`substantive_compares` and `substantive_matches` both remain zero. A later complete live Leader
owner may merge the ledger-issued spans, but the prefix itself must never be promoted to a final
channel checksum.

## Report shape

The JSON report carries two independent views:

- `totals.sparse_leaders_prefix` sums the exact prefix bytes, preserves the separate 62,824-byte
  Player-row projection and its 1,408-byte duplicate-who overcount, and records explicit zero
  prefix contributions to the substantive scoreboard;
- each file's `initial.leaders_prefix` records row counts, fixed bytes, owned/unknown bytes,
  expiration class, admission refusals, and byte totals by setup provenance.

The ordinary `totals.per_channel.leaders` record remains authoritative for checksum compatibility.
The sparse-prefix record is coverage/provenance evidence only.
