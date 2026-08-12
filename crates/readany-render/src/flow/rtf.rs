use crate::flow::{
    Alignment, FlowParagraph, FlowRun, ParagraphStyle, default_text_style, layout_flow,
};
use crate::model::{Colour, Size};
use crate::text::TextStyle;
use crate::{Format, Options, RenderError};
use encoding_rs::{Encoding, WINDOWS_1252};
use std::collections::BTreeMap;

pub(crate) fn render(bytes: &[u8], options: &Options<'_>) -> Result<crate::Rendered, RenderError> {
    let parsed = Parser::new(bytes)?.parse()?;
    layout_flow(
        &parsed.paragraphs,
        Format::Rtf,
        options,
        parsed.size,
        parsed.margins,
    )
}

struct ParsedRtf {
    paragraphs: Vec<FlowParagraph>,
    size: Size,
    margins: (f32, f32, f32, f32),
}

#[derive(Clone)]
struct State {
    style: TextStyle,
    paragraph: ParagraphStyle,
    codepage: &'static Encoding,
    unicode_skip: usize,
    skip_destination: bool,
}

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
    stack: Vec<State>,
    state: State,
    paragraphs: Vec<FlowParagraph>,
    runs: Vec<FlowRun>,
    text: String,
    fonts: BTreeMap<u32, String>,
    colours: Vec<Colour>,
    size: Size,
    margins: (f32, f32, f32, f32),
}

impl<'a> Parser<'a> {
    fn new(bytes: &'a [u8]) -> Result<Self, RenderError> {
        if !bytes.starts_with(b"{\\rtf") {
            return Err(RenderError::malformed(
                "the RTF header is missing; obtain a fresh copy",
            ));
        }
        Ok(Self {
            bytes,
            index: 0,
            stack: Vec::new(),
            state: State {
                style: default_text_style(),
                paragraph: ParagraphStyle::default(),
                codepage: WINDOWS_1252,
                unicode_skip: 1,
                skip_destination: false,
            },
            paragraphs: Vec::new(),
            runs: Vec::new(),
            text: String::new(),
            fonts: parse_font_table(bytes),
            colours: parse_colour_table(bytes),
            size: Size {
                width: 816.0,
                height: 1056.0,
            },
            margins: (96.0, 96.0, 96.0, 96.0),
        })
    }

    fn parse(mut self) -> Result<ParsedRtf, RenderError> {
        while self.index < self.bytes.len() {
            match self.bytes[self.index] {
                b'{' => {
                    self.stack.push(self.state.clone());
                    self.index += 1;
                }
                b'}' => {
                    self.flush_run();
                    self.state = self.stack.pop().ok_or_else(|| {
                        RenderError::malformed("RTF groups are unbalanced; obtain a fresh copy")
                    })?;
                    self.index += 1;
                }
                b'\\' => self.control()?,
                b'\r' | b'\n' => self.index += 1,
                byte => {
                    let start = self.index;
                    while self.index < self.bytes.len()
                        && !matches!(self.bytes[self.index], b'{' | b'}' | b'\\' | b'\r' | b'\n')
                    {
                        self.index += 1;
                    }
                    if !self.state.skip_destination {
                        let (decoded, _, _) =
                            self.state.codepage.decode(&self.bytes[start..self.index]);
                        self.text.push_str(&decoded);
                    }
                    if self.index == start {
                        self.index += usize::from(byte != 0);
                    }
                }
            }
        }
        if !self.stack.is_empty() {
            return Err(RenderError::malformed(
                "RTF groups are unbalanced; obtain a fresh copy",
            ));
        }
        self.finish_paragraph();
        Ok(ParsedRtf {
            paragraphs: self.paragraphs,
            size: self.size,
            margins: self.margins,
        })
    }

