// SPDX-License-Identifier: GPL-3.0-or-later

//! Isolated retail reconstruction of tick step 17,
//! `Leaders::end_process_all` at `0x006ED070`.
//!
//! The deterministic stores are applied to [`Step17State`]. Reads owned by `Game`,
//! `Console`, and the parsed `poplimits` category are supplied through [`ProductFacts`].
//! The three reached presentation calls are not simulated: they are preserved, in retail
//! order, as typed [`HostTailReceipt`] values. Every receipt shares one sequence domain so
//! an adapter can prove that the frame stamp preceded the product-data read and that later
//! leaders ran after an earlier leader's presentation tail.

/// PDB/public symbol for `Leaders::end_process_all`.
pub const END_PROCESS_ALL_VA: u32 = 0x006e_d070;
/// PDB procedure size. The first following instruction is padding at `0x006ED295`.
pub const END_PROCESS_ALL_SIZE: u32 = 549;
/// `?leaders@@3VLeaders@@A`.
pub const LEADERS_BASE_VA: u32 = 0x00e3_a390;
/// PDB `sizeof(Leader)` and the increment at `0x006ED272`.
pub const LEADER_STRIDE: u32 = 0x6eec;
/// The dispatcher stops with its `Leader + 8` cursor at `0x00E71AF8`.
pub const RETAIL_LEADER_SLOTS: usize = 8;
/// PDB `LeaderData::leader_flags & 2` is the outer dispatcher gate.
pub const LEADER_PROCESS_FLAG: u32 = 0x0000_0002;
/// PDB `LeaderData::who`.
pub const LEADER_WHO_OFFSET: u32 = 0x08;
/// PDB `LeaderData::pop_cap`.
pub const LEADER_POP_CAP_OFFSET: u32 = 0x7e4;
/// PDB `LeaderData::pop_issues`.
pub const LEADER_POP_ISSUES_OFFSET: u32 = 0x7e8;

/// PDB `Game::info` is at `+0x0C`; `GameInfo::player` is at `+0x38`.
pub const GAME_PLAYER_ARRAY_OFFSET: u32 = 0x44;
/// PDB `sizeof(Player)` and the scale at `0x006ED1E9`.
pub const PLAYER_STRIDE: u32 = 0x8c;
pub const PLAYER_POP_CAP_FRAME_OFFSET: u32 = 0x2c;
pub const PLAYER_FLAGS_OFFSET: u32 = 0x30;
pub const PLAYER_WHO_OFFSET: u32 = 0x33;
pub const PLAYER_VALID_FLAG: u16 = 0x0001;
pub const PLAYER_POP_CAP_WARNING_FLAG: u16 = 0x0800;

pub const GAME_FRAME_OFFSET: u32 = 0x550;
/// `Game::info + GameInfo::pop_limit` = `Game + 0x31`.
pub const GAME_POP_LIMIT_INDEX_OFFSET: u32 = 0x31;
pub const CONSOLE_WHO_OFFSET: u32 = 0x298;
pub const CONSOLE_PLAY_OFFSET: u32 = 0x2a0;
/// `cmp elapsed, 0x1C2; jl skip`: 450 is the first due elapsed value.
pub const POP_WARNING_MIN_ELAPSED: i32 = 450;

/// `?pop_limits@@3VCategories@@A`.
pub const POP_LIMITS_VA: u32 = 0x00e8_0020;
/// PDB `ArrayBase<Category>::list` inside `Categories::list`.
pub const POP_LIMITS_LIST_OFFSET: u32 = 0x10;
/// PDB `sizeof(Category)`.
pub const CATEGORY_STRIDE: u32 = 0x58;
/// PDB `Category::data[0]`.
pub const CATEGORY_DATA_OFFSET: u32 = 0x3c;
/// The six rows in the shipped `rules.xml` `poplimits` category.
pub const SHIPPED_POP_LIMITS: [i32; 6] = [50, 75, 100, 125, 150, 200];

pub const STRING_COPY_CTOR_VA: u32 = 0x00a1_d590;
pub const MESSAGE_WIN_ADD_FEEDBACK_VA: u32 = 0x007e_9ab0;
pub const SOUND_GLOBAL_PLAY_VA: u32 = 0x0097_f770;
pub const RED_COLOR_VA: u32 = 0x00c8_d248;
/// Offset added to the product's localized-text base before the `String` copy.
pub const POP_WARNING_TEXT_OFFSET: u32 = 0xc878;
pub const POP_WARNING_SOUND_CATEGORY: i32 = 0x5b;

