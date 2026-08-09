//! Persistent Arena identity/state for the recovered ordinary gather lifecycle.
//!
//! This is deliberately a dual boundary.  A retail-backed map may execute the exact
//! [`don_sim::systems::gather_lifecycle`] transactions once every mandatory host fact is
//! present.  The built-in symmetric Arena generator does not provide those facts, so its
//! existing MODEL 3 economy remains a separate gameplay model and an exact request returns
//! a typed refusal without mutating this state.

use std::collections::BTreeMap;
use std::convert::Infallible;

use don_sim::rng::Random;
use don_sim::systems::collision::CollUnits;
use don_sim::systems::containment::NearbyUnitType;
use don_sim::systems::gather_lifecycle::{
    attached_ordinary_order_state, farm_first_gather_tick, FarmFirstTickDisposition,
    FarmFirstTickOutcome, OrdinaryGatherKind, OrdinaryGatherLifecycleError, OrdinaryGatherTarget,
};
use don_sim::systems::gathering::{
    self, GatherAssignment, GatherRetirement, GatherSite, GatherWorker, NonFlatGatherState,
};

/// Retail object identity is the pair `(who,o)`.  Arena entity slots never recycle during
/// one world lifetime; registration rejects any attempted reuse so the narrower adapter
/// identity cannot silently alias a new object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct GatherObjectKey {
    pub owner: u8,
    pub o: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GatherCapacityAuthority {
    /// `BuildTypeData::max_gatherers`'s measured flat-Farm arm is the literal one.
    FlatFarmOne,
    /// The non-retail Arena generator has no installed LandData/resource-object source.
    MissingRetailTerrainSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SiteState {
    kind: OrdinaryGatherKind,
    capacity: GatherCapacityAuthority,
    site: GatherSite,
}

/// Why the generated Arena cannot enter the exact ordinary-gather path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GatherPrerequisiteRefusal {
    /// Farm activation still needs the exact `FarmData::update` result and game-global
    /// byte used by the one-in-256 footprint move gate.
    FarmUpdateAndGameGlobal,
    /// Camp needs source-backed LandData/MiningList capacity plus the mandatory
    /// `Unit::is_at` and ordered-collision hosts.
    CampTerrainGeometryAndOrderedCollision,
    /// Mine additionally needs the selected Mountain/Cliff object's ordered coordinates.
    MineObjectTerrainGeometryAndOrderedCollision,
}

#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum GatherRuntimeError {
    ObjectIndexOutOfRange(usize),
    RecycledObjectSlot(GatherObjectKey),
    MissingSite(GatherObjectKey),
    MissingWorker(GatherObjectKey),
    WrongSiteKind {
        expected: OrdinaryGatherKind,
        actual: OrdinaryGatherKind,
    },
    MissingAuthoritativeCapacity(GatherObjectKey),
    ExistingExactOrder(GatherObjectKey),
    NoExactOrder(GatherObjectKey),
    FarmLifecycle(OrdinaryGatherLifecycleError<Infallible, Infallible>),
    Retirement(&'static str),
}

/// The exact Farm-only facts which the generated Arena intentionally cannot manufacture.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AuthoritativeFarmFirstTick {
    pub farm_update_result: i32,
    pub game_gate_value: i32,
}

/// Persistent retail gathering fields, independent of the legacy MODEL 3 counters on
/// `Ent`.  Empty exact-order state is meaningful: it proves a fail-closed attempt did not
/// partially link the worker.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ArenaGatherRuntime {
    sites: BTreeMap<GatherObjectKey, SiteState>,
    workers: Vec<GatherWorker>,
    orders: BTreeMap<GatherObjectKey, NonFlatGatherState>,
}

impl ArenaGatherRuntime {
    pub(crate) fn object_index(index: usize) -> Result<i16, GatherRuntimeError> {
        i16::try_from(index).map_err(|_| GatherRuntimeError::ObjectIndexOutOfRange(index))
    }

