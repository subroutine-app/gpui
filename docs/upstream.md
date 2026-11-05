# Following upstream

## Where the gpui crates come from

Zed publishes its `gpui` crate to crates.io only now and then. GPUI Kit publishes snapshots of it
instead, as the `gpui-pre-*` crates. Its "Release GPUI" workflow runs
[`script/bump-gpui.ts`](https://github.com/longbridge/gpui-kit/blob/main/script/bump-gpui.ts)
every Monday and by hand. For 0.3.8 the script:

- took Zed's `gpui`, `gpui_platform`, `gpui_macros` and `reqwest_client` and the 21 workspace
  crates they need
- renamed each to `gpui-pre-*`, kept the Zed name as the `[lib]` name, gave all 25 the version
  0.3.8 and pinned them to each other with `=0.3.8`
- swapped `zed-reqwest` for `gpui-pre-reqwest` and proptest's git source for crates.io, and
  dropped `http_client`'s `github-download` feature with its git-only `async-tar`
- relaxed exact pins on outside crates to carets (`taffy`, `unicode-properties` and nine
  workspace entries)
- rewrote three things so gpui also works behind a facade crate such as `gpui-kit`:
  `gpui_macros`' 19 proc-macro entry points, the `actions!` macro, and `gpui_apple`'s build
  script, which gets the five gpui sources it reads copied under `vendor/gpui`. Each rewritten
  file starts with a "Modified for gpui-pre" line.
- left out every `[dev-dependencies]` table (some are back, see [testing.md](testing.md))
- put `LICENSE-APACHE` in every crate and Zed's root `LICENSE-*` files at the workspace root

For 0.3.11 it did the same for 24 crates. `gpui_ios` joined them, and `perf` and `util_macros`
left, since gpui no longer needs them. The script also swaps wgpu's git source for crates.io wgpu
30.0.1 and relaxes more pins. The commit of each release on `upstream` lists what its log reports.

## Bringing in a release

The worked example is 0.3.8, which needed Zed `279fe070`, gpui-kit `42fbb97` and Bun 1.4.2.

1. **Find the revisions.** Each gpui-pre crate's description on crates.io ends with the Zed
   revision, for example "(gpui-pre snapshot of zed@279fe07)", and `[package.metadata.gpui-pre]`
   in its `Cargo.toml` has it in full. The script version is the head commit of the "Release
   GPUI" run that published the release, and that run's log shows the Bun version and the
   staging output:

   ```sh
   gh api 'repos/longbridge/gpui-kit/actions/workflows/release-gpui.yml/runs?per_page=10' \
     --jq '.workflow_runs[] | [.id, .head_sha, .created_at, .conclusion] | @tsv'
   gh api repos/longbridge/gpui-kit/actions/runs/<run>/jobs --jq '.jobs[].id'
   gh api repos/longbridge/gpui-kit/actions/jobs/<job>/logs > run.log
   ```

2. **Stage the crates** with that script and that Zed revision:

   ```sh
   git init -q zed && git -C zed fetch -q --depth 1 https://github.com/zed-industries/zed <zed-rev>
   git -C zed checkout -q --detach FETCH_HEAD
   mkdir -p bump/script
   git -C <gpui-kit clone> show <kit-commit>:script/bump-gpui.ts > bump/script/bump-gpui.ts
   (cd bump && bun script/bump-gpui.ts <version> --zed ../zed --stage-only)
   ```

   The output should match the run's log, apart from the "published files changed since" line,
   which an explicit version skips. The staged workspace is
   `bump/target/gpui-pre/workspace`.

3. **Commit it on `upstream`**, replacing the whole tree:

   ```sh
   git switch upstream
   git rm -rq .
   (cd bump/target/gpui-pre/workspace && tar --exclude=./target -cf - .) | tar -xf -
   git add -A --force -- . ':!target'
   git commit -m "gpui-pre <version>, as released (Zed <short rev>)"
   ```

   `--force` keeps files that a crate's own `.gitignore` would hide, and `':!target'` keeps a
   local build out. The commit message records the revisions and the command, as the 0.3.8
   commit does.

4. **Compare it with crates.io** from a checkout of `main`, against a worktree of `upstream`:

   ```sh
   git worktree add --detach ../gpui-upstream upstream
   script/verify-upstream.sh -C ../gpui-upstream
   ```

   The script packages each gpui-pre crate and compares it with the published one. Every crate
   should match.

5. **Merge into `main`** with `git merge upstream`. Conflicts mark lines the fork changed too,
   listed in [Changes to the gpui crates](#changes-to-the-gpui-crates) and
   [The fork's names](#the-forks-names). In each `Cargo.toml`, keep `main`'s package name,
   version and crates.io metadata, and take upstream's dependencies and features. If
   `Cargo.lock` conflicts, take upstream's and let cargo rename the crates and add
   `mhr-gpui-mobile`'s dependencies back with `cargo metadata --format-version 1 > /dev/null`.
   Then:
   - fix `mhr-gpui-mobile` where gpui's API moved. Its gpui dependencies follow the workspace
     version on their own, and breaks show up in the checks in [mobile.md](mobile.md).
   - run the GPUI Kit fork's tests against the new `main`
   - list the files the fork has changed with `git diff --stat upstream main -- crates tooling`

## The fork's names

The crates are published as `mhr-` followed by their library names, at versions of their own
starting from 0.1.0, because the `gpui-pre` names belong to GPUI Kit on crates.io. Each crate's
`Cargo.toml` has its own name, version, description, repository, keywords, categories and README,
and the workspace sets `rust-version` and pins the crates to each other with `=`.
`[package.metadata.gpui-pre]` keeps the Zed crate and revision each one comes from.

## Publishing

Each crate has its own version in its `Cargo.toml`, and the `=` pins in the workspace's
`[workspace.dependencies]` follow them. A release gives a new version to every crate that changed
since the last one and to every crate that pins one of those, and leaves the other crates alone.
Version 0.1.1 covered eight crates, since `mhr-gpui` and four platform crates changed and three
more pin them. Version 0.1.2 covers the same eight, since `mhr-gpui`, `mhr-gpui-apple` and
`mhr-gpui-wgpu` changed and the other five pin `mhr-gpui`. So does 0.1.3, where only `mhr-gpui`
changed. The other seven pin it exactly, so each needs a new version for its pin to follow. The
same goes for 0.1.4 and 0.1.5. Version 0.1.6 covers ten, since `mhr-gpui-macros` changed and
`mhr-gpui` pins it, and `mhr-gpui-screenshot` was published for the first time and pins `mhr-gpui`
and `mhr-gpui-wgpu`. Version 0.1.7 covers the same ten, since `mhr-gpui`, `mhr-gpui-apple`
and `mhr-gpui-macros` changed and the rest pin `mhr-gpui`. So does 0.1.8, for the same reasons.
Version 0.1.9 covers nine, since only `mhr-gpui` changed and the other eight pin it.
`mhr-gpui-macros` pins nothing that changed, so it stays at 0.1.8 and the others pin it there.
The rest stay at 0.1.0, and the new versions pin them at `=0.1.0`.
A new gpui-pre release changes every crate, so it starts a new minor line for all of them.
Version 0.2.0 brings in gpui-pre 0.3.11 and covers all 25 published crates, `mhr-gpui-ios`
among them for the first time, and `mhr-perf` and `mhr-util-macros` end at 0.1.0.
`git diff --stat <last release> HEAD -- crates tooling` shows what changed. Publish the crates of
the release by name, since cargo refuses a version that's already on crates.io. The command below
names all ten, so leave out `-p mhr-gpui-macros` for a release such as 0.1.9 that doesn't change
it:

```sh
packages="-p mhr-gpui-macros -p mhr-gpui -p mhr-gpui-wgpu -p mhr-gpui-linux -p mhr-gpui-windows \
  -p mhr-gpui-apple -p mhr-gpui-macos -p mhr-gpui-web -p mhr-gpui-platform -p mhr-gpui-screenshot"
cargo publish --dry-run --locked $packages
cargo publish --locked $packages
```

Cargo packages the crates, builds each package against the others, and uploads them in
dependency order. crates.io takes new versions of existing crates quickly, but it limits how fast
one account creates new crates, so a release that adds a crate may have to wait and retry.
[CHANGELOG.md](../CHANGELOG.md) lists what each release changes.

## Changes to the gpui crates

Each of upstream's files that these changes touch starts with a line saying Mesa Hills Research
modified it, as the Apache License asks of modified files. A change to another of upstream's files
adds the same line.

- **Variable font weights**, in `mhr-gpui-wgpu`'s `src/cosmic_text_system.rs`, the text system
  of Linux and Android. It chose a face for the requested weight and then drew every weight of a
  variable face at the face's default, so Bold in a variable font such as Ubuntu Sans Mono came out
  Regular. Each weight of a variable face now gets its own `FontId`, shaped and rasterized at that
  point of the `wght` axis. Italic comes from the family's italic face, and other axes (`ital`,
  `slnt`, `wdth`) stay at their defaults, since cosmic-text shapes along `wght` alone.
  `crates/gpui_screenshot/tests/variable_fonts.rs` covers it. Zed's gpui has the same bug, which
  hasn't been reported upstream yet.
- **Screenshot test support** in `mhr-gpui` and `mhr-gpui-wgpu`, behind their `test-support`
  feature. [screenshots.md](screenshots.md#changes-to-the-gpui-crates) has the details.
- **Recent files in Windows jump lists**, in `mhr-gpui`'s `src/platform.rs` and `src/app.rs` and
  `mhr-gpui-windows`' `src/destination_list.rs` and `src/platform.rs`. Zed lists folders, so
  `App::update_jump_list` titles the recent entries "Recent Folders" with a folder icon beside
  each, and it still does. `App::update_jump_list_with` takes a `JumpListRecent` that names the
  title, the icon (a folder, the app's own, or one from a file) and whether to add Windows' own
  Recent category below it. `Platform::update_jump_list` takes the `JumpListRecent`.
  `App::add_recent_document` now works on Windows too, through `SHAddToRecentDocs` with the
  process's AppUserModelID when it has one. A jump list whose recent entries Windows declines,
  because the user turned off recent items, keeps its tasks. `mhr-gpui-apple`'s copy of
  `platform.rs` under `vendor/gpui` follows the original.
- **The golden ratio**, in `mhr-gpui`'s `src/geometry.rs`. `phi()` returns
  `f32::consts::GOLDEN_RATIO` in place of the literal `1.618_034`, which is the same `f32` and
  which the clippy of Rust 1.99 rejects. `mhr-gpui-apple`'s copy under `vendor/gpui` follows it.
- **Zed's forks from crates.io**, in the workspace's `Cargo.toml` and those of `mhr-gpui-linux`
  and `mhr-gpui-web`'s `hello_web` example. Zed's manifests name its forks of font-kit, xim, scap,
  oo7 and wasm_thread by git revision beside a crates.io version, and a published crate gets the
  crates.io release. This workspace names only the crates.io releases, so its builds and tests use
  what the published crates get. Since 0.3.11, font-kit, oo7 and wasm_thread are named in the
  workspace's `[workspace.dependencies]`. font-kit, xim and scap have the same sources as the
  revisions. oo7, which only Linux uses, is 0.6 from crates.io, as the published crates get it,
  and hasn't been compared with Zed's revision. In wasm_thread 0.3.3
  the web workers start wasm-bindgen with positional arguments, which wasm-bindgen still accepts
  with a deprecation warning in the console. A merge from `upstream` brings the git revisions back
  where upstream changed those lines.
- **Throttled animations share frames**, in `mhr-gpui`'s `src/elements/animation.rs` and
  `src/window.rs`. `Animation::with_max_fps` scheduled each animation's next render on a timer of
  its own, so animations started at different times drew in different frames, and two spinners
  capped at 30 fps drew 60 frames a second. Renders now fall on a grid of `1 / max_fps` from the
  app's animation epoch, and the throttled animations of a window that wait for the same point of
  the grid share one timer. `mhr-gpui-apple`'s copy of `window.rs` under `vendor/gpui` follows it.
- **List scrollbars that hold steady** over items not drawn yet, in `mhr-gpui`'s
  `src/elements/list.rs`. `ListState` counted an item it hadn't measured as 0px tall, so the
  scrollbar's range grew as the list drew items. While the reader scrolled, the thumb shrank and
  jumped back, a dragged thumb slid away from the pointer, and a thumb dragged to the top stopped
  short of the first item. An item not measured yet now counts its size hint, or else the mean
  height of the items whose height is known, and an item replaced one for one, or invalidated by a
  width change, keeps its old height as its estimate. The thumb moves with the list in proportion
  to the distance left to the end it moves toward, so measured items change its speed instead of
  its place, and both ends of the track are the ends of the list. Its speed changes by at most half
  of itself over a viewport's height of scrolling, so a correction spreads over the following
  steps, and a thumb that leads or trails the list by too much is pulled back gradually instead of
  clamped at once. The thumb never leaves its track, whatever speed it carries. A dragged thumb
  keeps its size and stays under the pointer. Wheel and track scrolling through
  `set_offset_from_scrollbar` moves the list by the distance asked. The list's own wheel handler
  scrolls from where the list is when the turn comes, not from where it painted. The tests at the
  end of `list.rs` cover it.
- **Height estimates for list items not drawn yet**, in `mhr-gpui`'s `src/elements/list.rs`.
  `List::estimate_item_heights` takes a closure that estimates an item's height at a width from
  what's known about it, such as its lines of text times their height. The scrollbar counts the
  estimates for the items the list hasn't drawn, so its thumb moves evenly and keeps its size as
  the reader scrolls onto new items. The list asks for estimates of new items, of every item after
  `ListState::remeasure`, and of every item when its width changes. An item measured at the old
  width then counts its height scaled by its estimate at the new width over its estimate at the
  old one. Items that change, after `ListState::remeasure_items` or a one-for-one `splice`, get a
  new estimate too, so a row that grows off screen counts its new height. They count the height
  they had until the next frame makes the estimate, because the mean height of the other items
  would shorten the scroll range for a moment and put the reader's place beyond its end. The
  scrollbar works out a position from items that wait for an estimate, but doesn't keep it as the
  place its thumb moves on from. Upstream 0.3.11 keeps the list's scroll top and the adjustment
  a remeasure leaves for it in one scroll anchor, and lets an app mark measurements provisional
  with `ListState::set_item_measurement_readiness`. The scrollbar sets the anchor like any scroll,
  and an item measured provisionally counts the height it measured, with no new estimate. The tests at the end of `list.rs` cover it, and those in
  `src/elements/list_smooth_tests.rs` measure how steadily the thumb moves over a transcript of
  mixed rows, with and without estimates.
- **Cached views inside cached views stay cached**, in `mhr-gpui`'s `src/view.rs`,
  `src/window.rs` and `src/text_system/line_layout.rs`. A cached view that redrew made every
  cached view inside it redraw too, so caching a view inside another gave nothing. Such a view is
  now reused when the view around it redraws, unless it was notified or its bounds, content mask
  or text style changed. The cached views inside a view reused whole move with it in the frame,
  and a view is reused only from what it drew in the previous frame, so a cached view under an
  element that was hidden for a frame draws again when it shows. The tests at the end of
  `view.rs` cover it. `mhr-gpui-apple`'s copy of `window.rs` under `vendor/gpui` follows it.
- **Text measured for its natural width**, in `mhr-gpui`'s `src/elements/text.rs`. A text
  element asked for its size without a width answered with the size it last wrapped to, so a
  box as wide as its text stayed as narrow as some earlier layout made it. It now lays the text
  out without wrapping, and keeps the layout it replaced, so text measured at its natural width
  and then laid out at a narrower one, frame after frame, is laid out once at each width. The
  tests at the end of `text.rs` cover it.
- **A check for one font family**, `TextSystem::has_font_family`, in `mhr-gpui`'s
  `src/text_system.rs` and `src/platform.rs`, with lookups in `mhr-gpui-wgpu`'s
  `src/cosmic_text_system.rs`, `mhr-gpui-windows`' `src/direct_write.rs` and `mhr-gpui-macos`'
  `src/text_system.rs`, which 0.3.11 moved to `mhr-gpui-apple`'s `src/text_system.rs`. Finding
  out whether a family was installed meant listing every family
  with `all_font_names`. The new method finds the family the way a `Font`'s family is found, and
  other platform text systems search `all_font_names` by default. `mhr-gpui-apple`'s copy of
  `platform.rs` under `vendor/gpui` follows the original.
  `crates/gpui_screenshot/tests/font_families.rs` covers it on Linux.
- **Accessibility nodes of reused cached views**, in `mhr-gpui`'s `src/window.rs`, `src/view.rs`
  and `src/window/a11y.rs`. A cached view that was reused pushed no nodes, so the tree sent to the
  screen reader lacked everything in it, for top-level and nested cached views alike. The builder
  now keeps the previous frame's nodes with their depths, and `Window::prepaint_index` and
  `Window::paint_index` record where a view's nodes and action listeners sit in them. Reusing a
  view copies its range into the new frame under the node being built, with the bounds, focus
  handles, focus and active descendant that belong to it, and moves its listeners over like
  mouse listeners. A cached view isn't reused while the previous frame has no nodes, which is
  the first frame after accessibility starts. `Window::transact`, which a list uses to draw its
  items again after an item asks to scroll, takes back the nodes the first drawing pushed.
  `mhr-gpui-apple`'s copy of `window.rs` under `vendor/gpui` follows it.
- **Paint animations**, in `mhr-gpui`'s `src/elements/animation.rs`, `src/scene.rs` and
  `src/window.rs`. An animation notified the view that held the element, so each frame drew that
  view and the views above it. `AnimationExt::with_paint_animation` lays an element out and
  prepaints it once, and paints it in every frame with a `PaintEffect`, an opacity and a rotation
  of sprites, that the animation's value makes. `Scene` keeps a stack of effects. It applies them
  to the copy of a primitive that it draws, and it keeps the primitive as it was in its paint
  operations. `Frame::paint_effects` records the operations each element painted, with its timing,
  and `PaintIndex` holds a position in the records, so that `Window::reuse_paint` copies the
  records of a reused view with their operations moved, works out the effects again at the time of
  the frame, and replays the operations with each effect pushed and popped around its own. After a
  draw the window asks for the next frame, or sets a timer for the next point of a throttled
  animation's grid, while a record's animation hasn't ended, and that frame draws the root view and
  notifies no view. `mhr-gpui-apple`'s copies of `scene.rs` and `window.rs` under `vendor/gpui`
  follow them. The tests at the end of `animation.rs` and `scene.rs` cover it, and
  `crates/gpui_screenshot/tests/spinner_frames.rs` times a spinner in a pane of rows.
- **Paint effects that move**, in `mhr-gpui`'s `src/scene.rs`, `src/window.rs` and
  `src/elements/animation.rs`. `PaintEffect` gained a `translation` in logical pixels. `Scene`
  adds the translations of the effects around a primitive and moves the primitive's bounds by
  them: `bounds` of every kind, `element_bounds` of a shadow and the vertices of a path. A sprite
  that has a transformation, which a rotation already turned, gets the translation in the
  transformation, applied after the rotation, so a nested effect's matrices compose exactly. The
  content mask of a primitive stays. Culling and the draw order use the place a primitive is
  drawn at, and a primitive that an effect hides is kept in the paint operations, so a cached view
  that replays them with another translation shows it. `Scene::push_effect` takes the window's
  scale factor to scale the translation, and the two callers in `window.rs` pass it.
  `mhr-gpui-apple`'s copies of `scene.rs` and `window.rs` under `vendor/gpui` follow them. The
  tests at the end of `scene.rs` and `animation.rs` cover it, among them a cached view that
  replays a nested slide frame after frame.
- **Cached list items of measured height**, in `mhr-gpui`'s `src/view.rs`, `src/element.rs` and
  `src/window.rs`. `Entity::cached_measured` and `AnyView::cached_measured` embed a view that
  takes the height it measured, where `cached` takes its size from a style. Since 0.3.11 a cached
  view's key holds the opacity it painted under, and a view of measured height keeps it too. `Drawable::layout_as_root`
  hands a view at the root of the layout the space it is laid out in, and the view reuses the
  height it measured when it is clean, was measured at the same width, text style and rem size,
  and the window isn't refreshing. Its layout is then a leaf of that height. Otherwise it renders
  while it is laid out, inside a box of its style, and takes its height from where the layout puts
  the box. `ViewElementState` keeps what the view measured. The tests at the end of `view.rs`
  compare lists of such rows with plain lists through notifications, scrolling, resizing, changes
  of text style and of the list's width, and following the end. `crates/gpui_scroll_harness/src/animated_rows.rs`
  times a list of 1,043 rows with one that animates. `mhr-gpui-apple`'s copy of `window.rs` under
  `vendor/gpui` follows it.
- **Long lines shaped in segments**, in `mhr-gpui`'s `src/text_system/line_layout.rs`,
  `src/text_system/line_layout/segments.rs` and `src/platform.rs`, and `mhr-gpui-wgpu`'s
  `src/cosmic_text_system.rs`. Shaping a long plain paragraph as one line took time in proportion
  to its length each time its text changed, so a streamed reply shaped all of its text again for
  every word. When a text system says it shapes each word on its own, with
  `PlatformTextSystem::shapes_words_independently`, a line of 1 KB or more is cut after spaces
  into segments, each segment is shaped and kept in a cache of its own by its text, font size and
  font runs, and the layouts are joined. The cuts depend on the words around them, so a change
  moves only the cuts near it. A line with right-to-left text, tabs or control characters is shaped
  whole, as is a line with a forced width. The cosmic-text system opts in, which covers Linux and
  Android, and the others shape whole lines as before. CoreText kerns a letter against the space
  before it, even in Roboto, so a cut after a space moves the next word by up to 0.8px at 15px and
  macOS shapes whole lines. DirectWrite doesn't kern across a space, but a whole line takes its
  ascent from its first font and each segment from its own, so the two disagree on a line in several
  fonts, and Windows shapes whole lines as well. `mhr-gpui-apple`'s copy of `platform.rs`
  under `vendor/gpui` follows the original. `crates/gpui_screenshot/tests/segmented_shaping.rs`
  compares segmented and whole layouts of ASCII, accented, Greek, Cyrillic, emoji, CJK, right-to-left
  and Indic text, and `long_line_measure.rs` there times a 2, 20 and 100 KB paragraph. The glyphs
  of the segments are the whole line's. A segment continues at the width the text system reports
  for it, a sum over its words, where the whole line keeps adding glyph advances in a running
  `f32`, so positions differ by float noise, up to about 0.16px at a cut in 2 KB of monospace text.
  The whole line drifts from the exact sum by more than that over 90,000px, so the segments are the
  closer. Wrapping follows the positions. Of 1,800 layouts of 300 random paragraphs of 1 to 10 KB at
  six widths, 23 (1.3%) put one word on the other side of a line end than the whole line does, each
  time a line end within a fraction of a pixel of the wrap width. The same test runs with
  DirectWrite on Windows and CoreText on macOS if they ever say they shape words independently, and
  adds a text of words that start with A, T, V, W or Y in Arial, Times New Roman, Helvetica and the
  like, since a font may kern a space against those. `mhr-gpui-windows`' `src/gpui_windows.rs`
  gives it the platform's text system under `test-support`. On macOS it uses
  `gpui_apple::AppleTextSystem`, which upstream made public in 0.3.11.
- **Shift on a symbol key in key bindings**, in `mhr-gpui`'s `src/platform/keystroke.rs`, with
  documentation in `src/keymap/binding.rs`. Windows, Linux and macOS send Ctrl+Shift+\ on a US
  keyboard as `ctrl-|`, since they drop Shift from every key that isn't a letter or a named key,
  and the web sends `ctrl-shift-|`. `Keystroke::parse` keeps Shift and the key `\` in
  `ctrl-shift-\`, and `should_match` compared modifiers and key exactly, so the binding never
  fired. Only Windows turned it into `ctrl-|`, in `WindowsKeyboardMapper::map_key_equivalent`, and
  only for a keymap loaded through `KeyBinding::load` with the platform's keyboard mapper.
  `KeyBinding::new` and the other platforms never did. `should_match` now folds Shift into the
  symbol on both keystrokes before it compares them, for the 21 keys of a US keyboard that carry
  two symbols. `ctrl-shift-\`, `ctrl-|` and `ctrl-shift-|` match one another whichever is bound
  and whichever is typed, and the other modifiers must agree. A `KeybindingKeystroke` keeps the
  spelling it was written with, so menus, palettes and the Windows mapper's display fields don't
  change, and a keystroke that matched before still matches. A chord bound in both spellings is two
  bindings for one keystroke, ordered like any two. `Unbind`, `null`, `bindings_for_action` and the
  pending-sequence check in `src/keymap.rs` compare bindings through `Keystroke::folded`, the same
  folding, so either spelling disables the other.
  The symbols are those of a US keyboard. A UK keyboard types `"` with Shift+2 and `~` on its `#`
  key, German, French and Czech keyboards type many of these symbols on other keys or with AltGr,
  and French and Czech keyboards send the digit for Ctrl+Shift+1. An app binds the symbol such a
  layout sends, and without the keyboard mapper `ctrl-shift-1` can't match on a French or Czech
  one. The tests at the end of `keystroke.rs`, `keymap.rs` and `key_dispatch.rs` cover it.
- **A keyboard mapper for Linux**, in `mhr-gpui-linux`'s `src/linux/keyboard_mapper.rs`, with a
  `keyboard_mapper` method on `DisplayConnection`, the X11 client and the Wayland connection. Linux
  returned `DummyKeyboardMapper`, so the Shift fold above was its only reading, and it assumes a US
  keyboard. `LinuxKeyboardMapper` is built from the xkb state each client keeps for key events, in
  its effective layout, as `keyboard_layout` is. For the 21 symbol keys of a US keyboard and the ISO
  key left of Z, it reads the key or the Shift symbol a binding names as the layout types it, as
  `WindowsKeyboardMapper` does. `KeyBinding::load` with `cx.keyboard_mapper()` turns `ctrl-shift-7`
  into `ctrl-/` on a German layout, which is what Ctrl+Shift+7 sends there. A key that doesn't type
  an ASCII symbol or digit at both levels is skipped, so letters (a French layout puts `m` on the
  `;` key) keep their Shift, and so do keys that type a non-ASCII symbol (`£` on a UK `3`, `§` on a
  German one, `ř` on a Czech `5`), which keep the US reading, so `ctrl-shift-3` still reads as
  `ctrl-#` there. A binding that the fold already reads the same way stays as written, so a US
  layout changes nothing. Linux has no display fields, so a binding that changes shows the
  symbol. `use_key_equivalents` isn't used, since Linux sends the US key for a layout with no ASCII
  keys. `KeyBinding::new` keeps the dummy mapper. macOS stays as it is, since `MacKeyboardMapper`
  only remaps key equivalents and a layout-aware reading there would need `UCKeyTranslate`. The
  tests in `keyboard_mapper.rs` compile keymaps for `us`, `de` and `fr` from names, with no display.
  The mapping happens in `KeyBinding::load`, so a binding keeps the keys it was loaded with after a
  layout change, and an app loads its keymap again from `App::on_keyboard_layout_change`.
- **Tests and examples that build**, in the `Cargo.toml` of `mhr-gpui`, `mhr-gpui-wgpu`,
  `mhr-gpui-web`, `mhr-gpui-macos`, `mhr-gpui-apple`, `mhr-gpui-macros`, `mhr-sum-tree` and
  `mhr-zlog`. The release tooling leaves out `[dev-dependencies]`, so these crates' tests didn't
  build. Each crate has the dev-dependencies its tests use again, as in Zed. Since 0.3.11 the
  release has no `util_macros`, so the three tests in `mhr-gpui`'s `src/style.rs` that Zed marks
  `#[perf]` are plain `#[test]`s, which is what `#[perf]` runs them as outside Zed's benchmarks.
  `mhr-gpui-macros` takes `gpui` by path without a version, so its published manifest leaves it
  out and a new `mhr-gpui` never forces a new `mhr-gpui-macros`. The workspace's `assets/fonts`
  holds the three fonts of Zed's `assets/fonts` that tests include (IBM Plex Sans Regular and Lilex
  Regular and Bold) with their licences. They sit outside every crate, so no package holds them.
  The examples of `mhr-gpui` build with Zed's dev-dependencies for them again (`env_logger`,
  `lyon` with `extra`, `unicode-segmentation`, `mhr-gpui-platform` and `mhr-reqwest-client`). The
  last two are by path without a version, so the published manifest leaves them out.
- **Modifier observers**, in `mhr-gpui`'s `src/window.rs`. A modifier change goes to the handlers
  on the focused element's dispatch path, which is the root node alone while nothing has focus, so
  an element never hears it then. `Window::observe_modifiers_changed` registers a callback that
  `Window::dispatch_event` calls after every `ModifiersChanged` event, once the elements have seen
  it and even if one of them stopped it. `mhr-gpui-apple`'s copy of `window.rs` under `vendor/gpui`
  follows it. A test in `window.rs` covers it.
- **GPUI Kit under its crates.io name**, in `mhr-gpui-macros`' `src/gpui_pre_facade_paths.rs`.
  When an app depends on GPUI Kit, the macros point their `gpui::` paths at it. They looked for the
  package `gpui-kit` alone, so an app that depends on `mhr-gpui-kit` got paths to a `gpui` it
  doesn't have. They try `mhr-gpui-kit` first and `gpui-kit` second. The library of `mhr-gpui-kit`
  is still `gpui_kit`, and an app that depends on it without a rename imports it by that name, so
  the macros use it where proc-macro-crate returns the key `mhr_gpui_kit`. `test = false` is gone
  from the crate's `Cargo.toml`, so its unit tests run, and one of them reads the manifests of
  made-up apps.
- **Animations on the executor's clock**, in `mhr-gpui`'s `src/elements/animation.rs`,
  `src/elements/img.rs`, `src/window.rs` and `src/app/test_context.rs`. `with_animation`,
  `with_animations` and the frames of animated images went by the real clock, while springs,
  paint animations and synced animations go by the executor's. A test that moved the executor's
  clock with `advance_clock` saw them stand still, and had to sleep. They now go by the time of the
  frame being drawn, as springs and paint animations do, and so does the delay before an image
  shows its loading element, which now shows in the frame its timer asks for. A one-shot animation
  ends in the frame drawn at its duration, so the next one of a chain starts from that frame, and
  one of no duration ends as it starts, as a paint animation of no duration does.
  `VisualTestContext::draw` sets the frame's time before it draws. `mhr-gpui-apple`'s copy of
  `window.rs` under `vendor/gpui` follows it. Tests at the end of `animation.rs` and `img.rs` cover
  it. Upstream 0.3.11 moved `with_animation` to the executor's clock too, read as each element
  lays out. The frame's time is kept here, so every animation of a frame is at the same point.
- **Property tests with proptest from crates.io**, in `mhr-gpui-macros`' `src/property_test.rs`
  and `Cargo.toml` and the workspace's `Cargo.toml`. `#[gpui::property_test]` gave proptest's
  macro `proptest_path` as a string, as Zed's git revision of proptest takes it. Proptest 1.11's
  macro has no `proptest_path`, and 1.12's takes a path, so the attribute expanded with neither.
  It now gives a path, and the workspace asks for proptest 1.12. `mhr-gpui-macros` tests its
  examples with `mhr-gpui`'s `test-support`, whose helpers the expansion calls.
  `randomized_element_tree_matches_full_render` in `mhr-gpui` is a property test again, as in Zed.
- **Glyph colors at paint time**, in `mhr-gpui`'s `src/elements/text.rs` and
  `src/text_system/line.rs`. A text's glyphs took their colors from its runs alone, and a color
  change between runs starts a new font run, which the shaper sees. macOS breaks ligatures at font
  runs, so recoloring part of a text, such as the current match of a search, could change its
  glyphs, widths and wrapping, and shaped it again each time the color moved.
  `TextLayout::paint_foreground_with_colors` paints the laid-out text like `paint_foreground`,
  with the glyphs in given byte ranges in their range's color. The later of two overlapping ranges
  wins, and a glyph of several characters, such as a ligature, takes the color of its first one.
  Underlines, strikethroughs and run backgrounds keep their runs' colors. The ranges are resolved
  once per call. Ranges that come sorted and apart are used as they are, with nothing allocated,
  and others take one sweep over their ends, in O(H log H) for H ranges. `paint_line` then looks
  each glyph's color up in a forward sweep over the ranges while the glyphs come in byte order,
  and with a binary search for a glyph that comes before the last one, as in right-to-left text.
  The tests at the end of `text.rs` draw with a text system that joins "fi" into one glyph within
  a font run and lays Hebrew out from right to left, and those at the end of `line.rs` check the
  resolved ranges against painting each range in turn, on random sets of ranges.

### Subroutine's changes

[Subroutine's fork](https://github.com/subroutine-app/gpui) adds these on top of Mesa Hills
Research's. Each of upstream's files they touch also starts with a line saying Subroutine modified
it, after Mesa Hills Research's line where there is one. GPUI Kit vendored most of them into its
copy of the gpui-pre 0.3.11 crates first, and they are ported here onto the fork's code.

- **Edges that serialize**, in `mhr-gpui`'s `src/geometry.rs`. `Edges` derives `Serialize`,
  `Deserialize` and `JsonSchema`, as its refinement already did, so a type that holds edges, such
  as a filter's fade bands, can derive them too. `mhr-gpui-apple`'s copy of `geometry.rs` under
  `vendor/gpui` follows it.

## The mobile crate

`crates/gpui_mobile` follows [longbridge/gpui-mobile](https://github.com/longbridge/gpui-mobile)
`main`, last taken at `9075e3a`. Newer upstream commits apply with their history:

```sh
git -C <gpui-mobile clone> format-patch --stdout 9075e3a..origin/main -- . ':!.github' \
  | git am -3 --directory=crates/gpui_mobile
```

Skip upstream's "Bump gpui-pre" commits, since the workspace sets that version.

Its `wgpu` follows `mhr-gpui-wgpu`'s, 30.0.1 for gpui-pre 0.3.11, since it hands wgpu types to it.
For 0.3.11 its iOS and Android platforms implement the new `platform_info`, `on_displays_changed`,
`on_display_changed` and `refresh_interval`. The callbacks are kept by nobody, since neither
platform watches its displays, and Android reports no refresh interval, so GPUI uses its default.

In `src/momentum.rs`, the velocity tracker's test gives its samples their own times instead of
sleeping between them, because sleeps overshoot on busy CI runners and the test failed there.

## Formatting

Format only the files you change, with `cargo fmt -p mhr-gpui-mobile` or `rustfmt <file>`.
`cargo fmt --all` would reformat code that the release tooling generates, such as
`crates/gpui_macros/src/gpui_pre_facade_paths.rs`, and every later import would conflict with it.
CI checks the formatting of every crate except `mhr-gpui-macros`.
