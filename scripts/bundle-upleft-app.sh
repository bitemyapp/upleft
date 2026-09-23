#!/bin/bash
# Assemble target/upleft-app/Upleft.app from the cargo build products, the way
# Downright's Scripts/bundle-app.sh assembles Downright.app from SwiftPM's:
# the app binary and `down` in Contents/MacOS, the resources, Sparkle.framework
# in Contents/Frameworks, the Spotlight importer in Contents/Library/Spotlight,
# and an ad-hoc signature.
#
# Identity: Upleft everywhere (AGENTS.md, "App identity"). The Info.plist
# values come from the rebranded Config/Downright-Info.plist (`just rebrand`),
# with the build settings filled in. As in Downright's dev bundles, the Sparkle
# keys are left out unless PRODUCTION=1, which disables the updater
# (UpdateCoordinator.isConfigured).
#
# Differences from bundle-app.sh, on purpose:
#   * no `lsregister -f`: registering a development bundle with Launch
#     Services would make it a candidate handler for .md on this machine;
#   * the themes are compiled into the binary (upleft-render embeds the same
#     JSON files), so there is no MarkdownRender resource bundle; the math
#     fonts ship as Contents/Resources/mathFonts.bundle, where upleft-math's
#     resolver looks;
#   * the Quick Look .appex targets are not built yet (a listed gap).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

REBRANDED="$ROOT/target/rebranded/downright"
[ -f "$REBRANDED/Config/Downright-Info.plist" ] || {
    echo "error: $REBRANDED is missing; run \`just rebrand\`" >&2
    exit 1
}

APP_NAME="Upleft"
BUNDLE_ID="com.bitemyapp.upleft"
# shellcheck disable=SC1091
source "$REBRANDED/Config/version.env"
VERSION="$MARKETING_VERSION"
PRODUCTION="${PRODUCTION:-0}"
# Downright's build-number script, run inside this repository: the commit
# count for a clean tree, a unique epoch-derived number for a dirty one.
BUILD="$("$REBRANDED/Scripts/build-number.sh")"

OUT="${OUT:-$ROOT/target/upleft-app}"
APP="$OUT/$APP_NAME.app"
CONTENTS="$APP/Contents"
MACOS="$CONTENTS/MacOS"
RESOURCES="$CONTENTS/Resources"
FRAMEWORKS="$CONTENTS/Frameworks"

echo "==> Locating Sparkle.framework (2.9.6)"
SPARKLE="$("$ROOT/scripts/sparkle-framework.sh")"
echo "    $SPARKLE"

echo "==> Building (release, build $BUILD)"
UPLEFT_SPARKLE_FRAMEWORK_DIR="$(dirname "$SPARKLE")" \
    cargo build --release -p upleft -p upleft-cli -p upleft-spotlight-importer

BIN_DIR="$ROOT/target/release"

echo "==> Assembling $APP"
rm -rf "$APP"
mkdir -p "$MACOS" "$RESOURCES" "$FRAMEWORKS"

cp "$BIN_DIR/upleft" "$MACOS/$APP_NAME"
cp "$BIN_DIR/down" "$MACOS/down"

# The math fonts, where upleft-math's resolver looks first in an app
# (Contents/Resources/mathFonts.bundle).
cp -R "$ROOT/vendor/downright/Vendor/SwiftMath/Sources/SwiftMath/mathFonts.bundle" "$RESOURCES/mathFonts.bundle"

printf 'APPL????' > "$CONTENTS/PkgInfo"

# The Info.plist: the rebranded template with the build settings Xcode would
# substitute. Dev bundles drop the Sparkle block, as bundle-app.sh's do.
sed \
    -e "s|\$(EXECUTABLE_NAME)|$APP_NAME|g" \
    -e "s|\$(PRODUCT_BUNDLE_IDENTIFIER)|$BUNDLE_ID|g" \
    -e "s|\$(MARKETING_VERSION)|$VERSION|g" \
    -e "s|\$(CURRENT_PROJECT_VERSION)|$BUILD|g" \
    "$REBRANDED/Config/Downright-Info.plist" > "$CONTENTS/Info.plist"
if [ "$PRODUCTION" = "1" ]; then
    [ -n "${SPARKLE_ED25519_PUBLIC_KEY:-}" ] || {
        echo "PRODUCTION=1 requires SPARKLE_ED25519_PUBLIC_KEY" >&2
        exit 1
    }
    /usr/libexec/PlistBuddy -c "Set :SUPublicEDKey $SPARKLE_ED25519_PUBLIC_KEY" "$CONTENTS/Info.plist"
