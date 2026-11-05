# gpui

This repository is Mesa Hills Research's fork of [GPUI](https://www.gpui.rs), the GPU-accelerated
UI framework of the [Zed](https://github.com/zed-industries/zed) editor, together with a mobile
platform for iOS and Android. GPUI fixes and the mobile work live here, next to each other, and
[Mesa Hills Research's fork of GPUI Kit](https://github.com/mesa-hills-research/gpui_kit)
builds on it. The crates are on crates.io as [`mhr-gpui`](https://crates.io/crates/mhr-gpui) and
its siblings, and [CHANGELOG.md](CHANGELOG.md) lists what each release changes.

It follows the `gpui-pre` crates that GPUI Kit publishes, which are snapshots of Zed's gpui cut by
GPUI Kit's release tooling. Today that is **gpui-pre 0.3.11**, from Zed
[`f816e1f9`](https://github.com/zed-industries/zed/commit/f816e1f9263e3a8ef63c4baaa02e1441ce1c6038)
(2026-11-01), the snapshot GPUI Kit 0.9.0 is built against.

| Crates | Source | State |
|---|---|---|
| `mhr-gpui` and 23 sibling crates (`crates/`) | gpui-pre 0.3.11 | Published. Unchanged apart from their names and [a few changes](docs/upstream.md#changes-to-the-gpui-crates), such as a fix for variable font weights on Linux and Android, recent files in Windows jump lists, throttled animations that share frames, list scrollbars that hold steady over rows not drawn yet, cached views that stay cached inside other cached views and keep their accessibility nodes, paint animations that don't redraw the view they are in, long lines shaped in cached segments on Linux and Android, key bindings with Shift on a symbol key that match on every platform, and test support for screenshot tests |
| `mhr-gpui-mobile` (`crates/gpui_mobile`) | [longbridge/gpui-mobile](https://github.com/longbridge/gpui-mobile) `9075e3a`, moved from gpui-pre 0.3.7 to 0.3.11 | Experimental and unpublished, see [Mobile](#mobile) |
| `mhr-gpui-screenshot` (`crates/gpui_screenshot`) | This fork | Published. Golden-image tests for GPUI views, see [Screenshot tests](#screenshot-tests) |
| `mhr-gpui-scroll-harness` (`crates/gpui_scroll_harness`) | This fork | Measures and films how smoothly a list's scrollbar moves, unpublished, see its [README](crates/gpui_scroll_harness/README.md) |

## Using it

Each crate is published as `mhr-` followed by its library name, and keeps the library name, so code
imports `gpui` and `gpui_platform` as usual:

```toml
[dependencies]
gpui = { package = "mhr-gpui", version = "0.2" }
gpui_platform = { package = "mhr-gpui-platform", version = "0.2", features = ["font-kit", "wayland", "x11"] }
```

```rust
fn main() {
    gpui_platform::application().run(|cx: &mut gpui::App| {
        // Open windows here.
    });
}
```

An app on the `gpui-pre` crates moves over by swapping each one for its `mhr-` package under the
same dependency name. [docs/using.md](docs/using.md) lists the packages and covers git and local
checkouts.

## Following upstream

Each gpui-pre release lands on the `upstream` branch as one commit holding the crates as GPUI
Kit's tooling stages them for crates.io. `main` merges `upstream` and carries the fork's names and
changes on top. [docs/upstream.md](docs/upstream.md) has the steps.

## Mobile

`mhr-gpui-mobile` implements GPUI's platform for iOS (Metal through wgpu, CoreText) and Android
(Vulkan or GLES through wgpu, cosmic-text). Upstream calls it experimental, since IME composition,
accessibility and the app lifecycle are incomplete. Its gpui dependencies come from this
workspace, so it always builds against the gpui beside it.

| Target | State |
|---|---|
| Android (`aarch64-linux-android`, `x86_64-linux-android`) | Type-checks |
| iOS (`aarch64-apple-ios`, `-sim`) | Type-checks on Linux. Linking and running need a Mac. |

[docs/mobile.md](docs/mobile.md) covers building, the gaps and the open upstream work.
[docs/testing.md](docs/testing.md) covers tests, including headless screenshots on Linux.

## Screenshot tests

`mhr-gpui-screenshot` renders GPUI views headlessly on Linux, on Mesa's software Vulkan driver
with bundled fonts, and compares them with golden PNGs. Android draws through the same renderer
and text system, so the goldens cover its rendering and text too. GPUI Kit's buttons, inputs,
list and menu have goldens in the GPUI Kit fork.

```rust
let mut app = gpui_screenshot::ScreenshotApp::new()?;
let shot = app.render_view(size(px(120.), px(40.)), 2.625, |_, cx| cx.new(|_| Badge))?;
gpui_screenshot::goldens!().assert("badge", &shot); // tests/screenshots/badge.png
```

`UPDATE_GOLDENS=1 cargo test` rewrites goldens after an intended change.
[docs/screenshots.md](docs/screenshots.md) covers writing the tests, reviewing failures, running
them on a remote machine and why the pixels repeat exactly.

## License

The gpui crates are Apache-2.0, like Zed's gpui. Each carries `LICENSE-APACHE`, and the files that
the release tooling rewrites or this fork changes say so in their first line. `mhr-gpui-mobile`
keeps upstream's choice of GPL-3.0-or-later, AGPL-3.0-or-later or Apache-2.0. `LICENSE-GPL` at the
root comes over from Zed's root with the release tooling and applies to none of the gpui crates.

`assets/fonts` holds IBM Plex Sans and Lilex, which some tests include. They are under the
SIL Open Font License 1.1 (see the licence files beside them), and no published crate contains
them. `mhr-gpui-screenshot` bundles Roboto under the same license and DejaVu Sans Mono under the
Bitstream Vera license, with their licence files in `crates/gpui_screenshot/fonts`.
