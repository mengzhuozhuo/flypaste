//! Status bar icon (menu bar icon) implementation using objc2-app-kit

use crate::assets::Assets;
use crate::i18n;
use objc2::define_class;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem};
use objc2_foundation::{NSData, NSObject, NSSize, NSString};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::thread;

/// Events that can be triggered from the status bar menu
#[derive(Debug, Clone, Copy)]
pub enum StatusBarEvent {
    OpenPasteWindow,
    OpenSettings,
    RequestQuit,
}

/// Global channel sender for status bar events
static STATUSBAR_EVENT_TX: OnceLock<mpsc::Sender<StatusBarEvent>> = OnceLock::new();

/// Pointer to the leaked NSStatusItem (as usize) to prevent release
static STATUS_ITEM_PTR: OnceLock<usize> = OnceLock::new();
/// Pointer to the leaked NSMenu (as usize) to prevent release
static STATUS_MENU_PTR: OnceLock<usize> = OnceLock::new();
static PASTE_ITEM_PTR: OnceLock<usize> = OnceLock::new();
static SETTINGS_ITEM_PTR: OnceLock<usize> = OnceLock::new();
static QUIT_ITEM_PTR: OnceLock<usize> = OnceLock::new();

/// Initialize the status bar (menu bar icon)
pub fn init() {
    let (tx, rx) = mpsc::channel::<StatusBarEvent>();
    let _ = STATUSBAR_EVENT_TX.set(tx);

    thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            match event {
                StatusBarEvent::OpenPasteWindow => {
                    if let Some(tx) = super::ui::HOTKEY_TX.get() {
                        let _ = tx.try_send(super::ui::HotkeyEvent::ToggleWindow);
                    }
                }
                StatusBarEvent::OpenSettings => {
                    if let Some(tx) = super::ui::HOTKEY_TX.get() {
                        let _ = tx.try_send(super::ui::HotkeyEvent::ToggleSettings);
                    }
                }
                StatusBarEvent::RequestQuit => {
                    if let Some(tx) = super::ui::STATUSBAR_QUIT_TX.get() {
                        let _ = tx.try_send(());
                    }
                }
            }
        }
    });

    init_ns_status_item();
}

pub fn refresh_menu_language() {
    let strings = i18n::from_settings();

    unsafe {
        if let Some(&ptr) = PASTE_ITEM_PTR.get() {
            let item = &*(ptr as *const Retained<NSMenuItem>);
            item.setTitle(&nsstring(strings.status_paste_history));
        }
        if let Some(&ptr) = SETTINGS_ITEM_PTR.get() {
            let item = &*(ptr as *const Retained<NSMenuItem>);
            item.setTitle(&nsstring(strings.status_settings));
        }
        if let Some(&ptr) = QUIT_ITEM_PTR.get() {
            let item = &*(ptr as *const Retained<NSMenuItem>);
            item.setTitle(&nsstring(strings.status_quit));
        }
    }
}

