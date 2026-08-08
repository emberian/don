//! Transcription of `ron-data/ai-scripts/aibestbuildlibrary.bhs`.
//!
//! Only the entry points `economic.bhs` calls are here:
//! `place_farm`, `place_mine`, `place_dock`, `place_woodcutter`,
//! `train_unit_with_need`, `assign_idle`, `city_placement`.
//!
//! This is a **transcription, not an improvement**. Where the shipped script
//! has a bug (a wrong resource string, a truthiness test on a function that
//! returns `-1`) the bug is reproduced and marked `SHIPPED BUG`.

use crate::api::{bhs_true, ScriptWorld};

/// The `static` locals of the library.
///
/// [source] BHS `static` locals live on the *script function*, not on the
/// player, which is exactly why `economic.bhs` hand-rolls a "ghetto array"
/// indexed by `who-1` for its own per-player state. The library does **not**
/// do that, so `place_woodcutter::min_size` and `woodcutter_check`'s two
/// statics are genuinely shared across all eight AI players. That sharing is
/// reproduced here.
///
/// **UNVERIFIED**: that BHS `static` initialisers run once (C semantics)
/// rather than on every call. C semantics is assumed.
#[derive(Debug, Clone)]
pub struct LibraryStatics {
    /// `place_woodcutter`: `static int min_size = 5;`
    pub min_size: i32,
    /// `woodcutter_check`: `static int wood_check_size = 5;`
    pub wood_check_size: i32,
    /// `woodcutter_check`: `static int wood_camp_placed = 0;`
    pub wood_camp_placed: i32,
}

impl Default for LibraryStatics {
    fn default() -> Self {
        LibraryStatics { min_size: 5, wood_check_size: 5, wood_camp_placed: 0 }
    }
}

/// `int ai place_farm (int who)` — aibestbuildlibrary.bhs:608.
///
/// Returns 1 placed, 0 affordable-but-not-placed, -1 otherwise.
pub fn place_farm<W: ScriptWorld>(w: &mut W, who: i32) -> i32 {
    let who_nation = w.find_nation(who);
    let my_capital = w.find_city_with_num(who, 1);
    let my_second_city = w.find_city_with_num(who, 2);
    let my_third_city = w.find_city_with_num(who, 3);

    let max_farms =
        if who_nation == "Egyptians" && w.get_is_no_nation_powers() < 1 { 7 } else { 5 };

    let n = w.num_cities(who);
    if n == 1 {
        if w.num_city_buildings(who, &my_capital, "Farm", 1) < max_farms {
            if w.place_building_with_cost(who, "Farm", &my_capital) > 0 {
                return 1;
            }
        } else {
            return 0;
        }
    } else if n == 2 {
        if w.num_city_buildings(who, &my_capital, "Farm", 1) < max_farms {
            if w.place_building_with_cost(who, "Farm", &my_capital) > 0 {
                return 1;
            }
        } else if w.num_city_buildings(who, &my_second_city, "Farm", 1) < max_farms {
            if w.place_building_with_cost(who, "Farm", &my_second_city) > 0 {
                return 1;
            }
        } else {
            return 0;
        }
    } else if n == 3 {
        if w.num_city_buildings(who, &my_capital, "Farm", 1) < max_farms {
            if w.place_building_with_cost(who, "Farm", &my_capital) > 0 {
                return 1;
            }
        } else if w.num_city_buildings(who, &my_second_city, "Farm", 1) < max_farms {
            if w.place_building_with_cost(who, "Farm", &my_second_city) > 0 {
                return 1;
            }
        } else if w.num_city_buildings(who, &my_third_city, "Farm", 1) < max_farms {
            if w.place_building_with_cost(who, "Farm", &my_third_city) > 0 {
                return 1;
            }
        } else {
            return 0;
        }
    }
    // NOTE: four-or-more cities falls straight through, as in the script.
    if w.can_pay_cost(who, "Farm") > 0 {
        return 0;
    }
    -1
}

