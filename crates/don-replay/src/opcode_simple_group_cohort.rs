//! Source-only audit authority for the canonical simple-Group opcode cohort.
//!
//! This module is intentionally not exported from `don-replay::lib`.  It freezes the
//! executable/PDB boundaries and the package topology which a future canonical `Sim`
//! transaction must admit.  It does not promote a command-table row: the existing
//! `Port::Complete` labels describe the shadow `command::Bridge` receiver, not a packet
//! mounted on `tick::Sim`'s fixed Groups/World owners.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CohortRow {
    pub opcode: u8,
    pub command: &'static str,
    pub handler: &'static str,
    pub wire_len: Option<usize>,
    pub handler_va: u32,
    pub handler_size: u32,
    /// Instruction which directly calls the action, or the virtual `+0x14` call for BEGIN.
    pub action_call_va: u32,
    pub action: Option<&'static str>,
    pub action_va: Option<u32>,
    pub action_size: Option<u32>,
    /// Count in the 61-recording source-bound validation artifact.  A live corpus audit is
    /// deliberately separate because additional user recordings may exist locally.
    pub validation_count: u64,
}

pub const COHORT: [CohortRow; 10] = [
    CohortRow {
        opcode: 0,
        command: "GroupCommand",
        handler: "process_group",
        wire_len: None,
        handler_va: 0x0094_a0c0,
        handler_size: 1_600,
        action_call_va: 0,
        action: None,
        action_va: None,
        action_size: None,
        validation_count: 78_197,
    },
    CohortRow {
        opcode: 1,
        command: "BeginCommand",
        handler: "process_begin",
        wire_len: Some(1),
        handler_va: 0x0094_9fd0,
        handler_size: 229,
        action_call_va: 0x0094_a0a6,
        action: Some("Group::action_begin"),
        action_va: Some(0x0071_4100),
        action_size: Some(8),
        validation_count: 0,
    },
    CohortRow {
        opcode: 2,
        command: "StanceCommand",
        handler: "process_stance",
        wire_len: Some(5),
        handler_va: 0x0094_9ed0,
        handler_size: 247,
        action_call_va: 0x0094_9fb4,
        action: Some("Group::action_stance"),
        action_va: Some(0x0070_d440),
        action_size: Some(928),
        validation_count: 74,
    },
    CohortRow {
        opcode: 12,
        command: "HaltCommand",
        handler: "process_halt",
        wire_len: Some(1),
        handler_va: 0x0094_9140,
        handler_size: 231,
        action_call_va: 0x0094_9216,
        action: Some("Group::action_halt"),
        action_va: Some(0x0070_d0c0),
        action_size: Some(685),
        validation_count: 69,
    },
    CohortRow {
        opcode: 14,
        command: "SetTransportCommand",
        handler: "process_set_transport",
        wire_len: Some(5),
        handler_va: 0x0094_8f60,
        handler_size: 237,
        action_call_va: 0x0094_903c,
        action: Some("Group::action_set_transport"),
        action_va: Some(0x0070_24b0),
        action_size: Some(357),
        validation_count: 5,
    },
    CohortRow {
        opcode: 21,
        command: "DisbandCommand",
        handler: "process_disband",
        wire_len: Some(5),
        handler_va: 0x0094_8660,
        handler_size: 246,
        action_call_va: 0x0094_8743,
        action: Some("Group::action_disband"),
        action_va: Some(0x0070_e260),
        action_size: Some(693),
        validation_count: 10_916,
    },
    CohortRow {
        opcode: 29,
        command: "StopSpellCommand",
        handler: "process_stop_spell",
        wire_len: Some(1),
        handler_va: 0x0094_7cc0,
        handler_size: 229,
        action_call_va: 0x0094_7d94,
        action: Some("Group::action_stop_spell"),
        action_va: Some(0x006f_d7a0),
        action_size: Some(480),
        validation_count: 113,
    },
    CohortRow {
        opcode: 30,
        command: "FollowCommand",
        handler: "process_follow",
        wire_len: Some(13),
        handler_va: 0x0094_79c0,
        handler_size: 274,
        action_call_va: 0x0094_7abf,
        action: Some("Group::action_follow"),
        action_va: Some(0x006f_d510),
        action_size: Some(645),
        validation_count: 7,
    },
    CohortRow {
        opcode: 32,
        command: "UnitmaskCommand",
        handler: "process_unitmask",
        wire_len: Some(9),
        handler_va: 0x0094_7790,
        handler_size: 260,
        action_call_va: 0x0094_7881,
        action: Some("Group::action_unitmask"),
        action_va: Some(0x006f_cb90),
        action_size: Some(404),
        validation_count: 297,
    },
    CohortRow {
        opcode: 33,
        command: "BuildmaskCommand",
        handler: "process_buildmask",
        wire_len: Some(9),
        handler_va: 0x0094_7680,
        handler_size: 260,
        action_call_va: 0x0094_7771,
        action: Some("Group::action_buildmask"),
        action_va: Some(0x006f_c9a0),
        action_size: Some(487),
        validation_count: 320,
    },
];

