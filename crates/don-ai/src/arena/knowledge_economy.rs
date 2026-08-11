//! Exact base-University knowledge state for the playable Arena.
//!
//! This is deliberately separate from [`super::gather_runtime`]. Ordinary Arena workers
//! are on-map Citizens assigned to Farm/Camp/Mine sites. Retail Scholars are live owner-
//! local Units held off-map inside a University, and `BuildData::gather_max` admits at most
//! seven of them. Mixing the two states would make Scholars visible, targetable Citizens
//! and would pay the wrong `PEASANT_RATE`.
//!
//! The module owns only the recovered base-level lifecycle: stable object identities,
//! University containment, the SUPPORT production-cost ramp, and exact-scale knowledge
//! gross. University research levels 2..=6 require the still-unhosted BonusType/property
//! resolver and are intentionally not inferred from player age.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use don_sim::systems::economy::{self, EconRules, NUM_RESOURCES, RES_KNOWLEDGE};
use don_sim::systems::gathering::{self, GatherInsideObject, MAX_KNOWLEDGE_GATHERERS};
use don_sim::systems::production::{
    self, ProdRules, RampClass, SupportPair, UnitPlacementRoute,
    UNIT_PLACEMENT_TYPE_KOREAN_SCHOLAR, UNIT_PLACEMENT_TYPE_SCHOLAR,
};
use don_sim::systems::tech_cities::{self, CityRules};

/// The only University level this Arena slice can prove from currently retained state.
pub const BASE_UNIVERSITY_LEVEL: i32 = 1;

/// Stable Arena projection of retail's owner-local `(who,o,uid)` object identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KnowledgeObject {
    pub inside: GatherInsideObject,
    pub uid: u16,
}

impl Ord for KnowledgeObject {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.inside.owner, self.inside.object, self.uid).cmp(&(
            other.inside.owner,
            other.inside.object,
            other.uid,
        ))
    }
}

impl PartialOrd for KnowledgeObject {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct UniversitySite {
    university: KnowledgeObject,
    /// `CityData::get_literacy` is a city contribution. A census/unassigned University
    /// must not receive the flat ten through this path.
    assigned_city: bool,
    scholars: Vec<KnowledgeObject>,
}

/// Prevalidated `Object::insert_inside` / `Build::check_gatherers` projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScholarContainmentPlan {
    pub university: KnowledgeObject,
    pub scholar: KnowledgeObject,
    pub route: UnitPlacementRoute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnowledgeError {
    DuplicateUniversity(KnowledgeObject),
    MissingUniversity(KnowledgeObject),
    OwnerMismatch,
    DuplicateScholar(KnowledgeObject),
    CapacityReached(KnowledgeObject),
    StalePlan,
}

/// Persistent owner-local University/Scholar state plus exact containment membership.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KnowledgeEconomy {
    sites: BTreeMap<KnowledgeObject, UniversitySite>,
    inside: BTreeMap<KnowledgeObject, KnowledgeObject>,
}

impl KnowledgeEconomy {
    pub fn register_university(
        &mut self,
        university: KnowledgeObject,
        assigned_city: bool,
    ) -> Result<(), KnowledgeError> {
        if self.sites.contains_key(&university) {
            return Err(KnowledgeError::DuplicateUniversity(university));
        }
        self.sites.insert(
            university,
            UniversitySite {
                university,
                assigned_city,
                scholars: Vec::new(),
            },
        );
        Ok(())
    }

    pub fn plan_contain_scholar(
        &self,
        university: KnowledgeObject,
        scholar: KnowledgeObject,
    ) -> Result<ScholarContainmentPlan, KnowledgeError> {
        let site = self
            .sites
            .get(&university)
            .ok_or(KnowledgeError::MissingUniversity(university))?;
        if university.inside.owner != scholar.inside.owner {
            return Err(KnowledgeError::OwnerMismatch);
        }
        if self.inside.contains_key(&scholar) {
            return Err(KnowledgeError::DuplicateScholar(scholar));
        }
        if site.scholars.len() >= MAX_KNOWLEDGE_GATHERERS as usize {
            return Err(KnowledgeError::CapacityReached(university));
        }
        // The queue gate makes overflow unreachable. After allocation the exact production
        // classifier therefore takes University Scholar -> CheckGatherers, never ComeOut.
        Ok(ScholarContainmentPlan {
            university,
            scholar,
            route: UnitPlacementRoute::CheckGatherers,
        })
    }

