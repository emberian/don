use don_bhs::disasm::asm;
use don_bhs::{builtin, Program, Script, ScriptFile, ScriptTy, Value, VarRef};
use don_sim::bhs_session::BhsSession;
use don_sim::script_runtime::{ScriptBinding, ScriptRuntime};
use don_sim::systems::bhs_type_factory::*;
use don_sim::systems::bhs_type_runtime::{TypeBuiltinOutcome, TypeBuiltinRuntime};
use don_sim::systems::bhs_type_stat_frontier::{LeaderStatRecalc, TypeStatBuiltin};
use don_sim::systems::bhs_type_table::*;
use don_sim::tick::Sim;

fn digest(byte: u8) -> Sha256Digest {
    Sha256Digest([byte; 32])
}

fn witness(role: TypeSourceRole, component: u8) -> TypeSourceWitness {
    TypeSourceWitness {
        role,
        composition: RulesCompositionId(digest(1)),
        manifest_sha256: digest(2),
        component_sha256: digest(component),
    }
}

fn factory_input() -> TypeBuiltinFactoryInput {
    let mut rows: Vec<_> = (0..NUM_TYPES)
        .map(|slot| {
            let mut row = TypeRow::empty(slot);
            row.name = format!("Internal {slot}");
            row.type_name = format!("Family {slot}");
            row
        })
        .collect();
    rows[50].name = "Infantry".into();
    rows[50].is_list = vec![50];
    rows[51].name = "Roman Infantry".into();
    rows[51].is_list = vec![51, 50];
    rows[401].is_list = vec![401, 50];

    for (row, base) in [(50, 10), (51, 20), (401, 30)] {
        let TypeBody::Unit { object, unit } = &mut rows[row].body else {
            panic!("fixture relation row is a Unit");
        };
        object.attack = base + 1;
        object.min_range = base + 2;
        object.max_range = base + 3;
        object.hits = base + 4;
        object.armor = base + 5;
        object.los = base + 6;
        unit.moves = base + 7;
        unit.mana = base + 8;
    }

    TypeBuiltinFactoryInput {
        types: WitnessedTypeSource {
            witness: witness(TypeSourceRole::TypeRows, 3),
            value: rows
                .into_iter()
                .map(|row| {
                    Some(ComposedTypeRow {
                        index: row.index,
                        name: row.name,
                        type_name: row.type_name,
                        common: row.common,
                        from: row.from,
                        where_type: row.where_type,
                        modified: row.modified,
                        grid_x: row.grid_x,
                        grid_y: row.grid_y,
                        is_non_strict: Some(row.is_list),
                        body: row.body,
                    })
                })
                .collect(),
        },
        tribes: WitnessedTypeSource {
            witness: witness(TypeSourceRole::TribeRoster, 4),
            value: (0..NUM_TRIBES)
                .map(|slot| Some(format!("Tribe {slot}")))
                .collect(),
        },
        leaders: WitnessedTypeSource {
            witness: witness(TypeSourceRole::LeaderMasks, 5),
            value: (0..NUM_LEADERS)
                .map(|slot| {
                    Some(LeaderTypeMasks {
                        leader_flags: if slot == 0 { 3 } else { 0 },
                        ..LeaderTypeMasks::default()
                    })
                })
                .collect(),
        },
    }
}

fn one_builtin_program(index: u32, name: &str, value: i32) -> Program {
    let decl = builtin(index).unwrap();
    assert_eq!(decl.params, &[ScriptTy::Str, ScriptTy::Int]);
    let args = vec![Value::str(name), Value::Int(value)];
    let instructions: Vec<(u8, Vec<u32>)> = vec![
        (0x47, vec![0]),
        (0x26, vec![VarRef::Const(1).encode()]),
        (0x26, vec![VarRef::Const(0).encode()]),
        (0x38, vec![index]),
        (0x27, Vec::new()),
        (0x3e, Vec::new()),
    ];
    let borrowed: Vec<_> = instructions
        .iter()
        .map(|(opcode, operands)| (*opcode, operands.as_slice()))
        .collect();
    Program::single(ScriptFile {
        code: asm(&borrowed),
        const_pool: args,
        scripts: vec![Script {
            name: "game_tick".into(),
            entry: 0,
            return_type: ScriptTy::Void.tag(),
            ..Default::default()
        }],
        ..Default::default()
    })
}

fn field_value(state: &TypeBuiltinState, builtin: TypeStatBuiltin, row: usize) -> i32 {
    match (&state.types.row(row).body, builtin) {
        (TypeBody::Unit { object, .. }, TypeStatBuiltin::SetMaxHealth) => object.hits,
        (TypeBody::Unit { object, .. }, TypeStatBuiltin::SetArmor) => object.armor,
        (TypeBody::Unit { object, .. }, TypeStatBuiltin::SetAttack) => object.attack,
        (TypeBody::Unit { object, .. }, TypeStatBuiltin::SetMaxRange) => object.max_range,
        (TypeBody::Unit { object, .. }, TypeStatBuiltin::SetMinRange) => object.min_range,
        (TypeBody::Unit { unit, .. }, TypeStatBuiltin::SetUnitSpeed) => unit.moves,
        (TypeBody::Unit { unit, .. }, TypeStatBuiltin::SetUnitMaxCraft) => unit.mana,
        (TypeBody::Unit { object, .. }, TypeStatBuiltin::SetLineOfSight) => object.los,
        _ => panic!("fixture field/domain mismatch"),
    }
}

