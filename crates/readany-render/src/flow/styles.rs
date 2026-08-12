use crate::RenderError;
use crate::container::xml;
use crate::flow::{Alignment, ParagraphStyle, default_text_style};
use crate::model::Colour;
use crate::text::TextStyle;
use quick_xml::events::{BytesStart, Event};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default)]
pub(crate) struct RunPatch {
    family: Option<String>,
    size_px: Option<f32>,
    colour: Option<Colour>,
    bold: Option<bool>,
    italic: Option<bool>,
}

impl RunPatch {
    pub(crate) fn apply_to(&self, style: &mut TextStyle) {
        if let Some(family) = &self.family {
            style.family.clone_from(family);
        }
        if let Some(size_px) = self.size_px {
            style.size_px = size_px;
        }
        if let Some(colour) = self.colour {
            style.colour = Some(colour);
        }
        if let Some(bold) = self.bold {
            style.bold = bold;
        }
        if let Some(italic) = self.italic {
            style.italic = italic;
        }
    }

    fn overlay(&mut self, patch: &Self) {
        if patch.family.is_some() {
            self.family.clone_from(&patch.family);
        }
        self.size_px = patch.size_px.or(self.size_px);
        self.colour = patch.colour.or(self.colour);
        self.bold = patch.bold.or(self.bold);
        self.italic = patch.italic.or(self.italic);
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ParagraphPatch {
    alignment: Option<Alignment>,
    left: Option<f32>,
    right: Option<f32>,
    first_line: Option<f32>,
    before: Option<f32>,
    after: Option<f32>,
    line_height: Option<f32>,
    line_height_at_least: Option<bool>,
    keep_next: Option<bool>,
    keep_lines: Option<bool>,
    widow_control: Option<bool>,
    page_break_before: Option<bool>,
    tabs: Option<Vec<f32>>,
}

impl ParagraphPatch {
    pub(crate) fn apply_to(&self, style: &mut ParagraphStyle) {
        style.alignment = self.alignment.unwrap_or(style.alignment);
        style.left = self.left.unwrap_or(style.left);
        style.right = self.right.unwrap_or(style.right);
        style.first_line = self.first_line.unwrap_or(style.first_line);
        style.before = self.before.unwrap_or(style.before);
        style.after = self.after.unwrap_or(style.after);
        style.line_height = self.line_height.or(style.line_height);
        style.line_height_at_least = self
            .line_height_at_least
            .unwrap_or(style.line_height_at_least);
        style.keep_next = self.keep_next.unwrap_or(style.keep_next);
        style.keep_lines = self.keep_lines.unwrap_or(style.keep_lines);
        style.widow_control = self.widow_control.unwrap_or(style.widow_control);
        style.page_break_before = self.page_break_before.unwrap_or(style.page_break_before);
        if let Some(tabs) = &self.tabs {
            style.tabs.clone_from(tabs);
        }
    }

    fn overlay(&mut self, patch: &Self) {
        self.alignment = patch.alignment.or(self.alignment);
        self.left = patch.left.or(self.left);
        self.right = patch.right.or(self.right);
        self.first_line = patch.first_line.or(self.first_line);
        self.before = patch.before.or(self.before);
        self.after = patch.after.or(self.after);
        self.line_height = patch.line_height.or(self.line_height);
        self.line_height_at_least = patch.line_height_at_least.or(self.line_height_at_least);
        self.keep_next = patch.keep_next.or(self.keep_next);
        self.keep_lines = patch.keep_lines.or(self.keep_lines);
        self.widow_control = patch.widow_control.or(self.widow_control);
        self.page_break_before = patch.page_break_before.or(self.page_break_before);
        if patch.tabs.is_some() {
            self.tabs.clone_from(&patch.tabs);
        }
    }
}

#[derive(Clone, Debug, Default)]
struct StyleDef {
    based_on: Option<String>,
    run: RunPatch,
    paragraph: ParagraphPatch,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedStyle {
    pub text: TextStyle,
    pub paragraph: ParagraphStyle,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct StyleSheet {
    default_run: RunPatch,
    default_paragraph: ParagraphPatch,
    styles: BTreeMap<String, StyleDef>,
}

impl StyleSheet {
    pub(crate) fn parse(bytes: Option<&[u8]>) -> Result<Self, RenderError> {
        let Some(bytes) = bytes else {
            return Ok(Self::default());
        };
        let mut reader = quick_xml::Reader::from_reader(bytes);
        let mut sheet = Self::default();
        let mut current_id = None::<String>;
        let mut current = StyleDef::default();
        let mut in_defaults = false;
        let mut in_run = false;
        let mut in_paragraph = false;
        loop {
            match reader.read_event() {
                Ok(Event::Start(start)) => {
                    let name = xml::local_name(start.name().as_ref()).to_vec();
                    match name.as_slice() {
                        b"docDefaults" => in_defaults = true,
                        b"style" => {
                            current_id = attr(&start, b"styleId");
                            current = StyleDef::default();
                        }
                        b"rPr" => in_run = true,
                        b"pPr" => in_paragraph = true,
                        b"basedOn" if current_id.is_some() => {
                            current.based_on = attr(&start, b"val")
                        }
                        _ => apply_property(
                            &start,
                            in_run,
                            in_paragraph,
                            in_defaults,
                            &mut sheet,
                            &mut current,
                        ),
                    }
                }
                Ok(Event::Empty(start)) => {
                    if xml::local_name(start.name().as_ref()) == b"basedOn" && current_id.is_some()
                    {
                        current.based_on = attr(&start, b"val");
                    } else {
                        apply_property(
                            &start,
                            in_run,
                            in_paragraph,
                            in_defaults,
                            &mut sheet,
                            &mut current,
                        );
                    }
                }
                Ok(Event::End(end)) => match xml::local_name(end.name().as_ref()) {
                    b"docDefaults" => in_defaults = false,
                    b"rPr" => in_run = false,
                    b"pPr" => in_paragraph = false,
                    b"style" => {
                        if let Some(id) = current_id.take() {
                            sheet.styles.insert(id, std::mem::take(&mut current));
                        }
                    }
                    _ => {}
                },
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(_) => {
                    return Err(RenderError::malformed(
                        "word/styles.xml is malformed; obtain a fresh copy",
                    ));
                }
            }
        }
        Ok(sheet)
    }

    pub(crate) fn resolve(&self, id: Option<&str>) -> ResolvedStyle {
        let mut run = self.default_run.clone();
        let mut paragraph = self.default_paragraph.clone();
        if let Some(id) = id {
            let mut chain = Vec::new();
            let mut cursor = Some(id);
            let mut visited = BTreeSet::new();
            while let Some(style_id) = cursor {
                if !visited.insert(style_id.to_owned()) {
                    break;
                }
                let Some(style) = self.styles.get(style_id) else {
                    break;
                };
                chain.push(style);
                cursor = style.based_on.as_deref();
            }
            for style in chain.into_iter().rev() {
                run.overlay(&style.run);
                paragraph.overlay(&style.paragraph);
            }
        }
        let mut text = default_text_style();
        run.apply_to(&mut text);
        let mut paragraph_style = ParagraphStyle::default();
        paragraph.apply_to(&mut paragraph_style);
        ResolvedStyle {
            text,
            paragraph: paragraph_style,
        }
    }

    pub(crate) fn apply_character_style(&self, id: &str, style: &mut TextStyle) {
        let resolved = self.resolve(Some(id));
        *style = resolved.text;
    }
}

fn apply_property(
    start: &BytesStart<'_>,
    in_run: bool,
    in_paragraph: bool,
    in_defaults: bool,
    sheet: &mut StyleSheet,
    current: &mut StyleDef,
) {
    if in_run {
        let target = if in_defaults {
            &mut sheet.default_run
        } else {
            &mut current.run
        };
        apply_run_property(start, target);
    }
    if in_paragraph {
        let target = if in_defaults {
            &mut sheet.default_paragraph
        } else {
            &mut current.paragraph
        };
        apply_paragraph_property(start, target);
    }
}

pub(crate) fn apply_run_property(start: &BytesStart<'_>, patch: &mut RunPatch) {
    match xml::local_name(start.name().as_ref()) {
        b"rFonts" => {
            patch.family = attr(start, b"ascii")
                .or_else(|| attr(start, b"hAnsi"))
                .or_else(|| attr(start, b"cs"));
        }
        b"sz" | b"szCs" => {
            patch.size_px = attr(start, b"val")
                .and_then(|value| value.parse::<f32>().ok())
                .map(|half_points| half_points / 2.0 * 96.0 / 72.0);
        }
        b"color" => patch.colour = attr(start, b"val").and_then(|value| colour(&value)),
        b"b" | b"bCs" => patch.bold = Some(toggle(start)),
        b"i" | b"iCs" => patch.italic = Some(toggle(start)),
        _ => {}
    }
}

pub(crate) fn apply_paragraph_property(start: &BytesStart<'_>, patch: &mut ParagraphPatch) {
    match xml::local_name(start.name().as_ref()) {
        b"jc" => {
            patch.alignment = attr(start, b"val").map(|value| match value.as_str() {
                "center" => Alignment::Centre,
                "right" | "end" => Alignment::Right,
                "both" | "distribute" => Alignment::Justify,
                _ => Alignment::Left,
            });
        }
        b"ind" => {
            patch.left = attr(start, b"left")
                .or_else(|| attr(start, b"start"))
                .and_then(|value| value.parse::<f32>().ok())
                .map(twip);
            patch.right = attr(start, b"right")
                .or_else(|| attr(start, b"end"))
                .and_then(|value| value.parse::<f32>().ok())
                .map(twip);
            patch.first_line = attr(start, b"firstLine")
                .and_then(|value| value.parse::<f32>().ok())
                .map(twip)
                .or_else(|| {
                    attr(start, b"hanging")
                        .and_then(|value| value.parse::<f32>().ok())
                        .map(|value| -twip(value))
                });
        }
        b"spacing" => {
            patch.before = attr(start, b"before")
                .and_then(|value| value.parse::<f32>().ok())
                .map(twip);
            patch.after = attr(start, b"after")
                .and_then(|value| value.parse::<f32>().ok())
                .map(twip);
            if let Some(line) = attr(start, b"line").and_then(|value| value.parse::<f32>().ok()) {
                let rule = attr(start, b"lineRule").unwrap_or_else(|| "auto".into());
                if rule == "auto" {
                    patch.line_height = Some(14.666_667 * 1.2 * line / 240.0);
                    patch.line_height_at_least = Some(false);
                } else {
                    patch.line_height = Some(twip(line));
                    patch.line_height_at_least = Some(rule == "atLeast");
                }
            }
        }
        b"keepNext" => patch.keep_next = Some(toggle(start)),
        b"keepLines" => patch.keep_lines = Some(toggle(start)),
        b"widowControl" => patch.widow_control = Some(toggle(start)),
        b"pageBreakBefore" => patch.page_break_before = Some(toggle(start)),
        b"tab" if attr(start, b"val").as_deref() != Some("clear") => {
            if let Some(position) = attr(start, b"pos")
                .and_then(|value| value.parse::<f32>().ok())
                .map(twip)
            {
                patch.tabs.get_or_insert_with(Vec::new).push(position);
            }
        }
        _ => {}
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

fn toggle(start: &BytesStart<'_>) -> bool {
    !matches!(
        attr(start, b"val").as_deref(),
        Some("0" | "false" | "off" | "none")
    )
}

fn twip(value: f32) -> f32 {
    value / 1440.0 * 96.0
}

fn colour(value: &str) -> Option<Colour> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_word_styles_resolve_defaults_and_based_on_chains() {
        let xml = br#"<w:styles xmlns:w="w"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri"/><w:sz w:val="20"/></w:rPr></w:rPrDefault></w:docDefaults><w:style w:type="paragraph" w:styleId="Base"><w:pPr><w:jc w:val="center"/></w:pPr><w:rPr><w:b/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Child"><w:basedOn w:val="Base"/><w:rPr><w:i/></w:rPr></w:style></w:styles>"#;
        let sheet = StyleSheet::parse(Some(xml))
            .unwrap_or_else(|error| panic!("the inline style sheet is valid: {error}"));
        let child = sheet.resolve(Some("Child"));
        assert_eq!(child.paragraph.alignment, Alignment::Centre);
        assert!(child.text.bold);
        assert!(child.text.italic);
        assert_eq!(child.text.size_px, 13.333_333);
    }

    #[test]
    fn a_cycle_in_based_on_is_bounded_and_deterministic() {
        let xml = br#"<w:styles xmlns:w="w"><w:style w:styleId="A"><w:basedOn w:val="B"/><w:rPr><w:b/></w:rPr></w:style><w:style w:styleId="B"><w:basedOn w:val="A"/><w:rPr><w:i/></w:rPr></w:style></w:styles>"#;
        let sheet = StyleSheet::parse(Some(xml))
            .unwrap_or_else(|error| panic!("the cyclic style sheet is parseable: {error}"));
        let style = sheet.resolve(Some("A"));
        assert!(style.text.bold);
        assert!(style.text.italic);
    }
}
