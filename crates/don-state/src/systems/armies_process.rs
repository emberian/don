//! Step 13: `Armies::process_all` 0x006F3B00 and the per-army
//! `Army::process` 0x006F93D0 it drives (AI standing armies).
//!
//! Transcribed from `re/decomp-all/006f3b00.c`, `006f93d0.c`, `006f9b50.c`
//! (`Army::normalize`), `006f8ea0.c` (`Army::close`), `006f4260.c`
//! (`Army::do_mustering`), `006f87c0.c` (`Army::release_mustering`),
//! `006f5470.c` (`Army::is_moving`), `006f56d0.c` (`Army::is_engaged`),
//! `006f8750.c` (`Army::set_stance`). Field offsets from
//! `re/scripts/pdb_layout.py Army` (sizeof 160; the walked image is
//! `valid` i16 at +0 then `body` = +0x02..+0x98).
//!
//! Status: **Partial**. Everything an army with no groups attached can
//! reach is transcribed up to `Army::find_muster_spot` 0x006F5CC0, which
//! is the writer of the only Army bytes retail moves in the captures
//! (`status |= 0x10`, `muster_x/y`, `muster_angle`, every 256 frames per
//! army). `find_muster_spot` (3,309 B) is an Objects spatial search
//! (`FUN_0065ca80`/`FUN_0065d260`, encrypted object coords `^0x63637`,
//! `DAT_00cae5fc` coord->tile table) and is left as `TODO(va)`; so are all
//! group/unit-bearing branches (`GroupData::count` 0x00711720,
//! `Group::action_halt` 0x0070D0C0, `Army::send_here` 0x006F98A0,
//! `Army::add_unit` 0x006F9F40, `do_forming`/`do_defending`/`do_marching`/
//! `engagement`/`do_transporting`). Untranscribed branches leave every
//! field untouched and stop processing that army for the frame.
//!
//! RNG: none of the transcribed code calls `game_random` (`FUN_00a39d70`).

use crate::tick::StepStatus;
use crate::Save;

pub const STATUS: StepStatus = StepStatus::Partial;

/// Army field offsets (class-relative, `pdb_layout.py Army`).
mod off {
    pub const ARMY: usize = 0x02; // i16
    pub const STATUS: usize = 0x04; // u32 bitfield
    pub const REG: usize = 0x08;
    pub const ROLE: usize = 0x0c;
    pub const NUM_UNITS: usize = 0x10;
    pub const NUM_CAPTAINS: usize = 0x14;
    pub const NUM_STANDARD: usize = 0x18;
    pub const NUM_DECOYS: usize = 0x1c;
    pub const CITY: usize = 0x20;
    pub const NAVY: usize = 0x24;
    pub const HUMAN_FRAME: usize = 0x28;
    pub const TARGET_O: usize = 0x30;
    pub const TARGET_WHO: usize = 0x34;
    pub const WHO: usize = 0x94; // i16
    pub const NUM_GROUPS: usize = 0x96; // i16
}

/// Leader record stride and `LeaderData::walk_data` image offsets.
/// `Leader.body` holds LeaderData +0x08.. so `body[x - 8]` is `+x`.
const LEADER_CITY_NUM: usize = 0x3f8; // LeaderData.city_num

/// `CityData` image: `City.flags` is +0x4, `City.pod` holds +0x06..+0x72.
const CITY_WHO: usize = 0x5e; // CityData.who (char)

/// Typed access to one `Army` byte image (`valid` + `body`).
struct ArmyRef<'a> {
    valid: &'a mut i16,
    body: &'a mut [u8],
}

