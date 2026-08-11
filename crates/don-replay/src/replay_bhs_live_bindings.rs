//! Exact production-script call arguments and the first reached host boundary.
//!
//! `Leader::production_ai` `0x006c1960` invokes the selected production BHS entry
//! with four live `ScriptInt` cells.  This module owns that external-call shape and
//! the smallest strict ScenarioFuncSet cohort reached by stock `economic.bhs`.
//! Unsupported calls remain [`HostError::Unimplemented`]; the adapter never supplies
//! a plausible map style, object count, command result, or checksum value.

use crate::initial::InitialPlayer;
use crate::replay_bhs_runtime::{ReplayBhsBinding, LEADER_FLAG_HUMAN};
use don_bhs::{
    BuiltinDecl, Host, HostError, HostResult, Program, RuntimeError, Value, Vm, VmError,
};

/// `economic.bhs` labels, and therefore the values interpreted by
/// `Leader::production_ai` after a successful BHS call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductionDisposition {
    BlockOnThis,
    DontBlockOnThis,
    ScriptDone,
    Other(i32),
}

impl From<i32> for ProductionDisposition {
    fn from(value: i32) -> Self {
        match value {
            1 => Self::BlockOnThis,
            2 => Self::DontBlockOnThis,
            3 => Self::ScriptDone,
            other => Self::Other(other),
        }
    }
}

/// The two live fields not serialized by the replay's setup prefix.
///
/// `script_step` is `LeaderData+0x790`. `personality_rush` is the first signed
/// `Personality` axis at `LeaderData+0x6dd4`.  A caller must source both from its
/// persistent Leader owner; this adapter deliberately has no defaults for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductionRetainedState {
    pub script_step: i32,
    pub personality_rush: i32,
}

/// Logical argument order at the external `RunTimeEnv::run_script` boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayProductionCall {
    /// `LeaderData::who + 1`; BHS player arguments are one-based.
    pub who: i32,
    /// `LeaderData+0x790`, passed by reference and copied back after a successful run.
    pub step: i32,
    /// `Personality::rush + 2`. Stock `economic.bhs` declares but never reads it.
    pub boom_vs_rush: i32,
    /// Literal `5` constructed at `0x006c1a14`.
    pub num_loops: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductionCallBindError {
    AbsentPlayer { slot: u8 },
    HumanPlayer { slot: u8 },
    InvalidWho { slot: u8, who: u8 },
}

/// Derive every setup-backed argument without manufacturing the two retained fields.
pub fn bind_production_call(
    player: &InitialPlayer,
    retained: ProductionRetainedState,
) -> Result<ReplayProductionCall, ProductionCallBindError> {
    if !player.present {
        return Err(ProductionCallBindError::AbsentPlayer { slot: player.slot });
    }
    if player.flags & LEADER_FLAG_HUMAN != 0 {
        return Err(ProductionCallBindError::HumanPlayer { slot: player.slot });
    }
    if player.who > 7 {
        return Err(ProductionCallBindError::InvalidWho {
            slot: player.slot,
            who: player.who,
        });
    }
    Ok(ReplayProductionCall {
        who: i32::from(player.who) + 1,
        step: retained.script_step,
        boom_vs_rush: retained.personality_rush.wrapping_add(2),
        num_loops: 5,
    })
}

/// One City row on the exact fields read by the admitted prefix.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProductionCityImage {
    /// `CityData::flags & 1`.
    pub active: bool,
    /// `CityData+0x90`, returned by `find_city_with_num`.
    pub name: String,
    /// `CityData+0x14`, read by `was_city_attacked`.
    pub last_attacked: i32,
    /// `CityData+0x18`, read by `was_city_raided`.
    pub last_raided: i32,
}

/// One Leader row on the exact fields read by the admitted prefix.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProductionLeaderImage {
    /// The live 32-bit `LeaderData::leader_flags`, not the replay's two-byte setup copy.
    pub flags: u32,
    /// `LeaderData::city_num` at `+0x3f8`. It is intentionally distinct from the
    /// Cities pointer-array length below because retail reads both independently.
    pub city_num: i32,
    /// The selected Tribe's name at `TribeData+0x2c`.
    pub nation: String,
    /// The de-obfuscated `LeaderData::age` value.
    pub age: i32,
    /// Live `Cities::lists[who0]` rows in pointer-array order.
    pub cities: Vec<ProductionCityImage>,
}

/// Explicit live image required by the seven admitted handlers.
///
/// This is an input receipt, not an initializer. In particular, the replay setup
/// prefix does not contain city rows, attack stamps, live ages, or rule values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductionBuiltinImage {
    pub leaders: [ProductionLeaderImage; 8],
    /// `Rules::get_num(0x220 + age)` for ages 0 through 6. `None` is an unowned
    /// rules fact and fails closed if execution reaches it.
    pub techs_per_age: [Option<i32>; 7],
}

impl Default for ProductionBuiltinImage {
    fn default() -> Self {
        Self {
            leaders: std::array::from_fn(|_| ProductionLeaderImage::default()),
            techs_per_age: [None; 7],
        }
    }
}

