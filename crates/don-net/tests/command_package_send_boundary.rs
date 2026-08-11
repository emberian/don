use don_net::{
    transport::{Datagram, Dest, LoopTransport, Transport},
    Role, Session, MAX_COMMAND_PACKAGE_PAYLOAD,
};
use std::io;
use std::time::Duration;

#[derive(Debug)]
struct RejectTransport;

impl Transport for RejectTransport {
    fn send(&mut self, _dest: Dest, _bytes: &[u8]) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "fixture refusal"))
    }

    fn recv(&mut self) -> io::Result<Option<Datagram>> {
        Ok(None)
    }

    fn poll(&mut self, _timeout: Duration) -> io::Result<()> {
        Ok(())
    }

    fn peers(&self) -> Vec<i32> {
        Vec::new()
    }

    fn local_id(&self) -> i32 {
        101
    }
}

#[test]
fn failed_transport_send_leaves_no_local_phantom_package() {
    let mut session = Session::new(RejectTransport, Role::Host, "host");
    let error = session
        .send_command_package(7, 0, &[0x0c])
        .expect_err("fixture transport must refuse the send");
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert!(session.package_for_turn(7, 0).is_none());
    assert!(!session.turn_ready(7));
}

#[test]
fn retail_payload_capacity_is_enforced_before_queueing() {
    let mut session = Session::new(LoopTransport::new(101), Role::Host, "host");
    let oversized = vec![0x0c; MAX_COMMAND_PACKAGE_PAYLOAD + 1];
    let error = session
        .send_command_package(9, 0, &oversized)
        .expect_err("513-byte package must be refused");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert!(session.package_for_turn(9, 0).is_none());

    let exact = vec![0x0c; MAX_COMMAND_PACKAGE_PAYLOAD];
    session
        .send_command_package(10, 0, &exact)
        .expect("the exact retail data[] capacity remains transportable");
    assert_eq!(session.package_for_turn(10, 0).unwrap().payload, exact);
}
