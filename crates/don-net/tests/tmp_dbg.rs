use don_net::session::{Role, Session};
use don_net::transport::TcpTransport;
use std::time::{Duration, Instant};
#[test]
fn dbg_session(){
    let htp = TcpTransport::host(1, "127.0.0.1:0").unwrap();
    let addr = htp.local_addr().unwrap();
    let mut host = Session::new(htp, Role::Host, "host");
    let ctp = TcpTransport::join(2, addr).unwrap();
    let mut client = Session::new(ctp, Role::Client, "client");
    let start = Instant::now();
    let mut hr=false; let mut cr=false;
    for i in 0..400 {
        let t = start.elapsed().as_millis() as u64;
        host.poll(t, Duration::from_millis(1)).unwrap();
        client.poll(t, Duration::from_millis(1)).unwrap();
        for e in host.drain_events() { println!("[{i}] HOST evt {e:?}"); }
        for e in client.drain_events() { println!("[{i}] CLI  evt {e:?}"); }
        if !hr && host.players().len()>=2 { println!("[{i}] host roster full; sending ready"); host.send_ready_flag(true).unwrap(); hr=true; }
        if !cr && client.players().len()>=2 { println!("[{i}] cli roster full; sending ready"); client.send_ready_flag(true).unwrap(); cr=true; }
        if hr&&cr&&host.all_ready()&&client.all_ready() { println!("[{i}] BOTH ALL READY"); return; }
        if i%50==49 { println!("[{i}] host={:?}", host.players().iter().map(|p|(p.unique_id,p.ready)).collect::<Vec<_>>());
                      println!("[{i}] cli ={:?}", client.players().iter().map(|p|(p.unique_id,p.ready)).collect::<Vec<_>>()); }
    }
    panic!("never converged");
}
