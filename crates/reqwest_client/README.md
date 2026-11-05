# mhr-reqwest-client

An HTTP client for GPUI built on reqwest, from Zed's `reqwest_client` crate. An app passes it to `App::set_http_client`.

It's part of [mhr-gpui](https://crates.io/crates/mhr-gpui), Mesa Hills Research's fork of
[GPUI](https://www.gpui.rs), the GPU-accelerated UI framework of the [Zed](https://zed.dev) editor.

```toml
[dependencies]
reqwest_client = { package = "mhr-reqwest-client", version = "0.2" }
```

The source is in [mesa-hills-research/gpui](https://github.com/mesa-hills-research/gpui). Apache-2.0, like Zed's gpui.
