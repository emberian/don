//! The explicit lifecycle around a DoN-owned local lockstep match.
//!
//! [`crate::session::Session`] owns the recovered roster/readiness/control
//! packets and the lockstep turn queue.  Before this module existed, every
//! local acceptance jumped directly from `Session::all_ready()` to
//! `Session::send_command_package()`: host, join, ready and turns were real,
//! but *start* was only an assumption in the harness.
//!
//! `LocalMatch` is the small product boundary that refuses that jump.  The
//! authoritative host must publish [`crate::extension::DonExtension::MatchStart`]
//! first; clients accept it only from that host and only after the complete
//! roster is ready.  The extension and this policy are DoN-owned. They are not
//! presented as recovered retail traffic.
//!
//! This module still begins after a transport connection exists and has no
//! lobby-service dependency. The opt-in `don-crossplay::match_bridge` adapter
//! now binds its roster and MatchStart to Crossplay Create/Find/Join/StartGame
//! state across processes; keeping that policy outside this crate leaves the
//! transport/session layer reusable on its own.

use crate::session::{AnnouncedMatchStart, MatchStartError, Role, Session, TurnPackage};
use crate::transport::Transport;
use std::io;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalMatchPhase {
    /// Transport membership is still converging or one or more players are not
    /// ready.
    Lobby,
    /// The complete current roster is ready, but the host has not started.
    AllReady,
    /// The explicit host-authoritative start transaction was accepted.
    Started(AnnouncedMatchStart),
}

#[derive(Debug)]
pub enum LocalMatchError {
    Start(MatchStartError),
    TurnBeforeStart,
    Transport(io::Error),
}

impl core::fmt::Display for LocalMatchError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LocalMatchError::Start(error) => write!(f, "{error}"),
            LocalMatchError::TurnBeforeStart => {
                write!(f, "refusing a lockstep package before MatchStart")
            }
            LocalMatchError::Transport(error) => write!(f, "local match transport: {error}"),
        }
    }
}

impl std::error::Error for LocalMatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LocalMatchError::Start(error) => Some(error),
            LocalMatchError::Transport(error) => Some(error),
            LocalMatchError::TurnBeforeStart => None,
        }
    }
}

impl From<MatchStartError> for LocalMatchError {
    fn from(value: MatchStartError) -> Self {
        Self::Start(value)
    }
}

impl From<io::Error> for LocalMatchError {
    fn from(value: io::Error) -> Self {
        Self::Transport(value)
    }
}

pub struct LocalMatch<T: Transport> {
    session: Session<T>,
}

impl<T: Transport> LocalMatch<T> {
    pub fn new(session: Session<T>) -> Self {
        Self { session }
    }

    pub fn role(&self) -> Role {
        self.session.role
    }

    pub fn phase(&self) -> LocalMatchPhase {
        if let Some(started) = self.session.announced_match_start() {
            LocalMatchPhase::Started(started)
        } else if self.session.all_ready() {
            LocalMatchPhase::AllReady
        } else {
            LocalMatchPhase::Lobby
        }
    }

    pub fn session(&self) -> &Session<T> {
        &self.session
    }

    pub fn session_mut(&mut self) -> &mut Session<T> {
        &mut self.session
    }

    pub fn into_session(self) -> Session<T> {
        self.session
    }

    pub fn poll(&mut self, now_ms: u64, budget: Duration) -> Result<(), LocalMatchError> {
        self.session.poll(now_ms, budget)?;
        Ok(())
    }

    pub fn set_ready(&mut self, ready: bool) -> Result<(), LocalMatchError> {
        self.session.send_ready_flag(ready)?;
        Ok(())
    }

    /// Start one local match. The underlying session enforces host authority,
    /// a non-zero attempt epoch, and complete readiness.
    pub fn start(&mut self, epoch: u32, seed: u32) -> Result<AnnouncedMatchStart, LocalMatchError> {
        Ok(self.session.start_match(epoch, seed)?)
    }

    /// Submit the local participant's package only after the explicit start.
    pub fn send_turn(
        &mut self,
        stamp: u32,
        play: i8,
        payload: &[u8],
    ) -> Result<(), LocalMatchError> {
        if !matches!(self.phase(), LocalMatchPhase::Started(_)) {
            return Err(LocalMatchError::TurnBeforeStart);
        }
        self.session.send_command_package(stamp, play, payload)?;
        Ok(())
    }

    pub fn turn_ready(&self, stamp: u32) -> bool {
        self.session.turn_ready(stamp)
    }

    pub fn take_turn(&mut self, stamp: u32) -> Vec<TurnPackage> {
        self.session.take_turn(stamp)
    }
}
