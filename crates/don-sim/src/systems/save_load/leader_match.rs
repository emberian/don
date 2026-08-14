//! DoNSave owner for the authoritative victory `Leaders`/`Match` pair.
//!
//! The older `PLAYER_SETUP` section is a reconstructive recipe: it can recreate the
//! frame-zero transaction, but not runtime diplomacy, victory/defeat flags, timers, score
//! inputs, Game semaphores, or lifecycle `Player` rows. This section stores those owners
//! directly. Setup is still reconstructed first so its immutable receipt remains the one
//! authority; this snapshot then restores the mutable mid-match rows over that owner.

use super::{Reader, SaveError, Writer};
use crate::systems::army_do_mustering::MUSTER_STRATEGY_REGIONS;
use crate::systems::leader_init_diplomacy_loop::LeaderInitDiplomacyRow;
use crate::systems::leader_production_ai::strategy_runtime::{
    CanonicalProductionAi, LEADER_MATCH_AI_EXTENSION_VALUES, LEADER_MATCH_AI_FORMAT_VERSION,
};
use crate::systems::tech_cities::{
    CaravanLink, CaravanLinkArray, CityPool, CityRecord, CITIES_PER_PLAYER,
    NUM_PLAYERS as CITY_PLAYERS,
};
use crate::systems::victory_score::{
    DefeatType, EncryptedEconomy, LeaderState, Leaders, Match, MatchEvent, ScoreConstants,
    TypeKind, TypeRow, TypeTable, VictoryOptions, VictoryType, NUM_BUILD_SLOTS, NUM_LEADERS,
    NUM_REG_BUILDING_SLOTS, NUM_RESOURCES, NUM_TYPES, NUM_UNIT_SLOTS,
};
use crate::tick::lifecycle_host::PlayerTable;
use crate::tick::Sim;

const MAX_CATEGORY_ROWS: usize = 4096;
const MAX_EVENTS: usize = 256;
const MAX_CITY_ROWS_PER_PLAYER: usize = 4096;
const MAX_CITY_CARAVAN_LINKS: usize = 1 << 16;
const MAX_CITY_STRING_BYTES: usize = 1 << 20;
const CITY_POOL_FORMAT_VERSION: u32 = 12;
/// First DoNSave version that carries `LeaderData::strategy[64]`, now consumed by the exact
/// released-land `Army::do_mustering` transaction.
pub(crate) const LEADER_MATCH_MUSTER_STRATEGY_FORMAT_VERSION: u32 = 21;
/// First DoNSave version that carries mutable `GameInfo::difficulty`.
pub(crate) const LEADER_MATCH_GAME_INFO_DIFFICULTY_FORMAT_VERSION: u32 = 22;
/// First DoNSave version carrying the live Building activation counters needed by the
/// golden frame-zero Market continuation.
pub(crate) const LEADER_MATCH_BUILD_ACCOUNTING_FORMAT_VERSION: u32 = 23;

pub(super) struct LeaderMatchState {
    game: Match,
    leaders: Leaders,
    players: Option<PlayerTable>,
    cities: CityPool,
}

fn write_i32s(w: &mut Writer, values: &[i32]) {
    for &value in values {
        w.i32(value);
    }
}

fn read_i32_array<const N: usize>(r: &mut Reader<'_>) -> Result<[i32; N], SaveError> {
    let mut values = [0; N];
    for value in &mut values {
        *value = r.i32()?;
    }
    Ok(values)
}

fn write_u16s(w: &mut Writer, values: &[u16]) {
    for &value in values {
        w.u16(value);
    }
}

fn read_u16s(r: &mut Reader<'_>, n: usize) -> Result<Vec<u16>, SaveError> {
    (0..n).map(|_| r.u16()).collect()
}

fn write_bools(w: &mut Writer, values: &[bool]) {
    for &value in values {
        w.bool(value);
    }
}

fn read_bools(r: &mut Reader<'_>, n: usize) -> Result<Vec<bool>, SaveError> {
    (0..n).map(|_| r.bool()).collect()
}

fn write_string(w: &mut Writer, value: &str, what: &'static str) -> Result<(), SaveError> {
    if value.len() > MAX_CITY_STRING_BYTES {
        return Err(SaveError::Limit(what));
    }
    w.len(value.len(), what)?;
    w.bytes(value.as_bytes());
    Ok(())
}

fn read_string(r: &mut Reader<'_>, max: usize, what: &'static str) -> Result<String, SaveError> {
    let len = r.len(max, what)?;
    String::from_utf8(r.take(len)?.to_vec()).map_err(|_| SaveError::Invalid("city string utf-8"))
}

