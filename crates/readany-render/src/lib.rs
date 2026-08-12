#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

//! Deterministic, inspectable display lists for common document formats.

mod container;
mod error;
mod flow;
mod image_format;
mod limits;
pub mod model;
mod raster;
mod sheet;
mod slides;
mod sniff;
mod text;

pub use error::{RenderError, RenderErrorCode};
pub use limits::Limits;
pub use model::*;
pub use raster::{Pixmap, SvgOptions, items_in_rect, rasterise, rasterise_rect, to_svg};
pub use sniff::Format;
pub use text::{FontSource, OwnedFont};

/// Selects a contiguous inclusive range of zero-based pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageRange {
    pub first: u32,
    pub last: u32,
}

/// Rendering policy. Every ceiling has a finite default.
#[derive(Clone, Debug, Default)]
pub struct Options<'a> {
    pub filename: Option<&'a str>,
    pub limits: Limits,
    pub fonts: FontSource<'a>,
    pub only: Option<PageRange>,
    pub strict: bool,
}

/// Turns bytes into a deterministic display list. Content is never fetched.
pub fn render(bytes: &[u8], options: &Options<'_>) -> Result<Rendered, RenderError> {
    if options.limits.xml_entity_expansions != 0 {
        return Err(RenderError::invalid_options(
            "XML entity expansion must remain zero because document entities are never resolved",
        ));
    }
    options.limits.check_input(bytes.len())?;
    let format = sniff::sniff(bytes, options.filename)?;
    if matches!(options.fonts, FontSource::None)
        && !matches!(
            format,
            Format::Pdf
                | Format::Png
                | Format::Jpeg
                | Format::Gif
                | Format::Bmp
                | Format::Webp
                | Format::Heic
        )
    {
        return Err(RenderError::new(
            RenderErrorCode::NoFonts,
            "no fonts were supplied; add a font byte source and render again",
        ));
    }
    if let FontSource::Borrowed(fonts) = &options.fonts {
        for font in *fonts {
            skrifa::FontRef::new(&font.bytes).map_err(|_| {
                RenderError::malformed(format!(
                    "the supplied font {} is malformed; choose a valid OpenType font",
                    font.family
                ))
            })?;
        }
    }
    text::begin_render(&options.fonts);
    let mut rendered = match format {
        Format::Csv | Format::Tsv => sheet::csv::render(bytes, format, options)?,
        Format::Xlsx | Format::Xlsm => sheet::xlsx::render(bytes, format, options)?,
        Format::Ods => sheet::ods::render(bytes, options)?,
        Format::Docx => flow::docx::render(bytes, options)?,
        Format::Odt => flow::odt::render(bytes, options)?,
        Format::Rtf => flow::rtf::render(bytes, options)?,
        Format::Pptx => slides::pptx::render(bytes, options)?,
        Format::Odp => slides::odp::render(bytes, options)?,
        Format::Pdf => Rendered::delegated(Format::Pdf),
        Format::Png | Format::Jpeg | Format::Gif | Format::Bmp | Format::Webp => {
            image_format::render(bytes, format, options)?
        }
        Format::Heic => Rendered::delegated(Format::Heic),
    };
    rendered
        .meta
        .substituted_fonts
        .extend(text::take_substitutions());
    rendered.format = format;
    if let Some(range) = options.only {
        if range.first > range.last {
            return Err(RenderError::invalid_options(
                "page range starts after it ends",
            ));
        }
        rendered.pages = rendered
            .pages
            .into_iter()
            .enumerate()
            .filter_map(|(index, page)| {
                let index = u32::try_from(index).ok()?;
                (index >= range.first && index <= range.last).then_some(page)
            })
            .collect();
    }
    report_unsupported_glyphs(&mut rendered);
    enforce_glyph_limits(&mut rendered, options.limits.glyphs_per_page);
    if options.strict && !rendered.unrendered.is_empty() {
        return Err(RenderError::strict(rendered.unrendered));
    }
    Ok(rendered)
}

/// Direct parser entry points used only by cargo-fuzz so container and XML
/// defences remain reachable even when format sniffing rejects a mutation.
#[cfg(feature = "fuzzing")]
#[doc(hidden)]
pub mod fuzzing {
    use crate::{Limits, RenderError, container};

    pub fn zip(bytes: &[u8]) -> Result<(), RenderError> {
        container::zip::Archive::open(bytes, &Limits::default()).map(|_| ())
    }

    pub fn xml(bytes: &[u8]) -> Result<(), RenderError> {
        container::xml::validate(bytes, &Limits::default())
    }
}

fn report_unsupported_glyphs(rendered: &mut Rendered) {
    let count = rendered
        .pages
        .iter()
        .flat_map(|page| &page.items)
        .map(count_missing_glyphs)
        .sum::<u64>();
    if count > 0 {
        rendered.unrendered.push(Unrendered::UnsupportedGlyphs {
            script: "unsupported Unicode script or symbol".into(),
            count: u32::try_from(count).unwrap_or(u32::MAX),
        });
    }
}

fn count_missing_glyphs(item: &Item) -> u64 {
    match item {
        Item::Glyphs(run) => run
            .glyphs
            .iter()
            .filter(|glyph| glyph.glyph_id == 0)
            .count() as u64,
        Item::Group(group) => group.items.iter().map(count_missing_glyphs).sum(),
        Item::Path(_) | Item::Image(_) => 0,
    }
}

fn enforce_glyph_limits(rendered: &mut Rendered, limit: u64) {
    for page in &mut rendered.pages {
        let total = page.items.iter().map(count_glyphs).sum::<u64>();
        if total <= limit {
            continue;
        }
        let mut remaining = limit;
        retain_within_glyph_limit(&mut page.items, &mut remaining);
        rendered.unrendered.push(Unrendered::Truncated {
            limit: "glyphs_per_page".into(),
            of: total,
        });
    }
}

fn count_glyphs(item: &Item) -> u64 {
    match item {
        Item::Glyphs(run) => run.glyphs.len() as u64,
        Item::Group(group) => group.items.iter().map(count_glyphs).sum(),
        Item::Path(_) | Item::Image(_) => 0,
    }
}

fn retain_within_glyph_limit(items: &mut Vec<Item>, remaining: &mut u64) {
    items.retain_mut(|item| match item {
        Item::Glyphs(run) => {
            let count = run.glyphs.len() as u64;
            if count <= *remaining {
                *remaining -= count;
                true
            } else {
                false
            }
        }
        Item::Group(group) => {
            retain_within_glyph_limit(&mut group.items, remaining);
            !group.items.is_empty()
        }
        Item::Path(_) | Item::Image(_) => true,
    });
}
