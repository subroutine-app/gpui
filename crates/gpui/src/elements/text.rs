// Modified by Mesa Hills Research, as docs/upstream.md in mesa-hills-research/gpui describes.
use crate::{
    ActiveTooltip, AnyView, App, Bounds, DispatchPhase, Element, ElementId, GlobalElementId,
    GlyphColors, HighlightStyle, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement,
    LayoutId, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, SharedString, Size,
    TextOverflow, TextRun, TextStyle, TooltipId, TruncateFrom, WhiteSpace, Window, WrappedLine,
    WrappedLineLayout, register_tooltip_mouse_handlers, resolve_glyph_colors,
    set_tooltip_on_window,
};
use anyhow::Context as _;
use gpui_util::ResultExt;
use itertools::Itertools;
use smallvec::SmallVec;
use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    mem,
    ops::{Deref, DerefMut, Range},
    rc::Rc,
    sync::Arc,
};

/// An [`Element`] that renders text.
///
/// In general, [`Text`] objects should be created via the [`text`] macro:
/// ```rust
/// # use gpui::*;
/// # fn render() -> impl IntoElement {
/// div().child(text!("hello"))
/// # }
/// ```
/// ## IDs and Accessibility
///
/// [`Text`] elements have an ID. This ID is primarily used to produce nodes in
/// the accessibility tree, which allows the text to be visible to screen
/// readers and other assistive technologies.
///
/// This ID is stable across frames. If the same text, with the same ID, is
/// present in two consecutive frames, no updates are reported to the screen
/// reader. If the text changes, but the ID stays the same, then the screen
/// reader will be notified that a text node's content has changed. **However**,
/// if the ID changes, then the screen reader will be notified that a node has
/// been removed, and a new node has been added.
///
/// When using the [`text`] macro, each invocation of the macro will get a
/// unique ID, derived from its position in the source code (filename, line, and
/// column). For example:
/// ```rust
/// # use gpui::*;
/// let x = text!("hello");
/// let y = text!("hello");
/// // not equal, because different `text!` invocations produced them
/// assert_ne!(x.id(), y.id());
///
/// fn make_text(s: &str) -> Text { text!(s) }
/// let x = make_text("hello");
/// let y = make_text("hello");
/// // equal, because the same `text!` invocation produced them
/// assert_eq!(x.id(), y.id());
/// ```
/// When the contents of an invocation of [`text`] do not change, this
/// distinction is less relevant (with the caveat that you still need to take
/// care to ensure that duplicate IDs do not appear).
///
/// However, when a [`text`] invocation's argument *does* change, you should
/// consider whether this change should be reported as a node "updating its
/// contents", or an old node being destroyed and a new node being created.
#[derive(Debug, Clone)]
pub struct Text {
    id: Option<ElementId>,
    text: SharedString,
}

impl Text {
    /// Create a new [`Text`] element with a specific ID.
    ///
    /// If you want a unique ID to be assigned automatically, use the [`text`]
    /// macro. The docs for [`Text`] have more detail about choosing IDs.
    #[inline]
    pub const fn new(id: ElementId, text: SharedString) -> Self {
        Self { id: Some(id), text }
    }

    /// Create a new [`Text`] element that is inaccessible to screen readers.
    ///
    /// In order for text to be accessible to screen readers, it must have an ID
    /// provided. If you want text to be accessible, either use [`text`] to have
    /// an ID automatically assigned, or use [`Text::new`] to manually assign an
    /// ID.
    ///
    /// This function is intended for use inside custom UI components, where
    /// accessible properties may be set on parent containers.
    #[inline]
    pub const fn new_inaccessible(text: SharedString) -> Self {
        Self { id: None, text }
    }

    /// The ID of this [`Text`] element.
    #[inline]
    pub const fn id(&self) -> Option<&ElementId> {
        self.id.as_ref()
    }

    /// Produce a new [`Text`] with the given `id`.
    pub fn with_id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// The text that this [`Text`] element will display.
    #[inline]
    pub const fn text(&self) -> &SharedString {
        &self.text
    }
}

impl Deref for Text {
    type Target = SharedString;
    fn deref(&self) -> &Self::Target {
        &self.text
    }
}

impl DerefMut for Text {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.text
    }
}

/// Trivial hash function for the location information produced by the [`text`]
/// macro. Not covered by semver guarantees. Performance is not particularly
/// significant because it's only used on small strings in const contexts.
#[doc(hidden)]
pub const fn __hash_text_macro_location_unstable_do_not_use(s: &'static str) -> u64 {
    const BASIS: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;

    let bytes = s.as_bytes();
    let mut hash = BASIS;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(PRIME);
        i += 1;
    }
    hash
}

/// Create a new [`Text`] element.
///
/// ```rust
/// # use gpui::*;
/// let a = text!("hello");
/// let b = text!(id = "farewell-message", "hello");
///
/// ```
///
/// Text created with this macro is *accessible*. The macro generates an ID
/// based on the source location. See the docs for [`Text`] for a more in-depth
/// explanation of the significance of the ID of a [`Text`] element.
#[macro_export]
macro_rules! text {
    (id = $id:expr, $text:expr) => {{ $crate::Text::new($id.into(), $text.into()) }};
    ($text:expr) => {{
        const ID: &'static str = concat!(file!(), "/", line!(), ":", column!());
        const HASH: u64 = $crate::__hash_text_macro_location_unstable_do_not_use(ID);
        $crate::Text::new($crate::ElementId::Integer(HASH), $text.into())
    }};
}

impl IntoElement for Text {
    type Element = Self;
    #[inline]
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Text {
    type RequestLayoutState = TextLayout;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        self.id.clone()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn a11y_role(&self) -> Option<accesskit::Role> {
        if self.id.is_some() {
            Some(accesskit::Role::Label)
        } else {
            None
        }
    }

    fn write_a11y_info(&self, node: &mut accesskit::Node) {
        node.set_value(self.text.to_string());
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        <SharedString as Element>::request_layout(&mut self.text, id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        <SharedString as Element>::prepaint(
            &mut self.text,
            id,
            inspector_id,
            bounds,
            request_layout,
            window,
            cx,
        )
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        <SharedString as Element>::paint(
            &mut self.text,
            id,
            inspector_id,
            bounds,
            request_layout,
            prepaint,
            window,
            cx,
        );
    }
}

impl Element for &'static str {
    type RequestLayoutState = TextLayout;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let state = TextLayout::default();
        let layout_id = state.layout(SharedString::from(*self), None, window, cx);
        (layout_id, state)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        text_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) {
        text_layout.prepaint(bounds, self)
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        text_layout: &mut TextLayout,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        text_layout.paint(self, window, cx)
    }
}

impl IntoElement for &'static str {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl IntoElement for String {
    type Element = SharedString;

    fn into_element(self) -> Self::Element {
        self.into()
    }
}

impl IntoElement for Cow<'static, str> {
    type Element = SharedString;

    fn into_element(self) -> Self::Element {
        self.into()
    }
}

impl Element for SharedString {
    type RequestLayoutState = TextLayout;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let state = TextLayout::default();
        let layout_id = state.layout(self.clone(), None, window, cx);
        (layout_id, state)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        text_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) {
        text_layout.prepaint(bounds, self.as_ref())
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        text_layout: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        text_layout.paint(self.as_ref(), window, cx)
    }
}

