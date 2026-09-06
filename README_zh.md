# Flypaste

[English](README.md) | 简体中文

一款专为 macOS 设计的剪贴板历史记录管理器，使用 Rust 和 GPUI 框架构建。

## 功能特性

- **全局快捷键**: 按下 `Alt + `` 即可呼出剪贴板历史窗口
- **支持多种内容格式**: 纯文本、富文本（RTF）、图片、HTML、文件路径
- **快速选择**: 使用 `Alt+1` ~ `Alt+9` 直接粘贴历史条目
- **强大的搜索功能**: 输入关键字即可搜索，输入 `/` 开头即可使用正则表达式搜索
- **格式保留**: 粘贴时可选择保留原有格式，或强制粘贴为纯文本
- **开机自启**: 支持登录时自动运行（可选）

## 技术栈

- **编程语言**: Rust (2024 Edition)
- **UI 框架**: GPUI (由 Zed Industries 提供)
- **异步运行时**: Tokio
- **剪贴板管理**: arboard + cocoa
- **本地存储**: rusqlite

## 项目结构

```
flypaste/
├── Cargo.toml
├── scripts/
│   └── bundle-universal.sh   # 构建通用架构版本 (Universal Binary)
├── crates/
│   ├── flypaste/        # 主程序入口
│   ├── clipboard/       # 剪贴板监听模块
│   ├── history/         # 历史记录管理与本地存储
│   ├── hotkey/          # 全局热键注册模块
│   ├── injector/        # 焦点应用文本注入模块
│   ├── search/          # 搜索与正则引擎
│   └── settings/        # 用户设置管理
```

## 编译指南

### 环境要求

- macOS 12 或更高版本
- Rust 1.75 或更高版本

### 基础编译运行

```bash
# 安装 Rust (如果还未安装)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 编译项目
cargo build

# 开发环境运行
cargo run --bin flypaste
```

### 打包为 macOS 应用 (.app)

Flypaste 使用 [cargo-bundle](https://github.com/burtonageo/cargo-bundle) 来生成标准的 macOS `.app` 应用程序包。打包相关的配置信息在 `crates/flypaste/Cargo.toml` 的 `[package.metadata.bundle]` 节点中。

首次使用前需要安装 `cargo-bundle`：

```bash
cargo install cargo-bundle
```

#### 构建通用架构应用 (Universal Binary) 与发布打包

为了让应用能在 Apple Silicon (M1/M2/M3/M4) 和 Intel Mac 上都原生运行，我们提供了一键打包脚本 `scripts/bundle-universal.sh`。

```bash
# 完整发布打包（编译多架构 + 代码签名 + 制作 DMG + Apple 官方公证 + 装订票据）
./scripts/bundle-universal.sh

# 本地调试打包（跳过 Apple 公证，生成本地测试使用的 DMG 和 App）
./scripts/bundle-universal.sh --skip-notarize
```

##### 脚本参数选项
- `--cert <name>`: 指定代码签名证书（默认自动检测钥匙串中的 `Developer ID Application` 证书）。
- `--notary-profile <name>`: 指定公证钥匙串凭据名称（默认使用 `developer-notary`）。
- `--skip-notarize`: 跳过 Apple 公证阶段，仅做签名与 DMG 打包。
- `--skip-build`: 跳过 Cargo 编译步骤（直接使用现有 `target/` 二进制快速重试打包/签名）。

##### 工作流程
1. 分别编译 `aarch64-apple-darwin` 和 `x86_64-apple-darwin` 的 Release 版本。
2. 使用 `lipo` 工具合并为单一 Universal Binary 可执行文件。
3. 使用 `cargo bundle` 打包生成标准的 `Flypaste.app`。
4. 自动检测 `Developer ID Application` 证书，开启 Hardened Runtime 与安全时间戳进行代码签名。
5. 打包生成带有 `/Applications` 软链接的 DMG 镜像文件，并对 DMG 签名。
6. 自动提交给苹果公证服务（Notary Service）并等待通过，随后装订公证票据（Staple）。
7. 使用 macOS Gatekeeper (`spctl`) 检验是否可以直接放行。

##### 产物位置
- **DMG 安装镜像（用于分发给他人）**: `target/dist/Flypaste-<version>-universal.dmg`
- **App 应用程序**: `target/release/bundle/osx/Flypaste.app`

> [!NOTE]
> **开源与安全性说明**：
> 打包脚本本身及项目代码库**绝不包含任何密码、私钥或账号敏感信息**。
> 证书与公证凭据均安全保存在开发者本地的 macOS 钥匙串（Keychain）中。其他开发者若没有配置证书，运行脚本会自动降级为本地 Ad-hoc 签名。

#### 构建当前机器的单架构应用

如果你只想在你当前的电脑架构上编译和打包，可以直接运行：

```bash
cargo bundle --release -p flypaste --format osx
```
*注意：请不要使用 `--bin flypaste`，只使用 `-p flypaste` 即可。*

#### 重新生成应用图标

如果你更新了源 PNG 图标，请在打包之前重新生成 `.icns` 文件：

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

## 系统权限要求

正常运行本应用需要以下 macOS 系统权限：
- **辅助功能 (Accessibility)**: 必选。用于自动将剪贴板内容注入到你当前正在使用的窗口中。

## 致谢

- [GPUI](https://gpui.rs) [Zed Industries](https://zed.dev) — 本应用底层的 GPU 硬件加速 UI 框架 (Apache-2.0)
- [Lucide Icons](https://lucide.dev) — 精美且强大的开源图标库 (ISC)

## 开源许可证

本项目基于 [GNU General Public License v3.0](LICENSE) 许可协议开源。
