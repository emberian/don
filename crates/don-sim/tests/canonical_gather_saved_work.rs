#[path = "../src/systems/canonical_gather_work.rs"]
mod canonical_gather_work;

use std::collections::BTreeSet;
use std::fs;

use canonical_gather_work::*;
use don_sim::order::{Order, OrderIndex};
use don_sim::systems::economy_order_payload_authority::{
    EconomyOrderHeader, EconomyOrderNode, EconomyOrderPayload, GatherOrderPayload,
    StableTargetIdentity,
};
use don_sim::systems::{production, save_load};
use don_sim::tick::Sim;

/// Exact 31-byte payloads and SHA-256 values from the fresh-SVX full Unit census.
const FRESH_GATHERS: &[(u8, i16, &str, &str)] = &[
    (
        0,
        3,
        "00d10700000000000001000e01000074000000a20100002701000000010401",
        "3d4d8ca9d0bd71b0f4af4416425953cd82c0fa5c452a5fba1a5e18f50615f0f2",
    ),
    (
        0,
        4,
        "00d10700000000000001000e01000072000000a2010000d701000000010401",
        "ee2428e332615c830cf9e6c4a06630d13e49686b37b9a07c00166a2b6b11194e",
    ),
    (
        0,
        6,
        "00d3070000000000000300ffffffffffffffffa10100000000000001000001",
        "85cd4324ee00922c840d5ea974a3ace8efcfdad2961b3a8049fac9671778f23e",
    ),
    (
        0,
        7,
        "00d4070000000000000400ffffffffffffffffa10100000000000001000001",
        "8a9c12ee3cb8894ffc4b1b15c946bd904b3742abb0ba914bb3d0df17fd3c7f77",
    ),
    (
        0,
        8,
        "00d2070000000000000200ffffffffffffffffa10100000000000001000001",
        "24e77ef2f7584057a6682a7c7255c9fe6aeca1b1579d58901eb37bc51f478043",
    ),
    (
        0,
        9,
        "00d10700000000000001000e01000071000000a20100009100000000010401",
        "2f51227e20951ac115445c289fe2016aaaf3f754ba62ece746580d50259e4bd7",
    ),
    (
        0,
        10,
        "00d8070000000000001700ffffffffffffffffa10100000000000001000001",
        "32d9ba145f863b5b9946540fa95939ffdb4c48153aac7e8565f705ec19563278",
    ),
    (
        1,
        2,
        "00d1070000010000000100890000000a000000a2010000c601000000010401",
        "3a2b6ace2296cd90035afa63fd49f6f93b12163880793988257a8006c71b8fe6",
    ),
    (
        1,
        3,
        "00d2070000010000000200ffffffffffffffffa10100000000000001000001",
        "a550e81535c305dc48e4a65cbd3f127b1f9e0701bf16f85644a1f60777708cb9",
    ),
    (
        1,
        4,
        "00d3070000010000000300ffffffffffffffffa10100000000000001000001",
        "91649de6ff0e80aaa00ece4a31dc7c2b58b6a09a09d0c489043d520a1caed9e3",
    ),
    (
        1,
        5,
        "00d4070000010000000400ffffffffffffffffa10100000000000001000001",
        "d329f9281993d88c6092b0c4b47e85ac4b8dffee9d2f6d8c29236bb592bcfaca",
    ),
    (
        1,
        6,
        "00d10700000100000001008800000007000000a20100005400000000010401",
        "76dbc21b39dd68697722e21bc428cf390d7eb2f3b38b45bcdcca55c087a89ff2",
    ),
    (
        1,
        7,
        "00d1070000010000000100880000000a000000a20100004b01000000010401",
        "81caf3b9ef8499b9af9d92628e198abc2ab34fefe85af01a331627ce59183874",
    ),
    (
        1,
        8,
        "00d6070000010000000c00ffffffffffffffffa10100000000000001000001",
        "b8bd2155337b5943fe0377e0ef7f4c3fe16bfd8ad628465892e39d96c3e0ab58",
    ),
    (
        2,
        1,
        "00d6070000020000000c00ffffffffffffffffa10100000000000001000001",
        "24b2f50867ffc0f84e41b3fa4694910d5fc32d0caddb29567c74ecf44f753c6d",
    ),
    (
        2,
        2,
        "00d107000002000000010016000000ae000000a2010000de00000000010401",
        "84382088811bef103f5da1fbd40bdc1aaade0f50536e993d85c95c2b24e68ede",
    ),
    (
        2,
        4,
        "00d3070000020000000300ffffffffffffffffa10100000000000001000001",
        "bfb9bc35f3bd316b11563ee216f937a8fa86fa1798cd6839615ffb1f726a93d3",
    ),
    (
        2,
        5,
        "00d4070000020000000400ffffffffffffffffa10100000000000001000001",
        "10bfded57e642b88a27958d7e44fb131283d021d73aaf92761f9c64fbafea4f8",
    ),
    (
        2,
        6,
        "00d107000002000000010016000000ad000000a20100002c01000000010401",
        "fbf5c8443178cf3f23697f21b3ca8fa89178220b9e65f5d8f572dcb6656a3020",
    ),
    (
        2,
        7,
        "00d107000002000000010016000000b0000000a20100005502000000010401",
        "7472c4c64a228ad7105a41748ad19a14f0f0ebd3eafc31ca9f0c5815d88db504",
    ),
    (
        2,
        8,
        "00d107000002000000010016000000ac000000a20100009001000000010401",
        "4056cf959a2014e509fabcff9b66c2c329519b3b34f64c66d5b07470e68f9c3b",
    ),
    (
        2,
        9,
        "00d2070000020000000200ffffffffffffffffa10100000000000001000001",
        "59a882603b91746be7630220e7377f82628c2eb622b597dfac040e94bec4efd7",
    ),
    (
        3,
        1,
        "00d9070000030000001100ffffffffffffffffa10100000000000001000001",
        "b6320a93dddc9d14a72f25154424ce6b5ac6e66c346a64d31672995014e0e065",
    ),
    (
        3,
        2,
        "00d1070000030000000100b4000000ff000000a20100001b02000000010401",
        "55b5bb27f417f48ddfdfd5b7bbf2e34128f0670cd6c5d3f47bee92ad52f6c40b",
    ),
    (
        3,
        3,
        "00d2070000030000000200ffffffffffffffffa10100000000000001000001",
        "f3351cdc6b35f2cdc5e59890bfb24d67a5c6cf1a2f26a060df528ba2b3b6cd18",
    ),
    (
        3,
        4,
        "00d3070000030000000300ffffffffffffffffa10100000000000001000001",
        "046c8184c5b2fb722b7dca6a87a80272b52e668a8299110216ed829c2eba9be3",
    ),
    (
        3,
        5,
        "00d4070000030000000400ffffffffffffffffa10100000000000001000001",
        "ed5a11ae0a2a3dcbc5a456010879dd3c8d65b6549554cdbb7f8d0568319df182",
    ),
    (
        3,
        7,
        "00d1070000030000000100b4000000fc000000a20100004501000000010401",
        "75847d72c000082d7cdf02c4aae8732a364339a4547311a380eea072a532e292",
    ),
    (
        3,
        8,
        "00d7070000030000000d00ffffffffffffffffa10100000000000001000001",
        "2f8dceb36cb4fdf8b08042f6d1f12cd329eb7300ffb6d959e355c88d60f9f77e",
    ),
];