impl ArmyRef<'_> {
    fn i32(&self, o: usize) -> i32 {
        i32::from_le_bytes(self.body[o - 2..o + 2].try_into().unwrap())
    }
    fn u32(&self, o: usize) -> u32 {
        self.i32(o) as u32
    }
    fn i16(&self, o: usize) -> i16 {
        i16::from_le_bytes(self.body[o - 2..o].try_into().unwrap())
    }
    fn set_i32(&mut self, o: usize, v: i32) {
        self.body[o - 2..o + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn set_u32(&mut self, o: usize, v: u32) {
        self.set_i32(o, v as i32);
    }
    fn set_i16(&mut self, o: usize, v: i16) {
        self.body[o - 2..o].copy_from_slice(&v.to_le_bytes());
    }
}

fn army_ref(save: &mut Save, who: usize, idx: usize) -> Option<ArmyRef<'_>> {
    let a = save.armies.lists.get_mut(who)?.elems.get_mut(idx)?.as_mut()?;
    if a.body.len() != 150 {
        return None;
    }
    Some(ArmyRef {
        valid: &mut a.valid,
        body: &mut a.body,
    })
}

fn game_frame(save: &Save) -> i32 {
    i32::from_le_bytes(save.game.scalars[crate::tick::FRAME..crate::tick::FRAME + 4].try_into().unwrap())
}

/// `Armies::process_all` 0x006F3B00 (006f3b00.c). Eight leader slots
/// (`DAT_00e3a390`, stride 0x6eec) paired with `Armies.lists[who]`
/// (`DAT_00c09710`, stride 28; count at -0xc, data at +0).
pub fn run(save: &mut Save, effects: &mut Vec<String>) {
    for who in 0..8usize {
        let Some(ld) = save.leaders.slots.get(who) else { continue };
        // (flags & 1) && (flags & 0xc) != 4 && (flags2 & 0xa) == 0
        if ld.flags & 1 == 0 || (ld.flags & 0xc) == 4 || (ld.flags2 & 0xa) != 0 {
            continue;
        }
        let n = save.armies.lists.get(who).map_or(0, |l| l.len.max(0) as usize);
        for idx in 0..n {
            let Some(mut a) = army_ref(save, who, idx) else { continue };
            if *a.valid == 0 {
                continue;
            }
            // bVar5 = (char)status < 0  → bit 7; cleared before the call.
            let st = a.u32(off::STATUS);
            let forced = st & 0x80 != 0;
            if forced {
                a.set_u32(off::STATUS, st & 0xffff_ff7f);
                effects.push(format!("Army[{who}][{idx}].status &= ~0x80 (forced process)"));
            }
            army_process(save, who, idx, forced, effects);
        }
    }
}

