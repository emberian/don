#!/usr/bin/env python3

from __future__ import annotations

import copy
import http.client
import json
import pathlib
import subprocess
import sys
import threading
import unittest


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import rontoy_host as host


DONFEED_FIXTURE = (
    pathlib.Path(__file__).resolve().parents[2] / "crates" / "donscan" / "fixtures" / "rontoy-observation-v1.ndjson"
)


class FakeClock:
    def __init__(self, value: float = 100.0) -> None:
        self.value = value

    def __call__(self) -> float:
        return self.value

    def advance(self, seconds: float) -> None:
        self.value += seconds


def snapshot(sequence: int = 1) -> dict:
    value = host.demo_snapshot(sequence)
    value["source"]["session_id"] = "test-session"
    return value


def donfeed_observation() -> dict:
    return host.parse_json_object(DONFEED_FIXTURE.read_bytes())


class ParseAndSchemaTests(unittest.TestCase):
    def test_example_and_demo_are_valid(self) -> None:
        host.validate_snapshot(snapshot())
        example_path = pathlib.Path(__file__).with_name("example-snapshot-v1.json")
        host.parse_snapshot_json(example_path.read_bytes())

    def test_duplicate_keys_are_rejected(self) -> None:
        with self.assertRaisesRegex(host.AdmissionError, "duplicate JSON key") as caught:
            host.parse_snapshot_json(b'{"schema_version":1,"schema_version":1}')
        self.assertEqual(caught.exception.code, "duplicate_key")

    def test_nan_is_rejected(self) -> None:
        raw = json.dumps(snapshot()).replace('"stock": 87', '"stock": NaN', 1).encode()
        with self.assertRaises(host.AdmissionError) as caught:
            host.parse_snapshot_json(raw)
        self.assertEqual(caught.exception.code, "invalid_json")

    def test_oversize_is_rejected_before_parse(self) -> None:
        with self.assertRaises(host.AdmissionError) as caught:
            host.parse_snapshot_json(b"{} ", max_body_bytes=2)
        self.assertEqual(caught.exception.status, 413)

    def test_unknown_fields_are_rejected(self) -> None:
        value = snapshot()
        value["surprise"] = True
        with self.assertRaisesRegex(host.AdmissionError, "unknown fields"):
            host.validate_snapshot(value)

    def test_boolean_is_not_an_integer(self) -> None:
        value = snapshot()
        value["source"]["sequence"] = True
        with self.assertRaisesRegex(host.AdmissionError, "source.sequence"):
            host.validate_snapshot(value)

    def test_process_start_identity_must_be_nonzero(self) -> None:
        value = snapshot()
        value["source"]["process_started_100ns"] = "0"
        with self.assertRaisesRegex(host.AdmissionError, "nonzero decimal u64"):
            host.validate_snapshot(value)

    def test_population_can_legitimately_exceed_cap(self) -> None:
        value = snapshot()
        value["economy"]["population"] = {
            "used": 33,
            "cap": 32,
            "idle_citizens": 0,
            "idle_basis": "direct_count",
        }
        host.validate_snapshot(value)
        self.assertEqual(host.analyze_snapshot(value)["metrics"]["population_headroom"], -1)

    def test_unproven_storage_capacity_field_is_rejected(self) -> None:
        value = snapshot()
        value["economy"]["resources"]["food"]["capacity"] = 1
        with self.assertRaisesRegex(host.AdmissionError, "unknown fields"):
            host.validate_snapshot(value)

    def test_mixed_or_partial_capture_is_rejected(self) -> None:
        value = snapshot()
        value["capture"]["frame_end"] += 1
        with self.assertRaises(host.AdmissionError) as caught:
            host.validate_snapshot(value)
        self.assertEqual(caught.exception.code, "incoherent_capture")
        value = snapshot()
        value["capture"]["complete"] = False
        with self.assertRaises(host.AdmissionError) as caught:
            host.validate_snapshot(value)
        self.assertEqual(caught.exception.code, "incoherent_capture")

    def test_rate_cache_age_is_derived_from_wrapping_frame_delta(self) -> None:
        value = snapshot()
        value["game"]["frame"] = 2
        value["capture"]["frame_start"] = 2
        value["capture"]["frame_end"] = 2
        value["economy"]["rate_sample"]["gather_stamp_raw"] = 0xFFFF_FFFF
        value["economy"]["rate_sample"]["age_frames"] = 3
        host.validate_snapshot(value)
        value["economy"]["rate_sample"]["age_frames"] += 1
        with self.assertRaisesRegex(host.AdmissionError, "does not match"):
            host.validate_snapshot(value)

    def test_gatherer_counts_are_optional_and_direct_only(self) -> None:
        value = snapshot()
        for resource in value["economy"]["resources"].values():
            resource.pop("gatherers")
            resource.pop("gatherers_basis")
        host.validate_snapshot(value)
        self.assertIsNone(host.analyze_snapshot(value)["metrics"]["reported_gatherers_total"])
        value = snapshot()
        value["economy"]["resources"]["food"].pop("gatherers_basis")
        with self.assertRaisesRegex(host.AdmissionError, "explicit basis"):
            host.validate_snapshot(value)

    def test_goal_ids_must_be_unique(self) -> None:
        value = snapshot()
        value["goals"].append(copy.deepcopy(value["goals"][0]))
        with self.assertRaisesRegex(host.AdmissionError, "duplicate goal id"):
            host.validate_snapshot(value)