    pub(crate) fn register_worker(
        &mut self,
        key: GatherObjectKey,
        type_index: i32,
    ) -> Result<(), GatherRuntimeError> {
        if self
            .workers
            .iter()
            .any(|worker| worker.owner == key.owner && worker.unit_o == key.o)
        {
            return Err(GatherRuntimeError::RecycledObjectSlot(key));
        }
        self.workers
            .push(GatherWorker::new(key.owner, key.o, type_index));
        Ok(())
    }

    pub(crate) fn register_site(
        &mut self,
        key: GatherObjectKey,
        uid: u16,
        kind: OrdinaryGatherKind,
        capacity: GatherCapacityAuthority,
    ) -> Result<(), GatherRuntimeError> {
        if self.sites.contains_key(&key) {
            return Err(GatherRuntimeError::RecycledObjectSlot(key));
        }
        let mut site = GatherSite::new(key.owner, key.o);
        site.uid = uid;
        if capacity == GatherCapacityAuthority::FlatFarmOne {
            site.set_authoritative_capacity(1);
        }
        self.sites.insert(
            key,
            SiteState {
                kind,
                capacity,
                site,
            },
        );
        Ok(())
    }

    pub(crate) const fn generated_map_refusal(
        kind: OrdinaryGatherKind,
    ) -> GatherPrerequisiteRefusal {
        match kind {
            OrdinaryGatherKind::Farm => GatherPrerequisiteRefusal::FarmUpdateAndGameGlobal,
            OrdinaryGatherKind::Camp => {
                GatherPrerequisiteRefusal::CampTerrainGeometryAndOrderedCollision
            }
            OrdinaryGatherKind::Mine => {
                GatherPrerequisiteRefusal::MineObjectTerrainGeometryAndOrderedCollision
            }
        }
    }

    fn worker_pos(&self, key: GatherObjectKey) -> Option<usize> {
        self.workers
            .iter()
            .position(|worker| worker.owner == key.owner && worker.unit_o == key.o)
    }

    /// Validate the persistent identity used by a generated-map Gather command, then
    /// return the literal missing prerequisite.  This is a read-only refusal boundary;
    /// legacy MODEL 3 gameplay may proceed separately, but fidelity/readiness cannot.
    pub(crate) fn generated_map_preflight(
        &self,
        worker_key: GatherObjectKey,
        site_key: GatherObjectKey,
        kind: OrdinaryGatherKind,
    ) -> Result<GatherPrerequisiteRefusal, GatherRuntimeError> {
        if self.worker_pos(worker_key).is_none() {
            return Err(GatherRuntimeError::MissingWorker(worker_key));
        }
        let site = self
            .sites
            .get(&site_key)
            .ok_or(GatherRuntimeError::MissingSite(site_key))?;
        if site.kind != kind {
            return Err(GatherRuntimeError::WrongSiteKind {
                expected: kind,
                actual: site.kind,
            });
        }
        Ok(Self::generated_map_refusal(kind))
    }

