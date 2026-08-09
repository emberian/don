//! Transcription of `ron-data/ai-scripts/economic.bhs`
//! (Mark Sobota / Mike Engle, "boom script, good for Aztec, Inca, Mongols,
//! Romans, Russians, Spanish").
//!
//! This is a faithful transcription in fidelity mode. Order of calls, dead stores,
//! unused locals, redundant re-queries and shipped bugs are preserved there. Improved
//! mode changes only the two registered, evidence-backed script fixes, through
//! `don_sim::deviations`; every other `SHIPPED BUG` or `QUIRK` remains untouched.
//!
//! Entry point signature in the script:
//! ```text
//! int ai economic (int who, ref int step, int boom_vs_rush, int num_loops)
//! ```
//! and the engine calls it (`FUN_006C1960` case 1) with
//! `who = player_index + 1`, `step = &player[0x790]`,
//! `boom_vs_rush = player[0x6DD4] + 2`, `num_loops = 5`.

use don_sim::deviations::behaviour as deviation_behaviour;

use crate::abi::ScriptResult;
use crate::api::{bhs_true, ScriptWorld};
use crate::library::{
    city_placement, place_dock, place_farm, place_mine, place_woodcutter, train_unit_with_need,
    LibraryStatics,
};

const BLOCK_ON_THIS: i32 = ScriptResult::BlockOnThis as i32;
#[allow(dead_code)]
const DONT_BLOCK_ON_THIS: i32 = ScriptResult::DontBlockOnThis as i32;
const SCRIPT_DONE: i32 = ScriptResult::ScriptDone as i32;

/// The script's `static` storage.
///
/// `economic.bhs` declares eight copies of each per-player value
/// (`prev_step0..7`, `needed_citizens0..7`, `timer_started0..7`,
/// `fishermen_total0..7`) and switches on `who-1` to load and store them —
/// the authors' comment calls it the "ghetto array". BHS `static` is
/// per-script-function, not per-player, so this hand-rolled indexing is
/// necessary and is reproduced as real arrays.
///
/// `needed_techs` is a genuine *shared* static (`static int needed_techs =
/// get_techs_per_age(who);`): under C initialise-once semantics it takes the
/// value from whichever player ran the script first and is then used by all
/// eight. That is preserved, not fixed.
#[derive(Debug, Clone, Default)]
pub struct EconomicStatics {
    pub prev_step: [i32; 8],
    pub needed_citizens: [i32; 8],
    pub timer_started: [i32; 8],
    pub fishermen_total: [i32; 8],
    /// `static int needed_techs = get_techs_per_age(who);` — initialised once.
    pub needed_techs: Option<i32>,
    /// Declared `static` in the script and never read. Kept so the storage
    /// layout matches the source.
    pub wood_camp: i32,
    pub wood_camp_1: i32,
    pub max_woodcutters: i32,
    pub build_merchant: i32,
    /// Statics owned by `aibestbuildlibrary.bhs`.
    pub lib: LibraryStatics,
}

impl EconomicStatics {
    pub fn new() -> Self {
        EconomicStatics {
            lib: LibraryStatics::default(),
            ..Default::default()
        }
    }
}

