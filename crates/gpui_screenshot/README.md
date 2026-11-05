# mhr-gpui-screenshot

Golden-image tests for GPUI views. It renders windows headlessly on Linux, on Mesa's software
Vulkan driver with bundled fonts, and compares them with PNGs committed beside your tests.

It's a companion of [mhr-gpui](https://crates.io/crates/mhr-gpui), Mesa Hills Research's fork of
[GPUI](https://www.gpui.rs), the GPU-accelerated UI framework of the [Zed](https://zed.dev) editor.
Its version matches the `mhr-gpui` it pins. Tests need Mesa's lavapipe, which is
`mesa-vulkan-drivers` on Debian and Ubuntu. Keep the library name `gpui_screenshot`:

```toml
[dev-dependencies]
gpui_screenshot = { package = "mhr-gpui-screenshot", version = "0.2" }
```

```rust
use gpui::{px, size};

#[test]
fn badge() {
    let mut app = gpui_screenshot::ScreenshotApp::new().unwrap();
    let shot = app
        .render_view(size(px(120.), px(40.)), 2., |_, cx| cx.new(|_| Badge))
        .unwrap();
    gpui_screenshot::goldens!().assert("badge", &shot); // tests/screenshots/badge.png
}
```

`UPDATE_GOLDENS=1 cargo test` writes the goldens that are missing or changed.
[docs/screenshots.md](https://github.com/mesa-hills-research/gpui/blob/main/docs/screenshots.md)
covers writing the tests, reviewing failures and running them on a remote machine.

The source is in [mesa-hills-research/gpui](https://github.com/mesa-hills-research/gpui). The code
is Apache-2.0. Roboto is under the SIL Open Font License and DejaVu Sans Mono under the Bitstream
Vera license, both in `fonts/`.
