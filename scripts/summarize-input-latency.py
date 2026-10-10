#!/usr/bin/env python3
"""Summarize one application's handler-to-present log (not input-to-photon)."""
import argparse
import json
import math
import re
from pathlib import Path

SAMPLE = re.compile(
    r"^lumapaint-input-present first_event=(\d+) latest_event=(\d+) "
    r"coalesced_events=(\d+) first_handler_to_present_host_ns=(\d+) "
    r"latest_handler_to_present_host_ns=(\d+)\b"
)
STAGE = re.compile(
    r"^lumapaint-input-stage event=(\d+) stage=([a-z_]+) "
    r"host_ns=(\d+) handler_elapsed_host_ns=(\d+)\b"
)


def distribution(values):
    ordered = sorted(values)
    return {key: ordered[math.ceil(len(ordered) * p / 100) - 1] / 1e6
            for key, p in [("p50_ms", 50), ("p95_ms", 95), ("p99_ms", 99), ("max_ms", 100)]}


def summarize(text, warmup=0):
    if warmup < 0:
        raise ValueError("warmup must be nonnegative")
    samples = [tuple(map(int, m.groups())) for line in text.splitlines()
               if (m := SAMPLE.match(line))]
    measured = samples[warmup:]
    if not measured:
        raise ValueError("no measured input/present samples remain")

    result = {
        "scope": "handler start to present API call; excludes OS queue and physical display",
        "samples_total": len(samples), "warmup_discarded": warmup,
        "samples_measured": len(measured),
        "first_handler": distribution([s[3] for s in measured]),
        "latest_handler": distribution([s[4] for s in measured]),
        "marked_events": sum(s[2] for s in measured),
        "latest_over_16_67ms": sum(s[4] > 16_666_667 for s in measured),
        "latest_over_8_33ms": sum(s[4] > 8_333_333 for s in measured),
    }
    stages = {}
    for line in text.splitlines():
        if (m := STAGE.match(line)):
            event, name, duration, elapsed = m.groups()
            # The whole capture is read first: synchronous stages may be logged
            # after the presentation they belong to. Do not use line ordering.
            if any(first <= int(event) <= latest for first, latest, *_ in measured):
                stages.setdefault(name, []).append((int(duration), int(elapsed)))
    if stages:
        result["host_stages_scope"] = "input IDs in retained presentation ranges; inclusive host durations, not additive"
        result["host_stages"] = {
            name: {"samples": len(values),
                   "duration": distribution([v[0] for v in values]),
                   "handler_elapsed": distribution([v[1] for v in values])}
            for name, values in sorted(stages.items())
        }
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("log", type=Path)
    parser.add_argument("--warmup", type=int, default=0)
    args = parser.parse_args()
    try:
        print(json.dumps(summarize(args.log.read_text(), args.warmup), indent=2))
    except ValueError as error:
        parser.error(str(error))
