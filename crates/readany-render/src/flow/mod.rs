pub(crate) mod docx;
pub(crate) mod odt;
pub(crate) mod rtf;
pub(crate) mod styles;

use crate::model::*;
use crate::text::{TextStyle, measure, shape};
use crate::{Format, Options, RenderError};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Alignment {
    #[default]
    Left,
    Centre,
    Right,
    Justify,
}

/// Which edge of the text a tab stop pins to its position.
///
/// A tab stop is not only a place to jump to. `w:tab w:val="right"` says the
/// text *ends* there, and treating every stop as a left stop put the NIST
/// running header's second half at x = 720 on an 816 px page — 446 px right of
/// where LibreOffice ends it — on all 34 pages, because the header's only stop
/// is `<w:tab w:val="right" w:pos="9360"/>`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TabAlignment {
    #[default]
    Left,
    Centre,
    Right,
    /// Pins the first `.` in the segment to the stop; without one, ECMA-376
    /// says the segment behaves as right-aligned.
    Decimal,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TabStop {
    /// Offset from the paragraph's text origin, in display-list pixels.
    pub position: f32,
    pub alignment: TabAlignment,
}

/// Word's default tab grid when a paragraph declares no stop of its own: every
/// half inch, which is 48 px at the 96 dpi the display list is expressed in.
const DEFAULT_TAB_INTERVAL: f32 = 48.0;

#[derive(Clone, Debug)]
pub(crate) struct ParagraphStyle {
    pub alignment: Alignment,
    pub left: f32,
    pub right: f32,
    pub first_line: f32,
    pub before: f32,
    pub after: f32,
    pub line_height: Option<f32>,
    pub line_height_at_least: bool,
    pub line_height_multiplier: f32,
    pub keep_next: bool,
    pub keep_lines: bool,
    pub widow_control: bool,
    pub page_break_before: bool,
    pub tabs: Vec<TabStop>,
}

