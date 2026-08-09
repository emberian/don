//! Completed-Wonder registry and Wonder-victory point supply.
//!
//! This is the bounded state transition reached by `Build::activate` after a Wonder
//! finishes.  It does **not** claim the rest of building activation or the unbuilt-Wonder
//! registry.  The recovered retail path is:
//!
//! - `Build::activate` calls `Wonders::init_wonder` at `0x00625B5B` and stores its return
//!   in `BuildData::wonder` (`+0x76`);
//! - `Wonders::init_wonder` `0x0073C860` reuses the first inactive slot below
//!   `LeaderData::wonder_mark` (`+0x424`), or appends at the mark, then calls the inlined
//!   `Wonder::init` body from `0x0073C986..0x0073C9E9`;
//! - `Wonders::close_wonder` `0x0073C7E0` invalidates a slot and trims only inactive
//!   records at the tail of `wonder_mark`;
//! - `LeaderData::get_wonder_value` `0x006EBB90`,
//!   `get_team_wonder_value` `0x006DA990`, and `get_wonder_net` `0x006EBB10` supply
//!   `Game::wonder_winning`.
//!
//! Retail reaches through global object/type/game stores for the completed object's type,
//! prerequisite bit, and current Wonder value.  Those stores are deliberately not copied
//! here.  [`WonderWorld`] is mandatory, has no default implementation, and every query or
//! write returns an effect receipt.  An absent, stale, mutating query or unconfirmed game-bit
//! write is an error; it never silently manufactures zero Wonder points.

use super::victory_score::{self, Leaders};

pub const NUM_WONDER_OWNERS: usize = victory_score::NUM_LEADERS;
pub const WONDER_FIRST: i32 = 0x20E;
pub const WONDER_LAST: i32 = 0x21E;
pub const WONDER_VALID: u8 = 0x01;
pub const INVALID_SHORT: i16 = -1;
pub const INVALID_WHO: i8 = -1;

/// The checksummed `WonderData` prefix (PDB size `0x10`).
///
/// `Wonder` itself is 24 bytes because the output/access class adds a virtual-base pointer;
/// the six fields below are the simulation data walked by `Wonder::walk_data` `0x0073C780`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WonderRecord {
    pub wonder: i16,
    pub o: i16,
    pub stamp: i32,
    pub timer: i32,
    pub wonder_flags: u8,
    pub who: i8,
}

impl Default for WonderRecord {
    fn default() -> Self {
        Self {
            wonder: INVALID_SHORT,
            o: INVALID_SHORT,
            stamp: 0,
            timer: 0,
            // `Wonder::Wonder` `0x0073C740` writes the adjacent bytes as `ff00`.
            wonder_flags: 0,
            who: INVALID_WHO,
        }
    }
}

impl WonderRecord {
    #[inline]
    pub fn is_valid(self) -> bool {
        self.wonder_flags & WONDER_VALID != 0
    }
}

/// One read-only host query plus explicit proof that it stayed read-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadReceipt<T> {
    pub value: T,
    pub rng_draws: u32,
    pub world_writes: u32,
}

/// Inputs read by `Wonder::init` `0x0073C5E0` / `Wonder::get_timer` `0x0073C660`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WonderInitFacts {
    pub who: i32,
    pub o: i32,
    pub frame: i32,
    pub type_index: i32,
    /// The prerequisite TypeIndex whose `Game` availability word receives `|= 1`.
    pub prerequisite_index: i32,
    pub world_xs: i32,
    /// `map_sizes.list[3].data[0]`, the denominator loaded at `0x0073C68C`.
    pub standard_map_xs: i32,
    /// `Constants+0xD08`.
    pub wonder_timer: i32,
    /// `Constants+0xD0C`.
    pub wonder_age: i32,
}

/// Confirmation of the one mandatory external write in `Wonder::init`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WonderFlagReceipt {
    pub who: i32,
    pub o: i32,
    pub type_index: i32,
    pub prerequisite_index: i32,
    pub flag_is_set: bool,
    pub rng_draws: u32,
    pub world_writes: u32,
}

