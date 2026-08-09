//! Completed-Wonder registry and Wonder-victory point supply.
//!
//! This is the bounded Wonder state transition reached by `Build::activate` after a Wonder
//! finishes, plus the caller-owned completion/close/capture transaction around it.  The
//! recovered retail path is:
//!
//! - `Build::activate` removes the first matching unbuilt record, increments
//!   `LeaderData::wonders_built`, calls `Wonders::init_wonder` at `0x00625B5B`, and stores
//!   its return in `BuildData::wonder` (`+0x76`);
//! - `Wonders::init_wonder` `0x0073C860` reuses the first inactive slot below
//!   `LeaderData::wonder_mark` (`+0x424`), or appends at the mark, then calls the inlined
//!   `Wonder::init` body from `0x0073C986..0x0073C9E9`;
//! - `Wonders::close_wonder` `0x0073C7E0` invalidates a slot and trims only inactive
//!   records at the tail of `wonder_mark`;
//! - `Build::check_capture` calls the generic `Build::swap_team`, activates the new build,
//!   closes the old build, and finally masks the new build. A captured Wonder is therefore
//!   newly registered for the capturer, not moved between registry lists;
//! - `LeaderData::get_wonder_value` `0x006EBB90`,
//!   `get_team_wonder_value` `0x006DA990`, and `get_wonder_net` `0x006EBB10` supply
//!   `Game::wonder_winning`.
//!
//! Retail reaches through global object/type/game stores for the completed object's type,
//! prerequisite bit, and current Wonder value.  Those stores are deliberately not copied
//! here.  [`WonderWorld`] is mandatory, has no default implementation, and every query or
//! write returns an effect receipt.  An absent, stale, mutating query or unconfirmed game-bit
//! write is an error; it never silently manufactures zero Wonder points.

use super::{
    production::BuildData,
    victory_score::{self, Leaders},
};

pub const NUM_WONDER_OWNERS: usize = victory_score::NUM_LEADERS;
pub const WONDER_FIRST: i32 = 0x20E;
pub const WONDER_LAST: i32 = 0x21E;
pub const WONDER_VALID: u8 = 0x01;
pub const INVALID_SHORT: i16 = -1;
pub const INVALID_WHO: i8 = -1;

/// Packed entry in the per-owner `UnbuiltWonders` lists (PDB size `0x4`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnbuiltWonder {
    pub o: i16,
    pub who: i8,
}

const _: [(); 4] = [(); std::mem::size_of::<UnbuiltWonder>()];

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