fn write_city(w: &mut Writer, city: &CityRecord) -> Result<(), SaveError> {
    w.u16(city.city_flags);
    w.i16(city.city);
    w.i16(city.o);
    w.i16(city.reg);
    for value in [
        city.x,
        city.y,
        city.attack_stamp,
        city.raid_stamp,
        city.reduce_stamp,
        city.capture_stamp,
        city.assimilation_timer,
        city.capture_strength,
    ] {
        w.i32(value);
    }
    write_i32s(w, &city.traded_with);
    for value in [
        city.scouted,
        city.in_port,
        city.peasant_dist,
        city.trade_val,
        city.conquest_node,
    ] {
        w.i16(value);
    }
    for value in [
        city.granary,
        city.lumber_mill,
        city.smelter,
        city.refinery,
        city.free,
        city.busy,
        city.gatherers,
        city.pop,
        city.who as u8,
        city.race as u8,
        city.founder as u8,
        city.plundered,
        city.ocean,
        city.land,
        city.filled,
        city.bordering,
        city.ocean_filled,
        city.dock_tile,
        city.was_capital_flags,
    ] {
        w.u8(value);
    }
    w.bytes(&city.space);
    w.bytes(&city.ter);
    if city.vans.capacity < city.vans.items.len() as i32 {
        return Err(SaveError::Invalid("city caravan capacity"));
    }
    w.len(city.vans.items.len(), "city caravan links")?;
    w.i32(city.vans.capacity);
    w.i16(city.vans.grow);
    w.u8(city.vans.flags);
    for link in &city.vans.items {
        w.i32(link.cara);
        w.i32(link.who);
    }
    write_string(w, &city.name, "city name bytes")?;
    write_string(w, &city.id, "city id bytes")?;
    Ok(())
}

fn read_city(r: &mut Reader<'_>) -> Result<CityRecord, SaveError> {
    let city_flags = r.u16()?;
    let city = r.i16()?;
    let o = r.i16()?;
    let reg = r.i16()?;
    let x = r.i32()?;
    let y = r.i32()?;
    let attack_stamp = r.i32()?;
    let raid_stamp = r.i32()?;
    let reduce_stamp = r.i32()?;
    let capture_stamp = r.i32()?;
    let assimilation_timer = r.i32()?;
    let capture_strength = r.i32()?;
    let traded_with = read_i32_array(r)?;
    let scouted = r.i16()?;
    let in_port = r.i16()?;
    let peasant_dist = r.i16()?;
    let trade_val = r.i16()?;
    let conquest_node = r.i16()?;
    let granary = r.u8()?;
    let lumber_mill = r.u8()?;
    let smelter = r.u8()?;
    let refinery = r.u8()?;
    let free = r.u8()?;
    let busy = r.u8()?;
    let gatherers = r.u8()?;
    let pop = r.u8()?;
    let who = r.i8()?;
    let race = r.i8()?;
    let founder = r.i8()?;
    let plundered = r.u8()?;
    let ocean = r.u8()?;
    let land = r.u8()?;
    let filled = r.u8()?;
    let bordering = r.u8()?;
    let ocean_filled = r.u8()?;
    let dock_tile = r.u8()?;
    let was_capital_flags = r.u8()?;
    let mut space = [0u8; 3];
    space.copy_from_slice(r.take(3)?);
    let mut ter = [0u8; 6];
    ter.copy_from_slice(r.take(6)?);
    let links = r.len(MAX_CITY_CARAVAN_LINKS, "city caravan links")?;
    let capacity = r.i32()?;
    if capacity < links as i32 {
        return Err(SaveError::Invalid("city caravan capacity"));
    }
    let grow = r.i16()?;
    let flags = r.u8()?;
    let items = (0..links)
        .map(|_| {
            Ok(CaravanLink {
                cara: r.i32()?,
                who: r.i32()?,
            })
        })
        .collect::<Result<Vec<_>, SaveError>>()?;
    let name = read_string(r, MAX_CITY_STRING_BYTES, "city name bytes")?;
    let id = read_string(r, MAX_CITY_STRING_BYTES, "city id bytes")?;
    Ok(CityRecord {
        city_flags,
        city,
        o,
        reg,
        x,
        y,
        attack_stamp,
        raid_stamp,
        reduce_stamp,
        capture_stamp,
        assimilation_timer,
        capture_strength,
        traded_with,
        scouted,
        in_port,
        peasant_dist,
        trade_val,
        conquest_node,
        granary,
        lumber_mill,
        smelter,
        refinery,
        free,
        busy,
        gatherers,
        pop,
        who,
        race,
        founder,
        plundered,
        ocean,
        land,
        filled,
        bordering,
        ocean_filled,
        dock_tile,
        was_capital_flags,
        space,
        ter,
        vans: CaravanLinkArray {
            items,
            capacity,
            grow,
            flags,
        },
        name,
        id,
    })
}

