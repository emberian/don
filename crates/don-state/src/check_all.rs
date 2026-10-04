//! The fifteen `CheckSums::check_*` channels (`CheckSums::check_all`
//! 0x00936560), each driving a fresh `CheckSum` visitor (adler init 1, bytes
//! 0, section mask set as retail does) through the same `walk_data` bodies the
//! loader/saver use.
//!
//! Per-channel sources (re/decomp-all):
//!   units    0x009371d0 — mask = -1; 9 leader slots unrotated; leader flags&1,
//!            object flags&1, virtual walk_data (+0x7c).
//!   builds   0x00937290 — 8 leader slots; +0xac build projection then +0x7c.
//!   walls    0x00937360 — 8 leader slots; +0xb0 wall projection then +0x7c.
//!   ammo     0x009374e0 — Objects ammo list, flags&3, full walk_data.
//!   deaths   0x00936bb0 — Objects deaths array, valid!=0 rows.
//!   groups   0x00937530 — Array<Group> elements only (no header) + last_group
//!            (8 i32 at 0x00e85f4c).
//!   guys     0x00937430 — same object loop as units; FUN_0046df30 walks the
//!            object's Guy PtrArray.
//!   leaders  — 8 x LeaderData::walk_data 0x006d6750 (records 0..8).
//!   cities   0x00937600 — 8 owner lists; City flags&1; names skipped under
//!            is_checksum.
//!   items    0x00937790 — Items list, flags&1, Item/Good bodies.
//!   goods    0x00937710 — Goods list, flags&1.
//!   world    — World::walk_data(visitor, -1), conditional on a runtime flag
//!            (always set in the captured frames).
//!   rules    — Game::walk_rules_data 0x00589550.
//!   scenario — ScenarioData::walk_data 0x00997ad0.
//!   script   — RunTimeEnv::walk_data 0x009c41a0.

use crate::prim::Body;
use crate::sections::{Obj, Save};
use crate::walk::{CheckSum, WalkError};

type R = Result<(), WalkError>;

pub const CHANNEL_NAMES: [&str; 15] = [
    "units",
    "builds",
    "walls",
    "ammo",
    "deaths",
    "groups",
    "guys",
    "leaders",
    "cities",
    "items",
    "goods",
    "world",
    "rules",
    "scenario_data",
    "script_run_time",
];

/// The fifteen channel results in `check_all` order.
#[derive(Debug, Default, Clone)]
pub struct CheckSums {
    pub word: [u32; 15],
    pub bytes: [u64; 15],
}

impl CheckSums {
    /// Plain wrapping sum of the channel words (the `total` retail logs).
    pub fn total(&self) -> u32 {
        self.word.iter().fold(0u32, |a, b| a.wrapping_add(*b))
    }
}

fn channel(save: &mut Save, idx: usize, out: &mut CheckSums, f: impl Fn(&mut Save, &mut CheckSum) -> R) -> R {
    let mut cs = CheckSum::new(u32::MAX);
    f(save, &mut cs)?;
    out.word[idx] = cs.adler;
    out.bytes[idx] = cs.bytes;
    Ok(())
}

fn leader_active(save: &Save, owner: usize) -> bool {
    save.leaders
        .slots
        .get(owner)
        .is_some_and(|l| l.flags & 1 != 0)
}

/// Iterate present, flags&1 objects of one owner's object list.
fn each_object(
    save: &mut Save,
    owner: usize,
    cs: &mut CheckSum,
    mut f: impl FnMut(&mut Obj, &mut CheckSum) -> R,
) -> R {
    if !leader_active(save, owner) {
        return Ok(());
    }
    let list = match save.objects.lists.get_mut(owner) {
        Some(l) => l,
        None => return Ok(()),
    };
    for i in 0..list.elems.len() {
        if let Some(o) = list.elems[i].as_mut() {
            if o.obj_flags() & 1 != 0 {
                f(o, cs)?;
            }
        }
    }
    Ok(())
}

impl CheckSums {
    /// All fifteen channels in retail order over the typed `Save` state.
    pub fn check_all(save: &mut Save) -> Result<CheckSums, WalkError> {
        let mut out = CheckSums::default();
        channel(save, 0, &mut out, Self::check_units)?;
        channel(save, 1, &mut out, Self::check_builds)?;
        channel(save, 2, &mut out, Self::check_walls)?;
        channel(save, 3, &mut out, Self::check_ammo)?;
        channel(save, 4, &mut out, Self::check_deaths)?;
        channel(save, 5, &mut out, Self::check_groups)?;
        channel(save, 6, &mut out, Self::check_guys)?;
        channel(save, 7, &mut out, Self::check_leaders)?;
        channel(save, 8, &mut out, Self::check_cities)?;
        channel(save, 9, &mut out, Self::check_items)?;
        channel(save, 10, &mut out, Self::check_goods)?;
        channel(save, 11, &mut out, Self::check_world)?;
        channel(save, 12, &mut out, Self::check_rules)?;
        channel(save, 13, &mut out, Self::check_scenario_data)?;
        channel(save, 14, &mut out, Self::check_script_run_time)?;
        Ok(out)
    }

    /// 0x009371d0: nine leader slots, object flags&1, virtual walk_data.
    /// The per-owner pointer array it iterates is the units list — Build
    /// objects live in the separate array check_builds walks (+0xac).
    fn check_units(save: &mut Save, cs: &mut CheckSum) -> R {
        for owner in 0..9 {
            each_object(save, owner, cs, |o, w| match o {
                Obj::Build(_) => Ok(()),
                _ => o.walk("units[]", w, "Objects"),
            })?;
        }
        Ok(())
    }

