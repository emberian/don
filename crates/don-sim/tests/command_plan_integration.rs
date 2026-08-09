// SPDX-License-Identifier: GPL-3.0-or-later

use don_sim::command::diplomacy_command_plans::{
    plan_diplomacy_command, DiplomacyCommandPlan, DiplomacyCommandReceipt, DiplomacyCommandRequest,
    DiplomacyCommandState, DiplomacyPlanDecision, DiplomacyTransactionStatus, ACCEPT_OPCODE,
    DECLARE_OPCODE, REJECT_OPCODE, TREATY_OPCODE,
};
use don_sim::command::late_command_plans::{
    plan_cannon_time, CannonTimeFacts, CannonTimePlan, CannonTimeReceipt, CannonTimeRequest,
    CannonTimeState, PlanStatus,
};
use don_sim::command::object_command_plans::{
    plan_rename_city, NormalizeSelectionReceipt, NormalizeSelectionRequest,
    NormalizedSelectionView, RenameCityCommand, RenameCityHostFacts, RenameCityObjectLookupReceipt,
    RenameCityObjectLookupRequest, RenameCityPlan, RenameCityTransactionReceipt,
    RenameCityTransactionStatus,
};
use don_sim::command::setup_diplomacy::{LeaderTeamState, PlayerSetup, DIPLO_ALLY};
use don_sim::command::{Bridge, Fleet, InlineDef, InlinePort, ObjectTable, Package};
use don_sim::systems::order_dispatch::OrderQueue;

struct TransactionHost {
    objects: ObjectTable,
    rename_commits: Vec<RenameCityPlan>,
    cannon_facts: Option<CannonTimeFacts>,
    cannon_commits: Vec<CannonTimePlan>,
    diplomacy: Option<DiplomacyCommandState>,
    diplomacy_commits: Vec<DiplomacyCommandPlan>,
    diplomacy_attempts: usize,
}

impl TransactionHost {
    fn new() -> Self {
        Self {
            objects: ObjectTable::new(1),
            rename_commits: Vec::new(),
            cannon_facts: None,
            cannon_commits: Vec::new(),
            diplomacy: None,
            diplomacy_commits: Vec::new(),
            diplomacy_attempts: 0,
        }
    }
}

impl Fleet for TransactionHost {
    fn alive(&self, who: u8, o: i16) -> bool {
        self.objects.alive(who, o)
    }

    fn is_unit(&self, who: u8, o: i16) -> bool {
        self.objects.is_unit(who, o)
    }

    fn is_building(&self, who: u8, o: i16) -> bool {
        self.objects.is_building(who, o)
    }

    fn group_of(&self, who: u8, o: i16) -> i16 {
        self.objects.group_of(who, o)
    }

    fn set_group_of(&mut self, who: u8, o: i16, slot: i16) {
        self.objects.set_group_of(who, o, slot);
    }

    fn uid(&self, who: u8, o: i16) -> u16 {
        self.objects.uid(who, o)
    }

    fn pos(&self, who: u8, o: i16) -> (i32, i32) {
        self.objects.pos(who, o)
    }

    fn orders(&self, who: u8, o: i16) -> Option<&OrderQueue> {
        self.objects.orders(who, o)
    }

    fn orders_mut(&mut self, who: u8, o: i16) -> Option<&mut OrderQueue> {
        self.objects.orders_mut(who, o)
    }

    fn set_stance(&mut self, who: u8, o: i16, stance: i8) {
        self.objects.set_stance(who, o, stance);
    }

    fn disband(&mut self, who: u8, o: i16) {
        self.objects.disband(who, o);
    }

    fn apply_rename_city_transaction(
        &mut self,
        request: RenameCityCommand,
        frame: i32,
    ) -> RenameCityTransactionReceipt {
        let facts = RenameCityHostFacts {
            object: RenameCityObjectLookupReceipt {
                request: RenameCityObjectLookupRequest {
                    who: request.who,
                    object: request.object,
                },
                object_flags: 0x21,
                city_type: Some(7),
            },
            normalized_groups: vec![NormalizeSelectionReceipt {
                request: NormalizeSelectionRequest {
                    slot: request.who * 18,
                    normalize_flag: 1,
                },
                post: NormalizedSelectionView {
                    who: request.who as u8,
                    members: vec![request.object as i16],
                },
            }],
            frame,
        };
        let plan = plan_rename_city(&request, &facts).unwrap();
        self.rename_commits.push(plan.clone());
        RenameCityTransactionReceipt {
            request,
            status: RenameCityTransactionStatus::Applied,
            facts: Some(facts),
            plan: Some(plan),
        }
    }

    fn cannon_time_facts(&self, _request: CannonTimeRequest) -> Option<CannonTimeFacts> {
        self.cannon_facts
    }

    fn apply_cannon_time_transaction(
        &mut self,
        request: CannonTimeRequest,
        facts: CannonTimeFacts,
    ) -> CannonTimeReceipt {
        let plan = plan_cannon_time(&request, &facts).unwrap();
        self.cannon_commits.push(plan.clone());
        CannonTimeReceipt {
            request,
            facts: Some(facts),
            status: PlanStatus::Planned,
            plan: Some(plan),
        }
    }

    fn diplomacy_command_state(&self) -> Option<DiplomacyCommandState> {
        self.diplomacy.clone()
    }

