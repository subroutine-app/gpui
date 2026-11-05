# Changelog

Each release lists the crates it publishes. Crates that aren't listed stay at their earlier
version, and a requirement such as `version = "0.1"` picks up the newest 0.1.x of each.
[docs/upstream.md](docs/upstream.md#changes-to-the-gpui-crates) describes each change in more
detail.

## 0.2.0

Brings in gpui-pre 0.3.11, from Zed `f816e1f9` (2026-11-01), the snapshot GPUI Kit 0.9.0 is built
against. Published as all 25 crates, which start the 0.2 line together, so a requirement such as
`version = "0.1"` stays on 0.1.x and an app moves to `version = "0.2"`. `mhr-gpui-ios`, the iOS
platform that gpui-pre 0.3.11 adds, is new. `mhr-perf` and `mhr-util-macros` end at 0.1.0, since
gpui no longer uses them.

### Upstream

- Everything gpui-pre 0.3.11 changes since 0.3.8. Among what an app may meet:
  `ScrollWheelEvent` is `ScrollEvent` and `on_scroll_wheel` is `on_scroll`, with the old names
  kept as deprecated aliases. `Keystroke::parse` and `KeyBinding::new` take a `&'static str`, and
  a keystroke made at run time goes through `Keystroke::parse_with_os` or `KeyBinding::load`.
  `Keystroke::unparse` takes the `OperatingSystem` whose modifier names it writes. The macOS text
  system is `gpui_apple::AppleTextSystem`. A list can hold measurements as provisional with
  `ListState::set_item_measurement_readiness`, and a cached view redraws when the opacity it
  paints under changes.
- The crates need Rust 1.98, which their `rust-version` says, since upstream uses
  `Option::map_or_default`.
- The fork's changes carry over. List scrollbars, height estimates and cached views of measured
  height work with upstream's new scroll anchor, provisional measurements and opacity in the
  cache key. Animations keep going by the frame's time where upstream reads the executor's clock
  as each element lays out.
- Linux builds use oo7 0.6 from crates.io, as the published crates do, where Zed names a git
  revision.

### Test support

- `gpui_macos::MacTextSystem`, which `mhr-gpui-macos` exported under `test-support`, is gone.
  Tests use `gpui_apple::AppleTextSystem` with `mhr-gpui-apple`'s `font-kit` feature.

### From Subroutine's fork

- `Edges` serializes, deserializes and has a JSON schema, like its refinement.
- Elements blur. `Styled::blur` and `Styled::filter` blur an element and what it holds as one
  layer, like CSS `filter`, and `Styled::backdrop_blur` and `Styled::backdrop_filter` blur what
  is painted behind it, like CSS `backdrop-filter`, on Metal, wgpu and DirectX.
  `Window::paint_backdrop_filter` and `Window::with_filter_layer` do the same for custom
  elements. A scene without filters renders as before.

## 0.1.9

Published as `mhr-gpui`, `mhr-gpui-wgpu`, `mhr-gpui-linux`, `mhr-gpui-windows`,
`mhr-gpui-macos`, `mhr-gpui-apple`, `mhr-gpui-web`, `mhr-gpui-platform` and
`mhr-gpui-screenshot`. They pin each other exactly, so an app that uses several of them gets them
all at 0.1.9. `mhr-gpui-macros` has no change and stays at 0.1.8, and the other crates stay at
0.1.0.

### Text

- `TextLayout::paint_foreground_with_colors` paints laid-out text with the glyphs in given byte
  ranges in their range's color, for a highlight such as the current match of a search. Colors in
  a text's runs split its font runs, which could change its glyphs, widths and wrapping, and
  shaped the text again each time they moved. The new method keeps the layout and changes only the
  color each glyph is painted in. Where ranges overlap, the later one wins. Underlines,
  strikethroughs and run backgrounds keep their runs' colors.

## 0.1.8

Published as `mhr-gpui`, `mhr-gpui-macros`, `mhr-gpui-wgpu`, `mhr-gpui-linux`,
`mhr-gpui-windows`, `mhr-gpui-macos`, `mhr-gpui-apple`, `mhr-gpui-web`, `mhr-gpui-platform` and
`mhr-gpui-screenshot`, the same ten as 0.1.7. They pin each other exactly, so an app that uses
several of them gets them all at 0.1.8. The other crates stay at 0.1.0.

### Animation

- `with_animation`, `with_animations` and the frames of animated images go by the time of the
  frame being drawn, as springs and paint animations do. They read the real clock, so under a test
  executor `advance_clock` didn't move them and a test had to sleep. Every animation in a frame is
  now at the same point, and tests drive them all with `advance_clock`. Synced animations take the
  frame's time too.
- A one-shot animation ends in the frame drawn at its duration, so the next one of a chain starts
  from that frame instead of one frame later.
- An animation of no duration ends in its first frame. A one-shot draws its end and hands over to
  the next one of a chain, and a repeating one draws its start and stops.
- An image's loading element shows in the frame its timer asks for, under a test executor too.

### Macros

- `#[gpui::property_test]` expands with proptest from crates.io. It gave proptest's macro
  `proptest_path` as a string, which neither proptest 1.11 nor 1.12 takes. It now gives a path,
  and `mhr-gpui`'s `test-support` asks for proptest 1.12, the first release whose macro takes one.

### Test support

- `VisualTestContext::draw` sets the frame's time before it draws, so the animations it draws see
  the time `advance_clock` moved to.

## 0.1.7

Published as `mhr-gpui`, `mhr-gpui-macros`, `mhr-gpui-wgpu`, `mhr-gpui-linux`,
`mhr-gpui-windows`, `mhr-gpui-macos`, `mhr-gpui-apple`, `mhr-gpui-web`, `mhr-gpui-platform` and
`mhr-gpui-screenshot`, the same ten as 0.1.6. They pin each other exactly, so an app that uses
several of them gets them all at 0.1.7. The other crates stay at 0.1.0.

### Animation

- `PaintEffect` has a `translation`, in logical pixels, that moves everything an element paints
  (quads, shadows, underlines, paths, sprites and surfaces). A paint animation can now slide an
  element without drawing the view it is in. `PaintEffect::translation` makes one, and
  `with_opacity`, `with_rotation` and `with_translation` combine the three, so an element can
  fade as it moves. A rotation turns sprites about the element's centre first and the translation
  moves them after. The translations of nested elements add up.
- An effect changes what is painted. Hitboxes, bounds and focus stay where the element was laid
  out, and so does the clip it paints under. An element that slides out of its clip is cut by
  it, and shows again when a later frame moves it back.
- `Scene::push_effect` takes the window's scale factor as a third argument. `PaintEffect` has a
  new public field, so a struct literal needs `..PaintEffect::NONE`.
- Springs step by the time of the frame being drawn. They read the real clock when laid out,
  while paint animations and synced animations go by the frame's time. Under a test executor with
  its own clock, a spring stepped by the few microseconds between two draws, which rounds to no
  movement on a fast machine. Every spring in a frame now moves by the same amount, and tests can
  drive springs with `advance_clock`.

### Macros

- The test of the package-name lookup writes each made-up manifest to a folder of its own.
  `proc-macro-crate` caches an answer by the manifest's path and modification time, and on Windows
  a manifest rewritten within one tick of the file clock read as unchanged.

### Test support

- The tests of `mhr-bench-metrics` no longer depend on the machine. The page fault check maps its
  own pages and turns huge pages off for them, because a 4 MiB vector faults in two or three times
  with transparent huge pages on. The context switch check sleeps until it counts five switches,
  because a sleep that ends before the thread is scheduled out is not one. The hardware counter
  tests skip when Linux lists no description of an event, as some virtual machines do.

## 0.1.6

Published as `mhr-gpui`, `mhr-gpui-macros`, `mhr-gpui-wgpu`, `mhr-gpui-linux`,
`mhr-gpui-windows`, `mhr-gpui-macos`, `mhr-gpui-apple`, `mhr-gpui-web`, `mhr-gpui-platform` and
`mhr-gpui-screenshot`. They pin each other exactly, so an app that uses several of them gets them
all at 0.1.6. The other crates stay at 0.1.0.

### Macros

- The macros find GPUI Kit under the name it is published as, `mhr-gpui-kit`. An app that gets
  GPUI through the kit has `#[gpui::test]`, `actions!` and the derives point their `gpui::` paths at
  it, and they looked for the package `gpui-kit` alone. A dependency on `mhr-gpui-kit`, with or
  without a rename, gave paths to a `gpui` the app doesn't have. They try `mhr-gpui-kit` first and
  `gpui-kit` second, so an app that still depends on the old name builds as before.

### Test support

- `mhr-gpui-screenshot` is on crates.io. It renders GPUI views headlessly on Linux and compares
  them with golden PNGs, as [docs/screenshots.md](docs/screenshots.md) describes. It was a git
  dependency before, and its version follows `mhr-gpui`'s from 0.1.6.

## 0.1.5

Published as `mhr-gpui`, `mhr-gpui-wgpu`, `mhr-gpui-linux`, `mhr-gpui-windows`,
`mhr-gpui-macos`, `mhr-gpui-apple`, `mhr-gpui-web` and `mhr-gpui-platform`. They pin each other
exactly, so an app that uses several of them gets them all at 0.1.5. The other crates stay at
0.1.0.

### Key bindings

- `Unbind`, `null`, the shortcut a menu shows and the pending-sequence check now read Shift on a
  symbol key the way matching does. 0.1.4 made `ctrl-shift-\` and `ctrl-|` one chord for a typed
  keystroke, but these compared the spelling, so `Unbind` of `ctrl-|` left a binding written
  `ctrl-shift-\` in place. `bindings_for_action` kept showing a shortcut that `null` or `Unbind`
  had disabled in the other spelling, and a sequence disabled in the other spelling still waited
  for its next key. A chord can be written either way throughout a keymap.
- On Linux, a keymap loaded with `KeyBinding::load` and `cx.keyboard_mapper()` reads Shift on a
  symbol key the way the active layout types it, on X11 and Wayland. The platform returned the dummy
  mapper, so `ctrl-shift-7` meant Ctrl+& on every layout and never fired on a German keyboard, where
  Shift+7 types `/`. The new mapper reads the 21 symbol keys of a US keyboard, and the ISO key left
  of Z, from the xkb layout in use, as the Windows mapper does. A binding that the US reading
  already gets right stays as written, so a US layout and the way a binding shows are unchanged. A
  key that types a non-ASCII symbol, such as `£` on a UK keyboard, keeps the US reading. A binding
  keeps the keys it was loaded with, so an app loads its keymap again when
  `App::on_keyboard_layout_change` fires. `use_key_equivalents` has no effect on Linux. macOS keeps
  its mapper, which only remaps key equivalents.

### Keyboard

- `Window::observe_modifiers_changed` calls back after every change of the modifier keys or of Caps
  Lock in a window, whatever has focus. Element handlers such as `on_modifiers_changed` hear a
  change only along the focused element's path, which is the root alone while nothing has focus, so
  an app that reacts to a held modifier, such as underlining menu access keys while Alt is down, had
  to poll `Window::modifiers`. The callback runs after the elements have seen the change, even when
  one of them stops it, and dropping the returned `Subscription` ends it.

### Test support

- `gpui_windows::text_system` and `gpui_macos::MacTextSystem` give a test the platform's
  text system, with the crate's `test-support` feature (and `font-kit` on macOS).

## 0.1.4

Published as `mhr-gpui`, `mhr-gpui-wgpu`, `mhr-gpui-linux`, `mhr-gpui-windows`,
`mhr-gpui-macos`, `mhr-gpui-apple`, `mhr-gpui-web` and `mhr-gpui-platform`. They pin each other
exactly, so an app that uses several of them gets them all at 0.1.4. The other crates stay at
0.1.0.

### Key bindings

- A binding with Shift on a symbol key, such as `ctrl-shift-\`, matches the symbol the platforms
  send. Windows, Linux and macOS send Ctrl+Shift+\ on a US keyboard as `ctrl-|`, and the web sends
  `ctrl-shift-|`. A binding written with Shift on the key matched neither, so it could not fire,
  except on Windows in a keymap loaded with the platform's keyboard mapper.
  `Keystroke::should_match` now reads Shift on one of the 21 symbol keys of a US keyboard as the
  symbol it types, so `ctrl-shift-\`, `ctrl-|` and `ctrl-shift-|` are one chord whichever is bound
  or typed. A binding shows as written, a keystroke that matched before still matches, and letters
  and named keys keep their Shift. A chord bound in both spellings is two bindings for one
  keystroke, ordered like any two. Where a layout's Shift types another symbol, such as `"` on
  Shift+2 of a UK keyboard, bind the symbol the layout sends. The documentation of
  `KeyBinding::new` says what to write.

## 0.1.3

Published as `mhr-gpui`, `mhr-gpui-wgpu`, `mhr-gpui-linux`, `mhr-gpui-windows`,
`mhr-gpui-macos`, `mhr-gpui-apple`, `mhr-gpui-web` and `mhr-gpui-platform`. They pin each other
exactly, so an app that uses several of them gets them all at 0.1.3. The other crates stay at
0.1.0.

### Lists

- A list given height estimates keeps the height an item had when the item changes, until its
  next frame estimates the item again. An item invalidated by `remeasure_items` or replaced by a
  one-for-one `splice` counted the mean height of the other items in the meantime. A reply that
  streamed in below the reader made the scroll range too short for that moment, and the reader's
  place fell beyond its end. Reading the scrollbar then moved the thumb to the end of its track and
  left it there, and a turn of the wheel through `set_offset_from_scrollbar` could move the list by
  hundreds of pixels. The scrollbar still gives a position while items wait for an estimate, but
  doesn't keep it as the place the thumb moves on from.
- The scrollbar's thumb stays on its track. A thumb that moved down with more speed than the list
  had distance left to the end went past 1, and the offset the scrollbar showed went past the end of
  its range. A wheel turn through `set_offset_from_scrollbar` then moved the list by the turn and
  by the excess as well.
- A turn of the wheel scrolls the list from where it is. The list's wheel handler scrolled from
  where the list was when it last painted, so a turn that came after another scroll in the same
  frame, by `scroll_by`, by `scroll_to` or through the scrollbar's offset, undid that scroll.

## 0.1.2

Published as `mhr-gpui`, `mhr-gpui-wgpu`, `mhr-gpui-linux`, `mhr-gpui-windows`,
`mhr-gpui-macos`, `mhr-gpui-apple`, `mhr-gpui-web` and `mhr-gpui-platform`. They pin each other
exactly, so an app that uses several of them gets them all at 0.1.2. The other crates stay at
0.1.0.

### Views

- A list that draws its items twice in a frame, as it does when an item asks to scroll, reports
  them to the accessibility tree once. Its second drawing pushed the nodes of the first again,
  which panicked in debug builds and left the first drawing's places in the tree in release builds.
- A cached view that is reused keeps its nodes in the accessibility tree. Every frame that
  reused one used to leave out its nodes, its focus and its action handlers, so a screen reader
  found holes where cached views were. The nodes are now copied from the previous frame along
  with the view's drawing. A cached view draws again when accessibility starts, to have nodes to
  copy afterwards. While accessibility is on, the window keeps one more frame of nodes and copies
  the tree once more in each frame. With accessibility off, a frame does no extra work.

### Animation

- `AnimationExt::with_paint_animation` animates how an element paints, its opacity and the
  rotation of its icons, without drawing the view it is in again. An animated element notified its
  view in every frame, so a spinner in a pane drew the whole pane, and every view above it, 60
  times a second. A paint animation makes a `PaintEffect` from the animation's value, and the
  window draws its root view in each frame and replays what the cached views drew with the
  effects applied. On a pane of 30, 120 and 480 rows, a frame in which a spinner turns took 0.26,
  0.32 and 0.65 ms, against 1.3, 4.2 and 17 ms when the spinner redraws the pane. Frames stop when
  the animations end or when `App::reduce_motion` is set, and `Animation::with_max_fps` draws
  them on the grid that throttled animations share. A view that holds a paint animation no longer
  draws in each frame, so code that counts that view's draws to measure the animation's frame rate
  counts fewer.

### Lists

- `Entity::cached_measured` and `AnyView::cached_measured` make a list item a cached view whose
  height is measured from its contents. A cached view took its size from its style alone, so a row
  of a list could not be one. Such a row renders when it is first shown, when it is notified, when
  the width it is laid out at or the text style around it changes, and when the window refreshes.
  Otherwise it reuses what it drew and the height it measured, so a row that animates redraws
  alone and the list's other rows stay as they were. On a list of 1,043 mixed rows, a frame in
  which one row animates took 0.15 ms with 14 rows in view, against 0.29 ms when every row is an
  element of the list's view, and 0.27 ms against 0.76 ms with 20 rows in view. The row has to be
  the item itself, since only the root of a layout knows its width. Anywhere else it renders in
  every frame, as an uncached view does.

### Text

- A long line of text is shaped in segments of a few hundred bytes on Linux and Android, and each
  segment is kept for later frames. A long paragraph that grows or changes by a word shapes the
  segment it touches instead of the whole paragraph. On a paragraph of 20 KB, a frame that adds
  a word takes 1.2 ms instead of 9.6 ms, and one that edits a word in the middle takes 1.0 ms
  instead of 9.0 ms. At 100 KB they take 2.7 and 2.9 ms instead of 51 and 52 ms. A paragraph
  drawn for the first time takes about 10% longer. The glyphs are those of the whole line.
  Positions can differ from the whole line's by float noise, up to about 0.16px at a cut in a line
  of monospace text, because each segment continues at the width the text system reports for it.
  Wrapping follows the positions, so a word can land on the other side of a line end than it did in
  0.1.1 when that line end sits within a fraction of a pixel of the wrap width. In a test of random
  paragraphs of 1 to 10 KB at six widths, 1.3% of the wraps put one word on the other line. Lines
  under 1 KB, lines with right-to-left text, tabs or control characters, and lines on other
  platforms are shaped whole. A text system opts in with
  `PlatformTextSystem::shapes_words_independently`.

## 0.1.1

Published as `mhr-gpui`, `mhr-gpui-wgpu`, `mhr-gpui-linux`, `mhr-gpui-windows`,
`mhr-gpui-macos`, `mhr-gpui-apple`, `mhr-gpui-web` and `mhr-gpui-platform`. They pin each other
exactly, so an app that uses several of them gets them all at 0.1.1. The other crates stay at
0.1.0.

### Lists

- A list's scrollbar thumb holds steady over rows that aren't drawn yet. Scrolling up a long
  list of rows of mixed heights used to shrink the thumb and make it jump back, a dragged thumb
  slid away from the pointer, and a thumb dragged to either end stopped short of it. A row that
  hasn't been measured now counts its size hint, or else the mean height of the rows that have.
  The thumb reaches both ends of the track together with the list, and it changes speed
  gradually when a row turns out taller or shorter than counted.
- `List::estimate_item_heights` takes a closure that estimates a row's height at a width, for
  example from its lines of text. The scrollbar counts the estimates for the rows that haven't
  been drawn, so the thumb keeps its size and moves evenly as the reader scrolls onto new rows.
  The list asks for estimates of new rows, of every row after `ListState::remeasure`, of every
  row when its width changes, and of rows that change through `ListState::remeasure_items` or a
  one-for-one `splice`. On a transcript of 1043 rows of 20 to 900px, the largest step of the
  thumb while scrolling up from the end went from 6.4 times the median step to 1.0 times.

### Views

- A cached view inside another cached view is reused when the view around it redraws. It redraws
  only when it was notified, a view inside it was notified, or its bounds, content mask or text
  style changed. Before, caching a view inside another one saved nothing. On a settings pane of
  8 cached groups of 6 rows of text, a frame in which one group changes now renders 1 group
  instead of 8 and takes 1.0 ms instead of 2.8 ms.
- A cached view under an element that was hidden for a frame no longer replays old paint when
  the element shows again. It draws again instead.

### Text

- Text measured without a width is laid out at its natural width. A box sized by its text, such
  as a chat message at the side of a column, used to stay as narrow as an earlier wrapped layout
  had made it.
- `TextSystem::has_font_family` says whether one font family is available, without listing every
  family with `all_font_names`. On a Linux machine with 231 families it takes 0.15 µs for a
  family that's there, against 38 µs for listing the families and searching them. A custom
  `PlatformTextSystem` gets a default that searches `all_font_names`.

### In the repository

`mhr-gpui-scroll-harness` measures and films how smoothly a list's scrollbar moves over a
transcript of mixed rows. It isn't published. The velocity tracker test in `mhr-gpui-mobile` gives
its samples their own times instead of sleeping between them, which made it fail on busy machines.

## 0.1.0

The first release of the fork, built on gpui-pre 0.3.8 (Zed `279fe070`). It adds a fix for
variable font weights on Linux and Android, test support for screenshot tests, recent files in
Windows jump lists, and throttled animations that share frames. It takes Zed's forks of font-kit,
xim, scap and wasm_thread from crates.io.
