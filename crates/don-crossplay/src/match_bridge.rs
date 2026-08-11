// SPDX-License-Identifier: GPL-3.0-or-later
//! Crossplay lobby → `don-net` MatchStart admission.
//!
//! The two underlying state machines deliberately remain independent:
//! [`crate::Backend`] owns Create/Find/Join/StartGame, while
//! [`don_net::LocalMatch`] owns the recovered roster/readiness packets and the
//! DoN-owned authoritative MatchStart extension. This adapter is the small
//! fail-closed seam between them. It does not add a Crossplay ABI slot or claim
//! a retail wire format.
//!
//! A lobby observation is admissible only when its numeric member identities
//! exactly equal the live `don-net` roster and its owner equals the one
//! transport player marked as host. Once StartGame publishes a session
//! reference, the adapter derives a stable, non-zero DoN attempt epoch from
//! `(lobby id, session reference)` and pairs it with the lobby's `game_seed`.
//! The host may then publish exactly that MatchStart. Clients may receive the
//! transport packet before their next GetLobby completes, but turns remain
//! closed until the directory record and transport announcement agree.

use std::collections::BTreeSet;
use std::fmt;
use std::string::{String, ToString};
use std::time::Duration;
use std::vec::Vec;

use don_net::{
    AnnouncedMatchStart, LocalMatch, LocalMatchError, LocalMatchPhase, Player, Role, Transport,
    TurnPackage,
};

use crate::Lobby;

/// Recovered lobby key that carries the simulation seed.
pub const GAME_SEED_ATTRIBUTE: &str = "game_seed";

/// DoN-only lobby attribute used by the local executable path to discover the
/// `don-net` TCP listener. It is not a recovered retail key.
pub const DON_MATCH_ENDPOINT_ATTRIBUTE: &str = "don_match_endpoint";

/// The directory-backed identity of one accepted match-start transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceStart {
    pub lobby_id: String,
    pub session_reference: String,
    pub host_id: i32,
    pub epoch: u32,
    pub seed: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceMatchPhase {
    Lobby,
    AllReady,
    /// A transport MatchStart arrived before GetLobby exposed StartGame.
    TransportStartedUnconfirmed(AnnouncedMatchStart),
    DirectoryStarted(ServiceStart),
    Confirmed(ServiceStart),
}

#[derive(Debug)]
pub enum ServiceMatchError {
    Local(LocalMatchError),
    LobbyChanged {
        expected: String,
        observed: String,
    },
    MissingGameSeed,
    InvalidGameSeed(String),
    GameSeedChanged {
        expected: u32,
        observed: u32,
    },
    InvalidMemberId(String),
    DuplicateMemberId(i32),
    RosterMismatch {
        directory: Vec<i32>,
        transport: Vec<i32>,
    },
    RosterChangedAfterStart {
        expected: Vec<i32>,
        observed: Vec<i32>,
    },
    InvalidOwnerId(String),
    HostRosterInvalid(Vec<i32>),
    OwnerHostMismatch {
        owner: i32,
        transport_host: i32,
    },
    SessionReferenceBeforeStart,
    MissingSessionReference,
    DirectoryStartRegressed,
    DirectoryStartChanged {
        expected: ServiceStart,
        observed: ServiceStart,
    },
    DirectoryNotStarted,
    LobbyNotBound,
    HostOnly,
    MatchStartMismatch {
        expected: ServiceStart,
        observed: AnnouncedMatchStart,
    },
    TurnBeforeConfirmedStart,
}

