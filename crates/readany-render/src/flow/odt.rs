use crate::container::{resolve_relationship, xml, zip::Archive};
use crate::flow::{
    Alignment, FlowParagraph, FlowRun, ParagraphStyle, default_text_style, layout_flow,
};
use crate::model::{Colour, ImageData, ImageItem, Item, Rect, Size, SourceRef};
use crate::text::TextStyle;
use crate::{Format, Options, RenderError};
use quick_xml::events::{BytesStart, Event};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn render(bytes: &[u8], options: &Options<'_>) -> Result<crate::Rendered, RenderError> {
    let archive = Archive::open(bytes, &options.limits)?;
    let content = archive.required("content.xml")?;
    xml::validate(content, &options.limits)?;
    if let Some(styles) = archive.get("styles.xml") {
        xml::validate(styles, &options.limits)?;
    }
    let registry = Styles::parse(archive.get("styles.xml"), content)?;
    let parsed = parse_content(content, &registry)?;
    let mut rendered = layout_flow(
        &parsed.paragraphs,
        Format::Odt,
        options,
        registry.page_size,
        registry.margins,
    )?;
    paint_images(
        &archive,
        &parsed.images,
        &mut rendered,
        options.limits.image_pixels,
    )?;
    Ok(rendered)
}

#[derive(Clone, Default)]
struct StylePatch {
    family: Option<String>,
    size_px: Option<f32>,
    colour: Option<Colour>,
    bold: Option<bool>,
    italic: Option<bool>,
    alignment: Option<Alignment>,
    left: Option<f32>,
    right: Option<f32>,
    first_line: Option<f32>,
    before: Option<f32>,
    after: Option<f32>,
    line_height: Option<f32>,
    keep_next: Option<bool>,
    page_break_before: Option<bool>,
}

impl StylePatch {
    fn overlay(&mut self, child: &Self) {
        macro_rules! overlay {
            ($field:ident) => {
                if child.$field.is_some() {
                    self.$field.clone_from(&child.$field);
                }
            };
        }
        overlay!(family);
        overlay!(size_px);
        overlay!(colour);
        overlay!(bold);
        overlay!(italic);
        overlay!(alignment);
        overlay!(left);
        overlay!(right);
        overlay!(first_line);
        overlay!(before);
        overlay!(after);
        overlay!(line_height);
        overlay!(keep_next);
        overlay!(page_break_before);
    }

    fn apply_text(&self, text: &mut TextStyle) {
        if let Some(family) = &self.family {
            text.family.clone_from(family);
        }
        text.size_px = self.size_px.unwrap_or(text.size_px);
        text.colour = self.colour.or(text.colour);
        text.bold = self.bold.unwrap_or(text.bold);
        text.italic = self.italic.unwrap_or(text.italic);
    }

    fn apply_paragraph(&self, paragraph: &mut ParagraphStyle) {
        paragraph.alignment = self.alignment.unwrap_or(paragraph.alignment);
        paragraph.left = self.left.unwrap_or(paragraph.left);
        paragraph.right = self.right.unwrap_or(paragraph.right);
        paragraph.first_line = self.first_line.unwrap_or(paragraph.first_line);
        paragraph.before = self.before.unwrap_or(paragraph.before);
        paragraph.after = self.after.unwrap_or(paragraph.after);
        paragraph.line_height = self.line_height.or(paragraph.line_height);
        paragraph.keep_next = self.keep_next.unwrap_or(paragraph.keep_next);
        paragraph.page_break_before = self
            .page_break_before
            .unwrap_or(paragraph.page_break_before);
    }
}

#[derive(Clone, Default)]
struct StyleDef {
    parent: Option<String>,
    patch: StylePatch,
}

struct Styles {
    default_paragraph: StylePatch,
    has_default_paragraph: bool,
    definitions: BTreeMap<String, StyleDef>,
    page_size: Size,
    margins: (f32, f32, f32, f32),
}

