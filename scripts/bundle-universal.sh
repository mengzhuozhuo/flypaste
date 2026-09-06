#!/usr/bin/env bash
# Build, sign, package, and notarize a universal (Intel + Apple Silicon) Flypaste app & DMG.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PACKAGE="flypaste"
ARM_TARGET="aarch64-apple-darwin"
INTEL_TARGET="x86_64-apple-darwin"
ARM_BIN="target/${ARM_TARGET}/release/${PACKAGE}"
INTEL_BIN="target/${INTEL_TARGET}/release/${PACKAGE}"
UNIVERSAL_BIN="target/universal/release/${PACKAGE}"
APP_PATH="target/release/bundle/osx/Flypaste.app"
DIST_DIR="target/dist"

# Default configuration
APP_CERT="${APPLE_APP_CERT:-}"
NOTARY_PROFILE="${NOTARY_PROFILE:-developer-notary}"
SKIP_NOTARIZE=0
SKIP_BUILD=0

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --cert)
            APP_CERT="$2"
            shift 2
            ;;
        --notary-profile)
            NOTARY_PROFILE="$2"
            shift 2
            ;;
        --skip-notarize|--no-notarize)
            SKIP_NOTARIZE=1
            shift
            ;;
        --skip-build)
            SKIP_BUILD=1
            shift
            ;;
        -h|--help)
            echo "Usage: $0 [options]"
            echo ""
            echo "Options:"
            echo "  --cert <name>             Apple Code Signing Certificate (auto-detected if omitted)"
            echo "  --notary-profile <name>   notarytool keychain profile (default: developer-notary)"
            echo "  --skip-notarize           Build and sign DMG without submitting to Apple Notary"
            echo "  --skip-build              Skip compiling Rust binaries (reuse target/ binaries)"
            echo "  -h, --help                Show this help message"
            exit 0
            ;;
        *)
            echo "Unknown argument: $1"
            exit 1
            ;;
    esac
done

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' crates/flypaste/Cargo.toml | head -n 1)
VERSION="${VERSION:-0.1.0}"
DMG_NAME="Flypaste-${VERSION}-universal.dmg"
DMG_PATH="${DIST_DIR}/${DMG_NAME}"
DMG_STAGING="${DIST_DIR}/dmg_staging"

echo "========================================================"
echo "🚀 Building Flypaste v${VERSION} Universal Bundle"
echo "========================================================"

if [ "$SKIP_BUILD" -eq 0 ]; then
    echo "==> Ensuring Rust targets are installed..."
    rustup target add "${ARM_TARGET}" "${INTEL_TARGET}"

    echo "==> Building for Apple Silicon (${ARM_TARGET})..."
    cargo build --release --target "${ARM_TARGET}" -p "${PACKAGE}"

    echo "==> Building for Intel (${INTEL_TARGET})..."
    cargo build --release --target "${INTEL_TARGET}" -p "${PACKAGE}"

    echo "==> Creating universal binary with lipo..."
    mkdir -p target/universal/release
    lipo -create "${ARM_BIN}" "${INTEL_BIN}" -output "${UNIVERSAL_BIN}"
    lipo -info "${UNIVERSAL_BIN}"

    echo "==> Staging universal binary for cargo-bundle..."
    mkdir -p target/release
    cp "${UNIVERSAL_BIN}" "target/release/${PACKAGE}"
    chmod +x "target/release/${PACKAGE}"

    echo "==> Bundling Flypaste.app (skip rebuild to keep universal binary)..."
    CARGO_BUNDLE_SKIP_BUILD=1 cargo bundle --release -p "${PACKAGE}" --format osx

    echo "==> Verifying bundled executable..."
    lipo -info "${APP_PATH}/Contents/MacOS/${PACKAGE}"
else
    echo "==> Skipping Rust build step as requested (--skip-build)..."
fi

# Clean extended attributes
echo "==> Cleaning extended attributes..."
xattr -cr "${APP_PATH}" || true

# Auto-detect Developer ID Application certificate if not provided
if [ -z "$APP_CERT" ]; then
    echo "==> Checking Keychain for 'Developer ID Application' certificate..."
    DETECTED_CERT=$(security find-identity -p codesigning -v 2>/dev/null | grep "Developer ID Application:" | head -n 1 | sed -E 's/.*"(Developer ID Application: [^"]+)".*/\1/' || true)
    if [ -n "$DETECTED_CERT" ]; then
        APP_CERT="$DETECTED_CERT"
        echo "    Auto-detected: ${APP_CERT}"
    fi
fi

