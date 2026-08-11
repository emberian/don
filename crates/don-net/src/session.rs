//! The session layer: membership, readiness, and the lockstep turn clock.
//!
//! This is the state machine `CrossplayNetLibSys` runs on top of a datagram
//! transport, reimplemented against the same wire formats. It handles the
//! netlib control plane ([`crate::internal`]) itself and hands game messages
//! ([`crate::msg`]) to the caller, which is exactly the split the shipped code
//! makes: type `>= 128` is consumed by the netlib, type `< 128` is pushed into
//! `fifo_receive` for `NetSys::get` to pop.
//!
//! # What is faithful and what is ours
//!
//! **Faithful** (byte-for-byte the shipped formats): every packet encoded and
//! decoded here, the readiness semantics (`IPT_READYFLAG` sets the peer's
//! `ready` bit; `reset_ready_flags` clears all), the host's authority over the
//! roster (`IPT_PLAYERLIST` is host-to-clients only), pulse-driven liveness,
//! and the host relay of command packages.
//!
//! **Ours** (a design choice, not a derivation): the *order* in which the join
//! handshake steps happen. The shipped order is not recorded anywhere we can
//! read without running the game; what we have is the set of messages and their
//! handlers. [`Session`] picks the order that satisfies every handler's
//! precondition — announce, receive roster, ready — and says so here rather
//! than in a comment nobody reads.
//!
//! # Turn model
//!
//! `TurnControl::timings` at `0x00AFC4A4` is `{200, 125, 67, 50, 1}` ms; Normal
//! is index 2, so a tick is **67 ms**. `TurnControl::turn_length` is how many
//! frames a turn spans. A peer may not simulate frame `f` until it holds every
//! peer's `CommandPackage` for the turn covering `f`. [`Session::turn_ready`]
//! is that predicate and nothing more — the simulation lives in `don-sim`.

use crate::extension::{DonExtension, ExtensionError, GameKeySource};
use crate::internal::{InternalError, InternalPacket, MAX_PLAYERS};
use crate::msg::{Framed, MsgError, NetMsg};
use crate::transport::{Datagram, Dest, Transport};
use std::collections::BTreeMap;
use std::io;
use std::time::Duration;

/// `TurnControl::timings` — milliseconds per frame by speed index.
/// `0x00AFC4A4`. **[measured]**
pub const TURN_TIMINGS_MS: [i32; 5] = [200, 125, 67, 50, 1];
/// Index into [`TURN_TIMINGS_MS`] for Normal speed.
pub const SPEED_NORMAL: usize = 2;

/// One remote participant, mirroring `CrossplayNetLibPlayer`'s live fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Player {
    /// `CrossplayNetLibPlayer::unique_id` — the netlib's own identity, an i32,
    /// distinct from the PlayFab entity id string.
    pub unique_id: i32,
    /// `AddPlayerRequest::player_name`, at most 64 bytes.
    pub name: String,
    /// `SNLPLAYER_HOST` (flag 1).
    pub is_host: bool,
    /// `SNLPLAYER_LOCAL` (flag 4).
    pub is_local: bool,
    /// `CrossplayNetLibPlayer::ready` at +40, driven by `IPT_READYFLAG`.
    pub ready: bool,
    /// `CrossplayNetLibPlayer::last_pulse` at +160, in the caller's clock.
    pub last_pulse_ms: u64,
    /// Slot in the roster; the host assigns it and `IPT_PLAYERLIST` carries it
    /// implicitly as array position.
    pub slot: usize,
}

/// Events the session surfaces to its owner. Netlib control traffic never
/// reaches here; it is handled internally, exactly as the real netlib does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    PlayerJoined(i32),
    PlayerLeft(i32),
    /// Every peer in the roster has its ready flag set. This is the gate the
    /// host uses before `StartGame`.
    AllReady,
    ReadyChanged {
        unique_id: i32,
        ready: bool,
    },
    /// A setup/control packet was malformed or contradicted transport/host
    /// authority. The packet is consumed without mutating membership.
    SetupRefused {
        from: i32,
        reason: SetupRefusal,
    },
    /// A peer reported a desync at `frame` (`IPT_DSYNCMSG`).
    Desync {
        unique_id: i32,
        frame: i32,
    },
    /// The host announced the match's `GameInfo::seed` over the DoN transport
    /// extension. Not a retail packet — see [`crate::extension`].
    GameKeyAnnounced {
        from: i32,
        seed: u32,
        source: GameKeySource,
    },
    /// The authoritative host crossed the explicit DoN-owned match-start
    /// boundary after every roster member was ready.
    MatchStarted(AnnouncedMatchStart),
    /// A game-layer message arrived. `from` is the sender's unique id.
    Game {
        from: i32,
        msg: OwnedMsg,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupRefusal {
    Malformed(InternalError),
    MalformedExtension(ExtensionError),
    /// A game key may only come from the authoritative host. Accepting one from
    /// any peer would let a third party choose how we decode retail's packages.
    GameKeyFromNonHost {
        expected_host: Option<i32>,
    },
    MatchStartFromNonHost {
        expected_host: Option<i32>,
    },
    MatchStartBeforeAllReady,
    MatchStartInvalidEpoch {
        epoch: u32,
    },
    MatchStartConflict {
        current_epoch: u32,
        current_seed: u32,
        announced_epoch: u32,
        announced_seed: u32,
    },
    SenderIdMismatch {
        announced: i32,
    },
    UnexpectedHostClaim,
    DuplicateAddConflict {
        unique_id: i32,
    },
    PlayerListFromNonHost,
    PlayerListHostMismatch {
        expected: i32,
    },
    HostMissingFromSlotZero,
    LocalMissingFromRoster,
    ReadyBeforeAdd,
    DestroySenderMismatch {
        announced: i32,
    },
}

/// An owned copy of a decoded game message, so events can outlive the buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedMsg {
    pub id: u8,
    pub response: bool,
    pub bytes: Vec<u8>,
}

