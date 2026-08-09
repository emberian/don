//! Exact deterministic body of retail `DeathObj::inc_time` (`0x008D5240`, 540 bytes).
//!
//! The corpse record is checksummed, but three things touched by the function live outside
//! it: the dead source object's `hold` word, the terrain blocking map, and presentation-only
//! bleed state.  [`DeathIncTimeWorld`] makes those dependencies explicit and requires them to
//! be prepared before the first retail mutation.  Once preparation succeeds, its commit
//! methods are deliberately infallible: returning an error after `hold++` would expose a
//! half-applied step that retail cannot produce.
//!
//! This module is intentionally not wired by its archaeology lane.  The exact frozen
//! integration hunk and the remaining `clear_blocking` admission requirement are recorded in
//! `docs/mechanics/death-inctime.md`.

use crate::systems::combat::DeathRecord;

/// Preferred VA and measured PDB size of `DeathObj::inc_time`.
pub const DEATH_INC_TIME_VA: u32 = 0x008D_5240;
pub const DEATH_INC_TIME_SIZE: u32 = 540;
/// `DeathObj::clear_blocking`, an external simulation mutation owned by the live adapter.
pub const DEATH_CLEAR_BLOCKING_VA: u32 = 0x008D_4AC0;
/// `DeathObjOut::inc_bleed`, presentation state outside `DeathObjData::walk_data`.
pub const DEATH_INC_BLEED_VA: u32 = 0x008D_3B90;
/// `AnimationPacket::get_game_frames`.
pub const GET_GAME_FRAMES_VA: u32 = 0x0091_8CC0;
/// `UnitTypeData::blocks_while_dead`, virtual slot `+0x120`.
pub const BLOCKS_WHILE_DEAD_VA: u32 = 0x0047_0440;
/// Global pointer `MiscAccess::scene`; the body writes `Scene::recalc_deaths` through it.
pub const MISC_ACCESS_SCENE_PTR_VA: u32 = 0x00C0_620C;
/// PDB offset of `Scene::recalc_deaths` (`unsigned char`).
pub const SCENE_RECALC_DEATHS_OFFSET: u32 = 0x22F;

/// Retail global at `0x00B2176C`, used for an ordinary ground corpse.
pub const ORDINARY_CORPSE_PADDING_VA: u32 = 0x00B2_176C;
pub const ORDINARY_CORPSE_PADDING: i32 = 135;
/// Retail global at `0x00B21774`, used whenever `skel_gpiece != -1`.
pub const SKELETON_CORPSE_PADDING_VA: u32 = 0x00B2_1774;
pub const SKELETON_CORPSE_PADDING: i32 = 627;
/// A live blocking corpse forces the dead source object's hold to at least this value.
pub const BLOCKING_CORPSE_MIN_HOLD: u16 = 30;

/// Result of resolving `DeathObjData::gpiece` through the live graphics registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeathGraphicState {
    /// The gpiece lookup itself returned null.  Retail invalidates and returns before
    /// advancing `cur_frame` or calling `inc_bleed`.
    MissingGraphicPiece,
    /// The graphic piece exists but its `AnimationPacket` at `+0x54` is null.  Retail marks
    /// the corpse invalid, uses duration zero, and continues through the rest of the body.
    MissingAnimationPacket,
    /// Exact result of `AnimationPacket::get_game_frames(cur_anim)`.
    Loaded { game_frames: i32 },
}

/// Authoritative, preflighted facts read by one corpse tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeathIncTimeFacts {
    pub graphic: DeathGraphicState,
    /// `ObjectTypeData::domain` at type offset `+0x218`.  Domain 1 has no ordinary-corpse
    /// tail padding; a skeleton graphic overrides this test.
    pub domain: i32,
    /// Result of virtual `UnitTypeData::blocks_while_dead` (`vtable +0x120`).
    pub blocks_while_dead: bool,
}

