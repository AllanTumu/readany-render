# Changelog

All notable changes follow [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Deterministic display-list model with cell, paragraph, and shape provenance.
- XLSX, XLSM, ODS, CSV, TSV, DOCX, ODT, RTF, PPTX, ODP, and image parsers.
- Explicit PDF and HEIC delegation and exhaustive incomplete-content reporting.
- Native PNG/SVG backends, CLI, WebAssembly bindings, and handwritten TypeScript model.
- Hostile-input limits, generated fixtures, fuzz targets, performance/size gates,
  and a LibreOffice fidelity harness.
- DOCX and ODT style inheritance, pagination controls, lists, tables, repeating
  parts, and embedded images; scoped codepage-aware RTF parsing.
- ODS style and geometry resolution plus PPTX/ODP layout/master inheritance,
  connectors, preset shapes, autofit, and embedded-image relationships.
- Regular, bold, italic, and bold-italic metric-compatible bundled faces with
  all required font licence texts in both Rust and npm packages.
- Bounded `rasterise_rect` and `items_in_rect` Rust APIs plus an opaque WASM
  `Document` handle for viewport pixels, page metadata, and visible items.
- Real-workbook performance and natural-sheet fidelity corpora, a photographed
  receipt image, and an official sample statement delegation contract.
- Default-on XLSX/ODS gridlines with file visibility and colour, opt-in row and
  column headers, and frozen-pane pixel extents for independently painted views.
- Licensed real-document evidence: a 34-page NIST DOCX chapter, an 11-slide NASA
  PPTX, and a two-page UK IPO ODT agreement, with source URLs and checksums.
- Non-waivable exact-text, p95-geometry, and pagination floors for every flow and
  slide corpus document.
- Explicit `UnsupportedMedia` evidence for SVG, EMF, and WMF slide assets rather
  than rejecting the whole deck or silently omitting them.

### Changed

- Spreadsheet gridlines are modelled as source view furniture beneath cell fills
  and explicit borders rather than being confused with source-defined borders.
- Excel `General` numbers are formatted from their numeric value, column width
  conversion matches the producer raster more closely, and repeated shaping is
  cached within a render.
- Fidelity gates display-list text identity and box geometry against PDF bbox or
  source-aware sheet DOM boxes. Pixel SSIM searches plus or minus 3 px, reports
  ink density, rejects incomparable dimensions, and remains diagnostic only.
- Real-sheet exact text now covers every non-empty cell, geometry samples five
  distributed viewports, and a non-waivable 99% exact / 4 px p95 publish bar is
  enforced. Reference fonts use the same metric-compatible substitutions.
- XLSX respects declared default row heights and inherited base font properties,
  preserves XML comparison characters, and formats `General` values to Excel's
  15-significant-digit display precision without binary floating-point drift.
- XML character references are retained across DOCX, ODT, ODS, PPTX, and ODP
  text paths instead of being silently dropped by event parsing.
- Page-document fidelity now measures every page shared with the reference and
  records extra or missing pages independently instead of assuming page counts
  match.
- DOCX paragraph pitch, boundary spacing, and referenced odd/even repeating
  parts now reproduce the real NIST chapter's 34-page pagination.
- ODT applies default paragraph styles to named styles and span deltas, and
  retains self-closing paragraphs as visible line boxes.
- PPTX preserves rich-text paragraphs and runs, resolves remapped placeholders
  by index and type, and orders text-bearing shapes for visual reading.

### Removed

- Withdrawn synthetic XLSX/ODS SSIM claims that were dominated by white padding
  between natural-sheet and print-page canvases.
- Removed absolute SSIM release floors and the 0.473395 sheet score as fidelity
  claims; sparse white canvases cannot support either conclusion.
- Removed uniform format-support wording: each README claim now identifies its
  real, synthetic, parser-only, or host-delegated evidence.

[Unreleased]: https://github.com/AllanTumu/readany-render/commits/main
