use crate::container::{resolve_relationship, xml, zip::Archive};
use crate::flow::styles::{
    ParagraphPatch, RunPatch, StyleSheet, apply_paragraph_property, apply_run_property,
};
use crate::flow::{FlowParagraph, FlowRun, layout_flow};
use crate::model::{ImageData, ImageItem, Item, Rect, Size, SourceRef, Unrendered};
use crate::{Format, Options, RenderError};
use quick_xml::events::{BytesStart, Event};
use std::collections::BTreeMap;

pub(crate) fn render(bytes: &[u8], options: &Options<'_>) -> Result<crate::Rendered, RenderError> {
    let archive = Archive::open(bytes, &options.limits)?;
    let document = archive.required("word/document.xml")?;
    xml::validate(document, &options.limits)?;
    check_table_spans(document, options.limits.cells)?;
    let styles = StyleSheet::parse(archive.get("word/styles.xml"))?;
    let numbering = Numbering::parse(archive.get("word/numbering.xml"))?;
    let relationships = Relationships::parse(
        archive.get("word/_rels/document.xml.rels"),
        "word/document.xml",
    )?;
    let mut parsed = parse_part(document, &styles, &numbering)?;

    for notes_name in ["word/footnotes.xml", "word/endnotes.xml"] {
        if let Some(notes) = archive.get(notes_name) {
            xml::validate(notes, &options.limits)?;
            let notes = parse_part(notes, &styles, &numbering)?;
            parsed.paragraphs.extend(notes.paragraphs);
        }
    }

    let mut rendered = layout_flow(
        &parsed.paragraphs,
        Format::Docx,
        options,
        parsed.size,
        parsed.margins,
    )?;
    paint_tables(&parsed.tables, parsed.margins, &mut rendered);
    paint_repeating_parts(
        &archive,
        &styles,
        &numbering,
        &mut rendered,
        parsed.margins,
        options,
    )?;
    paint_images(
        &archive,
        &relationships.targets,
        &parsed.images,
        &mut rendered,
        options.limits.image_pixels,
    )?;

    for target in relationships.external {
        rendered
            .unrendered
            .push(Unrendered::ExternalReference { target });
    }
    for name in archive.names() {
        if name.starts_with("word/embeddings/") {
            rendered.unrendered.push(Unrendered::Ole { page: 0 });
        }
        if name.ends_with("vbaProject.bin") {
            rendered.unrendered.push(Unrendered::Macro);
        }
    }
    Ok(rendered)
}

fn paint_repeating_parts(
    archive: &Archive,
    styles: &StyleSheet,
    numbering: &Numbering,
    rendered: &mut crate::Rendered,
    margins: Margins,
    options: &Options<'_>,
) -> Result<(), RenderError> {
    let names = archive
        .names()
        .filter(|name| {
            (name.starts_with("word/header") || name.starts_with("word/footer"))
                && name.ends_with(".xml")
        })
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for name in names {
        let Some(bytes) = archive.get(&name) else {
            continue;
        };
        let parsed = parse_part(bytes, styles, numbering)?;
        let header = name.starts_with("word/header");
        let vertical = if header { 24.0 } else { 48.0 };
        let part_margins = if header {
            (
                margins.0,
                vertical,
                margins.2,
                rendered.pages[0].size.height - 80.0,
            )
        } else {
            (
                margins.0,
                rendered.pages[0].size.height - 64.0,
                margins.2,
                16.0,
            )
        };
        let laid_out = layout_flow(
            &parsed.paragraphs,
            Format::Docx,
            options,
            rendered.pages[0].size,
            part_margins,
        )?;
        if let Some(part_page) = laid_out.pages.first() {
            for page in &mut rendered.pages {
                page.items.extend(part_page.items.clone());
            }
        }
    }
    Ok(())
}