/// Identity-bound result of `ObjectData::get_wonder_value` for one live record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WonderValue {
    pub who: i32,
    pub o: i32,
    pub value: i32,
}

/// Mandatory external object/type/game store used by the recovered bodies.
///
/// The successful retail body cannot fail because all referenced objects already exist.
/// The Rust boundary can fail, so it verifies all identities/effects before committing
/// registry state.  That gives the same successful final state and a fail-closed error path.
pub trait WonderWorld {
    fn init_facts(
        &mut self,
        who: i32,
        o: i32,
    ) -> Result<ReadReceipt<WonderInitFacts>, WonderWorldError>;

    fn set_prerequisite_complete(
        &mut self,
        facts: WonderInitFacts,
    ) -> Result<WonderFlagReceipt, WonderWorldError>;

    fn wonder_value(
        &mut self,
        who: i32,
        o: i32,
    ) -> Result<ReadReceipt<WonderValue>, WonderWorldError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WonderWorldError {
    pub detail: String,
}

impl From<&str> for WonderWorldError {
    fn from(detail: &str) -> Self {
        Self {
            detail: detail.to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WonderError {
    MissingWorld,
    World(WonderWorldError),
    InvalidOwner(i32),
    InvalidObject(i32),
    InvalidWonderType(i32),
    InvalidPrerequisite(i32),
    InvalidMapScale {
        world_xs: i32,
        standard_map_xs: i32,
    },
    StaleInitFacts {
        expected_who: i32,
        expected_o: i32,
        actual_who: i32,
        actual_o: i32,
    },
    UnexpectedEffects {
        operation: &'static str,
        rng_draws: u32,
        world_writes: u32,
    },
    FlagReceiptMismatch,
    ValueReceiptMismatch {
        expected_who: i32,
        expected_o: i32,
        actual_who: i32,
        actual_o: i32,
    },
    CorruptMark {
        who: usize,
        mark: i32,
        allocated: usize,
    },
    SlotIndexOverflow(usize),
    Allocation,
    InvalidSlot {
        who: i32,
        wonder: i32,
        allocated: usize,
    },
}

impl From<WonderWorldError> for WonderError {
    fn from(value: WonderWorldError) -> Self {
        Self::World(value)
    }
}

/// The eight `PtrArray<Wonder>` lists plus their two LeaderData-side counters.
///
/// `wonder_mark` is the logical prefix the retail getters walk. `wonders_held` is a
/// lifetime high-water statistic: initialization raises it to the maximum simultaneous
/// active count and closing a Wonder does not lower it (`0x0073CA11..0x0073CA1F`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wonders {
    lists: [Vec<WonderRecord>; NUM_WONDER_OWNERS],
    wonder_mark: [i32; NUM_WONDER_OWNERS],
    wonders_held: [i32; NUM_WONDER_OWNERS],
}

impl Default for Wonders {
    fn default() -> Self {
        Self {
            lists: std::array::from_fn(|_| Vec::new()),
            wonder_mark: [0; NUM_WONDER_OWNERS],
            wonders_held: [0; NUM_WONDER_OWNERS],
        }
    }
}

impl Wonders {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn wonder_mark(&self, who: usize) -> i32 {
        self.wonder_mark[who]
    }

    #[inline]
    pub fn wonders_held(&self, who: usize) -> i32 {
        self.wonders_held[who]
    }

    #[inline]
    pub fn record(&self, who: usize, wonder: usize) -> Option<&WonderRecord> {
        self.lists.get(who)?.get(wonder)
    }

    pub fn has_active(&self) -> bool {
        self.lists.iter().enumerate().any(|(who, list)| {
            let mark = self.wonder_mark[who].max(0) as usize;
            list.iter().take(mark).any(|record| record.is_valid())
        })
    }

    /// `Wonders::init_wonder` `0x0073C860`, the completed-Wonder registration called by
    /// `Build::activate` at `0x00625B5B`.
    pub fn init_wonder<W: WonderWorld + ?Sized>(
        &mut self,
        world: &mut W,
        who: i32,
        o: i32,
    ) -> Result<i16, WonderError> {
        let owner = checked_owner(who)?;
        if o < 0 || o > i16::MAX as i32 {
            return Err(WonderError::InvalidObject(o));
        }

        let facts_receipt = world.init_facts(who, o)?;
        require_read_only(
            "WonderWorld::init_facts",
            facts_receipt.rng_draws,
            facts_receipt.world_writes,
        )?;
        let facts = facts_receipt.value;
        if facts.who != who || facts.o != o {
            return Err(WonderError::StaleInitFacts {
                expected_who: who,
                expected_o: o,
                actual_who: facts.who,
                actual_o: facts.o,
            });
        }
        if !(WONDER_FIRST..=WONDER_LAST).contains(&facts.type_index) {
            return Err(WonderError::InvalidWonderType(facts.type_index));
        }
        if facts.prerequisite_index < 0 {
            return Err(WonderError::InvalidPrerequisite(facts.prerequisite_index));
        }
        let timer = wonder_timer(facts)?;

        let mark = checked_mark(owner, self.wonder_mark[owner], self.lists[owner].len())?;
        let slot = self.lists[owner][..mark]
            .iter()
            .position(|record| !record.is_valid())
            .unwrap_or(mark);
        let slot_i16 = i16::try_from(slot).map_err(|_| WonderError::SlotIndexOverflow(slot))?;

        // Nothing which can return normally is allowed to fail after the external OR.
        if slot == self.lists[owner].len() {
            self.lists[owner]
                .try_reserve(1)
                .map_err(|_| WonderError::Allocation)?;
        }

        let flag = world.set_prerequisite_complete(facts)?;
        if flag.rng_draws != 0 || flag.world_writes != 1 {
            return Err(WonderError::UnexpectedEffects {
                operation: "WonderWorld::set_prerequisite_complete",
                rng_draws: flag.rng_draws,
                world_writes: flag.world_writes,
            });
        }
        if flag.who != who
            || flag.o != o
            || flag.type_index != facts.type_index
            || flag.prerequisite_index != facts.prerequisite_index
            || !flag.flag_is_set
        {
            return Err(WonderError::FlagReceiptMismatch);
        }

        let record = WonderRecord {
            wonder: slot_i16,
            o: o as i16,
            stamp: facts.frame,
            timer,
            wonder_flags: WONDER_VALID,
            who: who as i8,
        };
        if slot == self.lists[owner].len() {
            self.lists[owner].push(record);
        } else {
            self.lists[owner][slot] = record;
        }
        self.wonder_mark[owner] = self.wonder_mark[owner].max(slot as i32 + 1);

        let active = self.lists[owner][..self.wonder_mark[owner] as usize]
            .iter()
            .filter(|record| record.is_valid())
            .count() as i32;
        self.wonders_held[owner] = self.wonders_held[owner].max(active);
        Ok(slot_i16)
    }

    /// `Wonders::close_wonder` `0x0073C7E0`.
    ///
    /// Retail always returns `-1`, clears only valid/who/o, and preserves the retired
    /// record's slot number, stamp, timer, and non-valid flag bits.  The caller owns its
    /// separate leader-dirty/tribe-bonus effects.
    pub fn close_wonder(&mut self, who: i32, wonder: i32) -> Result<i32, WonderError> {
        let owner = checked_owner(who)?;
        let mut mark = checked_mark(owner, self.wonder_mark[owner], self.lists[owner].len())?;
        let slot = usize::try_from(wonder).map_err(|_| WonderError::InvalidSlot {
            who,
            wonder,
            allocated: self.lists[owner].len(),
        })?;
        if slot >= self.lists[owner].len() {
            return Err(WonderError::InvalidSlot {
                who,
                wonder,
                allocated: self.lists[owner].len(),
            });
        }
        let record = &mut self.lists[owner][slot];
        record.wonder_flags &= !WONDER_VALID;
        record.who = INVALID_WHO;
        record.o = INVALID_SHORT;

        while mark > 0 && !self.lists[owner][mark - 1].is_valid() {
            mark -= 1;
        }
        self.wonder_mark[owner] = mark as i32;
        Ok(-1)
    }

    /// Exact inputs for `Game::wonder_winning`: per-leader Wonder net and individual
    /// Wonder value, in leader-slot order.
    pub fn victory_inputs<W: WonderWorld + ?Sized>(
        &self,
        world: &mut W,
        leaders: &Leaders,
    ) -> Result<([i32; NUM_WONDER_OWNERS], [i32; NUM_WONDER_OWNERS]), WonderError> {
        let mut individual = [0i32; NUM_WONDER_OWNERS];
        for (who, total) in individual.iter_mut().enumerate() {
            let mark = checked_mark(who, self.wonder_mark[who], self.lists[who].len())?;
            // All callers in the recovered net calculation first test LEADER_VALID.
            if !leaders.slots[who].flag(victory_score::leader_flag::VALID) {
                continue;
            }
            for (slot, record) in self.lists[who][..mark].iter().enumerate() {
                if !record.is_valid() {
                    continue;
                }
                if record.wonder != slot as i16 || record.who != who as i8 || record.o < 0 {
                    return Err(WonderError::InvalidSlot {
                        who: who as i32,
                        wonder: slot as i32,
                        allocated: self.lists[who].len(),
                    });
                }
                let value = world.wonder_value(who as i32, record.o as i32)?;
                require_read_only(
                    "WonderWorld::wonder_value",
                    value.rng_draws,
                    value.world_writes,
                )?;
                if value.value.who != who as i32 || value.value.o != record.o as i32 {
                    return Err(WonderError::ValueReceiptMismatch {
                        expected_who: who as i32,
                        expected_o: record.o as i32,
                        actual_who: value.value.who,
                        actual_o: value.value.o,
                    });
                }
                *total = total.wrapping_add(value.value.value);
            }
        }

        // `LeaderData::get_team_wonder_value` `0x006DA990`.
        let mut team = [0i32; NUM_WONDER_OWNERS];
        for (who, team_total) in team.iter_mut().enumerate() {
            let mut sum = 0i32;
            for (member, value) in individual.iter().copied().enumerate() {
                if leaders.slots[member].flag(victory_score::leader_flag::VALID)
                    && (member == who || leaders.is_ally(who, member))
                {
                    sum = sum.wrapping_add(value);
                }
            }
            *team_total = sum;
        }

        // `LeaderData::get_wonder_net` `0x006EBB10`: own team less the strongest
        // non-allied valid leader's team, clamped at zero.
        let mut net = [0i32; NUM_WONDER_OWNERS];
        for (who, net_total) in net.iter_mut().enumerate() {
            if !leaders.slots[who].flag(victory_score::leader_flag::VALID) {
                continue;
            }
            let mut enemy_max = 0i32;
            for (enemy, enemy_team) in team.iter().copied().enumerate() {
                if leaders.slots[enemy].flag(victory_score::leader_flag::VALID)
                    && enemy != who
                    && !leaders.is_ally(who, enemy)
                {
                    enemy_max = enemy_max.max(enemy_team);
                }
            }
            *net_total = team[who].wrapping_sub(enemy_max).max(0);
        }
        Ok((net, individual))
    }
}

/// `Wonder::get_timer` `0x0073C660`.
pub fn wonder_timer(facts: WonderInitFacts) -> Result<i32, WonderError> {
    if !(WONDER_FIRST..=WONDER_LAST).contains(&facts.type_index) {
        return Err(WonderError::InvalidWonderType(facts.type_index));
    }
    if facts.world_xs <= 0 || facts.standard_map_xs <= 0 {
        return Err(WonderError::InvalidMapScale {
            world_xs: facts.world_xs,
            standard_map_xs: facts.standard_map_xs,
        });
    }
    let base = scale_frames(facts.wonder_timer, facts.world_xs, facts.standard_map_xs);
    let per_age = scale_frames(facts.wonder_age, facts.world_xs, facts.standard_map_xs);
    Ok((WONDER_LAST.wrapping_sub(facts.type_index))
        .wrapping_mul(per_age)
        .wrapping_add(base))
}

#[inline]
fn scale_frames(value: i32, world_xs: i32, standard_map_xs: i32) -> i32 {
    if value < 1 {
        return 0;
    }
    let half = standard_map_xs / 2;
    world_xs
        .wrapping_mul(value)
        .wrapping_add(half)
        .wrapping_div(standard_map_xs)
        .max(1)
}

#[inline]
fn checked_owner(who: i32) -> Result<usize, WonderError> {
    let owner = usize::try_from(who).map_err(|_| WonderError::InvalidOwner(who))?;
    if owner >= NUM_WONDER_OWNERS {
        return Err(WonderError::InvalidOwner(who));
    }
    Ok(owner)
}

#[inline]
fn checked_mark(who: usize, mark: i32, allocated: usize) -> Result<usize, WonderError> {
    let converted = usize::try_from(mark).map_err(|_| WonderError::CorruptMark {
        who,
        mark,
        allocated,
    })?;
    if converted > allocated {
        return Err(WonderError::CorruptMark {
            who,
            mark,
            allocated,
        });
    }
    Ok(converted)
}

#[inline]
fn require_read_only(
    operation: &'static str,
    rng_draws: u32,
    world_writes: u32,
) -> Result<(), WonderError> {
    if rng_draws != 0 || world_writes != 0 {
        return Err(WonderError::UnexpectedEffects {
            operation,
            rng_draws,
            world_writes,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::victory_score::{leader_flag, Diplo, ScoreConstants, TypeTable};
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct FakeWorld {
        facts: BTreeMap<(i32, i32), WonderInitFacts>,
        values: BTreeMap<(i32, i32), i32>,
        flag_calls: Vec<WonderInitFacts>,
        fail_flag: bool,
        stale_value: bool,
        query_writes: u32,
    }

    impl FakeWorld {
        fn add(&mut self, who: i32, o: i32, type_index: i32, value: i32) {
            self.facts.insert(
                (who, o),
                WonderInitFacts {
                    who,
                    o,
                    frame: 120,
                    type_index,
                    prerequisite_index: 0x220 + (type_index - WONDER_FIRST),
                    world_xs: 70,
                    standard_map_xs: 70,
                    wonder_timer: 4500,
                    wonder_age: 0,
                },
            );
            self.values.insert((who, o), value);
        }
    }

    impl WonderWorld for FakeWorld {
        fn init_facts(
            &mut self,
            who: i32,
            o: i32,
        ) -> Result<ReadReceipt<WonderInitFacts>, WonderWorldError> {
            let value = self
                .facts
                .get(&(who, o))
                .copied()
                .ok_or_else(|| WonderWorldError::from("missing facts"))?;
            Ok(ReadReceipt {
                value,
                rng_draws: 0,
                world_writes: self.query_writes,
            })
        }

        fn set_prerequisite_complete(
            &mut self,
            facts: WonderInitFacts,
        ) -> Result<WonderFlagReceipt, WonderWorldError> {
            if self.fail_flag {
                return Err(WonderWorldError::from("flag store refused"));
            }
            self.flag_calls.push(facts);
            Ok(WonderFlagReceipt {
                who: facts.who,
                o: facts.o,
                type_index: facts.type_index,
                prerequisite_index: facts.prerequisite_index,
                flag_is_set: true,
                rng_draws: 0,
                world_writes: 1,
            })
        }

        fn wonder_value(
            &mut self,
            who: i32,
            o: i32,
        ) -> Result<ReadReceipt<WonderValue>, WonderWorldError> {
            let value = *self
                .values
                .get(&(who, o))
                .ok_or_else(|| WonderWorldError::from("missing value"))?;
            Ok(ReadReceipt {
                value: WonderValue {
                    who: if self.stale_value { who ^ 1 } else { who },
                    o,
                    value,
                },
                rng_draws: 0,
                world_writes: self.query_writes,
            })
        }
    }

    fn leaders(valid: &[usize]) -> Leaders {
        let mut leaders = Leaders::new(TypeTable::with_default_kinds(ScoreConstants::default()));
        for &who in valid {
            leaders.slots[who].leader_flags |= leader_flag::VALID | leader_flag::ACTIVE;
        }
        leaders
    }

    fn ally(leaders: &mut Leaders, a: usize, b: usize) {
        leaders.set_diplo(a, b, Diplo::Ally);
        leaders.set_diplo(b, a, Diplo::Ally);
    }

    #[test]
    fn init_stamps_exact_record_and_requires_the_game_flag_write() {
        let mut registry = Wonders::new();
        let mut world = FakeWorld::default();
        world.add(2, 17, WONDER_FIRST, 4);

        assert_eq!(registry.init_wonder(&mut world, 2, 17), Ok(0));
        assert_eq!(registry.wonder_mark(2), 1);
        assert_eq!(registry.wonders_held(2), 1);
        assert_eq!(
            registry.record(2, 0),
            Some(&WonderRecord {
                wonder: 0,
                o: 17,
                stamp: 120,
                timer: 4500,
                wonder_flags: WONDER_VALID,
                who: 2,
            })
        );
        assert_eq!(world.flag_calls.len(), 1);
    }

    #[test]
    fn refused_external_write_leaves_the_registry_unchanged() {
        let mut registry = Wonders::new();
        let before = registry.clone();
        let mut world = FakeWorld::default();
        world.add(0, 4, WONDER_FIRST, 1);
        world.fail_flag = true;

        assert!(matches!(
            registry.init_wonder(&mut world, 0, 4),
            Err(WonderError::World(_))
        ));
        assert_eq!(registry, before);
    }

    #[test]
    fn mutating_fact_query_is_rejected_before_the_external_write() {
        let mut registry = Wonders::new();
        let mut world = FakeWorld::default();
        world.add(0, 4, WONDER_FIRST, 1);
        world.query_writes = 1;

        assert_eq!(
            registry.init_wonder(&mut world, 0, 4),
            Err(WonderError::UnexpectedEffects {
                operation: "WonderWorld::init_facts",
                rng_draws: 0,
                world_writes: 1,
            })
        );
        assert!(world.flag_calls.is_empty());
        assert_eq!(registry.wonder_mark(0), 0);
    }

    #[test]
    fn first_hole_is_reused_and_tail_close_only_trims_the_tail() {
        let mut registry = Wonders::new();
        let mut world = FakeWorld::default();
        for o in 10..13 {
            world.add(0, o, WONDER_FIRST + (o - 10), 1);
            assert_eq!(registry.init_wonder(&mut world, 0, o), Ok((o - 10) as i16));
        }
        assert_eq!(registry.wonders_held(0), 3);

        assert_eq!(registry.close_wonder(0, 1), Ok(-1));
        assert_eq!(
            registry.wonder_mark(0),
            3,
            "a middle hole does not trim the mark"
        );
        let retired = *registry.record(0, 1).unwrap();
        assert_eq!(retired.wonder, 1);
        assert_eq!(retired.stamp, 120);
        assert_eq!(retired.timer, 4500);
        assert_eq!(retired.o, -1);
        assert_eq!(retired.who, -1);

        world.add(0, 99, WONDER_LAST, 9);
        assert_eq!(registry.init_wonder(&mut world, 0, 99), Ok(1));
        assert_eq!(registry.wonder_mark(0), 3);
        assert_eq!(registry.record(0, 1).unwrap().o, 99);

        assert_eq!(registry.close_wonder(0, 2), Ok(-1));
        assert_eq!(registry.wonder_mark(0), 2);
        assert_eq!(registry.wonders_held(0), 3, "held is a lifetime high-water");
    }

    #[test]
    fn timer_uses_map_rounding_and_the_descending_wonder_age_term() {
        let facts = WonderInitFacts {
            who: 0,
            o: 1,
            frame: 0,
            type_index: WONDER_FIRST,
            prerequisite_index: 0x220,
            world_xs: 40,
            standard_map_xs: 70,
            wonder_timer: 4500,
            wonder_age: 70,
        };
        // base=(40*4500+35)/70=2571; age=(40*70+35)/70=40; 0x21e-0x20e=16.
        assert_eq!(wonder_timer(facts), Ok(3211));
        assert_eq!(
            wonder_timer(WonderInitFacts {
                type_index: WONDER_LAST,
                ..facts
            }),
            Ok(2571)
        );
    }

    #[test]
    fn net_is_team_total_minus_the_strongest_hostile_team() {
        let mut registry = Wonders::new();
        let mut world = FakeWorld::default();
        for (who, o, value) in [(0, 10, 4), (1, 11, 3), (2, 12, 5), (3, 13, 2)] {
            world.add(who, o, WONDER_FIRST + who, value);
            registry.init_wonder(&mut world, who, o).unwrap();
        }
        let mut leaders = leaders(&[0, 1, 2, 3]);
        ally(&mut leaders, 0, 1);
        ally(&mut leaders, 2, 3);

        let (net, value) = registry.victory_inputs(&mut world, &leaders).unwrap();
        assert_eq!(&value[..4], &[4, 3, 5, 2]);
        assert_eq!(&net[..4], &[0, 0, 0, 0], "the hostile teams tie at seven");

        *world.values.get_mut(&(0, 10)).unwrap() = 5;
        let (net, _) = registry.victory_inputs(&mut world, &leaders).unwrap();
        assert_eq!(&net[..4], &[1, 1, 0, 0]);
    }

    #[test]
    fn one_sided_ally_state_is_hostile_and_changes_net() {
        let mut registry = Wonders::new();
        let mut world = FakeWorld::default();
        world.add(0, 1, WONDER_FIRST, 4);
        world.add(1, 2, WONDER_FIRST + 1, 3);
        registry.init_wonder(&mut world, 0, 1).unwrap();
        registry.init_wonder(&mut world, 1, 2).unwrap();
        let mut leaders = leaders(&[0, 1]);

        leaders.set_diplo(0, 1, Diplo::Ally);
        let (net, _) = registry.victory_inputs(&mut world, &leaders).unwrap();
        assert_eq!(
            &net[..2],
            &[1, 0],
            "mutual minimum keeps one-sided ally hostile"
        );

        leaders.set_diplo(1, 0, Diplo::Ally);
        let (net, _) = registry.victory_inputs(&mut world, &leaders).unwrap();
        assert_eq!(&net[..2], &[7, 7]);
    }

    #[test]
    fn stale_or_mutating_value_receipts_fail_closed() {
        let mut registry = Wonders::new();
        let mut world = FakeWorld::default();
        world.add(0, 1, WONDER_FIRST, 4);
        registry.init_wonder(&mut world, 0, 1).unwrap();
        let leaders = leaders(&[0]);

        world.stale_value = true;
        assert!(matches!(
            registry.victory_inputs(&mut world, &leaders),
            Err(WonderError::ValueReceiptMismatch { .. })
        ));
        world.stale_value = false;
        world.query_writes = 1;
        assert!(matches!(
            registry.victory_inputs(&mut world, &leaders),
            Err(WonderError::UnexpectedEffects {
                operation: "WonderWorld::wonder_value",
                ..
            })
        ));
    }
}
