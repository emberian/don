// SPDX-License-Identifier: GPL-3.0-or-later
//! The DoN-owned lobby/session/P2P backend that sits behind
//! `CrossplayProxy::ICrossPlayService`.
//!
//! # What this is, and what it is not
//!
//! The *interface* in [`crate::abi`] is retail's, measured slot by slot from the
//! shipped private PDBs and the shipped `.rdata` vtable. The *semantics* in this
//! module are **DoN's own**. There is no PlayFab account, no title id, no
//! WinHTTP request, no Party network and no TURN server anywhere in this file.
//! A lobby here is a record in a [`Directory`] that DoN owns outright.
//!
//! So the honesty rule for this module is inverted relative to the ABI crate:
//! nothing here is a claim about how Q-LOC's service behaved. Where a retail
//! behaviour *was* measured, it is reproduced and cited inline. Everything else
//! is a DoN policy decision, marked **[DoN policy]**, chosen to satisfy the way
//! the shipped game is observed to call the interface.
//!
//! ## Measured retail behaviour reproduced here
//!
//! * The session state machine. `CrossPlayService::StartSession`
//!   (`CrossplayProxy.dll` RVA `0x14670`) writes `1` (`Starting`) to the status
//!   field at `+0x930` synchronously at RVA `0x146ba`; its asynchronous
//!   completion lambda writes `2` (`Started`) at RVA `0x14b51` on success and
//!   `0` (`Stopped`) at RVA `0x1492b` on failure. `StopSession` (RVA `0x14d60`)
//!   returns immediately when the status is already `Stopped` or `Stopping`
//!   (RVA `0x14d73`/`0x14d7b`), otherwise writes `3` (`Stopping`) at RVA
//!   `0x14d84` and `0` at RVA `0x14f3e`. **[measured]**
//! * `IsConnectedToHub` is exactly `status == Started` — the shipped body at
//!   vtable `+212` is `cmp [ecx+0x930], 2; sete al`. **[measured]**
//! * `IsUserBlocked` is a constant `false` (`xor al, al; ret 4`, RVA `0x14f70`),
//!   and the whole blocking family is inert. **[measured]**
//! * Lobby attributes travel as the `unordered_map` parameter of `CreateLobby`
//!   and `UpdateLobby`; this build has no `SetLobbyAttribute`,
//!   `SetLobbyAttributes` or `SetLobbyMaxMemberCount` at all. **[measured —
//!   `docs/tracks/crossplay-abi.md` §3.6]**
//!
//! ## Why every request completes on `Tick`
//!
//! Every lobby entry point in the shipped interface takes an ok/error callback
//! pair rather than returning a result, and `Tick` is pumped from
//! `NetDaemon::process_all` (**[measured]** — `gen/slot_usage.py`). The game is
//! therefore written against an asynchronous service. A local backend *could*
//! answer synchronously inside the call, but that would let the game depend on
//! re-entrancy retail never offered. So requests are queued and drained by
//! [`Backend::tick`], preserving submission order.
//!
//! ## The attribute schema is not re-derived here
//!
//! `crates/don-net/src/lobby.rs` already recovered the key schema that travels
//! in this map — `game_seed`, `steam_ready_<slot>`, the 26 settings keys and the
//! shipped `"echowin"` misspelling. This module treats attributes as opaque
//! string pairs and never interprets them, so there is exactly one owner of that
//! schema and it is not this file.

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::abi::{
    SESSION_STATUS_STARTED, SESSION_STATUS_STARTING, SESSION_STATUS_STOPPED,
    SESSION_STATUS_STOPPING, VISIBILITY_PUBLIC,
};

/// A lobby attribute map: `unordered_map<wstring, wstring>` on the wire, an
/// ordered map here so iteration order is deterministic.
pub type Attributes = BTreeMap<String, String>;

/// Error codes handed to the callbacks that take an `int`
/// (`JoinLobby`'s error callback is `void(int, const wstring&)` and
/// `LeaveLobby`'s is `void(int)`).
///
/// **[DoN policy — not derived.]** Nothing was measured about which integers
/// the shipped service produced, nor whether `riseofnations.exe` branches on
/// specific values; `gen/slot_usage.py` finds no tracked dispatch of either
/// callback installer. These are DoN's own codes, kept negative so they cannot
/// collide with an HTTP status or a PlayFab error number if this is ever
/// compared against a capture.
pub mod error {
    /// The service has not had `Init` called on it.
    pub const NOT_INITIALIZED: i32 = -1;
    /// No session is running, so lobby operations are refused.
    pub const NO_SESSION: i32 = -2;
    /// No lobby with that id exists in the directory.
    pub const NOT_FOUND: i32 = -3;
    /// The lobby has no free slot.
    pub const FULL: i32 = -4;
    /// This player is already in a lobby.
    pub const ALREADY_JOINED: i32 = -5;
    /// This player is not a member of the lobby it named.
    pub const NOT_A_MEMBER: i32 = -6;
    /// This player does not own the lobby it tried to mutate.
    pub const NOT_OWNER: i32 = -7;
    /// The lobby's match has already been started.
    pub const GAME_ALREADY_STARTED: i32 = -8;
    /// The slot exists in the ABI but this backend deliberately does not
    /// implement it. The fail-closed code.
    pub const NOT_AVAILABLE: i32 = -9;
    /// A caller-supplied argument could not be read at the MSVC boundary.
    pub const BAD_ARGUMENT: i32 = -10;
}

/// One lobby member. Mirrors `Crossplay::Lobby::DTO::LobbyMemberDTO`, which is
/// `UserDTO` with nothing added — four `wstring`s. **[measured]**
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Member {
    pub user_id: String,
    pub user_name: String,
    pub platform: String,
    pub platform_account_id: String,
}

impl Member {
    /// A member identified only by id, which is all the transport needs.
    pub fn new(user_id: &str, user_name: &str) -> Self {
        Self {
            user_id: user_id.to_string(),
            user_name: user_name.to_string(),
            platform: "don".to_string(),
            platform_account_id: user_id.to_string(),
        }
    }
}

/// A lobby record. Field-for-field the data half of
/// `Crossplay::Lobby::DTO::LobbyDTO`, minus `_turnServer` — DoN peers talk
/// directly and there is no TURN relay to describe, so the outbound DTO carries
/// an empty `TurnServerDTO`. **[DoN policy]**
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lobby {
    pub id: String,
    pub owner_user_id: String,
    /// `_sessionReference`. DoN uses it for what `StartGame` publishes, which is
    /// how a joined peer learns the match has begun. **[DoN policy]**
    pub session_reference: String,
    pub max_members: i32,
    pub bot_count: i32,
    /// Bumped on every accepted `UpdateLobby`, so a polling peer can tell
    /// whether it has already seen this attribute generation. **[DoN policy]**
    pub attribute_version: i32,
    pub visibility: i32,
    pub members: Vec<Member>,
    pub attributes: Attributes,
    /// Set by `StartGame`, cleared by `CancelGameStart`.
    pub game_started: bool,
}