/// `Army::process(int forced)` 0x006F93D0 (006f93d0.c).
fn army_process(save: &mut Save, who: usize, idx: usize, forced: bool, effects: &mut Vec<String>) {
    let frame = game_frame(save);
    let tag = format!("Army[{who}][{idx}]");
    let Some(mut a) = army_ref(save, who, idx) else { return };

    // if (human_frame != 0) human_frame--;
    let hf = a.i32(off::HUMAN_FRAME);
    if hf != 0 {
        a.set_i32(off::HUMAN_FRAME, hf - 1);
        effects.push(format!("{tag}.human_frame -> {}", hf - 1));
    }

    let army_who = a.i16(off::WHO) as i32;
    let lwho = army_who as usize;
    // iVar5 = (army + who*2)*2 — per-army stagger.
    let stagger = (a.i16(off::ARMY) as i32 + army_who * 2) * 2;
    // Leader flag bit 0x40 gates both phases.
    let leader_flags = save.leaders.slots.get(lwho).map_or(0, |l| l.flags);

    if !forced {
        // (Game.frame - 30 + stagger) % 128 == 0  (signed-rem idiom)
        if (frame - 0x1e + stagger) % 128 == 0 {
            if leader_flags & 0x40 != 0 {
                return;
            }
            normalize(save, who, idx, effects);
            let Some(a) = army_ref(save, who, idx) else { return };
            if a.i32(off::NUM_CAPTAINS) == 0 {
                return;
            }
            // use_generals 0x006F4C30 (if num_standard && count(0x13,0x36)),
            // use_spies 0x006F4AF0 (count(0x13,0x3a)), use_scouts 0x006F49A0
            // (count(0x13,0x45)) — Army::count 0x006F9120 sums
            // GroupData::count 0x00711720 over the attached groups.
            // TODO(0x006f9120) TODO(0x006f4c30) TODO(0x006f4af0) TODO(0x006f49a0)
            effects.push(format!("{tag}: TODO(0x006f4c30/0x006f4af0/0x006f49a0) 128-phase specials"));
            // Falls through to the 256-phase test, which cannot pass when
            // (frame + stagger) ≡ 30 (mod 128): retail returns here.
        }
        // (Game.frame + stagger) % 256 == 0 else return
        if (frame + stagger) % 256 != 0 {
            return;
        }
    }

    if leader_flags & 0x40 != 0 {
        return;
    }
    normalize(save, who, idx, effects);
    let Some(a) = army_ref(save, who, idx) else { return };

    // Disband branch: no standard units, still valid, not mustering.
    if a.i32(off::NUM_STANDARD) < 1 && *a.valid != 0 && a.u32(off::STATUS) & 1 == 0 {
        let city_num = save
            .leaders
            .slots
            .get(lwho)
            .filter(|l| l.body.len() >= LEADER_CITY_NUM - 8 + 4)
            .map_or(0, |l| {
                i32::from_le_bytes(l.body[LEADER_CITY_NUM - 8..LEADER_CITY_NUM - 4].try_into().unwrap())
            });
        let Some(mut a) = army_ref(save, who, idx) else { return };
        a.set_i32(off::CITY, 0);
        effects.push(format!("{tag}.city = 0 (disband scan)"));
        if city_num > 0 {
            // while (!(Cities.lists[who][city].flags & 1)) if (++city >= city_num) { close(); return; }
            let mut city = 0i32;
            loop {
                let flags = save
                    .cities
                    .lists
                    .get(lwho)
                    .and_then(|l| l.elems.get(city as usize))
                    .and_then(|c| c.as_ref())
                    .map_or(0, |c| c.flags);
                if flags & 1 != 0 {
                    break;
                }
                city += 1;
                let Some(mut a) = army_ref(save, who, idx) else { return };
                a.set_i32(off::CITY, city);
                effects.push(format!("{tag}.city -> {city}"));
                if city >= city_num {
                    close(save, who, idx, effects);
                    return;
                }
            }
            // send_here(city.x, city.y, 1) 0x006F98A0: writes x/y (clamped to
            // World extents), muster_x/y via DAT_00cae5fc[coord >> 8],
            // muster_angle via FUN_0092d130, then orders every attached group.
            // TODO(0x006f98a0)
            effects.push(format!("{tag}: TODO(0x006f98a0) send_here(city {city}) before close"));
            return;
        }
        close(save, who, idx, effects);
        return;
    }

    if a.i32(off::HUMAN_FRAME) != 0 {
        // send_here(x, y, 2); return.  TODO(0x006f98a0)
        effects.push(format!("{tag}: TODO(0x006f98a0) send_here(x, y, 2)"));
        return;
    }

    // Merge-into-sibling scan when under half strength.
    if a.i32(off::NUM_STANDARD) < (a.i32(off::NUM_CAPTAINS) - a.i32(off::NUM_DECOYS)) / 2 {
        let my_reg = a.i32(off::REG);
        let list = &save.armies.lists[who];
        let mut merge_target = None;
        for j in 0..16usize {
            let Some(Some(o)) = list.elems.get(j) else { continue };
            if o.valid == 0 || o.body.len() != 150 {
                continue;
            }
            let g = |off: usize| i32::from_le_bytes(o.body[off - 2..off + 2].try_into().unwrap());
            if g(off::NAVY) == 0
                && g(off::REG) == my_reg
                && 4 < g(off::NUM_STANDARD)
                && (g(off::NUM_CAPTAINS) - g(off::NUM_DECOYS)) / 2 <= g(off::NUM_STANDARD)
            {
                merge_target = Some(j);
                break;
            }
        }
        if let Some(j) = merge_target {
            // for each group in list[]: for each unit: if Unit.flags & 1 →
            // sibling.add_unit(unit) 0x006F9F40; then close().
            // TODO(0x006f9f40)
            effects.push(format!("{tag}: TODO(0x006f9f40) merge into Army[{who}][{j}] then close"));
            return;
        }
    }

    // if (!is_moving() && !is_engaged()) { status &= ~0x12; retarget... }
    let Some(moving) = is_moving(save, who, idx) else {
        effects.push(format!("{tag}: TODO(0x006f5470) is_moving with num_units > 0"));
        return;
    };
    if !moving {
        let Some(engaged) = is_engaged(save, who, idx, effects) else {
            effects.push(format!("{tag}: TODO(0x006f56d0) is_engaged with num_units > 0"));
            return;
        };
        if !engaged {
            let Some(mut a) = army_ref(save, who, idx) else { return };
            let st = a.u32(off::STATUS);
            if st & 0x12 != 0 {
                effects.push(format!("{tag}.status &= ~0x12 ({st:#x} -> {:#x})", st & !0x12));
            }
            a.set_u32(off::STATUS, st & 0xffff_ffed);
            if a.i32(off::TARGET_O) >= 0 && a.i32(off::TARGET_WHO) >= 0 {
                // LeaderData::is_enemy(target_who) 0x006EBAA0; then either
                // find_muster_spot(target_o, target_who, 0) + status |= 0x12, or
                // muster_x/y = avg(muster, tile(target object coords ^ 0x63637))
                // + status |= 0x12.  TODO(0x006f5cc0) TODO(0x006ebaa0)
                effects.push(format!("{tag}: TODO(0x006f5cc0) retarget with target_o/target_who"));
                return;
            }
        }
    }

    let Some(mut a) = army_ref(save, who, idx) else { return };
    // if ((status & 0x18) == status) status = 2;
    let st = a.u32(off::STATUS);
    if st & 0x18 == st {
        a.set_u32(off::STATUS, 2);
        effects.push(format!("{tag}.status = 2 (was {st:#x})"));
    }

    // State dispatch. Each handler is driven by status bits in sequence.
    let st = a.u32(off::STATUS);
    if st & 1 != 0 {
        if !set_stance(save, who, idx, 1) {
            effects.push(format!("{tag}: TODO(0x006f8750) set_stance(1) with groups"));
            return;
        }
        if !do_mustering(save, who, idx, effects) {
            return;
        }
    }
    let Some(a) = army_ref(save, who, idx) else { return };
    let st = a.u32(off::STATUS);
    if st & 0x20 != 0 {
        // set_stance(1); do_defending() 0x006F4070.  TODO(0x006f4070)
        effects.push(format!("{tag}: TODO(0x006f4070) do_defending"));
        return;
    }
    if st & 2 != 0 {
        // set_stance(0); do_marching() 0x006F3DF0.  TODO(0x006f3df0)
        effects.push(format!("{tag}: TODO(0x006f3df0) do_marching"));
        return;
    }
    if st & 0x10 == 0 {
        // if (is_engaged()) engagement() 0x006F5160.  TODO(0x006f5160)
        effects.push(format!("{tag}: TODO(0x006f5160) engagement check"));
        return;
    } else {
        // do_forming() 0x006F43C0.  TODO(0x006f43c0)
        effects.push(format!("{tag}: TODO(0x006f43c0) do_forming"));
        return;
    }
}