fn paint_images(
    archive: &Archive,
    relationships: &BTreeMap<String, String>,
    images: &[PendingImage],
    rendered: &mut crate::Rendered,
    image_pixel_limit: u64,
) -> Result<(), RenderError> {
    for image in images {
        let Some(target) = relationships.get(&image.relationship) else {
            continue;
        };
        let Some(bytes) = archive.get(target) else {
            continue;
        };
        let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|_| RenderError::malformed("an embedded Word image has a damaged header"))?;
        let (pixel_width, pixel_height) = reader
            .into_dimensions()
            .map_err(|_| RenderError::malformed("an embedded Word image has damaged dimensions"))?;
        check_image_pixels(pixel_width, pixel_height, image_pixel_limit)?;
        let mut page_index = 0_usize;
        let mut baseline = 96.0;
        for (index, page) in rendered.pages.iter().enumerate() {
            if let Some(y) = paragraph_baseline(&page.items, image.paragraph) {
                page_index = index;
                baseline = y;
                break;
            }
        }
        let Some(page) = rendered.pages.get_mut(page_index) else {
            continue;
        };
        let width = image.width.max(1.0);
        let height = image.height.max(1.0);
        let x = image.x.unwrap_or(96.0);
        let y = image.y.unwrap_or((baseline - height).max(0.0));
        page.items.push(Item::Image(ImageItem {
            data: ImageData {
                mime: mime_for(target).into(),
                bytes: bytes.to_vec(),
                pixel_size: Size {
                    width: pixel_width as f32,
                    height: pixel_height as f32,
                },
            },
            rect: Rect {
                x,
                y,
                width,
                height,
            },
            source: Some(SourceRef::Text {
                paragraph: image.paragraph,
                start: 0,
                end: 0,
            }),
        }));
    }
    Ok(())
}

fn check_image_pixels(width: u32, height: u32, limit: u64) -> Result<(), RenderError> {
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| RenderError::limit("image_pixels", u64::MAX))?;
    if pixels > limit {
        return Err(RenderError::limit("image_pixels", pixels));
    }
    Ok(())
}

