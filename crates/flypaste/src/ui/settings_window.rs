#![allow(unexpected_cfgs)]

use gpui::*;
use crate::autolaunch;
use crate::i18n;
use crate::statusbar;
use crate::ui::number_field::{NumberField, NumberFieldMode};
use crate::ui::opacity_slider::{opacity_slider, BrowsePanelOpacityDrag, SearchPanelOpacityDrag};
use crate::ui::{GlobalAppState, refresh_paste_window, update_global_hotkey};
use fly_settings::{Language, Settings, Hotkey};
use std::rc::Rc;
use ui::{prelude::*, Divider, Switch, Color, Label, SpinnerLabel, ToggleState, Button, ButtonStyle};
use crate::ui::markdown_renderer::MarkdownRenderer;

const INDEX_REBUILD_MIN_LOADING: std::time::Duration = std::time::Duration::from_millis(800);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RecordingTarget {
    Activation,
    Regex,
    CaseSensitive,
    FocusSearch,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Shortcuts,
    Storage,
    Tutorial,
    Faq,
}

pub struct SettingsWindow {
    focus_handle: FocusHandle,
    scroll_handle: ScrollHandle,
    recording_target: Option<RecordingTarget>,
    rebuilding_index: bool,
    cleaning_up: bool,
    clear_confirm_open: bool,
    active_tab: SettingsTab,
}