/// `Army::normalize` 0x006F9B50 (006f9b50.c). Zeroes `role`, `num_units`,
/// `num_captains`, `num_standard`, `num_decoys`, then re-accumulates them
/// from the attached groups (`GroupData::count` 0x00711720, virtual +8)
/// and sorts `list[]` by leader-unit value. Fully transcribed only for
/// `num_groups == 0` (the pure zeroing); with groups attached the
/// accumulation is `TODO(0x00711720)` and the fields are left untouched.
fn normalize(save: &mut Save, who: usize, idx: usize, effects: &mut Vec<String>) {
    let Some(mut a) = army_ref(save, who, idx) else { return };
    let tag = format!("Army[{who}][{idx}]");
    if a.i16(off::NUM_GROUPS) > 0 {
        effects.push(format!("{tag}: TODO(0x00711720) normalize with {} groups", a.i16(off::NUM_GROUPS)));
        return;
    }
    for o in [off::ROLE, off::NUM_UNITS, off::NUM_CAPTAINS, off::NUM_STANDARD, off::NUM_DECOYS] {
        if a.i32(o) != 0 {
            effects.push(format!("{tag}.+{o:#x} = 0 (normalize)"));
        }
        a.set_i32(o, 0);
    }
}

/// `Army::close` 0x006F8EA0 (006f8ea0.c): detach each group whose
/// `GroupData.army == army` (set -1, `Group::action_halt(0)` 0x0070D0C0),
/// then `valid = 0; status = 0; human_frame = 0; num_groups = 0`.
/// Group detach is `TODO(0x0070d0c0)`; with groups attached nothing is
/// written.
fn close(save: &mut Save, who: usize, idx: usize, effects: &mut Vec<String>) {
    let Some(mut a) = army_ref(save, who, idx) else { return };
    let tag = format!("Army[{who}][{idx}]");
    if *a.valid == 0 {
        return;
    }
    if a.i16(off::NUM_GROUPS) > 0 {
        effects.push(format!("{tag}: TODO(0x0070d0c0) close with groups attached"));
        return;
    }
    *a.valid = 0;
    a.set_u32(off::STATUS, 0);
    a.set_i32(off::HUMAN_FRAME, 0);
    a.set_i16(off::NUM_GROUPS, 0);
    effects.push(format!("{tag}: close (valid=0, status=0, human_frame=0, num_groups=0)"));
}

