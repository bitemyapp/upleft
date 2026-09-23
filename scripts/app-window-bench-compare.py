#!/usr/bin/env python3
"""Compares the document window's timings in downright-app-oracle (Swift) and
upleft-oracle (Rust): document open to the first displayed frame, and a
Live -> Source -> Live mode switch (`bench-app-window`, AppWindowCapture.swift
and crates/conformance/src/dump/app_window_bench.rs).

  scripts/app-window-bench-compare.py [--rounds N] [--tolerance FRACTION]

Runs each scenario in corpus/app-window-bench/ on both oracles N times
(default 3), interleaved so machine load hits both sides alike, and compares
the median of the per-run p50s for every stage. Exits 1 if any stage is
slower than Swift beyond the tolerance (default 0.05). Windows are built
off-screen and never activated, as in the app-window suite; the runs take the
machine-wide window-capture lock.
"""

import argparse
import glob
import json
import os
import statistics
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SWIFT = os.path.join(ROOT, "target", "app-oracle", "release", "downright-app-oracle")
RUST = os.path.join(ROOT, "target", "release", "upleft-oracle")
HOME = os.path.join(ROOT, "target", "conform-home")


def run(binary, scenario):
    environment = dict(os.environ, HOME=HOME, CFFIXED_USER_HOME=HOME)
    with tempfile.TemporaryDirectory() as scratch:
        out = os.path.join(scratch, "out.json")
        result = subprocess.run([binary, "bench-app-window", scenario, out], capture_output=True, text=True, env=environment)
        if result.returncode != 0:
            sys.exit(f"{binary} bench-app-window {scenario} failed: {result.stderr.strip()}")
        with open(out) as handle:
            return {stage["stage"]: stage["p50"] for stage in json.load(handle)}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--tolerance", type=float, default=0.05)
    options = parser.parse_args()
    scenarios = sorted(glob.glob(os.path.join(ROOT, "corpus", "app-window-bench", "*.json")))
    failures = 0
    print(f"{'scenario / stage':<52} {'Swift p50':>10} {'Upleft p50':>11} {'ratio':>7}")
    for scenario in scenarios:
        swift_runs, rust_runs = [], []
        for _ in range(options.rounds):
            swift_runs.append(run(SWIFT, scenario))
            rust_runs.append(run(RUST, scenario))
        name = os.path.splitext(os.path.basename(scenario))[0]
        for stage in swift_runs[0]:
            swift = statistics.median(run[stage] for run in swift_runs)
            rust = statistics.median(run[stage] for run in rust_runs)
            ratio = rust / swift if swift else float("inf")
            flag = "" if ratio <= 1 + options.tolerance else "  SLOWER"
            failures += flag != ""
            print(f"{name + ' / ' + stage:<52} {swift:>10.2f} {rust:>11.2f} {ratio:>7.2f}{flag}")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
