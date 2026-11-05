//! How steadily a list's scrollbar moves while the list is scrolled by the
//! wheel through rows it hasn't measured yet, over a transcript of rows of
//! very different heights, with and without height estimates, while a row
//! streams in, and across a zoom. The tests print their numbers and assert
//! the properties the estimates are for. `ATTRIBUTE=1` prints the list's
//! numbers behind every step off the median, and `INDEX=1` the numbers a
//! thumb placed by item index would get.

use std::{cell::RefCell, rc::Rc};

use crate::{
    self as gpui, AppContext, Context, Element, FollowMode, IntoElement, ListState, Render,
    ScrollDelta, ScrollEvent, Styled, TestAppContext, Window, div, list, point, px, size,
};

const WIDTH: f32 = 800.;
const HEIGHT: f32 = 600.;
const WHEEL: f32 = 60.;
const OVERDRAW: f32 = 400.;
/// The kit's shortest thumb.
const MIN_THUMB: f32 = 48.;

/// Heights of a transcript's rows at 800px wide. A turn is the user's
/// question, a few collapsed reads (every fourth call open), a long shell
/// output every third turn, a reply of one of five kinds (one line, five
/// paragraphs of prose, a code block of 30 lines, one capped at 40 lines, or
/// a list of 8 points), the reply's actions and the turn's footer. Markdown
/// blocks are rows of their own.
pub(super) fn transcript_rows(turns: usize) -> Vec<f32> {
    let mut rows = Vec::new();
    let mut calls = 0;
    let mut call = |rows: &mut Vec<f32>, open_height: f32| {
        rows.push(if calls % 4 == 0 { open_height } else { 30. });
        calls += 1;
    };
    for turn in 0..turns {
        rows.push(56.);
        for _ in 0..turn % 3 {
            call(&mut rows, 96.);
        }
        if turn % 3 == 1 {
            call(&mut rows, 30. + 40. * 20.);
        }
        match turn % 5 {
            0 => rows.push(24.),
            1 => rows.extend([132.; 5]),
            2 => rows.extend([24., 30. * 20. + 24.]),
            3 => rows.extend([24., 40. * 20. + 24.]),
            _ => rows.push(8. * 24.),
        }
        rows.push(28.);
        rows.push(24.);
    }
    rows
}

/// An estimate of row `ix`'s height from its real height.
pub(super) type Estimate = Rc<dyn Fn(usize, f32) -> f32>;

pub(super) struct RowsView {
    pub state: ListState,
    pub heights: Rc<Vec<f32>>,
    pub estimate: Option<Estimate>,
}

impl Render for RowsView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let heights = self.heights.clone();
        let rows = list(self.state.clone(), {
            let heights = heights.clone();
            move |ix, _, _| div().h(px(heights[ix])).w_full().into_any()
        });
        match self.estimate.clone() {
            Some(estimate) => {
                rows.estimate_item_heights(move |ix, _, _, _| px(estimate(ix, heights[ix])))
            }
            None => rows,
        }
        .w_full()
        .h_full()
    }
}

/// The thumb as the kit draws it from the list's state.
#[derive(Clone, Copy, Debug)]
pub(super) struct Thumb {
    pub top: f32,
    pub length: f32,
    pub fraction: f32,
    pub max: f32,
}

pub(super) fn thumb(state: &ListState) -> Thumb {
    let track = state.viewport_bounds().size.height.0;
    let max = state.max_offset_for_scrollbar().y.0;
    let offset = -state.scroll_px_offset_for_scrollbar().y.0;
    let content = track + max;
    let length = (track / content * track).max(MIN_THUMB).min(track);
    let fraction = if max > 0. {
        (offset / max).clamp(0., 1.)
    } else {
        0.
    };
    Thumb {
        top: fraction * (track - length),
        length,
        fraction,
        max,
    }
}

