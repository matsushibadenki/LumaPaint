#!/usr/bin/env python3
"""Validate source/dispatch/probe and callback intervals without summing stages."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('stages', Path(__file__).with_name('analyze-input-stages.py'))
stages = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stages)


class InputStages(unittest.TestCase):
    def capture(self, text):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'phase.log'
            path.write_text(text)
            return stages.analyze({'trialLog':str(path),'planTag':'test','kind':'draw','mode':'on'})

    def test_warmup_unrelated_ids_and_one_nanosecond_conversion(self):
        result = self.capture('''lumapaint-qa-gesture tag=test kind=draw index=0 step=0 timestamp_ns=1000000000
lumapaint-qa-gesture tag=test kind=draw index=0 step=61 timestamp_ns=2000000000
lumapaint-qa-gesture tag=test kind=draw index=2 step=0 timestamp_ns=3000000000
lumapaint-qa-gesture tag=test kind=draw index=2 step=61 timestamp_ns=4000000000
lumapaint-input-queue event=1 age_ns=999999999 event_timestamp_ns=1050000000
lumapaint-input-queue event=10 age_ns=20000000 event_timestamp_ns=3050000001
lumapaint-qa-dispatch tag=test index=2 step=3 source_ns=3050000000 dispatch_ns=3055000000
lumapaint-input-present first_event=1 latest_event=1
lumapaint-input-present first_event=10 latest_event=10
lumapaint-input-stage event=1 stage=render_prepare host_ns=100000000 handler_elapsed_host_ns=999999999
lumapaint-input-stage event=10 stage=pointer_drag host_ns=50000 handler_elapsed_host_ns=100000
lumapaint-input-stage event=10 stage=native_display_callback host_ns=1000 handler_elapsed_host_ns=3001000
lumapaint-input-stage event=10 stage=render_prepare host_ns=100000 handler_elapsed_host_ns=3100000
lumapaint-input-stage event=99 stage=render_prepare host_ns=999999999 handler_elapsed_host_ns=999999999
''')
        self.assertEqual(result['retained_presentations'],1)
        self.assertAlmostEqual(result['synthetic_source_to_dispatch']['p95_ms'],5,places=5)
        self.assertAlmostEqual(result['dispatch_to_pointer_probe']['p95_ms'],15,places=5)
        self.assertEqual(result['handler_to_prepare_start']['p95_ms'],3)
        self.assertEqual(result['handler_end_to_display_callback']['p95_ms'],2.9)
        self.assertEqual(result['stage_host_durations']['render_prepare']['samples'],1)

    def test_shared_source_stamp_does_not_pair_move_with_release(self):
        result=self.capture("""lumapaint-qa-gesture tag=test kind=draw index=2 step=0 timestamp_ns=3000000000
lumapaint-qa-gesture tag=test kind=draw index=2 step=61 timestamp_ns=4000000000
lumapaint-input-queue event=10 age_ns=20000000 event_timestamp_ns=4000000000
lumapaint-qa-dispatch tag=test index=2 step=60 source_ns=4000000000 dispatch_ns=4005000000
lumapaint-qa-dispatch tag=test index=2 step=61 source_ns=4000000000 dispatch_ns=4006000000
lumapaint-input-present first_event=10 latest_event=10
""")
        self.assertEqual(result['retained_presentations'],1)
        self.assertIsNone(result['synthetic_source_to_dispatch'])
        self.assertEqual(result['dispatch_unmatched_or_incompatible'],1)

    def test_empty_input_does_not_invent_intervals(self):
        result = self.capture('lumapaint-pane-source-avoided document=2\n')
        self.assertIsNone(result['synthetic_source_to_dispatch'])
        self.assertIsNone(result['handler_to_display_callback'])
        self.assertEqual(result['retained_presentations'],0)
        self.assertEqual(result['render_forks_avoided'],1)


if __name__ == '__main__':
    unittest.main()
