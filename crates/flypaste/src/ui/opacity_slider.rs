use gpui::{prelude::*, px, rgb, App, Bounds, DragMoveEvent, Empty, Pixels};
use std::rc::Rc;
use ui::{prelude::*, Color, Label, LabelSize};

const TRACK_W: f32 = 140.0;
const TRACK_H: f32 = 28.0;
const BAR_H: f32 = 4.0;
const THUMB: f32 = 14.0;
const TRACK_BG: u32 = 0xE5E5EA;
const THUMB_BORDER: u32 = 0xC6C6C8;
const FILL: u32 = 0x007AFF;

/// Each slider must use a distinct drag type — GPUI broadcasts `on_drag_move` to
/// every listener registered for the same drag type, not just the dragged element.
#[derive(Clone, Copy)]
pub struct BrowsePanelOpacityDrag;

#[derive(Clone, Copy)]
pub struct SearchPanelOpacityDrag;

fn value_to_fraction(value: f32, min: f32, max: f32) -> f32 {
    if max <= min {
        return 0.0;
    }
    ((value - min) / (max - min)).clamp(0.0, 1.0)
}

fn fraction_to_value(fraction: f32, min: f32, max: f32) -> f32 {
    min + fraction.clamp(0.0, 1.0) * (max - min)
}

fn position_to_fraction(position_x: Pixels, bounds: Bounds<Pixels>) -> f32 {
    let width = bounds.size.width.as_f32().max(1.0);
    ((position_x - bounds.origin.x).as_f32() / width).clamp(0.0, 1.0)
}

/// Horizontal opacity slider with a fixed [min, max] range.
pub fn opacity_slider<Drag: 'static + Copy>(
    id: &'static str,
    value: f32,
    min: f32,
    max: f32,
    drag: Drag,
    on_change: Rc<dyn Fn(f32, &mut App)>,
) -> impl IntoElement {
    let fraction = value_to_fraction(value, min, max);
    let thumb_left = fraction * (TRACK_W - THUMB);
    let fill_width = thumb_left + THUMB / 2.0;
    let percent = (value * 100.0).round() as u32;

    let on_drag_start = on_change.clone();
    let on_drag_move = on_change;

    div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .id(id)
                .w(px(TRACK_W))
                .h(px(TRACK_H))
                .relative()
                .flex()
                .items_center()
                .cursor_pointer()
                .on_drag(drag, move |_, offset, _, cx| {
                    let fraction = (offset.x.as_f32() / TRACK_W).clamp(0.0, 1.0);
                    on_drag_start(fraction_to_value(fraction, min, max), cx);
                    cx.new(|_| Empty)
                })
                .on_drag_move(move |event: &DragMoveEvent<Drag>, _, cx| {
                    let fraction = position_to_fraction(event.event.position.x, event.bounds);
                    on_drag_move(fraction_to_value(fraction, min, max), cx);
                })
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top(px((TRACK_H - BAR_H) / 2.0))
                        .w_full()
                        .h(px(BAR_H))
                        .rounded_full()
                        .bg(rgb(TRACK_BG)),
                )
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top(px((TRACK_H - BAR_H) / 2.0))
                        .w(px(fill_width))
                        .h(px(BAR_H))
                        .rounded_full()
                        .bg(rgb(FILL)),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(thumb_left))
                        .top(px((TRACK_H - THUMB) / 2.0))
                        .size(px(THUMB))
                        .rounded_full()
                        .bg(rgb(0xFFFFFF))
                        .border_1()
                        .border_color(rgb(THUMB_BORDER))
                        .shadow_sm(),
                ),
        )
        .child(
            div()
                .w(px(40.0))
                .flex()
                .justify_end()
                .child(
                    Label::new(format!("{percent:>3}%"))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                ),
        )
}
