use readany_render::{
    Document, Format, Item, Limits, Options, Rect, RenderErrorCode, SvgOptions, Unrendered,
    items_in_rect, rasterise, rasterise_rect, render, to_svg,
};
use std::path::PathBuf;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name),
    )
    .unwrap_or_else(|error| panic!("fixture {name} is missing: {error}"))
}

fn real_corpus(name: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/real")
            .join(name),
    )
    .unwrap_or_else(|error| panic!("real corpus file {name} is missing: {error}"))
}

/// A workbook from the **private** corpus, which is not in this repository.
///
/// `None` when `READANY_RENDER_CORPUS` is unset, so a public checkout runs
/// every other test rather than failing on data it cannot have. The tests that
/// need one say so by returning early — and say it out loud on stderr, because
/// a test that silently becomes a no-op is a test that stops being one.
fn private_corpus(name: &str) -> Option<Vec<u8>> {
    let dir = std::env::var("READANY_RENDER_CORPUS").ok()?;
    match std::fs::read(PathBuf::from(dir).join(name)) {
        Ok(bytes) => Some(bytes),
        Err(error) => panic!("READANY_RENDER_CORPUS is set but {name} is missing: {error}"),
    }
}

#[test]
fn a_real_statement_is_explicitly_delegated_instead_of_looking_rendered() {
    let rendered = render(
        &real_corpus("cfpb-sample-credit-card-statement.pdf"),
        &Options {
            filename: Some("cfpb-sample-credit-card-statement.pdf"),
            ..Options::default()
        },
    )
    .expect("the official sample statement is recognized");
    assert!(rendered.pages.is_empty());
    assert_eq!(
        rendered.unrendered,
        vec![Unrendered::DelegateToHost {
            format: Format::Pdf
        }]
    );
}

#[test]
fn xlsx_declared_default_row_height_controls_sheet_geometry() {
    let Some(bytes) = private_corpus("sheet-b.xlsx") else {
        eprintln!(
            "skipped: READANY_RENDER_CORPUS is unset, so the private stress \
             workbook is unavailable"
        );
        return;
    };
    let rendered = render(
        &bytes,
        &Options {
            filename: Some("sheet-b.xlsx"),
            ..Options::default()
        },
    )
    .expect("the real stress workbook renders");
    let page = rendered.pages.first().expect("the workbook has a sheet");
    assert_eq!(page.size.height, 42_020.0);
}

