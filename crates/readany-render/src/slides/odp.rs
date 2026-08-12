use crate::container::{resolve_relationship, xml, zip::Archive};
use crate::model::*;
use crate::text::{TextStyle, measure, shape};
use crate::{Format, Options, RenderError};
use quick_xml::events::{BytesStart, Event};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn render(bytes: &[u8], options: &Options<'_>) -> Result<Rendered, RenderError> {
    let archive = Archive::open(bytes, &options.limits)?;
    let content = archive.required("content.xml")?;
    xml::validate(content, &options.limits)?;
    let styles_bytes = archive.get("styles.xml");
    if let Some(styles) = styles_bytes {
        xml::validate(styles, &options.limits)?;
    }
    let styles = Styles::parse(styles_bytes, content)?;
    let master = styles_bytes
        .map(|bytes| parse_shapes(bytes, &styles, 0, true))
        .transpose()?
        .unwrap_or_default();
    let parsed = parse_pages(content, &styles, &master, options)?;
    let mut pages = parsed.pages;
    paint_images(
        &archive,
        parsed.images,
        &mut pages,
        options.limits.image_pixels,
    )?;
    Ok(Rendered {
        pages,
        format: Format::Odp,
        unrendered: parsed.unrendered,
        meta: Meta::default(),
    })
}

#[derive(Clone)]
struct StyleDef {
    parent: Option<String>,
    fill: Option<Colour>,
    no_fill: bool,
    stroke: Option<Colour>,
    stroke_width: f32,
    text: TextStyle,
}

impl Default for StyleDef {
    fn default() -> Self {
        Self {
            parent: None,
            fill: None,
            no_fill: false,
            stroke: None,
            stroke_width: 1.0,
            text: TextStyle {
                family: "Carlito".into(),
                size_px: 24.0,
                ..TextStyle::default()
            },
        }
    }
}

struct Styles {
    definitions: BTreeMap<String, StyleDef>,
    size: Size,
}

