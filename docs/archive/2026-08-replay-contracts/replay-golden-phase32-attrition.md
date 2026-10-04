# Golden frames 26–32 Unit attrition early-return transaction

This tranche owns a detached, atomic prefix of the supported 2024 replay's first
`Unit::process_attrition` cadence. It does **not** mount the prefix in `Sim::do_frame`, and it
does not claim that frames 2–25 are reproduced. No production retail capture is checked into the
repository yet, so the implementation remains source-exact/capture-gated rather than installed.

## Exact chronology

Authority is the matched `riseofnations.exe`, SHA-256
`30478a44…625079`, plus `rise.pdb`. The caller is `Unit::process` `0x00610BC0`:

```text
0x006115EA  test signed (Game.frame + UnitData.o) % 32 == 0
0x00611609  UnitData.unit_masks2 &= ~0x00040000
0x00611612  Unit::process_attrition()
  0x005E11A6  UnitData.unit_masks &= ~0x00400080
  0x005E11B2  UnitData.attrition = 0
  ...         read WData.who under the decoded Unit position
  0x005E12A5  return when who < 0 and owner neutral_attrition == 0
  0x005E12C5  return when who == Unit owner
0x00611617  inspect unit_masks2 bit 1
0x00611626  UnitData.unit_masks2 &= ~3
```

The bit-1 branch at `0x0061161A` has one final value in both arms: if bit 1 was set retail clears
bits 0 and 1; if it was clear retail clears bit 0 and leaves bit 1 clear. The receipt nevertheless
records the pre-call and post-return values separately so the caller/child/caller order is not
erased.

The golden phase mapping is fixed:

| frame | owner-0 actor | setup role |
|---:|---:|---|
| 26 | `o6` | Citizen |
| 27 | `o5` | Citizen |
| 28 | `o4` | Citizen |
| 29 | `o3` | Citizen |
| 30 | `o2` | Dutch Merchant |
| 31 | `o1` | Merchant |
| 32 | `o0` | Scout |

The mapping proves only the call gate. It does not prove the dynamic positions or territory
owners: earlier Unit/Guy/Wall/road work may change them.

## Whole-owner authority

`GoldenPhase32AttritionAuthority` is keyed by:

- the stable Unit `Handle { id, generation }`, owner, object index, uid and current type;
- exact stored and decoded X/Y, derived WCoord X/Y and row-major WData cell index;
- exact signed `WData::who`;
- frame plus the world/victory frame agreement;
- the full serialized canonical Sim before-image;
- the complete terrain `WorldChecksum` section ledger and byte image;
- the canonical object-World digest;
- the persistent `vic_leaders` and live `step8` owner-0 `neutral_attrition` mirrors, agreeing at
  zero;
- an explicit source assertion that owner 0's retail ScenarioData attrition-free-point array is
  empty.

The replay-side `GoldenPhase32AttritionCapture` additionally binds the supported replay and
executable hashes, SHA-256 of the immediate pre-actor Sim and terrain images, and one SHA-256
composition digest over every capture claim. It retains a provenance pointer to the separately
attested frame-2 post-tick authority. That pointer is **not** frame-26 authority: frames 2–25 are
still unowned, so a new immediate full-Sim capture is mandatory.

Planning is read-only. Commit reruns all authority checks, including byte equality with the whole
canonical Sim before-image, before its first write. The four stores are then infallible. A stale
Handle, changed Unit field, changed unrelated WData cell, changed Leader mirror, or changed save
state therefore leaves every target field untouched.

## Honest residuals

Only two dynamic outcomes are admitted:

- same-owner `WData::who == 0`;
- signed `WData::who < 0` with both neutral-attrition mirrors equal to zero.

The following remain typed red:

- any populated ScenarioData attrition-free-point registry;
- foreign `WData::who`, beginning with the territory Leader/diplomacy/type selection;
- all later supply searches, period selection, attrition damage, and `Leader::meet` effects;
- general tick scheduling and the unowned frames between the frame-2 oracle and each immediate
  capture.

This boundary matters because applying the universal reset and then discovering a missing supply
authority would be a partial tick. Foreign terrain is rejected during planning, before even the
otherwise universal writes.

## Verification

```sh
cargo test -p don-sim --test golden_phase32_attrition
cargo test -p don-replay --test setup_2024_phase32_attrition
```

The focused suites cover all seven frame/object mappings, exact masks and attrition writes, both
positive cones, no-RNG behavior, foreign/scenario/Leader refusals, unrelated full-World staleness,
atomic failure, replay/executable/composition binding, and DoNSave round-trip preservation of the
committed fields.
