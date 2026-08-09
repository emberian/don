use don_bhs::chunk::load_program;
use don_bhs::disasm::{asm, asm_len};
use don_bhs::{find_builtin, Program, Script, ScriptFile, ScriptTy, Value, VarRef, VmError};
use don_bhs_cc::sema::{self, Severity};
use don_sim::order::Order;
use don_sim::rng::Random;
use don_sim::script_runtime::{
    ScriptBindError, ScriptBinding, ScriptFailure, ScriptOutput, ScriptRuntime, ScriptSlot,
};
use don_sim::systems::{
    leaders,
    map_terrain::{land, wflag},
    victory_score,
};
use don_sim::tick::{Sim, StepRun};
use std::path::Path;

fn void_script(name: &str, entry: u32) -> Script {
    Script {
        name: name.to_string(),
        entry,
        return_type: ScriptTy::Void.tag(),
        ..Default::default()
    }
}

fn print_order_program() -> Program {
    let mut code = asm(&[
        (0x47, &[0]),
        (0x26, &[VarRef::Const(0).encode()]),
        (0x38, &[find_builtin("print_line").unwrap().index]),
        (0x3e, &[]),
    ]);
    let general_entry = code.len() as u32;
    code.extend_from_slice(&asm(&[
        (0x47, &[1]),
        (0x26, &[VarRef::Const(1).encode()]),
        (0x38, &[find_builtin("print_line").unwrap().index]),
        (0x3e, &[]),
    ]));
    Program::single(ScriptFile {
        code,
        const_pool: vec![Value::str("game"), Value::str("general")],
        scripts: vec![
            void_script("game_tick", 0),
            void_script("general_powers", general_entry),
        ],
        ..Default::default()
    })
}

fn one_builtin_program(name: &str, args: &[Value]) -> Program {
    let builtin = find_builtin(name).unwrap();
    assert_eq!(builtin.arity as usize, args.len());
    let mut instructions: Vec<(u8, Vec<u32>)> = vec![(0x47, vec![0])];
    // Retail call convention: logical arguments are emitted right-to-left.
    for i in (0..args.len()).rev() {
        instructions.push((0x26, vec![VarRef::Const(i as u32).encode()]));
    }
    instructions.push((0x38, vec![builtin.index]));
    if builtin.ret != ScriptTy::Void {
        instructions.push((0x27, Vec::new()));
    }
    instructions.push((0x3e, Vec::new()));
    let borrowed: Vec<(u8, &[u32])> = instructions
        .iter()
        .map(|(op, operands)| (*op, operands.as_slice()))
        .collect();
    Program::single(ScriptFile {
        code: asm(&borrowed),
        const_pool: args.to_vec(),
        scripts: vec![void_script("game_tick", 0)],
        ..Default::default()
    })
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_chunk_string(out: &mut Vec<u8>, value: &str) {
    let units = value.encode_utf16().collect::<Vec<_>>();
    push_u32(out, units.len() as u32);
    for unit in units {
        out.extend_from_slice(&unit.to_le_bytes());
    }
}

fn chunk(tag: u16, payload: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + payload.len());
    push_u32(&mut out, (8 + payload.len()) as u32);
    out.extend_from_slice(&tag.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&payload);
    out
}

fn loaded_one_builtin_program(name: &str, args: &[Value]) -> Program {
    let compiled = one_builtin_program(name, args);
    loaded_scalar_program(compiled)
}

fn loaded_scalar_program(compiled: Program) -> Program {
    let file = &compiled.files[0];
    let entry = &file.scripts[0];

    let mut script = Vec::new();
    push_u32(&mut script, 0);
    push_chunk_string(&mut script, &entry.name);
    push_u32(&mut script, entry.entry);
    push_u32(&mut script, entry.script_type);
    push_u32(&mut script, entry.return_type);
    push_u32(&mut script, 0); // trigger_count
    push_u32(&mut script, 0); // param_count

    let mut children = vec![chunk(2, script)];
    for value in &file.const_pool {
        let mut payload = Vec::new();
        match value {
            Value::Int(value) => {
                push_u32(&mut payload, ScriptTy::Int.tag());
                push_u32(&mut payload, *value as u32);
            }
            Value::Str(value) => {
                push_u32(&mut payload, ScriptTy::Str.tag());
                push_chunk_string(&mut payload, value);
            }
            _ => panic!("loaded fixture uses only shipped scalar chunk constants"),
        }
        children.push(chunk(3, payload));
    }
    children.push(chunk(4, file.code.clone()));

    let size = 8 + children.iter().map(Vec::len).sum::<usize>();
    let mut root = Vec::with_capacity(size);
    push_u32(&mut root, size as u32);
    root.extend_from_slice(&0u16.to_le_bytes());
    root.extend_from_slice(&(children.len() as u16).to_le_bytes());
    for child in children {
        root.extend_from_slice(&child);
    }
    load_program(&root, "loaded_scenario_runtime.bhs").unwrap()
}

fn position_to_stockpile_program() -> Program {
    let position = find_builtin("object_position_x").unwrap();
    let set_good = find_builtin("set_good").unwrap();
    // `set_good(1, "Food", object_position_x(1, 1))`. The inner return remains on the
    // stack while the outer call's remaining arguments are emitted right-to-left.
    Program::single(ScriptFile {
        code: asm(&[
            (0x47, &[0]),
            (0x26, &[VarRef::Const(1).encode()]),
            (0x26, &[VarRef::Const(0).encode()]),
            (0x38, &[position.index]),
            (0x26, &[VarRef::Const(2).encode()]),
            (0x26, &[VarRef::Const(0).encode()]),
            (0x38, &[set_good.index]),
            (0x27, &[]),
            (0x3e, &[]),
        ]),
        const_pool: vec![Value::Int(1), Value::Int(1), Value::str("Food")],
        scripts: vec![void_script("game_tick", 0)],
        ..Default::default()
    })
}

fn game_runtime(program: Program) -> ScriptRuntime {
    ScriptRuntime::new(program, Some(ScriptBinding::new(0, "game_tick")), None).unwrap()
}

fn compile_source_fixture(name: &str) -> Program {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let includes = sema::IncludePath::with_roots([path.parent().unwrap().to_path_buf()]);
    let unit = sema::analyze(&path, &includes).unwrap();
    let (program, codegen_diags, _) = don_bhs_cc::codegen::compile(&unit);
    let errors = unit
        .diags
        .iter()
        .chain(codegen_diags.iter())
        .filter(|diag| diag.severity == Severity::Error)
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "{}", errors.join("\n"));
    program
}

fn static_counter_program() -> Program {
    let static_ref = VarRef::Static(0).encode();
    let guard = [
        (0x47u8, &[0][..]),
        (0x44, &[static_ref, 0][..]),
        (0x26, &[VarRef::Const(0).encode()][..]),
        (0x32, &[static_ref][..]),
    ];
    let body_at = asm_len(&guard) as u32;
    Program::single(ScriptFile {
        code: asm(&[
            (0x47, &[0]),
            (0x44, &[static_ref, body_at]),
            (0x26, &[VarRef::Const(0).encode()]),
            (0x32, &[static_ref]),
            (0x26, &[static_ref]),
            (0x26, &[VarRef::Const(1).encode()]),
            (0x04, &[]),
            (0x33, &[static_ref]),
            (0x3e, &[]),
        ]),
        const_pool: vec![Value::Int(0), Value::Int(1)],
        scripts: vec![Script {
            static_var_names: vec!["counter".to_string()],
            ..void_script("game_tick", 0)
        }],
        ..Default::default()
    })
}

