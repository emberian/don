use std::collections::BTreeMap;

use don_sim::systems::group_move_authority::{
    produce_group_move_authority, GroupMoveContent, GroupMoveTypeFacts,
};
use don_sim::systems::land_speed_authority::{
    produce_resolved_land_speed_authority, LandSpeedConstants, LandSpeedContent, LandSpeedTypeFacts,
};
use don_sim::systems::step12_visibility_runtime::{
    VisibilityHeroRecord, VisibilityLeaderAuthority, HERO_RECORD_ACTIVE,
};
use don_sim::systems::victory_score::Diplo;
use don_sim::tick::Sim;

#[derive(Clone)]
struct Content {
    revision: u64,
    digest: [u8; 32],
    constants: LandSpeedConstants,
    types: BTreeMap<i32, LandSpeedTypeFacts>,
}

impl Content {
    fn shipped_shape() -> Self {
        Self {
            revision: 9,
            digest: [0x5a; 32],
            constants: LandSpeedConstants {
                coord_scale: 1,
                irq_spear_bonus: 10,
                irq_mo_spear_bonus: 20,
                irq_hmo_spear_bonus: 30,
                irq_emo_spear_bonus: 40,
                alexander_napoleon_aura_256: 384,
                spitamenes_stable_256: 307,
                porus_elephant_256: 307,
                napoleon_siege_percent: 150,
                charles_percent: 120,
                blucher_stable_percent: 120,
                hero_aura_speed: 30,
            },
            types: BTreeMap::new(),
        }
    }

    fn add(&mut self, type_id: i32, from: i32, where_type: i32, flags: u32, flags2: u32) {
        self.types.insert(
            type_id,
            LandSpeedTypeFacts {
                type_id,
                from,
                where_type,
                graft: -1,
                domain: 0,
                unit_flags: flags,
                unit_flags2: flags2,
            },
        );
    }
}

impl LandSpeedContent for Content {
    fn land_speed_revision(&self) -> u64 {
        self.revision
    }

    fn land_speed_composition_digest(&self) -> [u8; 32] {
        self.digest
    }

    fn land_speed_type(&self, type_id: i32) -> Option<LandSpeedTypeFacts> {
        self.types.get(&type_id).copied()
    }

    fn land_speed_constants(&self) -> Option<LandSpeedConstants> {
        Some(self.constants)
    }
}

impl GroupMoveContent for Content {
    fn revision(&self) -> u64 {
        self.revision
    }

    fn type_facts(&self, type_id: i32) -> Option<GroupMoveTypeFacts> {
        let facts = self.types.get(&type_id)?;
        Some(GroupMoveTypeFacts {
            type_id,
            attack: 1,
            max_range: 0,
            obj_masks: 0,
            unit_flags: facts.unit_flags,
            unit_flags2: facts.unit_flags2,
            role: 0,
            domain: facts.domain,
            age: 0,
            guy_spacing: 48,
            x_spacing: 48,
            y_spacing: 48,
            uber_size: 1,
        })
    }
}

fn hero_record(
    sim: &Sim,
    slot: u16,
    handle: don_sim::Handle,
    radius_tiles: i32,
) -> VisibilityHeroRecord {
    let row = sim.world.row_of(handle).unwrap();
    VisibilityHeroRecord {
        slot,
        handle,
        uid: sim.world.units.get_uid(row),
        o: sim.world.units.o()[row],
        who: sim.world.units.get_who(row) as i8,
        hero_flags: HERO_RECORD_ACTIVE,
        type_index: sim.unit_type[row],
        radius_tiles,
    }
}

fn install_heroes(sim: &mut Sim, who: usize, handles: &[don_sim::Handle]) {
    let heroes = handles
        .iter()
        .enumerate()
        .map(|(slot, &handle)| hero_record(sim, slot as u16, handle, 100))
        .collect();
    sim.replace_step12_visibility_leader(
        who,
        VisibilityLeaderAuthority {
            unit_counts: [0; 352],
            heroes,
        },
    )
    .unwrap();
}

