use donscan::live::{
    encode_economy_ndjson, Coherence, GameMode, LiveLeader, ObservationMeta, Snapshot,
    COMPONENT_ECON, COMPONENT_GAME, COMPONENT_LEADERS, LEADER_VALID_CORE, LEADER_VALID_ECON,
    SOURCE_FILE_SIZE, SOURCE_SHA256,
};

fn fixture_line() -> String {
    let leader = LiveLeader {
        index: 0,
        leader_flags: 7,
        who: 0,
        tribe: 22,
        score: 117,
        pop: 48,
        pop_cap: 75,
        city_num: 2,
        gather_stamp: 12_390,
        team_color: 3,
        free_peasants: 1,
        gatherers: 26,
        fishermen: 2,
        idle_fishermen: 0,
        peasants: 31,
        scholars: 5,
        active_wars: 0,
        attacked: 0,
        gather_slots: [7, 6, 5, 4, 3, 1],
        filled_gather_slots: [7, 6, 5, 4, 3, 1],
        stockpile: [123, 234, 345, 456, 567, 0],
        leftover: [0; 6],
        resource_cap: [4000, 4000, 4000, 4000, 4000, 0, 0],
        over_cap: [0; 6],
        gross: [700, 640, 512, 384, 448, 0],
        support: [28, 0, 0, 0, 0, 0],
        income: [672, 640, 512, 384, 448, 0],
        ai_planning_rate: [0; 6],
        bonus: [0; 6],
        ages: 2,
        epochs: 0,
        discovered: 0,
        epoch: [3, 3, 2, 2],
        validity: LEADER_VALID_CORE | LEADER_VALID_ECON,
        ..Default::default()
    };
    let snapshot = Snapshot {
        ok: true,
        frame_start: 12_400,
        frame_end: 12_400,
        game_frame: 12_400,
        game_seconds: 826,
        game_mode: GameMode::SinglePlayer,
        paused: Some(false),
        leaders: vec![leader],
        reads: 18,
        bytes: 28_448,
        coherence: Coherence::Coherent,
        valid_components: COMPONENT_GAME | COMPONENT_LEADERS | COMPONENT_ECON,
        human_slot: 0,
        ..Default::default()
    };
    encode_economy_ndjson(
        &snapshot,
        ObservationMeta {
            session_id: 0x4242_4242_4242_4242,
            capture_seq: 19,
            pid: 4242,
            image_base: 0x00d6_0000,
            process_started_100ns: 133_800_000_000_000_000,
            module_size: SOURCE_FILE_SIZE,
            module_sha256: SOURCE_SHA256,
            captured_unix_ms: 1_785_000_000_100,
            monotonic_us: 55_123,
            capture_us: 410,
        },
    )
}

fn main() {
    let line = fixture_line();
    assert_eq!(
        format!("{line}\n"),
        include_str!("../fixtures/rontoy-observation-v1.ndjson")
    );
    println!("{line}");
}
