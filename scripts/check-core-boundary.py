#!/usr/bin/env python3
"""Enforce inward dependencies for the document, renderer and independent I/O adapters."""
import pathlib
import subprocess
import sys

root = pathlib.Path(__file__).resolve().parents[1]
engines = {"skia-safe", "skia-bindings", "resvg", "wgpu", "wgpu-core", "wgpu-hal",
           "tauri", "tao", "wry", "lumapaint-renderer"}
checks = {
    "lumapaint-core": engines | {"tiny-skia", "tiny-skia-path", "usvg", "fontdb",
                                "lumapaint-svg", "lumapaint-formats"},
    "lumapaint-formats": engines | {"tiny-skia", "lumapaint-svg"},
    "lumapaint-renderer": {"lumapaint-formats", "tauri", "tao", "wry"},
}
for package, forbidden in checks.items():
    command = ["cargo", "tree", "-p", package, "--edges", "normal", "--prefix", "none", "--locked"]
    if "--offline" in sys.argv[1:]:
        command.append("--offline")
    result = subprocess.run(command, cwd=root, check=True, capture_output=True, text=True)
    names = {line.split()[0] for line in result.stdout.splitlines() if line.strip()}
    violations = sorted(names & forbidden)
    if violations:
        sys.exit(package + " boundary violation: " + ", ".join(violations))
    print(package + " boundary OK.")
