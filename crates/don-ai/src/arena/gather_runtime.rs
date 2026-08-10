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
use don_sim::systems::collision::{CollCheck, CollUnits};
use don_sim::systems::containment::{NearbyUnitType, OrderedCollision};
use don_sim::systems::economy::{
    self, CapGates, DoGatherContext, EconRules, GatherInputs, LeaderEcon, Payout, NUM_RESOURCES,
};
use don_sim::systems::gather_lifecycle::{
    attached_ordinary_order_state, camp_mine_building_approach, farm_first_gather_tick,
    AuthoritativeOrdinaryGatherGeometry, CampMineApproachOutcome, CampMineDisposition,
    FarmFirstTickDisposition, FarmFirstTickOutcome, OrdinaryGatherKind,
    OrdinaryGatherLifecycleError, OrdinaryGatherTarget,
};
use don_sim::systems::gathering::{
    self, AttachResult, AuthoritativeGatherTerrain, GatherAssignment, GatherCapacityBonuses,
    GatherCapacityRules, GatherMiningList, GatherRefresh, GatherRefreshError, GatherRetirement,
    GatherSite, GatherTerrainDiscovery, GatherTerrainError, GatherTerrainKind,
    GatherTerrainRequest, GatherTile, GatherWorker, MineGatherCapacityRequest, NonFlatGatherState,
    NonFlatTilePreparation, WoodGatherCapacityRequest,
};
use don_sim::systems::leaders::NUM_LEADER_SLOTS;
use don_sim::systems::map_terrain::{Coord, World};

use super::map::RetainedGatherTerrainSources;
use super::types::TypeRow;

/// Candidate bit read by `Unit::do_non_flat_gather` at `0x005F0170` before the exact
/// `has_gather_access(tile, owner, 1, 0)` call. It is not an Arena terrain label.
#[allow(dead_code)]
const NON_FLAT_GATHER_RESOURCE_BIT: u16 = 0x4000;

/// Retail object identity is the pair `(who,o)`.  Arena entity slots never recycle during
/// one world lifetime; registration rejects any attempted reuse so the narrower adapter
/// identity cannot silently alias a new object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct GatherObjectKey {
    pub owner: u8,
    pub o: i16,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GatherCapacityAuthority {
    /// `BuildTypeData::max_gatherers`'s measured flat-Farm arm is the literal one.
    FlatFarmOne,
    /// The signed byte came from the source-backed Woodcutter/Mine evaluator and its
    /// synchronized ordered MiningList.
    EvaluatedRetailTerrain,
    /// The non-retail Arena generator has no installed LandData/resource-object source.
    MissingRetailTerrainSource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SiteState {
    kind: OrdinaryGatherKind,
    capacity: GatherCapacityAuthority,
    site: GatherSite,
    mining: GatherMiningList,
    /// Live type and object geometry consumed by the external
    /// `BuildTypeData::calc_gather` evaluator.  Capacity alone is not sufficient.
    payout_source: Option<AuthoritativeGatherPayoutSource>,
    /// Output of the still-separate authoritative `BuildTypeData::calc_gather` payout
    /// evaluator.  Capacity does not imply resource kind or per-worker yield.
    per_worker_gross: Option<[i32; NUM_RESOURCES]>,
}

/// The source-backed building facts which identify one invocation of retail's unresolved
/// `BuildTypeData::calc_gather` evaluator.  The remaining player/type/world predicates are
/// intentionally owned by [`GatherPerWorkerEvaluator`], not guessed in Arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AuthoritativeGatherPayoutSource {
    pub site_type: AuthoritativeGatherSiteType,
    pub placement: AuthoritativeGatherSitePlacement,
}

/// Exact Arena-to-host boundary for the per-worker six-slot building evaluator.
///
/// The request exposes the retained live type, object coordinates and ordered MiningList.
/// An implementation must execute the complete `BuildTypeData::calc_gather` path, including
/// resource choice and player/terrain modifiers.  Returning `PEASANT_RATE` in a slot chosen
/// from [`OrdinaryGatherKind`] is not an implementation of this trait's contract.
#[allow(dead_code)]
pub(crate) struct GatherPerWorkerEvaluationRequest<'a> {
    pub site_key: GatherObjectKey,
    pub source: AuthoritativeGatherPayoutSource,
    pub active_workers: i32,
    pub authoritative_capacity: i32,
    pub mining: &'a GatherMiningList,
}

pub(crate) trait GatherPerWorkerEvaluator {
    type Error;

    fn evaluate(
        &mut self,
        request: GatherPerWorkerEvaluationRequest<'_>,
    ) -> Result<[i32; NUM_RESOURCES], Self::Error>;
}