fn payload(hex: &str) -> [u8; 31] {
    assert_eq!(hex.len(), 62);
    let mut out = [0; 31];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).unwrap();
    }
    out
}

fn snapshot(actor_o: i16, order: GatherWorkOrder) -> GatherWorkSnapshot {
    GatherWorkSnapshot {
        revision: 0x4741_5448_4552,
        frame: 1,
        rng_state: 0x1234_5678,
        actor: GatherActorImage {
            who: order.target_who as u8,
            o: actor_o,
            uid: 77,
            group: 9,
            unit_masks: 0x7f00_0042,
            lead_animation: Some(GATHER_ANIMATION_19),
        },
        order,
        site: GatherSiteImage {
            who: order.target_who,
            o: order.target_o,
            uid: order.target_uid,
            resolved_build: true,
            valid_wall: true,
            active: true,
            property: order.build_type,
            build_masks: 0,
            recharging: 11,
        },
    }
}

#[test]
fn all_29_fresh_payloads_bind_exactly_and_only_camp_wait_ticks_plan() {
    assert_eq!(FRESH_GATHERS.len(), 29);
    let mut classes = [0usize; 2];
    let mut hashes = BTreeSet::new();
    for &(who, actor_o, hex, digest) in FRESH_GATHERS {
        assert_eq!(digest.len(), 64);
        assert!(hashes.insert(digest));
        let bytes = payload(hex);
        let order = GatherWorkOrder::from_retail_payload(0, bytes);
        assert_eq!(order.retail_payload_image(), bytes);
        match bind_fresh_gather_payload(who, order).unwrap() {
            FreshGatherPayloadClass::FarmActive => {
                classes[0] += 1;
                let before = snapshot(actor_o, order);
                assert_eq!(
                    plan_fresh_gather_tick(before),
                    Err(GatherWorkPlanError::Unowned(
                        UnownedGatherArm::FarmWorldAndAnimation
                    ))
                );
            }
            FreshGatherPayloadClass::CampActiveTimer => {
                classes[1] += 1;
                let before = snapshot(actor_o, order);
                let plan = plan_fresh_gather_tick(before).unwrap();
                assert_eq!(plan.after.order.wait, order.wait - 1);
                assert_eq!(plan.after.actor.group, -1);
                assert_eq!(plan.after.actor.unit_masks, 0x0700_0042);
                assert_eq!(plan.after.site.build_masks, GATHER_SITE_LATCH);
                assert_eq!(plan.after.site.recharging, 12);
                assert_eq!((plan.rng_draws, plan.external_effects), (0, 0));
                assert_eq!(plan.after.rng_state, before.rng_state);
            }
        }
    }
    assert_eq!(classes, [17, 12]);
    assert_eq!(hashes.len(), 29);
}