    fn apply_diplomacy_command_transaction(
        &mut self,
        request: DiplomacyCommandRequest,
    ) -> DiplomacyCommandReceipt {
        self.diplomacy_attempts += 1;
        let Ok(DiplomacyPlanDecision::Apply(plan)) =
            plan_diplomacy_command(&request.before, &request.wire)
        else {
            return DiplomacyCommandReceipt::unavailable(request);
        };
        self.diplomacy = Some(plan.state.clone());
        self.diplomacy_commits.push(plan.clone());
        DiplomacyCommandReceipt {
            request,
            status: DiplomacyTransactionStatus::Applied,
            plan: Some(plan),
        }
    }
}

fn rename_wire() -> Vec<u8> {
    let mut wire = vec![75];
    wire.extend_from_slice(&1i32.to_le_bytes());
    wire.extend_from_slice(&3i32.to_le_bytes());
    for unit in 0..22u16 {
        wire.extend_from_slice(&(0x40 + unit).to_le_bytes());
    }
    wire
}

fn diplomacy_state() -> DiplomacyCommandState {
    let mut state = DiplomacyCommandState::default();
    for slot in 0..8 {
        state.setup.players[slot] = PlayerSetup {
            flags: 1,
            who: slot as u8,
            team: slot as i8,
        };
        state.setup.leaders[slot] = LeaderTeamState {
            leader_flags: 1,
            who: slot as i32,
            diplos: [1; 8],
        };
        state.setup.leaders[slot].diplos[slot] = DIPLO_ALLY;
    }
    state
}

fn wire9(opcode: u8, sender: i32, target: i32) -> Vec<u8> {
    let mut wire = vec![opcode];
    wire.extend_from_slice(&sender.to_le_bytes());
    wire.extend_from_slice(&target.to_le_bytes());
    wire
}

fn wire13(opcode: u8, sender: i32, target: i32, value: i32) -> Vec<u8> {
    let mut wire = wire9(opcode, sender, target);
    wire.extend_from_slice(&value.to_le_bytes());
    wire
}

#[test]
fn rename_city_reaches_one_atomic_host_commit() {
    let mut bridge = Bridge::new();
    bridge.frame = 91;
    let mut package = Package::new(1, 91);
    let mut host = TransactionHost::new();

    bridge
        .process_all(&mut package, &rename_wire(), &mut host)
        .unwrap();

    assert_eq!(host.rename_commits.len(), 1);
    assert_eq!(host.rename_commits[0].matching_group, Some(18));
    assert_eq!(bridge.stats.by_opcode[75], 1);
    assert_eq!(bridge.stats.inline_state, 1);
}

#[test]
fn cannon_time_commits_all_effects_before_the_bridge_mirrors_speed() {
    let mut bridge = Bridge::new();
    bridge.frame = 91;
    bridge.inline.speed = 2;
    bridge.inline.player_who[0] = 2;
    let mut package = Package::new(0, 91);
    let mut host = TransactionHost::new();
    host.cannon_facts = Some(CannonTimeFacts {
        package_player_who: Some(2),
        display_who: 2,
        frame: 91,
        game_cannon_time_start: 700,
        remaining_uses: Some(3),
        state: CannonTimeState {
            active_player: -1,
            start_frame: 0,
            saved_speed: 0,
            current_speed: 2,
            cannon_time_start: 0,
        },
    });

    bridge
        .process_all(&mut package, &[77, 9], &mut host)
        .unwrap();

    assert_eq!(host.cannon_commits.len(), 1);
    assert_eq!(host.cannon_commits[0].remaining_uses, Some(2));
    assert_eq!(bridge.inline.speed, 0);
}

#[test]
fn bounded_diplomacy_commits_but_external_tail_branches_stay_unavailable() {
    let mut bridge = Bridge::new();
    let mut package = Package::new(1, 0);
    let mut host = TransactionHost::new();
    host.diplomacy = Some(diplomacy_state());

    bridge
        .process_all(
            &mut package,
            &wire13(TREATY_OPCODE, 1, 4, DIPLO_ALLY),
            &mut host,
        )
        .unwrap();
    assert_eq!(host.diplomacy_commits.len(), 1);
    assert_eq!(
        host.diplomacy.as_ref().unwrap().leaders[1].proposals[4].treaty,
        DIPLO_ALLY
    );

    let before_partial_rows = host.diplomacy.clone().unwrap();
    for wire in [
        wire13(DECLARE_OPCODE, 1, 4, 0),
        wire9(ACCEPT_OPCODE, 1, 4),
        wire9(REJECT_OPCODE, 1, 4),
    ] {
        bridge.process_all(&mut package, &wire, &mut host).unwrap();
    }
    assert_eq!(host.diplomacy_attempts, 4);
    assert_eq!(host.diplomacy_commits.len(), 1);
    assert_eq!(host.diplomacy, Some(before_partial_rows));
}

#[test]
fn closure_table_keeps_only_unbounded_diplomacy_rows_red() {
    for opcode in [37, 39, 40, 43, 44, 45, 75, 77] {
        assert_eq!(InlineDef::find(opcode).unwrap().port, InlinePort::Complete);
    }
    for opcode in [38, 41, 42] {
        assert_eq!(
            InlineDef::find(opcode).unwrap().port,
            InlinePort::StateWired
        );
    }
}