impl Default for ParagraphStyle {
    fn default() -> Self {
        Self {
            alignment: Alignment::Left,
            left: 0.0,
            right: 0.0,
            first_line: 0.0,
            before: 0.0,
            after: 6.6,
            line_height: None,
            line_height_at_least: false,
            line_height_multiplier: 1.2,
            keep_next: false,
            keep_lines: false,
            widow_control: true,
            page_break_before: false,
            tabs: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FlowRun {
    pub text: String,
    pub style: TextStyle,
}

#[derive(Clone, Debug)]
pub(crate) struct FlowParagraph {
    pub runs: Vec<FlowRun>,
    pub style: ParagraphStyle,
}

impl FlowParagraph {
    fn text(&self) -> String {
        self.runs.iter().map(|run| run.text.as_str()).collect()
    }
}

pub(crate) fn default_text_style() -> TextStyle {
    TextStyle {
        family: "Calibri".into(),
        size_px: 14.666_667,
        ..TextStyle::default()
    }
}

/// How a cell relates to the one above it in a vertical merge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum VerticalMerge {
    #[default]
    None,
    /// `w:vMerge w:val="restart"` — the cell that owns the merged span.
    Start,
    /// `w:vMerge` with no value — a continuation, which carries no content of
    /// its own and must not draw a rule across the span it belongs to.
    Continue,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum VerticalAlignment {
    #[default]
    Top,
    Centre,
    Bottom,
}

/// One drawn cell edge. `None` is the absence of a rule, not a black default:
/// a Word table with no `w:tblBorders` is borderless, and drawing rules anyway
/// invents structure the document does not have.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Border {
    pub width: f32,
    pub colour: Colour,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct BorderEdges {
    pub top: Option<Border>,
    pub left: Option<Border>,
    pub bottom: Option<Border>,
    pub right: Option<Border>,
}

#[derive(Clone, Debug)]
pub(crate) struct FlowCell {
    pub paragraphs: Vec<FlowParagraph>,
    /// Index of the first grid column this cell covers.
    pub column: usize,
    /// `w:gridSpan`, at least 1.
    pub span: usize,
    pub merge: VerticalMerge,
    pub vertical_alignment: VerticalAlignment,
    /// Resolved per cell, because `w:tcBorders` overrides `w:tblBorders` and
    /// an interior edge is a different rule from an outer one.
    pub borders: BorderEdges,
    pub shading: Option<Colour>,
}

#[derive(Clone, Debug)]
pub(crate) struct FlowRow {
    pub cells: Vec<FlowCell>,
    /// `w:trHeight` as a floor on the row, in pixels.
    pub minimum_height: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct FlowTable {
    pub rows: Vec<FlowRow>,
    /// Resolved grid column widths in pixels, one entry per `w:gridCol`.
    pub columns: Vec<f32>,
    /// Offset of the table's left edge from the text margin.
    pub indent: f32,
    /// Padding inside each cell, from `w:tblCellMar`.
    pub cell_margin_left: f32,
    pub cell_margin_right: f32,
    pub cell_margin_top: f32,
    pub cell_margin_bottom: f32,
}

#[derive(Clone, Debug)]
pub(crate) enum FlowBlock {
    Paragraph(FlowParagraph),
    Table(FlowTable),
}

pub(crate) fn layout_flow(
    paragraphs: &[FlowParagraph],
    format: Format,
    options: &Options<'_>,
    page_size: Size,
    margins: (f32, f32, f32, f32),
) -> Result<Rendered, RenderError> {
    let blocks = paragraphs
        .iter()
        .cloned()
        .map(FlowBlock::Paragraph)
        .collect::<Vec<_>>();
    layout_blocks(&blocks, format, options, page_size, margins)
}

/// Running state of the page flow, shared by paragraphs and by the rows of a
/// table so both break pages by the same rule.
struct FlowCursor<'a, 'font> {
    pages: Vec<Page>,
    y: f32,
    page_size: Size,
    margins: (f32, f32, f32, f32),
    options: &'a Options<'font>,
}

impl FlowCursor<'_, '_> {
    fn content_bottom(&self) -> f32 {
        self.page_size.height - self.margins.3
    }

    fn at_page_top(&self) -> bool {
        self.y <= self.margins.1
    }

    fn break_page(&mut self) -> Result<(), RenderError> {
        push_page(&mut self.pages, self.page_size, self.options)?;
        self.y = self.margins.1;
        Ok(())
    }

    fn items(&mut self) -> Result<&mut Vec<Item>, RenderError> {
        self.pages
            .last_mut()
            .map(|page| &mut page.items)
            .ok_or_else(|| RenderError::malformed("page layout could not be created"))
    }
}

pub(crate) fn layout_blocks(
    blocks: &[FlowBlock],
    format: Format,
    options: &Options<'_>,
    page_size: Size,
    margins: (f32, f32, f32, f32),
) -> Result<Rendered, RenderError> {
    let mut cursor = FlowCursor {
        pages: vec![new_page(page_size, 1)],
        y: margins.1,
        page_size,
        margins,
        options,
    };
    let mut paragraph_index = 0_u32;
    let mut pending_page_break = false;
    for block in blocks {
        match block {
            FlowBlock::Paragraph(paragraph) => {
                pending_page_break = layout_page_paragraph(
                    &mut cursor,
                    paragraph,
                    paragraph_index,
                    pending_page_break,
                )?;
                paragraph_index = paragraph_index.saturating_add(1);
            }
            FlowBlock::Table(table) => {
                paragraph_index = layout_table(&mut cursor, table, paragraph_index)?;
                pending_page_break = false;
            }
        }
    }
    Ok(Rendered {
        pages: cursor.pages,
        format,
        unrendered: Vec::new(),
        meta: Meta::default(),
    })
}

/// Lays one paragraph into the page flow, returning whether it ends with a
/// pending page break.
fn layout_page_paragraph(
    cursor: &mut FlowCursor<'_, '_>,
    paragraph: &FlowParagraph,
    paragraph_index: u32,
    pending_page_break: bool,
) -> Result<bool, RenderError> {
    let margins = cursor.margins;
    let page_size = cursor.page_size;
    let content_bottom = cursor.content_bottom();
    {
        let text = paragraph.text();
        let box_width = (page_size.width - margins.0 - margins.2).max(1.0);
        let lines = wrap_rich(&text, paragraph, box_width);
        let line_metrics = lines
            .iter()
            .map(|range| line_height(paragraph, range.clone()))
            .collect::<Vec<_>>();
        let paragraph_height =
            paragraph.style.before + line_metrics.iter().sum::<f32>() + paragraph.style.after;
        // **A break before nothing is not a break.** A `break-before` on the
        // very first block asks for a page boundary where one already exists,
        // and honouring it literally opens the document with a blank page.
        // Word and LibreOffice both suppress it, so a document carrying
        // `fo:break-before="page"` or `w:pageBreakBefore` on its first
        // paragraph — the UK IPO agreement does — paginated as three pages
        // against LibreOffice's two, and every page-aligned score after the
        // first page was then comparing unrelated content.
        //
        // The test is "is anything on this page yet", not "is this paragraph
        // index zero": a leading empty paragraph must not re-enable the break
        // either, and `y > margins.1` is already how the keep rules below ask
        // the same question.
        let page_is_empty = cursor.pages.len() == 1 && cursor.at_page_top();
        if (!page_is_empty && (pending_page_break || paragraph.style.page_break_before))
            || ((paragraph.style.keep_lines || paragraph.style.keep_next)
                && !cursor.at_page_top()
                && cursor.y + paragraph_height > content_bottom
                && paragraph_height <= content_bottom - margins.1)
        {
            cursor.break_page()?;
        }
        // Paragraph space-before is suppressed at the top of a page.  Applying
        // it where there is no preceding block made the NIST document's final
        // 649.6 px spacer overflow onto a second otherwise blank page.
        if !cursor.at_page_top() {
            cursor.y += paragraph.style.before;
        }
        let fitting_lines = line_metrics
            .iter()
            .scan(cursor.y, |scan, height| {
                *scan += *height;
                Some(*scan <= content_bottom)
            })
            .take_while(|fits| *fits)
            .count();
        if paragraph.style.widow_control
            && lines.len() > 1
            && fitting_lines == 1
            && !cursor.at_page_top()
        {
            cursor.break_page()?;
        }

        let mut previous_end = 0_usize;
        let line_count = line_metrics.len();
        for (line_index, range) in lines.into_iter().enumerate() {
            if line_index > 0
                && text
                    .get(previous_end..range.start)
                    .is_some_and(|between| between.contains('\u{c}'))
            {
                cursor.break_page()?;
            }
            let height = line_metrics.get(line_index).copied().unwrap_or(17.6);
            if cursor.y + height > content_bottom && !cursor.at_page_top() {
                cursor.break_page()?;
            }
            let range_end = range.end;
            let y = cursor.y;
            paint_paragraph_line(
                cursor.items()?,
                &ParagraphLine {
                    paragraph,
                    text: &text,
                    range,
                    paragraph_index,
                    line_index,
                    line_count,
                    origin_x: margins.0,
                    box_width,
                    y,
                    height,
                },
            );
            cursor.y += height;
            previous_end = range_end;
        }
        cursor.y += paragraph.style.after;
        Ok(text.ends_with('\u{c}'))
    }
}

/// Everything one line of a paragraph needs to place itself, whether the box
/// around it is the page's text column or a table cell.
struct ParagraphLine<'a> {
    paragraph: &'a FlowParagraph,
    text: &'a str,
    range: std::ops::Range<usize>,
    paragraph_index: u32,
    line_index: usize,
    line_count: usize,
    /// Left edge of the containing box — the page's text margin, or a table
    /// cell's text edge. Tab stops are measured from here, not from the
    /// paragraph's indent.
    origin_x: f32,
    /// Full width of that box, before the paragraph's own indents.
    box_width: f32,
    y: f32,
    height: f32,
}

fn paint_paragraph_line(items: &mut Vec<Item>, line: &ParagraphLine<'_>) {
    let paragraph = line.paragraph;
    let start = line_start_cursor(paragraph, line.line_index);
    let line_width = measure_rich(paragraph, line.range.clone(), start);
    let available = (line.box_width - paragraph.style.right - start).max(1.0);
    let offset = match paragraph.style.alignment {
        Alignment::Left | Alignment::Justify => 0.0,
        Alignment::Centre => ((available - line_width) / 2.0).max(0.0),
        Alignment::Right => (available - line_width).max(0.0),
    };
    let baseline = line.y + baseline_offset(paragraph, line.range.clone(), line.height);
    let mut x = line.origin_x + start + offset;
    let is_last_line = line.line_index + 1 == line.line_count;
    let spaces = line
        .text
        .get(line.range.clone())
        .unwrap_or_default()
        .chars()
        .filter(|character| *character == ' ')
        .count();
    let justify_extra =
        if paragraph.style.alignment == Alignment::Justify && !is_last_line && spaces > 0 {
            ((available - line_width) / spaces as f32).max(0.0)
        } else {
            0.0
        };
    paint_line(
        items,
        paragraph,
        line.text,
        line.range.clone(),
        line.paragraph_index,
        &mut x,
        baseline,
        justify_extra,
        line.origin_x,
    );
}

/// Lays a cell's paragraphs into a box `width` px wide, returning the items
/// placed relative to a top of zero and the height they consumed.
fn layout_cell(
    paragraphs: &[FlowParagraph],
    origin_x: f32,
    width: f32,
    first_paragraph_index: u32,
) -> (Vec<Item>, f32) {
    let mut items = Vec::new();
    let mut y = 0.0_f32;
    for (offset, paragraph) in paragraphs.iter().enumerate() {
        let text = paragraph.text();
        let lines = wrap_rich(&text, paragraph, width);
        let line_count = lines.len();
        if offset > 0 {
            y += paragraph.style.before;
        }
        for (line_index, range) in lines.into_iter().enumerate() {
            let height = line_height(paragraph, range.clone());
            paint_paragraph_line(
                &mut items,
                &ParagraphLine {
                    paragraph,
                    text: &text,
                    range,
                    paragraph_index: first_paragraph_index.saturating_add(offset as u32),
                    line_index,
                    line_count,
                    origin_x,
                    box_width: width,
                    y,
                    height,
                },
            );
            y += height;
        }
        y += paragraph.style.after;
    }
    (items, y)
}

fn translate(items: &mut [Item], dy: f32) {
    for item in items.iter_mut() {
        match item {
            Item::Glyphs(run) => run.origin.y += dy,
            Item::Path(path) => {
                for command in &mut path.path.commands {
                    match command {
                        PathCommand::Move(point) | PathCommand::Line(point) => point.y += dy,
                        PathCommand::Quad(one, two) => {
                            one.y += dy;
                            two.y += dy;
                        }
                        PathCommand::Cubic(one, two, three) => {
                            one.y += dy;
                            two.y += dy;
                            three.y += dy;
                        }
                        PathCommand::Close => {}
                    }
                }
            }
            Item::Image(image) => image.rect.y += dy,
            Item::Group(group) => translate(&mut group.items, dy),
        }
    }
}

fn edge_path(from: Point, to: Point, border: Border) -> Item {
    Item::Path(PathItem {
        path: Path {
            commands: vec![PathCommand::Move(from), PathCommand::Line(to)],
        },
        fill: None,
        stroke: Some(Stroke {
            paint: Paint {
                colour: border.colour,
            },
            width: border.width,
            dash: Vec::new(),
        }),
        source: None,
    })
}

/// Lays a table into the page flow and returns the next free paragraph index.
///
/// A row is the unit that breaks: Word only splits a row across pages when
/// `w:cantSplit` is absent *and* the row is taller than a page, and the corpus
/// has no such row. Laying each cell into its own column box — rather than
/// flattening the row into one tab-separated paragraph — is what makes a cell
/// that wraps stay inside its column instead of restarting at the table's left
/// edge and dragging every later column with it.
fn layout_table(
    cursor: &mut FlowCursor<'_, '_>,
    table: &FlowTable,
    first_paragraph_index: u32,
) -> Result<u32, RenderError> {
    let mut paragraph_index = first_paragraph_index;
    let table_left = cursor.margins.0 + table.indent;
    let content_bottom = cursor.content_bottom();
    let mut column_x = Vec::with_capacity(table.columns.len() + 1);
    let mut cumulative = table_left;
    for width in &table.columns {
        column_x.push(cumulative);
        cumulative += *width;
    }
    column_x.push(cumulative);
    for row in &table.rows {
        // Lay every cell first: the row is as tall as its tallest cell, and no
        // cell can be placed until that is known.
        let mut laid_out = Vec::with_capacity(row.cells.len());
        let mut content_height = 0.0_f32;
        for cell in &row.cells {
            let left = column_x
                .get(cell.column)
                .copied()
                .unwrap_or(cumulative.min(table_left));
            let right = column_x
                .get(cell.column.saturating_add(cell.span.max(1)))
                .copied()
                .unwrap_or(cumulative);
            let width = (right - left - table.cell_margin_left - table.cell_margin_right).max(1.0);
            let cell_index = paragraph_index;
            paragraph_index =
                paragraph_index.saturating_add(u32::try_from(cell.paragraphs.len()).unwrap_or(0));
            let (items, height) = layout_cell(
                &cell.paragraphs,
                left + table.cell_margin_left,
                width,
                cell_index,
            );
            content_height = content_height.max(height);
            laid_out.push((cell, left, right, items, height));
        }
        let row_height = content_height.max(row.minimum_height).max(1.0)
            + table.cell_margin_top
            + table.cell_margin_bottom;
        if cursor.y + row_height > content_bottom && !cursor.at_page_top() {
            cursor.break_page()?;
        }
        let top = cursor.y;
        let bottom = top + row_height;
        let items = cursor.items()?;
        for (cell, left, right, mut cell_items, height) in laid_out {
            let free =
                (row_height - table.cell_margin_top - table.cell_margin_bottom - height).max(0.0);
            let dy = top
                + table.cell_margin_top
                + match cell.vertical_alignment {
                    VerticalAlignment::Top => 0.0,
                    VerticalAlignment::Centre => free / 2.0,
                    VerticalAlignment::Bottom => free,
                };
            translate(&mut cell_items, dy);
            if let Some(colour) = cell.shading {
                items.push(Item::Path(PathItem {
                    path: rect_path(Rect {
                        x: left,
                        y: top,
                        width: right - left,
                        height: row_height,
                    }),
                    fill: Some(Paint { colour }),
                    stroke: None,
                    source: None,
                }));
            }
            items.append(&mut cell_items);
            let corners = [
                (Point { x: left, y: top }, Point { x: right, y: top }),
                (Point { x: left, y: top }, Point { x: left, y: bottom }),
                (
                    Point { x: left, y: bottom },
                    Point {
                        x: right,
                        y: bottom,
                    },
                ),
                (
                    Point { x: right, y: top },
                    Point {
                        x: right,
                        y: bottom,
                    },
                ),
            ];
            // A vertical-merge continuation must not draw the rule that would
            // cut the merged cell in half.
            let top_border = match cell.merge {
                VerticalMerge::Continue => None,
                VerticalMerge::None | VerticalMerge::Start => cell.borders.top,
            };
            for (border, (from, to)) in [
                top_border,
                cell.borders.left,
                cell.borders.bottom,
                cell.borders.right,
            ]
            .into_iter()
            .zip(corners)
            {
                if let Some(border) = border {
                    items.push(edge_path(from, to, border));
                }
            }
        }
        cursor.y = bottom;
    }
    Ok(paragraph_index)
}

fn new_page(size: Size, number: usize) -> Page {
    Page {
        size,
        label: Some(format!("Page {number}")),
        items: Vec::new(),
        source: None,
        frozen: None,
        grid: None,
    }
}

fn push_page(pages: &mut Vec<Page>, size: Size, options: &Options<'_>) -> Result<(), RenderError> {
    if pages.len() as u32 >= options.limits.pages {
        return Err(RenderError::limit("pages", pages.len() as u64 + 1));
    }
    pages.push(new_page(size, pages.len() + 1));
    Ok(())
}

/// The largest font on the line, and what to do when the line has no glyphs.
///
/// An empty paragraph still occupies a line, so the height falls back to the
/// paragraph's own first run rather than to a fixed size. It used to fall back
/// to 11 pt *as a floor on every line*, which silently rounded every smaller
/// line up: the NIST table of contents is set in 10 pt and each of its entries
/// was 1.5 px too tall, so its 33 entries drifted the whole document down by
/// 50 px before the first page of body text.
fn line_font_size(paragraph: &FlowParagraph, range: std::ops::Range<usize>) -> f32 {
    let measured = styled_segments(paragraph, range)
        .map(|(_, _, style)| style.size_px)
        .fold(0.0_f32, f32::max);
    if measured > 0.0 {
        measured
    } else {
        paragraph
            .runs
            .first()
            .map(|run| run.style.size_px)
            .unwrap_or(14.666_667)
    }
    .max(1.0)
}

fn line_height(paragraph: &FlowParagraph, range: std::ops::Range<usize>) -> f32 {
    let natural = line_font_size(paragraph, range) * paragraph.style.line_height_multiplier;
    match paragraph.style.line_height {
        Some(value) if paragraph.style.line_height_at_least => natural.max(value),
        Some(value) => value.max(1.0),
        None => natural,
    }
}

fn baseline_offset(paragraph: &FlowParagraph, range: std::ops::Range<usize>, height: f32) -> f32 {
    let font_size = line_font_size(paragraph, range);
    ((height - font_size) / 2.0).max(0.0) + font_size * 0.82
}

/// One run of a line between two tabs, carried as byte ranges into the
/// paragraph's text so the painter can keep each part's own style.
struct TabSegment<'a> {
    parts: Vec<(usize, usize, &'a TextStyle)>,
    width: f32,
    /// Width of the part before the segment's first `.`, for decimal stops.
    decimal_prefix: f32,
}

fn tab_segments<'a>(
    paragraph: &'a FlowParagraph,
    full_text: &str,
    range: std::ops::Range<usize>,
) -> Vec<TabSegment<'a>> {
    let mut segments = vec![TabSegment {
        parts: Vec::new(),
        width: 0.0,
        decimal_prefix: 0.0,
    }];
    let mut decimal_seen = false;
    for (start, end, style) in styled_segments(paragraph, range) {
        let source_text = full_text.get(start..end).unwrap_or_default();
        let mut offset = start;
        for (part_index, part) in source_text.split('\t').enumerate() {
            if part_index > 0 {
                segments.push(TabSegment {
                    parts: Vec::new(),
                    width: 0.0,
                    decimal_prefix: 0.0,
                });
                decimal_seen = false;
                offset = offset.saturating_add(1);
            }
            let part_start = offset;
            offset = offset.saturating_add(part.len());
            let Some(segment) = segments.last_mut() else {
                continue;
            };
            if part.is_empty() {
                continue;
            }
            let width = measure(part, style);
            match (decimal_seen, part.find('.')) {
                (true, _) => {}
                (false, Some(dot)) => {
                    decimal_seen = true;
                    segment.decimal_prefix =
                        segment.width + measure(part.get(..dot).unwrap_or_default(), style);
                }
                (false, None) => segment.decimal_prefix = segment.width + width,
            }
            segment.width += width;
            segment.parts.push((part_start, offset, style));
        }
    }
    segments
}

/// Where a segment of the given width starts once `stop` has been applied,
/// given how far the line has already advanced from its tab origin.
///
/// A tab never pulls text backwards over what is already set: Word clamps to
/// the current pen position and then falls through to the next stop, which is
/// why the clamp is `max` rather than an assertion.
fn tab_start(cursor: f32, segment: &TabSegment<'_>, stop: TabStop) -> f32 {
    let start = match stop.alignment {
        TabAlignment::Left => stop.position,
        TabAlignment::Centre => stop.position - segment.width / 2.0,
        TabAlignment::Right => stop.position - segment.width,
        TabAlignment::Decimal => stop.position - segment.decimal_prefix,
    };
    start.max(cursor)
}

/// The stop that applies to a pen sitting `cursor` px past the box's left edge.
///
/// The paragraph's own left indent is an implicit stop. Word and LibreOffice
/// both have it, and it is the whole of why the tab after `1.` in the UK IPO
/// agreement's hanging-indent clauses lands on the indent at 28.4 px rather
/// than on the 48 px default grid.
fn next_tab(cursor: f32, custom: &[TabStop], left_indent: f32) -> TabStop {
    let implicit = (left_indent > cursor).then_some(TabStop {
        position: left_indent,
        alignment: TabAlignment::Left,
    });
    let declared = custom.iter().copied().find(|stop| stop.position > cursor);
    match (implicit, declared) {
        (Some(implicit), Some(declared)) if implicit.position <= declared.position => implicit,
        (_, Some(declared)) => declared,
        (Some(implicit), None) => implicit,
        (None, None) => TabStop {
            position: ((cursor / DEFAULT_TAB_INTERVAL).floor() + 1.0) * DEFAULT_TAB_INTERVAL,
            alignment: TabAlignment::Left,
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_line(
    items: &mut Vec<Item>,
    paragraph: &FlowParagraph,
    full_text: &str,
    range: std::ops::Range<usize>,
    paragraph_index: u32,
    x: &mut f32,
    baseline: f32,
    justify_extra: f32,
    tab_origin: f32,
) {
    let segments = tab_segments(paragraph, full_text, range);
    for (segment_index, segment) in segments.iter().enumerate() {
        if segment_index > 0 {
            let cursor = *x - tab_origin;
            let stop = next_tab(cursor, &paragraph.style.tabs, paragraph.style.left);
            *x = tab_origin + tab_start(cursor, segment, stop);
        }
        for (start, end, style) in &segment.parts {
            let source = SourceRef::Text {
                paragraph: paragraph_index,
                start: u32::try_from(*start).unwrap_or(u32::MAX),
                end: u32::try_from(*end).unwrap_or(u32::MAX),
            };
            let Some(part) = full_text.get(*start..*end) else {
                continue;
            };
            let visual = bidi_visual(part);
            let mut run = shape(&visual, style, Point { x: *x, y: baseline }, Some(source));
            if justify_extra > 0.0 {
                for (character, glyph) in visual.chars().zip(&mut run.glyphs) {
                    if character == ' ' {
                        glyph.x_advance += justify_extra;
                    }
                }
            }
            *x += run.glyphs.iter().map(|glyph| glyph.x_advance).sum::<f32>();
            items.push(Item::Glyphs(run));
        }
    }
}

fn bidi_visual(text: &str) -> String {
    let bidi = unicode_bidi::BidiInfo::new(text, None);
    bidi.paragraphs
        .first()
        .map(|paragraph| {
            bidi.reorder_line(paragraph, paragraph.range.clone())
                .into_owned()
        })
        .unwrap_or_else(|| text.to_owned())
}

fn styled_segments(
    paragraph: &FlowParagraph,
    wanted: std::ops::Range<usize>,
) -> impl Iterator<Item = (usize, usize, &TextStyle)> {
    paragraph
        .runs
        .iter()
        .scan(0_usize, |offset, run| {
            let start = *offset;
            *offset = offset.saturating_add(run.text.len());
            Some((start, *offset, &run.style))
        })
        .filter_map(move |(start, end, style)| {
            let start = start.max(wanted.start);
            let end = end.min(wanted.end);
            (start < end).then_some((start, end, style))
        })
}

/// Where a range leaves the pen, measured from the paragraph's tab origin, with
/// tab stops resolved.
///
/// Wrapping and alignment both ask this question, and both were answering it by
/// summing glyph advances and ignoring the tabs between them. A right stop can
/// move a segment hundreds of pixels, so a tabbed line's measured width bore no
/// relation to the space it occupies.
///
/// `from` is where the pen already sits relative to the tab origin, and it is
/// not always zero: a hanging indent starts the first line *left* of the origin
/// — the UK IPO agreement's numbered clauses start 28.4 px left of theirs — so
/// measuring from zero resolves the clause number's tab to the 48 px stop where
/// painting resolves it to the 0 px one, and the line is then measured 39 px
/// wider than it is drawn.
fn advance_rich(paragraph: &FlowParagraph, range: std::ops::Range<usize>, from: f32) -> f32 {
    let text = paragraph.text();
    let segments = tab_segments(paragraph, &text, range);
    let mut cursor = from;
    // **A tab with nothing after it is not a tab.** Where the line ends is
    // where its last glyph ends; a trailing tab moves the pen and paints
    // nothing, and counting it as width made the line look wider than it is.
    // The NIST running header ends with a second `w:tab/` after its final run,
    // which pushed the measured advance to the next default stop 48 px past the
    // right margin and wrapped `2026` onto a second line on all 17 even pages.
    let mut painted = from;
    for (index, segment) in segments.iter().enumerate() {
        if index > 0 {
            let stop = next_tab(cursor, &paragraph.style.tabs, paragraph.style.left);
            cursor = tab_start(cursor, segment, stop);
        }
        cursor += segment.width;
        if segment.width > 0.0 {
            painted = cursor;
        }
    }
    painted
}

/// The width a range occupies when the pen starts it at `from`.
fn measure_rich(paragraph: &FlowParagraph, range: std::ops::Range<usize>, from: f32) -> f32 {
    advance_rich(paragraph, range, from) - from
}

/// The pen's starting offset from the tab origin for the line being built.
///
/// Only the first line carries the first-line indent; a hanging indent makes
/// that offset negative, which is exactly the extra room Word gives that line.
fn line_start_cursor(paragraph: &FlowParagraph, line_index: usize) -> f32 {
    if line_index == 0 {
        paragraph.style.left + paragraph.style.first_line
    } else {
        paragraph.style.left
    }
}

/// Does `range` overflow the line that starts at `line_index`?
///
/// The room a line has is the content width less its own starting offset,
/// which is how `layout_flow` already computes the alignment slack. Wrapping
/// used the bare content width instead, so a hanging first line — 28.4 px wider
/// than the ones below it in the UK IPO agreement — was broken a word early.
fn overflows(
    paragraph: &FlowParagraph,
    range: std::ops::Range<usize>,
    box_width: f32,
    line_index: usize,
) -> bool {
    let from = line_start_cursor(paragraph, line_index);
    advance_rich(paragraph, range, from) > box_width - paragraph.style.right
}

fn wrap_rich(text: &str, paragraph: &FlowParagraph, width: f32) -> Vec<std::ops::Range<usize>> {
    if text.is_empty() {
        return std::iter::once(0..0).collect();
    }
    let mut lines = Vec::new();
    let mut line_start = 0_usize;
    let mut last_break = 0_usize;
    for (break_at, opportunity) in unicode_linebreak::linebreaks(text) {
        if text[line_start..break_at].contains('\u{c}') {
            let page_break = text[line_start..break_at]
                .find('\u{c}')
                .map(|offset| line_start + offset)
                .unwrap_or(break_at);
            lines.push(line_start..page_break);
            line_start = page_break.saturating_add(1);
            last_break = line_start;
            continue;
        }
        if overflows(paragraph, line_start..break_at, width, lines.len()) && last_break > line_start
        {
            lines.push(trim_end_range(text, line_start..last_break));
            line_start = last_break;
        }
        last_break = break_at;
        if opportunity == unicode_linebreak::BreakOpportunity::Mandatory {
            lines.push(trim_end_range(text, line_start..break_at));
            line_start = break_at;
        }
    }
    if line_start < text.len() {
        let mut cursor = line_start;
        while overflows(paragraph, cursor..text.len(), width, lines.len()) {
            let mut end = cursor;
            for (offset, character) in text[cursor..].char_indices() {
                let candidate = cursor + offset + character.len_utf8();
                if candidate > cursor && overflows(paragraph, cursor..candidate, width, lines.len())
                {
                    break;
                }
                end = candidate;
            }
            if end == cursor {
                end = text[cursor..]
                    .chars()
                    .next()
                    .map(char::len_utf8)
                    .map(|length| cursor + length)
                    .unwrap_or(text.len());
            }
            lines.push(trim_end_range(text, cursor..end));
            cursor = end;
        }
        if cursor < text.len() {
            lines.push(trim_end_range(text, cursor..text.len()));
        }
    }
    if lines.is_empty() {
        lines.push(0..0);
    }
    lines
}

fn trim_end_range(text: &str, mut range: std::ops::Range<usize>) -> std::ops::Range<usize> {
    while range.end > range.start {
        let Some(character) = text[range.start..range.end].chars().next_back() else {
            break;
        };
        if !matches!(character, '\r' | '\n' | '\u{c}') {
            break;
        }
        range.end = range.end.saturating_sub(character.len_utf8());
    }
    range
}
