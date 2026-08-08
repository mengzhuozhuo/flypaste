#![allow(unexpected_cfgs)]

use crate::assets::Assets;
use crate::ui::GlobalAppState;
use crate::ui::text_input::TextInput;
use clipboard::ClipboardContent;
use image::GenericImageView;
use gpui::{
    ease_in_out, prelude::*, rgba, App, Bounds, BoxShadow, Div, FocusHandle, Focusable, HighlightStyle,
    Image, ImageFormat, ImageSource, KeyDownEvent, Render, ScrollHandle, ScrollWheelEvent, Stateful,
    StyledText, Window, WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowKind,
    WindowOptions, div, img, point, px, rgb, Entity,
};
use history::{image_path_for_hash, ClipboardItem, SearchResult};
use theme::ActiveTheme;
use std::sync::Arc;
use std::time::Duration;
use injector::inject;
use ui::{Color, Icon, IconName, IconSize};

const SEARCH_HIGHLIGHT_MS: u64 = 180;

fn focus_mode_glow(highlight: f32) -> Vec<BoxShadow> {
    vec![
        BoxShadow {
            color: rgba(0x007AFF00 | u32::from((56.0 * highlight) as u8)).into(),
            offset: point(px(0.), px(0.)),
            blur_radius: px(5.),
            spread_radius: px(0.),
        },
        BoxShadow {
            color: rgba(0x007AFF00 | u32::from((31.0 * highlight) as u8)).into(),
            offset: point(px(0.), px(0.)),
            blur_radius: px(10.),
            spread_radius: px(1.),
        },
    ]
}

static FLYPASTE_ICON_PATH: &str = "icons/ghost.svg";

/// The main paste window that shows clipboard history
pub struct PasteWindow {
    selected_index: usize,
    focus_handle: FocusHandle,
    search_input: Entity<TextInput>,
    items: Vec<SearchResult>,     // Current list of items for preview access
    offset: usize,                 // Current offset for lazy loading
    needs_load_more: bool,        // Flag to trigger lazy load in render
    scroll_handle: ScrollHandle,   // Handle for tracking scroll position
    hovered_index: Option<usize>,   // Track which item is hovered for space preview
    thumbnail_cache: std::collections::HashMap<i64, (Arc<gpui::Image>, u32, u32)>,  // 缓存解码后的图片和尺寸
    is_regex_mode: bool,          // Regex search mode toggle
    is_case_sensitive: bool,      // Case sensitive search toggle
    copy_flash_item_id: Option<i64>,
    /// 0 = browse highlight off, 1 = search highlight on (animated)
    search_highlight: f32,
    search_highlight_generation: u64,
    ghost_error_animation: f32,
    ghost_error_generation: u64,
}

impl PasteWindow {
    fn persist_search_modes(regex_enabled: bool, case_sensitive_enabled: bool) {
        if let Ok(mut settings) = fly_settings::Settings::load() {
            settings.regex_enabled = regex_enabled;
            settings.case_sensitive_enabled = case_sensitive_enabled;
            let _ = settings.save();
        }
    }

    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        crate::ui::apply_theme(window.appearance(), cx);

        cx.observe_window_appearance(window, |_, window, cx| {
            let appearance = window.appearance();
            cx.spawn(async move |_, cx| {
                let _ = cx.update(|cx| {
                    crate::ui::apply_theme(appearance, cx);
                });
            }).detach();
        }).detach();

        let focus_handle = cx.focus_handle();

        cx.on_release(|_this, _cx| {
            crate::ui::dismiss_existing_preview(_cx);
            crate::ui::on_paste_window_released();
        })
        .detach();

        // Load initial items from history
        let app_state = cx.global::<GlobalAppState>();
        let items: Vec<SearchResult> = app_state.history.get_recent_sync(0, 50)
            .into_iter()
            .map(|item| SearchResult { item, match_range: None })
            .collect();

        // Use our custom TextInput component for precise event control
        let paste_window = cx.entity().downgrade();
        let search_input = cx.new(|cx| {
            TextInput::new(cx, "Search history...".to_string())
                .on_focus_requested(|window, _cx| {
                    _cx.foreground_executor().spawn(async move {
                        crate::ui::activate_for_search();
                    }).detach();
                    window.activate_window();
                })
                .on_space_intercepted(move |window, cx| {
                    if let Some(paste_window) = paste_window.upgrade() {
                        cx.on_next_frame(window, move |_: &mut TextInput, _window, cx| {
                            // 在一次 update 中获取所有需要的数据，避免嵌套更新冲突
                            let result_to_preview = paste_window.update(cx, |this, _cx| {
                                this.items.get(this.hovered_index?).cloned()
                            });

                            if let Some(result) = result_to_preview {
                                crate::ui::dismiss_existing_preview(cx);
                                let preview_content = PreviewContent::from_clipboard_item(&result.item, result.match_range);
                                let result = cx.open_window(make_preview_window_options(&preview_content), |window, cx| {
                                    cx.new(|cx| PreviewWindow::new(preview_content, window, cx))
                                });
                                if let Ok(handle) = result {
                                    *crate::ui::PREVIEW_WINDOW_HANDLE.lock().unwrap() = Some(handle.clone());
                                    crate::ui::PREVIEW_IS_OPEN.store(true, std::sync::atomic::Ordering::SeqCst);
                                    let _ = handle.update(cx, |view: &mut PreviewWindow, window, cx| {
                                        window.activate_window();
                                        window.focus(&view.focus_handle(cx), cx);
                                    });
                                }
                            }
                        });
                    }
                })
        });

