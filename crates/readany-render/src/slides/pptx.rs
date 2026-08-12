use crate::container::{resolve_relationship, xml, zip::Archive};
use crate::model::*;
use crate::text::{TextStyle, shape};
use crate::{Format, Options, RenderError};
use quick_xml::events::Event;
use std::collections::BTreeMap;
pub(crate) fn render(bytes: &[u8], options: &Options<'_>) -> Result<Rendered, RenderError> {
    let archive = Archive::open(bytes, &options.limits)?;
    let presentation = archive.required("ppt/presentation.xml")?;
    xml::validate(presentation, &options.limits)?;
    let size = parse_size(presentation);
    let relationships = parse_relationships(
        archive.required("ppt/_rels/presentation.xml.rels")?,
        "ppt/presentation.xml",
    )?;
    let names = slide_targets(presentation, &relationships)?;
    if names.len() as u32 > options.limits.pages {
        return Err(RenderError::limit("pages", names.len() as u64));
    }
    let mut pages = Vec::new();
    let mut unrendered = Vec::new();
    for (name_index, name) in names.iter().enumerate() {
        let xml_bytes = archive.required(name)?;
        xml::validate(xml_bytes, &options.limits)?;
        let slide_relationships = archive
            .get(&relationship_part(name))
            .map(|bytes| parse_relationships_with_external(bytes, name))
            .transpose()?
            .unwrap_or_default();
        unrendered.extend(
            slide_relationships
                .external
                .into_iter()
                .map(|target| Unrendered::ExternalReference { target }),
        );
        let layout_name = slide_relationships
            .targets
            .values()
            .find(|target| target.contains("/slideLayouts/"))
            .cloned();
        let layout = layout_name.as_deref().and_then(|name| archive.get(name));
        let master_name = if let Some(layout_name) = layout_name.as_deref() {
            archive
                .get(&relationship_part(layout_name))
                .map(|bytes| parse_relationships_with_external(bytes, layout_name))
                .transpose()?
                .and_then(|relationships| {
                    unrendered.extend(
                        relationships
                            .external
                            .into_iter()
                            .map(|target| Unrendered::ExternalReference { target }),
                    );
                    relationships
                        .targets
                        .into_values()
                        .find(|target| target.contains("/slideMasters/"))
                })
        } else {
            None
        };
        let master = master_name.as_deref().and_then(|name| archive.get(name));
        let mut placeholders = BTreeMap::new();
        if let Some(master) = master {
            placeholders.extend(parse_placeholder_geometry(master));
        }
        if let Some(layout) = layout {
            placeholders.extend(parse_placeholder_geometry(layout));
        }
        pages.push(parse_slide(
            &archive,
            xml_bytes,
            name_index as u32,
            size,
            &placeholders,
            &slide_relationships.targets,
            options.limits.image_pixels,
        )?);
    }
    Ok(Rendered {
        pages,
        format: Format::Pptx,
        unrendered,
        meta: Meta::default(),
    })
}

#[derive(Default)]
struct Relationships {
    targets: BTreeMap<String, String>,
    external: Vec<String>,
}