impl Styles {
    fn parse(styles: Option<&[u8]>, content: &[u8]) -> Result<Self, RenderError> {
        let mut output = Self {
            default_paragraph: StylePatch::default(),
            has_default_paragraph: false,
            definitions: BTreeMap::new(),
            page_size: Size {
                width: 793.7008,
                height: 1122.5197,
            },
            margins: (75.59055, 75.59055, 75.59055, 75.59055),
        };
        if let Some(styles) = styles {
            output.parse_part(styles)?;
        }
        output.parse_part(content)?;
        Ok(output)
    }

    fn parse_part(&mut self, bytes: &[u8]) -> Result<(), RenderError> {
        let mut reader = quick_xml::Reader::from_reader(bytes);
        let mut id = None::<String>;
        let mut default_paragraph = false;
        let mut definition = StyleDef::default();
        loop {
            match reader.read_event() {
                Ok(Event::Start(start)) => {
                    if xml::local_name(start.name().as_ref()) == b"style" {
                        id = attr(&start, b"name");
                        default_paragraph = false;
                        definition = StyleDef {
                            parent: attr(&start, b"parent-style-name"),
                            patch: StylePatch::default(),
                        };
                    } else if xml::local_name(start.name().as_ref()) == b"default-style"
                        && attr(&start, b"family").as_deref() == Some("paragraph")
                    {
                        id = None;
                        default_paragraph = true;
                        definition = StyleDef::default();
                    } else {
                        apply_odf_property(
                            &start,
                            (id.is_some() || default_paragraph).then_some(&mut definition.patch),
                            &mut self.page_size,
                            &mut self.margins,
                        );
                    }
                }
                Ok(Event::Empty(start)) => apply_odf_property(
                    &start,
                    (id.is_some() || default_paragraph).then_some(&mut definition.patch),
                    &mut self.page_size,
                    &mut self.margins,
                ),
                Ok(Event::End(end)) if xml::local_name(end.name().as_ref()) == b"style" => {
                    if let Some(id) = id.take() {
                        self.definitions.insert(id, std::mem::take(&mut definition));
                    }
                }
                Ok(Event::End(end))
                    if default_paragraph
                        && xml::local_name(end.name().as_ref()) == b"default-style" =>
                {
                    self.default_paragraph = std::mem::take(&mut definition.patch);
                    self.has_default_paragraph = true;
                    default_paragraph = false;
                }
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(_) => {
                    return Err(RenderError::malformed(
                        "OpenDocument styles are malformed; obtain a fresh copy",
                    ));
                }
            }
        }
        Ok(())
    }

    fn resolve(&self, id: Option<&str>) -> (TextStyle, ParagraphStyle) {
        let mut patch = self.default_paragraph.clone();
        patch.overlay(&self.named_patch(id));
        let mut text = default_text_style();
        let mut paragraph = ParagraphStyle::default();
        if self.has_default_paragraph {
            paragraph.after = 0.0;
            // The UK agreement advances 11 pt Arial lines by 12.65 pt in the
            // LibreOffice reference; the generic 1.2 flow box is 13.2 pt.
            paragraph.line_height_multiplier = 1.15;
        }
        patch.apply_text(&mut text);
        patch.apply_paragraph(&mut paragraph);
        (text, paragraph)
    }

    fn apply_text_style(&self, id: Option<&str>, text: &mut TextStyle) {
        // A span is a delta on its paragraph's resolved text style.  Resolving
        // it from scratch reset the UK agreement's Arial paragraphs to the
        // renderer's Calibri default and changed every subsequent wrap.
        self.named_patch(id).apply_text(text);
    }

    fn named_patch(&self, id: Option<&str>) -> StylePatch {
        let mut patch = StylePatch::default();
        let mut chain = Vec::new();
        let mut cursor = id;
        let mut visited = BTreeSet::new();
        while let Some(id) = cursor {
            if !visited.insert(id.to_owned()) {
                break;
            }
            let Some(style) = self.definitions.get(id) else {
                break;
            };
            chain.push(style);
            cursor = style.parent.as_deref();
        }
        for style in chain.into_iter().rev() {
            patch.overlay(&style.patch);
        }
        patch
    }
}