/// One invocation of the `economic` AI script.
///
/// `step` is the engine's `ref int` (`player+0x790`); mutations are written
/// back by the engine whether or not the script returns early.
pub fn economic<W: ScriptWorld>(
    w: &mut W,
    who: i32,
    step: &mut i32,
    _boom_vs_rush: i32,
    num_loops: i32,
    st: &mut EconomicStatics,
) -> i32 {
    let mode = w.mode_config();
    // if ((num_cities(who) < 1) && (num_type_with_queued(who, "Citizen") < 1))
    //   return SCRIPT_DONE;
    if w.num_cities(who) < 1 && w.num_type_with_queued(who, "Citizen") < 1 {
        return SCRIPT_DONE;
    }

    let my_capital = w.find_city_with_num(who, 1);

    // Emergency barracks when the capital has ever been hit. Passing "" for
    // the city name and -1 for the window makes was_city_attacked/raided a
    // "has any city ever been attacked" query (see api.rs).
    if bhs_true(w.was_city_attacked(who, "", -1)) || bhs_true(w.was_city_raided(who, "", -1)) {
        if w.num_type_with_queued(who, "Barracks") < 1
            && bhs_true(w.have_tech(who, "The Art of War"))
        {
            if w.place_building_with_cost(who, "Barracks", &my_capital) > 0 {
                return SCRIPT_DONE;
            } else {
                return BLOCK_ON_THIS;
            }
        } else {
            return SCRIPT_DONE;
        }
    }

    let mut return_value = BLOCK_ON_THIS;

    let who_nation = w.find_nation(who);
    // static int needed_techs = get_techs_per_age(who);   (initialise once)
    let needed_techs = *st
        .needed_techs
        .get_or_insert_with(|| w.get_techs_per_age(who));
    // int player_age = age(who);  -- computed and then never used; the script
    // re-queries age(who) everywhere. Preserved as a call for call-order
    // fidelity.
    let _player_age = w.age(who);

    let mut new_city;
    let caravan_lim = w.num_cities(who) * (w.num_cities(who) - 1) / 2;
    let fishermen_cap = 4;
    let mut large_conquest_start = 0;

    // ---- ghetto array: load ------------------------------------------------
    let slot = who - 1;
    let idx = if (0..8).contains(&slot) {
        Some(slot as usize)
    } else {
        None
    };
    let (mut old_step, mut needed_citizens, mut timer_started, mut fishermen_total) = match idx {
        Some(i) => (
            st.prev_step[i],
            st.needed_citizens[i],
            st.timer_started[i],
            st.fishermen_total[i],
        ),
        // default: old_step = 0; needed_citizens = 0;  (timer_started and
        // fishermen_total keep their initialiser value of 0)
        None => (0, 0, 0, 0),
    };

    // ---- map classification ------------------------------------------------
    let mapstyle = w.get_mapstyle();
    let sea_map = if matches!(
        mapstyle.as_str(),
        "Atlantic Sea Power"
            | "Nile Delta"
            | "British Isles"
            | "Warring States"
            | "New World"
            | "East Indies"
    ) {
        1
    } else if matches!(mapstyle.as_str(), "Colonial Powers" | "Mediterranean") {
        1
    } else {
        0
    };

    let my_second_city = w.find_city_with_num(who, 2);
    let my_third_city = w.find_city_with_num(who, 3);

    // ---- Conquer-the-World entry selection ---------------------------------
    if bhs_true(w.is_conquest_scenario()) {
        let size = w.get_starting_town_size(who);
        if *step == 1 {
            if size == 0 {
                *step = 1; // nomad
            } else if size == 1 {
                *step = 2;
            } else if size == 2 {
                if sea_map > 0 {
                    *step = 6; // sea boom
                } else if who_nation == "Greeks" && w.get_is_no_nation_powers() < 1 && sea_map < 1 {
                    *step = 7;
                } else if who_nation == "Bantu" && w.get_is_no_nation_powers() < 1 && sea_map < 1 {
                    *step = 7;
                } else {
                    *step = 6; // standard
                }
            } else {
                large_conquest_start = 1;
            }
        } else if size > 2 {
            large_conquest_start = 1;
        }
    }

    if large_conquest_start > 0 {
        if let Some(r) = large_conquest(w, who, step, &my_capital, &my_second_city, &my_third_city)
        {
            return r;
        }
    }

    // ---- first-call entry selection for skirmish ---------------------------
    if *step == 1 {
        if w.get_starting_resources(who) > 4 || w.num_cities(who) > 1 {
            return SCRIPT_DONE;
        }
        let size = w.get_starting_town_size(who);
        if size == 0 {
            *step = 1; // nomad
        } else if size == 1 {
            *step = 2;
        } else if size >= 2 {
            if sea_map > 0 {
                *step = 6;
            } else if who_nation == "Greeks" && w.get_is_no_nation_powers() < 1 && sea_map < 1 {
                *step = 7;
            } else if who_nation == "Bantu" && w.get_is_no_nation_powers() < 1 && sea_map < 1 {
                *step = 7;
            } else {
                *step = 6;
            }
        } else {
            return SCRIPT_DONE;
        }
    }

    train_unit_with_need(w, who, needed_citizens, "Citizen");

    // ---- per-call maintenance ---------------------------------------------
    if w.num_type(who, "Market") >= 1 {
        if w.num_type_with_queued(who, "Caravan") < caravan_lim {
            w.train_unit_with_cost(who, 1, "Caravan");
        }
        if bhs_true(w.at_least_type(who, 100, "Wealth")) {
            let mut j = w.num_type_with_queued(who, "Merchant");
            while j < 3 && j < w.num_rare_resources_seen(who) {
                if w.train_unit_with_cost(who, 1, "Merchant") < 1 {
                    break;
                }
                j += 1;
            }
        }
    }

    if w.population(who) >= 23 && !bhs_true(w.have_tech(who, "The Art of War")) {
        w.research_tech_with_cost(who, "The Art of War");
    }

    if w.num_type(who, "University") > 0
        && (w.num_type(who, "Market") > 0 || w.num_type(who, "Dock") > 0)
    {
        if bhs_true(w.at_least_type(who, 130, "Wealth")) {
            w.train_unit_with_cost(who, 1, "Scholar");
        }
    }

    if fishermen_total < fishermen_cap
        && w.num_type(who, "Dock") > 0
        && bhs_true(w.at_least_type(who, 100, "Timber"))
    {
        if w.train_unit_with_cost(who, 1, "Fishermen") > 0 {
            fishermen_total += 1;
        }
    }

    if bhs_true(w.have_tech(who, "The Art of War")) && w.num_type_with_queued(who, "Tower") < 1 {
        if w.num_cities(who) > 1 {
            w.place_building_with_cost(who, "Tower", &my_second_city);
        } else {
            w.place_building_with_cost(who, "Tower", &my_capital);
        }
    }

    if w.num_type(who, "Tower") > 0
        && !bhs_true(w.have_tech(who, "Allegiance"))
        && w.age(who) > 0
        && bhs_true(w.have_tech(who, "Mathematics"))
    {
        w.research_tech_with_cost(who, "Allegiance");
    }

    // Lakota step
    if w.find_nation(who) == "Lakota" && w.age(who) > 0 {
        if !bhs_true(w.have_tech(who, "Mercenaries")) {
            w.research_tech_with_cost(who, "Mercenaries");
        } else if w.num_cities(who) > 1 {
            if w.num_type_with_queued(who, "Stable") < 1 {
                w.place_building_with_cost(who, "Stable", &my_second_city);
            }
        }
    }

    // ---- hang watchdog -----------------------------------------------------
    // QUIRK: set_timer/stop_timer/timer_expired take a String timer id in the
    // binary; the script passes `who`, an int. See api.rs.
    let timer_id = who.to_string();
    if *step == old_step && timer_started < 1 {
        w.set_timer(&timer_id, 300);
        timer_started = 1;
    }
    if *step != old_step {
        w.stop_timer(&timer_id);
        timer_started = 0;
    }
    if bhs_true(w.timer_expired(&timer_id)) {
        return SCRIPT_DONE;
    }

    if sea_map > 0 && w.age(who) > 0 && *step == 23 {
        if w.num_type_with_queued(who, "University") < 2 {
            w.place_building_with_cost(who, "University", &my_capital);
            w.place_building_with_cost(who, "University", &my_second_city);
        }
    }

    // ---- the build-order state machine -------------------------------------
    let greek = who_nation == "Greeks";
    let bantu = who_nation == "Bantu";
    let british = who_nation == "British";
    let korean = who_nation == "Koreans";
    let egyptian = who_nation == "Egyptians";
    let inca = who_nation == "Inca";
    let mongol = who_nation == "Mongols";

    for _i in 0..num_loops {
        if old_step != *step {
            old_step = *step;
        }
        let npow = w.get_is_no_nation_powers() < 1;

        match *step {
            // ---- nomad setup ----
            1 => {
                // place City
                old_step = 0;
                if w.num_type_with_queued(who, "Small City") < 1 {
                    w.place_city_with_cost(who);
                }
                if bhs_true(w.find_inactive_build(who, "Small City")) {
                    new_city = w.find_inactive_build(who, "Small City");
                    if bhs_true(w.building_started(who, new_city)) {
                        *step += 1;
                    }
                }
                return_value = BLOCK_ON_THIS;
            }
            2 => {
                // Woodcutter's Camp at the capital, all citizens onto the city
                old_step = 0;
                new_city = w.find_inactive_build(who, "Small City");
                if new_city > 0 {
                    let n = w.num_type(who, "Citizen");
                    for _j in 0..n {
                        let citizen_id = w.find_unit(who, "Citizen");
                        w.citizen_repair_order(who, citizen_id, new_city);
                    }
                }
                if bhs_true(w.num_type_with_queued(who, "Woodcutter's Camp")) {
                    if w.find_nation(who) != "Lakota" {
                        *step += 1;
                    } else {
                        *step += 2;
                    }
                } else if w.place_building_with_cost(who, "Woodcutter's Camp", &my_capital) > 0 {
                    if w.find_nation(who) != "Lakota" {
                        *step += 1;
                    } else {
                        *step += 2;
                    }
                } else if !my_capital.is_empty() && w.can_pay_cost(who, "Woodcutter's Camp") > 0 {
                    // Script: `(my_capital > -1)`. my_capital is a *String*;
                    // comparing it with -1 is nonsense in C terms and the
                    // interpreter's rule for it is UNVERIFIED. Modelled as
                    // "the capital name is non-empty", which is what
                    // find_city_with_num returns on failure.
                    return SCRIPT_DONE;
                }
                return_value = BLOCK_ON_THIS;
            }
            3 => {
                // three farms at the capital
                old_step = 0;
                let mut j = w.num_type_with_queued(who, "Farm");
                while j < 3 {
                    if w.place_building_with_cost(who, "Farm", &my_capital) < 1 {
                        break;
                    }
                    j += 1;
                }
                if w.num_type_with_queued(who, "Farm") >= 3 {
                    *step += 1;
                }
                return_value = BLOCK_ON_THIS;
            }
            4 => {
                // five citizens
                needed_citizens = 5;
                train_unit_with_need(w, who, needed_citizens, "Citizen");
                if greek && npow {
                    if w.num_type_with_queued(who, "University") > 0 {
                        *step += 1;
                    } else if w.place_building_with_cost(who, "University", &my_capital) > 0 {
                        *step += 1;
                    }
                } else {
                    *step += 1;
                }
                return_value = BLOCK_ON_THIS;
            }
            5 => {
                // library
                old_step = 0;
                if bhs_true(w.num_type_with_queued(who, "Library")) {
                    *step += 1;
                } else if w.place_building_with_cost(who, "Library", &my_capital) > 0 {
                    if greek && npow {
                        *step = 7;
                    } else if bantu && npow && sea_map < 1 {
                        *step = 7;
                    } else if w.at_least_type(who, 75, "Wealth") < 1 {
                        *step = 7;
                    } else {
                        *step += 1;
                    }
                }
                return_value = BLOCK_ON_THIS;
            }
            // ---- main boom ----
            6 => {
                // Science I
                old_step = 0;
                if !bhs_true(w.have_tech(who, "Written Word")) {
                    if w.research_tech_with_cost(who, "Written Word") > 0 {
                        if greek && npow && sea_map < 1 {
                            *step = 18;
                        } else {
                            *step += 1;
                        }
                    } else if w.at_least_type(who, 75, "Wealth") < 1 {
                        *step += 1;
                    }
                } else if greek && npow && sea_map < 1 {
                    *step = 18;
                } else {
                    *step += 1;
                }
                return_value = BLOCK_ON_THIS;
            }
            7 => {
                // Civic I
                if w.num_cities(who) > 1 {
                    return SCRIPT_DONE;
                } else if bhs_true(w.have_tech(who, "City State")) {
                    *step += 1;
                } else if bhs_true(w.research_tech_with_cost(who, "City State")) {
                    *step += 1;
                }
                return_value = BLOCK_ON_THIS;
            }
            8 => {
                // SHIPPED BUG: "Citizens" (plural) is not a type name in
                // typenames.xml / unitrules.xml — only "Citizen" is. The
                // engine's name lookup returns -1 for an unresolved name, so
                // this call and the ones in cases 11/16/22/27 cannot train
                // anything. Fidelity reproduces it; improved mode resolves the registered
                // type-name correction at the call boundary.
                needed_citizens = 9;
                train_unit_with_need(
                    w,
                    who,
                    needed_citizens,
                    deviation_behaviour::bhs_unit_type_name(&mode, "Citizens"),
                );
                if w.find_nation(who) != "Lakota" {
                    *step += 1;
                } else {
                    *step += 2;
                }
                return_value = BLOCK_ON_THIS;
            }
            9 => {
                if place_farm(w, who) >= 0 {
                    *step += 1;
                }
                return_value = BLOCK_ON_THIS;
            }
            10 => {
                // city #2
                if bhs_true(w.have_tech(who, "City State")) {
                    if w.num_cities(who) < 2 {
                        if city_placement(w, who, &mut st.lib) > 0 {
                            if bantu && npow && sea_map < 1 {
                                *step = 10;
                            } else {
                                *step += 1;
                            }
                        }
                    } else if bantu && npow && sea_map < 1 {
                        if w.num_cities(who) < 3 {
                            if city_placement(w, who, &mut st.lib) > 0 {
                                *step += 1;
                            }
                        } else {
                            *step += 1;
                        }
                    } else {
                        *step += 1;
                    }
                }
                return_value = BLOCK_ON_THIS;
            }
            11 => {
                needed_citizens = 11;
                train_unit_with_need(
                    w,
                    who,
                    needed_citizens,
                    deviation_behaviour::bhs_unit_type_name(&mode, "Citizens"),
                ); // SHIPPED BUG in fidelity mode
                if w.find_nation(who) != "Lakota" {
                    *step += 1;
                } else {
                    *step += 2;
                }
                return_value = BLOCK_ON_THIS;
            }
            12 => {
                if place_farm(w, who) >= 0 {
                    if bantu && npow && sea_map < 1 {
                        *step = 14;
                    } else {
                        *step += 1;
                    }
                }
                return_value = BLOCK_ON_THIS;
            }
            13 => {
                if place_woodcutter(w, who, &mut st.lib) >= 0 {
                    if bantu && npow && sea_map < 1 {
                        *step = 17;
                    } else {
                        *step += 1;
                    }
                }
                return_value = BLOCK_ON_THIS;
            }
            14 => {
                // Commerce I
                if w.research_tech_with_cost(who, "Barter") > 0 {
                    if sea_map > 0 {
                        *step = 35; // dock
                    } else if greek && npow && sea_map < 1 {
                        *step = 17;
                    } else if korean && npow && sea_map < 1 {
                        *step += 2;
                    } else if bantu && npow && sea_map < 1 {
                        *step = 13;
                    } else {
                        *step += 1;
                    }
                }
                return_value = BLOCK_ON_THIS;
            }
            15 => {
                // Market #1
                let advance = |step: &mut i32| {
                    if sea_map > 0 {
                        *step = 18;
                    } else if greek && npow && sea_map < 1 {
                        *step = 23;
                    } else if korean && npow && sea_map < 1 {
                        *step += 2;
                    } else if bantu && npow && sea_map < 1 {
                        *step = 18;
                    } else {
                        *step += 1;
                    }
                };
                if w.num_city_buildings(who, &my_capital, "Market", 1) < 1 {
                    if bhs_true(w.have_tech(who, "Barter"))
                        && w.place_building_with_cost(who, "Market", &my_capital) > 0
                    {
                        advance(step);
                    }
                } else {
                    advance(step);
                }
                return_value = BLOCK_ON_THIS;
            }
            16 => {
                needed_citizens = 14;
                train_unit_with_need(
                    w,
                    who,
                    needed_citizens,
                    deviation_behaviour::bhs_unit_type_name(&mode, "Citizens"),
                ); // SHIPPED BUG in fidelity mode
                if bantu && npow && sea_map < 1 {
                    *step = 15;
                } else if w.find_nation(who) != "Lakota" {
                    *step += 1;
                } else {
                    *step += 2;
                }
                return_value = BLOCK_ON_THIS;
            }
            17 => {
                // farms
                old_step = 0;
                if british && npow && sea_map < 1 {
                    needed_citizens = 23;
                    if w.num_type_with_queued(who, "Farm") < 10 {
                        if place_farm(w, who) > 0 {
                            old_step = 0;
                        }
                    } else {
                        *step = 28;
                    }
                } else if w.num_type_with_queued(who, "Farm") < 7 {
                    place_farm(w, who);
                } else if sea_map > 0 {
                    *step = 23;
                } else if greek && npow && sea_map < 1 {
                    *step = 20;
                } else if bantu && npow && sea_map < 1 {
                    *step = 16;
                } else {
                    *step += 1;
                }
                return_value = BLOCK_ON_THIS;
            }
            18 => {
                // Classical Age
                let advance = |step: &mut i32| {
                    if sea_map > 0 {
                        *step = 24;
                    } else if greek && npow && sea_map < 1 {
                        *step = 29;
                    } else if egyptian && npow && sea_map < 1 {
                        *step += 2;
                    } else if inca && npow && sea_map < 1 {
                        *step += 2;
                    } else if bantu && npow && sea_map < 1 {
                        *step = 20;
                    } else {
                        *step += 1;
                    }
                };
                if bhs_true(w.is_conquest_scenario()) && w.age(who) > 0 {
                    advance(step);
                } else if needed_techs <= 3 {
                    if w.age(who) > 0 {
                        return SCRIPT_DONE;
                    } else if w.research_tech_with_cost(who, "Classical Age") > 0 {
                        advance(step);
                    }
                } else {
                    return SCRIPT_DONE;
                }
                return_value = BLOCK_ON_THIS;
            }
            19 => {
                // Market #2
                if w.num_city_buildings(who, &my_second_city, "Market", 1) < 1 {
                    if bhs_true(w.have_tech(who, "Barter"))
                        && w.place_building_with_cost(who, "Market", &my_second_city) > 0
                    {
                        if bantu && npow && sea_map < 1 {
                            *step = 21;
                        } else {
                            *step += 1;
                        }
                    }
                } else if bantu && npow && sea_map < 1 {
                    *step = 21;
                } else {
                    *step += 1;
                }
                return_value = BLOCK_ON_THIS;
            }
            20 => {
                // universities
                if bhs_true(w.have_tech(who, "Classical Age")) || (greek && npow) {
                    if w.num_city_buildings(who, &my_second_city, "University", 1) < 1 {
                        let result = w.place_building_with_cost(who, "University", &my_second_city);
                        if deviation_behaviour::bhs_order_failed(&mode, result) {
                            return SCRIPT_DONE;
                        }
                        old_step = 0;
                    } else if w.num_city_buildings(who, &my_capital, "University", 1) < 1 {
                        let result = w.place_building_with_cost(who, "University", &my_capital);
                        if deviation_behaviour::bhs_order_failed(&mode, result) {
                            return SCRIPT_DONE;
                        }
                        old_step = 0;
                    } else if sea_map > 0 {
                        *step = 30;
                    } else if greek && npow && sea_map < 1 {
                        *step = 15;
                    } else if bantu && npow && sea_map < 1 {
                        *step = 32;
                    } else {
                        *step += 1;
                    }
                } else if w.researching_tech(who, "Classical Age") > 0 {
                    old_step = 0;
                }
                return_value = BLOCK_ON_THIS;
            }
            21 => {
                // Mine #1
                if place_mine(w, who) >= 0 {
                    if british && npow && sea_map < 1 {
                        *step = 29;
                    } else if greek && npow && sea_map < 1 {
                        *step = 32;
                    } else if inca && npow && sea_map < 1 {
                        old_step = 0;
                        if w.num_type_with_queued(who, "Mine") < 3 {
                            place_mine(w, who);
                        } else {
                            *step += 1;
                        }
                        if w.can_pay_cost(who, "Mine") > 0 {
                            *step += 1;
                        }
                    } else {
                        *step += 1;
                    }
                }
                return_value = BLOCK_ON_THIS;
            }
            22 => {
                old_step = 0;
                needed_citizens = if inca && npow && sea_map < 1 { 27 } else { 23 };
                train_unit_with_need(
                    w,
                    who,
                    needed_citizens,
                    deviation_behaviour::bhs_unit_type_name(&mode, "Citizens"),
                ); // SHIPPED BUG in fidelity mode
                if sea_map > 0 {
                    *step = 32;
                } else if bantu && npow && sea_map < 1 {
                    *step = 33;
                } else {
                    *step += 1;
                }
                return_value = BLOCK_ON_THIS;
            }
            23 => {
                // Commerce II
                old_step = 0;
                if bantu && npow && sea_map < 1 {
                    needed_citizens = 18;
                }
                let advance = |step: &mut i32| {
                    if sea_map > 0 {
                        *step = 15;
                    } else if bantu && npow && sea_map < 1 {
                        *step = 25;
                    } else {
                        *step += 1;
                    }
                };
                if !bhs_true(w.have_tech(who, "Coinage")) {
                    if w.research_tech_with_cost(who, "Coinage") > 0 {
                        advance(step);
                    }
                } else {
                    advance(step);
                }
                return_value = BLOCK_ON_THIS;
            }
            24 => {
                // Military I
                if !bhs_true(w.have_tech(who, "The Art of War")) {
                    if bhs_true(w.research_tech_with_cost(who, "The Art of War")) {
                        if sea_map > 0 {
                            *step = 29;
                        } else if british && npow && sea_map < 1 {
                            *step = 31;
                        } else if greek && npow && sea_map < 1 {
                            needed_citizens = 20;
                            *step += 1;
                        } else if bantu && npow && sea_map < 1 {
                            *step = 34;
                        } else {
                            *step += 1;
                        }
                    }
                } else if sea_map > 0 {
                    *step = 29;
                } else if british && npow && sea_map < 1 {
                    *step = 31;
                } else if greek && npow && sea_map < 1 {
                    needed_citizens = 20;
                    *step += 1;
                } else if bantu && npow && sea_map < 1 {
                    *step = 34;
                } else if w.find_nation(who) != "Lakota" {
                    *step += 1;
                } else {
                    *step += 3;
                }
                return_value = BLOCK_ON_THIS;
            }
            25 => {
                // more farms
                if egyptian && npow && sea_map < 1 {
                    if w.num_type_with_queued(who, "Farm") < 11 {
                        if place_farm(w, who) > 0 {
                            old_step = 0;
                        }
                    } else {
                        *step += 1;
                    }
                } else if w.num_type_with_queued(who, "Farm") < 9 {
                    if place_farm(w, who) > 0 {
                        old_step = 0;
                    }
                } else if greek && npow && sea_map < 1 {
                    *step = 6;
                } else if bantu && npow && sea_map < 1 {
                    *step = 24;
                } else {
                    *step += 1;
                }
                return_value = BLOCK_ON_THIS;
            }
            26 => {
                // Granary at city #1
                if w.num_city_buildings(who, &my_capital, "Granary", 1) < 1 {
                    if w.place_building_with_cost(who, "Granary", &my_capital) > 0 {
                        if british && npow && sea_map < 1 {
                            return SCRIPT_DONE;
                        }
                        *step += 1;
                    }
                } else if british && npow && sea_map < 1 {
                    return SCRIPT_DONE;
                } else {
                    *step += 1;
                }
                return_value = BLOCK_ON_THIS;
            }
            27 => {
                needed_citizens = 28;
                train_unit_with_need(
                    w,
                    who,
                    needed_citizens,
                    deviation_behaviour::bhs_unit_type_name(&mode, "Citizens"),
                ); // SHIPPED BUG in fidelity mode
                *step += 1;
                return_value = BLOCK_ON_THIS;
            }
            28 => {
                // Science II
                if !bhs_true(w.have_tech(who, "Mathematics")) {
                    if w.research_tech_with_cost(who, "Mathematics") > 0 {
                        if british && npow && sea_map < 1 {
                            *step = 24;
                        } else {
                            *step += 1;
                        }
                    }
                }
                return_value = BLOCK_ON_THIS;
            }
            29 => {
                // Civic II
                if !bhs_true(w.have_tech(who, "Empire")) {
                    if w.research_tech_with_cost(who, "Empire") > 0 {
                        if sea_map > 0 {
                            *step = 20;
                        } else {
                            *step += 1;
                        }
                    }
                }
                return_value = BLOCK_ON_THIS;
            }
            30 => {
                // city #3
                if bhs_true(w.have_tech(who, "Empire")) {
                    let want_more = w.num_cities(who) < 3
                        || (bantu && npow && w.num_cities(who) < 4 && sea_map > 0);
                    if want_more {
                        if city_placement(w, who, &mut st.lib) > 0 {
                            if sea_map > 0 {
                                if bantu && npow && w.num_type_with_queued(who, "Small City") < 4 {
                                    *step = 30;
                                } else {
                                    *step = 21;
                                }
                            } else if british && npow && sea_map < 1 {
                                *step = 26;
                            } else if greek && npow && sea_map < 1 {
                                *step = 21;
                            } else {
                                *step += 1;
                            }
                        }
                    } else if sea_map > 0 {
                        *step = 21;
                    } else if british && npow && sea_map < 1 {
                        *step = 26;
                    } else if greek && npow && sea_map < 1 {
                        *step = 21;
                    } else {
                        *step += 1;
                    }
                }
                return_value = BLOCK_ON_THIS;
            }
            31 => {
                // barracks (stable for Lakota)
                if w.find_nation(who) != "Lakota" {
                    if w.place_building_with_cost(who, "Barracks", &my_second_city) > 0 {
                        if british && npow && sea_map < 1 {
                            *step = 18;
                        } else if greek && npow && sea_map < 1 {
                            return SCRIPT_DONE;
                        } else if bantu && npow && sea_map < 1 {
                            return SCRIPT_DONE;
                        } else {
                            *step += 1;
                        }
                    }
                } else if w.place_building_with_cost(who, "Stable", &my_second_city) > 0
                    || w.num_type_with_queued(who, "Stable") > 0
                {
                    *step += 1;
                }
                return_value = BLOCK_ON_THIS;
            }
            32 => {
                // University #3
                if w.num_cities(who) > 2
                    && w.num_city_buildings(who, &my_third_city, "University", 1) < 1
                    && w.place_building_with_cost(who, "University", &my_third_city) > 0
                {
                    if sea_map > 0 {
                        return SCRIPT_DONE;
                    } else if greek && npow && sea_map < 1 {
                        *step = 31;
                    } else if bantu && npow && sea_map < 1 {
                        *step = 23;
                    } else {
                        *step += 1;
                    }
                }
                return_value = BLOCK_ON_THIS;
            }
            33 => {
                // Market #3
                if w.num_city_buildings(who, &my_third_city, "Market", 1) < 1 {
                    if bhs_true(w.have_tech(who, "Barter"))
                        && w.place_building_with_cost(who, "Market", &my_third_city) > 0
                    {
                        if bantu && npow && sea_map < 1 {
                            *step = 31;
                        } else {
                            *step += 1;
                        }
                    }
                } else if bantu && npow && sea_map < 1 {
                    *step = 31;
                } else {
                    *step += 1;
                }
                return_value = BLOCK_ON_THIS;
            }
            34 => {
                // Woodcutter's Camp #3
                if place_woodcutter(w, who, &mut st.lib) >= 0 {
                    if mongol && npow && sea_map < 1 {
                        *step = 36;
                    } else if bantu && npow && sea_map < 1 {
                        *step = 19;
                    } else {
                        return SCRIPT_DONE;
                    }
                }
                return_value = BLOCK_ON_THIS;
            }
            35 => {
                // dock
                if place_dock(w, who) > 0 {
                    *step = 16;
                } else if w.can_pay_cost(who, "Dock") > 0 {
                    return SCRIPT_DONE;
                }
                return_value = BLOCK_ON_THIS;
            }
            36 => {
                // Mongol stables
                if w.num_type_with_queued(who, "Stable") < 2 {
                    match w.num_cities(who) {
                        1 => {
                            w.place_building_with_cost(who, "Stable", &my_capital);
                        }
                        2 => {
                            w.place_building_with_cost(who, "Stable", &my_second_city);
                        }
                        3 => {
                            w.place_building_with_cost(who, "Stable", &my_third_city);
                        }
                        _ => return SCRIPT_DONE,
                    }
                } else {
                    return SCRIPT_DONE;
                }
                return_value = BLOCK_ON_THIS;
            }
            _ => {
                return_value = SCRIPT_DONE;
            }
        }
    }

    // ---- ghetto array: store ----------------------------------------------
    // Reached only on the fall-through path: every `return SCRIPT_DONE` above
    // skips this write-back, so `prev_step`, `needed_citizens`, `timer_started`
    // and `fishermen_total` keep their previous values on those paths. This is
    // load-bearing behaviour, not an oversight in the transcription.
    if let Some(i) = idx {
        st.prev_step[i] = old_step;
        st.needed_citizens[i] = needed_citizens;
        st.timer_started[i] = timer_started;
        st.fishermen_total[i] = fishermen_total;
    }

    return_value
}