/// Observable result of the deterministic body, including repeated retail stores/calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeathIncTimeReceipt {
    pub valid_before: i32,
    pub valid_after: i32,
    pub cur_frame_before: i32,
    pub cur_frame_after: i32,
    pub hold_before: u16,
    pub hold_after: u16,
    /// `None` only for the early missing-gpiece return; a missing packet records `Some(0)`.
    pub animation_frames: Option<i32>,
    pub expiry_threshold: Option<i32>,
    pub missing_animation_packet: bool,
    pub expired_at_threshold: bool,
    /// Number of byte stores to `Scene::recalc_deaths` at `+0x22F`.  The missing-packet
    /// path can store twice.
    pub dirty_writes: u8,
    pub clear_blocking_calls: u8,
    pub bleed_calls: u8,
}

/// Transaction boundary for the state that `DeathRecord` does not own.
///
/// `prepare` must resolve the source object `(who,o)`, its type, the graphic packet, and every
/// resource required by a possible `clear_blocking` call.  It is the only fallible method and
/// therefore runs before `hold++`.  `Prepared` should retain stable object/type/terrain
/// identities; it must not borrow `self`, because the commit calls need mutable world access.
pub trait DeathIncTimeWorld {
    type Prepared;
    type Error;

    fn prepare(
        &mut self,
        death: &DeathRecord,
    ) -> Result<(Self::Prepared, DeathIncTimeFacts), Self::Error>;

    fn source_hold(&self, prepared: &Self::Prepared) -> u16;
    fn write_source_hold(&mut self, prepared: &mut Self::Prepared, value: u16);
    fn mark_recalc_deaths(&mut self, prepared: &mut Self::Prepared);
    fn clear_blocking(&mut self, prepared: &mut Self::Prepared, death: &DeathRecord);
    fn inc_bleed(
        &mut self,
        prepared: &mut Self::Prepared,
        death: &DeathRecord,
        animation_frames: i32,
    );
}