impl IntoElement for SharedString {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// Renders text with runs of different styles.
///
/// Callers are responsible for setting the correct style for each run.
/// For text with a uniform style, you can usually avoid calling this constructor
/// and just pass text directly.
pub struct StyledText {
    text: SharedString,
    runs: Option<Vec<TextRun>>,
    delayed_highlights: Option<Vec<(Range<usize>, HighlightStyle)>>,
    delayed_font_family_overrides: Option<Vec<(Range<usize>, SharedString)>>,
    layout: TextLayout,
}

impl StyledText {
    /// Construct a new styled text element from the given string.
    pub fn new(text: impl Into<SharedString>) -> Self {
        StyledText {
            text: text.into(),
            runs: None,
            delayed_highlights: None,
            delayed_font_family_overrides: None,
            layout: TextLayout::default(),
        }
    }

    /// Get the layout for this element. This can be used to map indices to pixels and vice versa.
    pub fn layout(&self) -> &TextLayout {
        &self.layout
    }

    /// Set the styling attributes for the given text, as well as
    /// as any ranges of text that have had their style customized.
    pub fn with_default_highlights(
        self,
        default_style: &TextStyle,
        highlights: impl IntoIterator<Item = (Range<usize>, HighlightStyle)>,
    ) -> Self {
        debug_assert!(
            self.delayed_highlights.is_none(),
            "Can't use `with_default_highlights` and `with_highlights`"
        );
        let runs = Self::compute_runs(&self.text, default_style, highlights);
        self.with_runs(runs)
    }

    /// Set the styling attributes for the given text, as well as
    /// as any ranges of text that have had their style customized.
    pub fn with_highlights(
        mut self,
        highlights: impl IntoIterator<Item = (Range<usize>, HighlightStyle)>,
    ) -> Self {
        debug_assert!(
            self.runs.is_none(),
            "Can't use `with_highlights` and `with_default_highlights`"
        );
        self.delayed_highlights = Some(
            highlights
                .into_iter()
                .inspect(|(run, _)| {
                    debug_assert!(self.text.is_char_boundary(run.start));
                    debug_assert!(self.text.is_char_boundary(run.end));
                })
                .collect::<Vec<_>>(),
        );
        self
    }

    fn compute_runs(
        text: &str,
        default_style: &TextStyle,
        highlights: impl IntoIterator<Item = (Range<usize>, HighlightStyle)>,
    ) -> Vec<TextRun> {
        let mut runs = Vec::new();
        let mut ix = 0;
        for (range, highlight) in highlights {
            if ix < range.start {
                debug_assert!(text.is_char_boundary(range.start));
                runs.push(default_style.clone().to_run(range.start - ix));
            }
            debug_assert!(text.is_char_boundary(range.end));
            runs.push(
                default_style
                    .clone()
                    .highlight(highlight)
                    .to_run(range.len()),
            );
            ix = range.end;
        }
        if ix < text.len() {
            runs.push(default_style.to_run(text.len() - ix));
        }
        runs
    }

    /// Override the font family for specific byte ranges of the text.
    ///
    /// This is resolved lazily at layout time, so the overrides are applied
    /// on top of the inherited text style from the parent element.
    /// Can be combined with [`with_highlights`](Self::with_highlights).
    ///
    /// The overrides must be sorted by range start and non-overlapping.
    /// Each override range must fall on character boundaries.
    pub fn with_font_family_overrides(
        mut self,
        overrides: impl IntoIterator<Item = (Range<usize>, SharedString)>,
    ) -> Self {
        self.delayed_font_family_overrides = Some(
            overrides
                .into_iter()
                .inspect(|(range, _)| {
                    debug_assert!(self.text.is_char_boundary(range.start));
                    debug_assert!(self.text.is_char_boundary(range.end));
                })
                .collect(),
        );
        self
    }

    fn apply_font_family_overrides(
        runs: &mut [TextRun],
        overrides: &[(Range<usize>, SharedString)],
    ) {
        let mut byte_offset = 0;
        let mut override_idx = 0;
        for run in runs.iter_mut() {
            let run_end = byte_offset + run.len;
            while override_idx < overrides.len() && overrides[override_idx].0.end <= byte_offset {
                override_idx += 1;
            }
            if override_idx < overrides.len() {
                let (ref range, ref family) = overrides[override_idx];
                if byte_offset >= range.start && run_end <= range.end {
                    run.font.family = family.clone();
                }
            }
            byte_offset = run_end;
        }
    }

    /// Set the text runs for this piece of text.
    pub fn with_runs(mut self, runs: Vec<TextRun>) -> Self {
        let mut text = &*self.text;
        for run in &runs {
            text = text.get(run.len..).unwrap_or_else(|| {
                #[cfg(debug_assertions)]
                panic!("invalid text run. Text: '{text}', run: {run:?}");
                #[cfg(not(debug_assertions))]
                panic!("invalid text run");
            });
        }
        assert!(text.is_empty(), "invalid text run");
        self.runs = Some(runs);
        self
    }
}

impl Element for StyledText {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let font_family_overrides = self.delayed_font_family_overrides.take();
        let mut runs = self.runs.take().or_else(|| {
            self.delayed_highlights.take().map(|delayed_highlights| {
                Self::compute_runs(&self.text, &window.text_style(), delayed_highlights)
            })
        });

        if let Some(ref overrides) = font_family_overrides {
            let runs =
                runs.get_or_insert_with(|| vec![window.text_style().to_run(self.text.len())]);
            Self::apply_font_family_overrides(runs, overrides);
        }

        let layout_id = self.layout.layout(self.text.clone(), runs, window, cx);
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) {
        self.layout.prepaint(bounds, &self.text)
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.layout.paint(&self.text, window, cx)
    }
}

impl IntoElement for StyledText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// The Layout for TextElement. This can be used to map indices to pixels and vice versa.
#[derive(Default, Clone)]
pub struct TextLayout(Rc<RefCell<Option<TextLayoutInner>>>);

#[derive(Clone, Copy)]
enum TextPaintPass {
    All,
    Background,
    Foreground,
}

struct TextLayoutInner {
    len: usize,
    lines: SmallVec<[WrappedLine; 1]>,
    line_height: Pixels,
    wrap_width: Option<Pixels>,
    truncate_width: Option<Pixels>,
    size: Option<Size<Pixels>>,
    bounds: Option<Bounds<Pixels>>,
    /// The layout this one replaced, kept for when the text is measured at
    /// that layout's wrap width again. Layout often asks for the text's
    /// natural width and then lays it out at a narrower width.
    replaced: Option<Box<TextLayoutInner>>,
}

impl TextLayoutInner {
    /// Whether this layout answers a measurement at `wrap_width`. A truncated
    /// layout answers none, since the text may fit without truncation.
    fn answers(&self, wrap_width: Option<Pixels>) -> Option<Size<Pixels>> {
        if self.wrap_width == wrap_width && self.truncate_width.is_none() {
            self.size
        } else {
            None
        }
    }
}

