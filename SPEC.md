# Flypaste - Pasteboard Management Tool

## 1. 需求细节 (Requirements)

### 1.1 核心功能

#### 1.1.1 快捷键唤起粘贴界面
- 用户通过全局快捷键（默认 `Cmd+``）唤出粘贴历史浮窗
- 浮窗显示在光标附近
- 显示最近 N 条粘贴历史记录（可滚动），前 9 条带编号标签 (1-9)
- 支持鼠标点击选择
- 支持 `Cmd+1` ~ `Cmd+9` 快速选择对应项
- 按 `Esc` 或点击浮窗外部关闭浮窗

#### 1.1.2 粘贴历史管理
- 自动监控系统粘贴板变化
- 记录每次粘贴板内容变更
- 支持多种内容类型：
  - 纯文本 (plain text)
  - 富文本/RTF (rich text)
  - 图片 (images)
  - HTML
  - 文件路径 (file paths)
- 去重：连续相同内容不重复记录
- 支持手动删除单条历史
- 支持清空全部历史
- **搜索功能**：
  - 唤起浮窗后可直接输入搜索
  - 支持正则表达式搜索（以 `/` 开头表示正则）
  - 搜索匹配内容预览
  - 匹配到的项同样支持 `Cmd+1` ~ `Cmd+9` 快速选择

#### 1.1.3 选择即粘贴
- 选择历史记录后（鼠标点击或 `Cmd+数字`）
- 关闭浮窗
- **直接插入到当前焦点应用的光标位置**
- 保留文本格式（可选配置）

#### 1.1.4 设置界面
- 快捷键配置：自定义唤起快捷键
- 开机自动启动：可选启用/禁用
- 历史记录总数据量限制：10MiB / 50MiB / 100MiB / 500MiB / 1GiB / 无限制
- 格式保留选项：
  - 保留原始格式
  - 仅保留纯文本
- 启动方式：菜单栏图标 (menu bar only)
- 正则表达式默认启用：可选

### 1.2 用户交互流程

```
用户按下 Cmd+`
      ↓
显示粘贴历史浮窗（带动画）
      ↓
用户可直接输入搜索（支持正则，以 / 开头）
      ↓
用户点击 或 Cmd+1~9 选择
      ↓
关闭浮窗
      ↓
将内容插入到焦点窗口光标处
```

---

## 2. 技术栈 (Technology Stack)

### 2.1 参考 Zed 的架构

Zed 使用 Rust 构建高性能应用，核心框架为 **GPUI** (GPU-accelerated UI)，但 Flypaste 作为 macOS 专用工具，可以采用更轻量的方案。

### 2.2 推荐技术选型

| 层次 | 技术选型 | 说明 |
|------|----------|------|
| **语言** | Rust (2024 edition) | 性能和内存安全 |
| **UI 框架** | `gpui` | Zed 自研 UI 框架，支持 GPU 渲染 |
| **异步运行时** | `tokio` | Zed 使用的 async 运行时 |
| **粘贴板监控** | `arboard` + `cocoa` | 跨平台粘贴板访问 |
| **全局快捷键** | `rdev` 或 `cocoa` | 全局热键注册 |
| **窗口管理** | `gpui_macos` | 原生窗口创建 |
| **数据存储** | `rusqlite` | 社区主流 SQLite 封装 |
| **辅助功能** | `macos-audit` / ` Accessibility API` | 模拟输入 |
| **菜单栏** | `menu` (Zed crate) | 菜单栏应用支持 |

### 2.3 Zed 的关键 crates 参考

从 Zed 的 Cargo.toml 中提取的可复用依赖：

```toml
# UI 框架
gpui = { path = "crates/gpui", default-features = false }
gpui_macos = { path = "crates/gpui_macos", default-features = false }

# 异步
tokio = { version = "1" }

# 数据存储
rusqlite = { version = "0.32", features = ["bundled"] }

# macOS 原生接口
cocoa = "=0.26.0"
cocoa-foundation = "=0.2.0"
core-foundation = "=0.10.0"
objc2-foundation = { version = "=0.3.2", features = ["NSAttributedString", "NSString"] }

# 窗口/菜单
menu = { path = "crates/menu" }

# 设置管理
settings = { path = "crates/settings" }
serde = { version = "1.0.221", features = ["derive"] }
serde_json = "1.0.144"

# 搜索
regex = "1.10"
```