/// The Conquer-the-World "large start" branch. Returns `Some(result)` when the
/// script returns from inside it, `None` when it falls through (which happens
/// only when `age(who) == 0`).
fn large_conquest<W: ScriptWorld>(
    w: &mut W,
    who: i32,
    step: &mut i32,
    my_capital: &str,
    my_second_city: &str,
    my_third_city: &str,
) -> Option<i32> {
    let age = w.age(who);
    // The two arms differ only in the second military building
    // ("Stable" before the Enlightenment, "Auto Plant" after) and in case 4's
    // guard type ("Tower" vs "Stockade").
    let (second_building, tower_guard) = if age > 0 && age < 5 {
        ("Stable", "Tower")
    } else if age > 4 {
        ("Auto Plant", "Stockade")
    } else {
        return None;
    };

    let city_for = |n: i32| -> &str {
        match n {
            1 => my_capital,
            2 => my_second_city,
            _ => my_third_city,
        }
    };

    match *step {
        1 => {
            if !bhs_true(w.have_tech(who, "The Art of War")) {
                if w.research_tech_with_cost(who, "The Art of War") > 0 {
                    *step += 1;
                }
            } else {
                *step += 1;
            }
            Some(BLOCK_ON_THIS)
        }
        2 => {
            w.train_unit_with_cost(who, 5, "Citizen");
            if bhs_true(w.have_tech(who, "The Art of War")) {
                *step += 2;
            } else {
                *step += 1;
            }
            Some(BLOCK_ON_THIS)
        }
        3 => {
            let n = w.num_cities(who);
            if w.num_type_with_queued(who, "Barracks") < 1 && (1..=3).contains(&n) {
                w.place_building_upgrade_with_cost(who, "Barracks", city_for(n));
            }
            if w.num_type_with_queued(who, second_building) < 1 && (1..=3).contains(&n) {
                w.place_building_upgrade_with_cost(who, second_building, city_for(n));
            }
            if w.num_type_with_queued(who, "Barracks") > 0
                && w.num_type_with_queued(who, second_building) > 0
            {
                *step += 1;
            }
            Some(BLOCK_ON_THIS)
        }
        4 => {
            let cities = w.num_cities(who);
            if w.num_type_with_queued(who, tower_guard) < cities {
                // Towers are placed from the newest city backwards.
                if (1..=3).contains(&cities) {
                    for n in (1..=cities).rev() {
                        w.place_building_upgrade_with_cost(who, "Tower", city_for(n));
                    }
                }
            } else if w.num_type(who, "Market") < 1 {
                return Some(SCRIPT_DONE);
            }
            if w.num_type_with_queued(who, "Tower") > cities - 1 {
                return Some(SCRIPT_DONE);
            }
            Some(BLOCK_ON_THIS)
        }
        _ => Some(SCRIPT_DONE),
    }
}