#[test]
fn malformed_and_unowned_edges_fail_before_any_commit() {
    let (_, actor_o, hex, _) = FRESH_GATHERS[0];
    let camp = GatherWorkOrder::from_retail_payload(0, payload(hex));
    let mutations = [
        (
            GatherWorkOrder {
                node_metric: 1,
                ..camp
            },
            FreshGatherBindingError::NonzeroNodeMetric(1),
        ),
        (
            GatherWorkOrder { flags: 1, ..camp },
            FreshGatherBindingError::NonzeroFlags(1),
        ),
        (
            GatherWorkOrder {
                target_who: 7,
                ..camp
            },
            FreshGatherBindingError::ForeignTargetOwner {
                actor: 0,
                target: 7,
            },
        ),
        (
            GatherWorkOrder {
                target_o: 1999,
                ..camp
            },
            FreshGatherBindingError::TargetOutsideBuildBand(1999),
        ),
        (
            GatherWorkOrder {
                been_there: 0,
                ..camp
            },
            FreshGatherBindingError::CampStateMismatch,
        ),
    ];
    for (order, expected) in mutations {
        assert_eq!(bind_fresh_gather_payload(0, order), Err(expected));
    }

    let mut capacity = snapshot(actor_o, camp);
    capacity.frame = -i32::from(actor_o).wrapping_mul(4);
    assert_eq!(
        plan_fresh_gather_tick(capacity),
        Err(GatherWorkPlanError::Unowned(
            UnownedGatherArm::CapacityAndGathererCount
        ))
    );
    let mut rng_edge = snapshot(actor_o, camp);
    rng_edge.order.wait = 1;
    assert_eq!(
        plan_fresh_gather_tick(rng_edge),
        Err(GatherWorkPlanError::Unowned(
            UnownedGatherArm::AllGatheringAndRng
        ))
    );
    let mut animation = snapshot(actor_o, camp);
    animation.actor.lead_animation = Some(0x1d);
    assert_eq!(
        plan_fresh_gather_tick(animation),
        Err(GatherWorkPlanError::Unowned(
            UnownedGatherArm::LeadAnimation(0x1d)
        ))
    );
    let mut missing_guy = snapshot(actor_o, camp);
    missing_guy.actor.lead_animation = None;
    assert_eq!(
        plan_fresh_gather_tick(missing_guy),
        Err(GatherWorkPlanError::Unowned(
            UnownedGatherArm::MissingLeadGuy
        ))
    );
}

