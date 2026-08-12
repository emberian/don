// SPDX-License-Identifier: GPL-3.0-or-later
//! Atomic authority producer for retail `UnitData::speed` (`0x0060AAE0`).
//!
//! Land speed is not the static `MOVES` column.  The retail body joins the live Unit,
//! checksum-owned territory, mutual diplomacy, both ObjectType relation sets, ordered
//! `HeroesData`, five exact `LeaderData::num_units` cells, and twelve `Constants` fields.
//! This module performs that join without mutating any owner and binds the results to the
//! exact generational objects from which they were computed.

use std::collections::BTreeSet;

use crate::systems::air::is_on_map;
use crate::systems::group_move_authority::{
    GroupMoveContent, GroupMoveTypeFacts, ResolvedLandSpeed,
};
use crate::systems::map_terrain::{floor_div, COORD_PER_WCELL};
use crate::systems::movement::vector_dist;
use crate::systems::step12_visibility_runtime::{
    VisibilityHeroRecord, HERO_RECORD_ACTIVE, UNIT_TYPE_BASE,
};
use crate::tick::Sim;
use crate::world::{Handle, OBJ_FLAG_ACTIVE};

const TYPE_IRQ_SPEAR: i32 = 0x0a6;
const TYPE_IRQ_MO_SPEAR: i32 = 0x0a7;
const TYPE_IRQ_HMO_SPEAR: i32 = 0x0a8;
const TYPE_IRQ_EMO_SPEAR: i32 = 0x0a9;
const TYPE_HEAVY_ELEPHANT: i32 = 0x105;
const TYPE_ALEXANDER: i32 = 0x166;
const TYPE_NAPOLEON: i32 = 0x167;
const TYPE_SPITAMENES: i32 = 0x16d;
const TYPE_PORUS: i32 = 0x16f;
const TYPE_CHARLES: i32 = 0x171;
const TYPE_BLUCHER: i32 = 0x175;
const TYPE_STABLE: i32 = 0x1ac;

const SPEED_HERO_TYPES: [i32; 5] = [
    TYPE_SPITAMENES,
    TYPE_PORUS,
    TYPE_CHARLES,
    TYPE_NAPOLEON,
    TYPE_BLUCHER,
];

/// Static fields reached by `UnitData::speed` and `ObjectTypeData::is_slow`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LandSpeedTypeFacts {
    pub type_id: i32,
    pub from: i32,
    pub where_type: i32,
    pub graft: i32,
    pub domain: i32,
    pub unit_flags: u32,
    pub unit_flags2: u32,
}

/// The twelve `Constants` cells read by `UnitData::speed`, named by their retail use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LandSpeedConstants {
    /// `Constants+0x004`.
    pub coord_scale: i32,
    /// `Constants+0x838..+0x844`, strict IRQ spear terrain additions.
    pub irq_spear_bonus: i32,
    pub irq_mo_spear_bonus: i32,
    pub irq_hmo_spear_bonus: i32,
    pub irq_emo_spear_bonus: i32,
    /// `Constants+0xB4C`, Alexander/Napoleon aura multiplier, fixed 8.8.
    pub alexander_napoleon_aura_256: i32,
    /// `Constants+0xB78`, Spitamenes Stable multiplier, fixed 8.8.
    pub spitamenes_stable_256: i32,
    /// `Constants+0xB7C`, Porus Heavy-Elephant multiplier, fixed 8.8.
    pub porus_elephant_256: i32,
    /// `Constants+0xBB0`, Napoleon siege multiplier, percent.
    pub napoleon_siege_percent: i32,
    /// `Constants+0xBC0`, Archduke Charles multiplier, percent.
    pub charles_percent: i32,
    /// `Constants+0xBD4`, Blucher Stable multiplier, percent.
    pub blucher_stable_percent: i32,
    /// `Constants+0xC50`, the hero aura speed base.
    pub hero_aura_speed: i32,
}

