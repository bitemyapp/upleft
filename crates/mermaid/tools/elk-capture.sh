#!/bin/sh
# Builds the ELK recorder (see elk-capture/) into target/elk-capture and runs
# it over corpus/mermaid, writing corpus/mermaid-elk/<name>.elkrec.
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
cd "$here/elk-capture"
swift build -c release -Xswiftc -enable-testing -Xswiftc -Xfrontend -Xswiftc -enable-implicit-dynamic \
    --scratch-path "$root/target/elk-capture" 2>&1 | grep -E "error|Build complete" || true
"$root/target/elk-capture/release/elk-capture" "$root/corpus/mermaid" "$root/corpus/mermaid-elk"