/// `Army::is_moving` 0x006F5470 (006f5470.c): counts units en route via
/// Objects; `num_units / 6 < count`. `Some(false)` for `num_units <= 0`
/// (loop body never runs, `0 / 6 < 0` is false); `None` = TODO.
fn is_moving(save: &mut Save, who: usize, idx: usize) -> Option<bool> {
    let a = army_ref(save, who, idx)?;
    if a.i32(off::NUM_UNITS) <= 0 {
        return Some(false);
    }
    None // TODO(0x006f5470): per-unit Objects walk
}

/// `Army::is_engaged` 0x006F56D0 (006f56d0.c): `normalize()` then counts
/// units in combat; returns 1 when more than a quarter are. `Some(false)`
/// for `num_units <= 0`; `None` = TODO.
fn is_engaged(save: &mut Save, who: usize, idx: usize, effects: &mut Vec<String>) -> Option<bool> {
    normalize(save, who, idx, effects);
    let a = army_ref(save, who, idx)?;
    if a.i32(off::NUM_UNITS) <= 0 {
        return Some(false);
    }
    None // TODO(0x006f56d0): per-unit Objects walk
}

/// `Army::set_stance(stance)` 0x006F8750 (006f8750.c): for each attached
/// non-empty group not already in that stance (`FUN_0070d370`), apply it
/// (`FUN_0070d440`). No-op with no groups; `false` = TODO with groups.
fn set_stance(save: &mut Save, who: usize, idx: usize, _stance: i32) -> bool {
    let Some(a) = army_ref(save, who, idx) else { return true };
    a.i16(off::NUM_GROUPS) <= 0 // TODO(0x0070d440) group stance writes
}