impl TextLayout {
    fn layout(
        &self,
        text: SharedString,
        runs: Option<Vec<TextRun>>,
        window: &mut Window,
        _: &mut App,
    ) -> LayoutId {
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let line_height = window.pixel_snap(
            text_style
                .line_height
                .to_pixels(font_size.into(), window.rem_size()),
        );

        let runs = if let Some(runs) = runs {
            runs
        } else {
            vec![text_style.to_run(text.len())]
        };
        window.request_measured_layout(Default::default(), {
            let element_state = self.clone();

            move |known_dimensions, available_space, window, cx| {
                let wrap_width = if text_style.white_space == WhiteSpace::Normal {
                    known_dimensions.width.or(match available_space.width {
                        crate::AvailableSpace::Definite(x) => Some(x),
                        _ => None,
                    })
                } else {
                    None
                };

                let (truncate_width, truncation_affix, truncate_from) =
                    if let Some(text_overflow) = text_style.text_overflow.clone() {
                        let width = known_dimensions.width.or(match available_space.width {
                            crate::AvailableSpace::Definite(x) => match text_style.line_clamp {
                                Some(max_lines) => Some(x * max_lines),
                                None => Some(x),
                            },
                            _ => None,
                        });

                        match text_overflow {
                            TextOverflow::Truncate(s) => (width, s, TruncateFrom::End),
                            TextOverflow::TruncateStart(s) => (width, s, TruncateFrom::Start),
                            TextOverflow::TruncateMiddle(s) => (width, s, TruncateFrom::Middle),
                        }
                    } else {
                        (None, "".into(), TruncateFrom::End)
                    };

                // Only use a cached layout if:
                // 1. We have a cached size
                // 2. wrap_width matches, both None included. A layout wrapped at some width
                //    doesn't answer a probe for the text's natural width.
                // 3. truncate_width is None (if truncate_width is Some, we need to re-layout
                //    because the previous layout may have been computed without truncation)
                // 4. the cached layout was not truncated (a truncated layout answers an
                //    unconstrained probe with the truncated size, which poisons intrinsic
                //    sizing with whatever width some earlier measure pass happened to use)
                // The layout measured last is the one painted, so a match with the layout
                // it replaced swaps the two.
                if truncate_width.is_none()
                    && let Some(text_layout) = element_state.0.borrow_mut().as_mut()
                {
                    if let Some(size) = text_layout.answers(wrap_width) {
                        return size;
                    }
                    if let Some(mut replaced) = text_layout.replaced.take() {
                        if let Some(size) = replaced.answers(wrap_width) {
                            mem::swap(text_layout, &mut replaced);
                            text_layout.replaced = Some(replaced);
                            return size;
                        }
                        text_layout.replaced = Some(replaced);
                    }
                }
                let replaced = element_state.0.borrow_mut().take().map(|mut layout| {
                    layout.replaced = None;
                    Box::new(layout)
                });

                let mut line_wrapper = cx.text_system().line_wrapper(text_style.font(), font_size);
                let (text, runs) = if let Some(truncate_width) = truncate_width {
                    if let Some(max_lines) = text_style.line_clamp
                        && let Some(wrap_width) = wrap_width
                    {
                        line_wrapper.truncate_wrapped_line(
                            text.clone(),
                            wrap_width,
                            max_lines,
                            &truncation_affix,
                            &runs,
                            truncate_from,
                        )
                    } else if let Some(unclipped) = window
                        .text_system()
                        .shape_text(text.clone(), font_size, &runs, None, None)
                        .log_err()
                        && unclipped
                            .iter()
                            .all(|line| line.size(line_height).width <= truncate_width)
                    {
                        // The truncation decision below sums per-character advances,
                        // which overestimates the shaped width (no kerning), truncating
                        // text that fits exactly in its measured width. Skip truncation
                        // whenever the honestly-shaped text fits; the shaping result
                        // comes from the line layout cache when the same text was
                        // already measured untruncated this frame.
                        (text.clone(), Cow::Borrowed(&*runs))
                    } else {
                        line_wrapper.truncate_line(
                            text.clone(),
                            truncate_width,
                            &truncation_affix,
                            &runs,
                            truncate_from,
                        )
                    }
                } else {
                    (text.clone(), Cow::Borrowed(&*runs))
                };
                let len = text.len();

                let Some(lines) = window
                    .text_system()
                    .shape_text(
                        text,
                        font_size,
                        &runs,
                        wrap_width,            // Wrap if we know the width.
                        text_style.line_clamp, // Limit the number of lines if line_clamp is set.
                    )
                    .log_err()
                else {
                    element_state.0.borrow_mut().replace(TextLayoutInner {
                        lines: Default::default(),
                        len: 0,
                        line_height,
                        wrap_width,
                        truncate_width,
                        size: Some(Size::default()),
                        bounds: None,
                        replaced,
                    });
                    return Size::default();
                };

                let mut size: Size<Pixels> = Size::default();
                for line in &lines {
                    let line_size = line.size(line_height);
                    size.height += line_size.height;
                    size.width = size.width.max(line_size.width).ceil();
                }

                element_state.0.borrow_mut().replace(TextLayoutInner {
                    lines,
                    len,
                    line_height,
                    wrap_width,
                    truncate_width,
                    size: Some(size),
                    bounds: None,
                    replaced,
                });

                size
            }
        })
    }

    fn prepaint(&self, bounds: Bounds<Pixels>, text: &str) {
        let mut element_state = self.0.borrow_mut();
        let element_state = element_state
            .as_mut()
            .with_context(|| format!("measurement has not been performed on {text}"))
            .unwrap();
        element_state.bounds = Some(bounds);
    }

    fn paint(&self, text: &str, window: &mut Window, cx: &mut App) {
        self.paint_lines(TextPaintPass::All, &[], window, cx)
            .with_context(|| format!("failed to paint {text}"))
            .unwrap();
    }

    /// Paints the backgrounds of the text runs without the glyphs, so that callers can paint
    /// content, such as highlights, between the backgrounds and the text. Follow it with
    /// [`Self::paint_foreground`] to paint the glyphs.
    pub fn paint_background(&self, window: &mut Window, cx: &mut App) -> anyhow::Result<()> {
        self.paint_lines(TextPaintPass::Background, &[], window, cx)
    }

    /// Paints the glyphs and decorations of the text without the run backgrounds.
    pub fn paint_foreground(&self, window: &mut Window, cx: &mut App) -> anyhow::Result<()> {
        self.paint_lines(TextPaintPass::Foreground, &[], window, cx)
    }

    /// Paints the glyphs and decorations of the text like [`Self::paint_foreground`], with the
    /// glyphs in each of `colors`' ranges in its color. The ranges are UTF-8 byte offsets into
    /// the whole text as laid out, counting the newline between two lines as one byte. Where
    /// ranges overlap, the later one wins. An empty or reversed range colors nothing, and neither
    /// does the part of a range past the end of the text. A glyph that draws several characters,
    /// such as a ligature, takes the color of its first one, and color emoji keep their own
    /// colors.
    ///
    /// Only the glyphs change color. Underlines, strikethroughs and run backgrounds keep the
    /// colors of their runs, and the text is painted from its layout as it is, so this never
    /// shapes the text again and the text keeps its widths and wrapping.
    pub fn paint_foreground_with_colors(
        &self,
        colors: &[(Range<usize>, Hsla)],
        window: &mut Window,
        cx: &mut App,
    ) -> anyhow::Result<()> {
        let colors = resolve_glyph_colors(colors);
        self.paint_lines(TextPaintPass::Foreground, &colors, window, cx)
    }

