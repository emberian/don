//! The selection split `Group::action_move_near` `0x00704990` runs before it installs
//! anything, and the `buildings` refusal that follows it.
//!
//! `Group::action_move_near` is the root of the whole movement/attack family:
//! `move_to` tail-calls it, and `form`, `attack` and `swarm_around` reach it through
//! `move_to`. It is 9,205 bytes — larger than `re/scripts/BulkDecomp.java`'s 8,192-byte
//! cap, which is why `re/decomp-all/` had no file for it and why nine closure rows sat
//! behind it. `docs/mechanics/group-action-move-near.md` maps the whole body; this module
//! implements the one self-contained transaction inside it.
//!
//! Everything here is `[measured]` from capstone disassembly of
//! `ron-bin/riseofnations.exe` plus a Ghidra decompilation of the same body, named from
//! `ron-bin/sbl/rise.pdb`. Nothing has been executed against retail: **Tier C**.
//!
//! # What the prelude is
//!
//! Between the entry gates (`is_on_map` → scenario prune → `num > 0` → `action_begin` →
//! destination clamp, all of which `group_action_entry` already runs) and the first order
//! queue, retail sorts the selection into **two stack-local `Group`s** and, if both come
//! out non-empty, abandons the receiver: it `Groups::push_group`es each half, re-issues the
//! *identical* `action_move_near` call to each, and then `Group::clear(-1)`s the receiver.
//!
//! ```text
//! 00704b90  <pass 1>  for i in 0..num:  o = list[i]
//! 00704bb5    if (!objects[who][o]->vf(+0x08)) continue            ; is_valid_unit
//! 00704bd6    if (units[who][o]->ptype->domain == 1)   B.add(o, who, 0, 0)
//! 00704bfa    else if ((c = get_inside(&cw)) >= 0)     B.add(c, cw, 0, 0)
//! 00704c1f    else                                     A.add(o, who, 0, 0)
//! 00704c49  if (B.num == 0) goto buildings-gate                    ; no split, no pass 2
//! 00704c56  if (A.num != 0 && this->army < 0) goto SPLIT
//! 00704d31  if ((terrain(dest) & 0x30) == 0x20) goto buildings-gate
//! 00704d5b  A.clear(-1); B.clear(-1)
//! 00704d90  <pass 2>  for i in 0..num:  o = list[i]
//! 00704db5    if (!objects[who][o]->vf(+0x08)) continue
//! 00704dd6    if (units[who][o]->ptype->unit_flags & 0x10)  A.add(o, who, 0, 0)
//! 00704dfa    else if ((c = get_inside(&cw)) >= 0)          A.add(c, cw, 0, 0)
//! 00704e1f    else                                          B.add(o, who, 0, 0)
//! 00704e3d  if (B.num != 0 && A.num != 0 && this->army < 0) goto SPLIT
//! 00704e59  if (this->buildings != 0) return                       ; the whole body ends
//!
//! SPLIT (00704c6d):
//!   s = Groups::push_group(who, &A, 1);  groups[s].facing = this->facing
//!   groups[s].action_move_near(<the eleven arguments, unchanged>)
//!   s = Groups::push_group(who, &B, 1);  groups[s].facing = this->facing
//!   groups[s].action_move_near(<the eleven arguments, unchanged>)
//!   this->Group::clear(-1); return
//! ```
//!
//! The buckets swap between the passes and that is not a transcription slip — pass 1 puts
//! sea-domain members and *containers* in `B`, pass 2 puts containers in `A`. `A` is
//! pushed first either way.
//!
//! # What each read is
//!
//! * `domain` is `ObjectTypeData::domain` `+0x218`; `1` is `Sea` (`systems::air`'s
//!   `DOMAIN_SEA`, already measured there). So pass 1 is the **land/sea split**: a mixed
//!   naval-and-land selection is never given one formation.
//! * `get_inside` is `ObjectData::get_inside` `0x00651A80`, which returns the containing
//!   object and writes its owner through the out parameter. So an embarked passenger is
//!   replaced by the transport carrying it — this is the "garrison into transport" arm.
//! * `unit_flags` is `UnitTypeData::unit_flags` `+0x2B4`; bit `0x10` is the type-level
//!   *can board a transport* bit that `PathFinder`'s `calc_cost` also consults
//!   (`systems::movement::PathUnit::can_board_transport`). **No shipped unit type sets
//!   it**: all 364 `UNIT` records in `ron-data/unitrules.xml` have it clear, so pass 2's
//!   first arm is measured-dead on shipped rules and pass 2 degenerates to
//!   "containers versus everyone else". It is implemented anyway because a mod can set it.
//! * the terrain word is the same expression `Group::compute_form` `0x00707C80` uses at
//!   `0x00707C97` for its water flag, so [`crate::command::Fleet::formation_water_destination`]
//!   answers it and no new host column is needed.
//! * `army` is `GroupData::army` `+0x08`. `Group::clear` writes `-1`, so a transient
//!   selection splits and a real army never does.
//!
//! # Fail-closed shape
//!
//! [`MoveNearSplit::Unanswered`] means *a host did not connect a column retail reads*, and
//! the caller must then run the pre-existing body unchanged. It is deliberately **not** a
//! refusal: refusing would silently stop installing orders for every host that has not yet
//! grown an `ObjectData::get_inside` column, which is a far worse lie than the stated gap.
//! The only true refusal here is `buildings != 0`, which needs no host facts at all.

