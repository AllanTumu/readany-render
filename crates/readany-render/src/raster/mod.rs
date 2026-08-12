use crate::model::*;
use crate::{RenderError, RenderErrorCode};
use base64::Engine;
use image::ImageEncoder;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pixmap {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Pixmap {
    /// Encodes the deterministic RGBA buffer as a PNG without filesystem access.
    pub fn encode_png(&self) -> Result<Vec<u8>, RenderError> {
        let mut output = Vec::new();
        image::codecs::png::PngEncoder::new(&mut output)
            .write_image(
                &self.data,
                self.width,
                self.height,
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|_| {
                RenderError::new(
                    RenderErrorCode::Rasterisation,
                    "the rendered page could not be encoded; try a lower scale",
                )
            })?;
        Ok(output)
    }
}

#[derive(Clone, Debug)]
pub struct SvgOptions {
    pub embed_fonts: bool,
    pub precision: u8,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            embed_fonts: false,
            precision: 3,
        }
    }
}

pub fn rasterise(page: &Page, scale: f32) -> Result<Pixmap, RenderError> {
    rasterise_rect(
        page,
        Rect {
            x: 0.0,
            y: 0.0,
            width: page.size.width,
            height: page.size.height,
        },
        scale,
    )
}

/// Rasterises only a document-coordinate viewport.
///
/// This is the primary raster API for sheets: their natural canvas may contain
/// hundreds of millions of pixels even though a reader only displays a small
/// region at once. Items outside `rect` are neither decoded nor painted.
pub fn rasterise_rect(page: &Page, rect: Rect, scale: f32) -> Result<Pixmap, RenderError> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err(RenderError::new(
            RenderErrorCode::Rasterisation,
            "scale must be a positive finite number",
        ));
    }
    validate_viewport(rect)?;
    let width = dimension(rect.width * scale)?;
    let height = dimension(rect.height * scale)?;
    let pixel_count = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| {
            RenderError::new(
                RenderErrorCode::Rasterisation,
                "the raster dimensions are too large; choose a lower scale",
            )
        })?;
    // 100 MP matches Limits::default().image_pixels and kept peak RGBA memory
    // below 400 MiB on the motivating 393x328-sheet workload.
    if pixel_count > 100_000_000 {
        return Err(RenderError::new(
            RenderErrorCode::Rasterisation,
            "the raster exceeds 100 million pixels; choose a lower scale",
        ));
    }
    validate_raster_items(&page.items, rect)?;
    let pixels = pixel_count.checked_mul(4).ok_or_else(|| {
        RenderError::new(
            RenderErrorCode::Rasterisation,
            "the raster dimensions are too large; choose a lower scale",
        )
    })?;
    let len = usize::try_from(pixels).map_err(|_| {
        RenderError::new(
            RenderErrorCode::Rasterisation,
            "the raster dimensions are too large; choose a lower scale",
        )
    })?;
    let mut pixmap = Pixmap {
        width,
        height,
        data: vec![255; len],
    };
    paint_items(&mut pixmap, &page.items, scale, None, rect);
    Ok(pixmap)
}

/// Returns a provenance-preserving display-list slice in document coordinates.
/// Groups remain groups and contain only children intersecting the viewport.
pub fn items_in_rect(page: &Page, rect: Rect) -> Result<Vec<Item>, RenderError> {
    validate_viewport(rect)?;
    Ok(filter_items(&page.items, rect))
}

fn validate_viewport(rect: Rect) -> Result<(), RenderError> {
    if !rect.x.is_finite()
        || !rect.y.is_finite()
        || !rect.width.is_finite()
        || !rect.height.is_finite()
        || rect.width <= 0.0
        || rect.height <= 0.0
    {
        return Err(RenderError::new(
            RenderErrorCode::Rasterisation,
            "the viewport must have finite coordinates and positive dimensions",
        ));
    }
    Ok(())
}

fn validate_raster_items(items: &[Item], viewport: Rect) -> Result<(), RenderError> {
    for item in items {
        if !item_intersects(item, viewport) {
            continue;
        }
        match item {
            Item::Image(image) => {
                let reader = image::ImageReader::new(std::io::Cursor::new(&image.data.bytes))
                    .with_guessed_format()
                    .map_err(|_| {
                        RenderError::new(
                            RenderErrorCode::Rasterisation,
                            "an image item has a damaged header; render the source document again",
                        )
                    })?;
                let (width, height) = reader.into_dimensions().map_err(|_| {
                    RenderError::new(
                        RenderErrorCode::Rasterisation,
                        "an image item has damaged dimensions; render the source document again",
                    )
                })?;
                if u64::from(width).saturating_mul(u64::from(height)) > 100_000_000 {
                    return Err(RenderError::new(
                        RenderErrorCode::Rasterisation,
                        "an image item exceeds 100 million pixels; render at a lower resolution",
                    ));
                }
            }
            Item::Group(group) => validate_raster_items(&group.items, viewport)?,
            Item::Glyphs(_) | Item::Path(_) => {}
        }
    }
    Ok(())
}

