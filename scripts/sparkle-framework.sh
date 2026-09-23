#!/bin/sh
# Prints the path of Sparkle.framework 2.9.6, the framework the Upleft app
# binary links and embeds (Downright's host app links the same one; see
# vendor/downright/Package.swift, `exact: "2.9.6"`).
#
# The framework is the binary artifact SwiftPM resolves for oracle/app
# (oracle/app/Package.swift pins Sparkle `exact: "2.9.6"` too). When it is
# missing, the package is resolved first, which fetches Sparkle's release
# archive from GitHub. The version in the framework's Info.plist must be
# 2.9.6, or the script fails.
#
#   scripts/sparkle-framework.sh            the framework's path
#   -F "$(dirname "$(scripts/sparkle-framework.sh)")" -framework Sparkle
#                                           what the app binary's link line needs
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
scratch="$root/target/app-oracle"
framework="$scratch/artifacts/sparkle/Sparkle/Sparkle.xcframework/macos-arm64_x86_64/Sparkle.framework"
expected="2.9.6"

if [ ! -d "$framework" ]; then
    # oracle/app depends on the rebranded SwiftMath by path, so resolving
    # needs the rebranded copy (`just rebrand`).
    if [ ! -d "$root/target/rebranded/downright/Vendor/SwiftMath" ]; then
        python3 "$root/scripts/rebrand.py" copy >&2
    fi
    swift package --package-path "$root/oracle/app" --scratch-path "$scratch" resolve >&2
fi

if [ ! -d "$framework" ]; then
    echo "sparkle-framework.sh: Sparkle.framework not found at $framework after resolving oracle/app" >&2
    exit 1
fi

version=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$framework/Resources/Info.plist")
if [ "$version" != "$expected" ]; then
    echo "sparkle-framework.sh: $framework is Sparkle $version; Upleft pins $expected" >&2
    exit 1
fi

echo "$framework"
