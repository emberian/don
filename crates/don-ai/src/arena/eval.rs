//! RoNEval policy evaluation over the headless arena.
//!
//! This is a deterministic, batched *arena-model* evaluator.  It is deliberately not
//! called a retail-fidelity evaluator: [`World`](super::world::World) still has explicit
//! MODEL gaps and the result checksum below is not retail's lockstep checksum.  The useful
//! claim this module can make today is narrower and testable:
//!
//! * shipped-order, optimiser, Marshal, observation-only `Ai`, and action-head adapter
//!   policies play the same seeded schedule;
//! * every decision receives only [`Obs`](super::obs::Obs), including remembered enemies
//!   behind fog, and every action passes through [`World::submit`](super::world::World::submit);
//! * case assignment to workers cannot affect ordering, seeds, results, or checksums;
//! * the `MarshalHeads` policy converts Marshal commands to the public ten-head action
//!   layout and back, and must remain result-exact with native Marshal on equivalence probes.

use std::collections::BTreeMap;
use std::fmt;
use std::time::{Duration, Instant};

use don_env::eval::{ordered_parallel_map, EvalDigest};

use super::bots::ai::Ai;
use super::bots::boom::{CapFirst, ShippedOpening};
use super::bots::marshal::{Level, Marshal};
use super::bots::{Bot, HeadPolicy};
use super::match_run::{run_match, MatchConfig, MatchResult, Outcome};
use super::obs::Obs;

/// Policy surfaces included in the evaluator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum PolicyId {
    ShippedOpening = 0,
    CapFirst = 1,
    Marshal = 2,
    /// Marshal routed through `Cmd::to_heads` -> `HeadPolicy` -> `Cmd::from_heads`.
    MarshalHeads = 3,
    /// Improved-edition, observation-only strong player.
    Ai = 4,
}

impl PolicyId {
    pub const ALL: [PolicyId; 5] = [
        PolicyId::ShippedOpening,
        PolicyId::CapFirst,
        PolicyId::Marshal,
        PolicyId::MarshalHeads,
        PolicyId::Ai,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            PolicyId::ShippedOpening => "ShippedOpening",
            PolicyId::CapFirst => "CapFirst",
            PolicyId::Marshal => "Marshal",
            PolicyId::MarshalHeads => "MarshalHeads",
            PolicyId::Ai => "Ai",
        }
    }

    fn make(self) -> Box<dyn Bot> {
        match self {
            PolicyId::ShippedOpening => Box::<ShippedOpening>::default(),
            PolicyId::CapFirst => Box::<CapFirst>::default(),
            PolicyId::Marshal => Box::new(Marshal::new(Level::MARSHAL)),
            PolicyId::Ai => Box::<Ai>::default(),
            PolicyId::MarshalHeads => {
                let mut inner = Marshal::new(Level::MARSHAL);
                Box::new(HeadPolicy {
                    label: "MarshalHeads".into(),
                    f: move |obs: &Obs| {
                        let mut commands = Vec::new();
                        inner.act(obs, &mut commands);
                        commands
                            .into_iter()
                            .map(|command| {
                                let actor = command.actor();
                                let heads = command.to_heads(|target| obs.slot(target));
                                (actor, heads)
                            })
                            .collect()
                    },
                })
            }
        }
    }
}

impl fmt::Display for PolicyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvalCase {
    pub ordinal: usize,
    pub seed: u32,
    pub minutes: i64,
    pub seat0: PolicyId,
    pub seat1: PolicyId,
}

/// A balanced schedule: every unordered pairing, both seats, on every seed.
pub fn round_robin_cases(minutes: i64, seed_count: u32) -> Vec<EvalCase> {
    let mut cases = Vec::new();
    for seed_i in 0..seed_count.max(1) {
        let seed = evaluation_seed(seed_i);
        for a in 0..PolicyId::ALL.len() {
            for b in a + 1..PolicyId::ALL.len() {
                for &(seat0, seat1) in &[
                    (PolicyId::ALL[a], PolicyId::ALL[b]),
                    (PolicyId::ALL[b], PolicyId::ALL[a]),
                ] {
                    cases.push(EvalCase {
                        ordinal: cases.len(),
                        seed,
                        minutes: minutes.max(1),
                        seat0,
                        seat1,
                    });
                }
            }
        }
    }
    cases
}