impl Styles {
    fn parse(styles: Option<&[u8]>, content: &[u8]) -> Result<Self, RenderError> {
        let mut output = Self {
            definitions: BTreeMap::new(),
            size: Size {
                // ODF presentations without an explicit page layout use the
                // Impress widescreen default (28 cm x 15.75 cm), not A4.
                width: 1_058.267_7,
                height: 595.275_6,
            },
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
        let mut style = StyleDef::default();
        loop {
            match reader.read_event() {
                Ok(Event::Start(start)) => {
                    if xml::local_name(start.name().as_ref()) == b"style" {
                        id = attr(&start, b"name");
                        style = StyleDef {
                            parent: attr(&start, b"parent-style-name"),
                            ..StyleDef::default()
                        };
                    } else {
                        apply_style_property(
                            &start,
                            id.as_ref().map(|_| &mut style),
                            &mut self.size,
                        );
                    }
                }
                Ok(Event::Empty(start)) => {
                    apply_style_property(&start, id.as_ref().map(|_| &mut style), &mut self.size)
                }
                Ok(Event::End(end)) if xml::local_name(end.name().as_ref()) == b"style" => {
                    if let Some(id) = id.take() {
                        self.definitions.insert(id, std::mem::take(&mut style));
                    }
                }
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(_) => return Err(RenderError::malformed("ODP styles are malformed")),
            }
        }
        Ok(())
    }

    fn resolve(&self, id: Option<&str>) -> StyleDef {
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
        let mut output = StyleDef::default();
        for style in chain.into_iter().rev() {
            if style.fill.is_some() {
                output.fill = style.fill;
            }
            output.no_fill = style.no_fill;
            if style.stroke.is_some() {
                output.stroke = style.stroke;
                output.stroke_width = style.stroke_width;
            }
            if style.text.family != "Carlito" {
                output.text.family.clone_from(&style.text.family);
            }
            if style.text.size_px != 24.0 {
                output.text.size_px = style.text.size_px;
            }
            output.text.colour = style.text.colour.or(output.text.colour);
            output.text.bold |= style.text.bold;
            output.text.italic |= style.text.italic;
        }
        output
    }
}

fn apply_style_property(start: &BytesStart<'_>, style: Option<&mut StyleDef>, size: &mut Size) {
    match xml::local_name(start.name().as_ref()) {
        b"page-layout-properties" => {
            size.width = attr(start, b"page-width")
                .as_deref()
                .and_then(parse_length)
                .unwrap_or(size.width);
            size.height = attr(start, b"page-height")
                .as_deref()
                .and_then(parse_length)
                .unwrap_or(size.height);
        }
        b"graphic-properties" => {
            let Some(style) = style else { return };
            style.no_fill = attr(start, b"fill").as_deref() == Some("none");
            style.fill = attr(start, b"fill-color").as_deref().and_then(colour);
            style.stroke = attr(start, b"stroke-color").as_deref().and_then(colour);
            style.stroke_width = attr(start, b"stroke-width")
                .as_deref()
                .and_then(parse_length)
                .unwrap_or(1.0);
        }
        b"text-properties" => {
            let Some(style) = style else { return };
            style.text.family = attr(start, b"font-name")
                .or_else(|| attr(start, b"font-family"))
                .map(|value| value.trim_matches(['\'', '"']).to_owned())
                .unwrap_or_else(|| style.text.family.clone());
            style.text.size_px = attr(start, b"font-size")
                .as_deref()
                .and_then(parse_length)
                .unwrap_or(style.text.size_px);
            style.text.colour = attr(start, b"color").as_deref().and_then(colour);
            style.text.bold = attr(start, b"font-weight").as_deref() == Some("bold");
            style.text.italic = attr(start, b"font-style").as_deref() == Some("italic");
        }
        _ => {}
    }
}

struct ParsedPages {
    pages: Vec<Page>,
    images: Vec<PendingImage>,
    unrendered: Vec<Unrendered>,
}

#[derive(Clone)]
struct PendingImage {
    page: usize,
    path: String,
    rect: Rect,
    source: SourceRef,
}

fn parse_pages(
    bytes: &[u8],
    styles: &Styles,
    master: &[Item],
    options: &Options<'_>,
) -> Result<ParsedPages, RenderError> {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut pages = Vec::new();
    let mut items = Vec::new();
    let mut images = Vec::new();
    let mut unrendered = Vec::new();
    let mut in_page = false;
    let mut shape = ShapeState::default();
    let mut in_text = false;
    let mut shape_index = 0_u32;
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) if xml::local_name(start.name().as_ref()) == b"page" => {
                in_page = true;
                items = master.to_vec();
                shape_index = 0;
            }
            Ok(Event::End(end)) if xml::local_name(end.name().as_ref()) == b"page" => {
                finish_shape(
                    &mut shape,
                    &mut items,
                    pages.len() as u32,
                    &mut shape_index,
                    styles,
                );
                pages.push(Page {
                    size: styles.size,
                    label: Some(format!("Slide {}", pages.len() + 1)),
                    items: std::mem::take(&mut items),
                    source: None,
                    frozen: None,
                    grid: None,
                });
                in_page = false;
            }
            Ok(Event::Start(start))
                if in_page && is_shape(xml::local_name(start.name().as_ref())) =>
            {
                finish_shape(
                    &mut shape,
                    &mut items,
                    pages.len() as u32,
                    &mut shape_index,
                    styles,
                );
                shape = ShapeState::from_start(&start, styles.size);
            }
            Ok(Event::End(end)) if in_page && is_shape(xml::local_name(end.name().as_ref())) => {
                finish_shape(
                    &mut shape,
                    &mut items,
                    pages.len() as u32,
                    &mut shape_index,
                    styles,
                );
            }
            Ok(Event::Start(start))
                if in_page && matches!(xml::local_name(start.name().as_ref()), b"p" | b"h") =>
            {
                if !shape.text.is_empty() {
                    shape.text.push('\n');
                }
                in_text = true;
            }
            Ok(Event::End(end)) if matches!(xml::local_name(end.name().as_ref()), b"p" | b"h") => {
                in_text = false
            }
            Ok(Event::Text(value)) if in_text => {
                shape.text.push_str(&value.decode().map_err(|_| {
                    RenderError::malformed("presentation text is malformed; obtain a fresh copy")
                })?)
            }
            Ok(Event::GeneralRef(reference)) if in_text => {
                shape.text.push_str(&xml::decode_reference(&reference)?)
            }
            Ok(Event::Empty(start)) | Ok(Event::Start(start))
                if in_page && xml::local_name(start.name().as_ref()) == b"image" =>
            {
                if let Some(path) = attr(&start, b"href") {
                    let source = SourceRef::Shape {
                        slide: pages.len() as u32,
                        shape: shape_index,
                    };
                    if path.contains("://") {
                        unrendered.push(Unrendered::ExternalReference { target: path });
                    } else {
                        images.push(PendingImage {
                            page: pages.len(),
                            path,
                            rect: shape.rect,
                            source,
                        });
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => {
                return Err(RenderError::malformed(
                    "content.xml is malformed; obtain a fresh copy",
                ));
            }
        }
    }
    if pages.len() as u32 > options.limits.pages {
        return Err(RenderError::limit("pages", pages.len() as u64));
    }
    Ok(ParsedPages {
        pages,
        images,
        unrendered,
    })
}

fn parse_shapes(
    bytes: &[u8],
    styles: &Styles,
    slide: u32,
    master: bool,
) -> Result<Vec<Item>, RenderError> {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut items = Vec::new();
    let mut shape = ShapeState::default();
    let mut in_text = false;
    let mut index = 0_u32;
    let mut in_master = !master;
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) if xml::local_name(start.name().as_ref()) == b"master-page" => {
                in_master = true
            }
            Ok(Event::End(end)) if xml::local_name(end.name().as_ref()) == b"master-page" => {
                in_master = false
            }
            Ok(Event::Start(start))
                if in_master && is_shape(xml::local_name(start.name().as_ref())) =>
            {
                finish_shape(&mut shape, &mut items, slide, &mut index, styles);
                shape = ShapeState::from_start(&start, styles.size);
            }
            Ok(Event::End(end)) if in_master && is_shape(xml::local_name(end.name().as_ref())) => {
                finish_shape(&mut shape, &mut items, slide, &mut index, styles)
            }
            Ok(Event::Start(start))
                if in_master && matches!(xml::local_name(start.name().as_ref()), b"p" | b"h") =>
            {
                in_text = true
            }
            Ok(Event::End(end)) if matches!(xml::local_name(end.name().as_ref()), b"p" | b"h") => {
                in_text = false
            }
            Ok(Event::Text(value)) if in_text => {
                shape.text.push_str(&value.decode().unwrap_or_default())
            }
            Ok(Event::GeneralRef(reference)) if in_text => {
                shape.text.push_str(&xml::decode_reference(&reference)?)
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err(RenderError::malformed("ODP master page is malformed")),
        }
    }
    Ok(items)
}