fn paragraph_baseline(items: &[Item], paragraph: u32) -> Option<f32> {
    for item in items {
        match item {
            Item::Glyphs(run) if matches!(run.source, Some(SourceRef::Text { paragraph: value, .. }) if value == paragraph) =>
            {
                return Some(run.origin.y);
            }
            Item::Group(group) => {
                if let Some(y) = paragraph_baseline(&group.items, paragraph) {
                    return Some(y);
                }
            }
            Item::Glyphs(_) | Item::Path(_) | Item::Image(_) => {}
        }
    }
    None
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

#[derive(Clone, Debug)]
struct PendingImage {
    relationship: String,
    paragraph: u32,
    width: f32,
    height: f32,
    x: Option<f32>,
    y: Option<f32>,
}

type Margins = (f32, f32, f32, f32);

struct ParsedDocument {
    paragraphs: Vec<FlowParagraph>,
    size: Size,
    margins: Margins,
    images: Vec<PendingImage>,
    tables: Vec<TableRow>,
}

struct TableRow {
    paragraph: u32,
    widths: Vec<f32>,
}

struct TableCell {
    paragraphs: Vec<FlowParagraph>,
    width: Option<f32>,
    span: u32,
}

#[derive(Default)]
struct PendingRun {
    text: String,
    patch: RunPatch,
    style_id: Option<String>,
}

fn parse_part(
    bytes: &[u8],
    styles: &StyleSheet,
    numbering: &Numbering,
) -> Result<ParsedDocument, RenderError> {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut paragraphs = Vec::new();
    let mut pending_runs = Vec::<PendingRun>::new();
    let mut current_run = None::<PendingRun>;
    let mut paragraph_patch = ParagraphPatch::default();
    let mut paragraph_style_id = None::<String>;
    let mut list_id = None::<u32>;
    let mut list_level = 0_u32;
    let mut in_paragraph = false;
    let mut in_paragraph_properties = false;
    let mut in_run_properties = false;
    let mut in_text = false;
    let mut in_instruction = false;
    let mut width = 816.0;
    let mut height = 1056.0;
    let mut margins = (96.0, 96.0, 96.0, 96.0);
    let mut images = Vec::new();
    let mut image = None::<PendingImage>;
    let mut position_axis = None::<u8>;
    let mut in_position_offset = false;
    let mut position_text = String::new();
    let mut counters = BTreeMap::<(u32, u32), u32>::new();
    let mut table_depth = 0_u32;
    let mut table_grid = Vec::<f32>::new();
    let mut table_rows = Vec::<TableRow>::new();
    let mut row_cells = Vec::<TableCell>::new();
    let mut cell_start = None::<usize>;
    let mut cell_width = None::<f32>;
    let mut cell_span = 1_u32;
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => match xml::local_name(start.name().as_ref()) {
                b"tbl" => {
                    table_depth = table_depth.saturating_add(1);
                    if table_depth == 1 {
                        table_grid.clear();
                    }
                }
                b"tr" if table_depth == 1 => row_cells.clear(),
                b"tc" if table_depth == 1 => {
                    cell_start = Some(paragraphs.len());
                    cell_width = None;
                    cell_span = 1;
                }
                b"gridCol" if table_depth == 1 => {
                    table_grid.push(twip_num(&start, b"w", 1_440));
                }
                b"gridSpan" if table_depth == 1 => {
                    cell_span = attr(&start, b"val")
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(1);
                }
                b"tcW" if table_depth == 1 => {
                    cell_width = attr(&start, b"w")
                        .and_then(|value| value.parse::<u32>().ok())
                        .map(twip);
                }
                b"p" => {
                    in_paragraph = true;
                    pending_runs.clear();
                    paragraph_patch = ParagraphPatch::default();
                    paragraph_style_id = None;
                    list_id = None;
                    list_level = 0;
                }
                b"pPr" if in_paragraph => in_paragraph_properties = true,
                b"r" if in_paragraph => current_run = Some(PendingRun::default()),
                b"rPr" if current_run.is_some() => in_run_properties = true,
                b"t" if current_run.is_some() => in_text = true,
                b"instrText" => in_instruction = true,
                b"drawing" | b"pict" if in_paragraph => {
                    image = Some(PendingImage {
                        relationship: String::new(),
                        paragraph: paragraphs.len() as u32,
                        width: 96.0,
                        height: 96.0,
                        x: None,
                        y: None,
                    });
                }
                b"positionH" => position_axis = Some(b'x'),
                b"positionV" => position_axis = Some(b'y'),
                b"posOffset" if position_axis.is_some() => {
                    in_position_offset = true;
                    position_text.clear();
                }
                _ => {
                    handle_empty_like(
                        &start,
                        in_paragraph_properties,
                        in_run_properties,
                        &mut paragraph_patch,
                        &mut paragraph_style_id,
                        &mut list_id,
                        &mut list_level,
                        current_run.as_mut(),
                        image.as_mut(),
                    );
                }
            },
            Ok(Event::Empty(start)) => {
                let qualified_name = start.name();
                let name = xml::local_name(qualified_name.as_ref());
                if name == b"gridCol" && table_depth == 1 {
                    table_grid.push(twip_num(&start, b"w", 1_440));
                } else if name == b"gridSpan" && table_depth == 1 {
                    cell_span = attr(&start, b"val")
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(1);
                } else if name == b"tcW" && table_depth == 1 {
                    cell_width = attr(&start, b"w")
                        .and_then(|value| value.parse::<u32>().ok())
                        .map(twip);
                } else if name == b"tab" && current_run.is_some() && !in_run_properties {
                    if let Some(run) = &mut current_run {
                        run.text.push('\t');
                    }
                } else if name == b"br" && current_run.is_some() {
                    if let Some(run) = &mut current_run {
                        if attr(&start, b"type").as_deref() == Some("page") {
                            run.text.push('\u{c}');
                        } else {
                            run.text.push('\n');
                        }
                    }
                } else {
                    handle_empty_like(
                        &start,
                        in_paragraph_properties,
                        in_run_properties,
                        &mut paragraph_patch,
                        &mut paragraph_style_id,
                        &mut list_id,
                        &mut list_level,
                        current_run.as_mut(),
                        image.as_mut(),
                    );
                }
            }
            Ok(Event::Text(text)) if in_text && !in_instruction => {
                if let Some(run) = &mut current_run {
                    run.text.push_str(&text.decode().map_err(|_| {
                        RenderError::malformed("document text is malformed; obtain a fresh copy")
                    })?);
                }
            }
            Ok(Event::Text(text)) if in_position_offset => {
                position_text.push_str(&text.decode().unwrap_or_default());
            }
            Ok(Event::End(end)) => match xml::local_name(end.name().as_ref()) {
                b"p" if in_paragraph => {
                    if let Some(run) = current_run.take() {
                        if !run.text.is_empty() {
                            pending_runs.push(run);
                        }
                    }
                    let resolved = styles.resolve(paragraph_style_id.as_deref());
                    let mut paragraph_style = resolved.paragraph;
                    paragraph_patch.apply_to(&mut paragraph_style);
                    let mut runs = pending_runs
                        .drain(..)
                        .map(|pending| {
                            let mut style = resolved.text.clone();
                            if let Some(style_id) = pending.style_id.as_deref() {
                                styles.apply_character_style(style_id, &mut style);
                            }
                            pending.patch.apply_to(&mut style);
                            FlowRun {
                                text: pending.text,
                                style,
                            }
                        })
                        .collect::<Vec<_>>();
                    if let Some(num_id) = list_id {
                        if let Some(label) = numbering.label(num_id, list_level, &mut counters) {
                            runs.insert(
                                0,
                                FlowRun {
                                    text: format!("{label}\t"),
                                    style: resolved.text.clone(),
                                },
                            );
                        }
                    }
                    if runs.is_empty() {
                        runs.push(FlowRun {
                            text: String::new(),
                            style: resolved.text.clone(),
                        });
                    }
                    paragraphs.push(FlowParagraph {
                        runs,
                        style: paragraph_style,
                    });
                    in_paragraph = false;
                }
                b"pPr" => in_paragraph_properties = false,
                b"rPr" => in_run_properties = false,
                b"r" => {
                    if let Some(run) = current_run.take() {
                        if !run.text.is_empty() {
                            pending_runs.push(run);
                        }
                    }
                }
                b"t" => in_text = false,
                b"instrText" => in_instruction = false,
                b"drawing" | b"pict" => {
                    if let Some(image) = image.take().filter(|image| !image.relationship.is_empty())
                    {
                        images.push(image);
                    }
                }
                b"positionH" | b"positionV" => position_axis = None,
                b"posOffset" => {
                    if let (Some(axis), Some(image), Ok(value)) =
                        (position_axis, image.as_mut(), position_text.parse::<f32>())
                    {
                        let value = emu_value(value);
                        if axis == b'x' {
                            image.x = Some(value);
                        } else {
                            image.y = Some(value);
                        }
                    }
                    in_position_offset = false;
                }
                b"tc" if table_depth == 1 => {
                    let start = cell_start
                        .take()
                        .unwrap_or(paragraphs.len())
                        .min(paragraphs.len());
                    row_cells.push(TableCell {
                        paragraphs: paragraphs.drain(start..).collect(),
                        width: cell_width,
                        span: cell_span,
                    });
                }
                b"tr" if table_depth == 1 => {
                    if !row_cells.is_empty() {
                        let mut runs = Vec::<FlowRun>::new();
                        let mut paragraph_style = row_cells
                            .iter()
                            .flat_map(|cell| cell.paragraphs.first())
                            .next()
                            .map(|paragraph| paragraph.style.clone())
                            .unwrap_or_default();
                        let mut widths = Vec::new();
                        let mut grid_cursor = 0_usize;
                        for (cell_index, cell) in row_cells.drain(..).enumerate() {
                            if cell_index > 0 {
                                runs.push(FlowRun {
                                    text: "\t".into(),
                                    style: runs
                                        .last()
                                        .map(|run| run.style.clone())
                                        .unwrap_or_else(crate::flow::default_text_style),
                                });
                            }
                            for (paragraph_index, paragraph) in
                                cell.paragraphs.into_iter().enumerate()
                            {
                                if paragraph_index > 0 {
                                    runs.push(FlowRun {
                                        text: "\n".into(),
                                        style: paragraph
                                            .runs
                                            .first()
                                            .map(|run| run.style.clone())
                                            .unwrap_or_else(crate::flow::default_text_style),
                                    });
                                }
                                runs.extend(paragraph.runs);
                            }
                            let span = cell.span.max(1) as usize;
                            let grid_width =
                                table_grid.iter().skip(grid_cursor).take(span).sum::<f32>();
                            widths.push(cell.width.unwrap_or_else(|| grid_width.max(96.0)));
                            grid_cursor = grid_cursor.saturating_add(span);
                        }
                        let mut cumulative = 0.0;
                        paragraph_style.tabs = widths
                            .iter()
                            .take(widths.len().saturating_sub(1))
                            .map(|width| {
                                cumulative += *width;
                                cumulative
                            })
                            .collect();
                        paragraph_style.after = 0.0;
                        let paragraph = paragraphs.len() as u32;
                        paragraphs.push(FlowParagraph {
                            runs,
                            style: paragraph_style,
                        });
                        table_rows.push(TableRow { paragraph, widths });
                    }
                }
                b"tbl" => table_depth = table_depth.saturating_sub(1),
                _ => {}
            },
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => {
                return Err(RenderError::malformed(
                    "word/document.xml is malformed; obtain a fresh copy",
                ));
            }
        }
    }
    // Re-scan the small XML part for section geometry without complicating the
    // paragraph state machine; both passes are linear and deterministic.
    parse_geometry(bytes, &mut width, &mut height, &mut margins)?;
    Ok(ParsedDocument {
        paragraphs,
        size: Size { width, height },
        margins,
        images,
        tables: table_rows,
    })
}

