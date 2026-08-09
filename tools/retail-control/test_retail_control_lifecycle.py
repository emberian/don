# SPDX-License-Identifier: GPL-3.0-or-later
"""Source regressions for the injected controller's process-safety boundary."""

from pathlib import Path
import re
import unittest


SOURCE = Path(__file__).with_name("retail_control.c").read_text()


def section(start: str, end: str) -> str:
    begin = SOURCE.index(start)
    return SOURCE[begin : SOURCE.index(end, begin + len(start))]


class RetailControlLifecycleTests(unittest.TestCase):
    def test_large_player_payload_is_not_on_retail_callback_stack_or_event_ring(self):
        callback = section("static void trace_tick", "static BYTE *emit8")
        self.assertNotRegex(callback, r"\bevent_t\s+[A-Za-z_]")
        self.assertIn("static event_t g_callback_event;", SOURCE)
        self.assertIn("static event_t g_trace_event;", SOURCE)
        self.assertIn("player_payload_t *player_payload;", SOURCE)
        self.assertIn("sizeof(event_t) <= 4096", SOURCE)
        self.assertIn("sizeof(player_payload_t) <= 131072", SOURCE)
        self.assertIn("#define WORKER_STACK_RESERVE (128u * 1024u)", SOURCE)
        self.assertIn("STACK_SIZE_PARAM_IS_A_RESERVATION", SOURCE)

    def test_stop_cancels_every_stale_transaction_before_rearm(self):
        cancel = section("static void cancel_main_thread_work", "static void stop_from_main_thread")
        self.assertIn("InterlockedExchange(&g_pending, V_NONE)", cancel)
        self.assertIn("g_verify.active = 0", cancel)
        self.assertIn("g_trace.active = 0", cancel)
        self.assertIn("g_trace.finishing = 0", cancel)
        worker = section("static DWORD WINAPI worker", "BOOL WINAPI DllMain")
        self.assertGreaterEqual(worker.count("fence_request_epoch()"), 4)
        self.assertLess(
            worker.index("fence_request_epoch()"),
            worker.index("if (!install_hook())"),
        )

    def test_stop_ack_is_post_boundary_and_requires_owned_exact_bytes(self):
        stop = section("static void stop_from_main_thread", "static BYTE *emit8")
        post = section("static void __cdecl on_turn_post", "static BYTE *emit8")
        self.assertIn("memcmp(g_hook_addr, g_hook_patch", stop)
        self.assertIn("write_code(g_hook_addr, g_hook_orig, 5)", stop)
        self.assertLess(
            post.index("InterlockedDecrement(&g_hook_inflight)"),
            post.index("publish_main_thread_stop(remaining)"),
        )
        trampoline = section("static BYTE *build_trampoline", "static int resume_one")
        self.assertIn("PAGE_READWRITE", trampoline)
        self.assertIn("PAGE_EXECUTE_READ", trampoline)
        self.assertIn("FlushInstructionCache(g_self_process, m, 0x1000)", trampoline)
        self.assertIn("VirtualFree(m, 0, MEM_RELEASE)", trampoline)

    def test_thread_snapshot_partial_enumeration_is_not_success(self):
        suspend = section("static int suspend_others", "static int resume_all")
        self.assertIn("GetLastError()", suspend)
        self.assertIn("error != ERROR_NO_MORE_FILES", suspend)
        self.assertIn("resume_all(threads, n)", suspend)
        self.assertGreaterEqual(suspend.count("CreateToolhelp32Snapshot"), 2)
        self.assertIn("has_suspended_thread(threads, n, te.th32ThreadID)", suspend)

    def test_serializer_is_heap_bounded_and_payload_is_released_after_write(self):
        writer = section("static int append_json", "static void drain_events")
        drain = section("static void drain_events", "static void write_ready")
        self.assertIn("HeapAlloc(GetProcessHeap(), 0, EVENT_JSON_CAP)", writer)
        self.assertIn("goto serialize_failed", writer)
        self.assertNotIn("char line[262144]", writer)
        self.assertLess(drain.index("write_event(event)"),
                        drain.index("release_player_payload(event->player_payload)"))

    def test_package_max_preserves_measured_half_open_padding(self):
        self.assertRegex(SOURCE, r"#define PACKAGE_CAP\s+0x201u")
        self.assertIn("Random::get(0,2) at 0x00a39d70 is half-open", SOURCE)

    def test_network_observation_is_post_only_and_read_only(self):
        callback = section("static void __cdecl on_turn_frame", "static void cancel_main_thread_work")
        dispatch = section("static int dispatch", "static void trace_tick")
        self.assertIn("!is_post", callback)
        self.assertIn("V_OBSERVE_NETWORK", callback)
        self.assertIn("case V_OBSERVE_NETWORK:\n            return 1;", dispatch)
        self.assertIn('"observe-network"', SOURCE)

    def test_active_checksum_is_disabled_while_passive_gates_remain_structured(self):
        observer = section("static void observe_network", "static void snapshot")
        dispatch = section("static int dispatch", "static void trace_tick")
        for token in [
            "CHECKSUM_PLAYBACK",
            "CHECKSUM_NETWORK_CLEAR",
            "CHECKSUM_IMMEDIATE_PROCESS",
            "CHECKSUM_NO_CONSOLE",
            "CHECKSUM_BAD_PLAY",
            "CHECKSUM_PLAYER_INVALID",
            "CHECKSUM_PLAYER_TERMINAL",
            "CHECKSUM_NOT_CONNECTED",
            "CHECKSUM_PACKAGE_INVALID",
            "CHECKSUM_PACKAGE_FULL",
        ]:
            self.assertIn(token, observer)
        self.assertIn("package_size <= 0x1bf", observer)
        self.assertIn("CHECKSUM_ACTIVE_DISABLED", dispatch)
        self.assertIn("e->mutates_outgoing_package = 0", dispatch)
        self.assertNotIn("RVA_ISSUE_CHECKSUM))(manager)", dispatch)
        parser = section("static int parse_request", "static int read_request")
        self.assertNotIn('!strcmp(t[1], "checksum")', parser)


if __name__ == "__main__":
    unittest.main()