#[test]
fn step_four_runs_both_slots_in_recovered_order_with_real_work() {
    let mut scripts = ScriptRuntime::new(
        print_order_program(),
        Some(ScriptBinding::new(0, "game_tick")),
        Some(ScriptBinding::new(0, "general_powers")),
    )
    .unwrap();
    let mut sim = Sim::new(7, 8);

    // Retail gates general powers on the pre-increment Game::frame being positive.
    let first = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(first.steps[4], StepRun::Executed);
    assert!(
        first.work[4] > 0,
        "a call count without bytecode is vacuous"
    );
    assert_eq!(
        scripts.output(),
        [ScriptOutput {
            text: "game".to_string(),
            newline: true,
        }]
    );
    assert_eq!(sim.world.frame, 1);

    let second = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(second.steps[4], StepRun::Executed);
    assert!(second.work[4] > first.work[4]);
    assert_eq!(
        scripts.output(),
        [
            ScriptOutput {
                text: "game".to_string(),
                newline: true,
            },
            ScriptOutput {
                text: "game".to_string(),
                newline: true,
            },
            ScriptOutput {
                text: "general".to_string(),
                newline: true,
            },
        ]
    );
    assert_eq!(scripts.calls(), 3);
    assert!(scripts.bytecodes() >= (first.work[4] + second.work[4]) as u64);
    assert_eq!(sim.world.frame, 2);
}

#[test]
fn script_statics_are_a_real_cross_frame_state_producer() {
    let mut scripts = game_runtime(static_counter_program());
    let mut sim = Sim::new(17, 8);
    let first = sim.do_frame_with_scripts(&mut scripts).unwrap();
    let second = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert!(first.work[4] > 0 && second.work[4] > 0);
    assert_eq!(
        scripts.program().files[0].scripts[0].statics,
        [Some(Value::Int(2))],
        "step 4 must retain Script::static_vars between frames"
    );
}

#[test]
fn utility_rng_uses_the_simulation_stream() {
    let seed = 0x1234_5678u64;
    let mut expected = Random::new(seed as i32);
    let _ = expected.get(10, 20);

    let mut scripts = game_runtime(one_builtin_program(
        "rand_int",
        &[Value::Int(10), Value::Int(20)],
    ));
    let mut sim = Sim::new(seed, 8);
    let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(trace.steps[4], StepRun::Executed);
    assert_eq!(sim.world.random.state(), expected.state());
}

#[test]
fn ordinary_source_runtime_mutates_world_and_persists_retail_timers() {
    let program = compile_source_fixture("scenario_runtime.bhs");
    let mut scripts =
        ScriptRuntime::new(program, Some(ScriptBinding::new(0, "scenario_tick")), None).unwrap();
    let mut sim = Sim::new(0x1234, 8);
    sim.activate(0);
    sim.activate(1);
    sim.leaders[0].econ.age_alt = 4;
    // The first queried tile has retail's water mask and the second remains land.
    // A constant map answer changes the asserted Oil stockpile in either direction.
    sim.map.world.tdata[0] = 0x20;
    // `is_defeated` reads bit 6 from the exact Leader flags byte.
    sim.step8.leaders[0].flags |= 1 << 6;

    let first = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(first.steps[4], StepRun::Executed);
    assert!(first.work[4] > 0, "source-compiled script did no VM work");
    assert_eq!(
        sim.leaders[0].econ.stockpile,
        [15, 32, 2, 4, 101, 2],
        "map, player, leader, clock, timer, and resource handlers all feed live state"
    );
    assert_eq!(sim.leaders[0].gather_ctx.extra_income[2], 3 << 4);

    // The timer was set at Game::seconds 0 for one second. Step 23 reaches second 1
    // only after frame 14; the next step-4 call observes and consumes the timer.
    sim.run_with_scripts(&mut scripts, 14).unwrap();
    assert_eq!(sim.world.seconds, 1);
    assert_eq!(sim.leaders[0].econ.stockpile[0], 15);

    let expiry = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(expiry.steps[4], StepRun::Executed);
    assert_eq!(sim.leaders[0].econ.stockpile[0], 22);

    // `ScriptTimers::check` removes an expired entry. The following absent check
    // returns -1, so the source branch does not award the resource twice.
    sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(sim.leaders[0].econ.stockpile[0], 22);
    assert_eq!(
        scripts.program().files[0].scripts[0].statics,
        [Some(Value::Int(2))]
    );
}

#[test]
fn retail_chunk_loader_program_uses_the_same_mandatory_world_host() {
    let program = loaded_one_builtin_program(
        "give_good",
        &[Value::Int(1), Value::str("Food"), Value::Int(9)],
    );
    assert!(
        program.walk_meta().is_some(),
        "test must execute the normal chunk-loader producer, not a hand-built Program"
    );
    let mut scripts = game_runtime(program);
    let mut sim = Sim::new(0x5678, 8);
    sim.activate(0);

    let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(trace.steps[4], StepRun::Executed);
    assert!(trace.work[4] > 0);
    assert_eq!(sim.leaders[0].econ.stockpile[0], 9);
}

#[test]
fn ordinary_source_executes_unit_and_building_position_readers() {
    let program = compile_source_fixture("scenario_object_positions.bhs");
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "object_position_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x8119, 8);
    sim.activate(0);
    let unit = sim
        .spawn_unit(0, 7, 11 * 192 + 191, 13 * 192 + 1, 4)
        .unwrap();
    let unit_row = sim.world.row_of(unit).unwrap();
    sim.world.units.o_up_mut()[unit_row] = -1;
    sim.world.units.inside_up_mut()[unit_row] = -1;
    sim.world.units.inside_up_who_mut()[unit_row] = -1;

    let mut build = don_sim::systems::production::BuildData::default();
    build.flags = don_sim::systems::production::flag::VALID;
    build.other[don_sim::systems::production::off::OBJECT_ID
        ..don_sim::systems::production::off::OBJECT_ID + 2]
        .copy_from_slice(&2000i16.to_le_bytes());
    build.other[don_sim::systems::production::off::X_INTERNAL
        ..don_sim::systems::production::off::X_INTERNAL + 4]
        .copy_from_slice(&((17 * 192) ^ 0x63637i32).to_le_bytes());
    build.other[don_sim::systems::production::off::Y_INTERNAL
        ..don_sim::systems::production::off::Y_INTERNAL + 4]
        .copy_from_slice(&((19 * 192 + 191) ^ 0x63637i32).to_le_bytes());
    sim.spawn_build(0, build);

    let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(trace.steps[4], StepRun::Executed);
    assert!(trace.work[4] > 0);
    assert_eq!(
        sim.leaders[0].econ.stockpile[..4],
        [11, 13, 17, 19],
        "the source compiler must execute both unit and building address paths"
    );
}

