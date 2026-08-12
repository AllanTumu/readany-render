#![forbid(unsafe_code)]

use image::{DynamicImage, GenericImage, GenericImageView, Rgba, RgbaImage};
use quick_xml::Reader;
use quick_xml::events::Event;
use readany_render::{
    GlyphRun, Item, Options, Page, Rect, SourceRef, rasterise, rasterise_rect, render,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::Command;

const CORPUS: &[&str] = &[
    "basic.docx",
    "basic.odt",
    "basic.rtf",
    "basic.pptx",
    "basic.odp",
];

const SHEET_CORPUS: &[(&str, &str)] = &[
    ("endo-prem-2023.xlsx", "corpus/real/endo-prem-2023.xlsx"),
    (
        "oakprism-stress-v3.xlsx",
        "corpus/real/oakprism-stress-v3.xlsx",
    ),
];

const SHEET_VIEWPORTS: &[Rect] = &[
    Rect {
        x: 0.0,
        y: 0.0,
        width: 1_200.0,
        height: 800.0,
    },
    Rect {
        x: 1_200.0,
        y: 800.0,
        width: 1_200.0,
        height: 800.0,
    },
];

const IMAGE_CORPUS: &[(&str, &str)] = &[("receipt.jpg", "corpus/real/receipt.jpg")];

const IMAGE_VIEWPORTS: &[Rect] = &[
    Rect {
        x: 0.0,
        y: 0.0,
        width: 1_200.0,
        height: 800.0,
    },
    Rect {
        x: 0.0,
        y: 800.0,
        width: 1_200.0,
        height: 800.0,
    },
];

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Score {
    pixel: PixelScore,
    text: Option<TextScore>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PixelScore {
    aligned_ssim: f64,
    offset_x: f64,
    offset_y: f64,
    ink_density_ours: f64,
    ink_density_reference: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TextScore {
    ours: u64,
    reference: u64,
    matched: u64,
    exact_text: f64,
    geometry: f64,
    combined: f64,
    offset_x: f64,
    offset_y: f64,
    mean_error: f64,
    p95_error: f64,
}

#[derive(Clone, Debug, Deserialize)]
struct TextBox {
    text: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    source_key: Option<String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("fidelity harness: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let update = std::env::args().any(|argument| argument == "--update");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("missing repository root")?
        .to_owned();
    let report = root.join("harness/report");
    std::fs::create_dir_all(&report)?;
    let work = std::env::temp_dir().join(format!("readany-render-harness-{}", std::process::id()));
    if work.exists() {
        std::fs::remove_dir_all(&work)?;
    }
    std::fs::create_dir_all(&work)?;
    let mut scores = BTreeMap::new();
    let mut contact_rows = Vec::new();
    for (name, relative_path) in SHEET_CORPUS {
        let source = root.join(relative_path);
        let bytes = std::fs::read(&source)?;
        let rendered = render(
            &bytes,
            &Options {
                filename: Some(name),
                ..Options::default()
            },
        )?;
        let page = rendered
            .pages
            .first()
            .ok_or("spreadsheet has no visible sheet")?;
        let document_work = work.join(name.replace('.', "-"));
        std::fs::create_dir_all(&document_work)?;
        reference_html(&source, &document_work)?;
        let html = document_work.join(format!(
            "{}.html",
            source
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or("invalid spreadsheet corpus name")?
        ));
        let mut measurements = Vec::new();
        for dpi in [96_u32, 192] {
            let scale = dpi as f32 / 96.0;
            for (viewport_index, viewport) in SHEET_VIEWPORTS.iter().enumerate() {
                let ours = rasterise_rect(page, *viewport, scale)?;
                let ours_path = report.join(format!(
                    "{}-{}-viewport-{}-ours.png",
                    name.replace('.', "-"),
                    dpi,
                    viewport_index + 1
                ));
                let ours_png = ours.encode_png()?;
                std::fs::write(&ours_path, &ours_png)?;
                let reference_path = document_work.join(format!(
                    "reference-{dpi}-viewport-{}.png",
                    viewport_index + 1
                ));
                let boxes_path = document_work.join(format!(
                    "reference-{dpi}-viewport-{}.json",
                    viewport_index + 1
                ));
                reference_sheet_viewport(
                    &root,
                    &html,
                    &reference_path,
                    &boxes_path,
                    scale,
                    *viewport,
                )?;
                let reference = image::open(&reference_path)?.to_rgba8();
                let ours = image::load_from_memory(&ours_png)?.to_rgba8();
                ensure_comparable(name, &ours, &reference)?;
                let ours_text = display_text_boxes(page, Some(*viewport));
                let reference_text: Vec<TextBox> =
                    serde_json::from_slice(&std::fs::read(&boxes_path)?)?;
                measurements.push(Score {
                    pixel: compare_pixels(&ours, &reference),
                    text: Some(compare_text(name, &ours_text, &reference_text)),
                });
                if dpi == 96 && viewport_index == 0 {
                    contact_rows.push(contact_row(name, &ours, &reference));
                }
            }
        }
        insert_score(&mut scores, name, &measurements)?;
    }
    for (name, relative_path) in IMAGE_CORPUS {
        let source = root.join(relative_path);
        let bytes = std::fs::read(&source)?;
        let rendered = render(
            &bytes,
            &Options {
                filename: Some(name),
                ..Options::default()
            },
        )?;
        let page = rendered.pages.first().ok_or("image has no page")?;
        let original = image::load_from_memory(&bytes)?;
        let mut measurements = Vec::new();
        for dpi in [96_u32, 192] {
            let scale = dpi as f32 / 96.0;
            let scaled = original.resize_exact(
                (original.width() as f32 * scale) as u32,
                (original.height() as f32 * scale) as u32,
                image::imageops::FilterType::Triangle,
            );
            for (viewport_index, viewport) in IMAGE_VIEWPORTS.iter().enumerate() {
                let ours = rasterise_rect(page, *viewport, scale)?;
                let ours_path = report.join(format!(
                    "{}-{}-viewport-{}-ours.png",
                    name.replace('.', "-"),
                    dpi,
                    viewport_index + 1
                ));
                let ours_png = ours.encode_png()?;
                std::fs::write(&ours_path, &ours_png)?;
                let reference = image::imageops::crop_imm(
                    &scaled,
                    (viewport.x * scale) as u32,
                    (viewport.y * scale) as u32,
                    (viewport.width * scale) as u32,
                    (viewport.height * scale) as u32,
                )
                .to_image();
                let ours = image::load_from_memory(&ours_png)?.to_rgba8();
                ensure_comparable(name, &ours, &reference)?;
                measurements.push(Score {
                    pixel: compare_pixels(&ours, &reference),
                    text: None,
                });
                if dpi == 96 && viewport_index == 0 {
                    contact_rows.push(contact_row(name, &ours, &reference));
                }
            }
        }
        insert_score(&mut scores, name, &measurements)?;
    }
    for name in CORPUS {
        let source = root.join("fixtures").join(name);
        let bytes = std::fs::read(&source)?;
        let rendered = render(
            &bytes,
            &Options {
                filename: Some(name),
                ..Options::default()
            },
        )?;
        let document_work = work.join(name.replace('.', "-"));
        std::fs::create_dir_all(&document_work)?;
        reference_pdf(&source, &document_work)?;
        let pdf = document_work.join(format!(
            "{}.pdf",
            source
                .file_stem()
                .and_then(|v| v.to_str())
                .ok_or("invalid fixture name")?
        ));
        let reference_text = reference_pdf_text(&pdf, &rendered.pages, &document_work)?;
        let mut measurements = Vec::new();
        for dpi in [96_u32, 192] {
            let prefix = document_work.join(format!("reference-{dpi}"));
            let status = Command::new("pdftoppm")
                .args(["-png", "-r", &dpi.to_string()])
                .arg(&pdf)
                .arg(&prefix)
                .status()?;
            if !status.success() {
                return Err(format!("pdftoppm failed for {name}").into());
            }
            for (page_index, page) in rendered.pages.iter().enumerate() {
                let ours = rasterise(page, dpi as f32 / 96.0)?;
                let ours_path = report.join(format!("{}-{}-ours.png", name.replace('.', "-"), dpi));
                std::fs::write(&ours_path, ours.encode_png()?)?;
                let reference_path =
                    document_work.join(format!("reference-{dpi}-{}.png", page_index + 1));
                let reference = image::open(&reference_path)?.to_rgba8();
                let ours = image::load_from_memory(&ours.encode_png()?)?.to_rgba8();
                ensure_comparable(name, &ours, &reference)?;
                let (left, right) = equal_canvas(&ours, &reference);
                let ours_text = display_text_boxes(page, None);
                let page_reference_text = reference_text.get(page_index).ok_or_else(|| {
                    format!("pdftotext omitted page {} for {name}", page_index + 1)
                })?;
                measurements.push(Score {
                    pixel: compare_pixels(&left, &right),
                    text: Some(compare_text(name, &ours_text, page_reference_text)),
                });
                if dpi == 96 && page_index == 0 {
                    contact_rows.push(contact_row(name, &left, &right));
                }
            }
        }
        insert_score(&mut scores, name, &measurements)?;
    }
    write_contact_sheet(&report.join("contact-sheet.png"), &contact_rows)?;
    let paged_text_mean = mean_for(&scores, CORPUS.iter().copied(), |score| {
        score.text.as_ref().map(|text| text.combined)
    })?;
    let sheet_text_mean = mean_for(
        &scores,
        SHEET_CORPUS.iter().map(|(name, _)| *name),
        |score| score.text.as_ref().map(|text| text.combined),
    )?;
    let aligned_ssim_mean = scores
        .values()
        .map(|score| score.pixel.aligned_ssim)
        .sum::<f64>()
        / scores.len() as f64;
    println!("diagnostic_aligned_ssim_mean={aligned_ssim_mean:.6}");
    println!("paged_text_fidelity_mean={paged_text_mean:.6}");
    println!("sheet_text_fidelity_mean={sheet_text_mean:.6}");
    for (name, score) in &scores {
        if let Some(text) = &score.text {
            println!(
                "{name}: text={:.6} exact={:.6} geometry={:.6} matched={}/{}/{} mean_error={:.2}px p95_error={:.2}px registration=({:.2},{:.2})px aligned_ssim={:.6} ink={:.4}%/{:.4}%",
                text.combined,
                text.exact_text,
                text.geometry,
                text.matched,
                text.ours,
                text.reference,
                text.mean_error,
                text.p95_error,
                text.offset_x,
                text.offset_y,
                score.pixel.aligned_ssim,
                score.pixel.ink_density_ours * 100.0,
                score.pixel.ink_density_reference * 100.0,
            );
        } else {
            println!(
                "{name}: no-text diagnostic aligned_ssim={:.6} ink={:.4}%/{:.4}%",
                score.pixel.aligned_ssim,
                score.pixel.ink_density_ours * 100.0,
                score.pixel.ink_density_reference * 100.0,
            );
        }
    }
    let scores_path = root.join("harness/baseline.json");
    if update {
        std::fs::write(
            &scores_path,
            format!("{}\n", serde_json::to_string_pretty(&scores)?),
        )?;
    } else {
        let baseline: BTreeMap<String, Score> =
            serde_json::from_slice(&std::fs::read(&scores_path)?)?;
        gate(&scores, &baseline)?;
    }
    std::fs::write(
        report.join("scores.json"),
        format!("{}\n", serde_json::to_string_pretty(&scores)?),
    )?;
    let _ = std::fs::remove_dir_all(work);
    Ok(())
}

fn mean_for<'a>(
    scores: &BTreeMap<String, Score>,
    names: impl Iterator<Item = &'a str>,
    select: impl Fn(&Score) -> Option<f64>,
) -> Result<f64, Box<dyn std::error::Error>> {
    let values = names
        .map(|name| {
            scores
                .get(name)
                .and_then(&select)
                .ok_or_else(|| format!("missing fidelity score for {name}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(values.iter().sum::<f64>() / values.len().max(1) as f64)
}

fn insert_score(
    scores: &mut BTreeMap<String, Score>,
    name: &str,
    measurements: &[Score],
) -> Result<(), Box<dyn std::error::Error>> {
    if measurements.is_empty() {
        return Err(format!("{name} produced no comparable pages").into());
    }
    let count = measurements.len() as f64;
    let text_measurements = measurements
        .iter()
        .filter_map(|measurement| measurement.text.as_ref())
        .collect::<Vec<_>>();
    let text = if text_measurements.is_empty() {
        None
    } else {
        let ours: u64 = text_measurements.iter().map(|value| value.ours).sum();
        let reference: u64 = text_measurements.iter().map(|value| value.reference).sum();
        let matched: u64 = text_measurements.iter().map(|value| value.matched).sum();
        let exact_text = 2.0 * matched as f64 / (ours + reference).max(1) as f64;
        let matched_weight = matched.max(1) as f64;
        let weighted = |select: fn(&TextScore) -> f64| {
            text_measurements
                .iter()
                .map(|value| select(value) * value.matched as f64)
                .sum::<f64>()
                / matched_weight
        };
        let geometry = weighted(|value| value.geometry);
        Some(TextScore {
            ours,
            reference,
            matched,
            exact_text,
            geometry,
            combined: exact_text * geometry,
            offset_x: weighted(|value| value.offset_x),
            offset_y: weighted(|value| value.offset_y),
            mean_error: weighted(|value| value.mean_error),
            p95_error: weighted(|value| value.p95_error),
        })
    };
    scores.insert(
        name.to_owned(),
        Score {
            pixel: PixelScore {
                aligned_ssim: measurements
                    .iter()
                    .map(|value| value.pixel.aligned_ssim)
                    .sum::<f64>()
                    / count,
                offset_x: measurements
                    .iter()
                    .map(|value| value.pixel.offset_x)
                    .sum::<f64>()
                    / count,
                offset_y: measurements
                    .iter()
                    .map(|value| value.pixel.offset_y)
                    .sum::<f64>()
                    / count,
                ink_density_ours: measurements
                    .iter()
                    .map(|value| value.pixel.ink_density_ours)
                    .sum::<f64>()
                    / count,
                ink_density_reference: measurements
                    .iter()
                    .map(|value| value.pixel.ink_density_reference)
                    .sum::<f64>()
                    / count,
            },
            text,
        },
    );
    Ok(())
}

fn reference_pdf(source: &Path, output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let profile = output.join("libreoffice-profile");
    let status = Command::new("soffice")
        .arg(format!(
            "-env:UserInstallation=file://{}",
            profile.display()
        ))
        .args(["--headless", "--convert-to", "pdf", "--outdir"])
        .arg(output)
        .arg(source)
        .status()?;
    if !status.success() {
        return Err(format!("LibreOffice failed for {}", source.display()).into());
    }
    Ok(())
}

fn reference_pdf_text(
    pdf: &Path,
    pages: &[Page],
    output: &Path,
) -> Result<Vec<Vec<TextBox>>, Box<dyn std::error::Error>> {
    let bbox = output.join("reference-bbox.xhtml");
    let status = Command::new("pdftotext")
        .arg("-bbox")
        .arg(pdf)
        .arg(&bbox)
        .status()?;
    if !status.success() {
        return Err(format!("pdftotext -bbox failed for {}", pdf.display()).into());
    }
    parse_pdf_text_boxes(&bbox, pages)
}

fn parse_pdf_text_boxes(
    path: &Path,
    pages: &[Page],
) -> Result<Vec<Vec<TextBox>>, Box<dyn std::error::Error>> {
    let mut reader = Reader::from_reader(BufReader::new(File::open(path)?));
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut result = Vec::new();
    let mut page_size = None;
    let mut page_boxes = Vec::new();
    let mut word = None;
    loop {
        match reader.read_event_into(&mut buffer)? {
            Event::Start(event) if event.name().as_ref() == b"page" => {
                page_size = Some((
                    xml_number(&reader, &event, b"width")?,
                    xml_number(&reader, &event, b"height")?,
                ));
                page_boxes.clear();
            }
            Event::Start(event) if event.name().as_ref() == b"word" => {
                word = Some(TextBox {
                    text: String::new(),
                    x: xml_number(&reader, &event, b"xMin")?,
                    y: xml_number(&reader, &event, b"yMin")?,
                    width: xml_number(&reader, &event, b"xMax")?
                        - xml_number(&reader, &event, b"xMin")?,
                    height: xml_number(&reader, &event, b"yMax")?
                        - xml_number(&reader, &event, b"yMin")?,
                    source: None,
                    source_key: None,
                });
            }
            Event::Text(text) if word.is_some() => {
                if let Some(active) = &mut word {
                    let decoded = text.decode()?;
                    active
                        .text
                        .push_str(&quick_xml::escape::unescape(&decoded)?);
                }
            }
            Event::End(event) if event.name().as_ref() == b"word" => {
                if let Some(text_box) = word.take() {
                    page_boxes.push(text_box);
                }
            }
            Event::End(event) if event.name().as_ref() == b"page" => {
                let page = pages
                    .get(result.len())
                    .ok_or("pdftotext returned more pages than the display list")?;
                let (pdf_width, pdf_height) = page_size.ok_or("PDF bbox page has no size")?;
                let scale_x = f64::from(page.size.width) / pdf_width.max(1.0);
                let scale_y = f64::from(page.size.height) / pdf_height.max(1.0);
                for text_box in &mut page_boxes {
                    text_box.x *= scale_x;
                    text_box.y *= scale_y;
                    text_box.width *= scale_x;
                    text_box.height *= scale_y;
                }
                result.push(std::mem::take(&mut page_boxes));
                page_size = None;
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if result.len() != pages.len() {
        return Err(format!(
            "pdftotext returned {} pages for a {}-page display list",
            result.len(),
            pages.len()
        )
        .into());
    }
    Ok(result)
}

fn xml_number<R: std::io::BufRead>(
    reader: &Reader<R>,
    event: &quick_xml::events::BytesStart<'_>,
    name: &[u8],
) -> Result<f64, Box<dyn std::error::Error>> {
    for attribute in event.attributes().with_checks(false) {
        let attribute = attribute?;
        if attribute.key.as_ref() == name {
            return Ok(attribute
                .decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder())?
                .parse()?);
        }
    }
    Err(format!(
        "PDF bbox element is missing {}",
        String::from_utf8_lossy(name)
    )
    .into())
}

fn reference_html(source: &Path, output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let profile = output.join("libreoffice-profile");
    let status = Command::new("soffice")
        .arg(format!(
            "-env:UserInstallation=file://{}",
            profile.display()
        ))
        .args(["--headless", "--convert-to", "html", "--outdir"])
        .arg(output)
        .arg(source)
        .status()?;
    if !status.success() {
        return Err(format!("LibreOffice HTML export failed for {}", source.display()).into());
    }
    Ok(())
}

fn reference_sheet_viewport(
    root: &Path,
    html: &Path,
    output: &Path,
    boxes_output: &Path,
    scale: f32,
    viewport: Rect,
) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("node")
        .arg(root.join("harness/render-sheet.mjs"))
        .arg(html)
        .arg(output)
        .arg(boxes_output)
        .args([
            scale.to_string(),
            viewport.x.to_string(),
            viewport.y.to_string(),
            viewport.width.to_string(),
            viewport.height.to_string(),
        ])
        .status()?;
    if !status.success() {
        return Err(format!("browser sheet reference failed for {}", html.display()).into());
    }
    Ok(())
}

fn ensure_comparable(
    name: &str,
    ours: &RgbaImage,
    reference: &RgbaImage,
) -> Result<(), Box<dyn std::error::Error>> {
    // Padding images of unrelated sizes made blank page area dominate SSIM.
    // Two percent permits one-pixel raster rounding while refusing a natural
    // sheet canvas compared with a print page.
    const MAX_DIMENSION_DRIFT: f64 = 0.02;
    let width_drift = u32::abs_diff(ours.width(), reference.width()) as f64
        / f64::from(ours.width().max(reference.width()).max(1));
    let height_drift = u32::abs_diff(ours.height(), reference.height()) as f64
        / f64::from(ours.height().max(reference.height()).max(1));
    if width_drift > MAX_DIMENSION_DRIFT || height_drift > MAX_DIMENSION_DRIFT {
        return Err(format!(
            "{name} render dimensions are incomparable: ours={}x{}, reference={}x{}",
            ours.width(),
            ours.height(),
            reference.width(),
            reference.height()
        )
        .into());
    }
    Ok(())
}

fn equal_canvas(left: &RgbaImage, right: &RgbaImage) -> (RgbaImage, RgbaImage) {
    let width = left.width().max(right.width());
    let height = left.height().max(right.height());
    let mut a = RgbaImage::from_pixel(width, height, Rgba([255, 255, 255, 255]));
    let mut b = a.clone();
    let _ = a.copy_from(left, 0, 0);
    let _ = b.copy_from(right, 0, 0);
    (a, b)
}

fn compare_pixels(left: &RgbaImage, right: &RgbaImage) -> PixelScore {
    let mut best = (f64::NEG_INFINITY, 0_i32, 0_i32);
    for offset_y in -3..=3 {
        for offset_x in -3..=3 {
            let mut sum = 0.0;
            let mut windows = 0_u64;
            for top in (0..left.height()).step_by(8) {
                for left_edge in (0..left.width()).step_by(8) {
                    sum += window_ssim(left, right, left_edge, top, offset_x, offset_y);
                    windows += 1;
                }
            }
            let score = sum / windows.max(1) as f64;
            if score > best.0 {
                best = (score, offset_x, offset_y);
            }
        }
    }
    PixelScore {
        aligned_ssim: best.0,
        offset_x: f64::from(best.1),
        offset_y: f64::from(best.2),
        ink_density_ours: ink_density(left),
        ink_density_reference: ink_density(right),
    }
}

fn ink_density(image: &RgbaImage) -> f64 {
    let ink = image.pixels().filter(|pixel| luma(pixel) < 250.0).count();
    ink as f64
        / u64::from(image.width())
            .saturating_mul(u64::from(image.height()))
            .max(1) as f64
}

fn window_ssim(
    left: &RgbaImage,
    right: &RgbaImage,
    x: u32,
    y: u32,
    offset_x: i32,
    offset_y: i32,
) -> f64 {
    let right_edge = (x + 8).min(left.width());
    let bottom = (y + 8).min(left.height());
    let count = f64::from((right_edge - x) * (bottom - y));
    let (mut mean_a, mut mean_b) = (0.0, 0.0);
    for row in y..bottom {
        for column in x..right_edge {
            mean_a += luma(left.get_pixel(column, row));
            mean_b += shifted_luma(right, column, row, offset_x, offset_y);
        }
    }
    mean_a /= count;
    mean_b /= count;
    let (mut var_a, mut var_b, mut covariance) = (0.0, 0.0, 0.0);
    for row in y..bottom {
        for column in x..right_edge {
            let da = luma(left.get_pixel(column, row)) - mean_a;
            let db = shifted_luma(right, column, row, offset_x, offset_y) - mean_b;
            var_a += da * da;
            var_b += db * db;
            covariance += da * db;
        }
    }
    let denominator = (count - 1.0).max(1.0);
    var_a /= denominator;
    var_b /= denominator;
    covariance /= denominator;
    let c1 = (0.01_f64 * 255.0).powi(2);
    let c2 = (0.03_f64 * 255.0).powi(2);
    ((2.0 * mean_a * mean_b + c1) * (2.0 * covariance + c2)
        / ((mean_a * mean_a + mean_b * mean_b + c1) * (var_a + var_b + c2)))
        .clamp(0.0, 1.0)
}

fn shifted_luma(image: &RgbaImage, x: u32, y: u32, offset_x: i32, offset_y: i32) -> f64 {
    let shifted_x = i64::from(x) + i64::from(offset_x);
    let shifted_y = i64::from(y) + i64::from(offset_y);
    if shifted_x < 0
        || shifted_y < 0
        || shifted_x >= i64::from(image.width())
        || shifted_y >= i64::from(image.height())
    {
        255.0
    } else {
        luma(image.get_pixel(shifted_x as u32, shifted_y as u32))
    }
}

fn display_text_boxes(page: &Page, viewport: Option<Rect>) -> Vec<TextBox> {
    let mut boxes = Vec::new();
    collect_text_boxes(&page.items, viewport, &mut boxes);
    boxes
}

fn collect_text_boxes(items: &[Item], viewport: Option<Rect>, boxes: &mut Vec<TextBox>) {
    for item in items {
        match item {
            Item::Glyphs(run) => {
                for mut word in glyph_run_words(run) {
                    if let Some(rect) = viewport {
                        if word.x + word.width <= f64::from(rect.x)
                            || word.x >= f64::from(rect.x + rect.width)
                            || word.y + word.height <= f64::from(rect.y)
                            || word.y >= f64::from(rect.y + rect.height)
                        {
                            continue;
                        }
                        word.x -= f64::from(rect.x);
                        word.y -= f64::from(rect.y);
                    }
                    boxes.push(word);
                }
            }
            Item::Group(group) => collect_text_boxes(&group.items, viewport, boxes),
            Item::Path(_) | Item::Image(_) => {}
            _ => {}
        }
    }
}

fn glyph_run_words(run: &GlyphRun) -> Vec<TextBox> {
    let mut segments = Vec::new();
    let mut start = None;
    for (index, character) in run.text.char_indices() {
        if character.is_whitespace() {
            if let Some(begin) = start.take() {
                segments.push((begin, index));
            }
        } else if start.is_none() {
            start = Some(index);
        }
    }
    if let Some(begin) = start {
        segments.push((begin, run.text.len()));
    }

    let mut positioned = Vec::with_capacity(run.glyphs.len());
    let mut cursor = 0.0_f64;
    for glyph in &run.glyphs {
        positioned.push((
            usize::try_from(glyph.cluster).unwrap_or(usize::MAX),
            cursor + f64::from(glyph.x_offset),
            f64::from(glyph.x_advance.abs()),
        ));
        cursor += f64::from(glyph.x_advance);
    }
    segments
        .into_iter()
        .map(|(begin, end)| {
            let selected = positioned
                .iter()
                .filter(|(cluster, _, _)| *cluster >= begin && *cluster < end)
                .collect::<Vec<_>>();
            let (x, width) = if selected.is_empty() {
                let total = run.text.len().max(1) as f64;
                (
                    cursor * begin as f64 / total,
                    cursor * (end - begin) as f64 / total,
                )
            } else {
                let left = selected
                    .iter()
                    .map(|(_, x, _)| *x)
                    .fold(f64::INFINITY, f64::min);
                let right = selected
                    .iter()
                    .map(|(_, x, width)| *x + *width)
                    .fold(f64::NEG_INFINITY, f64::max);
                (left, (right - left).max(0.0))
            };
            rotated_text_box(run, &run.text[begin..end], x, width)
        })
        .collect()
}

fn rotated_text_box(run: &GlyphRun, text: &str, x: f64, width: f64) -> TextBox {
    let height = f64::from(run.size_px) * 1.2;
    let top = -f64::from(run.size_px);
    let angle = f64::from(run.rotation_deg).to_radians();
    let (sin, cos) = angle.sin_cos();
    let corners = [
        (x, top),
        (x + width, top),
        (x + width, top + height),
        (x, top + height),
    ];
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (corner_x, corner_y) in corners {
        let transformed_x = f64::from(run.origin.x) + corner_x * cos - corner_y * sin;
        let transformed_y = f64::from(run.origin.y) + corner_x * sin + corner_y * cos;
        min_x = min_x.min(transformed_x);
        min_y = min_y.min(transformed_y);
        max_x = max_x.max(transformed_x);
        max_y = max_y.max(transformed_y);
    }
    TextBox {
        text: text.to_owned(),
        x: min_x,
        y: min_y,
        width: max_x - min_x,
        height: max_y - min_y,
        source: run.source.as_ref().map(|source| format!("{source:?}")),
        source_key: run.source.as_ref().and_then(source_key),
    }
}

fn source_key(source: &SourceRef) -> Option<String> {
    match source {
        SourceRef::Cell { row, column, .. } => Some(format!("cell:{row}:{column}")),
        SourceRef::Text { .. } | SourceRef::Shape { .. } => None,
        _ => None,
    }
}

fn compare_text(name: &str, ours: &[TextBox], reference: &[TextBox]) -> TextScore {
    let mut ours_by_text: BTreeMap<String, Vec<&TextBox>> = BTreeMap::new();
    for text_box in ours {
        ours_by_text
            .entry(match_key(text_box))
            .or_default()
            .push(text_box);
    }
    let mut reference_by_text: BTreeMap<String, Vec<&TextBox>> = BTreeMap::new();
    for text_box in reference {
        reference_by_text
            .entry(match_key(text_box))
            .or_default()
            .push(text_box);
    }
    let mut seed_x = Vec::new();
    let mut seed_y = Vec::new();
    for (key, ours_values) in &ours_by_text {
        if ours_values.len() != 1 {
            continue;
        }
        let Some(reference_values) = reference_by_text.get(key) else {
            continue;
        };
        if reference_values.len() != 1 {
            continue;
        }
        seed_x.push(center_x(reference_values[0]) - center_x(ours_values[0]));
        seed_y.push(center_y(reference_values[0]) - center_y(ours_values[0]));
    }
    let seed_offset_x = if seed_x.is_empty() {
        0.0
    } else {
        median(&mut seed_x)
    };
    let seed_offset_y = if seed_y.is_empty() {
        0.0
    } else {
        median(&mut seed_y)
    };
    let mut pairs = Vec::new();
    for (key, ours_values) in &ours_by_text {
        let Some(reference_values) = reference_by_text.get(key) else {
            continue;
        };
        let mut candidates = Vec::new();
        for (ours_index, ours_box) in ours_values.iter().enumerate() {
            for (reference_index, reference_box) in reference_values.iter().enumerate() {
                let dx = center_x(ours_box) + seed_offset_x - center_x(reference_box);
                let dy = center_y(ours_box) + seed_offset_y - center_y(reference_box);
                candidates.push((dx.hypot(dy), ours_index, reference_index));
            }
        }
        candidates.sort_by(|left, right| left.0.total_cmp(&right.0));
        let mut used_ours = vec![false; ours_values.len()];
        let mut used_reference = vec![false; reference_values.len()];
        for (_, ours_index, reference_index) in candidates {
            if !used_ours[ours_index] && !used_reference[reference_index] {
                used_ours[ours_index] = true;
                used_reference[reference_index] = true;
                pairs.push((ours_values[ours_index], reference_values[reference_index]));
            }
        }
    }
    let exact_text = 2.0 * pairs.len() as f64 / (ours.len() + reference.len()).max(1) as f64;
    if pairs.is_empty() {
        return TextScore {
            ours: ours.len() as u64,
            reference: reference.len() as u64,
            matched: 0,
            exact_text,
            geometry: 0.0,
            combined: 0.0,
            offset_x: 0.0,
            offset_y: 0.0,
            mean_error: 1_000_000.0,
            p95_error: 1_000_000.0,
        };
    }
    let mut offsets_x = pairs
        .iter()
        .map(|(ours, reference)| center_x(reference) - center_x(ours))
        .collect::<Vec<_>>();
    let mut offsets_y = pairs
        .iter()
        .map(|(ours, reference)| center_y(reference) - center_y(ours))
        .collect::<Vec<_>>();
    let offset_x = median(&mut offsets_x);
    let offset_y = median(&mut offsets_y);
    let mut errors = pairs
        .iter()
        .map(|(ours, reference)| {
            let dx = center_x(ours) + offset_x - center_x(reference);
            let dy = center_y(ours) + offset_y - center_y(reference);
            let position = dx.hypot(dy);
            let size = ((ours.width - reference.width).abs()
                + (ours.height - reference.height).abs())
                * 0.25;
            (position + size, *ours, *reference)
        })
        .collect::<Vec<_>>();
    let geometry = errors
        .iter()
        .map(|(error, _, _)| (-error / 12.0).exp())
        .sum::<f64>()
        / errors.len() as f64;
    let mean_error = errors.iter().map(|(error, _, _)| error).sum::<f64>() / errors.len() as f64;
    errors.sort_by(|left, right| left.0.total_cmp(&right.0));
    let p95_index = ((errors.len() - 1) as f64 * 0.95).round() as usize;
    let p95_error = errors[p95_index].0;
    for (error, ours, reference) in errors.iter().rev().take(5).filter(|entry| entry.0 > 8.0) {
        eprintln!(
            "{name}: text geometry drift {:.1}px for {:?} {:?} ours=({:.1},{:.1}) reference=({:.1},{:.1})",
            error,
            ours.text,
            ours.source.as_deref().unwrap_or("unknown source"),
            ours.x,
            ours.y,
            reference.x,
            reference.y,
        );
    }
    TextScore {
        ours: ours.len() as u64,
        reference: reference.len() as u64,
        matched: pairs.len() as u64,
        exact_text,
        geometry,
        combined: exact_text * geometry,
        offset_x,
        offset_y,
        mean_error,
        p95_error,
    }
}

fn normalize_word(word: &str) -> String {
    word.trim().to_lowercase()
}

fn match_key(text_box: &TextBox) -> String {
    match &text_box.source_key {
        Some(source) => format!("{source}\u{1f}{}", normalize_word(&text_box.text)),
        None => normalize_word(&text_box.text),
    }
}

fn center_x(text_box: &TextBox) -> f64 {
    text_box.x + text_box.width * 0.5
}

fn center_y(text_box: &TextBox) -> f64 {
    text_box.y + text_box.height * 0.5
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) * 0.5
    } else {
        values[middle]
    }
}
fn luma(pixel: &Rgba<u8>) -> f64 {
    0.2126 * f64::from(pixel[0]) + 0.7152 * f64::from(pixel[1]) + 0.0722 * f64::from(pixel[2])
}
fn contact_row(name: &str, left: &RgbaImage, right: &RgbaImage) -> RgbaImage {
    let thumb_width = 320;
    let left = DynamicImage::ImageRgba8(left.clone())
        .thumbnail(thumb_width, 240)
        .to_rgba8();
    let right = DynamicImage::ImageRgba8(right.clone())
        .thumbnail(thumb_width, 240)
        .to_rgba8();
    let height = left.height().max(right.height());
    let mut row = RgbaImage::from_pixel(thumb_width * 2, height, Rgba([255, 255, 255, 255]));
    let _ = row.copy_from(&left, 0, 0);
    let _ = row.copy_from(&right, thumb_width, 0);
    let _ = name;
    row
}
fn write_contact_sheet(path: &Path, rows: &[RgbaImage]) -> Result<(), image::ImageError> {
    let width = rows.iter().map(GenericImageView::width).max().unwrap_or(1);
    let height: u32 = rows.iter().map(GenericImageView::height).sum();
    let mut sheet = RgbaImage::from_pixel(width, height.max(1), Rgba([255, 255, 255, 255]));
    let mut y = 0;
    for row in rows {
        sheet.copy_from(row, 0, y)?;
        y += row.height();
    }
    sheet.save(path)
}
fn gate(
    current: &BTreeMap<String, Score>,
    baseline: &BTreeMap<String, Score>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Pixels are diagnostic only. Release gating uses text identity and display-list
    // geometry, with a small allowance for reference extractor rounding.
    const REFERENCE_JITTER: f64 = 0.001;
    if current.keys().collect::<Vec<_>>() != baseline.keys().collect::<Vec<_>>() {
        return Err(
            "fidelity corpus differs from the committed baseline; run with --update".into(),
        );
    }
    for (name, score) in current {
        let old = baseline
            .get(name)
            .ok_or_else(|| format!("{name} has no committed baseline; run with --update"))?;
        match (&score.text, &old.text) {
            (Some(score), Some(old)) if score.combined + REFERENCE_JITTER < old.combined => {
                return Err(format!(
                    "{name} text geometry regressed from {:.6} to {:.6}",
                    old.combined, score.combined
                )
                .into());
            }
            (Some(_), Some(_)) | (None, None) => {}
            _ => {
                return Err(
                    format!("{name} text evidence changed shape; run with --update").into(),
                );
            }
        }
    }
    let text_values = current.values().filter_map(|value| value.text.as_ref());
    let old_text_values = baseline.values().filter_map(|value| value.text.as_ref());
    let (mean_sum, mean_count) = text_values.fold((0.0, 0_u64), |(sum, count), value| {
        (sum + value.combined, count + 1)
    });
    let (old_sum, old_count) = old_text_values.fold((0.0, 0_u64), |(sum, count), value| {
        (sum + value.combined, count + 1)
    });
    let mean = mean_sum / mean_count.max(1) as f64;
    let old_mean = old_sum / old_count.max(1) as f64;
    if mean + REFERENCE_JITTER < old_mean {
        return Err(format!("corpus mean regressed from {old_mean:.6} to {mean:.6}").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_natural_sheet_and_print_page_are_refused_before_ssim() {
        let sheet = RgbaImage::new(218, 65);
        let print_page = RgbaImage::new(816, 1_056);
        let error = ensure_comparable("basic.xlsx", &sheet, &print_page)
            .expect_err("padding incomparable canvases would manufacture a high SSIM score");
        assert!(error.to_string().contains("dimensions are incomparable"));
    }

    #[test]
    fn raster_rounding_within_two_percent_remains_comparable() {
        let ours = RgbaImage::new(816, 1_056);
        let reference = RgbaImage::new(817, 1_055);
        assert!(ensure_comparable("page.docx", &ours, &reference).is_ok());
    }

    #[test]
    fn pixel_diagnostic_recovers_a_three_pixel_registration_shift() {
        let mut original = RgbaImage::from_pixel(64, 64, Rgba([255, 255, 255, 255]));
        for y in 12..28 {
            for x in 9..25 {
                original.put_pixel(x, y, Rgba([0, 0, 0, 255]));
            }
        }
        let mut shifted = RgbaImage::from_pixel(64, 64, Rgba([255, 255, 255, 255]));
        for y in 14..30 {
            for x in 12..28 {
                shifted.put_pixel(x, y, Rgba([0, 0, 0, 255]));
            }
        }
        let score = compare_pixels(&original, &shifted);
        assert_eq!((score.offset_x, score.offset_y), (3.0, 2.0));
        assert!(score.aligned_ssim > 0.999);
        assert!(score.ink_density_ours > 0.05);
    }

    #[test]
    fn text_geometry_removes_global_registration_but_exposes_local_drift() {
        let ours = vec![
            TextBox {
                text: "alpha".into(),
                x: 10.0,
                y: 10.0,
                width: 20.0,
                height: 10.0,
                source: Some("cell A1".into()),
                source_key: None,
            },
            TextBox {
                text: "beta".into(),
                x: 40.0,
                y: 10.0,
                width: 20.0,
                height: 10.0,
                source: Some("cell B1".into()),
                source_key: None,
            },
        ];
        let globally_shifted = vec![
            TextBox {
                text: "alpha".into(),
                x: 12.0,
                y: 13.0,
                width: 20.0,
                height: 10.0,
                source: None,
                source_key: None,
            },
            TextBox {
                text: "beta".into(),
                x: 42.0,
                y: 13.0,
                width: 20.0,
                height: 10.0,
                source: None,
                source_key: None,
            },
        ];
        let aligned = compare_text("aligned", &ours, &globally_shifted);
        assert_eq!(aligned.combined, 1.0);
        let mut locally_drifted = globally_shifted;
        locally_drifted[1].x += 40.0;
        let drifted = compare_text("drifted", &ours, &locally_drifted);
        assert!(drifted.combined < aligned.combined);
        assert!(drifted.p95_error > 10.0);
    }

    #[test]
    fn repeated_sheet_text_is_matched_by_cell_provenance() {
        let ours = vec![TextBox {
            text: "No".into(),
            x: 10.0,
            y: 10.0,
            width: 12.0,
            height: 10.0,
            source: Some("Cell A1".into()),
            source_key: Some("cell:0:0".into()),
        }];
        let reference = vec![TextBox {
            text: "No".into(),
            x: 10.0,
            y: 10.0,
            width: 12.0,
            height: 10.0,
            source: None,
            source_key: Some("cell:1:0".into()),
        }];
        let score = compare_text("sheet", &ours, &reference);
        assert_eq!(score.matched, 0);
        assert_eq!(score.exact_text, 0.0);
        assert!(serde_json::to_string(&score).is_ok());
    }
}
