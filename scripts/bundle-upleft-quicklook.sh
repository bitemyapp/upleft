#!/bin/bash
# Build DownrightQL.appex and DownrightThumb.appex and embed them in
# Upleft.app, the way Downright's Scripts/bundle-quicklook.sh embeds them in
# Downright.app.
#
# Usage: scripts/bundle-upleft-quicklook.sh [APP=/path/to/Upleft.app]
#        (APP may also come from the environment; the default is
#        target/upleft-app/Upleft.app, what scripts/bundle-upleft-app.sh builds)
#
# An .appex is an ordinary bundle whose executable hands control to
# `NSExtensionMain` instead of running its own `main`. Downright generates a
# throwaway SwiftPM package with a `main.swift` that calls it; Upleft's are
# the `upleft-ql` and `upleft-thumb` binaries (crates/quicklook/src/main.rs,
# crates/thumb/src/main.rs), whose C `main` installs the Mermaid hook (the
# preview only), registers the principal class and calls
# `NSExtensionMain(argc, argv)`.
#
# Same layout, names and contract as bundle-quicklook.sh:
#   * flat bundles: Info.plist, the executable and the resources side by side
#     at the bundle root, which is the shape codesign seals with no special
#     handling;
#   * the Info.plists are the rebranded Config/DownrightQL-Info.plist and
#     DownrightThumb-Info.plist (`just rebrand`) with the Xcode build settings
#     substituted, and the host's own marketing and build versions;
#   * bundle ids com.bitemyapp.upleft.quicklook and .thumbnail;
#   * ad-hoc signatures with Config/QuickLook.entitlements (App Sandbox,
#     read-only access to the file Quick Look hands over), then the host is
#     re-signed.
#
# Differences, on purpose:
#   * NSExtensionPrincipalClass is the bare class name (`PreviewViewController`,
#     `ThumbnailProvider`). Swift's `$(PRODUCT_MODULE_NAME).PreviewViewController`
#     names a Swift class inside module DownrightQL; the Rust classes are
#     registered under the Swift classes' unqualified names (AGENTS.md), so
#     the module prefix is dropped (docs/KNOWN-DIFFERENCES.md);
#   * the themes are compiled into the binaries, so there is no
#     MarkdownRender resource bundle; the math fonts ship as
#     `mathFonts.bundle` at the bundle root, beside the executable, which is
#     where upleft-math's resolver looks;
#   * nothing is registered: no `pluginkit`, no `qlmanage -r`, no
#     `lsregister`, and the extensions are never launched. Downright's
#     follow-up (Scripts/install.sh) is not ported.
#
# Environment:
#   SIGN_HOST=0  leave the host's signature to the caller (bundle-upleft-app.sh
#                signs it after Sparkle.framework);
#   VERIFY=0     skip the layout checks at the end (the caller verifies).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

for argument in "$@"; do
    case "$argument" in
        APP=*) APP="${argument#APP=}" ;;
        *) echo "usage: $0 [APP=/path/to/Upleft.app]" >&2; exit 2 ;;
    esac
done
APP="${APP:-$ROOT/target/upleft-app/Upleft.app}"
SIGN_HOST="${SIGN_HOST:-1}"
VERIFY="${VERIFY:-1}"

[ -d "$APP" ] || { echo "No app bundle at $APP — run scripts/bundle-upleft-app.sh first" >&2; exit 1; }
REBRANDED="$ROOT/target/rebranded/downright"
[ -f "$REBRANDED/Config/DownrightQL-Info.plist" ] || {
    echo "error: $REBRANDED is missing; run \`just rebrand\`" >&2
    exit 1
}

# Take both versions from the host rather than recomputing them: an extension
# has to carry its host's numbers, and build-number.sh returns the epoch on a
# dirty tree, so two independent calls minutes apart never agree.
plist_value() { /usr/libexec/PlistBuddy -c "Print :$1" "$APP/Contents/Info.plist"; }
VERSION="$(plist_value CFBundleShortVersionString)"
BUILD="$(plist_value CFBundleVersion)"

echo "==> Building extensions (release)"
cargo build --release -p upleft-quicklook --bin upleft-ql -p upleft-thumb --bin upleft-thumb
BIN_DIR="$ROOT/target/release"

