// SPDX-License-Identifier: GPL-3.0-or-later
//! Running — not merely loading — the shipped `scenario/scriptlibrary/general_powers.bhs`.
//!
//! The load-path lane made this file compile through retail's own resolution rules and
//! said plainly that loading is not running: `ScenarioFuncSet` is 842 builtins deep and
//! this script needs five of them. `crates/don-bhs/src/scenario.rs` implements those five
//! (plus `stop_timer`, `num_objects_selected` and the twelve rules gates), so the script
//! can now be driven frame by frame against a `ScenarioHost`.
//!
//! What makes these assertions falsifiable is that **the script is the fixture**. Nobody
//! transcribed an expected trace: the expectations are `set_timer("pop", 5)` /
//! `set_timer("pop", 2)` / `find_unit(1, "Alexander")` as they appear in the shipped file,
//! and the *times* at which they fire come out of `ScriptTimers::check` `0x00a04b80`. A
//! regression in the timer container, the `who - 1` convention, the search cursor or the
//! selection guards moves the trace.
//!
//! `ron-data/` is gitignored game content. When it is absent these tests print a loud
//! SKIPPED line and pass; a skipped case is not evidence of anything.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use don_bhs::builtin_table::builtin;
use don_bhs::host::{Host, HostError, HostResult};
use don_bhs::scenario::{
    leader_flag, GameImage, ObjectImage, ObjectProbe, ScenarioHost, ScenarioWorldImage,
};
use don_bhs::value::Value;
use don_bhs::vm::Vm;
use don_bhs_cc::load;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn corpus_root() -> Option<PathBuf> {
    let root = repo().join("ron-data/bhs-corpus");
    root.is_dir().then_some(root)
}

/// A `ScenarioHost` that also records every builtin call, so the trace is observable.
struct TracingHost {
    inner: ScenarioHost,
    trace: Vec<(u32, &'static str, Vec<Value>)>,
}

impl TracingHost {
    fn calls(&self, name: &str) -> Vec<&(u32, &'static str, Vec<Value>)> {
        self.trace.iter().filter(|c| c.1 == name).collect()
    }
    fn names(&self) -> Vec<&'static str> {
        self.trace.iter().map(|c| c.1).collect()
    }
}

impl Host for TracingHost {
    fn call(&mut self, decl: &don_bhs::BuiltinDecl, args: &[Value]) -> HostResult {
        self.trace.push((decl.index, decl.name, args.to_vec()));
        self.inner.call(decl, args)
    }
    fn game_random(&mut self, lo: i32, hi: i32) -> Result<i32, HostError> {
        self.inner.game_random(lo, hi)
    }
    fn game_random_step(&mut self) -> Result<u32, HostError> {
        self.inner.game_random_step()
    }
    fn game_random_seed(&self) -> Result<u32, HostError> {
        self.inner.game_random_seed()
    }
    fn set_game_random_seed(&mut self, s: u32) -> Result<(), HostError> {
        self.inner.set_game_random_seed(s)
    }
    fn script_print(&mut self, s: &str, nl: bool) -> Result<(), HostError> {
        self.inner.script_print(s, nl)
    }
    fn game_seconds(&mut self) -> Result<i32, HostError> {
        self.inner.game_seconds()
    }
    fn game_info_flags(&mut self) -> Result<u32, HostError> {
        self.inner.game_info_flags()
    }
    fn game_info_rush_rules(&mut self) -> Result<u8, HostError> {
        self.inner.game_info_rush_rules()
    }
    fn game_info_victory(&mut self) -> Result<u8, HostError> {
        self.inner.game_info_victory()
    }
    fn game_semaphore_bit(&mut self, b: u32) -> Result<bool, HostError> {
        self.inner.game_semaphore_bit(b)
    }
    fn script_timers(&mut self) -> Result<&mut don_bhs::ScriptTimers, HostError> {
        self.inner.script_timers()
    }
    fn find_unit_cursor(&mut self) -> Result<i32, HostError> {
        self.inner.find_unit_cursor()
    }
    fn set_find_unit_cursor(&mut self, v: i32) -> Result<(), HostError> {
        self.inner.set_find_unit_cursor(v)
    }
    fn leader_flags(&mut self, who0: i32) -> Result<u32, HostError> {
        self.inner.leader_flags(who0)
    }
    fn type_index_by_name(&mut self, n: &str) -> Result<i32, HostError> {
        self.inner.type_index_by_name(n)
    }
    fn type_is_unit_type(&mut self, t: i32) -> Result<bool, HostError> {
        self.inner.type_is_unit_type(t)
    }
    fn unit_band_count(&mut self, w: i32) -> Result<i32, HostError> {
        self.inner.unit_band_count(w)
    }
    fn unit_band_probe(&mut self, w: i32, i: i32) -> Result<ObjectProbe, HostError> {
        self.inner.unit_band_probe(w, i)
    }
    fn unit_band_is_type(&mut self, w: i32, i: i32, t: i32) -> Result<bool, HostError> {
        self.inner.unit_band_is_type(w, i, t)
    }
    fn object_band_probe(&mut self, w: i32, o: i32) -> Result<ObjectProbe, HostError> {
        self.inner.object_band_probe(w, o)
    }
    fn object_band_is_type(&mut self, w: i32, o: i32, t: i32) -> Result<bool, HostError> {
        self.inner.object_band_is_type(w, o, t)
    }
    fn object_band_position_internal(&mut self, w: i32, o: i32) -> Result<(i32, i32), HostError> {
        self.inner.object_band_position_internal(w, o)
    }
    fn selection_group_owner(&mut self) -> Result<i32, HostError> {
        self.inner.selection_group_owner()
    }
    fn selection_group_members(&mut self) -> Result<Vec<i32>, HostError> {
        self.inner.selection_group_members()
    }
    fn local_display_player(&mut self) -> Result<i32, HostError> {
        self.inner.local_display_player()
    }
    fn add_bubble_message(&mut self, t: &str, x: i32, y: i32, w: i32) -> Result<(), HostError> {
        self.inner.add_bubble_message(t, x, y, w)
    }
}

