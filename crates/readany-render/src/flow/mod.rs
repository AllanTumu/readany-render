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
    pub tabs: Vec<f32>,
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

pub(crate) fn layout_flow(
    paragraphs: &[FlowParagraph],
    format: Format,
    options: &Options<'_>,
    page_size: Size,
    margins: (f32, f32, f32, f32),
) -> Result<Rendered, RenderError> {
    let mut pages = vec![new_page(page_size, 1)];
    let content_bottom = page_size.height - margins.3;
    let mut y = margins.1;
    let mut pending_page_break = false;
    for (paragraph_index, paragraph) in paragraphs.iter().enumerate() {
        let text = paragraph.text();
        let content_width = (page_size.width
            - margins.0
            - margins.2
            - paragraph.style.left
            - paragraph.style.right)
            .max(1.0);
        let lines = wrap_rich(&text, paragraph, content_width);
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
        let page_is_empty = pages.len() == 1 && y <= margins.1;
        if (!page_is_empty && (pending_page_break || paragraph.style.page_break_before))
            || ((paragraph.style.keep_lines || paragraph.style.keep_next)
                && y > margins.1
                && y + paragraph_height > content_bottom
                && paragraph_height <= content_bottom - margins.1)
        {
            push_page(&mut pages, page_size, options)?;
            y = margins.1;
        }
        // Paragraph space-before is suppressed at the top of a page.  Applying
        // it where there is no preceding block made the NIST document's final
        // 649.6 px spacer overflow onto a second otherwise blank page.
        if y > margins.1 {
            y += paragraph.style.before;
        }
        let fitting_lines = line_metrics
            .iter()
            .scan(y, |cursor, height| {
                *cursor += *height;
                Some(*cursor <= content_bottom)
            })
            .take_while(|fits| *fits)
            .count();
        if paragraph.style.widow_control && lines.len() > 1 && fitting_lines == 1 && y > margins.1 {
            push_page(&mut pages, page_size, options)?;
            y = margins.1;
        }

        let mut previous_end = 0_usize;
        for (line_index, range) in lines.into_iter().enumerate() {
            if line_index > 0
                && text
                    .get(previous_end..range.start)
                    .is_some_and(|between| between.contains('\u{c}'))
            {
                push_page(&mut pages, page_size, options)?;
                y = margins.1;
            }
            let height = line_metrics.get(line_index).copied().unwrap_or(17.6);
            if y + height > content_bottom && y > margins.1 {
                push_page(&mut pages, page_size, options)?;
                y = margins.1;
            }
            let first_indent = if line_index == 0 {
                paragraph.style.first_line
            } else {
                0.0
            };
            let line_width = measure_rich(paragraph, range.clone());
            let available = (content_width - first_indent).max(1.0);
            let offset = match paragraph.style.alignment {
                Alignment::Left | Alignment::Justify => 0.0,
                Alignment::Centre => ((available - line_width) / 2.0).max(0.0),
                Alignment::Right => (available - line_width).max(0.0),
            };
            let baseline = y + baseline_offset(paragraph, range.clone(), height);
            let mut x = margins.0 + paragraph.style.left + first_indent + offset;
            let is_last_line = line_index + 1 == line_metrics.len();
            let spaces = text[range.clone()]
                .chars()
                .filter(|character| *character == ' ')
                .count();
            let justify_extra =
                if paragraph.style.alignment == Alignment::Justify && !is_last_line && spaces > 0 {
                    ((available - line_width) / spaces as f32).max(0.0)
                } else {
                    0.0
                };
            let current = pages
                .last_mut()
                .ok_or_else(|| RenderError::malformed("page layout could not be created"))?;
            let range_end = range.end;
            paint_line(
                current,
                paragraph,
                &text,
                range,
                paragraph_index as u32,
                &mut x,
                baseline,
                justify_extra,
                margins.0 + paragraph.style.left,
            );
            y += height;
            previous_end = range_end;
        }
        y += paragraph.style.after;
        pending_page_break = text.ends_with('\u{c}');
    }
    Ok(Rendered {
        pages,
        format,
        unrendered: Vec::new(),
        meta: Meta::default(),
    })
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

fn line_height(paragraph: &FlowParagraph, range: std::ops::Range<usize>) -> f32 {
    let natural = styled_segments(paragraph, range)
        .map(|(_, _, style)| style.size_px.max(1.0) * paragraph.style.line_height_multiplier)
        .fold(0.0_f32, f32::max)
        .max(14.666_667 * paragraph.style.line_height_multiplier);
    match paragraph.style.line_height {
        Some(value) if paragraph.style.line_height_at_least => natural.max(value),
        Some(value) => value.max(1.0),
        None => natural,
    }
}

fn baseline_offset(paragraph: &FlowParagraph, range: std::ops::Range<usize>, height: f32) -> f32 {
    let font_size = styled_segments(paragraph, range)
        .map(|(_, _, style)| style.size_px)
        .fold(0.0_f32, f32::max)
        .max(14.666_667);
    ((height - font_size) / 2.0).max(0.0) + font_size * 0.82
}

#[allow(clippy::too_many_arguments)]
fn paint_line(
    page: &mut Page,
    paragraph: &FlowParagraph,
    full_text: &str,
    range: std::ops::Range<usize>,
    paragraph_index: u32,
    x: &mut f32,
    baseline: f32,
    justify_extra: f32,
    tab_origin: f32,
) {
    for (start, end, style) in styled_segments(paragraph, range.clone()) {
        let source = SourceRef::Text {
            paragraph: paragraph_index,
            start: u32::try_from(start).unwrap_or(u32::MAX),
            end: u32::try_from(end).unwrap_or(u32::MAX),
        };
        let source_text = full_text.get(start..end).unwrap_or_default();
        for (part_index, part) in source_text.split('\t').enumerate() {
            if part_index > 0 {
                *x = next_tab(*x, tab_origin, &paragraph.style.tabs);
            }
            if part.is_empty() {
                continue;
            }
            let visual = bidi_visual(part);
            let mut run = shape(
                &visual,
                style,
                Point { x: *x, y: baseline },
                Some(source.clone()),
            );
            if justify_extra > 0.0 {
                for (character, glyph) in visual.chars().zip(&mut run.glyphs) {
                    if character == ' ' {
                        glyph.x_advance += justify_extra;
                    }
                }
            }
            *x += run.glyphs.iter().map(|glyph| glyph.x_advance).sum::<f32>();
            page.items.push(Item::Glyphs(run));
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

fn next_tab(x: f32, origin: f32, custom: &[f32]) -> f32 {
    custom
        .iter()
        .map(|stop| origin + *stop)
        .find(|stop| *stop > x)
        .unwrap_or_else(|| origin + (((x - origin) / 48.0).floor() + 1.0) * 48.0)
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

fn measure_rich(paragraph: &FlowParagraph, range: std::ops::Range<usize>) -> f32 {
    let text = paragraph.text();
    styled_segments(paragraph, range)
        .map(|(start, end, style)| {
            text.get(start..end)
                .map(|value| measure(value, style))
                .unwrap_or(0.0)
        })
        .sum()
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
        if measure_rich(paragraph, line_start..break_at) > width && last_break > line_start {
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
        while measure_rich(paragraph, cursor..text.len()) > width {
            let mut end = cursor;
            for (offset, character) in text[cursor..].char_indices() {
                let candidate = cursor + offset + character.len_utf8();
                if candidate > cursor && measure_rich(paragraph, cursor..candidate) > width {
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
