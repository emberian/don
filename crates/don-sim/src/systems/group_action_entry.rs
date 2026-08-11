//! The measured entry prefix of the movement/attack `Group::action_*` family, and the
//! Coord centring every one of them applies to an installed movement destination.
//!
//! Nine wire opcodes reach this family: `FormCommand` (3), `AttackCommand` (4),
//! `SiegeAttackCommand` (5), `SwarmAroundCommand` (6), `MoveToCommand` (7),
//! `MoveNearCommand` (8), `AttackGroundCommand` (9), `PatrolCommand` (10) and
//! `LaunchPatrolCommand` (11). Every one of their `Group::action_*` bodies opens with a
//! short, fixed sequence of gates before it touches a single order queue. The bridge
//! previously ran **none** of them for `attack`, `attack_ground`, `patrol` and
//! `launch_patrol`, so a command addressed at a wholly off-map selection, or carrying a
//! negative object id, still installed orders.
//!
//! Everything below is `[measured]` by capstone disassembly of
//! `ron-bin/riseofnations.exe`, named from `ron-bin/sbl/rise.pdb`. Nothing here has been
//! executed against retail: **Tier C**.
//!
//! # The gate alphabet
//!
//! | gate | retail |
//! |---|---|
//! | [`EntryGate::GroupOnMap`] | `GroupData::is_on_map` `0x0070C450`; a zero result returns |
//! | [`EntryGate::TargetNonNegative`] | `test eax, eax; js` on the addressed object argument |
//! | [`EntryGate::IgnoreOrdersPrune`] | the `ScenarioData::ignore_orders` `0x00CC02F8` prelude |
//! | [`EntryGate::NumPositive`] | `cmp [this + 0x0C], 0; jle` |
//! | [`EntryGate::ActionBegin`] | the vtable `+0x14` call; `Group::action_begin` `0x00714100` is `disband = 0` |
//! | [`EntryGate::NotBuildings`] | `cmp byte [this + 0x49], 0`; a building selection returns |
//! | [`EntryGate::ClampDestination`] | clamp x/y into `0 ..= dim * 0x300 - 1` |
//! | [`EntryGate::ClearForm`] | `mov [this + 0x10], -1` |
//!
//! # The nine programs, in emitted order
//!
//! | action | VA | entry sequence |
//! |---|---|---|
//! | `move_to` | `0x0070FBA0` | none of its own: 49 bytes that re-push its arguments with `tolerance = 0` and tail into `move_near` |
//! | `move_near` | `0x00704990` | `GroupOnMap` → `IgnoreOrdersPrune` → `NumPositive` → `ActionBegin` → `ClampDestination` |
//! | `attack` | `0x00712490` | `GroupOnMap` → `TargetNonNegative` → `IgnoreOrdersPrune` → `NumPositive` → `ActionBegin` |
//! | `form` | `0x00707220` | `GroupOnMap` → `ActionBegin` → `NotBuildings` → `NumPositive` |
//! | `siege_attack` | `0x00706FF0` | `NotBuildings` → `TargetNonNegative` → `IgnoreOrdersPrune` → `NumPositive` |
//! | `swarm_around` | `0x0070FBE0` | `GroupOnMap` → `IgnoreOrdersPrune` → `ActionBegin` |
//! | `patrol` | `0x007030C0` | `IgnoreOrdersPrune` → `ActionBegin` → `ClampDestination` → `NotBuildings` → `ClearForm` → `NumPositive` |
//! | `launch_patrol` | `0x00703580` | `IgnoreOrdersPrune` → `NumPositive` |
//! | `attack_ground` | `0x00704520` | `ActionBegin` → `IgnoreOrdersPrune` → `NumPositive` → `ClampDestination` |
//!
//! The order is load-bearing and is not the same everywhere. `attack_ground` calls
//! `action_begin` *before* the scenario prune; `patrol` runs the prune first and
//! `action_begin` second; `form` never runs the prune at all; `siege_attack` never calls
//! `action_begin`. `swarm_around` additionally tests bit 0 of the addressed object's byte
//! at `+8` between `GroupOnMap` and the prune — that read belongs to the object host and is
//! already carried by `unimplemented_group_command_plans`, so it is deliberately not a gate
//! here.
//!
//! Two of the eight gates need facts this bridge does not hold, and both are therefore
//! *declared* rather than guessed:
//!
//! * [`EntryGate::IgnoreOrdersPrune`] runs `Group::kill` over
//!   `ScenarioData::objects_ignoring_orders[who]` (`0x00ED6574` count / `0x00ED6580` list,
//!   stride `0x1C`) and its complete recovered planner is
//!   [`crate::systems::groups_guys::plan_ignore_order_kills`]. The whole prelude is gated on
//!   the scalar `ScenarioData::ignore_orders` at `0x00CC02F8`, which
//!   `ScenarioFuncSet::init` zeroes at `0x00A04084` and only a scenario trigger sets. When
//!   that scalar is clear the prune is a **measured no-op**, which is the state of every
//!   non-scenario match in the replay corpus. When it is set and the host has not committed
//!   the prune, the entry is [`EntryDecision::Unavailable`] and the bridge installs nothing.
//! * [`EntryGate::ClampDestination`] needs `World::x_size`/`y_size`
//!   (`[[0x00C06188]]` / `[[0x00C06188] + 4]`, in tiles; the Coord bound is `tiles * 0x300`).
//!   A host that has not connected map bounds reports `None` and the clamp is skipped
//!   rather than invented; the row stays partial and says so.
//!
//! # Where an installed movement destination actually lands
//!
//! `Group::action_move_near`'s member loop passes **UCoord cell indices** to its two order
//! constructors, not Coord:
//!
//! ```text
//! 00705F87  mov  ecx, [edx + edi*4 + 0x514]   ; Form::to_x[i]
//! 00705F76  mov  eax, [edx + edi*4 + 0x714]   ; Form::to_y[i]
//! 00705F9D  sar  eax, 4
//! 00705FA6  sar  ecx, 4
//! 00705F92  mov  edi, [0xCAE5FC]              ; the divide-by-three table
//! 00705FAA  push dword ptr [edi + eax*4]      ; div3[to_y >> 4]
//! 00705FB1  push dword ptr [edi + ecx*4]      ; div3[to_x >> 4]
//! 00705FC7  call 0x5E55C0                     ; Unit::add_move_facing_order
//! ```
//!
//! and the same pair reaches `Unit::add_group_move_order` `0x005E4710` at `0x00705F57`.
//! Both constructors then store `arg * 0x30 + 0x18` — 48 Coord per cell plus a 24-unit
//! centre — into the order's `x`/`y` **and** `dest_x`/`dest_y`
//! [measured, `005e55c0.c` and `005e4710.c`; `Unit::add_move_order` `0x00616ED0` reaches the
//! same callee after converting Coord to UCoord with the same table, so the round trip is
//! Coord → UCoord → Coord]. [`formation_order_destination`] is that round trip; the bridge
//! previously stored the bare cell index, which is 1/48 of the intended destination.
//! `order_dispatch::install_follow_move` already performed exactly this centring for
//! `Unit::do_follow`, so the two movement arms now agree on what `OrderRec::x` means.

