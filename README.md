# Flypaste

English | [简体中文](README_zh.md)

A clipboard history manager for macOS, built with Rust and GPUI.

## Features

- **Global Hotkey**: Press `Cmd+`` to bring up clipboard history popup
- **Multiple Content Types**: Plain text, rich text (RTF), images, HTML, file paths
- **Quick Select**: Use `Cmd+1` ~ `Cmd+9` to select items directly
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

#### Universal app (Intel + Apple Silicon)

For distribution, build a **Universal Binary** so the same `.app` runs natively on both Apple Silicon and Intel Macs:

```bash
./scripts/bundle-universal.sh
```

The script:

1. Builds release binaries for `aarch64-apple-darwin` and `x86_64-apple-darwin`
2. Merges them with `lipo` into one executable
3. Packages `Flypaste.app` with `cargo bundle` (skipping rebuild so the universal binary is preserved)

Output:

```
target/release/bundle/osx/Flypaste.app
```

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
