// The playable page's raw Wasm ABI contract.
//
// Keep this list explicit: the checked-in Wasm is a generated release artefact, while the
// JavaScript and Rust sources can advance independently.  A missing export must stop at the
// loader boundary with a stale-artefact diagnostic instead of failing later in an unrelated
// panel.  The static checker also proves that every direct `x.game_*` reference in the runtime
// sources is represented here.

export const REQUIRED_PLAY_WASM_EXPORTS = Object.freeze([
  'memory',
  'game_activate_player',
  'game_active_player_mask',
  'game_capabilities',
  'game_capacity',
  'game_check_wcell',
  'game_cmd_capacity',
  'game_cmd_ptr',
  'game_commands_seen',
  'game_create',
  'game_debug_spawn',
  'game_destroy',
  'game_digest_hi',
  'game_digest_lo',
  'game_diplomacy',
  'game_error_len',
  'game_error_ptr',
  'game_frame',
  'game_gamedata_alloc',
  'game_gap_count',
  'game_gaps_ptr',
  'game_has_gamedata',
  'game_has_playdata',
  'game_id_at_row',
  'game_info_ptr',
  'game_is_over',
  'game_leader_flags',
  'game_live',
  'game_load_alloc',
  'game_load_commit',
  'game_map_span',
  'game_map_tiles',
  'game_object_info',
  'game_object_command_identity',
  'game_object_command_identity_ptr',
  'game_orders_applied',
  'game_package_receipt_ptr',
  'game_package_receipt_words',
  'game_pick_at',
  'game_pick_box',
  'game_pick_ptr',
  'game_pick_type',
  'game_placement_grade',
  'game_playable_blocker_count',
  'game_playable_blocker_slug_len',
  'game_playable_blocker_slug_ptr',
  'game_playable_blocker_title_len',
  'game_playable_blocker_title_ptr',
  'game_playdata_alloc',
  'game_player_fields',
  'game_players_count',
  'game_players_ptr',
  'game_products',
  'game_products_ptr',
  'game_process_command_package',
  'game_rng_state',
  'game_save',
  'game_save_len',
  'game_save_limit',
  'game_save_ptr',
  'game_seed',
  'game_set_income_mode',
  'game_set_pop_setting',
  'game_start_x',
  'game_start_y',
  'game_start_manual_teams',
  'game_step',
  'game_submit',
  'game_subtile',
  'game_tag_ptr',
  'game_team',
  'game_team_configured_mask',
  'game_team_score',
  'game_team_style',
  'game_terrain_version',
  'game_tile_ptr',
  'game_tile_resource',
  'game_victory_mode',
  'game_victory_score',
  'game_x_ptr',
  'game_y_ptr',
]);

// Raw individual setters remain forbidden. Team setup is admitted only through the atomic
// frame-zero `game_start_manual_teams` transaction; victory has no corresponding owner yet.
export const FORBIDDEN_PLAY_WASM_EXPORTS = Object.freeze([
  'game_set_team',
  'game_set_victory_mode',
]);

export function assertPlayWasmContract(exports) {
  const missing = REQUIRED_PLAY_WASM_EXPORTS.filter((name) => !(name in exports));
  const forbidden = FORBIDDEN_PLAY_WASM_EXPORTS.filter((name) => name in exports);
  const wrongKind = REQUIRED_PLAY_WASM_EXPORTS.filter((name) => name in exports &&
    (name === 'memory'
      ? !(exports[name] instanceof WebAssembly.Memory)
      : typeof exports[name] !== 'function'));
  if (missing.length || forbidden.length || wrongKind.length) {
    const details = [
      missing.length ? `missing ${missing.join(', ')}` : '',
      forbidden.length ? `forbidden ${forbidden.join(', ')}` : '',
      wrongKind.length ? `wrong-kind ${wrongKind.join(', ')}` : '',
    ].filter(Boolean).join('; ');
    throw new Error(`stale or unsupported don_web.wasm ABI: ${details}`);
  }
}
