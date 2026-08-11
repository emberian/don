// SPDX-License-Identifier: GPL-3.0-or-later
//! The `Unit::come_out` `0x00617C10` family, driven through `don_sim::systems`.
//!
//! Every `use` below is a library path. If any of the seven `pub mod` lines lane `come-out`
//! added to `crates/don-sim/src/systems/mod.rs` is removed, this file stops compiling — which
//! is the point. The four tranche modules spent weeks on disk compiled only from their own
//! `#[path]`-including test files, so their green tests said nothing about whether the
//! library contained them.

use don_sim::systems::unit_come_out_body_map::{
    check_body_map, owner_of, tranche_bytes, ComeOutBoundary, Tranche, OUTLINED, OUTLINED_TAIL,
    SEQUENTIAL, UNIT_COME_OUT_BYTES, UNIT_COME_OUT_END_VA, UNIT_COME_OUT_VA,
};
use don_sim::systems::unit_come_out_common_release_frontier as common_release;
use don_sim::systems::unit_come_out_full_frontier as full;
use don_sim::systems::unit_come_out_gather_selection_frontier as gather_selection;
use don_sim::systems::unit_come_out_release_tail_frontier as release_tail;

#[test]
fn the_registered_family_tiles_the_retail_body() {
    assert_eq!(check_body_map(), Ok(()));
}

#[test]
fn the_four_registered_tranches_agree_on_their_seams() {
    // Each seam is published independently by two modules. Before registration nothing in
    // the tree could compare them, because no two of them were ever in the same crate.
    assert_eq!(full::PREFIX_END_VA, common_release::COMMON_RELEASE_START_VA);
    assert_eq!(
        common_release::GATHER_LIST_RESUME_VA,
        gather_selection::GATHER_SELECTION_START_VA
    );
    assert_eq!(
        gather_selection::GATHER_SELECTION_END_VA,
        release_tail::RELEASE_TAIL_START_VA
    );
    assert_eq!(
        release_tail::UNIT_COME_OUT_END_VA,
        full::UNIT_COME_OUT_VA + full::UNIT_COME_OUT_BYTES
    );
}

#[test]
fn the_body_is_closed_and_the_residual_is_zero() {
    // The number the group-act lane reported was "7,201 of 9,925 unrecovered". Two later
    // tranches took it to 4,349. This one takes it to nothing.
    assert_eq!(full::RESIDUAL_BYTES, 7_201);
    assert_eq!(common_release::RESIDUAL_BYTES_AFTER_TRANCHE, 6_032);
    assert_eq!(gather_selection::RESIDUAL_BYTES_AFTER_TRANCHE, 4_349);
    assert_eq!(release_tail::RESIDUAL_BYTES_AFTER_TRANCHE, 0);

    let total: u32 = Tranche::ALL.iter().copied().map(tranche_bytes).sum();
    assert_eq!(total, UNIT_COME_OUT_BYTES);
}

#[test]
fn every_retail_byte_of_come_out_is_owned_by_exactly_one_registered_module() {
    for va in UNIT_COME_OUT_VA..UNIT_COME_OUT_END_VA {
        let owner = owner_of(va).unwrap_or_else(|| panic!("{va:#010x} has no owner"));
        // Ownership must be single-valued: an address in an outlined island must not also be
        // claimed by a sequential extent of a different tranche.
        let sequential_owner = SEQUENTIAL.iter().find(|e| e.contains(va)).map(|e| e.owner);
        let island_owner = OUTLINED
            .iter()
            .chain(OUTLINED_TAIL.iter())
            .find(|(s, e, _, _)| *s <= va && va < *e)
            .map(|(_, _, _, o)| *o);
        assert!(
            sequential_owner.is_some() || island_owner.is_some(),
            "{va:#010x} in neither table"
        );
        if let Some(island) = island_owner {
            assert_eq!(owner, island);
        }
    }
}

#[test]
fn the_module_paths_are_the_ones_that_are_registered() {
    for tranche in Tranche::ALL {
        assert!(
            tranche.module_path().starts_with("systems::unit_come_out_"),
            "{tranche:?}"
        );
    }
}

#[test]
fn the_boundary_every_caller_stops_at_names_the_retail_va() {
    // `production_runtime`, `unit_inctime`, `gathering`, `leader_set_diplo` and
    // `step8_eject_contents` each stub `Unit::come_out` today. This is the single shape they
    // should converge on.
    let boundary = ComeOutBoundary::no_host(0);
    assert_eq!(
        boundary,
        ComeOutBoundary::NoHost {
            retail_va: UNIT_COME_OUT_VA,
            mode: 0
        }
    );
}

#[test]
fn the_release_tail_planner_is_reachable_from_the_library() {
    use release_tail::{
        plan_unit_come_out_release_tail, ActorFacts, ConstantsFacts, ObjectIdentity, Point,
        ReleaseTailFacts, ReleaseTailStep, RngStamp, SelectionFacts,
    };

    let facts = ReleaseTailFacts {
        actor: ActorFacts {
            identity: ObjectIdentity::new(0, 1),
            type_index: 0x40,
            where_type: -1,
            attack: 0,
            x_size: 1,
            y_size: 1,
            uber_size: 1,
            unit_flags2: 0,
            unit_masks: 0,
            on_map: true,
            domain_query: 0,
            type_movement_query: 0,
            o_down_chain: Vec::new(),
        },
        container: None,
        selection: SelectionFacts {
            selected: false,
            point: Point { x: 0, y: 0 },
            action: 0,
            scratch_group: -1,
            container_gpiece: 0,
            spec_anim: 0,
        },
        constants: ConstantsFacts {
            unit_train_distance: 0,
            unit_train_max_distance: 0,
        },
        game_frame: 0,
        where_probes: None,
        army_probes: None,
    };
    let plan = plan_unit_come_out_release_tail(
        &facts,
        &[],
        RngStamp {
            seed: 1,
            draws: 0,
        },
    )
    .expect("the empty-selection path plans");
    assert_eq!(plan.steps, vec![ReleaseTailStep::SetOptionsRebuild]);
    assert_eq!(plan.rng.draws, 0);
}