#[test]
fn retail_chunk_position_reader_resolves_captain_and_outer_container() {
    let program = loaded_scalar_program(position_to_stockpile_program());
    assert!(program.walk_meta().is_some());
    let mut scripts = game_runtime(program);
    let mut sim = Sim::new(0x8120, 8);
    sim.activate(0);

    let queried = sim.spawn_unit(0, 7, 2 * 192, 3 * 192, 4).unwrap();
    let captain = sim.spawn_unit(0, 7, 5 * 192, 7 * 192, 4).unwrap();
    let queried_row = sim.world.row_of(queried).unwrap();
    let captain_row = sim.world.row_of(captain).unwrap();
    sim.world.units.o_up_mut()[queried_row] = 1;
    sim.world.units.inside_up_mut()[queried_row] = -1;
    sim.world.units.inside_up_who_mut()[queried_row] = -1;
    sim.world.units.o_up_mut()[captain_row] = -1;
    sim.world.units.inside_up_mut()[captain_row] = 2000;
    sim.world.units.inside_up_who_mut()[captain_row] = 0;

    let mut build = don_sim::systems::production::BuildData::default();
    build.flags = don_sim::systems::production::flag::VALID;
    build.other[don_sim::systems::production::off::OBJECT_ID
        ..don_sim::systems::production::off::OBJECT_ID + 2]
        .copy_from_slice(&2000i16.to_le_bytes());
    build.other[don_sim::systems::production::off::X_INTERNAL
        ..don_sim::systems::production::off::X_INTERNAL + 4]
        .copy_from_slice(&((23 * 192 + 191) ^ 0x63637i32).to_le_bytes());
    build.other[don_sim::systems::production::off::Y_INTERNAL
        ..don_sim::systems::production::off::Y_INTERNAL + 4]
        .copy_from_slice(&((29 * 192) ^ 0x63637i32).to_le_bytes());
    sim.spawn_build(0, build);

    let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(trace.steps[4], StepRun::Executed);
    assert!(trace.work[4] > 0);
    assert_eq!(sim.leaders[0].econ.stockpile[0], 23);
}

#[test]
fn ordinary_source_executes_world_dimensions_and_full_clock_family() {
    let program = compile_source_fixture("scenario_world_clock.bhs");
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "world_clock_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x8121, 8);
    sim.activate(0);
    sim.world.seconds = 125;

    let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(trace.steps[4], StepRun::Executed);
    assert!(trace.work[4] > 0);
    assert_eq!(
        sim.leaders[0].econ.stockpile,
        [32, 32, 2, 2, 125, 1],
        "tile dimensions, both minute aliases, seconds, and strict earlier-than must execute"
    );
}

#[test]
fn retail_chunk_executes_the_same_world_and_clock_handlers() {
    let compiled = compile_source_fixture("scenario_world_clock.bhs");
    let program = loaded_scalar_program(compiled);
    assert!(program.walk_meta().is_some());
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "world_clock_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x8122, 12);
    sim.activate(0);
    sim.world.seconds = 239;

    let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(trace.steps[4], StepRun::Executed);
    assert!(trace.work[4] > 0);
    assert_eq!(
        sim.leaders[0].econ.stockpile,
        [48, 48, 3, 3, 239, 0],
        "loaded chunks must retain the strict time comparison and live map dimensions"
    );
}

const SCRIPT_EXPLORED_FOG_CELLS: &[(i32, i32)] = &[
    (1, 1),
    (1, 2),
    (1, 3),
    (1, 4),
    (2, 1),
    (2, 2),
    (2, 3),
    (2, 4),
    (3, 1),
    (3, 2),
    (3, 3),
    (3, 4),
    (4, 2),
    (4, 3),
    (4, 4),
];

const SCRIPT_EXPLORED_WORLD_CELLS: &[(i32, i32)] = &[
    (0, 0),
    (0, 1),
    (0, 2),
    (1, 0),
    (1, 1),
    (1, 2),
    (2, 1),
    (2, 2),
];

const SCRIPT_ALL_PLAYER_FOG_CELLS: &[(i32, i32)] = &[(6, 6), (6, 7), (7, 6), (7, 7)];

fn configure_fog_effect_state(sim: &mut Sim) {
    sim.activate(0);
    sim.activate(1);
    // The all-player #67 arm must include an in-game slot whose PROCESS bit is clear.
    sim.step8.leaders[1].flags &= !leaders::flag::PROCESS;
    // Scenario handlers validate the instruction-derived Leader flags. Keeping the
    // later-step facade inactive makes step 12 vacuous, so the test can inspect all
    // five planes immediately after the step-4 mutation instead of only seen2.
    sim.leaders[0].active = false;
    sim.leaders[1].active = false;
}

fn assert_script_explored_disc(sim: &Sim) {
    let world = &sim.map.world;
    for fog_y in 0..world.fog_ys {
        for fog_x in 0..world.fog_xs {
            let expected = if SCRIPT_ALL_PLAYER_FOG_CELLS.contains(&(fog_x, fog_y)) {
                0b11
            } else {
                SCRIPT_EXPLORED_FOG_CELLS.contains(&(fog_x, fog_y)) as u8
            };
            let index = world.f_index(fog_x, fog_y);
            assert_eq!(world.seen[index], expected, "seen at ({fog_x},{fog_y})");
            assert_eq!(world.seen2[index], expected, "seen2 at ({fog_x},{fog_y})");
            assert_eq!(world.seen3[index], expected, "seen3 at ({fog_x},{fog_y})");
        }
    }
    for world_y in 0..world.ys {
        for world_x in 0..world.xs {
            let expected = if (world_x, world_y) == (3, 3) {
                0b11
            } else {
                SCRIPT_EXPLORED_WORLD_CELLS.contains(&(world_x, world_y)) as u8
            };
            let index = world.w_index(world_x, world_y);
            assert_eq!(
                world.wdata[index].was_seen, expected,
                "WData::was_seen at ({world_x},{world_y})"
            );
            assert_eq!(
                world.wcoord_seen[index], expected,
                "wcoord_seen at ({world_x},{world_y})"
            );
        }
    }
}

fn execute_fog_effect_sequence(sim: &mut Sim, scripts: &mut ScriptRuntime) {
    for (seconds, expected_show_all) in [(0, true), (1, false), (2, true), (3, false)] {
        sim.world.seconds = seconds;
        let trace = sim.do_frame_with_scripts(scripts).unwrap();
        assert_eq!(trace.steps[4], StepRun::Executed);
        assert!(trace.work[4] > 0);
        assert_eq!(
            sim.map.fog.leaders[0].see_all, expected_show_all,
            "whole-map mutation at second {seconds}"
        );
        assert_script_explored_disc(sim);
    }
}

#[test]
fn ordinary_source_executes_authoritative_fog_effects() {
    let program = compile_source_fixture("scenario_fog_effects.bhs");
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "fog_effects_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x8136, 8);
    configure_fog_effect_state(&mut sim);

    execute_fog_effect_sequence(&mut sim, &mut scripts);
}

#[test]
fn retail_chunk_executes_the_same_authoritative_fog_effects() {
    let compiled = compile_source_fixture("scenario_fog_effects.bhs");
    let program = loaded_scalar_program(compiled);
    assert!(program.walk_meta().is_some());
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "fog_effects_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x8137, 8);
    configure_fog_effect_state(&mut sim);

    execute_fog_effect_sequence(&mut sim, &mut scripts);
}