    /// Transactional exact Farm activation.  All persistent state and the simulation RNG
    /// are copied first; a validation error leaves the live runtime and stream untouched.
    #[allow(clippy::too_many_arguments, dead_code)]
    pub(crate) fn begin_farm<U: CollUnits>(
        &mut self,
        bitmap_units: &U,
        worker_key: GatherObjectKey,
        site_key: GatherObjectKey,
        target: OrdinaryGatherTarget,
        unit_type: NearbyUnitType,
        facts: AuthoritativeFarmFirstTick,
        rng: &mut Random,
    ) -> Result<FarmFirstTickOutcome, GatherRuntimeError> {
        let site_state = *self
            .sites
            .get(&site_key)
            .ok_or(GatherRuntimeError::MissingSite(site_key))?;
        if site_state.kind != OrdinaryGatherKind::Farm {
            return Err(GatherRuntimeError::WrongSiteKind {
                expected: OrdinaryGatherKind::Farm,
                actual: site_state.kind,
            });
        }
        if site_state.capacity != GatherCapacityAuthority::FlatFarmOne {
            return Err(GatherRuntimeError::MissingAuthoritativeCapacity(site_key));
        }
        let worker_pos = self
            .worker_pos(worker_key)
            .ok_or(GatherRuntimeError::MissingWorker(worker_key))?;
        if self.orders.contains_key(&worker_key) {
            return Err(GatherRuntimeError::ExistingExactOrder(worker_key));
        }

        let mut trial_site = site_state.site;
        let mut trial_workers = self.workers.clone();
        trial_workers[worker_pos].assignment = Some(GatherAssignment {
            target_owner: i32::from(site_key.owner),
            target_build: i32::from(site_key.o),
            target_uid: trial_site.uid,
            been_there: false,
            inside_target: None,
        });
        let mut trial_order = attached_ordinary_order_state(OrdinaryGatherKind::Farm);
        let mut trial_rng = *rng;
        let outcome = farm_first_gather_tick::<U, Infallible, Infallible>(
            bitmap_units,
            &mut trial_site,
            &mut trial_workers,
            worker_key.o,
            &mut trial_order,
            target,
            unit_type,
            facts.farm_update_result,
            facts.game_gate_value,
            &mut trial_rng,
        )
        .map_err(GatherRuntimeError::FarmLifecycle)?;

        self.sites.get_mut(&site_key).expect("validated site").site = trial_site;
        self.workers = trial_workers;
        if matches!(outcome.disposition, FarmFirstTickDisposition::Active { .. }) {
            self.orders.insert(worker_key, trial_order);
        }
        *rng = trial_rng;
        Ok(outcome)
    }

    /// Apply the exact Gather-order retirement epilogue.  It owns no world-location or
    /// containment transition; ordinary workers remain in the same collision row.
    pub(crate) fn retire_exact(
        &mut self,
        worker_key: GatherObjectKey,
        site_key: GatherObjectKey,
    ) -> Result<GatherRetirement, GatherRuntimeError> {
        if !self.orders.contains_key(&worker_key) {
            return Err(GatherRuntimeError::NoExactOrder(worker_key));
        }
        let site = self
            .sites
            .get(&site_key)
            .ok_or(GatherRuntimeError::MissingSite(site_key))?
            .site;
        let mut trial_site = site;
        let mut trial_workers = self.workers.clone();
        let retirement = gathering::retire_gather_order(
            Some(&mut trial_site),
            &mut trial_workers,
            worker_key.owner,
            worker_key.o,
            None,
        )
        .map_err(GatherRuntimeError::Retirement)?;
        self.sites.get_mut(&site_key).expect("validated site").site = trial_site;
        self.workers = trial_workers;
        self.orders.remove(&worker_key);
        Ok(retirement)
    }

    pub(crate) fn has_exact_order(&self, worker: GatherObjectKey) -> bool {
        self.orders.contains_key(&worker)
    }

    pub(crate) fn exact_site_for_worker(
        &self,
        worker_key: GatherObjectKey,
    ) -> Option<GatherObjectKey> {
        let assignment = self
            .worker_pos(worker_key)
            .and_then(|pos| self.workers[pos].assignment)?;
        let owner = u8::try_from(assignment.target_owner).ok()?;
        let o = i16::try_from(assignment.target_build).ok()?;
        Some(GatherObjectKey { owner, o })
    }

    /// `None` means the site is not executing the exact lifecycle.  A linked exact site
    /// returns the recovered active (`been_there`) count for payout composition.
    pub(crate) fn exact_active_workers(
        &self,
        site_key: GatherObjectKey,
    ) -> Result<Option<i32>, GatherRuntimeError> {
        let site = self
            .sites
            .get(&site_key)
            .ok_or(GatherRuntimeError::MissingSite(site_key))?;
        if site.site.gather_down < 0 {
            return Ok(None);
        }
        gathering::num_gatherers(&site.site, &self.workers, gathering::GatherCount::Active, 0)
            .map(Some)
            .map_err(GatherRuntimeError::Retirement)
    }

