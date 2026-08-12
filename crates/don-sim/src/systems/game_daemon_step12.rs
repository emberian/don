//! Exact deterministic shell of `GameDaemon::process_all` `0x00732700` (tick step 12).
//!
//! The 315-byte retail function is not merely a bag of subsystem calls. It owns three
//! checksum-visible pieces of `GameDaemon` state, decays the eight per-player repath
//! counters, schedules danger and fog on different frame phases, rolls two `Region::flags`
//! bits over in a fixed 64-record pass, and then finishes with borders, collision-block
//! collection, and group maintenance. This module keeps that ordering in one executable
//! adapter while leaving the large child bodies behind a typed host.
//!
//! Provenance is `ron-bin/riseofnations.exe` plus `ron-bin/sbl/rise.pdb`:
//!
//! - `GameDaemon` is 44 bytes: `repaths[8]` at `+0x00`, `empty_colls` at `+0x20`,
//!   `borders` at `+0x24`, and `busy` at `+0x28` (`schema/pdb-types.json`).
//! - `0x00732711..0x007327A1` decrements non-zero `busy`, then replaces each repath
//!   counter by `counter / 2`, except results below three become zero.
//! - `0x007327A4..0x007327E6` calls victory unconditionally, danger when `frame % 200 == 0`,
//!   fog when `frame % 100 == 33`, then markets unconditionally.
//! - `0x007327EB..0x00732820` walks exactly 64 `Region` records at stride `0x88`: clear
//!   `0x10`, then promote pending bit `0x20` to current bit `0x10`.
//! - `0x00732822..0x00732830` calls borders, collision-block collection, and groups in
//!   that order.
//!
//! This adapter is complete for the top-level function on its successful path. It is not a
//! claim that every child is complete: notably `calc_danger` `0x00732D10` still needs a live
//! object/type/world host. [`GameDaemonProcessAllHost::preflight`] exists so an integration
//! can refuse the whole step before the shell mutates state rather than silently skipping a
//! reached child.

use super::borders_fog::RegionBorderState;

pub const RETAIL_VA: u32 = 0x0073_2700;
pub const RETAIL_SIZE: usize = 315;
pub const LEADER_SLOTS: usize = 8;
pub const REGION_SLOTS: usize = 64;

/// The PDB-exact 44-byte logical state of `GameDaemon`.
///
/// Rust layout is intentionally not asserted to be ABI-compatible; these fields reproduce
/// the named retail values and offsets, not a pointer-cast boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GameDaemonState {
    /// `GameDaemon +0x00`, one collision-repath budget per leader.
    pub repaths: [i32; LEADER_SLOTS],
    /// `GameDaemon +0x20`, persistent `WData` cursor used by `process_coll_blocks`.
    pub empty_colls: i32,
    /// `GameDaemon +0x24`, per-frame territory-cell budget/counter.
    pub borders: i32,
    /// `GameDaemon +0x28`, a countdown decremented at the start of this pass.
    pub busy: i32,
}

/// Child calls made by `GameDaemon::process_all`, excluding its inlined state mutations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameDaemonCall {
    ProcessVictory,
    CalcDanger,
    UpdateAllSeen,
    CalcMarkets,
    CheckBorders,
    ProcessCollBlocks,
    GroupsProcess,
}

/// Allocation-free call schedule for one retail frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallSchedule {
    calls: [GameDaemonCall; 7],
    len: u8,
}

impl CallSchedule {
    /// Freeze the two signed `idiv` gates used at `0x007327BA` and `0x007327D8`.
    pub fn for_frame(frame: i32) -> Self {
        let mut out = Self {
            calls: [GameDaemonCall::ProcessVictory; 7],
            len: 0,
        };
        out.push(GameDaemonCall::ProcessVictory);
        if frame % 200 == 0 {
            out.push(GameDaemonCall::CalcDanger);
        }
        if frame % 100 == 33 {
            out.push(GameDaemonCall::UpdateAllSeen);
        }
        out.push(GameDaemonCall::CalcMarkets);
        out.push(GameDaemonCall::CheckBorders);
        out.push(GameDaemonCall::ProcessCollBlocks);
        out.push(GameDaemonCall::GroupsProcess);
        out
    }