### 2.4 项目结构 (参考 Zed)

```
flypaste/
├── Cargo.toml
├── crates/
│   ├── flypaste/              # 主应用入口
│   │   ├── main.rs
│   │   └── app.rs            # 应用状态管理
│   ├── clipboard/            # 粘贴板监控
│   │   └── src/lib.rs
│   ├── history/              # 历史记录管理
│   │   └── src/lib.rs
│   ├── hotkey/               # 全局快捷键
│   │   └── src/lib.rs
│   ├── injector/             # 粘贴到焦点窗口
│   │   └── src/lib.rs
│   ├── search/               # 搜索与正则
│   │   └── src/lib.rs
│   └── settings/             # 设置管理
│       └── src/lib.rs
└── assets/
    └── icons/
```

---

## 3. 主要功能需要的权限 (Permissions)

### 3.1 macOS 权限清单

| 权限 | 用途 | 配置方式 | 必须？ |
|------|------|----------|--------|------|
| **Accessibility** | CGEvent 模拟键盘输入 | `NSAppleEventsUsageDescription` + 用户授权 | 是 |
| **Screen Recording** | 可选，用于预览图片历史 | `NSScreenCaptureUsageDescription` | 否 |

### 3.2 文本注入到焦点窗口

要在其他应用的光标位置插入文本，需要使用 macOS 的 Accessibility API：

#### 方案 A: CGEvent 模拟键盘 (需要 Accessibility 权限)
```rust
// 使用 CoreGraphics 模拟 Cmd+V
// 必须有 Accessibility 权限
unsafe {
    let source = CGEventSource::new();
    let key_down = CGEvent::new_keyboard_event(source, KeyCode::V, true)?;
    key_down.set_flags(CGEventFlags::CGEventFlagCommand);
    key_down.post(tap: kCGHIDEventTap);
}
```

#### 方案 B: AppleScript (需要 Accessibility 权限)
```rust
// 使用 AppleScript 模拟 Cmd+V 粘贴
tell application "System Events"
    keystroke "v" using command down
end tell
```
**注意**：AppleScript 的 `keystroke` 同样需要 Accessibility 权限（与方案 A 相同）

**推荐实现**：使用 CGEvent 模拟（方案 A），成功率高，兼容性广。

### 3.3 权限申请流程

```rust
// 1. 检查是否有 Accessibility 权限
fn has_accessibility_permission() -> bool {
    // 使用 AXIsProcessTrusted()
}

// 2. 如果没有，发起权限申请
fn request_accessibility_permission() {
    // 打开系统偏好设置页面
    let options = AXIsProcessTrustedWithOptions(
        kAXTrustedCheckOptionPrompt.takeUnretainedValue() as CFDictionary
    );
}
```

---

## 4. 主要功能如何实现 (Implementation Details)

### 4.1 粘贴板监控

```rust
// 使用 arboard 监控粘贴板变化
use arboard::Clipboard;

pub struct ClipboardMonitor {
    clipboard: Clipboard,
    last_content: HashValue,
}

impl ClipboardMonitor {
    pub fn new() -> Result<Self> {
        Ok(Self {
            clipboard: Clipboard::new()?,
            last_content: HashValue::default(),
        })
    }

    pub fn poll(&mut self) -> Option<ClipboardContent> {
        // 检查粘贴板内容是否有变化
        // 返回新内容或 None
    }
}
```

**内容类型检测**：
```rust
pub enum ClipboardContent {
    PlainText(String),
    RichText { rtf: Vec<u8>, plain: String },
    Image { width: u32, height: u32, data: Vec<u8> },
    Html { html: String, plain: String },
    FilePaths(Vec<String>),
}
```

