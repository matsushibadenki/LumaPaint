import importlib.util
from pathlib import Path
import unittest
import sys

sys.dont_write_bytecode = True

spec = importlib.util.spec_from_file_location("latency", Path(__file__).with_name("summarize-input-latency.py"))
latency = importlib.util.module_from_spec(spec)
spec.loader.exec_module(latency)


class SummaryTests(unittest.TestCase):
    def test_ignores_unrelated_lines_and_uses_nearest_rank_after_warmup(self):
        lines = ["lumapaint-input-present-summary completed=60"]
        for i in range(1, 101):
            lines.append(f"lumapaint-input-present first_event={i} latest_event={i} coalesced_events=2 first_handler_to_present_host_ns={i * 2000000} latest_handler_to_present_host_ns={i * 1000000} gpu_stream_frame=None")
        result = latency.summarize("\n".join(lines), 10)
        self.assertEqual(result["samples_measured"], 90)
        self.assertEqual(result["latest_handler"], {"p50_ms": 55., "p95_ms": 96., "p99_ms": 100., "max_ms": 100.})
        self.assertEqual(result["first_handler"]["p50_ms"], 110.)
        self.assertEqual(result["marked_events"], 180)
        self.assertEqual(result["latest_over_16_67ms"], 84)

    def test_empty_or_invalid_warmup_is_rejected(self):
        for warmup in [0, -1, 10]:
            with self.assertRaises(ValueError):
                latency.summarize("unrelated log", warmup)

    def test_stages_follow_event_ids_even_when_logged_after_present(self):
        lines = [
            "lumapaint-input-present first_event=1 latest_event=1 coalesced_events=1 first_handler_to_present_host_ns=100 latest_handler_to_present_host_ns=100",
            "lumapaint-input-stage event=1 stage=pointer_down host_ns=9000000 handler_elapsed_host_ns=9000001",
            "lumapaint-input-stage event=2 stage=pointer_drag host_ns=1000000 handler_elapsed_host_ns=1000001",
            "lumapaint-input-present first_event=2 latest_event=3 coalesced_events=2 first_handler_to_present_host_ns=200 latest_handler_to_present_host_ns=100",
            "lumapaint-input-stage event=3 stage=pointer_up host_ns=2000000 handler_elapsed_host_ns=3000000",
            "lumapaint-input-stage event=4 stage=pointer_up host_ns=99000000 handler_elapsed_host_ns=99000000",
        ]
        result = latency.summarize("\n".join(lines), warmup=1)
        self.assertNotIn("pointer_down", result["host_stages"])
        self.assertEqual(result["host_stages"]["pointer_drag"]["samples"], 1)
        self.assertEqual(result["host_stages"]["pointer_up"]["duration"]["p95_ms"], 2.)
        self.assertEqual(result["host_stages"]["pointer_up"]["handler_elapsed"]["p95_ms"], 3.)


if __name__ == "__main__":
    unittest.main()