        // Subscribe to search input changes to re-render and reset selection
        // Use listener to access is_regex_mode state
        cx.subscribe(&search_input, |this, _text_input, _event, cx| {
            // Reload items when search text changes
            let app_state = cx.global::<GlobalAppState>();
            let search_query = this.search_input.read(cx).text(cx);
            if search_query.is_empty() {
                this.items = app_state.history.get_recent_sync(0, 50)
                    .into_iter()
                    .map(|item| SearchResult { item, match_range: None })
                    .collect();
                this.offset = 50;
            } else {
                // Use new unified search with regex mode support
                let results = app_state.history.blocking_search(&search_query, this.is_regex_mode, 50, this.is_case_sensitive);
                this.items = results;
                this.offset = 50;
            }
            this.selected_index = 0;
            this.needs_load_more = false; // Reset to prevent lazy load from overwriting search results
            cx.notify();
        }).detach();

        let search_focus = search_input.read(cx).focus_handle(cx);
        cx.on_focus(&search_focus, window, |this, _, cx| {
            crate::ui::enter_focus_mode();
            this.animate_search_highlight(1.0, cx);
        })
        .detach();

        cx.on_focus_out(&search_focus, window, |this, _, _, cx| {
            if crate::ui::PREVIEW_IS_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            this.animate_search_highlight(0.0, cx);
        })
        .detach();

        // Clicking outside the window deactivates it without firing focus_out on the
        // search field — blur explicitly so is_focused() and icon style stay in sync.
        cx.observe_window_activation(window, |_, window, cx| {
            if !window.is_window_active()
                && !crate::ui::PREVIEW_IS_OPEN.load(std::sync::atomic::Ordering::SeqCst)
            {
                window.blur();
                crate::ui::leave_focus_mode();
                cx.notify();
            }
        })
        .detach();

        let settings = fly_settings::Settings::load().unwrap_or_default();