### 4.2 快捷键唤起浮窗

使用 `rdev` 或直接用 `cocoa` 注册全局热键：

```rust
use rdev::{listen, EventType, Key};

fn start_hotkey_listener(callback: impl Fn() + 'static) {
    listen(move |event| {
        // 默认 Cmd+` (backtick)
        if let EventType::KeyPress(Key::Backquote) = event.event_type {
            if event.modifiers.contains(rdev::Modifiers::META) {
                callback();
            }
        }
    }).unwrap();
}
```

### 4.3 浮窗 UI (GPUI)

```rust
use gpui::*;

pub struct PasteHistoryView {
    items: Vec<ClipboardItem>,      // 可滚动列表
    selected_index: usize,
    search_query: String,
    is_regex: bool,
}

impl Render for PasteHistoryView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .w(400)
            .max_h(500)                   // 最大高度，可滚动
            .bg(rgba(40, 40, 40, 0.98))
            .rounded_lg()
            .shadow_xl()
            .border_1(rgba(100, 100, 100, 0.3))
            .overflow_hidden()            // 隐藏溢出，启用滚动
            .children([
                // 搜索框
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .p_2()
                    .border_b_1(rgba(80, 80, 80, 0.5))
                    .child(
                        div()
                            .text_sm()
                            .text_gray_400()
                            .children([
                                "Search (",
                                span().text_amber_400().text_bold("/re"),
                                span().text_gray_500(),
                                " for regex): ",
                            ])
                    )
                    .child(
                        div()
                            .flex_1()
                            .child(self.search_query.clone())
                    ),
                // 历史列表（可滚动）
                div()
                    .flex()
                    .flex_col()
                    .overflow_y_auto()    // 垂直滚动
                    .children(self.items.iter().enumerate().map(|(i, item)| {
                        let shortcut = if i < 9 { Some(format!("⌘{}", i + 1)) } else { None };
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .p_2()
                            .when(i == self.selected_index, |div| {
                                div.bg(rgba(80, 120, 255, 0.25))
                            })
                            .hover(|div| div.bg(rgba(100, 100, 100, 0.2)))
                            .children([
                                // 编号标签（仅前9条显示）
                                div()
                                    .w(24)
                                    .h(24)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .bg(rgba(80, 80, 120, 0.6))
                                    .text_xs()
                                    .text_white()
                                    .text_bold()
                                    .when(shortcut.is_some(), |div| div.child(shortcut.unwrap())),
                                // 内容预览
                                div()
                                    .flex_1()
                                    .ml_3()
                                    .child(item.preview()),
                            ])
                    }))
            ])
    }
}
```

### 4.4 选择即粘贴 - 核心实现

#### 4.4.1 粘贴流程

```
1. 隐藏浮窗
2. 将内容放入系统粘贴板
3. 模拟 Cmd+V（使用 CGEvent，需要 Accessibility 权限）
4. 延迟恢复原始粘贴板
```

#### 4.4.2 权限需求

| 方案 | 权限需求 | 成功率 |
|------|----------|--------|
| **CGEvent 模拟** | Accessibility | 高 |

```rust
pub struct TextInjector {
    original_clipboard: Option<ClipboardContent>,
}

impl TextInjector {
    /// 将文本注入到焦点窗口
    pub fn inject_text(&mut self, text: &str) -> Result<()> {
        // 1. 保存当前粘贴板
        self.original_clipboard = Some(clipboard::get_content());

        // 2. 设置新内容到粘贴板
        clipboard::set_text(text);

        // 3. 模拟 Cmd+V（需要 Accessibility 权限）
        self.simulate_paste();

        Ok(())
    }

