#!/usr/bin/env python3
"""Break down retained synthetic gesture timings; never physical input latency."""
import argparse
import json
import math
import re
from pathlib import Path


def distribution(values):
    if not values:
        return None
    values = sorted(values)
    return {"samples": len(values), "p50_ms": values[math.ceil(len(values)*.5)-1]/1e6,
            "p95_ms": values[math.ceil(len(values)*.95)-1]/1e6, "max_ms": values[-1]/1e6}


def analyze(trial):
    text = Path(trial['trialLog']).read_text()
    queues = {int(e): (int(age), int(ts)) for e, age, ts in re.findall(
        r'^lumapaint-input-queue event=(\d+) age_ns=(\d+) event_timestamp_ns=(\d+)', text, re.M)}
    bounds = {}
    for gesture, step, ts in re.findall(r'^lumapaint-qa-gesture tag=\S+ kind=\w+ index=(\d+) step=(0|61) timestamp_ns=(\d+)', text, re.M):
        bounds.setdefault(int(gesture), {})[int(step)] = int(ts)
    retained = {int(latest) for latest in re.findall(r'^lumapaint-input-present first_event=\d+ latest_event=(\d+)', text, re.M)
                if int(latest) in queues and any(g >= 2 and b.get(0, 10**30)-10 <= queues[int(latest)][1] <= b.get(61, -1)+10 for g, b in bounds.items())}
    stages = {}
    starts = []
    callbacks = {}
    handler_ends = {}
    for event, name, duration, elapsed in re.findall(r'^lumapaint-input-stage event=(\d+) stage=(\w+) host_ns=(\d+) handler_elapsed_host_ns=(\d+)', text, re.M):
        event, duration, elapsed = int(event), int(duration), int(elapsed)
        if event not in retained:
            continue
        stages.setdefault(name, []).append(duration)
        if name == 'render_prepare':
            starts.append(max(0, elapsed-duration))
        if name == 'native_display_callback':
            callbacks[event] = max(0, elapsed-duration)
        if name in ('pointer_down', 'pointer_drag', 'pointer_up', 'pan_down', 'pan_drag', 'pan_up'):
            handler_ends[event] = elapsed
    dispatches = {}
    for ts, dispatch in re.findall(r'^lumapaint-qa-dispatch tag=\S+ index=\d+ step=\d+ source_ns=(\d+) dispatch_ns=(\d+)', text, re.M):
        dispatches.setdefault(int(ts), []).append(int(dispatch))
    before_dispatch, after_dispatch = [], []
    for event in retained:
        age, ts = queues[event]
        matches = next((dispatches[t] for t in (ts, ts-1, ts+1) if t in dispatches), [])
        # Last move and mouse-up share source time. Do not pair ambiguously.
        dispatch = matches[0] if len(matches) == 1 else None
        if dispatch is not None:
            wait = dispatch-ts
            # Reject incompatible clocks instead of hiding negative intervals.
            if wait >= -10 and age-wait >= -10:
                before_dispatch.append(max(0, wait))
                after_dispatch.append(max(0, age-wait))
    return {"planTag": trial['planTag'], "kind": trial['kind'], "mode": trial['mode'],
            "retained_presentations": len(retained),
            "dispatch_unmatched_or_incompatible": len(retained)-len(before_dispatch),
            "source_to_probe": distribution([queues[e][0] for e in retained]),
            "synthetic_source_to_dispatch": distribution(before_dispatch),
            "dispatch_to_pointer_probe": distribution(after_dispatch),
            "handler_to_prepare_start": distribution(starts),
            "handler_to_display_callback": distribution(list(callbacks.values())),
            "handler_end_to_display_callback": distribution([callbacks[e]-handler_ends[e] for e in callbacks.keys() & handler_ends.keys() if callbacks[e] >= handler_ends[e]]),
            "stage_host_durations": {k: distribution(v) for k, v in sorted(stages.items())},
            "render_forks": len(re.findall(r'^lumapaint-pane-source document=', text, re.M)),
            "render_forks_avoided": len(re.findall(r'^lumapaint-pane-source-avoided document=', text, re.M)),
            "fork_host_duration": distribution([int(n) for n in re.findall(r'^lumapaint-pane-source document=\d+ host_ns=(\d+)', text, re.M)])}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('capture', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    capture = json.loads(args.capture.read_text())
    trials = capture['trials'] if isinstance(capture, dict) else capture
    result = {"scope": "Matched retained latest input IDs; two warmup gestures excluded. Stage durations overlap and are not additive. Synthetic source→dispatch includes producer scheduling/main dispatch/factory; dispatch→probe includes logging/post/AppKit/pre-pointer work. First-handler presentation tails are reported separately by the continuous summarizer.",
              "trials": [analyze(t) for t in trials if t['kind'] != 'idle']}
    args.output.write_text(json.dumps(result, indent=2)+'\n')