/// Direct `Ai`/Marshal schedule, both seats on every seed.
pub fn strong_ai_cases(minutes: i64, seed_count: u32) -> Vec<EvalCase> {
    strong_ai_cases_from(minutes, 0, seed_count)
}

/// Direct schedule starting at a disjoint deterministic seed index.
///
/// Keeping the offset explicit lets tuning use one seed block and certification use a
/// later holdout block without inventing a second seed algorithm.
pub fn strong_ai_cases_from(minutes: i64, seed_offset: u32, seed_count: u32) -> Vec<EvalCase> {
    let mut cases = Vec::new();
    for seed_i in 0..seed_count.max(1) {
        let seed = evaluation_seed(seed_offset.wrapping_add(seed_i));
        for &(seat0, seat1) in &[
            (PolicyId::Ai, PolicyId::Marshal),
            (PolicyId::Marshal, PolicyId::Ai),
        ] {
            cases.push(EvalCase {
                ordinal: cases.len(),
                seed,
                minutes: minutes.max(1),
                seat0,
                seat1,
            });
        }
    }
    cases
}

fn evaluation_seed(index: u32) -> u32 {
    0x5EED_0001u32.wrapping_add(index.wrapping_mul(0x9E37_79B9))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseReport {
    pub case: EvalCase,
    pub outcome: Outcome,
    pub verdict: Option<PolicyId>,
    pub verdict_reason: &'static str,
    pub frames: i64,
    pub result_checksum: u64,
    pub commands: [u64; 2],
    pub accepted: [u32; 2],
    pub refused: [u32; 2],
    pub invalid: [u32; 2],
    pub scores: [super::world::Score; 2],
    /// Optional deterministic arena event trace (`DON_RONEVAL_LOG=1`).
    pub events: Vec<(i64, u8, String)>,
}

impl CaseReport {
    fn policy(&self, seat: usize) -> PolicyId {
        if seat == 0 {
            self.case.seat0
        } else {
            self.case.seat1
        }
    }
}

/// Aggregates retain concrete win reasons and rejected actions instead of inventing a
/// scalar "skill score" from incomparable quantities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyReport {
    pub policy: PolicyId,
    pub played: u32,
    pub elimination_wins: u32,
    pub elimination_losses: u32,
    pub timeout_tiebreak_wins: u32,
    pub timeout_tiebreak_losses: u32,
    pub draws: u32,
    pub commands: u64,
    pub accepted: u64,
    pub refused: u64,
    pub invalid: u64,
    pub final_resources: i64,
    pub final_damage: i64,
    pub final_army_value: i64,
}

