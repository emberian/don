//! The BHS host-function surface that `economic.bhs` and
//! `aibestbuildlibrary.bhs` actually call.
//!
//! Every signature here was recovered from the registration table the engine
//! builds in `FUN_009C7570` (52,300 bytes, `skipped_large` in
//! `re/decomp-all/MANIFEST.jsonl`). Each registration is
//!
//! ```text
//!   push  <arity>
//!   push  <impl VA>
//!   push  <name, UTF-16>
//!   push  <type tag>
//!   mov   ecx, <registry>
//!   call  0x009D4DA0            ; AddFunc  -> ScriptFunc* in eax
//!   ...
//!   push  ecx / push 0 / push 0
//!   push  <param name, UTF-16>
//!   push  <type tag>
//!   mov   ecx, eax
//!   call  0x009D4F20            ; AddParam
//! ```
//!
//! 842 functions are registered this way (35 names are C++ overloads
//! distinguished by arity). Param type tags observed: `0x00057BAD` = int,
//! `0x00168174` = string. The names are **not** ASCII in the binary — they are
//! UTF-16 literals in `.rdata`, which is why a naive `strings` sweep finds
//! nothing.
//!
//! Reproduce the table:
//! ```text
//! cd ron-bin && uv run --with capstone --with pefile python \
//!   ../re/scripts/dump_script_funcs.py > /tmp/sfuncs.json
//! ```
//! (script written by this lane; see the report for the inline version).
//!
//! ## Calling convention
//! [measured] the implementations are `__stdcall` and take **pointers to value
//! cells**, one per declared parameter, e.g. `get_leader_difficulty` @
//! `0x009E53B0` is `mov eax,[ebp+8]; mov eax,[eax]; ... ret 4`. Int results
//! come back in `eax`; string results are written through a hidden first
//! pointer argument (`find_city_with_num` @ `0x009EFF00`).
//!
//! ## Failure convention
//! [measured] every player-taking host function begins with
//! `who-1 < 8 && (player.flags0 & 1) [&& (player.flags0 & 2)]` and returns
//! `-1` (`0xFFFFFFFF`) if that fails. Name lookups scan a 0x326-entry (806)
//! table and also yield `-1` when the name is not found.

/// The read-only + command surface the shipped scripts use.
///
/// Implementors are the *engine*, not the script. Nothing here is derived
/// behaviour: these are the seams where the BHS layer hands off to compiled
/// C++, and this crate deliberately does not guess what the C++ does.
///
/// All methods mirror the binary's parameter order and names exactly. Return
/// values follow the engine's convention: `-1` for "invalid / not found",
/// otherwise a count, an object id, or `1`/`0` for success/failure of a
/// command.
pub trait ScriptWorld {
    // ---- game / rules queries (no player argument) -------------------------
    /// `get_mapstyle()` @ `0x009E4CC0`.
    fn get_mapstyle(&self) -> String;
    /// `get_is_no_nation_powers()` @ `0x009E5230`.
    fn get_is_no_nation_powers(&self) -> i32;
    /// `get_rush_rules()` @ `0x009E5240`.
    fn get_rush_rules(&self) -> i32;
    /// `is_conquest_scenario()` @ `0x009E6040`.
    fn is_conquest_scenario(&self) -> i32;

