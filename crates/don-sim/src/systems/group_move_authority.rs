// SPDX-License-Identifier: GPL-3.0-or-later
//! Product authority producer for the canonical Group → Move transaction.
//!
//! The transaction host deliberately consumes a detached [`GroupMoveAuthority`]. This module
//! joins that value back to its authoritative owners: immutable postload type facts, stable
//! [`Handle`] identities, generated Unit columns, the walked order list/path row, live leader
//! flags/techs, and the checksum-owned terrain map.
//!
//! `UnitData::speed` `0x0060AAE0` is the important fail-closed edge. Non-land types return
//! `myspeed` immediately. Land types continue through terrain, nearby-object, leader-counter,
//! type-predicate, and Constants branches that the reduced Sim does not yet own. A content table
//! therefore cannot turn `MOVES` into an authoritative land speed: a caller must provide an
//! instance-bound resolved value through [`GroupMoveContent::resolved_land_speed`].

use crate::order::OrderIndex;
use crate::systems::air::is_on_map;
use crate::systems::canonical_group_move_host::{GroupMoveAuthority, MoveMemberAuthority};
use crate::systems::groups_guys::FormationMember;
use crate::tick::Sim;
use crate::world::{Handle, OBJ_FLAG_ACTIVE};

/// Exact postload type fields read by `Form::categorize` and the movement admission predicates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GroupMoveTypeFacts {
    pub type_id: i32,
    pub attack: i32,
    pub max_range: i32,
    pub obj_masks: u32,
    pub unit_flags: u32,
    pub unit_flags2: u32,
    pub role: i32,
    pub domain: i32,
    pub age: i32,
    pub guy_spacing: i32,
    pub x_spacing: i32,
    pub y_spacing: i32,
    pub uber_size: i32,
}

/// A resolved land speed is valid only for the exact live object instance named here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedLandSpeed {
    pub handle: Handle,
    pub speed: i32,
}

/// Immutable content plus the still-external, fully evaluated land-speed edge.
///
/// `revision` must change when either the postload type table or the resolved-speed source
/// changes. The produced authority copies it and also hashes every consumed fact.
pub trait GroupMoveContent {
    fn revision(&self) -> u64;
    fn type_facts(&self, type_id: i32) -> Option<GroupMoveTypeFacts>;

    /// Complete result of retail `UnitData::speed` for this exact land Unit instance.
    /// Static `MOVES`, a stale object id, or a renderer selection id are not valid answers.
    fn resolved_land_speed(&self, _handle: Handle) -> Option<ResolvedLandSpeed> {
        None
    }