fn apply_odf_property(
    start: &BytesStart<'_>,
    patch: Option<&mut StylePatch>,
    page_size: &mut Size,
    margins: &mut (f32, f32, f32, f32),
) {
    match xml::local_name(start.name().as_ref()) {
        b"page-layout-properties" => {
            page_size.width = attr(start, b"page-width")
                .as_deref()
                .and_then(length)
                .unwrap_or(page_size.width);
            page_size.height = attr(start, b"page-height")
                .as_deref()
                .and_then(length)
                .unwrap_or(page_size.height);
            let common = attr(start, b"margin").as_deref().and_then(length);
            margins.0 = attr(start, b"margin-left")
                .as_deref()
                .and_then(length)
                .or(common)
                .unwrap_or(margins.0);
            margins.1 = attr(start, b"margin-top")
                .as_deref()
                .and_then(length)
                .or(common)
                .unwrap_or(margins.1);
            margins.2 = attr(start, b"margin-right")
                .as_deref()
                .and_then(length)
                .or(common)
                .unwrap_or(margins.2);
            margins.3 = attr(start, b"margin-bottom")
                .as_deref()
                .and_then(length)
                .or(common)
                .unwrap_or(margins.3);
        }
        b"text-properties" => {
            let Some(patch) = patch else { return };
            patch.family = attr(start, b"font-name")
                .or_else(|| attr(start, b"font-family"))
                .map(|value| value.trim_matches(['\'', '"']).to_owned());
            patch.size_px = attr(start, b"font-size").as_deref().and_then(length);
            patch.colour = attr(start, b"color").as_deref().and_then(colour);
            patch.bold = attr(start, b"font-weight").map(|value| value == "bold");
            patch.italic = attr(start, b"font-style").map(|value| value == "italic");
        }
        b"paragraph-properties" => {
            let Some(patch) = patch else { return };
            patch.alignment = attr(start, b"text-align").map(|value| match value.as_str() {
                "center" => Alignment::Centre,
                "end" | "right" => Alignment::Right,
                "justify" => Alignment::Justify,
                _ => Alignment::Left,
            });
            patch.left = attr(start, b"margin-left").as_deref().and_then(length);
            patch.right = attr(start, b"margin-right").as_deref().and_then(length);
            patch.first_line = attr(start, b"text-indent").as_deref().and_then(length);
            patch.before = attr(start, b"margin-top").as_deref().and_then(length);
            patch.after = attr(start, b"margin-bottom").as_deref().and_then(length);
            patch.line_height = attr(start, b"line-height")
                .as_deref()
                .and_then(|value| (!value.ends_with('%')).then(|| length(value)).flatten());
            patch.keep_next = attr(start, b"keep-with-next").map(|value| value == "always");
            patch.page_break_before = attr(start, b"break-before").map(|value| value == "page");
        }
        _ => {}
    }
}

struct ParsedContent {
    paragraphs: Vec<FlowParagraph>,
    images: Vec<PendingImage>,
}

struct PendingImage {
    path: String,
    rect: Rect,
    paragraph: u32,
}