fn configure_ai_policy_effect_state(sim: &mut Sim) {
    for who in 0..3 {
        sim.activate(who);
        // Keep the later GameDaemon facade vacuous; these handlers gate on the exact
        // step-8 flags and mutate checksum channel 8 directly.
        sim.leaders[who].active = false;
    }

    // Script player 2 is valid but non-processing: the production/combat/unit AI pairs
    // must still accept it through their one-bit gate.
    sim.step8.leaders[1].flags &= !leaders::flag::PROCESS;
    sim.vic_leaders.slots[1].leader_flags &= !victory_score::leader_flag::ACTIVE;

    // Script player 3 is active but human. City AI must reject it while city defeat
    // remains a legal mutation.
    sim.vic_leaders.slots[2].leader_flags |= victory_score::leader_flag::HUMAN;

    sim.vic_leaders.slots[0].leader_flags2 = 0x4000;
    sim.vic_leaders.slots[1].leader_flags2 = 0x8000;
    sim.vic_leaders.slots[2].leader_flags2 = 0x2010;
}

fn execute_ai_policy_effect_sequence(sim: &mut Sim, scripts: &mut ScriptRuntime) {
    sim.world.seconds = 0;
    let disabled = sim.do_frame_with_scripts(scripts).unwrap();
    assert_eq!(disabled.steps[4], StepRun::Executed);
    assert!(disabled.work[4] > 0);
    assert_eq!(sim.vic_leaders.slots[0].leader_flags2, 0x4011);
    assert_eq!(sim.vic_leaders.slots[1].leader_flags2, 0x800e);
    assert_eq!(
        sim.vic_leaders.slots[2].leader_flags2, 0x2011,
        "human city AI is rejected while human city defeat is disabled"
    );

    sim.world.seconds = 1;
    let enabled = sim.do_frame_with_scripts(scripts).unwrap();
    assert_eq!(enabled.steps[4], StepRun::Executed);
    assert!(enabled.work[4] > 0);
    assert_eq!(sim.vic_leaders.slots[0].leader_flags2, 0x4000);
    assert_eq!(sim.vic_leaders.slots[1].leader_flags2, 0x8000);
    assert_eq!(
        sim.vic_leaders.slots[2].leader_flags2, 0x2010,
        "human city AI remains disabled while city defeat is re-enabled"
    );
}

#[test]
fn ordinary_source_executes_leader_ai_policy_effects() {
    let program = compile_source_fixture("scenario_ai_policy_effects.bhs");
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "ai_policy_effects_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x8138, 8);
    configure_ai_policy_effect_state(&mut sim);

    execute_ai_policy_effect_sequence(&mut sim, &mut scripts);
}

#[test]
fn retail_chunk_executes_the_same_leader_ai_policy_effects() {
    let compiled = compile_source_fixture("scenario_ai_policy_effects.bhs");
    let program = loaded_scalar_program(compiled);
    assert!(program.walk_meta().is_some());
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "ai_policy_effects_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x8139, 8);
    configure_ai_policy_effect_state(&mut sim);

    execute_ai_policy_effect_sequence(&mut sim, &mut scripts);
}

const SCRIPT_UNIT_MASK_AI_OFF: u32 = 0x0100_0000;

fn configure_unit_ai_effect_state(sim: &mut Sim) -> [usize; 3] {
    sim.activate(0);
    sim.activate(1);

    // Script player 2 is in-game but not processing. A positive address must reject,
    // while the negative all-unit sentinel still mutates LeaderFlag2.
    sim.step8.leaders[1].flags &= !leaders::flag::PROCESS;
    sim.vic_leaders.slots[1].leader_flags &= !victory_score::leader_flag::ACTIVE;
    sim.vic_leaders.slots[0].leader_flags2 = 0x4000;
    sim.vic_leaders.slots[1].leader_flags2 = 0x8000;

    let captain = sim.spawn_unit(0, 7, 2 * 192, 3 * 192, 4).unwrap();
    let addressed = sim.spawn_unit(0, 7, 4 * 192, 5 * 192, 4).unwrap();
    let tail = sim.spawn_unit(0, 7, 6 * 192, 7 * 192, 4).unwrap();
    let rows = [
        sim.world.row_of(captain).unwrap(),
        sim.world.row_of(addressed).unwrap(),
        sim.world.row_of(tail).unwrap(),
    ];
    sim.world.units.o_up_mut()[rows[0]] = -1;
    sim.world.units.o_down_mut()[rows[0]] = 1;
    sim.world.units.o_up_mut()[rows[1]] = 0;
    sim.world.units.o_down_mut()[rows[1]] = 2;
    sim.world.units.o_up_mut()[rows[2]] = 0;
    sim.world.units.o_down_mut()[rows[2]] = -1;

    // `active_unit_slot` admits an inactive formation member when `o_up >= 0`.
    let addressed_flags = sim.world.units.get_flags(rows[1]);
    sim.world.units.set_flags(rows[1], addressed_flags & !1);
    for (row, masks) in rows
        .into_iter()
        .zip([0x0040_0000, 0x0080_0000, 0x0200_0000])
    {
        sim.world.units.set_unit_masks(row, masks);
    }
    rows
}

fn execute_unit_ai_effect_sequence(sim: &mut Sim, scripts: &mut ScriptRuntime, rows: [usize; 3]) {
    let original = [0x0040_0000, 0x0080_0000, 0x0200_0000];

    sim.world.seconds = 0;
    let disabled = sim.do_frame_with_scripts(scripts).unwrap();
    assert_eq!(disabled.steps[4], StepRun::Executed);
    assert!(disabled.work[4] > 0);
    for (row, masks) in rows.into_iter().zip(original) {
        assert_eq!(
            sim.world.units.get_unit_masks(row),
            masks | SCRIPT_UNIT_MASK_AI_OFF,
            "a subordinate address must toggle the full captain-to-tail formation"
        );
    }
    assert_eq!(sim.vic_leaders.slots[0].leader_flags2, 0x4000);
    assert_eq!(sim.vic_leaders.slots[1].leader_flags2, 0x8002);

    sim.world.seconds = 1;
    let enabled = sim.do_frame_with_scripts(scripts).unwrap();
    assert_eq!(enabled.steps[4], StepRun::Executed);
    assert!(enabled.work[4] > 0);
    for (row, masks) in rows.into_iter().zip(original) {
        assert_eq!(sim.world.units.get_unit_masks(row), masks);
    }
    assert_eq!(sim.vic_leaders.slots[0].leader_flags2, 0x4000);
    assert_eq!(sim.vic_leaders.slots[1].leader_flags2, 0x8000);
}

#[test]
fn ordinary_source_executes_unit_specific_ai_effects() {
    let program = compile_source_fixture("scenario_unit_ai_effects.bhs");
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "unit_ai_effects_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x813a, 8);
    let rows = configure_unit_ai_effect_state(&mut sim);

    execute_unit_ai_effect_sequence(&mut sim, &mut scripts, rows);
}

#[test]
fn retail_chunk_executes_the_same_unit_specific_ai_effects() {
    let compiled = compile_source_fixture("scenario_unit_ai_effects.bhs");
    let program = loaded_scalar_program(compiled);
    assert!(program.walk_meta().is_some());
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "unit_ai_effects_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x813b, 8);
    let rows = configure_unit_ai_effect_state(&mut sim);

    execute_unit_ai_effect_sequence(&mut sim, &mut scripts, rows);
}

