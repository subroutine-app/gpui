# mhr-http-client-tls

The rustls client configuration of `mhr-reqwest-client`, from Zed's `http_client_tls` crate.

It's part of [mhr-gpui](https://crates.io/crates/mhr-gpui), Mesa Hills Research's fork of
[GPUI](https://www.gpui.rs), the GPU-accelerated UI framework of the [Zed](https://zed.dev) editor.

Apps get it through `mhr-gpui`. To depend on it directly, keep its library name `http_client_tls`:

```toml
[dependencies]
http_client_tls = { package = "mhr-http-client-tls", version = "0.2" }
```

The source is in [mesa-hills-research/gpui](https://github.com/mesa-hills-research/gpui). Apache-2.0, like Zed's gpui.
