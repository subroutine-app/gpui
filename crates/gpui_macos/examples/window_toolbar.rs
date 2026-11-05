//! macOS 26+: `cargo run -p mhr-gpui-macos --features font-kit --example window_toolbar -- --smoke`.
//! Without `--smoke` the windows stay open for review. Native tabs are off, since their strip
//! covers content drawn up to the window's top edge. This checks the window's chrome and input,
//! and measures no corner radius.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("window_toolbar requires macOS 26+");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(target_os = "macos")]
mod macos {
    use anyhow::{Result, ensure};
    use block2::RcBlock;
    use gpui::{
        App, Application, AsyncApp, Bounds, Context, MouseButton, TitlebarOptions, Window,
        WindowBounds, WindowHandle, WindowOptions, WindowToolbarStyle, div, point, prelude::*, px,
        rgb, size,
    };
    use gpui_macos::MacPlatform;
    use objc2::{
        class, msg_send,
        runtime::{AnyObject, Bool, Sel},
        sel,
    };
    use objc2_foundation::{NSOperatingSystemVersion, NSPoint, NSProcessInfo, NSRect, NSString};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::{
        ptr,
        rc::Rc,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };

    struct Demo {
        label: &'static str,
        style: Option<WindowToolbarStyle>,
        clicks: usize,
    }