impl OwnedMsg {
    pub fn decode(&self) -> Result<Framed<'_>, MsgError> {
        NetMsg::decode(&self.bytes)
    }
}

/// A command package awaiting execution, keyed by (stamp, player slot).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnPackage {
    pub stamp: u32,
    pub play: i8,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Host,
    Client,
}

/// One accepted DoN-owned match-start announcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnnouncedMatchStart {
    pub from: i32,
    pub epoch: u32,
    pub seed: u32,
}

/// Why the local side could not originate a match-start transaction.
#[derive(Debug)]
pub enum MatchStartError {
    NotHost,
    NotAllReady,
    ZeroEpoch,
    AlreadyStarted {
        current: AnnouncedMatchStart,
        requested_epoch: u32,
        requested_seed: u32,
    },
    Transport(io::Error),
}

impl core::fmt::Display for MatchStartError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            MatchStartError::NotHost => write!(f, "only the authoritative host may start a match"),
            MatchStartError::NotAllReady => write!(f, "the complete roster is not ready"),
            MatchStartError::ZeroEpoch => write!(f, "match epoch zero is reserved"),
            MatchStartError::AlreadyStarted {
                current,
                requested_epoch,
                requested_seed,
            } => write!(
                f,
                "match already started at epoch {} seed {:#010x}; refused epoch {} seed {:#010x}",
                current.epoch, current.seed, requested_epoch, requested_seed
            ),
            MatchStartError::Transport(error) => write!(f, "send match start: {error}"),
        }
    }
}

impl std::error::Error for MatchStartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            MatchStartError::Transport(error) => Some(error),
            _ => None,
        }
    }
}

/// The session. Generic over the transport so the same state machine drives
/// loopback in a test, TCP over the internet, and (in `netsys-shim`) the real
/// game's `NetSys` vtable.
pub struct Session<T: Transport> {
    pub transport: T,
    pub role: Role,
    pub local_name: String,
    players: Vec<Player>,
    events: Vec<Event>,
    /// Received command packages: stamp -> slot -> package.
    turns: BTreeMap<u32, BTreeMap<i8, TurnPackage>>,
    /// Transport peers we have already introduced ourselves to. Membership is
    /// per-peer, not global: see `republish` for why.
    announced_to: Vec<i32>,
    all_ready_fired: bool,
    now_ms: u64,
    last_pulse_sent_ms: u64,
    pulse_interval_ms: u64,
    timeout_ms: u64,
    /// The most recent host-announced match key, and who announced it. This is
    /// the DoN transport extension, not retail traffic — see
    /// [`crate::extension`].
    announced_game_key: Option<AnnouncedGameKey>,
    /// The accepted host-authoritative DoN match-start transaction. This is
    /// deliberately separate from `announced_game_key`: retail interop may
    /// announce a transform key after retail starts, while a DoN-owned match
    /// uses this explicit pre-turn boundary.
    announced_match_start: Option<AnnouncedMatchStart>,
}

/// A host-announced `GameInfo::seed` and its provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnnouncedGameKey {
    pub from: i32,
    pub seed: u32,
    pub source: GameKeySource,
}

impl<T: Transport> Session<T> {
    pub fn new(transport: T, role: Role, local_name: impl Into<String>) -> Self {
        let id = transport.local_id();
        let name = local_name.into();
        let mut s = Session {
            transport,
            role,
            local_name: name.clone(),
            players: Vec::new(),
            events: Vec::new(),
            turns: BTreeMap::new(),
            announced_to: Vec::new(),
            all_ready_fired: false,
            now_ms: 0,
            last_pulse_sent_ms: 0,
            pulse_interval_ms: 1000,
            timeout_ms: 30_000,
            announced_game_key: None,
            announced_match_start: None,
        };
        s.players.push(Player {
            unique_id: id,
            name,
            is_host: role == Role::Host,
            is_local: true,
            ready: false,
            last_pulse_ms: 0,
            slot: 0,
        });
        s
    }

    /// `NetSys::set_time_out(ulong)` / `get_time_out`.
    pub fn set_timeout_ms(&mut self, ms: u64) {
        self.timeout_ms = ms;
    }

    pub fn players(&self) -> &[Player] {
        &self.players
    }

    /// Clear every per-attempt state while keeping the transport allocation,
    /// its local identity, and the configured local name reusable.
    ///
    /// `CrossplayNetLibSys::init` begins by closing the previous session.  A
    /// replacement that only clears its C++-visible player pointers leaves a
    /// second, hidden roster in this object; the next poll can then resurrect
    /// stale peers, events, or turn packages.  Reset the complete session
    /// epoch instead.  The caller supplies the configured role because host
    /// migration may have changed `self.role` during the old attempt.
    pub fn reset_for_reuse(&mut self, role: Role) {
        let local_id = self.transport.local_id();
        self.role = role;
        self.players.clear();
        self.players.push(Player {
            unique_id: local_id,
            name: self.local_name.clone(),
            is_host: role == Role::Host,
            is_local: true,
            ready: false,
            last_pulse_ms: 0,
            slot: 0,
        });
        self.events.clear();
        self.turns.clear();
        self.announced_to.clear();
        self.all_ready_fired = false;
        self.now_ms = 0;
        self.last_pulse_sent_ms = 0;
        // A key belongs to one match. `init` starts a new attempt epoch, so
        // carrying the previous match's seed into it would be the exact silent
        // substitution this whole path exists to avoid.
        self.announced_game_key = None;
        self.announced_match_start = None;
    }

    pub fn local_id(&self) -> i32 {
        self.transport.local_id()
    }

    pub fn find(&self, unique_id: i32) -> Option<&Player> {
        self.players.iter().find(|p| p.unique_id == unique_id)
    }

    /// Everyone in the roster has their ready flag set.
    pub fn all_ready(&self) -> bool {
        !self.players.is_empty() && self.players.iter().all(|p| p.ready)
    }

