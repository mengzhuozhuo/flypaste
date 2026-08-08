use fly_settings::Hotkey as SettingsHotkey;
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowHotkeyAction {
    Close,
    PasteIndex(u8),
    TogglePin,
    ToggleRegex,
    ToggleCaseSensitive,
}

enum DispatchTarget {
    Toggle,
    Window(WindowHotkeyAction),
}

impl Copy for DispatchTarget {}
impl Clone for DispatchTarget {
    fn clone(&self) -> Self {
        *self
    }
}

pub struct HotkeyManager {
    manager: GlobalHotKeyManager,
    toggle_hotkey: Mutex<Option<HotKey>>,
    window_hotkeys: Mutex<Vec<HotKey>>,
    dispatch_map: Arc<Mutex<HashMap<u32, DispatchTarget>>>,
    toggle_callback: Arc<dyn Fn() + Send + Sync>,
    window_callback: Arc<dyn Fn(WindowHotkeyAction) + Send + Sync>,
}

impl HotkeyManager {
    pub fn new<F, W>(toggle_callback: F, window_callback: W) -> Self
    where
        F: Fn() + Send + Sync + 'static,
        W: Fn(WindowHotkeyAction) + Send + Sync + 'static,
    {
        Self {
            manager: GlobalHotKeyManager::new().expect("Failed to create GlobalHotKeyManager"),
            toggle_hotkey: Mutex::new(None),
            window_hotkeys: Mutex::new(Vec::new()),
            dispatch_map: Arc::new(Mutex::new(HashMap::new())),
            toggle_callback: Arc::new(toggle_callback),
            window_callback: Arc::new(window_callback),
        }
    }

    pub fn settings_to_hotkey(s: &SettingsHotkey) -> Option<HotKey> {
        let mut modifiers = Modifiers::empty();
        for m in &s.modifiers {
            match m.to_lowercase().as_str() {
                "cmd" | "command" => modifiers |= Modifiers::SUPER,
                "alt" | "option" => modifiers |= Modifiers::ALT,
                "shift" => modifiers |= Modifiers::SHIFT,
                "ctrl" | "control" => modifiers |= Modifiers::CONTROL,
                _ => {}
            }
        }

        let code = key_name_to_code(&s.key)?;
        Some(HotKey::new(Some(modifiers), code))
    }

    fn register_mapped(&self, hotkey: HotKey, target: DispatchTarget) {
        if let Err(e) = self.manager.register(hotkey) {
            log::error!("Failed to register hotkey {:?}: {:?}", hotkey, e);
            return;
        }
        self.dispatch_map.lock().unwrap().insert(hotkey.id(), target);
    }

    fn unregister_hotkey(&self, hotkey: HotKey) {
        let _ = self.manager.unregister(hotkey);
        self.dispatch_map.lock().unwrap().remove(&hotkey.id());
    }

    pub fn start(&self, initial_settings: &SettingsHotkey) {
        self.update_hotkey(initial_settings);

        std::thread::spawn({
            let dispatch_map = self.dispatch_map.clone();
            let toggle_callback = self.toggle_callback.clone();
            let window_callback = self.window_callback.clone();
            move || {
                let receiver = GlobalHotKeyEvent::receiver();
                loop {
                    match receiver.recv() {
                        Ok(event) => {
                            if event.state != global_hotkey::HotKeyState::Pressed {
                                continue;
                            }
                            let target = dispatch_map.lock().unwrap().get(&event.id).copied();
                            match target {
                                Some(DispatchTarget::Toggle) => toggle_callback(),
                                Some(DispatchTarget::Window(action)) => window_callback(action),
                                None => {}
                            }
                        }
                        Err(e) => {
                            log::error!("Hotkey receiver error: {:?}", e);
                            std::thread::sleep(std::time::Duration::from_millis(100));
                        }
                    }
                }
            }
        });
    }

    pub fn update_hotkey(&self, settings: &SettingsHotkey) {
        if let Some(old_hotkey) = self.toggle_hotkey.lock().unwrap().take() {
            self.unregister_hotkey(old_hotkey);
        }

        if let Some(new_hotkey) = Self::settings_to_hotkey(settings) {
            log::info!("Registered global hotkey: {}", settings.display());
            self.register_mapped(new_hotkey, DispatchTarget::Toggle);
            *self.toggle_hotkey.lock().unwrap() = Some(new_hotkey);
        }
    }

