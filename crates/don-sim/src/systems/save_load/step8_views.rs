//! Save admission for step-8's synchronized views.
//!
//! `Sim::sync_step8_inputs` materializes a second layout immediately before retail step 8:
//! economy values are copied from `LeaderSlot`, population cap and attrition policy from
//! the canonical `victory_score::LeaderState`, and unit/build views from the authoritative
//! `World`/`BuildData` stores. An inactive leader never consumes or mutates those views.
//! They are therefore reconstructible adapter state, not another save owner. This validator
//! admits only the constructor-empty form, canonical PlayerSetup activation, or the exact
//! post-sync form; every query package, persistent call counter, and non-default external host
//! remains refused.

use crate::objects::Band;
use crate::systems::{
    economy, leader_process_taunt, leader_production_ai::strategy_runtime::CanonicalProductionAi,
    leaders, production,
};
use crate::tick::{Sim, NUM_LEADERS};
use crate::world::OBJ_FLAG_ACTIVE;

// `Build::init` and `Wall::activate(0, 1, 0)` leave these two inbound LeaderData dirty
// bits set together. They are also written to the checksum-owned victory Leader row by the
// canonical Farm transaction. Keeping this mask local to save admission avoids treating an
// arbitrary victory/lifecycle flag as a reconstructible step-8 byte.
const WALL_ACTIVATION_DIRTY: u32 = 0x0200_0000 | leaders::flag::WALL_STATS_DIRTY;

fn wall_activation_after_image(sim: &Sim, who: usize) -> Option<(u32, i32)> {
    let flags = sim.vic_leaders.slots[who].leader_flags as u32;
    let allowed = leaders::flag::IN_GAME | leaders::flag::PROCESS | WALL_ACTIVATION_DIRTY;
    (flags & WALL_ACTIVATION_DIRTY == WALL_ACTIVATION_DIRTY && flags & !allowed == 0)
        .then(|| (flags, sim.cities.count(who)))
}

fn leader_has_only_mirrors(
    actual: &leaders::Leader,
    fresh: &leaders::Leader,
    sim: &Sim,
    who: usize,
) -> bool {
    let facade = &sim.leaders[who];
    let mirror_is_empty = actual.econ == fresh.econ
        && actual.last_calc_frame == fresh.last_calc_frame
        && actual.econ_dirty == fresh.econ_dirty;
    let mirror_is_synchronized = actual.econ == facade.econ
        && actual.last_calc_frame == facade.last_calc_frame
        && actual.econ_dirty == facade.dirty;
    let policy = &sim.vic_leaders.slots[who];
    let policy_is_empty = actual.attrition_off == fresh.attrition_off
        && actual.anti_attrition_off == fresh.anti_attrition_off
        && actual.neutral_attrition == fresh.neutral_attrition
        && actual.building_attrition_off == fresh.building_attrition_off;
    let policy_is_synchronized = actual.attrition_off == policy.give_attrition_disabled
        && actual.anti_attrition_off == policy.take_attrition_disabled
        && actual.neutral_attrition == policy.neutral_attrition
        && actual.building_attrition_off == policy.building_attrition_disabled;
    let population_cap_is_empty = actual.pop_cap == fresh.pop_cap;
    let population_cap_is_synchronized = actual.pop_cap == policy.population_cap;
    let canonical_ai = CanonicalProductionAi {
        leader_flags2: policy.leader_flags2,
        production_step: policy.production_step,
        prod_script_run: policy.prod_script_run,
        script_step: policy.script_step,
        control: policy.control,
        effective_pop: policy.effective_pop,
    };
    let ai_is_empty = actual.ai == fresh.ai;
    let ai_is_synchronized = CanonicalProductionAi::capture(&actual.ai) == canonical_ai
        && actual.ai.pers_arg == fresh.ai.pers_arg
        && actual.ai.queued_units == fresh.ai.queued_units
        && actual.ai.make_list_head == fresh.ai.make_list_head
        && actual.ai.script_result == fresh.ai.script_result
        && actual.ai.make_stuff_result == fresh.ai.make_stuff_result;

    let setup_flags = if sim.vic_leaders.setup_owner.is_configured(who) {
        fresh.flags | leaders::flag::IN_GAME | leaders::flag::PROCESS
    } else {
        fresh.flags
    };
    let activation_after_image = wall_activation_after_image(sim, who);
    let expected_flags = activation_after_image
        .map(|(flags, _)| flags)
        .unwrap_or(setup_flags);
    let expected_city_num = activation_after_image
        .map(|(_, city_num)| city_num)
        .unwrap_or(fresh.city_num);

    (mirror_is_empty || mirror_is_synchronized)
        && (policy_is_empty || policy_is_synchronized)
        && actual.flags == expected_flags
        && actual.slot == fresh.slot
        && crate::systems::canonical_diplomacy_runtime::step8_diplomacy_view_matches(sim, who)
        && actual.taunt_kind == fresh.taunt_kind
        && actual.taunt_arg == fresh.taunt_arg
        && actual.taunt_frame == fresh.taunt_frame
        && actual.timers == fresh.timers
        && actual.retake_scale == fresh.retake_scale
        && (population_cap_is_empty || population_cap_is_synchronized)
        && actual.pop_issues == fresh.pop_issues
        && actual.frame_counter_b == fresh.frame_counter_b
        && (ai_is_empty || ai_is_synchronized)
        && actual.attrition == fresh.attrition
        && actual.anti_attrition.to_bits() == fresh.anti_attrition.to_bits()
        && actual.explored == fresh.explored
        && actual.event_frame == fresh.event_frame
        && actual.conquest_byte == fresh.conquest_byte
        && actual.rare_effective == fresh.rare_effective
        && actual.rare_a == fresh.rare_a
        && actual.rare_b == fresh.rare_b
        && actual.unit_stats == fresh.unit_stats
        // The exact activation after-image can reconstruct `city_num` from the saved
        // CityPool. Outside that boundary it remains an unsupported independent host.
        && actual.city_num == expected_city_num
        && actual.build_stats == fresh.build_stats
}