    /// `CrossplayNetLibSys::send_ready_flag(bool)` — one of the nine symbols
    /// `riseofnations.exe` imports directly.
    pub fn send_ready_flag(&mut self, ready: bool) -> io::Result<()> {
        if let Some(me) = self.players.iter_mut().find(|p| p.is_local) {
            me.ready = ready;
        }
        self.emit_all(&InternalPacket::ReadyFlag { ready })
    }

    /// `CrossplayNetLibSys::reset_ready_flags()` — host-side; clears every
    /// peer's flag locally and tells them so.
    pub fn reset_ready_flags(&mut self) -> io::Result<()> {
        for p in self.players.iter_mut() {
            p.ready = false;
        }
        self.all_ready_fired = false;
        self.emit_all(&InternalPacket::ReadyFlag { ready: false })
    }

    /// `CrossplayNetLibSys::send_dsync(int)`.
    pub fn send_dsync(&mut self, frame: i32) -> io::Result<()> {
        self.emit_all(&InternalPacket::Dsync { frame })
    }

    /// The match key this session was told about, if any.
    pub fn announced_game_key(&self) -> Option<AnnouncedGameKey> {
        self.announced_game_key
    }

    /// The accepted explicit DoN match-start transaction, if one exists.
    pub fn announced_match_start(&self) -> Option<AnnouncedMatchStart> {
        self.announced_match_start
    }

    /// Publish the host-authoritative transition from all-ready to match play.
    ///
    /// This packet is **[DoN policy]**, not a retail wire claim. It closes the
    /// gap that previously let the headless local path jump straight from
    /// readiness into turn zero without any observable start transaction.
    /// Sending happens before local state changes, so an I/O failure cannot
    /// leave the host believing a start was published when no peer saw it.
    pub fn start_match(
        &mut self,
        epoch: u32,
        seed: u32,
    ) -> Result<AnnouncedMatchStart, MatchStartError> {
        if self.role != Role::Host {
            return Err(MatchStartError::NotHost);
        }
        if epoch == 0 {
            return Err(MatchStartError::ZeroEpoch);
        }
        if !self.all_ready() {
            return Err(MatchStartError::NotAllReady);
        }
        if let Some(current) = self.announced_match_start {
            if current.epoch == epoch && current.seed == seed {
                return Ok(current);
            }
            return Err(MatchStartError::AlreadyStarted {
                current,
                requested_epoch: epoch,
                requested_seed: seed,
            });
        }

        let mut bytes = Vec::new();
        DonExtension::MatchStart { epoch, seed }.encode(&mut bytes);
        self.transport
            .send(Dest::All, &bytes)
            .map_err(MatchStartError::Transport)?;
        let started = AnnouncedMatchStart {
            from: self.local_id(),
            epoch,
            seed,
        };
        self.announced_match_start = Some(started);
        self.events.push(Event::MatchStarted(started));
        Ok(started)
    }

    /// Announce the match key to one peer over the DoN transport extension.
    ///
    /// Not a retail packet. Only the host has a reason to call this: it is the
    /// side running inside `riseofnations.exe`, where `GameInfo::seed` lives.
    pub fn announce_game_key_to(
        &mut self,
        peer: i32,
        seed: u32,
        source: GameKeySource,
    ) -> io::Result<()> {
        let mut bytes = Vec::new();
        DonExtension::GameKey { seed, source }.encode(&mut bytes);
        self.transport.send(Dest::One(peer), &bytes)
    }

    /// Announce the match key to every peer.
    pub fn announce_game_key(&mut self, seed: u32, source: GameKeySource) -> io::Result<()> {
        let mut bytes = Vec::new();
        DonExtension::GameKey { seed, source }.encode(&mut bytes);
        self.transport.send(Dest::All, &bytes)
    }

    /// Announce an orderly local departure with the exact five-byte
    /// `IPT_DESTROYPLAYER` packet before the transport is closed. A later
    /// connection may reuse the same owned id as a new membership epoch.
    pub fn announce_disconnect(&mut self) -> io::Result<()> {
        self.emit_all(&InternalPacket::DestroyPlayer {
            unique_id: self.local_id(),
        })
    }

    /// Send a game message to every peer — `NetSys::send_all`.
    pub fn send_all(&mut self, msg: &NetMsg<'_>) -> io::Result<()> {
        let mut b = Vec::new();
        msg.encode(&mut b);
        self.transport.send(Dest::All, &b)
    }

    /// Send a game message to one peer — `NetSys::send`.
    pub fn send_to(&mut self, unique_id: i32, msg: &NetMsg<'_>) -> io::Result<()> {
        let mut b = Vec::new();
        msg.encode(&mut b);
        self.transport.send(Dest::One(unique_id), &b)
    }