    fn control(&mut self) -> Result<(), RenderError> {
        self.index += 1;
        let Some(&symbol) = self.bytes.get(self.index) else {
            return Ok(());
        };
        match symbol {
            b'\\' | b'{' | b'}' => {
                if !self.state.skip_destination {
                    self.text.push(char::from(symbol));
                }
                self.index += 1;
                return Ok(());
            }
            b'\'' => {
                self.index += 1;
                let Some(hex) = self.bytes.get(self.index..self.index.saturating_add(2)) else {
                    return Ok(());
                };
                if let Ok(hex) = std::str::from_utf8(hex) {
                    if let Ok(value) = u8::from_str_radix(hex, 16) {
                        if !self.state.skip_destination {
                            let encoded = [value];
                            let (decoded, _, _) = self.state.codepage.decode(&encoded);
                            self.text.push_str(&decoded);
                        }
                    }
                }
                self.index = self.index.saturating_add(2).min(self.bytes.len());
                return Ok(());
            }
            b'*' => {
                self.state.skip_destination = true;
                self.index += 1;
                return Ok(());
            }
            b'~' => {
                if !self.state.skip_destination {
                    self.text.push('\u{a0}');
                }
                self.index += 1;
                return Ok(());
            }
            b'_' => {
                if !self.state.skip_destination {
                    self.text.push('\u{2011}');
                }
                self.index += 1;
                return Ok(());
            }
            b'-' => {
                if !self.state.skip_destination {
                    self.text.push('\u{ad}');
                }
                self.index += 1;
                return Ok(());
            }
            _ => {}
        }
        let start = self.index;
        while self
            .bytes
            .get(self.index)
            .is_some_and(u8::is_ascii_alphabetic)
        {
            self.index += 1;
        }
        if self.index == start {
            self.index += 1;
            return Ok(());
        }
        let word = std::str::from_utf8(&self.bytes[start..self.index]).unwrap_or_default();
        let negative = self.bytes.get(self.index) == Some(&b'-');
        if negative {
            self.index += 1;
        }
        let number_start = self.index;
        while self.bytes.get(self.index).is_some_and(u8::is_ascii_digit) {
            self.index += 1;
        }
        let number = std::str::from_utf8(&self.bytes[number_start..self.index])
            .ok()
            .and_then(|value| value.parse::<i32>().ok())
            .map(|value| if negative { -value } else { value });
        if self.bytes.get(self.index) == Some(&b' ') {
            self.index += 1;
        }
        self.apply_control(word, number);
        Ok(())
    }

    fn apply_control(&mut self, word: &str, number: Option<i32>) {
        if is_destination(word) {
            self.state.skip_destination = true;
            return;
        }
        if self.state.skip_destination {
            return;
        }
        match word {
            "par" => self.finish_paragraph(),
            "line" => self.text.push('\n'),
            "page" => self.text.push('\u{c}'),
            "tab" => self.text.push('\t'),
            "u" => {
                if let Some(value) = number {
                    let scalar = if value < 0 {
                        (value + 65_536) as u32
                    } else {
                        value as u32
                    };
                    if let Some(character) = char::from_u32(scalar) {
                        self.text.push(character);
                    }
                    self.skip_fallback();
                }
            }
            "uc" => self.state.unicode_skip = number.unwrap_or(1).max(0) as usize,
            "ansicpg" => {
                if let Some(value) = number {
                    self.state.codepage = codepage(value as u16);
                }
            }
            "ansi" => self.state.codepage = WINDOWS_1252,
            "b" => self.change_style(|style| style.bold = number != Some(0)),
            "i" => self.change_style(|style| style.italic = number != Some(0)),
            "fs" => {
                if let Some(value) = number.filter(|value| *value > 0) {
                    self.change_style(|style| style.size_px = value as f32 / 2.0 * 96.0 / 72.0);
                }
            }
            "f" => {
                if let Some(family) = number
                    .and_then(|value| u32::try_from(value).ok())
                    .and_then(|id| self.fonts.get(&id).cloned())
                {
                    self.change_style(|style| style.family = family);
                }
            }
            "cf" => {
                if let Some(colour) = number
                    .and_then(|value| usize::try_from(value).ok())
                    .and_then(|index| self.colours.get(index).copied())
                {
                    self.change_style(|style| style.colour = Some(colour));
                }
            }
            "plain" => self.change_style(|style| *style = default_text_style()),
            "pard" => self.state.paragraph = ParagraphStyle::default(),
            "ql" => self.state.paragraph.alignment = Alignment::Left,
            "qc" => self.state.paragraph.alignment = Alignment::Centre,
            "qr" => self.state.paragraph.alignment = Alignment::Right,
            "qj" => self.state.paragraph.alignment = Alignment::Justify,
            "li" => self.state.paragraph.left = twip(number),
            "ri" => self.state.paragraph.right = twip(number),
            "fi" => self.state.paragraph.first_line = twip(number),
            "sb" => self.state.paragraph.before = twip(number),
            "sa" => self.state.paragraph.after = twip(number),
            "sl" => {
                if let Some(value) = number {
                    self.state.paragraph.line_height = Some(twip(Some(value.abs())));
                    self.state.paragraph.line_height_at_least = value > 0;
                }
            }
            "keepn" => self.state.paragraph.keep_next = number != Some(0),
            "keep" => self.state.paragraph.keep_lines = number != Some(0),
            "widowctrl" => self.state.paragraph.widow_control = number != Some(0),
            "pagebb" => self.state.paragraph.page_break_before = number != Some(0),
            "paperw" => self.size.width = twip(number),
            "paperh" => self.size.height = twip(number),
            "margl" => self.margins.0 = twip(number),
            "margt" => self.margins.1 = twip(number),
            "margr" => self.margins.2 = twip(number),
            "margb" => self.margins.3 = twip(number),
            _ => {}
        }
    }