/// One synchronized postload type/Constants source.
pub trait LandSpeedContent {
    fn land_speed_revision(&self) -> u64;
    fn land_speed_composition_digest(&self) -> [u8; 32];
    fn land_speed_type(&self, type_id: i32) -> Option<LandSpeedTypeFacts>;
    fn land_speed_constants(&self) -> Option<LandSpeedConstants>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LandSpeedAuthorityError {
    UnitTypeRowsUnavailable {
        live: usize,
        rows: usize,
    },
    MissingHandle {
        row: usize,
    },
    InvalidOwner {
        row: usize,
        who: i8,
    },
    MissingType {
        type_id: i32,
    },
    TypeIdentityMismatch {
        requested: i32,
        supplied: i32,
    },
    TypeRelationCycle {
        type_id: i32,
    },
    MissingConstants,
    PositionOutsideTerrain {
        handle: Handle,
        x: i32,
        y: i32,
    },
    InvalidTerrainOwner {
        handle: Handle,
        who: i8,
    },
    LeaderCounterShape {
        who: usize,
        rows: usize,
    },
    LeaderCounterMismatch {
        who: usize,
        type_id: i32,
        leader_count: u16,
        live_count: u16,
    },
    HeroRegistryCoverageMismatch {
        who: usize,
        expected: usize,
        got: usize,
    },
    MissingHeroObject {
        who: usize,
        slot: u16,
        handle: Handle,
    },
    HeroIdentityMismatch {
        who: usize,
        slot: u16,
        handle: Handle,
    },
    InvalidHeroRadius {
        who: usize,
        slot: u16,
        radius: i32,
    },
    DuplicateHeroObject {
        who: usize,
        handle: Handle,
    },
    DuplicateResolvedObject {
        handle: Handle,
    },
    StaleAuthority {
        expected: u64,
        current: u64,
    },
}

/// One resolved value plus the complete retail object identity that supplied it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedLandSpeedEntry {
    pub handle: Handle,
    pub uid: u16,
    pub who: i8,
    pub o: i16,
    pub type_id: i32,
    pub speed: i32,
}

/// Pure, revision-bound result of evaluating every live land Unit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolvedLandSpeedAuthority {
    pub content_revision: u64,
    pub composition_digest: [u8; 32],
    pub state_digest: u64,
    pub entries: Vec<ResolvedLandSpeedEntry>,
}

impl ResolvedLandSpeedAuthority {
    fn entry(&self, handle: Handle) -> Option<ResolvedLandSpeedEntry> {
        self.entries
            .binary_search_by_key(&(handle.id, handle.generation), |entry| {
                (entry.handle.id, entry.handle.generation)
            })
            .ok()
            .map(|index| self.entries[index])
    }

    /// Re-evaluate every input and borrow the exact Sim for the lifetime of the Group-Move
    /// projection.  The immutable borrow prevents a state mutation between validation and use.
    pub fn bind<'a, C>(
        &'a self,
        sim: &'a Sim,
        content: &'a C,
    ) -> Result<BoundLandSpeedContent<'a, C>, LandSpeedAuthorityError>
    where
        C: LandSpeedContent + GroupMoveContent,
    {
        let current = produce_resolved_land_speed_authority(sim, content)?;
        if &current != self {
            return Err(LandSpeedAuthorityError::StaleAuthority {
                expected: self.state_digest,
                current: current.state_digest,
            });
        }
        Ok(BoundLandSpeedContent {
            content,
            authority: self,
            _sim: sim,
        })
    }
}

/// Group-Move content view whose land speeds are proven against the simultaneously borrowed Sim.
pub struct BoundLandSpeedContent<'a, C> {
    content: &'a C,
    authority: &'a ResolvedLandSpeedAuthority,
    _sim: &'a Sim,
}