#[derive(Default)]
struct ShapeState {
    active: bool,
    kind: String,
    style: String,
    rect: Rect,
    text: String,
}

impl ShapeState {
    fn from_start(start: &BytesStart<'_>, size: Size) -> Self {
        Self {
            active: true,
            kind: String::from_utf8_lossy(xml::local_name(start.name().as_ref())).into_owned(),
            style: attr(start, b"style-name").unwrap_or_default(),
            rect: Rect {
                x: length(start, b"x").unwrap_or(48.0),
                y: length(start, b"y").unwrap_or(48.0),
                width: length(start, b"width").unwrap_or(size.width - 96.0),
                height: length(start, b"height").unwrap_or(80.0),
            },
            text: String::new(),
        }
    }
}

fn finish_shape(
    shape_state: &mut ShapeState,
    items: &mut Vec<Item>,
    slide: u32,
    index: &mut u32,
    styles: &Styles,
) {
    if !shape_state.active {
        return;
    }
    let source = SourceRef::Shape {
        slide,
        shape: *index,
    };
    *index = index.saturating_add(1);
    let style = styles.resolve(Some(&shape_state.style));
    let fill = if style.no_fill {
        None
    } else {
        Some(Paint {
            colour: style.fill.unwrap_or(Colour {
                r: 114,
                g: 159,
                b: 207,
                a: 255,
            }),
        })
    };
    let path = shape_path(&shape_state.kind, shape_state.rect);
    let mut shape_items = vec![Item::Path(PathItem {
        path,
        fill,
        stroke: style.stroke.map(|colour| Stroke {
            paint: Paint { colour },
            width: style.stroke_width,
            dash: Vec::new(),
        }),
        source: Some(source.clone()),
    })];
    let text = shape_state.text.trim();
    if !text.is_empty() {
        let mut text_style = style.text;
        let available = (shape_state.rect.width - 8.0).max(1.0);
        let measured = measure(text, &text_style);
        if measured > available {
            text_style.size_px = (text_style.size_px * available / measured).max(8.0);
        }
        shape_items.push(Item::Glyphs(shape(
            text,
            &text_style,
            Point {
                x: shape_state.rect.x + 4.0,
                y: shape_state.rect.y + text_style.size_px,
            },
            Some(source.clone()),
        )));
    }
    items.push(Item::Group(Group {
        items: shape_items,
        clip: Some(shape_state.rect),
        source: Some(source),
    }));
    *shape_state = ShapeState::default();
}

