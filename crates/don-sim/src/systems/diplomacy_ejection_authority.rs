// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact installed authority for the contained-Unit arm of alliance revocation.
//!
//! `Leader::eject_my_shit_from_his_ass` reads three facts which are not owned by the
//! diplomacy aggregate: the result of `Unit::come_out(0)`, the Unit type's domain, and an
//! AIR_PATROL order query.  This producer binds those answers to the canonical live Unit's
//! generational identity, retail address, type, containment link, UID, and whole-Sim channel
//! digest.  It only projects facts; the mutation calls remain mandatory external authority.

use super::leader_set_diplo::{EjectionUnitFact, DIPLO_SLOTS};
use crate::objects::Band;
use crate::world::World;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContainedEjectionAnswer {
    pub owner: usize,
    pub object_id: i32,
    /// Exact `Unit::come_out(0)` return. Zero is success.
    pub come_out_return: i32,
    /// Lazy `UnitTypeData::domain`; read only after successful `come_out`.
    pub domain: Option<i32>,
    /// Lazy AIR_PATROL query; read only for a successful domain-2 Unit.
    pub has_air_patrol_order: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundContainedEjectionFact {
    pub owner: usize,
    pub object_id: i32,
    pub handle_id: u32,
    pub generation: u32,
    pub uid: u16,
    pub type_index: i32,
    pub carrier_owner: i8,
    pub carrier_object_id: i16,
    pub come_out_return: i32,
    pub domain: Option<i32>,
    pub has_air_patrol_order: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContainedEjectionAuthority {
    pub expected_channel_digest: Option<u64>,
    /// Exact owner-list order. There is one entry for every contained live Unit and no others.
    pub units: Vec<BoundContainedEjectionFact>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainedEjectionAuthorityError {
    UnitRowOutOfRange {
        owner: usize,
        object_id: i32,
        row: usize,
    },
    UnitAddressMismatch {
        owner: usize,
        object_id: i32,
        row: usize,
    },
    MissingStableIdentity {
        owner: usize,
        object_id: i32,
        row: usize,
    },
    InvalidCarrier {
        owner: usize,
        object_id: i32,
        carrier_owner: i8,
    },
    AnswersNotInOwnerListOrder,
    MissingAnswer {
        owner: usize,
        object_id: i32,
    },
    UnexpectedAnswer {
        owner: usize,
        object_id: i32,
    },
    LazyAnswerShape {
        owner: usize,
        object_id: i32,
    },
    MissingChannelDigest,
    ChannelDigestMismatch {
        expected: u64,
        actual: u64,
    },
    StaleUnitIdentity {
        owner: usize,
        object_id: i32,
    },
}

#[derive(Clone, Copy)]
struct LiveContainedUnit {
    owner: usize,
    object_id: i32,
    handle_id: u32,
    generation: u32,
    uid: u16,
    type_index: i32,
    carrier_owner: i8,
    carrier_object_id: i16,
}

fn live_contained_units(
    world: &World,
) -> Result<Vec<LiveContainedUnit>, ContainedEjectionAuthorityError> {
    let mut units = Vec::new();
    for owner in 0..DIPLO_SLOTS {
        for (object_id, row) in world
            .objects
            .slot(owner)
            .band(Band::Unit)
            .iter()
            .copied()
            .enumerate()
        {
            let row = row as usize;
            if row >= world.units.len() {
                return Err(ContainedEjectionAuthorityError::UnitRowOutOfRange {
                    owner,
                    object_id: object_id as i32,
                    row,
                });
            }
            if usize::from(world.units.get_who(row)) != owner
                || i32::from(world.units.o()[row]) != object_id as i32
            {
                return Err(ContainedEjectionAuthorityError::UnitAddressMismatch {
                    owner,
                    object_id: object_id as i32,
                    row,
                });
            }
            if super::air::is_on_map(world.units.inside_up()[row]) {
                continue;
            }
            let carrier_owner = world.units.inside_up_who()[row];
            if !(0..DIPLO_SLOTS as i8).contains(&carrier_owner) {
                return Err(ContainedEjectionAuthorityError::InvalidCarrier {
                    owner,
                    object_id: object_id as i32,
                    carrier_owner,
                });
            }
            let Some(handle) = world.handle_at_row(row) else {
                return Err(ContainedEjectionAuthorityError::MissingStableIdentity {
                    owner,
                    object_id: object_id as i32,
                    row,
                });
            };
            let Some(type_index) = world.unit_type_id(row) else {
                return Err(ContainedEjectionAuthorityError::MissingStableIdentity {
                    owner,
                    object_id: object_id as i32,
                    row,
                });
            };
            units.push(LiveContainedUnit {
                owner,
                object_id: object_id as i32,
                handle_id: handle.id,
                generation: handle.generation,
                uid: world.units.get_uid(row),
                type_index,
                carrier_owner,
                carrier_object_id: world.units.inside_up()[row],
            });
        }
    }
    Ok(units)
}

fn answer_shape_is_exact(answer: &ContainedEjectionAnswer) -> bool {
    if answer.come_out_return != 0 {
        return answer.domain.is_none() && answer.has_air_patrol_order.is_none();
    }
    match answer.domain {
        Some(2) => answer.has_air_patrol_order.is_some(),
        Some(_) => answer.has_air_patrol_order.is_none(),
        None => false,
    }
}

impl ContainedEjectionAuthority {
    /// Bind the complete answer roster to one canonical live snapshot.
    pub fn capture(
        world: &World,
        channel_digest: u64,
        answers: &[ContainedEjectionAnswer],
    ) -> Result<Self, ContainedEjectionAuthorityError> {
        if answers
            .windows(2)
            .any(|pair| (pair[0].owner, pair[0].object_id) >= (pair[1].owner, pair[1].object_id))
        {
            return Err(ContainedEjectionAuthorityError::AnswersNotInOwnerListOrder);
        }
        let live = live_contained_units(world)?;
        let mut units = Vec::with_capacity(live.len());
        for (index, unit) in live.iter().enumerate() {
            let Some(answer) = answers.get(index) else {
                return Err(ContainedEjectionAuthorityError::MissingAnswer {
                    owner: unit.owner,
                    object_id: unit.object_id,
                });
            };
            let expected = (unit.owner, unit.object_id);
            let actual = (answer.owner, answer.object_id);
            if actual < expected {
                return Err(ContainedEjectionAuthorityError::UnexpectedAnswer {
                    owner: answer.owner,
                    object_id: answer.object_id,
                });
            }
            if actual != expected {
                return Err(ContainedEjectionAuthorityError::MissingAnswer {
                    owner: unit.owner,
                    object_id: unit.object_id,
                });
            }
            if !answer_shape_is_exact(answer) {
                return Err(ContainedEjectionAuthorityError::LazyAnswerShape {
                    owner: unit.owner,
                    object_id: unit.object_id,
                });
            }
            units.push(BoundContainedEjectionFact {
                owner: unit.owner,
                object_id: unit.object_id,
                handle_id: unit.handle_id,
                generation: unit.generation,
                uid: unit.uid,
                type_index: unit.type_index,
                carrier_owner: unit.carrier_owner,
                carrier_object_id: unit.carrier_object_id,
                come_out_return: answer.come_out_return,
                domain: answer.domain,
                has_air_patrol_order: answer.has_air_patrol_order,
            });
        }
        if let Some(answer) = answers.get(live.len()) {
            return Err(ContainedEjectionAuthorityError::UnexpectedAnswer {
                owner: answer.owner,
                object_id: answer.object_id,
            });
        }
        Ok(Self {
            expected_channel_digest: Some(channel_digest),
            units,
        })
    }

    /// Revalidate the complete live census and lower it into the exact set-diplo projection.
    pub fn project(
        &self,
        world: &World,
        channel_digest: u64,
    ) -> Result<[Vec<EjectionUnitFact>; DIPLO_SLOTS], ContainedEjectionAuthorityError> {
        let live = live_contained_units(world)?;
        if self.units.len() < live.len() {
            let unit = live[self.units.len()];
            return Err(ContainedEjectionAuthorityError::MissingAnswer {
                owner: unit.owner,
                object_id: unit.object_id,
            });
        }
        if self.units.len() > live.len() {
            let fact = self.units[live.len()];
            return Err(ContainedEjectionAuthorityError::UnexpectedAnswer {
                owner: fact.owner,
                object_id: fact.object_id,
            });
        }
        if !live.is_empty() || !self.units.is_empty() {
            let expected = self
                .expected_channel_digest
                .ok_or(ContainedEjectionAuthorityError::MissingChannelDigest)?;
            if expected != channel_digest {
                return Err(ContainedEjectionAuthorityError::ChannelDigestMismatch {
                    expected,
                    actual: channel_digest,
                });
            }
        }
        let mut projected: [Vec<EjectionUnitFact>; DIPLO_SLOTS] =
            std::array::from_fn(|_| Vec::new());
        let mut contained_index = 0;
        for owner in 0..DIPLO_SLOTS {
            for (object_id, row) in world
                .objects
                .slot(owner)
                .band(Band::Unit)
                .iter()
                .copied()
                .enumerate()
            {
                let row = row as usize;
                if super::air::is_on_map(world.units.inside_up()[row]) {
                    projected[owner].push(EjectionUnitFact {
                        object_id: object_id as i32,
                        is_unit: true,
                        carrier_who: None,
                        come_out_return: None,
                        domain: None,
                        has_air_patrol_order: None,
                    });
                    continue;
                }
                let unit = live[contained_index];
                let Some(fact) = self.units.get(contained_index) else {
                    return Err(ContainedEjectionAuthorityError::MissingAnswer {
                        owner: unit.owner,
                        object_id: unit.object_id,
                    });
                };
                if (fact.owner, fact.object_id) < (unit.owner, unit.object_id) {
                    return Err(ContainedEjectionAuthorityError::UnexpectedAnswer {
                        owner: fact.owner,
                        object_id: fact.object_id,
                    });
                }
                if fact.owner != unit.owner || fact.object_id != unit.object_id {
                    return Err(ContainedEjectionAuthorityError::MissingAnswer {
                        owner: unit.owner,
                        object_id: unit.object_id,
                    });
                }
                if fact.handle_id != unit.handle_id
                    || fact.generation != unit.generation
                    || fact.uid != unit.uid
                    || fact.type_index != unit.type_index
                    || fact.carrier_owner != unit.carrier_owner
                    || fact.carrier_object_id != unit.carrier_object_id
                    || !answer_shape_is_exact(&ContainedEjectionAnswer {
                        owner: fact.owner,
                        object_id: fact.object_id,
                        come_out_return: fact.come_out_return,
                        domain: fact.domain,
                        has_air_patrol_order: fact.has_air_patrol_order,
                    })
                {
                    return Err(ContainedEjectionAuthorityError::StaleUnitIdentity {
                        owner: unit.owner,
                        object_id: unit.object_id,
                    });
                }
                projected[owner].push(EjectionUnitFact {
                    object_id: unit.object_id,
                    is_unit: true,
                    carrier_who: Some(unit.carrier_owner as usize),
                    come_out_return: Some(fact.come_out_return),
                    domain: fact.domain,
                    has_air_patrol_order: fact.has_air_patrol_order,
                });
                contained_index += 1;
            }
        }
        if let Some(fact) = self.units.get(contained_index) {
            return Err(ContainedEjectionAuthorityError::UnexpectedAnswer {
                owner: fact.owner,
                object_id: fact.object_id,
            });
        }
        Ok(projected)
    }
}
