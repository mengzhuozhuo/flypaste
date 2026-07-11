// Adapted from Zed's settings_ui NumberField (GPL-3.0-or-later)

use std::{fmt::Display, rc::Rc, str::FromStr};

use editor::actions::{Backspace, Delete, SelectAll};
use editor::{
    Editor, EditorElement, EditorStyle, make_inlay_hints_style, make_suggestion_styles,
};
use gpui::{
    relative, rems, ClickEvent, Entity, FocusHandle, Focusable, KeyDownEvent,
    Modifiers, TextAlign, TextStyle, TextStyleRefinement, WeakEntity,
};
use settings::Settings;
use theme::PlayerColor;
use theme_settings::ThemeSettings;
use ui::prelude::*;
use zed_actions::editor::{MoveDown, MoveUp};

// Colors are pulled dynamically from cx.theme().colors()

fn number_field_editor_style(cx: &App, text_color: gpui::Hsla) -> EditorStyle {
    let settings = ThemeSettings::get_global(cx);
    let mut text_style = TextStyle {
        color: text_color,
        font_family: settings.ui_font.family.clone(),
        font_features: settings.ui_font.features.clone(),
        font_fallbacks: settings.ui_font.fallbacks.clone(),
        font_size: rems(0.875).into(),
        font_weight: settings.ui_font.weight,
        line_height: relative(settings.buffer_line_height.value()),
        ..Default::default()
    };
    text_style.refine(&TextStyleRefinement {
        color: Some(text_color),
        text_align: Some(TextAlign::Center),
        ..Default::default()
    });

    EditorStyle {
        background: cx.theme().colors().editor_background.into(),
        border: cx.theme().colors().border.into(),
        local_player: PlayerColor {
            cursor: cx.theme().colors().text.into(),
            background: cx.theme().colors().editor_background.into(),
            selection: cx.theme().colors().editor_document_highlight_read_background.into(),
        },
        text: text_style,
        scrollbar_width: px(15.),
        syntax: cx.theme().syntax().clone(),
        status: cx.theme().status().clone(),
        inlay_hints_style: make_inlay_hints_style(cx),
        edit_prediction_styles: make_suggestion_styles(cx),
        unnecessary_code_fade: settings.unnecessary_code_fade,
        show_underlines: false,
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NumberFieldMode {
    #[default]
    Read,
    Edit,
}

pub trait NumberFieldType: Display + Copy + Clone + Sized + PartialOrd + FromStr + 'static {
    fn default_format(value: &Self) -> String {
        format!("{}", value)
    }
    fn default_step() -> Self;
    fn large_step() -> Self;
    fn small_step() -> Self;
    fn min_value() -> Self;
    fn max_value() -> Self;
    fn saturating_add(self, rhs: Self) -> Self;
    fn saturating_sub(self, rhs: Self) -> Self;
}

macro_rules! impl_numeric_stepper_int {
    ($type:ident) => {
        impl NumberFieldType for $type {
            fn default_step() -> Self {
                1
            }

            fn large_step() -> Self {
                10
            }

            fn small_step() -> Self {
                1
            }

            fn min_value() -> Self {
                <$type>::MIN
            }

            fn max_value() -> Self {
                <$type>::MAX
            }

            fn saturating_add(self, rhs: Self) -> Self {
                self.saturating_add(rhs)
            }

            fn saturating_sub(self, rhs: Self) -> Self {
                self.saturating_sub(rhs)
            }
        }
    };
}

impl_numeric_stepper_int!(u32);

type OnChangeCallback<T> = Rc<dyn Fn(&T, &mut Window, &mut App) + 'static>;

#[derive(IntoElement)]
pub struct NumberField<T: NumberFieldType = u32> {
    id: ElementId,
    value: T,
    focus_handle: FocusHandle,
    mode: Entity<NumberFieldMode>,
    edit_editor: Entity<Option<WeakEntity<Editor>>>,
    on_change_state: Entity<Option<OnChangeCallback<T>>>,
    last_synced_value: Entity<Option<T>>,
    format: Box<dyn FnOnce(&T) -> String>,
    large_step: T,
    small_step: T,
    step: T,
    min_value: T,
    max_value: T,
    on_reset: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
    on_change: Rc<dyn Fn(&T, &mut Window, &mut App) + 'static>,
    tab_index: Option<isize>,
}

impl<T: NumberFieldType> NumberField<T> {
    pub fn new(id: impl Into<ElementId>, value: T, window: &mut Window, cx: &mut App) -> Self {
        let id = id.into();

        let (mode, focus_handle, edit_editor, on_change_state, last_synced_value) =
            window.with_id(id.clone(), |window| {
                let mode = window.use_state(cx, |_, _| NumberFieldMode::default());
                let focus_handle = window.use_state(cx, |_, cx| cx.focus_handle());
                let edit_editor = window.use_state(cx, |_, _| None);
                let on_change_state: Entity<Option<OnChangeCallback<T>>> =
                    window.use_state(cx, |_, _| None);
                let last_synced_value: Entity<Option<T>> = window.use_state(cx, |_, _| None);
                (
                    mode,
                    focus_handle,
                    edit_editor,
                    on_change_state,
                    last_synced_value,
                )
            });

        Self {
            id,
            mode,
            edit_editor,
            on_change_state,
            last_synced_value,
            value,
            focus_handle: focus_handle.read(cx).clone(),
            format: Box::new(T::default_format),
            large_step: T::large_step(),
            step: T::default_step(),
            small_step: T::small_step(),
            min_value: T::min_value(),
            max_value: T::max_value(),
            on_reset: None,
            on_change: Rc::new(|_, _, _| {}),
            tab_index: None,
        }
    }

    pub fn min(mut self, min: T) -> Self {
        self.min_value = min;
        self
    }

    pub fn max(mut self, max: T) -> Self {
        self.max_value = max;
        self
    }

    pub fn mode(self, mode: NumberFieldMode, cx: &mut App) -> Self {
        self.mode.write(cx, mode);
        self
    }

    pub fn on_change(mut self, on_change: impl Fn(&T, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Rc::new(on_change);
        self
    }

    fn sync_on_change_state(&self, cx: &mut App) {
        self.on_change_state
            .update(cx, |state, _| *state = Some(self.on_change.clone()));
    }
}

#[derive(Clone, Copy)]
enum ValueChangeDirection {
    Increment,
    Decrement,
}

impl<T: NumberFieldType> RenderOnce for NumberField<T> {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.sync_on_change_state(cx);

        let is_edit_mode = matches!(*self.mode.read(cx), NumberFieldMode::Edit);

        let get_step = {
            let large_step = self.large_step;
            let step = self.step;
            let small_step = self.small_step;
            move |modifiers: Modifiers| -> T {
                if modifiers.shift {
                    large_step
                } else if modifiers.alt {
                    small_step
                } else {
                    step
                }
            }
        };

        let clamp_value = {
            let min = self.min_value;
            let max = self.max_value;
            move |value: T| -> T {
                if value < min {
                    min
                } else if value > max {
                    max
                } else {
                    value
                }
            }
        };

        let change_value = {
            move |current: T, step: T, direction: ValueChangeDirection| -> T {
                let new_value = match direction {
                    ValueChangeDirection::Increment => current.saturating_add(step),
                    ValueChangeDirection::Decrement => current.saturating_sub(step),
                };
                clamp_value(new_value)
            }
        };

        let get_current_value = {
            let value = self.value;
            let edit_editor = self.edit_editor.clone();

            Rc::new(move |cx: &App| -> T {
                if !is_edit_mode {
                    return value;
                }
                edit_editor
                    .read(cx)
                    .as_ref()
                    .and_then(|weak| weak.upgrade())
                    .and_then(|editor| editor.read(cx).text(cx).parse::<T>().ok())
                    .unwrap_or(value)
            })
        };

        let update_editor_text = {
            let edit_editor = self.edit_editor.clone();

            Rc::new(move |new_value: T, window: &mut Window, cx: &mut App| {
                if !is_edit_mode {
                    return;
                }
                let Some(editor) = edit_editor
                    .read(cx)
                    .as_ref()
                    .and_then(|weak| weak.upgrade())
                else {
                    return;
                };
                editor.update(cx, |editor, cx| {
                    editor.set_text(format!("{}", new_value), window, cx);
                });
            })
        };

        let bg_color = cx.theme().colors().editor_background;
        let hover_bg_color = cx.theme().colors().element_hover;
        let border_color = cx.theme().colors().border;
        let focus_border_color = cx.theme().colors().border_focused;
        let text_color = cx.theme().colors().text;

        let base_button = |icon: IconName| {
            h_flex()
                .cursor_pointer()
                .w(px(32.0))
                .h(px(28.0))
                .flex()
                .items_center()
                .justify_center()
                .border_1()
                .border_color(border_color)
                .bg(bg_color)
                .hover(|s| s.bg(hover_bg_color))
                .focus_visible(|s| s.border_color(focus_border_color).bg(hover_bg_color))
                .child(
                    Icon::new(icon)
                        .size(IconSize::Small)
                        .color(Color::Muted),
                )
        };

        h_flex()
            .id(self.id.clone())
            .track_focus(&self.focus_handle)
            .gap_1()
            .flex_none()
            .when_some(self.on_reset, |this, on_reset| {
                this.child(
                    IconButton::new("reset", IconName::RotateCcw)
                        .icon_size(IconSize::Small)
                        .when_some(self.tab_index, |this, _| this.tab_index(0isize))
                        .on_click(on_reset),
                )
            })
            .child({
                let on_change_for_increment = self.on_change.clone();

                h_flex()
                    .flex_none()
                    .map(|decrement| {
                        let decrement_handler = {
                            let on_change = self.on_change.clone();
                            let get_current_value = get_current_value.clone();
                            let update_editor_text = update_editor_text.clone();

                            move |click: &ClickEvent, window: &mut Window, cx: &mut App| {
                                let current_value = get_current_value(cx);
                                let step = get_step(click.modifiers());
                                let new_value = change_value(
                                    current_value,
                                    step,
                                    ValueChangeDirection::Decrement,
                                );

                                update_editor_text(new_value, window, cx);
                                on_change(&new_value, window, cx);
                            }
                        };

                        decrement.child(
                            base_button(IconName::Dash)
                                .id((self.id.clone(), "decrement_button"))
                                .rounded_tl_sm()
                                .rounded_bl_sm()
                                .when_some(self.tab_index, |this, _| this.tab_index(0isize))
                                .on_click(decrement_handler),
                        )
                    })
                    .child({
                        h_flex()
                            .w(px(42.0))
                            .h(px(28.0))
                            .flex_none()
                            .overflow_hidden()
                            .border_y_1()
                            .border_color(border_color)
                            .bg(bg_color)
                            .in_focus(|this| this.border_color(focus_border_color))
                            .child(match *self.mode.read(cx) {
                                NumberFieldMode::Read => h_flex()
                                    .px_1()
                                    .w_full()
                                    .justify_center()
                                    .child(
                                        Label::new((self.format)(&self.value)).color(Color::Muted),
                                    )
                                    .into_any_element(),
                                NumberFieldMode::Edit => {
                                    let expected_text = format!("{}", self.value);
                                    let editor_key =
                                        ElementId::Name(format!("{}_editor", self.id).into());
                                    let min_value = self.min_value;
                                    let max_value = self.max_value;

                                    let editor = window.use_keyed_state(editor_key, cx, {
                                        let expected_text = expected_text.clone();

                                        move |window, cx| {
                                            let mut editor = Editor::single_line(window, cx);

                                            editor.set_text_style_refinement(TextStyleRefinement {
                                                color: Some(text_color.into()),
                                                text_align: Some(TextAlign::Center),
                                                ..Default::default()
                                            });

                                            editor.set_text(expected_text, window, cx);

                                            let editor_weak = cx.entity().downgrade();

                                            self.edit_editor.update(cx, |state, _| {
                                                *state = Some(editor_weak);
                                            });

                                            let entity = cx.entity();
                                            window
                                                .observe(&entity, cx, {
                                                    move |editor, window, cx| {
                                                        editor.update(cx, |editor, cx| {
                                                            let current_text = editor.text(cx);
                                                            let digits: String = current_text
                                                                .chars()
                                                                .filter(|c| c.is_ascii_digit())
                                                                .collect();
                                                            let sanitized = if digits.is_empty()
                                                            {
                                                                digits
                                                            } else if let Ok(parsed) =
                                                                digits.parse::<T>()
                                                            {
                                                                if parsed > max_value {
                                                                    max_value.to_string()
                                                                } else if parsed < min_value {
                                                                    min_value.to_string()
                                                                } else {
                                                                    parsed.to_string()
                                                                }
                                                            } else {
                                                                String::new()
                                                            };

                                                            if sanitized != current_text {
                                                                editor.set_text(
                                                                    sanitized, window, cx,
                                                                );
                                                            }
                                                        });
                                                    }
                                                })
                                                .detach();

                                            editor
                                                .register_action::<MoveUp>({
                                                    let on_change = self.on_change.clone();
                                                    let editor_handle = cx.entity().downgrade();
                                                    move |_, window, cx| {
                                                        let Some(editor) = editor_handle.upgrade()
                                                        else {
                                                            return;
                                                        };
                                                        editor.update(cx, |editor, cx| {
                                                            if let Ok(current_value) =
                                                                editor.text(cx).parse::<T>()
                                                            {
                                                                let step =
                                                                    get_step(window.modifiers());
                                                                let new_value = change_value(
                                                                    current_value,
                                                                    step,
                                                                    ValueChangeDirection::Increment,
                                                                );
                                                                editor.set_text(
                                                                    format!("{}", new_value),
                                                                    window,
                                                                    cx,
                                                                );
                                                                on_change(&new_value, window, cx);
                                                            }
                                                        });
                                                    }
                                                })
                                                .detach();

                                            editor
                                                .register_action::<MoveDown>({
                                                    let on_change = self.on_change.clone();
                                                    let editor_handle = cx.entity().downgrade();
                                                    move |_, window, cx| {
                                                        let Some(editor) = editor_handle.upgrade()
                                                        else {
                                                            return;
                                                        };
                                                        editor.update(cx, |editor, cx| {
                                                            if let Ok(current_value) =
                                                                editor.text(cx).parse::<T>()
                                                            {
                                                                let step =
                                                                    get_step(window.modifiers());
                                                                let new_value = change_value(
                                                                    current_value,
                                                                    step,
                                                                    ValueChangeDirection::Decrement,
                                                                );
                                                                editor.set_text(
                                                                    format!("{}", new_value),
                                                                    window,
                                                                    cx,
                                                                );
                                                                on_change(&new_value, window, cx);
                                                            }
                                                        });
                                                    }
                                                })
                                                .detach();

                                            cx.on_focus_out(&editor.focus_handle(cx), window, {
                                                let on_change_state = self.on_change_state.clone();
                                                move |this, _, window, cx| {
                                                    let text = this.text(cx);
                                                    if text.is_empty() {
                                                        return;
                                                    }
                                                    if let Ok(parsed_value) = text.parse::<T>() {
                                                        let new_value = clamp_value(parsed_value);
                                                        if new_value.to_string() != text {
                                                            this.set_text(
                                                                format!("{}", new_value),
                                                                window,
                                                                cx,
                                                            );
                                                        }
                                                        let on_change =
                                                            on_change_state.read(cx).clone();

                                                        if let Some(on_change) = on_change.as_ref()
                                                        {
                                                            on_change(&new_value, window, cx);
                                                        }
                                                    }
                                                }
                                            })
                                            .detach();

                                            cx.on_focus(&editor.focus_handle(cx), window, {
                                                move |editor, window, cx| {
                                                    editor.select_all(&SelectAll, window, cx);
                                                }
                                            })
                                            .detach();

                                            editor
                                        }
                                    });

                                    let focus_handle = editor.focus_handle(cx);
                                    let is_focused = focus_handle.is_focused(window);

                                    if !is_focused {
                                        let current_text = editor.read(cx).text(cx);
                                        let last_synced = *self.last_synced_value.read(cx);

                                        let value_changed_externally = last_synced
                                            .map(|last| last != self.value)
                                            .unwrap_or(true);

                                        let should_sync = if value_changed_externally {
                                            true
                                        } else {
                                            match current_text.parse::<T>().ok() {
                                                Some(parsed) => parsed == self.value,
                                                None => true,
                                            }
                                        };

                                        if should_sync && current_text != expected_text {
                                            editor.update(cx, |editor, cx| {
                                                editor.set_text(expected_text.clone(), window, cx);
                                            });
                                        }

                                        self.last_synced_value
                                            .update(cx, |state, _| *state = Some(self.value));
                                    }

                                    let focus_handle = if self.tab_index.is_some() {
                                        focus_handle.tab_index(0isize).tab_stop(true)
                                    } else {
                                        focus_handle
                                    };

                                    h_flex()
                                        .w_full()
                                        .h_full()
                                        .key_context("NumberFieldEditor")
                                        .track_focus(&focus_handle)
                                        .on_key_down({
                                            move |event: &KeyDownEvent, _, cx| {
                                                let key = event.keystroke.key.as_str();
                                                if key.len() == 1
                                                    && !key
                                                        .chars()
                                                        .next()
                                                        .is_some_and(|c| c.is_ascii_digit())
                                                {
                                                    cx.stop_propagation();
                                                }
                                            }
                                        })
                                        .on_action({
                                            let editor = editor.clone();
                                            move |_: &Backspace, window, cx| {
                                                editor.update(cx, |editor, cx| {
                                                    editor.backspace(
                                                        &Backspace,
                                                        window,
                                                        cx,
                                                    );
                                                });
                                            }
                                        })
                                        .on_action({
                                            let editor = editor.clone();
                                            move |_: &Delete, window, cx| {
                                                editor.update(cx, |editor, cx| {
                                                    editor.delete(&Delete, window, cx);
                                                });
                                            }
                                        })
                                        .when(is_focused, |this| {
                                            this.border_1().border_color(focus_border_color)
                                        })
                                        .child(EditorElement::new(
                                            &editor,
                                            number_field_editor_style(cx, text_color.into()),
                                        ))
                                        .on_action::<menu::Confirm>({
                                            move |_, window, _| {
                                                window.blur();
                                            }
                                        })
                                        .into_any_element()
                                }
                            })
                    })
                    .map(|increment| {
                        let increment_handler = {
                            let on_change = on_change_for_increment.clone();
                            let get_current_value = get_current_value.clone();
                            let update_editor_text = update_editor_text.clone();

                            move |click: &ClickEvent, window: &mut Window, cx: &mut App| {
                                let current_value = get_current_value(cx);
                                let step = get_step(click.modifiers());
                                let new_value = change_value(
                                    current_value,
                                    step,
                                    ValueChangeDirection::Increment,
                                );

                                update_editor_text(new_value, window, cx);
                                on_change(&new_value, window, cx);
                            }
                        };

                        increment.child(
                            base_button(IconName::Plus)
                                .id((self.id.clone(), "increment_button"))
                                .rounded_tr_sm()
                                .rounded_br_sm()
                                .when_some(self.tab_index, |this, _| this.tab_index(0isize))
                                .on_click(increment_handler),
                        )
                    })
            })
    }
}