#[derive(Clone)]
struct AtomicHost {
    state: GatherWorkSnapshot,
    commits: usize,
    stale: bool,
    bad_receipt: bool,
}

impl AtomicGatherWorkHost for AtomicHost {
    fn snapshot(
        &mut self,
        actor_who: u8,
        actor_o: i16,
    ) -> Result<GatherWorkSnapshot, GatherWorkHostError> {
        if (self.state.actor.who, self.state.actor.o) != (actor_who, actor_o) {
            return Err(GatherWorkHostError::Unavailable);
        }
        Ok(self.state)
    }

    fn compare_exchange(
        &mut self,
        plan: &GatherWorkPlan,
    ) -> Result<GatherWorkCommitReceipt, GatherWorkHostError> {
        self.commits += 1;
        if self.stale || self.state != plan.before {
            return Err(GatherWorkHostError::StaleSnapshot);
        }
        let mut receipt = GatherWorkCommitReceipt::applied(plan);
        if self.bad_receipt {
            receipt.changed_fields = receipt.changed_fields.wrapping_add(1);
            return Ok(receipt);
        }
        self.state = receipt.after;
        Ok(receipt)
    }
}

#[test]
fn host_compare_exchange_is_whole_image_and_rejections_are_zero_write() {
    let (_, actor_o, hex, _) = FRESH_GATHERS[0];
    let order = GatherWorkOrder::from_retail_payload(0, payload(hex));
    let before = snapshot(actor_o, order);
    let mut host = AtomicHost {
        state: before,
        commits: 0,
        stale: false,
        bad_receipt: false,
    };
    let receipt = resume_fresh_gather_tick(&mut host, 0, actor_o).unwrap();
    assert_eq!(host.state, receipt.after);
    assert_eq!(host.commits, 1);
    assert_eq!(receipt.after.revision, before.revision + 1);
    assert_eq!(receipt.changed_fields, 5);

    let mut stale = AtomicHost {
        state: before,
        commits: 0,
        stale: true,
        bad_receipt: false,
    };
    assert_eq!(
        resume_fresh_gather_tick(&mut stale, 0, actor_o),
        Err(GatherWorkTransactionError::Host(
            GatherWorkHostError::StaleSnapshot
        ))
    );
    assert_eq!(stale.state, before);

    let mut bad = AtomicHost {
        state: before,
        commits: 0,
        stale: false,
        bad_receipt: true,
    };
    assert_eq!(
        resume_fresh_gather_tick(&mut bad, 0, actor_o),
        Err(GatherWorkTransactionError::InvalidCommitReceipt)
    );
    assert_eq!(bad.state, before);
}

fn ordinary_build(uid: u16, object_o: i16) -> production::BuildData {
    let mut build = production::BuildData {
        flags: production::flag::VALID | production::flag::STARTED | production::flag::ACTIVE,
        myhits: 640,
        uid,
        constr_time: 20_000,
        construct_hits: 640,
        orig_type: CAMP_PROPERTY,
        ..Default::default()
    };
    build.gather_down = -1;
    build.city = -1;
    build.city_down = -1;
    build.wonder = -1;
    build.dock = -1;
    build.attack_ox = -1;
    build.attack_whom = -1;
    build.other[0x0a..0x0c].copy_from_slice(&object_o.to_le_bytes());
    build.other[0x28..0x2a].copy_from_slice(&(-1i16).to_le_bytes());
    build
}

fn exact_order(order: GatherWorkOrder) -> Order {
    Order::economy(EconomyOrderNode {
        metric: order.node_metric,
        header: EconomyOrderHeader {
            kind: OrderIndex::Gather,
            flags: order.flags,
            x: 0,
            y: 0,
            primary: StableTargetIdentity::banded(
                order.target_o,
                order.target_who,
                order.target_uid,
            ),
        },
        payload: EconomyOrderPayload::Gather(GatherOrderPayload {
            tx: order.tx,
            ty: order.ty,
            build_type: order.build_type,
            wait: order.wait,
            goto_build: order.goto_build,
            non_flat_gather: order.non_flat_gather,
            dist_mod: order.dist_mod,
            been_there: order.been_there,
        }),
    })
    .unwrap()
}

