# Canonical STRAFE runtime integration

`Unit::do_strafe` (`0x005EAB00`, order 16) now has one bounded production route from the
canonical `Sim` object pass. The route consumes the landed 57-byte payload authority, recovered
CFG planner, atomic detached-image transaction, and top-level air-physics planner. It does not
introduce another persistent STRAFE representation.

The canonical ownership chain is:

1. `order::Order::strafe` validates the full `patrol::StrafeOrder`, rejects target addresses that
   the flattened header cannot represent, and publishes the same identity in header and payload.
2. `order_dispatch::{adopt,publish}` retain `PatrolPayload::Strafe` without narrowing it.
3. DoNSave v13 writes the authority's fixed 47-byte tag-8 leaf. The target triple remains in the
   generic header and must agree. Versions 7--12 never emit tag 8 and reject a typed down-save.
4. `canonical_strafe_runtime` resolves the actor and active Unit target from the live `World`,
   consumes revision-bound type/search facts, builds the registered air-physics proof, and feeds
   exact ordered external receipts into `prepare_strafe_frame`.
5. `commit_strafe_activation` revalidates the complete World/order/path/RNG snapshot and publishes
   the detached order, path, Unit position/facing, `recharging` (`UnitData+0xAE`), `spell_time`, and
   air-physics RNG image once. The `Sim` caller then applies the already-preflighted animation or
   ammo effect synchronously and verifies the predicted final RNG state.

## Admitted and refused cones

The mounted route admits a live Unit target, ordinary fixed-speed non-animal/non-helicopter air
physics, no invalid-location collision, and hold, attack-animation, or preflighted ordinary ammo
tails. A due mod-16 search requires a revision-bound observation whose returned identity is
revalidated against the live World.

Building targets, captain repair, missing-target fallbacks, helicopter/animal/missile tails,
invalid-location collision, returning/landing, due queued reacquisition, and ammo without installed
shooter rules refuse before canonical mutation. Those refusals are deliberate; registration of this
bounded route does not close the full retail executor ledger.

## Executable evidence

`canonical_strafe_save_resume` starts from one v13 save, runs an uninterrupted branch and a loaded
branch through the real `Sim::do_frame`, and compares the stored tag-8 payload, order queue, path,
Unit position/facing/animation latch, `recharging`, live ammo slot, and simulation RNG. The fixture
deliberately reaches the mod-8 cruising-altitude draw followed by both ammo-scatter draws, so
equality is evidence about resumed RNG consumption rather than a no-draw frame.
