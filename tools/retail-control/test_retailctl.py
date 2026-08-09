import importlib.util
import io
import json
import copy
from pathlib import Path
import re
import subprocess
import tempfile
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location("retailctl", Path(__file__).with_name("retailctl.py"))
assert SPEC and SPEC.loader
retailctl = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(retailctl)


def netsys_manifest_fixture() -> dict:
    environment = retailctl.netsys_environment("load-only")
    return {
        "schema": retailctl.NETSYS_SCHEMA,
        "state": "installed",
        "created_unix_ms": 1,
        "credential_material": "none",
        "task_name": retailctl.NETSYS_TASK_NAME,
        "retail_executable": {
            "path": retailctl.RETAIL_EXE,
            "size": 9_925_120,
            "sha256": retailctl.EXPECTED_SHA256,
        },
        "original_dll": {
            "path": retailctl.RETAIL_NETSYS_DLL,
            "size": retailctl.EXPECTED_NETSYS_SIZE,
            "sha256": retailctl.EXPECTED_NETSYS_SHA256,
        },
        "backup_dll": {
            "path": retailctl.NETSYS_BACKUP,
            "size": retailctl.EXPECTED_NETSYS_SIZE,
            "sha256": retailctl.EXPECTED_NETSYS_SHA256,
        },
        "shim": {
            "path": retailctl.NETSYS_STAGED,
            "size": 198_656,
            "sha256": "a" * 64,
        },
        "mode": "load-only",
        "environment": environment,
        "launcher_sha256": "b" * 64,
        "generation": 1,
        "rollover": None,
    }