    fn change_style(&mut self, change: impl FnOnce(&mut TextStyle)) {
        self.flush_run();
        change(&mut self.state.style);
    }

    fn flush_run(&mut self) {
        if self.text.is_empty() || self.state.skip_destination {
            self.text.clear();
            return;
        }
        self.runs.push(FlowRun {
            text: std::mem::take(&mut self.text),
            style: self.state.style.clone(),
        });
    }

    fn finish_paragraph(&mut self) {
        self.flush_run();
        if self.runs.is_empty() {
            self.runs.push(FlowRun {
                text: String::new(),
                style: self.state.style.clone(),
            });
        }
        self.paragraphs.push(FlowParagraph {
            runs: std::mem::take(&mut self.runs),
            style: self.state.paragraph.clone(),
        });
    }

    fn skip_fallback(&mut self) {
        for _ in 0..self.state.unicode_skip {
            if self.index >= self.bytes.len() {
                break;
            }
            if self.bytes[self.index] == b'\\' && self.bytes.get(self.index + 1) == Some(&b'\'') {
                self.index = self.index.saturating_add(4).min(self.bytes.len());
            } else {
                self.index += 1;
            }
        }
    }
}

fn is_destination(word: &str) -> bool {
    matches!(
        word,
        "fonttbl"
            | "colortbl"
            | "stylesheet"
            | "info"
            | "pict"
            | "object"
            | "filetbl"
            | "listtable"
            | "listoverridetable"
            | "datastore"
            | "themedata"
            | "xmlnstbl"
    )
}

fn codepage(value: u16) -> &'static Encoding {
    let label = format!("windows-{value}");
    Encoding::for_label(label.as_bytes()).unwrap_or(WINDOWS_1252)
}

fn twip(value: Option<i32>) -> f32 {
    value.unwrap_or_default() as f32 / 1440.0 * 96.0
}

fn parse_font_table(bytes: &[u8]) -> BTreeMap<u32, String> {
    let source = String::from_utf8_lossy(bytes);
    let mut fonts = BTreeMap::new();
    for group in source.split('{') {
        let Some(position) = group.find("\\f") else {
            continue;
        };
        let rest = &group[position + 2..];
        let digits = rest
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>();
        let Some(id) = digits.parse::<u32>().ok() else {
            continue;
        };
        let Some(semicolon) = rest.find(';') else {
            continue;
        };
        let raw_name = rest[digits.len()..semicolon]
            .split(' ')
            .filter(|part| !part.starts_with('\\'))
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_owned();
        if !raw_name.is_empty() {
            fonts.insert(id, raw_name);
        }
    }
    fonts
}

fn parse_colour_table(bytes: &[u8]) -> Vec<Colour> {
    let source = String::from_utf8_lossy(bytes);
    let Some(start) = source.find("\\colortbl") else {
        return vec![Colour::BLACK];
    };
    let end = source[start..]
        .find('}')
        .map(|value| start + value)
        .unwrap_or(source.len());
    let mut colours = vec![Colour::BLACK];
    for entry in source[start..end].split(';').skip(1) {
        let component = |name: &str| {
            entry.find(name).and_then(|position| {
                entry[position + name.len()..]
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse::<u8>()
                    .ok()
            })
        };
        if let (Some(r), Some(g), Some(b)) = (
            component("\\red"),
            component("\\green"),
            component("\\blue"),
        ) {
            colours.push(Colour { r, g, b, a: 255 });
        }
    }
    colours
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_fallback_and_cp1252_hex_are_decoded_once() {
        let parsed = Parser::new(br#"{\rtf1\ansi\ansicpg1252 Price \u8364? and caf\'e9\par}"#)
            .and_then(Parser::parse)
            .unwrap_or_else(|error| panic!("the inline RTF parses: {error}"));
        let text = parsed.paragraphs[0]
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>();
        assert!(text.contains("Price € and café"));
    }

    #[test]
    fn group_scoped_bold_does_not_leak() {
        let parsed = Parser::new(br#"{\rtf1 normal {\b bold} normal}"#)
            .and_then(Parser::parse)
            .unwrap_or_else(|error| panic!("the inline RTF parses: {error}"));
        assert!(parsed.paragraphs[0].runs.iter().any(|run| run.style.bold));
        assert!(
            !parsed.paragraphs[0]
                .runs
                .last()
                .is_some_and(|run| run.style.bold)
        );
    }
}
