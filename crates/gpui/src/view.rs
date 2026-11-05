// Modified by Mesa Hills Research, as docs/upstream.md in mesa-hills-research/gpui describes.
use crate::{
    AnyElement, AnyEntity, AnyWeakEntity, App, AvailableSpace, Bounds, ContentMask, Context,
    Element, ElementId, Entity, EntityId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, PaintIndex, Pixels, PrepaintStateIndex, Render, RenderOnce, Size, Style,
    StyleRefinement, TextStyle, WeakEntity,
};
use crate::{Empty, Window};
use anyhow::Result;
use collections::FxHashSet;
use refineable::Refineable;
use std::{
    any::{TypeId, type_name},
    fmt,
    ops::Range,
};

/// A dynamically-typed view handle that can be downcast to a specific `Entity<V>`.
///
/// This is the type-erased counterpart to [`ViewElement`]: it holds an entity plus
/// a function pointer to its render, and is itself a [`View`], so embedding it as an
/// element goes through the same [`ViewElement`] machinery as any other view.
#[derive(Clone, Debug)]
pub struct AnyView {
    entity: AnyEntity,
    render: fn(&AnyView, &mut Window, &mut App) -> AnyElement,
    view_type_name: &'static str,
}

impl<V: Render> From<Entity<V>> for AnyView {
    fn from(value: Entity<V>) -> Self {
        AnyView {
            entity: value.into_any(),
            render: any_view::render::<V>,
            view_type_name: type_name::<V>(),
        }
    }
}

impl AnyView {
    /// Embed this view as a cached [`ViewElement`] laid out at `style`.
    ///
    /// The rendered subtree is recycled from the previous frame unless
    /// [Context::notify] was called on the backing entity since it was rendered
    /// (or [Window::refresh] is called, which ignores caching). That holds
    /// inside another cached view too, when the outer view redraws.
    pub fn cached(self, style: StyleRefinement) -> ViewElement<AnyView> {
        self.into_element().cached(style)
    }

    /// Embed this view as a cached [`ViewElement`] of measured height, which a list can use for
    /// its items. See [`Entity::cached_measured`].
    pub fn cached_measured(self, style: StyleRefinement) -> ViewElement<AnyView> {
        ViewElement::new(self).cached_measured(style)
    }

    /// Convert this to a weak handle.
    pub fn downgrade(&self) -> AnyWeakView {
        AnyWeakView {
            entity: self.entity.downgrade(),
            render: self.render,
            view_type_name: self.view_type_name,
        }
    }

    /// Convert this to a [Entity] of a specific type.
    /// If this handle does not contain a view of the specified type, returns itself in an `Err` variant.
    pub fn downcast<T: 'static>(self) -> Result<Entity<T>, Self> {
        match self.entity.downcast() {
            Ok(entity) => Ok(entity),
            Err(entity) => Err(Self {
                entity,
                render: self.render,
                view_type_name: self.view_type_name,
            }),
        }
    }

    /// Gets the [TypeId] of the underlying view.
    pub fn entity_type(&self) -> TypeId {
        self.entity.entity_type
    }

    /// The [`EntityId`] of this view.
    pub fn entity_id(&self) -> EntityId {
        self.entity.entity_id()
    }
}

impl PartialEq for AnyView {
    fn eq(&self, other: &Self) -> bool {
        self.entity == other.entity
    }
}

impl Eq for AnyView {}

/// `AnyView` is the type-erased [`View`]: its `render` is a function pointer rather
/// than a concrete type, but it participates in the reactive graph exactly like any
/// other view via [`ViewElement`].
impl View for AnyView {
    fn entity_id(&self) -> Option<EntityId> {
        Some(self.entity.entity_id())
    }

    #[cfg(not(gpui_hot_reload))]
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        (self.render)(&self, window, cx)
    }

    // The stored function pointer was captured from the original binary, so it must be looked up
    // in the jump table to reach the patched render.
    #[cfg(gpui_hot_reload)]
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        subsecond::HotFn::current(self.render).call((&self, window, cx))
    }
}

impl<V: 'static + Render> IntoElement for Entity<V> {
    type Element = ViewElement<Entity<V>>;

    fn into_element(self) -> Self::Element {
        ViewElement::with_type_name(self, type_name::<V>())
    }

    #[inline(never)]
    fn into_any_element(self) -> AnyElement {
        self.into_element().into_any()
    }
}

impl IntoElement for AnyView {
    type Element = ViewElement<AnyView>;

    fn into_element(self) -> Self::Element {
        let view_type_name = self.view_type_name;
        ViewElement::with_type_name(self, view_type_name)
    }
}

/// A weak, dynamically-typed view handle.
pub struct AnyWeakView {
    entity: AnyWeakEntity,
    render: fn(&AnyView, &mut Window, &mut App) -> AnyElement,
    view_type_name: &'static str,
}

impl AnyWeakView {
    /// Upgrade to a strong `AnyView` handle, if the view is still alive.
    pub fn upgrade(&self) -> Option<AnyView> {
        let entity = self.entity.upgrade()?;
        Some(AnyView {
            entity,
            render: self.render,
            view_type_name: self.view_type_name,
        })
    }
}

impl<V: 'static + Render> From<WeakEntity<V>> for AnyWeakView {
    fn from(view: WeakEntity<V>) -> Self {
        AnyWeakView {
            entity: view.into(),
            render: any_view::render::<V>,
            view_type_name: type_name::<V>(),
        }
    }
}

impl PartialEq for AnyWeakView {
    fn eq(&self, other: &Self) -> bool {
        self.entity == other.entity
    }
}

impl std::fmt::Debug for AnyWeakView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnyWeakView")
            .field("entity_id", &self.entity.entity_id)
            .finish_non_exhaustive()
    }
}

mod any_view {
    use crate::{AnyElement, AnyView, App, IntoElement, Render, Window};

    pub(crate) fn render<V: 'static + Render>(
        view: &AnyView,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let view = view.clone().downcast::<V>().unwrap();
        // Record the view's Render type name so the accessibility debug dump can
        // attribute nodes to the view that produced them.
        #[cfg(debug_assertions)]
        window
            .a11y
            .view_type_names
            .insert(view.entity_id(), std::any::type_name::<V>());
        view.update(cx, |view, cx| view.render(window, cx).into_any_element())
    }
}

/// A renderable that participates in GPUI's reactive graph — the unifying model
/// behind [`Render`] and [`RenderOnce`].
///
/// When `entity_id()` returns `Some`, that id becomes the view's identity: it gets
/// a unique element-id space (so internal `use_state` / `.id(..)` never collide
/// across siblings) and `cx.notify()` on that entity re-renders only this view's
/// subtree. `None` behaves like a stateless component.
///
/// You rarely implement `View` directly. `Entity<T: Render>` and any `T: RenderOnce`
/// get a blanket impl below; implement it by hand only when a component needs both
/// parent-supplied props *and* a backing entity for identity.
pub trait View: 'static + Sized {
    /// This view's identity, if it has one. A view typically holds the backing
    /// entity as a field and returns its [`EntityId`] here.
    ///
    /// The id becomes this view's [`ElementId`], so two views keyed on the same
    /// entity must not be rendered at the same position in the element tree
    /// (e.g. as siblings under the same parent): their internal element state
    /// (`use_state`, scroll offsets, etc.) would silently collide. Nesting is
    /// fine — the id is scoped by the parent path.
    fn entity_id(&self) -> Option<EntityId>;

    /// Render this view into an element tree, consuming `self`.
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement;
}

/// A stateless component (`RenderOnce`) is a `View` with no identity.
impl<T: RenderOnce> View for T {
    fn entity_id(&self) -> Option<EntityId> {
        None
    }

    #[inline]
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        RenderOnce::render(self, window, cx)
    }
}

/// An entity that renders itself (`Render`) is a `View` keyed on its own id.
impl<T: Render> View for Entity<T> {
    fn entity_id(&self) -> Option<EntityId> {
        Some(Entity::entity_id(self))
    }

    #[inline]
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.update(cx, |this, cx| {
            Render::render(this, window, cx).into_any_element()
        })
    }
}

impl<T: Render> Entity<T> {
    /// Embed this entity as a cached [`ViewElement`] whose height is measured from its contents
    /// and remembered, so that a list can use it for an item of any height.
    ///
    /// `style` gives the view's width, such as `StyleRefinement::default().w_full()`. The first
    /// time the view is laid out, and whenever the entity is notified or the width it is laid out
    /// at changes, it renders and is measured. Otherwise it takes the height it measured and is
    /// reused as [`Entity::cached`] does. The view must be the item itself, the element a list's
    /// render function returns, because only the root of a layout knows its width. Anywhere else
    /// it renders in every frame, as an uncached view does.
    ///
    /// A row of a [`list`](crate::list) that animates with this redraws alone, instead of
    /// with every row the list shows.
    #[track_caller]
    pub fn cached_measured(self, style: StyleRefinement) -> ViewElement<Entity<T>> {
        ViewElement::new(self).cached_measured(style)
    }