impl<C: GroupMoveContent> GroupMoveContent for BoundLandSpeedContent<'_, C> {
    fn revision(&self) -> u64 {
        self.content.revision().wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ self.authority.state_digest
    }

    fn type_facts(&self, type_id: i32) -> Option<GroupMoveTypeFacts> {
        self.content.type_facts(type_id)
    }

    fn resolved_land_speed(&self, handle: Handle) -> Option<ResolvedLandSpeed> {
        self.authority.entry(handle).map(|entry| ResolvedLandSpeed {
            handle: entry.handle,
            speed: entry.speed,
        })
    }

    fn water_effective_type(&self, handle: Handle) -> Option<i32> {
        self.content.water_effective_type(handle)
    }
}

struct Resolver<'a, C> {
    sim: &'a Sim,
    content: &'a C,
    constants: LandSpeedConstants,
    hash: u64,
}

impl<C: LandSpeedContent> Resolver<'_, C> {
    fn mix(&mut self, value: u64) {
        self.hash ^= value;
        self.hash = self.hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    fn mix_i32(&mut self, value: i32) {
        self.mix(value as u32 as u64);
    }

    fn type_facts(&mut self, type_id: i32) -> Result<LandSpeedTypeFacts, LandSpeedAuthorityError> {
        let facts = self
            .content
            .land_speed_type(type_id)
            .ok_or(LandSpeedAuthorityError::MissingType { type_id })?;
        if facts.type_id != type_id {
            return Err(LandSpeedAuthorityError::TypeIdentityMismatch {
                requested: type_id,
                supplied: facts.type_id,
            });
        }
        self.mix_i32(facts.type_id);
        self.mix_i32(facts.from);
        self.mix_i32(facts.where_type);
        self.mix_i32(facts.graft);
        self.mix_i32(facts.domain);
        self.mix(u64::from(facts.unit_flags));
        self.mix(u64::from(facts.unit_flags2));
        Ok(facts)
    }

    /// `ObjectTypeData::is_slow` (`0x00661AE0`), including the distinct strict graft arm.
    fn type_is(
        &mut self,
        actual: i32,
        queried: i32,
        strict: bool,
    ) -> Result<bool, LandSpeedAuthorityError> {
        if actual == queried {
            return Ok(true);
        }
        let facts = self.type_facts(actual)?;
        if strict {
            if !(50..=413).contains(&actual) || facts.graft != queried {
                return Ok(false);
            }
            if !(50..=413).contains(&queried) {
                return Ok(false);
            }
            let target = self.type_facts(queried)?;
            return Ok(target.unit_flags & 0x0100_0000 == 0);
        }
        if queried < 0 {
            return Ok(false);
        }
        if facts.graft == queried {
            return Ok(true);
        }
        let mut seen = BTreeSet::from([actual]);
        let mut parent = facts.from;
        while parent >= 0 {
            if !seen.insert(parent) {
                return Err(LandSpeedAuthorityError::TypeRelationCycle { type_id: actual });
            }
            if parent == queried {
                return Ok(true);
            }
            let parent_facts = self.type_facts(parent)?;
            if parent_facts.graft == queried {
                return Ok(true);
            }
            parent = parent_facts.from;
        }
        Ok(false)
    }

    fn validate_leader_counters(&mut self) -> Result<(), LandSpeedAuthorityError> {
        for who in 0..self.sim.vic_leaders.slots.len() {
            let leader = &self.sim.vic_leaders.slots[who];
            if leader.num_units.len() < 352 {
                return Err(LandSpeedAuthorityError::LeaderCounterShape {
                    who,
                    rows: leader.num_units.len(),
                });
            }
            for type_id in SPEED_HERO_TYPES {
                let index = usize::try_from(type_id - UNIT_TYPE_BASE).unwrap();
                let leader_count = leader.num_units[index];
                let live_count = (0..self.sim.world.live_count() as usize)
                    .filter(|&row| {
                        self.sim.world.units.get_flags(row) & OBJ_FLAG_ACTIVE != 0
                            && usize::from(self.sim.world.units.get_who(row)) == who
                            && self.sim.unit_type.get(row).copied() == Some(type_id)
                    })
                    .count()
                    .min(u16::MAX as usize) as u16;
                self.mix(u64::from(leader_count));
                self.mix(u64::from(live_count));
                if leader_count != live_count {
                    return Err(LandSpeedAuthorityError::LeaderCounterMismatch {
                        who,
                        type_id,
                        leader_count,
                        live_count,
                    });
                }
            }
        }
        Ok(())
    }

    fn validate_hero_registries(&mut self) -> Result<(), LandSpeedAuthorityError> {
        self.mix(self.sim.step12_visibility.state_revision());
        self.mix(self.sim.step12_visibility.digest());
        for who in 0..self.sim.vic_leaders.slots.len() {
            let records = &self.sim.step12_visibility.leaders()[who].heroes;
            let mut expected = BTreeSet::new();
            for row in 0..self.sim.world.live_count() as usize {
                if usize::from(self.sim.world.units.get_who(row)) != who {
                    continue;
                }
                let type_id = self.sim.unit_type[row];
                if self.type_facts(type_id)?.unit_flags2 & 0x20 != 0 {
                    let handle = self
                        .sim
                        .world
                        .handle_at_row(row)
                        .ok_or(LandSpeedAuthorityError::MissingHandle { row })?;
                    expected.insert((handle.id, handle.generation));
                }
            }
            if expected.len() != records.len() {
                return Err(LandSpeedAuthorityError::HeroRegistryCoverageMismatch {
                    who,
                    expected: expected.len(),
                    got: records.len(),
                });
            }
            let mut got = BTreeSet::new();
            for record in records {
                if record.radius_tiles < 0 {
                    return Err(LandSpeedAuthorityError::InvalidHeroRadius {
                        who,
                        slot: record.slot,
                        radius: record.radius_tiles,
                    });
                }
                if !got.insert((record.handle.id, record.handle.generation)) {
                    return Err(LandSpeedAuthorityError::DuplicateHeroObject {
                        who,
                        handle: record.handle,
                    });
                }
                let row = self.sim.world.row_of(record.handle).ok_or(
                    LandSpeedAuthorityError::MissingHeroObject {
                        who,
                        slot: record.slot,
                        handle: record.handle,
                    },
                )?;
                if self.sim.world.units.get_uid(row) != record.uid
                    || self.sim.world.units.get_who(row) != record.who as u8
                    || self.sim.world.units.o()[row] != record.o
                    || self.sim.unit_type[row] != record.type_index
                    || usize::from(record.who as u8) != who
                {
                    return Err(LandSpeedAuthorityError::HeroIdentityMismatch {
                        who,
                        slot: record.slot,
                        handle: record.handle,
                    });
                }
                self.mix(u64::from(record.slot));
                self.mix(u64::from(record.handle.id));
                self.mix(u64::from(record.handle.generation));
                self.mix(u64::from(record.uid));
                self.mix(record.o as u16 as u64);
                self.mix(record.who as u8 as u64);
                self.mix(u64::from(record.hero_flags));
                self.mix_i32(record.type_index);
                self.mix_i32(record.radius_tiles);
            }
            if got != expected {
                return Err(LandSpeedAuthorityError::HeroRegistryCoverageMismatch {
                    who,
                    expected: expected.len(),
                    got: got.intersection(&expected).count(),
                });
            }
        }
        Ok(())
    }

    fn live_hero(
        &mut self,
        who: usize,
        record: &'_ VisibilityHeroRecord,
    ) -> Result<Option<(usize, i32, i32)>, LandSpeedAuthorityError> {
        if record.hero_flags & HERO_RECORD_ACTIVE == 0 {
            return Ok(None);
        }
        let row = self.sim.world.row_of(record.handle).ok_or(
            LandSpeedAuthorityError::MissingHeroObject {
                who,
                slot: record.slot,
                handle: record.handle,
            },
        )?;
        if self.sim.world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0
            || !is_on_map(self.sim.world.units.inside_up()[row])
        {
            return Ok(None);
        }
        Ok(Some((row, self.sim.unit_type[row], record.radius_tiles)))
    }

    /// `HeroesData::find_hero` (`0x0073A1B0`), in owner-list order.  `extra_radius` is zero
    /// for `UnitData::speed`; the nonzero Object/Build arm at the call site is unreachable for
    /// a Unit receiver and is kept explicit here to prevent a future guessed size bonus.
    fn find_nearest_hero(
        &mut self,
        actor_row: usize,
        mask: u32,
        queried_type: Option<i32>,
        extra_radius: i32,
    ) -> Result<Option<usize>, LandSpeedAuthorityError> {
        let who = usize::from(self.sim.world.units.get_who(actor_row));
        let (x, y) = (
            self.sim.world.units.x_internal()[actor_row],
            self.sim.world.units.y_internal()[actor_row],
        );
        let records = self.sim.step12_visibility.leaders()[who].heroes.clone();
        for record in &records {
            let Some((hero_row, hero_type, radius_tiles)) = self.live_hero(who, record)? else {
                continue;
            };
            if let Some(queried) = queried_type {
                if !self.type_is(hero_type, queried, false)? {
                    continue;
                }
            }
            let hx = self.sim.world.units.x_internal()[hero_row];
            let hy = self.sim.world.units.y_internal()[hero_row];
            let distance = vector_dist(x.wrapping_sub(hx), y.wrapping_sub(hy));
            let radius = radius_tiles.wrapping_mul(0xc0);
            if distance.wrapping_sub(extra_radius) <= radius
                && (mask == 0 || self.sim.world.units.get_unit_masks(hero_row) & mask != 0)
            {
                return Ok(Some(hero_row));
            }
        }
        Ok(None)
    }

    /// `ObjectData::has_general` (`0x00646B00`) specialized to a Unit receiver.
    fn has_general(
        &mut self,
        actor_row: usize,
        mask: u32,
        queried_type: i32,
    ) -> Result<Option<usize>, LandSpeedAuthorityError> {
        let actor_type = self.sim.unit_type[actor_row];
        if self.type_facts(actor_type)?.unit_flags2 & 0x20 != 0
            && (mask == 0 || self.sim.world.units.get_unit_masks(actor_row) & mask != 0)
            && self.type_is(actor_type, queried_type, false)?
        {
            return Ok(Some(actor_row));
        }
        let who = usize::from(self.sim.world.units.get_who(actor_row));
        let count_index = usize::try_from(queried_type - UNIT_TYPE_BASE).unwrap();
        if self.sim.vic_leaders.slots[who].num_units[count_index] == 0 {
            return Ok(None);
        }
        self.find_nearest_hero(actor_row, mask, Some(queried_type), 0)
    }

    fn terrain_bonus(
        &mut self,
        row: usize,
        handle: Handle,
        facts: LandSpeedTypeFacts,
    ) -> Result<i32, LandSpeedAuthorityError> {
        if !self.type_is(facts.type_id, TYPE_IRQ_SPEAR, false)? {
            return Ok(0);
        }
        let x = self.sim.world.units.x_internal()[row];
        let y = self.sim.world.units.y_internal()[row];
        let wx = floor_div(x, COORD_PER_WCELL);
        let wy = floor_div(y, COORD_PER_WCELL);
        if !self.sim.map.world.valid_w(wx, wy) {
            return Err(LandSpeedAuthorityError::PositionOutsideTerrain { handle, x, y });
        }
        let terrain_who = self.sim.map.world.wdata(wx, wy).who;
        self.mix(terrain_who as u8 as u64);
        if terrain_who < 0 {
            return Ok(0);
        }
        let terrain_who = usize::try_from(terrain_who).map_err(|_| {
            LandSpeedAuthorityError::InvalidTerrainOwner {
                handle,
                who: terrain_who,
            }
        })?;
        let actor_who = usize::from(self.sim.world.units.get_who(row));
        if terrain_who >= self.sim.vic_leaders.slots.len() {
            return Err(LandSpeedAuthorityError::InvalidTerrainOwner {
                handle,
                who: terrain_who as i8,
            });
        }
        for &diplo in &self.sim.vic_leaders.slots[terrain_who].diplos {
            self.mix_i32(diplo);
        }
        for &diplo in &self.sim.vic_leaders.slots[actor_who].diplos {
            self.mix_i32(diplo);
        }
        if !self.sim.vic_leaders.is_ally(terrain_who, actor_who) {
            return Ok(0);
        }
        if self.type_is(facts.type_id, TYPE_IRQ_SPEAR, true)? {
            Ok(self.constants.irq_spear_bonus)
        } else if self.type_is(facts.type_id, TYPE_IRQ_MO_SPEAR, true)? {
            Ok(self.constants.irq_mo_spear_bonus)
        } else if self.type_is(facts.type_id, TYPE_IRQ_HMO_SPEAR, true)? {
            Ok(self.constants.irq_hmo_spear_bonus)
        } else if self.type_is(facts.type_id, TYPE_IRQ_EMO_SPEAR, true)? {
            Ok(self.constants.irq_emo_spear_bonus)
        } else {
            Ok(0)
        }
    }

    fn resolve(&mut self, row: usize, handle: Handle) -> Result<i32, LandSpeedAuthorityError> {
        let facts = self.type_facts(self.sim.unit_type[row])?;
        let base = i32::from(self.sim.world.units.myspeed()[row]);
        if facts.domain != 0 {
            return Ok(base);
        }

        let mut speed = base.wrapping_add(self.terrain_bonus(row, handle, facts)?);
        let who = usize::from(self.sim.world.units.get_who(row));
        let leader_flags = self.sim.vic_leaders.slots[who].leader_flags as u32;
        self.mix(u64::from(leader_flags));

        // The aura branch follows the terrain branch but compares against the original
        // `myspeed`, not the terrain-adjusted value.  A qualifying aura therefore replaces a
        // smaller terrain result; this ordering is visible in the retail instructions.
        if leader_flags & 0x8000 != 0 {
            let aura = if facts.unit_flags2 & 0x20 != 0
                && self.sim.world.units.get_unit_masks(row) & 0x8000 != 0
            {
                Some(row)
            } else {
                self.find_nearest_hero(row, 0x8000, None, 0)?
            };
            if let Some(hero_row) = aura {
                let hero_type = self.sim.unit_type[hero_row];
                let paired = self.type_is(hero_type, TYPE_ALEXANDER, false)?
                    || self.type_is(facts.type_id, TYPE_ALEXANDER, false)?
                    || self.type_is(hero_type, TYPE_NAPOLEON, false)?
                    || self.type_is(facts.type_id, TYPE_NAPOLEON, false)?;
                let candidate = if paired {
                    self.constants
                        .hero_aura_speed
                        .wrapping_mul(self.constants.alexander_napoleon_aura_256)
                        .wrapping_mul(self.constants.coord_scale)
                        / 256
                } else {
                    self.constants
                        .hero_aura_speed
                        .wrapping_mul(self.constants.coord_scale)
                };
                speed = if candidate > base { candidate } else { base };
            }
        }

        if facts.where_type == TYPE_STABLE && self.has_general(row, 0, TYPE_SPITAMENES)?.is_some() {
            speed = self.constants.spitamenes_stable_256.wrapping_mul(speed) / 256;
        }
        if facts.where_type == TYPE_STABLE && self.has_general(row, 0, TYPE_BLUCHER)?.is_some() {
            speed = self.constants.blucher_stable_percent.wrapping_mul(speed) / 100;
        }
        if self.type_is(facts.type_id, TYPE_HEAVY_ELEPHANT, false)?
            && self.has_general(row, 0, TYPE_PORUS)?.is_some()
        {
            speed = self.constants.porus_elephant_256.wrapping_mul(speed) / 256;
        }
        if self.has_general(row, 0, TYPE_CHARLES)?.is_some() {
            speed = self.constants.charles_percent.wrapping_mul(speed) / 100;
        }
        if facts.unit_flags & 0x0002_0000 != 0 && self.has_general(row, 0, TYPE_NAPOLEON)?.is_some()
        {
            speed = self.constants.napoleon_siege_percent.wrapping_mul(speed) / 100;
        }
        Ok(speed)
    }
}

