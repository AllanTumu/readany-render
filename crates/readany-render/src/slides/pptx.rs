use crate::container::{resolve_relationship, xml, zip::Archive};
use crate::model::*;
use crate::text::{TextStyle, shape};
use crate::{Format, Options, RenderError};
use quick_xml::events::Event;
use std::collections::BTreeMap;

#[derive(Clone, Default)]
struct Placeholder {
    index: Option<String>,
    kind: String,
}

impl Placeholder {
    fn keys(&self) -> impl Iterator<Item = String> + '_ {
        self.index
            .iter()
            .map(|value| format!("idx:{value}"))
            .chain(std::iter::once(format!("type:{}", self.kind)))
    }
}

/// A `p:grpSp`'s coordinate mapping.
///
/// A group declares where it sits on the slide (`a:off`/`a:ext`) *and* the
/// coordinate space its children are written in (`a:chOff`/`a:chExt`). The two
/// are unrelated numbers, so a child's `a:off` is meaningless until it is mapped
/// through its group — the NASA deck's second slide holds 44 groups, and reading
/// their children's raw offsets scattered every one of them.
#[derive(Clone, Copy, Default)]
struct GroupTransform {
    offset: (f32, f32),
    extent: (f32, f32),
    child_offset: (f32, f32),
    child_extent: (f32, f32),
}

impl GroupTransform {
    fn map(&self, rect: Rect) -> Rect {
        // `p:spTree`'s own `p:grpSpPr` is all zeroes, and so is any group that
        // has never been sized; both mean "no mapping", not "collapse to a
        // point".
        if self.child_extent.0 <= 0.0
            || self.child_extent.1 <= 0.0
            || self.extent.0 <= 0.0
            || self.extent.1 <= 0.0
        {
            return rect;
        }
        let scale_x = self.extent.0 / self.child_extent.0;
        let scale_y = self.extent.1 / self.child_extent.1;
        Rect {
            x: self.offset.0 + (rect.x - self.child_offset.0) * scale_x,
            y: self.offset.1 + (rect.y - self.child_offset.1) * scale_y,
            width: rect.width * scale_x,
            height: rect.height * scale_y,
        }
    }
}

/// Maps a child rectangle out through every group that encloses it, innermost
/// first.
fn apply_groups(rect: Rect, groups: &[GroupTransform]) -> Rect {
    groups
        .iter()
        .rev()
        .fold(rect, |rect, group| group.map(rect))
}

#[derive(Clone, Copy, Default)]
enum TextAlignment {
    #[default]
    Left,
    Centre,
    Right,
}

#[derive(Clone, Copy, Default)]
enum VerticalAnchor {
    #[default]
    Top,
    Centre,
    Bottom,
}

#[derive(Clone)]
struct SlideRun {
    text: String,
    style: TextStyle,
}

#[derive(Default)]
struct SlideParagraph {
    runs: Vec<SlideRun>,
    alignment: TextAlignment,
}

struct TextBody {
    paragraphs: Vec<SlideParagraph>,
    left_inset: f32,
    top_inset: f32,
    right_inset: f32,
    bottom_inset: f32,
    anchor: VerticalAnchor,
}