#[test]
fn a_live_hero_answers_has_general_directly_before_the_registry_scan() {
    let mut content = Content::shipped_shape();
    content.add(0x171, -1, -1, 0, 0x20);

    let mut sim = Sim::new(19, 8);
    sim.activate(0);
    let actor = sim.spawn_unit(0, 0x171, 1_200, 1_200, 4).unwrap();
    let row = sim.world.row_of(actor).unwrap();
    sim.world.units.myspeed_mut()[row] = 100;
    sim.vic_leaders.slots[0].num_units[(0x171 - 50) as usize] = 1;
    // `ObjectData::has_general` tests is-unit + is-hero before it consults HeroesData.
    // Keep the canonical registry identity present but inactive so only that direct arm can win.
    let mut record = hero_record(&sim, 0, actor, 100);
    record.hero_flags = 0;
    sim.replace_step12_visibility_leader(
        0,
        VisibilityLeaderAuthority {
            unit_counts: [0; 352],
            heroes: vec![record],
        },
    )
    .unwrap();

    let authority = produce_resolved_land_speed_authority(&sim, &content).unwrap();
    assert_eq!(authority.entries[0].speed, 120);
}

#[test]
fn terrain_then_ordered_hero_aura_binds_the_group_move_speed() {
    let mut content = Content::shipped_shape();
    content.add(0x0a6, -1, -1, 0, 0);
    content.add(0x166, -1, -1, 0, 0x20);

    let mut sim = Sim::new(17, 8);
    sim.activate(0);
    sim.activate(1);
    sim.vic_leaders.slots[0].diplos[1] = Diplo::Ally as i32;
    sim.vic_leaders.slots[1].diplos[0] = Diplo::Ally as i32;
    let actor = sim.spawn_unit(0, 0x0a6, 1_000, 1_000, 4).unwrap();
    let hero = sim.spawn_unit(0, 0x166, 1_010, 1_010, 4).unwrap();
    let actor_row = sim.world.row_of(actor).unwrap();
    let hero_row = sim.world.row_of(hero).unwrap();
    sim.world.units.myspeed_mut()[actor_row] = 20;
    sim.world.units.myspeed_mut()[hero_row] = 20;
    sim.world.units.set_unit_masks(hero_row, 0x8000);
    sim.vic_leaders.slots[0].leader_flags |= 0x8000;
    let wx = 1_000 / 0x300;
    let wy = 1_000 / 0x300;
    sim.map.world.wdata_mut(wx, wy).who = 1;
    install_heroes(&mut sim, 0, &[hero]);

    let authority = produce_resolved_land_speed_authority(&sim, &content).unwrap();
    let actor_speed = authority
        .entries
        .iter()
        .find(|entry| entry.handle == actor)
        .unwrap();
    assert_eq!(actor_speed.speed, 45);
    // Terrain first yields 30.  The paired aura is 30*384/256 = 45 and compares against
    // original myspeed 20, proving that it replaces rather than adds to the terrain result.
    let bound = authority.bind(&sim, &content).unwrap();
    let group = produce_group_move_authority(&sim, &bound, (1_500, 1_500), false).unwrap();
    assert_eq!(
        group
            .members
            .iter()
            .find(|member| member.handle == actor)
            .unwrap()
            .speed,
        45
    );

    sim.world.units.set_unit_masks(hero_row, 0);
    assert!(authority.bind(&sim, &content).is_err());
    sim.world.units.set_unit_masks(hero_row, 0x8000);
    sim.vic_leaders.slots[1].diplos[0] = Diplo::War as i32;
    assert!(authority.bind(&sim, &content).is_err());
    sim.vic_leaders.slots[1].diplos[0] = Diplo::Ally as i32;
    sim.vic_leaders.slots[0].leader_flags &= !0x8000;
    assert!(authority.bind(&sim, &content).is_err());
}