fn parse_content(bytes: &[u8], styles: &Styles) -> Result<ParsedContent, RenderError> {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut paragraphs = Vec::new();
    let mut runs = Vec::<FlowRun>::new();
    let mut text = String::new();
    let mut active = false;
    let mut paragraph_style = ParagraphStyle::default();
    let mut current_style = default_text_style();
    let mut style_stack = Vec::<TextStyle>::new();
    let mut list_depth = 0_u32;
    let mut prepend_list_label = false;
    let mut frame = None::<Rect>;
    let mut images = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => match xml::local_name(start.name().as_ref()) {
                b"p" | b"h" => {
                    active = true;
                    runs.clear();
                    text.clear();
                    let resolved = styles.resolve(attr(&start, b"style-name").as_deref());
                    current_style = resolved.0;
                    paragraph_style = resolved.1;
                    if xml::local_name(start.name().as_ref()) == b"h" {
                        current_style.bold = true;
                        if current_style.size_px <= 14.666_667 {
                            current_style.size_px = 20.0;
                        }
                        paragraph_style.keep_next = true;
                    }
                    prepend_list_label = list_depth > 0;
                }
                b"span" if active => {
                    flush_text(&mut runs, &mut text, &current_style);
                    style_stack.push(current_style.clone());
                    styles.apply_text_style(
                        attr(&start, b"style-name").as_deref(),
                        &mut current_style,
                    );
                }
                b"list" => list_depth = list_depth.saturating_add(1),
                b"frame" => {
                    frame = Some(Rect {
                        x: attr(&start, b"x")
                            .as_deref()
                            .and_then(length)
                            .unwrap_or(0.0),
                        y: attr(&start, b"y")
                            .as_deref()
                            .and_then(length)
                            .unwrap_or(0.0),
                        width: attr(&start, b"width")
                            .as_deref()
                            .and_then(length)
                            .unwrap_or(96.0),
                        height: attr(&start, b"height")
                            .as_deref()
                            .and_then(length)
                            .unwrap_or(96.0),
                    });
                }
                b"image" => {
                    if let (Some(rect), Some(path)) = (frame, attr(&start, b"href")) {
                        images.push(PendingImage {
                            path,
                            rect,
                            paragraph: paragraphs.len() as u32,
                        });
                    }
                }
                _ => {}
            },
            Ok(Event::Empty(start)) => match xml::local_name(start.name().as_ref()) {
                b"p" | b"h" => {
                    let (mut text_style, mut paragraph_style) =
                        styles.resolve(attr(&start, b"style-name").as_deref());
                    if xml::local_name(start.name().as_ref()) == b"h" {
                        text_style.bold = true;
                        if text_style.size_px <= 14.666_667 {
                            text_style.size_px = 20.0;
                        }
                        paragraph_style.keep_next = true;
                    }
                    paragraphs.push(FlowParagraph {
                        runs: vec![FlowRun {
                            text: String::new(),
                            style: text_style,
                        }],
                        style: paragraph_style,
                    });
                }
                b"tab" if active => text.push('\t'),
                b"line-break" if active => text.push('\n'),
                b"s" if active => {
                    let count = attr(&start, b"c")
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or(1)
                        .min(10_000);
                    text.extend(std::iter::repeat_n(' ', count));
                }
                b"page-number" if active => text.push('1'),
                b"page-count" if active => text.push('1'),
                b"image" if active => {
                    if let (Some(rect), Some(path)) = (frame, attr(&start, b"href")) {
                        images.push(PendingImage {
                            path,
                            rect,
                            paragraph: paragraphs.len() as u32,
                        });
                    }
                }
                _ => {}
            },
            Ok(Event::Text(value)) if active => text.push_str(&value.decode().map_err(|_| {
                RenderError::malformed("ODT text is malformed; obtain a fresh copy")
            })?),
            Ok(Event::GeneralRef(reference)) if active => {
                text.push_str(&xml::decode_reference(&reference)?)
            }
            Ok(Event::End(end)) => match xml::local_name(end.name().as_ref()) {
                b"span" if active => {
                    flush_text(&mut runs, &mut text, &current_style);
                    if let Some(style) = style_stack.pop() {
                        current_style = style;
                    }
                }
                b"p" | b"h" if active => {
                    flush_text(&mut runs, &mut text, &current_style);
                    if prepend_list_label {
                        runs.insert(
                            0,
                            FlowRun {
                                text: "•\t".into(),
                                style: current_style.clone(),
                            },
                        );
                        paragraph_style.left += list_depth as f32 * 24.0;
                        paragraph_style.first_line = -18.0;
                    }
                    if runs.is_empty() {
                        runs.push(FlowRun {
                            text: String::new(),
                            style: current_style.clone(),
                        });
                    }
                    paragraphs.push(FlowParagraph {
                        runs: std::mem::take(&mut runs),
                        style: paragraph_style.clone(),
                    });
                    active = false;
                }
                b"list" => list_depth = list_depth.saturating_sub(1),
                b"frame" => frame = None,
                _ => {}
            },
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => {
                return Err(RenderError::malformed(
                    "content.xml is malformed; obtain a fresh copy",
                ));
            }
        }
    }
    Ok(ParsedContent { paragraphs, images })
}