        Self {
            selected_index: 0,
            focus_handle,
            search_input,
            items,
            offset: 50,
            needs_load_more: false,
            scroll_handle: ScrollHandle::new(),
            hovered_index: None,
            thumbnail_cache: std::collections::HashMap::new(),
            is_regex_mode: settings.regex_enabled,
            is_case_sensitive: settings.case_sensitive_enabled,
            copy_flash_item_id: None,
            search_highlight: 0.0,
            search_highlight_generation: 0,
            ghost_error_animation: 0.0,
            ghost_error_generation: 0,
        }
    }

    fn animate_search_highlight(&mut self, target: f32, cx: &mut Context<Self>) {
        if (self.search_highlight - target).abs() < f32::EPSILON {
            return;
        }

        self.search_highlight_generation += 1;
        let generation = self.search_highlight_generation;
        let start = self.search_highlight;

        cx.spawn(async move |handle, cx| {
            const STEPS: u64 = 12;
            let step_ms = SEARCH_HIGHLIGHT_MS / STEPS;
            for step in 1..=STEPS {
                cx.background_executor()
                    .timer(Duration::from_millis(step_ms))
                    .await;

                let eased = ease_in_out(step as f32 / STEPS as f32);
                let value = start + (target - start) * eased;

                let should_continue = handle
                    .update(cx, |view, cx| {
                        if view.search_highlight_generation != generation {
                            return false;
                        }
                        view.search_highlight = value;
                        cx.notify();
                        true
                    })
                    .ok()
                    .unwrap_or(false);

                if !should_continue {
                    return;
                }
            }

            let _ = handle.update(cx, |view, cx| {
                if view.search_highlight_generation == generation {
                    view.search_highlight = target;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn load_more(&mut self, cx: &mut Context<Self>) {
        self.needs_load_more = true;
        cx.notify();
    }

    pub(crate) fn focus_search_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.foreground_executor().spawn(async move {
            crate::ui::activate_for_search();
        }).detach();
        window.activate_window();
        window.focus(&self.search_input.read(cx).focus_handle(cx), cx);
    }

    pub fn save_pinned_frame_if_needed(window: &mut Window, cx: &App) {
        if !crate::ui::PINNED.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let bounds = window.bounds();
        let display_id = window
            .display(cx)
            .map(|display| u32::from(display.id()));
        *crate::ui::PINNED_WINDOW_FRAME.lock().unwrap() = Some((
            bounds.origin.x.as_f32() as f64,
            bounds.origin.y.as_f32() as f64,
            bounds.size.width.as_f32() as f64,
            bounds.size.height.as_f32() as f64,
            display_id,
        ));
    }

    pub fn spawn_paste_after_close(
        item_id: i64,
        history: std::sync::Arc<history::Manager>,
        content: ClipboardContent,
    ) {
        let is_pinned = crate::ui::PINNED.load(std::sync::atomic::Ordering::SeqCst);

        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));

            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                history.move_to_front(item_id).await;
                let _ = inject(&content).await;
            });

            if is_pinned {
                if let Some(tx) = crate::ui::HOTKEY_TX.get() {
                    let _ = tx.try_send(crate::ui::HotkeyEvent::ReopenPinnedWindow);
                }
            }
        });
    }

    pub fn trigger_ghost_error(&mut self, item_id: i64, cx: &mut Context<Self>) {
        self.copy_flash_item_id = Some(item_id);
        self.ghost_error_generation += 1;
        let generation = self.ghost_error_generation;
        cx.notify();

        cx.spawn(async move |handle, cx| {
            const STEPS: u64 = 25;
            let step_ms = 400 / STEPS;
            for step in 1..=STEPS {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(step_ms))
                    .await;

                let t = step as f32 / STEPS as f32;
                
                let should_continue = handle
                    .update(cx, |view, cx| {
                        if view.ghost_error_generation != generation {
                            return false;
                        }
                        view.ghost_error_animation = t;
                        cx.notify();
                        true
                    })
                    .ok()
                    .unwrap_or(false);

                if !should_continue {
                    return;
                }
            }

            let _ = handle.update(cx, |view, cx| {
                if view.ghost_error_generation == generation {
                    view.ghost_error_animation = 0.0;
                    view.copy_flash_item_id = None;
                    cx.notify();

                    if !crate::ui::PINNED.load(std::sync::atomic::Ordering::SeqCst) {
                        if let Some(tx) = crate::ui::HOTKEY_TX.get() {
                            let _ = tx.try_send(crate::ui::HotkeyEvent::WindowAction(
                                hotkey::WindowHotkeyAction::Close,
                            ));
                        }
                    }
                }
            });
        }).detach();
    }


    /// Reload items from database (called when clipboard changes)
    pub fn reload_items(&mut self, cx: &mut Context<Self>) {
        let app_state = cx.global::<GlobalAppState>();
        let search_query = self.search_input.read(cx).text(cx);
        if search_query.is_empty() {
            self.items = app_state.history.get_recent_sync(0, 50)
                .into_iter()
                .map(|item| SearchResult { item, match_range: None })
                .collect();
            self.offset = 50;
        } else {
            // Use blocking_search with is_regex_mode to maintain search mode consistency
            let results = app_state.history.blocking_search(&search_query, self.is_regex_mode, 50, self.is_case_sensitive);
            self.items = results;
            self.offset = 50;
        }
        // Don't reset selected_index - user may be browsing, just refresh the list
        cx.notify();
    }

    pub fn item_id_at_index(&self, index: usize) -> Option<i64> {
        self.items.get(index).map(|r| r.item.id)
    }

    pub fn prepare_paste(
        &self,
        item_id: i64,
        cx: &mut Context<Self>,
    ) -> Option<(i64, std::sync::Arc<history::Manager>, ClipboardContent)> {
        if !self.items.iter().any(|i| i.item.id == item_id) {
            return None;
        }
        let app_state = cx.global::<GlobalAppState>();
        let history = app_state.history.clone();
        let content = app_state.history.get_content_by_id(item_id);
        Some((item_id, history, content))
    }

    pub fn handle_window_hotkey(
        &mut self,
        action: hotkey::WindowHotkeyAction,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            hotkey::WindowHotkeyAction::Close | hotkey::WindowHotkeyAction::PasteIndex(_) => {}
            hotkey::WindowHotkeyAction::TogglePin => {
                let new_state = !crate::ui::PINNED.load(std::sync::atomic::Ordering::SeqCst);
                crate::ui::PINNED.store(new_state, std::sync::atomic::Ordering::SeqCst);
                cx.notify();
            }
            hotkey::WindowHotkeyAction::ToggleRegex => self.toggle_regex_mode(cx),
            hotkey::WindowHotkeyAction::ToggleCaseSensitive => self.toggle_case_sensitive(cx),
        }
    }

    fn toggle_regex_mode(&mut self, cx: &mut Context<Self>) {
        self.is_regex_mode = !self.is_regex_mode;
        Self::persist_search_modes(self.is_regex_mode, self.is_case_sensitive);
        self.refresh_search_results(cx);
    }

    fn toggle_case_sensitive(&mut self, cx: &mut Context<Self>) {
        self.is_case_sensitive = !self.is_case_sensitive;
        Self::persist_search_modes(self.is_regex_mode, self.is_case_sensitive);
        self.refresh_search_results(cx);
    }

    fn refresh_search_results(&mut self, cx: &mut Context<Self>) {
        let search_query = self.search_input.read(cx).text(cx);
        let app_state = cx.global::<GlobalAppState>();
        if search_query.is_empty() {
            self.items = app_state.history.get_recent_sync(0, 50)
                .into_iter()
                .map(|item| SearchResult { item, match_range: None })
                .collect();
            self.offset = 50;
        } else {
            let results = app_state.history.blocking_search(
                &search_query,
                self.is_regex_mode,
                50,
                self.is_case_sensitive,
            );
            self.items = results;
            self.offset = 50;
        }
        self.selected_index = 0;
        cx.notify();
    }
}

