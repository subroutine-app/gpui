//! The scenarios, and the wheel scrolling that drives the list through them.

use std::{rc::Rc, time::Duration, time::Instant};

use anyhow::{Context as _, Result};
use gpui::{
    AnyWindowHandle, AppContext as _, FollowMode, InputEvent as _, ListAlignment, ListOffset,
    ListState, ScrollDelta, ScrollEvent, WindowHandle, point, px, size,
};
use gpui_screenshot::ScreenshotApp;

use crate::adopt::{Facts, adopt};
use crate::transcript::{Row, transcript};
use crate::view::{Harness, OVERDRAW, PADDING, Thumb, Truth, VIEWPORT, Wheel};

/// The list's width, and the narrower one the resize scenario changes to.
pub const WIDE: f32 = 800.;
pub const NARROW: f32 = 560.;
/// How far one step of the wheel scrolls, one notch of three lines.
pub const WHEEL_STEP: f32 = 60.;
/// The time between frames, for the idle frames between steps.
pub const FRAME: Duration = Duration::from_micros(16_667);
/// A leg that hasn't reached its end after this many steps is cut short.
const MAX_STEPS: usize = 20_000;

/// Which end of the list the window opens at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    /// At the start.
    Head,
    /// At the end, following new rows, as a chat transcript opens.
    Tail,
}

/// One pass of the wheel from one end of the list to the other.
#[derive(Clone, Copy, Debug)]
pub struct Leg {
    pub up: bool,
    pub width: f32,
}

impl Leg {
    pub fn name(&self) -> String {
        format!("{}@{}", if self.up { "up" } else { "down" }, self.width)
    }
}

/// How long a scenario's transcript is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Length {
    Long,
    Medium,
    Short,
}

#[derive(Clone, Debug)]
pub struct Scenario {
    pub name: &'static str,
    pub about: &'static str,
    pub length: Length,
    pub start: Start,
    pub wheel: Wheel,
    pub legs: Vec<Leg>,
}

pub fn scenarios() -> Vec<Scenario> {
    let up = Leg {
        up: true,
        width: WIDE,
    };
    let down = Leg {
        up: false,
        width: WIDE,
    };
    vec![
        Scenario {
            name: "tail",
            about: "a long transcript opened at its end, following new rows, scrolled up to the start and back down through GPUI Kit's mask",
            length: Length::Long,
            start: Start::Tail,
            wheel: Wheel::Mask,
            legs: vec![up, down],
        },
        Scenario {
            name: "head",
            about: "a long transcript opened at its start, scrolled down to the end and back up through GPUI Kit's mask",
            length: Length::Long,
            start: Start::Head,
            wheel: Wheel::Mask,
            legs: vec![down, up],
        },
        Scenario {
            name: "medium",
            about: "as tail, on a medium transcript, where each step moves the thumb further",
            length: Length::Medium,
            start: Start::Tail,
            wheel: Wheel::Mask,
            legs: vec![up, down],
        },
        Scenario {
            name: "tail-list",
            about: "as medium, with the list's own wheel handling",
            length: Length::Medium,
            start: Start::Tail,
            wheel: Wheel::List,
            legs: vec![up, down],
        },
        Scenario {
            name: "resize",
            about: "as medium's way up, then narrower, so heights are stale, and wheel down",
            length: Length::Medium,
            start: Start::Tail,
            wheel: Wheel::Mask,
            legs: vec![
                up,
                Leg {
                    up: false,
                    width: NARROW,
                },
            ],
        },
        Scenario {
            name: "short",
            about: "as tail, on a short transcript whose thumb is longer than the shortest",
            length: Length::Short,
            start: Start::Tail,
            wheel: Wheel::Mask,
            legs: vec![up, down],
        },
    ]
}

#[derive(Clone, Debug)]
pub struct Options {
    pub long_turns: usize,
    pub medium_turns: usize,
    pub short_turns: usize,
    pub seed: u64,
    /// Frames for each step of the wheel, the one after the step and then
    /// idle ones.
    pub frames_per_step: usize,
    /// Whether idle frames redraw the window, as when an animation runs
    /// elsewhere in it. Otherwise they draw only when something asked for a
    /// frame, such as `Window::request_animation_frame`.
    pub refresh_idle: bool,
    /// Idle frames after the window opens, before the first step, as when
    /// a reader looks at the list before scrolling it.
    pub settle_frames: usize,
}