/// Scalar call trace retained independently of VM coverage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProductionBuiltinValue {
    Int(i32),
    Str(String),
}

impl From<&Value> for ProductionBuiltinValue {
    fn from(value: &Value) -> Self {
        match value {
            Value::Int(value) => Self::Int(*value),
            Value::Str(value) => Self::Str(value.as_str().to_owned()),
            _ => unreachable!("admitted production builtins return only int or string"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductionBuiltinCall {
    pub index: u32,
    pub name: &'static str,
    pub args: Vec<ProductionBuiltinValue>,
    pub returned: ProductionBuiltinValue,
}

/// Exact builtin indices owned by this prefix.
pub const PRODUCTION_PREFIX_BUILTINS: [u32; 7] = [248, 258, 323, 358, 383, 712, 713];

/// A strict host for the first stock-economic prefix.
pub struct ReplayProductionBuiltinHost<'a> {
    image: &'a ProductionBuiltinImage,
    trace: Vec<ProductionBuiltinCall>,
}

impl<'a> ReplayProductionBuiltinHost<'a> {
    pub fn new(image: &'a ProductionBuiltinImage) -> Self {
        Self {
            image,
            trace: Vec::new(),
        }
    }

    pub fn trace(&self) -> &[ProductionBuiltinCall] {
        &self.trace
    }

    fn who0(args: &[Value], at: usize) -> Result<Option<usize>, HostError> {
        let raw = match args.get(at) {
            Some(Value::Int(value)) => *value,
            _ => return Err(HostError::BadArgs("production who argument is not int")),
        };
        let who0 = raw.wrapping_sub(1);
        Ok((0..=7).contains(&who0).then_some(who0 as usize))
    }

    fn int_arg(args: &[Value], at: usize) -> Result<i32, HostError> {
        match args.get(at) {
            Some(Value::Int(value)) => Ok(*value),
            _ => Err(HostError::BadArgs("production argument is not int")),
        }
    }

    fn str_arg(args: &[Value], at: usize) -> Result<&str, HostError> {
        match args.get(at) {
            Some(Value::Str(value)) => Ok(value.as_str()),
            _ => Err(HostError::BadArgs("production argument is not string")),
        }
    }

    fn leader(&self, who0: usize, present: bool) -> Option<&ProductionLeaderImage> {
        let leader = &self.image.leaders[who0];
        let required = if present { 3 } else { 1 };
        (leader.flags & required == required).then_some(leader)
    }

    fn city_event(&self, args: &[Value], attacked: bool) -> Result<Value, HostError> {
        let Some(who0) = Self::who0(args, 0)? else {
            return Ok(Value::Int(-1));
        };
        let Some(leader) = self.leader(who0, true) else {
            return Ok(Value::Int(-1));
        };
        let city_name = Self::str_arg(args, 1)?;
        let seconds = Self::int_arg(args, 2)?;
        // This is the complete subdomain reached by the shipped production scripts.
        // Named-city selection and positive timeout arithmetic remain separate frontiers.
        if !city_name.is_empty() || seconds != -1 {
            return Err(HostError::Unimplemented);
        }

        for city in &leader.cities {
            if !city.active {
                continue;
            }
            let stamp = if attacked {
                city.last_attacked
            } else {
                city.last_raided
            };
            if stamp != 0 {
                return Ok(Value::Int(1));
            }
        }
        Ok(Value::Int(0))
    }

    fn dispatch(&self, decl: &BuiltinDecl, args: &[Value]) -> HostResult {
        match decl.index {
            // age(who), `0x009e8f50`.
            248 => {
                let Some(who0) = Self::who0(args, 0)? else {
                    return Ok(Value::Int(-1));
                };
                Ok(Value::Int(
                    self.leader(who0, true).map_or(-1, |leader| leader.age),
                ))
            }
            // num_cities(who), `0x009e92f0`.
            258 => {
                let Some(who0) = Self::who0(args, 0)? else {
                    return Ok(Value::Int(-1));
                };
                Ok(Value::Int(
                    self.leader(who0, false)
                        .map_or(-1, |leader| leader.city_num),
                ))
            }
            // find_nation(who), `0x009ed190`.
            323 => {
                let Some(who0) = Self::who0(args, 0)? else {
                    return Ok(Value::str(""));
                };
                Ok(Value::str(
                    self.leader(who0, true)
                        .map_or("", |leader| leader.nation.as_str()),
                ))
            }
            // get_techs_per_age(who), `0x009ee8b0`.
            358 => {
                let Some(who0) = Self::who0(args, 0)? else {
                    return Ok(Value::Int(-1));
                };
                let Some(leader) = self.leader(who0, true) else {
                    return Ok(Value::Int(-1));
                };
                if leader.age > 6 {
                    return Ok(Value::Int(0));
                }
                let age = usize::try_from(leader.age).map_err(|_| HostError::Unimplemented)?;
                Ok(Value::Int(
                    self.image.techs_per_age[age].ok_or(HostError::Unimplemented)?,
                ))
            }
            // find_city_with_num(who, city_num), `0x009eff00`.
            383 => {
                let Some(who0) = Self::who0(args, 0)? else {
                    return Ok(Value::str(""));
                };
                let Some(leader) = self.leader(who0, false) else {
                    return Ok(Value::str(""));
                };
                let city0 = Self::int_arg(args, 1)?.wrapping_sub(1);
                if city0 < 0 || city0 >= leader.city_num {
                    return Ok(Value::str(""));
                }
                let city = leader
                    .cities
                    .get(city0 as usize)
                    .ok_or(HostError::Unimplemented)?;
                Ok(Value::str(if city.active {
                    city.name.as_str()
                } else {
                    ""
                }))
            }
            // was_city_raided / was_city_attacked.
            712 => self.city_event(args, false),
            713 => self.city_event(args, true),
            _ => Err(HostError::Unimplemented),
        }
    }
}

impl Host for ReplayProductionBuiltinHost<'_> {
    fn call(&mut self, decl: &BuiltinDecl, args: &[Value]) -> HostResult {
        let returned = self.dispatch(decl, args)?;
        self.trace.push(ProductionBuiltinCall {
            index: decl.index,
            name: decl.name,
            args: args.iter().map(ProductionBuiltinValue::from).collect(),
            returned: ProductionBuiltinValue::from(&returned),
        });
        Ok(returned)
    }
}

#[derive(Debug)]
pub enum ProductionRunFailure {
    BadFile(usize),
    MissingScript { file: usize, name: String },
    BadArity { actual: usize },
    Vm(VmError),
    Runtime(RuntimeError),
    StepWasNotInt,
    ReturnWasNotInt,
}

#[derive(Debug)]
pub struct ProductionRunError {
    pub failure: ProductionRunFailure,
    pub bytecodes_executed: u64,
    pub trace: Vec<ProductionBuiltinCall>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductionRunReceipt {
    pub before: ReplayProductionCall,
    pub after_step: i32,
    pub returned: i32,
    pub disposition: ProductionDisposition,
    pub bytecodes_executed: u64,
    pub trace: Vec<ProductionBuiltinCall>,
}

/// Execute one production call atomically over the Program and the `ref step` cell.
///
/// A missing DoN builtin is an implementation gap, not a retail script result. The
/// candidate Program is therefore discarded on every failure so partial static-variable
/// initializers cannot leak into channel 15.
pub fn run_production_call(
    program: &mut Program,
    binding: &ReplayBhsBinding,
    call: &mut ReplayProductionCall,
    image: &ProductionBuiltinImage,
) -> Result<ProductionRunReceipt, ProductionRunError> {
    let Some(file) = program.files.get(binding.file) else {
        return Err(ProductionRunError {
            failure: ProductionRunFailure::BadFile(binding.file),
            bytecodes_executed: 0,
            trace: Vec::new(),
        });
    };
    let Some(script) = file.find_script(&binding.name) else {
        return Err(ProductionRunError {
            failure: ProductionRunFailure::MissingScript {
                file: binding.file,
                name: binding.name.clone(),
            },
            bytecodes_executed: 0,
            trace: Vec::new(),
        });
    };
    let arity = file.scripts[script].arity;
    if arity != 4 {
        return Err(ProductionRunError {
            failure: ProductionRunFailure::BadArity { actual: arity },
            bytecodes_executed: 0,
            trace: Vec::new(),
        });
    }

    let before = *call;
    let mut args = [
        Value::Int(call.who),
        Value::Int(call.step),
        Value::Int(call.boom_vs_rush),
        Value::Int(call.num_loops),
    ];
    let mut candidate = program.clone();
    let mut host = ReplayProductionBuiltinHost::new(image);
    let (result, failed_bytecodes) = {
        let mut vm = Vm::new(&mut candidate, &mut host);
        let result = vm.run_script_index_mut(binding.file, script, &mut args);
        (result, vm.bytecodes_executed)
    };
    let trace = host.trace;
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(failure) => {
            return Err(ProductionRunError {
                failure: ProductionRunFailure::Vm(failure),
                bytecodes_executed: failed_bytecodes,
                trace,
            })
        }
    };
    if let Some(failure) = outcome.error {
        return Err(ProductionRunError {
            failure: ProductionRunFailure::Runtime(failure),
            bytecodes_executed: outcome.bytecodes_executed,
            trace,
        });
    }
    let Value::Int(after_step) = args[1] else {
        return Err(ProductionRunError {
            failure: ProductionRunFailure::StepWasNotInt,
            bytecodes_executed: outcome.bytecodes_executed,
            trace,
        });
    };
    let Some(Value::Int(returned)) = outcome.returned else {
        return Err(ProductionRunError {
            failure: ProductionRunFailure::ReturnWasNotInt,
            bytecodes_executed: outcome.bytecodes_executed,
            trace,
        });
    };

    *program = candidate;
    call.step = after_step;
    Ok(ProductionRunReceipt {
        before,
        after_step,
        returned,
        disposition: returned.into(),
        bytecodes_executed: outcome.bytecodes_executed,
        trace,
    })
}