fn dimension(value: f32) -> Result<u32, RenderError> {
    if !value.is_finite() || value < 0.0 || value > u32::MAX as f32 {
        return Err(RenderError::new(
            RenderErrorCode::Rasterisation,
            "the page dimensions are invalid",
        ));
    }
    Ok(value.ceil() as u32)
}

fn paint_items(
    pixmap: &mut Pixmap,
    items: &[Item],
    scale: f32,
    clip: Option<Rect>,
    viewport: Rect,
) {
    for item in items {
        if !item_intersects(item, viewport) {
            continue;
        }
        match item {
            Item::Path(path) => paint_path(pixmap, path, scale, clip, viewport),
            Item::Glyphs(run) => paint_glyphs(pixmap, run, scale, clip, viewport),
            Item::Image(image) => paint_image(pixmap, image, scale, clip, viewport),
            Item::Group(group) => {
                let nested_clip = match (clip, group.clip) {
                    (Some(parent), Some(child)) => intersect_rect(parent, child),
                    (Some(parent), None) => Some(parent),
                    (None, Some(child)) => Some(child),
                    (None, None) => None,
                };
                if group.clip.is_none() || nested_clip.is_some() {
                    paint_items(pixmap, &group.items, scale, nested_clip, viewport);
                }
            }
        }
    }
}

fn paint_path(
    pixmap: &mut Pixmap,
    item: &PathItem,
    scale: f32,
    clip: Option<Rect>,
    viewport: Rect,
) {
    let mut builder = tiny_skia::PathBuilder::new();
    for command in &item.path.commands {
        match command {
            PathCommand::Move(point) => builder.move_to(point.x, point.y),
            PathCommand::Line(point) => builder.line_to(point.x, point.y),
            PathCommand::Quad(control, point) => {
                builder.quad_to(control.x, control.y, point.x, point.y)
            }
            PathCommand::Cubic(first, second, point) => {
                builder.cubic_to(first.x, first.y, second.x, second.y, point.x, point.y)
            }
            PathCommand::Close => builder.close(),
        }
    }
    let Some(path) = builder.finish() else {
        return;
    };
    let Some(mut target) =
        tiny_skia::PixmapMut::from_bytes(&mut pixmap.data, pixmap.width, pixmap.height)
    else {
        return;
    };
    let transform = tiny_skia::Transform::from_row(
        scale,
        0.0,
        0.0,
        scale,
        -viewport.x * scale,
        -viewport.y * scale,
    );
    let mask =
        clip.and_then(|clip| raster_clip_mask(pixmap.width, pixmap.height, clip, scale, viewport));
    if let Some(fill) = &item.fill {
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(fill.colour.r, fill.colour.g, fill.colour.b, fill.colour.a);
        paint.anti_alias = true;
        target.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            transform,
            mask.as_ref(),
        );
    }
    if let Some(stroke) = &item.stroke {
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(
            stroke.paint.colour.r,
            stroke.paint.colour.g,
            stroke.paint.colour.b,
            stroke.paint.colour.a,
        );
        paint.anti_alias = true;
        let mut geometry = tiny_skia::Stroke {
            width: stroke.width.max(0.25),
            ..tiny_skia::Stroke::default()
        };
        if !stroke.dash.is_empty() {
            geometry.dash = tiny_skia::StrokeDash::new(stroke.dash.clone(), 0.0);
        }
        target.stroke_path(&path, &paint, &geometry, transform, mask.as_ref());
    }
}

fn paint_glyphs(
    pixmap: &mut Pixmap,
    run: &GlyphRun,
    scale: f32,
    clip: Option<Rect>,
    viewport: Rect,
) {
    if paint_font_glyphs(pixmap, run, scale, clip, viewport) {
        return;
    }
    paint_glyph_boxes(pixmap, run, scale, clip, viewport);
}