/// Shared body of `place_mine` (line 509) and `place_dock` (line 408). The two
/// script functions are textually identical apart from the building type, the
/// tech gate, and one typo — see `affordability_probe`.
fn place_resource_building<W: ScriptWorld>(
    w: &mut W,
    who: i32,
    build_type: &str,
    affordability_probe: &str,
) -> i32 {
    let my_capital = w.find_city_with_num(who, 1);
    let my_second_city = w.find_city_with_num(who, 2);
    let my_third_city = w.find_city_with_num(who, 3);

    let cities = w.num_cities(who);

    // Try each existing city directly, in order.
    let city_names: [&str; 3] = [&my_capital, &my_second_city, &my_third_city];
    if (1..=3).contains(&cities) {
        for name in city_names.iter().take(cities as usize) {
            if w.place_building_with_cost(who, build_type, name) > 0 {
                return 1;
            }
        }
        // Two- and three-city branches try a city under construction first.
        // SHIPPED BUG: the test is `if (find_inactive_build(...))`, not
        // `> 0`, so the -1 "none found" sentinel is *truthy* and this branch
        // is taken even when there is no city under construction — at which
        // point `place_orphan_building_with_cost(who, ty, -1)` runs and the
        // whole else-branch (the orphan sweep over camps/farms/library) is
        // skipped. Reproduced.
        if cities >= 2 {
            if bhs_true(w.find_inactive_build(who, "Small City")) {
                let building_cycle = w.find_inactive_build(who, "Small City");
                if w.place_orphan_building_with_cost(who, build_type, building_cycle) > 0 {
                    return 1;
                }
                return -1; // falls out of the if/else chain to `return -1`
            }
        }
        // Orphan sweep: woodcutter camps, then farms, then (capital only) the
        // library, city by city.
        for (i, name) in city_names.iter().enumerate().take(cities as usize) {
            let mut wc = w.num_city_buildings(who, name, "Woodcutter's Camp", 1);
            while wc > 0 {
                let building_cycle = w.find_build_at_city(who, name, "Woodcutter's Camp", 1);
                if w.place_orphan_building_with_cost(who, build_type, building_cycle) > 0 {
                    return 1;
                }
                wc -= 1;
            }
            let mut f = w.num_city_buildings(who, name, "Farm", 1);
            while f > 0 {
                let building_cycle = w.find_build_at_city(who, name, "Farm", 1);
                if w.place_orphan_building_with_cost(who, build_type, building_cycle) > 0 {
                    return 1;
                }
                f -= 1;
            }
            if i == 0 {
                let building_cycle = w.find_build_at_city(who, name, "Library", 1);
                if w.place_orphan_building_with_cost(who, build_type, building_cycle) > 0 {
                    return 1;
                }
            }
        }
        if w.can_pay_cost(who, affordability_probe) > 0 {
            return 0;
        }
        return -1;
    }
    -1
}

/// `int ai place_mine (int who)` — aibestbuildlibrary.bhs:509. Gated on
/// `have_tech(who, "Classical Age")`.
///
/// SHIPPED BUG: in the three-city branch the final affordability test is
/// `can_pay_cost(who, "Dock")`, not `"Mine"`. Reproduced by passing `"Dock"`
/// as the probe when the player has three cities.
pub fn place_mine<W: ScriptWorld>(w: &mut W, who: i32) -> i32 {
    if !bhs_true(w.have_tech(who, "Classical Age")) {
        return -1;
    }
    let probe = if w.num_cities(who) == 3 { "Dock" } else { "Mine" };
    place_resource_building(w, who, "Mine", probe)
}

/// `int ai place_dock (int who)` — aibestbuildlibrary.bhs:408. Gated on
/// `have_tech(who, "Barter")`.
pub fn place_dock<W: ScriptWorld>(w: &mut W, who: i32) -> i32 {
    if !bhs_true(w.have_tech(who, "Barter")) {
        return -1;
    }
    place_resource_building(w, who, "Dock", "Dock")
}

/// `int ai place_woodcutter (int who)` — aibestbuildlibrary.bhs:49.
///
/// Returns the camp's max worker count on success, 0 when affordable but not
/// placeable, -1 otherwise. The `min_size` ratchet is a shared static.
pub fn place_woodcutter<W: ScriptWorld>(w: &mut W, who: i32, st: &mut LibraryStatics) -> i32 {
    let my_capital = w.find_city_with_num(who, 1);

    // Placing at the capital when there is nowhere else.
    if w.num_cities(who) == 1 && w.find_inactive_build(who, "Small City") < 0 {
        if w.place_building_with_cost(who, "Woodcutter's Camp", &my_capital) > 0 {
            let camp = w.find_inactive_build(who, "Woodcutter's Camp");
            let size = w.max_workers_at_building(who, camp);
            if size < st.min_size && st.min_size > 3 {
                w.destroy_building(who, camp);
                st.min_size -= 1;
            } else {
                st.min_size = 5;
                return size;
            }
        } else if bhs_true(w.can_pay_cost(who, "Woodcutter's Camp")) {
            return 0;
        }
    } else if w.find_inactive_build(who, "Small City") > 0 {
        let my_new_city = w.find_inactive_build(who, "Small City");
        if w.place_orphan_building_with_cost(who, "Woodcutter's Camp", my_new_city) > 0 {
            let camp = w.find_inactive_build(who, "Woodcutter's Camp");
            let size = w.max_workers_at_building(who, camp);
            if size < st.min_size && st.min_size > 3 {
                w.destroy_building(who, camp);
                st.min_size -= 1;
            } else {
                st.min_size = 5;
                return size;
            }
        } else if w.place_building_with_cost(who, "Woodcutter's Camp", &my_capital) > 0 {
            let camp = w.find_inactive_build(who, "Woodcutter's Camp");
            let size = w.max_workers_at_building(who, camp);
            if size < st.min_size && st.min_size > 3 {
                w.destroy_building(who, camp);
                st.min_size -= 1;
            } else {
                st.min_size = 5;
                return size;
            }
        } else if bhs_true(w.can_pay_cost(who, "Woodcutter's Camp")) {
            return 0;
        }
    } else if w.num_cities(who) > 1 {
        let my_second_city = w.find_city_with_num(who, 2);
        if w.place_building_with_cost(who, "Woodcutter's Camp", &my_second_city) > 0 {
            let camp = w.find_inactive_build(who, "Woodcutter's Camp");
            return w.max_workers_at_building(who, camp);
        } else if w.place_building_with_cost(who, "Woodcutter's Camp", &my_capital) > 0 {
            let camp = w.find_inactive_build(who, "Woodcutter's Camp");
            return w.max_workers_at_building(who, camp);
        } else if w.can_pay_cost(who, "Woodcutter's Camp") > 0 {
            return 0;
        }
    }
    -1
}