use crate::systems::groups_guys::{GroupData, GROUP_MAX_MEMBERS};

/// `ObjectTypeData::domain` `+0x218` value for `Sea`. Mirrors
/// [`crate::systems::air::DOMAIN_SEA`]; repeated here so the split reads as the
/// disassembly does.
pub const DOMAIN_SEA: i32 = 1;

/// `UnitTypeData::unit_flags` `+0x2B4` bit `0x10` — the type-level "may board a
/// transport" bit. Clear on every shipped unit type.
pub const UNIT_FLAG_BOARDS_TRANSPORT: u32 = 0x10;

/// Everything the prelude and `Group::add` `0x00714350` read about one object.
///
/// The prelude asks these of members of the addressed selection *and* of the containers
/// `ObjectData::get_inside` names, which can belong to a different owner — so the lookup
/// is keyed by `(who, o)`, not by list position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitObject {
    /// Object virtual `+0x08` (`UnitData::is_valid_unit`), the prelude's `continue` gate.
    pub valid: bool,
    /// Object virtual `+0x18` (`ObjectData::is_unit`). `Group::add` only performs its
    /// captain substitution for a unit.
    pub is_unit: bool,
    /// Object virtual `+0x1C` (`ObjectData::is_build`) — the value `Group::add` writes
    /// into `GroupData::buildings`, and the homogeneity gate it compares against.
    pub is_build: bool,
    /// Object virtual `+0xE8` (`UnitData::is_captain`), i.e. `UnitData::o_up < 0`.
    pub is_captain: bool,
    /// `UnitData::o_up` `+0x8E`. `Group::add(o, who, 0, 0)` replaces a non-captain member
    /// with this object and returns.
    pub captain: i16,
    /// `UnitData::o_down` `+0x90`. `Group::add` re-adds it with `param_3 = 1`, which
    /// *kills* it from the group when it is itself a captain. Not modelled — a
    /// non-negative value makes the plan [`MoveNearSplit::Unanswered`].
    pub subordinate: i16,
    /// `UnitTypeData::role` `+0x2C8`, OR-ed into `GroupData::role` by `Group::add`.
    pub role: i32,
    /// `ObjectTypeData::domain` `+0x218`.
    pub domain: i32,
    /// `UnitTypeData::unit_flags` `+0x2B4`.
    pub unit_flags: u32,
    /// `ObjectData::get_inside(&who)` `0x00651A80`: the container and its owner, or
    /// `None` for retail's negative return.
    pub inside: Option<(i16, u8)>,
}

/// The receiver-side scalars the prelude reads, plus the destination it was clamped to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitFacts {
    /// `GroupData::who` `+0x4A`.
    pub who: u8,
    /// `GroupData::army` `+0x08`. A non-negative value forbids the split.
    pub army: i32,
    /// `GroupData::facing` `+0x48`, copied onto both pushed halves.
    pub facing: u8,
    /// `(terrain[dest] & 0x30) == 0x20` — pass 2 runs only when this is false.
    pub destination_is_water: bool,
    /// `Game::frame`, which `Group::add` stamps onto the temporaries.
    pub frame: i32,
}