else
    for key in SUEnableAutomaticChecks SUAutomaticallyUpdate SUScheduledCheckInterval \
        SUVerifyUpdateBeforeExtraction SURequireSignedFeed SUEnableSystemProfiling SUFeedURL SUPublicEDKey; do
        /usr/libexec/PlistBuddy -c "Delete :$key" "$CONTENTS/Info.plist" 2>/dev/null || true
    done
    echo "    updater disabled: no Sparkle Info.plist keys in this dev bundle"
fi
plutil -lint "$CONTENTS/Info.plist" >/dev/null

cp "$ROOT/vendor/downright/Resources/AppIcon.icns" "$RESOURCES/AppIcon.icns"
cp "$ROOT/vendor/downright/Resources/AppIcon.png" "$RESOURCES/AppIcon.png"
# The tour, with the Upleft identity. AppDelegate looks it up with
# Bundle.main and hides the start window's guide action when it is absent.
cp "$REBRANDED/Resources/Welcome.md" "$RESOURCES/Welcome.md"
cp "$ROOT/vendor/downright/Resources/PrivacyInfo.xcprivacy" "$RESOURCES/PrivacyInfo.xcprivacy"

echo "==> Embedding Sparkle.framework"
cp -R "$SPARKLE" "$FRAMEWORKS/"

echo "==> Embedding Spotlight importer"
# bundle-spotlight.sh's layout and names. A Spotlight importer must be an
# MH_BUNDLE, so the Rust static library is linked with `clang -bundle`, as
# Downright passes `-Xlinker -bundle` to its C target.
SPOTLIGHT_BUNDLE_IDENTIFIER="$BUNDLE_ID.spotlight"
IMPORTER="$CONTENTS/Library/Spotlight/DownrightSpotlight.mdimporter"
EXECUTABLE="$IMPORTER/Contents/MacOS/DownrightSpotlight"
mkdir -p "$(dirname "$EXECUTABLE")" "$IMPORTER/Contents/Resources"
clang -bundle -mmacosx-version-min=14.0 -o "$EXECUTABLE" \
    -Wl,-force_load,"$BIN_DIR/libupleft_spotlight_importer.a" -Wl,-dead_strip \
    -framework CoreServices -framework CoreFoundation -framework Foundation -framework AppKit -lobjc -liconv
sed \
    -e 's|\$(MARKETING_VERSION)|'"$VERSION"'|g' \
    -e 's|\$(CURRENT_PROJECT_VERSION)|'"$BUILD"'|g' \
    -e 's|com\.bitemyapp\.upleft\.spotlight|'"$SPOTLIGHT_BUNDLE_IDENTIFIER"'|g' \
    "$REBRANDED/Config/DownrightSpotlight-Info.plist" > "$IMPORTER/Contents/Info.plist"
cp "$ROOT/vendor/downright/Resources/Spotlight/schema.xml" "$IMPORTER/Contents/Resources/schema.xml"
codesign --force --sign - --identifier "$SPOTLIGHT_BUNDLE_IDENTIFIER.binary" "$EXECUTABLE"
codesign --force --sign - --identifier "$SPOTLIGHT_BUNDLE_IDENTIFIER" "$IMPORTER"

echo "==> Signing (ad-hoc)"
# Sparkle first (its XPC helpers are nested code), then the app without
# --deep so the framework's signature is preserved.
codesign --force --deep --sign - "$FRAMEWORKS/Sparkle.framework"
codesign --force --sign - "$APP"