fn write_cities(w: &mut Writer, cities: &CityPool) -> Result<(), SaveError> {
    for who in 0..CITY_PLAYERS {
        let rows = &cities.slots[who];
        let mark = cities.city_mark[who];
        if rows.len() < CITIES_PER_PLAYER
            || rows.len() > MAX_CITY_ROWS_PER_PLAYER
            || mark < 0
            || mark as usize > rows.len()
        {
            return Err(SaveError::Invalid("city pool shape"));
        }
        w.i32(mark);
        w.len(rows.len(), "city rows per player")?;
        for city in rows {
            write_city(w, city)?;
        }
    }
    Ok(())
}

fn read_cities(r: &mut Reader<'_>) -> Result<CityPool, SaveError> {
    let mut pool = CityPool::new();
    for who in 0..CITY_PLAYERS {
        let mark = r.i32()?;
        let rows = r.len(MAX_CITY_ROWS_PER_PLAYER, "city rows per player")?;
        if rows < CITIES_PER_PLAYER || mark < 0 || mark as usize > rows {
            return Err(SaveError::Invalid("city pool shape"));
        }
        pool.city_mark[who] = mark;
        pool.slots[who] = (0..rows).map(|_| read_city(r)).collect::<Result<_, _>>()?;
    }
    Ok(pool)
}

fn write_score_constants(w: &mut Writer, c: ScoreConstants) {
    for value in [
        c.unit_cost_factor,
        c.build_cost_factor,
        c.tech_cost_factor,
        c.spell_cost_factor,
        c.build_support_factor,
        c.research_premium,
        c.retake_capital,
        c.wonder_timer,
        c.wonder_age,
        c.popwin_timer,
        c.armageddon,
        c.armageddon_per_nation,
        c.armageddon_per_team,
    ] {
        w.i32(value);
    }
}

fn read_score_constants(r: &mut Reader<'_>) -> Result<ScoreConstants, SaveError> {
    Ok(ScoreConstants {
        unit_cost_factor: r.i32()?,
        build_cost_factor: r.i32()?,
        tech_cost_factor: r.i32()?,
        spell_cost_factor: r.i32()?,
        build_support_factor: r.i32()?,
        research_premium: r.i32()?,
        retake_capital: r.i32()?,
        wonder_timer: r.i32()?,
        wonder_age: r.i32()?,
        popwin_timer: r.i32()?,
        armageddon: r.i32()?,
        armageddon_per_nation: r.i32()?,
        armageddon_per_team: r.i32()?,
    })
}

fn write_i32_vec(w: &mut Writer, values: &[i32]) -> Result<(), SaveError> {
    w.len(values.len(), "victory category rows")?;
    write_i32s(w, values);
    Ok(())
}

fn read_i32_vec(r: &mut Reader<'_>) -> Result<Vec<i32>, SaveError> {
    let n = r.len(MAX_CATEGORY_ROWS, "victory category rows")?;
    (0..n).map(|_| r.i32()).collect()
}

fn write_victory_options(w: &mut Writer, v: &VictoryOptions) -> Result<(), SaveError> {
    for values in [
        &v.scores,
        &v.time_limits,
        &v.chairs,
        &v.wonderwins,
        &v.popwins,
        &v.econwins,
        &v.map_sizes,
    ] {
        write_i32_vec(w, values)?;
    }
    Ok(())
}

fn read_victory_options(r: &mut Reader<'_>) -> Result<VictoryOptions, SaveError> {
    Ok(VictoryOptions {
        scores: read_i32_vec(r)?,
        time_limits: read_i32_vec(r)?,
        chairs: read_i32_vec(r)?,
        wonderwins: read_i32_vec(r)?,
        popwins: read_i32_vec(r)?,
        econwins: read_i32_vec(r)?,
        map_sizes: read_i32_vec(r)?,
    })
}