/// The object column the split needs. One method, so a host can answer it from whatever
/// it already holds.
pub trait SplitWorld {
    /// `None` means "this host does not model this object's columns"; the split then
    /// yields [`MoveNearSplit::Unanswered`] and the caller runs its old body.
    fn object(&self, who: u8, o: i16) -> Option<SplitObject>;
}

/// The prelude's decision.
#[derive(Clone, Debug, PartialEq)]
pub enum MoveNearSplit {
    /// Retail reached the `buildings` gate with the receiver intact.
    Continue,
    /// Push `first`, re-issue, push `second`, re-issue, then `Group::clear(-1)` the
    /// receiver. `first` is the group the emitted code pushes first (`[ebp-0xa40]`).
    Split {
        first: Box<GroupData>,
        second: Box<GroupData>,
        /// Which pass produced the split. `1` is the land/sea pass, `2` the
        /// transport-versus-rest pass. Diagnostic only.
        pass: u8,
    },
    /// A column retail reads was not answered. Nothing about the command changes.
    Unanswered,
}

/// `Group::clear(-1)` `0x00713E80` — the scalar reset the two stack temporaries and the
/// abandoned receiver all take.
///
/// `clear` writes **scalars only**: the six parallel arrays keep whatever they held, which
/// is why retail's stack temporaries work at all. A negative argument leaves `id` alone.
pub fn group_clear(group: &mut GroupData, id: i32, frame: i32) {
    if id >= 0 {
        group.id = id;
    }
    group.who = 0;
    group.army = -1;
    group.num = 0;
    group.form = -1;
    group.stamp = frame;
    group.ox = 0;
    group.oy = 0;
    group.o_dist = 0;
    group.o_angle = 0;
    group.facing = 0;
    group.buildings = 0;
    group.disband = 0;
    group.order_num = 0;
    group.priority = 0;
    group.role = 0;
    group.think_frame = 0;
    group.new_speed = 0;
    group.speed = 0;
    group.form_num = 0;
    group.march = 0;
}

fn cleared_temporary(frame: i32) -> GroupData {
    let mut group = GroupData::default();
    group_clear(&mut group, -1, frame);
    // The two stack temporaries write `id = -1` explicitly, at `0x007049FE` and
    // `0x00704A30`, *after* `Group::clear(-1)` declined to.
    group.id = -1;
    group
}

/// `Group::add(o, who, 0, 0)` `0x00714350` as the prelude calls it.
///
/// ```text
/// if (Group::get_num() == 0) this->who = who;            ; param_4 == 0 arm
/// if (num != 0 && who != this->who) return;
/// if (objects[who][o]->is_unit() && !unit->is_captain())
///     return this->add(unit->o_up, who, 0, 0);           ; substitute the captain
/// this->disband = 0;
/// b = objects[who][o]->is_build();
/// if (num != 0 && buildings != b) return;
/// if (member(o) || num >= 0x80) return;
/// list[num] = o; off_x/off_y/curr_x/curr_y/angles[num] = 0; num++;
/// buildings = b; stamp = Game::frame; role |= type->role;
/// if (unit->o_down >= 0 && alive(o_down)) this->add(o_down, who, 1, 0);
/// if (id >= 0) compute_speed();
/// ```
///
/// `id` is `-1` on both temporaries, so `compute_speed` never runs here.
fn group_add(
    group: &mut GroupData,
    o: i16,
    who: u8,
    frame: i32,
    world: &dyn SplitWorld,
) -> Result<(), ()> {
    let mut o = o;
    // Retail has no bound on the `o_up` chain; a self- or cycle-referencing `o_up` on a
    // non-captain would hang the game. Refuse rather than reproduce a hang.
    for _ in 0..=GROUP_MAX_MEMBERS {
        if group.num == 0 {
            group.who = who;
        }
        if group.num != 0 && who != group.who {
            return Ok(());
        }
        let facts = world.object(who, o).ok_or(())?;
        if facts.is_unit && !facts.is_captain {
            if facts.captain < 0 || facts.captain == o {
                return Err(());
            }
            o = facts.captain;
            continue;
        }
        group.disband = 0;
        if group.num != 0 && (group.buildings != 0) != facts.is_build {
            return Ok(());
        }
        if group.member(o) || group.num >= GROUP_MAX_MEMBERS as i32 {
            return Ok(());
        }
        group.add(o, who, facts.is_build, facts.role, frame);
        if facts.subordinate >= 0 {
            // `Group::add`'s `o_down` re-add takes `param_3 = 1`, which turns the add into
            // a `Group::kill` for a captain. Neither arm is recovered; do not guess.
            return Err(());
        }
        return Ok(());
    }
    Err(())
}