/// The world `general_powers.bhs` is written for: player 1 has an Alexander selected.
///
/// Handles and band indices are arbitrary; the *shapes* are the retail ones — a
/// `Units::lists` band scanned by index, an `Objects::lists` band addressed by handle, a
/// select group with an owner byte and a `short[]` of handles.
fn world_with_alexander_selected() -> ScenarioWorldImage {
    let unit = ObjectImage {
        probe: ObjectProbe {
            alive: true,
            is_captain: true,
            is_valid_unit: true,
            is_on_map: true,
            is_active: true,
            is_build: false,
        },
        type_index: 3,
        // Stored internal coordinates: already XORed, so bubble_text_obj must un-XOR.
        x_internal: 4800 ^ 0x0006_3637,
        y_internal: 9600 ^ 0x0006_3637,
    };
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
        type_names: vec![
            "Citizen".into(),
            "Barracks".into(),
            "Napoleon".into(),
            "Alexander".into(),
        ],
        unit_type_indices: BTreeSet::from([2, 3]),
        selection_owner: 0,
        selection: vec![11],
        local_display_player: 0,
        ..ScenarioWorldImage::default()
    };
    // Slot 11 of owner 0. `general_powers.bhs` hands `find_unit`'s band index straight to
    // `bubble_text_obj`, so the two accessors must resolve the same slot.
    w.put_object(0, 11, unit);
    w
}

fn host_for(world: Option<ScenarioWorldImage>) -> TracingHost {
    TracingHost {
        inner: ScenarioHost {
            world,
            game: GameImage::default(),
            ..ScenarioHost::new()
        },
        trace: Vec::new(),
    }
}