    /// Embed this entity as a cached [`ViewElement`] laid out at `style`.
    ///
    /// The rendered subtree is reused until the entity is notified (or the
    /// cached bounds / text style change), inside another cached view too,
    /// when the outer view redraws. Caching requires a definite size:
    /// a cached view is laid out from `style` and is *not* measured from its
    /// contents. Use [`Entity::cached_measured`] to remember a height measured from
    /// the contents, and [`ViewElement::new`] (or `.child(entity)`) for the
    /// uncached case.
    #[track_caller]
    pub fn cached(self, style: StyleRefinement) -> ViewElement<Entity<T>> {
        ViewElement::with_type_name(self, type_name::<T>()).cached(style)
    }
}

/// The element type for [`View`] implementations. Wraps a `View` and hooks it
/// into layout, prepaint, and paint. Constructed via [`ViewElement::new`].
#[doc(hidden)]
pub struct ViewElement<V: View> {
    view: Option<V>,
    entity_id: Option<EntityId>,
    cached_style: Option<StyleRefinement>,
    /// Whether a cached view takes its height from what it measured, rather than from its style.
    measured: bool,
    #[cfg(feature = "profiler")]
    view_type_name: &'static str,
    #[cfg(debug_assertions)]
    source: &'static core::panic::Location<'static>,
}

impl<V: View> ViewElement<V> {
    /// Wrap a [`View`] as an element.
    #[track_caller]
    pub fn new(view: V) -> Self {
        Self::with_type_name(view, type_name::<V>())
    }

    /// Wraps a view whose draw time profiling attributes to `view_type_name`,
    /// such as an entity's type rather than [`Entity`]'s.
    #[track_caller]
    #[cfg_attr(not(feature = "profiler"), expect(unused_variables))]
    pub(crate) fn with_type_name(view: V, view_type_name: &'static str) -> Self {
        let entity_id = view.entity_id();
        ViewElement {
            entity_id,
            cached_style: None,
            measured: false,
            #[cfg(feature = "profiler")]
            view_type_name,
            view: Some(view),
            #[cfg(debug_assertions)]
            source: core::panic::Location::caller(),
        }
    }

    /// Enable caching of this view's rendered subtree, laid out at `style`.
    /// The composer supplies the layout style because caching skips rendering
    /// the contents to measure them.
    ///
    /// Crate-private on purpose: caching is only sound for entity-backed views,
    /// where [`Context::notify`] is the contract that busts the cache. A stateless
    /// view has no such contract, so a frozen subtree could never be invalidated.
    /// Reach this through [`Entity::cached`] or [`AnyView::cached`], which are
    /// entity-backed by construction.
    pub(crate) fn cached(mut self, style: StyleRefinement) -> Self {
        self.cached_style = Some(style);
        self
    }

    /// Like [`Self::cached`], with a height measured from the contents. See
    /// [`Entity::cached_measured`].
    pub(crate) fn cached_measured(mut self, style: StyleRefinement) -> Self {
        self.cached_style = Some(style);
        self.measured = true;
        self
    }
}

impl<V: View> IntoElement for ViewElement<V> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

pub(crate) struct ViewElementState {
    prepaint_range: Range<PrepaintStateIndex>,
    paint_range: Range<PaintIndex>,
    /// The [`Window::frame_number`]s of the frames that `prepaint_range` and
    /// `paint_range` index. A view is reused only if both are the rendered
    /// frame's.
    prepaint_frame: u64,
    paint_frame: u64,
    cache_key: ViewElementCacheKey,
    accessed_entities: FxHashSet<EntityId>,
    /// For a cached view of measured height, what it measured.
    measured: Option<MeasuredSize>,
}

/// The height a cached view of measured height took at a width.
#[derive(Clone)]
struct MeasuredSize {
    /// The width the view was asked to lay out at.
    available_width: Pixels,
    height: Pixels,
    /// What the layout depended on that is not in the view's own state.
    text_style: TextStyle,
    rem_size: Pixels,
}

impl ViewElementState {
    /// Follows a range of the rendered frame, starting at `from`, that this
    /// view was drawn inside, to `to` in frame `frame_number`, where
    /// [`Window::reuse_prepaint`] copied it.
    pub(crate) fn follow_reused_prepaint(
        &mut self,
        from: &PrepaintStateIndex,
        to: &PrepaintStateIndex,
        frame_number: u64,
    ) {
        if self.prepaint_frame + 1 == frame_number {
            let range = &self.prepaint_range;
            self.prepaint_range = range.start.rebased(from, to)..range.end.rebased(from, to);
            self.prepaint_frame = frame_number;
        }
    }

    /// Follows a range of the rendered frame, starting at `from`, that this
    /// view was painted inside, to `to` in frame `frame_number`, where
    /// [`Window::reuse_paint`] copied it.
    pub(crate) fn follow_reused_paint(
        &mut self,
        from: &PaintIndex,
        to: &PaintIndex,
        frame_number: u64,
    ) {
        if self.paint_frame + 1 == frame_number {
            let range = &self.paint_range;
            self.paint_range = range.start.rebased(from, to)..range.end.rebased(from, to);
            self.paint_frame = frame_number;
        }
    }
}

struct ViewElementCacheKey {
    bounds: Bounds<Pixels>,
    content_mask: ContentMask<Pixels>,
    text_style: TextStyle,
    opacity: f32,
}

#[cfg(not(gpui_hot_reload))]
#[inline]
fn render_view<V: View>(view: V, window: &mut Window, cx: &mut App) -> AnyElement {
    view.render(window, cx).into_any_element()
}

/// Every view and component render passes through this hot-patchable boundary, so the
/// patched code for that render and everything it calls runs on the next frame.
#[cfg(gpui_hot_reload)]
fn render_view<V: View>(view: V, window: &mut Window, cx: &mut App) -> AnyElement {
    #[inline(never)]
    fn render_view_impl<V: View>(view: V, window: &mut Window, cx: &mut App) -> AnyElement {
        view.render(window, cx).into_any_element()
    }

    subsecond::HotFn::current(render_view_impl::<V>).call((view, window, cx))
}

impl<V: View> Element for ViewElement<V> {
    type RequestLayoutState = Option<AnyElement>;
    type PrepaintState = Option<AnyElement>;

    fn id(&self) -> Option<ElementId> {
        self.entity_id.map(ElementId::View)
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        #[cfg(debug_assertions)]
        return Some(self.source);

        #[cfg(not(debug_assertions))]
        return None;
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        // Only a view at the root of the layout knows the space it's laid out in.
        let root_space = window.pending_root_space.take();
        if let Some(entity_id) = self.entity_id {
            // Stateful path: create a reactive boundary.
            #[cfg(feature = "profiler")]
            let start = window.window_profiler.begin_view();
            let view = &mut self.view;
            let layout = request_layout_view(
                entity_id,
                id,
                self.cached_style.as_ref(),
                self.measured,
                root_space,
                window,
                cx,
                &mut |window, cx| render_view(view.take().unwrap(), window, cx),
            );
            #[cfg(feature = "profiler")]
            window.window_profiler.end_view(
                start,
                self.view_type_name,
                crate::profiler::ViewPhase::RequestLayout,
            );
            layout
        } else {
            // Stateless path: isolate subtree via type name (no entity identity).
            request_layout_component(type_name::<V>(), window, cx, &mut |window, cx| {
                render_view(self.view.take().unwrap(), window, cx)
            })
        }
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        element: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        if let Some(entity_id) = self.entity_id {
            // Stateful path.
            #[cfg(feature = "profiler")]
            let start = window.window_profiler.begin_view();
            let prepainted = prepaint_view(
                entity_id,
                global_id,
                self.cached_style.is_some() && self.measured,
                bounds,
                element,
                window,
                cx,
                &mut |window, cx| render_view(self.view.take().unwrap(), window, cx),
            );
            #[cfg(feature = "profiler")]
            window.window_profiler.end_view(
                start,
                self.view_type_name,
                crate::profiler::ViewPhase::Prepaint,
            );
            prepainted
        } else {
            // Stateless path: just prepaint the element.
            prepaint_component(type_name::<V>(), element, window, cx)
        }
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        element: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(entity_id) = self.entity_id {
            // Stateful path.
            #[cfg(feature = "profiler")]
            let start = window.window_profiler.begin_view();
            paint_view(
                entity_id,
                self.cached_style.is_some(),
                global_id,
                element,
                window,
                cx,
            );
            #[cfg(feature = "profiler")]
            window.window_profiler.end_view(
                start,
                self.view_type_name,
                crate::profiler::ViewPhase::Paint,
            );
        } else {
            // Stateless path: just paint the element.
            paint_component(std::any::type_name::<V>(), element, window, cx);
        }
    }
}

/// A view that renders nothing
pub struct EmptyView;