impl Focusable for PasteWindow {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PasteWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.needs_load_more {
            self.needs_load_more = false;
            let search_query = self.search_input.read(cx).text(cx);
            if search_query.is_empty() {
                let app_state = cx.global::<GlobalAppState>();
                let new_items: Vec<SearchResult> = app_state.history.get_recent_sync(self.offset, 50)
                    .into_iter()
                    .map(|item| SearchResult { item, match_range: None })
                    .collect();
                if !new_items.is_empty() {
                    self.items.extend(new_items);
                    self.offset += 50;
                }
            }
        }

        let filtered_items = self.items.clone();

        let mut items: Vec<Stateful<Div>> = Vec::new();
        for (i, result) in filtered_items.iter().enumerate() {
            items.push(self.render_item(i, &result.item, cx));
        }

        // Pin button state
        let is_pinned = crate::ui::PINNED.load(std::sync::atomic::Ordering::SeqCst);

        // Regex button state
        let is_regex_mode = self.is_regex_mode;
        let is_case_sensitive = self.is_case_sensitive;

        // Case sensitive button - toggles case sensitive search
        let case_button = div()
            .flex_none()
            .size(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .id("case_button")
            .on_click(cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
                this.is_case_sensitive = !this.is_case_sensitive;
                Self::persist_search_modes(this.is_regex_mode, this.is_case_sensitive);
                // 切换大小写敏感时重新搜索
                let app_state = cx.global::<GlobalAppState>();
                let search_query = this.search_input.read(cx).text(cx);
                if !search_query.is_empty() {
                    let results = app_state.history.blocking_search(&search_query, this.is_regex_mode, 50, this.is_case_sensitive);
                    this.items = results;
                    this.offset = 50;
                }
                this.selected_index = 0;
                cx.notify();
            }))
            .child(
                Icon::from_path("icons/case_sensitive.svg")
                    .size(IconSize::Small)
                    .color(if is_case_sensitive { Color::Info } else { Color::Muted })
            );

        // Regex button - toggles regex search mode
        let regex_button = div()
            .flex_none()
            .size(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .id("regex_button")
            .on_click(cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
                this.is_regex_mode = !this.is_regex_mode;
                Self::persist_search_modes(this.is_regex_mode, this.is_case_sensitive);
                // 切换正则模式时重新搜索
                let app_state = cx.global::<GlobalAppState>();
                let search_query = this.search_input.read(cx).text(cx);
                if search_query.is_empty() {
                    this.items = app_state.history.get_recent_sync(0, 50)
                        .into_iter()
                        .map(|item| SearchResult { item, match_range: None })
                        .collect();
                    this.offset = 50;
                } else {
                    let results = app_state.history.blocking_search(&search_query, this.is_regex_mode, 50, this.is_case_sensitive);
                    this.items = results;
                    this.offset = 50;
                }
                this.selected_index = 0;
                cx.notify();
            }))
            .child(
                Icon::new(IconName::Regex)
                    .size(IconSize::Small)
                    .color(if is_regex_mode { Color::Info } else { Color::Muted })
            );

        // Pin button - toggles pinned state
        let pin_button = div()
            .flex_none()
            .size(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .id("pin_button")
            .on_click(cx.listener(move |_this, _event: &gpui::ClickEvent, _window, cx| {
                crate::ui::PINNED.store(!crate::ui::PINNED.load(std::sync::atomic::Ordering::SeqCst), std::sync::atomic::Ordering::SeqCst);
                cx.notify();
            }))
            .child(
                Icon::new(IconName::Pin)
                    .size(IconSize::Small)
                    .color(if is_pinned { Color::Custom(rgb(0x007aff).into()) } else { Color::Muted })
            );

        let highlight = self.search_highlight;

        let search_icon = div()
            .flex_none()
            .size(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(16.0))
                    .when(highlight > 0.001, |this| this.shadow(focus_mode_glow(highlight)))
                    .child(
                        Icon::new(IconName::MagnifyingGlass)
                            .size(IconSize::Small)
                            .color(Color::Muted)
                    )
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .opacity(highlight)
                            .child(
                                Icon::new(IconName::MagnifyingGlass)
                                    .size(IconSize::Small)
                                    .color(Color::Custom(rgb(0x007aff).into()))
                            )
                    )
            );

        let err_anim = self.ghost_error_animation;
        let is_error = err_anim > 0.0;
        let wiggle_x = if is_error {
            (err_anim * std::f32::consts::PI * 8.0).sin() * 3.0 * (1.0 - err_anim)
        } else {
            0.0
        };

        let app_icon = div()
            .flex_none()
            .size(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(16.0))
                    .left(px(wiggle_x))
                    .when(highlight > 0.001, |this| this.shadow(focus_mode_glow(highlight)))
                    .child(
                        Icon::from_path(FLYPASTE_ICON_PATH)
                            .size(IconSize::Small)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .opacity(highlight)
                            .child(
                                Icon::from_path(FLYPASTE_ICON_PATH)
                                    .size(IconSize::Small)
                                    .color(Color::Custom(rgb(0x007aff).into())),
                            ),
                    )
                    .when(is_error, |this| {
                        this.child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .opacity(1.0 - err_anim)
                                .child(
                                    Icon::from_path(FLYPASTE_ICON_PATH)
                                        .size(IconSize::Small)
                                        .color(Color::Custom(rgb(0xff3b30).into())),
                                ),
                        )
                    }),
            );

        // Header with search input - macOS native style
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .px_3()
            .py_2()
            .gap_2()
            .rounded_t_md()
            .child(app_icon)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .flex_grow()
                    .gap_2()
                    .cursor_text()
                    .id("search_area")
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                        this.focus_search_input(window, cx);
                    }))
                    .child(search_icon)
                    .child(div().flex_grow().child(self.search_input.clone()))
            )
            .child(case_button)
            .child(regex_button)
            .child(pin_button);

        // History list with scroll tracking for lazy loading
        let scroll_handle = self.scroll_handle.clone();
        let list = div()
            .flex()
            .flex_col()
            .id("history_list")
            .overflow_y_scroll()
            .track_scroll(&self.scroll_handle)
            .on_scroll_wheel(cx.listener(move |this, _event: &ScrollWheelEvent, _window, cx| {
                let offset = scroll_handle.offset();
                let max_offset = scroll_handle.max_offset();

                // Only trigger lazy load in non-search mode
                let search_query = this.search_input.read(cx).text(cx);
                if search_query.is_empty()
                    && max_offset.y > px(0.0)
                    && offset.y <= -max_offset.y + px(100.0)
                {
                    this.load_more(cx);
                }
            }));

        let (browse_opacity, search_opacity) = crate::ui::panel_opacity();
        let search_opacity = search_opacity.max(browse_opacity + 0.001).min(1.0);
        let current_opacity = browse_opacity + (search_opacity - browse_opacity) * highlight;
        
        // Tinted glass effect.
        //
        // GPUI's macOS Blurred background removes the OS default saturation/tint layers.
        // Therefore, we must provide a substantial color tint ourselves, or else a white
        // desktop background will completely wash out the dark mode window.
        //
        // We remap the user's opacity (0.0 to 1.0) to a highly visible alpha range:
        // MIN_TINT ensures that even at "100% transparency", the glass is still strongly
        // colored (e.g. 65% black in dark mode) so it looks like dark glass, not clear glass.
        const MIN_TINT: f32 = 0.65; 
        const MAX_TINT: f32 = 0.95; 
        let tint_alpha = MIN_TINT + current_opacity * (MAX_TINT - MIN_TINT);
        let alpha_byte = (tint_alpha * 255.0) as u8;

        let is_dark = cx.theme().appearance == theme::Appearance::Dark;
        let container_bg = if is_dark {
            rgba(u32::from_be_bytes([0x00, 0x00, 0x00, alpha_byte]))
        } else {
            rgba(u32::from_be_bytes([0xFF, 0xFF, 0xFF, alpha_byte]))
        };

        // Main container - macOS native style
        // size_full() ensures the container fills the entire window bounds, covering the
        // NSVisualEffectView perfectly to avoid white bars.
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(container_bg)
            .text_color(cx.theme().colors().text)
            .id("main_container")
            .child(header)
            .child(list.children(items))
    }
}