impl Lobby {
    /// `_availableSlots`. **[DoN policy]** — the shipped computation was not
    /// derived. DoN defines it as the seats a further human could take:
    /// `max_members - bot_count - members`, floored at zero. It is what
    /// `LobbySearchCriteriaDTO::_minAvailableSlots` filters against in
    /// [`Directory::find`], so the two definitions cannot drift apart.
    pub fn available_slots(&self) -> i32 {
        (self.max_members - self.bot_count - self.members.len() as i32).max(0)
    }

    pub fn is_member(&self, user_id: &str) -> bool {
        self.members.iter().any(|m| m.user_id == user_id)
    }
}

/// The outcome of one `UpdateLobby`, matching `UpdateLobbyResultDTO`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateResult {
    pub success: bool,
    pub lobby: Lobby,
    pub timestamp: i64,
}

/// A request handle. The MSVC layer keys the caller's `std::function` pair on
/// this, so a completion can find the callbacks that belong to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReqId(pub u64);

/// What a queued request resolved to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// `StartSession` succeeded; carries the entity/player id the service will
    /// report from `GetPlayerGuid`.
    SessionStarted(String),
    /// A single lobby: `GetLobby`, `CreateLobby`, `JoinLobby`.
    Lobby(Lobby),
    /// `FindLobbies`.
    Lobbies(Vec<Lobby>),
    /// `UpdateLobby`.
    Updated(UpdateResult),
    /// `StartGame` / `CancelGameStart` succeeded; carries the session
    /// reference the shipped signature returns as `const wstring&`.
    SessionReference(String),
    /// `LeaveLobby` succeeded; its ok callback is `void()`.
    Done,
    /// Any failure. `code` is from [`error`], `message` is human text.
    Failed { code: i32, message: String },
}

impl Outcome {
    pub fn is_error(&self) -> bool {
        matches!(self, Outcome::Failed { .. })
    }
}

/// Something that happened without a request behind it: the P2P event stream.
///
/// The three lobby broadcast callbacks (`SetJoinLobbyCallback`,
/// `SetLeaveLobbyCallback`, `SetUpdateLobbyCallback`) are deliberately **not**
/// represented here. See [`Backend::tick`] for why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// A peer became reachable — drives `SetP2PConnectionOpenedCallback` and
    /// then `SetP2PDataChannelOpenedCallback`.
    PeerOpened(String),
    /// A peer went away — drives the two closed callbacks.
    PeerClosed(String),
    /// Bytes from a peer, for `SetReceivedP2PDataCallback`.
    Data { from: String, bytes: Vec<u8> },
    /// A `P2PSendToAll`/`P2PSend` text payload, for
    /// `SetReceivedP2PTextCallback`.
    Text { from: String, text: String },
}

/// One drained item, in submission order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Emission {
    Completion(ReqId, Outcome),
    Notice(Notice),
}

// ---------------------------------------------------------------------------
// The directory: the shared world every local service instance talks to.
// ---------------------------------------------------------------------------

/// The set of lobbies and peer mailboxes DoN owns.
///
/// This is the piece a real deployment replaces: a process-local `Directory` is
/// a complete, correct lobby service for peers inside one address space, and a
/// networked one is the same operations carried over a transport. Every method
/// is a pure state transition on `&mut self`, so a transport implementation has
/// one obvious thing to replicate.
#[derive(Debug, Default)]
pub struct Directory {
    lobbies: BTreeMap<String, Lobby>,
    mailboxes: BTreeMap<String, VecDeque<Notice>>,
    next_lobby: u64,
}