    /// 0x00937290: eight leader slots; +0xac selects the Build projection, so
    /// only Build-typed objects contribute their full walk_data.
    fn check_builds(save: &mut Save, cs: &mut CheckSum) -> R {
        for owner in 0..8 {
            each_object(save, owner, cs, |o, w| {
                if let Obj::Build(b) = o {
                    b.walk("builds[]", w, "Objects")?;
                }
                Ok(())
            })?;
        }
        Ok(())
    }

    /// 0x00937360: +0xb0 wall projection. No serialized object type in these
    /// captures carries a wall body, so nothing is walked (manifest agrees:
    /// bytes 0).
    fn check_walls(_save: &mut Save, _cs: &mut CheckSum) -> R {
        Ok(())
    }

    /// 0x009374e0: Objects ammo list, flags&3, element walk_data.
    fn check_ammo(save: &mut Save, cs: &mut CheckSum) -> R {
        for i in 0..save.objects.ammo.elems.len() {
            if let Some(a) = save.objects.ammo.elems[i].as_mut() {
                if a.flags & 3 != 0 {
                    a.walk("ammo[]", cs)?;
                }
            }
        }
        Ok(())
    }

    /// 0x00936bb0: DeathObj array rows with valid != 0.
    fn check_deaths(save: &mut Save, cs: &mut CheckSum) -> R {
        for d in save.objects.deaths.elems.iter_mut() {
            if d.valid != 0 {
                d.walk("deaths[]", cs)?;
            }
        }
        Ok(())
    }

    /// 0x00937530: DAT_00e85f14 iterations of FUN_00708400 (Group::walk_data,
    /// no Array header), then the eight i32 last_group slots — fed by a raw
    /// FUN_005089d0 call that updates the adler (+0x10) but not the byte
    /// counter (+0x14).
    fn check_groups(save: &mut Save, cs: &mut CheckSum) -> R {
        for i in 0..save.groups.list.elems.len() {
            save.groups.list.elems[i].walk(&format!("groups[{i}]"), cs, "Group")?;
        }
        for i in 0..8 {
            let v = i32::from_le_bytes(save.groups.last_group[i * 4..i * 4 + 4].try_into().unwrap());
            cs.feed(&v.to_le_bytes());
        }
        Ok(())
    }

    /// 0x00937430: same nine-slot object loop as check_units; walks each
    /// object's Guy PtrArray (FUN_0046df30).
    fn check_guys(save: &mut Save, cs: &mut CheckSum) -> R {
        for owner in 0..9 {
            each_object(save, owner, cs, |o, w| match o {
                Obj::Unit(u) => u.guys.walk(w, "guys[]", "Unit", 0),
                Obj::Animal(a) => a.unit.guys.walk(w, "guys[]", "Animal", 0),
                Obj::Build(_) => Ok(()),
            })?;
        }
        Ok(())
    }

    /// Leaders channel: LeaderData::walk_data for records 0..8.
    fn check_leaders(save: &mut Save, cs: &mut CheckSum) -> R {
        for i in 0..8 {
            if let Some(l) = save.leaders.slots.get_mut(i) {
                l.walk(&format!("leaders[{i}]"), cs)?;
            }
        }
        Ok(())
    }

    /// 0x00937600: eight owner lists; City::walk_data (name/id strings are
    /// gated off by is_checksum inside the body).
    fn check_cities(save: &mut Save, cs: &mut CheckSum) -> R {
        for owner in 0..8 {
            if !leader_active(save, owner) {
                continue;
            }
            if let Some(list) = save.cities.lists.get_mut(owner) {
                for i in 0..list.elems.len() {
                    if let Some(c) = list.elems[i].as_mut() {
                        if c.flags & 1 != 0 {
                            c.walk(&format!("cities[{i}]"), cs)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// 0x00937790 / 0x00937710: Items / Goods lists, flags&1, element bodies.
    fn check_items(save: &mut Save, cs: &mut CheckSum) -> R {
        for i in 0..save.items.elems.len() {
            if let Some(it) = save.items.elems[i].as_mut() {
                if it.flags & 1 != 0 {
                    it.walk(&format!("items[{i}]"), cs)?;
                }
            }
        }
        Ok(())
    }

    fn check_goods(save: &mut Save, cs: &mut CheckSum) -> R {
        for i in 0..save.goods.elems.len() {
            if let Some(g) = save.goods.elems[i].as_mut() {
                if g.flags & 1 != 0 {
                    g.walk(&format!("goods[{i}]"), cs)?;
                }
            }
        }
        Ok(())
    }

    /// World::walk_data(visitor, -1); the retail conditional flag was set in
    /// all captured frames.
    fn check_world(save: &mut Save, cs: &mut CheckSum) -> R {
        save.world.walk(cs)
    }

    /// Game::walk_rules_data 0x00589550 — the same typed `Rules::walk` used by
    /// load/save; tags no-op and checksum-gated strings are skipped under
    /// `is_checksum()`.
    fn check_rules(save: &mut Save, cs: &mut CheckSum) -> R {
        save.rules_tail.rules.walk(cs)
    }

    /// ScenarioData::walk_data 0x00997ad0.
    fn check_scenario_data(save: &mut Save, cs: &mut CheckSum) -> R {
        save.scenario.walk(cs)
    }

    /// RunTimeEnv::walk_data 0x009c41a0.
    fn check_script_run_time(save: &mut Save, cs: &mut CheckSum) -> R {
        save.run_time_env.walk(cs)
    }
}