impl PasteWindow {
    fn render_item(&mut self, index: usize, item: &ClipboardItem, cx: &mut Context<Self>) -> Stateful<Div> {
        // Shortcut label at the end with ⌥ prefix
        let shortcut_label = if index < 9 {
            Some(format!("\u{2325} {}", index + 1))  // ⌥1, ⌥2, etc.
        } else {
            None
        };

        // Replace newlines with ↵ symbol for single-line display
        let preview_text = item.text_preview()
            .replace('\n', "↵ ")
            .replace('\r', "");

        let is_selected = index == self.selected_index;
        let is_image = item.content_type == "image";

        // Clone the id for the click handler
        let item_id = item.id;

        let item_div = div()
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .h(px(50.0))  // Fixed height for all items
            .flex_none()  // Prevent stretching when items count is less than one page
            .px_3()
            .gap_2()
            .cursor_pointer()
            .id(format!("paste_item_{}", index))
            .on_click(cx.listener(move |_this, _event: &gpui::ClickEvent, _window, _cx| {
                let in_focus_mode = crate::ui::is_in_focus_mode_for_paste();
                if let Some(tx) = crate::ui::HOTKEY_TX.get() {
                    let _ = tx.try_send(crate::ui::HotkeyEvent::PasteItem {
                        item_id,
                        in_focus_mode,
                    });
                }
            }))
            .on_mouse_move(cx.listener(move |this, _event, _window, cx| {
                // Only update if actually changed to avoid unnecessary re-renders
                if this.selected_index != index {
                    this.selected_index = index;
                    cx.notify();
                }
                this.hovered_index = Some(index);
                
                // Sync status to TextInput for interception
                this.search_input.update(cx, |input, cx| {
                    input.set_hovering_item(true, cx);
                });
            }))
            .on_hover(cx.listener(move |this, hovering: &bool, _window, cx| {
                if *hovering {
                    this.hovered_index = Some(index);
                } else if this.hovered_index == Some(index) {
                    this.hovered_index = None;
                }
                
                // Sync status to TextInput for interception
                let is_hovering = this.hovered_index.is_some();
                this.search_input.update(cx, |input, cx| {
                    input.set_hovering_item(is_hovering, cx);
                });
                
                cx.notify();
            }));

        let is_copy_flash = self.copy_flash_item_id == Some(item.id);

        let item_div = if is_copy_flash {
            item_div
                .bg(rgba(0x34C75928))
                .rounded_sm()
        } else if is_selected {
            item_div
                .bg(rgba(0x007AFF26))  // 0x26 ≈ 15% opacity
                .rounded_sm()
        } else {
            // Hover effect same as selected (blue background)
            item_div.hover(|div| div.bg(rgba(0x007AFF26)))
        };

        // Shortcut badge - fixed 31px width, doesn't shrink, stays on right
        let badge = div()
            .when(shortcut_label.is_some(), |div| {
                div.flex_none()
                    .items_center()
                    .justify_center()
                    .w(px(31.0))  // Fixed width
                    .px_1()
                    .rounded_sm()
                    .text_xs()
                    .text_color(cx.theme().colors().text_muted)
                    .child(shortcut_label.unwrap_or_default())
            });

        // App icon on the left - show app icon if available, otherwise show default icon
        let app_icon: gpui::Div = if let Some(ref bundle_id) = item.source_app_bundle_id {
            if let Some(png_data) = get_app_icon_image(bundle_id, cx) {
                let icon_img = create_icon_from_png(&png_data);
                div().flex_none().size(px(24.0)).child(icon_img)
            } else {
                // bundle_id exists but icon failed to load, show default icon
                if let Some(default_icon) = get_default_app_icon() {
                    div().flex_none().size(px(24.0)).child(create_icon_from_png(&default_icon))
                } else {
                    div().flex_none().size(px(24.0))
                }
            }
        } else {
            // No bundle_id, show default icon
            if let Some(default_icon) = get_default_app_icon() {
                div().flex_none().size(px(24.0)).child(create_icon_from_png(&default_icon))
            } else {
                div().flex_none().size(px(24.0))
            }
        };

        if is_image {
            // 使用缓存的图片
            let (thumbnail_img, thumb_width, thumb_height) = if let Some(cached) = self.thumbnail_cache.get(&item.id) {
                cached.clone()
            } else if let Some(thumb) = item.thumbnail() {
                let img = Image::from_bytes(ImageFormat::Png, thumb.data.as_ref().clone());
                let image = Arc::new(img);
                let dims = (image.clone(), thumb.width, thumb.height);
                self.thumbnail_cache.insert(item.id, dims.clone());
                dims
            } else {
                // 创建占位图
                let placeholder: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
                let img = Arc::new(Image::from_bytes(ImageFormat::Png, placeholder));
                (img, 48u32, 48u32)
            };

            // Thumbnail in middle, badge fixed to right
            let thumbnail = create_image_from_arc(thumbnail_img.clone(), thumb_width, thumb_height);

            // Layout: [app icon] [thumbnail] [badge]
            item_div.child(app_icon).child(thumbnail).child(badge)
        } else {
            // Text preview - single line with ellipsis for overflow
            let font = text_font();

            let text_preview = div()
                .flex_1()
                .text_sm()
                .font(font)
                .text_color(cx.theme().colors().text)
                .overflow_hidden()
                .text_ellipsis()
                .child(preview_text);

            // Layout: [app icon] [text] [badge]
            item_div.child(app_icon).child(text_preview).child(badge)
        }
    }
}