    fn simulate_paste(&self) {
        // 使用 CGEvent 模拟 Cmd+V
        // 需要 Accessibility 权限
        unsafe {
            let source = CGEventSource::new();
            let key_down = CGEvent::new_keyboard_event(source, KeyCode::V, true)?;
            let key_up = CGEvent::new_keyboard_event(source, KeyCode::V, false)?;

            key_down.set_flags(CGEventFlags::CGEventFlagKeyDown | CGEventFlags::CGEventFlagCommand);
            key_up.set_flags(CGEventFlags::CGEventFlagCommand);

            key_down.post(tap: kCGHIDEventTap);
            key_up.post(tap: kCGHIDEventTap);
        }
    }
}
```

#### 4.4.2 格式保留策略

| 目标格式 | 实现方式 |
|----------|----------|
| **保留富文本** | 将 RTF/HTML 数据直接放入粘贴板，不转成纯文本 |
| **仅纯文本** | 提取 `string()` 或 `plain_text()` 字段放入粘贴板 |
| **图片保留** | 将图片数据作为 `NSPasteboardType::PNG` 或 `TIFF` 放入 |

```rust
pub fn set_clipboard(content: &ClipboardContent) {
    let mut clipboard = Clipboard::new().unwrap();

    match content {
        ClipboardContent::PlainText(s) => {
            clipboard.set_text(s);
        }
        ClipboardContent::RichText { rtf, .. } => {
            // 使用 NSPasteboard 设置 RTF 数据
            set_rust_type(clipboard, rtf);
        }
        ClipboardContent::Image { width, height, data } => {
            // 设置为 PNG 或 TIFF
            set_image_type(clipboard, data, *width, *height);
        }
        _ => {}
    }
}
```

### 4.5 开机自动启动

```rust
use std::fs;

pub fn set_auto_launch(enabled: bool) -> std::io::Result<()> {
    let plist = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.flypaste.app</string>
    <key>ProgramArguments</key>
    <array>
        <string>/Applications/Flypaste.app/Contents/MacOS/flypaste</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>"#;

    let path = dirs::home_dir()
        .unwrap()
        .join("Library/LaunchAgents/com.flypaste.app.plist");

    if enabled {
        fs::write(&path, plist)?;
    } else if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}
```

### 4.6 数据量限制策略

不再按条目数限制，而是按**总数据量**限制：

```rust
pub struct HistoryConfig {
    pub max_total_bytes: u64,  // 10MiB = 10 * 1024 * 1024
}

impl HistoryDb {
    /// 添加新条目，自动清理旧数据以保持在容量限制内
    pub fn add_with_eviction(&self, item: ClipboardItem) -> Result<()> {
        let item_size = item.content.len() as u64;
        let mut current_size = self.get_total_size()?;
        let max_size = self.config.max_total_bytes;

        // 如果新条目本身就超过限制，直接丢弃
        if item_size > max_size {
            return Ok(());
        }

        // 清理旧数据直到有足够空间
        while current_size + item_size > max_size {
            let oldest = self.get_oldest()?;
            if let Some(old) = oldest {
                current_size -= old.content.len() as u64;
                self.delete(old.id)?;
            } else {
                break;  // 没有更多数据
            }
        }

        // 添加新条目
        self.add(&item)
    }
}
```

### 4.7 数据持久化

使用 SQLite 存储历史记录：

```rust
use rusqlite::*;

table! {
    #[derive(serde::Serialize, serde::Deserialize)]
    pub history_items (id) {
        id: i64 primary_key,
        content_type: String,
        content: Vec<u8>,       // 原始二进制内容
        preview: String,       // 纯文本预览（用于搜索）
        content_size: u64,     // 内容字节数
        created_at: i64,
    }
}

pub struct HistoryDb {
    db: Database,
}

impl HistoryDb {
    pub fn new() -> Result<Self> {
        let db = Database::open("flypaste.db")?;
        db.execute("CREATE TABLE IF NOT EXISTS history_items ...")?
        Ok(Self { db })
    }

    pub fn add(&self, item: &ClipboardItem) -> Result<()> {
        self.db.execute(
            "INSERT INTO history_items (content_type, content, preview, content_size, created_at) VALUES (?, ?, ?, ?, ?)",
            (item.content_type, &item.content, item.preview(), item.content.len() as u64, item.created_at),
        )?;
        Ok(())
    }