echo
echo "==> Verifying bundle layout"
FAILURES=0
check() {
    if [ "$1" = "1" ]; then echo "    ok  $2"; else echo "    FAIL $2"; FAILURES=$((FAILURES + 1)); fi
}
FW="$FRAMEWORKS/Sparkle.framework"
check "$([ -d "$FW" ] && echo 1 || echo 0)" "Sparkle.framework embedded"
check "$([ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$FW/Resources/Info.plist" 2>/dev/null)" = "2.9.6" ] && echo 1 || echo 0)" "Sparkle is 2.9.6"
check "$(find "$FW" -maxdepth 8 -type d -name 'Downloader.xpc' -path '*/XPCServices/*' | grep -q . && echo 1 || echo 0)" "Sparkle Downloader XPC helper"
check "$(find "$FW" -maxdepth 8 -type d -name 'Installer.xpc' -path '*/XPCServices/*' | grep -q . && echo 1 || echo 0)" "Sparkle Installer XPC helper"
check "$([ -x "$FW/Versions/B/Autoupdate" ] && echo 1 || echo 0)" "Sparkle Autoupdate helper"
check "$([ -d "$FW/Versions/B/Updater.app" ] && echo 1 || echo 0)" "Sparkle Updater app"
BIN="$MACOS/$APP_NAME"
check "$([ -x "$BIN" ] && echo 1 || echo 0)" "main executable present"
check "$(otool -L "$BIN" | grep -q '@rpath/Sparkle.framework/Versions/B/Sparkle' && echo 1 || echo 0)" "app links Sparkle"
BAD_DYLIBS="$(otool -L "$BIN" | tail -n +2 | grep -vE '@rpath|@executable_path|@loader_path|/usr/lib/|/System/' || true)"
check "$([ -z "$BAD_DYLIBS" ] && echo 1 || echo 0)" "no absolute dylib paths ($(echo "$BAD_DYLIBS" | tr '\n' ' '))"
check "$([ "$(otool -l "$BIN" | grep -c '@executable_path/../Frameworks' || true)" -ge 1 ] && echo 1 || echo 0)" "bundle-relative rpath present"
check "$([ -x "$MACOS/down" ] && echo 1 || echo 0)" "down CLI embedded"
for binary in "$BIN" "$MACOS/down" "$EXECUTABLE"; do
    check "$(vtool -show-build "$binary" | grep -q 'minos 14.0' && echo 1 || echo 0)" "$(basename "$binary") minos 14.0"
    check "$(vtool -show-build "$binary" | grep -q "sdk $(xcrun --sdk macosx --show-sdk-version)" && echo 1 || echo 0)" "$(basename "$binary") sdk $(xcrun --sdk macosx --show-sdk-version)"
done
plist() { /usr/libexec/PlistBuddy -c "Print :$1" "$CONTENTS/Info.plist" 2>/dev/null || true; }
check "$([ "$(plist CFBundleExecutable)" = "$APP_NAME" ] && echo 1 || echo 0)" "CFBundleExecutable $APP_NAME"
check "$([ "$(plist CFBundleIdentifier)" = "$BUNDLE_ID" ] && echo 1 || echo 0)" "CFBundleIdentifier $BUNDLE_ID"
check "$([ "$(plist CFBundleShortVersionString)" = "$VERSION" ] && echo 1 || echo 0)" "CFBundleShortVersionString $VERSION"
check "$([ "$(plist CFBundleVersion)" = "$BUILD" ] && echo 1 || echo 0)" "CFBundleVersion $BUILD"
if [ "$PRODUCTION" = "1" ]; then
    check "$([ -n "$(plist SUFeedURL)" ] && echo 1 || echo 0)" "production: SUFeedURL present"
else
    check "$([ -z "$(plist SUFeedURL)" ] && echo 1 || echo 0)" "dev bundle: no SUFeedURL (updater disabled)"
fi
check "$(grep -q 'NSPrivacyAccessedAPICategoryUserDefaults' "$RESOURCES/PrivacyInfo.xcprivacy" && echo 1 || echo 0)" "privacy manifest declares UserDefaults"
check "$([ -f "$RESOURCES/mathFonts.bundle/latinmodern-math.otf" ] && echo 1 || echo 0)" "math fonts present"
check "$([ -f "$RESOURCES/Welcome.md" ] && ! grep -q 'Downright' "$RESOURCES/Welcome.md" && echo 1 || echo 0)" "Welcome.md present, Upleft identity"
check "$([ -f "$RESOURCES/AppIcon.icns" ] && echo 1 || echo 0)" "AppIcon.icns present"
SPOTLIGHT_PLIST="$IMPORTER/Contents/Info.plist"
check "$(file "$EXECUTABLE" | grep -q 'bundle' && echo 1 || echo 0)" "Spotlight importer is an MH_BUNDLE"
check "$(nm -gU "$EXECUTABLE" | grep -q ' _MetadataImporterPluginFactory$' && echo 1 || echo 0)" "Spotlight importer exports MetadataImporterPluginFactory"
check "$([ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$SPOTLIGHT_PLIST")" = "DownrightSpotlight" ] && echo 1 || echo 0)" "Spotlight importer executable name"
check "$(grep -q '8B08C4BF-415B-11D8-B3F9-0003936726FC' "$SPOTLIGHT_PLIST" && echo 1 || echo 0)" "Spotlight importer declares the MDImporter plug-in type"
check "$([ -f "$IMPORTER/Contents/Resources/schema.xml" ] && echo 1 || echo 0)" "Spotlight importer schema present"
check "$(codesign --verify --strict "$APP" 2>/dev/null && echo 1 || echo 0)" "codesign --verify --strict"
[ "$FAILURES" = "0" ] || { echo "$FAILURES check(s) failed" >&2; exit 1; }

echo
echo "Built $APP (not registered with Launch Services, not launched)"
