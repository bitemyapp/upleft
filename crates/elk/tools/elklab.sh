#!/bin/sh
# Builds the instrumented elk-swift lab into target/elklab (see elklab/instrument.py).
# Usage: crates/elk/tools/elklab.sh   → target/elklab/.build/release/lab <graph.json>
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
lab="$root/target/elklab"
mkdir -p "$lab/Sources/lab"
rm -rf "$lab/Sources/ElkSwift"
cp -R "$root/vendor/elk-swift/Sources/ElkSwift" "$lab/Sources/ElkSwift"
cp "$here/elklab/Package.swift" "$lab/Package.swift"
cp "$here/elklab/main.swift" "$lab/Sources/lab/main.swift"
python3 "$here/elklab/instrument.py" "$lab/Sources/ElkSwift"
cd "$lab" && swift build -c release 2>&1 | grep -E "error:|Compiling lab|Build complete" || true
echo "$lab/.build/release/lab"
