// SPDX-License-Identifier: GPL-3.0-or-later

//! Isolated retail reconstruction of tick step 19,
//! `Leader::process_event_frame` at `0x006EC180`.
//!
//! The scheduled dispatcher and all deterministic queue/rate mutations execute here.
//! Product-owned team-score reads, the encrypted age word, JukeBox, and Achieve remain
//! explicit typed receipts. Every receipt shares one sequence domain, preserving the
//! retail ordering between queue folds, product reads, presentation calls, sentinel
//! stores, and the final current-event clear.

/// PDB/public symbol for `Leader::process_event_frame`.
pub const PROCESS_EVENT_FRAME_VA: u32 = 0x006e_c180;
/// PDB procedure size. The byte after the final `ret` is `0x006EC516`.
pub const PROCESS_EVENT_FRAME_SIZE: u32 = 918;
/// `Game::do_frame` step-19 dispatcher prefix and call site.
pub const DISPATCHER_VA: u32 = 0x0059_24a0;
pub const DISPATCH_CALL_VA: u32 = 0x0059_24ac;

/// `?leaders@@3VLeaders@@A`.
pub const LEADERS_BASE_VA: u32 = 0x00e3_a390;
/// PDB `sizeof(Leader)` and the increments at `0x005924B1` / `0x006EC385`.
pub const LEADER_STRIDE: u32 = 0x6eec;
/// The dispatcher compares its cursor with this one-past-end address.
pub const LEADERS_END_VA: u32 = 0x00e7_1af0;
pub const RETAIL_LEADER_SLOTS: usize = 8;
/// `LeaderData::leader_flags & 1`, tested as a byte by both relevant loops.
pub const LEADER_IN_GAME_FLAG: u32 = 0x0000_0001;

pub const LEADER_WHO_OFFSET: u32 = 0x08;
pub const LEADER_DIPLOS_OFFSET: u32 = 0x74;
pub const LEADER_FRAME_BATTLE_OFFSET: u32 = 0x0a4c;
pub const LEADER_AVERAGE_DEATH_RATE_OFFSET: u32 = 0x0a50;
pub const LEADER_AVERAGE_KILL_RATE_OFFSET: u32 = 0x0a52;
pub const LEADER_AVERAGE_DAMAGE_RATE_OFFSET: u32 = 0x0a54;
pub const LEADER_AVERAGE_HIT_RATE_OFFSET: u32 = 0x0a56;
pub const LEADER_CURRENT_EVENTS_OFFSET: u32 = 0x0a58;
pub const LEADER_FIFTEEN_SECOND_EVENTS_OFFSET: u32 = 0x0a60;
pub const LEADER_DATA_ENCRYPTED_OFFSET: u32 = 0x6eb8;
pub const ENCRYPTED_AGES_OFFSET: u32 = 0x00dc;
pub const AGES_XOR_KEY: u32 = 0x0006_2766;

pub const GAME_FRAME_OFFSET: u32 = 0x550;
pub const CONSOLE_WHO_OFFSET: u32 = 0x298;
pub const CURRENT_MUSIC_MOOD_VA: u32 = 0x00ec_ba20;
pub const NEXT_MUSIC_MOOD_VA: u32 = 0x00ec_ba2c;
pub const EMPTY_STRING_VA: u32 = 0x00eb_437c;

pub const GET_TEAM_SCORE_VA: u32 = 0x006d_6520;
pub const JUKEBOX_SET_NEXT_MOOD_VA: u32 = 0x0097_d5d0;
pub const ACHIEVE_ADD_EVENT_VA: u32 = 0x007a_f660;

pub const EVENT_RATE_PERIOD: i32 = 50;
pub const EVENT_RATE_SCALE: u16 = 100;
pub const QUIET_MOOD_THRESHOLD: u32 = 300;
pub const ACTIVE_MOOD_THRESHOLD: u32 = 600;
pub const FORCE_MOOD_THRESHOLD: u32 = 2_000;
pub const TEAM_SCORE_BIAS: i32 = 200;
pub const BATTLE_COOLDOWN: i32 = 1_800;
pub const BATTLE_RATE_PER_AGE: i32 = 125;
pub const BATTLE_IMBALANCE_PER_AGE: i32 = 10;
pub const BATTLE_IMBALANCE_BASE: i32 = 20;
pub const BATTLE_RATE_SENTINEL: u16 = 0xfc18;

