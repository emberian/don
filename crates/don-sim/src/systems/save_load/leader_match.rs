//! DoNSave owner for the authoritative victory `Leaders`/`Match` pair.
//!
//! The older `PLAYER_SETUP` section is a reconstructive recipe: it can recreate the
//! frame-zero transaction, but not runtime diplomacy, victory/defeat flags, timers, score
//! inputs, Game semaphores, or lifecycle `Player` rows. This section stores those owners
//! directly. Setup is still reconstructed first so its immutable receipt remains the one
//! authority; this snapshot then restores the mutable mid-match rows over that owner.

use super::{Reader, SaveError, Writer};
use crate::systems::leader_init_diplomacy_loop::LeaderInitDiplomacyRow;
use crate::systems::victory_score::{
    DefeatType, EncryptedEconomy, LeaderState, Leaders, Match, MatchEvent, ScoreConstants,
    TypeKind, TypeRow, TypeTable, VictoryOptions, VictoryType, NUM_BUILD_SLOTS, NUM_LEADERS,
    NUM_RESOURCES, NUM_TYPES, NUM_UNIT_SLOTS,
};
use crate::tick::lifecycle_host::PlayerTable;
use crate::tick::Sim;

const MAX_CATEGORY_ROWS: usize = 4096;
const MAX_EVENTS: usize = 256;

pub(super) struct LeaderMatchState {
    game: Match,
    leaders: Leaders,
    players: Option<PlayerTable>,
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

fn write_match(w: &mut Writer, game: &Match) -> Result<(), SaveError> {
    super::write_match_options(w, game.options);
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

fn read_match(r: &mut Reader<'_>) -> Result<Match, SaveError> {
    Ok(Match {
        options: super::read_match_options(r)?,
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

fn write_leader(w: &mut Writer, row: &LeaderState) -> Result<(), SaveError> {
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
    Ok(())
}

fn read_leader(r: &mut Reader<'_>) -> Result<LeaderState, SaveError> {
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
        victory_type,
        defeat_type,
        population_cap,
        misery,
        give_attrition_disabled,
        take_attrition_disabled,
        neutral_attrition,
        building_attrition_disabled,
        num_buildings,
        num_units,
        num_queued,
        tech_at_start,
        rare,
        territory,
        economy,
        has_tech,
        researching,
        resource_avail,
        economic: r.i32()?,
        has_preq_2b0: r.bool()?,
        has_preq_2b9: r.bool()?,
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
    write_match(&mut w, &sim.vic_match)?;
    write_type_table(&mut w, &sim.vic_leaders.types)?;
    for (slot, leader) in sim.vic_leaders.slots.iter().enumerate() {
        if leader.who != slot as i32 {
            return Err(SaveError::Invalid("victory leader identity"));
        }
        write_leader(&mut w, leader)?;
    }
    w.len(sim.vic_leaders.events.len(), "match events")?;
    if sim.vic_leaders.events.len() > MAX_EVENTS {
        return Err(SaveError::Limit("match events"));
    }
    for event in &sim.vic_leaders.events {
        write_event(&mut w, event);
    }
    write_players(&mut w, sim.players.as_ref());
    Ok(w.0)
}

pub(super) fn read(data: &[u8]) -> Result<LeaderMatchState, SaveError> {
    let mut r = Reader::new(data);
    let game = read_match(&mut r)?;
    let types = read_type_table(&mut r)?;
    let mut leaders = Leaders::new(types);
    for leader in &mut leaders.slots {
        *leader = read_leader(&mut r)?;
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
    r.finish()?;
    Ok(LeaderMatchState {
        game,
        leaders,
        players,
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
    Ok(())
}