/// Create image element from cached Arc<Image>
fn create_image_from_arc(image: Arc<Image>, width: u32, height: u32) -> impl IntoElement {
    // Content area dimensions
    const MAX_WIDTH: f32 = 330.0;
    const MAX_HEIGHT: f32 = 48.0;

    // Contain logic: calculate scaled dimensions to fit within MAX_WIDTH x MAX_HEIGHT
    // while maintaining aspect ratio
    let (scaled_width, scaled_height) = if width == 0 || height == 0 {
        (MAX_WIDTH, MAX_HEIGHT)
    } else {
        let orig_ratio = width as f32 / height as f32;
        let (w, h) = if orig_ratio > MAX_WIDTH / MAX_HEIGHT {
            // Image is wider relative to container
            (MAX_WIDTH, MAX_WIDTH / orig_ratio)
        } else {
            // Image is taller relative to container
            (MAX_HEIGHT * orig_ratio, MAX_HEIGHT)
        };
        // If image is smaller than container, don't scale up
        (w.min(width as f32), h.min(height as f32))
    };

    let image_source = ImageSource::Image(image);

    // Container: flex_1 to fill remaining space, fixed 50px height
    div()
        .flex_1()  // Fill remaining horizontal space
        .h(px(50.0))  // Fixed height
        .p(px(1.0))  // 1px padding
        .overflow_hidden()  // Clip overflow
        // Wrapper div needed because img with fixed size isn't a normal flex child
        .child(
            div()
                .flex()
                .items_center()  // Center vertically
                .justify_start()  // Left align horizontally
                .size_full()
                .child(
                    img(image_source)
                        .h(px(scaled_height as f32))
                        .w(px(scaled_width as f32))
                        .object_fit(gpui::ObjectFit::Contain)
                )
        )
}