fn work_order(order: &Order) -> GatherWorkOrder {
    let EconomyOrderPayload::Gather(payload) = order.economy.unwrap() else {
        panic!("expected Gather payload")
    };
    GatherWorkOrder {
        node_metric: order.node_metric,
        flags: order.flags,
        target_o: i32::from(order.target_o),
        target_who: i32::from(order.target_who),
        target_uid: order.target_uid,
        tx: payload.tx,
        ty: payload.ty,
        build_type: payload.build_type,
        wait: payload.wait,
        goto_build: payload.goto_build,
        non_flat_gather: payload.non_flat_gather,
        dist_mod: payload.dist_mod,
        been_there: payload.been_there,
    }
}

fn sim_fixture() -> (Sim, usize, usize, GatherWorkOrder) {
    let mut sim = Sim::new(0x1234_5678, 4);
    let mut actor_row = 0;
    for o in 0..=3 {
        let handle = sim.spawn_unit(0, 0x32, 1000 + o * 10, 1000, 4).unwrap();
        actor_row = sim.world.row_of(handle).unwrap();
    }
    assert_eq!(sim.world.units.o()[actor_row], 3);
    sim.world.units.group_mut()[actor_row] = 9;
    sim.world.units.set_unit_masks(actor_row, 0x7f00_0042);
    sim.world.frame = 1;
    sim.vic_match.frame = 1;

    let dummy = sim.spawn_build(0, ordinary_build(99, 2000));
    let site_row = sim.spawn_build(0, ordinary_build(1, 2001));
    assert_eq!((dummy, site_row), (0, 1));

    let order = GatherWorkOrder::from_retail_payload(0, payload(FRESH_GATHERS[0].2));
    sim.world.orders_mut(actor_row).replace(exact_order(order));
    (sim, actor_row, site_row, order)
}

fn apply_resumed_tick(sim: &mut Sim, actor_row: usize, site_row: usize) -> GatherWorkCommitReceipt {
    let actor_o = sim.world.units.o()[actor_row];
    let order = work_order(sim.world.orders(actor_row).current().unwrap());
    let site = &sim.builds[site_row];
    let before = GatherWorkSnapshot {
        revision: 17,
        frame: sim.world.frame,
        rng_state: sim.world.random.state(),
        actor: GatherActorImage {
            who: sim.world.units.get_who(actor_row),
            o: actor_o,
            uid: sim.world.units.get_uid(actor_row),
            group: sim.world.units.group()[actor_row],
            unit_masks: sim.world.units.get_unit_masks(actor_row),
            lead_animation: Some(GATHER_ANIMATION_19),
        },
        order,
        site: GatherSiteImage {
            who: i32::from(site.who),
            o: i32::from(site.object_id()),
            uid: site.uid,
            resolved_build: true,
            valid_wall: site.is_valid(),
            active: site.is_active(),
            property: site.orig_type,
            build_masks: site.build_masks,
            recharging: site.recharging,
        },
    };
    let mut host = AtomicHost {
        state: before,
        commits: 0,
        stale: false,
        bad_receipt: false,
    };
    let receipt = resume_fresh_gather_tick(&mut host, before.actor.who, actor_o).unwrap();

    // All comparisons above are complete; these assignments cannot fail halfway.
    sim.world.units.group_mut()[actor_row] = receipt.after.actor.group;
    sim.world
        .units
        .set_unit_masks(actor_row, receipt.after.actor.unit_masks);
    sim.builds[site_row].build_masks = receipt.after.site.build_masks;
    sim.builds[site_row].recharging = receipt.after.site.recharging;
    let current = sim.world.orders_mut(actor_row).current_mut().unwrap();
    let EconomyOrderPayload::Gather(payload) = current.economy.as_mut().unwrap() else {
        unreachable!()
    };
    payload.wait = receipt.after.order.wait;
    receipt
}

