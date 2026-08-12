use readany_render::{Options, Rect, rasterise_rect, render};
use std::path::PathBuf;
use std::time::Instant;

#[test]
fn four_hundred_by_three_hundred_fifty_sheet_meets_the_committed_budget() {
    if cfg!(debug_assertions) {
        return;
    }
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/wide.xlsx");
    let bytes =
        std::fs::read(path).unwrap_or_else(|error| panic!("wide fixture is missing: {error}"));
    let started = Instant::now();
    let rendered = render(&bytes, &Options::default())
        .unwrap_or_else(|error| panic!("wide fixture failed: {error}"));
    let elapsed = started.elapsed();
    assert_eq!(rendered.pages.len(), 1);
    assert!(
        elapsed.as_millis() < 500,
        "wide sheet took {} ms, over the 500 ms budget",
        elapsed.as_millis()
    );
    eprintln!("wide_sheet_ms={}", elapsed.as_millis());
}

#[test]
fn motivating_real_workbook_meets_the_display_list_and_viewport_budgets() {
    if cfg!(debug_assertions) {
        return;
    }
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/real/endo-prem-2023.xlsx");
    let bytes = std::fs::read(path)
        .unwrap_or_else(|error| panic!("real workbook corpus is missing: {error}"));
    let started = Instant::now();
    let rendered = render(
        &bytes,
        &Options {
            filename: Some("endo-prem-2023.xlsx"),
            ..Options::default()
        },
    )
    .expect("the motivating real workbook renders");
    let render_elapsed = started.elapsed();
    assert!(
        render_elapsed.as_millis() < 500,
        "real workbook took {} ms, over the 500 ms budget",
        render_elapsed.as_millis()
    );
    let viewport_started = Instant::now();
    let viewport = rasterise_rect(
        &rendered.pages[0],
        Rect {
            x: 0.0,
            y: 0.0,
            width: 1_200.0,
            height: 800.0,
        },
        1.0,
    )
    .expect("a screen-sized viewport rasterises");
    let viewport_elapsed = viewport_started.elapsed();
    assert_eq!((viewport.width, viewport.height), (1_200, 800));
    assert!(
        viewport_elapsed.as_millis() < 100,
        "real workbook viewport took {} ms, over the 100 ms budget",
        viewport_elapsed.as_millis()
    );
    eprintln!(
        "real_workbook_ms={} real_viewport_ms={}",
        render_elapsed.as_millis(),
        viewport_elapsed.as_millis()
    );
}

#[test]
fn one_page_raster_meets_the_committed_budget() {
    if cfg!(debug_assertions) {
        return;
    }
    let bytes = b"Name,Amount\nAlice,12\nBob,42\n";
    let rendered = render(bytes, &Options::default())
        .unwrap_or_else(|error| panic!("CSV render failed: {error}"));
    let started = Instant::now();
    let pixmap = readany_render::rasterise(&rendered.pages[0], 1.0)
        .unwrap_or_else(|error| panic!("raster failed: {error}"));
    let elapsed = started.elapsed();
    assert!(!pixmap.data.is_empty());
    assert!(
        elapsed.as_millis() < 100,
        "raster took {} ms",
        elapsed.as_millis()
    );
    eprintln!("one_page_raster_ms={}", elapsed.as_millis());
}

#[test]
fn one_hundred_page_docx_meets_the_committed_budget() {
    if cfg!(debug_assertions) {
        return;
    }
    let bytes = include_bytes!("../../../fixtures/hundred-pages.docx");
    let start = std::time::Instant::now();
    let rendered = readany_render::render(
        bytes,
        &Options {
            filename: Some("hundred-pages.docx"),
            ..Options::default()
        },
    )
    .expect("the generated performance document is valid");
    let elapsed = start.elapsed();
    assert_eq!(rendered.pages.len(), 100);
    assert!(
        elapsed < std::time::Duration::from_secs(3),
        "100-page DOCX exceeded 3 seconds: {:?}",
        elapsed
    );
    eprintln!("hundred_page_docx_ms={}", elapsed.as_millis());
}