/// `Army::release_mustering` 0x006F87C0 (006f87c0.c). `Some(true/false)`
/// where transcribed; `None` once the leader/city survey past
/// `num_standard < 2` is needed (TODO).
fn release_mustering(save: &Save, who: usize, idx: usize) -> Option<bool> {
    let a = save.armies.lists.get(who)?.elems.get(idx)?.as_ref()?;
    let g = |off: usize| i32::from_le_bytes(a.body[off - 2..off + 2].try_into().unwrap());
    let army_who = i16::from_le_bytes(a.body[off::WHO - 2..off::WHO].try_into().unwrap()) as i32;
    let city = g(off::CITY);
    // Cities.lists[who][city] — retail dereferences unconditionally.
    let c = save
        .cities
        .lists
        .get(army_who as usize)?
        .elems
        .get(usize::try_from(city).ok()?)?
        .as_ref()?;
    // if ((short)city.who != who) return 1;
    let c_who = *c.pod.get(CITY_WHO - 6)? as i8 as i32;
    if c_who != army_who {
        return Some(true);
    }
    // if (!(city.flags & 1)) return 1;
    if c.flags & 1 == 0 {
        return Some(true);
    }
    // if (Game.info.rush_rules && !Game::war_allowed()) return 0;
    // GameInfo.settings = info.data (+0x24..+0x42); rush_rules = +0x32.
    let rush_rules = save.game.info.settings.get(0x32 - 0x24).copied().unwrap_or(0);
    if rush_rules != 0 {
        // Game::war_allowed 0x00594670: FUN_005946d0() age vs rush_rules and
        // Game.frame vs Rules age table — TODO(0x00594670)
        return None;
    }
    // if (Leader.flags & 8) return 0;
    if save.leaders.slots.get(army_who as usize)?.flags & 8 != 0 {
        return Some(false);
    }
    // if (num_standard < 2) return 0;
    if g(off::NUM_STANDARD) < 2 {
        return Some(false);
    }
    // Remaining: survey of allied leaders' cities under siege (flags & 3 == 3),
    // city_num, navy, Leader +0x7e4/+0x9e0 ratios, Armies::num_armies
    // 0x006F3200, LeaderData::get_diff 0x006EC000, Leader +0x6dd4/+0x6e20.
    None // TODO(0x006f87c0)
}

