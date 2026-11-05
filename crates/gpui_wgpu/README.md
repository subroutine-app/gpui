# mhr-gpui-wgpu

GPUI's wgpu renderer and cosmic-text text system, used on Linux and the web.

It's part of [mhr-gpui](https://crates.io/crates/mhr-gpui), Mesa Hills Research's fork of
[GPUI](https://www.gpui.rs), the GPU-accelerated UI framework of the [Zed](https://zed.dev) editor.

Apps get it through `mhr-gpui`. To depend on it directly, keep its library name `gpui_wgpu`:

```toml
[dependencies]
gpui_wgpu = { package = "mhr-gpui-wgpu", version = "0.2" }
```

The source is in [mesa-hills-research/gpui](https://github.com/mesa-hills-research/gpui). Apache-2.0, like Zed's gpui.
