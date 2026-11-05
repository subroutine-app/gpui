//! A drag that waits for a held press, through `drag_hold`. Time is the test executor's clock.

use std::{cell::Cell, rc::Rc, time::Duration};

use crate::{
    self as gpui, AppContext, Context, InteractiveElement, IntoElement, Modifiers, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, Render,
    StatefulInteractiveElement, Styled, TestAppContext, VisualTestContext, Window, div, point, px,
};

struct Grab;

impl Render for Grab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// A 40px square that clicks, and drags after a press held for 200 ms.
struct HeldDrag {
    clicks: Rc<Cell<usize>>,
    grabbed_at: Rc<Cell<Option<Point<Pixels>>>>,
}

impl Render for HeldDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let clicks = self.clicks.clone();
        let grabbed_at = self.grabbed_at.clone();
        div().size_full().child(
            div()
                .id("held")
                .size(px(40.))
                .on_click(move |_, _, _| clicks.set(clicks.get() + 1))
                .on_drag(Grab, move |_, offset, _, cx| {
                    grabbed_at.set(Some(offset));
                    cx.new(|_| Grab)
                })
                .drag_hold(Duration::from_millis(200)),
        )
    }
}

#[gpui::test]
fn a_held_drag_waits_for_the_hold_and_keeps_the_press_point(cx: &mut TestAppContext) {
    let clicks = Rc::new(Cell::new(0));
    let grabbed_at = Rc::new(Cell::new(None));
    let (_, cx) = cx.add_window_view(|_, _| HeldDrag {
        clicks: clicks.clone(),
        grabbed_at: grabbed_at.clone(),
    });
    cx.run_until_parked();

    let at = |x: f32, y: f32| point(px(x), px(y));
    let down = |cx: &mut VisualTestContext, position| {
        cx.simulate_event(MouseDownEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
    };
    let moved = |cx: &mut VisualTestContext, position| {
        cx.simulate_event(MouseMoveEvent {
            position,
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::none(),
        });
    };
    let up = |cx: &mut VisualTestContext, position| {
        cx.simulate_event(MouseUpEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
        });
    };

    // Movement past the drag threshold during the hold is no drag, and the press still clicks.
    down(cx, at(10., 10.));
    moved(cx, at(20., 10.));
    assert!(
        !cx.update(|_, cx| cx.has_active_drag()),
        "a flick is no drag"
    );
    up(cx, at(20., 10.));
    assert_eq!(clicks.get(), 1, "a flick still clicks");

    // After the hold, the first movement drags, grabbing the element where it was pressed.
    down(cx, at(10., 10.));
    moved(cx, at(14., 10.));
    assert!(!cx.update(|_, cx| cx.has_active_drag()));
    cx.executor().advance_clock(Duration::from_millis(200));
    moved(cx, at(30., 30.));
    assert!(cx.update(|_, cx| cx.has_active_drag()));
    assert_eq!(grabbed_at.get(), Some(at(10., 10.)));
}
