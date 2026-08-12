pub(crate) mod csv;
pub(crate) mod numfmt;
pub(crate) mod ods;
pub(crate) mod styles;
pub(crate) mod xlsx;

use crate::model::*;
use crate::text::{TextStyle, measure, shape};

pub(crate) const DEFAULT_GRID_COLOUR: Colour = Colour {
    r: 217,
    g: 217,
    b: 217,
    a: 255,
};
pub(crate) const SHEET_HEADER_WIDTH: f32 = 48.0;
pub(crate) const SHEET_HEADER_HEIGHT: f32 = 24.0;

pub(crate) fn sheet_origin(headers: bool) -> Point {
    if headers {
        Point {
            x: SHEET_HEADER_WIDTH,
            y: SHEET_HEADER_HEIGHT,
        }
    } else {
        Point::default()
    }
}

pub(crate) fn paint_gridlines(
    items: &mut Vec<Item>,
    xs: &[f32],
    ys: &[f32],
    origin: Point,
    colour: Colour,
    sheet: u32,
) {
    let width = xs.last().copied().unwrap_or(0.0);
    let height = ys.last().copied().unwrap_or(0.0);
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    for (column, x) in xs.iter().copied().enumerate() {
        items.push(gridline(
            Point {
                x: origin.x + x,
                y: origin.y,
            },
            Point {
                x: origin.x + x,
                y: origin.y + height,
            },
            colour,
            SourceRef::Cell {
                sheet,
                row: 0,
                column: u32::try_from(column.saturating_sub(1)).unwrap_or(u32::MAX),
            },
        ));
    }
    for (row, y) in ys.iter().copied().enumerate() {
        items.push(gridline(
            Point {
                x: origin.x,
                y: origin.y + y,
            },
            Point {
                x: origin.x + width,
                y: origin.y + y,
            },
            colour,
            SourceRef::Cell {
                sheet,
                row: u32::try_from(row.saturating_sub(1)).unwrap_or(u32::MAX),
                column: 0,
            },
        ));
    }
}

pub(crate) fn paint_sheet_headers(items: &mut Vec<Item>, xs: &[f32], ys: &[f32], sheet: u32) {
    let Some(width) = xs.last().copied() else {
        return;
    };
    let Some(height) = ys.last().copied() else {
        return;
    };
    let background = Colour {
        r: 245,
        g: 246,
        b: 248,
        a: 255,
    };
    let edge = Colour {
        r: 190,
        g: 195,
        b: 201,
        a: 255,
    };
    for rect in [
        Rect {
            x: 0.0,
            y: 0.0,
            width: SHEET_HEADER_WIDTH + width,
            height: SHEET_HEADER_HEIGHT,
        },
        Rect {
            x: 0.0,
            y: SHEET_HEADER_HEIGHT,
            width: SHEET_HEADER_WIDTH,
            height,
        },
    ] {
        items.push(Item::Path(PathItem {
            path: rect_path(rect),
            fill: Some(Paint { colour: background }),
            stroke: Some(Stroke {
                paint: Paint { colour: edge },
                width: 1.0,
                dash: Vec::new(),
            }),
            source: Some(SourceRef::Cell {
                sheet,
                row: 0,
                column: 0,
            }),
        }));
    }
    let style = TextStyle {
        family: "Carlito".into(),
        size_px: 12.0,
        colour: Some(Colour {
            r: 55,
            g: 61,
            b: 68,
            a: 255,
        }),
        ..TextStyle::default()
    };
    for column in 0..xs.len().saturating_sub(1) {
        let label = column_label(u32::try_from(column + 1).unwrap_or(u32::MAX));
        let left = SHEET_HEADER_WIDTH + xs[column];
        let cell_width = xs[column + 1] - xs[column];
        let label_width = measure(&label, &style);
        let source = SourceRef::Cell {
            sheet,
            row: 0,
            column: u32::try_from(column).unwrap_or(u32::MAX),
        };
        items.push(Item::Glyphs(shape(
            &label,
            &style,
            Point {
                x: left + (cell_width - label_width).max(0.0) / 2.0,
                y: 16.0,
            },
            Some(source),
        )));
    }
    for row in 0..ys.len().saturating_sub(1) {
        let label = (row + 1).to_string();
        let top = SHEET_HEADER_HEIGHT + ys[row];
        let cell_height = ys[row + 1] - ys[row];
        let label_width = measure(&label, &style);
        let source = SourceRef::Cell {
            sheet,
            row: u32::try_from(row).unwrap_or(u32::MAX),
            column: 0,
        };
        items.push(Item::Glyphs(shape(
            &label,
            &style,
            Point {
                x: SHEET_HEADER_WIDTH - label_width - 6.0,
                y: top + (cell_height + style.size_px * 0.72) / 2.0,
            },
            Some(source),
        )));
    }
}

pub(crate) fn frozen_with_extents(
    frozen: Option<FrozenPanes>,
    xs: &[f32],
    ys: &[f32],
    origin: Point,
) -> Option<FrozenPanes> {
    frozen.map(|mut panes| {
        let column = usize::try_from(panes.columns)
            .unwrap_or(usize::MAX)
            .min(xs.len().saturating_sub(1));
        let row = usize::try_from(panes.rows)
            .unwrap_or(usize::MAX)
            .min(ys.len().saturating_sub(1));
        panes.columns = u32::try_from(column).unwrap_or(u32::MAX);
        panes.rows = u32::try_from(row).unwrap_or(u32::MAX);
        panes.width = origin.x + xs[column];
        panes.height = origin.y + ys[row];
        panes
    })
}

pub(crate) fn column_label(mut one_based: u32) -> String {
    if one_based == 0 {
        return String::new();
    }
    let mut bytes = Vec::new();
    while one_based > 0 {
        one_based -= 1;
        bytes.push(b'A' + u8::try_from(one_based % 26).unwrap_or(0));
        one_based /= 26;
    }
    bytes.reverse();
    String::from_utf8(bytes).unwrap_or_default()
}

fn gridline(from: Point, to: Point, colour: Colour, source: SourceRef) -> Item {
    Item::Path(PathItem {
        path: Path {
            commands: vec![PathCommand::Move(from), PathCommand::Line(to)],
        },
        fill: None,
        stroke: Some(Stroke {
            paint: Paint { colour },
            width: 1.0,
            dash: Vec::new(),
        }),
        source: Some(source),
    })
}

#[cfg(test)]
mod tests {
    use super::column_label;

    #[test]
    fn spreadsheet_column_labels_follow_bijective_base_26_through_zzz() {
        let labels = (1..=18_278).map(column_label).collect::<Vec<_>>();
        assert_eq!(labels.first().map(String::as_str), Some("A"));
        assert_eq!(labels.get(25).map(String::as_str), Some("Z"));
        assert_eq!(labels.get(26).map(String::as_str), Some("AA"));
        assert_eq!(labels.get(701).map(String::as_str), Some("ZZ"));
        assert_eq!(labels.last().map(String::as_str), Some("ZZZ"));
    }
}
