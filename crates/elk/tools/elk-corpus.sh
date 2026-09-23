#!/bin/sh
# Builds the ELK-graph capture tool (see elk-corpus/) into target/elk-corpus.
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
cd "$here/elk-corpus"
swift build -c release -Xswiftc -Xfrontend -Xswiftc -enable-implicit-dynamic --scratch-path "$root/target/elk-corpus" 2>&1 | grep -E "error|warning: var|Build complete" || true
echo "$root/target/elk-corpus/release/elk-corpus"