/// The shipped script's whole builtin surface, executed frame by frame.
///
/// Catches: any regression that stops the script reaching its last statement — a timer
/// that never comes due, a timer that comes due every frame, a `who` convention that
/// refuses player 1, a search cursor that never finds the unit, an object handle that
/// fails `valid_object_o`.
#[test]
fn the_shipped_general_powers_script_runs_to_its_bubble() {
    let Some(root) = corpus_root() else {
        eprintln!("SKIPPED: ron-data/bhs-corpus is absent (gitignored game content)");
        return;
    };
    let inc = load::install_include_path(&root);
    let loaded = match load::load_script(&inc, load::GENERAL_POWERS_SCRIPT_FILE) {
        Ok(l) => l,
        Err(e) => panic!("shipped general_powers.bhs did not load: {e}"),
    };
    let entry = loaded
        .tick_entry_index()
        .expect("general_powers has a zero-argument entry named after its file stem");
    let entry_name = loaded.program.files[0].scripts[entry].name.clone();

    let mut prog = loaded.program;
    let mut host = host_for(Some(world_with_alexander_selected()));

    // Retail drives this from `Game::do_frame` step 4 once per frame with `Game::tick`
    // advancing. Sixteen seconds is enough for the 5s arming timer and several 2s
    // re-arms.
    for tick in 0..16i32 {
        host.inner.game.tick = tick;
        let mut vm = Vm::new(&mut prog, &mut host);
        let out = vm
            .run_script(0, &entry_name)
            .unwrap_or_else(|e| panic!("frame {tick}: {e:?}"));
        assert!(
            vm.coverage.is_complete(),
            "frame {tick} needed a builtin nobody implements:\n{}",
            vm.coverage.report()
        );
        let _ = out;
    }

    // `run_once { set_timer("pop", 5); }` fires on the first frame only; the re-arm inside
    // `if (timer_expired("pop"))` uses 2. Both literals are read off the shipped file.
    let sets = host.calls("set_timer");
    assert_eq!(
        sets[0].2,
        vec![Value::str("pop"), Value::Int(5)],
        "the run_once arm is set_timer(\"pop\", 5)"
    );
    assert!(
        sets.len() > 1,
        "the timer never came due, so the script never re-armed"
    );
    for s in &sets[1..] {
        assert_eq!(s.2, vec![Value::str("pop"), Value::Int(2)]);
    }

    // The arming call at t=0 expires at 0+5=5; each re-arm expires two seconds later, and
    // `ScriptTimers::check` is `now >= expiry`. So over ticks 0..=15 the timer comes due
    // at t = 5, 7, 9, 11, 13, 15 — six re-arms after the one `run_once` arm.
    assert_eq!(
        sets.len(),
        7,
        "expected the 5s arm plus a re-arm at each of t=5,7,9,11,13,15; \
         got {} set_timer calls",
        sets.len()
    );

    // The script only reaches `find_unit` when a matching type is selected, and only
    // reaches `bubble_text_obj` when `length(bubble) > 0`.
    let finds = host.calls("find_unit");
    assert!(
        !finds.is_empty(),
        "object_type_selected never matched, so find_unit was never reached: {:?}",
        host.names()
    );
    assert_eq!(finds[0].2, vec![Value::Int(1), Value::str("Alexander")]);
    assert!(
        !host.inner.bubbles.is_empty(),
        "the script never reached bubble_text_obj"
    );

    // `bubble_text_obj` un-XORs `SubObjectData::x_internal`/`y_internal` with 0x00063637
    // before handing them to `MessageWin`; the fixture stored them XORed.
    let (text, x, y, display) = host.inner.bubbles[0].clone();
    assert_eq!((x, y), (4800, 9600));
    assert_eq!(display, 0, "Console+0x298, the local display player");
    assert!(!text.is_empty());
}

/// Every builtin the shipped script needs is one this tree implements.
///
/// Catches: a cohort that "covers general_powers" by name while the script actually calls
/// something else. The needed set is measured by running against a host that implements
/// nothing and reading `Coverage`.
#[test]
fn the_shipped_script_needs_exactly_the_implemented_cohort() {
    let Some(root) = corpus_root() else {
        eprintln!("SKIPPED: ron-data/bhs-corpus is absent (gitignored game content)");
        return;
    };
    let inc = load::install_include_path(&root);
    let loaded = load::load_script(&inc, load::GENERAL_POWERS_SCRIPT_FILE).expect("load");
    let entry_name = loaded.entry.clone();
    let mut prog = loaded.program;

    // Survey mode substitutes `ScriptFuncSet::get_err_return` so one run discovers more
    // than its first missing builtin. This is the crate's explicitly lossy path and is
    // used here to *measure the debt*, never to claim a successful run.
    let mut host = don_bhs::NullHost;
    let mut vm = Vm::new(&mut prog, &mut host)
        .with_missing_builtin_policy(don_bhs::MissingBuiltinPolicy::Survey);
    vm.run_script(0, &entry_name).expect("survey run");
    let needed: BTreeSet<u32> = vm
        .coverage
        .unimplemented()
        .map(|(i, _, _)| i)
        .chain(vm.coverage.implemented().map(|(i, _)| i))
        .collect();

    let covered: BTreeSet<u32> = don_bhs::builtins::implemented_indices()
        .into_iter()
        .chain(don_bhs::scenario::implemented_indices())
        .collect();
    let missing: Vec<&'static str> = needed
        .difference(&covered)
        .map(|i| builtin(*i).unwrap().name)
        .collect();
    assert!(
        missing.is_empty(),
        "general_powers.bhs still needs unimplemented builtins: {missing:?}"
    );
    // The first frame does not take the timer branch, so this is the arming subset.
    assert!(
        needed.contains(&298) && needed.contains(&77) && needed.contains(&79),
        "expected time_sec/set_timer/timer_expired on the arming frame, got {needed:?}"
    );
}
