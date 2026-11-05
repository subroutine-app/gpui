// Modified by Mesa Hills Research, as docs/upstream.md in mesa-hills-research/gpui describes.
// todo("windows"): remove
#![cfg_attr(windows, allow(dead_code))]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AtlasTextureId, AtlasTile, Background, Bounds, ContentMask, Corners, Edges, Hsla, Pixels,
    Point, Radians, ScaledPixels, Size, bounds_tree::BoundsTree, point,
};
use std::{
    fmt::Debug,
    iter::Peekable,
    ops::{Add, Range, Sub},
    slice,
};

#[allow(non_camel_case_types, unused)]
#[expect(missing_docs)]
pub type PathVertex_ScaledPixels = PathVertex<ScaledPixels>;

#[expect(missing_docs)]
pub type DrawOrder = u32;

/// A boolean stored as a `u32` so that GPU-facing structs contain no
/// compiler-inserted padding bytes, which would be undefined behavior to
/// reinterpret as `&[u8]` when writing instance buffers. Guaranteed to be
/// `0` or `1` by construction; shaders read it as a `u32`/`uint`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct PaddedBool32(u32);

impl From<bool> for PaddedBool32 {
    fn from(value: bool) -> Self {
        PaddedBool32(value as u32)
    }
}

#[derive(Default)]
#[expect(missing_docs)]
pub struct Scene {
    pub(crate) paint_operations: Vec<PaintOperation>,
    primitive_bounds: BoundsTree<ScaledPixels>,
    layer_stack: Vec<DrawOrder>,
    effect_stack: Vec<ActiveEffect>,
    pub shadows: Vec<Shadow>,
    pub quads: Vec<Quad>,
    pub paths: Vec<Path<ScaledPixels>>,
    pub underlines: Vec<Underline>,
    pub monochrome_sprites: Vec<MonochromeSprite>,
    pub subpixel_sprites: Vec<SubpixelSprite>,
    pub polychrome_sprites: Vec<PolychromeSprite>,
    pub surfaces: Vec<PaintSurface>,
}

#[expect(missing_docs)]
impl Scene {
    pub fn clear(&mut self) {
        self.paint_operations.clear();
        self.primitive_bounds.clear();
        self.layer_stack.clear();
        self.effect_stack.clear();
        self.paths.clear();
        self.shadows.clear();
        self.quads.clear();
        self.underlines.clear();
        self.monochrome_sprites.clear();
        self.subpixel_sprites.clear();
        self.polychrome_sprites.clear();
        self.surfaces.clear();
    }

    pub fn len(&self) -> usize {
        self.paint_operations.len()
    }

    pub fn push_layer(&mut self, bounds: Bounds<ScaledPixels>) {
        let order = self.primitive_bounds.insert(bounds);
        self.layer_stack.push(order);
        self.paint_operations
            .push(PaintOperation::StartLayer(bounds));
    }

    pub fn pop_layer(&mut self) {
        self.layer_stack.pop();
        self.paint_operations.push(PaintOperation::EndLayer);
    }

    pub fn insert_primitive(&mut self, primitive: impl Into<Primitive>) {
        let mut primitive = primitive.into();
        // Where it is drawn: an effect moves it, and the clip it was painted under stays where it
        // was.
        let drawn_bounds = self
            .effect_stack
            .last()
            .map_or(*primitive.bounds(), |effect| {
                shifted(*primitive.bounds(), effect.translation)
            });
        let clipped_bounds = drawn_bounds.intersect(&primitive.content_mask().bounds);

        if clipped_bounds.is_empty() {
            // Out of view where an effect puts it now, and perhaps in view where a later frame
            // puts it, so what is kept to replay includes it.
            if !self.effect_stack.is_empty() {
                self.paint_operations
                    .push(PaintOperation::Primitive(primitive));
            }
            return;
        }

        let order = self
            .layer_stack
            .last()
            .copied()
            .unwrap_or_else(|| self.primitive_bounds.insert(clipped_bounds));
        match &mut primitive {
            Primitive::Shadow(shadow) => shadow.order = order,
            Primitive::Quad(quad) => quad.order = order,
            Primitive::Path(path) => {
                path.order = order;
                path.id = PathId(self.paths.len());
            }
            Primitive::Underline(underline) => underline.order = order,
            Primitive::MonochromeSprite(sprite) => sprite.order = order,
            Primitive::SubpixelSprite(sprite) => sprite.order = order,
            Primitive::PolychromeSprite(sprite) => sprite.order = order,
            Primitive::Surface(surface) => surface.order = order,
        }

        // Inside a paint effect, what is drawn is the primitive with the effect applied, and
        // what is kept to replay is the primitive without it.
        let effect_applied = self
            .effect_stack
            .last()
            .map(|effect| effect.apply(&primitive));
        match effect_applied.as_ref().unwrap_or(&primitive) {
            Primitive::Shadow(shadow) => self.shadows.push(*shadow),
            Primitive::Quad(quad) => self.quads.push(*quad),
            Primitive::Path(path) => self.paths.push(path.clone()),
            Primitive::Underline(underline) => self.underlines.push(*underline),
            Primitive::MonochromeSprite(sprite) => self.monochrome_sprites.push(*sprite),
            Primitive::SubpixelSprite(sprite) => self.subpixel_sprites.push(*sprite),
            Primitive::PolychromeSprite(sprite) => self.polychrome_sprites.push(*sprite),
            Primitive::Surface(surface) => self.surfaces.push(surface.clone()),
        }
        self.paint_operations
            .push(PaintOperation::Primitive(primitive));
    }

    /// Applies `effect` to what is inserted until the matching [`Self::pop_effect`]. Rotation
    /// turns sprites about `center`, and then the translation moves what is painted. `scale_factor`
    /// takes the translation to scaled pixels.
    pub fn push_effect(
        &mut self,
        effect: PaintEffect,
        center: Point<ScaledPixels>,
        scale_factor: f32,
    ) {
        let outer = self.effect_stack.last();
        let opacity = outer.map_or(1., |outer| outer.opacity) * effect.opacity;
        let offset = effect.translation.scale(scale_factor);
        let translation = outer.map_or(Point::default(), |outer| outer.translation) + offset;
        // The sprites that have a transformation move with it, so the translation is in the
        // transformation of the effect, which turns them first and moves them after.
        let moves = offset != Point::default();
        let turns = effect.rotation.0 != 0.;
        let own = (moves || turns).then(|| {
            let moved = TransformationMatrix::unit().translate(offset);
            if turns {
                moved
                    .translate(center)
                    .rotate(effect.rotation)
                    .translate(center.map(|coordinate| ScaledPixels(-coordinate.0)))
            } else {
                moved
            }
        });
        let transformation = match (outer.and_then(|outer| outer.transformation), own) {
            (Some(outer), Some(own)) => Some(outer.compose(own)),
            (outer, own) => outer.or(own),
        };
        self.effect_stack.push(ActiveEffect {
            opacity,
            translation,
            transformation,
        });
    }