impl fmt::Display for ServiceMatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local(error) => write!(f, "{error}"),
            Self::LobbyChanged { expected, observed } => write!(
                f,
                "lobby identity changed from {expected:?} to {observed:?}"
            ),
            Self::MissingGameSeed => write!(f, "lobby is missing {GAME_SEED_ATTRIBUTE:?}"),
            Self::InvalidGameSeed(value) => {
                write!(f, "lobby game seed is not a u32: {value:?}")
            }
            Self::GameSeedChanged { expected, observed } => {
                write!(f, "lobby game seed changed from {expected} to {observed}")
            }
            Self::InvalidMemberId(id) => write!(
                f,
                "Crossplay member id is not a canonical don-net i32: {id:?}"
            ),
            Self::DuplicateMemberId(id) => {
                write!(f, "Crossplay lobby contains duplicate member id {id}")
            }
            Self::RosterMismatch {
                directory,
                transport,
            } => write!(
                f,
                "Crossplay roster {directory:?} does not equal don-net roster {transport:?}"
            ),
            Self::RosterChangedAfterStart { expected, observed } => write!(
                f,
                "Crossplay roster changed after StartGame from {expected:?} to {observed:?}"
            ),
            Self::InvalidOwnerId(id) => write!(
                f,
                "Crossplay owner id is not a canonical don-net i32: {id:?}"
            ),
            Self::HostRosterInvalid(hosts) => {
                write!(f, "don-net roster must contain one host, found {hosts:?}")
            }
            Self::OwnerHostMismatch {
                owner,
                transport_host,
            } => write!(
                f,
                "Crossplay owner {owner} does not equal don-net host {transport_host}"
            ),
            Self::SessionReferenceBeforeStart => {
                write!(f, "directory exposed a session reference before StartGame")
            }
            Self::MissingSessionReference => {
                write!(f, "started directory lobby has no session reference")
            }
            Self::DirectoryStartRegressed => {
                write!(f, "directory StartGame regressed after it was observed")
            }
            Self::DirectoryStartChanged { expected, observed } => write!(
                f,
                "directory StartGame changed from {expected:?} to {observed:?}"
            ),
            Self::DirectoryNotStarted => write!(f, "directory StartGame is not complete"),
            Self::LobbyNotBound => write!(f, "no exact Crossplay/don-net roster is bound"),
            Self::HostOnly => write!(f, "only the directory owner/transport host may publish"),
            Self::MatchStartMismatch { expected, observed } => write!(
                f,
                "don-net MatchStart {observed:?} does not match directory state {expected:?}"
            ),
            Self::TurnBeforeConfirmedStart => write!(
                f,
                "refusing a turn before directory StartGame and don-net MatchStart agree"
            ),
        }
    }
}

impl std::error::Error for ServiceMatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Local(error) => Some(error),
            _ => None,
        }
    }
}

impl From<LocalMatchError> for ServiceMatchError {
    fn from(value: LocalMatchError) -> Self {
        Self::Local(value)
    }
}

/// Fail-closed adapter joining one Crossplay lobby to one `don-net` match.
pub struct ServiceMatch<T: Transport> {
    local: LocalMatch<T>,
    lobby_id: Option<String>,
    seed: Option<u32>,
    roster: Option<Vec<i32>>,
    host_id: Option<i32>,
    directory_start: Option<ServiceStart>,
    confirmed: Option<ServiceStart>,
}

impl<T: Transport> ServiceMatch<T> {
    pub fn new(local: LocalMatch<T>) -> Self {
        Self {
            local,
            lobby_id: None,
            seed: None,
            roster: None,
            host_id: None,
            directory_start: None,
            confirmed: None,
        }
    }

    pub fn role(&self) -> Role {
        self.local.role()
    }

    pub fn local_id(&self) -> i32 {
        self.local.session().local_id()
    }

    pub fn players(&self) -> &[Player] {
        self.local.session().players()
    }

    pub fn all_ready(&self) -> bool {
        self.local.session().all_ready()
    }

    pub fn phase(&self) -> ServiceMatchPhase {
        if let Some(start) = &self.confirmed {
            return ServiceMatchPhase::Confirmed(start.clone());
        }
        if let Some(started) = self.local.session().announced_match_start() {
            return ServiceMatchPhase::TransportStartedUnconfirmed(started);
        }
        if let Some(start) = &self.directory_start {
            return ServiceMatchPhase::DirectoryStarted(start.clone());
        }
        match self.local.phase() {
            LocalMatchPhase::AllReady => ServiceMatchPhase::AllReady,
            LocalMatchPhase::Lobby => ServiceMatchPhase::Lobby,
            LocalMatchPhase::Started(started) => {
                ServiceMatchPhase::TransportStartedUnconfirmed(started)
            }
        }
    }

    pub fn directory_start(&self) -> Option<&ServiceStart> {
        self.directory_start.as_ref()
    }

    pub fn confirmed_start(&self) -> Option<&ServiceStart> {
        self.confirmed.as_ref()
    }

