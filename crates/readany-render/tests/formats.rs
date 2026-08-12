use readany_render::{
    Format, Item, Limits, Options, Rect, RenderErrorCode, SvgOptions, Unrendered, items_in_rect,
    rasterise, rasterise_rect, render, to_svg,
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
    let rendered = render(
        &real_corpus("oakprism-stress-v3.xlsx"),
        &Options {
            filename: Some("oakprism-stress-v3.xlsx"),
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
    assert!(
        rendered
            .pages
            .iter()
            .flat_map(|page| &page.items)
            .filter(|item| matches!(item, Item::Path(_)))
            .count()
            >= 2,
        "both table cells have explicit inspectable borders"
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