/// Create an app icon element from PNG bytes
/// Icon size: 24x24 pixels
fn create_icon_from_png(png_data: &Arc<Vec<u8>>) -> impl IntoElement {
    let image = Image::from_bytes(ImageFormat::Png, (*png_data).to_vec());
    let image_source = ImageSource::Image(std::sync::Arc::new(image));

    div()
        .flex_none()
        .size(px(24.0))
        .overflow_hidden()
        .child(
            img(image_source)
                .size_full()
                .object_fit(gpui::ObjectFit::Contain)
        )
}

/// Get the default "unknown app" icon as PNG bytes
fn get_default_app_icon() -> Option<Arc<Vec<u8>>> {
    Assets::get("icons/unknown_app.png").map(|f| Arc::new(f.data.to_vec()))
}

/// Get app icon as PNG data from bundle ID (uses shared caching from mod.rs)
#[cfg(target_os = "macos")]
fn get_app_icon_image(bundle_id: &str, cx: &mut App) -> Option<Arc<Vec<u8>>> {
    crate::ui::load_icon_async(bundle_id, cx)
}

#[cfg(not(target_os = "macos"))]
fn get_app_icon_image(_bundle_id: &str, cx: &mut App) -> Option<Arc<Vec<u8>>> {
    None
}

/// 统一的文字颜色
/// 统一字体
fn text_font() -> gpui::Font {
    gpui::Font {
        family: "PingFang SC".into(),
        ..Default::default()
    }
}

/// Preview content types
pub enum PreviewContent {
    Image { png_data: Vec<u8>, width: u32, height: u32 },
    Text { content: String, match_range: Option<(usize, usize)> },
}

impl PreviewContent {
    fn from_clipboard_item(item: &ClipboardItem, match_range: Option<(usize, usize)>) -> Self {
        match &item.content_type[..] {
            "image" => {
                let path = image_path_for_hash(
                    &history::data_dir().join("images"),
                    &item.content,
                );
                if let Ok(img) = image::open(&path) {
                    let (width, height) = img.dimensions();
                    // 重新编码为 PNG（保持原始质量）
                    let mut png_data = Vec::new();
                    let mut cursor = std::io::Cursor::new(&mut png_data);
                    if img.write_to(&mut cursor, image::ImageFormat::Png).is_ok() {
                        return PreviewContent::Image {
                            png_data,
                            width,
                            height,
                        };
                    }
                }
                // fallback: 返回空图片
                PreviewContent::Image {
                    png_data: Vec::new(),
                    width: 0,
                    height: 0,
                }
            }
            _ => {
                let valid_range = match_range.filter(|(start, end)| {
                    let byte_len = item.content.len();
                    *start < byte_len && *end <= byte_len && *start < *end &&
                        item.content.is_char_boundary(*start) && item.content.is_char_boundary(*end)
                });
                PreviewContent::Text { content: item.content.clone(), match_range: valid_range }
            }
        }
    }

    /// Convert byte offsets to character offsets for UTF-8 strings.
    /// NOTE: with_highlights expects byte offsets, not character offsets!
    #[allow(dead_code)]
    fn byte_to_char_range(content: &str, start_byte: usize, end_byte: usize) -> Option<(usize, usize)> {
        let byte_len = content.len();
        if start_byte >= byte_len || end_byte > byte_len || start_byte >= end_byte {
            return None;
        }
        if !content.is_char_boundary(start_byte) || !content.is_char_boundary(end_byte) {
            return None;
        }
        let start_char = content[..start_byte].chars().count();
        let end_char = content[..end_byte].chars().count();
        Some((start_char, end_char))
    }
}