impl<E, F> GatherPerWorkerEvaluator for F
where
    F: FnMut(GatherPerWorkerEvaluationRequest<'_>) -> Result<[i32; NUM_RESOURCES], E>,
{
    type Error = E;

    fn evaluate(
        &mut self,
        request: GatherPerWorkerEvaluationRequest<'_>,
    ) -> Result<[i32; NUM_RESOURCES], Self::Error> {
        self(request)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GatherPerWorkerEvaluationReceipt {
    pub site_key: GatherObjectKey,
    pub type_index: i32,
    pub active_workers: i32,
    pub authoritative_capacity: i32,
    pub evaluated: [i32; NUM_RESOURCES],
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum GatherPerWorkerEvaluationError<E> {
    Runtime(GatherRuntimeError),
    Evaluator(E),
}

/// Persistent inputs and checksum-visible economy state for one exact Leader payout lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GatherLeaderPayoutState {
    pub leader_slot: i32,
    pub econ: LeaderEcon,
    pub last_calc_frame: i32,
    pub dirty: bool,
}

/// Receipt for the complete recovered Leader economy transaction.
///
/// The checksum values cover `LeaderEcon::image()`'s recovered 244-byte economy block.
/// They are deliberately named `modelled`: `don-sim` documents unidentified bytes in the
/// full retail Leader walk, so these must not be advertised as the retail channel-8 hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GatherOwnerPayoutReceipt {
    pub evaluated_site_gross: [i32; NUM_RESOURCES],
    pub composed_object_income: [i32; NUM_RESOURCES],
    pub gross_recomputed: bool,
    pub payouts: [Payout; NUM_RESOURCES],
    pub stockpile: [i32; NUM_RESOURCES],
    pub accumulators: [i32; NUM_RESOURCES],
    pub commerce_cap: [i32; NUM_RESOURCES],
    pub expenses: [i32; NUM_RESOURCES],
    pub modelled_econ_adler32_before: u32,
    pub modelled_econ_adler32_after: u32,
}

/// The exact supported ordinary gathering rows in the shipped live building table.
///
/// Provenance: `schema/live/live-tables-building.tsv` records Farm 417, Woodcutter 418,
/// Mine 419 with their live `TypeIndex`, footprint and build flags.  Radii remain sourced
/// from the installed `rules.xml` through [`super::map::Spatial`].
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AuthoritativeGatherSiteType {
    pub type_index: i32,
    pub kind: OrdinaryGatherKind,
    pub x_size: i32,
    pub y_size: i32,
    pub gather_radius: Option<i32>,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GatherTypeSourceError {
    UnsupportedTypeIndex(i32),
    NotGatherBuilding(i32),
    InvalidFootprint(i32, i32),
    InvalidGatherRadius(i32),
    InvalidInstalledSpatialRules,
}

#[allow(dead_code)]
impl AuthoritativeGatherSiteType {
    pub(crate) fn from_live_row(
        row: &TypeRow,
        sources: &RetainedGatherTerrainSources,
    ) -> Result<Self, GatherTypeSourceError> {
        let spatial = super::map::Spatial::from_rules_xml(sources.rules_xml())
            .map_err(|_| GatherTypeSourceError::InvalidInstalledSpatialRules)?;
        let (kind, gather_radius) = match row.id {
            417 => (OrdinaryGatherKind::Farm, None),
            418 => (OrdinaryGatherKind::Camp, Some(spatial.woodcutter_radius)),
            419 => (OrdinaryGatherKind::Mine, Some(spatial.mine_radius)),
            id => return Err(GatherTypeSourceError::UnsupportedTypeIndex(id)),
        };
        if !row.is_gatherer() {
            return Err(GatherTypeSourceError::NotGatherBuilding(row.id));
        }
        if row.x_size <= 0 || row.y_size <= 0 {
            return Err(GatherTypeSourceError::InvalidFootprint(
                row.x_size, row.y_size,
            ));
        }
        if gather_radius.is_some_and(|radius| radius < 0) {
            return Err(GatherTypeSourceError::InvalidGatherRadius(
                gather_radius.expect("checked some"),
            ));
        }
        Ok(Self {
            type_index: row.id,
            kind,
            x_size: row.x_size,
            y_size: row.y_size,
            gather_radius,
        })
    }
}

/// Exact placement/owner fields read from the live building instance.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AuthoritativeGatherSitePlacement {
    pub centre_x: Coord,
    pub centre_y: Coord,
    pub corner_tx: i32,
    pub corner_ty: i32,
    pub region: i16,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GatherTerrainRefreshReceipt {
    pub discovery: GatherTerrainDiscovery,
    pub refresh: GatherRefresh,
    pub authoritative_capacity: i32,
    pub mining_header: (i32, i32, i16, u8),
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
    MissingAuthoritativePayoutSource(GatherObjectKey),
    MissingAuthoritativePayout(GatherObjectKey),
    LeaderSlotOutOfRange(i32),
    ExistingAuthoritativeLeaderEconomy(u8),
    MissingAuthoritativeLeaderEconomy(u8),
    WrongAuthoritativeType {
        site: GatherObjectKey,
        expected: OrdinaryGatherKind,
        actual: OrdinaryGatherKind,
    },
    FlatSiteHasNoTerrainRefresh(GatherObjectKey),
    ExistingExactOrder(GatherObjectKey),
    NoExactOrder(GatherObjectKey),
    FarmLifecycle(OrdinaryGatherLifecycleError<Infallible, Infallible>),
    TerrainMaterialization(don_sim::systems::gather_terrain::GatherTerrainMaterializationError),
    Terrain(GatherTerrainError),
    Refresh(GatherRefreshError),
    Retirement(&'static str),
}

#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CampMineRuntimeError<G, O> {
    Runtime(GatherRuntimeError),
    Lifecycle(OrdinaryGatherLifecycleError<G, O>),
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
    owner_economies: BTreeMap<u8, GatherLeaderPayoutState>,
}

#[allow(dead_code)]
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
                mining: GatherMiningList::default(),
                payout_source: None,
                per_worker_gross: None,
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

    /// Capture the target generation into a persistent GatherOrder and stage the exact
    /// per-worker identity consumed by `Build::add_gatherer`. No chain link is changed.
    pub(crate) fn stage_exact_order(
        &mut self,
        worker_key: GatherObjectKey,
        site_key: GatherObjectKey,
    ) -> Result<(), GatherRuntimeError> {
        if self.orders.contains_key(&worker_key) {
            return Err(GatherRuntimeError::ExistingExactOrder(worker_key));
        }
        let site_state = self
            .sites
            .get(&site_key)
            .ok_or(GatherRuntimeError::MissingSite(site_key))?;
        let worker_pos = self
            .worker_pos(worker_key)
            .ok_or(GatherRuntimeError::MissingWorker(worker_key))?;
        self.workers[worker_pos].assignment = Some(GatherAssignment {
            target_owner: i32::from(site_key.owner),
            target_build: i32::from(site_key.o),
            target_uid: site_state.site.uid,
            been_there: false,
            inside_target: None,
        });
        self.orders
            .insert(worker_key, attached_ordinary_order_state(site_state.kind));
        Ok(())
    }

    /// Execute only the recovered owner-local occupancy-chain attachment. Collision
    /// release, path/nearby search and phase progression remain their exact lifecycle
    /// caller's responsibility.
    pub(crate) fn attach_staged_worker(
        &mut self,
        worker_key: GatherObjectKey,
        site_key: GatherObjectKey,
    ) -> Result<AttachResult, GatherRuntimeError> {
        if !self.orders.contains_key(&worker_key) {
            return Err(GatherRuntimeError::NoExactOrder(worker_key));
        }
        let mut trial_site = self
            .sites
            .get(&site_key)
            .ok_or(GatherRuntimeError::MissingSite(site_key))?
            .site;
        let mut trial_workers = self.workers.clone();
        let result = gathering::attach_worker(&mut trial_site, &mut trial_workers, worker_key.o);
        if result == AttachResult::Invalid {
            return Err(GatherRuntimeError::Retirement(
                "staged Gather identity could not attach to the site",
            ));
        }
        self.sites.get_mut(&site_key).expect("validated site").site = trial_site;
        self.workers = trial_workers;
        Ok(result)
    }

    /// Persist the shared exact Camp/Mine building-approach and arrival transaction.
    /// Retail may attach before a later geometry/search error, so the local chain/order
    /// clones are committed on both success and error; external world/collision hosts keep
    /// the mutation semantics of the shared primitive itself.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn begin_camp_mine<U, O, G>(
        &mut self,
        world: &mut World,
        collcheck: &mut CollCheck,
        bitmap_units: &U,
        ordered: &mut O,
        geometry: &G,
        worker_key: GatherObjectKey,
        site_key: GatherObjectKey,
        target: OrdinaryGatherTarget,
        unit_type: NearbyUnitType,
        object_masks: u32,
    ) -> Result<CampMineApproachOutcome, CampMineRuntimeError<G::Error, O::Error>>
    where
        U: CollUnits,
        O: OrderedCollision,
        G: AuthoritativeOrdinaryGatherGeometry,
    {
        let mut trial_site = self
            .sites
            .get(&site_key)
            .ok_or(GatherRuntimeError::MissingSite(site_key))
            .map_err(CampMineRuntimeError::Runtime)?
            .site;
        let _worker_pos = self
            .worker_pos(worker_key)
            .ok_or(GatherRuntimeError::MissingWorker(worker_key))
            .map_err(CampMineRuntimeError::Runtime)?;
        let mut trial_workers = self.workers.clone();
        let mut trial_order = *self
            .orders
            .get(&worker_key)
            .ok_or(GatherRuntimeError::NoExactOrder(worker_key))
            .map_err(CampMineRuntimeError::Runtime)?;

        let result = camp_mine_building_approach(
            world,
            collcheck,
            bitmap_units,
            ordered,
            geometry,
            &mut trial_site,
            &mut trial_workers,
            worker_key.o,
            &mut trial_order,
            target,
            unit_type,
            object_masks,
        );
        self.sites.get_mut(&site_key).expect("validated site").site = trial_site;
        self.workers = trial_workers;
        self.orders.insert(worker_key, trial_order);
        match result {
            Ok(outcome) => {
                if matches!(
                    outcome.disposition,
                    CampMineDisposition::RetiredFarBlocked(_)
                        | CampMineDisposition::RetiredAtCapacity(_)
                ) {
                    self.orders.remove(&worker_key);
                }
                if outcome.leader_economy_dirty {
                    self.mark_owner_economy_dirty_if_present(site_key.owner);
                }
                Ok(outcome)
            }
            Err(error) => Err(CampMineRuntimeError::Lifecycle(error)),
        }
    }

    /// Execute the recovered Camp/Mine source discovery, capacity reduction, TData claim,
    /// MiningList disorder and signed-byte capacity store as one transaction.
    ///
    /// The live type row fixes kind and footprint; installed rules fix radius and capacity
    /// constants; retained map sources fix LandData and Mountain/Cliff identity.  A stale
    /// or incomplete input leaves runtime, world and RNG untouched.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn refresh_non_flat_site<D>(
        &mut self,
        sources: &RetainedGatherTerrainSources,
        world: &mut World,
        diplomacy: &D,
        site_key: GatherObjectKey,
        site_type: AuthoritativeGatherSiteType,
        placement: AuthoritativeGatherSitePlacement,
        bonuses: GatherCapacityBonuses,
        rng: &mut Random,
    ) -> Result<GatherTerrainRefreshReceipt, GatherRuntimeError>
    where
        D: don_sim::systems::gather_terrain::GatherTerrainDiplomacy + ?Sized,
    {
        let mut trial_state = self
            .sites
            .get(&site_key)
            .ok_or(GatherRuntimeError::MissingSite(site_key))?
            .clone();
        if trial_state.kind != site_type.kind {
            return Err(GatherRuntimeError::WrongAuthoritativeType {
                site: site_key,
                expected: trial_state.kind,
                actual: site_type.kind,
            });
        }
        let (terrain_kind, radius) = match site_type.kind {
            OrdinaryGatherKind::Farm => {
                return Err(GatherRuntimeError::FlatSiteHasNoTerrainRefresh(site_key));
            }
            OrdinaryGatherKind::Camp => (
                GatherTerrainKind::Forest,
                site_type
                    .gather_radius
                    .ok_or(GatherRuntimeError::FlatSiteHasNoTerrainRefresh(site_key))?,
            ),
            OrdinaryGatherKind::Mine => (
                GatherTerrainKind::Mine,
                site_type
                    .gather_radius
                    .ok_or(GatherRuntimeError::FlatSiteHasNoTerrainRefresh(site_key))?,
            ),
        };

        let mut trial_world = world.clone();
        let mut trial_rng = *rng;
        let previous_len = trial_state.mining.len();
        let host = sources
            .executable_host(&trial_world, diplomacy)
            .map_err(GatherRuntimeError::TerrainMaterialization)?;
        let discovery = gathering::discover_gather_terrain(
            &host,
            &mut trial_state.mining,
            GatherTerrainRequest {
                kind: terrain_kind,
                site_x: placement.centre_x,
                site_y: placement.centre_y,
                site_owner: i32::from(site_key.owner),
                site_region: placement.region,
                gather_radius: radius,
            },
        )
        .map_err(GatherRuntimeError::Terrain)?;
        let rules: GatherCapacityRules = sources.capacity_rules();
        let capacity = match site_type.kind {
            OrdinaryGatherKind::Camp => gathering::woodcutter_gather_capacity(
                &host,
                &trial_state.mining,
                WoodGatherCapacityRequest {
                    site_tx: placement.corner_tx,
                    site_ty: placement.corner_ty,
                    x_size: site_type.x_size,
                    y_size: site_type.y_size,
                    owner: i32::from(site_key.owner),
                    bonuses,
                },
                rules,
            ),
            OrdinaryGatherKind::Mine => gathering::mine_gather_capacity(
                &host,
                &trial_state.mining,
                MineGatherCapacityRequest {
                    owner: i32::from(site_key.owner),
                    bonuses,
                },
                rules,
            ),
            OrdinaryGatherKind::Farm => unreachable!("flat arm returned above"),
        }
        .map_err(GatherRuntimeError::Terrain)?;
        drop(host);
        let refresh = gathering::finish_gather_tile_refresh_with_capacity(
            &mut trial_world,
            &mut trial_state.mining,
            previous_len,
            &mut trial_rng,
            &mut trial_state.site,
            capacity,
        )
        .map_err(GatherRuntimeError::Refresh)?;
        trial_state.capacity = GatherCapacityAuthority::EvaluatedRetailTerrain;
        let payout_source = AuthoritativeGatherPayoutSource {
            site_type,
            placement,
        };
        trial_state.payout_source = Some(payout_source);
        // MiningList contents are evaluator input even when type/placement are unchanged.
        trial_state.per_worker_gross = None;
        let receipt = GatherTerrainRefreshReceipt {
            discovery,
            refresh,
            authoritative_capacity: trial_state.site.max_gatherers(),
            mining_header: trial_state.mining.array_header(),
        };

        self.sites.insert(site_key, trial_state);
        self.mark_owner_economy_dirty_if_present(site_key.owner);
        *world = trial_world;
        *rng = trial_rng;
        Ok(receipt)
    }

    /// Apply `Build::verify_gather_tiles` against the same synchronized territory and
    /// diplomacy source used for capacity.  Every fallible read is completed before the
    /// first reservation bit is cleared.
    pub(crate) fn verify_non_flat_site<D>(
        &mut self,
        sources: &RetainedGatherTerrainSources,
        world: &mut World,
        diplomacy: &D,
        site_key: GatherObjectKey,
    ) -> Result<usize, GatherRuntimeError>
    where
        D: don_sim::systems::gather_terrain::GatherTerrainDiplomacy + ?Sized,
    {
        let mut trial_state = self
            .sites
            .get(&site_key)
            .ok_or(GatherRuntimeError::MissingSite(site_key))?
            .clone();
        if trial_state.kind == OrdinaryGatherKind::Farm {
            return Err(GatherRuntimeError::FlatSiteHasNoTerrainRefresh(site_key));
        }
        let mut trial_world = world.clone();
        let host = sources
            .executable_host(&trial_world, diplomacy)
            .map_err(GatherRuntimeError::TerrainMaterialization)?;
        let mut valid = Vec::with_capacity(trial_state.mining.len());
        for &tile in trial_state.mining.tiles() {
            let wx = tile.tx >> 2;
            let wy = tile.ty >> 2;
            let territory_owner =
                host.territory_owner(wx, wy)
                    .ok_or(GatherRuntimeError::Terrain(
                        GatherTerrainError::MissingWorldCell(wx, wy),
                    ))?;
            let usable = if territory_owner < 0 || territory_owner == i32::from(site_key.owner) {
                true
            } else {
                host.is_allied(i32::from(site_key.owner), territory_owner)
                    .ok_or(GatherRuntimeError::Terrain(
                        GatherTerrainError::MissingDiplomacy(
                            i32::from(site_key.owner),
                            territory_owner,
                        ),
                    ))?
            };
            valid.push((tile, usable));
        }
        drop(host);
        let removed =
            gathering::verify_gather_tiles(&mut trial_world, &mut trial_state.mining, |tile| {
                valid
                    .iter()
                    .find_map(|(candidate, usable)| (*candidate == tile).then_some(*usable))
                    .unwrap_or(false)
            })
            .map_err(GatherRuntimeError::Refresh)?;
        if removed != 0 {
            trial_state.per_worker_gross = None;
        }
        self.sites.insert(site_key, trial_state);
        if removed != 0 {
            self.mark_owner_economy_dirty_if_present(site_key.owner);
        }
        *world = trial_world;
        Ok(removed)
    }

    /// Release the site's exact TData claims and clear its MiningList at `Build::close`.
    /// Capacity and allocation history intentionally survive.
    pub(crate) fn close_non_flat_site(
        &mut self,
        world: &mut World,
        site_key: GatherObjectKey,
    ) -> Result<usize, GatherRuntimeError> {
        let mut trial_state = self
            .sites
            .get(&site_key)
            .ok_or(GatherRuntimeError::MissingSite(site_key))?
            .clone();
        if trial_state.kind == OrdinaryGatherKind::Farm {
            return Err(GatherRuntimeError::FlatSiteHasNoTerrainRefresh(site_key));
        }
        let mut trial_world = world.clone();
        let released = gathering::close_gather_tiles(&mut trial_world, &mut trial_state.mining)
            .map_err(GatherRuntimeError::Refresh)?;
        if released != 0 {
            trial_state.per_worker_gross = None;
        }
        self.sites.insert(site_key, trial_state);
        if released != 0 {
            self.mark_owner_economy_dirty_if_present(site_key.owner);
        }
        *world = trial_world;
        Ok(released)
    }

    /// Execute the exact checksum-visible non-flat tile selection/move-to-back boundary.
    /// All terrain/diplomacy reads are preflighted before the order, worker, list or RNG
    /// changes; animation/doober callbacks therefore run only for an admitted transaction.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_non_flat_site_tile<D, SetDefaultAnimation, RemoveDoober>(
        &mut self,
        sources: &RetainedGatherTerrainSources,
        world: &World,
        diplomacy: &D,
        worker_key: GatherObjectKey,
        site_key: GatherObjectKey,
        site_origin: GatherTile,
        rng: &mut Random,
        set_default_animation: SetDefaultAnimation,
        remove_doober: RemoveDoober,
    ) -> Result<NonFlatTilePreparation, GatherRuntimeError>
    where
        D: don_sim::systems::gather_terrain::GatherTerrainDiplomacy + ?Sized,
        SetDefaultAnimation: FnOnce(),
        RemoveDoober: FnMut(i16),
    {
        let mut trial_state = self
            .sites
            .get(&site_key)
            .ok_or(GatherRuntimeError::MissingSite(site_key))?
            .clone();
        if trial_state.kind == OrdinaryGatherKind::Farm {
            return Err(GatherRuntimeError::FlatSiteHasNoTerrainRefresh(site_key));
        }
        let worker_pos = self
            .worker_pos(worker_key)
            .ok_or(GatherRuntimeError::MissingWorker(worker_key))?;
        let mut trial_workers = self.workers.clone();
        let mut trial_order = *self
            .orders
            .get(&worker_key)
            .ok_or(GatherRuntimeError::NoExactOrder(worker_key))?;
        let mut trial_rng = *rng;
        let host = sources
            .executable_host(world, diplomacy)
            .map_err(GatherRuntimeError::TerrainMaterialization)?;

        let access = |tile: GatherTile| {
            gathering::has_non_flat_gather_access(&host, tile, i32::from(worker_key.owner))
                .map_err(GatherRuntimeError::Terrain)
        };
        let existing_tile = trial_order.tile();
        let existing_access = if existing_tile.tx >= 0 && existing_tile.ty >= 0 {
            access(existing_tile)?
        } else {
            false
        };
        let mut candidates = Vec::with_capacity(trial_state.mining.len());
        for &tile in trial_state.mining.tiles() {
            let mask = host.tile_mask(tile).ok_or(GatherRuntimeError::Terrain(
                GatherTerrainError::MissingTile(tile),
            ))?;
            candidates.push((
                tile,
                mask & NON_FLAT_GATHER_RESOURCE_BIT != 0 && access(tile)?,
            ));
        }
        drop(host);

        let preparation = gathering::prepare_non_flat_tile(
            &mut trial_state.mining,
            &mut trial_workers[worker_pos],
            &mut trial_order,
            site_origin,
            sources.capacity_rules().mountain_size_thresholds[0],
            trial_state.kind == OrdinaryGatherKind::Camp,
            &mut trial_rng,
            set_default_animation,
            remove_doober,
            |tile| tile == existing_tile && existing_access,
            |tile| {
                candidates
                    .iter()
                    .find_map(|(candidate, eligible)| (*candidate == tile).then_some(*eligible))
                    .unwrap_or(false)
            },
        );
        self.sites.insert(site_key, trial_state);
        self.workers = trial_workers;
        self.orders.insert(worker_key, trial_order);
        *rng = trial_rng;
        Ok(preparation)
    }

    /// Bind the retained live type and building placement used by the payout evaluator.
    /// Rebinding clears any result evaluated against the previous descriptor.
    pub(crate) fn bind_authoritative_payout_source(
        &mut self,
        site_key: GatherObjectKey,
        source: AuthoritativeGatherPayoutSource,
    ) -> Result<(), GatherRuntimeError> {
        let state = self
            .sites
            .get_mut(&site_key)
            .ok_or(GatherRuntimeError::MissingSite(site_key))?;
        if state.kind != source.site_type.kind {
            return Err(GatherRuntimeError::WrongAuthoritativeType {
                site: site_key,
                expected: state.kind,
                actual: source.site_type.kind,
            });
        }
        let changed = state.payout_source != Some(source);
        state.payout_source = Some(source);
        if changed {
            state.per_worker_gross = None;
            self.mark_owner_economy_dirty_if_present(site_key.owner);
        }
        Ok(())
    }

    /// Execute the mandatory external `BuildTypeData::calc_gather` boundary and retain
    /// only its six-slot per-worker output.  Type, placement, capacity, active occupancy
    /// and MiningList are read from the same persistent site.  Evaluator failure is
    /// transactional: the previous result and Leader dirty bit are unchanged.
    pub(crate) fn evaluate_authoritative_per_worker<E: GatherPerWorkerEvaluator>(
        &mut self,
        site_key: GatherObjectKey,
        evaluator: &mut E,
    ) -> Result<GatherPerWorkerEvaluationReceipt, GatherPerWorkerEvaluationError<E::Error>> {
        let state = self
            .sites
            .get(&site_key)
            .ok_or(GatherRuntimeError::MissingSite(site_key))
            .map_err(GatherPerWorkerEvaluationError::Runtime)?;
        if state.capacity == GatherCapacityAuthority::MissingRetailTerrainSource {
            return Err(GatherPerWorkerEvaluationError::Runtime(
                GatherRuntimeError::MissingAuthoritativeCapacity(site_key),
            ));
        }
        let source = state.payout_source.ok_or_else(|| {
            GatherPerWorkerEvaluationError::Runtime(
                GatherRuntimeError::MissingAuthoritativePayoutSource(site_key),
            )
        })?;
        let active_workers = gathering::num_gatherers(
            &state.site,
            &self.workers,
            gathering::GatherCount::Active,
            0,
        )
        .map_err(GatherRuntimeError::Retirement)
        .map_err(GatherPerWorkerEvaluationError::Runtime)?;
        let authoritative_capacity = state.site.max_gatherers();
        let evaluated = evaluator
            .evaluate(GatherPerWorkerEvaluationRequest {
                site_key,
                source,
                active_workers,
                authoritative_capacity,
                mining: &state.mining,
            })
            .map_err(GatherPerWorkerEvaluationError::Evaluator)?;

        let changed = self
            .sites
            .get(&site_key)
            .expect("site borrowed above")
            .per_worker_gross
            != Some(evaluated);
        self.sites
            .get_mut(&site_key)
            .expect("site borrowed above")
            .per_worker_gross = Some(evaluated);
        if changed {
            self.mark_owner_economy_dirty_if_present(site_key.owner);
        }
        Ok(GatherPerWorkerEvaluationReceipt {
            site_key,
            type_index: source.site_type.type_index,
            active_workers,
            authoritative_capacity,
            evaluated,
        })
    }

    /// Compose every linked site's active occupancy into one owner-level gross vector.
    /// Missing evaluator output is an error even though capacity and occupancy exist.
    pub(crate) fn authoritative_owner_gross(
        &self,
        owner: u8,
    ) -> Result<[i32; NUM_RESOURCES], GatherRuntimeError> {
        let mut gross = [0i32; NUM_RESOURCES];
        for (key, state) in self.sites.iter().filter(|(key, _)| key.owner == owner) {
            if state.site.gather_down < 0 {
                continue;
            }
            let per_worker = state
                .per_worker_gross
                .ok_or(GatherRuntimeError::MissingAuthoritativePayout(*key))?;
            let active = gathering::num_gatherers(
                &state.site,
                &self.workers,
                gathering::GatherCount::Active,
                0,
            )
            .map_err(GatherRuntimeError::Retirement)?;
            let site = gathering::site_gross(per_worker, active, state.site.gather_max);
            for (out, value) in gross.iter_mut().zip(site) {
                *out = out.wrapping_add(value);
            }
        }
        Ok(gross)
    }

    /// Install the real Leader economy state which owns caps, expenses, carries,
    /// stockpiles and their recovered checksum image.  Object ownership is not assumed to
    /// equal the Leader slot; the authoritative host supplies and validates that mapping.
    pub(crate) fn bind_authoritative_leader_economy(
        &mut self,
        owner: u8,
        state: GatherLeaderPayoutState,
    ) -> Result<(), GatherRuntimeError> {
        if !(0..NUM_LEADER_SLOTS as i32).contains(&state.leader_slot) {
            return Err(GatherRuntimeError::LeaderSlotOutOfRange(state.leader_slot));
        }
        if self.owner_economies.contains_key(&owner) {
            return Err(GatherRuntimeError::ExistingAuthoritativeLeaderEconomy(
                owner,
            ));
        }
        self.owner_economies.insert(owner, state);
        Ok(())
    }

    pub(crate) fn authoritative_leader_economy(
        &self,
        owner: u8,
    ) -> Result<GatherLeaderPayoutState, GatherRuntimeError> {
        self.owner_economies
            .get(&owner)
            .copied()
            .ok_or(GatherRuntimeError::MissingAuthoritativeLeaderEconomy(owner))
    }

    pub(crate) fn mark_owner_economy_dirty(&mut self, owner: u8) -> Result<(), GatherRuntimeError> {
        let state = self
            .owner_economies
            .get_mut(&owner)
            .ok_or(GatherRuntimeError::MissingAuthoritativeLeaderEconomy(owner))?;
        state.dirty = true;
        Ok(())
    }

    fn mark_owner_economy_dirty_if_present(&mut self, owner: u8) {
        if let Some(state) = self.owner_economies.get_mut(&owner) {
            state.dirty = true;
        }
    }

    /// Compose exact site gross into the caller's authoritative non-site object income,
    /// then execute retail's recovered `Leader::gather` economy sequence as one local
    /// transaction: scheduled gross recomposition, expense reset, caps, payout, carry,
    /// stockpile and the obfuscated economy checksum image.
    ///
    /// `non_site_inputs.object_income` must contain every retail object-graph contribution
    /// except the sites owned here.  This explicit split prevents double-crediting.
    pub(crate) fn execute_authoritative_owner_payout(
        &mut self,
        owner: u8,
        rules: &EconRules,
        frame: i32,
        non_site_inputs: &GatherInputs,
        cap_gates: &CapGates,
        context: &DoGatherContext,
    ) -> Result<GatherOwnerPayoutReceipt, GatherRuntimeError> {
        // Complete every fallible read before touching checksum-visible Leader state.
        let evaluated_site_gross = self.authoritative_owner_gross(owner)?;
        let mut state = self.authoritative_leader_economy(owner)?;
        let mut inputs = non_site_inputs.clone();
        for (income, site) in inputs.object_income.iter_mut().zip(evaluated_site_gross) {
            *income = income.wrapping_add(site);
        }

        let modelled_econ_adler32_before = state.econ.adler32();
        let gross_recomputed =
            economy::calc_gather_due(frame, state.leader_slot, state.last_calc_frame, state.dirty);
        let payouts = economy::leader_gather(
            rules,
            &mut state.econ,
            frame,
            state.leader_slot,
            &mut state.last_calc_frame,
            &mut state.dirty,
            &inputs,
            cap_gates,
            context,
        );
        let receipt = GatherOwnerPayoutReceipt {
            evaluated_site_gross,
            composed_object_income: inputs.object_income,
            gross_recomputed,
            payouts,
            stockpile: state.econ.stockpile,
            accumulators: state.econ.accumulator,
            commerce_cap: state.econ.commerce_cap,
            expenses: state.econ.expense,
            modelled_econ_adler32_before,
            modelled_econ_adler32_after: state.econ.adler32(),
        };
        self.owner_economies.insert(owner, state);
        Ok(receipt)
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
        let site_state = self
            .sites
            .get(&site_key)
            .ok_or(GatherRuntimeError::MissingSite(site_key))?
            .clone();
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
        if outcome.leader_economy_dirty {
            self.mark_owner_economy_dirty_if_present(site_key.owner);
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
        if retirement.leader_economy_dirty {
            self.mark_owner_economy_dirty_if_present(site_key.owner);
        }
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

    /// Return the signed-byte capacity owned by the authoritative site record.
    /// `None` is the explicit non-retail terrain-source state, never an Arena-derived cap.
    pub(crate) fn authoritative_capacity(
        &self,
        site_key: GatherObjectKey,
    ) -> Result<Option<i32>, GatherRuntimeError> {
        let state = self
            .sites
            .get(&site_key)
            .ok_or(GatherRuntimeError::MissingSite(site_key))?;
        if state.capacity == GatherCapacityAuthority::MissingRetailTerrainSource {
            Ok(None)
        } else {
            Ok(Some(state.site.max_gatherers()))
        }
    }

    /// Compose one site's exact active occupancy with its retained six-slot per-worker
    /// evaluator result. `None` means this site has not crossed the authoritative payout
    /// boundary; callers must keep that case visibly separate from a zero gross result.
    pub(crate) fn authoritative_site_gross(
        &self,
        site_key: GatherObjectKey,
    ) -> Result<Option<[i32; NUM_RESOURCES]>, GatherRuntimeError> {
        let state = self
            .sites
            .get(&site_key)
            .ok_or(GatherRuntimeError::MissingSite(site_key))?;
        let Some(per_worker) = state.per_worker_gross else {
            return Ok(None);
        };
        let active = gathering::num_gatherers(
            &state.site,
            &self.workers,
            gathering::GatherCount::Active,
            0,
        )
        .map_err(GatherRuntimeError::Retirement)?;
        Ok(Some(gathering::site_gross(
            per_worker,
            active,
            state.site.gather_max,
        )))
    }

    #[cfg(test)]
    fn site(&self, key: GatherObjectKey) -> Option<GatherSite> {
        self.sites.get(&key).map(|state| state.site)
    }

    #[cfg(test)]
    fn mining(&self, key: GatherObjectKey) -> Option<&GatherMiningList> {
        self.sites.get(&key).map(|state| &state.mining)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use don_sim::systems::collision::{place, CollGuy, UnitRow, UnitTable};
    use don_sim::systems::containment::OrderedCollisionQuery;
    use don_sim::systems::gather_terrain::{
        GatherTerrainSourceStamp, MaterializedMountainObject, SUPPORTED_RULES_XML_SHA256,
    };
    use don_sim::systems::gathering::{
        GatherNearbyPoint, GatherTile, GatherWorldCell, MiningObjectKind, NonFlatTileResult,
    };
    use don_sim::systems::map_terrain::{wflag, Coord, World};

    use crate::arena::map::{Map, MapParams, Spatial};

    const RULES_XML: &[u8] = include_bytes!("../../../../ron-data/rules.xml");

    const WORKER: GatherObjectKey = GatherObjectKey { owner: 1, o: 7 };
    const SECOND_WORKER: GatherObjectKey = GatherObjectKey { owner: 1, o: 8 };
    const FARM: GatherObjectKey = GatherObjectKey { owner: 1, o: 3 };
    const CAMP: GatherObjectKey = GatherObjectKey { owner: 0, o: 4 };
    const CAMP_WORKER: GatherObjectKey = GatherObjectKey { owner: 0, o: 9 };
    const MINE: GatherObjectKey = GatherObjectKey { owner: 0, o: 5 };

    #[derive(Default)]
    struct OrderedHost;

    impl OrderedCollision for OrderedHost {
        type Error = Infallible;

        fn find_ordered_collision(
            &mut self,
            _query: OrderedCollisionQuery,
        ) -> Result<bool, Self::Error> {
            Ok(false)
        }
    }

    struct ArrivedGeometry;

    impl AuthoritativeOrdinaryGatherGeometry for ArrivedGeometry {
        type Error = Infallible;

        fn unit_is_at_gather_target(
            &self,
            _query: don_sim::systems::gather_lifecycle::OrdinaryIsAtQuery,
        ) -> Result<bool, Self::Error> {
            Ok(true)
        }
    }

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

    fn spatial() -> Spatial {
        Spatial {
            city_center_radius: 20,
            city_center_pop_radius: 4,
            city_capture_radius: 10,
            woodcutter_radius: 8,
            mine_radius: 6,
        }
    }

    fn camp_type() -> TypeRow {
        TypeRow {
            kind_building: true,
            id: 418,
            build_flags: 0x40,
            x_size: 2,
            y_size: 2,
            ..TypeRow::default()
        }
    }

    fn mine_type() -> TypeRow {
        TypeRow {
            kind_building: true,
            id: 419,
            build_flags: 0x40,
            x_size: 2,
            y_size: 2,
            ..TypeRow::default()
        }
    }

    fn camp_sources() -> (Map, World) {
        let mut params = MapParams::default();
        params.size = 16;
        params.start_radius = 4;
        params.forest_clusters = 0;
        params.mountain_clusters = 0;
        let mut map = Map::generate(params, spatial());
        map.retain_gather_terrain_sources(
            RULES_XML.to_vec(),
            GatherTerrainSourceStamp {
                installed_rules_sha256: SUPPORTED_RULES_XML_SHA256,
                world_seed: map.seed as i32,
                coherent_generation: true,
            },
            Vec::new(),
            Vec::new(),
        )
        .unwrap();

        let mut world = World::init_default_rules(4, 4);
        world.seed = map.seed as i32;
        world.set_land(0, 1, 0, 0, i32::from(wflag::FOREST), false);
        for ty in 4..8 {
            for tx in 0..4 {
                world.set_tree_at(tx, ty, true);
            }
        }
        world.set_gather_edge(2, 6);
        (map, world)
    }

    fn mine_sources() -> (Map, World) {
        let mut params = MapParams::default();
        params.size = 16;
        params.start_radius = 4;
        params.forest_clusters = 0;
        params.mountain_clusters = 0;
        let mut map = Map::generate(params, spatial());
        let tile = GatherTile { tx: 6, ty: 6 };
        map.retain_gather_terrain_sources(
            RULES_XML.to_vec(),
            GatherTerrainSourceStamp {
                installed_rules_sha256: SUPPORTED_RULES_XML_SHA256,
                world_seed: map.seed as i32,
                coherent_generation: true,
            },
            vec![Some(MaterializedMountainObject {
                search_wcoords: vec![GatherWorldCell { wx: 1, wy: 1 }],
                mining_tcoords: vec![tile],
                solid_wcoords: vec![GatherWorldCell { wx: 1, wy: 1 }],
                mountain_size: 7,
            })],
            Vec::new(),
        )
        .unwrap();
        let mut world = World::init_default_rules(4, 4);
        world.seed = map.seed as i32;
        world.set_mountain_at(tile.tx, tile.ty, true);
        (map, world)
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
    fn live_type_rows_and_shipped_radii_are_the_only_supported_site_type_source() {
        let (map, _world) = camp_sources();
        let sources = map.gather_terrain_sources().unwrap();
        assert_eq!(
            AuthoritativeGatherSiteType::from_live_row(&camp_type(), sources).unwrap(),
            AuthoritativeGatherSiteType {
                type_index: 418,
                kind: OrdinaryGatherKind::Camp,
                x_size: 2,
                y_size: 2,
                gather_radius: Some(8),
            }
        );
        let mut invented = camp_type();
        invented.id = 999;
        assert_eq!(
            AuthoritativeGatherSiteType::from_live_row(&invented, sources),
            Err(GatherTypeSourceError::UnsupportedTypeIndex(999))
        );
        let mut not_gathering = camp_type();
        not_gathering.build_flags = 0;
        assert_eq!(
            AuthoritativeGatherSiteType::from_live_row(&not_gathering, sources),
            Err(GatherTypeSourceError::NotGatherBuilding(418))
        );
    }

    #[test]
    fn source_backed_camp_refresh_owns_capacity_order_and_reservation_transactionally() {
        let (map, mut world) = camp_sources();
        let sources = map.gather_terrain_sources().unwrap();
        let diplomacy = |_: i32, _: i32| Some(false);
        let mut runtime = ArenaGatherRuntime::default();
        runtime
            .register_site(
                CAMP,
                7,
                OrdinaryGatherKind::Camp,
                GatherCapacityAuthority::MissingRetailTerrainSource,
            )
            .unwrap();
        let mut rng = Random::new(0x1234_5678);
        let receipt = runtime
            .refresh_non_flat_site(
                sources,
                &mut world,
                &diplomacy,
                CAMP,
                AuthoritativeGatherSiteType::from_live_row(&camp_type(), sources).unwrap(),
                AuthoritativeGatherSitePlacement {
                    centre_x: Coord(6 * 192),
                    centre_y: Coord(6 * 192),
                    corner_tx: 5,
                    corner_ty: 5,
                    region: 64,
                },
                GatherCapacityBonuses::default(),
                &mut rng,
            )
            .unwrap();
        assert_eq!(receipt.discovery.appended, 16);
        assert_eq!(receipt.authoritative_capacity, 1);
        assert_eq!(receipt.refresh.reserved_tiles, 16);
        assert_eq!(receipt.refresh.move_to_back_steps, 64);
        assert_eq!(receipt.refresh.rng_draws, 64);
        assert_eq!(receipt.mining_header.0, 16);
        assert_eq!(runtime.site(CAMP).unwrap().max_gatherers(), 1);
        assert_eq!(runtime.mining(CAMP).unwrap().len(), 16);
        assert!((0..4).all(|tx| (4..8).all(|ty| world.is_gathered_from(tx, ty))));
        runtime.register_worker(CAMP_WORKER, 0x32).unwrap();
        runtime.stage_exact_order(CAMP_WORKER, CAMP).unwrap();
        assert_eq!(
            runtime.attach_staged_worker(CAMP_WORKER, CAMP),
            Ok(AttachResult::Attached)
        );
        assert_eq!(runtime.site(CAMP).unwrap().gather_down, CAMP_WORKER.o);
        assert_eq!(runtime.exact_site_for_worker(CAMP_WORKER), Some(CAMP));

        let before_runtime = runtime.clone();
        let before_world = world.clone();
        let rng_before = rng;
        world.seed ^= 1;
        let stale_world = world.clone();
        assert!(matches!(
            runtime.refresh_non_flat_site(
                sources,
                &mut world,
                &diplomacy,
                CAMP,
                AuthoritativeGatherSiteType::from_live_row(&camp_type(), sources).unwrap(),
                AuthoritativeGatherSitePlacement {
                    centre_x: Coord(6 * 192),
                    centre_y: Coord(6 * 192),
                    corner_tx: 5,
                    corner_ty: 5,
                    region: 64,
                },
                GatherCapacityBonuses::default(),
                &mut rng,
            ),
            Err(GatherRuntimeError::TerrainMaterialization(_))
        ));
        assert_eq!(runtime, before_runtime);
        assert_eq!(world.checksum_image().0, stale_world.checksum_image().0);
        assert_eq!(rng, rng_before);
        // The valid pre-error world is retained solely to show the failed call did not
        // silently restore or mutate a different source identity.
        assert_ne!(world.checksum_image().0, before_world.checksum_image().0);
    }

    #[test]
    fn camp_arrival_persists_exact_attachment_and_active_occupancy() {
        let (map, mut world) = camp_sources();
        let sources = map.gather_terrain_sources().unwrap();
        let diplomacy = |_: i32, _: i32| Some(false);
        let mut runtime = ArenaGatherRuntime::default();
        runtime
            .register_site(
                CAMP,
                7,
                OrdinaryGatherKind::Camp,
                GatherCapacityAuthority::MissingRetailTerrainSource,
            )
            .unwrap();
        runtime.register_worker(CAMP_WORKER, 0x32).unwrap();
        let mut rng = Random::new(7);
        runtime
            .refresh_non_flat_site(
                sources,
                &mut world,
                &diplomacy,
                CAMP,
                AuthoritativeGatherSiteType::from_live_row(&camp_type(), sources).unwrap(),
                AuthoritativeGatherSitePlacement {
                    centre_x: Coord(6 * 192),
                    centre_y: Coord(6 * 192),
                    corner_tx: 5,
                    corner_ty: 5,
                    region: 64,
                },
                GatherCapacityBonuses::default(),
                &mut rng,
            )
            .unwrap();
        runtime.stage_exact_order(CAMP_WORKER, CAMP).unwrap();

        let mut units = UnitTable::default();
        place(
            &mut world,
            &mut units,
            UnitRow {
                who: i32::from(CAMP_WORKER.owner),
                o: i32::from(CAMP_WORKER.o),
                x: 6 * 192,
                y: 6 * 192,
                domain: 0,
                block_radius: 1,
                on_map: true,
                active: true,
                ..UnitRow::default()
            },
            [CollGuy {
                x: 6 * 192,
                y: 6 * 192,
                block_radius: 1,
            }],
        );
        let mut ordered = OrderedHost;
        let outcome = runtime
            .begin_camp_mine(
                &mut world,
                &mut CollCheck::new(),
                &units,
                &mut ordered,
                &ArrivedGeometry,
                CAMP_WORKER,
                CAMP,
                OrdinaryGatherTarget {
                    kind: OrdinaryGatherKind::Camp,
                    centre: GatherNearbyPoint {
                        x: Coord(6 * 192),
                        y: Coord(6 * 192),
                    },
                    corner: GatherTile { tx: 5, ty: 5 },
                    x_size: 2,
                    y_size: 2,
                    domain: 0,
                    completed: true,
                },
                unit_type(),
                0,
            )
            .unwrap();
        assert_eq!(outcome.disposition, CampMineDisposition::Arrived);
        assert_eq!(outcome.active_workers_after, 1);
        assert_eq!(runtime.site(CAMP).unwrap().gather_down, CAMP_WORKER.o);
        assert_eq!(runtime.exact_active_workers(CAMP), Ok(Some(1)));
        assert!(runtime.has_exact_order(CAMP_WORKER));
    }

    #[test]
    fn source_backed_mine_uses_retained_object_identity_size_and_zero_draw_singleton() {
        let (map, mut world) = mine_sources();
        let sources = map.gather_terrain_sources().unwrap();
        let diplomacy = |_: i32, _: i32| Some(false);
        let mut runtime = ArenaGatherRuntime::default();
        runtime
            .register_site(
                MINE,
                8,
                OrdinaryGatherKind::Mine,
                GatherCapacityAuthority::MissingRetailTerrainSource,
            )
            .unwrap();
        let mut rng = Random::new(0x1234);
        let before = rng;
        let receipt = runtime
            .refresh_non_flat_site(
                sources,
                &mut world,
                &diplomacy,
                MINE,
                AuthoritativeGatherSiteType::from_live_row(&mine_type(), sources).unwrap(),
                AuthoritativeGatherSitePlacement {
                    centre_x: Coord(6 * 192),
                    centre_y: Coord(6 * 192),
                    corner_tx: 5,
                    corner_ty: 5,
                    region: 64,
                },
                GatherCapacityBonuses::default(),
                &mut rng,
            )
            .unwrap();
        assert_eq!(
            receipt.discovery.selected_object,
            Some((MiningObjectKind::Mountain, 0))
        );
        assert_eq!(receipt.discovery.appended, 1);
        assert_eq!(receipt.authoritative_capacity, 3);
        assert_eq!(receipt.refresh.move_to_back_steps, 4);
        assert_eq!(receipt.refresh.rng_draws, 0);
        assert_eq!(rng, before);
        assert_eq!(runtime.mining(MINE).unwrap().mtn, 0);
        assert_eq!(runtime.mining(MINE).unwrap().cliff, -1);
        assert!(world.is_gathered_from(6, 6));
    }

    #[test]
    fn verification_and_close_mutate_only_the_authoritative_mining_list_claims() {
        let (map, mut world) = camp_sources();
        let sources = map.gather_terrain_sources().unwrap();
        let diplomacy = |_: i32, _: i32| Some(false);
        let mut runtime = ArenaGatherRuntime::default();
        runtime
            .register_site(
                CAMP,
                7,
                OrdinaryGatherKind::Camp,
                GatherCapacityAuthority::MissingRetailTerrainSource,
            )
            .unwrap();
        let mut rng = Random::new(7);
        runtime
            .refresh_non_flat_site(
                sources,
                &mut world,
                &diplomacy,
                CAMP,
                AuthoritativeGatherSiteType::from_live_row(&camp_type(), sources).unwrap(),
                AuthoritativeGatherSitePlacement {
                    centre_x: Coord(6 * 192),
                    centre_y: Coord(6 * 192),
                    corner_tx: 5,
                    corner_ty: 5,
                    region: 64,
                },
                GatherCapacityBonuses::default(),
                &mut rng,
            )
            .unwrap();
        world.wdata_mut(0, 1).who = 2;
        assert_eq!(
            runtime
                .verify_non_flat_site(sources, &mut world, &diplomacy, CAMP)
                .unwrap(),
            16
        );
        assert!(runtime.mining(CAMP).unwrap().is_empty());
        assert!((0..4).all(|tx| (4..8).all(|ty| !world.is_gathered_from(tx, ty))));
        assert_eq!(runtime.close_non_flat_site(&mut world, CAMP), Ok(0));
        assert_eq!(runtime.site(CAMP).unwrap().max_gatherers(), 1);
    }

    #[test]
    fn non_flat_selection_uses_live_tdata_access_and_moves_the_selected_value_to_tail() {
        let (map, mut world) = camp_sources();
        let sources = map.gather_terrain_sources().unwrap();
        let diplomacy = |_: i32, _: i32| Some(false);
        let mut runtime = ArenaGatherRuntime::default();
        runtime
            .register_site(
                CAMP,
                7,
                OrdinaryGatherKind::Camp,
                GatherCapacityAuthority::MissingRetailTerrainSource,
            )
            .unwrap();
        runtime.register_worker(CAMP_WORKER, 0x32).unwrap();
        let mut rng = Random::new(7);
        runtime
            .refresh_non_flat_site(
                sources,
                &mut world,
                &diplomacy,
                CAMP,
                AuthoritativeGatherSiteType::from_live_row(&camp_type(), sources).unwrap(),
                AuthoritativeGatherSitePlacement {
                    centre_x: Coord(6 * 192),
                    centre_y: Coord(6 * 192),
                    corner_tx: 5,
                    corner_ty: 5,
                    region: 64,
                },
                GatherCapacityBonuses::default(),
                &mut rng,
            )
            .unwrap();
        runtime.stage_exact_order(CAMP_WORKER, CAMP).unwrap();
        runtime.attach_staged_worker(CAMP_WORKER, CAMP).unwrap();
        let first = runtime.mining(CAMP).unwrap().tiles()[0];
        world.set_gather_edge(first.tx, first.ty);
        *world.tmask_mut(first.tx, first.ty) |= NON_FLAT_GATHER_RESOURCE_BIT;
        let order = runtime.orders.get_mut(&CAMP_WORKER).unwrap();
        order.tx = -1;
        order.ty = -1;
        order.wait = -1;
        order.goto_build = 1;
        let animated = std::cell::Cell::new(false);

        let result = runtime
            .prepare_non_flat_site_tile(
                sources,
                &world,
                &diplomacy,
                CAMP_WORKER,
                CAMP,
                GatherTile { tx: 6, ty: 6 },
                &mut rng,
                || animated.set(true),
                |_| panic!("worker has no held doober"),
            )
            .unwrap();
        assert_eq!(result.result, NonFlatTileResult::Selected(first));
        assert!(animated.get());
        assert_eq!(runtime.mining(CAMP).unwrap().tiles().last(), Some(&first));
        assert_eq!(runtime.orders[&CAMP_WORKER].tile(), first);
        assert_eq!(runtime.orders[&CAMP_WORKER].goto_build, 0);
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

#[cfg(test)]
#[path = "gather_payout_tests.rs"]
mod gather_payout_tests;
