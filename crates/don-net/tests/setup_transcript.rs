//! Exact setup/control transcript over real TCP, including malformed and
//! duplicate packets plus a same-owned-ID membership epoch.

use don_net::internal::{InternalError, InternalPacket, IPT_ADDPLAYER, IPT_READYFLAG, MAX_PLAYERS};
use don_net::session::{Event, Role, Session, SetupRefusal};
use don_net::transport::{Dest, TcpTransport, Transport};
use std::time::{Duration, Instant};

const HOST_ID: i32 = 1;
const CLIENT_ID: i32 = 2;

fn send_internal(session: &mut Session<TcpTransport>, packet: &InternalPacket) {
    let mut wire = Vec::new();
    packet.encode(&mut wire);
    assert_eq!(wire.len(), packet.wire_len());
    session.transport.send(Dest::All, &wire).unwrap();
}

fn poll(session: &mut Session<TcpTransport>, start: &Instant) -> Vec<Event> {
    session
        .poll(start.elapsed().as_millis() as u64, Duration::from_millis(5))
        .unwrap();
    session.drain_events()
}

fn wait_for_event(
    session: &mut Session<TcpTransport>,
    start: &Instant,
    expected: &Event,
) -> Vec<Event> {
    let deadline = Instant::now();
    let mut events = Vec::new();
    loop {
        events.extend(poll(session, start));
        if events.contains(expected) {
            return events;
        }
        assert!(deadline.elapsed() < Duration::from_secs(2));
    }
}

fn authoritative(session: &Session<TcpTransport>) -> bool {
    let players = session.players();
    players.len() == 2
        && players[0].unique_id == HOST_ID
        && players[0].slot == 0
        && players[0].name == "Ai"
        && players[1].unique_id == CLIENT_ID
        && players[1].slot == 1
        && players[1].name == "Ai"
}

fn connect_client(addr: std::net::SocketAddr) -> Session<TcpTransport> {
    Session::new(
        TcpTransport::join(CLIENT_ID, addr).unwrap(),
        Role::Client,
        "Ai",
    )
}