impl Directory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a peer so it can receive P2P traffic. Idempotent.
    pub fn attach_peer(&mut self, user_id: &str) {
        self.mailboxes.entry(user_id.to_string()).or_default();
    }

    /// Forget a peer and drop anything queued for it.
    pub fn detach_peer(&mut self, user_id: &str) {
        self.mailboxes.remove(user_id);
    }

    pub fn get(&self, id: &str) -> Option<&Lobby> {
        self.lobbies.get(id)
    }

    pub fn lobby_of(&self, user_id: &str) -> Option<&Lobby> {
        self.lobbies.values().find(|l| l.is_member(user_id))
    }

    /// `FindLobbies`. Public lobbies only, filtered by free seats, capped at
    /// `max_results`, ordered by id so the result is deterministic.
    ///
    /// **[DoN policy]** — `ELobbyProximity` is accepted and ignored: DoN has no
    /// geographic notion of a peer, and inventing one would be a fiction.
    pub fn find(&self, max_results: i32, min_available_slots: i32) -> Vec<Lobby> {
        let cap = if max_results <= 0 {
            usize::MAX
        } else {
            max_results as usize
        };
        self.lobbies
            .values()
            .filter(|l| l.visibility == VISIBILITY_PUBLIC)
            .filter(|l| !l.game_started)
            .filter(|l| l.available_slots() >= min_available_slots)
            .take(cap)
            .cloned()
            .collect()
    }

    /// Create a lobby owned by `owner`, who becomes its first member.
    pub fn create(
        &mut self,
        owner: &Member,
        max_members: i32,
        visibility: i32,
        attributes: Attributes,
    ) -> Result<Lobby, (i32, String)> {
        if self.lobby_of(&owner.user_id).is_some() {
            return Err((error::ALREADY_JOINED, "already in a lobby".to_string()));
        }
        if max_members < 1 {
            return Err((error::BAD_ARGUMENT, "max_members < 1".to_string()));
        }
        self.next_lobby += 1;
        let id = base36(self.next_lobby);
        let lobby = Lobby {
            id: id.clone(),
            owner_user_id: owner.user_id.clone(),
            session_reference: String::new(),
            max_members,
            bot_count: 0,
            attribute_version: 1,
            visibility,
            members: alloc::vec![owner.clone()],
            attributes,
            game_started: false,
        };
        self.lobbies.insert(id.clone(), lobby.clone());
        Ok(lobby)
    }

    /// Join an existing lobby. Announces the arrival to the members already in
    /// it and tells the joiner about each of them.
    pub fn join(&mut self, who: &Member, lobby_id: &str) -> Result<Lobby, (i32, String)> {
        if self.lobby_of(&who.user_id).is_some() {
            return Err((error::ALREADY_JOINED, "already in a lobby".to_string()));
        }
        let existing: Vec<String> = {
            let lobby = self
                .lobbies
                .get(lobby_id)
                .ok_or_else(|| (error::NOT_FOUND, "no such lobby".to_string()))?;
            if lobby.game_started {
                return Err((
                    error::GAME_ALREADY_STARTED,
                    "match already started".to_string(),
                ));
            }
            if lobby.available_slots() < 1 {
                return Err((error::FULL, "lobby is full".to_string()));
            }
            lobby.members.iter().map(|m| m.user_id.clone()).collect()
        };
        let lobby = self.lobbies.get_mut(lobby_id).expect("checked above");
        lobby.members.push(who.clone());
        let lobby = lobby.clone();
        for peer in &existing {
            self.post(peer, Notice::PeerOpened(who.user_id.clone()));
            self.post(&who.user_id, Notice::PeerOpened(peer.clone()));
        }
        Ok(lobby)
    }

    /// Leave whatever lobby the player is in. Removing the last member deletes
    /// the lobby; removing the owner hands ownership to the next member.
    /// **[DoN policy]** — retail's owner-migration rule was not derived.
    pub fn leave(&mut self, user_id: &str, lobby_id: &str) -> Result<(), (i32, String)> {
        let lobby = self
            .lobbies
            .get_mut(lobby_id)
            .ok_or_else(|| (error::NOT_FOUND, "no such lobby".to_string()))?;
        if !lobby.is_member(user_id) {
            return Err((error::NOT_A_MEMBER, "not a member".to_string()));
        }
        lobby.members.retain(|m| m.user_id != user_id);
        let remaining: Vec<String> = lobby.members.iter().map(|m| m.user_id.clone()).collect();
        if lobby.members.is_empty() {
            self.lobbies.remove(lobby_id);
        } else if lobby.owner_user_id == user_id {
            lobby.owner_user_id = lobby.members[0].user_id.clone();
        }
        for peer in &remaining {
            self.post(peer, Notice::PeerClosed(user_id.to_string()));
            self.post(user_id, Notice::PeerClosed(peer.clone()));
        }
        Ok(())
    }

    /// Merge attributes into a lobby the caller owns. Keys with an empty value
    /// are **deletions**, which is what makes this usable as the delta publish
    /// `ConnectionData::send_player` performs (see `don-net`'s `lobby.rs`).
    /// **[DoN policy]** — retail's empty-value handling was not derived.
    pub fn update(
        &mut self,
        user_id: &str,
        lobby_id: &str,
        max_members: i32,
        bot_count: i32,
        attributes: &Attributes,
    ) -> Result<Lobby, (i32, String)> {
        let lobby = self
            .lobbies
            .get_mut(lobby_id)
            .ok_or_else(|| (error::NOT_FOUND, "no such lobby".to_string()))?;
        if lobby.owner_user_id != user_id {
            return Err((error::NOT_OWNER, "not the lobby owner".to_string()));
        }
        if max_members > 0 {
            if max_members < lobby.members.len() as i32 {
                return Err((
                    error::BAD_ARGUMENT,
                    "max_members below current membership".to_string(),
                ));
            }
            lobby.max_members = max_members;
        }
        if bot_count >= 0 {
            lobby.bot_count = bot_count;
        }
        for (k, v) in attributes {
            if v.is_empty() {
                lobby.attributes.remove(k);
            } else {
                lobby.attributes.insert(k.clone(), v.clone());
            }
        }
        lobby.attribute_version += 1;
        Ok(lobby.clone())
    }

    /// `StartGame`. Publishes `session_reference` on the lobby so every polling
    /// member sees the match has begun.
    pub fn start_game(
        &mut self,
        user_id: &str,
        lobby_id: &str,
        session_reference: &str,
    ) -> Result<String, (i32, String)> {
        let lobby = self
            .lobbies
            .get_mut(lobby_id)
            .ok_or_else(|| (error::NOT_FOUND, "no such lobby".to_string()))?;
        if lobby.owner_user_id != user_id {
            return Err((error::NOT_OWNER, "not the lobby owner".to_string()));
        }
        if lobby.game_started {
            return Err((
                error::GAME_ALREADY_STARTED,
                "match already started".to_string(),
            ));
        }
        lobby.game_started = true;
        lobby.session_reference = session_reference.to_string();
        Ok(lobby.session_reference.clone())
    }

    pub fn cancel_game_start(
        &mut self,
        user_id: &str,
        lobby_id: &str,
    ) -> Result<String, (i32, String)> {
        let lobby = self
            .lobbies
            .get_mut(lobby_id)
            .ok_or_else(|| (error::NOT_FOUND, "no such lobby".to_string()))?;
        if lobby.owner_user_id != user_id {
            return Err((error::NOT_OWNER, "not the lobby owner".to_string()));
        }
        lobby.game_started = false;
        let reference = core::mem::take(&mut lobby.session_reference);
        Ok(reference)
    }

    /// Queue a notice for a peer. Unknown peers are dropped, not created — a
    /// mailbox exists only for a service instance that attached itself.
    fn post(&mut self, user_id: &str, notice: Notice) {
        if let Some(box_) = self.mailboxes.get_mut(user_id) {
            box_.push_back(notice);
        }
    }

    /// Deliver `bytes` to every other member of `from`'s lobby.
    /// Returns the number of peers it reached.
    pub fn send_to_all(&mut self, from: &str, bytes: &[u8]) -> usize {
        let peers: Vec<String> = match self.lobby_of(from) {
            Some(l) => l
                .members
                .iter()
                .filter(|m| m.user_id != from)
                .map(|m| m.user_id.clone())
                .collect(),
            None => return 0,
        };
        for peer in &peers {
            self.post(
                peer,
                Notice::Data {
                    from: from.to_string(),
                    bytes: bytes.to_vec(),
                },
            );
        }
        peers.len()
    }

    /// Deliver `bytes` to one peer, which must share a lobby with the sender.
    pub fn send_to(&mut self, from: &str, to: &str, bytes: &[u8]) -> bool {
        let shares = self
            .lobby_of(from)
            .map(|l| l.is_member(to) && to != from)
            .unwrap_or(false);
        if !shares {
            return false;
        }
        self.post(
            to,
            Notice::Data {
                from: from.to_string(),
                bytes: bytes.to_vec(),
            },
        );
        true
    }

    fn drain_mailbox(&mut self, user_id: &str) -> Vec<Notice> {
        match self.mailboxes.get_mut(user_id) {
            Some(box_) => box_.drain(..).collect(),
            None => Vec::new(),
        }
    }
}