/// Resolve every live object in one pure transaction.  Any missing static row, malformed
/// leader counter, incomplete ordered hero registry, invalid identity, or terrain fault aborts
/// the whole result; no partial authority is returned or installed.
pub fn produce_resolved_land_speed_authority<C: LandSpeedContent>(
    sim: &Sim,
    content: &C,
) -> Result<ResolvedLandSpeedAuthority, LandSpeedAuthorityError> {
    let live = sim.world.live_count() as usize;
    if sim.unit_type.len() != live {
        return Err(LandSpeedAuthorityError::UnitTypeRowsUnavailable {
            live,
            rows: sim.unit_type.len(),
        });
    }
    let constants = content
        .land_speed_constants()
        .ok_or(LandSpeedAuthorityError::MissingConstants)?;
    let mut resolver = Resolver {
        sim,
        content,
        constants,
        hash: 0xcbf2_9ce4_8422_2325,
    };
    resolver.mix(content.land_speed_revision());
    for chunk in content.land_speed_composition_digest().chunks_exact(8) {
        resolver.mix(u64::from_le_bytes(chunk.try_into().unwrap()));
    }
    for value in [
        constants.coord_scale,
        constants.irq_spear_bonus,
        constants.irq_mo_spear_bonus,
        constants.irq_hmo_spear_bonus,
        constants.irq_emo_spear_bonus,
        constants.alexander_napoleon_aura_256,
        constants.spitamenes_stable_256,
        constants.porus_elephant_256,
        constants.napoleon_siege_percent,
        constants.charles_percent,
        constants.blucher_stable_percent,
        constants.hero_aura_speed,
    ] {
        resolver.mix_i32(value);
    }
    resolver.validate_leader_counters()?;
    resolver.validate_hero_registries()?;

    let mut entries = Vec::new();
    for row in 0..live {
        let handle = sim
            .world
            .handle_at_row(row)
            .ok_or(LandSpeedAuthorityError::MissingHandle { row })?;
        let who = sim.world.units.get_who(row) as i8;
        if !(0..sim.vic_leaders.slots.len() as i8).contains(&who) {
            return Err(LandSpeedAuthorityError::InvalidOwner { row, who });
        }
        resolver.mix(u64::from(handle.id));
        resolver.mix(u64::from(handle.generation));
        resolver.mix(u64::from(sim.world.units.get_uid(row)));
        resolver.mix(who as u8 as u64);
        resolver.mix(sim.world.units.o()[row] as u16 as u64);
        resolver.mix(u64::from(sim.world.units.get_flags(row)));
        resolver.mix_i32(sim.unit_type[row]);
        resolver.mix_i32(sim.world.units.x_internal()[row]);
        resolver.mix_i32(sim.world.units.y_internal()[row]);
        resolver.mix(sim.world.units.myspeed()[row] as u16 as u64);
        resolver.mix(sim.world.units.o_up()[row] as u16 as u64);
        resolver.mix(sim.world.units.inside_up()[row] as u16 as u64);
        resolver.mix(u64::from(sim.world.units.get_unit_masks(row)));
        let facts = resolver.type_facts(sim.unit_type[row])?;
        if facts.domain != 0 {
            continue;
        }
        let speed = resolver.resolve(row, handle)?;
        entries.push(ResolvedLandSpeedEntry {
            handle,
            uid: sim.world.units.get_uid(row),
            who,
            o: sim.world.units.o()[row],
            type_id: sim.unit_type[row],
            speed,
        });
    }
    entries.sort_by_key(|entry| (entry.handle.id, entry.handle.generation));
    for pair in entries.windows(2) {
        if pair[0].handle == pair[1].handle {
            return Err(LandSpeedAuthorityError::DuplicateResolvedObject {
                handle: pair[0].handle,
            });
        }
    }
    Ok(ResolvedLandSpeedAuthority {
        content_revision: content.land_speed_revision(),
        composition_digest: content.land_speed_composition_digest(),
        state_digest: resolver.hash,
        entries,
    })
}
