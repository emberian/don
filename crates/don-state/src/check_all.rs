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
use crate::walk::{CheckSum, DataWalk, WalkError};

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

// ---------------------------------------------------------------------------
// SaveGame::verify_save 0x005a76b0 — the trailer after Game::walk_rules_data.
// ---------------------------------------------------------------------------

/// Run one verify step with a fresh adler seed and append the resulting word.
fn verify_step(
    save: &mut Save,
    out: &mut Vec<(&'static str, u32)>,
    label: &'static str,
    f: impl FnOnce(&mut Save, &mut CheckSum) -> R,
) -> R {
    let mut cs = CheckSum::new(u32::MAX);
    f(save, &mut cs)?;
    out.push((label, cs.adler));
    Ok(())
}

/// The `verify_save` trailer as little-endian bytes (what the stream holds).
pub fn verify_save(save: &mut Save) -> Result<Vec<u8>, WalkError> {
    let words = verify_save_words(save)?;
    let mut out = Vec::with_capacity(words.len() * 4);
    for (_, w) in words {
        out.extend_from_slice(&w.to_le_bytes());
    }
    Ok(out)
}

/// `SaveGame::verify_save` 0x005a76b0 (vtable 0x00b35ac4 slot +0x10, called
/// by `SaveGame::do_save` 0x005a81f0 right after `WalkDataGame::walk_data`).
/// A stack `CheckSum` (seed 1, mask -1) is driven through the state walkers
/// below in this exact order; after each step the 4-byte adler word is
/// written to the stream and the seed reset. The per-tile / per-fog loops
/// make the trailer ~840 KB for a 100x100 map. Returns the emitted bytes.
pub fn verify_save_words(save: &mut Save) -> Result<Vec<(&'static str, u32)>, WalkError> {
    let mut out = Vec::new();
    let o = &mut out;
    let version = save.version;
    verify_step(save, o, "Game", |s, cs| s.game.walk(cs, version))?; // Game::walk_data 0x00589600
    verify_step(save, o, "TileSet", |s, cs| s.tileset.walk(cs))?; // 0x0087b290
    verify_step(save, o, "Mountains", |s, cs| s.mountains.walk(cs))?; // 0x0089d320
    verify_step(save, o, "empty", |_, _| Ok(()))?;
    // Constants [c061f0,+0xd40) + [+0x804,+0x808) (first dword of direct_scalars).
    verify_step(save, o, "Constants", |s, cs| {
        cs.walk_bytes("verify.constants", &mut s.constants)?;
        cs.walk_bytes("verify.const_dup", &mut s.direct_scalars[0..4])
    })?;
    // GameDaemon [c061bc,+0x28) == post_world[0..40).
    verify_step(save, o, "GameDaemon", |s, cs| cs.walk_bytes("verify.game_daemon", &mut s.post_world[0..40]))?;
    verify_step(save, o, "Armies", |s, cs| s.armies.walk(cs, "Armies", 0x006f3700))?;
    verify_step(save, o, "Cities", |s, cs| s.cities.walk(cs, "Cities", 0x00735410))?;
    verify_step(save, o, "Forms", |s, cs| s.forms.walk(cs))?; // tag + ObjectArray<Form> 0x00481190
    // Every slot of the goods PtrArray through vtable +0x7c (walk_data).
    for i in 0..save.goods.elems.len() {
        verify_step(save, o, "Good", |s, cs| match s.goods.elems[i].as_mut() {
            Some(g) => g.walk("verify.goods[]", cs),
            None => Err(cs.fail("SaveGame::verify_save", 0x005a76b0, format!("goods slot {i} is null"))),
        })?;
    }
    verify_step(save, o, "Goods", |s, cs| s.goods.walk(cs, "Goods", "Goods", 0))?; // PtrArray<Good> 0x0045cce0
    verify_step(save, o, "Items", |s, cs| s.items.walk(cs, "Items", "Items", 0))?; // PtrArray<Item> 0x0045d020
    verify_step(save, o, "Regions", |s, cs| {
        cs.walk_bytes("verify.regions.head", &mut s.post_doober)?;
        s.regions.walk(cs, "Regions", "ObjectArray<Region>", 0x00478ed0)?;
        s.wcoords.walk(cs, "Regions.wcoords", "Array<WCoordData>", 0x00478990)
    })?;
    verify_step(save, o, "Heroes", |s, cs| s.heroes.walk(cs, "Heroes", 0))?;
    verify_step(save, o, "Herds", |s, cs| s.herds.walk(cs))?;
    verify_step(save, o, "Specials", |s, cs| s.specials.walk(cs, "Specials", 0))?;
    verify_step(save, o, "Wonders", |s, cs| s.wonders.walk(cs, "Wonders", 0))?;
    verify_step(save, o, "Forts", |s, cs| s.forts.walk(cs, "Forts", 0))?;
    verify_step(save, o, "Docks", |s, cs| s.docks.walk(cs, "Docks", 0))?;
    verify_step(save, o, "OilWells", |s, cs| s.oil_wells.walk(cs, "OilWells", 0))?;
    verify_step(save, o, "Supplies", |s, cs| s.supplies.walk(cs, "Supplies", 0))?;
    verify_step(save, o, "Caravans", |s, cs| s.caravans.walk(cs, "Caravans", 0))?;
    verify_step(save, o, "Lands", |s, cs| s.lands.walk(cs))?;
    // Nine LeaderData::walk_data 0x006d6750, one word each (0xe3a390..0xe789dc).
    for i in 0..9 {
        verify_step(save, o, "LeaderData", |s, cs| s.leaders.slots[i].walk(&format!("verify.leader[{i}]"), cs))?;
    }
    verify_step(save, o, "Leaders", |s, cs| s.leaders.walk(cs))?; // tag + prod script path + 9 leaders
    // 85 TechType bytes ([tech]+0x1e2 for slots 0x880/4..0x9d4/4).
    verify_step(save, o, "Types", |s, cs| cs.walk_bytes("verify.types", &mut s.types))?;
    verify_step(save, o, "LeaderOptions", |s, cs| s.leader_options.walk(cs))?;
    verify_step(save, o, "Tribes", |s, cs| s.tribes.walk(cs))?;
    verify_step(save, o, "OptionInfo", |s, cs| s.option_info.walk(cs))?;
    verify_step(save, o, "Pathfinder", |s, cs| cs.walk_bytes("verify.pathfinder", &mut s.pathfinder))?;
    verify_step(save, o, "Groups", |s, cs| s.groups.walk(cs))?;
    verify_step(save, o, "HotKeyGroups", |s, cs| s.hotkey_groups.walk(cs))?;
    // Nine MultiPtrArray<Object>::walk_data 0x0045d550, one word each.
    for i in 0..9 {
        verify_step(save, o, "ObjectList", |s, cs| s.objects.lists[i].walk(&format!("verify.objects[{i}]"), cs, "Objects"))?;
    }
    verify_step(save, o, "Objects", |s, cs| s.objects.walk(cs))?;
    // Per tile: WorldData[+0x134] rows, first 0x15 bytes (== wdata rows).
    let tiles = save.world.wdata.len() / 21;
    for i in 0..tiles {
        verify_step(save, o, "tile", |s, cs| cs.walk_bytes("verify.tile", &mut s.world.wdata[i * 21..i * 21 + 21]))?;
    }
    // Per tile-size entry: u16 at [+0x138] (== tdata).
    let tsz = save.world.tdata.len() / 2;
    for i in 0..tsz {
        verify_step(save, o, "tdata", |s, cs| cs.walk_bytes("verify.tdata", &mut s.world.tdata[i * 2..i * 2 + 2]))?;
    }
    // CheckSums::check_seen 0x009370f0 per fog index: seen[i], seen2[i], seen3[i].
    let fog = save.world.seen.len();
    for i in 0..fog {
        verify_step(save, o, "seen", |s, cs| {
            cs.walk_bytes("verify.seen", &mut s.world.seen[i..i + 1])?;
            cs.walk_bytes("verify.seen2", &mut s.world.seen2[i..i + 1])?;
            cs.walk_bytes("verify.seen3", &mut s.world.seen3[i..i + 1])
        })?;
    }
    verify_step(save, o, "World", |s, cs| s.world.walk(cs))?; // World::walk_data(-1)
    let (xs, ys) = (save.world.xs.max(0) as usize, save.world.ys.max(0) as usize);
    verify_step(save, o, "TerrainRoads", |s, cs| s.terrain.walk(cs, xs, ys))?; // Terrain::walk_roads 0x00852b00
    verify_step(save, o, "GraphicEvents", |s, cs| s.graphic_events.walk(cs))?;
    verify_step(save, o, "empty", |_, _| Ok(()))?;
    verify_step(save, o, "Scene", |s, cs| s.scene.walk(cs))?;
    verify_step(save, o, "MessageWin", |s, cs| s.message_win.walk(cs))?;
    verify_step(save, o, "Doober", |s, cs| s.doober.walk(cs))?;
    verify_step(save, o, "Farms", |s, cs| s.farms.walk(cs))?;
    verify_step(save, o, "UnbuiltWonders", |s, cs| s.unbuilt_wonders.walk(cs, "UnbuiltWonders", 0x0073c290))?;
    verify_step(save, o, "UnbuiltCities", |s, cs| s.unbuilt_cities.walk(cs, "UnbuiltCities", 0x00460dc0))?;
    verify_step(save, o, "UnbuiltForts", |s, cs| s.unbuilt_forts.walk(cs, "UnbuiltForts", 0x0073bcc0))?;
    verify_step(save, o, "TurnControl", |s, cs| s.final_globals.walk(cs))?;
    verify_step(save, o, "check_units", CheckSums::check_units)?;
    verify_step(save, o, "check_builds", CheckSums::check_builds)?;
    verify_step(save, o, "check_walls", CheckSums::check_walls)?;
    verify_step(save, o, "check_guys", CheckSums::check_guys)?;
    verify_step(save, o, "check_ammo", CheckSums::check_ammo)?; // inline Objects list loop, flags&3
    verify_step(save, o, "check_groups", CheckSums::check_groups)?;
    // Eight LeaderData (0xe3a390..0xe71af0) in one word.
    verify_step(save, o, "Leaders8", |s, cs| {
        for i in 0..8 {
            s.leaders.slots[i].walk(&format!("verify.leaders8[{i}]"), cs)?;
        }
        Ok(())
    })?;
    verify_step(save, o, "World2", |s, cs| s.world.walk(cs))?; // gated on World+0x134 != 0
    verify_step(save, o, "check_cities", CheckSums::check_cities)?;
    verify_step(save, o, "check_goods", CheckSums::check_goods)?;
    verify_step(save, o, "check_items", CheckSums::check_items)?;
    verify_step(save, o, "RunTimeEnv", |s, cs| s.run_time_env.walk(cs))?;
    Ok(out)
}
