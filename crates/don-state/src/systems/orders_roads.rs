//! Steps 21 and 22 of `Game::do_frame` 0x00591ef0 (decomp 00591ef0.c:235-236),
//! both called unconditionally right after `Game::frame++`:
//!
//! * 21 `OrdersMemManager::cycle` 0x00730E20 — **writes no walked state**.
//!   `Ported` as a no-op.
//! * 22 `Roads::scan_and_kill_stray_roads` 0x008956A0 — writes walked
//!   `World.tdata` road bits only when a stray road is found; the scan
//!   cursor it depends on lives in the unwalked `Roads` singleton, so the
//!   body cannot be executed from a `Save`. `Stub`.
//!
//! `tick.rs` calls `run` for both steps without the index; `run` is a
//! no-op (there is nothing transcribable to execute). `run_step` /
//! `step_status` expose the per-step truth for when the schedule is
//! switched to per-step dispatch.
//!
//! # Step 21 — `OrdersMemManager::cycle` 0x00730E20 (`Ported`, no-op)
//!
//! `ordmgr` (PUB `class OrdersMemManager ordmgr` @ 0x00EB4390, sizeof 896 =
//! `SafeRecycler<UnitOrder> order_lists[28]`, stride 0x20). The body
//! (re/decomp-all/00730e20.c) walks the 28 recyclers from `&DAT_00eb4394`
//! to `0x00EB4714` and, for each, pops every pointer off the *pending*
//! list (`[+0x18]` count, `[+0x10]` list) and pushes it onto the *free*
//! list (`[+0x00]` list, `[+0x04]` size, `[+0x08]` length, `[+0x0c]`
//! increment), growing the free list with `malloc`/`memcpy`/`free`. It
//! never dereferences a `UnitOrder`; it only moves pointers between the
//! two per-pool arrays.
//!
//! Evidence that nothing it writes is walked: the only code referencing
//! `0x00EB4390..0x00EB4714` is `OrdersMemManager::{cycle 0x00730E20,
//! clear 0x00730DB0, get_obj 0x00730AC0}`, the static initializer
//! 0x004044A0 and the atexit destructor 0x00AB6B50 (grep of
//! `re/decomp-all` for `DAT_00eb4(39.|[4-6]..|70.|71[0-3])`). No
//! `walk_data`/`check_*` function touches the pool, and `WalkDataGame`
//! (0x005a2360) has no `OrdersMemManager` section. The free/pending lists
//! are heap bookkeeping only. A no-op is therefore the complete port with
//! respect to save/checksum state.
//!
//! # Step 22 — `Roads::scan_and_kill_stray_roads` 0x008956A0 (`Stub`)
//!
//! `this` = `MiscAccess::roads` (PUB @ 0x00C06238; `Roads` sizeof 1688, no
//! `Roads::walk_data` exists — PDB lists only ctor/dtor, add/clear/set
//! helpers and the three scan functions). Body (re/decomp-all/008956a0.c):
//!
//! ```text
//! n = World.size(+0x08) / 500            // iterations per frame
//! for n times:
//!   Roads.curscan_x(+0x5fc) += 1; wrap at World.xs(+0x00) -> curscan_y(+0x600) += 1, wrap at World.ys(+0x04)
//!   ci = CoordInfo[curscan_y * [0x00c061d0] + curscan_x]  (via [0x00c06218]+0x6ab0)
//!   for sub in 0..16:   // 4x4 tiles of the wcoord
//!     tx = curscan_x*4 + (sub&3); ty = curscan_y*4 + (sub>>2)
//!     fill Roads.road_cache2(+0x604)[9], build_cache(+0x628)[9], neighbor flags from
//!       World.tdata(+0x138)[World.tile_xs(+0x18) * y + x]  (bits 0x30==0x10 road, &3==3, 0x800 / 0x30==0x20)
//!     Roads::scan_and_kill_bad_tcoord 0x0088E100(ci, &tx, &ty, sub)
//!     if road_cache2[0] != 0: Roads::scan_and_kill_straggled_tcoord 0x0088E050(&tx, &ty)
//! ```
//!
//! Both kill helpers, when they decide a tile's road is stray, call
//! `TerrainOut::road_changed` 0x00866A70 (renderer, unwalked) and
//! `World::set_road_at` 0x006B43B0 (writes `World.tdata` — **walked**, the
//! `world` checksum channel). So the step *does* write walked state, but
//! only on the kill path.
//!
//! Why it is a `Stub` and not `Partial`:
//! * The scan position `Roads::curscan_x/curscan_y` is unwalked. After a
//!   retail load it restarts from whatever the `Roads` singleton holds, so
//!   even retail does not reproduce the pre-save scan phase; from a `Save`
//!   there is no way to know which wcoord is examined this frame.
//! * The kill decision reads `CoordInfo[+0x5c]` (per-wcoord road-piece
//!   table owned by `[0x00c06218]+0x6ab0`) — not in the `Save` tree.
//! * In all captured frames (`schema/live/frame-pairs/*`) no road bits
//!   change in `World.tdata` from this step; stray roads arise only after
//!   road-owning buildings are destroyed.
//!
//! Stopping point: a port needs (a) the `Roads` scan cursor to be either
//! walked or declared part of the sim state we track, and (b) the
//! `CoordInfo` road-piece table. Until then the body leaves `World.tdata`
//! untouched.

use crate::tick::StepStatus;
use crate::Save;

/// Aggregate for the shared `21 | 22 => orders_roads::run` arm in
/// `tick.rs`: step 22 is a stub, so the pair is reported as `Stub`. Use
/// [`step_status`] for the per-step truth.
pub const STATUS: StepStatus = StepStatus::Stub;

/// Per-step status.
pub fn step_status(idx: usize) -> StepStatus {
    match idx {
        21 => StepStatus::Ported,
        22 => StepStatus::Stub,
        _ => StepStatus::Stub,
    }
}

/// Shared entry used by `tick.rs` for steps 21 and 22. Neither step has
/// an executable transcription (21 writes nothing walked; 22 cannot be
/// evaluated from a `Save`), so this is a no-op by construction.
pub fn run(_save: &mut Save, _effects: &mut Vec<String>) {}

/// Per-step entry. Same behaviour as [`run`] today; kept so a per-step
/// schedule can dispatch here directly.
pub fn run_step(idx: usize, _save: &mut Save, _effects: &mut Vec<String>) {
    match idx {
        // OrdersMemManager::cycle 0x00730E20 — SafeRecycler pool flip only
        // (see module docs). No walked state.
        21 => {}
        // Roads::scan_and_kill_stray_roads 0x008956A0 — stub (see module
        // docs). Fields untouched.
        22 => {}
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses() {
        assert_eq!(step_status(21), StepStatus::Ported);
        assert_eq!(step_status(22), StepStatus::Stub);
    }
}