impl Render for EmptyView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[inline(never)]
fn request_layout_view(
    entity_id: EntityId,
    global_id: Option<&GlobalElementId>,
    cached_style: Option<&StyleRefinement>,
    measured: bool,
    root_space: Option<Size<AvailableSpace>>,
    window: &mut Window,
    cx: &mut App,
    render: &mut dyn FnMut(&mut Window, &mut App) -> AnyElement,
) -> (LayoutId, Option<AnyElement>) {
    window.with_rendered_view(entity_id, |window| {
        let caching_disabled = window.is_inspector_picking(cx);
        match cached_style {
            Some(style) if measured && !caching_disabled => request_layout_measured_view(
                entity_id,
                global_id.unwrap(),
                style,
                root_space,
                window,
                cx,
                render,
            ),
            Some(style) if !caching_disabled => {
                let mut root_style = Style::default();
                root_style.refine(style);
                let layout_id = window.request_layout(root_style, None, cx);
                (layout_id, None)
            }
            _ => {
                let mut element = render(window, cx);
                let layout_id = element.request_layout(window, cx);
                (layout_id, Some(element))
            }
        }
    })
}

/// The height a cached view of measured height took the last time, if it can be taken again for
/// a layout `width` wide.
fn measured_height(
    window: &Window,
    entity_id: EntityId,
    global_id: &GlobalElementId,
    width: Pixels,
) -> Option<Pixels> {
    let frame_number = window.frame_number;
    if window.refreshing
        || window.dirty_views.contains(&entity_id)
        || !window.a11y.can_reuse_views(frame_number)
    {
        return None;
    }
    let key = (global_id.clone(), TypeId::of::<ViewElementState>());
    let state = window
        .next_frame
        .element_states
        .get(&key)
        .or_else(|| window.rendered_frame.element_states.get(&key))?;
    let state = state
        .inner
        .downcast_ref::<Option<ViewElementState>>()?
        .as_ref()?;
    let measured = state.measured.as_ref()?;
    (state.prepaint_frame + 1 == frame_number
        && state.paint_frame + 1 == frame_number
        && measured.available_width == width
        && measured.rem_size == window.rem_size()
        && measured.text_style == window.text_style())
    .then_some(measured.height)
}

/// Lays out a cached view of measured height. A view that is clean and was measured at this width
/// is a leaf with the height it measured, and renders nothing. Any other view renders now, and
/// its contents are laid out in a box of `style` whose height is whatever they come to.
fn request_layout_measured_view(
    entity_id: EntityId,
    global_id: &GlobalElementId,
    style: &StyleRefinement,
    root_space: Option<Size<AvailableSpace>>,
    window: &mut Window,
    cx: &mut App,
    render: &mut dyn FnMut(&mut Window, &mut App) -> AnyElement,
) -> (LayoutId, Option<AnyElement>) {
    let mut root_style = Style::default();
    root_style.refine(style);
    let width = match root_space {
        Some(Size {
            width: AvailableSpace::Definite(width),
            ..
        }) => Some(width),
        _ => None,
    };

    if let Some(width) = width
        && let Some(height) = measured_height(window, entity_id, global_id, width)
    {
        root_style.size.height = height.into();
        let layout_id = window.request_layout(root_style, None, cx);
        return (layout_id, None);
    }

    // The state is made here, so that the accessed entities are known, and finished when the
    // view is prepainted.
    window.with_element_state::<ViewElementState, _>(global_id, |_, window| {
        let ((layout_id, element), accessed_entities) = cx.detect_accessed_entities(|cx| {
            let mut element = render(window, cx);
            let content = element.request_layout(window, cx);
            (window.request_layout(root_style, [content], cx), element)
        });
        (
            (layout_id, Some(element)),
            ViewElementState {
                prepaint_range: PrepaintStateIndex::default()..PrepaintStateIndex::default(),
                paint_range: PaintIndex::default()..PaintIndex::default(),
                prepaint_frame: 0,
                paint_frame: 0,
                cache_key: ViewElementCacheKey {
                    bounds: Bounds::default(),
                    content_mask: window.content_mask(),
                    text_style: window.text_style(),
                    opacity: window.element_opacity(),
                },
                accessed_entities,
                measured: width.map(|width| MeasuredSize {
                    available_width: width,
                    // Set when the view is prepainted.
                    height: Pixels::ZERO,
                    text_style: window.text_style(),
                    rem_size: window.rem_size(),
                }),
            },
        )
    })
}

#[inline(never)]
fn request_layout_component(
    name: &'static str,
    window: &mut Window,
    cx: &mut App,
    render: &mut dyn FnMut(&mut Window, &mut App) -> AnyElement,
) -> (LayoutId, Option<AnyElement>) {
    window.with_id(ElementId::from(name), |window| {
        let mut element = render(window, cx);
        let layout_id = element.request_layout(window, cx);
        (layout_id, Some(element))
    })
}

#[inline(never)]
fn prepaint_view(
    entity_id: EntityId,
    global_id: Option<&GlobalElementId>,
    measured: bool,
    bounds: Bounds<Pixels>,
    element: &mut Option<AnyElement>,
    window: &mut Window,
    cx: &mut App,
    render: &mut dyn FnMut(&mut Window, &mut App) -> AnyElement,
) -> Option<AnyElement> {
    window.set_view_id(entity_id);
    window.with_rendered_view(entity_id, |window| {
        if let Some(mut element) = element.take() {
            if measured && !window.is_inspector_picking(cx) {
                // A cached view of measured height that rendered when it was laid out.
                return Some(window.with_element_state::<ViewElementState, _>(
                    global_id.unwrap(),
                    |element_state, window| {
                        let mut element_state = element_state
                            .expect("a view of measured height makes its state when laid out");
                        let prepaint_start = window.prepaint_index();
                        let (_, accessed_entities) =
                            cx.detect_accessed_entities(|cx| element.prepaint(window, cx));
                        element_state.accessed_entities.extend(accessed_entities);
                        element_state.prepaint_range = prepaint_start..window.prepaint_index();
                        element_state.prepaint_frame = window.frame_number;
                        element_state.cache_key = ViewElementCacheKey {
                            bounds,
                            content_mask: window.content_mask(),
                            text_style: window.text_style(),
                            opacity: window.element_opacity(),
                        };
                        if let Some(measured) = &mut element_state.measured {
                            measured.height = bounds.size.height;
                        }
                        (element, element_state)
                    },
                ));
            }
            element.prepaint(window, cx);
            return Some(element);
        }

        window.with_element_state::<ViewElementState, _>(
            global_id.unwrap(),
            |element_state, window| {
                let content_mask = window.content_mask();
                let text_style = window.text_style();
                let frame_number = window.frame_number;
                // A view that was reused for its height keeps it when it draws again.
                let measured = element_state
                    .as_ref()
                    .and_then(|element_state| element_state.measured.clone());
                let opacity = window.element_opacity();

                // A view inside another cached view that redraws is reused too,
                // unless it was notified itself. Opacity is compared by bits so that
                // a NaN opacity still matches itself and the view keeps using its cache.
                if let Some(mut element_state) = element_state
                    && element_state.cache_key.bounds == bounds
                    && element_state.cache_key.content_mask == content_mask
                    && element_state.cache_key.text_style == text_style
                    && element_state.prepaint_frame + 1 == frame_number
                    && element_state.paint_frame + 1 == frame_number
                    && element_state.cache_key.opacity.to_bits() == opacity.to_bits()
                    && !window.dirty_views.contains(&entity_id)
                    && !window.refreshing
                    && window.a11y.can_reuse_views(frame_number)
                {
                    let prepaint_start = window.prepaint_index();
                    window.reuse_prepaint(element_state.prepaint_range.clone());
                    cx.entities
                        .extend_accessed(&element_state.accessed_entities);
                    let prepaint_end = window.prepaint_index();
                    element_state.prepaint_range = prepaint_start..prepaint_end;
                    element_state.prepaint_frame = frame_number;

                    return (None, element_state);
                }

                let prepaint_start = window.prepaint_index();
                let (element, accessed_entities) = cx.detect_accessed_entities(|cx| {
                    let mut element = render(window, cx);
                    element.layout_as_root(Size::<AvailableSpace>::from(bounds.size), window, cx);
                    element.prepaint_at(bounds.origin, window, cx);
                    element
                });

                let prepaint_end = window.prepaint_index();

                (
                    Some(element),
                    ViewElementState {
                        accessed_entities,
                        prepaint_range: prepaint_start..prepaint_end,
                        paint_range: PaintIndex::default()..PaintIndex::default(),
                        prepaint_frame: frame_number,
                        // Set when the view is painted in this frame.
                        paint_frame: 0,
                        cache_key: ViewElementCacheKey {
                            bounds,
                            content_mask,
                            text_style,
                            opacity,
                        },
                        measured,
                    },
                )
            },
        )
    })
}

#[inline(never)]
fn prepaint_component(
    name: &'static str,
    element: &mut Option<AnyElement>,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    window.with_id(ElementId::from(name), |window| {
        element.as_mut().unwrap().prepaint(window, cx);
    });
    Some(element.take().unwrap())
}