    pub fn pop_effect(&mut self) {
        self.effect_stack.pop();
    }

    pub fn replay(&mut self, range: Range<usize>, prev_scene: &Scene) {
        for operation in &prev_scene.paint_operations[range] {
            match operation {
                PaintOperation::Primitive(primitive) => self.insert_primitive(primitive.clone()),
                PaintOperation::StartLayer(bounds) => self.push_layer(*bounds),
                PaintOperation::EndLayer => self.pop_layer(),
            }
        }
    }

    pub fn finish(&mut self) {
        self.shadows.sort_by_key(|shadow| shadow.order);
        self.quads.sort_by_key(|quad| quad.order);
        self.paths.sort_by_key(|path| path.order);
        self.underlines.sort_by_key(|underline| underline.order);
        self.monochrome_sprites
            .sort_by_key(|sprite| (sprite.order, sprite.tile.texture_id.index));
        self.subpixel_sprites
            .sort_by_key(|sprite| (sprite.order, sprite.tile.texture_id.index));
        self.polychrome_sprites
            .sort_by_key(|sprite| (sprite.order, sprite.tile.texture_id.index));
        self.surfaces.sort_by_key(|surface| surface.order);
    }

    #[cfg_attr(
        all(
            any(target_os = "linux", target_os = "freebsd"),
            not(any(feature = "x11", feature = "wayland"))
        ),
        allow(dead_code)
    )]
    pub fn batches(&self) -> impl Iterator<Item = PrimitiveBatch> + '_ {
        BatchIterator {
            shadows_start: 0,
            shadows_iter: self.shadows.iter().peekable(),
            quads_start: 0,
            quads_iter: self.quads.iter().peekable(),
            paths_start: 0,
            paths_iter: self.paths.iter().peekable(),
            underlines_start: 0,
            underlines_iter: self.underlines.iter().peekable(),
            monochrome_sprites_start: 0,
            monochrome_sprites_iter: self.monochrome_sprites.iter().peekable(),
            subpixel_sprites_start: 0,
            subpixel_sprites_iter: self.subpixel_sprites.iter().peekable(),
            polychrome_sprites_start: 0,
            polychrome_sprites_iter: self.polychrome_sprites.iter().peekable(),
            surfaces_start: 0,
            surfaces_iter: self.surfaces.iter().peekable(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Default)]
#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
pub(crate) enum PrimitiveKind {
    Shadow,
    #[default]
    Quad,
    Path,
    Underline,
    MonochromeSprite,
    SubpixelSprite,
    PolychromeSprite,
    Surface,
}

pub(crate) enum PaintOperation {
    Primitive(Primitive),
    StartLayer(Bounds<ScaledPixels>),
    EndLayer,
}

/// A change to how an element paints that an animation can make without laying the element out
/// again, so that it can be replayed when the view the element is in is reused.
///
/// An effect changes what is painted and nothing else. Hit testing, focus, the bounds an element
/// reports, tooltips and the place of an input method stay where the element was laid out, and an
/// effect that moves an element does not move them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaintEffect {
    /// Multiplies the opacity of everything the element paints.
    pub opacity: f32,
    /// Turns the sprites the element paints, such as the glyphs of an icon, about the element's
    /// centre. Other primitives stay where they are.
    pub rotation: Radians,
    /// Moves everything the element paints by this much, in logical pixels: quads, shadows,
    /// underlines, paths, sprites and surfaces. A rotation turns the sprites about the element's
    /// centre first, and the translation moves them with the rest.
    ///
    /// The clip an element paints under stays where it is, as the clip of a parent does around a
    /// child that moves in it, so an element that moves out of its clip is cut by the clip. That
    /// goes for a clip inside the element too, which doesn't move with what is clipped by it.
    pub translation: Point<Pixels>,
}

impl PaintEffect {
    /// An effect that changes nothing.
    pub const NONE: PaintEffect = PaintEffect {
        opacity: 1.,
        rotation: Radians(0.),
        translation: Point {
            x: Pixels(0.),
            y: Pixels(0.),
        },
    };

    /// An effect that multiplies the opacity of what is painted by `opacity`.
    pub fn opacity(opacity: f32) -> Self {
        Self {
            opacity,
            ..Self::NONE
        }
    }

    /// An effect that turns the sprites that are painted by `rotation`.
    pub fn rotation(rotation: Radians) -> Self {
        Self {
            rotation,
            ..Self::NONE
        }
    }

    /// An effect that moves what is painted by `translation`.
    pub fn translation(translation: Point<Pixels>) -> Self {
        Self {
            translation,
            ..Self::NONE
        }
    }

    /// This effect with the opacity set to `opacity`.
    pub fn with_opacity(self, opacity: f32) -> Self {
        Self { opacity, ..self }
    }

    /// This effect with the rotation set to `rotation`.
    pub fn with_rotation(self, rotation: Radians) -> Self {
        Self { rotation, ..self }
    }

    /// This effect with the translation set to `translation`, for an element that fades as it
    /// moves.
    pub fn with_translation(self, translation: Point<Pixels>) -> Self {
        Self {
            translation,
            ..self
        }
    }
}

impl Default for PaintEffect {
    fn default() -> Self {
        Self::NONE
    }
}

/// The paint effects around what is inserted, composed.
struct ActiveEffect {
    opacity: f32,
    /// How far the effects together move what is painted, in scaled pixels.
    translation: Point<ScaledPixels>,
    /// What turns and moves the sprites that have a transformation.
    transformation: Option<TransformationMatrix>,
}

/// `bounds`, moved by `offset`.
fn shifted(bounds: Bounds<ScaledPixels>, offset: Point<ScaledPixels>) -> Bounds<ScaledPixels> {
    Bounds {
        origin: bounds.origin + offset,
        size: bounds.size,
    }
}