/// Preview window that shows full content
pub struct PreviewWindow {
    content: PreviewContent,
    focus_handle: FocusHandle,
}

impl PreviewWindow {
    pub fn new(content: PreviewContent, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();

        cx.on_release(|_this, _cx| {
            crate::ui::on_preview_window_released();
        })
        .detach();

        // When preview window loses focus, close it and restore search focus
        cx.observe_window_activation(window, |_this, window, _cx| {
            if !window.is_window_active() {
                crate::ui::close_preview_from_window(window);
            }
        })
        .detach();

        Self { content, focus_handle }
    }
}

impl Focusable for PreviewWindow {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PreviewWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_image = matches!(self.content, PreviewContent::Image { .. });

        // 获取窗口尺寸用于图片自适应
        let window_bounds = window.bounds();
        let window_width = window_bounds.size.width.as_f32();
        let window_height = window_bounds.size.height.as_f32();

        div()
            .flex()
            .size_full()
            .key_context("PreviewWindow")
            .track_focus(&self.focus_handle(cx))
            .bg(if is_image { rgba(0x1C1C1E) } else { rgba(0x2C2C2E) })
            .items_center()
            .justify_center()
            .on_key_down(cx.listener(move |_this, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                let cmd = event.keystroke.modifiers.platform;
                if (cmd && key == "w") || key == "space" {
                    crate::ui::close_preview_from_window(window);
                    cx.stop_propagation();
                }
            }))
            .child(match &self.content {
                PreviewContent::Image { png_data, width: _, height: _ } => {
                    // 使用预编码的 PNG 数据
                    let image = Image::from_bytes(ImageFormat::Png, png_data.clone());
                    let source = ImageSource::Image(std::sync::Arc::new(image));

                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .size_full()
                        .child(
                            img(source)
                                .w(px(window_width))
                                .h(px(window_height))
                                .object_fit(gpui::ObjectFit::Contain)
                        )
                        .into_any()
                }
                PreviewContent::Text { content, match_range } => {
                    let child = if let Some(&(start, end)) = match_range.as_ref() {
                        let styled = StyledText::new(content.clone())
                            .with_highlights([(start..end, HighlightStyle { font_weight: Some(gpui::FontWeight(700.0)), ..Default::default() })]);
                        div().child(styled).into_any()
                    } else {
                        div().child(content.clone()).into_any()
                    };

                    div()
                        .flex()
                        .flex_col()
                        .id("text_preview")
                        .size_full()
                        .bg(cx.theme().colors().elevated_surface_background)
                        .rounded_lg()
                        .p_6()
                        .overflow_y_scroll()
                        .child(child)
                        .into_any()
                }
            }).into_any()
    }
}


/// Create window options for preview window
fn make_preview_window_options(content: &PreviewContent) -> WindowOptions {
    use crate::ui::get_cursor_position;
    use crate::ui::get_display_bounds_for_point;

    let (cursor_x, cursor_y) = get_cursor_position();
    let display_bounds = get_display_bounds_for_point(cursor_x as f64, cursor_y as f64);

    // Calculate window size based on content
    let screen_width = display_bounds.size.width as f32;
    let screen_height = display_bounds.size.height as f32;
    let max_width = (screen_width * 0.9).min(1200.0);
    let max_height = (screen_height * 0.9).min(800.0);

    let (window_width, window_height) = match content {
        PreviewContent::Image { width, height, .. } => {
            // Use image dimensions, capped to screen size
            let w = (*width as f32).min(max_width);
            let h = (*height as f32).min(max_height);
            // If image is smaller than defaults, use image size
            (w.max(300.0), h.max(200.0))
        }
        PreviewContent::Text { content, .. } => {
            // Estimate text size based on content
            let char_count = content.chars().count();
            let lines = char_count / 60 + 1;
            let estimated_height = (lines as f32 * 24.0).min(max_height);
            (600.0_f32, estimated_height.max(150.0))
        }
    };

    let center_x = display_bounds.origin.x as f32 + (screen_width - window_width) / 2.0;
    let center_y = display_bounds.origin.y as f32 + (screen_height - window_height) / 2.0;

    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: gpui::point(px(center_x), px(center_y)),
            size: gpui::size(px(window_width), px(window_height)),
        })),
        titlebar: None,
        focus: true,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: true,
        is_resizable: true,
        is_minimizable: true,
        display_id: None,
        window_background: WindowBackgroundAppearance::Blurred,
        app_id: Some("flypaste".to_string()),
        window_min_size: Some(gpui::size(px(200.0), px(150.0))),
        window_decorations: Some(WindowDecorations::Client),
        tabbing_identifier: None,
    }
}
