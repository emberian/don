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
use crate::systems::{economy, leaders, production};
use crate::tick::{Sim, NUM_LEADERS};
use crate::world::OBJ_FLAG_ACTIVE;

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

    let expected_flags = if sim.vic_leaders.setup_owner.is_configured(who) {
        fresh.flags | leaders::flag::IN_GAME | leaders::flag::PROCESS
    } else {
        fresh.flags
    };

    (mirror_is_empty || mirror_is_synchronized)
        && (policy_is_empty || policy_is_synchronized)
        && actual.flags == expected_flags
        && actual.slot == fresh.slot
        && actual.diplo == fresh.diplo
        && actual.taunt_kind == fresh.taunt_kind
        && actual.taunt_arg == fresh.taunt_arg
        && actual.taunt_frame == fresh.taunt_frame
        && actual.timers == fresh.timers
        && actual.retake_scale == fresh.retake_scale
        && (population_cap_is_empty || population_cap_is_synchronized)
        && actual.pop_issues == fresh.pop_issues
        && actual.frame_counter_b == fresh.frame_counter_b
        && actual.attrition == fresh.attrition
        && actual.anti_attrition.to_bits() == fresh.anti_attrition.to_bits()
        && actual.explored == fresh.explored
        && actual.event_frame == fresh.event_frame
        && actual.conquest_byte == fresh.conquest_byte
        && actual.rare_effective == fresh.rare_effective
        && actual.rare_a == fresh.rare_a
        && actual.rare_b == fresh.rare_b
        && actual.unit_stats == fresh.unit_stats
}

fn expected_unit_view(sim: &Sim, row: u32) -> Option<leaders::StatObject> {
    let row = row as usize;
    Some(leaders::StatObject {
        active: (*sim.world.units.flags().get(row)? as u8) & OBJ_FLAG_ACTIVE != 0,
        captain: sim.world.units.o_up().get(row).copied()? < 0,
        unit_query_source: Some(leaders::UnitQuerySource {
            type_id: *sim.unit_type.get(row)?,
            unit_masks2: *sim.world.units.unit_masks2().get(row)? as u32,
        }),
        owner_in_game: false,
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

fn expected_build_view(sim: &Sim, row: u32) -> leaders::StatObject {
    let Some(build) = sim.builds.get(row as usize) else {
        return leaders::StatObject::default();
    };
    leaders::StatObject {
        active: build.is_valid(),
        wall_active: build.is_active(),
        wall_started: build.flags & production::flag::STARTED != 0,
        wall_city_flag: build.flags & production::flag::CAPTURED != 0,
        owner_in_game: false,
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
            .all(|(view, &row)| expected_unit_view(sim, row).as_ref() == Some(view))
        {
            return false;
        }
        if !actual
            .band_2000
            .iter()
            .zip(build_rows)
            .all(|(view, &row)| *view == expected_build_view(sim, row))
        {
            return false;
        }
    }
    true
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

    let fresh = leaders::Leaders::new();
    if sim.step8.end != fresh.end || sim.step8.event != fresh.event {
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