    pub fn register_window_hotkeys(&self, settings: &fly_settings::Settings) {
        self.unregister_window_hotkeys();

        let mut hotkeys = Vec::new();

        let escape = HotKey::new(None, Code::Escape);
        hotkeys.push((escape, WindowHotkeyAction::Close));

        for (i, code) in [
            Code::Digit1,
            Code::Digit2,
            Code::Digit3,
            Code::Digit4,
            Code::Digit5,
            Code::Digit6,
            Code::Digit7,
            Code::Digit8,
            Code::Digit9,
        ]
        .into_iter()
        .enumerate()
        {
            let hotkey = HotKey::new(Some(Modifiers::ALT), code);
            hotkeys.push((hotkey, WindowHotkeyAction::PasteIndex(i as u8)));
        }

        let pin = HotKey::new(Some(Modifiers::SUPER | Modifiers::ALT), Code::KeyP);
        hotkeys.push((pin, WindowHotkeyAction::TogglePin));

        if let Some(hotkey) = Self::settings_to_hotkey(&settings.regex_hotkey) {
            hotkeys.push((hotkey, WindowHotkeyAction::ToggleRegex));
        }
        if let Some(hotkey) = Self::settings_to_hotkey(&settings.case_sensitive_hotkey) {
            hotkeys.push((hotkey, WindowHotkeyAction::ToggleCaseSensitive));
        }

        let mut registered = Vec::new();
        for (hotkey, action) in hotkeys {
            self.register_mapped(hotkey, DispatchTarget::Window(action));
            registered.push(hotkey);
        }
        *self.window_hotkeys.lock().unwrap() = registered;
        log::debug!(
            "Registered {} window-scoped hotkeys",
            self.window_hotkeys.lock().unwrap().len()
        );
    }

    pub fn unregister_window_hotkeys(&self) {
        let hotkeys: Vec<HotKey> = self.window_hotkeys.lock().unwrap().drain(..).collect();
        if hotkeys.is_empty() {
            return;
        }
        for hotkey in hotkeys {
            self.unregister_hotkey(hotkey);
        }
        log::debug!("Unregistered window-scoped hotkeys");
    }
}

fn key_name_to_code(key: &str) -> Option<Code> {
    match key.to_lowercase().as_str() {
        "backquote" | "`" => Some(Code::Backquote),
        "comma" | "," => Some(Code::Comma),
        "period" | "." => Some(Code::Period),
        "slash" | "/" => Some(Code::Slash),
        "backslash" | "\\" => Some(Code::Backslash),
        "minus" | "-" => Some(Code::Minus),
        "equal" | "=" => Some(Code::Equal),
        "space" => Some(Code::Space),
        "enter" | "return" => Some(Code::Enter),
        "tab" => Some(Code::Tab),
        "escape" | "esc" => Some(Code::Escape),
        "a" => Some(Code::KeyA),
        "b" => Some(Code::KeyB),
        "c" => Some(Code::KeyC),
        "d" => Some(Code::KeyD),
        "e" => Some(Code::KeyE),
        "f" => Some(Code::KeyF),
        "g" => Some(Code::KeyG),
        "h" => Some(Code::KeyH),
        "i" => Some(Code::KeyI),
        "j" => Some(Code::KeyJ),
        "k" => Some(Code::KeyK),
        "l" => Some(Code::KeyL),
        "m" => Some(Code::KeyM),
        "n" => Some(Code::KeyN),
        "o" => Some(Code::KeyO),
        "p" => Some(Code::KeyP),
        "q" => Some(Code::KeyQ),
        "r" => Some(Code::KeyR),
        "s" => Some(Code::KeyS),
        "t" => Some(Code::KeyT),
        "u" => Some(Code::KeyU),
        "v" => Some(Code::KeyV),
        "w" => Some(Code::KeyW),
        "x" => Some(Code::KeyX),
        "y" => Some(Code::KeyY),
        "z" => Some(Code::KeyZ),
        "1" => Some(Code::Digit1),
        "2" => Some(Code::Digit2),
        "3" => Some(Code::Digit3),
        "4" => Some(Code::Digit4),
        "5" => Some(Code::Digit5),
        "6" => Some(Code::Digit6),
        "7" => Some(Code::Digit7),
        "8" => Some(Code::Digit8),
        "9" => Some(Code::Digit9),
        "0" => Some(Code::Digit0),
        _ => {
            log::warn!("Unknown key name: {}", key);
            None
        }
    }
}
