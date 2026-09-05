#![allow(unexpected_cfgs)]

pub mod number_field;
pub mod opacity_slider;
pub mod paste_window;
pub mod settings_window;
pub mod markdown_renderer;
pub mod text_input;
pub mod animated_gif;

use gpui::*;
use std::sync::Arc;
use tokio::sync::mpsc;

use paste_window::PasteWindow;
use settings_window::SettingsWindow;

actions!(flypaste, [
    Quit,
    Backspace,
    Delete,
    Left,
    Right,
    SelectAll,
    DeleteToBeginningOfLine,
    MoveToBeginningOfLine,
    MoveToEndOfLine,
    ToggleRegex,
    ToggleSettings
]);

/// Statics for window management
pub static WINDOW_IS_OPEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static WINDOW_HANDLE: std::sync::Mutex<Option<gpui::WindowHandle<PasteWindow>>> = std::sync::Mutex::new(None);
pub static SETTINGS_WINDOW_HANDLE: std::sync::Mutex<Option<gpui::WindowHandle<SettingsWindow>>> = std::sync::Mutex::new(None);
pub static SETTINGS_IS_OPEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static PREVIEW_WINDOW_HANDLE: std::sync::Mutex<Option<gpui::WindowHandle<paste_window::PreviewWindow>>> = std::sync::Mutex::new(None);
pub static PREVIEW_IS_OPEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static RESTORE_SEARCH_FOCUS_AFTER_PREVIEW: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
pub static PINNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Whether the paste panel is in focus (search) mode — updated synchronously on focus events.
pub(crate) static IN_FOCUS_MODE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
/// Stores the last window frame (x, y, width, height, display_id) for pinned mode reopening
pub static PINNED_WINDOW_FRAME: std::sync::Mutex<Option<(f64, f64, f64, f64, Option<u32>)>> =
    std::sync::Mutex::new(None);
pub static CLIPBOARD_UPDATE_FLAG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Live panel opacity values (synced from settings on load/save).
static PANEL_OPACITY: std::sync::Mutex<(f32, f32)> =
    std::sync::Mutex::new((0.53, 0.70));

pub fn sync_panel_opacity_from_settings(settings: &fly_settings::Settings) {
    *PANEL_OPACITY.lock().unwrap() =
        (settings.browse_panel_opacity, settings.search_panel_opacity);
}

pub fn panel_opacity() -> (f32, f32) {
    *PANEL_OPACITY.lock().unwrap()
}

static LAST_APPLIED_THEME: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(255);

pub fn apply_theme(window_appearance: gpui::WindowAppearance, cx: &mut App) {
    let settings = fly_settings::Settings::load().unwrap_or_default();
    
    let theme_val = match settings.theme {
        fly_settings::Theme::Auto => 0,
        fly_settings::Theme::Light => 1,
        fly_settings::Theme::Dark => 2,
    };

    let theme_changed = LAST_APPLIED_THEME.swap(theme_val, std::sync::atomic::Ordering::SeqCst) != theme_val;

    #[cfg(target_os = "macos")]
    if theme_changed {
        let appearance_name: Option<&'static str> = match settings.theme {
            fly_settings::Theme::Auto => None,
            fly_settings::Theme::Light => Some("NSAppearanceNameAqua\0"),
            fly_settings::Theme::Dark => Some("NSAppearanceNameDarkAqua\0"),
        };
        
        cx.foreground_executor().spawn(async move {
            unsafe {
                use objc::{msg_send, sel, sel_impl, runtime::Class};
                if let Some(cls_app) = Class::get("NSApplication") {
                    let app: *mut objc::runtime::Object = msg_send![cls_app, sharedApplication];
                    let cls_appearance = Class::get("NSAppearance").unwrap();
                    let cls_str = Class::get("NSString").unwrap();
                    
                    if let Some(name) = appearance_name {
                        let ns_str: *mut objc::runtime::Object = msg_send![cls_str, stringWithUTF8String: name.as_ptr()];
                        let appearance: *mut objc::runtime::Object = msg_send![cls_appearance, appearanceNamed: ns_str];
                        let _: () = msg_send![app, setAppearance: appearance];
                    } else {
                        let _: () = msg_send![app, setAppearance: std::ptr::null_mut::<objc::runtime::Object>()];
                    }
                }
            }
        }).detach();
    }

    let target_appearance = match settings.theme {
        fly_settings::Theme::Auto => window_appearance.into(),
        fly_settings::Theme::Light => gpui::WindowAppearance::Light.into(),
        fly_settings::Theme::Dark => gpui::WindowAppearance::Dark.into(),
    };
    *theme::SystemAppearance::global_mut(cx) = theme::SystemAppearance(target_appearance);
    theme_settings::reload_theme(cx);
    theme_settings::reload_icon_theme(cx);
}

/// Notify the paste window to re-render (e.g. after settings change).
pub fn refresh_paste_window(cx: &mut App) {
    let _ = update_paste_window(cx, |_, _, cx| cx.notify());
}

pub(crate) fn on_paste_window_released() {
    WINDOW_HANDLE.lock().unwrap().take();
    on_preview_window_released();
    reset_window_flag();
}

pub(crate) fn on_settings_window_released() {
    SETTINGS_WINDOW_HANDLE.lock().unwrap().take();
    SETTINGS_IS_OPEN.store(false, std::sync::atomic::Ordering::SeqCst);
    if !WINDOW_IS_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
        crate::ui::hide_flypaste_app();
    }
}

pub(crate) fn on_preview_window_released() {
    PREVIEW_WINDOW_HANDLE.lock().unwrap().take();
    PREVIEW_IS_OPEN.store(false, std::sync::atomic::Ordering::SeqCst);
}

fn update_paste_window<C, R>(
    cx: &mut C,
    f: impl FnOnce(&mut PasteWindow, &mut Window, &mut Context<PasteWindow>) -> R,
) -> Option<R>
where
    C: AppContext,
{
    if !WINDOW_IS_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
        return None;
    }
    let handle = WINDOW_HANDLE.lock().unwrap().clone()?;
    match handle.update(cx, f) {
        Ok(result) => Some(result),
        Err(_) => {
            on_paste_window_released();
            None
        }
    }
}