    /// Effective type chosen by the water-transport branch of `Form::categorize`.
    /// The branch performs a runtime 318/320 choice followed by current-upgrade graft lookup;
    /// returning the original type or a guessed ferry type is not admissible.
    fn water_effective_type(&self, _handle: Handle) -> Option<i32> {
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroupMoveAuthorityError {
    UnitTypeRowsUnavailable { live: usize, rows: usize },
    MissingHandle { row: usize },
    InvalidOwner { row: usize, who: u8 },
    MissingType { row: usize, type_id: i32 },
    TypeIdentityMismatch { requested: i32, supplied: i32 },
    InvalidFormationFacts { type_id: i32 },
    MissingResolvedLandSpeed { handle: Handle, type_id: i32 },
    StaleResolvedLandSpeed { expected: Handle, supplied: Handle },
    MissingWaterEffectiveType { handle: Handle, type_id: i32 },
    MissingContainedEffectiveType { handle: Handle, who: i8, o: i16 },
    MalformedSpecialAnim { handle: Handle },
    DestinationOutsideMap { x: i32, y: i32 },
}

/// `FormData::type_cat` `0x0072DFC0`, specialized to the shipped `UnitType` vtable.
pub fn formation_category(facts: GroupMoveTypeFacts, leader_flags: u32) -> i32 {
    let leader_flag_4 = leader_flags & 4 != 0;
    if facts.attack != 0
        && facts.unit_flags2 & 0x40 == 0
        && facts.unit_flags2 & 8 == 0
        && !matches!(facts.type_id, 0x3d | 0x3e | 400)
    {
        if facts.obj_masks & 4 != 0 {
            return 10;
        }
        if facts.obj_masks & 0x20 != 0 {
            return 3 + i32::from(facts.max_range != 0);
        }
        if facts.unit_flags2 & 4 != 0 || facts.obj_masks & 0x8000_0000 != 0 {
            return 6;
        }
        if facts.obj_masks & 0x1000 != 0 {
            return if facts.max_range != 0 && leader_flag_4 {
                5
            } else {
                2
            };
        }
        return if facts.obj_masks & 0x0020_0000 != 0 {
            0
        } else {
            6
        };
    }
    if facts.unit_flags2 & 0x60 != 0 {
        6
    } else if facts.obj_masks & 4 != 0 {
        10
    } else {
        8
    }
}

fn checked_type<C: GroupMoveContent + ?Sized>(
    content: &C,
    type_id: i32,
) -> Result<GroupMoveTypeFacts, GroupMoveAuthorityError> {
    let facts = content
        .type_facts(type_id)
        .ok_or(GroupMoveAuthorityError::MissingType { row: 0, type_id })?;
    if facts.type_id != type_id {
        return Err(GroupMoveAuthorityError::TypeIdentityMismatch {
            requested: type_id,
            supplied: facts.type_id,
        });
    }
    if facts.guy_spacing <= 0
        || facts.x_spacing <= 0
        || facts.y_spacing <= 0
        || facts.uber_size <= 0
    {
        return Err(GroupMoveAuthorityError::InvalidFormationFacts { type_id });
    }
    Ok(facts)
}

fn formation_member(
    facts: GroupMoveTypeFacts,
    leader_flags: u32,
    modern_infantry: bool,
    width: i32,
    angle: i32,
) -> FormationMember {
    FormationMember {
        category: formation_category(facts, leader_flags),
        x_spacing: facts.x_spacing,
        y_spacing: facts.y_spacing,
        formation_size: facts.uber_size,
        guy_spacing: facts.guy_spacing,
        modern_infantry,
        width,
        angle,
    }
}

fn effective_water_type<C: GroupMoveContent + ?Sized>(
    content: &C,
    handle: Handle,
    facts: GroupMoveTypeFacts,
    unit_masks: u32,
    unit_masks2: u32,
) -> Result<i32, GroupMoveAuthorityError> {
    // Form::categorize's water arm substitutes a leader-grafted ferry for a type that can
    // enter water. The literal 318/320 choice is dynamic; do not infer it from either flag.
    let substitutes = facts.domain == 0
        && ((unit_masks & 0x0080_0000 != 0 && unit_masks2 & 0x2000 == 0)
            || facts.unit_flags & 0x10 != 0);
    if !substitutes {
        return Ok(facts.type_id);
    }
    content
        .water_effective_type(handle)
        .ok_or(GroupMoveAuthorityError::MissingWaterEffectiveType {
            handle,
            type_id: facts.type_id,
        })
}

fn effective_land_type(
    sim: &Sim,
    row: usize,
    handle: Handle,
    facts: GroupMoveTypeFacts,
) -> Result<i32, GroupMoveAuthorityError> {
    if facts.domain != 1 || facts.unit_flags & 0x10 == 0 {
        return Ok(facts.type_id);
    }
    let o = sim.world.units.inside_down()[row];
    if o < 0 {
        return Ok(facts.type_id);
    }
    let who = sim.world.units.inside_down_who()[row];
    let contained_row = usize::try_from(who)
        .ok()
        .and_then(|who| sim.world.unit_row_at(who as i32, i32::from(o)))
        .ok_or(GroupMoveAuthorityError::MissingContainedEffectiveType { handle, who, o })?;
    sim.unit_type
        .get(contained_row)
        .copied()
        .ok_or(GroupMoveAuthorityError::MissingContainedEffectiveType { handle, who, o })
}

fn exact_entering_or_exiting(
    sim: &Sim,
    row: usize,
    handle: Handle,
) -> Result<bool, GroupMoveAuthorityError> {
    let Some(order) = sim.world.orders(row).current() else {
        return Ok(false);
    };
    if order.kind != OrderIndex::SpecialAnim {
        return Ok(false);
    }
    order
        .is_entering_or_exiting()
        .ok_or(GroupMoveAuthorityError::MalformedSpecialAnim { handle })
}

fn push_i32(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_member(bytes: &mut Vec<u8>, member: &MoveMemberAuthority) {
    push_u32(bytes, member.handle.id);
    push_u32(bytes, member.handle.generation);
    push_i32(bytes, member.role);
    for value in [
        member.on_map,
        member.is_captain,
        member.can_move,
        member.can_install_order,
        member.is_plane,
        member.admits_unsplit_move_near,
    ] {
        bytes.push(u8::from(value));
    }
    push_i32(bytes, member.domain);
    push_u32(bytes, member.unit_flags);
    push_i32(bytes, member.speed);
    for formation in [member.land_formation, member.water_formation] {
        push_i32(bytes, formation.category);
        push_i32(bytes, formation.x_spacing);
        push_i32(bytes, formation.y_spacing);
        push_i32(bytes, formation.formation_size);
        push_i32(bytes, formation.guy_spacing);
        bytes.push(u8::from(formation.modern_infantry));
        push_i32(bytes, formation.width);
        push_i32(bytes, formation.angle);
    }
}

fn composition_digest(
    revision: u64,
    destination_is_water: bool,
    members: &[MoveMemberAuthority],
) -> [u8; 32] {
    let mut bytes = Vec::with_capacity(16 + members.len() * 96);
    bytes.extend_from_slice(&revision.to_le_bytes());
    bytes.push(u8::from(destination_is_water));
    for member in members {
        push_member(&mut bytes, member);
    }
    let mut digest = [0u8; 32];
    // Independent initial values keep the whole 32-byte transaction key meaningful without
    // adding a second checksum dependency to don-sim.
    for lane in 0..8 {
        let seed = 1u32.wrapping_add((lane as u32).wrapping_mul(0x1f12_3bb5));
        let word = crate::checksum::adler32(seed, &bytes);
        digest[lane * 4..lane * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    digest
}

/// Rebuild the complete Handle-bound authority immediately before one package is prepared.
///
/// All live active Unit rows are included because the fixed Group allocator may normalize a
/// prior small Group while choosing a slot. Omitting non-selected rows would make allocation
/// behavior depend on an incomplete projection.
pub fn produce_group_move_authority<C: GroupMoveContent + ?Sized>(
    sim: &Sim,
    content: &C,
    destination: (i32, i32),
    force_formation_facing_zero: bool,
) -> Result<GroupMoveAuthority, GroupMoveAuthorityError> {
    if !sim.map.world.valid_coord(destination.0, destination.1) {
        return Err(GroupMoveAuthorityError::DestinationOutsideMap {
            x: destination.0,
            y: destination.1,
        });
    }
    let live = sim.world.live_count() as usize;
    if sim.unit_type.len() < live {
        return Err(GroupMoveAuthorityError::UnitTypeRowsUnavailable {
            live,
            rows: sim.unit_type.len(),
        });
    }
    let destination_is_water = sim
        .map
        .world
        .is_tocean(destination.0 / 192, destination.1 / 192);
    let mut members = Vec::with_capacity(live);
    for row in 0..live {
        if sim.world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
            continue;
        }
        let handle = sim
            .world
            .handle_at_row(row)
            .ok_or(GroupMoveAuthorityError::MissingHandle { row })?;
        let who_raw = sim.world.units.get_who(row);
        let who = usize::try_from(who_raw)
            .ok()
            .filter(|who| *who < sim.vic_leaders.slots.len())
            .ok_or(GroupMoveAuthorityError::InvalidOwner { row, who: who_raw })?;
        let type_id = sim.unit_type[row];
        let facts = checked_type(content, type_id).map_err(|error| match error {
            GroupMoveAuthorityError::MissingType { type_id, .. } => {
                GroupMoveAuthorityError::MissingType { row, type_id }
            }
            other => other,
        })?;
        let leader = &sim.vic_leaders.slots[who];
        let leader_flags = leader.leader_flags as u32;
        let has_tech_0x12 = leader.has_tech.get(0x12).copied().unwrap_or(false);
        let width = i32::from(sim.world.units.form_mod()[row]);
        let angle = sim.world.units.angle()[row];
        let modern_infantry = facts.unit_flags & 0x100 != 0 && (has_tech_0x12 || facts.age > 5);
        let land_type_id = effective_land_type(sim, row, handle, facts)?;
        let land_facts = checked_type(content, land_type_id)?;
        let land_formation =
            formation_member(land_facts, leader_flags, modern_infantry, width, angle);
        let water_type_id = effective_water_type(
            content,
            handle,
            facts,
            sim.world.units.get_unit_masks(row),
            sim.world.units.get_unit_masks2(row),
        )?;
        let water_facts = checked_type(content, water_type_id)?;
        let water_formation =
            formation_member(water_facts, leader_flags, modern_infantry, width, angle);

        let speed = if facts.domain != 0 {
            // First branch of UnitData::speed: non-land returns the live Unit field.
            i32::from(sim.world.units.myspeed()[row])
        } else {
            let resolved = content
                .resolved_land_speed(handle)
                .ok_or(GroupMoveAuthorityError::MissingResolvedLandSpeed { handle, type_id })?;
            if resolved.handle != handle {
                return Err(GroupMoveAuthorityError::StaleResolvedLandSpeed {
                    expected: handle,
                    supplied: resolved.handle,
                });
            }
            resolved.speed
        };
        let inside_up = sim.world.units.inside_up()[row];
        let on_map = is_on_map(inside_up);
        let is_captain = sim.world.units.o_up()[row] < 0;
        let entering_or_exiting = exact_entering_or_exiting(sim, row, handle)?;
        let is_blown = sim.world.units.get_unit_masks(row) & 0x1000 != 0;
        let can_install_order = sim.paths.get(row).is_some();
        let can_move = speed > 0 && on_map && is_captain && !entering_or_exiting && !is_blown;
        let admits_unsplit_move_near =
            can_move && can_install_order && sim.world.units.o_down()[row] < 0;
        members.push(MoveMemberAuthority {
            handle,
            role: facts.role,
            on_map,
            is_captain,
            can_move,
            can_install_order,
            is_plane: facts.domain == 2 && facts.unit_flags & 0x20 == 0,
            domain: facts.domain,
            unit_flags: facts.unit_flags,
            speed,
            admits_unsplit_move_near,
            land_formation,
            water_formation,
        });
    }
    let revision = content.revision();
    let composition_digest = composition_digest(revision, destination_is_water, &members);
    Ok(GroupMoveAuthority {
        revision,
        composition_digest,
        destination_is_water,
        force_formation_facing_zero,
        members,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::order::{Order, SpecialAnimType};
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Source {
        revision: u64,
        types: BTreeMap<i32, GroupMoveTypeFacts>,
        speeds: BTreeMap<(u32, u32), ResolvedLandSpeed>,
        water_types: BTreeMap<(u32, u32), i32>,
    }

    impl GroupMoveContent for Source {
        fn revision(&self) -> u64 {
            self.revision
        }

        fn type_facts(&self, type_id: i32) -> Option<GroupMoveTypeFacts> {
            self.types.get(&type_id).copied()
        }

        fn resolved_land_speed(&self, handle: Handle) -> Option<ResolvedLandSpeed> {
            self.speeds.get(&(handle.id, handle.generation)).copied()
        }

        fn water_effective_type(&self, handle: Handle) -> Option<i32> {
            self.water_types
                .get(&(handle.id, handle.generation))
                .copied()
        }
    }

    fn infantry(type_id: i32) -> GroupMoveTypeFacts {
        GroupMoveTypeFacts {
            type_id,
            attack: 10,
            max_range: 0,
            obj_masks: 0x20,
            role: 0x100,
            domain: 0,
            guy_spacing: 48,
            x_spacing: 96,
            y_spacing: 144,
            uber_size: 1,
            ..GroupMoveTypeFacts::default()
        }
    }

    fn fixture() -> (Sim, Source, Handle) {
        let mut sim = Sim::new(7, 4);
        let handle = sim.spawn_unit(0, 50, 384, 384, 5).unwrap();
        let mut source = Source::default();
        source.revision = 9;
        source.types.insert(50, infantry(50));
        source.speeds.insert(
            (handle.id, handle.generation),
            ResolvedLandSpeed { handle, speed: 27 },
        );
        (sim, source, handle)
    }

    #[test]
    fn type_category_and_leader_flag_arm_are_exact() {
        let mut mounted = infantry(70);
        mounted.obj_masks = 0x1000;
        mounted.max_range = 3;
        assert_eq!(formation_category(mounted, 0), 2);
        assert_eq!(formation_category(mounted, 4), 5);
        mounted.obj_masks = 4;
        assert_eq!(formation_category(mounted, 0), 10);
    }

    #[test]
    fn producer_binds_dynamic_gates_and_digest() {
        let (mut sim, source, handle) = fixture();
        let authority = produce_group_move_authority(&sim, &source, (384, 384), false).unwrap();
        assert_eq!(authority.revision, 9);
        assert_ne!(authority.composition_digest, [0; 32]);
        assert_eq!(authority.members[0].handle, handle);
        assert_eq!(authority.members[0].speed, 27);
        assert!(authority.members[0].can_move);

        let row = sim.world.row_of(handle).unwrap();
        sim.world.units.set_unit_masks(row, 0x1000);
        let blown = produce_group_move_authority(&sim, &source, (384, 384), false).unwrap();
        assert!(!blown.members[0].can_move);
        assert_ne!(authority.composition_digest, blown.composition_digest);

        sim.world.units.set_unit_masks(row, 0);
        sim.world
            .orders_mut(row)
            .push(Order::special_anim(SpecialAnimType::Enter, 0, 0));
        let entering = produce_group_move_authority(&sim, &source, (384, 384), false).unwrap();
        assert!(!entering.members[0].can_move);
    }

    #[test]
    fn land_speed_is_instance_bound_and_reuse_fails_closed() {
        let (mut sim, source, handle) = fixture();
        assert!(produce_group_move_authority(&sim, &source, (384, 384), false).is_ok());
        assert!(sim.world.despawn(handle));
        let replacement = sim.spawn_unit(0, 50, 384, 384, 5).unwrap();
        assert_eq!(replacement.id, handle.id);
        assert_ne!(replacement.generation, handle.generation);
        assert_eq!(
            produce_group_move_authority(&sim, &source, (384, 384), false),
            Err(GroupMoveAuthorityError::MissingResolvedLandSpeed {
                handle: replacement,
                type_id: 50,
            })
        );
    }

    #[test]
    fn malformed_special_anim_and_missing_path_fail_closed() {
        let (mut sim, source, handle) = fixture();
        let row = sim.world.row_of(handle).unwrap();
        sim.world.orders_mut(row).push(Order {
            kind: OrderIndex::SpecialAnim,
            special_anim: None,
            ..Order::default()
        });
        assert_eq!(
            produce_group_move_authority(&sim, &source, (384, 384), false),
            Err(GroupMoveAuthorityError::MalformedSpecialAnim { handle })
        );

        sim.world.orders_mut(row).clear();
        sim.paths.clear();
        let authority = produce_group_move_authority(&sim, &source, (384, 384), false).unwrap();
        assert!(!authority.members[0].can_install_order);
        assert!(!authority.members[0].admits_unsplit_move_near);
    }

    #[test]
    fn effective_type_branches_require_live_containment_and_graft_facts() {
        let (mut sim, mut source, land) = fixture();
        let land_row = sim.world.row_of(land).unwrap();
        sim.world.units.set_unit_masks(land_row, 0x0080_0000);
        assert_eq!(
            produce_group_move_authority(&sim, &source, (384, 384), false),
            Err(GroupMoveAuthorityError::MissingWaterEffectiveType {
                handle: land,
                type_id: 50,
            })
        );
        let mut ferry = infantry(318);
        ferry.obj_masks = 4;
        source.types.insert(318, ferry);
        source.water_types.insert((land.id, land.generation), 318);
        let grafted = produce_group_move_authority(&sim, &source, (384, 384), false).unwrap();
        assert_eq!(grafted.members[0].water_formation.category, 10);

        sim.world.units.set_unit_masks(land_row, 0);
        let mut carrier = infantry(80);
        carrier.domain = 1;
        carrier.unit_flags = 0x10;
        carrier.obj_masks = 4;
        source.types.insert(80, carrier);
        let ship = sim.spawn_unit(0, 80, 576, 576, 5).unwrap();
        let ship_row = sim.world.row_of(ship).unwrap();
        let cargo_o = sim.world.units.o()[land_row];
        sim.world.units.inside_down_mut()[ship_row] = cargo_o;
        sim.world.units.inside_down_who_mut()[ship_row] = 0;
        let contained = produce_group_move_authority(&sim, &source, (384, 384), false).unwrap();
        let ship_member = contained
            .members
            .iter()
            .find(|member| member.handle == ship)
            .unwrap();
        assert_eq!(ship_member.land_formation.category, 3);
        assert_eq!(ship_member.water_formation.category, 10);
    }

    #[test]
    fn load_drops_the_projection_and_rebuilds_from_preserved_identity() {
        let (mut sim, source, handle) = fixture();
        let authority = produce_group_move_authority(&sim, &source, (384, 384), false).unwrap();
        sim.replace_group_move_authority(authority);
        let bytes = crate::systems::save_load::save_sim(&sim).unwrap();
        let loaded = crate::systems::save_load::load_sim(&bytes).unwrap();
        assert!(loaded.group_move_authority.members.is_empty());
        assert_eq!(loaded.world.handle_at_row(0), Some(handle));
        let rebuilt = produce_group_move_authority(&loaded, &source, (384, 384), false).unwrap();
        assert_eq!(rebuilt.members[0].handle, handle);
        assert_eq!(rebuilt.members[0].speed, 27);
    }
}