/// The list and its scrollbar after one step of the wheel.
#[derive(Clone, Copy, Debug)]
pub struct Step {
    /// The list's scroll top.
    pub top: ListOffset,
    /// The thumb as drawn in the step's last frame.
    pub thumb: Thumb,
    /// The thumb as drawn if every row's height were known.
    pub exact: Thumb,
    /// How far down the content the list is, by the rows' real heights.
    pub exact_offset: f32,
    /// How long the wheel event and the frame after it took, in ns.
    pub event_ns: u64,
    pub frame_ns: u64,
    /// How long the step's idle frames took, in ns.
    pub idle_ns: u64,
    /// How many rows the step's frames drew or measured.
    pub renders: u32,
}

/// The thumb in one frame, idle frames included.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub step: usize,
    pub thumb: Thumb,
}

pub struct LegRun {
    pub leg: Leg,
    /// The state the leg starts from, then one per step of the wheel.
    pub steps: Vec<Step>,
    pub frames: Vec<Frame>,
    /// The content's real height at the leg's width.
    pub content: f32,
}

pub struct Run {
    pub scenario: Scenario,
    pub rows: Rc<Vec<Row>>,
    pub legs: Vec<LegRun>,
    /// The bytes the process had allocated at the end, before the window
    /// closed.
    pub heap: usize,
}

/// What a filmstrip sees after each step.
pub struct Shot<'a> {
    pub leg: usize,
    pub step: usize,
    pub steps: &'a [Step],
}

pub type Film<'a> = dyn FnMut(&mut ScreenshotApp, WindowHandle<Harness>, &Shot) -> Result<()> + 'a;

/// The heights of `rows` at `width`, from laying each one out once.
pub fn measure(app: &mut ScreenshotApp, rows: &Rc<Vec<Row>>, width: f32) -> Result<Vec<f32>> {
    let truth = Truth::new(rows.clone(), px(width));
    let heights = truth.heights.clone();
    let window = app.open_window(size(px(width), px(100.)), 1., |_, cx| cx.new(|_| truth))?;
    draw(app, window.into())?;
    app.update_window(window.into(), |_, window, _| window.remove_window())?;
    app.run_until_parked();
    let heights = heights.take();
    anyhow::ensure!(heights.len() == rows.len(), "the rows weren't measured");
    Ok(heights)
}

/// An idle frame: the clock moves on by a frame, and the window draws if
/// something asked for a frame (or always, with `refresh`). Returns how long
/// it took, in ns.
fn idle_frame(app: &mut ScreenshotApp, window: AnyWindowHandle, refresh: bool) -> Result<u64> {
    app.advance_clock(FRAME);
    app.run_until_parked();
    let started = Instant::now();
    app.update_window(window, |_, window, cx| {
        window.simulate_next_frame(cx);
        if refresh {
            window.refresh();
        }
    })?;
    Ok(started.elapsed().as_nanos() as u64)
}

/// Draws a frame of `window` and returns how long it took, in ns.
pub fn draw(app: &mut ScreenshotApp, window: AnyWindowHandle) -> Result<u64> {
    app.update_window(window, |_, window, cx| {
        let started = Instant::now();
        window.draw(cx).clear(cx);
        started.elapsed().as_nanos() as u64
    })
}

pub fn rows_for(scenario: &Scenario, options: &Options) -> Rc<Vec<Row>> {
    let turns = match scenario.length {
        Length::Long => options.long_turns,
        Length::Medium => options.medium_turns,
        Length::Short => options.short_turns,
    };
    Rc::new(transcript(turns, options.seed))
}

