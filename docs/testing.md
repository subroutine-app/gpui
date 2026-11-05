# Testing

## What runs here

- `cargo check --workspace` covers every library, and `cargo clippy --workspace --all-targets` adds
  the tests and examples. The crates need Rust 1.98.
- `cargo test --workspace --tests` runs the tests of every crate, those of the gpui crates and the
  mobile crate included. `mhr-gpui-macos` and `mhr-gpui-apple` have tests only on a Mac.
- `cargo test -p mhr-gpui-screenshot` runs the screenshot harness's tests and its goldens of
  plain GPUI elements, on Linux.
- CI runs clippy and all of those tests on Linux, Windows and macOS.
  [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) has the commands.
- The suite of the [GPUI Kit fork](https://github.com/mesa-hills-research/gpui_kit), about 2,650
  tests, exercises the gpui crates through GPUI Kit. Run it from a checkout of the kit whose
  `[patch.crates-io]` points at this one.

The tests of some crates include IBM Plex Sans and Lilex from `assets/fonts` at the root, under the
SIL Open Font License that sits beside them. No published crate holds them. The release tooling
leaves out every crate's `[dev-dependencies]`, so the crates whose tests need some have them again.
`mhr-gpui`'s examples build too. They use `mhr-gpui-platform` and `mhr-reqwest-client` as
dev-dependencies, by path without a version, so the published `mhr-gpui` leaves them out. A web
build of the examples would need five more of Zed's fonts than `assets/fonts` holds. The two
benchmarks of `mhr-gpui-platform`, behind `bench-support`, still don't build because they use
`criterion` and `bench_metrics`.

## Headless screenshots on Linux

GPUI's test platform can render windows for real and read the pixels back.
`HeadlessAppContext::with_platform` takes a renderer factory, and
`gpui_platform::current_headless_renderer()` returns a `WgpuHeadlessRenderer` on Linux (Metal on
macOS). It needs no GPU: Mesa's lavapipe, the CPU Vulkan driver in `mesa-vulkan-drivers`, is
enough.

With `mhr-gpui`, `mhr-gpui-platform` and `mhr-gpui-wgpu` built with their `test-support` feature:

```rust
let text_system = Arc::new(gpui_wgpu::CosmicTextSystem::new("DejaVu Sans"));
let mut cx = HeadlessAppContext::with_platform(text_system, Arc::new(()), || {
    gpui_platform::current_headless_renderer()
});
let window = cx.open_window(size(px(240.), px(100.)), |_, cx| cx.new(|_| MyView))?;
cx.run_until_parked();
let image = cx.capture_screenshot(window.into())?; // an image::RgbaImage
image.save("screenshot.png")?;
```

On a machine without a GPU, wgpu draws through lavapipe on Vulkan. The test platform renders at
scale 2, so a 240×100 window comes back as a 480×200 image, and repeated runs give the same
pixels. EGL may log "DRI2: failed to load driver" while wgpu probes its GL backend, which leaves
the Vulkan renderer unaffected.

This reaches further than desktop: Android draws through the same wgpu renderer core and the
same cosmic-text system, so screenshot tests on Linux cover Android's rendering and text. They
leave out its platform code: touch, IME, surfaces and the lifecycle.

`mhr-gpui-screenshot` builds golden-image tests on this, with bundled fonts and the software
adapter even where a GPU is present. [screenshots.md](screenshots.md) covers writing the tests and
updating their goldens.
