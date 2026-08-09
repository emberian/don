//! Datagram transport under the session layer.
//!
//! `NetSys` is a *datagram* interface: `send(packet, size, to, flags)`,
//! `send_all(packet, size, flags)`, `get(packet, &from, &size)`. Whatever sits
//! underneath must preserve message boundaries and must not reorder within a
//! peer. PlayFab Party gives that over DTLS/UDP. TCP gives it too once you add
//! a length prefix, and TCP is what we can actually drive today — from this
//! machine, over the internet, with no PlayFab account.
//!
//! Two implementations:
//!
//! * [`LoopTransport`] — in-process, for tests. No threads, no sockets.
//! * [`TcpTransport`] — a real socket. Host listens, peers connect, the host
//!   relays. Works on a LAN, over a VPN, or across the internet with one
//!   forwarded port; the framing is `u32` little-endian length then payload.
//!   The high length bit marks a host-relayed frame whose payload is prefixed
//!   by the original sender's `i32` id.
//!
//! The relay topology is not an invention: `NetDaemon::process` case 7 does
//! `netsys->send_all(packet, size, 1)` when the local peer is the host and the
//! packet came from someone else, i.e. **the host rebroadcasts command packages
//! to everyone**. `0x009510e5` is that arm. **[measured]**

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::time::Duration;

/// Which peers a datagram goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dest {
    /// `NetSys::send_all`.
    All,
    /// `NetSys::send(.., const NetPlayer* to, ..)`, addressed by unique id.
    One(i32),
}

/// One received datagram plus the unique id of the peer it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Datagram {
    pub from: i32,
    pub bytes: Vec<u8>,
}

/// The contract the session layer needs. Deliberately small: this is the only
/// surface a replacement `NetSys` has to provide.
pub trait Transport {
    /// Queue a datagram. Message boundaries must be preserved.
    fn send(&mut self, dest: Dest, bytes: &[u8]) -> io::Result<()>;
    /// Pop one received datagram, or `None` if nothing is pending.
    fn recv(&mut self) -> io::Result<Option<Datagram>>;
    /// Drive I/O. Must be cheap and non-blocking beyond `timeout`.
    fn poll(&mut self, timeout: Duration) -> io::Result<()>;
    /// Unique ids of every peer currently connected, excluding ourselves.
    fn peers(&self) -> Vec<i32>;
    /// Our own unique id.
    fn local_id(&self) -> i32;
}

// ---------------------------------------------------------------------------
// LoopTransport
// ---------------------------------------------------------------------------

/// A pair of in-process endpoints wired to each other. Nothing here blocks.
#[derive(Debug, Default)]
pub struct LoopTransport {
    id: i32,
    inbox: VecDeque<Datagram>,
    /// `(peer_id, outbound queue)` — drained by [`LoopTransport::pump`].
    outbox: Vec<(i32, VecDeque<Vec<u8>>)>,
}

impl LoopTransport {
    pub fn new(id: i32) -> Self {
        LoopTransport {
            id,
            inbox: VecDeque::new(),
            outbox: Vec::new(),
        }
    }

    pub fn connect(&mut self, peer: i32) {
        if !self.outbox.iter().any(|(p, _)| *p == peer) {
            self.outbox.push((peer, VecDeque::new()));
        }
    }

    /// Move everything queued on `a` and `b` into the other's inbox.
    pub fn pump(a: &mut LoopTransport, b: &mut LoopTransport) {
        fn drain(from: &mut LoopTransport, to: &mut LoopTransport) {
            let src = from.id;
            let dst = to.id;
            for (peer, q) in from.outbox.iter_mut() {
                if *peer != dst {
                    continue;
                }
                while let Some(bytes) = q.pop_front() {
                    to.inbox.push_back(Datagram { from: src, bytes });
                }
            }
        }
        drain(a, b);
        drain(b, a);
    }
}

