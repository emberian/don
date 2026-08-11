# Replay Leaders deferred-history frontier

This tranche crosses the first deferred `LeaderData` field, completes retail's fixed-body
visitor call, reuses the already-established eight `Diplomacy` children, and stops at the
next visitor call: `Personality` at `+0x6dd4`. It is a conditional authority receipt, not a
checksum-channel producer, and does not install Leaders channel 8.

## Supported evidence

```text
riseofnations.exe sha256 30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079
rise.pdb          sha256 334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5
RSDS GUID/age              51D4F219-61C6-4F84-9D5B-C3361B0D291F / 1
```

`python3 re/scripts/pdb_types.py ron-bin/sbl/rise.pdb --struct LeaderData` records the
remaining fixed fields exactly:

```text
+0x14de  unsigned short[129][64] reg_buildings          16,512 bytes
+0x555e  unsigned short[129]     num_buildings             258 bytes
+0x5660  unsigned short[129]     high_buildings            258 bytes
+0x5762  unsigned short[352]     num_units                 704 bytes
+0x5a22  unsigned short[806]     num_queued              1,612 bytes
+0x606e  <PDB padding>                                      2 bytes
+0x6070  int[129]                last_building_finished     516 bytes
+0x6274  int[352]                last_unit_finished       1,408 bytes
+0x67f4  unsigned char           ages_queued                 1 byte
+0x67f5  unsigned char           epochs_queued               1 byte
+0x67f6  unsigned char[64]       reg_attacked                64 bytes
+0x6836  unsigned char[64]       reg_wars                    64 bytes
+0x6876  unsigned char[64]       reg_neutrals                64 bytes
+0x68b6  unsigned char[64]       reg_allies                  64 bytes
+0x68f6  unsigned char[38]       num_bonus_cards             38 bytes
+0x691c  unsigned char[6]        num_ctw_rate_bonuses         6 bytes
+0x6922  <PDB padding>                                      2 bytes
+0x6924  int                      defeat_stamp                 4 bytes
+0x6928  unsigned char            team_color                   1 byte
+0x6929  unsigned char            ally_mask                    1 byte
```

Both unnamed two-byte spans are inside the one retail walk of `[+0x08,+0x692a)`. They are
therefore checksum state even though the PDB has no field name for them. `Leader::init`
initially clears them as part of its `memset(Leader, 0, 0x6929)` path, but save/load uses the
same raw visitor range. The binder consequently requires their current bytes explicitly; it
does not infer that construction-time zero remains current.

## Regional matrix representation and history

The generated `LeaderCols` descriptor reports `reg_buildings` as:

```text
offset +0x14de, size 16,512, count 8,256
ctype unsigned short[129][64]
Repr::Deferred, Pool::None
```

`Deferred` is the generator's allocation policy for arrays above 384 elements, not evidence
that the retail field is absent. The PE establishes the useful logical shape. In
`Leader::init`:

```text
006e4acd  lea  edi,[ebx+0x14de]
006e4b7d  push 0x40
006e4b80  rep stosd                    ; 128 u16 values
006e4b82  stosw                        ; final 129th u16
006e4b91  add  edi,0x102               ; next 129-u16 region slice
006e4b9f  cmp  esi,0x7f                ; surrounding 127-field init loop
```

The matrix clearing arm is guarded by `esi < 0x40`, so it emits 64 slices of 129 `u16`
values. `LeaderData::get_reg_buildings` independently pins the lookup formula:

```text
006d5484  imul eax,edx,0x81            ; region * 129
006d548b  add  eax,ecx                 ; raw building TypeIndex
006d548d  movzx esi,word ptr [edi+eax*2+0x11a2]
```

Building TypeIndex 414 turns `+0x11a2 + 414*2` into `+0x14de`. The conditional authority
therefore exposes the executable's observed order as `64 regions x 129 building slots`, flat
index `region * 129 + (TypeIndex - 414)`. Tests mutate a far cell and inspect its exact
little-endian byte position. They separately mutate `high_buildings`: the historical
high-water array is not derived from the current regional matrix or current aggregate count.

## Owners and agreement

`bind_deferred_history_frontier` takes three inputs:

1. the established generated fixed prefix ending at `+0x14de`;
2. the same explicit caller-supplied current `LeaderCols` snapshot;
3. an explicit eight-row conditional authority containing each 8,256-value regional matrix
   and the four unnamed current padding bytes.