/// `Army::do_mustering` 0x006F4260 (006f4260.c). Returns `false` when it
/// stopped at a TODO (caller must stop processing this army).
fn do_mustering(save: &mut Save, who: usize, idx: usize, effects: &mut Vec<String>) -> bool {
    let tag = format!("Army[{who}][{idx}]");
    let Some(release) = release_mustering(save, who, idx) else {
        effects.push(format!("{tag}: TODO(0x006f87c0) release_mustering"));
        return false;
    };
    let Some(a) = army_ref(save, who, idx) else { return false };
    let army_who = a.i16(off::WHO) as i32;
    let city = a.i32(off::CITY);
    if !release {
        if city >= 0 {
            let flags = save
                .cities
                .lists
                .get(army_who as usize)
                .and_then(|l| l.elems.get(city as usize))
                .and_then(|c| c.as_ref())
                .map_or(0, |c| c.flags);
            if flags & 1 != 0 {
                // if (find_muster_spot(city.o, who, 1)) { status |= 0x10; return 1; }
                // find_muster_spot 0x006F5CC0 writes muster_x, muster_y,
                // muster_angle (FUN_0092d130), hurry; Objects spatial search.
                // TODO(0x006f5cc0)
                effects.push(format!("{tag}: TODO(0x006f5cc0) find_muster_spot(city {city}, {army_who}, 1)"));
                return false;
            }
        }
        // Leader +0xa68 unit-type table bit 8, num_captains > 7, Leader.flags & 0x300
        // → status = 0x40 (transport); else fall through to status = 2.
        // TODO(0x006f4260): tail (status/city/x/y/angle rewrite)
        effects.push(format!("{tag}: TODO(0x006f4260) do_mustering tail after release==0"));
        return false;
    }
    // release != 0: navy==0 → status in {0x20, 2, 0x40} by Leader tables/get_diff,
    // else status = 2; then city = -1; x/y = muster*0x300+0x180; angle = muster_angle.
    // TODO(0x006f4260)
    effects.push(format!("{tag}: TODO(0x006f4260) do_mustering release path"));
    false
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::tick;

    fn capture_dirs() -> Vec<PathBuf> {
        let Ok(root) = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize() else {
            return Vec::new();
        };
        let pairs = root.join("schema/live/frame-pairs");
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(pairs) {
            for e in rd.flatten() {
                let d = e.path();
                if d.is_dir() && d.join("manifest.json").is_file() {
                    out.push(d);
                }
            }
        }
        out.sort();
        out
    }

    /// `(frame, path)` for every `.svx` in a capture dir, sorted by frame.
    fn frames(dir: &Path) -> Vec<(i32, PathBuf)> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.extension().map_or(false, |x| x == "svx") {
                let raw = crate::container::load_svx(&p).unwrap();
                let img = crate::load(&raw).unwrap();
                out.push((super::game_frame(&img.state), p));
            }
        }
        out.sort();
        out
    }

    fn army_images(save: &crate::Save) -> Vec<(usize, usize, i16, Vec<u8>)> {
        let mut cur = Vec::new();
        for (o, l) in save.armies.lists.iter().enumerate() {
            for (i, e) in l.elems.iter().enumerate() {
                if let Some(a) = e {
                    cur.push((o, i, a.valid, a.body.clone()));
                }
            }
        }
        cur
    }

    /// Stride-1 pairs: the transcribed writes must not move a single Army
    /// byte retail left alone, and for every Army retail changed we must
    /// either match or have stopped at a TODO for that army.
    #[test]
    fn stride1_army_bytes_not_introduced() {
        let mut pairs = 0;
        let mut phase128_hits = 0;
        for dir in capture_dirs() {
            let fr = frames(&dir);
            for w in fr.windows(2) {
                let ((fa, pa), (fb, pb)) = (&w[0], &w[1]);
                if fb - fa != 1 {
                    continue;
                }
                pairs += 1;
                let raw_a = crate::container::load_svx(pa).unwrap();
                let raw_b = crate::container::load_svx(pb).unwrap();
                let mut ours = crate::load(&raw_a).unwrap().state;
                let retail_b = crate::load(&raw_b).unwrap().state;
                let before = army_images(&ours);
                let report = tick::do_frame(&mut ours);
                let after = army_images(&ours);
                let want = army_images(&retail_b);
                let step = &report.steps[13];
                assert_eq!(step.va, Some(0x006F3B00));
                phase128_hits += step.effects.iter().filter(|e| e.contains("128-phase")).count();
                for ((b, a), wnt) in before.iter().zip(after.iter()).zip(want.iter()) {
                    if a != b {
                        assert_eq!(
                            a, wnt,
                            "f{fa}->f{fb} Army[{}][{}] introduced a byte retail did not write; effects: {:?}",
                            a.0, a.1, step.effects
                        );
                    }
                }
            }
        }
        if pairs == 0 {
            eprintln!("SKIP: proprietary live captures absent (schema/live/frame-pairs/*)");
            return;
        }
        // Frames 26/22/18/14 (who = 1..4, army 0) fall in the stride-1
        // window: (frame - 30 + who*4) % 128 == 0 → normalize, then
        // num_captains == 0 → return. Nothing special fires (no captains),
        // so the 128-phase TODO must NOT appear.
        assert_eq!(phase128_hits, 0, "128-phase specials reached with num_captains == 0");
    }

    /// Stride-15 pairs: the Army records retail changed inside a 15-frame
    /// window must be exactly the ones whose 256-frame slot
    /// `(frame + (army + who*2)*2) % 256 == 0` falls inside it, and our
    /// transcription must stop at `find_muster_spot` for those and only
    /// those. Validates the stagger/gating without deriving any write.
    #[test]
    fn stride15_muster_slots_match_find_muster_spot_todo() {
        let mut checked = 0;
        for dir in capture_dirs() {
            let fr = frames(&dir);
            for w in fr.windows(2) {
                let ((fa, pa), (fb, pb)) = (&w[0], &w[1]);
                if fb - fa != 15 {
                    continue;
                }
                let raw_a = crate::container::load_svx(pa).unwrap();
                let raw_b = crate::container::load_svx(pb).unwrap();
                let mut ours = crate::load(&raw_a).unwrap().state;
                let retail_b = crate::load(&raw_b).unwrap().state;
                let before = army_images(&ours);
                let want = army_images(&retail_b);
                let mut retail_changed: Vec<(usize, usize)> = Vec::new();
                for (b, wnt) in before.iter().zip(want.iter()) {
                    if b != wnt {
                        retail_changed.push((b.0, b.1));
                    }
                }
                let mut ours_stopped: Vec<(usize, usize)> = Vec::new();
                for _ in 0..15 {
                    let report = tick::do_frame(&mut ours);
                    for e in &report.steps[13].effects {
                        if let Some(rest) = e.strip_prefix("Army[") {
                            if e.contains("TODO(0x006f5cc0) find_muster_spot") {
                                let mut it = rest.split(|c| c == '[' || c == ']').filter(|s| !s.is_empty());
                                let o: usize = it.next().unwrap().parse().unwrap();
                                let i: usize = it.next().unwrap().parse().unwrap();
                                ours_stopped.push((o, i));
                            }
                        }
                    }
                }
                ours_stopped.sort();
                ours_stopped.dedup();
                retail_changed.sort();
                if !retail_changed.is_empty() || !ours_stopped.is_empty() {
                    checked += 1;
                    eprintln!("f{fa}->f{fb}: retail changed {retail_changed:?}, ours stopped at find_muster_spot {ours_stopped:?}");
                }
                assert_eq!(
                    ours_stopped, retail_changed,
                    "f{fa}->f{fb}: armies retail changed vs armies we stopped at find_muster_spot"
                );
                // Retail's observed change set for a stopped army is exactly
                // the find_muster_spot footprint: status bit 0x10, muster_x,
                // muster_y, muster_angle. Anything else would mean a write
                // our transcription skipped.
                for &(o, i) in &retail_changed {
                    let b = before.iter().find(|r| r.0 == o && r.1 == i).unwrap();
                    let a = want.iter().find(|r| r.0 == o && r.1 == i).unwrap();
                    assert_eq!(b.2, a.2, "valid changed");
                    for (k, (x, y)) in b.3.iter().zip(a.3.iter()).enumerate() {
                        let off = k + 2;
                        if x != y {
                            let allowed = off == 0x04 // status low byte
                                || (0x48..0x54).contains(&off); // muster_x/y/angle
                            assert!(allowed, "f{fa}->f{fb} Army[{o}][{i}] +{off:#x} {x:02x}->{y:02x} outside find_muster_spot footprint");
                        }
                    }
                    let st_b = u32::from_le_bytes(b.3[2..6].try_into().unwrap());
                    let st_a = u32::from_le_bytes(a.3[2..6].try_into().unwrap());
                    assert_eq!(st_a, st_b | 0x10, "status must gain exactly bit 0x10");
                }
            }
        }
        if capture_dirs().is_empty() {
            eprintln!("SKIP: proprietary live captures absent (schema/live/frame-pairs/*)");
            return;
        }
        // 20261004-081342-stride15 has muster slots in f220->f235 (who 6,7),
        // f235->f250 (who 2..5) and f250->f265 (who 1).
        assert!(checked >= 3, "expected stride-15 pairs with army activity, saw {checked}");
    }
}
