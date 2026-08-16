# Changelog

All notable changes follow [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `SourceRef::TableCell { table, row, column }`, so a cell of a table in a flow
  document is addressable the way a spreadsheet cell is. Every glyph, rule,
  shading fill and picture inside a Word table cell carries it; text outside a
  table still reports `Text`. Table indices run in document order across the
  whole document — body, then notes, then headers and footers — so a table split
  across a page boundary keeps one identity and a header's table cannot take the
  body's first index. Rows and columns are grid positions: a `w:gridSpan` cell
  reports the first column it covers and a `w:vMerge` continuation the row its
  merge began on.

  On `nist-hb133-2026-chapter-2.docx`, 515 items now carry an address against
  zero before. This is an additive variant: `Cell`, `Text` and `Shape` mean
  exactly what they meant, and `readany_render_wasm.d.ts` carries the new
  member.

  **ODT is not included.** The OpenDocument parser does not build tables at all
  — a `table:table`'s cells flow as ordinary stacked paragraphs — so there is no
  ODT table layout to label. Nested tables are in the same position: a `w:tbl`
  inside a `w:tc` has its paragraphs flowed into the enclosing cell, so its
  content reports that enclosing cell.

### Fixed

- An image anchored to a paragraph that holds nothing else is placed against
  that paragraph instead of falling back to the top of page one. Layout records
  where each paragraph came to rest rather than being asked afterwards to find a
  glyph carrying its source — a paragraph holding only a drawing has no glyph to
  find. Both images in the NIST chapter were affected.

## [0.1.2] - 2026-08-16

### Fixed

- **Word tables lay out as columns.** They previously rendered as flowing text
  with no cell borders and no column positions, which was worse than the
  markdown fallback a consumer would otherwise use. Table columns now agree
  with LibreOffice to 0.2 px on the real corpus chapter.
- Tabs are placed where the document puts them, and a tab with nothing after it
  no longer widens the line it ends.
- PPTX shapes are placed through their groups, and rotated shapes are rotated.
  A slide run with no size takes the one its paragraph declares.

### Measured

Against LibreOffice, on the real corpus:

| document | exact text | geometry | p95 |
| --- | ---: | ---: | ---: |
| `uk-ipo-one-way-nda.odt` | 1.0000 | 0.9533 | 1.73 px |
| `nasa-agency-report-2022.pptx` | 0.9473 | 0.3719 | 177.55 px |
| `nist-hb133-2026-chapter-2.docx` | 0.8549 | 0.2756 | 347.53 px |

Spreadsheets are unchanged at 1.0000 exact, p95 0.27 px and 3.27 px.

**ODT is the second proven format.** DOCX now reads and tabulates correctly and
still drifts vertically down the page; PPTX places plainly-stated shapes and
still misplaces grouped and tabular frames. Neither is finished, and the status
table in `README.md` says so per format.

### Fixed

- **Word tables are laid out as columns.** A table row was flattened into one
  tab-separated paragraph, so a cell that wrapped restarted at the table's left
  edge and dragged every later column with it, and the row took its alignment
  from whichever cell happened to be first. Each cell is now laid out in its own
  column box, the row is as tall as its tallest cell, and `w:tblBorders`,
  `w:tcBorders`, `w:gridSpan`, `w:vMerge`, `w:vAlign` and `w:tblCellMar` are
  read. Rules are drawn only where the document declares them; a bare `w:tbl` is
  borderless and used to be drawn with a black rectangle round every cell.
- **Tab stops carry their alignment.** `w:tab w:val="right"` says the text ends
  at the stop. Stops are measured from the text margin rather than the paragraph
  indent, and the indent is itself an implicit stop.
- **Line height no longer has an 11 pt floor**, which rounded every smaller line
  up and drifted 10 pt contents down the page.
- **Multi-level list labels.** `w:lvlText` is substituted at every level, and
  `w:startOverride` and `w:suff` are honoured.
- **`w:vanish` text is not rendered.** Hidden paragraphs were being drawn.
- **Headers and footers sit at the `w:header` and `w:footer` offsets** the
  section declares rather than at a fixed 24 px.
- **PPTX group shapes.** `p:grpSp` was not read, so every child of every group
  was placed at its raw offset instead of being mapped through
  `a:chOff`/`a:chExt`.
- **PPTX shape rotation.** `a:xfrm rot` reaches the display list.
- **PPTX run styles inherit.** A run without `sz` now takes its paragraph's
  `a:defRPr`, then the shape body's `a:lstStyle`, instead of a generic 18 pt.
- **A word is measured without the space after it** when slide text wraps, so a
  line ending at the box edge keeps its last word.

Measured against the committed baseline, real corpus only; every synthetic
fixture and both private workbooks are unchanged to the last recorded digit.

| Document | Geometry | Exact text | p95 error |
| --- | ---: | ---: | ---: |
| `uk-ipo-one-way-nda.odt` | 0.8006 → **0.9533** | 1.0000 → 1.0000 | 35.29 → **1.73 px** |
| `nasa-agency-report-2022.pptx` | 0.2527 → **0.3719** | 0.9452 → 0.9473 | 237.07 → **177.55 px** |
| `nist-hb133-2026-chapter-2.docx` | 0.2536 → **0.2756** | 0.8030 → **0.8549** | 561.48 → **347.53 px** |

## [0.1.1] - 2026-08-13

### Changed

- The two private corpus workbooks are described by shape rather than by name.
  Their filenames disclosed what the data was — "endo" with PREM/PROM together
  say endometriosis patient-reported outcome measures — which is the one thing
  keeping the files out of the repository was meant to avoid saying. A filename
  is disclosure even when the file is absent. They are now `sheet-a.xlsx` and
  `sheet-b.xlsx`, 393 rows by 328 columns and the wider of the two.
- The status table reports ODT as proven: 1.0000 exact text, 0.8006 geometry,
  35.29 px p95, and pagination matching the reference. DOCX and PPTX moved from
  broken to poor rather than to fixed, and say so.

### Fixed

- Flow and slide placement. DOCX paginates 34 pages against 34, exact text
  0.3650 to 0.8030. ODT geometry 0.1116 to 0.8006. PPTX reading order and rich
  text placement, exact text 0.7789 to 0.9452.
- A page break before the first block no longer opens a blank page.

No published artefact contained the corpus at 0.1.0 or at any version; this
release changes documentation and layout, not what ships in the package.

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
