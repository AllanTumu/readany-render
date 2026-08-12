# Fidelity

LibreOffice 26.2 is the committed office reference, not truth. Page documents
are exported to PDF and inspected with `pdftotext -bbox`. Natural spreadsheets
are exported to HTML; Chromium supplies cell text, word boxes, and pixels. The
harness measures five distributed 1,200 x 800 sheet viewports at 96 and 192 dpi.

## Release evidence

The release gate is based on semantic text and geometry, not blank raster area.
For spreadsheets, exact text is exhaustive: every non-empty cell in each full
sheet is compared by `SourceRef::Cell`. Geometry is sampled at the four sheet
corners and centre. This covers about 1.08% of Endo and 2.05% of OakPrism, and
the report states that boundary rather than presenting it as full-sheet layout
coverage. Page display-list `GlyphRun`s are split into positioned words and
matched against PDF bounding-box text.

`exact` is the F1 score `2 * matched / (ours + reference)`. The median global
translation is removed before local geometry is measured. Each matched word
then contributes `exp(-error / 12)`, where error includes centre and box-size
drift. `text fidelity` is `exact * geometry`. The report includes translation,
mean error, p95 error, and source-aware largest drifts.

The publish bars are executable before the `--update` branch: updating a
baseline cannot waive them. Every real sheet must have at least **99%
exhaustive exact cell text** and sampled text-box **p95 error no greater than 4
px**. Page-document floors are document-specific because the evidence is not
uniform; they preserve the measured starting point but do not turn a poor
starting point into a fidelity claim.

Measured 13 August 2026:

| Corpus document | Exact | Geometry | Text fidelity | Mean / p95 error |
| --- | ---: | ---: | ---: | ---: |
| `basic.docx` | 1.000000 | 0.959565 | 0.959565 | 0.50 / 1.16 px |
| `basic.odp` | 1.000000 | 0.715689 | 0.715689 | 4.05 / 4.97 px |
| `basic.odt` | 1.000000 | 0.607502 | 0.607502 | 6.90 / 12.35 px |
| `basic.pptx` | 1.000000 | 0.682948 | 0.682948 | 4.61 / 5.48 px |
| `basic.rtf` | 1.000000 | 0.717480 | 0.717480 | 4.30 / 8.07 px |
| `endo-prem-2023.xlsx` | 1.000000 | 0.978372 | 0.978372 | 0.26 / 0.27 px |
| `nasa-agency-report-2022.pptx` | 0.945187 | 0.252692 | 0.238842 | 70.09 / 237.07 px |
| `nist-hb133-2026-chapter-2.docx` | 0.802986 | 0.253570 | 0.203613 | 117.97 / 561.48 px |
| `oakprism-stress-v3.xlsx` | 1.000000 | 0.967207 | 0.967207 | 0.45 / 3.27 px |
| `uk-ipo-one-way-nda.odt` | 1.000000 | 0.800640 | 0.800640 | 13.16 / 35.29 px |

The page-document text-fidelity mean is **0.615785** and the real-sheet mean is
**0.972789**. These remain regression evidence, not proof of universal format
fidelity. CI rejects per-document or corpus regression beyond 0.001, requires
baseline keys to match the corpus, and enforces both sets of hard floors.

### What the page-aligned figures do and do not say

**The 12 August starting measurements corrected a misleading impression.** The
`exact` column above is computed **page by page**. When pagination diverges,
every later page is compared against a reference page holding different content,
so one early page break drags the whole score down. Read alone, `0.148` on the
UK IPO agreement suggests the reader recovered a seventh of the text. It did
not. It recovered all of it.

Comparing the same documents **document-wide** — whitespace collapsed, Symbol
private-use bullet glyphs dropped, Unicode NFKC-normalised, page boundaries
ignored — against the same LibreOffice reference:

| Corpus document | Page-aligned `exact` | Characters ours / reference | Document-wide similarity |
| --- | ---: | ---: | ---: |
| `uk-ipo-one-way-nda.odt` | 0.148148 → **1.000000** | 3,171 / 3,171 | **1.000000** |
| `nist-hb133-2026-chapter-2.docx` | 0.365007 → **0.802986** | 65,997 / 65,843 | **0.897400** |
| `nasa-agency-report-2022.pptx` | 0.778898 → **0.945187** | 4,034 / 4,037 | 0.679200 at the starting z-order |

Three different facts, and the single number hid all three:

* **ODT is character-identical.** Suppressing a break before the first block
  first restored 2/2 pagination. Applying the ODF default paragraph style,
  inheriting span deltas, and retaining self-closing paragraphs then raised
  geometry from **0.111628 to 0.800640** and reduced p95 from **452.10 to
  35.29 px**.
* **DOCX reads within 0.2% of the reference character count** and agrees on
  ~90% of the document-wide sequence. Correct paragraph spacing, line pitch,
  top-of-page spacing, and header/footer selection restored **34/34 pages**.
  Geometry moved with that pagination fix, from **0.017430 to 0.253570**;
  p95 fell from **610.30 to 561.48 px**, so pagination was a major cause but
  not the only remaining DOCX placement error.
* **PPTX reads within 0.07% of the character count** but agrees on only 68% of
  the starting document-wide sequence. Rich-text paragraphs are now laid out
  as lines and text-bearing shapes use visual reading order rather than XML
  paint order. Geometry rose from **0.098206 to 0.252692**, p95 fell from
  **373.57 to 237.07 px**, and the slide count remains 11/11.