fn parse_relationships_with_external(
    bytes: &[u8],
    base: &str,
) -> Result<Relationships, RenderError> {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut relationships = Relationships::default();
    loop {
        match reader.read_event() {
            Ok(Event::Empty(start)) | Ok(Event::Start(start))
                if xml::local_name(start.name().as_ref()) == b"Relationship" =>
            {
                if let Some(target) = attr(&start, b"Target") {
                    if attr(&start, b"TargetMode").as_deref() == Some("External") {
                        relationships.external.push(target);
                    } else if let Some(id) = attr(&start, b"Id") {
                        relationships
                            .targets
                            .insert(id, resolve_relationship(base, &target)?);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => {
                return Err(RenderError::malformed(
                    "presentation relationships are malformed; obtain a fresh copy",
                ));
            }
        }
    }
    Ok(relationships)
}

fn relationship_part(part: &str) -> String {
    let (parent, file) = part.rsplit_once('/').unwrap_or(("", part));
    if parent.is_empty() {
        format!("_rels/{file}.rels")
    } else {
        format!("{parent}/_rels/{file}.rels")
    }
}
fn parse_size(bytes: &[u8]) -> Size {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    loop {
        match reader.read_event() {
            Ok(Event::Empty(s)) | Ok(Event::Start(s))
                if xml::local_name(s.name().as_ref()) == b"sldSz" =>
            {
                let cx = attr(&s, b"cx")
                    .and_then(|v| v.parse::<f32>().ok())
                    .unwrap_or(12_192_000.0);
                let cy = attr(&s, b"cy")
                    .and_then(|v| v.parse::<f32>().ok())
                    .unwrap_or(6_858_000.0);
                return Size {
                    width: cx / 914_400.0 * 96.0,
                    height: cy / 914_400.0 * 96.0,
                };
            }
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => {}
        }
    }
    Size {
        width: 1280.0,
        height: 720.0,
    }
}

fn parse_relationships(bytes: &[u8], base: &str) -> Result<BTreeMap<String, String>, RenderError> {
    Ok(parse_relationships_with_external(bytes, base)?.targets)
}

fn slide_targets(
    bytes: &[u8],
    relationships: &BTreeMap<String, String>,
) -> Result<Vec<String>, RenderError> {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut targets = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Empty(start)) | Ok(Event::Start(start))
                if xml::local_name(start.name().as_ref()) == b"sldId" =>
            {
                let id = attr_exact(&start, b"r:id").ok_or_else(|| {
                    RenderError::malformed("a slide has no relationship id; obtain a fresh copy")
                })?;
                let target = relationships.get(&id).cloned().ok_or_else(|| {
                    RenderError::malformed("a slide relationship is missing; obtain a fresh copy")
                })?;
                targets.push(target);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => {
                return Err(RenderError::malformed(
                    "presentation metadata is malformed; obtain a fresh copy",
                ));
            }
        }
    }
    Ok(targets)
}
fn parse_slide(
    archive: &Archive,
    bytes: &[u8],
    slide: u32,
    size: Size,
    placeholders: &BTreeMap<String, Rect>,
    relationships: &BTreeMap<String, String>,
    image_pixel_limit: u64,
) -> Result<Page, RenderError> {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let default_style = TextStyle {
        family: "Calibri".into(),
        size_px: 24.0,
        ..TextStyle::default()
    };
    let mut style = default_style.clone();
    let mut items = Vec::new();
    let mut shape_index = 0_u32;
    let mut text = String::new();
    let mut in_t = false;
    let mut in_run_properties = false;
    let mut in_shape_properties = false;
    let mut placeholder = None::<String>;
    let mut explicit_geometry = false;
    let mut fill = None::<Colour>;
    let mut geometry = String::new();
    let mut shape_kind = Vec::<u8>::new();
    let mut image_relationship = None::<String>;
    let (mut x, mut y, mut width, mut height) = (48.0, 48.0, size.width - 96.0, 80.0);
    loop {
        match reader.read_event() {
            Ok(Event::Start(s))
                if matches!(
                    xml::local_name(s.name().as_ref()),
                    b"sp" | b"cxnSp" | b"graphicFrame" | b"pic"
                ) =>
            {
                text.clear();
                style = default_style.clone();
                shape_index = shape_index.saturating_add(1);
                placeholder = None;
                explicit_geometry = false;
                fill = None;
                geometry.clear();
                image_relationship = None;
                shape_kind = xml::local_name(s.name().as_ref()).to_vec();
                (x, y, width, height) = (48.0, 48.0, size.width - 96.0, 80.0);
            }
            Ok(Event::Start(s)) if xml::local_name(s.name().as_ref()) == b"spPr" => {
                in_shape_properties = true
            }
            Ok(Event::End(s)) if xml::local_name(s.name().as_ref()) == b"spPr" => {
                in_shape_properties = false
            }
            Ok(Event::Empty(s)) | Ok(Event::Start(s))
                if matches!(xml::local_name(s.name().as_ref()), b"rPr" | b"defRPr") =>
            {
                in_run_properties = true;
                if let Some(size) = attr(&s, b"sz").and_then(|value| value.parse::<f32>().ok()) {
                    style.size_px = size / 100.0 * 96.0 / 72.0;
                }
                style.bold = attr(&s, b"b").is_some_and(|value| value == "1");
                style.italic = attr(&s, b"i").is_some_and(|value| value == "1");
            }
            Ok(Event::End(s))
                if matches!(xml::local_name(s.name().as_ref()), b"rPr" | b"defRPr") =>
            {
                in_run_properties = false
            }
            Ok(Event::Empty(s)) | Ok(Event::Start(s))
                if xml::local_name(s.name().as_ref()) == b"latin" =>
            {
                if let Some(family) = attr(&s, b"typeface").filter(|value| !value.is_empty()) {
                    style.family = family;
                }
            }
            Ok(Event::Empty(s)) | Ok(Event::Start(s))
                if xml::local_name(s.name().as_ref()) == b"srgbClr" =>
            {
                if let Some(colour) = attr(&s, b"val").and_then(|value| rgb(&value)) {
                    if in_shape_properties && !in_run_properties {
                        fill = Some(colour);
                    } else {
                        style.colour = Some(colour);
                    }
                }
            }
            Ok(Event::Empty(s)) if xml::local_name(s.name().as_ref()) == b"off" => {
                x = emu(&s, b"x", x);
                y = emu(&s, b"y", y);
                explicit_geometry = true;
            }
            Ok(Event::Empty(s)) if xml::local_name(s.name().as_ref()) == b"ext" => {
                width = emu(&s, b"cx", width);
                height = emu(&s, b"cy", height);
                explicit_geometry = true;
            }
            Ok(Event::Empty(s)) | Ok(Event::Start(s))
                if xml::local_name(s.name().as_ref()) == b"ph" =>
            {
                placeholder = placeholder_key(&s);
            }
            Ok(Event::Empty(s)) | Ok(Event::Start(s))
                if xml::local_name(s.name().as_ref()) == b"prstGeom" =>
            {
                geometry = attr(&s, b"prst").unwrap_or_default();
            }
            Ok(Event::Empty(s)) | Ok(Event::Start(s))
                if xml::local_name(s.name().as_ref()) == b"blip" =>
            {
                image_relationship = attr_exact(&s, b"r:embed");
            }
            Ok(Event::Start(s)) if xml::local_name(s.name().as_ref()) == b"t" => in_t = true,
            Ok(Event::End(s)) if xml::local_name(s.name().as_ref()) == b"t" => in_t = false,
            Ok(Event::Text(t)) if in_t => {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(&t.decode().map_err(|_| {
                    RenderError::malformed("slide text is malformed; obtain a fresh copy")
                })?);
            }
            Ok(Event::End(s))
                if matches!(
                    xml::local_name(s.name().as_ref()),
                    b"sp" | b"cxnSp" | b"graphicFrame" | b"pic"
                ) =>
            {
                if !explicit_geometry {
                    if let Some(rect) = placeholder.as_ref().and_then(|key| placeholders.get(key)) {
                        (x, y, width, height) = (rect.x, rect.y, rect.width, rect.height);
                    }
                }
                let source = SourceRef::Shape {
                    slide,
                    shape: shape_index - 1,
                };
                let rect = Rect {
                    x,
                    y,
                    width,
                    height,
                };
                let mut shape_items = Vec::new();
                if let Some(target) = image_relationship
                    .as_ref()
                    .and_then(|relationship| relationships.get(relationship))
                {
                    let bytes = archive.get(target).ok_or_else(|| {
                        RenderError::malformed(
                            "an embedded slide image is missing; obtain a fresh copy",
                        )
                    })?;
                    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
                        .with_guessed_format()
                        .map_err(|_| {
                            RenderError::malformed("an embedded slide image has a damaged header")
                        })?;
                    let (pixel_width, pixel_height) = reader.into_dimensions().map_err(|_| {
                        RenderError::malformed("an embedded slide image has damaged dimensions")
                    })?;
                    let pixels = u64::from(pixel_width)
                        .checked_mul(u64::from(pixel_height))
                        .ok_or_else(|| RenderError::limit("image_pixels", u64::MAX))?;
                    if pixels > image_pixel_limit {
                        return Err(RenderError::limit("image_pixels", pixels));
                    }
                    shape_items.push(Item::Image(ImageItem {
                        data: ImageData {
                            mime: mime_for(target).into(),
                            bytes: bytes.to_vec(),
                            pixel_size: Size {
                                width: pixel_width as f32,
                                height: pixel_height as f32,
                            },
                        },
                        rect,
                        source: Some(source.clone()),
                    }));
                } else if shape_kind == b"cxnSp" || geometry == "line" {
                    shape_items.push(Item::Path(PathItem {
                        path: Path {
                            commands: vec![
                                PathCommand::Move(Point { x, y }),
                                PathCommand::Line(Point {
                                    x: x + width,
                                    y: y + height,
                                }),
                            ],
                        },
                        fill: None,
                        stroke: Some(Stroke {
                            paint: Paint {
                                colour: fill.unwrap_or(Colour::BLACK),
                            },
                            width: 1.0,
                            dash: Vec::new(),
                        }),
                        source: Some(source.clone()),
                    }));
                } else if let Some(colour) = fill {
                    shape_items.push(Item::Path(PathItem {
                        path: preset_path(&geometry, rect),
                        fill: Some(Paint { colour }),
                        stroke: None,
                        source: Some(source.clone()),
                    }));
                }
                if !text.is_empty() {
                    let available = (width - 8.0).max(1.0);
                    if measure_text(&text, &style) > available {
                        let ratio = available / measure_text(&text, &style).max(1.0);
                        style.size_px = (style.size_px * ratio).max(8.0);
                    }
                    shape_items.push(Item::Glyphs(shape(
                        &text,
                        &style,
                        Point {
                            x: x + 4.0,
                            y: y + style.size_px,
                        },
                        Some(source.clone()),
                    )));
                }
                items.push(Item::Group(Group {
                    clip: Some(Rect {
                        x,
                        y,
                        width,
                        height,
                    }),
                    items: shape_items,
                    source: Some(source),
                }));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => {
                return Err(RenderError::malformed(
                    "a slide is malformed; obtain a fresh copy",
                ));
            }
        }
    }
    Ok(Page {
        size,
        label: Some(format!("Slide {}", slide + 1)),
        items,
        source: None,
        frozen: None,
    })
}

fn parse_placeholder_geometry(bytes: &[u8]) -> BTreeMap<String, Rect> {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut output = BTreeMap::new();
    let mut in_shape = false;
    let mut key = None::<String>;
    let mut x = None::<f32>;
    let mut y = None::<f32>;
    let mut width = None::<f32>;
    let mut height = None::<f32>;
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) if xml::local_name(start.name().as_ref()) == b"sp" => {
                in_shape = true;
                key = None;
                x = None;
                y = None;
                width = None;
                height = None;
            }
            Ok(Event::Empty(start)) | Ok(Event::Start(start))
                if in_shape && xml::local_name(start.name().as_ref()) == b"ph" =>
            {
                key = placeholder_key(&start);
            }
            Ok(Event::Empty(start))
                if in_shape && xml::local_name(start.name().as_ref()) == b"off" =>
            {
                x = attr(&start, b"x")
                    .and_then(|value| value.parse::<f32>().ok())
                    .map(emu_value);
                y = attr(&start, b"y")
                    .and_then(|value| value.parse::<f32>().ok())
                    .map(emu_value);
            }
            Ok(Event::Empty(start))
                if in_shape && xml::local_name(start.name().as_ref()) == b"ext" =>
            {
                width = attr(&start, b"cx")
                    .and_then(|value| value.parse::<f32>().ok())
                    .map(emu_value);
                height = attr(&start, b"cy")
                    .and_then(|value| value.parse::<f32>().ok())
                    .map(emu_value);
            }
            Ok(Event::End(end)) if xml::local_name(end.name().as_ref()) == b"sp" => {
                if let (Some(key), Some(x), Some(y), Some(width), Some(height)) =
                    (key.take(), x, y, width, height)
                {
                    output.insert(
                        key,
                        Rect {
                            x,
                            y,
                            width,
                            height,
                        },
                    );
                }
                in_shape = false;
            }
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => {}
        }
    }
    output
}