impl ActiveEffect {
    fn apply(&self, primitive: &Primitive) -> Primitive {
        let opacity = self.opacity;
        let offset = self.translation;
        let mut primitive = primitive.clone();
        match &mut primitive {
            Primitive::Shadow(shadow) => {
                shadow.color = shadow.color.opacity(opacity);
                shadow.bounds = shifted(shadow.bounds, offset);
                shadow.element_bounds = shifted(shadow.element_bounds, offset);
            }
            Primitive::Quad(quad) => {
                quad.background = quad.background.opacity(opacity);
                quad.border_color = quad.border_color.opacity(opacity);
                quad.bounds = shifted(quad.bounds, offset);
            }
            Primitive::Path(path) => {
                path.color = path.color.opacity(opacity);
                path.bounds = shifted(path.bounds, offset);
                for vertex in &mut path.vertices {
                    vertex.xy_position = vertex.xy_position + offset;
                }
            }
            Primitive::Underline(underline) => {
                underline.color = underline.color.opacity(opacity);
                underline.bounds = shifted(underline.bounds, offset);
            }
            Primitive::MonochromeSprite(sprite) => {
                sprite.color = sprite.color.opacity(opacity);
                if let Some(transformation) = self.transformation {
                    sprite.transformation = transformation.compose(sprite.transformation);
                }
            }
            Primitive::SubpixelSprite(sprite) => {
                sprite.color = sprite.color.opacity(opacity);
                if let Some(transformation) = self.transformation {
                    sprite.transformation = transformation.compose(sprite.transformation);
                }
            }
            Primitive::PolychromeSprite(sprite) => {
                sprite.opacity *= opacity;
                sprite.bounds = shifted(sprite.bounds, offset);
            }
            Primitive::Surface(surface) => surface.bounds = shifted(surface.bounds, offset),
        }
        primitive
    }
}

#[derive(Clone)]
#[expect(missing_docs)]
pub enum Primitive {
    Shadow(Shadow),
    Quad(Quad),
    Path(Path<ScaledPixels>),
    Underline(Underline),
    MonochromeSprite(MonochromeSprite),
    SubpixelSprite(SubpixelSprite),
    PolychromeSprite(PolychromeSprite),
    Surface(PaintSurface),
}

#[expect(missing_docs)]
impl Primitive {
    pub fn bounds(&self) -> &Bounds<ScaledPixels> {
        match self {
            Primitive::Shadow(shadow) => &shadow.bounds,
            Primitive::Quad(quad) => &quad.bounds,
            Primitive::Path(path) => &path.bounds,
            Primitive::Underline(underline) => &underline.bounds,
            Primitive::MonochromeSprite(sprite) => &sprite.bounds,
            Primitive::SubpixelSprite(sprite) => &sprite.bounds,
            Primitive::PolychromeSprite(sprite) => &sprite.bounds,
            Primitive::Surface(surface) => &surface.bounds,
        }
    }

    pub fn content_mask(&self) -> &ContentMask<ScaledPixels> {
        match self {
            Primitive::Shadow(shadow) => &shadow.content_mask,
            Primitive::Quad(quad) => &quad.content_mask,
            Primitive::Path(path) => &path.content_mask,
            Primitive::Underline(underline) => &underline.content_mask,
            Primitive::MonochromeSprite(sprite) => &sprite.content_mask,
            Primitive::SubpixelSprite(sprite) => &sprite.content_mask,
            Primitive::PolychromeSprite(sprite) => &sprite.content_mask,
            Primitive::Surface(surface) => &surface.content_mask,
        }
    }
}

#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
struct BatchIterator<'a> {
    shadows_start: usize,
    shadows_iter: Peekable<slice::Iter<'a, Shadow>>,
    quads_start: usize,
    quads_iter: Peekable<slice::Iter<'a, Quad>>,
    paths_start: usize,
    paths_iter: Peekable<slice::Iter<'a, Path<ScaledPixels>>>,
    underlines_start: usize,
    underlines_iter: Peekable<slice::Iter<'a, Underline>>,
    monochrome_sprites_start: usize,
    monochrome_sprites_iter: Peekable<slice::Iter<'a, MonochromeSprite>>,
    subpixel_sprites_start: usize,
    subpixel_sprites_iter: Peekable<slice::Iter<'a, SubpixelSprite>>,
    polychrome_sprites_start: usize,
    polychrome_sprites_iter: Peekable<slice::Iter<'a, PolychromeSprite>>,
    surfaces_start: usize,
    surfaces_iter: Peekable<slice::Iter<'a, PaintSurface>>,
}

impl<'a> Iterator for BatchIterator<'a> {
    type Item = PrimitiveBatch;