/// `int ai train_unit_with_need (int who, int high_num, String what)`
/// — aibestbuildlibrary.bhs:126.
///
/// Counts unfilled gather slots per city (farms with zero workers, plus every
/// free slot at woodcutter camps and mines), subtracts what is already queued
/// there and the number of idle units of that type, and trains one unit at the
/// first city with a positive need. Loops until `num_type_with_queued` reaches
/// `high_num` or no city needs anything.
///
/// Returns 1 when the need is satisfied (and assigns idle citizens), else -1.
pub fn train_unit_with_need<W: ScriptWorld>(
    w: &mut W,
    who: i32,
    high_num: i32,
    what: &str,
) -> i32 {
    let my_capital = w.find_city_with_num(who, 1);
    let my_second_city = w.find_city_with_num(who, 2);
    let my_third_city = w.find_city_with_num(who, 3);
    let capital_id = w.find_city_id(&my_capital);
    let second_city_id = w.find_city_id(&my_second_city);
    let third_city_id = w.find_city_id(&my_third_city);

    let mut idle = 0;

    let mut pop = w.num_type_with_queued(who, what);
    while pop < high_num {
        idle = w.find_num_idle_unit(who, what);

        let needed_workers_1 = city_need(w, who, &my_capital, what, capital_id, idle, 1);
        let mut needed_workers_2 = 0;
        let mut needed_workers_3 = 0;
        // The script's own bookkeeping: the "aux" figure subtracts the
        // per-city figures that were computed, and stale values persist when
        // the city does not exist. Reproduced by keeping the running sums.
        let (mut sum_wood, mut sum_metal) = (0, 0);
        let (w1, m1) = city_free_slots(w, who, &my_capital);
        sum_wood += w1;
        sum_metal += m1;

        if w.num_cities(who) > 1 {
            needed_workers_2 =
                city_need(w, who, &my_second_city, what, second_city_id, idle, 0);
            let (w2, m2) = city_free_slots(w, who, &my_second_city);
            sum_wood += w2;
            sum_metal += m2;
        }
        if w.num_cities(who) > 2 {
            needed_workers_3 = city_need(w, who, &my_third_city, what, third_city_id, idle, 0);
            let (w3, m3) = city_free_slots(w, who, &my_third_city);
            sum_wood += w3;
            sum_metal += m3;
        }

        // Player-wide sweep over every camp and mine, including ones outside
        // any city.
        let mut needed_wood_aux = 0;
        let mut wc = w.num_type(who, "Woodcutter's Camp");
        while wc > 0 {
            let b = w.find_build(who, "Woodcutter's Camp");
            needed_wood_aux +=
                w.max_workers_at_building(who, b) - w.num_workers_at_building(who, b);
            wc -= 1;
        }
        let mut needed_metal_aux = 0;
        let mut m = w.num_type(who, "Mine");
        while m > 0 {
            let b = w.find_build(who, "Mine");
            needed_metal_aux +=
                w.max_workers_at_building(who, b) - w.num_workers_at_building(who, b);
            m -= 1;
        }
        let needed_workers_aux = needed_wood_aux + needed_metal_aux - sum_metal - sum_wood;

        if needed_workers_1 > 0 {
            w.train_unit_at_with_cost(who, 1, what, capital_id);
        } else if needed_workers_2 > 0 {
            w.train_unit_at_with_cost(who, 1, what, second_city_id);
        } else if needed_workers_3 > 0 {
            w.train_unit_at_with_cost(who, 1, what, third_city_id);
        } else if needed_workers_aux > 0 {
            w.train_unit_with_cost(who, 1, what);
        } else {
            return 1;
        }
        pop += 1;
    }

    if high_num <= w.num_type_with_queued(who, what) {
        if idle > 0 {
            assign_idle(w, who);
        }
        return 1;
    }
    -1
}

