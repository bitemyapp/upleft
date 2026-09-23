#!/usr/bin/env python3
"""Compares the panel benchmarks (`bench-panel`) of downright-app-oracle
(Swift) and upleft-oracle (Rust) stage by stage.

  scripts/panel-bench-compare.py [--rounds N] [--tolerance FRACTION] [--filter TEXT]

Runs each bench command of both oracles N times (default 3), interleaved so
machine load hits both sides alike, and compares the median of the per-run
p50s for every stage, as scripts/bench-compare.py does for drbench. Exits 1 if
any stage is slower than Swift beyond the tolerance (default 0.05).

The benches are the scenario files in corpus/panel-bench/ (one bench-panel
run each; every state is a stage: the panel built and laid out, and drawn
into a bitmap where the state says `"draw": true`; the scene's `prepare`,
which parses the document and builds the model, is not timed).
"""

import argparse
import os
import re
import statistics
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SWIFT = os.path.join(ROOT, "target", "app-oracle", "release", "downright-app-oracle")
RUST = os.path.join(ROOT, "target", "release", "upleft-oracle")
HOME = os.path.join(ROOT, "target", "conform-home")
BENCH_DIRECTORY = os.path.join(ROOT, "corpus", "panel-bench")
LINE = re.compile(r"^\s*(?P<label>.+?)\s+p50\s+(?P<p50>[\d.]+) ms\s+p95\s+(?P<p95>[\d.]+) ms")


def run(binary, command, path):
    environment = dict(os.environ, HOME=HOME, CFFIXED_USER_HOME=HOME)
    with tempfile.TemporaryDirectory() as scratch:
        result = subprocess.run(
            [binary, command, os.path.join(ROOT, path), os.path.join(scratch, "out.json")],
            capture_output=True, text=True, check=False, env=environment,
        )
    if result.returncode != 0:
        sys.exit(f"{binary} {command} {path} failed: {result.stderr.strip()}")
    stages = {}
    for line in result.stdout.splitlines():
        match = LINE.match(line)
        if match:
            stages[match.group("label").strip()] = (float(match.group("p50")), float(match.group("p95")))
    return stages


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--tolerance", type=float, default=0.05)
    parser.add_argument("--filter", default="")
    options = parser.parse_args()
    benches = [
        ("bench-panel", os.path.join("corpus", "panel-bench", name))
        for name in sorted(os.listdir(BENCH_DIRECTORY))
        if name.endswith(".json") and options.filter in name
    ]
    for binary, hint in ((SWIFT, "just app-oracle"), (RUST, "cargo build --release -p upleft-conformance")):
        if not os.path.exists(binary):
            sys.exit(f"{binary} is missing; run `{hint}`")
    os.makedirs(HOME, exist_ok=True)

    swift_runs, rust_runs = [], []
    for round_index in range(options.rounds):
        print(f"round {round_index + 1}/{options.rounds}", file=sys.stderr)
        swift, rust = {}, {}
        for command, path in benches:
            swift.update(run(SWIFT, command, path))
            rust.update(run(RUST, command, path))
        swift_runs.append(swift)
        rust_runs.append(rust)

    labels = [label for label in swift_runs[0] if all(label in r for r in rust_runs + swift_runs)]
    missing = [label for label in swift_runs[0] if label not in rust_runs[0]]
    width = max(len(label) for label in labels)
    print(f"{'stage':<{width}}  {'Swift p50':>10}  {'Rust p50':>10}  {'ratio':>6}  {'Swift p95':>10}  {'Rust p95':>10}")
    failures = []
    for label in labels:
        swift_p50 = statistics.median(r[label][0] for r in swift_runs)
        rust_p50 = statistics.median(r[label][0] for r in rust_runs)
        swift_p95 = statistics.median(r[label][1] for r in swift_runs)
        rust_p95 = statistics.median(r[label][1] for r in rust_runs)
        ratio = rust_p50 / swift_p50 if swift_p50 > 0 else float("inf")
        flag = ""
        if rust_p50 > swift_p50 * (1 + options.tolerance):
            flag = "  SLOWER"
            failures.append(label)
        print(f"{label:<{width}}  {swift_p50:>10.3f}  {rust_p50:>10.3f}  {ratio:>6.2f}  {swift_p95:>10.3f}  {rust_p95:>10.3f}{flag}")
    for label in missing:
        print(f"{label:<{width}}  missing from upleft-oracle  NOT MEASURED")
    if failures or missing:
        print(f"\n{len(failures)} stage(s) slower than Swift beyond {options.tolerance:.0%}; {len(missing)} not measured")
        sys.exit(1)
    print(f"\nall {len(labels)} stages as fast or faster (median p50 of {options.rounds} interleaved rounds)")


if __name__ == "__main__":
    main()