fn close_paste_window(cx: &mut AsyncApp) {
    let Some(handle) = WINDOW_HANDLE.lock().unwrap().take() else {
        reset_window_flag();
        return;
    };

    if !SETTINGS_IS_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
        crate::ui::hide_flypaste_app();
    }

    if handle
        .update(cx, |_, window, _| window.remove_window())
        .is_err()
    {
        on_paste_window_released();
    }
}

/// Get the current mouse cursor position on screen
fn get_cursor_position() -> (f32, f32) {
    use core_graphics::event::CGEvent;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState);
    match source {
        Ok(source) => {
            let event = CGEvent::new(source);
            match event {
                Ok(event) => {
                    let location = event.location();
                    (location.x as f32, location.y as f32)
                }
                Err(_) => (0.0, 0.0),
            }
        }
        Err(_) => (0.0, 0.0),
    }
}

/// Get the bundle ID of the frontmost application (safe to call from any thread)
/// Returns None if the frontmost app is Flypaste
#[cfg(target_os = "macos")]
fn get_front_app_bundle_id_internal() -> Option<String> {
    use objc::{msg_send, sel, sel_impl, runtime::Class};

    unsafe {
        let cls = Class::get("NSWorkspace")?;
        let workspace: *mut objc::runtime::Object = msg_send![cls, sharedWorkspace];
        let front_app: *mut objc::runtime::Object = msg_send![workspace, frontmostApplication];
        if front_app.is_null() {
            return None;
        }

        // Check if front app is Flypaste using PID
        let pid: i32 = msg_send![front_app, processIdentifier];
        if pid == libc::getpid() {
            return None;
        }

        let bundle_id: *mut objc::runtime::Object = msg_send![front_app, bundleIdentifier];
        if bundle_id.is_null() {
            return None;
        }
        let c_str: *const std::ffi::c_char = msg_send![bundle_id, UTF8String];
        if c_str.is_null() {
            return None;
        }
        Some(std::ffi::CStr::from_ptr(c_str).to_string_lossy().to_string())
    }
}

#[cfg(not(target_os = "macos"))]
fn get_front_app_bundle_id_internal() -> Option<String> {
    None
}

/// Get the frontmost application PID (ignoring flypaste itself)
/// Static cache for application icons (Bundle ID -> PNG Bytes)
static MONITOR_ICON_CACHE: std::sync::Mutex<Option<std::collections::HashMap<String, Arc<Vec<u8>>>>> = std::sync::Mutex::new(None);
static MONITOR_ICON_FETCHING_STATE: std::sync::Mutex<Option<std::collections::HashSet<String>>> = std::sync::Mutex::new(None);

/// Internal helper to fetch app icon using NSWorkspace (macOS only)
/// MUST be called from the main thread
#[cfg(target_os = "macos")]
fn fetch_app_icon_internal(bundle_id: &str) -> Option<Vec<u8>> {
    use objc::{msg_send, sel, sel_impl, runtime::Class};

    unsafe {
        let cls_workspace = Class::get("NSWorkspace")?;
        let workspace: *mut objc::runtime::Object = msg_send![cls_workspace, sharedWorkspace];

        let cls_str = Class::get("NSString")?;
        let c_bundle_id = std::ffi::CString::new(bundle_id).ok()?;
        let bid_ns: *mut objc::runtime::Object = msg_send![cls_str, stringWithUTF8String: c_bundle_id.as_ptr()];

        // Get path to application bundle
        let path: *mut objc::runtime::Object = msg_send![workspace, absolutePathForAppBundleWithIdentifier: bid_ns];
        if path.is_null() {
            log::error!("fetch_app_icon_internal: path is null for bundle_id {}", bundle_id);
            return None;
        }

        // Get icon for the file at that path
        let icon: *mut objc::runtime::Object = msg_send![workspace, iconForFile: path];
        if icon.is_null() {
            log::error!("fetch_app_icon_internal: icon is null for bundle_id {}", bundle_id);
            return None;
        }

        // Get TIFF representation (raw icon data)
        let tiff_data: *mut objc::runtime::Object = msg_send![icon, TIFFRepresentation];
        if tiff_data.is_null() {
            log::error!("fetch_app_icon_internal: tiff_data is null for bundle_id {}", bundle_id);
            return None;
        }

        // Use macOS native API to convert TIFF to PNG (avoids 'image' crate panicking on 16-bit float TIFFs)
        let cls_bitmap = Class::get("NSBitmapImageRep")?;
        let bitmap_rep: *mut objc::runtime::Object = msg_send![cls_bitmap, imageRepWithData: tiff_data];
        if bitmap_rep.is_null() {
            log::error!("fetch_app_icon_internal: bitmap_rep is null for bundle_id {}", bundle_id);
            return None;
        }

        let cls_dict = Class::get("NSDictionary")?;
        let empty_dict: *mut objc::runtime::Object = msg_send![cls_dict, dictionary];

        // 4 is NSPNGFileType
        let png_data: *mut objc::runtime::Object = msg_send![bitmap_rep, representationUsingType: 4_usize properties: empty_dict];
        if png_data.is_null() {
            log::error!("fetch_app_icon_internal: png_data is null for bundle_id {}", bundle_id);
            return None;
        }

        let png_bytes: *const u8 = msg_send![png_data, bytes];
        let png_length: usize = msg_send![png_data, length];

        if png_bytes.is_null() || png_length == 0 {
            log::error!("fetch_app_icon_internal: png_bytes is null or length is 0 for bundle_id {}", bundle_id);
            return None;
        }

        let png_slice = std::slice::from_raw_parts(png_bytes, png_length);

        // Load PNG and resize to 48x48 for consistent icon size and sharpness
        let img = match image::load_from_memory(png_slice) {
            Ok(img) => img,
            Err(e) => {
                log::error!("fetch_app_icon_internal: image::load_from_memory failed for bundle_id {}: {:?}", bundle_id, e);
                return None;
            }
        };
        let img = img.resize_exact(48, 48, image::imageops::FilterType::Triangle);

        let mut final_png_bytes = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut final_png_bytes);
        if let Err(e) = img.write_to(&mut cursor, image::ImageFormat::Png) {
            log::error!("fetch_app_icon_internal: write_to png failed for bundle_id {}: {:?}", bundle_id, e);
            return None;
        }

        Some(final_png_bytes)
    }
}

