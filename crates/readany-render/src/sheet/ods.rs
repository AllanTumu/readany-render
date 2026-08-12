use crate::container::{xml, zip::Archive};
use crate::model::*;
use crate::sheet::styles::{
    AlignmentStyle, BorderSide, BorderStyle, CellStyle, FillStyle, FontStyle,
};
use crate::text::{TextStyle, measure, shape};
use crate::{Format, Options, RenderError};
use quick_xml::events::{BytesStart, Event};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn render(bytes: &[u8], options: &Options<'_>) -> Result<Rendered, RenderError> {
    let archive = Archive::open(bytes, &options.limits)?;
    let content = archive.required("content.xml")?;
    xml::validate(content, &options.limits)?;
    let mut styles = OdsStyles::default();
    if let Some(bytes) = archive.get("styles.xml") {
        xml::validate(bytes, &options.limits)?;
        styles.parse_part(bytes)?;
    }
    styles.parse_part(content)?;
    let frozen = archive
        .get("settings.xml")
        .map(parse_frozen)
        .transpose()?
        .flatten();
    let parsed = parse_tables(content, &styles, frozen, options)?;
    Ok(Rendered {
        pages: parsed.pages,
        format: Format::Ods,
        unrendered: parsed.unrendered,
        meta: Meta::default(),
    })
}

#[derive(Clone, Default)]
struct StyleDef {
    parent: Option<String>,
    cell: CellStyle,
    column_width: Option<f32>,
    row_height: Option<f32>,
}

#[derive(Default)]
struct OdsStyles {
    definitions: BTreeMap<String, StyleDef>,
}

impl OdsStyles {
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
                    } else if id.is_some() {
                        apply_style(&start, &mut style);
                    }
                }
                Ok(Event::Empty(start)) if id.is_some() => apply_style(&start, &mut style),
                Ok(Event::End(end)) if xml::local_name(end.name().as_ref()) == b"style" => {
                    if let Some(id) = id.take() {
                        self.definitions.insert(id, std::mem::take(&mut style));
                    }
                }
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(_) => return Err(RenderError::malformed("ODS styles are malformed")),
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
        let mut resolved = StyleDef::default();
        for style in chain.into_iter().rev() {
            overlay_style(&mut resolved, style);
        }
        resolved
    }
}

fn overlay_style(base: &mut StyleDef, child: &StyleDef) {
    if child.cell.font.family != FontStyle::default().family {
        base.cell.font.family.clone_from(&child.cell.font.family);
    }
    if child.cell.font.size_px != FontStyle::default().size_px {
        base.cell.font.size_px = child.cell.font.size_px;
    }
    if child.cell.font.colour != Colour::BLACK {
        base.cell.font.colour = child.cell.font.colour;
    }
    base.cell.font.bold |= child.cell.font.bold;
    base.cell.font.italic |= child.cell.font.italic;
    base.cell.font.underline |= child.cell.font.underline;
    base.cell.font.strike |= child.cell.font.strike;
    if child.cell.fill.colour.is_some() {
        base.cell.fill = child.cell.fill.clone();
    }
    if child.cell.border != BorderStyle::default() {
        base.cell.border = child.cell.border.clone();
    }
    if child.cell.alignment != AlignmentStyle::default() {
        base.cell.alignment = child.cell.alignment.clone();
    }
    base.column_width = child.column_width.or(base.column_width);
    base.row_height = child.row_height.or(base.row_height);
}