class AdvisorTests(unittest.TestCase):
    def test_analysis_is_pure_and_deterministic(self) -> None:
        value = snapshot(5)
        frozen = copy.deepcopy(value)
        first = host.analyze_snapshot(value)
        second = host.analyze_snapshot(value)
        self.assertEqual(first, second)
        self.assertEqual(value, frozen)
        self.assertNotIn("generated_at", first)

    def test_idle_population_and_goal_rules_have_stable_order(self) -> None:
        value = snapshot(0)
        value["economy"]["population"] = {
            "used": 32,
            "cap": 32,
            "idle_citizens": 2,
            "idle_basis": "direct_count",
        }
        analysis = host.analyze_snapshot(value)
        self.assertEqual(
            [item["code"] for item in analysis["advice"]],
            ["idle_citizens_observed", "population_pressure", "goal_bottleneck_age-up"],
        )
        self.assertEqual(analysis["metrics"]["goal_etas"][0]["bottleneck"], "knowledge")

    def test_unfunded_goal_is_critical(self) -> None:
        value = snapshot(5)
        value["economy"]["resources"]["knowledge"]["income_per_min"] = 0
        advice = host.analyze_snapshot(value)["advice"]
        goal = next(item for item in advice if item["code"] == "goal_bottleneck_age-up")
        self.assertEqual(goal["severity"], "critical")
        self.assertIsNone(goal["evidence"]["eta_minutes"])

    def test_stale_rate_suppresses_only_rate_dependent_advice(self) -> None:
        value = snapshot(5)
        value["economy"]["rate_sample"].update(gather_stamp_raw=29, age_frames=46)
        host.validate_snapshot(value)
        analysis = host.analyze_snapshot(value)
        self.assertTrue(analysis["advice_allowed"])
        self.assertFalse(analysis["rate_advice_allowed"])
        self.assertIn("income_rate_too_old_for_eta", analysis["rate_suppressed_reasons"])
        self.assertEqual(analysis["metrics"]["goal_etas"], [])

    def test_multiplayer_or_reader_gate_suppresses_all_advice(self) -> None:
        for mutate, reason in (
            (lambda value: value["game"].update(mode="multiplayer"), "not_single_player"),
            (lambda value: value["capture"].update(advice_allowed=False), "reader_disallowed_advice"),
        ):
            value = snapshot(5)
            mutate(value)
            with self.subTest(reason):
                analysis = host.analyze_snapshot(value)
                self.assertFalse(analysis["advice_allowed"])
                self.assertIn(reason, analysis["suppressed_reasons"])
                self.assertEqual(analysis["advice"], [])
                self.assertEqual(analysis["metrics"]["goal_etas"], [])

    def test_sampled_delta_rate_is_observation_only(self) -> None:
        value = snapshot(5)
        value["economy"]["rate_sample"] = {
            "basis": "sampled_stock_delta",
            "gather_stamp_raw": None,
            "age_frames": 15,
            "confidence": "estimated",
        }
        host.validate_snapshot(value)
        analysis = host.analyze_snapshot(value)
        self.assertTrue(analysis["advice_allowed"])
        self.assertFalse(analysis["rate_advice_allowed"])
        self.assertIn("income_rate_not_engine_direct", analysis["rate_suppressed_reasons"])
        self.assertEqual(analysis["metrics"]["goal_etas"], [])

    def test_unique_human_evidence_gates_advice(self) -> None:
        value = snapshot(5)
        value["game"]["human_count"] = 2
        analysis = host.analyze_snapshot(value)
        self.assertFalse(analysis["advice_allowed"])
        self.assertIn("not_unique_human", analysis["suppressed_reasons"])