#[cfg(not(target_os = "macos"))]
fn fetch_app_icon_internal(_bundle_id: &str) -> Option<Vec<u8>> {
    None

}

/// Public getter for cached icons with async loading
pub fn load_icon_async(bundle_id: &str, cx: &mut App) -> Option<Arc<Vec<u8>>> {
    // 1. Try to get from cache first
    {
        let cache = MONITOR_ICON_CACHE.lock().unwrap();
        if let Some(ref map) = *cache {
            if let Some(icon) = map.get(bundle_id) {
                return Some(Arc::clone(icon));
            }
        }
    }

    // 2. Not in cache. Mark as fetching to avoid duplicate background tasks
    {
        let mut fetching = MONITOR_ICON_FETCHING_STATE.lock().unwrap();
        if fetching.is_none() {
            *fetching = Some(std::collections::HashSet::new());
        }
        if let Some(ref mut set) = *fetching {
            if set.contains(bundle_id) {
                return None; // Already fetching
            }
            set.insert(bundle_id.to_string());
        }
    }

    // 3. Spawn background fetch
    let bundle_id_str = bundle_id.to_string();
    cx.background_executor().spawn(async move {
        if let Some(png_data) = fetch_app_icon_internal(&bundle_id_str) {
            let mut cache = MONITOR_ICON_CACHE.lock().unwrap();
            if let Some(ref mut map) = *cache {
                map.insert(bundle_id_str.clone(), Arc::new(png_data));
            }
        }
        
        // Remove from fetching state
        {
            let mut fetching = MONITOR_ICON_FETCHING_STATE.lock().unwrap();
            if let Some(ref mut set) = *fetching {
                set.remove(&bundle_id_str);
            }
        }

        // Notify UI to re-render
        if let Some(tx) = ICON_REFRESH_TX.get() {
            let _ = tx.try_send(());
        }
    }).detach();

    None
}

/// Initialize the icon cache
fn init_icon_cache() {
    let mut cache = MONITOR_ICON_CACHE.lock().unwrap();
    if cache.is_none() {
        *cache = Some(std::collections::HashMap::new());
    }
}