fn nsstring(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

// Create a simple NSObject subclass to handle menu actions
define_class!(
    #[unsafe(super = NSObject)]
    #[name = "FlypasteMenuHandler"]
    struct MenuHandler;

    impl MenuHandler {
        #[unsafe(method(handlePasteHistory:))]
        fn handle_paste_history(&self, _sender: &NSObject) {
            if let Some(tx) = STATUSBAR_EVENT_TX.get() {
                let _ = tx.send(StatusBarEvent::OpenPasteWindow);
            }
        }

        #[unsafe(method(handleSettings:))]
        fn handle_settings(&self, _sender: &NSObject) {
            if let Some(tx) = STATUSBAR_EVENT_TX.get() {
                let _ = tx.send(StatusBarEvent::OpenSettings);
            }
        }

        #[unsafe(method(handleQuit:))]
        fn handle_quit(&self, _sender: &NSObject) {
            if let Some(tx) = STATUSBAR_EVENT_TX.get() {
                let _ = tx.send(StatusBarEvent::RequestQuit);
            }
        }
    }
);

fn init_ns_status_item() {
    let mtm = MainThreadMarker::new().expect("NSStatusItem must be created on main thread");
    let strings = i18n::from_settings();

    let status_bar = NSStatusBar::systemStatusBar();
    let item: Retained<NSStatusItem> = status_bar.statusItemWithLength(-1.0);

    // Set icon from template SVG
    if let Some(button) = item.button(mtm) {
        if let Some(svg_file) = Assets::get("icons/ghost.svg") {
            let data = NSData::from_vec(svg_file.data.to_vec());
            let image = NSImage::initWithData(mtm.alloc(), &data);
            if let Some(image) = image {
                unsafe {
                    let _: () = msg_send![&image, setSize: NSSize::new(18.0, 18.0)];
                    let _: () = msg_send![&image, setTemplate: true];
                }
                button.setImage(Some(&image));
            } else {
                log::warn!("Failed to create NSImage from ghost.svg");
                button.setTitle(&nsstring("📋"));
            }
        } else {
            log::warn!("Could not find ghost.svg in assets");
            button.setTitle(&nsstring("📋"));
        }
    } else {
        log::warn!("Could not get button to set icon");
    }

    // Create menu
    let menu: Retained<NSMenu> = NSMenu::new(mtm);

    // Create handler for menu actions
    let handler: Retained<MenuHandler> = unsafe { msg_send![mtm.alloc(), init] };
    let handler_retained = handler;

    // Leak handler to prevent release
    let _handler_ptr = Box::leak(Box::new(handler_retained.clone())) as *mut Retained<MenuHandler>;

    // Paste History
    let paste_item = NSMenuItem::new(mtm);
    paste_item.setTitle(&nsstring(strings.status_paste_history));
    paste_item.setTag(1);
    paste_item.setEnabled(true);
    // Set action and target using objc2's methods
    unsafe {
        paste_item.setAction(Some(objc2::sel!(handlePasteHistory:)));
        paste_item.setTarget(Some(&*handler_retained));
    }
    menu.addItem(&paste_item);

    // Settings
    let settings_item = NSMenuItem::new(mtm);
    settings_item.setTitle(&nsstring(strings.status_settings));
    settings_item.setEnabled(true);
    unsafe {
        settings_item.setAction(Some(objc2::sel!(handleSettings:)));
        settings_item.setTarget(Some(&*handler_retained));
    }
    menu.addItem(&settings_item);

    // Separator
    let sep = NSMenuItem::separatorItem(mtm);
    menu.addItem(&sep);

    // Quit
    let quit_item = NSMenuItem::new(mtm);
    quit_item.setTitle(&nsstring(strings.status_quit));
    quit_item.setTag(2);
    quit_item.setEnabled(true);
    unsafe {
        quit_item.setAction(Some(objc2::sel!(handleQuit:)));
        quit_item.setTarget(Some(&*handler_retained));
    }
    menu.addItem(&quit_item);

    // Leak objects to prevent release and store pointers as usize
    let item_ptr = Box::leak(Box::new(item.clone())) as *mut Retained<NSStatusItem>;
    let _ = STATUS_ITEM_PTR.set(item_ptr as usize);
    let menu_ptr = Box::leak(Box::new(menu.clone())) as *mut Retained<NSMenu>;
    let _ = STATUS_MENU_PTR.set(menu_ptr as usize);
    let paste_ptr = Box::leak(Box::new(paste_item.clone())) as *mut Retained<NSMenuItem>;
    let _ = PASTE_ITEM_PTR.set(paste_ptr as usize);
    let settings_ptr = Box::leak(Box::new(settings_item.clone())) as *mut Retained<NSMenuItem>;
    let _ = SETTINGS_ITEM_PTR.set(settings_ptr as usize);
    let quit_ptr = Box::leak(Box::new(quit_item.clone())) as *mut Retained<NSMenuItem>;
    let _ = QUIT_ITEM_PTR.set(quit_ptr as usize);

    // Set the menu
    item.setMenu(Some(&menu));
}