    #[inline]
    fn push(&mut self, call: GameDaemonCall) {
        self.calls[self.len as usize] = call;
        self.len += 1;
    }

    #[inline]
    pub fn as_slice(&self) -> &[GameDaemonCall] {
        &self.calls[..self.len as usize]
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn contains(&self, call: GameDaemonCall) -> bool {
        self.as_slice().contains(&call)
    }
}

/// Typed boundary around the seven child bodies reached from the step-12 shell.
///
/// `preflight` must validate every child named by `schedule`, including all live state the
/// child will require. After it returns `Ok`, the methods are deliberately infallible: retail
/// has no rollback path, and allowing a late "unsupported" result would leave a prefix of the
/// frame committed. The host may still record its own deterministic work counters.
pub trait GameDaemonProcessAllHost {
    type Fault;

    fn preflight(&self, schedule: &CallSchedule) -> Result<(), Self::Fault>;
    fn process_victory(&mut self);
    fn calc_danger(&mut self);
    /// Execute `GameDaemon::update_all_seen` against the daemon-owned `busy` field.  The
    /// callback receives the field because the exact fog-option-three return must leave it
    /// untouched, while every entered producer path stores four before clearing a World plane.
    fn update_all_seen(&mut self, busy: &mut i32);
    fn calc_markets(&mut self);

    /// Run `GameDaemon::check_borders` `0x00732060` and return the final value of
    /// `GameDaemon::borders` (`+0x24`). The shell resets that field to zero immediately
    /// before this call, matching the child prologue.
    fn check_borders(&mut self, regions: &mut [RegionBorderState]) -> i32;

    /// Run `GameDaemon::process_coll_blocks` `0x00731F90` against the persistent
    /// `GameDaemon::empty_colls` (`+0x20`) cursor.
    fn process_coll_blocks(&mut self, empty_colls: &mut i32);
    fn groups_process(&mut self);
}

/// A fail-closed structural error. Neither form mutates daemon or region state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessAllError<E> {
    RegionCardinality { expected: usize, actual: usize },
    Host(E),
}

/// Deterministic evidence emitted by one successful step-12 shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessAllTrace {
    pub frame: i32,
    pub schedule: CallSchedule,
    pub daemon_before: GameDaemonState,
    pub daemon_after: GameDaemonState,
    /// Region records whose old current bit `0x10` was cleared.
    pub region_current_cleared: u8,
    /// Region records whose pending bit `0x20` was promoted to current bit `0x10`.
    pub region_pending_promoted: u8,
}

