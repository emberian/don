// SPDX-License-Identifier: GPL-3.0-or-later
//! DoN-owned loopback RPC for [`crate::local::Directory`].
//!
//! This module is deliberately feature-gated behind `std-rpc`: the checked
//! Crossplay ABI and its ordinary in-process backend remain allocator-only.
//! The protocol below is not PlayFab, Party, `CrossplayProxy.dll`, or any other
//! shipped wire format. It is a small versioned transport for DoN's own
//! [`DirectoryCall`] / [`DirectoryAnswer`] boundary. **[DoN policy]**
//!
//! A client owns one worker thread and exposes the non-blocking
//! [`AsyncDirectory`] interface. The game thread only queues a call or polls a
//! channel; socket reads and writes never happen inside an ABI slot or `Tick`.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::local::{
    error, AsyncDirectory, AsyncDirectoryEvent, Attributes, Directory, DirectoryAnswer,
    DirectoryCall, DirectoryOperation, Lobby, Member, Notice, Outcome, UpdateResult,
};

const REQUEST_MAGIC: &[u8; 4] = b"DNRQ";
const RESPONSE_MAGIC: &[u8; 4] = b"DNRS";
const VERSION: u16 = 1;
const MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;
const MAX_STRING_BYTES: usize = 64 * 1024;
const MAX_COLLECTION_ITEMS: usize = 4096;
const MAX_PACKET_BYTES: usize = 1024 * 1024;
const MAX_NOTICE_PAGE_ITEMS: usize = 1024;

/// A loopback directory authority. Every connection executes transactions
/// against one mutex-protected [`Directory`]; socket handling can be concurrent
/// while each state transition remains atomic.
pub struct DirectoryRpcServer {
    address: SocketAddr,
    stopping: Arc<AtomicBool>,
    accept_thread: Option<JoinHandle<()>>,
}

impl DirectoryRpcServer {
    /// Bind and start a directory authority. Port `0` is accepted for tests.
    pub fn bind(address: impl ToSocketAddrs) -> io::Result<Self> {
        let listener = TcpListener::bind(address)?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        if !address.ip().is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "directory RPC authority must bind a loopback address",
            ));
        }
        let stopping = Arc::new(AtomicBool::new(false));
        let directory = Arc::new(Mutex::new(Directory::new()));
        let active_users = Arc::new(Mutex::new(BTreeSet::new()));
        let stop = Arc::clone(&stopping);
        let accept_thread = thread::Builder::new()
            .name("don-directory-accept".to_string())
            .spawn(move || accept_loop(listener, directory, active_users, stop))?;
        Ok(Self {
            address,
            stopping,
            accept_thread: Some(accept_thread),
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.address
    }
}

impl Drop for DirectoryRpcServer {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        // Wake a platform whose non-blocking accept is still parked in the
        // kernel. The accepted stream exits immediately because `stopping` is
        // already visible.
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.accept_thread.take() {
            let _ = thread.join();
        }
    }
}

fn accept_loop(
    listener: TcpListener,
    directory: Arc<Mutex<Directory>>,
    active_users: Arc<Mutex<BTreeSet<String>>>,
    stopping: Arc<AtomicBool>,
) {
    let mut connections = Vec::new();
    while !stopping.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                if stopping.load(Ordering::Acquire) {
                    break;
                }
                // Accepted sockets inherit `O_NONBLOCK` from the listener on
                // some hosts (including macOS). Each connection owns a thread,
                // so restore blocking I/O before its framed read loop.
                if stream.set_nonblocking(false).is_err() {
                    continue;
                }
                let directory = Arc::clone(&directory);
                let active_users = Arc::clone(&active_users);
                let stopping = Arc::clone(&stopping);
                let control = match stream.try_clone() {
                    Ok(control) => control,
                    Err(_) => continue,
                };
                if let Ok(connection) = thread::Builder::new()
                    .name("don-directory-client".to_string())
                    .spawn(move || serve_connection(stream, directory, active_users, stopping))
                {
                    connections.push((connection, control));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2));
            }
            Err(_) => break,
        }
    }
    // Interrupt a connection even if it has read only part of a frame. A read
    // timeout would be unsafe here: `read_exact` may consume a prefix before
    // timing out, and restarting at the next length field would desynchronise
    // the stream. The cloned handles give shutdown a deterministic, lossless
    // way to wake every blocking connection thread.
    for (_, control) in &connections {
        let _ = control.shutdown(Shutdown::Both);
    }
    for (connection, _) in connections {
        let _ = connection.join();
    }
}