fn write_type_table(w: &mut Writer, table: &TypeTable) -> Result<(), SaveError> {
    if table.rows.len() != NUM_TYPES {
        return Err(SaveError::Invalid("victory type table length"));
    }
    write_score_constants(w, table.constants);
    for row in &table.rows {
        write_i32s(w, &row.costs);
        w.u8(match row.kind {
            TypeKind::Unit => 0,
            TypeKind::Build => 1,
            TypeKind::Tech => 2,
            TypeKind::Spell => 3,
            TypeKind::Other => 4,
        });
        w.bool(row.is_wonder);
        write_i32s(w, &row.support_cost);
        w.i32(row.attack);
        w.i32(row.unit_flags);
        w.i32(row.research_premium_cost);
    }
    Ok(())
}

fn read_type_table(r: &mut Reader<'_>) -> Result<TypeTable, SaveError> {
    let constants = read_score_constants(r)?;
    let mut rows = Vec::with_capacity(NUM_TYPES);
    for _ in 0..NUM_TYPES {
        let costs = read_i32_array::<NUM_RESOURCES>(r)?;
        let kind = match r.u8()? {
            0 => TypeKind::Unit,
            1 => TypeKind::Build,
            2 => TypeKind::Tech,
            3 => TypeKind::Spell,
            4 => TypeKind::Other,
            _ => return Err(SaveError::Invalid("victory type kind")),
        };
        rows.push(TypeRow {
            costs,
            kind,
            is_wonder: r.bool()?,
            support_cost: read_i32_array::<2>(r)?,
            attack: r.i32()?,
            unit_flags: r.i32()?,
            research_premium_cost: r.i32()?,
        });
    }
    Ok(TypeTable { rows, constants })
}

fn write_match(w: &mut Writer, game: &Match, format_version: u32) -> Result<(), SaveError> {
    super::write_match_options(w, game.options, format_version)?;
    write_score_constants(w, game.constants);
    write_victory_options(w, &game.victory_options)?;
    w.i32(game.frame);
    w.i32(game.tick);
    write_i32s(w, &game.starting);
    write_i32s(w, &game.on_team);
    for value in [
        game.num_nations,
        game.num_sides,
        game.musical_chairs,
        game.armageddon,
    ] {
        w.i32(value);
    }
    w.u32(game.semaphore);
    w.i32(game.world_xs);
    w.i32(game.world_land_size);
    Ok(())
}

fn read_match(r: &mut Reader<'_>, format_version: u32) -> Result<Match, SaveError> {
    Ok(Match {
        options: super::read_match_options(r, format_version)?,
        constants: read_score_constants(r)?,
        victory_options: read_victory_options(r)?,
        frame: r.i32()?,
        tick: r.i32()?,
        starting: read_i32_array::<NUM_RESOURCES>(r)?,
        on_team: read_i32_array::<NUM_LEADERS>(r)?,
        num_nations: r.i32()?,
        num_sides: r.i32()?,
        musical_chairs: r.i32()?,
        armageddon: r.i32()?,
        semaphore: r.u32()?,
        world_xs: r.i32()?,
        world_land_size: r.i32()?,
    })
}

fn validate_leader(row: &LeaderState) -> Result<(), SaveError> {
    if row.num_buildings.len() != NUM_BUILD_SLOTS
        || row.high_buildings.len() != NUM_BUILD_SLOTS
        || row.reg_buildings.len() != NUM_REG_BUILDING_SLOTS
        || row.num_units.len() != NUM_UNIT_SLOTS
        || row.num_queued.len() != NUM_TYPES
        || row.tech_at_start.len() != NUM_TYPES.div_ceil(8)
        || row.has_tech.len() != NUM_TYPES
        || row.researching[0].len() != NUM_TYPES
        || row.researching[1].len() != NUM_TYPES
    {
        return Err(SaveError::Invalid("victory leader vector length"));
    }
    Ok(())
}

fn write_init_row(w: &mut Writer, row: &LeaderInitDiplomacyRow) {
    for values in [
        &row.treaties,
        &row.agendas,
        &row.good_deeds,
        &row.attack_stamp,
        &row.raid_stamp,
        &row.capital_stamp,
        &row.ally_stamp,
        &row.tribute_stamp,
        &row.gift_stamp,
        &row.hire_stamp,
        &row.hire_who,
        &row.aggression,
        &row.strong,
        &row.weak,
        &row.dow,
        &row.invaders,
        &row.broke_alliance,
        &row.made_peace,
    ] {
        write_i32s(w, values);
    }
    w.i32(row.got_diplo_message);
    for values in [
        &row.last_spoke,
        &row.counteroffer,
        &row.tribute_demanded,
        &row.last_taunt,
        &row.taunt_frame,
    ] {
        write_i32s(w, values);
    }
    w.u8(row.ally_mask);
}