/// `GameDaemon::process_all` `0x00732700`, including every mutation local to the 315-byte
/// body and every child dispatch in retail order.
///
/// The adapter rejects anything other than the retail 64-region lattice. The existing
/// reduced one-region test map therefore needs an explicit 64-slot live adapter; silently
/// iterating only the populated prefix would not reproduce `0x007327EB..0x00732820`.
pub fn process_all<H: GameDaemonProcessAllHost>(
    daemon: &mut GameDaemonState,
    frame: i32,
    regions: &mut [RegionBorderState],
    host: &mut H,
) -> Result<ProcessAllTrace, ProcessAllError<H::Fault>> {
    if regions.len() != REGION_SLOTS {
        return Err(ProcessAllError::RegionCardinality {
            expected: REGION_SLOTS,
            actual: regions.len(),
        });
    }

    let schedule = CallSchedule::for_frame(frame);
    host.preflight(&schedule).map_err(ProcessAllError::Host)?;

    let daemon_before = *daemon;
    if daemon.busy != 0 {
        // `dec eax` at 0x00732718 has wrapping x86 machine semantics.
        daemon.busy = daemon.busy.wrapping_sub(1);
    }
    for repath in &mut daemon.repaths {
        // `cdq; sub eax,edx; sar eax,1` is signed division by two toward zero.
        let half = *repath / 2;
        *repath = if half < 3 { 0 } else { half };
    }

    host.process_victory();
    if schedule.contains(GameDaemonCall::CalcDanger) {
        host.calc_danger();
    }
    if schedule.contains(GameDaemonCall::UpdateAllSeen) {
        host.update_all_seen(&mut daemon.busy);
    }
    host.calc_markets();

    let mut region_current_cleared = 0u8;
    let mut region_pending_promoted = 0u8;
    for region in regions.iter_mut() {
        let old = region.flags;
        region_current_cleared += u8::from(old & 0x10 != 0);
        region.flags &= !0x10;
        if region.flags & 0x20 != 0 {
            region_pending_promoted += 1;
            region.flags = (region.flags & !0x20) | 0x10;
        }
    }

    daemon.borders = 0;
    daemon.borders = host.check_borders(regions);
    host.process_coll_blocks(&mut daemon.empty_colls);
    host.groups_process();

    Ok(ProcessAllTrace {
        frame,
        schedule,
        daemon_before,
        daemon_after: *daemon,
        region_current_cleared,
        region_pending_promoted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[derive(Default)]
    struct Host {
        reject: bool,
        preflights: Cell<u32>,
        calls: Vec<GameDaemonCall>,
        flags_at_borders: Vec<u32>,
        border_result: i32,
        coll_cursor_before: Option<i32>,
    }

    impl GameDaemonProcessAllHost for Host {
        type Fault = &'static str;

        fn preflight(&self, _schedule: &CallSchedule) -> Result<(), Self::Fault> {
            self.preflights.set(self.preflights.get() + 1);
            if self.reject {
                Err("missing live child")
            } else {
                Ok(())
            }
        }

        fn process_victory(&mut self) {
            self.calls.push(GameDaemonCall::ProcessVictory);
        }

        fn calc_danger(&mut self) {
            self.calls.push(GameDaemonCall::CalcDanger);
        }

        fn update_all_seen(&mut self, busy: &mut i32) {
            self.calls.push(GameDaemonCall::UpdateAllSeen);
            *busy = 4;
        }

        fn calc_markets(&mut self) {
            self.calls.push(GameDaemonCall::CalcMarkets);
        }

        fn check_borders(&mut self, regions: &mut [RegionBorderState]) -> i32 {
            self.calls.push(GameDaemonCall::CheckBorders);
            self.flags_at_borders = regions.iter().take(5).map(|r| r.flags).collect();
            self.border_result
        }

        fn process_coll_blocks(&mut self, empty_colls: &mut i32) {
            self.calls.push(GameDaemonCall::ProcessCollBlocks);
            self.coll_cursor_before = Some(*empty_colls);
            *empty_colls = (*empty_colls).wrapping_add(5);
        }

        fn groups_process(&mut self) {
            self.calls.push(GameDaemonCall::GroupsProcess);
        }
    }

    fn regions() -> Vec<RegionBorderState> {
        vec![RegionBorderState::default(); REGION_SLOTS]
    }

    #[test]
    fn frame_gates_and_call_order_match_the_two_idiv_remainders() {
        assert_eq!(
            CallSchedule::for_frame(0).as_slice(),
            &[
                GameDaemonCall::ProcessVictory,
                GameDaemonCall::CalcDanger,
                GameDaemonCall::CalcMarkets,
                GameDaemonCall::CheckBorders,
                GameDaemonCall::ProcessCollBlocks,
                GameDaemonCall::GroupsProcess,
            ]
        );
        assert_eq!(
            CallSchedule::for_frame(33).as_slice(),
            &[
                GameDaemonCall::ProcessVictory,
                GameDaemonCall::UpdateAllSeen,
                GameDaemonCall::CalcMarkets,
                GameDaemonCall::CheckBorders,
                GameDaemonCall::ProcessCollBlocks,
                GameDaemonCall::GroupsProcess,
            ]
        );
        assert_eq!(
            CallSchedule::for_frame(34).as_slice(),
            &[
                GameDaemonCall::ProcessVictory,
                GameDaemonCall::CalcMarkets,
                GameDaemonCall::CheckBorders,
                GameDaemonCall::ProcessCollBlocks,
                GameDaemonCall::GroupsProcess,
            ]
        );
    }

    #[test]
    fn local_state_and_region_rollover_happen_before_the_tail_calls() {
        let mut daemon = GameDaemonState {
            repaths: [-9, 0, 3, 5, 6, 7, 8, 100],
            empty_colls: 17,
            borders: 99,
            busy: 2,
        };
        let mut rs = regions();
        rs[0].flags = 0x10;
        rs[1].flags = 0x20;
        rs[2].flags = 0x30;
        rs[3].flags = 0x40;
        rs[4].flags = 0x70;
        let mut host = Host {
            border_result: 123,
            ..Default::default()
        };

        let trace = process_all(&mut daemon, 0, &mut rs, &mut host).unwrap();

        assert_eq!(daemon.repaths, [0, 0, 0, 0, 3, 3, 4, 50]);
        assert_eq!(daemon.busy, 1);
        assert_eq!(daemon.borders, 123);
        assert_eq!(host.coll_cursor_before, Some(17));
        assert_eq!(daemon.empty_colls, 22);
        assert_eq!(
            host.flags_at_borders.as_slice(),
            &[0, 0x10, 0x10, 0x40, 0x50]
        );
        assert_eq!(trace.region_current_cleared, 3);
        assert_eq!(trace.region_pending_promoted, 3);
        assert_eq!(host.calls.as_slice(), trace.schedule.as_slice());
    }

    #[test]
    fn nonzero_busy_uses_the_retail_wrapping_decrement() {
        let mut daemon = GameDaemonState {
            busy: i32::MIN,
            ..Default::default()
        };
        let mut rs = regions();
        let mut host = Host::default();
        process_all(&mut daemon, 1, &mut rs, &mut host).unwrap();
        assert_eq!(daemon.busy, i32::MAX);
    }

    #[test]
    fn reached_visibility_child_owns_the_post_decrement_busy_store() {
        let mut daemon = GameDaemonState {
            busy: 19,
            ..Default::default()
        };
        let mut rs = regions();
        let mut host = Host::default();

        process_all(&mut daemon, 33, &mut rs, &mut host).unwrap();

        assert_eq!(daemon.busy, 4);
        assert!(host.calls.contains(&GameDaemonCall::UpdateAllSeen));
    }

    #[test]
    fn host_preflight_failure_is_atomic() {
        let before = GameDaemonState {
            repaths: [12; LEADER_SLOTS],
            empty_colls: 8,
            borders: 7,
            busy: 6,
        };
        let mut daemon = before;
        let mut rs = regions();
        rs[0].flags = 0x30;
        let before_flags: Vec<_> = rs.iter().map(|r| r.flags).collect();
        let mut host = Host {
            reject: true,
            ..Default::default()
        };

        assert_eq!(
            process_all(&mut daemon, 0, &mut rs, &mut host),
            Err(ProcessAllError::Host("missing live child"))
        );
        assert_eq!(daemon, before);
        assert_eq!(rs.iter().map(|r| r.flags).collect::<Vec<_>>(), before_flags);
        assert!(host.calls.is_empty());
    }

    #[test]
    fn nonretail_region_cardinality_fails_before_host_preflight() {
        let before = GameDaemonState {
            repaths: [9; LEADER_SLOTS],
            empty_colls: 2,
            borders: 3,
            busy: 4,
        };
        let mut daemon = before;
        let mut rs = vec![RegionBorderState::default(); REGION_SLOTS - 1];
        let mut host = Host::default();

        assert_eq!(
            process_all(&mut daemon, 33, &mut rs, &mut host),
            Err(ProcessAllError::RegionCardinality {
                expected: REGION_SLOTS,
                actual: REGION_SLOTS - 1,
            })
        );
        assert_eq!(daemon, before);
        assert_eq!(host.preflights.get(), 0);
        assert!(host.calls.is_empty());
    }
}