    // ---- player queries ----------------------------------------------------
    /// `population(who)` @ `0x009E8E70`.
    fn population(&self, who: i32) -> i32;
    /// `age(who)` @ `0x009E8F50`. Reads `[player+0x6EB8]+0xDC` XOR `0x62766`.
    fn age(&self, who: i32) -> i32;
    /// `get_starting_town_size(who)` @ `0x009E9170`.
    fn get_starting_town_size(&self, who: i32) -> i32;
    /// `get_starting_resources(who)` @ `0x009E91F0`.
    fn get_starting_resources(&self, who: i32) -> i32;
    /// `num_cities(who)` @ `0x009E92F0`. Reads `player+0x3F8`.
    fn num_cities(&self, who: i32) -> i32;
    /// `num_type(who, object_type)` @ `0x009E9320`.
    fn num_type(&self, who: i32, object_type: &str) -> i32;
    /// `num_type_with_queued(who, unit_type)` @ `0x009E9630`.
    fn num_type_with_queued(&self, who: i32, unit_type: &str) -> i32;
    /// `num_rare_resources_seen(who)` @ `0x009EA010`.
    fn num_rare_resources_seen(&self, who: i32) -> i32;
    /// `find_nation(who)` @ `0x009ED190`.
    fn find_nation(&self, who: i32) -> String;
    /// `at_least_type(who, num, r#type)` @ `0x009ED570`.
    fn at_least_type(&self, who: i32, num: i32, ty: &str) -> i32;
    /// `get_techs_per_age(who)` @ `0x009EE8B0`. Rules lookup at index
    /// `0x220 + age`; returns 0 when `age > 6`.
    fn get_techs_per_age(&self, who: i32) -> i32;
    /// `have_tech(who, tech_type)` @ `0x009EE990`.
    fn have_tech(&self, who: i32, tech_type: &str) -> i32;
    /// `researching_tech(who, tech_type)` @ `0x009EEA20`.
    fn researching_tech(&self, who: i32, tech_type: &str) -> i32;
    /// `can_pay_cost(who, r#type)` @ `0x009EEDC0`.
    fn can_pay_cost(&self, who: i32, ty: &str) -> i32;

    // ---- object / building lookup ------------------------------------------
    /// `find_idle_citizen(who)` @ `0x009EAD70`.
    fn find_idle_citizen(&self, who: i32) -> i32;
    /// `find_unit(who, unit_type)` @ `0x009EBE10`.
    fn find_unit(&self, who: i32, unit_type: &str) -> i32;
    /// `find_build(who, build_type)` @ `0x009EC160`.
    fn find_build(&self, who: i32, build_type: &str) -> i32;
    /// `find_build_at_city(who, city_name, build_type, bool_count_inactive)`
    /// @ `0x009EC7A0`.
    fn find_build_at_city(
        &self,
        who: i32,
        city_name: &str,
        build_type: &str,
        count_inactive: i32,
    ) -> i32;
    /// `find_inactive_build(who, build_type)` @ `0x009ECE40`.
    fn find_inactive_build(&self, who: i32, build_type: &str) -> i32;
    /// `find_city_id(city_name)` @ `0x009EF580`.
    fn find_city_id(&self, city_name: &str) -> i32;
    /// `find_city_with_num(who, city_num)` @ `0x009EFF00`. Returns a city
    /// *name*; the engine writes an empty `RString` (`DAT_00EB437C`) when the
    /// index is out of range.
    fn find_city_with_num(&self, who: i32, city_num: i32) -> String;
    /// `num_city_buildings(who, city_name, build_type, bool_count_inactive)`
    /// @ `0x009F0150`.
    fn num_city_buildings(
        &self,
        who: i32,
        city_name: &str,
        build_type: &str,
        count_inactive: i32,
    ) -> i32;
    /// `building_started(who, build_o)` @ `0x009F1F20`.
    fn building_started(&self, who: i32, build_o: i32) -> i32;
    /// `num_workers_at_building(who, build_o)` @ `0x009F2450`.
    fn num_workers_at_building(&self, who: i32, build_o: i32) -> i32;
    /// `max_workers_at_building(who, build_o)` @ `0x009F2520`.
    fn max_workers_at_building(&self, who: i32, build_o: i32) -> i32;
    /// `num_type_queued(who, build_o, unit_type)` @ `0x009F25E0`.
    fn num_type_queued(&self, who: i32, build_o: i32, unit_type: &str) -> i32;
    /// `find_num_idle_unit(who, unit_type)` @ `0x009F3390`.
    fn find_num_idle_unit(&self, who: i32, unit_type: &str) -> i32;

    // ---- city attack history ----------------------------------------------
    /// `was_city_attacked(who_defender, city_name, seconds)` @ `0x009FD510`.
    ///
    /// [measured] with `seconds < 1` the predicate is just "the city has a
    /// non-zero last-attacked stamp"; otherwise it is
    /// `(Game[0x550] - stamp) / 15 <= seconds`, i.e. the stamp is in
    /// fifteenths of the `seconds` unit.
    fn was_city_attacked(&self, who_defender: i32, city_name: &str, seconds: i32) -> i32;
    /// `was_city_raided(who_defender, city_name, seconds)` @ `0x009FD3E0`.
    fn was_city_raided(&self, who_defender: i32, city_name: &str, seconds: i32) -> i32;

