# mhr-gpui-platform

Picks GPUI's platform backend for the target and creates the `Application`. Apps depend on it beside `mhr-gpui`.

It's part of [mhr-gpui](https://crates.io/crates/mhr-gpui), Mesa Hills Research's fork of
[GPUI](https://www.gpui.rs), the GPU-accelerated UI framework of the [Zed](https://zed.dev) editor.

```toml
[dependencies]
gpui = { package = "mhr-gpui", version = "0.2" }
gpui_platform = { package = "mhr-gpui-platform", version = "0.2", features = ["font-kit", "wayland", "x11"] }
```

```rust,ignore
fn main() {
    gpui_platform::application().run(|cx: &mut gpui::App| {
        // Open windows here.
    });
}
```

macOS needs `font-kit` to draw text, and Linux needs `wayland`, `x11` or both. Windows and the
web need no features. [mhr-gpui's README](https://crates.io/crates/mhr-gpui) has more.

The source is in [mesa-hills-research/gpui](https://github.com/mesa-hills-research/gpui). Apache-2.0, like Zed's gpui.
