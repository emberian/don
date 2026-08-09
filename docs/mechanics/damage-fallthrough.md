# `Object::do_damage` building fallthrough and capture-zero tail

Status: Tier C, address-bounded executable reconstruction. This proof pack has not been
executed against retail and does not promote the surrounding damage pipeline.

## Frozen provenance

- Retail image: `ron-bin/riseofnations.exe`, SHA-256
  `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
- Symbols/types: `ron-bin/sbl/rise.pdb`, SHA-256
  `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`.
- Procedure: PDB `Object::do_damage` at `0x0064A480`, 9,214 bytes, `ret 0x20`.
- Constants/types: PDB `TypeIndex`, `LeaderDataEncrypt`, `CityData`, `Constants`, and the
  named procedures in `schema/rise-procs.tsv` / `schema/rise-symbols.tsv`.
- Shipped rules: `GATHER_RATE = 450` (`Constants +0x27C`) and
  `LAKOTA_CAV_DMG_BONUS = 85` (`+0x85C`, parsed at scale 256 from
  `"1/3 resources per dmg point"`).

The executable bytes, not decompiler output, are authoritative for every branch and mutation
below. `Object::do_damage` lies in a historical Ghidra function-list gap, so an analysis that
only walks `schema/islands.jsonl` will miss it.

## Building-only block: `0x0064BEB7..0x0064C10C`

The victim `is_build()` virtual is the outer gate. A false result jumps directly to the
`Object::do_damage` epilogue; a true result admits two independent transactions before the
splash scan begins.

### Lakota/Sunka Wakan damage bounty

`0x0064BED0..0x0064C089` first tests
`attacker->is(SUNKAWAKAN /* TypeIndex 0xCD */, 0)`. For that attacker only, retail classifies
the victim building in this exact order:

1. `BuildTypeData::is_gather_type()` (`vt +0x90`). If true,
   `BuildTypeData::get_good()` supplies the resource.
2. Otherwise `BuildTypeData::is_gather_enhancer()`; if true,
   `BuildTypeData::get_enhancing_good()` supplies it.
3. Only GoodIndex 0, 1, and 4 — Food, Timber, and Metal — qualify.

For a qualifying good, with `D` the retained post-scale damage passed to `take_damage`:

```text
threshold = GATHER_RATE << 4                         // shipped: 7200
delta = wrap32(LAKOTA_CAV_DMG_BONUS * threshold * D) / 256
leftover[good] = wrap32(leftover[good] + delta)
while leftover[good] >= threshold:
    leftover[good] = wrap32(leftover[good] - threshold)
    payouts = wrap32(payouts + 1)
bucket[good] = wrap32(bucket[good] + payouts)
emit_gold_coin_particles(victim.o, victim.who)
```

Division is signed truncation toward zero. The two products are 32-bit wrapping `imul`s.
`LeaderDataEncrypt::leftover` is at `+0x18`; `bucket` is at `+0x00`. Retail XORs those
logical fields with `0x3421` and `0x8221` around the accesses. Coin particles are emitted
even when `payouts == 0`.

### Fire-raft/air dock ejection

At `0x0064C089..0x0064C10C`, retail independently tests
`ObjectTypeData::is_dock()` (`vt +0x108`). A dock calls
`Object::eject_contents(1, -1, 0, 1)` when the attacker is Fire Raft
(`TypeIndex 0x14E`), Heavy Fire Raft (`0x14F`), or has air domain (`type +0x218 == 2`).
The two Fire Raft comparisons short-circuit the domain read. The call follows the bounty
state writes and coin-particle call.

## Capture returned zero: `0x0064C558..0x0064C86B`

This edge is reached only after the already bounded city/same-owner gates and an enemy
city's `Build::check_capture(attacker.o, attacker.who)` returned zero. The continuation is:

1. Require victim object flag `0x20`, `Build::is_active()`, and
   `victim.current_hits >= victim.hits(0)`. Any failed gate exits.
2. Read signed `BuildData::city` at `+0x72`.
3. If `attacker.o >= 0`, attacker leader flag `0x4` is clear, the attacker is siege,
   `UnitData::unit_masks & 0x40000`, and `Object::get_army() >= 0`, call
   `Armies[attacker.who][army]->charge(victim.o, victim.who)`.
4. `[ebp-0x6C]` is the pre-hit "active and hits >= max" snapshot. When it was false,
   suppress the rest if signed `frame - CityData::raid_stamp < 300`. This suppression is
   after the army charge.
5. Store the current frame to `CityData::raid_stamp` (`CityData +0x18`).
6. Admit a local presentation event when the console player owns the victim; owns the
   attacker; or is allied to the applicable side and the victim is seen. Retail can call
   `victim->is_seen(console, 0)` three times: victim-side audience, attacker-side audience,
   and repeated attacker-side message selection.
7. Build a `TextBubble` from retail String slots `0x00C99F50` (attacker-side) or
   `0x00C99F64` (victim-side), then issue `MessageWin::add_event` at the victim coordinates.
   Attacker-side uses event kind `-7`; victim-side uses `-6`; duration is `0xC00`. The neon
   color is selected from the victim owner's team color.

`TextBubble::init` only consumes the string and final owner byte in this build; the otherwise
odd duplicated coordinate value in the unused `Font*` argument has no simulation effect.
The implementation therefore freezes the meaningful presentation request rather than
inventing a font dependency.

## Executable proof surface

`crates/don-sim/src/systems/combat/damage_fallthrough.rs` separates pure, fail-closed
planning from ordered application. Its receipts make the following mutation order testable:

```text
leftover add -> each threshold subtraction -> bucket add -> coin particles -> dock eject
army charge -> city raid stamp -> local raid event
```

The apply boundary admits stale leader/city snapshots before its first mutation. Tests in
`crates/don-sim/tests/combat_damage_fallthrough.rs` pin wrapping/truncating bounty math,
the zero-payout particle rule, Fire Raft domain short-circuiting, charge-before-cooldown,
stamp-without-audience, repeated visibility reads, event polarity, and stale-state rejection.

## Integration handoff and residual boundary

The new module is intentionally exclusive. It needs this one-line integration in
`crates/don-sim/src/systems/combat.rs`:

```diff
 pub mod damage_world;
+pub mod damage_fallthrough;
```

After that line lands, the recovered deterministic post-hit chain is contiguous through the
already implemented splash scan (`0x0064C10C..0x0064C4E3`) and capture attempt
(`0x0064C4E3..0x0064C558`) to the `Object::do_damage` epilogue at `0x0064C86B`.

The residual `Object::do_damage` frontier is the earlier presentation-entangled interval
`0x0064AA60..0x0064BA17` plus live world-adapter wiring. `Build::check_capture` itself is
2,357 bytes at `0x006276A0`; this proof pack still models only its atomic return receipt and
does not claim that body is generally ported.