#[test]
fn unit_ai_effect_fails_closed_before_a_malformed_formation_write() {
    let mut scripts = game_runtime(one_builtin_program(
        "disable_unit_ai",
        &[Value::Int(1), Value::Int(0)],
    ));
    let mut sim = Sim::new(0x813c, 8);
    sim.activate(0);
    let captain = sim.spawn_unit(0, 7, 2 * 192, 3 * 192, 4).unwrap();
    let row = sim.world.row_of(captain).unwrap();
    sim.world.units.o_up_mut()[row] = -1;
    sim.world.units.o_down_mut()[row] = 1;
    sim.world.units.set_unit_masks(row, 0x0040_0000);

    let error = sim.do_frame_with_scripts(&mut scripts).unwrap_err();
    assert!(matches!(
        error.failure,
        ScriptFailure::Vm(VmError::UnimplementedBuiltin {
            name: "disable_unit_ai",
            ..
        })
    ));
    assert_eq!(sim.world.units.get_unit_masks(row), 0x0040_0000);
}

const SCRIPT_CAN_TRANSPORT: u32 = 0x0080_0000;

fn configure_force_transport_effect_state(sim: &mut Sim) -> [usize; 6] {
    use don_sim::systems::ammo::ShooterRules;

    sim.activate(0);
    sim.activate(1);
    // The handler's Leader gate is only VALID, not VALID|ACTIVE.
    sim.step8.leaders[0].flags &= !leaders::flag::PROCESS;
    sim.vic_leaders.slots[0].leader_flags &= !victory_score::leader_flag::ACTIVE;
    sim.vic_leaders.slots[0].leader_flags |= 0x4000;
    sim.vic_leaders.slots[0].leader_flags2 = 0x8000;

    let ground = sim.spawn_unit(0, 7, 2 * 192, 3 * 192, 4).unwrap();
    let inactive = sim.spawn_unit(0, 8, 4 * 192, 5 * 192, 4).unwrap();
    let transport = sim.spawn_unit(0, 320, 6 * 192, 7 * 192, 4).unwrap();
    let warship = sim.spawn_unit(0, 323, 8 * 192, 9 * 192, 4).unwrap();
    let carrier = sim.spawn_unit(0, 351, 10 * 192, 11 * 192, 4).unwrap();
    let foreign = sim.spawn_unit(1, 7, 12 * 192, 13 * 192, 4).unwrap();
    let rows = [ground, inactive, transport, warship, carrier, foreign]
        .map(|handle| sim.world.row_of(handle).unwrap());

    let inactive_flags = sim.world.units.get_flags(rows[1]);
    sim.world.units.set_flags(rows[1], inactive_flags & !1);
    for (row, masks) in rows
        .into_iter()
        .zip([0x100, 0x200, 0x400, 0x800, 0x1000, 0x2000])
    {
        sim.world.units.set_unit_masks(row, masks);
    }

    // `ShooterRules::domain` is the installed ObjectTypeData+0x218 projection.
    // The inactive type intentionally has no row: retail skips it before type access.
    sim.shooter_rules.push((
        7,
        ShooterRules {
            domain: 0,
            ..Default::default()
        },
    ));
    for type_id in [320, 323, 351] {
        sim.shooter_rules.push((
            type_id,
            ShooterRules {
                domain: 1,
                ..Default::default()
            },
        ));
    }
    rows
}

fn execute_force_transport_effect(sim: &mut Sim, scripts: &mut ScriptRuntime, rows: [usize; 6]) {
    let trace = sim.do_frame_with_scripts(scripts).unwrap();
    assert_eq!(trace.steps[4], StepRun::Executed);
    assert!(trace.work[4] > 0);
    assert_eq!(sim.vic_leaders.slots[0].leader_flags, 0x4701);
    assert_eq!(sim.vic_leaders.slots[0].leader_flags2, 0x8020);
    assert_eq!(
        rows.map(|row| sim.world.units.get_unit_masks(row)),
        [
            0x100 | SCRIPT_CAN_TRANSPORT,
            0x200,
            0x400 | SCRIPT_CAN_TRANSPORT,
            0x800,
            0x1000,
            0x2000,
        ],
        "only active ground units and non-carrier sea transports owned by the player mutate"
    );
}

#[test]
fn ordinary_source_executes_force_transport_effects() {
    let program = compile_source_fixture("scenario_force_transport_effects.bhs");
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "force_transport_effects_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x813d, 8);
    let rows = configure_force_transport_effect_state(&mut sim);

    execute_force_transport_effect(&mut sim, &mut scripts, rows);
}

#[test]
fn retail_chunk_executes_the_same_force_transport_effects() {
    let compiled = compile_source_fixture("scenario_force_transport_effects.bhs");
    let program = loaded_scalar_program(compiled);
    assert!(program.walk_meta().is_some());
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "force_transport_effects_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x813e, 8);
    let rows = configure_force_transport_effect_state(&mut sim);

    execute_force_transport_effect(&mut sim, &mut scripts, rows);
}

#[test]
fn force_transport_fails_closed_before_missing_type_facts_write() {
    let mut scripts = game_runtime(one_builtin_program(
        "force_transport_ability",
        &[Value::Int(1)],
    ));
    let mut sim = Sim::new(0x813f, 8);
    sim.activate(0);
    sim.vic_leaders.slots[0].leader_flags |= 0x4000;
    sim.vic_leaders.slots[0].leader_flags2 = 0x8000;
    let unit = sim.spawn_unit(0, 7, 2 * 192, 3 * 192, 4).unwrap();
    let row = sim.world.row_of(unit).unwrap();
    sim.world.units.set_unit_masks(row, 0x100);

    let error = sim.do_frame_with_scripts(&mut scripts).unwrap_err();
    assert!(matches!(
        error.failure,
        ScriptFailure::Vm(VmError::UnimplementedBuiltin {
            name: "force_transport_ability",
            ..
        })
    ));
    assert_eq!(sim.vic_leaders.slots[0].leader_flags, 0x4003);
    assert_eq!(sim.vic_leaders.slots[0].leader_flags2, 0x8000);
    assert_eq!(sim.world.units.get_unit_masks(row), 0x100);
}

fn expected_victory_option_reads(victory: victory_score::Victory, time_limit: i32) -> [i32; 6] {
    use victory_score::Victory;

    let packed_other_modes = match victory {
        Victory::Economic => 1,
        Victory::MusicalChairs => 2,
        Victory::Score => 4,
        Victory::SuddenDeath => 8,
        Victory::TechRace => 16,
        Victory::Population => 32,
        _ => 0,
    };
    [
        if victory == Victory::TimeLimit {
            time_limit + 1
        } else {
            0
        },
        (victory == Victory::Standard) as i32,
        (victory == Victory::Conquest) as i32,
        (victory == Victory::TimeLimit) as i32,
        (victory == Victory::Wonder) as i32,
        packed_other_modes,
    ]
}

const SCRIPT_VICTORY_MODES: [victory_score::Victory; 10] = [
    victory_score::Victory::Standard,
    victory_score::Victory::Conquest,
    victory_score::Victory::Economic,
    victory_score::Victory::MusicalChairs,
    victory_score::Victory::Score,
    victory_score::Victory::SuddenDeath,
    victory_score::Victory::TechRace,
    victory_score::Victory::Population,
    victory_score::Victory::TimeLimit,
    victory_score::Victory::Wonder,
];