fn read_init_row(r: &mut Reader<'_>) -> Result<LeaderInitDiplomacyRow, SaveError> {
    Ok(LeaderInitDiplomacyRow {
        treaties: read_i32_array(r)?,
        agendas: read_i32_array(r)?,
        good_deeds: read_i32_array(r)?,
        attack_stamp: read_i32_array(r)?,
        raid_stamp: read_i32_array(r)?,
        capital_stamp: read_i32_array(r)?,
        ally_stamp: read_i32_array(r)?,
        tribute_stamp: read_i32_array(r)?,
        gift_stamp: read_i32_array(r)?,
        hire_stamp: read_i32_array(r)?,
        hire_who: read_i32_array(r)?,
        aggression: read_i32_array(r)?,
        strong: read_i32_array(r)?,
        weak: read_i32_array(r)?,
        dow: read_i32_array(r)?,
        invaders: read_i32_array(r)?,
        broke_alliance: read_i32_array(r)?,
        made_peace: read_i32_array(r)?,
        got_diplo_message: r.i32()?,
        last_spoke: read_i32_array(r)?,
        counteroffer: read_i32_array(r)?,
        tribute_demanded: read_i32_array(r)?,
        last_taunt: read_i32_array(r)?,
        taunt_frame: read_i32_array(r)?,
        ally_mask: r.u8()?,
    })
}

fn write_leader(w: &mut Writer, row: &LeaderState, format_version: u32) -> Result<(), SaveError> {
    validate_leader(row)?;
    for value in [
        row.leader_flags,
        row.leader_flags2,
        row.who,
        row.defeated_by,
        row.score,
        row.score_explored,
        row.score_territory,
        row.score_units,
        row.score_units_2,
        row.score_buildings,
        row.score_economy,
        row.score_pop,
        row.score_unit_upgrades,
        row.score_research,
        row.score_wonders,
        row.score_combat,
        row.multi_diff,
    ] {
        w.i32(value);
    }
    write_i32s(w, &row.diplos);
    write_init_row(w, &row.init_diplomacy);
    for value in [
        row.popwin_stamp,
        row.popwin_timer,
        row.wonderwin_stamp,
        row.wonderwin_timer,
        row.lost_capital_stamp,
        row.lost_capital_timer,
        row.victory_type,
        row.defeat_type,
        row.population_cap,
        row.misery,
        row.give_attrition_disabled,
        row.take_attrition_disabled,
        row.neutral_attrition,
        row.building_attrition_disabled,
    ] {
        w.i32(value);
    }
    if format_version >= CITY_POOL_FORMAT_VERSION {
        w.i32(row.cities_captured);
        w.i32(row.cities_lost);
    } else if row.cities_captured != 0 || row.cities_lost != 0 {
        return Err(SaveError::Unsupported("City capture counters"));
    }
    write_u16s(w, &row.num_buildings);
    write_u16s(w, &row.num_units);
    write_u16s(w, &row.num_queued);
    w.bytes(&row.tech_at_start);
    w.u32(row.rare as u32);
    w.u32((row.rare >> 32) as u32);
    w.i32(row.territory);
    write_i32s(w, &row.economy.bucket);
    write_i32s(w, &row.economy.income);
    write_bools(w, &row.has_tech);
    write_bools(w, &row.researching[0]);
    write_bools(w, &row.researching[1]);
    write_bools(w, &row.resource_avail);
    w.i32(row.economic);
    w.bool(row.has_preq_2b0);
    w.bool(row.has_preq_2b9);
    let production_ai = CanonicalProductionAi {
        leader_flags2: row.leader_flags2,
        production_step: row.production_step,
        prod_script_run: row.prod_script_run,
        script_step: row.script_step,
        control: row.control,
        effective_pop: row.effective_pop,
    };
    if format_version >= LEADER_MATCH_AI_FORMAT_VERSION {
        write_i32s(w, &production_ai.extension_values());
    } else if production_ai.extension_values() != [0; LEADER_MATCH_AI_EXTENSION_VALUES] {
        return Err(SaveError::Unsupported("production AI Leader extension"));
    }
    if format_version >= LEADER_MATCH_MUSTER_STRATEGY_FORMAT_VERSION {
        write_u16s(w, &row.strategy);
    } else if row.strategy != [0; MUSTER_STRATEGY_REGIONS] {
        return Err(SaveError::Unsupported(
            "Army muster strategy Leader extension",
        ));
    }
    if format_version >= LEADER_MATCH_BUILD_ACCOUNTING_FORMAT_VERSION {
        w.i32(row.buildings_built);
        write_i32s(w, &row.gather_slots);
        write_i32s(w, &row.gather_slots_high);
        write_u16s(w, &row.high_buildings);
        write_u16s(w, &row.reg_buildings);
    } else if row.buildings_built != 0
        || row.gather_slots != [0; NUM_RESOURCES]
        || row.gather_slots_high != [0; NUM_RESOURCES]
        || row.high_buildings.iter().any(|&value| value != 0)
        || row.reg_buildings.iter().any(|&value| value != 0)
    {
        return Err(SaveError::Unsupported(
            "Leader Building-accounting extension",
        ));
    }
    Ok(())
}