#[inline(never)]
fn paint_view(
    entity_id: EntityId,
    cached: bool,
    global_id: Option<&GlobalElementId>,
    element: &mut Option<AnyElement>,
    window: &mut Window,
    cx: &mut App,
) {
    window.with_rendered_view(entity_id, |window| {
        let caching_disabled = window.is_inspector_picking(cx);
        if cached && !caching_disabled {
            window.with_element_state::<ViewElementState, _>(
                global_id.unwrap(),
                |element_state, window| {
                    let mut element_state = element_state.unwrap();

                    let paint_start = window.paint_index();
                    let opacity = window.element_opacity();

                    // Prepaint can only predict the opacity paint applies: an ancestor's
                    // hover or drag-over style depends on this frame's hit test, which
                    // isn't known until prepaint finishes. The key records what was
                    // actually painted, and output replayed under a different opacity is
                    // redrawn on the next frame.
                    if let Some(element) = element {
                        element.paint(window, cx);
                        element_state.cache_key.opacity = opacity;
                    } else {
                        window.reuse_paint(element_state.paint_range.clone());
                        if element_state.cache_key.opacity.to_bits() != opacity.to_bits() {
                            window.request_animation_frame();
                        }
                    }

                    let paint_end = window.paint_index();
                    element_state.paint_range = paint_start..paint_end;
                    element_state.paint_frame = window.frame_number;

                    ((), element_state)
                },
            )
        } else {
            element.as_mut().unwrap().paint(window, cx);
        }
    });
}

#[inline(never)]
fn paint_component(
    name: &'static str,
    element: &mut Option<AnyElement>,
    window: &mut Window,
    cx: &mut App,
) {
    window.with_id(ElementId::Name(name.into()), |window| {
        element.as_mut().unwrap().paint(window, cx);
    });
}

#[cfg(test)]
mod tests {
    use crate::{
        AnyWindowHandle, App, Background, Bounds, ContentMask, Context, Entity, FocusHandle,
        FollowMode, Hsla, ListAlignment, ListState, Modifiers, Pixels, Role, ScaledPixels,
        StyleRefinement, TestAppContext, VisualTestContext, Window, WindowHandle, accesskit, blue,
        deferred, div, green, hsla, list, point, prelude::*, px, red, size,
    };
    use rand::{Rng as _, SeedableRng as _, rngs::StdRng};
    use std::{cell::Cell, rc::Rc};

    const GROUP_COUNT: usize = 4;
    const GROUP_HEIGHT: f32 = 40.;

    /// A cached group in a pane, which counts its renders and its clicks.
    struct Group {
        color: Hsla,
        renders: Rc<Cell<usize>>,
        clicks: Rc<Cell<usize>>,
    }

