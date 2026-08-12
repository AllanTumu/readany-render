use crate::model::*;
use crate::sheet::{paint_sheet_headers, sheet_grid, sheet_origin};
use crate::text::{TextStyle, measure, shape};
use crate::{Format, Options, RenderError, RenderErrorCode};
use encoding_rs::{UTF_16BE, UTF_16LE, WINDOWS_1252};

pub(crate) fn render(
    bytes: &[u8],
    format: Format,
    options: &Options<'_>,
) -> Result<Rendered, RenderError> {
    let text = decode(bytes)?;
    let separator = if format == Format::Tsv {
        '\t'
    } else {
        sniff_separator(&text)
    };
    let rows = parse(&text, separator, options.limits.cells)?;
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let style = TextStyle {
        family: "Carlito".into(),
        size_px: 14.666_667,
        ..TextStyle::default()
    };
    let mut widths = vec![80.0_f32; columns];
    for row in &rows {
        for (column, value) in row.iter().enumerate() {
            widths[column] = widths[column].max((measure(value, &style) + 12.0).min(400.0));
        }
    }
    let height = 22.0_f32;
    let page_width = widths.iter().sum::<f32>().max(1.0);
    let page_height = (rows.len() as f32 * height).max(1.0);
    let origin = sheet_origin(options.sheet_headers);
    let mut xs = Vec::with_capacity(widths.len() + 1);
    xs.push(0.0);
    for width in &widths {
        xs.push(xs.last().copied().unwrap_or(0.0) + width);
    }
    let ys = (0..=rows.len())
        .map(|row| row as f32 * height)
        .collect::<Vec<_>>();
    let mut items = Vec::new();
    let mut y = origin.y;
    for (row_index, row) in rows.iter().enumerate() {
        let mut x = origin.x;
        for (column, column_width) in widths.iter().copied().enumerate().take(columns) {
            let source = SourceRef::Cell {
                sheet: 0,
                row: u32::try_from(row_index).unwrap_or(u32::MAX),
                column: u32::try_from(column).unwrap_or(u32::MAX),
            };
            let rect = Rect {
                x,
                y,
                width: column_width,
                height,
            };
            items.push(Item::Path(PathItem {
                path: rect_path(rect),
                fill: Some(Paint {
                    colour: Colour::WHITE,
                }),
                stroke: Some(Stroke {
                    paint: Paint {
                        colour: Colour {
                            r: 208,
                            g: 215,
                            b: 222,
                            a: 255,
                        },
                    },
                    width: 1.0,
                    dash: Vec::new(),
                }),
                source: Some(source.clone()),
            }));
            if let Some(value) = row.get(column).filter(|value| !value.is_empty()) {
                items.push(Item::Glyphs(shape(
                    value,
                    &style,
                    Point {
                        x: x + 6.0,
                        y: y + 16.0,
                    },
                    Some(source),
                )));
            }
            x += column_width;
        }
        y += height;
    }
    if options.sheet_headers {
        paint_sheet_headers(&mut items, &xs, &ys, 0);
    }
    Ok(Rendered {
        pages: vec![Page {
            size: Size {
                width: origin.x + page_width,
                height: origin.y + page_height,
            },
            label: options.filename.map(str::to_owned),
            items,
            source: None,
            frozen: None,
            grid: Some(sheet_grid(&xs, &ys, origin)),
        }],
        format,
        unrendered: Vec::new(),
        meta: Meta::default(),
    })
}

fn decode(bytes: &[u8]) -> Result<String, RenderError> {
    if let Some(rest) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return String::from_utf8(rest.to_vec()).map_err(|_| {
            RenderError::new(
                RenderErrorCode::InvalidEncoding,
                "the UTF-8 text is malformed; save it as UTF-8 and try again",
            )
        });
    }
    if let Some(rest) = bytes.strip_prefix(&[0xff, 0xfe]) {
        let (value, _, malformed) = UTF_16LE.decode(rest);
        return (!malformed).then(|| value.into_owned()).ok_or_else(|| {
            RenderError::new(
                RenderErrorCode::InvalidEncoding,
                "the UTF-16 text is malformed; save it as UTF-8 and try again",
            )
        });
    }
    if let Some(rest) = bytes.strip_prefix(&[0xfe, 0xff]) {
        let (value, _, malformed) = UTF_16BE.decode(rest);
        return (!malformed).then(|| value.into_owned()).ok_or_else(|| {
            RenderError::new(
                RenderErrorCode::InvalidEncoding,
                "the UTF-16 text is malformed; save it as UTF-8 and try again",
            )
        });
    }
    match std::str::from_utf8(bytes) {
        Ok(value) => Ok(value.to_owned()),
        Err(_) => {
            let (value, _, _) = WINDOWS_1252.decode(bytes);
            Ok(value.into_owned())
        }
    }
}

fn sniff_separator(text: &str) -> char {
    [',', ';', '\t', '|']
        .into_iter()
        .max_by_key(|separator| {
            text.lines()
                .take(20)
                .map(|line| line.matches(*separator).count())
                .sum::<usize>()
        })
        .unwrap_or(',')
}

fn parse(text: &str, separator: char, limit: u64) -> Result<Vec<Vec<String>>, RenderError> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    let mut cells = 0_u64;
    while let Some(character) = chars.next() {
        match character {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            value if value == separator && !quoted => {
                push_field(&mut row, &mut field, &mut cells, limit)?
            }
            '\r' if !quoted => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                push_field(&mut row, &mut field, &mut cells, limit)?;
                rows.push(std::mem::take(&mut row));
            }
            '\n' if !quoted => {
                push_field(&mut row, &mut field, &mut cells, limit)?;
                rows.push(std::mem::take(&mut row));
            }
            value => field.push(value),
        }
    }
    if quoted {
        return Err(RenderError::malformed(
            "a quoted field never closes; repair the delimited text and try again",
        ));
    }
    if !field.is_empty() || !row.is_empty() {
        push_field(&mut row, &mut field, &mut cells, limit)?;
        rows.push(row);
    }
    Ok(rows)
}

fn push_field(
    row: &mut Vec<String>,
    field: &mut String,
    cells: &mut u64,
    limit: u64,
) -> Result<(), RenderError> {
    *cells = cells
        .checked_add(1)
        .ok_or_else(|| RenderError::limit("cells", u64::MAX))?;
    if *cells > limit {
        return Err(RenderError::limit("cells", *cells));
    }
    row.push(std::mem::take(field));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_newlines_and_doubled_quotes_remain_in_one_cell() {
        assert_eq!(
            parse("a,\"b\n\"\"c\"\r\nd,e", ',', 10).ok(),
            Some(vec![
                vec!["a".into(), "b\n\"c".into()],
                vec!["d".into(), "e".into()]
            ])
        );
    }
}