#[test]
fn v13_save_reload_resumes_and_resaves_the_exact_camp_wait_transaction() {
    let (mut direct, actor_row, site_row, witness) = sim_fixture();
    let saved = save_load::save_sim(&direct).unwrap();
    let mut resumed = save_load::load_sim(&saved).unwrap();
    assert_eq!(
        work_order(resumed.world.orders(actor_row).current().unwrap()),
        witness
    );

    let rng_before = resumed.world.random.state();
    let direct_receipt = apply_resumed_tick(&mut direct, actor_row, site_row);
    let resumed_receipt = apply_resumed_tick(&mut resumed, actor_row, site_row);
    assert_eq!(direct_receipt, resumed_receipt);
    assert_eq!(resumed.world.random.state(), rng_before);
    assert_eq!(resumed.world.units.group()[actor_row], -1);
    assert_eq!(resumed.world.units.get_unit_masks(actor_row), 0x0700_0042);
    assert_eq!(resumed.builds[site_row].build_masks, GATHER_SITE_LATCH);
    assert_eq!(resumed.builds[site_row].recharging, 1);
    assert_eq!(
        work_order(resumed.world.orders(actor_row).current().unwrap()).wait,
        witness.wait - 1
    );
    assert_eq!(
        save_load::save_sim(&resumed).unwrap(),
        save_load::save_sim(&direct).unwrap()
    );

    let saved_after = save_load::save_sim(&resumed).unwrap();
    let loaded_after = save_load::load_sim(&saved_after).unwrap();
    assert_eq!(loaded_after.world.units.group()[actor_row], -1);
    assert_eq!(
        loaded_after.world.units.get_unit_masks(actor_row),
        0x0700_0042
    );
    assert_eq!(loaded_after.builds[site_row].build_masks, GATHER_SITE_LATCH);
    assert_eq!(loaded_after.builds[site_row].recharging, 1);
    assert_eq!(
        work_order(loaded_after.world.orders(actor_row).current().unwrap()).wait,
        witness.wait - 1
    );
}

fn pe_offset(image: &[u8], va: u32) -> usize {
    let u16_at = |at| u16::from_le_bytes(image[at..at + 2].try_into().unwrap());
    let u32_at = |at| u32::from_le_bytes(image[at..at + 4].try_into().unwrap());
    let pe = u32_at(0x3c) as usize;
    let sections = u16_at(pe + 6) as usize;
    let optional_size = u16_at(pe + 20) as usize;
    let optional = pe + 24;
    let rva = va - u32_at(optional + 28);
    let table = optional + optional_size;
    for index in 0..sections {
        let section = table + index * 40;
        let virtual_size = u32_at(section + 8);
        let virtual_address = u32_at(section + 12);
        let raw_size = u32_at(section + 16);
        let raw = u32_at(section + 20);
        if (virtual_address..virtual_address + virtual_size.max(raw_size)).contains(&rva) {
            return (raw + rva - virtual_address) as usize;
        }
    }
    panic!("VA {va:#x} is outside the image")
}

#[test]
fn matched_pe_and_pdb_pin_the_exact_call_latch_and_timer_sites() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let exe_path = root.join("ron-bin/riseofnations.exe");
    if !exe_path.exists() {
        return;
    }
    let image = fs::read(exe_path).unwrap();
    let exact = [
        (0x005f_0136, "ff75ec8bcbe830000000"),
        (0x005f_01e5, "b900080000668548607541"),
        (0x005f_020a, "8b4ddc66ff407a"),
        (0x005f_0228, "b90008000066094860"),
        (
            0x005f_0db4,
            "8b83f40000008b000fbe809c00000083f81d0f842f06000083f8190f8582000000836f2001",
        ),
    ];
    for (va, expected) in exact {
        let expected = expected
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        let offset = pe_offset(&image, va);
        assert_eq!(&image[offset..offset + expected.len()], expected);
    }

    let symbols = fs::read_to_string(root.join("re/symtab.json")).unwrap();
    assert!(symbols.contains(r#"{"va": 6222496, "size": 3780, "name": "Unit::do_gather""#));
    assert!(symbols.contains(r#"{"va": 6226288, "size": 4766, "name": "Unit::do_non_flat_gather""#));
}