fn paint_tables(rows: &[TableRow], margins: Margins, rendered: &mut crate::Rendered) {
    for row in rows {
        let mut target = None;
        for (page_index, page) in rendered.pages.iter().enumerate() {
            if let Some(baseline) = paragraph_baseline(&page.items, row.paragraph) {
                target = Some((page_index, baseline));
                break;
            }
        }
        let Some((page_index, baseline)) = target else {
            continue;
        };
        let Some(page) = rendered.pages.get_mut(page_index) else {
            continue;
        };
        let height = 22.0;
        let mut x = margins.0;
        for width in &row.widths {
            let rect = Rect {
                x,
                y: (baseline - 16.0).max(0.0),
                width: *width,
                height,
            };
            page.items.push(Item::Path(crate::model::PathItem {
                path: crate::model::rect_path(rect),
                fill: None,
                stroke: Some(crate::model::Stroke {
                    paint: crate::model::Paint {
                        colour: crate::model::Colour::BLACK,
                    },
                    width: 1.0,
                    dash: Vec::new(),
                }),
                source: Some(SourceRef::Text {
                    paragraph: row.paragraph,
                    start: 0,
                    end: 0,
                }),
            }));
            x += *width;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_empty_like(
    start: &BytesStart<'_>,
    in_paragraph_properties: bool,
    in_run_properties: bool,
    paragraph_patch: &mut ParagraphPatch,
    paragraph_style_id: &mut Option<String>,
    list_id: &mut Option<u32>,
    list_level: &mut u32,
    run: Option<&mut PendingRun>,
    image: Option<&mut PendingImage>,
) {
    let qualified_name = start.name();
    let name = xml::local_name(qualified_name.as_ref());
    if in_paragraph_properties {
        match name {
            b"pStyle" => *paragraph_style_id = attr(start, b"val"),
            b"numId" => *list_id = attr(start, b"val").and_then(|value| value.parse().ok()),
            b"ilvl" => {
                *list_level = attr(start, b"val")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0)
            }
            _ => apply_paragraph_property(start, paragraph_patch),
        }
    }
    if in_run_properties {
        if let Some(run) = run {
            if name == b"rStyle" {
                run.style_id = attr(start, b"val");
            } else {
                apply_run_property(start, &mut run.patch);
            }
        }
    }
    if let Some(image) = image {
        match name {
            b"extent" => {
                image.width = attr(start, b"cx")
                    .and_then(|value| value.parse::<f32>().ok())
                    .map(emu_value)
                    .unwrap_or(image.width);
                image.height = attr(start, b"cy")
                    .and_then(|value| value.parse::<f32>().ok())
                    .map(emu_value)
                    .unwrap_or(image.height);
            }
            b"blip" | b"imagedata" => {
                image.relationship = attr_exact(start, b"r:embed")
                    .or_else(|| attr_exact(start, b"r:id"))
                    .unwrap_or_default();
            }
            b"simplePos" => {
                image.x = attr(start, b"x")
                    .and_then(|value| value.parse::<f32>().ok())
                    .map(emu_value);
                image.y = attr(start, b"y")
                    .and_then(|value| value.parse::<f32>().ok())
                    .map(emu_value);
            }
            _ => {}
        }
    }
}

fn parse_geometry(
    bytes: &[u8],
    width: &mut f32,
    height: &mut f32,
    margins: &mut Margins,
) -> Result<(), RenderError> {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    loop {
        match reader.read_event() {
            Ok(Event::Empty(start)) | Ok(Event::Start(start))
                if xml::local_name(start.name().as_ref()) == b"pgSz" =>
            {
                *width = twip(
                    attr(&start, b"w")
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(12_240),
                );
                *height = twip(
                    attr(&start, b"h")
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(15_840),
                );
            }
            Ok(Event::Empty(start)) | Ok(Event::Start(start))
                if xml::local_name(start.name().as_ref()) == b"pgMar" =>
            {
                *margins = (
                    twip_num(&start, b"left", 1_440),
                    twip_num(&start, b"top", 1_440),
                    twip_num(&start, b"right", 1_440),
                    twip_num(&start, b"bottom", 1_440),
                );
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err(RenderError::malformed("section geometry is malformed")),
        }
    }
    Ok(())
}

#[derive(Default)]
struct Relationships {
    targets: BTreeMap<String, String>,
    external: Vec<String>,
}

impl Relationships {
    fn parse(bytes: Option<&[u8]>, base: &str) -> Result<Self, RenderError> {
        let Some(bytes) = bytes else {
            return Ok(Self::default());
        };
        let mut reader = quick_xml::Reader::from_reader(bytes);
        let mut output = Self::default();
        loop {
            match reader.read_event() {
                Ok(Event::Empty(start)) | Ok(Event::Start(start))
                    if xml::local_name(start.name().as_ref()) == b"Relationship" =>
                {
                    if let Some(target) = attr(&start, b"Target") {
                        if attr(&start, b"TargetMode").as_deref() == Some("External") {
                            output.external.push(target);
                        } else if let Some(id) = attr(&start, b"Id") {
                            output
                                .targets
                                .insert(id, resolve_relationship(base, &target)?);
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(_) => return Err(RenderError::malformed("Word relationships are malformed")),
            }
        }
        Ok(output)
    }
}

#[derive(Clone, Debug)]
struct NumberLevel {
    start: u32,
    format: String,
    text: String,
}

#[derive(Default)]
struct Numbering {
    nums: BTreeMap<u32, u32>,
    levels: BTreeMap<(u32, u32), NumberLevel>,
}

impl Numbering {
    fn parse(bytes: Option<&[u8]>) -> Result<Self, RenderError> {
        let Some(bytes) = bytes else {
            return Ok(Self::default());
        };
        let mut reader = quick_xml::Reader::from_reader(bytes);
        let mut output = Self::default();
        let mut abstract_id = None;
        let mut num_id = None;
        let mut level_id = None;
        let mut level = NumberLevel {
            start: 1,
            format: "decimal".into(),
            text: "%1.".into(),
        };
        loop {
            match reader.read_event() {
                Ok(Event::Start(start)) => match xml::local_name(start.name().as_ref()) {
                    b"abstractNum" => {
                        abstract_id =
                            attr(&start, b"abstractNumId").and_then(|value| value.parse().ok())
                    }
                    b"num" => num_id = attr(&start, b"numId").and_then(|value| value.parse().ok()),
                    b"lvl" => {
                        level_id = attr(&start, b"ilvl").and_then(|value| value.parse().ok());
                        level = NumberLevel {
                            start: 1,
                            format: "decimal".into(),
                            text: "%1.".into(),
                        };
                    }
                    _ => apply_numbering_value(&start, num_id, &mut output, &mut level),
                },
                Ok(Event::Empty(start)) => {
                    apply_numbering_value(&start, num_id, &mut output, &mut level)
                }
                Ok(Event::End(end)) => match xml::local_name(end.name().as_ref()) {
                    b"lvl" => {
                        if let (Some(abstract_id), Some(level_id)) = (abstract_id, level_id.take())
                        {
                            output.levels.insert((abstract_id, level_id), level.clone());
                        }
                    }
                    b"abstractNum" => abstract_id = None,
                    b"num" => num_id = None,
                    _ => {}
                },
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(_) => return Err(RenderError::malformed("word/numbering.xml is malformed")),
            }
        }
        Ok(output)
    }

    fn label(
        &self,
        num_id: u32,
        level_id: u32,
        counters: &mut BTreeMap<(u32, u32), u32>,
    ) -> Option<String> {
        let abstract_id = *self.nums.get(&num_id)?;
        let level = self.levels.get(&(abstract_id, level_id))?;
        let counter = counters.entry((num_id, level_id)).or_insert(level.start);
        let value = *counter;
        *counter = counter.saturating_add(1);
        if level.format == "bullet" {
            return Some(level.text.clone());
        }
        let formatted = format_number(value, &level.format);
        Some(
            level
                .text
                .replace(&format!("%{}", level_id + 1), &formatted),
        )
    }
}

fn apply_numbering_value(
    start: &BytesStart<'_>,
    num_id: Option<u32>,
    output: &mut Numbering,
    level: &mut NumberLevel,
) {
    match xml::local_name(start.name().as_ref()) {
        b"abstractNumId" => {
            if let (Some(num_id), Some(abstract_id)) = (
                num_id,
                attr(start, b"val").and_then(|value| value.parse().ok()),
            ) {
                output.nums.insert(num_id, abstract_id);
            }
        }
        b"start" => {
            level.start = attr(start, b"val")
                .and_then(|value| value.parse().ok())
                .unwrap_or(1)
        }
        b"numFmt" => level.format = attr(start, b"val").unwrap_or_else(|| "decimal".into()),
        b"lvlText" => level.text = attr(start, b"val").unwrap_or_else(|| "%1.".into()),
        _ => {}
    }
}

fn format_number(value: u32, format: &str) -> String {
    match format {
        "lowerLetter" => letters(value, false),
        "upperLetter" => letters(value, true),
        "lowerRoman" => roman(value).to_ascii_lowercase(),
        "upperRoman" => roman(value),
        _ => value.to_string(),
    }
}

fn letters(mut value: u32, uppercase: bool) -> String {
    let mut output = String::new();
    while value > 0 {
        value -= 1;
        let character = char::from_u32(u32::from(b'a') + value % 26).unwrap_or('a');
        output.insert(0, character);
        value /= 26;
    }
    if uppercase {
        output.to_ascii_uppercase()
    } else {
        output
    }
}

fn roman(mut value: u32) -> String {
    let mut output = String::new();
    for (amount, symbols) in [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ] {
        while value >= amount {
            output.push_str(symbols);
            value -= amount;
        }
    }
    output
}

fn check_table_spans(bytes: &[u8], cell_limit: u64) -> Result<(), RenderError> {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    loop {
        match reader.read_event() {
            Ok(Event::Empty(start)) | Ok(Event::Start(start))
                if xml::local_name(start.name().as_ref()) == b"gridSpan" =>
            {
                let span = attr(&start, b"val")
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(1);
                if span > cell_limit {
                    return Err(RenderError::limit("cells", span));
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => {
                return Err(RenderError::malformed(
                    "a Word table is malformed; obtain a fresh copy",
                ));
            }
        }
    }
    Ok(())
}

fn twip(value: u32) -> f32 {
    value as f32 / 1440.0 * 96.0
}

fn twip_num(start: &BytesStart<'_>, name: &[u8], default: u32) -> f32 {
    twip(
        attr(start, name)
            .and_then(|value| value.parse().ok())
            .unwrap_or(default),
    )
}

fn emu_value(value: f32) -> f32 {
    value / 914_400.0 * 96.0
}

fn attr(start: &BytesStart<'_>, name: &[u8]) -> Option<String> {
    start
        .attributes()
        .with_checks(false)
        .flatten()
        .find(|attribute| xml::local_name(attribute.key.as_ref()) == name)
        .map(|attribute| String::from_utf8_lossy(attribute.value.as_ref()).into_owned())
}

fn attr_exact(start: &BytesStart<'_>, name: &[u8]) -> Option<String> {
    start
        .attributes()
        .with_checks(false)
        .flatten()
        .find(|attribute| attribute.key.as_ref() == name)
        .map(|attribute| String::from_utf8_lossy(attribute.value.as_ref()).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_number_formats_cover_letters_and_roman_numerals() {
        assert_eq!(format_number(28, "lowerLetter"), "ab");
        assert_eq!(format_number(9, "upperRoman"), "IX");
        assert_eq!(format_number(14, "lowerRoman"), "xiv");
    }
}