impl SettingsWindow {
    pub fn new(window: &mut gpui::Window, cx: &mut Context<Self>) -> Self {
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
            crate::ui::on_settings_window_released();
        })
        .detach();

        Self {
            focus_handle,
            scroll_handle: ScrollHandle::new(),
            recording_target: None,
            rebuilding_index: false,
            cleaning_up: false,
            clear_confirm_open: false,
            active_tab: SettingsTab::General,
        }
    }

    fn confirm_clear_history(&mut self, cx: &mut Context<Self>) {
        self.clear_confirm_open = false;
        cx.notify();

        let history = cx.global::<GlobalAppState>().history.clone();

        cx.spawn(async move |_, cx| {
            cx.background_executor()
                .spawn(async move {
                    history.wipe_all_data();
                })
                .await;

            let _ = cx.update(|cx| {
                cx.quit();
            });
        })
        .detach();
    }

    fn render_dialog_button(
        id: &'static str,
        label: &str,
        destructive: bool,
        on_click: impl Fn(&mut SettingsWindow, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let color = if destructive {
            Color::Error
        } else {
            Color::Default
        };

        Button::new(id, label.to_string())
            .color(color)
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| on_click(this, cx)))
    }

    fn render_clear_confirm_dialog(
        &self,
        t: &'static i18n::Strings,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(cx.theme().colors().elevated_surface_background.opacity(0.8))
            .occlude()
            .child(
                div()
                    .id("clear_history_confirm")
                    .w(px(400.0))
                    .bg(cx.theme().colors().elevated_surface_background)
                    .border_1()
                    .border_color(cx.theme().colors().border)
                    .rounded_lg()
                    .shadow_md()
                    .overflow_hidden()
                    .child(
                        v_flex()
                            .p_4()
                            .gap_3()
                            .child(
                                Label::new(t.clear_confirm_title.to_string())
                                    .size(LabelSize::Large)
                                    .color(Color::Default),
                            )
                            .child(
                                Label::new(t.clear_confirm_message.to_string())
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            )
                            .child(
                                h_flex()
                                    .justify_end()
                                    .gap_2()
                                    .child(Self::render_dialog_button(
                                        "clear_history_cancel",
                                        t.clear_confirm_cancel,
                                        false,
                                        |this, cx| {
                                            this.clear_confirm_open = false;
                                            cx.notify();
                                        },
                                        cx,
                                    ))
                                    .child(Self::render_dialog_button(
                                        "clear_history_confirm",
                                        t.clear_confirm_primary,
                                        true,
                                        |this, cx| this.confirm_clear_history(cx),
                                        cx,
                                    )),
                            ),
                    ),
            )
    }

    fn start_rebuild_index(&mut self, cx: &mut Context<Self>) {
        if self.rebuilding_index {
            return;
        }

        self.rebuilding_index = true;
        cx.notify();

        let history = cx.global::<GlobalAppState>().history.clone();
        let handle = cx.entity();

        cx.spawn(async move |_, cx| {
            let started = std::time::Instant::now();

            cx.background_executor()
                .spawn(async move {
                    history.rebuild_index();
                })
                .await;

            let elapsed = started.elapsed();
            if elapsed < INDEX_REBUILD_MIN_LOADING {
                cx.background_executor()
                    .timer(INDEX_REBUILD_MIN_LOADING - elapsed)
                    .await;
            }

            let _ = cx.update_entity(&handle, |this, cx| {
                this.rebuilding_index = false;
                cx.notify();
            });
        })
        .detach();
    }

    fn start_cleanup(&mut self, cx: &mut Context<Self>) {
        if self.cleaning_up {
            return;
        }

        self.cleaning_up = true;
        cx.notify();

        let history = cx.global::<GlobalAppState>().history.clone();
        let handle = cx.entity();
        let settings = fly_settings::Settings::load().unwrap_or_default();

        cx.spawn(async move |_, cx| {
            let started = std::time::Instant::now();

            cx.background_executor()
                .spawn(async move {
                    history.cleanup_by_duration(settings.text_retention_days, settings.image_retention_days).await;
                })
                .await;

            let elapsed = started.elapsed();
            if elapsed < INDEX_REBUILD_MIN_LOADING {
                cx.background_executor()
                    .timer(INDEX_REBUILD_MIN_LOADING - elapsed)
                    .await;
            }

            let _ = cx.update_entity(&handle, |this, cx| {
                this.cleaning_up = false;
                cx.notify();
            });
        })
        .detach();
    }

    fn update_settings<F>(&self, cx: &mut Context<Self>, f: F)
    where
        F: FnOnce(&mut Settings),
    {
        let mut settings = fly_settings::Settings::load().unwrap_or_default();
        f(&mut settings);
        let _ = settings.save();
        cx.notify();
    }

    fn render_sidebar_nav_item(
        &self,
        tab: SettingsTab,
        icon: ui::IconName,
        label: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_active = self.active_tab == tab;
        
        h_flex()
            .id(SharedString::from(format!("tab_{}", label)))
            .items_center()
            .gap_2()
            .cursor_pointer()
            .px_3()
            .py_1p5()
            .rounded_md()
            .when(is_active, |this| {
                this.bg(cx.theme().colors().element_selected)
            })
            .hover(|this| {
                if !is_active {
                    this.bg(cx.theme().colors().element_hover)
                } else {
                    this
                }
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.active_tab = tab;
                cx.notify();
            }))
            .child(
                ui::Icon::new(icon)
                    .color(if is_active { ui::Color::Default } else { ui::Color::Muted })
                    .size(ui::IconSize::Small)
            )
            .child(Label::new(label.to_string())
                .color(if is_active { Color::Default } else { Color::Muted }))
    }

    fn render_sidebar(&self, t: &'static i18n::Strings, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .w_40()
            .h_full()
            .p_2p5()
            .when(cfg!(target_os = "macos"), |this| this.pt_10())
            .flex_none()
            .border_r_1()
            .border_color(cx.theme().colors().border)
            .bg(cx.theme().colors().panel_background)
            .child(
                v_flex()
                    .w_full()
                    .gap_1()
                    .child(self.render_sidebar_nav_item(SettingsTab::General, ui::IconName::Settings, t.section_general, cx))
                    .child(self.render_sidebar_nav_item(SettingsTab::Shortcuts, ui::IconName::Keyboard, t.section_shortcuts, cx))
                    .child(self.render_sidebar_nav_item(SettingsTab::Storage, ui::IconName::Archive, t.section_storage, cx))
                    .child(div().h_4()) // Separator
                    .child(Self::section_header(t.section_help))
                    .child(self.render_sidebar_nav_item(SettingsTab::Tutorial, ui::IconName::Book, t.section_tutorial, cx))
                    .child(self.render_sidebar_nav_item(SettingsTab::Faq, ui::IconName::CircleHelp, t.section_faq, cx))
            )
    }

    fn section_header(title: &str) -> impl IntoElement {
        div()
            .pt_1()
            .pb_1()
            .pl_2()
            .child(
                Label::new(title.to_string())
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
    }

    fn section_footer(text: &str) -> impl IntoElement {
        div()
            .pt_1()
            .pb_2()
            .pl_2()
            .pr_2()
            .child(
                Label::new(text.to_string())
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
    }

    fn render_hotkey_button(
        &self,
        target: RecordingTarget,
        hotkey: &Hotkey,
        recording_label: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_recording = self.recording_target == Some(target);
        let id = match target {
            RecordingTarget::Activation => "record_activation",
            RecordingTarget::Regex => "record_regex",
            RecordingTarget::CaseSensitive => "record_case",
            RecordingTarget::FocusSearch => "record_focus_search",
        };

        Button::new(id, if is_recording { recording_label.to_string() } else { hotkey.display() })
            .style(if is_recording { ButtonStyle::Filled } else { ButtonStyle::Subtle })
            .color(if is_recording { Color::Accent } else { Color::Default })
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                if this.recording_target == Some(target) {
                    this.recording_target = None;
                } else {
                    this.recording_target = Some(target);
                }
                cx.notify();
            }))
    }

    fn render_labeled_row(label: &str, control: impl IntoElement) -> impl IntoElement {
        div()
            .flex()
            .justify_between()
            .items_center()
            .px_4()
            .py_2p5()
            .child(Label::new(label.to_string()).color(Color::Default))
            .child(control)
    }



    fn render_loading_value_row(label: &str, value: &str, loading: bool) -> impl IntoElement {
        div()
            .flex()
            .justify_between()
            .items_center()
            .px_4()
            .py_2p5()
            .child(Label::new(label.to_string()).color(Color::Default))
            .child(if loading {
                SpinnerLabel::sand()
                    .size(LabelSize::Small)
                    .color(Color::Muted)
                    .into_any_element()
            } else {
                Label::new(value.to_string())
                    .size(LabelSize::Small)
                    .color(Color::Muted)
                    .into_any_element()
            })
    }

    fn render_action_row(
        label: &str,
        color: Color,
        id: &'static str,
        round_bottom: bool,
        on_click: impl Fn(&mut SettingsWindow, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .cursor_pointer()
            .flex()
            .items_center()
            .px_4()
            .py_2p5()
            .when(round_bottom, |this| this.rounded_b_lg())
            .hover(|style| style.bg(cx.theme().colors().element_hover))
            .on_click(cx.listener(move |this, _, _, cx| on_click(this, cx)))
            .child(Label::new(label.to_string()).color(color))
    }

    fn render_language_picker(
        &self,
        current: Language,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let languages = fly_settings::Language::all();
        let len = languages.len();

        h_flex()
            .border_1()
            .border_color(cx.theme().colors().border)
            .rounded_md()
            .overflow_hidden()
            .children(languages.into_iter().enumerate().map(|(i, language)| {
                let selected = language == current;
                let id = match language {
                    Language::Zh => "lang_zh",
                    Language::En => "lang_en",
                };

                let mut el = div()
                    .id(id)
                    .cursor_pointer()
                    .px_4()
                    .py_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(selected, |this| {
                        this.bg(cx.theme().colors().element_selected)
                    })
                    .when(!selected, |this| {
                        this.hover(|style| style.bg(cx.theme().colors().element_hover))
                    })
                    .child(
                        Label::new(language.display_name().to_string())
                            .color(if selected { Color::Default } else { Color::Muted })
                    )
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        if current == language {
                            return;
                        }
                        this.update_settings(cx, |s| s.language = language);
                        statusbar::refresh_menu_language();
                        cx.notify();
                    }));

                if i < len - 1 {
                    el = el.border_r_1().border_color(cx.theme().colors().border);
                }

                el.into_any_element()
            }))
    }

    fn render_theme_picker(
        &self,
        current: fly_settings::Theme,
        t: &'static i18n::Strings,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let themes = fly_settings::Theme::all();
        let len = themes.len();

        h_flex()
            .border_1()
            .border_color(cx.theme().colors().border)
            .rounded_md()
            .overflow_hidden()
            .children(themes.into_iter().enumerate().map(|(i, theme)| {
                let selected = theme == current;
                let id = match theme {
                    fly_settings::Theme::Auto => "theme_auto",
                    fly_settings::Theme::Light => "theme_light",
                    fly_settings::Theme::Dark => "theme_dark",
                };

                let display_name = match theme {
                    fly_settings::Theme::Auto => t.theme_auto,
                    fly_settings::Theme::Light => t.theme_light,
                    fly_settings::Theme::Dark => t.theme_dark,
                };

                let mut el = div()
                    .id(id)
                    .cursor_pointer()
                    .px_4()
                    .py_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(selected, |this| {
                        this.bg(cx.theme().colors().element_selected)
                    })
                    .when(!selected, |this| {
                        this.hover(|style| style.bg(cx.theme().colors().element_hover))
                    })
                    .child(
                        Label::new(display_name.to_string())
                            .color(if selected { Color::Default } else { Color::Muted })
                    )
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                        if current == theme {
                            return;
                        }
                        let appearance = window.appearance();
                        this.update_settings(cx, |s| s.theme = theme);
                        crate::ui::apply_theme(appearance, cx);
                        refresh_paste_window(cx);
                        cx.notify();
                    }));

                if i < len - 1 {
                    el = el.border_r_1().border_color(cx.theme().colors().border);
                }

                el.into_any_element()
            }))
    }

    fn render_general_tab(&mut self, t: &'static i18n::Strings, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = fly_settings::Settings::load().unwrap_or_default();
        let entity = cx.entity().clone();
        let entity_id = cx.entity_id();
        let browse_transparency = 1.0 - settings.browse_panel_opacity;
        let search_transparency = 1.0 - settings.search_panel_opacity;

        v_flex()
            .gap_6()
            .child(
                v_flex()
                    .gap_1()
                    .child(Self::section_header(t.auto_launch))
                    .child(
                        div()
                            .border_1()
                            .border_color(cx.theme().colors().border)
                            .bg(cx.theme().colors().elevated_surface_background)
                            .rounded_lg()
                            .child(Self::render_labeled_row(
                                t.auto_launch,
                                Switch::new("auto_launch", if settings.auto_launch { ToggleState::Selected } else { ToggleState::Unselected })
                                    .on_click(move |_, _, cx| {
                                        let current = fly_settings::Settings::load()
                                            .unwrap_or_default()
                                            .auto_launch;
                                        let new_value = !current;

                                        let mut settings = fly_settings::Settings::load().unwrap_or_default();
                                        settings.auto_launch = new_value;
                                        let _ = settings.save();
                                        
                                        let _ = cx.update_entity(&entity, |_this: &mut SettingsWindow, cx| {
                                            cx.notify();
                                        });

                                        cx.background_executor().spawn(async move {
                                            if let Err(error) = autolaunch::set_enabled(new_value) {
                                                log::error!("Failed to set auto launch: {error}");
                                                if error.kind() == std::io::ErrorKind::PermissionDenied {
                                                    autolaunch::open_login_items_settings();
                                                }
                                            }
                                        }).detach();
                                    })
                            ))
                    )
                    .child(Self::section_footer(t.general_footer))
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(Self::section_header(t.section_language))
                    .child(
                        div()
                            .border_1()
                            .border_color(cx.theme().colors().border)
                            .bg(cx.theme().colors().elevated_surface_background)
                            .rounded_lg()
                            .child(Self::render_labeled_row(
                                t.language_row,
                                self.render_language_picker(settings.language, cx),
                            ))
                    )
                    .child(Self::section_footer(t.language_footer))
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(Self::section_header(t.section_appearance))
                    .child(
                        div()
                            .border_1()
                            .border_color(cx.theme().colors().border)
                            .bg(cx.theme().colors().elevated_surface_background)
                            .rounded_lg()
                            .child(Self::render_labeled_row(
                                t.theme_row,
                                self.render_theme_picker(settings.theme, t, cx),
                            ))
                            .child(Divider::horizontal())
                            .child(Self::render_labeled_row(
                                t.browse_panel_opacity,
                                opacity_slider(
                                    "browse_panel_opacity",
                                    browse_transparency,
                                    0.0,
                                    1.0,
                                    BrowsePanelOpacityDrag,
                                    Rc::new({
                                        let entity_id = entity_id;
                                        move |transparency, cx| {
                                            let mut settings =
                                                Settings::load().unwrap_or_default();
                                            settings.set_browse_transparency(transparency);
                                            let _ = settings.save();
                                            crate::ui::sync_panel_opacity_from_settings(
                                                &settings,
                                            );
                                            refresh_paste_window(cx);
                                            cx.notify(entity_id);
                                        }
                                    }),
                                ),
                            ))
                            .child(Divider::horizontal())
                            .child(Self::render_labeled_row(
                                t.search_panel_opacity,
                                opacity_slider(
                                    "search_panel_opacity",
                                    search_transparency,
                                    0.0,
                                    1.0,
                                    SearchPanelOpacityDrag,
                                    Rc::new({
                                        let entity_id = entity_id;
                                        move |transparency, cx| {
                                            let mut settings =
                                                Settings::load().unwrap_or_default();
                                            settings.set_search_transparency(transparency);
                                            let _ = settings.save();
                                            crate::ui::sync_panel_opacity_from_settings(
                                                &settings,
                                            );
                                            refresh_paste_window(cx);
                                            cx.notify(entity_id);
                                        }
                                    }),
                                ),
                            ))
                    )
                    .child(Self::section_footer(t.appearance_footer))
            )
    }

    fn render_shortcuts_tab(&self, t: &'static i18n::Strings, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = fly_settings::Settings::load().unwrap_or_default();
        
        v_flex()
            .gap_6()
            .child(
                v_flex()
                    .gap_1()
                    .child(Self::section_header(t.section_shortcuts))
                    .child(
                        div()
                            .border_1()
                            .border_color(cx.theme().colors().border)
                            .bg(cx.theme().colors().elevated_surface_background)
                            .rounded_lg()
                            .child(Self::render_labeled_row(
                                t.open_history,
                                self.render_hotkey_button(
                                    RecordingTarget::Activation,
                                    &settings.hotkey,
                                    t.press_hotkey,
                                    cx,
                                ),
                            ))
                            .child(Divider::horizontal())
                            .child(Self::render_labeled_row(
                                t.regex_search,
                                self.render_hotkey_button(
                                    RecordingTarget::Regex,
                                    &settings.regex_hotkey,
                                    t.press_hotkey,
                                    cx,
                                ),
                            ))
                            .child(Divider::horizontal())
                            .child(Self::render_labeled_row(
                                t.case_sensitive,
                                self.render_hotkey_button(
                                    RecordingTarget::CaseSensitive,
                                    &settings.case_sensitive_hotkey,
                                    t.press_hotkey,
                                    cx,
                                ),
                            ))
                            .child(Divider::horizontal())
                            .child(Self::render_labeled_row(
                                t.focus_search,
                                self.render_hotkey_button(
                                    RecordingTarget::FocusSearch,
                                    &settings.focus_hotkey,
                                    t.press_hotkey,
                                    cx,
                                ),
                            ))
                    )
                    .child(Self::section_footer(t.shortcuts_footer))
            )
    }

    fn render_storage_tab(&mut self, t: &'static i18n::Strings, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = fly_settings::Settings::load().unwrap_or_default();
        let entity_id = cx.entity_id();
        let app_state = cx.global::<GlobalAppState>();
        let history = app_state.history.clone();

        let data_size = history.get_db_size() + history.get_images_size();
        let index_size = history.get_index_size();

        fn format_size(bytes: u64) -> String {
            if bytes < 1024 {
                format!("{} B", bytes)
            } else if bytes < 1024 * 1024 {
                format!("{:.2} KB", bytes as f64 / 1024.0)
            } else {
                format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
            }
        }

        v_flex()
            .gap_6()
            .child(
                v_flex()
                    .gap_1()
                    .child(Self::section_header(t.section_retention))
                    .child(
                        div()
                            .border_1()
                            .border_color(cx.theme().colors().border)
                            .bg(cx.theme().colors().elevated_surface_background)
                            .rounded_lg()
                            .child(Self::render_labeled_row(
                                t.text_retention,
                                NumberField::new(
                                    "text_retention_days",
                                    settings.text_retention_days,
                                    window,
                                    cx,
                                )
                                .mode(NumberFieldMode::Edit, cx)
                                .min(1)
                                .max(365)
                                .on_change({
                                    move |value, _, cx| {
                                        let mut settings =
                                            fly_settings::Settings::load()
                                                .unwrap_or_default();
                                        settings.text_retention_days = *value;
                                        let _ = settings.save();
                                        cx.notify(entity_id);
                                    }
                                }),
                            ))
                            .child(Divider::horizontal())
                            .child(Self::render_labeled_row(
                                t.image_retention,
                                NumberField::new(
                                    "image_retention_days",
                                    settings.image_retention_days,
                                    window,
                                    cx,
                                )
                                .mode(NumberFieldMode::Edit, cx)
                                .min(1)
                                .max(90)
                                .on_change({
                                    move |value, _, cx| {
                                        let mut settings =
                                            fly_settings::Settings::load()
                                                .unwrap_or_default();
                                        settings.image_retention_days = *value;
                                        let _ = settings.save();
                                        cx.notify(entity_id);
                                    }
                                }),
                            ))
                    )
                    .child(
                        div()
                            .pt_1()
                            .pb_2()
                            .pl_2()
                            .pr_2()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(Label::new(t.retention_footer.to_string()).size(LabelSize::XSmall).color(Color::Muted))
                            .child(
                                div()
                                    .id("retention_clean_now")
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| this.start_cleanup(cx)))
                                    .child(Label::new(t.clean_expired_inline.to_string()).size(LabelSize::XSmall).color(Color::Accent))
                            )
                    )
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(Self::section_header(t.section_storage))
                    .child(
                        div()
                            .border_1()
                            .border_color(cx.theme().colors().border)
                            .bg(cx.theme().colors().elevated_surface_background)
                            .rounded_lg()
                            .child(Self::render_loading_value_row(
                                t.data_size,
                                &format_size(data_size),
                                self.cleaning_up,
                            ))
                            .child(Divider::horizontal())
                            .child(Self::render_loading_value_row(
                                t.index_size,
                                &format_size(index_size),
                                self.rebuilding_index,
                            ))
                            .child(Divider::horizontal())
                            .child(Self::render_action_row(
                                t.clean_expired_button,
                                Color::Accent,
                                "clean_expired_button",
                                false,
                                |this, cx| this.start_cleanup(cx),
                                cx,
                            ))
                            .child(Divider::horizontal())
                            .child(Self::render_action_row(
                                t.rebuild_index,
                                Color::Accent,
                                "rebuild_index",
                                false,
                                |this, cx| this.start_rebuild_index(cx),
                                cx,
                            ))
                            .child(Divider::horizontal())
                            .child(Self::render_action_row(
                                t.clear_history,
                                Color::Error,
                                "clear_history",
                                true,
                                |this, cx| {
                                    this.clear_confirm_open = true;
                                    cx.notify();
                                },
                                cx,
                            ))
                    )
                    .child(Self::section_footer(t.storage_footer))
            )
    }
}

