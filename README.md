# Flypaste

English | [简体中文](README_zh.md)

A clipboard history manager for macOS, built with Rust and GPUI.

## Features

- **Global Hotkey**: Press `Alt+`` to bring up clipboard history popup
- **Multiple Content Types**: Plain text, rich text (RTF), images, HTML, file paths
- **Quick Select**: Use `Alt+1` ~ `Alt+9` to select items directly
- **Search**: Type to search history, prefix with `/` for regex search
- **Format Preservation**: Keep original formatting or paste as plain text
- **Auto-start**: Launch at login (optional)

## Tech Stack

- **Language**: Rust (2024 edition)
- **UI Framework**: GPUI (from Zed Industries)
- **Async Runtime**: Tokio
- **Clipboard**: arboard + cocoa
- **Storage**: rusqlite

## Project Structure

```
flypaste/
├── Cargo.toml
├── scripts/
│   └── bundle-universal.sh   # Build Universal Binary .app
├── crates/
│   ├── flypaste/        # Main application entry
│   ├── clipboard/      # Clipboard monitoring
│   ├── history/        # History management & storage
│   ├── hotkey/         # Global hotkey registration
│   ├── injector/       # Text injection to focused window
│   ├── search/         # Search & regex engine
│   └── settings/       # Settings management
```

## Getting Started

### Prerequisites

- macOS 12+
- Rust 1.75+

### Build

```bash
# Install Rust (if needed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build the project
cargo build

# Run during development
cargo run --bin flypaste
```

### Package as macOS App

Flypaste uses [cargo-bundle](https://github.com/burtonageo/cargo-bundle) to produce a standard `.app` bundle. Bundle metadata lives in `crates/flypaste/Cargo.toml` under `[package.metadata.bundle]`.

Install `cargo-bundle` once:

```bash
cargo install cargo-bundle
```

#### Universal App (Intel + Apple Silicon) & Distribution Packaging

For distribution, we provide an all-in-one packaging script `scripts/bundle-universal.sh` to build a **Universal Binary** that runs natively on both Apple Silicon and Intel Macs:

```bash
# Full distribution build (Multi-arch compilation + Developer ID signing + DMG packaging + Apple Notarization + Stapling)
./scripts/bundle-universal.sh

# Local build (skip Apple Notarization, produces signed DMG and App for local testing)
./scripts/bundle-universal.sh --skip-notarize
```

##### Script Options
- `--cert <name>`: Code signing identity (auto-detects `Developer ID Application` from Keychain if omitted).
- `--notary-profile <name>`: Keychain profile name for `notarytool` (defaults to `developer-notary`).
- `--skip-notarize`: Skip submitting to Apple Notary Service.
- `--skip-build`: Skip cargo compilation and reuse existing universal binaries in `target/`.

##### Workflow
1. Builds release binaries for `aarch64-apple-darwin` and `x86_64-apple-darwin`.
2. Merges them with `lipo` into a single universal executable.
3. Packages `Flypaste.app` using `cargo bundle`.
4. Signs `Flypaste.app` with `Developer ID Application`, Hardened Runtime, and secure timestamp.
5. Builds a distributable `.dmg` installer with an `/Applications` shortcut, and signs the DMG.
6. Submits the DMG to Apple Notary Service and staples the notarization ticket upon success.
7. Verifies Gatekeeper acceptance with `spctl`.

##### Artifacts
- **DMG Installer (for distribution)**: `target/dist/Flypaste-<version>-universal.dmg`
- **Application Bundle**: `target/release/bundle/osx/Flypaste.app`

> [!NOTE]
> **Open Source & Security**:
> The packaging script contains **no passwords, private keys, or secret tokens**.
> All credentials and certificates are stored securely in your local macOS Keychain (`login.keychain`). Other contributors without developer certificates can run the script safely; it will automatically fall back to local ad-hoc signing.

Verify both architectures are included:

```bash
lipo -info target/release/bundle/osx/Flypaste.app/Contents/MacOS/flypaste
# Expected: Architectures in the fat file: ... are: x86_64 arm64
```

Install locally (optional):

```bash
codesign --force --deep --sign - target/release/bundle/osx/Flypaste.app
cp -R target/release/bundle/osx/Flypaste.app /Applications/
```

#### Single-architecture app (current machine only)

To package only for the machine you are building on:

```bash
cargo bundle --release -p flypaste --format osx
```

Do **not** pass `--bin flypaste`; use `-p flypaste` only.

#### Regenerate app icon

If you update the source PNG icon, regenerate the `.icns` file before bundling:

```bash
ICON="crates/flypaste/assets/icons/flypaste-iOS-Default-1024x1024@1x.png"
ICONSET="crates/flypaste/assets/icons/Flypaste.iconset"
ICNS="crates/flypaste/assets/icons/Flypaste.icns"

rm -rf "$ICONSET" && mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
  sips -z $size $size "$ICON" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  sips -z $((size*2)) $((size*2)) "$ICON" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$ICNS" && rm -rf "$ICONSET"
```

## Requirements

- **Accessibility Permission**: Required for injecting text into other applications

## Acknowledgments

- [GPUI](https://gpui.rs) by [Zed Industries](https://zed.dev) — the GPU-accelerated UI framework powering this app (Apache-2.0)
- [Lucide Icons](https://lucide.dev) — beautiful open-source icons (ISC)

## License

This project is licensed under the [GNU General Public License v3.0](LICENSE).