fn read_leader(r: &mut Reader<'_>, format_version: u32) -> Result<LeaderState, SaveError> {
    let leader_flags = r.i32()?;
    let leader_flags2 = r.i32()?;
    let who = r.i32()?;
    let defeated_by = r.i32()?;
    let score = r.i32()?;
    let score_explored = r.i32()?;
    let score_territory = r.i32()?;
    let score_units = r.i32()?;
    let score_units_2 = r.i32()?;
    let score_buildings = r.i32()?;
    let score_economy = r.i32()?;
    let score_pop = r.i32()?;
    let score_unit_upgrades = r.i32()?;
    let score_research = r.i32()?;
    let score_wonders = r.i32()?;
    let score_combat = r.i32()?;
    let multi_diff = r.i32()?;
    let diplos = read_i32_array(r)?;
    let init_diplomacy = read_init_row(r)?;
    let popwin_stamp = r.i32()?;
    let popwin_timer = r.i32()?;
    let wonderwin_stamp = r.i32()?;
    let wonderwin_timer = r.i32()?;
    let lost_capital_stamp = r.i32()?;
    let lost_capital_timer = r.i32()?;
    let victory_type = r.i32()?;
    let defeat_type = r.i32()?;
    let population_cap = r.i32()?;
    let misery = r.i32()?;
    let give_attrition_disabled = r.i32()?;
    let take_attrition_disabled = r.i32()?;
    let neutral_attrition = r.i32()?;
    let building_attrition_disabled = r.i32()?;
    let (cities_captured, cities_lost) = if format_version >= CITY_POOL_FORMAT_VERSION {
        (r.i32()?, r.i32()?)
    } else {
        (0, 0)
    };
    let num_buildings = read_u16s(r, NUM_BUILD_SLOTS)?;
    let num_units = read_u16s(r, NUM_UNIT_SLOTS)?;
    let num_queued = read_u16s(r, NUM_TYPES)?;
    let tech_at_start = r.take(NUM_TYPES.div_ceil(8))?.to_vec();
    let rare = u64::from(r.u32()?) | (u64::from(r.u32()?) << 32);
    let territory = r.i32()?;
    let economy = EncryptedEconomy {
        bucket: read_i32_array(r)?,
        income: read_i32_array(r)?,
    };
    let has_tech = read_bools(r, NUM_TYPES)?;
    let researching = [read_bools(r, NUM_TYPES)?, read_bools(r, NUM_TYPES)?];
    let mut resource_avail = [false; NUM_RESOURCES];
    for value in &mut resource_avail {
        *value = r.bool()?;
    }
    let economic = r.i32()?;
    let has_preq_2b0 = r.bool()?;
    let has_preq_2b9 = r.bool()?;
    let extension = if format_version >= LEADER_MATCH_AI_FORMAT_VERSION {
        Some(read_i32_array(r)?)
    } else {
        None
    };
    let production_ai =
        CanonicalProductionAi::for_save_version(format_version, leader_flags2, extension)
            .map_err(|_| SaveError::Invalid("production AI Leader extension"))?;
    let strategy = if format_version >= LEADER_MATCH_MUSTER_STRATEGY_FORMAT_VERSION {
        read_u16s(r, MUSTER_STRATEGY_REGIONS)?
            .try_into()
            .map_err(|_| SaveError::Invalid("Army muster strategy Leader extension"))?
    } else {
        [0; MUSTER_STRATEGY_REGIONS]
    };
    let (buildings_built, gather_slots, gather_slots_high, high_buildings, reg_buildings) =
        if format_version >= LEADER_MATCH_BUILD_ACCOUNTING_FORMAT_VERSION {
            (
                r.i32()?,
                read_i32_array(r)?,
                read_i32_array(r)?,
                read_u16s(r, NUM_BUILD_SLOTS)?,
                read_u16s(r, NUM_REG_BUILDING_SLOTS)?,
            )
        } else {
            (
                0,
                [0; NUM_RESOURCES],
                [0; NUM_RESOURCES],
                vec![0; NUM_BUILD_SLOTS],
                vec![0; NUM_REG_BUILDING_SLOTS],
            )
        };
    Ok(LeaderState {
        leader_flags,
        leader_flags2,
        who,
        defeated_by,
        score,
        score_explored,
        score_territory,
        score_units,
        score_units_2,
        score_buildings,
        score_economy,
        score_pop,
        score_unit_upgrades,
        score_research,
        score_wonders,
        score_combat,
        multi_diff,
        diplos,
        init_diplomacy,
        popwin_stamp,
        popwin_timer,
        wonderwin_stamp,
        wonderwin_timer,
        lost_capital_stamp,
        lost_capital_timer,
        production_step: production_ai.production_step,
        prod_script_run: production_ai.prod_script_run,
        script_step: production_ai.script_step,
        victory_type,
        defeat_type,
        population_cap,
        misery,
        give_attrition_disabled,
        take_attrition_disabled,
        neutral_attrition,
        building_attrition_disabled,
        cities_captured,
        cities_lost,
        buildings_built,
        gather_slots,
        gather_slots_high,
        control: production_ai.control,
        num_buildings,
        high_buildings,
        reg_buildings,
        num_units,
        num_queued,
        tech_at_start,
        rare,
        territory,
        effective_pop: production_ai.effective_pop,
        strategy,
        economy,
        has_tech,
        researching,
        resource_avail,
        economic,
        has_preq_2b0,
        has_preq_2b9,
    })
}