#[inline]
pub const fn leader_va(index: usize) -> u32 {
    LEADERS_BASE_VA + index as u32 * LEADER_STRIDE
}

/// The first `Player` begins at `Game + 0x44`; the executable's static Game is
/// `0x00E37EC0`, making Player zero `0x00E37F04`.
#[inline]
pub const fn static_player_va(index: usize) -> u32 {
    0x00e3_7ec0 + GAME_PLAYER_ARRAY_OFFSET + index as u32 * PLAYER_STRIDE
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LeaderSlot {
    pub flags: u32,
    pub who: i32,
    pub pop_cap: i32,
    pub pop_issues: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlayerRecord {
    pub pop_cap_frame: i32,
    pub flags: u16,
    pub who: u8,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Step17State {
    pub leaders: [LeaderSlot; RETAIL_LEADER_SLOTS],
    pub players: [PlayerRecord; RETAIL_LEADER_SLOTS],
}

/// Product-owned reads used by the inlined body.
///
/// `population_limits` is the current `Category::data[0]` sequence, not an invented
/// fallback. A missing selected row becomes a typed residual after the retail-order frame
/// stamp. Pass [`SHIPPED_POP_LIMITS`] for the unmodified product data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProductFacts<'a> {
    pub frame: i32,
    pub console_who: i32,
    pub console_play: i32,
    pub pop_limit_index: u8,
    pub population_limits: &'a [i32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaderOutcome {
    NotVisited,
    ProcessFlagClear,
    WarningScan { stores: u8 },
    NonLocalPopulationIssue,
    LocalPlayerUnavailable,
    FeedbackRateLimited { elapsed: i32 },
    PopulationLimitUnavailable,
    AtOrAbovePopulationLimit { limit: i32 },
    PresentationTailReached { limit: i32 },
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
    /// Retail performs the store even if warning bit `0x800` was already clear.
    PlayerFlags {
        leader_index: usize,
        player_index: usize,
        before: u16,
        after: u16,
    },
    PlayerPopCapFrame {
        leader_index: usize,
        player_index: usize,
        before: i32,
        after: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalMutationReceipt {
    pub sequence: u16,
    pub mutation: LocalMutation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProductReadReceipt {
    pub sequence: u16,
    pub leader_index: usize,
    pub category_index: u8,
    pub value: i32,
}

/// The product/UI/audio work reached only after a below-limit comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostTail {
    CopyLocalizedString {
        constructor_va: u32,
        text_offset: u32,
    },
    MessageWinAddFeedback {
        call_va: u32,
        duration: i32,
        color_va: u32,
    },
    SoundGlobalPlay {
        call_va: u32,
        category: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostTailReceipt {
    pub sequence: u16,
    pub leader_index: usize,
    pub host_tail: HostTail,
}

/// Guards needed by a headless adapter for invariants retail assumes by construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenResidual {
    ConsolePlayOutsidePlayerArray { play: i32 },
    PopulationLimitRowUnavailable { index: u8, supplied_rows: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResidualReceipt {
    pub sequence: u16,
    pub leader_index: usize,
    pub residual: OpenResidual,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step17Trace {
    /// Always eight entries in exact retail address order, including gated slots.
    pub visits: [LeaderVisit; RETAIL_LEADER_SLOTS],
    pub local_mutations: Vec<LocalMutationReceipt>,
    pub product_reads: Vec<ProductReadReceipt>,
    pub host_tails: Vec<HostTailReceipt>,
    pub residuals: Vec<ResidualReceipt>,
    pub total_sequenced_effects: u16,
}

impl Default for Step17Trace {
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

impl Step17Trace {
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

    fn push_product_read(&mut self, leader_index: usize, category_index: u8, value: i32) {
        let sequence = self.take_sequence();
        self.product_reads.push(ProductReadReceipt {
            sequence,
            leader_index,
            category_index,
            value,
        });
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

/// Execute the full deterministic portion of the 549-byte scheduled body.
///
/// Retail's zero-issue branch is unrolled for eight `Player` records. The loop here has the
/// same cardinality and order. The population-warning branch uses `wrapping_sub` followed
/// by a signed comparison, reproducing x86 `sub; cmp 0x1C2; jl`. A due Player is stamped
/// before the category row is read or compared.
pub fn execute_step17(state: &mut Step17State, product: ProductFacts<'_>) -> Step17Trace {
    let mut trace = Step17Trace::default();

    for leader_index in 0..RETAIL_LEADER_SLOTS {
        let leader = state.leaders[leader_index];
        if leader.flags & LEADER_PROCESS_FLAG == 0 {
            trace.visits[leader_index].outcome = LeaderOutcome::ProcessFlagClear;
            continue;
        }

        if leader.pop_issues == 0 {
            let mut stores = 0u8;
            for player_index in 0..RETAIL_LEADER_SLOTS {
                let player = &mut state.players[player_index];
                if player.flags & PLAYER_VALID_FLAG == 0 || i32::from(player.who) != leader.who {
                    continue;
                }
                let before = player.flags;
                let after = before & !PLAYER_POP_CAP_WARNING_FLAG;
                player.flags = after;
                trace.push_local(LocalMutation::PlayerFlags {
                    leader_index,
                    player_index,
                    before,
                    after,
                });
                stores += 1;
            }
            trace.visits[leader_index].outcome = LeaderOutcome::WarningScan { stores };
            continue;
        }

        if leader.who != product.console_who {
            trace.visits[leader_index].outcome = LeaderOutcome::NonLocalPopulationIssue;
            continue;
        }

        let Ok(player_index) = usize::try_from(product.console_play) else {
            trace.push_residual(
                leader_index,
                OpenResidual::ConsolePlayOutsidePlayerArray {
                    play: product.console_play,
                },
            );
            trace.visits[leader_index].outcome = LeaderOutcome::LocalPlayerUnavailable;
            continue;
        };
        if player_index >= RETAIL_LEADER_SLOTS {
            trace.push_residual(
                leader_index,
                OpenResidual::ConsolePlayOutsidePlayerArray {
                    play: product.console_play,
                },
            );
            trace.visits[leader_index].outcome = LeaderOutcome::LocalPlayerUnavailable;
            continue;
        }

        let previous_frame = state.players[player_index].pop_cap_frame;
        let elapsed = product.frame.wrapping_sub(previous_frame);
        if elapsed < POP_WARNING_MIN_ELAPSED {
            trace.visits[leader_index].outcome = LeaderOutcome::FeedbackRateLimited { elapsed };
            continue;
        }

        state.players[player_index].pop_cap_frame = product.frame;
        trace.push_local(LocalMutation::PlayerPopCapFrame {
            leader_index,
            player_index,
            before: previous_frame,
            after: product.frame,
        });

        let category_index = product.pop_limit_index;
        let Some(&limit) = product.population_limits.get(usize::from(category_index)) else {
            trace.push_residual(
                leader_index,
                OpenResidual::PopulationLimitRowUnavailable {
                    index: category_index,
                    supplied_rows: product.population_limits.len(),
                },
            );
            trace.visits[leader_index].outcome = LeaderOutcome::PopulationLimitUnavailable;
            continue;
        };
        trace.push_product_read(leader_index, category_index, limit);

        if leader.pop_cap >= limit {
            trace.visits[leader_index].outcome = LeaderOutcome::AtOrAbovePopulationLimit { limit };
            continue;
        }

        trace.push_host_tail(
            leader_index,
            HostTail::CopyLocalizedString {
                constructor_va: STRING_COPY_CTOR_VA,
                text_offset: POP_WARNING_TEXT_OFFSET,
            },
        );
        trace.push_host_tail(
            leader_index,
            HostTail::MessageWinAddFeedback {
                call_va: MESSAGE_WIN_ADD_FEEDBACK_VA,
                duration: -1,
                color_va: RED_COLOR_VA,
            },
        );
        trace.push_host_tail(
            leader_index,
            HostTail::SoundGlobalPlay {
                call_va: SOUND_GLOBAL_PLAY_VA,
                category: POP_WARNING_SOUND_CATEGORY,
            },
        );
        trace.visits[leader_index].outcome = LeaderOutcome::PresentationTailReached { limit };
    }

    trace
}