/// Get the display bounds that contains the given point (mouse position)
/// Returns the bounds of the display containing the point, or the main display as fallback
#[cfg(target_os = "macos")]
fn get_display_bounds_for_point(x: f64, y: f64) -> Bounds<f64> {
    use core_graphics::display::{CGGetActiveDisplayList, CGDisplayBounds};

    // Get all active displays
    let mut display_count: u32 = 0;
    let mut display_ids = [0u32; 16];

    unsafe {
        let result = CGGetActiveDisplayList(
            16,
            display_ids.as_mut_ptr(),
            &mut display_count
        );
        
        if result != 0 || display_count == 0 {
            // Fallback to main display if query fails
            return Bounds {
                origin: gpui::point(0.0, 0.0),
                size: gpui::size(1920.0, 1080.0),
            };
        }

        for i in 0..display_count {
            let id = display_ids[i as usize];
            let cg_bounds = CGDisplayBounds(id);
            
            // Check if point is inside this display's bounds
            if x >= cg_bounds.origin.x && x < cg_bounds.origin.x + cg_bounds.size.width &&
               y >= cg_bounds.origin.y && y < cg_bounds.origin.y + cg_bounds.size.height {
                return Bounds {
                    origin: gpui::point(cg_bounds.origin.x, cg_bounds.origin.y),
                    size: gpui::size(cg_bounds.size.width, cg_bounds.size.height),
                };
            }
        }
        
        // Fallback to the first (main) display
        let main_bounds = CGDisplayBounds(display_ids[0]);
        Bounds {
            origin: gpui::point(main_bounds.origin.x, main_bounds.origin.y),
            size: gpui::size(main_bounds.size.width, main_bounds.size.height),
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn get_display_bounds_for_point(_x: f64, _y: f64) -> Bounds<f64> {
    Bounds {
        origin: gpui::point(0.0, 0.0),
        size: gpui::size(1920.0, 1080.0),
    }
}

/// Helper to ensure a window position is fully visible on the given display
fn constrain_window_to_display(x: f32, y: f32, width: f32, height: f32) -> (f32, f32) {
    let display_bounds = get_display_bounds_for_point(x as f64, y as f64);
    
    let min_x = display_bounds.origin.x as f32;
    let max_x = (display_bounds.origin.x + display_bounds.size.width) as f32 - width;
    let min_y = display_bounds.origin.y as f32;
    let max_y = (display_bounds.origin.y + display_bounds.size.height) as f32 - height;
    
    (x.clamp(min_x, max_x), y.clamp(min_y, max_y))
}

/// Global state to track last window origin for "follow cursor" logic
static LAST_WINDOW_ORIGIN: std::sync::Mutex<Option<Point<Pixels>>> = std::sync::Mutex::new(None);

/// Iterate visible Flypaste paste panels (excludes settings window).
#[cfg(target_os = "macos")]
fn for_each_paste_panel_window(mut f: impl FnMut(*mut objc::runtime::Object)) {
    unsafe {
        use objc::{msg_send, sel, sel_impl};

        let cls_app = objc::runtime::Class::get("NSApplication").unwrap();
        let app: *mut objc::runtime::Object = msg_send![cls_app, sharedApplication];
        let windows: *mut objc::runtime::Object = msg_send![app, windows];
        let count: usize = msg_send![windows, count];

        for i in 0..count {
            let win: *mut objc::runtime::Object = msg_send![windows, objectAtIndex: i];
            if !msg_send![win, canBecomeKeyWindow] || !msg_send![win, isVisible] {
                continue;
            }

            let title: *mut objc::runtime::Object = msg_send![win, title];
            if !title.is_null() {
                let c_str: *const std::ffi::c_char = msg_send![title, UTF8String];
                if !c_str.is_null() {
                    let title_str = std::ffi::CStr::from_ptr(c_str).to_string_lossy();
                    // Settings window (Normal) — not a paste panel
                    if title_str == "设置" || title_str == "Settings" {
                        continue;
                    }
                }
            }

            f(win);
        }
    }
}

/// Make the paste panel key so TextInput can receive keyboard events.
#[cfg(target_os = "macos")]
pub(crate) fn make_paste_panel_key() {
    for_each_paste_panel_window(|win| unsafe {
        use objc::{msg_send, sel, sel_impl};
        let _: () = msg_send![win, makeKeyWindow];
    });
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn make_paste_panel_key() {}

/// Bring Flypaste to the foreground so IME and menu-bar input sources attach to this app.
#[cfg(target_os = "macos")]
pub(crate) fn activate_flypaste_app() {
    unsafe {
        use objc::{msg_send, sel, sel_impl};
        let cls_app = objc::runtime::Class::get("NSApplication").unwrap();
        let app: *mut objc::runtime::Object = msg_send![cls_app, sharedApplication];
        let _: () = msg_send![app, activateIgnoringOtherApps: 1];
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn activate_flypaste_app() {}

#[cfg(target_os = "macos")]
pub(crate) fn hide_flypaste_app() {
    unsafe {
        use objc::{msg_send, sel, sel_impl};
        let cls_app = objc::runtime::Class::get("NSApplication").unwrap();
        let app: *mut objc::runtime::Object = msg_send![cls_app, sharedApplication];
        let _: () = msg_send![app, hide: std::ptr::null_mut::<objc::runtime::Object>()];
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn hide_flypaste_app() {}

/// Activate Flypaste and make the paste panel key (required for Chinese IME in search).
#[cfg(target_os = "macos")]
pub(crate) fn activate_for_search() {
    activate_flypaste_app();
    make_paste_panel_key();
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn activate_for_search() {}

/// Apply macOS panel settings so clicks on buttons/list items don't activate Flypaste.
#[cfg(target_os = "macos")]
fn configure_paste_panel_windows() {
    for_each_paste_panel_window(|win| unsafe {
        use objc::{msg_send, sel, sel_impl};

        let responds: bool =
            msg_send![win, respondsToSelector: sel!(setBecomesKeyOnlyIfNeeded:)];
        if !responds {
            return;
        }

        let _: () = msg_send![win, setHidesOnDeactivate: 0];
        let _: () = msg_send![win, setCanHide: 0];
        // stationary | moveToActiveSpace | Transient | IgnoresCycle | fullScreenAuxiliary
        let _: () = msg_send![win, setCollectionBehavior: 2 | 16 | 4096 | 8192 | 2048];
        // Only become key when clicking a view that needs keyboard (e.g. search field)
        let _: () = msg_send![win, setBecomesKeyOnlyIfNeeded: 1];
        let _: () = msg_send![win, setLevel: 25];
        let _: () = msg_send![win, orderFrontRegardless];
    });
}

#[cfg(not(target_os = "macos"))]
fn configure_paste_panel_windows() {}

/// Whether the paste window is in browse mode (outside-click monitor only active then).
static PASTE_WINDOW_BROWSE_MODE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

#[cfg(target_os = "macos")]
static BROWSE_CLICK_MONITOR: std::sync::Mutex<Option<usize>> = std::sync::Mutex::new(None);

/// Toggle browse-mode outside-click monitor (search mode disables it).
pub fn set_paste_window_browse_mode(browse: bool) {
    PASTE_WINDOW_BROWSE_MODE.store(browse, std::sync::atomic::Ordering::SeqCst);
    #[cfg(target_os = "macos")]
    {
        if browse {
            start_browse_click_monitor();
        } else {
            stop_browse_click_monitor();
        }
    }
}

pub(crate) fn enter_focus_mode() {
    IN_FOCUS_MODE.store(true, std::sync::atomic::Ordering::SeqCst);
    set_paste_window_browse_mode(false);
}

pub(crate) fn leave_focus_mode() {
    IN_FOCUS_MODE.store(false, std::sync::atomic::Ordering::SeqCst);
    set_paste_window_browse_mode(true);
}

pub(crate) fn is_in_focus_mode_for_paste() -> bool {
    IN_FOCUS_MODE.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(target_os = "macos")]
fn get_paste_panel_frame() -> Option<cocoa::foundation::NSRect> {
    let mut frame = None;
    for_each_paste_panel_window(|win| {
        if frame.is_none() {
            unsafe {
                use objc::{msg_send, sel, sel_impl};
                let f: cocoa::foundation::NSRect = msg_send![win, frame];
                frame = Some(f);
            }
        }
    });
    frame
}

#[cfg(target_os = "macos")]
fn mouse_location_appkit() -> (f64, f64) {
    unsafe {
        use objc::{msg_send, sel, sel_impl};
        let cls = objc::runtime::Class::get("NSEvent").unwrap();
        let loc: cocoa::foundation::NSPoint = msg_send![cls, mouseLocation];
        (loc.x, loc.y)
    }
}

#[cfg(target_os = "macos")]
fn point_in_frame(x: f64, y: f64, frame: cocoa::foundation::NSRect) -> bool {
    x >= frame.origin.x
        && x <= frame.origin.x + frame.size.width
        && y >= frame.origin.y
        && y <= frame.origin.y + frame.size.height
}

#[cfg(target_os = "macos")]
fn handle_browse_mode_outside_click() {
    if !WINDOW_IS_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    if !PASTE_WINDOW_BROWSE_MODE.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    if PINNED.load(std::sync::atomic::Ordering::SeqCst)
        || PREVIEW_IS_OPEN.load(std::sync::atomic::Ordering::SeqCst)
    {
        return;
    }

    let Some(frame) = get_paste_panel_frame() else {
        return;
    };

    let (mx, my) = mouse_location_appkit();
    if point_in_frame(mx, my, frame) {
        return;
    }

    if let Some(tx) = HOTKEY_TX.get() {
        let _ = tx.try_send(HotkeyEvent::WindowAction(hotkey::WindowHotkeyAction::Close));
    }
}

#[cfg(target_os = "macos")]
fn start_browse_click_monitor() {
    use block::ConcreteBlock;
    use objc::{msg_send, sel, sel_impl};

    if !WINDOW_IS_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    if !PASTE_WINDOW_BROWSE_MODE.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }

    stop_browse_click_monitor();

    let handler = ConcreteBlock::new(
        |_event: *mut objc::runtime::Object| handle_browse_mode_outside_click(),
    );
    let handler = handler.copy();

    unsafe {
        let cls = objc::runtime::Class::get("NSEvent").unwrap();
        // NSLeftMouseDownMask
        let mask: u64 = 1 << 1;
        let monitor: *mut objc::runtime::Object =
            msg_send![cls, addGlobalMonitorForEventsMatchingMask: mask handler: &*handler];

        if monitor.is_null() {
            log::warn!(
                "Global mouse monitor unavailable — grant Accessibility permission to Flypaste"
            );
            return;
        }

        *BROWSE_CLICK_MONITOR.lock().unwrap() = Some(monitor as usize);
    }
}

#[cfg(target_os = "macos")]
fn stop_browse_click_monitor() {
    use objc::{msg_send, sel, sel_impl};

    if let Some(monitor) = BROWSE_CLICK_MONITOR.lock().unwrap().take() {
        unsafe {
            let cls = objc::runtime::Class::get("NSEvent").unwrap();
            let monitor = monitor as *mut objc::runtime::Object;
            let _: () = msg_send![cls, removeMonitor: monitor];
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn stop_browse_click_monitor() {}

/// Create window options for main paste window
/// If `pinned_frame` is Some, use that position/size/display instead of following cursor
pub fn make_window_options(
    follow_cursor: bool,
    pinned_frame: Option<(f64, f64, f64, f64, Option<u32>)>,
) -> WindowOptions {
    let (cursor_x, cursor_y) = get_cursor_position();
    const WINDOW_WIDTH: f32 = 400.0;
    const WINDOW_HEIGHT: f32 = 500.0;

    let display_id = pinned_frame
        .and_then(|(_, _, _, _, id)| id.map(DisplayId::new));

    let origin = if let Some((x, y, _, _, _)) = pinned_frame {
        // Pinned mode: use the saved frame position (display-relative GPUI coords)
        point(px(x as f32), px(y as f32))
    } else if follow_cursor {
        let (constrained_x, constrained_y) =
            constrain_window_to_display(cursor_x, cursor_y, WINDOW_WIDTH, WINDOW_HEIGHT);
        point(px(constrained_x), px(constrained_y))
    } else if let Some(last_origin) = *LAST_WINDOW_ORIGIN.lock().unwrap() {
        last_origin
    } else {
        let (x, y) = (100.0, 100.0);
        let (constrained_x, constrained_y) =
            constrain_window_to_display(x, y, WINDOW_WIDTH, WINDOW_HEIGHT);
        point(px(constrained_x), px(constrained_y))
    };

    // Store the origin we're about to use
    *LAST_WINDOW_ORIGIN.lock().unwrap() = Some(origin);

    let (width, height) = if let Some((_, _, w, h, _)) = pinned_frame {
        (w as f32, h as f32)
    } else {
        (WINDOW_WIDTH, WINDOW_HEIGHT)
    };

    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin,
            size: size(px(width), px(height)),
        })),
        titlebar: None,
        focus: false,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: true,
        is_resizable: true,
        is_minimizable: true,
        display_id,
        window_background: WindowBackgroundAppearance::Blurred,
        app_id: Some("flypaste".to_string()),
        window_min_size: None,
        window_decorations: Some(WindowDecorations::Client),
        tabbing_identifier: None,
    }
}

/// Create window options for settings window
pub fn make_settings_window_options() -> WindowOptions {
    let (cursor_x, cursor_y) = get_cursor_position();
    let display_bounds = get_display_bounds_for_point(cursor_x as f64, cursor_y as f64);

    let screen_width = display_bounds.size.width as f32;
    let screen_height = display_bounds.size.height as f32;
    
    const WINDOW_WIDTH: f32 = 760.0;
    const WINDOW_HEIGHT: f32 = 560.0;

    let center_x = display_bounds.origin.x as f32 + (screen_width - WINDOW_WIDTH) / 2.0;
    let center_y = display_bounds.origin.y as f32 + (screen_height - WINDOW_HEIGHT) / 2.0;

    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(center_x), px(center_y)),
            size: size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)),
        })),
        titlebar: Some(TitlebarOptions {
            title: Some(crate::i18n::from_settings().settings_title.into()),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        focus: true,
        show: true,
        kind: WindowKind::Normal,
        is_movable: true,
        is_resizable: false,
        is_minimizable: true,
        display_id: None,
        window_background: WindowBackgroundAppearance::Opaque,
        app_id: Some("flypaste".to_string()),
        window_min_size: Some(size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT))),
        window_decorations: Some(WindowDecorations::Client),
        tabbing_identifier: None,
    }
}

