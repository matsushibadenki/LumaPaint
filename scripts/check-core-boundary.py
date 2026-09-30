#!/usr/bin/env python3
"""Reject rendering, windowing and SVG runtime dependencies anywhere below core."""
import pathlib
import subprocess
import sys

root = pathlib.Path(__file__).resolve().parents[1]
command = ["cargo", "tree", "-p", "lumapaint-core", "--edges", "normal", "--prefix", "none", "--locked"]
if "--offline" in sys.argv[1:]:
    command.append("--offline")
result = subprocess.run(command, cwd=root, check=True, capture_output=True, text=True)
names = {line.split()[0] for line in result.stdout.splitlines() if line.strip()}
forbidden = {"skia-safe", "skia-bindings", "tiny-skia", "tiny-skia-path", "usvg", "resvg",
             "wgpu", "wgpu-core", "wgpu-hal", "tauri", "tao", "wry", "fontdb", "lumapaint-svg", "lumapaint-renderer"}
violations = sorted(names & forbidden)
if violations:
    sys.exit("Core boundary violation: " + ", ".join(violations))
print("Core boundary OK: no rendering, SVG runtime, font database or window dependencies.")