    /// Paints the lines for `pass`, with the glyph colors of `colors`, which are sorted by start
    /// and don't overlap.
    fn paint_lines(
        &self,
        pass: TextPaintPass,
        colors: &[(Range<usize>, Hsla)],
        window: &mut Window,
        cx: &mut App,
    ) -> anyhow::Result<()> {
        let element_state = self.0.borrow();
        let element_state = element_state
            .as_ref()
            .context("measurement has not been performed")?;
        let bounds = element_state
            .bounds
            .context("prepaint has not been performed")?;

        let line_height = element_state.line_height;
        let mut line_origin = bounds.origin;
        let mut line_start = 0;
        let text_style = window.text_style();
        for line in &element_state.lines {
            if matches!(pass, TextPaintPass::All | TextPaintPass::Background) {
                line.paint_background(
                    line_origin,
                    line_height,
                    text_style.text_align,
                    Some(bounds),
                    window,
                    cx,
                )
                .log_err();
            }
            let line_end = line_start + line.len();
            if matches!(pass, TextPaintPass::All | TextPaintPass::Foreground) {
                let first = colors.partition_point(|(range, _)| range.end <= line_start);
                let last = colors.partition_point(|(range, _)| range.start < line_end);
                line.paint_with_glyph_colors(
                    line_origin,
                    line_height,
                    text_style.text_align,
                    Some(bounds),
                    GlyphColors::new(&colors[first..last], line_start),
                    window,
                    cx,
                )
                .log_err();
            }
            line_origin.y += line.size(line_height).height;
            // Lines are split at a newline, which is one byte.
            line_start = line_end + 1;
        }
        Ok(())
    }

    /// Get the byte index into the input of the pixel position.
    pub fn index_for_position(&self, position: Point<Pixels>) -> Result<usize, usize> {
        let element_state = self.0.borrow();
        let element_state = element_state
            .as_ref()
            .expect("measurement has not been performed");
        let bounds = element_state
            .bounds
            .expect("prepaint has not been performed");

        if position.y < bounds.top() {
            return Err(0);
        }

        let line_height = element_state.line_height;
        let mut line_origin = bounds.origin;
        let mut line_start_ix = 0;
        for line in &element_state.lines {
            let line_bottom = line_origin.y + line.size(line_height).height;
            if position.y > line_bottom {
                line_origin.y = line_bottom;
                line_start_ix += line.len() + 1;
            } else {
                let position_within_line = position - line_origin;
                match line.index_for_position(position_within_line, line_height) {
                    Ok(index_within_line) => return Ok(line_start_ix + index_within_line),
                    Err(index_within_line) => return Err(line_start_ix + index_within_line),
                }
            }
        }

        Err(line_start_ix.saturating_sub(1))
    }

    /// Get the pixel position for the given byte index.
    pub fn position_for_index(&self, index: usize) -> Option<Point<Pixels>> {
        let element_state = self.0.borrow();
        let element_state = element_state
            .as_ref()
            .expect("measurement has not been performed");
        let bounds = element_state
            .bounds
            .expect("prepaint has not been performed");
        let line_height = element_state.line_height;

        let mut line_origin = bounds.origin;
        let mut line_start_ix = 0;

        for line in &element_state.lines {
            let line_end_ix = line_start_ix + line.len();
            if index < line_start_ix {
                break;
            } else if index > line_end_ix {
                line_origin.y += line.size(line_height).height;
                line_start_ix = line_end_ix + 1;
                continue;
            } else {
                let ix_within_line = index - line_start_ix;
                return Some(line_origin + line.position_for_index(ix_within_line, line_height)?);
            }
        }

        None
    }

    /// Retrieve the layout for the line containing the given byte index.
    pub fn line_layout_for_index(&self, index: usize) -> Option<Arc<WrappedLineLayout>> {
        let element_state = self.0.borrow();
        let element_state = element_state
            .as_ref()
            .expect("measurement has not been performed");
        let mut line_start_ix = 0;

        for line in &element_state.lines {
            let line_end_ix = line_start_ix + line.len();
            if index < line_start_ix {
                break;
            } else if index > line_end_ix {
                line_start_ix = line_end_ix + 1;
                continue;
            } else {
                return Some(line.layout.clone());
            }
        }

        None
    }

    /// Retrieve all line layouts in source order.
    pub fn line_layouts(&self) -> SmallVec<[Arc<WrappedLineLayout>; 1]> {
        self.0
            .borrow()
            .as_ref()
            .expect("measurement has not been performed")
            .lines
            .iter()
            .map(|line| line.layout.clone())
            .collect()
    }

    /// The bounds of this layout.
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.0.borrow().as_ref().unwrap().bounds.unwrap()
    }

    /// The line height for this layout.
    pub fn line_height(&self) -> Pixels {
        self.0.borrow().as_ref().unwrap().line_height
    }

    /// The UTF-8 length of the underlying text.
    pub fn len(&self) -> usize {
        self.0.borrow().as_ref().unwrap().len
    }

    /// The text for this layout.
    pub fn text(&self) -> String {
        self.0
            .borrow()
            .as_ref()
            .unwrap()
            .lines
            .iter()
            .map(|s| &s.text)
            .join("\n")
    }

    /// The text for this layout (with soft-wraps as newlines)
    pub fn wrapped_text(&self) -> String {
        let mut accumulator = String::new();

        for wrapped in self.0.borrow().as_ref().unwrap().lines.iter() {
            let mut seen = 0;
            for boundary in wrapped.layout.wrap_boundaries.iter() {
                let index = wrapped.layout.unwrapped_layout.runs[boundary.run_ix].glyphs
                    [boundary.glyph_ix]
                    .index;

                accumulator.push_str(&wrapped.text[seen..index]);
                accumulator.push('\n');
                seen = index;
            }
            accumulator.push_str(&wrapped.text[seen..]);
            accumulator.push('\n');
        }
        // Remove trailing newline
        accumulator.pop();
        accumulator
    }
}

/// A text element that can be interacted with.
pub struct InteractiveText {
    element_id: ElementId,
    text: StyledText,
    click_listener:
        Option<Box<dyn Fn(&[Range<usize>], InteractiveTextClickEvent, &mut Window, &mut App)>>,
    hover_listener: Option<Box<dyn Fn(Option<usize>, MouseMoveEvent, &mut Window, &mut App)>>,
    tooltip_builder: Option<Rc<dyn Fn(usize, &mut Window, &mut App) -> Option<AnyView>>>,
    tooltip_id: Option<TooltipId>,
    clickable_ranges: Vec<Range<usize>>,
}

struct InteractiveTextClickEvent {
    mouse_down_index: usize,
    mouse_up_index: usize,
}

#[doc(hidden)]
#[derive(Default)]
pub struct InteractiveTextState {
    mouse_down_index: Rc<Cell<Option<usize>>>,
    hovered_index: Rc<Cell<Option<usize>>>,
    active_tooltip: Rc<RefCell<Option<ActiveTooltip>>>,
    long_press_tooltip_active: Rc<Cell<bool>>,
}

/// InteractiveTest is a wrapper around StyledText that adds mouse interactions.
impl InteractiveText {
    /// Creates a new InteractiveText from the given text.
    pub fn new(id: impl Into<ElementId>, text: StyledText) -> Self {
        Self {
            element_id: id.into(),
            text,
            click_listener: None,
            hover_listener: None,
            tooltip_builder: None,
            tooltip_id: None,
            clickable_ranges: Vec::new(),
        }
    }

    /// on_click is called when the user clicks on one of the given ranges, passing the index of
    /// the clicked range.
    pub fn on_click(
        mut self,
        ranges: Vec<Range<usize>>,
        listener: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.click_listener = Some(Box::new(move |ranges, event, window, cx| {
            for (range_ix, range) in ranges.iter().enumerate() {
                if range.contains(&event.mouse_down_index) && range.contains(&event.mouse_up_index)
                {
                    listener(range_ix, window, cx);
                }
            }
        }));
        self.clickable_ranges = ranges;
        self
    }