pub mod mood {
    pub const WINNING: i32 = 0;
    pub const LOSING: i32 = 1;
    pub const QUIET: i32 = 2;
}

#[inline]
pub const fn leader_va(index: usize) -> u32 {
    LEADERS_BASE_VA + index as u32 * LEADER_STRIDE
}

/// The PDB-named `LeaderData +0xA4C..+0xA66` block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EventQueueState {
    pub frame_battle: i32,
    pub average_death_rate: u16,
    pub average_kill_rate: u16,
    pub average_damage_rate: u16,
    pub average_hit_rate: u16,
    pub deaths_current_frame: u16,
    pub kills_current_frame: u16,
    pub hits_current_frame: u16,
    pub damage_current_frame: u16,
    pub deaths_fifteen_seconds: u16,
    pub kills_fifteen_seconds: u16,
    pub hits_fifteen_seconds: u16,
    pub damage_fifteen_seconds: u16,
}

impl EventQueueState {
    pub fn current_events(&self) -> [u16; 4] {
        [
            self.deaths_current_frame,
            self.kills_current_frame,
            self.hits_current_frame,
            self.damage_current_frame,
        ]
    }

    pub fn fifteen_second_events(&self) -> [u16; 4] {
        [
            self.deaths_fifteen_seconds,
            self.kills_fifteen_seconds,
            self.hits_fifteen_seconds,
            self.damage_fifteen_seconds,
        ]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LeaderSlot {
    pub flags: u32,
    pub who: i32,
    pub diplos: [i32; RETAIL_LEADER_SLOTS],
    pub event_queue: EventQueueState,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MusicState {
    /// Product global at [`CURRENT_MUSIC_MOOD_VA`]. The body only reads it.
    pub current_mood: i32,
    /// Product global at [`NEXT_MUSIC_MOOD_VA`]. The body writes it before JukeBox.
    pub next_mood: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Step19State {
    pub leaders: [LeaderSlot; RETAIL_LEADER_SLOTS],
    pub music: MusicState,
}

/// Product reads which the recovered body reaches but this isolated owner cannot derive.
/// Scores are keyed by Leader record, while encrypted ages are keyed by `LeaderData::who`,
/// matching the two distinct addressing expressions in the retail instructions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProductFacts {
    pub frame: i32,
    pub console_who: i32,
    pub team_scores: [Option<i32>; RETAIL_LEADER_SLOTS],
    pub encrypted_ages: [Option<u32>; RETAIL_LEADER_SLOTS],
}

impl Default for ProductFacts {
    fn default() -> Self {
        Self {
            frame: 0,
            console_who: -1,
            team_scores: [None; RETAIL_LEADER_SLOTS],
            encrypted_ages: [None; RETAIL_LEADER_SLOTS],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BattleEventKind {
    KillsOverDeaths = 0,
    DeathsOverKills = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaderOutcome {
    NotVisited,
    InGameFlagClear,
    FrameNotDue,
    Due {
        requested_mood: Option<i32>,
        battle_event: Option<BattleEventKind>,
        residuals: u8,
    },
}

impl Default for LeaderOutcome {
    fn default() -> Self {
        Self::NotVisited
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LeaderVisit {
    pub leader_index: usize,
    pub leader_va: u32,
    pub outcome: LeaderOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalMutation {
    /// Includes raw-current wrapping accumulation, the temporary `current*100` stores,
    /// and all four average writes. The final current-event clear is a later mutation.
    FoldEventQueue {
        leader_index: usize,
        before: EventQueueState,
        after: EventQueueState,
    },
    NextMusicMood {
        leader_index: usize,
        before: i32,
        after: i32,
    },
    BattleRateSentinels {
        leader_index: usize,
        before: [u16; 2],
        after: [u16; 2],
    },
    BattleFrameStamp {
        leader_index: usize,
        before: i32,
        after: i32,
    },
    ClearCurrentEvents {
        leader_index: usize,
        before: [u16; 4],
        after: [u16; 4],
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalMutationReceipt {
    pub sequence: u16,
    pub mutation: LocalMutation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProductRead {
    TeamScore {
        call_va: u32,
        owner_leader_index: usize,
        queried_leader_index: usize,
        value: i32,
    },
    EncryptedAge {
        owner_leader_index: usize,
        who_index: usize,
        encrypted: u32,
        xor_key: u32,
        decoded: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProductReadReceipt {
    pub sequence: u16,
    pub read: ProductRead,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostTail {
    /// Retail stores `requested_mood` in the global first; the call itself receives only
    /// the boolean in `ECX` and owns wall-clock/audio behavior.
    JukeBoxSetNextMood {
        call_va: u32,
        requested_mood: i32,
        force: bool,
    },
    AchieveAddEvent {
        call_va: u32,
        kind: BattleEventKind,
        who: i32,
        empty_string_va: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostTailReceipt {
    pub sequence: u16,
    pub leader_index: usize,
    pub host_tail: HostTail,
}

/// Retail assumes these invariants by construction. A headless adapter must provide the
/// missing fact rather than letting an out-of-range memory read or fabricated zero decide
/// simulation state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenResidual {
    LeaderWhoOutsideArray { who: i32 },
    OtherWhoOutsideArray { other_leader_index: usize, who: i32 },
    TeamScoreUnavailable { queried_leader_index: usize },
    EncryptedAgeUnavailable { who_index: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResidualReceipt {
    pub sequence: u16,
    pub leader_index: usize,
    pub residual: OpenResidual,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step19Trace {
    /// Always eight entries in exact retail address order, including gated slots.
    pub visits: [LeaderVisit; RETAIL_LEADER_SLOTS],
    pub local_mutations: Vec<LocalMutationReceipt>,
    pub product_reads: Vec<ProductReadReceipt>,
    pub host_tails: Vec<HostTailReceipt>,
    pub residuals: Vec<ResidualReceipt>,
    pub total_sequenced_effects: u16,
}

impl Default for Step19Trace {
    fn default() -> Self {
        Self {
            visits: std::array::from_fn(|leader_index| LeaderVisit {
                leader_index,
                leader_va: leader_va(leader_index),
                outcome: LeaderOutcome::NotVisited,
            }),
            local_mutations: Vec::new(),
            product_reads: Vec::new(),
            host_tails: Vec::new(),
            residuals: Vec::new(),
            total_sequenced_effects: 0,
        }
    }
}

impl Step19Trace {
    #[inline]
    fn take_sequence(&mut self) -> u16 {
        let sequence = self.total_sequenced_effects;
        self.total_sequenced_effects += 1;
        sequence
    }

    fn push_local(&mut self, mutation: LocalMutation) {
        let sequence = self.take_sequence();
        self.local_mutations
            .push(LocalMutationReceipt { sequence, mutation });
    }

    fn push_product_read(&mut self, read: ProductRead) {
        let sequence = self.take_sequence();
        self.product_reads
            .push(ProductReadReceipt { sequence, read });
    }

    fn push_host_tail(&mut self, leader_index: usize, host_tail: HostTail) {
        let sequence = self.take_sequence();
        self.host_tails.push(HostTailReceipt {
            sequence,
            leader_index,
            host_tail,
        });
    }

    fn push_residual(&mut self, leader_index: usize, residual: OpenResidual) {
        let sequence = self.take_sequence();
        self.residuals.push(ResidualReceipt {
            sequence,
            leader_index,
            residual,
        });
    }
}

#[inline]
fn fold_rate(current: u16, average: u16) -> (u16, u16) {
    // Retail's `imul reg,reg,100` is followed by a word store and a word zero-test.
    let scaled = current.wrapping_mul(EVENT_RATE_SCALE);
    let next_average = if scaled == 0 {
        ((u32::from(average) * 7) >> 3) as u16
    } else {
        ((u32::from(scaled) + u32::from(average)) >> 1) as u16
    };
    (scaled, next_average)
}

fn fold_event_queue(queue: &mut EventQueueState) {
    // The four totals receive raw event counts before the current slots are scaled.
    queue.deaths_fifteen_seconds = queue
        .deaths_fifteen_seconds
        .wrapping_add(queue.deaths_current_frame);
    queue.damage_fifteen_seconds = queue
        .damage_fifteen_seconds
        .wrapping_add(queue.damage_current_frame);
    queue.kills_fifteen_seconds = queue
        .kills_fifteen_seconds
        .wrapping_add(queue.kills_current_frame);
    queue.hits_fifteen_seconds = queue
        .hits_fifteen_seconds
        .wrapping_add(queue.hits_current_frame);

    (queue.deaths_current_frame, queue.average_death_rate) =
        fold_rate(queue.deaths_current_frame, queue.average_death_rate);
    (queue.kills_current_frame, queue.average_kill_rate) =
        fold_rate(queue.kills_current_frame, queue.average_kill_rate);
    (queue.hits_current_frame, queue.average_hit_rate) =
        fold_rate(queue.hits_current_frame, queue.average_hit_rate);
    (queue.damage_current_frame, queue.average_damage_rate) =
        fold_rate(queue.damage_current_frame, queue.average_damage_rate);
}

fn read_team_score(
    trace: &mut Step19Trace,
    product: ProductFacts,
    owner_leader_index: usize,
    queried_leader_index: usize,
) -> Option<i32> {
    match product.team_scores[queried_leader_index] {
        Some(value) => {
            trace.push_product_read(ProductRead::TeamScore {
                call_va: GET_TEAM_SCORE_VA,
                owner_leader_index,
                queried_leader_index,
                value,
            });
            Some(value)
        }
        None => {
            trace.push_residual(
                owner_leader_index,
                OpenResidual::TeamScoreUnavailable {
                    queried_leader_index,
                },
            );
            None
        }
    }
}

fn resolve_active_mood(
    state: &Step19State,
    trace: &mut Step19Trace,
    product: ProductFacts,
    leader_index: usize,
    combat_sum: u32,
) -> Option<(i32, bool)> {
    // `get_team_score(this)` is called before retail enters the eight-record enemy scan.
    let own_score = read_team_score(trace, product, leader_index, leader_index);
    let local_who = state.leaders[leader_index].who;
    let Ok(local_who_index) = usize::try_from(local_who) else {
        trace.push_residual(
            leader_index,
            OpenResidual::LeaderWhoOutsideArray { who: local_who },
        );
        return None;
    };
    if local_who_index >= RETAIL_LEADER_SLOTS {
        trace.push_residual(
            leader_index,
            OpenResidual::LeaderWhoOutsideArray { who: local_who },
        );
        return None;
    }

    let mut strongest_hostile_score = 0i32;
    let mut complete = own_score.is_some();
    for other_leader_index in 0..RETAIL_LEADER_SLOTS {
        let other = state.leaders[other_leader_index];
        if other.flags & LEADER_IN_GAME_FLAG == 0 || other.who == local_who {
            continue;
        }

        // Retail short-circuits: `other.diplos[local_who] == 0` is sufficient. Only a
        // non-zero first relation indexes `leaders[local_who].diplos[other.who]`.
        let hostile = if other.diplos[local_who_index] == 0 {
            true
        } else {
            let Ok(other_who_index) = usize::try_from(other.who) else {
                trace.push_residual(
                    leader_index,
                    OpenResidual::OtherWhoOutsideArray {
                        other_leader_index,
                        who: other.who,
                    },
                );
                complete = false;
                continue;
            };
            if other_who_index >= RETAIL_LEADER_SLOTS {
                trace.push_residual(
                    leader_index,
                    OpenResidual::OtherWhoOutsideArray {
                        other_leader_index,
                        who: other.who,
                    },
                );
                complete = false;
                continue;
            }
            state.leaders[local_who_index].diplos[other_who_index] == 0
        };
        if !hostile
            || (other.event_queue.average_hit_rate == 0
                && other.event_queue.average_damage_rate == 0)
        {
            continue;
        }

        if let Some(score) = read_team_score(trace, product, leader_index, other_leader_index) {
            strongest_hostile_score = strongest_hostile_score.max(score);
        } else {
            complete = false;
        }
    }

    if !complete {
        return None;
    }
    let own_score = own_score.expect("complete score read");
    let event = state.leaders[leader_index].event_queue;
    let mut balance =
        i32::from(event.average_hit_rate).wrapping_sub(i32::from(event.average_damage_rate));

    // These are signed truncating divisions in the retail strength-reduced sequences.
    let hostile_four_thirds = strongest_hostile_score.wrapping_mul(4) / 3;
    if own_score >= hostile_four_thirds {
        balance = balance.wrapping_add(TEAM_SCORE_BIAS);
    } else {
        let hostile_three_quarters = strongest_hostile_score.wrapping_mul(3) / 4;
        if own_score <= hostile_three_quarters {
            balance = balance.wrapping_sub(TEAM_SCORE_BIAS);
        }
    }

    let requested_mood = if balance < 0 {
        mood::LOSING
    } else {
        mood::WINNING
    };
    let force = state.music.current_mood == mood::QUIET && combat_sum >= FORCE_MOOD_THRESHOLD;
    Some((requested_mood, force))
}

fn read_age(
    trace: &mut Step19Trace,
    product: ProductFacts,
    leader_index: usize,
    who: i32,
) -> Option<i32> {
    let Ok(who_index) = usize::try_from(who) else {
        trace.push_residual(leader_index, OpenResidual::LeaderWhoOutsideArray { who });
        return None;
    };
    if who_index >= RETAIL_LEADER_SLOTS {
        trace.push_residual(leader_index, OpenResidual::LeaderWhoOutsideArray { who });
        return None;
    }

    match product.encrypted_ages[who_index] {
        Some(encrypted) => {
            let decoded = (encrypted ^ AGES_XOR_KEY) as i32;
            trace.push_product_read(ProductRead::EncryptedAge {
                owner_leader_index: leader_index,
                who_index,
                encrypted,
                xor_key: AGES_XOR_KEY,
                decoded,
            });
            Some(decoded)
        }
        None => {
            trace.push_residual(
                leader_index,
                OpenResidual::EncryptedAgeUnavailable { who_index },
            );
            None
        }
    }
}

fn battle_event_kind(queue: EventQueueState, frame: i32, age: i32) -> Option<BattleEventKind> {
    let combined =
        i32::from(queue.average_death_rate).wrapping_add(i32::from(queue.average_kill_rate));
    let minimum = age.wrapping_add(1).wrapping_mul(BATTLE_RATE_PER_AGE);
    if combined < minimum
        || (queue.frame_battle != 0 && frame.wrapping_sub(queue.frame_battle) < BATTLE_COOLDOWN)
    {
        return None;
    }

    let imbalance = age
        .wrapping_mul(BATTLE_IMBALANCE_PER_AGE)
        .wrapping_add(BATTLE_IMBALANCE_BASE);
    let deaths = i32::from(queue.average_death_rate);
    let kills = i32::from(queue.average_kill_rate);
    if deaths >= kills.wrapping_add(imbalance) {
        Some(BattleEventKind::DeathsOverKills)
    } else if kills >= deaths.wrapping_add(imbalance) {
        Some(BattleEventKind::KillsOverDeaths)
    } else {
        None
    }
}

/// Execute the complete deterministic step-19 dispatcher and 918-byte body.
///
/// The dispatcher is deliberately sequential. A local Leader's combat scan sees freshly
/// folded rates for earlier array records and prior rates for later records. On a due
/// Leader, current counters are raw event counts until the fold, temporarily become their
/// low-word `*100` values, then are cleared only after music and achievement work.
pub fn execute_step19(state: &mut Step19State, product: ProductFacts) -> Step19Trace {
    let mut trace = Step19Trace::default();

    for leader_index in 0..RETAIL_LEADER_SLOTS {
        if state.leaders[leader_index].flags & LEADER_IN_GAME_FLAG == 0 {
            trace.visits[leader_index].outcome = LeaderOutcome::InGameFlagClear;
            continue;
        }

        // `cdq; idiv 50; test edx,edx`: signed remainder, before any Leader write.
        if product.frame % EVENT_RATE_PERIOD != 0 {
            trace.visits[leader_index].outcome = LeaderOutcome::FrameNotDue;
            continue;
        }

        let residuals_before = trace.residuals.len();
        let before_fold = state.leaders[leader_index].event_queue;
        fold_event_queue(&mut state.leaders[leader_index].event_queue);
        let after_fold = state.leaders[leader_index].event_queue;
        trace.push_local(LocalMutation::FoldEventQueue {
            leader_index,
            before: before_fold,
            after: after_fold,
        });

        let who = state.leaders[leader_index].who;
        let event = state.leaders[leader_index].event_queue;
        let combat_sum = u32::from(event.average_hit_rate) + u32::from(event.average_damage_rate);
        let mut requested_mood = None;
        if who == product.console_who {
            let mood_and_force = if combat_sum < QUIET_MOOD_THRESHOLD {
                Some((
                    mood::QUIET,
                    event.average_hit_rate == 0 && event.average_damage_rate == 0,
                ))
            } else if combat_sum < ACTIVE_MOOD_THRESHOLD {
                None
            } else {
                resolve_active_mood(state, &mut trace, product, leader_index, combat_sum)
            };

            if let Some((next_mood, force)) = mood_and_force {
                requested_mood = Some(next_mood);
                let before = state.music.next_mood;
                state.music.next_mood = next_mood;
                trace.push_local(LocalMutation::NextMusicMood {
                    leader_index,
                    before,
                    after: next_mood,
                });
                if state.music.current_mood != next_mood {
                    trace.push_host_tail(
                        leader_index,
                        HostTail::JukeBoxSetNextMood {
                            call_va: JUKEBOX_SET_NEXT_MOOD_VA,
                            requested_mood: next_mood,
                            force,
                        },
                    );
                }
            }
        }

        // The encrypted age word is read for every due Leader, before the combined-rate
        // threshold. It is addressed by `who`, not by the current dispatcher record.
        let age = read_age(&mut trace, product, leader_index, who);
        let battle_event = age.and_then(|age| {
            battle_event_kind(state.leaders[leader_index].event_queue, product.frame, age)
        });
        if let Some(kind) = battle_event {
            trace.push_host_tail(
                leader_index,
                HostTail::AchieveAddEvent {
                    call_va: ACHIEVE_ADD_EVENT_VA,
                    kind,
                    who,
                    empty_string_va: EMPTY_STRING_VA,
                },
            );

            let queue = &mut state.leaders[leader_index].event_queue;
            let before = [queue.average_death_rate, queue.average_kill_rate];
            queue.average_death_rate = BATTLE_RATE_SENTINEL;
            queue.average_kill_rate = BATTLE_RATE_SENTINEL;
            trace.push_local(LocalMutation::BattleRateSentinels {
                leader_index,
                before,
                after: [BATTLE_RATE_SENTINEL; 2],
            });

            let before = queue.frame_battle;
            queue.frame_battle = product.frame;
            trace.push_local(LocalMutation::BattleFrameStamp {
                leader_index,
                before,
                after: product.frame,
            });
        }

        // `mov [edi+A58],eax; mov [edi+A5C],eax` clears all four adjacent words.
        let queue = &mut state.leaders[leader_index].event_queue;
        let before = queue.current_events();
        queue.deaths_current_frame = 0;
        queue.kills_current_frame = 0;
        queue.hits_current_frame = 0;
        queue.damage_current_frame = 0;
        trace.push_local(LocalMutation::ClearCurrentEvents {
            leader_index,
            before,
            after: [0; 4],
        });

        trace.visits[leader_index].outcome = LeaderOutcome::Due {
            requested_mood,
            battle_event,
            residuals: (trace.residuals.len() - residuals_before) as u8,
        };
    }

    trace
}