use crate::systems::groups_guys::formation_order_coord;
use crate::systems::movement::UCELL;

/// One measured gate of a `Group::action_*` entry prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryGate {
    /// `GroupData::is_on_map` `0x0070C450`: `num != 0` and either a building selection or
    /// some member which is alive, a captain and on the map.
    GroupOnMap,
    /// The addressed object argument must be non-negative.
    TargetNonNegative,
    /// The `ScenarioData::ignore_orders` `0x00CC02F8` prelude.
    IgnoreOrdersPrune,
    /// `cmp dword ptr [this + 0x0C], 0; jle`.
    NumPositive,
    /// The vtable `+0x14` call. `Group::action_begin` `0x00714100` is `disband = 0`.
    ActionBegin,
    /// `cmp byte ptr [this + 0x49], 0`: a building selection returns.
    NotBuildings,
    /// Clamp the commanded destination into `0 ..= dim * 0x300 - 1` on each axis.
    ClampDestination,
    /// `mov dword ptr [this + 0x10], -1`.
    ClearForm,
}

/// The entry prefix of one `Group::action_*`.
#[derive(Clone, Copy, Debug)]
pub struct EntryProgram {
    /// The `<x>` of `Group::action_<x>`, matching `ActionDef::name`.
    pub action: &'static str,
    pub va: u32,
    /// Gates in emitted order.
    pub gates: &'static [EntryGate],
}

