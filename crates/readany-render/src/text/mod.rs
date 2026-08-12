use crate::model::{Colour, FontId, GlyphRun, Point, PositionedGlyph, SourceRef};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
};

#[derive(Clone, Debug)]
pub struct OwnedFont {
    pub family: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
pub enum FontSource<'a> {
    Bundled,
    None,
    Borrowed(&'a [OwnedFont]),
}

#[allow(clippy::derivable_impls)]
impl Default for FontSource<'_> {
    fn default() -> Self {
        #[cfg(feature = "fonts")]
        {
            Self::Bundled
        }
        #[cfg(not(feature = "fonts"))]
        {
            Self::None
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct TextStyle {
    pub family: String,
    pub size_px: f32,
    pub colour: Option<Colour>,
    pub bold: bool,
    pub italic: bool,
    pub rotation_deg: f32,
}

thread_local! {
    static ACTIVE_FONTS: RefCell<Vec<ActiveFont>> = const { RefCell::new(Vec::new()) };
    static SUBSTITUTIONS: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
    static SHAPE_CACHE: RefCell<BTreeMap<ShapeKey, CachedShape>> = const { RefCell::new(BTreeMap::new()) };
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ShapeKey {
    text: String,
    family: String,
    size_bits: u32,
    bold: bool,
    italic: bool,
}

#[derive(Clone)]
struct CachedShape {
    font: FontId,
    family: String,
    glyphs: Vec<PositionedGlyph>,
}

#[derive(Clone)]
struct ActiveFont {
    family: String,
    bytes: Arc<[u8]>,
}

static FONT_REGISTRY: OnceLock<Mutex<BTreeMap<u32, Arc<[u8]>>>> = OnceLock::new();

#[cfg(feature = "fonts")]
const CARLITO: &[u8] = include_bytes!("../../fonts/Carlito/Carlito-Regular.ttf");
#[cfg(feature = "fonts")]
const CARLITO_BOLD: &[u8] = include_bytes!("../../fonts/Carlito/Carlito-Bold.ttf");
#[cfg(feature = "fonts")]
const CARLITO_ITALIC: &[u8] = include_bytes!("../../fonts/Carlito/Carlito-Italic.ttf");
#[cfg(feature = "fonts")]
const CARLITO_BOLD_ITALIC: &[u8] = include_bytes!("../../fonts/Carlito/Carlito-BoldItalic.ttf");
#[cfg(feature = "fonts")]
const CALADEA: &[u8] = include_bytes!("../../fonts/Caladea/Caladea-Regular.ttf");
#[cfg(feature = "fonts")]
const CALADEA_BOLD: &[u8] = include_bytes!("../../fonts/Caladea/Caladea-Bold.ttf");
#[cfg(feature = "fonts")]
const CALADEA_ITALIC: &[u8] = include_bytes!("../../fonts/Caladea/Caladea-Italic.ttf");
#[cfg(feature = "fonts")]
const CALADEA_BOLD_ITALIC: &[u8] = include_bytes!("../../fonts/Caladea/Caladea-BoldItalic.ttf");
#[cfg(feature = "fonts")]
const LIBERATION_SANS: &[u8] = include_bytes!("../../fonts/Liberation/LiberationSans-Regular.ttf");
#[cfg(feature = "fonts")]
const LIBERATION_SANS_BOLD: &[u8] =
    include_bytes!("../../fonts/Liberation/LiberationSans-Bold.ttf");
#[cfg(feature = "fonts")]
const LIBERATION_SANS_ITALIC: &[u8] =
    include_bytes!("../../fonts/Liberation/LiberationSans-Italic.ttf");
#[cfg(feature = "fonts")]
const LIBERATION_SANS_BOLD_ITALIC: &[u8] =
    include_bytes!("../../fonts/Liberation/LiberationSans-BoldItalic.ttf");
#[cfg(feature = "fonts")]
const LIBERATION_SERIF: &[u8] =
    include_bytes!("../../fonts/Liberation/LiberationSerif-Regular.ttf");
#[cfg(feature = "fonts")]
const LIBERATION_SERIF_BOLD: &[u8] =
    include_bytes!("../../fonts/Liberation/LiberationSerif-Bold.ttf");
#[cfg(feature = "fonts")]
const LIBERATION_SERIF_ITALIC: &[u8] =
    include_bytes!("../../fonts/Liberation/LiberationSerif-Italic.ttf");
#[cfg(feature = "fonts")]
const LIBERATION_SERIF_BOLD_ITALIC: &[u8] =
    include_bytes!("../../fonts/Liberation/LiberationSerif-BoldItalic.ttf");
#[cfg(feature = "fonts")]
const LIBERATION_MONO: &[u8] = include_bytes!("../../fonts/Liberation/LiberationMono-Regular.ttf");
#[cfg(feature = "fonts")]
const LIBERATION_MONO_BOLD: &[u8] =
    include_bytes!("../../fonts/Liberation/LiberationMono-Bold.ttf");
#[cfg(feature = "fonts")]
const LIBERATION_MONO_ITALIC: &[u8] =
    include_bytes!("../../fonts/Liberation/LiberationMono-Italic.ttf");
#[cfg(feature = "fonts")]
const LIBERATION_MONO_BOLD_ITALIC: &[u8] =
    include_bytes!("../../fonts/Liberation/LiberationMono-BoldItalic.ttf");
#[cfg(feature = "fonts")]
const DEJAVU_SANS: &[u8] = include_bytes!("../../fonts/DejaVu/DejaVuSans.ttf");

pub(crate) fn begin_render(source: &FontSource<'_>) {
    ACTIVE_FONTS.with(|active| {
        let mut active = active.borrow_mut();
        active.clear();
        if let FontSource::Borrowed(fonts) = source {
            active.extend(fonts.iter().map(|font| ActiveFont {
                family: font.family.clone(),
                bytes: Arc::from(font.bytes.clone()),
            }));
        }
    });
    SUBSTITUTIONS.with(|substitutions| substitutions.borrow_mut().clear());
    SHAPE_CACHE.with(|cache| cache.borrow_mut().clear());
}

pub(crate) fn take_substitutions() -> BTreeMap<String, String> {
    SUBSTITUTIONS.with(|substitutions| std::mem::take(&mut *substitutions.borrow_mut()))
}

pub(crate) fn font_bytes(id: FontId) -> Option<Arc<[u8]>> {
    FONT_REGISTRY
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .ok()?
        .get(&id.0)
        .cloned()
}

pub(crate) fn shape(
    text: &str,
    style: &TextStyle,
    origin: Point,
    source: Option<SourceRef>,
) -> GlyphRun {
    let size = if style.size_px > 0.0 {
        style.size_px
    } else {
        14.666_667
    };
    let requested = if style.family.trim().is_empty() {
        "Calibri"
    } else {
        style.family.trim()
    };
    let key = ShapeKey {
        text: text.to_owned(),
        family: requested.to_ascii_lowercase(),
        size_bits: size.to_bits(),
        bold: style.bold,
        italic: style.italic,
    };
    let cacheable = !text
        .chars()
        .all(|character| character.is_ascii_digit() || ".,()-+% E".contains(character));
    if let Some(cached) = cacheable
        .then(|| SHAPE_CACHE.with(|cache| cache.borrow().get(&key).cloned()))
        .flatten()
    {
        return GlyphRun {
            font: cached.font,
            family: cached.family,
            size_px: size,
            origin,
            glyphs: cached.glyphs,
            text: text.into(),
            colour: style.colour.unwrap_or(Colour::BLACK),
            rotation_deg: style.rotation_deg,
            source,
        };
    }
    if let Some((font, family, id)) = select_font(requested, text, style.bold, style.italic) {
        if !family.eq_ignore_ascii_case(requested) {
            SUBSTITUTIONS.with(|substitutions| {
                substitutions
                    .borrow_mut()
                    .insert(requested.to_owned(), family.clone());
            });
        }
        if let Some(glyphs) = shape_with_font(&font, text, size) {
            if cacheable {
                SHAPE_CACHE.with(|cache| {
                    cache.borrow_mut().insert(
                        key.clone(),
                        CachedShape {
                            font: id,
                            family: family.clone(),
                            glyphs: glyphs.clone(),
                        },
                    );
                });
            }
            return GlyphRun {
                font: id,
                family,
                size_px: size,
                origin,
                glyphs,
                text: text.into(),
                colour: style.colour.unwrap_or(Colour::BLACK),
                rotation_deg: style.rotation_deg,
                source,
            };
        }
    }

    // A no-font build never reaches this through the public API, but keeping
    // the fallback total makes malformed font fuzzing non-panicking.
    let glyphs: Vec<PositionedGlyph> = text
        .char_indices()
        .map(|(offset, character)| PositionedGlyph {
            glyph_id: 0,
            x_advance: size
                * if character.is_whitespace() {
                    0.33
                } else {
                    0.55
                },
            x_offset: 0.0,
            y_offset: 0.0,
            cluster: u32::try_from(offset).unwrap_or(u32::MAX),
        })
        .collect();
    if cacheable {
        SHAPE_CACHE.with(|cache| {
            cache.borrow_mut().insert(
                key,
                CachedShape {
                    font: FontId(0),
                    family: requested.into(),
                    glyphs: glyphs.clone(),
                },
            );
        });
    }
    GlyphRun {
        font: FontId(0),
        family: requested.into(),
        size_px: size,
        origin,
        glyphs,
        text: text.into(),
        colour: style.colour.unwrap_or(Colour::BLACK),
        rotation_deg: style.rotation_deg,
        source,
    }
}

#[cfg_attr(not(feature = "fonts"), allow(unused_variables))]
fn select_font(
    requested: &str,
    text: &str,
    bold: bool,
    italic: bool,
) -> Option<(Arc<[u8]>, String, FontId)> {
    if let Some(font) = ACTIVE_FONTS.with(|fonts| {
        fonts
            .borrow()
            .iter()
            .find(|font| font.family.eq_ignore_ascii_case(requested))
            .cloned()
    }) {
        if text.is_ascii() || font_covers(&font.bytes, text) {
            let id = register_font(&font.family, font.bytes.clone());
            return Some((font.bytes, font.family, id));
        }
    }

    #[cfg(feature = "fonts")]
    {
        let lower = requested.to_ascii_lowercase();
        let face = usize::from(bold) * 2 + usize::from(italic);
        let (bytes, family, stable_id) = match lower.as_str() {
            "calibri" | "carlito" => (
                font_face(
                    [CARLITO, CARLITO_ITALIC, CARLITO_BOLD, CARLITO_BOLD_ITALIC],
                    face,
                ),
                "Carlito",
                1 + face as u32 * 10,
            ),
            "cambria" | "caladea" => (
                font_face(
                    [CALADEA, CALADEA_ITALIC, CALADEA_BOLD, CALADEA_BOLD_ITALIC],
                    face,
                ),
                "Caladea",
                2 + face as u32 * 10,
            ),
            "arial" | "helvetica" | "liberation sans" => (
                font_face(
                    [
                        LIBERATION_SANS,
                        LIBERATION_SANS_ITALIC,
                        LIBERATION_SANS_BOLD,
                        LIBERATION_SANS_BOLD_ITALIC,
                    ],
                    face,
                ),
                "Liberation Sans",
                3 + face as u32 * 10,
            ),
            "times new roman" | "times" | "liberation serif" => (
                font_face(
                    [
                        LIBERATION_SERIF,
                        LIBERATION_SERIF_ITALIC,
                        LIBERATION_SERIF_BOLD,
                        LIBERATION_SERIF_BOLD_ITALIC,
                    ],
                    face,
                ),
                "Liberation Serif",
                4 + face as u32 * 10,
            ),
            "courier new" | "courier" | "liberation mono" => (
                font_face(
                    [
                        LIBERATION_MONO,
                        LIBERATION_MONO_ITALIC,
                        LIBERATION_MONO_BOLD,
                        LIBERATION_MONO_BOLD_ITALIC,
                    ],
                    face,
                ),
                "Liberation Mono",
                5 + face as u32 * 10,
            ),
            "dejavu sans" => (DEJAVU_SANS, "DejaVu Sans", 6),
            _ => (DEJAVU_SANS, "DejaVu Sans", 6),
        };
        let (bytes, family, stable_id) = if text.is_ascii() || font_covers(bytes, text) {
            (bytes, family, stable_id)
        } else {
            (DEJAVU_SANS, "DejaVu Sans", 6)
        };
        let bytes = registered_or_insert(stable_id, bytes);
        Some((bytes, family.into(), FontId(stable_id)))
    }
    #[cfg(not(feature = "fonts"))]
    {
        let fallback = ACTIVE_FONTS.with(|fonts| {
            fonts
                .borrow()
                .iter()
                .find(|font| font_covers(&font.bytes, text))
                .cloned()
        })?;
        let id = register_font(&fallback.family, fallback.bytes.clone());
        Some((fallback.bytes, fallback.family, id))
    }
}

#[cfg(feature = "fonts")]
fn font_face(faces: [&'static [u8]; 4], face: usize) -> &'static [u8] {
    faces.get(face).copied().unwrap_or(faces[0])
}

fn font_covers(bytes: &[u8], text: &str) -> bool {
    use skrifa::MetadataProvider;
    skrifa::FontRef::new(bytes).ok().is_some_and(|font| {
        let charmap = font.charmap();
        text.chars()
            .filter(|character| !character.is_control())
            .all(|character| charmap.map(character).is_some())
    })
}

fn register_font(family: &str, bytes: Arc<[u8]>) -> FontId {
    let mut hash = 2_166_136_261_u32;
    for byte in family.as_bytes().iter().chain(bytes.iter()) {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    // IDs below 100 are reserved for committed bundled family/style faces.
    let id = hash.max(100);
    register_with_id(id, bytes);
    FontId(id)
}

fn register_with_id(id: u32, bytes: Arc<[u8]>) {
    if let Ok(mut registry) = FONT_REGISTRY
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
    {
        registry.entry(id).or_insert(bytes);
    }
}

#[cfg(feature = "fonts")]
fn registered_or_insert(id: u32, bytes: &'static [u8]) -> Arc<[u8]> {
    if let Ok(mut registry) = FONT_REGISTRY
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
    {
        return registry
            .entry(id)
            .or_insert_with(|| Arc::from(bytes))
            .clone();
    }
    Arc::from(bytes)
}

fn shape_with_font(bytes: &[u8], text: &str, size: f32) -> Option<Vec<PositionedGlyph>> {
    use skrifa::{
        MetadataProvider,
        instance::{LocationRef, Size},
    };
    let metrics_font = skrifa::FontRef::new(bytes).ok()?;
    let metrics = metrics_font.metrics(Size::unscaled(), LocationRef::default());
    let units = f32::from(metrics.units_per_em);
    if text
        .chars()
        .all(|character| character.is_ascii_digit() || ".,()-+% E".contains(character))
    {
        let charmap = metrics_font.charmap();
        let glyph_metrics = metrics_font.glyph_metrics(Size::unscaled(), LocationRef::default());
        return Some(
            text.char_indices()
                .map(|(cluster, character)| {
                    let glyph = charmap.map(character).unwrap_or_default();
                    PositionedGlyph {
                        glyph_id: glyph.to_u32(),
                        x_advance: glyph_metrics.advance_width(glyph).unwrap_or(0.0) / units * size,
                        x_offset: 0.0,
                        y_offset: 0.0,
                        cluster: u32::try_from(cluster).unwrap_or(u32::MAX),
                    }
                })
                .collect(),
        );
    }
    let font = harfrust::FontRef::new(bytes).ok()?;
    let data = harfrust::ShaperData::new(&font);
    let shaper = data.shaper(&font).build();
    let mut buffer = harfrust::UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.guess_segment_properties();
    let shaped = shaper.shape(buffer, harfrust::ShapeOptions::default());
    Some(
        shaped
            .glyph_infos()
            .iter()
            .zip(shaped.glyph_positions())
            .map(|(info, position)| PositionedGlyph {
                glyph_id: info.glyph_id,
                x_advance: position.x_advance as f32 / units * size,
                x_offset: position.x_offset as f32 / units * size,
                y_offset: -(position.y_offset as f32) / units * size,
                cluster: info.cluster,
            })
            .collect(),
    )
}

pub(crate) fn measure(text: &str, style: &TextStyle) -> f32 {
    shape(text, style, Point::default(), None)
        .glyphs
        .iter()
        .map(|glyph| glyph.x_advance)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "fonts")]
    fn microsoft_families_use_the_documented_metric_substitutes() {
        begin_render(&FontSource::Bundled);
        for (requested, expected) in [
            ("Calibri", "Carlito"),
            ("Cambria", "Caladea"),
            ("Arial", "Liberation Sans"),
            ("Times New Roman", "Liberation Serif"),
            ("Courier New", "Liberation Mono"),
        ] {
            let run = shape(
                "Metric test",
                &TextStyle {
                    family: requested.into(),
                    size_px: 12.0,
                    ..TextStyle::default()
                },
                Point::default(),
                None,
            );
            assert_eq!(run.family, expected);
            assert!(run.glyphs.iter().all(|glyph| glyph.glyph_id != 0));
        }
    }

    #[test]
    #[cfg(feature = "fonts")]
    fn complex_text_is_shaped_with_real_clusters() {
        begin_render(&FontSource::Bundled);
        let run = shape(
            "office",
            &TextStyle {
                family: "Calibri".into(),
                size_px: 12.0,
                ..TextStyle::default()
            },
            Point::default(),
            None,
        );
        assert!(run.glyphs.len() < "office".chars().count());
    }
}