/// Scrolls through `scenario` with the wheel, one leg after the other.
/// `film` sees every step, and `hud_height` makes room for its caption.
pub fn run(
    app: &mut ScreenshotApp,
    scenario: &Scenario,
    options: &Options,
    hud_height: f32,
    mut film: Option<&mut Film>,
) -> Result<Run> {
    let rows = rows_for(scenario, options);
    let mut heights_at: Vec<(f32, Rc<Vec<f32>>)> = Vec::new();
    for leg in &scenario.legs {
        if !heights_at.iter().any(|(width, _)| *width == leg.width) {
            heights_at.push((leg.width, Rc::new(measure(app, &rows, leg.width)?)));
        }
    }
    let heights_for = |width: f32| {
        heights_at
            .iter()
            .find(|(at, _)| *at == width)
            .map(|(_, heights)| heights.clone())
            .unwrap()
    };

    let state = ListState::new(rows.len(), ListAlignment::Top, px(OVERDRAW));
    let first_width = scenario.legs[0].width;
    let first_heights = heights_for(first_width);
    adopt(
        &state,
        &Facts {
            rows: &rows,
            width: px(first_width),
            exact: &first_heights,
        },
    );
    if scenario.start == Start::Tail {
        state.set_follow_mode(FollowMode::Tail);
    }
    let window = app.open_window(size(px(WIDE), px(hud_height + VIEWPORT)), 1., |_, cx| {
        cx.new(|_| {
            Harness::new(
                state.clone(),
                rows.clone(),
                px(first_width),
                scenario.wheel,
                hud_height,
            )
        })
    })?;
    let any_window: AnyWindowHandle = window.into();
    draw(app, any_window)?;
    draw(app, any_window)?;
    for _ in 0..options.settle_frames {
        idle_frame(app, any_window, options.refresh_idle)?;
    }
    let (renders, draws, thumb) = app
        .update(|cx| {
            window
                .read(cx)
                .map(|view| (view.renders.clone(), view.draws.clone(), view.thumb.clone()))
        })
        .context("the harness window closed")?;

    let mut legs = Vec::new();
    for (leg_ix, leg) in scenario.legs.iter().enumerate() {
        let heights = heights_for(leg.width);
        let content = heights.iter().sum::<f32>() + PADDING * 2.;
        let prefix: Vec<f64> = std::iter::once(0.)
            .chain(heights.iter().scan(0f64, |sum, height| {
                *sum += *height as f64;
                Some(*sum)
            }))
            .collect();
        let exact_max = (content - VIEWPORT).max(0.);
        let exact_at = |top: ListOffset| {
            let offset = prefix[top.item_ix.min(rows.len())] as f32 + f32::from(top.offset_in_item);
            (offset, Thumb::place(offset, exact_max))
        };

        let current = app.update(|cx| window.read(cx).map(|view| view.width))?;
        if f32::from(current) != leg.width {
            app.update(|cx| {
                window.update(cx, |view, _, cx| {
                    view.width = px(leg.width);
                    cx.notify();
                })
            })?;
            adopt(
                &state,
                &Facts {
                    rows: &rows,
                    width: px(leg.width),
                    exact: &heights,
                },
            );
            draw(app, any_window)?;
            draw(app, any_window)?;
        }

        let begin = |thumb: Thumb| {
            let top = state.logical_scroll_top();
            let (exact_offset, exact) = exact_at(top);
            Step {
                top,
                thumb,
                exact,
                exact_offset,
                event_ns: 0,
                frame_ns: 0,
                idle_ns: 0,
                renders: 0,
            }
        };
        let mut steps = vec![begin(thumb.get())];
        let mut frames = vec![Frame {
            step: 0,
            thumb: thumb.get(),
        }];
        let dy = if leg.up { WHEEL_STEP } else { -WHEEL_STEP };
        let position = point(px(leg.width / 2.), px(hud_height + VIEWPORT / 2.));
        while steps.len() <= MAX_STEPS {
            renders.set(0);
            draws.set(0);
            // In GPUI's test mode a window that an update leaves dirty is
            // drawn as the update's effects are flushed, so the frame after
            // the wheel event is drawn within this update, after the
            // callbacks the last frame asked to run before the next one.
            app.advance_clock(FRAME);
            app.run_until_parked();
            let started = Instant::now();
            let event_ns = app.update_window(any_window, |_, window, cx| {
                window.simulate_next_frame(cx);
                let started = Instant::now();
                window.dispatch_event(
                    ScrollEvent {
                        position,
                        delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
                        ..Default::default()
                    }
                    .to_platform_input(),
                    cx,
                );
                started.elapsed().as_nanos() as u64
            })?;
            let mut frame_ns = (started.elapsed().as_nanos() as u64).saturating_sub(event_ns);
            app.run_until_parked();
            if draws.get() == 0 {
                frame_ns = draw(app, any_window)?;
            }
            let step_ix = steps.len();
            frames.push(Frame {
                step: step_ix,
                thumb: thumb.get(),
            });
            let mut idle_ns = 0;
            for _ in 1..options.frames_per_step.max(1) {
                app.advance_clock(FRAME);
                app.run_until_parked();
                idle_ns += idle_frame(app, any_window, options.refresh_idle)?;
                frames.push(Frame {
                    step: step_ix,
                    thumb: thumb.get(),
                });
            }
            let step = Step {
                event_ns,
                frame_ns,
                idle_ns,
                renders: renders.get(),
                ..begin(thumb.get())
            };
            let last = steps.last().unwrap();
            let moved = step.top.item_ix != last.top.item_ix
                || (f32::from(step.top.offset_in_item) - f32::from(last.top.offset_in_item)).abs()
                    > 0.001;
            if !moved {
                // At the end of the list the step did nothing. Its frames stay
                // in the trace, so a thumb that eases can be seen settling.
                break;
            }
            steps.push(step);
            if let Some(film) = film.as_deref_mut() {
                film(
                    app,
                    window,
                    &Shot {
                        leg: leg_ix,
                        step: step_ix,
                        steps: &steps,
                    },
                )?;
            }
        }
        legs.push(LegRun {
            leg: *leg,
            steps,
            frames,
            content,
        });
    }
    let heap = crate::live_bytes();
    app.update_window(any_window, |_, window, _| window.remove_window())?;
    app.run_until_parked();
    Ok(Run {
        scenario: scenario.clone(),
        rows,
        legs,
        heap,
    })
}