#[derive(Clone, Copy, Debug)]
pub enum HotkeyEvent {
    ToggleWindow,
    ToggleSettings,
    ReopenPinnedWindow,
    WindowAction(hotkey::WindowHotkeyAction),
    PasteItem { item_id: i64, in_focus_mode: bool },
    RestoreSearchFocusAfterPreview,
}

pub static HOTKEY_TX: std::sync::OnceLock<mpsc::Sender<HotkeyEvent>> = std::sync::OnceLock::new();

/// Channel for status bar quit request (tokio async channel)
pub static STATUSBAR_QUIT_TX: std::sync::OnceLock<tokio::sync::mpsc::Sender<()>> = std::sync::OnceLock::new();

pub static ICON_REFRESH_TX: std::sync::OnceLock<tokio::sync::mpsc::Sender<()>> = std::sync::OnceLock::new();

#[derive(Clone)]
pub struct GlobalAppState {
    pub history: Arc<history::Manager>,
}

impl GlobalAppState {
    pub fn new() -> Self {
        Self {
            history: Arc::new(history::Manager::new()),
        }
    }
}

impl Global for GlobalAppState {}

static HOTKEY_MANAGER: std::sync::OnceLock<hotkey::HotkeyManager> = std::sync::OnceLock::new();

pub fn update_global_hotkey(settings: &fly_settings::Hotkey) {
    if let Some(mgr) = HOTKEY_MANAGER.get() {
        log::info!("Updating global hotkey to: {}", settings.display());
        mgr.update_hotkey(settings);
    }
}