#[test]
fn ordinary_source_executes_all_victory_option_readers() {
    let program = compile_source_fixture("scenario_victory_option_reads.bhs");
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "victory_option_reads_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x8133, 8);
    sim.activate(0);
    sim.vic_match.options.time_limit = 3;

    for victory in SCRIPT_VICTORY_MODES {
        sim.vic_match.options.victory = victory as u8;
        let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
        assert_eq!(trace.steps[4], StepRun::Executed);
        assert!(trace.work[4] > 0);
        assert_eq!(
            sim.leaders[0].econ.stockpile,
            expected_victory_option_reads(victory, 60),
            "ordinary source must track the live {victory:?} selector"
        );
    }
}

#[test]
fn retail_chunk_executes_all_victory_option_readers() {
    let compiled = compile_source_fixture("scenario_victory_option_reads.bhs");
    let program = loaded_scalar_program(compiled);
    assert!(program.walk_meta().is_some());
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "victory_option_reads_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x8134, 12);
    sim.activate(0);
    sim.vic_match.options.time_limit = 7;

    for victory in SCRIPT_VICTORY_MODES {
        sim.vic_match.options.victory = victory as u8;
        let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
        assert_eq!(trace.steps[4], StepRun::Executed);
        assert!(trace.work[4] > 0);
        assert_eq!(
            sim.leaders[0].econ.stockpile,
            expected_victory_option_reads(victory, 240),
            "loaded chunk must track the live {victory:?} selector"
        );
    }
}

#[test]
fn custom_time_limit_fails_closed_without_scenario_override_state() {
    let mut scripts = game_runtime(one_builtin_program("get_time_limit", &[]));
    let mut sim = Sim::new(0x8135, 8);
    sim.activate(0);
    sim.vic_match.options.victory = victory_score::Victory::TimeLimit as u8;
    sim.vic_match.options.time_limit = 8;

    let error = sim.do_frame_with_scripts(&mut scripts).unwrap_err();
    assert!(matches!(
        error.failure,
        ScriptFailure::Vm(VmError::UnimplementedBuiltin {
            name: "get_time_limit",
            ..
        })
    ));
}

fn configure_player_read_state(sim: &mut Sim) {
    sim.activate(0);
    sim.activate(1);
    sim.step8.leaders[0].pop_cap = 275;
    sim.vic_leaders.slots[0].score = 1_234;
    sim.vic_leaders.slots[0].num_units[0] = 7;
    sim.vic_leaders.slots[0].num_units[175] = 11;
    sim.vic_leaders.slots[0].num_units[351] = 13;
    sim.step8.leaders[0].diplo[1] = victory_score::Diplo::Ally as i32;
    sim.step8.leaders[1].diplo[0] = victory_score::Diplo::War as i32;
}

#[test]
fn ordinary_source_executes_authoritative_player_state_reads() {
    let program = compile_source_fixture("scenario_player_reads.bhs");
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "player_reads_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x8123, 8);
    configure_player_read_state(&mut sim);

    let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(trace.steps[4], StepRun::Executed);
    assert!(trace.work[4] > 0);
    assert_eq!(
        sim.leaders[0].econ.stockpile,
        [275, 1_234, 31, 1, 0, 0],
        "population cap, score, all 352 counters, directed alliance, and invalid-player sentinel must execute"
    );
}

#[test]
fn retail_chunk_executes_the_same_player_state_reads() {
    let compiled = compile_source_fixture("scenario_player_reads.bhs");
    let program = loaded_scalar_program(compiled);
    assert!(program.walk_meta().is_some());
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "player_reads_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x8124, 12);
    configure_player_read_state(&mut sim);

    let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(trace.steps[4], StepRun::Executed);
    assert!(trace.work[4] > 0);
    assert_eq!(
        sim.leaders[0].econ.stockpile,
        [275, 1_234, 31, 1, 0, 0],
        "loaded chunks must read the same authoritative leader stores"
    );
}

fn configure_map_resource_diplomacy_state(sim: &mut Sim) {
    sim.activate(0);
    sim.activate(1);
    let rock = sim.map.world.wdata_mut(2, 2);
    rock.flags = wflag::ROCKS;
    rock.land = land::FERTILE;
    sim.leaders[0].econ.displayed[2] = -31;
    sim.step8.leaders[0].diplo[1] = victory_score::Diplo::Peace as i32;
    sim.step8.leaders[1].diplo[0] = victory_score::Diplo::War as i32;
}

#[test]
fn ordinary_source_executes_map_resource_and_diplomacy_queries() {
    let program = compile_source_fixture("scenario_map_resource_diplomacy.bhs");
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "map_resource_diplomacy_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x8125, 8);
    configure_map_resource_diplomacy_state(&mut sim);

    let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(trace.steps[4], StepRun::Executed);
    assert!(trace.work[4] > 0);
    assert_eq!(
        sim.leaders[0].econ.stockpile,
        [1, 0, 9, 13, 1, 1],
        "WCoord masks, signed gather division, take/clamp/add, and directed diplomacy must execute"
    );
}

#[test]
fn retail_chunk_executes_the_same_map_resource_and_diplomacy_queries() {
    let compiled = compile_source_fixture("scenario_map_resource_diplomacy.bhs");
    let program = loaded_scalar_program(compiled);
    assert!(program.walk_meta().is_some());
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "map_resource_diplomacy_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x8126, 12);
    configure_map_resource_diplomacy_state(&mut sim);

    let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(trace.steps[4], StepRun::Executed);
    assert!(trace.work[4] > 0);
    assert_eq!(
        sim.leaders[0].econ.stockpile,
        [1, 0, 9, 13, 1, 1],
        "loaded chunks must retain the same live map, economy, and diplomacy reads"
    );
}

fn configure_live_player_map_read_state(sim: &mut Sim) {
    sim.activate(0);
    sim.activate(1);
    sim.activate(2);
    sim.step8.leaders[1].flags = leaders::flag::IN_GAME;
    sim.step8.leaders[2].flags = leaders::flag::IN_GAME;
    sim.production_runtime.leaders[0].control = 37;
    sim.production_runtime.leaders[1].last_unit_built = 123;
    sim.map.world.start_x.items.extend([3, 4]);
    sim.map.world.start_y.items.extend([5, 6]);
    sim.leaders[0].gather_ctx.extra_income[2] = -31;
}

#[test]
fn ordinary_source_executes_live_player_and_map_readbacks() {
    let program = compile_source_fixture("scenario_live_player_map_reads.bhs");
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "live_player_map_reads_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x8127, 8);
    configure_live_player_map_read_state(&mut sim);

    let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(trace.steps[4], StepRun::Executed);
    assert!(trace.work[4] > 0);
    assert_eq!(
        sim.leaders[0].econ.stockpile,
        [37, 16, 24, 9, 123, 0],
        "control, WCoord shifts, base rate, last unit, one-bit gates, and array sentinel must execute"
    );
}