    fn next(&mut self) -> Option<Self::Item> {
        let mut orders_and_kinds = [
            (
                self.shadows_iter.peek().map(|s| s.order),
                PrimitiveKind::Shadow,
            ),
            (self.quads_iter.peek().map(|q| q.order), PrimitiveKind::Quad),
            (self.paths_iter.peek().map(|q| q.order), PrimitiveKind::Path),
            (
                self.underlines_iter.peek().map(|u| u.order),
                PrimitiveKind::Underline,
            ),
            (
                self.monochrome_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::MonochromeSprite,
            ),
            (
                self.subpixel_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::SubpixelSprite,
            ),
            (
                self.polychrome_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::PolychromeSprite,
            ),
            (
                self.surfaces_iter.peek().map(|s| s.order),
                PrimitiveKind::Surface,
            ),
        ];
        orders_and_kinds.sort_by_key(|(order, kind)| (order.unwrap_or(u32::MAX), *kind));

        let first = orders_and_kinds[0];
        let second = orders_and_kinds[1];
        let (batch_kind, max_order_and_kind) = if first.0.is_some() {
            (first.1, (second.0.unwrap_or(u32::MAX), second.1))
        } else {
            return None;
        };

        match batch_kind {
            PrimitiveKind::Shadow => {
                let shadows_start = self.shadows_start;
                let mut shadows_end = shadows_start + 1;
                self.shadows_iter.next();
                while self
                    .shadows_iter
                    .next_if(|shadow| (shadow.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    shadows_end += 1;
                }
                self.shadows_start = shadows_end;
                Some(PrimitiveBatch::Shadows(shadows_start..shadows_end))
            }
            PrimitiveKind::Quad => {
                let quads_start = self.quads_start;
                let mut quads_end = quads_start + 1;
                self.quads_iter.next();
                while self
                    .quads_iter
                    .next_if(|quad| (quad.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    quads_end += 1;
                }
                self.quads_start = quads_end;
                Some(PrimitiveBatch::Quads(quads_start..quads_end))
            }
            PrimitiveKind::Path => {
                let paths_start = self.paths_start;
                let mut paths_end = paths_start + 1;
                self.paths_iter.next();
                while self
                    .paths_iter
                    .next_if(|path| (path.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    paths_end += 1;
                }
                self.paths_start = paths_end;
                Some(PrimitiveBatch::Paths(paths_start..paths_end))
            }
            PrimitiveKind::Underline => {
                let underlines_start = self.underlines_start;
                let mut underlines_end = underlines_start + 1;
                self.underlines_iter.next();
                while self
                    .underlines_iter
                    .next_if(|underline| (underline.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    underlines_end += 1;
                }
                self.underlines_start = underlines_end;
                Some(PrimitiveBatch::Underlines(underlines_start..underlines_end))
            }
            PrimitiveKind::MonochromeSprite => {
                let texture_id = self.monochrome_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.monochrome_sprites_start;
                let mut sprites_end = sprites_start + 1;
                self.monochrome_sprites_iter.next();
                while self
                    .monochrome_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.monochrome_sprites_start = sprites_end;
                Some(PrimitiveBatch::MonochromeSprites {
                    texture_id,
                    range: sprites_start..sprites_end,
                })
            }
            PrimitiveKind::SubpixelSprite => {
                let texture_id = self.subpixel_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.subpixel_sprites_start;
                let mut sprites_end = sprites_start + 1;
                self.subpixel_sprites_iter.next();
                while self
                    .subpixel_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.subpixel_sprites_start = sprites_end;
                Some(PrimitiveBatch::SubpixelSprites {
                    texture_id,
                    range: sprites_start..sprites_end,
                })
            }
            PrimitiveKind::PolychromeSprite => {
                let texture_id = self.polychrome_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.polychrome_sprites_start;
                let mut sprites_end = sprites_start + 1;
                self.polychrome_sprites_iter.next();
                while self
                    .polychrome_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.polychrome_sprites_start = sprites_end;
                Some(PrimitiveBatch::PolychromeSprites {
                    texture_id,
                    range: sprites_start..sprites_end,
                })
            }
            PrimitiveKind::Surface => {
                let surfaces_start = self.surfaces_start;
                let mut surfaces_end = surfaces_start + 1;
                self.surfaces_iter.next();
                while self
                    .surfaces_iter
                    .next_if(|surface| (surface.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    surfaces_end += 1;
                }
                self.surfaces_start = surfaces_end;
                Some(PrimitiveBatch::Surfaces(surfaces_start..surfaces_end))
            }
        }
    }
}

#[derive(Debug)]
#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
#[allow(missing_docs)]
pub enum PrimitiveBatch {
    Shadows(Range<usize>),
    Quads(Range<usize>),
    Paths(Range<usize>),
    Underlines(Range<usize>),
    MonochromeSprites {
        texture_id: AtlasTextureId,
        range: Range<usize>,
    },
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    SubpixelSprites {
        texture_id: AtlasTextureId,
        range: Range<usize>,
    },
    PolychromeSprites {
        texture_id: AtlasTextureId,
        range: Range<usize>,
    },
    Surfaces(Range<usize>),
}

impl PrimitiveBatch {
    #[expect(missing_docs)]
    pub fn label(&self) -> String {
        match self {
            Self::Shadows(range) => format!("shadows ({})", range.len()),
            Self::Quads(range) => format!("quads ({})", range.len()),
            Self::Paths(range) => format!("paths ({})", range.len()),
            Self::Underlines(range) => format!("underlines ({})", range.len()),
            Self::MonochromeSprites { texture_id, range } => {
                format!(
                    "monochrome sprites ({}) on atlas {}",
                    range.len(),
                    texture_id.index
                )
            }
            Self::SubpixelSprites { texture_id, range } => {
                format!(
                    "subpixel sprites ({}) on atlas {}",
                    range.len(),
                    texture_id.index
                )
            }
            Self::PolychromeSprites { texture_id, range } => {
                format!(
                    "polychrome sprites ({}) on atlas {}",
                    range.len(),
                    texture_id.index
                )
            }
            Self::Surfaces(range) => format!("surfaces ({})", range.len()),
        }
    }
}

#[derive(Default, Debug, Copy, Clone)]
#[repr(C)]
#[expect(missing_docs)]
pub struct Quad {
    pub order: DrawOrder,
    pub border_style: BorderStyle,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub background: Background,
    pub border_color: Hsla,
    pub corner_radii: Corners<ScaledPixels>,
    pub border_widths: Edges<ScaledPixels>,
}

impl From<Quad> for Primitive {
    fn from(quad: Quad) -> Self {
        Primitive::Quad(quad)
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
#[expect(missing_docs)]
pub struct Underline {
    pub order: DrawOrder,
    pub pad: u32, // align to 8 bytes
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub thickness: ScaledPixels,
    pub wavy: PaddedBool32,
}

impl From<Underline> for Primitive {
    fn from(underline: Underline) -> Self {
        Primitive::Underline(underline)
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
#[expect(missing_docs)]
pub struct Shadow {
    pub order: DrawOrder,
    pub blur_radius: ScaledPixels,
    pub bounds: Bounds<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub element_bounds: Bounds<ScaledPixels>,
    pub element_corner_radii: Corners<ScaledPixels>,
    /// 0 = drop shadow (rendered outside the element), 1 = inset shadow (rendered inside).
    pub inset: u32,
    pub pad: u32, // align to 8 bytes
}

impl From<Shadow> for Primitive {
    fn from(shadow: Shadow) -> Self {
        Primitive::Shadow(shadow)
    }
}

/// The style of a border.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[repr(C)]
pub enum BorderStyle {
    /// A solid border.
    #[default]
    Solid = 0,
    /// A dashed border.
    Dashed = 1,
}

/// A data type representing a 2 dimensional transformation that can be applied to an element.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct TransformationMatrix {
    /// 2x2 matrix containing rotation and scale,
    /// stored row-major
    pub rotation_scale: [[f32; 2]; 2],
    /// translation vector
    pub translation: [f32; 2],
}

impl Eq for TransformationMatrix {}

impl TransformationMatrix {
    /// The unit matrix, has no effect.
    pub fn unit() -> Self {
        Self {
            rotation_scale: [[1.0, 0.0], [0.0, 1.0]],
            translation: [0.0, 0.0],
        }
    }

    /// Move the origin by a given point
    pub fn translate(self, point: Point<ScaledPixels>) -> Self {
        self.compose(Self {
            rotation_scale: [[1.0, 0.0], [0.0, 1.0]],
            translation: [point.x.0, point.y.0],
        })
    }

    /// Clockwise rotation in radians around the origin
    pub fn rotate(self, angle: Radians) -> Self {
        self.compose(Self {
            rotation_scale: [
                [angle.0.cos(), -angle.0.sin()],
                [angle.0.sin(), angle.0.cos()],
            ],
            translation: [0.0, 0.0],
        })
    }

    /// Scale around the origin
    pub fn scale(self, size: Size<f32>) -> Self {
        self.compose(Self {
            rotation_scale: [[size.width, 0.0], [0.0, size.height]],
            translation: [0.0, 0.0],
        })
    }

    /// Perform matrix multiplication with another transformation
    /// to produce a new transformation that is the result of
    /// applying both transformations: first, `other`, then `self`.
    #[inline]
    pub fn compose(self, other: TransformationMatrix) -> TransformationMatrix {
        if other == Self::unit() {
            return self;
        }
        // Perform matrix multiplication
        TransformationMatrix {
            rotation_scale: [
                [
                    self.rotation_scale[0][0] * other.rotation_scale[0][0]
                        + self.rotation_scale[0][1] * other.rotation_scale[1][0],
                    self.rotation_scale[0][0] * other.rotation_scale[0][1]
                        + self.rotation_scale[0][1] * other.rotation_scale[1][1],
                ],
                [
                    self.rotation_scale[1][0] * other.rotation_scale[0][0]
                        + self.rotation_scale[1][1] * other.rotation_scale[1][0],
                    self.rotation_scale[1][0] * other.rotation_scale[0][1]
                        + self.rotation_scale[1][1] * other.rotation_scale[1][1],
                ],
            ],
            translation: [
                self.translation[0]
                    + self.rotation_scale[0][0] * other.translation[0]
                    + self.rotation_scale[0][1] * other.translation[1],
                self.translation[1]
                    + self.rotation_scale[1][0] * other.translation[0]
                    + self.rotation_scale[1][1] * other.translation[1],
            ],
        }
    }

    /// Apply transformation to a point, mainly useful for debugging
    pub fn apply(&self, point: Point<Pixels>) -> Point<Pixels> {
        let input = [point.x.0, point.y.0];
        let mut output = self.translation;
        for (i, output_cell) in output.iter_mut().enumerate() {
            for (k, input_cell) in input.iter().enumerate() {
                *output_cell += self.rotation_scale[i][k] * *input_cell;
            }
        }
        Point::new(output[0].into(), output[1].into())
    }
}

impl Default for TransformationMatrix {
    fn default() -> Self {
        Self::unit()
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct MonochromeSprite {
    pub order: DrawOrder,
    pub pad: u32,
    pub tile: AtlasTile,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub transformation: TransformationMatrix,
}

impl From<MonochromeSprite> for Primitive {
    fn from(sprite: MonochromeSprite) -> Self {
        Primitive::MonochromeSprite(sprite)
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct SubpixelSprite {
    pub order: DrawOrder,
    pub pad: u32, // align to 8 bytes
    pub tile: AtlasTile,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub transformation: TransformationMatrix,
}

impl From<SubpixelSprite> for Primitive {
    fn from(sprite: SubpixelSprite) -> Self {
        Primitive::SubpixelSprite(sprite)
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct PolychromeSprite {
    pub order: DrawOrder,
    pub pad: u32,
    pub tile: AtlasTile,
    pub grayscale: PaddedBool32,
    pub opacity: f32,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
}

impl From<PolychromeSprite> for Primitive {
    fn from(sprite: PolychromeSprite) -> Self {
        Primitive::PolychromeSprite(sprite)
    }
}

#[derive(Clone, Debug)]
#[allow(missing_docs)]
pub struct PaintSurface {
    pub order: DrawOrder,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub image_buffer: objc2_core_foundation::CFRetained<objc2_core_video::CVPixelBuffer>,
}

impl From<PaintSurface> for Primitive {
    fn from(surface: PaintSurface) -> Self {
        Primitive::Surface(surface)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[expect(missing_docs)]
pub struct PathId(pub usize);

/// A line made up of a series of vertices and control points.
#[derive(Clone, Debug)]
#[expect(missing_docs)]
pub struct Path<P: Clone + Debug + Default + PartialEq> {
    pub id: PathId,
    pub order: DrawOrder,
    pub bounds: Bounds<P>,
    pub content_mask: ContentMask<P>,
    pub vertices: Vec<PathVertex<P>>,
    pub color: Background,
    start: Point<P>,
    current: Point<P>,
    contour_count: usize,
}

impl Path<Pixels> {
    /// Create a new path with the given starting point.
    pub fn new(start: Point<Pixels>) -> Self {
        Self {
            id: PathId(0),
            order: DrawOrder::default(),
            vertices: Vec::new(),
            start,
            current: start,
            bounds: Bounds {
                origin: start,
                size: Default::default(),
            },
            content_mask: Default::default(),
            color: Default::default(),
            contour_count: 0,
        }
    }

    /// Scale this path by the given factor.
    pub fn scale(&self, factor: f32) -> Path<ScaledPixels> {
        Path {
            id: self.id,
            order: self.order,
            bounds: self.bounds.scale(factor),
            content_mask: self.content_mask.scale(factor),
            vertices: self
                .vertices
                .iter()
                .map(|vertex| vertex.scale(factor))
                .collect(),
            start: self.start.map(|start| start.scale(factor)),
            current: self.current.scale(factor),
            contour_count: self.contour_count,
            color: self.color,
        }
    }

    /// Move the start, current point to the given point.
    pub fn move_to(&mut self, to: Point<Pixels>) {
        self.contour_count += 1;
        self.start = to;
        self.current = to;
    }

    /// Draw a straight line from the current point to the given point.
    pub fn line_to(&mut self, to: Point<Pixels>) {
        self.contour_count += 1;
        if self.contour_count > 1 {
            self.push_triangle(
                (self.start, self.current, to),
                (point(0., 1.), point(0., 1.), point(0., 1.)),
            );
        }
        self.current = to;
    }

    /// Draw a curve from the current point to the given point, using the given control point.
    pub fn curve_to(&mut self, to: Point<Pixels>, ctrl: Point<Pixels>) {
        self.contour_count += 1;
        if self.contour_count > 1 {
            self.push_triangle(
                (self.start, self.current, to),
                (point(0., 1.), point(0., 1.), point(0., 1.)),
            );
        }

        self.push_triangle(
            (self.current, ctrl, to),
            (point(0., 0.), point(0.5, 0.), point(1., 1.)),
        );
        self.current = to;
    }

    /// Push a triangle to the Path.
    pub fn push_triangle(
        &mut self,
        xy: (Point<Pixels>, Point<Pixels>, Point<Pixels>),
        st: (Point<f32>, Point<f32>, Point<f32>),
    ) {
        self.bounds = self
            .bounds
            .union(&Bounds {
                origin: xy.0,
                size: Default::default(),
            })
            .union(&Bounds {
                origin: xy.1,
                size: Default::default(),
            })
            .union(&Bounds {
                origin: xy.2,
                size: Default::default(),
            });

        self.vertices.push(PathVertex {
            xy_position: xy.0,
            st_position: st.0,
            content_mask: Default::default(),
        });
        self.vertices.push(PathVertex {
            xy_position: xy.1,
            st_position: st.1,
            content_mask: Default::default(),
        });
        self.vertices.push(PathVertex {
            xy_position: xy.2,
            st_position: st.2,
            content_mask: Default::default(),
        });
    }
}

impl<T> Path<T>
where
    T: Clone + Debug + Default + PartialEq + PartialOrd + Add<T, Output = T> + Sub<Output = T>,
{
    #[allow(unused)]
    #[expect(missing_docs)]
    pub fn clipped_bounds(&self) -> Bounds<T> {
        self.bounds.intersect(&self.content_mask.bounds)
    }
}

impl From<Path<ScaledPixels>> for Primitive {
    fn from(path: Path<ScaledPixels>) -> Self {
        Primitive::Path(path)
    }
}

#[derive(Clone, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct PathVertex<P: Clone + Debug + Default + PartialEq> {
    pub xy_position: Point<P>,
    pub st_position: Point<f32>,
    pub content_mask: ContentMask<P>,
}

#[expect(missing_docs)]
impl PathVertex<Pixels> {
    pub fn scale(&self, factor: f32) -> PathVertex<ScaledPixels> {
        PathVertex {
            xy_position: self.xy_position.scale(factor),
            st_position: self.st_position,
            content_mask: self.content_mask.scale(factor),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AtlasTextureId, AtlasTextureKind, DevicePixels, TileId, hsla, radians, red};

    fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<ScaledPixels> {
        Bounds {
            origin: point(ScaledPixels(x), ScaledPixels(y)),
            size: Size {
                width: ScaledPixels(width),
                height: ScaledPixels(height),
            },
        }
    }

    fn content_mask() -> ContentMask<ScaledPixels> {
        ContentMask {
            bounds: bounds(0., 0., 1000., 1000.),
        }
    }

    fn quad() -> Quad {
        Quad {
            bounds: bounds(10., 10., 20., 20.),
            content_mask: content_mask(),
            background: red().into(),
            border_color: hsla(0.5, 1., 0.5, 0.8),
            ..Default::default()
        }
    }

    fn tile() -> AtlasTile {
        AtlasTile {
            texture_id: AtlasTextureId {
                index: 0,
                kind: AtlasTextureKind::Monochrome,
            },
            tile_id: TileId(0),
            padding: 0,
            bounds: Bounds::<DevicePixels>::default(),
        }
    }

    fn monochrome_sprite() -> MonochromeSprite {
        MonochromeSprite {
            order: 0,
            pad: 0,
            bounds: bounds(10., 10., 20., 20.),
            content_mask: content_mask(),
            color: hsla(0., 0., 1., 0.5),
            tile: tile(),
            transformation: TransformationMatrix::unit(),
        }
    }

    fn center() -> Point<ScaledPixels> {
        point(ScaledPixels(20.), ScaledPixels(20.))
    }

    #[test]
    fn an_effect_changes_what_is_drawn_and_not_what_is_kept() {
        let mut scene = Scene::default();
        scene.push_effect(PaintEffect::opacity(0.5), center(), 1.);
        scene.insert_primitive(quad());
        scene.pop_effect();
        scene.insert_primitive(quad());

        assert_eq!(scene.quads[0].background.solid.a, 0.5);
        assert_eq!(scene.quads[0].border_color.a, 0.4);
        assert_eq!(scene.quads[1].background.solid.a, 1.);
        assert_eq!(scene.quads[1].border_color.a, 0.8);
        let kept = scene
            .paint_operations
            .iter()
            .map(|operation| match operation {
                PaintOperation::Primitive(Primitive::Quad(quad)) => quad.background.solid.a,
                _ => panic!("a quad"),
            });
        assert_eq!(kept.collect::<Vec<_>>(), [1., 1.]);

        // Replaying applies the effect that is active then to the primitive as it was.
        let mut replayed = Scene::default();
        replayed.push_effect(PaintEffect::opacity(0.25), center(), 1.);
        replayed.replay(0..2, &scene);
        replayed.pop_effect();
        assert_eq!(replayed.quads[0].background.solid.a, 0.25);
        assert_eq!(replayed.quads[1].background.solid.a, 0.25);
        assert_eq!(replayed.quads[0].border_color.a, 0.2);
    }

    #[test]
    fn effects_compose_and_end_with_their_element() {
        let mut scene = Scene::default();
        scene.push_effect(PaintEffect::opacity(0.5), center(), 1.);
        scene.push_effect(PaintEffect::opacity(0.5), center(), 1.);
        scene.insert_primitive(quad());
        scene.pop_effect();
        scene.insert_primitive(quad());
        scene.pop_effect();
        scene.insert_primitive(quad());
        let alphas: Vec<_> = scene
            .quads
            .iter()
            .map(|quad| quad.background.solid.a)
            .collect();
        assert_eq!(alphas, [0.25, 0.5, 1.]);
    }

    #[test]
    fn an_effect_changes_the_opacity_of_every_kind_of_primitive() {
        let mut scene = Scene::default();
        scene.push_effect(PaintEffect::opacity(0.5), center(), 1.);
        scene.insert_primitive(Shadow {
            order: 0,
            blur_radius: ScaledPixels(2.),
            bounds: bounds(10., 10., 20., 20.),
            corner_radii: Default::default(),
            content_mask: content_mask(),
            color: hsla(0., 0., 0., 0.6),
            element_bounds: bounds(10., 10., 20., 20.),
            element_corner_radii: Default::default(),
            inset: 0,
            pad: 0,
        });
        scene.insert_primitive(Underline {
            order: 0,
            pad: 0,
            bounds: bounds(10., 10., 20., 2.),
            content_mask: content_mask(),
            color: hsla(0., 0., 0., 0.6),
            thickness: ScaledPixels(1.),
            wavy: false.into(),
        });
        let mut path = Path::new(point(crate::px(10.), crate::px(10.))).scale(1.);
        path.bounds = bounds(10., 10., 20., 20.);
        path.content_mask = content_mask();
        path.color = red().into();
        scene.insert_primitive(path);
        scene.insert_primitive(monochrome_sprite());
        scene.insert_primitive(SubpixelSprite {
            order: 0,
            pad: 0,
            bounds: bounds(10., 10., 20., 20.),
            content_mask: content_mask(),
            color: hsla(0., 0., 1., 0.5),
            tile: tile(),
            transformation: TransformationMatrix::unit(),
        });
        scene.insert_primitive(PolychromeSprite {
            order: 0,
            pad: 0,
            grayscale: false.into(),
            opacity: 0.8,
            bounds: bounds(10., 10., 20., 20.),
            content_mask: content_mask(),
            corner_radii: Default::default(),
            tile: tile(),
        });
        scene.pop_effect();

        assert_eq!(scene.shadows[0].color.a, 0.3);
        assert_eq!(scene.underlines[0].color.a, 0.3);
        assert_eq!(scene.paths[0].color.solid.a, 0.5);
        assert_eq!(scene.monochrome_sprites[0].color.a, 0.25);
        assert_eq!(scene.subpixel_sprites[0].color.a, 0.25);
        assert_eq!(scene.polychrome_sprites[0].opacity, 0.4);
    }

    #[test]
    fn a_rotation_turns_sprites_about_the_centre_and_leaves_quads_alone() {
        let mut scene = Scene::default();
        scene.push_effect(
            PaintEffect::rotation(radians(std::f32::consts::FRAC_PI_2)),
            center(),
            1.,
        );
        scene.insert_primitive(quad());
        scene.insert_primitive(monochrome_sprite());
        scene.pop_effect();

        assert_eq!(scene.quads[0].bounds, bounds(10., 10., 20., 20.));
        // A quarter turn about (20, 20) takes the point (30, 20) to (20, 30).
        let moved = scene.monochrome_sprites[0]
            .transformation
            .apply(point(crate::px(30.), crate::px(20.)));
        assert!(
            (moved.x.0 - 20.).abs() < 1e-4 && (moved.y.0 - 30.).abs() < 1e-4,
            "{moved:?}"
        );
        // The centre stays where it was.
        let still = scene.monochrome_sprites[0]
            .transformation
            .apply(point(crate::px(20.), crate::px(20.)));
        assert!((still.x.0 - 20.).abs() < 1e-4 && (still.y.0 - 20.).abs() < 1e-4);
        // What is kept to replay has the sprite as it was.
        let PaintOperation::Primitive(Primitive::MonochromeSprite(kept)) =
            &scene.paint_operations[1]
        else {
            panic!("a sprite");
        };
        assert_eq!(kept.transformation, TransformationMatrix::unit());
    }

    #[test]
    fn a_rotation_comes_after_the_sprites_own_transformation_and_inside_an_outer_rotation() {
        let own = TransformationMatrix::unit().translate(point(ScaledPixels(5.), ScaledPixels(0.)));
        let mut scene = Scene::default();
        scene.push_effect(
            PaintEffect::rotation(radians(std::f32::consts::PI)),
            point(ScaledPixels(0.), ScaledPixels(0.)),
            1.,
        );
        scene.push_effect(
            PaintEffect::rotation(radians(std::f32::consts::PI)),
            point(ScaledPixels(0.), ScaledPixels(0.)),
            1.,
        );
        scene.insert_primitive(MonochromeSprite {
            transformation: own,
            ..monochrome_sprite()
        });
        scene.pop_effect();
        scene.pop_effect();
        // Two half turns about the origin bring the sprite's own translation back.
        let moved = scene.monochrome_sprites[0]
            .transformation
            .apply(point(crate::px(1.), crate::px(1.)));
        assert!(
            (moved.x.0 - 6.).abs() < 1e-4 && (moved.y.0 - 1.).abs() < 1e-4,
            "{moved:?}"
        );
    }

    fn moved(x: f32, y: f32) -> PaintEffect {
        PaintEffect::translation(point(crate::px(x), crate::px(y)))
    }

    fn shadow() -> Shadow {
        Shadow {
            order: 0,
            blur_radius: ScaledPixels(2.),
            bounds: bounds(10., 10., 20., 20.),
            corner_radii: Default::default(),
            content_mask: content_mask(),
            color: hsla(0., 0., 0., 0.6),
            element_bounds: bounds(12., 12., 16., 16.),
            element_corner_radii: Default::default(),
            inset: 0,
            pad: 0,
        }
    }

    fn underline() -> Underline {
        Underline {
            order: 0,
            pad: 0,
            bounds: bounds(10., 28., 20., 2.),
            content_mask: content_mask(),
            color: hsla(0., 0., 0., 0.6),
            thickness: ScaledPixels(1.),
            wavy: false.into(),
        }
    }

    fn path() -> Path<ScaledPixels> {
        let mut path = Path::new(point(crate::px(10.), crate::px(10.))).scale(1.);
        path.bounds = bounds(10., 10., 20., 20.);
        path.content_mask = content_mask();
        path.color = red().into();
        for (x, y) in [(10., 10.), (30., 10.), (10., 30.)] {
            path.vertices.push(PathVertex {
                xy_position: point(ScaledPixels(x), ScaledPixels(y)),
                st_position: point(0., 0.),
                content_mask: content_mask(),
            });
        }
        path
    }

    fn polychrome_sprite() -> PolychromeSprite {
        PolychromeSprite {
            order: 0,
            pad: 0,
            grayscale: false.into(),
            opacity: 0.8,
            bounds: bounds(10., 10., 20., 20.),
            content_mask: content_mask(),
            corner_radii: Default::default(),
            tile: tile(),
        }
    }

    #[test]
    fn a_translation_moves_every_kind_of_primitive_and_the_clip_stays() {
        let mut scene = Scene::default();
        // Logical pixels, at the scale of 2 of the window.
        scene.push_effect(moved(3., 4.), center(), 2.);
        scene.insert_primitive(quad());
        scene.insert_primitive(shadow());
        scene.insert_primitive(underline());
        scene.insert_primitive(path());
        scene.insert_primitive(polychrome_sprite());
        scene.pop_effect();

        assert_eq!(scene.quads[0].bounds, bounds(16., 18., 20., 20.));
        assert_eq!(scene.shadows[0].bounds, bounds(16., 18., 20., 20.));
        assert_eq!(scene.shadows[0].element_bounds, bounds(18., 20., 16., 16.));
        assert_eq!(scene.underlines[0].bounds, bounds(16., 36., 20., 2.));
        assert_eq!(scene.paths[0].bounds, bounds(16., 18., 20., 20.));
        let corners: Vec<_> = scene.paths[0]
            .vertices
            .iter()
            .map(|vertex| (vertex.xy_position.x.0, vertex.xy_position.y.0))
            .collect();
        assert_eq!(corners, [(16., 18.), (36., 18.), (16., 38.)]);
        assert_eq!(
            scene.polychrome_sprites[0].bounds,
            bounds(16., 18., 20., 20.)
        );
        // Opacity stays as it was.
        assert_eq!(scene.quads[0].background.solid.a, 1.);
        assert_eq!(scene.polychrome_sprites[0].opacity, 0.8);
        // The clip they were painted under did not move.
        assert_eq!(scene.quads[0].content_mask, content_mask());
        assert_eq!(scene.shadows[0].content_mask, content_mask());
        assert_eq!(scene.polychrome_sprites[0].content_mask, content_mask());
        // What is kept to replay is where they were painted.
        let PaintOperation::Primitive(Primitive::Quad(kept)) = &scene.paint_operations[0] else {
            panic!("a quad");
        };
        assert_eq!(kept.bounds, bounds(10., 10., 20., 20.));
        let PaintOperation::Primitive(Primitive::Path(kept)) = &scene.paint_operations[3] else {
            panic!("a path");
        };
        assert_eq!(kept.bounds, bounds(10., 10., 20., 20.));
        assert_eq!(kept.vertices[1].xy_position.x, ScaledPixels(30.));
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    #[test]
    fn a_translation_moves_a_surface() {
        let mut scene = Scene::default();
        scene.push_effect(moved(3., 4.), center(), 1.);
        scene.insert_primitive(PaintSurface {
            order: 0,
            bounds: bounds(10., 10., 20., 20.),
            content_mask: content_mask(),
        });
        scene.pop_effect();
        assert_eq!(scene.surfaces[0].bounds, bounds(13., 14., 20., 20.));
    }

    #[test]
    fn a_translation_moves_sprites_by_their_transformation() {
        let mut scene = Scene::default();
        scene.push_effect(moved(3., 4.), center(), 1.);
        scene.insert_primitive(monochrome_sprite());
        scene.insert_primitive(SubpixelSprite {
            order: 0,
            pad: 0,
            bounds: bounds(10., 10., 20., 20.),
            content_mask: content_mask(),
            color: hsla(0., 0., 1., 0.5),
            tile: tile(),
            transformation: TransformationMatrix::unit(),
        });
        scene.pop_effect();

        let at = |transformation: TransformationMatrix, x: f32, y: f32| {
            let moved = transformation.apply(point(crate::px(x), crate::px(y)));
            (moved.x.0, moved.y.0)
        };
        assert_eq!(
            at(scene.monochrome_sprites[0].transformation, 30., 20.),
            (33., 24.)
        );
        assert_eq!(
            at(scene.subpixel_sprites[0].transformation, 0., 0.),
            (3., 4.)
        );
        assert_eq!(
            scene.monochrome_sprites[0].bounds,
            bounds(10., 10., 20., 20.)
        );
    }

    #[test]
    fn a_rotation_turns_sprites_before_a_translation_moves_them() {
        let mut scene = Scene::default();
        scene.push_effect(
            moved(5., 7.).with_rotation(radians(std::f32::consts::FRAC_PI_2)),
            center(),
            1.,
        );
        scene.insert_primitive(quad());
        scene.insert_primitive(monochrome_sprite());
        scene.pop_effect();

        // A quarter turn about (20, 20) takes (30, 20) to (20, 30), and the move follows.
        let turned = scene.monochrome_sprites[0]
            .transformation
            .apply(point(crate::px(30.), crate::px(20.)));
        assert!(
            (turned.x.0 - 25.).abs() < 1e-4 && (turned.y.0 - 37.).abs() < 1e-4,
            "{turned:?}"
        );
        // The centre went where the quad did.
        let centre = scene.monochrome_sprites[0]
            .transformation
            .apply(point(crate::px(20.), crate::px(20.)));
        assert!((centre.x.0 - 25.).abs() < 1e-4 && (centre.y.0 - 27.).abs() < 1e-4);
        assert_eq!(scene.quads[0].bounds, bounds(15., 17., 20., 20.));
    }

    #[test]
    fn translations_add_up_and_opacity_and_translation_go_together() {
        let mut scene = Scene::default();
        scene.push_effect(moved(10., 0.).with_opacity(0.5), center(), 1.);
        scene.push_effect(moved(0., 5.).with_opacity(0.5), center(), 1.);
        scene.insert_primitive(quad());
        scene.insert_primitive(monochrome_sprite());
        scene.pop_effect();
        scene.insert_primitive(quad());
        scene.pop_effect();
        scene.insert_primitive(quad());

        let drawn: Vec<_> = scene
            .quads
            .iter()
            .map(|quad| {
                (
                    quad.bounds.origin.x.0,
                    quad.bounds.origin.y.0,
                    quad.background.solid.a,
                )
            })
            .collect();
        assert_eq!(drawn, [(20., 15., 0.25), (20., 10., 0.5), (10., 10., 1.)]);
        let moved = scene.monochrome_sprites[0]
            .transformation
            .apply(point(crate::px(0.), crate::px(0.)));
        assert_eq!((moved.x.0, moved.y.0), (10., 5.));
    }

    #[test]
    fn an_outer_rotation_carries_what_an_inner_translation_moves() {
        let mut scene = Scene::default();
        let origin = point(ScaledPixels(0.), ScaledPixels(0.));
        scene.push_effect(
            PaintEffect::rotation(radians(std::f32::consts::PI)),
            origin,
            1.,
        );
        scene.push_effect(moved(1., 0.), origin, 1.);
        scene.insert_primitive(monochrome_sprite());
        scene.pop_effect();
        scene.pop_effect();

        // The inner move takes (2, 3) to (3, 3), and a half turn about the origin takes that to
        // (-3, -3).
        let turned = scene.monochrome_sprites[0]
            .transformation
            .apply(point(crate::px(2.), crate::px(3.)));
        assert!(
            (turned.x.0 + 3.).abs() < 1e-4 && (turned.y.0 + 3.).abs() < 1e-4,
            "{turned:?}"
        );
    }

    #[test]
    fn a_translation_is_for_the_place_a_primitive_is_drawn_at() {
        // A quad painted at (10, 10), under a clip that ends at 50.
        let clipped = |x: f32| Quad {
            bounds: bounds(x, 10., 20., 20.),
            content_mask: ContentMask {
                bounds: bounds(0., 0., 50., 50.),
            },
            ..quad()
        };
        let mut scene = Scene::default();
        scene.push_effect(moved(100., 0.), center(), 1.);
        scene.insert_primitive(clipped(10.));
        scene.pop_effect();
        // Painted outside the clip, and moved into it.
        scene.push_effect(moved(-60., 0.), center(), 1.);
        scene.insert_primitive(clipped(60.));
        scene.pop_effect();
        assert_eq!(scene.quads.len(), 1, "one moved out of its clip, one in");
        assert_eq!(scene.quads[0].bounds, bounds(0., 10., 20., 20.));
        assert_eq!(scene.len(), 2, "and both are kept");
    }

    #[test]
    fn what_a_translation_hid_is_there_to_replay_when_it_moves_back() {
        let clipped = Quad {
            content_mask: ContentMask {
                bounds: bounds(0., 0., 50., 50.),
            },
            ..quad()
        };
        let mut scene = Scene::default();
        scene.push_effect(moved(100., 0.), center(), 1.);
        scene.insert_primitive(clipped);
        scene.pop_effect();
        assert!(scene.quads.is_empty(), "moved out of its clip");
        assert_eq!(scene.len(), 1, "and kept");

        let mut replayed = Scene::default();
        replayed.push_effect(moved(20., 0.), center(), 1.);
        replayed.replay(0..1, &scene);
        replayed.pop_effect();
        assert_eq!(replayed.quads[0].bounds, bounds(30., 10., 20., 20.));

        // Without an effect, a primitive outside its clip is dropped, as it always was.
        let mut plain = Scene::default();
        plain.insert_primitive(Quad {
            bounds: bounds(100., 10., 20., 20.),
            ..clipped
        });
        assert_eq!(plain.len(), 0);
    }

    #[test]
    fn a_scene_that_is_cleared_has_no_effect_left() {
        let mut scene = Scene::default();
        scene.push_effect(PaintEffect::opacity(0.5), center(), 1.);
        scene.clear();
        scene.insert_primitive(quad());
        assert_eq!(scene.quads[0].background.solid.a, 1.);
    }
}
