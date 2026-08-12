use crate::container::{resolve_relationship, xml, zip::Archive};
use crate::model::*;
use crate::sheet::numfmt;
use crate::sheet::styles::{self, CellStyle, Styles};
use crate::text::{TextStyle, measure, shape};
use crate::{Format, Options, RenderError};
use quick_xml::events::{BytesStart, Event};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Cell {
    row: u32,
    column: u32,
    value: String,
    style: usize,
    numeric: bool,
    formula_without_value: bool,
}

struct Sheet {
    name: String,
    hidden: bool,
    target: String,
}

pub(crate) fn render(
    bytes: &[u8],
    format: Format,
    options: &Options<'_>,
) -> Result<Rendered, RenderError> {
    let archive = Archive::open(bytes, &options.limits)?;
    let workbook = archive.required("xl/workbook.xml")?;
    xml::validate(workbook, &options.limits)?;
    let relationships = parse_relationships(
        archive.required("xl/_rels/workbook.xml.rels")?,
        &options.limits,
    )?;
    let (sheets, date_1904) = parse_workbook(workbook, &relationships)?;
    if u32::try_from(sheets.len()).unwrap_or(u32::MAX) > options.limits.pages {
        return Err(RenderError::limit("pages", sheets.len() as u64));
    }
    let shared = archive
        .get("xl/sharedStrings.xml")
        .map(|xml| parse_shared_strings(xml, &options.limits))
        .transpose()?
        .unwrap_or_default();
    let styles = archive
        .get("xl/styles.xml")
        .map(styles::parse)
        .transpose()?
        .unwrap_or_default();
    let mut pages = Vec::new();
    let mut unrendered = Vec::new();
    for (sheet_index, sheet) in sheets.iter().enumerate() {
        if sheet.hidden {
            unrendered.push(Unrendered::HiddenSheet {
                name: sheet.name.clone(),
            });
            continue;
        }
        let sheet_bytes = archive.required(&sheet.target)?;
        xml::validate(sheet_bytes, &options.limits)?;
        let parsed = parse_sheet(
            sheet_bytes,
            sheet_index as u32,
            &shared,
            &styles,
            date_1904,
            options,
        )?;
        unrendered.extend(parsed.1);
        pages.push(Page {
            label: Some(sheet.name.clone()),
            ..parsed.0
        });
    }
    for name in archive.names() {
        if name.starts_with("xl/charts/") && name.ends_with(".xml") {
            unrendered.push(Unrendered::Chart {
                page: 0,
                kind: "OOXML chart".into(),
            });
        }
        if name.starts_with("xl/pivotTables/") && name.ends_with(".xml") {
            unrendered.push(Unrendered::PivotTable { page: 0 });
        }
        if name.ends_with("vbaProject.bin") {
            unrendered.push(Unrendered::Macro);
        }
        if name.starts_with("xl/externalLinks/") && name.ends_with(".xml") {
            unrendered.push(Unrendered::ExternalReference {
                target: name.into(),
            });
        }
    }
    dedupe_unrendered(&mut unrendered);
    Ok(Rendered {
        pages,
        format,
        unrendered,
        meta: Meta::default(),
    })
}