impl Transport for LoopTransport {
    fn send(&mut self, dest: Dest, bytes: &[u8]) -> io::Result<()> {
        for (peer, q) in self.outbox.iter_mut() {
            if matches!(dest, Dest::All) || dest == Dest::One(*peer) {
                q.push_back(bytes.to_vec());
            }
        }
        Ok(())
    }
    fn recv(&mut self) -> io::Result<Option<Datagram>> {
        Ok(self.inbox.pop_front())
    }
    fn poll(&mut self, _timeout: Duration) -> io::Result<()> {
        Ok(())
    }
    fn peers(&self) -> Vec<i32> {
        self.outbox.iter().map(|(p, _)| *p).collect()
    }
    fn local_id(&self) -> i32 {
        self.id
    }
}

// ---------------------------------------------------------------------------
// TcpTransport
// ---------------------------------------------------------------------------

/// `u32` little-endian length prefix. 512 KB ceiling: the largest thing the
/// engine ever sends is a `NetMsg_CommandPackageData` wrapping at most
/// `CommandPackage::data[512]`, so this is three orders of magnitude of slack
/// and still bounds a hostile peer.
const MAX_FRAME: usize = 512 * 1024;
const RELAYED_FRAME: u32 = 1 << 31;

struct Peer {
    id: i32,
    sock: TcpStream,
    rx: Vec<u8>,
    /// Set once the id handshake has completed.
    identified: bool,
    dead: bool,
}

/// TCP star topology: the host listens, every other peer dials it, and the host
/// relays `Dest::All` traffic on behalf of clients.
///
/// The handshake is one frame carrying the sender's unique id, little-endian
/// i32. Everything after that is opaque datagrams.
pub struct TcpTransport {
    id: i32,
    listener: Option<TcpListener>,
    peers: Vec<Peer>,
    inbox: VecDeque<Datagram>,
    is_host: bool,
}

impl TcpTransport {
    /// Bind and listen. `addr` may be `0.0.0.0:0` to let the OS pick a port.
    pub fn host(id: i32, addr: impl ToSocketAddrs) -> io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        Ok(TcpTransport {
            id,
            listener: Some(listener),
            peers: Vec::new(),
            inbox: VecDeque::new(),
            is_host: true,
        })
    }

    /// Dial a host and announce our id.
    pub fn join(id: i32, addr: impl ToSocketAddrs) -> io::Result<Self> {
        let sock = TcpStream::connect(addr)?;
        sock.set_nodelay(true)?;
        sock.set_nonblocking(true)?;
        let mut t = TcpTransport {
            id,
            listener: None,
            peers: Vec::new(),
            inbox: VecDeque::new(),
            is_host: false,
        };
        // Host id is not known until it replies; 0 is a placeholder that
        // `poll` overwrites from the host's own hello frame.
        t.peers.push(Peer {
            id: 0,
            sock,
            rx: Vec::new(),
            identified: false,
            dead: false,
        });
        let hello = t.id.to_le_bytes().to_vec();
        write_frame(&mut t.peers[0].sock, &hello)?;
        Ok(t)
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        match &self.listener {
            Some(l) => l.local_addr(),
            None => self.peers[0].sock.peer_addr(),
        }
    }

    pub fn is_host(&self) -> bool {
        self.is_host
    }

    fn accept_pending(&mut self) -> io::Result<()> {
        let Some(l) = &self.listener else {
            return Ok(());
        };
        loop {
            match l.accept() {
                Ok((sock, _)) => {
                    sock.set_nodelay(true)?;
                    sock.set_nonblocking(true)?;
                    let mut p = Peer {
                        id: 0,
                        sock,
                        rx: Vec::new(),
                        identified: false,
                        dead: false,
                    };
                    write_frame(&mut p.sock, &self.id.to_le_bytes())?;
                    self.peers.push(p);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) => return Err(e),
            }
        }
    }
}

fn write_frame(sock: &mut TcpStream, payload: &[u8]) -> io::Result<()> {
    let mut buf = Vec::with_capacity(4 + payload.len());
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(payload);
    write_buffer(sock, &buf)
}

