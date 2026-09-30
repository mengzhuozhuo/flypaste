use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// 应用设置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// 全局快捷键
    pub hotkey: Hotkey,
    /// 正则匹配快捷键
    pub regex_hotkey: Hotkey,
    /// 大小写敏感快捷键
    pub case_sensitive_hotkey: Hotkey,
    /// 聚焦搜索快捷键
    #[serde(default = "default_focus_hotkey")]
    pub focus_hotkey: Hotkey,
    /// 窗口固定快捷键
    #[serde(default = "default_pin_hotkey")]
    pub pin_hotkey: Hotkey,
    /// 数字选择项目的修饰键（配合 1-9 使用）
    #[serde(default = "default_item_select_modifiers")]
    pub item_select_modifiers: Vec<String>,
    /// 最大历史数据量（字节）
    pub max_total_bytes: u64,
    /// 文本保留天数
    #[serde(default = "default_text_retention_days")]
    pub text_retention_days: u32,
    /// 图片保留天数
    #[serde(default = "default_image_retention_days")]
    pub image_retention_days: u32,
    /// 正则搜索开关（记住上次状态）
    #[serde(alias = "default_regex")]
    pub regex_enabled: bool,
    /// 大小写敏感开关（记住上次状态）
    #[serde(alias = "default_case_sensitive")]
    pub case_sensitive_enabled: bool,
    /// 格式保留模式
    pub format_mode: FormatMode,
    /// 开机自动启动
    pub auto_launch: bool,
    /// 界面语言
    #[serde(default = "default_language")]
    pub language: Language,
    /// 浏览模式面板不透明度（内部存储，0 = 全透明，1 = 不透明；设置界面以「透明度」展示）
    #[serde(default = "default_browse_panel_opacity")]
    pub browse_panel_opacity: f32,
    /// 搜索模式面板不透明度（内部存储；搜索模式应比浏览模式更不透明，即透明度更低）
    #[serde(default = "default_search_panel_opacity")]
    pub search_panel_opacity: f32,
    /// 主题
    #[serde(default)]
    pub theme: Theme,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    En,
    Zh,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Auto,
    Light,
    Dark,
}

impl Theme {
    pub fn all() -> [Theme; 3] {
        [Theme::Auto, Theme::Light, Theme::Dark]
    }
}

impl Language {
    pub fn all() -> [Language; 2] {
        [Language::Zh, Language::En]
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Language::Zh => "中文",
            Language::En => "English",
        }
    }
}

fn default_language() -> Language {
    std::env::var("LANG")
        .map(|locale| {
            if locale.starts_with("zh") {
                Language::Zh
            } else {
                Language::En
            }
        })
        .unwrap_or(Language::En)
}

fn default_text_retention_days() -> u32 {
    30
}

fn default_image_retention_days() -> u32 {
    7
}

fn default_browse_panel_opacity() -> f32 {
    0.53
}

fn default_search_panel_opacity() -> f32 {
    0.70
}

fn default_focus_hotkey() -> Hotkey {
    Hotkey {
        modifiers: vec!["cmd".to_string(), "alt".to_string()],
        key: "f".to_string(),
    }
}

fn default_pin_hotkey() -> Hotkey {
    Hotkey {
        modifiers: vec!["cmd".to_string(), "alt".to_string()],
        key: "p".to_string(),
    }
}