/// Retail caller-side operation requested around the completed-Wonder registry.
///
/// Mutable [`BuildData`] references make the caller-owned `wonder` link part of the same
/// local commit as the registry and counters. The `Capture` variant starts after the generic
/// object copy has produced `new_build`; its mandatory swap receipt still has to prove the
/// old/new object identities before any Wonder state can commit.
pub enum WonderLifecycle<'a> {
    Complete {
        o: i32,
        build: &'a mut BuildData,
    },
    Close {
        o: i32,
        build: &'a mut BuildData,
        remove_unbuilt: bool,
    },
    Capture {
        old_o: i32,
        old_build: &'a mut BuildData,
        new_o: i32,
        new_build: &'a mut BuildData,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureSwapRequest {
    pub old_who: i32,
    pub old_o: i32,
    pub old_wonder: i16,
    pub new_who: i32,
    pub new_o: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureSwapReceipt {
    pub old_who: i32,
    pub old_o: i32,
    pub old_wonder: i16,
    pub new_who: i32,
    pub new_o: i32,
    pub swap_complete: bool,
    pub rng_draws: u32,
    pub world_writes: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloseEffectsRequest {
    pub who: i32,
    pub o: i32,
    pub wonder: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloseEffectsReceipt {
    pub who: i32,
    pub o: i32,
    pub wonder: i16,
    pub type_index: i32,
    /// The caller ORed `leaders::flag::UNIT_STATS_DIRTY` (`0x0400_0000`).
    pub unit_stats_dirty: bool,
    /// Whether the type-specific Wonder bonus/terrain recalculation completed.
    pub type_specific_recalculated: bool,
    pub rng_draws: u32,
    pub world_writes: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureMaskRequest {
    pub who: i32,
    pub o: i32,
    pub first: i32,
    pub second: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureMaskReceipt {
    pub who: i32,
    pub o: i32,
    pub first: i32,
    pub second: i32,
    pub mask_complete: bool,
    pub rng_draws: u32,
    pub world_writes: u32,
}

/// Mandatory non-registry effects surrounding Wonder completion/capture/close.
///
/// These methods have no defaults. In particular, DoN must not treat a local
/// `close_wonder` as a complete `Build::close`: retail also marks unit stats dirty and, for
/// four Wonder types, runs a type-specific recalculation. Capture likewise remains blocked
/// without identity-bound proof of `Build::swap_team` and the final `mask_me(1, 2)`.
pub trait WonderLifecycleHost {
    fn capture_swap(
        &mut self,
        request: CaptureSwapRequest,
    ) -> Result<CaptureSwapReceipt, WonderWorldError>;

    fn close_effects(
        &mut self,
        request: CloseEffectsRequest,
    ) -> Result<CloseEffectsReceipt, WonderWorldError>;

    fn capture_mask(
        &mut self,
        request: CaptureMaskRequest,
    ) -> Result<CaptureMaskReceipt, WonderWorldError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WonderLifecycleReceipt {
    Completed {
        who: i32,
        o: i32,
        wonder: i16,
        unbuilt_removed: bool,
        wonders_built: i32,
    },
    Closed {
        who: i32,
        o: i32,
        wonder: Option<i16>,
        unbuilt_removed: bool,
    },
    Captured {
        old_who: i32,
        old_o: i32,
        old_wonder: i16,
        new_who: i32,
        new_o: i32,
        new_wonder: i16,
        new_unbuilt_removed: bool,
        old_unbuilt_removed: bool,
        new_wonders_built: i32,
    },
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
    BuildWonderMismatch {
        who: i32,
        o: i32,
        wonder: i16,
    },
    SameCaptureOwner(i32),
    LifecycleReceiptMismatch {
        operation: &'static str,
    },
}

impl From<WonderWorldError> for WonderError {
    fn from(value: WonderWorldError) -> Self {
        Self::World(value)
    }
}

/// The eight completed/unbuilt lists plus their LeaderData-side counters.
///
/// `wonder_mark` is the logical prefix the retail getters walk. `wonders_held` is a
/// lifetime high-water statistic: initialization raises it to the maximum simultaneous
/// active count and closing a Wonder does not lower it (`0x0073CA11..0x0073CA1F`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wonders {
    lists: [Vec<WonderRecord>; NUM_WONDER_OWNERS],
    unbuilt: [Vec<UnbuiltWonder>; NUM_WONDER_OWNERS],
    wonder_mark: [i32; NUM_WONDER_OWNERS],
    wonders_built: [i32; NUM_WONDER_OWNERS],
    wonders_held: [i32; NUM_WONDER_OWNERS],
}

impl Default for Wonders {
    fn default() -> Self {
        Self {
            lists: std::array::from_fn(|_| Vec::new()),
            unbuilt: std::array::from_fn(|_| Vec::new()),
            wonder_mark: [0; NUM_WONDER_OWNERS],
            wonders_built: [0; NUM_WONDER_OWNERS],
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
    pub fn wonders_built(&self, who: usize) -> i32 {
        self.wonders_built[who]
    }

    #[inline]
    pub fn unbuilt(&self, who: usize) -> &[UnbuiltWonder] {
        &self.unbuilt[who]
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

    /// `UnbuiltWonders::add_unbuilt_wonder` `0x0073C1D0`.
    pub fn add_unbuilt_wonder(&mut self, who: i32, o: i32) -> Result<(), WonderError> {
        let owner = checked_owner(who)?;
        if o < 0 || o > i16::MAX as i32 {
            return Err(WonderError::InvalidObject(o));
        }
        self.unbuilt[owner]
            .try_reserve(1)
            .map_err(|_| WonderError::Allocation)?;
        self.unbuilt[owner].push(UnbuiltWonder {
            o: o as i16,
            who: who as i8,
        });
        Ok(())
    }

    /// Execute one caller-owned Wonder lifecycle transaction.
    ///
    /// Local registry, counter, unbuilt-list, and `BuildData::wonder` changes are staged and
    /// commit together only after every mandatory external receipt validates. External host
    /// operations cannot be rolled back by this module; an error after a host call therefore
    /// remains an explicit incomplete lifecycle, never a locally reported success.
    pub fn apply_build_lifecycle<W>(
        &mut self,
        world: &mut W,
        lifecycle: WonderLifecycle<'_>,
    ) -> Result<WonderLifecycleReceipt, WonderError>
    where
        W: WonderWorld + WonderLifecycleHost + ?Sized,
    {
        let mut staged = self.clone();
        match lifecycle {
            WonderLifecycle::Complete { o, build } => {
                let mut staged_build = build.clone();
                let receipt = staged.complete_build(world, o, &mut staged_build)?;
                *self = staged;
                *build = staged_build;
                Ok(receipt)
            }
            WonderLifecycle::Close {
                o,
                build,
                remove_unbuilt,
            } => {
                let mut staged_build = build.clone();
                let receipt = staged.close_build(world, o, &mut staged_build, remove_unbuilt)?;
                *self = staged;
                *build = staged_build;
                Ok(receipt)
            }
            WonderLifecycle::Capture {
                old_o,
                old_build,
                new_o,
                new_build,
            } => {
                let mut staged_old = old_build.clone();
                let mut staged_new = new_build.clone();
                let receipt =
                    staged.capture_build(world, old_o, &mut staged_old, new_o, &mut staged_new)?;
                *self = staged;
                *old_build = staged_old;
                *new_build = staged_new;
                Ok(receipt)
            }
        }
    }

    fn complete_build<W>(
        &mut self,
        world: &mut W,
        o: i32,
        build: &mut BuildData,
    ) -> Result<WonderLifecycleReceipt, WonderError>
    where
        W: WonderWorld + WonderLifecycleHost + ?Sized,
    {
        let who = i32::from(build.who);
        let owner = checked_build_identity(o, build)?;

        // `Build::activate` `0x00625B32..0x00625B60`: swap-remove first, then increment,
        // register, and finally store the returned short in `BuildData::wonder`.
        let unbuilt_removed = remove_unbuilt(&mut self.unbuilt[owner], o as i16);
        self.wonders_built[owner] = self.wonders_built[owner].wrapping_add(1);
        let wonder = self.init_wonder(world, who, o)?;
        build.wonder = wonder;

        Ok(WonderLifecycleReceipt::Completed {
            who,
            o,
            wonder,
            unbuilt_removed,
            wonders_built: self.wonders_built[owner],
        })
    }

    fn close_build<W>(
        &mut self,
        world: &mut W,
        o: i32,
        build: &mut BuildData,
        remove_unbuilt_entry: bool,
    ) -> Result<WonderLifecycleReceipt, WonderError>
    where
        W: WonderWorld + WonderLifecycleHost + ?Sized,
    {
        let who = i32::from(build.who);
        let owner = checked_build_identity(o, build)?;
        let mut closed = None;

        // `Build::close` guards the completed-registry portion with `wonder >= 0`.
        if build.wonder >= 0 {
            let wonder = build.wonder;
            self.require_linked_record(who, o, wonder)?;
            self.close_wonder(who, i32::from(wonder))?;

            let request = CloseEffectsRequest { who, o, wonder };
            let effect = world.close_effects(request)?;
            validate_close_effects(request, effect)?;
            build.wonder = INVALID_SHORT;
            closed = Some(wonder);
        }

        // The retail caller performs this after clearing `BuildData::wonder`, and its close
        // argument decides whether it happens. Missing entries are intentionally a no-op.
        let unbuilt_removed =
            remove_unbuilt_entry && remove_unbuilt(&mut self.unbuilt[owner], o as i16);
        Ok(WonderLifecycleReceipt::Closed {
            who,
            o,
            wonder: closed,
            unbuilt_removed,
        })
    }

    fn capture_build<W>(
        &mut self,
        world: &mut W,
        old_o: i32,
        old_build: &mut BuildData,
        new_o: i32,
        new_build: &mut BuildData,
    ) -> Result<WonderLifecycleReceipt, WonderError>
    where
        W: WonderWorld + WonderLifecycleHost + ?Sized,
    {
        checked_build_identity(old_o, old_build)?;
        checked_build_identity(new_o, new_build)?;
        let old_who = i32::from(old_build.who);
        let new_who = i32::from(new_build.who);
        if old_who == new_who {
            return Err(WonderError::SameCaptureOwner(old_who));
        }
        let old_wonder = old_build.wonder;
        if old_wonder < 0 {
            return Err(WonderError::BuildWonderMismatch {
                who: old_who,
                o: old_o,
                wonder: old_wonder,
            });
        }
        self.require_linked_record(old_who, old_o, old_wonder)?;

        // `Build::check_capture` success tail `0x00627DFA..0x00627FBA`.
        let swap_request = CaptureSwapRequest {
            old_who,
            old_o,
            old_wonder,
            new_who,
            new_o,
        };
        validate_capture_swap(swap_request, world.capture_swap(swap_request)?)?;

        let completed = self.complete_build(world, new_o, new_build)?;
        let (new_wonder, new_unbuilt_removed, new_wonders_built) = match completed {
            WonderLifecycleReceipt::Completed {
                wonder,
                unbuilt_removed,
                wonders_built,
                ..
            } => (wonder, unbuilt_removed, wonders_built),
            _ => unreachable!("complete_build has one receipt shape"),
        };

        let closed = self.close_build(world, old_o, old_build, true)?;
        let old_unbuilt_removed = match closed {
            WonderLifecycleReceipt::Closed {
                wonder: Some(closed),
                unbuilt_removed,
                ..
            } if closed == old_wonder => unbuilt_removed,
            _ => {
                return Err(WonderError::LifecycleReceiptMismatch {
                    operation: "Wonders::capture_build/close",
                })
            }
        };

        let mask_request = CaptureMaskRequest {
            who: new_who,
            o: new_o,
            first: 1,
            second: 2,
        };
        validate_capture_mask(mask_request, world.capture_mask(mask_request)?)?;

        Ok(WonderLifecycleReceipt::Captured {
            old_who,
            old_o,
            old_wonder,
            new_who,
            new_o,
            new_wonder,
            new_unbuilt_removed,
            old_unbuilt_removed,
            new_wonders_built,
        })
    }

    fn require_linked_record(&self, who: i32, o: i32, wonder: i16) -> Result<(), WonderError> {
        let owner = checked_owner(who)?;
        let slot = usize::try_from(wonder).map_err(|_| WonderError::BuildWonderMismatch {
            who,
            o,
            wonder,
        })?;
        let Some(record) = self.lists[owner].get(slot) else {
            return Err(WonderError::BuildWonderMismatch { who, o, wonder });
        };
        if !record.is_valid()
            || record.wonder != wonder
            || record.o != o as i16
            || record.who != who as i8
        {
            return Err(WonderError::BuildWonderMismatch { who, o, wonder });
        }
        Ok(())
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
fn checked_build_identity(o: i32, build: &BuildData) -> Result<usize, WonderError> {
    if o < 0 || o > i16::MAX as i32 {
        return Err(WonderError::InvalidObject(o));
    }
    checked_owner(i32::from(build.who))
}

#[inline]
fn remove_unbuilt(list: &mut Vec<UnbuiltWonder>, o: i16) -> bool {
    let Some(index) = list.iter().position(|record| record.o == o) else {
        return false;
    };
    // `remove_unbuilt_wonder` `0x0073C220` overwrites the first match with the final
    // logical record and decrements the count.
    list.swap_remove(index);
    true
}

#[inline]
fn requires_close_recalculation(type_index: i32) -> bool {
    matches!(type_index, 0x20F | 0x212 | 0x214 | 0x21C)
}

fn validate_capture_swap(
    request: CaptureSwapRequest,
    receipt: CaptureSwapReceipt,
) -> Result<(), WonderError> {
    require_mutating_effect(
        "WonderLifecycleHost::capture_swap",
        receipt.rng_draws,
        receipt.world_writes,
    )?;
    if receipt.old_who != request.old_who
        || receipt.old_o != request.old_o
        || receipt.old_wonder != request.old_wonder
        || receipt.new_who != request.new_who
        || receipt.new_o != request.new_o
        || !receipt.swap_complete
    {
        return Err(WonderError::LifecycleReceiptMismatch {
            operation: "WonderLifecycleHost::capture_swap",
        });
    }
    Ok(())
}

fn validate_close_effects(
    request: CloseEffectsRequest,
    receipt: CloseEffectsReceipt,
) -> Result<(), WonderError> {
    require_mutating_effect(
        "WonderLifecycleHost::close_effects",
        receipt.rng_draws,
        receipt.world_writes,
    )?;
    if receipt.who != request.who
        || receipt.o != request.o
        || receipt.wonder != request.wonder
        || !(WONDER_FIRST..=WONDER_LAST).contains(&receipt.type_index)
        || !receipt.unit_stats_dirty
        || receipt.type_specific_recalculated != requires_close_recalculation(receipt.type_index)
    {
        return Err(WonderError::LifecycleReceiptMismatch {
            operation: "WonderLifecycleHost::close_effects",
        });
    }
    Ok(())
}

fn validate_capture_mask(
    request: CaptureMaskRequest,
    receipt: CaptureMaskReceipt,
) -> Result<(), WonderError> {
    require_mutating_effect(
        "WonderLifecycleHost::capture_mask",
        receipt.rng_draws,
        receipt.world_writes,
    )?;
    if receipt.who != request.who
        || receipt.o != request.o
        || receipt.first != request.first
        || receipt.second != request.second
        || !receipt.mask_complete
    {
        return Err(WonderError::LifecycleReceiptMismatch {
            operation: "WonderLifecycleHost::capture_mask",
        });
    }
    Ok(())
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

#[inline]
fn require_mutating_effect(
    operation: &'static str,
    rng_draws: u32,
    world_writes: u32,
) -> Result<(), WonderError> {
    if rng_draws != 0 || world_writes == 0 {
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
        lifecycle_calls: Vec<&'static str>,
        fail_flag: bool,
        fail_lifecycle: Option<&'static str>,
        stale_swap: bool,
        bad_close: bool,
        bad_mask: bool,
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
            self.lifecycle_calls.push("init_facts");
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
            self.lifecycle_calls.push("prerequisite");
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

    impl WonderLifecycleHost for FakeWorld {
        fn capture_swap(
            &mut self,
            request: CaptureSwapRequest,
        ) -> Result<CaptureSwapReceipt, WonderWorldError> {
            self.lifecycle_calls.push("swap");
            if self.fail_lifecycle == Some("swap") {
                return Err(WonderWorldError::from("swap refused"));
            }
            Ok(CaptureSwapReceipt {
                old_who: request.old_who,
                old_o: request.old_o,
                old_wonder: request.old_wonder,
                new_who: request.new_who,
                new_o: if self.stale_swap {
                    request.new_o.wrapping_add(1)
                } else {
                    request.new_o
                },
                swap_complete: true,
                rng_draws: 0,
                world_writes: 1,
            })
        }

        fn close_effects(
            &mut self,
            request: CloseEffectsRequest,
        ) -> Result<CloseEffectsReceipt, WonderWorldError> {
            self.lifecycle_calls.push("close_effects");
            if self.fail_lifecycle == Some("close") {
                return Err(WonderWorldError::from("close effects refused"));
            }
            let type_index = self
                .facts
                .get(&(request.who, request.o))
                .map(|facts| facts.type_index)
                .unwrap_or(WONDER_FIRST);
            Ok(CloseEffectsReceipt {
                who: request.who,
                o: request.o,
                wonder: request.wonder,
                type_index,
                unit_stats_dirty: !self.bad_close,
                type_specific_recalculated: requires_close_recalculation(type_index),
                rng_draws: 0,
                world_writes: 1,
            })
        }

        fn capture_mask(
            &mut self,
            request: CaptureMaskRequest,
        ) -> Result<CaptureMaskReceipt, WonderWorldError> {
            self.lifecycle_calls.push("mask");
            if self.fail_lifecycle == Some("mask") {
                return Err(WonderWorldError::from("mask refused"));
            }
            Ok(CaptureMaskReceipt {
                who: request.who,
                o: request.o,
                first: request.first,
                second: request.second,
                mask_complete: !self.bad_mask,
                rng_draws: 0,
                world_writes: 1,
            })
        }
    }

    fn build(who: u8, wonder: i16) -> BuildData {
        BuildData {
            who,
            wonder,
            ..BuildData::default()
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
    fn completion_swap_removes_unbuilt_then_wraps_built_and_stores_slot() {
        let mut registry = Wonders::new();
        registry.add_unbuilt_wonder(1, 10).unwrap();
        registry.add_unbuilt_wonder(1, 20).unwrap();
        registry.add_unbuilt_wonder(1, 30).unwrap();
        registry.wonders_built[1] = i32::MAX;
        let mut world = FakeWorld::default();
        world.add(1, 20, WONDER_FIRST, 4);
        let mut completed = build(1, INVALID_SHORT);

        let receipt = registry
            .apply_build_lifecycle(
                &mut world,
                WonderLifecycle::Complete {
                    o: 20,
                    build: &mut completed,
                },
            )
            .unwrap();

        assert_eq!(
            receipt,
            WonderLifecycleReceipt::Completed {
                who: 1,
                o: 20,
                wonder: 0,
                unbuilt_removed: true,
                wonders_built: i32::MIN,
            }
        );
        assert_eq!(completed.wonder, 0);
        assert_eq!(registry.wonders_built(1), i32::MIN);
        assert_eq!(
            registry.unbuilt(1),
            &[
                UnbuiltWonder { o: 10, who: 1 },
                UnbuiltWonder { o: 30, who: 1 },
            ],
            "retail overwrites the first match with the final logical entry"
        );
    }

    #[test]
    fn completion_missing_unbuilt_is_a_retail_exact_noop() {
        let mut registry = Wonders::new();
        let mut world = FakeWorld::default();
        world.add(3, 44, WONDER_LAST, 8);
        let mut completed = build(3, 77);

        let receipt = registry
            .apply_build_lifecycle(
                &mut world,
                WonderLifecycle::Complete {
                    o: 44,
                    build: &mut completed,
                },
            )
            .unwrap();
        assert!(matches!(
            receipt,
            WonderLifecycleReceipt::Completed {
                wonder: 0,
                unbuilt_removed: false,
                wonders_built: 1,
                ..
            }
        ));
        assert_eq!(completed.wonder, 0, "activation overwrites the old link");
    }

    #[test]
    fn failed_completion_rolls_back_unbuilt_counter_and_build_link() {
        let mut registry = Wonders::new();
        registry.add_unbuilt_wonder(0, 4).unwrap();
        let before = registry.clone();
        let mut world = FakeWorld::default();
        world.add(0, 4, WONDER_FIRST, 1);
        world.fail_flag = true;
        let mut completed = build(0, INVALID_SHORT);

        assert!(matches!(
            registry.apply_build_lifecycle(
                &mut world,
                WonderLifecycle::Complete {
                    o: 4,
                    build: &mut completed,
                },
            ),
            Err(WonderError::World(_))
        ));
        assert_eq!(registry, before);
        assert_eq!(completed.wonder, INVALID_SHORT);
    }

    #[test]
    fn close_requires_dirty_and_type_recalculation_receipt_before_commit() {
        let mut registry = Wonders::new();
        let mut world = FakeWorld::default();
        world.add(2, 17, 0x20F, 4);
        let mut completed = build(2, INVALID_SHORT);
        registry
            .apply_build_lifecycle(
                &mut world,
                WonderLifecycle::Complete {
                    o: 17,
                    build: &mut completed,
                },
            )
            .unwrap();
        world.lifecycle_calls.clear();

        let receipt = registry
            .apply_build_lifecycle(
                &mut world,
                WonderLifecycle::Close {
                    o: 17,
                    build: &mut completed,
                    remove_unbuilt: true,
                },
            )
            .unwrap();
        assert_eq!(
            receipt,
            WonderLifecycleReceipt::Closed {
                who: 2,
                o: 17,
                wonder: Some(0),
                unbuilt_removed: false,
            }
        );
        assert_eq!(world.lifecycle_calls, ["close_effects"]);
        assert_eq!(completed.wonder, INVALID_SHORT);
        assert!(!registry.record(2, 0).unwrap().is_valid());
        assert_eq!(registry.wonders_built(2), 1);
        assert_eq!(registry.wonders_held(2), 1);
    }

    #[test]
    fn malformed_close_receipt_rolls_back_registry_and_build_link() {
        let mut registry = Wonders::new();
        let mut world = FakeWorld::default();
        world.add(0, 9, WONDER_FIRST, 2);
        let mut completed = build(0, INVALID_SHORT);
        registry
            .apply_build_lifecycle(
                &mut world,
                WonderLifecycle::Complete {
                    o: 9,
                    build: &mut completed,
                },
            )
            .unwrap();
        let before = registry.clone();
        world.bad_close = true;

        assert_eq!(
            registry.apply_build_lifecycle(
                &mut world,
                WonderLifecycle::Close {
                    o: 9,
                    build: &mut completed,
                    remove_unbuilt: true,
                },
            ),
            Err(WonderError::LifecycleReceiptMismatch {
                operation: "WonderLifecycleHost::close_effects",
            })
        );
        assert_eq!(registry, before);
        assert_eq!(completed.wonder, 0);
    }

    #[test]
    fn close_argument_controls_unbuilt_removal_even_without_completed_link() {
        let mut registry = Wonders::new();
        registry.add_unbuilt_wonder(4, 33).unwrap();
        let mut world = FakeWorld::default();
        let mut unfinished = build(4, INVALID_SHORT);

        let kept = registry
            .apply_build_lifecycle(
                &mut world,
                WonderLifecycle::Close {
                    o: 33,
                    build: &mut unfinished,
                    remove_unbuilt: false,
                },
            )
            .unwrap();
        assert!(matches!(
            kept,
            WonderLifecycleReceipt::Closed {
                wonder: None,
                unbuilt_removed: false,
                ..
            }
        ));
        assert_eq!(registry.unbuilt(4).len(), 1);
        assert!(world.lifecycle_calls.is_empty());

        let removed = registry
            .apply_build_lifecycle(
                &mut world,
                WonderLifecycle::Close {
                    o: 33,
                    build: &mut unfinished,
                    remove_unbuilt: true,
                },
            )
            .unwrap();
        assert!(matches!(
            removed,
            WonderLifecycleReceipt::Closed {
                wonder: None,
                unbuilt_removed: true,
                ..
            }
        ));
        assert!(registry.unbuilt(4).is_empty());
        assert!(world.lifecycle_calls.is_empty());
    }

    #[test]
    fn capture_registers_new_owner_then_closes_old_owner_in_retail_order() {
        let mut registry = Wonders::new();
        let mut world = FakeWorld::default();
        world.add(0, 10, WONDER_FIRST, 4);
        world.add(1, 21, WONDER_FIRST + 2, 4);
        let mut old_build = build(0, INVALID_SHORT);
        registry
            .apply_build_lifecycle(
                &mut world,
                WonderLifecycle::Complete {
                    o: 10,
                    build: &mut old_build,
                },
            )
            .unwrap();
        registry.add_unbuilt_wonder(1, 21).unwrap();
        let mut new_build = build(1, old_build.wonder);
        world.lifecycle_calls.clear();

        let receipt = registry
            .apply_build_lifecycle(
                &mut world,
                WonderLifecycle::Capture {
                    old_o: 10,
                    old_build: &mut old_build,
                    new_o: 21,
                    new_build: &mut new_build,
                },
            )
            .unwrap();

        assert_eq!(
            receipt,
            WonderLifecycleReceipt::Captured {
                old_who: 0,
                old_o: 10,
                old_wonder: 0,
                new_who: 1,
                new_o: 21,
                new_wonder: 0,
                new_unbuilt_removed: true,
                old_unbuilt_removed: false,
                new_wonders_built: 1,
            }
        );
        assert_eq!(
            world.lifecycle_calls,
            [
                "swap",
                "init_facts",
                "prerequisite",
                "close_effects",
                "mask"
            ]
        );
        assert_eq!(old_build.wonder, INVALID_SHORT);
        assert_eq!(new_build.wonder, 0);
        assert!(!registry.record(0, 0).unwrap().is_valid());
        assert!(registry.record(1, 0).unwrap().is_valid());
        assert_eq!(registry.wonders_built(0), 1);
        assert_eq!(registry.wonders_built(1), 1);
        assert_eq!(registry.wonders_held(0), 1);
        assert_eq!(registry.wonders_held(1), 1);
    }

    #[test]
    fn stale_swap_or_failed_final_mask_never_commits_partial_local_capture() {
        let mut registry = Wonders::new();
        let mut world = FakeWorld::default();
        world.add(0, 10, WONDER_FIRST, 4);
        world.add(1, 21, WONDER_LAST, 4);
        let mut old_build = build(0, INVALID_SHORT);
        registry
            .apply_build_lifecycle(
                &mut world,
                WonderLifecycle::Complete {
                    o: 10,
                    build: &mut old_build,
                },
            )
            .unwrap();
        let mut new_build = build(1, old_build.wonder);
        let before = registry.clone();

        world.stale_swap = true;
        assert!(matches!(
            registry.apply_build_lifecycle(
                &mut world,
                WonderLifecycle::Capture {
                    old_o: 10,
                    old_build: &mut old_build,
                    new_o: 21,
                    new_build: &mut new_build,
                },
            ),
            Err(WonderError::LifecycleReceiptMismatch {
                operation: "WonderLifecycleHost::capture_swap"
            })
        ));
        assert_eq!(registry, before);
        assert_eq!(old_build.wonder, 0);
        assert_eq!(new_build.wonder, 0);

        world.stale_swap = false;
        world.bad_mask = true;
        assert!(matches!(
            registry.apply_build_lifecycle(
                &mut world,
                WonderLifecycle::Capture {
                    old_o: 10,
                    old_build: &mut old_build,
                    new_o: 21,
                    new_build: &mut new_build,
                },
            ),
            Err(WonderError::LifecycleReceiptMismatch {
                operation: "WonderLifecycleHost::capture_mask"
            })
        ));
        assert_eq!(registry, before);
        assert_eq!(old_build.wonder, 0);
        assert_eq!(new_build.wonder, 0);
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