#[test]
fn retail_chunk_executes_the_same_live_player_and_map_readbacks() {
    let compiled = compile_source_fixture("scenario_live_player_map_reads.bhs");
    let program = loaded_scalar_program(compiled);
    assert!(program.walk_meta().is_some());
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "live_player_map_reads_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x8128, 12);
    configure_live_player_map_read_state(&mut sim);

    let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(trace.steps[4], StepRun::Executed);
    assert!(trace.work[4] > 0);
    assert_eq!(
        sim.leaders[0].econ.stockpile,
        [37, 16, 24, 9, 123, 0],
        "loaded chunks must retain the exact player, map, production, and economy reads"
    );
}

fn configure_territory_building_read_state(sim: &mut Sim) {
    sim.activate(0);
    sim.activate(1);
    sim.step8.leaders[1].flags = leaders::flag::IN_GAME;
    sim.map.world.wdata_mut(2, 2).who = 1;
    sim.map.world.land_size = 1_000;
    sim.vic_leaders.slots[0].territory = 123;
    sim.vic_leaders.slots[0].num_buildings[0] = 7;
    sim.vic_leaders.slots[0].num_buildings[64] = 11;
    sim.vic_leaders.slots[0].num_buildings[128] = 13;
}

#[test]
fn ordinary_source_executes_territory_and_building_reads() {
    let program = compile_source_fixture("scenario_territory_building_reads.bhs");
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "territory_building_reads_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x8129, 8);
    configure_territory_building_read_state(&mut sim);

    let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(trace.steps[4], StepRun::Executed);
    assert!(trace.work[4] > 0);
    assert_eq!(
        sim.leaders[0].econ.stockpile,
        [2, 0, 12, 0, 31, 0],
        "WData owner, bounds sentinel, land percentage, all 129 counters, and active gate must execute"
    );
}

#[test]
fn retail_chunk_executes_the_same_territory_and_building_reads() {
    let compiled = compile_source_fixture("scenario_territory_building_reads.bhs");
    let program = loaded_scalar_program(compiled);
    assert!(program.walk_meta().is_some());
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "territory_building_reads_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x812a, 12);
    configure_territory_building_read_state(&mut sim);

    let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(trace.steps[4], StepRun::Executed);
    assert!(trace.work[4] > 0);
    assert_eq!(
        sim.leaders[0].econ.stockpile,
        [2, 0, 12, 0, 31, 0],
        "loaded chunks must retain the same authoritative territory and building reads"
    );
}

fn configure_unit_status_read_state(sim: &mut Sim) {
    sim.activate(0);

    let idle_captain = sim.spawn_unit(0, 7, 2 * 192, 3 * 192, 4).unwrap();
    let idle_subordinate = sim.spawn_unit(0, 7, 4 * 192, 5 * 192, 4).unwrap();
    let garrisoned = sim.spawn_unit(0, 7, 6 * 192, 7 * 192, 4).unwrap();
    let moving = sim.spawn_unit(0, 7, 8 * 192, 9 * 192, 4).unwrap();
    let idle_captain_row = sim.world.row_of(idle_captain).unwrap();
    let idle_subordinate_row = sim.world.row_of(idle_subordinate).unwrap();
    let garrisoned_row = sim.world.row_of(garrisoned).unwrap();
    let moving_row = sim.world.row_of(moving).unwrap();

    sim.world.units.o_up_mut()[idle_captain_row] = -1;
    sim.world.units.o_up_mut()[idle_subordinate_row] = 0;
    sim.world.units.o_up_mut()[garrisoned_row] = -1;
    sim.world.units.o_up_mut()[moving_row] = -1;
    sim.world.units.inside_up_mut()[garrisoned_row] = 2000;
    sim.world.units.inside_up_who_mut()[garrisoned_row] = 0;
    assert!(sim
        .world
        .issue(moving, Order::move_to(12 * 192, 13 * 192, 0)));

    let mut build = don_sim::systems::production::BuildData::default();
    build.flags = don_sim::systems::production::flag::VALID;
    build.other[don_sim::systems::production::off::OBJECT_ID
        ..don_sim::systems::production::off::OBJECT_ID + 2]
        .copy_from_slice(&2000i16.to_le_bytes());
    sim.spawn_build(0, build);
}

#[test]
fn ordinary_source_executes_unit_status_reads() {
    let program = compile_source_fixture("scenario_unit_status_reads.bhs");
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "unit_status_reads_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x812b, 8);
    configure_unit_status_read_state(&mut sim);

    let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(trace.steps[4], StepRun::Executed);
    assert!(trace.work[4] > 0);
    assert_eq!(
        sim.leaders[0].econ.stockpile,
        [1, 0, 0, 1, 1, 0],
        "captain identity, current-order classification, and outer building containment must execute"
    );
}

#[test]
fn retail_chunk_executes_the_same_unit_status_reads() {
    let compiled = compile_source_fixture("scenario_unit_status_reads.bhs");
    let program = loaded_scalar_program(compiled);
    assert!(program.walk_meta().is_some());
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "unit_status_reads_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x812c, 12);
    configure_unit_status_read_state(&mut sim);

    let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(trace.steps[4], StepRun::Executed);
    assert!(trace.work[4] > 0);
    assert_eq!(
        sim.leaders[0].econ.stockpile,
        [1, 0, 0, 1, 1, 0],
        "loaded chunks must retain exact idle, move-order, and garrison reads"
    );
}

fn configure_object_health_read_state(sim: &mut Sim) {
    use don_sim::systems::{ammo, production};

    sim.activate(0);

    // A two-object formation. `object_health(1, 1)` must redirect the addressed
    // subordinate to captain 0 and aggregate both objects' damage. In contrast,
    // `object_max_health(1, 1)` reads the subordinate's own maximum directly.
    let captain = sim.spawn_unit(0, 7, 2 * 192, 3 * 192, 4).unwrap();
    let subordinate = sim.spawn_unit(0, 7, 4 * 192, 5 * 192, 4).unwrap();
    let captain_row = sim.world.row_of(captain).unwrap();
    let subordinate_row = sim.world.row_of(subordinate).unwrap();
    sim.world.units.o_up_mut()[captain_row] = -1;
    sim.world.units.o_down_mut()[captain_row] = 1;
    sim.world.units.o_up_mut()[subordinate_row] = 0;
    sim.world.units.o_down_mut()[subordinate_row] = -1;
    sim.world.units.myhits_mut()[captain_row] = 200;
    sim.world.units.myhits_mut()[subordinate_row] = 120;
    sim.world.units.damage_mut()[captain_row] = 10;
    sim.world.units.damage_mut()[subordinate_row] = 20;
    sim.shooter_rules.push((
        7,
        ammo::ShooterRules {
            uber_size: 2,
            ..Default::default()
        },
    ));

    // A normal construction site exposes construct_hits, not its eventual myhits.
    let mut construction = production::BuildData::default();
    construction.flags = production::flag::VALID | production::flag::STARTED;
    construction.myhits = 1_000;
    construction.construct_hits = 250;
    construction.damage = 50;
    assert_eq!(sim.spawn_build(0, construction), 0);

    // An active razing site runs the existing f32 queue-progress interpolation before
    // both max-health and hits-left are observed.
    let mut razing = production::BuildData::default();
    razing.flags = production::flag::VALID | production::flag::STARTED | production::flag::ACTIVE;
    razing.myhits = 1_000;
    razing.construct_hits = 800;
    razing.damage = 100;
    razing.queue.queued = 1;
    razing.queue.entries.push(production::BuildQueueEntry {
        elapsed: 500,
        type_index: 0x29a,
        ..Default::default()
    });
    assert_eq!(sim.spawn_build(0, razing), 1);
    sim.production_runtime
        .install_type(production::runtime::LiveProductionType::research(
            0x29a, 1_000,
        ));
}

