# Changelog

All notable changes follow [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

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

### Changed

- Spreadsheet cells paint only source-defined fills and borders; application UI
  gridlines are no longer invented as document content.
- Excel `General` numbers are formatted from their numeric value, column width
  conversion matches the producer raster more closely, and repeated shaping is
  cached within a render.
- Fidelity gates display-list text identity and box geometry against PDF bbox or
  source-aware sheet DOM boxes. Pixel SSIM searches plus or minus 3 px, reports
  ink density, rejects incomparable dimensions, and remains diagnostic only.

### Removed

- Withdrawn synthetic XLSX/ODS SSIM claims that were dominated by white padding
  between natural-sheet and print-page canvases.
- Removed absolute SSIM release floors and the 0.473395 sheet score as fidelity
  claims; sparse white canvases cannot support either conclusion.
