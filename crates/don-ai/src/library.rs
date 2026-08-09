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
    /// Whether [`city_placement`] runs the modelled trigger pump. **Off is the
    /// faithful setting**; on is an unverified interpretation that lets the
    /// build order past step 10. See [`city_placement`].
    pub model_triggers: bool,
    /// Per-player trigger state for [`city_placement`], used only when
    /// `model_triggers` is set. Per-player is itself a guess; see the docs.
    pub city_trigger: [CityTrigger; 8],
}

impl Default for LibraryStatics {
    fn default() -> Self {
        LibraryStatics {
            min_size: 5,
            wood_check_size: 5,
            wood_camp_placed: 0,
            model_triggers: false,
            city_trigger: [CityTrigger::None; 8],
        }
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

    let max_farms = if who_nation == "Egyptians" && w.get_is_no_nation_powers() < 1 {
        7
    } else {
        5
    };

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
    let probe = if w.num_cities(who) == 3 {
        "Dock"
    } else {
        "Mine"
    };
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
pub fn train_unit_with_need<W: ScriptWorld>(w: &mut W, who: i32, high_num: i32, what: &str) -> i32 {
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
            needed_workers_2 = city_need(w, who, &my_second_city, what, second_city_id, idle, 0);
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
/// **Now transcribed.** The earlier pass called this "not transcribed" on the
/// grounds that only the forward declaration at line 4 existed; the body is at
/// line 310 of the same file. It is line-for-line below.
///
/// Shape of the shipped code, preserved verbatim:
///
/// * The `for` loop over Woodcutter's Camps `return 4` the moment it finds one
///   with a free slot — **before assigning anyone**. So on the common path
///   `assign_idle` does nothing at all except report "a camp has room".
/// * `build_new_wood_camp` is set inside the loop, so it reflects only the
///   *last* camp examined.
/// * `wood_camp` is initialised to 0 and only assigned inside the branches, so
///   the trailing `if (wood_camp >= 0)` sweep runs with `wood_camp == 0` when
///   the player has no camps at all — object id 0. This model never issues id
///   0 (see `game::Building::id`), so that sweep finds nothing, which is the
///   same outcome as retail's "no such object".
/// * `static`-free: every local is fresh per call, including `been_here`, whose
///   `been_here > 1` test can therefore never be true. `SHIPPED BUG`.
/// * The double semicolon on `return -1;;` is in the shipped source.
// The shipped body fetches `my_second_city` at the top and again inside the
// branch that uses it. Both calls are kept because both are host calls the
// engine really makes.
#[allow(unused_assignments)]
pub fn assign_idle<W: ScriptWorld>(w: &mut W, who: i32) -> i32 {
    let my_capital = w.find_city_with_num(who, 1);
    let mut my_second_city = w.find_city_with_num(who, 2);
    let mut wood_camp = 0;
    let mut build_new_wood_camp = 0;
    // SHIPPED BUG: `been_here` is a plain local, so `been_here > 1` is dead.
    let mut been_here = 0;

    let idle = w.find_idle_citizen(who);
    if idle != 0 {
        let mut i = w.num_type(who, "Woodcutter's Camp");
        while i > 0 {
            wood_camp = w.find_build(who, "Woodcutter's Camp");
            if w.num_workers_at_building(who, wood_camp) < w.max_workers_at_building(who, wood_camp)
            {
                return 4;
            } else {
                build_new_wood_camp = 1;
            }
            i -= 1;
        }

        if build_new_wood_camp == 1 {
            if w.num_cities(who) >= 2 {
                my_second_city = w.find_city_with_num(who, 2);
                if w.place_building_with_cost(who, "Woodcutter's Camp", &my_second_city) > 0 {
                    let _ = w.find_inactive_build(who, "Woodcutter's Camp");
                    return 2;
                } else if w.place_building_with_cost(who, "Woodcutter's Camp", &my_capital) > 0 {
                    let _ = w.find_inactive_build(who, "Woodcutter's Camp");
                    return 1;
                } else if w.can_pay_cost(who, "Woodcutter's Camp") > 0 {
                    been_here += 1;
                    if been_here > 1 {
                        return 1;
                    } else {
                        return 0;
                    }
                }
            } else if w.place_building_with_cost(who, "Woodcutter's Camp", &my_capital) > 0 {
                let _ = w.find_inactive_build(who, "Woodcutter's Camp");
                return 1;
            } else if w.can_pay_cost(who, "Woodcutter's Camp") > 0 {
                return 0;
            }
        }

        if wood_camp >= 0 {
            let xpos = w.object_position_x(who, wood_camp);
            let ypos = w.object_position_y(who, wood_camp);
            let mut it = w.find_idle_citizen(who);
            // DEVIATION, marked: the shipped loop has no progress guarantee —
            // it re-queries `find_idle_citizen` and relies on the move order
            // eventually consuming every idle citizen. In retail a citizen
            // ordered to walk somewhere stops being idle immediately; here the
            // assignment can be refused (a full camp), which would spin
            // forever. The transcription therefore stops when an order is
            // refused. Reproducing a hang is not fidelity.
            while it > -1 {
                if w.unit_move_order(who, it, xpos, ypos) <= 0 {
                    break;
                }
                it = w.find_idle_citizen(who);
            }
        }
    }
    -1
}

/// Which trigger of `city_placement` is armed for a player.
///
/// `enable_trigger("name")` arms a named coroutine body; the shipped
/// `city_placement` arms `city_build`, returns `-1` immediately, and expects a
/// later invocation to run whichever trigger is armed. **The interpreter's
/// actual trigger scheduling is not derived** — see [`city_placement`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CityTrigger {
    /// Nothing armed; the next call arms `city_build`.
    #[default]
    None,
    CityBuild,
    HealthCheck,
    /// `health_check` returned 1; the pump is finished.
    Done,
}

/// `int ai city_placement (int who)` — aibestbuildlibrary.bhs:8.
///
/// # This is the one function the whole opening hangs on
///
/// `economic.bhs` step 10 ("Build City #2") advances only on
/// `city_placement(who) > 0`. The shipped body returns `-1` unconditionally and
/// arms two `trigger` bodies:
///
/// ```text
/// city_placement(who) {
///   new_city = 0;  enable_trigger("city_build");
///   trigger city_build()   { if (have_tech(who,"City State")>0) { place_city_with_cost(who); enable_trigger("health_check"); }
///                            else { research_tech_with_cost(who,"City State"); enable_trigger("city_build"); } }
///   trigger health_check() { if (find_inactive_build(who,"Small City")) {
///                              new_city = find_inactive_build(who,"Small City");
///                              if (building_started(who,new_city)) return 1;
///                              else enable_trigger("health_check"); }
///                            else enable_trigger("city_build"); }
///   return -1;
/// }
/// ```
///
/// so the `1` that step 10 waits for can only come out of a trigger body. How
/// the interpreter delivers it — and on what schedule triggers run — is
/// **still not derived**.
///
/// # Two behaviours, selectable, both marked
///
/// * `model_triggers == false` (retail-faithful-as-far-as-we-know): return
///   `-1` and nothing else. **Measured consequence:** the script parks on step
///   10 forever, and 300 script-seconds later its own hang watchdog
///   (`timer_expired` → `SCRIPT_DONE`) retires it. Steps 11..36 are then
///   unreachable. That is a real, reproducible outcome of not knowing this
///   mechanism, and it is what the default measures.
/// * `model_triggers == true` (**DEVIATION, unverified**): treat the pair as a
///   per-player state machine pumped once per `city_placement` call, in the
///   order the bodies would run if a trigger fired on the next invocation, and
///   return `1` when `health_check` would have. This is the reading that makes
///   the shipped build order *work*, which is weak evidence for it and no more.
///
/// A second unverified choice inside the second option: the trigger state is
/// kept **per player**. BHS `static`s are per script *function*, which is
/// exactly why `economic.bhs` hand-rolls a per-player array — so a shared
/// trigger state is equally plausible and would serialise all eight players'
/// city founding. Flagged, not resolved.
pub fn city_placement<W: ScriptWorld>(w: &mut W, who: i32, st: &mut LibraryStatics) -> i32 {
    if !st.model_triggers {
        return -1;
    }
    let slot = ((who - 1).clamp(0, 7)) as usize;
    match st.city_trigger[slot] {
        CityTrigger::Done => {
            st.city_trigger[slot] = CityTrigger::None;
            -1
        }
        CityTrigger::None => {
            st.city_trigger[slot] = CityTrigger::CityBuild;
            -1
        }
        CityTrigger::CityBuild => {
            if w.have_tech(who, "City State") > 0 {
                w.place_city_with_cost(who);
                st.city_trigger[slot] = CityTrigger::HealthCheck;
            } else {
                w.research_tech_with_cost(who, "City State");
                st.city_trigger[slot] = CityTrigger::CityBuild;
            }
            -1
        }
        CityTrigger::HealthCheck => {
            // SHIPPED BUG (reproduced): bare truthiness on a function whose
            // "not found" value is -1, so this branch is taken when there is
            // *no* city under construction as well as when there is one.
            if bhs_true(w.find_inactive_build(who, "Small City")) {
                let new_city = w.find_inactive_build(who, "Small City");
                if bhs_true(w.building_started(who, new_city)) {
                    st.city_trigger[slot] = CityTrigger::Done;
                    return 1;
                }
                st.city_trigger[slot] = CityTrigger::HealthCheck;
            } else {
                st.city_trigger[slot] = CityTrigger::CityBuild;
            }
            -1
        }
    }
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
