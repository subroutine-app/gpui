//! Eased wheel notches and animated scrolls in a scrolling div, a uniform list and a list. Time is
//! the test executor's clock, so each frame samples a known point of a transition.

use std::time::Duration;

use crate::{
    self as gpui, AppContext, Context, Entity, InteractiveElement, IntoElement, ListAlignment,
    ListState, ParentElement, Pixels, Render, ScrollDelta, ScrollEvent, ScrollHandle,
    ScrollStrategy, StatefulInteractiveElement, Styled, TestAppContext, UniformListScrollHandle,
    VisualTestContext, Window, div, list, point, px, size, uniform_list,
};

const ROW: Pixels = px(40.);
/// Past both the 80 ms wheel transition and the 200 ms animated-scroll transition.
const SETTLED: Duration = Duration::from_millis(250);

fn row(_: usize) -> impl IntoElement {
    div().h(ROW).w_full().flex_none()
}

/// Columns 100px wide and 200px tall: 20 rows in a div (a range of 600px), 100 in a uniform list
/// and 50 in a list (1800px once measured).
struct Containers {
    scroll: ScrollHandle,
    uniform: UniformListScrollHandle,
    list: ListState,
}

impl Render for Containers {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .child(
                div()
                    .id("rows")
                    .w(px(100.))
                    .h_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children((0..20).map(row)),
            )
            .child(
                uniform_list("uniform", 100, |range, _, _| range.map(row).collect())
                    .track_scroll(&self.uniform)
                    .w(px(100.))
                    .h_full(),
            )
            .child(
                div().w(px(100.)).h_full().child(
                    list(self.list.clone(), |ix, _, _| row(ix).into_any_element()).size_full(),
                ),
            )
    }
}

fn open(cx: &mut TestAppContext) -> (Entity<Containers>, &mut VisualTestContext) {
    let cx = cx.add_empty_window();
    let view = cx.update(|_, cx| {
        cx.new(|_| Containers {
            scroll: ScrollHandle::new(),
            uniform: UniformListScrollHandle::new(),
            // Measured whole, so the list's range covers every row.
            list: ListState::new(50, ListAlignment::Top, px(0.)).measure_all(),
        })
    });
    frame(cx, &view, Duration::ZERO);
    (view, cx)
}

fn frame(cx: &mut VisualTestContext, view: &Entity<Containers>, after: Duration) {
    cx.executor().advance_clock(after);
    cx.draw(point(px(0.), px(0.)), size(px(300.), px(200.)), |_, _| {
        view.clone().into_any_element()
    });
}

/// Turns the wheel over the column whose left edge is at `x`.
fn wheel(cx: &mut VisualTestContext, x: f32, delta: ScrollDelta) {
    cx.simulate_event(ScrollEvent {
        position: point(px(x + 50.), px(100.)),
        delta,
        ..Default::default()
    });
}

const DIV: f32 = 0.;
const LIST: f32 = 200.;

fn list_offset(view: &Entity<Containers>, cx: &mut VisualTestContext) -> Pixels {
    let top = view.read_with(cx, |view, _| view.list.logical_scroll_top());
    -(ROW * top.item_ix as f32 + top.offset_in_item)
}

#[track_caller]
fn assert_close(actual: Pixels, expected: Pixels) {
    assert!(
        (actual - expected).abs() < px(0.01),
        "expected {expected:?}, got {actual:?}"
    );
}

#[gpui::test]
fn notched_wheels_ease_add_up_and_stay_in_range(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    let scroll = view.read_with(cx, |view, _| view.scroll.clone());
    let line = cx.update(|window, _| window.line_height());

    wheel(cx, DIV, ScrollDelta::Lines(point(0., -3.)));
    assert_eq!(
        scroll.offset().y,
        px(0.),
        "a notch eases instead of jumping"
    );
    frame(cx, &view, Duration::from_millis(40));
    let partway = scroll.offset().y;
    assert!(partway < px(0.) && partway > -line * 3., "{partway:?}");

    // A notch during the motion extends its target without dropping any distance.
    wheel(cx, DIV, ScrollDelta::Lines(point(0., -3.)));
    frame(cx, &view, SETTLED);
    assert_close(scroll.offset().y, -line * 6.);

    // The target is clamped to the range: no overshoot on the way, none at rest.
    wheel(cx, DIV, ScrollDelta::Lines(point(0., -1000.)));
    frame(cx, &view, Duration::from_millis(20));
    assert!(scroll.offset().y > px(-600.));
    frame(cx, &view, SETTLED);
    assert_close(scroll.offset().y, px(-600.));

    // Trackpad deltas stay immediate.
    wheel(cx, DIV, ScrollDelta::Pixels(point(px(0.), px(50.))));
    assert_close(scroll.offset().y, px(-550.));

    // A list eases and clamps its notches the same way.
    wheel(cx, LIST, ScrollDelta::Lines(point(0., -1000.)));
    assert_close(list_offset(&view, cx), px(0.));
    frame(cx, &view, Duration::from_millis(20));
    let partway = list_offset(&view, cx);
    assert!(partway < px(0.) && partway > px(-1800.), "{partway:?}");
    frame(cx, &view, SETTLED);
    assert_close(list_offset(&view, cx), px(-1800.));
}

#[gpui::test]
fn animated_scrolls_ease_to_their_targets(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    let (scroll, uniform) =
        view.read_with(cx, |view, _| (view.scroll.clone(), view.uniform.clone()));
    let uniform_offset = || uniform.0.borrow().base_handle.offset().y;

    view.read_with(cx, |view, _| {
        view.scroll.set_offset_animated(point(px(0.), px(-300.)));
        view.uniform
            .scroll_to_item_animated(40, ScrollStrategy::Top);
        view.list.scroll_to_reveal_item_animated(30);
    });
    frame(cx, &view, Duration::ZERO);
    frame(cx, &view, Duration::from_millis(100));
    for (offset, target) in [
        (scroll.offset().y, px(-300.)),
        (uniform_offset(), px(-1600.)),
        (list_offset(&view, cx), px(-1040.)),
    ] {
        assert!(
            offset < px(0.) && offset > target,
            "{offset:?} toward {target:?}"
        );
    }

    frame(cx, &view, SETTLED);
    assert_close(scroll.offset().y, px(-300.));
    assert_close(uniform_offset(), px(-1600.));
    // Row 30 ends at 1240px, so revealing it scrolls its bottom to the viewport's.
    assert_close(list_offset(&view, cx), px(-1040.));

    // An animated target is clamped to the range of the last layout.
    scroll.set_offset_animated(point(px(0.), px(-5000.)));
    frame(cx, &view, Duration::ZERO);
    frame(cx, &view, SETTLED);
    assert_close(scroll.offset().y, px(-600.));
}

#[gpui::test]
fn reduced_motion_scrolls_without_easing(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_reduce_motion(true));
    let (view, cx) = open(cx);
    let scroll = view.read_with(cx, |view, _| view.scroll.clone());
    let line = cx.update(|window, _| window.line_height());

    wheel(cx, DIV, ScrollDelta::Lines(point(0., -3.)));
    assert_close(scroll.offset().y, -line * 3.);

    scroll.set_offset_animated(point(px(0.), px(-300.)));
    frame(cx, &view, Duration::ZERO);
    assert_close(scroll.offset().y, px(-300.));
}