fn write_relayed_frame(sock: &mut TcpStream, origin: i32, payload: &[u8]) -> io::Result<()> {
    let wire_len = 4 + payload.len();
    let mut buf = Vec::with_capacity(4 + wire_len);
    buf.extend_from_slice(&((wire_len as u32) | RELAYED_FRAME).to_le_bytes());
    buf.extend_from_slice(&origin.to_le_bytes());
    buf.extend_from_slice(payload);
    write_buffer(sock, &buf)
}

fn write_buffer(sock: &mut TcpStream, buf: &[u8]) -> io::Result<()> {
    // The socket is non-blocking; a short write on a healthy socket is rare at
    // these sizes, but must still be handled rather than silently truncated.
    let mut off = 0;
    loop {
        match sock.write(&buf[off..]) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "peer closed")),
            Ok(n) => {
                off += n;
                if off == buf.len() {
                    return Ok(());
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::yield_now();
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

impl Transport for TcpTransport {
    fn send(&mut self, dest: Dest, bytes: &[u8]) -> io::Result<()> {
        for p in self.peers.iter_mut() {
            if p.dead {
                continue;
            }
            if matches!(dest, Dest::All) || dest == Dest::One(p.id) {
                if let Err(e) = write_frame(&mut p.sock, bytes) {
                    if e.kind() == io::ErrorKind::BrokenPipe
                        || e.kind() == io::ErrorKind::ConnectionReset
                    {
                        p.dead = true;
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        Ok(())
    }

    fn recv(&mut self) -> io::Result<Option<Datagram>> {
        Ok(self.inbox.pop_front())
    }

    fn poll(&mut self, timeout: Duration) -> io::Result<()> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            self.accept_pending()?;
            let mut progressed = false;
            let mut relay: Vec<(i32, Vec<u8>)> = Vec::new();
            for p in self.peers.iter_mut() {
                if p.dead {
                    continue;
                }
                let mut chunk = [0u8; 8192];
                loop {
                    match p.sock.read(&mut chunk) {
                        Ok(0) => {
                            p.dead = true;
                            break;
                        }
                        Ok(n) => {
                            p.rx.extend_from_slice(&chunk[..n]);
                            progressed = true;
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                        Err(_) => {
                            p.dead = true;
                            break;
                        }
                    }
                }
                // Deframe.
                loop {
                    if p.rx.len() < 4 {
                        break;
                    }
                    let header = u32::from_le_bytes(p.rx[0..4].try_into().unwrap());
                    let relayed = header & RELAYED_FRAME != 0;
                    let len = (header & !RELAYED_FRAME) as usize;
                    if len > MAX_FRAME + usize::from(relayed) * 4
                        || (relayed && (self.is_host || len < 4))
                    {
                        p.dead = true;
                        break;
                    }
                    if p.rx.len() < 4 + len {
                        break;
                    }
                    let mut body = p.rx[4..4 + len].to_vec();
                    p.rx.drain(..4 + len);
                    if !p.identified {
                        if body.len() == 4 {
                            p.id = i32::from_le_bytes(body[0..4].try_into().unwrap());
                            p.identified = true;
                        } else {
                            p.dead = true;
                        }
                        continue;
                    }
                    let from = if relayed {
                        let origin = i32::from_le_bytes(body[0..4].try_into().unwrap());
                        body.drain(..4);
                        origin
                    } else {
                        p.id
                    };
                    if self.is_host {
                        relay.push((p.id, body.clone()));
                    }
                    self.inbox.push_back(Datagram { from, bytes: body });
                }
            }
            // Host relay: everything a client sends reaches every other client.
            // This mirrors `NetDaemon::process` case 7 at 0x009510e5, where the
            // host re-issues `send_all` for a package it received.
            for (origin, body) in relay {
                for p in self.peers.iter_mut() {
                    if p.dead || !p.identified || p.id == origin {
                        continue;
                    }
                    if write_relayed_frame(&mut p.sock, origin, &body).is_err() {
                        p.dead = true;
                    }
                }
            }
            self.peers.retain(|p| {
                if p.dead {
                    let _ = p.sock.shutdown(Shutdown::Both);
                }
                !p.dead
            });
            if !self.inbox.is_empty() || std::time::Instant::now() >= deadline {
                return Ok(());
            }
            if !progressed {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }

    fn peers(&self) -> Vec<i32> {
        self.peers
            .iter()
            .filter(|p| p.identified && !p.dead)
            .map(|p| p.id)
            .collect()
    }

    fn local_id(&self) -> i32 {
        self.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_transport_delivers_both_ways_preserving_boundaries() {
        let mut a = LoopTransport::new(1);
        let mut b = LoopTransport::new(2);
        a.connect(2);
        b.connect(1);
        a.send(Dest::All, &[1, 2, 3]).unwrap();
        a.send(Dest::All, &[4]).unwrap();
        b.send(Dest::One(1), &[9, 9]).unwrap();
        LoopTransport::pump(&mut a, &mut b);
        assert_eq!(
            b.recv().unwrap(),
            Some(Datagram {
                from: 1,
                bytes: vec![1, 2, 3]
            })
        );
        assert_eq!(
            b.recv().unwrap(),
            Some(Datagram {
                from: 1,
                bytes: vec![4]
            })
        );
        assert_eq!(b.recv().unwrap(), None);
        assert_eq!(
            a.recv().unwrap(),
            Some(Datagram {
                from: 2,
                bytes: vec![9, 9]
            })
        );
    }

    #[test]
    fn tcp_transport_exchanges_datagrams_over_a_real_socket() {
        let mut host = TcpTransport::host(1, "127.0.0.1:0").unwrap();
        let addr = host.local_addr().unwrap();
        let mut client = TcpTransport::join(2, addr).unwrap();

        // Let the id handshake settle.
        for _ in 0..50 {
            host.poll(Duration::from_millis(5)).unwrap();
            client.poll(Duration::from_millis(5)).unwrap();
            if host.peers() == vec![2] && client.peers() == vec![1] {
                break;
            }
        }
        assert_eq!(host.peers(), vec![2]);
        assert_eq!(client.peers(), vec![1]);

        client.send(Dest::All, b"turn-0").unwrap();
        let mut got = None;
        for _ in 0..100 {
            host.poll(Duration::from_millis(5)).unwrap();
            if let Some(d) = host.recv().unwrap() {
                got = Some(d);
                break;
            }
        }
        assert_eq!(
            got,
            Some(Datagram {
                from: 2,
                bytes: b"turn-0".to_vec()
            })
        );

        host.send(Dest::One(2), b"turn-1").unwrap();
        let mut got = None;
        for _ in 0..100 {
            client.poll(Duration::from_millis(5)).unwrap();
            if let Some(d) = client.recv().unwrap() {
                got = Some(d);
                break;
            }
        }
        assert_eq!(
            got,
            Some(Datagram {
                from: 1,
                bytes: b"turn-1".to_vec()
            })
        );
    }

    #[test]
    fn tcp_host_relay_preserves_the_original_sender() {
        let mut host = TcpTransport::host(1, "127.0.0.1:0").unwrap();
        let addr = host.local_addr().unwrap();
        let mut a = TcpTransport::join(2, addr).unwrap();
        let mut b = TcpTransport::join(3, addr).unwrap();

        for _ in 0..100 {
            host.poll(Duration::from_millis(5)).unwrap();
            a.poll(Duration::from_millis(5)).unwrap();
            b.poll(Duration::from_millis(5)).unwrap();
            if host.peers().len() == 2 && a.peers() == vec![1] && b.peers() == vec![1] {
                break;
            }
        }
        assert_eq!(host.peers().len(), 2);

        a.send(Dest::All, b"from-two").unwrap();
        let mut got = None;
        for _ in 0..100 {
            host.poll(Duration::from_millis(5)).unwrap();
            b.poll(Duration::from_millis(5)).unwrap();
            if let Some(datagram) = b.recv().unwrap() {
                got = Some(datagram);
                break;
            }
        }
        assert_eq!(
            got,
            Some(Datagram {
                from: 2,
                bytes: b"from-two".to_vec(),
            })
        );
    }
}