class RetailCtlTests(unittest.TestCase):
    def test_all_guest_transports_have_bounded_exact_timeout_results(self):
        timeout = subprocess.TimeoutExpired(["prlctl"], 45)
        expected = {
            "cmd": retailctl.guest_timeout_record("cmd"),
            "powershell": retailctl.guest_timeout_record("powershell"),
            "powershell-encoded": retailctl.guest_timeout_record("powershell-encoded"),
        }
        with mock.patch.object(retailctl, "run", side_effect=timeout) as command:
            with self.assertRaisesRegex(
                    retailctl.GuestCommandTimeout, re.escape(expected["cmd"])):
                retailctl.guest_cmd("ver")
            self.assertEqual(
                command.call_args.kwargs["timeout"],
                retailctl.GUEST_COMMAND_TIMEOUT_SECONDS,
            )
        with (
            mock.patch.object(retailctl, "run", side_effect=timeout),
            mock.patch("sys.stderr", new_callable=io.StringIO) as stderr,
        ):
            self.assertEqual(retailctl.guest_cmd("ver", check=False), expected["cmd"])
            self.assertEqual(stderr.getvalue().strip(), expected["cmd"])
        with mock.patch.object(retailctl, "run", side_effect=timeout):
            self.assertEqual(
                retailctl.guest_cmd_status("ver"),
                (retailctl.GUEST_TIMEOUT_RETURN_CODE, expected["cmd"]),
            )
            with self.assertRaisesRegex(
                    retailctl.GuestCommandTimeout, re.escape(expected["powershell"])):
                retailctl.guest_ps("Get-Date")
            with self.assertRaisesRegex(
                    retailctl.GuestCommandTimeout,
                    re.escape(expected["powershell-encoded"]),
            ):
                retailctl.guest_ps_encoded("Get-Date")
        source = Path(retailctl.__file__).read_text()
        self.assertEqual(source.count('["prlctl", "exec", VM,'), 4)
        for start in [
                "def guest_cmd(", "def guest_cmd_status(", "def guest_ps(",
                "def guest_ps_encoded("]:
            section = source[source.index(start):]
            section = section[:section.index("\ndef ", 1)]
            self.assertIn("timeout=GUEST_COMMAND_TIMEOUT_SECONDS", section)

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
            "# base=00D60000 addr=00EF1686 len=5 module=riseofnations.exe "
            "rva=191686 deref=0 nderef=0 off=0 root=00EF1686 "
            "pointer_addr=00EF1686 root_value=00000000 stable=-1\n"
            "00EF1686: E8 45 67 3C 00"
        )
        self.assertEqual(retailctl.parse_hook_peek_output(original)["status"], "original")
        address = 0x00EF1686
        target = 0x6A001000
        displacement = target - (address + 5)
        patched_bytes = b"\xe8" + displacement.to_bytes(4, "little", signed=True)
        patched = (
            original.splitlines()[0] + "\n"
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
        inconsistent = original.replace("pointer_addr=00EF1686", "pointer_addr=00EF1687")
        self.assertEqual(
            retailctl.parse_hook_peek_output(inconsistent)["status"], "unreadable"
        )

    def test_ready_record_is_bound_to_pid_root_and_exact_rebased_addresses(self):
        root = r"C:\Users\Public\don-retail-control-tactical-v21"
        raw = (
            f"state=parked\npid=7804\nroot={root}\nbase=0x00d60000\n"
            "turn_call_site=0x00ef1686\nturn_do_frame=0x012b7dd0\n"
        )
        record = retailctl.parse_ready_record(raw)
        self.assertEqual(retailctl.ready_identity_errors(record, 7804, root), [])
        canonical_record = retailctl.parse_ready_record(
            raw.replace(f"root={root}", rf"root=\\?\{root}")
        )
        self.assertEqual(
            retailctl.ready_identity_errors(canonical_record, 7804, root), []
        )
        self.assertIn(
            "ready pid does not match target",
            retailctl.ready_identity_errors(record, 12324, root),
        )
        torn = retailctl.parse_ready_record(raw + "pid=7804\n")
        self.assertTrue(torn["errors"])

    def test_donject_machine_parser_preserves_canonical_windows_paths(self):
        path = r"\\?\C:\Users\Public\a generation\controller.dll"
        fields = retailctl.parse_donject_fields(
            f'protocol=donject.v2 command=modules module_path="{path}" index=1'
        )
        self.assertIsNotNone(fields)
        self.assertEqual(fields["module_path"], path)
        self.assertIsNone(
            retailctl.parse_donject_fields(
                "protocol=donject.v2 protocol=duplicate command=modules"
            )
        )

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

    def test_netsys_launcher_profiles_are_process_local_and_load_only_first(self):
        load_environment = retailctl.netsys_environment("load-only")
        self.assertEqual(load_environment["DON_NET_NAME"], "Ai")
        self.assertEqual(load_environment["DON_NET_ID"], "1")
        self.assertEqual(load_environment["DON_NET_LOAD_ONLY"], "1")
        load_launcher = retailctl.netsys_launcher_text("load-only", load_environment)
        self.assertLess(
            load_launcher.index('set "DON_NET_LOAD_ONLY="'),
            load_launcher.index('set "DON_NET_LOAD_ONLY=1"'),
        )
        host_environment = retailctl.netsys_environment("host", "127.0.0.1:31337")
        self.assertNotIn("DON_NET_LOAD_ONLY", host_environment)
        host_launcher = retailctl.netsys_launcher_text("host", host_environment)
        self.assertIn('set "DON_NET_LOAD_ONLY="', host_launcher)
        self.assertNotIn('set "DON_NET_LOAD_ONLY=1"', host_launcher)
        self.assertIn(f'"{retailctl.RETAIL_EXE}"', host_launcher)
        bridge_environment = retailctl.netsys_environment(
            "host-bridge", "127.0.0.1:31337"
        )
        self.assertEqual(bridge_environment["DON_NET_SETUP_BRIDGE"], "1")
        bridge_launcher = retailctl.netsys_launcher_text(
            "host-bridge", bridge_environment
        )
        self.assertLess(
            bridge_launcher.index('set "DON_NET_SETUP_BRIDGE="'),
            bridge_launcher.index('set "DON_NET_SETUP_BRIDGE=1"'),
        )
        with self.assertRaisesRegex(ValueError, "loopback or 0.0.0.0"):
            retailctl.netsys_environment("host", "192.0.2.1:31337")

    def test_netsys_manifest_is_bound_to_shipped_backup_and_exact_environment(self):
        manifest = netsys_manifest_fixture()
        self.assertIs(retailctl.validate_netsys_manifest(manifest), manifest)
        wrong_backup = copy.deepcopy(manifest)
        wrong_backup["backup_dll"]["sha256"] = "c" * 64
        with self.assertRaisesRegex(ValueError, "shipped files"):
            retailctl.validate_netsys_manifest(wrong_backup)
        leaked_environment = copy.deepcopy(manifest)
        leaked_environment["environment"]["STEAM_TICKET"] = "forbidden"
        with self.assertRaisesRegex(ValueError, "exact supported profile"):
            retailctl.validate_netsys_manifest(leaked_environment)
        rollover = copy.deepcopy(manifest)
        rollover["state"] = "rollover-staged"
        rollover["rollover"] = {
            "from_generation": 1,
            "archive_root": retailctl.netsys_generation_root(1),
            "next_shim": {
                "path": retailctl.NETSYS_NEXT,
                "size": 197_632,
                "sha256": "c" * 64,
            },
        }
        self.assertIs(retailctl.validate_netsys_manifest(rollover), rollover)
        wrong_archive = copy.deepcopy(rollover)
        wrong_archive["rollover"]["archive_root"] = retailctl.netsys_generation_root(2)
        with self.assertRaisesRegex(ValueError, "generation/archive"):
            retailctl.validate_netsys_manifest(wrong_archive)

    def test_netsys_next_generation_archives_before_swap_and_never_replaces_backup(self):
        self.assertEqual(
            retailctl.netsys_generation_root(7),
            retailctl.NETSYS_ARCHIVE_ROOT + r"\generation-0007",
        )
        with self.assertRaisesRegex(ValueError, "between 1 and 9999"):
            retailctl.netsys_generation_root(0)
        source = Path(retailctl.__file__).read_text()
        rollover = source[source.index("def netsys_next_generation"):
                          source.index("def netsys_configure_host")]
        archive = rollover.index("archive_netsys_evidence(manifest)")
        target_swap = rollover.index("[IO.File]::Replace({ps_literal(target_temp)}")
        self.assertLess(archive, target_swap)
        self.assertNotIn("[IO.File]::Replace({ps_literal(NETSYS_BACKUP)}", rollover)
        self.assertIn("archive = read_netsys_archive(manifest)", rollover)
        self.assertIn('rollover_state == "pre-swap"', rollover)
        self.assertIn('rollover_state == "post-first-swap"', rollover)
        staged_swap = rollover.index(
            "[IO.File]::Replace({ps_literal(NETSYS_NEXT)}"
        )
        self.assertLess(target_swap, staged_swap)
        self.assertIn('"mode": "load-only"', rollover)
        self.assertIn('"rollover": None', rollover)

    def test_netsys_rollover_classifier_accepts_only_exact_monotonic_states(self):
        manifest = netsys_manifest_fixture()
        new = {
            "path": retailctl.NETSYS_NEXT,
            "size": 198_700,
            "sha256": "c" * 64,
        }
        manifest.update({
            "state": "rollover-staged",
            "rollover": {
                "from_generation": 1,
                "archive_root": retailctl.netsys_generation_root(1),
                "next_shim": new,
            },
        })
        retailctl.validate_netsys_manifest(manifest)

        def present(path, identity):
            return {"present": True, "path": path, **{
                key: identity[key] for key in ("size", "sha256")
            }}

        old_target = present(retailctl.RETAIL_NETSYS_DLL, manifest["shim"])
        old_staged = present(retailctl.NETSYS_STAGED, manifest["shim"])
        new_target = present(retailctl.RETAIL_NETSYS_DLL, new)
        new_staged = present(retailctl.NETSYS_STAGED, new)
        next_new = present(retailctl.NETSYS_NEXT, new)
        next_absent = {"present": False, "path": retailctl.NETSYS_NEXT}
        self.assertEqual(
            retailctl.classify_netsys_rollover_files(
                manifest, old_target, old_staged, next_new
            ),
            "pre-swap",
        )
        self.assertEqual(
            retailctl.classify_netsys_rollover_files(
                manifest, new_target, old_staged, next_new
            ),
            "post-first-swap",
        )
        self.assertEqual(
            retailctl.classify_netsys_rollover_files(
                manifest, new_target, new_staged, next_absent
            ),
            "post-both-swaps",
        )
        with self.assertRaisesRegex(SystemExit, "unknown or mixed"):
            retailctl.classify_netsys_rollover_files(
                manifest, old_target, new_staged, next_absent
            )

    def test_netsys_rollover_resumes_pre_swap_and_post_first_swap(self):
        for initial_state in ["pre-swap", "post-first-swap"]:
            with self.subTest(initial_state=initial_state):
                manifest = netsys_manifest_fixture()
                old = manifest["shim"]
                new = {
                    "path": retailctl.NETSYS_NEXT,
                    "size": 198_700,
                    "sha256": "c" * 64,
                }
                manifest.update({
                    "state": "rollover-staged",
                    "rollover": {
                        "from_generation": 1,
                        "archive_root": retailctl.netsys_generation_root(1),
                        "next_shim": new,
                    },
                })
                retailctl.validate_netsys_manifest(manifest)
                identities = {
                    retailctl.RETAIL_NETSYS_DLL: (
                        new if initial_state == "post-first-swap" else old
                    ),
                    retailctl.NETSYS_STAGED: old,
                    retailctl.NETSYS_NEXT: new,
                    retailctl.NETSYS_BACKUP: {
                        "size": retailctl.EXPECTED_NETSYS_SIZE,
                        "sha256": retailctl.EXPECTED_NETSYS_SHA256,
                    },
                }
                scripts = []

                def file_record(path):
                    identity = identities.get(path)
                    if identity is None:
                        return {"present": False, "path": path}
                    return {
                        "present": True,
                        "path": path,
                        "size": identity["size"],
                        "sha256": identity["sha256"],
                    }

                def encoded(script, *, check=True):
                    self.assertTrue(check)
                    scripts.append(script)
                    if ".don-next-generation" in script and "[IO.File]::Replace" in script:
                        identities[retailctl.RETAIL_NETSYS_DLL] = new
                    if (retailctl.NETSYS_NEXT in script and
                            retailctl.NETSYS_STAGED in script and
                            ".don-next-generation" not in script):
                        identities[retailctl.NETSYS_STAGED] = new
                        identities.pop(retailctl.NETSYS_NEXT, None)
                    return ""

                written = []
                archive = {"exact_archive": initial_state}
                with (
                    mock.patch.object(retailctl, "require_retail_absent"),
                    mock.patch.object(retailctl, "read_netsys_manifest",
                                      return_value=manifest),
                    mock.patch.object(retailctl, "host_netsys_identity",
                                      return_value={"path": "host", **new}),
                    mock.patch.object(retailctl, "read_netsys_archive",
                                      return_value=archive),
                    mock.patch.object(retailctl, "guest_file_record",
                                      side_effect=file_record),
                    mock.patch.object(retailctl, "guest_ps_encoded",
                                      side_effect=encoded),
                    mock.patch.object(retailctl, "guest_write_bytes"),
                    mock.patch.object(retailctl, "write_netsys_manifest",
                                      side_effect=lambda value: written.append(copy.deepcopy(value))),
                    mock.patch.object(retailctl, "guest_cmd", return_value=""),
                    mock.patch("builtins.print"),
                ):
                    result = retailctl.netsys_next_generation(
                        retailctl.DEFAULT_NETSYS_SHIM, 8765
                    )
                first_swaps = [script for script in scripts
                               if ".don-next-generation" in script]
                second_swaps = [script for script in scripts
                                if retailctl.NETSYS_NEXT in script and
                                retailctl.NETSYS_STAGED in script and
                                ".don-next-generation" not in script]
                self.assertEqual(len(first_swaps), initial_state == "pre-swap")
                self.assertEqual(len(second_swaps), 1)
                self.assertEqual(result["archived_generation"], archive)
                self.assertEqual(result["current"]["state"], "installed")
                self.assertEqual(result["current"]["generation"], 2)
                self.assertIsNone(result["current"]["rollover"])
                self.assertEqual(written[-1], result["current"])

    def test_netsys_archive_manifest_is_exact_for_resume(self):
        manifest = netsys_manifest_fixture()
        archive = {
            "schema": "don.retail-netsys-evidence-generation.v1",
            "generation": 1,
            "credential_material": "none",
            "shim": manifest["shim"],
            "mode": manifest["mode"],
            "environment": manifest["environment"],
            "evidence": [{
                "label": "exit-load-only",
                "source_path": retailctl.NETSYS_LOAD_EXIT,
                "archive_path": (
                    retailctl.netsys_generation_root(1) + r"\exit-load-only.txt"
                ),
                "size": 12,
                "sha256": "d" * 64,
                "summary": {"kind": "exit", "exit_code": 0},
            }],
        }
        self.assertIs(
            retailctl.validate_netsys_archive_manifest(archive, manifest), archive
        )
        changed = copy.deepcopy(archive)
        changed["evidence"][0]["summary"]["exit_code"] = "0"
        with self.assertRaisesRegex(ValueError, "exit summary"):
            retailctl.validate_netsys_archive_manifest(changed, manifest)

    def test_netsys_next_generation_migrates_exact_committed_v1_installed_manifest(self):
        current = netsys_manifest_fixture()
        legacy = copy.deepcopy(current)
        legacy.pop("generation")
        legacy.pop("rollover")
        migrated = retailctl.migrate_netsys_manifest(legacy)
        self.assertEqual(migrated["generation"], 1)
        self.assertIsNone(migrated["rollover"])
        self.assertEqual(migrated["shim"], legacy["shim"])
        self.assertNotIn("generation", legacy)
        with mock.patch.object(
            retailctl,
            "guest_read_bytes",
            return_value=(json.dumps(legacy) + "\n").encode("utf-8"),
        ):
            self.assertEqual(retailctl.read_netsys_manifest(), migrated)
        wrong_state = copy.deepcopy(legacy)
        wrong_state["state"] = "restored"
        with self.assertRaisesRegex(ValueError, "exact installed v1"):
            retailctl.migrate_netsys_manifest(wrong_state)
        extra = copy.deepcopy(legacy)
        extra["unrecognized"] = True
        with self.assertRaisesRegex(ValueError, "incomplete or unexpected"):
            retailctl.migrate_netsys_manifest(extra)

    def test_netsys_directory_probe_ignores_clixml_before_bounded_json(self):
        output = (
            "#< CLIXML\n<Objs Version=\"1.1.0.1\"></Objs>\n"
            f"{retailctl.NETSYS_JSON_BEGIN}\n"
            '{"present":true}\n'
            f"{retailctl.NETSYS_JSON_END}\n"
        )
        with mock.patch.object(retailctl, "guest_ps_encoded", return_value=output):
            self.assertTrue(retailctl.guest_directory_present(retailctl.NETSYS_ARCHIVE_ROOT))
        ambiguous = output + output
        with mock.patch.object(retailctl, "guest_ps_encoded", return_value=ambiguous):
            with self.assertRaisesRegex(ValueError, "missing or ambiguous"):
                retailctl.guest_directory_present(retailctl.NETSYS_ARCHIVE_ROOT)

    def test_live_guest_file_evidence_hashes_an_immutable_copy(self):
        data = b"seq=1 pid=77 call=factory.get_netsys_object_ptr\n"
        digest = __import__("hashlib").sha256(data).hexdigest()
        commands = []
        with (
            mock.patch.object(retailctl, "guest_cmd",
                              side_effect=lambda command, **_kwargs: commands.append(command)),
            mock.patch.object(retailctl, "guest_file_record", return_value={
                "present": True, "path": "snapshot", "size": len(data),
                "sha256": digest,
            }),
            mock.patch.object(retailctl, "guest_read_bytes", return_value=data),
        ):
            record, copied = retailctl.guest_live_file_evidence(
                retailctl.NETSYS_LOAD_TRACE, 1024
            )
        self.assertEqual(copied, data)
        self.assertEqual(record, {
            "path": retailctl.NETSYS_LOAD_TRACE,
            "size": len(data),
            "sha256": digest,
        })
        self.assertIn("copy /b /y", commands[1])
        self.assertTrue(commands[-1].startswith("del /q"))

    def test_netsys_preintent_retry_reuses_only_exact_current_parity_next(self):
        manifest = netsys_manifest_fixture()
        host = {"size": 198_656, "sha256": "c" * 64}
        next_record = {
            "present": True,
            "path": retailctl.NETSYS_NEXT,
            "size": host["size"],
            "sha256": host["sha256"],
        }
        self.assertTrue(
            retailctl.reuse_preintent_netsys_next(next_record, host, manifest)
        )
        self.assertFalse(
            retailctl.reuse_preintent_netsys_next(
                {"present": False, "path": retailctl.NETSYS_NEXT}, host, manifest
            )
        )
        unknown = {**next_record, "sha256": "d" * 64}
        with self.assertRaisesRegex(SystemExit, "unknown or different orphaned"):
            retailctl.reuse_preintent_netsys_next(unknown, host, manifest)
        in_flight = copy.deepcopy(manifest)
        in_flight["state"] = "rollover-staged"
        in_flight["rollover"] = {"unexpected": True}
        with self.assertRaisesRegex(SystemExit, "unknown or different orphaned"):
            retailctl.reuse_preintent_netsys_next(next_record, host, in_flight)

    def test_netsys_rollover_never_passes_a_null_replace_backup(self):
        source = Path(retailctl.__file__).read_text()
        rollover = source[source.index("def netsys_next_generation"):
                          source.index("def netsys_configure_host")]
        self.assertNotIn(", $null)", rollover)
        self.assertIn("target_previous", rollover)
        self.assertIn("staged_previous", rollover)

    def test_netsys_trace_is_contiguous_pid_bound_and_credential_free(self):
        trace = (
            "seq=1 pid=77 call=factory.get_netsys_object_ptr\n"
            "seq=2 pid=77 factory=ready abi=netsys-v65 role=Host "
            "load_only=true local_addr=127.0.0.1:49152\n"
            "seq=3 pid=77 call=vtable.ns_error_set_callback\n"
            "seq=4 pid=77 call=vtable.ns_set_profiler\n"
            "seq=5 pid=77 call=vtable.ns_init\n"
            "seq=6 pid=77 init=stored messenger=true crossplay_service=true "
            "object_size=0x3d4\n"
            "seq=7 pid=77 call=vtable.ns_close\n"
            "seq=8 pid=77 call=vtable.ns_cleanup_system\n"
        )
        parsed = retailctl.parse_netsys_trace(trace, 77)
        self.assertTrue(parsed["load_only"])
        self.assertEqual(len(parsed["records"]), 8)
        retailctl.validate_netsys_load_only_initialized_frontier(parsed)
        retailctl.validate_netsys_load_only_frontier(parsed)
        initialized_only = "\n".join(trace.splitlines()[:6]) + "\n"
        retailctl.validate_netsys_load_only_initialized_frontier(
            retailctl.parse_netsys_trace(initialized_only, 77)
        )
        with self.assertRaisesRegex(ValueError, "cleanup_system"):
            retailctl.validate_netsys_load_only_frontier(
                retailctl.parse_netsys_trace(initialized_only, 77)
            )
        cleanup_only = trace.replace(
            "seq=7 pid=77 call=vtable.ns_close\n"
            "seq=8 pid=77 call=vtable.ns_cleanup_system\n",
            "seq=7 pid=77 call=vtable.ns_cleanup_system\n",
        )
        retailctl.validate_netsys_load_only_frontier(
            retailctl.parse_netsys_trace(cleanup_only, 77)
        )
        duplicate_close = trace.replace(
            "seq=8 pid=77 call=vtable.ns_cleanup_system\n",
            "seq=8 pid=77 call=vtable.ns_close\n"
            "seq=9 pid=77 call=vtable.ns_cleanup_system\n",
        )
        with self.assertRaisesRegex(ValueError, "multiple ns_close"):
            retailctl.validate_netsys_load_only_frontier(
                retailctl.parse_netsys_trace(duplicate_close, 77)
            )
        self.assertEqual(retailctl.parse_netsys_exit(b"exit_code=0\r\n"), {"exit_code": 0})
        self.assertIn(8008, retailctl.NETSYS_NORMAL_EXIT_CODES)
        with self.assertRaisesRegex(ValueError, "malformed"):
            retailctl.parse_netsys_exit(b"result=success\n")
        with self.assertRaisesRegex(ValueError, "sequence"):
            retailctl.parse_netsys_trace(trace.replace("seq=2", "seq=3"), 77)
        with self.assertRaisesRegex(ValueError, "multiple process|does not match"):
            retailctl.parse_netsys_trace(trace.replace("pid=77 factory", "pid=78 factory"))
        with self.assertRaisesRegex(ValueError, "credential"):
            retailctl.parse_netsys_trace(trace + "seq=9 pid=77 token=abc\n", 77)
        bridge_off = retailctl.parse_netsys_trace(
            "seq=1 pid=81 call=factory.get_netsys_object_ptr\n"
            "seq=2 pid=81 factory=ready abi=netsys-v65 role=Host "
            "load_only=false local_addr=127.0.0.1:31337\n"
            "seq=3 pid=81 call=vtable.ns_host\n"
            "seq=4 pid=81 callback=NetMessenger.on_player_added player_non_null=true\n"
            "seq=5 pid=81 callback=NetMessenger.on_player_added player_non_null=true\n",
            81,
        )
        retailctl.validate_netsys_bridge_off_frontier(bridge_off)

    def test_netsys_live_run_resumes_at_the_only_human_ui_pause_and_cleans_up(self):
        source = netsys_manifest_fixture()
        source["generation"] = 2
        current = {
            "path": "host-current",
            "size": 200_704,
            "sha256": "c" * 64,
        }
        peer = {"path": "host-peer", "size": 892_704, "sha256": "d" * 64}
        manifest = copy.deepcopy(source)
        calls = []

        def rollover(*_args):
            calls.append("rollover")
            manifest.update({
                "generation": 3,
                "shim": {
                    "path": retailctl.NETSYS_STAGED,
                    "size": current["size"],
                    "sha256": current["sha256"],
                },
            })
            return {"operation": "next-generation", "current": copy.deepcopy(manifest)}

        def configure(bind, _termination=None):
            calls.append(f"configure:{bind}")
            manifest.update({
                "mode": "host-bridge",
                "environment": retailctl.netsys_environment("host-bridge", bind),
                "launcher_sha256": "e" * 64,
            })
            return {"operation": "configure", "current": copy.deepcopy(manifest)}

        def load_proof(path, _timeout):
            calls.append(f"proof:{Path(path).name}")
            return {"generation": manifest["generation"], "artifact": str(path)}

        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            state_path = root / "state.json"
            generation2 = root / "generation2.json"
            latest = root / "latest.json"
            host_capture = root / "host.json"
            evidence = root / "run.donlstp"
            arguments = (
                state_path, Path("shim"), Path("peer"), "0.0.0.0:31337",
                "10.211.55.6:31337", 4, 300, 3, generation2, latest,
                host_capture, evidence,
            )
            with (
                mock.patch.object(retailctl, "host_netsys_identity", return_value=current),
                mock.patch.object(retailctl, "host_owned_peer_identity", return_value=peer),
                mock.patch.object(retailctl, "read_netsys_manifest",
                                  side_effect=lambda: copy.deepcopy(manifest)),
                mock.patch.object(retailctl, "guest_file_record", return_value={
                    "present": True, "path": retailctl.RETAIL_NETSYS_DLL,
                    "size": source["shim"]["size"],
                    "sha256": source["shim"]["sha256"],
                }),
                mock.patch.object(retailctl, "netsys_live_load_proof",
                                  side_effect=load_proof),
                mock.patch.object(retailctl, "netsys_next_generation",
                                  side_effect=rollover),
                mock.patch.object(retailctl, "netsys_configure_bridge_from_load_only",
                                  side_effect=configure),
                mock.patch.object(retailctl, "process_pids", side_effect=[([], ""), ([77], "")]),
                mock.patch.object(retailctl, "netsys_launch", return_value={
                    "mode": "host-bridge", "process": {"pid": 77},
                }),
                mock.patch.object(retailctl, "netsys_friend_game_gate",
                                  return_value={"ns_host_observed": True}),
                mock.patch.object(retailctl, "run_owned_peer_live",
                                  return_value={"evidence_identity": {"sha256": "f" * 64}}),
                mock.patch.object(retailctl, "netsys_capture", return_value={
                    "mode": "host-bridge",
                }),
                mock.patch.object(retailctl, "validated_local_netsys_capture",
                                  return_value={"artifact": {"sha256": "1" * 64}}),
                mock.patch.object(retailctl, "netsys_stop_retail",
                                  return_value={"forced": False}),
                mock.patch.object(retailctl, "netsys_restore",
                                  return_value={"state": "restored"}),
                mock.patch("builtins.print"),
            ):
                paused = retailctl.netsys_live_run(
                    *arguments, confirm_friend_game_ready=False, cleanup=False
                )
                self.assertEqual(paused["status"], "paused-for-user")
                state = retailctl.read_netsys_live_state(state_path)
                self.assertEqual(state["phase"], "awaiting-friend-game-ui")
                self.assertEqual(state["active_pid"], 77)
                complete = retailctl.netsys_live_run(
                    *arguments, confirm_friend_game_ready=True, cleanup=False
                )
            self.assertEqual(complete["status"], "complete")
            state = retailctl.read_netsys_live_state(state_path)
            self.assertEqual(state["phase"], "complete")
            self.assertIsNone(state["active_pid"])
            self.assertEqual(calls, [
                "proof:generation2.json", "rollover", "proof:latest.json",
                "configure:0.0.0.0:31337",
            ])

    def test_netsys_live_resume_refuses_host_artifact_identity_drift(self):
        state = {
            "shim": {"path": "shim", "size": 100, "sha256": "a" * 64},
            "owned_peer": {"path": "peer", "size": 200, "sha256": "b" * 64},
        }
        with (
            mock.patch.object(retailctl, "host_netsys_identity", return_value={
                "path": "shim", "size": 101, "sha256": "c" * 64,
            }),
            mock.patch.object(retailctl, "host_owned_peer_identity",
                              return_value=state["owned_peer"]),
        ):
            with self.assertRaisesRegex(SystemExit, "identity drift"):
                retailctl.verify_netsys_live_host_identities(
                    state, Path("shim"), Path("peer")
                )

    def test_direct_live_bridge_configuration_requires_current_load_proof_only(self):
        manifest = netsys_manifest_fixture()
        manifest["generation"] = 3
        proof = {"generation": 3, "factory_ready": "exact"}
        written = []
        launchers = []
        with (
            mock.patch.object(retailctl, "require_retail_absent"),
            mock.patch.object(retailctl, "read_netsys_manifest", return_value=manifest),
            mock.patch.object(retailctl, "validated_netsys_load_only_proof",
                              return_value=proof),
            mock.patch.object(retailctl, "guest_file_record", return_value={
                "present": False, "path": retailctl.NETSYS_BRIDGE_TRACE,
            }),
            mock.patch.object(retailctl, "guest_write_bytes",
                              side_effect=lambda path, data: launchers.append((path, data))),
            mock.patch.object(retailctl, "write_netsys_manifest",
                              side_effect=lambda value: written.append(copy.deepcopy(value))),
            mock.patch("builtins.print"),
        ):
            result = retailctl.netsys_configure_bridge_from_load_only(
                "0.0.0.0:31337"
            )
        self.assertEqual(result["load_only_proof"], proof)
        self.assertEqual(result["current"]["mode"], "host-bridge")
        self.assertEqual(
            result["current"]["environment"],
            retailctl.netsys_environment("host-bridge", "0.0.0.0:31337"),
        )
        self.assertEqual(written[-1], result["current"])
        self.assertEqual(launchers[0][0], retailctl.NETSYS_LAUNCHER)
        self.assertIn(b'DON_NET_SETUP_BRIDGE=1', launchers[0][1])

    def test_forced_load_proof_requires_exact_pid_and_exit(self):
        manifest = netsys_manifest_fixture()
        trace = (
            "seq=1 pid=77 call=factory.get_netsys_object_ptr\n"
            "seq=2 pid=77 factory=ready abi=netsys-v65 role=Host "
            "load_only=true local_addr=127.0.0.1:49152\n"
            "seq=3 pid=77 call=vtable.ns_error_set_callback\n"
            "seq=4 pid=77 call=vtable.ns_set_profiler\n"
            "seq=5 pid=77 call=vtable.ns_init\n"
            "seq=6 pid=77 init=stored messenger=true crossplay_service=true "
            "object_size=0x3d4\n"
        ).encode()
        termination = {
            "process": {
                "pid": 77, "path": retailctl.RETAIL_EXE,
                "session_id": 1, "start_utc": "2026-08-09T00:00:00Z",
            },
            "close_requested": False,
            "forced": True,
        }
        records = {
            retailctl.RETAIL_NETSYS_DLL: {
                "present": True, "path": retailctl.RETAIL_NETSYS_DLL,
                "size": manifest["shim"]["size"],
                "sha256": manifest["shim"]["sha256"],
            },
            retailctl.NETSYS_LOAD_TRACE: {
                "present": True, "path": retailctl.NETSYS_LOAD_TRACE,
                "size": len(trace), "sha256": "c" * 64,
            },
            retailctl.NETSYS_LOAD_EXIT: {
                "present": True, "path": retailctl.NETSYS_LOAD_EXIT,
                "size": 13, "sha256": "d" * 64,
            },
        }
        with (
            mock.patch.object(retailctl, "guest_file_record",
                              side_effect=lambda path: records[path]),
            mock.patch.object(retailctl, "guest_read_bytes",
                              side_effect=lambda path, _bound:
                              trace if path == retailctl.NETSYS_LOAD_TRACE else b"exit_code=-1\n"),
        ):
            proof = retailctl.validated_netsys_load_only_proof(
                manifest, termination
            )
            self.assertEqual(proof["termination"], termination)
            wrong = copy.deepcopy(termination)
            wrong["process"]["pid"] = 78
            with self.assertRaisesRegex(SystemExit, "process identity drift"):
                retailctl.validated_netsys_load_only_proof(manifest, wrong)

    def test_encoded_guest_powershell_preserves_quotes_without_shell_reparsing(self):
        completed = mock.Mock(stdout="ok\r\n")
        with mock.patch.object(retailctl, "run", return_value=completed) as invoked:
            self.assertEqual(retailctl.guest_ps_encoded("Write-Output 'Ai'"), "ok")
        arguments = invoked.call_args.args[0]
        self.assertIn("-EncodedCommand", arguments)
        payload = arguments[arguments.index("-EncodedCommand") + 1]
        decoded = retailctl.base64.b64decode(payload).decode("utf-16le")
        self.assertEqual(decoded, "Write-Output 'Ai'")

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
