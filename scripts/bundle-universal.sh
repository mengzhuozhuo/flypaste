#!/usr/bin/env bash
# Build a universal (Intel + Apple Silicon) Flypaste.app bundle.
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

echo "==> Code Signing & Packaging..."
echo "    Cleaning extended attributes..."
xattr -rc "${APP_PATH}" || true

APP_CERT="${APPLE_APP_CERT:-}"

if [ -n "$APP_CERT" ]; then
    echo "    [Production] Signing with Apple Developer Certificate..."
    codesign --force --deep --verify --verbose --sign "$APP_CERT" --options runtime "${APP_PATH}"
    
    echo
    echo "========================================================"
    echo "✅ Done! Signed App ready: ${APP_PATH}"
    echo "========================================================"
else
    echo "    [Local Dev] Ad-hoc signing the App Bundle..."
    codesign --force --deep --sign - "${APP_PATH}"
    echo
    echo "✅ Done! Local App ready: ${APP_PATH}"
    echo "    (To sign for distribution, set APPLE_APP_CERT env var)"
fi
