# Replay Leaders generated fixed-body frontier

This tranche advances the exact Leaders prefix from `LeaderData::gov` at `+0x14` to the
first field which the PDB-generated live-column owner deliberately does not materialise,
`reg_buildings[129][64]` at `+0x14de`. It does not install or issue checksum channel 8.

## Supported binary and PDB evidence

The inputs are the supported Extended Edition pair:

```text
riseofnations.exe sha256 30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079
rise.pdb          sha256 334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5
RSDS GUID/age              51D4F219-61C6-4F84-9D5B-C3361B0D291F / 1
```

`python3 tools/pdb/lookup.py 0x006d6750` identifies `LeaderData::walk_data`. Capstone over
the matching PE shows the exact fixed-range visitor call:

```text
006d6781  test byte ptr [ebx], 1
006d6784  je   006d69db
006d678c  lea  eax, [ebx + 0x692a]
006d6792  push eax
006d6793  push edi                     ; edi = ebx + 8
006d6796  call dword ptr [edx]         ; walk [Leader+8, Leader+0x692a)
```

`python3 re/scripts/pdb_types.py ron-bin/sbl/rise.pdb --struct LeaderData` supplies the
compiler-recorded field sequence. From `gov` onward the prefix remains tightly packed:

```text
+0x0014  int gov
+0x0018  scores ...
+0x0048  unsigned long cannon_time
+0x004c  int handicap
+0x0054  int chat_status[8]
          ... fixed economy, diplomacy, AI, event and regional fields ...
+0x145e  unsigned short reg_terr[64]             ends +0x14de
+0x14de  unsigned short reg_buildings[129][64]   16,512 bytes
```

The generated `leader::FIELDS` table contains 260 fields in `[0,+0x14de)`. `LeaderCols`
materialises 259 of them, totalling 5,310 bytes. The sole exception is the PDB enum array
`TauntRequest last_taunt[8]` at `[+0x354,+0x374)`, conservatively represented as an
aggregate by the generator. `RuntimeLeadersFrontier` already owns those exact 32 bytes from
the current victory/taunt states. There is no padding or other hole before `+0x14de`.

## Authority and agreement gates

`bind_generated_fixed_prefix` accepts an explicitly caller-supplied canonical current
`LeaderCols` instance with exactly eight rows. This is a conditional admission boundary,
not evidence that `Sim` currently owns, populates, or same-frame synchronises those columns.
`LeaderCols::with_capacity` alone is merely zeroed storage and is not authority. No
constructor in this replay module creates or silently fills a column owner.

Before the extended frontier exists, the binder:

1. requires the established tribe frontier still to stop at `+0x14`;
2. renders only PDB fields whose generated representation is materialised;
3. compares every overlapping byte with the victory, step-8, taunt, production-AI and tech
   projections already in `RuntimeLeadersFrontier`;
4. separately compares `LeaderCols::tribe` with the live tribe claim. This remains required
   when replay setup selected random sentinel `24`: the replay selector is lineage, while
   both current live owners must agree on the resolved value in `0..24`;
5. fills only `[+0x354,+0x374)` from the established current taunt owner;
6. refuses on the first disagreement, missing byte, wrong row count, changed prior boundary,
   or unsupported generated field layout.

The merge is therefore duplicate-owner agreement, not last-writer-wins and not zero-fill.
Tests mutate a duplicate score in each direction, mutate the resolved random tribe in the
column owner, and mutate the final `reg_terr` element to prove the new tail bites the Adler
prefix.

## Coverage and red boundary

For the first active row, the lawful contiguous walk advances exactly:

```text
old boundary  +0x0014  gov
new boundary  +0x14de  reg_buildings[129][64]
prefix delta  0x14ca = 5,322 bytes
```

Some of those 5,322 bytes were already sparsely owned but unreachable beyond earlier gaps;
the receipt reports conditionally admitted and duplicate-checked byte counts separately. It
does not mislabel the whole delta as new provenance or source-produced coverage.

The 16,512-byte matrix at the new boundary is `Repr::Deferred` in the generated owner.
Later materialised fields, the remainder of the fixed byte range, the eight `Diplomacy`
children, length-bearing children, `Personality`, and the decoded `LeaderDataEncrypt`
transcript remain unreachable until every preceding owner is established.

`source_produced_walked_bytes()` is pinned zero until a same-frame canonical `Sim` join
exists. `checksum()` always returns `Err(LeadersWalkFrontier)`, including a synthetic empty
roster; `installed_in_scoreboard()` is pinned false and there is no `SimState` hook. Leaders
substantive compares and matches therefore remain zero.