fn expected_owner_in_game(sim: &Sim, who: usize) -> bool {
    sim.step8.leaders[who].flags & leaders::flag::IN_GAME != 0
}

fn expected_unit_view(sim: &Sim, who: usize, row: u32) -> Option<leaders::StatObject> {
    let row = row as usize;
    Some(leaders::StatObject {
        active: (*sim.world.units.flags().get(row)? as u8) & OBJ_FLAG_ACTIVE != 0,
        captain: sim.world.units.o_up().get(row).copied()? < 0,
        unit_query_source: Some(leaders::UnitQuerySource {
            type_id: *sim.unit_type.get(row)?,
            unit_masks2: *sim.world.units.unit_masks2().get(row)? as u32,
        }),
        // `Sim::sync_step8_inputs` derives this query answer from the exact leader flags
        // immediately before dispatch. It is not an independently saved object field.
        owner_in_game: expected_owner_in_game(sim, who),
        myhits: *sim.world.units.myhits().get(row)?,
        mylos: *sim.world.units.mylos().get(row)?,
        o_down: {
            let down = *sim.world.units.o_down().get(row)?;
            (down >= 0).then_some(down as usize)
        },
        myspeed: *sim.world.units.myspeed().get(row)?,
        myarmor: *sim.world.units.myarmor().get(row)?,
        ..leaders::StatObject::default()
    })
}

fn expected_build_view(sim: &Sim, who: usize, row: u32) -> leaders::StatObject {
    let Some(build) = sim.builds.get(row as usize) else {
        return leaders::StatObject::default();
    };
    leaders::StatObject {
        active: build.is_valid(),
        wall_active: build.is_active(),
        wall_started: build.flags & production::flag::STARTED != 0,
        wall_city_flag: build.flags & production::flag::CAPTURED != 0,
        owner_in_game: expected_owner_in_game(sim, who),
        myhits: build.myhits,
        mylos: build.other[0x3c] as i8,
        job_counter: build.job_counter,
        constr_time: build.constr_time,
        construct_hits: build.construct_hits,
        damage: build.damage,
        inside_down: i16::from_le_bytes([build.other[0x28], build.other[0x29]]),
        ..leaders::StatObject::default()
    }
}