/// Free gather slots at one city: `(woodcutter slots, mine slots)`.
fn city_free_slots<W: ScriptWorld>(w: &mut W, who: i32, city: &str) -> (i32, i32) {
    let mut wood = 0;
    let mut wc = w.num_city_buildings(who, city, "Woodcutter's Camp", 1);
    while wc > 0 {
        let b = w.find_build_at_city(who, city, "Woodcutter's Camp", 1);
        wood += w.max_workers_at_building(who, b) - w.num_workers_at_building(who, b);
        wc -= 1;
    }
    let mut metal = 0;
    let mut m = w.num_city_buildings(who, city, "Mine", 1);
    while m > 0 {
        let b = w.find_build_at_city(who, city, "Mine", 1);
        metal += w.max_workers_at_building(who, b) - w.num_workers_at_building(who, b);
        m -= 1;
    }
    (wood, metal)
}

/// `needed_wood + needed_farm + needed_metal - num_type_queued - idle` for one
/// city.
///
/// `count_inactive_for_farms` reproduces a script inconsistency: the capital
/// counts farms with `bool_count_inactive = 1` and then looks them up with
/// `0`, while the second and third cities count with `0` and look up with `1`.
fn city_need<W: ScriptWorld>(
    w: &mut W,
    who: i32,
    city: &str,
    what: &str,
    city_id: i32,
    idle: i32,
    count_inactive_for_farms: i32,
) -> i32 {
    let mut needed_farm = 0;
    let farms = w.num_city_buildings(who, city, "Farm", count_inactive_for_farms);
    let mut f = farms;
    while f > 0 {
        let lookup_flag = if count_inactive_for_farms == 1 { 0 } else { 1 };
        let b = w.find_build_at_city(who, city, "Farm", lookup_flag);
        if w.num_workers_at_building(who, b) == 0 {
            needed_farm += 1;
        }
        f -= 1;
    }
    let (needed_wood, needed_metal) = city_free_slots(w, who, city);
    needed_wood + needed_farm + needed_metal - w.num_type_queued(who, city_id, what) - idle
}

/// `int ai assign_idle (int who)` — aibestbuildlibrary.bhs:310.
///
/// NOT TRANSCRIBED. The body walks idle citizens and issues gather orders via
/// `find_idle_citizen` / `citizen_repair_order` / `unit_move_order`, and its
/// behaviour depends on the engine's idle-unit ordering, which this lane has
/// not derived. Returning -1 (the script's own "nothing to do" value) keeps
/// the caller's control flow intact without inventing behaviour.
pub fn assign_idle<W: ScriptWorld>(_w: &mut W, _who: i32) -> i32 {
    -1
}

/// `int ai city_placement (int who)` — aibestbuildlibrary.bhs:8.
///
/// NOT TRANSCRIBED as written. The script body immediately `return -1;`s and
/// arms a `trigger city_build()` / `trigger health_check()` pair via
/// `enable_trigger`. How a `return 1` inside a trigger reaches the caller of
/// `city_placement` is **not established** — see the lane report, "what I could
/// not establish". Modelling it as anything other than "ask the engine" would
/// be inventing behaviour.
pub fn city_placement<W: ScriptWorld>(_w: &mut W, _who: i32) -> i32 {
    -1
}

/// `int ai woodcutter_check (int who, int max_woodcutters, int needed_workers)`
/// — aibestbuildlibrary.bhs:378. Unused by `economic.bhs`; transcribed for
/// completeness because it owns two of the shared statics.
pub fn woodcutter_check<W: ScriptWorld>(
    w: &mut W,
    who: i32,
    max_woodcutters: i32,
    needed_workers: i32,
    st: &mut LibraryStatics,
) -> i32 {
    let gate = (max_woodcutters <= st.wood_check_size
        || w.num_cities(who) > 1
        || w.find_inactive_build(who, "Small City") == 1)
        && st.wood_camp_placed == 0
        && w.num_type(who, "Woodcutter's Camp") == 1;
    if gate {
        let size = place_woodcutter(w, who, st);
        let total = max_woodcutters + size;
        train_unit_with_need(w, who, needed_workers, "Citizen");
        st.wood_camp_placed = 1;
        total
    } else if st.wood_camp_placed == 1 {
        -1
    } else {
        st.wood_check_size += 1;
        0
    }
}