fn register_paste_window_hotkeys() {
    if let Some(mgr) = HOTKEY_MANAGER.get() {
        let settings = fly_settings::Settings::load().unwrap_or_default();
        mgr.register_window_hotkeys(&settings);
    }
}

fn unregister_paste_window_hotkeys() {
    if let Some(mgr) = HOTKEY_MANAGER.get() {
        mgr.unregister_window_hotkeys();
    }
}

pub fn refresh_paste_window_hotkeys() {
    if let Some(mgr) = HOTKEY_MANAGER.get() {
        if WINDOW_IS_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
            let settings = fly_settings::Settings::load().unwrap_or_default();
            mgr.register_window_hotkeys(&settings);
        }
    }
}

/// Open the settings window, or bring the existing one to front.
/// Uses `cx.defer` when focusing an already-open window to avoid nested `window.update` deadlocks.
fn toggle_settings(cx: &mut App) {
    if SETTINGS_IS_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
        if let Some(handle) = SETTINGS_WINDOW_HANDLE.lock().unwrap().clone() {
            cx.defer(move |cx| {
                if handle
                    .update(cx, |view, window, cx| {
                        window.activate_window();
                        window.focus(&view.focus_handle(cx), cx);
                    })
                    .is_err()
                {
                    on_settings_window_released();
                }
            });
            return;
        }
        on_settings_window_released();
    }

    let result = cx.open_window(make_settings_window_options(), |window, cx| {
        cx.new(|cx| SettingsWindow::new(window, cx))
    });

    if let Ok(handle) = result {
        SETTINGS_IS_OPEN.store(true, std::sync::atomic::Ordering::SeqCst);
        *SETTINGS_WINDOW_HANDLE.lock().unwrap() = Some(handle.clone());
        let _ = handle.update(cx, |view, window, cx| {
            window.activate_window();
            window.focus(&view.focus_handle(cx), cx);
        });
    }
}

/// Prepare paste data, then close the window outside the view update callback to avoid GPUI deadlock.
fn paste_and_close_window(cx: &mut AsyncApp, item_id: i64) {
    let prepared = update_paste_window(cx, |view, window, cx| {
        let data = view.prepare_paste(item_id, cx);
        if data.is_some() {
            paste_window::PasteWindow::save_pinned_frame_if_needed(window, cx);
        }
        data
    })
    .flatten();

    let Some((item_id, history, content)) = prepared else {
        return;
    };

    let Some(handle) = WINDOW_HANDLE.lock().unwrap().take() else {
        return;
    };

    if handle
        .update(cx, |_, window, _| window.remove_window())
        .is_err()
    {
        on_paste_window_released();
        return;
    }

    paste_window::PasteWindow::spawn_paste_after_close(item_id, history, content);
}

fn copy_item_to_clipboard_with_error(cx: &mut AsyncApp, item_id: i64) {
    let prepared = update_paste_window(cx, |view, _, cx| view.prepare_paste(item_id, cx)).flatten();

    let Some((item_id, _, content)) = prepared else {
        return;
    };

    if let Err(e) = injector::copy_to_clipboard(&content) {
        log::error!("copy_to_clipboard failed: {:?}", e);
        return;
    }

    let _ = update_paste_window(cx, |view, _window, cx| view.trigger_ghost_error(item_id, cx));
}

fn handle_paste_item(cx: &mut AsyncApp, item_id: i64, in_focus_mode: Option<bool>) {
    let in_focus_mode =
        in_focus_mode.unwrap_or_else(is_in_focus_mode_for_paste);

    if in_focus_mode {
        copy_item_to_clipboard_with_error(cx, item_id);
    } else {
        paste_and_close_window(cx, item_id);
    }
}