fn paint_glyph_boxes(
    pixmap: &mut Pixmap,
    run: &GlyphRun,
    scale: f32,
    clip: Option<Rect>,
    viewport: Rect,
) {
    let angle = run.rotation_deg.to_radians();
    let (sin, cos) = angle.sin_cos();
    let mut advance = 0.0;
    for glyph in &run.glyphs {
        if glyph.glyph_id != u32::from(' ') {
            fill_rect(
                pixmap,
                Rect {
                    x: (run.origin.x + advance * cos + glyph.x_offset - viewport.x) * scale,
                    y: (run.origin.y + advance * sin - run.size_px * 0.76 + glyph.y_offset
                        - viewport.y)
                        * scale,
                    width: (glyph.x_advance * 0.78).max(1.0) * scale,
                    height: run.size_px * 0.76 * scale,
                },
                run.colour,
                clip.map(|clip| Rect {
                    x: (clip.x - viewport.x) * scale,
                    y: (clip.y - viewport.y) * scale,
                    width: clip.width * scale,
                    height: clip.height * scale,
                }),
            );
        }
        advance += glyph.x_advance;
    }
}

fn paint_font_glyphs(
    pixmap: &mut Pixmap,
    run: &GlyphRun,
    scale: f32,
    clip: Option<Rect>,
    viewport: Rect,
) -> bool {
    use skrifa::{
        MetadataProvider,
        instance::{LocationRef, Size},
        outline::DrawSettings,
    };
    let Some(font_bytes) = crate::text::font_bytes(run.font) else {
        return false;
    };
    let Ok(face) = skrifa::FontRef::new(&font_bytes) else {
        return false;
    };
    let Some(mut target) =
        tiny_skia::PixmapMut::from_bytes(&mut pixmap.data, pixmap.width, pixmap.height)
    else {
        return false;
    };
    let units = face
        .metrics(Size::unscaled(), LocationRef::default())
        .units_per_em;
    let font_scale = run.size_px / f32::from(units) * scale;
    let outlines = face.outline_glyphs();
    let mask =
        clip.and_then(|clip| raster_clip_mask(pixmap.width, pixmap.height, clip, scale, viewport));
    let angle = run.rotation_deg.to_radians();
    let (sin, cos) = angle.sin_cos();
    let mut advance = 0.0;
    for glyph in &run.glyphs {
        let mut builder = OutlinePathBuilder::default();
        let drew_outline = outlines
            .get(skrifa::GlyphId::new(glyph.glyph_id))
            .is_some_and(|outline| {
                outline
                    .draw(
                        DrawSettings::unhinted(Size::unscaled(), LocationRef::default()),
                        &mut builder,
                    )
                    .is_ok()
            });
        if drew_outline {
            if let Some(path) = builder.inner.finish() {
                let mut paint = tiny_skia::Paint::default();
                paint.set_color_rgba8(run.colour.r, run.colour.g, run.colour.b, run.colour.a);
                paint.anti_alias = true;
                target.fill_path(
                    &path,
                    &paint,
                    tiny_skia::FillRule::Winding,
                    tiny_skia::Transform::from_row(
                        font_scale * cos,
                        font_scale * sin,
                        font_scale * sin,
                        -font_scale * cos,
                        (run.origin.x - viewport.x + advance * cos + glyph.x_offset * cos
                            - glyph.y_offset * sin)
                            * scale,
                        (run.origin.y - viewport.y
                            + advance * sin
                            + glyph.x_offset * sin
                            + glyph.y_offset * cos)
                            * scale,
                    ),
                    mask.as_ref(),
                );
            }
        }
        advance += glyph.x_advance;
    }
    true
}

#[derive(Default)]
struct OutlinePathBuilder {
    inner: tiny_skia::PathBuilder,
}

