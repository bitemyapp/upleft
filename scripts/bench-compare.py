#!/usr/bin/env python3
"""Runs Downright's drbench (Swift) and upleft-bench (Rust) alternately and
compares every stage both report.

  scripts/bench-compare.py [--rounds N] [--tolerance FRACTION]

Each binary runs N times (default 3), interleaved so machine load hits both
sides alike. For every stage label the median of the per-run p50s is compared.
Upleft's contract is "as fast or faster"; the tolerance (default 0.05) only
absorbs run-to-run noise on stages that are ties. A stage slower than that
fails the gate and the script exits 1.
"""

import argparse
import os
import re
import statistics
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SWIFT = os.path.join(ROOT, "target", "drbench", "release", "drbench")
RUST = os.path.join(ROOT, "target", "release", "upleft-bench")
LINE = re.compile(r"^\s*(?P<label>.+?)\s+p50\s+(?P<p50>[\d.]+) ms\s+p95\s+(?P<p95>[\d.]+) ms")


def run(binary):
    output = subprocess.run([binary], capture_output=True, text=True, check=False).stdout
    stages = {}
    for line in output.splitlines():
        match = LINE.match(line)
        if match:
            label = match.group("label").strip()
            stages[label] = (float(match.group("p50")), float(match.group("p95")))
    return stages


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--tolerance", type=float, default=0.05)
    options = parser.parse_args()
    for binary, hint in ((SWIFT, "just drbench"), (RUST, "cargo build --release -p upleft-bench")):
        if not os.path.exists(binary):
            sys.exit(f"{binary} is missing; run `{hint}`")

    swift_runs, rust_runs = [], []
    for round_index in range(options.rounds):
        print(f"round {round_index + 1}/{options.rounds}", file=sys.stderr)
        swift_runs.append(run(SWIFT))
        rust_runs.append(run(RUST))

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
        print(f"{label:<{width}}  missing from upleft-bench  NOT MEASURED")
    if failures or missing:
        print(f"\n{len(failures)} stage(s) slower than Swift beyond {options.tolerance:.0%}; {len(missing)} not measured")
        sys.exit(1)
    print(f"\nall {len(labels)} stages as fast or faster (median p50 of {options.rounds} interleaved rounds)")


if __name__ == "__main__":
    main()