/// The list's own numbers behind a thumb, for attributing a spike.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Inside {
    /// The scroll range by the estimates.
    pub max: f32,
    /// How far down the list the scroll top is, by the estimates.
    pub offset: f32,
    /// The height counted for each item of unknown height.
    pub estimate: f32,
    /// Items of unknown height.
    pub unknown: usize,
    /// The thumb's place, and the list's.
    pub fraction: f32,
    pub line: f32,
}

pub(super) fn inside(state: &ListState) -> Inside {
    let inner = state.0.borrow();
    let max = inner.estimated_max_offset();
    let offset = inner.estimated_scroll_offset(max);
    Inside {
        max: max.0,
        offset: offset.0,
        estimate: inner.scrollbar_estimate().0,
        unknown: inner.items.summary().unknown_count,
        fraction: inner.scrollbar_anchor.map_or(0., |anchor| anchor.fraction),
        line: if max > px(0.) { offset / max } else { 0. },
    }
}

/// How steadily the thumb moved, one wheel step a frame, scrolling one way.
#[derive(Debug, Default)]
pub(super) struct Steadiness {
    pub steps: usize,
    /// Of the thumb's travel per step, in px.
    pub mean: f32,
    pub sd: f32,
    pub cv: f32,
    pub median: f32,
    pub max: f32,
    /// The largest step against the median.
    pub spike: f32,
    /// The 5th and 95th percentiles of the travel.
    pub p05: f32,
    pub p95: f32,
    /// Steps off the median by over 10% and over 25%.
    pub off10: usize,
    pub off25: usize,
    /// Steps against the scrolling, and the largest.
    pub backwards: usize,
    pub backward: f32,
    /// Frames where the thumb's length changed by over half a pixel, the
    /// largest change, and the sum of the changes.
    pub resizes: usize,
    pub resize: f32,
    pub resized: f32,
}

pub(super) fn steadiness(thumbs: &[Thumb], down: bool) -> Steadiness {
    let mut s = Steadiness::default();
    let mut steps = Vec::new();
    for pair in thumbs.windows(2) {
        let resize = (pair[1].length - pair[0].length).abs();
        s.resize = s.resize.max(resize);
        s.resized += resize;
        s.resizes += usize::from(resize > 0.5);
        let step = pair[1].top - pair[0].top;
        let forward = if down { step } else { -step };
        if forward < -0.5 {
            s.backward = s.backward.max(-forward);
            s.backwards += 1;
        }
        steps.push(forward);
    }
    s.steps = steps.len();
    if steps.is_empty() {
        return s;
    }
    s.mean = steps.iter().sum::<f32>() / steps.len() as f32;
    s.sd = (steps.iter().map(|x| (x - s.mean).powi(2)).sum::<f32>() / steps.len() as f32).sqrt();
    s.cv = if s.mean != 0. { s.sd / s.mean } else { 0. };
    let mut sorted = steps.clone();
    sorted.sort_by(f32::total_cmp);
    s.median = sorted[sorted.len() / 2];
    s.p05 = sorted[sorted.len() * 5 / 100];
    s.p95 = sorted[sorted.len() * 95 / 100];
    s.max = steps.iter().copied().fold(0., f32::max);
    s.spike = if s.median != 0. { s.max / s.median } else { 0. };
    s.off10 = steps
        .iter()
        .filter(|x| (*x - s.median).abs() > s.median * 0.1)
        .count();
    s.off25 = steps
        .iter()
        .filter(|x| (*x - s.median).abs() > s.median * 0.25)
        .count();
    s
}