class DonfeedAdapterTests(unittest.TestCase):
    def test_rust_encoder_golden_normalizes_to_instrument_snapshot(self) -> None:
        normalized = host.normalize_donfeed_observation(donfeed_observation())
        self.assertEqual(normalized["source"]["session_id"], "donfeed-4242424242424242")
        self.assertEqual(normalized["source"]["process_started_100ns"], "133800000000000000")
        self.assertEqual(normalized["game"]["seconds"], 826)
        self.assertFalse(normalized["game"]["paused"])
        self.assertEqual(normalized["economy"]["resources"]["food"]["income_per_min"], 84.0)
        self.assertEqual(normalized["economy"]["resources"]["knowledge"]["stock"], 456)
        self.assertNotIn("production", normalized["economy"])
        self.assertNotIn("gatherers", normalized["economy"]["resources"]["food"])
        analysis = host.analyze_snapshot(normalized)
        self.assertTrue(analysis["advice_allowed"])
        self.assertTrue(analysis["rate_advice_allowed"])

    def test_unknown_or_true_pause_suppresses_advice(self) -> None:
        for paused, reason in ((None, "pause_state_unknown"), (True, "game_paused")):
            observation = donfeed_observation()
            observation["game"]["paused"] = paused
            normalized = host.normalize_donfeed_observation(observation)
            analysis = host.analyze_snapshot(normalized)
            with self.subTest(paused=paused):
                self.assertFalse(normalized["capture"]["advice_allowed"])
                self.assertFalse(analysis["advice_allowed"])
                self.assertIn(reason, analysis["suppressed_reasons"])

    def test_donfeed_integrity_evidence_is_not_inferred(self) -> None:
        mutations = (
            (lambda value: value["capture"].update(coherence="torn"), "incoherent_capture"),
            (lambda value: value["leader"].update(flags=3), "invalid_observation"),
            (lambda value: value["source"].update(module_sha256="1" * 64), "unsupported_build"),
            (lambda value: value["leader"].update(gather_cache_age_frames=11), "invalid_observation"),
        )
        for mutate, code in mutations:
            observation = donfeed_observation()
            mutate(observation)
            with self.subTest(code=code), self.assertRaises(host.AdmissionError) as caught:
                host.normalize_donfeed_observation(observation)
            self.assertEqual(caught.exception.code, code)