impl PolicyReport {
    fn new(policy: PolicyId) -> Self {
        PolicyReport {
            policy,
            played: 0,
            elimination_wins: 0,
            elimination_losses: 0,
            timeout_tiebreak_wins: 0,
            timeout_tiebreak_losses: 0,
            draws: 0,
            commands: 0,
            accepted: 0,
            refused: 0,
            invalid: 0,
            final_resources: 0,
            final_damage: 0,
            final_army_value: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Readiness {
    pub label: &'static str,
    pub roneval_release_ready: bool,
    pub fog_safe_observations: bool,
    pub difficulty_income_bonus: bool,
    pub retail_lockstep_checksum: bool,
    pub blockers: &'static [&'static str],
}

pub const READINESS: Readiness = Readiness {
    label: "arena-model-evaluation-only",
    roneval_release_ready: false,
    fog_safe_observations: true,
    difficulty_income_bonus: false,
    retail_lockstep_checksum: false,
    blockers: &[
        "arena fidelity remains Tier C and is not differentially measured against retail",
        "water, naval, air, diplomacy, attrition, and supply are not full arena dynamics",
        "result checksums identify evaluator output; they are not CheckSum::walk_data",
        "this measures arena matches, not don-env policy inference or accelerator transfer",
    ],
};

#[derive(Clone, Debug)]
pub struct EvalReport {
    pub readiness: Readiness,
    pub requested_threads: usize,
    pub worker_threads: usize,
    pub serial_checksum: u64,
    pub batch_checksum: u64,
    pub thread_equivalent: bool,
    pub adapter_equivalent: bool,
    pub strong_ai: StrongAiGate,
    pub elapsed: Duration,
    pub cases: Vec<CaseReport>,
    pub policies: Vec<PolicyReport>,
}

#[derive(Clone, Debug)]
pub struct StrongAiReport {
    pub requested_threads: usize,
    pub worker_threads: usize,
    pub serial_checksum: u64,
    pub batch_checksum: u64,
    pub elapsed: Duration,
    pub gate: StrongAiGate,
    pub cases: Vec<CaseReport>,
}

impl StrongAiReport {
    pub fn render_text(&self) -> String {
        use std::fmt::Write as _;
        let frames: i64 = self.cases.iter().map(|case| case.frames).sum();
        let mut out = String::new();
        writeln!(out, "don.roneval.ai-head-to-head.v2").unwrap();
        writeln!(out, "readiness={}", READINESS.label).unwrap();
        writeln!(
            out,
            "threads=requested:{} used:{} equivalent:true",
            self.requested_threads, self.worker_threads
        )
        .unwrap();
        writeln!(out, "serial_checksum={:#018x}", self.serial_checksum).unwrap();
        writeln!(out, "batch_checksum={:#018x}", self.batch_checksum).unwrap();
        writeln!(
            out,
            "strong_ai_gate=passed:{} direct_matches:{} wins:{} losses:{} draws:{} paired_seeds:{} paired_wins:{} paired_losses:{} paired_ties:{} paired_one_sided_p:{:.6}",
            self.gate.passed,
            self.gate.direct_matches,
            self.gate.wins,
            self.gate.losses,
            self.gate.draws,
            self.gate.paired_seeds,
            self.gate.paired_wins,
            self.gate.paired_losses,
            self.gate.paired_ties,
            self.gate.one_sided_p,
        )
        .unwrap();
        writeln!(
            out,
            "raw_delta=cities:{} damage:{} resources:{} army_value:{}",
            self.gate.city_delta,
            self.gate.damage_delta,
            self.gate.resources_delta,
            self.gate.army_value_delta,
        )
        .unwrap();
        writeln!(
            out,
            "throughput=matches/s:{:.3} simulated_frames/s:{:.0} elapsed_s:{:.3}",
            self.cases.len() as f64 / self.elapsed.as_secs_f64().max(f64::MIN_POSITIVE),
            frames as f64 / self.elapsed.as_secs_f64().max(f64::MIN_POSITIVE),
            self.elapsed.as_secs_f64(),
        )
        .unwrap();
        writeln!(
            out,
            "seed,seat0,seat1,winner,reason,ai_cities,marshal_cities,ai_damage,marshal_damage,ai_resources,marshal_resources,ai_army,marshal_army,checksum"
        )
        .unwrap();
        for case in &self.cases {
            let ai_seat = usize::from(case.case.seat1 == PolicyId::Ai);
            let marshal_seat = 1 - ai_seat;
            writeln!(
                out,
                "{:#010x},{},{},{},{},{},{},{},{},{},{},{},{},{:#018x}",
                case.case.seed,
                case.case.seat0,
                case.case.seat1,
                case.verdict.map(PolicyId::label).unwrap_or("draw"),
                case.verdict_reason,
                case.scores[ai_seat].cities,
                case.scores[marshal_seat].cities,
                case.scores[ai_seat].damage_dealt,
                case.scores[marshal_seat].damage_dealt,
                case.scores[ai_seat].resources,
                case.scores[marshal_seat].resources,
                case.scores[ai_seat].army_value,
                case.scores[marshal_seat].army_value,
                case.result_checksum,
            )
            .unwrap();
            for (frame, who, text) in &case.events {
                writeln!(out, "event,{:#010x},{frame},{who},{text}", case.case.seed).unwrap();
            }
        }
        out
    }
}

impl EvalReport {
    pub fn matches_per_second(&self) -> f64 {
        self.cases.len() as f64 / self.elapsed.as_secs_f64().max(f64::MIN_POSITIVE)
    }

    pub fn simulated_frames_per_second(&self) -> f64 {
        let frames: i64 = self.cases.iter().map(|case| case.frames).sum();
        frames as f64 / self.elapsed.as_secs_f64().max(f64::MIN_POSITIVE)
    }

    /// Stable text suitable for CI logs and checked-in benchmark records.
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        use std::fmt::Write as _;
        writeln!(out, "don.roneval.arena.v2").unwrap();
        writeln!(out, "readiness={}", self.readiness.label).unwrap();
        writeln!(
            out,
            "roneval_release_ready={}",
            self.readiness.roneval_release_ready
        )
        .unwrap();
        writeln!(
            out,
            "fog_safe_observations={}",
            self.readiness.fog_safe_observations
        )
        .unwrap();
        writeln!(
            out,
            "difficulty_income_bonus={}",
            self.readiness.difficulty_income_bonus
        )
        .unwrap();
        writeln!(
            out,
            "retail_lockstep_checksum={}",
            self.readiness.retail_lockstep_checksum
        )
        .unwrap();
        writeln!(
            out,
            "threads=requested:{} used:{} equivalent:{}",
            self.requested_threads, self.worker_threads, self.thread_equivalent
        )
        .unwrap();
        writeln!(out, "adapter_equivalent={}", self.adapter_equivalent).unwrap();
        writeln!(
            out,
            "strong_ai_gate=passed:{} direct_matches:{} wins:{} losses:{} draws:{} paired_seeds:{} paired_wins:{} paired_losses:{} paired_ties:{} paired_one_sided_p:{:.6}",
            self.strong_ai.passed,
            self.strong_ai.direct_matches,
            self.strong_ai.wins,
            self.strong_ai.losses,
            self.strong_ai.draws,
            self.strong_ai.paired_seeds,
            self.strong_ai.paired_wins,
            self.strong_ai.paired_losses,
            self.strong_ai.paired_ties,
            self.strong_ai.one_sided_p,
        )
        .unwrap();
        writeln!(
            out,
            "strong_ai_raw_delta=cities:{} damage:{} resources:{} army_value:{}",
            self.strong_ai.city_delta,
            self.strong_ai.damage_delta,
            self.strong_ai.resources_delta,
            self.strong_ai.army_value_delta,
        )
        .unwrap();
        writeln!(out, "serial_checksum={:#018x}", self.serial_checksum).unwrap();
        writeln!(out, "batch_checksum={:#018x}", self.batch_checksum).unwrap();
        writeln!(
            out,
            "throughput=matches/s:{:.3} simulated_frames/s:{:.0} elapsed_s:{:.3}",
            self.matches_per_second(),
            self.simulated_frames_per_second(),
            self.elapsed.as_secs_f64()
        )
        .unwrap();
        writeln!(
            out,
            "policy,played,elim_w,elim_l,timeout_w,timeout_l,draw,commands,accepted,refused,invalid,resources,damage,army_value"
        )
        .unwrap();
        for policy in &self.policies {
            writeln!(
                out,
                "{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                policy.policy,
                policy.played,
                policy.elimination_wins,
                policy.elimination_losses,
                policy.timeout_tiebreak_wins,
                policy.timeout_tiebreak_losses,
                policy.draws,
                policy.commands,
                policy.accepted,
                policy.refused,
                policy.invalid,
                policy.final_resources,
                policy.final_damage,
                policy.final_army_value,
            )
            .unwrap();
        }
        writeln!(out, "blockers:").unwrap();
        for blocker in self.readiness.blockers {
            writeln!(out, "- {blocker}").unwrap();
        }
        out
    }
}

/// Execute a balanced schedule, first serially as the reference and then at the requested
/// worker count.  A mismatch fails closed instead of emitting a performance number for a
/// schedule-dependent evaluator.
pub fn evaluate_verified(
    minutes: i64,
    seed_count: u32,
    requested_threads: usize,
) -> Result<EvalReport, String> {
    let cases = round_robin_cases(minutes, seed_count);
    let serial_started = Instant::now();
    let serial = run_cases(&cases, 1)?;
    let serial_elapsed = serial_started.elapsed();
    let serial_checksum = batch_checksum(&serial);
    let workers = requested_threads.max(1).min(cases.len().max(1));

    let (parallel, elapsed) = if workers == 1 {
        (serial.clone(), serial_elapsed)
    } else {
        let started = Instant::now();
        let reports = run_cases(&cases, workers)?;
        (reports, started.elapsed())
    };
    let checksum = batch_checksum(&parallel);
    if serial != parallel || serial_checksum != checksum {
        let first = serial
            .iter()
            .zip(&parallel)
            .position(|(a, b)| a != b)
            .map(|i| i.to_string())
            .unwrap_or_else(|| "batch checksum".into());
        return Err(format!(
            "thread-count equivalence failed at {first}: serial={serial_checksum:#018x} parallel={checksum:#018x}"
        ));
    }
    let adapter_equivalent = adapter_equivalence(&parallel);
    if !adapter_equivalent {
        return Err(
            "MarshalHeads diverged from native Marshal on CapFirst equivalence probes".into(),
        );
    }

    let strong_ai = strong_ai_gate(&parallel);
    Ok(EvalReport {
        readiness: READINESS,
        requested_threads,
        worker_threads: workers,
        serial_checksum,
        batch_checksum: checksum,
        thread_equivalent: true,
        adapter_equivalent,
        strong_ai,
        elapsed,
        policies: aggregate(&parallel),
        cases: parallel,
    })
}

/// Certification entry point for the improved-edition strong player.
///
/// A pass requires at least sixteen complete seat-swapped seed pairs, a one-sided exact
/// sign-test probability at or below 0.05 on decisive pair margins, and positive
/// aggregate damage. Two matches on one map are deliberately one sample, not two
/// independent trials. Raw score deltas remain in the report; the gate does not invent a
/// weighted scalar.
pub fn evaluate_strong_ai_verified(
    minutes: i64,
    seed_count: u32,
    requested_threads: usize,
) -> Result<EvalReport, String> {
    let report = evaluate_verified(minutes, seed_count, requested_threads)?;
    if !report.strong_ai.passed {
        return Err(format!(
            "Ai strong-AI gate failed: matches {}-{}-{}; paired seeds {}-{}-{} over {}, p={:.6}, damage delta {}",
            report.strong_ai.wins,
            report.strong_ai.losses,
            report.strong_ai.draws,
            report.strong_ai.paired_wins,
            report.strong_ai.paired_losses,
            report.strong_ai.paired_ties,
            report.strong_ai.paired_seeds,
            report.strong_ai.one_sided_p,
            report.strong_ai.damage_delta,
        ));
    }
    Ok(report)
}

/// Fast direct diagnostic/certification schedule without unrelated round-robin pairings.
pub fn evaluate_strong_ai_head_to_head_verified(
    minutes: i64,
    seed_count: u32,
    requested_threads: usize,
) -> Result<StrongAiReport, String> {
    evaluate_strong_ai_head_to_head_from_verified(minutes, 0, seed_count, requested_threads)
}

/// Direct diagnostic/certification schedule over a deterministic seed block.
pub fn evaluate_strong_ai_head_to_head_from_verified(
    minutes: i64,
    seed_offset: u32,
    seed_count: u32,
    requested_threads: usize,
) -> Result<StrongAiReport, String> {
    let cases = strong_ai_cases_from(minutes, seed_offset, seed_count);
    let serial_started = Instant::now();
    let serial = run_cases(&cases, 1)?;
    let serial_elapsed = serial_started.elapsed();
    let serial_checksum = batch_checksum(&serial);
    let workers = requested_threads.max(1).min(cases.len().max(1));
    let (parallel, elapsed) = if workers == 1 {
        (serial.clone(), serial_elapsed)
    } else {
        let started = Instant::now();
        let reports = run_cases(&cases, workers)?;
        (reports, started.elapsed())
    };
    let checksum = batch_checksum(&parallel);
    if serial != parallel || serial_checksum != checksum {
        return Err(format!(
            "Ai thread-count equivalence failed: serial={serial_checksum:#018x} parallel={checksum:#018x}"
        ));
    }
    Ok(StrongAiReport {
        requested_threads,
        worker_threads: workers,
        serial_checksum,
        batch_checksum: checksum,
        elapsed,
        gate: strong_ai_gate(&parallel),
        cases: parallel,
    })
}

fn run_cases(cases: &[EvalCase], threads: usize) -> Result<Vec<CaseReport>, String> {
    ordered_parallel_map(cases, threads, |_, case| run_case(*case))
        .into_iter()
        .collect()
}

fn run_case(case: EvalCase) -> Result<CaseReport, String> {
    let mut cfg = MatchConfig {
        minutes: case.minutes,
        logging: std::env::var_os("DON_RONEVAL_LOG").is_some(),
        ..MatchConfig::default()
    };
    cfg.map.seed = case.seed;
    let mut bots = vec![case.seat0.make(), case.seat1.make()];
    let result = run_match(&cfg, &mut bots)?;
    Ok(case_report(case, &result))
}

fn case_report(case: EvalCase, result: &MatchResult) -> CaseReport {
    let (winner, verdict_reason) = result.verdict();
    CaseReport {
        case,
        outcome: result.outcome,
        verdict: winner.map(|seat| if seat == 0 { case.seat0 } else { case.seat1 }),
        verdict_reason,
        frames: result.frames,
        result_checksum: result_checksum(result),
        commands: [result.commands[0], result.commands[1]],
        accepted: [result.orders_ok[0], result.orders_ok[1]],
        refused: [result.orders_refused[0], result.orders_refused[1]],
        invalid: [result.orders_invalid[0], result.orders_invalid[1]],
        scores: [result.scores[0], result.scores[1]],
        events: result
            .log
            .iter()
            .map(|event| (event.frame, event.who, event.text.clone()))
            .collect(),
    }
}

/// Result identity, intentionally excluding bot display names and elapsed time.
fn result_checksum(result: &MatchResult) -> u64 {
    let mut h = EvalDigest::new();
    h.write_str("don.roneval.arena-result.v1");
    match result.outcome {
        Outcome::Elimination { winner, frame } => {
            h.write_u8(1);
            h.write_u64(winner as u64);
            h.write_i64(frame);
        }
        Outcome::Timeout => h.write_u8(0),
    }
    h.write_i64(result.frames);
    for score in &result.scores {
        h.write_i64(score.cities);
        h.write_i64(score.buildings);
        h.write_i64(score.building_value);
        h.write_i64(score.army);
        h.write_i64(score.army_value);
        h.write_i64(score.civilians);
        h.write_i64(score.resources);
        h.write_i64(score.damage_dealt);
        h.write_i64(score.age);
    }
    for values in [
        &result.orders_ok,
        &result.orders_refused,
        &result.orders_invalid,
    ] {
        h.write_u64(values.len() as u64);
        for &value in values {
            h.write_u32(value);
        }
    }
    h.write_u64(result.commands.len() as u64);
    for &value in &result.commands {
        h.write_u64(value);
    }
    for value in result.flank_hist {
        h.write_u64(value);
    }
    h.write_u64(result.shots);
    h.write_u64(result.rejects.len() as u64);
    for ((who, verb, tag), count) in &result.rejects {
        h.write_u8(*who);
        h.write_str(verb);
        h.write_str(tag);
        h.write_u32(*count);
    }
    h.finish()
}

fn batch_checksum(reports: &[CaseReport]) -> u64 {
    let mut h = EvalDigest::new();
    h.write_str("don.roneval.arena-batch.v1");
    h.write_u64(reports.len() as u64);
    for report in reports {
        h.write_u64(report.case.ordinal as u64);
        h.write_u32(report.case.seed);
        h.write_i64(report.case.minutes);
        h.write_u8(report.case.seat0 as u8);
        h.write_u8(report.case.seat1 as u8);
        h.write_u64(report.result_checksum);
    }
    h.finish()
}

fn adapter_equivalence(reports: &[CaseReport]) -> bool {
    reports
        .iter()
        .filter(|report| {
            (report.case.seat0 == PolicyId::Marshal && report.case.seat1 == PolicyId::CapFirst)
                || (report.case.seat0 == PolicyId::CapFirst
                    && report.case.seat1 == PolicyId::Marshal)
        })
        .all(|native| {
            reports.iter().any(|heads| {
                heads.case.seed == native.case.seed
                    && heads.case.minutes == native.case.minutes
                    && heads.case.seat0
                        == if native.case.seat0 == PolicyId::Marshal {
                            PolicyId::MarshalHeads
                        } else {
                            native.case.seat0
                        }
                    && heads.case.seat1
                        == if native.case.seat1 == PolicyId::Marshal {
                            PolicyId::MarshalHeads
                        } else {
                            native.case.seat1
                        }
                    && heads.result_checksum == native.result_checksum
            })
        })
}

fn aggregate(reports: &[CaseReport]) -> Vec<PolicyReport> {
    let mut policies: Vec<_> = PolicyId::ALL.into_iter().map(PolicyReport::new).collect();
    for case in reports {
        for seat in 0..2 {
            let policy = case.policy(seat);
            let row = &mut policies[policy as usize];
            row.played += 1;
            row.commands += case.commands[seat];
            row.accepted += u64::from(case.accepted[seat]);
            row.refused += u64::from(case.refused[seat]);
            row.invalid += u64::from(case.invalid[seat]);
            row.final_resources += case.scores[seat].resources;
            row.final_damage += case.scores[seat].damage_dealt;
            row.final_army_value += case.scores[seat].army_value;
            match (case.outcome, case.verdict) {
                (Outcome::Elimination { .. }, Some(winner)) if winner == policy => {
                    row.elimination_wins += 1
                }
                (Outcome::Elimination { .. }, Some(_)) => row.elimination_losses += 1,
                (Outcome::Timeout, Some(winner)) if winner == policy => {
                    row.timeout_tiebreak_wins += 1
                }
                (Outcome::Timeout, Some(_)) => row.timeout_tiebreak_losses += 1,
                (_, None) => row.draws += 1,
            }
        }
    }
    policies
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrongAiGate {
    pub direct_matches: u32,
    pub wins: u32,
    pub losses: u32,
    pub draws: u32,
    /// Complete seed blocks, each containing both seat assignments.
    pub paired_seeds: u32,
    /// Seed blocks where `Ai` won more of the two seat assignments.
    pub paired_wins: u32,
    pub paired_losses: u32,
    pub paired_ties: u32,
    /// Exact one-sided sign-test tail over decisive paired-seed margins.
    pub one_sided_p: f64,
    pub city_delta: i64,
    pub damage_delta: i64,
    pub resources_delta: i64,
    pub army_value_delta: i64,
    pub passed: bool,
}

fn strong_ai_gate(reports: &[CaseReport]) -> StrongAiGate {
    let mut gate = StrongAiGate {
        direct_matches: 0,
        wins: 0,
        losses: 0,
        draws: 0,
        paired_seeds: 0,
        paired_wins: 0,
        paired_losses: 0,
        paired_ties: 0,
        one_sided_p: 1.0,
        city_delta: 0,
        damage_delta: 0,
        resources_delta: 0,
        army_value_delta: 0,
        passed: false,
    };
    let mut pair_margins = BTreeMap::<u32, (i32, u8)>::new();
    for report in reports.iter().filter(|report| {
        (report.case.seat0 == PolicyId::Ai && report.case.seat1 == PolicyId::Marshal)
            || (report.case.seat0 == PolicyId::Marshal && report.case.seat1 == PolicyId::Ai)
    }) {
        gate.direct_matches += 1;
        let ai_seat = usize::from(report.case.seat1 == PolicyId::Ai);
        let marshal_seat = 1 - ai_seat;
        match report.verdict {
            Some(PolicyId::Ai) => {
                gate.wins += 1;
                pair_margins.entry(report.case.seed).or_default().0 += 1;
            }
            Some(PolicyId::Marshal) => {
                gate.losses += 1;
                pair_margins.entry(report.case.seed).or_default().0 -= 1;
            }
            _ => gate.draws += 1,
        }
        let seat_bit = if report.case.seat0 == PolicyId::Ai {
            1
        } else {
            2
        };
        pair_margins.entry(report.case.seed).or_default().1 |= seat_bit;
        let ai = report.scores[ai_seat];
        let marshal = report.scores[marshal_seat];
        gate.city_delta += ai.cities - marshal.cities;
        gate.damage_delta += ai.damage_dealt - marshal.damage_dealt;
        gate.resources_delta += ai.resources - marshal.resources;
        gate.army_value_delta += ai.army_value - marshal.army_value;
    }
    for (margin, seat_mask) in pair_margins.into_values() {
        if seat_mask != 3 {
            continue;
        }
        gate.paired_seeds += 1;
        match margin.cmp(&0) {
            std::cmp::Ordering::Greater => gate.paired_wins += 1,
            std::cmp::Ordering::Less => gate.paired_losses += 1,
            std::cmp::Ordering::Equal => gate.paired_ties += 1,
        }
    }
    gate.one_sided_p = binomial_upper_tail(gate.paired_wins, gate.paired_wins + gate.paired_losses);
    gate.passed = gate.paired_seeds >= 16
        && gate.paired_wins > gate.paired_losses
        && gate.one_sided_p <= 0.05
        && gate.damage_delta > 0;
    gate
}

fn binomial_upper_tail(wins: u32, trials: u32) -> f64 {
    if trials == 0 {
        return 1.0;
    }
    let mut choose = 1.0f64;
    let mut tail = 0.0;
    for k in 0..=trials {
        if k >= wins {
            tail += choose;
        }
        if k < trials {
            choose *= f64::from(trials - k) / f64::from(k + 1);
        }
    }
    tail / 2f64.powi(trials as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn direct_report(seed: u32, ai_seat: usize, verdict: PolicyId) -> CaseReport {
        let (seat0, seat1) = if ai_seat == 0 {
            (PolicyId::Ai, PolicyId::Marshal)
        } else {
            (PolicyId::Marshal, PolicyId::Ai)
        };
        let mut scores = [super::super::world::Score::default(); 2];
        scores[ai_seat].damage_dealt = 1;
        CaseReport {
            case: EvalCase {
                ordinal: 0,
                seed,
                minutes: 20,
                seat0,
                seat1,
            },
            outcome: Outcome::Timeout,
            verdict: Some(verdict),
            verdict_reason: "test",
            frames: 1,
            result_checksum: 0,
            commands: [0; 2],
            accepted: [0; 2],
            refused: [0; 2],
            invalid: [0; 2],
            scores,
            events: Vec::new(),
        }
    }

    #[test]
    fn round_robin_is_balanced_and_stably_ordered() {
        let cases = round_robin_cases(7, 3);
        assert_eq!(cases.len(), 3 * 5 * 4);
        for policy in PolicyId::ALL {
            let seats = cases.iter().fold([0usize; 2], |mut count, case| {
                if case.seat0 == policy {
                    count[0] += 1;
                }
                if case.seat1 == policy {
                    count[1] += 1;
                }
                count
            });
            assert_eq!(seats, [12, 12], "{policy}");
        }
        assert!(cases.iter().enumerate().all(|(i, case)| case.ordinal == i));
    }

    #[test]
    fn strong_schedule_has_both_seats_for_every_seed() {
        let cases = strong_ai_cases(20, 8);
        assert_eq!(cases.len(), 16);
        for pair in cases.chunks_exact(2) {
            assert_eq!(pair[0].seed, pair[1].seed);
            assert_eq!(
                (pair[0].seat0, pair[0].seat1),
                (PolicyId::Ai, PolicyId::Marshal)
            );
            assert_eq!(
                (pair[1].seat0, pair[1].seat1),
                (PolicyId::Marshal, PolicyId::Ai)
            );
        }
    }

    #[test]
    fn strong_holdout_offset_is_disjoint_and_reproducible() {
        let development = strong_ai_cases_from(20, 0, 8);
        let holdout = strong_ai_cases_from(20, 8, 8);
        assert_eq!(holdout, strong_ai_cases_from(20, 8, 8));
        assert!(development
            .iter()
            .all(|case| holdout.iter().all(|other| case.seed != other.seed)));
    }

    #[test]
    fn marshal_head_adapter_is_result_exact() {
        let native = EvalCase {
            ordinal: 0,
            seed: 0x5EED_0001,
            minutes: 1,
            seat0: PolicyId::Marshal,
            seat1: PolicyId::CapFirst,
        };
        let heads = EvalCase {
            ordinal: 1,
            seat0: PolicyId::MarshalHeads,
            ..native
        };
        let native = run_case(native).expect("arena data");
        let heads = run_case(heads).expect("arena data");
        assert_eq!(native.result_checksum, heads.result_checksum);
        assert_eq!(native.outcome, heads.outcome);
        assert_eq!(native.scores, heads.scores);
    }

    #[test]
    fn exact_binomial_tail_has_known_small_images() {
        assert!((binomial_upper_tail(12, 16) - 0.038_406_372_070_312_5).abs() < 1e-15);
        assert!((binomial_upper_tail(11, 16) - 0.105_056_762_695_312_5).abs() < 1e-15);
        // The earlier 12-4 *match* result was only 5-1 over decisive paired seed
        // margins; treating both seats on one map as independent would overstate it.
        assert!((binomial_upper_tail(5, 6) - 0.109_375).abs() < 1e-15);
        assert_eq!(binomial_upper_tail(0, 0), 1.0);
    }

    #[test]
    fn strong_gate_counts_complete_seat_pairs_as_samples() {
        let mut reports = Vec::new();
        for seed_i in 0..16 {
            let verdict = if seed_i < 12 {
                PolicyId::Ai
            } else {
                PolicyId::Marshal
            };
            reports.push(direct_report(evaluation_seed(seed_i), 0, verdict));
            reports.push(direct_report(evaluation_seed(seed_i), 1, verdict));
        }
        // A lone match from a seventeenth map stays visible in match totals but is not an
        // independent paired-map sample.
        reports.push(direct_report(evaluation_seed(16), 0, PolicyId::Ai));
        let gate = strong_ai_gate(&reports);
        assert_eq!(gate.direct_matches, 33);
        assert_eq!(
            (gate.paired_seeds, gate.paired_wins, gate.paired_losses),
            (16, 12, 4)
        );
        assert!((gate.one_sided_p - 0.038_406_372_070_312_5).abs() < 1e-15);
        assert!(gate.passed);
    }
}