/// A deterministic error of up to `pct` either way, by item.
pub(super) fn jitter(ix: usize, pct: f32) -> f32 {
    let unit = ((ix as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40) as f32 / (1u64 << 24) as f32;
    1. + pct * (unit * 2. - 1.)
}

/// Whether a row of `height` depends on the width: prose and lists do,
/// fixed rows and code blocks don't.
pub(super) fn is_prose(height: f32) -> bool {
    height == 132. || height == 192.
}

/// Prints every step whose travel is off the median by over 40%, with the
/// list's numbers before and after it.
pub(super) fn attribute(label: &str, thumbs: &[Thumb], insides: &[Inside], down: bool) {
    let s = steadiness(thumbs, down);
    for (ix, pair) in thumbs.windows(2).enumerate() {
        let step = pair[1].top - pair[0].top;
        let forward = if down { step } else { -step };
        if (forward - s.median).abs() > s.median * 0.4 {
            let (a, b) = (insides[ix], insides[ix + 1]);
            eprintln!(
                "{label} step {ix}: travel {forward:.2} (median {:.2}) max {:.0}->{:.0} est {:.1}->{:.1} unknown {}->{} off {:.0}->{:.0} frac {:.4}->{:.4} line {:.4}->{:.4} len {:.1}->{:.1}",
                s.median,
                a.max,
                b.max,
                a.estimate,
                b.estimate,
                a.unknown,
                b.unknown,
                a.offset,
                b.offset,
                a.fraction,
                b.fraction,
                a.line,
                b.line,
                pair[0].length,
                pair[1].length
            );
        }
    }
}

/// A turn of the wheel, through the list's own handler, or through the
/// scrollbar's offset as the kit's ScrollableMask sends it.
pub(super) fn wheel(cx: &mut gpui::VisualTestContext, state: &ListState, dy: f32, by_bar: bool) {
    if by_bar {
        let max = state.max_offset_for_scrollbar().y;
        let offset = state.scroll_px_offset_for_scrollbar().y;
        let asked = (offset + px(dy)).clamp(-max, px(0.));
        state.set_offset_from_scrollbar(point(px(0.), asked));
        return;
    }
    cx.simulate_event(ScrollEvent {
        position: point(px(WIDTH / 2.), px(HEIGHT / 2.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
        ..Default::default()
    });
}

/// Scrolls a transcript of 60 turns up from its end by the wheel, then back
/// down, one step a frame, and reports how steadily the thumb moved.
pub(super) fn run_transcript(
    cx: &mut TestAppContext,
    label: &str,
    by_bar: bool,
    estimate: Option<Estimate>,
) -> (Steadiness, Steadiness) {
    run_transcript_of(cx, label, 60, 1, by_bar, estimate)
}

/// A transcript of `turns` turns, drawn `frames` times a wheel step (the
/// frames after the first are idle).
pub(super) fn run_transcript_of(
    cx: &mut TestAppContext,
    label: &str,
    turns: usize,
    frames: usize,
    by_bar: bool,
    estimate: Option<Estimate>,
) -> (Steadiness, Steadiness) {
    let cx = cx.add_empty_window();
    let heights = Rc::new(transcript_rows(turns));
    let state = ListState::new(heights.len(), crate::ListAlignment::Top, px(OVERDRAW));
    state.set_follow_mode(FollowMode::Tail);
    let view = cx.update(|_, cx| {
        cx.new(|_| RowsView {
            state: state.clone(),
            heights: heights.clone(),
            estimate,
        })
    });
    let draw = |cx: &mut gpui::VisualTestContext| {
        cx.draw(
            point(px(0.), px(0.)),
            size(px(WIDTH), px(HEIGHT)),
            |_, _| view.clone().into_any_element(),
        );
    };
    draw(cx);
    let total: f32 = heights.iter().sum();
    eprintln!(
        "{label}: {} rows, {total:.0}px, estimated {:.0}px at the end",
        heights.len(),
        thumb(&state).max + HEIGHT
    );

    let mut up = vec![thumb(&state)];
    let mut up_inside = vec![inside(&state)];
    let mut up_tops = vec![state.logical_scroll_top()];
    while up.last().unwrap().fraction > 0. && up.len() < 3000 {
        wheel(cx, &state, WHEEL, by_bar);
        for _ in 0..frames {
            draw(cx);
        }
        up.push(thumb(&state));
        up_inside.push(inside(&state));
        up_tops.push(state.logical_scroll_top());
    }
    let mut down = vec![thumb(&state)];
    let mut down_inside = vec![inside(&state)];
    let mut down_tops = vec![state.logical_scroll_top()];
    while down.last().unwrap().fraction < 1. && down.len() < 3000 {
        wheel(cx, &state, -WHEEL, by_bar);
        for _ in 0..frames {
            draw(cx);
        }
        down.push(thumb(&state));
        down_inside.push(inside(&state));
        down_tops.push(state.logical_scroll_top());
    }
    let up_s = steadiness(&up, false);
    let down_s = steadiness(&down, true);
    eprintln!("{label} up:   {up_s:?}");
    eprintln!("{label} down: {down_s:?}");
    if std::env::var("INDEX").is_ok() {
        // A thumb placed by the item index alone (the item at the top, and
        // the share of it scrolled past), as a list that scrolls by item
        // would place it: no estimates, so no corrections.
        let by_index = |thumbs: &[Thumb], tops: &[gpui::ListOffset]| {
            thumbs
                .iter()
                .zip(tops)
                .map(|(thumb, top)| {
                    let within = if top.item_ix < heights.len() {
                        top.offset_in_item.0 / heights[top.item_ix]
                    } else {
                        0.
                    };
                    let fraction = ((top.item_ix as f32 + within) / heights.len() as f32).min(1.);
                    Thumb {
                        top: fraction * (HEIGHT - thumb.length),
                        fraction,
                        ..*thumb
                    }
                })
                .collect::<Vec<_>>()
        };
        let up_ix = by_index(&up, &up_tops);
        let down_ix = by_index(&down, &down_tops);
        eprintln!("{label} index up:   {:?}", steadiness(&up_ix, false));
        eprintln!("{label} index down: {:?}", steadiness(&down_ix, true));
    }
    if std::env::var("ATTRIBUTE").is_ok() {
        attribute(&format!("{label} up"), &up, &up_inside, false);
        attribute(&format!("{label} down"), &down, &down_inside, true);
    }
    (up_s, down_s)
}

/// Without estimates, by the list's own wheel handling and by the
/// scrollbar's offset, as GPUI Kit's mask sends the wheel. The thumb never
/// steps back and reaches both ends.
#[gpui::test]
fn smooth_baseline(cx: &mut TestAppContext) {
    for (label, by_bar) in [("baseline-list", false), ("baseline-bar", true)] {
        let (up, down) = run_transcript(cx, label, by_bar, None);
        assert_eq!(
            (up.backwards, down.backwards),
            (0, 0),
            "{label}: {up:?} {down:?}"
        );
        assert!(down.spike < 1.01, "{label}: {down:?}");
    }
}

/// A view of rows whose heights change, estimated by `estimate` from the
/// rows' heights now and as they were at first.
struct ChangingView {
    state: ListState,
    heights: Rc<RefCell<Vec<f32>>>,
    first: Rc<Vec<f32>>,
    estimate: fn(usize, &[f32], &[f32]) -> f32,
}

impl Render for ChangingView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let heights = self.heights.clone();
        let estimates = self.heights.clone();
        let first = self.first.clone();
        let estimate = self.estimate;
        list(self.state.clone(), move |ix, _, _| {
            div().h(px(heights.borrow()[ix])).w_full().into_any()
        })
        .estimate_item_heights(move |ix, _, _, _| px(estimate(ix, &estimates.borrow(), &first)))
        .w_full()
        .h_full()
    }
}

/// A list of the transcript's rows that change, at the end, following it,
/// and a way to draw it.
fn changing_list(
    cx: &mut TestAppContext,
    estimate: fn(usize, &[f32], &[f32]) -> f32,
) -> (
    &mut gpui::VisualTestContext,
    ListState,
    Rc<RefCell<Vec<f32>>>,
    gpui::Entity<ChangingView>,
) {
    let cx = cx.add_empty_window();
    let rows = transcript_rows(60);
    let state = ListState::new(rows.len(), crate::ListAlignment::Top, px(OVERDRAW));
    state.set_follow_mode(FollowMode::Tail);
    let heights = Rc::new(RefCell::new(rows.clone()));
    let view = cx.update(|_, cx| {
        cx.new(|_| ChangingView {
            state: state.clone(),
            heights: heights.clone(),
            first: Rc::new(rows),
            estimate,
        })
    });
    (cx, state, heights, view)
}

/// The rows' heights now, which an app that estimates from the rows'
/// content knows.
fn estimate_now(ix: usize, now: &[f32], _: &[f32]) -> f32 {
    now[ix]
}

/// The rows' heights as they were at first, as an app whose estimates don't
/// follow the rows' content would give.
fn estimate_first(ix: usize, _: &[f32], first: &[f32]) -> f32 {
    first[ix]
}

/// The last row grows a line a step, as a reply streams in, while the reader
/// scrolls up away from it, then holds still, then scrolls back down to it.
/// Each growth reaches the list as `remeasure_items` for the row, which asks
/// for its estimate again. An estimate that follows the row's text counts
/// it as it grows, and an estimate that doesn't leaves the list with the
/// row's old height until it draws the row again. The other rows are
/// estimated exactly, so what's left is the growing row's doing.
#[gpui::test]
fn smooth_streaming(cx: &mut TestAppContext) {
    for (label, follows) in [("stream-old-height", false), ("stream-estimate", true)] {
        let estimate = if follows {
            estimate_now
        } else {
            estimate_first
        };
        let (cx, state, heights, view) = changing_list(cx, estimate);
        let draw = |cx: &mut gpui::VisualTestContext| {
            cx.draw(
                point(px(0.), px(0.)),
                size(px(WIDTH), px(HEIGHT)),
                |_, _| view.clone().into_any_element(),
            );
        };
        draw(cx);
        let last = heights.borrow().len() - 1;
        let grow = |state: &ListState| {
            heights.borrow_mut()[last] += 24.;
            state.remeasure_items(last..last + 1);
        };

        // Away from the end first, so the growing row is off screen.
        for _ in 0..40 {
            wheel(cx, &state, WHEEL, true);
            draw(cx);
        }
        // Up while the row grows.
        let mut up = vec![thumb(&state)];
        for _ in 0..150 {
            grow(&state);
            wheel(cx, &state, WHEEL, true);
            draw(cx);
            up.push(thumb(&state));
        }
        // Holding still while the row grows.
        let held = thumb(&state);
        let (mut moved, mut resized) = (0f32, 0f32);
        for _ in 0..100 {
            grow(&state);
            draw(cx);
            let now = thumb(&state);
            moved = moved.max((now.top - held.top).abs());
            resized = resized.max((now.length - held.length).abs());
        }
        // Back down to the end, over the grown row.
        let mut down = vec![thumb(&state)];
        while down.last().unwrap().fraction < 1. && down.len() < 3000 {
            wheel(cx, &state, -WHEEL, true);
            draw(cx);
            down.push(thumb(&state));
        }
        let up_s = steadiness(&up, false);
        let down_s = steadiness(&down, true);
        eprintln!("{label} up while growing:   {up_s:?}");
        eprintln!(
            "{label} held while growing: thumb moved {moved:.2}px, length changed {resized:.2}px over 100 frames (2400px of growth)"
        );
        eprintln!("{label} down afterwards:    {down_s:?}");
        assert!(up_s.spike < 1.01, "{label}: {up_s:?}");
        assert_eq!(
            moved, 0.,
            "{label}: the thumb moved while the reader held still"
        );
        if follows {
            // Counted as it grows, the row never surprises the thumb.
            assert!(
                down_s.spike < 1.01 && down_s.backwards == 0,
                "{label}: {down_s:?}"
            );
        } else {
            // Counted only once drawn, it does.
            assert!(down_s.backwards > 0, "{label}: {down_s:?}");
        }
    }
}

/// A zoom of 125% midway up the list. Every row grows by a quarter, which
/// the app tells the list of with `remeasure()`, and the list asks for every
/// estimate again. Estimates in line units rescale to the new zoom, exact
/// by construction, and estimates that don't follow the zoom leave the
/// list with the old heights until it draws each row again.
#[gpui::test]
fn smooth_zoom(cx: &mut TestAppContext) {
    for (label, follows) in [("zoom-old-heights", false), ("zoom-line-units", true)] {
        let estimate = if follows {
            estimate_now
        } else {
            estimate_first
        };
        let (cx, state, heights, view) = changing_list(cx, estimate);
        let draw = |cx: &mut gpui::VisualTestContext| {
            cx.draw(
                point(px(0.), px(0.)),
                size(px(WIDTH), px(HEIGHT)),
                |_, _| view.clone().into_any_element(),
            );
        };
        draw(cx);
        // Halfway up, then the zoom.
        let mut before = vec![thumb(&state)];
        for _ in 0..300 {
            wheel(cx, &state, WHEEL, true);
            draw(cx);
            before.push(thumb(&state));
        }
        for h in heights.borrow_mut().iter_mut() {
            *h *= 1.25;
        }
        state.remeasure();
        draw(cx);
        let drawn = thumb(&state);
        let mut after = vec![thumb(&state)];
        while after.last().unwrap().fraction > 0. && after.len() < 3000 {
            wheel(cx, &state, WHEEL, true);
            draw(cx);
            after.push(thumb(&state));
        }
        let truth: f32 = heights.borrow().iter().sum();
        eprintln!(
            "{label}: content {:.0}px before the zoom, {:.0} once drawn, {:.0} at the top (true {truth:.0}); thumb top {:.1} -> {:.1}, length {:.1} -> {:.1}",
            before.last().unwrap().max + HEIGHT,
            drawn.max + HEIGHT,
            after.last().unwrap().max + HEIGHT,
            before.last().unwrap().top,
            drawn.top,
            before.last().unwrap().length,
            drawn.length,
        );
        let after_s = steadiness(&after, false);
        eprintln!("{label} up after the zoom: {after_s:?}");
        if follows {
            assert_eq!(
                drawn.max + HEIGHT,
                truth,
                "{label}: the range isn't exact after the zoom"
            );
            assert!(after_s.spike < 1.01, "{label}: {after_s:?}");
        } else {
            assert!(
                drawn.max + HEIGHT < truth * 0.85,
                "{label}: the old range caught up"
            );
            assert!(after_s.cv > 0.1, "{label}: {after_s:?}");
        }
    }
}

/// A short transcript, whose thumb is longer than the kit's minimum and so
/// shows its size change.
#[gpui::test]
fn smooth_short(cx: &mut TestAppContext) {
    let (plain, _) = run_transcript_of(cx, "short-baseline", 10, 1, true, None);
    let (estimated, _) = run_transcript_of(
        cx,
        "short-estimates-25pct",
        10,
        1,
        true,
        Some(Rc::new(|ix, height| height * jitter(ix, 0.25))),
    );
    assert!(
        estimated.spike < 1.3 && estimated.resizes < plain.resizes,
        "{estimated:?} against {plain:?}"
    );
}

/// Estimates for every row: exact, off by up to 10% or 25% either way, or
/// exact for fixed rows and off by up to 20% for prose.
#[gpui::test]
fn smooth_estimates(cx: &mut TestAppContext) {
    let (up, _) = run_transcript(
        cx,
        "estimates-exact",
        true,
        Some(Rc::new(|_, height| height)),
    );
    assert!(up.spike < 1.001 && up.off10 <= 1, "{up:?}");
    for pct in [0.10, 0.25] {
        let (up, _) = run_transcript(
            cx,
            &format!("estimates-{:.0}pct", pct * 100.),
            true,
            Some(Rc::new(move |ix, height| height * jitter(ix, pct))),
        );
        assert!(up.spike < 1.05 && up.cv < 0.04, "{pct}: {up:?}");
    }
    let (up, _) = run_transcript(
        cx,
        "estimates-kinds",
        true,
        Some(Rc::new(|ix, height| {
            if is_prose(height) {
                height * jitter(ix, 0.2)
            } else {
                height
            }
        })),
    );
    assert!(up.spike < 1.05 && up.cv < 0.03, "{up:?}");
}