/// Advance one valid corpse using the exact retail ordering and 32/16-bit wrap semantics.
///
/// The caller owns the valid-slot loop from `Objects::inc_time` (`0x0065DB70`).  Retail calls
/// this body only when `death.valid != 0`; this function intentionally does not invent a
/// second validity gate.
pub fn death_inc_time<W: DeathIncTimeWorld>(
    death: &mut DeathRecord,
    world: &mut W,
) -> Result<DeathIncTimeReceipt, W::Error> {
    // All potentially missing live state is resolved before the first retail write.
    let (mut prepared, facts) = world.prepare(death)?;

    let valid_before = death.valid;
    let cur_frame_before = death.cur_frame;
    let hold_before = world.source_hold(&prepared);

    // 0x008D528D: `incw ObjectData+0x32`, including u16 wrap.
    world.write_source_hold(&mut prepared, hold_before.wrapping_add(1));

    let mut dirty_writes = 0u8;
    let mut clear_blocking_calls = 0u8;

    let (animation_frames, missing_animation_packet) = match facts.graphic {
        DeathGraphicState::MissingGraphicPiece => {
            // 0x008D529A..52CE.  This arm returns without `cur_frame++` or `inc_bleed`.
            death.valid = 0;
            world.mark_recalc_deaths(&mut prepared);
            dirty_writes += 1;
            if facts.blocks_while_dead {
                world.clear_blocking(&mut prepared, death);
                clear_blocking_calls += 1;
            }
            return Ok(DeathIncTimeReceipt {
                valid_before,
                valid_after: death.valid,
                cur_frame_before,
                cur_frame_after: death.cur_frame,
                hold_before,
                hold_after: world.source_hold(&prepared),
                animation_frames: None,
                expiry_threshold: None,
                missing_animation_packet: false,
                expired_at_threshold: false,
                dirty_writes,
                clear_blocking_calls,
                bleed_calls: 0,
            });
        }
        DeathGraphicState::MissingAnimationPacket => {
            // 0x008D5306..5382: invalidation precedes the duration-zero continuation.
            death.valid = 0;
            world.mark_recalc_deaths(&mut prepared);
            dirty_writes += 1;
            (0, true)
        }
        DeathGraphicState::Loaded { game_frames } => (game_frames, false),
    };

    // 0x008D5384.  Both the clock and all threshold additions are signed i32 operations.
    death.cur_frame = death.cur_frame.wrapping_add(1);
    let padding = if death.skel_gpiece != -1 {
        SKELETON_CORPSE_PADDING
    } else if facts.domain == 1 {
        0
    } else {
        ORDINARY_CORPSE_PADDING
    };
    let expiry_threshold = animation_frames.wrapping_add(padding);

    // 0x008D53CD is signed `jl`: equality expires.  Do not merge this dirty store with the
    // missing-packet store above; retail can perform both in one call.
    let expired_at_threshold = death.cur_frame >= expiry_threshold;
    if expired_at_threshold {
        death.valid = 0;
        world.mark_recalc_deaths(&mut prepared);
        dirty_writes += 1;
    }

    if facts.blocks_while_dead {
        if death.valid == 0 {
            // 0x008D540B.  The adapter must apply the full collision/terrain mutation.
            world.clear_blocking(&mut prepared, death);
            clear_blocking_calls += 1;
        } else {
            // 0x008D5434 reloads hold after the first write and 0x008D543F stores even when
            // it was already >= 30.  Keeping both writes pins the retail call ordering.
            let current_hold = world.source_hold(&prepared);
            world.write_source_hold(&mut prepared, current_hold.max(BLOCKING_CORPSE_MIN_HOLD));
        }
    }

    // 0x008D5446.  DeathObjOut is not walked by the deaths checksum, but presentation must
    // still receive the exact duration and call order.
    world.inc_bleed(&mut prepared, death, animation_frames);

    Ok(DeathIncTimeReceipt {
        valid_before,
        valid_after: death.valid,
        cur_frame_before,
        cur_frame_after: death.cur_frame,
        hold_before,
        hold_after: world.source_hold(&prepared),
        animation_frames: Some(animation_frames),
        expiry_threshold: Some(expiry_threshold),
        missing_animation_packet,
        expired_at_threshold,
        dirty_writes,
        clear_blocking_calls,
        bleed_calls: 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Event {
        Hold(u16),
        Dirty,
        ClearBlocking,
        Bleed(i32),
    }

    struct ProbeWorld {
        facts: DeathIncTimeFacts,
        hold: u16,
        reject: bool,
        events: Vec<Event>,
    }

    impl ProbeWorld {
        fn new(facts: DeathIncTimeFacts, hold: u16) -> Self {
            Self {
                facts,
                hold,
                reject: false,
                events: Vec::new(),
            }
        }
    }

    impl DeathIncTimeWorld for ProbeWorld {
        type Prepared = (i32, i32);
        type Error = &'static str;

        fn prepare(
            &mut self,
            death: &DeathRecord,
        ) -> Result<(Self::Prepared, DeathIncTimeFacts), Self::Error> {
            if self.reject {
                return Err("missing live death facts");
            }
            Ok(((death.who, death.o), self.facts))
        }

        fn source_hold(&self, _prepared: &Self::Prepared) -> u16 {
            self.hold
        }

        fn write_source_hold(&mut self, _prepared: &mut Self::Prepared, value: u16) {
            self.hold = value;
            self.events.push(Event::Hold(value));
        }

        fn mark_recalc_deaths(&mut self, _prepared: &mut Self::Prepared) {
            self.events.push(Event::Dirty);
        }

        fn clear_blocking(&mut self, _prepared: &mut Self::Prepared, _death: &DeathRecord) {
            self.events.push(Event::ClearBlocking);
        }

        fn inc_bleed(
            &mut self,
            _prepared: &mut Self::Prepared,
            _death: &DeathRecord,
            animation_frames: i32,
        ) {
            self.events.push(Event::Bleed(animation_frames));
        }
    }

    fn corpse(cur_frame: i32) -> DeathRecord {
        DeathRecord {
            valid: 1,
            who: 2,
            o: 41,
            gpiece: 7,
            cur_frame,
            skel_gpiece: -1,
            ..DeathRecord::default()
        }
    }

    fn facts(graphic: DeathGraphicState, domain: i32, blocks: bool) -> DeathIncTimeFacts {
        DeathIncTimeFacts {
            graphic,
            domain,
            blocks_while_dead: blocks,
        }
    }

    #[test]
    fn retail_addresses_and_mutation_constants_are_frozen() {
        assert_eq!(DEATH_INC_TIME_VA, 0x008D_5240);
        assert_eq!(DEATH_INC_TIME_SIZE, 540);
        assert_eq!(DEATH_INC_TIME_VA + DEATH_INC_TIME_SIZE, 0x008D_545C);
        assert_eq!(DEATH_CLEAR_BLOCKING_VA, 0x008D_4AC0);
        assert_eq!(DEATH_INC_BLEED_VA, 0x008D_3B90);
        assert_eq!(GET_GAME_FRAMES_VA, 0x0091_8CC0);
        assert_eq!(BLOCKS_WHILE_DEAD_VA, 0x0047_0440);
        assert_eq!(MISC_ACCESS_SCENE_PTR_VA, 0x00C0_620C);
        assert_eq!(SCENE_RECALC_DEATHS_OFFSET, 0x22F);
        assert_eq!(ORDINARY_CORPSE_PADDING_VA, 0x00B2_176C);
        assert_eq!(ORDINARY_CORPSE_PADDING, 0x87);
        assert_eq!(SKELETON_CORPSE_PADDING_VA, 0x00B2_1774);
        assert_eq!(SKELETON_CORPSE_PADDING, 0x273);
        assert_eq!(BLOCKING_CORPSE_MIN_HOLD, 0x1E);
    }

    #[test]
    fn missing_gpiece_returns_before_clock_and_bleed_but_after_hold_and_clear() {
        let mut death = corpse(91);
        let mut world = ProbeWorld::new(facts(DeathGraphicState::MissingGraphicPiece, 0, true), 4);
        let receipt = death_inc_time(&mut death, &mut world).unwrap();

        assert_eq!(
            world.events,
            [Event::Hold(5), Event::Dirty, Event::ClearBlocking]
        );
        assert_eq!(death.valid, 0);
        assert_eq!(death.cur_frame, 91);
        assert_eq!(receipt.animation_frames, None);
        assert_eq!(receipt.expiry_threshold, None);
        assert_eq!(receipt.bleed_calls, 0);
    }

    #[test]
    fn missing_nonblocking_gpiece_does_not_invent_a_clear_call() {
        let mut death = corpse(91);
        let mut world = ProbeWorld::new(facts(DeathGraphicState::MissingGraphicPiece, 0, false), 4);
        let receipt = death_inc_time(&mut death, &mut world).unwrap();

        assert_eq!(world.events, [Event::Hold(5), Event::Dirty]);
        assert_eq!(receipt.clear_blocking_calls, 0);
    }

    #[test]
    fn missing_packet_continues_with_zero_and_can_write_dirty_twice() {
        let mut death = corpse(-1);
        let mut world =
            ProbeWorld::new(facts(DeathGraphicState::MissingAnimationPacket, 1, true), 7);
        let receipt = death_inc_time(&mut death, &mut world).unwrap();

        assert_eq!(
            world.events,
            [
                Event::Hold(8),
                Event::Dirty,
                Event::Dirty,
                Event::ClearBlocking,
                Event::Bleed(0),
            ]
        );
        assert_eq!(death.cur_frame, 0);
        assert_eq!(receipt.expiry_threshold, Some(0));
        assert!(receipt.missing_animation_packet);
        assert!(receipt.expired_at_threshold);
        assert_eq!(receipt.dirty_writes, 2);
    }

    #[test]
    fn ordinary_corpse_expires_on_equality_not_one_frame_later() {
        let loaded = DeathGraphicState::Loaded { game_frames: 10 };

        let mut before = corpse(143);
        let mut before_world = ProbeWorld::new(facts(loaded, 0, false), 0);
        let before_receipt = death_inc_time(&mut before, &mut before_world).unwrap();
        assert_eq!(before.cur_frame, 144);
        assert_eq!(before.valid, 1);
        assert!(!before_receipt.expired_at_threshold);
        assert_eq!(before_world.events, [Event::Hold(1), Event::Bleed(10)]);

        let mut at = corpse(144);
        let mut at_world = ProbeWorld::new(facts(loaded, 0, false), 0);
        let at_receipt = death_inc_time(&mut at, &mut at_world).unwrap();
        assert_eq!(at_receipt.expiry_threshold, Some(145));
        assert_eq!(at.cur_frame, 145);
        assert_eq!(at.valid, 0);
        assert!(at_receipt.expired_at_threshold);
        assert_eq!(
            at_world.events,
            [Event::Hold(1), Event::Dirty, Event::Bleed(10)]
        );
    }

    #[test]
    fn domain_one_removes_the_ordinary_tail_padding() {
        let loaded = DeathGraphicState::Loaded { game_frames: 10 };
        let mut death = corpse(9);
        let mut world = ProbeWorld::new(facts(loaded, 1, false), 12);
        let receipt = death_inc_time(&mut death, &mut world).unwrap();

        assert_eq!(receipt.expiry_threshold, Some(10));
        assert!(receipt.expired_at_threshold);
        assert_eq!(death.valid, 0);
    }

    #[test]
    fn skeleton_padding_overrides_domain_one() {
        let mut death = corpse(635);
        death.skel_gpiece = 99;
        let mut world = ProbeWorld::new(
            facts(DeathGraphicState::Loaded { game_frames: 10 }, 1, false),
            2,
        );
        let receipt = death_inc_time(&mut death, &mut world).unwrap();

        assert_eq!(receipt.expiry_threshold, Some(637));
        assert_eq!(death.cur_frame, 636);
        assert_eq!(death.valid, 1);
    }

    #[test]
    fn a_live_blocking_corpse_performs_both_hold_stores_before_bleed() {
        let mut death = corpse(0);
        let mut world = ProbeWorld::new(
            facts(DeathGraphicState::Loaded { game_frames: 100 }, 0, true),
            5,
        );
        let receipt = death_inc_time(&mut death, &mut world).unwrap();

        assert_eq!(
            world.events,
            [Event::Hold(6), Event::Hold(30), Event::Bleed(100)]
        );
        assert_eq!(receipt.hold_before, 5);
        assert_eq!(receipt.hold_after, 30);
        assert_eq!(death.valid, 1);
    }

    #[test]
    fn the_second_blocking_hold_store_is_not_elided_above_the_floor() {
        let mut death = corpse(0);
        let mut world = ProbeWorld::new(
            facts(DeathGraphicState::Loaded { game_frames: 100 }, 0, true),
            40,
        );
        death_inc_time(&mut death, &mut world).unwrap();
        assert_eq!(
            world.events,
            [Event::Hold(41), Event::Hold(41), Event::Bleed(100)]
        );
    }

    #[test]
    fn source_hold_and_frame_use_native_retail_wrap() {
        let mut death = corpse(i32::MAX);
        let mut world = ProbeWorld::new(
            facts(DeathGraphicState::Loaded { game_frames: 3 }, 0, false),
            u16::MAX,
        );
        let receipt = death_inc_time(&mut death, &mut world).unwrap();

        assert_eq!(death.cur_frame, i32::MIN);
        assert_eq!(receipt.hold_after, 0);
        assert_eq!(world.events, [Event::Hold(0), Event::Bleed(3)]);
    }

    #[test]
    fn threshold_addition_wraps_before_the_signed_compare() {
        let mut death = corpse(0);
        let mut world = ProbeWorld::new(
            facts(
                DeathGraphicState::Loaded {
                    game_frames: i32::MAX,
                },
                0,
                false,
            ),
            0,
        );
        let receipt = death_inc_time(&mut death, &mut world).unwrap();

        assert_eq!(receipt.expiry_threshold, Some(i32::MIN + 134));
        assert!(receipt.expired_at_threshold);
        assert_eq!(death.valid, 0);
    }

    #[test]
    fn failed_preflight_is_atomic_and_does_not_increment_hold() {
        let mut death = corpse(17);
        let before = death;
        let mut world = ProbeWorld::new(
            facts(DeathGraphicState::Loaded { game_frames: 3 }, 0, true),
            22,
        );
        world.reject = true;

        assert_eq!(
            death_inc_time(&mut death, &mut world),
            Err("missing live death facts")
        );
        assert_eq!(death, before);
        assert_eq!(world.hold, 22);
        assert!(world.events.is_empty());
    }
}