fn apply_style(start: &BytesStart<'_>, style: &mut StyleDef) {
    match xml::local_name(start.name().as_ref()) {
        b"text-properties" => {
            style.cell.font.family = attr(start, b"font-name")
                .or_else(|| attr(start, b"font-family"))
                .map(|value| value.trim_matches(['\'', '"']).to_owned())
                .unwrap_or_else(|| style.cell.font.family.clone());
            style.cell.font.size_px = attr(start, b"font-size")
                .as_deref()
                .and_then(length)
                .unwrap_or(style.cell.font.size_px);
            style.cell.font.colour = attr(start, b"color")
                .as_deref()
                .and_then(colour)
                .unwrap_or(style.cell.font.colour);
            style.cell.font.bold = attr(start, b"font-weight").as_deref() == Some("bold");
            style.cell.font.italic = attr(start, b"font-style").as_deref() == Some("italic");
            style.cell.font.underline =
                attr(start, b"text-underline-style").is_some_and(|value| value != "none");
            style.cell.font.strike =
                attr(start, b"text-line-through-style").is_some_and(|value| value != "none");
        }
        b"table-cell-properties" => {
            style.cell.fill = FillStyle {
                pattern: "solid".into(),
                colour: attr(start, b"background-color").as_deref().and_then(colour),
            };
            if let Some(border) = attr(start, b"border") {
                let side = parse_border(&border);
                style.cell.border = BorderStyle {
                    left: side.clone(),
                    right: side.clone(),
                    top: side.clone(),
                    bottom: side,
                };
            }
            for (name, side) in [
                (b"border-left".as_slice(), &mut style.cell.border.left),
                (b"border-right".as_slice(), &mut style.cell.border.right),
                (b"border-top".as_slice(), &mut style.cell.border.top),
                (b"border-bottom".as_slice(), &mut style.cell.border.bottom),
            ] {
                if let Some(value) = attr(start, name) {
                    *side = parse_border(&value);
                }
            }
            style.cell.alignment.vertical = attr(start, b"vertical-align").unwrap_or_default();
            style.cell.alignment.wrap = attr(start, b"wrap-option").as_deref() == Some("wrap");
            style.cell.alignment.rotation = attr(start, b"rotation-angle")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
        }
        b"paragraph-properties" => {
            style.cell.alignment.horizontal = attr(start, b"text-align").unwrap_or_default();
            style.cell.alignment.indent = attr(start, b"margin-left")
                .as_deref()
                .and_then(length)
                .unwrap_or(0.0)
                .round() as u32;
        }
        b"table-column-properties" => {
            style.column_width = attr(start, b"column-width").as_deref().and_then(length);
        }
        b"table-row-properties" => {
            style.row_height = attr(start, b"row-height")
                .or_else(|| attr(start, b"min-row-height"))
                .as_deref()
                .and_then(length);
        }
        _ => {}
    }
}

#[derive(Clone)]
struct Cell {
    text: String,
    style: String,
    columns: u32,
    rows: u32,
    covered: bool,
}

struct ParsedTables {
    pages: Vec<Page>,
    unrendered: Vec<Unrendered>,
}

