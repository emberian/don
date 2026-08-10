// SPDX-License-Identifier: GPL-3.0-or-later

use don_ai::arena::match_run::{load_world, MatchConfig};
use don_ai::arena::world::{
    AuthoritativeFarmSiteError, AuthoritativeFarmSiteFacts, AuthoritativeFarmSiteReceipt, World,
};
use don_ai::arena::{Cmd, EntId};
use don_ai::OrderResult;
use don_sim::systems::gather_terrain::{GatherTerrainSourceStamp, SUPPORTED_RULES_XML_SHA256};
use don_sim::systems::victory_score::Diplo;

const RULES_XML: &[u8] = include_bytes!("../../../ron-data/rules.xml");

fn starting_farm_and_citizen(w: &World) -> (EntId, EntId) {
    let farm = w
        .own_ents(0)
        .find(|ent| ent.type_id == w.ids.farm)
        .expect("the shipped Small Town start has completed Farms")
        .id;
    let citizen = w
        .own_ents(0)
        .find(|ent| ent.type_id == w.ids.citizen)
        .expect("the shipped Small Town start has citizens")
        .id;
    (farm, citizen)
}

fn retain_sources(w: &mut World) {
    w.map
        .retain_gather_terrain_sources(
            RULES_XML.to_vec(),
            GatherTerrainSourceStamp {
                installed_rules_sha256: SUPPORTED_RULES_XML_SHA256,
                world_seed: w.map.seed as i32,
                coherent_generation: true,
            },
            Vec::new(),
            Vec::new(),
        )
        .expect("supported rules and the live Arena world share one identity");
}

fn authoritative_world() -> Option<(World, EntId, EntId, AuthoritativeFarmSiteReceipt)> {
    let mut w = load_world(&MatchConfig::default()).ok()?;
    retain_sources(&mut w);
    w.diplomacy
        .write_declaration_state_only(0, 1, Diplo::Ally)
        .ok()?;
    w.diplomacy
        .write_declaration_state_only(1, 0, Diplo::Ally)
        .ok()?;
    let (farm, citizen) = starting_farm_and_citizen(&w);
    let receipt = w
        .bind_authoritative_farm_site(farm, AuthoritativeFarmSiteFacts::default())
        .expect("retained terrain admits the shared shipped Farm evaluator");
    Some((w, farm, citizen, receipt))
}

#[test]
fn generated_map_cannot_cross_the_farm_authority_boundary() {
    let Ok(mut w) = load_world(&MatchConfig::default()) else {
        return;
    };
    let (farm, _) = starting_farm_and_citizen(&w);
    let worker_cap_before = w.ent(farm).expect("Farm is live").worker_cap;

    assert_eq!(
        w.bind_authoritative_farm_site(farm, AuthoritativeFarmSiteFacts::default()),
        Err(AuthoritativeFarmSiteError::MissingRetailTerrainSources)
    );
    assert_eq!(
        w.ent(farm)
            .expect("failed binding leaves Farm live")
            .worker_cap,
        worker_cap_before,
        "failed authority preflight is transactional"
    );
}

#[test]
fn retained_farm_owns_capacity_attachment_occupancy_and_evaluated_gross() {
    let Some((mut low_rate, farm, citizen, receipt)) = authoritative_world() else {
        return;
    };
    let Some((mut high_rate, high_farm, high_citizen, high_receipt)) = authoritative_world() else {
        return;
    };
    assert_eq!(receipt, high_receipt);
    assert_eq!(receipt.site, farm);
    assert_eq!(receipt.capacity, 1);
    assert_eq!(
        receipt.footprint_resources, [0; 6],
        "Arena's generated terrain has no admitted retail LandData plane"
    );
    assert_eq!(
        receipt.per_worker_gross, [0; 6],
        "the exact evaluator preserves a zero retail footprint instead of substituting MODEL yield"
    );

    assert_eq!(
        low_rate.submit(
            0,
            Cmd::Gather {
                unit: citizen,
                target: farm,
            },
        ),
        OrderResult::Ok(1)
    );
    assert_eq!(
        high_rate.submit(
            0,
            Cmd::Gather {
                unit: high_citizen,
                target: high_farm,
            },
        ),
        OrderResult::Ok(1)
    );
    assert_eq!(low_rate.ent(farm).expect("Farm is live").workers, 1);
    assert_eq!(
        low_rate
            .ent(citizen)
            .expect("attached citizen is live")
            .assigned_to,
        farm
    );
    assert_eq!(
        low_rate.submit(
            0,
            Cmd::Gather {
                unit: citizen,
                target: farm,
            },
        ),
        OrderResult::Ok(1),
        "reissuing the exact Farm order retires and reattaches the same worker"
    );
    assert_eq!(low_rate.ent(farm).expect("Farm is live").workers, 1);

    // The authoritative branch composes the retained evaluator output with active
    // occupancy. The old MODEL PEASANT_RATE must be observationally irrelevant.
    low_rate.types.constants.peasant_rate = 1;
    high_rate.types.constants.peasant_rate = 1_000_000;
    for _ in 0..4 {
        low_rate.step();
        high_rate.step();
    }
    assert_eq!(low_rate.players[0].stock, high_rate.players[0].stock);
    assert_eq!(low_rate.players[0].acc, high_rate.players[0].acc);
    assert_eq!(low_rate.players[0].gathered, high_rate.players[0].gathered);
    assert_eq!(
        low_rate.submit(0, Cmd::Halt { unit: citizen }),
        OrderResult::Ok(1)
    );
    assert_eq!(low_rate.ent(farm).expect("Farm remains live").workers, 0);
    assert_eq!(
        low_rate
            .ent(citizen)
            .expect("retired citizen remains live")
            .assigned_to,
        EntId::NONE
    );
}