    pub fn get_total_size(&self) -> Result<u64> {
        let total: i64 = self.db.query(
            "SELECT COALESCE(SUM(content_size), 0) FROM history_items",
            (),
        )?;
        Ok(total as u64)
    }
}
```

### 4.8 搜索与正则表达式

```rust
use regex::Regex;

pub struct SearchEngine {
    normal_cache: HashMap<String, Vec<usize>>,      // 普通搜索缓存
    regex_cache: HashMap<String, Result<Regex>>,    // 正则编译缓存
}

impl SearchEngine {
    /// 搜索历史记录
    /// - 以 `/` 开头表示正则表达式搜索
    /// - 否则按普通字符串搜索
    pub fn search(&self, query: &str, items: &[ClipboardItem]) -> Vec<SearchResult> {
        if query.starts_with('/') {
            let pattern = &query[1..];
            self.regex_search(pattern, items)
        } else {
            self.fuzzy_search(query, items)
        }
    }

    /// 正则表达式搜索
    fn regex_search(&self, pattern: &str, items: &[ClipboardItem]) -> Vec<SearchResult> {
        let regex = match self.regex_cache.entry(pattern.to_string()) {
            Entry::Occupied(e) => e.get().clone(),
            Entry::Vacant(e) => {
                let r = Regex::new(pattern).ok();
                e.insert(r);
                return r.map(|r| self.filter_by_regex(&r, items)).unwrap_or_default();
            }
        };

        regex.map(|r| self.filter_by_regex(&r, items)).unwrap_or_default()
    }

    fn filter_by_regex(&self, regex: &Regex, items: &[ClipboardItem]) -> Vec<SearchResult> {
        items
            .iter()
            .enumerate()
            .filter(|(_, item)| regex.is_match(&item.preview))
            .map(|(idx, item)| SearchResult {
                index: idx,
                item: item.clone(),
                matched_range: regex.find(&item.preview).map(|m| (m.start(), m.end())),
            })
            .collect()
    }

    /// 模糊搜索（包含匹配）
    fn fuzzy_search(&self, query: &str, items: &[ClipboardItem]) -> Vec<SearchResult> {
        let query_lower = query.to_lowercase();
        items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.preview.to_lowercase().contains(&query_lower))
            .map(|(idx, item)| SearchResult {
                index: idx,
                item: item.clone(),
                matched_range: item.preview.to_lowercase().find(&query_lower).map(|i| (i, i + query.len())),
            })
            .collect()
    }
}
```

---

## 5. 实现计划 (初步)

### Phase 1: 核心框架
- [ ] 项目初始化，集成 gpui
- [ ] 粘贴板监控 (arboard)
- [ ] 简单浮窗显示

### Phase 2: 快捷键与交互
- [ ] 全局快捷键注册 (Cmd+`)
- [ ] 鼠标点击选择
- [ ] Cmd+1~9 快速选择
- [ ] 浮窗动画

### Phase 3: 粘贴功能
- [ ] CGEvent 模拟输入
- [ ] Accessibility 权限处理
- [ ] 格式保留

### Phase 4: 搜索功能
- [ ] 搜索框 UI
- [ ] 普通字符串搜索
- [ ] 正则表达式搜索（以 `/` 开头）

### Phase 5: 设置与持久化
- [ ] 设置界面
- [ ] SQLite 存储（按数据量限制）
- [ ] 开机启动

### Phase 6: 优化
- [ ] 性能优化
- [ ] 隐私处理（敏感内容过滤）
- [ ] 图片预览
- [ ] 菜单栏图标

---

## 6. 参考资料

- [Zed GitHub](https://github.com/zed-industries/zed)
- [GPUI Framework](https://zed.dev/blog/gpui)
- [macOS Accessibility API](https://developer.apple.com/documentation/applicationcomponents/accessibility)
- [arboard crate](https://crates.io/crates/arboard)
- [rdev crate (global hotkeys)](https://crates.io/crates/rdev)