So two questions were being answered by one number, and they have different
answers:

| Question | ODT | DOCX | PPTX |
| --- | --- | --- | --- |
| Was the text read? | completely | essentially completely | completely |
| Was it placed correctly? | materially improved; residual drift remains | pagination fixed; residual drift remains | ordering fixed; residual drift remains |

**Neither figure is retracted and neither is promoted.** The page-aligned score
remains the release gate, because placement is part of fidelity and a renderer
that puts the right words on the wrong page has not rendered the document. What
changes is that it is no longer reported as though it measured reading. A README
that said "36.5% exact text" and nothing else would have understated this
library to the point of dishonesty in the other direction.

### Page-document evidence floors

Each row shows the measurement followed by the rounded hard floor derived from
it. The floor is checked for exact text, p95 geometry error, and page-count
agreement. `pagination` is `min(ours, reference) / max(ours, reference)`.

| Corpus document | Measured exact / minimum | Measured p95 / maximum | Pages ours / reference | Pagination / minimum |
| --- | ---: | ---: | ---: | ---: |
| `basic.docx` | 1.000000 / 0.99 | 1.16 / 6.50 px | 1 / 1 | 1.000000 / 1.00 |
| `basic.odt` | 1.000000 / 0.99 | 12.35 / 12.50 px | 1 / 1 | 1.000000 / 1.00 |
| `basic.rtf` | 1.000000 / 0.99 | 8.07 / 8.25 px | 1 / 1 | 1.000000 / 1.00 |
| `basic.pptx` | 1.000000 / 0.99 | 5.48 / 5.60 px | 1 / 1 | 1.000000 / 1.00 |
| `basic.odp` | 1.000000 / 0.99 | 4.97 / 5.10 px | 1 / 1 | 1.000000 / 1.00 |
| `nist-hb133-2026-chapter-2.docx` | 0.802986 / 0.80 | 561.48 / 562.00 px | 34 / 34 | 1.000000 / 1.00 |
| `nasa-agency-report-2022.pptx` | 0.945187 / 0.87 | 237.07 / 238.00 px | 11 / 11 | 1.000000 / 1.00 |
| `uk-ipo-one-way-nda.odt` | 1.000000 / 0.99 | 35.29 / 36.00 px | 2 / 2 | 1.000000 / 1.00 |

The real corpus still answers questions the synthetic fixtures cannot. ODT and
DOCX now paginate exactly, and all three repaired formats materially improve
their geometry, but the remaining 35.29, 561.48, and 237.07 px p95 errors are
not publication-readiness claims. PPTX also retains explicit unsupported EMF
evidence rather than silently dropping those assets.

Only pages present on both sides contribute text and pixel measurements; extra
or missing pages are measured separately by the pagination score. This avoids
pretending an unmatched page has usable geometry while making the pagination
defect impossible to hide.

## Pixel diagnostics

Pixels remain useful for the contact sheet and investigation, but do not gate a
release. The harness reports the best local 8 x 8-window SSIM over every integer
translation in a plus or minus 3 px search and reports ink density for both
images. A dimension difference above 2% is a hard failure before comparison.

| Corpus document | Aligned SSIM | Ink, ours / reference |
| --- | ---: | ---: |
| `basic.docx` | 0.999202 | 0.1859% / 0.1944% |
| `basic.odp` | 0.985421 | 8.7667% / 8.8467% |
| `basic.odt` | 0.993403 | 0.1425% / 0.1992% |
| `basic.pptx` | 0.997029 | 0.1138% / 0.1360% |
| `basic.rtf` | 0.995884 | 0.1133% / 0.1370% |
| `endo-prem-2023.xlsx` | 0.733993 | 14.8375% / 6.7182% |
| `nasa-agency-report-2022.pptx` | 0.754455 | 44.3381% / 78.3646% |
| `nist-hb133-2026-chapter-2.docx` | 0.679422 | 6.5714% / 7.0554% |
| `oakprism-stress-v3.xlsx` | 0.677122 | 21.6146% / 13.9774% |
| `receipt.jpg` | 1.000000 | 95.6258% / 95.6258% |
| `uk-ipo-one-way-nda.odt` | 0.934531 | 6.3837% / 6.7372% |

The old XLSX/ODS scores of 0.987544 and 0.995099 are withdrawn because white
padding dominated them. The later unaligned sheet mean of 0.473395 is also not
fidelity evidence. Likewise, approximately 0.99 scores on sparse page fixtures
are pixel diagnostics only: their low ink density cannot prove layout fidelity.

### Gridline before/after diagnostic

The missing-gridline hypothesis was measured rather than assumed. Before sheet
gridlines were implemented, our/reference ink was 5.9187%/6.7182% on Endo and
12.0678%/13.9774% on OakPrism: relative gaps of 11.9% and 13.7%. With the
workbook's default-on gridlines painted, the same figures became
14.8375%/6.7182% and 21.6146%/13.9774%. That deliberately recorded result shows
the reference HTML export does not preserve Calc's interactive grid furniture;
its old ink gap was not a valid way to tune gridline darkness. Grid visibility,
indexed colour, z-order, and raster output are therefore pinned structurally
and visually, while pixel ink remains a diagnostic rather than a release gate.