It re-renders every materialised column field before `+0x14de` and compares it with the
already-bound prefix, preventing a caller from splicing two different column snapshots.
After the matrix:

- materialised PDB fields come from `LeaderCols`;
- `num_buildings[129]`, `num_units[352]`, `num_bonus_cards[10]` (the established conquest
  byte), and `ally_mask` must agree byte-for-byte with the established runtime owner;
- `num_queued[806]` is itself `Repr::Deferred, Pool::None`, but the existing
  `victory_score::LeaderState` owns all 806 current `u16` values, so those exact runtime bytes
  fill the generated representation hole;
- all missing, changed-layout, short-matrix, changed-snapshot, and duplicate disagreement
  cases refuse before a receipt exists.

The executable's mutation history supports keeping these as independent state. For example,
`Leader::track_queued` writes `num_queued` at `0x006e0f58`; `Leader::plan_strategy` writes the
four regional history arrays (`reg_attacked`, `reg_wars`, `reg_neutrals`, `reg_allies`) in the
`0x006b9bf8..0x006bbcc2` body; and `Leader::defeat_by` writes `defeat_stamp` at `0x006d1d98`.
No current count is substituted for a historical field.

## Child order and stopping boundary

Capstone over `LeaderData::walk_data` recovers the exact program order:

```text
006d6796  walk [Leader+0x0008, Leader+0x692a)
006d6798  lea  ecx,[Leader+0x692c]
006d67ac  walk [ecx,ecx+0x5c)          ; loop count 8, ends +0x6c0c
006d67b8  lea  ecx,[Leader+0x6dd4]
006d67c7  walk [ecx,ecx+0x60)          ; Personality
```

The two layout bytes `[+0x692a,+0x692c)` are skipped between visitor calls and are never fed
to Adler-32. The established taunt runtime owns every byte of each 92-byte `Diplomacy` row
(`agree`, `any_offer`, `treaty`, six offers, six declarations of war, and eight attacks), so
the binder can lawfully execute those eight calls. It stops before `Personality`; the sparse
existing `personality_raid` dword at `Personality+0x18` does not own the 96-byte child.

Although the tech bitmasks occupy the lower addresses beginning at `+0x6c0c`, retail calls
the `Personality` visitor first. They are therefore unreachable in checksum program order at
this boundary.

## Coverage remains conditional and red

For the first active row, this tranche advances the executable walk by:

```text
fixed continuation  +0x14de .. +0x692a    21,580 bytes
Diplomacy[8]         8 * 0x5c                 736 bytes
total delta                                    22,316 bytes
new boundary       Personality at +0x6dd4
```

The caller-supplied matrix, padding, and materialised fields account for 19,968 bytes in the
fixed continuation. Of those, 964 bytes duplicate current runtime claims and must agree,
leaving 19,004 conditionally admitted bytes per active row. Runtime-owned `num_queued`
contributes the other 1,612 fixed bytes, and runtime-owned Diplomacy contributes 736 child
bytes. These categories are reported separately rather than relabeling the whole walk delta
as new source-produced coverage.

There is still no same-frame canonical `Sim` join for the regional/history authority or the
generated columns. `source_produced_walked_bytes()` is zero, `checksum()` always returns the
frontier as `Err`, `installed_in_scoreboard()` is false, and no `SimState` hook exists.
Leaders substantive compares and matches remain zero.

## Verification

An isolated `git archive HEAD` checkout with the local replay corpus mounted read-only ran all
five focused tests: exact boundary/accounting, short-matrix refusal, executable region-major
indexing, independent matrix/high-water/padding mutations, duplicate current-count refusal in
both directions, runtime-deferred queue mutation, Diplomacy child mutation, column-snapshot
identity, and the zero substantive Leaders score. Result: 5 passed, 0 failed, 0 skipped.

Persvati clean-checkout job
`replay-leaders-deferred-history-20260811T180607Z-82882-9300-ff96ce0e063a` compiled the focused
target successfully with the two new-file overlays and allowlisted balance asset. Persvati does
not carry the replay corpus, so its earlier executable run self-reported each corpus case as
`SKIPPED — NOT A PASS`; that run is deliberately not counted as behavioral validation.
