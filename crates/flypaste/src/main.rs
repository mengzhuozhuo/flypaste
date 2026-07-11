#![allow(unexpected_cfgs)]

use gpui::Application;
use gpui_platform;

mod assets;
mod autolaunch;
mod i18n;
mod statusbar;
mod ui;

fn init_logging() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
}

fn main() {
    init_logging();

    let app = Application::with_platform(gpui_platform::current_platform(false))
        .with_assets(assets::Assets);

    app.with_quit_mode(gpui::QuitMode::Explicit)
        .run(move |cx| {
            #[cfg(target_os = "macos")]
            unsafe {
                use objc::{msg_send, sel, sel_impl};
                let cls = objc::runtime::Class::get("NSApplication").unwrap();
                let app: *mut objc::runtime::Object = msg_send![cls, sharedApplication];
                let _: () = msg_send![app, setActivationPolicy: 1];
            }

            autolaunch::sync_from_settings();

            // Initialize status bar (menu bar icon) after GPUUI is running
            statusbar::init();
            ui::init(cx);
        });
}
