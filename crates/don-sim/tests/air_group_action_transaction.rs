// SPDX-License-Identifier: GPL-3.0-or-later

// Keep the source-inclusion contract gate independent of the registered crate module. These
// aliases give its crate-relative imports the same shape in this integration test.
mod command {
    pub mod air_launch_receivers {
        pub use don_sim::command::air_launch_receivers::*;
    }
}
mod order {
    pub use don_sim::order::*;
}
mod systems {
    pub mod groups_guys {
        pub use don_sim::systems::groups_guys::*;
    }
}

#[allow(dead_code)]
#[path = "../src/systems/air_group_action_transaction.rs"]
mod air_group_action_transaction;

use air_group_action_transaction::*;
use don_sim::command::air_launch_receivers::{
    AirLaunchBoundary, AirLaunchFacts, ContainedFacts, LaunchPatrolRequest, MemberFacts,
};
use don_sim::order::OrderIndex;

const OWNER: u8 = 2;
const GROUP_SLOT: u8 = 7;
const GROUP_ID: i32 = 91;
const BASE_O: i32 = 2_007;
const PLANE_O: i32 = 40;

fn build_identity(o: i32, row: u32) -> CanonicalObjectIdentity {
    CanonicalObjectIdentity {
        owner: OWNER,
        band: CanonicalObjectBand::Build,
        o,
        generation: CanonicalObjectGeneration::BuildRow(row),
    }
}

fn unit_identity(o: i32, id: u32, generation: u32) -> CanonicalObjectIdentity {
    CanonicalObjectIdentity {
        owner: OWNER,
        band: CanonicalObjectBand::Unit,
        o,
        generation: CanonicalObjectGeneration::Unit { id, generation },
    }
}

fn group_walk(id: i32, who: u8, members: &[i16]) -> Vec<u8> {
    let mut walk = vec![0; 72];
    walk[0..4].copy_from_slice(&id.to_le_bytes());
    walk[8..12].copy_from_slice(&(members.len() as i32).to_le_bytes());
    walk[70] = who;
    for member in members {
        walk.extend_from_slice(&member.to_le_bytes());
    }
    // off_x, off_y, curr_x, curr_y, then angles.
    walk.resize(walk.len() + members.len() * 4 * 4 + members.len(), 0);
    walk
}

fn group_before() -> GroupBeforeImage {
    let base = build_identity(BASE_O, 13);
    GroupBeforeImage {
        key: CanonicalGroupKey {
            owner: OWNER,
            slot: GROUP_SLOT,
            group_id: GROUP_ID,
        },
        revision: 4,
        walk_image: group_walk(GROUP_ID, OWNER, &[BASE_O as i16]),
        members: vec![base],
    }
}

fn authority() -> AuthorityRevisions {
    AuthorityRevisions {
        scenario: 10,
        types: 20,
        world_orders: 30,
    }
}

fn package_position() -> CommandPackagePosition {
    CommandPackagePosition {
        game_frame: 1_000,
        package_serial: 701,
        play: 0,
        group_command_index: 0,
        action_command_index: 1,
    }
}

fn group_packet() -> Vec<u8> {
    let mut packet = vec![GROUP_OPCODE, 1, OWNER];
    packet.extend_from_slice(&(BASE_O as i16).to_le_bytes());
    packet
}

fn launch_packet(queue: i32, force_all: i32, bombers: i32, fighters: i32) -> Vec<u8> {
    let mut packet = vec![LAUNCH_PATROL_OPCODE];
    for field in [4_800, 9_600, queue, force_all, bombers, fighters] {
        packet.extend_from_slice(&field.to_le_bytes());
    }
    packet
}

fn request(packet: Vec<u8>) -> AirGroupActionRequest {
    AirGroupActionRequest::from_paired_packets(
        package_position(),
        group_packet(),
        packet,
        group_before(),
        authority(),
    )
    .unwrap()
}

fn eligible_plane() -> ContainedFacts {
    ContainedFacts {
        inside: Some(Some((BASE_O as i16, OWNER))),
        ..ContainedFacts::eligible((OWNER, PLANE_O as i16), (2_400, 3_600))
    }
}