    /// on_hover is called when the mouse moves over a character within the text, passing the
    /// index of the hovered character, or None if the mouse leaves the text.
    pub fn on_hover(
        mut self,
        listener: impl Fn(Option<usize>, MouseMoveEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.hover_listener = Some(Box::new(listener));
        self
    }

    /// tooltip lets you specify a tooltip for a given character index in the string.
    pub fn tooltip(
        mut self,
        builder: impl Fn(usize, &mut Window, &mut App) -> Option<AnyView> + 'static,
    ) -> Self {
        self.tooltip_builder = Some(Rc::new(builder));
        self
    }
}

impl Element for InteractiveText {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.element_id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn a11y_role(&self) -> Option<accesskit::Role> {
        Some(accesskit::Role::Label)
    }

    fn write_a11y_info(&self, node: &mut accesskit::Node) {
        node.set_value(self.text.text.to_string());
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.text.request_layout(None, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Hitbox {
        window.with_optional_element_state::<InteractiveTextState, _>(
            global_id,
            |interactive_state, window| {
                let mut interactive_state = interactive_state
                    .map(|interactive_state| interactive_state.unwrap_or_default());

                if let Some(interactive_state) = interactive_state.as_mut() {
                    if self.tooltip_builder.is_some() {
                        self.tooltip_id =
                            set_tooltip_on_window(&interactive_state.active_tooltip, window);
                    } else {
                        // If there is no longer a tooltip builder, remove the active tooltip.
                        interactive_state.long_press_tooltip_active.set(false);
                        interactive_state.active_tooltip.take();
                    }
                }

                self.text
                    .prepaint(None, inspector_id, bounds, state, window, cx);
                let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
                (hitbox, interactive_state)
            },
        )
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        hitbox: &mut Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        let current_view = window.current_view();
        let text_layout = self.text.layout().clone();
        window.with_element_state::<InteractiveTextState, _>(
            global_id.unwrap(),
            |interactive_state, window| {
                let interactive_state = interactive_state.unwrap_or_default();
                if let Some(click_listener) = self.click_listener.take() {
                    let mouse_position = window.mouse_position();
                    if let Ok(ix) = text_layout.index_for_position(mouse_position)
                        && self
                            .clickable_ranges
                            .iter()
                            .any(|range| range.contains(&ix))
                    {
                        window.set_cursor_style(crate::CursorStyle::PointingHand, hitbox)
                    }

                    let text_layout = text_layout.clone();
                    let mouse_down = interactive_state.mouse_down_index.clone();
                    if let Some(mouse_down_index) = mouse_down.get() {
                        let hitbox = hitbox.clone();
                        let clickable_ranges = mem::take(&mut self.clickable_ranges);
                        window.on_mouse_event(
                            move |event: &MouseUpEvent, phase, window: &mut Window, cx| {
                                if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                                    if let Ok(mouse_up_index) =
                                        text_layout.index_for_position(event.position)
                                    {
                                        click_listener(
                                            &clickable_ranges,
                                            InteractiveTextClickEvent {
                                                mouse_down_index,
                                                mouse_up_index,
                                            },
                                            window,
                                            cx,
                                        )
                                    }

                                    mouse_down.take();
                                    window.refresh();
                                }
                            },
                        );
                    } else {
                        let hitbox = hitbox.clone();
                        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
                            if phase == DispatchPhase::Bubble
                                && hitbox.is_hovered(window)
                                && let Ok(mouse_down_index) =
                                    text_layout.index_for_position(event.position)
                            {
                                mouse_down.set(Some(mouse_down_index));
                                window.refresh();
                            }
                        });
                    }
                }

                window.on_mouse_event({
                    let hover_listener = self.hover_listener.take();
                    let hitbox = hitbox.clone();
                    let text_layout = text_layout.clone();
                    let hovered_index = interactive_state.hovered_index.clone();
                    move |event: &MouseMoveEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                            let current = hovered_index.get();
                            let updated = text_layout.index_for_position(event.position).ok();
                            if current != updated {
                                hovered_index.set(updated);
                                if let Some(hover_listener) = hover_listener.as_ref() {
                                    hover_listener(updated, event.clone(), window, cx);
                                }
                                cx.notify(current_view);
                            }
                        }
                    }
                });

                if let Some(tooltip_builder) = self.tooltip_builder.clone() {
                    let active_tooltip = interactive_state.active_tooltip.clone();
                    let build_tooltip = Rc::new({
                        let tooltip_is_hoverable = false;
                        let text_layout = text_layout.clone();
                        move |window: &mut Window, cx: &mut App| {
                            text_layout
                                .index_for_position(window.mouse_position())
                                .ok()
                                .and_then(|position| tooltip_builder(position, window, cx))
                                .map(|view| (view, tooltip_is_hoverable))
                        }
                    });

                    // Use bounds instead of testing hitbox since this is called during prepaint.
                    let check_is_hovered_during_prepaint = Rc::new({
                        let source_bounds = hitbox.bounds;
                        let text_layout = text_layout.clone();
                        let pending_mouse_down = interactive_state.mouse_down_index.clone();
                        move |window: &Window| {
                            text_layout
                                .index_for_position(window.mouse_position())
                                .is_ok()
                                && source_bounds.contains(&window.mouse_position())
                                && pending_mouse_down.get().is_none()
                        }
                    });

                    let check_is_hovered = Rc::new({
                        let hitbox = hitbox.clone();
                        let text_layout = text_layout.clone();
                        let pending_mouse_down = interactive_state.mouse_down_index.clone();
                        move |window: &Window| {
                            text_layout
                                .index_for_position(window.mouse_position())
                                .is_ok()
                                && hitbox.is_hovered(window)
                                && pending_mouse_down.get().is_none()
                        }
                    });

                    register_tooltip_mouse_handlers(
                        &active_tooltip,
                        self.tooltip_id,
                        build_tooltip,
                        check_is_hovered,
                        check_is_hovered_during_prepaint,
                        interactive_state.long_press_tooltip_active.clone(),
                        None,
                        window,
                    );
                }

                self.text
                    .paint(None, inspector_id, bounds, &mut (), &mut (), window, cx);

                ((), interactive_state)
            },
        );
    }
}

