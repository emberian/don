import importlib.util
import json
import copy
from pathlib import Path
import tempfile
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location("retailctl", Path(__file__).with_name("retailctl.py"))
assert SPEC and SPEC.loader
retailctl = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(retailctl)


class RetailCtlTests(unittest.TestCase):
    def test_coord_lookup_global_is_dereferenced_before_indexing(self):
        source = Path(__file__).with_name("retail_control.c").read_text()
        self.assertNotIn("g_base + RVA_COORD_LOOKUP +", source)
        self.assertIn(
            "rd32(g_base + RVA_COORD_LOOKUP, &coord_lookup)", source
        )

    def test_live_stop_restores_on_the_retail_callback_before_worker_fallback(self):
        source = Path(__file__).with_name("retail_control.c").read_text()
        callback = source[source.index("static void __cdecl on_turn_frame"):
                          source.index("static BYTE *emit8")]
        removal = source[source.index("static int remove_hook(void)"):
                         source.index("static int image_supported")]
        self.assertIn("write_code(g_hook_addr, g_hook_orig, 5)", callback)
        self.assertIn("InterlockedExchange(&g_stop_ack, 1)", callback)
        self.assertLess(removal.index("g_stop_ack"), removal.index("quiesce"))
        self.assertNotIn("WriteProcessMemory", source)

    def test_controller_generations_have_isolated_paths_and_unique_dll_names(self):
        self.assertEqual(
            retailctl.generation_root("v1"), r"C:\Users\Public\don-retail-control"
        )
        self.assertEqual(
            retailctl.generation_root("trajectory-2"),
            r"C:\Users\Public\don-retail-control-trajectory-2",
        )
        self.assertEqual(retailctl.generation_dll("v1"), "retail_control.dll")
        self.assertEqual(
            retailctl.generation_dll("trajectory-2"), "retail_control-trajectory-2.dll"
        )

    def test_unsafe_controller_generation_is_refused(self):
        for generation in ["", "../shared", "v2 & whoami", "has space", "x" * 49]:
            with self.assertRaises(SystemExit):
                retailctl.generation_root(generation)

    def test_process_absence_and_tasklist_noise_are_reported_cleanly(self):
        with mock.patch.object(
            retailctl, "guest_cmd_status",
            return_value=(0, "INFO: No tasks are running which match the specified criteria."),
        ):
            self.assertEqual(retailctl.process_pids()[0], [])
        with mock.patch.object(
            retailctl, "guest_cmd_status",
            return_value=(0, "Parallels noise\n12324\n7804\n12324\n"),
        ):
            self.assertEqual(retailctl.process_pids()[0], [7804, 12324])
        with mock.patch.object(
            retailctl, "guest_cmd_status", return_value=(1, "VM unavailable")
        ):
            with self.assertRaisesRegex(RuntimeError, "could not enumerate"):
                retailctl.process_pids()

    def test_donject_v2_module_records_are_strict_and_legacy_zero_is_indeterminate(self):
        mapped = (
            'protocol=donject.v2 command=base status=mapped pid=12324 '
            'module_name="retail_control-tactical-v21.dll" '
            'module_path="C:\\Users\\Public\\don-retail-control-tactical-v21\\'
            'retail_control-tactical-v21.dll" module_base=0x6AF00000 '
            'module_size=0x00023000'
        )
        self.assertEqual(
            retailctl.parse_module_base_output(mapped)["base"], 0x6AF00000
        )
        absent = (
            'protocol=donject.v2 command=base status=absent pid=12324 '
            'module_name="retail_control-next.dll"'
        )
        self.assertEqual(retailctl.parse_module_base_output(absent)["status"], "absent")
        explicit_error = (
            "protocol=donject.v2 command=base status=error stage=module-snapshot "
            "pid=12324 module_name=retail_control-next.dll win32_error=5"
        )
        self.assertEqual(
            retailctl.parse_module_base_output(explicit_error)["status"], "error"
        )
        decimal_size = mapped.replace("module_size=0x00023000", "module_size=143360")
        self.assertEqual(
            retailctl.parse_module_base_output(decimal_size)["status"], "error"
        )
        self.assertEqual(retailctl.parse_module_base_output("00000000")["status"], "error")
        duplicate = mapped + "\n" + mapped
        self.assertEqual(retailctl.parse_module_base_output(duplicate)["status"], "error")

    def test_injector_prepare_is_atomic_hash_bound_and_selftested(self):
        digest = "b" * 64
        server = mock.Mock()
        commands = []

        def record_command(command, **_kwargs):
            commands.append(command)
            return ""

        with (
            mock.patch.object(retailctl, "build_injector", return_value=digest),
            mock.patch.object(retailctl, "serve_once", return_value=server),
            mock.patch.object(retailctl, "guest_cmd", side_effect=record_command),
            mock.patch.object(retailctl, "guest_sha256", side_effect=[digest, digest]),
            mock.patch.object(
                retailctl, "guest_cmd_status",
                return_value=(0, "selftest: status=ok architecture=PE32/i386"),
            ),
            mock.patch("builtins.print"),
        ):
            result = retailctl.prepare_injector()
        self.assertTrue(result["ready"])
        curl_index = next(i for i, command in enumerate(commands) if "curl.exe" in command)
        move_index = next(i for i, command in enumerate(commands) if "move /y" in command)
        self.assertLess(curl_index, move_index)
        server.shutdown.assert_called_once()
        server.server_close.assert_called_once()

        commands.clear()
        server = mock.Mock()
        with (
            mock.patch.object(retailctl, "build_injector", return_value=digest),
            mock.patch.object(retailctl, "serve_once", return_value=server),
            mock.patch.object(retailctl, "guest_cmd", side_effect=record_command),
            mock.patch.object(retailctl, "guest_sha256", return_value="c" * 64),
        ):
            with self.assertRaisesRegex(SystemExit, "download hash"):
                retailctl.prepare_injector()
        self.assertFalse(any("move /y" in command for command in commands))

    def test_strict_injector_build_is_byte_reproducible(self):
        with tempfile.TemporaryDirectory() as directory:
            first = Path(directory) / "first.exe"
            second = Path(directory) / "second.exe"
            first_hash = retailctl.build_injector(first)
            second_hash = retailctl.build_injector(second)
            self.assertEqual(first_hash, second_hash)
            self.assertEqual(first.read_bytes(), second.read_bytes())

    def test_module_probe_binds_explicit_status_to_exit_pid_and_name(self):
        record = (
            'protocol=donject.v2 command=base status=absent pid=12324 '
            'module_name="retail_control-next.dll"'
        )
        with mock.patch.object(retailctl, "guest_cmd_status", return_value=(10, record)):
            probe = retailctl.module_probe(12324, "retail_control-next.dll")
        self.assertEqual(probe["status"], "absent")
        with mock.patch.object(retailctl, "guest_cmd_status", return_value=(0, record)):
            probe = retailctl.module_probe(12324, "retail_control-next.dll")
        self.assertEqual(probe["status"], "error")
        with mock.patch.object(retailctl, "module_probe", return_value=probe):
            with self.assertRaisesRegex(SystemExit, "indeterminate"):
                retailctl.loaded_module(12324, "retail_control-next.dll")

    def test_donject_v2_full_module_list_is_counted_and_identity_bound(self):
        captured = "\n".join([
            "unrelated prlctl noise",
            "protocol=donject.v2 command=modules status=ok pid=12324 count=2",
            'protocol=donject.v2 command=modules status=module pid=12324 index=0 '
            'module_name="riseofnations.exe" module_path="C:\\Game\\riseofnations.exe" '
            'module_base=0x00D60000 module_size=0x00BB4000',
            'protocol=donject.v2 command=modules status=module pid=12324 index=1 '
            'module_name="retail_control-v2.dll" '
            'module_path="C:\\Users\\Public\\don-retail-control-v2\\retail_control-v2.dll" '
            'module_base=0x6AF00000 module_size=0x00023000',
        ])
        parsed = retailctl.parse_module_list_output(captured)
        self.assertEqual(parsed["status"], "ok")
        self.assertEqual(parsed["pid"], 12324)
        self.assertEqual(parsed["modules"][1]["base_hex"], "0x6af00000")
        self.assertEqual(parsed["modules"][1]["size"], 0x23000)
        incomplete = captured.replace("count=2", "count=3")
        self.assertEqual(retailctl.parse_module_list_output(incomplete)["status"], "error")
        with mock.patch.object(retailctl, "guest_cmd_status", return_value=(12, captured)):
            self.assertEqual(retailctl.remote_modules(12324)["status"], "error")

    def test_injector_postcondition_rejects_timeout_and_already_loaded_race(self):
        name = "retail_control-fresh-v1.dll"
        path = rf"C:\Users\Public\don-retail-control-fresh-v1\{name}"
        canonical_path = "\\\\?\\" + path
        dll_hash = "a" * 64
        loaded = (
            f"inject: result=loaded status=ok pid=12324 module={name} "
            f"base=6AF00000 path={canonical_path} sha256={dll_hash}"
        )
        result = retailctl.parse_inject_output(
            loaded, 0, 12324, name, path, dll_hash
        )
        self.assertEqual(result["status"], "loaded")
        self.assertEqual(result["module_base"], 0x6AF00000)
        already = loaded.replace("result=loaded", "result=already-loaded")
        self.assertEqual(
            retailctl.parse_inject_output(
                already, 0, 12324, name, path, dll_hash
            )["status"],
            "error",
        )
        timeout = (
            "inject: INDETERMINATE remote-thread-timeout wait_ms=15000 "
            "remote_path=01230000 allocation=retained target_state=tainted "
            "restart_required=1"
        )
        result = retailctl.parse_inject_output(timeout, 15, 12324, name, path, dll_hash)
        self.assertEqual(result["status"], "indeterminate")
        self.assertTrue(result["restart_required"])

    def test_deployed_generation_capture_rejects_marker_and_ready_noise(self):
        payload = [{
            "root_name": "don-retail-control-tactical-v21",
            "root": r"C:\Users\Public\don-retail-control-tactical-v21",
            "dlls": ["retail_control-tactical-v21.dll"],
            "downloads": [],
            "ready": (
                "state=parked\r\npid=7804\r\n"
                "root=C:\\Users\\Public\\don-retail-control-tactical-v21\r\n"
                "base=0x00d60000\r\nturn_call_site=0x00ef1686\r\n"
                "turn_do_frame=0x012b7dd0\r\n"
            ),
        }]
        captured = (
            "unrelated guest noise\n" + retailctl.PREFLIGHT_JSON_BEGIN + "\n" +
            json.dumps(payload) + "\n" + retailctl.PREFLIGHT_JSON_END + "\nmore noise"
        )
        rows, errors = retailctl.parse_deployed_generations(captured)
        self.assertEqual(errors, [])
        self.assertEqual(rows[0]["generation"], "tactical-v21")
        self.assertEqual(rows[0]["ready"]["values"]["state"], "parked")
        with self.assertRaises(ValueError):
            retailctl.parse_deployed_generations(captured + "\n" + captured)

    def test_hook_probe_distinguishes_original_patch_and_unrecognized_bytes(self):
        original = (
            "# base=00D60000 addr=00EF1686 len=5\n"
            "00EF1686: E8 45 67 3C 00"
        )
        self.assertEqual(retailctl.parse_hook_peek_output(original)["status"], "original")
        address = 0x00EF1686
        target = 0x6A001000
        displacement = target - (address + 5)
        patched_bytes = b"\xe8" + displacement.to_bytes(4, "little", signed=True)
        patched = (
            "# base=00D60000 addr=00EF1686 len=5\n"
            f"00EF1686: {' '.join(f'{byte:02X}' for byte in patched_bytes)}"
        )
        parsed = retailctl.parse_hook_peek_output(patched)
        self.assertEqual(parsed["status"], "patched")
        self.assertEqual(parsed["call_target"], "0x6a001000")
        unknown = original.replace("E8 45 67 3C 00", "90 90 90 90 90")
        self.assertEqual(retailctl.parse_hook_peek_output(unknown)["status"], "unknown")
        self.assertEqual(
            retailctl.parse_hook_peek_output(original + "\n" + original)["status"],
            "unreadable",
        )

    def test_ready_record_is_bound_to_pid_root_and_exact_rebased_addresses(self):
        root = r"C:\Users\Public\don-retail-control-tactical-v21"
        raw = (
            f"state=parked\npid=7804\nroot={root}\nbase=0x00d60000\n"
            "turn_call_site=0x00ef1686\nturn_do_frame=0x012b7dd0\n"
        )
        record = retailctl.parse_ready_record(raw)
        self.assertEqual(retailctl.ready_identity_errors(record, 7804, root), [])
        self.assertIn(
            "ready pid does not match target",
            retailctl.ready_identity_errors(record, 12324, root),
        )
        torn = retailctl.parse_ready_record(raw + "pid=7804\n")
        self.assertTrue(torn["errors"])

    def test_retail_requests_require_exact_current_hook_ownership(self):
        root = retailctl.generation_root("fresh-v1")
        exact = {
            "complete": True,
            "issues": [],
            "hook_owner": "fresh-v1",
        }
        with (
            mock.patch.object(
                retailctl, "injector_diagnostic", return_value={"ready": True, "issues": []}
            ),
            mock.patch.object(retailctl, "pid", return_value=12324),
            mock.patch.object(retailctl, "preflight"),
            mock.patch.object(retailctl, "controller_inventory", return_value=exact),
        ):
            self.assertEqual(retailctl.require_armed_controller(root), (12324, "fresh-v1"))
        wrong = {**exact, "hook_owner": "other-v1"}
        with (
            mock.patch.object(
                retailctl, "injector_diagnostic", return_value={"ready": True, "issues": []}
            ),
            mock.patch.object(retailctl, "pid", return_value=12324),
            mock.patch.object(retailctl, "preflight"),
            mock.patch.object(retailctl, "controller_inventory", return_value=wrong),
        ):
            with self.assertRaisesRegex(SystemExit, "ownership is not exact"):
                retailctl.require_armed_controller(root)

    def test_refused_stop_is_not_substring_misread_as_parked(self):
        root = r"C:\Users\Public\don-retail-control-tactical-v21"
        raw = (
            f"state=refused-stop\npid=7804\nroot={root}\nbase=0x00d60000\n"
            "turn_call_site=0x00ef1686\nturn_do_frame=0x012b7dd0\n"
        )
        with mock.patch.object(
            retailctl, "read_ready", return_value=(raw, retailctl.parse_ready_record(raw))
        ):
            with self.assertRaisesRegex(SystemExit, "refused-stop"):
                retailctl.wait_for_ready_state(root, 7804, "parked", 0.1)

    def test_generation_budget_refuses_projected_mapping_before_deploy(self):
        inventory = {
            "complete": True,
            "mapped_generation_count": 4,
            "mapped_modules": [
                {"name": f"retail_control-old-{i}.dll"} for i in range(4)
            ],
            "issues": [],
        }
        with mock.patch.object(retailctl, "controller_inventory", return_value=inventory):
            with self.assertRaisesRegex(SystemExit, "configured maximum 4"):
                retailctl.enforce_generation_budget(12324, "next", 4)

    def test_deployment_refuses_incomplete_inventory_and_an_owned_hook(self):
        incomplete = {
            "complete": False,
            "mapped_generation_count": 0,
            "mapped_modules": [],
            "issues": ["could not determine module state for retail_control.dll"],
        }
        with mock.patch.object(retailctl, "controller_inventory", return_value=incomplete):
            with self.assertRaisesRegex(SystemExit, "inventory is incomplete"):
                retailctl.enforce_generation_budget(12324, "next", 4)
        owned = {
            "complete": True,
            "mapped_generation_count": 1,
            "mapped_modules": [{"name": "retail_control-old.dll"}],
            "issues": [],
            "hook": {"status": "patched"},
        }
        with mock.patch.object(retailctl, "controller_inventory", return_value=owned):
            with self.assertRaisesRegex(SystemExit, "another controller owns"):
                retailctl.enforce_generation_budget(12324, "next", 4)

    def test_inventory_flags_stale_armed_ready_against_original_hook(self):
        root = r"C:\Users\Public\don-retail-control-tactical-v21"
        ready = retailctl.parse_ready_record(
            f"state=armed\npid=7804\nroot={root}\nbase=0x00d60000\n"
            "turn_call_site=0x00ef1686\nturn_do_frame=0x012b7dd0\n"
        )
        row = {
            "generation": "tactical-v21",
            "root": root,
            "expected_dll": "retail_control-tactical-v21.dll",
            "dlls": ["retail_control-tactical-v21.dll"],
            "downloads": [],
            "ready": ready,
        }
        mapped = {
            "status": "mapped", "name": "retail_control-tactical-v21.dll",
            "path": root + r"\retail_control-tactical-v21.dll",
            "base": 0x6AF00000, "base_hex": "0x6af00000",
            "size": 0x23000, "size_hex": "0x23000",
        }
        with (
            mock.patch.object(retailctl, "deployed_generations", return_value=([row], [])),
            mock.patch.object(
                retailctl, "remote_modules",
                return_value={"status": "ok", "pid": 7804, "modules": [mapped]},
            ),
            mock.patch.object(retailctl, "hook_call_state", return_value={"status": "original"}),
        ):
            inventory = retailctl.controller_inventory(7804)
        self.assertIn(
            "armed ready record conflicts with original retail call bytes",
            inventory["issues"],
        )

    def test_inventory_does_not_promote_historical_unmapped_ready_noise(self):
        row = {
            "generation": "v1",
            "root": retailctl.generation_root("v1"),
            "expected_dll": retailctl.generation_dll("v1"),
            "dlls": [retailctl.generation_dll("v1")],
            "downloads": [],
            "ready": retailctl.parse_ready_record(
                "state=armed\npid=12324\nbase=0x00d60000\n"
                "turn_call_site=0x00ef1686\nturn_do_frame=0x012b7dd0\n"
            ),
        }
        with (
            mock.patch.object(retailctl, "deployed_generations", return_value=([row], [])),
            mock.patch.object(
                retailctl, "remote_modules",
                return_value={"status": "ok", "pid": 7804, "modules": []},
            ),
            mock.patch.object(retailctl, "hook_call_state", return_value={"status": "original"}),
        ):
            inventory = retailctl.controller_inventory(7804)
        self.assertEqual(inventory["issues"], [])
        self.assertTrue(inventory["complete"])

    def test_wer_diagnostics_require_both_scoped_views_full_dumps_and_free_space(self):
        views = [{
            "view": view,
            "present": True,
            "folder": retailctl.EXPECTED_DUMP_FOLDER,
            "expanded_folder": retailctl.EXPECTED_DUMP_FOLDER,
            "folder_exists": True,
            "dump_type": 2,
            "dump_count": 2,
        } for view in ["64", "32"]]
        payload = {
            "views": views,
            "free_bytes": retailctl.MIN_DUMP_FREE_BYTES,
            "wer_service_status": "Stopped",
        }
        captured = (
            retailctl.PREFLIGHT_JSON_BEGIN + "\n" + json.dumps(payload) + "\n" +
            retailctl.PREFLIGHT_JSON_END
        )
        diagnostic = retailctl.parse_wer_diagnostics(captured)
        self.assertTrue(diagnostic["ready"])
        self.assertIn("informational", diagnostic["wer_service_note"])
        payload["views"][1]["dump_type"] = 1
        payload["free_bytes"] -= 1
        captured = (
            retailctl.PREFLIGHT_JSON_BEGIN + "\n" + json.dumps(payload) + "\n" +
            retailctl.PREFLIGHT_JSON_END
        )
        diagnostic = retailctl.parse_wer_diagnostics(captured)
        self.assertFalse(diagnostic["ready"])
        self.assertTrue(any("registry view 32" in issue for issue in diagnostic["issues"]))

    def test_prelaunch_report_treats_absent_retail_as_a_clean_process_state(self):
        wer = {"ready": True, "issues": []}
        with (
            mock.patch.object(
                retailctl, "injector_diagnostic", return_value={"ready": True, "issues": []}
            ),
            mock.patch.object(retailctl, "process_pids", return_value=([], "no tasks")),
            mock.patch.object(retailctl, "deployed_generations", return_value=([], [])),
            mock.patch.object(retailctl, "wer_diagnostics", return_value=wer),
        ):
            report = retailctl.prelaunch_report(4)
        self.assertEqual(report["process"]["status"], "absent")
        self.assertEqual(report["controllers"]["hook"]["status"], "not-applicable")
        self.assertTrue(report["ready"])

    def test_command_tokens_accept_the_documented_protocol(self):
        for words in [
            ["observe"], ["observe-network"], ["pause", "1"], ["speed", "3"],
            ["speed-up"],
            ["halt", "0", "12", "13"],
            ["move", "0", "100", "200", "2", "1", "-1", "-1", "0", "12"],
            ["attack", "0", "1", "22", "0", "2", "12", "13"],
            ["attack-visible", "0", "1", "22", "37", "0", "2", "12"],
            ["trace-move", "0", "12", "100", "200", "120"],
            ["observe-guys", "0", "12"],
            ["observe-player"], ["validate-queue", "0", "2000", "50"],
            ["validate-build", "0", "1", "2", "3", "4", "427", "2", "3"],
            ["gather", "0", "2001", "2", "3"],
            ["queue", "0", "50", "1", "2000"],
            ["build", "0", "1", "2", "3", "4", "427", "2", "3"],
            ["find-build", "0", "1488", "32544", "8", "417", "3"],
            ["find-gather-build", "0", "2496", "30144", "12", "12", "418", "3"],
            ["find-scout-step", "0", "3", "2496", "30144"],
            ["validate-attack", "0", "12", "1", "22", "37"],
            ["run-frames", "30"],
        ]:
            retailctl.validate_words(words)

    def test_shell_metacharacters_are_refused(self):
        for words in [["observe&whoami"], ["pause", "1>pwn"], ["move", "$(x)"]]:
            with self.assertRaises(SystemExit):
                retailctl.validate_words(words)

    def test_unknown_verb_is_refused(self):
        with self.assertRaises(SystemExit):
            retailctl.validate_words(["cheat"])
        with self.assertRaises(SystemExit):
            retailctl.validate_words(["checksum"])

    def test_multiplayer_observation_is_passive_and_checksum_capture_is_strict(self):
        network = {
            "phase": "observed",
            "checksum_gate": "not_connected",
            "mutates_outgoing_package": 0,
            "network_last_num_received": [0] * 8,
            "network_peer_checksums": [0] * 8,
            "checksum_capture_valid": 0,
        }
        retailctl.validate_multiplayer_events("observe-network", [network])
        with self.assertRaisesRegex(SystemExit, "passive observation"):
            retailctl.validate_multiplayer_events(
                "observe-network", [{**network, "checksum_capture_valid": 1}]
            )
        checksum = {
            **network,
            "phase": "queued",
            "checksum_gate": "eligible",
            "mutates_outgoing_package": 1,
            "checksum_capture_valid": 1,
            "checksum_total_consistent": 1,
            "checksum_adler_shaped": 1,
            "checksum_words": list(range(16)),
        }
        retailctl.validate_multiplayer_events("checksum", [checksum])
        with self.assertRaisesRegex(SystemExit, "complete retail capture"):
            retailctl.validate_multiplayer_events(
                "checksum", [{**checksum, "checksum_total_consistent": 0}]
            )

    def test_tactical_move_and_attack_replay_exact_public_identities(self):
        observation = json.loads(
            (Path(__file__).parents[2] / "schema/live/retail-player-observation-v3-post-camp.json")
            .read_text()
        )
        observation["protocol"] = "don.retail-player.v4"
        actor = next(obj for obj in observation["objects"] if obj["category"] == "unit")
        move = {
            "verb": "move", "owner": 0, "object_ids": [actor["object_id"]],
            "actor": actor["id"], "target": {"x": 1920, "y": 2112},
            "queue": 2, "order": 1, "form": -1, "width": -1, "disembark": 0,
            "scout_evidence": {"policy_goal": {"coord_x": 3000, "coord_y": 4000}},
        }
        frontier = {"accepted": True, "target": move["target"]}
        with mock.patch.object(retailctl, "scout_step_validation",
                               return_value=frontier) as replay:
            result = retailctl.validate_tactical_action(move, observation, "unused")
        self.assertEqual(result["validation_result"], 1)
        replay.assert_called_once_with(
            "unused", observation, actor["object_id"], 3000, 4000
        )
        self.assertEqual(retailctl.tactical_action_words(move), [
            "move", "0", "1920", "2112", "2", "1", "-1", "-1", "0",
            str(actor["object_id"]),
        ])

        target = {
            "id": {"slot": 1, "band": "unit", "o": 7, "uid": 91},
            "owner": 1, "object_id": 7, "category": "unit",
        }
        observation["visible_enemies"] = [target]
        attack = {
            "verb": "attack", "owner": 0, "object_ids": [actor["object_id"]],
            "actor": actor["id"], "target": target["id"], "target_owner": 1,
            "target_id": 7, "target_uid": 91, "flags": 0, "queue": 2,
        }
        with mock.patch.object(retailctl, "visible_attack_validation",
                               return_value={"accepted": True}) as replay:
            result = retailctl.validate_tactical_action(attack, observation, "unused")
        self.assertEqual(result["validation_result"], 1)
        replay.assert_called_once_with("unused", observation, actor["object_id"], target)
        self.assertEqual(retailctl.tactical_action_words(attack), [
            "attack-visible", "0", "1", "7", "91", "0", "2",
            str(actor["object_id"]),
        ])

    def test_tactical_proof_requires_exact_applied_move_order(self):
        before = json.loads(
            (Path(__file__).parents[2] / "schema/live/retail-player-observation-v3-post-camp.json")
            .read_text()
        )
        before["protocol"] = "don.retail-player.v4"
        before["visible_enemies"] = []
        actor = next(obj for obj in before["objects"] if obj["category"] == "unit")
        action = {
            "verb": "move", "owner": 0, "object_ids": [actor["object_id"]],
            "actor": actor["id"], "target": {"x": 1920, "y": 2112},
        }
        after = copy.deepcopy(before)
        next(obj for obj in after["objects"]
             if obj["object_id"] == actor["object_id"])["order"]["kind"] = "MoveOrder"
        events = [
            {"phase": "queued", "paused": 1, "command_hex": "0007"},
            {"phase": "applied", "paused": 1, "move_valid": 1,
             "move_x": 1920, "move_y": 2112},
        ]
        with tempfile.TemporaryDirectory() as td, \
                mock.patch.object(retailctl, "player_observation",
                                  side_effect=[before, after]), \
                mock.patch.object(retailctl, "validate_tactical_action",
                                  return_value={"validation_result": 1}), \
                mock.patch.object(retailctl, "tactical_action_words",
                                  return_value=["move"]), \
                mock.patch.object(retailctl, "send", return_value=events):
            proof = retailctl.prove_tactical_action(
                "unused", "test-generation", action, Path(td) / "proof.json", before
            )
        self.assertEqual(proof["schema"], "don.retail-tactical-action-proof.v1")
        self.assertEqual(proof["frame_boundary"]["before"], proof["frame_boundary"]["after"])
        self.assertEqual(proof["pause_before_after"], [1, 1])

    def test_marshal_attack_requires_persistent_push_state_and_visible_target(self):
        observation = json.loads(
            (Path(__file__).parents[2] / "schema/live/retail-player-observation-v3-post-camp.json")
            .read_text()
        )
        actor = next(obj for obj in observation["objects"] if obj["category"] == "unit")
        actor["type_index"] = 351
        target = {
            "id": {"slot": 1, "band": "build", "o": 2000, "uid": 44},
            "owner": 1, "object_id": 2000, "category": "build", "type_index": 414,
            "position": {"x": 30000, "y": 30000},
        }
        observation["visible_enemies"] = [target]
        state = {"mode": "Massing", "enemy_base": {"tile_x": 156, "tile_y": 156}}
        rows = {351: {"is_military": True, "value": 420, "cat": 0},
                414: {"is_military": False, "value": 0, "cat": 0}}
        with mock.patch.object(retailctl, "live_unit_policy_rows", return_value=rows), \
                mock.patch.object(retailctl, "visible_attack_validation",
                                  return_value={"accepted": True}):
            trace, action = retailctl.marshal_army_tactical_action(
                observation, "unused", state
            )
            self.assertEqual(trace["result"], "state-transition")
            self.assertIsNone(action)
            self.assertEqual(state["mode"], "Pushing")
            trace, action = retailctl.marshal_army_tactical_action(
                observation, "unused", state
            )
        self.assertEqual(trace["result"], "emit")
        self.assertEqual(action["verb"], "attack")
        self.assertEqual(action["target"], target["id"])

    def test_live_trajectory_artifact_is_bounded_and_aslr_normalized(self):
        artifact = json.loads(
            (Path(__file__).parents[2] / "schema/live/retail-move-trajectory-v1.json")
            .read_text()
        )
        self.assertEqual(artifact["schema"], "don.retail-move-trajectory.v1")
        self.assertEqual(artifact["termination"], "trace-complete")
        self.assertEqual(artifact["terminal"]["pause"], 1)
        self.assertEqual(artifact["terminal"]["order"]["length"], 0)
        self.assertEqual(
            [sample["position"]["x"] for sample in artifact["samples"]],
            [2938, 2972, 3006, 3040, 3074, 3096],
        )
        self.assertTrue(all(
            sample["order"]["preferred_vtable"] == "0x00b4a12c"
            for sample in artifact["samples"][:-1]
        ))
        self.assertFalse(artifact["checksum"]["available"])

    def test_player_observation_is_own_state_only_and_names_exact_retail_types(self):
        observation = json.loads(
            (Path(__file__).parents[2] / "schema/live/retail-player-observation-v1.json")
            .read_text()
        )
        self.assertEqual(observation["public_scope"]["owner"], 0)
        self.assertIn("enemy and neutral object tables", observation["public_scope"]["excludes"])
        self.assertEqual(len(observation["objects"]), 15)
        self.assertNotIn("pointer", observation["objects"][0])
        self.assertEqual(observation["population"], {"current": 8, "cap": 25})
        self.assertEqual(observation["economy"]["stockpile_i32"], [214, 210, 103, 0, 0, 0])
        self.assertNotIn("resource_caps", observation["economy"])
        by_id = {obj["object_id"]: obj for obj in observation["objects"]}
        self.assertEqual(by_id[0]["type_name"], "Scout")
        self.assertEqual(by_id[0]["id"], {"slot": 0, "band": "unit", "o": 0, "uid": 7})
        self.assertEqual(by_id[3]["type_name"], "Citizen")
        self.assertEqual(by_id[2000]["type_name"], "Small City")
        self.assertEqual(by_id[3]["order"]["kind"], "GatherOrder")
        self.assertEqual(by_id[3]["order"]["index"], 7)
        self.assertNotIn("order", by_id[2000])
        self.assertTrue(all(obj["runtime_class"] != "unknown" for obj in by_id.values()))

    def test_scout_policy_is_deterministic_and_never_retasks_citizens(self):
        observation = json.loads(
            (Path(__file__).parents[2] / "schema/live/retail-player-observation-v1.json")
            .read_text()
        )
        batch = retailctl.scout_policy(observation)
        self.assertEqual(len(batch["actions"]), 1)
        self.assertEqual(batch["actions"][0]["object_ids"], [0])
        self.assertEqual(batch["actions"][0]["target"], {"x": 3480, "y": 31896})
        retailctl.validate_action_batch(batch, observation)
        chosen = set(batch["actions"][0]["object_ids"])
        citizen_ids = {obj["object_id"] for obj in observation["objects"]
                       if obj["type_name"] == "Citizen"}
        self.assertTrue(chosen.isdisjoint(citizen_ids))

        busy = copy.deepcopy(observation)
        next(obj for obj in busy["objects"] if obj["object_id"] == 0)["order"]["length"] = 1
        self.assertEqual(retailctl.scout_policy(busy)["actions"], [])

    def test_live_player_run_is_bounded_and_restores_pause(self):
        run = json.loads(
            (Path(__file__).parents[2] / "schema/live/retail-player-policy-run-v1.json")
            .read_text()
        )
        self.assertEqual(run["mode"], "apply")
        self.assertEqual(len(run["action_batch"]["actions"]), 1)
        self.assertEqual(run["traces"][0]["termination"], "trace-complete")
        self.assertEqual(run["traces"][0]["terminal"]["pause"], 1)
        self.assertEqual(
            [sample["position"]["x"] for sample in run["traces"][0]["samples"]],
            [3322, 3356, 3390, 3424, 3458, 3480],
        )
        before = next(obj for obj in run["before"]["objects"] if obj["object_id"] == 0)
        after = next(obj for obj in run["after"]["objects"] if obj["object_id"] == 0)
        self.assertEqual(before["position"]["x"], 3288)
        self.assertEqual(after["position"]["x"], 3480)
        self.assertEqual(after["order"]["length"], 0)

    def test_player_protocol_contract_records_exact_scope_and_limits(self):
        protocol = json.loads(
            (Path(__file__).parents[2] / "schema/live/retail-player-protocol-v1.json")
            .read_text()
        )
        self.assertEqual(protocol["protocol"], "don.retail-player.v1")
        self.assertEqual(protocol["action_batch"]["max_actions"], 4)
        self.assertEqual(protocol["observation"]["objects"]["bands"]["build"],
                         "[2000, build_mark)")
        self.assertIn("all enemy and neutral owner lists", protocol["observation"]["excluded"])

    def test_action_batch_rejects_wrong_owner_and_world_escape(self):
        observation = json.loads(
            (Path(__file__).parents[2] / "schema/live/retail-player-observation-v1.json")
            .read_text()
        )
        batch = retailctl.scout_policy(observation)
        wrong_owner = copy.deepcopy(batch)
        wrong_owner["actions"][0]["owner"] = 1
        with self.assertRaises(RuntimeError):
            retailctl.validate_action_batch(wrong_owner, observation)
        escape = copy.deepcopy(batch)
        escape["actions"][0]["target"]["x"] = observation["world"]["tile_xs"] * 192
        with self.assertRaises(RuntimeError):
            retailctl.validate_action_batch(escape, observation)

    def test_v2_observation_is_fog_safe_and_exposes_exact_own_queues(self):
        observation = json.loads(
            (Path(__file__).parents[2] / "schema/live/retail-player-observation-v2.json")
            .read_text()
        )
        self.assertEqual(observation["protocol"], "don.retail-player.v2")
        self.assertEqual(observation["paused"], 1)
        self.assertIn("enemy and neutral object tables", observation["public_scope"]["excludes"])
        self.assertTrue(all("pointer" not in obj for obj in observation["objects"]))
        city = next(obj for obj in observation["objects"] if obj["object_id"] == 2000)
        self.assertEqual(city["production_queue"]["logical_length"], 1)
        self.assertEqual(city["production_queue"]["items"][0]["type_name"], "Citizen")
        library = next(obj for obj in observation["objects"] if obj["object_id"] == 2005)
        self.assertEqual(library["production_queue"]["items"][0]["type_name"], "City State")
        gatherer = next(obj for obj in observation["objects"] if obj["object_id"] == 3)
        self.assertEqual(gatherer["order"]["own_target"]["object_id"], 2001)

    def test_live_economy_queue_research_and_gather_proofs_are_bounded(self):
        root = Path(__file__).parents[2] / "schema/live"
        queue = json.loads((root / "retail-economy-action-proof-v1.json").read_text())
        self.assertEqual(bytes.fromhex(queue["retail_command_hex"])[3], 0x18)
        self.assertEqual(queue["pause_before_after"], [1, 1])
        self.assertEqual(queue["before"]["frame"], queue["after"]["frame"])
        research = json.loads((root / "retail-economy-research-proof-v1.json").read_text())
        self.assertEqual(research["action"]["type_name"], "City State")
        self.assertEqual(research["pause_before_after"], [1, 1])
        self.assertIn("18", research["retail_command_hex"])
        gather = json.loads((root / "retail-economy-gather-proof-v1.json").read_text())
        self.assertEqual(gather["pause_before_after"], [1, 1])
        self.assertEqual(gather["after"]["objects"][3]["order"]["own_target"]["object_id"], 2001)

    def test_build_attempt_is_not_misrepresented_as_positive_proof(self):
        attempt = json.loads(
            (Path(__file__).parents[2] / "schema/live/retail-economy-build-attempt-v1.json")
            .read_text()
        )
        self.assertFalse(attempt["positive_proof"])
        self.assertEqual(attempt["pause_before_after"], [1, 1])
        self.assertEqual(attempt["object_marks"]["building_before"],
                         attempt["object_marks"]["building_after"])
        protocol = json.loads(
            (Path(__file__).parents[2] / "schema/live/retail-player-protocol-v2.json")
            .read_text()
        )
        self.assertIn("simple retail UI pick", protocol["actions"]["build"]["coordinates"])

    def test_build_public_gate_accepts_farm_and_rejects_locked_barracks(self):
        observation = json.loads(
            (Path(__file__).parents[2] / "schema/live/retail-player-observation-v2.json")
            .read_text()
        )
        farm = retailctl.static_build_legality(417, observation)
        self.assertTrue(farm["accepted"])
        self.assertEqual(farm["base_cost_i32"], [0, 40, 0, 0, 0, 0])
        barracks = retailctl.static_build_legality(427, observation)
        self.assertFalse(barracks["accepted"])
        self.assertIn(572, barracks["prerequisites"])

    def test_build_site_query_uses_observed_worker_and_retail_simple_pick(self):
        observation = json.loads(
            (Path(__file__).parents[2] / "schema/live/retail-player-observation-v2.json")
            .read_text()
        )
        event = {"phase": "observed", "paused": 1, "validation_result": 1,
                 "placement_x": 1536, "placement_y": 32544, "placement_tested": 7}
        with mock.patch.object(retailctl, "exact_validation", return_value=event) as query:
            result = retailctl.find_build_site("unused", observation, 3, 417, 8)
        query.assert_called_once_with(
            "unused", ["find-build", "0", "1488", "32544", "8", "417", "3"]
        )
        self.assertEqual(result["site"], {"x": 1536, "y": 32544,
                                          "x2": -1, "y2": -1})
        self.assertEqual(result["tested"], 7)

    def test_arena_marshal_adapter_preserves_source_order_and_rl_heads(self):
        observation = json.loads(
            (Path(__file__).parents[2] / "schema/live/retail-player-observation-v2.json")
            .read_text()
        )
        calls = []
        def accept(root, owner, producer, type_index):
            calls.append((owner, producer, type_index))
            return {"validation_result": 1}
        plan = retailctl.arena_marshal_extracted_plan(observation, "unused", accept)
        self.assertEqual(plan["policy"], "Arena Marshal faithful-supported-subsequence")
        self.assertEqual(plan["command_order"],
                         ["sense", "economy", "scout", "military", "army_control", "employ"])
        # City State is already queued. Marshal::next_tech chooses it and queue_at
        # suppresses it without falling through; the supported later economy command is Citizen.
        self.assertEqual(plan["selected_action"]["type_index"], 50)
        self.assertEqual(plan["selected_don_env_heads"], [23, 0, 0, 0, 50, 0, 0, 0, 0, 1])
        self.assertEqual(calls, [(0, 2000, 50)])
        placement = next(t for t in plan["trace"] if t["stage"] == "economy.placement")
        self.assertEqual(placement["result"], "unsupported")
        self.assertIn("no Farm or other Build command", placement["reason"])

    def test_arena_marshal_adapter_does_not_skip_a_rejected_first_tech(self):
        observation = json.loads(
            (Path(__file__).parents[2] / "schema/live/retail-player-observation-v2.json")
            .read_text()
        )
        observation = copy.deepcopy(observation)
        observation["queued_types"] = [q for q in observation["queued_types"]
                                       if q["type_index"] != 565]
        library = next(o for o in observation["objects"] if o["object_id"] == 2005)
        library["production_queue"]["logical_length"] = 0
        library["production_queue"]["items"] = []
        calls = []
        def reject_city_state(root, owner, producer, type_index):
            calls.append(type_index)
            return {"validation_result": 0 if type_index == 565 else 1}
        plan = retailctl.arena_marshal_extracted_plan(observation, "unused", reject_city_state)
        self.assertEqual(calls, [565, 50])
        self.assertEqual(plan["selected_action"]["type_index"], 50)
        self.assertNotIn(572, calls)

    def test_live_arena_marshal_run_applies_exactly_one_bounded_action(self):
        live = Path(__file__).parents[2] / "schema/live"
        dry = json.loads((live / "retail-arena-marshal-dry-run-v1.json").read_text())
        self.assertEqual(dry["mode"], "dry-run")
        self.assertIsNone(dry["proof"])
        run = json.loads((live / "retail-arena-marshal-run-v1.json").read_text())
        self.assertEqual(run["mode"], "apply")
        self.assertEqual(run["plan"]["selected_don_env_heads"],
                         [23, 0, 0, 0, 50, 0, 0, 0, 0, 1])
        self.assertEqual(run["proof"]["frame_boundary"], {"before": 357, "after": 357})
        self.assertEqual(run["proof"]["pause_before_after"], [1, 1])
        proof = json.loads((live / "retail-arena-marshal-action-proof-v1.json").read_text())
        command = bytes.fromhex(proof["retail_command_hex"])
        self.assertEqual(command[-9], 0x18)
        for side, expected in (("before", 1), ("after", 2)):
            aggregate = next(q["count"] for q in proof[side]["queued_types"]
                             if q["type_index"] == 50)
            city = next(o for o in proof[side]["objects"] if o["object_id"] == 2000)
            self.assertEqual(aggregate, expected)
            self.assertEqual(city["production_queue"]["logical_length"], expected)

    def test_arena_marshal_protocol_refuses_unsupported_build_substitution(self):
        protocol = json.loads(
            (Path(__file__).parents[2] / "schema/live/retail-arena-marshal-protocol-v1.json")
            .read_text()
        )
        self.assertEqual(protocol["adapter"], "faithful supported subsequence")
        self.assertEqual(protocol["mappings"]["Build"]["packed_opcode"], "0x19")
        self.assertIn("must not substitute", protocol["mappings"]["Build"]["adapter_limit"])
        self.assertIn("at most one", protocol["selection"])

    def test_live_build_at_proof_uses_retail_oracle_and_materializes_own_farm(self):
        live = Path(__file__).parents[2] / "schema/live"
        placement = json.loads((live / "retail-build-placement-proof-v1.json").read_text())
        self.assertEqual(placement["mode"], "validation-only")
        self.assertTrue(placement["query"]["accepted"])
        self.assertEqual(placement["query"]["site"],
                         {"x": 2832, "y": 32448, "x2": -1, "y2": -1})
        proof = json.loads((live / "retail-economy-build-proof-v1.json").read_text())
        self.assertEqual(proof["pause_before_after"], [1, 1])
        self.assertEqual(proof["frame_boundary"], {"before": 357, "after": 387})
        self.assertEqual(bytes.fromhex(proof["retail_command_hex"])[5], 0x19)
        before_ids = {(obj["object_id"], obj["id"]["uid"]) for obj in proof["before"]["objects"]}
        new_farms = [obj for obj in proof["after"]["objects"]
                     if obj["category"] == "build" and obj["type_index"] == 417 and
                     (obj["object_id"], obj["id"]["uid"]) not in before_ids]
        self.assertEqual([(obj["object_id"], obj["id"]["uid"]) for obj in new_farms],
                         [(2007, 16)])

    def test_v3_gather_state_uses_complete_cities_and_signed_retail_capacity(self):
        observation = json.loads(
            (Path(__file__).parents[2] / "schema/live/retail-player-observation-v3-post-camp.json")
            .read_text()
        )
        state = retailctl.marshal_gather_state(observation)
        self.assertEqual(observation["protocol"], "don.retail-player.v3")
        self.assertEqual(state["complete_city_count"], 1)
        self.assertEqual(state["useful_slots"][:2], [9, 9])
        self.assertEqual(state["seats"][:2], [4, 8])
        self.assertEqual((state["food_gap"], state["wood_gap"]), (5, 1))
        self.assertTrue(all("pointer" not in obj for obj in observation["objects"]))

    def test_live_marshal_camp_plan_preserves_capacity_first_policy(self):
        live = Path(__file__).parents[2] / "schema/live"
        dry = json.loads((live / "retail-arena-marshal-camp-dry-run-v1.json").read_text())
        action = dry["plan"]["selected_action"]
        self.assertEqual(action["type_index"], 418)
        self.assertEqual((action["x1"], action["y1"]), (4800, 32064))
        self.assertEqual(action["placement_evidence"]["capacity"], 4)
        self.assertEqual(action["placement_evidence"]["ring"], 12)
        self.assertEqual(dry["plan"]["selected_don_env_heads"],
                         [24, 25, 167, 0, 418, 0, 0, 0, 0, 0])
        placement = next(t for t in dry["plan"]["trace"]
                         if t["stage"] == "economy.placement")
        self.assertEqual([w["type_index"] for w in placement["wants"]], [418, 414, 417])
        self.assertEqual(placement["attempts"][0]["result"], "emit")

    def test_live_marshal_camp_materialization_and_pending_worker_target_are_proven(self):
        live = Path(__file__).parents[2] / "schema/live"
        proof = json.loads(
            (live / "retail-arena-marshal-camp-action-proof-v1.json").read_text()
        )
        self.assertEqual(proof["frame_boundary"], {"before": 687, "after": 717})
        self.assertEqual(proof["pause_before_after"], [1, 1])
        self.assertEqual(bytes.fromhex(proof["retail_command_hex"])[5], 0x19)
        before_ids = {(obj["object_id"], obj["id"]["uid"])
                      for obj in proof["before"]["objects"]}
        new_camps = [obj for obj in proof["after"]["objects"]
                     if obj["category"] == "build" and obj["type_index"] == 418 and
                     (obj["object_id"], obj["id"]["uid"]) not in before_ids]
        self.assertEqual([(obj["object_id"], obj["id"]["uid"],
                           obj["gathering"]["capacity"]) for obj in new_camps],
                         [(2008, 19, 4)])
        worker = next(obj for obj in proof["after"]["objects"] if obj["object_id"] == 3)
        self.assertEqual(worker["order"]["kind"], "MoveOrder")
        self.assertEqual(worker["order"]["queued_build_target"],
                         {"object_id": 2008, "uid": 19})

    def test_supervised_marshal_loop_applies_only_proven_verbs_and_records_noops(self):
        template = json.loads(
            (Path(__file__).parents[2] / "schema/live/retail-player-observation-v3-post-camp.json")
            .read_text()
        )
        template["protocol"] = "don.retail-player.v4"
        template["schema"] = "don.retail-player-observation.v4"
        template["visible_enemies"] = []
        observations = []
        for frame in [717, 747, 747, 777, 777, 807]:
            obs = copy.deepcopy(template)
            obs["frame"] = frame
            observations.append(obs)
        queue = {"verb": "queue", "owner": 0, "producer_id": 2000,
                 "type_index": 50, "count": 1}
        move = {"verb": "move", "owner": 0, "object_ids": [0]}
        plans = [{"selected_action": queue}, {"selected_action": None},
                 {"selected_action": move}]
        proof = {
            "schema": "don.retail-economy-action-proof.v1",
            "retail_validation": {"validation_result": 1},
            "retail_command_hex": "18",
            "frame_boundary": {"before": 717, "after": 717},
            "pause_before_after": [1, 1],
            "before": observations[0], "after": observations[0],
            "bounded_settlement": [],
        }
        advances = [
            {"requested": 30, "frame_before": 717, "frame_after": 747, "pause_after": 1},
            {"requested": 30, "frame_before": 747, "frame_after": 777, "pause_after": 1},
            {"requested": 30, "frame_before": 777, "frame_after": 807, "pause_after": 1},
        ]
        with tempfile.TemporaryDirectory() as td, \
                mock.patch.object(retailctl, "player_observation",
                                  side_effect=observations), \
                mock.patch.object(retailctl, "arena_marshal_extracted_plan",
                                  side_effect=plans) as planner, \
                mock.patch.object(retailctl, "prove_supported_action",
                                  return_value=proof) as apply_action, \
                mock.patch.object(retailctl, "advance_frames", side_effect=advances), \
                mock.patch.object(retailctl, "send", return_value=[]), \
                mock.patch.object(retailctl, "stop"), \
                mock.patch.object(retailctl, "guest_cmd", return_value="state=parked"):
            path = Path(td) / "loop.json"
            retailctl.arena_marshal_supervised_loop(
                "unused", "test-generation", path, 3, 30, True
            )
            run = json.loads(path.read_text())
        self.assertEqual(run["status"], "complete")
        self.assertEqual([step["action_mode"] for step in run["decisions"]],
                         ["apply", "no-op-no-supported-action", "apply"])
        self.assertEqual([step["invariants"]["actions_applied"]
                          for step in run["decisions"]], [1, 0, 1])
        self.assertEqual([step["invariants"]["frame_delta"]
                          for step in run["decisions"]], [30, 30, 30])
        self.assertEqual(apply_action.call_count, 2)
        self.assertTrue(all(call.kwargs["settlement_limit_frames"] == 30
                            for call in apply_action.call_args_list))
        tactical_states = [call.kwargs["tactical_state"] for call in planner.call_args_list]
        self.assertTrue(all(state is tactical_states[0] for state in tactical_states))

    def test_same_frame_apply_token_covers_complete_public_own_object_state(self):
        observation = json.loads(
            (Path(__file__).parents[2] / "schema/live/retail-player-observation-v3-post-camp.json")
            .read_text()
        )
        changed = copy.deepcopy(observation)
        next(obj for obj in changed["objects"] if obj["category"] == "unit")["order"][
            "length"
        ] += 1
        self.assertNotEqual(retailctl.paused_observation_token(observation),
                            retailctl.paused_observation_token(changed))

    def test_supervised_marshal_loop_fails_closed_on_player_identity_change(self):
        template = json.loads(
            (Path(__file__).parents[2] / "schema/live/retail-player-observation-v3-post-camp.json")
            .read_text()
        )
        template["protocol"] = "don.retail-player.v4"
        template["schema"] = "don.retail-player-observation.v4"
        template["visible_enemies"] = []
        changed = copy.deepcopy(template)
        changed["frame"] += 30
        changed["player"]["tribe"] += 1
        with tempfile.TemporaryDirectory() as td, \
                mock.patch.object(retailctl, "player_observation",
                                  side_effect=[template, changed]), \
                mock.patch.object(retailctl, "arena_marshal_extracted_plan",
                                  return_value={"selected_action": None}), \
                mock.patch.object(retailctl, "advance_frames",
                                  return_value={"requested": 30}), \
                mock.patch.object(retailctl, "send", return_value=[]), \
                mock.patch.object(retailctl, "stop"), \
                mock.patch.object(retailctl, "guest_cmd", return_value="state=parked"):
            path = Path(td) / "failed.json"
            with self.assertRaisesRegex(RuntimeError, "identity changed"):
                retailctl.arena_marshal_supervised_loop(
                    "unused", "test-generation", path, 1, 30, True
                )
            run = json.loads(path.read_text())
        self.assertEqual(run["status"], "failed")
        self.assertIn("identity changed", run["status_detail"])

    def test_live_supervised_marshal_loop_has_exact_actions_noops_and_parked_boundary(self):
        live = Path(__file__).parents[2] / "schema/live"
        run = json.loads(
            (live / "retail-arena-marshal-supervised-loop-v1.json").read_text()
        )
        self.assertEqual(run["schema"], "don.retail-arena-marshal-supervised-loop.v1")
        self.assertEqual(run["status"], "complete")
        self.assertEqual(run["controller_generation"], "marshal-loop-v16")
        self.assertEqual(len(run["decisions"]), 8)
        self.assertEqual(
            [(step["before"]["frame"], step["after"]["frame"])
             for step in run["decisions"]],
            [(807, 837), (837, 867), (867, 897), (897, 927),
             (927, 957), (957, 987), (987, 1017), (1017, 1047)],
        )
        self.assertEqual(
            [step["action_mode"] for step in run["decisions"]],
            ["apply", "apply"] + ["no-op-no-supported-action"] * 6,
        )
        self.assertEqual(
            [step["invariants"]["actions_applied"] for step in run["decisions"]],
            [1, 1, 0, 0, 0, 0, 0, 0],
        )
        self.assertTrue(all(
            step["invariants"]["identity_stable"] and
            step["invariants"]["pause_before_after"] == [1, 1] and
            step["invariants"]["frame_delta"] == 30 and
            not step["invariants"]["unsupported_substitution"]
            for step in run["decisions"]
        ))

        queue = json.loads(
            (live / "retail-arena-marshal-supervised-loop-v1-step-00-action-proof.json")
            .read_text()
        )
        self.assertEqual(queue["action"]["type_name"], "Citizen")
        self.assertEqual(bytes.fromhex(queue["retail_command_hex"])[-9], 0x18)
        self.assertEqual(queue["frame_boundary"], {"before": 807, "after": 807})
        self.assertEqual(queue["pause_before_after"], [1, 1])

        build = json.loads(
            (live / "retail-arena-marshal-supervised-loop-v1-step-01-action-proof.json")
            .read_text()
        )
        self.assertEqual(build["action"]["type_name"], "Farm")
        self.assertEqual(build["action"]["worker_ids"], [9])
        self.assertEqual(bytes.fromhex(build["retail_command_hex"])[-25], 0x19)
        self.assertEqual(build["frame_boundary"], {"before": 837, "after": 867})
        before_ids = {(obj["object_id"], obj["id"]["uid"])
                      for obj in build["before"]["objects"]}
        new_farms = [obj for obj in build["after"]["objects"]
                     if obj["category"] == "build" and obj["type_index"] == 417 and
                     (obj["object_id"], obj["id"]["uid"]) not in before_ids]
        self.assertEqual(
            [(obj["object_id"], obj["id"]["uid"], obj["position"]["x"],
              obj["position"]["y"], obj["gathering"]["capacity"])
             for obj in new_farms],
            [(2009, 20, 3456, 29184, 1)],
        )
        worker = next(obj for obj in build["after"]["objects"]
                      if obj["object_id"] == 9)
        self.assertEqual(worker["order"]["queued_build_target"],
                         {"object_id": 2009, "uid": 20})
        self.assertIn("state=parked", run["parked_ready_record"])
        self.assertIn("pid=12324", run["parked_ready_record"])


if __name__ == "__main__":
    unittest.main()