fn write_event(w: &mut Writer, event: &MatchEvent) {
    match *event {
        MatchEvent::Victory { who, victory_type } => {
            w.u8(0);
            w.u8(who as u8);
            w.i32(victory_type as i32);
        }
        MatchEvent::Defeat { who, defeat_type } => {
            w.u8(1);
            w.u8(who as u8);
            w.i32(defeat_type as i32);
        }
        MatchEvent::ArmageddonAll => w.u8(2),
        MatchEvent::GameOver => w.u8(3),
    }
}

fn victory_type(value: i32) -> Result<VictoryType, SaveError> {
    Ok(match value {
        0 => VictoryType::Generic,
        1 => VictoryType::ByWonder,
        2 => VictoryType::ByTerritory,
        3 => VictoryType::ByTechRace,
        4 => VictoryType::ByScore,
        5 => VictoryType::ByEconomy,
        6 => VictoryType::ByTimeLimit,
        _ => return Err(SaveError::Invalid("victory event type")),
    })
}

fn read_event(r: &mut Reader<'_>) -> Result<MatchEvent, SaveError> {
    Ok(match r.u8()? {
        0 => {
            let who = r.u8()? as usize;
            if who >= NUM_LEADERS {
                return Err(SaveError::Invalid("victory event leader"));
            }
            MatchEvent::Victory {
                who,
                victory_type: victory_type(r.i32()?)?,
            }
        }
        1 => {
            let who = r.u8()? as usize;
            if who >= NUM_LEADERS {
                return Err(SaveError::Invalid("defeat event leader"));
            }
            MatchEvent::Defeat {
                who,
                defeat_type: DefeatType::from_i32(r.i32()?)
                    .ok_or(SaveError::Invalid("defeat event type"))?,
            }
        }
        2 => MatchEvent::ArmageddonAll,
        3 => MatchEvent::GameOver,
        _ => return Err(SaveError::Invalid("match event tag")),
    })
}

fn write_players(w: &mut Writer, players: Option<&PlayerTable>) {
    w.bool(players.is_some());
    let Some(players) = players else { return };
    for row in players.players {
        w.u16(row.flags);
        w.u8(row.who);
        w.i8(row.team);
        w.u8(row.play);
    }
    w.i32(players.playing);
    w.i32(players.semaphore_flags);
    w.i32(players.console_play);
    w.i32(players.console_who);
    w.bool(players.drop_window_open);
}