    // ---- timers ------------------------------------------------------------
    /// `set_timer(timer_id, seconds)` @ `0x009E4BC0`.
    ///
    /// The declared type of `timer_id` is **String**, but `economic.bhs` calls
    /// `set_timer(who, 300)` with an int. Whether the compiler coerces or this
    /// is a latent script bug is **not established** — see the lane report.
    fn set_timer(&mut self, timer_id: &str, seconds: i32) -> i32;
    /// `stop_timer(timer_id)` @ `0x009E4C10`.
    fn stop_timer(&mut self, timer_id: &str) -> i32;
    /// `timer_expired(timer_id)` @ `0x009E4C80`.
    fn timer_expired(&self, timer_id: &str) -> i32;

    // ---- commands ----------------------------------------------------------
    /// `research_tech_with_cost(who, tech)` @ `0x009EE700`.
    fn research_tech_with_cost(&mut self, who: i32, tech: &str) -> i32;
    /// `train_unit_with_cost(who, num, unit_type)` @ `0x009F42D0`.
    fn train_unit_with_cost(&mut self, who: i32, num: i32, unit_type: &str) -> i32;
    /// `train_unit_at_with_cost(who, num, unit_type, build_o)` @ `0x009F4460`.
    fn train_unit_at_with_cost(
        &mut self,
        who: i32,
        num: i32,
        unit_type: &str,
        build_o: i32,
    ) -> i32;
    /// `place_building_with_cost(who, build_type, city_name)` @ `0x009F54A0`.
    fn place_building_with_cost(&mut self, who: i32, build_type: &str, city_name: &str) -> i32;
    /// `place_orphan_building_with_cost(who, build_type, build_o)`
    /// @ `0x009F5520`.
    fn place_orphan_building_with_cost(
        &mut self,
        who: i32,
        build_type: &str,
        build_o: i32,
    ) -> i32;
    /// `place_building_upgrade_with_cost(who, build_type, city_name)`
    /// @ `0x009F5680`.
    fn place_building_upgrade_with_cost(
        &mut self,
        who: i32,
        build_type: &str,
        city_name: &str,
    ) -> i32;
    /// `place_city_with_cost(who)` @ `0x009F5860`.
    fn place_city_with_cost(&mut self, who: i32) -> i32;
    /// `destroy_building(who, build_o)` @ `0x009F6FC0`.
    fn destroy_building(&mut self, who: i32, build_o: i32) -> i32;
    /// `citizen_repair_order(who, unit_o, build_o_target)` @ `0x009F85B0`.
    fn citizen_repair_order(&mut self, who: i32, unit_o: i32, build_o_target: i32) -> i32;

    // ---- unit orders, used by `assign_idle` --------------------------------
    /// `object_position_x(who, object)`.
    fn object_position_x(&self, who: i32, object: i32) -> i32;
    /// `object_position_y(who, object)`.
    fn object_position_y(&self, who: i32, object: i32) -> i32;
    /// `unit_move_order(who, unit_o, x, y)`.
    fn unit_move_order(&mut self, who: i32, unit_o: i32, x: i32, y: i32) -> i32;
}

/// BHS truthiness, applied to `if (expr)` where `expr` is an int.
///
/// **UNVERIFIED.** The engine's failure sentinel is `-1`, and the shipped
/// scripts mix `if (f(...))` with `if (f(...) > 0)` on functions that can
/// return `-1`, so the two readings differ in behaviour. C semantics
/// (`!= 0`) is assumed here; the interpreter's actual rule has not been
/// extracted. Every call site that depends on it goes through this function so
/// the assumption can be flipped in one place.
#[inline]
pub fn bhs_true(v: i32) -> bool {
    v != 0
}

/// Sentinel every player-taking host function returns for an invalid `who`,
/// an unresolvable name, or "not found". [measured] `or eax, 0xFFFFFFFF`.
pub const SCRIPT_INVALID: i32 = -1;