/// Coord units per map tile on one axis: `0x300`, i.e. 16 UCoord cells of [`UCELL`].
pub const COORD_PER_TILE: i32 = 0x300;

/// Every entry program in this family, keyed by `Group::action_*` name.
///
/// `move_to` is present with an empty gate list on purpose: its 49-byte body is a pure
/// forwarder and its gates are exactly `move_near`'s, run once, by `move_near`.
pub static ENTRY_PROGRAMS: [EntryProgram; 9] = [
    EntryProgram {
        action: "move_to",
        va: 0x0070_FBA0,
        gates: &[],
    },
    EntryProgram {
        action: "move_near",
        va: 0x0070_4990,
        gates: &[
            EntryGate::GroupOnMap,
            EntryGate::IgnoreOrdersPrune,
            EntryGate::NumPositive,
            EntryGate::ActionBegin,
            EntryGate::ClampDestination,
        ],
    },
    EntryProgram {
        action: "attack",
        va: 0x0071_2490,
        gates: &[
            EntryGate::GroupOnMap,
            EntryGate::TargetNonNegative,
            EntryGate::IgnoreOrdersPrune,
            EntryGate::NumPositive,
            EntryGate::ActionBegin,
        ],
    },
    EntryProgram {
        action: "form",
        va: 0x0070_7220,
        gates: &[
            EntryGate::GroupOnMap,
            EntryGate::ActionBegin,
            EntryGate::NotBuildings,
            EntryGate::NumPositive,
        ],
    },
    EntryProgram {
        action: "siege_attack",
        va: 0x0070_6FF0,
        gates: &[
            EntryGate::NotBuildings,
            EntryGate::TargetNonNegative,
            EntryGate::IgnoreOrdersPrune,
            EntryGate::NumPositive,
        ],
    },
    EntryProgram {
        action: "swarm_around",
        va: 0x0070_FBE0,
        gates: &[
            EntryGate::GroupOnMap,
            EntryGate::IgnoreOrdersPrune,
            EntryGate::ActionBegin,
        ],
    },
    EntryProgram {
        action: "patrol",
        va: 0x0070_30C0,
        gates: &[
            EntryGate::IgnoreOrdersPrune,
            EntryGate::ActionBegin,
            EntryGate::ClampDestination,
            EntryGate::NotBuildings,
            EntryGate::ClearForm,
            EntryGate::NumPositive,
        ],
    },
    EntryProgram {
        action: "launch_patrol",
        va: 0x0070_3580,
        gates: &[EntryGate::IgnoreOrdersPrune, EntryGate::NumPositive],
    },
    EntryProgram {
        action: "attack_ground",
        va: 0x0070_4520,
        gates: &[
            EntryGate::ActionBegin,
            EntryGate::IgnoreOrdersPrune,
            EntryGate::NumPositive,
            EntryGate::ClampDestination,
        ],
    },
];

/// The entry program for one `Group::action_*`, or `None` for an action outside this family.
pub fn entry_program(action: &str) -> Option<&'static EntryProgram> {
    ENTRY_PROGRAMS.iter().find(|p| p.action == action)
}

/// The program a dispatcher must actually run for `action`.
///
/// `Group::action_move_to` `0x0070FBA0` is 49 bytes of argument re-push plus a tail call, so
/// a `MoveToCommand` runs `move_near`'s gates — once, in `move_near`. A bridge which calls
/// its `action_move_near` port directly must therefore use this, not [`entry_program`].
pub fn dispatch_program(action: &str) -> Option<&'static EntryProgram> {
    entry_program(if action == "move_to" {
        "move_near"
    } else {
        action
    })
}