#[test]
fn setup_wire_is_authoritative_idempotent_and_epoch_safe() {
    let transport = TcpTransport::host(HOST_ID, "127.0.0.1:0").unwrap();
    let addr = transport.local_addr().unwrap();
    let mut host = Session::new(transport, Role::Host, "Ai");
    let mut client = connect_client(addr);
    let start = Instant::now();
    while !authoritative(&host) || !authoritative(&client) {
        poll(&mut host, &start);
        poll(&mut client, &start);
        assert!(start.elapsed() < Duration::from_secs(5));
    }
    host.drain_events();
    client.drain_events();

    // An exact duplicate AddPlayer is idempotent: no second player object and
    // no second PlayerJoined callback.
    send_internal(
        &mut client,
        &InternalPacket::AddPlayer {
            player_name: "Ai".into(),
            unique_id: CLIENT_ID,
            is_hosting: false,
        },
    );
    // A duplicate with conflicting identity data is refused without mutation.
    send_internal(
        &mut client,
        &InternalPacket::AddPlayer {
            player_name: "NotAi".into(),
            unique_id: CLIENT_ID,
            is_hosting: false,
        },
    );
    let conflict = Event::SetupRefused {
        from: CLIENT_ID,
        reason: SetupRefusal::DuplicateAddConflict {
            unique_id: CLIENT_ID,
        },
    };
    let duplicate_events = wait_for_event(&mut host, &start, &conflict);
    assert_eq!(host.players().len(), 2);
    assert!(!duplicate_events
        .iter()
        .any(|event| matches!(event, Event::PlayerJoined(_))));
    assert_eq!(
        duplicate_events
            .iter()
            .filter(|event| matches!(event, Event::SetupRefused { .. }))
            .count(),
        1
    );
    assert_eq!(host.find(CLIENT_ID).unwrap().name, "Ai");

    // Ready is level state. The first true transition emits one event; an
    // exact duplicate true packet emits none.
    client.send_ready_flag(true).unwrap();
    let first_ready = Event::ReadyChanged {
        unique_id: CLIENT_ID,
        ready: true,
    };
    wait_for_event(&mut host, &start, &first_ready);
    send_internal(&mut client, &InternalPacket::ReadyFlag { ready: true });

    // Only the host may publish the exact 34-byte authoritative list.
    let mut ids = [0i32; MAX_PLAYERS];
    ids[0] = HOST_ID;
    ids[1] = CLIENT_ID;
    send_internal(
        &mut client,
        &InternalPacket::PlayerList {
            num_players: 2,
            unique_ids: ids,
        },
    );
    let non_host_list = Event::SetupRefused {
        from: CLIENT_ID,
        reason: SetupRefusal::PlayerListFromNonHost,
    };
    let repeated_ready_events = wait_for_event(&mut host, &start, &non_host_list);
    assert!(!repeated_ready_events
        .iter()
        .any(|event| matches!(event, Event::ReadyChanged { .. })));

    // A duplicate authoritative list is level state too: it changes neither
    // roster nor events.
    send_internal(
        &mut host,
        &InternalPacket::PlayerList {
            num_players: 2,
            unique_ids: ids,
        },
    );
    // A third transport peer cannot replace the already established host by
    // putting itself in slot zero. The TCP relay preserves its origin id.
    let mut rogue = TcpTransport::join(3, addr).unwrap();
    let mut takeover_ids = [0i32; MAX_PLAYERS];
    takeover_ids[0] = 3;
    takeover_ids[1] = CLIENT_ID;
    let mut takeover = Vec::new();
    InternalPacket::PlayerList {
        num_players: 2,
        unique_ids: takeover_ids,
    }
    .encode(&mut takeover);
    let rogue_deadline = Instant::now();
    while rogue.peers().is_empty() {
        rogue.poll(Duration::from_millis(5)).unwrap();
        host.poll(start.elapsed().as_millis() as u64, Duration::from_millis(5))
            .unwrap();
        assert!(rogue_deadline.elapsed() < Duration::from_secs(2));
    }
    rogue.send(Dest::All, &takeover).unwrap();
    let takeover_refusal = Event::SetupRefused {
        from: 3,
        reason: SetupRefusal::PlayerListHostMismatch { expected: HOST_ID },
    };
    let mut takeover_events = Vec::new();
    loop {
        poll(&mut host, &start);
        takeover_events.extend(poll(&mut client, &start));
        if takeover_events.contains(&takeover_refusal) {
            break;
        }
        assert!(
            rogue_deadline.elapsed() < Duration::from_secs(2),
            "events: {takeover_events:?}"
        );
    }
    assert!(!takeover_events
        .iter()
        .any(|event| matches!(event, Event::PlayerJoined(_) | Event::PlayerLeft(_))));
    assert!(authoritative(&client));
    drop(rogue);

    // Malformed size and non-bool packets are surfaced and consumed without
    // changing readiness or roster state.
    client
        .transport
        .send(Dest::All, &[IPT_READYFLAG, 1, 0])
        .unwrap();
    let trailing_ready = Event::SetupRefused {
        from: CLIENT_ID,
        reason: SetupRefusal::Malformed(InternalError::Trailing {
            id: IPT_READYFLAG,
            need: 2,
            have: 3,
        }),
    };
    wait_for_event(&mut host, &start, &trailing_ready);
    let mut bad_add = vec![0u8; 70];
    bad_add[0] = IPT_ADDPLAYER;
    bad_add[65..69].copy_from_slice(&CLIENT_ID.to_le_bytes());
    bad_add[69] = 2;
    client.transport.send(Dest::All, &bad_add).unwrap();
    let invalid_add = Event::SetupRefused {
        from: CLIENT_ID,
        reason: SetupRefusal::Malformed(InternalError::InvalidBool {
            id: IPT_ADDPLAYER,
            value: 2,
        }),
    };
    wait_for_event(&mut host, &start, &invalid_add);

    // A peer cannot remove somebody else.
    send_internal(
        &mut client,
        &InternalPacket::DestroyPlayer { unique_id: HOST_ID },
    );
    let false_destroy = Event::SetupRefused {
        from: CLIENT_ID,
        reason: SetupRefusal::DestroySenderMismatch { announced: HOST_ID },
    };
    wait_for_event(&mut host, &start, &false_destroy);
    assert_eq!(host.players().len(), 2);

    // Exact five-byte departure is authoritative and duplicate departure is
    // idempotent. A ready packet after removal is refused as pre-membership.
    client.announce_disconnect().unwrap();
    wait_for_event(&mut host, &start, &Event::PlayerLeft(CLIENT_ID));
    client.announce_disconnect().unwrap();
    send_internal(&mut client, &InternalPacket::ReadyFlag { ready: true });
    let post_leave_ready = Event::SetupRefused {
        from: CLIENT_ID,
        reason: SetupRefusal::ReadyBeforeAdd,
    };
    let duplicate_leave_events = wait_for_event(&mut host, &start, &post_leave_ready);
    assert!(!duplicate_leave_events
        .iter()
        .any(|event| matches!(event, Event::PlayerLeft(_))));
    drop(client);

    // Same id on a new socket is a fresh epoch. The destroy reset causes the
    // host to republish AddPlayer before ReadyFlag, with no external repair.
    let mut reconnect = connect_client(addr);
    let mut host_epoch_events = Vec::new();
    while !authoritative(&host) || !authoritative(&reconnect) {
        host_epoch_events.extend(poll(&mut host, &start));
        poll(&mut reconnect, &start);
        assert!(start.elapsed() < Duration::from_secs(8));
    }
    reconnect.send_ready_flag(true).unwrap();
    host.send_ready_flag(true).unwrap();
    while !host.all_ready() || !reconnect.all_ready() {
        host_epoch_events.extend(poll(&mut host, &start));
        poll(&mut reconnect, &start);
        assert!(start.elapsed() < Duration::from_secs(8));
    }
    let joined = host_epoch_events
        .iter()
        .position(|event| *event == Event::PlayerJoined(CLIENT_ID))
        .unwrap();
    let ready = host_epoch_events
        .iter()
        .position(|event| {
            *event
                == Event::ReadyChanged {
                    unique_id: CLIENT_ID,
                    ready: true,
                }
        })
        .unwrap();
    assert!(joined < ready);
}