fn serve_connection(
    mut stream: TcpStream,
    directory: Arc<Mutex<Directory>>,
    active_users: Arc<Mutex<BTreeSet<String>>>,
    stopping: Arc<AtomicBool>,
) {
    let _ = stream.set_nodelay(true);
    let mut identity: Option<Member> = None;
    let mut stopping_identity = false;
    while !stopping.load(Ordering::Acquire) {
        let bytes = match read_frame(&mut stream) {
            Ok(bytes) => bytes,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::UnexpectedEof
                        | io::ErrorKind::ConnectionReset
                        | io::ErrorKind::BrokenPipe
                ) =>
            {
                break;
            }
            Err(_) => break,
        };
        let call = match decode_call(&bytes) {
            Ok(call) => call,
            Err(_) => break,
        };
        let is_start = matches!(call.operation, DirectoryOperation::StartSession);
        let is_stop = matches!(call.operation, DirectoryOperation::StopSession { .. });
        let is_poll = matches!(call.operation, DirectoryOperation::Poll);

        let answer = if is_start {
            match &identity {
                Some(bound) if bound != &call.user => failure_answer(
                    error::IDENTITY_MISMATCH,
                    "StartSession user does not match this connection",
                ),
                Some(_) => match directory.lock() {
                    Ok(mut directory) => transact_directory(&mut directory, call),
                    Err(_) => break,
                },
                None if call.user.user_id.is_empty() => {
                    failure_answer(error::BAD_ARGUMENT, "StartSession user id is empty")
                }
                None => {
                    let claimed = match active_users.lock() {
                        Ok(mut users) => users.insert(call.user.user_id.clone()),
                        Err(_) => break,
                    };
                    if !claimed {
                        failure_answer(
                            error::IDENTITY_IN_USE,
                            "directory identity is already active",
                        )
                    } else {
                        identity = Some(call.user.clone());
                        match directory.lock() {
                            Ok(mut directory) => transact_directory(&mut directory, call),
                            Err(_) => break,
                        }
                    }
                }
            }
        } else {
            match &identity {
                None => failure_answer(error::NOT_INITIALIZED, "StartSession is required"),
                Some(bound) if bound != &call.user => failure_answer(
                    error::IDENTITY_MISMATCH,
                    "directory call user does not match this connection",
                ),
                Some(_) if stopping_identity && !is_poll => failure_answer(
                    error::NOT_AVAILABLE,
                    "StopSession is draining this connection",
                ),
                Some(_) => match directory.lock() {
                    Ok(mut directory) => transact_directory(&mut directory, call),
                    Err(_) => break,
                },
            }
        };

        if is_stop && !matches!(answer.outcome, Outcome::Failed { .. }) {
            stopping_identity = true;
        }
        if stopping_identity && !answer.more_notices {
            release_identity(&mut identity, &directory, &active_users);
            stopping_identity = false;
        }
        let bytes = encode_answer(&answer);
        if write_frame(&mut stream, &bytes).is_err() {
            break;
        }
    }
    release_identity(&mut identity, &directory, &active_users);
}

fn transact_directory(directory: &mut Directory, call: DirectoryCall) -> DirectoryAnswer {
    let user_id = call.user.user_id.clone();
    let exceeds_member_cap = match &call.operation {
        DirectoryOperation::CreateLobby { max_members, .. }
        | DirectoryOperation::UpdateLobby { max_members, .. } => {
            *max_members > MAX_COLLECTION_ITEMS as i32
        }
        _ => false,
    };
    let mut outcome = if exceeds_member_cap {
        Outcome::Failed {
            code: error::BAD_ARGUMENT,
            message: "lobby member bound exceeds the RPC collection cap".to_string(),
        }
    } else {
        directory.execute(call)
    };
    if !outcome_collections_fit(&outcome) {
        outcome = Outcome::Failed {
            code: error::NOT_AVAILABLE,
            message: "directory outcome exceeds the RPC collection cap".to_string(),
        };
    }
    let mut answer = DirectoryAnswer {
        outcome: outcome.clone(),
        notices: Vec::new(),
        more_notices: directory.has_pending_notices(&user_id),
    };
    if encode_answer(&answer).len() > MAX_FRAME_BYTES {
        outcome = Outcome::Failed {
            code: error::NOT_AVAILABLE,
            message: "directory outcome exceeds the RPC frame bound".to_string(),
        };
        answer.outcome = outcome;
    }

    // The fixed answer and every selected notice are sized before a notice is
    // removed. Consequently encoding cannot fail after mutating the mailbox.
    let fixed_bytes = encode_answer(&answer).len();
    let available = MAX_FRAME_BYTES.saturating_sub(fixed_bytes);
    let (notices, more_notices) = directory.drain_mailbox_page(
        &user_id,
        MAX_NOTICE_PAGE_ITEMS,
        available,
        encoded_notice_size,
    );
    answer.notices = notices;
    answer.more_notices = more_notices;
    debug_assert!(encode_answer(&answer).len() <= MAX_FRAME_BYTES);
    answer
}

fn outcome_collections_fit(outcome: &Outcome) -> bool {
    let lobby_fits = |lobby: &Lobby| {
        lobby.members.len() <= MAX_COLLECTION_ITEMS
            && lobby.attributes.len() <= MAX_COLLECTION_ITEMS
    };
    match outcome {
        Outcome::Lobby(lobby) => lobby_fits(lobby),
        Outcome::Lobbies(lobbies) => {
            lobbies.len() <= MAX_COLLECTION_ITEMS && lobbies.iter().all(lobby_fits)
        }
        Outcome::Updated(update) => lobby_fits(&update.lobby),
        _ => true,
    }
}

fn encoded_notice_size(notice: &Notice) -> usize {
    let mut encoder = Encoder { bytes: Vec::new() };
    encoder.notice(notice);
    encoder.bytes.len()
}

fn failure_answer(code: i32, message: &str) -> DirectoryAnswer {
    DirectoryAnswer {
        outcome: Outcome::Failed {
            code,
            message: message.to_string(),
        },
        notices: Vec::new(),
        more_notices: false,
    }
}

fn release_identity(
    identity: &mut Option<Member>,
    directory: &Arc<Mutex<Directory>>,
    active_users: &Arc<Mutex<BTreeSet<String>>>,
) {
    let Some(identity) = identity.take() else {
        return;
    };
    if let Ok(mut directory) = directory.lock() {
        directory.cleanup_peer(&identity.user_id);
    }
    if let Ok(mut users) = active_users.lock() {
        users.remove(&identity.user_id);
    }
}