fn placeholder_key(start: &quick_xml::events::BytesStart<'_>) -> Option<String> {
    attr(start, b"idx")
        .map(|value| format!("idx:{value}"))
        .or_else(|| attr(start, b"type").map(|value| format!("type:{value}")))
        .or_else(|| Some("type:body".into()))
}

fn preset_path(kind: &str, rect: Rect) -> Path {
    if kind == "triangle" {
        return Path {
            commands: vec![
                PathCommand::Move(Point {
                    x: rect.x + rect.width / 2.0,
                    y: rect.y,
                }),
                PathCommand::Line(Point {
                    x: rect.x + rect.width,
                    y: rect.y + rect.height,
                }),
                PathCommand::Line(Point {
                    x: rect.x,
                    y: rect.y + rect.height,
                }),
                PathCommand::Close,
            ],
        };
    }
    if kind == "diamond" {
        return Path {
            commands: vec![
                PathCommand::Move(Point {
                    x: rect.x + rect.width / 2.0,
                    y: rect.y,
                }),
                PathCommand::Line(Point {
                    x: rect.x + rect.width,
                    y: rect.y + rect.height / 2.0,
                }),
                PathCommand::Line(Point {
                    x: rect.x + rect.width / 2.0,
                    y: rect.y + rect.height,
                }),
                PathCommand::Line(Point {
                    x: rect.x,
                    y: rect.y + rect.height / 2.0,
                }),
                PathCommand::Close,
            ],
        };
    }
    rect_path(rect)
}

fn measure_text(text: &str, style: &TextStyle) -> f32 {
    crate::text::measure(text, style)
}

fn emu_value(value: f32) -> f32 {
    value / 914_400.0 * 96.0
}
fn rgb(value: &str) -> Option<Colour> {
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
fn attr(s: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<String> {
    s.attributes()
        .with_checks(false)
        .flatten()
        .find(|a| xml::local_name(a.key.as_ref()) == name)
        .map(|a| String::from_utf8_lossy(a.value.as_ref()).into_owned())
}
fn attr_exact(s: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<String> {
    s.attributes()
        .with_checks(false)
        .flatten()
        .find(|attribute| attribute.key.as_ref() == name)
        .map(|attribute| String::from_utf8_lossy(attribute.value.as_ref()).into_owned())
}
fn emu(s: &quick_xml::events::BytesStart<'_>, name: &[u8], default: f32) -> f32 {
    attr(s, name)
        .and_then(|v| v.parse::<f32>().ok())
        .map(|v| v / 914_400.0 * 96.0)
        .unwrap_or(default)
}