#[test]
fn sheet_headers_are_opt_in_and_frozen_panes_expose_pixel_extents() {
    let bytes = fixture("basic.xlsx");
    let plain = render(
        &bytes,
        &Options {
            filename: Some("basic.xlsx"),
            ..Options::default()
        },
    )
    .expect("the workbook renders without optional furniture");
    let labelled = render(
        &bytes,
        &Options {
            filename: Some("basic.xlsx"),
            sheet_headers: true,
            ..Options::default()
        },
    )
    .expect("the workbook renders with optional furniture");
    assert_eq!(
        plain.pages[0].size.width + 48.0,
        labelled.pages[0].size.width
    );
    assert_eq!(
        plain.pages[0].size.height + 24.0,
        labelled.pages[0].size.height
    );
    let header_labels = labelled.pages[0]
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Glyphs(run) => Some(run.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(header_labels, ["A", "B", "C", "1", "2", "3"]);
    let frozen = labelled.pages[0]
        .frozen
        .expect("the fixture freezes its first row and column");
    assert_eq!((frozen.rows, frozen.columns), (1, 1));
    assert_eq!((frozen.width, frozen.height), (137.0, 44.0));

    let ods = render(
        &fixture("basic.ods"),
        &Options {
            filename: Some("basic.ods"),
            sheet_headers: true,
            ..Options::default()
        },
    )
    .expect("ODS exposes the same optional headers and frozen extents");
    let ods_frozen = ods.pages[0]
        .frozen
        .expect("the ODS fixture freezes its first row and column");
    assert_eq!((ods_frozen.rows, ods_frozen.columns), (1, 1));
    assert_eq!((ods_frozen.width, ods_frozen.height), (128.0, 46.0));
}

#[test]
fn every_supported_family_reaches_a_display_list_or_deliberate_delegate() {
    for (name, expected) in [
        ("basic.csv", Format::Csv),
        ("basic.tsv", Format::Tsv),
        ("basic.xlsx", Format::Xlsx),
        ("features.xlsm", Format::Xlsm),
        ("basic.ods", Format::Ods),
        ("basic.docx", Format::Docx),
        ("basic.odt", Format::Odt),
        ("basic.rtf", Format::Rtf),
        ("basic.pptx", Format::Pptx),
        ("basic.odp", Format::Odp),
        ("pixel.png", Format::Png),
    ] {
        let bytes = fixture(name);
        let rendered = render(
            &bytes,
            &Options {
                filename: Some(name),
                ..Options::default()
            },
        )
        .unwrap_or_else(|error| panic!("{name} failed: {error}"));
        assert_eq!(rendered.format, expected, "{name}");
        assert!(!rendered.pages.is_empty(), "{name}");
        assert!(
            rendered.pages.iter().any(|page| !page.items.is_empty()),
            "{name}"
        );
    }
    for (name, expected) in [
        ("delegate.pdf", Format::Pdf),
        ("delegate.heic", Format::Heic),
    ] {
        let bytes = fixture(name);
        let rendered = render(
            &bytes,
            &Options {
                filename: Some(name),
                ..Options::default()
            },
        )
        .unwrap_or_else(|error| panic!("{name} failed: {error}"));
        assert!(rendered.pages.is_empty());
        assert_eq!(
            rendered.unrendered,
            vec![Unrendered::DelegateToHost { format: expected }]
        );
    }
}

#[test]
fn xlsx_features_are_reported_instead_of_silently_dropped() {
    let bytes = fixture("features.xlsm");
    let rendered = render(
        &bytes,
        &Options {
            filename: Some("features.xlsm"),
            ..Options::default()
        },
    )
    .unwrap_or_else(|error| panic!("fixture failed: {error}"));
    assert!(
        rendered
            .unrendered
            .iter()
            .any(|value| matches!(value, Unrendered::Chart { .. }))
    );
    assert!(
        rendered
            .unrendered
            .iter()
            .any(|value| matches!(value, Unrendered::PivotTable { .. }))
    );
    assert!(
        rendered
            .unrendered
            .iter()
            .any(|value| matches!(value, Unrendered::ConditionalFormatting { .. }))
    );
    assert!(
        rendered
            .unrendered
            .iter()
            .any(|value| matches!(value, Unrendered::HiddenSheet { .. }))
    );
    assert!(
        rendered
            .unrendered
            .iter()
            .any(|value| matches!(value, Unrendered::FormulaWithoutCachedValue { .. }))
    );
    assert!(
        rendered
            .unrendered
            .iter()
            .any(|value| matches!(value, Unrendered::ExternalReference { .. }))
    );
    assert!(
        rendered
            .unrendered
            .iter()
            .any(|value| matches!(value, Unrendered::Macro))
    );
    assert!(
        rendered
            .unrendered
            .iter()
            .any(|value| matches!(value, Unrendered::UnsupportedGlyphs { .. }))
    );
}

#[test]
fn strict_mode_refuses_a_partial_result_with_the_stable_incomplete_code() {
    let error = render(
        &fixture("features.xlsm"),
        &Options {
            filename: Some("features.xlsm"),
            strict: true,
            ..Options::default()
        },
    )
    .expect_err("strict mode must reject every reported omission");
    assert_eq!(error.code, RenderErrorCode::StrictIncomplete);
    assert!(!error.unrendered.is_empty());
}

#[test]
fn every_unrendered_variant_has_a_generated_fixture() {
    let feature_bytes = include_bytes!("../../../fixtures/features.xlsm");
    let feature = render(
        feature_bytes,
        &Options {
            filename: Some("features.xlsm"),
            ..Options::default()
        },
    )
    .expect("the generated feature workbook is valid");
    assert!(
        feature
            .unrendered
            .iter()
            .any(|entry| matches!(entry, Unrendered::Chart { .. }))
    );
    assert!(
        feature
            .unrendered
            .iter()
            .any(|entry| matches!(entry, Unrendered::PivotTable { .. }))
    );
    assert!(
        feature
            .unrendered
            .iter()
            .any(|entry| matches!(entry, Unrendered::ConditionalFormatting { .. }))
    );
    assert!(
        feature
            .unrendered
            .iter()
            .any(|entry| matches!(entry, Unrendered::HiddenSheet { .. }))
    );
    assert!(
        feature
            .unrendered
            .iter()
            .any(|entry| matches!(entry, Unrendered::FormulaWithoutCachedValue { .. }))
    );
    assert!(
        feature
            .unrendered
            .iter()
            .any(|entry| matches!(entry, Unrendered::ExternalReference { .. }))
    );
    assert!(
        feature
            .unrendered
            .iter()
            .any(|entry| matches!(entry, Unrendered::UnsupportedGlyphs { .. }))
    );
    assert!(
        feature
            .unrendered
            .iter()
            .any(|entry| matches!(entry, Unrendered::Macro))
    );

    let unsupported_media = render(
        include_bytes!("../../../fixtures/unsupported-media.pptx"),
        &Options {
            filename: Some("unsupported-media.pptx"),
            ..Options::default()
        },
    )
    .expect("unsupported slide media returns a named partial result");
    assert_eq!(
        unsupported_media.unrendered,
        vec![Unrendered::UnsupportedMedia {
            page: 0,
            kind: "svg".into(),
            count: 1,
        }]
    );

    let docx = render(
        include_bytes!("../../../fixtures/basic.docx"),
        &Options {
            filename: Some("basic.docx"),
            ..Options::default()
        },
    )
    .expect("the generated DOCX is valid");
    assert!(
        docx.unrendered
            .iter()
            .any(|entry| matches!(entry, Unrendered::Ole { .. }))
    );

    for (bytes, filename) in [
        (
            &include_bytes!("../../../fixtures/delegate.pdf")[..],
            "delegate.pdf",
        ),
        (
            &include_bytes!("../../../fixtures/delegate.heic")[..],
            "delegate.heic",
        ),
    ] {
        let delegated = render(
            bytes,
            &Options {
                filename: Some(filename),
                ..Options::default()
            },
        )
        .expect("the deliberate delegate fixture is valid");
        assert!(
            delegated
                .unrendered
                .iter()
                .any(|entry| matches!(entry, Unrendered::DelegateToHost { .. }))
        );
    }

    let limits = Limits {
        glyphs_per_page: 1,
        ..Limits::default()
    };
    let truncated = render(
        include_bytes!("../../../fixtures/basic.csv"),
        &Options {
            filename: Some("basic.csv"),
            limits,
            ..Options::default()
        },
    )
    .expect("truncation returns a partial, explicitly marked display list");
    assert!(
        truncated
            .unrendered
            .iter()
            .any(|entry| matches!(entry, Unrendered::Truncated { .. }))
    );
}

#[test]
fn flow_embeds_are_reported_instead_of_silently_dropped() {
    let rendered = render(&fixture("basic.docx"), &Options::default())
        .unwrap_or_else(|error| panic!("fixture failed: {error}"));
    assert!(
        rendered
            .unrendered
            .iter()
            .any(|value| matches!(value, Unrendered::Ole { .. }))
    );
    assert!(
        rendered
            .unrendered
            .iter()
            .any(|value| matches!(value, Unrendered::ExternalReference { .. }))
    );
}

#[test]
fn docx_style_lists_tables_images_and_repeating_parts_share_one_display_list() {
    let rendered = render(
        &fixture("flow-features.docx"),
        &Options {
            filename: Some("flow-features.docx"),
            ..Options::default()
        },
    )
    .expect("the generated flow feature document is valid");
    assert_eq!(
        rendered.pages.len(),
        2,
        "the explicit page break is retained"
    );

    let glyphs = rendered
        .pages
        .iter()
        .flat_map(|page| &page.items)
        .filter_map(|item| match item {
            Item::Glyphs(run) => Some(run),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(glyphs.iter().any(|run| run.text == "1."));
    assert!(
        glyphs
            .iter()
            .find(|run| run.text == "Styled heading")
            .is_some_and(|run| run.font.0 == 21),
        "the based-on bold style selects the committed bold Carlito face"
    );
    assert!(
        glyphs
            .iter()
            .find(|run| run.text == "Numbered italic item")
            .is_some_and(|run| run.font.0 == 11),
        "direct italic formatting selects the committed italic Carlito face"
    );
    assert_eq!(
        glyphs
            .iter()
            .filter(|run| run.text == "Repeated header")
            .count(),
        2
    );
    assert_eq!(
        glyphs
            .iter()
            .filter(|run| run.text == "Repeated footer")
            .count(),
        2
    );
    assert!(rendered.pages.iter().flat_map(|page| &page.items).any(|item| {
        matches!(item, Item::Image(image) if matches!(image.source, Some(readany_render::SourceRef::Text { .. })))
    }));
    let rules = rendered
        .pages
        .iter()
        .flat_map(|page| &page.items)
        .filter_map(|item| match item {
            Item::Path(path) => Some(path),
            _ => None,
        })
        .count();
    assert!(
        rules >= 8,
        "the declared w:tblBorders draw a rule on every edge of every cell, \
         and this table has three cells; got {rules}"
    );

    // The grid is 2160 + 2880 twips inside a 1440 twip margin: column one runs
    // 96 to 240 px and column two 240 to 432 px, each inset by the declared
    // 108 twip cell padding.
    let left = glyphs
        .iter()
        .find(|run| run.text == "Left cell")
        .expect("the first column's text");
    assert!(
        (left.origin.x - 103.2).abs() < 0.5,
        "the first column starts at its own grid position, not the text margin: {}",
        left.origin.x
    );
    let right = glyphs
        .iter()
        .find(|run| run.text == "Right cell")
        .expect("the second column's text");
    let right_edge = right.origin.x + right.glyphs.iter().map(|g| g.x_advance).sum::<f32>();
    assert!(
        (right_edge - 424.8).abs() < 0.5,
        "a right-aligned cell ends at its own column's right edge, which a \
         tab-separated row cannot express: {right_edge}"
    );
    assert!(
        right.origin.x > left.origin.x + 130.0,
        "the columns are laid out side by side rather than run together"
    );
}

/// A `w:gridSpan` cell covers the columns it claims, so its rules stand at the
/// table's outer edges rather than at the first column's.
///
/// **Falsified** by ignoring `w:gridSpan` when resolving a cell's right-hand
/// grid column: the spanning header's right-hand rule moves from 432 px to
/// 240 px and the assertion below fails.
#[test]
fn a_spanning_header_cell_is_as_wide_as_the_columns_it_covers() {
    let rendered = render(
        &fixture("flow-features.docx"),
        &Options {
            filename: Some("flow-features.docx"),
            ..Options::default()
        },
    )
    .expect("the generated flow feature document is valid");
    let header = rendered
        .pages
        .iter()
        .flat_map(|page| &page.items)
        .filter_map(|item| match item {
            Item::Glyphs(run) if run.text == "Spanning header" => Some(run),
            _ => None,
        })
        .next()
        .expect("the spanning header's text");
    let vertical_rules = rendered
        .pages
        .iter()
        .flat_map(|page| &page.items)
        .filter_map(|item| match item {
            Item::Path(path) => path.path.commands.first().and_then(|command| {
                match (command, path.path.commands.get(1)) {
                    (
                        readany_render::PathCommand::Move(from),
                        Some(readany_render::PathCommand::Line(to)),
                    ) if (from.x - to.x).abs() < 0.01
                        && (from.y - to.y).abs() > 0.01
                        && from.y < header.origin.y =>
                    {
                        Some(from.x)
                    }
                    _ => None,
                }
            }),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        vertical_rules.iter().any(|x| (*x - 432.0).abs() < 0.5),
        "the spanning cell's right-hand rule stands at the table's right edge: {vertical_rules:?}"
    );
    assert!(
        !vertical_rules.iter().any(|x| (*x - 240.0).abs() < 0.5),
        "no rule divides the spanned columns inside the merged header: {vertical_rules:?}"
    );
}

#[test]
fn pptx_inherits_placeholder_geometry_and_embeds_relationship_images() {
    let bytes = fixture("slide-features.pptx");
    let rendered = render(
        &bytes,
        &Options {
            filename: Some("slide-features.pptx"),
            ..Options::default()
        },
    )
    .expect("the generated slide feature document is valid");
    let title = rendered.pages[0].items.iter().find_map(|item| {
        let Item::Group(group) = item else {
            return None;
        };
        matches!(
            group.source,
            Some(readany_render::SourceRef::Shape { shape: 0, .. })
        )
        .then_some(group)
    });
    let title = title.expect("the inherited title placeholder is present");
    let clip = title
        .clip
        .expect("the title placeholder has inherited geometry");
    assert_eq!(
        (clip.x, clip.y, clip.width, clip.height),
        (96.0, 48.0, 960.0, 96.0)
    );
    assert!(rendered.pages[0].items.iter().any(|item| {
        matches!(item, Item::Group(group) if group.items.iter().any(|child| matches!(child, Item::Image(_))))
    }));
    assert!(rendered.pages[0].items.iter().any(|item| {
        matches!(item, Item::Group(group) if group.items.iter().any(|child| matches!(child, Item::Path(path) if path.stroke.is_some())))
    }));

    let error = render(
        &bytes,
        &Options {
            filename: Some("slide-features.pptx"),
            limits: Limits {
                image_pixels: 0,
                ..Limits::default()
            },
            ..Options::default()
        },
    )
    .expect_err("embedded slide images obey the same pixel ceiling as image documents");
    assert_eq!(error.code, RenderErrorCode::LimitExceeded);
}

fn slide_run(text: &str) -> readany_render::GlyphRun {
    let rendered = render(
        &fixture("slide-features.pptx"),
        &Options {
            filename: Some("slide-features.pptx"),
            ..Options::default()
        },
    )
    .expect("the generated slide feature deck is valid");
    fn find(items: &[Item], text: &str) -> Option<readany_render::GlyphRun> {
        for item in items {
            match item {
                Item::Glyphs(run) if run.text.starts_with(text) => return Some(run.clone()),
                Item::Group(group) => {
                    if let Some(run) = find(&group.items, text) {
                        return Some(run);
                    }
                }
                Item::Glyphs(_) | Item::Path(_) | Item::Image(_) => {}
                // `Item` is `#[non_exhaustive]` outside the crate.
                _ => {}
            }
        }
        None
    }
    find(&rendered.pages[0].items, text).unwrap_or_else(|| panic!("no run beginning {text:?}"))
}

/// A shape inside a `p:grpSp` is placed through its group's transform, not at
/// the raw offset it declares.
///
/// The fixture's group sits at 192 px and writes its children in a space twice
/// its own size, so a child at 192 px lands at 192 + 192 / 2 = 288 px, plus the
/// 9.6 px default text inset.
///
/// **Falsified** by returning `rect` unchanged from `GroupTransform::map`: the
/// label stays at its raw 201.6 px and the assertion below fails.
#[test]
fn a_grouped_shape_is_placed_through_the_transform_its_group_declares() {
    let run = slide_run("Grouped");
    assert!(
        (run.origin.x - 297.6).abs() < 0.5,
        "the grouped label is mapped out of its group's child space: {}",
        run.origin.x
    );
    // 144 px mapped top, plus the 4.8 px inset, plus the 16 px baseline drop.
    assert!(
        (run.origin.y - 164.8).abs() < 0.5,
        "and mapped on both axes: {}",
        run.origin.y
    );
}

/// A run that declares no size of its own takes its paragraph's `a:defRPr`,
/// which in turn overrides the shape body's `a:lstStyle`.
///
/// The fixture sets 10 pt on the body and 36 pt on the paragraph, and the run
/// asks for neither, so 36 pt — 48 px — is the answer.
///
/// **Falsified** by reading only `a:rPr`: the run falls back to the generic
/// 18 pt presentation default and is drawn at 24 px, half size.
#[test]
fn a_run_without_a_size_inherits_its_paragraph_default_over_the_body_default() {
    let run = slide_run("Inherited size");
    assert!(
        (run.size_px - 48.0).abs() < 0.01,
        "the paragraph's 36 pt wins over the body's 10 pt: {}",
        run.size_px
    );
}

/// `a:xfrm rot` is carried into the display list so a turned text box is not
/// drawn flat.
///
/// **Falsified** by dropping the `rot` attribute read: `rotation_deg` is 0 and
/// the origin sits at the unturned position.
#[test]
fn a_turned_shape_carries_its_quarter_turn_into_the_display_list() {
    let run = slide_run("Turned label");
    assert!(
        (run.rotation_deg - 90.0).abs() < 0.01,
        "5,400,000 sixty-thousandths of a degree is a quarter turn: {}",
        run.rotation_deg
    );
    // The box spans 576..768 px across and 96..144 px down, so its centre is
    // (672, 120); turning the top-left text origin a quarter turn about that
    // centre puts it right of centre and above it.
    assert!(
        run.origin.x > 672.0 && run.origin.y < 120.0,
        "the origin turns about the shape's centre: ({}, {})",
        run.origin.x,
        run.origin.y
    );
}

#[test]
fn every_text_item_carries_provenance() {
    for name in [
        "basic.csv",
        "basic.xlsx",
        "basic.ods",
        "basic.docx",
        "basic.odt",
        "basic.rtf",
        "basic.pptx",
        "basic.odp",
    ] {
        let bytes = fixture(name);
        let rendered = render(
            &bytes,
            &Options {
                filename: Some(name),
                ..Options::default()
            },
        )
        .unwrap_or_else(|error| panic!("{name} failed: {error}"));
        for item in rendered.pages.iter().flat_map(|page| &page.items) {
            if let Item::Glyphs(run) = item {
                assert!(run.source.is_some(), "{name}");
            }
        }
    }
}

#[test]
fn raster_and_svg_backends_draw_non_blank_output() {
    let rendered = render(&fixture("basic.csv"), &Options::default())
        .unwrap_or_else(|error| panic!("fixture failed: {error}"));
    let pixmap =
        rasterise(&rendered.pages[0], 1.0).unwrap_or_else(|error| panic!("raster failed: {error}"));
    assert!(
        pixmap
            .data
            .chunks_exact(4)
            .any(|pixel| pixel != [255, 255, 255, 255])
    );
    assert!(pixmap.encode_png().is_ok());
    let svg = to_svg(&rendered.pages[0], &SvgOptions::default())
        .unwrap_or_else(|error| panic!("svg failed: {error}"));
    assert!(svg.contains("<text"));
}

#[test]
fn a_wide_sheet_rasterises_by_viewport_without_allocating_the_whole_canvas() {
    let rendered = render(
        &fixture("wide.xlsx"),
        &Options {
            filename: Some("wide.xlsx"),
            ..Options::default()
        },
    )
    .expect("the generated wide workbook is valid");
    let page = &rendered.pages[0];
    assert!(page.size.width * page.size.height > 100_000_000.0);
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 1_200.0,
        height: 800.0,
    };
    let region = items_in_rect(page, rect).expect("the viewport is valid");
    assert!(!region.is_empty());
    assert!(region.len() < page.items.len() / 10);
    assert!(region.iter().all(item_has_source));
    let pixmap = rasterise_rect(page, rect, 1.0).expect("the viewport raster stays below the cap");
    assert_eq!((pixmap.width, pixmap.height), (1_200, 800));
    assert!(
        pixmap
            .data
            .chunks_exact(4)
            .any(|pixel| pixel != [255, 255, 255, 255])
    );
    let error = rasterise(page, 1.0).expect_err("the full natural sheet exceeds the pixel cap");
    assert_eq!(error.code, RenderErrorCode::Rasterisation);
}

#[test]
fn a_viewport_matches_the_same_crop_of_a_full_page_within_raster_rounding() {
    let rendered = render(
        &fixture("basic.xlsx"),
        &Options {
            filename: Some("basic.xlsx"),
            ..Options::default()
        },
    )
    .expect("the generated workbook is valid");
    let page = &rendered.pages[0];
    let full = rasterise(page, 1.0).expect("the small sheet rasterises in full");
    let rect = Rect {
        x: 50.0,
        y: 10.0,
        width: 120.0,
        height: 45.0,
    };
    let viewport = rasterise_rect(page, rect, 1.0).expect("the crop rasterises");
    for y in 0..viewport.height {
        let full_start = (((y + 10) * full.width + 50) * 4) as usize;
        let full_end = full_start + viewport.width as usize * 4;
        let viewport_start = (y * viewport.width * 4) as usize;
        let viewport_end = viewport_start + viewport.width as usize * 4;
        let full_row = &full.data[full_start..full_end];
        let viewport_row = &viewport.data[viewport_start..viewport_end];
        let differing = full_row
            .iter()
            .zip(viewport_row)
            .filter(|(left, right)| u8::abs_diff(**left, **right) > 16)
            .count();
        assert!(
            differing <= (viewport_row.len() / 1_000).max(4),
            "viewport transform changed {differing} channels beyond antialias rounding"
        );
    }
}

fn item_has_source(item: &Item) -> bool {
    match item {
        Item::Glyphs(run) => run.source.is_some(),
        Item::Path(path) => path.source.is_some(),
        Item::Image(image) => image.source.is_some(),
        Item::Group(group) => group.source.is_some() && group.items.iter().all(item_has_source),
        _ => false,
    }
}

#[test]
fn hostile_entities_repeats_and_input_sizes_stop_before_expansion() {
    let entity =
        render(&fixture("entity.xlsx"), &Options::default()).expect_err("entities must fail");
    assert_eq!(entity.code, RenderErrorCode::MalformedDocument);
    let repeated =
        render(&fixture("repeat-bomb.ods"), &Options::default()).expect_err("repeat must fail");
    assert_eq!(repeated.code, RenderErrorCode::LimitExceeded);
    let limited = render(
        b"a,b\n1,2",
        &Options {
            limits: Limits {
                input_bytes: 2,
                ..Limits::default()
            },
            ..Options::default()
        },
    )
    .expect_err("input limit must fail");
    assert_eq!(limited.code, RenderErrorCode::LimitExceeded);
    for name in [
        "zip-bomb.xlsx",
        "many-entries.xlsx",
        "deep-xml.xlsx",
        "gridspan-bomb.docx",
        "huge.bmp",
    ] {
        let error = render(
            &fixture(name),
            &Options {
                filename: Some(name),
                ..Options::default()
            },
        )
        .expect_err("the generated hostile fixture must cross its named limit");
        assert_eq!(error.code, RenderErrorCode::LimitExceeded, "{name}");
    }
    let slip = render(
        &fixture("zip-slip.xlsx"),
        &Options {
            filename: Some("zip-slip.xlsx"),
            ..Options::default()
        },
    )
    .expect_err("parent paths must be rejected even though nothing is written");
    assert_eq!(slip.code, RenderErrorCode::MalformedDocument);
    let entities_enabled = render(
        b"a,b",
        &Options {
            limits: Limits {
                xml_entity_expansions: 1,
                ..Limits::default()
            },
            ..Options::default()
        },
    )
    .expect_err("entity expansion cannot be enabled");
    assert_eq!(entities_enabled.code, RenderErrorCode::InvalidOptions);
    let pages = render(
        &fixture("hundred-pages.docx"),
        &Options {
            filename: Some("hundred-pages.docx"),
            limits: Limits {
                pages: 10,
                ..Limits::default()
            },
            ..Options::default()
        },
    )
    .expect_err("explicit page breaks must cross the page ceiling");
    assert_eq!(pages.code, RenderErrorCode::LimitExceeded);
    let truncated = render(
        b"one,two\nthree,four",
        &Options {
            limits: Limits {
                glyphs_per_page: 2,
                ..Limits::default()
            },
            ..Options::default()
        },
    )
    .unwrap_or_else(|error| panic!("truncation fixture failed: {error}"));
    assert!(truncated.unrendered.iter().any(
        |value| matches!(value, Unrendered::Truncated { limit, .. } if limit == "glyphs_per_page")
    ));
}

#[test]
fn identical_bytes_produce_byte_identical_canonical_json() {
    let bytes = fixture("basic.xlsx");
    let first = serde_json::to_vec(
        &render(&bytes, &Options::default())
            .unwrap_or_else(|error| panic!("fixture failed: {error}")),
    )
    .ok();
    let second = serde_json::to_vec(
        &render(&bytes, &Options::default())
            .unwrap_or_else(|error| panic!("fixture failed: {error}")),
    )
    .ok();
    assert_eq!(first, second);
}

#[test]
fn basic_xlsx_display_list_matches_the_committed_readable_golden() {
    let bytes = include_bytes!("../../../fixtures/basic.xlsx");
    let rendered = render(
        bytes,
        &Options {
            filename: Some("fixtures/basic.xlsx"),
            ..Options::default()
        },
    )
    .expect("the generated workbook is valid");
    let actual = serde_json::to_string_pretty(&rendered).expect("the model is serializable") + "\n";
    assert_eq!(
        actual,
        include_str!("goldens/basic-xlsx.json"),
        "display-list changes must be reviewed as a golden diff"
    );
}

#[test]
fn a_retained_document_without_overrides_is_the_exact_committed_golden() {
    let bytes = include_bytes!("../../../fixtures/basic.xlsx");
    let rendered = render(
        bytes,
        &Options {
            filename: Some("fixtures/basic.xlsx"),
            ..Options::default()
        },
    )
    .unwrap_or_else(|error| panic!("the generated workbook renders: {error}"));
    let document = Document::new(rendered);
    let actual = serde_json::to_string_pretty(document.rendered())
        .map(|json| format!("{json}\n"))
        .unwrap_or_else(|error| panic!("the retained display list serialises: {error}"));
    assert_eq!(actual, include_str!("goldens/basic-xlsx.json"));
}

/// **A break before nothing is not a break.**
///
/// The UK IPO agreement carries `fo:break-before="page"` on its very first
/// paragraph. Honoured literally it opens the document with a blank page, and
/// the whole document then sits one page later than LibreOffice puts it — which
/// took its page-aligned fidelity score to **0.148** while the text was
/// character-identical to the reference. Word and LibreOffice both suppress a
/// break before the first block; so does this.
///
/// **Falsified** by dropping the `page_is_empty` guard in `flow::layout`: the
/// count becomes three and this goes red.
#[test]
fn a_page_break_before_the_first_block_does_not_open_a_blank_page() {
    let rendered = render(
        &real_corpus("uk-ipo-one-way-nda.odt"),
        &Options {
            filename: Some("uk-ipo-one-way-nda.odt"),
            ..Options::default()
        },
    )
    .expect("the agreement renders");
    assert_eq!(
        rendered.pages.len(),
        2,
        "LibreOffice paginates this agreement as two pages"
    );
    let first = rendered.pages.first().expect("a first page");
    assert!(
        !first.items.is_empty(),
        "the first page must carry content, not be the blank one the break asked for"
    );
}

/// A tab with nothing after it does not widen the line it ends.
///
/// The NIST running header is one paragraph — text, a right tab at the right
/// margin, the book and year, and then a second `w:tab/` with no run after it.
/// Counting that trailing tab as width pushed the measured advance to the next
/// default stop 48 px past the margin, so the header wrapped and `2026` fell to
/// a second line on all 17 even pages.
///
/// **Falsified** by returning `cursor` instead of `painted` from
/// `advance_rich`: the header occupies two baselines and this fails.
#[test]
fn a_trailing_tab_does_not_wrap_the_running_header() {
    let rendered = render(
        &real_corpus("nist-hb133-2026-chapter-2.docx"),
        &Options {
            filename: Some("nist-hb133-2026-chapter-2.docx"),
            ..Options::default()
        },
    )
    .expect("the NIST chapter renders");
    let page = rendered.pages.get(3).expect("an even page");
    let header = page
        .items
        .iter()
        .filter_map(|item| match item {
            // The header sits above the 96 px text margin.
            Item::Glyphs(run) if run.origin.y < 80.0 => Some(run),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(!header.is_empty(), "the even page carries a running header");
    let baselines = header
        .iter()
        .map(|run| (run.origin.y * 10.0).round() as i64)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        baselines.len(),
        1,
        "the running header is one line, not two: {baselines:?}"
    );
    // The right tab is at 9,360 twips, which is 624 px past the 96 px margin.
    let right = header
        .iter()
        .map(|run| run.origin.x + run.glyphs.iter().map(|g| g.x_advance).sum::<f32>())
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        (right - 720.0).abs() < 1.0,
        "and it ends on the right tab stop: {right}"
    );
}

/// An omitted OOXML spacing value means zero, not the flow renderer's generic
/// visual gap.  The NIST contents has 33 compact entries; adding 6.6 px after
/// every one consumed about 218 px and moved the last four entries to a second
/// page even though LibreOffice keeps the complete contents on page one.
///
/// **Falsified** by constructing the DOCX paragraph style from the unmodified
/// `ParagraphStyle::default()`: paragraph 32 moves back to page two.
#[test]
fn unspecified_docx_paragraph_spacing_does_not_split_the_contents_page() {
    let rendered = render(
        &real_corpus("nist-hb133-2026-chapter-2.docx"),
        &Options {
            filename: Some("nist-hb133-2026-chapter-2.docx"),
            ..Options::default()
        },
    )
    .expect("the NIST chapter renders");
    let first = rendered.pages.first().expect("the contents page exists");
    assert!(
        first.items.iter().any(|item| {
            matches!(
                item,
                Item::Glyphs(run)
                    if matches!(
                        run.source,
                        Some(readany_render::SourceRef::Text { paragraph: 32, .. })
                    )
            )
        }),
        "the final contents entry remains on LibreOffice's first page"
    );
}

/// An even/odd header pair describes alternatives, not two layers to paint on
/// every page.  The NIST chapter has one of each; painting both duplicated 105
/// characters per page and put header words hundreds of pixels from their
/// reference positions.
///
/// **Falsified** by returning to an archive-wide header scan: the title occurs
/// twice above the first page's body instead of once.
#[test]
fn docx_even_and_odd_headers_are_alternatives() {
    let rendered = render(
        &real_corpus("nist-hb133-2026-chapter-2.docx"),
        &Options {
            filename: Some("nist-hb133-2026-chapter-2.docx"),
            ..Options::default()
        },
    )
    .expect("the NIST chapter renders");
    let first = rendered.pages.first().expect("the contents page exists");
    let titles = first
        .items
        .iter()
        .filter(|item| {
            matches!(
                item,
                Item::Glyphs(run)
                    if run.origin.y < 80.0
                        && run.text.starts_with("Chapter 2.  Test Procedures")
            )
        })
        .count();
    assert_eq!(titles, 1, "only the odd-page header is painted on page one");
}

/// Word suppresses paragraph space-before at a page boundary and advances an
/// ordinary 11 pt line by 12.65 pt in the measured NIST reference.  Applying
/// the generic 13.2 pt line box consumed one body page, while applying a 649.6
/// px space-before to the final blank page consumed another.
///
/// **Falsified** by restoring the generic 1.2 line multiplier or adding
/// `paragraph.style.before` unconditionally: the page count rises above 34.
#[test]
fn docx_line_pitch_and_top_spacing_match_the_reference_pagination() {
    let rendered = render(
        &real_corpus("nist-hb133-2026-chapter-2.docx"),
        &Options {
            filename: Some("nist-hb133-2026-chapter-2.docx"),
            ..Options::default()
        },
    )
    .expect("the NIST chapter renders");
    assert_eq!(
        rendered.pages.len(),
        34,
        "LibreOffice paginates the NIST chapter as 34 pages"
    );
}

/// ODF span styles are deltas on the paragraph style.  The UK agreement's
/// default paragraph family is Arial and its automatic spans mostly change
/// size or weight; resolving each span from scratch reset the family to
/// Calibri/Carlito, narrowing lines and shifting later words by hundreds of
/// pixels.
///
/// **Falsified** by ignoring `style:default-style`: the title run reports
/// Carlito instead of the bundled Arial substitute.
#[test]
fn odt_span_styles_inherit_the_paragraph_font_family() {
    let rendered = render(
        &real_corpus("uk-ipo-one-way-nda.odt"),
        &Options {
            filename: Some("uk-ipo-one-way-nda.odt"),
            ..Options::default()
        },
    )
    .expect("the agreement renders");
    let title = rendered.pages[0]
        .items
        .iter()
        .find_map(|item| {
            let Item::Glyphs(run) = item else {
                return None;
            };
            run.text.starts_with("An Example").then_some(run)
        })
        .expect("the agreement title is present");
    assert_eq!(title.family, "Liberation Sans");
}

/// An empty ODF paragraph is a line box, not zero height.  The agreement has
/// one between its two-line title and date; omitting it leaves only about 20 px
/// between their baselines instead of the measured 37 px.
///
/// **Falsified** by ignoring self-closing `text:p` elements: the baseline gap
/// falls below 35 px.
#[test]
fn odt_empty_paragraphs_preserve_vertical_space() {
    let rendered = render(
        &real_corpus("uk-ipo-one-way-nda.odt"),
        &Options {
            filename: Some("uk-ipo-one-way-nda.odt"),
            ..Options::default()
        },
    )
    .expect("the agreement renders");
    let glyph = |prefix: &str| {
        rendered.pages[0]
            .items
            .iter()
            .find_map(|item| {
                let Item::Glyphs(run) = item else {
                    return None;
                };
                run.text.starts_with(prefix).then_some(run)
            })
            .unwrap_or_else(|| panic!("{prefix:?} is present"))
    };
    let baseline_gap = glyph("Date:").origin.y - glyph("One-way").origin.y;
    assert!(
        baseline_gap > 35.0,
        "the empty paragraph contributes its line box"
    );
}

/// DrawingML paragraphs and rich-text runs keep their own line boxes.  The
/// NASA cover has five author/date paragraphs in one shape; flattening them
/// put October beside Andrew near the top instead of at its 504 px baseline.
/// Its slide-number placeholder also changes index between master and layout,
/// so geometry inheritance must fall back to the placeholder type.
///
/// **Falsified** by restoring the single-line shape painter, changing the body
/// line factor back to 0.9, or matching placeholder geometry only by index:
/// October rises above 495 px or slide 7's number returns to x=52 px.
#[test]
fn pptx_preserves_rich_text_lines_and_type_inherited_placeholder_geometry() {
    let rendered = render(
        &real_corpus("nasa-agency-report-2022.pptx"),
        &Options {
            filename: Some("nasa-agency-report-2022.pptx"),
            ..Options::default()
        },
    )
    .expect("the NASA presentation renders");
    let glyph = |page: usize, prefix: &str| {
        rendered.pages[page]
            .items
            .iter()
            .filter_map(|item| {
                let Item::Group(group) = item else {
                    return None;
                };
                group.items.iter().find_map(|item| {
                    let Item::Glyphs(run) = item else {
                        return None;
                    };
                    run.text.starts_with(prefix).then_some(run)
                })
            })
            .next()
            .unwrap_or_else(|| panic!("{prefix:?} is present on page {}", page + 1))
    };
    assert!(glyph(0, "October").origin.y > 495.0);
    assert!(glyph(6, "7").origin.x > 1_170.0);
    assert!(glyph(6, "7").origin.y > 680.0);
}

/// PresentationML stores shapes in paint order.  It is not reading order: on
/// NASA slide 9 two page numbers and the footer precede the title in XML even
/// though LibreOffice exposes the title first and the footer/page number last.
///
/// **Falsified** by removing order_text_shapes: shape 4 follows shape 3.
#[test]
fn pptx_text_shapes_follow_visual_reading_order() {
    let rendered = render(
        &real_corpus("nasa-agency-report-2022.pptx"),
        &Options {
            filename: Some("nasa-agency-report-2022.pptx"),
            ..Options::default()
        },
    )
    .expect("the NASA presentation renders");
    let shapes = rendered.pages[8]
        .items
        .iter()
        .filter_map(|item| {
            let Item::Group(group) = item else {
                return None;
            };
            group
                .items
                .iter()
                .any(|item| matches!(item, Item::Glyphs(_)))
                .then(|| {
                    let Some(readany_render::SourceRef::Shape { shape, .. }) = group.source else {
                        panic!("a text-bearing slide group carries shape provenance");
                    };
                    shape
                })
        })
        .collect::<Vec<_>>();
    let position = |shape| {
        shapes
            .iter()
            .position(|candidate| *candidate == shape)
            .unwrap_or_else(|| panic!("shape {shape} is present"))
    };
    assert!(position(4) < position(3), "the title precedes the footer");
    assert!(
        position(3) < position(1),
        "the footer precedes the page number"
    );
}

/// Every item of a rendered document, flattened out of its groups.
fn flat_items(bytes: &[u8], filename: &str) -> Vec<Item> {
    let rendered = render(
        bytes,
        &Options {
            filename: Some(filename),
            ..Options::default()
        },
    )
    .unwrap_or_else(|error| panic!("{filename} renders: {error}"));
    fn walk(items: &[Item], out: &mut Vec<Item>) {
        for item in items {
            out.push(item.clone());
            if let Item::Group(group) = item {
                walk(&group.items, out);
            }
        }
    }
    let mut out = Vec::new();
    for page in &rendered.pages {
        walk(&page.items, &mut out);
    }
    out
}

fn item_source(item: &Item) -> Option<&readany_render::SourceRef> {
    match item {
        Item::Glyphs(run) => run.source.as_ref(),
        Item::Path(path) => path.source.as_ref(),
        Item::Image(image) => image.source.as_ref(),
        Item::Group(group) => group.source.as_ref(),
        _ => None,
    }
}

/// The address of the cell an item belongs to, if it belongs to one.
fn table_cell(item: &Item) -> Option<(usize, usize, usize)> {
    match item_source(item) {
        Some(readany_render::SourceRef::TableCell { table, row, column }) => {
            Some((*table, *row, *column))
        }
        _ => None,
    }
}

/// The text of the run addressed by one cell of the generated feature document.
fn feature_cell_text(table: usize, row: usize, column: usize) -> Option<String> {
    flat_items(&fixture("flow-features.docx"), "flow-features.docx")
        .iter()
        .find_map(|item| match (item, table_cell(item)) {
            (Item::Glyphs(run), Some(address)) if address == (table, row, column) => {
                Some(run.text.clone())
            }
            _ => None,
        })
}

/// A Word table cell is addressable the way a spreadsheet cell is: every glyph
/// and every rule inside the corpus chapter's tables carries a table, row and
/// column, and text outside a table still carries its paragraph.
///
/// **Falsified** by passing `None` as the cell to `paint_line` and dropping the
/// source from `edge_path`: the `TableCell` count falls to zero and the rules
/// go back to carrying nothing at all.
#[test]
fn every_glyph_and_rule_inside_a_word_table_is_addressed_by_its_cell() {
    let items = flat_items(
        &real_corpus("nist-hb133-2026-chapter-2.docx"),
        "nist-hb133-2026-chapter-2.docx",
    );
    let addressed = items
        .iter()
        .filter(|item| table_cell(item).is_some())
        .count();
    assert!(
        addressed > 400,
        "the chapter's tables carry hundreds of addressed items, not {addressed}"
    );

    // Every rule this document draws is a cell rule; there is no other source
    // of paths in a flow document. A rule without an address would highlight a
    // row's words and leave its box behind.
    let unaddressed_rules = items
        .iter()
        .filter(|item| matches!(item, Item::Path(_)) && table_cell(item).is_none())
        .count();
    assert_eq!(
        unaddressed_rules, 0,
        "every cell rule carries the address of the cell it bounds"
    );

    assert!(
        items.iter().any(|item| matches!(
            item_source(item),
            Some(readany_render::SourceRef::Text { .. })
        )),
        "text outside a table still reports its paragraph and character range"
    );
}

/// A cell spanning several columns reports the first one it covers, and the
/// cells beside it keep their own grid positions.
///
/// The generated table's grid is 2160 + 2880 twips; its header spans both
/// columns and the row below fills them separately.
///
/// **Falsified** by addressing a cell by its position in the row rather than by
/// `cell.column`: the right-hand cell of the second row reports column 1 either
/// way, but the sub-header rows of the corpus chapter — where a spanned cell
/// precedes single ones — then report one column too few.
#[test]
fn a_spanning_cell_reports_the_first_column_it_covers() {
    assert_eq!(
        feature_cell_text(0, 0, 0).as_deref(),
        Some("Spanning header"),
        "the header spans columns 0 and 1 and reports 0"
    );
    assert_eq!(feature_cell_text(0, 1, 0).as_deref(), Some("Left cell"));
    assert_eq!(feature_cell_text(0, 1, 1).as_deref(), Some("Right cell"));
    assert_eq!(
        feature_cell_text(0, 0, 1),
        None,
        "the spanned-over column is not an address of its own"
    );
}

/// A vertically merged cell is addressed by the row its merge began on, so both
/// of its boxes answer to one address.
///
/// **Falsified** by addressing a cell by the row it is drawn in rather than by
/// `origin_row`: rules appear at table 0 row 3 column 0, and a highlight of the
/// merged cell would light only half of it.
#[test]
fn a_vertically_merged_cell_is_addressed_by_the_row_its_merge_began_on() {
    let items = flat_items(&fixture("flow-features.docx"), "flow-features.docx");
    assert_eq!(
        feature_cell_text(0, 2, 0).as_deref(),
        Some("Merged label"),
        "the merge starts in row 2"
    );
    assert!(
        !items.iter().any(|item| table_cell(item) == Some((0, 3, 0))),
        "the continuation row has no cell of its own in column 0"
    );

    // The merged cell is drawn as two boxes on two rows; both carry row 2.
    let tops = items
        .iter()
        .filter(|item| table_cell(item) == Some((0, 2, 0)))
        .filter_map(|item| match item {
            Item::Path(path) => path
                .path
                .commands
                .first()
                .and_then(|command| match command {
                    readany_render::PathCommand::Move(point) => Some((point.y * 4.0) as i64),
                    _ => None,
                }),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert!(
        tops.len() >= 2,
        "the merged cell draws a box on each of its two rows, and both carry \
         row 2: {tops:?}"
    );
}

/// A picture inside a cell is addressed by that cell, not by the paragraph that
/// anchors it.
///
/// **Falsified** by giving an image `SourceRef::Text` unconditionally: the
/// picture in the fourth row reports a paragraph index and the pane beside it
/// cannot tell which row it belongs to.
#[test]
fn a_picture_inside_a_cell_is_addressed_by_that_cell() {
    let items = flat_items(&fixture("flow-features.docx"), "flow-features.docx");
    let addresses = items
        .iter()
        .filter(|item| matches!(item, Item::Image(_)))
        .map(table_cell)
        .collect::<Vec<_>>();
    assert!(
        addresses.contains(&Some((0, 4, 0))),
        "the picture in the table is addressed by its cell: {addresses:?}"
    );
    assert!(
        addresses.contains(&None),
        "the picture outside the table keeps its paragraph: {addresses:?}"
    );
}

/// Tables are numbered across the whole document, so a table in a header cannot
/// take the body's first index.
///
/// **Falsified** by resetting the counter for each part: the header's table
/// becomes table 0 and shares an address with the body's, so highlighting a
/// body row would light the header too.
#[test]
fn a_table_in_a_header_continues_the_documents_numbering() {
    assert_eq!(
        feature_cell_text(1, 0, 0).as_deref(),
        Some("Header table"),
        "the header's table is the document's second, not another first"
    );
    assert_eq!(
        feature_cell_text(0, 0, 0).as_deref(),
        Some("Spanning header"),
        "and the body's table keeps index zero"
    );
}

/// Every `SourceRef` variant appears in the hand-written WASM declarations with
/// the field names it actually serializes.
///
/// The `.d.ts` is copied over wasm-bindgen's generated one by
/// `scripts/build-wasm.sh` and is what the consuming app compiles against, so a
/// variant added here and forgotten there is a runtime surprise in a browser
/// rather than a compile error in this workspace. Nothing else in this
/// repository compares the two.
///
/// **Falsified** by deleting the `TableCell` line from
/// `crates/readany-render-wasm/readany_render_wasm.d.ts`: this test names the
/// missing member.
#[test]
fn every_source_ref_variant_is_declared_for_the_wasm_boundary() {
    let declarations = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../readany-render-wasm/readany_render_wasm.d.ts"),
    )
    .expect("the checked-in WASM declarations are readable");
    let collapsed = declarations
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for source in [
        readany_render::SourceRef::Cell {
            sheet: 0,
            row: 0,
            column: 0,
        },
        readany_render::SourceRef::Text {
            paragraph: 0,
            start: 0,
            end: 0,
        },
        readany_render::SourceRef::Shape { slide: 0, shape: 0 },
        readany_render::SourceRef::TableCell {
            table: 0,
            row: 0,
            column: 0,
        },
    ] {
        let serde_json::Value::Object(fields) =
            serde_json::to_value(&source).expect("a source reference serializes")
        else {
            panic!("a source reference serializes as an object");
        };
        let kind = fields
            .get("kind")
            .and_then(|value| value.as_str())
            .expect("the tag is named kind")
            .to_owned();
        // Serialization orders the fields alphabetically and the declarations
        // read in declaration order, so the two are compared as sets.
        let tag = format!("kind: \"{kind}\"");
        let member = collapsed
            .split_once(&tag)
            .map(|(_, rest)| {
                rest.split_once('}')
                    .map(|(member, _)| member)
                    .unwrap_or(rest)
            })
            .unwrap_or_else(|| panic!("readany_render_wasm.d.ts declares no `{tag}` member"));
        for (name, value) in fields.iter().filter(|(name, _)| name.as_str() != "kind") {
            let ts = match value {
                serde_json::Value::Number(_) => "number",
                serde_json::Value::String(_) => "string",
                other => panic!("{kind}.{name} serializes as an unhandled {other:?}"),
            };
            let field = format!("{name}: {ts}");
            assert!(
                member.contains(&field),
                "the `{kind}` member of readany_render_wasm.d.ts is missing `{field}`"
            );
        }
    }
}