enum Job {
    Answered(DirectoryCall),
    Detached(DirectoryCall),
}

/// Async client adapter used by `Backend::tick_remote`.
///
/// `connect` reaches an existing authority. `listen` starts an authority in
/// this process and connects the same worker to it; retaining the client keeps
/// that authority alive, which is the DLL host-process mode.
pub struct DirectoryRpcClient {
    jobs: Option<mpsc::Sender<Job>>,
    answers: mpsc::Receiver<AsyncDirectoryEvent>,
    busy: bool,
    unavailable: Option<String>,
    worker_stream: Option<TcpStream>,
    worker: Option<JoinHandle<()>>,
    owned_server: Option<DirectoryRpcServer>,
}

impl DirectoryRpcClient {
    pub fn connect(address: impl ToSocketAddrs) -> io::Result<Self> {
        let stream = TcpStream::connect(address)?;
        if !stream.peer_addr()?.ip().is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "directory RPC peer must use a loopback address",
            ));
        }
        Self::from_stream(stream, None)
    }

    pub fn listen(address: impl ToSocketAddrs) -> io::Result<Self> {
        let server = DirectoryRpcServer::bind(address)?;
        let stream = TcpStream::connect(server.local_addr())?;
        Self::from_stream(stream, Some(server))
    }

    /// A fail-closed adapter for a configured endpoint that could not be
    /// opened. It lets the DLL still return its mandatory singleton while each
    /// queued request receives an asynchronous error instead of silently
    /// falling back to a process-private directory.
    pub fn unavailable(message: impl Into<String>) -> Self {
        let (_answer_tx, answer_rx) = mpsc::channel();
        Self {
            jobs: None,
            answers: answer_rx,
            busy: false,
            unavailable: Some(message.into()),
            worker_stream: None,
            worker: None,
            owned_server: None,
        }
    }

    /// Address of the authority retained by a client created with `listen`.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.owned_server
            .as_ref()
            .map(DirectoryRpcServer::local_addr)
    }

    fn from_stream(
        stream: TcpStream,
        owned_server: Option<DirectoryRpcServer>,
    ) -> io::Result<Self> {
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        let worker_stream = stream.try_clone()?;
        let (job_tx, job_rx) = mpsc::channel();
        let (answer_tx, answer_rx) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("don-directory-rpc".to_string())
            .spawn(move || client_worker(stream, job_rx, answer_tx))?;
        Ok(Self {
            jobs: Some(job_tx),
            answers: answer_rx,
            busy: false,
            unavailable: None,
            worker_stream: Some(worker_stream),
            worker: Some(worker),
            owned_server,
        })
    }

    fn send_job(&self, job: Job) -> Result<(), String> {
        if let Some(message) = &self.unavailable {
            return Err(message.clone());
        }
        self.jobs
            .as_ref()
            .ok_or_else(|| "directory RPC worker is unavailable".to_string())?
            .send(job)
            .map_err(|_| "directory RPC worker stopped".to_string())
    }
}

impl AsyncDirectory for DirectoryRpcClient {
    fn submit(&mut self, call: DirectoryCall) -> Result<(), String> {
        if self.busy {
            return Err("directory RPC already has an answered call in flight".to_string());
        }
        self.send_job(Job::Answered(call))?;
        self.busy = true;
        Ok(())
    }

    fn poll(&mut self) -> Option<AsyncDirectoryEvent> {
        match self.answers.try_recv() {
            Ok(event) => {
                if matches!(event, AsyncDirectoryEvent::Requested(_)) {
                    self.busy = false;
                }
                Some(event)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) if self.busy => {
                self.busy = false;
                Some(AsyncDirectoryEvent::Requested(Err(
                    "directory RPC worker stopped".to_string(),
                )))
            }
            Err(mpsc::TryRecvError::Disconnected) => None,
        }
    }

    fn notify(&mut self, call: DirectoryCall) -> Result<(), String> {
        self.send_job(Job::Detached(call))
    }
}

impl Drop for DirectoryRpcClient {
    fn drop(&mut self) {
        self.jobs.take();
        if let Some(stream) = self.worker_stream.take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.owned_server.take();
    }
}

fn client_worker(
    mut stream: TcpStream,
    jobs: mpsc::Receiver<Job>,
    answers: mpsc::Sender<AsyncDirectoryEvent>,
) {
    let mut failed: Option<String> = None;
    while let Ok(job) = jobs.recv() {
        let answered = matches!(job, Job::Answered(_));
        if let Some(message) = &failed {
            let event = if answered {
                AsyncDirectoryEvent::Requested(Err(message.clone()))
            } else {
                AsyncDirectoryEvent::Detached(Err(message.clone()))
            };
            if answers.send(event).is_err() {
                break;
            }
            continue;
        }
        let call = match job {
            Job::Answered(call) | Job::Detached(call) => call,
        };
        let result = transact(&mut stream, &call).map_err(|error| error.to_string());
        if let Err(message) = &result {
            failed = Some(message.clone());
        }
        let event = if answered {
            AsyncDirectoryEvent::Requested(result)
        } else {
            AsyncDirectoryEvent::Detached(result)
        };
        if answers.send(event).is_err() {
            break;
        }
    }
}