    pub fn apply_containment(
        &mut self,
        plan: ScholarContainmentPlan,
    ) -> Result<(), KnowledgeError> {
        if plan.route != UnitPlacementRoute::CheckGatherers
            || self.inside.contains_key(&plan.scholar)
        {
            return Err(KnowledgeError::StalePlan);
        }
        let Some(site) = self.sites.get_mut(&plan.university) else {
            return Err(KnowledgeError::StalePlan);
        };
        if site.scholars.len() >= MAX_KNOWLEDGE_GATHERERS as usize
            || plan.university.inside.owner != plan.scholar.inside.owner
        {
            return Err(KnowledgeError::StalePlan);
        }
        site.scholars.push(plan.scholar);
        self.inside.insert(plan.scholar, plan.university);
        Ok(())
    }

    pub fn is_inside(&self, scholar: KnowledgeObject) -> bool {
        self.inside.contains_key(&scholar)
    }

    pub fn contained_count(&self, university: KnowledgeObject) -> usize {
        self.sites
            .get(&university)
            .map_or(0, |site| site.scholars.len())
    }

    /// Exact gross-income image for active base-level Universities.
    ///
    /// `CityData::get_literacy` returns stock units, so its result is shifted to the same
    /// sixteenths scale as `BuildTypeData::calc_gather`. Scholar gross is delegated to the
    /// recovered rules-table lookup and cap helper.
    pub fn gross(&self, owner: i8, active: &BTreeSet<KnowledgeObject>) -> [i32; NUM_RESOURCES] {
        let econ = EconRules::shipped();
        let cities = CityRules::RETAIL;
        let mut out = [0i32; NUM_RESOURCES];
        for site in self.sites.values().filter(|site| {
            site.university.inside.owner == owner && active.contains(&site.university)
        }) {
            if site.assigned_city {
                out[RES_KNOWLEDGE] = out[RES_KNOWLEDGE]
                    .wrapping_add(tech_cities::city_literacy(&cities, true, false).wrapping_shl(4));
            }
            let mut per_worker = [0i32; NUM_RESOURCES];
            per_worker[RES_KNOWLEDGE] =
                economy::scholar_rate_for_level(&econ, BASE_UNIVERSITY_LEVEL);
            let scholars = gathering::site_gross(
                per_worker,
                site.scholars.len() as i32,
                MAX_KNOWLEDGE_GATHERERS as i8,
            );
            for (slot, value) in out.iter_mut().zip(scholars) {
                *slot = slot.wrapping_add(value);
            }
        }
        out
    }
}

pub fn is_scholar_type(type_id: i32) -> bool {
    matches!(
        type_id,
        UNIT_PLACEMENT_TYPE_SCHOLAR | UNIT_PLACEMENT_TYPE_KOREAN_SCHOLAR
    )
}

/// `TypeData::get_cost`'s exact Scholar SUPPORT ramp for one pre-purchase owned+queued
/// count. Nation/wonder modifiers absent from Arena remain outside this bounded slice.
pub fn scholar_cost(
    base: [i32; NUM_RESOURCES],
    support: [i32; 2],
    support_cost: [i32; 2],
    progression: i32,
    owned_and_queued: i32,
    rules: &ProdRules,
) -> [i32; NUM_RESOURCES] {
    let pair = SupportPair {
        support,
        support_cost,
    };
    std::array::from_fn(|resource| {
        base[resource].wrapping_add(production::ramp_cost(
            resource as i32,
            owned_and_queued,
            progression,
            RampClass::Scholar,
            base[resource],
            &pair,
            false,
            rules,
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(owner: i8, object: i16) -> KnowledgeObject {
        KnowledgeObject {
            inside: GatherInsideObject { owner, object },
            uid: object as u16,
        }
    }

    #[test]
    fn exact_cost_cap_and_gross_owners_are_composed() {
        let rules = ProdRules::shipped();
        let base = [0, 0, 30, 0, 0, 0];
        assert_eq!(scholar_cost(base, [2, -1], [2, 0], 0, 0, &rules)[2], 30);
        assert_eq!(scholar_cost(base, [2, -1], [2, 0], 0, 1, &rules)[2], 32);
        assert_eq!(scholar_cost(base, [2, -1], [2, 0], 0, 7, &rules)[2], 44);

        let university = object(0, 2000);
        let mut k = KnowledgeEconomy::default();
        k.register_university(university, true).unwrap();
        for o in 0..MAX_KNOWLEDGE_GATHERERS as i16 {
            let plan = k.plan_contain_scholar(university, object(0, o)).unwrap();
            k.apply_containment(plan).unwrap();
        }
        assert!(matches!(
            k.plan_contain_scholar(university, object(0, 8)),
            Err(KnowledgeError::CapacityReached(_))
        ));
        assert_eq!(
            k.gross(0, &BTreeSet::from([university]))[RES_KNOWLEDGE],
            720
        );
        assert_eq!(k.gross(0, &BTreeSet::new())[RES_KNOWLEDGE], 0);
    }
}