    impl Render for Demo {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let header = div()
                .id("header-click")
                .h(px(40.))
                .pl(px(112.))
                .bg(rgb(0x226b60))
                .child(format!("Click header: {}", self.clicks))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.clicks += 1;
                        cx.notify();
                    }),
                );
            let fullscreen = div()
                .id("fullscreen")
                .child("Toggle fullscreen")
                .on_click(|_, window, _| window.toggle_fullscreen());
            div()
                .id("toolbar-demo")
                .size_full()
                .flex()
                .flex_col()
                .bg(rgb(0x172331))
                .text_color(rgb(0xffffff))
                .child(header)
                .child(
                    div()
                        .id("corner-content")
                        .flex_1()
                        .p_2()
                        .flex()
                        .flex_col()
                        .justify_between()
                        .child(self.label)
                        .child(fullscreen)
                        .child("Bottom edge, where the corners clip"),
                )
        }
    }

    /// The `NSView` that GPUI draws the window into.
    fn surface(window: &Window) -> *mut AnyObject {
        let RawWindowHandle::AppKit(handle) = HasWindowHandle::window_handle(window)
            .expect("a live window handle")
            .as_raw()
        else {
            unreachable!("a macOS window")
        };
        handle.ns_view.as_ptr().cast()
    }

    fn native_window(window: &Window) -> *mut AnyObject {
        // SAFETY: The view belongs to a live GPUI window, borrowed on the main thread.
        unsafe { msg_send![surface(window), window] }
    }

    fn inspect(window: &Window, style: Option<WindowToolbarStyle>, fullscreen: bool) -> Result<()> {
        // SAFETY: Every object is borrowed from a live GPUI window on the main thread.
        unsafe {
            let view = surface(window);
            let native = native_window(window);
            let toolbar: *mut AnyObject = msg_send![native, toolbar];
            ensure!(toolbar.is_null() == style.is_none(), "toolbar presence");
            if let Some(style) = style {
                // NSWindowToolbarStyleUnified and NSWindowToolbarStyleUnifiedCompact.
                let expected = match style {
                    WindowToolbarStyle::Unified => 3isize,
                    WindowToolbarStyle::UnifiedCompact => 4,
                };
                let selected: isize = msg_send![native, toolbarStyle];
                let items: *mut AnyObject = msg_send![toolbar, items];
                let count: usize = msg_send![items, count];
                let visible: Bool = msg_send![toolbar, isVisible];
                ensure!(
                    selected == expected && count == 0,
                    "toolbar style and items"
                );
                ensure!(visible.as_bool() != fullscreen, "toolbar visibility");
            }
            let tabs: *mut AnyObject = msg_send![native, tabbedWindows];
            ensure!(tabs.is_null(), "native tabs must be off");
            let content: *mut AnyObject = msg_send![native, contentView];
            let bounds: NSRect = msg_send![content, bounds];
            let frame: NSRect = msg_send![view, frame];
            let viewport = window.viewport_size();
            ensure!(
                frame.origin.x == bounds.origin.x
                    && frame.origin.y == bounds.origin.y
                    && frame.size.width == bounds.size.width
                    && frame.size.height == bounds.size.height,
                "the surface doesn't cover the content view"
            );
            ensure!(
                (f64::from(viewport.width) - bounds.size.width).abs() < 1.
                    && (f64::from(viewport.height) - bounds.size.height).abs() < 1.,
                "GPUI's viewport is stale"
            );
            ensure!(window.is_fullscreen() == fullscreen, "fullscreen state");
            let transparent: Bool = msg_send![native, titlebarAppearsTransparent];
            ensure!(transparent.as_bool() != fullscreen, "titlebar transparency");

            // A point in the header, 20px below the top edge, must reach GPUI's view.
            let flipped: Bool = msg_send![view, isFlipped];
            let y = if flipped.as_bool() {
                20.
            } else {
                bounds.size.height - 20.
            };
            let local = NSPoint::new(200., y);
            let root: *mut AnyObject = msg_send![content, superview];
            let parent: *mut AnyObject = msg_send![root, superview];
            let in_parent: NSPoint = msg_send![view, convertPoint: local, toView: parent];
            let hit: *mut AnyObject = msg_send![root, hitTest: in_parent];
            ensure!(hit == view, "the native titlebar covers GPUI's header");
            let in_window: NSPoint =
                msg_send![view, convertPoint: local, toView: ptr::null_mut::<AnyObject>()];
            let on_screen: NSPoint = msg_send![native, convertPointToScreen: in_window];
            let topmost: isize = msg_send![
                class!(NSWindow),
                windowNumberAtPoint: on_screen,
                belowWindowWithWindowNumber: 0isize
            ];
            let own: isize = msg_send![native, windowNumber];
            ensure!(
                topmost == own,
                "another window, such as a toolbar's, takes the input"
            );

            let layout: NSRect = msg_send![native, contentLayoutRect];
            let coordinates = |r: NSRect| [r.origin.x, r.origin.y, r.size.width, r.size.height];
            println!(
                "{style:?} fullscreen={fullscreen}: surface={:?}, layout={:?}",
                coordinates(frame),
                coordinates(layout)
            );
        }
        Ok(())
    }

    /// Sets a flag when the window finishes entering or leaving fullscreen.
    struct Completion(*mut AnyObject, Arc<AtomicBool>);

    impl Completion {
        fn new(window: &Window, entering: bool) -> Self {
            let name = NSString::from_str(if entering {
                "NSWindowDidEnterFullScreenNotification"
            } else {
                "NSWindowDidExitFullScreenNotification"
            });
            let done = Arc::new(AtomicBool::new(false));
            let signal = done.clone();
            let block = RcBlock::new(move |_: *mut AnyObject| signal.store(true, Ordering::SeqCst));
            // SAFETY: The window is live, and the observer is retained until it is removed.
            unsafe {
                let center: *mut AnyObject = msg_send![class!(NSNotificationCenter), defaultCenter];
                let observer: *mut AnyObject = msg_send![
                    center,
                    addObserverForName: &*name,
                    object: native_window(window),
                    queue: ptr::null_mut::<AnyObject>(),
                    usingBlock: &*block
                ];
                let observer: *mut AnyObject = msg_send![observer, retain];
                Self(observer, done)
            }
        }
    }

    impl Drop for Completion {
        fn drop(&mut self) {
            // SAFETY: The observer was retained in `new`.
            unsafe {
                let center: *mut AnyObject = msg_send![class!(NSNotificationCenter), defaultCenter];
                let _: () = msg_send![center, removeObserver: self.0];
                let _: () = msg_send![self.0, release];
            }
        }
    }

    /// Calls the delegate's will-enter or will-exit method and then its failure method, as AppKit
    /// does when a transition fails. It exercises the delegate's recovery, and no transition of
    /// the system's fails.
    fn simulated_failure(window: &Window, entering: bool) -> Result<()> {
        // SAFETY: The delegate belongs to a live GPUI window, called on the main thread.
        unsafe {
            let native = native_window(window);
            let delegate: *mut AnyObject = msg_send![native, delegate];
            let failure: Sel = if entering {
                sel!(windowDidFailToEnterFullScreen:)
            } else {
                sel!(windowDidFailToExitFullScreen:)
            };
            let supported: Bool = msg_send![delegate, respondsToSelector: failure];
            ensure!(
                supported.as_bool() && window.is_fullscreen() != entering,
                "no failure callback, or the wrong fullscreen state"
            );
            let name = NSString::from_str(if entering {
                "NSWindowWillEnterFullScreenNotification"
            } else {
                "NSWindowWillExitFullScreenNotification"
            });
            let notification: *mut AnyObject = msg_send![
                class!(NSNotification),
                notificationWithName: &*name,
                object: native
            ];
            if entering {
                let _: () = msg_send![delegate, windowWillEnterFullScreen: notification];
            } else {
                let _: () = msg_send![delegate, windowWillExitFullScreen: notification];
            }
            let toolbar: *mut AnyObject = msg_send![native, toolbar];
            let visible = !toolbar.is_null() && {
                let visible: Bool = msg_send![toolbar, isVisible];
                visible.as_bool()
            };
            if entering {
                let _: () = msg_send![delegate, windowDidFailToEnterFullScreen: notification];
            } else {
                let _: () = msg_send![delegate, windowDidFailToExitFullScreen: notification];
            }
            ensure!(!visible, "the toolbar showed during the transition");
            println!("A simulated failed transition, entering={entering}, recovered");
        }
        Ok(())
    }

    async fn pause(cx: &AsyncApp, milliseconds: u64) {
        cx.background_executor()
            .timer(Duration::from_millis(milliseconds))
            .await;
    }

    async fn smoke(windows: Vec<WindowHandle<Demo>>, cx: &mut AsyncApp) -> Result<()> {
        for handle in windows {
            handle.update(cx, |_, window, _| window.activate_window())?;
            pause(cx, 400).await;
            handle.update(cx, |demo, window, _| inspect(window, demo.style, false))??;

            handle.update(cx, |_, window, _| window.resize(size(px(520.), px(340.))))?;
            pause(cx, 300).await;
            handle.update(cx, |demo, window, _| {
                ensure!(
                    window.viewport_size() == size(px(520.), px(340.)),
                    "the window didn't resize"
                );
                inspect(window, demo.style, false)?;
                simulated_failure(window, true)?;
                inspect(window, demo.style, false)
            })??;
            for entering in [true, false] {
                let completion = handle.update(cx, |_, window, _| {
                    let completion = Completion::new(window, entering);
                    window.toggle_fullscreen();
                    completion
                })?;
                let deadline = Instant::now() + Duration::from_secs(8);
                while !completion.1.load(Ordering::SeqCst) {
                    ensure!(
                        Instant::now() < deadline,
                        "the fullscreen transition didn't finish, entering={entering}"
                    );
                    pause(cx, 50).await;
                }
                pause(cx, 250).await;
                handle.update(cx, |demo, window, _| {
                    inspect(window, demo.style, entering)?;
                    if entering {
                        simulated_failure(window, false)?;
                        inspect(window, demo.style, true)?;
                    }
                    anyhow::Ok(())
                })??;
            }
        }
        Ok(())
    }

    pub fn run() -> Result<()> {
        let smoke_mode = std::env::args().skip(1).any(|arg| arg == "--smoke");
        ensure!(
            NSProcessInfo::processInfo().isOperatingSystemAtLeastVersion(
                NSOperatingSystemVersion {
                    majorVersion: 26,
                    minorVersion: 0,
                    patchVersion: 0,
                }
            ),
            "window_toolbar requires macOS 26+"
        );
        if smoke_mode {
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(60));
                eprintln!("window_toolbar smoke took more than 60 seconds");
                std::process::exit(124);
            });
        }
        Application::with_platform(Rc::new(MacPlatform::new(false))).run(move |cx: &mut App| {
            let mut windows = Vec::new();
            for (index, (label, style)) in [
                ("No toolbar", None),
                ("UnifiedCompact", Some(WindowToolbarStyle::UnifiedCompact)),
                ("Unified", Some(WindowToolbarStyle::Unified)),
            ]
            .into_iter()
            .enumerate()
            {
                let mut bounds = Bounds::centered(None, size(px(440.), px(300.)), cx);
                bounds.origin.x += px((index as f32 - 1.) * 450.);
                let handle = cx
                    .open_window(
                        WindowOptions {
                            window_bounds: Some(WindowBounds::Windowed(bounds)),
                            titlebar: Some(TitlebarOptions {
                                title: Some(label.into()),
                                appears_transparent: true,
                                traffic_light_position: Some(point(px(16.), px(16.))),
                                toolbar_style: style,
                            }),
                            tabbing_identifier: None,
                            app_owns_titlebar_drag: true,
                            ..Default::default()
                        },
                        |_, cx| {
                            cx.new(|_| Demo {
                                label,
                                style,
                                clicks: 0,
                            })
                        },
                    )
                    .expect("a demo window opens");
                windows.push(handle);
            }
            cx.activate(true);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            if smoke_mode {
                cx.spawn(async move |cx| {
                    if let Err(error) = smoke(windows, cx).await {
                        eprintln!("window_toolbar smoke FAILED: {error:#}");
                        std::process::exit(1);
                    }
                    println!("window_toolbar smoke PASSED");
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        Ok(())
    }
}