impl skrifa::outline::OutlinePen for OutlinePathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.inner.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.inner.line_to(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.inner.quad_to(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.inner.cubic_to(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        self.inner.close();
    }
}

fn paint_image(
    pixmap: &mut Pixmap,
    item: &ImageItem,
    scale: f32,
    clip: Option<Rect>,
    viewport: Rect,
) {
    let Ok(decoded) = image::load_from_memory(&item.data.bytes) else {
        return;
    };
    let width = (item.rect.width * scale).max(1.0) as u32;
    let height = (item.rect.height * scale).max(1.0) as u32;
    let image = decoded
        .resize_exact(width, height, image::imageops::FilterType::Triangle)
        .to_rgba8();
    let x0 = ((item.rect.x - viewport.x) * scale).floor() as i64;
    let y0 = ((item.rect.y - viewport.y) * scale).floor() as i64;
    for (x, y, pixel) in image.enumerate_pixels() {
        let target_x = x0 + i64::from(x);
        let target_y = y0 + i64::from(y);
        if target_x < 0 || target_y < 0 {
            continue;
        }
        let Ok(target_x) = u32::try_from(target_x) else {
            continue;
        };
        let Ok(target_y) = u32::try_from(target_y) else {
            continue;
        };
        if clip.is_some_and(|clip| {
            !point_in_rect(
                target_x as f32 / scale + viewport.x,
                target_y as f32 / scale + viewport.y,
                clip,
            )
        }) {
            continue;
        }
        set_pixel(
            pixmap,
            target_x,
            target_y,
            Colour {
                r: pixel[0],
                g: pixel[1],
                b: pixel[2],
                a: pixel[3],
            },
        );
    }
}

fn fill_rect(pixmap: &mut Pixmap, rect: Rect, colour: Colour, clip: Option<Rect>) {
    let Some(rect) = clip.map_or(Some(rect), |clip| intersect_rect(rect, clip)) else {
        return;
    };
    let x0 = rect.x.max(0.0).floor() as u32;
    let y0 = rect.y.max(0.0).floor() as u32;
    let x1 = (rect.x + rect.width)
        .max(0.0)
        .ceil()
        .min(pixmap.width as f32) as u32;
    let y1 = (rect.y + rect.height)
        .max(0.0)
        .ceil()
        .min(pixmap.height as f32) as u32;
    for y in y0..y1 {
        for x in x0..x1 {
            set_pixel(pixmap, x, y, colour);
        }
    }
}

fn intersect_rect(first: Rect, second: Rect) -> Option<Rect> {
    let x = first.x.max(second.x);
    let y = first.y.max(second.y);
    let right = (first.x + first.width).min(second.x + second.width);
    let bottom = (first.y + first.height).min(second.y + second.height);
    (right > x && bottom > y).then_some(Rect {
        x,
        y,
        width: right - x,
        height: bottom - y,
    })
}

fn point_in_rect(x: f32, y: f32, rect: Rect) -> bool {
    x >= rect.x && y >= rect.y && x < rect.x + rect.width && y < rect.y + rect.height
}

fn raster_clip_mask(
    width: u32,
    height: u32,
    clip: Rect,
    scale: f32,
    viewport: Rect,
) -> Option<tiny_skia::Mask> {
    let rect = tiny_skia::Rect::from_xywh(
        (clip.x - viewport.x) * scale,
        (clip.y - viewport.y) * scale,
        clip.width * scale,
        clip.height * scale,
    )?;
    let path = tiny_skia::PathBuilder::from_rect(rect);
    let mut mask = tiny_skia::Mask::new(width, height)?;
    mask.fill_path(
        &path,
        tiny_skia::FillRule::Winding,
        true,
        tiny_skia::Transform::identity(),
    );
    Some(mask)
}

fn filter_items(items: &[Item], viewport: Rect) -> Vec<Item> {
    items
        .iter()
        .filter_map(|item| {
            if !item_intersects(item, viewport) {
                return None;
            }
            match item {
                Item::Group(group) => {
                    let children = filter_items(&group.items, viewport);
                    (!children.is_empty()).then(|| {
                        Item::Group(Group {
                            items: children,
                            clip: group.clip,
                            source: group.source.clone(),
                        })
                    })
                }
                Item::Glyphs(_) | Item::Path(_) | Item::Image(_) => Some(item.clone()),
            }
        })
        .collect()
}

fn item_intersects(item: &Item, viewport: Rect) -> bool {
    match item {
        Item::Glyphs(run) => intersect_rect(glyph_bounds(run), viewport).is_some(),
        Item::Path(path) => path_bounds(path)
            .and_then(|bounds| intersect_rect(bounds, viewport))
            .is_some(),
        Item::Image(image) => intersect_rect(image.rect, viewport).is_some(),
        Item::Group(group) => {
            if group
                .clip
                .is_some_and(|clip| intersect_rect(clip, viewport).is_none())
            {
                return false;
            }
            group
                .items
                .iter()
                .any(|item| item_intersects(item, viewport))
        }
    }
}

fn glyph_bounds(run: &GlyphRun) -> Rect {
    let advance = run.glyphs.iter().map(|glyph| glyph.x_advance).sum::<f32>();
    let width = advance.max(run.size_px * 0.5);
    let height = run.size_px.max(1.0);
    if run.rotation_deg == 0.0 {
        return Rect {
            x: run.origin.x,
            y: run.origin.y - height,
            width,
            height: height * 1.25,
        };
    }
    let angle = run.rotation_deg.to_radians();
    let (sin, cos) = angle.sin_cos();
    let corners = [
        (0.0, -height),
        (width, -height),
        (width, height * 0.25),
        (0.0, height * 0.25),
    ];
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for (x, y) in corners {
        let transformed_x = run.origin.x + x * cos - y * sin;
        let transformed_y = run.origin.y + x * sin + y * cos;
        min_x = min_x.min(transformed_x);
        min_y = min_y.min(transformed_y);
        max_x = max_x.max(transformed_x);
        max_y = max_y.max(transformed_y);
    }
    Rect {
        x: min_x,
        y: min_y,
        width: max_x - min_x,
        height: max_y - min_y,
    }
}

fn path_bounds(item: &PathItem) -> Option<Rect> {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    let mut include = |point: Point| {
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    };
    for command in &item.path.commands {
        match command {
            PathCommand::Move(point) | PathCommand::Line(point) => include(*point),
            PathCommand::Quad(control, point) => {
                include(*control);
                include(*point);
            }
            PathCommand::Cubic(first, second, point) => {
                include(*first);
                include(*second);
                include(*point);
            }
            PathCommand::Close => {}
        }
    }
    if !min_x.is_finite() {
        return None;
    }
    let padding = item
        .stroke
        .as_ref()
        .map(|stroke| stroke.width)
        .unwrap_or(0.0)
        / 2.0
        + 0.5;
    Some(Rect {
        x: min_x - padding,
        y: min_y - padding,
        width: (max_x - min_x + padding * 2.0).max(1.0),
        height: (max_y - min_y + padding * 2.0).max(1.0),
    })
}

fn set_pixel(pixmap: &mut Pixmap, x: u32, y: u32, colour: Colour) {
    if x >= pixmap.width || y >= pixmap.height {
        return;
    }
    let Some(offset) = y
        .checked_mul(pixmap.width)
        .and_then(|v| v.checked_add(x))
        .and_then(|v| v.checked_mul(4))
        .and_then(|v| usize::try_from(v).ok())
    else {
        return;
    };
    let alpha = u16::from(colour.a);
    for (index, channel) in [colour.r, colour.g, colour.b].into_iter().enumerate() {
        pixmap.data[offset + index] = ((u16::from(channel) * alpha
            + u16::from(pixmap.data[offset + index]) * (255 - alpha))
            / 255) as u8;
    }
    pixmap.data[offset + 3] = 255;
}

pub fn to_svg(page: &Page, options: &SvgOptions) -> Result<String, RenderError> {
    let mut out = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}">"#,
        page.size.width, page.size.height, page.size.width, page.size.height
    );
    out.push_str(r#"<rect width="100%" height="100%" fill="white"/>"#);
    if options.embed_fonts {
        let mut fonts = std::collections::BTreeMap::new();
        collect_fonts(&page.items, &mut fonts);
        out.push_str("<style>");
        for (id, family) in fonts {
            let bytes = crate::text::font_bytes(id).ok_or_else(|| {
                RenderError::new(
                    RenderErrorCode::NoFonts,
                    format!("font {family} is no longer available; render the document again before embedding SVG fonts"),
                )
            })?;
            let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
            out.push_str(&format!(
                "@font-face{{font-family:'{}';src:url(data:font/ttf;base64,{encoded})}}",
                escape(&family)
            ));
        }
        out.push_str("</style>");
    }
    let mut clip_id = 0_u32;
    svg_items(&mut out, &page.items, options.precision, &mut clip_id);
    out.push_str("</svg>");
    Ok(out)
}

