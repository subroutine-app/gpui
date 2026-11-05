//! The window the harness scrolls: the transcript in a `gpui::list`, with
//! a scrollbar drawn as GPUI Kit draws one, and the wheel delivered either
//! the way GPUI Kit's `ScrollableMask` does it or by the list itself.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Instant,
};

use gpui::{
    AvailableSpace, Bounds, Context, DispatchPhase, Hitbox, HitboxBehavior, IntoElement, ListState,
    ParentElement as _, Pixels, Render, ScrollEvent, SharedString, Styled as _, Window, canvas,
    div, fill, list, point, px, rgb, size,
};
use gpui_screenshot::{MONO_FAMILY, SANS_FAMILY};

use crate::transcript::{Row, render_row};

/// The list's viewport, which is also the scrollbar's track.
pub const VIEWPORT: f32 = 600.;
/// The list's padding above its first row and below its last.
pub const PADDING: f32 = 8.;
/// How far past the viewport the list lays out rows.
pub const OVERDRAW: f32 = 400.;
/// GPUI Kit's scrollbar track width and its shortest thumb.
pub const TRACK_WIDTH: f32 = 16.;
pub const MIN_THUMB: f32 = 48.;
/// The pixels a line of wheel movement scrolls, as GPUI's list and GPUI
/// Kit count them.
pub const LINE: f32 = 20.;

/// How the wheel reaches the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wheel {
    /// In the capture phase, by `set_offset_from_scrollbar`, as GPUI Kit's
    /// `ScrollableMask` does.
    Mask,
    /// By the list's own wheel handler.
    List,
}

/// The thumb as the scrollbar drew it in the last frame, in px along its
/// track, and what the scrollbar asked the list for it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Thumb {
    pub top: f32,
    pub length: f32,
    /// The scroll range the list gave, and the offset along it.
    pub max: f32,
    pub offset: f32,
    /// How long the scrollbar's questions to the list took, in ns.
    pub query_ns: u64,
}

impl Thumb {
    /// The thumb GPUI Kit draws for `offset` down a scroll range `max` on a
    /// track that is also the viewport.
    pub fn place(offset: f32, max: f32) -> Self {
        let track = VIEWPORT;
        let content = track + max;
        let length = (track / content * track).clamp(MIN_THUMB, track);
        let fraction = if max > 0. {
            (offset / max).clamp(0., 1.)
        } else {
            0.
        };
        Self {
            top: fraction * (track - length),
            length,
            max,
            offset,
            query_ns: 0,
        }
    }
}

pub struct Harness {
    pub list: ListState,
    rows: Rc<Vec<Row>>,
    /// The list's width.
    pub width: Pixels,
    /// A line above the list for the filmstrip, `hud_height` tall.
    pub hud: SharedString,
    /// For the filmstrip, where the thumb's top was in the last steps,
    /// oldest first, drawn as ticks beside the track.
    pub trail: Vec<f32>,
    hud_height: f32,
    wheel: Wheel,
    /// The rows the list drew or measured.
    pub renders: Rc<Cell<u32>>,
    /// The frames drawn.
    pub draws: Rc<Cell<u32>>,
    pub thumb: Rc<Cell<Thumb>>,
}

impl Harness {
    pub fn new(
        list: ListState,
        rows: Rc<Vec<Row>>,
        width: Pixels,
        wheel: Wheel,
        hud_height: f32,
    ) -> Self {
        Self {
            list,
            rows,
            width,
            hud: SharedString::default(),
            trail: Vec::new(),
            hud_height,
            wheel,
            renders: Rc::default(),
            draws: Rc::default(),
            thumb: Rc::default(),
        }
    }
}

impl Render for Harness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.draws.set(self.draws.get() + 1);
        let rows = self.rows.clone();
        let renders = self.renders.clone();
        let rows_list = crate::adopt::element(
            list(self.list.clone(), move |ix, _, _| {
                renders.set(renders.get() + 1);
                render_row(&rows[ix], ix)
            }),
            &self.rows,
        )
        .size_full()
        .py(px(PADDING));
        let mut viewport = div()
            .relative()
            .flex_none()
            .w(self.width)
            .h(px(VIEWPORT))
            .overflow_hidden()
            .child(rows_list)
            .child(scrollbar(
                self.list.clone(),
                self.thumb.clone(),
                self.trail.clone(),
            ));
        if self.wheel == Wheel::Mask {
            viewport = viewport.child(mask(self.list.clone()));
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0xffffff))
            .font_family(SANS_FAMILY)
            .child(
                div()
                    .flex_none()
                    .h(px(self.hud_height))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .bg(rgb(0x24292f))
                    .text_color(rgb(0xffffff))
                    .font_family(MONO_FAMILY)
                    .text_size(px(self.hud_height * 0.45))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(self.hud.clone()),
            )
            .child(viewport)
    }
}