fn flush_text(runs: &mut Vec<FlowRun>, text: &mut String, style: &TextStyle) {
    if !text.is_empty() {
        runs.push(FlowRun {
            text: std::mem::take(text),
            style: style.clone(),
        });
    }
}

fn paint_images(
    archive: &Archive,
    images: &[PendingImage],
    rendered: &mut crate::Rendered,
    image_pixel_limit: u64,
) -> Result<(), RenderError> {
    for image in images {
        let path = resolve_relationship("content.xml", &image.path)?;
        let Some(bytes) = archive.get(&path) else {
            continue;
        };
        let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|_| RenderError::malformed("an embedded ODT image has a damaged header"))?;
        let (width, height) = reader
            .into_dimensions()
            .map_err(|_| RenderError::malformed("an embedded ODT image has damaged dimensions"))?;
        let pixels = u64::from(width)
            .checked_mul(u64::from(height))
            .ok_or_else(|| RenderError::limit("image_pixels", u64::MAX))?;
        if pixels > image_pixel_limit {
            return Err(RenderError::limit("image_pixels", pixels));
        }
        let Some(page) = rendered.pages.first_mut() else {
            continue;
        };
        page.items.push(Item::Image(ImageItem {
            data: ImageData {
                mime: mime_for(&path).into(),
                bytes: bytes.to_vec(),
                pixel_size: Size {
                    width: width as f32,
                    height: height as f32,
                },
            },
            rect: image.rect,
            source: Some(SourceRef::Text {
                paragraph: image.paragraph,
                start: 0,
                end: 0,
            }),
        }));
    }
    Ok(())
}

fn mime_for(path: &str) -> &'static str {
    match path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "webp" => "image/webp",
        _ => "image/png",
    }
}

fn attr(start: &BytesStart<'_>, name: &[u8]) -> Option<String> {
    start
        .attributes()
        .with_checks(false)
        .flatten()
        .find(|attribute| xml::local_name(attribute.key.as_ref()) == name)
        .map(|attribute| String::from_utf8_lossy(attribute.value.as_ref()).into_owned())
}

fn length(value: &str) -> Option<f32> {
    let split = value
        .find(|character: char| {
            !character.is_ascii_digit() && !matches!(character, '.' | '-' | '+')
        })
        .unwrap_or(value.len());
    let number = value.get(..split)?.parse::<f32>().ok()?;
    let unit = value.get(split..)?.trim();
    Some(match unit {
        "in" => number * 96.0,
        "cm" => number / 2.54 * 96.0,
        "mm" => number / 25.4 * 96.0,
        "pt" => number / 72.0 * 96.0,
        "pc" => number / 6.0 * 96.0,
        "px" | "" => number,
        _ => return None,
    })
}

fn colour(value: &str) -> Option<Colour> {
    let value = value.strip_prefix('#')?;
    if value.len() != 6 {
        return None;
    }
    Some(Colour {
        r: u8::from_str_radix(&value[0..2], 16).ok()?,
        g: u8::from_str_radix(&value[2..4], 16).ok()?,
        b: u8::from_str_radix(&value[4..6], 16).ok()?,
        a: 255,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn odf_lengths_convert_at_the_parser_boundary() {
        assert!((length("2.54cm").unwrap_or_default() - 96.0).abs() < 0.001);
        assert!((length("72pt").unwrap_or_default() - 96.0).abs() < 0.001);
    }
}
