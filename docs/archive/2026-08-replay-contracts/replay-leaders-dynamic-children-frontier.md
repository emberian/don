# Replay Leaders dynamic-children frontier

This tranche starts at the exact `Personality +0x6dd4` refusal left by the deferred-history
frontier and reproduces every remaining `LeaderData::walk_data` visitor call. The resulting
walk receipt reaches `Complete`, but it is intentionally only a conditional-authority
receipt: no same-frame canonical `Sim` owner exists for the supplied children, so Leaders
channel 8 still cannot issue or install a checksum.

## Supported evidence

```text
riseofnations.exe sha256 30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079
rise.pdb          sha256 334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5
RSDS GUID/age              51D4F219-61C6-4F84-9D5B-C3361B0D291F / 1
```

The PDB pins the suffix layout:

```text
+0x6c0c  BitMask<806>       tech
+0x6c80  BitMask<806>       tech_at_start
+0x6cf4  BitMask<806>       obs_flags
+0x6d68  BitMask<17>        conquest_wonders
+0x6d78  BitMask<17>        conquest_wonders_in_game
+0x6d88  BitMask<24>        conquest_racial_powers
+0x6d98  BitMask<44>        rare
+0x6dac  BitMask<44>        rare_owned
+0x6dc0  BitMask<44>        rare_conquest
+0x6dd4  Personality        pers                         96 bytes
+0x6e34  Sites              sites                       28 bytes
+0x6e50  SimpleArray<int>   mil_trainers                28 bytes
+0x6e6c  SimpleArray<int>   new_rares                   28 bytes
+0x6e88  SimpleArray<int>   oil_patches                 28 bytes
+0x6ea4  String             prod_script                 20 bytes
+0x6eb8  LeaderDataEncrypt* data_encrypted               4 bytes
+0x6ebc  int                warn_cities_stamp
+0x6ec0  int                warn_caravans_stamp
+0x6ec4  int                warn_universities_stamp
+0x6ec8  MakeList           make_list                   28 bytes
```

The three warning stamps are object state but are not visited by this walker.

`Personality` is 24 consecutive `i32` fields in PDB order: `rush`, `cities`, `upgrades`,
`arms`, `army`, `army_size`, `raid`, `invade`, `target`, `strategy`, `raze`, `spells`,
`forts`, `nukes`, `air`, `naval`, `market`, `scouts`, `civilians`, `early_army`,
`friendly_human`, `alliance_human`, `friendly_ai`, and `alliance_ai`. The authority keeps
these names rather than exposing an untyped 96-byte blob.

## Executable program order

Capstone and `schema/walkops.json` agree that retail does not walk these objects in address
order. After the fixed body and eight `Diplomacy` children, `LeaderData::walk_data`
`0x006d6750` performs:

```text
006d67c7  Personality raw bytes [Leader+0x6dd4,+0x6e34)
006d67d8  tech header                  8 bytes
006d67eb  tech payload                 `size` bytes
006d6809  tech_at_start header         8 bytes
006d681c  tech_at_start payload        `size` bytes
006d683a  obs_flags header             8 bytes
006d684d  obs_flags payload            `size` bytes
006d686b  conquest_wonders header      8 bytes
006d687e  conquest_wonders payload     `size` bytes
006d689c  wonders_in_game header       8 bytes
006d68af  wonders_in_game payload      `size` bytes
006d68cd  racial_powers header         8 bytes
006d68e0  racial_powers payload        `size` bytes
006d68f7  Array<Site>::walk_data       0x0047cee0
006d6904  Array<MakeObject>::walk_data 0x0047d440
006d6911  SimpleArray<int>::walk_data  0x00473120
006d691e  SimpleArray<int>::walk_data  0x00473120
006d692b  SimpleArray<int>::walk_data  0x00473120
006d6937  String::walk_data            0x00a1b2d0
006d694b  rare header                  8 bytes
006d695e  rare payload                 `size` bytes
006d697c  rare_owned header            8 bytes
006d698f  rare_owned payload           `size` bytes
006d69ad  rare_conquest header         8 bytes
006d69c0  rare_conquest payload        `size` bytes
006d69d6  LeaderDataEncrypt::walk_data 0x006d9900
```