/// The vertical scrollbar over the list's right edge, placed as GPUI Kit's
/// `Scrollbar` places its thumb, from the same questions to the list.
fn scrollbar(state: ListState, thumb: Rc<Cell<Thumb>>, trail: Vec<f32>) -> impl IntoElement {
    canvas(
        move |bounds, _, _| {
            let started = Instant::now();
            let max = f32::from(state.max_offset_for_scrollbar().y);
            let offset = -f32::from(state.scroll_px_offset_for_scrollbar().y);
            let query_ns = started.elapsed().as_nanos() as u64;
            let placed = Thumb {
                query_ns,
                ..Thumb::place(offset, max)
            };
            thumb.set(placed);
            (bounds, placed)
        },
        move |_, (bounds, placed): (Bounds<Pixels>, Thumb), window, _| {
            if placed.max <= 0. {
                return;
            }
            for (age, top) in trail.iter().rev().enumerate() {
                let tick = Bounds::new(
                    point(bounds.left() - px(14.), bounds.top() + px(*top)),
                    size(px(10.), px(2.)),
                );
                let alpha = 1. - age as f32 / (trail.len() as f32 + 1.);
                window.paint_quad(fill(tick, gpui::hsla(0., 0.85, 0.45, alpha)));
            }
            window.paint_quad(fill(bounds, rgb(0xeef0f2)));
            let thumb = Bounds::new(
                point(bounds.left() + px(4.), bounds.top() + px(placed.top)),
                size(px(TRACK_WIDTH - 8.), px(placed.length)),
            );
            window.paint_quad(fill(thumb, rgb(0x57606a)).corner_radii(px(4.)));
        },
    )
    .absolute()
    .top_0()
    .right_0()
    .w(px(TRACK_WIDTH))
    .h_full()
}

/// Takes vertical wheel movement over the list in the capture phase and
/// scrolls by the scrollbar's offset, as GPUI Kit's `ScrollableMask` does.
/// At either end of the range the event goes on to the list.
fn mask(state: ListState) -> impl IntoElement {
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |_, hitbox: Hitbox, window, _| {
            let view = window.current_view();
            window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
                if !(phase == DispatchPhase::Capture && hitbox.should_handle_scroll(window)) {
                    return;
                }
                let mut offset = state.scroll_px_offset_for_scrollbar();
                let delta = event.delta.pixel_delta(px(LINE));
                let max = state.max_offset_for_scrollbar().y.max(px(0.));
                let current = offset.y.clamp(-max, px(0.));
                let next = (current + delta.y).clamp(-max, px(0.));
                if next == current {
                    return;
                }
                offset.y = next;
                state.set_offset_from_scrollbar(offset);
                cx.notify(view);
                cx.stop_propagation();
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Lays out every row at `width` once and keeps its height, which is what
/// the list would find drawing them all.
pub struct Truth {
    rows: Rc<Vec<Row>>,
    width: Pixels,
    pub heights: Rc<RefCell<Vec<f32>>>,
}

impl Truth {
    pub fn new(rows: Rc<Vec<Row>>, width: Pixels) -> Self {
        Self {
            rows,
            width,
            heights: Rc::default(),
        }
    }
}

impl Render for Truth {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows.clone();
        let width = self.width;
        let heights = self.heights.clone();
        div().size_full().font_family(SANS_FAMILY).child(
            canvas(
                move |_, window, cx| {
                    if !heights.borrow().is_empty() {
                        return;
                    }
                    let space = size(AvailableSpace::Definite(width), AvailableSpace::MinContent);
                    let measured = rows
                        .iter()
                        .enumerate()
                        .map(|(ix, row)| {
                            let mut element = render_row(row, ix);
                            f32::from(element.layout_as_root(space, window, cx).height)
                        })
                        .collect();
                    *heights.borrow_mut() = measured;
                },
                |_, _, _, _| {},
            )
            .size_full(),
        )
    }
}
