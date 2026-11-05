# Using the fork

The crates are on crates.io under `mhr-` names. Each keeps its library name, so a dependency
named after the library imports as usual:

```toml
[dependencies]
gpui = { package = "mhr-gpui", version = "0.2" }
gpui_platform = { package = "mhr-gpui-platform", version = "0.2", features = ["font-kit", "wayland", "x11"] }
# For HTTP, through `App::set_http_client`
reqwest_client = { package = "mhr-reqwest-client", version = "0.2" }
```

The crates of one release pin each other exactly, so an app always gets one consistent set.
`gpui_platform` needs `font-kit` on macOS to draw text and `wayland`, `x11` or both on Linux.
Windows and the web need no features.

## Moving from gpui-pre

Swap each `gpui-pre` crate for its `mhr-` package and keep the dependency's name, for example
`gpui = { package = "gpui-pre", version = "=0.3.11" }` becomes
`gpui = { package = "mhr-gpui", version = "0.2" }`. Code needs no changes.

An app can't use both sets, since each has its own `gpui` types. Every `gpui-pre` crate leaves the
app's `Cargo.lock`, including those that other dependencies bring in, such as GPUI Kit from
crates.io, which builds on the `gpui-pre` crates. `cargo tree -i gpui-pre` should then find
nothing, and `cargo tree -i mhr-gpui` should show one `mhr-gpui`. `gpui-pre-reqwest`, GPUI Kit's
build of reqwest, stays, since `mhr-reqwest-client` uses it too.

| Package | Replaces | Directory |
|---|---|---|
| `mhr-gpui` | `gpui-pre` | `crates/gpui` |
| `mhr-gpui-apple` | `gpui-pre-apple` | `crates/gpui_apple` |
| `mhr-bench-metrics` | `gpui-pre-bench-metrics` | `crates/bench_metrics` |
| `mhr-collections` | `gpui-pre-collections` | `crates/collections` |
| `mhr-derive-refineable` | `gpui-pre-derive-refineable` | `crates/refineable/derive_refineable` |
| `mhr-http-client` | `gpui-pre-http-client` | `crates/http_client` |
| `mhr-http-client-tls` | `gpui-pre-http-client-tls` | `crates/http_client_tls` |
| `mhr-gpui-ios` | `gpui-pre-ios` | `crates/gpui_ios` |
| `mhr-gpui-linux` | `gpui-pre-linux` | `crates/gpui_linux` |
| `mhr-gpui-macos` | `gpui-pre-macos` | `crates/gpui_macos` |
| `mhr-gpui-macros` | `gpui-pre-macros` | `crates/gpui_macros` |
| `mhr-gpui-platform` | `gpui-pre-platform` | `crates/gpui_platform` |
| `mhr-refineable` | `gpui-pre-refineable` | `crates/refineable` |
| `mhr-reqwest-client` | `gpui-pre-reqwest-client` | `crates/reqwest_client` |
| `mhr-scheduler` | `gpui-pre-scheduler` | `crates/scheduler` |
| `mhr-gpui-shared-string` | `gpui-pre-shared-string` | `crates/gpui_shared_string` |
| `mhr-sum-tree` | `gpui-pre-sum-tree` | `crates/sum_tree` |
| `mhr-gpui-util` | `gpui-pre-util` | `crates/gpui_util` |
| `mhr-gpui-web` | `gpui-pre-web` | `crates/gpui_web` |
| `mhr-gpui-wgpu` | `gpui-pre-wgpu` | `crates/gpui_wgpu` |
| `mhr-gpui-windows` | `gpui-pre-windows` | `crates/gpui_windows` |
| `mhr-zlog` | `gpui-pre-zlog` | `crates/zlog` |
| `mhr-ztracing` | `gpui-pre-ztracing` | `crates/ztracing` |
| `mhr-ztracing-macro` | `gpui-pre-ztracing-macro` | `crates/ztracing_macro` |

`mhr-gpui-mobile` (`crates/gpui_mobile`) and `mhr-gpui-scroll-harness`
(`crates/gpui_scroll_harness`) are unpublished, so an app takes them from git.
`mhr-gpui-screenshot` (`crates/gpui_screenshot`) is published and has no gpui-pre counterpart.

## From git or a local checkout

A git dependency names the package and a commit:

```toml
gpui = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>", package = "mhr-gpui" }
```

For changes in progress, a `[patch.crates-io]` in the app's root `Cargo.toml` points the published
crates at a checkout. Patch every `mhr-` crate in the app's `Cargo.lock`, so none of them stays on
crates.io beside its replacement:

```toml
[patch.crates-io]
mhr-gpui          = { path = "../gpui/crates/gpui" }
mhr-gpui-platform = { path = "../gpui/crates/gpui_platform" }
mhr-gpui-macros   = { path = "../gpui/crates/gpui_macros" }
# ...one entry for every mhr- crate in Cargo.lock, with the directories above
```

Cargo caps lints for git and registry dependencies, and treats path dependencies as the app's own
code. With `RUSTFLAGS="-D warnings"`, which `actions-rust-lang/setup-rust-toolchain` sets by
default, gpui's own warnings then fail the build. Rust 1.99 warns three times about gpui's
profiler, which calls `Atomic*::fetch_update` where 1.99 prefers `try_update`. Use crates.io or git
entries in CI.