/// Render a `u64` as lower-case base-36, the same short-identifier spelling
/// `netsys-shim` uses so ids stay inside the seven-code-unit MSVC small-string
/// domain for realistic counts.
fn base36(mut value: u64) -> String {
    if value == 0 {
        return "0".to_string();
    }
    let mut out = Vec::new();
    while value > 0 {
        let digit = (value % 36) as u8;
        out.push(if digit < 10 {
            b'0' + digit
        } else {
            b'a' + digit - 10
        });
        value /= 36;
    }
    out.reverse();
    String::from_utf8(out).expect("base-36 digits are ASCII")
}

// ---------------------------------------------------------------------------
// The per-instance backend.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
enum Request {
    StartSession { user_id: String },
    GetLobby(String),
    FindLobbies { max_results: i32, min_slots: i32 },
    CreateLobby {
        max_members: i32,
        visibility: i32,
        attributes: Attributes,
    },
    JoinLobby(String),
    LeaveLobby(String),
    UpdateLobby {
        lobby_id: String,
        max_members: i32,
        bot_count: i32,
        attributes: Attributes,
    },
    StartGame {
        lobby_id: String,
        session_reference: String,
    },
    CancelGameStart(String),
    /// A slot that exists in the ABI but that this backend refuses. Carries the
    /// slot name so the failure names itself.
    FailClosed(&'static str),
}

/// One local `ICrossPlayService` instance's state.
///
/// This is deliberately *not* the retail `CrossPlayService` object layout. The
/// only field of that 2520-byte object anything outside the DLL reads is the
/// session status at `+0x930`, and it is read exclusively through
/// `GetSessionStatus` and `IsConnectedToHub` — both of which are vtable slots.
/// So nothing constrains this layout, and pretending to reproduce a layout that
/// was never transcribed would be a fiction. **[measured that the two
/// one-line accessors are the only readers; `docs/tracks/crossplay-abi.md`
/// §3.3, §5 item 4]**
#[derive(Debug)]
pub struct Backend {
    initialized: bool,
    status: i32,
    user: Member,
    /// Set once the session starts; `GetPlayerGuid` returns it.
    player_guid: String,
    joined_lobby: Option<String>,
    queue: VecDeque<(ReqId, Request)>,
    ready: VecDeque<Emission>,
    next_req: u64,
    /// Peers we have told the game about via `PeerOpened` and not yet closed.
    open_peers: BTreeSet<String>,
    /// `SetP2PTimeoutDuration`'s argument. Retained and reported; DoN's local
    /// transport has no timeout to apply it to.
    p2p_timeout: i32,
    /// `SetUsername`'s argument, which also becomes the member display name.
    username: String,
    /// `SetReliability`'s argument. The shipped slot is one of the eight
    /// ICF-folded no-ops at RVA `0x145b0`, so retaining it is already more than
    /// retail did. **[measured]**
    reliable: bool,
    /// Errors reported through `SetServiceErrorCallback`, oldest first.
    service_errors: VecDeque<String>,
}

impl Backend {
    /// A service instance for one local user. `user_id` is DoN's own identity
    /// for this peer; there is no account system behind it.
    pub fn new(user_id: &str, user_name: &str) -> Self {
        Self {
            initialized: false,
            status: SESSION_STATUS_STOPPED,
            user: Member::new(user_id, user_name),
            player_guid: String::new(),
            joined_lobby: None,
            queue: VecDeque::new(),
            ready: VecDeque::new(),
            next_req: 0,
            open_peers: BTreeSet::new(),
            p2p_timeout: 0,
            username: user_name.to_string(),
            reliable: true,
            service_errors: VecDeque::new(),
        }
    }

    pub fn user_id(&self) -> &str {
        &self.user.user_id
    }

    /// `GetSessionStatus` (slot 7). The single most widely dispatched slot in
    /// the exe — 17 distinct functions gate on it. **[measured]**
    pub fn session_status(&self) -> i32 {
        self.status
    }

    /// `IsConnectedToHub` (slot 53) is `status == Started`, byte for byte what
    /// the shipped one-liner computes. **[measured]**
    pub fn is_connected_to_hub(&self) -> bool {
        self.status == SESSION_STATUS_STARTED
    }