    /// Validate and remember one GetLobby/CreateLobby/JoinLobby observation.
    /// This is intentionally a snapshot input rather than a directory client:
    /// callback/Tick chronology stays owned by `Backend`.
    pub fn observe_lobby(&mut self, lobby: &Lobby) -> Result<(), ServiceMatchError> {
        if let Some(expected) = &self.lobby_id {
            if expected != &lobby.id {
                return Err(ServiceMatchError::LobbyChanged {
                    expected: expected.clone(),
                    observed: lobby.id.clone(),
                });
            }
        }

        let seed_text = lobby
            .attributes
            .get(GAME_SEED_ATTRIBUTE)
            .ok_or(ServiceMatchError::MissingGameSeed)?;
        let observed_seed = seed_text
            .parse::<u32>()
            .map_err(|_| ServiceMatchError::InvalidGameSeed(seed_text.clone()))?;
        if let Some(expected) = self.seed {
            if expected != observed_seed {
                return Err(ServiceMatchError::GameSeedChanged {
                    expected,
                    observed: observed_seed,
                });
            }
        }

        let mut directory = Vec::with_capacity(lobby.members.len());
        let mut unique = BTreeSet::new();
        for member in &lobby.members {
            let id = canonical_i32(&member.user_id)
                .ok_or_else(|| ServiceMatchError::InvalidMemberId(member.user_id.clone()))?;
            if !unique.insert(id) {
                return Err(ServiceMatchError::DuplicateMemberId(id));
            }
            directory.push(id);
        }
        directory.sort_unstable();

        let mut transport: Vec<i32> = self
            .players()
            .iter()
            .map(|player| player.unique_id)
            .collect();
        transport.sort_unstable();
        if directory != transport {
            return Err(ServiceMatchError::RosterMismatch {
                directory,
                transport,
            });
        }

        let owner = canonical_i32(&lobby.owner_user_id)
            .ok_or_else(|| ServiceMatchError::InvalidOwnerId(lobby.owner_user_id.clone()))?;
        let hosts: Vec<i32> = self
            .players()
            .iter()
            .filter(|player| player.is_host)
            .map(|player| player.unique_id)
            .collect();
        let [transport_host] = hosts.as_slice() else {
            return Err(ServiceMatchError::HostRosterInvalid(hosts));
        };
        if owner != *transport_host {
            return Err(ServiceMatchError::OwnerHostMismatch {
                owner,
                transport_host: *transport_host,
            });
        }
        if let (Some(expected), Some(_)) = (&self.roster, &self.directory_start) {
            if expected != &directory {
                return Err(ServiceMatchError::RosterChangedAfterStart {
                    expected: expected.clone(),
                    observed: directory,
                });
            }
        }

        if lobby.game_started {
            if lobby.session_reference.is_empty() {
                return Err(ServiceMatchError::MissingSessionReference);
            }
            let observed = ServiceStart {
                lobby_id: lobby.id.clone(),
                session_reference: lobby.session_reference.clone(),
                host_id: owner,
                epoch: service_epoch(&lobby.id, &lobby.session_reference),
                seed: observed_seed,
            };
            if let Some(expected) = &self.directory_start {
                if expected != &observed {
                    return Err(ServiceMatchError::DirectoryStartChanged {
                        expected: expected.clone(),
                        observed,
                    });
                }
            } else {
                self.directory_start = Some(observed);
            }
        } else {
            if !lobby.session_reference.is_empty() {
                return Err(ServiceMatchError::SessionReferenceBeforeStart);
            }
            if self.directory_start.is_some() {
                return Err(ServiceMatchError::DirectoryStartRegressed);
            }
        }

        self.lobby_id.get_or_insert_with(|| lobby.id.clone());
        self.seed.get_or_insert(observed_seed);
        self.roster = Some(directory);
        self.host_id = Some(owner);
        self.reconcile()
    }

    pub fn poll(&mut self, now_ms: u64, budget: Duration) -> Result<(), ServiceMatchError> {
        self.local.poll(now_ms, budget)?;
        self.reconcile()
    }

    pub fn set_ready(&mut self, ready: bool) -> Result<(), ServiceMatchError> {
        // A caller must bind the exact lobby roster before readiness can be
        // treated as service state.
        self.validate_binding()?;
        self.local.set_ready(ready)?;
        Ok(())
    }