fn collect_fonts(items: &[Item], fonts: &mut std::collections::BTreeMap<FontId, String>) {
    for item in items {
        match item {
            Item::Glyphs(run) => {
                fonts.entry(run.font).or_insert_with(|| run.family.clone());
            }
            Item::Group(group) => collect_fonts(&group.items, fonts),
            Item::Path(_) | Item::Image(_) => {}
        }
    }
}

fn svg_items(out: &mut String, items: &[Item], precision: u8, clip_id: &mut u32) {
    for item in items {
        match item {
            Item::Glyphs(run) => {
                let transform = if run.rotation_deg == 0.0 {
                    String::new()
                } else {
                    format!(
                        " transform=\"rotate({} {} {})\"",
                        run.rotation_deg, run.origin.x, run.origin.y
                    )
                };
                out.push_str(&format!(r#"<text x="{:.*}" y="{:.*}" font-family="{}" font-size="{:.*}" fill="{}"{}>{}</text>"#, precision as usize, run.origin.x, precision as usize, run.origin.y, escape(&run.family), precision as usize, run.size_px, css(run.colour), transform, escape(&run.text)));
            }
            Item::Path(path) => {
                let mut d = String::new();
                for command in &path.path.commands {
                    match command {
                        PathCommand::Move(p) => d.push_str(&format!("M{} {} ", p.x, p.y)),
                        PathCommand::Line(p) => d.push_str(&format!("L{} {} ", p.x, p.y)),
                        PathCommand::Quad(a, b) => {
                            d.push_str(&format!("Q{} {} {} {} ", a.x, a.y, b.x, b.y))
                        }
                        PathCommand::Cubic(a, b, c) => d.push_str(&format!(
                            "C{} {} {} {} {} {} ",
                            a.x, a.y, b.x, b.y, c.x, c.y
                        )),
                        PathCommand::Close => d.push_str("Z "),
                    }
                }
                let fill = path
                    .fill
                    .as_ref()
                    .map(|v| css(v.colour))
                    .unwrap_or_else(|| "none".into());
                let stroke = path
                    .stroke
                    .as_ref()
                    .map(|v| css(v.paint.colour))
                    .unwrap_or_else(|| "none".into());
                let width = path.stroke.as_ref().map(|v| v.width).unwrap_or(0.0);
                let dash = path
                    .stroke
                    .as_ref()
                    .filter(|stroke| !stroke.dash.is_empty())
                    .map(|stroke| {
                        format!(
                            " stroke-dasharray=\"{}\"",
                            stroke
                                .dash
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join(" ")
                        )
                    })
                    .unwrap_or_default();
                out.push_str(&format!(
                    r#"<path d="{}" fill="{}" stroke="{}" stroke-width="{}"{}/>"#,
                    d, fill, stroke, width, dash
                ));
            }
            Item::Image(image) => {
                let encoded = base64::engine::general_purpose::STANDARD.encode(&image.data.bytes);
                out.push_str(&format!(
                    r#"<image x="{}" y="{}" width="{}" height="{}" href="data:{};base64,{}"/>"#,
                    image.rect.x,
                    image.rect.y,
                    image.rect.width,
                    image.rect.height,
                    image.data.mime,
                    encoded
                ));
            }
            Item::Group(group) => {
                if let Some(clip) = group.clip {
                    let id = *clip_id;
                    *clip_id = clip_id.saturating_add(1);
                    out.push_str(&format!(
                        r#"<defs><clipPath id="clip-{id}"><rect x="{}" y="{}" width="{}" height="{}"/></clipPath></defs><g clip-path="url(#clip-{id})">"#,
                        clip.x, clip.y, clip.width, clip.height
                    ));
                } else {
                    out.push_str("<g>");
                }
                svg_items(out, &group.items, precision, clip_id);
                out.push_str("</g>");
            }
        }
    }
}

fn css(c: Colour) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_group_clips_constrain_raster_painting() {
        let red = Item::Path(PathItem {
            path: rect_path(Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }),
            fill: Some(Paint {
                colour: Colour {
                    r: 255,
                    g: 0,
                    b: 0,
                    a: 255,
                },
            }),
            stroke: None,
            source: None,
        });
        let page = Page {
            size: Size {
                width: 10.0,
                height: 10.0,
            },
            label: None,
            items: vec![Item::Group(Group {
                items: vec![Item::Group(Group {
                    items: vec![red],
                    clip: Some(Rect {
                        x: 3.0,
                        y: 3.0,
                        width: 6.0,
                        height: 6.0,
                    }),
                    source: None,
                })],
                clip: Some(Rect {
                    x: 1.0,
                    y: 1.0,
                    width: 5.0,
                    height: 5.0,
                }),
                source: None,
            })],
            source: None,
            frozen: None,
            grid: None,
        };

        let pixmap = rasterise(&page, 1.0)
            .unwrap_or_else(|error| panic!("the synthetic page rasterises: {error}"));
        let pixel = |x: usize, y: usize| &pixmap.data[(y * 10 + x) * 4..(y * 10 + x) * 4 + 4];
        assert_eq!(pixel(4, 4), [255, 0, 0, 255]);
        assert_eq!(pixel(2, 2), [255, 255, 255, 255]);
        assert_eq!(pixel(7, 7), [255, 255, 255, 255]);
    }
}
