// SPDX-License-Identifier: GPL-3.0-or-later
//! The `ScenarioFuncSet` cohort: `ScriptTimers`, the rules gates, and the four builtins
//! that read the object bands.
//!
//! Expectations here come from three places that are independent of the code under test:
//!
//! * `schema/bhs-builtins.json` (via the generated `builtin_table`) — the index, name,
//!   arity, return type and **handler VA** of every entry. A dispatch arm wired to the
//!   wrong index fails on the VA, not on a comment.
//! * `schema/types.json` — the PDB's `VictoryIndex` enumerators, which are compared
//!   against the immediates the ten `is_victory_*` handlers actually `cmp` with.
//! * The refusal trichotomy each handler emits (`-1` / `0` / `1`), which is the part of
//!   these bodies a caller can observe and the part a "plausible" implementation gets
//!   wrong.

use std::collections::{BTreeMap, BTreeSet};

use don_bhs::builtin_table::builtin;
use don_bhs::host::{Host, HostError};
use don_bhs::scenario::{
    self, find_unit_mode, leader_flag, object_band, semaphore_bit, string_eq, victory, GameImage,
    ObjectImage, ObjectProbe, ScenarioHost, ScenarioWorldImage, ScriptTimers,
};
use don_bhs::value::Value;
use don_bhs::NullHost;

fn call(h: &mut ScenarioHost, index: u32, args: &[Value]) -> Result<Value, HostError> {
    let d = builtin(index).unwrap_or_else(|| panic!("no builtin {index}"));
    h.call(d, args)
}

