use crate::container::{resolve_relationship, xml, zip::Archive};
use crate::flow::styles::{
    ParagraphPatch, RunPatch, StyleSheet, apply_paragraph_property, apply_run_property,
};
use crate::flow::{
    Border, BorderEdges, FlowBlock, FlowCell, FlowParagraph, FlowRow, FlowRun, FlowTable,
    VerticalAlignment, VerticalMerge, layout_blocks,
};
use crate::model::{Colour, ImageData, ImageItem, Item, Rect, Size, SourceRef, Unrendered};
use crate::{Format, Options, RenderError};
use quick_xml::events::{BytesStart, Event};
use std::collections::{BTreeMap, BTreeSet};

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
            parsed.blocks.extend(notes.blocks);
        }
    }

    let mut rendered = layout_blocks(
        &parsed.blocks,
        Format::Docx,
        options,
        parsed.size,
        parsed.margins,
    )?;
    paint_repeating_parts(
        &RepeatingPartContext {
            archive: &archive,
            document,
            relationships: &relationships.targets,
            styles: &styles,
            numbering: &numbering,
            options,
        },
        &mut rendered,
        parsed.margins,
        parsed.header_offset,
        parsed.footer_offset,
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

struct RepeatingPartContext<'a, 'font> {
    archive: &'a Archive,
    document: &'a [u8],
    relationships: &'a BTreeMap<String, String>,
    styles: &'a StyleSheet,
    numbering: &'a Numbering,
    options: &'a Options<'font>,
}

fn paint_repeating_parts(
    context: &RepeatingPartContext<'_, '_>,
    rendered: &mut crate::Rendered,
    margins: Margins,
    header_offset: f32,
    footer_offset: f32,
) -> Result<(), RenderError> {
    let parts = parse_repeating_parts(context.document, context.relationships)?;
    let names = parts.targets().map(str::to_owned).collect::<BTreeSet<_>>();
    let mut laid_out = BTreeMap::<String, Vec<Item>>::new();
    let Some(page_size) = rendered.pages.first().map(|page| page.size) else {
        return Ok(());
    };
    for name in names {
        let Some(bytes) = context.archive.get(&name) else {
            continue;
        };
        let parsed = parse_part(bytes, context.styles, context.numbering)?;
        let header = name.starts_with("word/header");
        // A header starts `w:header` px below the top edge and may run down to
        // the text margin; a footer starts `w:footer` px above the bottom edge.
        let part_margins = if header {
            (
                margins.0,
                header_offset,
                margins.2,
                (page_size.height - margins.1).max(0.0),
            )
        } else {
            (
                margins.0,
                (page_size.height - footer_offset).max(0.0),
                margins.2,
                0.0,
            )
        };
        let part = layout_blocks(
            &parsed.blocks,
            Format::Docx,
            context.options,
            page_size,
            part_margins,
        )?;
        if let Some(part_page) = part.pages.first() {
            laid_out.insert(name, part_page.items.clone());
        }
    }
    for (page_index, page) in rendered.pages.iter_mut().enumerate() {
        let even = (page_index + 1) % 2 == 0;
        for target in [parts.header(even), parts.footer(even)]
            .into_iter()
            .flatten()
        {
            if let Some(items) = laid_out.get(target) {
                page.items.extend(items.clone());
            }
        }
    }
    Ok(())
}

#[derive(Default)]
struct RepeatingParts {
    odd_header: Option<String>,
    even_header: Option<String>,
    odd_footer: Option<String>,
    even_footer: Option<String>,
}

impl RepeatingParts {
    fn header(&self, even: bool) -> Option<&str> {
        if even {
            self.even_header.as_deref().or(self.odd_header.as_deref())
        } else {
            self.odd_header.as_deref()
        }
    }

    fn footer(&self, even: bool) -> Option<&str> {
        if even {
            self.even_footer.as_deref().or(self.odd_footer.as_deref())
        } else {
            self.odd_footer.as_deref()
        }
    }

    fn targets(&self) -> impl Iterator<Item = &str> {
        [
            self.odd_header.as_deref(),
            self.even_header.as_deref(),
            self.odd_footer.as_deref(),
            self.even_footer.as_deref(),
        ]
        .into_iter()
        .flatten()
    }
}

fn parse_repeating_parts(
    document: &[u8],
    relationships: &BTreeMap<String, String>,
) -> Result<RepeatingParts, RenderError> {
    let mut reader = quick_xml::Reader::from_reader(document);
    let mut parts = RepeatingParts::default();
    loop {
        match reader.read_event() {
            Ok(Event::Empty(start)) | Ok(Event::Start(start)) => {
                let qualified_name = start.name();
                let name = xml::local_name(qualified_name.as_ref());
                if name != b"headerReference" && name != b"footerReference" {
                    continue;
                }
                let Some(target) = attr_exact(&start, b"r:id")
                    .and_then(|id| relationships.get(&id))
                    .cloned()
                else {
                    continue;
                };
                let even = attr(&start, b"type").as_deref() == Some("even");
                if name == b"headerReference" {
                    if even {
                        parts.even_header = Some(target);
                    } else {
                        parts.odd_header = Some(target);
                    }
                } else if even {
                    parts.even_footer = Some(target);
                } else {
                    parts.odd_footer = Some(target);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err(RenderError::malformed("section references are malformed")),
        }
    }
    Ok(parts)
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
    blocks: Vec<FlowBlock>,
    size: Size,
    margins: Margins,
    /// Distance from the page edge to the running header's text, and to the
    /// footer's, from `w:pgMar`.
    header_offset: f32,
    footer_offset: f32,
    images: Vec<PendingImage>,
}

#[derive(Default)]
struct PendingRun {
    text: String,
    patch: RunPatch,
    style_id: Option<String>,
}

/// The six rules a `w:tblBorders` declares. The four outer ones apply to the
/// table's edges and the two inside ones to every seam between cells, which is
/// why a cell cannot resolve its own borders without the table's.
#[derive(Clone, Copy, Debug, Default)]
struct TableBorders {
    top: Option<Border>,
    left: Option<Border>,
    bottom: Option<Border>,
    right: Option<Border>,
    inside_horizontal: Option<Border>,
    inside_vertical: Option<Border>,
}

/// A `w:tcBorders` edge, where "absent" and "explicitly none" differ: the first
/// falls back to the table's rule and the second suppresses it.
#[derive(Clone, Copy, Debug, Default)]
struct BorderOverrides {
    top: Option<Option<Border>>,
    left: Option<Option<Border>>,
    bottom: Option<Option<Border>>,
    right: Option<Option<Border>>,
}

#[derive(Default)]
struct PendingCell {
    paragraphs: Vec<FlowParagraph>,
    column: usize,
    span: usize,
    merge: VerticalMerge,
    vertical_alignment: VerticalAlignment,
    overrides: BorderOverrides,
    shading: Option<Colour>,
    /// `w:tcW` in pixels, used only where `w:tblGrid` is absent.
    width: Option<f32>,
}

#[derive(Default)]
struct PendingRow {
    cells: Vec<PendingCell>,
    minimum_height: f32,
    /// Where the next cell starts in the grid, advanced by each `w:gridSpan`.
    next_column: usize,
}

struct TableBuilder {
    grid: Vec<f32>,
    borders: TableBorders,
    indent: f32,
    /// Left, top, right, bottom padding inside every cell.
    cell_margins: (f32, f32, f32, f32),
    rows: Vec<PendingRow>,
}

impl Default for TableBuilder {
    fn default() -> Self {
        Self {
            grid: Vec::new(),
            borders: TableBorders::default(),
            indent: 0.0,
            // ECMA-376's default cell padding is 108 twips left and right and
            // nothing above or below.
            cell_margins: (7.2, 0.0, 7.2, 0.0),
            rows: Vec::new(),
        }
    }
}

impl TableBuilder {
    /// Resolves the pending rows into a laid-out-ready table.
    ///
    /// Column widths come from `w:tblGrid`; a table that omits it — legal, and
    /// what an auto-width table written by a converter looks like — falls back
    /// to the widest `w:tcW` seen in each column so the columns still line up
    /// with each other.
    fn finish(self) -> FlowTable {
        let mut columns = self.grid;
        for row in &self.rows {
            for cell in &row.cells {
                let span = cell.span.max(1);
                let end = cell.column.saturating_add(span);
                if columns.len() < end {
                    columns.resize(end, 0.0);
                }
                let Some(width) = cell.width else { continue };
                let declared = columns
                    .get(cell.column..end)
                    .map(|slice| slice.iter().sum::<f32>())
                    .unwrap_or(0.0);
                if declared > 0.0 {
                    continue;
                }
                if let Some(column) = columns.get_mut(cell.column) {
                    *column = column.max(width / span as f32);
                }
                for offset in 1..span {
                    if let Some(column) = columns.get_mut(cell.column + offset) {
                        *column = column.max(width / span as f32);
                    }
                }
            }
        }
        let last_column = columns.len();
        let rows = self
            .rows
            .into_iter()
            .enumerate()
            .map(|(row_index, row)| {
                let first_row = row_index == 0;
                let cells = row
                    .cells
                    .into_iter()
                    .map(|cell| {
                        let span = cell.span.max(1);
                        let first_column = cell.column == 0;
                        let last = cell.column.saturating_add(span) >= last_column;
                        FlowCell {
                            borders: BorderEdges {
                                top: cell.overrides.top.unwrap_or(if first_row {
                                    self.borders.top
                                } else {
                                    self.borders.inside_horizontal
                                }),
                                left: cell.overrides.left.unwrap_or(if first_column {
                                    self.borders.left
                                } else {
                                    self.borders.inside_vertical
                                }),
                                bottom: cell.overrides.bottom.unwrap_or(self.borders.bottom),
                                right: cell.overrides.right.unwrap_or(if last {
                                    self.borders.right
                                } else {
                                    self.borders.inside_vertical
                                }),
                            },
                            paragraphs: cell.paragraphs,
                            column: cell.column,
                            span,
                            merge: cell.merge,
                            vertical_alignment: cell.vertical_alignment,
                            shading: cell.shading,
                        }
                    })
                    .collect();
                FlowRow {
                    cells,
                    minimum_height: row.minimum_height,
                }
            })
            .collect::<Vec<FlowRow>>();
        // The bottom rule of every row but the last is the inside horizontal
        // one, and the row below draws it too; letting both draw it keeps a
        // single-row table's bottom edge without special-casing the seam.
        let mut rows = rows;
        let row_count = rows.len();
        for (row_index, row) in rows.iter_mut().enumerate() {
            if row_index + 1 == row_count {
                continue;
            }
            for cell in &mut row.cells {
                cell.borders.bottom = None;
            }
        }
        FlowTable {
            rows,
            columns,
            indent: self.indent,
            cell_margin_left: self.cell_margins.0,
            cell_margin_top: self.cell_margins.1,
            cell_margin_right: self.cell_margins.2,
            cell_margin_bottom: self.cell_margins.3,
        }
    }
}

/// A `w:sz` is in eighths of a point; at 96 dpi that is `sz / 6` pixels.
/// `w:val="nil"` and `"none"` are the absence of a rule, not a thin one.
fn parse_border(start: &BytesStart<'_>) -> Option<Border> {
    match attr(start, b"val").as_deref() {
        Some("nil") | Some("none") | None => None,
        Some(_) => Some(Border {
            width: attr(start, b"sz")
                .and_then(|value| value.parse::<f32>().ok())
                .map(|eighths| eighths / 6.0)
                .unwrap_or(1.0)
                .max(0.5),
            colour: attr(start, b"color")
                .and_then(|value| border_colour(&value))
                .unwrap_or(Colour::BLACK),
        }),
    }
}

fn border_colour(value: &str) -> Option<Colour> {
    if value.eq_ignore_ascii_case("auto") || value.len() != 6 {
        return None;
    }
    Some(Colour {
        r: u8::from_str_radix(&value[0..2], 16).ok()?,
        g: u8::from_str_radix(&value[2..4], 16).ok()?,
        b: u8::from_str_radix(&value[4..6], 16).ok()?,
        a: 255,
    })
}

/// Which part of a table the parser is inside. Cell padding and border rules
/// share element names — `w:top` means both — so the scope has to be tracked
/// rather than inferred from the name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TableScope {
    None,
    TableBorders,
    CellMargins,
    CellBorders,
    /// `w:tcMar` and `w:tblPrEx`, which use the same element names and which
    /// this renderer does not apply.
    Ignored,
}

fn parse_part(
    bytes: &[u8],
    styles: &StyleSheet,
    numbering: &Numbering,
) -> Result<ParsedDocument, RenderError> {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut blocks = Vec::<FlowBlock>::new();
    let mut pending_runs = Vec::<PendingRun>::new();
    let mut current_run = None::<PendingRun>;
    let mut paragraph_patch = ParagraphPatch::default();
    let mut paragraph_style_id = None::<String>;
    let mut list_id = None::<u32>;
    let mut list_level = 0_u32;
    let mut in_paragraph = false;
    let mut in_paragraph_properties = false;
    // `w:pPr/w:rPr` is the paragraph *mark's* formatting, and `w:vanish` there
    // hides the whole paragraph rather than one run.
    let mut in_paragraph_mark_properties = false;
    let mut paragraph_hidden = false;
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
    let mut table = TableBuilder::default();
    let mut row = PendingRow::default();
    let mut cell = None::<PendingCell>;
    let mut table_scope = TableScope::None;
    // The index every paragraph is known by, counted in document order across
    // cells as well as the body, so an image anchored to a paragraph inside a
    // table still finds it after layout.
    let mut paragraph_index = 0_u32;
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => match xml::local_name(start.name().as_ref()) {
                b"tbl" => {
                    table_depth = table_depth.saturating_add(1);
                    if table_depth == 1 {
                        table = TableBuilder::default();
                    }
                }
                b"tr" if table_depth == 1 => row = PendingRow::default(),
                b"tc" if table_depth == 1 => {
                    cell = Some(PendingCell {
                        column: row.next_column,
                        span: 1,
                        ..PendingCell::default()
                    });
                }
                b"tblBorders" if table_depth == 1 => table_scope = TableScope::TableBorders,
                b"tblCellMar" if table_depth == 1 => table_scope = TableScope::CellMargins,
                b"tcBorders" if table_depth == 1 => table_scope = TableScope::CellBorders,
                b"tcMar" | b"tblPrEx" | b"pBdr" => table_scope = TableScope::Ignored,
                b"p" => {
                    in_paragraph = true;
                    pending_runs.clear();
                    paragraph_patch = ParagraphPatch::default();
                    paragraph_style_id = None;
                    paragraph_hidden = false;
                    list_id = None;
                    list_level = 0;
                }
                b"pPr" if in_paragraph => in_paragraph_properties = true,
                b"rPr" if in_paragraph_properties => in_paragraph_mark_properties = true,
                b"r" if in_paragraph => current_run = Some(PendingRun::default()),
                b"rPr" if current_run.is_some() => in_run_properties = true,
                b"t" if current_run.is_some() => in_text = true,
                b"instrText" => in_instruction = true,
                b"drawing" | b"pict" if in_paragraph => {
                    image = Some(PendingImage {
                        relationship: String::new(),
                        paragraph: paragraph_index,
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
                if table_depth == 1
                    && apply_table_property(
                        &start,
                        name,
                        table_scope,
                        &mut table,
                        &mut row,
                        cell.as_mut(),
                    )
                {
                    // Consumed as table geometry.
                } else if in_paragraph_mark_properties && matches!(name, b"vanish" | b"specVanish")
                {
                    paragraph_hidden = attr(&start, b"val").as_deref() != Some("0")
                        && attr(&start, b"val").as_deref() != Some("false");
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
            Ok(Event::GeneralRef(reference)) if in_text && !in_instruction => {
                if let Some(run) = &mut current_run {
                    run.text.push_str(&xml::decode_reference(&reference)?);
                }
            }
            Ok(Event::GeneralRef(reference)) if in_position_offset => {
                position_text.push_str(&xml::decode_reference(&reference)?);
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
                        .filter(|pending| !pending.patch.hidden())
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
                    if let Some((label, suffix)) = list_id
                        .and_then(|num_id| numbering.label(num_id, list_level, &mut counters))
                    {
                        runs.insert(
                            0,
                            FlowRun {
                                text: format!("{label}{suffix}"),
                                style: resolved.text.clone(),
                            },
                        );
                    }
                    if runs.is_empty() {
                        runs.push(FlowRun {
                            text: String::new(),
                            style: resolved.text.clone(),
                        });
                    }
                    let paragraph = FlowParagraph {
                        runs,
                        style: paragraph_style,
                    };
                    if paragraph_hidden {
                        paragraph_index = paragraph_index.saturating_add(1);
                        in_paragraph = false;
                        continue;
                    }
                    match cell.as_mut() {
                        Some(cell) => cell.paragraphs.push(paragraph),
                        None => blocks.push(FlowBlock::Paragraph(paragraph)),
                    }
                    paragraph_index = paragraph_index.saturating_add(1);
                    in_paragraph = false;
                }
                b"pPr" => in_paragraph_properties = false,
                b"rPr" => {
                    in_paragraph_mark_properties = false;
                    in_run_properties = false;
                }
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
                b"tblBorders" | b"tblCellMar" | b"tcBorders" | b"tcMar" | b"tblPrEx" | b"pBdr" => {
                    table_scope = TableScope::None;
                }
                b"tc" if table_depth == 1 => {
                    if let Some(cell) = cell.take() {
                        row.next_column = cell.column.saturating_add(cell.span.max(1));
                        row.cells.push(cell);
                    }
                }
                b"tr" if table_depth == 1 => {
                    if !row.cells.is_empty() {
                        table.rows.push(std::mem::take(&mut row));
                    }
                }
                b"tbl" => {
                    if table_depth == 1 && !table.rows.is_empty() {
                        blocks.push(FlowBlock::Table(std::mem::take(&mut table).finish()));
                    }
                    table_depth = table_depth.saturating_sub(1);
                }
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
    let mut header_offset = 48.0;
    let mut footer_offset = 48.0;
    parse_geometry(
        bytes,
        &mut width,
        &mut height,
        &mut margins,
        &mut header_offset,
        &mut footer_offset,
    )?;
    Ok(ParsedDocument {
        blocks,
        size: Size { width, height },
        margins,
        header_offset,
        footer_offset,
        images,
    })
}

/// Applies one table, row or cell property, returning whether it was consumed.
///
/// The scope matters as much as the name: `w:top` is a border inside
/// `w:tblBorders` and a padding inside `w:tblCellMar`, and the two are not
/// interchangeable.
fn apply_table_property(
    start: &BytesStart<'_>,
    name: &[u8],
    scope: TableScope,
    table: &mut TableBuilder,
    row: &mut PendingRow,
    cell: Option<&mut PendingCell>,
) -> bool {
    match scope {
        TableScope::TableBorders => {
            let border = parse_border(start);
            match name {
                b"top" => table.borders.top = border,
                b"left" | b"start" => table.borders.left = border,
                b"bottom" => table.borders.bottom = border,
                b"right" | b"end" => table.borders.right = border,
                b"insideH" => table.borders.inside_horizontal = border,
                b"insideV" => table.borders.inside_vertical = border,
                _ => return false,
            }
            return true;
        }
        TableScope::CellMargins => {
            let value = twip_num(start, b"w", 0);
            match name {
                b"top" => table.cell_margins.1 = value,
                b"left" | b"start" => table.cell_margins.0 = value,
                b"bottom" => table.cell_margins.3 = value,
                b"right" | b"end" => table.cell_margins.2 = value,
                _ => return false,
            }
            return true;
        }
        TableScope::CellBorders => {
            let Some(cell) = cell else { return false };
            let border = Some(parse_border(start));
            match name {
                b"top" => cell.overrides.top = border,
                b"left" | b"start" => cell.overrides.left = border,
                b"bottom" => cell.overrides.bottom = border,
                b"right" | b"end" => cell.overrides.right = border,
                _ => return false,
            }
            return true;
        }
        TableScope::Ignored => {
            if matches!(
                name,
                b"top" | b"left" | b"start" | b"bottom" | b"right" | b"end"
            ) {
                return true;
            }
        }
        TableScope::None => {}
    }
    match name {
        b"gridCol" => table.grid.push(twip_num(start, b"w", 1_440)),
        b"tblInd" => table.indent = twip_num(start, b"w", 0),
        b"trHeight" => {
            row.minimum_height = attr(start, b"val")
                .and_then(|value| value.parse::<u32>().ok())
                .map(twip)
                .unwrap_or(row.minimum_height)
        }
        b"gridSpan" => {
            if let Some(cell) = cell {
                cell.span = attr(start, b"val")
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(1)
                    .max(1);
            }
        }
        b"tcW" => {
            if let Some(cell) = cell {
                // `w:type="pct"` and `"auto"` are proportions rather than
                // widths; only `dxa` is a measurement in twips.
                cell.width = match attr(start, b"type").as_deref() {
                    Some("dxa") | None => attr(start, b"w")
                        .and_then(|value| value.parse::<u32>().ok())
                        .map(twip)
                        .filter(|width| *width > 0.0),
                    Some(_) => None,
                };
            }
        }
        b"vMerge" => {
            if let Some(cell) = cell {
                cell.merge = match attr(start, b"val").as_deref() {
                    Some("restart") => VerticalMerge::Start,
                    Some("continue") | None => VerticalMerge::Continue,
                    Some(_) => VerticalMerge::Continue,
                };
            }
        }
        b"vAlign" => {
            if let Some(cell) = cell {
                cell.vertical_alignment = match attr(start, b"val").as_deref() {
                    Some("center") => VerticalAlignment::Centre,
                    Some("bottom") => VerticalAlignment::Bottom,
                    Some("top") | None => VerticalAlignment::Top,
                    Some(_) => VerticalAlignment::Top,
                };
            }
        }
        b"shd" => {
            if let Some(cell) = cell {
                cell.shading = match attr(start, b"val").as_deref() {
                    Some("nil") | Some("clear") | None => None,
                    Some(_) => attr(start, b"fill").and_then(|value| border_colour(&value)),
                };
            }
        }
        _ => return false,
    }
    true
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
    header_offset: &mut f32,
    footer_offset: &mut f32,
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
                // `w:header` and `w:footer` are the distance from the page edge
                // to the repeating part, and the section declares them. Fixing
                // the header at 24 px put the NIST running head 24 px above
                // LibreOffice's on all 34 pages, because that section asks for
                // 720 twips.
                *header_offset = twip_num(&start, b"header", 720);
                *footer_offset = twip_num(&start, b"footer", 720);
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
    /// `w:suff`: what separates the label from the paragraph. `tab` is the
    /// default; the NIST chapter headings ask for `space`, and a tab there
    /// pushed the heading to the next default stop.
    suffix: &'static str,
}

#[derive(Default)]
struct Numbering {
    nums: BTreeMap<u32, u32>,
    levels: BTreeMap<(u32, u32), NumberLevel>,
    /// `w:startOverride` per numbering instance, which is how one abstract
    /// definition is reused with a different first number.
    starts: BTreeMap<(u32, u32), u32>,
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
        let mut level = NumberLevel::default();
        let mut override_level = None::<u32>;
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
                        level = NumberLevel::default();
                    }
                    b"lvlOverride" => {
                        override_level = attr(&start, b"ilvl").and_then(|value| value.parse().ok())
                    }
                    _ => apply_numbering_value(
                        &start,
                        num_id,
                        override_level,
                        &mut output,
                        &mut level,
                    ),
                },
                Ok(Event::Empty(start)) => {
                    apply_numbering_value(&start, num_id, override_level, &mut output, &mut level)
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
                    b"lvlOverride" => override_level = None,
                    _ => {}
                },
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(_) => return Err(RenderError::malformed("word/numbering.xml is malformed")),
            }
        }
        Ok(output)
    }

    /// The first number this instance uses at `level_id`, honouring
    /// `w:startOverride`.
    fn start(&self, num_id: u32, abstract_id: u32, level_id: u32) -> u32 {
        self.starts
            .get(&(num_id, level_id))
            .copied()
            .or_else(|| {
                self.levels
                    .get(&(abstract_id, level_id))
                    .map(|level| level.start)
            })
            .unwrap_or(1)
    }

    /// The label for one numbered paragraph, and what separates it from the
    /// text.
    ///
    /// `w:lvlText` is a template over *every* level, not just this one:
    /// `%1.%2.` at level 1 is "chapter dot section dot", and substituting only
    /// `%2` left the chapter number as a literal `%1`. Advancing a level also
    /// restarts the ones below it, which is what makes 2.2.1 follow 2.2 rather
    /// than continuing from wherever the previous section left off.
    fn label(
        &self,
        num_id: u32,
        level_id: u32,
        counters: &mut BTreeMap<(u32, u32), u32>,
    ) -> Option<(String, &'static str)> {
        let abstract_id = *self.nums.get(&num_id)?;
        let level = self.levels.get(&(abstract_id, level_id))?;
        if level.format == "none" {
            return None;
        }
        let counter = counters
            .entry((num_id, level_id))
            .or_insert_with(|| self.start(num_id, abstract_id, level_id));
        let value = *counter;
        *counter = counter.saturating_add(1);
        for deeper in level_id.saturating_add(1)..=MAX_LIST_LEVEL {
            counters.remove(&(num_id, deeper));
        }
        if level.format == "bullet" {
            return Some((level.text.clone(), level.suffix));
        }
        let mut text = level.text.clone();
        for ancestor in 0..=level_id {
            let Some(ancestor_level) = self.levels.get(&(abstract_id, ancestor)) else {
                continue;
            };
            let ancestor_value = if ancestor == level_id {
                value
            } else {
                counters
                    .get(&(num_id, ancestor))
                    .map(|next| next.saturating_sub(1))
                    .unwrap_or_else(|| self.start(num_id, abstract_id, ancestor))
            };
            text = text.replace(
                &format!("%{}", ancestor + 1),
                &format_number(ancestor_value, &ancestor_level.format),
            );
        }
        Some((text, level.suffix))
    }
}

/// `w:ilvl` is 0 to 8 in ECMA-376.
const MAX_LIST_LEVEL: u32 = 8;

impl Default for NumberLevel {
    fn default() -> Self {
        Self {
            start: 1,
            format: "decimal".into(),
            text: "%1.".into(),
            suffix: "\t",
        }
    }
}

fn apply_numbering_value(
    start: &BytesStart<'_>,
    num_id: Option<u32>,
    override_level: Option<u32>,
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
        b"startOverride" => {
            if let (Some(num_id), Some(level_id), Some(value)) = (
                num_id,
                override_level,
                attr(start, b"val").and_then(|value| value.parse().ok()),
            ) {
                output.starts.insert((num_id, level_id), value);
            }
        }
        b"start" => {
            level.start = attr(start, b"val")
                .and_then(|value| value.parse().ok())
                .unwrap_or(1)
        }
        b"numFmt" => level.format = attr(start, b"val").unwrap_or_else(|| "decimal".into()),
        b"lvlText" => level.text = attr(start, b"val").unwrap_or_else(|| "%1.".into()),
        b"suff" => {
            level.suffix = match attr(start, b"val").as_deref() {
                Some("space") => " ",
                Some("nothing") => "",
                Some("tab") | None => "\t",
                Some(_) => "\t",
            }
        }
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