fn read_players(r: &mut Reader<'_>) -> Result<Option<PlayerTable>, SaveError> {
    if !r.bool()? {
        return Ok(None);
    }
    let mut players = PlayerTable::new();
    for (slot, row) in players.players.iter_mut().enumerate() {
        row.flags = r.u16()?;
        row.who = r.u8()?;
        row.team = r.i8()?;
        row.play = r.u8()?;
        if row.play as usize != slot || row.who as usize >= NUM_LEADERS {
            return Err(SaveError::Invalid("lifecycle player identity"));
        }
    }
    players.playing = r.i32()?;
    players.semaphore_flags = r.i32()?;
    players.console_play = r.i32()?;
    players.console_who = r.i32()?;
    players.drop_window_open = r.bool()?;
    Ok(Some(players))
}

pub(super) fn write(sim: &Sim) -> Result<Vec<u8>, SaveError> {
    write_for_version(sim, LEADER_MATCH_BUILD_ACCOUNTING_FORMAT_VERSION)
}

pub(super) fn write_for_version(sim: &Sim, format_version: u32) -> Result<Vec<u8>, SaveError> {
    if sim.vic_match.frame != sim.world.frame || sim.vic_match.tick != sim.world.seconds {
        return Err(SaveError::Invalid("victory match/world clock mismatch"));
    }
    if sim.vic_leaders.slots.len() != NUM_LEADERS {
        return Err(SaveError::Invalid("victory leader slot count"));
    }
    if sim.vic_leaders.pending_cleanup_masks() != (0, 0) || sim.defeat_cleanup_error.is_some() {
        return Err(SaveError::Unsupported("pending terminal leader cleanup"));
    }
    let mut w = Writer::default();
    write_match(&mut w, &sim.vic_match, format_version)?;
    write_type_table(&mut w, &sim.vic_leaders.types)?;
    for (slot, leader) in sim.vic_leaders.slots.iter().enumerate() {
        if leader.who != slot as i32 {
            return Err(SaveError::Invalid("victory leader identity"));
        }
        write_leader(&mut w, leader, format_version)?;
    }
    w.len(sim.vic_leaders.events.len(), "match events")?;
    if sim.vic_leaders.events.len() > MAX_EVENTS {
        return Err(SaveError::Limit("match events"));
    }
    for event in &sim.vic_leaders.events {
        write_event(&mut w, event);
    }
    write_players(&mut w, sim.players.as_ref());
    if format_version >= CITY_POOL_FORMAT_VERSION {
        write_cities(&mut w, &sim.cities)?;
    } else {
        let pristine = CityPool::new();
        if sim.cities.city_mark != pristine.city_mark || sim.cities.slots != pristine.slots {
            return Err(SaveError::Unsupported("Cities pool"));
        }
    }
    Ok(w.0)
}

pub(super) fn read(data: &[u8], format_version: u32) -> Result<LeaderMatchState, SaveError> {
    let mut r = Reader::new(data);
    let game = read_match(&mut r, format_version)?;
    let types = read_type_table(&mut r)?;
    let mut leaders = Leaders::new(types);
    for leader in &mut leaders.slots {
        *leader = read_leader(&mut r, format_version)?;
    }
    for (slot, leader) in leaders.slots.iter().enumerate() {
        if leader.who != slot as i32 {
            return Err(SaveError::Invalid("victory leader identity"));
        }
    }
    let events = r.len(MAX_EVENTS, "match events")?;
    leaders.events = (0..events)
        .map(|_| read_event(&mut r))
        .collect::<Result<_, _>>()?;
    let players = read_players(&mut r)?;
    let cities = if format_version >= CITY_POOL_FORMAT_VERSION {
        read_cities(&mut r)?
    } else {
        CityPool::new()
    };
    r.finish()?;
    Ok(LeaderMatchState {
        game,
        leaders,
        players,
        cities,
    })
}

pub(super) fn restore(sim: &mut Sim, mut state: LeaderMatchState) -> Result<(), SaveError> {
    if state.game.frame != sim.world.frame || state.game.tick != sim.world.seconds {
        return Err(SaveError::Invalid("victory match/world clock mismatch"));
    }
    let setup_owner = std::mem::take(&mut sim.vic_leaders.setup_owner);
    state.leaders.setup_owner = setup_owner;
    let configured = state.leaders.setup_owner.configured_mask();
    for (who, leader) in state.leaders.slots.iter().enumerate() {
        let configured_here = configured & (1u8 << who) != 0;
        if (leader.leader_flags & super::super::victory_score::leader_flag::VALID != 0)
            != configured_here
        {
            return Err(SaveError::Invalid(
                "victory leader/setup projection mismatch",
            ));
        }
    }
    sim.vic_match = state.game;
    sim.vic_leaders = state.leaders;
    sim.players = state.players;
    sim.cities = state.cities;
    Ok(())
}