impl Focusable for SettingsWindow {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = fly_settings::Settings::load().unwrap_or_default();
        let t = i18n::strings(settings.language);
        let scroll_handle = self.scroll_handle.clone();
        let show_title = scroll_handle.offset().y < px(-30.0);
        
        let tab_title = match self.active_tab {
            SettingsTab::General => t.section_general,
            SettingsTab::Shortcuts => t.section_shortcuts,
            SettingsTab::Storage => t.section_storage,
            SettingsTab::Tutorial => t.section_tutorial,
            SettingsTab::Faq => t.section_faq,
        };

        div()
            .relative()
            .size_full()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .size_full()
                    .key_context("SettingsWindow")
                    .track_focus(&self.focus_handle)
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if this.clear_confirm_open {
                            if event.keystroke.key == "escape" {
                                this.clear_confirm_open = false;
                                cx.notify();
                                cx.stop_propagation();
                                return;
                            }
                        }

                        if let Some(target) = this.recording_target {
                            let mut modifiers = Vec::new();
                            if event.keystroke.modifiers.platform {
                                modifiers.push("cmd".to_string());
                            }
                            if event.keystroke.modifiers.alt {
                                modifiers.push("alt".to_string());
                            }
                            if event.keystroke.modifiers.shift {
                                modifiers.push("shift".to_string());
                            }
                            if event.keystroke.modifiers.control {
                                modifiers.push("ctrl".to_string());
                            }

                            let mut key = event.keystroke.key.clone();

                            if key == "`" {
                                key = "BackQuote".to_string();
                            }

                            if key == "cmd"
                                || key == "alt"
                                || key == "shift"
                                || key == "ctrl"
                                || key == "function"
                            {
                                return;
                            }

                            this.update_settings(cx, |s| {
                                let hotkey = match target {
                                    RecordingTarget::Activation => &mut s.hotkey,
                                    RecordingTarget::Regex => &mut s.regex_hotkey,
                                    RecordingTarget::CaseSensitive => &mut s.case_sensitive_hotkey,
                                    RecordingTarget::FocusSearch => &mut s.focus_hotkey,
                                };
                                hotkey.modifiers = modifiers;
                                hotkey.key = key;

                                if target == RecordingTarget::Activation {
                                    update_global_hotkey(hotkey);
                                }
                            });

                            this.recording_target = None;
                            cx.notify();
                            cx.stop_propagation();
                        } else {
                            if event.keystroke.key == "w" && event.keystroke.modifiers.platform {
                                window.remove_window();
                                cx.stop_propagation();
                            }
                        }
                    }))
                    .bg(cx.theme().colors().background)
                    .text_color(cx.theme().colors().text)
                    .child(self.render_sidebar(t, cx))
                    .child(
                        div()
                            .relative()
                            .flex_grow()
                            .min_w_0()
                            .size_full()
                            .child(
                                div()
                                    .id("settings_scroll")
                                    .size_full()
                                    .overflow_y_scroll()
                                    .track_scroll(&self.scroll_handle)
                                    .on_scroll_wheel(cx.listener(|_this, _event: &ScrollWheelEvent, _window, cx| {
                                        cx.notify();
                                    }))
                                    .px_8()
                                    .pt_10()
                                    .pb_6()
                                    .flex()
                                    .flex_col()
                                    .gap_8()
                                    .child(match self.active_tab {
                                        SettingsTab::General => self.render_general_tab(t, cx).into_any_element(),
                                        SettingsTab::Shortcuts => self.render_shortcuts_tab(t, cx).into_any_element(),
                                        SettingsTab::Storage => self.render_storage_tab(t, window, cx).into_any_element(),
                                        SettingsTab::Tutorial => {
                                            let language = fly_settings::Settings::load().unwrap_or_default().language;
                                            let file_path = match language {
                                                fly_settings::Language::En => "docs/tutorial_en.md",
                                                _ => "docs/tutorial.md",
                                            };
                                            let content = std::str::from_utf8(crate::assets::Assets::get(file_path).unwrap().data.as_ref()).unwrap().to_string();
                                            div().pr_12().child(MarkdownRenderer::new("tutorial_md", content)).into_any_element()
                                        }
                                        SettingsTab::Faq => {
                                            let language = fly_settings::Settings::load().unwrap_or_default().language;
                                            let file_path = match language {
                                                fly_settings::Language::En => "docs/faq_en.md",
                                                _ => "docs/faq.md",
                                            };
                                            let content = std::str::from_utf8(crate::assets::Assets::get(file_path).unwrap().data.as_ref()).unwrap().to_string();
                                            div().pr_12().child(MarkdownRenderer::new("faq_md", content)).into_any_element()
                                        }
                                    })
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .w_full()
                                    .h_8()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(cx.theme().colors().background)
                                    .when(show_title, |this| {
                                        this.child(Label::new(tab_title.to_string()).size(LabelSize::Small).color(Color::Muted).weight(gpui::FontWeight::BOLD))
                                    })
                            )
                    )
            )
            .when(self.clear_confirm_open, |this| {
                this.child(self.render_clear_confirm_dialog(t, cx))
            })
    }
}