fn int(r: Result<Value, HostError>) -> i32 {
    match r {
        Ok(Value::Int(i)) => i,
        other => panic!("expected Int, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// The cohort is wired to the indices it claims
// ---------------------------------------------------------------------------

/// Every implemented index carries the handler VA this lane read.
///
/// Catches an off-by-one in the dispatch `match` — the failure mode that produces a
/// builtin which runs and answers the wrong question. The right-hand side of each row was
/// read off `ron-bin/riseofnations.exe`; the left-hand side comes from the generated table.
#[test]
fn every_implemented_index_is_the_handler_that_was_read() {
    let expected: &[(u32, &str, u32, u8)] = &[
        (77, "set_timer", 0x009e_4bc0, 2),
        (78, "stop_timer", 0x009e_4c10, 1),
        (79, "timer_expired", 0x009e_4c80, 1),
        (94, "get_is_no_nation_powers", 0x009e_5230, 0),
        (95, "get_rush_rules", 0x009e_5240, 0),
        (96, "is_victory_standard", 0x009e_5250, 0),
        (97, "is_victory_conquest", 0x009e_5260, 0),
        (98, "is_victory_economic", 0x009e_5270, 0),
        (99, "is_victory_musical_chairs", 0x009e_5280, 0),
        (100, "is_victory_score", 0x009e_5290, 0),
        (101, "is_victory_sudden_death", 0x009e_52a0, 0),
        (102, "is_victory_tech_race", 0x009e_52b0, 0),
        (103, "is_victory_territory", 0x009e_52c0, 0),
        (104, "is_victory_time_limit", 0x009e_52d0, 0),
        (105, "is_victory_wonder", 0x009e_52e0, 0),
        (147, "is_conquest_scenario", 0x009e_6040, 0),
        (298, "time_sec", 0x009e_ad40, 0),
        (311, "find_unit", 0x009e_be10, 2),
        (390, "object_type_selected", 0x009f_04e0, 2),
        (392, "num_objects_selected", 0x009f_06f0, 1),
        (783, "bubble_text_obj", 0x009f_f550, 3),
    ];
    let claimed: BTreeSet<u32> = scenario::implemented_indices();
    let listed: BTreeSet<u32> = expected.iter().map(|r| r.0).collect();
    assert_eq!(
        claimed, listed,
        "implemented_indices() and this table disagree"
    );

    for (index, name, va, arity) in expected {
        let d = builtin(*index).unwrap();
        assert_eq!(d.name, *name, "index {index}");
        assert_eq!(d.handler_va, *va, "index {index} ({name}) handler VA");
        assert_eq!(d.arity, *arity, "index {index} ({name}) arity");
    }
}

/// The cohort does not overlap the utility function sets.
#[test]
fn the_scenario_cohort_is_disjoint_from_the_utility_sets() {
    let util = don_bhs::builtins::implemented_indices();
    let scen = scenario::implemented_indices();
    assert!(util.intersection(&scen).next().is_none());
    assert!(
        scen.iter().all(|i| *i >= scenario::SCENARIO_MIN_INDEX),
        "a ScenarioFuncSet index below 31 would be a utility registration"
    );
}

/// Nothing in the cohort answers on a host that models nothing.
///
/// This is the poison rule: an unmodelled builtin must stop the VM, not return a value the
/// engine happens to also return on a rejected call.
#[test]
fn a_host_that_models_nothing_poisons_every_entry() {
    let mut h = NullHost;
    for i in scenario::implemented_indices() {
        let d = builtin(i).unwrap();
        let args: Vec<Value> = (0..d.arity)
            .map(|_| match d.params.first() {
                Some(don_bhs::ScriptTy::Str) => Value::str("x"),
                _ => Value::Int(1),
            })
            .collect();
        assert_eq!(
            h.call(d, &args),
            Err(HostError::Unimplemented),
            "{} ({i}) answered on a host with no world",
            d.name
        );
    }
}

// ---------------------------------------------------------------------------
// GameInfo scalars
// ---------------------------------------------------------------------------

/// The ten `is_victory_*` immediates agree with the PDB's `VictoryIndex`.
///
/// Two independent artefacts: the constants were read off the `cmp` in each handler, the
/// enumerators out of `schema/types.json`. The one row that does *not* agree by name is
/// asserted as a mismatch on purpose — `is_victory_territory` compares against 7, which
/// the PDB calls `VICTORY_POPULATION`, and there is no `VICTORY_TERRITORY` at all.
#[test]
fn the_victory_constants_are_the_pdb_victory_index() {
    // `don-bhs` has no dependencies on purpose, so this reads the one enumerator list it
    // needs out of the schema text rather than pulling in a JSON crate. The shape is
    // `"VictoryIndex":{...,"values":[["NAME",0],...]}`.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/types.json");
    let text = std::fs::read_to_string(&path).expect("schema/types.json");
    let at = text
        .find("\"VictoryIndex\":")
        .expect("VictoryIndex is in schema/types.json");
    let vals = text[at..]
        .find("\"values\":[")
        .map(|o| at + o + "\"values\":[".len())
        .expect("VictoryIndex has a values array");
    let end = vals + text[vals..].find("]]").expect("values array closes") + 1;
    let mut by_name: BTreeMap<String, i64> = BTreeMap::new();
    for entry in text[vals..end].split("],[") {
        let e = entry.trim_matches(|c| c == '[' || c == ']');
        let (name, num) = e.split_once(',').expect("[\"NAME\",N]");
        by_name
            .entry(name.trim_matches('"').to_string())
            .or_insert(num.trim().parse::<i64>().expect("enumerator value"));
    }
    assert!(
        by_name.len() >= 10,
        "only parsed {} VictoryIndex enumerators",
        by_name.len()
    );
    let named: &[(&str, u8)] = &[
        ("VICTORY_STANDARD", victory::STANDARD),
        ("VICTORY_SUDDEN_DEATH", victory::SUDDEN_DEATH),
        ("VICTORY_CONQUEST", victory::CONQUEST),
        ("VICTORY_SCORE", victory::SCORE),
        ("VICTORY_TIME_LIMIT", victory::TIME_LIMIT),
        ("VICTORY_MUSICAL_CHAIRS", victory::MUSICAL_CHAIRS),
        ("VICTORY_WONDER", victory::WONDER),
        ("VICTORY_ECONOMIC", victory::ECONOMIC),
        ("VICTORY_TECH_RACE", victory::TECH_RACE),
    ];
    for (n, c) in named {
        assert_eq!(
            by_name.get(*n).copied(),
            Some(*c as i64),
            "{n} disagrees with the constant the handler compares"
        );
    }
    assert!(
        !by_name.contains_key("VICTORY_TERRITORY"),
        "if the PDB grew a VICTORY_TERRITORY, is_victory_territory should be rechecked"
    );
    assert_eq!(
        by_name.get("VICTORY_POPULATION").copied(),
        Some(victory::TERRITORY as i64),
        "is_victory_territory 0x009e52c0 compares against VICTORY_POPULATION"
    );
}

/// Each zero-argument gate answers 1 for exactly its own mode.
#[test]
fn exactly_one_victory_gate_answers_yes_at_a_time() {
    let gates: &[(u32, u8)] = &[
        (96, victory::STANDARD),
        (97, victory::CONQUEST),
        (98, victory::ECONOMIC),
        (99, victory::MUSICAL_CHAIRS),
        (100, victory::SCORE),
        (101, victory::SUDDEN_DEATH),
        (102, victory::TECH_RACE),
        (103, victory::TERRITORY),
        (104, victory::TIME_LIMIT),
        (105, victory::WONDER),
    ];
    for (_, mode) in gates {
        let mut h = ScenarioHost::new();
        h.game.victory = *mode;
        let yes: Vec<u32> = gates
            .iter()
            .filter(|(i, _)| int(call(&mut h, *i, &[])) == 1)
            .map(|(i, _)| *i)
            .collect();
        assert_eq!(yes.len(), 1, "victory mode {mode} lit {yes:?}");
    }
    // `VICTORY_SCENARIO` (10) has no gate, so all ten answer no.
    let mut h = ScenarioHost::new();
    h.game.victory = 10;
    assert!(gates.iter().all(|(i, _)| int(call(&mut h, *i, &[])) == 0));
}

/// `get_is_no_nation_powers` is bit 2 of the low byte of `GameInfo::flags`, and
/// `is_conquest_scenario` is `Game::semaphore` bit 17 — not a `GameInfo` byte.
#[test]
fn the_flag_gates_read_the_bit_the_handler_reads() {
    let mut h = ScenarioHost::new();
    assert_eq!(int(call(&mut h, 94, &[])), 0);
    h.game.info_flags = 0xffff_fffb; // every bit but 2
    assert_eq!(int(call(&mut h, 94, &[])), 0);
    h.game.info_flags = 0b100;
    assert_eq!(int(call(&mut h, 94, &[])), 1);

    h.game.rush_rules = 14; // RUSH_30_MINUTES
    assert_eq!(int(call(&mut h, 95, &[])), 14);

    assert_eq!(int(call(&mut h, 147, &[])), 0);
    h.game
        .set_semaphore_bit(semaphore_bit::CONQUEST_SCENARIO, true);
    assert_eq!(int(call(&mut h, 147, &[])), 1);
    // bit 17 is byte 2 bit 1, i.e. `Game+0x822 & 2`.
    assert_eq!(h.game.semaphore[2], 0b10);

    h.game.tick = 4321;
    assert_eq!(int(call(&mut h, 298, &[])), 4321);
}

// ---------------------------------------------------------------------------
// ScriptTimers
// ---------------------------------------------------------------------------

#[test]
fn set_timer_refuses_a_nonpositive_duration_and_an_empty_name() {
    let mut h = ScenarioHost::new();
    h.game.tick = 100;
    assert_eq!(int(call(&mut h, 77, &[Value::str("a"), Value::Int(0)])), -1);
    assert_eq!(
        int(call(&mut h, 77, &[Value::str("a"), Value::Int(-1)])),
        -1
    );
    assert_eq!(int(call(&mut h, 77, &[Value::str(""), Value::Int(5)])), -1);
    assert!(h.timers.is_empty(), "a refused set_timer must not insert");
    assert_eq!(int(call(&mut h, 77, &[Value::str("a"), Value::Int(5)])), 1);
    assert_eq!(h.timers.len(), 1);
    assert_eq!(h.timers.iter().next().unwrap().expires_at, 105);
}

/// `stop_timer` has no empty-name guard, unlike `set_timer`.
#[test]
fn stop_timer_will_look_for_an_empty_name() {
    let mut h = ScenarioHost::new();
    assert_eq!(int(call(&mut h, 78, &[Value::str("")])), -1);
    // A timer really can be named "" — only `set_timer` refuses to create one.
    h.timers.add_timer("", 10).unwrap();
    assert_eq!(int(call(&mut h, 78, &[Value::str("")])), 1);
    assert!(h.timers.is_empty());
}

/// The `-1` / `0` / `1` trichotomy, and that a due timer is consumed.
#[test]
fn timer_expired_is_absent_pending_or_due_and_due_consumes() {
    let mut h = ScenarioHost::new();
    h.game.tick = 0;
    assert_eq!(
        int(call(&mut h, 79, &[Value::str("pop")])),
        -1,
        "no such timer is -1, not 0"
    );
    assert_eq!(
        int(call(&mut h, 77, &[Value::str("pop"), Value::Int(5)])),
        1
    );
    for t in 0..5 {
        h.game.tick = t;
        assert_eq!(int(call(&mut h, 79, &[Value::str("pop")])), 0, "t={t}");
    }
    h.game.tick = 5;
    assert_eq!(
        int(call(&mut h, 79, &[Value::str("pop")])),
        1,
        "the comparison is now >= expiry"
    );
    assert!(h.timers.is_empty(), "a due timer is removed by check");
    h.game.tick = 9;
    assert_eq!(int(call(&mut h, 79, &[Value::str("pop")])), -1);
}

/// `add_timer` seeks and removes before inserting, so a name maps to one expiry.
#[test]
fn re_arming_the_same_name_replaces_rather_than_duplicates() {
    let mut t = ScriptTimers::default();
    t.add_timer("pop", 50).unwrap();
    t.add_timer("pop", 20).unwrap();
    assert_eq!(t.len(), 1);
    assert_eq!(t.iter().next().unwrap().expires_at, 20);
    // Case-insensitively, because `String::operator==` forwards to `_wcsicmp`.
    t.add_timer("POP", 30).unwrap();
    assert_eq!(t.len(), 1);
    assert_eq!(t.iter().next().unwrap().expires_at, 30);
    assert_eq!(t.iter().next().unwrap().name, "POP");
}

/// `ordered_insert` keeps the list ascending by expiry, newest first among equals.
#[test]
fn the_timer_list_is_ordered_by_expiry_with_the_newest_first_among_equals() {
    let mut t = ScriptTimers::default();
    for (n, e) in [("c", 30), ("a", 10), ("b", 20), ("d", 20)] {
        t.add_timer(n, e).unwrap();
    }
    let order: Vec<(&str, i32)> = t.iter().map(|x| (x.name.as_str(), x.expires_at)).collect();
    assert_eq!(
        order,
        vec![("a", 10), ("d", 20), ("b", 20), ("c", 30)],
        "d was inserted after b at the same expiry and must precede it"
    );
}

/// A hundred live timers is the cap, and the hundred-and-first is refused with -1.
#[test]
fn the_timer_list_caps_at_a_hundred() {
    let mut h = ScenarioHost::new();
    for i in 0..ScriptTimers::MAX_TIMERS {
        assert_eq!(
            int(call(
                &mut h,
                77,
                &[Value::str(format!("t{i}")), Value::Int(1 + i as i32)]
            )),
            1
        );
    }
    assert_eq!(h.timers.len(), ScriptTimers::MAX_TIMERS);
    assert_eq!(
        int(call(&mut h, 77, &[Value::str("overflow"), Value::Int(1)])),
        -1
    );
    assert_eq!(h.timers.overflowed, 1);
    // Re-arming an existing name is still refused: the cap is checked before the seek.
    assert_eq!(
        int(call(&mut h, 77, &[Value::str("t0"), Value::Int(9)])),
        -1
    );
    assert_eq!(h.timers.len(), ScriptTimers::MAX_TIMERS);
}

/// `set_timer`'s `Game::tick + seconds` is an `add` on two ints: it wraps.
#[test]
fn a_timer_set_past_the_end_of_the_tick_counter_wraps_rather_than_saturating() {
    let mut h = ScenarioHost::new();
    h.game.tick = i32::MAX - 1;
    assert_eq!(int(call(&mut h, 77, &[Value::str("t"), Value::Int(5)])), 1);
    assert_eq!(
        h.timers.iter().next().unwrap().expires_at,
        i32::MIN + 3,
        "add eax, ecx wraps"
    );
}

// ---------------------------------------------------------------------------
// String equality
// ---------------------------------------------------------------------------

#[test]
fn string_equality_is_case_insensitive_and_length_first() {
    assert_eq!(string_eq("Alexander", "alexander"), Ok(true));
    assert_eq!(string_eq("pop", "POP"), Ok(true));
    assert_eq!(string_eq("pop", "pops"), Ok(false));
    assert_eq!(string_eq("", ""), Ok(true));
    // Different length: settled by `curr_len` before any folding, so no poison.
    assert_eq!(string_eq("é", "ee"), Ok(false));
    // Same length, both non-ASCII, not identical: MSVCRT's fold is not established.
    assert_eq!(string_eq("é", "É"), Err(HostError::Unimplemented));
    // Identical is equal whatever the fold does.
    assert_eq!(string_eq("é", "é"), Ok(true));
}

/// A non-ASCII timer name that would need folding poisons rather than guessing.
#[test]
fn a_timer_name_needing_an_underived_fold_poisons() {
    let mut t = ScriptTimers::default();
    t.add_timer("é", 5).unwrap();
    assert_eq!(t.check("É", 9), Err(HostError::Unimplemented));
    assert_eq!(t.check("é", 9), Ok(1));
}

// ---------------------------------------------------------------------------
// The object-band builtins
// ---------------------------------------------------------------------------

fn probe_unit() -> ObjectProbe {
    ObjectProbe {
        alive: true,
        is_captain: true,
        is_valid_unit: true,
        is_on_map: true,
        is_active: true,
        is_build: false,
    }
}

fn world() -> ScenarioWorldImage {
    let mut w = ScenarioWorldImage {
        leader_flags: [
            leader_flag::VALID | leader_flag::PRESENT,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ],
        type_names: vec!["Citizen".into(), "Barracks".into(), "Alexander".into()],
        unit_type_indices: BTreeSet::from([0, 2]),
        selection_owner: 0,
        selection: vec![],
        local_display_player: 0,
        ..ScenarioWorldImage::default()
    };
    w.put_object(
        0,
        3,
        ObjectImage {
            probe: probe_unit(),
            type_index: 2,
            x_internal: 4800 ^ object_band::POSITION_XOR,
            y_internal: 9600 ^ object_band::POSITION_XOR,
        },
    );
    w
}

fn host() -> ScenarioHost {
    ScenarioHost {
        world: Some(world()),
        game: GameImage::default(),
        ..ScenarioHost::new()
    }
}

/// `who` is 1-based and the bound is unsigned, so 0 and 9 are both refusals.
#[test]
fn who_is_one_based_everywhere_in_the_cohort() {
    let mut h = host();
    for (index, tail) in [
        (311u32, vec![Value::str("Alexander")]),
        (390, vec![Value::str("Alexander")]),
        (392, vec![]),
    ] {
        for who in [0i32, 9, -1, i32::MIN] {
            let mut args = vec![Value::Int(who)];
            args.extend(tail.clone());
            assert_eq!(int(call(&mut h, index, &args)), -1, "{index} who={who}");
        }
    }
    let mut args = vec![Value::str("hi"), Value::Int(0), Value::Int(3)];
    assert_eq!(int(call(&mut h, 783, &args)), -1);
    args[1] = Value::Int(9);
    assert_eq!(int(call(&mut h, 783, &args)), -1);
}

/// `object_type_selected` needs `Leaders` bit 0; `num_objects_selected` needs bits 0 and 1.
///
/// The two handlers are four instructions apart and this asymmetry is easy to "tidy up".
#[test]
fn only_num_objects_selected_requires_the_present_leader_bit() {
    let mut h = host();
    h.world.as_mut().unwrap().leader_flags[0] = leader_flag::VALID;
    h.world.as_mut().unwrap().selection = vec![3];
    assert_eq!(
        int(call(&mut h, 390, &[Value::Int(1), Value::str("Alexander")])),
        1,
        "object_type_selected 0x009f055e tests bit 0 only"
    );
    assert_eq!(
        int(call(&mut h, 392, &[Value::Int(1)])),
        -1,
        "num_objects_selected 0x009f0711 also tests bit 1"
    );
    h.world.as_mut().unwrap().leader_flags[0] = leader_flag::VALID | leader_flag::PRESENT;
    assert_eq!(int(call(&mut h, 392, &[Value::Int(1)])), 1);
}

/// The selection semaphore refuses with `-1`; a foreign selection answers `0`.
#[test]
fn the_selection_builtins_distinguish_refusal_from_a_negative_answer() {
    let mut h = host();
    h.world.as_mut().unwrap().selection = vec![3];
    assert_eq!(
        int(call(&mut h, 390, &[Value::Int(1), Value::str("Alexander")])),
        1
    );
    h.game
        .set_semaphore_bit(semaphore_bit::SELECTION_BLOCKED, true);
    assert_eq!(
        int(call(&mut h, 390, &[Value::Int(1), Value::str("Alexander")])),
        -1
    );
    assert_eq!(int(call(&mut h, 392, &[Value::Int(1)])), -1);
    h.game
        .set_semaphore_bit(semaphore_bit::SELECTION_BLOCKED, false);

    // The group belongs to someone else: 0, not -1.
    h.world.as_mut().unwrap().selection_owner = 1;
    assert_eq!(
        int(call(&mut h, 390, &[Value::Int(1), Value::str("Alexander")])),
        0
    );
    assert_eq!(int(call(&mut h, 392, &[Value::Int(1)])), 0);
    // An unknown type name is a refusal, an empty one too.
    h.world.as_mut().unwrap().selection_owner = 0;
    assert_eq!(
        int(call(&mut h, 390, &[Value::Int(1), Value::str("Nope")])),
        -1
    );
    assert_eq!(int(call(&mut h, 390, &[Value::Int(1), Value::str("")])), -1);
}

/// A matching **building** answers with `is_active` and stops the scan either way.
#[test]
fn a_matching_building_answers_with_is_active_and_does_not_keep_looking() {
    let mut h = host();
    let w = h.world.as_mut().unwrap();
    let unfinished = ObjectImage {
        probe: ObjectProbe {
            is_build: true,
            is_active: false,
            ..probe_unit()
        },
        type_index: 1,
        ..Default::default()
    };
    let finished = ObjectImage {
        probe: ObjectProbe {
            is_build: true,
            is_active: true,
            ..probe_unit()
        },
        type_index: 1,
        ..Default::default()
    };
    w.put_object(0, 10, unfinished);
    w.put_object(0, 11, finished);
    w.selection = vec![10, 11];
    assert_eq!(
        int(call(&mut h, 390, &[Value::Int(1), Value::str("Barracks")])),
        0,
        "the unfinished Barracks comes first and ends the scan"
    );
    h.world.as_mut().unwrap().selection = vec![11, 10];
    assert_eq!(
        int(call(&mut h, 390, &[Value::Int(1), Value::str("Barracks")])),
        1
    );
}

/// `num_objects_selected` is `get_num_cap_const`, not the group's raw member count.
#[test]
fn num_objects_selected_counts_only_live_captains() {
    let mut h = host();
    let w = h.world.as_mut().unwrap();
    w.put_object(
        0,
        4,
        ObjectImage {
            probe: ObjectProbe {
                alive: false,
                ..probe_unit()
            },
            type_index: 2,
            ..Default::default()
        },
    );
    w.put_object(
        0,
        5,
        ObjectImage {
            probe: ObjectProbe {
                is_captain: false,
                ..probe_unit()
            },
            type_index: 2,
            ..Default::default()
        },
    );
    w.selection = vec![3, 4, 5];
    assert_eq!(int(call(&mut h, 392, &[Value::Int(1)])), 1);
}

/// The `find_unit` scan begins at `cursor + 1` and wraps, so repeated calls iterate.
#[test]
fn find_unit_iterates_through_the_band_from_its_static_cursor() {
    let mut h = host();
    let w = h.world.as_mut().unwrap();
    for handle in [1, 4, 7] {
        w.put_object(
            0,
            handle,
            ObjectImage {
                probe: probe_unit(),
                type_index: 2,
                ..Default::default()
            },
        );
    }
    // Band slots 0..=7; matching Alexanders at 1, 3, 4, 7.
    let seen: Vec<i32> = (0..6)
        .map(|_| int(call(&mut h, 311, &[Value::Int(1), Value::str("Alexander")])))
        .collect();
    assert_eq!(
        seen,
        vec![1, 3, 4, 7, 1, 3],
        "the scan starts at cursor+1 and wraps at the band count"
    );
    assert_eq!(h.find_cursor, 3, "the cursor is left on the last hit");
}

/// An empty `unit_type` is "any unit", not a refusal; an unknown one is `-1`.
#[test]
fn find_unit_treats_an_empty_type_as_a_wildcard() {
    let mut h = host();
    assert_eq!(
        int(call(&mut h, 311, &[Value::Int(1), Value::str("")])),
        3,
        "an empty unit_type encodes type_index -1"
    );
    assert_eq!(
        int(call(&mut h, 311, &[Value::Int(1), Value::str("Nope")])),
        -1
    );
    // A known type that is not a *unit* type is refused too.
    assert_eq!(
        int(call(&mut h, 311, &[Value::Int(1), Value::str("Barracks")])),
        -1,
        "TypeData::is_unit_type gates the type before the band is touched"
    );
}

/// Mode 1 is `is_valid_unit && is_on_map`; the other two modes are different predicates.
#[test]
fn the_find_unit_scan_modes_are_three_different_predicates() {
    let mut h = host();
    h.world.as_mut().unwrap().put_object(
        0,
        1,
        ObjectImage {
            probe: ObjectProbe {
                is_on_map: false,
                is_active: true,
                ..probe_unit()
            },
            type_index: 2,
            ..Default::default()
        },
    );
    // Handle 1 is off map, handle 3 is on map.
    let any = scenario::find_unit_scan(&mut h, 0, 2, 0, find_unit_mode::ANY).unwrap();
    let on_map = scenario::find_unit_scan(&mut h, 0, 2, 0, find_unit_mode::ON_MAP).unwrap();
    let off = scenario::find_unit_scan(&mut h, 0, 2, 0, find_unit_mode::OFF_MAP_ACTIVE).unwrap();
    assert_eq!((any, on_map, off), (1, 3, 1));
    // An unlisted mode rejects everything rather than defaulting to "accept".
    assert_eq!(scenario::find_unit_scan(&mut h, 0, 2, 0, 7).unwrap(), -1);
}

/// `valid_object_o` bounds the handle at 2999 and requires the liveness bit.
#[test]
fn bubble_text_obj_resolves_the_handle_before_it_draws_anything() {
    let mut h = host();
    let ok = vec![Value::str("hi"), Value::Int(1), Value::Int(3)];
    assert_eq!(int(call(&mut h, 783, &ok)), 1);
    assert_eq!(
        h.bubbles,
        vec![("hi".to_string(), 4800, 9600, 0)],
        "the stored internal position is un-XORed with 0x00063637 before MessageWin"
    );

    h.bubbles.clear();
    for bad in [
        object_band::MAX_OBJECT_O + 1,
        i32::MAX,
        -1,
        999, // in range, but no object lives there
    ] {
        assert_eq!(
            int(call(
                &mut h,
                783,
                &[Value::str("hi"), Value::Int(1), Value::Int(bad)]
            )),
            -1,
            "handle {bad}"
        );
    }
    assert!(h.bubbles.is_empty(), "a refused bubble must draw nothing");
}
