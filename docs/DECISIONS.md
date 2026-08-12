# Decisions

## Display list before pixels

All formats converge on `Page` and `Item`. This preserves provenance, makes
serialization deterministic, and prevents format-specific raster backends.

## PDF and HEIC delegate

Hosts already provide PDF viewers; a competitive PDF implementation would
dominate this library. HEIC lacks a suitable small portable decoder. Both
boundaries are explicit `Unrendered` values rather than blank pages.

## Formula values are cached values

A viewer must not recalculate a workbook. Missing caches are reported at their
cell source rather than guessed.

## Natural sheets are viewport-rendered

A worksheet remains one natural, scrollable canvas; it is not silently
repaginated using guessed print settings. `rasterise_rect` allocates only the
requested viewport and culls unrelated display-list items. `items_in_rect`
returns the provenance-bearing subset needed for hit testing. In WASM, `open`
keeps the full document behind an opaque handle so it does not cross the
JavaScript serialization boundary.

## LibreOffice is a reference

The harness uses LibreOffice for repeatability, but it does not redefine the
source format. A measured, documented producer disagreement may justify keeping
the library behavior rather than matching the reference.

Spreadsheet references use LibreOffice HTML rendered at the same natural-sheet
viewport. LibreOffice PDF output is a print-layout reference and is therefore
not comparable with a scrollable canvas.

## Incomparable images are not scored

The fidelity harness rejects a width or height difference above 2% before any
padding or similarity calculation. Small raster rounding can still be aligned;
a tiny sheet and a print page cannot produce a score dominated by shared white
space.

## Fidelity is measured from positioned text

Sparse white canvases make scalar pixel similarity depend more on ink density
than layout correctness. Release gating therefore compares display-list words
and their boxes with `pdftotext -bbox` for page documents and Chromium DOM boxes
for natural sheets. Sheet matches include cell provenance. A median translation
is removed before residual geometry is scored, and failures name the source and
coordinate drift. Pixel SSIM remains on the contact sheet, searches plus or
minus 3 px for registration, and is always reported with both ink densities.

## Font libraries changed from the advance specification

The specification named rustybuzz and ttf-parser. Both became formally
unmaintained in 2026, producing RUSTSEC-2026-0206 and RUSTSEC-2026-0192 at the
release policy gate. HarfRust and Skrifa are their maintained project-backed
successors and retain the pure-Rust, WASM-safe design.

## Reference extraction has a measured tolerance

The text-geometry no-regression comparison allows 0.001 for bbox and DOM
rounding. There is no absolute SSIM release floor: the current sparse page
corpus cannot justify one. Pixel changes are reviewed in the contact sheet;
semantic text and geometry changes are gated against their committed baselines.