fn complete_integration() -> AirIntegrationCapabilities {
    AirIntegrationCapabilities {
        packet_to_sim_route: true,
        move_order_tag_v1: true,
        air_patrol_order_tag_v1: true,
        dynamic_patrol_arrays: true,
        move_unit_work: true,
        air_patrol_unit_work: true,
        move_save_reload_resume: true,
        air_patrol_save_reload_resume: true,
    }
}

fn snapshot(plane: ContainedFacts) -> AirGroupActionSnapshot {
    let plane_identity = unit_identity(PLANE_O, 144, 3);
    let binding_request = request(vec![SCRAMBLE_OPCODE]);
    AirGroupActionSnapshot {
        group_before: group_before(),
        authority: authority(),
        canonical_group_packet: Some(CanonicalGroupPacketReceipt::for_request(
            &binding_request,
            Some(5),
        )),
        ignore_orders: IgnoreOrdersSnapshot::Clear {
            revision: authority().scenario,
        },
        type_authority: AirTypeAuthoritySnapshot {
            revision: authority().types,
            domain_column: true,
            object_masks_column: true,
            unit_flags_column: true,
            nonstrict_type_relations: true,
        },
        integration: complete_integration(),
        facts: AirLaunchFacts {
            who: OWNER,
            members: vec![MemberFacts {
                object: (OWNER, BASE_O as i16),
                pos: (4_800, 9_600),
                contained: vec![plane],
                containment_answered: true,
            }],
        },
        contained_identities: vec![vec![plane_identity]],
        target_orders: vec![AirOrderTargetBefore {
            identity: plane_identity,
            order_revision: 8,
            order_digest: 0x1111,
            unit_mask: 0xffff_ffff,
            action_revision: 6,
            path_revision: 7,
        }],
    }
}

#[test]
fn exact_wire_decoders_reject_truncation_and_trailing_bytes() {
    assert_eq!(
        decode_group_selection_packet(&[GROUP_OPCODE, 0, OWNER]),
        Ok(GroupSelectionCommand {
            owner: OWNER,
            requested: Vec::new(),
        })
    );
    assert_eq!(
        decode_group_selection_packet(&group_packet()),
        Ok(GroupSelectionCommand {
            owner: OWNER,
            requested: vec![BASE_O as i16],
        })
    );
    assert_eq!(
        decode_group_selection_packet(&[GROUP_OPCODE, 1, OWNER]),
        Err(GroupSelectionWireError::WrongSize {
            expected: 5,
            actual: 3,
        })
    );
    assert_eq!(
        decode_air_group_packet(&[SCRAMBLE_OPCODE]),
        Ok(AirGroupCommand::Scramble)
    );
    let packet = launch_packet(1, 2, 3, 4);
    assert_eq!(
        decode_air_group_packet(&packet),
        Ok(AirGroupCommand::LaunchPatrol(LaunchPatrolRequest {
            to_x: 4_800,
            to_y: 9_600,
            queue: 1,
            force_all: 2,
            bombers_only: 3,
            fighters_only: 4,
        }))
    );
    assert!(matches!(
        decode_air_group_packet(&packet[..24]),
        Err(AirGroupWireError::WrongSize {
            opcode: LAUNCH_PATROL_OPCODE,
            expected: 25,
            actual: 24,
        })
    ));
    assert!(matches!(
        decode_air_group_packet(&[SCRAMBLE_OPCODE, 0]),
        Err(AirGroupWireError::WrongSize {
            opcode: SCRAMBLE_OPCODE,
            expected: 1,
            actual: 2,
        })
    ));
}