fn objects_are_empty(sim: &Sim) -> bool {
    sim.step8_env.leaders.iter().all(|env| {
        env.objects.units.is_empty()
            && env.objects.band_2000.is_empty()
            && env.objects.band_3000.is_empty()
    })
}

fn objects_are_exact_mirrors(sim: &Sim) -> bool {
    for who in 0..NUM_LEADERS {
        let registry = sim.world.objects.slot(who);
        let actual = &sim.step8_env.leaders[who].objects;
        let unit_rows = registry.band(Band::Unit);
        let build_rows = registry.band(Band::Build);

        if !registry.band(Band::Wall).is_empty()
            || !actual.band_3000.is_empty()
            || actual.units.len() != unit_rows.len()
            || actual.band_2000.len() != build_rows.len()
        {
            return false;
        }
        if !actual
            .units
            .iter()
            .zip(unit_rows)
            .all(|(view, &row)| expected_unit_view(sim, who, row).as_ref() == Some(view))
        {
            return false;
        }
        if !actual
            .band_2000
            .iter()
            .zip(build_rows)
            .all(|(view, &row)| *view == expected_build_view(sim, who, row))
        {
            return false;
        }
    }
    true
}

fn end_is_empty_or_exact_mirror(sim: &Sim, fresh: &leaders::Leaders) -> bool {
    if sim.step8.end == fresh.end {
        return true;
    }
    let mut expected = fresh.end.clone();
    for who in 0..NUM_LEADERS {
        expected.players[who].who = sim.step8.leaders[who].slot as u8;
        expected.players[who].flags = if expected_owner_in_game(sim, who) {
            leaders::PLAYER_VALID
        } else {
            0
        };
    }
    sim.step8.end == expected
}

/// Whether step 8 contains no state that needs its own save owner.
pub(super) fn is_supported_derived_snapshot(sim: &Sim) -> bool {
    if sim.step8_rules != leaders::Step8Rules::shipped() {
        return false;
    }
    // The local post-load type catalog is an external runtime input. DoNSave has no chunk for
    // its two provenance digests or admitted rows, so accepting it here would reload a Sim that
    // answers the same dirty-edge query differently.
    if sim.step8_env.unit_type_stats.is_some() {
        return false;
    }
    // `Console::who`, `GameInfo::team_style`, `LeaderData::type_avail`/`is_neutral` and the
    // `internal_random` draw queue are external runtime answers with no DoNSave chunk, in
    // the same position as the type catalog above: accepting them would reload a `Sim` that
    // answers a taunt dispatch differently.
    if sim.step8_env.taunt != leader_process_taunt::TauntEnv::default() {
        return false;
    }

    let fresh = leaders::Leaders::new();
    if !end_is_empty_or_exact_mirror(sim, &fresh) || sim.step8.event != fresh.event {
        return false;
    }
    for who in 0..NUM_LEADERS {
        if !leader_has_only_mirrors(&sim.step8.leaders[who], &fresh.leaders[who], sim, who) {
            return false;
        }
        let env = &sim.step8_env.leaders[who];
        if !super::gather_inputs_are_default(&env.gather)
            || env.caps != economy::CapGates::default()
            || env.payout != economy::DoGatherContext::default()
            || env.attrition != leaders::AttritionGates::default()
        {
            return false;
        }
    }

    objects_are_empty(sim) || objects_are_exact_mirrors(sim)
}

/// Rebuild the save-supported step-8 adapter after all canonical owners have loaded.
///
/// The ordinary sync restores economy, diplomacy, policy, AI and object query mirrors. The
/// one persistent pre-step-8 edge that sync deliberately does not overwrite is the exact
/// `Build::init`/`Wall::activate` dirty after-image; its flags live in the saved victory
/// Leader row and its `city_num` is the saved CityPool's live count.
pub(super) fn restore_supported_derived_snapshot(sim: &mut Sim) {
    for who in 0..NUM_LEADERS {
        if let Some((flags, city_num)) = wall_activation_after_image(sim, who) {
            sim.step8.leaders[who].flags = flags;
            sim.step8.leaders[who].city_num = city_num;
        }
    }
    sim.sync_step8_inputs();
}