/// The receiver and host facts the gate alphabet reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryFacts {
    /// `GroupData::is_on_map` `0x0070C450`, already evaluated over the live member facts.
    pub on_map: bool,
    /// `GroupData::num` `+0x0C`.
    pub num: i32,
    /// `GroupData::buildings` `+0x49`.
    pub buildings: bool,
    /// The addressed object argument, for the two actions that test it.
    pub target: Option<i32>,
    /// The commanded destination, for the actions that clamp one.
    pub destination: Option<(i32, i32)>,
    /// `World::x_size` / `y_size` in tiles, or `None` when the host has not connected them.
    pub map_tiles: Option<(i32, i32)>,
    /// The scalar `ScenarioData::ignore_orders` at `0x00CC02F8`.
    pub ignore_orders: bool,
    /// Whether the host has already committed the recovered `plan_ignore_order_kills`
    /// prelude for this receiver, so the supplied group is post-prune.
    pub ignore_orders_prune_committed: bool,
}

impl EntryFacts {
    /// The ordinary non-scenario shape: `ignore_orders` clear, so the prelude is a measured
    /// no-op and needs no host commitment.
    pub fn new(on_map: bool, num: i32, buildings: bool) -> Self {
        Self {
            on_map,
            num,
            buildings,
            target: None,
            destination: None,
            map_tiles: None,
            ignore_orders: false,
            ignore_orders_prune_committed: false,
        }
    }

    pub fn with_target(mut self, target: i32) -> Self {
        self.target = Some(target);
        self
    }

    pub fn with_destination(mut self, x: i32, y: i32) -> Self {
        self.destination = Some((x, y));
        self
    }

    pub fn with_map_tiles(mut self, x_size: i32, y_size: i32) -> Self {
        self.map_tiles = Some((x_size, y_size));
        self
    }
}

/// The state writes an admitted entry prefix performs on the receiver, in gate order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EntryEffects {
    /// `Group::action_begin` ran: `GroupData::disband = 0`.
    pub action_begin: bool,
    /// `GroupData::form = -1`.
    pub clear_form: bool,
    /// The destination after [`EntryGate::ClampDestination`], when the action carries one.
    /// Equal to the input when the gate is absent or the host supplied no map bounds.
    pub destination: Option<(i32, i32)>,
    /// The clamp gate was reached but no map bounds were available, so it did not run.
    /// The caller may proceed; the row is not closure-complete while this can be true.
    pub clamp_unbounded: bool,
}

/// What the caller must do with the command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryDecision {
    /// A measured gate rejected the command. The action installs nothing, but any effect
    /// already applied by an *earlier* gate still stands — retail's `action_begin` runs
    /// before `form`'s building gate, so a building selection still gets `disband = 0`.
    Refuse(EntryEffects),
    /// The scenario prune is armed and the host has not committed it. Fail closed: the
    /// bridge must not mutate the receiver or any order queue.
    Unavailable,
    /// Run the action body with these effects applied.
    Proceed(EntryEffects),
}

/// Retail's per-axis clamp: `if (v < 0) v = 0; if (v >= dim * 0x300) v = dim * 0x300 - 1;`
///
/// Both comparisons are signed and the upper bound is exclusive, so the admitted range is
/// `0 ..= tiles * 0x300 - 1` [measured, `Group::action_move_near+0x19A..0x1DF`].
pub fn clamp_axis(v: i32, tiles: i32) -> i32 {
    let v = v.max(0);
    let limit = tiles.wrapping_mul(COORD_PER_TILE);
    if v >= limit {
        limit.wrapping_sub(1)
    } else {
        v
    }
}

/// Evaluate one entry program against the receiver facts.
pub fn evaluate(program: &EntryProgram, facts: EntryFacts) -> EntryDecision {
    let mut effects = EntryEffects {
        destination: facts.destination,
        ..EntryEffects::default()
    };
    for gate in program.gates {
        match gate {
            EntryGate::GroupOnMap => {
                if !facts.on_map {
                    return EntryDecision::Refuse(effects);
                }
            }
            EntryGate::TargetNonNegative => {
                // Retail reads the argument register unconditionally; an action whose
                // program lists this gate always has the argument on the wire.
                if facts.target.is_none_or(|target| target < 0) {
                    return EntryDecision::Refuse(effects);
                }
            }
            EntryGate::IgnoreOrdersPrune => {
                if facts.ignore_orders && !facts.ignore_orders_prune_committed {
                    return EntryDecision::Unavailable;
                }
            }
            EntryGate::NumPositive => {
                if facts.num <= 0 {
                    return EntryDecision::Refuse(effects);
                }
            }
            EntryGate::ActionBegin => effects.action_begin = true,
            EntryGate::NotBuildings => {
                if facts.buildings {
                    return EntryDecision::Refuse(effects);
                }
            }
            EntryGate::ClampDestination => match (facts.destination, facts.map_tiles) {
                (Some((x, y)), Some((x_tiles, y_tiles))) => {
                    effects.destination = Some((clamp_axis(x, x_tiles), clamp_axis(y, y_tiles)));
                }
                (Some(_), None) => effects.clamp_unbounded = true,
                (None, _) => {}
            },
            EntryGate::ClearForm => effects.clear_form = true,
        }
    }
    EntryDecision::Proceed(effects)
}

