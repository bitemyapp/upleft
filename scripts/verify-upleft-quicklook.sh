#!/bin/bash
# The Quick Look section of Downright's Scripts/verify-bundle.sh, for
# Upleft.app. Sourced by scripts/bundle-upleft-quicklook.sh and
# scripts/bundle-upleft-app.sh; `verify_quicklook_plugins APP` reports through
# the caller's `check <1|0> <description>` function.
#
# Downright's checks, adapted to Upleft's flat bundles: both extensions are
# embedded; each carries the math fonts where the resolver looks (the themes
# are compiled into the binary, so there is no MarkdownRender bundle to
# check); each keeps the App Sandbox entitlement and omits get-task-allow;
# each carries the host's versions and an identifier under the host's. Added
# for the Rust build: the executable, package type, extension point and
# principal class, the substituted and rebranded Info.plist, the linked
# Quick Look framework, bundle-relative dylib paths, the canonical build
# version (docs/BUILD-VERSION.md) and a strict signature check.

quicklook_entitlement() {
    local appex="$1" key="$2"
    local key_path="${key//./\\.}"
    # `codesign -d` writes the entitlement plist to stderr along with its
    # diagnostics. Keep that stream, isolate the XML, then query it. Dropping
    # stderr made every correctly signed extension look unsandboxed.
    codesign -d --entitlements :- "$appex" 2>&1 \
        | awk '/<\?xml/{found=1} found{print} /<\/plist>/{exit}' \
        | plutil -extract "$key_path" raw -o - - 2>/dev/null \
        || true
}

verify_quicklook_plugins() {
    local app="$1"
    local plugins="$app/Contents/PlugIns"
    local host_plist="$app/Contents/Info.plist"
    local host_id host_short host_build sdk
    host_id="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$host_plist" 2>/dev/null || true)"
    host_short="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$host_plist" 2>/dev/null || true)"
    host_build="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$host_plist" 2>/dev/null || true)"
    sdk="$(xcrun --sdk macosx --show-sdk-version)"

    check "$([ -d "$plugins/DownrightQL.appex" ] && echo 1 || echo 0)" "DownrightQL.appex embedded"
    check "$([ -d "$plugins/DownrightThumb.appex" ] && echo 1 || echo 0)" "DownrightThumb.appex embedded"
    local appex
    for appex in "$plugins"/*.appex; do
        [ -e "$appex" ] || continue
        local name plist executable expected_id principal point framework bad_dylibs
        name="$(basename "$appex")"
        plist="$appex/Info.plist"
        ext_plist() { /usr/libexec/PlistBuddy -c "Print :$1" "$plist" 2>/dev/null || true; }
        executable="$appex/$(ext_plist CFBundleExecutable)"
        case "$name" in
            DownrightQL.appex)
                expected_id="$host_id.quicklook"; principal=PreviewViewController
                point=com.apple.quicklook.preview; framework=QuickLookUI ;;
            DownrightThumb.appex)
                expected_id="$host_id.thumbnail"; principal=ThumbnailProvider
                point=com.apple.quicklook.thumbnail; framework=QuickLookThumbnailing ;;
            *)
                check 0 "$name is a known extension"; continue ;;
        esac
        check "$([ -f "$executable" ] && [ -x "$executable" ] && echo 1 || echo 0)" "$name executable $(ext_plist CFBundleExecutable)"
        check "$([ "$(ext_plist CFBundlePackageType)" = "XPC!" ] && echo 1 || echo 0)" "$name is an XPC! bundle"
        check "$([ "$(ext_plist NSExtension:NSExtensionPointIdentifier)" = "$point" ] && echo 1 || echo 0)" "$name extension point $point"
        check "$([ "$(ext_plist NSExtension:NSExtensionPrincipalClass)" = "$principal" ] && echo 1 || echo 0)" "$name principal class $principal"
        check "$(! grep -q '\$(' "$plist" && echo 1 || echo 0)" "$name Info.plist has no unsubstituted build settings"
        check "$(! grep -qE 'Downright Quick Look|Downright Thumbnails|com\.ezzy' "$plist" && echo 1 || echo 0)" "$name Info.plist carries the Upleft identity"
        # SwiftMath traps when its fonts are missing; upleft-math degrades
        # instead, so a miss here is silently worse, not fatal. Assert the
        # exact file the resolver opens first.
        check "$([ -f "$appex/mathFonts.bundle/latinmodern-math.otf" ] && echo 1 || echo 0)" "$name carries the math fonts"
        check "$(otool -L "$executable" 2>/dev/null | grep -q "/$framework.framework/" && echo 1 || echo 0)" "$name links $framework"
        bad_dylibs="$(otool -L "$executable" 2>/dev/null | tail -n +2 | grep -vE '\(architecture [^)]*\):$|@rpath|@executable_path|@loader_path|/usr/lib/|/System/' || true)"
        check "$([ -z "$bad_dylibs" ] && echo 1 || echo 0)" "$name has no absolute build-machine dylib paths"
        check "$(vtool -show-build "$executable" 2>/dev/null | grep -q 'minos 14.0' && echo 1 || echo 0)" "$name minos 14.0"
        check "$(vtool -show-build "$executable" 2>/dev/null | grep -q "sdk $sdk" && echo 1 || echo 0)" "$name sdk $sdk"
        check "$([ "$(quicklook_entitlement "$appex" com.apple.security.app-sandbox)" = "true" ] && echo 1 || echo 0)" "$name retains App Sandbox entitlement"
        check "$([ "$(quicklook_entitlement "$appex" com.apple.security.files.user-selected.read-only)" = "true" ] && echo 1 || echo 0)" "$name reads the user-selected file"
        check "$([ "$(quicklook_entitlement "$appex" com.apple.security.get-task-allow)" != "true" ] && echo 1 || echo 0)" "$name omits get-task-allow"
        check "$(codesign --verify --strict "$appex" 2>/dev/null && echo 1 || echo 0)" "$name codesign --verify --strict"
        check "$([ "$(ext_plist CFBundleShortVersionString)" = "$host_short" ] && echo 1 || echo 0)" "$name marketing version matches host"
        check "$([ "$(ext_plist CFBundleVersion)" = "$host_build" ] && echo 1 || echo 0)" "$name build version matches host"
        check "$([ "$(ext_plist CFBundleIdentifier)" = "$expected_id" ] && echo 1 || echo 0)" "$name identifier follows host"
    done
}