#[test]
fn five_general_modifiers_follow_the_retail_sequence() {
    let mut content = Content::shipped_shape();
    // A synthetic derived elephant whose direct `where` is Stable and whose type virtual is
    // siege.  This lets one actor exercise every sequential branch without pre-answering any
    // predicate: the relation walk and flag test still supply the retail answers.
    content.add(100, 0x105, 0x1ac, 0x0002_0000, 0);
    content.add(0x105, -1, -1, 0, 0);
    for type_id in [0x16d, 0x175, 0x16f, 0x171, 0x167] {
        content.add(type_id, -1, -1, 0, 0x20);
    }

    let mut sim = Sim::new(23, 8);
    sim.activate(0);
    let actor = sim.spawn_unit(0, 100, 2_000, 2_000, 4).unwrap();
    let actor_row = sim.world.row_of(actor).unwrap();
    sim.world.units.myspeed_mut()[actor_row] = 100;
    let mut heroes = Vec::new();
    for (i, type_id) in [0x16d, 0x175, 0x16f, 0x171, 0x167].into_iter().enumerate() {
        let hero = sim
            .spawn_unit(0, type_id, 2_010 + i as i32, 2_010, 4)
            .unwrap();
        let index = (type_id - 50) as usize;
        sim.vic_leaders.slots[0].num_units[index] = 1;
        heroes.push(hero);
    }
    install_heroes(&mut sim, 0, &heroes);

    let authority = produce_resolved_land_speed_authority(&sim, &content).unwrap();
    assert_eq!(
        authority
            .entries
            .iter()
            .find(|entry| entry.handle == actor)
            .unwrap()
            .speed,
        306
    );
    // 100 -> *307/256=119 -> *120%=142 -> *307/256=170 -> *120%=204 -> *150%=306.
}

#[test]
fn terrain_content_and_generation_mutations_all_reject_the_snapshot() {
    let mut content = Content::shipped_shape();
    content.add(0x0a6, -1, -1, 0, 0);
    let mut sim = Sim::new(31, 8);
    sim.activate(0);
    sim.activate(1);
    sim.vic_leaders.slots[0].diplos[1] = Diplo::Ally as i32;
    sim.vic_leaders.slots[1].diplos[0] = Diplo::Ally as i32;
    let old = sim.spawn_unit(0, 0x0a6, 900, 900, 4).unwrap();
    sim.map.world.wdata_mut(900 / 0x300, 900 / 0x300).who = 1;
    let authority = produce_resolved_land_speed_authority(&sim, &content).unwrap();
    assert!(authority.bind(&sim, &content).is_ok());

    sim.map.world.wdata_mut(900 / 0x300, 900 / 0x300).who = -1;
    assert!(authority.bind(&sim, &content).is_err());
    sim.map.world.wdata_mut(900 / 0x300, 900 / 0x300).who = 1;

    let old_row = sim.world.row_of(old).unwrap();
    sim.world.units.myspeed_mut()[old_row] += 1;
    assert!(authority.bind(&sim, &content).is_err());
    sim.world.units.myspeed_mut()[old_row] -= 1;

    let mut changed_content = content.clone();
    changed_content.revision += 1;
    assert!(authority.bind(&sim, &changed_content).is_err());
    let mut changed_constants = content.clone();
    changed_constants.constants.irq_spear_bonus += 1;
    assert!(authority.bind(&sim, &changed_constants).is_err());
    let mut changed_relation = content.clone();
    changed_relation.types.get_mut(&0x0a6).unwrap().graft = 0x0a7;
    assert!(authority.bind(&sim, &changed_relation).is_err());

    assert!(sim.world.despawn(old));
    sim.unit_type.pop();
    sim.paths.pop();
    sim.path_unit.pop();
    sim.crash_units.pop();
    let replacement = sim.spawn_unit(0, 0x0a6, 900, 900, 4).unwrap();
    assert_eq!(replacement.id, old.id);
    assert_ne!(replacement.generation, old.generation);
    assert!(authority.bind(&sim, &content).is_err());
}