/// Compact command-interface facts from retail replay
/// SHA-256 `558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`.
/// The ignored/copyrighted replay itself is not a fixture.
#[test]
fn finished_retail_replay_scramble_pairs_pin_empty_and_explicit_group_shapes() {
    let cases: &[(CommandPackagePosition, &[u8], &[i16])] = &[
        (
            CommandPackagePosition {
                game_frame: 43_722,
                package_serial: 44_041,
                play: 0,
                group_command_index: 0,
                action_command_index: 1,
            },
            &[0x00, 0x00, 0x00],
            &[],
        ),
        (
            CommandPackagePosition {
                game_frame: 47_706,
                package_serial: 48_065,
                play: 0,
                group_command_index: 1,
                action_command_index: 2,
            },
            &[0x00, 0x01, 0x00, 0xdf, 0x07],
            &[2_015],
        ),
        (
            CommandPackagePosition {
                game_frame: 51_868,
                package_serial: 52_254,
                play: 0,
                group_command_index: 1,
                action_command_index: 2,
            },
            &[0x00, 0x02, 0x00, 0x2a, 0x08, 0x2b, 0x08],
            &[2_090, 2_091],
        ),
        (
            CommandPackagePosition {
                game_frame: 57_968,
                package_serial: 58_402,
                play: 0,
                group_command_index: 0,
                action_command_index: 1,
            },
            &[0x00, 0x03, 0x00, 0x2a, 0x08, 0x2b, 0x08, 0x63, 0x08],
            &[2_090, 2_091, 2_147],
        ),
    ];
    for (position, group, expected) in cases {
        let pair = decode_air_group_packet_pair(*position, group, &[SCRAMBLE_OPCODE]).unwrap();
        assert_eq!(pair.action, AirGroupCommand::Scramble);
        assert_eq!(pair.group.owner, 0);
        assert_eq!(pair.group.requested, *expected);
        assert!(pair
            .group
            .requested
            .iter()
            .all(|o| (2_000..3_000).contains(o)));
    }
}

#[test]
fn group_before_image_binds_walked_order_and_stable_member_identity() {
    let image = group_before();
    assert_eq!(image.validate(), Ok(()));
    assert_ne!(image.diagnostic_digest(), 0);

    let mut wrong_order = image.clone();
    wrong_order.walk_image[72..74].copy_from_slice(&41_i16.to_le_bytes());
    assert!(matches!(
        wrong_order.validate(),
        Err(GroupImageError::MemberAddressMismatch { index: 0, .. })
    ));

    let mut stale_band = image;
    stale_band.members[0].generation = CanonicalObjectGeneration::WallRow(13);
    assert_eq!(
        stale_band.validate(),
        Err(GroupImageError::InvalidIdentity { index: 0 })
    );
}

#[test]
fn both_rows_recompute_through_one_preflight_contract() {
    let scramble = request(vec![SCRAMBLE_OPCODE]);
    let scramble_snapshot = snapshot(eligible_plane());
    let prepared = prepare_air_group_action(&scramble, &scramble_snapshot).unwrap();
    assert!(matches!(prepared.plan, AirGroupActionPlan::Scramble(_)));
    assert_eq!(prepared.plan.installs().len(), 1);
    assert_eq!(prepared.plan.installs()[0].kind(), OrderIndex::AirPatrol);

    let launch = request(launch_packet(1, 0, 0, 0));
    let launch_snapshot = snapshot(eligible_plane());
    let prepared = prepare_air_group_action(&launch, &launch_snapshot).unwrap();
    assert!(matches!(prepared.plan, AirGroupActionPlan::LaunchPatrol(_)));
    assert_eq!(prepared.plan.installs().len(), 1);
    assert_eq!(prepared.plan.installs()[0].kind(), OrderIndex::AirPatrol);
}

#[test]
fn helicopter_branch_has_its_own_move_payload_requirements() {
    let request = request(vec![SCRAMBLE_OPCODE]);
    let mut plane = eligible_plane();
    plane.unit_flags = don_sim::command::air_launch_receivers::HELICOPTER_TYPE_FLAG;
    let mut image = snapshot(plane);
    let prepared = prepare_air_group_action(&request, &image).unwrap();
    assert_eq!(prepared.plan.installs()[0].kind(), OrderIndex::MoveTo);

    image.integration.move_order_tag_v1 = false;
    assert_eq!(
        prepare_air_group_action(&request, &image).unwrap_err(),
        AirTransactionBlocker::PayloadCapabilityUnavailable(AirPayloadCapability::MoveOrderTagV1)
    );
}