fn shape_path(kind: &str, rect: Rect) -> Path {
    if kind == "line" {
        return Path {
            commands: vec![
                PathCommand::Move(Point {
                    x: rect.x,
                    y: rect.y,
                }),
                PathCommand::Line(Point {
                    x: rect.x + rect.width,
                    y: rect.y + rect.height,
                }),
            ],
        };
    }
    rect_path(rect)
}

fn is_shape(name: &[u8]) -> bool {
    matches!(
        name,
        b"frame" | b"custom-shape" | b"rect" | b"ellipse" | b"line"
    )
}

fn paint_images(
    archive: &Archive,
    images: Vec<PendingImage>,
    pages: &mut [Page],
    image_pixel_limit: u64,
) -> Result<(), RenderError> {
    for image in images {
        let path = resolve_relationship("content.xml", &image.path)?;
        let Some(bytes) = archive.get(&path) else {
            continue;
        };
        let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|_| RenderError::malformed("an embedded ODP image has a damaged header"))?;
        let (width, height) = reader
            .into_dimensions()
            .map_err(|_| RenderError::malformed("an embedded ODP image has damaged dimensions"))?;
        let pixels = u64::from(width)
            .checked_mul(u64::from(height))
            .ok_or_else(|| RenderError::limit("image_pixels", u64::MAX))?;
        if pixels > image_pixel_limit {
            return Err(RenderError::limit("image_pixels", pixels));
        }
        if let Some(page) = pages.get_mut(image.page) {
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
                source: Some(image.source),
            }));
        }
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

fn length(start: &BytesStart<'_>, name: &[u8]) -> Option<f32> {
    attr(start, name).as_deref().and_then(parse_length)
}

fn parse_length(value: &str) -> Option<f32> {
    let split = value
        .find(|character: char| {
            !character.is_ascii_digit() && !matches!(character, '.' | '-' | '+')
        })
        .unwrap_or(value.len());
    let number = value.get(..split)?.parse::<f32>().ok()?;
    Some(match value.get(split..)?.trim() {
        "in" => number * 96.0,
        "cm" => number / 2.54 * 96.0,
        "mm" => number / 25.4 * 96.0,
        "pt" => number / 72.0 * 96.0,
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
