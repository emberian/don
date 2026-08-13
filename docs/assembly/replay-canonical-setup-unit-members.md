# Canonical replay setup-Unit member authority

Lane: first-checkpoint setup / Groups prerequisite · 2026-08-13.

## Product join

`setup_unit_member_authority` joins the existing exact `Setup::build_units` receipt boundary to
one canonical `tick::Sim` snapshot. It does not create Units. Given a complete
`BuildUnitsPrefixReceipt`, it validates all native call, RNG, `Objects::init_unit`, container,
Guy-identity, and stable authority-key invariants before resolving selected members through the
Sim's sparse Unit band.

Every accepted member retains:

- its native setup call and ordinal plus the complete `InitUnitAuthorityReceipt`;
- `{id,generation,owner,o}` allocation identity and exact current Handle;
- current canonical Unit row, type, UID, group/form scalars, position/facing, orders, and path;
- exact replay-carried `UnitTypeData` facts from the SHA-gated serialized Rules section; and
- revision/digest, replay-file SHA, frame, World checksum, and RNG snapshot provenance.

The join rejects a stale generation, inactive or missing row, lagging Unit mark, divergent
World/Sim type mirrors, malformed setup receipt, changed replay, changed World, or changed RNG.
No `CheckSumsCommand` word is read.

`bind_canonical_setup_citizens` selects calls by `StartingUnitPhase::Citizen` rather than by an
assumed object range. For the 2024-02-23 Dutch clear-pool Groups witness, the exact outer schedule
is Scout, two Dutch Merchants, then four Citizens. The focused fixture therefore demonstrates that
a valid product input returns setup ordinals and object ids 3..6 with Rules-derived type 50, role
262912, flags 6273, and ground domain 0.

Each result directly converts its replay-carried fields to `GroupMoveTypeFacts` and can key an
independently resolved land speed to the exact Handle generation. It deliberately does not turn
Rules `moves` into `UnitData::speed`: the landed Sim producer still requires the live
terrain/Leader/hero/Constants join. The Groups lane can therefore consume formation/type identity
immediately while speed remains an explicit, fail-closed next authority.

## Evidence boundary

The focused fixture uses the real replay file and serialized Rules, but deliberately supplies a
synthetic complete setup receipt and canonical Sim snapshot so mutation gates can exercise the
join. It is not an attestation that retail's 2024 setup ran in don-sim. The four real Citizen
`Objects::init_unit` receipts and the canonical chronology through frame 379 remain absent.

The immediately preceding 2018 setup lane now carries ordinal one through placement, allocation,
one-Guy RNG, location, collision/common tail, visibility, and the complete outer initializer. It
emits the same generic `InitUnitAuthorityReceipt`, while honestly retaining the remaining external
graphics/terrain/stat/Leader input provenance. Once each real setup call receives those inputs,
this generic join is the single handoff to Groups, Units/Guys, Cities' Unit census, and later
first-checkpoint consumers; none needs a parallel Unit pool.

Focused gate:

```sh
cargo test -p don-replay --test setup_unit_member_authority -- --nocapture
```
