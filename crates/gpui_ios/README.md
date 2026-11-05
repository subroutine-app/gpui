# mhr-gpui-ios

The iOS platform of GPUI, on UIKit and Metal.

It's part of [mhr-gpui](https://crates.io/crates/mhr-gpui), Mesa Hills Research's fork of
[GPUI](https://www.gpui.rs), the GPU-accelerated UI framework of the [Zed](https://zed.dev) editor.

Apps get it through `mhr-gpui-platform`. To depend on it directly, keep its library name `gpui_ios`:

```toml
[dependencies]
gpui_ios = { package = "mhr-gpui-ios", version = "0.2" }
```

The source is in [mesa-hills-research/gpui](https://github.com/mesa-hills-research/gpui). Apache-2.0, like Zed's gpui.
