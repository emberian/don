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

use crate::internal::{InternalPacket, MAX_PLAYERS};
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
    ReadyChanged { unique_id: i32, ready: bool },
    /// A peer reported a desync at `frame` (`IPT_DSYNCMSG`).
    Desync { unique_id: i32, frame: i32 },
    /// A game-layer message arrived. `from` is the sender's unique id.
    Game { from: i32, msg: OwnedMsg },
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
            announced: false,
            all_ready_fired: false,
            now_ms: 0,
            last_pulse_sent_ms: 0,
            pulse_interval_ms: 1000,
            timeout_ms: 30_000,
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
    pub fn send_command_package(
        &mut self,
        stamp: u32,
        play: i8,
        payload: &[u8],
    ) -> io::Result<()> {
        self.turns.entry(stamp).or_default().insert(
            play,
            TurnPackage { stamp, play, payload: payload.to_vec() },
        );
        self.send_all(&NetMsg::CommandPackage { stamp, play, payload })
    }

    /// Have we got a package from every player for this stamp? This is the
    /// lockstep gate: `CommandManager::process_turn` `0x0093EF10` will not
    /// advance without one package per participating slot.
    pub fn turn_ready(&self, stamp: u32) -> bool {
        let want = self.players.len();
        self.turns.get(&stamp).map(|m| m.len() >= want).unwrap_or(false)
    }

    /// Take every package for a stamp, in slot order.
    pub fn take_turn(&mut self, stamp: u32) -> Vec<TurnPackage> {
        self.turns.remove(&stamp).map(|m| m.into_values().collect()).unwrap_or_default()
    }

    pub fn drain_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// Advance the session. `now_ms` is a monotonic millisecond clock supplied
    /// by the caller — the crate has no dependencies and takes no opinion on
    /// where time comes from.
    pub fn poll(&mut self, now_ms: u64, budget: Duration) -> io::Result<()> {
        self.now_ms = now_ms;

        // A client announces itself once the transport reports a peer, which is
        // the earliest moment `IPT_ADDPLAYER` can be delivered.
        if !self.announced && !self.transport.peers().is_empty() {
            let me = self.local_id();
            let name = self.local_name.clone();
            self.emit_all(&InternalPacket::AddPlayer {
                player_name: name,
                unique_id: me,
                is_hosting: self.role == Role::Host,
            })?;
            self.announced = true;
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

    fn handle(&mut self, d: Datagram) -> io::Result<()> {
        let Some(&first) = d.bytes.first() else { return Ok(()) };
        if first >= crate::internal::IPT_BASE {
            let Ok(p) = InternalPacket::decode(&d.bytes) else { return Ok(()) };
            self.handle_internal(d.from, p)
        } else {
            self.handle_game(d)
        }
    }

    fn handle_internal(&mut self, from: i32, p: InternalPacket) -> io::Result<()> {
        match p {
            // `process_create_player_message`
            InternalPacket::AddPlayer { player_name, unique_id, is_hosting } => {
                if self.find(unique_id).is_none() {
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
            InternalPacket::PlayerList { num_players, unique_ids } => {
                if self.role == Role::Host {
                    return Ok(()); // host-authoritative; ignore inbound lists
                }
                let n = (num_players as usize).min(MAX_PLAYERS);
                for (slot, &id) in unique_ids.iter().take(n).enumerate() {
                    if id == 0 {
                        continue;
                    }
                    match self.players.iter_mut().find(|p| p.unique_id == id) {
                        Some(p) => p.slot = slot,
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
                        }
                    }
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
                        self.events.push(Event::ReadyChanged { unique_id: from, ready });
                    }
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
                self.events.push(Event::Desync { unique_id: from, frame });
                Ok(())
            }
            // `process_destroy_player_message`
            InternalPacket::DestroyPlayer { unique_id } => {
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
            InternalPacket::DropRequest { .. } | InternalPacket::CancelDropRequest { .. } => {
                Ok(())
            }
        }
    }

    fn handle_game(&mut self, d: Datagram) -> io::Result<()> {
        let Ok(f) = NetMsg::decode(&d.bytes) else { return Ok(()) };
        if let NetMsg::CommandPackage { stamp, play, payload } = f.msg {
            self.turns.entry(stamp).or_default().insert(
                play,
                TurnPackage { stamp, play, payload: payload.to_vec() },
            );
        }
        self.events.push(Event::Game {
            from: d.from,
            msg: OwnedMsg { id: f.ty.id, response: f.ty.response, bytes: d.bytes },
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
        assert_eq!(client.players().len(), 2, "client roster: {:?}", client.players());
        assert!(host.find(202).is_some());
        assert!(client.find(101).is_some());
        assert_eq!(client.find(101).unwrap().is_host, true);
        // slots agree
        let hs: Vec<(i32, usize)> = host.players().iter().map(|p| (p.unique_id, p.slot)).collect();
        let cs: Vec<(i32, usize)> =
            client.players().iter().map(|p| (p.unique_id, p.slot)).collect();
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
    fn peers_hand_each_other_turns_and_the_lockstep_gate_holds() {
        let (mut host, mut client) = pair();
        let mut now = 0u64;
        settle(&mut host, &mut client, &mut now, 8);
        host.drain_events();
        client.drain_events();

        for stamp in 0u32..5 {
            host.send_command_package(stamp, 0, &[0x39, stamp as u8]).unwrap();
            // Before the client's package arrives the host must NOT be able to
            // advance: that is the whole point of lockstep.
            assert!(!host.turn_ready(stamp), "host advanced on its own package");
            client.send_command_package(stamp, 1, &[0x39, (stamp + 100) as u8]).unwrap();
            settle(&mut host, &mut client, &mut now, 3);

            assert!(host.turn_ready(stamp), "host missing a package for {stamp}");
            assert!(client.turn_ready(stamp), "client missing a package for {stamp}");
            let ht = host.take_turn(stamp);
            let ct = client.take_turn(stamp);
            assert_eq!(ht, ct, "the two peers disagree about turn {stamp}");
            assert_eq!(ht.len(), 2);
            assert_eq!(ht[0].payload, vec![0x39, stamp as u8]);
            assert_eq!(ht[1].payload, vec![0x39, (stamp + 100) as u8]);
        }
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
        assert!(host
            .drain_events()
            .contains(&Event::Desync { unique_id: 202, frame: 4242 }));
    }

    #[test]
    fn normal_speed_is_sixty_seven_milliseconds() {
        assert_eq!(TURN_TIMINGS_MS[SPEED_NORMAL], 67);
    }
}