#[test]
fn all_eight_generated_declarations_execute_through_the_opaque_session() {
    for (offset, builtin_kind) in TypeStatBuiltin::ALL.into_iter().enumerate() {
        let index = u32::from(builtin_kind.registration());
        let value = 40 + offset as i32;
        let scripts = ScriptRuntime::new(
            one_builtin_program(index, "Infantry", value),
            Some(ScriptBinding::new(0, "game_tick")),
            None,
        )
        .unwrap();
        let mut sim = Sim::new(0x529_u64 + offset as u64, 8);
        sim.step8.leaders[0].flags = 3;
        let mut session = BhsSession::new(sim, scripts, factory_input()).unwrap();

        session.do_frame().unwrap();

        for row in [50, 51, 401] {
            assert_eq!(field_value(session.type_state(), builtin_kind, row), value);
            assert_eq!(session.type_state().types.row(row).modified, 1);
        }
        let receipt = session.last_type_builtin_receipt().unwrap();
        assert_eq!(receipt.index, index);
        assert_eq!(
            receipt.outcome,
            TypeBuiltinOutcome::Returned(if index == 814 { 1 } else { 50 })
        );
        assert_eq!(receipt.revision_before, 0);
        assert_eq!(receipt.revision_after, 1);
        assert!(receipt.leader_recalcs_applied);
        assert_eq!(receipt.leader_recalcs.len(), 1);
        assert_eq!(receipt.leader_recalcs[0].leader_slot, 0);
        assert_eq!(
            receipt.leader_recalcs[0].kind,
            if index == 538 {
                LeaderStatRecalc::UnitOnly
            } else {
                LeaderStatRecalc::WallThenUnit
            }
        );
        assert_eq!(session.type_provenance().manifest_sha256, digest(2));
        assert!(session.save().is_err());
        assert!(session.partial_channel_digest().is_err());
    }
}

#[test]
fn detached_runtime_receipt_keeps_the_required_cache_tail_pending() {
    let produced = produce_type_builtin_state(factory_input()).unwrap();
    let (state, _) = produced.into_parts();
    let mut runtime = TypeBuiltinRuntime::new(state);
    let receipt = runtime
        .dispatch(
            builtin(532).unwrap(),
            &[Value::str("Infantry"), Value::Int(77)],
        )
        .unwrap()
        .unwrap();

    assert_eq!(receipt.outcome, TypeBuiltinOutcome::Returned(50));
    assert!(receipt.mutated());
    assert_eq!(receipt.leader_recalcs.len(), 1);
    assert!(!receipt.leader_recalcs_applied);
}

#[test]
fn live_script_host_rebuilds_a_captain_cache_from_the_mutated_owner_row() {
    let produced = produce_type_builtin_state(factory_input()).unwrap();
    let (state, _) = produced.into_parts();
    let mut scripts = ScriptRuntime::new(
        one_builtin_program(531, "Infantry", 77),
        Some(ScriptBinding::new(0, "game_tick")),
        None,
    )
    .unwrap();
    scripts.install_type_builtins(state).unwrap();

    let mut sim = Sim::new(0x531, 8);
    sim.activate(0);
    let unit = sim.spawn_unit(0, 50, 192, 192, 2).unwrap();
    let row = sim.world.row_of(unit).unwrap();
    sim.world.units.o_up_mut()[row] = -1;
    sim.world.units.o_down_mut()[row] = -1;

    sim.do_frame_with_scripts(&mut scripts).unwrap();

    assert_eq!(sim.world.units.myarmor()[row], 77);
    let receipt = scripts.last_type_builtin_receipt().unwrap();
    assert_eq!(receipt.outcome, TypeBuiltinOutcome::Returned(50));
    assert!(receipt.leader_recalcs_applied);
}

#[test]
fn session_rejects_a_split_leader_gate_owner_before_installation() {
    let scripts = ScriptRuntime::new(
        one_builtin_program(532, "Infantry", 77),
        Some(ScriptBinding::new(0, "game_tick")),
        None,
    )
    .unwrap();
    let sim = Sim::new(0x532, 8);
    assert!(matches!(
        BhsSession::new(sim, scripts, factory_input()),
        Err(
            don_sim::bhs_session::BhsSessionSetupError::LeaderFlagsMismatch {
                slot: 0,
                type_owner: 3,
                sim: 0,
            }
        )
    ));
}