fn parse_relationships(
    bytes: &[u8],
    limits: &crate::Limits,
) -> Result<BTreeMap<String, String>, RenderError> {
    xml::validate(bytes, limits)?;
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut result = BTreeMap::new();
    loop {
        match reader.read_event() {
            Ok(Event::Empty(start))
                if xml::local_name(start.name().as_ref()) == b"Relationship" =>
            {
                let id = attr(&start, b"Id").unwrap_or_default();
                let target = attr(&start, b"Target").unwrap_or_default();
                if !id.is_empty() && !target.is_empty() && !target.contains("://") {
                    result.insert(id, resolve_relationship("xl/workbook.xml", &target)?);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => {
                return Err(RenderError::malformed(
                    "workbook relationships are malformed; obtain a fresh copy",
                ));
            }
        }
    }
    Ok(result)
}

fn parse_workbook(
    bytes: &[u8],
    rels: &BTreeMap<String, String>,
) -> Result<(Vec<Sheet>, bool), RenderError> {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut sheets = Vec::new();
    let mut date_1904 = false;
    loop {
        match reader.read_event() {
            Ok(Event::Empty(start)) | Ok(Event::Start(start))
                if xml::local_name(start.name().as_ref()) == b"workbookPr" =>
            {
                date_1904 = attr(&start, b"date1904")
                    .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            }
            Ok(Event::Empty(start)) | Ok(Event::Start(start))
                if xml::local_name(start.name().as_ref()) == b"sheet" =>
            {
                let name = attr(&start, b"name").unwrap_or_else(|| "Sheet".into());
                let id = attr(&start, b"id").ok_or_else(|| {
                    RenderError::malformed(
                        "a workbook sheet has no relationship id; obtain a fresh copy",
                    )
                })?;
                let target = rels.get(&id).cloned().ok_or_else(|| {
                    RenderError::malformed(
                        "a workbook sheet relationship is missing; obtain a fresh copy",
                    )
                })?;
                let hidden = attr(&start, b"state").is_some_and(|v| v != "visible");
                sheets.push(Sheet {
                    name,
                    hidden,
                    target,
                });
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => {
                return Err(RenderError::malformed(
                    "the workbook metadata is malformed; obtain a fresh copy",
                ));
            }
        }
    }
    Ok((sheets, date_1904))
}

fn parse_shared_strings(bytes: &[u8], limits: &crate::Limits) -> Result<Vec<String>, RenderError> {
    xml::validate(bytes, limits)?;
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_si = false;
    let mut in_t = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(s)) if xml::local_name(s.name().as_ref()) == b"si" => {
                in_si = true;
                current.clear();
            }
            Ok(Event::End(s)) if xml::local_name(s.name().as_ref()) == b"si" => {
                in_si = false;
                values.push(std::mem::take(&mut current));
            }
            Ok(Event::Start(s)) if in_si && xml::local_name(s.name().as_ref()) == b"t" => {
                in_t = true
            }
            Ok(Event::End(s)) if xml::local_name(s.name().as_ref()) == b"t" => in_t = false,
            Ok(Event::Text(t)) if in_t => {
                let decoded = t.decode().map_err(|_| {
                    RenderError::malformed(
                        "shared strings contain invalid XML text; obtain a fresh copy",
                    )
                })?;
                current.push_str(&quick_xml::escape::unescape(&decoded).map_err(|_| {
                    RenderError::malformed(
                        "shared strings contain invalid XML escapes; obtain a fresh copy",
                    )
                })?);
            }
            Ok(Event::GeneralRef(reference)) if in_t => {
                current.push_str(&xml::decode_reference(&reference)?);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => {
                return Err(RenderError::malformed(
                    "shared strings are malformed; obtain a fresh copy",
                ));
            }
        }
    }
    Ok(values)
}

fn parse_sheet(
    bytes: &[u8],
    sheet: u32,
    shared: &[String],
    styles: &Styles,
    date_1904: bool,
    options: &Options<'_>,
) -> Result<(Page, Vec<Unrendered>), RenderError> {
    let default_style = TextStyle {
        family: styles.default_font.family.clone(),
        size_px: styles.default_font.size_px,
        colour: Some(styles.default_font.colour),
        bold: styles.default_font.bold,
        italic: styles.default_font.italic,
        ..TextStyle::default()
    };
    let maximum_digit_width = measure("0", &default_style).max(1.0);
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut cells = Vec::new();
    let mut widths: BTreeMap<u32, f32> = BTreeMap::new();
    let mut heights: BTreeMap<u32, f32> = BTreeMap::new();
    let mut default_row_height = 20.0_f32;
    let mut merges = Vec::new();
    let mut frozen = None;
    let mut unrendered = Vec::new();
    let mut current: Option<Cell> = None;
    let mut kind = String::new();
    let mut value = String::new();
    let mut inline = String::new();
    let mut in_v = false;
    let mut in_t = false;
    let mut has_formula = false;
    let mut has_cached_value = false;
    let mut conditional = 0_u32;
    loop {
        match reader.read_event() {
            Ok(Event::Empty(s)) | Ok(Event::Start(s))
                if xml::local_name(s.name().as_ref()) == b"sheetFormatPr" =>
            {
                if let Some(height) = attr(&s, b"defaultRowHeight")
                    .and_then(|value| value.parse::<f32>().ok())
                    .filter(|height| height.is_finite() && *height > 0.0)
                {
                    default_row_height = height * 96.0 / 72.0;
                }
            }
            Ok(Event::Empty(s)) | Ok(Event::Start(s))
                if xml::local_name(s.name().as_ref()) == b"col" =>
            {
                let min = number_attr(&s, b"min").unwrap_or(1);
                let max = number_attr(&s, b"max")
                    .unwrap_or(min)
                    .min(min.saturating_add(100_000));
                let width = attr(&s, b"width")
                    .and_then(|v| v.parse::<f32>().ok())
                    .unwrap_or(8.43);
                let px = ((256.0 * width + (128.0 / maximum_digit_width).round()) / 256.0
                    * maximum_digit_width)
                    .floor();
                for column in min..=max {
                    widths.insert(column - 1, px);
                }
            }
            Ok(Event::Start(s)) if xml::local_name(s.name().as_ref()) == b"row" => {
                if let Some(row) = number_attr(&s, b"r") {
                    if let Some(height) = attr(&s, b"ht").and_then(|v| v.parse::<f32>().ok()) {
                        heights.insert(row - 1, height * 96.0 / 72.0);
                    }
                }
            }
            Ok(Event::Start(s)) if xml::local_name(s.name().as_ref()) == b"c" => {
                let (row, column) = attr(&s, b"r").and_then(|v| cell_ref(&v)).unwrap_or((0, 0));
                kind = attr(&s, b"t").unwrap_or_default();
                value.clear();
                inline.clear();
                has_formula = false;
                has_cached_value = false;
                current = Some(Cell {
                    row,
                    column,
                    style: number_attr(&s, b"s").unwrap_or(0) as usize,
                    ..Cell::default()
                });
            }
            Ok(Event::Start(s))
                if current.is_some() && xml::local_name(s.name().as_ref()) == b"f" =>
            {
                has_formula = true
            }
            Ok(Event::Start(s))
                if current.is_some() && xml::local_name(s.name().as_ref()) == b"v" =>
            {
                in_v = true;
                has_cached_value = true;
            }
            Ok(Event::Empty(s))
                if current.is_some() && xml::local_name(s.name().as_ref()) == b"v" =>
            {
                has_cached_value = true;
            }
            Ok(Event::End(s)) if xml::local_name(s.name().as_ref()) == b"v" => in_v = false,
            Ok(Event::Start(s))
                if current.is_some() && xml::local_name(s.name().as_ref()) == b"t" =>
            {
                in_t = true
            }
            Ok(Event::End(s)) if xml::local_name(s.name().as_ref()) == b"t" => in_t = false,
            Ok(Event::Text(t)) if in_v => value.push_str(&t.decode().map_err(|_| {
                RenderError::malformed("a cell value is invalid XML text; obtain a fresh copy")
            })?),
            Ok(Event::Text(t)) if in_t => {
                let decoded = t.decode().map_err(|_| {
                    RenderError::malformed(
                        "an inline string is invalid XML text; obtain a fresh copy",
                    )
                })?;
                inline.push_str(&quick_xml::escape::unescape(&decoded).map_err(|_| {
                    RenderError::malformed(
                        "an inline string contains invalid XML escapes; obtain a fresh copy",
                    )
                })?);
            }
            Ok(Event::GeneralRef(reference)) if in_t => {
                inline.push_str(&xml::decode_reference(&reference)?);
            }
            Ok(Event::End(s)) if xml::local_name(s.name().as_ref()) == b"c" => {
                if let Some(mut cell) = current.take() {
                    cell.formula_without_value = has_formula && !has_cached_value;
                    cell.numeric = kind.is_empty() || kind == "n";
                    cell.value = cell_value(
                        &kind,
                        &value,
                        &inline,
                        shared,
                        styles
                            .cells
                            .get(cell.style)
                            .map(|style| style.number_format.as_str())
                            .unwrap_or("General"),
                        date_1904,
                    );
                    if cells.len() as u64 >= options.limits.cells {
                        return Err(RenderError::limit("cells", cells.len() as u64 + 1));
                    }
                    cells.push(cell);
                }
            }
            Ok(Event::Empty(s)) if xml::local_name(s.name().as_ref()) == b"mergeCell" => {
                if let Some(range) = attr(&s, b"ref") {
                    if let Some((a, b)) = range
                        .split_once(':')
                        .and_then(|(a, b)| Some((cell_ref(a)?, cell_ref(b)?)))
                    {
                        merges.push((a, b));
                    }
                }
            }
            Ok(Event::Empty(s)) | Ok(Event::Start(s))
                if xml::local_name(s.name().as_ref()) == b"pane"
                    && attr(&s, b"state").as_deref() == Some("frozen") =>
            {
                frozen = Some(FrozenPanes {
                    columns: attr(&s, b"xSplit")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0),
                    rows: attr(&s, b"ySplit")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0),
                });
            }
            Ok(Event::Start(s))
                if xml::local_name(s.name().as_ref()) == b"conditionalFormatting" =>
            {
                conditional = conditional.saturating_add(1)
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => {
                return Err(RenderError::malformed(
                    "a worksheet is malformed; obtain a fresh copy",
                ));
            }
        }
    }
    if cells.len() as u64 > options.limits.cells {
        return Err(RenderError::limit("cells", cells.len() as u64));
    }
    if conditional > 0 {
        unrendered.push(Unrendered::ConditionalFormatting {
            page: sheet,
            rules: conditional,
        });
    }
    let mut merged_cells = 0_u64;
    for (start, end) in &merges {
        if end.0 < start.0 || end.1 < start.1 {
            return Err(RenderError::malformed(
                "a merged-cell range is reversed; repair the workbook and try again",
            ));
        }
        let area = u64::from(end.0 - start.0 + 1)
            .checked_mul(u64::from(end.1 - start.1 + 1))
            .ok_or_else(|| RenderError::limit("cells", u64::MAX))?;
        merged_cells = merged_cells
            .checked_add(area)
            .ok_or_else(|| RenderError::limit("cells", u64::MAX))?;
        if merged_cells > options.limits.cells {
            return Err(RenderError::limit("cells", merged_cells));
        }
    }
    let max_row = cells
        .iter()
        .map(|c| c.row)
        .chain(merges.iter().map(|(_, end)| end.0))
        .max()
        .unwrap_or(0);
    let max_col = cells
        .iter()
        .map(|c| c.column)
        .chain(merges.iter().map(|(_, end)| end.1))
        .max()
        .unwrap_or(0);
    let cols = (0..=max_col)
        .map(|c| *widths.get(&c).unwrap_or(&64.0))
        .collect::<Vec<_>>();
    let rows = (0..=max_row)
        .map(|row| heights.get(&row).copied().unwrap_or(default_row_height))
        .collect::<Vec<_>>();
    let xs = prefix(&cols);
    let ys = prefix(&rows);
    let merge_covered: BTreeSet<(u32, u32)> = merges
        .iter()
        .flat_map(|(a, b)| {
            (a.0..=b.0)
                .flat_map(move |r| (a.1..=b.1).map(move |c| (r, c)))
                .filter(move |p| *p != *a)
        })
        .collect();
    let mut items = Vec::new();
    for cell in cells {
        if merge_covered.contains(&(cell.row, cell.column)) {
            continue;
        }
        let mut end = (cell.row, cell.column);
        if let Some((_, b)) = merges.iter().find(|(a, _)| *a == (cell.row, cell.column)) {
            end = *b;
        }
        let x = xs[cell.column as usize];
        let y = ys[cell.row as usize];
        let width = xs[end.1 as usize + 1] - x;
        let height = ys[end.0 as usize + 1] - y;
        let source = SourceRef::Cell {
            sheet,
            row: cell.row,
            column: cell.column,
        };
        let cell_style = styles.cells.get(cell.style).cloned().unwrap_or_default();
        let fill = cell_style
            .fill
            .colour
            .filter(|_| !cell_style.fill.pattern.is_empty() && cell_style.fill.pattern != "none");
        if let Some(colour) = fill {
            items.push(Item::Path(PathItem {
                path: rect_path(Rect {
                    x,
                    y,
                    width,
                    height,
                }),
                fill: Some(Paint { colour }),
                stroke: None,
                source: Some(source.clone()),
            }));
        }
        paint_borders(
            &mut items,
            Rect {
                x,
                y,
                width,
                height,
            },
            &cell_style,
            source.clone(),
        );
        if cell.formula_without_value {
            unrendered.push(Unrendered::FormulaWithoutCachedValue {
                sheet,
                row: cell.row,
                column: cell.column,
            });
        } else if !cell.value.is_empty() {
            let mut text_style = TextStyle {
                family: cell_style.font.family.clone(),
                size_px: cell_style.font.size_px,
                colour: Some(cell_style.font.colour),
                bold: cell_style.font.bold,
                italic: cell_style.font.italic,
                rotation_deg: match cell_style.alignment.rotation {
                    255 => 90.0,
                    value if value > 90 => f32::from(value - 180),
                    value => f32::from(value),
                },
            };
            let available = (width - 8.0 - cell_style.alignment.indent as f32 * 3.0).max(1.0);
            if cell_style.alignment.shrink {
                let measured = measure(&cell.value, &text_style);
                if measured > available {
                    text_style.size_px = (text_style.size_px * available / measured).max(4.0);
                }
            }
            let lines = if cell_style.alignment.wrap {
                wrap_cell(&cell.value, &text_style, available)
            } else {
                vec![cell.value.clone()]
            };
            let line_height = text_style.size_px * 1.2;
            let block_height = line_height * lines.len() as f32;
            let first_baseline = match cell_style.alignment.vertical.as_str() {
                "top" => y + text_style.size_px,
                "center" => y + (height - block_height) / 2.0 + text_style.size_px,
                _ => y + height - block_height + text_style.size_px - 2.0,
            };
            for (line_index, line) in lines.iter().enumerate() {
                let mut line_items = Vec::new();
                let line_width = measure(line, &text_style);
                let text_x = match cell_style.alignment.horizontal.as_str() {
                    "center" | "centerContinuous" => x + (width - line_width) / 2.0,
                    "right" => x + width - line_width - 4.0,
                    "left" => x + 4.0 + cell_style.alignment.indent as f32 * 3.0,
                    _ if cell.numeric => x + width - line_width - 4.0,
                    _ => x + 4.0 + cell_style.alignment.indent as f32 * 3.0,
                };
                let baseline = first_baseline + line_index as f32 * line_height;
                line_items.push(Item::Glyphs(shape(
                    line,
                    &text_style,
                    Point {
                        x: text_x,
                        y: baseline,
                    },
                    Some(source.clone()),
                )));
                if cell_style.font.underline || cell_style.font.strike {
                    let line_y = if cell_style.font.strike {
                        baseline - text_style.size_px * 0.35
                    } else {
                        baseline + text_style.size_px * 0.08
                    };
                    line_items.push(line_item(
                        Point {
                            x: text_x,
                            y: line_y,
                        },
                        Point {
                            x: text_x + line_width,
                            y: line_y,
                        },
                        cell_style.font.colour,
                        1.0,
                        Vec::new(),
                        source.clone(),
                    ));
                }
                items.push(Item::Group(Group {
                    items: line_items,
                    clip: Some(Rect {
                        x,
                        y,
                        width,
                        height,
                    }),
                    source: Some(source.clone()),
                }));
            }
        }
    }
    Ok((
        Page {
            size: Size {
                width: *xs.last().unwrap_or(&1.0),
                height: *ys.last().unwrap_or(&1.0),
            },
            label: None,
            items,
            source: None,
            frozen,
        },
        unrendered,
    ))
}