impl Default for TextBody {
    fn default() -> Self {
        Self {
            paragraphs: Vec::new(),
            // ECMA-376 DrawingML text body defaults: 0.1 in horizontally and
            // 0.05 in vertically.  Treating every inset as 4 px shifted the
            // NASA title 5.6 px left of the LibreOffice reference.
            left_inset: 9.6,
            top_inset: 4.8,
            right_inset: 9.6,
            bottom_inset: 4.8,
            anchor: VerticalAnchor::Top,
        }
    }
}
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
        let (page, page_unrendered) = parse_slide(
            &archive,
            xml_bytes,
            name_index as u32,
            size,
            &placeholders,
            &slide_relationships.targets,
            options.limits.image_pixels,
        )?;
        pages.push(page);
        unrendered.extend(page_unrendered);
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
) -> Result<(Page, Vec<Unrendered>), RenderError> {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let default_style = TextStyle {
        family: "Calibri".into(),
        size_px: 24.0,
        ..TextStyle::default()
    };
    let mut items = Vec::new();
    let mut unsupported_media = BTreeMap::<String, u32>::new();
    let mut shape_index = 0_u32;
    let mut text_body = TextBody::default();
    let mut paragraph = None::<SlideParagraph>;
    let mut run = None::<SlideRun>;
    let mut in_t = false;
    let mut in_run_properties = false;
    let mut in_shape_properties = false;
    let mut placeholder = None::<Placeholder>;
    let mut explicit_geometry = false;
    let mut fill = None::<Colour>;
    let mut geometry = String::new();
    let mut shape_kind = Vec::<u8>::new();
    let mut image_relationship = None::<String>;
    let mut groups = Vec::<GroupTransform>::new();
    let mut in_group_properties = false;
    let mut rotation_deg = 0.0_f32;
    let (mut x, mut y, mut width, mut height) = (48.0, 48.0, size.width - 96.0, 80.0);
    loop {
        match reader.read_event() {
            Ok(Event::Start(s))
                if matches!(
                    xml::local_name(s.name().as_ref()),
                    b"sp" | b"cxnSp" | b"graphicFrame" | b"pic"
                ) =>
            {
                text_body = TextBody::default();
                paragraph = None;
                run = None;
                shape_index = shape_index.saturating_add(1);
                placeholder = None;
                explicit_geometry = false;
                fill = None;
                geometry.clear();
                image_relationship = None;
                shape_kind = xml::local_name(s.name().as_ref()).to_vec();
                rotation_deg = 0.0;
                (x, y, width, height) = (48.0, 48.0, size.width - 96.0, 80.0);
            }
            Ok(Event::Start(s)) if xml::local_name(s.name().as_ref()) == b"grpSp" => {
                groups.push(GroupTransform::default());
            }
            Ok(Event::End(s)) if xml::local_name(s.name().as_ref()) == b"grpSp" => {
                groups.pop();
            }
            Ok(Event::Start(s)) if xml::local_name(s.name().as_ref()) == b"grpSpPr" => {
                in_group_properties = true
            }
            Ok(Event::End(s)) if xml::local_name(s.name().as_ref()) == b"grpSpPr" => {
                in_group_properties = false
            }
            Ok(Event::Empty(s)) | Ok(Event::Start(s))
                if xml::local_name(s.name().as_ref()) == b"xfrm" =>
            {
                // 60,000ths of a degree, clockwise.
                if !in_group_properties {
                    rotation_deg = attr(&s, b"rot")
                        .and_then(|value| value.parse::<f32>().ok())
                        .map(|value| value / 60_000.0)
                        .unwrap_or(0.0);
                }
            }
            Ok(Event::Empty(s))
                if in_group_properties
                    && matches!(
                        xml::local_name(s.name().as_ref()),
                        b"off" | b"ext" | b"chOff" | b"chExt"
                    ) =>
            {
                // The slide's root `p:grpSpPr` has no group to fill, and must
                // not be mistaken for the geometry of the shape that follows.
                if let Some(group) = groups.last_mut() {
                    match xml::local_name(s.name().as_ref()) {
                        b"off" => group.offset = (emu(&s, b"x", 0.0), emu(&s, b"y", 0.0)),
                        b"ext" => group.extent = (emu(&s, b"cx", 0.0), emu(&s, b"cy", 0.0)),
                        b"chOff" => group.child_offset = (emu(&s, b"x", 0.0), emu(&s, b"y", 0.0)),
                        b"chExt" => group.child_extent = (emu(&s, b"cx", 0.0), emu(&s, b"cy", 0.0)),
                        _ => {}
                    }
                }
            }
            Ok(Event::Start(s)) if xml::local_name(s.name().as_ref()) == b"spPr" => {
                in_shape_properties = true
            }
            Ok(Event::End(s)) if xml::local_name(s.name().as_ref()) == b"spPr" => {
                in_shape_properties = false
            }
            Ok(Event::Start(s))
                if matches!(xml::local_name(s.name().as_ref()), b"rPr" | b"defRPr") =>
            {
                in_run_properties = true;
                apply_run_properties(&s, &mut run, &default_style);
            }
            Ok(Event::Empty(s))
                if matches!(xml::local_name(s.name().as_ref()), b"rPr" | b"defRPr") =>
            {
                apply_run_properties(&s, &mut run, &default_style);
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
                    if let Some(run) = run.as_mut() {
                        run.style.family = family;
                    }
                }
            }
            Ok(Event::Empty(s)) | Ok(Event::Start(s))
                if xml::local_name(s.name().as_ref()) == b"srgbClr" =>
            {
                if let Some(colour) = attr(&s, b"val").and_then(|value| rgb(&value)) {
                    if in_shape_properties && !in_run_properties {
                        fill = Some(colour);
                    } else if let Some(run) = run.as_mut() {
                        run.style.colour = Some(colour);
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
                placeholder = Some(parse_placeholder(&s));
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
            Ok(Event::Empty(s)) | Ok(Event::Start(s))
                if xml::local_name(s.name().as_ref()) == b"bodyPr" =>
            {
                text_body.left_inset = emu(&s, b"lIns", text_body.left_inset);
                text_body.top_inset = emu(&s, b"tIns", text_body.top_inset);
                text_body.right_inset = emu(&s, b"rIns", text_body.right_inset);
                text_body.bottom_inset = emu(&s, b"bIns", text_body.bottom_inset);
                text_body.anchor = match attr(&s, b"anchor").as_deref() {
                    Some("ctr") => VerticalAnchor::Centre,
                    Some("b") => VerticalAnchor::Bottom,
                    Some("t") | Some("just") | Some("dist") | None => VerticalAnchor::Top,
                    Some(_) => VerticalAnchor::Top,
                };
            }
            Ok(Event::Start(s)) if xml::local_name(s.name().as_ref()) == b"p" => {
                finish_slide_paragraph(&mut text_body, &mut paragraph, &mut run);
                paragraph = Some(SlideParagraph::default());
            }
            Ok(Event::Empty(s)) if xml::local_name(s.name().as_ref()) == b"p" => {
                finish_slide_paragraph(&mut text_body, &mut paragraph, &mut run);
                text_body.paragraphs.push(SlideParagraph::default());
            }
            Ok(Event::Empty(s)) | Ok(Event::Start(s))
                if xml::local_name(s.name().as_ref()) == b"pPr" =>
            {
                if let Some(paragraph) = paragraph.as_mut() {
                    paragraph.alignment = match attr(&s, b"algn").as_deref() {
                        Some("ctr") => TextAlignment::Centre,
                        Some("r") => TextAlignment::Right,
                        Some("l") | Some("just") | Some("dist") | None => TextAlignment::Left,
                        Some(_) => TextAlignment::Left,
                    };
                }
            }
            Ok(Event::Start(s)) if matches!(xml::local_name(s.name().as_ref()), b"r" | b"fld") => {
                finish_slide_run(&mut paragraph, &mut run);
                run = Some(SlideRun {
                    text: String::new(),
                    style: default_style.clone(),
                });
            }
            Ok(Event::Start(s)) if xml::local_name(s.name().as_ref()) == b"t" => in_t = true,
            Ok(Event::End(s)) if xml::local_name(s.name().as_ref()) == b"t" => in_t = false,
            Ok(Event::Text(t)) if in_t => {
                run.get_or_insert_with(|| SlideRun {
                    text: String::new(),
                    style: default_style.clone(),
                })
                .text
                .push_str(&t.decode().map_err(|_| {
                    RenderError::malformed("slide text is malformed; obtain a fresh copy")
                })?);
            }
            Ok(Event::GeneralRef(reference)) if in_t => {
                run.get_or_insert_with(|| SlideRun {
                    text: String::new(),
                    style: default_style.clone(),
                })
                .text
                .push_str(&xml::decode_reference(&reference)?);
            }
            Ok(Event::End(s)) if matches!(xml::local_name(s.name().as_ref()), b"r" | b"fld") => {
                finish_slide_run(&mut paragraph, &mut run);
            }
            Ok(Event::End(s)) if xml::local_name(s.name().as_ref()) == b"p" => {
                finish_slide_paragraph(&mut text_body, &mut paragraph, &mut run);
            }
            Ok(Event::End(s))
                if matches!(
                    xml::local_name(s.name().as_ref()),
                    b"sp" | b"cxnSp" | b"graphicFrame" | b"pic"
                ) =>
            {
                finish_slide_paragraph(&mut text_body, &mut paragraph, &mut run);
                if !explicit_geometry {
                    if let Some(rect) = placeholder.as_ref().and_then(|placeholder| {
                        placeholder.keys().find_map(|key| placeholders.get(&key))
                    }) {
                        (x, y, width, height) = (rect.x, rect.y, rect.width, rect.height);
                    }
                }
                let source = SourceRef::Shape {
                    slide,
                    shape: shape_index - 1,
                };
                let rect = apply_groups(
                    Rect {
                        x,
                        y,
                        width,
                        height,
                    },
                    &groups,
                );
                let (x, y, width, height) = (rect.x, rect.y, rect.width, rect.height);
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
                    if let Some(kind) = unsupported_media_kind(target) {
                        *unsupported_media.entry(kind).or_default() += 1;
                    } else {
                        let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
                            .with_guessed_format()
                            .map_err(|_| {
                                RenderError::malformed(
                                    "an embedded slide image has a damaged header",
                                )
                            })?;
                        let (pixel_width, pixel_height) =
                            reader.into_dimensions().map_err(|_| {
                                RenderError::malformed(
                                    "an embedded slide image has damaged dimensions",
                                )
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
                    }
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
                shape_items.extend(layout_slide_text(
                    &text_body,
                    rect,
                    placeholder.as_ref(),
                    &source,
                    rotation_deg,
                ));
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
    order_text_shapes(&mut items);
    Ok((
        Page {
            size,
            label: Some(format!("Slide {}", slide + 1)),
            items,
            source: None,
            frozen: None,
            grid: None,
        },
        unsupported_media
            .into_iter()
            .map(|(kind, count)| Unrendered::UnsupportedMedia {
                page: slide,
                kind,
                count,
            })
            .collect(),
    ))
}

fn order_text_shapes(items: &mut Vec<Item>) {
    let original = std::mem::take(items);
    let mut slots = Vec::with_capacity(original.len());
    let mut text_items = Vec::new();
    for item in original {
        if text_shape_origin(&item).is_some() {
            text_items.push(item);
            slots.push(None);
        } else {
            slots.push(Some(item));
        }
    }
    // PresentationML's shape tree is z-order, not reading order.  LibreOffice
    // exposes slide text top-to-bottom and then left-to-right; on NASA slide 9
    // the XML order started with two bottom-right page numbers, which dropped
    // sequence agreement to 0.679 even though the characters were present.
    text_items.sort_by(
        |left, right| match (text_shape_origin(left), text_shape_origin(right)) {
            (Some(left), Some(right)) => left
                .0
                .total_cmp(&right.0)
                .then_with(|| left.1.total_cmp(&right.1)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        },
    );
    let mut text_items = text_items.into_iter();
    for slot in slots {
        match slot {
            Some(item) => items.push(item),
            None => {
                if let Some(item) = text_items.next() {
                    items.push(item);
                }
            }
        }
    }
}

fn text_shape_origin(item: &Item) -> Option<(f32, f32)> {
    let Item::Group(group) = item else {
        return None;
    };
    group
        .items
        .iter()
        .filter_map(|item| {
            let Item::Glyphs(run) = item else { return None };
            Some((run.origin.y - run.size_px, run.origin.x))
        })
        .min_by(|left, right| {
            left.0
                .total_cmp(&right.0)
                .then_with(|| left.1.total_cmp(&right.1))
        })
}

fn unsupported_media_kind(target: &str) -> Option<String> {
    let extension = target.rsplit_once('.')?.1;
    match extension.to_ascii_lowercase().as_str() {
        "svg" => Some("svg".into()),
        "emf" => Some("emf".into()),
        "wmf" => Some("wmf".into()),
        _ => None,
    }
}

fn apply_run_properties(
    start: &quick_xml::events::BytesStart<'_>,
    run: &mut Option<SlideRun>,
    default_style: &TextStyle,
) {
    let style = &mut run
        .get_or_insert_with(|| SlideRun {
            text: String::new(),
            style: default_style.clone(),
        })
        .style;
    if let Some(size) = attr(start, b"sz").and_then(|value| value.parse::<f32>().ok()) {
        style.size_px = size / 100.0 * 96.0 / 72.0;
    }
    style.bold = attr(start, b"b").is_some_and(|value| value == "1");
    style.italic = attr(start, b"i").is_some_and(|value| value == "1");
}

fn finish_slide_run(paragraph: &mut Option<SlideParagraph>, run: &mut Option<SlideRun>) {
    let Some(run) = run.take() else { return };
    if run.text.is_empty() {
        return;
    }
    paragraph
        .get_or_insert_with(SlideParagraph::default)
        .runs
        .push(run);
}

fn finish_slide_paragraph(
    body: &mut TextBody,
    paragraph: &mut Option<SlideParagraph>,
    run: &mut Option<SlideRun>,
) {
    finish_slide_run(paragraph, run);
    if let Some(paragraph) = paragraph.take() {
        body.paragraphs.push(paragraph);
    }
}

#[derive(Default)]
struct SlideLine {
    runs: Vec<SlideRun>,
    width: f32,
    height: f32,
    alignment: TextAlignment,
}

fn layout_slide_text(
    body: &TextBody,
    rect: Rect,
    placeholder: Option<&Placeholder>,
    source: &SourceRef,
    rotation_deg: f32,
) -> Vec<Item> {
    // Leave one device pixel for the EMU-to-pixel rounding at the far edge.
    // Without it the 585.6 px NASA image-credit line narrowly fit a 586.4 px
    // body where LibreOffice wraps its final 38.8 px word.
    let available_width = (rect.width - body.left_inset - body.right_inset - 1.0).max(1.0);
    let kind = placeholder
        .map(|value| value.kind.as_str())
        .unwrap_or_default();
    // DrawingML's percentage is applied to font line metrics, while our
    // display-list boxes use the em square.  The measured equivalents in the
    // NASA reference are 1.10 em for title wraps (87.8 px after a 60 pt line)
    // and 1.08 em for body lines (51.9 px after a 36 pt line).
    let line_spacing = match kind {
        "title" | "ctrTitle" => 1.1,
        "body" | "obj" => 1.08,
        _ => 1.2,
    };
    let paragraph_before = if matches!(kind, "body" | "obj") {
        13.333_333
    } else {
        0.0
    };
    let mut lines = Vec::<SlideLine>::new();
    for (paragraph_index, paragraph) in body.paragraphs.iter().enumerate() {
        let mut paragraph_lines = wrap_slide_paragraph(paragraph, available_width, kind);
        if paragraph_lines.is_empty() {
            paragraph_lines.push(SlideLine {
                height: 24.0,
                alignment: paragraph.alignment,
                ..SlideLine::default()
            });
        }
        if paragraph_index > 0 && paragraph_before > 0.0 {
            lines.push(SlideLine {
                height: paragraph_before,
                ..SlideLine::default()
            });
        }
        lines.extend(paragraph_lines);
    }

    if kind == "sldNum" {
        for line in &mut lines {
            line.alignment = TextAlignment::Right;
        }
    }
    let content_height = lines
        .iter()
        .map(|line| {
            if line.runs.is_empty() {
                line.height
            } else {
                line.height * line_spacing
            }
        })
        .sum::<f32>();
    let inherited_anchor = if matches!(kind, "dt" | "ftr" | "sldNum") {
        VerticalAnchor::Centre
    } else {
        body.anchor
    };
    let mut output = Vec::new();
    let mut y = match inherited_anchor {
        VerticalAnchor::Top => rect.y + body.top_inset,
        VerticalAnchor::Centre => rect.y + (rect.height - content_height) / 2.0,
        VerticalAnchor::Bottom => rect.y + rect.height - body.bottom_inset - content_height,
    };
    for line in lines {
        let line_height = line.height.max(1.0);
        if line.runs.is_empty() {
            y += line_height;
            continue;
        }
        let mut x = match line.alignment {
            TextAlignment::Left => rect.x + body.left_inset,
            TextAlignment::Centre => rect.x + (rect.width - line.width) / 2.0,
            TextAlignment::Right => rect.x + rect.width - body.right_inset - line.width,
        };
        for run in line.runs {
            let width = measure_text(&run.text, &run.style);
            // `a:xfrm rot` turns the shape about its own centre, so the text
            // inside it turns with the box rather than about its own origin.
            let origin = rotate_about(
                Point {
                    x,
                    y: y + run.style.size_px,
                },
                Point {
                    x: rect.x + rect.width / 2.0,
                    y: rect.y + rect.height / 2.0,
                },
                rotation_deg,
            );
            let mut glyphs = shape(&run.text, &run.style, origin, Some(source.clone()));
            glyphs.rotation_deg = rotation_deg;
            output.push(Item::Glyphs(glyphs));
            x += width;
        }
        y += line_height * line_spacing;
    }
    output
}

/// Turns `point` about `centre` by `degrees` clockwise in display-list space.
fn rotate_about(point: Point, centre: Point, degrees: f32) -> Point {
    if degrees == 0.0 {
        return point;
    }
    let (sin, cos) = degrees.to_radians().sin_cos();
    let dx = point.x - centre.x;
    let dy = point.y - centre.y;
    Point {
        x: centre.x + dx * cos - dy * sin,
        y: centre.y + dx * sin + dy * cos,
    }
}

fn wrap_slide_paragraph(paragraph: &SlideParagraph, width: f32, kind: &str) -> Vec<SlideLine> {
    let mut lines = Vec::new();
    let mut line = SlideLine {
        alignment: paragraph.alignment,
        ..SlideLine::default()
    };
    for run in &paragraph.runs {
        let mut style = run.style.clone();
        if matches!(kind, "dt" | "ftr" | "sldNum") && style.size_px == 24.0 {
            // The master footer placeholders use 12 pt default runs.  Slide
            // fields omit `sz`, so the generic 18 pt presentation fallback
            // made them 8 px too large.
            style.size_px = 16.0;
        }
        for token in run.text.split_inclusive(char::is_whitespace) {
            // A word is measured without the space that follows it. Counting
            // that space made a line that ends exactly at the box edge look
            // like an overflow, and every such line in the NASA deck lost its
            // last word to the line below.
            let token_width = measure_text(token.trim_end(), &style);
            if line.width + token_width > width && !line.runs.is_empty() {
                lines.push(std::mem::replace(
                    &mut line,
                    SlideLine {
                        alignment: paragraph.alignment,
                        ..SlideLine::default()
                    },
                ));
            }
            let text = if line.runs.is_empty() {
                token.trim_start().to_owned()
            } else {
                token.to_owned()
            };
            if text.is_empty() {
                line.height = line.height.max(style.size_px);
                continue;
            }
            let measured = measure_text(&text, &style);
            if let Some(previous) = line
                .runs
                .last_mut()
                .filter(|previous| previous.style == style)
            {
                previous.text.push_str(&text);
            } else {
                line.runs.push(SlideRun {
                    text,
                    style: style.clone(),
                });
            }
            line.width += measured;
            line.height = line.height.max(style.size_px);
        }
    }
    if !line.runs.is_empty() || line.height > 0.0 {
        lines.push(line);
    }
    lines
}

fn parse_placeholder_geometry(bytes: &[u8]) -> BTreeMap<String, Rect> {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut output = BTreeMap::new();
    let mut in_shape = false;
    let mut placeholder = None::<Placeholder>;
    let mut x = None::<f32>;
    let mut y = None::<f32>;
    let mut width = None::<f32>;
    let mut height = None::<f32>;
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) if xml::local_name(start.name().as_ref()) == b"sp" => {
                in_shape = true;
                placeholder = None;
                x = None;
                y = None;
                width = None;
                height = None;
            }
            Ok(Event::Empty(start)) | Ok(Event::Start(start))
                if in_shape && xml::local_name(start.name().as_ref()) == b"ph" =>
            {
                placeholder = Some(parse_placeholder(&start));
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
                if let (Some(placeholder), Some(x), Some(y), Some(width), Some(height)) =
                    (placeholder.take(), x, y, width, height)
                {
                    let rect = Rect {
                        x,
                        y,
                        width,
                        height,
                    };
                    for key in placeholder.keys() {
                        output.insert(key, rect);
                    }
                }
                in_shape = false;
            }
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => {}
        }
    }
    output
}

fn parse_placeholder(start: &quick_xml::events::BytesStart<'_>) -> Placeholder {
    Placeholder {
        index: attr(start, b"idx"),
        kind: attr(start, b"type").unwrap_or_else(|| "body".into()),
    }
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