fn default_item_select_modifiers() -> Vec<String> {
    vec!["alt".to_string()]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Hotkey {
    pub modifiers: Vec<String>,
    pub key: String,
}

impl Hotkey {
    pub fn display(&self) -> String {
        let mut parts = Vec::new();
        for m in &self.modifiers {
            match m.to_lowercase().as_str() {
                "cmd" | "command" => parts.push("⌘"),
                "alt" | "option" => parts.push("⌥"),
                "shift" => parts.push("⇧"),
                "ctrl" | "control" => parts.push("⌃"),
                _ => parts.push(m),
            }
        }
        
        let key_display = match self.key.to_lowercase().as_str() {
            "backquote" => "`",
            "comma" => ",",
            "period" => ".",
            "slash" => "/",
            "backslash" => "\\",
            "minus" => "-",
            "equal" => "=",
            _ => &self.key,
        };
        
        parts.push(key_display);
        parts.join("")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FormatMode {
    Preserve,  // 保留原始格式
    PlainText, // 仅保留纯文本
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: Hotkey {
                modifiers: vec!["alt".to_string()],
                key: "BackQuote".to_string(),
            },
            regex_hotkey: Hotkey {
                modifiers: vec!["cmd".to_string(), "alt".to_string()],
                key: "r".to_string(),
            },
            case_sensitive_hotkey: Hotkey {
                modifiers: vec!["cmd".to_string(), "alt".to_string()],
                key: "c".to_string(),
            },
            focus_hotkey: default_focus_hotkey(),
            pin_hotkey: default_pin_hotkey(),
            item_select_modifiers: default_item_select_modifiers(),
            max_total_bytes: 100 * 1024 * 1024, // 100MiB
            text_retention_days: default_text_retention_days(),
            image_retention_days: default_image_retention_days(),
            regex_enabled: false,
            case_sensitive_enabled: false,
            format_mode: FormatMode::Preserve,
            auto_launch: false,
            language: default_language(),
            browse_panel_opacity: default_browse_panel_opacity(),
            search_panel_opacity: default_search_panel_opacity(),
            theme: Theme::Auto,
        }
    }
}

impl Settings {
    /// 获取设置文件路径
    fn path() -> PathBuf {
        let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("flypaste");
        fs::create_dir_all(&path).ok();
        path.push("settings.json");
        path
    }

    /// 加载设置
    pub fn load() -> std::io::Result<Self> {
        let path = Self::path();
        if !path.exists() {
            return Ok(Settings::default());
        }

        let content = fs::read_to_string(path)?;
        let mut settings: Settings = serde_json::from_str(&content).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e)
        })?;
        settings.sanitize_retention();
        Ok(settings)
    }

    fn sanitize_retention(&mut self) {
        const MIN_RETENTION_DAYS: u32 = 1;
        const MAX_TEXT_RETENTION_DAYS: u32 = 365;
        const MAX_IMAGE_RETENTION_DAYS: u32 = 90;

        self.text_retention_days = self
            .text_retention_days
            .clamp(MIN_RETENTION_DAYS, MAX_TEXT_RETENTION_DAYS);
        self.image_retention_days = self
            .image_retention_days
            .clamp(MIN_RETENTION_DAYS, MAX_IMAGE_RETENTION_DAYS);
        self.clamp_panel_opacities();
    }

    /// Repair invalid browse/search pairs after loading from disk only.
    pub fn clamp_panel_opacities(&mut self) {
        const GAP: f32 = 0.01;

        self.browse_panel_opacity = self.browse_panel_opacity.clamp(0.0, 1.0);
        self.search_panel_opacity = self.search_panel_opacity.clamp(0.0, 1.0);

        // search_transparency < browse_transparency  ⟺  search_opacity > browse_opacity
        if self.search_panel_opacity <= self.browse_panel_opacity {
            self.search_panel_opacity =
                (self.browse_panel_opacity + GAP).min(1.0);
        }
    }

    /// Set browse transparency (0 = opaque, 1 = fully transparent).
    /// If browse drops below search transparency, search is pulled down too.
    pub fn set_browse_transparency(&mut self, transparency: f32) {
        const GAP: f32 = 0.01;
        self.browse_panel_opacity = 1.0 - transparency.clamp(0.0, 1.0);
        if self.search_panel_opacity <= self.browse_panel_opacity {
            self.search_panel_opacity = (self.browse_panel_opacity + GAP).min(1.0);
        }
    }

    /// Set search transparency (0 = opaque, 1 = fully transparent).
    /// If search rises above browse transparency, browse is pushed up too.
    pub fn set_search_transparency(&mut self, transparency: f32) {
        const GAP: f32 = 0.01;
        self.search_panel_opacity = 1.0 - transparency.clamp(0.0, 1.0);
        // Violation: search_transparency >= browse_transparency ⟺ search_opacity <= browse_opacity
        if self.search_panel_opacity <= self.browse_panel_opacity {
            self.browse_panel_opacity = (self.search_panel_opacity - GAP).max(0.0);
        }
    }

    pub fn set_browse_panel_opacity(&mut self, value: f32) {
        self.browse_panel_opacity = value.clamp(0.0, 1.0);
    }

    pub fn set_search_panel_opacity(&mut self, value: f32) {
        self.search_panel_opacity = value.clamp(0.0, 1.0);
    }

    /// 获取数字选择快捷键的修饰键显示文本（例如 "⌥", "⌘", "⌃"）
    pub fn item_select_modifier_display(&self) -> String {
        let mut parts = Vec::new();
        for m in &self.item_select_modifiers {
            match m.to_lowercase().as_str() {
                "cmd" | "command" => parts.push("⌘"),
                "alt" | "option" => parts.push("⌥"),
                "shift" => parts.push("⇧"),
                "ctrl" | "control" => parts.push("⌃"),
                _ => parts.push(m.as_str()),
            }
        }
        if parts.is_empty() {
            "⌥".to_string()
        } else {
            parts.join("")
        }
    }

    /// 获取主修饰键规范名称 ("alt", "cmd", "ctrl")
    pub fn item_select_modifier_name(&self) -> &str {
        for m in &self.item_select_modifiers {
            match m.to_lowercase().as_str() {
                "cmd" | "command" => return "cmd",
                "ctrl" | "control" => return "ctrl",
                "alt" | "option" => return "alt",
                _ => {}
            }
        }
        "alt"
    }

    /// 保存设置
    pub fn save(&self) -> std::io::Result<()> {
        let mut settings = self.clone();
        settings.sanitize_retention();
        let path = Settings::path();
        let content = serde_json::to_string_pretty(&settings)?;
        fs::write(path, content)
    }
}