fn cell_value(
    kind: &str,
    value: &str,
    inline: &str,
    shared: &[String],
    format: &str,
    date_1904: bool,
) -> String {
    match kind {
        "s" => value
            .parse::<usize>()
            .ok()
            .and_then(|i| shared.get(i))
            .cloned()
            .map(|text| numfmt::format_text(&text, format))
            .unwrap_or_default(),
        "inlineStr" => numfmt::format_text(inline, format),
        "b" => {
            if value == "1" {
                "TRUE".into()
            } else {
                "FALSE".into()
            }
        }
        "e" | "d" => value.into(),
        "str" => numfmt::format_text(value, format),
        _ if format.eq_ignore_ascii_case("general") => {
            numfmt::format_general_lexical(value).unwrap_or_else(|| value.into())
        }
        _ => value
            .parse::<f64>()
            .map(|n| numfmt::format_number(n, format, date_1904))
            .unwrap_or_else(|_| value.into()),
    }
}
fn prefix(values: &[f32]) -> Vec<f32> {
    let mut out = Vec::with_capacity(values.len() + 1);
    out.push(0.0);
    for value in values {
        out.push(out.last().copied().unwrap_or(0.0) + value);
    }
    out
}

fn wrap_cell(text: &str, style: &TextStyle, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for hard_line in text.split('\n') {
        let mut line = String::new();
        for word in hard_line.split_inclusive(char::is_whitespace) {
            let candidate = format!("{line}{word}");
            if !line.is_empty() && measure(&candidate, style) > width {
                lines.push(line.trim_end().to_owned());
                line.clear();
            }
            line.push_str(word);
        }
        lines.push(line.trim_end().to_owned());
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn paint_borders(items: &mut Vec<Item>, rect: Rect, style: &CellStyle, source: SourceRef) {
    for (side, from, to) in [
        (
            &style.border.top,
            Point {
                x: rect.x,
                y: rect.y,
            },
            Point {
                x: rect.x + rect.width,
                y: rect.y,
            },
        ),
        (
            &style.border.right,
            Point {
                x: rect.x + rect.width,
                y: rect.y,
            },
            Point {
                x: rect.x + rect.width,
                y: rect.y + rect.height,
            },
        ),
        (
            &style.border.bottom,
            Point {
                x: rect.x,
                y: rect.y + rect.height,
            },
            Point {
                x: rect.x + rect.width,
                y: rect.y + rect.height,
            },
        ),
        (
            &style.border.left,
            Point {
                x: rect.x,
                y: rect.y,
            },
            Point {
                x: rect.x,
                y: rect.y + rect.height,
            },
        ),
    ] {
        if side.style.is_empty() {
            continue;
        }
        let (width, dash) = border_geometry(&side.style);
        items.push(line_item(
            from,
            to,
            side.colour.unwrap_or(Colour::BLACK),
            width,
            dash,
            source.clone(),
        ));
    }
}

fn border_geometry(style: &str) -> (f32, Vec<f32>) {
    match style {
        "medium" | "mediumDashDot" | "mediumDashDotDot" | "mediumDashed" => (2.0, vec![5.0, 3.0]),
        "thick" | "double" => (3.0, Vec::new()),
        "dashed" => (1.0, vec![4.0, 3.0]),
        "dotted" | "hair" => (1.0, vec![1.0, 2.0]),
        "dashDot" => (1.0, vec![4.0, 2.0, 1.0, 2.0]),
        "dashDotDot" => (1.0, vec![4.0, 2.0, 1.0, 2.0, 1.0, 2.0]),
        _ => (1.0, Vec::new()),
    }
}

fn line_item(
    from: Point,
    to: Point,
    colour: Colour,
    width: f32,
    dash: Vec<f32>,
    source: SourceRef,
) -> Item {
    Item::Path(PathItem {
        path: Path {
            commands: vec![PathCommand::Move(from), PathCommand::Line(to)],
        },
        fill: None,
        stroke: Some(Stroke {
            paint: Paint { colour },
            width,
            dash,
        }),
        source: Some(source),
    })
}

fn cell_ref(value: &str) -> Option<(u32, u32)> {
    let split = value.find(|c: char| c.is_ascii_digit())?;
    let mut column = 0_u32;
    for ch in value[..split].chars().filter(|c| *c != '$') {
        if !ch.is_ascii_alphabetic() {
            return None;
        }
        column = column
            .checked_mul(26)?
            .checked_add(u32::from(ch.to_ascii_uppercase()) - u32::from('A') + 1)?;
    }
    let row = value[split..].trim_start_matches('$').parse::<u32>().ok()?;
    Some((row.saturating_sub(1), column.saturating_sub(1)))
}
fn number_attr(s: &BytesStart<'_>, name: &[u8]) -> Option<u32> {
    attr(s, name)?.parse().ok()
}
fn attr(s: &BytesStart<'_>, name: &[u8]) -> Option<String> {
    s.attributes()
        .with_checks(false)
        .flatten()
        .find(|a| xml::local_name(a.key.as_ref()) == name)
        .map(|a| String::from_utf8_lossy(a.value.as_ref()).into_owned())
}
pub(crate) fn builtin_format(id: u32) -> &'static str {
    match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        5 => "$#,##0_);($#,##0)",
        6 => "$#,##0_);[Red]($#,##0)",
        7 => "$#,##0.00_);($#,##0.00)",
        8 => "$#,##0.00_);[Red]($#,##0.00)",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => "m/d/yy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yy h:mm",
        23..=26 => "General",
        27..=31 | 36 => "m/d/yy",
        32 => "h:mm",
        33 => "h:mm:ss",
        34 => "m/d/yy",
        35 => "m/d",
        37 => "#,##0 ;(#,##0)",
        38 => "#,##0 ;[Red](#,##0)",
        39 => "#,##0.00;(#,##0.00)",
        40 => "#,##0.00;[Red](#,##0.00)",
        41 => "_(* #,##0_);_(* (#,##0);_(* \"-\"_);_(@_)",
        42 => "_($* #,##0_);_($* (#,##0);_($* \"-\"_);_(@_)",
        43 => "_(* #,##0.00_);_(* (#,##0.00);_(* \"-\"??_);_(@_)",
        44 => "_($* #,##0.00_);_($* (#,##0.00);_($* \"-\"??_);_(@_)",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mmss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => "General",
    }
}
fn dedupe_unrendered(values: &mut Vec<Unrendered>) {
    let mut seen = BTreeSet::new();
    values.retain(|value| seen.insert(serde_json::to_string(value).unwrap_or_default()));
}

#[cfg(test)]
mod tests {
    use super::cell_value;

    #[test]
    fn general_numbers_are_formatted_instead_of_echoing_xml_lexemes() {
        assert_eq!(cell_value("n", "14.0", "", &[], "General", false), "14");
        assert_eq!(
            cell_value("n", "99.24450792793081", "", &[], "General", false),
            "99.2445079279308"
        );
        assert_eq!(
            cell_value("n", "4.638514392343736", "", &[], "General", false),
            "4.63851439234374"
        );
        assert_eq!(
            cell_value("n", "1.296049239E9", "", &[], "General", false),
            "1296049239"
        );
    }

    #[test]
    fn shared_strings_unescape_xml_comparison_characters() {
        let values = super::parse_shared_strings(
            br#"<sst><si><t>&lt;1yr</t></si><si><t>&gt;9yrs</t></si></sst>"#,
            &crate::Limits::default(),
        )
        .unwrap_or_else(|error| panic!("the shared string table is valid: {error}"));
        assert_eq!(values, ["<1yr", ">9yrs"]);
    }
}