#[test]
fn every_missing_production_authority_is_a_typed_blocker() {
    let scramble = request(vec![SCRAMBLE_OPCODE]);
    let full = snapshot(eligible_plane());

    let mut image = full.clone();
    image.canonical_group_packet = None;
    assert_eq!(
        prepare_air_group_action(&scramble, &image).unwrap_err(),
        AirTransactionBlocker::CanonicalGroupPackageHostUnavailable
    );

    let mut image = full.clone();
    image
        .canonical_group_packet
        .as_mut()
        .unwrap()
        .group_packet_digest ^= 1;
    assert_eq!(
        prepare_air_group_action(&scramble, &image).unwrap_err(),
        AirTransactionBlocker::CanonicalGroupPackageReceiptMismatch
    );

    let mut image = full.clone();
    image.ignore_orders = IgnoreOrdersSnapshot::Unavailable;
    assert_eq!(
        prepare_air_group_action(&scramble, &image).unwrap_err(),
        AirTransactionBlocker::IgnoreOrdersUnavailable
    );

    let mut image = full.clone();
    image.ignore_orders = IgnoreOrdersSnapshot::Armed {
        revision: authority().scenario,
    };
    assert_eq!(
        prepare_air_group_action(&scramble, &image).unwrap_err(),
        AirTransactionBlocker::IgnoreOrdersPruneUnavailable
    );

    let mut image = full.clone();
    image.integration.packet_to_sim_route = false;
    assert_eq!(
        prepare_air_group_action(&scramble, &image).unwrap_err(),
        AirTransactionBlocker::PacketToSimRouteUnavailable
    );

    let mut image = full.clone();
    image.type_authority.domain_column = false;
    assert_eq!(
        prepare_air_group_action(&scramble, &image).unwrap_err(),
        AirTransactionBlocker::TypeAuthorityUnavailable(AirTypeAuthorityColumn::Domain)
    );

    for (mut image, capability) in [
        {
            let mut image = full.clone();
            image.integration.air_patrol_order_tag_v1 = false;
            (image, AirPayloadCapability::AirPatrolOrderTagV1)
        },
        {
            let mut image = full.clone();
            image.integration.dynamic_patrol_arrays = false;
            (image, AirPayloadCapability::DynamicPatrolArrays)
        },
        {
            let mut image = full.clone();
            image.integration.air_patrol_unit_work = false;
            (image, AirPayloadCapability::AirPatrolUnitWork)
        },
        {
            let mut image = full.clone();
            image.integration.air_patrol_save_reload_resume = false;
            (image, AirPayloadCapability::AirPatrolSaveReloadResume)
        },
    ] {
        assert_eq!(
            prepare_air_group_action(&scramble, &image).unwrap_err(),
            AirTransactionBlocker::PayloadCapabilityUnavailable(capability)
        );
        // Make it evident the loop owns each snapshot and cannot mutate `full`.
        image.integration = complete_integration();
    }

    let launch = request(launch_packet(1, 0, 0, 0));
    let mut image = full;
    image.type_authority.nonstrict_type_relations = false;
    assert_eq!(
        prepare_air_group_action(&launch, &image).unwrap_err(),
        AirTransactionBlocker::TypeAuthorityUnavailable(
            AirTypeAuthorityColumn::NonstrictTypeRelations
        )
    );
}

#[test]
fn retail_empty_group_pair_requires_the_canonical_reselection_cache_receipt() {
    let request = AirGroupActionRequest::from_paired_packets(
        CommandPackagePosition {
            game_frame: 43_722,
            package_serial: 44_041,
            play: 0,
            group_command_index: 0,
            action_command_index: 1,
        },
        vec![GROUP_OPCODE, 0, OWNER],
        vec![SCRAMBLE_OPCODE],
        group_before(),
        authority(),
    )
    .unwrap();
    let mut image = snapshot(eligible_plane());
    image.canonical_group_packet = Some(CanonicalGroupPacketReceipt::for_request(&request, None));
    assert_eq!(
        prepare_air_group_action(&request, &image).unwrap_err(),
        AirTransactionBlocker::GroupReselectionCacheUnavailable
    );

    image.canonical_group_packet =
        Some(CanonicalGroupPacketReceipt::for_request(&request, Some(19)));
    assert!(prepare_air_group_action(&request, &image).is_ok());
}