class StoreTests(unittest.TestCase):
    def setUp(self) -> None:
        self.mono = FakeClock()
        self.wall = FakeClock(1_000.0)
        self.store = host.SnapshotStore(min_interval_ms=100, monotonic=self.mono, wall_time=self.wall)

    def test_latest_only_and_waiter_skip_to_current(self) -> None:
        first = self.store.admit(snapshot(1))
        self.mono.advance(0.1)
        second = self.store.admit(snapshot(2))
        self.assertEqual(first.revision, 1)
        self.assertIs(self.store.latest(), second)
        self.assertIs(self.store.wait_after(0, 0), second)
        self.assertIsNone(self.store.wait_after(2, 0))
        self.assertFalse(hasattr(self.store, "history"))

    def test_rate_limit_does_not_replace_latest(self) -> None:
        first = self.store.admit(snapshot(1))
        with self.assertRaises(host.AdmissionError) as caught:
            self.store.admit(snapshot(2))
        self.assertEqual(caught.exception.code, "rate_limited")
        self.assertIs(self.store.latest(), first)

    def test_sequence_frame_and_capture_are_monotonic(self) -> None:
        self.store.admit(snapshot(2))
        cases = []
        stale_sequence = snapshot(2)
        cases.append((stale_sequence, "stale_sequence"))
        stale_capture = snapshot(3)
        stale_capture["source"]["captured_at_ms"] = 0
        cases.append((stale_capture, "stale_capture"))
        stale_frame = snapshot(3)
        stale_frame["game"]["frame"] = 0
        stale_frame["capture"]["frame_start"] = 0
        stale_frame["capture"]["frame_end"] = 0
        stale_frame["economy"]["rate_sample"]["gather_stamp_raw"] = 0
        stale_frame["economy"]["rate_sample"]["age_frames"] = 0
        cases.append((stale_frame, "stale_frame"))
        for value, expected in cases:
            self.mono.advance(0.1)
            with self.subTest(expected), self.assertRaises(host.AdmissionError) as caught:
                self.store.admit(value)
            self.assertEqual(caught.exception.code, expected)
        self.assertEqual(self.store.latest().snapshot["source"]["sequence"], 2)

    def test_new_session_requires_explicit_host_restart(self) -> None:
        self.store.admit(snapshot(10))
        self.mono.advance(0.1)
        value = snapshot(0)
        value["source"]["session_id"] = "new-session"
        with self.assertRaises(host.AdmissionError) as caught:
            self.store.admit(value)
        self.assertEqual(caught.exception.code, "source_conflict")

    def test_same_session_is_bound_to_process_fingerprint(self) -> None:
        self.store.admit(snapshot(1))
        self.mono.advance(0.1)
        value = snapshot(2)
        value["source"]["module_sha256"] = "1" * 64
        with self.assertRaises(host.AdmissionError) as caught:
            self.store.admit(value)
        self.assertEqual(caught.exception.code, "identity_changed")

    def test_unpaused_stalled_frame_is_not_freshness(self) -> None:
        self.store.admit(snapshot(1))
        self.mono.advance(0.1)
        value = snapshot(2)
        value["game"]["frame"] = 15
        value["capture"]["frame_start"] = 15
        value["capture"]["frame_end"] = 15
        value["economy"]["rate_sample"]["gather_stamp_raw"] = 15
        value["economy"]["rate_sample"]["age_frames"] = 0
        with self.assertRaises(host.AdmissionError) as caught:
            self.store.admit(value)
        self.assertEqual(caught.exception.code, "stalled_frame")

    def test_status_exposes_age_and_drop_counters(self) -> None:
        self.store.admit(snapshot(1))
        self.wall.advance(1.25)
        self.mono.advance(1.25)
        self.store.reject("test_drop")
        status = self.store.status()
        self.assertEqual(status["latest"]["age_ms"], 1250)
        self.assertFalse(status["latest"]["stale"])
        self.assertEqual(status["rejections_by_code"], {"test_drop": 1})