    #[cfg(test)]
    fn site(&self, key: GatherObjectKey) -> Option<GatherSite> {
        self.sites.get(&key).map(|state| state.site)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use don_sim::systems::collision::{place, CollGuy, UnitRow, UnitTable};
    use don_sim::systems::gathering::{GatherNearbyPoint, GatherTile};
    use don_sim::systems::map_terrain::{Coord, World};

    const WORKER: GatherObjectKey = GatherObjectKey { owner: 1, o: 7 };
    const SECOND_WORKER: GatherObjectKey = GatherObjectKey { owner: 1, o: 8 };
    const FARM: GatherObjectKey = GatherObjectKey { owner: 1, o: 3 };

    fn target() -> OrdinaryGatherTarget {
        OrdinaryGatherTarget {
            kind: OrdinaryGatherKind::Farm,
            centre: GatherNearbyPoint {
                x: Coord(0x480),
                y: Coord(0x480),
            },
            corner: GatherTile { tx: 5, ty: 5 },
            x_size: 2,
            y_size: 2,
            domain: 0,
            completed: true,
        }
    }

    fn unit_type() -> NearbyUnitType {
        NearbyUnitType {
            type_index: 0x32,
            domain: 0,
            big_radius: 0x60,
            block_radius: 1,
            unit_flags: 0,
        }
    }

    fn collision() -> (World, UnitTable) {
        let mut world = World::init_default_rules(8, 8);
        let mut units = UnitTable::default();
        place(
            &mut world,
            &mut units,
            UnitRow {
                who: i32::from(WORKER.owner),
                o: i32::from(WORKER.o),
                x: 0x300,
                y: 0x300,
                domain: 0,
                block_radius: 1,
                on_map: true,
                active: true,
                ..UnitRow::default()
            },
            [CollGuy {
                x: 0x300,
                y: 0x300,
                block_radius: 1,
            }],
        );
        (world, units)
    }

    fn runtime() -> ArenaGatherRuntime {
        let mut runtime = ArenaGatherRuntime::default();
        runtime.register_worker(WORKER, 0x32).unwrap();
        runtime
            .register_site(
                FARM,
                11,
                OrdinaryGatherKind::Farm,
                GatherCapacityAuthority::FlatFarmOne,
            )
            .unwrap();
        runtime
    }

    #[test]
    fn object_slots_are_narrow_and_never_recycled_within_a_world() {
        assert_eq!(
            ArenaGatherRuntime::object_index(i16::MAX as usize),
            Ok(i16::MAX)
        );
        assert_eq!(
            ArenaGatherRuntime::object_index(i16::MAX as usize + 1),
            Err(GatherRuntimeError::ObjectIndexOutOfRange(
                i16::MAX as usize + 1
            ))
        );
        let mut runtime = runtime();
        assert_eq!(
            runtime.register_site(
                FARM,
                12,
                OrdinaryGatherKind::Farm,
                GatherCapacityAuthority::FlatFarmOne,
            ),
            Err(GatherRuntimeError::RecycledObjectSlot(FARM))
        );
        assert_eq!(runtime.site(FARM).unwrap().uid, 11);
    }

    #[test]
    fn generated_farm_camp_and_mine_requests_refuse_before_any_link() {
        let runtime = runtime();
        let before = runtime.clone();
        assert_eq!(
            ArenaGatherRuntime::generated_map_refusal(OrdinaryGatherKind::Farm),
            GatherPrerequisiteRefusal::FarmUpdateAndGameGlobal
        );
        assert_eq!(
            ArenaGatherRuntime::generated_map_refusal(OrdinaryGatherKind::Camp),
            GatherPrerequisiteRefusal::CampTerrainGeometryAndOrderedCollision
        );
        assert_eq!(
            ArenaGatherRuntime::generated_map_refusal(OrdinaryGatherKind::Mine),
            GatherPrerequisiteRefusal::MineObjectTerrainGeometryAndOrderedCollision
        );
        assert_eq!(runtime, before);
        assert_eq!(runtime.site(FARM).unwrap().gather_down, -1);
        assert!(!runtime.has_exact_order(WORKER));
    }

    #[test]
    fn authoritative_farm_attach_and_retire_preserve_the_on_map_body() {
        let (_world, units) = collision();
        let before_row = units
            .row(i32::from(WORKER.owner), i32::from(WORKER.o))
            .unwrap();
        let mut runtime = runtime();
        let mut rng = Random::new(0x1234);
        let outcome = runtime
            .begin_farm(
                &units,
                WORKER,
                FARM,
                target(),
                unit_type(),
                AuthoritativeFarmFirstTick {
                    farm_update_result: 1,
                    // Avoid the one-in-256 movement arm; this is an exact declined gate.
                    game_gate_value: 1,
                },
                &mut rng,
            )
            .unwrap();
        assert!(matches!(
            outcome.disposition,
            FarmFirstTickDisposition::Active {
                move_order: None,
                action: 0x23,
                rng_draws: 0
            }
        ));
        assert!(runtime.has_exact_order(WORKER));
        assert_eq!(runtime.site(FARM).unwrap().gather_down, WORKER.o);

        let retired = runtime.retire_exact(WORKER, FARM).unwrap();
        assert!(retired.detached);
        assert!(!runtime.has_exact_order(WORKER));
        assert_eq!(runtime.site(FARM).unwrap().gather_down, -1);
        assert_eq!(
            units.row(i32::from(WORKER.owner), i32::from(WORKER.o)),
            Some(before_row),
            "ordinary Gather retirement must not unlink, teleport, or repaint the worker"
        );
    }

    #[test]
    fn failed_farm_kernel_is_transactional_including_rng() {
        let (_world, units) = collision();
        let mut runtime = runtime();
        let before = runtime.clone();
        let mut rng = Random::new(0x1234);
        let rng_before = rng;
        assert_eq!(
            runtime.begin_farm(
                &units,
                WORKER,
                FARM,
                target(),
                unit_type(),
                AuthoritativeFarmFirstTick {
                    farm_update_result: 0,
                    game_gate_value: 0,
                },
                &mut rng,
            ),
            Err(GatherRuntimeError::FarmLifecycle(
                OrdinaryGatherLifecycleError::UnsupportedFarmUpdateResult(0)
            ))
        );
        assert_eq!(runtime, before);
        assert_eq!(rng, rng_before);
    }

    #[test]
    fn farm_capacity_retirement_does_not_leave_an_exact_order() {
        let (mut world, mut units) = collision();
        place(
            &mut world,
            &mut units,
            UnitRow {
                who: i32::from(SECOND_WORKER.owner),
                o: i32::from(SECOND_WORKER.o),
                x: 0x360,
                y: 0x300,
                domain: 0,
                block_radius: 1,
                on_map: true,
                active: true,
                ..UnitRow::default()
            },
            [CollGuy {
                x: 0x360,
                y: 0x300,
                block_radius: 1,
            }],
        );
        let mut runtime = runtime();
        runtime.register_worker(SECOND_WORKER, 0x32).unwrap();
        let mut rng = Random::new(0x1234);
        runtime
            .begin_farm(
                &units,
                WORKER,
                FARM,
                target(),
                unit_type(),
                AuthoritativeFarmFirstTick {
                    farm_update_result: 1,
                    game_gate_value: 1,
                },
                &mut rng,
            )
            .unwrap();

        let outcome = runtime
            .begin_farm(
                &units,
                SECOND_WORKER,
                FARM,
                target(),
                unit_type(),
                AuthoritativeFarmFirstTick {
                    farm_update_result: 1,
                    game_gate_value: 1,
                },
                &mut rng,
            )
            .unwrap();

        assert!(matches!(
            outcome.disposition,
            FarmFirstTickDisposition::RetiredAtCapacity(_)
        ));
        assert!(!runtime.has_exact_order(SECOND_WORKER));
        assert_eq!(runtime.exact_site_for_worker(SECOND_WORKER), None);
        assert_eq!(runtime.site(FARM).unwrap().gather_down, WORKER.o);
        assert_eq!(runtime.exact_active_workers(FARM), Ok(Some(1)));
    }
}