    /// Host-only transition after a started GetLobby snapshot was observed.
    pub fn publish_match_start(&mut self) -> Result<ServiceStart, ServiceMatchError> {
        if self.role() != Role::Host {
            return Err(ServiceMatchError::HostOnly);
        }
        self.validate_binding()?;
        let expected = self
            .directory_start
            .clone()
            .ok_or(ServiceMatchError::DirectoryNotStarted)?;
        self.local.start(expected.epoch, expected.seed)?;
        self.reconcile()?;
        Ok(expected)
    }

    pub fn send_turn(
        &mut self,
        stamp: u32,
        play: i8,
        payload: &[u8],
    ) -> Result<(), ServiceMatchError> {
        self.validate_binding()?;
        self.reconcile()?;
        if self.confirmed.is_none() {
            return Err(ServiceMatchError::TurnBeforeConfirmedStart);
        }
        self.local.send_turn(stamp, play, payload)?;
        Ok(())
    }

    pub fn turn_ready(&self, stamp: u32) -> bool {
        self.local.turn_ready(stamp)
    }

    pub fn take_turn(&mut self, stamp: u32) -> Vec<TurnPackage> {
        self.local.take_turn(stamp)
    }

    pub fn into_local_match(self) -> LocalMatch<T> {
        self.local
    }

    fn reconcile(&mut self) -> Result<(), ServiceMatchError> {
        let Some(observed) = self.local.session().announced_match_start() else {
            return Ok(());
        };
        let Some(expected) = self.directory_start.as_ref() else {
            // A valid host packet may race ahead of this process's next
            // directory poll. Preserve it, but do not admit turns yet.
            return Ok(());
        };
        if observed.from != expected.host_id
            || observed.epoch != expected.epoch
            || observed.seed != expected.seed
        {
            return Err(ServiceMatchError::MatchStartMismatch {
                expected: expected.clone(),
                observed,
            });
        }
        self.confirmed = Some(expected.clone());
        Ok(())
    }

    fn validate_binding(&self) -> Result<(), ServiceMatchError> {
        let expected = self
            .roster
            .as_ref()
            .ok_or(ServiceMatchError::LobbyNotBound)?;
        let mut transport: Vec<i32> = self
            .players()
            .iter()
            .map(|player| player.unique_id)
            .collect();
        transport.sort_unstable();
        if expected != &transport {
            return Err(ServiceMatchError::RosterMismatch {
                directory: expected.clone(),
                transport,
            });
        }
        let hosts: Vec<i32> = self
            .players()
            .iter()
            .filter(|player| player.is_host)
            .map(|player| player.unique_id)
            .collect();
        let [transport_host] = hosts.as_slice() else {
            return Err(ServiceMatchError::HostRosterInvalid(hosts));
        };
        let expected_host = self.host_id.ok_or(ServiceMatchError::LobbyNotBound)?;
        if expected_host != *transport_host {
            return Err(ServiceMatchError::OwnerHostMismatch {
                owner: expected_host,
                transport_host: *transport_host,
            });
        }
        Ok(())
    }
}