fn parse_tables(
    bytes: &[u8],
    styles: &OdsStyles,
    frozen: Option<FrozenPanes>,
    options: &Options<'_>,
) -> Result<ParsedTables, RenderError> {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut pages = Vec::new();
    let mut unrendered = Vec::new();
    let mut table_name = String::new();
    let mut rows = Vec::<Vec<Cell>>::new();
    let mut row = Vec::<Cell>::new();
    let mut column_widths = Vec::<f32>::new();
    let mut row_heights = Vec::<f32>::new();
    let mut row_style = String::new();
    let mut row_repeat = 1_u32;
    let mut cell = None::<Cell>;
    let mut cell_repeat = 1_u32;
    let mut in_table = false;
    let mut in_text = false;
    let mut cells = 0_u64;
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) if xml::local_name(start.name().as_ref()) == b"table" => {
                in_table = true;
                table_name =
                    attr(&start, b"name").unwrap_or_else(|| format!("Sheet {}", pages.len() + 1));
                rows.clear();
                column_widths.clear();
                row_heights.clear();
                if attr(&start, b"display").as_deref() == Some("false") {
                    unrendered.push(Unrendered::HiddenSheet {
                        name: table_name.clone(),
                    });
                }
            }
            Ok(Event::End(end)) if xml::local_name(end.name().as_ref()) == b"table" => {
                in_table = false;
                if !unrendered.iter().any(|entry| matches!(entry, Unrendered::HiddenSheet { name } if name == &table_name)) {
                    pages.push(make_page(&table_name, &rows, &column_widths, &row_heights, pages.len() as u32, frozen, styles));
                }
            }
            Ok(Event::Empty(start)) | Ok(Event::Start(start))
                if in_table && xml::local_name(start.name().as_ref()) == b"table-column" =>
            {
                let repeat = number(&start, b"number-columns-repeated").unwrap_or(1);
                check_repeat(repeat, options.limits.cells)?;
                let width = styles
                    .resolve(attr(&start, b"style-name").as_deref())
                    .column_width
                    .unwrap_or(80.0);
                column_widths.extend(std::iter::repeat_n(width, repeat as usize));
            }
            Ok(Event::Start(start))
                if in_table && xml::local_name(start.name().as_ref()) == b"table-row" =>
            {
                row.clear();
                row_repeat = number(&start, b"number-rows-repeated").unwrap_or(1);
                check_repeat(row_repeat, options.limits.cells)?;
                row_style = attr(&start, b"style-name").unwrap_or_default();
            }
            Ok(Event::End(end))
                if in_table && xml::local_name(end.name().as_ref()) == b"table-row" =>
            {
                let added = (row.len() as u64).saturating_mul(u64::from(row_repeat));
                cells = cells
                    .checked_add(added)
                    .ok_or_else(|| RenderError::limit("cells", u64::MAX))?;
                if cells > options.limits.cells {
                    return Err(RenderError::limit("cells", cells));
                }
                let height = styles.resolve(Some(&row_style)).row_height.unwrap_or(22.0);
                for _ in 0..row_repeat {
                    rows.push(row.clone());
                    row_heights.push(height);
                }
            }
            Ok(Event::Start(start)) | Ok(Event::Empty(start))
                if in_table
                    && matches!(
                        xml::local_name(start.name().as_ref()),
                        b"table-cell" | b"covered-table-cell"
                    ) =>
            {
                let covered = xml::local_name(start.name().as_ref()) == b"covered-table-cell";
                cell_repeat = number(&start, b"number-columns-repeated").unwrap_or(1);
                check_repeat(cell_repeat, options.limits.cells)?;
                cell = Some(Cell {
                    text: value_text(&start),
                    style: attr(&start, b"style-name").unwrap_or_default(),
                    columns: number(&start, b"number-columns-spanned").unwrap_or(1),
                    rows: number(&start, b"number-rows-spanned").unwrap_or(1),
                    covered,
                });
                if start.is_empty() {
                    if let Some(value) = cell.take() {
                        row.extend(std::iter::repeat_n(value, cell_repeat as usize));
                    }
                }
            }
            Ok(Event::End(end))
                if cell.is_some()
                    && matches!(
                        xml::local_name(end.name().as_ref()),
                        b"table-cell" | b"covered-table-cell"
                    ) =>
            {
                if let Some(value) = cell.take() {
                    row.extend(std::iter::repeat_n(value, cell_repeat as usize));
                }
            }
            Ok(Event::Start(start))
                if cell.is_some()
                    && matches!(xml::local_name(start.name().as_ref()), b"p" | b"h") =>
            {
                in_text = true
            }
            Ok(Event::End(end)) if matches!(xml::local_name(end.name().as_ref()), b"p" | b"h") => {
                if let Some(cell) = &mut cell {
                    if !cell.text.is_empty() && !cell.text.ends_with('\n') {
                        cell.text.push('\n');
                    }
                }
                in_text = false;
            }
            Ok(Event::Empty(start))
                if in_text && xml::local_name(start.name().as_ref()) == b"line-break" =>
            {
                if let Some(cell) = &mut cell {
                    cell.text.push('\n');
                }
            }
            Ok(Event::Text(text)) if in_text => {
                if let Some(cell) = &mut cell {
                    cell.text.push_str(
                        &text
                            .decode()
                            .map_err(|_| RenderError::malformed("ODS text is malformed"))?,
                    );
                }
            }
            Ok(Event::Empty(start)) | Ok(Event::Start(start))
                if attr(&start, b"href").is_some_and(|target| target.contains("://")) =>
            {
                if let Some(target) = attr(&start, b"href") {
                    unrendered.push(Unrendered::ExternalReference { target });
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
    Ok(ParsedTables { pages, unrendered })
}

fn make_page(
    name: &str,
    rows: &[Vec<Cell>],
    provided_widths: &[f32],
    row_heights: &[f32],
    sheet: u32,
    frozen: Option<FrozenPanes>,
    styles: &OdsStyles,
) -> Page {
    let columns = rows
        .iter()
        .map(Vec::len)
        .max()
        .unwrap_or(0)
        .max(provided_widths.len());
    let widths = (0..columns)
        .map(|index| provided_widths.get(index).copied().unwrap_or(80.0))
        .collect::<Vec<_>>();
    let heights = (0..rows.len())
        .map(|index| row_heights.get(index).copied().unwrap_or(22.0))
        .collect::<Vec<_>>();
    let xs = prefix(&widths);
    let ys = prefix(&heights);
    let mut items = Vec::new();
    for (row_index, row) in rows.iter().enumerate() {
        for (column_index, cell) in row.iter().enumerate() {
            if cell.covered || column_index >= columns {
                continue;
            }
            let end_column = column_index
                .saturating_add(cell.columns as usize)
                .min(columns);
            let end_row = row_index.saturating_add(cell.rows as usize).min(rows.len());
            let rect = Rect {
                x: xs[column_index],
                y: ys[row_index],
                width: xs[end_column] - xs[column_index],
                height: ys[end_row] - ys[row_index],
            };
            let source = SourceRef::Cell {
                sheet,
                row: row_index as u32,
                column: column_index as u32,
            };
            let style = styles.resolve(Some(&cell.style)).cell;
            paint_cell(&mut items, rect, cell.text.trim_end(), source, &style);
        }
    }
    Page {
        size: Size {
            width: xs.last().copied().unwrap_or(0.0),
            height: ys.last().copied().unwrap_or(0.0),
        },
        label: Some(name.into()),
        items,
        source: None,
        frozen,
    }
}

fn paint_cell(items: &mut Vec<Item>, rect: Rect, text: &str, source: SourceRef, style: &CellStyle) {
    if let Some(colour) = style.fill.colour {
        items.push(Item::Path(PathItem {
            path: rect_path(rect),
            fill: Some(Paint { colour }),
            stroke: None,
            source: Some(source.clone()),
        }));
    }
    for (a, b, side) in [
        (
            (rect.x, rect.y),
            (rect.x + rect.width, rect.y),
            &style.border.top,
        ),
        (
            (rect.x, rect.y + rect.height),
            (rect.x + rect.width, rect.y + rect.height),
            &style.border.bottom,
        ),
        (
            (rect.x, rect.y),
            (rect.x, rect.y + rect.height),
            &style.border.left,
        ),
        (
            (rect.x + rect.width, rect.y),
            (rect.x + rect.width, rect.y + rect.height),
            &style.border.right,
        ),
    ] {
        if side.style.is_empty() {
            continue;
        }
        items.push(Item::Path(PathItem {
            path: Path {
                commands: vec![
                    PathCommand::Move(Point { x: a.0, y: a.1 }),
                    PathCommand::Line(Point { x: b.0, y: b.1 }),
                ],
            },
            fill: None,
            stroke: Some(Stroke {
                paint: Paint {
                    colour: side.colour.unwrap_or(Colour {
                        r: 210,
                        g: 215,
                        b: 220,
                        a: 255,
                    }),
                },
                width: border_width(&side.style),
                dash: border_dash(&side.style),
            }),
            source: Some(source.clone()),
        }));
    }
    if text.is_empty() {
        return;
    }
    let text_style = TextStyle {
        family: style.font.family.clone(),
        size_px: style.font.size_px,
        colour: Some(style.font.colour),
        bold: style.font.bold,
        italic: style.font.italic,
        rotation_deg: -(style.alignment.rotation as f32),
    };
    let text_width = measure(text, &text_style);
    let x = match style.alignment.horizontal.as_str() {
        "center" => rect.x + ((rect.width - text_width) / 2.0).max(3.0),
        "end" | "right" => rect.x + (rect.width - text_width - 4.0).max(3.0),
        _ => rect.x + 4.0 + style.alignment.indent as f32,
    };
    let y = match style.alignment.vertical.as_str() {
        "middle" => rect.y + (rect.height + text_style.size_px * 0.72) / 2.0,
        "bottom" => rect.y + rect.height - 4.0,
        _ => rect.y + text_style.size_px + 1.333_333,
    };
    items.push(Item::Group(Group {
        items: vec![Item::Glyphs(shape(
            text,
            &text_style,
            Point { x, y },
            Some(source.clone()),
        ))],
        clip: Some(rect),
        source: Some(source),
    }));
}

fn border_width(style: &str) -> f32 {
    match style {
        "double" | "thick" => 2.0,
        "medium" => 1.5,
        _ => 1.0,
    }
}

fn border_dash(style: &str) -> Vec<f32> {
    match style {
        "dashed" => vec![4.0, 3.0],
        "dotted" => vec![1.0, 2.0],
        _ => Vec::new(),
    }
}

fn parse_border(value: &str) -> BorderSide {
    let colour = value.split_whitespace().find_map(colour);
    let style = if value.contains("double") {
        "double"
    } else if value.contains("dashed") {
        "dashed"
    } else if value.contains("dotted") {
        "dotted"
    } else if value == "none" {
        "none"
    } else {
        "solid"
    };
    BorderSide {
        style: style.into(),
        colour,
    }
}

fn prefix(values: &[f32]) -> Vec<f32> {
    let mut output = Vec::with_capacity(values.len() + 1);
    output.push(0.0);
    for value in values {
        output.push(output.last().copied().unwrap_or(0.0) + value);
    }
    output
}

fn value_text(start: &BytesStart<'_>) -> String {
    match attr(start, b"value-type").as_deref() {
        Some("boolean") => attr(start, b"boolean-value")
            .map(|value| {
                if value == "true" {
                    "TRUE".into()
                } else {
                    "FALSE".into()
                }
            })
            .unwrap_or_default(),
        Some("date") => attr(start, b"date-value").unwrap_or_default(),
        Some("time") => attr(start, b"time-value").unwrap_or_default(),
        Some("string") => attr(start, b"string-value").unwrap_or_default(),
        _ => attr(start, b"value").unwrap_or_default(),
    }
}

fn parse_frozen(bytes: &[u8]) -> Result<Option<FrozenPanes>, RenderError> {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut name = String::new();
    let mut in_item = false;
    let mut columns = 0_u32;
    let mut rows = 0_u32;
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) if xml::local_name(start.name().as_ref()) == b"config-item" => {
                name = attr(&start, b"name").unwrap_or_default();
                in_item = true;
            }
            Ok(Event::Text(text)) if in_item => {
                let value = text
                    .decode()
                    .ok()
                    .and_then(|value| value.parse::<u32>().ok())
                    .unwrap_or(0);
                match name.as_str() {
                    "HorizontalSplitPosition" | "SplitPositionHorizontal" => columns = value,
                    "VerticalSplitPosition" | "SplitPositionVertical" => rows = value,
                    _ => {}
                }
            }
            Ok(Event::End(end)) if xml::local_name(end.name().as_ref()) == b"config-item" => {
                in_item = false
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err(RenderError::malformed("ODS view settings are malformed")),
        }
    }
    Ok((columns > 0 || rows > 0).then_some(FrozenPanes { rows, columns }))
}

fn check_repeat(value: u32, limit: u64) -> Result<(), RenderError> {
    if u64::from(value) > limit {
        Err(RenderError::limit("cells", u64::from(value)))
    } else {
        Ok(())
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

fn number(start: &BytesStart<'_>, name: &[u8]) -> Option<u32> {
    attr(start, name)?.parse().ok()
}

fn length(value: &str) -> Option<f32> {
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
    fn odf_lengths_are_converted_to_css_pixels() {
        assert!((length("2.54cm").unwrap_or_default() - 96.0).abs() < 0.001);
    }
}