#[test]
fn ordinary_source_executes_captain_and_construction_health_reads() {
    let program = compile_source_fixture("scenario_object_health.bhs");
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "object_health_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x812d, 8);
    configure_object_health_read_state(&mut sim);

    let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(trace.steps[4], StepRun::Executed);
    assert!(trace.work[4] > 0);
    assert_eq!(
        sim.leaders[0].econ.stockpile,
        [85, 120, 80, 250, 75, 400],
        "health percent must use captain damage and each building's dynamic construction maximum"
    );
}

#[test]
fn retail_chunk_executes_the_same_object_health_reads() {
    let compiled = compile_source_fixture("scenario_object_health.bhs");
    let program = loaded_scalar_program(compiled);
    assert!(program.walk_meta().is_some());
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "object_health_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x812e, 12);
    configure_object_health_read_state(&mut sim);

    let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(trace.steps[4], StepRun::Executed);
    assert!(trace.work[4] > 0);
    assert_eq!(
        sim.leaders[0].econ.stockpile,
        [85, 120, 80, 250, 75, 400],
        "loaded chunks must retain exact captain, construction, and razing health semantics"
    );
}

#[test]
fn object_health_fails_closed_without_unit_type_uber_size() {
    let mut scripts = game_runtime(one_builtin_program(
        "object_health",
        &[Value::Int(1), Value::Int(0)],
    ));
    let mut sim = Sim::new(0x812f, 8);
    sim.activate(0);
    let unit = sim.spawn_unit(0, 7, 2 * 192, 3 * 192, 4).unwrap();
    let row = sim.world.row_of(unit).unwrap();
    sim.world.units.o_up_mut()[row] = -1;
    sim.world.units.o_down_mut()[row] = -1;

    let error = sim.do_frame_with_scripts(&mut scripts).unwrap_err();
    assert!(matches!(
        error.failure,
        ScriptFailure::Vm(VmError::UnimplementedBuiltin {
            name: "object_health",
            ..
        })
    ));
}

fn configure_object_proximity_state(sim: &mut Sim) {
    sim.activate(0);

    // Query subordinate 1. Every addressed-object location handler must redirect it
    // to captain 0, while retaining the captain's own point rather than an outer
    // containment point.
    let captain = sim.spawn_unit(0, 7, 10 * 192, 20 * 192, 4).unwrap();
    let subordinate = sim.spawn_unit(0, 7, 90 * 192, 90 * 192, 4).unwrap();
    let captain_row = sim.world.row_of(captain).unwrap();
    let subordinate_row = sim.world.row_of(subordinate).unwrap();
    sim.world.units.o_up_mut()[captain_row] = -1;
    sim.world.units.inside_up_mut()[captain_row] = -1;
    sim.world.units.inside_up_who_mut()[captain_row] = -1;
    sim.world.units.o_up_mut()[subordinate_row] = 0;
    sim.world.units.inside_up_mut()[subordinate_row] = -1;
    sim.world.units.inside_up_who_mut()[subordinate_row] = -1;

    // The fixture also probes a 3-by-4 tile delta from the captain, whose retail
    // approximate distance is 5. Radius 5 must therefore be false and radius 6 true;
    // this catches an accidental <= comparison or Euclidean replacement.
}

#[test]
fn ordinary_source_executes_addressed_object_proximity_reads() {
    let program = compile_source_fixture("scenario_object_proximity.bhs");
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "object_proximity_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x8130, 8);
    configure_object_proximity_state(&mut sim);

    let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(trace.steps[4], StepRun::Executed);
    assert!(trace.work[4] > 0);
    assert_eq!(
        sim.leaders[0].econ.stockpile,
        [1, 0, 1, 0, 1, -1],
        "captain redirection, approximate distance, strict radius, and invalid radius must execute"
    );
}

#[test]
fn retail_chunk_executes_the_same_addressed_object_proximity_reads() {
    let compiled = compile_source_fixture("scenario_object_proximity.bhs");
    let program = loaded_scalar_program(compiled);
    assert!(program.walk_meta().is_some());
    let mut scripts = ScriptRuntime::new(
        program,
        Some(ScriptBinding::new(0, "object_proximity_tick")),
        None,
    )
    .unwrap();
    let mut sim = Sim::new(0x8131, 12);
    configure_object_proximity_state(&mut sim);

    let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(trace.steps[4], StepRun::Executed);
    assert!(trace.work[4] > 0);
    assert_eq!(
        sim.leaders[0].econ.stockpile,
        [1, 0, 1, 0, 1, -1],
        "loaded chunks must retain exact addressed-object proximity semantics"
    );
}

#[test]
fn object_near_fails_closed_outside_the_recovered_coordinate_table() {
    let mut scripts = game_runtime(one_builtin_program(
        "object_near",
        &[
            Value::Int(1),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(1),
        ],
    ));
    let mut sim = Sim::new(0x8132, 8);
    sim.activate(0);
    let unit = sim.spawn_unit(0, 7, -192, 3 * 192, 4).unwrap();
    let row = sim.world.row_of(unit).unwrap();
    sim.world.units.o_up_mut()[row] = -1;

    let error = sim.do_frame_with_scripts(&mut scripts).unwrap_err();
    assert!(matches!(
        error.failure,
        ScriptFailure::Vm(VmError::UnimplementedBuiltin {
            name: "object_near",
            ..
        })
    ));
}

#[test]
fn unsupported_scenario_builtin_stops_before_the_rest_of_the_tick() {
    let mut scripts = game_runtime(one_builtin_program("num_cities", &[Value::Int(1)]));
    let mut sim = Sim::new(99, 8);
    sim.activate(0);

    let error = sim.do_frame_with_scripts(&mut scripts).unwrap_err();
    assert_eq!(error.slot, ScriptSlot::Game);
    assert!(error.bytecodes_executed > 0);
    assert!(matches!(
        error.failure,
        ScriptFailure::Vm(VmError::UnimplementedBuiltin {
            name: "num_cities",
            ..
        })
    ));
    assert_eq!(sim.world.frame, 0, "step 20 must not run after BHS failure");
    assert_eq!(sim.cover.ticks, 0, "a failed partial tick is not coverage");
    assert_eq!(sim.cover.leader_gathers, 0, "step 8 must not run");
}

#[test]
fn tick_bindings_reject_parameterized_entries_before_execution() {
    let mut parameterized = void_script("needs_arg", 0);
    parameterized.arity = 1;
    parameterized.params = vec![ScriptTy::Int.tag()];
    let program = Program::single(ScriptFile {
        code: asm(&[(0x47, &[0]), (0x3e, &[])]),
        scripts: vec![parameterized],
        ..Default::default()
    });
    let error = match ScriptRuntime::new(program, Some(ScriptBinding::new(0, "needs_arg")), None) {
        Ok(_) => panic!("parameterized step-4 binding was accepted"),
        Err(error) => error,
    };
    assert_eq!(
        error,
        ScriptBindError::NonzeroArity {
            file: 0,
            name: "needs_arg".to_string(),
            arity: 1,
        }
    );
}