/// Which temporary a member lands in, per pass.
#[derive(Clone, Copy)]
enum Bucket {
    First,
    Second,
}

fn bucket_pass(
    members: &[i16],
    facts: &SplitFacts,
    world: &dyn SplitWorld,
    // `(predicate bucket, container bucket, remainder bucket)`
    routing: (Bucket, Bucket, Bucket),
    predicate: &dyn Fn(&SplitObject) -> bool,
) -> Result<(GroupData, GroupData), ()> {
    let mut first = cleared_temporary(facts.frame);
    let mut second = cleared_temporary(facts.frame);
    for &o in members {
        let member = world.object(facts.who, o).ok_or(())?;
        if !member.valid {
            continue;
        }
        let (bucket, target, target_who) = if predicate(&member) {
            (routing.0, o, facts.who)
        } else if let Some((container, container_who)) = member.inside {
            (routing.1, container, container_who)
        } else {
            (routing.2, o, facts.who)
        };
        let group = match bucket {
            Bucket::First => &mut first,
            Bucket::Second => &mut second,
        };
        group_add(group, target, target_who, facts.frame, world)?;
    }
    Ok((first, second))
}

/// Run the prelude. `members` is `GroupData::list[..num]` of the addressed receiver.
pub fn plan_move_near_split(
    members: &[i16],
    facts: &SplitFacts,
    world: &dyn SplitWorld,
) -> MoveNearSplit {
    // Pass 1: sea domain and embarked passengers to `second`, everyone else to `first`.
    let Ok((first, second)) = bucket_pass(
        members,
        facts,
        world,
        (Bucket::Second, Bucket::Second, Bucket::First),
        &|member| member.domain == DOMAIN_SEA,
    ) else {
        return MoveNearSplit::Unanswered;
    };
    if second.num == 0 {
        // `0x00704C50`: an all-land, nobody-embarked selection skips pass 2 entirely.
        return MoveNearSplit::Continue;
    }
    if first.num != 0 && facts.army < 0 {
        return MoveNearSplit::Split {
            first: Box::new(first),
            second: Box::new(second),
            pass: 1,
        };
    }
    if facts.destination_is_water {
        return MoveNearSplit::Continue;
    }
    // Pass 2: transport-capable types and the containers to `first`, everyone else to
    // `second`. Both temporaries are re-cleared first (`0x00704D5B`/`0x00704D68`).
    let Ok((first, second)) = bucket_pass(
        members,
        facts,
        world,
        (Bucket::First, Bucket::First, Bucket::Second),
        &|member| member.unit_flags & UNIT_FLAG_BOARDS_TRANSPORT != 0,
    ) else {
        return MoveNearSplit::Unanswered;
    };
    if second.num != 0 && first.num != 0 && facts.army < 0 {
        return MoveNearSplit::Split {
            first: Box::new(first),
            second: Box::new(second),
            pass: 2,
        };
    }
    MoveNearSplit::Continue
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct World(BTreeMap<(u8, i16), SplitObject>);

    impl SplitWorld for World {
        fn object(&self, who: u8, o: i16) -> Option<SplitObject> {
            self.0.get(&(who, o)).copied()
        }
    }

    fn land(o: i16) -> SplitObject {
        SplitObject {
            valid: true,
            is_unit: true,
            is_build: false,
            is_captain: true,
            captain: o,
            subordinate: -1,
            role: 0,
            domain: 0,
            unit_flags: 0,
            inside: None,
        }
    }

    fn facts() -> SplitFacts {
        SplitFacts {
            who: 1,
            army: -1,
            facing: 3,
            destination_is_water: false,
            frame: 7,
        }
    }

    #[test]
    fn an_all_land_selection_never_splits() {
        let mut world = World::default();
        for o in 0..3 {
            world.0.insert((1, o), land(o));
        }
        assert_eq!(
            plan_move_near_split(&[0, 1, 2], &facts(), &world),
            MoveNearSplit::Continue
        );
    }

    #[test]
    fn land_and_sea_split_on_pass_one() {
        let mut world = World::default();
        world.0.insert((1, 0), land(0));
        world.0.insert((1, 1), land(1));
        let mut ship = land(2);
        ship.domain = DOMAIN_SEA;
        world.0.insert((1, 2), ship);

        let MoveNearSplit::Split {
            first,
            second,
            pass,
        } = plan_move_near_split(&[0, 1, 2], &facts(), &world)
        else {
            panic!("a mixed land/sea selection splits");
        };
        assert_eq!(pass, 1);
        assert_eq!((first.num, &first.list[..2]), (2, &[0, 1][..]));
        assert_eq!((second.num, &second.list[..1]), (1, &[2][..]));
        assert_eq!(second.who, 1);
        assert_eq!(first.army, -1, "Group::clear(-1) leaves army negative");
        assert_eq!(first.id, -1);
    }

    #[test]
    fn an_army_never_splits() {
        let mut world = World::default();
        world.0.insert((1, 0), land(0));
        let mut ship = land(1);
        ship.domain = DOMAIN_SEA;
        world.0.insert((1, 1), ship);
        let mut facts = facts();
        facts.army = 4;
        // Pass 1 declines because `army >= 0`; pass 2 then puts nothing in `first`
        // because no shipped type carries the transport flag and nobody is embarked.
        assert_eq!(
            plan_move_near_split(&[0, 1], &facts, &world),
            MoveNearSplit::Continue
        );
    }

    #[test]
    fn an_embarked_passenger_is_replaced_by_its_transport() {
        let mut world = World::default();
        world.0.insert((1, 0), land(0));
        let mut rider = land(1);
        rider.inside = Some((9, 1));
        world.0.insert((1, 1), rider);
        let mut transport = land(9);
        transport.domain = DOMAIN_SEA;
        world.0.insert((1, 9), transport);

        let MoveNearSplit::Split {
            first,
            second,
            pass,
        } = plan_move_near_split(&[0, 1], &facts(), &world)
        else {
            panic!("one land unit plus one embarked unit splits");
        };
        assert_eq!(pass, 1);
        assert_eq!((first.num, first.list[0]), (1, 0));
        assert_eq!(
            (second.num, second.list[0]),
            (1, 9),
            "the transport, not the passenger"
        );
    }

    #[test]
    fn two_passengers_in_one_transport_add_it_once() {
        let mut world = World::default();
        world.0.insert((1, 0), land(0));
        for o in [1, 2] {
            let mut rider = land(o);
            rider.inside = Some((9, 1));
            world.0.insert((1, o), rider);
        }
        let mut transport = land(9);
        transport.domain = DOMAIN_SEA;
        world.0.insert((1, 9), transport);

        let MoveNearSplit::Split { second, .. } =
            plan_move_near_split(&[0, 1, 2], &facts(), &world)
        else {
            panic!("splits");
        };
        assert_eq!(second.num, 1, "GroupData::member rejects the duplicate");
    }

    #[test]
    fn pass_two_runs_only_when_the_destination_is_not_water() {
        // Nothing lands in `first` on pass 1 (the only member is a ship), so pass 1
        // declines; pass 2 would then route the transport-flagged ship into `first`.
        let mut world = World::default();
        let mut ship = land(0);
        ship.domain = DOMAIN_SEA;
        ship.unit_flags = UNIT_FLAG_BOARDS_TRANSPORT;
        world.0.insert((1, 0), ship);
        let mut other = land(1);
        other.domain = DOMAIN_SEA;
        world.0.insert((1, 1), other);

        let MoveNearSplit::Split {
            first,
            second,
            pass,
        } = plan_move_near_split(&[0, 1], &facts(), &world)
        else {
            panic!("pass 2 splits a transport-flagged member out");
        };
        assert_eq!(pass, 2);
        assert_eq!((first.num, first.list[0]), (1, 0));
        assert_eq!((second.num, second.list[0]), (1, 1));

        let mut water = facts();
        water.destination_is_water = true;
        assert_eq!(
            plan_move_near_split(&[0, 1], &water, &world),
            MoveNearSplit::Continue,
            "a water destination skips pass 2"
        );
    }

    #[test]
    fn a_non_captain_member_is_replaced_by_its_captain() {
        let mut world = World::default();
        let mut squaddie = land(0);
        squaddie.is_captain = false;
        squaddie.captain = 5;
        world.0.insert((1, 0), squaddie);
        world.0.insert((1, 5), land(5));
        let mut ship = land(1);
        ship.domain = DOMAIN_SEA;
        world.0.insert((1, 1), ship);

        let MoveNearSplit::Split { first, .. } = plan_move_near_split(&[0, 1], &facts(), &world)
        else {
            panic!("splits");
        };
        assert_eq!((first.num, first.list[0]), (1, 5));
    }

    #[test]
    fn an_unanswered_object_is_unanswered_not_a_refusal() {
        let world = World::default();
        assert_eq!(
            plan_move_near_split(&[0], &facts(), &world),
            MoveNearSplit::Unanswered
        );
    }

    #[test]
    fn a_subordinate_chain_is_unanswered() {
        let mut world = World::default();
        let mut leader = land(0);
        leader.subordinate = 4;
        world.0.insert((1, 0), leader);
        let mut ship = land(1);
        ship.domain = DOMAIN_SEA;
        world.0.insert((1, 1), ship);
        assert_eq!(
            plan_move_near_split(&[0, 1], &facts(), &world),
            MoveNearSplit::Unanswered
        );
    }

    #[test]
    fn a_second_owner_cannot_join_a_started_temporary() {
        // `Group::add`'s one-owner gate: a container owned by a different player is
        // dropped rather than mixed in.
        let mut world = World::default();
        world.0.insert((1, 0), land(0));
        let mut rider = land(1);
        rider.inside = Some((9, 2));
        world.0.insert((1, 1), rider);
        let mut foreign = land(9);
        foreign.domain = DOMAIN_SEA;
        world.0.insert((2, 9), foreign);
        let mut rider2 = land(3);
        rider2.inside = Some((8, 1));
        world.0.insert((1, 3), rider2);
        let mut own = land(8);
        own.domain = DOMAIN_SEA;
        world.0.insert((1, 8), own);

        let MoveNearSplit::Split { second, .. } =
            plan_move_near_split(&[0, 1, 3], &facts(), &world)
        else {
            panic!("splits");
        };
        assert_eq!(second.who, 2, "the first add fixes the temporary's owner");
        assert_eq!(
            (second.num, second.list[0]),
            (1, 9),
            "the owner-1 transport is refused by the one-owner gate"
        );
    }

    #[test]
    fn group_clear_matches_the_emitted_scalar_writes() {
        let mut group = GroupData {
            id: 12,
            army: 3,
            num: 5,
            form: 2,
            ox: 11,
            oy: 13,
            facing: 9,
            buildings: 1,
            who: 4,
            march: 1,
            ..GroupData::default()
        };
        group.list[0] = 0x1234;
        group_clear(&mut group, -1, 99);
        assert_eq!(group.id, 12, "a negative argument leaves id alone");
        assert_eq!((group.army, group.num, group.form), (-1, 0, -1));
        assert_eq!(group.stamp, 99);
        assert_eq!(
            (group.facing, group.buildings, group.who, group.march),
            (0, 0, 0, 0)
        );
        assert_eq!(
            group.list[0], 0x1234,
            "Group::clear writes scalars only; the arrays survive"
        );
        group_clear(&mut group, 7, 99);
        assert_eq!(group.id, 7);
    }
}
