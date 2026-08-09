use rontoy_proto::{encode_frame, Frame, Heartbeat, Message, WireLimits};

fn main() {
    let frame = Frame::new(
        [0x11; 16],
        1,
        2,
        Message::Heartbeat(Heartbeat {
            last_snapshot_sequence: Some(0),
            producer_state: "attached".into(),
            unknown: vec![],
        }),
    );
    let bytes = encode_frame(&frame, WireLimits::default()).expect("golden frame encodes");
    for byte in bytes {
        print!("{byte:02x}");
    }
    println!();
}