fn transact(stream: &mut TcpStream, call: &DirectoryCall) -> io::Result<DirectoryAnswer> {
    write_frame(stream, &encode_call(call))?;
    let bytes = read_frame(stream)?;
    decode_answer(&bytes)
}

fn write_frame(stream: &mut TcpStream, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(invalid("directory RPC frame exceeds the configured bound"));
    }
    stream.write_all(&(bytes.len() as u32).to_le_bytes())?;
    stream.write_all(bytes)?;
    stream.flush()
}

fn read_frame(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut length = [0u8; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_FRAME_BYTES {
        return Err(invalid("directory RPC frame exceeds the configured bound"));
    }
    let mut bytes = vec![0u8; length];
    stream.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn encode_call(call: &DirectoryCall) -> Vec<u8> {
    let mut out = Encoder::new(REQUEST_MAGIC);
    out.member(&call.user);
    match &call.operation {
        DirectoryOperation::StartSession => out.u8(0),
        DirectoryOperation::StopSession { lobby_id } => {
            out.u8(1);
            out.option_string(lobby_id.as_deref());
        }
        DirectoryOperation::GetLobby(id) => {
            out.u8(2);
            out.string(id);
        }
        DirectoryOperation::FindLobbies {
            max_results,
            min_slots,
        } => {
            out.u8(3);
            out.i32(*max_results);
            out.i32(*min_slots);
        }
        DirectoryOperation::CreateLobby {
            max_members,
            visibility,
            attributes,
        } => {
            out.u8(4);
            out.i32(*max_members);
            out.i32(*visibility);
            out.attributes(attributes);
        }
        DirectoryOperation::JoinLobby(id) => {
            out.u8(5);
            out.string(id);
        }
        DirectoryOperation::LeaveLobby(id) => {
            out.u8(6);
            out.string(id);
        }
        DirectoryOperation::UpdateLobby {
            lobby_id,
            max_members,
            bot_count,
            attributes,
        } => {
            out.u8(7);
            out.string(lobby_id);
            out.i32(*max_members);
            out.i32(*bot_count);
            out.attributes(attributes);
        }
        DirectoryOperation::StartGame {
            lobby_id,
            session_reference,
        } => {
            out.u8(8);
            out.string(lobby_id);
            out.string(session_reference);
        }
        DirectoryOperation::CancelGameStart(id) => {
            out.u8(9);
            out.string(id);
        }
        DirectoryOperation::SendToAll(bytes) => {
            out.u8(10);
            out.bytes(bytes);
        }
        DirectoryOperation::SendTo { peer, bytes } => {
            out.u8(11);
            out.string(peer);
            out.bytes(bytes);
        }
        DirectoryOperation::Poll => out.u8(12),
    }
    out.finish()
}

fn decode_call(bytes: &[u8]) -> io::Result<DirectoryCall> {
    let mut input = Decoder::new(bytes, REQUEST_MAGIC)?;
    let user = input.member()?;
    let operation = match input.u8()? {
        0 => DirectoryOperation::StartSession,
        1 => DirectoryOperation::StopSession {
            lobby_id: input.option_string()?,
        },
        2 => DirectoryOperation::GetLobby(input.string()?),
        3 => DirectoryOperation::FindLobbies {
            max_results: input.i32()?,
            min_slots: input.i32()?,
        },
        4 => DirectoryOperation::CreateLobby {
            max_members: input.i32()?,
            visibility: input.i32()?,
            attributes: input.attributes()?,
        },
        5 => DirectoryOperation::JoinLobby(input.string()?),
        6 => DirectoryOperation::LeaveLobby(input.string()?),
        7 => DirectoryOperation::UpdateLobby {
            lobby_id: input.string()?,
            max_members: input.i32()?,
            bot_count: input.i32()?,
            attributes: input.attributes()?,
        },
        8 => DirectoryOperation::StartGame {
            lobby_id: input.string()?,
            session_reference: input.string()?,
        },
        9 => DirectoryOperation::CancelGameStart(input.string()?),
        10 => DirectoryOperation::SendToAll(input.bytes(MAX_PACKET_BYTES)?),
        11 => DirectoryOperation::SendTo {
            peer: input.string()?,
            bytes: input.bytes(MAX_PACKET_BYTES)?,
        },
        12 => DirectoryOperation::Poll,
        _ => return Err(invalid("unknown directory operation")),
    };
    input.finish()?;
    Ok(DirectoryCall { user, operation })
}

fn encode_answer(answer: &DirectoryAnswer) -> Vec<u8> {
    let mut out = Encoder::new(RESPONSE_MAGIC);
    out.outcome(&answer.outcome);
    out.bool(answer.more_notices);
    out.u32(answer.notices.len() as u32);
    for notice in &answer.notices {
        out.notice(notice);
    }
    out.finish()
}

fn decode_answer(bytes: &[u8]) -> io::Result<DirectoryAnswer> {
    let mut input = Decoder::new(bytes, RESPONSE_MAGIC)?;
    let outcome = input.outcome()?;
    let more_notices = input.bool()?;
    let count = input.count()?;
    let mut notices = Vec::with_capacity(count);
    for _ in 0..count {
        notices.push(input.notice()?);
    }
    input.finish()?;
    Ok(DirectoryAnswer {
        outcome,
        notices,
        more_notices,
    })
}

struct Encoder {
    bytes: Vec<u8>,
}

impl Encoder {
    fn new(magic: &[u8; 4]) -> Self {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(magic);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        Self { bytes }
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }

    fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn bool(&mut self, value: bool) {
        self.u8(value as u8);
    }

    fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn i64(&mut self, value: i64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn string(&mut self, value: &str) {
        self.u32(value.len() as u32);
        self.bytes.extend_from_slice(value.as_bytes());
    }

    fn option_string(&mut self, value: Option<&str>) {
        self.bool(value.is_some());
        if let Some(value) = value {
            self.string(value);
        }
    }

    fn bytes(&mut self, value: &[u8]) {
        self.u32(value.len() as u32);
        self.bytes.extend_from_slice(value);
    }

    fn attributes(&mut self, attributes: &Attributes) {
        self.u32(attributes.len() as u32);
        for (key, value) in attributes {
            self.string(key);
            self.string(value);
        }
    }

    fn member(&mut self, member: &Member) {
        self.string(&member.user_id);
        self.string(&member.user_name);
        self.string(&member.platform);
        self.string(&member.platform_account_id);
    }

    fn lobby(&mut self, lobby: &Lobby) {
        self.string(&lobby.id);
        self.string(&lobby.owner_user_id);
        self.string(&lobby.session_reference);
        self.i32(lobby.max_members);
        self.i32(lobby.bot_count);
        self.i32(lobby.attribute_version);
        self.i32(lobby.visibility);
        self.u32(lobby.members.len() as u32);
        for member in &lobby.members {
            self.member(member);
        }
        self.attributes(&lobby.attributes);
        self.bool(lobby.game_started);
    }

    fn outcome(&mut self, outcome: &Outcome) {
        match outcome {
            Outcome::SessionStarted(value) => {
                self.u8(0);
                self.string(value);
            }
            Outcome::Lobby(lobby) => {
                self.u8(1);
                self.lobby(lobby);
            }
            Outcome::Lobbies(lobbies) => {
                self.u8(2);
                self.u32(lobbies.len() as u32);
                for lobby in lobbies {
                    self.lobby(lobby);
                }
            }
            Outcome::Updated(update) => {
                self.u8(3);
                self.bool(update.success);
                self.lobby(&update.lobby);
                self.i64(update.timestamp);
            }
            Outcome::SessionReference(value) => {
                self.u8(4);
                self.string(value);
            }
            Outcome::Done => self.u8(5),
            Outcome::Failed { code, message } => {
                self.u8(6);
                self.i32(*code);
                self.string(message);
            }
        }
    }

    fn notice(&mut self, notice: &Notice) {
        match notice {
            Notice::PeerOpened(peer) => {
                self.u8(0);
                self.string(peer);
            }
            Notice::PeerClosed(peer) => {
                self.u8(1);
                self.string(peer);
            }
            Notice::Data { from, bytes } => {
                self.u8(2);
                self.string(from);
                self.bytes(bytes);
            }
            Notice::Text { from, text } => {
                self.u8(3);
                self.string(from);
                self.string(text);
            }
        }
    }
}

struct Decoder<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Decoder<'a> {
    fn new(bytes: &'a [u8], magic: &[u8; 4]) -> io::Result<Self> {
        if bytes.len() < 6 || &bytes[..4] != magic {
            return Err(invalid("directory RPC magic mismatch"));
        }
        if u16::from_le_bytes([bytes[4], bytes[5]]) != VERSION {
            return Err(invalid("unsupported directory RPC version"));
        }
        Ok(Self { bytes, at: 6 })
    }

    fn finish(&self) -> io::Result<()> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(invalid("trailing bytes in directory RPC message"))
        }
    }

    fn take(&mut self, count: usize) -> io::Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(count)
            .ok_or_else(|| invalid("directory RPC length overflow"))?;
        if end > self.bytes.len() {
            return Err(invalid("truncated directory RPC message"));
        }
        let value = &self.bytes[self.at..end];
        self.at = end;
        Ok(value)
    }

    fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn bool(&mut self) -> io::Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(invalid("non-boolean directory RPC value")),
        }
    }

    fn u32(&mut self) -> io::Result<u32> {
        let bytes: [u8; 4] = self.take(4)?.try_into().expect("length checked");
        Ok(u32::from_le_bytes(bytes))
    }

    fn i32(&mut self) -> io::Result<i32> {
        let bytes: [u8; 4] = self.take(4)?.try_into().expect("length checked");
        Ok(i32::from_le_bytes(bytes))
    }

    fn i64(&mut self) -> io::Result<i64> {
        let bytes: [u8; 8] = self.take(8)?.try_into().expect("length checked");
        Ok(i64::from_le_bytes(bytes))
    }

    fn count(&mut self) -> io::Result<usize> {
        let count = self.u32()? as usize;
        if count > MAX_COLLECTION_ITEMS {
            return Err(invalid(
                "directory RPC collection exceeds the configured bound",
            ));
        }
        Ok(count)
    }

    fn string(&mut self) -> io::Result<String> {
        let length = self.u32()? as usize;
        if length > MAX_STRING_BYTES {
            return Err(invalid("directory RPC string exceeds the configured bound"));
        }
        let bytes = self.take(length)?;
        let value = core::str::from_utf8(bytes)
            .map_err(|_| invalid("directory RPC string is not UTF-8"))?;
        Ok(value.to_string())
    }

    fn option_string(&mut self) -> io::Result<Option<String>> {
        if self.bool()? {
            Ok(Some(self.string()?))
        } else {
            Ok(None)
        }
    }

    fn bytes(&mut self, limit: usize) -> io::Result<Vec<u8>> {
        let length = self.u32()? as usize;
        if length > limit {
            return Err(invalid(
                "directory RPC byte vector exceeds the configured bound",
            ));
        }
        Ok(self.take(length)?.to_vec())
    }

    fn attributes(&mut self) -> io::Result<Attributes> {
        let count = self.count()?;
        let mut attributes = BTreeMap::new();
        for _ in 0..count {
            let key = self.string()?;
            let value = self.string()?;
            if attributes.insert(key, value).is_some() {
                return Err(invalid("duplicate directory RPC attribute key"));
            }
        }
        Ok(attributes)
    }

    fn member(&mut self) -> io::Result<Member> {
        Ok(Member {
            user_id: self.string()?,
            user_name: self.string()?,
            platform: self.string()?,
            platform_account_id: self.string()?,
        })
    }

    fn lobby(&mut self) -> io::Result<Lobby> {
        let id = self.string()?;
        let owner_user_id = self.string()?;
        let session_reference = self.string()?;
        let max_members = self.i32()?;
        let bot_count = self.i32()?;
        let attribute_version = self.i32()?;
        let visibility = self.i32()?;
        let count = self.count()?;
        let mut members = Vec::with_capacity(count);
        for _ in 0..count {
            members.push(self.member()?);
        }
        let attributes = self.attributes()?;
        let game_started = self.bool()?;
        Ok(Lobby {
            id,
            owner_user_id,
            session_reference,
            max_members,
            bot_count,
            attribute_version,
            visibility,
            members,
            attributes,
            game_started,
        })
    }

    fn outcome(&mut self) -> io::Result<Outcome> {
        match self.u8()? {
            0 => Ok(Outcome::SessionStarted(self.string()?)),
            1 => Ok(Outcome::Lobby(self.lobby()?)),
            2 => {
                let count = self.count()?;
                let mut lobbies = Vec::with_capacity(count);
                for _ in 0..count {
                    lobbies.push(self.lobby()?);
                }
                Ok(Outcome::Lobbies(lobbies))
            }
            3 => Ok(Outcome::Updated(UpdateResult {
                success: self.bool()?,
                lobby: self.lobby()?,
                timestamp: self.i64()?,
            })),
            4 => Ok(Outcome::SessionReference(self.string()?)),
            5 => Ok(Outcome::Done),
            6 => Ok(Outcome::Failed {
                code: self.i32()?,
                message: self.string()?,
            }),
            _ => Err(invalid("unknown directory RPC outcome")),
        }
    }

    fn notice(&mut self) -> io::Result<Notice> {
        match self.u8()? {
            0 => Ok(Notice::PeerOpened(self.string()?)),
            1 => Ok(Notice::PeerClosed(self.string()?)),
            2 => Ok(Notice::Data {
                from: self.string()?,
                bytes: self.bytes(MAX_PACKET_BYTES)?,
            }),
            3 => Ok(Notice::Text {
                from: self.string()?,
                text: self.string()?,
            }),
            _ => Err(invalid("unknown directory RPC notice")),
        }
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::VISIBILITY_PUBLIC;
    use std::time::Instant;

    fn sample_lobby() -> Lobby {
        let mut attributes = Attributes::new();
        attributes.insert("game_seed".to_string(), "123".to_string());
        Lobby {
            id: "abc".to_string(),
            owner_user_id: "host".to_string(),
            session_reference: String::new(),
            max_members: 4,
            bot_count: 1,
            attribute_version: 2,
            visibility: VISIBILITY_PUBLIC,
            members: vec![Member::new("host", "Host")],
            attributes,
            game_started: false,
        }
    }

    fn wait_event(client: &mut DirectoryRpcClient) -> AsyncDirectoryEvent {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Some(event) = client.poll() {
                return event;
            }
            thread::sleep(Duration::from_millis(1));
        }
        panic!("directory RPC event timed out")
    }

    fn requested(client: &mut DirectoryRpcClient, call: DirectoryCall) -> DirectoryAnswer {
        client.submit(call).unwrap();
        match wait_event(client) {
            AsyncDirectoryEvent::Requested(answer) => answer.unwrap(),
            AsyncDirectoryEvent::Detached(answer) => {
                panic!("unexpected detached answer: {answer:?}")
            }
        }
    }

    fn detached(client: &mut DirectoryRpcClient, call: DirectoryCall) -> DirectoryAnswer {
        client.notify(call).unwrap();
        match wait_event(client) {
            AsyncDirectoryEvent::Detached(answer) => answer.unwrap(),
            AsyncDirectoryEvent::Requested(answer) => {
                panic!("unexpected requested answer: {answer:?}")
            }
        }
    }

    fn start(client: &mut DirectoryRpcClient, user: &Member) -> DirectoryAnswer {
        requested(
            client,
            DirectoryCall {
                user: user.clone(),
                operation: DirectoryOperation::StartSession,
            },
        )
    }

    fn create(client: &mut DirectoryRpcClient, host: &Member) -> Lobby {
        let answer = requested(
            client,
            DirectoryCall {
                user: host.clone(),
                operation: DirectoryOperation::CreateLobby {
                    max_members: 4,
                    visibility: VISIBILITY_PUBLIC,
                    attributes: Attributes::new(),
                },
            },
        );
        match answer.outcome {
            Outcome::Lobby(lobby) => lobby,
            other => panic!("CreateLobby returned {other:?}"),
        }
    }

    #[test]
    fn request_and_response_codecs_round_trip() {
        let user = Member::new("peer", "Peer");
        let attributes = sample_lobby().attributes;
        let operations = vec![
            DirectoryOperation::StartSession,
            DirectoryOperation::StopSession {
                lobby_id: Some("abc".to_string()),
            },
            DirectoryOperation::GetLobby("abc".to_string()),
            DirectoryOperation::FindLobbies {
                max_results: 12,
                min_slots: 1,
            },
            DirectoryOperation::CreateLobby {
                max_members: 8,
                visibility: VISIBILITY_PUBLIC,
                attributes: attributes.clone(),
            },
            DirectoryOperation::JoinLobby("abc".to_string()),
            DirectoryOperation::LeaveLobby("abc".to_string()),
            DirectoryOperation::UpdateLobby {
                lobby_id: "abc".to_string(),
                max_members: 8,
                bot_count: 2,
                attributes,
            },
            DirectoryOperation::StartGame {
                lobby_id: "abc".to_string(),
                session_reference: "session".to_string(),
            },
            DirectoryOperation::CancelGameStart("abc".to_string()),
            DirectoryOperation::SendToAll(vec![1, 2, 3]),
            DirectoryOperation::SendTo {
                peer: "host".to_string(),
                bytes: vec![4, 5, 6],
            },
            DirectoryOperation::Poll,
        ];
        for operation in operations {
            let call = DirectoryCall {
                user: user.clone(),
                operation,
            };
            assert_eq!(decode_call(&encode_call(&call)).unwrap(), call);
        }

        let lobby = sample_lobby();
        let outcomes = vec![
            Outcome::SessionStarted("peer".to_string()),
            Outcome::Lobby(lobby.clone()),
            Outcome::Lobbies(vec![lobby.clone()]),
            Outcome::Updated(UpdateResult {
                success: true,
                lobby,
                timestamp: 7,
            }),
            Outcome::SessionReference("session".to_string()),
            Outcome::Done,
            Outcome::Failed {
                code: -3,
                message: "no such lobby".to_string(),
            },
        ];
        for outcome in outcomes {
            let answer = DirectoryAnswer {
                outcome,
                notices: vec![
                    Notice::PeerOpened("host".to_string()),
                    Notice::PeerClosed("gone".to_string()),
                    Notice::Data {
                        from: "host".to_string(),
                        bytes: vec![1, 2, 3],
                    },
                    Notice::Text {
                        from: "host".to_string(),
                        text: "hello".to_string(),
                    },
                ],
                more_notices: true,
            };
            assert_eq!(decode_answer(&encode_answer(&answer)).unwrap(), answer);
        }
    }

    #[test]
    fn client_worker_answers_through_the_poll_channel() {
        let mut client = DirectoryRpcClient::listen("127.0.0.1:0").unwrap();
        client
            .submit(DirectoryCall {
                user: Member::new("host", "Host"),
                operation: DirectoryOperation::StartSession,
            })
            .unwrap();
        // `submit` only writes to a channel. A caller has to poll later; this
        // is the adapter property the ABI Tick path relies on.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut answer = None;
        while std::time::Instant::now() < deadline {
            if let Some(AsyncDirectoryEvent::Requested(value)) = client.poll() {
                answer = Some(value.unwrap());
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        assert!(matches!(
            answer.unwrap().outcome,
            Outcome::SessionStarted(ref id) if id == "host"
        ));
    }

    #[test]
    fn detached_send_and_stop_answers_preserve_inbound_notices() {
        let mut host_client = DirectoryRpcClient::listen("127.0.0.1:0").unwrap();
        let address = host_client.local_addr().unwrap();
        let mut peer_client = DirectoryRpcClient::connect(address).unwrap();
        let host = Member::new("host", "Host");
        let peer = Member::new("peer", "Peer");
        assert!(matches!(
            start(&mut host_client, &host).outcome,
            Outcome::SessionStarted(_)
        ));
        assert!(matches!(
            start(&mut peer_client, &peer).outcome,
            Outcome::SessionStarted(_)
        ));
        let lobby = create(&mut host_client, &host);
        let joined = requested(
            &mut peer_client,
            DirectoryCall {
                user: peer.clone(),
                operation: DirectoryOperation::JoinLobby(lobby.id),
            },
        );
        assert!(matches!(joined.outcome, Outcome::Lobby(_)));

        // Join queued PeerOpened(peer) for the host. A detached send performs
        // the next host transaction and therefore owns that inbound notice.
        let send = detached(
            &mut host_client,
            DirectoryCall {
                user: host.clone(),
                operation: DirectoryOperation::SendToAll(b"queued-before-stop".to_vec()),
            },
        );
        assert!(send
            .notices
            .iter()
            .any(|notice| matches!(notice, Notice::PeerOpened(id) if id == "peer")));

        // StopSession is also detached by the ABI adapter. It must return the
        // already-queued packet before the authority removes the mailbox.
        let stop = detached(
            &mut peer_client,
            DirectoryCall {
                user: peer,
                operation: DirectoryOperation::StopSession { lobby_id: None },
            },
        );
        assert!(stop.notices.iter().any(|notice| matches!(
            notice,
            Notice::Data { from, bytes }
                if from == "host" && bytes == b"queued-before-stop"
        )));
    }

    #[test]
    fn mailbox_pages_two_max_packets_without_draining_the_second() {
        let mut host_client = DirectoryRpcClient::listen("127.0.0.1:0").unwrap();
        let address = host_client.local_addr().unwrap();
        let mut peer_client = DirectoryRpcClient::connect(address).unwrap();
        let host = Member::new("host", "Host");
        let peer = Member::new("peer", "Peer");
        start(&mut host_client, &host);
        start(&mut peer_client, &peer);
        let lobby = create(&mut host_client, &host);
        requested(
            &mut peer_client,
            DirectoryCall {
                user: peer.clone(),
                operation: DirectoryOperation::JoinLobby(lobby.id),
            },
        );

        for byte in [0x11, 0x22] {
            detached(
                &mut host_client,
                DirectoryCall {
                    user: host.clone(),
                    operation: DirectoryOperation::SendTo {
                        peer: peer.user_id.clone(),
                        bytes: vec![byte; MAX_PACKET_BYTES],
                    },
                },
            );
        }

        let first = requested(
            &mut peer_client,
            DirectoryCall {
                user: peer.clone(),
                operation: DirectoryOperation::Poll,
            },
        );
        assert!(first.more_notices);
        assert!(matches!(
            first.notices.as_slice(),
            [Notice::Data { bytes, .. }] if bytes.len() == MAX_PACKET_BYTES && bytes[0] == 0x11
        ));
        let second = requested(
            &mut peer_client,
            DirectoryCall {
                user: peer,
                operation: DirectoryOperation::Poll,
            },
        );
        assert!(!second.more_notices);
        assert!(matches!(
            second.notices.as_slice(),
            [Notice::Data { bytes, .. }] if bytes.len() == MAX_PACKET_BYTES && bytes[0] == 0x22
        ));
    }

    #[test]
    fn mailbox_pages_4097_notices_below_the_decoder_collection_cap() {
        let user = Member::new("peer", "Peer");
        let mut directory = Directory::new();
        directory.attach_peer(&user.user_id);
        for index in 0..4097 {
            directory.post(&user.user_id, Notice::PeerOpened(format!("peer-{index}")));
        }

        let mut observed = Vec::new();
        loop {
            let answer = transact_directory(
                &mut directory,
                DirectoryCall {
                    user: user.clone(),
                    operation: DirectoryOperation::Poll,
                },
            );
            assert!(answer.notices.len() <= MAX_NOTICE_PAGE_ITEMS);
            let decoded = decode_answer(&encode_answer(&answer)).unwrap();
            observed.extend(decoded.notices);
            if !decoded.more_notices {
                break;
            }
        }
        assert_eq!(observed.len(), 4097);
        for (index, notice) in observed.iter().enumerate() {
            assert_eq!(notice, &Notice::PeerOpened(format!("peer-{index}")));
        }
    }

    #[test]
    fn connection_identity_rejects_duplicates_and_spoofed_frames() {
        let mut host_client = DirectoryRpcClient::listen("127.0.0.1:0").unwrap();
        let address = host_client.local_addr().unwrap();
        let mut victim_client = DirectoryRpcClient::connect(address).unwrap();
        let mut duplicate_client = DirectoryRpcClient::connect(address).unwrap();
        let host = Member::new("host", "Host");
        let victim = Member::new("victim", "Victim");
        start(&mut host_client, &host);
        start(&mut victim_client, &victim);
        let duplicate = start(&mut duplicate_client, &victim);
        assert!(matches!(
            duplicate.outcome,
            Outcome::Failed {
                code: error::IDENTITY_IN_USE,
                ..
            }
        ));
        let unbound_stop = requested(
            &mut duplicate_client,
            DirectoryCall {
                user: victim.clone(),
                operation: DirectoryOperation::StopSession { lobby_id: None },
            },
        );
        assert!(matches!(
            unbound_stop.outcome,
            Outcome::Failed {
                code: error::NOT_INITIALIZED,
                ..
            }
        ));

        let lobby = create(&mut host_client, &host);
        requested(
            &mut victim_client,
            DirectoryCall {
                user: victim.clone(),
                operation: DirectoryOperation::JoinLobby(lobby.id),
            },
        );
        detached(
            &mut host_client,
            DirectoryCall {
                user: host.clone(),
                operation: DirectoryOperation::SendTo {
                    peer: victim.user_id.clone(),
                    bytes: b"victim-only".to_vec(),
                },
            },
        );

        let spoof = requested(
            &mut victim_client,
            DirectoryCall {
                user: Member::new("victim", "Different identity fields"),
                operation: DirectoryOperation::Poll,
            },
        );
        assert!(matches!(
            spoof.outcome,
            Outcome::Failed {
                code: error::IDENTITY_MISMATCH,
                ..
            }
        ));
        assert!(
            spoof.notices.is_empty(),
            "a spoof must not drain the mailbox"
        );
        let legitimate = requested(
            &mut victim_client,
            DirectoryCall {
                user: victim.clone(),
                operation: DirectoryOperation::Poll,
            },
        );
        assert!(legitimate.notices.iter().any(|notice| matches!(
            notice,
            Notice::Data { bytes, .. } if bytes == b"victim-only"
        )));

        drop(victim_client);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let retry = start(&mut duplicate_client, &victim);
            if matches!(retry.outcome, Outcome::SessionStarted(_)) {
                break;
            }
            assert!(matches!(
                retry.outcome,
                Outcome::Failed {
                    code: error::IDENTITY_IN_USE,
                    ..
                }
            ));
            assert!(
                Instant::now() < deadline,
                "identity was not released on disconnect"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}