impl IntoElement for InteractiveText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppContext as _, AvailableSpace, Background, Context, DevicePixels, FontId, FontRun,
        GlyphId, InteractiveElement as _, LineLayout, NoopTextSystem, PaddedBool32,
        ParentElement as _, PlatformTextSystem, Render, RenderGlyphParams, ScaledPixels,
        ShapedGlyph, ShapedRun, StrikethroughStyle, Styled as _, TestAppContext, UnderlineStyle,
        WindowHandle, black, blue, div, green, hsla, point, px, red, relative, size,
        window::DrawPhase, yellow,
    };
    use std::sync::atomic::{self, AtomicUsize};

    const SENTENCE: &str = "one two three four five six";

    /// Measures `layout` as the root of a layout, in `width`.
    fn measure(
        layout: &TextLayout,
        width: AvailableSpace,
        window: &mut Window,
        cx: &mut App,
    ) -> Size<Pixels> {
        let layout_id = layout.layout(SENTENCE.into(), None, window, cx);
        window.compute_layout(layout_id, size(width, AvailableSpace::MinContent), cx);
        window.layout_bounds(layout_id).size
    }

    #[gpui::test]
    fn text_measured_without_a_width_after_wrapping_takes_its_natural_size(
        cx: &mut TestAppContext,
    ) {
        let cx = cx.add_empty_window();
        cx.update(|window, cx| {
            window.invalidator.set_phase(DrawPhase::Prepaint);
            let natural = measure(
                &TextLayout::default(),
                AvailableSpace::MaxContent,
                window,
                cx,
            );

            let layout = TextLayout::default();
            let wrapped = measure(&layout, AvailableSpace::Definite(px(40.)), window, cx);
            assert!(wrapped.width <= px(40.) && wrapped.height > natural.height);
            assert_eq!(
                measure(&layout, AvailableSpace::MaxContent, window, cx),
                natural
            );
            // The lines it paints are the ones it measured last.
            assert_eq!(layout.wrapped_text(), SENTENCE);

            assert_eq!(
                measure(&layout, AvailableSpace::Definite(px(40.)), window, cx),
                wrapped
            );
            assert_eq!(
                layout.wrapped_text().lines().count() as f32,
                wrapped.height / layout.line_height()
            );
            window.invalidator.set_phase(DrawPhase::None);
        });
    }

    /// Draws the same `StyledText` every frame, as an element that keeps its
    /// layout from one frame to the next does.
    struct RetainedText(Rc<RefCell<StyledText>>);

    impl IntoElement for RetainedText {
        type Element = Self;

        fn into_element(self) -> Self::Element {
            self
        }
    }

    impl Element for RetainedText {
        type RequestLayoutState = ();
        type PrepaintState = ();

        fn id(&self) -> Option<ElementId> {
            None
        }

        fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
            None
        }

        fn request_layout(
            &mut self,
            id: Option<&GlobalElementId>,
            inspector_id: Option<&InspectorElementId>,
            window: &mut Window,
            cx: &mut App,
        ) -> (LayoutId, ()) {
            self.0
                .borrow_mut()
                .request_layout(id, inspector_id, window, cx)
        }

        fn prepaint(
            &mut self,
            id: Option<&GlobalElementId>,
            inspector_id: Option<&InspectorElementId>,
            bounds: Bounds<Pixels>,
            request_layout: &mut (),
            window: &mut Window,
            cx: &mut App,
        ) {
            self.0
                .borrow_mut()
                .prepaint(id, inspector_id, bounds, request_layout, window, cx)
        }

        fn paint(
            &mut self,
            id: Option<&GlobalElementId>,
            inspector_id: Option<&InspectorElementId>,
            bounds: Bounds<Pixels>,
            request_layout: &mut (),
            prepaint: &mut (),
            window: &mut Window,
            cx: &mut App,
        ) {
            self.0.borrow_mut().paint(
                id,
                inspector_id,
                bounds,
                request_layout,
                prepaint,
                window,
                cx,
            )
        }
    }

    /// A message at the right of the window, as wide as its text up to half
    /// the window.
    struct MessageView {
        text: Rc<RefCell<StyledText>>,
    }

    impl Render for MessageView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .flex_row()
                .w_full()
                .items_start()
                .child(
                    div()
                        .flex_grow(1000.)
                        .flex_basis(px(0.))
                        .max_w(relative(0.5)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_grow(1.)
                        .flex_basis(px(40.))
                        .min_w_0()
                        .items_end()
                        .child(
                            div().flex().flex_row().w_full().justify_end().child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .min_w_0()
                                    .debug_selector(|| "message".into())
                                    .child(RetainedText(self.text.clone())),
                            ),
                        ),
                )
        }
    }

    #[gpui::test]
    fn a_message_grows_to_its_text_natural_width_when_the_window_widens(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| MessageView {
            text: Rc::new(RefCell::new(StyledText::new(SENTENCE))),
        });
        let natural = cx.update(|window, cx| {
            window.invalidator.set_phase(DrawPhase::Prepaint);
            let width = measure(
                &TextLayout::default(),
                AvailableSpace::MaxContent,
                window,
                cx,
            )
            .width;
            window.invalidator.set_phase(DrawPhase::None);
            width
        });

        cx.simulate_resize(size(px(200.), px(400.)));
        cx.run_until_parked();
        let narrow = cx.debug_bounds("message").expect("message is drawn");
        assert!(narrow.size.width < natural);

        cx.simulate_resize(size(px(1000.), px(400.)));
        cx.run_until_parked();
        let wide = cx.debug_bounds("message").expect("message is drawn");
        assert_eq!(wide.size.width, natural);
    }

    /// The glyph a [`LigatureTextSystem`] draws for "fi".
    const FI_LIGATURE: GlyphId = GlyphId(1);

    /// A text system whose font draws "fi" as one glyph narrower than the two letters, as fonts
    /// with ligatures do, when both letters are in one font run. Every other character is a
    /// glyph 8px wide whose id is the character, and Hebrew words come from right to left,
    /// against byte order. It counts the lines it shapes, and rasterizes each glyph as wide as
    /// its id, so a test can tell which glyph a sprite draws.
    #[derive(Default)]
    struct LigatureTextSystem {
        shaped: AtomicUsize,
    }

    impl PlatformTextSystem for LigatureTextSystem {
        fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> anyhow::Result<()> {
            NoopTextSystem.add_fonts(fonts)
        }

        fn all_font_names(&self) -> Vec<String> {
            Vec::new()
        }

        fn font_id(&self, descriptor: &crate::Font) -> anyhow::Result<FontId> {
            NoopTextSystem.font_id(descriptor)
        }

        fn font_metrics(&self, font_id: FontId) -> crate::FontMetrics {
            NoopTextSystem.font_metrics(font_id)
        }

        fn typographic_bounds(
            &self,
            font_id: FontId,
            glyph_id: GlyphId,
        ) -> anyhow::Result<Bounds<f32>> {
            NoopTextSystem.typographic_bounds(font_id, glyph_id)
        }

        fn advance(&self, font_id: FontId, glyph_id: GlyphId) -> anyhow::Result<Size<f32>> {
            NoopTextSystem.advance(font_id, glyph_id)
        }

        fn glyph_for_char(&self, _font_id: FontId, ch: char) -> Option<GlyphId> {
            Some(GlyphId(ch as u32))
        }

        fn glyph_raster_bounds(
            &self,
            params: &RenderGlyphParams,
        ) -> anyhow::Result<Bounds<DevicePixels>> {
            Ok(Bounds::new(
                point(DevicePixels(0), DevicePixels(-8)),
                size(DevicePixels(params.glyph_id.0 as i32), DevicePixels(1)),
            ))
        }

        fn rasterize_glyph(
            &self,
            _params: &RenderGlyphParams,
            raster_bounds: Bounds<DevicePixels>,
        ) -> anyhow::Result<(Size<DevicePixels>, Vec<u8>)> {
            let bytes = (raster_bounds.size.width.0 * raster_bounds.size.height.0) as usize;
            Ok((raster_bounds.size, vec![0; bytes]))
        }

        fn layout_line(&self, text: &str, font_size: Pixels, runs: &[FontRun]) -> LineLayout {
            self.shaped.fetch_add(1, atomic::Ordering::SeqCst);
            let mut x = px(0.);
            let mut run_start = 0;
            let mut shaped_runs = Vec::new();
            for run in runs {
                // Each glyph in byte order, with its advance and whether it's Hebrew.
                let mut logical = Vec::new();
                let mut chars = text[run_start..run_start + run.len]
                    .char_indices()
                    .peekable();
                while let Some((ix, ch)) = chars.next() {
                    let (id, advance) =
                        if ch == 'f' && chars.next_if(|(_, ch)| *ch == 'i').is_some() {
                            (FI_LIGATURE, px(10.))
                        } else {
                            (GlyphId(ch as u32), px(8.))
                        };
                    let hebrew = ('\u{0590}'..='\u{05ff}').contains(&ch);
                    logical.push((run_start + ix, id, advance, hebrew));
                }
                // Hebrew words read from right to left, so their glyphs come in reverse.
                let mut glyphs = Vec::new();
                for word in logical.chunk_by(|a, b| a.3 == b.3) {
                    let in_order: Box<dyn Iterator<Item = _>> = if word[0].3 {
                        Box::new(word.iter().rev())
                    } else {
                        Box::new(word.iter())
                    };
                    for &(index, id, advance, _) in in_order {
                        glyphs.push(ShapedGlyph {
                            id,
                            position: point(x, px(0.)),
                            index,
                            is_emoji: false,
                        });
                        x += advance;
                    }
                }
                shaped_runs.push(ShapedRun {
                    font_id: run.font_id,
                    glyphs,
                });
                run_start += run.len;
            }
            LineLayout {
                font_size,
                width: x,
                ascent: px(12.),
                descent: px(4.),
                runs: shaped_runs,
                len: text.len(),
            }
        }

        fn recommended_rendering_mode(
            &self,
            font_id: FontId,
            font_size: Pixels,
        ) -> crate::TextRenderingMode {
            NoopTextSystem.recommended_rendering_mode(font_id, font_size)
        }
    }

    /// Draws `text` in `runs` in a box 72px wide, the glyphs in `colors` when there are some.
    struct ColoredTextView {
        text: SharedString,
        runs: Vec<TextRun>,
        colors: Option<Vec<(Range<usize>, Hsla)>>,
        /// The layout drawn last.
        layout: TextLayout,
    }

    impl Render for ColoredTextView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let text = StyledText::new(self.text.clone()).with_runs(self.runs.clone());
            self.layout = text.layout().clone();
            div().w(px(72.)).child(ColoredText {
                text,
                colors: self.colors.clone(),
            })
        }
    }

    /// A `StyledText` painted with [`TextLayout::paint_foreground_with_colors`] when it has
    /// colors, and as usual otherwise.
    struct ColoredText {
        text: StyledText,
        colors: Option<Vec<(Range<usize>, Hsla)>>,
    }

    impl IntoElement for ColoredText {
        type Element = Self;

        fn into_element(self) -> Self::Element {
            self
        }
    }

    impl Element for ColoredText {
        type RequestLayoutState = ();
        type PrepaintState = ();

        fn id(&self) -> Option<ElementId> {
            None
        }

        fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
            None
        }

        fn request_layout(
            &mut self,
            id: Option<&GlobalElementId>,
            inspector_id: Option<&InspectorElementId>,
            window: &mut Window,
            cx: &mut App,
        ) -> (LayoutId, ()) {
            self.text.request_layout(id, inspector_id, window, cx)
        }

        fn prepaint(
            &mut self,
            id: Option<&GlobalElementId>,
            inspector_id: Option<&InspectorElementId>,
            bounds: Bounds<Pixels>,
            request_layout: &mut (),
            window: &mut Window,
            cx: &mut App,
        ) {
            self.text
                .prepaint(id, inspector_id, bounds, request_layout, window, cx)
        }

        fn paint(
            &mut self,
            id: Option<&GlobalElementId>,
            inspector_id: Option<&InspectorElementId>,
            bounds: Bounds<Pixels>,
            request_layout: &mut (),
            prepaint: &mut (),
            window: &mut Window,
            cx: &mut App,
        ) {
            match &self.colors {
                Some(colors) => {
                    let layout = self.text.layout();
                    layout.paint_background(window, cx).unwrap();
                    layout
                        .paint_foreground_with_colors(colors, window, cx)
                        .unwrap();
                }
                None => self.text.paint(
                    id,
                    inspector_id,
                    bounds,
                    request_layout,
                    prepaint,
                    window,
                    cx,
                ),
            }
        }
    }

    /// What a frame of a [`ColoredTextView`] drew.
    #[derive(Debug, PartialEq)]
    struct DrawnText {
        /// The glyph id, top, left and color of each glyph sprite, in reading order.
        glyphs: Vec<(u32, f32, f32, Hsla)>,
        /// The bounds, color and waviness of each underline and strikethrough.
        lines: Vec<(Bounds<ScaledPixels>, Hsla, PaddedBool32)>,
        /// The bounds and color of each run background.
        backgrounds: Vec<(Bounds<ScaledPixels>, Background)>,
    }

    /// Draws `window` with `colors` and returns what it drew, with the shaped lines.
    fn draw_with_colors(
        window: WindowHandle<ColoredTextView>,
        colors: Option<Vec<(Range<usize>, Hsla)>>,
        cx: &mut TestAppContext,
    ) -> (DrawnText, SmallVec<[Arc<WrappedLineLayout>; 1]>) {
        window
            .update(cx, |view, _, cx| {
                view.colors = colors;
                cx.notify();
            })
            .unwrap();
        cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        window
            .update(cx, |view, window, _| {
                let scene = &window.rendered_frame.scene;
                let mut glyphs = scene
                    .monochrome_sprites
                    .iter()
                    .map(|sprite| {
                        (
                            sprite.bounds.size.width.0 as u32,
                            sprite.bounds.origin.y.0,
                            sprite.bounds.origin.x.0,
                            sprite.color,
                        )
                    })
                    .collect::<Vec<_>>();
                glyphs.sort_by(|a, b| (a.1, a.2).partial_cmp(&(b.1, b.2)).unwrap());
                let drawn = DrawnText {
                    glyphs,
                    lines: scene
                        .underlines
                        .iter()
                        .map(|line| (line.bounds, line.color, line.wavy))
                        .collect(),
                    backgrounds: scene
                        .quads
                        .iter()
                        .map(|quad| (quad.bounds, quad.background))
                        .collect(),
                };
                (drawn, view.layout.line_layouts())
            })
            .unwrap()
    }

    /// A run of `len` bytes in `color`.
    fn plain_run(len: usize, color: Hsla) -> TextRun {
        TextRun {
            len,
            font: crate::font("Ligatures"),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        }
    }

    /// The color of each glyph of `lines`, in the order they are drawn: that of the last of
    /// `colors` holding the glyph's first byte, or else that of its run.
    fn expected_glyph_colors(
        lines: &[Arc<WrappedLineLayout>],
        runs: &[TextRun],
        colors: &[(Range<usize>, Hsla)],
    ) -> Vec<Hsla> {
        let run_color = |ix: usize| {
            let mut end = 0;
            runs.iter()
                .find(|run| {
                    end += run.len;
                    ix < end
                })
                .unwrap()
                .color
        };
        let mut expected = Vec::new();
        let mut line_start = 0;
        for line in lines {
            for glyph in line
                .unwrapped_layout
                .runs
                .iter()
                .flat_map(|run| &run.glyphs)
            {
                let ix = line_start + glyph.index;
                expected.push(
                    colors
                        .iter()
                        .rev()
                        .find(|(range, _)| range.contains(&ix))
                        .map_or_else(|| run_color(ix), |(_, color)| *color),
                );
            }
            line_start += line.len() + 1;
        }
        expected
    }

    /// The colors of `glyphs`.
    fn painted(glyphs: &[(u32, f32, f32, Hsla)]) -> Vec<Hsla> {
        glyphs.iter().map(|glyph| glyph.3).collect()
    }

    /// The ids and places of `glyphs`.
    fn placed(glyphs: &[(u32, f32, f32, Hsla)]) -> Vec<(u32, f32, f32)> {
        glyphs
            .iter()
            .map(|(id, top, left, _)| (*id, *top, *left))
            .collect()
    }

    #[test]
    fn glyph_colors_paint_over_the_shaped_text_without_shaping_it_again() {
        let text_system = Arc::new(LigatureTextSystem::default());
        let mut cx = TestAppContext::build_with_text_system(
            crate::TestDispatcher::new(0),
            None,
            text_system.clone(),
        );
        let cx = &mut cx;

        // Plain text, an underlined link, a newline, text on a background and struck-out text.
        let text = "fine fish fit\nfifty files";
        let run = plain_run;
        let runs = vec![
            run(10, black()),
            TextRun {
                underline: Some(UnderlineStyle {
                    thickness: px(1.),
                    color: None,
                    wavy: false,
                }),
                ..run(3, red())
            },
            run(1, black()),
            TextRun {
                background_color: Some(yellow()),
                ..run(6, black())
            },
            TextRun {
                strikethrough: Some(StrikethroughStyle {
                    thickness: px(1.),
                    color: None,
                }),
                ..run(5, black())
            },
        ];
        let purple = hsla(0.8, 1., 0.5, 1.);
        let colors = vec![
            // From the "i" of a ligature, which keeps the color of its "f".
            (1..3, green()),
            // Across the wrap after "fish " and into the link.
            (5..12, blue()),
            // Over the one before, which it wins.
            (8..9, green()),
            // From the middle of the link's ligature across the newline.
            (11..17, purple),
        ];

        let window = cx.open_window(size(px(400.), px(200.)), |_, _| ColoredTextView {
            text: text.into(),
            runs: runs.clone(),
            colors: None,
            layout: TextLayout::default(),
        });
        let (plain, plain_lines) = draw_with_colors(window, None, cx);
        let shaped = text_system.shaped.load(atomic::Ordering::SeqCst);
        let (colored, colored_lines) = draw_with_colors(window, Some(colors.clone()), cx);

        // The glyphs are those shaped without colors, in the same places, wrapped the same.
        assert_eq!(text_system.shaped.load(atomic::Ordering::SeqCst), shaped);
        assert_eq!(colored_lines.len(), 2);
        assert!(
            colored_lines
                .iter()
                .zip(&plain_lines)
                .all(|(a, b)| Arc::ptr_eq(a, b))
        );
        assert_eq!(colored_lines[0].wrap_boundaries.len(), 1);
        assert_eq!(placed(&colored.glyphs), placed(&plain.glyphs));
        // The underline, the strikethrough and the background keep their colors.
        assert_eq!(colored.lines, plain.lines);
        assert_eq!(colored.lines.len(), 2);
        assert_eq!(colored.backgrounds, plain.backgrounds);
        assert_eq!(colored.backgrounds.len(), 1);

        // Each glyph takes the color of the last range that holds its first byte, or its run's.
        assert_eq!(
            painted(&colored.glyphs),
            expected_glyph_colors(&plain_lines, &runs, &colors)
        );
        assert_eq!(
            painted(&plain.glyphs),
            expected_glyph_colors(&plain_lines, &runs, &[])
        );
        let ids = colored
            .glyphs
            .iter()
            .map(|glyph| glyph.0)
            .collect::<Vec<_>>();
        assert_eq!(
            ids.iter().filter(|id| **id == FI_LIGATURE.0).count(),
            5,
            "{ids:?}"
        );
        // "fish " ends the first row in blue and "fi" starts the second in it.
        let rows_in_blue = colored
            .glyphs
            .iter()
            .filter(|glyph| glyph.3 == blue())
            .map(|glyph| glyph.1.to_bits())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(rows_in_blue.len(), 2);

        // Drawn without colors again, the text is as it was.
        let (cleared, _) = draw_with_colors(window, None, cx);
        assert_eq!(cleared, plain);
        assert_eq!(text_system.shaped.load(atomic::Ordering::SeqCst), shaped);

        // Colors in the text's runs would have split its font runs, and broken its ligatures.
        window
            .update(cx, |_, window, _| {
                let mut recolored = runs.clone();
                recolored[0].len = 1;
                recolored.insert(1, run(9, green()));
                let shape = |runs: &[TextRun]| {
                    window
                        .text_system()
                        .shape_text(text.into(), px(16.), runs, Some(px(72.)), None)
                        .unwrap()
                };
                let (plain, recolored) = (shape(&runs), shape(&recolored));
                assert_ne!(
                    plain[0].unwrapped_layout.width,
                    recolored[0].unwrapped_layout.width
                );
            })
            .unwrap();
    }

    /// Right-to-left glyphs come against byte order, and each still takes the color of the range
    /// that holds its first byte.
    #[test]
    fn glyph_colors_follow_right_to_left_glyphs() {
        let mut cx = TestAppContext::build_with_text_system(
            crate::TestDispatcher::new(0),
            None,
            Arc::new(LigatureTextSystem::default()),
        );
        let cx = &mut cx;
        // "a", a Hebrew word drawn from right to left, and "b".
        let text = "a \u{05e9}\u{05dc}\u{05d5}\u{05dd} b";
        let runs = vec![plain_run(text.len(), black())];
        let colors = vec![(0..4, green()), (6..11, blue())];
        let window = cx.open_window(size(px(400.), px(200.)), |_, _| ColoredTextView {
            text: text.into(),
            runs: runs.clone(),
            colors: None,
            layout: TextLayout::default(),
        });
        let (plain, lines) = draw_with_colors(window, None, cx);
        let (colored, _) = draw_with_colors(window, Some(colors.clone()), cx);

        let indices = lines[0]
            .unwrapped_layout
            .runs
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.index))
            .collect::<Vec<_>>();
        assert_eq!(indices, [0, 1, 8, 6, 4, 2, 10, 11]);
        assert_eq!(placed(&colored.glyphs), placed(&plain.glyphs));
        assert_eq!(
            painted(&colored.glyphs),
            expected_glyph_colors(&lines, &runs, &colors)
        );
        assert_eq!(
            painted(&colored.glyphs),
            [
                green(),
                green(),
                blue(),
                blue(),
                black(),
                green(),
                blue(),
                black()
            ]
        );
    }

    #[test]
    fn test_into_element_for() {
        use crate::{ParentElement as _, SharedString, div};
        use std::borrow::Cow;

        let _ = div().child("static str");
        let _ = div().child("String".to_string());
        let _ = div().child(Cow::Borrowed("Cow"));
        let _ = div().child(SharedString::from("SharedString"));
    }

    #[test]
    fn text_macro_id() {
        // one call to `text!` = one id
        fn make_text_stable_id(happy: bool) -> Text {
            text!(if happy { "happy" } else { "sad" })
        }

        // two calls to `text!` = two ids
        fn make_text_unstable_id(happy: bool) -> Text {
            if happy { text!("happy") } else { text!("sad") }
        }

        assert_eq!(make_text_stable_id(false).id, make_text_stable_id(true).id);
        assert_ne!(
            make_text_unstable_id(false).id,
            make_text_unstable_id(true).id
        );
    }
}