#[test]
fn planner_unknowns_and_identity_mutations_never_reach_commit() {
    let request = request(vec![SCRAMBLE_OPCODE]);
    let mut unanswered = eligible_plane();
    unanswered.busy = None;
    let image = snapshot(unanswered);
    assert_eq!(
        prepare_air_group_action(&request, &image).unwrap_err(),
        AirTransactionBlocker::Planner(AirLaunchBoundary::BusyUnanswered {
            object: (OWNER, PLANE_O as i16)
        })
    );

    let mut image = snapshot(eligible_plane());
    image.contained_identities[0][0].generation = CanonicalObjectGeneration::Unit {
        id: 144,
        generation: 4,
    };
    assert_eq!(
        prepare_air_group_action(&request, &image).unwrap_err(),
        AirTransactionBlocker::TargetBeforeMismatch { install: 0 }
    );

    let mut image = snapshot(eligible_plane());
    image.group_before.revision += 1;
    assert_eq!(
        prepare_air_group_action(&request, &image).unwrap_err(),
        AirTransactionBlocker::GroupBeforeImageChanged
    );
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct FakeState {
    installs: Vec<(CanonicalObjectIdentity, OrderIndex)>,
}

#[derive(Debug, Default)]
struct FakeHost {
    state: FakeState,
    stale: Option<AirCommitFailure>,
    fail_after_first_write: bool,
    forge_evidence: bool,
}

impl FakeHost {
    fn digest(&self) -> u64 {
        self.state
            .installs
            .iter()
            .fold(0xcbf2_9ce4_8422_2325, |h, (identity, kind)| {
                let word = (identity.o as u64) ^ ((*kind as u64) << 40);
                (h ^ word).wrapping_mul(0x0000_0100_0000_01b3)
            })
    }
}

impl AtomicAirGroupActionHost for FakeHost {
    type Checkpoint = FakeState;

    fn checkpoint(&self) -> Self::Checkpoint {
        self.state.clone()
    }

    fn state_digest(&self) -> u64 {
        self.digest()
    }

    fn revalidate(&self, _prepared: &PreparedAirGroupAction) -> Result<(), AirCommitFailure> {
        self.stale.map_or(Ok(()), Err)
    }

    fn commit(
        &mut self,
        prepared: &PreparedAirGroupAction,
    ) -> Result<Vec<CommittedAirInstall>, AirCommitFailure> {
        let mut committed = Vec::new();
        for (install, before) in prepared
            .plan
            .installs()
            .iter()
            .zip(&prepared.snapshot.target_orders)
        {
            self.state.installs.push((before.identity, install.kind()));
            if self.fail_after_first_write {
                return Err(AirCommitFailure::HostRejected("injected partial write"));
            }
            committed.push(CommittedAirInstall {
                identity: before.identity,
                kind: install.kind(),
                order_revision_after: before.order_revision + 1,
                order_digest_after: before.order_digest.rotate_left(7) ^ 0xa17,
                unit_mask_after: match install {
                    don_sim::command::air_launch_receivers::AirLaunchInstall::MoveFacing {
                        clear_unit_mask,
                        ..
                    } => before.unit_mask & !clear_unit_mask,
                    don_sim::command::air_launch_receivers::AirLaunchInstall::AirPatrol {
                        ..
                    } => before.unit_mask,
                },
                action_revision_after: before.action_revision + 1,
                path_revision_after: before.path_revision + 1,
            });
        }
        if self.forge_evidence {
            committed.clear();
        }
        Ok(committed)
    }

    fn restore(&mut self, checkpoint: Self::Checkpoint) {
        self.state = checkpoint;
    }
}

#[test]
fn applied_receipt_recomputes_and_binds_every_install_identity() {
    let request = request(vec![SCRAMBLE_OPCODE]);
    let snapshot = snapshot(eligible_plane());
    let mut host = FakeHost::default();
    let receipt = execute_air_group_action(&mut host, &request, &snapshot);
    assert!(matches!(receipt.status, AirTransactionStatus::Applied(_)));
    assert_eq!(host.state.installs.len(), 1);
    assert!(receipt.validates_for(&request, &snapshot));

    let mut forged = receipt.clone();
    let Some(AirGroupActionPlan::Scramble(plan)) = forged.plan.as_mut() else {
        panic!("scramble receipt carries its plan");
    };
    plan.installs.clear();
    assert!(!forged.validates());

    let mut changed_packet = receipt;
    changed_packet.request.packet[0] = LAUNCH_PATROL_OPCODE;
    assert!(!changed_packet.validates());

    let mut changed_group = execute_air_group_action(&mut host, &request, &snapshot);
    changed_group.request.group_packet[4] ^= 1;
    assert!(!changed_group.validates());

    let mut changed_position = execute_air_group_action(&mut host, &request, &snapshot);
    changed_position.request.position.group_command_index =
        changed_position.request.position.action_command_index;
    assert!(!changed_position.validates());
}

#[test]
fn stale_and_partial_commits_restore_the_exact_checkpoint() {
    let request = request(vec![SCRAMBLE_OPCODE]);
    let snapshot = snapshot(eligible_plane());

    let mut stale = FakeHost {
        stale: Some(AirCommitFailure::StaleWorldOrders),
        ..FakeHost::default()
    };
    let before = stale.state.clone();
    let receipt = execute_air_group_action(&mut stale, &request, &snapshot);
    assert_eq!(stale.state, before);
    assert!(matches!(
        receipt.status,
        AirTransactionStatus::RolledBack {
            failure: AirCommitFailure::StaleWorldOrders,
            restored: true,
            ..
        }
    ));
    assert!(receipt.validates());

    let mut partial = FakeHost {
        fail_after_first_write: true,
        ..FakeHost::default()
    };
    let before = partial.state.clone();
    let receipt = execute_air_group_action(&mut partial, &request, &snapshot);
    assert_eq!(partial.state, before);
    assert!(matches!(
        receipt.status,
        AirTransactionStatus::RolledBack {
            failure: AirCommitFailure::HostRejected("injected partial write"),
            restored: true,
            ..
        }
    ));
    assert!(receipt.validates());
}

#[test]
fn malformed_commit_evidence_is_rolled_back_instead_of_published() {
    let request = request(vec![SCRAMBLE_OPCODE]);
    let snapshot = snapshot(eligible_plane());
    let mut host = FakeHost {
        forge_evidence: true,
        ..FakeHost::default()
    };
    let before = host.state.clone();
    let receipt = execute_air_group_action(&mut host, &request, &snapshot);
    assert_eq!(host.state, before);
    assert!(matches!(
        receipt.status,
        AirTransactionStatus::RolledBack {
            failure: AirCommitFailure::CommitEvidenceMismatch,
            restored: true,
            ..
        }
    ));
}

#[test]
fn empty_containment_is_a_real_no_effect_commit_not_a_guessed_aircraft() {
    let request = request(vec![SCRAMBLE_OPCODE]);
    let mut snapshot = snapshot(eligible_plane());
    snapshot.facts.members[0].contained.clear();
    snapshot.contained_identities[0].clear();
    snapshot.target_orders.clear();
    let mut host = FakeHost::default();
    let receipt = execute_air_group_action(&mut host, &request, &snapshot);
    let AirTransactionStatus::Applied(evidence) = &receipt.status else {
        panic!("answered empty chain is an exact no-effect apply");
    };
    assert_eq!(evidence.state_digest_before, evidence.state_digest_after);
    assert!(evidence.installs.is_empty());
    assert!(receipt.validates());
}