/// Stable DoN policy for turning the completed StartGame identity into the
/// non-zero u32 epoch carried by the local MatchStart extension.
pub fn service_epoch(lobby_id: &str, session_reference: &str) -> u32 {
    // FNV-1a is specified here rather than delegated to Hash so results are
    // identical across processes, Rust versions, and target architectures.
    let mut hash = 0x811c_9dc5u32;
    for byte in lobby_id
        .as_bytes()
        .iter()
        .copied()
        .chain(core::iter::once(0))
        .chain(session_reference.as_bytes().iter().copied())
    {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    if hash == 0 {
        1
    } else {
        hash
    }
}

fn canonical_i32(text: &str) -> Option<i32> {
    let parsed = text.parse::<i32>().ok()?;
    (parsed.to_string() == text).then_some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Attributes, Member};
    use don_net::{LoopTransport, Session};
    use std::vec;

    fn lobby(started: bool) -> Lobby {
        let mut attributes = Attributes::new();
        attributes.insert(GAME_SEED_ATTRIBUTE.into(), "3134984190".into());
        Lobby {
            id: "lobby-7".into(),
            owner_user_id: "101".into(),
            session_reference: if started {
                "start-9".into()
            } else {
                String::new()
            },
            max_members: 2,
            bot_count: 0,
            attribute_version: 0,
            visibility: 1,
            members: vec![Member::new("101", "Host"), Member::new("202", "Peer")],
            attributes,
            game_started: started,
        }
    }

    fn connected() -> (ServiceMatch<LoopTransport>, ServiceMatch<LoopTransport>) {
        let mut host = LoopTransport::new(101);
        let mut client = LoopTransport::new(202);
        host.connect(202);
        client.connect(101);
        let mut host = LocalMatch::new(Session::new(host, Role::Host, "Host"));
        let mut client = LocalMatch::new(Session::new(client, Role::Client, "Peer"));
        for now in 0..5 {
            host.poll(now, Duration::ZERO).unwrap();
            client.poll(now, Duration::ZERO).unwrap();
            LoopTransport::pump(
                &mut host.session_mut().transport,
                &mut client.session_mut().transport,
            );
        }
        (ServiceMatch::new(host), ServiceMatch::new(client))
    }

    fn ready_pair() -> (ServiceMatch<LoopTransport>, ServiceMatch<LoopTransport>) {
        let (mut host, mut client) = connected();
        let before = lobby(false);
        host.observe_lobby(&before).unwrap();
        client.observe_lobby(&before).unwrap();
        host.set_ready(true).unwrap();
        client.set_ready(true).unwrap();
        for now in 10..20 {
            host.poll(now, Duration::ZERO).unwrap();
            client.poll(now, Duration::ZERO).unwrap();
            LoopTransport::pump(
                &mut host.local.session_mut().transport,
                &mut client.local.session_mut().transport,
            );
        }
        assert!(host.all_ready() && client.all_ready());
        (host, client)
    }

    #[test]
    fn exact_roster_is_required_before_binding() {
        let (mut host, _) = connected();
        let mut wrong = lobby(false);
        wrong.members[1].user_id = "203".into();
        assert!(matches!(
            host.observe_lobby(&wrong),
            Err(ServiceMatchError::RosterMismatch { .. })
        ));
    }

    #[test]
    fn turn_stays_closed_until_directory_and_transport_start_agree() {
        let (mut host, mut client) = ready_pair();

        let started = lobby(true);
        host.observe_lobby(&started).unwrap();
        host.publish_match_start().unwrap();
        LoopTransport::pump(
            &mut host.local.session_mut().transport,
            &mut client.local.session_mut().transport,
        );
        client.poll(21, Duration::ZERO).unwrap();
        assert!(matches!(
            client.phase(),
            ServiceMatchPhase::TransportStartedUnconfirmed(_)
        ));
        assert!(matches!(
            client.send_turn(0, 1, &[0x39]),
            Err(ServiceMatchError::TurnBeforeConfirmedStart)
        ));

        client.observe_lobby(&started).unwrap();
        assert!(matches!(client.phase(), ServiceMatchPhase::Confirmed(_)));
        client.send_turn(0, 1, &[0x39]).unwrap();
    }

    #[test]
    fn a_different_start_reference_cannot_confirm_the_transport_packet() {
        let (mut host, mut client) = ready_pair();
        let mut host_started = lobby(true);
        host_started.session_reference = "host-start".into();
        host.observe_lobby(&host_started).unwrap();
        host.publish_match_start().unwrap();
        LoopTransport::pump(
            &mut host.local.session_mut().transport,
            &mut client.local.session_mut().transport,
        );
        client.poll(21, Duration::ZERO).unwrap();

        let client_started = lobby(true);
        assert!(matches!(
            client.observe_lobby(&client_started),
            Err(ServiceMatchError::MatchStartMismatch { .. })
        ));
        assert!(client.confirmed_start().is_none());
    }

    #[test]
    fn epoch_is_stable_nonzero_and_sensitive_to_start_identity() {
        let first = service_epoch("lobby-7", "start-9");
        assert_ne!(first, 0);
        assert_eq!(first, service_epoch("lobby-7", "start-9"));
        assert_ne!(first, service_epoch("lobby-7", "start-10"));
        assert_ne!(first, service_epoch("lobby-8", "start-9"));
    }
}
