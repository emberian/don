import importlib.util
import json
import copy
from pathlib import Path
import unittest


SPEC = importlib.util.spec_from_file_location("retailctl", Path(__file__).with_name("retailctl.py"))
assert SPEC and SPEC.loader
retailctl = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(retailctl)


class RetailCtlTests(unittest.TestCase):
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

    def test_command_tokens_accept_the_documented_protocol(self):
        for words in [
            ["observe"], ["pause", "1"], ["speed", "3"], ["speed-up"],
            ["checksum"], ["halt", "0", "12", "13"],
            ["move", "0", "100", "200", "2", "1", "-1", "-1", "0", "12"],
            ["attack", "0", "1", "22", "0", "2", "12", "13"],
            ["trace-move", "0", "12", "100", "200", "120"],
            ["observe-guys", "0", "12"],
        ]:
            retailctl.validate_words(words)

    def test_shell_metacharacters_are_refused(self):
        for words in [["observe&whoami"], ["pause", "1>pwn"], ["move", "$(x)"]]:
            with self.assertRaises(SystemExit):
                retailctl.validate_words(words)

    def test_unknown_verb_is_refused(self):
        with self.assertRaises(SystemExit):
            retailctl.validate_words(["cheat"])

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


if __name__ == "__main__":
    unittest.main()