sign_with_retry() {
    local target="$1"
    local max_attempts=5
    local attempt=1
    local delay=3

    while [ "$attempt" -le "$max_attempts" ]; do
        echo "    Signing attempt $attempt/$max_attempts: $target"
        if codesign --force --deep --verify --verbose \
            --sign "$APP_CERT" \
            --options runtime \
            --timestamp \
            "$target"; then
            return 0
        fi

        echo "⚠️  Code signing attempt $attempt failed (Apple timestamp service may be temporarily unreachable)."
        if [ "$attempt" -lt "$max_attempts" ]; then
            echo "    Retrying in ${delay}s... (Tips: Check VPN/proxy, or test with: curl -I http://timestamp.apple.com/ts01)"
            sleep "$delay"
        fi
        attempt=$((attempt + 1))
    done

    echo "❌ Code signing failed after $max_attempts attempts."
    echo "   Error: Apple timestamp service is not available."
    echo "   Please check:"
    echo "   1. Internet connectivity to Apple servers (test: curl -I http://timestamp.apple.com/ts01)"
    echo "   2. If using a proxy/VPN, make sure HTTP port 80 to timestamp.apple.com is allowed/bypassed"
    return 1
}

# Code Signing
if [ -n "$APP_CERT" ]; then
    echo "==> Code Signing Flypaste.app with Developer ID..."
    echo "    Certificate: ${APP_CERT}"
    sign_with_retry "${APP_PATH}"

    echo "==> Verifying App code signature..."
    codesign --verify --deep --strict --verbose=2 "${APP_PATH}"
else
    echo "⚠️  No Developer ID certificate specified or detected."
    echo "    Falling back to local ad-hoc signing (App will ONLY run locally, not for distribution)."
    codesign --force --deep --sign - "${APP_PATH}"
    SKIP_NOTARIZE=1
fi

# Packaging DMG
echo "==> Creating DMG installer package..."
mkdir -p "${DIST_DIR}"
rm -rf "${DMG_STAGING}" "${DMG_PATH}"
mkdir -p "${DMG_STAGING}"

cp -R "${APP_PATH}" "${DMG_STAGING}/"
ln -s /Applications "${DMG_STAGING}/Applications"

hdiutil create \
    -volname "Flypaste" \
    -srcfolder "${DMG_STAGING}" \
    -ov \
    -format UDZO \
    "${DMG_PATH}"

rm -rf "${DMG_STAGING}"

# Sign DMG
if [ -n "$APP_CERT" ]; then
    echo "==> Code Signing DMG container..."
    attempt=1
    while [ "$attempt" -le 5 ]; do
        if codesign --force --sign "$APP_CERT" --timestamp "${DMG_PATH}"; then
            break
        fi
        echo "⚠️  DMG signing attempt $attempt failed, retrying in 3s..."
        sleep 3
        attempt=$((attempt + 1))
        if [ "$attempt" -gt 5 ]; then
            echo "❌ Failed to sign DMG."
            exit 1
        fi
    done
fi

# Notarization and Stapling
if [ -n "$APP_CERT" ] && [ "$SKIP_NOTARIZE" -eq 0 ]; then
    echo "==> Submitting DMG to Apple Notary Service..."
    echo "    Profile: ${NOTARY_PROFILE}"
    echo "    Waiting for Apple notarization to complete (usually 1-3 minutes)..."

    if xcrun notarytool submit "${DMG_PATH}" --keychain-profile "${NOTARY_PROFILE}" --wait; then
        echo "==> Stapling notarization ticket to DMG and App..."
        xcrun stapler staple "${DMG_PATH}"
        xcrun stapler staple "${APP_PATH}"

        echo "==> Verifying Gatekeeper assessment..."
        spctl --assess -vv --type execute "${APP_PATH}" || true
        spctl --assess -vv --type open --context context:primary-signature "${DMG_PATH}" || true
    else
        echo "❌ Apple Notarization failed!"
        echo "   Check submission logs with:"
        echo "   xcrun notarytool history --keychain-profile ${NOTARY_PROFILE}"
        exit 1
    fi
fi

echo
echo "========================================================"
if [ -n "$APP_CERT" ] && [ "$SKIP_NOTARIZE" -eq 0 ]; then
    echo "🎉 SUCCESS! Production DMG ready for distribution:"
    echo "   DMG: ${DMG_PATH}"
    echo "   App: ${APP_PATH}"
    echo "   Status: Fully Signed, Notarized & Stapled"
    echo "   Users can download and double-click to open directly!"
elif [ -n "$APP_CERT" ]; then
    echo "✅ Signed DMG created (Notarization skipped):"
    echo "   DMG: ${DMG_PATH}"
    echo "   App: ${APP_PATH}"
else
    echo "⚠️  Ad-hoc build complete (Not for distribution):"
    echo "   App: ${APP_PATH}"
    echo "   DMG: ${DMG_PATH}"
fi
echo "========================================================"
