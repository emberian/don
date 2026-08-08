//! A recording test double for [`crate::api::ScriptWorld`].
//!
//! **This is not a simulation.** It is a stub that logs every host call and
//! answers from a table the test sets up, so that the transcription's *call
//! sequence and control flow* can be checked. It makes no claim about what the
//! engine would actually return; nothing derived may be built on it.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::api::ScriptWorld;

/// One recorded host-function call, rendered as `name(arg, arg, ...)`.
pub type CallLog = Vec<String>;

#[derive(Default)]
pub struct ProbeWorld {
    /// Answers keyed by the rendered call string; missing keys fall back to
    /// [`ProbeWorld::default_int`].
    pub answers: HashMap<String, i32>,
    /// String answers, same keying.
    pub string_answers: HashMap<String, String>,
    /// Value returned by any int query with no entry in `answers`.
    pub default_int: i32,
    pub log: RefCell<CallLog>,
}

impl ProbeWorld {
    pub fn new() -> Self {
        ProbeWorld { default_int: 0, ..Default::default() }
    }

    pub fn with(mut self, call: &str, v: i32) -> Self {
        self.answers.insert(call.to_string(), v);
        self
    }

    pub fn with_str(mut self, call: &str, v: &str) -> Self {
        self.string_answers.insert(call.to_string(), v.to_string());
        self
    }

    fn q(&self, call: String) -> i32 {
        let v = *self.answers.get(&call).unwrap_or(&self.default_int);
        self.log.borrow_mut().push(call);
        v
    }

    fn qs(&self, call: String) -> String {
        let v = self.string_answers.get(&call).cloned().unwrap_or_default();
        self.log.borrow_mut().push(call);
        v
    }

    pub fn calls(&self) -> CallLog {
        self.log.borrow().clone()
    }

    pub fn count(&self, prefix: &str) -> usize {
        self.log.borrow().iter().filter(|c| c.starts_with(prefix)).count()
    }
}

macro_rules! call {
    ($name:literal) => {
        concat!($name, "()").to_string()
    };
    ($name:literal, $($a:expr),+ $(,)?) => {
        format!(concat!($name, "({})"), [$(format!("{}", $a)),+].join(","))
    };
}