    impl Render for Group {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            let clicks = self.clicks.clone();
            div()
                .id("group")
                .size_full()
                .bg(self.color)
                .on_click(move |_, _, _| clicks.set(clicks.get() + 1))
        }
    }

    /// A cached pane of cached groups, one of which it can hide.
    struct Pane {
        groups: Vec<Entity<Group>>,
        hidden_group: Option<usize>,
        renders: Rc<Cell<usize>>,
    }

    impl Render for Pane {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            div()
                .flex()
                .flex_col()
                .size_full()
                .children(self.groups.iter().enumerate().map(|(ix, group)| {
                    div()
                        .w_full()
                        .h(px(GROUP_HEIGHT))
                        .when(self.hidden_group == Some(ix), |this| this.invisible())
                        .child(group.clone().cached(StyleRefinement::default().size_full()))
                }))
        }
    }

    /// A cached view holding the pane, so that the groups are two cached views deep.
    struct Outer {
        pane: Entity<Pane>,
        renders: Rc<Cell<usize>>,
    }

    impl Render for Outer {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            div().size_full().child(
                self.pane
                    .clone()
                    .cached(StyleRefinement::default().size_full()),
            )
        }
    }

    /// The window's root. Each time it's notified it draws one more small
    /// element ahead of the outer view, so the outer view's drawing lands at
    /// other positions in the frame.
    struct Root {
        outer: Entity<Outer>,
        leading: usize,
    }

    impl Render for Root {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .children((0..self.leading).map(|ix| {
                    div()
                        .id(ix)
                        .absolute()
                        .size(px(1.))
                        .bg(blue())
                        .on_click(|_, _, _| {})
                }))
                .child(
                    self.outer
                        .clone()
                        .cached(StyleRefinement::default().size_full()),
                )
        }
    }

    struct Fixture {
        window: WindowHandle<Root>,
        pane: Entity<Pane>,
        groups: Vec<Entity<Group>>,
        outer_renders: Rc<Cell<usize>>,
        pane_renders: Rc<Cell<usize>>,
        group_renders: Vec<Rc<Cell<usize>>>,
        group_clicks: Vec<Rc<Cell<usize>>>,
    }

    impl Fixture {
        fn open(cx: &mut TestAppContext) -> Self {
            let outer_renders = Rc::new(Cell::new(0));
            let pane_renders = Rc::new(Cell::new(0));
            let group_renders: Vec<_> = (0..GROUP_COUNT).map(|_| Rc::new(Cell::new(0))).collect();
            let group_clicks: Vec<_> = (0..GROUP_COUNT).map(|_| Rc::new(Cell::new(0))).collect();
            let window = cx.open_window(size(px(200.), px(200.)), |_, cx| {
                let groups = (0..GROUP_COUNT)
                    .map(|ix| {
                        cx.new(|_| Group {
                            color: red(),
                            renders: group_renders[ix].clone(),
                            clicks: group_clicks[ix].clone(),
                        })
                    })
                    .collect();
                let pane = cx.new(|_| Pane {
                    groups,
                    hidden_group: None,
                    renders: pane_renders.clone(),
                });
                let outer = cx.new(|_| Outer {
                    pane,
                    renders: outer_renders.clone(),
                });
                Root { outer, leading: 0 }
            });
            cx.run_until_parked();
            let (pane, groups) = window
                .update(cx, |root, _, cx| {
                    let pane = root.outer.read(cx).pane.clone();
                    let groups = pane.read(cx).groups.clone();
                    (pane, groups)
                })
                .unwrap();
            Self {
                window,
                pane,
                groups,
                outer_renders,
                pane_renders,
                group_renders,
                group_clicks,
            }
        }

        /// How many times the outer view, the pane and each group rendered.
        fn renders(&self) -> (usize, usize, Vec<usize>) {
            (
                self.outer_renders.get(),
                self.pane_renders.get(),
                self.group_renders
                    .iter()
                    .map(|renders| renders.get())
                    .collect(),
            )
        }

        /// The groups' backgrounds in the rendered frame, from top to bottom,
        /// with their tops in pixels.
        fn painted_groups(&self, cx: &mut TestAppContext) -> Vec<(f32, Background)> {
            self.window
                .update(cx, |_, window, _| {
                    let scale_factor = window.scale_factor();
                    let mut quads: Vec<_> = window
                        .rendered_frame
                        .scene
                        .quads
                        .iter()
                        .filter(|quad| quad.bounds.size.height.0 / scale_factor == GROUP_HEIGHT)
                        .map(|quad| (quad.bounds.origin.y.0 / scale_factor, quad.background))
                        .collect();
                    quads.sort_by(|a, b| a.0.total_cmp(&b.0));
                    quads
                })
                .unwrap()
        }

        fn expect_painted(&self, cx: &mut TestAppContext, colors: [Hsla; GROUP_COUNT]) {
            let expected: Vec<_> = colors
                .iter()
                .enumerate()
                .map(|(ix, color)| (ix as f32 * GROUP_HEIGHT, Background::from(*color)))
                .collect();
            assert_eq!(self.painted_groups(cx), expected);
        }

        /// Clicks each group once and checks that each one's own handler ran.
        fn click_each_group(&self, cx: &mut TestAppContext) {
            let mut cx = VisualTestContext::from_window(AnyWindowHandle::from(self.window), cx);
            for (ix, clicks) in self.group_clicks.iter().enumerate() {
                let before = clicks.get();
                let y = (ix as f32 + 0.5) * GROUP_HEIGHT;
                cx.simulate_click(point(px(100.), px(y)), Modifiers::none());
                assert_eq!(clicks.get(), before + 1, "group {ix} handles its click");
            }
        }

        fn notify_root(&self, cx: &mut TestAppContext) {
            self.window
                .update(cx, |root, _, cx| {
                    root.leading += 1;
                    cx.notify();
                })
                .unwrap();
            cx.run_until_parked();
        }

        fn recolor_group(&self, cx: &mut TestAppContext, ix: usize, color: Hsla) {
            self.groups[ix].update(cx, |group, cx| {
                group.color = color;
                cx.notify();
            });
            cx.run_until_parked();
        }
    }

    #[gpui::test]
    fn a_clean_nested_view_is_reused_when_the_view_around_it_redraws(cx: &mut TestAppContext) {
        let fixture = Fixture::open(cx);
        assert_eq!(fixture.renders(), (1, 1, vec![1; GROUP_COUNT]));

        fixture.pane.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        assert_eq!(fixture.renders(), (2, 2, vec![1; GROUP_COUNT]));
        fixture.expect_painted(cx, [red(); GROUP_COUNT]);
        fixture.click_each_group(cx);
    }

    #[gpui::test]
    fn a_notified_nested_view_redraws_alone(cx: &mut TestAppContext) {
        let fixture = Fixture::open(cx);

        fixture.recolor_group(cx, 1, blue());
        assert_eq!(fixture.renders(), (2, 2, vec![1, 2, 1, 1]));
        fixture.expect_painted(cx, [red(), blue(), red(), red()]);
        fixture.click_each_group(cx);
    }

    #[gpui::test]
    fn nested_views_stay_reusable_while_the_views_around_them_are_reused(cx: &mut TestAppContext) {
        let fixture = Fixture::open(cx);

        // The outer view is reused whole twice, so the groups aren't drawn,
        // and then each group in turn redraws while the others are reused.
        fixture.notify_root(cx);
        fixture.notify_root(cx);
        assert_eq!(fixture.renders(), (1, 1, vec![1; GROUP_COUNT]));
        let mut colors = [red(); GROUP_COUNT];
        for ix in 0..GROUP_COUNT {
            colors[ix] = green();
            fixture.recolor_group(cx, ix, green());
            fixture.notify_root(cx);
            fixture.expect_painted(cx, colors);
        }
        assert_eq!(fixture.renders(), (5, 5, vec![2; GROUP_COUNT]));
        fixture.click_each_group(cx);
    }

    #[gpui::test]
    fn a_refresh_or_a_resize_redraws_every_nested_view(cx: &mut TestAppContext) {
        let fixture = Fixture::open(cx);

        fixture
            .window
            .update(cx, |_, window, _| window.refresh())
            .unwrap();
        cx.run_until_parked();
        assert_eq!(fixture.renders(), (2, 2, vec![2; GROUP_COUNT]));

        cx.simulate_window_resize(fixture.window.into(), size(px(300.), px(250.)));
        cx.run_until_parked();
        assert_eq!(fixture.renders(), (3, 3, vec![3; GROUP_COUNT]));
        fixture.expect_painted(cx, [red(); GROUP_COUNT]);
    }

    #[gpui::test]
    fn a_nested_view_hidden_in_the_last_frame_redraws_when_shown(cx: &mut TestAppContext) {
        let fixture = Fixture::open(cx);

        // Hidden, the group is laid out but not painted.
        fixture.pane.update(cx, |pane, cx| {
            pane.hidden_group = Some(2);
            cx.notify();
        });
        cx.run_until_parked();
        let painted = fixture.painted_groups(cx);
        assert_eq!(painted.len(), GROUP_COUNT - 1);
        assert!(painted.iter().all(|(top, _)| *top != 2. * GROUP_HEIGHT));

        // What it painted two frames ago is gone, so it draws again.
        fixture.pane.update(cx, |pane, cx| {
            pane.hidden_group = None;
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(fixture.renders(), (3, 3, vec![1, 1, 2, 1]));
        fixture.expect_painted(cx, [red(); GROUP_COUNT]);
        fixture.click_each_group(cx);
    }

    /// A list of accessible items whose third asks to scroll to reveal part of the one above.
    struct AutoscrollList(ListState);

    impl Render for AutoscrollList {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            list(self.0.clone(), |ix, _, _| {
                let item = div()
                    .id(("item", ix))
                    .role(Role::ListItem)
                    .aria_label(format!("item {ix}"))
                    .h(px(20.))
                    .w_full();
                if ix == 2 {
                    item.child(
                        crate::canvas(
                            |bounds, window, _| {
                                window.request_autoscroll(Bounds::from_corners(
                                    point(bounds.left(), bounds.top() - px(30.)),
                                    point(bounds.right(), bounds.top() + px(5.)),
                                ));
                            },
                            |_, _, _, _| {},
                        )
                        .size_full(),
                    )
                    .into_any_element()
                } else {
                    item.into_any_element()
                }
            })
            .size_full()
        }
    }

    #[gpui::test]
    fn a_list_that_draws_its_items_twice_reports_them_once(cx: &mut TestAppContext) {
        let state = ListState::new(5, ListAlignment::Top, px(10.));
        state.scroll_to(crate::ListOffset {
            item_ix: 2,
            offset_in_item: px(0.),
        });
        let window = cx.open_window(size(px(100.), px(60.)), {
            let state = state.clone();
            move |window, _| {
                // Accessibility is active when the first frame, which scrolls, is drawn.
                window.a11y.activate_for_test();
                AutoscrollList(state)
            }
        });
        cx.run_until_parked();

        let tree = a11y_tree(window.into(), cx);
        let mut ids: Vec<_> = tree.nodes.iter().map(|(id, _)| *id).collect();
        let count = ids.len();
        ids.sort_by_key(|id| id.0);
        ids.dedup();
        assert_eq!(ids.len(), count, "a node is in the tree twice");
        // The list scrolled to 10px into the first item, so it shows the first four.
        assert_eq!(state.logical_scroll_top().item_ix, 0);
        let scale_factor = cx
            .update_window(window.into(), |_, window, _| window.scale_factor())
            .unwrap() as f64;
        let items: Vec<_> = a11y_shape(&tree)
            .into_iter()
            .filter_map(|node| Some((node.label?, node.bounds?.y0)))
            .collect();
        // Each is where the second drawing put it, the first item 10px above the list.
        let expected: Vec<_> = (0..4)
            .map(|ix| (format!("item {ix}"), (ix as f64 * 20. - 10.) * scale_factor))
            .collect();
        assert_eq!(items, expected);
    }

    /// A cached list of accessible items. The first reacts to clicks, the
    /// second to the screen reader's click action, and the third can be
    /// focused.
    struct Panel {
        focus: FocusHandle,
        renders: Rc<Cell<usize>>,
        clicks: Rc<Cell<usize>>,
        actions: Rc<Cell<usize>>,
    }

    impl Render for Panel {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            let clicks = self.clicks.clone();
            let actions = self.actions.clone();
            div()
                .id("panel")
                .role(Role::List)
                .aria_label("panel")
                .flex()
                .flex_col()
                .size_full()
                .child(
                    div()
                        .id("click")
                        .role(Role::ListItem)
                        .aria_label("click")
                        .h(px(20.))
                        .w_full()
                        .on_click(move |_, _, _| clicks.set(clicks.get() + 1)),
                )
                .child(
                    div()
                        .id("action")
                        .role(Role::ListItem)
                        .aria_label("action")
                        .h(px(20.))
                        .w_full()
                        .on_a11y_action(accesskit::Action::Click, move |_, _, _| {
                            actions.set(actions.get() + 1)
                        }),
                )
                .child(
                    div()
                        .id("focus")
                        .role(Role::ListItem)
                        .aria_label("focus")
                        .track_focus(&self.focus)
                        .h(px(20.))
                        .w_full()
                        .child(div().id("inner").role(Role::Label).aria_label("inner")),
                )
        }
    }

    /// The window's root, which can redraw without its panel.
    struct Shell {
        panel: Entity<Panel>,
        leading: usize,
    }

    impl Render for Shell {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("shell")
                .role(Role::Group)
                .size_full()
                .children((0..self.leading).map(|ix| {
                    div()
                        .id(("leading", ix))
                        .role(Role::Label)
                        .aria_label("leading")
                        .absolute()
                        .size(px(1.))
                }))
                .child(
                    self.panel
                        .clone()
                        .cached(StyleRefinement::default().size_full()),
                )
        }
    }

    struct PanelFixture {
        window: WindowHandle<Shell>,
        panel: Entity<Panel>,
        focus: FocusHandle,
        renders: Rc<Cell<usize>>,
        clicks: Rc<Cell<usize>>,
        actions: Rc<Cell<usize>>,
    }

    impl PanelFixture {
        /// Opens the window and draws it once, with accessibility active
        /// unless `active` is false.
        fn open(cx: &mut TestAppContext, active: bool) -> Self {
            let renders = Rc::new(Cell::new(0));
            let clicks = Rc::new(Cell::new(0));
            let actions = Rc::new(Cell::new(0));
            let window = cx.open_window(size(px(200.), px(200.)), |_, cx| {
                let panel = cx.new(|cx| Panel {
                    focus: cx.focus_handle(),
                    renders: renders.clone(),
                    clicks: clicks.clone(),
                    actions: actions.clone(),
                });
                Shell { panel, leading: 0 }
            });
            cx.run_until_parked();
            let panel = window
                .update(cx, |shell, _, _| shell.panel.clone())
                .unwrap();
            let focus = panel.read_with(cx, |panel, _| panel.focus.clone());
            if active {
                activate_a11y(window.into(), cx);
            }
            Self {
                window,
                panel,
                focus,
                renders,
                clicks,
                actions,
            }
        }

        fn tree(&self, cx: &mut TestAppContext) -> accesskit::TreeUpdate {
            a11y_tree(self.window.into(), cx)
        }

        /// How many labels the shell has drawn ahead of the panel.
        fn leading(&self, cx: &mut TestAppContext) -> usize {
            self.window.update(cx, |shell, _, _| shell.leading).unwrap()
        }

        /// Draws the shell again, which reuses the panel.
        fn notify_shell(&self, cx: &mut TestAppContext) {
            self.window
                .update(cx, |shell, _, cx| {
                    shell.leading += 1;
                    cx.notify();
                })
                .unwrap();
            cx.run_until_parked();
        }

        /// Sends the screen reader's request for `action` on the node
        /// labelled `label`.
        fn request(&self, cx: &mut TestAppContext, label: &str, action: accesskit::Action) {
            let tree = self.tree(cx);
            let (target_node, _) = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .unwrap_or_else(|| panic!("a node labelled {label}"));
            let request = accesskit::ActionRequest {
                action,
                target_tree: accesskit::TreeId::ROOT,
                target_node: *target_node,
                data: None,
            };
            self.window
                .update(cx, |_, window, cx| window.handle_a11y_action(request, cx))
                .unwrap();
            cx.run_until_parked();
        }
    }

    #[gpui::test]
    fn a_reused_cached_view_keeps_its_accessibility_nodes(cx: &mut TestAppContext) {
        let fixture = PanelFixture::open(cx, true);
        let expected = a11y_shape(&fixture.tree(cx));
        // The window, the shell, the panel, its three items and one label.
        assert_eq!(expected.len(), 7, "{expected:#?}");
        let renders = fixture.renders.get();

        for _ in 0..3 {
            fixture.notify_shell(cx);
            assert_eq!(fixture.renders.get(), renders, "the panel is reused");
            let shape = a11y_shape(&fixture.tree(cx));
            // The shell gains one more label ahead of the panel each time.
            assert_eq!(shape.len(), expected.len() + fixture.leading(cx));
            let panel_nodes = |shape: &[A11yShape]| {
                shape
                    .iter()
                    .skip_while(|node| node.label.as_deref() != Some("panel"))
                    .map(|node| (node.depth, node.role, node.label.clone()))
                    .collect::<Vec<_>>()
            };
            assert_eq!(panel_nodes(&shape), panel_nodes(&expected));
        }

        // The panel redrawn on its own while the shell is reused.
        fixture.panel.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        assert_eq!(fixture.renders.get(), renders + 1);
        fixture.notify_shell(cx);
        assert_eq!(fixture.renders.get(), renders + 1);
        assert_eq!(
            a11y_shape(&fixture.tree(cx)).len(),
            expected.len() + fixture.leading(cx)
        );
    }

    /// A fixture whose panel was reused by the last frame.
    fn fixture_with_reused_panel(cx: &mut TestAppContext) -> PanelFixture {
        let fixture = PanelFixture::open(cx, true);
        let renders = fixture.renders.get();
        fixture.notify_shell(cx);
        fixture.notify_shell(cx);
        assert_eq!(fixture.renders.get(), renders, "the panel is reused");
        fixture
    }

    #[gpui::test]
    fn an_action_handler_of_a_reused_view_still_runs(cx: &mut TestAppContext) {
        let fixture = fixture_with_reused_panel(cx);
        fixture.request(cx, "action", accesskit::Action::Click);
        assert_eq!(fixture.actions.get(), 1);
    }

    #[gpui::test]
    fn a_click_action_presses_a_node_of_a_reused_view(cx: &mut TestAppContext) {
        let fixture = fixture_with_reused_panel(cx);
        // Without a handler, the click goes where the node is.
        fixture.request(cx, "click", accesskit::Action::Click);
        assert_eq!(fixture.clicks.get(), 1);
    }

    #[gpui::test]
    fn a_focus_action_focuses_a_node_of_a_reused_view(cx: &mut TestAppContext) {
        let fixture = fixture_with_reused_panel(cx);
        fixture.request(cx, "focus", accesskit::Action::Focus);
        let focused = fixture
            .window
            .update(cx, |_, window, _| fixture.focus.is_focused(window))
            .unwrap();
        assert!(focused);
    }

    #[gpui::test]
    fn the_focused_node_of_a_reused_view_stays_focused(cx: &mut TestAppContext) {
        let fixture = PanelFixture::open(cx, true);
        fixture
            .window
            .update(cx, |_, window, cx| window.focus(&fixture.focus, cx))
            .unwrap();
        cx.run_until_parked();
        let focused_label = |fixture: &PanelFixture, cx: &mut TestAppContext| {
            let tree = fixture.tree(cx);
            tree.nodes
                .iter()
                .find(|(id, _)| *id == tree.focus)
                .and_then(|(_, node)| node.label().map(str::to_string))
        };
        assert_eq!(focused_label(&fixture, cx).as_deref(), Some("focus"));

        let renders = fixture.renders.get();
        fixture.notify_shell(cx);
        fixture.notify_shell(cx);
        assert_eq!(fixture.renders.get(), renders, "the panel is reused");
        assert_eq!(focused_label(&fixture, cx).as_deref(), Some("focus"));
    }

    #[gpui::test]
    fn a_view_drawn_before_accessibility_started_is_drawn_again(cx: &mut TestAppContext) {
        let fixture = PanelFixture::open(cx, false);
        let renders = fixture.renders.get();

        // Accessibility starts without a forced redraw, so the panel has no
        // nodes to reuse and has to draw them.
        fixture
            .window
            .update(cx, |_, window, _| window.a11y.activate_for_test())
            .unwrap();
        fixture.notify_shell(cx);
        assert_eq!(fixture.renders.get(), renders + 1);
        let shape = a11y_shape(&fixture.tree(cx));
        assert!(
            shape
                .iter()
                .any(|node| node.label.as_deref() == Some("panel"))
        );
        assert_eq!(shape.len(), 8);

        // From then on it's reused.
        fixture.notify_shell(cx);
        assert_eq!(fixture.renders.get(), renders + 1);
        assert_eq!(a11y_shape(&fixture.tree(cx)).len(), 9);
    }

    /// A view in a randomly built tree of cached views.
    struct Node {
        color: Hsla,
        popover: bool,
        children: Vec<Child>,
    }

    struct Child {
        node: Entity<Node>,
        height: f32,
        hidden: bool,
    }

    impl Render for Node {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("node")
                .role(Role::Group)
                .aria_label(format!("{:.4}", self.color.h))
                .flex()
                .flex_col()
                .size_full()
                .overflow_hidden()
                .bg(self.color)
                .on_click(|_, _, _| {})
                .when(self.popover, |this| {
                    this.child(deferred(
                        div()
                            .id("popover")
                            .role(Role::Dialog)
                            .aria_label("popover")
                            .size(px(6.))
                            .bg(self.color.opacity(0.5))
                            .on_click(|_, _, _| {}),
                    ))
                })
                .children(self.children.iter().map(|child| {
                    div()
                        .w_full()
                        .flex_none()
                        .h(px(child.height))
                        .when(child.hidden, |this| this.invisible())
                        .child(
                            child
                                .node
                                .clone()
                                .cached(StyleRefinement::default().size_full()),
                        )
                }))
        }
    }

    struct Tree {
        node: Entity<Node>,
    }

    impl Render for Tree {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                self.node
                    .clone()
                    .cached(StyleRefinement::default().size_full()),
            )
        }
    }

    /// A node and its children, to draw the same tree from scratch.
    struct NodeSnapshot {
        color: Hsla,
        popover: bool,
        children: Vec<(NodeSnapshot, f32, bool)>,
    }

    impl NodeSnapshot {
        fn of(node: &Entity<Node>, cx: &App) -> Self {
            let node = node.read(cx);
            Self {
                color: node.color,
                popover: node.popover,
                children: node
                    .children
                    .iter()
                    .map(|child| (Self::of(&child.node, cx), child.height, child.hidden))
                    .collect(),
            }
        }

        fn build(&self, cx: &mut App) -> Entity<Node> {
            let children = self
                .children
                .iter()
                .map(|(node, height, hidden)| Child {
                    node: node.build(cx),
                    height: *height,
                    hidden: *hidden,
                })
                .collect();
            cx.new(|_| Node {
                color: self.color,
                popover: self.popover,
                children,
            })
        }
    }

    /// A node of the accessibility tree without its id, which differs from one
    /// window to the next.
    #[derive(Debug, PartialEq)]
    struct A11yShape {
        depth: usize,
        role: Role,
        label: Option<String>,
        bounds: Option<accesskit::Rect>,
    }

    /// The nodes of a tree update below the root, in tree order.
    fn a11y_shape(update: &accesskit::TreeUpdate) -> Vec<A11yShape> {
        fn visit(
            update: &accesskit::TreeUpdate,
            id: accesskit::NodeId,
            depth: usize,
            shape: &mut Vec<A11yShape>,
        ) {
            let (_, node) = update
                .nodes
                .iter()
                .find(|(node_id, _)| *node_id == id)
                .expect("a child is in the update");
            shape.push(A11yShape {
                depth,
                role: node.role(),
                label: node.label().map(str::to_string),
                bounds: node.bounds(),
            });
            for child in node.children() {
                visit(update, *child, depth + 1, shape);
            }
        }
        let root = update.tree.as_ref().expect("a full tree").root;
        let mut shape = Vec::new();
        visit(update, root, 0, &mut shape);
        shape
    }

    /// Reports accessibility as active, as a screen reader does, and draws the
    /// window again.
    fn activate_a11y(window: AnyWindowHandle, cx: &mut TestAppContext) {
        cx.update_window(window, |_, window, _| {
            window.a11y.activate_for_test();
            window.refresh();
        })
        .unwrap();
        cx.run_until_parked();
    }

    /// The accessibility tree that the last frame sent to the platform.
    fn a11y_tree(window: AnyWindowHandle, cx: &mut TestAppContext) -> accesskit::TreeUpdate {
        cx.update_window(window, |_, window, _| {
            window.a11y.last_tree_for_test().cloned()
        })
        .unwrap()
        .expect("accessibility is active")
    }

    #[derive(Debug, PartialEq)]
    struct DrawnFrame {
        quads: Vec<(
            u32,
            Bounds<ScaledPixels>,
            ContentMask<ScaledPixels>,
            Background,
        )>,
        hitboxes: Vec<Bounds<Pixels>>,
        a11y: Vec<A11yShape>,
    }

    fn drawn_frame(window: AnyWindowHandle, cx: &mut TestAppContext) -> DrawnFrame {
        cx.update_window(window, |_, window, _| {
            let frame = &window.rendered_frame;
            DrawnFrame {
                quads: frame
                    .scene
                    .quads
                    .iter()
                    .map(|quad| (quad.order, quad.bounds, quad.content_mask, quad.background))
                    .collect(),
                hitboxes: frame.hitboxes.iter().map(|hitbox| hitbox.bounds).collect(),
                a11y: window
                    .a11y
                    .last_tree_for_test()
                    .map(a11y_shape)
                    .unwrap_or_default(),
            }
        })
        .unwrap()
    }

    fn random_node(
        rng: &mut StdRng,
        depth: usize,
        nodes: &mut Vec<Entity<Node>>,
        cx: &mut App,
    ) -> Entity<Node> {
        let child_count = if depth == 0 {
            0
        } else {
            rng.random_range(0..=3)
        };
        let children = (0..child_count)
            .map(|_| Child {
                node: random_node(rng, depth - 1, nodes, cx),
                height: rng.random_range(10..=120) as f32,
                hidden: false,
            })
            .collect();
        let color = hsla(rng.random(), 0.5, 0.5, 1.);
        let popover = rng.random_bool(0.2);
        let node = cx.new(|_| Node {
            color,
            popover,
            children,
        });
        nodes.push(node.clone());
        node
    }

    /// Changes random parts of a tree of cached views, frame after frame, and
    /// compares each frame with the same tree drawn from scratch.
    #[gpui::test]
    fn nested_views_draw_what_a_fresh_window_draws(cx: &mut TestAppContext) {
        const WINDOW_SIZE: f32 = 400.;
        for seed in 0..24 {
            let mut rng = StdRng::seed_from_u64(seed);
            let mut nodes = Vec::new();
            let root = cx.update(|cx| random_node(&mut rng, 3, &mut nodes, cx));
            let window = cx.open_window(size(px(WINDOW_SIZE), px(WINDOW_SIZE)), |_, _| Tree {
                node: root.clone(),
            });
            cx.run_until_parked();
            activate_a11y(window.into(), cx);

            for step in 0..40 {
                let changes = rng.random_range(1..=2);
                for _ in 0..changes {
                    let node = nodes[rng.random_range(0..nodes.len())].clone();
                    match rng.random_range(0..7) {
                        0 => node.update(cx, |node, cx| {
                            node.color = hsla(rng.random(), 0.5, 0.5, 1.);
                            cx.notify();
                        }),
                        1 => node.update(cx, |_, cx| cx.notify()),
                        2 => node.update(cx, |node, cx| {
                            node.popover = !node.popover;
                            cx.notify();
                        }),
                        3 => node.update(cx, |node, cx| {
                            if !node.children.is_empty() {
                                let ix = rng.random_range(0..node.children.len());
                                node.children[ix].height = rng.random_range(10..=120) as f32;
                                cx.notify();
                            }
                        }),
                        4 => node.update(cx, |node, cx| {
                            if !node.children.is_empty() {
                                let ix = rng.random_range(0..node.children.len());
                                node.children[ix].hidden = !node.children[ix].hidden;
                                cx.notify();
                            }
                        }),
                        5 => window.update(cx, |_, _, cx| cx.notify()).unwrap(),
                        _ => window.update(cx, |_, window, _| window.refresh()).unwrap(),
                    }
                }
                cx.run_until_parked();
                let drawn = drawn_frame(window.into(), cx);

                let snapshot = cx.read(|cx| NodeSnapshot::of(&root, cx));
                let fresh = cx.open_window(size(px(WINDOW_SIZE), px(WINDOW_SIZE)), |_, cx| Tree {
                    node: snapshot.build(cx),
                });
                cx.run_until_parked();
                activate_a11y(fresh.into(), cx);
                let expected = drawn_frame(fresh.into(), cx);
                fresh
                    .update(cx, |_, window, _| window.remove_window())
                    .unwrap();
                assert_eq!(drawn, expected, "seed {seed}, step {step}");
            }
            window
                .update(cx, |_, window, _| window.remove_window())
                .unwrap();
            cx.run_until_parked();
        }
    }

    /// A row of text that wraps to the width it's laid out at.
    struct TextRow {
        words: usize,
        color: Hsla,
        /// Words added by something outside the row's state, which doesn't notify it.
        extra_words: Rc<Cell<usize>>,
        renders: Rc<Cell<usize>>,
    }

    impl Render for TextRow {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            div()
                .w_full()
                .p_1()
                .bg(self.color)
                .child("word ".repeat(self.words + self.extra_words.get()))
        }
    }

    /// How a list draws its rows.
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum RowCaching {
        /// The rows are elements of the view that has the list.
        Plain,
        /// Each row is its own view, cached at a height it measures.
        Measured,
        /// Each row is its own view in a box of the list, which isn't the item itself.
        MeasuredInABox,
    }

    struct RowList {
        state: ListState,
        rows: Vec<Entity<TextRow>>,
        caching: RowCaching,
        text_size: Pixels,
        /// The width of the box the list is in, which isn't the window's.
        width: Pixels,
        extra_words: Rc<Cell<usize>>,
        renders: Rc<Cell<usize>>,
    }

    impl Render for RowList {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            let rows = self.rows.clone();
            let caching = self.caching;
            let extra_words = self.extra_words.clone();
            // Only a plain list builds its rows from the rows' state, and so redraws when they change.
            let row_contents: Vec<(usize, Hsla)> = if caching == RowCaching::Plain {
                rows.iter()
                    .map(|row| {
                        let row = row.read(cx);
                        (row.words, row.color)
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let list = list(self.state.clone(), move |ix, _, _| match caching {
                RowCaching::Plain => {
                    let (words, color) = row_contents[ix];
                    div()
                        .w_full()
                        .p_1()
                        .bg(color)
                        .child("word ".repeat(words + extra_words.get()))
                        .into_any_element()
                }
                RowCaching::Measured => rows[ix]
                    .clone()
                    .cached_measured(StyleRefinement::default().w_full())
                    .into_any_element(),
                RowCaching::MeasuredInABox => div()
                    .w_full()
                    .child(
                        rows[ix]
                            .clone()
                            .cached_measured(StyleRefinement::default().w_full()),
                    )
                    .into_any_element(),
            })
            .size_full();
            div()
                .h_full()
                .w(self.width)
                .text_size(self.text_size)
                .child(list)
        }
    }

    const ROW_COUNT: usize = 24;
    const LIST_WIDTH: f32 = 200.;
    const LIST_HEIGHT: f32 = 240.;

    struct ListFixture {
        window: WindowHandle<RowList>,
        state: ListState,
        rows: Vec<Entity<TextRow>>,
        row_renders: Rc<Cell<usize>>,
        list_renders: Rc<Cell<usize>>,
        extra_words: Rc<Cell<usize>>,
    }

    impl ListFixture {
        fn open(cx: &mut TestAppContext, caching: RowCaching, seed: u64) -> Self {
            let mut rng = StdRng::seed_from_u64(seed);
            let row_renders = Rc::new(Cell::new(0));
            let list_renders = Rc::new(Cell::new(0));
            let extra_words = Rc::new(Cell::new(0));
            let state = ListState::new(ROW_COUNT, ListAlignment::Top, px(60.));
            let window = cx.open_window(size(px(LIST_WIDTH), px(LIST_HEIGHT)), |_, cx| {
                let rows = (0..ROW_COUNT)
                    .map(|_| {
                        cx.new(|_| TextRow {
                            words: rng.random_range(1..=12),
                            color: hsla(rng.random(), 0.5, 0.5, 1.),
                            extra_words: extra_words.clone(),
                            renders: row_renders.clone(),
                        })
                    })
                    .collect();
                RowList {
                    state: state.clone(),
                    rows,
                    caching,
                    text_size: px(16.),
                    width: px(LIST_WIDTH),
                    extra_words: extra_words.clone(),
                    renders: list_renders.clone(),
                }
            });
            cx.run_until_parked();
            let rows = window.update(cx, |list, _, _| list.rows.clone()).unwrap();
            Self {
                window,
                state,
                rows,
                row_renders,
                list_renders,
                extra_words,
            }
        }

        fn set_words(&self, cx: &mut TestAppContext, ix: usize, words: usize) {
            self.rows[ix].update(cx, |row, cx| {
                row.words = words;
                cx.notify();
            });
            cx.run_until_parked();
        }

        /// Changes the width of the list without changing the window's.
        fn set_width(&self, cx: &mut TestAppContext, width: Pixels) {
            self.window
                .update(cx, |list, _, cx| {
                    list.width = width;
                    cx.notify();
                })
                .unwrap();
            cx.run_until_parked();
        }

        fn set_text_size(&self, cx: &mut TestAppContext, text_size: Pixels) {
            self.window
                .update(cx, |list, _, cx| {
                    list.text_size = text_size;
                    cx.notify();
                })
                .unwrap();
            cx.run_until_parked();
        }

        fn notify_list(&self, cx: &mut TestAppContext) {
            self.window.update(cx, |_, _, cx| cx.notify()).unwrap();
            cx.run_until_parked();
        }

        /// What the list drew, and how it's scrolled.
        fn drawn(&self, cx: &mut TestAppContext) -> (DrawnFrame, usize, Pixels, Pixels) {
            let drawn = drawn_frame(self.window.into(), cx);
            let top = self.state.logical_scroll_top();
            (
                drawn,
                top.item_ix,
                top.offset_in_item,
                self.state.max_offset_for_scrollbar().y,
            )
        }
    }

    #[gpui::test]
    fn the_rows_of_a_list_that_are_clean_are_not_rendered_again(cx: &mut TestAppContext) {
        let fixture = ListFixture::open(cx, RowCaching::Measured, 1);
        let rendered = fixture.row_renders.get();
        assert!(rendered > 0);

        // The list redraws, with the same rows.
        fixture.notify_list(cx);
        fixture.notify_list(cx);
        assert_eq!(fixture.row_renders.get(), rendered);
        assert_eq!(fixture.list_renders.get(), 3);

        // One row changes, and only it renders. Rows below it move with its height.
        let plain = ListFixture::open(cx, RowCaching::Plain, 1);
        let words_before = fixture.rows[1].read_with(cx, |row, _| row.words);
        let words = words_before + 20;
        fixture.set_words(cx, 1, words);
        plain.set_words(cx, 1, words);
        assert_eq!(fixture.row_renders.get(), rendered + 1);
        assert_eq!(fixture.drawn(cx), plain.drawn(cx));
    }

    #[gpui::test]
    fn a_row_in_a_box_is_rendered_with_every_frame_of_the_list(cx: &mut TestAppContext) {
        let fixture = ListFixture::open(cx, RowCaching::MeasuredInABox, 2);
        let plain = ListFixture::open(cx, RowCaching::Plain, 2);
        let rendered = fixture.row_renders.get();
        fixture.notify_list(cx);
        plain.notify_list(cx);
        assert!(fixture.row_renders.get() > rendered);
        assert_eq!(fixture.drawn(cx), plain.drawn(cx));
        fixture.set_words(cx, 0, 30);
        plain.set_words(cx, 0, 30);
        assert_eq!(fixture.drawn(cx), plain.drawn(cx));
    }

    #[gpui::test]
    fn rows_measured_at_one_width_are_measured_again_at_another(cx: &mut TestAppContext) {
        let fixture = ListFixture::open(cx, RowCaching::Measured, 3);
        let plain = ListFixture::open(cx, RowCaching::Plain, 3);
        for width in [150., 240., 200., 90.] {
            for fixture in [&fixture, &plain] {
                cx.simulate_window_resize(fixture.window.into(), size(px(width), px(LIST_HEIGHT)));
                cx.run_until_parked();
            }
            assert_eq!(fixture.drawn(cx), plain.drawn(cx), "{width}");
        }
    }

    #[gpui::test]
    fn rows_measured_at_one_width_of_the_list_are_measured_again_at_another(
        cx: &mut TestAppContext,
    ) {
        let fixture = ListFixture::open(cx, RowCaching::Measured, 5);
        let plain = ListFixture::open(cx, RowCaching::Plain, 5);
        for width in [px(120.), px(200.), px(80.), px(160.)] {
            fixture.set_width(cx, width);
            plain.set_width(cx, width);
            assert_eq!(fixture.drawn(cx), plain.drawn(cx), "{width:?}");
        }
    }

    #[gpui::test]
    fn a_cached_row_that_grows_at_the_end_of_a_list_is_followed(cx: &mut TestAppContext) {
        let fixture = ListFixture::open(cx, RowCaching::Measured, 7);
        let plain = ListFixture::open(cx, RowCaching::Plain, 7);
        for fixture in [&fixture, &plain] {
            fixture.state.set_follow_mode(FollowMode::Tail);
            fixture.notify_list(cx);
        }
        assert_eq!(fixture.drawn(cx), plain.drawn(cx));
        for words in (1..40).step_by(3) {
            fixture.set_words(cx, ROW_COUNT - 1, words);
            plain.set_words(cx, ROW_COUNT - 1, words);
            assert_eq!(fixture.drawn(cx), plain.drawn(cx), "{words}");
            assert_eq!(
                fixture.state.is_scrolled_to_end(),
                plain.state.is_scrolled_to_end()
            );
            assert!(fixture.state.is_following_tail());
        }
    }

    #[gpui::test]
    fn a_refresh_measures_the_rows_again(cx: &mut TestAppContext) {
        let fixture = ListFixture::open(cx, RowCaching::Measured, 6);
        let plain = ListFixture::open(cx, RowCaching::Plain, 6);
        // The rows change without being notified, as when they read something a notification
        // doesn't reach, and only a refresh draws them again.
        for extra_words in [9, 0, 4] {
            for fixture in [&fixture, &plain] {
                fixture.extra_words.set(extra_words);
                fixture
                    .window
                    .update(cx, |_, window, _| window.refresh())
                    .unwrap();
                cx.run_until_parked();
            }
            assert_eq!(fixture.drawn(cx), plain.drawn(cx), "{extra_words}");
        }
    }

    #[gpui::test]
    fn rows_measured_in_one_text_style_are_measured_again_in_another(cx: &mut TestAppContext) {
        let fixture = ListFixture::open(cx, RowCaching::Measured, 4);
        let plain = ListFixture::open(cx, RowCaching::Plain, 4);
        for text_size in [px(24.), px(12.), px(16.)] {
            fixture.set_text_size(cx, text_size);
            plain.set_text_size(cx, text_size);
            assert_eq!(fixture.drawn(cx), plain.drawn(cx), "{text_size:?}");
        }
    }

    #[gpui::test]
    fn a_list_of_cached_rows_draws_what_a_plain_list_draws(cx: &mut TestAppContext) {
        for seed in 0..16 {
            let mut rng = StdRng::seed_from_u64(seed);
            let fixture = ListFixture::open(cx, RowCaching::Measured, seed);
            let plain = ListFixture::open(cx, RowCaching::Plain, seed);
            for step in 0..60 {
                let changes = rng.random_range(1..=2);
                for _ in 0..changes {
                    match rng.random_range(0..9) {
                        8 => {
                            let width = px(rng.random_range(80..=260) as f32);
                            fixture.set_width(cx, width);
                            plain.set_width(cx, width);
                        }
                        7 => {
                            let text_size = px(rng.random_range(10..=24) as f32);
                            fixture.set_text_size(cx, text_size);
                            plain.set_text_size(cx, text_size);
                        }
                        0..=2 => {
                            let ix = rng.random_range(0..ROW_COUNT);
                            let words = rng.random_range(1..=30);
                            fixture.set_words(cx, ix, words);
                            plain.set_words(cx, ix, words);
                        }
                        3 => {
                            fixture.notify_list(cx);
                            plain.notify_list(cx);
                        }
                        4 | 5 => {
                            let distance = px(rng.random_range(-120..=120) as f32);
                            fixture.state.scroll_by(distance);
                            plain.state.scroll_by(distance);
                            fixture.notify_list(cx);
                            plain.notify_list(cx);
                        }
                        _ => {
                            let width = rng.random_range(90..=260) as f32;
                            for fixture in [&fixture, &plain] {
                                cx.simulate_window_resize(
                                    fixture.window.into(),
                                    size(px(width), px(LIST_HEIGHT)),
                                );
                                cx.run_until_parked();
                            }
                        }
                    }
                }
                assert_eq!(
                    fixture.drawn(cx),
                    plain.drawn(cx),
                    "seed {seed}, step {step}"
                );
            }
            for fixture in [fixture, plain] {
                fixture
                    .window
                    .update(cx, |_, window, _| window.remove_window())
                    .unwrap();
            }
            cx.run_until_parked();
        }
    }
}