pub const ACTION_OPCODES: [u8; 9] = [1, 2, 12, 14, 21, 29, 30, 32, 33];

pub fn row(opcode: u8) -> Option<&'static CohortRow> {
    COHORT.iter().find(|row| row.opcode == opcode)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageRelation {
    /// The action is immediately preceded by the Group command it consumes.
    AdjacentGroup,
    /// A Group command occurs earlier in this package, with admitted presentation/admin
    /// commands between it and the action.  The future package shell must preserve them.
    EarlierGroup,
    /// Retail may reuse the play-keyed receive cache after an empty or prior selection.
    /// This cannot be executed without the canonical `CommandPackageState[play]` owner.
    CachedGroup,
}

/// Classify one action position without normalising or deleting intervening commands.
pub fn classify_action(commands: &[u8], action_index: usize) -> Option<PackageRelation> {
    let &opcode = commands.get(action_index)?;
    if !ACTION_OPCODES.contains(&opcode) {
        return None;
    }
    if action_index > 0 && commands[action_index - 1] == 0 {
        return Some(PackageRelation::AdjacentGroup);
    }
    if commands[..action_index].contains(&0) {
        return Some(PackageRelation::EarlierGroup);
    }
    Some(PackageRelation::CachedGroup)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalSurface {
    /// Network-play to object-owner mapping used before opcode-0 selection is accepted.
    PlayerMap,
    /// Authoritative frame and RNG state; this cohort reads both and must leave RNG unchanged.
    GameClockRng,
    CommandPackageState,
    Groups,
    ObjectRegistry,
    UnitState,
    BuildState,
    /// The queue is physically nested in canonical `BuildData`; it is listed separately so
    /// DISBAND cannot be mounted while silently omitting its active-building queue arm.
    BuildProductionQueue,
    Orders,
    Paths,
    /// Read/revalidation surface for stance and transport-level flags.
    LeaderState,
    ScenarioIgnoreOrders,
    /// Revision/digest-bound type and capability predicates absent from generated World columns.
    ActionFactAuthority,
    PresentationReceipt,
}

/// Union of the canonical read, write, and receipt surfaces reached by this cohort. One
/// transaction must snapshot and revalidate the reached subset; none may be copied into a second
/// mutable authority. `BuildProductionQueue` is a named child of `BuildState`, not a second store.
pub const CANONICAL_SURFACES: [CanonicalSurface; 14] = [
    CanonicalSurface::PlayerMap,
    CanonicalSurface::GameClockRng,
    CanonicalSurface::CommandPackageState,
    CanonicalSurface::Groups,
    CanonicalSurface::ObjectRegistry,
    CanonicalSurface::UnitState,
    CanonicalSurface::BuildState,
    CanonicalSurface::BuildProductionQueue,
    CanonicalSurface::Orders,
    CanonicalSurface::Paths,
    CanonicalSurface::LeaderState,
    CanonicalSurface::ScenarioIgnoreOrders,
    CanonicalSurface::ActionFactAuthority,
    CanonicalSurface::PresentationReceipt,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegrationState {
    /// A strict receiver and recomputable transaction already exist, but only on the
    /// shadow Bridge/ObjectTable owner.
    ShadowReceiverComplete,
    /// Opcode 0 + Move is mounted; the reusable selection/package shell is canonical.
    CanonicalShellExists,
    /// No canonical Sim adapter yet captures/revalidates/commits every reached owner.
    CanonicalAdapterMissing,
}

pub const COHORT_INTEGRATION: [IntegrationState; 3] = [
    IntegrationState::ShadowReceiverComplete,
    IntegrationState::CanonicalShellExists,
    IntegrationState::CanonicalAdapterMissing,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifier_never_treats_a_non_cohort_opcode_as_an_action() {
        assert_eq!(classify_action(&[0, 7], 1), None);
        assert_eq!(
            classify_action(&[0, 2], 1),
            Some(PackageRelation::AdjacentGroup)
        );
        assert_eq!(
            classify_action(&[0, 72, 2], 2),
            Some(PackageRelation::EarlierGroup)
        );
        assert_eq!(classify_action(&[2], 0), Some(PackageRelation::CachedGroup));
    }

    #[test]
    fn row_boundaries_are_nonempty_and_unique() {
        let mut opcodes = COHORT.iter().map(|row| row.opcode).collect::<Vec<_>>();
        opcodes.sort_unstable();
        opcodes.dedup();
        assert_eq!(opcodes.len(), COHORT.len());
        for row in COHORT {
            assert_ne!(row.handler_size, 0);
            assert_eq!(row.action.is_some(), row.action_va.is_some());
            assert_eq!(row.action.is_some(), row.action_size.is_some());
        }
    }
}