impl ScriptWorld for ProbeWorld {
    fn get_mapstyle(&self) -> String {
        self.qs(call!("get_mapstyle"))
    }
    fn get_is_no_nation_powers(&self) -> i32 {
        self.q(call!("get_is_no_nation_powers"))
    }
    fn get_rush_rules(&self) -> i32 {
        self.q(call!("get_rush_rules"))
    }
    fn is_conquest_scenario(&self) -> i32 {
        self.q(call!("is_conquest_scenario"))
    }
    fn population(&self, who: i32) -> i32 {
        self.q(call!("population", who))
    }
    fn age(&self, who: i32) -> i32 {
        self.q(call!("age", who))
    }
    fn get_starting_town_size(&self, who: i32) -> i32 {
        self.q(call!("get_starting_town_size", who))
    }
    fn get_starting_resources(&self, who: i32) -> i32 {
        self.q(call!("get_starting_resources", who))
    }
    fn num_cities(&self, who: i32) -> i32 {
        self.q(call!("num_cities", who))
    }
    fn num_type(&self, who: i32, object_type: &str) -> i32 {
        self.q(call!("num_type", who, object_type))
    }
    fn num_type_with_queued(&self, who: i32, unit_type: &str) -> i32 {
        self.q(call!("num_type_with_queued", who, unit_type))
    }
    fn num_rare_resources_seen(&self, who: i32) -> i32 {
        self.q(call!("num_rare_resources_seen", who))
    }
    fn find_nation(&self, who: i32) -> String {
        self.qs(call!("find_nation", who))
    }
    fn at_least_type(&self, who: i32, num: i32, ty: &str) -> i32 {
        self.q(call!("at_least_type", who, num, ty))
    }
    fn get_techs_per_age(&self, who: i32) -> i32 {
        self.q(call!("get_techs_per_age", who))
    }
    fn have_tech(&self, who: i32, tech_type: &str) -> i32 {
        self.q(call!("have_tech", who, tech_type))
    }
    fn researching_tech(&self, who: i32, tech_type: &str) -> i32 {
        self.q(call!("researching_tech", who, tech_type))
    }
    fn can_pay_cost(&self, who: i32, ty: &str) -> i32 {
        self.q(call!("can_pay_cost", who, ty))
    }
    fn find_idle_citizen(&self, who: i32) -> i32 {
        self.q(call!("find_idle_citizen", who))
    }
    fn find_unit(&self, who: i32, unit_type: &str) -> i32 {
        self.q(call!("find_unit", who, unit_type))
    }
    fn find_build(&self, who: i32, build_type: &str) -> i32 {
        self.q(call!("find_build", who, build_type))
    }
    fn find_build_at_city(
        &self,
        who: i32,
        city_name: &str,
        build_type: &str,
        count_inactive: i32,
    ) -> i32 {
        self.q(call!("find_build_at_city", who, city_name, build_type, count_inactive))
    }
    fn find_inactive_build(&self, who: i32, build_type: &str) -> i32 {
        self.q(call!("find_inactive_build", who, build_type))
    }
    fn find_city_id(&self, city_name: &str) -> i32 {
        self.q(call!("find_city_id", city_name))
    }
    fn find_city_with_num(&self, who: i32, city_num: i32) -> String {
        self.qs(call!("find_city_with_num", who, city_num))
    }
    fn num_city_buildings(
        &self,
        who: i32,
        city_name: &str,
        build_type: &str,
        count_inactive: i32,
    ) -> i32 {
        self.q(call!("num_city_buildings", who, city_name, build_type, count_inactive))
    }
    fn building_started(&self, who: i32, build_o: i32) -> i32 {
        self.q(call!("building_started", who, build_o))
    }
    fn num_workers_at_building(&self, who: i32, build_o: i32) -> i32 {
        self.q(call!("num_workers_at_building", who, build_o))
    }
    fn max_workers_at_building(&self, who: i32, build_o: i32) -> i32 {
        self.q(call!("max_workers_at_building", who, build_o))
    }
    fn num_type_queued(&self, who: i32, build_o: i32, unit_type: &str) -> i32 {
        self.q(call!("num_type_queued", who, build_o, unit_type))
    }
    fn find_num_idle_unit(&self, who: i32, unit_type: &str) -> i32 {
        self.q(call!("find_num_idle_unit", who, unit_type))
    }
    fn was_city_attacked(&self, who_defender: i32, city_name: &str, seconds: i32) -> i32 {
        self.q(call!("was_city_attacked", who_defender, city_name, seconds))
    }
    fn was_city_raided(&self, who_defender: i32, city_name: &str, seconds: i32) -> i32 {
        self.q(call!("was_city_raided", who_defender, city_name, seconds))
    }
    fn set_timer(&mut self, timer_id: &str, seconds: i32) -> i32 {
        self.q(call!("set_timer", timer_id, seconds))
    }
    fn stop_timer(&mut self, timer_id: &str) -> i32 {
        self.q(call!("stop_timer", timer_id))
    }
    fn timer_expired(&self, timer_id: &str) -> i32 {
        self.q(call!("timer_expired", timer_id))
    }
    fn research_tech_with_cost(&mut self, who: i32, tech: &str) -> i32 {
        self.q(call!("research_tech_with_cost", who, tech))
    }
    fn train_unit_with_cost(&mut self, who: i32, num: i32, unit_type: &str) -> i32 {
        self.q(call!("train_unit_with_cost", who, num, unit_type))
    }
    fn train_unit_at_with_cost(
        &mut self,
        who: i32,
        num: i32,
        unit_type: &str,
        build_o: i32,
    ) -> i32 {
        self.q(call!("train_unit_at_with_cost", who, num, unit_type, build_o))
    }
    fn place_building_with_cost(&mut self, who: i32, build_type: &str, city_name: &str) -> i32 {
        self.q(call!("place_building_with_cost", who, build_type, city_name))
    }
    fn place_orphan_building_with_cost(
        &mut self,
        who: i32,
        build_type: &str,
        build_o: i32,
    ) -> i32 {
        self.q(call!("place_orphan_building_with_cost", who, build_type, build_o))
    }
    fn place_building_upgrade_with_cost(
        &mut self,
        who: i32,
        build_type: &str,
        city_name: &str,
    ) -> i32 {
        self.q(call!("place_building_upgrade_with_cost", who, build_type, city_name))
    }
    fn place_city_with_cost(&mut self, who: i32) -> i32 {
        self.q(call!("place_city_with_cost", who))
    }
    fn destroy_building(&mut self, who: i32, build_o: i32) -> i32 {
        self.q(call!("destroy_building", who, build_o))
    }
    fn citizen_repair_order(&mut self, who: i32, unit_o: i32, build_o_target: i32) -> i32 {
        self.q(call!("citizen_repair_order", who, unit_o, build_o_target))
    }
}
