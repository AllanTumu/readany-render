use crate::RenderError;
use crate::container::xml;
use crate::model::Colour;
use quick_xml::events::{BytesStart, Event};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FontStyle {
    pub family: String,
    pub size_px: f32,
    pub colour: Colour,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
}

impl Default for FontStyle {
    fn default() -> Self {
        Self {
            family: "Calibri".into(),
            size_px: 11.0 * 96.0 / 72.0,
            colour: Colour::BLACK,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct FillStyle {
    pub pattern: String,
    pub colour: Option<Colour>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct BorderSide {
    pub style: String,
    pub colour: Option<Colour>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct BorderStyle {
    pub left: BorderSide,
    pub right: BorderSide,
    pub top: BorderSide,
    pub bottom: BorderSide,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct AlignmentStyle {
    pub horizontal: String,
    pub vertical: String,
    pub wrap: bool,
    pub indent: u32,
    pub shrink: bool,
    pub rotation: i16,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CellStyle {
    pub font: FontStyle,
    pub fill: FillStyle,
    pub border: BorderStyle,
    pub alignment: AlignmentStyle,
    pub number_format: String,
}

impl Default for CellStyle {
    fn default() -> Self {
        Self {
            font: FontStyle::default(),
            fill: FillStyle::default(),
            border: BorderStyle::default(),
            alignment: AlignmentStyle::default(),
            number_format: "General".into(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Styles {
    pub cells: Vec<CellStyle>,
    pub default_font: FontStyle,
}

impl Default for Styles {
    fn default() -> Self {
        Self {
            cells: vec![CellStyle::default()],
            default_font: FontStyle::default(),
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Xf {
    num_fmt: u32,
    font: usize,
    fill: usize,
    border: usize,
    alignment: AlignmentStyle,
}

pub(crate) fn parse(bytes: &[u8]) -> Result<Styles, RenderError> {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut section = Section::None;
    let mut fonts = Vec::new();
    let mut fills = Vec::new();
    let mut borders = Vec::new();
    let mut xfs = Vec::new();
    let mut formats = BTreeMap::new();
    let mut font: Option<FontStyle> = None;
    let mut fill: Option<FillStyle> = None;
    let mut border: Option<BorderStyle> = None;
    let mut border_side: Option<Side> = None;
    let mut xf: Option<Xf> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => {
                let qualified_name = start.name();
                let name = xml::local_name(qualified_name.as_ref());
                match name {
                    b"fonts" => section = Section::Fonts,
                    b"fills" => section = Section::Fills,
                    b"borders" => section = Section::Borders,
                    b"cellXfs" => section = Section::CellXfs,
                    b"font" if section == Section::Fonts => {
                        // Spreadsheet producers commonly omit properties that match the
                        // workbook's base font. Preserve those inherited properties rather
                        // than resetting every font record to Calibri 11 pt.
                        font = Some(fonts.first().cloned().unwrap_or_default());
                    }
                    b"fill" if section == Section::Fills => fill = Some(FillStyle::default()),
                    b"border" if section == Section::Borders => {
                        border = Some(BorderStyle::default())
                    }
                    b"xf" if section == Section::CellXfs => xf = Some(parse_xf(&start)),
                    b"left" if border.is_some() => {
                        set_side(&mut border, Side::Left, &start);
                        border_side = Some(Side::Left);
                    }
                    b"right" if border.is_some() => {
                        set_side(&mut border, Side::Right, &start);
                        border_side = Some(Side::Right);
                    }
                    b"top" if border.is_some() => {
                        set_side(&mut border, Side::Top, &start);
                        border_side = Some(Side::Top);
                    }
                    b"bottom" if border.is_some() => {
                        set_side(&mut border, Side::Bottom, &start);
                        border_side = Some(Side::Bottom);
                    }
                    _ => apply_element(
                        name,
                        &start,
                        &mut font,
                        &mut fill,
                        &mut border,
                        border_side,
                        &mut xf,
                    ),
                }
            }
            Ok(Event::Empty(start)) => {
                let qualified_name = start.name();
                let name = xml::local_name(qualified_name.as_ref());
                if name == b"numFmt" {
                    if let (Some(id), Some(code)) =
                        (number(&start, b"numFmtId"), attr(&start, b"formatCode"))
                    {
                        formats.insert(id, code);
                    }
                } else if name == b"xf" && section == Section::CellXfs {
                    xfs.push(parse_xf(&start));
                } else if matches!(name, b"left" | b"right" | b"top" | b"bottom")
                    && border.is_some()
                {
                    let side = match name {
                        b"left" => Side::Left,
                        b"right" => Side::Right,
                        b"top" => Side::Top,
                        b"bottom" => Side::Bottom,
                        _ => Side::Left,
                    };
                    set_side(&mut border, side, &start);
                } else {
                    apply_element(
                        name,
                        &start,
                        &mut font,
                        &mut fill,
                        &mut border,
                        border_side,
                        &mut xf,
                    );
                }
            }
            Ok(Event::End(end)) => {
                let qualified_name = end.name();
                let name = xml::local_name(qualified_name.as_ref());
                match name {
                    b"fonts" | b"fills" | b"borders" | b"cellXfs" => section = Section::None,
                    b"font" if font.is_some() => {
                        if let Some(value) = font.take() {
                            fonts.push(value);
                        }
                    }
                    b"fill" if fill.is_some() => {
                        if let Some(value) = fill.take() {
                            fills.push(value);
                        }
                    }
                    b"border" if border.is_some() => {
                        if let Some(value) = border.take() {
                            borders.push(value);
                        }
                    }
                    b"xf" if xf.is_some() => {
                        if let Some(value) = xf.take() {
                            xfs.push(value);
                        }
                    }
                    b"left" | b"right" | b"top" | b"bottom" => border_side = None,
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => {
                return Err(RenderError::malformed(
                    "workbook styles are malformed; obtain a fresh copy",
                ));
            }
        }
    }
    if fonts.is_empty() {
        fonts.push(FontStyle::default());
    }
    if fills.is_empty() {
        fills.push(FillStyle::default());
    }
    if borders.is_empty() {
        borders.push(BorderStyle::default());
    }
    if xfs.is_empty() {
        xfs.push(Xf::default());
    }
    let cells = xfs
        .into_iter()
        .map(|xf| CellStyle {
            font: fonts.get(xf.font).cloned().unwrap_or_default(),
            fill: fills.get(xf.fill).cloned().unwrap_or_default(),
            border: borders.get(xf.border).cloned().unwrap_or_default(),
            alignment: xf.alignment,
            number_format: formats
                .get(&xf.num_fmt)
                .cloned()
                .unwrap_or_else(|| super::xlsx::builtin_format(xf.num_fmt).into()),
        })
        .collect();
    Ok(Styles {
        default_font: fonts.first().cloned().unwrap_or_default(),
        cells,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    None,
    Fonts,
    Fills,
    Borders,
    CellXfs,
}

#[derive(Clone, Copy)]
enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

fn parse_xf(start: &BytesStart<'_>) -> Xf {
    Xf {
        num_fmt: number(start, b"numFmtId").unwrap_or(0),
        font: number(start, b"fontId").unwrap_or(0) as usize,
        fill: number(start, b"fillId").unwrap_or(0) as usize,
        border: number(start, b"borderId").unwrap_or(0) as usize,
        alignment: AlignmentStyle::default(),
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_element(
    name: &[u8],
    start: &BytesStart<'_>,
    font: &mut Option<FontStyle>,
    fill: &mut Option<FillStyle>,
    border: &mut Option<BorderStyle>,
    border_side: Option<Side>,
    xf: &mut Option<Xf>,
) {
    if let Some(font) = font {
        match name {
            b"name" => font.family = attr(start, b"val").unwrap_or_else(|| font.family.clone()),
            b"sz" => {
                if let Some(points) = attr(start, b"val").and_then(|v| v.parse::<f32>().ok()) {
                    font.size_px = points * 96.0 / 72.0;
                }
            }
            b"color" => font.colour = parse_colour(start).unwrap_or(font.colour),
            b"b" => font.bold = boolean(start, true),
            b"i" => font.italic = boolean(start, true),
            b"u" => font.underline = boolean(start, true),
            b"strike" => font.strike = boolean(start, true),
            _ => {}
        }
    }
    if let Some(fill) = fill {
        match name {
            b"patternFill" => fill.pattern = attr(start, b"patternType").unwrap_or_default(),
            b"fgColor" => fill.colour = parse_colour(start),
            _ => {}
        }
    }
    if name == b"color" {
        if let (Some(border), Some(side), Some(colour)) = (border, border_side, parse_colour(start))
        {
            side_mut(border, side).colour = Some(colour);
        }
    }
    if name == b"alignment" {
        if let Some(xf) = xf {
            xf.alignment = AlignmentStyle {
                horizontal: attr(start, b"horizontal").unwrap_or_default(),
                vertical: attr(start, b"vertical").unwrap_or_default(),
                wrap: bool_attr(start, b"wrapText"),
                indent: number(start, b"indent").unwrap_or(0),
                shrink: bool_attr(start, b"shrinkToFit"),
                rotation: attr(start, b"textRotation")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0),
            };
        }
    }
}

fn set_side(border: &mut Option<BorderStyle>, side: Side, start: &BytesStart<'_>) {
    if let Some(border) = border {
        side_mut(border, side).style = attr(start, b"style").unwrap_or_default();
    }
}

fn side_mut(border: &mut BorderStyle, side: Side) -> &mut BorderSide {
    match side {
        Side::Left => &mut border.left,
        Side::Right => &mut border.right,
        Side::Top => &mut border.top,
        Side::Bottom => &mut border.bottom,
    }
}

fn parse_colour(start: &BytesStart<'_>) -> Option<Colour> {
    if let Some(value) = attr(start, b"rgb") {
        let rgb = value.strip_prefix('#').unwrap_or(&value);
        let rgb = if rgb.len() == 8 { &rgb[2..] } else { rgb };
        if rgb.len() == 6 {
            return Some(Colour {
                r: u8::from_str_radix(&rgb[0..2], 16).ok()?,
                g: u8::from_str_radix(&rgb[2..4], 16).ok()?,
                b: u8::from_str_radix(&rgb[4..6], 16).ok()?,
                a: 255,
            });
        }
    }
    const INDEXED: [Colour; 16] = [
        Colour {
            r: 0,
            g: 0,
            b: 0,
            a: 255,
        },
        Colour {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
        Colour {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        },
        Colour {
            r: 0,
            g: 255,
            b: 0,
            a: 255,
        },
        Colour {
            r: 0,
            g: 0,
            b: 255,
            a: 255,
        },
        Colour {
            r: 255,
            g: 255,
            b: 0,
            a: 255,
        },
        Colour {
            r: 255,
            g: 0,
            b: 255,
            a: 255,
        },
        Colour {
            r: 0,
            g: 255,
            b: 255,
            a: 255,
        },
        Colour {
            r: 128,
            g: 0,
            b: 0,
            a: 255,
        },
        Colour {
            r: 0,
            g: 128,
            b: 0,
            a: 255,
        },
        Colour {
            r: 0,
            g: 0,
            b: 128,
            a: 255,
        },
        Colour {
            r: 128,
            g: 128,
            b: 0,
            a: 255,
        },
        Colour {
            r: 128,
            g: 0,
            b: 128,
            a: 255,
        },
        Colour {
            r: 0,
            g: 128,
            b: 128,
            a: 255,
        },
        Colour {
            r: 192,
            g: 192,
            b: 192,
            a: 255,
        },
        Colour {
            r: 128,
            g: 128,
            b: 128,
            a: 255,
        },
    ];
    if let Some(index) = number(start, b"indexed") {
        return INDEXED.get(index as usize).copied();
    }
    match number(start, b"theme") {
        Some(0) => Some(Colour::WHITE),
        Some(1) => Some(Colour::BLACK),
        _ => None,
    }
}

fn boolean(start: &BytesStart<'_>, absent_value: bool) -> bool {
    attr(start, b"val")
        .map(|value| value != "0" && !value.eq_ignore_ascii_case("false"))
        .unwrap_or(absent_value)
}

fn bool_attr(start: &BytesStart<'_>, name: &[u8]) -> bool {
    attr(start, name).is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
}

fn number(start: &BytesStart<'_>, name: &[u8]) -> Option<u32> {
    attr(start, name)?.parse().ok()
}

fn attr(start: &BytesStart<'_>, name: &[u8]) -> Option<String> {
    start
        .attributes()
        .with_checks(false)
        .flatten()
        .find(|attribute| xml::local_name(attribute.key.as_ref()) == name)
        .map(|attribute| String::from_utf8_lossy(attribute.value.as_ref()).into_owned())
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn later_fonts_inherit_omitted_base_properties() {
        let styles = parse(
            br#"<styleSheet><fonts count="2"><font><sz val="10"/><name val="Arial"/></font><font><b/></font></fonts><fills><fill/></fills><borders><border/></borders><cellXfs><xf fontId="1"/></cellXfs></styleSheet>"#,
        )
        .unwrap_or_else(|error| panic!("styles should parse: {error}"));

        assert_eq!(styles.cells[0].font.family, "Arial");
        assert!((styles.cells[0].font.size_px - 13.333_333).abs() < 0.001);
        assert!(styles.cells[0].font.bold);
    }
}