/// Manager for loading and accessing settings
#[derive(Clone)]
pub struct Manager {
    settings: Settings,
}

impl Manager {
    pub fn load() -> Self {
        Self {
            settings: Settings::load().unwrap_or_default(),
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_pin_hotkey() {
        let settings = Settings::default();
        assert_eq!(settings.pin_hotkey.key, "p");
        assert_eq!(settings.pin_hotkey.modifiers, vec!["cmd", "alt"]);
        assert_eq!(settings.pin_hotkey.display(), "⌘⌥p");
    }

    #[test]
    fn test_deserialize_without_pin_hotkey() {
        // Serialize default settings, remove pin_hotkey, and verify deserialization succeeds with default pin_hotkey
        let mut val: serde_json::Value = serde_json::to_value(Settings::default()).unwrap();
        val.as_object_mut().unwrap().remove("pin_hotkey");
        let settings: Settings = serde_json::from_value(val).expect("should deserialize with default pin_hotkey");
        assert_eq!(settings.pin_hotkey.key, "p");
        assert_eq!(settings.pin_hotkey.modifiers, vec!["cmd", "alt"]);
    }

    #[test]
    fn test_deserialize_without_item_select_modifiers() {
        let mut val: serde_json::Value = serde_json::to_value(Settings::default()).unwrap();
        val.as_object_mut().unwrap().remove("item_select_modifiers");
        let settings: Settings = serde_json::from_value(val).expect("should deserialize with default item_select_modifiers");
        assert_eq!(settings.item_select_modifiers, vec!["alt"]);
        assert_eq!(settings.item_select_modifier_display(), "⌥");
        assert_eq!(settings.item_select_modifier_name(), "alt");
    }

    #[test]
    fn test_item_select_modifier_helpers() {
        let mut settings = Settings::default();
        settings.item_select_modifiers = vec!["cmd".to_string()];
        assert_eq!(settings.item_select_modifier_display(), "⌘");
        assert_eq!(settings.item_select_modifier_name(), "cmd");

        settings.item_select_modifiers = vec!["ctrl".to_string()];
        assert_eq!(settings.item_select_modifier_display(), "⌃");
        assert_eq!(settings.item_select_modifier_name(), "ctrl");
    }
}