class HttpTests(unittest.TestCase):
    def setUp(self) -> None:
        self.store = host.SnapshotStore(min_interval_ms=0)
        self.server = host.RoNtoyServer(("127.0.0.1", 0), self.store, heartbeat_seconds=0.05, ingest_token="test-token")
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.port = self.server.server_port

    def tearDown(self) -> None:
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)

    def request(self, method: str, path: str, body: bytes | None = None, headers: dict | None = None):
        connection = http.client.HTTPConnection("127.0.0.1", self.port, timeout=2)
        connection.request(method, path, body=body, headers=headers or {})
        response = connection.getresponse()
        payload = response.read()
        connection.close()
        return response.status, response.getheaders(), payload

    def post(self, value: dict, token: str = "test-token"):
        raw = json.dumps(value).encode()
        return self.request(
            "POST",
            "/v1/snapshot",
            raw,
            {"Content-Type": "application/json", "X-RoNtoy-Token": token},
        )

    def test_health_status_and_empty_latest(self) -> None:
        status, _, payload = self.request("GET", "/healthz")
        self.assertEqual(status, 200)
        self.assertEqual(json.loads(payload), {"ok": True})
        status, _, payload = self.request("GET", "/v1/latest")
        self.assertEqual(status, 404)
        self.assertEqual(json.loads(payload)["error"]["code"], "no_snapshot")

    def test_ingest_requires_token_and_json(self) -> None:
        status, _, payload = self.post(snapshot(), token="wrong")
        self.assertEqual(status, 401)
        self.assertEqual(json.loads(payload)["error"]["code"], "unauthorized")
        raw = json.dumps(snapshot()).encode()
        status, _, _ = self.request(
            "POST", "/v1/snapshot", raw, {"Content-Type": "text/plain", "X-RoNtoy-Token": "test-token"}
        )
        self.assertEqual(status, 415)

    def test_dns_rebinding_host_is_rejected(self) -> None:
        status, _, payload = self.request(
            "GET", "/v1/latest", headers={"Host": f"attacker.example:{self.port}"}
        )
        self.assertEqual(status, 421)
        self.assertEqual(json.loads(payload)["error"]["code"], "invalid_host")

    def test_post_latest_and_status(self) -> None:
        status, _, payload = self.post(snapshot())
        self.assertEqual(status, 202)
        admitted = json.loads(payload)
        self.assertEqual(admitted["stream_revision"], 1)
        status, _, payload = self.request("GET", "/v1/latest")
        self.assertEqual(status, 200)
        self.assertEqual(json.loads(payload), admitted)
        status, _, payload = self.request("GET", "/v1/status")
        self.assertEqual(status, 200)
        self.assertEqual(json.loads(payload)["latest"]["sequence"], 1)

    def test_cross_language_fixture_pipes_through_bridge_into_latest_slot(self) -> None:
        completed = subprocess.run(
            [
                sys.executable,
                str(pathlib.Path(__file__).with_name("bridge.py")),
                "--port",
                str(self.port),
                "--token",
                "test-token",
            ],
            input=DONFEED_FIXTURE.read_bytes(),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=5,
            check=False,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr.decode())
        self.assertIn(b"1 accepted, 0 dropped", completed.stderr)
        status, _, payload = self.request("GET", "/v1/latest")
        self.assertEqual(status, 200)
        latest = json.loads(payload)
        self.assertEqual(latest["snapshot"]["source"]["reader_version"], "donfeed-observation-1.0")
        self.assertEqual(latest["snapshot"]["game"]["frame"], 12400)
        self.assertTrue(latest["analysis"]["advice_allowed"])

    def test_stream_sends_current_snapshot_without_history(self) -> None:
        self.post(snapshot())
        connection = http.client.HTTPConnection("127.0.0.1", self.port, timeout=2)
        connection.request("GET", "/v1/stream")
        response = connection.getresponse()
        self.assertEqual(response.status, 200)
        self.assertEqual(response.getheader("Content-Type"), "text/event-stream; charset=utf-8")
        self.assertEqual(response.readline(), b"id: 1\n")
        self.assertEqual(response.readline(), b"event: snapshot\n")
        data_line = response.readline()
        self.assertTrue(data_line.startswith(b"data: "))
        connection.close()

    def test_dashboard_is_same_origin_and_has_no_external_assets(self) -> None:
        status, headers, payload = self.request("GET", "/")
        self.assertEqual(status, 200)
        self.assertIn(b'src="./js/rontoy.js"', payload)
        self.assertIn("Content-Security-Policy", dict(headers))
        self.assertNotIn(b"https://", payload)
        status, _, javascript = self.request("GET", "/js/rontoy.js")
        self.assertEqual(status, 200)
        self.assertIn(b"/v1/stream", javascript)
        status, headers, head_body = self.request("HEAD", "/")
        self.assertEqual(status, 200)
        self.assertGreater(int(dict(headers)["Content-Length"]), 1000)
        self.assertEqual(head_body, b"")

    def test_server_refuses_non_loopback_bind(self) -> None:
        with self.assertRaisesRegex(ValueError, "non-loopback"):
            host.RoNtoyServer(("0.0.0.0", 0), host.SnapshotStore())


if __name__ == "__main__":
    unittest.main()
