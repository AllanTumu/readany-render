# Implementation report

Measured on 12 August 2026 on an Apple M4 Pro MacBook Pro (14 cores, 24 GB,
macOS 26.5.1) with Rust 1.97.1. This report supersedes the earlier acceptance
report: its spreadsheet fidelity figures were invalid because incomparable
canvases were padded with white pixels.

## Rendering status

All locally rendered inputs converge on the same deterministic display list.
XLSM uses the XLSX renderer while reporting macros. PDF and HEIC return an
explicit host-delegate signal. PNG, JPEG, GIF, BMP, and WebP produce one image
page, and JPEG honours EXIF orientation.

| Family | Status | Current visual evidence |
| --- | --- | --- |
| XLSX / XLSM | Natural-sheet display list plus bounded viewport raster/item APIs; cached values, geometry, merges, gridline view settings, opt-in headers, frozen-pane pixel extents, explicit styles, number formats, and omissions covered | Exhaustive exact cell text 1.000000 on both real sheets; text fidelity 0.978372 and 0.967207 across five distributed viewports |
| ODS | Natural-sheet display list and viewport APIs; ODF styles, repeats, spans, gridline settings, opt-in headers, frozen-pane pixel extents, and explicit cell paint covered | Parser/golden/raster tests; no honest real ODS visual score currently claimed |
| CSV / TSV | Encoding and separator sniffing plus RFC 4180 quoting | Parser/raster tests |
| DOCX | Style cascade, shaping, pagination, lists, table paint, repeating parts, and images | Real NIST chapter: 0.365007 exact text, 610.30 px p95, and 37 pages versus 34; materially unfaithful |
| ODT | Named/automatic styles, page geometry, lists, spans, and images | Real UK IPO agreement: 0.148148 exact text, 864.96 px p95, and 3 pages versus 2; materially unfaithful |
| RTF | Scoped token stream, destinations, codepages, Unicode fallback, and formatting | Synthetic `basic.rtf` only: 1.000000 exact text and 8.07 px p95 |
| PPTX | Relationships, layout/master placeholder geometry, shapes, connectors, autofit, and images | Real NASA deck: all 11 slides, but 0.778898 exact text, 373.57 px p95, and two named unsupported EMFs; materially unfaithful |
| ODP | Masters, named styles, explicit geometry, shapes, autofit, and images | Synthetic `basic.odp` only: 1.000000 exact text and 4.97 px p95 |
| PDF / HEIC | Deliberate `DelegateToHost`; no misleading partial page | Official CFPB sample statement pins the PDF contract |
| PNG / JPEG / GIF / BMP / WebP | Single inspectable image page with pixel ceiling and orientation | Photographed receipt pixel diagnostic is exact |

The page-shaped text-fidelity mean is **0.457618**. The real spreadsheet mean is
**0.972789**. Exact cell text is **1.000000** for both Endo and OakPrism; sampled
mean / p95 box errors are 0.26 / 0.27 px and 0.45 / 3.27 px. See
`docs/FIDELITY.md` for the scoring definition and sampling boundary.

## Spreadsheet viewport path

Rust exposes `rasterise_rect(page, rect, scale)` and `items_in_rect(page, rect)`.
The browser `open()` API retains the full `Rendered` value inside WASM and
exposes `Document.pageInfo`, `Document.itemsInRect`,
`Document.renderRectRgba`, and `Document.renderRectToCanvas`. A consumer no
longer serializes a hundreds-of-megabytes display list or allocates the full
natural sheet merely to draw the visible pane.

XLSX and ODS gridlines default on and honour file visibility and colour settings.
They are emitted before fills and explicit borders, so document paint wins at a
shared edge. `Options::sheet_headers` adds row and column furniture without
changing the default display list; the bijective label sequence is exhaustively
pinned from A through ZZZ. Frozen row/column counts now carry their clamped pixel
width and height, which lets a viewport repaint the body and each frozen axis
independently.

The full Endo and generated-wide sheet rasters still correctly fail above the
100-million-pixel safety ceiling. Their 1,200 x 800 viewports succeed.

## Size

| Build | Measured gzip size | Budget | Result |
| --- | ---: | ---: | --- |
| Core WASM, no bundled fonts | 1,583,532 bytes | 4,194,304 bytes | pass |
| WASM with bundled fonts | 5,610,140 bytes | 9,437,184 bytes | pass |

## Performance

| Gate | Measured | Budget | Result |
| --- | ---: | ---: | --- |
| Generated 400 x 350 XLSX to display list | 439 ms | 500 ms | pass |
| Real Endo workbook to display list | 241 ms | 500 ms | pass |
| Real Endo 1,200 x 800 viewport raster | 41 ms | 100 ms | pass |
| 100-page DOCX to display list | 1 ms | 3,000 ms | pass |
| One small page raster | <1 ms | 100 ms | pass |

The Endo gate is a committed real input. XLSX row-height derivation is one pass,
and repeated non-numeric shaping is cached for the duration of a render. The
performance suite runs serially so scheduler contention is not mistaken for a
parser regression.

## Fidelity harness corrections

1. Release gating compares display-list words and boxes with LibreOffice PDF
   bbox or HTML DOM boxes. Spreadsheet exact text compares every non-empty cell
   in the full sheets by `SourceRef::Cell`; geometry samples five distributed
   viewports at both 96 and 192 dpi.
2. A median registration offset is removed before local geometry drift is
   measured. CI prints source-aware largest drifts, mean error, and p95 error.
3. Pixel SSIM searches plus or minus 3 px and reports both ink densities, but is
   diagnostic only. Images differing by more than 2% are still rejected.
4. The sparse synthetic page corpus no longer carries unsupported 0.95/0.90
   absolute SSIM floors. Text-geometry regression is gated against the committed
   baseline with a 0.001 extractor-rounding allowance.
5. The corpus contains the motivating Endo workbook, a second 1 MiB stress
   workbook, a photographed receipt fixture, a 34-page NIST DOCX chapter, an
   11-slide NASA PPTX, a two-page UK IPO ODT agreement, and an official public
   sample statement. The statement is used to verify deliberate PDF delegation
   rather than manufacturing a PDF fidelity score inside a non-PDF renderer.
6. Spreadsheet publication additionally requires at least 99% exhaustive exact
   cell text and p95 sampled geometry drift no greater than 4 px. This hard bar
   runs even during an explicit baseline update.
7. Every page document has a measured hard floor for exact text, p95 geometry,
   and pagination. These run before the `--update` branch. The low real-document
   floors preserve evidence against regression; they are explicitly not
   publication-readiness targets.

## Incomplete-content evidence

Every public `Unrendered` variant is pinned by a generated or real input:
charts, pivots, conditional formatting, hidden sheets, uncached formulae,
external references, unsupported glyphs, unsupported slide media, OLE, macros,
host delegation, and limit truncation. Strict mode is separately pinned to stable error code
`RR-0302`, so partial content cannot masquerade as a complete render.

## Dependency correction

The advance specification selected `rustybuzz` and `ttf-parser`; the policy
gate reported them as unmaintained under RUSTSEC-2026-0206 and
RUSTSEC-2026-0192. HarfRust and Skrifa are the maintained pure-Rust, WASM-safe
equivalents. Text shaping still uses real glyph clusters and positioned glyphs.