/// The Coord a movement order actually stores, from a `Form::to_x`/`to_y` coordinate.
///
/// `Group::action_move_near` hands `div3[to >> 4]` — a UCoord cell index, which is exactly
/// [`formation_order_coord`] — to `Unit::add_move_facing_order` `0x005E55C0` or
/// `Unit::add_group_move_order` `0x005E4710`, and both store `cell * 0x30 + 0x18` into the
/// order's `x`/`y` and `dest_x`/`dest_y`.
#[inline]
pub fn formation_order_destination(form_coord: i32) -> i32 {
    ucell_order_destination(formation_order_coord(form_coord))
}

/// The `cell * 0x30 + 0x18` half of [`formation_order_destination`], for callers that
/// already hold a UCoord cell index.
#[inline]
pub fn ucell_order_destination(cell: i32) -> i32 {
    cell.wrapping_mul(UCELL).wrapping_add(UCELL / 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_group_action_in_the_family_has_exactly_one_program() {
        for name in [
            "move_to",
            "move_near",
            "attack",
            "form",
            "siege_attack",
            "swarm_around",
            "patrol",
            "launch_patrol",
            "attack_ground",
        ] {
            let found: Vec<_> = ENTRY_PROGRAMS.iter().filter(|p| p.action == name).collect();
            assert_eq!(found.len(), 1, "{name}");
        }
    }

    #[test]
    fn programs_address_the_measured_retail_procedures() {
        // Cross-checked against `schema/rise-procs.tsv`; a drifting VA means the program
        // was copied from the wrong body.
        for (name, va) in [
            ("move_to", 0x0070_FBA0),
            ("move_near", 0x0070_4990),
            ("attack", 0x0071_2490),
            ("form", 0x0070_7220),
            ("siege_attack", 0x0070_6FF0),
            ("swarm_around", 0x0070_FBE0),
            ("patrol", 0x0070_30C0),
            ("launch_patrol", 0x0070_3580),
            ("attack_ground", 0x0070_4520),
        ] {
            assert_eq!(entry_program(name).unwrap().va, va, "{name}");
        }
    }

    #[test]
    fn action_begin_precedes_the_prune_only_for_attack_ground() {
        for program in &ENTRY_PROGRAMS {
            let begin = program
                .gates
                .iter()
                .position(|g| *g == EntryGate::ActionBegin);
            let prune = program
                .gates
                .iter()
                .position(|g| *g == EntryGate::IgnoreOrdersPrune);
            let Some((begin, prune)) = begin.zip(prune) else {
                continue;
            };
            assert_eq!(
                begin < prune,
                program.action == "attack_ground",
                "{}",
                program.action
            );
        }
    }

    #[test]
    fn form_never_runs_the_scenario_prune_and_siege_attack_never_begins() {
        let form = entry_program("form").unwrap();
        assert!(!form.gates.contains(&EntryGate::IgnoreOrdersPrune));
        let siege = entry_program("siege_attack").unwrap();
        assert!(!siege.gates.contains(&EntryGate::ActionBegin));
    }

    #[test]
    fn a_building_selection_still_receives_form_s_action_begin() {
        let decision = evaluate(
            entry_program("form").unwrap(),
            EntryFacts::new(true, 3, true),
        );
        let EntryDecision::Refuse(effects) = decision else {
            panic!("a building selection must not reach action_form's body");
        };
        assert!(effects.action_begin, "action_begin precedes the +0x49 test");
    }

    #[test]
    fn siege_attack_refuses_a_building_selection_before_reading_its_target() {
        let facts = EntryFacts::new(true, 3, true).with_target(-1);
        assert_eq!(
            evaluate(entry_program("siege_attack").unwrap(), facts),
            EntryDecision::Refuse(EntryEffects::default()),
        );
    }

    #[test]
    fn attack_refuses_a_negative_object_and_an_off_map_selection() {
        let program = entry_program("attack").unwrap();
        assert!(matches!(
            evaluate(program, EntryFacts::new(true, 2, false).with_target(-1)),
            EntryDecision::Refuse(_)
        ));
        assert!(matches!(
            evaluate(program, EntryFacts::new(false, 2, false).with_target(7)),
            EntryDecision::Refuse(_)
        ));
        assert!(matches!(
            evaluate(program, EntryFacts::new(true, 0, false).with_target(7)),
            EntryDecision::Refuse(_)
        ));
        let EntryDecision::Proceed(effects) =
            evaluate(program, EntryFacts::new(true, 2, false).with_target(7))
        else {
            panic!("an ordinary attack must proceed");
        };
        assert!(effects.action_begin);
    }

    #[test]
    fn an_armed_scenario_prune_without_a_committed_host_is_unavailable() {
        let mut facts = EntryFacts::new(true, 2, false).with_target(7);
        facts.ignore_orders = true;
        assert_eq!(
            evaluate(entry_program("attack").unwrap(), facts),
            EntryDecision::Unavailable
        );
        facts.ignore_orders_prune_committed = true;
        assert!(matches!(
            evaluate(entry_program("attack").unwrap(), facts),
            EntryDecision::Proceed(_)
        ));
    }

    #[test]
    fn the_destination_clamp_is_inclusive_of_the_last_coord_in_the_map() {
        let facts = EntryFacts::new(true, 1, false)
            .with_destination(-5, 1_000_000)
            .with_map_tiles(4, 4);
        let EntryDecision::Proceed(effects) = evaluate(entry_program("move_near").unwrap(), facts)
        else {
            panic!("an in-range move must proceed");
        };
        assert_eq!(effects.destination, Some((0, 4 * 0x300 - 1)));
        assert!(!effects.clamp_unbounded);
    }

    #[test]
    fn a_host_without_map_bounds_declares_the_clamp_instead_of_inventing_one() {
        let facts = EntryFacts::new(true, 1, false).with_destination(-5, 1_000_000);
        let EntryDecision::Proceed(effects) = evaluate(entry_program("move_near").unwrap(), facts)
        else {
            panic!("an unbounded host still proceeds");
        };
        assert_eq!(effects.destination, Some((-5, 1_000_000)));
        assert!(effects.clamp_unbounded);
    }

    #[test]
    fn patrol_clears_form_and_launch_patrol_only_counts_members() {
        let EntryDecision::Proceed(effects) = evaluate(
            entry_program("patrol").unwrap(),
            EntryFacts::new(true, 2, false).with_destination(10, 20),
        ) else {
            panic!("an ordinary patrol must proceed");
        };
        assert!(effects.clear_form);
        assert!(effects.action_begin);

        let launch = entry_program("launch_patrol").unwrap();
        assert_eq!(launch.gates.len(), 2);
        let EntryDecision::Proceed(effects) = evaluate(launch, EntryFacts::new(false, 2, true))
        else {
            panic!("launch_patrol runs no on-map or buildings gate");
        };
        assert!(!effects.action_begin);
        assert!(!effects.clear_form);
    }

    #[test]
    fn a_formation_destination_is_the_centre_of_its_ucoord_cell() {
        // 4_800 Coord is cell 100; retail stores 100 * 0x30 + 0x18.
        assert_eq!(formation_order_coord(4_800), 100);
        assert_eq!(formation_order_destination(4_800), 4_824);
        assert_eq!(ucell_order_destination(0), 24);
        // The table floors negatives, and the centring is a wrapping multiply.
        assert_eq!(formation_order_coord(-1), -1);
        assert_eq!(formation_order_destination(-1), -24);
    }
}