    /// Queue the local command package for `stamp` and broadcast it.
    pub fn send_command_package(&mut self, stamp: u32, play: i8, payload: &[u8]) -> io::Result<()> {
        if payload.len() > crate::MAX_COMMAND_PACKAGE_PAYLOAD {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "command package payload is {} bytes; retail capacity is {}",
                    payload.len(),
                    crate::MAX_COMMAND_PACKAGE_PAYLOAD,
                ),
            ));
        }
        // A failed transport send must not leave a local-only package that can
        // satisfy this participant's lockstep barrier. Queue the owned copy
        // only after the complete framed message was accepted by Transport.
        self.send_all(&NetMsg::CommandPackage {
            stamp,
            play,
            payload,
        })?;
        self.turns.entry(stamp).or_default().insert(
            play,
            TurnPackage {
                stamp,
                play,
                payload: payload.to_vec(),
            },
        );
        Ok(())
    }

    /// Have we got a package from every player for this stamp? This is the
    /// lockstep gate: `CommandManager::process_turn` `0x0093EF10` will not
    /// advance without one package per participating slot.
    pub fn turn_ready(&self, stamp: u32) -> bool {
        let want = self.players.len();
        self.turns
            .get(&stamp)
            .map(|m| m.len() >= want)
            .unwrap_or(false)
    }

    /// Borrow one received/local package without consuming the turn. This is
    /// the inspection point used to validate retail checksum traffic before a
    /// reactive peer emits its same-stamp reply.
    pub fn package_for_turn(&self, stamp: u32, play: i8) -> Option<&TurnPackage> {
        self.turns.get(&stamp)?.get(&play)
    }

    /// Take every package for a stamp, in slot order.
    pub fn take_turn(&mut self, stamp: u32) -> Vec<TurnPackage> {
        self.turns
            .remove(&stamp)
            .map(|m| m.into_values().collect())
            .unwrap_or_default()
    }

    pub fn drain_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// Advance the session. `now_ms` is a monotonic millisecond clock supplied
    /// by the caller — the crate has no dependencies and takes no opinion on
    /// where time comes from.
    pub fn poll(&mut self, now_ms: u64, budget: Duration) -> io::Result<()> {
        self.now_ms = now_ms;

        // Introduce ourselves to every transport peer we have not greeted yet,
        // and immediately restate our ready flag to that peer.
        //
        // Both halves are load-bearing, and the second one was paid for. The
        // netlib's `process_ready_flag(const CrossplayNetLibPlayer*,
        // ReadyFlagRequest*)` takes an already-resolved player, so a flag from
        // a peer the receiver has not yet added is *dropped* — and nothing in
        // the protocol ever asks for it again. The shipped game never hits that
        // race because membership is lobby-driven: `OnPlayerJoined` materialises
        // the peer object before any P2P channel to it exists. We have no lobby
        // in front of the transport, so ordering is not guaranteed, and an
        // edge-triggered ready flag deadlocks: measured, two TCP peers hung
        // forever with each believing the other was not ready.
        //
        // The fix is to treat readiness as *state* rather than an event and
        // republish it whenever the peer set changes. That is strictly more
        // robust than the shipped behaviour and cannot desynchronise it: the
        // packet is byte-identical, only the retransmit policy differs.
        let fresh: Vec<i32> = self
            .transport
            .peers()
            .into_iter()
            .filter(|p| !self.announced_to.contains(p))
            .collect();
        if !fresh.is_empty() {
            let me = self.local_id();
            let name = self.local_name.clone();
            let hosting = self.role == Role::Host;
            let ready = self
                .players
                .iter()
                .find(|p| p.is_local)
                .map(|p| p.ready)
                .unwrap_or(false);
            for peer in fresh {
                self.emit_to(
                    peer,
                    &InternalPacket::AddPlayer {
                        player_name: name.clone(),
                        unique_id: me,
                        is_hosting: hosting,
                    },
                )?;
                self.emit_to(peer, &InternalPacket::ReadyFlag { ready })?;
                self.announced_to.push(peer);
            }
        }

        if now_ms.saturating_sub(self.last_pulse_sent_ms) >= self.pulse_interval_ms {
            self.emit_all(&InternalPacket::Pulse)?;
            self.last_pulse_sent_ms = now_ms;
        }

        self.transport.poll(budget)?;
        while let Some(d) = self.transport.recv()? {
            self.handle(d)?;
        }

        self.reap_timeouts();

        if self.all_ready() && !self.all_ready_fired {
            self.all_ready_fired = true;
            self.events.push(Event::AllReady);
        } else if !self.all_ready() {
            self.all_ready_fired = false;
        }
        Ok(())
    }

    // -- internals ---------------------------------------------------------

    fn emit_all(&mut self, p: &InternalPacket) -> io::Result<()> {
        let mut b = Vec::new();
        p.encode(&mut b);
        self.transport.send(Dest::All, &b)
    }

    fn emit_to(&mut self, peer: i32, p: &InternalPacket) -> io::Result<()> {
        let mut b = Vec::new();
        p.encode(&mut b);
        self.transport.send(Dest::One(peer), &b)
    }

    /// Restate our own ready flag to everyone. Called whenever the roster
    /// grows, so a peer that joined after we readied still learns our state.
    fn republish_ready(&mut self) -> io::Result<()> {
        let ready = self
            .players
            .iter()
            .find(|p| p.is_local)
            .map(|p| p.ready)
            .unwrap_or(false);
        self.emit_all(&InternalPacket::ReadyFlag { ready })
    }

    fn handle(&mut self, d: Datagram) -> io::Result<()> {
        let Some(&first) = d.bytes.first() else {
            return Ok(());
        };
        if DonExtension::is_extension(first) {
            // Ours, not retail's. Consumed here exactly like the shipped
            // internal range, so it can never reach the game layer.
            let p = match DonExtension::decode(&d.bytes) {
                Ok(packet) => packet,
                Err(error) => {
                    self.events.push(Event::SetupRefused {
                        from: d.from,
                        reason: SetupRefusal::MalformedExtension(error),
                    });
                    return Ok(());
                }
            };
            return self.handle_extension(d.from, p);
        }
        if first >= crate::internal::IPT_BASE {
            let p = match InternalPacket::decode(&d.bytes) {
                Ok(packet) => packet,
                Err(error) => {
                    self.events.push(Event::SetupRefused {
                        from: d.from,
                        reason: SetupRefusal::Malformed(error),
                    });
                    return Ok(());
                }
            };
            self.handle_internal(d.from, p)
        } else {
            self.handle_game(d)
        }
    }

    fn handle_extension(&mut self, from: i32, p: DonExtension) -> io::Result<()> {
        match p {
            DonExtension::GameKey { seed, source } => {
                let host = self
                    .players
                    .iter()
                    .find(|player| player.is_host)
                    .map(|player| player.unique_id);
                if host != Some(from) {
                    self.events.push(Event::SetupRefused {
                        from,
                        reason: SetupRefusal::GameKeyFromNonHost {
                            expected_host: host,
                        },
                    });
                    return Ok(());
                }
                let announcement = AnnouncedGameKey { from, seed, source };
                // Re-announcement of the same key is normal: the announcing side
                // restates it to every peer it has not told. Only a change is
                // news, and it is reported rather than resolved here — the
                // consumer decides whether a second key is legitimate.
                if self.announced_game_key != Some(announcement) {
                    self.announced_game_key = Some(announcement);
                    self.events
                        .push(Event::GameKeyAnnounced { from, seed, source });
                }
                Ok(())
            }
            DonExtension::MatchStart { epoch, seed } => {
                let host = self
                    .players
                    .iter()
                    .find(|player| player.is_host)
                    .map(|player| player.unique_id);
                if host != Some(from) {
                    self.events.push(Event::SetupRefused {
                        from,
                        reason: SetupRefusal::MatchStartFromNonHost {
                            expected_host: host,
                        },
                    });
                    return Ok(());
                }
                if !self.all_ready() {
                    self.events.push(Event::SetupRefused {
                        from,
                        reason: SetupRefusal::MatchStartBeforeAllReady,
                    });
                    return Ok(());
                }
                if epoch == 0 {
                    self.events.push(Event::SetupRefused {
                        from,
                        reason: SetupRefusal::MatchStartInvalidEpoch { epoch },
                    });
                    return Ok(());
                }
                let announced = AnnouncedMatchStart { from, epoch, seed };
                match self.announced_match_start {
                    None => {
                        self.announced_match_start = Some(announced);
                        self.events.push(Event::MatchStarted(announced));
                    }
                    Some(current) if current == announced => {}
                    Some(current) => self.events.push(Event::SetupRefused {
                        from,
                        reason: SetupRefusal::MatchStartConflict {
                            current_epoch: current.epoch,
                            current_seed: current.seed,
                            announced_epoch: epoch,
                            announced_seed: seed,
                        },
                    }),
                }
                Ok(())
            }
        }
    }

    fn handle_internal(&mut self, from: i32, p: InternalPacket) -> io::Result<()> {
        match p {
            // `process_create_player_message`
            InternalPacket::AddPlayer {
                player_name,
                unique_id,
                is_hosting,
            } => {
                if unique_id != from {
                    self.events.push(Event::SetupRefused {
                        from,
                        reason: SetupRefusal::SenderIdMismatch {
                            announced: unique_id,
                        },
                    });
                    return Ok(());
                }
                if is_hosting {
                    let known_host = self.players.iter().find(|p| p.is_host);
                    if self.role == Role::Host
                        || known_host.is_some_and(|host| host.unique_id != unique_id)
                    {
                        self.events.push(Event::SetupRefused {
                            from,
                            reason: SetupRefusal::UnexpectedHostClaim,
                        });
                        return Ok(());
                    }
                }
                let mut grew = false;
                match self.players.iter_mut().find(|p| p.unique_id == unique_id) {
                    // A peer we already know from `IPT_PLAYERLIST` has no name
                    // yet — the roster packet carries ids only. Fill it in.
                    Some(p) => {
                        if (!p.name.is_empty() && p.name != player_name) || p.is_host != is_hosting
                        {
                            self.events.push(Event::SetupRefused {
                                from,
                                reason: SetupRefusal::DuplicateAddConflict { unique_id },
                            });
                            return Ok(());
                        }
                        if p.name.is_empty() {
                            p.name = player_name;
                        }
                        p.is_host = is_hosting;
                    }
                    None => {
                        let slot = self.players.len();
                        self.players.push(Player {
                            unique_id,
                            name: player_name,
                            is_host: is_hosting,
                            is_local: false,
                            ready: false,
                            last_pulse_ms: self.now_ms,
                            slot,
                        });
                        self.events.push(Event::PlayerJoined(unique_id));
                        grew = true;
                    }
                }
                if grew {
                    self.republish_ready()?;
                }
                // `send_playerlist()` — the host is the only authority on the
                // roster, so it answers every announcement with the full list.
                if self.role == Role::Host {
                    let mut ids = [0i32; MAX_PLAYERS];
                    let n = self.players.len().min(MAX_PLAYERS);
                    for (i, p) in self.players.iter().take(n).enumerate() {
                        ids[i] = p.unique_id;
                    }
                    self.emit_all(&InternalPacket::PlayerList {
                        num_players: n as u8,
                        unique_ids: ids,
                    })?;
                    // Re-announce ourselves so a late joiner learns the host's
                    // name, not just its id.
                    let me = self.local_id();
                    let name = self.local_name.clone();
                    self.emit_all(&InternalPacket::AddPlayer {
                        player_name: name,
                        unique_id: me,
                        is_hosting: true,
                    })?;
                }
                Ok(())
            }
            // `process_playerlist`
            InternalPacket::PlayerList {
                num_players,
                unique_ids,
            } => {
                if self.role == Role::Host {
                    self.events.push(Event::SetupRefused {
                        from,
                        reason: SetupRefusal::PlayerListFromNonHost,
                    });
                    return Ok(());
                }
                if let Some(expected) = self
                    .players
                    .iter()
                    .find(|player| player.is_host)
                    .map(|player| player.unique_id)
                {
                    if expected != from {
                        self.events.push(Event::SetupRefused {
                            from,
                            reason: SetupRefusal::PlayerListHostMismatch { expected },
                        });
                        return Ok(());
                    }
                }
                let n = num_players as usize;
                if unique_ids[0] != from {
                    self.events.push(Event::SetupRefused {
                        from,
                        reason: SetupRefusal::HostMissingFromSlotZero,
                    });
                    return Ok(());
                }
                if !unique_ids[..n].contains(&self.local_id()) {
                    self.events.push(Event::SetupRefused {
                        from,
                        reason: SetupRefusal::LocalMissingFromRoster,
                    });
                    return Ok(());
                }
                let mut grew = false;
                for (slot, &id) in unique_ids.iter().take(n).enumerate() {
                    if id == 0 {
                        continue;
                    }
                    match self.players.iter_mut().find(|p| p.unique_id == id) {
                        Some(p) => {
                            p.slot = slot;
                            p.is_host = id == from;
                        }
                        None => {
                            self.players.push(Player {
                                unique_id: id,
                                name: String::new(),
                                is_host: id == from,
                                is_local: id == self.transport.local_id(),
                                ready: false,
                                last_pulse_ms: self.now_ms,
                                slot,
                            });
                            self.events.push(Event::PlayerJoined(id));
                            grew = true;
                        }
                    }
                }
                if grew {
                    self.republish_ready()?;
                }
                let keep: Vec<i32> = unique_ids[..n].to_vec();
                let dropped: Vec<i32> = self
                    .players
                    .iter()
                    .filter(|p| !p.is_local && !keep.contains(&p.unique_id))
                    .map(|p| p.unique_id)
                    .collect();
                for id in dropped {
                    self.players.retain(|p| p.unique_id != id);
                    self.announced_to.retain(|peer| *peer != id);
                    self.events.push(Event::PlayerLeft(id));
                }
                self.players.sort_by_key(|p| p.slot);
                Ok(())
            }
            // `process_ready_flag`
            InternalPacket::ReadyFlag { ready } => {
                if let Some(p) = self.players.iter_mut().find(|p| p.unique_id == from) {
                    if p.ready != ready {
                        p.ready = ready;
                        self.events.push(Event::ReadyChanged {
                            unique_id: from,
                            ready,
                        });
                    }
                } else {
                    self.events.push(Event::SetupRefused {
                        from,
                        reason: SetupRefusal::ReadyBeforeAdd,
                    });
                }
                Ok(())
            }
            // `process_pulse`
            InternalPacket::Pulse => {
                if let Some(p) = self.players.iter_mut().find(|p| p.unique_id == from) {
                    p.last_pulse_ms = self.now_ms;
                }
                Ok(())
            }
            // `process_dsync`
            InternalPacket::Dsync { frame } => {
                self.events.push(Event::Desync {
                    unique_id: from,
                    frame,
                });
                Ok(())
            }
            // `process_destroy_player_message`
            InternalPacket::DestroyPlayer { unique_id } => {
                if unique_id != from {
                    self.events.push(Event::SetupRefused {
                        from,
                        reason: SetupRefusal::DestroySenderMismatch {
                            announced: unique_id,
                        },
                    });
                    return Ok(());
                }
                // This id may reconnect on a fresh TCP channel. Forget the
                // previous greeting even if another authoritative roster
                // transition already removed its player object.
                self.announced_to.retain(|peer| *peer != unique_id);
                if self.players.iter().any(|p| p.unique_id == unique_id) {
                    self.players.retain(|p| p.unique_id != unique_id);
                    self.events.push(Event::PlayerLeft(unique_id));
                }
                Ok(())
            }
            // `process_host_migrate_message`
            InternalPacket::MigrateHost { new_host } => {
                for p in self.players.iter_mut() {
                    p.is_host = p.unique_id == new_host;
                }
                if new_host == self.transport.local_id() {
                    self.role = Role::Host;
                }
                Ok(())
            }
            // Drop requests are surfaced as nothing yet; `DropControl` in the
            // engine owns the vote, and we do not model the vote here.
            InternalPacket::DropRequest { .. } | InternalPacket::CancelDropRequest { .. } => Ok(()),
        }
    }

    fn handle_game(&mut self, d: Datagram) -> io::Result<()> {
        let Ok(f) = NetMsg::decode(&d.bytes) else {
            return Ok(());
        };
        if let NetMsg::CommandPackage {
            stamp,
            play,
            payload,
        } = f.msg
        {
            self.turns.entry(stamp).or_default().insert(
                play,
                TurnPackage {
                    stamp,
                    play,
                    payload: payload.to_vec(),
                },
            );
        }
        self.events.push(Event::Game {
            from: d.from,
            msg: OwnedMsg {
                id: f.ty.id,
                response: f.ty.response,
                bytes: d.bytes,
            },
        });
        Ok(())
    }

    fn reap_timeouts(&mut self) {
        let now = self.now_ms;
        let to = self.timeout_ms;
        let dropped: Vec<i32> = self
            .players
            .iter()
            .filter(|p| !p.is_local && now.saturating_sub(p.last_pulse_ms) > to)
            .map(|p| p.unique_id)
            .collect();
        for id in dropped {
            self.players.retain(|p| p.unique_id != id);
            self.announced_to.retain(|peer| *peer != id);
            self.events.push(Event::PlayerLeft(id));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::LoopTransport;

    /// Drive two loopback sessions to a settled state.
    fn settle(
        a: &mut Session<LoopTransport>,
        b: &mut Session<LoopTransport>,
        now: &mut u64,
        rounds: usize,
    ) {
        for _ in 0..rounds {
            a.poll(*now, Duration::ZERO).unwrap();
            b.poll(*now, Duration::ZERO).unwrap();
            LoopTransport::pump(&mut a.transport, &mut b.transport);
            *now += 10;
        }
    }

    fn pair() -> (Session<LoopTransport>, Session<LoopTransport>) {
        let mut ta = LoopTransport::new(101);
        let mut tb = LoopTransport::new(202);
        ta.connect(202);
        tb.connect(101);
        (
            Session::new(ta, Role::Host, "host"),
            Session::new(tb, Role::Client, "client"),
        )
    }

    #[test]
    fn two_peers_discover_each_other_and_agree_on_the_roster() {
        let (mut host, mut client) = pair();
        let mut now = 0u64;
        settle(&mut host, &mut client, &mut now, 8);

        assert_eq!(host.players().len(), 2, "host roster: {:?}", host.players());
        assert_eq!(
            client.players().len(),
            2,
            "client roster: {:?}",
            client.players()
        );
        assert!(host.find(202).is_some());
        assert!(client.find(101).is_some());
        assert_eq!(client.find(101).unwrap().is_host, true);
        // slots agree
        let hs: Vec<(i32, usize)> = host
            .players()
            .iter()
            .map(|p| (p.unique_id, p.slot))
            .collect();
        let cs: Vec<(i32, usize)> = client
            .players()
            .iter()
            .map(|p| (p.unique_id, p.slot))
            .collect();
        assert_eq!(hs, cs);
    }

    #[test]
    fn readiness_handshake_reaches_all_ready_on_both_sides() {
        let (mut host, mut client) = pair();
        let mut now = 0u64;
        settle(&mut host, &mut client, &mut now, 8);
        host.drain_events();
        client.drain_events();

        host.send_ready_flag(true).unwrap();
        settle(&mut host, &mut client, &mut now, 4);
        assert!(!host.all_ready(), "one side ready is not all ready");

        client.send_ready_flag(true).unwrap();
        settle(&mut host, &mut client, &mut now, 4);

        assert!(host.all_ready());
        assert!(client.all_ready());
        assert!(host.drain_events().contains(&Event::AllReady));
        assert!(client.drain_events().contains(&Event::AllReady));

        // reset_ready_flags clears both sides again
        host.reset_ready_flags().unwrap();
        settle(&mut host, &mut client, &mut now, 4);
        assert!(!host.all_ready());
        assert!(!client.all_ready());
    }

    #[test]
    fn reset_for_reuse_preserves_transport_identity_and_clears_the_attempt() {
        let (mut host, mut client) = pair();
        let mut now = 0u64;
        settle(&mut host, &mut client, &mut now, 8);
        host.send_ready_flag(true).unwrap();
        client.send_command_package(77, 1, &[0x39, 0xaa]).unwrap();
        settle(&mut host, &mut client, &mut now, 3);
        assert_eq!(host.players().len(), 2);
        assert!(!host.drain_events().is_empty());
        assert!(host.package_for_turn(77, 1).is_some());

        let local_id = host.local_id();
        host.reset_for_reuse(Role::Client);

        assert_eq!(host.local_id(), local_id);
        assert_eq!(host.role, Role::Client);
        assert_eq!(host.players().len(), 1);
        let local = &host.players()[0];
        assert_eq!(local.unique_id, local_id);
        assert_eq!(local.name, "host");
        assert!(local.is_local);
        assert!(!local.is_host);
        assert!(!local.ready);
        assert_eq!(local.slot, 0);
        assert!(host.drain_events().is_empty());
        assert!(host.package_for_turn(77, 1).is_none());
        assert!(!host.all_ready());
    }

    #[test]
    fn peers_hand_each_other_turns_and_the_lockstep_gate_holds() {
        let (mut host, mut client) = pair();
        let mut now = 0u64;
        settle(&mut host, &mut client, &mut now, 8);
        host.drain_events();
        client.drain_events();

        for stamp in 0u32..5 {
            host.send_command_package(stamp, 0, &[0x39, stamp as u8])
                .unwrap();
            // Before the client's package arrives the host must NOT be able to
            // advance: that is the whole point of lockstep.
            assert!(!host.turn_ready(stamp), "host advanced on its own package");
            client
                .send_command_package(stamp, 1, &[0x39, (stamp + 100) as u8])
                .unwrap();
            settle(&mut host, &mut client, &mut now, 3);

            assert!(host.turn_ready(stamp), "host missing a package for {stamp}");
            assert!(
                client.turn_ready(stamp),
                "client missing a package for {stamp}"
            );
            let ht = host.take_turn(stamp);
            let ct = client.take_turn(stamp);
            assert_eq!(ht, ct, "the two peers disagree about turn {stamp}");
            assert_eq!(ht.len(), 2);
            assert_eq!(ht[0].payload, vec![0x39, stamp as u8]);
            assert_eq!(ht[1].payload, vec![0x39, (stamp + 100) as u8]);
        }
    }

    /// Regression: readiness must be level-triggered.
    ///
    /// A peer that sets its ready flag *before* the other side has added it to
    /// the roster used to deadlock — `process_ready_flag` resolves the sender
    /// to a known player and drops the packet otherwise, and nothing retried.
    /// Two TCP peers hung for the full 15 s timeout on this. The session now
    /// republishes the flag whenever the roster grows.
    #[test]
    fn a_ready_flag_sent_before_the_peer_knows_us_still_converges() {
        let mut ta = LoopTransport::new(101);
        let mut tb = LoopTransport::new(202);
        ta.connect(202);
        tb.connect(101);
        let mut host = Session::new(ta, Role::Host, "host");
        let mut client = Session::new(tb, Role::Client, "client");

        // The client readies immediately — before a single packet has moved,
        // so the flag is on the wire ahead of its own IPT_ADDPLAYER.
        client.send_ready_flag(true).unwrap();
        host.send_ready_flag(true).unwrap();

        let mut now = 0u64;
        settle(&mut host, &mut client, &mut now, 10);

        assert_eq!(host.players().len(), 2);
        assert!(host.all_ready(), "host roster: {:?}", host.players());
        assert!(client.all_ready(), "client roster: {:?}", client.players());
    }

    #[test]
    fn a_silent_peer_times_out() {
        let (mut host, mut client) = pair();
        let mut now = 0u64;
        settle(&mut host, &mut client, &mut now, 8);
        host.set_timeout_ms(100);
        host.drain_events();
        // Advance the host's clock without letting the client pulse.
        for _ in 0..5 {
            now += 100;
            host.poll(now, Duration::ZERO).unwrap();
        }
        assert_eq!(host.players().len(), 1);
        assert!(host.drain_events().contains(&Event::PlayerLeft(202)));
    }

    #[test]
    fn desync_reports_surface_as_events() {
        let (mut host, mut client) = pair();
        let mut now = 0u64;
        settle(&mut host, &mut client, &mut now, 8);
        host.drain_events();
        client.send_dsync(4242).unwrap();
        settle(&mut host, &mut client, &mut now, 3);
        assert!(host.drain_events().contains(&Event::Desync {
            unique_id: 202,
            frame: 4242
        }));
    }

    #[test]
    fn normal_speed_is_sixty_seven_milliseconds() {
        assert_eq!(TURN_TIMINGS_MS[SPEED_NORMAL], 67);
    }

    #[test]
    fn the_host_announced_match_key_reaches_the_client_and_never_the_game_layer() {
        let (mut host, mut client) = pair();
        let mut now = 0u64;
        settle(&mut host, &mut client, &mut now, 8);
        client.drain_events();
        assert_eq!(client.announced_game_key(), None);

        host.announce_game_key(0x005a_c33d, GameKeySource::RetailGameInfoSeed)
            .unwrap();
        settle(&mut host, &mut client, &mut now, 3);
        let events = client.drain_events();
        assert_eq!(
            client.announced_game_key(),
            Some(AnnouncedGameKey {
                from: 101,
                seed: 0x005a_c33d,
                source: GameKeySource::RetailGameInfoSeed,
            })
        );
        assert!(events.contains(&Event::GameKeyAnnounced {
            from: 101,
            seed: 0x005a_c33d,
            source: GameKeySource::RetailGameInfoSeed,
        }));
        assert!(
            !events.iter().any(|e| matches!(e, Event::Game { .. })),
            "an extension packet must never surface as game traffic: {events:?}"
        );

        // Restating the identical key is how a late peer is told; it must not
        // look like news.
        host.announce_game_key(0x005a_c33d, GameKeySource::RetailGameInfoSeed)
            .unwrap();
        settle(&mut host, &mut client, &mut now, 3);
        assert!(client.drain_events().is_empty());

        // A different key is reported, not swallowed, so a consumer can refuse.
        host.announce_game_key(0x005a_c33e, GameKeySource::RetailGameInfoSeed)
            .unwrap();
        settle(&mut host, &mut client, &mut now, 3);
        assert!(client.drain_events().contains(&Event::GameKeyAnnounced {
            from: 101,
            seed: 0x005a_c33e,
            source: GameKeySource::RetailGameInfoSeed,
        }));
    }

    #[test]
    fn explicit_match_start_requires_the_host_and_the_complete_ready_roster() {
        let (mut host, mut client) = pair();
        let mut now = 0u64;
        settle(&mut host, &mut client, &mut now, 8);

        assert!(matches!(
            host.start_match(1, 0x0d0a_11ce),
            Err(MatchStartError::NotAllReady)
        ));
        assert!(matches!(
            client.start_match(1, 0x0d0a_11ce),
            Err(MatchStartError::NotHost)
        ));

        host.send_ready_flag(true).unwrap();
        client.send_ready_flag(true).unwrap();
        settle(&mut host, &mut client, &mut now, 4);
        host.drain_events();
        client.drain_events();

        let started = host.start_match(7, 0x0d0a_11ce).unwrap();
        assert_eq!(host.announced_match_start(), Some(started));
        settle(&mut host, &mut client, &mut now, 3);
        assert_eq!(client.announced_match_start(), Some(started));
        assert!(client
            .drain_events()
            .contains(&Event::MatchStarted(started)));

        // Repeating the exact transaction is idempotent; changing its
        // identity after start is a local refusal rather than a second match.
        assert_eq!(host.start_match(7, 0x0d0a_11ce).unwrap(), started);
        assert!(matches!(
            host.start_match(8, 0x0d0a_11ce),
            Err(MatchStartError::AlreadyStarted { .. })
        ));
    }

    #[test]
    fn a_non_host_match_start_is_consumed_and_refused() {
        let (mut host, mut client) = pair();
        let mut now = 0u64;
        settle(&mut host, &mut client, &mut now, 8);
        host.send_ready_flag(true).unwrap();
        client.send_ready_flag(true).unwrap();
        settle(&mut host, &mut client, &mut now, 4);
        host.drain_events();

        let mut bytes = Vec::new();
        DonExtension::MatchStart {
            epoch: 9,
            seed: 0xdead_beef,
        }
        .encode(&mut bytes);
        client.transport.send(Dest::All, &bytes).unwrap();
        settle(&mut host, &mut client, &mut now, 3);

        assert_eq!(host.announced_match_start(), None);
        assert!(host.drain_events().contains(&Event::SetupRefused {
            from: 202,
            reason: SetupRefusal::MatchStartFromNonHost {
                expected_host: Some(101),
            },
        }));
    }

    #[test]
    fn a_match_key_from_anyone_but_the_host_is_refused() {
        let (mut host, mut client) = pair();
        let mut now = 0u64;
        settle(&mut host, &mut client, &mut now, 8);
        host.drain_events();

        // The client is not the host; the host must not adopt its key.
        client
            .announce_game_key(0xdead_beef, GameKeySource::Configured)
            .unwrap();
        settle(&mut host, &mut client, &mut now, 3);
        assert_eq!(host.announced_game_key(), None);
        assert!(host.drain_events().contains(&Event::SetupRefused {
            from: 202,
            reason: SetupRefusal::GameKeyFromNonHost {
                expected_host: Some(101)
            },
        }));
    }

    #[test]
    fn a_malformed_extension_is_consumed_without_touching_membership() {
        let (mut host, mut client) = pair();
        let mut now = 0u64;
        settle(&mut host, &mut client, &mut now, 8);
        client.drain_events();
        let before = client.players().to_vec();

        // Right id, wrong length: fail closed rather than guess the seed.
        host.transport
            .send(Dest::All, &[crate::extension::DON_EXT_GAMEKEY, 1, 2, 3])
            .unwrap();
        settle(&mut host, &mut client, &mut now, 3);
        assert_eq!(client.announced_game_key(), None);
        assert_eq!(client.players(), before.as_slice());
        assert!(client.drain_events().iter().any(|event| matches!(
            event,
            Event::SetupRefused {
                reason: SetupRefusal::MalformedExtension(_),
                ..
            }
        )));
    }
}