Each fixed `BitMask<N>` stores `bits`, byte `size`, flags, then payload. The inlined visitor
walks the first eight bytes (`bits` and `size`) and exactly `size` payload bytes beginning at
object `+0x0c`; it skips flags at `+0x08`. The binder requires the PDB-shaped values exactly:
806 bits/101 bytes, 17 bits/3 bytes, 24 bits/3 bytes, or 44 bits/6 bytes. A changed header
refuses instead of changing the transcript shape silently.

## Length-bearing representation and history

The array child routines always visit a local signed 32-bit length. If it is zero, that is
the entire checksum representation: current capacity, increment, flags, pointer, and spare
storage do not participate. For a positive length the visitor then walks:

```text
capacity         i32
increment        i16
flags & 0xbf     u8
elements         length * element-size bytes
```

`Site` is six `i32` values (24 bytes), `MakeObject` is ten `i32` values (40 bytes), and each
`SimpleArray<int>` uses a contiguous four-byte element representation. Length is retained
explicitly rather than inferred from a host vector. Negative lengths, a vector/count
disagreement, or capacity below positive length refuse. Tests also establish the subtle
normalization rule: changing only flag bit `0x40` does not change Adler-32, while changing a
walked flag bit does.

`String::walk_data` visits its logical UTF-16 length as an `i32`, then exactly that many
two-byte code units. It does not visit object pointers, hash/capacity metadata, or a trailing
terminator. The authority rejects more than 65,535 logical code units, matching retail's
stored 16-bit current length.

## Decoded economy transcript

`LeaderDataEncrypt::walk_data` does not hash the 248 encrypted object bytes. At `0x006d9900`
it decodes each word before passing it to the visitor. The exact 62-`i32` plaintext order is:

1. for resources 0 through 5: `bucket`, `leftover`, `resource_cap[resource]`, `over_cap`,
   `resources`, `support`, `income`, `rate`, `bonus`;
2. `resource_cap[6]`;
3. `epoch[0..4]`;
4. `ages`, `epochs`, `discovered`.

The current runtime projection owns 49 of those 62 plaintext dwords. The binder compares
those values as decoded integers, not as coincidentally shaped object bytes. The remaining
13 dwords stay explicit conditional authority.

## Agreement and accounting

For each active Leader with empty arrays and an empty string, this tranche adds exactly 770
visitor bytes:

```text
Personality                                           96
three 806-bit masks                    3 * (8 + 101) 327
two 17-bit and one 24-bit masks        3 * (8 + 3)    33
five empty array children                    5 * 4     20
empty UTF-16 String                                  4
three 44-bit masks                      3 * (8 + 6)    42
decoded LeaderDataEncrypt                           248
total                                                770
```

Existing same-frame runtime claims duplicate-check 420 of those bytes per active row:

```text
Personality.raid                                       4
tech payload                                         101
tech_at_start payload                                101
three rare payloads                                   18
49 decoded economy dwords                       49 * 4
total                                                420
```

The other 350 bytes are conditionally admitted. Mutations on either side of a duplicate
(`Personality.raid`, tech payloads, rare payloads, or an owned economy dword) refuse before a
receipt exists. Conditional-only personality, observation-mask, container-history, string,
and missing economy mutations alter the exact Adler transcript.

Dynamic arrays and strings extend the 770-byte default by their actual visited payload; the
ledger records conditional and duplicate bytes per named child. It never relabels the prior
fixed/deferred bytes as new coverage.

## Authority remains red

A successful bind now has `LeadersWalkBoundary::Complete`, because all eight rows can be
executed in retail program order. That structural fact is deliberately separate from source
authority:

- `source_produced_walked_bytes()` remains zero;
- `checksum()` always returns `Err(complete_frontier)`;
- `installed_in_scoreboard()` remains false;
- `CheckAll` Leaders remains non-installed, non-exact, and non-substantive.

The required next step is a same-frame canonical `Sim` join for every conditional child and
history field. Until then, this receipt is useful for provenance, mutation, and future join
work but cannot be presented as a retail checksum channel.

## Verification

The focused corpus target runs five tests covering exact program order and the 770/420/350
ledger, full-boundary-but-red issuance, array and UTF-16 representation history, normalized
flags, conditional Adler mutations, invalid mask/array/string refusal, duplicate disagreement
in both authority and runtime directions, decoded economy agreement, and the zero substantive
Leaders score.
