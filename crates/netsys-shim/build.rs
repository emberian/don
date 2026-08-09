//! Export the ten decorated `CrossplayNetLib` symbols under their exact
//! shipped names.
//!
//! `#[export_name = "?...@Z"]` does not survive into a cdylib's export set on
//! `i686-pc-windows-msvc` — the symbol lands in the object file but rustc's
//! generated `.def` and the link then disagree, and every one of the ten comes
//! back as `undefined symbol` (measured: all ten, `lld-link`, this tree).
//! Explicit `/EXPORT:exported=internal` aliases are deterministic and do not
//! depend on rustc's export-set inference.
//!
//! The mangled names are copied verbatim from the shipped DLL's export table.
//! The alias target is written *without* the x86 leading underscore: lld-link
//! prepends it itself when resolving `/EXPORT:a=b` on i386, so spelling
//! `_shim_foo` here produces a lookup for `__shim_foo` and fails. Measured, all
//! ten at once.

const EXPORTS: &[(&str, &str)] = &[
    (
        r"?is_connected_to_network@CrossplayNetLib@@YA_NXZ",
        "shim_is_connected_to_network",
    ),
    (
        r"?set_network_connection_state@CrossplayNetLib@@YAX_N@Z",
        "shim_set_network_connection_state",
    ),
    (
        r"?send_ready_flag@CrossplayNetLibSys@@QAEX_N@Z",
        "shim_send_ready_flag",
    ),
    (
        r"?reset_ready_flags@CrossplayNetLibSys@@QAEXXZ",
        "shim_reset_ready_flags",
    ),
    (
        r"?IsHost@CrossplayNetLibSys@@QAE_NABVLobbyMemberDTO@DTO@Lobby@Crossplay@@@Z",
        "shim_IsHost",
    ),
    (
        r"?OnPlayerJoined@CrossplayNetLibSys@@QAEXABVLobbyMemberDTO@DTO@Lobby@Crossplay@@ABV?$basic_string@_WU?$char_traits@_W@std@@V?$allocator@_W@2@@std@@@Z",
        "shim_OnPlayerJoined",
    ),
    (
        r"?OnPlayerLeft@CrossplayNetLibSys@@QAEXABVLobbyMemberDTO@DTO@Lobby@Crossplay@@@Z",
        "shim_OnPlayerLeft_member",
    ),
    (
        r"?OnPlayerLeft@CrossplayNetLibSys@@QAEXPBVCrossplayNetLibPlayer@@@Z",
        "shim_OnPlayerLeft_player",
    ),
    (
        r"?OnHostUpdated@CrossplayNetLibSys@@QAEXABV?$basic_string@_WU?$char_traits@_W@std@@V?$allocator@_W@2@@std@@@Z",
        "shim_OnHostUpdated",
    ),
    (
        r"?set_p2p_callbacks@CrossplayNetLibSys@@QAEXV?$function@$$A6AXPAVICrossplayPlayer@P2P@Crossplay@@@Z@std@@0V?$function@$$A6AXV?$basic_string@_WU?$char_traits@_W@std@@V?$allocator@_W@2@@std@@0@Z@3@@Z",
        "shim_set_p2p_callbacks",
    ),
];

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    for (exported, internal) in EXPORTS {
        println!("cargo::rustc-link-arg-cdylib=/EXPORT:{exported}={internal}");
    }
}