# Keep Xcode, the direct bundler, and production signing on one entitlement
# contract. The read-only grant covers the file URL Quick Look hands the
# extension; no broader directory entitlement is present.
ENTITLEMENTS="$REBRANDED/Config/QuickLook.entitlements"
MATH_FONTS="$ROOT/vendor/downright/Vendor/SwiftMath/Sources/SwiftMath/mathFonts.bundle"

PLUGINS="$APP/Contents/PlugIns"
rm -rf "$PLUGINS"
mkdir -p "$PLUGINS"

# $(...) placeholders in the checked-in plists are Xcode build settings; this
# script substitutes the same values as bundle-quicklook.sh, except that the
# principal class loses its module prefix (see above).
bundle_extension() {
    local name="$1" binary="$2" bundle_id="$3" src_plist="$4"
    local appex="$PLUGINS/$name.appex"

    echo "==> Assembling $name.appex"
    mkdir -p "$appex"
    cp "$BIN_DIR/$binary" "$appex/$name"

    sed -e "s|\$(EXECUTABLE_NAME)|$name|g" \
        -e "s|\$(PRODUCT_BUNDLE_IDENTIFIER)|$bundle_id|g" \
        -e "s|\$(PRODUCT_MODULE_NAME)\.||g" \
        -e "s|\$(PRODUCT_MODULE_NAME)|$name|g" \
        -e "s|\$(MARKETING_VERSION)|$VERSION|g" \
        -e "s|\$(CURRENT_PROJECT_VERSION)|$BUILD|g" \
        "$src_plist" > "$appex/Info.plist"
    plutil -lint "$appex/Info.plist" >/dev/null

    # The resources beside the executable: the math fonts where the resolver
    # finds them, and the privacy manifest.
    cp -R "$MATH_FONTS" "$appex/mathFonts.bundle"
    cp "$ROOT/vendor/downright/Resources/PrivacyInfo.xcprivacy" "$appex/" 2>/dev/null || true

    codesign --force --sign - --entitlements "$ENTITLEMENTS" \
        --identifier "$bundle_id" "$appex" 2>/dev/null \
        || echo "    (codesign unavailable, continuing unsigned)"

    codesign --verify "$appex" 2>/dev/null \
        || echo "    WARNING: $name.appex failed signature verification"
}

bundle_extension DownrightQL upleft-ql com.bitemyapp.upleft.quicklook "$REBRANDED/Config/DownrightQL-Info.plist"
bundle_extension DownrightThumb upleft-thumb com.bitemyapp.upleft.thumbnail "$REBRANDED/Config/DownrightThumb-Info.plist"

# Embedding new code invalidates the host's signature, so re-sign it. Nested
# code (Sparkle, the extensions) is already signed and is left alone.
if [ "$SIGN_HOST" = "1" ]; then
    echo "==> Re-signing $APP"
    codesign --force --sign - "$APP" 2>/dev/null \
        || echo "    (codesign unavailable, continuing unsigned)"
fi

[ "$VERIFY" = "1" ] || exit 0

# The Quick Look section of Downright's Scripts/verify-bundle.sh
# (scripts/verify-upleft-quicklook.sh, which bundle-upleft-app.sh runs too).
echo
echo "==> Verifying the Quick Look extensions"
FAILURES=0
check() {
    if [ "$1" = "1" ]; then echo "    ok  $2"; else echo "    FAIL $2"; FAILURES=$((FAILURES + 1)); fi
}
# shellcheck source=scripts/verify-upleft-quicklook.sh
source "$ROOT/scripts/verify-upleft-quicklook.sh"
verify_quicklook_plugins "$APP"
if [ "$SIGN_HOST" = "1" ]; then
    check "$(codesign --verify --strict "$APP" 2>/dev/null && echo 1 || echo 0)" "host codesign --verify --strict"
fi
[ "$FAILURES" = "0" ] || { echo "$FAILURES check(s) failed" >&2; exit 1; }

echo
echo "Embedded (not registered with pluginkit or Launch Services, not launched):"
echo "  $PLUGINS/DownrightQL.appex"
echo "  $PLUGINS/DownrightThumb.appex"