/// Frame times of a list of many rows while its width changes by a pixel a
/// frame, as when a window's edge is dragged, and while nothing changes.
pub struct ResizeBench {
    pub rows: usize,
    pub estimates: String,
    /// The frames before the drag, in ms.
    pub still: Vec<f32>,
    /// The frames of the drag, in ms.
    pub drag: Vec<f32>,
}

/// Opens a list of `count` rows (the long transcript, repeated) with the
/// estimates `estimates` picks, draws frames without a change, then drags
/// its width from 800 to 700px a pixel a frame.
pub fn bench_resize(
    app: &mut ScreenshotApp,
    count: usize,
    estimates: &str,
    options: &Options,
) -> Result<ResizeBench> {
    crate::adopt::set_mode(estimates);
    let long = transcript(options.long_turns, options.seed);
    let rows: Rc<Vec<Row>> = Rc::new(long.iter().cycle().take(count).cloned().collect());
    let state = ListState::new(rows.len(), ListAlignment::Top, px(OVERDRAW));
    adopt(
        &state,
        &Facts {
            rows: &rows,
            width: px(WIDE),
            exact: &vec![0.; rows.len()],
        },
    );
    let window = app.open_window(size(px(WIDE), px(VIEWPORT)), 1., |_, cx| {
        cx.new(|_| Harness::new(state.clone(), rows.clone(), px(WIDE), Wheel::List, 0.))
    })?;
    let any_window: AnyWindowHandle = window.into();
    draw(app, any_window)?;
    draw(app, any_window)?;
    let mut still = Vec::new();
    for _ in 0..30 {
        let started = Instant::now();
        app.update_window(any_window, |_, window, _| window.refresh())?;
        still.push(started.elapsed().as_secs_f32() * 1e3);
    }
    let mut drag = Vec::new();
    for step in 1..=100 {
        let started = Instant::now();
        app.update(|cx| {
            window.update(cx, |view, _, cx| {
                view.width = px(WIDE - step as f32);
                cx.notify();
            })
        })?;
        drag.push(started.elapsed().as_secs_f32() * 1e3);
    }
    app.update_window(any_window, |_, window, _| window.remove_window())?;
    app.run_until_parked();
    Ok(ResizeBench {
        rows: count,
        estimates: estimates.to_string(),
        still,
        drag,
    })
}