    /// `GetPlayerGuid` (slot 56), 19 call sites across both images. Empty until
    /// the session reaches `Started`. **[measured that it is dispatched;
    /// DoN policy that it is empty before `Started`]**
    pub fn player_guid(&self) -> &str {
        &self.player_guid
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    pub fn joined_lobby(&self) -> Option<&str> {
        self.joined_lobby.as_deref()
    }

    pub fn p2p_timeout(&self) -> i32 {
        self.p2p_timeout
    }

    pub fn reliable(&self) -> bool {
        self.reliable
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    /// `Init` (slot 0), dispatched from `Main::init`. **[measured]**
    pub fn init(&mut self) {
        self.initialized = true;
    }

    /// `SetReliability` (slot 3) — inert in the shipped build; retained here.
    pub fn set_reliability(&mut self, reliable: bool) {
        self.reliable = reliable;
    }

    /// `SetP2PTimeoutDuration` (slot 47).
    pub fn set_p2p_timeout(&mut self, ms: i32) {
        self.p2p_timeout = ms;
    }

    /// `SetUsername` (slot 55). Also updates the display name this peer
    /// publishes into a lobby it later joins.
    pub fn set_username(&mut self, name: &str) {
        self.username = name.to_string();
        self.user.user_name = name.to_string();
    }

    /// Record an error for `SetServiceErrorCallback` to drain.
    pub fn report_service_error(&mut self, message: &str) {
        self.service_errors.push_back(message.to_string());
    }

    pub fn take_service_error(&mut self) -> Option<String> {
        self.service_errors.pop_front()
    }

    fn submit(&mut self, request: Request) -> ReqId {
        self.next_req += 1;
        let id = ReqId(self.next_req);
        self.queue.push_back((id, request));
        id
    }

    /// `StartSession` (slot 4). Sets `Starting` synchronously, exactly as the
    /// shipped body does at RVA `0x146ba`; the transition to `Started` lands on
    /// the next [`tick`](Self::tick). **[measured]**
    pub fn start_session(&mut self, user_id: &str) -> ReqId {
        self.status = SESSION_STATUS_STARTING;
        self.submit(Request::StartSession {
            user_id: user_id.to_string(),
        })
    }

    /// `StopSession` (slot 5). Synchronous, and a no-op when already `Stopped`
    /// or `Stopping` — the shipped body's first two branches. **[measured]**
    ///
    /// Returns the peers whose connections it closed, so the caller can run the
    /// P2P closed callbacks.
    pub fn stop_session(&mut self, directory: &mut Directory) -> Vec<String> {
        if self.status == SESSION_STATUS_STOPPED || self.status == SESSION_STATUS_STOPPING {
            return Vec::new();
        }
        self.status = SESSION_STATUS_STOPPING;
        let closed = self.p2p_close_all(directory);
        if let Some(id) = self.joined_lobby.take() {
            let _ = directory.leave(&self.user.user_id, &id);
        }
        directory.detach_peer(&self.user.user_id);
        self.queue.clear();
        self.ready.clear();
        self.player_guid.clear();
        self.status = SESSION_STATUS_STOPPED;
        closed
    }

    /// `GetLobby` (slot 12).
    pub fn get_lobby(&mut self, lobby_id: &str) -> ReqId {
        self.submit(Request::GetLobby(lobby_id.to_string()))
    }

    /// `FindLobbies` (slot 13), from `LobbyManager::refresh_lobbies`.
    /// **[measured]**
    pub fn find_lobbies(&mut self, max_results: i32, min_slots: i32) -> ReqId {
        self.submit(Request::FindLobbies {
            max_results,
            min_slots,
        })
    }

    /// `CreateLobby` (slot 14), from `LobbyManager::CreateLobby`.
    /// **[measured]**
    pub fn create_lobby(
        &mut self,
        max_members: i32,
        visibility: i32,
        attributes: Attributes,
    ) -> ReqId {
        self.submit(Request::CreateLobby {
            max_members,
            visibility,
            attributes,
        })
    }

    /// `JoinLobby` (slot 15), from `LobbyManager::JoinLobby`. **[measured]**
    pub fn join_lobby(&mut self, lobby_id: &str) -> ReqId {
        self.submit(Request::JoinLobby(lobby_id.to_string()))
    }

    /// `LeaveLobby` (slot 17).
    pub fn leave_lobby(&mut self, lobby_id: &str) -> ReqId {
        self.submit(Request::LeaveLobby(lobby_id.to_string()))
    }

    /// `UpdateLobby` (slot 19). This is the only way lobby attributes change in
    /// this build — there is no `SetLobbyAttribute` slot. **[measured]**
    pub fn update_lobby(
        &mut self,
        lobby_id: &str,
        max_members: i32,
        bot_count: i32,
        attributes: Attributes,
    ) -> ReqId {
        self.submit(Request::UpdateLobby {
            lobby_id: lobby_id.to_string(),
            max_members,
            bot_count,
            attributes,
        })
    }

    /// `StartGame` (slot 22), reached from `SetupWin::*`. **[measured]**
    pub fn start_game(&mut self, lobby_id: &str, session_reference: &str) -> ReqId {
        self.submit(Request::StartGame {
            lobby_id: lobby_id.to_string(),
            session_reference: session_reference.to_string(),
        })
    }

    /// `CancelGameStart` (slot 23). **[measured]**
    pub fn cancel_game_start(&mut self, lobby_id: &str) -> ReqId {
        self.submit(Request::CancelGameStart(lobby_id.to_string()))
    }

    /// Queue a named refusal for a slot this backend does not implement. The
    /// caller's error callback still runs, on the same `Tick` path as every
    /// other completion, so the game is never left waiting on a request that
    /// will not answer. This is the `LIBERR_NOT_AVAILABLE` discipline
    /// `crates/netsys-shim` uses, transposed to a callback interface.
    pub fn fail_closed(&mut self, slot: &'static str) -> ReqId {
        self.submit(Request::FailClosed(slot))
    }

    /// `P2PSendToAll` (slot 49) — where every turn packet leaves.
    /// `CrossplayNetLibSys::{send,send_pulse,send_playerlist,send_ready_flag,
    /// send_dsync,cancel_join,cancel_drop_player}` all reach it. **[measured]**
    pub fn p2p_send_to_all(&mut self, directory: &mut Directory, bytes: &[u8]) -> bool {
        if !self.can_transport() {
            return false;
        }
        directory.send_to_all(&self.user.user_id, bytes);
        true
    }

    /// `P2PSend` (slot 50), the unicast twin.
    pub fn p2p_send(&mut self, directory: &mut Directory, peer: &str, bytes: &[u8]) -> bool {
        if !self.can_transport() {
            return false;
        }
        directory.send_to(&self.user.user_id, peer, bytes)
    }

    /// `P2PCloseAll` (slot 51). Returns the peers that were open so the caller
    /// can run the closed callbacks; the connections themselves are just our
    /// bookkeeping, since a local directory has no sockets.
    pub fn p2p_close_all(&mut self, _directory: &mut Directory) -> Vec<String> {
        let closed: Vec<String> = self.open_peers.iter().cloned().collect();
        self.open_peers.clear();
        closed
    }

    /// `P2PClose` (slot 52) — one of the eight ICF-folded `ret 4` no-ops in the
    /// shipped build, and retail dispatches it anyway from
    /// `LobbyManager::OnLobbyLeft`. Reproduced as a no-op that reports whether
    /// the peer was known, so a caller can tell the difference without the
    /// backend inventing a teardown retail never performed. **[measured]**
    pub fn p2p_close(&mut self, peer: &str) -> bool {
        self.open_peers.contains(peer)
    }

    pub fn open_peers(&self) -> impl Iterator<Item = &String> {
        self.open_peers.iter()
    }

    fn can_transport(&self) -> bool {
        self.status == SESSION_STATUS_STARTED && self.joined_lobby.is_some()
    }

    /// `Tick` (slot 57), pumped from `NetDaemon::process_all`. **[measured]**
    ///
    /// Drains at most one queued request per call plus everything the directory
    /// has posted to this peer, and returns them in order. Draining one request
    /// per tick keeps the completion order observable and stops a burst of lobby
    /// calls from re-entering the game's callbacks an unbounded number of times
    /// inside a single frame. **[DoN policy]**
    ///
    /// # Why no join/leave/update broadcast callbacks
    ///
    /// `SetJoinLobbyCallback`(16), `SetLeaveLobbyCallback`(18) and
    /// `SetUpdateLobbyCallback`(20) each take a `std::function<void(const DTO&,
    /// bool)>`, and **the meaning of that `bool` is unobserved**. In
    /// `CrossplayProxy.dll` the three fields (`this+0x120`, `+0x170`, and the
    /// update one) are written by the setters at RVAs `0x17fd0`/`0x183a0` and
    /// read only by `~CrossPlayService`, which calls `_Delete_this` on them — a
    /// full-image sweep for an invocation finds none. `gen/slot_usage.py`
    /// likewise finds no dispatch of the installers from `riseofnations.exe`.
    /// **[measured]**
    ///
    /// So DoN retains those callbacks (a `std::function` handed to us is owned
    /// and must be destroyed) and never calls them, because calling them would
    /// mean inventing that `bool`. Membership and attribute changes reach the
    /// game through the polling path it *is* measured to use:
    /// `LobbyManager::refresh_lobbies` → `FindLobbies`, and `GetLobby`.
    pub fn tick(&mut self, directory: &mut Directory) -> Vec<Emission> {
        let mut out = Vec::new();
        if let Some((id, request)) = self.queue.pop_front() {
            let outcome = self.resolve(directory, request);
            out.push(Emission::Completion(id, outcome));
        }
        for notice in directory.drain_mailbox(&self.user.user_id) {
            match &notice {
                Notice::PeerOpened(peer) => {
                    self.open_peers.insert(peer.clone());
                }
                Notice::PeerClosed(peer) => {
                    self.open_peers.remove(peer);
                }
                _ => {}
            }
            out.push(Emission::Notice(notice));
        }
        out.extend(self.ready.drain(..));
        out
    }

    fn resolve(&mut self, directory: &mut Directory, request: Request) -> Outcome {
        if !self.initialized {
            return failed(error::NOT_INITIALIZED, "Init has not been called");
        }
        if let Request::StartSession { user_id } = &request {
            self.user.user_id = user_id.to_string();
            self.user.platform_account_id = user_id.to_string();
            self.player_guid = user_id.to_string();
            self.status = SESSION_STATUS_STARTED;
            directory.attach_peer(user_id);
            return Outcome::SessionStarted(user_id.clone());
        }
        if self.status != SESSION_STATUS_STARTED {
            return failed(error::NO_SESSION, "no session is started");
        }
        match request {
            Request::StartSession { .. } => unreachable!("handled above"),
            Request::GetLobby(id) => match directory.get(&id) {
                Some(lobby) => Outcome::Lobby(lobby.clone()),
                None => failed(error::NOT_FOUND, "no such lobby"),
            },
            Request::FindLobbies {
                max_results,
                min_slots,
            } => Outcome::Lobbies(directory.find(max_results, min_slots)),
            Request::CreateLobby {
                max_members,
                visibility,
                attributes,
            } => match directory.create(&self.user, max_members, visibility, attributes) {
                Ok(lobby) => {
                    self.joined_lobby = Some(lobby.id.clone());
                    Outcome::Lobby(lobby)
                }
                Err((code, message)) => Outcome::Failed { code, message },
            },
            Request::JoinLobby(id) => match directory.join(&self.user, &id) {
                Ok(lobby) => {
                    self.joined_lobby = Some(lobby.id.clone());
                    Outcome::Lobby(lobby)
                }
                Err((code, message)) => Outcome::Failed { code, message },
            },
            Request::LeaveLobby(id) => match directory.leave(&self.user.user_id, &id) {
                Ok(()) => {
                    if self.joined_lobby.as_deref() == Some(id.as_str()) {
                        self.joined_lobby = None;
                    }
                    Outcome::Done
                }
                Err((code, message)) => Outcome::Failed { code, message },
            },
            Request::UpdateLobby {
                lobby_id,
                max_members,
                bot_count,
                attributes,
            } => match directory.update(
                &self.user.user_id,
                &lobby_id,
                max_members,
                bot_count,
                &attributes,
            ) {
                Ok(lobby) => Outcome::Updated(UpdateResult {
                    success: true,
                    timestamp: lobby.attribute_version as i64,
                    lobby,
                }),
                Err((code, message)) => Outcome::Failed { code, message },
            },
            Request::StartGame {
                lobby_id,
                session_reference,
            } => match directory.start_game(&self.user.user_id, &lobby_id, &session_reference) {
                Ok(reference) => Outcome::SessionReference(reference),
                Err((code, message)) => Outcome::Failed { code, message },
            },
            Request::CancelGameStart(id) => {
                match directory.cancel_game_start(&self.user.user_id, &id) {
                    Ok(reference) => Outcome::SessionReference(reference),
                    Err((code, message)) => Outcome::Failed { code, message },
                }
            }
            Request::FailClosed(slot) => Outcome::Failed {
                code: error::NOT_AVAILABLE,
                message: {
                    let mut m = String::from("not available in the DoN local backend: ");
                    m.push_str(slot);
                    m
                },
            },
        }
    }
}

fn failed(code: i32, message: &str) -> Outcome {
    Outcome::Failed {
        code,
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::VISIBILITY_PRIVATE;

    fn started(directory: &mut Directory, id: &str) -> Backend {
        let mut b = Backend::new(id, id);
        b.init();
        b.start_session(id);
        let out = b.tick(directory);
        assert!(matches!(out[0], Emission::Completion(_, Outcome::SessionStarted(_))));
        assert_eq!(b.session_status(), SESSION_STATUS_STARTED);
        b
    }

    fn drain_completion(b: &mut Backend, d: &mut Directory, req: ReqId) -> Outcome {
        for _ in 0..8 {
            for e in b.tick(d) {
                if let Emission::Completion(id, outcome) = e {
                    if id == req {
                        return outcome;
                    }
                }
            }
        }
        panic!("request {req:?} never completed");
    }

    #[test]
    fn session_walks_the_measured_state_machine() {
        let mut d = Directory::new();
        let mut b = Backend::new("host", "host");
        assert_eq!(b.session_status(), SESSION_STATUS_STOPPED);
        assert!(!b.is_connected_to_hub());
        b.init();
        b.start_session("host");
        // StartSession sets Starting synchronously: RVA 0x146ba.
        assert_eq!(b.session_status(), SESSION_STATUS_STARTING);
        assert!(!b.is_connected_to_hub());
        b.tick(&mut d);
        // The completion lambda sets Started: RVA 0x14b51.
        assert_eq!(b.session_status(), SESSION_STATUS_STARTED);
        assert!(b.is_connected_to_hub());
        assert_eq!(b.player_guid(), "host");
        // StopSession is synchronous through Stopping to Stopped.
        b.stop_session(&mut d);
        assert_eq!(b.session_status(), SESSION_STATUS_STOPPED);
        assert!(!b.is_connected_to_hub());
        // ...and is a no-op the second time: RVA 0x14d73/0x14d7b.
        assert!(b.stop_session(&mut d).is_empty());
        assert_eq!(b.session_status(), SESSION_STATUS_STOPPED);
    }

    #[test]
    fn requests_before_init_fail_closed() {
        let mut d = Directory::new();
        let mut b = Backend::new("host", "host");
        let r = b.get_lobby("nope");
        match drain_completion(&mut b, &mut d, r) {
            Outcome::Failed { code, .. } => assert_eq!(code, error::NOT_INITIALIZED),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn lobby_operations_require_a_started_session() {
        let mut d = Directory::new();
        let mut b = Backend::new("host", "host");
        b.init();
        let r = b.create_lobby(4, VISIBILITY_PUBLIC, Attributes::new());
        match drain_completion(&mut b, &mut d, r) {
            Outcome::Failed { code, .. } => assert_eq!(code, error::NO_SESSION),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn create_join_and_carry_the_game_seed_to_the_joiner() {
        let mut d = Directory::new();
        let mut host = started(&mut d, "host");
        let mut peer = started(&mut d, "peer");

        // The key schema itself lives in don-net's lobby.rs; this backend only
        // carries the pairs, so the test uses the real key name to show the
        // seed survives the round trip a joining peer depends on.
        let mut attrs = Attributes::new();
        attrs.insert("game_seed".into(), "3735928559".into());
        attrs.insert("name".into(), "don test lobby".into());
        let r = host.create_lobby(4, VISIBILITY_PUBLIC, attrs);
        let lobby = match drain_completion(&mut host, &mut d, r) {
            Outcome::Lobby(l) => l,
            other => panic!("{other:?}"),
        };
        assert_eq!(lobby.members.len(), 1);
        assert_eq!(lobby.owner_user_id, "host");
        assert_eq!(lobby.available_slots(), 3);

        let r = peer.find_lobbies(10, 1);
        let found = match drain_completion(&mut peer, &mut d, r) {
            Outcome::Lobbies(l) => l,
            other => panic!("{other:?}"),
        };
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, lobby.id);

        let r = peer.join_lobby(&lobby.id);
        let joined = match drain_completion(&mut peer, &mut d, r) {
            Outcome::Lobby(l) => l,
            other => panic!("{other:?}"),
        };
        assert_eq!(joined.members.len(), 2);
        assert_eq!(
            joined.attributes.get("game_seed").map(String::as_str),
            Some("3735928559"),
            "the joining peer must receive the simulation seed"
        );
        assert_eq!(joined.available_slots(), 2);
    }

    #[test]
    fn private_lobbies_are_not_discoverable_but_are_joinable_by_id() {
        let mut d = Directory::new();
        let mut host = started(&mut d, "host");
        let mut peer = started(&mut d, "peer");
        let r = host.create_lobby(2, VISIBILITY_PRIVATE, Attributes::new());
        let lobby = match drain_completion(&mut host, &mut d, r) {
            Outcome::Lobby(l) => l,
            other => panic!("{other:?}"),
        };
        let r = peer.find_lobbies(10, 0);
        assert_eq!(
            drain_completion(&mut peer, &mut d, r),
            Outcome::Lobbies(Vec::new())
        );
        let r = peer.join_lobby(&lobby.id);
        assert!(matches!(
            drain_completion(&mut peer, &mut d, r),
            Outcome::Lobby(_)
        ));
    }

    #[test]
    fn a_full_lobby_refuses_the_next_joiner() {
        let mut d = Directory::new();
        let mut host = started(&mut d, "host");
        let mut a = started(&mut d, "a");
        let mut b = started(&mut d, "b");
        let r = host.create_lobby(2, VISIBILITY_PUBLIC, Attributes::new());
        let lobby = match drain_completion(&mut host, &mut d, r) {
            Outcome::Lobby(l) => l,
            other => panic!("{other:?}"),
        };
        let r = a.join_lobby(&lobby.id);
        assert!(matches!(
            drain_completion(&mut a, &mut d, r),
            Outcome::Lobby(_)
        ));
        let r = b.join_lobby(&lobby.id);
        match drain_completion(&mut b, &mut d, r) {
            Outcome::Failed { code, .. } => assert_eq!(code, error::FULL),
            other => panic!("{other:?}"),
        }
        // and min_available_slots filters it out of a search
        let r = b.find_lobbies(10, 1);
        assert_eq!(
            drain_completion(&mut b, &mut d, r),
            Outcome::Lobbies(Vec::new())
        );
    }

    #[test]
    fn update_lobby_is_owner_only_and_bumps_the_attribute_version() {
        let mut d = Directory::new();
        let mut host = started(&mut d, "host");
        let mut peer = started(&mut d, "peer");
        let r = host.create_lobby(4, VISIBILITY_PUBLIC, Attributes::new());
        let lobby = match drain_completion(&mut host, &mut d, r) {
            Outcome::Lobby(l) => l,
            other => panic!("{other:?}"),
        };
        assert_eq!(lobby.attribute_version, 1);
        let r = peer.join_lobby(&lobby.id);
        drain_completion(&mut peer, &mut d, r);

        let mut delta = Attributes::new();
        delta.insert("steam_ready_1".into(), "1".into());
        let r = peer.update_lobby(&lobby.id, 0, -1, delta.clone());
        match drain_completion(&mut peer, &mut d, r) {
            Outcome::Failed { code, .. } => assert_eq!(code, error::NOT_OWNER),
            other => panic!("{other:?}"),
        }

        let r = host.update_lobby(&lobby.id, 0, -1, delta);
        let updated = match drain_completion(&mut host, &mut d, r) {
            Outcome::Updated(u) => u,
            other => panic!("{other:?}"),
        };
        assert!(updated.success);
        assert_eq!(updated.lobby.attribute_version, 2);
        assert_eq!(
            updated.lobby.attributes.get("steam_ready_1").map(String::as_str),
            Some("1")
        );

        // an empty value is a deletion, so a delta publish can retract a key
        let mut retract = Attributes::new();
        retract.insert("steam_ready_1".into(), String::new());
        let r = host.update_lobby(&lobby.id, 0, -1, retract);
        let updated = match drain_completion(&mut host, &mut d, r) {
            Outcome::Updated(u) => u,
            other => panic!("{other:?}"),
        };
        assert!(!updated.lobby.attributes.contains_key("steam_ready_1"));
        assert_eq!(updated.lobby.attribute_version, 3);
    }

    #[test]
    fn p2p_reaches_every_other_member_and_nobody_else() {
        let mut d = Directory::new();
        let mut host = started(&mut d, "host");
        let mut a = started(&mut d, "a");
        let mut b = started(&mut d, "b");
        let mut outsider = started(&mut d, "outsider");

        let r = host.create_lobby(4, VISIBILITY_PUBLIC, Attributes::new());
        let lobby = match drain_completion(&mut host, &mut d, r) {
            Outcome::Lobby(l) => l,
            other => panic!("{other:?}"),
        };
        for peer in [&mut a, &mut b] {
            let r = peer.join_lobby(&lobby.id);
            drain_completion(peer, &mut d, r);
        }
        // everyone drains their PeerOpened notices
        for peer in [&mut host, &mut a, &mut b] {
            peer.tick(&mut d);
            peer.tick(&mut d);
        }
        assert_eq!(host.open_peers().count(), 2);

        assert!(host.p2p_send_to_all(&mut d, b"turn-packet"));
        let got_a: Vec<Emission> = a.tick(&mut d);
        assert!(got_a.iter().any(|e| matches!(
            e,
            Emission::Notice(Notice::Data { from, bytes })
                if from == "host" && bytes == b"turn-packet"
        )));
        let got_b: Vec<Emission> = b.tick(&mut d);
        assert!(got_b.iter().any(|e| matches!(
            e,
            Emission::Notice(Notice::Data { from, .. }) if from == "host"
        )));
        let got_out: Vec<Emission> = outsider.tick(&mut d);
        assert!(
            !got_out
                .iter()
                .any(|e| matches!(e, Emission::Notice(Notice::Data { .. }))),
            "a peer outside the lobby must never see its traffic"
        );

        // unicast reaches exactly one peer
        assert!(a.p2p_send(&mut d, "b", b"unicast"));
        assert!(!a.p2p_send(&mut d, "outsider", b"unicast"));
        assert!(!a.p2p_send(&mut d, "a", b"self"));
        let got_b: Vec<Emission> = b.tick(&mut d);
        assert_eq!(
            got_b
                .iter()
                .filter(|e| matches!(e, Emission::Notice(Notice::Data { .. })))
                .count(),
            1
        );
    }

    #[test]
    fn p2p_refuses_before_a_lobby_exists() {
        let mut d = Directory::new();
        let mut host = started(&mut d, "host");
        assert!(!host.p2p_send_to_all(&mut d, b"early"));
        assert!(!host.p2p_send(&mut d, "peer", b"early"));
    }

    #[test]
    fn leaving_closes_the_peer_and_deletes_an_empty_lobby() {
        let mut d = Directory::new();
        let mut host = started(&mut d, "host");
        let mut peer = started(&mut d, "peer");
        let r = host.create_lobby(4, VISIBILITY_PUBLIC, Attributes::new());
        let lobby = match drain_completion(&mut host, &mut d, r) {
            Outcome::Lobby(l) => l,
            other => panic!("{other:?}"),
        };
        let r = peer.join_lobby(&lobby.id);
        drain_completion(&mut peer, &mut d, r);
        host.tick(&mut d);
        assert_eq!(host.open_peers().count(), 1);

        let r = peer.leave_lobby(&lobby.id);
        assert_eq!(drain_completion(&mut peer, &mut d, r), Outcome::Done);
        host.tick(&mut d);
        host.tick(&mut d);
        assert_eq!(host.open_peers().count(), 0);
        assert!(d.get(&lobby.id).is_some());

        let r = host.leave_lobby(&lobby.id);
        assert_eq!(drain_completion(&mut host, &mut d, r), Outcome::Done);
        assert!(d.get(&lobby.id).is_none(), "empty lobbies are removed");
    }

    #[test]
    fn ownership_migrates_when_the_owner_leaves() {
        let mut d = Directory::new();
        let mut host = started(&mut d, "host");
        let mut peer = started(&mut d, "peer");
        let r = host.create_lobby(4, VISIBILITY_PUBLIC, Attributes::new());
        let lobby = match drain_completion(&mut host, &mut d, r) {
            Outcome::Lobby(l) => l,
            other => panic!("{other:?}"),
        };
        let r = peer.join_lobby(&lobby.id);
        drain_completion(&mut peer, &mut d, r);
        let r = host.leave_lobby(&lobby.id);
        drain_completion(&mut host, &mut d, r);
        assert_eq!(d.get(&lobby.id).unwrap().owner_user_id, "peer");
    }

    #[test]
    fn start_game_publishes_the_session_reference_and_closes_the_lobby() {
        let mut d = Directory::new();
        let mut host = started(&mut d, "host");
        let mut peer = started(&mut d, "peer");
        let r = host.create_lobby(4, VISIBILITY_PUBLIC, Attributes::new());
        let lobby = match drain_completion(&mut host, &mut d, r) {
            Outcome::Lobby(l) => l,
            other => panic!("{other:?}"),
        };
        let r = host.start_game(&lobby.id, "session-1");
        assert_eq!(
            drain_completion(&mut host, &mut d, r),
            Outcome::SessionReference("session-1".to_string())
        );
        // a started match is no longer joinable or discoverable
        let r = peer.find_lobbies(10, 0);
        assert_eq!(
            drain_completion(&mut peer, &mut d, r),
            Outcome::Lobbies(Vec::new())
        );
        let r = peer.join_lobby(&lobby.id);
        match drain_completion(&mut peer, &mut d, r) {
            Outcome::Failed { code, .. } => assert_eq!(code, error::GAME_ALREADY_STARTED),
            other => panic!("{other:?}"),
        }
        // and a member polling GetLobby sees the reference
        let r = host.get_lobby(&lobby.id);
        match drain_completion(&mut host, &mut d, r) {
            Outcome::Lobby(l) => {
                assert!(l.game_started);
                assert_eq!(l.session_reference, "session-1");
            }
            other => panic!("{other:?}"),
        }
        let r = host.cancel_game_start(&lobby.id);
        assert_eq!(
            drain_completion(&mut host, &mut d, r),
            Outcome::SessionReference("session-1".to_string())
        );
        assert!(!d.get(&lobby.id).unwrap().game_started);
    }

    #[test]
    fn fail_closed_names_the_slot_it_refused() {
        let mut d = Directory::new();
        let mut b = started(&mut d, "host");
        let r = b.fail_closed("GetGlobalLeaderboards");
        match drain_completion(&mut b, &mut d, r) {
            Outcome::Failed { code, message } => {
                assert_eq!(code, error::NOT_AVAILABLE);
                assert!(message.contains("GetGlobalLeaderboards"), "{message}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn completions_are_delivered_in_submission_order() {
        let mut d = Directory::new();
        let mut b = started(&mut d, "host");
        let first = b.create_lobby(4, VISIBILITY_PUBLIC, Attributes::new());
        let second = b.find_lobbies(10, 0);
        let third = b.fail_closed("SetStats");
        let mut seen = Vec::new();
        for _ in 0..6 {
            for e in b.tick(&mut d) {
                if let Emission::Completion(id, _) = e {
                    seen.push(id);
                }
            }
        }
        assert_eq!(seen, alloc::vec![first, second, third]);
    }

    #[test]
    fn base36_is_injective_on_the_ids_it_produces() {
        let mut seen = BTreeSet::new();
        for i in 0..5000u64 {
            assert!(seen.insert(base36(i)), "collision at {i}");
        }
        assert_eq!(base36(0), "0");
        assert_eq!(base36(35), "z");
        assert_eq!(base36(36), "10");
    }
}
