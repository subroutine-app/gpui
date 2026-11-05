# mhr-bench-metrics

Criterion measurements beyond wall time, such as hardware counters, `getrusage` statistics and heap allocations, from Zed's `bench_metrics` crate. The `bench-support` feature of `mhr-gpui` uses it.

It's part of [mhr-gpui](https://crates.io/crates/mhr-gpui), Mesa Hills Research's fork of
[GPUI](https://www.gpui.rs), the GPU-accelerated UI framework of the [Zed](https://zed.dev) editor.

Apps get it through `mhr-gpui`. To depend on it directly, keep its library name `bench_metrics`:

```toml
[dependencies]
bench_metrics = { package = "mhr-bench-metrics", version = "0.2" }
```

The source is in [mesa-hills-research/gpui](https://github.com/mesa-hills-research/gpui). Apache-2.0, like Zed's gpui.
