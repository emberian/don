//! Two `Session`s over two real TCP sockets, in two threads, handing each other
//! turns. This is the integration test for the transport the headless client
//! actually uses over the internet; the loopback tests in `session.rs` cannot
//! catch framing or socket-lifecycle bugs.

use don_net::session::{Role, Session};
use don_net::transport::TcpTransport;
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn run_peer(mut s: Session<TcpTransport>, slot: i8, turns: u32, tx: mpsc::Sender<(String, u64)>) {
    let start = Instant::now();
    let now = || start.elapsed().as_millis() as u64;

    while s.players().len() < 2 {
        s.poll(now(), Duration::from_millis(5)).unwrap();
        s.drain_events();
        assert!(start.elapsed() < Duration::from_secs(15), "roster timeout");
    }
    s.send_ready_flag(true).unwrap();
    while !s.all_ready() {
        s.poll(now(), Duration::from_millis(5)).unwrap();
        s.drain_events();
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "ready timeout; roster {:?}",
            s.players()
        );
    }
    let mut hash: u64 = 0;
    for stamp in 0..turns {
        let payload = vec![0x39u8, slot as u8, stamp as u8];
        s.send_command_package(stamp, slot, &payload).unwrap();
        while !s.turn_ready(stamp) {
            s.poll(now(), Duration::from_millis(5)).unwrap();
            s.drain_events();
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "turn {stamp} timeout"
            );
        }
        for p in s.take_turn(stamp) {
            hash = hash.wrapping_mul(1_000_003).wrapping_add(p.stamp as u64);
            hash = hash.wrapping_mul(1_000_003).wrapping_add(p.play as u64);
            for b in &p.payload {
                hash = hash.wrapping_mul(1_000_003).wrapping_add(*b as u64);
            }
        }
    }
    tx.send((format!("slot{slot}"), hash)).unwrap();
}

#[test]
fn two_tcp_peers_agree_on_every_turn() {
    const TURNS: u32 = 50;
    let host_tp = TcpTransport::host(1, "127.0.0.1:0").unwrap();
    let addr = host_tp.local_addr().unwrap();
    let host = Session::new(host_tp, Role::Host, "host");

    let (tx, rx) = mpsc::channel();
    let tx2 = tx.clone();
    let h = std::thread::spawn(move || run_peer(host, 0, TURNS, tx));
    let c = std::thread::spawn(move || {
        let tp = TcpTransport::join(2, addr).unwrap();
        let sess = Session::new(tp, Role::Client, "client");
        run_peer(sess, 1, TURNS, tx2);
    });
    h.join().unwrap();
    c.join().unwrap();

    let a = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let b = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(a.1, b.1, "peers disagree: {a:?} vs {b:?}");
    assert_ne!(a.1, 0);
}
