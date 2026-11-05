# Mobile

`crates/gpui_mobile` is `mhr-gpui-mobile`, GPUI's platform for iOS and Android. It has the
window, input, text system, dispatcher and display for each, the `packages` modules (camera,
location, notifications and others) and an example app. It comes from
[longbridge/gpui-mobile](https://github.com/longbridge/gpui-mobile), GPUI Kit's fork of
[itsbalamurali/gpui-mobile](https://github.com/itsbalamurali/gpui-mobile).

| | iOS | Android |
|---|---|---|
| Renderer | wgpu on Metal | wgpu on Vulkan, GLES as fallback |
| Text | CoreText through font-kit | cosmic-text and swash |
| Host | UIKit, embeddable in a Swift app | `NativeActivity` or `GpuiInputActivity` |
| Minimum (upstream's README) | iOS 13 | API 26, tested on arm64 |

Upstream's README calls it experimental and lists IME composition, accessibility and the app
lifecycle hooks as incomplete. GPUI Kit's mobile guide documents the iOS simulator path and says
Android was validated for one AI chat screen.

## Status

On gpui-pre 0.3.11:

- `cargo check -p mhr-gpui-mobile --all-targets --target <target>` passes for
  `aarch64-linux-android`, `x86_64-linux-android`, `aarch64-apple-ios` and
  `aarch64-apple-ios-sim`, checked on macOS. The unit tests passed on Linux on 0.3.8. Two
  doctests are snippets without imports and fail to compile, as upstream's do.
- Up to commit `f98b12a`, the example app's `.so` built and linked with NDK r29 for both
  Android targets. It builds again once GPUI Kit builds on the `mhr-` crates (see below).
- On Linux the example fails to type-check for iOS: `ring` and `aws-lc-sys` compile C with the iOS
  SDK, and `backtrace` hits the libc break below.

Still to do: an APK, a run on a device or emulator, and anything on a Mac.

## Android

The crate type-checks with just the Rust target (`rustup target add aarch64-linux-android`). The
example also needs the NDK, because `ring` and `aws-lc-sys` compile C for the target. Point
cargo at the NDK's clang, in `.cargo/config.toml` or with `--config`:

```toml
[target.aarch64-linux-android]
linker = "<ndk>/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android26-clang"

[env]
CC_aarch64_linux_android = "<ndk>/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android26-clang"
AR_aarch64_linux_android = "<ndk>/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ar"
```

```sh
cargo build --lib --target aarch64-linux-android \
  --manifest-path crates/gpui_mobile/example/Cargo.toml
```

An APK takes `example/build.sh android`, which also needs cargo-ndk, the Android SDK and JDK 17.
The crate's [README](../crates/gpui_mobile/README.md) has the details.

The example is a workspace of its own, so it keeps its release profile. It uses GPUI Kit 0.7.1
from crates.io, which builds on the `gpui-pre` crates. Up to commit `f98b12a`, a
`[patch.crates-io]` in the example's manifest pointed those at this workspace. A patch can't
rename a crate, so with the `mhr-` names the example needs a GPUI Kit built on them, and until
then it builds from that commit.

## iOS

Building and running need a Mac with Xcode, and XcodeGen for the example. On Linux the crate
type-checks for iOS, which catches breaks in gpui's API.

libc 0.2.190 (2026-10-02) declares `_dyld_image_count` and its siblings for macOS only, and
backtrace 0.3.76, the newest release, still calls them on iOS. With a fresh lock file, backtrace
therefore fails to compile for iOS. This workspace's lock has libc 0.2.186. An app with a fresh
lock file pins libc below 0.2.190:

```sh
cargo update -p libc --precise 0.2.189
```

## Open upstream work

- [#24](https://github.com/longbridge/gpui-mobile/pull/24): Android uses the system clipboard,
  and tears down only the surface a destroy names
- [#25](https://github.com/longbridge/gpui-mobile/pull/25): Android exposes GPUI's accessibility
  tree to TalkBack