pub fn init(cx: &mut App) {
    // 0. Initialize Hotkey Channel early to avoid race conditions
    let (tx, mut rx) = mpsc::channel::<HotkeyEvent>(100);
    if let Err(_) = HOTKEY_TX.set(tx) {
        log::error!("HOTKEY_TX already set during init");
    }

    let (icon_tx, mut icon_rx) = tokio::sync::mpsc::channel::<()>(100);
    if let Err(_) = ICON_REFRESH_TX.set(icon_tx) {
        log::error!("ICON_REFRESH_TX already set during init");
    }

    init_icon_cache();

    settings::init(cx);
    theme_settings::init(theme::LoadThemes::All(Box::new(crate::assets::Assets)), cx);
    editor::init(cx);

    cx.set_menus(vec![
        Menu {
            name: "Flypaste".into(),
            items: vec![MenuItem::action("Quit Flypaste", Quit)],
            disabled: false,
        },
        Menu {
            name: "Edit".into(),
            items: vec![
                MenuItem::action("Undo", editor::actions::Undo),
                MenuItem::action("Redo", editor::actions::Redo),
                MenuItem::separator(),
                MenuItem::action("Cut", editor::actions::Cut),
                MenuItem::action("Copy", editor::actions::Copy),
                MenuItem::action("Paste", editor::actions::Paste),
                MenuItem::action("Select All", editor::actions::SelectAll),
            ],
            disabled: false,
        },
    ]);

    cx.on_action(|_: &Quit, cx| {
        let app_state = cx.global::<GlobalAppState>();
        app_state.history.save_searcher();
        cx.quit();
    });

    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some("TextInput")),
        KeyBinding::new("delete", Delete, Some("TextInput")),
        KeyBinding::new("left", Left, Some("TextInput")),
        KeyBinding::new("right", Right, Some("TextInput")),
        KeyBinding::new("cmd-a", SelectAll, Some("TextInput")),
        KeyBinding::new("cmd-,", ToggleSettings, None),
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("backspace", editor::actions::Backspace, Some("NumberFieldEditor")),
        KeyBinding::new("delete", editor::actions::Delete, Some("NumberFieldEditor")),
        KeyBinding::new("cmd-backspace", DeleteToBeginningOfLine::default(), Some("TextInput")),
        KeyBinding::new("cmd-left", MoveToBeginningOfLine::default(), Some("TextInput")),
        KeyBinding::new("cmd-right", MoveToEndOfLine::default(), Some("TextInput")),
        KeyBinding::new("cmd-v", editor::actions::Paste, None),
    ]);

    cx.on_action(|_: &ToggleSettings, cx| {
        toggle_settings(cx);
    });

    let app_state = GlobalAppState::new();
    cx.set_global(app_state.clone());

    // Initialize statusbar quit channel
    let (statusbar_quit_tx, mut statusbar_quit_rx) = tokio::sync::mpsc::channel::<()>(1);
    if let Err(_) = STATUSBAR_QUIT_TX.set(statusbar_quit_tx) {
        log::error!("STATUSBAR_QUIT_TX already set during init");
    }

    let settings = fly_settings::Settings::load().unwrap_or_default();
    sync_panel_opacity_from_settings(&settings);
    let hotkey_manager = hotkey::HotkeyManager::new(
        move || {
            if let Some(tx) = HOTKEY_TX.get() {
                if let Err(e) = tx.try_send(HotkeyEvent::ToggleWindow) {
                    log::error!("Failed to send ToggleWindow event: {:?}", e);
                }
            }
        },
        move |action| {
            if let Some(tx) = HOTKEY_TX.get() {
                if let Err(e) = tx.try_send(HotkeyEvent::WindowAction(action)) {
                    log::error!("Failed to send WindowAction event: {:?}", e);
                }
            }
        },
    );
    hotkey_manager.start(&settings.hotkey);
    let _ = HOTKEY_MANAGER.set(hotkey_manager);

    let history_bg = app_state.history.clone();
    std::thread::spawn(move || {
        history_bg.load_or_build_searcher();
        animated_gif::preload_doc_gifs();
    });

    let history_poll = app_state.history.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let mut monitor = clipboard::Monitor::new().unwrap();
            loop {
                if let Some(content) = monitor.poll().await {
                    let bundle_id = get_front_app_bundle_id_internal();
                    history_poll.add(content, bundle_id).await;
                    CLIPBOARD_UPDATE_FLAG.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        });
    });

    let history_cleanup = app_state.history.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let settings = fly_settings::Settings::load().unwrap_or_default();
            history_cleanup
                .cleanup_by_duration(settings.text_retention_days, settings.image_retention_days)
                .await;
        });
    });

    let (refresh_tx, mut refresh_rx) = tokio::sync::mpsc::channel::<()>(1);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                if CLIPBOARD_UPDATE_FLAG.load(std::sync::atomic::Ordering::SeqCst) {
                    CLIPBOARD_UPDATE_FLAG.store(false, std::sync::atomic::Ordering::SeqCst);
                    let _ = refresh_tx.send(()).await;
                }
            }
        });
    });

    let task = cx.spawn(|cx: &mut AsyncApp| {
        let mut cx = cx.clone();
        async move {
            loop {
                tokio::select! {
                    event = rx.recv() => {
                        if let Some(event) = event {
                            match event {
                                HotkeyEvent::PasteItem { item_id, in_focus_mode } => {
                                    if WINDOW_IS_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
                                        handle_paste_item(&mut cx, item_id, Some(in_focus_mode));
                                    }
                                }
                                HotkeyEvent::RestoreSearchFocusAfterPreview => {
                                    if WINDOW_IS_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
                                        restore_search_focus_on_paste_window(&mut cx);
                                    }
                                }
                                HotkeyEvent::WindowAction(action) => {
                                    if !WINDOW_IS_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
                                        continue;
                                    }
                                    match action {
                                        hotkey::WindowHotkeyAction::Close => {
                                            close_paste_window(&mut cx);
                                        }
                                        hotkey::WindowHotkeyAction::PasteIndex(index) => {
                                            let item_id = update_paste_window(&mut cx, |view, _, _| {
                                                view.item_id_at_index(index as usize)
                                            })
                                            .flatten();
                                            if let Some(item_id) = item_id {
                                                handle_paste_item(&mut cx, item_id, None);
                                            }
                                        }
                                        _ => {
                                            let _ = update_paste_window(&mut cx, |view, window, cx| {
                                                view.handle_window_hotkey(action, window, cx);
                                            });
                                        }
                                    }
                                }
                                HotkeyEvent::ToggleWindow => {
                                    let is_open = WINDOW_IS_OPEN.load(std::sync::atomic::Ordering::SeqCst);
                                    let has_handle = WINDOW_HANDLE.lock().unwrap().is_some();
                                    
                                    log::info!("HotkeyEvent::ToggleWindow received. is_open: {}, has_handle: {}", is_open, has_handle);

                                    if is_open && !has_handle { reset_window_flag(); }

                                    if is_open && has_handle {
                                        let mut handled = false;
                                        if let Some(handle) = WINDOW_HANDLE.lock().unwrap().clone() {
                                            let _ = handle.update(&mut cx, |view, _window, cx| {
                                                let settings = fly_settings::Settings::load().unwrap_or_default();
                                                let has_hotkey_modifier = PasteWindow::is_global_modifier_pressed(&settings);
                                                
                                                log::info!("ToggleWindow - has_hotkey_modifier: {}, modifier_released_since_open: {}", has_hotkey_modifier, view.modifier_released_since_open);
                                                
                                                if has_hotkey_modifier && !view.modifier_released_since_open {
                                                    log::info!("ToggleWindow - cycling to next item");
                                                    view.start_alt_tab_monitor(cx);
                                                    if !view.first_alt_tab_done {
                                                        view.first_alt_tab_done = true;
                                                        cx.notify();
                                                    } else {
                                                        view.select_next(cx);
                                                    }
                                                    handled = true;
                                                }
                                            });
                                        }
                                        if handled {
                                            continue;
                                        }
                                        log::info!("ToggleWindow - closing window");
                                        close_paste_window(&mut cx);
                                    } else {
                                        if WINDOW_IS_OPEN.compare_exchange(false, true, std::sync::atomic::Ordering::SeqCst, std::sync::atomic::Ordering::SeqCst).is_ok() {
                                            #[cfg(target_os = "macos")]
                                            unsafe {
                                                use objc::{msg_send, sel, sel_impl, runtime::Class};
                                                let _cls = Class::get("NSWorkspace").unwrap();
                                                let _workspace: *mut objc::runtime::Object = msg_send![_cls, sharedWorkspace];
                                                // Front app PID is no longer stored - paste will rely on macOS focus restoration
                                            }

                                            let result = cx.open_window(make_window_options(true, None), |window, cx| {
                                                cx.new(|cx| PasteWindow::new(window, cx))
                                            });

                                            if let Ok(handle) = result {
                                                let handle_clone = handle.clone();
                                                *WINDOW_HANDLE.lock().unwrap() = Some(handle);

                                                let _ = handle_clone.update(&mut cx, |_view, _window, _cx| {
                                                    configure_paste_panel_windows();
                                                    register_paste_window_hotkeys();
                                                    _cx.foreground_executor().spawn(async move {
                                                        set_paste_window_browse_mode(true);
                                                    }).detach();
                                                });
                                            } else { reset_window_flag(); }
                                        }
                                    }
                                }
                                HotkeyEvent::ToggleSettings => {
                                    activate_flypaste_app();
                                    let _ = cx.update(|cx| toggle_settings(cx));
                                }
                                HotkeyEvent::ReopenPinnedWindow => {
                                    // Reopen the pinned window after paste
                                    let is_open = WINDOW_IS_OPEN.load(std::sync::atomic::Ordering::SeqCst);
                                    if is_open {
                                        // Window is already open, don't reopen
                                        return;
                                    }

                                    // Get saved frame for pinned mode
                                    let pinned_frame = if PINNED.load(std::sync::atomic::Ordering::SeqCst) {
                                        *PINNED_WINDOW_FRAME.lock().unwrap()
                                    } else {
                                        None
                                    };

                                    if WINDOW_IS_OPEN.compare_exchange(false, true, std::sync::atomic::Ordering::SeqCst, std::sync::atomic::Ordering::SeqCst).is_ok() {
                                        let result = cx.open_window(make_window_options(false, pinned_frame), |window, cx| {
                                            cx.new(|cx| PasteWindow::new(window, cx))
                                        });

                                        if let Ok(handle) = result {
                                            *WINDOW_HANDLE.lock().unwrap() = Some(handle);

                                            let _ = handle.update(&mut cx, |_view, _window, _cx| {
                                                configure_paste_panel_windows();
                                                register_paste_window_hotkeys();
                                                _cx.foreground_executor().spawn(async move {
                                                    set_paste_window_browse_mode(true);
                                                }).detach();
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ = refresh_rx.recv() => {
                        let _ = update_paste_window(&mut cx, |view, _, cx| view.reload_items(cx));
                    }
                    _ = icon_rx.recv() => {
                        let _ = update_paste_window(&mut cx, |_, _, cx| cx.notify());
                    }
                    _ = statusbar_quit_rx.recv() => {
                        // Statusbar quit received - save and quit
                        let _ = cx.update(|cx| {
                            let app_state = cx.global::<GlobalAppState>();
                            app_state.history.save_searcher();
                            cx.quit();
                        });
                    }
                }
            }
        }
    });
    task.detach();
}

pub fn reset_window_flag() {
    unregister_paste_window_hotkeys();
    stop_browse_click_monitor();
    IN_FOCUS_MODE.store(false, std::sync::atomic::Ordering::SeqCst);
    PASTE_WINDOW_BROWSE_MODE.store(true, std::sync::atomic::Ordering::SeqCst);
    WINDOW_IS_OPEN.store(false, std::sync::atomic::Ordering::SeqCst);
}

fn schedule_search_focus_restore() {
    if RESTORE_SEARCH_FOCUS_AFTER_PREVIEW
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
        )
        .is_err()
    {
        return;
    }
    if let Some(tx) = HOTKEY_TX.get() {
        let _ = tx.try_send(HotkeyEvent::RestoreSearchFocusAfterPreview);
    }
}

fn restore_search_focus_on_paste_window(cx: &mut AsyncApp) {
    if !RESTORE_SEARCH_FOCUS_AFTER_PREVIEW.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let _ = update_paste_window(cx, |_view, window, cx| {
        cx.on_next_frame(window, |view, window, cx| {
            RESTORE_SEARCH_FOCUS_AFTER_PREVIEW.store(false, std::sync::atomic::Ordering::SeqCst);
            view.focus_search_input(window, cx);
        });
    });
}

/// Close preview from inside PreviewWindow callbacks (direct remove_window, no nested entity update).
pub(crate) fn close_preview_from_window(window: &mut Window) {
    if PREVIEW_IS_OPEN
        .compare_exchange(
            true,
            false,
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
        )
        .is_err()
    {
        return;
    }
    PREVIEW_WINDOW_HANDLE.lock().unwrap().take();
    schedule_search_focus_restore();
    window.remove_window();
}

/// Close an existing preview before opening a new one (external context).
pub(crate) fn dismiss_existing_preview(cx: &mut App) {
    if !PREVIEW_IS_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    PREVIEW_IS_OPEN.store(false, std::sync::atomic::Ordering::SeqCst);
    if let Some(handle) = PREVIEW_WINDOW_HANDLE.lock().unwrap().take() {
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    }
}
